//! Minimal localhost HTTP/1.1 client (plumbing for the parity harness).
//! Supports exactly what the harness needs and nothing more:
//!
//! - plain `http://` to loopback only,
//! - Basic auth,
//! - `Content-Length`, `chunked`, and close-delimited response bodies,
//! - incremental body streaming for SSE subscriptions.
//!
//! No TLS, no redirects, no compression (requests send
//! `Accept-Encoding: identity`). This keeps the harness free of heavyweight
//! HTTP dependencies so it can build and run hermetically.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// A fully-buffered HTTP response.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    /// Header names lowercased, values trimmed.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }
}

/// A parsed `http://host:port` base URL.
pub(crate) struct Endpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
}

pub(crate) fn parse_base(base: &str) -> Result<Endpoint> {
    let rest = base
        .strip_prefix("http://")
        .ok_or_else(|| anyhow!("parity harness only supports http:// base urls, got {base}"))?;
    let authority = rest.trim_end_matches('/');
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host.to_string(), port.parse::<u16>().context("invalid port in base url")?)
        }
        _ => (authority.to_string(), 80),
    };
    // IPv6 literal: strip the brackets for ToSocketAddrs (getaddrinfo rejects
    // "[::1]"); keep the bracketed display form for the Host header (RFC 7230).
    let host = host
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .map(|inner| inner.to_string())
        .unwrap_or(host);
    Ok(Endpoint { host, port })
}

/// RFC 4648 standard base64 (with padding), enough for Basic auth.
pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let triple = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(TABLE[(triple >> 18) as usize & 0x3f] as char);
        out.push(TABLE[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 { TABLE[(triple >> 6) as usize & 0x3f] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[triple as usize & 0x3f] as char } else { '=' });
    }
    out
}

/// `Basic <base64(user:password)>` header value.
#[must_use]
pub fn basic_auth(user: &str, password: &str) -> String {
    format!("Basic {}", base64_encode(format!("{user}:{password}").as_bytes()))
}

fn build_request(
    endpoint: &Endpoint,
    method: &str,
    path: &str,
    auth: Option<&str>,
    accept: &str,
    body: Option<&str>,
    connection: &str,
) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut head = String::new();
    // RFC 7230: an IPv6 literal in the Host header keeps its brackets.
    let host_display = if endpoint.host.contains(':') {
        format!("[{}]", endpoint.host)
    } else {
        endpoint.host.clone()
    };
    let _ = write!(
        head,
        "{method} {path} HTTP/1.1\r\nHost: {host_display}:{}\r\nAccept: {accept}\r\nAccept-Encoding: identity\r\nConnection: {connection}\r\n",
        endpoint.port
    );
    if let Some(auth) = auth {
        let _ = write!(head, "Authorization: {auth}\r\n");
    }
    if let Some(body) = body {
        let _ =
            write!(head, "Content-Type: application/json\r\nContent-Length: {}\r\n", body.len());
    }
    head.push_str("\r\n");
    let mut request = head.into_bytes();
    if let Some(body) = body {
        request.extend_from_slice(body.as_bytes());
    }
    request
}

async fn connect(endpoint: &Endpoint, timeout: Duration) -> Result<TcpStream> {
    let stream =
        tokio::time::timeout(timeout, TcpStream::connect((endpoint.host.as_str(), endpoint.port)))
            .await
            .map_err(|_| anyhow!("connect timeout to 127.0.0.1:{}", endpoint.port))?
            .context("connect failed")?;
    Ok(stream)
}

async fn write_all(stream: &mut TcpStream, request: &[u8], timeout: Duration) -> Result<()> {
    tokio::time::timeout(timeout, stream.write_all(request))
        .await
        .map_err(|_| anyhow!("write timeout"))?
        .context("write failed")?;
    Ok(())
}

/// Read the status line + headers. Returns them plus any body bytes that
/// arrived in the same read (which must seed the body stream's backlog).
async fn read_head(
    stream: &mut TcpStream,
    timeout: Duration,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(timeout, stream.read(&mut chunk))
            .await
            .map_err(|_| anyhow!("timeout waiting for response headers"))?
            .context("connection closed before response headers")?;
        if n == 0 {
            return Err(anyhow!("connection closed before response headers"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if find_head_end(&buf).is_some() {
            break;
        }
    }
    let (status, headers, head_end) = parse_head(&buf)?;
    Ok((status, headers, buf[head_end..].to_vec()))
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Parsed response head: status, headers (lowercased names), and the byte
/// offset just past the `\r\n\r\n` terminator.
type Head = (u16, Vec<(String, String)>, usize);

fn parse_head(buf: &[u8]) -> Result<Head> {
    let head_end = find_head_end(buf).ok_or_else(|| anyhow!("response head not terminated"))?;
    let head = std::str::from_utf8(&buf[..head_end]).context("response head is not utf-8")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or_else(|| anyhow!("empty response head"))?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/1.") {
        return Err(anyhow!("unexpected protocol in status line: {status_line}"));
    }
    let status = parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("unparseable status line: {status_line}"))?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    Ok((status, headers, head_end))
}

/// Incrementally-decoded HTTP response body. Yields decoded bytes across
/// Content-Length, chunked, and close-delimited framings.
pub(crate) struct BodyStream {
    stream: TcpStream,
    backlog: Vec<u8>,
    chunked: bool,
    /// Bytes remaining in the current extent: whole body when not chunked,
    /// current chunk when chunked. `None` until first extent is known
    /// (chunked only).
    remaining: Option<u64>,
    /// Chunked: a data extent just ended and its trailing CRLF is pending.
    crlf_pending: bool,
    eof: bool,
}

impl BodyStream {
    fn from_headers(stream: TcpStream, headers: &[(String, String)], initial: Vec<u8>) -> Self {
        let chunked = headers.iter().any(|(name, value)| {
            name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked")
        });
        let remaining = if chunked {
            None
        } else {
            headers
                .iter()
                .find(|(name, _)| name == "content-length")
                .and_then(|(_, value)| value.trim().parse::<u64>().ok())
        };
        Self { stream, backlog: initial, chunked, remaining, crlf_pending: false, eof: false }
    }

    /// Read raw bytes from the socket into the backlog. Returns bytes read
    /// (0 means EOF).
    async fn fill(&mut self, timeout: Duration) -> Result<usize> {
        let mut chunk = [0u8; 8192];
        let n = tokio::time::timeout(timeout, self.stream.read(&mut chunk))
            .await
            .map_err(|_| anyhow!("timeout reading response body"))?
            .context("connection closed mid-body")?;
        self.backlog.extend_from_slice(&chunk[..n]);
        Ok(n)
    }

    fn take_backlog(&mut self, max: usize) -> Vec<u8> {
        let n = self.backlog.len().min(max);
        let out = self.backlog.drain(..n).collect();
        out
    }

    async fn read_line(&mut self, timeout: Duration) -> Result<String> {
        loop {
            if let Some(pos) = self.backlog.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.backlog.drain(..=pos).collect();
                let mut text = String::from_utf8_lossy(&line).into_owned();
                if text.ends_with('\n') {
                    text.pop();
                    if text.ends_with('\r') {
                        text.pop();
                    }
                }
                return Ok(text);
            }
            if self.fill(timeout).await? == 0 {
                return Err(anyhow!("connection closed inside chunked framing"));
            }
        }
    }

    async fn read_exact_backlog(&mut self, n: usize, timeout: Duration) -> Result<()> {
        while self.backlog.len() < n {
            if self.fill(timeout).await? == 0 {
                return Err(anyhow!("connection closed inside chunked framing"));
            }
        }
        self.backlog.drain(..n);
        Ok(())
    }

    /// Next slab of decoded body bytes, or `None` once the body is complete.
    pub(crate) async fn next_bytes(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        if self.eof {
            return Ok(None);
        }
        if !self.chunked {
            let remaining = self.remaining.unwrap_or(0);
            if remaining == 0 {
                // Framed by connection close (no content-length): read until EOF.
                if self.remaining.is_none() {
                    if !self.backlog.is_empty() {
                        return Ok(Some(self.take_backlog(usize::MAX)));
                    }
                    if self.fill(timeout).await? == 0 {
                        self.eof = true;
                        return Ok(None);
                    }
                    return Ok(Some(self.take_backlog(usize::MAX)));
                }
                self.eof = true;
                return Ok(None);
            }
            let want = remaining.min(8192) as usize;
            while self.backlog.is_empty() {
                if self.fill(timeout).await? == 0 {
                    return Err(anyhow!("connection closed before content-length body completed"));
                }
            }
            let out = self.take_backlog(want);
            self.remaining = Some(remaining - out.len() as u64);
            return Ok(Some(out));
        }
        // Chunked framing.
        if self.crlf_pending {
            self.read_exact_backlog(2, timeout).await?;
            self.crlf_pending = false;
        }
        let remaining = match self.remaining {
            Some(0) => {
                let line = self.read_line(timeout).await?;
                let size_text = line.split(';').next().unwrap_or_default().trim();
                let size = u64::from_str_radix(size_text, 16)
                    .with_context(|| format!("invalid chunk size line {line:?}"))?;
                if size == 0 {
                    // Trailers: consume lines until the blank terminator.
                    loop {
                        if self.read_line(timeout).await?.is_empty() {
                            self.eof = true;
                            return Ok(None);
                        }
                    }
                }
                size
            }
            Some(n) => n,
            None => {
                let line = self.read_line(timeout).await?;
                let size_text = line.split(';').next().unwrap_or_default().trim();
                let size = u64::from_str_radix(size_text, 16)
                    .with_context(|| format!("invalid chunk size line {line:?}"))?;
                if size == 0 {
                    loop {
                        if self.read_line(timeout).await?.is_empty() {
                            self.eof = true;
                            return Ok(None);
                        }
                    }
                }
                size
            }
        };
        let want = remaining.min(8192) as usize;
        while self.backlog.is_empty() {
            if self.fill(timeout).await? == 0 {
                return Err(anyhow!("connection closed inside chunk"));
            }
        }
        let out = self.take_backlog(want);
        let new_remaining = remaining - out.len() as u64;
        self.remaining = Some(new_remaining);
        if new_remaining == 0 {
            self.crlf_pending = true;
        }
        Ok(Some(out))
    }
}

/// Buffer a full response for `method path` on `base`.
pub async fn request(
    base: &str,
    auth: Option<&str>,
    method: &str,
    path: &str,
    body: Option<&str>,
    accept: &str,
    timeout: Duration,
) -> Result<Response> {
    let endpoint = parse_base(base)?;
    let request = build_request(&endpoint, method, path, auth, accept, body, "close");
    let mut stream = connect(&endpoint, timeout).await?;
    write_all(&mut stream, &request, timeout).await?;
    let (status, headers, initial) = read_head(&mut stream, timeout).await?;
    let mut body_stream = BodyStream::from_headers(stream, &headers, initial);
    let mut body = Vec::new();
    while let Some(bytes) = body_stream.next_bytes(timeout).await? {
        body.extend_from_slice(&bytes);
    }
    Ok(Response { status, headers, body })
}

/// Open a long-lived streaming GET (used for text/event-stream). Returns the
/// status plus a body stream the caller reads incrementally.
pub(crate) async fn stream_request(
    base: &str,
    auth: Option<&str>,
    accept: &str,
    path: &str,
    timeout: Duration,
) -> Result<(u16, Vec<(String, String)>, BodyStream)> {
    let endpoint = parse_base(base)?;
    let request = build_request(&endpoint, "GET", path, auth, accept, None, "keep-alive");
    let mut stream = connect(&endpoint, timeout).await?;
    write_all(&mut stream, &request, timeout).await?;
    let (status, headers, initial) = read_head(&mut stream, timeout).await?;
    let body = BodyStream::from_headers(stream, &headers, initial);
    Ok((status, headers, body))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(5);

    /// Serve one canned raw HTTP response, then close.
    async fn serve_once(response: &'static [u8]) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut sink = [0u8; 512];
                let _ = socket.read(&mut sink).await;
                let _ = socket.write_all(response).await;
            }
        });
        base
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b"opencode:pw"), "b3BlbmNvZGU6cHc=");
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[tokio::test]
    async fn ipv6_literal_base_connects() {
        // gate5's dual-stack poll hit exactly this: http://[::1]:port must
        // parse (brackets stripped for ToSocketAddrs) and reach a real
        // loopback listener.
        let listener = tokio::net::TcpListener::bind("[::1]:0").await.expect("bind [::1]");
        let port = listener.local_addr().expect("addr").port();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
            let (mut sock, _) = listener.accept().await.expect("accept");
            let mut buf = [0u8; 512];
            let n = sock.read(&mut buf).await.expect("read");
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            assert!(head.contains("Host: [::1]"), "Host header: {head}");
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await.expect("write");
        });
        let response = crate::minih::request(
            &format!("http://[::1]:{port}"),
            None,
            "GET",
            "/api/info",
            None,
            "application/json",
            Duration::from_secs(5),
        )
        .await
        .expect("request over [::1]");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
        server.await.expect("server join");
    }

    #[test]
    fn parses_base_urls() {
        let e = parse_base("http://127.0.0.1:49374").unwrap();
        assert_eq!((e.host.as_str(), e.port), ("127.0.0.1", 49374));
        let e = parse_base("http://localhost").unwrap();
        assert_eq!((e.host.as_str(), e.port), ("localhost", 80));
        assert!(parse_base("https://example.com").is_err());
    }

    #[test]
    fn parses_response_heads() {
        let raw = b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\r\n{\"x\":1}";
        let (status, headers, head_end) = parse_head(raw).unwrap();
        assert_eq!(status, 401);
        assert_eq!(headers, vec![("content-type".to_string(), "application/json".to_string())]);
        assert_eq!(head_end, raw.len() - 7);
    }

    #[tokio::test]
    async fn buffers_content_length_response() {
        let base = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"version\":1}",
        )
        .await;
        let response = request(
            &base,
            Some(&basic_auth("opencode", "pw")),
            "GET",
            "/api/info",
            None,
            "application/json",
            TIMEOUT,
        )
        .await
        .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{\"version\":1}");
    }

    #[tokio::test]
    async fn decodes_chunked_body() {
        // "data: hi" in one chunk, "!" in a second, then terminator + trailer.
        let base =
            serve_once(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\ndata: hi\r\n1\r\n!\r\n0\r\nX-T: v\r\n\r\n")
                .await;
        let (_, headers, mut body) =
            stream_request(&base, None, "text/event-stream", "/api/event", TIMEOUT).await.unwrap();
        assert_eq!(headers[0].1, "chunked");
        let mut decoded = Vec::new();
        while let Some(bytes) = body.next_bytes(TIMEOUT).await.unwrap() {
            decoded.extend_from_slice(&bytes);
        }
        assert_eq!(decoded, b"data: hi!");
    }

    #[tokio::test]
    async fn handles_close_delimited_body() {
        let base =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nno length, closed")
                .await;
        let response = request(&base, None, "GET", "/", None, "*/*", TIMEOUT).await.unwrap();
        assert_eq!(response.body, b"no length, closed");
    }
}
