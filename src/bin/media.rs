
//! src/bin/media.rs
//! 扫描媒体目录, 并将文件内容和路径索引同步到 PostgreSQL.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, MAIN_SEPARATOR, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use postgres::IsolationLevel;

#[path = "../kg/utils.rs"]
mod utils;

/// SHA-256 的 32 字节原始值.
type Sha256Data = [u8; 32];
/// 前 16 MiB MD5 的 16 字节原始值.
type Md5Data = [u8; 16];

/// 数组参数中每次最多写入的条目数.
const DB_BATCH_SIZE: usize = 1000;

/// 扫描得到的普通文件及其索引数据.
struct MediaFile {
    path: PathBuf,                   // 文件的绝对路径.
    size: u64,                       // 扫描时的字节数.
    created_at: Option<SystemTime>,  // 文件创建时间, 不支持时为空.
    sha256: Option<Sha256Data>,       // 文件内容的 SHA-256.
    pre16mib_md5: Option<Md5Data>,   // 文件前 16 MiB 的 MD5.
}

/// 同步失败时输出错误链并返回非零退出码.
fn main() {
    if let Err(error) = run() {
        eprintln!("同步媒体文件失败: {error:#}");
        std::process::exit(1);
    }
}

/// 按扫描, 比对, 计算哈希和集中写入的顺序同步.
fn run() -> Result<()> {
    let env = utils::EnvConfig::load(false)?;
    let media_dirs = env.media_dirs()?;
    let (prefixes, mut existing_file_list) = scan_dirs(&media_dirs)?;

    // 比对与写入使用同一份数据库快照, 并发修改同一行时让本次同步失败.
    let mut db = env.connect_configured()?;
    let mut tx = db.build_transaction().isolation_level(IsolationLevel::RepeatableRead)
        .start().context("开始媒体同步事务失败")?;
    let db_file_list = tx.query(utils::sql!("media/select_paths.sql"), &[&prefixes])
        .context("读取媒体目录的路径索引失败")?;
    let (mut to_add, mut to_remove, mut to_update) = (Vec::new(), Vec::new(), Vec::new());
    for row in db_file_list {
        let path: String = row.try_get(0)?;
        if let Some(mut file) = existing_file_list.remove(&path) {
            let hash: Vec<u8> = row.try_get(1)
                .with_context(|| format!("读取路径索引的 SHA-256 失败: {path}"))?;
            file.sha256 = Some(hash.try_into()
                .map_err(|_| anyhow::anyhow!("路径索引的 SHA-256 不是 32 字节: {path}"))?);
            to_update.push(file);
        } else {
            to_remove.push(path);
        }
    }
    to_add.extend(existing_file_list.into_values());

    let progress = utils::progress_bar(0, utils::SYNC_PROGRESS_TEMPLATE, Some("计算新文件哈希"))?;
    let mut buffer = vec![0; utils::FILE_BUFFER_SIZE];
    for file in &mut to_add {
        let (sha256, md5, size, created_at) = utils::hash_file(&file.path, &mut buffer, &progress)
            .with_context(|| format!("计算媒体文件哈希失败: {}", file.path.display()))?;
        if file.size != size as u64 {
            bail!("文件大小在扫描后发生变化: {}", file.path.display());
        }
        file.created_at = created_at;
        file.sha256 = Some(sha256);
        file.pre16mib_md5 = Some(md5);
    }
    progress.finish_with_message("新文件哈希计算完成");

    let file_stmt = tx.prepare(utils::sql!("qb/upsert_files.sql")).context("准备更新 file 失败")?;
    let path_stmt = tx.prepare(utils::sql!("qb/upsert_paths.sql")).context("准备更新 path_file 失败")?;
    let remove_stmt = tx.prepare(utils::sql!("media/delete_paths.sql")).context("准备删除路径失败")?;
    let media_stmt = tx.prepare(utils::sql!("media/insert_hashes.sql")).context("准备写入 media_file 失败")?;

    for chunk in to_add.chunks(DB_BATCH_SIZE) {
        let hashes: Vec<_> = chunk.iter().map(|file| file.sha256.as_ref()
            .map(|hash| hash.as_slice()).context("新媒体文件缺少 SHA-256")).collect::<Result<_>>()?;
        let md5s: Vec<_> = chunk.iter().map(|file| file.pre16mib_md5.as_ref()
            .map(|hash| hash.as_slice()).context("新媒体文件缺少前缀 MD5")).collect::<Result<_>>()?;
        let sizes: Vec<_> = chunk.iter().map(|file| i64::try_from(file.size)).collect::<std::result::Result<_, _>>()?;
        let paths: Vec<_> = chunk.iter().map(|file| file.path.to_str()
            .context("媒体文件路径不是有效的 UTF-8")).collect::<Result<_>>()?;
        let times: Vec<_> = chunk.iter().map(|file| file.created_at).collect();
        tx.execute(&file_stmt, &[&hashes, &md5s, &sizes]).context("更新 file 失败")?;
        tx.execute(&path_stmt, &[&paths, &hashes, &times]).context("更新 path_file 失败")?;
    }

    for chunk in to_remove.chunks(DB_BATCH_SIZE) {
        tx.execute(&remove_stmt, &[&chunk]).context("删除失效的 path_file 记录失败")?;
    }

    // 只注册内容哈希, 不覆盖已有的集数, 位置或流索引.
    let mut registered = 0;
    let mut hashes = to_add.iter().chain(&to_update);
    loop {
        let chunk = hashes.by_ref().take(DB_BATCH_SIZE).map(|file| file.sha256.as_ref()
            .map(|hash| hash.as_slice()).context("媒体文件缺少 SHA-256")).collect::<Result<Vec<_>>>()?;
        if chunk.is_empty() { break; }
        registered += tx.execute(&media_stmt, &[&chunk]).context("登记 media_file 失败")?;
    }
    tx.commit().context("提交媒体同步事务失败")?;
    println!(
        "媒体同步完成: 新增路径 {}, 删除路径 {}, 复用路径索引 {}, 新登记媒体哈希 {registered}",
        to_add.len(), to_remove.len(), to_update.len(),
    );
    Ok(())
}

/// 扫描每个绝对目录, 去重重叠目录, 并生成带分隔符的数据库匹配前缀.
fn scan_dirs(media_dirs: &[PathBuf]) -> Result<(Vec<String>, BTreeMap<String, MediaFile>)> {
    if media_dirs.is_empty() {
        bail!("MEDIA_DIRS 至少需要一个绝对目录");
    }
    let mut prefixes = HashSet::new();
    let mut pending = Vec::new();
    for dir in media_dirs {
        if !dir.is_absolute() || dir.components().any(|part| part == Component::ParentDir) {
            bail!("媒体目录必须是无 '..' 的绝对路径: {}", dir.display());
        }
        let dir = dir.components().collect::<PathBuf>();
        let mut prefix = dir.to_str().context("媒体目录不是有效的 UTF-8")?.to_owned();
        if !prefix.ends_with(MAIN_SEPARATOR) { prefix.push(MAIN_SEPARATOR); }
        if prefixes.insert(prefix) { pending.push(dir); }
    }
    let mut visited = HashSet::new();
    let mut files = BTreeMap::new();
    while let Some(dir) = pending.pop() {
        if !visited.insert(dir.clone()) { continue; }
        for entry in fs::read_dir(&dir).with_context(|| format!("读取媒体目录失败: {}", dir.display()))? {
            let entry = entry.with_context(|| format!("读取媒体目录项失败: {}", dir.display()))?;
            let path = entry.path();
            let kind = entry.file_type().with_context(|| format!("读取文件类型失败: {}", path.display()))?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                let metadata = entry.metadata().with_context(|| format!("读取文件元数据失败: {}", path.display()))?;
                let key = path.to_str().context("媒体文件路径不是有效的 UTF-8")?.to_owned();
                files.insert(key, MediaFile {
                    path, size: metadata.len(), created_at: metadata.created().ok(),
                    sha256: None, pre16mib_md5: None,
                });
            }
        }
    }
    Ok((prefixes.into_iter().collect(), files))
}
