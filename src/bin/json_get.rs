use anime_tool::{json_path, markdown};
use anyhow::{Result, anyhow};
use clap::Parser;
use serde_json::Value;

#[derive(Debug, Parser)]
#[command(about = "读取 JSON 文件中的指定项目")]
struct Args {
    /// 格式为 <json_file>:<json_path>
    target: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let (file_path, json_path_value, data) = markdown::read_json_value(&args.target)?;
    if !file_path.is_file() {
        return Err(anyhow!("JSON 文件不存在: {}", file_path.display()));
    }
    let result = json_path::resolve(&data, &json_path_value)?;
    if let Value::String(value) = result {
        println!("{}", value);
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}
