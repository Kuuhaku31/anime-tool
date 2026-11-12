// src/bin/scan-files.rs
// 递归扫描多个文件夹, 将(所在文件夹, 文件名)重建写入 sqlite 的 file 表
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use rusqlite::{Connection, params};

/// 命令行参数
#[derive(Parser, Debug)]
#[command(
    name = "scan-files",
    version,
    about = "递归扫描文件夹并写入 sqlite 的 file 表"
)]
struct Cli {
    /// sqlite 数据库文件路径
    #[arg(long)]
    db: PathBuf,

    /// 要扫描的文件夹路径, 可多个, 支持挂载或 UNC 网络路径
    #[arg(required = true)]
    dirs: Vec<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut files: Vec<(String, String)> = Vec::new();
    for dir in &cli.dirs {
        if !dir.is_dir() {
            anyhow::bail!("不是可访问的文件夹: {}", dir.display());
        }
        scan_dir(dir, &mut files);
    }

    let mut conn = Connection::open(&cli.db)?;
    let tx = conn.transaction()?;
    tx.execute_batch(
        "DROP TABLE IF EXISTS file;
         CREATE TABLE file (
             folder TEXT NOT NULL,
             name TEXT NOT NULL
         );",
    )?;
    {
        let mut stmt = tx.prepare("INSERT INTO file (folder, name) VALUES (?1, ?2)")?;
        for (folder, name) in &files {
            stmt.execute(params![folder, name])?;
        }
    }
    tx.commit()?;

    println!(
        "已写入 {} 个文件到 {} 的 file 表",
        files.len(),
        cli.db.display()
    );
    Ok(())
}

/// 递归扫描文件夹, 收集其下所有文件; 无法读取的目录只打印警告并跳过
fn scan_dir(dir: &Path, out: &mut Vec<(String, String)>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("读取目录失败 {}: {e}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            scan_dir(&path, out);
        } else if ft.is_file() || (ft.is_symlink() && path.is_file()) {
            out.push((
                dir.to_string_lossy().into_owned(),
                entry.file_name().to_string_lossy().into_owned(),
            ));
        }
    }
}
