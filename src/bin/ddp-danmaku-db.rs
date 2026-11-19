
//! src/bin/ddp-danmaku-db.rs
//! 获取单集 Dandanplay 弹幕并 upsert 到 PostgreSQL.

use std::collections::HashSet;

use anime_tool::cli::CommonArgs;
use anyhow::{Context, Result, bail};
use clap::Parser;
use postgres::Client;
use serde_json::Value;

#[path = "../kg/utils.rs"]
mod utils;

/// 一个已解析并可写入数据库的 Dandanplay 弹幕.
struct Danmaku {
    cid: i64,                  // Dandanplay 弹幕 ID.
    appear_time: Option<f64>,  // 弹幕出现时间, 秒.
    mode: i16,                 // 弹幕模式, 默认滚动.
    color: i32,                // RGB 颜色, 默认白色.
    user_id: Option<String>,   // API 提供的用户 ID 文本.
    content: Option<String>,  // 弹幕正文.
    raw_p: Option<String>,    // API 原始参数字符串.
}

/// 单集弹幕数据库同步参数.
#[derive(Parser)]
#[command(name = "ddp-danmaku-db", version, about = "同步指定 Dandanplay 剧集的弹幕到 PostgreSQL")]
struct Cli {
    /// Dandanplay 剧集 ID.
    ddp_epi_id: i64,

    /// Dandanplay API 认证和全局配置选项.
    #[command(flatten)]
    common: CommonArgs,
}

/// 同步失败时报告完整错误链并返回非零退出码.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步 Dandanplay 弹幕失败: {error:#}");
        std::process::exit(1);
    }
}

/// 确认剧集存在后请求完整弹幕, 完整解析后在单个事务中 upsert.
fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.ddp_epi_id <= 0 {
        bail!("ddp_epi_id 必须是正的 int64");
    }

    let env = utils::EnvConfig::load(false)?;
    let mut db = env.connect_main(None)?;

    // 先检查本地剧集记录, 不存在时直接结束且不请求弹幕 API.
    let episode_exists: bool = db.query_one(
        "SELECT EXISTS (SELECT 1 FROM public.dandanplay_episode WHERE ddp_epi_id = $1)",
        &[&cli.ddp_epi_id],
    ).context("查询 Dandanplay 剧集是否存在失败")?.get(0);
    if !episode_exists {
        println!("数据库中不存在 Dandanplay 剧集 {}, 结束弹幕同步", cli.ddp_epi_id);
        return Ok(());
    }

    let ddp_config = env.ddp()?;
    let file_config = cli.common.load_config()?;
    let (appid, appsecret) = cli.common.credentials(&file_config)?;
    let client = utils::DdpApiClient::new(
        appid,
        appsecret,
        ddp_config.timeout,
        &ddp_config.api_base,
    )?;
    let body = client.danmaku(cli.ddp_epi_id, 0)
        .with_context(|| format!("获取剧集 {} 的弹幕失败", cli.ddp_epi_id))?;
    let comments = parse_response(&body)?;

    let count = save_comments(&mut db, cli.ddp_epi_id, &comments)?;
    println!("剧集 {} 弹幕同步完成, 写入或更新 {count} 条", cli.ddp_epi_id);
    Ok(())
}

/// 校验 API 数量和弹幕 ID, 无效 p 字段使用默认模式和颜色.
fn parse_response(body: &str) -> Result<Vec<Danmaku>> {
    let response: Value = serde_json::from_str(body).context("解析 Dandanplay 弹幕 JSON 失败")?;
    let object = response.as_object().context("Dandanplay 弹幕响应必须是 JSON 对象")?;
    let count = object.get("count").and_then(Value::as_i64)
        .filter(|count| *count >= 0)
        .context("弹幕响应 count 必须是非负整数")?;
    let values = match object.get("comments") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(comments)) => comments.as_slice(),
        Some(value) => bail!("弹幕响应 comments 必须是数组或 null, 实际为 {value}"),
    };
    if count != values.len() as i64 {
        bail!("弹幕响应 count 为 {count}, comments 数量为 {}", values.len());
    }

    let mut seen = HashSet::with_capacity(values.len());
    values.iter().enumerate().map(|(index, value)| {
        let comment = value.as_object()
            .with_context(|| format!("comments[{index}] 必须是对象"))?;
        let cid = utils::required_i64(comment.get("cid"), &format!("comments[{index}].cid"))?;
        if !seen.insert(cid) {
            bail!("弹幕响应包含重复 cid: {cid}");
        }
        let raw_p = optional_comment_string(comment.get("p"), "p", index)?;
        let content = optional_comment_string(comment.get("m"), "m", index)?;
        let (appear_time, mode, color, user_id) = parse_parameters(raw_p.as_deref());
        Ok(Danmaku { cid, appear_time, mode, color, user_id, content, raw_p })
    }).collect()
}

/// 读取可空 API 字符串, 错误类型不得静默转换.
fn optional_comment_string(value: Option<&Value>, key: &str, index: usize) -> Result<Option<String>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(value) => bail!("comments[{index}].{key} 必须是字符串或 null, 实际为 {value}"),
    }
}

/// 解析 API p 字段, 无效时保留原始字符串并使用数据库默认值.
fn parse_parameters(raw_p: Option<&str>) -> (Option<f64>, i16, i32, Option<String>) {
    let Some(value) = raw_p else {
        return (None, 1, 16777215, None);
    };
    let parts = value.split(',').collect::<Vec<_>>();
    if parts.len() != 4 {
        return (None, 1, 16777215, None);
    }
    let Ok(appear_time) = parts[0].parse::<f64>() else {
        return (None, 1, 16777215, None);
    };
    let Ok(mode) = parts[1].parse::<i16>() else {
        return (None, 1, 16777215, None);
    };
    let Ok(color) = parts[2].parse::<i32>() else {
        return (None, 1, 16777215, None);
    };
    if !appear_time.is_finite()
        || appear_time < 0.0
        || !matches!(mode, 1 | 4 | 5)
        || !(0..=16777215).contains(&color)
    {
        return (None, 1, 16777215, None);
    }
    (Some(appear_time), mode, color, Some(parts[3].to_owned()))
}

/// 在单集事务中保存全部弹幕, 保留未被本次 API 返回的历史行.
fn save_comments(db: &mut Client, episode_id: i64, comments: &[Danmaku]) -> Result<u64> {
    if comments.is_empty() {
        return Ok(0);
    }
    let mut transaction = db.transaction().context("开始弹幕同步事务失败")?;
    let statement = transaction.prepare(utils::sql!("ddp/upsert_danmaku.sql"))
        .context("准备弹幕 upsert 失败")?;
    let mut affected = 0u64;
    for comment in comments {
        affected += transaction.execute(&statement, &[
            &episode_id,
            &comment.cid,
            &comment.appear_time,
            &comment.mode,
            &comment.color,
            &comment.user_id,
            &comment.content,
            &comment.raw_p,
        ]).with_context(|| format!("写入弹幕 {} 失败", comment.cid))?;
    }
    transaction.commit().context("提交弹幕同步事务失败")?;
    Ok(affected)
}
