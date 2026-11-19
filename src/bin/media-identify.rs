
//! src/bin/media-identify.rs
//! 使用 Dandanplay 哈希匹配识别媒体文件并同步剧集 ID.

use std::{path::Path, thread};

use anime_tool::cli::CommonArgs;
use anyhow::{Context, Result, bail};
use clap::Parser;
use postgres::{Client, Row, Statement};
use serde_json::Value;

#[path = "../kg/utils.rs"]
mod utils;

/// 未精确匹配的数据库标记, 与未处理的 NULL 区分.
const NO_MATCH: i64 = -1;

/// 媒体识别命令的参数.
#[derive(Parser)]
#[command(version, about = "使用 Dandanplay 识别尚未匹配的媒体文件")]
struct Cli {
    /// Dandanplay 认证和全局配置选项.
    #[command(flatten)]
    common: CommonArgs,
}

/// 执行媒体识别并在失败时报告错误链.
fn main() {
    if let Err(error) = run() {
        eprintln!("媒体识别失败: {error:#}");
        std::process::exit(1);
    }
}

/// 查询待识别媒体, 限速调用 API 并逐条更新数据库.
fn run() -> Result<()> {
    let cli = Cli::parse();
    let env = utils::EnvConfig::load(false)?;
    let mut db = env.connect_configured()?;
    let rows = db.query(utils::sql!("media/select_unidentified.sql"), &[])
        .context("查询待识别媒体文件失败")?;
    if rows.is_empty() {
        println!("没有可识别的媒体文件");
        return Ok(());
    }

    let config = env.ddp()?;
    let file_config = cli.common.load_config()?;
    let (appid, appsecret) = cli.common.credentials(&file_config)?;
    let client = utils::DdpApiClient::new(appid, appsecret, config.timeout, &config.api_base)?;
    let update = db.prepare(utils::sql!("media/update_identification.sql"))
        .context("准备更新媒体剧集 ID 失败")?;
    let progress = utils::progress_bar(rows.len(), utils::SYNC_PROGRESS_TEMPLATE, None)?;
    let (mut matched, mut unmatched, mut skipped, mut failed) = (0, 0, 0, 0);
    for (index, row) in rows.iter().enumerate() {
        progress.set_message("匹配媒体文件");
        match identify(&client, &mut db, &update, row) {
            Ok(Some(NO_MATCH)) => unmatched += 1,
            Ok(Some(_)) => matched += 1,
            Ok(None) => skipped += 1,
            Err(error) => {
                progress.suspend(|| eprintln!("第 {} 个文件识别失败: {error:#}", index + 1));
                failed += 1;
            }
        }
        progress.inc(1);
        if index + 1 < rows.len() && !config.delay.is_zero() {
            thread::sleep(config.delay);
        }
    }
    progress.finish_with_message("媒体识别完成");
    println!("精确匹配: {matched}, 无精确匹配: {unmatched}, 已变更而跳过: {skipped}, 失败: {failed}");
    if failed > 0 {
        bail!("{failed} 个文件识别失败, 未写入失败结果");
    }
    Ok(())
}

/// 从路径提取无扩展名的文件名, 匹配后仅更新仍为 NULL 的记录.
fn identify(
    client: &utils::DdpApiClient, db: &mut Client, update: &Statement, row: &Row,
) -> Result<Option<i64>> {
    let path: &str = row.try_get("absolute_path").context("读取媒体路径失败")?;
    let result = (|| {
        let sha256: &[u8] = row.try_get("sha256").context("读取 SHA-256 失败")?;
        let md5: Option<&[u8]> = row.try_get("pre16mib_md5")
            .context("读取前 16 MiB MD5 失败")?;
        let md5 = md5.context("文件缺少前 16 MiB MD5, 无法进行哈希匹配")?;
        let size: i64 = row.try_get("byte_count").context("读取文件大小失败")?;
        if sha256.len() != 32 || md5.len() != 16 || size <= 0 {
            bail!("媒体记录的哈希长度或文件大小无效");
        }

        // 数据库路径可能来自另一个操作系统, 同时识别两种分隔符.
        let basename = path.rsplit(['/', '\\']).next().context("媒体路径为空")?;
        let name = Path::new(basename).file_stem().and_then(|stem| stem.to_str())
            .context("媒体路径缺少文件名")?;
        let body = client.match_hash_only(name, md5, size)?;
        let episode_id = parse_match(&body)?;
        let changed = db.execute(update, &[&sha256, &episode_id]).with_context(|| {
            format!("写入剧集 ID {episode_id} 失败")
        })?;
        Ok((changed != 0).then_some(episode_id))
    })();
    result.with_context(|| format!("处理媒体路径 {path} 失败"))
}

/// 仅接受 API 明确标为精确匹配的唯一剧集, 成功但非精确匹配返回 -1.
fn parse_match(body: &str) -> Result<i64> {
    let response: Value = serde_json::from_str(body).context("解析 Dandanplay 匹配响应失败")?;
    if response.get("success").and_then(Value::as_bool) != Some(true)
        || response.get("errorCode").and_then(Value::as_i64) != Some(0)
    {
        let message = response.get("errorMessage").and_then(Value::as_str).unwrap_or("无错误信息");
        bail!("Dandanplay 匹配请求失败: {message}");
    }
    match response.get("isMatched").and_then(Value::as_bool) {
        Some(false) => Ok(NO_MATCH),
        Some(true) => {
            let matches = response.get("matches").and_then(Value::as_array)
                .context("精确匹配响应缺少 matches 数组")?;
            let [matched] = matches.as_slice() else {
                bail!("精确匹配响应必须包含唯一剧集");
            };
            utils::required_i64(matched.get("episodeId"), "matches[0].episodeId")
        }
        None => bail!("匹配响应缺少有效的 isMatched 字段"),
    }
}
