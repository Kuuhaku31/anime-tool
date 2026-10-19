
//! src/bin/qb-info.rs
//! 顺序获取 qBittorrent 种子及文件信息, 并同步指定字段到 PostgreSQL.

use std::env;

use anime_tool::qbittorrent::{QbConfig, TorrentInfo};
use anyhow::{Context, Result};

// 执行同步流程并在失败时输出完整错误链.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步种子信息失败: {error:#}");
        std::process::exit(1);
    }
}

// 登录 qBittorrent, 获取全部种子及文件信息, 最后统一写入数据库.
fn run() -> Result<()> {

    // 加载配置
    let config = QbConfig::from_env().map_err(anyhow::Error::msg)?;
    let url = config.url.context("请设置 QBITTORRENT_URL 环境变量")?;
    let url = url.trim_end_matches('/');
    let username = config.username.context("请设置 QB_USERNAME 环境变量")?;
    let password = config.password.context("请设置 QB_PASSWORD 环境变量")?;
    println!("使用的 qBittorrent URL: {}", url);
    println!("使用的 QB_USERNAME: {}", username);
    println!("使用的 QB_PASSWORD: {}", if password.is_empty() { "未设置" } else { "*****" });

    let client = reqwest::blocking::Client::builder()
        .cookie_store(true)
        .build()
        .context("创建 HTTP 客户端失败")?;
    let _ = client
        .post(format!("{url}/api/v2/auth/login"))
        .form(&[("username", &username), ("password", &password)])
        .send()
        .context("请求 qBittorrent 登录失败")?
        .error_for_status()
        .context("qBittorrent 登录返回错误状态")?;

    // 获取种子信息列表
    let mut torrent_infos = get_torrents_info(&client, url)?;
    println!("获取种子信息数量: {}", torrent_infos.len());

    // 顺序请求每个种子的文件列表, 全部获取成功后才开始写库.
    for torrent_info in &mut torrent_infos {
        let hash = hex::encode(torrent_info.hash_v1);
        let files = client
            .get(format!("{url}/api/v2/torrents/files"))
            .query(&[("hash", &hash)])
            .send()
            .with_context(|| format!("请求种子 {hash} 的文件列表失败"))?
            .error_for_status()
            .with_context(|| format!("种子 {hash} 的文件列表返回错误状态"))?
            .json::<serde_json::Value>()
            .with_context(|| format!("解析种子 {hash} 的文件列表响应失败"))?;

        torrent_info
            .set_files_info_from_json(&files)
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("设置种子 {hash} 的文件信息失败"))?;
    }

    // 获取每个种子的文件数量
    let mut total_file_count = 0;
    for torrent_info in &torrent_infos {
        total_file_count += torrent_info.get_file_count();
    }
    println!("获取的文件总数量: {}", total_file_count);

    let mut connection = connect_postgres()?;
    update_postgres(&mut connection, &torrent_infos).map_err(anyhow::Error::msg)?;
    println!("同步种子信息完成: {} 个种子", torrent_infos.len());
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

// 仅插入或更新种子标识, 标题, 分类及文件标识和相对路径.
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
        INSERT INTO public.torrent (torrent_hash_v1, title, category)
        SELECT torrent_hash_v1, title, category
        FROM (
            SELECT DISTINCT ON (torrent_hash_v1) torrent_hash_v1, title, category
            FROM UNNEST($1::TEXT[], $2::TEXT[], $3::TEXT[])
            WITH ORDINALITY AS input(torrent_hash_v1, title, category, ord)
            ORDER BY torrent_hash_v1, ord DESC
        ) AS latest
        ON CONFLICT (torrent_hash_v1) DO UPDATE SET
            title = EXCLUDED.title,
            category = EXCLUDED.category
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

        // 一次请求更新整批种子.
        transaction.execute(
            &torrent_statement,
            &[&hashes, &titles, &categories],
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
