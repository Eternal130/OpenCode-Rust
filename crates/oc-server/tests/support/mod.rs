//! Raw HTTP/1.1 test client (the crate's dev-dependencies do not include an
//! HTTP client; full control over headers, HEAD, incremental SSE reads, and
//! deliberately-unread bodies is exactly what these tests need).

#![allow(dead_code)]

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct RawResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("valid json body")
    }
}

pub async fn request(
    addr: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<RawResponse> {
    let mut writer = TcpStream::connect(addr).await?;
    write_request(&mut writer, &addr.to_string(), method, target, headers, body).await?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = timeout_read(&mut writer, &mut chunk, Duration::from_secs(10)).await?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    parse(&raw)
}

/// Streaming variant: returns once headers arrived, body readable in pieces.
pub async fn request_streaming(
    addr: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<(RawResponse, StreamingBody)> {
    let mut stream = TcpStream::connect(addr).await?;
    write_request(&mut stream, &addr.to_string(), method, target, headers, body).await?;
    let mut head = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = timeout_read(&mut stream, &mut chunk, Duration::from_secs(10)).await?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no response"));
        }
        head.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_head_end(&head) {
            let raw_head = head[..position].to_vec();
            let response = parse_head(&raw_head)?;
            let rest = head[position + 4..].to_vec();
            let streaming = StreamingBody {
                stream,
                buf: rest,
                chunked: response
                    .header("transfer-encoding")
                    .is_some_and(|value| value.contains("chunked")),
                remaining: 0,
                done: false,
            };
            return Ok((response, streaming));
        }
    }
}

async fn write_request(
    stream: &mut TcpStream,
    host: &str,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    write_request_public(stream, host, method, target, headers, body).await
}

/// Writes a raw request on an already-connected stream.
pub async fn write_request_public(
    stream: &mut TcpStream,
    host: &str,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    use std::fmt::Write as _;
    let mut request = format!(
        "{method} {target} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        let _ = write!(request, "{name}: {value}\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(body).await?;
    Ok(())
}

async fn timeout_read(
    stream: &mut TcpStream,
    chunk: &mut [u8],
    limit: Duration,
) -> io::Result<usize> {
    match tokio::time::timeout(limit, stream.read(chunk)).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "read timed out")),
    }
}

fn find_head_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse(raw: &[u8]) -> io::Result<RawResponse> {
    let position = find_head_end(raw)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated response head"))?;
    let mut response = parse_head(&raw[..position])?;
    let rest = &raw[position + 4..];
    if let Some(length) =
        response.header("content-length").and_then(|value| value.parse::<usize>().ok())
    {
        response.body = rest[..length.min(rest.len())].to_vec();
    } else if response.header("transfer-encoding").is_some_and(|value| value.contains("chunked")) {
        response.body = dechunk(rest);
    } else {
        response.body = rest.to_vec();
    }
    Ok(response)
}

fn parse_head(head: &[u8]) -> io::Result<RawResponse> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let status_line =
        lines.next().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty response"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "malformed status"))?;
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    Ok(RawResponse { status, headers, body: Vec::new() })
}

fn dechunk(mut input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(line_end) = input.windows(2).position(|window| window == b"\r\n") {
        let header = String::from_utf8_lossy(&input[..line_end]).into_owned();
        let Ok(size) =
            usize::from_str_radix(header.split(';').next().unwrap_or_default().trim(), 16)
        else {
            break;
        };
        if size == 0 {
            break;
        }
        let start = line_end + 2;
        let end = (start + size).min(input.len());
        out.extend_from_slice(&input[start..end]);
        input = &input[end.min(input.len())..];
        if input.starts_with(b"\r\n") {
            input = &input[2..];
        }
    }
    out
}

/// Incrementally readable (chunk-aware) response body.
pub struct StreamingBody {
    stream: TcpStream,
    buf: Vec<u8>,
    chunked: bool,
    remaining: usize,
    done: bool,
}

impl StreamingBody {
    /// Takes a connected stream after the request was written: consumes the
    /// response head, then exposes the (chunk-aware) body.
    pub async fn from_stream_after_head(mut stream: TcpStream, head_budget: usize) -> Self {
        let mut head = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let read = timeout_read(&mut stream, &mut chunk, Duration::from_secs(10))
                .await
                .expect("response head");
            assert!(read != 0, "no response head received");
            head.extend_from_slice(&chunk[..read]);
            assert!(head.len() <= head_budget, "response head too large");
            if let Some(position) = find_head_end(&head) {
                let response = parse_head(&head[..position]).expect("valid head");
                let chunked = response
                    .header("transfer-encoding")
                    .is_some_and(|value| value.contains("chunked"));
                let leftover = head[position + 4..].to_vec();
                return StreamingBody { stream, buf: leftover, chunked, remaining: 0, done: false };
            }
        }
    }

    /// Next decoded bytes; empty vector = stream ended.
    pub async fn read_some(&mut self, limit: Duration) -> io::Result<Vec<u8>> {
        if self.done {
            return Ok(Vec::new());
        }
        loop {
            if self.buf.is_empty() {
                let mut chunk = [0u8; 8192];
                let read = timeout_read(&mut self.stream, &mut chunk, limit).await;
                match read {
                    Ok(0) => {
                        self.done = true;
                        return Ok(std::mem::take(&mut self.buf));
                    }
                    Ok(read) => self.buf.extend_from_slice(&chunk[..read]),
                    Err(error) => {
                        self.done = true;
                        return Err(error);
                    }
                }
            }
            if !self.chunked {
                return Ok(std::mem::take(&mut self.buf));
            }
            // Chunked: emit payload slices between chunk headers.
            if self.remaining == 0 {
                if let Some(line_end) = self.buf.windows(2).position(|window| window == b"\r\n") {
                    let header = String::from_utf8_lossy(&self.buf[..line_end]).into_owned();
                    self.buf.drain(..line_end + 2);
                    let size_text = header.split(';').next().unwrap_or_default().trim().to_owned();
                    let Ok(size) = usize::from_str_radix(&size_text, 16) else {
                        self.done = true;
                        return Ok(Vec::new());
                    };
                    if size == 0 {
                        self.done = true;
                        return Ok(Vec::new());
                    }
                    self.remaining = size;
                    continue;
                }
                // Need more bytes for the chunk header.
                continue;
            }
            let take = self.remaining.min(self.buf.len());
            let out: Vec<u8> = self.buf.drain(..take).collect();
            self.remaining -= take;
            if self.remaining == 0 && self.buf.starts_with(b"\r\n") {
                self.buf.drain(..2);
            }
            return Ok(out);
        }
    }

    /// Reads until the accumulated output contains `needle` or the stream
    /// ends / times out.
    pub async fn read_until(&mut self, needle: &str, limit: Duration) -> io::Result<String> {
        let deadline = std::time::Instant::now() + limit;
        let mut seen = String::new();
        while std::time::Instant::now() < deadline {
            let budget = deadline.saturating_duration_since(std::time::Instant::now());
            if budget.is_zero() {
                break;
            }
            let chunk = self.read_some(budget).await?;
            if chunk.is_empty() {
                break;
            }
            seen.push_str(&String::from_utf8_lossy(&chunk));
            if seen.contains(needle) {
                return Ok(seen);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("did not see {needle:?}; got {seen:?}"),
        ))
    }

    /// Never reads; closes the connection.
    pub fn ignore(self) {
        drop(self.stream);
    }
}
