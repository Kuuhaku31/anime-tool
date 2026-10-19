
//! src/bin/qb-info.rs
//! 顺序获取 qBittorrent 种子及文件信息, 并同步指定字段到 PostgreSQL.

use std::fs::File;
use std::{env, path::PathBuf};

use anime_tool::qbittorrent::{QbConfig, TorrentInfo};
use anyhow::{Context, Result};
use indicatif::ProgressStyle;
use hex;
use sha2::{Digest, Sha256};

type HashV1 = [u8; 20];
#[allow(dead_code)]
type HashSha256 = [u8; 32];

// 执行同步流程并在失败时输出完整错误链.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步种子信息失败: {error:#}");
        std::process::exit(1);
    }
}

// 登录 qBittorrent, 获取全部种子及文件信息, 最后统一写入数据库.
fn run() -> Result<(), String> {

    // 输出当前工作目录, 用于排查 .env 文件定位问题.
    let cwd = env::current_dir().map_err(|e| format!("获取当前工作目录失败: {e}"))?;
    eprintln!("当前工作目录: {}", cwd.display());

    // 显示 .env 文件的实际加载结果.
    match dotenvy::dotenv() {
        Ok(path) => eprintln!("已加载配置文件: {}", path.display()),
        Err(e) => eprintln!("加载 .env 失败: {e}"),
    }

    // 加载配置
    let config = QbConfig::from_env().map_err(anyhow::Error::msg).map_err(|e| e.to_string())?;
    let url = config.url.context("请设置 QBITTORRENT_URL 环境变量").map_err(|e| e.to_string())?;
    let url = url.trim_end_matches('/');
    let username = config.username.context("请设置 QB_USERNAME 环境变量").map_err(|e| e.to_string())?;
    let password = config.password.context("请设置 QB_PASSWORD 环境变量").map_err(|e| e.to_string())?;
    println!("使用的 qBittorrent URL: {}", url);
    println!("使用的 QB_USERNAME: {}", username);
    println!("使用的 QB_PASSWORD: {}", if password.is_empty() { "未设置" } else { "*****" });

    let client = reqwest::blocking::Client::builder()
        .cookie_store(true)
        .build()
        .context("创建 HTTP 客户端失败")
        .map_err(|e| e.to_string())?;
    let _ = client
        .post(format!("{url}/api/v2/auth/login"))
        .form(&[("username", &username), ("password", &password)])
        .send()
        .context("请求 qBittorrent 登录失败").map_err(|e| e.to_string())?
        .error_for_status()
        .context("qBittorrent 登录返回错误状态").map_err(|e| e.to_string())?;

    // 获取种子信息列表
    let mut torrent_infos = get_torrents_info(&client, url).map_err(|e| e.to_string())?;
    println!("获取种子信息数量: {}", torrent_infos.len());

    // 顺序请求每个种子的文件列表和种子文件数据, 全部获取成功后才开始写库.
    let pb = indicatif::ProgressBar::new(torrent_infos.len() as u64); // 创建总长度为种子数量的进度条.
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} ({percent}%) {msg}")
        .unwrap().progress_chars("##-"),
    );
    pb.set_position(0); // 初始化进度条位置为 0
    for torrent_info in &mut torrent_infos {
        let hash = hex::encode(torrent_info.hash_v1);
        let files = client
        .get(format!("{url}/api/v2/torrents/files"))
        .query(&[("hash", &hash)])
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<serde_json::Value>()
        .map_err(|e| e.to_string())?;

        // 请求导出接口.
        let url = format!("{url}/api/v2/torrents/export");
        let response = client
            .get(&url)
            .query(&[("hash", &hash)])
            .send()
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .bytes()
            .map_err(|e| e.to_string())?
            .to_vec();

        torrent_info.torrent_file_data = Some(response);

        torrent_info
            .set_files_info_from_json(&files)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("设置种子 {hash} 的文件信息失败"))
            .map_err(|e| e.to_string())?;
        pb.inc(1); // 更新进度条位置
    }
    pb.finish_with_message("所有种子文件信息获取完成"); // 完成进度条显示

    // 获取每个种子的文件数量
    let mut total_file_count = 0;
    for torrent_info in &torrent_infos {
        total_file_count += torrent_info.get_file_count();
    }
    println!("获取的文件总数量: {}", total_file_count);


    // ======================== //
    // 连接到 PostgreSQL 数据库 //
    // ======================== //
    let mut connection = connect_postgres().map_err(|e| e.to_string())?;

    // 更新种子信息到数据库.
    update_postgres(&mut connection, &torrent_infos).map_err(|e| e.to_string())?;
    println!("同步种子信息完成: {} 个种子", torrent_infos.len());

    // 查询输入文件列表中, 数据库记录的 sha256 尚未计算的文件.
    let mut missing_files = find_files_missing_sha256(&mut connection, &torrent_infos)
        .map_err(|e| format!("查询 sha256 缺失的文件失败: {e}"))?;
    println!("数据库中 sha256 缺失的文件数量: {}", missing_files.len());

    // 构建需要重新计算 sha256 的文件绝对路径
    for file in &mut missing_files {
        let torrent_info = torrent_infos.iter().find(|t| t.hash_v1 == file.torrent_hash_v1);
        if let Some(torrent_info) = torrent_info {
            if let Some(base_path) = &torrent_info.save_path {
                let absolute_path = base_path.join(&file.relative_path);
                file.absolute_path = Some(absolute_path);
            }
        }
    }
    // 输出需要重新计算 sha256 的文件绝对路径示例
    for file in missing_files.iter().take(20) {
        if let Some(absolute_path) = &file.absolute_path {
            println!("待计算 sha256 的文件绝对路径: {}", absolute_path.display());
        }
    }
    if missing_files.len() > 20 { println!("其余 {} 个文件未显示", missing_files.len() - 20);}

    // 计算缺失的 sha256, 并插入更新数据库
    let pb = indicatif::ProgressBar::new(missing_files.len() as u64); // 创建总长度为种子数量的进度条.
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} ({percent}%) {msg}")
        .unwrap().progress_chars("##-"),
    );
    pb.set_position(0); // 初始化进度条位置为 0
    for file in &missing_files {
        if let Some(path) = &file.absolute_path {
            let mut f = File::open(path).map_err(|e| e.to_string())?;
            let mut hasher = Sha256::new();

            std::io::copy(&mut f, &mut hasher).map_err(|e| e.to_string())?;
            let sha256_result = hasher.finalize();
            let sha256_hex = hex::encode(sha256_result);

            // 插入或更新数据库中的 sha256
            let sha256_hex = sha256_hex;
            let torrent_hash_v1 = hex::encode(&file.torrent_hash_v1);
            let relative_path = &file.relative_path.to_string_lossy();
            connection.execute(
                "UPDATE public.torrent_file SET sha256 = $1 WHERE torrent_hash_v1 = $2 AND relative_path = $3",
                &[&sha256_hex, &torrent_hash_v1, &relative_path],
            ).map_err(|e| e.to_string())?;
        }
        pb.inc(1); // 更新进度条位置
    }
    pb.finish_with_message("计算 sha256 完成");

    Ok(())
}

// 表示数据库中尚未计算 sha256 的种子文件.
#[derive(Debug)]
struct MissingSha256File {
    torrent_hash_v1: HashV1,
    relative_path: PathBuf,
    absolute_path: Option<PathBuf>,
}

// 从数据库查找输入文件列表中 sha256 缺失的记录, 使用批量查询避免逐文件访问数据库.
fn find_files_missing_sha256(
    client: &mut postgres::Client,
    torrent_infos: &[TorrentInfo],
) -> Result<Vec<MissingSha256File>, String> {
    // 每批查询的文件数量; SQL 参数始终只有两个数组, 不受单条参数数量限制影响.
    const BATCH_SIZE: usize = 1000;

    if torrent_infos.is_empty() {
        return Ok(Vec::new());
    }

    // 仅返回 files_info 中存在, 且数据库对应记录的 sha256 为 NULL 的文件.
    let statement = client
        .prepare(
            r#"
            SELECT DISTINCT tf.torrent_hash_v1, tf.relative_path
            FROM public.torrent_file AS tf
            INNER JOIN UNNEST($1::TEXT[], $2::TEXT[])
                AS input(torrent_hash_v1, relative_path)
                ON tf.torrent_hash_v1 = input.torrent_hash_v1
                AND tf.relative_path = input.relative_path
            WHERE tf.sha256 IS NULL
            ORDER BY tf.torrent_hash_v1, tf.relative_path
            "#,
        )
        .map_err(|e| format!("准备查询 sha256 缺失文件的语句失败: {e:?}"))?;

    let mut hashes: Vec<String> = Vec::with_capacity(BATCH_SIZE);
    let mut paths: Vec<String> = Vec::with_capacity(BATCH_SIZE);
    let mut missing_files = Vec::new();

    // 分批收集 hash 和相对路径, 并批量查询数据库.
    for torrent_info in torrent_infos {
        let hash = hex::encode(torrent_info.hash_v1);

        if let Some(files) = &torrent_info.files_info {
            for file in files {
                let relative_path = &file.relative_path;

                hashes.push(hash.clone());
                paths.push(relative_path.to_string_lossy().into_owned());

                if hashes.len() >= BATCH_SIZE {
                    append_missing_sha256_batch(
                        client,
                        &statement,
                        &hashes,
                        &paths,
                        &mut missing_files,
                    )?;
                    hashes.clear();
                    paths.clear();
                }
            }
        }
    }

    // 查询最后一批不足 BATCH_SIZE 的文件.
    if !hashes.is_empty() {
        append_missing_sha256_batch(
            client,
            &statement,
            &hashes,
            &paths,
            &mut missing_files,
        )?;
    }

    Ok(missing_files)
}

// 执行一批 sha256 缺失文件查询, 并将结果添加到输出列表.
fn append_missing_sha256_batch(
    client: &mut postgres::Client,
    statement: &postgres::Statement,
    hashes: &[String],
    paths: &[String],
    missing_files: &mut Vec<MissingSha256File>,
) -> Result<(), String> {
    let rows = client
        .query(statement, &[&hashes, &paths])
        .map_err(|e| format!("查询 sha256 缺失文件失败: {e:?}"))?;

    missing_files.extend(rows.into_iter().map(|row| {
        let hash_str: String = row.get(0);
        let hash: HashV1 = hex::decode(&hash_str).unwrap().try_into().unwrap();
        let relative_path: PathBuf = PathBuf::from(row.get::<_, String>(1));
        MissingSha256File {
        torrent_hash_v1: hash,
        relative_path,
        absolute_path: None,
    }}));

    Ok(())
}

// 获取全部种子信息并构建种子对象列表.
fn get_torrents_info(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<Vec<TorrentInfo>> {
    let response = client
        .get(format!("{url}/api/v2/torrents/info"))
        .send()
        .context("请求种子信息失败")?
        .error_for_status()
        .context("种子信息返回错误状态")?
        .json::<serde_json::Value>()
        .context("解析种子信息响应失败")?;
    response
        .as_array()
        .context("种子信息响应不是数组")?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            TorrentInfo::new_from_json(value)
                .map_err(anyhow::Error::msg)
                .with_context(|| format!("解析第 {} 个种子信息失败", index + 1))
        })
        .collect()
}

// 插入或更新种子标识, 标题, 分类, 文件数据及文件标识和相对路径.
fn update_postgres(
    client: &mut postgres::Client,
    torrent_infos: &[TorrentInfo],
) -> Result<(), String> {

    // 每批处理的最大记录数, 可根据实际数据量调整.
    const BATCH_SIZE: usize = 1000;

    // 没有数据时无需创建事务.
    if torrent_infos.is_empty() { return Ok(()); }

    // 开启事务, 确保所有更新操作一起提交.
    let mut transaction = client.transaction().map_err(|e| format!("开始更新种子事务失败: {e:?}"))?;

    // 准备种子批量更新语句.
    // 使用 DISTINCT ON 处理同一批次中的重复 hash, 保留最后一次出现的记录.
    let torrent_statement = transaction.prepare(
        r#"
        INSERT INTO public.torrent (torrent_hash_v1, title, category, data)
        SELECT torrent_hash_v1, title, category, data
        FROM (
            SELECT DISTINCT ON (torrent_hash_v1) torrent_hash_v1, title, category, data
            FROM UNNEST($1::TEXT[], $2::TEXT[], $3::TEXT[], $4::BYTEA[])
            WITH ORDINALITY AS input(torrent_hash_v1, title, category, data, ord)
            ORDER BY torrent_hash_v1, ord DESC
        ) AS latest
        ON CONFLICT (torrent_hash_v1) DO UPDATE SET
            title = EXCLUDED.title,
            category = EXCLUDED.category,
            data = EXCLUDED.data
        "#,
    ).map_err(|e| format!("准备种子批量更新语句失败: {e:?}"))?;

    // 准备种子文件批量插入语句.
    // 发生联合主键冲突时保留已有记录及其 sha256.
    let file_statement = transaction.prepare(
        r#"
        INSERT INTO public.torrent_file (torrent_hash_v1, relative_path)
        SELECT torrent_hash_v1, relative_path
        FROM UNNEST($1::TEXT[], $2::TEXT[])
        AS input(torrent_hash_v1, relative_path)
        ON CONFLICT (torrent_hash_v1, relative_path) DO NOTHING
        "#,
    ).map_err(|e| format!("准备种子文件批量插入语句失败: {e:?}"))?;

    // 分批更新种子基本信息.
    for chunk in torrent_infos.chunks(BATCH_SIZE) {

        // 将每一列转换为 PostgreSQL 数组参数.
        let hashes: Vec<_> = chunk.iter().map(|i| hex::encode(i.hash_v1)).collect();
        let titles: Vec<_> = chunk.iter().map(|i| i.title.clone()).collect();
        let categories: Vec<_> = chunk.iter().map(|i| i.category.clone()).collect();
        let data_s: Vec<_> = chunk.iter().map(|i| i.torrent_file_data.as_deref().unwrap_or(&[])).collect();

        // 一次请求更新整批种子.
        transaction.execute(
            &torrent_statement,
            &[&hashes, &titles, &categories, &data_s],
        ).map_err(|e| format!("批量更新种子失败: {e:?}"))?;
    }

    // 用两个数组缓存待插入的文件记录.
    let mut file_hashes = Vec::with_capacity(BATCH_SIZE);
    let mut file_paths = Vec::with_capacity(BATCH_SIZE);

    // 遍历文件信息时直接借用, 不再克隆 files_info.
    for torrent_info in torrent_infos {

        let hash = hex::encode(torrent_info.hash_v1);

        if let Some(files) = &torrent_info.files_info { for file in files {

            // 数据库使用 TEXT 保存路径, 因此要求路径为有效 UTF-8.
            let relative_path = file.relative_path.to_str()
            .ok_or_else(|| {format!("种子 {hash} 的文件路径不是有效的 UTF-8")})?;

            // 将文件记录加入缓存.
            file_hashes.push(hash.clone());
            file_paths.push(relative_path.to_owned());

            // 达到批次大小时立即写入数据库.
            if file_hashes.len() >= BATCH_SIZE {
                transaction.execute(
                    &file_statement,
                    &[&file_hashes, &file_paths],
                ).map_err(|e| format!("批量插入种子文件失败: {e:?}"))?;

                // 清空缓存, 为下一批文件记录做准备.
                file_hashes.clear();
                file_paths.clear();
            }

        }} else { eprintln!("种子 {hash} 没有文件信息"); }
    }
    // 插入最后一批不足 BATCH_SIZE 的文件记录.
    if !file_hashes.is_empty() {
        transaction.execute(
            &file_statement,
            &[&file_hashes, &file_paths],
        ).map_err(|e| format!("批量插入种子文件失败: {e:?}"))?;
    }

    // 所有操作成功后统一提交.
    transaction.commit().map_err(|e| format!("提交种子更新事务失败: {e:?}"))?;

    Ok(())
}

// 使用环境变量建立数据库连接, 避免手动拼接含密码的连接字符串.
fn connect_postgres() -> Result<postgres::Client> {
    let host = env::var("POSTGRESQL_URL").context("请设置 POSTGRESQL_URL 环境变量")?;
    let port = env::var("POSTGRESQL_PORT").context("请设置 POSTGRESQL_PORT 环境变量")?
        .parse::<u16>().context("POSTGRESQL_PORT 必须是有效的端口号")?;
    let username = env::var("POSTGRESQL_USERNAME")
        .context("请设置 POSTGRESQL_USERNAME 环境变量")?;
    let password = env::var("POSTGRESQL_PASSWORD")
        .context("请设置 POSTGRESQL_PASSWORD 环境变量")?;
    let database = env::var("POSTGRESQL_DATABASE")
        .context("请设置 POSTGRESQL_DATABASE 环境变量")?;
    postgres::Config::new()
        .host(&host)
        .port(port)
        .user(&username)
        .password(&password)
        .dbname(&database)
        .connect(postgres::NoTls)
        .context("连接 PostgreSQL 数据库失败")
}
