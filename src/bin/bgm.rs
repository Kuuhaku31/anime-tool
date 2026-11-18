//! src/bin/bgm.rs
//! 从 main.ids 获取 Bangumi 动画并同步动画和剧集信息到 PostgreSQL.

use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::Parser;
use indicatif::ProgressBar;
use postgres::Client;
use reqwest::blocking::Client as HttpClient;
use serde_json::Value;

#[path = "../kg/utils.rs"]
mod utils;

/// Bangumi episode API 每页最多请求的记录数.
const EPISODE_PAGE_SIZE: usize = 200;
/// Bangumi API 使用的客户端标识.
const USER_AGENT: &str = concat!("anime-tool/", env!("CARGO_PKG_VERSION"), " (Bangumi sync)");

/// 无位置参数的同步命令, 支持标准帮助和版本选项.
#[derive(Parser)]
#[command(
    name = "bgm",
    version,
    about = "从 main.ids 获取 Bangumi 动画并刷新动画和剧集表"
)]
struct Cli {}

/// 一个动画及其全部有效剧集数据.
struct AnimePackage {
    anime_id: i32,                 // 动画 Bangumi ID.
    title: Option<String>,         // 动画原始标题.
    title_cn: Option<String>,      // 动画中文标题.
    airdate: Option<NaiveDate>,    // 动画首播日期.
    summary: Option<String>,       // 动画简介.
    ep_count: Option<i32>,         // 正篇集数.
    object_count: Option<i32>,     // 包含特别篇等内容的总集数.
    cover_url: Option<String>,     // 大尺寸封面 URL.
    official_site: Option<String>, // 官网 URL.
    episodes: Vec<Episode>,        // 动画下的有效剧集列表.
}

/// 一个可写入 bangumi_episode 的剧集条目.
struct Episode {
    episode_id: i32,            // Bangumi Episode ID
    title: Option<String>,      // 原始标题
    title_cn: Option<String>,   // 中文标题
    airdate: Option<NaiveDate>, // 放送日期
    summary: Option<String>,    // 剧情简介
    ep: Option<String>,         // 剧集标识
    sort: Option<i16>,          // 剧集排序
    duration: Option<i32>,      // 剧集时长秒数
}

/// 运行同步并报告完整错误链.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步 Bangumi 数据失败: {error:#}");
        std::process::exit(1);
    }
}

/// 读取动画 ID, 获取 API 数据并逐动画事务化更新数据库.
fn run() -> Result<()> {
    let _cli = Cli::parse();
    let env = utils::EnvConfig::load(false)?;
    let config = env.bgm()?;
    let mut db = env.connect_main(None)?;
    // 获取全部动画 ID, 忽略 NULL 并去重排序.
    let ids: Vec<i32> = db.query(
        utils::sql!("bgm/select_ids.sql"),
        &[],
    )
    .context("读取 public.ids.bgm_ani_id 失败")?
    .into_iter()
    .map(|row| row.try_get(0).context("读取 Bangumi 动画 ID 失败"))
    .collect::<Result<_>>()?;
    if ids.is_empty() {
        println!("ids.bgm_ani_id 没有可同步的 ID");
        return Ok(());
    }

    let http = HttpClient::builder()
        .user_agent(USER_AGENT)
        .timeout(config.timeout)
        .build()
        .context("创建 Bangumi HTTP 客户端失败")?;
    let progress = utils::progress_bar(ids.len(), utils::SYNC_PROGRESS_TEMPLATE, None)?;

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut episode_count = 0usize;
    for (index, anime_id) in ids.iter().enumerate() {
        progress.set_message(format!("获取动画 {anime_id}"));
        let result = fetch_package(&http, &config, *anime_id, &progress).and_then(|package| {
            progress.set_message(format!(
                "写入动画 {anime_id}, {} 个剧集",
                package.episodes.len()
            ));
            write_package(&mut db, &package)?;
            Ok(package.episodes.len())
        });
        match result {
            Ok(count) => {
                succeeded += 1;
                episode_count += count;
            }
            Err(error) => {
                progress.suspend(|| {
                    eprintln!("Bangumi 动画 {anime_id} 同步失败: {error:#}");
                });
                failed += 1;
            }
        }
        progress.inc(1);
        if index + 1 < ids.len() && !config.delay.is_zero() {
            progress.set_message("等待下一次 API 请求");
            thread::sleep(config.delay);
        }
    }

    progress.finish_with_message("Bangumi 同步完成");
    println!("成功动画: {succeeded}, 失败动画: {failed}, 写入剧集: {episode_count}");
    if failed > 0 {
        bail!("{failed} 个动画同步失败, 其他成功动画已提交");
    }
    Ok(())
}

/// 获取一个动画 subject 和所有 episode 分页, 完成后再交给数据库事务.
fn fetch_package(
    http: &HttpClient,
    config: &utils::BgmConfig,
    anime_id: i32,
    progress: &ProgressBar,
) -> Result<AnimePackage> {
    /// 将可选 JSON 整数解析为 PostgreSQL integer 范围内的值.
    fn optional_i32(value: &Value, key: &str) -> Result<Option<i32>> {
        match value.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(number)) => number
                .as_i64()
                .and_then(|value| i32::try_from(value).ok())
                .map(Some)
                .with_context(|| format!("{key} 不是有效的 32 位整数")),
            Some(value) => bail!("{key} 必须是整数或 null, 实际为 {value}"),
        }
    }

    /// 将 subject 和 episode JSON 映射为精简 PostgreSQL 字段.
    fn map_package(
        subject: &Value,
        episode_values: Vec<Value>,
        progress: &ProgressBar,
    ) -> Result<AnimePackage> {
        /// 将可选 JSON 值解析为 SQL 文本, 非字符串值显式报错.
        fn optional_string(value: Option<&Value>, key: &str) -> Result<Option<String>> {
            match value {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(value)) => Ok(Some(value.clone())),
                Some(value) => bail!("{key} 必须是字符串或 null, 实际为 {value}"),
            }
        }

        let anime_id = optional_i32(subject, "id")?.context("subject.id 缺失")?;
        let images = subject.get("images");
        let cover_url = images.and_then(|images| images.get("large"));
        let official_site = subject
            .get("infobox")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item.get("key").and_then(Value::as_str) == Some("官方网站"))
            })
            .and_then(|item| item.get("value"))
            .and_then(Value::as_str)
            .map(str::to_owned);

        let mut episodes = Vec::with_capacity(episode_values.len());
        for (index, value) in episode_values.iter().enumerate() {
            if !value.is_object() {
                progress.suspend(|| {
                    eprintln!("动画 {anime_id} 的 episodes[{index}] 不是对象, 已跳过");
                });
                continue;
            }
            let episode_id = match optional_i32(value, "id") {
                Ok(Some(id)) => id,
                Ok(None) | Err(_) => {
                    progress.suspend(|| {
                        eprintln!("动画 {anime_id} 的 episodes[{index}] 缺少有效 id, 已跳过");
                    });
                    continue;
                }
            };
            let episode_anime_id = optional_i32(value, "subject_id")?.unwrap_or(anime_id);
            if episode_anime_id != anime_id {
                progress.suspend(|| {
                eprintln!(
                    "动画 {anime_id} 的 episodes[{index}] 属于 subject {episode_anime_id}, 已跳过"
                );
            });
                continue;
            }
            episodes.push(Episode {
                episode_id,
                title: optional_string(value.get("name"), "name")?,
                title_cn: optional_string(value.get("name_cn"), "name_cn")?,
                airdate: optional_date(value, "airdate")?,
                summary: optional_string(value.get("desc"), "desc")?,
                // 数据库列名与 API 字段交叉映射, 保留 API sort 的小数表示.
                ep: optional_episode_label(value, "sort")?,
                // 将 API 集数转换为 smallint, 拒绝小数和越界值以避免静默丢失数据.
                sort: optional_i32(value, "ep")?.map(|number| {
                    i16::try_from(number)
                        .with_context(|| format!("ep 超出 smallint 范围: {number}"))
                }).transpose()?,
                duration: optional_i32(value, "duration_seconds")?,
            });
        }

        Ok(AnimePackage {
            anime_id,
            title: optional_string(subject.get("name"), "name")?,
            title_cn: optional_string(subject.get("name_cn"), "name_cn")?,
            airdate: optional_date(subject, "date")?,
            summary: optional_string(subject.get("summary"), "summary")?,
            ep_count: optional_i32(subject, "eps")?,
            object_count: optional_i32(subject, "total_episodes")?,
            cover_url: optional_string(cover_url, "subject.images.large")?,
            official_site,
            episodes,
        })
    }

    let subject_url = format!("{}/v0/subjects/{anime_id}", config.api_base);
    let subject: Value = http
        .get(subject_url)
        .send()
        .with_context(|| format!("请求 subject {anime_id} 失败"))?
        .error_for_status()
        .with_context(|| format!("subject {anime_id} 返回错误状态"))?
        .json()
        .with_context(|| format!("解析 subject {anime_id} JSON 失败"))?;
    if !subject.is_object() {
        bail!("subject {anime_id} 的响应不是 JSON 对象");
    }
    let response_id =
        optional_i32(&subject, "id")?.with_context(|| format!("subject {anime_id} 缺少有效 id"))?;
    if response_id != anime_id {
        bail!("subject ID 不匹配, 请求 {anime_id}, 响应 {response_id}");
    }

    pause(config.delay);
    let episodes = fetch_episodes(http, config, anime_id, progress)?;
    map_package(&subject, episodes, progress)
}

/// 按 API 分页规则获取一个动画的全部剧集.
fn fetch_episodes(
    http: &HttpClient,
    config: &utils::BgmConfig,
    anime_id: i32,
    progress: &ProgressBar,
) -> Result<Vec<Value>> {
    let mut episodes = Vec::new();
    let mut offset = 0usize;
    loop {
        progress.set_message(format!("获取动画 {anime_id} 的剧集: {}", episodes.len()));
        let response: Value = http
            .get(format!("{}/v0/episodes", config.api_base))
            .query(&[
                ("subject_id", anime_id.to_string()),
                ("limit", EPISODE_PAGE_SIZE.to_string()),
                ("offset", offset.to_string()),
            ])
            .send()
            .with_context(|| format!("请求动画 {anime_id} 的剧集分页 offset={offset} 失败"))?
            .error_for_status()
            .with_context(|| format!("动画 {anime_id} 的剧集分页返回错误状态"))?
            .json()
            .with_context(|| format!("解析动画 {anime_id} 的剧集分页失败"))?;
        if !response.is_object() {
            bail!("动画 {anime_id} 的剧集响应不是 JSON 对象");
        }
        let page = response
            .get("data")
            .and_then(Value::as_array)
            .with_context(|| format!("动画 {anime_id} 的 episodes.data 不是数组"))?;
        let page_len = page.len();
        let total = response.get("total").and_then(Value::as_u64);
        episodes.extend(page.iter().cloned());
        progress.set_message(match total {
            Some(total) => format!("动画 {anime_id} 的剧集: {}/{}", episodes.len(), total),
            None => format!("动画 {anime_id} 的剧集: {}", episodes.len()),
        });
        if page_len < EPISODE_PAGE_SIZE || total.is_some_and(|total| episodes.len() as u64 >= total)
        {
            break;
        }
        offset += page_len;
        pause(config.delay);
    }
    Ok(episodes)
}

/// 在单个事务中 upsert 动画和其全部剧集.
fn write_package(db: &mut Client, package: &AnimePackage) -> Result<()> {
    let mut transaction = db.transaction().context("开始动画同步事务失败")?;
    // 写入或更新 bangumi_anime 的业务字段并刷新更新时间.
    {
        transaction.execute(
            utils::sql!("bgm/upsert_anime.sql"),
            &[
                &package.anime_id,
                &package.title,
                &package.title_cn,
                &package.airdate,
                &package.summary,
                &package.ep_count,
                &package.object_count,
                &package.cover_url,
                &package.official_site,
            ],
        ).context("upsert bangumi_anime 失败")?;
    }

    // 写入或更新动画的剧集记录, 不删除 API 本次未返回的历史记录.
    {
        let statement = transaction.prepare(
            utils::sql!("bgm/upsert_episode.sql"),
        ).context("准备剧集 upsert 语句失败")?;
        for episode in &package.episodes {
            transaction
                .execute(
                    &statement,
                    &[
                        &episode.episode_id,
                        &package.anime_id,
                        &episode.title,
                        &episode.title_cn,
                        &episode.airdate,
                        &episode.summary,
                        &episode.ep,
                        &episode.sort,
                        &episode.duration,
                    ],
                )
                .with_context(|| format!("upsert episode {} 失败", episode.episode_id))?;
        }
    }

    transaction.commit().context("提交动画同步事务失败")?;
    Ok(())
}

/// 将可选日期字符串转换为 PostgreSQL date.
fn optional_date(value: &Value, key: &str) -> Result<Option<NaiveDate>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.is_empty() => Ok(None),
        Some(Value::String(value)) => parse_date(value)
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
fn optional_episode_label(value: &Value, key: &str) -> Result<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => Ok(Some(number.to_string())),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(value) => bail!("{key} 必须是数字, 字符串或 null, 实际为 {value}"),
    }
}

/// 按请求间隔暂停, 零延迟时不阻塞.
fn pause(delay: Duration) {
    if !delay.is_zero() {
        thread::sleep(delay);
    }
}
