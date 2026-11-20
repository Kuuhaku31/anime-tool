# kg-play

`kg-play` 在 Linux 终端中读取 PostgreSQL 的 Dandanplay 弹幕, 启动 MPV 播放
媒体, 并将弹幕作为独立 ASS 次字幕轨显示. 本地路径和 MPV 支持的网络地址都
作为单独的进程参数传递, 不经过 shell.

## 依赖和数据库

- Linux.
- 可执行的 `mpv`, 默认从 `PATH` 查找.
- 可用的 PostgreSQL `main` 数据库连接.
- 可交互的终端.

程序启动时加载当前目录 `.env`, 进程环境变量优先. 数据库连接使用
`PG_HOST`, `PG_USER`, `PG_PASSWORD` 和可选 `PG_PORT`, 数据库名固定为 `main`.
样式和 MPV 配置也可通过下表的 `KG_PLAY_*` 环境变量提供.

弹幕从 `public.dandanplay_danmaku` 按以下条件读取:

```sql
SELECT appear_time, mode, color, content
FROM public.dandanplay_danmaku
WHERE ddp_epi_id = $1
ORDER BY appear_time NULLS LAST, cid;
```

出现时间为空, 正文为空或仅包含空白字符的记录不渲染. 没有可用弹幕时仍然
启动 MPV 并播放媒体.

## 用法

```text
kg-play <media-path-or-url> <ddp-epi-id> [OPTIONS]
```

例子:

```bash
kg-play "/media/Anime Episode 01.mkv" 196000012 --font "Noto Sans CJK SC" \
  --font-size 24 --alpha 0.9 --border 1 --offset -1.5 --speed 1.2 --density 0.8
kg-play "https://example.com/episode.mkv" 196000012 --hidden
```

`--offset -1.5` 和 `--offset=-1.5` 都有效. 负数表示弹幕提前, 正数表示延后.
MPV 窗口处理暂停, 跳转, 音量和其他播放控制, 终端只控制弹幕和退出.
`kg-play` 禁用 MPV 对次字幕 ASS 样式的覆盖, 保留弹幕坐标, 滚动动画和字号,
不改变主字幕的样式设置.

| 选项 | 默认值 | 范围和说明 |
| --- | --- | --- |
| `--font` | `sans-serif` | ASS 字体名称, 播放中不变 |
| `--font-size` | `24` | `8..=60` |
| `--alpha` | `1.0` | 不透明度, `0.05..=1.0` |
| `--bold` / `--no-bold` | `false` | 是否使用粗体, 两者互斥 |
| `--border` | `1.0` | 描边宽度, `0..=8` |
| `--offset` | `0.0` | 时间偏移秒数, `-600..=600` |
| `--speed` | `1.0` | 滚动速度倍率, `0.5..=3.0` |
| `--density` | `1.0` | 可用画面高度比例, `0.1..=1.0` |
| `--danmaku-fps` | `60` | 弹幕更新的目标最低帧率, `0..=240`, `0` 表示跟随原视频 |
| `--hidden` / `--visible` | `visible` | 初始显示状态, 两者互斥 |

ASS 弹幕随 MPV 输出的视频帧更新. 有可用弹幕时, MPV 会重复低帧率视频的画面,
让弹幕在相邻原始帧之间继续移动. 这不做运动插帧, 也不会主动降低视频的标称帧率.
实际流畅度取决于屏幕刷新率和机器性能. 如果视频滤镜增加了负载或影响硬件解码,
可使用 `--danmaku-fps 0` 关闭. 无可用弹幕时不启用该滤镜.

支持以下环境变量, 对应 CLI 选项优先级更高. 环境变量值无效时程序会报错,
不会静默使用默认值.

| 环境变量 | 默认值 |
| --- | --- |
| `KG_PLAY_MPV` | `mpv` |
| `KG_PLAY_FONT` | `sans-serif` |
| `KG_PLAY_FONT_SIZE` | `24` |
| `KG_PLAY_ALPHA` | `1.0` |
| `KG_PLAY_BOLD` | `false` |
| `KG_PLAY_BORDER` | `1.0` |
| `KG_PLAY_OFFSET` | `0.0` |
| `KG_PLAY_SPEED` | `1.0` |
| `KG_PLAY_DENSITY` | `1.0` |
| `KG_PLAY_VISIBLE` | `true` |

布尔环境变量接受 `true/false`, `1/0` 和 `yes/no`, 不区分大小写.
弹幕速度通过调整 ASS 滚动持续时间实现, 持续时间为 `10 / speed` 秒.

## 终端控制

在普通终端的 `kg-play>` 提示符后直接输入命令, 按 `Enter` 执行,
不需要 `:` 前缀. 启动时和每条命令处理后, 都会输出有效弹幕数,
ASS 实际渲染条数, 次字幕轨和显示状态, 当前样式, 以及 MPV 播放状态和位置.
空行和 `status` 都只查询状态. 错误命令不会更改设置, 仍会输出当前状态.
输出保留在终端历史中, 等待输入时不会持续刷新.

```text
status
size 28
alpha 0.8
border 1.5
offset -1.5
speed 1.2
density 0.8
bold
regular
show
hide
toggle
reset
help
quit
```

`reset` 恢复本次启动时的设置. 样式变化在执行命令后立即重新生成 ASS 并替换弹幕轨,
保留播放位置和主字幕. `show`, `hide` 和 `toggle` 只切换可见性.
输入 `quit`, 按 `Ctrl+C` 或结束输入 (`Ctrl+D`) 会结束播放.
退出时回收 MPV 子进程并清理临时 ASS 文件和 IPC socket.
