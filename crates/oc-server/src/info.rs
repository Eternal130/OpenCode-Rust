//! `GET /api/info` (`handlers/server.ts` + `process.ts infoResponse`).

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

/// `urls()`: the bound address formatted as `http://host:port` (IPv6 gets
/// brackets). The unspecified-address expansion (`connectionURLs`) collapses
/// to the bound form here since this shell binds concrete addresses.
fn connection_urls(state: &AppState) -> Vec<String> {
    match *state.local_addr.read() {
        Some(addr) => vec![format!("http://{}", addr)],
        None => Vec::new(),
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
