//! oc-client: generated Rust HTTP/SSE client SDK for the opencode `HttpApi`
//! (v2.0.24 wire contract).
//!
//! * [`types`] — wire-faithful serde types generated from the pinned
//!   `openapi.json` (exact field spellings, camelCase incl. `ID` suffixes).
//! * Namespace modules ([`agent`], [`session`], ...) — one method per
//!   operation, 1:1 with `oc_server::routes_gen::ROUTES`.
//! * [`sse::SseStream`] — v2-parity SSE decoding (16 MiB cap, CRLF
//!   normalization, heartbeat skip, JSON-in-string double decode).
//! * [`ClientError`] — declared-error envelope ("spread" fields, `_tag` name,
//!   `message ?? data.message` fallback) + `UnexpectedStatus` for the rest.
//!
//! `/openapi.json` self-host equivalence: the server lane must serve the byte-
//! identical document this client was generated from (sha256 in
//! `oc_server::routes_gen::OPENAPI_SHA256`).

pub mod error;
pub mod event_ext;
pub mod sse;
pub mod transport;

#[path = "types.gen.rs"]
pub mod types;

#[path = "methods.gen.rs"]
pub mod methods;

pub use error::ClientError;
pub use methods::*;
pub use sse::{SseFrame, SseStream, MAX_SSE_EVENT_BYTES};

use base64::Engine as _;

/// HTTP client handle. Auth is injected per request: Basic credentials via
/// the `Authorization` header (v2 merges caller headers; use
/// [`Client::with_default_header`] for those), `auth_token` query support for
/// browser/WS URLs via [`Client::url_with_auth_token`].
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    basic: Option<(String, Option<String>)>,
    auth_token: Option<String>,
    default_headers: reqwest::header::HeaderMap,
}

impl Client {
    /// Creates a client for `base_url` (e.g. `http://127.0.0.1:4096`).
    pub fn new(base_url: &str) -> Result<Self, ClientError> {
        let parsed = reqwest::Url::parse(base_url)
            .map_err(|_| ClientError::InvalidBaseUrl(base_url.to_owned()))?;
        let mut base = parsed.to_string();
        while base.ends_with('/') {
            base.pop();
        }
        Ok(Self {
            http: reqwest::Client::new(),
            base,
            basic: None,
            auth_token: None,
            default_headers: reqwest::header::HeaderMap::new(),
        })
    }

    /// Basic auth: `Authorization: Basic base64(user:pass)` on every request
    /// (v2 server: user `opencode`, password optional).
    #[must_use]
    pub fn with_basic_auth(mut self, user: &str, password: Option<String>) -> Self {
        self.basic = Some((user.to_owned(), password));
        self
    }

    /// Stores the base64 `user:pass` token used for browser/WS credential
    /// URLs (`?auth_token=...`, server-deep-map §1).
    #[must_use]
    pub fn with_auth_token(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    /// Computes the `auth_token` query value for the configured credentials.
    #[must_use]
    pub fn auth_token_value(&self) -> Option<String> {
        let (user, pass) = self.basic.as_ref()?;
        let joined = format!("{user}:{}", pass.clone().unwrap_or_default());
        Some(base64::engine::general_purpose::STANDARD.encode(joined))
    }

    /// Absolute URL with `auth_token` appended (WS upgrade endpoints).
    #[must_use]
    pub fn url_with_auth_token(&self, path: &str) -> Option<String> {
        let token = self.auth_token.clone().or_else(|| self.auth_token_value())?;
        let separator = if path.contains('?') { '&' } else { '?' };
        Some(format!("{}{path}{separator}auth_token={token}", self.base))
    }

    /// A default header merged into every request (v2 `ClientOptions.headers`).
    pub fn with_default_header(mut self, name: &str, value: &str) -> Result<Self, ClientError> {
        let name = reqwest::header::HeaderName::try_from(name)
            .map_err(|_| ClientError::InvalidBaseUrl(format!("invalid header name {name:?}")))?;
        let value = reqwest::header::HeaderValue::try_from(value)
            .map_err(|_| ClientError::InvalidBaseUrl(format!("invalid header value {value:?}")))?;
        self.default_headers.insert(name, value);
        Ok(self)
    }

    /// Configured base URL (no trailing slash).
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn basic_auth_header(&self) -> Option<String> {
        let (user, pass) = self.basic.as_ref()?;
        let joined = format!("{user}:{}", pass.clone().unwrap_or_default());
        Some(format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(joined)))
    }
}
