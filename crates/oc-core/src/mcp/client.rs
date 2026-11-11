//! MCP client — port of `mcp/client.ts` (core-runner-map §5).
//!
//! Wraps an [`rmcp`] client connection behind the same operation surface v2
//! exposes (`tools`/`prompts`/`resources`/`readResource`/`prompt`/`callTool`
//! plus close / list-changed callbacks), with:
//!
//! - **timeouts** — startup 30s, catalog (discovery) 30s, execution 12h,
//!   terminate 1s (`config.timeout` overrides);
//! - **transient retry** — up to two more attempts, 250ms then 500ms
//!   (base 250ms ×2 per hop; the ref's two hops are 250/1000 —
//!   G-280①) while the failure looks transient (HTTP 408/429/5xx
//!   except 501, network errors carrying an OS error code);
//! - **session-expiry detection** — a legacy Streamable HTTP server that no
//!   longer recognizes `Mcp-Session-Id` answers 404 (or the 400
//!   "Bad Request: Server not initialized" a freshly restarted
//!   single-session server emits). Those map to
//!   [`SESSION_EXPIRED_MESSAGE`] so the registry reconnects and replays the
//!   request once.
//!
//! # Deviations from v2 (rmcp 3.7.0 SDK constraints)
//!
//! - **Elicitation capability shape** — v2 advertises
//!   `elicitation: { form: { applyDefaults: true }, url: {} }`
//!   (client.ts:188). rmcp's `FormElicitationCapability` models only
//!   `schemaValidation`, so this port advertises the same structure with
//!   both sub-capabilities present but the `applyDefaults` hint omitted:
//!   `elicitation: { form: {}, url: {} }`. Behavioral parity holds (the
//!   converted form fields do carry schema `default` values).
//! - The negotiated `Mcp-Session-Id` lives inside rmcp's transport worker
//!   and is not exposed, so expiry classification keys off the HTTP status
//!   carried in the error chain (remote connections only — a stdio server
//!   never carries one). rmcp performs the terminate DELETE itself when the
//!   transport closes, bounded here by [`TERMINATE_TIMEOUT`]; the
//!   transport's own transparent 404 session recovery (new in 3.7,
//!   `reinit_on_expired_session`) is disabled so expiry still surfaces to
//!   the registry exactly like v2.
//! - Protocol negotiation (client.ts:193 + 393-397
//!   `negotiation(config.protocol)`): absent/`legacy` runs the plain
//!   `initialize` handshake offering `2025-11-25` (v2.0.24's TS client
//!   offers its `SUPPORTED_PROTOCOL_VERSIONS[0]`); `auto` probes
//!   `server/discover` preferring `2026-07-28` then `2025-11-25` and
//!   falls back to the legacy handshake for old servers; a dated pin
//!   (`2026-07-28`) requires the server to declare that revision —
//!   otherwise the connect fails with v2's
//!   `Set "protocol" for this server to one of those or to "legacy"`
//!   hint (client.ts:376-380). rmcp's own server-answer acceptance
//!   semantics decide the negotiated revision within the offered set.
//! - Requests ride `send_cancellable_request` directly (not rmcp's
//!   `call_tool`/`get_prompt`/`read_resource` convenience wrappers): the
//!   wrappers neither accept per-request timeouts (catalog 30s / execution
//!   12h here) nor preserve the single-round MRTR semantics v2 has, and
//!   they engage the 3.7 response cache (SEP-2549) — all behavior this
//!   port must keep unchanged.
//! - Modern-era url elicitations (`tools/call` answering
//!   `resultType: "input_required"`, SEP-2322's MRTR
//!   retry-with-input round) ARE surfaced: the client fulfills every
//!   embedded `elicitation/create` request through the elicitation
//!   handler and retries `tools/call` with `inputResponses` + the
//!   echoed `requestState`, up to rmcp's
//!   [`DEFAULT_MRTR_MAX_ROUNDS`] and under one shared
//!   execution-timeout deadline. Two wire shapes reach the driver:
//!   rmcp's typed [`rmcp::model::ServerResult::InputRequiredResult`]
//!   (url elicitations carrying `elicitationId`) and the id-less url
//!   shape rmcp cannot model (its `UrlElicitationParams` requires
//!   `elicitationId`), which degrades to
//!   [`rmcp::model::ServerResult::CustomResult`] and is parsed
//!   leniently here. Non-elicitation input requests (sampling, roots)
//!   and `input_required` without an installed elicitation handler
//!   keep the opaque `unexpected tools/call response` error. The
//!   legacy `elicitation/create` server-request flow remains fully
//!   wired through the rmcp handler.

// rmcp request constructors use `Default::default()` for their const-string
// method markers; the pedantic `default_constructed_unit_structs`-style lints
// do not apply to these zero-sized marker types.
#![allow(clippy::default_trait_access, clippy::must_use_candidate)]

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use oc_schema::mcp::{Protocol, ServerConfig};
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ContentBlock, GetPromptRequestParams,
    Implementation, InitializeRequestParams, ListPromptsRequest, ListResourceTemplatesRequest,
    ListResourcesRequest, ListToolsRequest, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, RequestMetaObject, ServerPeerInfo, ServerResult,
};
use rmcp::service::{
    ClientLifecycleMode, ClientServiceExt, NotificationContext, Peer, PeerRequestOptions,
    RequestContext, RoleClient, RunningService,
};
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use rmcp::ClientHandler;
use thiserror::Error;
use tokio::io::AsyncRead;

use crate::environment::stdio::{FrameCapReader, PipedProcess, StdioSpawner, StdioSpec};

/// Maximum time to establish and initialize the server.
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time for discovery requests (`tools/list`…).
pub const DEFAULT_CATALOG_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum time for tool and prompt execution (12 hours).
pub const DEFAULT_EXECUTION_TIMEOUT: Duration = Duration::from_hours(12);
/// Budget for the terminate + close handshake.
pub const TERMINATE_TIMEOUT: Duration = Duration::from_secs(1);

/// `SessionExpiredError.message` prefix — matches v2
/// `MCP server session expired: {server}`.
pub const SESSION_EXPIRED_MESSAGE: &str = "MCP server session expired: ";

/// G-358 — the catalog pagination safety cap. The ref SDK follows
/// `nextCursor` in an unbounded loop (rmcp's own `list_all_*` helpers
/// are likewise unbounded); this port keeps the loop but bounds it so a
/// misbehaving server that always answers a cursor cannot spin forever.
/// 1000 pages × any page size comfortably exceeds every real catalog.
const MAX_LIST_PAGES: usize = 1000;

/// `MCP.NeedsAuthError` — the server answered 401/403 and OAuth has no token.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct NeedsAuthError {
    pub server: String,
    pub message: String,
}

/// `MCP.ConnectError`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct ConnectError {
    pub server: String,
    pub message: String,
}

/// `MCP.SessionExpiredError` — a legacy Streamable HTTP server no longer
/// recognizes this connection's session; the registry reconnects.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("MCP server session expired: {server}")]
pub struct SessionExpiredError {
    pub server: String,
}

/// One opaque failure of a live-connection request. A message starting with
/// [`SESSION_EXPIRED_MESSAGE`] is the replayable expiry signal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct RequestError {
    pub server: String,
    pub message: String,
}

impl RequestError {
    /// Whether this failure observed a session expiry.
    #[must_use]
    pub fn is_session_expired(&self) -> bool {
        self.message.starts_with(SESSION_EXPIRED_MESSAGE)
    }

    /// G-359: whether this failure was a 401/403 authentication
    /// rejection — [`Connection::classify`] shapes exactly this message
    /// suffix for [`unauthorized`] chains (the registry keys its runtime
    /// token-refresh recovery off it, mirroring the SDK's 401
    /// auto-refresh hook).
    #[must_use]
    pub fn is_unauthorized(&self) -> bool {
        self.message.ends_with(" (unauthorized)")
    }
}

/// `McpClient.CallToolContent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallToolContent {
    Text { text: String },
    Media { data: String, mime_type: String },
}

/// `McpClient.CallToolResult`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CallToolResult {
    pub is_error: bool,
    pub structured: Option<serde_json::Value>,
    pub content: Vec<CallToolContent>,
}

/// `toCallToolResult` (client.ts:399-418): text stays text, image/audio
/// become media, `resource_link` flattens to its URI as text
/// (client.ts:407), embedded resources flatten to their text/blob (or
/// URI).
#[must_use]
pub fn to_call_tool_result(result: &rmcp::model::CallToolResult) -> CallToolResult {
    let mut content = Vec::new();
    for part in &result.content {
        match part {
            ContentBlock::Text(text) => {
                content.push(CallToolContent::Text { text: text.text.clone() });
            }
            ContentBlock::Image(image) => {
                content.push(CallToolContent::Media {
                    data: image.data.clone(),
                    mime_type: image.mime_type.clone(),
                });
            }
            ContentBlock::Audio(audio) => {
                content.push(CallToolContent::Media {
                    data: audio.data.clone(),
                    mime_type: audio.mime_type.clone(),
                });
            }
            ContentBlock::ResourceLink(link) => {
                content.push(CallToolContent::Text { text: link.uri.clone() });
            }
            ContentBlock::Resource(resource) => match &resource.resource {
                rmcp::model::ResourceContents::TextResourceContents { text, .. } => {
                    content.push(CallToolContent::Text { text: text.clone() });
                }
                rmcp::model::ResourceContents::BlobResourceContents {
                    uri,
                    blob,
                    mime_type,
                    ..
                } => match mime_type {
                    Some(mime_type) => content.push(CallToolContent::Media {
                        data: blob.clone(),
                        mime_type: mime_type.clone(),
                    }),
                    None => content.push(CallToolContent::Text { text: uri.clone() }),
                },
                _ => {}
            },
            // Future block kinds.
            _ => {}
        }
    }
    CallToolResult {
        is_error: result.is_error.unwrap_or(false),
        structured: result.structured_content.clone(),
        content,
    }
}

/// One modern-era `input_required` payload (`tools/call`, SEP-2322) as
/// raw JSON, from either wire shape rmcp hands back: the typed
/// [`ServerResult::InputRequiredResult`] (url elicitations carrying
/// `elicitationId`) or the id-less url elicitation rmcp cannot model
/// (its `UrlElicitationParams` requires the id), which degrades to
/// [`ServerResult::CustomResult`] in the untagged union and is
/// recognized by its `resultType` discriminator.
fn input_required_payload(result: &ServerResult) -> Option<serde_json::Value> {
    match result {
        ServerResult::InputRequiredResult(required) => serde_json::to_value(required).ok(),
        ServerResult::CustomResult(custom)
            if custom.0.get("resultType").and_then(serde_json::Value::as_str)
                == Some("input_required") =>
        {
            Some(custom.0.clone())
        }
        _ => None,
    }
}

/// One embedded `elicitation/create` params object → the registry's
/// params, lenient where rmcp is strict: a url-mode request WITHOUT
/// `elicitationId` (the TS SDK's `elicitUrl` sends exactly that inside
/// MRTR rounds; test/mcp.test.ts:168) keeps `elicitation_id: None` —
/// no parked completion entry, no `elicitationID` form metadata.
fn lenient_elicit_params(
    params: &serde_json::Value,
) -> Option<super::elicitation::ElicitationParams> {
    let message = params.get("message").and_then(serde_json::Value::as_str)?.to_string();
    if params.get("mode").and_then(serde_json::Value::as_str) == Some("url") {
        let url = params.get("url").and_then(serde_json::Value::as_str)?.to_string();
        let elicitation_id =
            params.get("elicitationId").and_then(serde_json::Value::as_str).map(str::to_string);
        return Some(super::elicitation::ElicitationParams::Url { message, url, elicitation_id });
    }
    Some(super::elicitation::ElicitationParams::Form {
        message,
        requested_schema: params.get("requestedSchema").cloned().unwrap_or(serde_json::Value::Null),
    })
}

/// live on inner errors and are lost once flattened to one message).
fn error_chain(error: &(dyn std::error::Error + '_)) -> Vec<String> {
    let mut chain = vec![error.to_string()];
    let mut source = error.source();
    while let Some(err) = source {
        chain.push(err.to_string());
        source = err.source();
    }
    chain
}

/// HTTP statuses found anywhere in the error chain. NOTE (G-223): the
/// `(HTTP status NNN)` text is REQWEST's own `Display` for status errors
/// (and rmcp's `HTTP {status}: {body}` for `UnexpectedServerResponse`),
/// not remote server wording — behavior-affecting classification
/// (`session_expired`/`unauthorized`) uses the typed downcasts below;
/// this remains only for the transient-retry heuristic.
fn http_statuses(chain: &[String]) -> Vec<u16> {
    let mut out = Vec::new();
    for text in chain {
        let mut rest = text.as_str();
        while let Some(index) = rest.find("(HTTP status ") {
            rest = &rest[index + "(HTTP status ".len()..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(status) = digits.parse::<u16>() {
                out.push(status);
            }
        }
        for (marker, status) in [("404 Not Found", 404u16), ("400 Bad Request", 400)] {
            if text.contains(marker) {
                out.push(status);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// G-223 — typed error classification
// ---------------------------------------------------------------------------

/// A remote-transport client that records whether the transport ever
/// attached a negotiated `Mcp-Session-Id` (G-223): rmcp 3.7 keeps the
/// session id inside its transport worker with no accessor, so the
/// client we hand the transport observes it at the `StreamableHttpClient`
/// seam (every worker POST/GET carries `session_id: Option<Arc<str>>`).
/// This is the port of the ref's `session.transport?.sessionId !==
/// undefined` gate (client.ts:230-233).
#[derive(Clone)]
struct SessionObservingClient {
    inner: reqwest13::Client,
    /// Set once any request carried a session id (a sessionful server).
    session_attached: Arc<std::sync::atomic::AtomicBool>,
}

impl SessionObservingClient {
    fn record(&self, session_id: Option<&std::sync::Arc<str>>) {
        if session_id.is_some() {
            self.session_attached.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl rmcp::transport::streamable_http_client::StreamableHttpClient for SessionObservingClient {
    type Error = reqwest13::Error;

    async fn post_message(
        &self,
        uri: std::sync::Arc<str>,
        message: rmcp::model::ClientJsonRpcMessage,
        session_id: Option<std::sync::Arc<str>>,
        auth_header: Option<String>,
        custom_headers: std::collections::HashMap<
            reqwest13::header::HeaderName,
            reqwest13::header::HeaderValue,
        >,
    ) -> Result<
        rmcp::transport::streamable_http_client::StreamableHttpPostResponse,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        self.record(session_id.as_ref());
        self.inner.post_message(uri, message, session_id, auth_header, custom_headers).await
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: std::sync::Arc<str>,
        message: rmcp::model::ClientJsonRpcMessage,
        session_id: Option<std::sync::Arc<str>>,
        auth_header: Option<String>,
        custom_headers: std::collections::HashMap<
            reqwest13::header::HeaderName,
            reqwest13::header::HeaderValue,
        >,
        max_sse_event_size: usize,
    ) -> Result<
        rmcp::transport::streamable_http_client::StreamableHttpPostResponse,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        self.record(session_id.as_ref());
        self.inner
            .post_message_with_max_sse_event_size(
                uri,
                message,
                session_id,
                auth_header,
                custom_headers,
                max_sse_event_size,
            )
            .await
    }

    async fn delete_session(
        &self,
        uri: std::sync::Arc<str>,
        session_id: std::sync::Arc<str>,
        auth_header: Option<String>,
        custom_headers: std::collections::HashMap<
            reqwest13::header::HeaderName,
            reqwest13::header::HeaderValue,
        >,
    ) -> Result<(), rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>> {
        self.inner.delete_session(uri, session_id, auth_header, custom_headers).await
    }

    async fn get_stream(
        &self,
        uri: std::sync::Arc<str>,
        session_id: Option<std::sync::Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: std::collections::HashMap<
            reqwest13::header::HeaderName,
            reqwest13::header::HeaderValue,
        >,
    ) -> Result<
        futures::stream::BoxStream<
            'static,
            Result<sse_stream::Sse, rmcp::transport::streamable_http_client::SseError>,
        >,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        self.record(session_id.as_ref());
        self.inner.get_stream(uri, session_id, last_event_id, auth_header, custom_headers).await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: std::sync::Arc<str>,
        session_id: Option<std::sync::Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: std::collections::HashMap<
            reqwest13::header::HeaderName,
            reqwest13::header::HeaderValue,
        >,
        max_sse_event_size: usize,
    ) -> Result<
        futures::stream::BoxStream<
            'static,
            Result<sse_stream::Sse, rmcp::transport::streamable_http_client::SseError>,
        >,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        self.record(session_id.as_ref());
        self.inner
            .get_stream_with_max_sse_event_size(
                uri,
                session_id,
                last_event_id,
                auth_header,
                custom_headers,
                max_sse_event_size,
            )
            .await
    }
}

/// G-223 — the typed HTTP error rmcp's streamable transport produces,
/// reached by walking the `#[source]` chain: `ServiceError::TransportSend`
/// boxes the transport's `StreamableHttpError<reqwest::Error>` behind
/// `DynamicTransportError::error` (also a `#[source]`), so the concrete
/// type survives for `downcast_ref`.
pub(super) fn transport_http_error<'a>(
    error: &'a (dyn std::error::Error + 'static),
) -> Option<&'a rmcp::transport::streamable_http_client::StreamableHttpError<reqwest13::Error>> {
    let mut node: Option<&(dyn std::error::Error + '_)> = Some(error);
    while let Some(current) = node {
        if let Some(rmcp::ServiceError::TransportSend(dynamic)) =
            current.downcast_ref::<rmcp::ServiceError>()
        {
            if let Some(inner) = dynamic
                .error
                .downcast_ref::<rmcp::transport::streamable_http_client::StreamableHttpError<
                reqwest13::Error,
            >>() {
                return Some(inner);
            }
        }
        if let Some(direct) = current
            .downcast_ref::<rmcp::transport::streamable_http_client::StreamableHttpError<reqwest13::Error>>()
        {
            return Some(direct);
        }
        node = current.source();
    }
    None
}

/// Whether any error in the chain is a 401/403 authentication rejection
/// (G-223: typed — the streamable transport's `AuthRequired` /
/// `InsufficientScope` variants, a reqwest status error, or rmcp's own
/// `HTTP {status}: {body}` rendering of a challenge-less 401/403; no
/// remote-server wording is matched).
pub(super) fn unauthorized(error: &(dyn std::error::Error + 'static)) -> bool {
    let Some(http) = transport_http_error(error) else { return false };
    match http {
        rmcp::transport::streamable_http_client::StreamableHttpError::AuthRequired(_)
        | rmcp::transport::streamable_http_client::StreamableHttpError::InsufficientScope(_) => {
            true
        }
        rmcp::transport::streamable_http_client::StreamableHttpError::Client(inner) => {
            inner.status().is_some_and(|status| status.as_u16() == 401 || status.as_u16() == 403)
        }
        rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
            text,
        ) => text.starts_with("HTTP 401") || text.starts_with("HTTP 403"),
        _ => false,
    }
}

/// Whether the chain shows a session expiry (G-223, ref client.ts:230-243):
///
/// * the TYPED `StreamableHttpError::SessionExpired` — rmcp only emits it
///   when the POST actually carried the negotiated `Mcp-Session-Id`
///   header (its `session_was_attached` gate) or a control raced a
///   replaced session, so this carries the ref's
///   `session.transport?.sessionId !== undefined` gate by construction; a
///   plain 404 from a stateless server arrives as
///   `UnexpectedServerResponse("HTTP 404 …")` and does NOT match;
/// * the restarted single-session server's 400 (ref client.ts:234-239):
///   the JSON-RPC error text `Bad Request: Server not initialized` — a
///   server protocol message, not HTTP wording — still gated on a
///   session having been attached, mirroring the ref.
pub(super) fn session_expired(
    error: &(dyn std::error::Error + 'static),
    session_attached: bool,
) -> bool {
    if transport_http_error(error).is_some_and(|http| {
        matches!(http, rmcp::transport::streamable_http_client::StreamableHttpError::SessionExpired)
    }) {
        return true;
    }
    session_attached
        && error_chain(error)
            .iter()
            .any(|text| text.contains("Bad Request: Server not initialized"))
}

/// `isTransient` (client.ts:55-58): 408/429/5xx≠501 recover on retry; 501
/// means the server will never support the request; network failures carry
/// an OS error code.
fn is_transient(chain: &[String]) -> bool {
    let statuses = http_statuses(chain);
    if !statuses.is_empty() {
        return statuses
            .iter()
            .any(|status| *status == 408 || *status == 429 || (*status >= 500 && *status != 501));
    }
    let text = chain.join(": ");
    (text.contains("error trying to connect") || text.contains("connection closed"))
        && text.contains("os error")
}

type BoxedRequest<T> = Pin<Box<dyn Future<Output = Result<T, RequestError>> + Send>>;

/// The transient-retry loop (client.ts:169-182): "Two more attempts,
/// 250 ms and 1 s later, while the failure is transient and the
/// deadline has not passed" — `Schedule.max([exponential(250ms, 4),
/// recurs(2)])` (G-280①): the exponential arm proposes 250/1000/4000…,
/// `recurs(2)` bounds it to two hops, `max` picks the proposed delay.
pub(super) async fn retrying<T>(
    mut attempt: impl FnMut() -> BoxedRequest<T>,
) -> Result<T, RequestError> {
    const DELAYS_MS: [u64; 2] = [250, 1000];
    let mut round = 0;
    loop {
        match attempt().await {
            Ok(value) => return Ok(value),
            Err(error) => {
                if round >= DELAYS_MS.len() || !is_transient(std::slice::from_ref(&error.message)) {
                    return Err(error);
                }
                tokio::time::sleep(Duration::from_millis(DELAYS_MS[round])).await;
                round += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// handler
// ---------------------------------------------------------------------------

/// Watch callbacks the registry installs after connect (client.ts:156-161):
/// late registrations replace the no-op defaults because the SDK takes
/// list-changed handlers at construction.
#[derive(Default)]
pub struct Changed {
    pub tools: Option<Box<dyn Fn() + Send + Sync>>,
    pub prompts: Option<Box<dyn Fn() + Send + Sync>>,
    pub resources: Option<Box<dyn Fn() + Send + Sync>>,
    /// `onSessionExpired` (client.ts:161 / index.ts:415): fired once when
    /// request classification observes a session expiry. Background
    /// consumers (list-changed watches) do not retry, so the registry
    /// reconnects through this signal.
    pub session_expired: Option<Box<dyn Fn() + Send + Sync>>,
}

/// The rmcp client handler: answers `roots/list` with the location
/// directory, forwards list-changed notifications, and dispatches
/// `elicitation/create` (+ legacy
/// `notifications/elicitation/complete`) to the elicitation handler
/// (client.ts:206-214).
struct Handler {
    capabilities: ClientCapabilities,
    implementation: Implementation,
    roots_directory: String,
    changed: Arc<Mutex<Changed>>,
    server: Arc<str>,
    elicitation: Option<Arc<super::elicitation::ClientElicitation>>,
}

impl ClientHandler for Handler {
    // Roots are SEP-2577-deprecated in rmcp 3.x; answering `roots/list`
    // is the pre-existing behavior and stays.
    #[allow(deprecated)]
    fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<rmcp::model::ListRootsResult, rmcp::ErrorData>> + Send + '_
    {
        std::future::ready(Ok(rmcp::model::ListRootsResult::new(vec![rmcp::model::Root::new(
            path_to_file_url(&self.roots_directory),
        )])))
    }

    /// `elicitation/create` (client.ts:208-210): the form ask races the
    /// request's abort signal; the elicitation module owns the mapping.
    fn create_elicitation(
        &self,
        request: rmcp::model::ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<rmcp::model::ElicitResult, rmcp::ErrorData>> + Send + '_ {
        let server = Arc::clone(&self.server);
        let Some(elicitation) = self.elicitation.as_ref().map(Arc::clone) else {
            // The capability was not advertised; rmcp's default decline
            // applies. Unreachable for well-behaved servers.
            return std::future::ready(Ok(rmcp::model::ElicitResult::new(
                rmcp::model::ElicitationAction::Decline,
            )))
            .left_future();
        };
        async move { Ok(elicitation.handle(&server, request, context.ct).await) }.right_future()
    }

    /// Legacy `notifications/elicitation/complete` rides rmcp's custom
    /// notification path (the SDK models no typed variant; client.ts
    /// registers it as a raw handler).
    fn on_custom_notification(
        &self,
        notification: rmcp::model::CustomNotification,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        if notification.method == "notifications/elicitation/complete" {
            if let Some(elicitation) = &self.elicitation {
                elicitation.complete_notification(&self.server, notification.params.as_ref());
            }
        }
        std::future::ready(())
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        if let Some(callback) = self.changed.lock().tools.as_ref() {
            callback();
        }
        std::future::ready(())
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        if let Some(callback) = self.changed.lock().prompts.as_ref() {
            callback();
        }
        std::future::ready(())
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        if let Some(callback) = self.changed.lock().resources.as_ref() {
            callback();
        }
        std::future::ready(())
    }

    fn get_info(&self) -> rmcp::model::ClientConfig {
        // v2.0.24's TS client offers `2025-11-25` — its
        // SUPPORTED_PROTOCOL_VERSIONS[0] over the legacy initialize
        // handshake. rmcp's own acceptance rules govern the negotiated
        // revision; this pins only the offered version.
        InitializeRequestParams::new(self.capabilities.clone(), self.implementation.clone())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
    }
}

/// `pathToFileURL` for the roots/list answer.
fn path_to_file_url(path: &str) -> String {
    let encoded: String = path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '~') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect();
    format!("file://{encoded}")
}

// ---------------------------------------------------------------------------
// connection
// ---------------------------------------------------------------------------

/// A live connection to one MCP server (client.ts `Connection`). Cheap to
/// clone: every handle shares the same rmcp service.
#[derive(Clone)]
pub struct Connection {
    server: Arc<str>,
    peer: Peer<RoleClient>,
    peer_info: Option<Arc<ServerPeerInfo>>,
    /// Sole owner of the rmcp service; taken by close / close-watch.
    service: Arc<tokio::sync::Mutex<Option<RunningService<RoleClient, Handler>>>>,
    changed: Arc<Mutex<Changed>>,
    catalog_timeout: Duration,
    execution_timeout: Duration,
    remote: bool,
    /// Set once an expiry was observed so the registry reconnects once.
    expiry_flag: Arc<std::sync::atomic::AtomicBool>,
    /// G-223 — whether the transport ever attached a negotiated
    /// `Mcp-Session-Id` (observed at the client seam; stdio/local
    /// connections stay false).
    session_attached: Arc<std::sync::atomic::AtomicBool>,
    /// The elicitation handler (drives MRTR `input_required` rounds).
    elicitation: Option<Arc<super::elicitation::ClientElicitation>>,
}

impl Connection {
    fn server_name(&self) -> &str {
        &self.server
    }

    /// The server's initialization `instructions`, trimmed.
    pub fn instructions(&self) -> Option<String> {
        self.peer_info
            .as_ref()
            .and_then(|info| info.instructions.as_ref())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    }

    /// Whether the server advertises the resources capability.
    #[must_use]
    pub fn has_resources(&self) -> bool {
        self.peer_info.as_ref().is_some_and(|info| info.capabilities.resources.is_some())
    }

    /// True when this connection already reported a session expiry (the
    /// registry reconnects once per observed expiry).
    #[must_use]
    pub fn expiry_reported(&self) -> bool {
        self.expiry_flag.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// G-223 test seam: mark this (mock-transport) connection as carrying
    /// a negotiated session id. The real observing client only runs on
    /// the streamable HTTP transport; scripted/mock transports route
    /// through [`connect_with_transport`] and default to NOT attached.
    #[cfg(test)] // test-only attachment marker (mock transports)
    pub(crate) fn mark_session_attached(&self) {
        self.session_attached.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// G-223 — whether the transport ever attached a session id (the
    /// ref's `session.transport?.sessionId !== undefined` gate).
    #[must_use]
    pub fn session_attached(&self) -> bool {
        self.session_attached.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Maps one rmcp failure to [`RequestError`], classifying session
    /// expiry (once) and authentication rejections.
    fn classify(&self, error: &rmcp::ServiceError) -> RequestError {
        let chain = error_chain(error);
        // G-223: typed classification — a 404 only counts as expiry when
        // the transport carried a session id (see `session_expired`).
        if self.remote && !self.expiry_reported() && session_expired(error, self.session_attached())
        {
            self.expiry_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            // `onSessionExpired` (index.ts:415): background refreshes can
            // observe the expiry too — they do not retry, so the registry
            // reconnects for them here.
            if let Some(callback) = self.changed.lock().session_expired.as_ref() {
                callback();
            }
            return RequestError {
                server: self.server_name().to_string(),
                message: format!("{SESSION_EXPIRED_MESSAGE}{}", self.server_name()),
            };
        }
        if unauthorized(error) {
            return RequestError {
                server: self.server_name().to_string(),
                message: format!("{} (unauthorized)", chain.join(": ")),
            };
        }
        RequestError {
            server: self.server_name().to_string(),
            message: format!("failed to call MCP server: {}", chain.join(": ")),
        }
    }

    async fn call(
        &self,
        request: rmcp::model::ClientRequest,
        timeout: Duration,
    ) -> Result<ServerResult, RequestError> {
        let handle = self
            .peer
            .send_cancellable_request(request, PeerRequestOptions::with_timeout(timeout))
            .await
            .map_err(|error| self.classify(&error))?;
        handle.await_response().await.map_err(|error| self.classify(&error))
    }

    async fn call_with_meta(
        &self,
        request: rmcp::model::ClientRequest,
        timeout: Duration,
        meta: RequestMetaObject,
    ) -> Result<ServerResult, RequestError> {
        let handle = self
            .peer
            .send_cancellable_request(
                request,
                PeerRequestOptions::with_timeout(timeout).with_meta(meta),
            )
            .await
            .map_err(|error| self.classify(&error))?;
        handle.await_response().await.map_err(|error| self.classify(&error))
    }

    #[allow(clippy::unused_self)] // symmetry with the other error shapers
    fn request_error(&self, what: &str, error: RequestError) -> RequestError {
        if error.is_session_expired() {
            // The expiry signal stays unprefixed (v2 replaces the error
            // with SessionExpiredError wholesale).
            return error;
        }
        RequestError {
            server: error.server,
            message: format!("failed to {what}: {}", error.message),
        }
    }

    /// `tools()` — listing under the catalog timeout with transient retry.
    /// G-358: follows `nextCursor` until the server answers none (the
    /// SDK's `listAllTools` pagination), merging pages in order.
    pub async fn tools(&self) -> Result<Vec<rmcp::model::Tool>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let mut tools = Vec::new();
                let mut cursor: Option<String> = None;
                for _ in 0..MAX_LIST_PAGES {
                    let result = this
                        .call(
                            rmcp::model::ClientRequest::ListToolsRequest(
                                ListToolsRequest::with_param(
                                    PaginatedRequestParams::default().with_cursor(cursor),
                                ),
                            ),
                            timeout,
                        )
                        .await
                        .map_err(|error| this.request_error("list MCP tools", error))?;
                    match result {
                        ServerResult::ListToolsResult(result) => {
                            tools.extend(result.tools);
                            cursor = result.next_cursor;
                            if cursor.is_none() {
                                return Ok(tools);
                            }
                        }
                        _ => {
                            return Err(RequestError {
                                server: this.server_name().to_string(),
                                message: "unexpected tools/list response".to_string(),
                            })
                        }
                    }
                }
                Err(RequestError {
                    server: this.server_name().to_string(),
                    message: format!(
                        "tools/list exceeded {MAX_LIST_PAGES} pagination rounds without completing"
                    ),
                })
            })
            .boxed()
        })
        .await
    }

    /// `prompts()` — G-358: follows `nextCursor` like [`Connection::tools`].
    pub async fn prompts(&self) -> Result<Vec<rmcp::model::Prompt>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let mut prompts = Vec::new();
                let mut cursor: Option<String> = None;
                for _ in 0..MAX_LIST_PAGES {
                    let result = this
                        .call(
                            rmcp::model::ClientRequest::ListPromptsRequest(
                                ListPromptsRequest::with_param(PaginatedRequestParams::default().with_cursor(cursor)),
                            ),
                            timeout,
                        )
                        .await
                        .map_err(|error| this.request_error("list MCP prompts", error))?;
                    match result {
                        ServerResult::ListPromptsResult(result) => {
                            prompts.extend(result.prompts);
                            cursor = result.next_cursor;
                            if cursor.is_none() {
                                return Ok(prompts);
                            }
                        }
                        _ => {
                            return Err(RequestError {
                                server: this.server_name().to_string(),
                                message: "unexpected prompts/list response".to_string(),
                            })
                        }
                    }
                }
                Err(RequestError {
                    server: this.server_name().to_string(),
                    message: format!(
                        "prompts/list exceeded {MAX_LIST_PAGES} pagination rounds without completing"
                    ),
                })
            })
            .boxed()
        })
        .await
    }

    /// `resources()` — G-358: follows `nextCursor` like
    /// [`Connection::tools`].
    pub async fn resources(&self) -> Result<Vec<rmcp::model::Resource>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let mut resources = Vec::new();
                let mut cursor: Option<String> = None;
                for _ in 0..MAX_LIST_PAGES {
                    let result = this
                        .call(
                            rmcp::model::ClientRequest::ListResourcesRequest(
                                ListResourcesRequest::with_param(PaginatedRequestParams::default().with_cursor(cursor)),
                            ),
                            timeout,
                        )
                        .await
                        .map_err(|error| this.request_error("list MCP resources", error))?;
                    match result {
                        ServerResult::ListResourcesResult(result) => {
                            resources.extend(result.resources);
                            cursor = result.next_cursor;
                            if cursor.is_none() {
                                return Ok(resources);
                            }
                        }
                        _ => {
                            return Err(RequestError {
                                server: this.server_name().to_string(),
                                message: "unexpected resources/list response".to_string(),
                            })
                        }
                    }
                }
                Err(RequestError {
                    server: this.server_name().to_string(),
                    message: format!(
                        "resources/list exceeded {MAX_LIST_PAGES} pagination rounds without completing"
                    ),
                })
            })
            .boxed()
        })
        .await
    }

    /// `resourceTemplates()` — some servers declare resources without
    /// implementing template listing; callers collapse failures to empty.
    /// G-358: follows `nextCursor` like [`Connection::tools`].
    pub async fn resource_templates(
        &self,
    ) -> Result<Vec<rmcp::model::ResourceTemplate>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let mut templates = Vec::new();
                let mut cursor: Option<String> = None;
                for _ in 0..MAX_LIST_PAGES {
                    let result = this
                        .call(
                            rmcp::model::ClientRequest::ListResourceTemplatesRequest(
                                ListResourceTemplatesRequest::with_param(PaginatedRequestParams::default().with_cursor(cursor)),
                            ),
                            timeout,
                        )
                        .await
                        .map_err(|error| {
                            this.request_error("list MCP resource templates", error)
                        })?;
                    match result {
                        ServerResult::ListResourceTemplatesResult(result) => {
                            templates.extend(result.resource_templates);
                            cursor = result.next_cursor;
                            if cursor.is_none() {
                                return Ok(templates);
                            }
                        }
                        _ => {
                            return Err(RequestError {
                                server: this.server_name().to_string(),
                                message: "unexpected resources/templates/list response".to_string(),
                            })
                        }
                    }
                }
                Err(RequestError {
                    server: this.server_name().to_string(),
                    message: format!(
                        "resources/templates/list exceeded {MAX_LIST_PAGES} pagination rounds without completing"
                    ),
                })
            })
            .boxed()
        })
        .await
    }

    /// `readResource` — `Ok(None)` when the server does not advertise the
    /// resources capability.
    pub async fn read_resource(
        &self,
        uri: &str,
    ) -> Result<Option<rmcp::model::ReadResourceResult>, RequestError> {
        if !self.has_resources() {
            return Ok(None);
        }
        let result = self
            .call(
                rmcp::model::ClientRequest::ReadResourceRequest(
                    rmcp::model::ReadResourceRequest::new(ReadResourceRequestParams::new(uri)),
                ),
                self.execution_timeout,
            )
            .await
            .map_err(|error| self.request_error("read MCP resource", error))?;
        match result {
            ServerResult::ReadResourceResult(result) => Ok(Some(result)),
            _ => Err(RequestError {
                server: self.server_name().to_string(),
                message: "unexpected resources/read response".to_string(),
            }),
        }
    }

    /// `prompt(name, args?)`.
    pub async fn prompt(
        &self,
        name: &str,
        args: Option<BTreeMap<String, String>>,
    ) -> Result<rmcp::model::GetPromptResult, RequestError> {
        let arguments = args.map(|args| {
            args.into_iter()
                .map(|(key, value)| (key, serde_json::Value::String(value)))
                .collect::<serde_json::Map<String, serde_json::Value>>()
        });
        let mut params = GetPromptRequestParams::new(name);
        if let Some(arguments) = arguments {
            params = params.with_arguments(arguments);
        }
        let result = self
            .call(
                rmcp::model::ClientRequest::GetPromptRequest(rmcp::model::GetPromptRequest::new(
                    params,
                )),
                self.execution_timeout,
            )
            .await
            .map_err(|error| self.request_error("get MCP prompt", error))?;
        match result {
            ServerResult::GetPromptResult(result) => Ok(result),
            _ => Err(RequestError {
                server: self.server_name().to_string(),
                message: "unexpected prompts/get response".to_string(),
            }),
        }
    }

    /// `callTool(name, args?, sessionID?)` — the session id travels as the
    /// `ai.opencode/sessionID` request meta; the SDK's auto progress token
    /// keeps long calls alive under the execution-timeout hard wall.
    ///
    /// A `tools/call` answer of `resultType: "input_required"` (modern
    /// MRTR url/form elicitations) drives the retry-with-input round:
    /// each embedded `elicitation/create` request is answered through
    /// the elicitation handler and the call retried with
    /// `inputResponses` + the echoed `requestState` (the TS SDK 2.0
    /// client does the same inside `callTool`; test/mcp.test.ts:164-176
    /// "Modern servers cannot call elicitInput; they return
    /// `input_required` and the client retries").
    pub async fn call_tool(
        &self,
        name: &str,
        args: Option<&serde_json::Map<String, serde_json::Value>>,
        session_id: Option<&str>,
    ) -> Result<CallToolResult, RequestError> {
        let mut meta = RequestMetaObject::new();
        if let Some(session_id) = session_id {
            meta.insert(
                "ai.opencode/sessionID".to_string(),
                serde_json::Value::String(session_id.to_string()),
            );
        }
        let mut params = CallToolRequestParams::new(name.to_string());
        if let Some(args) = args {
            params = params.with_arguments(args.clone());
        }
        // One execution-timeout deadline spans every MRTR round — the
        // TS SDK's single `timeout: execution` covers its internal
        // retries the same way.
        let deadline = tokio::time::Instant::now() + self.execution_timeout;
        for _ in 0..rmcp::model::DEFAULT_MRTR_MAX_ROUNDS {
            let timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
            if timeout.is_zero() {
                return Err(RequestError {
                    server: self.server_name().to_string(),
                    message: format!(
                        "failed to call MCP tool: timed out after {}ms",
                        self.execution_timeout.as_millis()
                    ),
                });
            }
            let result = self
                .call_with_meta(
                    rmcp::model::ClientRequest::CallToolRequest(rmcp::model::CallToolRequest::new(
                        params.clone(),
                    )),
                    timeout,
                    meta.clone(),
                )
                .await
                .map_err(|error| self.request_error("call MCP tool", error))?;
            match input_required_payload(&result) {
                Some(required) => {
                    params = self.fulfill_input_required(params, required).await?;
                }
                None => match result {
                    ServerResult::CallToolResult(result) => {
                        return Ok(to_call_tool_result(&result))
                    }
                    _ => {
                        return Err(RequestError {
                            server: self.server_name().to_string(),
                            message: "unexpected tools/call response".to_string(),
                        })
                    }
                },
            }
        }
        // Round exhaustion mirrors the unfulfillable shapes: the opaque
        // v2 error (rmcp's high-level helper errors out at the same
        // bound).
        Err(RequestError {
            server: self.server_name().to_string(),
            message: "unexpected tools/call response".to_string(),
        })
    }

    /// One MRTR round: answer every embedded input request, then stage
    /// the retry's `inputResponses` + `requestState`. Only
    /// `elicitation/create` requests are fulfillable (the v2 client
    /// installs no sampling handler); anything else — or an
    /// `input_required` answer with no elicitation handler installed —
    /// keeps the opaque `unexpected tools/call response` error.
    async fn fulfill_input_required(
        &self,
        mut params: CallToolRequestParams,
        required: serde_json::Value,
    ) -> Result<CallToolRequestParams, RequestError> {
        let opaque = || RequestError {
            server: self.server_name().to_string(),
            message: "unexpected tools/call response".to_string(),
        };
        let Some(elicitation) = self.elicitation.as_ref().map(Arc::clone) else {
            return Err(opaque());
        };
        let mut responses = serde_json::Map::new();
        if let Some(requests) = required.get("inputRequests").and_then(serde_json::Value::as_object)
        {
            for (key, request) in requests {
                let method = request.get("method").and_then(serde_json::Value::as_str);
                if method != Some("elicitation/create") {
                    // sampling/roots input requests are unfulfillable
                    // here (v2 installs no sampling handler either).
                    return Err(opaque());
                }
                let Some(params) = request.get("params") else { return Err(opaque()) };
                let Some(elicit) = lenient_elicit_params(params) else { return Err(opaque()) };
                let url_mode = matches!(elicit, super::elicitation::ElicitationParams::Url { .. });
                let answer = elicitation.answer(&self.server, elicit).await;
                responses.insert(key.clone(), super::elicitation::result_wire(&answer, url_mode));
            }
        }
        params.input_responses = Some(responses.into_iter().collect::<BTreeMap<_, _>>());
        params.request_state =
            required.get("requestState").and_then(serde_json::Value::as_str).map(str::to_string);
        Ok(params)
    }

    /// Install the registry's watch callbacks (list-changed). The
    /// session-expiry signal surfaces through request classification.
    pub fn watch(&self, changed: Changed) {
        *self.changed.lock() = changed;
    }

    /// Resolves once the transport ends. Takes the rmcp service handle; a
    /// later [`Connection::close`] becomes a no-op.
    pub async fn watch_close(&self) {
        let service = self.service.lock().await.take();
        if let Some(service) = service {
            let _ = service.waiting().await;
        }
    }

    /// Close the connection: rmcp terminates the HTTP session (DELETE)
    /// during transport close; the whole teardown is bounded by
    /// [`TERMINATE_TIMEOUT`].
    pub async fn close(&self) {
        let service = self.service.lock().await.take();
        if let Some(service) = service {
            let _ = tokio::time::timeout(TERMINATE_TIMEOUT, service.cancel()).await;
        }
    }

    /// Whether two handles refer to the same underlying connection (the
    /// registry's `entry.client === connection` liveness check).
    #[must_use]
    pub fn is_same(&self, other: &Connection) -> bool {
        Arc::ptr_eq(&self.service, &other.service)
    }
}

// ---------------------------------------------------------------------------
// connect
// ---------------------------------------------------------------------------

/// Options for [`connect`].
pub struct ConnectInput<'a> {
    pub server: &'a str,
    pub config: &'a ServerConfig,
    /// Location directory: stdio cwd base + the `roots/list` answer.
    pub directory: &'a str,
    /// Bearer token from the credential-store seam (remote only).
    pub bearer: Option<String>,
    /// The elicitation handler (v2's optional `elicitation` connect
    /// argument): present ⇒ the client advertises the elicitation
    /// capability and answers `elicitation/create`.
    pub elicitation: Option<Arc<super::elicitation::ClientElicitation>>,
    /// G-1 residual — the Environment stdio seam: stdio servers spawn
    /// through this [`StdioSpawner`] (the location-bound Environment
    /// service's driver) instead of a host-bound `Command`, mirroring
    /// the ref's `mcp/stdio.ts`. `None` falls back to the local host
    /// spawner (byte-identical spawn).
    pub stdio: Option<Arc<dyn StdioSpawner>>,
}

/// Typed connect failures (`NeedsAuthError` / `ConnectError`).
#[derive(Debug, Error)]
pub enum ConnectFailure {
    #[error(transparent)]
    NeedsAuth(#[from] NeedsAuthError),
    #[error(transparent)]
    Connect(#[from] ConnectError),
}

/// The config's timeout block, when present.
#[must_use]
pub fn config_timeout(config: &ServerConfig) -> Option<&oc_schema::mcp::TimeoutConfig> {
    match config {
        ServerConfig::Local(local) => local.timeout.as_ref(),
        ServerConfig::Remote(remote) => remote.timeout.as_ref(),
    }
}

/// `negotiation(config.protocol)` (client.ts:393-397): absent config is
/// legacy — the plain `initialize` handshake with no discover probe;
/// `"auto"` probes `server/discover` and falls back to legacy for old
/// servers; a dated pin requires the server to declare that revision.
fn lifecycle_mode(config: &ServerConfig) -> ClientLifecycleMode {
    let protocol = match config {
        ServerConfig::Local(local) => local.protocol,
        ServerConfig::Remote(remote) => remote.protocol,
    };
    match protocol {
        None | Some(Protocol::Legacy) => ClientLifecycleMode::Initialize,
        Some(Protocol::Auto) => ClientLifecycleMode::Auto {
            // v2 "auto" probes for 2026-07-28 first, 2025-11-25 next;
            // the legacy fallback offers the same 2025-11-25 pin get_info
            // carries.
            // rmcp check (3.7.0, service/client.rs): Auto discover waits
            // `DEFAULT_AUTO_DISCOVER_TIMEOUT` = 10s before falling back
            // to legacy. Ref uses @modelcontextprotocol/client 2.0.0
            // with `{ mode: "auto" }` and no negotiation-level timeout
            // (client.ts:393-397); its node_modules are absent from the
            // checkout so any SDK-internal discover timeout is not
            // inspectable — ref instead bounds the whole connect with
            // config.timeout?.startup around client.connect
            // (client.ts:218). No behavioral divergence found.
            preferred_versions: vec![ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25],
            legacy_version: Some(ProtocolVersion::V_2025_11_25),
        },
        Some(Protocol::Dated) => ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    }
}

fn startup_timeout(config: &ServerConfig) -> Duration {
    config_timeout(config)
        .and_then(|timeout| timeout.startup)
        .map_or(DEFAULT_STARTUP_TIMEOUT, Duration::from_millis)
}

fn catalog_timeout(config: &ServerConfig) -> Duration {
    config_timeout(config)
        .and_then(|timeout| timeout.catalog)
        .map_or(DEFAULT_CATALOG_TIMEOUT, Duration::from_millis)
}

fn execution_timeout(config: &ServerConfig) -> Duration {
    config_timeout(config)
        .and_then(|timeout| timeout.execution)
        .map_or(DEFAULT_EXECUTION_TIMEOUT, Duration::from_millis)
}

/// `McpClient.connect` — builds the transport for the config, runs the
/// initialize handshake, and returns the live connection. On failure the
/// transport is torn down and the error collapses to
/// [`NeedsAuthError`] / [`ConnectError`].
pub async fn connect(input: ConnectInput<'_>) -> Result<Connection, ConnectFailure> {
    match input.config {
        ServerConfig::Local(local) => connect_local(&input, local).await,
        ServerConfig::Remote(remote) => connect_remote(&input, remote).await,
    }
}

/// Test/seam variant of [`connect`] over a caller-provided transport
/// (the mock-transport suites drive the client this way).
pub async fn connect_with_transport<T, E, A>(
    input: &ConnectInput<'_>,
    transport: T,
    remote: bool,
) -> Result<Connection, ConnectFailure>
where
    T: rmcp::transport::IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    // A caller-provided transport never routes through the observing
    // client, so no session attachment is observed (G-223 default: off).
    let session_attached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    start(input, transport, remote, session_attached).await
}

/// Environment overlay a stdio MCP server declares (the caller side of
/// the seam): `BUN_BE_BUN=1` for `opencode` commands (so nested spawns
/// resolve to the same runtime) plus the server config's `environment`
/// entries. The HOST environment is deliberately NOT merged here — the
/// spawner merges its execution plane's own environment underneath via
/// `extend_env` (the local driver extends with the host `process.env`,
/// exactly the `{ ...process.env, ...opts.env }` cross-spawn semantics;
/// a workspace driver would extend with the sandbox's environment so
/// host variables never cross the seam). [`stdio_environment`] remains
/// as the merge oracle for the local driver's byte-for-byte behavior.
pub(super) fn stdio_overlay(
    command: &str,
    environment: Option<&BTreeMap<String, String>>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if command == "opencode" {
        env.insert("BUN_BE_BUN".to_string(), "1".to_string());
    }
    for (key, value) in environment.into_iter().flat_map(|map| map.iter()) {
        env.insert(key.clone(), value.clone());
    }
    env
}

/// The local-driver merge oracle: `{ ...host, ...overlay(command,
/// environment) }` — exactly the environment the local stdio spawner's
/// `extend_env` path produces.
#[cfg(test)]
pub(super) fn stdio_environment(
    host: impl IntoIterator<Item = (String, String)>,
    command: &str,
    environment: Option<&BTreeMap<String, String>>,
) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = host.into_iter().collect();
    if command == "opencode" {
        env.insert("BUN_BE_BUN".to_string(), "1".to_string());
    }
    for (key, value) in environment.into_iter().flat_map(|map| map.iter()) {
        env.insert(key.clone(), value.clone());
    }
    env
}

/// Stdout half carrying the child's stop closure: when the transport
/// drops the reader (connection closed / service torn down) the child is
/// killed, mirroring the scope-owned `ChildProcessHandle` of the ref's
/// env-routed stdio transport (and rmcp's kill-on-drop child).
struct StopOnDropReader {
    inner: crate::environment::stdio::BoxedReader,
    stop: Option<crate::environment::stdio::StopChild>,
}

impl AsyncRead for StopOnDropReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(self.inner.as_mut()).poll_read(cx, buf)
    }
}

impl Drop for StopOnDropReader {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// A connect-time spawn failure surfaced through the stdio seam.
#[derive(Debug, Error)]
#[error("{0}")]
struct StdioSpawnFailure(String);

async fn connect_local(
    input: &ConnectInput<'_>,
    local: &oc_schema::mcp::LocalConfig,
) -> Result<Connection, ConnectFailure> {
    let Some((command, args)) = local.command.split_first() else {
        return Err(ConnectError {
            server: input.server.to_string(),
            message: format!("Invalid MCP command for \"{}\"", input.server),
        }
        .into());
    };
    let cwd = match &local.cwd {
        Some(cwd) => normalize_path(&std::path::Path::new(input.directory).join(cwd)),
        None => input.directory.to_string(),
    };
    // G-1 residual — spawn through the Environment stdio seam (the
    // location-bound driver) instead of a host-bound Command; the local
    // driver reproduces the previous spawn byte-for-byte (`extend_env`
    // merges the host environment beneath the overlay).
    let spawner = input.stdio.clone().unwrap_or_else(|| {
        Arc::new(crate::environment::stdio::LocalStdioSpawner) as Arc<dyn StdioSpawner>
    });
    let spec = StdioSpec {
        command: command.clone(),
        args: args.to_vec(),
        cwd,
        env: stdio_overlay(command, local.environment.as_ref()),
        extend_env: true,
    };
    let PipedProcess { stdin, stdout, stop, stderr_tail } = spawner
        .spawn_piped(spec)
        .await
        .map_err(|error| connect_failure(input.server, &StdioSpawnFailure(error)))?;
    // G-72: rmcp's codec owns the framing, so the ref's 16 MiB frame cap
    // (mcp/stdio.ts:15,121) is enforced at this seam — the reader we hand
    // the transport — as a fail-fast io::Error on an oversized line.
    let stdout = FrameCapReader::new(stdout);
    let stdout = StopOnDropReader { inner: Box::new(stdout), stop: Some(stop) };
    // rmcp's async-rw transport over (read, write) — the same plumbing
    // TokioChildProcess wrapped internally.
    match start(input, (stdout, stdin), false, Arc::new(std::sync::atomic::AtomicBool::new(false)))
        .await
    {
        Ok(connection) => Ok(connection),
        Err(failure) => {
            // G-72 crash diagnostics (ref mcp/stdio.ts:165-180): the
            // process's last stderr bytes usually name the cause of an
            // exit-during-handshake; give the drain task a short grace
            // (the ref waits EXIT_WAIT on the exit code) and append the
            // tail to the failure text.
            let tail = stderr_tail.as_ref().map(|tail| async {
                let deadline =
                    tokio::time::Instant::now() + tokio::time::Duration::from_millis(300);
                loop {
                    let snapshot = tail.snapshot();
                    if !snapshot.is_empty() || deadline < tokio::time::Instant::now() {
                        return snapshot;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                }
            });
            let tail = match tail {
                Some(tail) => {
                    let snapshot = tail.await;
                    if snapshot.is_empty() {
                        None
                    } else {
                        Some(snapshot)
                    }
                }
                None => None,
            };
            match tail {
                Some(tail) => Err(ConnectError {
                    server: input.server.to_string(),
                    message: format!("{failure}: {tail}"),
                }
                .into()),
                None => Err(failure),
            }
        }
    }
}

async fn connect_remote(
    input: &ConnectInput<'_>,
    remote: &oc_schema::mcp::RemoteConfig,
) -> Result<Connection, ConnectFailure> {
    let Ok(mut url) = reqwest::Url::parse(&remote.url) else {
        return Err(ConnectError {
            server: input.server.to_string(),
            message: format!("Invalid MCP URL for \"{}\"", input.server),
        }
        .into());
    };
    // Servers bundling their own Code Mode expose raw tools when asked with
    // `?codemode=false`; the configured URL stays the OAuth identity.
    let mut added_codemode = remote.codemode != Some(false);
    if url.query_pairs().any(|(key, _)| key == "codemode") {
        added_codemode = false;
    }
    if added_codemode {
        url.query_pairs_mut().append_pair("codemode", "false");
    }
    // Bearer + configured headers ride as reqwest default headers (a
    // credential switch reconnects and rebuilds them).
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in remote.headers.iter().flatten() {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_bytes()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            headers.insert(name, value);
        }
    }
    if let Some(bearer) = &input.bearer {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {bearer}")) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }
    // rmcp 3.7's built-in `StreamableHttpClient` impl targets reqwest 0.13
    // while the workspace stays on 0.12; both wrap the same `http` crate,
    // so one `HeaderMap` feeds the pre-flight (0.12) and the transport
    // (0.13) identically.
    let preflight_client = oc_http::builder()
        .default_headers(headers.clone())
        .build()
        .map_err(|error| connect_failure(input.server, &error))?;
    let client = reqwest13::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|error| connect_failure(input.server, &error))?;
    // PLUG-D30: the needs-auth signal. rmcp's streamable worker swallows
    // the initialize 401 into a `connection closed` quit (the status never
    // reaches the caller), so the connect pre-flights the same initialize
    // v2's SDK issues — its 401 surfaces directly as `NeedsAuthError`
    // there. A 401/403 answer settles `needs_auth` with the server's own
    // challenge text; every other outcome falls through to the real
    // handshake (transient network failures included — the rmcp attempt
    // owns those retries).
    if let Some(message) = preflight_challenge(&preflight_client, &url).await {
        return Err(NeedsAuthError {
            server: input.server.to_string(),
            message: format!("HTTP {message}"),
        }
        .into());
    }
    // Every attempt opens a fresh transport; one retry at the pristine URL
    // when the codemode param drew a 400/404, plus transient retries (250ms
    // then 1s — `max([exponential(250ms, 4), recurs(2)])`, G-280①) while
    // the startup deadline has not passed.
    let deadline = tokio::time::Instant::now()
        + startup_timeout(&ServerConfig::Remote(Box::new(remote.clone())));
    let mut delays: Vec<Duration> = vec![Duration::from_millis(250), Duration::from_millis(1000)];
    let open = |url: reqwest::Url| {
        let client = client.clone();
        async move {
            // G-223: the observing client records whether the transport
            // ever attached a negotiated `Mcp-Session-Id` (the ref's
            // expiry gate); delegation keeps rmcp's reqwest behavior
            // (SSE size caps included) byte-for-byte.
            let observing = SessionObservingClient {
                inner: client,
                session_attached: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            };
            let session_attached = Arc::clone(&observing.session_attached);
            let transport = StreamableHttpClientTransport::<SessionObservingClient>::with_client(
                observing,
                // rmcp 0.5 had no transparent 404 session recovery — the
                // registry's expiry handling owns reconnect+replay, so the
                // 3.7 default (`true`) must stay off.
                StreamableHttpClientTransportConfig::with_uri(url.to_string())
                    .reinit_on_expired_session(false),
            );
            start(input, transport, true, session_attached).await
        }
    };
    let mut attempt = open(url.clone());
    loop {
        match attempt.await {
            Ok(connection) => return Ok(connection),
            Err(failure) => {
                let chain = vec![failure.to_string()];
                let pristine_retry = added_codemode
                    && matches!(&failure, ConnectFailure::Connect(error) if error_400_404(error));
                if pristine_retry {
                    let pristine = reqwest::Url::parse(&remote.url).map_err(|_| ConnectError {
                        server: input.server.to_string(),
                        message: format!("Invalid MCP URL for \"{}\"", input.server),
                    })?;
                    attempt = open(pristine);
                    added_codemode = false;
                    continue;
                }
                let transient =
                    matches!(&failure, ConnectFailure::Connect(_)) && is_transient(&chain);
                if transient && !delays.is_empty() && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(delays.remove(0)).await;
                    attempt = open(url.clone());
                    continue;
                }
                return Err(failure);
            }
        }
    }
}

fn error_400_404(error: &ConnectError) -> bool {
    let message = &error.message;
    message.contains("400") || message.contains("404") || message.contains("Bad Request")
}

/// The connect-time initialize probe (PLUG-D30): `Some(status text)` when
/// the resource server answered 401/403 — the challenge the registry
/// settles `needs_auth` on. The probe carries the SAME headers the
/// transport would (bearer included), so it classifies exactly the
/// handshake the rmcp attempt is about to make.
async fn preflight_challenge(client: &reqwest::Client, url: &reqwest::Url) -> Option<String> {
    let attempt = async {
        let response = client
            .post(url.clone())
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 0,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "clientInfo": { "name": "opencode" },
                    },
                })
                .to_string(),
            )
            .send()
            .await
            .ok()?;
        let status = response.status();
        if status.as_u16() != 401 && status.as_u16() != 403 {
            return None;
        }
        let challenge = response
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        Some(match challenge {
            Some(challenge) => format!("{status} Unauthorized: {challenge}"),
            None => format!("{status} Unauthorized"),
        })
    };
    tokio::time::timeout(Duration::from_secs(10), attempt).await.ok().flatten()
}

fn connect_failure(server: &str, error: &(dyn std::error::Error + 'static)) -> ConnectFailure {
    // G-223: typed 401/403 detection (see `unauthorized`).
    if unauthorized(error) {
        let chain = error_chain(error);
        return NeedsAuthError { server: server.to_string(), message: chain.join(": ") }.into();
    }
    ConnectError { server: server.to_string(), message: error_chain(error).join(": ") }.into()
}

/// The `UnsupportedProtocolVersionError` mapping (client.ts:376-380): a
/// pin (or discover negotiation) the server cannot satisfy fails with
/// v2's remediation hint naming the server's supported revisions.
fn unsupported_protocol_message(
    server: &str,
    error: &rmcp::service::ClientInitializeError,
) -> Option<String> {
    let rmcp::service::ClientInitializeError::NoCompatibleProtocolVersion {
        client_supported,
        server_supported,
    } = error
    else {
        return None;
    };
    let supported = server_supported
        .iter()
        .map(rmcp::model::ProtocolVersion::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let offered = client_supported
        .iter()
        .map(rmcp::model::ProtocolVersion::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "Unsupported protocol version for \"{server}\": the client offered {offered}, \
         the server supports {supported}. \
         Set \"protocol\" for this server to one of those or to \"legacy\"."
    ))
}

fn normalize_path(path: &std::path::Path) -> String {
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

async fn start<T, E, A>(
    input: &ConnectInput<'_>,
    transport: T,
    remote: bool,
    session_attached: Arc<std::sync::atomic::AtomicBool>,
) -> Result<Connection, ConnectFailure>
where
    T: rmcp::transport::IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let changed = Arc::new(Mutex::new(Changed::default()));
    // v2 declares roots alongside elicitation (and for legacy servers
    // that refuse to run without one); rmcp always answers roots/list,
    // so declare unconditionally. (Roots are SEP-2577-deprecated in
    // rmcp 3.x; declaring stays the pre-existing behavior.)
    //
    // client.ts:188 — `elicitation: { form: {...}, url: {} }`, both
    // sub-capabilities present whenever the handler is. rmcp's form
    // sub-capability models only `schemaValidation`, so the
    // `applyDefaults: true` hint v2 sends cannot ride it (see the
    // module deviations).
    let capabilities = if input.elicitation.is_some() {
        #[allow(deprecated)]
        ClientCapabilities::builder()
            .enable_roots()
            .enable_elicitation_with(
                rmcp::model::ElicitationCapability::new()
                    .with_form(rmcp::model::FormElicitationCapability::new())
                    .with_url(rmcp::model::UrlElicitationCapability::new()),
            )
            .build()
    } else {
        #[allow(deprecated)]
        ClientCapabilities::builder().enable_roots().build()
    };
    let handler = Handler {
        capabilities,
        implementation: rmcp::model::Implementation::new("opencode", "unknown"),
        roots_directory: input.directory.to_string(),
        changed: Arc::clone(&changed),
        server: Arc::from(input.server),
        elicitation: input.elicitation.clone(),
    };
    let startup = startup_timeout(input.config);
    let outcome = tokio::time::timeout(
        startup,
        handler.serve_with_lifecycle(transport, lifecycle_mode(input.config)),
    )
    .await;
    let service = match outcome {
        Ok(Ok(service)) => service,
        Ok(Err(error)) => {
            if let Some(message) = unsupported_protocol_message(input.server, &error) {
                return Err(ConnectError { server: input.server.to_string(), message }.into());
            }
            return Err(connect_failure(input.server, &error));
        }
        Err(_) => {
            return Err(ConnectError {
                server: input.server.to_string(),
                message: format!("Timed out after {}ms", startup.as_millis()),
            }
            .into());
        }
    };
    let peer_info = service.peer_info();
    let peer = service.peer().clone();
    Ok(Connection {
        server: input.server.into(),
        peer,
        peer_info,
        service: Arc::new(tokio::sync::Mutex::new(Some(service))),
        changed,
        catalog_timeout: catalog_timeout(input.config),
        execution_timeout: execution_timeout(input.config),
        remote,
        expiry_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        session_attached,
        elicitation: input.elicitation.clone(),
    })
}
