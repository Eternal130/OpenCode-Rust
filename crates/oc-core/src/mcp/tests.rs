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
struct Script {
    tools: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    resources: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    templates: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
    calls: BTreeMap<String, serde_json::Value>,
    call_errors: BTreeMap<String, String>,
    instructions: Option<String>,
    /// The initialize requests the client offered (protocol-offer probe).
    offers: Arc<parking_lot::Mutex<Vec<serde_json::Value>>>,
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
}

impl Script {
    fn new() -> Self {
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

#[allow(clippy::unnecessary_wraps)] // the `_` request arm has no response
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
            response(
                serde_json::json!({
                        // The mock echoes the client's offered version back —
                        // v2.0.24's TS client offers `2025-11-25` (its
                        // SUPPORTED_PROTOCOL_VERSIONS[0], legacy handshake).
                        "protocolVersion": rmcp::model::ProtocolVersion::V_2025_11_25.to_string(),
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
        ClientRequest::ListToolsRequest(_) => {
            response(serde_json::json!({ "tools": *script.tools.lock() }), id)
        }
        ClientRequest::ListResourcesRequest(_) => {
            response(serde_json::json!({ "resources": *script.resources.lock() }), id)
        }
        ClientRequest::ListResourceTemplatesRequest(_) => {
            response(serde_json::json!({ "resourceTemplates": *script.templates.lock() }), id)
        }
        ClientRequest::ListPromptsRequest(_) => response(serde_json::json!({ "prompts": [] }), id),
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
                return Some(JsonRpcMessage::error(
                    rmcp::ErrorData::new(
                        rmcp::model::ErrorCode::INTERNAL_ERROR,
                        "HTTP status client error (404 Not Found)",
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

fn local_config() -> ServerConfig {
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
    };
    let connection =
        client::connect_with_transport(&input, transport, false).await.expect("mock connect");
    (connection, mock)
}

/// A connector scripted per-call: each invocation connects a fresh mock.
fn scripted_connector(next: impl FnMut() -> Script + Send + Sync + 'static) -> Connector {
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
    };
    // Remote classification requires remote=true.
    let connection =
        client::connect_with_transport(&input, transport, true).await.expect("connect");
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
            client::connect_with_transport(
                &ConnectInput {
                    server: &server,
                    config: &config,
                    directory: &directory,
                    bearer,
                    elicitation,
                },
                transport,
                true,
            )
            .await
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
    let bridge = super::registry::McpToolBridge::install(&registry, &tools, &permission);
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

fn test_permission(_bus: &crate::bus::Bus) -> crate::permission::Permission {
    use crate::permission::*;
    let ids: Arc<dyn oc_util::determinism::IdGen> =
        Arc::new(oc_util::determinism::SequentialIdGen::new());
    Permission::new(
        ids,
        Arc::new(oc_util::determinism::FixedClock::epoch()),
        Arc::new(crate::permission::BroadcastEventSink::new(8)),
        Arc::new(crate::permission::saved::MemorySavedGrantStore::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
        )),
        Arc::new(FixedRules),
        oc_schema::id::ProjectId::global(),
        None,
    )
}

use crate::permission::RuleResolver;

#[derive(Default)]
struct FixedRules;

#[async_trait::async_trait]
impl RuleResolver for FixedRules {
    async fn resolve(
        &self,
        _session_id: &oc_schema::id::SessionId,
        _agent: Option<&oc_schema::id::AgentId>,
    ) -> Result<
        (Option<oc_schema::permission::Ruleset>, Option<oc_schema::permission::Ruleset>),
        crate::permission::PermissionError,
    > {
        Ok((None, None))
    }
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
    assert!(result.content.is_empty());
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
