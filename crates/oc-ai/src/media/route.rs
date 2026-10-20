//! Media route executor (port of `src/route/media.ts` — ledger AI-M2).
//!
//! v2 composes every media protocol with its endpoint, auth, deployment
//! headers, and transport inside `MediaRoute`: option merging,
//! upload-before-submit (`prepare`), URL and query rendering, auth
//! headers, JSON / multipart / binary encoding, submit → poll →
//! result / cancel for queued routes, and the frame state machine for
//! stream routes. The protocol modules under [`crate::media`] were dead
//! code without this layer; the drivers below close that seam.
//!
//! Divergence (determinism): the TS `READ_RETRY` schedule is jittered
//! (`Schedule.jittered`); the port keeps the un-jittered exponential gaps
//! so retry timing is deterministic under test. A provider `retryAfterMs`
//! still raises the gap, capped at 30s, exactly like v2.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt as _;

use crate::media::body::{Body, Form, FormValue, Query, QueryValue};
use crate::media::model::{
    is_terminal, Addressed, CollectedResponse, Mode, ResponseContext, Snapshot,
};
use crate::media::protocol::{
    Frame, Framing, InlineProtocol, MediaSend, QueuedProtocol, SendResponse, StreamProtocol,
    TextOutput,
};
use crate::route::auth::{self, AuthInput, Definition as AuthDefinition, Headers};
use crate::route::endpoint::trim_base_url;
use crate::route::executor::{http_failure, provider_message, HttpFailureInput, MAX_BODY_CHARS};
use crate::route::framing::{Framing as RouteFraming, FramingDecoder};
use crate::route::transport::timeouts::TimeoutBudget;
use crate::route::transport::{
    self, read_body_limited, SharedTransport, TransportError, TransportRequest, TransportResponse,
};
use crate::schema::{AiError, AiErrorReason, HttpContext, HttpOptions};

/// Response bodies read for decoding are capped at 64 MiB (queued results
/// can carry megabytes of base64 media, but never unbounded).
const MAX_DECODE_BODY: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------
// READ_RETRY (route/media.ts:141-165)
// ---------------------------------------------------------------------------

/// Status/result read retry policy (port of `READ_RETRY`): exponential
/// gaps from 1s capped at 30s, at most [`Self::DEFAULT_ATTEMPTS`] retries,
/// raised to a provider `retryAfterMs` for rate limits / provider
/// internals. `start` and `cancel` never retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRetryPolicy {
    /// Maximum retry attempts after the first failure (v2: 8).
    pub max_attempts: u32,
    /// First gap (v2: 1 second).
    pub initial_delay: Duration,
    /// Gap cap (v2: 30 seconds).
    pub max_delay: Duration,
}

impl Default for ReadRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: Self::DEFAULT_ATTEMPTS,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl ReadRetryPolicy {
    pub const DEFAULT_ATTEMPTS: u32 = 8;

    /// The gap before retry `attempt` (1-based) after `error`, or `None`
    /// when the policy is exhausted or the error is not retryable
    /// (`Schedule.max([exponential-spaced, recurs(8)]).jittered` with the
    /// `modifyDelay` rate-limit raise, un-jittered).
    #[must_use]
    pub fn delay(&self, attempt: u32, error: &AiError) -> Option<Duration> {
        if attempt > self.max_attempts || !crate::provider_error::is_retryable(error) {
            return None;
        }
        let exponent = attempt.saturating_sub(1).min(31);
        let mut delay = self
            .initial_delay
            .checked_mul(2u32.saturating_pow(exponent))
            .unwrap_or(self.max_delay)
            .min(self.max_delay);
        // RateLimit / ProviderInternal raise the gap to retryAfterMs.
        let retry_after = match &error.reason {
            AiErrorReason::RateLimit { retry_after_ms, .. } => *retry_after_ms,
            AiErrorReason::ProviderInternal { retry_after_ms, .. } => *retry_after_ms,
            _ => None,
        };
        if let Some(ms) = retry_after {
            delay = delay.max(Duration::from_secs_f64((ms / 1_000.0).max(0.0)));
        }
        Some(delay.min(self.max_delay))
    }
}

// ---------------------------------------------------------------------------
// Deployment spec (route/media.ts:25-49)
// ---------------------------------------------------------------------------

/// Deployment inputs every media route accepts (port of `ModelInput` /
/// `MediaRoute.deployment`): the route's base URL and header overlays, the
/// model-level `http` overlay, auth, and the transport.
#[derive(Clone)]
pub struct MediaRouteSpec {
    /// Overrides the protocol's default deployment base URL.
    pub base_url: Option<String>,
    /// The model-level `http` overlay (merged under the request's own).
    pub model_http: Option<HttpOptions>,
    pub auth: Option<AuthDefinition>,
    pub transport: SharedTransport,
    /// Status/result read retry; `start`/`cancel` never retry.
    pub read_retry: ReadRetryPolicy,
}

impl MediaRouteSpec {
    #[must_use]
    pub fn new(transport: SharedTransport) -> Self {
        Self {
            base_url: None,
            model_http: None,
            auth: None,
            transport,
            read_retry: ReadRetryPolicy::default(),
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    #[must_use]
    pub fn with_auth(mut self, auth: AuthDefinition) -> Self {
        self.auth = Some(auth);
        self
    }
}

// ---------------------------------------------------------------------------
// Encoding helpers (route/media.ts:390-416)
// ---------------------------------------------------------------------------

/// The encoded body plus the headers the encoding owns.
struct Encoded {
    /// What auth signs (`encode(call.body).text`): the JSON text, a
    /// `[contentType]` marker for binary, `[multipart/form-data]`, or the
    /// empty string for bodiless calls.
    text: String,
    bytes: Bytes,
    content_type: Option<String>,
    /// Multipart bodies must not carry a caller content-type (the boundary
    /// lives there); the encoder replaces it.
    replaces_content_type: bool,
}

impl Encoded {
    fn none() -> Self {
        Self {
            text: String::new(),
            bytes: Bytes::new(),
            content_type: None,
            replaces_content_type: false,
        }
    }

    fn json(value: &serde_json::Map<String, serde_json::Value>) -> Self {
        let text = serde_json::to_string(value).unwrap_or_default();
        Self {
            bytes: Bytes::from(text.clone()),
            text,
            content_type: Some("application/json".to_string()),
            replaces_content_type: true,
        }
    }

    fn binary(value: &[u8], content_type: &str) -> Self {
        Self {
            text: format!("[{content_type}]"),
            bytes: Bytes::copy_from_slice(value),
            content_type: Some(content_type.to_string()),
            replaces_content_type: true,
        }
    }

    fn multipart(form: &Form) -> Self {
        let boundary = format!("opencode-media-{}", next_boundary());
        Self {
            text: "[multipart/form-data]".to_string(),
            bytes: Bytes::from(encode_multipart(form, &boundary)),
            content_type: Some(format!("multipart/form-data; boundary={boundary}")),
            replaces_content_type: true,
        }
    }
}

fn encode(body: Option<&Body>) -> Encoded {
    match body {
        None => Encoded::none(),
        Some(Body::Json { value, .. }) => Encoded::json(value),
        Some(Body::Multipart(form)) => Encoded::multipart(form),
        Some(Body::Binary { value, content_type, .. }) => Encoded::binary(value, content_type),
    }
}

fn next_boundary() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Encode a multipart form (the transport seam is bytes + headers, so the
/// route renders the `multipart/form-data` body itself).
fn encode_multipart(form: &Form, boundary: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for (name, value) in &form.fields {
        match value {
            FormValue::Field(text) => {
                out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                out.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                out.extend_from_slice(text.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
            FormValue::File { filename, media_type, data } => {
                out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n"
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(format!("Content-Type: {media_type}\r\n\r\n").as_bytes());
                out.extend_from_slice(data);
                out.extend_from_slice(b"\r\n");
            }
        }
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

/// `encodeURIComponent` parity for query values.
fn encode_query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Ordered query parameters: later layers delete-then-append per key, as
/// `withQuery` does on `URL.searchParams` (route/media.ts:390-396).
#[derive(Default, Debug, Clone)]
struct QueryParams {
    entries: Vec<(String, String)>,
}

impl QueryParams {
    fn apply(&mut self, query: &Query) {
        for (key, value) in &query.0 {
            let values = match value {
                QueryValue::One(one) => vec![one.clone()],
                QueryValue::Many(many) => many.clone(),
            };
            self.entries.retain(|(k, _)| k != key);
            for value in values {
                self.entries.push((key.clone(), value));
            }
        }
    }

    fn render(&self) -> String {
        self.entries
            .iter()
            .map(|(key, value)| {
                format!("{}={}", encode_query_value(key), encode_query_value(value))
            })
            .collect::<Vec<_>>()
            .join("&")
    }
}

/// Build a request URL: trimmed base + path (which may carry its own
/// query, like Gemini's `?alt=sse`), then each query layer applied in
/// order (route/media.ts:370-377, 383-385).
#[must_use]
pub fn build_url(base: &str, path: &str, layers: &[Option<&Query>]) -> String {
    let mut params = QueryParams::default();
    let (path_part, existing) = match path.split_once('?') {
        Some((path_part, existing)) => (path_part, Some(existing)),
        None => (path, None),
    };
    if let Some(existing) = existing {
        for pair in existing.split('&').filter(|pair| !pair.is_empty()) {
            match pair.split_once('=') {
                Some((key, value)) => params.entries.push((key.to_string(), value.to_string())),
                None => params.entries.push((pair.to_string(), String::new())),
            }
        }
    }
    for layer in layers.iter().flatten() {
        params.apply(layer);
    }
    let url = format!("{}{}", trim_base_url(base), path_part);
    let query = params.render();
    if query.is_empty() {
        url
    } else {
        format!("{url}?{query}")
    }
}

/// The `http` overlay of any media request: the four modality structs all
/// carry `http`, so the driver erases the modality at this one seam.
fn erased_http(request: &(dyn std::any::Any + Send + Sync)) -> Option<&HttpOptions> {
    request
        .downcast_ref::<crate::media::model::ImageRequest>()
        .and_then(|r| r.http.as_ref())
        .or_else(|| {
            request
                .downcast_ref::<crate::media::model::SpeechRequest>()
                .and_then(|r| r.http.as_ref())
        })
        .or_else(|| {
            request
                .downcast_ref::<crate::media::model::VideoRequest>()
                .and_then(|r| r.http.as_ref())
        })
        .or_else(|| {
            request
                .downcast_ref::<crate::media::model::TranscriptionRequest>()
                .and_then(|r| r.http.as_ref())
        })
}

/// A body's query layer (multipart bodies carry none on the URL,
/// route/media.ts:370-375).
fn query_of(body: &Body) -> Option<Query> {
    match body {
        Body::Json { query, .. } | Body::Binary { query, .. } => query.clone(),
        Body::Multipart(_) => None,
    }
}

fn http_query_layer(http: &HttpOptions) -> Option<Query> {
    http.query.as_ref().map(|query| {
        Query(query.iter().map(|(k, v)| (k.clone(), QueryValue::One(v.clone()))).collect())
    })
}

// ---------------------------------------------------------------------------
// Transport plumbing shared by every kind (route/media.ts:309-416)
// ---------------------------------------------------------------------------

/// One executed request: the response plus the auth headers that were
/// added or changed on the wire (`auth` in route/media.ts:338-341).
struct Sent {
    response: TransportResponse,
    auth: BTreeMap<String, String>,
    url: String,
}

impl Sent {
    fn http_context(&self) -> HttpContext {
        HttpContext {
            url: self.url.clone(),
            status: self.response.status,
            headers: self.response.headers.clone(),
        }
    }
}

/// An owned decode side: the body text plus the HTTP context and auth
/// diff the protocol decoders receive.
struct Decoded {
    body: String,
    http: HttpContext,
    auth: BTreeMap<String, String>,
}

impl Decoded {
    fn text_output<'a>(&'a self, identity: &'a crate::media::protocol::Identity) -> TextOutput<'a> {
        TextOutput { identity, body: &self.body, http: Some(&self.http) }
    }
}

/// The shared transport plumbing (port of `makeTransport`).
struct CoreTransport {
    base_url: String,
    /// Deployment headers applied before transport authentication
    /// (`input.headers`, route/media.ts:97-99).
    headers: Vec<(String, String)>,
    auth: AuthDefinition,
    transport: SharedTransport,
}

impl Clone for CoreTransport {
    fn clone(&self) -> Self {
        Self {
            base_url: self.base_url.clone(),
            headers: self.headers.clone(),
            auth: self.auth.clone(),
            transport: Arc::clone(&self.transport),
        }
    }
}

impl CoreTransport {
    fn new(spec: &MediaRouteSpec, base_url: String, headers: Vec<(String, String)>) -> Self {
        Self {
            base_url,
            headers,
            auth: spec.auth.clone().unwrap_or_else(auth::Definition::none),
            transport: Arc::clone(&spec.transport),
        }
    }

    /// Route + model overlays (`http(model)`, route/media.ts:344-345): the
    /// route's deployment headers ride as `http.headers` under the
    /// model's own overlay.
    fn http_options(&self, model_http: Option<&HttpOptions>) -> HttpOptions {
        let route = (!self.headers.is_empty()).then(|| HttpOptions {
            headers: Some(self.headers.iter().cloned().collect()),
            ..Default::default()
        });
        crate::schema::options::merge_http_options(&[route.as_ref(), model_http])
            .unwrap_or_default()
    }

    /// The request-level overlay chain: route headers ⊕ model ⊕ request
    /// (`mergeHttpOptions(routeHttp, request.model.http, request.http)`,
    /// route/media.ts:357).
    fn request_http(
        &self,
        model_http: Option<&HttpOptions>,
        request_http: Option<&HttpOptions>,
    ) -> HttpOptions {
        let route = (!self.headers.is_empty()).then(|| HttpOptions {
            headers: Some(self.headers.iter().cloned().collect()),
            ..Default::default()
        });
        crate::schema::options::merge_http_options(&[route.as_ref(), model_http, request_http])
            .unwrap_or_default()
    }

    fn resolve(&self, path: &str) -> String {
        // Provider-issued absolute URLs (fal `status_url`) are used as-is.
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", trim_base_url(&self.base_url), path)
        }
    }

    /// `send` (route/media.ts:316-342): encode, authorize, execute under
    /// the v2 timeout budget, and report the auth-only header diff.
    async fn send(
        &self,
        method: &str,
        url: String,
        base_headers: Headers,
        body: Option<&Body>,
        http: Option<&HttpOptions>,
    ) -> Result<Sent, AiError> {
        let encoded = encode(body);
        let mut headers = base_headers;
        if encoded.replaces_content_type {
            headers.remove("content-type");
        }
        if let Some(content_type) = &encoded.content_type {
            headers.set("content-type", content_type.clone());
        }
        let auth_input = AuthInput {
            request_http: http,
            method,
            url: &url,
            body: &encoded.text,
            headers: headers.clone(),
        };
        let authorized = self.auth.apply(&auth_input)?;
        let header_vec: Vec<(String, String)> =
            authorized.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        // The auth diff: only headers auth added or changed, never
        // untouched deployment headers (route/media.ts:340).
        let auth_diff: BTreeMap<String, String> = header_vec
            .iter()
            .filter(|(name, value)| {
                auth_input.headers.get(name).is_none_or(|old| old != value.as_str())
            })
            .cloned()
            .collect();

        let budget = TimeoutBudget::from_http_options(http);
        let started = tokio::time::Instant::now();
        let request = TransportRequest {
            method: method.to_string(),
            url: url.clone(),
            headers: header_vec,
            body: encoded.bytes.clone(),
        };
        let response = transport::timeouts::execute_with_timeouts(
            Arc::clone(&self.transport),
            request,
            budget,
        )
        .await
        .map_err(TransportError::into_ai_error)?;
        let TransportResponse { status, headers: response_headers, body } = response;
        let http_context =
            HttpContext { url: url.clone(), status, headers: response_headers.clone() };
        let body =
            transport::timeouts::body_with_timeouts(body, budget, started, &url, http_context);
        Ok(Sent {
            response: TransportResponse { status, headers: response_headers, body },
            auth: auth_diff,
            url,
        })
    }

    /// Bodiless follow-up call (status, result, cancel) with the same auth
    /// and deployment headers as submit (route/media.ts:380-386).
    async fn call(&self, method: &str, path: &str, http: &HttpOptions) -> Result<Sent, AiError> {
        let mut headers = Headers::new();
        for (name, value) in &self.headers {
            headers.set(name.clone(), value.clone());
        }
        if let Some(extra) = &http.headers {
            for (name, value) in extra {
                headers.set(name.clone(), value.clone());
            }
        }
        let url = build_url(&self.resolve(path), "", &[http_query_layer(http).as_ref()]);
        self.send(method, url, headers, None, Some(http)).await
    }

    /// Classify one exchange: non-2xx statuses fail through the
    /// provider-error taxonomy (the TS executor's `httpFailure`); 2xx
    /// bodies decode into the [`Decoded`] read side.
    async fn classified(&self, sent: Sent) -> Result<Decoded, AiError> {
        let http = sent.http_context();
        if http.status >= 400 {
            return Err(status_failure(http, sent.response.body).await);
        }
        let body = read_body_limited(sent.response.body, MAX_DECODE_BODY)
            .await
            .map_err(TransportError::into_ai_error)?;
        Ok(Decoded { body, http, auth: sent.auth })
    }
}

/// Non-2xx classification with the provider-error taxonomy.
async fn status_failure(http: HttpContext, body: transport::ByteStream) -> AiError {
    let text = read_body_limited(body, MAX_BODY_CHARS).await.unwrap_or_default();
    let message = provider_message(http.status, Some(&text));
    let response_headers: Vec<(String, String)> =
        http.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    http_failure(HttpFailureInput {
        message: &message,
        url: Some(&http.url),
        status: Some(http.status),
        data: None,
        response_headers: &response_headers,
        response_body: Some(&text),
    })
}

// ---------------------------------------------------------------------------
// Inline routes (route/media.ts:121-139)
// ---------------------------------------------------------------------------

/// One runnable inline media route (port of `MediaRoute.inline`).
pub struct InlineMediaRoute<P: InlineProtocol> {
    protocol: Arc<P>,
    core: CoreTransport,
    model_http: Option<HttpOptions>,
}

impl<P: InlineProtocol> Clone for InlineMediaRoute<P> {
    fn clone(&self) -> Self {
        Self {
            protocol: Arc::clone(&self.protocol),
            core: self.core.clone(),
            model_http: self.model_http.clone(),
        }
    }
}

impl<P: InlineProtocol> InlineMediaRoute<P> {
    #[must_use]
    pub fn new(protocol: Arc<P>, spec: MediaRouteSpec) -> Self {
        let headers = protocol.headers();
        let base = spec.base_url.clone().unwrap_or_else(|| protocol.base_url());
        Self {
            core: CoreTransport::new(&spec, base, headers),
            model_http: spec.model_http,
            protocol,
        }
    }

    /// One request, one response (port of `inline.generate`): submit the
    /// protocol body, then decode through the protocol's response side.
    pub async fn generate(&self, request: &P::Request) -> Result<P::Response, AiError> {
        let (sent, body) = self.submit(request).await?;
        let identity = self.protocol.identity();
        let decoded = self.core.classified(sent).await?;
        let output = decoded.text_output(&identity);
        self.protocol.decode(&output, &crate::media::model::DecodeContext { request, body: &body })
    }

    /// The submit plumbing: merge route ⊕ model ⊕ request `http`, build
    /// the body, render the URL, send (route/media.ts:347-379).
    async fn submit(&self, request: &P::Request) -> Result<(Sent, Body), AiError> {
        let http = self.core.request_http(self.model_http.as_ref(), erased_http(request));
        let headers = base_headers(&http);
        let body = self.protocol.body_from(request)?;
        let url = build_url(
            &self.core.base_url,
            &self.protocol.path(request),
            &[query_of(&body).as_ref(), http_query_layer(&http).as_ref()],
        );
        let sent = self.core.send("POST", url, headers, Some(&body), Some(&http)).await?;
        Ok((sent, body))
    }
}

fn base_headers(http: &HttpOptions) -> Headers {
    let mut headers = Headers::new();
    if let Some(extra) = &http.headers {
        for (name, value) in extra {
            headers.set(name.clone(), value.clone());
        }
    }
    headers
}

// ---------------------------------------------------------------------------
// Queued routes (route/media.ts:141-245)
// ---------------------------------------------------------------------------

/// One runnable queued media route (port of `MediaRoute.queued`).
pub struct QueuedMediaRoute<P: QueuedProtocol> {
    protocol: Arc<P>,
    core: CoreTransport,
    model_http: Option<HttpOptions>,
    read_retry: ReadRetryPolicy,
}

impl<P: QueuedProtocol> Clone for QueuedMediaRoute<P> {
    fn clone(&self) -> Self {
        Self {
            protocol: Arc::clone(&self.protocol),
            core: self.core.clone(),
            model_http: self.model_http.clone(),
            read_retry: self.read_retry,
        }
    }
}

impl<P: QueuedProtocol> QueuedMediaRoute<P> {
    #[must_use]
    pub fn new(protocol: Arc<P>, spec: MediaRouteSpec) -> Self {
        let read_retry = spec.read_retry;
        let headers = protocol.headers();
        let base = spec.base_url.clone().unwrap_or_else(|| protocol.base_url());
        Self {
            core: CoreTransport::new(&spec, base, headers),
            model_http: spec.model_http,
            protocol,
            read_retry,
        }
    }

    /// Submit, then hand back a pollable [`Generation`] (port of `start`).
    /// The request's merged `http` overlay rides the handle so follow-up
    /// calls reuse it (`generationRoute(started.token,
    /// submitted.context.request.http)`).
    pub async fn start(&self, request: &P::Request) -> Result<Generation<P>, AiError> {
        let (sent, _body, http) = self.submit(request).await?;
        let identity = self.protocol.identity();
        let decoded = self.core.classified(sent).await?;
        let output = decoded.text_output(&identity);
        let started = self.protocol.decode_start(&output)?;
        let encoded = self.protocol.encode_token(&started.token)?;
        Ok(Generation {
            route: Arc::new(self.clone()),
            token: started.token,
            encoded,
            http,
            first_snapshot: Some(started.snapshot),
        })
    }

    /// Rebuild a handle from a persisted token (port of `resume`): a token
    /// this protocol's codec rejects fails typed; the handle's first
    /// observation is a fresh status read.
    pub async fn resume(&self, raw: &serde_json::Value) -> Result<Generation<P>, AiError> {
        let token = self.protocol.decode_token(raw).map_err(|cause| {
            AiError::new(AiErrorReason::InvalidRequest {
                fields: crate::schema::ReasonFields {
                    message: format!(
                        "{} cannot resume a generation from this token",
                        self.protocol.identity().id
                    ),
                    body: None,
                    http: None,
                    cause: Some(cause.to_string()),
                },
                parameter: None,
                classification: None,
            })
        })?;
        let http = self.core.http_options(self.model_http.as_ref());
        let generation = Generation {
            route: Arc::new(self.clone()),
            encoded: self.protocol.encode_token(&token)?,
            http,
            first_snapshot: None,
            token,
        };
        // The resume handle eagerly reads one status observation.
        let snapshot = generation.status().await?;
        Ok(Generation { first_snapshot: Some(snapshot), ..generation })
    }

    /// The submit plumbing: `prepare` (upload-before-submit) runs before
    /// the body build, then the body posts to the route endpoint
    /// (route/media.ts:213-222).
    async fn submit(&self, request: &P::Request) -> Result<(Sent, Body, HttpOptions), AiError> {
        let http = self.core.request_http(self.model_http.as_ref(), erased_http(request));
        let headers = base_headers(&http);
        let prepared = match self.protocol.prepare() {
            Some(prepare) => {
                let sender =
                    RouteMediaSend { core: &self.core, http: &http, headers: headers.clone() };
                Some(prepare(request, &sender).await?)
            }
            None => None,
        };
        let effective = prepared.as_ref().unwrap_or(request);
        let body = self.protocol.body_from(effective)?;
        let url = build_url(
            &self.core.base_url,
            &self.protocol.start_path(effective),
            &[query_of(&body).as_ref(), http_query_layer(&http).as_ref()],
        );
        let sent = self.core.send("POST", url, headers, Some(&body), Some(&http)).await?;
        Ok((sent, body, http))
    }
}

/// `MediaProtocol.Send` bound to one submit's transport plumbing.
struct RouteMediaSend<'a> {
    core: &'a CoreTransport,
    http: &'a HttpOptions,
    headers: Headers,
}

impl MediaSend for RouteMediaSend<'_> {
    fn send(
        &self,
        path: &str,
        body: Body,
    ) -> Pin<Box<dyn Future<Output = Result<SendResponse, AiError>> + Send + '_>> {
        let path = path.to_string();
        Box::pin(async move {
            let url = build_url(
                &self.core.resolve(&path),
                "",
                &[query_of(&body).as_ref(), http_query_layer(self.http).as_ref()],
            );
            let sent = self
                .core
                .send("POST", url, self.headers.clone(), Some(&body), Some(self.http))
                .await?;
            let status = sent.response.status;
            let headers = sent.response.headers.clone();
            let body_text = read_body_limited(sent.response.body, MAX_DECODE_BODY)
                .await
                .map_err(TransportError::into_ai_error)?;
            Ok(SendResponse { status, headers, body: body_text, auth: sent.auth })
        })
    }
}

/// A submitted generation: poll `status`/`result`, or `cancel` it
/// (port of `Generation` over `generationRoute`, route/media.ts:181-211).
pub struct Generation<P: QueuedProtocol> {
    route: Arc<QueuedMediaRoute<P>>,
    token: P::Token,
    encoded: serde_json::Value,
    http: HttpOptions,
    first_snapshot: Option<Snapshot>,
}

impl<P: QueuedProtocol> Generation<P> {
    /// The serializable token (`Generation.token`): persist this and
    /// [`QueuedMediaRoute::resume`] it elsewhere.
    #[must_use]
    pub fn token(&self) -> &serde_json::Value {
        &self.encoded
    }

    /// The snapshot `start` (or `resume`) already observed, if any.
    #[must_use]
    pub fn first_snapshot(&self) -> Option<&Snapshot> {
        self.first_snapshot.as_ref()
    }

    /// One status observation. Only status/result reads retry transient
    /// failures (`READ_RETRY`); the decode itself never retries.
    pub async fn status(&self) -> Result<Snapshot, AiError> {
        let decoded = self.poll_read(&self.route.protocol.status_path(&self.token)).await?;
        let identity = self.route.protocol.identity();
        let output = decoded.text_output(&identity);
        let context = crate::media::model::PollContext { token: &self.token, auth: &decoded.auth };
        self.route.protocol.decode_status(&output, &context)
    }

    /// The terminal result. A `result()` read before the generation
    /// finished is caller misuse and fails typed through the protocol's
    /// `pending` error.
    pub async fn result(&self) -> Result<P::Response, AiError> {
        let decoded = self.poll_read(&self.route.protocol.result_path(&self.token)).await?;
        let identity = self.route.protocol.identity();
        let output = decoded.text_output(&identity);
        let context = crate::media::model::PollContext { token: &self.token, auth: &decoded.auth };
        self.route.protocol.decode_result(&output, &context)
    }

    /// Cancel the generation (`cancel` never retries). Providers whose
    /// cancel endpoint destroys finished work (`activeOnly`) fetch a fresh
    /// status first and skip the call for terminal generations.
    pub async fn cancel(&self) -> Result<(), AiError> {
        let identity = self.route.protocol.identity();
        let Some(cancel) = self.route.protocol.cancel() else {
            return Err(identity.unsupported(
                "media.cancel",
                format!("{}/{} does not support cancellation", identity.provider, identity.id),
            ));
        };
        if cancel.active_only {
            let snapshot = self.status().await?;
            if is_terminal(snapshot.status) {
                return Ok(());
            }
        }
        let path = self.route.protocol.cancel_path(&self.token);
        let sent = self.route.core.call(cancel.method, &path, &self.http).await?;
        // Drain the response so mid-body transport failures surface.
        let _ = read_body_limited(sent.response.body, MAX_DECODE_BODY)
            .await
            .map_err(TransportError::into_ai_error)?;
        Ok(())
    }

    /// A poll read: only the GET exchange (including non-2xx
    /// classification) retries per `READ_RETRY` — the decode never does, so
    /// a decoded terminal failure does not spin until the caller's
    /// deadline (route/media.ts:183-196).
    async fn poll_read(&self, path: &str) -> Result<Decoded, AiError> {
        let mut attempt = 0u32;
        loop {
            let exchange = async {
                let sent = self.route.core.call("GET", path, &self.http).await?;
                self.route.core.classified(sent).await
            };
            match exchange.await {
                Ok(decoded) => return Ok(decoded),
                Err(error) => {
                    attempt += 1;
                    match self.route.read_retry.delay(attempt, &error) {
                        Some(delay) => tokio::time::sleep(delay).await,
                        None => return Err(error),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stream routes (route/media.ts:248-303)
// ---------------------------------------------------------------------------

/// One runnable streaming media route (port of `MediaRoute.stream`):
/// `generate` runs the same stream in generate mode and folds it with
/// `collect`.
pub struct StreamMediaRoute<P: StreamProtocol> {
    protocol: Arc<P>,
    core: CoreTransport,
    model_http: Option<HttpOptions>,
    collect: fn(&[P::Event]) -> CollectedResponse,
}

impl<P: StreamProtocol + 'static> Clone for StreamMediaRoute<P> {
    fn clone(&self) -> Self {
        Self {
            protocol: Arc::clone(&self.protocol),
            core: self.core.clone(),
            model_http: self.model_http.clone(),
            collect: self.collect,
        }
    }
}

impl<P: StreamProtocol + 'static> StreamMediaRoute<P> {
    #[must_use]
    pub fn new(
        protocol: Arc<P>,
        spec: MediaRouteSpec,
        collect: fn(&[P::Event]) -> CollectedResponse,
    ) -> Self {
        let headers = protocol.headers();
        let base = spec.base_url.clone().unwrap_or_else(|| protocol.base_url());
        Self {
            core: CoreTransport::new(&spec, base, headers),
            model_http: spec.model_http,
            protocol,
            collect,
        }
    }

    /// Run the stream in generate mode and collect the finish event
    /// (port of `generate`: `Stream.runCollect |> flatMap(collect)`). A
    /// stream that ends without a finish event is a protocol defect and
    /// fails typed (the TS `collectResponse` dies).
    pub async fn generate(&self, request: P::Request) -> Result<CollectedResponse, AiError> {
        let mut stream = self.stream(request, Mode::Generate).await?;
        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(event) => events.push(event),
                Err(error) => return Err(error),
            }
        }
        let collected = (self.collect)(&events);
        match collected {
            CollectedResponse::Incomplete => {
                let identity = self.protocol.identity();
                Err(identity
                    .frame_error(format!("{} stream did not finish", identity.name), String::new()))
            }
            collected => Ok(collected),
        }
    }

    /// The event stream for one request (port of `events`): submit with
    /// the call `mode`, frame the response, drive the protocol state
    /// machine (`step` per frame), and emit the terminal event at
    /// `finish`. Parser state is local to one response, exactly like
    /// `Route.make`'s LLM stream loop.
    pub async fn stream(
        &self,
        request: P::Request,
        mode: Mode,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<P::Event, AiError>> + Send>>, AiError>
    {
        let addressed = Addressed { request, mode };
        let (sent, body) = self.submit(&addressed).await?;
        let framing = self.protocol.framing(&addressed, &body);
        let requested =
            body.json_field("output_format").and_then(|value| value.as_str()).map(str::to_string);
        let http_context = HttpContext {
            url: sent.url.clone(),
            status: sent.response.status,
            headers: sent.response.headers.clone(),
        };
        if http_context.status >= 400 {
            return Err(status_failure(http_context, sent.response.body).await);
        }
        let route_framing = match framing {
            Framing::Sse => RouteFraming::sse(),
            Framing::Lines => RouteFraming::lines(),
            Framing::Document | Framing::Bytes => RouteFraming::document(),
        };
        let protocol = Arc::clone(&self.protocol);
        let events = async_stream::stream! {
            let mut decoder = FramingDecoder::new(route_framing);
            let mut state = P::State::default();
            let mut response = sent.response;
            loop {
                let chunk = response.body.next().await;
                let Some(chunk) = chunk else { break };
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(error.into_ai_error());
                        return;
                    }
                };
                let frames: Vec<Frame> = if framing == Framing::Bytes {
                    vec![Frame::Bytes(chunk.to_vec())]
                } else {
                    match decoder.feed(&chunk) {
                        Ok(texts) => texts
                            .into_iter()
                            .map(|text| document_frame(framing, text, requested.clone()))
                            .collect(),
                        Err(error) => {
                            yield Err(attach_http(error, &http_context));
                            return;
                        }
                    }
                };
                for frame in frames {
                    match protocol.step(&mut state, &frame) {
                        Ok(stepped) => {
                            for event in stepped {
                                yield Ok(event);
                            }
                        }
                        Err(error) => {
                            yield Err(attach_http(error, &http_context));
                            return;
                        }
                    }
                }
            }
            // Flush the decoder's remaining text, then the terminal event.
            if framing != Framing::Bytes {
                match decoder.finish() {
                    Ok(texts) => {
                        for frame in texts
                            .into_iter()
                            .map(|text| document_frame(framing, text, requested.clone()))
                        {
                            match protocol.step(&mut state, &frame) {
                                Ok(stepped) => {
                                    for event in stepped {
                                        yield Ok(event);
                                    }
                                }
                                Err(error) => {
                                    yield Err(attach_http(error, &http_context));
                                    return;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        yield Err(attach_http(error, &http_context));
                        return;
                    }
                }
            }
            let context = ResponseContext {
                request: &addressed,
                body: &body,
                http: &http_context,
            };
            match protocol.finish(&state, &context) {
                Ok(events) => {
                    for event in events {
                        yield Ok(event);
                    }
                }
                Err(error) => yield Err(attach_http(error, &http_context)),
            }
        };
        Ok(Box::pin(events))
    }

    async fn submit(&self, addressed: &Addressed<P::Request>) -> Result<(Sent, Body), AiError> {
        let http =
            self.core.request_http(self.model_http.as_ref(), erased_http(&addressed.request));
        let headers = base_headers(&http);
        let body = self.protocol.body_from(addressed)?;
        let url = build_url(
            &self.core.base_url,
            &self.protocol.path(addressed),
            &[query_of(&body).as_ref(), http_query_layer(&http).as_ref()],
        );
        let sent = self.core.send("POST", url, headers, Some(&body), Some(&http)).await?;
        Ok((sent, body))
    }
}

/// A decoded text frame, or a whole-document frame carrying the requested
/// `output_format` (openai-images, protocol.rs:421-430).
fn document_frame(framing: Framing, text: String, requested: Option<String>) -> Frame {
    if framing == Framing::Document {
        Frame::Document { document: text, requested }
    } else {
        Frame::Text(text)
    }
}

/// Attach the HTTP context to errors that carry reason fields without one
/// (port of the stream error mapping, route/media.ts:278-289).
fn attach_http(mut error: AiError, http: &HttpContext) -> AiError {
    match &mut error.reason {
        AiErrorReason::InvalidProviderOutput { fields, .. }
        | AiErrorReason::InvalidRequest { fields, .. }
        | AiErrorReason::ContentPolicy { fields }
        | AiErrorReason::RateLimit { fields, .. }
        | AiErrorReason::ProviderInternal { fields, .. }
        | AiErrorReason::Authentication { fields }
            if fields.http.is_none() =>
        {
            fields.http = Some(http.clone());
        }
        _ => {}
    }
    error
}
