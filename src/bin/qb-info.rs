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

use std::{env, path::PathBuf};

const QB_URL: &str = "QBITTORRENT_URL";
const PORT: &str = "PORT";
const QB_USERNAME: &str = "QB_USERNAME";
const QB_PASSWORD: &str = "QB_PASSWORD";

struct QbConfig {
    url: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
}

impl QbConfig {
    fn from_env() -> Result<Self, String> {
        dotenvy::dotenv().ok(); // 加载 .env 文件, 将配置写入进程环境变量

        // 读取字符串类型的环境变量
        let url = env::var(QB_URL)
            .map_err(|e| eprintln!("获取 {} 失败: {}", QB_URL, e))
            .ok();
        let port = env::var(PORT).ok().and_then(|p| {
            p.parse::<u16>()
                .map_err(|e| eprintln!("获取 {} 失败: {}", PORT, e))
                .ok()
        });
        let username = env::var(QB_USERNAME)
            .map_err(|e| eprintln!("获取 {} 失败: {}", QB_USERNAME, e))
            .ok();
        let password = env::var(QB_PASSWORD)
            .map_err(|e| eprintln!("获取 {} 失败: {}", QB_PASSWORD, e))
            .ok();

        Ok(Self {
            url,
            port,
            username,
            password,
        })
    }

    fn print_self(&self) {
        println!("QbConfig {{");
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

struct TorrentFileInfo {
    relative_path: PathBuf,
    is_seed: bool,
}

struct TorrentInfo {
    hash_v1: [u8; 20], // primary key
    category: Option<String>,
    save_path: Option<PathBuf>,
    file_infos: Vec<TorrentFileInfo>,
}

impl Eq for TorrentInfo {}
impl PartialEq for TorrentInfo {
    fn eq(&self, other: &Self) -> bool {
        self.hash_v1 == other.hash_v1
    }
}

impl TorrentInfo {
    fn from_json_response(
        torrent_info: &serde_json::Value,
        file_infos: &serde_json::Value,
    ) -> Result<Self, String> {
        let hash_v1_str = torrent_info["infohash_v1"]
            .as_str()
            .ok_or("缺少 infohash_v1 字段")?;
        let mut hash_v1 = [0u8; 20];
        for (i, byte) in hash_v1_str.as_bytes().chunks(2).enumerate() {
            if i < 20 {
                hash_v1[i] = match u8::from_str_radix(&String::from_utf8_lossy(byte), 16) {
                    Ok(b) => b,
                    Err(e) => return Err(format!("解析 hash_v1 失败: {}", e)),
                };
            }
        }

        let category = torrent_info["category"].as_str().map(|s| s.to_string());
        let save_path = torrent_info["save_path"].as_str().map(|s| PathBuf::from(s));
        let file_infos = file_infos.as_array().ok_or("缺少 file_infos 字段")?;
        let mut file_infos_vec = Vec::new();
        for f in file_infos {
            let relative_path = match f["name"].as_str() {
                Some(s) => PathBuf::from(s),
                None => return Err("缺少 name 字段".to_string()),
            };
            let is_seed = match f["is_seed"].as_bool() {
                Some(b) => b,
                None => return Err("缺少 is_seed 字段".to_string()),
            };
            file_infos_vec.push(TorrentFileInfo {
                relative_path,
                is_seed,
            });
        }

        Ok(Self {
            hash_v1,
            category,
            save_path,
            file_infos: file_infos_vec,
        })
    }

    fn to_json(&self) -> serde_json::Value {
        let hash_v1_str = self
            .hash_v1
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let category_str = self.category.as_deref().unwrap_or("null");
        let save_path_str = self
            .save_path
            .as_deref()
            .map(|p| p.to_string_lossy())
            .unwrap_or("null".into());

        serde_json::json!({
            "hash_v1": hash_v1_str,
            "category": category_str,
            "save_path": save_path_str,
            "file_infos": self.file_infos.iter().map(|f| {
                serde_json::json!({
                    "relative_path": f.relative_path,
                    "is_seed": f.is_seed
                })
            }).collect::<Vec<_>>()
        })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("请提供种子哈希值");
        std::process::exit(1);
    }
    let hash = &args[1];

    let config = QbConfig::from_env().expect("从环境变量获取配置失败");
    config.print_self();

    let qbt_url = config.url.expect("请设置 QBITTORRENT_URL 环境变量");
    let qb_username = config.username.expect("请设置 QB_USERNAME 环境变量");
    let qb_password = config.password.expect("请设置 QB_PASSWORD 环境变量");

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

    // 获取种子信息
    let info_response = client
        .get(&format!("{}/api/v2/torrents/info", qbt_url))
        .query(&[("hashes", hash)])
        .send();
    let info_response = match info_response {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("获取种子信息失败: {}", e);
            std::process::exit(1);
        }
    };
    if !info_response.status().is_success() {
        eprintln!("获取种子信息失败: {}", info_response.status());
        std::process::exit(1);
    }
    let info_response: serde_json::Value = info_response.json().expect("解析种子信息为 JSON 失败");

    // 获取种子文件列表
    let files_response = client
        .get(&format!("{}/api/v2/torrents/files", qbt_url))
        .query(&[("hash", hash)])
        .send()
        .expect("获取种子文件列表失败");
    if !files_response.status().is_success() {
        eprintln!("获取种子文件列表失败");
        std::process::exit(1);
    }
    let files_json: serde_json::Value =
        files_response.json().expect("解析种子文件列表为 JSON 失败");

    let json_response = &info_response[0];

    // 创建 TorrentInfo 实例
    let torrent_info =
        TorrentInfo::from_json_response(json_response, &files_json).expect("解析 Torrent 信息失败");

    println!(
        "Torrent 信息: {}",
        serde_json::to_string_pretty(&torrent_info.to_json()).expect("格式化 JSON 失败")
    );
}
