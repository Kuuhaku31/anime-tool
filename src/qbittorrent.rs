
// src/qbittorrent.rs

use std::env;
use std::path::PathBuf;


const QB_URL: &str = "QBITTORRENT_URL";
const PORT: &str = "PORT";
const QB_USERNAME: &str = "QB_USERNAME";
const QB_PASSWORD: &str = "QB_PASSWORD";

pub struct QbConfig {
    pub url: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl QbConfig {
    pub fn from_env() -> Result<Self, String> {
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

    pub fn print_self(&self) {
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
    progress_complete: bool,
}

pub struct TorrentInfo {
    pub hash_v1: [u8; 20], // primary key
    pub category: Option<String>,
    pub title: Option<String>,
    save_path: Option<PathBuf>,
    files_info: Option<Vec<TorrentFileInfo>>,
}


impl TorrentInfo {
    pub fn set_files_info_from_json(&mut self, file_infos: &serde_json::Value) -> Result<(), String> {

        if self.files_info.is_some() { return Err("files_info 已经存在".to_string()); }

        let files_info_array = file_infos.as_array().ok_or("缺少 files_info 字段")?;
        self.files_info = Some(Vec::new());
        for f in files_info_array {
            let relative_path = match f["name"].as_str() {
                Some(s) => PathBuf::from(s),
                None => return Err("缺少 name 字段".to_string()),
            };
            let complete = match f["progress"].as_f64() {
                Some(p) => p == 1.0,
                None => return Err("缺少 progress 字段".to_string()),
            };
            self.files_info.as_mut().unwrap().push(TorrentFileInfo {
                relative_path,
                progress_complete: complete,
            });
        }

        Ok(())
    }

    pub fn new_from_json(torrent_info: &serde_json::Value) -> Result<Self, String> {

        let hash_v1_str = torrent_info["infohash_v1"].as_str().ok_or("缺少 infohash_v1 字段")?;
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
        let title = torrent_info["name"].as_str().map(|s| s.to_string());

        Ok(Self {
            hash_v1,
            category,
            save_path,
            title,
            files_info: None,
        })
    }

    pub fn to_json(&self) -> serde_json::Value {
        let hash_v1_str = self
            .hash_v1
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();
        let category_str = self.category.as_deref().unwrap_or("null");
        let title_str = self.title.as_deref().unwrap_or("null");
        let save_path_str = self
            .save_path
            .as_deref()
            .map(|p| p.to_string_lossy())
            .unwrap_or("null".into());

        serde_json::json!({
            "hash_v1": hash_v1_str,
            "category": category_str,
            "title": title_str,
            "save_path": save_path_str,
            "files_info": self.files_info.as_ref().map(|file_infos| {
                file_infos.iter().map(|f| {
                    serde_json::json!({
                        "relative_path": f.relative_path,
                        "is_complete": f.progress_complete
                    })
                }).collect::<Vec<_>>()
            }).unwrap_or(Vec::new())
        })
    }
}

impl Eq for TorrentInfo {}
impl PartialEq for TorrentInfo {
    fn eq(&self, other: &Self) -> bool {
        self.hash_v1 == other.hash_v1
    }
}
