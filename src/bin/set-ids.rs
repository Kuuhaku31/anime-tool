
//! src/bin/set-ids.rs
//! 读取三列 ID 文本, 在单个事务中覆盖 PostgreSQL main 数据库的 ids 表.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use postgres::{Client, NoTls};

/// 导入命令的参数, 数据库名称固定为 main.
#[derive(Parser)]
#[command(name = "set-ids", version, about = "用文本文件覆盖 PostgreSQL main.public.ids 表")]
struct Cli {
    /// 输入文件, 列顺序为 bgm_id mikan_id ddp_id; Tab 分隔时保留空列.
    input: PathBuf,
}

/// 一行经校验的 ID, 保持输入顺序和重复记录.
struct IdRow {
    bgm: i32,
    mikan: Option<i32>,
    ddp: Option<i32>,
}

/// 执行导入并在失败时打印完整错误链, 返回非零退出码.
fn main() {
    if let Err(error) = run() {
        eprintln!("导入 ID 失败: {error:#}");
        std::process::exit(1);
    }
}

/// 先完整解析文件, 再连接 main 数据库并原子替换 ids 表内容.
fn run() -> Result<()> {
    let cli = Cli::parse();
    let text = fs::read_to_string(&cli.input).with_context(|| format!("读取输入文件失败: {}", cli.input.display()))?;
    let rows = parse_rows(&text)?;

    match dotenvy::from_path(".env") {
        Ok(_) => {}
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("加载当前目录 .env 失败"),
    }
    let mut db = connect_postgres()?;
    let mut tx = db.transaction().context("开始导入事务失败")?;

    // 串行化同表的写入, 防止并发导入在清空后混入另一份文件的数据.
    // 不删除表结构, 清空或 COPY 失败时旧数据随事务一起回滚.
    tx.batch_execute("LOCK TABLE public.ids IN EXCLUSIVE MODE; DELETE FROM public.ids;")
        .context("锁定并清空 ids 表失败")?;
    let mut writer = tx.copy_in(
        "COPY public.ids (bgm_ani_id, mikan_rss_id, ddp_ani_id) FROM STDIN",
    ).context("开始批量写入 ids 表失败")?;
    for row in &rows {
        // 仅输出经过校验的整数和 COPY 的 NULL 标记, 无需处理文本转义.
        let mikan = row.mikan.map_or_else(|| "\\N".into(), |id| id.to_string());
        let ddp = row.ddp.map_or_else(|| "\\N".into(), |id| id.to_string());
        writeln!(writer, "{}\t{}\t{}", row.bgm, mikan, ddp)
            .context("发送 ID 数据失败")?;
    }
    let count = writer.finish().context("批量写入 ids 表失败")?;
    tx.commit().context("提交导入事务失败")?;
    println!("已用 {} 覆盖 main.public.ids, 写入 {count} 行", cli.input.display());
    Ok(())
}

/// 解析输入的 bgm, mikan, ddp 三列, 空行和整行注释不计入数据.
fn parse_rows(text: &str) -> Result<Vec<IdRow>> {

    /// 将非空字段解析为适合 PostgreSQL integer 的正整数, 空字段返回 None.
    fn parse_id(value: Option<&str>, line: usize, column: &str) -> Result<Option<i32>> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let id: i32 = value.parse()
            .with_context(|| format!("第 {line} 行的 {column} 不是有效的 32 位整数: {value:?}"))?;
        if id <= 0 {
            bail!("第 {line} 行的 {column} 必须大于 0: {id}");
        }
        Ok(Some(id))
    }

    let mut rows = Vec::new();
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') { continue; }
        // 不能先 trim 再按空白拆分, 否则中间或末尾缺失的列会发生错位.
        let columns: Vec<_> = if line.contains('\t') {
            line.split('\t').map(str::trim).collect()
        } else {
            line.split_whitespace().collect()
        };
        if columns.len() > 3 {
            bail!("第 {} 行超过三列, 期望 bgm_id mikan_id ddp_id", index + 1);
        }
        let bgm = parse_id(columns.first().copied(), index + 1, "bgm_id")?
        .with_context(|| format!("第 {} 行缺少 bgm_id", index + 1))?;
        rows.push(IdRow {
            bgm,
            mikan: parse_id(columns.get(1).copied(), index + 1, "mikan_id")?,
            ddp: parse_id(columns.get(2).copied(), index + 1, "ddp_id")?,
        });
    }
    Ok(rows)
}

/// 使用 PG_* 配置连接 main, 不读取 PG_DB, 避免误写其他数据库.
fn connect_postgres() -> Result<Client> {

    /// 读取必需配置, 已有进程环境变量优先于 .env.
    fn required_env(name: &str) -> Result<String> {
        let value = env::var(name).with_context(|| format!("请设置 {name}"))?;
        if value.is_empty() {
            bail!("{name} 不能为空");
        }
        Ok(value)
    }

    let mut config = postgres::Config::new();
    config.host(&required_env("PG_HOST")?)
        .user(&required_env("PG_USER")?)
        .password(required_env("PG_PASSWORD")?)
        .dbname("main");
    match env::var("PG_PORT") {
        Ok(value) => {
            let port: u16 = value.parse().context("PG_PORT 必须是有效端口")?;
            if port == 0 {
                bail!("PG_PORT 不能为 0");
            }
            config.port(port);
        }
        Err(env::VarError::NotPresent) => {}
        Err(error) => return Err(error).context("读取 PG_PORT 失败"),
    }
    config.connect(NoTls).context("连接 PostgreSQL main 数据库失败")
}
