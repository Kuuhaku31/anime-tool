use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, bail};
use md5::{Digest, Md5};
use suppaftp::FtpStream;
use url::Url;
use urlencoding;

pub const HASH_BYTES: u64 = 16 * 1024 * 1024;

pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub first_16m_md5: String,
}

pub fn inspect(input: &str) -> anyhow::Result<FileInfo> {
    if let Ok(url) = Url::parse(input) {
        match url.scheme() {
            "http" | "https" => inspect_http(&url),
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

fn inspect_http(url: &Url) -> anyhow::Result<FileInfo> {
    println!("\n开始解析 HTTP(S) URL: {}", url);
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("创建 HTTP 客户端失败")?;

    let mut response = client
        .get(url.clone())
        .header(reqwest::header::RANGE, format!("bytes=0-{}", HASH_BYTES - 1))
        .send()
        .context("HTTP Range 请求失败")?;

    println!("响应状态: {}", response.status());
    for (name, value) in response.headers() {
        println!("\t{name}: {value:?}");
    }

    let file_name = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let file_name = urlencoding::decode(&file_name)
        .map(|value| value.to_string())
        .unwrap_or(file_name);

    let content_range = response.headers().get("Content-Range");
    let file_size = if let Some(content_range) = content_range {
        let value = content_range.to_str()?;
        value
            .rsplit_once('/')
            .and_then(|(_, size)| size.parse::<u64>().ok())
            .ok_or_else(|| anyhow::anyhow!("无效的 Content-Range"))?
    } else if let Some(content_length) = response.headers().get("Content-Length") {
        content_length.to_str()?.parse::<u64>()?
    } else {
        bail!("服务器未提供文件大小信息");
    };

    let mut data = Vec::with_capacity(HASH_BYTES as usize);
    let mut buffer = [0u8; 64 * 1024];
    while data.len() < HASH_BYTES as usize {
        let remain = HASH_BYTES as usize - data.len();
        let read_size = remain.min(buffer.len());
        let count = response.read(&mut buffer[..read_size])?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..count]);
    }

    if !data.is_empty() {
        let hash = md5_bytes(&data);
        println!(
            "解析完成: name='{}', size={}, first_16m_md5={}\n",
            file_name, file_size, hash
        );
        return Ok(FileInfo {
            name: file_name,
            size: file_size,
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
