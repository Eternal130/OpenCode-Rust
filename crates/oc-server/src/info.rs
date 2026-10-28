//! `GET /api/info` (`handlers/server.ts` + `process.ts infoResponse`).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::json;

use crate::lifecycle::{BUILD_HEADER, BUILD_STAMP};
use crate::pairing::json_response;
use crate::state::{AppState, Readiness};
/// Renders the info payload with readiness-dependent status and headers
/// (`infoResponse`): 200 ready / 500 failed / 503 starting|stopping, with
/// `retry-after: 1` for the transient states.
///
/// B4-H3: every info response carries the build identity in the
/// `x-opencode-build` HEADER (never the body — the body shape is v2's
/// API surface: `reference_api_info.json` pins it and the served
/// `/openapi.json` is sha-pinned in `goldens/p10`; headers are
/// invisible to both). The connect probe compares it to replace a
/// stale same-version daemon after an upgrade.
#[must_use]
pub fn info_response(state: &AppState) -> Response {
    let readiness = state.readiness();
    let status = match readiness {
        Readiness::Ready => StatusCode::OK,
        Readiness::Failed => StatusCode::INTERNAL_SERVER_ERROR,
        Readiness::Starting | Readiness::Stopping => StatusCode::SERVICE_UNAVAILABLE,
    };
    let urls = connection_urls(state);
    let body = json!({
        "version": state.version,
        "pid": std::process::id(),
        "urls": urls,
        "paths": { "tmp": state.tmp_dir.display().to_string() },
        // v2 handlers/server.ts:23-32 — the SDK's capability gate for the
        // persistent-PTY surface (`PersistentPty.available` =
        // `platform !== "win32"`, binary.node.ts:2; workerd serves false).
        "capabilities": { "persistentPty": cfg!(not(target_os = "windows")) },
    });
    let mut response = json_response(status, &body);
    if let Ok(value) = HeaderValue::from_str(BUILD_STAMP) {
        response.headers_mut().insert(BUILD_HEADER, value);
    }
    if matches!(readiness, Readiness::Starting | Readiness::Stopping) {
        response.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

/// `urls()` (`connectionURLs`, server-info.ts:18-36): the bound address
/// formatted as `http://host:port` (IPv6 gets brackets); an unspecified
/// (wildcard) listen address expands to every non-internal interface
/// address of the same family, de-duplicated in interface order.
fn connection_urls(state: &AppState) -> Vec<String> {
    match *state.local_addr.read() {
        Some(addr) => expand_wildcard(addr),
        None => Vec::new(),
    }
}

/// `connectionURLs`: `0.0.0.0` expands over IPv4 interface addresses,
/// `::` over IPv6 (bracketed); a concrete host renders as-is.
fn expand_wildcard(addr: SocketAddr) -> Vec<String> {
    if !addr.ip().is_unspecified() {
        return vec![format!("http://{addr}")];
    }
    let ipv4 = addr.is_ipv4();
    let mut seen = std::collections::HashSet::new();
    interface_addresses(ipv4)
        .into_iter()
        .filter(|ip| seen.insert(*ip))
        .map(|ip| match ip {
            IpAddr::V4(v4) => format!("http://{v4}:{}", addr.port()),
            IpAddr::V6(v6) => format!("http://[{v6}]:{}", addr.port()),
        })
        .collect()
}

/// Non-loopback interface addresses of one family (`os.networkInterfaces()`
/// filtered to `!internal && family`), via `getifaddrs(3)` (no new deps —
/// libc is already a direct oc-server dependency). Interface order is
/// preserved; failures yield an empty list (the wildcard then expands to
/// no urls, exactly like an interface-less v2 host).
#[allow(unsafe_code, reason = "getifaddrs(3) is the crate-free interface enumeration (RV-P6)")]
// getifaddrs yields sockaddr variants at their natural alignment; the
// variant casts are the canonical libc idiom.
#[allow(clippy::cast_ptr_alignment)]
fn interface_addresses(ipv4: bool) -> Vec<IpAddr> {
    #[cfg(unix)]
    unsafe {
        let mut addrs: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(std::ptr::addr_of_mut!(addrs)) != 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut cursor = addrs;
        while !cursor.is_null() {
            let ifa = &*cursor;
            cursor = ifa.ifa_next;
            if ifa.ifa_addr.is_null() {
                continue;
            }
            let sockaddr = ifa.ifa_addr;
            let ip = match i32::from((*sockaddr).sa_family) {
                libc::AF_INET if ipv4 => IpAddr::V4(Ipv4Addr::from(
                    (*(sockaddr as *const libc::sockaddr_in)).sin_addr.s_addr.to_ne_bytes(),
                )),
                libc::AF_INET6 if !ipv4 => {
                    let sa = &*(sockaddr as *const libc::sockaddr_in6);
                    // Scoped link-local addresses (`fe80::%x`) cannot form a
                    // reachable URL hostname; skip them.
                    if sa.sin6_scope_id != 0 {
                        continue;
                    }
                    IpAddr::V6(Ipv6Addr::from(sa.sin6_addr.s6_addr))
                }
                _ => continue,
            };
            if !ip.is_loopback() {
                out.push(ip);
            }
        }
        libc::freeifaddrs(addrs);
        out
    }
    #[cfg(not(unix))]
    {
        let _ = ipv4;
        Vec::new()
    }
}

/// The not-ready 503 envelope (`unavailable`): failed carries the remediation
/// text; starting/stopping just the code, plus `retry-after: 1`.
#[must_use]
pub fn unavailable_response(readiness: Readiness) -> Response {
    use crate::errors::{service_failed, service_starting, service_stopping};
    let error = match readiness {
        Readiness::Failed => service_failed(),
        Readiness::Stopping => service_stopping(),
        Readiness::Starting => service_starting(),
        Readiness::Ready => return Response::default(),
    };
    let mut response = error.into_response();
    if !matches!(readiness, Readiness::Failed) {
        response.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket(host: &str) -> SocketAddr {
        format!("{host}:0").parse().expect("socket addr")
    }

    #[test]
    fn info_connection_urls_concrete_address_renders_as_is() {
        assert_eq!(expand_wildcard(socket("127.0.0.1")), vec!["http://127.0.0.1:0"]);
        assert_eq!(expand_wildcard(socket("[::1]")), vec!["http://[::1]:0"]);
    }

    #[test]
    fn info_connection_urls_wildcard_expands_to_interface_addresses() {
        let urls = expand_wildcard(socket("0.0.0.0"));
        let expected: Vec<String> =
            interface_addresses(true).into_iter().map(|ip| format!("http://{ip}:0")).collect();
        assert_eq!(urls, expected);
        // LAN discovery: a wildcard bind yields one URL per non-loopback
        // interface address (no raw `0.0.0.0` left behind).
        assert!(!urls.iter().any(|url| url.contains("0.0.0.0")));
        for ip in interface_addresses(true) {
            assert!(ip.is_ipv4() && !ip.is_loopback());
        }
    }

    #[test]
    fn info_connection_urls_ipv6_wildcard_brackets_expanded_addresses() {
        for url in expand_wildcard(socket("[::]")) {
            assert!(url.starts_with("http://["), "bracketed IPv6 url: {url}");
        }
    }
}
