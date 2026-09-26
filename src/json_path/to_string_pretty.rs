use serde_json::{Map, Value};

pub fn format_json(value: &Value, max_width: usize) -> String {
    format_value(value, 0, max_width)
}

fn to_one_line_string(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => {
            serde_json::to_string(value).expect("serde_json::Value 序列化不应该失败")
        }

        Value::String(s) => {
            let mut n = "";
            if s.contains('\n') {
                n = "\n";
            }
            format!("\"{n}{s}{n}\"")
        }

        Value::Array(array) => {
            let mut result = String::from("[");
            for (index, value) in array.iter().enumerate() {
                if index > 0 {
                    result.push(',');
                    result.push(' ');
                }
                result.push_str(&to_one_line_string(value));
            }
            result.push(']');
            result
        }

        Value::Object(object) => {
            let mut result = String::from("{");
            for (index, (key, value)) in object.iter().enumerate() {
                if index > 0 {
                    result.push(',');
                    result.push(' ');
                }
                let key = serde_json::to_string(key).expect("JSON object key 序列化不应该失败");
                result.push_str(&key);
                result.push(':');
                result.push_str(&to_one_line_string(value));
            }
            result.push('}');
            result
        }
    }
}

fn format_value(value: &Value, indent: usize, max_width: usize) -> String {
    // 先生成一行的紧凑格式, 然后检查是否超过最大宽度.
    let compact = to_one_line_string(value);

    // 当前值能够放入一行时, 直接使用紧凑格式.
    if indent + compact.len() <= max_width {
        return compact;
    }

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => compact,

        Value::String(s) => {
            // 如果 indent + 字符串长度超过最大宽度, 特殊显示
            if indent + compact.len() <= max_width {
                compact
            } else {
                format!("\"\n{}\n\"", s)
            }
        }

        Value::Array(array) => {
            if array.is_empty() {
                return "[]".to_string();
            }

            format_array(array, indent, max_width)
        }

        Value::Object(object) => {
            if object.is_empty() {
                return "{}".to_string();
            }

            format_object(object, indent, max_width)
        }
    }
}

fn format_array(array: &[Value], indent: usize, max_width: usize) -> String {
    let child_indent = indent + 2;
    let mut result = String::from("[\n");

    for (index, value) in array.iter().enumerate() {
        // 格式化数组元素.
        let formatted = format_value(value, child_indent, max_width);

        result.push_str(&" ".repeat(child_indent));
        result.push_str(&formatted);

        // 最后一个元素不加逗号.
        if index + 1 != array.len() {
            result.push(',');
        }

        result.push('\n');
    }

    result.push_str(&" ".repeat(indent));
    result.push(']');

    result
}

fn format_object(object: &Map<String, Value>, indent: usize, max_width: usize) -> String {
    let child_indent = indent + 2;
    let mut result = String::from("{\n");

    let len = object.len();

    for (index, (key, value)) in object.iter().enumerate() {
        // 使用 serde_json 处理 key 的 JSON 转义.
        let key = serde_json::to_string(key).expect("JSON object key 序列化不应该失败");

        let formatted = format_value(value, child_indent, max_width);

        result.push_str(&" ".repeat(child_indent));
        result.push_str(&key);
        result.push_str(": ");
        result.push_str(&formatted);

        if index + 1 != len {
            result.push(',');
        }

        result.push('\n');
    }

    result.push_str(&" ".repeat(indent));
    result.push('}');

    result
}

// 测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json() {
        let json = serde_json::json!({
            "name": "Alice",
            "age": 30,
            "hobbies": ["reading", "traveling", "coding"],
            "address": {
                "street": "123 Main St",
                "city": "Wonderland"
            }
        });

        let formatted = format_json(&json, 40);
        println!("{}", formatted);
    }
}
