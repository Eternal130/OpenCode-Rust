//! HTTP transport: URL/path building, auth injection, status classification,
//! body decoding. Mirrors the v2 generated client's `prepare`/`execute`/
//! `request`/`sse` helpers.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::ClientError;
use crate::Client;

/// Per-request options (v2 `RequestOptions`): cancellation signal, extra
/// headers applied after the client's defaults (so they override), and an
/// `onActivity` hook invoked for every chunk a streaming response receives.
///
/// Mirrors `packages/client/src/promise/generated/client.ts`:
/// `signal` races the request (abort surfaces as [`ClientError::Cancelled`]
/// rather than the ref's `Transport`/AbortError — reqwest cannot fabricate
/// the transport error), `headers` are merged last, `on_activity` fires per
/// SSE chunk including keepalive comments that yield no frame.
#[derive(Debug, Clone, Default)]
pub struct RequestOptions {
    /// Cancellation token; when cancelled (including before send) the
    /// in-flight request is dropped and [`ClientError::Cancelled`] returns.
    pub signal: Option<tokio_util::sync::CancellationToken>,
    /// Per-request headers, applied after client defaults (override).
    pub headers: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>,
    /// Invoked for every received streaming chunk (SSE).
    pub on_activity: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl RequestOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the cancellation signal (v2 `signal: AbortSignal`).
    #[must_use]
    pub fn with_signal(mut self, token: tokio_util::sync::CancellationToken) -> Self {
        self.signal = Some(token);
        self
    }

    /// Adds a per-request header (v2 `headers`), overriding client defaults.
    pub fn with_header(mut self, name: &str, value: &str) -> Result<Self, ClientError> {
        let name = reqwest::header::HeaderName::try_from(name)
            .map_err(|_| ClientError::Encoding(format!("invalid request header name {name:?}")))?;
        let value = reqwest::header::HeaderValue::try_from(value).map_err(|_| {
            ClientError::Encoding(format!("invalid request header value {value:?}"))
        })?;
        self.headers.push((name, value));
        Ok(self)
    }

    /// Sets the streaming-activity hook (v2 `onActivity`).
    #[must_use]
    pub fn with_on_activity(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_activity = Some(Arc::new(callback));
        self
    }
}

/// The `{ data: T }` envelope for operations whose response is unwrapped
/// client-side (v2 client parity).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Envelope<T> {
    pub data: T,
}

/// Request payload kinds.
pub enum Body<'a> {
    None,
    Json(&'a serde_json::Value),
    Bytes(&'a [u8]),
}

/// Everything a generated method needs to describe one HTTP call.
pub struct Call<'a> {
    pub method: reqwest::Method,
    /// Path template with `{name}` (single-segment) / `{*name}` (wildcard
    /// tail, slashes preserved) placeholders.
    pub path: &'static str,
    pub path_params: &'a [(&'static str, &'a str)],
    pub query: &'a [(String, String)],
    pub headers: &'a [(&'static str, String)],
    pub body: Body<'a>,
    pub success: u16,
    pub declared: &'static [u16],
    /// Per-request options (v2 `RequestOptions`) — signal/headers/on-activity.
    pub options: &'a RequestOptions,
}

/// `encodeURIComponent`-compatible segment encoding.
fn encode_segment(value: &str) -> String {
    use std::fmt::Write as _;
    // RFC 3986 unreserved + the five `encodeURIComponent` extras.
    const OK: &str = "-_.!~*'()";
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() || OK.contains(c) {
            out.push(c);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// v2 `encodePath`: encode every segment, keep `/` separators.
pub(crate) fn encode_path(value: &str) -> String {
    value.split('/').map(encode_segment).collect::<Vec<_>>().join("/")
}

pub(crate) fn render_path(
    template: &str,
    params: &[(&'static str, &str)],
) -> Result<String, ClientError> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(index) = rest.find('{') {
        out.push_str(&rest[..index]);
        let end = rest[index..].find('}').ok_or_else(|| {
            ClientError::Encoding(format!("unbalanced path template {template:?}"))
        })?;
        let token = &rest[index + 1..index + end];
        let value = if let Some(name) = token.strip_prefix('*') {
            params.iter().find(|(n, _)| *n == name).map(|(_, v)| encode_path(v))
        } else {
            params.iter().find(|(n, _)| *n == token).map(|(_, v)| encode_segment(v))
        };
        let Some(value) = value else {
            return Err(ClientError::Encoding(format!(
                "missing path parameter {token:?} for {template:?}"
            )));
        };
        out.push_str(&value);
        rest = &rest[index + end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Serializes a typed request body to JSON for [`Body::Json`].
pub fn encode_body<T: Serialize + ?Sized>(body: &T) -> Result<serde_json::Value, ClientError> {
    serde_json::to_value(body).map_err(|e| ClientError::Encoding(e.to_string()))
}

fn is_content_type(response: &reqwest::Response, expected: &str) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().to_ascii_lowercase() == expected)
}

fn is_json_content_type(response: &reqwest::Response) -> bool {
    response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(
        |v| {
            let v = v.trim().to_ascii_lowercase();
            v.starts_with("application/json") || v.contains("+json")
        },
    )
}

impl Client {
    fn build_request(&self, call: &Call<'_>) -> Result<reqwest::RequestBuilder, ClientError> {
        let path = render_path(call.path, call.path_params)?;
        let base = self.base_url();
        let url = format!("{base}{path}");
        let mut request = self.http.request(call.method.clone(), &url);
        if !call.query.is_empty() {
            request = request.query(call.query);
        }
        for (name, value) in call.headers {
            request = request.header(*name, value.clone());
        }
        if let Some(header) = self.basic_auth_header() {
            request = request.header(reqwest::header::AUTHORIZATION, header);
        }
        for (name, value) in &self.default_headers {
            request = request.header(name, value.clone());
        }
        // Per-request headers go last: they override client defaults (v2
        // merges `requestOptions.headers` after `options.headers`).
        for (name, value) in &call.options.headers {
            request = request.header(name, value.clone());
        }
        match &call.body {
            Body::None => {}
            Body::Json(value) => {
                request =
                    request.header(reqwest::header::CONTENT_TYPE, "application/json").json(value);
            }
            Body::Bytes(bytes) => {
                request = request
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(bytes.to_vec());
            }
        }
        Ok(request)
    }

    async fn send(&self, call: &Call<'_>) -> Result<reqwest::Response, ClientError> {
        let request = self.build_request(call)?;
        let future = request.send();
        match &call.options.signal {
            // Dropping the in-flight future cancels the request. `biased`
            // checks the token first so a pre-cancelled signal never sends.
            Some(token) => tokio::select! {
                biased;
                _ = token.cancelled() => Err(ClientError::Cancelled),
                response = future => response.map_err(ClientError::from),
            },
            None => future.await.map_err(ClientError::from),
        }
    }

    async fn classify(
        &self,
        call: &Call<'_>,
    ) -> Result<Result<reqwest::Response, reqwest::Response>, ClientError> {
        let response = self.send(call).await?;
        let status = response.status().as_u16();
        if status == call.success {
            Ok(Ok(response))
        } else if call.declared.contains(&status) {
            Ok(Err(response))
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(ClientError::UnexpectedStatus { status, body })
        }
    }

    async fn decode_error(response: reqwest::Response) -> ClientError {
        let status = response.status().as_u16();
        if !is_json_content_type(&response) {
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<none>")
                .to_owned();
            return ClientError::UnsupportedContentType(content_type);
        }
        let text = response.text().await.unwrap_or_default();
        if text.is_empty() {
            return ClientError::MalformedResponse("empty error body".to_owned());
        }
        match serde_json::from_str(&text) {
            Ok(value) => crate::error::declared(status, value),
            Err(e) => ClientError::MalformedResponse(format!("error body: {e}")),
        }
    }

    async fn json_body<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ClientError> {
        if !is_json_content_type(&response) {
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<none>")
                .to_owned();
            return Err(ClientError::UnsupportedContentType(content_type));
        }
        let text = response.text().await.map_err(ClientError::from)?;
        if text.is_empty() {
            return Err(ClientError::MalformedResponse("empty body".to_owned()));
        }
        serde_json::from_str(&text).map_err(|e| ClientError::MalformedResponse(e.to_string()))
    }

    /// JSON request with declared-error classification.
    pub async fn execute_json<T: DeserializeOwned>(
        &self,
        call: &Call<'_>,
    ) -> Result<T, ClientError> {
        match self.classify(call).await? {
            Ok(response) => Self::json_body(response).await,
            Err(response) => Err(Self::decode_error(response).await),
        }
    }

    /// 204-style request: success consumes the (empty) body.
    pub async fn execute_empty(&self, call: &Call<'_>) -> Result<(), ClientError> {
        match self.classify(call).await? {
            Ok(response) => {
                // Drain so the connection can be reused.
                let _ = response.bytes().await?;
                Ok(())
            }
            Err(response) => Err(Self::decode_error(response).await),
        }
    }

    /// Binary (octet-stream) request.
    pub async fn execute_bytes(&self, call: &Call<'_>) -> Result<Vec<u8>, ClientError> {
        match self.classify(call).await? {
            Ok(response) => {
                let bytes = response.bytes().await?;
                Ok(bytes.to_vec())
            }
            Err(response) => Err(Self::decode_error(response).await),
        }
    }

    /// SSE request: content-type must be `text/event-stream` (§7).
    pub async fn execute_sse(
        &self,
        call: &Call<'_>,
        double_decode: bool,
    ) -> Result<crate::sse::SseStream, ClientError> {
        let response = match self.classify(call).await? {
            Ok(response) => response,
            Err(response) => return Err(Self::decode_error(response).await),
        };
        if !is_content_type(&response, "text/event-stream") {
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("<none>")
                .to_owned();
            return Err(ClientError::UnsupportedContentType(content_type));
        }
        let mut stream = crate::sse::SseStream::new(response, double_decode);
        if let Some(callback) = &call.options.on_activity {
            let callback = Arc::clone(callback);
            stream = stream.with_on_activity(move || callback());
        }
        Ok(stream)
    }
}
