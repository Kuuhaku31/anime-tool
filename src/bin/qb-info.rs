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
        let url = env::var(QB_URL).map_err(|e| eprintln!("获取 {} 失败: {}", QB_URL, e)).ok();
        let port = env::var(PORT).ok().and_then(|p| p.parse::<u16>().map_err(|e| eprintln!("获取 {} 失败: {}", PORT, e)).ok());
        let username = env::var(QB_USERNAME).map_err(|e| eprintln!("获取 {} 失败: {}", QB_USERNAME, e)).ok();
        let password = env::var(QB_PASSWORD).map_err(|e| eprintln!("获取 {} 失败: {}", QB_PASSWORD, e)).ok();

        Ok(Self {url, port, username, password})
    }

    fn print_self(&self) {
        println!("QbConfig {{");
        println!("    url: {:?}", self.url);
        println!("    port: {:?}", self.port);
        println!("    username: {:?}", self.username);
        println!("    password: {}", if self.password.is_some() { "*****" } else { "null" });
        println!("}}");
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
    let info_text = info_response.text().expect("读取种子信息失败");
    println!("种子信息: {}", info_text);

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
    let files_text = files_response.text().expect("读取种子文件列表失败");
    println!("种子文件列表: {}", files_text);
}
