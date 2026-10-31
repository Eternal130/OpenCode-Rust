//! WebSocket proxy support: the v2 stack routes WebSocket
//! connections through `http(s)-proxy-agent`, so `HTTP_PROXY` /
//! `HTTPS_PROXY` / `ALL_PROXY` (and `NO_PROXY`) apply to WS transports
//! the same way reqwest applies them to HTTP. The rust port's WS
//! connector dials directly, so this module selects a proxy for a
//! target URL from the environment and tunnels the WebSocket through
//! it: `http://` proxies get a plain CONNECT, `https://` proxies get a
//! TLS leg to the proxy first (rustls with webpki roots — intentional
//! for this WS stack; the 0.13 reqwest stack uses the platform
//! verifier instead, see G-26) then CONNECT inside it, and
//! `socks5://` / `socks5h://` / `socks://` proxies (alias socks5,
//! default port 1080) run the SOCKS5 handshake via `tokio-socks`
//! (new dependency: the SOCKS5 greeting/method/connect exchange is a
//! protocol of its own; nothing else in the tree carries it).
//! `socks4://` stays unsupported. Proxy selection + `NO_PROXY` are
//! pure functions so tests can drive them without process env races.

use std::time::Duration;

use crate::schema::{AiError, TransportDelivery, TransportOperation, TransportPhase};

use super::websocket::transport_error;

/// Environment lookup seam: tests pass a map, production passes
/// `std::env::var`.
pub type EnvLookup<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// v2 `truthy` (server-process.ts:191): exactly `"1"` or the
/// case-insensitive `"true"`.
#[must_use]
pub fn env_truthy(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

/// Selects the proxy URL for `target` per the standard env precedence:
/// secure targets (`wss`/`https`) honor `HTTPS_PROXY` first, plain
/// targets (`ws`/`http`) honor `HTTP_PROXY` first, and `ALL_PROXY`
/// covers both; lowercase spellings are checked after the canonical
/// ones. `NO_PROXY` (comma-separated host suffixes, `*` = bypass all)
/// removes a matching target.
#[must_use]
pub fn select_proxy(target: &str, env: &EnvLookup<'_>) -> Option<String> {
    let secure = match target.split_once("://") {
        Some((scheme, _)) => match scheme.to_ascii_lowercase().as_str() {
            "wss" | "https" => true,
            "ws" | "http" => false,
            // WS transport only opens ws/wss (http/https kept for callers);
            // anything else never goes through the WS proxy seam.
            _ => return None,
        },
        None => false,
    };
    let candidates: &[&str] = if secure {
        &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    let proxy = candidates.iter().find_map(|name| env(name).filter(|value| !value.is_empty()))?;
    let host = target_host(target)?;
    let port = target_port(target);
    let effective_port = port.or_else(|| default_port(target));
    if no_proxy_bypasses(
        &host,
        effective_port,
        env("NO_PROXY").or_else(|| env("no_proxy")).as_deref(),
    ) {
        return None;
    }
    Some(proxy)
}

/// The scheme's default port (wss/https → 443, ws/http → 80) so
/// `NO_PROXY=host:443` also bypasses an implicit-port `wss://host`
/// (proxy-from-env compares the EFFECTIVE port).
fn default_port(target: &str) -> Option<u16> {
    match target.split_once("://").map(|(scheme, _)| scheme)? {
        "wss" | "https" => Some(443),
        "ws" | "http" => Some(80),
        _ => None,
    }
}

/// The lowercase host of a URL (brackets stripped from IPv6 literals).
fn target_host(target: &str) -> Option<String> {
    let rest = target.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    let host = authority.split(':').next()?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// The URL's explicit port (defaults live with the caller — the proxy
/// side only needs the port `NO_PROXY` entries may pin).
fn target_port(target: &str) -> Option<u16> {
    let rest = target.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    let port = authority.rsplit_once(':').and_then(|(host, port)| {
        // only a real trailing port (an IPv6 literal keeps its colons)
        (!host.is_empty() && !host.contains(':')).then(|| port.parse::<u16>().ok()).flatten()
    })?;
    Some(port)
}

/// `NO_PROXY` matching (the `proxy-from-env` semantics v2 inherits):
/// comma-separated entries, `*` bypasses everything, an entry matches
/// when it equals the host or is a dot-separated suffix of it (leading
/// dots are optional). An entry may pin a port (`host:port`), in which
/// case the target's port must match too.
#[must_use]
pub fn no_proxy_bypasses(host: &str, port: Option<u16>, no_proxy: Option<&str>) -> bool {
    let Some(no_proxy) = no_proxy else { return false };
    let host = host.to_ascii_lowercase();
    for entry in no_proxy.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if entry == "*" {
            return true;
        }
        let (entry_host, entry_port) = match entry.rsplit_once(':') {
            // a bare IPv6 literal keeps its colons (only strip a real port)
            Some((host, port)) if !host.contains(':') => (host, port),
            _ => (entry, ""),
        };
        let entry_host = entry_host.trim_start_matches('.').to_ascii_lowercase();
        if entry_host.is_empty() {
            continue;
        }
        let host_match = host == entry_host || host.ends_with(&format!(".{entry_host}"));
        if !host_match {
            continue;
        }
        if entry_port.is_empty() {
            return true;
        }
        let port_match = entry_port.parse::<u16>().is_ok_and(|entry_port| Some(entry_port) == port);
        if port_match {
            return true;
        }
    }
    false
}

/// The tunnel's proxy scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyScheme {
    /// Plain TCP + `CONNECT` (default port 80).
    Http,
    /// TLS to the proxy (rustls), then `CONNECT` inside it (default
    /// port 443).
    Https,
    /// SOCKS5 handshake (`tokio-socks`, default port 1080).
    Socks5,
}

/// A parsed proxy endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
    pub scheme: ProxyScheme,
    /// The raw URL the endpoint came from (error reporting).
    pub source: String,
}

/// Parses a proxy URL into a dialable endpoint. `http://` and
/// scheme-less `host[:port]` parse (default port 80), `https://`
/// parses (default 443), `socks5://` / `socks5h://` / `socks://` parse
/// as SOCKS5 (default 1080); `socks4`/`socks4a` stay unsupported.
pub fn parse_proxy_url(url: &str) -> Result<ProxyEndpoint, String> {
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("http".to_string(), url),
    };
    let scheme = match scheme.as_str() {
        "http" => ProxyScheme::Http,
        "https" => ProxyScheme::Https,
        "socks5" | "socks5h" | "socks" => ProxyScheme::Socks5,
        other if other.starts_with("socks") => {
            return Err(format!(
                "{other}:// proxy URLs are not supported for WebSocket tunnels ({url})"
            ));
        }
        other => {
            return Err(format!("unsupported proxy URL scheme {other:?} ({url})"));
        }
    };
    let default_port = match scheme {
        ProxyScheme::Http => 80,
        ProxyScheme::Https => 443,
        ProxyScheme::Socks5 => 1080,
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    if authority.is_empty() {
        return Err(format!("proxy URL has no host ({url})"));
    }
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        // [ipv6]:port
        let Some((host, rest)) = bracketed.split_once(']') else {
            return Err(format!("malformed IPv6 proxy host in {url}"));
        };
        let port = rest.strip_prefix(':').unwrap_or("");
        if !port.is_empty() && !port.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("invalid proxy port in {url}"));
        }
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() || port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("invalid proxy authority in {url}"));
        }
        (host, port)
    } else {
        (authority, "")
    };
    let host = host.to_string();
    let port: Option<u16> = if port.is_empty() {
        None
    } else {
        Some(port.parse().map_err(|_| format!("invalid proxy port in {url}"))?)
    };
    Ok(ProxyEndpoint { host, port: port.unwrap_or(default_port), scheme, source: url.to_string() })
}

/// The default CONNECT budget (v2 `CONNECT_TIMEOUT` is 15s; the tunnel
/// setup shares it).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Response-header cap for the CONNECT reply.
const MAX_CONNECT_RESPONSE: usize = 8 * 1024;

/// Resolves the proxy endpoint for a WS target URL from the real
/// process environment, or `None` when no proxy applies.
pub fn resolve_for(target: &str) -> Result<Option<ProxyEndpoint>, AiError> {
    let invalid = |message: String| {
        transport_error(
            message,
            TransportOperation::Request,
            Some(TransportPhase::Connect),
            Some(TransportDelivery::NotSent),
            Some("invalid-proxy".into()),
            Some(target.to_string()),
        )
    };
    match select_proxy(target, &|name| std::env::var(name).ok()) {
        Some(url) => parse_proxy_url(&url).map(Some).map_err(invalid),
        None => Ok(None),
    }
}

/// The established tunnel: a plain TCP stream, or a TLS session to an
/// `https://` proxy that the CONNECT rode inside of.
pub enum ProxyTunnel {
    Plain(tokio::net::TcpStream),
    #[cfg(feature = "ws-transport")]
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl tokio::io::AsyncRead for ProxyTunnel {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            ProxyTunnel::Plain(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            #[cfg(feature = "ws-transport")]
            ProxyTunnel::Tls(stream) => std::pin::Pin::new(stream.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for ProxyTunnel {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match &mut *self {
            ProxyTunnel::Plain(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
            #[cfg(feature = "ws-transport")]
            ProxyTunnel::Tls(stream) => std::pin::Pin::new(stream.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            ProxyTunnel::Plain(stream) => std::pin::Pin::new(stream).poll_flush(cx),
            #[cfg(feature = "ws-transport")]
            ProxyTunnel::Tls(stream) => std::pin::Pin::new(stream.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match &mut *self {
            ProxyTunnel::Plain(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
            #[cfg(feature = "ws-transport")]
            ProxyTunnel::Tls(stream) => std::pin::Pin::new(stream.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Dials the proxy for `target_host:target_port` and returns the
/// tunnel ready for the WebSocket handshake: an `http://` proxy gets a
/// plain `CONNECT`, an `https://` proxy gets a rustls TLS leg first
/// (webpki roots — intentional for this WS transport stack; the 0.13
/// reqwest stack uses the platform verifier instead, see G-26), and a
/// `socks5` proxy runs the SOCKS5 handshake (`tokio-socks`; the target
/// host is resolved locally, matching `socks5`/`socks5h` aliasing here
/// since nothing remote resolves differently for WS targets).
pub async fn connect_through(
    proxy: &ProxyEndpoint,
    target_host: &str,
    target_port: u16,
) -> Result<ProxyTunnel, AiError> {
    #[cfg(feature = "ws-transport")]
    {
        match proxy.scheme {
            ProxyScheme::Http => {
                let stream = http_connect(proxy, target_host, target_port).await?;
                Ok(ProxyTunnel::Plain(stream))
            }
            ProxyScheme::Https => {
                let stream = https_connect(proxy, target_host, target_port, None).await?;
                Ok(ProxyTunnel::Tls(Box::new(stream)))
            }
            ProxyScheme::Socks5 => {
                let stream = socks5_connect(proxy, target_host, target_port).await?;
                Ok(ProxyTunnel::Plain(stream))
            }
        }
    }
    #[cfg(not(feature = "ws-transport"))]
    {
        match proxy.scheme {
            ProxyScheme::Http => {
                let stream = http_connect(proxy, target_host, target_port).await?;
                Ok(ProxyTunnel::Plain(stream))
            }
            _ => Err(transport_error(
                format!(
                    "{} proxy URLs require the ws-transport feature ({})",
                    if proxy.scheme == ProxyScheme::Https { "https" } else { "socks5" },
                    proxy.source
                ),
                TransportOperation::Request,
                Some(TransportPhase::Connect),
                Some(TransportDelivery::NotSent),
                Some("invalid-proxy".into()),
                Some(proxy.source.clone()),
            )),
        }
    }
}

/// The rustls connector an `https://` proxy's TLS leg uses (webpki
/// roots); tests inject a trust store carrying their stub's self-signed
/// certificate.
#[cfg(feature = "ws-transport")]
fn proxy_tls_connector(
    roots: Option<tokio_rustls::rustls::RootCertStore>,
) -> Result<tokio_rustls::TlsConnector, String> {
    use tokio_rustls::rustls::crypto::aws_lc_rs;
    // The workspace enables rustls' ring AND aws-lc-rs backends (via
    // tungstenite and reqwest), so the process-default provider is
    // ambiguous — pin the provider explicitly (aws-lc-rs, the same
    // stack the 0.13 side uses).
    let provider = std::sync::Arc::new(aws_lc_rs::default_provider());
    let builder = tokio_rustls::rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("rustls protocol versions: {error}"))?;
    let config = match roots {
        Some(roots) => builder.with_root_certificates(roots).with_no_client_auth(),
        None => builder
            .with_root_certificates(tokio_rustls::rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            })
            .with_no_client_auth(),
    };
    Ok(std::sync::Arc::new(config).into())
}

#[cfg(feature = "ws-transport")]
fn server_name(host: &str) -> Result<tokio_rustls::rustls::pki_types::ServerName<'static>, String> {
    tokio_rustls::rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| format!("invalid proxy host for TLS: {host}"))
}

/// TLS leg to an `https://` proxy, then `CONNECT` inside the session.
/// `connector` overrides the TLS config (tests' self-signed stubs).
#[cfg(feature = "ws-transport")]
async fn https_connect(
    proxy: &ProxyEndpoint,
    target_host: &str,
    target_port: u16,
    connector: Option<tokio_rustls::TlsConnector>,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, AiError> {
    let fail = |message: String| proxy_failure(proxy, message);
    let connector = match connector {
        Some(connector) => connector,
        None => proxy_tls_connector(None).map_err(fail)?,
    };
    let name = server_name(&proxy.host).map_err(|message| fail(message))?;
    let future = async {
        let tcp = tokio::net::TcpStream::connect((proxy.host.as_str(), proxy.port))
            .await
            .map_err(|error| format!("failed to dial proxy {}: {error}", proxy.source))?;
        let mut tls = connector
            .connect(name, tcp)
            .await
            .map_err(|error| format!("TLS to proxy {} failed: {error}", proxy.source))?;
        http_connect_over(&mut tls, proxy, target_host, target_port).await?;
        Ok(tls)
    };
    run_connect(proxy, future).await
}

/// SOCKS5 handshake (`tokio-socks`): greet → no-auth → CONNECT the
/// target; the returned stream is the raw tunnel.
#[cfg(feature = "ws-transport")]
async fn socks5_connect(
    proxy: &ProxyEndpoint,
    target_host: &str,
    target_port: u16,
) -> Result<tokio::net::TcpStream, AiError> {
    let future = async {
        let stream = tokio_socks::tcp::Socks5Stream::connect(
            (proxy.host.as_str(), proxy.port),
            (target_host.to_string(), target_port),
        )
        .await
        .map_err(|error| format!("SOCKS5 proxy {} failed: {error}", proxy.source))?;
        Ok(stream.into_inner())
    };
    run_connect(proxy, future).await
}

/// One `CONNECT host:port` exchange over an established (plain or TLS)
/// proxy stream.
async fn http_connect(
    proxy: &ProxyEndpoint,
    target_host: &str,
    target_port: u16,
) -> Result<tokio::net::TcpStream, AiError> {
    let future = async {
        let mut stream = tokio::net::TcpStream::connect((proxy.host.as_str(), proxy.port))
            .await
            .map_err(|error| format!("failed to dial proxy {}: {error}", proxy.source))?;
        http_connect_over(&mut stream, proxy, target_host, target_port).await?;
        Ok(stream)
    };
    run_connect(proxy, future).await
}

async fn http_connect_over<S>(
    stream: &mut S,
    _proxy: &ProxyEndpoint,
    target_host: &str,
    target_port: u16,
) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // NOTE: any bytes read past the CONNECT response header are discarded —
    // harmless for ws/wss because the CLIENT speaks first after the upgrade.
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let host_display = if target_host.contains(':') {
        format!("[{target_host}]")
    } else {
        target_host.to_string()
    };
    let request = format!(
        "CONNECT {host_display}:{target_port} HTTP/1.1\r\nHost: {host_display}:{target_port}\r\nProxy-Connection: keep-alive\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.map_err(|e| format!("proxy write failed: {e}"))?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.map_err(|e| format!("proxy read failed: {e}"))?;
        if read == 0 {
            return Err("proxy closed the connection before answering CONNECT".to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n")
            || buffer.len() > MAX_CONNECT_RESPONSE
        {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buffer);
    let status = head.lines().next().unwrap_or_default();
    let code = status.split_whitespace().nth(1).unwrap_or_default();
    if code.len() == 3 && code.starts_with('2') {
        Ok(())
    } else {
        Err(format!("proxy refused CONNECT: {status}"))
    }
}

/// Shared CONNECT-budget wrapper (v2 `CONNECT_TIMEOUT` is 15s).
async fn run_connect<T>(
    proxy: &ProxyEndpoint,
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, AiError> {
    match tokio::time::timeout(CONNECT_TIMEOUT, future).await {
        Ok(result) => result.map_err(|message| proxy_failure(proxy, message)),
        Err(_elapsed) => {
            Err(proxy_failure(proxy, "timed out establishing the proxy tunnel".into()))
        }
    }
}

fn proxy_failure(proxy: &ProxyEndpoint, message: String) -> AiError {
    transport_error(
        message,
        TransportOperation::Request,
        Some(TransportPhase::Connect),
        Some(TransportDelivery::NotSent),
        Some("proxy-connect".into()),
        Some(proxy.source.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map<'a>(env: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| env.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string())
    }

    #[test]
    fn selection_honors_scheme_precedence() {
        let env = map(&[("HTTPS_PROXY", "https-proxy:1"), ("HTTP_PROXY", "http-proxy:2")]);
        assert_eq!(select_proxy("wss://api.example.com/v1", &env), Some("https-proxy:1".into()));
        assert_eq!(select_proxy("ws://api.example.com/v1", &env), Some("http-proxy:2".into()));
        assert_eq!(
            select_proxy("https://api.example.com", &map(&[("ALL_PROXY", "any:3")])),
            Some("any:3".into())
        );
        // lowercase fallback after the canonical spelling
        assert_eq!(
            select_proxy("wss://a.test", &map(&[("https_proxy", "low:4")])),
            Some("low:4".into())
        );
        // empty values are ignored
        assert_eq!(select_proxy("wss://a.test", &map(&[("HTTPS_PROXY", "")])), None);
        assert_eq!(select_proxy("ftp://a.test", &env), None);
    }

    #[test]
    fn no_proxy_matches_suffixes_and_star() {
        assert!(no_proxy_bypasses("api.example.com", None, Some("api.example.com")));
        assert!(no_proxy_bypasses("API.EXAMPLE.COM", None, Some("example.com")));
        assert!(no_proxy_bypasses("sub.example.com", None, Some(".example.com")));
        assert!(no_proxy_bypasses("anything", None, Some("*")));
        assert!(no_proxy_bypasses("localhost", None, Some("localhost, other.test")));
        // port-pinned entries match only the pinned port
        assert!(no_proxy_bypasses("api.example.com", Some(8443), Some("api.example.com:8443")));
        assert!(!no_proxy_bypasses("api.example.com", Some(443), Some("api.example.com:8443")));
        assert!(!no_proxy_bypasses("api.example.com", None, Some("other.com")));
        assert!(!no_proxy_bypasses("api.example.com", None, Some("example.com.evil.com")));
        assert!(!no_proxy_bypasses("api.example.com", None, None));
        assert!(!no_proxy_bypasses("api.example.com", None, Some("  ")));
    }

    #[test]
    fn no_proxy_port_pinned_matches_implicit_default_port() {
        // proxy-from-env compares the EFFECTIVE port: NO_PROXY=host:443 must
        // bypass an implicit-port wss://host target.
        let env =
            |name: &str| (name == "HTTPS_PROXY").then(|| "http://proxy.test:3128".to_string());
        let lookup = |name: &str| {
            if name == "NO_PROXY" {
                Some("api.example.com:443".to_string())
            } else {
                env(name)
            }
        };
        assert_eq!(select_proxy("wss://api.example.com/v1", &lookup), None);
    }

    #[test]
    fn selection_respects_no_proxy() {
        let env = map(&[("HTTPS_PROXY", "proxy:8080"), ("NO_PROXY", "localhost, .internal.test")]);
        assert_eq!(select_proxy("wss://api.external.test", &env), Some("proxy:8080".into()));
        assert_eq!(select_proxy("wss://localhost/v1", &env), None);
        assert_eq!(select_proxy("wss://node.internal.test/v1", &env), None);
    }

    #[test]
    fn proxy_url_parsing() {
        assert_eq!(
            parse_proxy_url("http://proxy:8080").unwrap(),
            ProxyEndpoint {
                host: "proxy".into(),
                port: 8080,
                scheme: ProxyScheme::Http,
                source: "http://proxy:8080".into()
            }
        );
        assert_eq!(parse_proxy_url("proxy.corp").unwrap().port, 80);
        assert_eq!(parse_proxy_url("http://[::1]:8118").unwrap().host, "::1");
        assert_eq!(parse_proxy_url("http://[::1]").unwrap().port, 80);
        // https and socks5 proxy URLs are supported (TLS-to-proxy and the
        // tokio-socks handshake); only socks4/4a stay out of scope.
        let https = parse_proxy_url("https://proxy:8443").unwrap();
        assert_eq!((https.scheme, https.port), (ProxyScheme::Https, 8443));
        assert_eq!(parse_proxy_url("https://proxy").unwrap().port, 443);
        for url in ["socks5://proxy", "socks5h://proxy", "socks://proxy"] {
            let socks = parse_proxy_url(url).unwrap();
            assert_eq!((socks.scheme, socks.port), (ProxyScheme::Socks5, 1080), "{url}");
        }
        assert_eq!(parse_proxy_url("socks5://proxy:1081").unwrap().port, 1081);
        assert!(parse_proxy_url("socks4://proxy:1080").is_err(), "socks4 stays unsupported");
        assert!(parse_proxy_url("gopher://proxy").is_err());
        assert!(parse_proxy_url("http://proxy:notaport").is_err());
    }

    #[test]
    fn selection_keeps_no_proxy_semantics_across_schemes() {
        // NO_PROXY applies before the scheme is even considered: an
        // https:// proxy is not dialed for a bypassed target.
        let env = map(&[
            ("HTTPS_PROXY", "https://proxy:8443"),
            ("ALL_PROXY", "socks5://proxy:1080"),
            ("NO_PROXY", "api.internal.test"),
        ]);
        assert_eq!(select_proxy("wss://api.internal.test", &env), None);
        assert_eq!(
            select_proxy("wss://api.external.test", &env),
            Some("https://proxy:8443".into())
        );
    }

    #[test]
    fn truthiness_mirrors_v2() {
        assert!(env_truthy("1"));
        assert!(env_truthy("true"));
        assert!(env_truthy("TRUE"));
        assert!(!env_truthy("0"));
        assert!(!env_truthy("yes"));
        assert!(!env_truthy(""));
    }
}

#[cfg(all(test, feature = "ws-transport"))]
mod scheme_tunnel_tests {
    //! End-to-end tunnels over the supported schemes: a real SOCKS5
    //! stub (greeting → no-auth → CONNECT) and an https CONNECT stub
    //! (self-signed rustls TLS to the proxy, the CONNECT riding inside
    //! the session). Both then carry a real WebSocket exchange, like
    //! the http CONNECT stub in `websocket.rs`.

    use super::{
        connect_through, https_connect, parse_proxy_url, proxy_tls_connector, ProxyEndpoint,
        ProxyScheme, ProxyTunnel,
    };
    use futures::{SinkExt as _, StreamExt as _};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio_tungstenite::tungstenite::Message;

    async fn echo_over(tunnel: ProxyTunnel) {
        let (ws, _response) =
            tokio_tungstenite::client_async("ws://upstream.test/", tunnel).await.unwrap();
        let (mut sink, mut incoming) = ws.split();
        sink.send(Message::Text("ping".into())).await.unwrap();
        let reply = incoming.next().await.unwrap().unwrap();
        assert_eq!(reply.into_text().unwrap(), "echo:ping");
    }

    #[tokio::test]
    async fn websocket_handshakes_through_a_socks5_proxy() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            // greeting: \x05, one no-auth method byte
            let mut head = [0u8; 2];
            stream.read_exact(&mut head).await.unwrap();
            assert_eq!(head, [5, 1]);
            let mut method = [0u8; 1];
            stream.read_exact(&mut method).await.unwrap();
            assert_eq!(method, [0], "no-auth method offered");
            stream.write_all(&[5, 0]).await.unwrap();
            // request: \x05 CONNECT domain
            let mut head = [0u8; 4];
            stream.read_exact(&mut head).await.unwrap();
            assert_eq!(head[1], 1, "CONNECT command");
            let len = match head[3] {
                3 => {
                    let mut len = [0u8; 1];
                    stream.read_exact(&mut len).await.unwrap();
                    len[0] as usize
                }
                _ => 0,
            };
            let mut rest = vec![0u8; len + 2];
            stream.read_exact(&mut rest).await.unwrap();
            let host = String::from_utf8_lossy(&rest[..len]).to_string();
            assert_eq!(host, "upstream.test");
            // success: \x05 \x00 (IPv4 placeholder address)
            stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
            // the tunnel now carries the WebSocket
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut sink, mut incoming) = ws.split();
            let first = incoming.next().await.unwrap().unwrap();
            sink.send(Message::Text(format!("echo:{first}"))).await.unwrap();
        });

        let proxy = ProxyEndpoint {
            host: "127.0.0.1".into(),
            port: addr.port(),
            scheme: ProxyScheme::Socks5,
            source: format!("socks5://127.0.0.1:{}", addr.port()),
        };
        let tunnel = connect_through(&proxy, "upstream.test", 80).await.unwrap();
        echo_over(tunnel).await;
        server.await.unwrap();
    }

    #[tokio::test]
    async fn websocket_handshakes_through_an_https_connect_proxy() {
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
            .expect("self-signed stub certificate");
        let server_config = tokio_rustls::rustls::ServerConfig::builder_with_provider(
            std::sync::Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()),
        )
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            tokio_rustls::rustls::pki_types::PrivateKeyDer::Pkcs8(
                tokio_rustls::rustls::pki_types::PrivatePkcs8KeyDer::from(
                    certificate.signing_key.serialize_der(),
                ),
            ),
        )
        .expect("stub server certificate");
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_config));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let acceptor_clone = acceptor.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut stream = acceptor_clone.accept(stream).await.unwrap();
            // CONNECT rides inside the TLS session
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 512];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buffer);
            assert!(head.starts_with("CONNECT upstream.test:80 "), "got: {head}");
            stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await.unwrap();
            let ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (mut sink, mut incoming) = ws.split();
            let first = incoming.next().await.unwrap().unwrap();
            sink.send(Message::Text(format!("echo:{first}"))).await.unwrap();
        });

        let proxy = ProxyEndpoint {
            host: "127.0.0.1".into(),
            port: addr.port(),
            scheme: ProxyScheme::Https,
            source: format!("https://127.0.0.1:{}", addr.port()),
        };
        // The stub's certificate is the ONLY root the client trusts —
        // production uses webpki roots (proxy_tls_connector(None)).
        let mut roots = tokio_rustls::rustls::RootCertStore::empty();
        roots.add(certificate.cert.der().clone()).unwrap();
        let connector = proxy_tls_connector(Some(roots)).unwrap();
        let tls = https_connect(&proxy, "upstream.test", 80, Some(connector)).await.unwrap();
        echo_over(ProxyTunnel::Tls(Box::new(tls))).await;
        server.await.unwrap();
    }

    #[test]
    fn parse_and_scheme_routing() {
        // The scheme must survive parse → connect dispatch: a
        // scheme-less URL is http, and the socks aliases unify.
        assert_eq!(parse_proxy_url("proxy:3128").unwrap().scheme, ProxyScheme::Http);
        assert_eq!(parse_proxy_url("socks5h://p").unwrap().scheme, ProxyScheme::Socks5);
        assert_eq!(parse_proxy_url("https://p").unwrap().scheme, ProxyScheme::Https);
    }
}
