
//! src/bin/ddp-pkg.rs
//! 按输入的 Dandanplay 动画 ID 获取详情并同步动画和剧集表.

use anime_tool::cli::CommonArgs;
use anyhow::{Result, bail};
use clap::Parser;

#[path = "../kg/utils.rs"]
mod utils;
#[path = "../kg/ddp_package.rs"]
mod ddp_package;

/// 单动画详情同步命令的参数.
#[derive(Parser)]
#[command(name = "ddp-pkg", version, about = "同步指定 Dandanplay 动画及其剧集到 PostgreSQL")]
struct Cli {
    ddp_ani_id  : i64,        // 需要获取并同步的 Dandanplay 动画 ID
    #[command(flatten)]
    common      : CommonArgs, // Dandanplay API 认证和全局配置选项
}

/// 同步失败时输出完整错误链并返回非零退出码.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步 Dandanplay 动画失败: {error:#}");
        std::process::exit(1);
    }
}

/// 校验动画 ID, 复用 API 配置并事务化更新指定动画及其剧集.
fn run() -> Result<()> {

    let cli = Cli::parse();
    if cli.ddp_ani_id <= 0 {
        bail!("ddp_ani_id 必须是正的 int64");
    }

    let env = utils::EnvConfig::load(false)?;
    let config = env.ddp()?;
    let file_config = cli.common.load_config()?;
    let (appid, appsecret) = cli.common.credentials(&file_config)?;
    let client = utils::DdpApiClient::new(appid, appsecret, config.timeout, &config.api_base)?;
    let mut db = env.connect_main(None)?;

    // 不依赖 ids 表, 动画和剧集任一写入失败时回滚整个动画包.
    let count = ddp_package::sync_package(&client, &mut db, cli.ddp_ani_id)?;
    println!("动画 {} 同步完成, 写入或更新 {count} 个剧集", cli.ddp_ani_id);
    Ok(())
}
