use std::path::PathBuf;

use clap::{ArgAction, Args};

use crate::config::Config;

#[derive(Args, Debug, Clone)]
pub struct CommonArgs {
    /// Cache root directory.
    #[arg(short = 'c', long = "cache-dir")]
    pub cache_dir: Option<PathBuf>,

    /// Dandanplay AppID.
    #[arg(short = 'a', long = "appid")]
    pub appid: Option<String>,

    /// Dandanplay AppSecret.
    #[arg(short = 's', long = "appsecret")]
    pub appsecret: Option<String>,

    /// Global configuration file.
    #[arg(short = 'f', long = "config")]
    pub config: Option<PathBuf>,
}

impl CommonArgs {
    pub fn load_config(&self) -> anyhow::Result<Config> {
        let path = self.config.clone().unwrap_or_else(Config::default_path);
        Config::load_or_default(&path)
    }

    pub fn cache_dir(&self, config: &Config) -> anyhow::Result<PathBuf> {
        self.cache_dir
            .clone()
            .or_else(|| std::env::var_os("ANIME_TOOL_CACHE_DIR").map(PathBuf::from))
            .or_else(|| config.cache_dir.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "未设置缓存目录。请使用 --cache-dir/-c、\
                     ANIME_TOOL_CACHE_DIR 或配置文件中的 cache_dir"
                )
            })
    }

    pub fn credentials(&self, config: &Config) -> anyhow::Result<(String, String)> {
        let appid = self
            .appid
            .clone()
            .or_else(|| std::env::var("ANIME_TOOL_APPID").ok())
            .or_else(|| config.appid.clone())
            .ok_or_else(|| anyhow::anyhow!("未设置 AppID"))?;

        let appsecret = self
            .appsecret
            .clone()
            .or_else(|| std::env::var("ANIME_TOOL_APPSECRET").ok())
            .or_else(|| config.appsecret.clone())
            .ok_or_else(|| anyhow::anyhow!("未设置 AppSecret"))?;

        if appid.trim().is_empty() || appsecret.trim().is_empty() {
            anyhow::bail!("AppID/AppSecret 不能为空");
        }

        Ok((appid, appsecret))
    }
}

#[derive(Args, Debug, Clone)]
pub struct QueryArgs {
    /// Force a new API request and overwrite the cache.
    #[arg(long = "query", short = 'q', action = ArgAction::SetTrue, conflicts_with = "no_query")]
    pub query: bool,

    /// Do not query the API; require an existing cache.
    #[arg(long = "no-query", action = ArgAction::SetTrue, conflicts_with = "query")]
    pub no_query: bool,
}

impl QueryArgs {
    pub fn force_query(&self) -> bool {
        self.query
    }

    pub fn allow_network(&self) -> bool {
        !self.no_query
    }
}
