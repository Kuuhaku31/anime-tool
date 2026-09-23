use clap::Parser;

use anime_tool::{
    api::DandanplayClient,
    cache::Cache,
    cli::{CommonArgs, QueryArgs},
};

#[derive(Parser, Debug)]
#[command(name = "ddp-anime", version, about = "获取 Dandanplay 番剧详情")]
struct Cli {
    #[command(flatten)]
    common: CommonArgs,

    #[command(flatten)]
    query: QueryArgs,

    /// Dandanplay anime ID.
    anime_id: i64,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = cli.common.load_config()?;
    let cache_dir = cli.common.cache_dir(&config)?;
    let cache = Cache::new(cache_dir);
    let cache_file = cache.anime_file(cli.anime_id);

    println!("anime_id: {}", cli.anime_id);
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
    let body = client.anime(cli.anime_id)?;

    cache.write_raw_json(&cache_file, &body)?;
    println!("已保存原始 JSON: {}", cache_file.display());

    Ok(())
}
