// src/bin/set-db-ids.rs
// 读取文本文件中的 id 列表, 重建 sqlite 数据库的 ids 表并写入数据
use std::fs;
use std::path::PathBuf;

use clap::Parser;
use rusqlite::{Connection, params};

/// 命令行参数
#[derive(Parser, Debug)]
#[command(
    name = "set-db-ids",
    version,
    about = "将 id 文本文件写入 sqlite 的 ids 表"
)]
struct Cli {
    /// 输入文本文件, 每行最多三个以空格或 tab 分隔的整数: bgm_id mikan_id ddp_id
    input: PathBuf,

    /// sqlite 数据库文件路径
    db: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let text = fs::read_to_string(&cli.input)?;
    let rows = parse_rows(&text)?;

    let mut conn = Connection::open(&cli.db)?;
    let tx = conn.transaction()?;
    tx.execute_batch(
        "DROP TABLE IF EXISTS ids;
         CREATE TABLE ids (
             bgm_id INTEGER PRIMARY KEY,
             mikan_id INTEGER,
             ddp_id INTEGER
         );",
    )?;
    {
        // 主键重复时以后出现的行覆盖先前的行
        let mut stmt = tx
            .prepare("INSERT OR REPLACE INTO ids (bgm_id, mikan_id, ddp_id) VALUES (?1, ?2, ?3)")?;
        for (bgm, mikan, ddp) in &rows {
            stmt.execute(params![bgm, mikan, ddp])?;
        }
    }
    tx.commit()?;

    println!("已写入 {} 行到 {} 的 ids 表", rows.len(), cli.db.display());
    Ok(())
}

/// 一行数据: (bgm_id, mikan_id, ddp_id), 缺失的列为 None
type IdRow = (i64, Option<i64>, Option<i64>);

/// 解析文本, 跳过空行, 每行至多取三个整数, bgm_id 必须存在
fn parse_rows(text: &str) -> anyhow::Result<Vec<IdRow>> {
    let mut rows = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let mut nums = Vec::with_capacity(3);
        for token in line.split_whitespace().take(3) {
            let n: i64 = token
                .parse()
                .map_err(|e| anyhow::anyhow!("第 {} 行无法解析整数 {:?}: {e}", idx + 1, token))?;
            nums.push(n);
        }
        if nums.is_empty() {
            continue;
        }
        rows.push((nums[0], nums.get(1).copied(), nums.get(2).copied()));
    }
    Ok(rows)
}
