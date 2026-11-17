# src/bin/qb.rs

功能概要:
访问 qBittorrent Web API 获得种子及其文件信息,
更新到 PostgreSQL 数据库.

必要凭证从环境变量读取,
环境变量加载顺序: 当前命令行环境 > 工作目录下的 .env 文件 > 系统环境变量.

## 具体步骤

### 从环境变量读取必要凭证

- QB_USERNAME: qBittorrent Web API 用户名
- QB_PASSWORD: qBittorrent Web API 密码
- QB_URL: qBittorrent Web API 地址
- QB_PORT: qBittorrent Web API 端口(可省略)

- PG_HOST: PostgreSQL 数据库主机
- PG_PORT: PostgreSQL 数据库端口(可省略)
- PG_USER: PostgreSQL 数据库用户名
- PG_PASSWORD: PostgreSQL 数据库密码
- PG_DB: PostgreSQL 数据库名称

打印这些环境变量的值(密码隐藏为`******`)

### 访问 qBittorrent Web API

- 访问 `/api/v2/torrents/info` 获取种子信息, 构建 `List<Torrent>`

  ```rust
  struct Torrent {
    hash_v1: u8[20], // 使用 hash_v1, 长度固定为 20 字节
    title: String,
    category: PathBuf, // 形如 "anime/2024" 的种子分类
    contents: Vec<u8>,

    files: Option<Vec<TorrentFile>> // 初始化为 None, 后续根据 API 返回填充
  }
  ```

- 用所有 `hash_v1` 调用 `/api/v2/torrents/files` 获取种子文件信息,
  构建 `List<TorrentFile>`

  ```rust
  struct TorrentFile {
    relative_path: PathBuf, // 相对于种子根目录的文件路径
    absolute_path: PathBuf, // 文件在系统的绝对路径
  }
  ```

### 数据库业务#1

- 将所有 `torrent_hash_v1` 覆盖 `in_qb` 表: 删除该表原有内容, 插入新的
  `torrent_hash_v1` 列表
- 将所有 `torrent_hash_v1 -> (title, category, contents)`
  插入更新到 `torrent` 表
- 将所有 `(torrent_hash_v1, relative_path)`
  插入更新到 `torrent_file` 表, 对于新的纪录, `sha256` 字段留空
- 利用所有 `absolute_path` 查询数据库 `path_file` 表
- 数据库业务#1 返回: `List<T1Result>`

  ```rust
  struct T1Result {
    torrent_hash_v1: u8[20],
    relative_path: PathBuf,
    absolute_path: PathBuf,

    // 依据 (torrent_hash_v1, relative_path) 所对应的 absolute_path
    // 记录是否存在于 path_file 表, 设置 file_sha256

    file_sha256: Option<[u8; 32]>, // 如果数据库中没有对应记录则为空

    file_pre16mib_md5: Option<[u8; 16]>, // 文件至多前16mib的MD5哈希,
    // 后面会用到, 初始为空
    file_size: Option<u64>, // 后面会用到, 初始为空
    file_created_at: Option<DateTime<Utc>>, // 后面会用到, 初始为空
  }
  ```

### 数据处理

遍历 `List<T1Result>`, 对于每一条记录实例 record:

- 如果 `file_sha256` 为空, 则说明数据库中没有对应记录:
  - 尝试访问 `absolute_path` 对应的文件, 计算其 SHA256 和文件大小,
    更新 record 的 `file_sha256`, `file_pre16mib_md5`,
    `file_size`, `file_created_at` 字段,
    如果文件不存在或无法访问, 打印信息并丢弃 record
  - 将 record 放到列表A中
- 如果 `file_sha256` 不为空, 则说明数据库中已有对应记录,
  将 record 放到列表B中

### 数据库业务#2

- 遍历列表A, 对于每一条记录实例 record:
  - 将 record 记录的 `file_sha256 -> (file_pre16mib_md5, file_size)`
    插入更新到`file`表

- 再次遍历列表A, 对于每一条记录实例 record:
  - 将 `absolute_path -> (file_sha256, file_created_at, <当前时间>)`
    插入更新到`path_file`表

- 遍历列表A+B, 对于每一条记录实例 record:
  - 将 `(torrent_hash_v1, relative_path) -> file_sha256`
    插入更新到 `torrent_file` 表
