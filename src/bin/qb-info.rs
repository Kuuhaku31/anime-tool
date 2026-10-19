
//! src/bin/qb-info.rs
//! 查询 qBittorrent 种子信息并查找 PostgreSQL 中缺失的 hash.
//!
//! 从环境变量获取:
//!   QBITTORRENT_URL
//!   QB_USERNAME
//!   QB_PASSWORD
//!
//! 获取第一个参数作为要查询的种子信息的哈希值
//!
//! 登录 web ui 获取 session
//! 1. session.get(f"{QBITTORRENT_URL}/api/v2/torrents/info", params={"hashes": hash})
//! 2. session.get(f"{QBITTORRENT_URL}/api/v2/torrents/files?hash={hash}")
//! 3. 打印相关信息

use std::env;

use anime_tool::qbittorrent::{QbConfig, TorrentInfo};



const PG_URL: &str = "POSTGRESQL_URL";
const PG_PORT: &str = "POSTGRESQL_PORT";
const PG_USERNAME: &str = "POSTGRESQL_USERNAME";
const PG_PASSWORD: &str = "POSTGRESQL_PASSWORD";
const PG_DATABASE: &str = "POSTGRESQL_DATABASE";

// PostgreSQL 环境变量连接配置.
struct PgConfig {
    // 数据库服务器地址.
    url: Option<String>,
    // 数据库服务器端口.
    port: Option<u16>,
    // 数据库登录用户名.
    username: Option<String>,
    // 数据库登录密码.
    password: Option<String>,
    // 要连接的数据库名称.
    database: Option<String>,
}

impl PgConfig {
    // 从环境变量读取 PostgreSQL 连接配置.
    fn from_env() -> Result<Self, String> {
        dotenvy::dotenv().ok(); // 加载 .env 文件, 将配置写入进程环境变量

        let url = env::var(PG_URL)
            .map_err(|e| eprintln!("获取 {} 失败: {}", PG_URL, e))
            .ok();
        let port = env::var(PG_PORT).ok().and_then(|p| {
            p.parse::<u16>()
                .map_err(|e| eprintln!("获取 {} 失败: {}", PG_PORT, e))
                .ok()
        });
        let username = env::var(PG_USERNAME)
            .map_err(|e| eprintln!("获取 {} 失败: {}", PG_USERNAME, e))
            .ok();
        let password = env::var(PG_PASSWORD)
            .map_err(|e| eprintln!("获取 {} 失败: {}", PG_PASSWORD, e))
            .ok();
        let database = env::var(PG_DATABASE)
            .map_err(|e| eprintln!("获取 {} 失败: {}", PG_DATABASE, e))
            .ok();

        Ok(Self {
            url,
            port,
            username,
            password,
            database,
        })
    }

    // 打印连接配置并隐藏密码.
    fn print_self(&self) {
        println!("PgConfig {{");
        println!("    url: {:?}", self.url);
        println!("    port: {:?}", self.port);
        println!("    username: {:?}", self.username);
        println!(
            "    password: {}",
            if self.password.is_some() {
                "*****"
            } else {
                "null"
            }
        );
        println!("}}");
    }
}

// 从给定的 hash 集合中筛选出数据库不存在的 hash
fn find_missing_hashes(
    client: &mut postgres::Client,
    hashes: &[[u8; 20]],
) -> Result<Vec<[u8; 20]>, String> {
    // 空集合直接返回
    if hashes.is_empty() {
        return Ok(Vec::new());
    }

    // 数据库使用 VARCHAR(40) 保存十六进制 hash, 不能直接与 BYTEA 比较.
    let hash_strings: Vec<String> = hashes
        .iter()
        .map(hex::encode)
        .collect();

    // 查询数据库中不存在的 hash
    let rows = client
        .query(
            r#"
            SELECT input.hash
            FROM UNNEST($1::TEXT[]) AS input(hash)
            WHERE NOT EXISTS (
                SELECT 1
                FROM public.torrent AS t
                WHERE t.torrent_hash_v1 = input.hash
            )
            "#,
            &[&hash_strings],
        )
        .map_err(|e| match e.as_db_error() {
            Some(db_error) => format!(
                "查询缺失 hash 失败: {} (SQLSTATE {})",
                db_error,
                db_error.code().code()
            ),
            None => format!("查询缺失 hash 失败: {e:?}"),
        })?;

    // 将查询结果转换为固定长度的 hash
    rows.into_iter()
        .map(|row| {
            let hash: String = row
                .try_get(0)
                .map_err(|e| format!("读取缺失 hash 失败: {e}"))?;
            let bytes = hex::decode(&hash)
                .map_err(|e| format!("解析缺失 hash 的十六进制值失败: {e}"))?;

            // 验证数据库返回的 hash 长度
            bytes
                .try_into()
                .map_err(|bytes: Vec<u8>| {
                    format!("hash 长度错误: 期望 20 字节, 实际 {} 字节", bytes.len())
                })
        })
        .collect()
}

// 返回所有种子的信息
fn get_torrents_info(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<serde_json::Value, String>
{
    let response = client
        .get(&format!("{}/api/v2/torrents/info", url))
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("获取种子信息失败: {}", response.status()));
    }
    let json: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    Ok(json)
}

// 根据 v1 hash 获取种子的文件列表.
fn get_files_info_by_hash_v1(
    client: &reqwest::blocking::Client,
    url: &str,
    hash: &[u8; 20]
) -> Result<serde_json::Value, String>
{
    let hash_hex = hex::encode(&hash);
    let response = client
        .get(&format!("{}/api/v2/torrents/files", url))
        .query(&[("hash", &hash_hex)])
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("获取种子文件列表失败: {}", response.status()));
    }
    let json: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    Ok(json)
}

// 根据 v1 hash 下载种子文件的原始字节.
fn get_torrent_bytes_by_hash_v1(
    client: &reqwest::blocking::Client,
    url: &str,
    hash: &[u8; 20]
) -> Result<Vec<u8>, String>
{
    let hash_hex = hex::encode(&hash);
    let export_url = format!(
        "{}/api/v2/torrents/export",
        url.trim_end_matches('/')
    );
    let response = client
        .get(&export_url)
        .query(&[("hash", &hash_hex)])
        .send()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("获取种子文件失败: {}", response.status()));
    }
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}

// 获取 qBittorrent 种子列表并输出数据库中缺失的 v1 hash.
fn main() {

    let qb_config = QbConfig::from_env().expect("从环境变量获取配置失败");
    qb_config.print_self();

    let qbt_url = qb_config.url.expect("请设置 QBITTORRENT_URL 环境变量");
    let qb_username = qb_config.username.expect("请设置 QB_USERNAME 环境变量");
    let qb_password = qb_config.password.expect("请设置 QB_PASSWORD 环境变量");

    // 登录
    let client = reqwest::blocking::Client::builder()
        .cookie_store(true)
        .build()
        .expect("创建 HTTP 客户端失败");
    let login_response = client
        .post(&format!("{}/api/v2/auth/login", qbt_url))
        .form(&[("username", &qb_username), ("password", &qb_password)])
        .send()
        .expect("登录失败");
    if !login_response.status().is_success() {
        eprintln!("登录失败");
        std::process::exit(1);
    } else {
        println!("登录成功: {}", qbt_url);
    }

    
    // 获取种子文件列表
    // let info_response = get_files_info_by_hash_v1(&client, &qbt_url, &hash).expect("获取种子信息失败");
    // println!("种子文件列表: {}", serde_json::to_string_pretty(&info_response).expect("格式化 JSON 失败"));


    // 获取种子信息, 创建 TorrentInfo 实例
    let mut torrent_info_list = Vec::new();
    let res = get_torrents_info(&client, &qbt_url).expect("获取种子信息失败");
    let l = res.as_array().expect("解析 Torrent 信息列表失败");
    for torrent_info_json in l {
        let torrent_info = TorrentInfo::new_from_json(torrent_info_json)
        .expect("解析 Torrent 信息失败");
        torrent_info_list.push(torrent_info);
    }
    println!("获取种子信息数量: {}", torrent_info_list.len());

    // 提取所有种子的 hash_v1 列表
    let mut qb_hash_list = Vec::new();
    for torrent_info in &torrent_info_list {
        qb_hash_list.push(torrent_info.hash_v1.clone());
    }

    // 测试 PostgreSQL 配置
    let pg_config = PgConfig::from_env().expect("从环境变量获取 PostgreSQL 配置失败");
    pg_config.print_self();

    // 连接数据库
    let mut pg_connection = postgres::Client::connect(
        &format!(
            "host={} port={} user={} password={} dbname={}",
            pg_config.url.expect("请设置 POSTGRESQL_URL 环境变量"),
            pg_config.port.expect("请设置 POSTGRESQL_PORT 环境变量"),
            pg_config.username.expect("请设置 POSTGRESQL_USERNAME 环境变量"),
            pg_config.password.expect("请设置 POSTGRESQL_PASSWORD 环境变量"),
            pg_config.database.expect("请设置 POSTGRESQL_DATABASE 环境变量")
        ),
        postgres::NoTls,
    )
    .expect("连接 PostgreSQL 数据库失败");

    // 查找不在数据库中的 hash_v1 列表
    // fn find_missing_hashes(client: &mut postgres::Client, hashes: &[Vec<u8>])
    let missing_hash_list = find_missing_hashes(&mut pg_connection, &qb_hash_list).map_err(|e| {
        eprintln!("查找缺失的 hash_v1 失败: {}", e);
        std::process::exit(1);
    }).unwrap();

    // 打印缺失的 hash_v1 列表
    println!("缺失的 hash_v1 数量: {}", missing_hash_list.len());
    // 提取出缺失的 hash_v1 列表对应的 torrent_info_list
    let missing_torrent_info_list: Vec<&TorrentInfo> = torrent_info_list.iter()
        .filter(|torrent_info| missing_hash_list.contains(&torrent_info.hash_v1))
        .collect();

    // 获取 torrent 文件的原始字节
    // let export_url = format!(
    //     "{}/api/v2/torrents/export",
    //     qbt_url.trim_end_matches('/')
    // );

    // let response = client
    //     .get(&export_url)
    //     .header(reqwest::header::REFERER, qbt_url.as_str())
    //     .query(&[("hash", hash)])
    //     .send()
    //     .map_err(|e| format!("获取 torrent 文件失败: {e}")).ok().unwrap();

    // if !response.status().is_success() {
    //     println!("获取 torrent 文件失败: HTTP {}", response.status());
    // }

    // // 保留原始二进制内容, 不转换为字符串.
    // let torrent_file_bytes: Vec<u8> = match response.bytes().map(|bytes| bytes.to_vec()) {
    //         Ok(bytes) => bytes,
    //         Err(e) => {
    //             eprintln!("获取 torrent 文件字节失败: {}", e);
    //             std::process::exit(1);
    //         }
    // };
    // let torrent_file_bytes = get_torrent_bytes_by_hash_v1(&client, &qbt_url, &hash).expect("获取 torrent 文件字节失败");
    // println!("获取 torrent 文件字节成功, 长度: {}", torrent_file_bytes.len());


/*
INSERT INTO public.torrent (torrent_hash_v1, data, title, category) VALUES ($1, $2, $3, $4)
ON CONFLICT (torrent_hash_v1) DO UPDATE SET
data = EXCLUDED.data, category = EXCLUDED.category, title = EXCLUDED.title
*/

    // 插入 missing_hash_list Torrent 信息到数据库
    let mut count = 0;
    for missing_info in &missing_torrent_info_list {

        // 下载种子文件
        let torrent_file_bytes = get_torrent_bytes_by_hash_v1(&client, &qbt_url, &missing_info.hash_v1).expect("获取 torrent 文件字节失败");

        let torrent_hash_v1_str = missing_info.hash_v1.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        pg_connection.execute(
            "INSERT INTO public.torrent (torrent_hash_v1, data, title, category) VALUES ($1, $2, $3, $4) ON CONFLICT (torrent_hash_v1) DO UPDATE SET data = EXCLUDED.data, category = EXCLUDED.category, title = EXCLUDED.title",
            &[
                &torrent_hash_v1_str,
                &torrent_file_bytes,
                &missing_info.title,
                &missing_info.category,
            ],
        ).expect("插入缺失的 Torrent 信息失败");
        count += 1;
    }
    println!("插入缺失的 Torrent 信息数量: {}", count);
    // let torrent_hash_v1_str = torrent_info.hash_v1.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    // pg_connection.execute(
    //     "INSERT INTO public.torrent (torrent_hash_v1, data, title, category) VALUES ($1, $2, $3, $4) ON CONFLICT (torrent_hash_v1) DO UPDATE SET data = EXCLUDED.data, category = EXCLUDED.category, title = EXCLUDED.title",
    //     &[
    //         &torrent_hash_v1_str,
    //         &torrent_file_bytes,
    //         &torrent_info.title,
    //         &torrent_info.category,
    //     ],
    // ).expect("插入 Torrent 信息失败");

}
