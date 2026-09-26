// 针对 `json_get` 可执行文件的集成测试.
//
// 测试内容对应 README.md 中 `json_get` 一节描述的用法和示例.

use std::io::Write;
use std::process::{Command, Stdio};

fn fixture_path() -> String {
    format!(
        "{}/tests/fixtures/object.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn run_arg(json_path: &str) -> (String, String, i32) {
    let input = format!("{}:{}", fixture_path(), json_path);

    let output = Command::new(env!("CARGO_BIN_EXE_json_get"))
        .arg(&input)
        .output()
        .expect("执行 json_get 失败");

    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

fn run_stdin(input: &str) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_json_get"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动 json_get 失败");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    let output = child.wait_with_output().expect("等待 json_get 失败");

    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn empty_path_returns_whole_json() {
    let (stdout, _, code) = run_arg("");

    assert_eq!(code, 0);
    assert!(stdout.contains("\"subject\""));
    assert!(stdout.contains("\"episodes\""));
}

#[test]
fn single_field_path() {
    let (stdout, _, code) = run_arg("subject/date");

    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        "{\n  \"subject\": {\n    \"date\": \"2013-04-01\"\n  }\n}"
    );
}

#[test]
fn group_with_space_separator() {
    let (stdout, _, code) = run_arg("subject/{date name}");

    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        "{\n  \"subject\": {\n    \"date\": \"2013-04-01\",\n    \"name\": \"Anime Name\"\n  }\n}"
    );
}

#[test]
fn group_with_comma_separator() {
    let (stdout, _, code) = run_arg("subject/{date, name}");

    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        "{\n  \"subject\": {\n    \"date\": \"2013-04-01\",\n    \"name\": \"Anime Name\"\n  }\n}"
    );
}

#[test]
fn array_field_auto_maps_and_keeps_empty_object_when_missing() {
    let (stdout, _, code) = run_arg("episodes/airdate");

    assert_eq!(code, 0);
    let expected = "{\n  \"episodes\": [\n    {\n      \"airdate\": \"2013-04-07\"\n    },\n    {\n      \"airdate\": \"2013-04-14\"\n    },\n    {}\n  ]\n}";
    assert_eq!(stdout.trim(), expected);
}

#[test]
fn array_filter_with_group_fields() {
    let (stdout, _, code) = run_arg("episodes[duration=1500]/{name airdate}");

    assert_eq!(code, 0);
    let expected = "{\n  \"episodes\": [\n    {\n      \"name\": \"Episode Name A\",\n      \"airdate\": \"2013-04-07\"\n    },\n    {\n      \"name\": \"Episode Name C\"\n    }\n  ]\n}";
    assert_eq!(stdout.trim(), expected);
}

#[test]
fn recursive_group_combines_multiple_paths() {
    let (stdout, _, code) = run_arg("{subject/date episodes[duration=1500]/{name airdate}}");

    assert_eq!(code, 0);
    let expected = "{\n  \"subject\": {\n    \"date\": \"2013-04-01\"\n  },\n  \"episodes\": [\n    {\n      \"name\": \"Episode Name A\",\n      \"airdate\": \"2013-04-07\"\n    },\n    {\n      \"name\": \"Episode Name C\"\n    }\n  ]\n}";
    assert_eq!(stdout.trim(), expected);
}

#[test]
fn missing_field_returns_empty_object() {
    let (stdout, _, code) = run_arg("subject/nonexistent");

    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "{}");
}

#[test]
fn empty_group_returns_empty_object() {
    let (stdout, _, code) = run_arg("subject/{}");

    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "{\n  \"subject\": {}\n}");
}

#[test]
fn stdin_input_is_supported() {
    let (stdout, _, code) = run_stdin(&format!("{}:subject/date", fixture_path()));

    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        "{\n  \"subject\": {\n    \"date\": \"2013-04-01\"\n  }\n}"
    );
}

#[test]
fn nonexistent_file_reports_error() {
    let (_, stderr, code) = run_arg_with_file("/no/such/file.json", "subject");

    assert_eq!(code, 1);
    assert!(stderr.contains("无法读取 JSON 文件"));
}

#[test]
fn empty_input_reports_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_json_get"))
        .arg("")
        .output()
        .expect("执行 json_get 失败");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Usage:"));
}

fn run_arg_with_file(file_path: &str, json_path: &str) -> (String, String, i32) {
    let input = format!("{}:{}", file_path, json_path);

    let output = Command::new(env!("CARGO_BIN_EXE_json_get"))
        .arg(&input)
        .output()
        .expect("执行 json_get 失败");

    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}
