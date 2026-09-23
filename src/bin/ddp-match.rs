use anime_tool::{
    api::DandanplayClient,
    cache::Cache,
    cli::{CommonArgs, QueryArgs},
    media,
};
use anyhow::Context;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "ddp-match",
    version,
    about = "使用 Dandanplay hashOnly 识别视频文件"
)]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,

    #[command(flatten)]
    query: QueryArgs,

    /// Local path or http/https/ftp URL.
    file_path: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = cli.common.load_config()?;
    let cache_dir = cli.common.cache_dir(&config)?;
    let info = media::inspect(&cli.file_path)
        .with_context(|| format!("解析视频文件失败: {}", cli.file_path))?;

    let cache = Cache::new(cache_dir);
    let cache_file = cache.match_file(&info.first_16m_md5);

    println!("file: {}", cli.file_path);
    println!("fileName: {}", info.name);
    println!("fileSize: {}", info.size);
    println!("fileHash: {}", info.first_16m_md5);
    println!("cache: {}", cache_file.display());

    if cache_file.is_file() && !cli.query.force_query() {
        if cli.query.no_query {
            println!("使用现有缓存（--no-query）");
        } else {
            println!("缓存已存在，不执行网络请求");
        }
        return Ok(());
    }

    if cli.query.no_query {
        anyhow::bail!("缓存不存在且指定了 --no-query");
    }

    let (appid, appsecret) = cli.common.credentials(&config)?;
    let client = DandanplayClient::new(appid, appsecret)?;
    let body = client.match_hash_only(&info.first_16m_md5, info.size)?;

    cache.write_raw_json(&cache_file, &body)?;
    println!("已保存原始 JSON: {}", cache_file.display());

    // 仅用于 CLI 反馈，不修改缓存内容。
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
        println!("{}", serde_json::to_string_pretty(&value).unwrap_or(body));
    } else {
        println!("{body}");
    }

    Ok(())
}
