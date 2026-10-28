//! MCP client — port of `mcp/client.ts` (core-runner-map §5).
//!
//! Wraps an [`rmcp`] client connection behind the same operation surface v2
//! exposes (`tools`/`prompts`/`resources`/`readResource`/`prompt`/`callTool`
//! plus close / list-changed callbacks), with:
//!
//! - **timeouts** — startup 30s, catalog (discovery) 30s, execution 12h,
//!   terminate 1s (`config.timeout` overrides);
//! - **transient retry** — `max(exponential 250ms ×4, 2 recursions)` while
//!   the failure looks transient (HTTP 408/429/5xx except 501, network
//!   errors carrying an OS error code);
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
//! - Protocol negotiation: v2.0.24's TS client offers `2025-11-25`
//!   (its `SUPPORTED_PROTOCOL_VERSIONS[0]`, the legacy initialize
//!   handshake); the pin matches that offered version, and rmcp's own
//!   server-answer acceptance semantics decide the negotiated revision.
//!   `config.protocol` stays advisory (tracked for the SDK swap).
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
use oc_schema::mcp::ServerConfig;
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ContentBlock, GetPromptRequestParams,
    Implementation, InitializeRequestParams, ListPromptsRequest, ListResourceTemplatesRequest,
    ListResourcesRequest, ListToolsRequest, ProtocolVersion, ReadResourceRequestParams,
    RequestMetaObject, ServerPeerInfo, ServerResult,
};
use rmcp::service::{
    NotificationContext, Peer, PeerRequestOptions, RequestContext, RoleClient, RunningService,
};
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use rmcp::transport::TokioChildProcess;
use rmcp::{ClientHandler, ServiceExt};
use thiserror::Error;

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

/// HTTP statuses found anywhere in the error chain (reqwest renders them as
/// `"HTTP status client error (429 Too Many Requests)"`).
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

/// Whether any error in the chain is a 401/403 authentication rejection.
fn unauthorized(chain: &[String]) -> bool {
    http_statuses(chain).iter().any(|status| *status == 401 || *status == 403)
        || chain
            .iter()
            .any(|text| text.contains("401 Unauthorized") || text.contains("403 Forbidden"))
}

/// Whether the chain shows a 404, or the 400 "Bad Request: Server not
/// initialized" a freshly restarted single-session server emits.
fn session_expired(chain: &[String]) -> bool {
    http_statuses(chain).contains(&404)
        || chain.iter().any(|text| {
            text.contains("400 Bad Request") && text.contains("Bad Request: Server not initialized")
        })
}

type BoxedRequest<T> = Pin<Box<dyn Future<Output = Result<T, RequestError>> + Send>>;

/// The transient-retry loop (client.ts:170-182): up to two more attempts,
/// 250ms and 500ms later (exponential base 250ms ×4, `recurs(2)`), only
/// while the failure is transient.
async fn retrying<T>(mut attempt: impl FnMut() -> BoxedRequest<T>) -> Result<T, RequestError> {
    const DELAYS_MS: [u64; 2] = [250, 500];
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

    /// Maps one rmcp failure to [`RequestError`], classifying session
    /// expiry (once) and authentication rejections.
    fn classify(&self, error: &rmcp::ServiceError) -> RequestError {
        let chain = error_chain(error);
        if self.remote && !self.expiry_reported() && session_expired(&chain) {
            self.expiry_flag.store(true, std::sync::atomic::Ordering::SeqCst);
            return RequestError {
                server: self.server_name().to_string(),
                message: format!("{SESSION_EXPIRED_MESSAGE}{}", self.server_name()),
            };
        }
        if unauthorized(&chain) {
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
    pub async fn tools(&self) -> Result<Vec<rmcp::model::Tool>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let result = this
                    .call(
                        rmcp::model::ClientRequest::ListToolsRequest(ListToolsRequest::default()),
                        timeout,
                    )
                    .await
                    .map_err(|error| this.request_error("list MCP tools", error))?;
                match result {
                    ServerResult::ListToolsResult(result) => Ok(result.tools),
                    _ => Err(RequestError {
                        server: this.server_name().to_string(),
                        message: "unexpected tools/list response".to_string(),
                    }),
                }
            })
            .boxed()
        })
        .await
    }

    /// `prompts()`.
    pub async fn prompts(&self) -> Result<Vec<rmcp::model::Prompt>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let result = this
                    .call(
                        rmcp::model::ClientRequest::ListPromptsRequest(
                            ListPromptsRequest::default(),
                        ),
                        timeout,
                    )
                    .await
                    .map_err(|error| this.request_error("list MCP prompts", error))?;
                match result {
                    ServerResult::ListPromptsResult(result) => Ok(result.prompts),
                    _ => Err(RequestError {
                        server: this.server_name().to_string(),
                        message: "unexpected prompts/list response".to_string(),
                    }),
                }
            })
            .boxed()
        })
        .await
    }

    /// `resources()`.
    pub async fn resources(&self) -> Result<Vec<rmcp::model::Resource>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let result = this
                    .call(
                        rmcp::model::ClientRequest::ListResourcesRequest(
                            ListResourcesRequest::default(),
                        ),
                        timeout,
                    )
                    .await
                    .map_err(|error| this.request_error("list MCP resources", error))?;
                match result {
                    ServerResult::ListResourcesResult(result) => Ok(result.resources),
                    _ => Err(RequestError {
                        server: this.server_name().to_string(),
                        message: "unexpected resources/list response".to_string(),
                    }),
                }
            })
            .boxed()
        })
        .await
    }

    /// `resourceTemplates()` — some servers declare resources without
    /// implementing template listing; callers collapse failures to empty.
    pub async fn resource_templates(
        &self,
    ) -> Result<Vec<rmcp::model::ResourceTemplate>, RequestError> {
        let timeout = self.catalog_timeout;
        let this = self.clone();
        retrying(move || {
            let this = this.clone();
            Box::pin(async move {
                let result = this
                    .call(
                        rmcp::model::ClientRequest::ListResourceTemplatesRequest(
                            ListResourceTemplatesRequest::default(),
                        ),
                        timeout,
                    )
                    .await
                    .map_err(|error| this.request_error("list MCP resource templates", error))?;
                match result {
                    ServerResult::ListResourceTemplatesResult(result) => {
                        Ok(result.resource_templates)
                    }
                    _ => Err(RequestError {
                        server: this.server_name().to_string(),
                        message: "unexpected resources/templates/list response".to_string(),
                    }),
                }
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
    start(input, transport, remote).await
}

/// Environment for a stdio MCP server, mirroring the ref's local spawner
/// semantics (`mcp/stdio.ts` passes `extendEnv: true`, and the local
/// cross-spawn spawner flattens that to `{ ...process.env, ...opts.env }`):
/// the child inherits the host environment in full — nothing is stripped,
/// `PATH`/`HOME` included — with `BUN_BE_BUN=1` set for `opencode`
/// commands (so nested spawns resolve to the same runtime) and the server
/// config's `environment` entries overriding host values on collision.
/// Order: host env < `BUN_BE_BUN` < config `environment`.
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
    let env = stdio_environment(std::env::vars(), command, local.environment.as_ref());
    let mut builder = tokio::process::Command::new(command);
    builder.args(args).current_dir(&cwd).env_clear().envs(&env);
    #[cfg(unix)]
    {
        builder.process_group(0);
    }
    let transport =
        TokioChildProcess::new(builder).map_err(|error| connect_failure(input.server, &error))?;
    start(input, transport, false).await
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
    let preflight_client = reqwest::Client::builder()
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
    // when the codemode param drew a 400/404, plus transient retries while
    // the startup deadline has not passed.
    let deadline = tokio::time::Instant::now()
        + startup_timeout(&ServerConfig::Remote(Box::new(remote.clone())));
    let mut delays: Vec<Duration> = vec![Duration::from_millis(250), Duration::from_millis(500)];
    let open = |url: reqwest::Url| {
        let client = client.clone();
        async move {
            let transport = StreamableHttpClientTransport::<reqwest13::Client>::with_client(
                client,
                // rmcp 0.5 had no transparent 404 session recovery — the
                // registry's expiry handling owns reconnect+replay, so the
                // 3.7 default (`true`) must stay off.
                StreamableHttpClientTransportConfig::with_uri(url.to_string())
                    .reinit_on_expired_session(false),
            );
            start(input, transport, true).await
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

fn connect_failure(server: &str, error: &dyn std::error::Error) -> ConnectFailure {
    let chain = error_chain(error);
    if unauthorized(&chain) {
        return NeedsAuthError { server: server.to_string(), message: chain.join(": ") }.into();
    }
    ConnectError { server: server.to_string(), message: chain.join(": ") }.into()
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
    let outcome = tokio::time::timeout(startup, handler.serve(transport)).await;
    let service = match outcome {
        Ok(Ok(service)) => service,
        Ok(Err(error)) => return Err(connect_failure(input.server, &error)),
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
        elicitation: input.elicitation.clone(),
    })
}
