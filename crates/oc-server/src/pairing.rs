//! Pairing codes (`server/src/pairing.ts` + `handlers/server.ts`).
//!
//! `POST /api/pair` issues a 128-bit base64url code (TTL 5 min, cache cap
//! 1000 evict-oldest). `GET /auth/connect/{code}` consumes it once:
//! browsers get a 302 to `/` plus the session cookie; API clients get the
//! session token as JSON. Expired or reused links fail per the v2 texts.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine as _;
use rand::RngCore;
use serde_json::json;

use crate::auth;
use crate::state::{AppState, PairingEntry};

/// Pairing-code TTL.
pub const TTL_SECONDS: u64 = 300;

/// Cache capacity (`Cache.make({ capacity: 1_000, ... })`).
pub const CAPACITY: usize = 1_000;

/// Issues a pairing code: 16 random bytes, base64url, expiring in 5 minutes.
pub fn issue(state: &AppState, now_ms: i64) -> (String, u64) {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let code = B64URL.encode(bytes);
    evict_if_full(&state.pairing, CAPACITY);
    state.pairing.insert(
        code.clone(),
        PairingEntry { expires_at: u64::try_from(now_ms).unwrap_or(0) + TTL_SECONDS * 1000 },
    );
    (code, TTL_SECONDS)
}

/// Consumes a code (single-use, expiry-checked). `true` when the code was
/// live and is now taken.
pub fn consume(state: &AppState, code: &str, now_ms: i64) -> bool {
    match state.pairing.remove(code) {
        Some((_, entry)) => u64::try_from(now_ms).unwrap_or(0) < entry.expires_at,
        None => false,
    }
}

/// Evicts the oldest (nearest-expiry) entry when the cache reached capacity.
fn evict_if_full(map: &dashmap::DashMap<String, PairingEntry>, capacity: usize) {
    if map.len() < capacity {
        return;
    }
    let oldest =
        map.iter().min_by_key(|entry| entry.value().expires_at).map(|entry| entry.key().clone());
    if let Some(oldest) = oldest {
        map.remove(&oldest);
    }
}

/// The browser-facing failure text (`handlers/server.ts`).
pub const EXPIRED_BROWSER_MESSAGE: &str =
    "This pairing link expired or was already used. Run `opencode pair` to get a new one.";

/// The API-client failure message.
pub const EXPIRED_API_MESSAGE: &str = "Pairing link expired or already used";

/// `POST /api/pair` handler (already auth-gated upstream).
pub fn pair_response(state: &AppState) -> Response {
    let (code, expires_in) = issue(state, auth::unix_now_ms());
    let body = json!({ "code": code, "expires_in": expires_in });
    json_response(StatusCode::OK, &body)
}

/// `GET /auth/connect/{code}` handler. `code` is the raw path segment
/// (already percent-decoded by the dispatch).
pub fn connect_response(state: &AppState, code: &str, headers: &axum::http::HeaderMap) -> Response {
    let browser = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));
    let consumed = consume(state, code, auth::unix_now_ms());
    let token = consumed.then(|| auth::issue_session(&state.auth, auth::unix_now_ms())).flatten();

    let Some(token) = token else {
        if browser {
            let mut response = Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(axum::body::Body::from(EXPIRED_BROWSER_MESSAGE))
                .unwrap_or_default();
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(auth::WWW_AUTHENTICATE));
            return response;
        }
        return crate::errors::unauthorized(EXPIRED_API_MESSAGE).into_response();
    };

    if !browser {
        return json_response(StatusCode::OK, &json!({ "token": token }));
    }

    let host = headers.get(header::HOST).and_then(|value| value.to_str().ok());
    let cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        auth::session_cookie_name(host),
        token,
        auth::SESSION_TTL_SECONDS
    );
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, "/")
        .header(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).expect("token is base64url + digits"),
        )
        .body(axum::body::Body::empty())
        .unwrap_or_default()
}

/// Small JSON response helper.
pub(crate) fn json_response(status: StatusCode, body: &serde_json::Value) -> Response {
    let mut response = (status, axum::body::Body::from(body.to_string())).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AuthConfig;
    use oc_core::engine::{Engine, EngineOptions};
    use oc_util::determinism::{SystemClock, UuidV4IdGen};

    async fn state(password: Option<&str>) -> std::sync::Arc<AppState> {
        let tmp = tempfile::tempdir().expect("tempdir");
        let engine =
            Engine::boot(EngineOptions::new(tmp.path().to_owned(), tmp.path().to_owned())).await;
        std::sync::Arc::new(AppState::new(
            engine,
            AuthConfig { password: password.map(str::to_owned) },
            "test".to_owned(),
            std::sync::Arc::new(UuidV4IdGen),
            std::sync::Arc::new(SystemClock),
        ))
    }

    fn headers(entries: &[(&'static str, &str)]) -> axum::http::HeaderMap {
        let mut map = axum::http::HeaderMap::new();
        for (name, value) in entries {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[tokio::test]
    async fn server_pairing_issue_and_single_use() {
        let state = state(Some("secret")).await;
        let (code, expires_in) = issue(&state, 1_000);
        assert_eq!(expires_in, 300);
        assert_eq!(code.len(), 22, "16 bytes base64url unpadded");
        assert!(consume(&state, &code, 1_001), "first use consumes");
        assert!(!consume(&state, &code, 1_002), "second use fails");
        assert!(!consume(&state, "unknown", 1_000), "unknown code fails");
    }

    #[tokio::test]
    async fn server_pairing_expires() {
        let state = state(Some("secret")).await;
        let (code, _) = issue(&state, 0);
        assert!(
            !consume(&state, &code, i64::try_from(TTL_SECONDS * 1000).unwrap() + 1),
            "expired code fails"
        );
    }

    #[tokio::test]
    async fn server_pairing_capacity_evicts_oldest() {
        let state = state(Some("secret")).await;
        let (first, _) = issue(&state, 0);
        for index in 1..CAPACITY {
            issue(&state, i64::try_from(index).unwrap());
        }
        assert_eq!(state.pairing.len(), CAPACITY);
        issue(&state, i64::try_from(CAPACITY).unwrap() + 1);
        assert_eq!(state.pairing.len(), CAPACITY);
        assert!(!state.pairing.contains_key(&first), "oldest evicted");
    }

    #[tokio::test]
    async fn server_pairing_browser_round_trip() {
        let state = state(Some("secret")).await;
        let (code, _) = issue(&state, auth::unix_now_ms());

        let browser = headers(&[("accept", "text/html"), ("host", "127.0.0.1:8080")]);
        let response = connect_response(&state, &code, &browser);
        assert_eq!(response.status().as_u16(), 302);
        assert_eq!(response.headers()[header::LOCATION], "/");
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap().to_owned();
        assert!(cookie.starts_with("opencode_session_8080="), "cookie was {cookie}");
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Max-Age=2592000"));

        // Reuse: browser text failure.
        let reused = connect_response(&state, &code, &browser);
        assert_eq!(reused.status().as_u16(), 401);
    }

    #[tokio::test]
    async fn server_pairing_api_client_round_trip() {
        let state = state(Some("secret")).await;
        let (code, _) = issue(&state, auth::unix_now_ms());
        let api = axum::http::HeaderMap::new();
        let response = connect_response(&state, &code, &api);
        assert_eq!(response.status().as_u16(), 200);
        let body = axum::body::to_bytes(response.into_body(), 1 << 16).await.expect("body");
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let token = parsed["token"].as_str().expect("token").to_owned();

        // The token authorizes as a session credential.
        let credentials =
            auth::Credentials { username: "opencode".into(), password: token.clone() };
        assert!(auth::authorized(&credentials, &state.auth));

        // Reuse: envelope failure.
        let reused = connect_response(&state, &code, &api);
        assert_eq!(reused.status().as_u16(), 401);
    }

    #[tokio::test]
    async fn server_pairing_unknown_code_fails() {
        let state = state(Some("secret")).await;
        let response = connect_response(&state, "does-not-exist", &axum::http::HeaderMap::new());
        assert_eq!(response.status().as_u16(), 401);
    }
}
