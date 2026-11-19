
//! src/bin/ddp.rs
//! 从 main.ids 获取 Dandanplay 动画并同步动画和剧集信息到 PostgreSQL.

use std::thread;

use anyhow::{Context, Result, bail};
use anime_tool::cli::CommonArgs;
use clap::Parser;

#[path = "../kg/utils.rs"]
mod utils;
#[path = "../kg/ddp_package.rs"]
mod ddp_package;

/// 无位置参数的 Dandanplay 同步命令.
#[derive(Parser)]
#[command(name = "ddp", version, about = "从 main.ids 获取 Dandanplay 动画并刷新动画和剧集表")]
struct Cli {
    /// Dandanplay 认证和全局配置选项.
    #[command(flatten)]
    common: CommonArgs,
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

        P.set_message(format!("同步动画 {anime_id}"));

        let result = ddp_package::sync_package(&ddp_client, &mut DB, *anime_id);

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

