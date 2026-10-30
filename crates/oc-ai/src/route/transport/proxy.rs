//! WebSocket proxy support (gap G-8): the v2 stack routes WebSocket
//! connections through `http(s)-proxy-agent`, so `HTTP_PROXY` /
//! `HTTPS_PROXY` / `ALL_PROXY` (and `NO_PROXY`) apply to WS transports
//! the same way reqwest applies them to HTTP. The rust port's WS
//! connector dials directly, so this module selects a proxy for a
//! target URL from the environment and tunnels the WebSocket through
//! it with a hand-rolled HTTP CONNECT (proxy selection + `NO_PROXY` are
//! pure functions so tests can drive them without process env races).
//!
//! Supported proxy URLs: `http://host[:port]` and scheme-less
//! `host[:port]` (default port 80). `socks5://` proxies need a SOCKS
//! handshake this port does not carry (v2 would use a different agent
//! package), and `https://` proxies need a TLS leg to the proxy itself;
//! both are rejected with a clear error rather than silently dialing
//! direct.

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
    if no_proxy_bypasses(&host, effective_port, env("NO_PROXY").or_else(|| env("no_proxy")).as_deref()) {
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
    if host.is_empty() { None } else { Some(host.to_ascii_lowercase()) }
}

/// The URL's explicit port (defaults live with the caller — the proxy
/// side only needs the port `NO_PROXY` entries may pin).
fn target_port(target: &str) -> Option<u16> {
    let rest = target.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    let port = authority.rsplit_once(':').and_then(|(host, port)| {
        // only a real trailing port (an IPv6 literal keeps its colons)
        (!host.is_empty() && !host.contains(':'))
            .then(|| port.parse::<u16>().ok())
            .flatten()
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
        let port_match = entry_port
            .parse::<u16>()
            .is_ok_and(|entry_port| Some(entry_port) == port);
        if port_match {
            return true;
        }
    }
    false
}

/// A parsed proxy endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
    /// The raw URL the endpoint came from (error reporting).
    pub source: String,
}

/// Parses a proxy URL into a dialable endpoint. `http://` and
/// scheme-less `host[:port]` parse (default port 80); `https://` and
/// `socks*://` are unsupported for the WS CONNECT tunnel and error out.
pub fn parse_proxy_url(url: &str) -> Result<ProxyEndpoint, String> {
    let (scheme, rest) = match url.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("http".to_string(), url),
    };
    match scheme.as_str() {
        "http" => {}
        "https" => {
            return Err(format!(
                "https:// proxy URLs are not supported for WebSocket tunnels ({url})"
            ));
        }
        other if other.starts_with("socks") => {
            return Err(format!(
                "{other}:// proxy URLs are not supported for WebSocket tunnels ({url})"
            ));
        }
        other => {
            return Err(format!("unsupported proxy URL scheme {other:?} ({url})"));
        }
    }
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
    Ok(ProxyEndpoint { host, port: port.unwrap_or(80), source: url.to_string() })
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

/// Dials the proxy and issues `CONNECT host:port`, returning the
/// upgraded TCP stream ready for the WebSocket handshake.
pub async fn connect_through(
    proxy: &ProxyEndpoint,
    target_host: &str,
    target_port: u16,
) -> Result<tokio::net::TcpStream, AiError> {
    // NOTE: any bytes read past the CONNECT response header are discarded —
    // harmless for ws/wss because the CLIENT speaks first after the upgrade.
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let fail = |message: String| {
        transport_error(
            message,
            TransportOperation::Request,
            Some(TransportPhase::Connect),
            Some(TransportDelivery::NotSent),
            Some("proxy-connect".into()),
            Some(proxy.source.clone()),
        )
    };
    let host_display =
        if target_host.contains(':') { format!("[{target_host}]") } else { target_host.to_string() };
    let future = async {
        let mut stream = tokio::net::TcpStream::connect((proxy.host.as_str(), proxy.port))
            .await
            .map_err(|error| format!("failed to dial proxy {}: {error}", proxy.source))?;
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
            if buffer.windows(4).any(|window| window == b"\r\n\r\n") || buffer.len() > MAX_CONNECT_RESPONSE {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buffer);
        let status = head.lines().next().unwrap_or_default();
        let code = status.split_whitespace().nth(1).unwrap_or_default();
        if code.len() == 3 && code.starts_with('2') {
            Ok(stream)
        } else {
            Err(format!("proxy refused CONNECT: {status}"))
        }
    };
    match tokio::time::timeout(CONNECT_TIMEOUT, future).await {
        Ok(result) => result.map_err(fail),
        Err(_elapsed) => Err(fail("timed out establishing the proxy tunnel".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map<'a>(env: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            env.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn selection_honors_scheme_precedence() {
        let env = map(&[("HTTPS_PROXY", "https-proxy:1"), ("HTTP_PROXY", "http-proxy:2")]);
        assert_eq!(
            select_proxy("wss://api.example.com/v1", &env),
            Some("https-proxy:1".into())
        );
        assert_eq!(
            select_proxy("ws://api.example.com/v1", &env),
            Some("http-proxy:2".into())
        );
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
        let env = |name: &str| {
            (name == "HTTPS_PROXY").then(|| "http://proxy.test:3128".to_string())
        };
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
        let env = map(&[
            ("HTTPS_PROXY", "proxy:8080"),
            ("NO_PROXY", "localhost, .internal.test"),
        ]);
        assert_eq!(select_proxy("wss://api.external.test", &env), Some("proxy:8080".into()));
        assert_eq!(select_proxy("wss://localhost/v1", &env), None);
        assert_eq!(select_proxy("wss://node.internal.test/v1", &env), None);
    }

    #[test]
    fn proxy_url_parsing() {
        assert_eq!(
            parse_proxy_url("http://proxy:8080").unwrap(),
            ProxyEndpoint { host: "proxy".into(), port: 8080, source: "http://proxy:8080".into() }
        );
        assert_eq!(parse_proxy_url("proxy.corp").unwrap().port, 80);
        assert_eq!(parse_proxy_url("http://[::1]:8118").unwrap().host, "::1");
        assert_eq!(parse_proxy_url("http://[::1]").unwrap().port, 80);
        assert!(parse_proxy_url("socks5://proxy:1080").is_err());
        assert!(parse_proxy_url("https://proxy:443").is_err());
        assert!(parse_proxy_url("http://proxy:notaport").is_err());
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
