# Anime Tool

1. 基于 Dandanplay 弹幕网络的本地视频文件识别, 弹幕获取/渲染工具.
2. JSON 文件信息提取

## 功能

### 视频文件识别

提供视频文件地址, 解析相关信息并请求 Dandanplay API 识别视频,
将返回结果原始 json 保存在缓存目录.

### 弹幕下载和渲染

1. 提供 Dandanplay 剧集 ID, 获取对应的弹幕数据, 将其原始 json 保存在缓存目录.
2. 访问本地缓存目录, 将原始 json 渲染成 ASS 文件.

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

支持:

```text
data.json:episodes[0]
data.json:episodes[id=1687829]/airdate
data.json:subject/episodes[id=1687829]/name
```

其中 `episodes[id=1687829]` 会选取数组中第一个满足条件的对象.

## md_update

每个标题单元, 以及文件开头到第一个标题之间的文件级区域,
都可以包含一个配置用的 `meta`:

````markdown
```meta
match: /path/to/583729.json:episodes[id=1687829]
get: airdate name duration desc ep
to: out-area
```
````

脚本读取 `match` 指定的 JSON 对象, 从中取得 `get` 列出的字段,
然后更新当前标题单元中的:

````markdown
```out-area
...
```
````

已有字段原位置更新, 未出现的字段按照 `get` 顺序追加, 未被 `get` 指定的字段保留.
如果目标区域不存在, 则创建.

JSON 字符串中的实际换行会写成字面量 `\\n`, 保持 meta 的单行 `key: value` 结构.
