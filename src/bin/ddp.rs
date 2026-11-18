
//! src/bin/ddp.rs
//! 从 main.ids 获取 Dandanplay 动画并同步动画和剧集信息到 PostgreSQL.

use std::thread;

use anyhow::{Context, Result, bail};
use anime_tool::cli::CommonArgs;
use chrono::NaiveDateTime;
use clap::Parser;
use postgres::Client;
use serde_json::Value;

#[path = "../kg/utils.rs"]
mod utils;

/// 无位置参数的 Dandanplay 同步命令.
#[derive(Parser)]
#[command(name = "ddp", version, about = "从 main.ids 获取 Dandanplay 动画并刷新动画和剧集表")]
struct Cli {
    /// Dandanplay 认证和全局配置选项.
    #[command(flatten)]
    common: CommonArgs,
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

/// 运行同步并报告完整错误链.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步 Dandanplay 数据失败: {error:#}");
        std::process::exit(1);
    }
}

/// 读取动画 ID, 获取 API 数据并逐动画事务化更新数据库.
fn run() -> Result<()> {

    // 参数解析
    let cli = Cli::parse();
    let env = utils::EnvConfig::load(false)?;
    let config = env.ddp()?;
    let file_config = cli.common.load_config()?;
    let (appid, appsecret) = cli.common.credentials(&file_config)?;
    let ddp_client = utils::DdpApiClient::new(appid, appsecret, config.timeout, &config.api_base)?;

    // 数据库准备
    #[allow(non_snake_case)]
    let mut DB = env.connect_main(None)?;
    let ids = DB
        .query(utils::sql!("ddp/select_ids.sql"), &[])
        .context("读取 public.ids.ddp_ani_id 失败")?
        .into_iter()
        .map(|row| row.try_get(0).context("读取 Dandanplay 动画 ID 失败"))
        .collect::<Result<Vec<i64>>>()?;
    if ids.is_empty() { println!("ids.ddp_ani_id 没有可同步的 ID"); return Ok(()); }

    #[allow(non_snake_case)]
    let P = utils::progress_bar(ids.len(), utils::SYNC_PROGRESS_TEMPLATE, None)?; 
    let mut succeeded     = 0usize;
    let mut failed        = 0usize;
    let mut episode_count = 0usize;
    for (index, anime_id) in ids.iter().enumerate() {

        P.set_message(format!("获取动画 {anime_id}"));

        let result = fetch_package(&ddp_client, *anime_id).and_then(|package| {
            P.set_message(format!("写入动画 {anime_id}, {} 个剧集", package.episodes.len()));
            write_package(&mut DB, &package)?;
            Ok(package.episodes.len())
        });

        match result {
            Ok(count) => { succeeded += 1; episode_count += count; }
            Err(error) => {
                P.suspend(|| eprintln!("Dandanplay 动画 {anime_id} 同步失败: {error:#}"));
                failed += 1;
            }
        }

        P.inc(1); // 更新进度条, 表示已处理一个元素

        if index + 1 < ids.len() && !config.delay.is_zero() {
            P.set_message("等待下一次 API 请求");
            thread::sleep(config.delay);
        }
    }
    P.finish_with_message("Dandanplay 同步完成");

    println!("成功动画: {succeeded}, 失败动画: {failed}, 写入剧集: {episode_count}");
    if failed > 0 { bail!("{failed} 个动画同步失败, 其他成功动画已提交"); }
    Ok(())
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

    // 执行动画信息的 upsert
    tx.execute(utils::sql!("ddp/upsert_anime.sql"), &[
        &package.anime_id,
        &package.bgm_ani_id,
        &package.title,
        &package.intro,
        &package.summary,
        &package.episode_count,
        &package.image_url,
    ]).context("upsert dandanplay_anime 失败")?;

    // 遍历每个剧集并执行 upsert
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

