use anyhow::{Result, anyhow};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

pub fn resolve(root: &Value, path: &str) -> Result<Value> {
    let mut current = root.clone();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        current = resolve_segment(&current, segment)?;
    }
    Ok(current)
}

fn index_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([^\[\]]+)\[(\d+)\]$").unwrap())
}

fn filter_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([^\[\]]+)\[([^=\[\]]+)=([^\[\]]*)\]$").unwrap())
}

fn resolve_segment(current: &Value, segment: &str) -> Result<Value> {
    if let Some(caps) = index_re().captures(segment) {
        let field = &caps[1];
        let index: usize = caps[2].parse()?;
        let array = current
            .get(field)
            .ok_or_else(|| anyhow!("JSON 中不存在数组字段: {field}"))?;
        let array = array
            .as_array()
            .ok_or_else(|| anyhow!("字段 {field} 不是数组"))?;
        return array
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow!("数组 {field} 下标越界: {index}"));
    }

    if let Some(caps) = filter_re().captures(segment) {
        let field = &caps[1];
        let condition_key = &caps[2];
        let expected = &caps[3];
        let array = current
            .get(field)
            .ok_or_else(|| anyhow!("JSON 中不存在数组字段: {field}"))?
            .as_array()
            .ok_or_else(|| anyhow!("字段 {field} 不是数组"))?;

        for item in array {
            let Some(actual) = item.get(condition_key) else {
                continue;
            };
            if matches(actual, expected) {
                return Ok(item.clone());
            }
        }
        return Err(anyhow!(
            "数组 {field} 中不存在 {condition_key}={expected} 的项目"
        ));
    }

    current
        .get(segment)
        .cloned()
        .ok_or_else(|| anyhow!("JSON 中不存在字段: {segment}"))
}

fn matches(value: &Value, expected: &str) -> bool {
    match value {
        Value::Null => expected.eq_ignore_ascii_case("null"),
        Value::Bool(v) => v.to_string().eq_ignore_ascii_case(expected),
        Value::Number(v) => v.to_string() == expected,
        Value::String(v) => v == expected,
        Value::Array(_) | Value::Object(_) => false,
    }
}
