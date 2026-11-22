# src/bin 程序功能和外部组件报告

本文依据 `src/bin/*.rs` 和相关共享模块整理, 共 24 个可执行程序.
`<...>` 表示必填参数, `[...]` 表示可选参数, `...` 表示可以重复.
除特别说明外, 表中程序名对应 `src/bin/<程序名>.rs`.

使用 Cargo 运行的通用形式为:

```text
cargo run --bin <程序名> -- <程序参数>
```

使用 Clap 的程序支持 `-h` / `--help`; 配置了版本信息的程序还支持
`-V` / `--version`.
`clean-path`, `media` 和 `qb` 不解析命令行参数, 不应将 `--help` 当作
安全的只读操作. `json_get` 则会将第一个参数当作查询表达式.

## 共用配置

### PostgreSQL

多数 PostgreSQL 工具读取当前工作目录下的 `.env`, 已存在的进程环境变量优先:

| 变量          | 用途                               |
| ------------- | ---------------------------------- |
| `PG_HOST`     | 数据库主机                         |
| `PG_PORT`     | 可选端口, 省略时使用驱动默认值     |
| `PG_USER`     | 数据库用户名                       |
| `PG_PASSWORD` | 数据库密码                         |
| `PG_DB`       | 使用配置数据库的命令所需的数据库名 |

`set-ids`, `bgm`, `mikan`, `ddp`, `ddp-pkg`, `ddp-danmaku-db`
和 `kg-play` 固定连接 `main` 数据库, 不使用 `PG_DB` 选择数据库.
`qb`, `media`, `media-identify` 和 `clean-path` 使用 `PG_DB`.
`ods2pg` 使用 `PG_DB`, 也可通过命令行指定数据库连接.
这些连接使用 PostgreSQL 驱动的 `NoTls`.

### Dandanplay 通用选项

以下选项由 `ddp-anime`, `ddp-danmaku`, `ddp-match`, `ddp`,
`ddp-pkg`, `ddp-danmaku-db`, `media-identify` 和 `render-ass` 接受:

| 参数                         | 环境变量               | 用途                   |
| ---------------------------- | ---------------------- | ---------------------- |
| `-c`, `--cache-dir <DIR>`    | `ANIME_TOOL_CACHE_DIR` | 缓存目录               |
| `-a`, `--appid <APPID>`      | `ANIME_TOOL_APPID`     | Dandanplay AppID       |
| `-s`, `--appsecret <SECRET>` | `ANIME_TOOL_APPSECRET` | Dandanplay AppSecret   |
| `-f`, `--config <FILE>`      | 无                     | 指定 JSON 全局配置文件 |

实际使用哪一项取决于程序: `ddp-anime`, `ddp-danmaku`, `ddp-match`
和 `render-ass` 使用缓存目录; PostgreSQL 同步命令不使用该目录.
`render-ass` 不请求 API, 不需要认证信息.
配置优先级为命令行选项, 对应环境变量, JSON 配置文件.
默认配置文件为 `$XDG_CONFIG_HOME/anime-tool/config.json`;
未设置时使用系统配置目录, 再回退到 `.config/anime-tool/config.json`.

`ddp-anime`, `ddp-danmaku` 和 `ddp-match` 还支持:

- `-q`, `--query`: 强制请求并覆盖缓存.
- `--no-query`: 禁止请求 API, 缓存不存在时报错.

二者互斥; 默认优先使用已有缓存. 这三个缓存工具使用固定的
`https://api.dandanplay.net` API 地址. 使用共享 `EnvConfig` 的数据库同步工具
可通过 `DANDANPLAY_API_BASE` 覆盖 API 地址; `REQUEST_TIMEOUT` 和
`REQUEST_DELAY` 分别配置超时和请求间隔, 默认 15 秒和 0.2 秒;
单次请求的程序不会执行请求间隔等待.

## Bangumi 和 Dandanplay

### [bgm](../src/bin/bgm.rs)

- 功能: 从 `main.public.ids.bgm_ani_id` 读取去重 ID, 获取动画详情和全部剧集分页,
  逐动画事务化更新 `bangumi_anime` 和 `bangumi_episode`.
- 输入参数: 无位置参数. `BANGUMI_API_BASE` 可覆盖 API 地址,
  `REQUEST_TIMEOUT` 和 `REQUEST_DELAY` 控制请求.
- 外部组件: PostgreSQL `main`; Bangumi HTTP API 的 `/v0/subjects/{id}`
  和 `/v0/episodes`; 当前目录 `.env`.

### [bgm-pkg](../src/bin/bgm-pkg.rs)

- 功能: 获取 Bangumi 动画及其完整剧集, 可先保存 JSON 缓存, 再写入 SQLite 的
  `bangumi_anime` 和 `bangumi_episode`. 无缓存时直接获取并写入.
- 输入参数: `-d`, `--database <DATABASE>` 必填; 可选 `[IDS_FILE]`,
  `-c`, `--cache-dir <DIR>`, `--overwrite`, `--delay <SECONDS>`,
  `--timeout <SECONDS>` 和 `--api-base <URL>`. ID 文件每行一个 ID.
  请求间隔默认 0.2 秒, 超时默认 15 秒, API 地址默认 `https://api.bgm.tv`.
  不给 ID 文件但给缓存目录时只导入现有 JSON; 两者均不提供时从 SQLite 的
  `bangumi_anime_ids` 读取 ID.
- 子命令: `bgm-pkg set-id -d <DATABASE> [IDS_FILE]` 根据文本文件或现有
  `bangumi_anime` 的 ID 重建 `bangumi_anime_ids` 表, 会删除其原有内容.
- 外部组件: SQLite 数据库, 可选本地 ID 文件及 JSON 缓存目录,
  Bangumi HTTP API `/v0/subjects/{id}` 和 `/v0/episodes`.
  仅导入现有缓存时不需要网络请求.

### [ddp](../src/bin/ddp.rs)

- 功能: 从 `main.public.ids.ddp_ani_id` 读取动画 ID, 获取详情并逐动画事务化
  更新 `dandanplay_anime` 和 `dandanplay_episode`.
- 输入参数: 无位置参数; 接受 Dandanplay 通用选项.
- 外部组件: PostgreSQL `main`; 带签名认证的 Dandanplay 动画详情 API;
  当前目录 `.env` 和可选 JSON 配置文件.

### [ddp-pkg](../src/bin/ddp-pkg.rs)

- 功能: 同步单个动画及其剧集到 PostgreSQL, 不依赖 `ids` 表.
- 输入参数: `<ddp_ani_id>` 必须为正整数; 接受 Dandanplay 通用选项.
- 外部组件: PostgreSQL `main` 的 `dandanplay_anime` 和 `dandanplay_episode`;
  Dandanplay 动画详情 API; `.env` 和可选 JSON 配置文件.

### [ddp-anime](../src/bin/ddp-anime.rs)

- 功能: 根据动画 ID 获取 Dandanplay 动画详情, 将原始 JSON 保存为本地缓存.
- 输入参数: `<anime_id>`; Dandanplay 通用选项以及 `--query` / `--no-query`.
- 外部组件: Dandanplay `/api/v2/bangumi/{anime_id}`; 本地配置和
  `<cache-dir>/dandanplay/anime/<anime_id>.json`.

### [ddp-danmaku](../src/bin/ddp-danmaku.rs)

- 功能: 根据剧集 ID 获取弹幕, 将原始 JSON 保存为本地缓存.
- 输入参数: `<episode_id>`; Dandanplay 通用选项以及 `--query` / `--no-query`.
- 外部组件: Dandanplay `/api/v2/comment/{episode_id}`; 本地配置和
  `<cache-dir>/dandanplay/danmaku/<episode_id>.json`.

### [ddp-danmaku-db](../src/bin/ddp-danmaku-db.rs)

- 功能: 先检查剧集是否存在, 再获取并解析单集弹幕, 事务化 upsert 到数据库;
  不删除本次响应中缺失的历史弹幕.
- 输入参数: `<ddp_epi_id>` 必须为正整数; 接受 Dandanplay 通用选项.
- 外部组件: PostgreSQL `main` 的 `dandanplay_episode` 和 `dandanplay_danmaku`;
  Dandanplay 弹幕 API; `.env` 和可选 JSON 配置文件.

### [ddp-match](../src/bin/ddp-match.rs)

- 功能: 读取媒体大小和前 16 MiB 的 MD5, 调用 Dandanplay `hashOnly` 匹配,
  缓存并输出 JSON 结果.
- 输入参数: `<file_path>` 为本地路径或 HTTP, HTTPS, FTP, `file://` 地址;
  接受 Dandanplay 通用选项以及 `--query` / `--no-query`.
- 外部组件: 本地文件或 HTTP Range / FTP 服务; Dandanplay `/api/v2/match`;
  本地配置和 `<cache-dir>/dandanplay/match/<md5>.json`.
  即使使用已有缓存, 仍需先读取媒体信息和前缀哈希.

### [media-identify](../src/bin/media-identify.rs)

- 功能: 查询 `ddp_epi_id IS NULL` 且有关联路径的媒体, 使用数据库中的文件名,
  大小和前 16 MiB MD5 调用 `hashOnly` 匹配. 精确匹配写入剧集 ID,
  未精确匹配写入 `-1`, 请求失败的记录保留未识别状态.
- 输入参数: 无位置参数; 接受 Dandanplay 通用选项, 以及共享 API 环境配置.
- 外部组件: `PG_DB` 指定的 PostgreSQL, 包括 `media_file`, `file`,
  `path_file`; Dandanplay 匹配 API; `.env` 和可选 JSON 配置文件.
  匹配时使用数据库记录, 不重新读取视频内容.

## 订阅, 种子和媒体索引

### [mikan](../src/bin/mikan.rs)

- 功能: 从 `main.public.ids.mikan_rss_id` 读取订阅 ID, 获取并解析蜜柑 RSS,
  逐订阅更新 `mikan_rss` 和 `mikan_torrent`; 不下载 torrent 文件.
- 输入参数: 无位置参数. `MIKAN_RSS_BASE` 可覆盖 RSS 地址,
  `REQUEST_TIMEOUT` 和 `REQUEST_DELAY` 控制请求.
- 外部组件: PostgreSQL `main`; 蜜柑 HTTP RSS 接口; 当前目录 `.env`.

### [qb](../src/bin/qb.rs)

- 功能: 登录 qBittorrent, 获取种子列表, 文件列表和原始 torrent,
  更新种子和文件索引, 计算新增文件的 SHA-256 和前 16 MiB MD5.
  第一阶段会替换 `in_qb` 中的现有种子列表.
- 输入参数: 不解析命令行参数. 环境变量 `QB_URL`, `QB_USERNAME`,
  `QB_PASSWORD` 必填, `QB_PORT` 可选; 另需 PostgreSQL `PG_*` 和 `PG_DB`.
- 外部组件: qBittorrent Web API 的登录, `torrents/info`, `torrents/files`,
  `torrents/export`; `PG_DB` 指定的 PostgreSQL 的 `in_qb`, `torrent`,
  `torrent_file`, `file`, `path_file`; 本机可访问的下载文件和 `.env`.

### [media](../src/bin/media.rs)

- 功能: 递归扫描指定目录中的常见视频文件, 为新文件计算 SHA-256 和
  前 16 MiB MD5, 同步文件和路径并登记到 `media_file`; 不清理已失效路径.
- 输入参数: 不解析命令行参数. `MEDIA_DIRS` 是必填的 JSON 路径字符串数组,
  路径必须为本机可访问的绝对目录; 另需 `PG_*` 和 `PG_DB`.
- 外部组件: 本地或已挂载目录中的视频文件; `PG_DB` 指定的 PostgreSQL 的
  `file`, `path_file`, `media_file`; 当前目录 `.env`.

### [clean-path](../src/bin/clean-path.rs)

- 功能: 检查数据库中记录的绝对路径, 删除不存在或不是普通文件的
  `path_file` 记录; 非本机绝对路径跳过, 其他文件系统检查错误会中止.
- 输入参数: 不解析命令行参数; 通过 `PG_*` 和 `PG_DB` 配置数据库.
- 外部组件: 本机文件系统和已挂载目录; `PG_DB` 指定的 PostgreSQL
  `public.path_file`; 当前目录 `.env`.

### [scan-files](../src/bin/scan-files.rs)

- 功能: 递归扫描一个或多个目录, 将文件的所在目录和文件名写入 SQLite.
  会删除并重建目标数据库的 `file` 表.
- 输入参数: `--db <SQLITE_DB> <DIR> [DIR ...]`; 目录可以是本机可访问的
  挂载路径或 UNC 路径.
- 外部组件: 指定目录及文件系统; 指定的 SQLite 数据库.

## ID 和数据导入

### [set-ids](../src/bin/set-ids.rs)

- 功能: 读取三列 ID 文本, 在单个事务中清空并替换 PostgreSQL `main.public.ids`
  的数据, 不删除表结构.
- 输入参数: `<input>`; 每行依次为 `bgm_id`, 可选 `mikan_id`,
  可选 `ddp_id`, 使用空格或 Tab 分隔; Tab 可保留中间空列.
  空行和 `#` 开头的行跳过, ID 必须为正的 32 位整数.
- 外部组件: 本地文本文件; PostgreSQL `main` 的 `public.ids`; 当前目录 `.env`.

### [set-db-ids](../src/bin/set-db-ids.rs)

- 功能: 从文本导入最多三列 ID 到 SQLite, 会删除并重建 `ids` 表;
  相同 `bgm_id` 由后出现的行覆盖.
- 输入参数: `<input> <db>`; 每行按空格或 Tab 分隔
  `bgm_id [mikan_id] [ddp_id]`, 最多读取前三列, 第一列必填.
- 外部组件: 本地文本文件; 指定的 SQLite 数据库.

### [ods2pg](../src/bin/ods2pg.rs)

- 功能: 按列映射读取 ODS 工作表实际使用范围, 逐行独立事务 upsert 到
  PostgreSQL 现有表. 单行解析或写入失败不阻止其他行, 最终报告失败并返回非零.
- 输入参数:

  ```text
  <ods-file> <sheet-name> <table-name>
  <pk-column> [x] <pk-name> [更多主键映射]
  -
  <column> <name> [更多普通字段映射]
  [--pg-host <host>] [--pg-port <port>] [--pg-user <user>]
  [--pg-password <password>] [--pg-database <db>]
  ```

  `--pg-db` 是 `--pg-database` 的别名. `-` 必须作为独立参数,
  两侧都至少提供一个映射; 主键映射的 `x` 为非空文本添加 `\x` 前缀.
  ODS 列使用 `A`, `B` 等字母, 数据库表名可使用 `schema.table`,
  省略 schema 时使用 `public`. 不指定行范围, 工作表实际范围的首行也作为数据.

- 外部组件: 本地 ODS 文件; PostgreSQL 目标表及系统目录; 当前目录 `.env`.
  数据库选项优先于 `PG_*` 环境变量. 不创建目标表.
- 详细说明: [ods2pg.md](ods2pg.md).

## 弹幕和媒体处理

### [ani-prepare](../src/bin/ani-prepare.rs)

- 功能: 将媒体复制到输出目录, 使用 `ffprobe` 查找字幕轨并调用 `ffmpeg`
  抽取为 ASS, 同时生成 `ani.json` 清单.
- 输入参数: `<media_path> [--output_dir <DIR>]`. 默认目录为
  `<媒体文件名去扩展名> - ani`; 输入支持本地文件, `file://`, HTTP,
  HTTPS 和 FTP 地址.
- 外部组件: 本地文件系统或 HTTP / FTP 服务; `PATH` 中的 `ffprobe` 和
  `ffmpeg`; 输出目录中的媒体副本, ASS 文件和 `ani.json`.
  同名目标媒体文件会被替换.

### [render-ass](../src/bin/render-ass.rs)

- 功能: 将指定剧集的 Dandanplay 弹幕 JSON 缓存渲染为 ASS 字幕.
- 输入参数: `<episode_id>`; Dandanplay 通用选项中的缓存和配置选项;
  `--output_path <FILE>`, `--font <NAME>`, `--font-size <NUMBER>`,
  `--alpha <0..1>`, `--bold`, `--border <NUMBER>`, `--offset <SECONDS>`.
  默认输出到当前目录的 `<episode_id>.ass`.
- 外部组件: 本地配置, `dandanplay/danmaku/<episode_id>.json` 缓存和输出 ASS
  文件; 不请求 Dandanplay API, 不需要 MPV 或数据库.

### [merge-ass](../src/bin/merge-ass.rs)

- 功能: 合并已有字幕 ASS 和弹幕 ASS.
- 输入参数: `<subtitle_ass> <danmaku_ass> [--output_path <FILE>]`.
  默认输出为当前目录下 `<字幕名> - <弹幕名>.ass`.
- 外部组件: 本地 ASS 输入和输出文件; 不需要网络或数据库.

### [kg-play](../src/bin/kg-play.rs)

- 功能: 读取剧集弹幕, 生成 ASS 并作为 MPV 次字幕轨播放; 通过终端命令实时
  调整字号, 透明度, 描边, 偏移, 速度, 密度和显隐.
- 输入参数: `<media> <ddp_epi_id>`; 可选 `--font`, `--font-size`, `--alpha`,
  `--bold`, `--no-bold`, `--border`, `--offset`, `--speed`, `--density`,
  `--danmaku-fps`, `--hidden`, `--visible`. 剧集 ID 必须为正整数,
  媒体可以是 MPV 支持的本地路径或网络地址. 字号范围为 `8..=60`, 描边为
  `0..=8`, 偏移为 `-600..=600` 秒, 速度为 `0.5..=3.0`, 密度为
  `0.1..=1.0`, 弹幕帧率为 `0..=240`, 其中 `0` 跟随原视频.
- 外部组件: Linux 交互式终端; PostgreSQL `main.public.dandanplay_danmaku`;
  MPV 子进程和 Unix socket JSON IPC; 临时 ASS 和日志文件; 媒体源.
  `KG_PLAY_MPV` 可指定 MPV 路径; `KG_PLAY_FONT`, `KG_PLAY_FONT_SIZE`,
  `KG_PLAY_ALPHA`, `KG_PLAY_BOLD`, `KG_PLAY_BORDER`, `KG_PLAY_OFFSET`,
  `KG_PLAY_SPEED`, `KG_PLAY_DENSITY`, `KG_PLAY_VISIBLE` 可设置默认样式.
- 说明: 启动选项 `--alpha` 接受 `0.05..=1.0`, 当前代码默认 `0.7`;
  终端交互命令 `alpha` 当前只接受 `0.05..=0.7`.
  详细参数和交互命令参见 [kg-play.md](kg-play.md), 其透明度默认值及范围
  与当前源码存在差异, 应以源码为准.

## JSON 和 Markdown

### [json_get](../src/bin/json_get.rs)

- 功能: 从 JSON 文件中按项目的 JSON 路径表达式提取数据并格式化输出.
- 输入参数: `'<json文件路径>:<JSON路径>'`; 未给参数时从标准输入读取同样
  的表达式. 使用最后一个冒号分割文件路径和 JSON 路径.
- 外部组件: 本地 JSON 文件和标准输入 / 输出; 不访问数据库或网络.

### [md_update](../src/bin/md_update.rs)

- 功能: 解析 Markdown 各标题区域的 `meta` 中的 `match` 和 `to`,
  从指定 JSON 路径读取数据并创建或覆盖对应 fenced 代码块.
- 输入参数: `<input>` 为 Markdown 文件路径.
- 外部组件: 本地 Markdown 和其 meta 指定的 JSON 文件; 原地修改输入
  Markdown 文件.

### [md2table](../src/bin/md2table.rs)

- 功能: 将 Markdown 标题及 `meta` 整理为表格, 可输出带 UTF-8 BOM 的
  Tab 分隔文本, 也可导入 SQLite.
- 输入参数: `<input> [-o <FILE> | --output <FILE>] [--to-db <SQLITE_DB:TABLE>]`;
  输出文件和数据库选项可以同时使用.
- 外部组件: 本地 Markdown 和可选 TSV 文件; 可选 SQLite 数据库.
  使用 `--to-db` 时会删除并重建指定表.
