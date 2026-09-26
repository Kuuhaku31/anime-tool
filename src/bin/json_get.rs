use serde_json::{Map, Value};
use std::{
    env, fs,
    io::{self, Read},
    process,
};

#[derive(Debug, Clone)]
enum Component {
    // 访问一个 JSON 字段.
    Key { key: String, filter: Option<Filter> },

    // 在当前对象上同时执行多个 JSON 路径.
    Group(Vec<Vec<Component>>),
}

#[derive(Debug, Clone)]
struct Filter {
    // 数组元素中用于筛选的字段.
    key: String,

    // 需要匹配的值.
    expected: Value,
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
        // 不建议使用, 这里直接拒绝.
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

        // 读取 [key=value].
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

fn run(input: &str) -> Result<(), String> {
    // 从最后一个 ':' 分离文件路径和 JSON 路径.
    //
    // 这样可以处理:
    // /mnt/C/test.json:subject/name
    // C:\test\test.json:subject/name
    let (file_path, json_path) = input
        .rsplit_once(':')
        .ok_or_else(|| "输入格式必须为 <json文件路径>:<JSON路径>".to_string())?;

    if file_path.is_empty() {
        return Err("JSON 文件路径不能为空".to_string());
    }

    // 读取 JSON.
    let text = fs::read_to_string(file_path).map_err(|err| format!("无法读取 JSON 文件: {err}"))?;

    // 解析 JSON.
    let root: Value = serde_json::from_str(&text).map_err(|err| format!("JSON 解析失败: {err}"))?;

    // 解析 JSON 路径.
    let parser = PathParser::new(json_path);
    let path = parser.parse()?;

    // 空路径直接返回整个 JSON.
    let result = if path.is_empty() {
        root
    } else {
        eval_path(&root, &path)?.unwrap_or_else(|| Value::Object(Map::new()))
    };

    // 输出漂亮格式 JSON.
    let output =
        serde_json::to_string_pretty(&result).map_err(|err| format!("JSON 序列化失败: {err}"))?;

    println!("{output}");

    Ok(())
}

fn parse_filter(condition: &str) -> Result<Filter, String> {
    let (key, raw_value) = condition
        .split_once('=')
        .ok_or_else(|| format!("筛选条件必须使用 key=value 形式: {condition}"))?;

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

    Ok(Filter {
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
                Some(filter) => apply_filter(value, filter)?,
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

fn apply_filter(value: &Value, filter: &Filter) -> Result<Value, String> {
    let Value::Array(items) = value else {
        return Err(format!("字段 '{}' 不是数组, 无法使用筛选条件", filter.key));
    };

    let result = items
        .iter()
        .filter(|item| {
            let Value::Object(object) = item else {
                return false;
            };

            object
                .get(&filter.key)
                .is_some_and(|actual| actual == &filter.expected)
        })
        .cloned()
        .collect::<Vec<_>>();

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
