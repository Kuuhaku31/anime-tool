use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use suppaftp::FtpStream;
use url::Url;

#[derive(Debug, Serialize)]
pub struct AniManifest {
    pub media_path: String,
    pub output_dir: PathBuf,
    pub media_file: PathBuf,
    pub subtitle_tracks: Vec<SubtitleTrack>,
}

#[derive(Debug, Serialize)]
pub struct SubtitleTrack {
    pub index: usize,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    index: usize,
    codec_type: String,
    #[serde(default)]
    tags: ProbeTags,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeTags {
    title: Option<String>,
    language: Option<String>,
}

pub fn default_output_dir(media_path: &str) -> anyhow::Result<PathBuf> {
    let file_name = source_file_name(media_path)?;
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(&file_name);
    Ok(PathBuf::from(format!("{stem} - ani")))
}

pub fn prepare(media_path: &str, output_dir: &Path) -> anyhow::Result<AniManifest> {
    require_program("ffprobe")?;
    require_program("ffmpeg")?;
    fs::create_dir_all(output_dir)
        .with_context(|| format!("无法创建输出目录: {}", output_dir.display()))?;

    let file_name = source_file_name(media_path)?;
    let media_file = output_dir.join(file_name);
    copy_media(media_path, &media_file)?;

    let streams = probe_subtitles(&media_file)?;
    let mut subtitle_tracks = Vec::with_capacity(streams.len());
    for stream in streams {
        let name = track_name(&stream);
        let path = output_dir.join(format!("{}.{}.ass", stream.index, sanitize_name(&name)));
        extract_subtitle(&media_file, stream.index, &path)?;
        subtitle_tracks.push(SubtitleTrack {
            index: stream.index,
            name,
            path,
        });
    }

    let manifest = AniManifest {
        media_path: media_path.to_owned(),
        output_dir: output_dir.to_owned(),
        media_file,
        subtitle_tracks,
    };
    let manifest_path = output_dir.join("ani.json");
    let body = serde_json::to_string_pretty(&manifest).context("无法序列化 ani.json")?;
    fs::write(&manifest_path, format!("{body}\n"))
        .with_context(|| format!("无法写入清单: {}", manifest_path.display()))?;
    Ok(manifest)
}

fn source_file_name(input: &str) -> anyhow::Result<String> {
    if let Ok(url) = Url::parse(input) {
        match url.scheme() {
            "http" | "https" | "ftp" | "file" => {
                let encoded = url
                    .path_segments()
                    .and_then(|mut values| values.next_back())
                    .filter(|value| !value.is_empty())
                    .context("媒体 URL 缺少文件名")?;
                let decoded = urlencoding::decode(encoded)
                    .map(|value| value.into_owned())
                    .unwrap_or_else(|_| encoded.to_owned());
                return safe_file_name(&decoded);
            }
            _ => {}
        }
    }
    let name = Path::new(input)
        .file_name()
        .and_then(|value| value.to_str())
        .context("媒体路径缺少有效文件名")?;
    safe_file_name(name)
}

fn safe_file_name(value: &str) -> anyhow::Result<String> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        bail!("无效媒体文件名: {value}");
    }
    Ok(value.to_owned())
}

fn copy_media(input: &str, destination: &Path) -> anyhow::Result<()> {
    let temporary = destination.with_extension(format!(
        "{}part",
        destination
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!("{value}."))
            .unwrap_or_default()
    ));
    let result = if let Ok(url) = Url::parse(input) {
        match url.scheme() {
            "http" | "https" => copy_http(&url, &temporary),
            "ftp" => copy_ftp(&url, &temporary),
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("无效 file:// URL"))?;
                copy_local(&path, &temporary)
            }
            _ => copy_local(Path::new(input), &temporary),
        }
    } else {
        copy_local(Path::new(input), &temporary)
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if destination.exists() {
        fs::remove_file(destination)
            .with_context(|| format!("无法覆盖媒体文件: {}", destination.display()))?;
    }
    fs::rename(&temporary, destination)
        .with_context(|| format!("无法保存媒体文件: {}", destination.display()))
}

fn copy_local(source: &Path, destination: &Path) -> anyhow::Result<()> {
    if !source.is_file() {
        bail!("媒体文件不存在: {}", source.display());
    }
    fs::copy(source, destination)
        .with_context(|| format!("无法复制媒体文件: {}", source.display()))?;
    Ok(())
}

fn copy_http(url: &Url, destination: &Path) -> anyhow::Result<()> {
    let mut response = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("创建 HTTP 客户端失败")?
        .get(url.clone())
        .send()
        .context("下载媒体文件失败")?
        .error_for_status()
        .context("下载媒体文件失败")?;
    let mut file = File::create(destination)
        .with_context(|| format!("无法创建媒体文件: {}", destination.display()))?;
    io::copy(&mut response, &mut file).context("写入媒体文件失败")?;
    Ok(())
}

fn copy_ftp(url: &Url, destination: &Path) -> anyhow::Result<()> {
    let host = url.host_str().context("FTP URL 缺少主机名")?;
    let mut ftp =
        FtpStream::connect((host, url.port().unwrap_or(21))).context("连接 FTP 服务器失败")?;
    let username = if url.username().is_empty() {
        "anonymous"
    } else {
        url.username()
    };
    ftp.login(username, url.password().unwrap_or("anonymous@example.com"))
        .context("FTP 登录失败")?;
    let mut file = File::create(destination)
        .with_context(|| format!("无法创建媒体文件: {}", destination.display()))?;
    ftp.retr(url.path(), |stream| {
        io::copy(stream, &mut file).map_err(suppaftp::FtpError::ConnectionError)
    })
    .context("FTP 下载媒体文件失败")?;
    let _ = ftp.quit();
    Ok(())
}

fn probe_subtitles(media_file: &Path) -> anyhow::Result<Vec<ProbeStream>> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=index,codec_type:stream_tags=title,language",
            "-of",
            "json",
        ])
        .arg(media_file)
        .output()
        .context("无法运行 ffprobe")?;
    if !output.status.success() {
        bail!(
            "ffprobe 解析媒体失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let probe: ProbeOutput = serde_json::from_slice(&output.stdout).context("ffprobe 输出无效")?;
    Ok(probe
        .streams
        .into_iter()
        .filter(|stream| stream.codec_type == "subtitle")
        .collect())
}

fn extract_subtitle(media_file: &Path, index: usize, output_file: &Path) -> anyhow::Result<()> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(media_file)
        .args(["-map", &format!("0:{index}")])
        .arg(output_file)
        .output()
        .context("无法运行 ffmpeg")?;
    if !output.status.success() {
        let _ = fs::remove_file(output_file);
        bail!(
            "字幕轨道 {index} 无法转换为 ASS: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn require_program(name: &str) -> anyhow::Result<()> {
    match Command::new(name).arg("-version").output() {
        Ok(output) if output.status.success() => Ok(()),
        _ => bail!("未找到可用的 {name}, 请先安装 FFmpeg"),
    }
}

fn track_name(stream: &ProbeStream) -> String {
    stream
        .tags
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            stream
                .tags
                .language
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or("subtitle")
        .to_owned()
}

fn sanitize_name(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let value = value.trim().trim_matches('.');
    if value.is_empty() {
        "subtitle".to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_default_directory_from_local_path_and_url() {
        assert_eq!(
            default_output_dir("/video/My Anime.mkv").unwrap(),
            PathBuf::from("My Anime - ani")
        );
        assert_eq!(
            default_output_dir("https://example.com/My%20Anime.mp4?token=x").unwrap(),
            PathBuf::from("My Anime - ani")
        );
    }

    #[test]
    fn sanitizes_track_name() {
        assert_eq!(sanitize_name("简体/中文: SC"), "简体_中文_ SC");
        assert_eq!(sanitize_name("..."), "subtitle");
    }
}
