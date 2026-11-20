# Anime Tool

1. 基于 Dandanplay 弹幕网络的本地视频文件识别, 弹幕获取/渲染工具.
2. JSON 文件信息提取

## 数据库文档

- [Torrent 表结构](docs/torrent-schema.md): `public.torrent` 和 `public.torrent_file`
  的字段, 约束, 索引, 关联关系和建表 SQL.
- [Bangumi 表结构](docs/bangumi-schema.md): `public.bangumi_anime` 和
  `public.bangumi_episode` 的字段, 约束, 级联关系, 索引和建表 SQL.
- [Dandanplay 表结构](docs/dandanplay-schema.md): 动画, 剧集和弹幕表的字段,
  约束, 外键和同步行为.
- [ODS 到 PostgreSQL 导入](docs/ods2pg.md): ODS 工作表按列映射并事务化 upsert.

## 功能

### 视频文件识别

提供视频文件地址, 解析相关信息并请求 Dandanplay API 识别视频,
将返回结果原始 json 保存在缓存目录.

### 弹幕下载和渲染

1. 提供 Dandanplay 剧集 ID, 获取对应的弹幕数据, 将其原始 json 保存在缓存目录.
2. 访问本地缓存目录, 将原始 json 渲染成 ASS 文件.
3. Linux 可使用 `kg-play <media> <ddp-epi-id>` 实时播放媒体并显示数据库弹幕,
   详情见 [kg-play 文档](docs/kg-play.md).

### 缓存检查

检查某个番剧/剧集 ID 所对应的缓存文件是否存在.

## 用法

- `ddp-match <file_path> [OPTIONS]`

  利用 Dandanplay API 进行视频文件识别, 使用 `hashOnly` 模式,
  结果保存在缓存目录: `<cache_dir>/dandanplay/match/<文件前16MiB HASH>.json` 中.
  执行该命令时, 先检查缓存目录中是否存在对应的识别结果, 若存在则无动作.

  `[OPTIONS]`:
  - `file_path` 支持 http/https/ftp 等网络协议.
  - `--[no-]query | -<n|q>` 设置强制重新识别并刷新缓存,
    或者是在缓存不存在时不进行获取, 直接返回错误.

- `ddp-danmaku <episode_id> [OPTIONS]`

  利用 Dandanplay API 获取对应剧集的弹幕数据,
  结果保存在缓存目录: `<cache_dir>/dandanplay/danmaku/<episode_id>.json` 中.
  执行该命令时, 先检查缓存目录中是否存在对应的弹幕数据, 若存在则无动作.

  `[OPTIONS]`:
  - `--[no-]query | -<n|q>` 设置强制重新获取并刷新缓存,
    或者是在缓存不存在时不进行获取, 直接返回错误.

- `ddp-anime <anime_id> [OPTIONS]`

  利用 Dandanplay API 获取对应番剧的详细信息,
  结果保存在缓存目录: `<cache_dir>/dandanplay/anime/<anime_id>.json` 中.
  执行该命令时, 先检查缓存目录中是否存在对应的番剧信息, 若存在则无动作.

  `[OPTIONS]`:
  - `--[no-]query | -<n|q>` 设置强制重新获取并刷新缓存,
    或者是在缓存不存在时不进行获取, 直接返回错误.

- `render-ass <episode_id> [OPTIONS]`

  将缓存目录中对应剧集的弹幕数据渲染成 ASS 文件, 并保存到指定路径.
  如果缓存目录中不存在对应的弹幕数据, 则报错.

  `[OPTIONS]`:
  - `--output_path <output_path>` 设置输出路径,
    未设置时默认输出到当前目录下的 `<episode_id>.ass`
  - `--font <font_name>` 设置 ASS 字体
  - `--font-size <size>` 设置字体大小 (单位: pt)
  - `--alpha <0.0-1.0>` 设置透明度
  - `--bold` 设置字体加粗
  - `--border <size>` 设置字体描边大小 (单位: pt)
  - `--offset <number>` 设置弹幕时间偏移 (单位: 秒, 正数为延迟, 负数为提前)

例子:

```bash
render-ass 123456 \
  --output_path "/tmp/123456.ass" \
  --font "Sarasa Gothic SC" \
  --font-size 24 \
  --alpha 0.9 \
  --bold \
  --border 1 \
  --offset 0.5
```

- `kg-play <media-path-or-url> <ddp-epi-id> [OPTIONS]`

  使用 MPV 播放本地或网络媒体, 从 PostgreSQL `main` 读取剧集弹幕并显示为次字幕轨.
  播放期间可在 `kg-play>` 输入命令调整弹幕, 每条命令后显示弹幕数量, 样式和 MPV 状态.
  Linux 版要求安装 MPV,
  并使用交互式终端.

  例子:

  ```bash
  kg-play "/media/Anime Episode 01.mkv" 196000012 --offset -1.5 --speed 1.2
  ```

  选项, 环境变量和终端快捷键见 [kg-play 文档](docs/kg-play.md).

- `ani-prepare <media_path> [OPTIONS]`
  1. 把视频文件复制到指定目录, 默认是当前路径下的 `<视频文件名称> - ani/` 目录,
     如果指定了 `--output_dir`, 则复制到该目录, 目录不存在时会自动创建.

     支持 http/https/ftp 等网络协议, 也支持本地文件路径.

  2. 抽取出视频文件的所有字幕轨道, 保存到输出目录 ASS 文件,
     以 `<轨道号>.<轨道名>.ass` 命名.

  3. 在目录下创建 `ani.json` 文件,
     记录 `media_path`, `output_dir` 以及抽取的字幕轨道等信息.

  `[OPTIONS]`:
  - `--output_dir <output_dir>` 设置输出文件夹路径,
    未设置时默认输出到当前路径下的 `<视频文件名称> - ani/` 目录.

- `merge-ass <字幕ASS文件> <弹幕ASS文件> [OPTIONS]`

  将字幕 ASS 文件和弹幕 ASS 文件合并, 输出到指定路径.
  实现同一个字幕轨道同时显示字幕和弹幕.
  1. 对比两个 ASS 文件的画幅比例是否相同, 如果相同则等比合并, 保证字体宽高比一致.
  2. 如果画幅比例不同, 则在保证弹幕 ASS 文件的画幅大小不变的情况下:
     - **通过增加字幕 ASS 文件的画幅的宽或者高的方式**, 使其画幅比例与弹幕 ASS 文件一致.
     - 在增加的画幅区域中, 同步调整字体宽高, 保证画幅比例变化前后字体宽高比不变.
     - 执行 1 中的等比合并操作.

  `[OPTIONS]`:
  - `--output_path <output_path>` 设置输出路径,
    未设置时默认输出到当前目录下的 `<字幕ASS文件名> - <弹幕ASS文件名>.ass`,
    会覆盖原有文件.

### 共同选项

- `--cache-dir <path> | -c <path>` 设置缓存目录
- `--appid <appid> | -a <appid>` 设置 AppID
- `--appsecret <appsecret> | -s <appsecret>` 设置 AppSecret
- `--config <path> | -f <path>` 设置全局配置文件路径,
  默认: `XDG_CONFIG_HOME/anime-tool/config.json`
- `--version | -v` 显示版本信息
- `--help | -h` 显示帮助信息, 不执行任何操作

### 蜜柑 RSS 同步

运行 `cargo run --bin mikan`, 从 PostgreSQL `main.ids.mikan_rss_id` 读取
非 NULL 且去重排序后的 ID, 顺序请求 RSS, 更新 `mikan_rss` 和 `mikan_torrent`.
数据库使用 `PG_HOST`, `PG_USER`, `PG_PASSWORD`, 可选 `PG_PORT`,
固定连接 `main`, 不使用 `PG_DB`. 支持当前目录的 `.env`, 进程环境变量优先.

请求配置沿用旧项目: `MIKAN_RSS_BASE` 默认为 `https://mikanani.me/RSS/Bangumi`,
`REQUEST_TIMEOUT` 默认为 15 秒, `REQUEST_DELAY` 默认为 0.2 秒.
显示订阅进度, 每个订阅先完整解析再独立事务写库, 先更新订阅再逐条更新种子.
同 hash 的后续条目覆盖之前的记录, 不删除已有种子, 不下载 torrent 文件,
也不使用旧脚本的本地 RSS 缓存.

无时区的发布时间按 UTC+8 解释, 带时区的时间保留其实际时刻.
单个订阅失败会报告并继续处理, 最后如有失败则返回非零退出码,
已成功订阅的数据保持提交. 表结构使用实际 `main` 库结构,
不自动创建或修改数据库表.

### PostgreSQL ID 列表覆盖导入

运行 `cargo run --bin set-ids -- /mnt/C/ani/ids.txt`, 将文本文件覆盖到
PostgreSQL `main.public.ids` 表. 需要设置 `PG_HOST`, `PG_USER`, `PG_PASSWORD`,
可选 `PG_PORT`, 支持当前目录的 `.env`, 已有环境变量优先.
数据库固定为 `main`, 不使用 `PG_DB`.

输入列顺序为 `bgm_id`, `mikan_id`, `ddp_id`, 与数据库字段排列顺序不同.
Tab 分隔时保留空列, 缺失的可选 ID 写入 NULL; 也接受空格分隔的连续列.
第一列必填, 非空 ID 必须是正的 32 位整数. 空行和以 `#` 开头的行会跳过,
重复记录原样保留. 文件中的所有行通过校验后, 才在同一事务中清空旧记录
并批量写入; 失败时回滚. 空文件或仅含空行及注释的文件会清空表.

### ODS 数据导入

运行 `cargo run --bin ods2pg -- <ods-file> <sheet> <table> <pk-column>
<pk-name> - <column> <name>`, 自动遍历工作表实际范围并按列映射写入
PostgreSQL. 支持复合主键, 全映射字段为空的行会跳过, 其他空单元格写 SQL NULL.
冲突时更新普通字段, 全部数据在单个事务中提交或回滚. 数据库参数可用
`--pg-*` 选项或 `PG_*` 环境变量配置. 详见 [ODS 导入文档](docs/ods2pg.md).

### Bangumi 动画和剧集同步

运行 `cargo run --bin bgm`, 从 PostgreSQL `main.public.ids.bgm_ani_id`
读取非 NULL 且去重排序后的 ID, 请求 Bangumi subject 和完整 episode 分页,
再刷新 `public.bangumi_anime` 和 `public.bangumi_episode`.
需要设置 `PG_HOST`, `PG_USER`, `PG_PASSWORD`, 可选 `PG_PORT`.
API 配置可用 `BANGUMI_API_BASE` (默认 `https://api.bgm.tv`),
`REQUEST_TIMEOUT` (默认 15 秒) 和 `REQUEST_DELAY` (默认 0.2 秒).
支持当前目录 `.env`, 进程环境变量优先, 数据库固定为 `main`.

每个动画独立事务, 单个 ID 同步失败会回滚该动画并继续处理其他 ID,
最终有失败时返回非零退出码. 已有动画和剧集按 Bangumi ID 更新,
刷新 `update_at`, 不删除 API 本次未返回的历史剧集.
特别注意: 数据库 `bangumi_episode.ep` 为 text, 映射 API `episode.sort`;
数据库 `bangumi_episode.sort` 为 smallint, 映射 API `episode.ep`, 不是同名映射.

### Dandanplay 动画和剧集同步

运行 `cargo run --bin ddp`, 从 PostgreSQL `main.public.ids.ddp_ani_id` 读取动画 ID,
请求 Dandanplay 番剧详情 API, 并更新 `dandanplay_anime` 和 `dandanplay_episode`.
需要设置 `PG_HOST`, `PG_USER`, `PG_PASSWORD`, 可选 `PG_PORT`,
以及全局配置或 `ANIME_TOOL_APPID`、`ANIME_TOOL_APPSECRET`.
`DANDANPLAY_API_BASE` 默认为 `https://api.dandanplay.net`,
`REQUEST_TIMEOUT` 默认为 15 秒, `REQUEST_DELAY` 默认为 0.2 秒.
数据库固定为 `main`, 支持当前目录 `.env`, 进程环境变量优先.

每个动画独立事务, 失败后继续处理其他动画. 不删除 API 本次未返回的历史剧集.
`bangumiUrl` 中的 Bangumi subject ID 写入可空的 `dandanplay_anime.bgm_ani_id`;
该字段不设外键.

运行 `cargo run --bin ddp-pkg -- <ddp_ani_id>` 可仅同步指定动画及其剧集,
不读取 `ids` 表. 配置和字段映射与 `ddp` 相同, 动画和剧集在同一事务中
插入或更新, 失败时整体回滚, 不删除 API 本次未返回的历史剧集.

### Dandanplay 弹幕同步到数据库

运行 `cargo run --bin ddp-danmaku-db -- <ddp_epi_id>`, 按指定 Dandanplay
剧集 ID 获取关联弹幕并写入 `main.public.dandanplay_danmaku`.
复用 `ddp` 命令的 Dandanplay AppID/AppSecret、`DANDANPLAY_API_BASE`,
`REQUEST_TIMEOUT` 和 `PG_*` 配置. 写入按 `(ddp_epi_id, cid)` upsert,
不删除 API 本次未返回的历史弹幕. 表结构见 [Dandanplay 表结构](docs/dandanplay-schema.md).

### qBittorrent 数据同步

`qb` 从 qBittorrent 获取种子及文件信息, 同步 PostgreSQL 的 `in_qb`,
`torrent`, `torrent_file`, `file` 和 `path_file` 表. 缺少路径索引的文件会
计算 SHA-256 和前 16 MiB 的 MD5; 无法读取的文件会报告并跳过.
文件路径必须能在运行程序的机器上访问. 数据库要求文件大小大于 0,
因此空文件也会报告并跳过.

需要设置 `QB_URL`, `QB_USERNAME`, `QB_PASSWORD`, `PG_HOST`, `PG_USER`,
`PG_PASSWORD`, `PG_DB`; `QB_PORT` 和 `PG_PORT` 可省略.
也支持从当前目录的 `.env` 加载, 已有环境变量优先. 数据库需要使用
`main` 的二进制 hash 表结构, 并包含以下结构:

```sql
CREATE TABLE public.in_qb (
    torrent_hash_v1 bytea PRIMARY KEY CHECK (octet_length(torrent_hash_v1) = 20)
);
ALTER TABLE public.file ADD COLUMN pre16mib_md5 bytea
    CHECK (pre16mib_md5 IS NULL OR octet_length(pre16mib_md5) = 16);
```

## ASS 渲染规则

- ASS 基准分辨率 1920×1080
- 字号映射: 输入字号 × 1.6, 结果限制在 18..96
- 普通弹幕: 右→左滚动
- 顶部弹幕: 5 秒固定显示
- 底部弹幕: 5 秒固定显示
- 普通滚动弹幕: 10 秒横穿屏幕
- CJK 文本按约 1 倍字号, ASCII 按约 0.6 倍字号估算宽度
- 预先分配轨道, 无法避免重叠时普通滚动弹幕丢弃
- `0xRRGGBB` 转成 ASS 的 `&HBBGGRR&`
- 接近纯黑的弹幕使用白色描边
- ASS 特殊字符进行转义
- 时间偏移在生成 Dialogue 前加入

## 缓存目录约定

缓存根目录由用户设定

### 缓存目录结构

```
<cache_dir>/
└─ dandanplay/
   ├─ danmaku/
   │  └─ <episode_id>.json
   ├─ anime/
   │  └─ <anime_id>.json
   └─ match/
      └─ <文件前16MiB HASH>.json
```

所有 Dandanplay 返回的原始 json 文件都保存在 `<cache_dir>/dandanplay/` 中.

- `danmaku/<episode_id>.json`: 弹幕数据
- `anime/<anime_id>.json`: 番剧信息, 内包含剧集列表
- `match/<文件前16MiB HASH>.json`: 视频文件识别结果

## 关键参数

优先级: 运行参数 > 环境变量 > 全局配置文件

### 缓存目录

1. 运行参数传递: `--cache-dir <path>` 或 `-c <path>`
2. 环境变量: `ANIME_TOOL_CACHE_DIR=<path>`
3. 全局配置文件: `XDG_CONFIG_HOME/anime-tool/config.json` 中的 `cache_dir` 字段

### AppID 和 AppSecret

需要由用户自行设置:

1. 运行参数传递: `--appid | -a <appid>`, `--appsecret | -s <appsecret>`
2. 环境变量: `ANIME_TOOL_APPID=<appid>`, `ANIME_TOOL_APPSECRET=<appsecret>`
3. 全局配置文件: `XDG_CONFIG_HOME/anime-tool/config.json`
   中的 `appid` 和 `appsecret` 字段

所有需要访问 Dandanplay API 的动作执行都前需要确认 AppID 和 AppSecret 就绪,
否则报错.
某些不需要访问 Dandanplay API 的动作, 如渲染 ASS 文件, 不需要 AppID 和 AppSecret.

---

# md-tools

一组用于 Markdown + meta fenced block + JSON 的命令行工具.

## 可执行文件

| Rust 可执行文件 | 用途                                                           |
| --------------- | -------------------------------------------------------------- |
| `md2table`      | 读取 Markdown 标题/meta, 输出 TSV CSV, 可选导入 SQLite         |
| `json_get`      | 按 JSON 路径读取指定值                                         |
| `md_update`     | 根据 Markdown meta 的 `match/get/to` 从 JSON 更新 fenced block |

## 架构

共通代码位于 `src/`:

- `markdown.rs`: Markdown 标题, meta, fenced block, `match` 文件路径解析.
- `json_path.rs`: JSON 路径, 数组下标和 `[key=value]` 查询.
- `sqlite.rs`: SQLite 表覆盖创建和 TEXT 字段批量写入.
- `lib.rs`: 导出共通模块.

JSON 使用 `serde_json` 的 `preserve_order` 特性, 保留对象字段顺序.
Markdown 字段使用 `IndexMap`, 保留第一次出现的字段顺序.

## md2table

每个连续 `#` 开头的标题对应一行数据, 不限制标题级别.
文件开头第一个标题之前的区域作为文件级数据项, 其 `title` 是文件名.
如果整个文件没有标题, 也至少生成文件级数据项.

`md2table` 只读取 meta, 不要求存在 `match/get/to`.

导入 SQLite 时使用:

```bash
md2table input.md -o output.csv --to-db data.db:episode
```

SQLite 中原表会先删除, 再重新创建. 所有列都是 `TEXT`.

## json_get

输入格式定义为: `<json文件路径>:<JSON路径>`

```json /mnt/C/def/583729.json
{
  "subject": {
    "id": 583729,
    "name": "Anime Name",
    "date": "2013-04-01"
  },
  "episodes": [
    {
      "id": 1687829,
      "airdate": "2013-04-07",
      "name": "Episode Name A",
      "duration": 1500
    },
    {
      "id": 1687830,
      "airdate": "2013-04-14",
      "name": "Episode Name B",
      "duration": 1600
    },
    {
      "id": 1687831,
      "name": "Episode Name C",
      "duration": 1500
    }
  ]
}
```

String -> JsonObject

输入输出示例

```json
// 输入: /mnt/C/def/583729.json:subject/date
{
  "subject": {
    "date": "2013-04-01"
  }
}
```

```json
// 支持 {} 表示获取多个字段
// 输入: /mnt/C/def/583729.json:subject/{date name}
{
  "subject": {
    "date": "2013-04-01",
    "name": "Anime Name"
  }
}
```

```json
// 对于数组, 默认返回所有元素的指定字段
// 输入: /mnt/C/def/583729.json:episodes/airdate
{
  "episodes": [
    {
      "airdate": "2013-04-07"
    },
    {
      "airdate": "2013-04-14"
    },
    {} // 哪怕没有对应字段, 也会返回一个空对象
  ]
}
```

```json
// 数组条件筛选:
// 输入: /mnt/C/def/583729.json:episodes[duration=1500]/{name airdate}
// 其中 episodes[duration=1500] 表示在 episodes 数组中筛选出 duration 等于 1500 的对象.
// 支持多级筛选, 例如 a/b[x=1]/c[y=test]/name.
{
  "episodes": [
    {
      "name": "Episode Name A",
      "airdate": "2013-04-07"
    },
    {
      "name": "Episode Name C"
    }
  ]
}
```

```json
// 数组下标筛选: 方括号中的纯数字表示按下标筛选.
// 输入: /mnt/C/def/583729.json:episodes[0]/name
{
  "episodes": [
    {
      "name": "Episode Name A"
    }
  ]
}
```

```json
// 支持递归 {<json路径> <json路径> ...}
// 输入: /mnt/C/def/583729.json:{subject/date episodes[duration=1500]/{name airdate}}
{
  "subject": {
    "date": "2013-04-01"
  },
  "episodes": [
    {
      "name": "Episode Name A",
      "airdate": "2013-04-07"
    },
    {
      "name": "Episode Name C"
    }
  ]
}
```

## md_update

每个标题单元, 以及文件开头到第一个标题之间的文件级区域,
都可以包含一个配置用的 `meta`:

````markdown
```meta
match: /path/to/583729.json:episodes[id=1687829]
to: json out
```
````

读取出 `match` 指定的 JSON 对象, 然后更新当前标题单元中的 `to`
指定的目标区域, 例如 `json out`:

````markdown
```json out
{
...
}
```
````

如果目标区域存在, 原地覆盖, 如果目标区域不存在, 则创建.

`match` 支持和 `json_get` 相同的 JSON 路径语法, 包括字段集合 `{...}`,
用于只选取部分字段并合并为一个 JSON 对象, 例如:

````markdown
```meta
match: /path/to/583729.json:subject/{summary date name}
to: json out
```
````

---
