use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;

pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn dandanplay_root(&self) -> PathBuf {
        self.root.join("dandanplay")
    }

    pub fn match_file(&self, hash: &str) -> PathBuf {
        self.dandanplay_root()
            .join("match")
            .join(format!("{hash}.json"))
    }

    pub fn danmaku_file(&self, episode_id: i64) -> PathBuf {
        self.dandanplay_root()
            .join("danmaku")
            .join(format!("{episode_id}.json"))
    }

    pub fn anime_file(&self, anime_id: i64) -> PathBuf {
        self.dandanplay_root()
            .join("anime")
            .join(format!("{anime_id}.json"))
    }

    pub fn read_if_exists(&self, path: &Path) -> anyhow::Result<Option<String>> {
        if !path.is_file() {
            return Ok(None);
        }

        Ok(Some(fs::read_to_string(path).with_context(|| {
            format!("无法读取缓存: {}", path.display())
        })?))
    }

    pub fn write_raw_json(&self, path: &Path, body: &str) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("无法创建缓存目录: {}", parent.display()))?;
        }

        fs::write(path, body).with_context(|| format!("无法写入缓存: {}", path.display()))?;
        Ok(())
    }
}
