use std::{fs, path::PathBuf};

use anime_tool::ass_merge;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "merge-ass", version, about = "合并字幕 ASS 和弹幕 ASS")]
struct Cli {
    /// Subtitle ASS file.
    subtitle_ass: PathBuf,

    /// Danmaku ASS file.
    danmaku_ass: PathBuf,

    /// Output ASS path.
    #[arg(long = "output_path")]
    output_path: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let subtitle = fs::read_to_string(&cli.subtitle_ass)?;
    let danmaku = fs::read_to_string(&cli.danmaku_ass)?;
    let merged = ass_merge::merge(&subtitle, &danmaku)?;
    let output = cli.output_path.unwrap_or_else(|| {
        let subtitle_name = cli
            .subtitle_ass
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let danmaku_name = cli
            .danmaku_ass
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        PathBuf::from(format!("{subtitle_name} - {danmaku_name}.ass"))
    });
    fs::write(&output, merged)?;
    println!("已生成 ASS: {}", output.display());
    Ok(())
}