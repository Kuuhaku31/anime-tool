//! src/bin/clean-path.rs
//! 检查 public.path_file 中的文件路径并删除已经失效的路径记录.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use postgres::Client;

#[path = "../kg/utils.rs"]
mod utils;

/// 每批最多从数据库中删除的路径记录数.
const DB_BATCH_SIZE: usize = 1000;

/// 运行清理并在失败时输出完整错误链.
fn main() {
    if let Err(error) = run() {
        eprintln!("清理路径索引失败: {error:#}");
        std::process::exit(1);
    }
}

/// 查询全部路径, 检查本机文件系统并删除失效路径记录.
fn run() -> Result<()> {

    let env = utils::EnvConfig::load(false)?;
    let mut db = env.connect_configured()?;

    // 查询数据库中全部路径记录.
    let paths = db
    .query(utils::sql!("media/select_all_paths.sql"), &[])
    .context("读取 public.path_file 全部路径失败")?
    .into_iter()
    .map(|row| {
        row.try_get::<_, String>(0)
        .context("读取 path_file.absolute_path 失败")
    })
    .collect::<Result<Vec<_>>>()?;

    // 检查本机文件系统中每个路径的有效性.
    let (mut invalid_paths, mut skipped_paths) = (Vec::new(), 0usize);
    for path in &paths {
        let file_path = Path::new(path);
        if !file_path.is_absolute() {
            skipped_paths += 1;
            continue;
        }
        match fs::metadata(file_path) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => invalid_paths.push(path.clone()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                invalid_paths.push(path.clone());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("检查文件路径失败: {path}"));
            }
        }
    }

    // 删除失效路径记录.
    let db   : &mut Client = &mut db;
    let paths: &[String] = &invalid_paths;
    let deleted =
    if paths.is_empty() { 0 }
    else {
        let mut transaction = db.transaction().context("开始路径清理事务失败")?;
        let statement = transaction
            .prepare(utils::sql!("media/delete_paths.sql"))
            .context("准备删除失效路径语句失败")?;

        let mut count = 0u64;
        for chunk in paths.chunks(DB_BATCH_SIZE) {
            count += transaction
            .execute(&statement, &[&chunk])
            .context("删除失效 path_file 记录失败")?;
        }

        transaction.commit().context("提交路径清理事务失败")?;
        count
    };

    println!(
        "路径清理完成: 检查 {}, 失效 {}, 删除 {}, 非本机绝对路径跳过 {}",
        paths.len(), invalid_paths.len(), deleted, skipped_paths
    );
    Ok(())
}
