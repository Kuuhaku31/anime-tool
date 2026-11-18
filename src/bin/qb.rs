
//! src/bin/qb.rs
//! 从 qBittorrent 同步种子和文件信息, 并维护 PostgreSQL 文件索引.

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use postgres::Client;
use reqwest::blocking::Client as HttpClient;
use serde_json::Value;
use url::Url;

#[path = "../kg/utils.rs"]
mod utils;

use utils::{FILE_BUFFER_SIZE, hash_file};

// 几个哈希类型定义
type Sha256Data = [u8; 32];
type Md5Data    = [u8; 16];
type Sha1Data   = [u8; 20];

/// 限制单次数据库请求的记录数
const DB_BATCH_SIZE: usize = 1000;


/// qBittorrent 中一个种子及其文件列表.
struct QbTorrent {
    hash_v1  : Sha1Data,         // 固定长度的 v1 info hash
    title    : String,           // qBittorrent 显示的种子名称
    category : PathBuf,          // qBittorrent 中的分类路径
    contents : Vec<u8>,          // 导出的原始 torrent 文件内容

    // 种子中的文件列表
    // 第一个 PathBuf 为相对于 qBittorrent 保存目录的路径
    // 第二个为文件在运行本程序的系统中的绝对路径
    files : Vec<(PathBuf, PathBuf)>,
}


/// 第一阶段数据库操作返回的文件记录.
struct FileRecord {
    torrent_hash_v1   : Sha1Data, // 所属种子的 v1 info hash
    relative_path     : PathBuf,  // 种子中的相对路径
    absolute_path     : PathBuf,  // 用于查询和读取文件的绝对路径

    file_sha256       : Option<Sha256Data>,   // 已有文件索引或新计算出的 SHA-256
    file_pre16mib_md5 : Option<Md5Data>,   // 新计算出的前 16 MiB MD5
    file_size         : Option<i64>,        // 新计算的文件大小, 单位为字节
    file_created_at   : Option<SystemTime>, // 文件系统提供的创建时间, 不支持时为空
}

/// 运行同步流程并在失败时输出错误上下文.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步失败: {error:#}");
        std::process::exit(1);
    }
}

/// 按 API 读取, 首次写库, 文件处理和二次写库的顺序执行同步.
fn run() -> Result<()> {

    // 加载配置
    let config = utils::EnvConfig::load(true)?.qb()?;

    // 打印配置信息
    println!("QB_URL: {}", config.qb_url);
    println!("QB_PORT: {}", config.qb_url.port()
        .map_or_else(|| "默认".into(), |p| p.to_string())
    );
    println!("QB_USERNAME: {}", config.qb_username);
    println!("QB_PASSWORD: ******");
    println!("PG_HOST: {}", config.pg_host);
    println!("PG_PORT: {}", config.pg_port
        .map_or_else(|| "默认".into(), |p| p.to_string())
    );
    println!("PG_USER: {}", config.pg_user);
    println!("PG_PASSWORD: ******");
    println!("PG_DB: {}", config.pg_db);

    // 创建 HTTP 客户端并登录 qBittorrent.
    let http = {
        let client = HttpClient::builder()
        .cookie_store(true).build()
        .context("创建 HTTP 客户端失败")?;

        let response = client
        .post(api_url(&config.qb_url, "auth/login")?)
        .form(&[
            ("username", &config.qb_username),
            ("password", &config.qb_password),
        ]).send()
        .context("请求 qBittorrent 登录失败")?.error_for_status()
        .context("qBittorrent 登录返回错误状态")?;
        let status = response.status();
        let body = response.text()
        .context("读取 qBittorrent 登录结果失败")?;

        // 兼容新版的 204 空响应和旧版的 200 Ok.
        if status != reqwest::StatusCode::NO_CONTENT
        && !(status == reqwest::StatusCode::OK && body.trim() == "Ok.")
        { bail!("qBittorrent 登录被拒绝, HTTP 状态: {status}"); }

        client
    };

    // 获取种子列表
    let torrents = fetch_torrents(&http, &config.qb_url)?;
    println!("获取种子数量: {}", torrents.len());

    // 配置 PostgreSQL 连接并初始化数据库客户端
    let mut db = utils::connect_postgres(
        &config.pg_host, config.pg_port, &config.pg_user, &config.pg_password, &config.pg_db,
    ).context("连接 PostgreSQL 失败")?;

    // 执行第一阶段数据库操作, 获取文件记录
    let records = transaction_1(&mut db, &torrents)?;

    // 处理文件记录, 计算哈希值并分类为新文件或已索引文件
    let mut new_files     = Vec::new();
    let mut indexed_files = Vec::new();
    let mut skipped       = 0usize;

    // 创建多进度条显示器
    let display  = MultiProgress::new();
    let progress = display.add(progress_bar(records.len(), "处理文件")?);
    let bytes    = display.add(progress_bar(0, "文件哈希")?);
    bytes.set_style(ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] {msg} [{bar:30.cyan/blue}] {bytes}/{total_bytes} {bytes_per_sec} ETA {eta}",
    )?);

    // 所有文件复用同一缓冲区, 保持顺序读取且避免重复分配内存
    let mut buffer = vec![0u8; FILE_BUFFER_SIZE];
    for mut record in records {

        // 如果文件已经有 SHA256, 直接归类为已索引文件
        if record.file_sha256.is_some() { indexed_files.push(record); }

        // 否则计算文件哈希并归类为新文件
        else { match hash_file(&record.absolute_path, &mut buffer, &bytes) {
            Ok((sha256, md5, size, created_at)) => {
                record.file_sha256       = Some(sha256);
                record.file_pre16mib_md5 = Some(md5);
                record.file_size         = Some(size);
                record.file_created_at   = created_at;
                new_files.push(record);
            }
            Err(error) => {
                // 输出跳过的文件信息, 并增加跳过计数
                progress.suspend(||
                    eprintln!("跳过文件 {}: {error:#}", record.absolute_path.display())
                );
                skipped += 1;
            }
        }}
        progress.inc(1);
    }
    bytes.finish_and_clear();
    progress.finish_with_message("文件处理完成");

    transaction_2(&mut db, &new_files, &indexed_files)?;

    println!(
        "同步完成: 新索引 {} 个, 已有索引 {} 个, 跳过 {} 个文件",
        new_files.len(), indexed_files.len(), skipped
    );
    Ok(())
}

/// 获取所有种子及每个种子的文件列表和原始 torrent 内容.
fn fetch_torrents(client: &HttpClient, base: &Url) -> Result<Vec<QbTorrent>> {

    let progress = progress_bar(0, "获取种子列表")?;
    // 获取种子列表
    let values: Vec<Value> = client
    .get(api_url(base, "torrents/info")?).send()
    .context("获取种子列表失败")?.error_for_status()
    .context("种子列表返回错误状态")?.json()
    .context("解析种子列表失败")?;
    progress.set_length(values.len() as u64);
    progress.set_message("获取种子文件列表和 torrent 内容");

    // 对于每个种子, 进一步获取其详细信息和文件列表
    let mut torrents = Vec::with_capacity(values.len());
    for value in values {

        // 依次解析: 种子 v1 hash, 标题和分类
        let hash_text = json_string(&value, "infohash_v1")?;
        let hash_v1: [u8; 20] = hex::decode(hash_text)
        .context("解析种子 v1 hash 失败")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("种子 v1 hash 必须为 20 字节: {hash_text}"))?;
        let title = json_string(&value, "name")?.to_owned();
        let category = PathBuf::from(json_string(&value, "category")?);

        // 获取种子文件列表
        let files_json: Vec<Value> = client
        .get(api_url(base, "torrents/files")?)
        .query(&[("hash", hash_text)]).send()
        .with_context(|| format!("获取种子 {hash_text} 的文件列表失败"))?.error_for_status()
        .with_context(|| format!("种子 {hash_text} 的文件列表返回错误状态"))?.json()
        .with_context(|| format!("解析种子 {hash_text} 的文件列表失败"))?;

        // 单文件种子的 content_path 是文件路径, 多文件种子的 content_path 是目录.
        let is_single_file = files_json.len() == 1;
        let base_path = if is_single_file {
            let content_path = json_string(&value, "content_path")?;
            if content_path.is_empty() {
                bail!("种子 {hash_text} 的 content_path 为空");
            }
            PathBuf::from(content_path)
        } else { PathBuf::from(json_string(&value, "save_path")?) };

        // 保留种子相对路径原文, 仅在拼接多文件种子的绝对路径时转换分隔符.
        let mut files = Vec::with_capacity(files_json.len());
        for file in files_json {

            let relative_path = PathBuf::from(json_string(&file, "name")?);
            let path: &Path = &relative_path;
            let text = path.to_str().context("文件路径不是有效的 UTF-8")?;
            if text.is_empty()
            || text.contains('\\')
            || text.split('/').any(|part| part.is_empty() || part == "." || part == "..")
            || path.components().any(|part| !matches!(part, Component::Normal(_)))
            { bail!("无效的种子相对路径: {text}"); }

            // 正确处理不同操作系统的路径分隔符
            let absolute_path = if is_single_file { base_path.clone() }
            else { base_path.join(text.replace('/', &std::path::MAIN_SEPARATOR.to_string())) };

            files.push((relative_path, absolute_path));
        }

        // 获取种子文件本体
        let contents = client
        .get(api_url(base, "torrents/export")?)
        .query(&[("hash", hash_text)]).send()
        .with_context(|| format!("导出种子 {hash_text} 失败"))?.error_for_status()
        .with_context(|| format!("导出种子 {hash_text} 返回错误状态"))?.bytes()
        .with_context(|| format!("读取种子 {hash_text} 内容失败"))?.to_vec();
        if contents.is_empty() { bail!("导出的种子 {hash_text} 内容为空"); }

        // 完成一个种子的处理
        torrents.push(QbTorrent {hash_v1, title, category, contents, files});
        progress.inc(1);
    }
    progress.finish_with_message("种子信息获取完成");

    // 反回处理后的种子列表
    Ok(torrents)
}

/// 在同一事务中重建 in_qb, 更新种子和文件, 并读取已有路径索引.
fn transaction_1(db: &mut Client, torrents: &[QbTorrent]) -> Result<Vec<FileRecord>> {

    let file_count: usize = torrents.iter().map(|torrent| torrent.files.len()).sum();
    let progress = progress_bar(torrents.len() * 2 + file_count * 2, "数据库阶段 1")?;
    let mut tx = db.transaction().context("开始第一阶段事务失败")?;

    // 准备 SQL 语句
    let ins_in_qb_stmt = tx.prepare(utils::sql!("qb/insert_in_qb.sql"))?;
    let ins_torrent_stmt = tx.prepare(utils::sql!("qb/upsert_torrent.sql"))?;
    let ins_file_stmt = tx.prepare(utils::sql!("qb/insert_torrent_files.sql"))?;
    let sel_path_stmt = tx.prepare(utils::sql!("qb/select_path_hashes.sql"))?;

    // 清空 in_qb 表
    tx.execute(utils::sql!("qb/clear_in_qb.sql"), &[]).context("清空 in_qb 失败")?;

    // 遍历每个种子, 更新 in_qb, torrent 和 torrent_file 表, 并收集文件记录 
    let mut records = Vec::with_capacity(file_count);
    for torrent in torrents {

        let hash_v1 = torrent.hash_v1.as_slice();

        // 插入 in_qb, 插入更新 torrent 表
        tx.execute(&ins_in_qb_stmt, &[&hash_v1]).context("写入 in_qb 失败")?;
        tx.execute(&ins_torrent_stmt, &[
            &hash_v1,
            &torrent.title,
            &path_text(&torrent.category)?,
            &torrent.contents,
        ]).context("更新 torrent 失败")?;
        progress.inc(2);

        // 每批插入文件并查询路径索引, 依靠序号保持返回记录与原始文件顺序一致.
        for chunk in torrent.files.chunks(DB_BATCH_SIZE) {

            let relatives: Vec<_> = chunk.iter().map(|file| path_text(&file.0)).collect::<Result<_>>()?;
            let absolutes: Vec<_> = chunk.iter().map(|file| path_text(&file.1)).collect::<Result<_>>()?;
            tx.execute(&ins_file_stmt, &[&hash_v1, &relatives]).context("更新 torrent_file 失败")?;
            progress.inc(chunk.len() as u64);
            let rows = tx.query(&sel_path_stmt, &[&absolutes]).context("查询 path_file 失败")?;
            for (file, row) in chunk.iter().zip(rows) {
                let file_sha256 = match row.try_get::<_, Option<Vec<u8>>>(0)
                .context("读取 path_file 的 SHA-256 失败")? {
                    Some(bytes) => {
                        Some(bytes.try_into().map_err(|_| {
                            anyhow::anyhow!("path_file 的 SHA-256 长度无效: {}", file.1.display())
                        })?)
                    }
                    None => None,
                };

                records.push(FileRecord {
                    torrent_hash_v1 : torrent.hash_v1,
                    relative_path   : file.0.clone(),
                    absolute_path   : file.1.clone(),

                    file_sha256,
                    file_pre16mib_md5 : None,
                    file_size         : None,
                    file_created_at   : None,
                });
            }
            progress.inc(chunk.len() as u64);
        }
    }

    // 提交第一阶段事务
    progress.set_message("提交数据库阶段 1");
    tx.commit().context("提交第一阶段事务失败")?;
    progress.finish_with_message("数据库阶段 1 完成");
    Ok(records)
}

/// 在同一事务中更新文件索引, 路径索引及所有种子文件的 SHA-256.
fn transaction_2(db: &mut Client, new_files: &[FileRecord], indexed_files: &[FileRecord]) -> Result<()> {

    let progress = progress_bar(new_files.len() * 3 + indexed_files.len(), "数据库阶段 2")?;
    let mut tx = db.transaction().context("开始第二阶段事务失败")?;

    let file_stmt = tx.prepare(utils::sql!("qb/upsert_files.sql"))?;
    let path_stmt = tx.prepare(utils::sql!("qb/upsert_paths.sql"))?;
    let torrent_file_stmt = tx.prepare(utils::sql!("qb/update_torrent_file_hashes.sql"))?;

    // file 插入 file_sha256 -> (file_pre16mib_md5, file_size)
    for chunk in new_files.chunks(DB_BATCH_SIZE) {
        let hashes: Vec<_> = chunk.iter().map(|record| record.file_sha256.as_ref()
        .map(|hash| hash.as_slice()).context("新文件缺少 SHA-256")).collect::<Result<_>>()?;
        let md5s: Vec<_> = chunk.iter().map(|record| record.file_pre16mib_md5.as_ref()
        .map(|hash| hash.as_slice()).context("新文件缺少 MD5")).collect::<Result<_>>()?;
        let sizes: Vec<_> = chunk.iter().map(|record| record.file_size.context("新文件缺少文件大小"))
        .collect::<Result<_>>()?;

        tx.execute(&file_stmt, &[&hashes, &md5s, &sizes])
        .context("更新 file 失败")?;
        progress.inc(chunk.len() as u64);
    }

    // path_file 插入 absolute_path -> (file_sha256, file_created_at, <当前时间>)
    for chunk in new_files.chunks(DB_BATCH_SIZE) {
        let paths: Vec<_> = chunk.iter().map(|record| path_text(&record.absolute_path)).collect::<Result<_>>()?;
        let hashes: Vec<_> = chunk.iter().map(|record| record.file_sha256.as_ref()
        .map(|hash| hash.as_slice()).context("新文件缺少 SHA-256")).collect::<Result<_>>()?;
        let times: Vec<_> = chunk.iter().map(|record| record.file_created_at).collect();
        tx.execute(&path_stmt, &[&paths, &hashes, &times])
        .context("更新 path_file 失败")?;
        progress.inc(chunk.len() as u64);
    }

    // torrent_file 插入 (torrent_hash_v1, relative_path) -> file_sha256
    let mut records = new_files.iter().chain(indexed_files);
    loop {
        let chunk: Vec<_> = records.by_ref().take(DB_BATCH_SIZE).collect();
        if chunk.is_empty() { break; }
        let torrent_hashes: Vec<_> = chunk.iter().map(|record| record.torrent_hash_v1.as_slice()).collect();
        let paths: Vec<_> = chunk.iter().map(|record| path_text(&record.relative_path)).collect::<Result<_>>()?;
        let hashes: Vec<_> = chunk.iter().map(|record| record.file_sha256.as_ref()
        .map(|hash| hash.as_slice()).context("文件缺少 SHA-256")).collect::<Result<_>>()?;
        if let Some(row) = tx.query_opt(&torrent_file_stmt, &[&torrent_hashes, &paths, &hashes])
        .context("更新 torrent_file 的 SHA-256 失败")?
        {
            let path: String = row.try_get(0)?;
            bail!("更新 torrent_file 时记录不存在: {path}");
        }
        progress.inc(chunk.len() as u64);
    }

    progress.set_message("提交数据库阶段 2");
    tx.commit().context("提交第二阶段事务失败")?;
    progress.finish_with_message("数据库阶段 2 完成");
    Ok(())
}



/// 创建限频刷新的进度条, 等待期间持续显示耗时, 出错退出时保留中断位置.
fn progress_bar(length: usize, message: &'static str) -> Result<ProgressBar> {
    utils::progress_bar(
        length,
        "{spinner:.green} [{elapsed_precise}] {msg} [{bar:30.cyan/blue}] {pos}/{len} ETA {eta}",
        Some(message),
    )
}

/// 基于配置地址构建 API 地址.
fn api_url(base: &Url, endpoint: &str) -> Result<Url> {
    let mut url = base.clone();
    url.set_query(None);
    url.set_fragment(None);
    let path = format!("{}/api/v2/{endpoint}", base.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

/// 从 JSON 对象读取必需的字符串字段.
fn json_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value.get(key).and_then(Value::as_str)
    .with_context(|| format!("qBittorrent 响应缺少字符串字段 {key}"))
}

/// 将路径转换为数据库使用的 UTF-8 文本.
fn path_text(path: &Path) -> Result<&str> {
    path.to_str().with_context(|| format!("路径不是有效的 UTF-8: {}", path.display()))
}
