# Anime Tool

基于 Dandanplay 弹幕网络的本地视频文件识别, 弹幕获取/渲染工具.

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

### 共同选项

- `--cache-dir <path> | -c <path>` 设置缓存目录
- `--appid <appid> | -a <appid>` 设置 AppID
- `--appsecret <appsecret> | -s <appsecret>` 设置 AppSecret
- `--config <path> | -f <path>` 设置全局配置文件路径,
  默认: `XDG_CONFIG_HOME/anime-tool/config.json`
- `--version | -v` 显示版本信息
- `--help | -h` 显示帮助信息, 不执行任何操作

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
