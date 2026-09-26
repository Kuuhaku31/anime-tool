//! JSON 路径解析和取值, 供 `json_get` 和 `md_update` 共用.
//!
//! 语法:
//! - `a/b/c` 逐级访问字段.
//! - `a/{b c}` 或 `a/{b, c}` 同时取多个字段, 支持空格或逗号分隔.
//! - `{a/b c/d}` 递归字段集合, 结果按各自路径合并.
//! - 数组字段会自动对每个元素执行相同的剩余路径.
//! - `a[n]` 按下标筛选数组, `a[key=value]` 按字段值筛选数组,
//!   支持链式筛选, 例如 `a[x=1]/b[y=2]`.
//!
//! 详细语法和输出示例见 README.md 中 `json_get` 一节.

use anyhow::{Result, anyhow};
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub enum Component {
    // 访问一个 JSON 字段.
    Key { key: String, filter: Option<Filter> },

    // 在当前对象上同时执行多个 JSON 路径.
    Group(Vec<Vec<Component>>),
}

#[derive(Debug, Clone)]
pub enum Filter {
    // 按下标筛选数组, 例如 a[0].
    Index(usize),

    // 按字段值筛选数组, 例如 a[key=value].
    Eq { key: String, expected: Value },
}

/// 解析 JSON 路径字符串, 返回路径组件序列.
pub fn parse(input: &str) -> Result<Vec<Component>> {
    PathParser::new(input).parse().map_err(|err| anyhow!(err))
}

/// 按照 `json_get` 的输出规则计算路径结果:
/// 字段按原始结构包裹, 数组自动映射到每个元素,
/// 缺失的字段以空对象 `{}` 表示.
pub fn get(root: &Value, path_expr: &str) -> Result<Value> {
    let path = parse(path_expr)?;

    if path.is_empty() {
        return Ok(root.clone());
    }

    let result = eval_path(root, &path)
        .map_err(|err| anyhow!(err))?
        .unwrap_or_else(|| Value::Object(Map::new()));

    Ok(result)
}

/// 按 `字段[条件]` 语法逐级取值, 返回未包裹的原始 JSON 值.
///
/// 数组筛选只取第一个匹配项, 用于 `md_update` 的 `match` 解析,
/// 结果通常需要是单个 JSON 对象.
pub fn resolve(root: &Value, path_expr: &str) -> Result<Value> {
    let path = parse(path_expr)?;

    let mut current = root.clone();

    for component in &path {
        current = resolve_component(&current, component)?;
    }

    Ok(current)
}

fn resolve_component(current: &Value, component: &Component) -> Result<Value> {
    let Component::Key { key, filter } = component else {
        return Err(anyhow!("match 路径不支持字段集合语法 {{...}}"));
    };

    let value = current
        .get(key)
        .ok_or_else(|| anyhow!("JSON 中不存在字段: {key}"))?;

    let Some(filter) = filter else {
        return Ok(value.clone());
    };

    let array = value
        .as_array()
        .ok_or_else(|| anyhow!("字段 {key} 不是数组, 无法使用筛选条件"))?;

    match filter {
        Filter::Index(index) => array
            .get(*index)
            .cloned()
            .ok_or_else(|| anyhow!("数组 {key} 下标越界: {index}")),

        Filter::Eq { key: cond_key, expected } => array
            .iter()
            .find(|item| {
                item.get(cond_key)
                    .is_some_and(|actual| actual == expected)
            })
            .cloned()
            .ok_or_else(|| anyhow!("数组 {key} 中不存在 {cond_key}={expected} 的项目")),
    }
}

struct PathParser {
    chars: Vec<char>,
    pos: usize,
}

impl PathParser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn parse(mut self) -> Result<Vec<Component>, String> {
        self.skip_separators();

        // 空路径表示返回整个 JSON.
        if self.is_end() {
            return Ok(Vec::new());
        }

        let result = self.parse_path(None)?;

        self.skip_separators();

        if !self.is_end() {
            return Err(format!("无法解析 JSON 路径: {}", self.remaining_string()));
        }

        Ok(result)
    }

    fn parse_path(&mut self, end: Option<char>) -> Result<Vec<Component>, String> {
        let mut components = Vec::new();

        self.skip_horizontal_whitespace();

        while !self.is_end() {
            if let Some(end_char) = end {
                if self.peek() == Some(end_char) {
                    break;
                }
            }

            let component = self.parse_component()?;
            components.push(component);

            // '/' 表示继续访问下一级路径, 允许 '/' 前后有空白.
            //
            // 只在确认下一个非空白字符是 '/' 时才消耗空白,
            // 否则空白仅用于分隔 Group 中的多个路径,
            // 需要交还给调用方 (parse_group) 处理, 不能在这里消耗掉.
            let mut lookahead = self.pos;

            while lookahead < self.chars.len()
                && (self.chars[lookahead] == ' ' || self.chars[lookahead] == '\t')
            {
                lookahead += 1;
            }

            if lookahead < self.chars.len() && self.chars[lookahead] == '/' {
                self.pos = lookahead + 1;
                self.skip_horizontal_whitespace();
                continue;
            }

            // 当前字段的路径已经结束, 剩余的空白/逗号/结束符交由调用方处理.
            break;
        }

        Ok(components)
    }

    fn parse_component(&mut self) -> Result<Component, String> {
        match self.peek() {
            Some('{') => self.parse_group(),
            Some('}') => Err("JSON 路径中出现多余的 '}'".to_string()),
            Some(',') => Err("JSON 路径中出现多余的 ','".to_string()),
            Some('/') => Err("JSON 路径中出现多余的 '/'".to_string()),
            Some(_) => self.parse_key(),
            None => Err("JSON 路径意外结束".to_string()),
        }
    }

    fn parse_group(&mut self) -> Result<Component, String> {
        // 消耗 '{'.
        self.expect('{')?;

        // 空 {} 是合法的, 表示选择空对象.
        self.skip_horizontal_whitespace();

        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Component::Group(Vec::new()));
        }

        let mut paths = Vec::new();

        loop {
            self.skip_group_separators();

            if self.peek() == Some('}') {
                self.pos += 1;
                break;
            }

            if self.is_end() {
                return Err("字段集合缺少结束的 '}'".to_string());
            }

            let path = self.parse_path(Some('}'))?;

            if path.is_empty() {
                return Err("字段集合中存在空路径".to_string());
            }

            paths.push(path);

            self.skip_group_separators();

            if self.peek() == Some('}') {
                self.pos += 1;
                break;
            }

            if self.is_end() {
                return Err("字段集合缺少结束的 '}'".to_string());
            }
        }

        // Group 必须作为当前路径的最后一个组件.
        //
        // 例如:
        // episodes/{id name}
        //
        // 是合法的.
        //
        // 而:
        // {name}/foo
        //
        // 不建议使用, 这里直接拒绝, 由 eval_object 检查.
        Ok(Component::Group(paths))
    }

    fn parse_key(&mut self) -> Result<Component, String> {
        let start = self.pos;

        // 读取字段名.
        while let Some(ch) = self.peek() {
            if ch == '[' || ch == '/' || ch == ',' || ch == '}' || ch.is_whitespace() {
                break;
            }

            self.pos += 1;
        }

        if self.pos == start {
            return Err(format!(
                "字段名不能为空, 当前位置附近: {}",
                self.remaining_string()
            ));
        }

        let key: String = self.chars[start..self.pos].iter().collect();

        // 没有筛选条件.
        if self.peek() != Some('[') {
            return Ok(Component::Key { key, filter: None });
        }

        // 读取 [条件].
        self.pos += 1;

        let condition_start = self.pos;
        let mut depth = 1;

        while let Some(ch) = self.peek() {
            self.pos += 1;

            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;

                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }

        if depth != 0 {
            return Err(format!("筛选条件缺少 ']': {key}[...]"));
        }

        let condition_end = self.pos - 1;

        let condition: String = self.chars[condition_start..condition_end].iter().collect();

        let filter = parse_filter(&condition)?;

        Ok(Component::Key {
            key,
            filter: Some(filter),
        })
    }

    fn skip_separators(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() || ch == '/' || ch == ',' {
                self.pos += 1;
                continue;
            }

            break;
        }
    }

    fn skip_horizontal_whitespace(&mut self) {
        while self.peek().is_some_and(|ch| ch == ' ' || ch == '\t') {
            self.pos += 1;
        }
    }

    fn skip_group_separators(&mut self) {
        loop {
            let old = self.pos;

            self.skip_horizontal_whitespace();

            if self.peek() == Some(',') {
                self.pos += 1;
                continue;
            }

            if self.pos == old {
                break;
            }
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("期望 '{}'", expected))
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn is_end(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn remaining_string(&self) -> String {
        self.chars[self.pos..].iter().collect()
    }
}

fn parse_filter(condition: &str) -> Result<Filter, String> {
    let condition = condition.trim();

    if condition.is_empty() {
        return Err("筛选条件不能为空".to_string());
    }

    // 纯数字表示按下标筛选, 例如 episodes[0].
    if let Ok(index) = condition.parse::<usize>() {
        return Ok(Filter::Index(index));
    }

    let (key, raw_value) = condition
        .split_once('=')
        .ok_or_else(|| format!("筛选条件必须使用下标或 key=value 形式: {condition}"))?;

    let key = key.trim();
    let raw_value = raw_value.trim();

    if key.is_empty() {
        return Err(format!("筛选字段不能为空: {condition}"));
    }

    if raw_value.is_empty() {
        return Err(format!("筛选值不能为空: {condition}"));
    }

    // 优先按照 JSON 值解析.
    //
    // 1500    -> number
    // true    -> boolean
    // false   -> boolean
    // null    -> null
    // "test"  -> string
    //
    // 如果不是合法 JSON, 则按照普通字符串处理.
    let expected = match serde_json::from_str::<Value>(raw_value) {
        Ok(value) => value,
        Err(_) => Value::String(raw_value.to_string()),
    };

    Ok(Filter::Eq {
        key: key.to_string(),
        expected,
    })
}

fn eval_path(value: &Value, path: &[Component]) -> Result<Option<Value>, String> {
    if path.is_empty() {
        return Ok(Some(value.clone()));
    }

    // 数组会自动对每个元素执行相同路径.
    //
    // 例如:
    // episodes/name
    //
    // episodes 是数组, 因此会变成:
    // [
    //   {"name": ...},
    //   {"name": ...}
    // ]
    //
    // 如果某个元素不存在对应字段, 保留 {}.
    if let Value::Array(items) = value {
        let mut result = Vec::with_capacity(items.len());

        for item in items {
            match eval_path(item, path)? {
                Some(item_result) => result.push(item_result),
                None => result.push(Value::Object(Map::new())),
            }
        }

        return Ok(Some(Value::Array(result)));
    }

    let Value::Object(object) = value else {
        // 标量无法继续访问字段.
        return Ok(None);
    };

    eval_object(object, path)
}

fn eval_object(object: &Map<String, Value>, path: &[Component]) -> Result<Option<Value>, String> {
    if path.is_empty() {
        return Ok(Some(Value::Object(object.clone())));
    }

    match &path[0] {
        Component::Key { key, filter } => {
            let Some(value) = object.get(key) else {
                return Ok(None);
            };

            // 先处理筛选.
            let value = match filter {
                Some(filter) => apply_filter(value, key, filter)?,
                None => value.clone(),
            };

            // 已经到达路径末尾.
            if path.len() == 1 {
                return Ok(Some(wrap_field(key, value)));
            }

            // 继续处理剩余路径.
            let Some(result) = eval_path(&value, &path[1..])? else {
                return Ok(None);
            };

            Ok(Some(wrap_field(key, result)))
        }

        Component::Group(paths) => {
            // Group 当前必须是最后一个组件.
            if path.len() != 1 {
                return Err("字段集合 {...} 必须位于路径末尾".to_string());
            }

            eval_group(object, paths)
        }
    }
}

fn eval_group(
    object: &Map<String, Value>,
    paths: &[Vec<Component>],
) -> Result<Option<Value>, String> {
    let mut result = Map::new();

    for path in paths {
        if path.is_empty() {
            continue;
        }

        let Some(value) = eval_path(&Value::Object(object.clone()), path)? else {
            continue;
        };

        // Group 中的每个路径正常都会产生 Object.
        // 这里将这些 Object 合并.
        match value {
            Value::Object(fields) => {
                merge_object(&mut result, fields);
            }

            other => {
                return Err(format!("字段集合中的路径产生了非对象结果: {other}"));
            }
        }
    }

    Ok(Some(Value::Object(result)))
}

fn apply_filter(value: &Value, key: &str, filter: &Filter) -> Result<Value, String> {
    let Value::Array(items) = value else {
        return Err(format!("字段 '{key}' 不是数组, 无法使用筛选条件"));
    };

    let result = match filter {
        Filter::Index(index) => items.get(*index).cloned().into_iter().collect(),

        Filter::Eq {
            key: cond_key,
            expected,
        } => items
            .iter()
            .filter(|item| {
                let Value::Object(object) = item else {
                    return false;
                };

                object
                    .get(cond_key)
                    .is_some_and(|actual| actual == expected)
            })
            .cloned()
            .collect::<Vec<_>>(),
    };

    Ok(Value::Array(result))
}

fn wrap_field(key: &str, value: Value) -> Value {
    let mut result = Map::new();
    result.insert(key.to_string(), value);
    Value::Object(result)
}

fn merge_object(target: &mut Map<String, Value>, source: Map<String, Value>) {
    for (key, value) in source {
        target.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "subject": {
                "id": 1,
                "name": "Anime Name",
                "date": "2013-04-01"
            },
            "episodes": [
                {"id": 1, "airdate": "2013-04-07", "name": "A", "duration": 1500},
                {"id": 2, "airdate": "2013-04-14", "name": "B", "duration": 1600},
                {"id": 3, "name": "C", "duration": 1500}
            ]
        })
    }

    #[test]
    fn get_empty_path_returns_whole_json() {
        let root = sample();
        assert_eq!(get(&root, "").unwrap(), root);
    }

    #[test]
    fn get_single_field() {
        let root = sample();
        assert_eq!(get(&root, "subject/date").unwrap(), json!({"subject": {"date": "2013-04-01"}}));
    }

    #[test]
    fn get_group_with_space_and_comma() {
        let root = sample();
        let expected = json!({"subject": {"date": "2013-04-01", "name": "Anime Name"}});
        assert_eq!(get(&root, "subject/{date name}").unwrap(), expected);
        assert_eq!(get(&root, "subject/{date, name}").unwrap(), expected);
    }

    #[test]
    fn get_array_maps_each_element_and_keeps_empty_object_when_missing() {
        let root = sample();
        let expected = json!({"episodes": [
            {"airdate": "2013-04-07"},
            {"airdate": "2013-04-14"},
            {}
        ]});
        assert_eq!(get(&root, "episodes/airdate").unwrap(), expected);
    }

    #[test]
    fn get_eq_filter_on_array() {
        let root = sample();
        let expected = json!({"episodes": [
            {"name": "A", "airdate": "2013-04-07"},
            {"name": "C"}
        ]});
        assert_eq!(
            get(&root, "episodes[duration=1500]/{name airdate}").unwrap(),
            expected
        );
    }

    #[test]
    fn get_index_filter_on_array() {
        let root = sample();
        let expected = json!({"episodes": [{"name": "B"}]});
        assert_eq!(get(&root, "episodes[1]/name").unwrap(), expected);
    }

    #[test]
    fn get_missing_field_returns_empty_object() {
        let root = sample();
        assert_eq!(get(&root, "subject/nonexistent").unwrap(), json!({}));
    }

    #[test]
    fn resolve_single_field() {
        let root = sample();
        assert_eq!(resolve(&root, "subject").unwrap(), root["subject"].clone());
    }

    #[test]
    fn resolve_eq_filter_returns_first_match_unwrapped() {
        let root = sample();
        assert_eq!(
            resolve(&root, "episodes[duration=1500]").unwrap(),
            root["episodes"][0].clone()
        );
    }

    #[test]
    fn resolve_index_filter_unwrapped() {
        let root = sample();
        assert_eq!(
            resolve(&root, "episodes[1]").unwrap(),
            root["episodes"][1].clone()
        );
    }

    #[test]
    fn resolve_missing_filter_match_errors() {
        let root = sample();
        assert!(resolve(&root, "episodes[duration=9999]").is_err());
    }

    #[test]
    fn resolve_rejects_group_syntax() {
        let root = sample();
        assert!(resolve(&root, "subject/{date name}").is_err());
    }
}
