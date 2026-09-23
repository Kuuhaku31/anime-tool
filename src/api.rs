use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT},
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const API_BASE: &str = "https://api.dandanplay.net";
const USER_AGENT_VALUE: &str = "anime-tool/0.1";

pub struct DandanplayClient {
    client: Client,
    appid: String,
    appsecret: String,
}

impl DandanplayClient {
    pub fn new(appid: String, appsecret: String) -> anyhow::Result<Self> {
        let client = Client::builder()
            .user_agent(USER_AGENT_VALUE)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .context("创建 HTTP 客户端失败")?;

        Ok(Self {
            client,
            appid,
            appsecret,
        })
    }

    fn timestamp() -> anyhow::Result<i64> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("系统时间早于 Unix epoch")?
            .as_secs() as i64)
    }

    fn signature(&self, timestamp: i64, api_path: &str) -> String {
        let text = format!("{}{}{}{}", self.appid, timestamp, api_path, self.appsecret);
        let digest = Sha256::digest(text.as_bytes());
        BASE64.encode(digest)
    }

    fn headers(&self, timestamp: i64, api_path: &str, json: bool) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        if json {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        headers.insert(
            "X-AppId",
            HeaderValue::from_str(&self.appid).expect("invalid AppID header"),
        );
        headers.insert(
            "X-Timestamp",
            HeaderValue::from_str(&timestamp.to_string()).expect("timestamp is always valid ASCII"),
        );
        headers.insert(
            "X-Signature",
            HeaderValue::from_str(&self.signature(timestamp, api_path))
                .expect("signature is always valid ASCII"),
        );
        headers
    }

    fn check_response(response: Response, api_path: &str) -> anyhow::Result<String> {
        let status = response.status();
        let body = response.text().context("读取 Dandanplay 响应失败")?;

        if !status.is_success() && status != StatusCode::FOUND {
            let message = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("errorMessage")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| body.trim().to_owned());

            bail!(
                "Dandanplay API 请求失败: {} {}: {}",
                status,
                api_path,
                if message.is_empty() {
                    "无错误信息"
                } else {
                    &message
                }
            );
        }

        Ok(body)
    }

    pub fn match_hash_only(&self, file_hash: &str, file_size: u64) -> anyhow::Result<String> {
        let api_path = "/api/v2/match";
        let timestamp = Self::timestamp()?;

        let mut body = BTreeMap::new();
        body.insert("fileHash", Value::String(file_hash.to_owned()));
        body.insert("fileSize", Value::from(file_size));
        body.insert("matchMode", Value::String("hashOnly".to_owned()));

        let response = self
            .client
            .post(format!("{API_BASE}{api_path}"))
            .headers(self.headers(timestamp, api_path, true))
            .json(&body)
            .send()
            .context("请求 Dandanplay 匹配 API 失败")?;

        Self::check_response(response, api_path)
    }

    pub fn danmaku(&self, episode_id: i64) -> anyhow::Result<String> {
        let api_path = format!("/api/v2/comment/{episode_id}");
        let timestamp = Self::timestamp()?;

        let response = self
            .client
            .get(format!("{API_BASE}{api_path}"))
            .query(&[("withRelated", "true"), ("chConvert", "0")])
            .headers(self.headers(timestamp, &api_path, false))
            .send()
            .context("请求 Dandanplay 弹幕 API 失败")?;
        Self::check_response(response, &api_path)
    }

    pub fn anime(&self, anime_id: i64) -> anyhow::Result<String> {
        let api_path = format!("/api/v2/bangumi/{anime_id}");
        let timestamp = Self::timestamp()?;

        let response = self
            .client
            .get(format!("{API_BASE}{api_path}"))
            .headers(self.headers(timestamp, &api_path, false))
            .send()
            .context("请求 Dandanplay 番剧详情 API 失败")?;

        Self::check_response(response, &api_path)
    }
}
