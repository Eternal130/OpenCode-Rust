//! C7b AI-PROVIDERS transport matrix — WebSocket path (ledger `AI-M1`,
//! cluster C7).
//!
//! v2 streams `openai-responses` over a WebSocket channel when the caller
//! supplies a channel executor (`packages/ai/src/protocols/open-responses-
//! channel.ts` + `open-responses-continuation.ts` + `packages/core/src/
//! session/model-transport.ts`): the request becomes one
//! `{"type":"response.create", ...}` message (stream options stripped), the
//! response is a frame stream, and the session channel reuses one socket
//! across exchanges, resuming with `previous_response_id` + the input
//! suffix when the new input extends the completed baseline.
//!
//! The mock split mirrors the channel options' `url` seam: the route's HTTP
//! endpoint is a local SSE mock (the channel fallback), while the channel
//! `url` map points the WebSocket handshake at a separate local WS mock —
//! the same seam Azure uses to strip `api-version`. All servers bind
//! `127.0.0.1:0` (ADR-0006: no real network, no real providers).

#![allow(
    clippy::result_large_err,
    reason = "tungstenite handshake error type is fixed by the mock callback"
)]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use oc_ai::protocols::open_responses_channel::{stream_channel, ChannelOptions};
use oc_ai::protocols::openai_responses::{
    channel_transport, openai_responses_route, OpenAIResponsesRouteSpec, WEBSOCKET_PROTOCOL_HEADER,
};
use oc_ai::route::auth::{self, Secret};
use oc_ai::route::transport::reqwest_transport::ReqwestTransport;
use oc_ai::route::transport::websocket::{
    SessionChannel, SessionChannelConfig, WebSocketChannelExecutor,
};
use oc_ai::route::{Route, RouteDefaults};
use oc_ai::schema::{
    AiError, ContentPart, GenerationOptions, LLMEvent, LLMRequest, Message, MessageRole,
    SystemPart, TransportDelivery, TransportRecovery, TransportType,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

// ---------------------------------------------------------------------------
// Local WebSocket mock server (127.0.0.1:0)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Handshake {
    path: String,
    headers: Vec<(String, String)>,
}

/// Scriptable WS mock: records handshakes + create messages; each queued
/// reply-set answers the next create message on the current connection.
struct WsMock {
    port: u16,
    connections: Arc<AtomicUsize>,
    handshakes: Arc<Mutex<Vec<Handshake>>>,
    creates: Arc<Mutex<Vec<Value>>>,
    replies: Arc<Mutex<VecDeque<Vec<String>>>>,
}

async fn ws_mock() -> WsMock {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ws mock");
    let port = listener.local_addr().expect("ws mock port").port();
    let connections = Arc::new(AtomicUsize::new(0));
    let handshakes: Arc<Mutex<Vec<Handshake>>> = Arc::new(Mutex::new(Vec::new()));
    let creates: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let replies: Arc<Mutex<VecDeque<Vec<String>>>> = Arc::new(Mutex::new(VecDeque::new()));
    tokio::spawn({
        let connections = Arc::clone(&connections);
        let handshakes = Arc::clone(&handshakes);
        let creates = Arc::clone(&creates);
        let replies = Arc::clone(&replies);
        async move {
            while let Ok((stream, _)) = listener.accept().await {
                let connections = Arc::clone(&connections);
                let handshakes = Arc::clone(&handshakes);
                let creates = Arc::clone(&creates);
                let replies = Arc::clone(&replies);
                tokio::spawn(async move {
                    let mut captured = Handshake::default();
                    let callback = |request: &Request, response: Response| {
                        captured.path = request.uri().to_string();
                        captured.headers = request
                            .headers()
                            .iter()
                            .map(|(name, value)| {
                                (
                                    name.as_str().to_string(),
                                    value.to_str().unwrap_or("").to_string(),
                                )
                            })
                            .collect();
                        Ok(response)
                    };
                    let Ok(mut websocket) =
                        tokio_tungstenite::accept_hdr_async(stream, callback).await
                    else {
                        return;
                    };
                    connections.fetch_add(1, Ordering::SeqCst);
                    handshakes.lock().await.push(Handshake {
                        path: captured.path.clone(),
                        headers: captured.headers.clone(),
                    });
                    while let Some(message) = websocket.next().await {
                        let text = match message {
                            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => text,
                            Ok(_) => continue,
                            Err(_) => break,
                        };
                        let Ok(create) = serde_json::from_str::<Value>(&text) else { continue };
                        creates.lock().await.push(create);
                        let reply_set = {
                            let mut queue = replies.lock().await;
                            queue.pop_front().unwrap_or_default()
                        };
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
    WsMock { port, connections, handshakes, creates, replies }
}

impl WsMock {
    async fn queue(&self, frames: Vec<String>) {
        self.replies.lock().await.push_back(frames);
    }

    async fn handshake(&self, index: usize) -> Handshake {
        let mut waited = 0;
        loop {
            let handshakes = self.handshakes.lock().await;
            if let Some(handshake) = handshakes.get(index) {
                return handshake.clone();
            }
            drop(handshakes);
            assert!(waited < 200, "handshake {index} never arrived");
            waited += 1;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn create(&self, index: usize) -> Value {
        let mut waited = 0;
        loop {
            let creates = self.creates.lock().await;
            if let Some(create) = creates.get(index) {
                return create.clone();
            }
            drop(creates);
            assert!(waited < 200, "create message {index} never arrived");
            waited += 1;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

/// Local HTTP SSE mock backing the channel fallback (raw TCP; records the
/// request line + body, answers one canned Responses SSE stream).
struct HttpSseMock {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    bodies: Arc<Mutex<Vec<String>>>,
}

async fn http_sse_mock() -> HttpSseMock {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind http mock");
    let port = listener.local_addr().expect("port").port();
    let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn({
        let requests = Arc::clone(&requests);
        let bodies = Arc::clone(&bodies);
        async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let requests = Arc::clone(&requests);
                let bodies = Arc::clone(&bodies);
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
                    let body = text.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
                    requests.lock().await.push(request_line);
                    bodies.lock().await.push(body);
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
    HttpSseMock { port, requests, bodies }
}

const SSE_REPLY: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_http\"}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_http\",\"output\":[]}}\n\n",
);

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn assistant_output(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "assistant",
        "id": "msg_1",
        "status": "completed",
        "content": [{"type": "output_text", "text": text}],
    })
}

fn frames_for(text: &str, response_id: &str) -> Vec<String> {
    vec![
        json!({"type": "response.created", "response": {"id": response_id}}).to_string(),
        json!({"type": "response.output_item.done", "output_index": 0, "item": assistant_output(text)}).to_string(),
        json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": text}).to_string(),
        json!({"type": "response.completed", "response": {"id": response_id, "output": [assistant_output(text)]}}).to_string(),
    ]
}

fn session_channel() -> Arc<SessionChannel> {
    SessionChannel::new(
        Arc::new(
            oc_ai::route::transport::websocket::tungstenite_connector::TokioTungsteniteConnector,
        ),
        SessionChannelConfig {
            connect_timeout: Some(Duration::from_secs(5)),
            ..Default::default()
        },
    )
}

fn channel_route(http_port: u16) -> Route {
    openai_responses_route(OpenAIResponsesRouteSpec {
        base_url: Some(format!("http://127.0.0.1:{http_port}/v1")),
        auth: Some(auth::Definition::bearer(Secret::Value("ws-test".into()))),
        transport: Arc::new(ReqwestTransport::new()),
        defaults: RouteDefaults::default(),
    })
}

/// Channel options with the WS handshake pointed at the mock: the `url` seam
/// v2's Azure provider uses for its own endpoint mapping.
fn ws_options(ws_port: u16) -> ChannelOptions {
    channel_transport()
        .with_url(Arc::new(move |_http_url| format!("http://127.0.0.1:{ws_port}/v1/responses")))
}

fn executor_for(channel: &Arc<SessionChannel>) -> Arc<dyn WebSocketChannelExecutor> {
    channel.clone()
}

fn channel_request(route: &Route, turns: &[(&str, &str)]) -> LLMRequest {
    let model = route.model("gpt-4.1-mini", None).expect("model");
    let mut request = LLMRequest::new(model);
    request.system = vec![SystemPart::new("You are concise.")];
    request.messages = turns
        .iter()
        .map(|(role, text)| {
            let role = match *role {
                "assistant" => MessageRole::Assistant,
                _ => MessageRole::User,
            };
            Message::new(role, vec![ContentPart::text(*text)])
        })
        .collect();
    request.generation = Some(GenerationOptions {
        max_tokens: Some(20.0),
        temperature: Some(0.0),
        ..Default::default()
    });
    request
}

async fn collect_events(
    stream: impl futures::Stream<Item = Result<LLMEvent, AiError>>,
) -> Vec<Result<LLMEvent, AiError>> {
    let mut collected = Vec::new();
    let mut stream = Box::pin(stream);
    while let Some(item) = stream.next().await {
        let terminal = item.as_ref().is_ok_and(LLMEvent::is_terminal);
        collected.push(item);
        if terminal {
            break;
        }
    }
    collected
}

fn assert_terminal_finish(events: &[Result<LLMEvent, AiError>], label: &str) {
    assert!(
        events.iter().all(std::result::Result::is_ok),
        "{label}: unexpected stream error: {:?}",
        events.iter().filter_map(|item| item.as_ref().err()).collect::<Vec<_>>()
    );
    assert!(
        events.iter().any(|item| item.as_ref().is_ok_and(LLMEvent::is_terminal)),
        "{label}: expected a terminal event"
    );
}

async fn wait_for_connections(mock: &WsMock, count: usize) {
    let mut waited = 0;
    while mock.connection_count() < count && waited < 300 {
        waited += 1;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// ---------------------------------------------------------------------------
// Matrix rows
// ---------------------------------------------------------------------------

#[tokio::test]
async fn responses_ws_resume() {
    let server = ws_mock().await;
    let http = http_sse_mock().await;
    let route = channel_route(http.port);
    let channel = session_channel();
    let options = ws_options(server.port);

    // Exchange 1: full send over the WS channel.
    server.queue(frames_for("hello", "resp_1")).await;
    let request = channel_request(&route, &[("user", "hi")]);
    let stream = stream_channel(&route, request, executor_for(&channel), &options)
        .await
        .expect("channel stream");
    let events = collect_events(stream.events).await;
    assert_terminal_finish(&events, "exchange 1 events");
    let checkpoint = (stream.complete)().expect("checkpoint after exchange 1");
    assert_eq!(checkpoint.value["responseID"], "resp_1");
    assert_eq!(
        checkpoint.protocol, "open-responses.websocket.v1",
        "checkpoint carries the continuation protocol"
    );

    // The handshake went to the ws form of the mapped URL with the
    // openai-beta protocol header and the auth headers.
    let handshake = server.handshake(0).await;
    assert!(handshake.path.starts_with("/v1/responses"), "path: {}", handshake.path);
    assert!(
        handshake
            .headers
            .iter()
            .any(|(name, value)| name == "openai-beta" && value == WEBSOCKET_PROTOCOL_HEADER),
        "openai-beta handshake header missing: {:?}",
        handshake.headers
    );
    assert!(
        handshake
            .headers
            .iter()
            .any(|(name, value)| name == "authorization" && value == "Bearer ws-test"),
        "auth headers ride the handshake"
    );

    // The create message is one response.create JSON with the stream
    // options stripped — not the SSE body.
    let create1 = server.create(0).await;
    assert_eq!(create1["type"], "response.create");
    assert!(create1.get("stream").is_none());
    assert!(create1.get("stream_options").is_none());
    assert_eq!(create1["input"].as_array().map(Vec::len), Some(1));

    // Exchange 2: same socket, previous_response_id + the input suffix.
    server.queue(frames_for("hello again", "resp_2")).await;
    let request =
        channel_request(&route, &[("user", "hi"), ("assistant", "hello"), ("user", "again")]);
    let stream = stream_channel(&route, request, executor_for(&channel), &options)
        .await
        .expect("channel stream 2");
    let events = collect_events(stream.events).await;
    assert_terminal_finish(&events, "exchange 2 events");
    let checkpoint = (stream.complete)().expect("checkpoint after exchange 2");
    assert_eq!(checkpoint.value["responseID"], "resp_2");

    // one socket for both exchanges
    wait_for_connections(&server, 1).await;
    assert_eq!(server.connection_count(), 1, "session channel reuses the socket");

    let create2 = server.create(1).await;
    assert_eq!(create2["type"], "response.create");
    assert_eq!(create2["previous_response_id"], "resp_1");
    let delta = create2["input"].as_array().expect("incremental input");
    assert_eq!(delta.len(), 1, "only the suffix is sent: {delta:?}");
    assert_eq!(delta[0]["content"][0]["text"], "again");
    assert_eq!(delta[0]["role"], "user");
}

#[tokio::test]
async fn responses_ws_rejected_previous_response_not_found() {
    let server = ws_mock().await;
    let http = http_sse_mock().await;
    let route = channel_route(http.port);
    let channel = session_channel();
    let options = ws_options(server.port);

    // Exchange 1 completes and stages the checkpoint.
    server.queue(frames_for("hello", "resp_1")).await;
    let request = channel_request(&route, &[("user", "hi")]);
    let stream =
        stream_channel(&route, request, executor_for(&channel), &options).await.expect("stream 1");
    collect_events(stream.events).await;
    assert!((stream.complete)().is_some(), "checkpoint staged");

    // Exchange 2: the provider rejects the stale previous_response_id —
    // the continuation maps it to retry-full on a websocket transport error.
    server
        .queue(vec![json!({
            "type": "error",
            "code": "previous_response_not_found",
            "message": "previous response not found"
        })
        .to_string()])
        .await;
    let request =
        channel_request(&route, &[("user", "hi"), ("assistant", "hello"), ("user", "again")]);
    let stream =
        stream_channel(&route, request, executor_for(&channel), &options).await.expect("stream 2");
    let events = collect_events(stream.events).await;
    let error = events
        .into_iter()
        .find_map(std::result::Result::err)
        .unwrap_or_else(|| panic!("expected a stream error"));
    match &error.reason {
        oc_ai::schema::AiErrorReason::Transport { transport, delivery, recovery, .. } => {
            assert_eq!(*transport, TransportType::Websocket);
            assert_eq!(*delivery, Some(TransportDelivery::Rejected));
            assert_eq!(*recovery, Some(TransportRecovery::RetryFull));
        }
        reason => panic!("expected a rejected transport error, got {reason:?}"),
    }
    assert!((stream.complete)().is_none(), "no checkpoint after a rejected exchange");

    // The socket is dropped after error frames (Codex stops serving a
    // connection after any error) — the next exchange reconnects.
    server.queue(frames_for("ok", "resp_3")).await;
    let request = channel_request(
        &route,
        &[("user", "hi"), ("assistant", "hello"), ("user", "again"), ("user", "more")],
    );
    let stream =
        stream_channel(&route, request, executor_for(&channel), &options).await.expect("stream 3");
    let events = collect_events(stream.events).await;
    assert_terminal_finish(&events, "post-error reconnect events");
    wait_for_connections(&server, 2).await;
    assert_eq!(server.connection_count(), 2, "error terminal dropped the socket");
}

#[tokio::test]
async fn responses_ws_connect_failure_falls_back_to_http() {
    // No WebSocket listener: the WS port is a bound-then-dropped socket.
    let dead_port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("port").port();
        drop(listener);
        port
    };
    let http = http_sse_mock().await;
    let route = channel_route(http.port);
    let channel = session_channel();
    let options = ws_options(dead_port);

    let request = channel_request(&route, &[("user", "hi")]);
    let stream = stream_channel(&route, request, executor_for(&channel), &options)
        .await
        .expect("channel stream");
    let events = collect_events(stream.events).await;
    assert_terminal_finish(&events, "fallback events");
    assert!(channel.is_http_fallback(), "session pinned to HTTP after refused connect");
    // the HTTP mock served the fallback request
    let requests = http.requests.lock().await.clone();
    assert!(!requests.is_empty(), "HTTP fallback used");
    assert!(requests[0].contains("/v1/responses"), "fallback URL: {}", requests[0]);
    let bodies = http.bodies.lock().await.clone();
    assert!(bodies[0].contains("\"stream\""), "SSE body keeps the stream field");
}

#[tokio::test]
async fn responses_ws_gate_off_falls_back_to_http() {
    // An endpoint the options decline never offers the channel:
    // stream_channel runs the plain HTTP path with no checkpoint.
    let http = http_sse_mock().await;
    let route = channel_route(http.port);
    let options = ChannelOptions::new("gated", "Gated").with_enabled(Arc::new(|_url| false));
    let channel = session_channel();
    let request = channel_request(&route, &[("user", "hi")]);
    let stream = stream_channel(&route, request, executor_for(&channel), &options)
        .await
        .expect("channel stream");
    let events = collect_events(stream.events).await;
    assert_terminal_finish(&events, "gated events");
    assert!((stream.complete)().is_none(), "no checkpoint over HTTP");
    let requests = http.requests.lock().await.clone();
    assert_eq!(requests.len(), 1, "plain HTTP request served");
}
