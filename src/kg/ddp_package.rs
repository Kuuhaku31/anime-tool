
//! src/kg/ddp_package.rs
//! 为 Dandanplay 批量和单动画命令共享详情解析与事务写入逻辑.

use anyhow::{Context, Result, bail};
use chrono::NaiveDateTime;
use postgres::Client;
use serde_json::Value;

use crate::utils;

/// 获取并完整解析动画详情, 事务化写入动画和剧集并返回剧集数量.
pub(super) fn sync_package(client: &utils::DdpApiClient, db: &mut Client, anime_id: i64) -> Result<usize> {
    let package = fetch_package(client, anime_id)?;
    write_package(db, &package)?;
    Ok(package.episodes.len())
}

/// 一个动画及其全部剧集信息.
struct DandanPlayAnimePackage {
    anime_id: i64,                       // Dandanplay 动画 ID.
    bgm_ani_id: Option<i64>,             // 从 Bangumi 主题 URL 提取的动画 ID.
    title: Option<String>,               // 动画标题.
    intro: Option<String>,               // 动画短简介.
    summary: Option<String>,             // 动画简介.
    episode_count: i32,                  // API 返回的剧集数量.
    image_url: Option<String>,           // 动画封面 URL.
    episodes: Vec<DandanPlayEpisode>,    // 动画下的剧集.
}

/// 一个可写入 dandanplay_episode 的剧集条目.
struct DandanPlayEpisode {
    episode_id: i64,                     // Dandanplay 剧集 ID.
    title: Option<String>,               // 剧集完整标题.
    ep: Option<String>,                  // 剧集短标题, 可能包含字母.
    airdate: Option<NaiveDateTime>,      // 当地播出时间, 不转换为 UTC.
}

/// 获取并严格解析一个动画的 API 响应, 完整解析后才允许写入.
fn fetch_package(client: &utils::DdpApiClient, anime_id: i64) -> Result<DandanPlayAnimePackage> {
    let body = client.anime(anime_id).with_context(|| format!("请求动画 {anime_id} 失败"))?;
    let response: Value = serde_json::from_str(&body).with_context(|| format!("解析动画 {anime_id} 的 JSON 响应失败"))?;
    if response.get("success").and_then(Value::as_bool) != Some(true)
    || response.get("errorCode").and_then(Value::as_i64) != Some(0) {
        let message = response.get("errorMessage").and_then(Value::as_str).unwrap_or("无错误信息");
        bail!("动画 {anime_id} API 返回失败: {message}");
    }
    let bangumi = response.get("bangumi").and_then(Value::as_object).with_context(|| format!("动画 {anime_id} 响应缺少 bangumi 对象"))?;
    let episode_values = match bangumi.get("episodes") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(episodes)) => episodes.as_slice(),
        Some(value) => bail!("bangumi.episodes 必须是数组或 null, 实际为 {value}"),
    };

    // 映射单个 episode, API 字段无效时拒绝整个动画包.
    fn map_episode(value: &Value) -> Result<DandanPlayEpisode> {
        if !value.is_object() { bail!("bangumi.episodes 条目必须是对象"); }
        Ok(DandanPlayEpisode {
            episode_id : utils::required_i64     (value.get("episodeId"    ), "episode.episodeId"    )?,
            title      : utils::optional_string  (value.get("episodeTitle" ), "episode.episodeTitle" )?,
            ep         : utils::optional_string  (value.get("episodeNumber"), "episode.episodeNumber")?,
            airdate    : utils::optional_datetime(value.get("airDate"      ), "episode.airDate"      )?,
        })
    }
    let episodes = episode_values.iter().map(map_episode).collect::<Result<Vec<_>>>()?;

    Ok(DandanPlayAnimePackage {
        anime_id,
        bgm_ani_id    : utils::optional_bangumi_id(bangumi.get("bangumiUrl"))?,
        title         : utils::optional_string    (bangumi.get("animeTitle"), "bangumi.animeTitle")?,
        intro         : utils::optional_string    (bangumi.get("intro"), "bangumi.intro")?,
        summary       : utils::optional_string    (bangumi.get("summary"), "bangumi.summary")?,
        image_url     : utils::optional_string    (bangumi.get("imageUrl"), "bangumi.imageUrl")?,
        episode_count : i32::try_from(episodes.len()).context("剧集数量超出 PostgreSQL integer 范围")?,
        episodes
    })
}

/// 在单个事务中 upsert 动画和其全部剧集, 不清理未返回的历史剧集.
fn write_package(db: &mut Client, package: &DandanPlayAnimePackage) -> Result<()> {
    let mut tx = db.transaction().context("开始 Dandanplay 动画事务失败")?;
    tx.execute(utils::sql!("ddp/upsert_anime.sql"), &[
        &package.anime_id,
        &package.bgm_ani_id,
        &package.title,
        &package.intro,
        &package.summary,
        &package.episode_count,
        &package.image_url,
    ]).context("upsert dandanplay_anime 失败")?;

    let statement = tx
        .prepare(utils::sql!("ddp/upsert_episode.sql"))
        .context("准备 Dandanplay 剧集 upsert 失败")?;
    for episode in &package.episodes {
        tx.execute(&statement, &[
            &episode.episode_id,
            &package.anime_id,
            &episode.title,
            &episode.ep,
            &episode.airdate,
        ]).with_context(|| format!("upsert Dandanplay episode {} 失败", episode.episode_id))?;
    }

    tx.commit().context("提交 Dandanplay 动画事务失败")?;
    Ok(())
}
