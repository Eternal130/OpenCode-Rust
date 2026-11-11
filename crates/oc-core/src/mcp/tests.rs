//! MCP lane tests — ports of the `test/mcp*.test.ts` behaviors exercised
//! against an in-process mock MCP server speaking JSON-RPC over a
//! [`Transport`](rmcp::transport::Transport) (rmcp's server feature is not
//! in the dependency set, so the suites test through mock transports),
//! plus registry / elicitation / tool-bridge coverage over a scripted
//! connector.
#![allow(
    clippy::default_trait_access,
    clippy::implied_bounds_in_impls,
    clippy::items_after_statements,
    clippy::wildcard_enum_match_arm
)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use oc_schema::mcp::{LocalConfig, ServerConfig, Status};
use rmcp::model::{
    ClientJsonRpcMessage, ClientRequest, JsonRpcMessage, RequestId, ServerJsonRpcMessage,
    ServerResult,
};
use rmcp::service::RoleClient;
use rmcp::transport::Transport;
use std::future::Future;
use tokio::sync::mpsc;

use super::client::{self, ConnectInput};
use super::elicitation::{self, ElicitationParams, FormService, RegistryElicitation};
use super::registry::{
    integration_suffix, namespace, tool_name, unavailable, Connector, McpRegistry, NoCredentials,
};

// ---------------------------------------------------------------------------
// mock server
// ---------------------------------------------------------------------------

/// What the scripted server answers. Catalogs are shared so tests can
/// mutate them live and push `tools/list_changed`.
#[derive(Default, Clone)]
pub(crate) struct Script {
    tools: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    resources: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    templates: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    calls: BTreeMap<String, serde_json::Value>,
    call_errors: BTreeMap<String, String>,
    instructions: Option<String>,
    /// The initialize requests the client offered (protocol-offer probe).
    offers: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    /// The `server/discover` probes the client sent (protocol
    /// negotiation): raw request JSON — the `_meta.protocolVersion`
    /// names the offered revision.
    discovers: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    /// `server/discover` answer: the versions a modern server declares,
    /// or `None` = a legacy server answering method-not-found.
    discover_supported: Option<Vec<rmcp::model::ProtocolVersion>>,
    /// The `protocolVersion` the initialize answer echoes (defaults to
    /// echoing the client's offer).
    initialize_version: Option<&'static str>,
    /// Tools that elicit before answering: tool → `elicitation/create`
    /// params (the ref servers' `tools/call` → `elicitInput` flow).
    elicit_on_call: BTreeMap<String, serde_json::Value>,
    /// Modern-era MRTR (SEP-2322): tools whose FIRST `tools/call`
    /// answer is the given `input_required` payload; the retry (the
    /// request carrying `inputResponses`) answers normally, embedding
    /// the responses + echoed `requestState` (mcp.test.ts:164-176).
    input_required: BTreeMap<String, serde_json::Value>,
    /// Emit a 404 on the next call-tool (session-expiry probe), then behave.
    expire_once: Arc<std::sync::atomic::AtomicBool>,
    /// Emit the expiry rejection on the next tools/LIST (background
    /// session-expiry probe — list-changed refreshes observe it), then
    /// behave.
    expire_list_once: Arc<std::sync::atomic::AtomicBool>,
    /// G-358 — when set, list answers page over the catalog with this
    /// many items per page (cursor = the next start index as a string).
    page_size: Option<usize>,
    /// G-360① — the serialized `tools/call` params of every call
    /// (asserts the `sessionID` request metadata).
    call_metas: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
}

impl Script {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn tool(self, name: &str, description: &str, output_schema: Option<serde_json::Value>) -> Self {
        let mut tool = serde_json::json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": {} },
        });
        if let Some(schema) = output_schema {
            tool["outputSchema"] = schema;
        }
        self.tools.lock().push(tool);
        self
    }

    /// Adds a tool without consuming (live-catalog mutation helper).
    fn add_tool(&self, name: &str, description: &str) {
        self.tools
            .lock()
            .push(serde_json::json!({ "name": name, "description": description, "inputSchema": { "type": "object" } }));
    }
}

/// Handle for driving the mock server after connect.
struct Mock {
    /// Push a raw server message (notifications / server requests).
    push: mpsc::UnboundedSender<ServerJsonRpcMessage>,
    /// The client's answers to server-initiated requests.
    answers: mpsc::UnboundedReceiver<ClientJsonRpcMessage>,
}

impl Mock {
    fn request_roots(&self) {
        // Roots are SEP-2577-deprecated in rmcp 3.x; the scripted server
        // still pushes `roots/list` (pre-existing test behavior).
        #[allow(deprecated)]
        let request = rmcp::model::ListRootsRequest {
            method: Default::default(),
            extensions: Default::default(),
        };
        let _ = self.push.send(JsonRpcMessage::request(
            rmcp::model::ServerRequest::ListRootsRequest(request),
            RequestId::Number(9001),
        ));
    }

    /// Pushes `elicitation/create` as a server-initiated request.
    fn request_elicitation(&self, id: i64, params: serde_json::Value) {
        let request = rmcp::model::ElicitRequest::new(
            serde_json::from_value(params).expect("elicitation params parse"),
        );
        let _ = self.push.send(JsonRpcMessage::request(
            rmcp::model::ServerRequest::ElicitRequest(request),
            RequestId::Number(id),
        ));
    }

    /// Pushes legacy `notifications/elicitation/complete`.
    fn notify_elicitation_complete(&self, elicitation_id: &str) {
        let _ = self.push.send(JsonRpcMessage::notification(
            rmcp::model::ServerNotification::CustomNotification(
                rmcp::model::CustomNotification::new(
                    "notifications/elicitation/complete",
                    Some(serde_json::json!({ "elicitationId": elicitation_id })),
                ),
            ),
        ));
    }

    /// Cancels a server-initiated request (`notifications/cancelled`).
    fn cancel_request(&self, id: i64) {
        let _ = self.push.send(JsonRpcMessage::notification(
            rmcp::model::ServerNotification::CancelledNotification(
                rmcp::model::CancelledNotification::new(
                    rmcp::model::CancelledNotificationParam::new(Some(RequestId::Number(id)), None),
                ),
            ),
        ));
    }
}

/// Client-role transport: sends to the scripted server, receives merged
/// (scripted responses + manual pushes).
struct MockTransport {
    to_server: mpsc::UnboundedSender<ClientJsonRpcMessage>,
    from_server: mpsc::UnboundedReceiver<ServerJsonRpcMessage>,
}

// The rmcp `Transport` trait declares `+ Send + 'static` on some RPITIT
// methods and plain `+ Send` on others; matching the declaration exactly
// triggers the implied-bounds lint, so the impl keeps the explicit bounds.
#[allow(clippy::implied_bounds_in_impls)]
impl Transport<RoleClient> for MockTransport {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: ClientJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let send = self.to_server.send(item);
        async move { send.map_err(|_| std::io::Error::other("mock server dropped")) }
    }

    fn receive(&mut self) -> impl Future<Output = Option<ServerJsonRpcMessage>> + Send {
        self.from_server.recv()
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        std::future::ready(Ok(()))
    }
}

fn result_of(value: serde_json::Value) -> ServerResult {
    serde_json::from_value(value).expect("valid server result payload")
}

/// Spawns the scripted server and returns the transport-side channels.
fn mock_pair(script: Script) -> (MockTransport, Mock) {
    let (to_server_tx, mut to_server_rx) = mpsc::unbounded_channel::<ClientJsonRpcMessage>();
    let (from_server_tx, from_server_rx) = mpsc::unbounded_channel::<ServerJsonRpcMessage>();
    let push_handle = from_server_tx.clone();
    let (answers_tx, answers_rx) = mpsc::unbounded_channel::<ClientJsonRpcMessage>();
    tokio::spawn(async move {
        while let Some(message) = to_server_rx.recv().await {
            match message {
                JsonRpcMessage::Request(request) => {
                    let id = request.id;
                    // The ref servers elicit inside `tools/call`
                    // (`elicitInput`, mcp.test.ts:140-185): the call is
                    // answered only after the client answered the
                    // elicitation, embedding the result like the TS mock.
                    let elicit_params = match &request.request {
                        ClientRequest::CallToolRequest(call) => {
                            script.elicit_on_call.get(call.params.name.as_ref()).cloned()
                        }
                        _ => None,
                    };
                    if let Some(params) = elicit_params {
                        let elicit_id = RequestId::Number(7001);
                        let elicit = rmcp::model::ElicitRequest::new(
                            serde_json::from_value(params).expect("elicitation params parse"),
                        );
                        let _ = from_server_tx.send(JsonRpcMessage::request(
                            rmcp::model::ServerRequest::ElicitRequest(elicit),
                            elicit_id.clone(),
                        ));
                        // Hold the tools/call answer until the elicitation
                        // resolves; other client messages keep flowing
                        // through their usual paths.
                        let answer = loop {
                            match to_server_rx.recv().await {
                                Some(JsonRpcMessage::Response(response))
                                    if response.id == elicit_id =>
                                {
                                    break response.result;
                                }
                                Some(JsonRpcMessage::Response(response)) => {
                                    let _ = answers_tx.send(JsonRpcMessage::Response(response));
                                }
                                Some(_) => {}
                                None => return,
                            }
                        };
                        let _ = from_server_tx.send(JsonRpcMessage::response(
                            result_of(serde_json::json!({
                                "content": [{
                                    "type": "text",
                                    "text": serde_json::to_string(&answer).unwrap_or_default(),
                                }],
                                "structuredContent": answer,
                            })),
                            id,
                        ));
                        continue;
                    }
                    if let Some(response) = respond(&request.request, id, &script) {
                        let _ = from_server_tx.send(response);
                    }
                }
                // The client answering a server-initiated request.
                JsonRpcMessage::Response(_) => {
                    let _ = answers_tx.send(message);
                }
                _ => {}
            }
        }
    });
    (
        MockTransport { to_server: to_server_tx, from_server: from_server_rx },
        Mock { push: push_handle, answers: answers_rx },
    )
}

/// G-358 — one `tools/list`-shaped page over `items`: `cursor` carries
/// the next start index; the answer includes `nextCursor` while items
/// remain (index-cursor paging like the SDK's example servers).
fn paged(
    items: &[serde_json::Value],
    cursor: Option<&str>,
    page_size: usize,
    key: &str,
) -> serde_json::Value {
    let start = cursor.and_then(|cursor| cursor.parse::<usize>().ok()).unwrap_or(0);
    let end = (start + page_size).max(start).min(items.len());
    let mut value = serde_json::json!({ key: &items[start..end] });
    if end < items.len() {
        value["nextCursor"] = serde_json::json!(end.to_string());
    }
    value
}

#[allow(clippy::unnecessary_wraps)] // the `_` request arm has no response
#[allow(clippy::too_many_lines)] // scripted mock responder; 1:1 with the fixture scripts
fn respond(
    request: &ClientRequest,
    id: RequestId,
    script: &Script,
) -> Option<ServerJsonRpcMessage> {
    fn response(value: serde_json::Value, id: RequestId) -> Option<ServerJsonRpcMessage> {
        Some(JsonRpcMessage::response(result_of(value), id))
    }
    match request {
        ClientRequest::InitializeRequest(request) => {
            script
                .offers
                .lock()
                .push(serde_json::to_value(request).expect("initialize offer serializes"));
            let protocol_version = script.initialize_version.map_or_else(
                || rmcp::model::ProtocolVersion::V_2025_11_25.to_string(),
                str::to_owned,
            );
            response(
                serde_json::json!({
                        // The mock echoes the client's offered version back —
                        // v2.0.24's TS client offers `2025-11-25` (its
                        // SUPPORTED_PROTOCOL_VERSIONS[0], legacy handshake).
                        "protocolVersion": protocol_version,
                        "capabilities": {
                            "tools": { "listChanged": true },
                            "resources": { "subscribe": true, "listChanged": true },
                            "prompts": { "listChanged": true },
                        },
                        "serverInfo": { "name": "mock", "version": "1" },
                    "instructions": script.instructions,
                }),
                id,
            )
        }
        ClientRequest::DiscoverRequest(request) => {
            script
                .discovers
                .lock()
                .push(serde_json::to_value(request).expect("discover probe serializes"));
            let Some(supported) = script.discover_supported.clone() else {
                // A legacy server: correlated error, not a modern-era
                // rejection — the client falls back to initialize.
                return Some(JsonRpcMessage::error(
                    rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                        "server/discover not found",
                        None,
                    ),
                    Some(id),
                ));
            };
            let mut result = rmcp::model::DiscoverResult::new(supported, Default::default());
            result.set_server_info(rmcp::model::Implementation::new("mock", "1"));
            Some(JsonRpcMessage::response(ServerResult::DiscoverResult(result), id))
        }
        ClientRequest::ListToolsRequest(request) => {
            if script.expire_list_once.swap(false, std::sync::atomic::Ordering::SeqCst) {
                // Same single-session expiry rejection as `expire_once`,
                // observed by a BACKGROUND catalog refresh.
                return Some(JsonRpcMessage::error(
                    rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::INTERNAL_ERROR,
                        "Bad Request: Server not initialized",
                        None,
                    ),
                    Some(id),
                ));
            }
            let cursor = request.params.as_ref().and_then(|params| params.cursor.clone());
            match script.page_size {
                Some(size) => {
                    response(paged(&script.tools.lock(), cursor.as_deref(), size, "tools"), id)
                }
                None => response(serde_json::json!({ "tools": *script.tools.lock() }), id),
            }
        }
        ClientRequest::ListResourcesRequest(request) => {
            let cursor = request.params.as_ref().and_then(|params| params.cursor.clone());
            match script.page_size {
                Some(size) => response(
                    paged(&script.resources.lock(), cursor.as_deref(), size, "resources"),
                    id,
                ),
                None => response(serde_json::json!({ "resources": *script.resources.lock() }), id),
            }
        }
        ClientRequest::ListResourceTemplatesRequest(request) => {
            let cursor = request.params.as_ref().and_then(|params| params.cursor.clone());
            match script.page_size {
                Some(size) => response(
                    paged(&script.templates.lock(), cursor.as_deref(), size, "resourceTemplates"),
                    id,
                ),
                None => response(
                    serde_json::json!({ "resourceTemplates": *script.templates.lock() }),
                    id,
                ),
            }
        }
        ClientRequest::ListPromptsRequest(request) => {
            let cursor = request.params.as_ref().and_then(|params| params.cursor.clone());
            match script.page_size {
                Some(size) => response(paged(&[], cursor.as_deref(), size, "prompts"), id),
                None => response(serde_json::json!({ "prompts": [] }), id),
            }
        }
        ClientRequest::ReadResourceRequest(request) => {
            let uri = &request.params.uri;
            if uri.starts_with("mock://text") {
                response(
                    serde_json::json!({
                        "contents": [{ "uri": uri, "mimeType": "text/plain", "text": "resource body" }]
                    }),
                    id,
                )
            } else {
                response(
                    serde_json::json!({
                        "contents": [{ "uri": uri, "mimeType": "image/png", "blob": "aGk=" }]
                    }),
                    id,
                )
            }
        }
        ClientRequest::GetPromptRequest(_) => {
            response(serde_json::json!({ "description": "a prompt", "messages": [] }), id)
        }
        ClientRequest::CallToolRequest(request) => {
            let name = request.params.name.as_ref();
            // G-360① — record the request `_meta` (deserialization strips
            // `params._meta` into the envelope extensions; the sessionID
            // rides there on the mock side).
            let meta = request.extensions.get::<rmcp::model::RequestMetaObject>();
            script.call_metas.lock().push(serde_json::to_value(meta).unwrap_or_default());
            if let Some(required) = script.input_required.get(name) {
                if request.params.input_responses.is_none() {
                    return response(required.clone(), id);
                }
                // The retry: echo what the client sent back.
                let responses = serde_json::to_value(&request.params.input_responses)
                    .unwrap_or(serde_json::Value::Null);
                return response(
                    serde_json::json!({
                        "content": [{ "type": "text", "text": "retried" }],
                        "structuredContent": {
                            "inputResponses": responses,
                            "requestState": request.params.request_state,
                        },
                    }),
                    id,
                );
            }
            if script.expire_once.swap(false, std::sync::atomic::Ordering::SeqCst) {
                // G-223: the restarted single-session server's JSON-RPC
                // rejection (ref client.ts:234-239) — the expiry signal a
                // mock transport CAN produce (the typed 404 variant only
                // comes from the real streamable transport). The caller
                // must mark the connection session-attached for the
                // classifier to accept it.
                return Some(JsonRpcMessage::error(
                    rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::INTERNAL_ERROR,
                        "Bad Request: Server not initialized",
                        None,
                    ),
                    Some(id),
                ));
            }
            if let Some(error) = script.call_errors.get(name) {
                return Some(JsonRpcMessage::error(
                    rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::INTERNAL_ERROR,
                        error.clone(),
                        None,
                    ),
                    Some(id),
                ));
            }
            match script.calls.get(name) {
                Some(result) => response(result.clone(), id),
                None => response(serde_json::json!({ "content": [], "isError": false }), id),
            }
        }
        _ => None,
    }
}

pub(crate) fn local_config() -> ServerConfig {
    ServerConfig::Local(Box::new(LocalConfig {
        command: vec!["mock".to_string()],
        cwd: None,
        environment: None,
        disabled: None,
        codemode: None,
        timeout: None,
        protocol: None,
    }))
}

/// Connects the real client over a mock transport.
async fn connect_mock(script: Script) -> (client::Connection, Mock) {
    connect_mock_with(script, None).await
}

/// Connects against an explicit server config (protocol-negotiation
/// legs vary `config.protocol`).
async fn connect_mock_config(
    script: Script,
    config: &ServerConfig,
) -> Result<client::Connection, client::ConnectFailure> {
    let (transport, _mock) = mock_pair(script);
    let input = ConnectInput {
        server: "mock",
        config,
        directory: "/tmp",
        bearer: None,
        elicitation: None,
        stdio: None,
    };
    client::connect_with_transport(&input, transport, false).await
}

/// Connects with an elicitation handler installed.
async fn connect_mock_with(
    script: Script,
    elicitation: Option<Arc<elicitation::ClientElicitation>>,
) -> (client::Connection, Mock) {
    let (transport, mock) = mock_pair(script);
    let input = ConnectInput {
        server: "mock",
        config: &local_config(),
        directory: "/tmp",
        bearer: None,
        elicitation,
        stdio: None,
    };
    let connection =
        client::connect_with_transport(&input, transport, false).await.expect("mock connect");
    (connection, mock)
}

/// A connector scripted per-call: each invocation connects a fresh mock.
pub(crate) fn scripted_connector(
    next: impl FnMut() -> Script + Send + Sync + 'static,
) -> Connector {
    let next = Arc::new(parking_lot::Mutex::new(next));
    Arc::new(move |input: ConnectInput<'_>| {
        let script = (next.lock())();
        let (transport, _mock) = mock_pair(script);
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                false,
            )
            .await
        })
    })
}

fn failing_connector(message: &'static str) -> Connector {
    Arc::new(move |input: ConnectInput<'_>| {
        let server = input.server.to_string();
        Box::pin(async move {
            Err(client::ConnectFailure::Connect(client::ConnectError {
                server,
                message: message.to_string(),
            }))
        })
    })
}

async fn test_bus() -> (crate::database::Database, crate::bus::Bus) {
    let database = crate::database::Database::open_in_memory().await.expect("database");
    let bus = crate::bus::Bus::new(database.clone(), crate::bus::BusOptions::deterministic());
    (database, bus)
}

fn registry_with(
    bus: &crate::bus::Bus,
    connector: Connector,
    servers: BTreeMap<String, ServerConfig>,
) -> McpRegistry {
    let source = Arc::new(move || servers.clone());
    McpRegistry::with_connector(bus.clone(), connector, Arc::new(NoCredentials))
        .with_config_source(source)
        .with_directory("/tmp")
}

async fn wait_for_status(registry: &McpRegistry, server: &str, want: Status) {
    for _ in 0..200 {
        let found =
            registry.servers().into_iter().find(|info| info.name == server).map(|info| info.status);
        if found.as_ref() == Some(&want) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("server {server} never reached {want:?}: {:?}", registry.servers());
}

/// G-359 — polls until the server settles at ANY `needs_auth` (the
/// error text varies with the refresh failure).
async fn wait_for_needs_auth(registry: &McpRegistry, server: &str) {
    for _ in 0..400 {
        let found = registry.servers().into_iter().find(|info| info.name == server);
        if found.as_ref().is_some_and(|info| matches!(info.status, Status::NeedsAuth { .. })) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("server {server} never reached needs_auth: {:?}", registry.servers());
}

// ---------------------------------------------------------------------------
// client behaviors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_client_lists_tools_and_reads_instructions() {
    let script = Script::new().tool("echo", "Echo tool", None).tool(
        "fetch",
        "Fetch tool",
        Some(serde_json::json!({ "type": "object" })),
    );
    let script = Script { instructions: Some("Use echo carefully.\n".to_string()), ..script };
    let (connection, _mock) = connect_mock(script).await;
    let tools = connection.tools().await.expect("tools");
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, vec!["echo", "fetch"]);
    assert!(tools[1].output_schema.is_some(), "output schema retained across discovery");
    assert_eq!(connection.instructions().as_deref(), Some("Use echo carefully."));
}

#[tokio::test]
async fn mcp_client_paginated_discovery_retains_output_schemas() {
    // rmcp's per-page listing keeps each page's `outputSchema`; the mock
    // returns both tools in one page and the schemas must survive the
    // JSON round trip (v2 test "retains output schemas across paginated
    // MCP discovery").
    let script = Script::new()
        .tool(
            "one",
            "one",
            Some(serde_json::json!({ "type": "object", "properties": { "ok": { "type": "boolean" } } })),
        )
        .tool("two", "two", Some(serde_json::json!({ "type": "string" })));
    let (connection, _mock) = connect_mock(script).await;
    let tools = connection.tools().await.expect("tools");
    assert!(tools.iter().all(|tool| tool.output_schema.is_some()));
}

#[tokio::test]
async fn mcp_client_call_tool_maps_content_and_structured_output() {
    let mut calls = BTreeMap::new();
    calls.insert(
        "echo".to_string(),
        serde_json::json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "image", "data": "aGk=", "mimeType": "image/png" },
                { "type": "resource", "resource": { "uri": "mock://text/1", "mimeType": "text/plain", "text": "embedded" } },
            ],
            "structuredContent": { "answer": 42 },
            "isError": false,
        }),
    );
    let script = Script { calls, ..Script::new() };
    let (connection, _mock) = connect_mock(script).await;
    let result = connection.call_tool("echo", None, Some("ses_1")).await.expect("call");
    assert!(!result.is_error);
    assert_eq!(result.structured, Some(serde_json::json!({ "answer": 42 })));
    assert_eq!(result.content.len(), 3);
    assert_eq!(result.content[0], client::CallToolContent::Text { text: "hello".into() });
    assert!(
        matches!(&result.content[1], client::CallToolContent::Media { data, mime_type } if data == "aGk=" && mime_type == "image/png")
    );
    assert_eq!(result.content[2], client::CallToolContent::Text { text: "embedded".into() });
}

#[tokio::test]
async fn mcp_client_reads_resources_text_and_blob() {
    let script = Script::new();
    let (connection, _mock) = connect_mock(script).await;
    let text = connection.read_resource("mock://text/1").await.expect("read").expect("present");
    assert_eq!(text.contents.len(), 1);
    let blob = connection.read_resource("mock://blob/1").await.expect("read").expect("present");
    assert_eq!(blob.contents.len(), 1);
    // Capability-absent servers return None: the mock advertises
    // resources, so the None path is covered by the registry tests.
}

#[tokio::test]
async fn mcp_client_answers_roots_list_with_location_directory() {
    let (transport, mut mock) = mock_pair(Script::new());
    let input = ConnectInput {
        server: "mock",
        config: &local_config(),
        directory: "/repo space",
        bearer: None,
        elicitation: None,
        stdio: None,
    };
    let connection =
        client::connect_with_transport(&input, transport, false).await.expect("connect");
    mock.request_roots();
    let answer = tokio::time::timeout(Duration::from_secs(2), mock.answers.recv())
        .await
        .expect("roots answer timeout")
        .expect("answer channel");
    let JsonRpcMessage::Response(response) = answer else { panic!("expected response") };
    let value = serde_json::to_value(&response.result).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "roots": [ { "uri": "file:///repo%20space" } ] }),
        "roots/list answers the location directory as a file URL"
    );
    connection.close().await;
}

#[tokio::test]
async fn mcp_client_classifies_session_expiry_once() {
    let mut calls = BTreeMap::new();
    calls.insert(
        "ping".to_string(),
        serde_json::json!({ "content": [{ "type": "text", "text": "pong" }] }),
    );
    let expire = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let script = Script { calls, expire_once: expire, ..Script::new() };
    let (transport, _mock) = mock_pair(script);
    let input = ConnectInput {
        server: "mock",
        config: &local_config(),
        directory: "/tmp",
        bearer: None,
        elicitation: None,
        stdio: None,
    };
    // Remote classification requires remote=true AND (G-223) an attached
    // session — the mock simulates a sessionful remote server.
    let connection =
        client::connect_with_transport(&input, transport, true).await.expect("connect");
    connection.mark_session_attached();
    let first = connection.call_tool("ping", None, None).await;
    let Err(error) = first else { panic!("expected expiry") };
    assert!(
        error.message.starts_with(client::SESSION_EXPIRED_MESSAGE),
        "message: {}",
        error.message
    );
    assert_eq!(error.message, "MCP server session expired: mock");
    assert!(connection.expiry_reported());
    // Subsequent non-transient failures pass through untouched.
    let second = connection.call_tool("ping", None, None).await.expect("recovered call");
    assert!(!second.is_error);
}

#[tokio::test]
async fn mcp_client_offers_v2_protocol_version_2025_11_25() {
    // v2.0.24's TS client offers `2025-11-25`
    // (SUPPORTED_PROTOCOL_VERSIONS[0], the legacy initialize handshake);
    // rmcp's server-answer acceptance semantics stay untouched.
    let script = Script::new();
    let offers = Arc::clone(&script.offers);
    let (_connection, _mock) = connect_mock(script).await;
    let offered = offers.lock();
    assert!(!offered.is_empty(), "initialize offer recorded: {:?}", *offered);
    assert_eq!(
        offered[0].pointer("/params/protocolVersion").and_then(serde_json::Value::as_str),
        Some("2025-11-25"),
        "client offers the v2.0.24 protocol version: {:?}",
        *offered
    );
}

// ---------------------------------------------------------------------------
// protocol negotiation (client.ts:193 + 393-397 negotiation(config.protocol))
// ---------------------------------------------------------------------------

fn protocol_config(protocol: Option<oc_schema::mcp::Protocol>) -> ServerConfig {
    ServerConfig::Local(Box::new(LocalConfig {
        command: vec!["mock".to_string()],
        cwd: None,
        environment: None,
        disabled: None,
        codemode: None,
        timeout: None,
        protocol,
    }))
}

#[tokio::test]
async fn mcp_client_auto_negotiates_modern_discover() {
    // "auto" (client.ts:395): probe server/discover, prefer 2026-07-28.
    let script = Script {
        discover_supported: Some(vec![
            rmcp::model::ProtocolVersion::V_2026_07_28,
            rmcp::model::ProtocolVersion::V_2025_11_25,
        ]),
        ..Script::new()
    };
    let discovers = Arc::clone(&script.discovers);
    let offers = Arc::clone(&script.offers);
    let connection =
        connect_mock_config(script, &protocol_config(Some(oc_schema::mcp::Protocol::Auto)))
            .await
            .expect("modern discover connect");
    assert_eq!(connection.tools().await.expect("tools").len(), 0);
    assert!(offers.lock().is_empty(), "no legacy initialize was sent");
    let discovers = discovers.lock();
    assert_eq!(discovers.len(), 1, "exactly one discover probe");
    assert!(
        serde_json::to_string(&discovers[0]).unwrap_or_default().contains("2026-07-28"),
        "the probe offered 2026-07-28 first: {:?}",
        *discovers
    );
}

#[tokio::test]
async fn mcp_client_auto_negotiates_older_declared_version() {
    // The server declares only 2025-11-25: auto falls back within the
    // supported range instead of failing.
    let script = Script {
        discover_supported: Some(vec![rmcp::model::ProtocolVersion::V_2025_11_25]),
        ..Script::new()
    };
    let connection =
        connect_mock_config(script, &protocol_config(Some(oc_schema::mcp::Protocol::Auto)))
            .await
            .expect("older declared version negotiates");
    assert_eq!(connection.tools().await.expect("tools").len(), 0);
}

#[tokio::test]
async fn mcp_client_auto_falls_back_to_legacy_initialize() {
    // A legacy server answers the discover probe with a correlated
    // error: auto falls back to the plain initialize handshake
    // (client.ts "Absent config is legacy" sibling behavior).
    let script = Script::new(); // discover_supported: None → method-not-found
    let offers = Arc::clone(&script.offers);
    let connection =
        connect_mock_config(script, &protocol_config(Some(oc_schema::mcp::Protocol::Auto)))
            .await
            .expect("legacy fallback connect");
    assert_eq!(connection.tools().await.expect("tools").len(), 0);
    let offers = offers.lock();
    assert_eq!(offers.len(), 1, "the fallback sent one initialize");
    assert_eq!(
        offers[0].pointer("/params/protocolVersion").and_then(serde_json::Value::as_str),
        Some("2025-11-25"),
        "the legacy fallback offers the v2 pin"
    );
}

#[tokio::test]
async fn mcp_client_legacy_mode_accepts_server_declared_older_version() {
    // Explicit legacy: the initialize handshake accepts an old server
    // answering 2025-06-18 instead of echoing the offer.
    let script = Script { initialize_version: Some("2025-06-18"), ..Script::new() };
    let connection =
        connect_mock_config(script, &protocol_config(Some(oc_schema::mcp::Protocol::Legacy)))
            .await
            .expect("legacy handshake against an old server");
    assert_eq!(connection.tools().await.expect("tools").len(), 0);
}

#[tokio::test]
async fn mcp_client_pin_requires_the_declared_version() {
    // Pin 2026-07-28 against a server declaring it: connect succeeds.
    let script = Script {
        discover_supported: Some(vec![rmcp::model::ProtocolVersion::V_2026_07_28]),
        ..Script::new()
    };
    let connection =
        connect_mock_config(script, &protocol_config(Some(oc_schema::mcp::Protocol::Dated)))
            .await
            .expect("pinned version connect");
    assert_eq!(connection.tools().await.expect("tools").len(), 0);
}

#[tokio::test]
async fn mcp_client_pin_against_older_server_fails_with_the_hint() {
    // Pin 2026-07-28 against a server declaring only 2025-11-25: the
    // connect fails with v2's remediation hint (client.ts:376-380).
    let script = Script {
        discover_supported: Some(vec![rmcp::model::ProtocolVersion::V_2025_11_25]),
        ..Script::new()
    };
    let error =
        connect_mock_config(script, &protocol_config(Some(oc_schema::mcp::Protocol::Dated)))
            .await
            .err()
            .expect("the pin must not negotiate down");
    let client::ConnectFailure::Connect(error) = error else { panic!("expected a connect error") };
    assert!(
        error.message.contains("the server supports 2025-11-25"),
        "the hint names the server's versions: {}",
        error.message
    );
    assert!(
        error.message.contains("Set \"protocol\" for this server to one of those or to \"legacy\""),
        "the hint suggests protocol/legacy: {}",
        error.message
    );
}

#[tokio::test]
async fn mcp_client_call_tool_maps_resource_link_to_text_uri() {
    // client.ts:407 — `resource_link` flattens to `{type:"text", text: uri}`.
    let mut calls = BTreeMap::new();
    calls.insert(
        "link".to_string(),
        serde_json::json!({
            "content": [
                { "type": "resource_link", "uri": "mock://docs/1", "name": "Docs" },
                { "type": "text", "text": "after" },
            ],
            "isError": false,
        }),
    );
    let script = Script { calls, ..Script::new() };
    let (connection, _mock) = connect_mock(script).await;
    let result = connection.call_tool("link", None, None).await.expect("call");
    assert_eq!(result.content.len(), 2);
    assert_eq!(
        result.content[0],
        client::CallToolContent::Text { text: "mock://docs/1".into() },
        "resource_link surfaces as a text block carrying its URI"
    );
    assert_eq!(result.content[1], client::CallToolContent::Text { text: "after".into() });
}

// ---------------------------------------------------------------------------
// elicitation over the wire
// ---------------------------------------------------------------------------

/// The real forms service over deterministic ids/clock (no bus — the
/// form rows themselves are the observable surface).
fn real_forms() -> crate::forms::Forms {
    crate::forms::Forms::new(
        Arc::new(oc_util::determinism::SequentialIdGen::new()),
        Arc::new(oc_util::determinism::FixedClock::epoch()),
    )
}

/// Waits for one pending form on the sentinel session.
async fn await_global_form(forms: &crate::forms::Forms) -> oc_schema::form::Info {
    for _ in 0..400 {
        if let Some(form) = forms.list(Some(elicitation::GLOBAL_ELICITATION_SESSION_ID)).pop() {
            return form;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("no global elicitation form appeared");
}

fn form_params() -> serde_json::Value {
    serde_json::json!({
        "mode": "form",
        "message": "Who?",
        "requestedSchema": {
            "type": "object",
            "properties": { "name": { "type": "string", "title": "Name" } },
            "required": ["name"],
        },
    })
}

#[tokio::test]
async fn mcp_client_elicitation_form_accept_answers_and_continues_tool_call() {
    let forms = real_forms();
    let script = Script {
        elicit_on_call: BTreeMap::from([("work".to_string(), form_params())]),
        ..Script::new()
    };
    let offers = Arc::clone(&script.offers);
    let (connection, mock) =
        connect_mock_with(script, Some(RegistryElicitation::adapter(Arc::new(forms.clone()))))
            .await;

    // client.ts:188 — with a handler installed the client advertises the
    // elicitation capability with both sub-capabilities present. rmcp's
    // form sub-capability cannot carry v2's `applyDefaults: true` hint
    // (it models only `schemaValidation`), so the form object is empty.
    let capabilities =
        offers.lock()[0].pointer("/params/capabilities").cloned().expect("capabilities offered");
    assert_eq!(
        capabilities,
        serde_json::json!({
            "roots": {},
            "elicitation": { "form": {}, "url": {} },
        })
    );

    let call = tokio::spawn({
        let connection = connection.clone();
        async move { connection.call_tool("work", None, Some("ses_1")).await }
    });

    // The elicitation surfaces as a form owned by the "global" sentinel
    // session with the ref payload shape (index.ts:245-277).
    let form = await_global_form(&forms).await;
    assert_eq!(form.title, "mock is requesting input");
    assert_eq!(
        form.metadata.as_ref().and_then(|m| m.get("kind")).and_then(|v| v.as_str()),
        Some("mcp-elicitation")
    );
    assert_eq!(
        form.metadata.as_ref().and_then(|m| m.get("server")).and_then(|v| v.as_str()),
        Some("mock")
    );
    assert_eq!(
        form.metadata.as_ref().and_then(|m| m.get("message")).and_then(|v| v.as_str()),
        Some("Who?")
    );
    assert!(form.metadata.as_ref().is_some_and(|m| !m.contains_key("elicitationID")));

    // Answer accept+content; the elicitation result flows back and the
    // tool call continues (ref "empty-elicitation" result shape).
    let mut answer = oc_schema::form::Answer::new();
    answer.insert("name".to_string(), oc_schema::form::FormValue::String("Ada".into()));
    forms.reply(&form.id, answer).await.expect("reply");
    let result = call.await.expect("join").expect("call");
    assert_eq!(
        result.structured,
        Some(serde_json::json!({ "action": "accept", "content": { "name": "Ada" } }))
    );
    connection.close().await;
    drop(mock);
}

#[tokio::test]
async fn mcp_client_elicitation_empty_schema_accepts_without_creating_forms() {
    // Ref test "accepts empty MCP elicitations without creating forms":
    // the structured result is exactly `{action:"accept", content:{}}`.
    let forms = real_forms();
    let script = Script {
        elicit_on_call: BTreeMap::from([(
            "empty-elicitation".to_string(),
            serde_json::json!({
                "mode": "form",
                "message": "Confirm",
                "requestedSchema": { "type": "object", "properties": {} },
            }),
        )]),
        ..Script::new()
    };
    let (connection, _mock) =
        connect_mock_with(script, Some(RegistryElicitation::adapter(Arc::new(forms.clone()))))
            .await;
    let result = connection.call_tool("empty-elicitation", None, None).await.expect("call");
    assert_eq!(result.structured, Some(serde_json::json!({ "action": "accept", "content": {} })));
    assert!(forms.list(None).is_empty(), "an empty schema accepts without creating a form");
    connection.close().await;
}

#[tokio::test]
async fn mcp_client_elicitation_dismissed_form_answers_cancel() {
    // A dismissed form maps to `{action:"cancel"}` (index.ts:283 — every
    // non-answer cancels; v2's handler never declines).
    let forms = real_forms();
    let script = Script {
        elicit_on_call: BTreeMap::from([("work".to_string(), form_params())]),
        ..Script::new()
    };
    let (connection, _mock) =
        connect_mock_with(script, Some(RegistryElicitation::adapter(Arc::new(forms.clone()))))
            .await;
    let call = tokio::spawn({
        let connection = connection.clone();
        async move { connection.call_tool("work", None, None).await }
    });
    let form = await_global_form(&forms).await;
    forms.cancel(&form.id, None).await.expect("cancel");
    let result = call.await.expect("join").expect("call");
    assert_eq!(result.structured, Some(serde_json::json!({ "action": "cancel" })));
    connection.close().await;
}

#[tokio::test]
async fn mcp_client_elicitation_url_mode_settles_via_completion_notification() {
    // Ref test "acknowledges completed MCP URL elicitations without
    // returning internal content": the parked form settles on the legacy
    // `notifications/elicitation/complete` and the answer is exactly
    // `{action:"accept"}` — no content key.
    let forms = real_forms();
    let script = Script {
        elicit_on_call: BTreeMap::from([(
            "url-elicitation".to_string(),
            serde_json::json!({
                "mode": "url",
                "message": "Authorize access",
                "url": "https://example.com/authorize",
                "elicitationId": "elicitation-test",
            }),
        )]),
        ..Script::new()
    };
    let (connection, mock) =
        connect_mock_with(script, Some(RegistryElicitation::adapter(Arc::new(forms.clone()))))
            .await;
    let call = tokio::spawn({
        let connection = connection.clone();
        async move { connection.call_tool("url-elicitation", None, None).await }
    });
    let form = await_global_form(&forms).await;
    // The URL-mode form is one external field keyed "elicitation" and
    // carries the elicitationID it will be settled by.
    assert_eq!(form.fields.len(), 1);
    assert!(matches!(
        form.fields[0],
        oc_schema::form::Field::External { ref key, ref url, .. }
            if key == elicitation::URL_ELICITATION_FIELD_KEY && url == "https://example.com/authorize"
    ));
    assert_eq!(
        form.metadata.as_ref().and_then(|m| m.get("elicitationID")).and_then(|v| v.as_str()),
        Some("elicitation-test")
    );
    mock.notify_elicitation_complete("elicitation-test");
    let result = call.await.expect("join").expect("call");
    assert_eq!(result.structured, Some(serde_json::json!({ "action": "accept" })));
    // The parked form settled answered with the sentinel field, not
    // internal content.
    match forms.state(&form.id).expect("state") {
        oc_schema::form::State::Answered { answer } => {
            assert_eq!(
                answer.get(elicitation::URL_ELICITATION_FIELD_KEY),
                Some(&oc_schema::form::FormValue::Bool(true))
            );
        }
        other => panic!("expected answered, got {other:?}"),
    }
    connection.close().await;
}

#[tokio::test]
async fn mcp_client_elicitation_abort_race_answers_cancel() {
    // Timeout/cancel semantics per ref: no dedicated elicit timeout — the
    // ask races the request's abort signal (`waitForAbort(signal)`,
    // client.ts:209) and an aborted elicitation settles cancelled, never
    // dangling a pending form. rmcp additionally drops the late response
    // for a request it already cancelled, so nothing rides the wire.
    let forms = real_forms();
    let (connection, mut mock) = connect_mock_with(
        Script::new(),
        Some(RegistryElicitation::adapter(Arc::new(forms.clone()))),
    )
    .await;
    mock.request_elicitation(7001, form_params());
    let form = await_global_form(&forms).await;
    assert!(matches!(forms.state(&form.id), Ok(oc_schema::form::State::Pending)));
    mock.cancel_request(7001);
    // The aborted form settles cancelled (the ask lost the abort race).
    for _ in 0..400 {
        if matches!(forms.state(&form.id), Ok(oc_schema::form::State::Cancelled { .. })) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        matches!(forms.state(&form.id), Ok(oc_schema::form::State::Cancelled { .. })),
        "aborted elicitation cancels its form"
    );
    // rmcp drops the cancelled request's late response — nothing answers
    // the elicitation on the wire.
    if let Ok(answer) = tokio::time::timeout(Duration::from_millis(200), mock.answers.recv()).await
    {
        panic!("no elicitation answer should ride the wire after cancellation: {answer:?}");
    }
    connection.close().await;
}

// ---------------------------------------------------------------------------
// modern-era MRTR input_required (SEP-2322)
// ---------------------------------------------------------------------------

/// The id-less url elicitation the TS SDK's `elicitUrl` sends inside
/// MRTR rounds — the shape rmcp's `UrlElicitationParams` cannot model
/// (it requires `elicitationId`), so it arrives as `CustomResult`.
fn id_less_url_input_required() -> serde_json::Value {
    serde_json::json!({
        "resultType": "input_required",
        "inputRequests": {
            "auth": {
                "method": "elicitation/create",
                "params": { "mode": "url", "message": "Authorize", "url": "https://example.com/authorize" },
            },
        },
        "requestState": "opaque-state",
    })
}

#[tokio::test]
async fn mcp_client_modern_url_elicitation_settles_when_user_confirms() {
    // Ref "settles modern MCP URL elicitations when the user confirms"
    // (mcp.test.ts:1332): the `tools/call` answer is `input_required`
    // carrying an id-less url elicitation; the form carries NO
    // `elicitationID` metadata, the confirm reply settles it, and the
    // client retries with `{auth: {action: "accept"}}` + the echoed
    // `requestState` (mcp.test.ts:171: `{action: response.kind ===
    // "elicit" ? response.action : "cancel"}` — no content key).
    let forms = real_forms();
    let script = Script {
        input_required: BTreeMap::from([(
            "url-elicitation".to_string(),
            id_less_url_input_required(),
        )]),
        ..Script::new()
    };
    let (connection, _mock) =
        connect_mock_with(script, Some(RegistryElicitation::adapter(Arc::new(forms.clone()))))
            .await;
    let call = tokio::spawn({
        let connection = connection.clone();
        async move { connection.call_tool("url-elicitation", None, None).await }
    });
    let form = await_global_form(&forms).await;
    assert!(matches!(
        form.fields[0],
        oc_schema::form::Field::External { ref url, .. } if url == "https://example.com/authorize"
    ));
    assert!(
        !form.metadata.as_ref().is_some_and(|m| m.contains_key("elicitationID")),
        "id-less url elicitations park no completion id"
    );
    let mut answer = oc_schema::form::Answer::new();
    answer.insert("elicitation".to_string(), oc_schema::form::FormValue::Bool(true));
    forms.reply(&form.id, answer).await.expect("reply");
    let result = call.await.expect("join").expect("call");
    let structured = result.structured.expect("structured");
    assert_eq!(
        structured.pointer("/inputResponses/auth"),
        Some(&serde_json::json!({ "action": "accept" })),
        "url-mode MRTR answers carry no content key"
    );
    assert_eq!(
        structured.get("requestState").and_then(serde_json::Value::as_str),
        Some("opaque-state"),
        "requestState is echoed verbatim"
    );
    connection.close().await;
}

#[tokio::test]
async fn mcp_client_modern_form_elicitation_answers_with_content() {
    // A form-mode elicitation inside `inputRequests` (elicitationId
    // present parses to rmcp's typed `InputRequiredResult`; the driver
    // handles both shapes): the answered form rides the retry as
    // `{action: "accept", content: {...}}`.
    let forms = real_forms();
    let input_required = serde_json::json!({
        "resultType": "input_required",
        "inputRequests": {
            "who": {
                "method": "elicitation/create",
                "params": {
                    "mode": "form",
                    "message": "Who?",
                    "requestedSchema": {
                        "type": "object",
                        "properties": { "name": { "type": "string", "title": "Name" } },
                        "required": ["name"],
                    },
                },
            },
        },
    });
    let script = Script {
        input_required: BTreeMap::from([("form-elicitation".to_string(), input_required)]),
        ..Script::new()
    };
    let (connection, _mock) =
        connect_mock_with(script, Some(RegistryElicitation::adapter(Arc::new(forms.clone()))))
            .await;
    let call = tokio::spawn({
        let connection = connection.clone();
        async move { connection.call_tool("form-elicitation", None, None).await }
    });
    let form = await_global_form(&forms).await;
    let mut answer = oc_schema::form::Answer::new();
    answer.insert("name".to_string(), oc_schema::form::FormValue::String("Ada".into()));
    forms.reply(&form.id, answer).await.expect("reply");
    let result = call.await.expect("join").expect("call");
    assert_eq!(
        result.structured.expect("structured").pointer("/inputResponses/who"),
        Some(&serde_json::json!({ "action": "accept", "content": { "name": "Ada" } }))
    );
    connection.close().await;
}

#[tokio::test]
async fn mcp_client_input_required_without_handler_stays_opaque() {
    // No elicitation handler installed ⇒ the capability was not
    // advertised ⇒ `input_required` keeps the opaque v2 error (the
    // boundary the ref SDK leaves to the server's discretion).
    let script = Script {
        input_required: BTreeMap::from([(
            "url-elicitation".to_string(),
            id_less_url_input_required(),
        )]),
        ..Script::new()
    };
    let (connection, _mock) = connect_mock(script).await;
    let error =
        connection.call_tool("url-elicitation", None, None).await.expect_err("opaque degradation");
    assert_eq!(error.message, "unexpected tools/call response");
    connection.close().await;
}

#[tokio::test]
async fn mcp_client_input_required_sampling_request_stays_opaque() {
    // Sampling input requests are unfulfillable (v2 installs no
    // sampling handler): parity-exact opaque error.
    let input_required = serde_json::json!({
        "resultType": "input_required",
        "inputRequests": {
            "ask": {
                "method": "sampling/createMessage",
                "params": {
                    "messages": [{ "role": "user", "content": { "type": "text", "text": "hi" } }],
                    "maxTokens": 10,
                },
            },
        },
    });
    let forms = real_forms();
    let script = Script {
        input_required: BTreeMap::from([("sampled".to_string(), input_required)]),
        ..Script::new()
    };
    let (connection, _mock) =
        connect_mock_with(script, Some(RegistryElicitation::adapter(Arc::new(forms.clone()))))
            .await;
    let error = connection.call_tool("sampled", None, None).await.expect_err("opaque degradation");
    assert_eq!(error.message, "unexpected tools/call response");
    connection.close().await;
}

#[tokio::test]
async fn mcp_registry_passes_elicitation_handler_to_connector() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("alpha".to_string(), local_config());
    let saw_elicitation = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let marker = Arc::clone(&saw_elicitation);
    let connector: Connector = Arc::new(move |input: ConnectInput<'_>| {
        marker.store(input.elicitation.is_some(), std::sync::atomic::Ordering::SeqCst);
        let script = Script::new().tool("echo", "Echo", None);
        let (transport, _mock) = mock_pair(script);
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                false,
            )
            .await
        })
    });
    let registry = McpRegistry::with_connector(bus, connector, Arc::new(NoCredentials))
        .with_config_source(Arc::new(move || servers.clone()))
        .with_directory("/tmp")
        .with_elicitation(RegistryElicitation::adapter(Arc::new(real_forms())));
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;
    assert!(
        saw_elicitation.load(std::sync::atomic::Ordering::SeqCst),
        "the registry's elicitation handler rides every connect"
    );
    registry.shutdown().await;
}

// ---------------------------------------------------------------------------
// registry lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_registry_connects_and_publishes_status() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("alpha".to_string(), local_config());
    let registry = registry_with(
        &bus,
        scripted_connector(|| Script::new().tool("echo", "Echo", None)),
        servers,
    );
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;
    let tools = registry.tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].server, "alpha");
    assert_eq!(tools[0].name, "echo");
    registry.shutdown().await;
}

#[tokio::test]
async fn mcp_registry_failed_and_unavailable_messages_exact() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("broken".to_string(), local_config());
    let registry = registry_with(&bus, failing_connector("spawn failed"), servers);
    registry.reconcile().await;
    wait_for_status(&registry, "broken", Status::Failed { error: "spawn failed".to_string() })
        .await;

    // Not-yet-connected entries and every status flavor render the exact
    // v2 unavailable messages.
    assert_eq!(
        unavailable("broken", &Status::Failed { error: "spawn failed".into() }),
        "MCP server \"broken\" is not connected: spawn failed. Reconnect it from /mcps."
    );
    assert_eq!(
        unavailable("auth", &Status::NeedsAuth { error: "expired".into() }),
        "MCP server \"auth\" needs authentication: expired. Sign in from /mcps."
    );
    assert_eq!(unavailable("off", &Status::Disabled), "MCP server \"off\" is disabled.");
    assert_eq!(unavailable("off", &Status::Pending), "MCP server \"off\" is not connected.");
    assert_eq!(unavailable("off", &Status::Connected), "MCP server \"off\" is not connected.");

    let error = registry.call_tool("broken", "echo", None, None).await.expect_err("not connected");
    match error {
        super::registry::McpError::ToolCall(error) => assert_eq!(
            error.message,
            "MCP server \"broken\" is not connected: spawn failed. Reconnect it from /mcps."
        ),
        other @ super::registry::McpError::NotFound(_) => {
            panic!("expected ToolCallError, got {other:?}")
        }
    }
    registry.shutdown().await;
}

#[tokio::test]
async fn mcp_registry_unknown_server_not_found() {
    let (_db, bus) = test_bus().await;
    let registry = registry_with(&bus, failing_connector("nope"), BTreeMap::new());
    let error = registry.call_tool("ghost", "echo", None, None).await.unwrap_err();
    assert_eq!(error.to_string(), "MCP server not found: ghost");
}

#[tokio::test]
async fn mcp_registry_tools_sorted_by_server_then_name() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("zeta".to_string(), local_config());
    servers.insert("alpha".to_string(), local_config());
    let registry = registry_with(
        &bus,
        scripted_connector(|| Script::new().tool("zz", "Z", None).tool("aa", "A", None)),
        servers,
    );
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;
    wait_for_status(&registry, "zeta", Status::Connected).await;
    let order: Vec<String> =
        registry.tools().iter().map(|tool| format!("{}/{}", tool.server, tool.name)).collect();
    assert_eq!(order, vec!["alpha/aa", "alpha/zz", "zeta/aa", "zeta/zz"]);
    registry.shutdown().await;
}

/// RD-18 — the listing surfaces order like JS `localeCompare`
/// (index.ts:632-770: servers/tools/prompts/resources): accent- and
/// case-insensitive primary, so non-ASCII interleaves — byte order
/// would put "Eclair" < "Zebra" < "apple" < "éclair".
#[tokio::test]
async fn mcp_listings_sort_like_locale_compare() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    for name in ["Zebra", "apple", "éclair", "Eclair"] {
        servers.insert(name.to_string(), local_config());
    }
    let registry = registry_with(
        &bus,
        scripted_connector(|| Script::new().tool("zz", "Z", None).tool("aa", "A", None)),
        servers,
    );
    registry.reconcile().await;
    wait_for_status(&registry, "apple", Status::Connected).await;
    wait_for_status(&registry, "Zebra", Status::Connected).await;
    let names: Vec<String> = registry.servers().into_iter().map(|info| info.name).collect();
    assert_eq!(names, vec!["apple", "Eclair", "éclair", "Zebra"]);
    let tools: Vec<String> =
        registry.tools().iter().map(|tool| format!("{}/{}", tool.server, tool.name)).collect();
    assert_eq!(
        tools,
        vec![
            "apple/aa",
            "apple/zz",
            "Eclair/aa",
            "Eclair/zz",
            "éclair/aa",
            "éclair/zz",
            "Zebra/aa",
            "Zebra/zz",
        ]
    );
    registry.shutdown().await;
}

#[tokio::test]
async fn mcp_registry_add_disconnect_reconnect() {
    let (_db, bus) = test_bus().await;
    let registry = registry_with(
        &bus,
        scripted_connector(|| Script::new().tool("echo", "Echo", None)),
        BTreeMap::new(),
    );
    registry.reconcile().await;
    registry.add("fresh", local_config()).await;
    wait_for_status(&registry, "fresh", Status::Connected).await;
    assert_eq!(registry.tools().len(), 1);

    registry.disconnect("fresh").await.expect("disconnect");
    assert_eq!(registry.servers()[0].status, Status::Disabled);
    assert!(registry.tools().is_empty(), "disconnected server drops its tools");

    registry.connect("fresh").await.expect("reconnect");
    wait_for_status(&registry, "fresh", Status::Connected).await;
    assert_eq!(registry.tools().len(), 1);

    registry.remove("fresh").await.expect("remove");
    assert!(registry.servers().is_empty());
    registry.shutdown().await;
}

#[tokio::test]
async fn mcp_registry_reconciles_only_changed_servers() {
    let (_db, bus) = test_bus().await;
    let configs = Arc::new(parking_lot::Mutex::new(BTreeMap::<String, ServerConfig>::new()));
    configs.lock().insert("keep".to_string(), local_config());
    configs.lock().insert("swap".to_string(), local_config());
    configs.lock().insert("gone".to_string(), local_config());
    let source = {
        let configs = Arc::clone(&configs);
        Arc::new(move || configs.lock().clone())
    };
    let connects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&connects);
    let connector: Connector = Arc::new(move |input: ConnectInput<'_>| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let script = Script::new().tool("echo", "Echo", None);
        let (transport, _mock) = mock_pair(script);
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                false,
            )
            .await
        })
    });
    let registry = McpRegistry::with_connector(bus, connector, Arc::new(NoCredentials))
        .with_config_source(source)
        .with_directory("/tmp");
    registry.reconcile().await;
    wait_for_status(&registry, "gone", Status::Connected).await;
    let first = connects.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(first, 3, "initial pass connects every non-disabled server");

    // Only "swap" changed (fresh config object ⇒ replace), "gone" removed,
    // "keep" untouched.
    let mut next = BTreeMap::new();
    next.insert("keep".to_string(), local_config());
    next.insert("swap".to_string(), local_config());
    *configs.lock() = next;
    registry.reconcile().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let names: Vec<String> = registry.servers().into_iter().map(|server| server.name).collect();
    assert_eq!(names, vec!["keep", "swap"]);
    assert!(
        connects.load(std::sync::atomic::Ordering::SeqCst) <= first + 1,
        "unchanged servers are not reconnected"
    );
    registry.shutdown().await;
}

#[tokio::test]
async fn mcp_registry_recovers_and_replays_after_session_expiry() {
    let (_db, bus) = test_bus().await;
    let expire = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let mut servers = BTreeMap::new();
    servers.insert("remote".to_string(), local_config());
    let mut calls = BTreeMap::new();
    calls.insert(
        "work".to_string(),
        serde_json::json!({ "content": [{ "type": "text", "text": "replayed" }] }),
    );
    let connector: Connector = Arc::new(move |input: ConnectInput<'_>| {
        let script =
            Script { calls: calls.clone(), expire_once: Arc::clone(&expire), ..Script::new() };
        let (transport, _mock) = mock_pair(script);
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            // G-223: the mock simulates a sessionful remote server —
            // expiry classification requires the session-attached gate.
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                true,
            )
            .await
            .inspect(super::client::Connection::mark_session_attached)
        })
    });
    let registry = registry_with(&bus, connector, servers);
    registry.reconcile().await;
    wait_for_status(&registry, "remote", Status::Connected).await;
    // The first call observes the expiry; the registry reconnects and
    // replays once, settling on the replacement's answer.
    let result = registry.call_tool("remote", "work", None, None).await.expect("replayed call");
    assert!(!result.is_error);
    assert_eq!(result.server, "remote");
    assert_eq!(result.tool, "work");
    registry.shutdown().await;
}

#[tokio::test]
async fn mcp_registry_watch_tools_changed_refreshes_catalog() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("live".to_string(), local_config());
    let push_slot: Arc<parking_lot::Mutex<Vec<mpsc::UnboundedSender<ServerJsonRpcMessage>>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let catalog: Arc<parking_lot::Mutex<Vec<serde_json::Value>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let base = Script { tools: Arc::clone(&catalog), ..Script::new() };
    base.add_tool("before", "B");
    let push_capture = Arc::clone(&push_slot);
    let connector: Connector = Arc::new(move |input: ConnectInput<'_>| {
        let script = Script { tools: Arc::clone(&catalog), ..Script::new() };
        let (transport, mock) = mock_pair(script);
        push_capture.lock().push(mock.push.clone());
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                false,
            )
            .await
        })
    });
    let registry = registry_with(&bus, connector, servers);
    let mut events = bus.subscribe_type("mcp.tools.changed");
    registry.reconcile().await;
    wait_for_status(&registry, "live", Status::Connected).await;
    assert_eq!(registry.tools().len(), 1);

    // Mutate the live catalog and announce tools/list_changed; the watch
    // refreshes under the server lock and publishes.
    base.add_tool("after", "A");
    let push = push_slot.lock().last().cloned().expect("connector ran");
    let _ = push.send(JsonRpcMessage::notification(
        rmcp::model::ServerNotification::ToolListChangedNotification(
            rmcp::model::ToolListChangedNotification {
                method: Default::default(),
                extensions: Default::default(),
            },
        ),
    ));
    for _ in 0..200 {
        if registry.tools().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let names: Vec<String> = registry.tools().into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, vec!["after", "before"], "catalog refreshed after list-changed");
    // The refresh published mcp.tools.changed (beyond the connect burst).
    use futures::StreamExt;
    let mut seen = 0;
    while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(5), events.next()).await {
        seen += 1;
    }
    assert!(seen >= 1, "tools.changed published for the refresh");
    registry.shutdown().await;
}

/// G-294 — the background list-changed watch reconnects on session
/// expiry (index.ts:415 `onSessionExpired(() => fork(recover(...)))`):
/// a refresh that observes the expiry signal triggers the same
/// stop+start recovery the foreground paths use, and the replacement
/// connection's startup load repopulates the tool table.
#[tokio::test]
async fn mcp_registry_watch_reconnects_after_background_session_expiry() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("remote".to_string(), local_config());
    let expire = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let connects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let push_slot: Arc<parking_lot::Mutex<Vec<mpsc::UnboundedSender<ServerJsonRpcMessage>>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let catalog: Arc<parking_lot::Mutex<Vec<serde_json::Value>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let base = Script { tools: Arc::clone(&catalog), ..Script::new() };
    base.add_tool("before", "B");
    let push_capture = Arc::clone(&push_slot);
    let expire_capture = Arc::clone(&expire);
    let connects_capture = Arc::clone(&connects);
    let catalog_capture = Arc::clone(&catalog);
    let connector: Connector = Arc::new(move |input: ConnectInput<'_>| {
        connects_capture.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let script = Script {
            tools: Arc::clone(&catalog_capture),
            expire_list_once: Arc::clone(&expire_capture),
            ..Script::new()
        };
        let (transport, mock) = mock_pair(script);
        push_capture.lock().push(mock.push.clone());
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            // G-223: expiry classification requires the session-attached
            // gate — the mock simulates a sessionful remote server.
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                true,
            )
            .await
            .inspect(super::client::Connection::mark_session_attached)
        })
    });
    let registry = registry_with(&bus, connector, servers);
    registry.reconcile().await;
    wait_for_status(&registry, "remote", Status::Connected).await;
    assert_eq!(registry.tools().len(), 1);

    // The next tools/list observes the expiry; arm it, replace the live
    // catalog, and announce tools/list_changed. The background refresh
    // errors with the expiry signal — the session-expired watch leg
    // reconnects (stop + start) and the replacement's startup load
    // sees the new catalog.
    expire.store(true, std::sync::atomic::Ordering::SeqCst);
    catalog.lock().clear();
    base.add_tool("after", "A");
    let push = push_slot.lock().last().cloned().expect("connector ran");
    let _ = push.send(JsonRpcMessage::notification(
        rmcp::model::ServerNotification::ToolListChangedNotification(
            rmcp::model::ToolListChangedNotification {
                method: Default::default(),
                extensions: Default::default(),
            },
        ),
    ));
    for _ in 0..400 {
        if connects.load(std::sync::atomic::Ordering::SeqCst) >= 2 && registry.tools().len() == 1 {
            let names: Vec<String> = registry.tools().into_iter().map(|tool| tool.name).collect();
            if names == vec!["after".to_string()] {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let names: Vec<String> = registry.tools().into_iter().map(|tool| tool.name).collect();
    assert_eq!(names, vec!["after"], "tool table repopulated from the replacement");
    assert_eq!(
        connects.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "the expiry triggered exactly one reconnect"
    );
    registry.shutdown().await;
}

// ---------------------------------------------------------------------------
// namespacing / bridge
// ---------------------------------------------------------------------------

#[test]
fn mcp_tool_names_match_v1_sanitization() {
    assert_eq!(namespace("my server"), "my_server");
    assert_eq!(namespace("a.b/c"), "a_b_c");
    assert_eq!(tool_name("my server", "echo tool"), "my_server_echo_tool");
    assert_eq!(tool_name("Srv-1", "ok"), "Srv-1_ok");
}

#[test]
fn mcp_integration_suffix_shape() {
    let suffix = integration_suffix("srv", "https://x.test/mcp");
    assert!(suffix.starts_with("mcp_"), "{suffix}");
    assert_eq!(suffix.len(), 4 + 16, "mcp_ + first 16 hex chars");
    assert_ne!(integration_suffix("a", "https://x"), integration_suffix("b", "https://x"));
    assert_ne!(integration_suffix("a", "https://x"), integration_suffix("a", "https://y"));
}

#[tokio::test]
async fn mcp_tool_bridge_registers_namespaced_tools_and_flushes() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("alpha".to_string(), local_config());
    let registry = registry_with(
        &bus,
        scripted_connector(|| Script::new().tool("echo", "Echo", None)),
        servers,
    );
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;

    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let permission = test_permission(&bus);
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let services = test_services(&permission);
    let bridge =
        super::registry::McpToolBridge::install(&registry, &tools, &permission, &hooks, &services);
    bridge.flush().await;
    assert_eq!(tools.list().len(), 1);
    let registration = tools.list().remove(0);
    // v2 registration shape (tool/mcp.ts:40-45): bare tool name +
    // namespace; the effective model-facing name is `alpha_echo` and the
    // permission action is the composed `{server}_{tool}` form.
    assert_eq!(registration.name, "echo");
    assert_eq!(registration.permission.as_deref(), Some("alpha_echo"));
    assert_eq!(registration.namespace.as_deref(), Some("alpha"));
    assert_eq!(registration.effective_name(), "alpha_echo");
    assert!(registration.codemode, "codemode defaults to true");
    registry.shutdown().await;
}

// ---------------------------------------------------------------------------
// G-291 — the registry keys by effectiveName (tool.ts:188): same-named
// tools on different servers coexist; a same-named MCP tool never
// shadows a builtin.
// ---------------------------------------------------------------------------

/// A scripted server exposing `search` whose answer names its server.
fn g291_script(server: &str) -> Script {
    let calls = BTreeMap::from([(
        "search".to_string(),
        serde_json::json!({
            "content": [{ "type": "text", "text": format!("hit from {server}") }],
            "isError": false,
        }),
    )]);
    let script = Script { calls, ..Script::new() };
    script.tools.lock().push(serde_json::json!({
        "name": "search",
        "description": "Search",
        "inputSchema": { "type": "object", "properties": {} },
    }));
    script
}

/// A connector scripting `search` per server name (the connect input
/// picks the script, unlike [`scripted_connector`]'s fixed one).
fn g291_connector() -> Connector {
    Arc::new(move |input: ConnectInput<'_>| {
        let script = g291_script(input.server);
        let (transport, _mock) = mock_pair(script);
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                false,
            )
            .await
        })
    })
}

async fn g291_fixture(
    bus: &crate::bus::Bus,
    servers: Vec<&'static str>,
) -> (McpRegistry, Arc<crate::location::ToolRegistryState>) {
    let mut config = BTreeMap::new();
    for server in &servers {
        config.insert((*server).to_string(), local_config());
    }
    let registry = registry_with(bus, g291_connector(), config);
    registry.reconcile().await;
    for server in servers {
        wait_for_status(&registry, server, Status::Connected).await;
    }
    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let (permission, _rx) = test_permission_with_rules(bus, Some(allow_all_rules()));
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let services = test_services(&permission);
    let bridge =
        super::registry::McpToolBridge::install(&registry, &tools, &permission, &hooks, &services);
    bridge.flush().await;
    (registry, tools)
}

/// Two servers exposing the SAME bare tool name both live in the
/// registry under distinct effective keys, and each effective name
/// routes to its own executor.
#[tokio::test]
async fn g291_same_named_tools_on_two_servers_coexist() {
    let (_db, bus) = test_bus().await;
    let (registry, tools) = g291_fixture(&bus, vec!["one", "two"]).await;

    // Distinct effective rows — no overwrite, and no bare `search` key.
    assert!(tools.get("one_search").is_some(), "one_search registered");
    assert!(tools.get("two_search").is_some(), "two_search registered");
    assert!(tools.get("search").is_none(), "no bare-name key");
    assert_eq!(tools.list().len(), 2, "both same-named tools live: {:?}", tools.list());

    // Each effective name routes to its own server's executor.
    for (key, expect) in [("one_search", "hit from one"), ("two_search", "hit from two")] {
        let result = tools
            .executor(key)
            .unwrap_or_else(|| panic!("executor for {key}"))
            .execute_with_context(&serde_json::json!({}), &g242_context())
            .await
            .unwrap_or_else(|failure| panic!("{key} executes: {failure:?}"));
        match &result.content[0] {
            oc_schema::tool::Content::Text { text } => assert_eq!(text, expect),
            other @ oc_schema::tool::Content::File { .. } => {
                panic!("expected text part from {key}, got {other:?}")
            }
        }
    }
    registry.shutdown().await;
}

/// A fixed-text builtin executor (the shadow probe).
struct G291Builtin {
    text: &'static str,
}

#[async_trait::async_trait]
impl crate::location::ToolExecute for G291Builtin {
    async fn execute(
        &self,
        _input: &serde_json::Value,
    ) -> Result<crate::location::ToolRunResult, crate::location::ToolRunFailure> {
        Ok(crate::location::ToolRunResult {
            content: vec![oc_schema::tool::Content::Text { text: self.text.to_owned() }],
            metadata: None,
            output: None,
        })
    }
}

/// An MCP tool whose bare name equals a builtin's does NOT overwrite
/// the builtin: both the registration row and the executor survive.
#[tokio::test]
async fn g291_mcp_tool_does_not_shadow_same_named_builtin() {
    let (_db, bus) = test_bus().await;
    // The builtin lands first (the real registries seed builtins before
    // MCP reconciliation).
    let (registry, tools) = g291_fixture(&bus, vec!["shadower"]).await;
    tools.register(crate::location::ToolRegistration::new(
        "search",
        "builtin search",
        serde_json::json!({ "type": "object" }),
    ));
    tools.register_executor("search", std::sync::Arc::new(G291Builtin { text: "builtin" }));

    assert_eq!(
        tools.get("search").map(|tool| tool.description.clone()),
        Some("builtin search".to_owned()),
        "the builtin row survives the same-named MCP registration"
    );
    assert!(tools.get("shadower_search").is_some(), "the MCP tool registers under its own key");

    let builtin = tools.executor("search").expect("builtin executor survives");
    let result = builtin.execute(&serde_json::json!({})).await.expect("builtin executes");
    match &result.content[0] {
        oc_schema::tool::Content::Text { text } => assert_eq!(text, "builtin"),
        other @ oc_schema::tool::Content::File { .. } => {
            panic!("expected builtin text, got {other:?}")
        }
    }
    registry.shutdown().await;
}

fn g242_context() -> crate::location::ToolCallContext {
    crate::location::ToolCallContext {
        session_id: oc_schema::id::SessionId("ses_g242".into()),
        agent: oc_schema::id::AgentId::parse("build"),
        message_id: "msg_1".into(),
        call_id: "call_1".into(),
    }
}

fn allow_all_rules() -> oc_schema::permission::Ruleset {
    vec![oc_schema::permission::Rule {
        action: "*".into(),
        resource: "*".into(),
        effect: oc_schema::permission::Effect::Allow,
    }]
}

/// The shared G-242 fixture: a connected scripted `alpha` server exposing
/// `echo` (free-form input, scripted `pong` answer) and `gated` (a
/// required input field, no scripted answer) plus `shaped` (a declared
/// outputSchema), bridged into a tool registry through the pipeline.
async fn g242_fixture(
    bus: &crate::bus::Bus,
    rules: Option<oc_schema::permission::Ruleset>,
) -> (
    McpRegistry,
    Arc<crate::location::ToolRegistryState>,
    Arc<crate::hooks::HookRegistry>,
    crate::permission::Permission,
    tokio::sync::broadcast::Receiver<oc_schema::misc_events::PermissionEvent>,
) {
    let mut servers = BTreeMap::new();
    servers.insert("alpha".to_string(), local_config());
    let mut calls = BTreeMap::new();
    calls.insert(
        "echo".to_string(),
        serde_json::json!({
            "content": [{ "type": "text", "text": "pong" }],
            "isError": false,
        }),
    );
    calls.insert(
        "shaped".to_string(),
        serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }],
            "structuredContent": { "answer": 42 },
            "isError": false,
        }),
    );
    calls.insert(
        "badshape".to_string(),
        serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }],
            "structuredContent": { "answer": "lots" },
            "isError": false,
        }),
    );
    let script = Script { calls, ..Script::new() };
    script.tools.lock().push(serde_json::json!({
        "name": "echo",
        "description": "Echo",
        "inputSchema": { "type": "object", "properties": {} },
    }));
    script.tools.lock().push(serde_json::json!({
        "name": "gated",
        "description": "Gated",
        "inputSchema": {
            "type": "object",
            "properties": { "q": { "type": "string" } },
            "required": ["q"],
        },
    }));
    let shape = serde_json::json!({
        "type": "object",
        "properties": { "answer": { "type": "number" } },
        "required": ["answer"],
    });
    for name in ["shaped", "badshape"] {
        script.tools.lock().push(serde_json::json!({
            "name": name,
            "description": name,
            "inputSchema": { "type": "object", "properties": {} },
            "outputSchema": shape,
        }));
    }
    let registry = registry_with(bus, scripted_connector(move || script.clone()), servers);
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;

    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let (permission, rx) = test_permission_with_rules(bus, rules);
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let services = test_services(&permission);
    let bridge =
        super::registry::McpToolBridge::install(&registry, &tools, &permission, &hooks, &services);
    bridge.flush().await;
    (registry, tools, hooks, permission, rx)
}

/// G-242 (ref tool/mcp.ts:51-63): an agent ruleset ASK rule on the
/// namespaced action parks the MCP tool call until the user approves —
/// previously ask rules never intercepted MCP tools at all.
#[tokio::test]
async fn g242_ask_rule_parks_mcp_tool_execution_until_reply() {
    let (_db, bus) = test_bus().await;
    let rules = vec![oc_schema::permission::Rule {
        action: "alpha_echo".into(),
        resource: "*".into(),
        effect: oc_schema::permission::Effect::Ask,
    }];
    let (registry, tools, _hooks, permission, mut rx) = g242_fixture(&bus, Some(rules)).await;

    let executor = tools.executor("alpha_echo").expect("pipeline executor registered");
    let spawned = tokio::spawn(async move {
        executor.execute_with_context(&serde_json::json!({}), &g242_context()).await
    });
    // The assert parks on an `alpha_echo` ask with ref's exact shape:
    // resources ["*"], save ["*"] (mcp.ts:53-54).
    let asked = match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        Ok(Ok(oc_schema::misc_events::PermissionEvent::Asked(env))) => env.data,
        other => panic!("expected permission.asked, got {other:?}"),
    };
    assert_eq!(asked.action, "alpha_echo");
    assert_eq!(asked.resources, vec!["*".to_string()]);
    assert_eq!(asked.save, Some(vec!["*".to_string()]));
    // The call must not have completed while parked.
    assert!(!spawned.is_finished(), "the MCP call stays parked while the ask is pending");
    // Approving "once" releases the parked call; the tool then runs.
    permission
        .reply(crate::permission::ReplyInput {
            request_id: asked.id,
            reply: oc_schema::permission::Reply::Once,
            message: None,
        })
        .await
        .expect("reply");
    let result = spawned.await.expect("join").expect("tool runs after approval");
    match &result.content[0] {
        oc_schema::tool::Content::Text { text } => assert_eq!(text, "pong"),
        other @ oc_schema::tool::Content::File { .. } => {
            panic!("expected text part, got {other:?}")
        }
    }
    registry.shutdown().await;
}

/// G-242 deny leg: a deny rule on the namespaced action rejects the call
/// with the permission failure (never reaching the server).
#[tokio::test]
async fn g242_deny_rule_rejects_mcp_tool_execution() {
    let (_db, bus) = test_bus().await;
    let rules = vec![
        oc_schema::permission::Rule {
            action: "*".into(),
            resource: "*".into(),
            effect: oc_schema::permission::Effect::Allow,
        },
        oc_schema::permission::Rule {
            action: "alpha_echo".into(),
            resource: "*".into(),
            effect: oc_schema::permission::Effect::Deny,
        },
    ];
    let (registry, tools, _hooks, _permission, _rx) = g242_fixture(&bus, Some(rules)).await;
    let executor = tools.executor("alpha_echo").expect("pipeline executor registered");
    let failure = executor
        .execute_with_context(&serde_json::json!({}), &g242_context())
        .await
        .expect_err("denied");
    match failure {
        crate::location::ToolRunFailure::PermissionRejected { message, .. } => {
            assert_eq!(message, "Permission denied: alpha_echo");
        }
        other => panic!("expected PermissionRejected, got {other:?}"),
    }
    registry.shutdown().await;
}

/// G-242 pipeline leg (ref registers MCP tools through `editor.add`,
/// mcp.ts:43): `tool.execute.before`/`after` fire under the effective
/// name and `decodeInput` rejects bad arguments BEFORE the leaf (the
/// scripted `gated` tool has no call answer — reaching the server would
/// answer differently).
#[tokio::test]
async fn g242_pipeline_hooks_and_decode_wrap_mcp_tools() {
    let (_db, bus) = test_bus().await;
    let (registry, tools, hooks, _permission, _rx) =
        g242_fixture(&bus, Some(allow_all_rules())).await;

    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let before_seen = Arc::clone(&seen);
    let scope = crate::hooks::HookScope::new();
    scope.track(
        hooks
            .register_tool_execute_before(
                crate::hooks::fn_hook(move |event: &mut crate::hooks::ToolExecuteBeforeEvent| {
                    before_seen
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(format!("before:{}", event.tool));
                    Ok(())
                }),
                crate::hooks::HookOptions::default(),
            )
            .await,
    );
    let after_seen = Arc::clone(&seen);
    scope.track(
        hooks
            .register_tool_execute_after(
                crate::hooks::fn_hook(move |event: &mut crate::hooks::ToolExecuteAfterEvent| {
                    let phase = match event.outcome {
                        crate::hooks::ToolOutcome::Completed { .. } => "after",
                        crate::hooks::ToolOutcome::Error { .. } => "after-error",
                    };
                    after_seen
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(format!("{phase}:{}", event.tool));
                    Ok(())
                }),
                crate::hooks::HookOptions::default(),
            )
            .await,
    );

    // Success leg: allow rules (none configured) → the tool answers.
    let result = tools
        .executor("alpha_echo")
        .expect("executor")
        .execute_with_context(&serde_json::json!({}), &g242_context())
        .await
        .expect("echo runs");
    match &result.content[0] {
        oc_schema::tool::Content::Text { text } => assert_eq!(text, "pong"),
        other @ oc_schema::tool::Content::File { .. } => {
            panic!("expected text part, got {other:?}")
        }
    }
    // Decode leg: the required `q` is missing — the pipeline's
    // decodeInput fires before the leaf (ref's exact issue message).
    let failure = tools
        .executor("alpha_gated")
        .expect("executor")
        .execute_with_context(&serde_json::json!({}), &g242_context())
        .await
        .expect_err("decode fails");
    assert!(
        failure.message().starts_with("Invalid arguments for tool \"alpha_gated\""),
        "got: {}",
        failure.message()
    );

    let observed = seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    // G-321 — the decode-failure leg now fires `after` with an Error
    // outcome too (ref tool.ts:130-137: decodeInput is caught inside
    // `execute`, so the after-event error leg runs before the return).
    assert_eq!(
        observed,
        vec![
            "before:alpha_echo".to_string(),
            "after:alpha_echo".to_string(),
            "before:alpha_gated".to_string(),
            "after-error:alpha_gated".to_string(),
        ]
    );
    registry.shutdown().await;
}

/// G-242's G-230 face (mcp.ts:48 `output: tool.outputSchema ?? {}`): a
/// server-DECLARED outputSchema enters the pipeline's `encodeOutput`
/// path — matching structured content passes, mismatching content fails
/// with the validation message; the undeclared `echo` stays the
/// accept-all pass-through.
#[tokio::test]
async fn g242_declared_output_schema_enters_encode_output() {
    let (_db, bus) = test_bus().await;
    let (registry, tools, _hooks, _permission, _rx) =
        g242_fixture(&bus, Some(allow_all_rules())).await;

    let result = tools
        .executor("alpha_shaped")
        .expect("executor")
        .execute_with_context(&serde_json::json!({}), &g242_context())
        .await
        .expect("valid structured output passes encodeOutput");
    assert_eq!(result.output, Some(serde_json::json!({ "answer": 42 })));

    let failure = tools
        .executor("alpha_badshape")
        .expect("executor")
        .execute_with_context(&serde_json::json!({}), &g242_context())
        .await
        .expect_err("mismatching output fails encodeOutput");
    assert!(
        failure.message().contains("Tool returned an invalid value for its output schema"),
        "got: {}",
        failure.message()
    );
    registry.shutdown().await;
}

// ---------------------------------------------------------------------------
// G-280① — transient-retry hops (client.ts:169-182)
// ---------------------------------------------------------------------------

/// G-280① — "Two more attempts, 250 ms and 1 s later"
/// (`Schedule.max([Schedule.exponential("250 millis", 4),
/// Schedule.recurs(2)])`): the exponential arm proposes 250/1000/4000…,
/// `recurs(2)` bounds the schedule to two hops — NOT 250/500 (the old
/// "×2 per hop" reading) and not four hops. Real clock: the pinned
/// schedule sleeps ~1.25s total.
#[tokio::test]
async fn g280_transient_retry_hops_are_250ms_then_1s() {
    use super::client::{retrying, RequestError};
    use std::pin::Pin;
    use std::time::Instant;
    type Attempt = Pin<Box<dyn Future<Output = Result<u32, RequestError>> + Send>>;
    let transient = |server: &str| RequestError {
        server: server.to_string(),
        message: "connection closed before response: os error 104".to_string(),
    };

    // Two transient failures, then success: attempt starts at ~0ms,
    // ~250ms, ~1250ms.
    let starts: Arc<parking_lot::Mutex<Vec<std::time::Duration>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let starts_for_attempts = Arc::clone(&starts);
    let counter_for_attempts = Arc::clone(&counter);
    let started = Instant::now();
    let value = retrying(move || {
        let starts = Arc::clone(&starts_for_attempts);
        let counter = Arc::clone(&counter_for_attempts);
        let attempt: Attempt = Box::pin(async move {
            starts.lock().push(started.elapsed());
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n < 2 {
                Err(transient("alpha"))
            } else {
                Ok(n)
            }
        });
        attempt
    })
    .await
    .expect("third attempt succeeds");
    assert_eq!(value, 2);
    let starts = starts.lock().clone();
    assert_eq!(starts.len(), 3, "initial + two retries");
    assert!(
        starts[1].as_millis() >= 230 && starts[1].as_millis() < 400,
        "first hop is 250ms (got {}ms)",
        starts[1].as_millis()
    );
    assert!(
        starts[2].as_millis() >= 1220 && starts[2].as_millis() < 1450,
        "second hop lands 1000ms after the first (got {}ms)",
        starts[2].as_millis()
    );

    // Three transient failures: the schedule is exhausted after two
    // retries — the third failure surfaces.
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter_for_failures = Arc::clone(&counter);
    let failure = retrying(move || {
        let counter = Arc::clone(&counter_for_failures);
        let attempt: Attempt = Box::pin(async move {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(transient("alpha"))
        });
        attempt
    })
    .await
    .expect_err("schedule exhausted");
    assert_eq!(failure.message, "connection closed before response: os error 104");
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);

    // A non-transient failure never retries.
    let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter_for_fatal = Arc::clone(&counter);
    let _failure = retrying(move || {
        let counter = Arc::clone(&counter_for_fatal);
        let attempt: Attempt = Box::pin(async move {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(RequestError { server: "alpha".into(), message: "not transient".into() })
        });
        attempt
    })
    .await
    .expect_err("non-transient surfaces immediately");
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
}

fn test_permission(bus: &crate::bus::Bus) -> crate::permission::Permission {
    test_permission_with_rules(bus, None).0
}

/// G-242 — the same builder with an agent ruleset (ask/deny legs of the
/// MCP tool permission gate), plus the asked-event receiver.
fn test_permission_with_rules(
    _bus: &crate::bus::Bus,
    rules: Option<oc_schema::permission::Ruleset>,
) -> (
    crate::permission::Permission,
    tokio::sync::broadcast::Receiver<oc_schema::misc_events::PermissionEvent>,
) {
    use crate::permission::*;
    let ids: Arc<dyn oc_util::determinism::IdGen> =
        Arc::new(oc_util::determinism::SequentialIdGen::new());
    struct RulesResolver(Option<oc_schema::permission::Ruleset>);
    #[async_trait::async_trait]
    impl RuleResolver for RulesResolver {
        async fn resolve(
            &self,
            _session_id: &oc_schema::id::SessionId,
            _agent: Option<&oc_schema::id::AgentId>,
        ) -> Result<
            (Option<oc_schema::permission::Ruleset>, Option<oc_schema::permission::Ruleset>),
            crate::permission::PermissionError,
        > {
            Ok((self.0.clone(), None))
        }
    }
    let sink = Arc::new(BroadcastEventSink::new(8));
    let rx = sink.subscribe();
    let permission = Permission::new(
        ids,
        Arc::new(oc_util::determinism::FixedClock::epoch()),
        sink,
        Arc::new(crate::permission::saved::MemorySavedGrantStore::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
        )),
        Arc::new(RulesResolver(rules)),
        oc_schema::id::ProjectId::global(),
        None,
    );
    (permission, rx)
}

/// A minimal `ToolServices` for the pipeline-wrapped bridge (the MCP
/// leaf never touches it; the pipeline only threads it through).
fn test_services(permission: &crate::permission::Permission) -> Arc<crate::tool::ToolServices> {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let access = crate::file_access::FileAccess::new(
        permission.clone(),
        dir.path().to_path_buf(),
        dir.path().to_path_buf(),
        home.clone(),
    );
    let services = crate::tool::ToolServices {
        directory: dir.path().to_path_buf(),
        project_directory: dir.path().to_path_buf(),
        home: home.clone(),
        access,
        permission: permission.clone(),
        mutation: Arc::new(crate::file_mutation::FileMutation::new(dir.path().to_path_buf(), home)),
        files: Arc::new(crate::environment::LocalFiles),
        formatter: Arc::new(crate::formatter::NoopFormatter::new()),
        instructions: None,
        db: None,
        bus: None,
    };
    std::mem::forget(dir); // keep the tempdir for the test's lifetime
    Arc::new(services)
}

// ---------------------------------------------------------------------------
// G-358 — catalog pagination: the client follows `nextCursor` (the SDK's
// list pagination) instead of reading one page.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn g358_client_follows_next_cursor_across_pages() {
    // Five tools, page size 2 → pages [t1,t2] [t3,t4] [t5]; the merged
    // listing keeps page order.
    let mut script = Script::new();
    for index in 1..=5 {
        script.tools.lock().push(serde_json::json!({
            "name": format!("t{index}"),
            "description": "paged",
            "inputSchema": { "type": "object" },
        }));
    }
    for index in 1..=3 {
        script.resources.lock().push(serde_json::json!({
            "uri": format!("mock://r{index}"),
            "name": format!("r{index}"),
        }));
    }
    script.page_size = Some(2);
    let (connection, _mock) = connect_mock(script).await;

    let names: Vec<String> = connection
        .tools()
        .await
        .expect("tools")
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(names, vec!["t1", "t2", "t3", "t4", "t5"], "all pages merged in order");

    let resources = connection.resources().await.expect("resources");
    assert_eq!(resources.len(), 3, "resources paginate too: {resources:?}");

    let prompts = connection.prompts().await.expect("prompts");
    assert!(prompts.is_empty(), "empty catalogs page to empty: {prompts:?}");
}

// ---------------------------------------------------------------------------
// G-359 — runtime OAuth token expiry: 401 on a live connection refreshes
// the stored grant, reconnects, and replays once; a failed/missing
// refresh settles `needs_auth`.
// ---------------------------------------------------------------------------

/// In-memory [`CredentialStore`] (the fake credential-row seam).
#[derive(Default)]
struct FakeCredentials {
    credential: parking_lot::Mutex<Option<super::oauth::StoredCredential>>,
    saved: parking_lot::Mutex<Vec<(String, String)>>, // (credential id, access)
    removed: parking_lot::Mutex<Vec<String>>,
}

impl FakeCredentials {
    fn with(credential: super::oauth::StoredCredential) -> Arc<Self> {
        Arc::new(Self {
            credential: parking_lot::Mutex::new(Some(credential)),
            saved: parking_lot::Mutex::new(Vec::new()),
            removed: parking_lot::Mutex::new(Vec::new()),
        })
    }
}

impl super::registry::CredentialStore for FakeCredentials {
    fn oauth_credential(
        &self,
        _integration_id: &oc_schema::id::IntegrationId,
    ) -> futures::future::BoxFuture<'static, Option<super::oauth::StoredCredential>> {
        let stored = self.credential.lock().clone();
        Box::pin(async move { stored })
    }

    fn save_oauth_tokens(
        &self,
        _integration_id: &oc_schema::id::IntegrationId,
        credential_id: &str,
        tokens: &super::oauth::StoredTokens,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        let mut stored = self.credential.lock();
        let Some(previous) = stored.as_mut() else {
            let missing = format!("credential {credential_id} not found");
            return Box::pin(async { Err(missing) });
        };
        previous.access = tokens.access_token.clone();
        if let Some(refresh) = &tokens.refresh_token {
            previous.refresh = refresh.clone();
        }
        if let Some(expires_in) = tokens.expires_in {
            previous.expires = now_ms() + i64::try_from(expires_in).unwrap_or(i64::MAX / 2) * 1000;
        }
        self.saved.lock().push((credential_id.to_string(), tokens.access_token.clone()));
        Box::pin(async { Ok(()) })
    }

    fn remove_credential(
        &self,
        credential_id: &str,
    ) -> futures::future::BoxFuture<'static, Result<(), String>> {
        *self.credential.lock() = None;
        self.removed.lock().push(credential_id.to_string());
        Box::pin(async { Ok(()) })
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or_default())
        .unwrap_or_default()
}

/// A hermetic OAuth authorization-server stub (plain HTTP over a
/// loopback socket): `GET /metadata` answers the server document,
/// `POST /token` answers the refresh grant (`ok`) or rejects it
/// (`reject`), everything else 404s.
async fn oauth_stub(ok: bool) -> (String, reqwest::Url) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let base = format!("http://{address}");
    let metadata = format!("{base}/metadata");
    let base = std::sync::Arc::new(base);
    let returned = base.to_string();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let base = Arc::clone(&base);
            let ok = ok;
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = [0u8; 8192];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buffer[..read]).to_string();
                let Some(request_line) = text.lines().next() else { return };
                let mut parts = request_line.split(' ');
                let method = parts.next().unwrap_or_default();
                let path = parts.next().unwrap_or_default();
                let (status, content_type, body) = match (method, path) {
                    ("GET", "/metadata") => (
                        "200 OK",
                        "application/json",
                        format!(
                            "{{\"authorization_endpoint\":\"{base}/authorize\",\"token_endpoint\":\"{base}/token\"}}"
                        ),
                    ),
                    ("POST", "/token") if ok => (
                        "200 OK",
                        "application/json",
                        "{\"access_token\":\"tok-new\",\"refresh_token\":\"r2\",\"token_type\":\"Bearer\",\"expires_in\":3600}".to_owned(),
                    ),
                    ("POST", "/token") => ("400 Bad Request", "application/json", "{\"error\":\"invalid_grant\"}".to_owned()),
                    _ => ("404 Not Found", "text/plain", "not found".to_owned()),
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (returned, reqwest::Url::parse(&metadata).expect("metadata url"))
}

fn oauth_remote_config(metadata: &str) -> ServerConfig {
    ServerConfig::Remote(Box::new(oc_schema::mcp::RemoteConfig {
        url: format!("{metadata}/server"),
        headers: None,
        oauth: Some(oc_schema::mcp::OAuthValue::Config(oc_schema::mcp::OAuthConfig {
            auth_server_metadata_url: Some(metadata.to_string()),
            ..oc_schema::mcp::OAuthConfig::default()
        })),
        disabled: None,
        codemode: None,
        timeout: None,
        protocol: None,
    }))
}

fn oauth_credential(refresh: &str) -> super::oauth::StoredCredential {
    super::oauth::StoredCredential {
        id: "cred1".to_string(),
        method_id: "mcp_test".to_string(),
        access: "tok-old".to_string(),
        refresh: refresh.to_string(),
        // far-future: the connect-time prepare passes the token through
        // so the runtime 401 is what triggers the refresh.
        expires: now_ms() + 3_600_000,
        token_type: "Bearer".to_string(),
        scope: None,
        client: Some(super::oauth::StoredClient {
            client_id: "client-1".to_string(),
            client_secret: None,
        }),
    }
}

/// A scripted connector that records the bearer each connect received
/// (asserting the refreshed token rode the reconnect).
fn bearer_recording_connector(
    scripts: Vec<Script>,
    bearers: Arc<parking_lot::Mutex<Vec<Option<String>>>>,
) -> Connector {
    let scripts = Arc::new(parking_lot::Mutex::new(scripts));
    Arc::new(move |input: ConnectInput<'_>| {
        let script = scripts.lock().remove(0);
        let (transport, _mock) = mock_pair(script);
        bearers.lock().push(input.bearer.clone());
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                false,
            )
            .await
        })
    })
}

/// The 401 trigger: the mock answers the tool call with a JSON-RPC error
/// whose text ends in the client's unauthorized suffix — the exact
/// [`RequestError`] shape `classify` produces for a typed 401 (mock
/// transports cannot emit a real HTTP status).
fn unauthorized_error() -> String {
    "expired token (unauthorized)".to_string()
}

#[tokio::test]
async fn g359_runtime_401_refreshes_reconnects_and_replays_once() {
    let (_db, bus) = test_bus().await;
    let (_base, metadata) = oauth_stub(true).await;

    // First connection: the tool call answers the unauthorized error;
    // the reconnect (second script) answers it.
    let mut rejected = Script::new();
    rejected.tools.lock().push(serde_json::json!({
        "name": "echo", "description": "Echo", "inputSchema": { "type": "object" },
    }));
    rejected.call_errors.insert("echo".to_string(), unauthorized_error());
    let mut healthy = Script::new();
    healthy.tools.lock().push(serde_json::json!({
        "name": "echo", "description": "Echo", "inputSchema": { "type": "object" },
    }));
    healthy.calls.insert(
        "echo".to_string(),
        serde_json::json!({ "content": [{ "type": "text", "text": "recovered" }], "isError": false }),
    );

    let bearers = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let connector = bearer_recording_connector(vec![rejected, healthy], Arc::clone(&bearers));
    let credentials = FakeCredentials::with(oauth_credential("r1"));
    let mut servers = BTreeMap::new();
    servers.insert("oauth-srv".to_string(), oauth_remote_config(metadata.as_str()));
    let registry = super::registry::McpRegistry::with_connector(
        bus.clone(),
        connector,
        credentials.clone() as Arc<dyn super::registry::CredentialStore>,
    )
    .with_config_source(Arc::new(move || servers.clone()))
    .with_directory("/tmp");
    registry.reconcile().await;
    wait_for_status(&registry, "oauth-srv", Status::Connected).await;

    let result = registry
        .call_tool("oauth-srv", "echo", Some(serde_json::Map::new()), None)
        .await
        .expect("the refreshed replay succeeds");
    assert!(!result.is_error);
    assert_eq!(
        bearers.lock().as_slice(),
        [Some("tok-old".to_string()), Some("tok-new".to_string())]
    );
    assert_eq!(
        credentials.saved.lock().as_slice(),
        [("cred1".to_string(), "tok-new".to_string())].as_slice(),
        "the rotated grant persisted"
    );
    assert!(credentials.removed.lock().is_empty(), "a successful refresh keeps the credential");
    registry.shutdown().await;
}

#[tokio::test]
async fn g359_refresh_failure_settles_needs_auth_and_drops_dead_credential() {
    let (_db, bus) = test_bus().await;
    let (_base, metadata) = oauth_stub(false).await;

    let mut script = Script::new();
    script.tools.lock().push(serde_json::json!({
        "name": "echo", "description": "Echo", "inputSchema": { "type": "object" },
    }));
    script.call_errors.insert("echo".to_string(), unauthorized_error());

    let bearers = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let connector = bearer_recording_connector(vec![script], Arc::clone(&bearers));
    let credentials = FakeCredentials::with(oauth_credential("r1"));
    let mut servers = BTreeMap::new();
    servers.insert("oauth-srv".to_string(), oauth_remote_config(metadata.as_str()));
    let registry = super::registry::McpRegistry::with_connector(
        bus.clone(),
        connector,
        credentials.clone() as Arc<dyn super::registry::CredentialStore>,
    )
    .with_config_source(Arc::new(move || servers.clone()))
    .with_directory("/tmp");
    registry.reconcile().await;
    wait_for_status(&registry, "oauth-srv", Status::Connected).await;

    let error = registry
        .call_tool("oauth-srv", "echo", Some(serde_json::Map::new()), None)
        .await
        .expect_err("the original unauthorized error surfaces");
    assert!(error.to_string().ends_with("(unauthorized)"), "surfaces the 401 text: {error}");
    wait_for_needs_auth(&registry, "oauth-srv").await;
    assert_eq!(credentials.removed.lock().as_slice(), ["cred1".to_string()]);
    assert_eq!(bearers.lock().len(), 1, "no reconnect without a fresh token");
    registry.shutdown().await;
}

#[tokio::test]
async fn g359_no_refresh_token_settles_needs_auth_without_invalidating() {
    let (_db, bus) = test_bus().await;
    let (_base, metadata) = oauth_stub(true).await;

    let mut script = Script::new();
    script.tools.lock().push(serde_json::json!({
        "name": "echo", "description": "Echo", "inputSchema": { "type": "object" },
    }));
    script.call_errors.insert("echo".to_string(), unauthorized_error());

    let bearers = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let connector = bearer_recording_connector(vec![script], Arc::clone(&bearers));
    // No refresh token: the SDK would refuse the unrefreshable token —
    // needs_auth engages, the credential row is NOT invalidated.
    let credentials = FakeCredentials::with(oauth_credential(""));
    let mut servers = BTreeMap::new();
    servers.insert("oauth-srv".to_string(), oauth_remote_config(metadata.as_str()));
    let registry = super::registry::McpRegistry::with_connector(
        bus.clone(),
        connector,
        credentials.clone() as Arc<dyn super::registry::CredentialStore>,
    )
    .with_config_source(Arc::new(move || servers.clone()))
    .with_directory("/tmp");
    registry.reconcile().await;
    wait_for_status(&registry, "oauth-srv", Status::Connected).await;

    let _ = registry
        .call_tool("oauth-srv", "echo", Some(serde_json::Map::new()), None)
        .await
        .expect_err("no refresh path");
    wait_for_needs_auth(&registry, "oauth-srv").await;
    assert!(
        credentials.removed.lock().is_empty(),
        "an unrefreshable token is not a rejected grant"
    );
    assert_eq!(bearers.lock().len(), 1);
    registry.shutdown().await;
}

// ---------------------------------------------------------------------------
// G-360 — bridge: sessionID metadata, sync serialization, name
// validation.
// ---------------------------------------------------------------------------

/// G-360① — the bridge threads the invoking session id into the
/// `tools/call` request metadata (`sessionID: context.sessionID`,
/// mcp.ts:69/124).
#[tokio::test]
async fn g360_bridge_threads_session_id_into_call_metadata() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("alpha".to_string(), local_config());
    let mut script = Script::new();
    script.tools.lock().push(serde_json::json!({
        "name": "echo", "description": "Echo", "inputSchema": { "type": "object" },
    }));
    script.calls.insert(
        "echo".to_string(),
        serde_json::json!({ "content": [{ "type": "text", "text": "pong" }], "isError": false }),
    );
    let metas = Arc::clone(&script.call_metas);
    let registry = registry_with(&bus, scripted_connector(move || script.clone()), servers);
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;

    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let (permission, _rx) = test_permission_with_rules(&bus, Some(allow_all_rules()));
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let services = test_services(&permission);
    let bridge =
        super::registry::McpToolBridge::install(&registry, &tools, &permission, &hooks, &services);
    bridge.flush().await;

    let result = tools
        .executor("alpha_echo")
        .expect("alpha_echo registered")
        .execute_with_context(&serde_json::json!({}), &g242_context())
        .await
        .expect("executes");
    let _ = result;
    let recorded = metas.lock().clone();
    assert_eq!(recorded.len(), 1, "one tools/call: {recorded:?}");
    assert_eq!(
        recorded[0].get("ai.opencode/sessionID"),
        Some(&serde_json::json!("ses_g242")),
        "sessionID rides the request metadata: {recorded:?}"
    );
    registry.shutdown().await;
}

/// G-360③ — invalid MCP tool names are skipped with a logged error while
/// valid siblings register (ref editor.add's registrationError gate,
/// tool.ts:177-183 + 304-320).
#[tokio::test]
async fn g360_bridge_skips_invalid_tool_names() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("alpha".to_string(), local_config());
    let script = Script::new();
    for (name, register) in [
        ("ok", true),
        // 129 chars after sanitization — over the 128 limit.
        (&"x".repeat(129)[..], false),
        // empty name — under the 1 minimum.
        ("", false),
    ] {
        if register {
            script.tools.lock().push(serde_json::json!({
                "name": name, "description": "ok", "inputSchema": { "type": "object" },
            }));
        } else {
            script.tools.lock().push(serde_json::json!({
                "name": name, "description": "invalid", "inputSchema": { "type": "object" },
            }));
        }
    }
    let registry = registry_with(&bus, scripted_connector(move || script.clone()), servers);
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;

    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let permission = test_permission(&bus);
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let services = test_services(&permission);
    let bridge =
        super::registry::McpToolBridge::install(&registry, &tools, &permission, &hooks, &services);
    bridge.flush().await;

    let effective: Vec<String> =
        tools.list().into_iter().map(|tool| tool.effective_name()).collect();
    assert_eq!(
        effective,
        vec!["alpha_ok".to_string()],
        "only the valid tool registers: {effective:?}"
    );
    assert!(tools.executor("alpha_ok").is_some(), "the valid tool has an executor");
    registry.shutdown().await;
}

/// G-360② — the initial registration and the debounced reload serialize
/// (the ref's `Semaphore.makeUnsafe(1)`): rapid catalog mutations racing
/// the first flush never lose registrations.
#[tokio::test]
async fn g360_initial_registration_and_reload_race_keeps_latest_table() {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("live".to_string(), local_config());
    let catalog: Arc<parking_lot::Mutex<Vec<serde_json::Value>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let push_slot: Arc<parking_lot::Mutex<Vec<mpsc::UnboundedSender<ServerJsonRpcMessage>>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let push_capture = Arc::clone(&push_slot);
    let catalog_capture = Arc::clone(&catalog);
    let connector: Connector = Arc::new(move |input: ConnectInput<'_>| {
        let script = Script { tools: Arc::clone(&catalog_capture), ..Script::new() };
        let (transport, mock) = mock_pair(script);
        push_capture.lock().push(mock.push.clone());
        let server = input.server.to_string();
        let config = input.config.clone();
        let directory = input.directory.to_string();
        let bearer = input.bearer.clone();
        let elicitation = input.elicitation.clone();
        Box::pin(async move {
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                    stdio: None,
                },
                transport,
                false,
            )
            .await
        })
    });
    let registry = registry_with(&bus, connector, servers);
    registry.reconcile().await;
    wait_for_status(&registry, "live", Status::Connected).await;

    // Install the bridge and, concurrently with its initial flush, grow
    // the catalog a burst of times — each growth announces
    // tools/list_changed (a debounced reload per burst).
    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let permission = test_permission(&bus);
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let services = test_services(&permission);
    let bridge =
        super::registry::McpToolBridge::install(&registry, &tools, &permission, &hooks, &services);
    for index in 0..12 {
        catalog.lock().push(serde_json::json!({
            "name": format!("tool{index}"),
            "description": "raced",
            "inputSchema": { "type": "object" },
        }));
        let push = push_slot.lock().last().cloned().expect("connector ran");
        let _ = push.send(JsonRpcMessage::notification(
            rmcp::model::ServerNotification::ToolListChangedNotification(
                rmcp::model::ToolListChangedNotification {
                    method: Default::default(),
                    extensions: Default::default(),
                },
            ),
        ));
    }
    bridge.flush().await;
    // The debounced reload settles after the burst; the final table must
    // be the union of every registered tool — no lost registrations.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if tools.list().len() == 12 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "never converged: {:?}", tools.list());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    registry.shutdown().await;
}

// ---------------------------------------------------------------------------
// elicitation
// ---------------------------------------------------------------------------

/// Scripted form service: canned answers, dismissals, or asks that park
/// until `reply` settles them.
struct FakeForms {
    answer: parking_lot::Mutex<Option<Result<oc_schema::form::Answer, elicitation::FormCancelled>>>,
    asked: parking_lot::Mutex<Vec<oc_schema::form::Info>>,
    reply: parking_lot::Mutex<Vec<(oc_schema::id::FormId, oc_schema::form::Answer)>>,
    /// Parking mode: senders for asks waiting on `reply`.
    parked: parking_lot::Mutex<
        Vec<(oc_schema::id::FormId, tokio::sync::oneshot::Sender<oc_schema::form::Answer>)>,
    >,
    park_until_reply: bool,
}

impl FakeForms {
    fn answering(answer: oc_schema::form::Answer) -> Self {
        Self {
            answer: parking_lot::Mutex::new(Some(Ok(answer))),
            asked: parking_lot::Mutex::new(Vec::new()),
            reply: parking_lot::Mutex::new(Vec::new()),
            parked: parking_lot::Mutex::new(Vec::new()),
            park_until_reply: false,
        }
    }

    /// Asks park until `reply` settles them by form id — the shape the
    /// legacy URL completion path relies on.
    fn parking() -> Self {
        Self {
            answer: parking_lot::Mutex::new(None),
            asked: parking_lot::Mutex::new(Vec::new()),
            reply: parking_lot::Mutex::new(Vec::new()),
            parked: parking_lot::Mutex::new(Vec::new()),
            park_until_reply: true,
        }
    }
}

impl FormService for FakeForms {
    fn ask(
        &self,
        form: oc_schema::form::Info,
    ) -> futures::future::BoxFuture<
        'static,
        Result<oc_schema::form::Answer, elicitation::FormCancelled>,
    > {
        let id = form.id.clone();
        self.asked.lock().push(form);
        if self.park_until_reply {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.parked.lock().push((id, sender));
            return Box::pin(async move { receiver.await.map_err(|_| elicitation::FormCancelled) });
        }
        let answer = self.answer.lock().clone().unwrap_or(Err(elicitation::FormCancelled));
        Box::pin(async move { answer })
    }

    fn reply(&self, id: &oc_schema::id::FormId, answer: oc_schema::form::Answer) {
        self.reply.lock().push((id.clone(), answer.clone()));
        let mut parked = self.parked.lock();
        if let Some(index) = parked.iter().position(|(parked_id, _)| parked_id == id) {
            let (_, sender) = parked.remove(index);
            let _ = sender.send(answer);
        }
    }
}

#[tokio::test]
async fn mcp_elicitation_empty_schema_accepts_without_creating_forms() {
    let forms = Arc::new(FakeForms::answering(oc_schema::form::Answer::new()));
    let handler = RegistryElicitation::new(Arc::clone(&forms) as Arc<dyn FormService>);
    let result = handler
        .create(super::elicitation::ElicitationCreate {
            server: "srv",
            params: ElicitationParams::Form {
                message: "hi".into(),
                requested_schema: serde_json::json!({ "properties": {} }),
            },
        })
        .await;
    assert_eq!(result.action, super::elicitation::ElicitationAction::Accept);
    assert_eq!(result.content.len(), 0);
    assert!(forms.asked.lock().is_empty(), "no form for an empty schema");
}

#[tokio::test]
async fn mcp_elicitation_form_roundtrip_and_global_sentinel() {
    let mut answer = oc_schema::form::Answer::new();
    answer.insert("name".to_string(), oc_schema::form::FormValue::String("Ada".into()));
    answer.insert("flags".to_string(), oc_schema::form::FormValue::Strings(vec!["a".into()]));
    let forms = Arc::new(FakeForms::answering(answer));
    let handler = RegistryElicitation::new(Arc::clone(&forms) as Arc<dyn FormService>);
    let result = handler
        .create(super::elicitation::ElicitationCreate {
            server: "srv",
            params: ElicitationParams::Form {
                message: "who?".into(),
                requested_schema: serde_json::json!({
                    "properties": {
                        "name": { "type": "string", "title": "Name" },
                        "flags": { "type": "array", "items": { "enum": ["a", "b"] } },
                    },
                    "required": ["name"],
                }),
            },
        })
        .await;
    assert_eq!(result.action, super::elicitation::ElicitationAction::Accept);
    assert_eq!(
        result.content.get("name"),
        Some(&super::elicitation::ElicitationValue::String("Ada".into()))
    );
    assert_eq!(
        result.content.get("flags"),
        Some(&super::elicitation::ElicitationValue::Strings(vec!["a".into()]))
    );
    let asked = forms.asked.lock();
    assert_eq!(asked[0].session_id, "global");
    assert_eq!(asked[0].title, "srv is requesting input");
    assert_eq!(asked[0].fields.len(), 2);
}

/// Waits until the fake has parked a form ask (bounded, like
/// [`await_global_form`]).
async fn wait_until_asked(forms: &FakeForms) {
    for _ in 0..400 {
        if !forms.asked.lock().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("no elicitation form was asked");
}

#[tokio::test]
async fn mcp_elicitation_url_mode_and_completion_settlement() {
    let forms = Arc::new(FakeForms::parking());
    let handler = Arc::new(RegistryElicitation::new(Arc::clone(&forms) as Arc<dyn FormService>));
    // Legacy URL elicitation with an id: the ask parks, and
    // notifications/elicitation/complete settles it with
    // `elicitation: true`, which maps to accept (index.ts:296-301).
    let handle = tokio::spawn({
        let handler = Arc::clone(&handler);
        async move {
            RegistryElicitation::create(
                &handler,
                super::elicitation::ElicitationCreate {
                    server: "srv",
                    params: ElicitationParams::Url {
                        message: "login".into(),
                        url: "https://srv/auth".into(),
                        elicitation_id: Some("el_1".into()),
                    },
                },
            )
            .await
        }
    });
    wait_until_asked(&forms).await;
    assert!(handler.is_pending_url("srv", "el_1"), "entry parked while the form waits");
    handler.complete("srv", "el_1");
    let result = handle.await.expect("join");
    // The settled form resolves the ask with `elicitation: true` → accept.
    assert_eq!(result.action, super::elicitation::ElicitationAction::Accept);
    assert!(result.content.is_empty(), "url mode returns no content");
    // Settlement reply shape: routed to the parked form id with the
    // sentinel field set.
    let replies = forms.reply.lock();
    assert_eq!(replies.len(), 1, "exactly one settlement reply");
    assert_eq!(replies[0].0, forms.asked.lock()[0].id, "reply routed to the parked form");
    assert_eq!(
        replies[0].1.get(elicitation::URL_ELICITATION_FIELD_KEY),
        Some(&oc_schema::form::FormValue::Bool(true))
    );
    drop(replies);
    assert!(!handler.is_pending_url("srv", "el_1"), "entry removed after settlement");
}

#[tokio::test]
async fn mcp_elicitation_url_abort_removes_pending_entry() {
    // Aborting a URL elicitation drops the create future mid-ask (the
    // client's select takes the cancellation arm); the pending entry must
    // be deleted on that exit too (Effect.ensuring, index.ts:260), not
    // only after the ask settles.
    let forms = Arc::new(FakeForms::parking());
    let registry = Arc::new(RegistryElicitation::new(Arc::clone(&forms) as Arc<dyn FormService>));
    let client = elicitation::ClientElicitation::new(Arc::clone(&registry));
    let ct = tokio_util::sync::CancellationToken::new();
    let request = rmcp::model::ElicitRequestParams::UrlElicitationParams {
        meta: None,
        message: "login".into(),
        url: "https://srv/auth".into(),
        elicitation_id: "el_abort".into(),
    };
    let handle = tokio::spawn({
        let ct = ct.clone();
        async move { client.handle("srv", request, ct).await }
    });
    wait_until_asked(&forms).await;
    assert!(registry.is_pending_url("srv", "el_abort"), "entry parked while the form waits");
    ct.cancel();
    let result = handle.await.expect("join");
    assert_eq!(
        result.action,
        rmcp::model::ElicitationAction::Cancel,
        "aborted elicitation cancels"
    );
    assert!(result.content.is_none(), "url mode carries no content key");
    assert!(!registry.is_pending_url("srv", "el_abort"), "abort removes the pending entry");
}

#[tokio::test]
async fn mcp_elicitation_modern_url_settles_on_user_confirm() {
    // Modern URL elicitation (no id): the form settles when the user
    // confirms; an answered form maps to accept.
    let forms = FakeForms::answering(oc_schema::form::Answer::new());
    let handler = RegistryElicitation::new(Arc::new(forms) as Arc<dyn FormService>);
    let result = handler
        .create(super::elicitation::ElicitationCreate {
            server: "srv",
            params: ElicitationParams::Url {
                message: "login".into(),
                url: "https://srv/auth".into(),
                elicitation_id: None,
            },
        })
        .await;
    assert_eq!(result.action, super::elicitation::ElicitationAction::Accept);
}

#[test]
fn mcp_elicitation_field_conversion_rules() {
    use oc_schema::form::Field;
    // Machine titles lose to description/key.
    let field = elicitation::to_elicitation_field(
        "email",
        &serde_json::json!({ "type": "string", "title": "string with format email", "description": "Your email", "format": "email" }),
        true,
    );
    match field {
        Field::String { key, title, description, required, format, .. } => {
            assert_eq!(key, "email");
            assert_eq!(title.as_deref(), Some("Your email"));
            assert_eq!(description, None, "description equal to title is dropped");
            assert_eq!(required, Some(true));
            assert_eq!(format, Some(oc_schema::form::StringFormat::Email));
        }
        other => panic!("expected string field, got {other:?}"),
    }
    // enum + enumNames → options with labels.
    let field = elicitation::to_elicitation_field(
        "pick",
        &serde_json::json!({ "type": "string", "enum": ["a", "b"], "enumNames": ["Alpha", "Beta"] }),
        false,
    );
    match field {
        Field::String { options: Some(options), .. } => {
            assert_eq!(options[0].value, "a");
            assert_eq!(options[0].label, "Alpha");
            assert_eq!(options[1].label, "Beta");
        }
        other => panic!("expected options, got {other:?}"),
    }
    // Array with anyOf consts → multiselect options, custom disabled.
    let field = elicitation::to_elicitation_field(
        "tags",
        &serde_json::json!({
            "type": "array",
            "items": { "anyOf": [ { "const": "x", "title": "X" }, { "const": "y", "title": "Y" } ] },
            "minItems": 1,
            "maxItems": 4,
        }),
        false,
    );
    match field {
        Field::Multiselect { options, custom, min_items, max_items, .. } => {
            assert_eq!(options.len(), 2);
            assert_eq!(options[1].value, "y");
            assert_eq!(custom, Some(false));
            assert_eq!(min_items, Some(1));
            assert_eq!(max_items, Some(4));
        }
        other => panic!("expected multiselect, got {other:?}"),
    }
    // Boolean + integer keep defaults and bounds.
    assert!(matches!(
        elicitation::to_elicitation_field(
            "flag",
            &serde_json::json!({ "type": "boolean", "default": true }),
            false
        ),
        Field::Boolean { default: Some(true), .. }
    ));
    assert!(matches!(
        elicitation::to_elicitation_field(
            "count",
            &serde_json::json!({ "type": "integer", "minimum": 1, "maximum": 9 }),
            true
        ),
        Field::Integer { minimum: Some(1.0), maximum: Some(9.0), required: Some(true), .. }
    ));
}

/// G-280② — `number` keeps its own wire kind (ref mcp/index.ts:800-807
/// `type: property.type`; schema form.ts `NumberField` vs IntegerField):
/// collapsing it onto Integer made float elicitation answers impossible
/// to input.
#[test]
fn g280_number_elicitation_maps_to_number_field() {
    use oc_schema::form::Field;
    assert!(matches!(
        elicitation::to_elicitation_field(
            "ratio",
            &serde_json::json!({ "type": "number", "minimum": 0.5, "maximum": 1.5, "default": 0.75 }),
            true
        ),
        Field::Number {
            minimum: Some(0.5),
            maximum: Some(1.5),
            default: Some(0.75),
            required: Some(true),
            ..
        }
    ));
    // The wire kind round-trips: `type: "number"` (not "integer").
    let field =
        elicitation::to_elicitation_field("ratio", &serde_json::json!({ "type": "number" }), false);
    assert_eq!(serde_json::to_value(&field).expect("wire")["type"], serde_json::json!("number"));
    // Integer stays Integer.
    let field = elicitation::to_elicitation_field(
        "count",
        &serde_json::json!({ "type": "integer" }),
        false,
    );
    assert_eq!(serde_json::to_value(&field).expect("wire")["type"], serde_json::json!("integer"));
}

// ---------------------------------------------------------------------------
// stdio environment construction (local spawner parity)
// ---------------------------------------------------------------------------

/// Pins the local stdio env semantics to the ref's `extendEnv: true`
/// behavior (`{ ...process.env, ...opts.env }` in the cross-spawn
/// spawner): the host environment is inherited in full, `BUN_BE_BUN=1`
/// is layered on for `opencode` commands, and the server config's
/// `environment` overrides host values on collision.
mod stdio_environment {
    use super::client::stdio_environment;
    use std::collections::BTreeMap;

    fn host() -> Vec<(String, String)> {
        vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/home/user".to_string()),
            ("OPENCODE_HOST".to_string(), "leaked".to_string()),
            ("BUN_BE_BUN".to_string(), "host-value".to_string()),
        ]
    }

    #[test]
    fn inherits_host_environment_in_full() {
        // Nothing is stripped: PATH, HOME, and unknown host vars all cross.
        let env = stdio_environment(host(), "npx", None);
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/user"));
        assert_eq!(env.get("OPENCODE_HOST").map(String::as_str), Some("leaked"));
        // Not an opencode command: no BUN_BE_BUN is injected, but a host
        // value (if any) survives untouched.
        assert_eq!(env.get("BUN_BE_BUN").map(String::as_str), Some("host-value"));
    }

    #[test]
    fn opencode_command_gets_bun_be_bun() {
        let env = stdio_environment(host(), "opencode", None);
        assert_eq!(env.get("BUN_BE_BUN").map(String::as_str), Some("1"));
        // The rest of the host env is untouched.
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[test]
    fn config_environment_overrides_host_values() {
        let mut config = BTreeMap::new();
        config.insert("PATH".to_string(), "/custom/bin".to_string());
        config.insert("MCP_VAR".to_string(), "set".to_string());
        let env = stdio_environment(host(), "npx", Some(&config));
        assert_eq!(env.get("PATH").map(String::as_str), Some("/custom/bin"));
        assert_eq!(env.get("MCP_VAR").map(String::as_str), Some("set"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/user"));
    }

    #[test]
    fn config_environment_overrides_bun_be_bun() {
        // Config sits above BUN_BE_BUN in the merge order.
        let mut config = BTreeMap::new();
        config.insert("BUN_BE_BUN".to_string(), "0".to_string());
        let env = stdio_environment(host(), "opencode", Some(&config));
        assert_eq!(env.get("BUN_BE_BUN").map(String::as_str), Some("0"));
    }

    #[test]
    fn empty_config_keeps_host_env_verbatim() {
        let config = BTreeMap::new();
        let env = stdio_environment(host(), "npx", Some(&config));
        assert_eq!(env.get("BUN_BE_BUN").map(String::as_str), Some("host-value"));
        assert_eq!(env.len(), 4);
    }
}

// ---------------------------------------------------------------------------
// G-1 residual — stdio spawns route through the Environment seam
// ---------------------------------------------------------------------------

mod stdio_environment_seam {
    use super::*;
    use crate::environment::stdio::{
        LocalStdioSpawner, PipedProcess, StdioSpawner, StdioSpec, StopChild,
    };
    use std::sync::Mutex;

    /// Records the spawn request and hands back a child that speaks
    /// nothing (stdout is immediate EOF → the handshake fails fast).
    struct RecordingSpawner {
        recorded: Mutex<Option<StdioSpec>>,
    }

    #[async_trait::async_trait]
    impl StdioSpawner for RecordingSpawner {
        async fn spawn_piped(&self, spec: StdioSpec) -> Result<PipedProcess, String> {
            *self.recorded.lock().unwrap() = Some(spec);
            Ok(PipedProcess {
                stdin: Box::new(tokio::io::sink()),
                stdout: Box::new(tokio::io::empty()),
                stop: Box::new(|| {}) as StopChild,
                stderr_tail: None,
            })
        }
    }

    #[tokio::test]
    async fn connect_local_spawns_through_the_environment_stdio_seam() {
        let spawner = Arc::new(RecordingSpawner { recorded: Mutex::new(None) });
        let config = ServerConfig::Local(Box::new(LocalConfig {
            command: vec!["opencode".to_string(), "mcp".to_string(), "serve".to_string()],
            cwd: Some("sub".to_string()),
            environment: Some(BTreeMap::from([("MCP_VAR".to_string(), "set".to_string())])),
            disabled: None,
            codemode: None,
            timeout: None,
            protocol: None,
        }));
        let input = ConnectInput {
            server: "seam",
            config: &config,
            directory: "/base",
            bearer: None,
            elicitation: None,
            stdio: Some(spawner.clone()),
        };
        // The mute child ends the handshake — any ConnectError proves the
        // spawn reached OUR spawner (no host process was consulted).
        let result = tokio::time::timeout(Duration::from_secs(10), client::connect(input)).await;
        let failure = match result.expect("settles") {
            Ok(connection) => {
                connection.close().await;
                panic!("mute child cannot connect");
            }
            Err(failure) => failure,
        };
        assert!(matches!(failure, client::ConnectFailure::Connect(_)), "{failure:?}");
        let spec = spawner.recorded.lock().unwrap().take().expect("spawn recorded");
        assert_eq!(spec.command, "opencode");
        assert_eq!(spec.args, vec!["mcp".to_string(), "serve".to_string()]);
        // cwd resolves against the location directory.
        assert_eq!(spec.cwd, "/base/sub");
        // extend_env: the spawner (not the client) merges the execution
        // plane's environment underneath the overlay.
        assert!(spec.extend_env);
        // The overlay carries BUN_BE_BUN for `opencode` + the config env.
        assert_eq!(spec.env.get("BUN_BE_BUN").map(String::as_str), Some("1"));
        assert_eq!(spec.env.get("MCP_VAR").map(String::as_str), Some("set"));
    }

    /// G-72: a local server that dies immediately after printing to
    /// stderr surfaces the stderr tail in the connect-failure message.
    #[tokio::test]
    async fn connect_local_crash_diagnostics_include_stderr_tail() {
        struct PassthroughSpawner;
        #[async_trait::async_trait]
        impl StdioSpawner for PassthroughSpawner {
            async fn spawn_piped(&self, spec: StdioSpec) -> Result<PipedProcess, String> {
                LocalStdioSpawner.spawn_piped(spec).await
            }
        }
        let config = ServerConfig::Local(Box::new(LocalConfig {
            command: vec![
                "sh".to_string(),
                "-c".to_string(),
                "echo 'fatal: port taken' >&2; exit 3".to_string(),
            ],
            cwd: None,
            environment: None,
            disabled: None,
            codemode: None,
            timeout: None,
            protocol: None,
        }));
        let input = ConnectInput {
            server: "crasher",
            config: &config,
            directory: "/tmp",
            bearer: None,
            elicitation: None,
            stdio: Some(Arc::new(PassthroughSpawner)),
        };
        let result = tokio::time::timeout(Duration::from_secs(10), client::connect(input)).await;
        let failure = match result.expect("settles") {
            Ok(connection) => {
                connection.close().await;
                panic!("a dying child cannot connect");
            }
            Err(failure) => failure,
        };
        let client::ConnectFailure::Connect(error) = failure else {
            panic!("expected a plain connect error");
        };
        assert!(error.message.contains("fatal: port taken"), "{}", error.message);
    }

    /// Byte-for-byte parity with the previous direct spawn: the local
    /// spawner's `extend_env` merge reproduces `stdio_environment(host,
    /// ...)` exactly (the merge oracle the old client code computed).
    #[tokio::test]
    #[cfg(unix)]
    async fn local_spawner_extend_env_matches_stdio_environment() {
        let config_env = BTreeMap::from([
            ("PATH".to_string(), "/custom/bin".to_string()),
            ("MCP_VAR".to_string(), "set".to_string()),
        ]);
        let overlay = client::stdio_overlay("opencode", Some(&config_env));
        let mut process = LocalStdioSpawner
            .spawn_piped(StdioSpec {
                command: "/usr/bin/env".to_string(),
                args: vec![],
                cwd: "/tmp".to_string(),
                env: overlay,
                extend_env: true,
            })
            .await
            .expect("spawn env");
        use tokio::io::AsyncReadExt as _;
        let mut raw = Vec::new();
        process.stdout.read_to_end(&mut raw).await.expect("read env output");
        drop(process.stop); // reap-side cleanup; the child already exited
        let printed: BTreeMap<String, String> = String::from_utf8(raw)
            .expect("utf8")
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let expected = client::stdio_environment(std::env::vars(), "opencode", Some(&config_env));
        assert_eq!(printed, expected, "local seam merge is byte-for-byte the old env");
    }
}

// ===========================================================================
// G-223 — typed expiry/unauthorized classification (ref client.ts:230-243:
// status from the typed HTTP error; expiry gated on a session id being
// attached to the transport).
// ===========================================================================

mod g223_classification {
    use super::*;

    /// `DynamicTransportError` is `#[non_exhaustive]` (not constructible
    /// outside rmcp); the classifier also reaches the typed transport
    /// error by walking `source()` of any wrapper, which is exactly the
    /// production chain (`ServiceError::TransportSend` → boxed
    /// `StreamableHttpError`).
    #[derive(Debug, thiserror::Error)]
    #[error("Transport send error")]
    struct Wrap(
        #[source] rmcp::transport::streamable_http_client::StreamableHttpError<reqwest13::Error>,
    );

    /// The restarted single-session server's JSON-RPC rejection (reaches
    /// the classifier as `ServiceError::McpError`, no typed HTTP status).
    #[derive(Debug, thiserror::Error)]
    #[error("Mcp error: Bad Request: Server not initialized")]
    struct RestartedServer;

    /// A sessionful 404: rmcp's own `SessionExpired` variant — produced
    /// only when the POST carried the negotiated session id.
    #[test]
    fn g223_404_with_session_is_expired() {
        let error =
            Wrap(rmcp::transport::streamable_http_client::StreamableHttpError::SessionExpired);
        assert!(client::session_expired(&error, true));
        // The typed variant carries the session gate by construction — it
        // classifies even without the observing flag.
        assert!(client::session_expired(&error, false));
    }

    /// A STATELESS 404 (no session id attached): rmcp surfaces it as
    /// `UnexpectedServerResponse("HTTP 404 …")` — a plain error, NOT an
    /// expiry (the old string matcher classified any "404" text as
    /// expired).
    #[test]
    fn g223_404_without_session_is_not_expired() {
        let error = Wrap(
            rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                std::borrow::Cow::Borrowed("HTTP 404 Not Found: no such resource"),
            ),
        );
        assert!(!client::session_expired(&error, false));
        assert!(!client::unauthorized(&error));
    }

    /// 401s: the typed auth challenge, a challenge-less rendering, and a
    /// reqwest status error all classify unauthorized.
    #[test]
    fn g223_unauthorized_typed_variants() {
        let challenge =
            Wrap(rmcp::transport::streamable_http_client::StreamableHttpError::AuthRequired(
                rmcp::transport::streamable_http_client::AuthRequiredError::new(
                    "Bearer realm=\"mcp\"".to_string(),
                ),
            ));
        assert!(client::unauthorized(&challenge));
        assert!(!client::session_expired(&challenge, true));

        let bare = Wrap(
            rmcp::transport::streamable_http_client::StreamableHttpError::UnexpectedServerResponse(
                std::borrow::Cow::Borrowed("HTTP 401 Unauthorized: token revoked"),
            ),
        );
        assert!(client::unauthorized(&bare));

        let forbidden =
            Wrap(rmcp::transport::streamable_http_client::StreamableHttpError::InsufficientScope(
                rmcp::transport::streamable_http_client::InsufficientScopeError::new(
                    "Bearer scope=\"admin\"".to_string(),
                    Some("admin".to_string()),
                ),
            ));
        assert!(client::unauthorized(&forbidden));
    }

    /// Non-HTTP failures (timeouts, io) match nothing.
    #[test]
    fn g223_non_http_errors_do_not_match() {
        let timeout = rmcp::ServiceError::Timeout { timeout: Duration::from_secs(5) };
        assert!(!client::session_expired(&timeout, true));
        assert!(!client::unauthorized(&timeout));
    }

    /// The restarted single-session server's protocol error
    /// ("Bad Request: Server not initialized") classifies as expiry ONLY
    /// when a session was attached (ref gates the 400 branch the same
    /// way).
    #[test]
    fn g223_restarted_server_message_is_gated_on_session() {
        let error = RestartedServer;
        assert!(client::session_expired(&error, true));
        assert!(!client::session_expired(&error, false));
    }
}

// ---------------------------------------------------------------------------
// G-230 — the two MCP resource builtins declare their outputs; real
// executions through the pipeline validate against the schemas.
// ---------------------------------------------------------------------------

async fn g230_resources_fixture()
-> (McpRegistry, Arc<crate::location::ToolRegistryState>, Arc<crate::hooks::HookRegistry>, crate::permission::Permission) {
    let (_db, bus) = test_bus().await;
    let mut servers = BTreeMap::new();
    servers.insert("alpha".to_string(), local_config());
    let script = Script::new();
    script.resources.lock().push(serde_json::json!({
        "uri": "mock://text/1", "name": "Text one", "mimeType": "text/plain",
        "description": "A text resource"
    }));
    script.templates.lock().push(serde_json::json!({
        "uriTemplate": "mock://doc/{id}", "name": "Docs", "mimeType": "text/plain",
    }));
    let registry = registry_with(&bus, scripted_connector(move || script.clone()), servers);
    registry.reconcile().await;
    wait_for_status(&registry, "alpha", Status::Connected).await;
    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let (permission, _rx) = test_permission_with_rules(&bus, Some(allow_all_rules()));
    let services = test_services(&permission);
    crate::tool::mcp_resource::register(
        &tools,
        &hooks,
        &services,
        registry.clone(),
        permission.clone(),
    )
    .expect("register resource tools");
    (registry, tools, hooks, permission)
}

#[tokio::test]
async fn g230_list_mcp_resources_declared_output_validates() {
    let (registry, tools, _hooks, _permission) = g230_resources_fixture().await;
    let executor = tools.executor("opencode_list_mcp_resources").expect("executor");
    let result = executor
        .execute_with_context(&serde_json::json!({}), &g242_context())
        .await
        .expect("list executes");
    let output = result.output.expect("list returns declared output");
    let issues = crate::tool::registry::validate_input(
        &crate::tool::mcp_resource::list_output_schema(),
        &output,
    );
    assert!(issues.is_empty(), "list output {output} failed schema: {issues:?}");
    assert_eq!(output["resources"][0]["uri"], "mock://text/1");
    registry.shutdown().await;
}

#[tokio::test]
async fn g230_read_mcp_resource_declared_output_validates() {
    let (registry, tools, _hooks, _permission) = g230_resources_fixture().await;
    let executor = tools.executor("opencode_read_mcp_resource").expect("executor");
    let result = executor
        .execute_with_context(
            &serde_json::json!({ "server": "alpha", "uri": "mock://text/1" }),
            &g242_context(),
        )
        .await
        .expect("read executes");
    let output = result.output.expect("read returns declared output");
    let issues = crate::tool::registry::validate_input(
        &crate::tool::mcp_resource::read_output_schema(),
        &output,
    );
    assert!(issues.is_empty(), "read output {output} failed schema: {issues:?}");
    assert_eq!(output["contents"][0]["text"], "resource body");
    registry.shutdown().await;
}
