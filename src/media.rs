use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, bail};
use md5::{Digest, Md5};
use suppaftp::FtpStream;
use url::Url;

pub const HASH_BYTES: u64 = 16 * 1024 * 1024;

pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub first_16m_md5: String,
}

pub fn inspect(input: &str) -> anyhow::Result<FileInfo> {
    if let Ok(url) = Url::parse(input) {
        match url.scheme() {
            "http" | "https" => inspect_http(input, &url),
            "ftp" => inspect_ftp(&url),
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| anyhow::anyhow!("无效 file:// URL"))?;
                inspect_local(&path)
            }
            _ => inspect_local(Path::new(input)),
        }
    } else {
        inspect_local(Path::new(input))
    }
}

fn inspect_local(path: &Path) -> anyhow::Result<FileInfo> {
    if !path.is_file() {
        bail!("文件不存在: {}", path.display());
    }

    let metadata =
        std::fs::metadata(path).with_context(|| format!("无法读取文件信息: {}", path.display()))?;

    let mut file = File::open(path).with_context(|| format!("无法打开文件: {}", path.display()))?;

    let hash = hash_reader(&mut file, HASH_BYTES)?;

    let name = path
        .file_stem()
        .and_then(|v| v.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("video")
                .to_owned()
        });

    Ok(FileInfo {
        name,
        size: metadata.len(),
        first_16m_md5: hash,
    })
}

fn inspect_http(input: &str, url: &Url) -> anyhow::Result<FileInfo> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("创建 HTTP 客户端失败")?;

    let response = client.head(input).send().context("HTTP HEAD 请求失败")?;

    let mut size = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let range_end = HASH_BYTES - 1;
    let response = client
        .get(input)
        .header(reqwest::header::RANGE, format!("bytes=0-{range_end}"))
        .send()
        .context("HTTP Range 请求失败")?;

    if response.status().is_success() || response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        if let Some(total) =
            parse_content_range_total(response.headers().get(reqwest::header::CONTENT_RANGE))
        {
            size = Some(total);
        }

        let mut take = response.take(HASH_BYTES);
        let mut data = Vec::with_capacity(HASH_BYTES as usize);
        take.read_to_end(&mut data)
            .context("读取远程视频前 16 MiB 失败")?;

        if size.is_none() && data.len() < HASH_BYTES as usize {
            size = Some(data.len() as u64);
        }

        let hash = md5_bytes(&data);
        let name = url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .and_then(|s| s.rsplit_once('.').map(|x| x.0).or(Some(s)))
            .filter(|s| !s.is_empty())
            .unwrap_or("video")
            .to_owned();

        return Ok(FileInfo {
            name,
            size: size.ok_or_else(|| anyhow::anyhow!("远程 HTTP 资源缺少文件总大小"))?,
            first_16m_md5: hash,
        });
    }

    bail!("HTTP 下载前 16 MiB 失败: {}", response.status())
}

fn inspect_ftp(url: &Url) -> anyhow::Result<FileInfo> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("FTP URL 缺少主机名"))?;
    let port = url.port().unwrap_or(21);

    let mut ftp = FtpStream::connect((host, port)).context("连接 FTP 服务器失败")?;

    let username = if url.username().is_empty() {
        "anonymous"
    } else {
        url.username()
    };

    let password = url.password().unwrap_or("anonymous@example.com");

    ftp.login(username, password).context("FTP 登录失败")?;

    let remote_path = url.path();
    if remote_path.is_empty() || remote_path == "/" {
        bail!("FTP URL 缺少文件路径");
    }

    let size = ftp.size(remote_path).context("FTP SIZE 请求失败")? as u64;

    let hash = ftp
        .retr(remote_path, |stream| {
            let mut limited = stream.take(HASH_BYTES);
            let mut data = Vec::with_capacity(HASH_BYTES as usize);
            limited
                .read_to_end(&mut data)
                .map_err(suppaftp::FtpError::ConnectionError)?;
            Ok(md5_bytes(&data))
        })
        .context("FTP 下载文件前 16 MiB 失败")?;

    let _ = ftp.quit();

    let name = remote_path
        .rsplit('/')
        .next()
        .unwrap_or("video")
        .rsplit_once('.')
        .map(|x| x.0)
        .unwrap_or_else(|| remote_path.rsplit('/').next().unwrap_or("video"))
        .to_owned();

    Ok(FileInfo {
        name,
        size,
        first_16m_md5: hash,
    })
}

fn hash_reader<R: Read>(reader: &mut R, limit: u64) -> anyhow::Result<String> {
    let mut hasher = Md5::new();
    let mut remaining = limit;
    let mut buf = [0u8; 1024 * 1024];

    while remaining > 0 {
        let nmax = remaining.min(buf.len() as u64) as usize;
        let n = reader.read(&mut buf[..nmax]).context("读取文件失败")?;

        if n == 0 {
            break;
        }

        hasher.update(&buf[..n]);
        remaining -= n as u64;
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn md5_bytes(data: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn parse_content_range_total(value: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let value = value?.to_str().ok()?;
    let total = value.split('/').nth(1)?;
    if total == "*" {
        None
    } else {
        total.parse().ok()
    }
}
