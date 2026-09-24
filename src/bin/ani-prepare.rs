use std::path::PathBuf;

use anime_tool::prepare::{default_output_dir, prepare};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "ani-prepare",
    version,
    about = "复制媒体文件并将字幕轨道抽取为 ASS"
)]
struct Cli {
    /// Local path or HTTP(S)/FTP URL of the media file.
    media_path: String,

    /// Output directory. Defaults to <media name> - ani.
    #[arg(long = "output_dir")]
    output_dir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let output_dir = cli
        .output_dir
        .unwrap_or(default_output_dir(&cli.media_path)?);
    let manifest = prepare(&cli.media_path, &output_dir)?;
    println!("已准备媒体目录: {}", manifest.output_dir.display());
    println!("已抽取 {} 个字幕轨道", manifest.subtitle_tracks.len());
    Ok(())
}