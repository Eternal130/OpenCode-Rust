//! Dispatch-gate pieces (`process.ts` + `cors.ts` + `location.ts`).
//!
//! - [`is_allowed_cors_origin`]: the v2 allowlist predicate (localhost,
//!   127.0.0.1, `oc://renderer`, tauri hosts, `*.opencode.ai`, plus
//!   configured `cors` origins).
//! - [`cors_middleware`]: the CORS layer (preflight 204, echo-on-allow,
//!   max-age 86400).
//! - Auth exemptions: pairing connect URLs (always) and PTY/persistent-pty
//!   connect URLs carrying a `ticket` query (only when ready).
//! - [`request_location`]: `location[directory]` query →
//!   `x-opencode-directory` header (URI-decoded, malformed → raw) → cwd.

use std::sync::Arc;

use axum::extract::Request;
use axum::http::{header, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// CORS allowlist (cors.ts)
// ---------------------------------------------------------------------------

/// `^https:\/\/([a-z0-9-]+\.)*opencode\.ai$` — hand-rolled (no regex dep).
fn is_opencode_ai_origin(input: &str) -> bool {
    let Some(rest) = input.strip_prefix("https://") else {
        return false;
    };
    let Some(labels) = rest.strip_suffix("opencode.ai") else {
        return false;
    };
    if labels.is_empty() {
        return true; // https://opencode.ai
    }
    // Every preceding label must be `[a-z0-9-]+` followed by a dot.
    let Some(labels) = labels.strip_suffix('.') else {
        return false;
    };
    !labels.is_empty()
        && labels.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

/// `isAllowedCorsOrigin`: the default same-machine allowlist plus configured
/// origins. Missing input is allowed (no Origin header → no CORS decision).
#[must_use]
pub fn is_allowed_cors_origin(input: Option<&str>, cors: &[String]) -> bool {
    let Some(input) = input else {
        return true;
    };
    if input.starts_with("http://localhost:") {
        return true;
    }
    if input.starts_with("http://127.0.0.1:") {
        return true;
    }
    if input == "oc://renderer" {
        return true;
    }
    if input == "tauri://localhost"
        || input == "http://tauri.localhost"
        || input == "https://tauri.localhost"
    {
        return true;
    }
    if is_opencode_ai_origin(input) {
        return true;
    }
    cors.iter().any(|allowed| allowed == input)
}

/// `isAllowedRequestOrigin`: same-host access plus the CORS allowlist
/// (used by the PTY WS origin checks).
#[must_use]
pub fn is_allowed_request_origin(input: Option<&str>, host: Option<&str>, cors: &[String]) -> bool {
    let Some(input) = input else {
        return true;
    };
    if let Some(host) = host {
        if crate::auth::same_origin_host(input, host) {
            return true;
        }
    }
    is_allowed_cors_origin(Some(input), cors)
}

// ---------------------------------------------------------------------------
// CORS layer (HttpMiddleware.cors parity)
// ---------------------------------------------------------------------------

/// `access-control-max-age` value.
const CORS_MAX_AGE: u64 = 86_400;

/// CORS methods advertised on preflight.
const CORS_METHODS: &str = "GET,HEAD,POST,PUT,DELETE,PATCH";

/// The CORS layer over the whole app (outermost, like `process.ts:71`).
///
/// Preflights (OPTIONS + `access-control-request-method`) short-circuit with
/// 204 and echo the request origin/methods/headers when allowed, always
/// advertising `max-age: 86400`. Actual requests pass through and gain
/// `access-control-allow-origin` only when the origin is allowed. Disallowed
/// origins are never blocked — they simply get no CORS headers, exactly like
/// effect's `allowedOrigins` predicate.
pub async fn cors_middleware(
    axum::extract::State(cors): axum::extract::State<Arc<Vec<String>>>,
    request: Request,
    next: Next,
) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let preflight = request.method() == axum::http::Method::OPTIONS
        && request.headers().contains_key("access-control-request-method");

    if preflight {
        let allowed =
            origin.as_deref().is_some_and(|origin| is_allowed_cors_origin(Some(origin), &cors));
        let mut response = Response::builder().status(axum::http::StatusCode::NO_CONTENT);
        let headers = response.headers_mut().expect("builder with status is valid");
        headers.insert(header::VARY, HeaderValue::from_static("Origin"));
        if allowed {
            if let Some(origin) = &origin {
                if let Ok(value) = HeaderValue::from_str(origin) {
                    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
                }
            }
            if let Some(method) = request
                .headers()
                .get("access-control-request-method")
                .and_then(|value| value.to_str().ok())
            {
                if let Ok(value) = HeaderValue::from_str(&method.to_ascii_uppercase()) {
                    headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, value);
                }
            } else {
                headers.insert(
                    header::ACCESS_CONTROL_ALLOW_METHODS,
                    HeaderValue::from_static(CORS_METHODS),
                );
            }
            if let Some(requested) = request
                .headers()
                .get("access-control-request-headers")
                .and_then(|value| value.to_str().ok())
            {
                let echoed: String =
                    requested.split(',').map(str::trim).collect::<Vec<_>>().join(", ");
                if !echoed.is_empty() {
                    if let Ok(value) = HeaderValue::from_str(&echoed) {
                        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, value);
                    }
                }
            }
            headers.insert(
                header::HeaderName::from_static("access-control-max-age"),
                HeaderValue::from(CORS_MAX_AGE),
            );
        }
        return response.body(axum::body::Body::empty()).unwrap_or_default();
    }

    let mut response = next.run(request).await;
    if origin.as_deref().is_some_and(|origin| is_allowed_cors_origin(Some(origin), &cors)) {
        if let Some(origin) = &origin {
            if let Ok(value) = HeaderValue::from_str(origin) {
                response.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
            }
        }
    }
    response
}

// ---------------------------------------------------------------------------
// Auth exemptions (process.ts:189-194 + protocol groups)
// ---------------------------------------------------------------------------

/// `^\/auth\/connect\/[^/]+$` — ALWAYS exempt from auth.
#[must_use]
pub fn is_pairing_connect_url(path: &str) -> bool {
    match path.strip_prefix("/auth/connect/") {
        Some(code) => !code.is_empty() && !code.contains('/'),
        None => false,
    }
}

/// `^\/api\/pty\/[^/]+\/connect$` + non-empty `ticket` query — exempt when
/// ready (the handler validates and consumes the ticket).
#[must_use]
pub fn has_pty_connect_ticket_url(path: &str, query: Option<&str>) -> bool {
    connect_path_id(path, "/api/pty/").is_some()
        && crate::auth::query_param(query, "ticket").is_some_and(|ticket| !ticket.is_empty())
}

/// `^\/api\/experimental\/persistent-pty\/[^/]+\/connect$` + `ticket` query.
#[must_use]
pub fn has_persistent_pty_connect_ticket_url(path: &str, query: Option<&str>) -> bool {
    connect_path_id(path, "/api/experimental/persistent-pty/").is_some()
        && crate::auth::query_param(query, "ticket").is_some_and(|ticket| !ticket.is_empty())
}

/// `{prefix}{id}/connect` with a non-empty single-segment `id`.
fn connect_path_id<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(prefix)?;
    let id = rest.strip_suffix("/connect")?;
    if id.is_empty() || id.contains('/') {
        return None;
    }
    Some(id)
}

/// The raw dispatch gate: `!isPairingConnectURL && (!ready || !ticketURLs)
/// && !authorized → 401`. Passwordless passes through.
#[must_use]
pub fn dispatch_gate_passes(
    uri: &axum::http::Uri,
    headers: &axum::http::HeaderMap,
    state: &AppState,
    ready: bool,
) -> bool {
    let path = uri.path();
    if is_pairing_connect_url(path) {
        return true;
    }
    if ready
        && (has_pty_connect_ticket_url(path, uri.query())
            || has_persistent_pty_connect_ticket_url(path, uri.query()))
    {
        return true;
    }
    crate::auth::request_authorized(uri, headers, state)
}

// ---------------------------------------------------------------------------
// Location derivation (location.ts requestRef)
// ---------------------------------------------------------------------------

/// Derives the request location ref: `location[directory]` query →
/// `x-opencode-directory` header (URI-decoded, malformed → raw) → the
/// server's launch directory.
///
/// The fallback mirrors v2's `requestRef` (`server/src/location.ts:
/// 57-65`): `process.cwd()` OF THE SERVER PROCESS — v2 spawns the
/// server in the project directory, so a location-less request means
/// "the server's home location". The Rust production entries coincide
/// with the process cwd, but an EMBEDDED server
/// (`boot_and_serve` with an explicit `config_location`) lives in a
/// host process whose cwd is unrelated to the server, so callers pass
/// [`crate::state::AppState::boot_location`] as `fallback`; `None`
/// keeps the raw process cwd (unit tests, non-server derivations).
#[must_use]
pub fn request_location(
    uri: &axum::http::Uri,
    headers: &axum::http::HeaderMap,
    fallback: Option<&std::path::Path>,
) -> oc_schema::location::Ref {
    let directory = crate::auth::query_param(uri.query(), "location[directory]")
        .filter(|value| !value.is_empty())
        .or_else(|| {
            headers
                .get("x-opencode-directory")
                .and_then(|value| value.to_str().ok())
                .map(crate::auth::percent_decode)
        })
        .unwrap_or_else(|| {
            fallback.map_or_else(
                || {
                    std::env::current_dir()
                        .map_or_else(|_| ".".to_owned(), |path| path.display().to_string())
                },
                |path| path.display().to_string(),
            )
        });
    oc_schema::location::Ref {
        directory: oc_schema::common::AbsolutePath(directory),
        workspace_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cors(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn server_cors_custom_origins_extend_defaults() {
        let options = cors(&["http://192.168.1.10:3001", "https://example.com"]);
        assert!(!is_allowed_cors_origin(Some("http://192.168.1.10:3001"), &[]));
        assert!(is_allowed_cors_origin(Some("http://192.168.1.10:3001"), &options));
        assert!(is_allowed_cors_origin(Some("https://example.com"), &options));
        assert!(is_allowed_cors_origin(Some("http://localhost:3001"), &options));
        assert!(is_allowed_cors_origin(Some("https://app.opencode.ai"), &options));
        assert!(is_allowed_cors_origin(Some("https://opencode.ai"), &options));
        assert!(is_allowed_cors_origin(None, &options));
        assert!(!is_allowed_cors_origin(Some("http://192.168.1.10:3002"), &options));
        assert!(!is_allowed_cors_origin(Some("https://example.com.evil.test"), &options));
        assert!(!is_allowed_cors_origin(Some("http://example.com"), &options));
        assert!(!is_allowed_cors_origin(Some("null"), &options));
        assert!(!is_allowed_cors_origin(Some("https://evilopencode.ai"), &options));
        assert!(is_allowed_cors_origin(Some("oc://renderer"), &options));
        assert!(is_allowed_cors_origin(Some("tauri://localhost"), &options));
    }

    #[test]
    fn server_cors_request_origin_keeps_same_host() {
        let options = cors(&["http://192.168.1.10:3001"]);
        assert!(!is_allowed_request_origin(
            Some("http://192.168.1.10:3001"),
            Some("192.168.1.10:1029"),
            &[]
        ));
        assert!(is_allowed_request_origin(
            Some("http://192.168.1.10:3001"),
            Some("192.168.1.10:1029"),
            &options
        ));
        assert!(is_allowed_request_origin(
            Some("http://192.168.1.10:1029"),
            Some("192.168.1.10:1029"),
            &options
        ));
        assert!(!is_allowed_request_origin(
            Some("http://192.168.1.10:3002"),
            Some("192.168.1.10:1029"),
            &options
        ));
        assert!(is_allowed_request_origin(None, None, &[]));
    }

    #[test]
    fn server_gate_pairing_connect_regex() {
        assert!(is_pairing_connect_url("/auth/connect/abc123"));
        assert!(is_pairing_connect_url("/auth/connect/a%2Fb"));
        assert!(!is_pairing_connect_url("/auth/connect/"));
        assert!(!is_pairing_connect_url("/auth/connect/a/b"));
        assert!(!is_pairing_connect_url("/auth/connect"));
        assert!(!is_pairing_connect_url("/api/event"));
    }

    #[test]
    fn server_gate_pty_ticket_urls() {
        assert!(has_pty_connect_ticket_url("/api/pty/pty_1/connect", Some("ticket=abc")));
        assert!(has_pty_connect_ticket_url(
            "/api/pty/pty_1/connect",
            Some("location[directory]=%2Ftmp&ticket=abc")
        ));
        assert!(!has_pty_connect_ticket_url("/api/pty/pty_1/connect", Some("ticket=")));
        assert!(!has_pty_connect_ticket_url("/api/pty/pty_1/connect", None));
        assert!(!has_pty_connect_ticket_url("/api/pty/connect", Some("ticket=abc")));
        assert!(!has_pty_connect_ticket_url("/api/pty/pty_1/connect/extra", Some("ticket=abc")));

        assert!(has_persistent_pty_connect_ticket_url(
            "/api/experimental/persistent-pty/term_1/connect",
            Some("ticket=abc")
        ));
        assert!(!has_persistent_pty_connect_ticket_url(
            "/api/experimental/persistent-pty/connect",
            Some("ticket=abc")
        ));
    }

    #[test]
    fn server_location_query_header_cwd_precedence() {
        let uri = axum::http::Uri::builder()
            .path_and_query("/api/session?location%5Bdirectory%5D=%2Ffrom%2Fquery")
            .build()
            .unwrap();
        let location = request_location(&uri, &axum::http::HeaderMap::new(), None);
        assert_eq!(location.directory.0, "/from/query");

        let uri = axum::http::Uri::from_static("/api/session");
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::HeaderName::from_static("x-opencode-directory"),
            HeaderValue::from_str("/from%20header").unwrap(),
        );
        let location = request_location(&uri, &headers, None);
        assert_eq!(location.directory.0, "/from header");

        // Malformed escape falls back to the raw value.
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::HeaderName::from_static("x-opencode-directory"),
            HeaderValue::from_str("/100%").unwrap(),
        );
        let location = request_location(&uri, &headers, None);
        assert_eq!(location.directory.0, "/100%");

        // The server-launch fallback (v2's server-process cwd) wins
        // over the host process's cwd when provided.
        let location = request_location(
            &uri,
            &axum::http::HeaderMap::new(),
            Some(std::path::Path::new("/boot/project")),
        );
        assert_eq!(location.directory.0, "/boot/project");

        // cwd fallback is absolute.
        let location = request_location(&uri, &axum::http::HeaderMap::new(), None);
        assert!(location.directory.0.starts_with('/'));
    }
}
