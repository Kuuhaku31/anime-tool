use anime_tool::{json_path, markdown};
use anyhow::{Context, Result, anyhow};
use clap::Parser;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "根据 Markdown meta 的 match/to 从 JSON 更新 fenced 区域")]
struct Args {
    /// 需要原地编辑的 Markdown 文件
    input: PathBuf,
}

fn load_match(match_value: &str, cache: &mut HashMap<PathBuf, Value>) -> Result<Value> {
    let (path, path_expr) = markdown::parse_match(match_value)?;
    let path = path.canonicalize().unwrap_or(path);

    if !cache.contains_key(&path) {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("读取 JSON 失败: {}", path.display()))?;
        let value: Value = serde_json::from_str(&text)
            .with_context(|| format!("JSON 解析失败: {}", path.display()))?;
        cache.insert(path.clone(), value);
    }

    let root = cache.get(&path).expect("JSON cache entry must exist");
    json_path::resolve(root, &path_expr)
}

fn process_section(
    lines: &mut Vec<String>,
    start: usize,
    end: usize,
    cache: &mut HashMap<PathBuf, Value>,
) -> Result<bool> {
    let Some((meta_start, meta_end)) = markdown::find_meta_blocks(lines, start, end)
        .first()
        .copied()
    else {
        return Ok(false);
    };
    let meta = markdown::parse_meta(&lines[meta_start + 1..meta_end]);
    let Some(match_value) = meta.get("match") else {
        return Ok(false);
    };
    let Some(to_value) = meta.get("to") else {
        return Ok(false);
    };
    let target = to_value.trim();

    if target.is_empty() {
        return Ok(false);
    }

    let source = load_match(match_value, cache)?;
    let output = json_path::format_json(&source, 80);
    let content: Vec<String> = output.lines().map(str::to_string).collect();

    // 目标区域存在时原地覆盖, 否则创建.
    if let Some((block_start, block_end)) =
        markdown::find_named_code_block(lines, start, end, target)
    {
        lines.splice(block_start + 1..block_end, content);
        return Ok(true);
    }

    let mut insert_at = end;

    while insert_at > start && lines[insert_at - 1].trim().is_empty() {
        insert_at -= 1;
    }

    let mut insertion = Vec::new();

    if insert_at > start && !lines[insert_at - 1].trim().is_empty() {
        insertion.push(String::new());
    }

    insertion.push(format!("```{target}"));
    insertion.extend(content);
    insertion.push("```".to_string());
    insertion.push(String::new());

    lines.splice(insert_at..insert_at, insertion);

    Ok(true)
}

fn main() -> Result<()> {
    let args = Args::parse();

    if !args.input.is_file() {
        return Err(anyhow!("Markdown 文件不存在: {}", args.input.display()));
    }

    let text = std::fs::read_to_string(&args.input)
        .with_context(|| format!("读取 Markdown 失败: {}", args.input.display()))?;
    let newline = if text.contains("\r\n") {
        "\r\n"
    } else if text.contains('\r') {
        "\r"
    } else {
        "\n"
    };
    let had_trailing_newline = text.ends_with('\n') || text.ends_with('\r');
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let sections = markdown::split_sections(&lines);
    let mut cache = HashMap::new();
    let mut updated = 0usize;

    for section in sections.into_iter().rev() {
        if process_section(&mut lines, section.start, section.end, &mut cache)? {
            updated += 1;
        }
    }

    let mut output = lines.join(newline);

    if had_trailing_newline {
        output.push_str(newline);
    }

    std::fs::write(&args.input, output)
        .with_context(|| format!("写入 Markdown 失败: {}", args.input.display()))?;
    println!(
        "完成: {}, 更新 {} 个标题单元",
        args.input.display(),
        updated
    );

    Ok(())
}
