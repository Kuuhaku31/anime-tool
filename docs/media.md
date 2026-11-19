# src/bin/media.rs

媒体文件夹管理和数据库同步

## 步骤

- 从当前目录的 `.env` 或进程环境读取 `MEDIA_DIRS`, 值为 JSON 字符串数组.
  Windows 路径需要使用 JSON 转义, `.env` 中可用单引号保留反斜杠:
  ```env
  MEDIA_DIRS='["D:\\aaa", "D:\\bbb\\c", "C:\\xxx"]'
  ```
- Linux/WSL 示例: `MEDIA_DIRS='["/mnt/d/aaa", "/mnt/d/bbb/c"]'`.
  路径必须为当前系统实际可访问的绝对目录, 并与 `path_file` 的路径拼写一致.
- 使用 `PG_HOST`, 可选的 `PG_PORT`, `PG_USER`, `PG_PASSWORD`, `PG_DB` 连接 PostgreSQL.
- 运行: `cargo run --bin media`.

### 文件夹扫描

递归扫描每个媒体目录下的普通文件, 不跟随目录内的符号链接, 重叠目录只统计一次.
**只收集文件后缀是`mp4`, `mkv`等常见视频媒体文件的普通文件.**
任何扫描错误都会中止同步, 不把未读取的目录误判为已删除.
扫描结果是一份文件绝对路径列表,
记作 `existing_media_file_list: Vec<MediaFile>`, 其中

```rust
struct MediaFile {
  path: PathBuf,
  size: u64,
  created_at: Option<SystemTime>,

  sha256: Option<Sha256Data>, # 初始化始终为None
  pre16mib_md5: Option<Md5Data>, # 初始化始终为None
}
```

### 数据库业务#1

通过 `existing_media_file_list` 查找path_file表,
返回所有绝对地址相同的`absolute_path -> sha256`,
记作 `db_file_list: Vec<(PathBuf, Sha256Data)>`.

### 数据处理

遍历`db_file_list`:
- 设置 `existing_media_file_list` 中对应实例的 `sha256` 字段.
- 将该实例放到 `to_update` 集合中.

剩下的 `existing_media_file_list` 中没有 `sha256` 的实例, 即为 `to_add` 集合.

### 计算哈希

遍历 `to_add` 中的每个文件, 计算其 `sha256` 和 `pre16mib_md5` 哈希值,
并更新MediaFile实例对应字段

### 数据库业务#2

- 将`to_add`的所有`sha256 -> (pre16mib_md5, size)`插入更新file表
- 将`to_add`的所有`absolute_path -> (sha256, created_at)`更新path_file表
- 将 `to_update` 和 `to_add` 的 SHA-256 登记到 `media_file`.
  已有同哈希时保留 `ddp_epi_id`, `postiton` 和各流索引.
