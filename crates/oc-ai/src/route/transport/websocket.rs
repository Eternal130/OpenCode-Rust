//! WebSocket transport path (port of `packages/ai/src/route/transport/
//! websocket.ts` + the channel-executor seam of `websocket-channel.ts`, plus
//! the checkpoint-carrying session channel of `packages/core/src/session/
//! model-transport.ts`).
//!
//! Two executors ship, mirroring v2:
//!
//! * [`make_direct`] — `WebSocketTransport.makeDirect`: every exchange opens
//!   a fresh connection, sends the create message, and streams frames until
//!   the driver observes a terminal observation (`complete` is a no-op —
//!   nothing is staged).
//! * [`SessionChannel`] — the session channel manager: one long-lived socket
//!   per (url + headers) affinity, rotated by age (rotation drops the
//!   continuation state — the next exchange resends in full, like v2),
//!   carrying the OpenResponses continuation checkpoint between exchanges
//!   (`create(checkpoint)` → terminal observation stages → `complete()`
//!   commits), with sticky HTTP fallback on connect failure, ambiguous
//!   sends, oversized frames, and repeated stream losses.
//!
#![allow(
    clippy::doc_markdown, // WebSocket/OpenResponses identifiers in prose stay unquoted
    clippy::type_complexity, // boxed stream/future seams stay explicit
)]

//! Everything is connector-injectable: protocols and tests only see
//! [`WebSocketConnector`] (raw text frames in, unified errors out); a
//! tokio-tungstenite connector ships behind the `ws-transport` feature.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{Stream, StreamExt as _};
use sha2::{Digest, Sha256};

use crate::schema::{
    AiError, AiErrorReason, HttpContext, ReasonFields, TransportDelivery, TransportOperation,
    TransportPhase, TransportRecovery, TransportType,
};

// ---------------------------------------------------------------------------
// Errors (websocket.ts transportError / annotateTransportError)
// ---------------------------------------------------------------------------

/// A websocket transport failure with the v2 taxonomy attached.
pub fn transport_error(
    message: impl Into<String>,
    operation: TransportOperation,
    phase: Option<TransportPhase>,
    delivery: Option<TransportDelivery>,
    code: Option<String>,
    url: Option<String>,
) -> AiError {
    AiError::new(AiErrorReason::Transport {
        fields: ReasonFields::new(message),
        transport: TransportType::Websocket,
        operation,
        code,
        url,
        phase,
        delivery,
        recovery: None,
    })
}

/// Overwrite phase/delivery on an existing transport reason (port of
/// `annotateTransportError` — non-transport reasons pass through).
#[must_use]
pub fn annotate_transport_error(
    error: AiError,
    phase: TransportPhase,
    delivery: TransportDelivery,
) -> AiError {
    let AiErrorReason::Transport { fields, transport, operation, code, url, .. } = error.reason
    else {
        return error;
    };
    AiError::new(AiErrorReason::Transport {
        fields,
        transport,
        operation,
        code,
        url,
        phase: Some(phase),
        delivery: Some(delivery),
        recovery: None,
    })
}

/// Attach a raw frame body to a transport error's fields (the channel
/// executor's `body: frame` annotation on observe failures).
#[must_use]
fn with_body(error: AiError, body: String) -> AiError {
    let AiErrorReason::Transport {
        fields,
        transport,
        operation,
        code,
        url,
        phase,
        delivery,
        recovery,
    } = error.reason
    else {
        return error;
    };
    let fields = ReasonFields { body: Some(body), ..fields };
    AiError::new(AiErrorReason::Transport {
        fields,
        transport,
        operation,
        code,
        url,
        phase,
        delivery,
        recovery,
    })
}

fn is_code(error: &AiError, code: &str) -> bool {
    matches!(&error.reason, AiErrorReason::Transport { code: Some(c), .. } if c == code)
}

// ---------------------------------------------------------------------------
// Connection seam (websocket.ts WebSocketRequest/Connection/Connector)
// ---------------------------------------------------------------------------

/// v2 `MAX_FRAME_BYTES`: a WebSocket message above this fails the stream
/// with `message-too-large` and closes the socket with 1009.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Convert `https:` → `wss:` / `http:` → `ws:`, preserving path and query;
/// JS `URL` normalizes an authority-only URL with a trailing slash. Any other
/// scheme fails with `invalid-url` (`prepare` / `not-sent`).
pub fn to_websocket_url(value: &str) -> Result<String, AiError> {
    let invalid = |message: String| {
        transport_error(
            message,
            TransportOperation::Request,
            Some(TransportPhase::Prepare),
            Some(TransportDelivery::NotSent),
            Some("invalid-url".into()),
            Some(value.to_string()),
        )
    };
    let Some((scheme, rest)) = value.split_once("://") else {
        return Err(invalid("Invalid WebSocket URL".into()));
    };
    let converted = match scheme.to_ascii_lowercase().as_str() {
        "https" => "wss",
        "http" => "ws",
        other => return Err(invalid(format!("Unsupported WebSocket URL protocol {other}:"))),
    };
    let mut url = format!("{converted}://{rest}");
    // authority without path or query gets the trailing slash JS `URL` adds
    if !rest.contains('/') {
        url.push('/');
    }
    Ok(url)
}

/// One outbound open request: URL + handshake headers.
#[derive(Debug, Clone)]
pub struct WebSocketRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

/// One inbound socket message (text or binary).
#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
}

impl WebSocketMessage {
    /// Byte length of the payload (v2 `oversized` check).
    pub fn len(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Binary(bytes) => bytes.len(),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// v2 `messageText`: text passes through; binary decodes as UTF-8
    /// (lossy, like `TextDecoder`).
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Binary(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

pub type CloseFuture = Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
pub type SendFuture = Pin<Box<dyn std::future::Future<Output = Result<(), AiError>> + Send>>;
pub type SendFn = Arc<dyn Fn(String) -> SendFuture + Send + Sync>;
pub type CloseFn = Arc<dyn Fn(u16, &str) -> CloseFuture + Send + Sync>;

/// An open socket (port of `WebSocketConnection`): send + inbound message
/// stream + close with a code. The inbound stream ends after a clean close;
/// socket failures and close frames surface as `Err` items.
pub struct WebSocketConnection {
    pub http: Option<HttpContext>,
    pub send_text: SendFn,
    pub messages: Pin<Box<dyn Stream<Item = Result<WebSocketMessage, AiError>> + Send>>,
    pub close: CloseFn,
}

impl std::fmt::Debug for WebSocketConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WebSocketConnection(<live>)")
    }
}

/// Opens connections (port of `WebSocketConnector`).
#[async_trait]
pub trait WebSocketConnector: Send + Sync {
    async fn open(&self, request: WebSocketRequest) -> Result<WebSocketConnection, AiError>;
}

/// Enforce the 16 MiB frame limit on an inbound message stream (port of
/// `fromWebSocket`'s `rejectOversized`): an oversized message closes the
/// socket with 1009 "Message too large" and fails the stream with
/// `message-too-large` (`receive` phase).
pub fn frame_limited_messages(
    mut messages: Pin<Box<dyn Stream<Item = Result<WebSocketMessage, AiError>> + Send>>,
    url: String,
    close: CloseFn,
) -> Pin<Box<dyn Stream<Item = Result<WebSocketMessage, AiError>> + Send>> {
    Box::pin(async_stream::stream! {
        while let Some(item) = messages.next().await {
            match item {
                Ok(message) if message.len() > MAX_FRAME_BYTES => {
                    let _body = message.text();
                    close(1009, "Message too large").await;
                    yield Err(transport_error(
                        "WebSocket message exceeds the 16 MiB limit",
                        TransportOperation::Read,
                        Some(TransportPhase::Receive),
                        None,
                        Some("message-too-large".into()),
                        Some(url.clone()),
                    ));
                    return;
                }
                other => yield other,
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Channel seam (websocket-channel.ts)
// ---------------------------------------------------------------------------

/// Whether a create message is the whole request or an incremental step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    Full,
    Incremental,
}

/// The message a driver asks the executor to send (port of `ChannelCreate`).
#[derive(Debug, Clone)]
pub struct ChannelCreate {
    pub message: String,
    pub mode: ChannelMode,
}

/// Opaque continuation state staged by a completed exchange (port of
/// `ChannelCheckpoint`).
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelCheckpoint {
    pub protocol: String,
    pub value: serde_json::Value,
}

/// What a driver made of one inbound frame (port of `ChannelObservation`).
#[derive(Debug)]
pub enum ChannelObservation {
    Frame { frame: String },
    Completed { frame: String, checkpoint: Option<ChannelCheckpoint> },
    Incomplete { frame: String },
    ProviderFailure { error: AiError },
    Rejected { error: AiError, recovery: TransportRecovery },
}

impl ChannelObservation {
    #[must_use]
    pub fn frame(&self) -> &str {
        match self {
            Self::Frame { frame } | Self::Completed { frame, .. } | Self::Incomplete { frame } => {
                frame
            }
            Self::ProviderFailure { .. } | Self::Rejected { .. } => "",
        }
    }

    #[must_use]
    pub fn is_frame(&self) -> bool {
        matches!(self, Self::Frame { .. })
    }

    #[must_use]
    pub fn is_terminal(&self) -> bool {
        !self.is_frame()
    }
}

/// Protocol-specific channel behavior (port of `WebSocketChannelDriver`).
/// `create` runs under the channel lock before the socket is written;
/// `observe` classifies each inbound frame.
pub trait WebSocketChannelDriver: Send + Sync {
    fn create(&self, checkpoint: Option<&ChannelCheckpoint>) -> Result<ChannelCreate, AiError>;
    fn observe(&self, create: &ChannelCreate, frame: &str) -> Result<ChannelObservation, AiError>;
}

/// Connection selection for one exchange (port of `WebSocketChannelExchange`,
/// minus the interceptor seam, which stays with callers).
#[derive(Debug, Clone)]
pub struct ChannelConnect {
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Provider-safe connection age after which to reconnect before sending.
    pub rotate_after_ms: Option<u64>,
}

pub type RawFrameStream = Pin<Box<dyn Stream<Item = Result<String, AiError>> + Send>>;
pub type FallbackFuture =
    Pin<Box<dyn std::future::Future<Output = Result<RawFrameStream, AiError>> + Send>>;
pub type FallbackFn = Arc<dyn Fn() -> FallbackFuture + Send + Sync>;

/// One exchange handed to a channel executor.
pub struct WebSocketChannelExchange {
    pub id: String,
    pub connect: ChannelConnect,
    /// HTTP path for this exchange (SSE frames) when the socket is unusable.
    pub fallback: FallbackFn,
    pub driver: Arc<dyn WebSocketChannelDriver>,
}

/// Commits staged state after the decoded stream ends successfully; returns
/// the committed checkpoint so callers (the session runner, tests) can
/// observe the continuation state. v2's `complete` commits the same
/// transition inside the channel and returns void.
pub type CompleteFn = Arc<dyn Fn() -> Option<ChannelCheckpoint> + Send + Sync>;

/// A running channel exchange (port of `WebSocketChannelExecution`).
pub struct WebSocketChannelExecution {
    pub frames: RawFrameStream,
    pub http: Option<HttpContext>,
    pub complete: CompleteFn,
}

fn noop_complete() -> CompleteFn {
    Arc::new(|| None)
}

/// The frame of an observation; terminal observations carrying a frame
/// (completed/incomplete) pass it through, failures raise their error (port
/// of `observationFrame`).
fn observation_frame(observation: ChannelObservation) -> Result<String, AiError> {
    match observation {
        ChannelObservation::Frame { frame }
        | ChannelObservation::Completed { frame, .. }
        | ChannelObservation::Incomplete { frame } => Ok(frame),
        ChannelObservation::ProviderFailure { error } => Err(error),
        ChannelObservation::Rejected { error, .. } => Err(error),
    }
}

async fn fallback_execution(
    exchange: &WebSocketChannelExchange,
) -> Result<WebSocketChannelExecution, AiError> {
    let frames = (exchange.fallback)().await?;
    Ok(WebSocketChannelExecution { frames, http: None, complete: noop_complete() })
}

/// Executes channel exchanges (port of `WebSocketChannelExecutor`).
#[async_trait]
pub trait WebSocketChannelExecutor: Send + Sync {
    async fn execute(
        &self,
        exchange: WebSocketChannelExchange,
    ) -> Result<WebSocketChannelExecution, AiError>;
}

impl ChannelConnect {
    fn into_request(self) -> WebSocketRequest {
        WebSocketRequest { url: self.url, headers: self.headers }
    }
}

// ---------------------------------------------------------------------------
// Direct executor (websocket.ts makeDirect)
// ---------------------------------------------------------------------------

/// `WebSocketTransport.makeDirect`: each exchange opens a connection, sends
/// the driver's create message (with `checkpoint = None` — the direct
/// executor carries no continuation state), and streams driver-observed
/// frames until the first terminal observation.
#[must_use]
pub fn make_direct(
    connector: Arc<dyn WebSocketConnector>,
) -> Arc<dyn WebSocketChannelExecutor + Send + Sync> {
    Arc::new(DirectChannelExecutor { connector })
}

struct DirectChannelExecutor {
    connector: Arc<dyn WebSocketConnector>,
}

#[async_trait]
impl WebSocketChannelExecutor for DirectChannelExecutor {
    async fn execute(
        &self,
        exchange: WebSocketChannelExchange,
    ) -> Result<WebSocketChannelExecution, AiError> {
        let connection = self
            .connector
            .open(exchange.connect.clone().into_request())
            .await
            .map_err(|error| {
                annotate_transport_error(error, TransportPhase::Connect, TransportDelivery::NotSent)
            })?;
        let create = exchange.driver.create(None)?;
        let connection_http = connection.http.clone();
        if let Err(mut error) = (connection.send_text)(create.message.clone()).await {
            if matches!(error.reason, AiErrorReason::Transport { .. }) {
                error = annotate_transport_error(
                    error,
                    TransportPhase::Send,
                    TransportDelivery::NotSent,
                );
                if let AiErrorReason::Transport { fields, .. } = &mut error.reason {
                    fields.http = fields.http.clone().or_else(|| connection_http.clone());
                }
            }
            return Err(error);
        }

        let driver = Arc::clone(&exchange.driver);
        let url = exchange.connect.url.clone();
        let mut messages =
            frame_limited_messages(connection.messages, url.clone(), Arc::clone(&connection.close));
        let mut observed = false;
        let stream_http = connection_http.clone();
        let frames: RawFrameStream = Box::pin(async_stream::stream! {
            loop {
                let Some(message) = messages.next().await else { return };
                let message = match message {
                    Ok(message) => message,
                    Err(error) => {
                        let phase = matches!(
                            &error.reason,
                            AiErrorReason::Transport { phase: Some(TransportPhase::Close), .. }
                        )
                        .then_some(TransportPhase::Close)
                        .unwrap_or(TransportPhase::Receive);
                        let delivery = if observed {
                            TransportDelivery::Accepted
                        } else {
                            TransportDelivery::Ambiguous
                        };
                        let mut error = annotate_transport_error(error, phase, delivery);
                        if let AiErrorReason::Transport { fields, .. } = &mut error.reason {
                            fields.http = fields.http.clone().or_else(|| stream_http.clone());
                        }
                        yield Err(error);
                        return;
                    }
                };
                observed = true;
                let frame = message.text();
                match driver.observe(&create, &frame) {
                    Ok(observation) if observation.is_terminal() => {
                        match observation_frame(observation) {
                            Ok(frame) => yield Ok(frame),
                            Err(error) => yield Err(annotate_body(error, frame)),
                        }
                        return;
                    }
                    Ok(observation) => {
                        yield Ok(observation.frame().to_string());
                    }
                    Err(error) => {
                        yield Err(annotate_body(error, frame));
                        return;
                    }
                }
            }
        });
        Ok(WebSocketChannelExecution { frames, http: connection_http, complete: noop_complete() })
    }
}

/// The channel executor's `body: frame` annotation (observe failures).
fn annotate_body(error: AiError, frame: String) -> AiError {
    if matches!(error.reason, AiErrorReason::Transport { .. }) {
        with_body(error, frame)
    } else {
        error
    }
}

// ---------------------------------------------------------------------------
// Session channel (core/src/session/model-transport.ts)
// ---------------------------------------------------------------------------

/// Default connection age after which the session channel rotates the
/// socket before sending (v2 `ROTATE_AFTER_MS`, 55 minutes).
pub const DEFAULT_ROTATE_AFTER_MS: u64 = 55 * 60 * 1000;

/// Default wait for the WebSocket handshake (v2 `CONNECT_TIMEOUT`).
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Default idle wait between inbound frames. Reasoning models can stream
/// nothing for several minutes while still working (v2 `IDLE_TIMEOUT`,
/// 30 minutes).
#[expect(clippy::duration_suboptimal_units, reason = "30 minutes reads as 30*60 seconds")]
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Consecutive exchanges lost to the socket before the session channel
/// stays on HTTP (v2 `MAX_STREAM_FAILURES`).
pub const MAX_STREAM_FAILURES: u32 = 5;

/// Idle wait between inbound frames (`idleTimeout?: number | false`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IdleTimeout {
    /// 30 minutes (v2 `IDLE_TIMEOUT`).
    #[default]
    Default,
    /// `false` — disables the idle budget entirely.
    Disabled,
    Limited(Duration),
}

impl IdleTimeout {
    fn get(self) -> Option<Duration> {
        match self {
            Self::Default => Some(DEFAULT_IDLE_TIMEOUT),
            Self::Disabled => None,
            Self::Limited(duration) => Some(duration),
        }
    }
}

/// Session channel tuning (v2 constants; every field has the v2 default).
#[derive(Debug, Clone)]
pub struct SessionChannelConfig {
    pub rotate_after_ms: Option<u64>,
    pub connect_timeout: Option<Duration>,
    pub idle_timeout: IdleTimeout,
}

impl Default for SessionChannelConfig {
    fn default() -> Self {
        Self { rotate_after_ms: None, connect_timeout: None, idle_timeout: IdleTimeout::Default }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    SendAttempted,
    ProviderObserved,
    Terminal,
}

/// Continuation state for the live socket (v2 `channel.checkpoint` /
/// `channel.pending`). Rotation starts a fresh cell — like v2's fresh
/// `Channel` object, the next exchange resends in full. Sync-mutex'd so the
/// `complete` commit stays a synchronous closure.
#[derive(Debug, Default)]
struct ContinuationCell {
    slot: u64,
    checkpoint: Option<ChannelCheckpoint>,
    pending: Option<(usize, ChannelCheckpoint)>,
}

struct ActiveExchange {
    sender: tokio::sync::mpsc::UnboundedSender<Result<String, AiError>>,
    delivery: Arc<std::sync::Mutex<Delivery>>,
}

struct ChannelSlot {
    id: u64,
    affinity: String,
    send_text: SendFn,
    close: CloseFn,
    http: Option<HttpContext>,
    opened_at: std::time::Instant,
    closing: bool,
    active: Option<ActiveExchange>,
}

impl ChannelSlot {
    async fn close_socket(&self) {
        (self.close)(1000, "").await;
    }
}

struct OwnerState {
    closed: bool,
    http_fallback: bool,
    stream_failures: u32,
    channel: Option<ChannelSlot>,
    next_channel_id: u64,
    next_token: usize,
}

/// The per-session channel executor (port of `SessionModelTransport`'s bound
/// executor): one socket reused across exchanges (affinity = url + sorted
/// handshake headers), rotated by age, carrying the continuation checkpoint.
/// Exchanges are serialized (v2's semaphore); the socket dies on protocol
/// violations (poison) and the session falls back to HTTP after repeated
/// stream losses or refused connects.
pub struct SessionChannel {
    connector: Arc<dyn WebSocketConnector>,
    config: SessionChannelConfig,
    state: tokio::sync::Mutex<OwnerState>,
    continuation: Arc<std::sync::Mutex<ContinuationCell>>,
    weak: std::sync::Weak<SessionChannel>,
}

impl SessionChannel {
    /// Build a session channel (always held through `Arc`).
    #[must_use]
    pub fn new(connector: Arc<dyn WebSocketConnector>, config: SessionChannelConfig) -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            connector,
            config,
            state: tokio::sync::Mutex::new(OwnerState {
                closed: false,
                http_fallback: false,
                stream_failures: 0,
                channel: None,
                next_channel_id: 0,
                next_token: 0,
            }),
            continuation: Arc::new(std::sync::Mutex::new(ContinuationCell::default())),
            weak: std::sync::Weak::clone(weak),
        })
    }

    /// Close the socket and refuse further exchanges (v2 `close`).
    pub async fn close(&self) {
        let mut state = self.state.lock().await;
        state.closed = true;
        if let Some(mut channel) = state.channel.take() {
            channel.closing = true;
            fail_active(
                &mut channel,
                transport_error(
                    "Session WebSocket closed",
                    TransportOperation::Read,
                    Some(TransportPhase::Close),
                    None,
                    Some("close".into()),
                    None,
                ),
            );
            channel.close_socket().await;
        }
    }

    /// Whether the session pinned itself to HTTP (connect failures, stream
    /// losses, oversized requests) — v2 `httpFallback`.
    #[must_use]
    pub fn is_http_fallback(&self) -> bool {
        match self.state.try_lock() {
            Ok(state) => state.http_fallback,
            Err(_) => false,
        }
    }
}

/// Fail the active exchange queue with `error` (v2 `Queue.failCauseUnsafe`).
fn fail_active(slot: &mut ChannelSlot, error: AiError) {
    if let Some(active) = slot.active.take() {
        let _ = active.sender.send(Err(error));
    }
}

/// A socket that keeps dying mid-exchange costs a retry every step; after
/// enough consecutive losses the session stays on HTTP.
fn stream_failure(state: &mut OwnerState) {
    state.stream_failures += 1;
    if state.stream_failures >= MAX_STREAM_FAILURES {
        state.http_fallback = true;
    }
}

/// Affinity key: url + a hash of the sorted handshake headers (v2 `affinity`)
/// — changed headers reopen the socket.
fn affinity_key(url: &str, headers: &[(String, String)]) -> String {
    let mut headers: Vec<&(String, String)> = headers.iter().collect();
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    let rendered =
        headers.iter().map(|(name, value)| format!("{name}:{value}")).collect::<Vec<_>>().join(",");
    format!("{url}:{:x}", Sha256::digest(rendered.as_bytes()))
}

#[async_trait]
impl WebSocketChannelExecutor for SessionChannel {
    async fn execute(
        &self,
        exchange: WebSocketChannelExchange,
    ) -> Result<WebSocketChannelExecution, AiError> {
        let this = self.weak.upgrade().expect("session channel outlives its exchanges");
        let mut state = this.state.lock().await;
        if state.closed {
            return Err(transport_error(
                "Session WebSocket owner is closed",
                TransportOperation::Request,
                Some(TransportPhase::Queue),
                Some(TransportDelivery::NotSent),
                Some("owner-closed".into()),
                Some(exchange.connect.url.clone()),
            ));
        }
        if state.http_fallback {
            return fallback_execution(&exchange).await;
        }
        {
            // Rotation by affinity or age, then reuse or open.
            let key = affinity_key(&exchange.connect.url, &exchange.connect.headers);
            let rotate_after = Duration::from_millis(
                exchange
                    .connect
                    .rotate_after_ms
                    .or(this.config.rotate_after_ms)
                    .unwrap_or(DEFAULT_ROTATE_AFTER_MS),
            );
            let rotate = state.channel.as_ref().is_some_and(|current| {
                current.affinity != key || current.opened_at.elapsed() >= rotate_after
            });
            if rotate {
                if let Some(mut channel) = state.channel.take() {
                    channel.closing = true;
                    channel.close_socket().await;
                }
            }
            if state.channel.is_none() {
                match open_slot(&this, &exchange, &mut state).await {
                    Ok(()) => {}
                    Err(error) => {
                        if is_code(&error, "owner-closed") {
                            return Err(error);
                        }
                        // Any connect failure pins the session to HTTP: a
                        // network refusing the upgrade would otherwise charge
                        // every step for a failed connect.
                        state.http_fallback = true;
                        return fallback_execution(&exchange).await;
                    }
                }
            }
        }

        let channel_id = state.channel.as_ref().expect("slot present").id;
        let channel_http = state.channel.as_ref().expect("slot present").http.clone();

        // A staged-but-uncommitted checkpoint is dropped with its baseline.
        {
            let mut continuation = lock_continuation(&this);
            if continuation.slot == channel_id && continuation.pending.is_some() {
                continuation.pending = None;
                continuation.checkpoint = None;
            }
        }

        let checkpoint = {
            let continuation = lock_continuation(&this);
            (continuation.slot == channel_id).then(|| continuation.checkpoint.clone()).flatten()
        };
        let create = match exchange.driver.create(checkpoint.as_ref()) {
            Ok(create) => create,
            Err(error) => {
                let mut slot = state.channel.take().expect("slot present");
                slot.closing = true;
                slot.close_socket().await;
                return Err(error);
            }
        };
        if create.mode == ChannelMode::Full {
            lock_continuation(&this).checkpoint = None;
        }

        let token = state.next_token;
        state.next_token += 1;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let delivery = Arc::new(std::sync::Mutex::new(Delivery::SendAttempted));
        let send_text = {
            let slot = state.channel.as_mut().expect("slot present");
            slot.active = Some(ActiveExchange { sender, delivery: Arc::clone(&delivery) });
            Arc::clone(&slot.send_text)
        };

        let sent =
            tokio::time::timeout(Duration::from_secs(30), (send_text)(create.message.clone()))
                .await;
        let sent = match sent {
            Ok(result) => result,
            Err(_elapsed) => Err(transport_error(
                "Timed out sending the create message",
                TransportOperation::Write,
                Some(TransportPhase::Send),
                Some(TransportDelivery::NotSent),
                Some("send-timeout".into()),
                Some(exchange.connect.url.clone()),
            )),
        };
        if let Err(failure) = sent {
            let not_sent = matches!(
                &failure.reason,
                AiErrorReason::Transport { delivery: Some(TransportDelivery::NotSent), .. }
            );
            let mut slot = state.channel.take().expect("slot present");
            slot.closing = true;
            slot.close_socket().await;
            if not_sent {
                return fallback_execution(&exchange).await;
            }
            stream_failure(&mut state);
            return Err(annotate_transport_error(
                failure,
                TransportPhase::Send,
                TransportDelivery::Ambiguous,
            ));
        }
        drop(state);

        // The frames stream and the complete closure each hold their own
        // reference; `this` itself is moved into the stream.
        let complete_this = Arc::clone(&this);
        let url = exchange.connect.url.clone();
        let driver = Arc::clone(&exchange.driver);
        let idle_timeout = this.config.idle_timeout.get();
        let fallback = Arc::clone(&exchange.fallback);
        let mut receiver = receiver;
        let frames: RawFrameStream = Box::pin(async_stream::stream! {
            loop {
                let item = match idle_timeout {
                    Some(wait) => match tokio::time::timeout(wait, receiver.recv()).await {
                        Ok(item) => item,
                        Err(_elapsed) => {
                            let observed = *delivery_lock(&delivery) == Delivery::ProviderObserved;
                            let error = transport_error(
                                "Timed out waiting for WebSocket data",
                                TransportOperation::Read,
                                Some(TransportPhase::Receive),
                                Some(if observed { TransportDelivery::Accepted } else { TransportDelivery::Ambiguous }),
                                Some("idle-timeout".into()),
                                Some(url.clone()),
                            );
                            yield Err(error);
                            not_reached_terminal(&this, channel_id).await;
                            return;
                        }
                    },
                    None => receiver.recv().await,
                };
                // all senders dropped without a failure — the channel
                // was closed underneath the exchange
                let Some(item) = item else {
                    not_reached_terminal(&this, channel_id).await;
                    return;
                };
                let frame = match item {
                    Ok(frame) => frame,
                    Err(error) => {
                        // Server refused an oversized request (close 1009):
                        // stay on HTTP from now on and retry this exchange.
                        if is_code(&error, "1009")
                            && matches!(
                                &error.reason,
                                AiErrorReason::Transport {
                                    delivery: Some(TransportDelivery::Rejected),
                                    ..
                                }
                            )
                        {
                            {
                                let mut state = this.state.lock().await;
                                state.http_fallback = true;
                            }
                            match (fallback)().await {
                                Ok(mut frames) => {
                                    while let Some(frame) = frames.next().await {
                                        yield frame;
                                    }
                                    return;
                                }
                                Err(error) => {
                                    yield Err(error);
                                    return;
                                }
                            }
                        }
                        yield Err(error);
                        not_reached_terminal(&this, channel_id).await;
                        return;
                    }
                };
                let observation = match driver.observe(&create, &frame) {
                    Ok(observation) => observation,
                    Err(error) => {
                        yield Err(annotate_body(error, frame));
                        not_reached_terminal(&this, channel_id).await;
                        return;
                    }
                };
                if observation.is_terminal() {
                    *delivery_lock(&delivery) = Delivery::Terminal;
                    let staged =
                        if let ChannelObservation::Completed { checkpoint, .. } = &observation {
                            checkpoint.clone()
                        } else {
                            None
                        };
                    {
                        // v2 tap: stage completed checkpoints; clear otherwise
                        let mut continuation = lock_continuation(&this);
                        if continuation.slot == channel_id {
                            if let Some(checkpoint) = staged {
                                continuation.pending = Some((token, checkpoint));
                            } else {
                                continuation.checkpoint = None;
                                continuation.pending = None;
                            }
                        }
                    }
                    let outcome = observation_frame(observation);
                    terminal_cleanup(&this, channel_id, &mut receiver, &outcome).await;
                    match outcome {
                        Ok(frame) => yield Ok(frame),
                        Err(error) => yield Err(error),
                    }
                    return;
                }
                yield Ok(frame);
            }
        });
        let complete: CompleteFn = {
            let this = complete_this;
            Arc::new(move || {
                let mut continuation = lock_continuation(&this);
                if continuation.slot != channel_id {
                    return None;
                }
                if let Some((pending_token, checkpoint)) = continuation.pending.take() {
                    if pending_token == token {
                        continuation.checkpoint = Some(checkpoint.clone());
                        return Some(checkpoint);
                    }
                }
                None
            })
        };
        Ok(WebSocketChannelExecution { frames, http: channel_http, complete })
    }
}

fn lock_continuation(this: &Arc<SessionChannel>) -> std::sync::MutexGuard<'_, ContinuationCell> {
    match this.continuation.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn delivery_lock(
    delivery: &Arc<std::sync::Mutex<Delivery>>,
) -> std::sync::MutexGuard<'_, Delivery> {
    match delivery.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

async fn open_slot(
    this: &Arc<SessionChannel>,
    exchange: &WebSocketChannelExchange,
    state: &mut OwnerState,
) -> Result<(), AiError> {
    let timeout = this.config.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT);
    let opened =
        tokio::time::timeout(timeout, this.connector.open(exchange.connect.clone().into_request()))
            .await;
    let connection = match opened {
        Ok(Ok(connection)) => connection,
        Ok(Err(error)) => {
            return Err(annotate_transport_error(
                error,
                TransportPhase::Connect,
                TransportDelivery::NotSent,
            ));
        }
        Err(_elapsed) => {
            return Err(transport_error(
                "Timed out opening the Session WebSocket",
                TransportOperation::Request,
                Some(TransportPhase::Connect),
                Some(TransportDelivery::NotSent),
                Some("connect-timeout".into()),
                Some(exchange.connect.url.clone()),
            ));
        }
    };
    let WebSocketConnection { http, send_text, messages, close } = connection;
    if state.closed {
        close(1000, "").await;
        return Err(transport_error(
            "Session WebSocket owner closed while connecting",
            TransportOperation::Request,
            Some(TransportPhase::Connect),
            Some(TransportDelivery::NotSent),
            Some("owner-closed".into()),
            None,
        ));
    }
    let channel_id = state.next_channel_id;
    state.next_channel_id += 1;
    let url = exchange.connect.url.clone();
    let affinity = affinity_key(&url, &exchange.connect.headers);
    let slot = ChannelSlot {
        id: channel_id,
        affinity,
        send_text,
        close: Arc::clone(&close),
        http,
        opened_at: std::time::Instant::now(),
        closing: false,
        active: None,
    };
    // Rotation drops the continuation baseline (fresh Channel in v2).
    *lock_continuation(this) =
        ContinuationCell { slot: channel_id, checkpoint: None, pending: None };
    spawn_reader(Arc::clone(this), channel_id, url, messages, close);
    state.channel = Some(slot);
    Ok(())
}

/// The socket reader (v2 `channel.reader`): every inbound text frame goes to
/// the active exchange's queue; data without an active exchange, binary
/// frames, or socket failures poison the channel.
fn spawn_reader(
    this: Arc<SessionChannel>,
    channel_id: u64,
    url: String,
    messages: Pin<Box<dyn Stream<Item = Result<WebSocketMessage, AiError>> + Send>>,
    close: CloseFn,
) {
    tokio::spawn(async move {
        let mut messages = frame_limited_messages(messages, url.clone(), Arc::clone(&close));
        while let Some(item) = messages.next().await {
            let message = match item {
                Ok(message) => message,
                Err(error) => {
                    let error = {
                        let state = this.state.lock().await;
                        let closing = state
                            .channel
                            .as_ref()
                            .is_some_and(|slot| slot.id == channel_id && slot.closing);
                        if closing {
                            return;
                        }
                        let phase = matches!(
                            &error.reason,
                            AiErrorReason::Transport { phase: Some(TransportPhase::Close), .. }
                        )
                        .then_some(TransportPhase::Close)
                        .unwrap_or(TransportPhase::Receive);
                        let delivery = match &error.reason {
                            AiErrorReason::Transport { code: Some(code), .. } if code == "1009" => {
                                TransportDelivery::Rejected
                            }
                            AiErrorReason::Transport {
                                delivery: Some(TransportDelivery::Accepted),
                                ..
                            } => TransportDelivery::Accepted,
                            _ => TransportDelivery::Ambiguous,
                        };
                        drop(state);
                        annotate_transport_error(error, phase, delivery)
                    };
                    poison(&this, channel_id, error).await;
                    continue;
                }
            };
            let outcome = {
                let mut state = this.state.lock().await;
                let Some(slot) = state.channel.as_mut() else { return };
                if slot.id != channel_id {
                    return; // replaced by a newer socket
                }
                if matches!(message, WebSocketMessage::Binary(_)) {
                    Err(transport_error(
                        "Unsupported binary WebSocket frame",
                        TransportOperation::Read,
                        Some(TransportPhase::Receive),
                        None,
                        Some("message".into()),
                        Some(url.clone()),
                    ))
                } else if slot.active.is_none() {
                    Err(transport_error(
                        "WebSocket data arrived without an active exchange",
                        TransportOperation::Read,
                        Some(TransportPhase::Receive),
                        None,
                        Some("idle-data".into()),
                        Some(url.clone()),
                    ))
                } else {
                    let active = slot.active.as_ref().expect("checked");
                    *delivery_lock(&active.delivery) = Delivery::ProviderObserved;
                    let _ = active.sender.send(Ok(message.text()));
                    Ok(())
                }
            };
            if let Some(error) = outcome.err() {
                poison(&this, channel_id, error).await;
            }
        }
    });
}

/// v2 `poison`: the socket violated the channel protocol — fail the active
/// exchange, count the stream loss, close the socket.
async fn poison(this: &Arc<SessionChannel>, channel_id: u64, error: AiError) {
    let mut state = this.state.lock().await;
    let Some(slot) = state.channel.as_mut() else { return };
    if slot.id != channel_id || slot.closing {
        return;
    }
    slot.closing = true;
    fail_active(slot, error);
    stream_failure(&mut state);
    let slot = state.channel.take().expect("slot present");
    drop(state);
    slot.close_socket().await;
}

/// v2 `Stream.ensuring` cancellation path: the exchange never reached a
/// terminal observation — clear the continuation baseline and poison the
/// socket so a half-consumed response cannot corrupt the next exchange.
async fn not_reached_terminal(this: &Arc<SessionChannel>, channel_id: u64) {
    {
        let mut continuation = lock_continuation(this);
        if continuation.slot == channel_id {
            continuation.checkpoint = None;
            continuation.pending = None;
        }
    }
    let error = transport_error(
        "Session WebSocket exchange did not reach a terminal event",
        TransportOperation::Read,
        Some(TransportPhase::Receive),
        None,
        Some("incomplete".into()),
        None,
    );
    poison(this, channel_id, error).await;
}

/// v2 `Stream.ensuring` terminal path: a drained queue keeps the staged
/// checkpoint for `complete()` and resets the stream-loss counter; the
/// Codex backend stops serving a connection after any error frame, so
/// non-completed terminals drop the socket.
async fn terminal_cleanup(
    this: &Arc<SessionChannel>,
    channel_id: u64,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<Result<String, AiError>>,
    outcome: &Result<String, AiError>,
) {
    let drained = receiver.try_recv().is_err();
    let mut state = this.state.lock().await;
    let Some(slot) = state.channel.as_mut() else { return };
    if slot.id != channel_id {
        return;
    }
    slot.active = None;
    if !drained {
        // frames queued behind the terminal observation
        {
            let mut continuation = lock_continuation(this);
            if continuation.slot == channel_id {
                continuation.checkpoint = None;
                continuation.pending = None;
            }
        }
        fail_active(
            slot,
            transport_error(
                "WebSocket data arrived after the terminal event",
                TransportOperation::Read,
                Some(TransportPhase::Receive),
                Some(TransportDelivery::Accepted),
                Some("idle-data".into()),
                None,
            ),
        );
        stream_failure(&mut state);
        return;
    }
    state.stream_failures = 0;
    if outcome.is_err() {
        // provider-failure / rejected: drop the socket after error frames
        let mut slot = state.channel.take().expect("slot present");
        slot.closing = true;
        drop(state);
        slot.close_socket().await;
    }
}

/// Session-bound channel set (v2 `SessionModelTransport.Service`):
/// one [`SessionChannel`] per session id, closed individually or all at
/// once (the production consumer is the session runner's transport lane).
pub struct SessionChannels {
    connector: Arc<dyn WebSocketConnector>,
    config: SessionChannelConfig,
    channels: std::sync::Mutex<HashMap<String, Arc<SessionChannel>>>,
}

impl SessionChannels {
    #[must_use]
    pub fn new(connector: Arc<dyn WebSocketConnector>, config: SessionChannelConfig) -> Arc<Self> {
        Arc::new(Self { connector, config, channels: std::sync::Mutex::new(HashMap::new()) })
    }

    /// The channel executor bound to one session (v2 `bind`).
    #[must_use]
    pub fn bind(self: &Arc<Self>, session_id: &str) -> Arc<SessionChannel> {
        let mut channels = match self.channels.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        channels
            .entry(session_id.to_string())
            .or_insert_with(|| {
                SessionChannel::new(Arc::clone(&self.connector), self.config.clone())
            })
            .clone()
    }

    /// Close one session's channel (v2 `close`).
    pub async fn close(&self, session_id: &str) {
        let channel = {
            let mut channels = match self.channels.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            channels.remove(session_id)
        };
        if let Some(channel) = channel {
            channel.close().await;
        }
    }

    /// Close every session channel (v2 `closeAll`).
    pub async fn close_all(&self) {
        let drained: Vec<Arc<SessionChannel>> = {
            let mut channels = match self.channels.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            channels.drain().map(|(_id, channel)| channel).collect()
        };
        for channel in drained {
            channel.close().await;
        }
    }
}

/// The production session-channel owner (v2 `SessionModelTransport.node`'s
/// global layer): the tokio-tungstenite connector when the `ws-transport`
/// feature is on; without it every connect fails and each exchange's
/// built-in HTTP SSE fallback carries the stream.
#[must_use]
pub fn default_session_channels() -> Arc<SessionChannels> {
    SessionChannels::new(default_connector(), SessionChannelConfig::default())
}

#[cfg(feature = "ws-transport")]
fn default_connector() -> Arc<dyn WebSocketConnector> {
    Arc::new(tungstenite_connector::TokioTungsteniteConnector)
}

#[cfg(not(feature = "ws-transport"))]
fn default_connector() -> Arc<dyn WebSocketConnector> {
    Arc::new(UnavailableConnector)
}

/// Connect-failing stand-in for builds without `ws-transport`.
#[cfg(not(feature = "ws-transport"))]
struct UnavailableConnector;

#[cfg(not(feature = "ws-transport"))]
#[async_trait]
impl WebSocketConnector for UnavailableConnector {
    async fn open(&self, request: WebSocketRequest) -> Result<WebSocketConnection, AiError> {
        Err(transport_error(
            "WebSocket support is not compiled in",
            TransportOperation::Request,
            Some(TransportPhase::Connect),
            Some(TransportDelivery::NotSent),
            Some("unavailable".into()),
            Some(request.url),
        ))
    }
}

// ---------------------------------------------------------------------------
// tokio-tungstenite connector (feature `ws-transport`)
// ---------------------------------------------------------------------------

#[cfg(feature = "ws-transport")]
pub mod tungstenite_connector {
    //! Production connector over `tokio-tungstenite` (`Socket.WebSocketConstructor`
    //! in v2): handshakes with caller headers, splits the socket into a shared
    //! sink (send/close) and a message stream mapping close frames onto the v2
    //! transport-error taxonomy.

    use super::{
        annotate_transport_error, transport_error, CloseFn, SendFn, WebSocketConnection,
        WebSocketConnector, WebSocketMessage, WebSocketRequest,
    };
    use crate::schema::{AiError, TransportDelivery, TransportOperation, TransportPhase};
    use async_trait::async_trait;
    use futures::{SinkExt as _, StreamExt as _};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_tungstenite::tungstenite::{
        client::IntoClientRequest, handshake::client::generate_key, http, Message,
    };

    /// Connector over tokio-tungstenite (ws:// and wss://).
    #[derive(Debug, Default)]
    pub struct TokioTungsteniteConnector;

    #[async_trait]
    impl WebSocketConnector for TokioTungsteniteConnector {
        async fn open(&self, request: WebSocketRequest) -> Result<WebSocketConnection, AiError> {
            let uri: http::Uri = request.url.parse().map_err(|error| {
                transport_error(
                    format!("Invalid WebSocket URL: {error}"),
                    TransportOperation::Request,
                    Some(TransportPhase::Connect),
                    Some(TransportDelivery::NotSent),
                    Some("invalid-url".into()),
                    Some(request.url.clone()),
                )
            })?;
            let authority =
                uri.authority().map(|authority| authority.as_str().to_string()).unwrap_or_default();
            let mut builder = http::Request::builder()
                .method("GET")
                .uri(uri)
                .header("host", authority)
                .header("connection", "Upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", generate_key());
            for (name, value) in &request.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            let handshake = builder.body(()).map_err(|error| {
                transport_error(
                    format!("Failed to construct WebSocket request: {error}"),
                    TransportOperation::Request,
                    Some(TransportPhase::Connect),
                    Some(TransportDelivery::NotSent),
                    None,
                    Some(request.url.clone()),
                )
            })?;
            let handshake = handshake.into_client_request().map_err(|error| {
                transport_error(
                    format!("Failed to construct WebSocket request: {error}"),
                    TransportOperation::Request,
                    Some(TransportPhase::Connect),
                    Some(TransportDelivery::NotSent),
                    None,
                    Some(request.url.clone()),
                )
            })?;
            let (stream, response) =
                tokio_tungstenite::connect_async(handshake).await.map_err(|error| {
                    transport_error(
                        format!("Failed to open WebSocket: {error}"),
                        TransportOperation::Request,
                        Some(TransportPhase::Connect),
                        Some(TransportDelivery::NotSent),
                        None,
                        Some(request.url.clone()),
                    )
                })?;
            let http = Some(crate::schema::HttpContext {
                url: request.url.clone(),
                status: response.status().as_u16(),
                headers: response
                    .headers()
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_ascii_lowercase(),
                            value.to_str().unwrap_or("").to_string(),
                        )
                    })
                    .collect(),
            });
            let (sink, mut stream) = stream.split();
            let sink = Arc::new(Mutex::new(sink));
            let url = request.url.clone();
            let send_sink = Arc::clone(&sink);
            let send_text: SendFn = Arc::new(move |message| {
                let sink = Arc::clone(&send_sink);
                Box::pin(async move {
                    sink.lock().await.send(Message::Text(message)).await.map_err(|error| {
                        transport_error(
                            format!("Failed to send WebSocket message: {error}"),
                            TransportOperation::Write,
                            Some(TransportPhase::Send),
                            Some(TransportDelivery::NotSent),
                            None,
                            None,
                        )
                    })
                })
            });
            let close_sink = Arc::clone(&sink);
            let close: CloseFn = Arc::new(move |code, reason| {
                let sink = Arc::clone(&close_sink);
                let reason = reason.to_string();
                Box::pin(async move {
                    let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                        code: code.into(),
                        reason: reason.into(),
                    };
                    let _ = sink.lock().await.send(Message::Close(Some(frame))).await;
                    let _ = sink.lock().await.close().await;
                })
            });
            let messages_url = url.clone();
            let messages = async_stream::stream! {
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(Message::Text(text)) => yield Ok(WebSocketMessage::Text(text)),
                        Ok(Message::Binary(bytes)) => yield Ok(WebSocketMessage::Binary(bytes)),
                        Ok(Message::Ping(_) | Message::Pong(_)) => {}
                        Ok(Message::Close(frame)) => {
                            let code = frame.as_ref().map_or(1005, |frame| u16::from(frame.code));
                            let reason = frame
                                .as_ref()
                                .map(|frame| frame.reason.to_string())
                                .unwrap_or_default();
                            let mut error = transport_error(
                                format!("WebSocket closed with code {code}"),
                                TransportOperation::Read,
                                Some(TransportPhase::Close),
                                None,
                                Some(code.to_string()),
                                Some(messages_url.clone()),
                            );
                            if let crate::schema::AiErrorReason::Transport { fields, .. } =
                                &mut error.reason
                            {
                                if !reason.is_empty() {
                                    fields.body = Some(reason);
                                }
                            }
                            yield Err(error);
                            return;
                        }
                        Ok(Message::Frame(_)) => {}
                        Err(error) => {
                            let message = format!("WebSocket error: {error}");
                            let base = transport_error(
                                message,
                                TransportOperation::Read,
                                Some(TransportPhase::Receive),
                                None,
                                Some("message".into()),
                                Some(messages_url.clone()),
                            );
                            yield Err(annotate_transport_error(
                                base,
                                TransportPhase::Receive,
                                TransportDelivery::Ambiguous,
                            ));
                            return;
                        }
                    }
                }
                // stream ended without a close frame — abnormal closure
                yield Err(transport_error(
                    "WebSocket closed with code 1006",
                    TransportOperation::Read,
                    Some(TransportPhase::Close),
                    None,
                    Some("1006".into()),
                    Some(messages_url.clone()),
                ));
            };
            Ok(WebSocketConnection { http, send_text, messages: Box::pin(messages), close })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_url_converts_schemes_and_preserves_the_rest() {
        assert_eq!(
            to_websocket_url("https://api.openai.test/v1").unwrap(),
            "wss://api.openai.test/v1"
        );
        assert_eq!(
            to_websocket_url("http://127.0.0.1:8080/v1/responses?api-version=v1").unwrap(),
            "ws://127.0.0.1:8080/v1/responses?api-version=v1"
        );
        assert_eq!(to_websocket_url("https://host.example").unwrap(), "wss://host.example/");
        let error = to_websocket_url("ftp://host.example").unwrap_err();
        assert!(error.message().contains("Unsupported WebSocket URL protocol ftp"));
        assert!(to_websocket_url("host.example").is_err());
    }

    #[test]
    fn frame_limit_is_sixteen_mib() {
        assert_eq!(MAX_FRAME_BYTES, 16 * 1024 * 1024);
        assert_eq!(DEFAULT_ROTATE_AFTER_MS, 55 * 60 * 1000);
        assert_eq!(DEFAULT_CONNECT_TIMEOUT, Duration::from_secs(15));
        assert_eq!(DEFAULT_IDLE_TIMEOUT, DEFAULT_IDLE_TIMEOUT);
        assert_eq!(DEFAULT_IDLE_TIMEOUT.as_secs(), 1800);
        assert_eq!(MAX_STREAM_FAILURES, 5);
    }
}
