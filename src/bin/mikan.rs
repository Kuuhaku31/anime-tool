
//! src/bin/mikan.rs
//! 从 main.public.view_ids 读取蜜柑 RSS ID, 获取订阅内容并更新订阅和种子信息.

use std::thread;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use clap::Parser;
use reqwest::blocking::Client as HttpClient;
use roxmltree::{Document, Node};
use url::Url;

#[path = "../kg/utils.rs"]
mod utils;

/// 蜜柑 RSS 扩展节点使用的命名空间, 不随请求域名改变.
const TORRENT_NS: &str = "https://mikanani.me/0.1/";

type Sha1Data   = [u8; 20];


/// 无位置参数的同步命令, 支持标准帮助和版本选项.
#[derive(Parser)]
#[command(name = "mikan", version, about = "从 main.public.view_ids 获取蜜柑 RSS 并更新种子信息")]
struct Cli {}

/// 一次 RSS 请求解析出的频道信息和种子列表.
struct Feed {
    title: Option<String>,
    torrents: Vec<MikanTorrent>,
}

/// 对应 mikan_torrent 表的可写种子信息.
struct MikanTorrent {
    hash_v1         : Sha1Data,    // 从下载地址或详情页地址提取的 20 字节 v1 hash
    file_byte_count : Option<i64>, // torrent/contentLength 或 enclosure/length 中的文件字节数

    title        : Option<String>,        // 种子标题
    description  : Option<String>,        // RSS 条目描述, 保留解码后的文本内容
    pub_datetime : Option<DateTime<Utc>>, // 发布时间, 无时区的原始值按 UTC+8 解释
}

/// 执行同步并在整体失败时输出错误链和非零退出码.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步蜜柑 RSS 失败: {error:#}");
        std::process::exit(1);
    }
}

/// 读取去重后的订阅 ID, 顺序获取并逐订阅提交, 单项失败不影响后续项.
fn run() -> Result<()> {

    let _cli = Cli::parse();
    let env = utils::EnvConfig::load(false)?;

    let config = env.mikan()?;
    let mut db = env.connect_main(Some("PG_PORT 不是有效的 Unicode"))?;

    // 读取去重后的订阅 ID
    let ids: Vec<i32> = db.query(utils::sql!("mikan/select_ids.sql"), &[]).context("读取 public.view_ids.mikan_rss_id 失败")?
    .into_iter().map(|row| row.try_get(0)).collect::<std::result::Result<_, _>>()?;
    if ids.is_empty() {
        println!("view_ids 中没有 mikan_rss_id, 无需同步");
        return Ok(());
    }

    let client = HttpClient::builder()
    .user_agent(concat!("anime-tool/", env!("CARGO_PKG_VERSION"), " Mikan-RSS"))
    .timeout(config.timeout)
    .build().context("创建 HTTP 客户端失败")?;

    // 在请求前准备语句, 尽早发现表结构问题, 并复用服务器端执行计划.
    let rss_stmt = db.prepare(utils::sql!("mikan/upsert_rss.sql"))
        .context("准备 mikan_rss 更新语句失败")?;
    let torrent_stmt = db.prepare(utils::sql!("mikan/upsert_torrent.sql"))
        .context("准备 mikan_torrent 更新语句失败")?;
    // 设置进度条
    let progress = utils::progress_bar(ids.len(), utils::SYNC_PROGRESS_TEMPLATE, None)?;

    let mut success = 0usize;
    let mut failed = 0usize;
    let mut item_count = 0usize;
    for (index, id) in ids.iter().enumerate() {
        progress.set_message(format!("获取蜜柑 RSS {id}"));
        let result = fetch_feed(&client, &config.rss_base, *id).and_then(|feed| {
            progress.set_message(format!("写入蜜柑 RSS {id}, {} 个种子", feed.torrents.len()));
            let id = *id;
            let mut tx = db.transaction().context("开始订阅事务失败")?;
            tx.execute(&rss_stmt, &[&id, &feed.title]).context("更新 mikan_rss 失败")?;
            for torrent in &feed.torrents {
                tx.execute(&torrent_stmt, &[
                    &torrent.hash_v1.as_slice(), &id, &torrent.file_byte_count,
                    &torrent.title, &torrent.description, &torrent.pub_datetime,
                ]).context("更新 mikan_torrent 失败")?;
            }
            tx.commit().context("提交订阅事务失败")?;
            Ok(feed.torrents.len())
        });
        match result {
            Ok(count) => { success += 1; item_count += count; }
            Err(error) => {
                progress.suspend(|| eprintln!("RSS {id} 同步失败: {error:#}"));
                failed += 1;
            }
        }
        progress.inc(1);
        if index + 1 < ids.len() && !config.delay.is_zero() {
            progress.set_message("等待下一次请求");
            thread::sleep(config.delay);
        }
    }

    progress.finish_with_message("蜜柑 RSS 处理完成");
    println!("成功订阅: {success}, 失败订阅: {failed}, 写入种子条目: {item_count}");
    if failed > 0 { bail!("{failed} 个订阅同步失败, 其他成功订阅已提交"); }
    Ok(())
}

/// 请求指定订阅, 校验 HTTP 状态后解析 RSS, 不下载 torrent 文件.
fn fetch_feed(client: &HttpClient, base: &Url, id: i32) -> Result<Feed> {

    /// 按名称和命名空间查找直接子节点, 不受 XML 前缀名称影响.
    fn child<'a, 'input>(node: Node<'a, 'input>, name: &str, namespace: Option<&str>) -> Option<Node<'a, 'input>> {
        node.children().find(|child| child.is_element()
            && child.tag_name().name() == name && child.tag_name().namespace() == namespace)
    }

    /// 获取直接子节点的首段文本, 解码 XML 实体及 CDATA 并去除首尾空白.
    fn child_text(node: Node<'_, '_>, name: &str, namespace: Option<&str>) -> Option<String> {
        child(node, name, namespace)?.text().map(str::trim)
            .filter(|text| !text.is_empty()).map(str::to_owned)
    }

    /// 沿用旧脚本的字段优先级, 从命名空间扩展节点和 enclosure 解析条目.
    fn parse_item(item: Node<'_, '_>) -> Result<MikanTorrent> {

        /// 按地址优先级寻找恰好 40 位且边界不是十六进制字符的 hash.
        fn extract_hash(urls: &[Option<&str>]) -> Option<[u8; 20]> {
            for url in urls.iter().flatten() {
                for token in url.split(|ch: char| !ch.is_ascii_hexdigit()).filter(|token| token.len() == 40) {
                    let mut hash = [0u8; 20];
                    if hex::decode_to_slice(token, &mut hash).is_ok() { return Some(hash); }
                }
            }
            None
        }

        /// 解析带时区日期, 无时区的蜜柑日期按用户指定的 UTC+8 解释
        fn parse_datetime(value: &str) -> Result<DateTime<Utc>> {
            if let Ok(date) = DateTime::parse_from_rfc3339(value) {
                return Ok(date.with_timezone(&Utc));
            }
            if let Ok(date) = DateTime::parse_from_rfc2822(value) {
                return Ok(date.with_timezone(&Utc));
            }
            let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
                .with_context(|| format!("无法解析发布时间: {value:?}"))?;
            let offset = FixedOffset::east_opt(8 * 60 * 60).context("创建 UTC+8 时区失败")?;
            let date = offset.from_local_datetime(&naive).single().context("转换发布时间失败")?;
            Ok(date.with_timezone(&Utc))
        }

        let torrent = child(item, "torrent", Some(TORRENT_NS)).context("缺少 torrent 节点")?;
        let enclosure = child(item, "enclosure", None).context("缺少 enclosure 节点")?;
        let torrent_url = enclosure.attribute("url").filter(|url| !url.is_empty())
        .context("缺少 enclosure.url")?;

        let page_url = child_text(item, "link", None);
        let torrent_page_url = child_text(torrent, "link", Some(TORRENT_NS));
        let hash_v1 = extract_hash(&[Some(torrent_url), page_url.as_deref(), torrent_page_url.as_deref()])
        .context("无法从下载地址或详情页提取 40 位种子 hash")?;

        let size = child_text(torrent, "contentLength", Some(TORRENT_NS));
        let file_byte_count = size.as_deref().or_else(|| enclosure.attribute("length"))
        .map(|value| value.parse::<i64>().context("种子大小不是有效的 bigint"))
        .transpose()?;

        let pub_datetime = child_text(torrent, "pubDate", Some(TORRENT_NS))
        .map(|value| parse_datetime(&value)).transpose()?;

        Ok(MikanTorrent {
            hash_v1,
            file_byte_count,
            title: child_text(item, "title", None),
            description: child_text(item, "description", None),
            pub_datetime,
        })
    }

    if id < 0 { bail!("mikan_rss_id 不能小于 0"); }
    let mut url = base.clone();
    // 覆盖已有 bangumiId, 同时保留反向代理可能需要的其他查询参数.
    let pairs: Vec<_> = url.query_pairs().filter(|(key, _)| key != "bangumiId")
        .map(|(key, value)| (key.into_owned(), value.into_owned())).collect();
    url.set_query(None);
    url.query_pairs_mut().extend_pairs(pairs).append_pair("bangumiId", &id.to_string());
    let xml = client.get(url).send().context("获取 RSS 失败")?
        .error_for_status().context("RSS 返回错误 HTTP 状态")?
        .text().context("读取 RSS 响应失败")?;
    {
        let xml: &str = &xml;
        let document = Document::parse(xml.trim_start_matches('\u{feff}')).context("解析 RSS XML 失败")?;
        let root = document.root_element();
        if !root.has_tag_name("rss") {
            bail!("响应根节点不是 rss");
        }
        let channel = child(root, "channel", None).context("RSS 缺少 channel 节点")?;
        let mut torrents = Vec::new();
        for (index, item) in channel.children().filter(|node| node.has_tag_name("item")).enumerate() {
            torrents.push(parse_item(item).with_context(|| format!("解析 item[{}] 失败", index + 1))?);
        }
        // 移除 title 开头 'Mikan Project - ' 前缀
        let title = child_text(channel, "title", None);
        let title = title.as_deref().map(|t| t.strip_prefix("Mikan Project - ").unwrap_or(t).to_string());
        Ok(Feed { title, torrents })
    }
}
