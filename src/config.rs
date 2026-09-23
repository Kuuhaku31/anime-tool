use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub cache_dir: Option<PathBuf>,
    pub appid: Option<String>,
    pub appsecret: Option<String>,
}

impl Config {
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(dirs::config_dir)
            .unwrap_or_else(|| PathBuf::from(".config"));

        base.join("anime-tool").join("config.json")
    }

    pub fn load_or_default(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(path)
            .with_context(|| format!("无法读取配置文件: {}", path.display()))?;

        serde_json::from_str(&text)
            .with_context(|| format!("配置文件 JSON 无效: {}", path.display()))
    }
}
