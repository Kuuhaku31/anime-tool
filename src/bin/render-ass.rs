
// src/bin/render-ass.rs
// 将 Dandanplay 弹幕缓存渲染为 ASS 字幕文件.

use std::fs;

use clap::Parser;

use anime_tool::{
    ass::{AssSettings, render_from_json},
    cache::Cache,
    cli::CommonArgs,
};

/// `render-ass` 命令行参数.
#[derive(Parser, Debug)]
#[command(
    name = "render-ass",
    version,
    about = "将 Dandanplay 原始弹幕 JSON 渲染为 ASS"
)]
struct Cli {
    /// 缓存目录和 API 的通用参数.
    #[command(flatten)]
    common: CommonArgs,

    /// Dandanplay 剧集 ID.
    episode_id: i64,

    /// ASS 输出路径.
    #[arg(long = "output_path")]
    output_path: Option<std::path::PathBuf>,

    /// ASS 字体名称.
    #[arg(long = "font")]
    font: Option<String>,

    /// 用户侧字体大小, 按 1.6 倍映射到 ASS 坐标.
    #[arg(long = "font-size")]
    font_size: Option<f64>,

    /// 弹幕不透明度, 范围为 0.0..=1.0.
    #[arg(long = "alpha")]
    alpha: Option<f64>,

    /// 使用粗体字体.
    #[arg(long = "bold")]
    bold: bool,

    /// ASS 描边宽度, 最大限制为 8.
    #[arg(long = "border")]
    border: Option<f64>,

    /// 弹幕时间偏移, 正值延后, 负值提前.
    #[arg(long = "offset")]
    offset: Option<f64>,
}

/// 读取缓存弹幕, 校验样式选项并写出 ASS 文件.
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = cli.common.load_config()?;
    let cache = Cache::new(cli.common.cache_dir(&config)?);
    let cache_file = cache.danmaku_file(cli.episode_id);

    if !cache_file.is_file() {
        anyhow::bail!("弹幕缓存不存在: {}", cache_file.display());
    }

    let json = fs::read_to_string(&cache_file)?;

    let defaults = AssSettings::default();
    let settings = AssSettings {
        font: cli.font.unwrap_or(defaults.font),
        font_size: cli.font_size.unwrap_or(defaults.font_size),
        alpha: cli.alpha.unwrap_or(defaults.alpha),
        bold: cli.bold,
        border: cli.border.unwrap_or(defaults.border),
        offset: cli.offset.unwrap_or(defaults.offset),
        scroll_duration: defaults.scroll_duration,
        display_area: defaults.display_area,
    };

    if !(0.0..=1.0).contains(&settings.alpha) {
        anyhow::bail!("--alpha 必须在 0.0 到 1.0 之间");
    }

    if !settings.font_size.is_finite() || settings.font_size <= 0.0 {
        anyhow::bail!("--font-size 必须为正数");
    }

    if !settings.border.is_finite() || settings.border < 0.0 {
        anyhow::bail!("--border 必须为非负数");
    }

    if !settings.offset.is_finite() {
        anyhow::bail!("--offset 必须是有限数字");
    }

    let ass = render_from_json(&json, &settings)?;

    let output = cli
        .output_path
        .unwrap_or_else(|| std::path::PathBuf::from(format!("{}.ass", cli.episode_id)));

    fs::write(&output, ass)?;

    println!("已生成 ASS: {}", output.display());
    Ok(())
}
