
//! src/bin/qb.rs
//! 从 qBittorrent 同步种子和文件信息, 并维护 PostgreSQL 文件索引.

use std::env;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use md5::Md5;
use postgres::{Client, NoTls};
use reqwest::blocking::Client as HttpClient;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

// 几个哈希类型定义
type Sha256Data = [u8; 32];
type Md5Data    = [u8; 16];
type Sha1Data   = [u8; 20];

/// 计算文件前 16 MiB 的 MD5, 较短文件则计算全部内容
const MD5_PREFIX_BYTES: u64 = 16 * 1024 * 1024;
/// 限制单次数据库请求的记录数
const DB_BATCH_SIZE: usize = 1000;
/// 单次读取文件的缓冲区大小, 1 MiB
const FILE_BUFFER_SIZE: usize = 1024 * 1024;


/// 运行所需的 qBittorrent 和 PostgreSQL 连接配置.
struct Config {
    qb_url      : Url,         // qBittorrent Web API 的基础地址.
    qb_username : String,      // qBittorrent 登录用户名.
    qb_password : String,      // qBittorrent 登录密码.

    pg_host     : String,      // PostgreSQL 主机名.
    pg_port     : Option<u16>, // PostgreSQL 端口, 为空时使用驱动默认值.
    pg_user     : String,      // PostgreSQL 用户名.
    pg_password : String,      // PostgreSQL 密码.
    pg_db       : String,      // PostgreSQL 数据库名称.
}

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
    let config = load_config()?;

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
    let mut db = {
        let mut pg_config = postgres::Config::new();
        pg_config.host(&config.pg_host).user(&config.pg_user)
        .password(&config.pg_password).dbname(&config.pg_db);
        if let Some(port) = config.pg_port { pg_config.port(port); }
        pg_config.connect(NoTls).context("连接 PostgreSQL 失败")?
    };

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

        // 依次解析: 种子 v1 hash, 标题, 分类, 保存路径
        let hash_text = json_string(&value, "infohash_v1")?;
        let hash_v1: [u8; 20] = hex::decode(hash_text)
        .context("解析种子 v1 hash 失败")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("种子 v1 hash 必须为 20 字节: {hash_text}"))?;
        let title = json_string(&value, "name")?.to_owned();
        let category = PathBuf::from(json_string(&value, "category")?);
        let save_path = PathBuf::from(json_string(&value, "save_path")?);

        // 获取种子文件列表
        let files_json: Vec<Value> = client
        .get(api_url(base, "torrents/files")?)
        .query(&[("hash", hash_text)]).send()
        .with_context(|| format!("获取种子 {hash_text} 的文件列表失败"))?.error_for_status()
        .with_context(|| format!("种子 {hash_text} 的文件列表返回错误状态"))?.json()
        .with_context(|| format!("解析种子 {hash_text} 的文件列表失败"))?;

        // 解析种子文件列表, 并构建相对于保存目录的路径和系统中的绝对路径的元组列表
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

            let absolute_path = save_path.join(&relative_path);
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
    let ins_in_qb_stmt = tx.prepare("
        INSERT INTO public.in_qb (torrent_hash_v1)
        VALUES ($1) ON CONFLICT DO NOTHING
    ")?;
    let ins_torrent_stmt = tx.prepare("
        INSERT INTO public.torrent (hash_v1, title, category, contents)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (hash_v1) DO UPDATE SET
            title = EXCLUDED.title,
            category = EXCLUDED.category,
            contents = EXCLUDED.contents
    ")?;
    let ins_file_stmt = tx.prepare("
        INSERT INTO public.torrent_file (torrent_hash_v1, relative_path)
        SELECT $1, relative_path FROM UNNEST($2::TEXT[]) AS input(relative_path)
        ON CONFLICT (torrent_hash_v1, relative_path) DO NOTHING
    ")?;
    let sel_path_stmt = tx.prepare("
        SELECT pf.file_sha256
        FROM UNNEST($1::TEXT[]) WITH ORDINALITY AS input(absolute_path, ord)
        LEFT JOIN public.path_file AS pf ON pf.absolute_path = input.absolute_path
        ORDER BY input.ord
    ")?;

    // 清空 in_qb 表
    tx.execute("DELETE FROM public.in_qb", &[]).context("清空 in_qb 失败")?;

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

    let file_stmt = tx.prepare("
        INSERT INTO public.file (sha256, pre16mib_md5, byte_count)
        SELECT DISTINCT ON (sha256) sha256, md5, size
        FROM UNNEST($1::BYTEA[], $2::BYTEA[], $3::BIGINT[])
        WITH ORDINALITY AS input(sha256, md5, size, ord)
        ORDER BY sha256, ord DESC
        ON CONFLICT (sha256) DO UPDATE SET
            pre16mib_md5 = EXCLUDED.pre16mib_md5,
            byte_count   = EXCLUDED.byte_count
    ")?;
    let path_stmt = tx.prepare("
        INSERT INTO public.path_file (absolute_path, file_sha256, created_time, update_time)
        SELECT DISTINCT ON (path) path, sha256, created_at, CURRENT_TIMESTAMP
        FROM UNNEST($1::TEXT[], $2::BYTEA[], $3::TIMESTAMPTZ[])
        WITH ORDINALITY AS input(path, sha256, created_at, ord)
        ORDER BY path, ord DESC
        ON CONFLICT (absolute_path) DO UPDATE SET
            file_sha256  = EXCLUDED.file_sha256,
            created_time = EXCLUDED.created_time,
            update_time  = EXCLUDED.update_time
    ")?;
    let torrent_file_stmt = tx.prepare("
        WITH input AS (
            SELECT hash, path, sha256, ord
            FROM UNNEST($1::BYTEA[], $2::TEXT[], $3::BYTEA[])
            WITH ORDINALITY AS data(hash, path, sha256, ord)
        ), updated AS (
            UPDATE public.torrent_file AS tf SET sha256 = latest.sha256
            FROM (
                SELECT DISTINCT ON (hash, path) hash, path, sha256
                FROM input ORDER BY hash, path, ord DESC
            ) AS latest
            WHERE tf.torrent_hash_v1 = latest.hash AND tf.relative_path = latest.path
            RETURNING tf.torrent_hash_v1, tf.relative_path
        )
        SELECT input.path FROM input
        LEFT JOIN updated ON updated.torrent_hash_v1 = input.hash AND updated.relative_path = input.path
        WHERE updated.torrent_hash_v1 IS NULL ORDER BY input.ord LIMIT 1"
    )?;

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


/// 只加载当前目录的 .env, 不覆盖进程中已存在的环境变量.
fn load_config() -> Result<Config> {

    /// 读取必需的环境变量, 缺失或为空时返回明确错误.
    fn required_env(name: &str) -> Result<String> {
        let value = env::var(name).with_context(|| format!("请设置 {name}"))?;
        if value.is_empty() { bail!("{name} 不能为空"); }
        Ok(value)
    }

    /// 读取可选端口, 若存在则检查其格式和范围.
    fn optional_port(name: &str) -> Result<Option<u16>> {
        match env::var(name) {
            Ok(value) => {
                let port = value.parse::<u16>().with_context(|| format!("{name} 必须是有效端口"))?;
                if port == 0 { bail!("{name} 不能为 0"); }
                Ok(Some(port))
            }
            Err(env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(error).with_context(|| format!("读取 {name} 失败")),
        }
    }

    match dotenvy::from_path(".env") {
        Ok(_) => eprintln!("已加载当前目录的 .env"),
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("当前目录没有 .env, 使用现有环境变量");
        }
        Err(error) => return Err(error).context("加载 .env 失败"),
    }

    let mut qb_url = Url::parse(&required_env("QB_URL")?).context("QB_URL 不是有效的 URL")?;
    if !matches!(qb_url.scheme(), "http" | "https") || qb_url.host_str().is_none() {
        bail!("QB_URL 必须是包含主机名的 HTTP 或 HTTPS 地址");
    }
    if let Some(port) = optional_port("QB_PORT")? {
        qb_url.set_port(Some(port)).map_err(|_| anyhow::anyhow!("无法设置 QB_PORT"))?;
    }
    if !qb_url.username().is_empty() || qb_url.password().is_some() {
        bail!("QB_URL 不应包含用户名或密码");
    }

    let config = Config {
        qb_url,
        qb_username: required_env("QB_USERNAME")?,
        qb_password: required_env("QB_PASSWORD")?,
        pg_host: required_env("PG_HOST")?,
        pg_port: optional_port("PG_PORT")?,
        pg_user: required_env("PG_USER")?,
        pg_password: required_env("PG_PASSWORD")?,
        pg_db: required_env("PG_DB")?,
    };

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
    Ok(config)
}

/// 流式计算 SHA-256 和前缀 MD5, 并检查文件在读取期间是否变化.
///
/// 返回值: (SHA-256, 前缀 MD5, 文件大小, 创建时间)
fn hash_file(path: &Path, buffer: &mut [u8], progress: &ProgressBar)
-> Result<([u8; 32], [u8; 16], i64, Option<SystemTime>)> {

    let mut file = File::open(path).context("打开文件失败")?;
    let before = file.metadata().context("读取文件元数据失败")?;
    if !before.is_file() {
        bail!("路径不是普通文件");
    }
    if before.len() == 0 {
        bail!("数据库 file.byte_count 要求大于 0, 无法索引空文件");
    }
    let size = i64::try_from(before.len()).context("文件大小超出数据库 bigint 范围")?;
    let created_at = match before.created() {
        Ok(time) => Some(time),
        Err(error) => {
            progress.suspend(|| eprintln!("无法读取文件创建时间 {}: {error}", path.display()));
            None
        }
    };
    progress.reset();
    progress.set_length(before.len());
    progress.set_message(format!("文件哈希 {}", path.display()));
    let mut sha256 = Sha256::new();
    let mut md5 = Md5::new();
    let mut remaining = MD5_PREFIX_BYTES;
    let mut total = 0u64;
    loop {
        let count = file.read(buffer).context("读取文件失败")?;
        if count == 0 {
            break;
        }
        sha256.update(&buffer[..count]);
        let prefix_count = remaining.min(count as u64) as usize;
        md5.update(&buffer[..prefix_count]);
        remaining -= prefix_count as u64;
        total += count as u64;
        progress.inc(count as u64);
    }
    let after = file.metadata().context("再次读取文件元数据失败")?;
    if total != before.len()
        || after.len() != before.len()
        || before.modified()? != after.modified()?
    {
        bail!("文件在计算哈希期间发生变化");
    }
    Ok((
        sha256.finalize().into(),
        md5.finalize().into(),
        size,
        created_at,
    ))
}

/// 创建限频刷新的进度条, 等待期间持续显示耗时, 出错退出时保留中断位置.
fn progress_bar(length: usize, message: &'static str) -> Result<ProgressBar> {
    let progress = ProgressBar::new(length as u64)
        .with_style(ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] {msg} [{bar:30.cyan/blue}] {pos}/{len} ETA {eta}",
        )?)
        .with_message(message)
        .with_finish(ProgressFinish::Abandon);
    progress.enable_steady_tick(Duration::from_millis(100));
    Ok(progress)
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
