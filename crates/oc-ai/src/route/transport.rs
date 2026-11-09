//! Process-agnostic transport seam (oracle gate 2, finding 7 — BINDING).
//!
//! Protocols and routes only ever see [`Transport`] and its own
//! request/response/error types: raw byte streams in, unified event streams
//! out. A reqwest-backed implementation ([`ReqwestTransport`]) ships behind
//! the default `reqwest-transport` feature; a subprocess-backed provider
//! (the `aisdk:` shim, phase 9) can implement the same trait without any
//! route-layer rework. NO reqwest types leak into protocol/route signatures.
//!
//! The WebSocket transport path (channel executors + the connector seam)
//! lives in [`websocket`] — feature `ws-transport` adds the production
//! tokio-tungstenite connector.

pub mod proxy;
pub mod websocket;

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use futures::StreamExt as _;

use crate::schema::{AiError, AiErrorReason, HttpContext, ReasonFields};

pub use crate::schema::{
    TransportDelivery, TransportOperation, TransportPhase, TransportRecovery, TransportType,
};

/// A raw byte stream from a response body. Items are `Result` so read
/// failures mid-stream (connection reset while streaming) surface with
/// transport diagnostics; `TransportError` is this crate's own type.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>> + Send + 'static>>;

/// Transport-level failure (before or during the response body).
#[derive(Debug, Clone, PartialEq)]
pub struct TransportError {
    pub message: String,
    pub transport: TransportType,
    pub operation: TransportOperation,
    pub code: Option<String>,
    pub url: Option<String>,
    pub phase: Option<TransportPhase>,
    pub delivery: Option<TransportDelivery>,
    pub recovery: Option<TransportRecovery>,
    pub http: Option<HttpContext>,
}

impl TransportError {
    /// A failure while sending the request or reading the body of a live
    /// connection (the common case for dropped streams).
    pub fn connection(message: impl Into<String>, operation: TransportOperation) -> Self {
        Self {
            message: message.into(),
            transport: TransportType::Http,
            operation,
            code: None,
            url: None,
            phase: None,
            delivery: None,
            recovery: None,
            http: None,
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    #[must_use]
    pub fn with_http(mut self, http: HttpContext) -> Self {
        self.http = Some(http);
        self
    }

    /// Map into the AI error taxonomy.
    #[must_use]
    pub fn into_ai_error(self) -> AiError {
        let fields =
            ReasonFields { message: self.message, http: self.http, ..ReasonFields::default() };
        AiError::new(AiErrorReason::Transport {
            fields,
            transport: self.transport,
            operation: self.operation,
            code: self.code,
            url: self.url,
            phase: self.phase,
            delivery: self.delivery,
            recovery: self.recovery,
        })
    }
}

/// A prepared, encoded request handed to a transport.
#[derive(Debug, Clone)]
pub struct TransportRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

impl TransportRequest {
    pub fn post(url: impl Into<String>) -> Self {
        Self { method: "POST".into(), url: url.into(), headers: Vec::new(), body: Bytes::new() }
    }

    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_body(mut self, body: Bytes) -> Self {
        self.body = body;
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A transport response: status, headers, and the raw body byte stream.
pub struct TransportResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: ByteStream,
}

impl std::fmt::Debug for TransportResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

impl TransportResponse {
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(std::string::String::as_str)
    }

    /// HTTP context for diagnostics.
    #[must_use]
    pub fn http_context(&self, url: &str) -> HttpContext {
        HttpContext { url: url.to_string(), status: self.status, headers: self.headers.clone() }
    }
}

/// The transport seam: executes one prepared request (BINDING — see module
/// docs). Implementations must be usable from async contexts and shareable
/// across threads (`Arc<dyn Transport>`).
#[async_trait]
pub trait Transport: Send + Sync {
    async fn execute(&self, request: TransportRequest)
        -> Result<TransportResponse, TransportError>;
}

/// Read a whole response body into a string (bounded by `limit` bytes) — used
/// by the executor for error classification on non-2xx statuses.
pub async fn read_body_limited(
    mut body: ByteStream,
    limit: usize,
) -> Result<String, TransportError> {
    use futures::StreamExt;
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        if buffer.len() + chunk.len() > limit {
            buffer.extend_from_slice(&chunk[..limit.saturating_sub(buffer.len())]);
            break;
        }
        buffer.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// Wrap a transport read failure into the AI error taxonomy with HTTP context
/// attached (port of `responseStream`'s error mapping).
#[must_use]
pub fn transport_read_error(error: TransportError) -> AiError {
    error.into_ai_error()
}

/// A transport that always fails — useful for wiring tests and as an
/// explicit "no network" placeholder.
#[derive(Debug, Default)]
pub struct FailingTransport {
    pub message: &'static str,
}

#[async_trait]
impl Transport for FailingTransport {
    async fn execute(
        &self,
        _request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        Err(TransportError::connection(self.message, TransportOperation::Request))
    }
}

/// The v2 HTTP timeout budget (port of `packages/ai/src/route/transport/
/// http.ts` timeout wiring + `packages/core/src/provider.ts` `timeouts()`).
///
/// Every provider request runs under three budgets: `headerTimeout` (wait
/// for response headers, default [`DEFAULT_HTTP_TIMEOUT_MS`] = 300s),
/// `chunkTimeout` (idle wait between streamed body chunks, same default),
/// and `timeout` (whole-request budget, **no default** — unbounded when
/// unset). A budget of `false` disables that budget. Expired budgets
/// surface as typed transport errors (`code: "Timeout"`, operation
/// `request` for headers / `read` for the body) instead of hanging.
/// [`Route::stream`](crate::route::client::Route::stream) applies this
/// around the real transport call — the production path, not test-only.
pub mod timeouts {
    use std::time::Duration;

    use super::{
        ByteStream, SharedTransport, TransportError, TransportOperation, TransportRequest,
        TransportResponse,
    };
    use crate::schema::{HttpContext, HttpOptions, HttpTimeout, DEFAULT_HTTP_TIMEOUT_MS};

    /// A resolved three-part timeout budget for one request.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct TimeoutBudget {
        /// Whole-request budget (send → response complete). `None` is
        /// unbounded — v2 gives `timeout` no default.
        pub total: Option<Duration>,
        /// Wait for response headers. `None` disables the budget.
        pub header: Option<Duration>,
        /// Idle wait between body chunks. `None` disables the budget.
        pub chunk: Option<Duration>,
    }

    impl TimeoutBudget {
        /// Resolve the merged per-request `http` options with v2 defaults:
        /// unset `headerTimeout`/`chunkTimeout` fall back to
        /// [`DEFAULT_HTTP_TIMEOUT_MS`] (300s), `false` disables them, and
        /// `timeout` stays unbounded when unset (invalid values are
        /// dropped, mirroring v2's decode-or-drop `timeout()` helper).
        #[must_use]
        pub fn from_http_options(http: Option<&HttpOptions>) -> Self {
            let resolve = |value: Option<HttpTimeout>, default: Option<Duration>| match value {
                Some(HttpTimeout::Ms(ms)) if ms.is_finite() && ms > 0.0 => {
                    Some(duration_from_ms(ms))
                }
                Some(_) => None,
                None => default,
            };
            let default = Some(duration_from_ms(DEFAULT_HTTP_TIMEOUT_MS));
            let (timeout, header, chunk) = match http {
                Some(options) => (options.timeout, options.header_timeout, options.chunk_timeout),
                None => (None, None, None),
            };
            Self {
                total: resolve(timeout, None),
                header: resolve(header, default),
                chunk: resolve(chunk, default),
            }
        }

        /// The header wait budget: `min(headerTimeout, total)` (v2
        /// `Duration.min(timeoutDuration(request.http?.headerTimeout), total)`).
        /// `None` when both are unbounded.
        #[must_use]
        pub fn header_wait(&self) -> Option<Duration> {
            match (self.header, self.total) {
                (Some(header), Some(total)) => Some(header.min(total)),
                (Some(header), None) => Some(header),
                (None, total) => total,
            }
        }
    }

    fn duration_from_ms(ms: f64) -> Duration {
        // Inputs are validated finite/positive at the call sites; the clamp
        // keeps the conversion panic-free for NaN (`max` resolves to 0.0).
        Duration::from_secs_f64((ms / 1_000.0).max(0.0))
    }

    /// Header budget expired — the response headers never arrived in time.
    pub fn header_timeout_error(url: &str) -> TransportError {
        TransportError::connection(
            "Timed out waiting for response headers",
            TransportOperation::Request,
        )
        .with_url(url)
        .with_code("Timeout")
    }

    /// Chunk idle budget expired — no body data arrived within the budget.
    pub fn chunk_timeout_error(url: &str, http: HttpContext) -> TransportError {
        read_timeout("Timed out waiting for response data", url, http)
    }

    /// Whole-request budget expired while the response was still streaming.
    pub fn total_timeout_error(url: &str, http: HttpContext) -> TransportError {
        read_timeout("Timed out waiting for the response to complete", url, http)
    }

    fn read_timeout(message: &'static str, url: &str, http: HttpContext) -> TransportError {
        TransportError::connection(message, TransportOperation::Read)
            .with_url(url)
            .with_code("Timeout")
            .with_http(http)
    }

    /// Execute one request under the header budget (`min(header, total)`):
    /// a response whose headers do not arrive in time fails with the typed
    /// header timeout error instead of hanging the caller forever.
    pub async fn execute_with_timeouts(
        transport: SharedTransport,
        request: TransportRequest,
        budget: TimeoutBudget,
    ) -> Result<TransportResponse, TransportError> {
        match budget.header_wait() {
            None => transport.execute(request).await,
            Some(wait) => {
                let url = request.url.clone();
                match tokio::time::timeout(wait, transport.execute(request)).await {
                    Ok(result) => result,
                    Err(_elapsed) => Err(header_timeout_error(&url)),
                }
            }
        }
    }

    /// Which budget expired while waiting for the next body chunk.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Expiry {
        Chunk,
        Total,
    }

    /// Wrap a response body with the chunk idle budget and the remaining
    /// whole-request budget (v2 `Stream.timeoutOrElse(chunkTimeout)` +
    /// `interruptWhen(remaining)`): a stalled or overrunning stream fails
    /// with a typed read timeout instead of hanging. Underlying transport
    /// read failures pass through unchanged.
    pub fn body_with_timeouts(
        body: ByteStream,
        budget: TimeoutBudget,
        started: tokio::time::Instant,
        url: &str,
        http: HttpContext,
    ) -> ByteStream {
        let total_deadline = budget.total.map(|total| started + total);
        let chunk_wait = budget.chunk;
        let url = url.to_string();
        Box::pin(async_stream::stream! {
            use futures::StreamExt;
            let mut body = body;
            loop {
                let outcome = if chunk_wait.is_none() && total_deadline.is_none() {
                    Ok(body.next().await)
                } else {
                    let inner = async {
                        match chunk_wait {
                            Some(wait) => tokio::time::timeout(wait, body.next())
                                .await
                                .map_err(|_| Expiry::Chunk),
                            None => Ok(body.next().await),
                        }
                    };
                    match total_deadline {
                        Some(deadline) => match tokio::time::timeout_at(deadline, inner).await {
                            Ok(outcome) => outcome,
                            Err(_elapsed) => Err(Expiry::Total),
                        },
                        None => inner.await,
                    }
                };
                match outcome {
                    Ok(Some(Ok(chunk))) => yield Ok(chunk),
                    Ok(Some(Err(error))) => {
                        yield Err(error);
                        return;
                    }
                    Ok(None) => return,
                    Err(Expiry::Chunk) => {
                        yield Err(chunk_timeout_error(&url, http.clone()));
                        return;
                    }
                    Err(Expiry::Total) => {
                        yield Err(total_timeout_error(&url, http.clone()));
                        return;
                    }
                }
            }
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn budget_defaults_match_v2() {
            assert_eq!(DEFAULT_HTTP_TIMEOUT_MS, 300_000.0, "v2 DEFAULT_TIMEOUT_MS");
            let budget = TimeoutBudget::from_http_options(None);
            assert_eq!(budget.header, Some(Duration::from_secs(300)));
            assert_eq!(budget.chunk, Some(Duration::from_secs(300)));
            assert_eq!(budget.total, None, "timeout has no default");

            let empty = HttpOptions::default();
            let budget = TimeoutBudget::from_http_options(Some(&empty));
            assert_eq!(budget.header, Some(Duration::from_secs(300)));
            assert_eq!(budget.chunk, Some(Duration::from_secs(300)));
            assert_eq!(budget.total, None);
        }

        #[test]
        fn budget_respects_explicit_and_disabled_values() {
            let options = HttpOptions {
                timeout: Some(HttpTimeout::Ms(1_000.0)),
                header_timeout: Some(HttpTimeout::Disabled),
                chunk_timeout: Some(HttpTimeout::Ms(250.0)),
                ..Default::default()
            };
            let budget = TimeoutBudget::from_http_options(Some(&options));
            assert_eq!(budget.total, Some(Duration::from_secs(1)));
            assert_eq!(budget.header, None, "false disables the header budget");
            assert_eq!(budget.chunk, Some(Duration::from_millis(250)));
        }

        #[test]
        fn header_wait_is_min_of_header_and_total() {
            let long = HttpOptions {
                timeout: Some(HttpTimeout::Ms(1_000.0)),
                header_timeout: Some(HttpTimeout::Ms(60_000.0)),
                ..Default::default()
            };
            let budget = TimeoutBudget::from_http_options(Some(&long));
            assert_eq!(budget.header_wait(), Some(Duration::from_secs(1)));

            let disabled = HttpOptions {
                timeout: Some(HttpTimeout::Ms(1_000.0)),
                header_timeout: Some(HttpTimeout::Disabled),
                ..Default::default()
            };
            let budget = TimeoutBudget::from_http_options(Some(&disabled));
            assert_eq!(
                budget.header_wait(),
                Some(Duration::from_secs(1)),
                "total still bounds a disabled header timeout"
            );

            let unbounded = TimeoutBudget::from_http_options(None);
            assert_eq!(unbounded.header_wait(), Some(Duration::from_secs(300)));
            assert_eq!(TimeoutBudget::default().header_wait(), None);
        }

        #[test]
        fn timeout_errors_carry_the_v2_shape() {
            let error = header_timeout_error("http://provider.test/v1");
            assert_eq!(error.operation, TransportOperation::Request);
            assert_eq!(error.code.as_deref(), Some("Timeout"));
            assert_eq!(error.message, "Timed out waiting for response headers");
            assert_eq!(error.url.as_deref(), Some("http://provider.test/v1"));

            let http = HttpContext {
                url: "http://provider.test/v1".into(),
                status: 200,
                headers: Default::default(),
            };
            let error = chunk_timeout_error("http://provider.test/v1", http.clone());
            assert_eq!(error.operation, TransportOperation::Read);
            assert_eq!(error.message, "Timed out waiting for response data");
            assert_eq!(error.http, Some(http));

            let error = total_timeout_error(
                "http://provider.test/v1",
                HttpContext {
                    url: "http://provider.test/v1".into(),
                    status: 200,
                    headers: Default::default(),
                },
            );
            assert_eq!(error.operation, TransportOperation::Read);
            assert_eq!(error.message, "Timed out waiting for the response to complete");
        }
    }
}

/// Reqwest-backed transport behind the default `reqwest-transport` feature.
#[cfg(feature = "reqwest-transport")]
pub mod reqwest_transport {
    use super::{
        async_trait, BTreeMap, ByteStream, Transport, TransportError, TransportOperation,
        TransportRequest, TransportResponse,
    };
    use futures::TryStreamExt as _;

    /// Transport backed by a shared reqwest client. Timeouts are the
    /// caller's concern (route-level headers/chunk budgets wrap the frame
    /// stream); reqwest here only performs delivery.
    #[derive(Clone)]
    pub struct ReqwestTransport {
        client: reqwest::Client,
    }

    impl Default for ReqwestTransport {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ReqwestTransport {
        #[must_use]
        pub fn new() -> Self {
            Self { client: oc_http::client() }
        }

        #[must_use]
        pub fn with_client(client: reqwest::Client) -> Self {
            Self { client }
        }
    }

    #[async_trait]
    impl Transport for ReqwestTransport {
        async fn execute(
            &self,
            request: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            let method =
                reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|error| {
                    TransportError::connection(
                        format!("invalid method: {error}"),
                        TransportOperation::Request,
                    )
                    .with_url(&request.url)
                })?;
            let mut builder = self.client.request(method, &request.url);
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if !request.body.is_empty() {
                builder = builder.body(request.body.clone());
            }
            let response = builder.send().await.map_err(|error| {
                let code = error.is_timeout().then_some("Timeout").map(str::to_string);
                TransportError::connection(
                    describe_reqwest_error(&error),
                    TransportOperation::Request,
                )
                .with_url(&request.url)
                .with_code(code.unwrap_or_else(|| "RequestFailed".into()))
            })?;
            let status = response.status().as_u16();
            let headers: BTreeMap<String, String> = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_ascii_lowercase(),
                        value.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect();
            let stream = response.bytes_stream();
            let body: ByteStream = Box::pin(stream.map_err(|error| {
                let code = error.is_timeout().then_some("Timeout").map(str::to_string);
                TransportError::connection(describe_reqwest_error(&error), TransportOperation::Read)
                    .with_code(code.unwrap_or_else(|| "ReadFailed".into()))
            }));
            Ok(TransportResponse { status, headers, body })
        }
    }

    fn describe_reqwest_error(error: &reqwest::Error) -> String {
        if error.is_timeout() {
            format!("HTTP transport timed out: {error}")
        } else if error.is_connect() {
            format!("HTTP connect failed: {error}")
        } else {
            format!("HTTP transport failed: {error}")
        }
    }
}

/// Shared handle type used by routes.
pub type SharedTransport = Arc<dyn Transport>;

/// Wrap any transport into a shared handle.
pub fn shared(transport: impl Transport + 'static) -> SharedTransport {
    Arc::new(transport)
}

// ---------------------------------------------------------------------------
// The http-fetch interceptor seam (PLUG-M02 — v2 `StreamOptions.http`,
// model-request.ts:320-348)
// ---------------------------------------------------------------------------

/// A buffered response snapshot handed to an [`HttpInterceptors`]
/// `on_response` callback (v2 passes a `Response`; the Rust contract is
/// the plain parts plugins rewrite — status, headers, full body).
#[derive(Debug, Clone)]
pub struct HttpResponseSnapshot {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// One half of the interceptor pair.
pub type RequestIntercept = Arc<
    dyn Fn(
            TransportRequest,
        ) -> futures::future::BoxFuture<'static, Result<TransportRequest, String>>
        + Send
        + Sync,
>;
/// One half of the interceptor pair: the (possibly rewritten) snapshot
/// replaces what the route's framing sees.
pub type ResponseIntercept = Arc<
    dyn Fn(
            HttpResponseSnapshot,
        ) -> futures::future::BoxFuture<'static, Result<HttpResponseSnapshot, String>>
        + Send
        + Sync,
>;

/// The request/response hook pair wrapping one transport (v2's fetch
/// wrapper `(req, handler) => …`): `on_request` rewrites the outbound
/// request, `on_response` observes/rewrites the answer. Both run per
/// executed request, so every transport the route uses — the WebSocket
/// fallback included — passes through them (model-request.ts:349-350).
pub struct HttpInterceptors {
    pub on_request: RequestIntercept,
    pub on_response: ResponseIntercept,
}

/// The wrapped transport: dispatches [`HttpInterceptors`] around the
/// inner transport's execution. The response body is buffered for the
/// `on_response` pass (hooks opt into this by registering — the
/// unwrapped path streams unchanged, v2's `hasHttpHooks` gate).
struct InterceptedTransport {
    inner: SharedTransport,
    intercepts: HttpInterceptors,
}

#[async_trait]
impl Transport for InterceptedTransport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let request = (self.intercepts.on_request)(request).await.map_err(hook_transport_error)?;
        let response = self.inner.execute(request).await?;
        let body = read_body_limited(response.body, usize::MAX).await?;
        let snapshot = (self.intercepts.on_response)(HttpResponseSnapshot {
            status: response.status,
            headers: response.headers,
            body: body.into_bytes(),
        })
        .await
        .map_err(hook_transport_error)?;
        let body: ByteStream =
            match snapshot.body.len() {
                0 => futures::stream::empty::<Result<Bytes, TransportError>>().boxed(),
                _ => futures::stream::once(async move {
                    Ok::<_, TransportError>(Bytes::from(snapshot.body))
                })
                .boxed(),
            };
        Ok(TransportResponse { status: snapshot.status, headers: snapshot.headers, body })
    }
}

/// A hook failure is a transport failure: v2 fails the request with the
/// hook's defect, and the route's error taxonomy carries it.
fn hook_transport_error(message: String) -> TransportError {
    TransportError::connection(
        format!("http interceptor failed: {message}"),
        TransportOperation::Request,
    )
}

/// Wraps `inner` with the [`HttpInterceptors`] pair — the transport-level
/// seam `session.http.request` / `session.http.response` hooks ride (the
/// caller installs the wrapped transport onto the request's route).
#[must_use]
pub fn intercepted(inner: SharedTransport, intercepts: HttpInterceptors) -> SharedTransport {
    Arc::new(InterceptedTransport { inner, intercepts })
}

// ---------------------------------------------------------------------------
// The 401 refresh-retry seam (G-285② — v2's provider `retry` hook, the
// snowflake-cortex shape, plugin/provider/snowflake-cortex.ts:236-246)
// ---------------------------------------------------------------------------

/// The refresh consult: returns the freshly issued bearer token, or
/// `None` when the gate declined (no qualifying credential, a refresh
/// failure, an env-token override) — `None` leaves the 401 to the
/// normal Authentication classification.
pub type AuthRefresh =
    Arc<dyn Fn() -> futures::future::BoxFuture<'static, Option<String>> + Send + Sync>;

/// The 401-refresh-retry transport (G-285② — the route-layer shape of
/// snowflake-cortex's `retry` hook, snowflake-cortex.ts:236-246): a 401
/// answer consults [`AuthRefresh`]; a fresh token re-issues the SAME
/// request exactly ONCE with the rewritten `Authorization` header
/// (`event.decision = { retry: true, delay: 0 }`), every other outcome
/// returns the original response untouched. The refresh gate itself
/// (provider, credential shape, env override) lives with the caller —
/// the seam stays provider-agnostic.
struct AuthRetry401Transport {
    inner: SharedTransport,
    refresh: AuthRefresh,
}

#[async_trait]
impl Transport for AuthRetry401Transport {
    async fn execute(
        &self,
        request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let first = self.inner.execute(request.clone()).await?;
        if first.status != 401 {
            return Ok(first);
        }
        // `attempt !== 2` gate: exactly one refresh-retry per executed
        // request — a second 401 (or a declined gate) falls through.
        let Some(token) = (self.refresh)().await else {
            return Ok(first);
        };
        let mut retry = request;
        let value = format!("Bearer {token}");
        let mut replaced = false;
        for (name, header) in retry.headers.iter_mut() {
            if name.eq_ignore_ascii_case("authorization") {
                *header = value.clone();
                replaced = true;
            }
        }
        if !replaced {
            retry.headers.push(("authorization".to_owned(), value));
        }
        self.inner.execute(retry).await
    }
}

/// Wrap `inner` with the G-285② 401 refresh-retry ([`AuthRetry401Transport`]).
#[must_use]
pub fn auth_retry_401(inner: SharedTransport, refresh: AuthRefresh) -> SharedTransport {
    Arc::new(AuthRetry401Transport { inner, refresh })
}

#[cfg(test)]
mod auth_retry_tests {
    use super::*;
    use futures::StreamExt as _;

    /// A scripted transport: answers with the queued statuses in order
    /// (repeating the last), recording every request's authorization
    /// header. Status 0 stands for a transport failure.
    struct Scripted {
        statuses: std::sync::Mutex<Vec<u16>>,
        auths: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Transport for Scripted {
        async fn execute(
            &self,
            request: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            self.auths
                .lock()
                .unwrap()
                .push(request.header("authorization").unwrap_or("<none>").to_owned());
            let status = {
                let mut queue = self.statuses.lock().unwrap();
                if queue.len() > 1 {
                    queue.remove(0)
                } else {
                    queue[0]
                }
            };
            if status == 0 {
                return Err(TransportError::connection("boom", TransportOperation::Request));
            }
            Ok(TransportResponse {
                status,
                headers: BTreeMap::new(),
                body: futures::stream::iter(vec![Ok::<_, TransportError>(Bytes::new())]).boxed(),
            })
        }
    }

    fn request() -> TransportRequest {
        TransportRequest {
            method: "POST".into(),
            url: "https://api.test/v1".into(),
            headers: vec![("Authorization".into(), "Bearer stale".into())],
            body: Bytes::new(),
        }
    }

    fn refresh_returning(token: Option<&'static str>) -> AuthRefresh {
        let token = token.map(str::to_owned);
        Arc::new(move || {
            let token = token.clone();
            Box::pin(async move { token })
        })
    }

    /// G-285② — a 401 consults the refresh seam and re-issues the SAME
    /// request once with the rewritten bearer (snowflake-cortex.ts:236-246
    /// — `{ retry: true, delay: 0 }`).
    #[tokio::test]
    async fn refreshes_once_and_retries_with_the_new_bearer() {
        let scripted = Arc::new(Scripted {
            statuses: std::sync::Mutex::new(vec![401, 200]),
            auths: std::sync::Mutex::new(vec![]),
        });
        let transport =
            auth_retry_401(Arc::clone(&scripted) as SharedTransport, refresh_returning(Some("fresh")));
        let response = transport.execute(request()).await.expect("execute");
        assert_eq!(response.status, 200, "the retry's answer surfaces");
        assert_eq!(
            *scripted.auths.lock().unwrap(),
            vec!["Bearer stale".to_owned(), "Bearer fresh".to_owned()],
            "exactly one retry with the refreshed bearer"
        );
    }

    /// A declined gate (`None` — no qualifying credential / env token)
    /// leaves the original 401 untouched: no retry, the normal
    /// Authentication classification applies.
    #[tokio::test]
    async fn declined_gate_returns_the_original_401() {
        let scripted = Arc::new(Scripted {
            statuses: std::sync::Mutex::new(vec![401]),
            auths: std::sync::Mutex::new(vec![]),
        });
        let transport =
            auth_retry_401(Arc::clone(&scripted) as SharedTransport, refresh_returning(None));
        let response = transport.execute(request()).await.expect("execute");
        assert_eq!(response.status, 401, "the 401 passes through");
        assert_eq!(scripted.auths.lock().unwrap().len(), 1, "no retry was issued");
    }

    /// Exactly-once discipline: a retry that 401s again never triggers a
    /// second refresh/retry (the ref's `attempt !== 2` gate).
    #[tokio::test]
    async fn a_second_401_does_not_retry_again() {
        let scripted = Arc::new(Scripted {
            statuses: std::sync::Mutex::new(vec![401, 401, 200]),
            auths: std::sync::Mutex::new(vec![]),
        });
        let transport =
            auth_retry_401(Arc::clone(&scripted) as SharedTransport, refresh_returning(Some("fresh")));
        let response = transport.execute(request()).await.expect("execute");
        assert_eq!(response.status, 401, "the second 401 surfaces");
        assert_eq!(scripted.auths.lock().unwrap().len(), 2, "no third request");
    }

    /// Non-401 statuses (and transport failures) never consult the seam.
    #[tokio::test]
    async fn only_401_consults_the_refresh_seam() {
        let scripted = Arc::new(Scripted {
            statuses: std::sync::Mutex::new(vec![400]),
            auths: std::sync::Mutex::new(vec![]),
        });
        let transport =
            auth_retry_401(Arc::clone(&scripted) as SharedTransport, refresh_returning(Some("fresh")));
        let response = transport.execute(request()).await.expect("execute");
        assert_eq!(response.status, 400);
        assert_eq!(scripted.auths.lock().unwrap().len(), 1);
    }

    /// A request without an authorization header gains one on the retry.
    #[tokio::test]
    async fn the_retry_gains_an_authorization_header_when_absent() {
        let scripted = Arc::new(Scripted {
            statuses: std::sync::Mutex::new(vec![401, 200]),
            auths: std::sync::Mutex::new(vec![]),
        });
        let transport =
            auth_retry_401(Arc::clone(&scripted) as SharedTransport, refresh_returning(Some("fresh")));
        let bare = TransportRequest { headers: vec![], ..request() };
        let response = transport.execute(bare).await.expect("execute");
        assert_eq!(response.status, 200);
        assert_eq!(
            *scripted.auths.lock().unwrap(),
            vec!["<none>".to_owned(), "Bearer fresh".to_owned()]
        );
    }
}
