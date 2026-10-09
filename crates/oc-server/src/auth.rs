//! HTTP authentication (`server/src/auth.ts` + `middleware/authorization.ts`).
//!
//! - Basic credentials: `Authorization: Basic <base64>` (case-insensitive
//!   scheme), decoded, split at the FIRST colon; invalid base64 or a missing
//!   colon yields the empty credential.
//! - Session tokens (issued by pairing links): `${expires}.${hmac}` where the
//!   signing key is derived from the server password
//!   (`HMAC-SHA256(password, "opencode-session-v1")`), so rotating the
//!   password revokes every session. Verification is constant-time.
//! - Cookies: `opencode_session_${port}` (port from the Host header), only
//!   accepted when the Origin header is absent or same-host.
//! - `auth_token` query parameter: base64 `user:pass` credential source used
//!   by browser WebSocket clients.
//!
//! Passwordless mode (`password: None` or empty) passes every request
//! through.

use axum::http::{header, HeaderMap, HeaderValue, Uri};
use axum::response::Response;
use base64::engine::general_purpose::{
    STANDARD as B64, STANDARD_NO_PAD as B64NOPAD, URL_SAFE_NO_PAD as B64URL,
};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::state::{AppState, AuthConfig};

/// The fixed Basic username (`auth.ts`).
pub const AUTH_USERNAME: &str = "opencode";

/// The Basic challenge value (`authorization.ts`).
pub const WWW_AUTHENTICATE: &str = "Basic realm=\"Secure Area\"";

/// 30 days, in seconds (`SESSION_TTL_SECONDS`).
pub const SESSION_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

/// The session-token KDF info string.
const SESSION_KDF_INFO: &str = "opencode-session-v1";

type HmacSha256 = Hmac<Sha256>;

/// Decoded Basic credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl Credentials {
    /// The empty credential (failed decode).
    #[must_use]
    pub fn empty() -> Self {
        Self { username: String::new(), password: String::new() }
    }
}

/// Whether auth is enforced (`required`): a set, non-empty password.
#[must_use]
pub fn required(config: &AuthConfig) -> bool {
    config.password.as_ref().is_some_and(|password| !password.is_empty())
}

/// Decodes base64 into credentials, splitting at the first colon
/// (`decodeCredential`). Invalid base64 or no colon → empty credential.
/// The padding is stripped and the decoder accepts unpadded input —
/// Node's `Buffer.from(..., "base64")` (v2) is padding-tolerant, so a
/// password whose encoding carries `=` must still decode (a padded
/// encoding stripped to a non-multiple-of-4 length fails the strict
/// engine).
#[must_use]
pub fn decode_credential(input: &str) -> Credentials {
    let trimmed = input.trim_end_matches('=');
    let decoded = B64NOPAD.decode(trimmed.as_bytes()).or_else(|_| B64.decode(trimmed.as_bytes()));
    let Ok(decoded) = decoded else {
        return Credentials::empty();
    };
    let Ok(text) = String::from_utf8(decoded) else {
        return Credentials::empty();
    };
    match text.find(':') {
        Some(index) => Credentials {
            username: text[..index].to_owned(),
            password: text[index + 1..].to_owned(),
        },
        None => Credentials::empty(),
    }
}

/// Extracts credentials from the `Authorization` header (`^Basic\s+(.+)$`,
/// case-insensitive). `None` when the header carries no Basic scheme.
#[must_use]
pub fn basic_from_headers(headers: &HeaderMap) -> Option<Credentials> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, payload) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Basic") {
        return None;
    }
    Some(decode_credential(payload))
}

/// `authorized` (auth.ts): username must match exactly; password is either
/// the plain server password or a valid session token. Passwordless configs
/// never authorize through this path (the layer passes through instead).
#[must_use]
pub fn authorized(credentials: &Credentials, config: &AuthConfig) -> bool {
    let Some(password) = config.password.as_ref() else {
        return false;
    };
    if credentials.username != AUTH_USERNAME {
        return false;
    }
    credentials.password == *password
        || verify_session(&credentials.password, config, unix_now_ms())
}

/// Issues a session token `${expires}.${hmac}` (`issueSession`).
/// `None` in passwordless mode.
#[must_use]
pub fn issue_session(config: &AuthConfig, now_ms: i64) -> Option<String> {
    let password = config.password.as_ref()?;
    let expires =
        now_ms.div_euclid(1000) + i64::try_from(SESSION_TTL_SECONDS).expect("constant fits");
    let signature = sign(password, &expires.to_string());
    Some(format!("{expires}.{signature}"))
}

/// Verifies a session token (`verifySession`): exactly two parts, a safe
/// integer expiry in the future, and a constant-time HMAC match.
#[must_use]
pub fn verify_session(token: &str, config: &AuthConfig, now_ms: i64) -> bool {
    let Some(password) = config.password.as_ref() else {
        return false;
    };
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return false;
    }
    let Ok(expires) = parts[0].parse::<f64>() else {
        return false;
    };
    // Number.isSafeInteger
    if !expires.is_finite() || expires.fract() != 0.0 || expires.abs() > 9_007_199_254_740_991.0 {
        return false;
    }
    // JS Number semantics: the comparison happens in f64 on purpose.
    #[allow(clippy::cast_precision_loss)]
    let now = now_ms as f64;
    if expires * 1000.0 <= now {
        return false;
    }
    let expected = sign(password, parts[0]);
    constant_time_eq(expected.as_bytes(), parts[1].as_bytes())
}

/// `sign`: key = HMAC-SHA256(password, "opencode-session-v1"),
/// hmac = HMAC-SHA256(key, payload) as base64url.
fn sign(password: &str, payload: &str) -> String {
    let mut kdf =
        HmacSha256::new_from_slice(password.as_bytes()).expect("hmac accepts any key length");
    kdf.update(SESSION_KDF_INFO.as_bytes());
    let key = kdf.finalize().into_bytes();

    let mut signer = HmacSha256::new_from_slice(&key).expect("hmac accepts any key length");
    signer.update(payload.as_bytes());
    B64URL.encode(signer.finalize().into_bytes())
}

/// Length check then constant-time comparison (`timingSafeEqual`).
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && bool::from(subtle::ConstantTimeEq::ct_eq(left, right))
}

/// Cookie name for a Host header value: `opencode_session_${port}`, or the
/// bare `opencode_session` when the host carries no (valid) port.
#[must_use]
pub fn session_cookie_name(host: Option<&str>) -> String {
    match host.and_then(host_port) {
        Some(port) => format!("opencode_session_{port}"),
        None => "opencode_session".to_owned(),
    }
}

/// Extracts the port from a Host header (IPv6 bracket form aware). Invalid
/// hosts (e.g. bare IPv6 without brackets) yield `None`, like a failed
/// `URL.parse`.
fn host_port(host: &str) -> Option<u16> {
    let authority = if let Some(rest) = host.strip_prefix('[') {
        let end = rest.find(']')?;
        let tail = &rest[end + 1..];
        let port = tail.strip_prefix(':')?;
        if port.is_empty() {
            return None;
        }
        return port.parse::<u16>().ok();
    } else {
        host
    };
    let (candidate, port) = authority.rsplit_once(':')?;
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // A second colon means a bare (unbracketed) IPv6 host — not a port.
    if candidate.contains(':') {
        return None;
    }
    port.parse::<u16>().ok()
}

/// The `host` component of an Origin/URL value (`URL.parse(x)?.host`):
/// `scheme://[user@]host[:port]` → `host:port` (brackets preserved for IPv6).
#[must_use]
pub fn origin_host(origin: &str) -> Option<String> {
    let rest = origin.split_once("://")?.1;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let mut authority = &rest[..authority_end];
    if let Some(at) = authority.rfind('@') {
        authority = &authority[at + 1..];
    }
    Some(authority.to_owned())
}

/// Whether an Origin header is same-host with the Host header.
#[must_use]
pub fn same_origin_host(origin: &str, host: &str) -> bool {
    origin_host(origin).is_some_and(|candidate| candidate == host)
}

/// Cookie value for the session cookie name, if present.
fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().ok()?;
        for pair in value.split(';') {
            let pair = pair.trim();
            if let Some((key, cookie)) = pair.split_once('=') {
                if key == name {
                    return Some(cookie.to_owned());
                }
            }
        }
    }
    None
}

/// `authorizedRequest`: the request's credential (query `auth_token` first,
/// then the Authorization header) authorizes, or the session cookie does
/// (Origin absent / same-host only).
#[must_use]
pub fn authorized_request(headers: &HeaderMap, query: Option<&str>, config: &AuthConfig) -> bool {
    if let Some(credentials) = credential_from_request(headers, query) {
        if authorized(&credentials, config) {
            return true;
        }
    }
    authorized_session_cookie(headers, config)
}

/// Credential source order: `auth_token` query, then Basic header.
fn credential_from_request(headers: &HeaderMap, query: Option<&str>) -> Option<Credentials> {
    if let Some(token) = query_param(query, "auth_token") {
        if !token.is_empty() {
            return Some(decode_credential(&token));
        }
    }
    basic_from_headers(headers)
}

/// Cookie-based authorization with the same-origin restriction.
fn authorized_session_cookie(headers: &HeaderMap, config: &AuthConfig) -> bool {
    let host = headers.get(header::HOST).and_then(|value| value.to_str().ok());
    let Some(token) = cookie_value(headers, &session_cookie_name(host)) else {
        return false;
    };
    let origin = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok());
    if let Some(origin) = origin {
        let host = host.unwrap_or_default();
        if !same_origin_host(origin, host) {
            return false;
        }
    }
    verify_session(&token, config, unix_now_ms())
}

/// The raw-dispatch 401: envelope + conditional `www-authenticate`
/// (only when `sec-fetch-mode` is absent or `navigate`).
#[must_use]
pub fn unauthorized_response(headers: &HeaderMap) -> Response {
    let mut response = crate::errors::unauthorized("Authentication required").into_response();
    if challenge_request(headers) {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static(WWW_AUTHENTICATE));
    }
    response
}

/// `challengeRequest`: browsers get the native Basic prompt only for
/// non-browser clients and page navigations.
#[must_use]
pub fn challenge_request(headers: &HeaderMap) -> bool {
    match headers.get("sec-fetch-mode").and_then(|value| value.to_str().ok()) {
        None | Some("navigate") => true,
        Some(_) => false,
    }
}

/// Reads a query parameter from a raw query string (percent-decoded,
/// `+` treated as space).
#[must_use]
pub fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode_plus(key) == name {
            return Some(percent_decode_plus(value));
        }
    }
    None
}

/// Percent-decoding with `+` → space (query semantics). Malformed sequences
/// pass through undecoded.
#[must_use]
pub fn percent_decode_plus(input: &str) -> String {
    decode_inner(input, true)
}

/// `decodeURIComponent` semantics: malformed sequences pass through raw.
#[must_use]
pub fn percent_decode(input: &str) -> String {
    decode_inner(input, false)
}

fn decode_inner(input: &str, plus_as_space: bool) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = bytes.get(index + 1..index + 3);
                match hex.and_then(|slice| std::str::from_utf8(slice).ok()) {
                    Some(slice) if slice.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
                        let high = hex_val(slice.as_bytes()[0]);
                        let low = hex_val(slice.as_bytes()[1]);
                        out.push(high * 16 + low);
                        index += 3;
                    }
                    _ => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

/// The full auth gate for one request, shared by the dispatch gate and the
/// per-route Authorization equivalent. Passwordless → pass-through.
/// Exempt URL checks are the caller's job (they differ per gate).
#[must_use]
pub fn request_authorized(uri: &Uri, headers: &HeaderMap, state: &AppState) -> bool {
    if !required(&state.auth) {
        return true;
    }
    authorized_request(headers, uri.query(), &state.auth)
}

/// Current unix time in milliseconds.
pub(crate) fn unix_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_millis()).unwrap_or(i64::MAX),
        Err(err) => -i64::try_from(err.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(password: Option<&str>) -> AuthConfig {
        AuthConfig { password: password.map(str::to_owned) }
    }

    fn basic(user: &str, pass: &str) -> String {
        format!("Basic {}", B64.encode(format!("{user}:{pass}")))
    }

    #[test]
    fn server_auth_accepts_only_fixed_username() {
        let cfg = config(Some("secret"));
        assert!(authorized(
            &Credentials { username: "opencode".into(), password: "secret".into() },
            &cfg
        ));
        assert!(!authorized(
            &Credentials { username: "custom".into(), password: "secret".into() },
            &cfg
        ));
        assert!(!authorized(
            &Credentials { username: "opencode".into(), password: "wrong".into() },
            &cfg
        ));
    }

    #[test]
    fn server_auth_session_tokens_expire_and_rotate() {
        let cfg = config(Some("secret"));
        let now = 1_700_000_000_000;
        let token = issue_session(&cfg, now).expect("token");
        assert!(verify_session(&token, &cfg, now));
        // Expired at exactly the TTL boundary.
        assert!(!verify_session(
            &token,
            &cfg,
            now + i64::try_from(SESSION_TTL_SECONDS * 1000).unwrap()
        ));
        // Tampered shapes.
        assert!(!verify_session(&format!("{token}x"), &cfg, now));
        let parts: Vec<&str> = token.split('.').collect();
        let bumped = format!("{}.{}", parts[0].parse::<i64>().unwrap() + 1, parts[1]);
        assert!(!verify_session(&bumped, &cfg, now));
        // Rotation revokes.
        let rotated = config(Some("rotated"));
        assert!(!verify_session(&token, &rotated, now));
        // Passwordless issues/verifies nothing.
        assert!(issue_session(&config(None), now).is_none());
        assert!(!verify_session(&token, &config(None), now));

        // The `authorized` path uses the live clock: a fresh token works as
        // a password, an old one does not.
        let live = issue_session(&cfg, unix_now_ms()).expect("live token");
        assert!(authorized(&Credentials { username: "opencode".into(), password: live }, &cfg));
        assert!(!authorized(&Credentials { username: "opencode".into(), password: token }, &cfg));
    }

    #[test]
    fn server_auth_basic_parsing() {
        assert_eq!(
            basic_from_headers(&header_map(&[("authorization", &basic("opencode", "secret"))])),
            Some(Credentials { username: "opencode".into(), password: "secret".into() })
        );
        // Case-insensitive scheme.
        let lower = format!("bAsIc {}", B64.encode("opencode:secret"));
        assert!(basic_from_headers(&header_map(&[("authorization", &lower)])).is_some());
        // Password containing colons: split at FIRST colon only.
        let colon = format!("Basic {}", B64.encode("opencode:a:b"));
        assert_eq!(
            basic_from_headers(&header_map(&[("authorization", &colon)])),
            Some(Credentials { username: "opencode".into(), password: "a:b".into() })
        );
        // Invalid base64 → empty credential.
        assert_eq!(decode_credential("!!!"), Credentials::empty());
        // No colon → empty credential.
        let nocolon = B64.encode("justusername");
        assert_eq!(decode_credential(&nocolon), Credentials::empty());
        // No header / other scheme → None.
        assert_eq!(basic_from_headers(&HeaderMap::new()), None);
        assert_eq!(basic_from_headers(&header_map(&[("authorization", "Bearer xyz")])), None);
    }

    #[test]
    fn server_auth_cookie_name_carries_port() {
        assert_eq!(session_cookie_name(Some("127.0.0.1:8080")), "opencode_session_8080");
        assert_eq!(session_cookie_name(Some("localhost")), "opencode_session");
        assert_eq!(session_cookie_name(Some("[::1]:9000")), "opencode_session_9000");
        assert_eq!(session_cookie_name(Some("::1")), "opencode_session");
        assert_eq!(session_cookie_name(None), "opencode_session");
        assert_eq!(session_cookie_name(Some("bad host")), "opencode_session");
    }

    #[test]
    fn server_auth_query_and_header_credential_paths() {
        let cfg = config(Some("secret"));
        let token = B64.encode("opencode:secret");
        // auth_token query.
        assert!(authorized_request(&HeaderMap::new(), Some(&format!("auth_token={token}")), &cfg));
        // Header.
        let headers = header_map(&[("authorization", &basic("opencode", "secret"))]);
        assert!(authorized_request(&headers, None, &cfg));
        // Cookie session, no origin.
        let session = issue_session(&cfg, unix_now_ms()).unwrap();
        let cookie_headers = header_map(&[
            ("host", "127.0.0.1:8080"),
            ("cookie", &format!("opencode_session_8080={session}")),
        ]);
        assert!(authorized_request(&cookie_headers, None, &cfg));
        // Same-host origin accepted.
        let same = header_map(&[
            ("host", "127.0.0.1:8080"),
            ("origin", "http://127.0.0.1:8080"),
            ("cookie", &format!("opencode_session_8080={session}")),
        ]);
        assert!(authorized_request(&same, None, &cfg));
        // Cross-origin cookie rejected.
        let cross = header_map(&[
            ("host", "127.0.0.1:8080"),
            ("origin", "http://127.0.0.1:1"),
            ("cookie", &format!("opencode_session_8080={session}")),
        ]);
        assert!(!authorized_request(&cross, None, &cfg));
        // Garbage cookie rejected.
        let garbage = header_map(&[
            ("host", "127.0.0.1:8080"),
            ("cookie", &format!("opencode_session_8080={session}x")),
        ]);
        assert!(!authorized_request(&garbage, None, &cfg));
    }

    #[test]
    fn server_auth_unauthorized_challenge_conditioning() {
        let plain = HeaderMap::new();
        assert!(challenge_request(&plain));
        let response = unauthorized_response(&plain);
        assert_eq!(response.status().as_u16(), 401);
        assert_eq!(response.headers()["www-authenticate"], WWW_AUTHENTICATE);

        let cors = header_map(&[("sec-fetch-mode", "cors")]);
        assert!(!challenge_request(&cors));
        let response = unauthorized_response(&cors);
        assert!(response.headers().get("www-authenticate").is_none());

        let navigate = header_map(&[("sec-fetch-mode", "navigate")]);
        assert!(challenge_request(&navigate));
    }

    #[test]
    fn server_auth_percent_decoding() {
        assert_eq!(percent_decode_plus("a%20b+c"), "a b c");
        assert_eq!(percent_decode("a%20b+c"), "a b+c");
        // Malformed escapes pass through raw.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
    }

    fn header_map(entries: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in entries {
            map.insert(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }
}
