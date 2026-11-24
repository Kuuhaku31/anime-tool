
//! src/bin/set-ids.rs
//! 读取单列 Bangumi ID 文本, 在单个事务中覆盖 PostgreSQL main 数据库的 ids 表.

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
    /// 输入文件, 每行仅包含一个正整数 bgm_ani_id.
    input: PathBuf,
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
        "COPY public.ids (bgm_ani_id) FROM STDIN",
    ).context("开始批量写入 ids 表失败")?;
    for row in &rows {
        // 仅输出经过校验的整数, 无需处理文本转义.
        writeln!(writer, "{row}")
            .context("发送 ID 数据失败")?;
    }
    let count = writer.finish().context("批量写入 ids 表失败")?;
    tx.commit().context("提交导入事务失败")?;
    println!("已用 {} 覆盖 main.public.ids, 写入 {count} 行", cli.input.display());
    Ok(())
}

/// 解析单列 Bangumi ID, 拒绝旧多列格式并保留输入顺序和重复记录.
fn parse_rows(text: &str) -> Result<Vec<i32>> {
    let mut rows = Vec::new();
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') { continue; }
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns.len() != 1 {
            bail!(
                "第 {} 行必须仅包含一列 bgm_ani_id, ids 已不再保存 mikan_rss_id 和 ddp_ani_id",
                index + 1
            );
        }
        let bgm: i32 = columns[0].parse()
            .with_context(|| format!("第 {} 行的 bgm_ani_id 不是有效的 32 位整数: {:?}", index + 1, columns[0]))?;
        if bgm <= 0 {
            bail!("第 {} 行的 bgm_ani_id 必须大于 0: {bgm}", index + 1);
        }
        rows.push(bgm);
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
