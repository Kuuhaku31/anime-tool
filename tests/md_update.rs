// 针对 `md_update` 可执行文件的集成测试.
//
// 测试内容对应 README.md 中 `md_update` 一节描述的用法:
// 读取 `match` 指定的 JSON 值, 原样 (pretty) 写入 `to` 指定的目标区域,
// 目标区域存在则原地覆盖, 不存在则创建.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn fixture_json_path() -> String {
    format!(
        "{}/tests/fixtures/object.json",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// 在临时目录创建一个 Markdown 文件, 返回其路径.
fn write_temp_md(name: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "anime_tool_md_update_test_{name}_{}.md",
        std::process::id()
    ));
    fs::write(&path, content).expect("写入临时 Markdown 文件失败");
    path
}

/// 执行 md_update, 返回 (stdout, stderr, exit_code).
fn run_md_update(path: &PathBuf) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_md_update"))
        .arg(path)
        .output()
        .expect("执行 md_update 失败");

    (
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn creates_missing_target_block() {
    let json = fixture_json_path();
    let content = format!(
        "# Test Episode\n\n```meta\nmatch: {json}:episodes[id=1687829]\nto: json out\n```\n"
    );
    let path = write_temp_md("creates_missing_block", &content);

    let (stdout, stderr, code) = run_md_update(&path);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("更新 1 个标题单元"));

    let result = fs::read_to_string(&path).unwrap();
    assert!(result.contains(
        "```json out\n{\n  \"id\": 1687829,\n  \"airdate\": \"2013-04-07\",\n  \"name\": \"Episode Name A\",\n  \"duration\": 1500\n}\n```"
    ));

    fs::remove_file(&path).ok();
}

#[test]
fn overwrites_existing_target_block_in_place() {
    let json = fixture_json_path();
    let content = format!(
        "# Test Episode\n\n```meta\nmatch: {json}:episodes[id=1687830]\nto: json out\n```\n\n```json out\n{{\n  \"stale\": true\n}}\n```\n"
    );
    let path = write_temp_md("overwrite_in_place", &content);

    let (_, stderr, code) = run_md_update(&path);
    assert_eq!(code, 0, "stderr: {stderr}");

    let result = fs::read_to_string(&path).unwrap();
    assert!(!result.contains("stale"));
    assert!(result.contains(
        "```json out\n{\n  \"id\": 1687830,\n  \"airdate\": \"2013-04-14\",\n  \"name\": \"Episode Name B\",\n  \"duration\": 1600\n}\n```"
    ));

    fs::remove_file(&path).ok();
}

#[test]
fn supports_array_index_match() {
    let json = fixture_json_path();
    let content =
        format!("# Test Episode\n\n```meta\nmatch: {json}:episodes[0]\nto: json out\n```\n");
    let path = write_temp_md("index_match", &content);

    let (_, stderr, code) = run_md_update(&path);
    assert_eq!(code, 0, "stderr: {stderr}");

    let result = fs::read_to_string(&path).unwrap();
    assert!(result.contains("\"name\": \"Episode Name A\""));

    fs::remove_file(&path).ok();
}

#[test]
fn file_level_section_before_first_heading_is_updated() {
    let json = fixture_json_path();
    let content =
        format!("```meta\nmatch: {json}:subject\nto: json out\n```\n\n# Heading A\n\ncontent here\n");
    let path = write_temp_md("file_level_section", &content);

    let (_, stderr, code) = run_md_update(&path);
    assert_eq!(code, 0, "stderr: {stderr}");

    let result = fs::read_to_string(&path).unwrap();
    assert!(result.contains("\"name\": \"Anime Name\""));
    assert!(result.contains("# Heading A"));

    fs::remove_file(&path).ok();
}

#[test]
fn missing_match_target_reports_error() {
    let json = fixture_json_path();
    let content = format!(
        "# Test Episode\n\n```meta\nmatch: {json}:episodes[id=999999]\nto: json out\n```\n"
    );
    let path = write_temp_md("missing_match", &content);

    let (_, stderr, code) = run_md_update(&path);
    assert_eq!(code, 1);
    assert!(stderr.contains("不存在"));

    fs::remove_file(&path).ok();
}

#[test]
fn missing_meta_fields_skip_section() {
    let path = write_temp_md("missing_meta", "# Test\n\ncontent without meta\n");

    let (stdout, stderr, code) = run_md_update(&path);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("更新 0 个标题单元"));

    fs::remove_file(&path).ok();
}
