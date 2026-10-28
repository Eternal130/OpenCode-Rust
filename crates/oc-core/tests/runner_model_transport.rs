//! RV-P1 — the session runner consults the model transport (REF
//! model-request.ts:352 + runner llm.ts:242: `webSocket: "session"` binds
//! `SessionModelTransport` when `model.transport === "websocket"`).
//!
//! The seam under test is [`crate::session::runner::LlmClient::
//! stream_for_session`] — the surface step.rs calls with the resolved
//! model's transport. The production [`RouteLlmClient`] answer is exercised
//! end-to-end against a local WebSocket mock (ADR-0006: loopback only, no
//! real providers): the OpenResponses WS channel opens with the expected
//! ws:// URL + handshake headers, sticky-falls-back to HTTP SSE on refused
//! connects, and `transport: None` / WS-less routes never open a channel.

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use oc_ai::protocols::open_responses_channel::channel_options_for_route;
use oc_ai::protocols::openai_responses::{OpenAIResponsesRouteSpec, openai_responses_route};
use oc_ai::route::RouteDefaults;
use oc_ai::route::auth::{self, Secret};
use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::schema::{
    ContentPart, LLMEvent, LLMRequest, Message, MessageRole, SystemPart, TransportType,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use oc_core::session::{LlmClient as _, LlmEventStream, RouteLlmClient};

// ---------------------------------------------------------------------------
// Loopback mocks (transport_matrix.rs shrunk to the runner seam)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Handshake {
    path: String,
    headers: Vec<(String, String)>,
}

/// Loopback WS mock: records the handshake + create message, answers one
/// scripted reply-set per create.
struct WsMock {
    port: u16,
    handshakes: Arc<Mutex<Vec<Handshake>>>,
    creates: Arc<Mutex<Vec<Value>>>,
    replies: Arc<Mutex<std::collections::VecDeque<Vec<String>>>>,
}

async fn ws_mock(reply: Vec<String>) -> WsMock {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ws mock");
    let port = listener.local_addr().expect("ws mock port").port();
    let handshakes: Arc<Mutex<Vec<Handshake>>> = Arc::new(Mutex::new(Vec::new()));
    let creates: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let replies: Arc<Mutex<std::collections::VecDeque<Vec<String>>>> =
        Arc::new(Mutex::new(std::collections::VecDeque::from([reply])));
    tokio::spawn({
        let handshakes = Arc::clone(&handshakes);
        let creates = Arc::clone(&creates);
        let replies = Arc::clone(&replies);
        async move {
            while let Ok((stream, _)) = listener.accept().await {
                let handshakes = Arc::clone(&handshakes);
                let creates = Arc::clone(&creates);
                let replies = Arc::clone(&replies);
                tokio::spawn(async move {
                    let mut captured = Handshake::default();
                    let callback = |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                                    response: tokio_tungstenite::tungstenite::handshake::server::Response| {
                        captured.path = request.uri().to_string();
                        captured.headers = request
                            .headers()
                            .iter()
                            .map(|(name, value)| {
                                (name.as_str().to_string(), value.to_str().unwrap_or("").to_string())
                            })
                            .collect();
                        Ok(response)
                    };
                    let Ok(mut websocket) =
                        tokio_tungstenite::accept_hdr_async(stream, callback).await
                    else {
                        return;
                    };
                    handshakes.lock().await.push(captured.clone());
                    while let Some(message) = websocket.next().await {
                        let text = match message {
                            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => text,
                            Ok(_) => continue,
                            Err(_) => break,
                        };
                        let Ok(create) = serde_json::from_str::<Value>(&text) else { continue };
                        creates.lock().await.push(create);
                        let reply_set = replies.lock().await.pop_front().unwrap_or_default();
                        for frame in reply_set {
                            if websocket
                                .send(tokio_tungstenite::tungstenite::Message::Text(frame))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                });
            }
        }
    });
    WsMock { port, handshakes, creates, replies }
}

impl WsMock {
    async fn queue(&self, frames: Vec<String>) {
        self.replies.lock().await.push_back(frames);
    }

    async fn handshake(&self) -> Handshake {
        for _ in 0..200 {
            if let Some(handshake) = self.handshakes.lock().await.first() {
                return handshake.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("handshake never arrived");
    }

    async fn create(&self, index: usize) -> Value {
        for _ in 0..200 {
            if let Some(create) = self.creates.lock().await.get(index) {
                return create.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("create message {index} never arrived");
    }
}

/// Loopback HTTP mock (raw TCP): records the request line of every request,
/// answers one canned Responses SSE stream.
struct HttpMock {
    port: u16,
    request_lines: Arc<Mutex<Vec<String>>>,
}

const SSE_REPLY: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_http\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_http\",\"output\":[]}}\n\n",
);

async fn http_mock() -> HttpMock {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind http mock");
    let port = listener.local_addr().expect("http mock port").port();
    let request_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn({
        let request_lines = Arc::clone(&request_lines);
        async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let request_lines = Arc::clone(&request_lines);
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    let mut buffer = [0u8; 16384];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let request_line = text
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .split(' ')
                        .nth(1)
                        .unwrap_or_default()
                        .to_string();
                    request_lines.lock().await.push(request_line);
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                        SSE_REPLY.len(),
                        SSE_REPLY
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        }
    });
    HttpMock { port, request_lines }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// The `openai-responses` route over the loopback HTTP mock — the WS
/// handshake derives from the same base URL (`channel_options_for_route`
/// uses the identity URL map), so a WS mock on ANOTHER port is not
/// reachable through this seam; the happy-path test instead points the
/// route at the WS mock port (the WS server also absorbs the fallback POST
/// badly enough that a silent HTTP fallback fails the assertions).
fn route(port: u16) -> Route {
    openai_responses_route(OpenAIResponsesRouteSpec {
        base_url: Some(format!("http://127.0.0.1:{port}/v1")),
        auth: Some(auth::Definition::bearer(Secret::Value("rvpe-test".into()))),
        transport: Arc::new(ReqwestTransport::new()),
        defaults: RouteDefaults::default(),
    })
}

use oc_ai::route::Route;

fn request(route: &Route) -> LLMRequest {
    let model = route.model("gpt-4.1-mini", None).expect("model");
    let mut request = LLMRequest::new(model);
    request.system = vec![SystemPart::new("You are concise.")];
    request.messages = vec![Message::new(MessageRole::User, vec![ContentPart::text("hi")])];
    request
}

fn frames_for(text: &str, response_id: &str) -> Vec<String> {
    let assistant_output = json!({
        "type": "message",
        "role": "assistant",
        "id": "msg_1",
        "status": "completed",
        "content": [{"type": "output_text", "text": text}],
    });
    vec![
        json!({"type": "response.created", "response": {"id": response_id}}).to_string(),
        json!({"type": "response.output_item.done", "output_index": 0, "item": assistant_output}).to_string(),
        json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": text}).to_string(),
        json!({"type": "response.completed", "response": {"id": response_id, "output": [assistant_output]}}).to_string(),
    ]
}

/// Collect until the terminal event (or stream end).
async fn collect_events(stream: LlmEventStream) -> Vec<Result<LLMEvent, oc_ai::schema::AiError>> {
    let mut collected = Vec::new();
    let mut stream = stream;
    while let Some(item) = stream.next().await {
        let terminal = item.as_ref().is_ok_and(LLMEvent::is_terminal);
        collected.push(item);
        if terminal {
            break;
        }
    }
    collected
}

fn assert_clean_terminal(events: &[Result<LLMEvent, oc_ai::schema::AiError>]) {
    assert!(
        events.iter().all(Result::is_ok),
        "stream error: {:?}",
        events.iter().filter_map(|item| item.as_ref().err()).collect::<Vec<_>>()
    );
    assert!(
        events.iter().any(|item| item.as_ref().is_ok_and(LLMEvent::is_terminal)),
        "expected a terminal event"
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Unit: transport selection follows `ResolvedModel.transport` and the
/// route's protocol (openai-responses incl. the Azure flavor offer the
/// channel; xai-responses and every non-responses protocol stay on HTTP).
#[test]
fn channel_options_follow_route_protocol() {
    let responses = route(1);
    let options =
        channel_options_for_route(&responses).expect("openai-responses offers the channel");
    assert_eq!(options.id, "openai-responses");

    let xai = oc_ai::protocols::xai_responses::xai_responses_route(
        oc_ai::protocols::xai_responses::XaiResponsesRouteSpec {
            base_url: Some("http://127.0.0.1:9/v1".into()),
            auth: None,
            transport: Arc::new(oc_ai::route::transport::FailingTransport {
                message: "never streamed",
            }),
            defaults: RouteDefaults::default(),
        },
    );
    assert!(
        channel_options_for_route(&xai).is_none(),
        "xai-responses has no WS channel (REF xai-responses.ts wires none)"
    );
}

/// End-to-end leg: `transport: websocket` + an OpenResponses route streams
/// through the session channel — the handshake hits the ws:// form of the
/// endpoint with the protocol + auth headers, and the request is one
/// `response.create` message (stream options stripped).
#[tokio::test]
async fn websocket_transport_opens_session_channel() {
    let server = ws_mock(frames_for("hello", "resp_ws")).await;
    let client = RouteLlmClient;
    let route = route(server.port);

    let stream = client
        .stream_for_session(
            "sess-rvpe-ws",
            Some(oc_schema::provider::Transport::Websocket),
            request(&route),
        )
        .await
        .expect("channel stream");
    let events = collect_events(stream).await;
    assert_clean_terminal(&events);

    let handshake = server.handshake().await;
    assert!(handshake.path.starts_with("/v1/responses"), "path: {}", handshake.path);
    assert!(
        handshake.headers.iter().any(|(name, value)| name == "openai-beta"
            && value == oc_ai::protocols::openai_responses::WEBSOCKET_PROTOCOL_HEADER),
        "openai-beta handshake header missing: {:?}",
        handshake.headers
    );
    assert!(
        handshake
            .headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer rvpe-test"),
        "auth headers ride the handshake"
    );
    let create = server.create(0).await;
    assert_eq!(create["type"], "response.create");
    assert!(create.get("stream").is_none());
    assert!(create.get("stream_options").is_none());
}

/// Sticky HTTP fallback (REF model-transport.ts: a refused connect pins
/// the session to HTTP): the WS handshake dies against the plain HTTP
/// mock, the same request streams over SSE, and the session reuses HTTP
/// afterwards (the second call never sends a WS handshake — no GET on the
/// wire).
#[tokio::test]
async fn websocket_transport_falls_back_to_http_on_connect_failure() {
    let http = http_mock().await;
    let client = RouteLlmClient;
    let route = route(http.port);

    for _ in 0..2 {
        let stream = client
            .stream_for_session(
                "sess-rvpe-fallback",
                Some(oc_schema::provider::Transport::Websocket),
                request(&route),
            )
            .await
            .expect("channel stream");
        let events = collect_events(stream).await;
        assert_clean_terminal(&events);
    }

    let lines = http.request_lines.lock().await.clone();
    assert_eq!(lines.len(), 3, "one refused WS GET + two HTTP POSTs: {lines:?}");
    assert!(lines[0].starts_with("/v1/responses"), "first attempt is the WS handshake GET");
    assert!(
        lines[1..].iter().all(|line| line.starts_with("/v1/responses")),
        "HTTP fallback POSTs the same endpoint"
    );
}

/// `transport: None` (the default for every provider without an explicit
/// websocket setting) never opens a channel: one plain HTTP POST.
#[tokio::test]
async fn http_transport_never_opens_a_channel() {
    let http = http_mock().await;
    let client = RouteLlmClient;
    let route = route(http.port);

    let stream = client
        .stream_for_session("sess-rvpe-http", None, request(&route))
        .await
        .expect("http stream");
    let events = collect_events(stream).await;
    assert_clean_terminal(&events);

    let lines = http.request_lines.lock().await.clone();
    assert_eq!(lines, vec!["/v1/responses".to_string()], "exactly one HTTP POST, no WS GET");
}

/// `close_transport` closes the session channel (REF llm.ts:78
/// `modelTransport.close`): after close, the pinned-HTTP/http-fallback
/// state resets and the owner refuses nothing new — a fresh bind works.
#[tokio::test]
async fn close_transport_closes_the_session_channel() {
    let server = ws_mock(frames_for("hello", "resp_close")).await;
    let client = RouteLlmClient;
    let route = route(server.port);

    let stream = client
        .stream_for_session(
            "sess-rvpe-close",
            Some(oc_schema::provider::Transport::Websocket),
            request(&route),
        )
        .await
        .expect("channel stream");
    assert_clean_terminal(&collect_events(stream).await);

    client.close_transport("sess-rvpe-close").await;

    // The channel closed with the session; a fresh exchange reopens the
    // socket (REF close semantics: one socket per bind window).
    server.queue(frames_for("again", "resp_close2")).await;
    let stream = client
        .stream_for_session(
            "sess-rvpe-close",
            Some(oc_schema::provider::Transport::Websocket),
            request(&route),
        )
        .await
        .expect("channel stream after close");
    assert_clean_terminal(&collect_events(stream).await);
    let create = server.create(1).await;
    assert_eq!(create["type"], "response.create");
    assert!(
        create.get("previous_response_id").is_none(),
        "a closed channel restarts from a full send"
    );
}

/// Marker: the WS path is a websocket transport error taxonomy citizen.
#[test]
fn websocket_errors_carry_the_websocket_transport_type() {
    let error = oc_ai::route::transport::websocket::transport_error(
        "marker",
        oc_ai::schema::TransportOperation::Request,
        None,
        None,
        None,
        None,
    );
    assert!(matches!(
        error.reason,
        oc_ai::schema::AiErrorReason::Transport { transport: TransportType::Websocket, .. }
    ));
}
