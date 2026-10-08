//! Process-agnostic transport seam (oracle gate 2, finding 7 — BINDING).
//!
//! Protocols and routes only ever see [`Transport`] and its own
//! request/response/error types: raw byte streams in, unified event streams
//! out. A reqwest-backed implementation ([`ReqwestTransport`]) ships behind
//! the default `reqwest-transport` feature; a subprocess-backed provider
//! (the `aisdk:` shim, phase 9) can implement the same trait without any
//! route-layer rework. NO reqwest types leak into protocol/route signatures.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;

use crate::schema::{
    AiError, AiErrorReason, HttpContext, ReasonFields, TransportDelivery, TransportOperation,
    TransportPhase, TransportRecovery, TransportType,
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

    pub fn with_http(mut self, http: HttpContext) -> Self {
        self.http = Some(http);
        self
    }

    /// Map into the AI error taxonomy.
    pub fn into_ai_error(self) -> AiError {
        AiError::new(AiErrorReason::Transport {
            fields: ReasonFields::new(self.message),
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
#[derive(Debug)]
pub struct TransportResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: ByteStream,
}

impl TransportResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_ascii_lowercase()).map(|v| v.as_str())
    }

    /// HTTP context for diagnostics.
    pub fn http_context(&self, url: &str) -> HttpContext {
        HttpContext { url: url.to_string(), status: self.status, headers: self.headers.clone() }
    }
}

/// The transport seam: executes one prepared request (BINDING — see module
/// docs). Implementations must be usable from async contexts and shareable
/// across threads (`Arc<dyn Transport>`).
#[async_trait]
pub trait Transport: Send + Sync {
    async fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError>;
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
    async fn execute(&self, _request: TransportRequest) -> Result<TransportResponse, TransportError> {
        Err(TransportError::connection(self.message, TransportOperation::Request))
    }
}

/// Reqwest-backed transport behind the default `reqwest-transport` feature.
#[cfg(feature = "reqwest-transport")]
pub mod reqwest_transport {
    use super::*;
    use futures::StreamExt as _;

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
        pub fn new() -> Self {
            Self { client: reqwest::Client::new() }
        }

        pub fn with_client(client: reqwest::Client) -> Self {
            Self { client }
        }
    }

    #[async_trait]
    impl Transport for ReqwestTransport {
        async fn execute(&self, request: TransportRequest) -> Result<TransportResponse, TransportError> {
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|error| TransportError::connection(format!("invalid method: {error}"), TransportOperation::Prepare).with_url(&request.url))?;
            let mut builder = self.client.request(method, &request.url);
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if !request.body.is_empty() {
                builder = builder.body(request.body.clone());
            }
            let response = builder
                .send()
                .await
                .map_err(|error| {
                    let code = error.is_timeout().then_some("Timeout").map(str::to_string);
                    TransportError::connection(describe_reqwest_error(&error), TransportOperation::Request)
                        .with_url(&request.url)
                        .with_code(code.unwrap_or_else(|| "RequestFailed".into()))
                })?;
            let status = response.status().as_u16();
            let headers: BTreeMap<String, String> = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (name.as_str().to_ascii_lowercase(), value.to_str().unwrap_or_default().to_string())
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
