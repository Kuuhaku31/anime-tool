use anime_tool::json_path;
use anyhow::{Result, anyhow};
use std::{
    env, fs,
    io::{self, Read},
    process,
};

fn main() {
    // 优先使用命令行参数.
    // 没有命令行参数时从标准输入读取.
    let input = match env::args().nth(1) {
        Some(value) => value,
        None => {
            let mut value = String::new();

            if let Err(err) = io::stdin().read_to_string(&mut value) {
                eprintln!("Error: 读取标准输入失败: {err}");
                process::exit(1);
            }

            value.trim().to_string()
        }
    };

    if input.is_empty() {
        eprintln!("Usage: json_get '<json文件路径>:<JSON路径>'");
        process::exit(2);
    }

    if let Err(err) = run(&input) {
        eprintln!("Error: {err}");
        process::exit(1);
    }
}

fn run(input: &str) -> Result<()> {
    // 从最后一个 ':' 分离文件路径和 JSON 路径.
    //
    // 这样可以处理:
    // /mnt/C/test.json:subject/name
    // C:\test\test.json:subject/name
    let (file_path, json_path_expr) = input
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("输入格式必须为 <json文件路径>:<JSON路径>"))?;

    if file_path.is_empty() {
        return Err(anyhow!("JSON 文件路径不能为空"));
    }

    // 读取并解析 JSON.
    let text =
        fs::read_to_string(file_path).map_err(|err| anyhow!("无法读取 JSON 文件: {err}"))?;
    let root: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| anyhow!("JSON 解析失败: {err}"))?;

    // 计算 JSON 路径结果.
    let result = json_path::get(&root, json_path_expr)?;

    // 输出漂亮格式 JSON.
    let output = serde_json::to_string_pretty(&result)
        .map_err(|err| anyhow!("JSON 序列化失败: {err}"))?;

    println!("{output}");

    Ok(())
}
