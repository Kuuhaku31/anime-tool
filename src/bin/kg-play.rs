
// src/bin/kg-play.rs
// 从 PostgreSQL 加载弹幕并使用 MPV 播放视频.

use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, IsTerminal, Write},
    net::Shutdown,
    os::unix::{net::UnixStream, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use anime_tool::ass::{AssDanmaku, AssSettings, render_danmaku};
use clap::{ArgAction, Parser};
use postgres::Client;
use serde_json::{Value, json};

#[path = "../kg/utils.rs"]
#[allow(unused_macros, unused_imports)]
mod utils;

/// kg-play 的位置参数和启动样式选项.
#[derive(Debug, Parser)]
#[command(name = "kg-play", version, about = "使用 MPV 播放视频并显示 Dandanplay 弹幕")]
struct Cli {
    /// MPV 支持的本地路径或网络媒体地址.
    media: String,

    /// Dandanplay 剧集 ID.
    ddp_epi_id: i64,

    /// ASS 字体名称.
    #[arg(long)]
    font: Option<String>,

    /// 字体大小, 范围 8..=60.
    #[arg(long, allow_hyphen_values = true)]
    font_size: Option<f64>,

    /// 弹幕不透明度, 范围 0.05..=1.0.
    #[arg(long, allow_hyphen_values = true)]
    alpha: Option<f64>,

    /// 使用粗体字体.
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "no_bold")]
    bold: bool,

    /// 不使用粗体字体.
    #[arg(long = "no-bold", action = ArgAction::SetTrue)]
    no_bold: bool,

    /// ASS 描边宽度, 范围 0..=8.
    #[arg(long, allow_hyphen_values = true)]
    border: Option<f64>,

    /// 弹幕时间偏移, 负数表示提前.
    #[arg(long, allow_hyphen_values = true)]
    offset: Option<f64>,

    /// 滚动弹幕速度倍率, 范围 0.5..=3.0.
    #[arg(long, allow_hyphen_values = true)]
    speed: Option<f64>,

    /// 弹幕可使用的画面高度比例, 范围 0.1..=1.0.
    #[arg(long, allow_hyphen_values = true)]
    density: Option<f64>,

    /// 弹幕更新的目标最低帧率, 0 表示跟随原视频.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(0..=240))]
    danmaku_fps: u32,

    /// 启动时隐藏弹幕.
    #[arg(long, action = ArgAction::SetTrue, conflicts_with = "visible")]
    hidden: bool,

    /// 启动时显示弹幕.
    #[arg(long, action = ArgAction::SetTrue)]
    visible: bool,
}

/// 播放期间可调整的 ASS 样式和可见性.
#[derive(Clone)]
struct PlaybackSettings {
    /// 当前 ASS 渲染选项.
    ass: AssSettings,
    /// 滚动弹幕速度倍率.
    speed: f64,
    /// 可用弹幕画面高度比例.
    density: f64,
    /// 是否显示次字幕轨.
    visible: bool,
}

/// 管理 MPV JSON IPC Unix socket.
struct MpvClient {
    /// 与 MPV 的双向 IPC 连接.
    stream: BufReader<UnixStream>,
    /// 下一个请求编号.
    request_id: u64,
}

/// 持有 MPV 子进程并在离开作用域时确保进程被回收.
struct MpvProcess {
    /// 启动的 MPV 子进程.
    child: Child,
    /// MPV 标准错误日志临时文件.
    log_path: PathBuf,
}

/// 管理本次播放使用的 ASS 文件和 IPC socket 路径.
struct RuntimeFiles {
    /// 本次运行独占的临时目录.
    directory: PathBuf,
    /// MPV 读取的 ASS 文件路径.
    ass_path: PathBuf,
    /// MPV 创建的 IPC Unix socket 路径.
    socket_path: PathBuf,
    /// MPV 诊断输出日志路径.
    log_path: PathBuf,
    /// 标记临时目录是否已显式清理.
    cleaned: bool,
}

/// 保存控制台报告所需的弹幕轨信息.
#[derive(Default)]
struct DanmakuTrack {
    /// 查询到的有效弹幕数.
    danmaku_count: usize,
    /// ASS 文件中实际渲染的弹幕数.
    rendered_count: usize,
    /// 当前使用的 MPV 次字幕轨 ID.
    track_id: Option<i64>,
    /// 加载弹幕前已选择的 MPV 主字幕轨 ID.
    primary_subtitle_id: Option<i64>,
}

/// 校验启动参数并运行数据库, MPV 和终端交互流程.
fn main() {
    if let Err(error) = run() {
        eprintln!("kg-play 运行失败: {error:#}");
        std::process::exit(1);
    }
}

/// 启动播放会话并确保终端和 MPV 子进程得到清理.
fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.ddp_epi_id <= 0 {
        bail!("ddp_epi_id 必须是正的 int64");
    }

    let env = utils::EnvConfig::load(false)?;
    let settings = load_settings(&cli)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("kg-play 需要交互式终端");
    }
    if !cfg!(target_os = "linux") {
        bail!("kg-play 第一版仅支持 Linux");
    }

    let interrupted = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&interrupted))
        .context("注册 Ctrl+C 退出处理失败")?;
    let mut ui = DanmakuTrack::default();
    println!("读取剧集 {} 的弹幕...", cli.ddp_epi_id);

    let mut db = env
        .connect_main(None)
        .context("连接 PostgreSQL main 数据库失败")?;
    let comments = load_danmaku(&mut db, cli.ddp_epi_id)?;
    drop(db);
    if interrupted.load(Ordering::Relaxed) {
        return Ok(());
    }
    ui.danmaku_count = comments.len();

    let mut runtime = RuntimeFiles::create()?;
    let mut render_settings = settings.ass.clone();
    render_settings.scroll_duration = 10.0 / settings.speed;
    render_settings.display_area = settings.density;
    let ass = render_danmaku(&comments, &render_settings)?;
    ui.rendered_count = ass.lines().filter(|line| line.starts_with("Dialogue:")).count();
    if !comments.is_empty() {
        write_atomic(&runtime.ass_path, ass.as_bytes())?;
    }

    println!("启动 MPV...");
    let mpv_path = env_string("KG_PLAY_MPV", "mpv")?;
    let danmaku_fps = if comments.is_empty() { 0 } else { cli.danmaku_fps };
    let mut process = MpvProcess::spawn(
        &mpv_path,
        &cli.media,
        &runtime.socket_path,
        &runtime.log_path,
        danmaku_fps,
    )?;

    let Some(mut client) = MpvClient::connect(&runtime.socket_path, &mut process, &interrupted)? else {
        return Ok(());
    };
    if !wait_until_playing(&mut client, &mut process, &interrupted)? {
        return process.shutdown(&mut client);
    }

    if !comments.is_empty() {
        ui.primary_subtitle_id = selected_primary_subtitle_id(&mut client)?;
        attach_danmaku_track(&mut client, &runtime, &mut ui, &settings)?;
    }

    println!("输入 help 查看命令, 输入 quit 退出.");
    let session_result = play_loop(
        &mut client,
        &mut process,
        &runtime,
        &mut ui,
        settings,
        &comments,
        &interrupted,
    );
    let shutdown_result = process.shutdown(&mut client);
    drop(client);
    let cleanup_result = runtime.cleanup();

    match (session_result, shutdown_result, cleanup_result) {
        (Err(error), _, Err(cleanup_error)) => {
            Err(error).context(format!("清理 kg-play 临时目录失败: {cleanup_error:#}"))
        }
        (Err(error), _, _) => Err(error),
        (Ok(()), Err(error), Err(cleanup_error)) => {
            Err(error).context(format!("清理 kg-play 临时目录失败: {cleanup_error:#}"))
        }
        (Ok(()), Err(error), _) => Err(error),
        (Ok(()), Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(()), Ok(())) => Ok(()),
    }
}

/// 按 CLI, 环境变量和内置默认值的优先级解析样式设置.
fn load_settings(cli: &Cli) -> Result<PlaybackSettings> {
    let defaults = AssSettings::default();
    let font = match &cli.font {
        Some(value) => value.clone(),
        None => env_string("KG_PLAY_FONT", "sans-serif")?,
    };
    let font_size = match cli.font_size {
        Some(value) => value,
        None => env_number("KG_PLAY_FONT_SIZE", 24.0)?,
    };
    let alpha = match cli.alpha {
        Some(value) => value,
        None => env_number("KG_PLAY_ALPHA", 0.7)?,
    };
    let border = match cli.border {
        Some(value) => value,
        None => env_number("KG_PLAY_BORDER", 1.0)?,
    };
    let offset = match cli.offset {
        Some(value) => value,
        None => env_number("KG_PLAY_OFFSET", 0.0)?,
    };
    let speed = match cli.speed {
        Some(value) => value,
        None => env_number("KG_PLAY_SPEED", 1.0)?,
    };
    let density = match cli.density {
        Some(value) => value,
        None => env_number("KG_PLAY_DENSITY", 1.0)?,
    };

    let bold = if cli.bold {
        true
    } else if cli.no_bold {
        false
    } else {
        env_bool("KG_PLAY_BOLD", false)?
    };
    let visible = if cli.hidden {
        false
    } else if cli.visible {
        true
    } else {
        env_bool("KG_PLAY_VISIBLE", true)?
    };

    validate_range("font-size", font_size, 8.0, 60.0)?;
    validate_range("alpha", alpha, 0.05, 1.0)?;
    validate_range("border", border, 0.0, 8.0)?;
    validate_range("offset", offset, -600.0, 600.0)?;
    validate_range("speed", speed, 0.5, 3.0)?;
    validate_range("density", density, 0.1, 1.0)?;

    Ok(PlaybackSettings {
        ass: AssSettings {
            font,
            font_size,
            alpha,
            bold,
            border,
            offset,
            scroll_duration: defaults.scroll_duration,
            display_area: defaults.display_area,
        },
        speed,
        density,
        visible,
    })
}

/// 从环境变量读取字符串并在变量缺失时使用默认值.
fn env_string(name: &str, default: &str) -> Result<String> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(error) => bail!("环境变量 {name} 不是有效 Unicode 字符串: {error}"),
    }
}

/// 从环境变量解析浮点数并指出无效变量名称.
fn env_number(name: &str, default: f64) -> Result<f64> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<f64>()
            .with_context(|| format!("环境变量 {name} 必须是数字")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => bail!("环境变量 {name} 不是有效 Unicode 字符串: {error}"),
    }
}

/// 解析 true/false, 1/0 和 yes/no 格式的环境布尔值.
fn env_bool(name: &str, default: bool) -> Result<bool> {
    let value = match std::env::var(name) {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return Ok(default),
        Err(error) => bail!("环境变量 {name} 不是有效 Unicode 字符串: {error}"),
    };
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => bail!("环境变量 {name} 必须是 true/false, 1/0 或 yes/no"),
    }
}

/// 校验数值为有限值且位于闭区间内.
fn validate_range(name: &str, value: f64, minimum: f64, maximum: f64) -> Result<()> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        bail!("{name} 必须是 {minimum}..={maximum} 范围内的有限数字");
    }
    Ok(())
}

/// 查询指定剧集的弹幕并跳过时间或正文缺失的记录.
fn load_danmaku(db: &mut Client, episode_id: i64) -> Result<Vec<AssDanmaku>> {
    let rows = db
        .query(
            "SELECT appear_time, mode, color, content
             FROM public.dandanplay_danmaku
             WHERE ddp_epi_id = $1
             ORDER BY appear_time NULLS LAST, cid",
            &[&episode_id],
        )
        .with_context(|| format!("查询剧集 {episode_id} 的数据库弹幕失败"))?;
    let mut comments = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(time) = row.get::<_, Option<f64>>(0) else {
            continue;
        };
        let Some(content) = row.get::<_, Option<String>>(3) else {
            continue;
        };
        if !time.is_finite() || content.trim().is_empty() {
            continue;
        }
        comments.push(AssDanmaku {
            time,
            content,
            mode: i32::from(row.get::<_, i16>(1)),
            color: row.get::<_, i32>(2),
        });
    }
    Ok(comments)
}

impl RuntimeFiles {
    /// 创建本次运行独占的临时目录和 MPV 文件路径.
    fn create() -> Result<Self> {
        let root = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("读取系统时间失败")?
            .as_nanos();
        let directory = root.join(format!("kg-play-{}-{stamp:x}", std::process::id()));
        fs::create_dir(&directory).context("创建 kg-play 临时目录失败")?;
        let value = Self {
            ass_path: directory.join("danmaku.ass"),
            socket_path: directory.join("mpv.sock"),
            log_path: directory.join("mpv.log"),
            directory,
            cleaned: false,
        };
        if value.socket_path.as_os_str().as_encoded_bytes().len() >= 104 {
            bail!("临时 MPV socket 路径超过 Linux Unix socket 长度限制");
        }
        Ok(value)
    }
}

impl RuntimeFiles {
    /// 删除本次运行独占的临时目录并报告清理失败.
    fn cleanup(&mut self) -> Result<()> {
        fs::remove_dir_all(&self.directory).context("删除 kg-play 临时目录失败")?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for RuntimeFiles {
    /// 删除本次运行专用的 ASS 和 socket 临时目录.
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

impl MpvProcess {
    /// 以无终端模式启动 MPV, 并按需提高弹幕的更新帧率.
    fn spawn(
        program: &str, media: &str, socket_path: &Path, log_path: &Path, danmaku_fps: u32,
    ) -> Result<Self> {
        let log = File::create(log_path).context("创建 MPV 日志文件失败")?;
        let mut command = Command::new(program);
        command
            .arg("--no-terminal")
            // MPV 默认会剥离次字幕的 ASS 样式, 导致弹幕丢失坐标, 动画和字号.
            .arg("--secondary-sub-ass-override=no")
            .arg(format!(
                "--input-ipc-server={}",
                socket_path.to_string_lossy()
            ));
        if danmaku_fps > 0 {
            // 重复帧提供额外的 ASS 渲染时刻, 不生成中间画面, 不覆盖用户视频滤镜.
            command.arg(format!(
                "--vf-append=@kg-play-danmaku-fps:lavfi=[fps=fps='max(source_fps,{danmaku_fps})']"
            ));
        }
        let child = command
            .arg("--")
            .arg(media)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .process_group(0)
            .spawn()
            .with_context(|| format!("启动 MPV 失败: {program}"))?;
        Ok(Self {
            child,
            log_path: log_path.to_owned(),
        })
    }

    /// 请求 MPV 正常退出, 超时后终止并回收子进程.
    fn shutdown(&mut self, client: &mut MpvClient) -> Result<()> {
        if let Some(status) = self.child.try_wait()? {
            if !status.success() {
                bail!("{}", self.exit_message(status));
            }
            return Ok(());
        }
        let quit_result = client.command(&[json!("quit")]);
        client.close();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                if !status.success() {
                    bail!("{}", self.exit_message(status));
                }
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let pid = libc::pid_t::try_from(self.child.id())
            .context("MPV 进程 ID 超出系统范围")?;
        // 仅对当前 Child 返回的 MPV PID 发送终止信号.
        if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
            let error = io::Error::last_os_error();
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            return match quit_result {
                Err(quit_error) => Err(error).context(format!(
                    "发送 MPV SIGTERM 信号失败, IPC 退出命令也失败: {quit_error:#}"
                )),
                Ok(_) => Err(error).context("发送 MPV SIGTERM 信号失败"),
            };
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.child.kill().context("终止 MPV 子进程失败")?;
        self.child.wait().context("回收 MPV 子进程失败")?;
        Ok(())
    }

    /// 格式化 MPV 退出状态并附加日志尾部.
    fn exit_message(&self, status: std::process::ExitStatus) -> String {
        let log = fs::read_to_string(&self.log_path).unwrap_or_default();
        let details = log.lines().rev().take(20).collect::<Vec<_>>();
        let details = details.into_iter().rev().collect::<Vec<_>>().join("\n");
        if details.is_empty() {
            format!("MPV 异常退出: {status}")
        } else {
            format!("MPV 异常退出: {status}\n{details}")
        }
    }
}

impl Drop for MpvProcess {
    /// 发生错误或提前返回时终止并回收尚未退出的 MPV.
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl MpvClient {
    /// 等待 MPV 创建 IPC socket, 若用户中断启动则返回空值.
    fn connect(
        socket_path: &Path,
        process: &mut MpvProcess,
        interrupted: &AtomicBool,
    ) -> Result<Option<Self>> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if interrupted.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if let Some(status) = process
                .child
                .try_wait()
                .context("检查 MPV 子进程状态失败")?
            {
                bail!("{}", process.exit_message(status));
            }
            match UnixStream::connect(socket_path) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .context("设置 MPV IPC 读取超时失败")?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .context("设置 MPV IPC 写入超时失败")?;
                    return Ok(Some(Self {
                        stream: BufReader::new(stream),
                        request_id: 0,
                    }));
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => {
                    return Err(error).context("连接 MPV IPC socket 失败");
                }
            }
        }
    }

    /// 发送 JSON IPC 命令, 校验响应并返回其 data 字段.
    fn command(&mut self, command: &[Value]) -> Result<Value> {
        self.request_id = self.request_id.wrapping_add(1);
        let request_id = self.request_id;
        let mut request = serde_json::to_vec(&json!({
            "command": command,
            "request_id": request_id
        }))?;
        request.push(b'\n');
        self.stream
            .get_mut()
            .write_all(&request)
            .context("写入 MPV IPC 命令失败")?;

        loop {
            let mut response_line = String::new();
            self.stream
                .read_line(&mut response_line)
                .context("读取 MPV IPC 响应失败")?;
            if response_line.is_empty() {
                bail!("MPV IPC 在返回响应前关闭连接");
            }
            let response: Value =
                serde_json::from_str(&response_line).context("MPV IPC 响应不是合法 JSON")?;
            let response_object = response
                .as_object()
                .context("MPV IPC 响应不是 JSON 对象")?;
            if response_object.get("request_id").and_then(Value::as_u64) != Some(request_id) {
                continue;
            }
            let response_error = response_object
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("missing error field");
            if response_error != "success" {
                bail!("MPV IPC 命令失败: {response_error}");
            }
            return Ok(response_object.get("data").cloned().unwrap_or(Value::Null));
        }
    }

    /// 获取 MPV 属性, 并将媒体尚未载入时的 unavailable 视为无值.
    fn get_property(&mut self, name: &str) -> Result<Option<Value>> {
        match self.command(&[json!("get_property"), json!(name)]) {
            Ok(value) => Ok(Some(value)),
            Err(error)
                if error
                    .to_string()
                    .contains("MPV IPC 命令失败: property unavailable") =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// 关闭 IPC 连接, 使已收到 quit 命令的 MPV 能完成退出.
    fn close(&mut self) {
        let _ = self.stream.get_ref().shutdown(Shutdown::Both);
    }
}

/// 轮询 MPV 播放位置, 返回媒体是否在用户中断前载入.
fn wait_until_playing(
    client: &mut MpvClient,
    process: &mut MpvProcess,
    interrupted: &AtomicBool,
) -> Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if interrupted.load(Ordering::Relaxed) {
            return Ok(false);
        }
        if let Some(status) = process
            .child
            .try_wait()
            .context("检查 MPV 播放状态失败")?
        {
            bail!("{}", process.exit_message(status));
        }
        if client
            .get_property("time-pos")?
            .and_then(|value| value.as_f64())
            .is_some()
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            bail!("等待 MPV 媒体载入超时");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 从 MPV track-list 中查找刚加入的 ASS 次字幕轨.
fn wait_for_track(client: &mut MpvClient, ass_path: &Path) -> Result<i64> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let tracks = client.command(&[json!("get_property"), json!("track-list")])?;
        if let Some(tracks) = tracks.as_array() {
            for track in tracks {
                let is_subtitle = track.get("type").and_then(Value::as_str) == Some("sub");
                let filename = track
                    .get("external-filename")
                    .and_then(Value::as_str)
                    .map(Path::new);
                if is_subtitle && filename == Some(ass_path) {
                    if let Some(id) = track.get("id").and_then(Value::as_i64) {
                        return Ok(id);
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            bail!("MPV 已执行 sub-add, 但 track-list 中没有找到弹幕轨");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 添加 ASS 文件并将其设置为次字幕轨, 保留主字幕轨选择.
fn attach_danmaku_track(
    client: &mut MpvClient,
    runtime: &RuntimeFiles,
    ui: &mut DanmakuTrack,
    settings: &PlaybackSettings,
) -> Result<()> {
    let tracks = client.command(&[json!("get_property"), json!("track-list")])?;
    let already_added = tracks.as_array().is_some_and(|tracks| {
        tracks.iter().any(|track| {
            track.get("type").and_then(Value::as_str) == Some("sub")
                && track
                    .get("external-filename")
                    .and_then(Value::as_str)
                    .is_some_and(|path| Path::new(path) == runtime.ass_path)
        })
    });
    if !already_added {
        client.command(&[
            json!("sub-add"),
            json!(runtime.ass_path.to_string_lossy()),
            json!("auto"),
            json!("KG Play Danmaku"),
        ])?;
    }
    ui.track_id = Some(wait_for_track(client, &runtime.ass_path)?);
    restore_subtitle_selection(client, ui, settings)
}

/// 获取加载弹幕前已选中的主字幕轨 ID.
fn selected_primary_subtitle_id(client: &mut MpvClient) -> Result<Option<i64>> {
    let sid = client.command(&[json!("get_property"), json!("sid")])?;
    Ok(sid.as_i64())
}

/// 逐行处理命令, 并在等待输入时监控 MPV 退出.
fn play_loop(
    client: &mut MpvClient,
    process: &mut MpvProcess,
    runtime: &RuntimeFiles,
    ui: &mut DanmakuTrack,
    mut settings: PlaybackSettings,
    comments: &[AssDanmaku],
    interrupted: &AtomicBool,
) -> Result<()> {
    let initial_settings = settings.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    // 阻塞读行放在独立线程中, 避免等待回车时无法感知 MPV 退出.
    thread::Builder::new().name("kg-play-input".to_owned()).spawn(move || {
        for line in io::stdin().lock().lines() {
            let failed = line.is_err();
            if sender.send(line).is_err() || failed {
                break;
            }
        }
    }).context("启动控制台输入线程失败")?;

    if interrupted.load(Ordering::Relaxed) {
        return Ok(());
    }
    print_status(client, ui, &settings)?;
    print_prompt()?;
    loop {
        if interrupted.load(Ordering::Relaxed) {
            println!("\n收到 Ctrl+C, 正在退出 MPV.");
            return Ok(());
        }
        if let Some(status) = process
            .child
            .try_wait()
            .context("检查 MPV 子进程状态失败")?
        {
            if status.success() {
                println!("\nMPV 已退出.");
                return Ok(());
            }
            bail!("{}", process.exit_message(status));
        }

        let line = match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => line.context("读取控制台命令失败")?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                println!("\n输入已结束, 正在退出 MPV.");
                return Ok(());
            }
        };
        let mut next = settings.clone();
        let quit = match run_command(line.trim(), &mut next, &initial_settings) {
            Ok(quit) => {
                if ui.track_id.is_some() {
                    if !settings_equal(&settings, &next) {
                        reload_ass(client, runtime, ui, &next, comments)?;
                    } else if next.visible != settings.visible {
                        client.command(&[
                            json!("set_property"),
                            json!("secondary-sub-visibility"),
                            json!(next.visible),
                        ])?;
                    }
                }
                settings = next;
                quit
            }
            Err(error) => {
                println!("命令错误: {error:#}");
                false
            }
        };
        print_status(client, ui, &settings)?;
        if quit {
            println!("正在退出 MPV.");
            return Ok(());
        }
        print_prompt()?;
    }
}

/// 输出并刷新命令提示符, 不清除终端历史.
fn print_prompt() -> Result<()> {
    let mut output = io::stdout().lock();
    write!(output, "kg-play> ")?;
    output.flush().context("刷新命令提示符失败")
}

/// 查询 MPV 实时状态并输出当前弹幕数量和样式.
fn print_status(
    client: &mut MpvClient,
    track: &DanmakuTrack,
    settings: &PlaybackSettings,
) -> Result<()> {
    let position = client.get_property("time-pos")?.and_then(|value| value.as_f64());
    let paused = client.get_property("pause")?.and_then(|value| value.as_bool());
    let state = match (position, paused) {
        (None, _) => "无播放位置",
        (_, Some(true)) => "已暂停",
        (_, Some(false)) => "播放中",
        _ => "未知",
    };
    let time = position.map(format_time).unwrap_or_else(|| "--:--:--".to_owned());
    let id = track.track_id.map_or_else(|| "-".to_owned(), |id| id.to_string());
    let visible = match (track.track_id, settings.visible) {
        (None, _) => "无弹幕轨",
        (_, true) => "显示",
        (_, false) => "隐藏",
    };
    println!("弹幕: 已读取 {} 条, 已渲染 {} 条, 次字幕轨 {id}, {visible}",
        track.danmaku_count, track.rendered_count);
    println!("样式: 字体 {:?}, 字号 {}, 不透明度 {}, 粗体 {}, 描边 {}, 偏移 {:+}s, 速度 {}x, 密度 {}",
        settings.ass.font, settings.ass.font_size, settings.ass.alpha, settings.ass.bold,
        settings.ass.border, settings.ass.offset, settings.speed, settings.density);
    println!("MPV: {state}, 位置 {time}");
    Ok(())
}

/// 处理一行控制台命令, 返回是否需要退出播放.
fn run_command(
    command: &str,
    settings: &mut PlaybackSettings,
    initial_settings: &PlaybackSettings,
) -> Result<bool> {
    let (name, value) = command.split_once(char::is_whitespace).unwrap_or((command, ""));
    let name = name.to_ascii_lowercase();
    if !value.trim().is_empty()
        && !matches!(name.as_str(),
            "size" | "font-size" | "alpha" | "border" | "offset" | "speed" | "density")
    {
        bail!("命令 {name} 不接受参数");
    }
    match name.as_str() {
        "" | "status" => {}
        "size" | "font-size" => set_number(value, "字号", 8.0, 60.0, |number| {
            settings.ass.font_size = number;
        })?,
        "alpha" => set_number(value, "透明度", 0.05, 0.7, |number| {
            settings.ass.alpha = number;
        })?,
        "border" => set_number(value, "描边", 0.0, 8.0, |number| {
            settings.ass.border = number;
        })?,
        "offset" => set_number(value, "偏移", -600.0, 600.0, |number| {
            settings.ass.offset = number;
        })?,
        "speed" => set_number(value, "速度", 0.5, 3.0, |number| {
            settings.speed = number;
        })?,
        "density" => set_number(value, "密度", 0.1, 1.0, |number| {
            settings.density = number;
        })?,
        "show" | "visible" => settings.visible = true,
        "hide" | "hidden" => settings.visible = false,
        "toggle" => settings.visible = !settings.visible,
        "bold" => settings.ass.bold = true,
        "regular" => settings.ass.bold = false,
        "reset" => {
            *settings = initial_settings.clone();
        }
        "help" => {
            println!("命令: status, size N, alpha N, border N, offset N, speed N, density N,\n      bold, regular, show, hide, toggle, reset, help, quit");
        }
        "quit" | "q" => return Ok(true),
        "font" => {
            println!("字体通过启动选项 --font 设置.");
        }
        _ => bail!("未知命令: {name}"),
    }
    Ok(false)
}

/// 解析并限制命令中的数值参数.
fn set_number(
    value: &str,
    label: &str,
    minimum: f64,
    maximum: f64,
    set: impl FnOnce(f64),
) -> Result<()> {
    let number = value
        .trim()
        .parse::<f64>()
        .with_context(|| format!("{label} 命令需要一个数字参数"))?;
    validate_range(label, number, minimum, maximum)?;
    set(number);
    Ok(())
}

/// 比较影响 ASS 输出的配置, 不将可见性变化视为字幕重载.
fn settings_equal(left: &PlaybackSettings, right: &PlaybackSettings) -> bool {
    left.ass.font == right.ass.font
        && left.ass.font_size == right.ass.font_size
        && left.ass.alpha == right.ass.alpha
        && left.ass.bold == right.ass.bold
        && left.ass.border == right.ass.border
        && left.ass.offset == right.ass.offset
        && left.speed == right.speed
        && left.density == right.density
}

/// 原子替换 ASS 并重新挂载字幕轨, 保留播放位置和主字幕选择.
fn reload_ass(
    client: &mut MpvClient,
    runtime: &RuntimeFiles,
    ui: &mut DanmakuTrack,
    settings: &PlaybackSettings,
    comments: &[AssDanmaku],
) -> Result<()> {
    ui.primary_subtitle_id = selected_primary_subtitle_id(client)?;
    let old_content = fs::read(&runtime.ass_path).context("读取旧 ASS 文件失败")?;
    let mut render_settings = settings.ass.clone();
    render_settings.scroll_duration = 10.0 / settings.speed;
    render_settings.display_area = settings.density;
    let ass = render_danmaku(comments, &render_settings)?;
    write_atomic(&runtime.ass_path, ass.as_bytes())?;
    let remove_result = client.command(&[
        json!("sub-remove"),
        json!(ui.track_id.context("次字幕轨 ID 尚未设置")?),
    ]);
    if let Err(remove_error) = remove_result {
        write_atomic(&runtime.ass_path, &old_content)
            .context("移除旧弹幕轨失败后恢复旧 ASS 文件失败")?;
        return Err(remove_error).context("MPV 移除旧弹幕轨失败");
    }
    attach_danmaku_track(client, runtime, ui, settings)
        .context("ASS 已更新, 但重新添加次字幕轨失败")?;
    ui.rendered_count = ass.lines().filter(|line| line.starts_with("Dialogue:")).count();
    Ok(())
}

/// 在 MPV 重载弹幕轨后恢复主字幕, 次字幕和显示状态.
fn restore_subtitle_selection(
    client: &mut MpvClient,
    ui: &DanmakuTrack,
    settings: &PlaybackSettings,
) -> Result<()> {
    let track_id = ui.track_id.context("次字幕轨 ID 尚未设置")?;
    client.command(&[
        json!("set_property"),
        json!("sid"),
        ui.primary_subtitle_id
            .map_or(json!("no"), |id| json!(id)),
    ])?;
    client.command(&[json!("set_property"), json!("secondary-sid"), json!(track_id)])?;
    client.command(&[
        json!("set_property"),
        json!("secondary-sub-visibility"),
        json!(settings.visible),
    ])?;
    Ok(())
}

/// 将内容写入同目录临时文件并原子替换目标.
fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("读取系统时间失败")?
        .as_nanos();
    let temporary = path.with_extension(format!("ass-{stamp:x}.tmp"));
    let mut file = File::create(&temporary).context("创建临时 ASS 文件失败")?;
    file.write_all(content).context("写入临时 ASS 文件失败")?;
    file.sync_all().context("同步临时 ASS 文件失败")?;
    fs::rename(&temporary, path).context("原子替换 ASS 文件失败")?;
    Ok(())
}

/// 将播放时间格式化为时分秒.
fn format_time(seconds: f64) -> String {
    let total_seconds = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        total_seconds / 3600,
        (total_seconds / 60) % 60,
        total_seconds % 60
    )
}
