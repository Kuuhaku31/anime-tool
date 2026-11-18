
//! src/bin/media.rs
//! 扫描媒体目录, 并将文件内容和路径索引同步到 PostgreSQL.

use std::borrow::Borrow;
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Component, MAIN_SEPARATOR, Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use postgres::IsolationLevel;

#[path = "../kg/utils.rs"]
mod utils;
use utils::{Md5Data, Sha256Data};

/// 数组参数中每次最多写入的条目数.
const DB_BATCH_SIZE: usize = 1000;


/// 扫描得到的普通文件及其索引数据.
struct MediaFile {
    path: String,                   // 文件的 UTF-8 绝对路径
    size: u64,                      // 扫描时的字节数
    created_at: Option<SystemTime>, // 文件创建时间, 不支持时为空

    sha256: Option<Sha256Data>,    // 文件内容的 SHA-256
    pre16mib_md5: Option<Md5Data>, // 文件前 16 MiB 的 MD5
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

    if media_dirs.is_empty() { bail!("MEDIA_DIRS 至少需要一个绝对目录"); }

    // 扫描每个绝对目录, 去重重叠目录, 并生成带分隔符的数据库匹配前缀
    let mut prefixes      = HashSet::new(); // 用于存储去重后的媒体目录前缀
    let mut pending_dir_queue = Vec::new(); // 待扫描的媒体目录队列
    for dir in &media_dirs {

        #[cfg(windows)]
        let dir = PathBuf::from(database_path(dir).context("媒体目录不是有效的 UTF-8")?);

        // 检查媒体目录是否为绝对路径且不包含父目录引用 '..'
        if !dir.is_absolute() || dir.components().any(|part| part == Component::ParentDir) {
            bail!("媒体目录必须是无 '..' 的绝对路径: {}", dir.display());
        }

        let dir = dir.components().collect::<PathBuf>(); // 去除路径中的冗余组件, 例如 '.'
        let mut prefix = database_path(&dir).context("媒体目录不是有效的 UTF-8")?;
        // 确保前缀以路径分隔符结尾, 便于数据库匹配
        if !prefix.ends_with(MAIN_SEPARATOR) { prefix.push(MAIN_SEPARATOR); }

        // 如果该前缀是新的, 则加入待扫描列表; 否则忽略重复的媒体目录.
        if prefixes.insert(prefix) { pending_dir_queue.push(dir); }
        else { println!("媒体目录已被忽略: {}", dir.display()); }
    }
    let prefixes = prefixes.into_iter().collect::<Vec<_>>(); // 将前缀集合转换为向量, 便于数据库查询

    // 遍历待扫描的媒体目录队列, 扫描所有文件并去重
    let mut visited            = HashSet::new(); // 用于存储已访问的媒体目录, 避免重复扫描
    let mut existing_file_list = HashSet::new(); // 用于存储扫描到的媒体文件信息
    while let Some(dir) = pending_dir_queue.pop() {

        if !visited.insert(dir.clone()) { continue; } // 如果该目录已访问过, 则跳过

        // 读取媒体目录的条目列表
        let entries = fs::read_dir(&dir).with_context(||
            format!("读取媒体目录失败: {}", dir.display())
        )?;
        for entry in entries {

            let entry = entry.with_context(|| format!("读取媒体目录项失败: {}", dir.display()))?;
            let path = entry.path();
            let kind = entry.file_type().with_context(||
                format!("读取文件类型失败: {}", path.display())
            )?;

            if kind.is_dir() { pending_dir_queue.push(path); } // 如果是目录, 则加入待扫描队列
            else if kind.is_file() { // 如果是文件, 则读取元数据并加入集合

                let metadata = entry.metadata().with_context(||
                    format!("读取文件元数据失败: {}", path.display())
                )?;

                let path = database_path(&path).context("媒体文件路径不是有效的 UTF-8")?;
                let size = metadata.len();
                let created_at = metadata.created().ok();

                // 将扫描到的媒体文件信息加入集合, 如果已存在则替换.
                existing_file_list.replace(MediaFile {path, size, created_at, sha256: None, pre16mib_md5: None });
            }
        }
    }

    // 连接数据库并开始事务
    let mut db = env.connect_configured()?;
    let mut tx = db.build_transaction()
    .isolation_level(IsolationLevel::RepeatableRead).start()
    .context("开始媒体同步事务失败")?;

    // 从数据库中读取已有的媒体文件路径索引
    let db_file_list = tx.query(utils::sql!("media/select_paths.sql"), &[&prefixes])
    .context("读取媒体目录的路径索引失败")?;

    #[cfg(windows)]
    if db_file_list.is_empty() && !existing_file_list.is_empty() {
        eprintln!("path_file 无匹配记录, 查询前缀: {}", prefixes.join(" | "));
    }

    // 比对与写入使用同一份数据库快照, 并发修改同一行时让本次同步失败.
    let (mut to_remove, mut to_update) = (Vec::new(), Vec::new());
    for row in db_file_list {
        let path: String = row.try_get(0)?;

        // 尝试从 existing_file_list 中取出该路径对应的文件信息
        if let Some(mut file) = existing_file_list.take(path.as_str()) {

            let sha256: Vec<u8> = row.try_get(1)
            .with_context(|| format!("读取路径索引的 SHA-256 失败: {path}"))?;
            let sha256 = sha256.try_into()
            .map_err(|_| anyhow::anyhow!("路径索引的 SHA-256 不是 32 字节: {path}"))?;

            file.sha256 = Some(sha256);
            to_update.push(file); // 将更新后的文件信息加入待更新列表

        } else { to_remove.push(path); } // 如果该路径在现有文件列表中不存在, 则标记为待删除
    }
    let mut to_add: Vec<_> = existing_file_list.into_iter().collect();
    to_add.sort_unstable_by(|a, b| a.path.cmp(&b.path));  // 保持原有的路径顺序

    // 计算新增文件的哈希值, 并更新文件信息中的 SHA-256 和前缀 MD5
    let progress = utils::progress_bar(0, utils::SYNC_PROGRESS_TEMPLATE, Some("计算新文件哈希"))?;
    let mut buffer = vec![0; utils::FILE_BUFFER_SIZE];
    for file in &mut to_add {
        let (sha256, md5, size, created_at) =
        utils::hash_file(Path::new(&file.path), &mut buffer, &progress)
        .with_context(|| format!("计算媒体文件哈希失败: {}", file.path))?;
        if file.size != size as u64 {
            bail!("文件大小在扫描后发生变化: {}", file.path);
        }
        file.created_at = created_at;
        file.sha256 = Some(sha256);
        file.pre16mib_md5 = Some(md5);
    }
    progress.finish_with_message("新文件哈希计算完成");

    // 载入 SQL 语句准备
    let file_stmt   = tx.prepare(utils::sql!("qb/upsert_files.sql"    )).context("准备更新 file 失败"      )?;
    let path_stmt   = tx.prepare(utils::sql!("qb/upsert_paths.sql"    )).context("准备更新 path_file 失败" )?;
    let remove_stmt = tx.prepare(utils::sql!("media/delete_paths.sql" )).context("准备删除路径失败"        )?;
    let media_stmt  = tx.prepare(utils::sql!("media/insert_hashes.sql")).context("准备写入 media_file 失败")?;

    // 写入新增的媒体文件信息到数据库
    for chunk in to_add.chunks(DB_BATCH_SIZE) {

        // 准备数据
        let hashes : Vec<_> = chunk.iter().map(|file| file.sha256      .as_ref().map(|hash| hash.as_slice()).context("新媒体文件缺少 SHA-256")).collect::<Result<_>>()?;
        let md5s   : Vec<_> = chunk.iter().map(|file| file.pre16mib_md5.as_ref().map(|hash| hash.as_slice()).context("新媒体文件缺少前缀 MD5")).collect::<Result<_>>()?;
        let sizes  : Vec<_> = chunk.iter().map(|file| i64::try_from(file.size)                                                                ).collect::<std::result::Result<_, _>>()?;
        let paths  : Vec<_> = chunk.iter().map(|file| file.path.as_str()                                                                      ).collect();
        let times  : Vec<_> = chunk.iter().map(|file| file.created_at                                                                         ).collect();

        tx.execute(&file_stmt, &[&hashes, &md5s,   &sizes]).context("更新 file 失败")?;
        tx.execute(&path_stmt, &[&paths,  &hashes, &times]).context("更新 path_file 失败")?;
    }

    // 删除失效的媒体文件路径记录
    for chunk in to_remove.chunks(DB_BATCH_SIZE) {
        tx.execute(&remove_stmt, &[&chunk]).context("删除失效的 path_file 记录失败")?;
    }

    // 只注册内容哈希
    let mut registered = 0; // 已登记的媒体哈希数量
    let mut hashes = to_add.iter().chain(&to_update);
    loop {
        let chunk = hashes.by_ref().take(DB_BATCH_SIZE).map(|file| {
            file.sha256.as_ref().map(|hash| hash.as_slice()).context("媒体文件缺少 SHA-256")
        }).collect::<Result<Vec<_>>>()?;

        if chunk.is_empty() { break; } // 如果当前批次为空, 则退出循环

        // 登记当前批次的媒体哈希
        registered += tx.execute(&media_stmt, &[&chunk]).context("登记 media_file 失败")?;
    }
    tx.commit().context("提交媒体同步事务失败")?;

    println!(
        "媒体同步完成: 新增路径 {}, 删除路径 {}, 复用路径索引 {}, 新登记媒体哈希 {registered}",
        to_add.len(), to_remove.len(), to_update.len(),
    );
    Ok(())
}

/// 生成数据库比较和写入使用的 UTF-8 路径, 仅在 Windows 上统一为反斜杠.
fn database_path(path: &Path) -> Option<String> {
    let path = path.to_str()?;
    if cfg!(windows) { Some(path.replace('/', "\\")) }
    else { Some(path.to_owned()) }
}


// 借用原始路径
impl Borrow<str> for MediaFile {
    fn borrow(&self) -> &str { self.path.as_str() }
}
// 仅比较原始路径
impl PartialEq for MediaFile {
    fn eq(&self, other: &Self) -> bool { self.path == other.path }
}
// 仅按原始路径哈希, 支持直接用数据库返回的路径查找.
impl Eq for MediaFile {}
impl Hash for MediaFile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}
