
//! src/kg/utils.rs
//! 为同步命令提供共享的配置, 数据库连接, 文件哈希和进度条工具.

// 各二进制通过路径引入此模块, 只使用其中需要的工具.
#![allow(dead_code)]

use std::{collections::HashMap, env, ffi::{OsStr, OsString}, fs::File, io::Read,
    path::{Path, PathBuf}, time::{Duration, SystemTime, UNIX_EPOCH}};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};
use md5::Md5;
use postgres::{Client, NoTls};
use reqwest::{
    StatusCode,
    blocking::{Client as HttpClient, Response},
    header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT},
};
use sha2::{Digest, Sha256};
use url::Url;

/// SHA-256 的 32 字节原始值.
pub(super) type Sha256Data = [u8; 32];
/// 前 16 MiB MD5 的 16 字节原始值.
pub(super) type Md5Data = [u8; 16];

/// 单次读取文件的缓冲区大小, 1 MiB.
pub(super) const FILE_BUFFER_SIZE: usize = 1024 * 1024;
/// 计算文件前 16 MiB 的 MD5, 较短文件则计算全部内容.
const MD5_PREFIX_BYTES: u64 = 16 * 1024 * 1024;

/// 在编译时载入 src/kg/sql 下的 SQL 文件, 不依赖运行目录.
macro_rules! sql {
    ($file:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/kg/sql/", $file))
    };
}
pub(super) use sql;

/// bgm 和 mikan 共用的进度条样式.
pub(super) const SYNC_PROGRESS_TEMPLATE: &str =
    "{spinner:.green} [{elapsed_precise}] [{bar:30.cyan/blue}] {pos}/{len} ETA {eta} {msg}";

/// 启动时读取的环境快照, 只在调用方取用时校验对应变量.
pub(super) struct EnvConfig {
    vars: HashMap<OsString, OsString>, // 保留原始值, 避免无关的非 Unicode 变量导致失败.
}

/// 运行所需的 qBittorrent 和 PostgreSQL 连接配置.
pub(super) struct QbConfig {
    pub(super) qb_url      : Url,         // qBittorrent Web API 的基础地址.
    pub(super) qb_username : String,      // qBittorrent 登录用户名.
    pub(super) qb_password : String,      // qBittorrent 登录密码.
    pub(super) pg_host     : String,      // PostgreSQL 主机名.
    pub(super) pg_port     : Option<u16>, // PostgreSQL 端口, 为空时使用驱动默认值.
    pub(super) pg_user     : String,      // PostgreSQL 用户名.
    pub(super) pg_password : String,      // PostgreSQL 密码.
    pub(super) pg_db       : String,      // PostgreSQL 数据库名称.
}

/// Bangumi 同步命令的 HTTP 和请求间隔配置.
pub(super) struct BgmConfig {
    pub(super) api_base: String,  // Bangumi API 基础地址, 不含末尾斜杠.
    pub(super) timeout: Duration, // 单次 HTTP 请求的超时时间.
    pub(super) delay: Duration,   // 两次 API 请求之间的等待时间.
}

/// Dandanplay 同步命令的请求超时和间隔.
pub(super) struct DdpConfig {
    pub(super) api_base: String,  // Dandanplay API 基础地址.
    pub(super) timeout: Duration, // 单次 HTTP 请求的超时时间.
    pub(super) delay: Duration,   // 相邻动画请求之间的等待时间.
}

/// 发送带 Dandanplay 签名认证的番剧详情请求.
pub(super) struct DdpApiClient {
    client: HttpClient,  // 设置超时和重定向策略的 HTTP 客户端.
    appid: String,       // Dandanplay 应用 ID.
    appsecret: String,   // Dandanplay 应用密钥.
    api_base: String,    // Dandanplay API 基础地址.
}

/// 蜜柑请求配置, 数据库使用同项目的 PG_* 环境变量.
pub(super) struct MikanConfig {
    pub(super) rss_base: Url,     // RSS 接口地址.
    pub(super) timeout: Duration, // 单次请求的超时.
    pub(super) delay: Duration,   // 相邻订阅请求之间的等待时间.
}

impl EnvConfig {
    /// 只加载当前目录的 .env, 不覆盖进程中已存在的环境变量.
    pub(super) fn load(verbose: bool) -> Result<Self> {
        match dotenvy::from_path(".env") {
            Ok(_) => {
                if verbose { eprintln!("已加载当前目录的 .env"); }
            }
            Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                if verbose { eprintln!("当前目录没有 .env, 使用现有环境变量"); }
            }
            Err(error) => return Err(error).context("加载 .env 失败"),
        }
        Ok(Self { vars: env::vars_os().collect() })
    }

    /// 读取 qBittorrent API 和 PostgreSQL 连接配置.
    pub(super) fn qb(&self) -> Result<QbConfig> {
        let mut qb_url = Url::parse(&self.required_env("QB_URL")?).context("QB_URL 不是有效的 URL")?;
        if !matches!(qb_url.scheme(), "http" | "https") || qb_url.host_str().is_none() {
            bail!("QB_URL 必须是包含主机名的 HTTP 或 HTTPS 地址");
        }
        if let Some(port) = self.optional_port("QB_PORT", None)? {
            qb_url.set_port(Some(port)).map_err(|_| anyhow::anyhow!("无法设置 QB_PORT"))?;
        }
        if !qb_url.username().is_empty() || qb_url.password().is_some() {
            bail!("QB_URL 不应包含用户名或密码");
        }
        Ok(QbConfig {
            qb_url,
            qb_username: self.required_env("QB_USERNAME")?,
            qb_password: self.required_env("QB_PASSWORD")?,
            pg_host: self.required_env("PG_HOST")?,
            pg_port: self.optional_port("PG_PORT", None)?,
            pg_user: self.required_env("PG_USER")?,
            pg_password: self.required_env("PG_PASSWORD")?,
            pg_db: self.required_env("PG_DB")?,
        })
    }

    /// 读取 Bangumi API URL, 超时和请求间隔.
    pub(super) fn bgm(&self) -> Result<BgmConfig> {
        let api_base = self.env_or("BANGUMI_API_BASE", "https://api.bgm.tv")?;
        let api_base = api_base.trim_end_matches('/').to_owned();
        let parsed = Url::parse(&api_base).context("BANGUMI_API_BASE 不是有效 URL")?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            bail!("BANGUMI_API_BASE 必须是 HTTP 或 HTTPS URL");
        }
        let (timeout, delay) = self.request_timing()?;
        if !delay.is_finite() || delay < 0.0 {
            bail!("REQUEST_DELAY 必须是有限的非负秒数");
        }
        Ok(BgmConfig {
            api_base,
            timeout,
            delay: Duration::try_from_secs_f64(delay).context("REQUEST_DELAY 超出有效范围")?,
        })
    }

    /// 读取 Dandanplay API 请求超时和间隔.
    pub(super) fn ddp(&self) -> Result<DdpConfig> {
        let api_base = self.env_or("DANDANPLAY_API_BASE", "https://api.dandanplay.net")?;
        let api_base = api_base.trim_end_matches('/').to_owned();
        let parsed = Url::parse(&api_base).context("DANDANPLAY_API_BASE 不是有效 URL")?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            bail!("DANDANPLAY_API_BASE 必须是 HTTP 或 HTTPS URL");
        }
        let (timeout, seconds) = self.request_timing()?;
        if !seconds.is_finite() || seconds < 0.0 {
            bail!("REQUEST_DELAY 必须是有限的非负秒数");
        }
        let delay = Duration::try_from_secs_f64(seconds)
            .context("REQUEST_DELAY 超出有效范围")?;
        Ok(DdpConfig { api_base, timeout, delay })
    }

    /// 读取旧脚本兼容的接口地址和请求节流配置.
    pub(super) fn mikan(&self) -> Result<MikanConfig> {
        let rss_base = Url::parse(&self.env_or("MIKAN_RSS_BASE", "https://mikanani.me/RSS/Bangumi")?)
            .context("MIKAN_RSS_BASE 不是有效的 URL")?;
        if !matches!(rss_base.scheme(), "http" | "https") || rss_base.host_str().is_none() {
            bail!("MIKAN_RSS_BASE 必须是 HTTP 或 HTTPS 地址");
        }
        let (timeout, seconds) = self.request_timing()?;
        let delay = Duration::try_from_secs_f64(seconds).context("REQUEST_DELAY 必须是有限的非负秒数")?;
        Ok(MikanConfig { rss_base, timeout, delay })
    }

    /// 从 JSON 数组读取媒体根目录.
    pub(super) fn media_dirs(&self) -> Result<Vec<PathBuf>> {
        serde_json::from_str(&self.required_env("MEDIA_DIRS")?)
            .context("MEDIA_DIRS 必须是 JSON 路径字符串数组")
    }

    /// 使用 PG_DB 及其他 PG_* 环境变量连接数据库.
    pub(super) fn connect_configured(&self) -> Result<Client> {
        let host = self.required_env("PG_HOST")?;
        let port = self.optional_port("PG_PORT", None)?;
        let user = self.required_env("PG_USER")?;
        let password = self.required_env("PG_PASSWORD")?;
        let db = self.required_env("PG_DB")?;
        connect_postgres(&host, port, &user, &password, &db)
            .context("连接 PostgreSQL 失败")
    }

    /// 使用 PG_* 配置连接固定的 main 数据库, 与 set-ids 保持相同的数据库选择规则.
    pub(super) fn connect_main(&self, non_unicode_port_error: Option<&str>) -> Result<Client> {
        let host = self.required_env("PG_HOST")?;
        let user = self.required_env("PG_USER")?;
        let password = self.required_env("PG_PASSWORD")?;
        let port = self.optional_port("PG_PORT", non_unicode_port_error)?;
        connect_postgres(&host, port, &user, &password, "main")
            .context("连接 PostgreSQL main 失败")
    }

    /// 从快照取值, 区分未设置和非 Unicode 值.
    fn get(&self, name: &str) -> std::result::Result<&str, env::VarError> {
        let value = self.vars.get(OsStr::new(name)).ok_or(env::VarError::NotPresent)?;
        value.to_str().ok_or_else(|| env::VarError::NotUnicode(value.clone()))
    }

    /// 读取必需的环境变量, 缺失或为空时返回明确错误.
    fn required_env(&self, name: &str) -> Result<String> {
        let value = self.get(name).with_context(|| format!("请设置 {name}"))?;
        if value.is_empty() { bail!("{name} 不能为空"); }
        Ok(value.to_owned())
    }

    /// 读取环境变量, 仅在未设置时使用默认值, 非 Unicode 值显式报错.
    fn env_or<'a>(&'a self, name: &str, default: &'a str) -> Result<&'a str> {
        match self.get(name) {
            Ok(value) => Ok(value),
            Err(env::VarError::NotPresent) => Ok(default),
            Err(error) => Err(error).with_context(|| format!("读取 {name} 失败")),
        }
    }

    /// 读取可选端口, 若存在则检查其格式和范围.
    fn optional_port(&self, name: &str, non_unicode_error: Option<&str>) -> Result<Option<u16>> {
        match self.get(name) {
            Ok(value) => {
                let port = value.parse::<u16>().with_context(|| format!("{name} 必须是有效端口"))?;
                if port == 0 { bail!("{name} 不能为 0"); }
                Ok(Some(port))
            }
            Err(env::VarError::NotPresent) => Ok(None),
            Err(error) => {
                if let Some(message) = non_unicode_error {
                    bail!("{message}");
                }
                Err(error).with_context(|| format!("读取 {name} 失败"))
            }
        }
    }

    /// 读取请求超时和间隔, 由调用方保留各自的间隔校验和错误信息.
    fn request_timing(&self) -> Result<(Duration, f64)> {
        let timeout = self.env_or("REQUEST_TIMEOUT", "15")?
            .parse::<u64>()
            .context("REQUEST_TIMEOUT 必须是正整数秒数")?;
        if timeout == 0 { bail!("REQUEST_TIMEOUT 必须大于 0"); }
        let delay = self.env_or("REQUEST_DELAY", "0.2")?
            .parse::<f64>()
            .context("REQUEST_DELAY 必须是非负秒数")?;
        Ok((Duration::from_secs(timeout), delay))
    }
}

impl DdpApiClient {
    /// 使用共享配置创建番剧详情 API 客户端.
    pub(super) fn new(
        appid: String,
        appsecret: String,
        timeout: Duration,
        api_base: &str,
    ) -> Result<Self> {
        let parsed_base = Url::parse(api_base).context("Dandanplay API 基础地址无效")?;
        if !matches!(parsed_base.scheme(), "http" | "https") || parsed_base.host_str().is_none() {
            bail!("Dandanplay API 基础地址必须是 HTTP 或 HTTPS URL");
        }
        let client = HttpClient::builder()
            .user_agent("anime-tool/0.1")
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(timeout)
            .build()
            .context("创建 Dandanplay HTTP 客户端失败")?;
        Ok(Self {
            client,
            appid,
            appsecret,
            api_base: api_base.trim_end_matches('/').to_owned(),
        })
    }

    /// 获取指定动画的详情和剧集列表响应正文.
    pub(super) fn anime(&self, anime_id: i64) -> Result<String> {
        let api_path = format!("/api/v2/bangumi/{anime_id}");
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("系统时间早于 Unix epoch")?
            .as_secs() as i64;
        let signature = BASE64.encode(Sha256::digest(
            format!("{}{}{}{}", self.appid, timestamp, api_path, self.appsecret).as_bytes(),
        ));
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static("anime-tool/0.1"));
        headers.insert(
            "X-AppId",
            HeaderValue::from_str(&self.appid).context("Dandanplay AppID 不能用作 HTTP 请求头")?,
        );
        headers.insert(
            "X-Timestamp",
            HeaderValue::from_str(&timestamp.to_string()).expect("timestamp is valid ASCII"),
        );
        headers.insert(
            "X-Signature",
            HeaderValue::from_str(&signature).expect("signature is valid ASCII"),
        );
        let response = self.client
            .get(format!("{}{api_path}", self.api_base))
            .headers(headers)
            .send()
            .context("请求 Dandanplay 番剧详情 API 失败")?;
        Self::check_response(response, &api_path)
    }

    /// 检查 HTTP 状态并读取响应正文.
    fn check_response(response: Response, api_path: &str) -> Result<String> {
        let status = response.status();
        let body = response.text().context("读取 Dandanplay 响应失败")?;
        if !status.is_success() && status != StatusCode::FOUND {
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| value.get("errorMessage")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned))
                .unwrap_or_else(|| body.trim().to_owned());
            bail!(
                "Dandanplay API 请求失败: {} {}: {}",
                status,
                api_path,
                if message.is_empty() { "无错误信息" } else { &message }
            );
        }
        Ok(body)
    }
}

/// 将可选 JSON 整数解析为 PostgreSQL integer 范围内的值.
pub(super) fn optional_i32(value: &serde_json::Value, key: &str) -> Result<Option<i32>> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => number
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .map(Some)
            .with_context(|| format!("{key} 不是有效的 32 位整数")),
        Some(value) => bail!("{key} 必须是整数或 null, 实际为 {value}"),
    }
}

/// 将必需的 JSON 整数校验为非负 int64.
pub(super) fn required_i64(value: Option<&serde_json::Value>, key: &str) -> Result<i64> {
    let number = value.and_then(serde_json::Value::as_i64)
        .with_context(|| format!("{key} 必须是 int64"))?;
    if number < 0 {
        bail!("{key} 不能是负数");
    }
    Ok(number)
}

/// 将可选 JSON 文本读取为字符串, 非字符串值显式报错.
pub(super) fn optional_string(
    value: Option<&serde_json::Value>,
    key: &str,
) -> Result<Option<String>> {
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        Some(value) => bail!("{key} 必须是字符串或 null, 实际为 {value}"),
    }
}

/// 从 Bangumi 主题 URL 提取非负 int64 subject ID.
pub(super) fn optional_bangumi_id(value: Option<&serde_json::Value>) -> Result<Option<i64>> {
    let Some(url) = optional_string(value, "bangumi.bangumiUrl")? else {
        return Ok(None);
    };
    if url.is_empty() {
        return Ok(None);
    }
    let parsed = Url::parse(&url).with_context(|| format!("bangumiUrl 不是有效 URL: {url}"))?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("bangumi.tv") {
        bail!("bangumiUrl 必须使用 https://bangumi.tv/subject/{{id}}: {url}");
    }
    let mut segments = parsed.path_segments()
        .context("bangumiUrl 不支持路径段解析")?
        .collect::<Vec<_>>();
    if segments.last() == Some(&"") {
        segments.pop();
    }
    if segments.len() != 2 || segments[0] != "subject" {
        bail!("bangumiUrl 路径必须是 /subject/{{id}}: {url}");
    }
    let id = segments[1].parse::<i64>()
        .with_context(|| format!("bangumiUrl 的 subject ID 不是有效的 int64: {url}"))?;
    if id < 0 {
        bail!("bangumiUrl 的 subject ID 不能是负数: {url}");
    }
    Ok(Some(id))
}

/// 将可选 API 当地时间解析为不含时区的 PostgreSQL 时间.
pub(super) fn optional_datetime(
    value: Option<&serde_json::Value>,
    key: &str,
) -> Result<Option<NaiveDateTime>> {
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) if value.is_empty() => Ok(None),
        Some(serde_json::Value::String(value)) => {
            if let Ok(date) = DateTime::parse_from_rfc3339(value) {
                return Ok(Some(date.naive_local()));
            }
            NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
                .map(Some)
                .with_context(|| format!("{key} 不是有效的 ISO 本地日期时间: {value:?}"))
        }
        Some(value) => bail!("{key} 必须是日期时间字符串或 null, 实际为 {value}"),
    }
}

/// 将可选日期字符串转换为 PostgreSQL date.
pub(super) fn optional_date(value: &serde_json::Value, key: &str) -> Result<Option<NaiveDate>> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) if value.is_empty() => Ok(None),
        Some(serde_json::Value::String(value)) => parse_date(value)
            .map(Some)
            .with_context(|| format!("{key} 不是有效的 YYYY-MM-DD 日期: {value:?}")),
        Some(value) => bail!("{key} 必须是日期字符串或 null, 实际为 {value}"),
    }
}

/// 解析标准 ISO 日期和旧 Bangumi 数据中未补零的月日.
fn parse_date(value: &str) -> Result<NaiveDate> {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || !(1..=2).contains(&parts[1].len())
        || !(1..=2).contains(&parts[2].len())
        || parts
            .iter()
            .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        bail!("日期格式错误");
    }
    let year = parts[0].parse().context("解析日期年份失败")?;
    let month = parts[1].parse().context("解析日期月份失败")?;
    let day = parts[2].parse().context("解析日期日失败")?;
    NaiveDate::from_ymd_opt(year, month, day).context("日期超出有效范围")
}

/// 将 API 排序数字或文本保存为文本, 不丢失小数集数.
pub(super) fn optional_episode_label(
    value: &serde_json::Value,
    key: &str,
) -> Result<Option<String>> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => Ok(Some(number.to_string())),
        Some(serde_json::Value::String(text)) => Ok(Some(text.clone())),
        Some(value) => bail!("{key} 必须是数字, 字符串或 null, 实际为 {value}"),
    }
}

/// 配置 PostgreSQL 连接并初始化数据库客户端.
pub(super) fn connect_postgres(
    host: &str, port: Option<u16>, user: &str, password: &str, database: &str,
) -> std::result::Result<Client, postgres::Error> {
    let mut config = postgres::Config::new();
    config.host(host).user(user).password(password).dbname(database);
    if let Some(port) = port { config.port(port); }
    config.connect(NoTls)
}

/// 创建限频刷新的进度条, 等待期间持续显示耗时.
/// 指定初始消息时, 出错退出会保留中断位置.
pub(super) fn progress_bar(
    length: usize, template: &str, message: Option<&'static str>,
) -> Result<ProgressBar> {
    let mut progress = ProgressBar::new(length as u64)
        .with_style(ProgressStyle::with_template(template)?);
    if let Some(message) = message {
        progress = progress.with_message(message).with_finish(ProgressFinish::Abandon);
    }
    progress.enable_steady_tick(Duration::from_millis(100));
    Ok(progress)
}

/// 流式计算 SHA-256 和前缀 MD5, 并检查文件在读取期间是否变化.
///
/// 返回值: (SHA-256, 前缀 MD5, 文件大小, 创建时间)
pub(super) fn hash_file(path: &Path, buffer: &mut [u8], progress: &ProgressBar)
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
    Ok((sha256.finalize().into(), md5.finalize().into(), size, created_at))
}
