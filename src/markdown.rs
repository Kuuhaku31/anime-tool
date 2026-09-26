use anyhow::{Result, anyhow};
use indexmap::IndexMap;
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct Section {
    pub start: usize,
    pub end: usize,
    pub title: Option<String>,
}

pub fn split_sections(lines: &[String]) -> Vec<Section> {
    let headings: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| heading_re().is_match(line).then_some(i))
        .collect();

    let mut sections = Vec::new();
    if headings.is_empty() {
        sections.push(Section {
            start: 0,
            end: lines.len(),
            title: None,
        });
        return sections;
    }

    if headings[0] > 0 {
        sections.push(Section {
            start: 0,
            end: headings[0],
            title: None,
        });
    }

    for (i, &start) in headings.iter().enumerate() {
        let end = headings.get(i + 1).copied().unwrap_or(lines.len());
        sections.push(Section {
            start,
            end,
            title: {
                let line: &str = &lines[start];
                let caps = heading_re().captures(line).unwrap();
                let hashes = caps.get(1).unwrap().as_str();
                let title = caps.get(2).map(|m| m.as_str()).unwrap_or("").trim();
                let title = title.trim_end_matches('#').trim().to_string();
                Some((hashes.len(), title))
            }
            .map(|(_, title)| title),
        });
    }

    sections
}

pub fn parse_meta(lines: &[String]) -> IndexMap<String, String> {
    let mut values = IndexMap::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            if !key.is_empty() {
                values.insert(key.to_string(), value.trim().to_string());
            }
        }
    }
    values
}

pub fn find_meta_blocks(lines: &[String], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut blocks = Vec::new();
    let mut i = start;
    while i < end {
        if !meta_open_re().is_match(&lines[i]) {
            i += 1;
            continue;
        }
        let begin = i;
        i += 1;
        while i < end {
            if code_close_re().is_match(&lines[i]) {
                blocks.push((begin, i));
                i += 1;
                break;
            }
            i += 1;
        }
    }
    blocks
}

pub fn find_named_code_block(
    lines: &[String],
    start: usize,
    end: usize,
    target: &str,
) -> Option<(usize, usize)> {
    let mut i = start;
    while i < end {
        if meta_open_re().is_match(&lines[i]) {
            i += 1;
            while i < end && !code_close_re().is_match(&lines[i]) {
                i += 1;
            }
            i += 1;
            continue;
        }

        let Some(caps) = code_open_re().captures(&lines[i]) else {
            i += 1;
            continue;
        };
        let language = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if language == target {
            let begin = i;
            i += 1;
            while i < end {
                if code_close_re().is_match(&lines[i]) {
                    return Some((begin, i));
                }
                i += 1;
            }
            return None;
        }

        i += 1;
        while i < end && !code_close_re().is_match(&lines[i]) {
            i += 1;
        }
        i += 1;
    }
    None
}

pub fn normalize_output_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(v) => v.to_string(),
        serde_json::Value::Number(v) => v.to_string(),
        serde_json::Value::String(v) => v
            .replace("\r\n", "\\n")
            .replace('\r', "\\n")
            .replace('\n', "\\n"),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_default()
        }
    }
}

pub fn update_named_block(
    content: &mut Vec<String>,
    fields: &[String],
    source: &serde_json::Map<String, serde_json::Value>,
) {
    let parsed: Vec<(usize, String, String)> = content
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            let (key, value) = line.trim().split_once(':')?;
            let key = key.trim();
            (!key.is_empty()).then(|| (i, key.to_string(), value.trim().to_string()))
        })
        .collect();
    let wanted: std::collections::HashSet<&str> = fields.iter().map(String::as_str).collect();
    let mut existing = std::collections::HashSet::new();

    for (index, key, _) in &parsed {
        if wanted.contains(key.as_str()) {
            existing.insert(key.clone());
        }
        if let Some(value) = source.get(key) {
            content[*index] = format!("{}: {}", key, normalize_output_value(value));
            existing.insert(key.clone());
        }
    }

    while content.last().is_some_and(|line| line.trim().is_empty()) {
        content.pop();
    }

    for key in fields {
        if existing.contains(key) {
            continue;
        }
        if let Some(value) = source.get(key) {
            content.push(format!("{}: {}", key, normalize_output_value(value)));
        }
    }
}

pub fn read_json_value(value: &str) -> Result<(std::path::PathBuf, String, serde_json::Value)> {
    let (path, path_expr) = parse_match(value)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|e| anyhow!("读取 JSON 失败: {}: {e}", path.display()))?;
    let json = serde_json::from_str(&text)
        .map_err(|e| anyhow!("JSON 解析失败: {}: {e}", path.display()))?;
    Ok((path, path_expr, json))
}

pub fn parse_match(value: &str) -> Result<(std::path::PathBuf, String)> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("match 不能为空"));
    }

    let chars: Vec<(usize, char)> = value.char_indices().collect();
    for &(index, ch) in chars.iter().rev() {
        if ch != ':' {
            continue;
        }
        let left = &value[..index];
        let right = &value[index + 1..];
        if !left.is_empty() && !right.is_empty() && std::path::Path::new(left).is_file() {
            return Ok((std::path::PathBuf::from(left), right.to_string()));
        }
    }

    let (left, right) = value
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("match 必须使用 <json_file>:<json_path> 格式"))?;
    if left.is_empty() || right.is_empty() {
        return Err(anyhow!("match 必须使用 <json_file>:<json_path> 格式"));
    }
    Ok((std::path::PathBuf::from(left), right.to_string()))
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*(#+)(?:[ \t]+(.*?)|[ \t]*)[ \t]*$").unwrap())
}

fn meta_open_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*```[ \t]*meta[ \t]*$").unwrap())
}

fn code_close_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*```[ \t]*$").unwrap())
}

fn code_open_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ \t]*```(.+?)[ \t]*$").unwrap())
}
