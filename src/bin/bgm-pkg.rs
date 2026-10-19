
// src/bin/bgm-pkg.rs
// 按 Bangumi 动画 ID 获取 subject 与 episodes 数据包, 可选缓存为 JSON 文件, 并写入 SQLite 数据库

use std::{
    fs,
    path::{Path, PathBuf},
    thread::sleep,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use reqwest::blocking::Client;
use rusqlite::{Connection, params_from_iter, types::Value as SqlValue};
use serde_json::Value;

/// episodes 接口单页大小
const EPISODE_PAGE_SIZE: usize = 200;

/// 请求使用的 User-Agent
const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// 动画表建表语句
const CREATE_ANI_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS bangumi_anime (
    id INTEGER PRIMARY KEY,
    type INTEGER,
    name TEXT,
    name_cn TEXT,
    summary TEXT,
    series INTEGER,
    nsfw INTEGER,
    locked INTEGER,
    date TEXT,
    platform TEXT,
    volumes INTEGER,
    eps INTEGER,
    total_episodes INTEGER,
    rating_rank INTEGER,
    rating_total INTEGER,
    rating_score REAL,
    collection_wish INTEGER,
    collection_collect INTEGER,
    collection_doing INTEGER,
    collection_on_hold INTEGER,
    collection_dropped INTEGER,
    image_large TEXT,
    image_common TEXT,
    image_medium TEXT,
    image_small TEXT,
    image_grid TEXT,
    meta_tags TEXT,
    tags TEXT,
    official_site TEXT
)";

/// 剧集表建表语句
const CREATE_EPISODE_TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS bangumi_episode (
    id INTEGER PRIMARY KEY,
    subject_id INTEGER,
    type INTEGER,
    name TEXT,
    name_cn TEXT,
    sort REAL,
    ep REAL,
    airdate TEXT,
    comment INTEGER,
    duration TEXT,
    duration_seconds INTEGER,
    desc TEXT,
    disc INTEGER
)";

/// 动画表列名
const ANI_COLUMNS: &[&str] = &[
    "id",
    "type",
    "name",
    "name_cn",
    "summary",
    "series",
    "nsfw",
    "locked",
    "date",
    "platform",
    "volumes",
    "eps",
    "total_episodes",
    "rating_rank",
    "rating_total",
    "rating_score",
    "collection_wish",
    "collection_collect",
    "collection_doing",
    "collection_on_hold",
    "collection_dropped",
    "image_large",
    "image_common",
    "image_medium",
    "image_small",
    "image_grid",
    "meta_tags",
    "tags",
    "official_site",
];

/// 剧集表列名
const EPISODE_COLUMNS: &[&str] = &[
    "id",
    "subject_id",
    "type",
    "name",
    "name_cn",
    "sort",
    "ep",
    "airdate",
    "comment",
    "duration",
    "duration_seconds",
    "desc",
    "disc",
];

/// 命令行参数
#[derive(Parser, Debug)]
#[command(
    name = "bgm-pkg",
    version,
    about = "获取 Bangumi 动画数据包并写入 SQLite 数据库, 缓存目录可选",
    override_usage = "bgm-pkg [OPTIONS] --database <DATABASE> [IDS_FILE]\n       bgm-pkg set-id --database <DATABASE> [IDS_FILE]",
    after_help = "\
子命令:
  set-id    重建数据库的 bangumi_anime_ids 表并写入 ID
            bgm-pkg set-id -d <DATABASE> [IDS_FILE]
            IDS_FILE 省略时从 bangumi_anime 表读取 id; 详见 bgm-pkg set-id --help

示例:
  bgm-pkg -d ani.db ids.txt                  获取后直接写入数据库, 不落盘
  bgm-pkg -d ani.db -c cache ids.txt         下载到 cache 后导入数据库
  bgm-pkg -d ani.db                          读取 bangumi_anime_ids 表的 ID 获取并写入\n  bgm-pkg -d ani.db -c cache                 仅导入 cache 中已有的 JSON
  bgm-pkg set-id -d ani.db                   用已入库的动画 id 重建 bangumi_anime_ids"
)]
struct Cli {
    /// 目标 SQLite 数据库路径(不存在则创建)
    #[arg(short, long, value_name = "DATABASE")]
    database: PathBuf,

    /// Bangumi ID 输入文件, 每行一个, # 开头为注释; 省略时: 指定了 -c 则仅导入缓存中已有的 JSON, 否则读取数据库 bangumi_anime_ids 表
    ids_file: Option<PathBuf>,

    /// JSON 缓存目录; 省略时不落盘, 获取后直接写入数据库
    #[arg(short, long, value_name = "DIR")]
    cache_dir: Option<PathBuf>,

    /// 缓存文件已存在时重新获取并覆盖
    #[arg(long)]
    overwrite: bool,

    /// 请求间隔(秒)
    #[arg(long, value_name = "SECONDS", default_value_t = 0.2)]
    delay: f64,

    /// 请求超时(秒)
    #[arg(long, value_name = "SECONDS", default_value_t = 15)]
    timeout: u64,

    /// Bangumi API 地址
    #[arg(long, value_name = "URL", default_value = "https://api.bgm.tv")]
    api_base: String,
}

/// 运行统计
#[derive(Default)]
struct Stats {
    /// 成功获取或导入的数量
    success: usize,
    /// 因缓存存在而跳过下载的数量
    skipped: usize,
    /// 失败数量
    failed: usize,
    /// 写入的 episode 数量
    episodes: usize,
}

/// set-id 子命令参数
#[derive(Parser, Debug)]
#[command(name = "bgm-pkg set-id", override_usage = "bgm-pkg set-id --database <DATABASE> [IDS_FILE]", about = "将 ID 写入数据库的 bangumi_anime_ids 表(已存在则删表重建)")]
struct SetIdCli {
    /// 目标 SQLite 数据库路径
    #[arg(short, long)]
    database: PathBuf,

    /// 每行一个 ID 的输入文件; 省略时从数据库 bangumi_anime 表读取 id
    ids_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("set-id") {
        let rest = std::iter::once(args[0].clone()).chain(args[2..].iter().cloned());
        return set_id(SetIdCli::parse_from(rest));
    }
    let cli = Cli::parse();

    let mut connection = Connection::open(&cli.database)
        .with_context(|| format!("无法打开数据库: {}", cli.database.display()))?;

    // 未指定 ID 文件与缓存目录时, 从数据库 bangumi_anime_ids 表读取 ID
    let ids = match (&cli.ids_file, &cli.cache_dir) {
        (Some(path), _) => read_positive_ints(path)?,
        (None, Some(_)) => Vec::new(),
        (None, None) => read_db_ids(&connection)?,
    };
    if ids.is_empty() && cli.cache_dir.is_none() {
        bail!("没有可用的 Bangumi ID: 请指定 IDS_FILE, 或先用 set-id 写入 bangumi_anime_ids 表");
    }

    connection.execute(CREATE_ANI_TABLE_SQL, [])?;
    connection.execute(CREATE_EPISODE_TABLE_SQL, [])?;

    let mut stats = Stats::default();
    match &cli.cache_dir {
        Some(dir) => {
            fs::create_dir_all(dir)?;
            if !ids.is_empty() {
                download_to_cache(&cli, &ids, dir, &mut stats)?;
            }
            import_cache(&mut connection, dir, &mut stats)?;
        }
        None => fetch_to_database(&cli, &ids, &mut connection, &mut stats)?,
    }

    println!();
    println!("完成");
    println!("成功:         {}", stats.success);
    println!("已跳过:       {}", stats.skipped);
    println!("失败:         {}", stats.failed);
    println!("处理 episode: {}", stats.episodes);
    println!("数据库:       {}", cli.database.display());
    Ok(())
}

/// 执行 set-id: 重建 bangumi_anime_ids 表并写入 ID
fn set_id(cli: SetIdCli) -> Result<()> {
    let mut connection = Connection::open(&cli.database)
        .with_context(|| format!("无法打开数据库: {}", cli.database.display()))?;

    // 先读出全部 ID, 再删表, 避免数据来源被删除
    let mut ids = match &cli.ids_file {
        Some(path) => read_positive_ints(path)?,
        None => {
            let mut stmt = connection.prepare("SELECT id FROM bangumi_anime ORDER BY id")?;
            stmt.query_map([], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        }
    };
    ids.sort_unstable();
    ids.dedup();

    let tx = connection.transaction()?;
    tx.execute("DROP TABLE IF EXISTS bangumi_anime_ids", [])?;
    tx.execute("CREATE TABLE bangumi_anime_ids (id INTEGER PRIMARY KEY)", [])?;
    {
        let mut stmt = tx.prepare("INSERT INTO bangumi_anime_ids (id) VALUES (?)")?;
        for id in &ids {
            stmt.execute([id])?;
        }
    }
    tx.commit()?;

    println!("已写入 {} 个 ID 到 bangumi_anime_ids: {}", ids.len(), cli.database.display());
    Ok(())
}

/// 从 bangumi_anime_ids 表读取全部 ID
fn read_db_ids(connection: &Connection) -> Result<Vec<i64>> {
    let mut stmt = connection
        .prepare("SELECT id FROM bangumi_anime_ids ORDER BY id")
        .context("无法读取 bangumi_anime_ids 表, 请先执行 set-id")?;
    Ok(stmt
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 逐个获取数据包并保存到缓存目录, 已存在且未指定覆盖时跳过
fn download_to_cache(cli: &Cli, ids: &[i64], dir: &Path, stats: &mut Stats) -> Result<()> {
    let client = build_client(cli.timeout)?;
    for (index, id) in ids.iter().enumerate() {
        println!("[{}/{}] Bangumi subject ID = {id}", index + 1, ids.len());
        let file = dir.join(format!("{id}.json"));
        if file.exists() && !cli.overwrite {
            println!("  [SKIP] 文件已存在: {}", file.display());
            stats.skipped += 1;
            continue;
        }

        let result = fetch_subject(&client, cli, *id).and_then(|pkg| {
            let mut text = serde_json::to_string_pretty(&pkg)?;
            text.push('\n');
            fs::write(&file, text)?;
            Ok(pkg)
        });
        match result {
            Ok(pkg) => println!("  [OK] {} ({} episodes)", file.display(), pkg["episode_count"]),
            Err(err) => {
                eprintln!("  [ERROR] {err:#}");
                stats.failed += 1;
            }
        }
        pause(cli.delay, index + 1 < ids.len());
    }
    Ok(())
}

/// 不使用缓存, 获取后直接写入数据库
fn fetch_to_database(
    cli: &Cli,
    ids: &[i64],
    connection: &mut Connection,
    stats: &mut Stats,
) -> Result<()> {
    let client = build_client(cli.timeout)?;
    for (index, id) in ids.iter().enumerate() {
        println!("[{}/{}] Bangumi subject ID = {id}", index + 1, ids.len());
        let result = fetch_subject(&client, cli, *id).and_then(|pkg| import_package(connection, &pkg, "内存数据"));
        match result {
            Ok(count) => {
                println!("  [OK] anime={id}, episodes={count}");
                stats.success += 1;
                stats.episodes += count;
            }
            Err(err) => {
                eprintln!("  [ERROR] {err:#}");
                stats.failed += 1;
            }
        }
        pause(cli.delay, index + 1 < ids.len());
    }
    Ok(())
}

/// 将缓存目录中的全部 JSON 文件导入数据库
fn import_cache(connection: &mut Connection, dir: &Path, stats: &mut Stats) -> Result<()> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    if files.is_empty() {
        eprintln!("目录中没有 JSON 文件: {}", dir.display());
        return Ok(());
    }

    for (index, file) in files.iter().enumerate() {
        println!("[{}/{}] {}", index + 1, files.len(), file.file_name().unwrap_or_default().to_string_lossy());
        let result = fs::read_to_string(file)
            .map_err(anyhow::Error::from)
            .and_then(|text| Ok(serde_json::from_str::<Value>(&text)?))
            .and_then(|pkg| import_package(connection, &pkg, &file.display().to_string()));
        match result {
            Ok(count) => {
                println!("  [OK] episodes={count}");
                stats.success += 1;
                stats.episodes += count;
            }
            Err(err) => {
                eprintln!("  [ERROR] {err:#}");
                stats.failed += 1;
            }
        }
    }
    Ok(())
}

/// 在单个事务中写入一个数据包, 返回写入的 episode 数量
fn import_package(connection: &mut Connection, pkg: &Value, source: &str) -> Result<usize> {
    let (subject_id, anime_row) = extract_anime(pkg)?;
    let episode_rows = extract_episodes(pkg, subject_id, source)?;

    let tx = connection.transaction()?;
    tx.execute(
        &build_upsert_sql("bangumi_anime", ANI_COLUMNS, "id"),
        params_from_iter(anime_row),
    )?;
    {
        let mut stmt = tx.prepare(&build_upsert_sql("bangumi_episode", EPISODE_COLUMNS, "id"))?;
        for row in &episode_rows {
            stmt.execute(params_from_iter(row.iter()))?;
        }
    }
    tx.commit()?;
    Ok(episode_rows.len())
}

/// 获取 subject 信息与完整 episodes 列表
fn fetch_subject(client: &Client, cli: &Cli, id: i64) -> Result<Value> {
    println!("  获取 subject 信息...");
    let subject: Value = client
        .get(format!("{}/v0/subjects/{id}", cli.api_base))
        .send()?
        .error_for_status()?
        .json()?;
    if !subject.is_object() {
        bail!("subject API 返回的数据不是 JSON object");
    }
    pause(cli.delay, true);

    println!("  获取完整 episodes 列表...");
    let mut episodes: Vec<Value> = Vec::new();
    let mut offset = 0usize;
    loop {
        let data: Value = client
            .get(format!("{}/v0/episodes", cli.api_base))
            .query(&[
                ("subject_id", id.to_string()),
                ("limit", EPISODE_PAGE_SIZE.to_string()),
                ("offset", offset.to_string()),
            ])
            .send()?
            .error_for_status()?
            .json()?;
        if !data.is_object() {
            bail!("episodes API 返回的数据不是 JSON object: subject={id}");
        }
        let page = data
            .get("data")
            .and_then(Value::as_array)
            .with_context(|| format!("episodes.data 不是数组: subject={id}"))?;
        let page_len = page.len();
        episodes.extend(page.iter().cloned());

        let total = data.get("total").and_then(Value::as_u64);
        match total {
            Some(t) => println!("    episodes: {}/{t}", episodes.len()),
            None => println!("    episodes: {}", episodes.len()),
        }
        if page_len < EPISODE_PAGE_SIZE || total.is_some_and(|t| episodes.len() as u64 >= t) {
            break;
        }
        offset += page_len;
        pause(cli.delay, true);
    }

    let count = episodes.len();
    Ok(serde_json::json!({
        "subject": subject,
        "episodes": episodes,
        "episode_count": count,
    }))
}

/// 从数据包提取动画行, 返回 (subject_id, 行数据)
fn extract_anime(pkg: &Value) -> Result<(i64, Vec<SqlValue>)> {
    let subject = pkg
        .get("subject")
        .filter(|v| v.is_object())
        .context("缺少 subject 对象")?;
    let subject_id = to_int(subject.get("id")).context("subject.id 不存在或不是有效整数")?;

    let empty = Value::Null;
    let sub = |key: &str| subject.get(key).unwrap_or(&empty);
    let obj = |key: &str| subject.get(key).filter(|v| v.is_object()).unwrap_or(&empty);
    let rating = obj("rating");
    let collection = obj("collection");
    let images = obj("images");

    let official_site = subject
        .get("infobox")
        .and_then(Value::as_array)
        .and_then(|items| items.iter().find(|i| i.get("key").and_then(Value::as_str) == Some("官方网站")))
        .and_then(|i| i.get("value"))
        .filter(|v| v.is_string());

    let row = vec![
        SqlValue::Integer(subject_id),
        to_sql(sub("type")),
        to_sql(sub("name")),
        to_sql(sub("name_cn")),
        to_sql(sub("summary")),
        to_sql(sub("series")),
        to_sql(sub("nsfw")),
        to_sql(sub("locked")),
        to_sql(sub("date")),
        to_sql(sub("platform")),
        to_sql(sub("volumes")),
        to_sql(sub("eps")),
        to_sql(sub("total_episodes")),
        to_sql(rating.get("rank").unwrap_or(&empty)),
        to_sql(rating.get("total").unwrap_or(&empty)),
        to_sql(rating.get("score").unwrap_or(&empty)),
        to_sql(collection.get("wish").unwrap_or(&empty)),
        to_sql(collection.get("collect").unwrap_or(&empty)),
        to_sql(collection.get("doing").unwrap_or(&empty)),
        to_sql(collection.get("on_hold").unwrap_or(&empty)),
        to_sql(collection.get("dropped").unwrap_or(&empty)),
        to_sql(images.get("large").unwrap_or(&empty)),
        to_sql(images.get("common").unwrap_or(&empty)),
        to_sql(images.get("medium").unwrap_or(&empty)),
        to_sql(images.get("small").unwrap_or(&empty)),
        to_sql(images.get("grid").unwrap_or(&empty)),
        compact_json(sub("meta_tags")),
        compact_json(sub("tags")),
        to_sql(official_site.unwrap_or(&empty)),
    ];
    Ok((subject_id, row))
}

/// 从数据包提取剧集行, 无效条目会警告并跳过
fn extract_episodes(pkg: &Value, subject_id: i64, source: &str) -> Result<Vec<Vec<SqlValue>>> {
    let episodes = match pkg.get("episodes") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) => list,
        Some(_) => bail!("episodes 不是数组"),
    };

    let empty = Value::Null;
    let mut rows = Vec::new();
    for (index, episode) in episodes.iter().enumerate() {
        if !episode.is_object() {
            eprintln!("[WARN] {source}: episodes[{index}] 不是 object, 已跳过");
            continue;
        }
        let Some(episode_id) = to_int(episode.get("id")) else {
            eprintln!("[WARN] {source}: episodes[{index}] 缺少有效 id, 已跳过");
            continue;
        };
        let ep_subject = match episode.get("subject_id") {
            None => SqlValue::Integer(subject_id),
            Some(Value::Null) => SqlValue::Null,
            Some(v) => SqlValue::Integer(to_int(Some(v)).unwrap_or(subject_id)),
        };
        let field = |key: &str| to_sql(episode.get(key).unwrap_or(&empty));
        rows.push(vec![
            SqlValue::Integer(episode_id),
            ep_subject,
            field("type"),
            field("name"),
            field("name_cn"),
            field("sort"),
            field("ep"),
            field("airdate"),
            field("comment"),
            field("duration"),
            field("duration_seconds"),
            field("desc"),
            field("disc"),
        ]);
    }
    Ok(rows)
}

/// 读取输入文件中的正整数 ID, 忽略空行与注释, 无效行输出警告
fn read_positive_ints(path: &Path) -> Result<Vec<i64>> {
    let text = fs::read_to_string(path).with_context(|| format!("无法读取输入文件: {}", path.display()))?;
    let mut result = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.parse::<i64>() {
            Ok(v) if v > 0 => result.push(v),
            Ok(_) => eprintln!("[WARN] 第 {} 行 Bangumi ID 必须 > 0, 已跳过: {line:?}", index + 1),
            Err(_) => eprintln!("[WARN] 第 {} 行不是有效的 Bangumi ID, 已跳过: {line:?}", index + 1),
        }
    }
    Ok(result)
}

/// 构造 UPSERT 语句
fn build_upsert_sql(table: &str, columns: &[&str], primary_key: &str) -> String {
    let names = columns.join(", ");
    let placeholders = vec!["?"; columns.len()].join(", ");
    let updates = columns
        .iter()
        .filter(|c| **c != primary_key)
        .map(|c| format!("{c} = excluded.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO {table} ({names}) VALUES ({placeholders}) ON CONFLICT({primary_key}) DO UPDATE SET {updates}")
}

/// 创建 HTTP 客户端
fn build_client(timeout: u64) -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(timeout))
        .build()?)
}

/// 按需等待指定秒数
fn pause(delay: f64, enabled: bool) {
    if enabled && delay > 0.0 {
        sleep(Duration::from_secs_f64(delay));
    }
}

/// 将 JSON 值转为整数, 支持数字与数字字符串
fn to_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 将 JSON 值转为 SQLite 值, 数组与对象序列化为紧凑 JSON 文本
fn to_sql(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => SqlValue::Real(n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

/// 将 JSON 值序列化为紧凑 JSON 文本, null 对应 SQL NULL
fn compact_json(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        other => SqlValue::Text(other.to_string()),
    }
}
