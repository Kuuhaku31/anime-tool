# src/bin/media.rs

媒体文件夹管理和数据库同步

## 步骤

- 从当前目录的 `.env` 或进程环境读取 `MEDIA_DIRS`, 值为 JSON 字符串数组.
  Windows 路径需要使用 JSON 转义, `.env` 中可用单引号保留反斜杠:
  ```env
  MEDIA_DIRS='["D:\\aaa", "D:\\bbb\\c", "C:\\xxx"]'
  ```
  也可写成 `MEDIA_DIRS='["D:/aaa", "D:/bbb/c", "C:/xxx"]'`.
  Windows 原生程序会将目录前缀和扫描到的文件路径统一为反斜杠再查询或写入.
- Linux/WSL 示例: `MEDIA_DIRS='["/mnt/d/aaa", "/mnt/d/bbb/c"]'`.
  路径必须为当前系统实际可访问的绝对目录, 并与 `path_file` 的路径拼写一致.
- 使用 `PG_HOST`, 可选的 `PG_PORT`, `PG_USER`, `PG_PASSWORD`, `PG_DB` 连接 PostgreSQL.
- 运行: `cargo run --bin media`.

### 文件夹扫描

递归扫描每个媒体目录下的普通文件, 不跟随目录内的符号链接, 重叠目录只统计一次.
任何扫描错误都会中止同步, 不把未读取的目录误判为已删除.
扫描结果是一份文件绝对路径列表,
记作 `existing_file_list: Vec<MediaFile>`, 其中

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

通过 `media_dirs` 查找path_file表,
返回 `absolute_path` 以某个目录和路径分隔符开头的记录及其 `file_sha256`.
例如 `/media/anime/` 不匹配 `/media/anime-old/`.
查询按参数值逐字匹配, 不做 SQL 转义处理, 不忽略大小写, 也不转换 UNC 和盘符等别名.
Windows 上扫描到文件但未匹配任何索引时, 会打印实际查询使用的目录前缀.

### 数据处理

比对 `existing_file_list` 和 `db_file_list`, 得到三个集合

- `to_add: Vec<MediaFile>`:
  存在于 `existing_file_list` 但不存在于 `db_file_list` 的文件
- `to_remove: Vec<PathBuf>`:
  存在于 `db_file_list` 但不存在于 `existing_file_list` 的文件
- `to_update: Vec<MediaFile>`:
  存在于 `existing_file_list` 和 `db_file_list` 中的文件

`to_update` 沿用数据库中的 SHA-256, 不检测文件内容是否被原地替换.

### 计算哈希

遍历 `to_add` 中的每个文件, 计算其 `sha256` 和 `pre16mib_md5` 哈希值,
并更新MediaFile实例对应字段

### 数据库业务#2

- 将`to_add`的所有`sha256 -> (pre16mib_md5, size)`插入更新file表
- 将`to_add`的所有`absolute_path -> (sha256, created_at)`更新path_file表
- 删除path_file表中所有`to_remove`的记录
- 将 `to_update` 和 `to_add` 的 SHA-256 登记到 `media_file`.
  已有同哈希时保留 `ddp_epi_id`, `postiton` 和各流索引.
- 所有写入使用一个事务, 失败时不会部分提交.
  删除的是路径索引, 不删除磁盘文件, `file` 记录或 `media_file` 记录.
