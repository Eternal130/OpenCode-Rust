//! G-453 residual (w13-g-empty-stream-error): a provider stream that
//! fails MID-PARSE (the anthropic-messages invalid-stream-event — the
//! empty text block start the w13 `empty` seed carries) must terminalize
//! the run: `session.execution.failed` on the wire and the engine's
//! active map back to empty. The observed sandbox defect: the error part
//! rendered (`session.step.failed`) but NO terminal execution event ever
//! fired, so the TUI's busy verdict (running map OR pending inbox) spun
//! forever while the ref settled idle.

mod support;

use std::sync::Arc;
use std::time::Duration;

use oc_client::types::{LocationPublicRef, SessionCreateBody, SessionPromptBody};
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The w13 cassette body byte-for-byte (seed-454b…): SSE blocks joined
/// by SINGLE newlines — only the last carries the blank-line event
/// terminator, so the whole body is ONE SSE event whose data lines
/// concatenate into one non-JSON document (the invalid-stream-event
/// probe BOTH engines reject; ref settles the error line).
const EMPTY_SEED_SSE: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
    event: message_start\n\
    data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_w13_seed\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-opus-4-7\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":32,\"output_tokens\":1}}}\n\
    event: content_block_start\n\
    data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
    event: content_block_stop\n\
    data: {\"type\":\"content_block_stop\",\"index\":0}\n\
    event: message_delta\n\
    data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"input_tokens\":32,\"output_tokens\":1}}\n\
    event: message_stop\n\
    data: {\"type\":\"message_stop\"}\n\n";

fn user_config(provider_port: u16) -> String {
    format!(
        r#"{{
            "model": "replay/claude-opus-4-7",
            "autoupdate": false,
            "providers": {{
                "replay": {{
                    "name": "Replay Anthropic",
                    "package": "@ai-sdk/anthropic",
                    "settings": {{
                        "baseURL": "http://127.0.0.1:{provider_port}/v1",
                        "apiKey": "replay-sandbox-key"
                    }},
                    "models": {{
                        "claude-opus-4-7": {{ "name": "claude-opus-4-7" }}
                    }}
                }}
            }}
        }}"#
    )
}

/// Serves the empty seed to every /v1/messages request.
async fn seed_provider(listener: TcpListener) {
    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(_) => return,
        };
        if read_one_request(&mut stream).await.is_some() {
            let _ = stream.write_all(EMPTY_SEED_SSE.as_bytes()).await;
        }
        let _ = stream.shutdown().await;
    }
}

async fn read_one_request(stream: &mut TcpStream) -> Option<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(position) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..position]).to_owned();
            let length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if buf.len() >= position + 4 + length {
                return Some(());
            }
        }
    }
}

struct TestServer {
    client: Client,
    addr: std::net::SocketAddr,
    state: Arc<AppState>,
    _workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
}

async fn boot(workspace: std::path::PathBuf, data: tempfile::TempDir) -> TestServer {
    std::env::set_current_dir(&workspace).expect("chdir to workspace");
    let engine =
        Engine::boot(EngineOptions::new(data.path().join("data"), data.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, addr, state, _workspace: workspace, _data: data }
}

/// The w13-g-empty face: the invalid-stream-event failure settles the
/// turn — `session.execution.failed` reaches the SSE wire (the TUI's
/// running-map clear feeds on exactly this) and the engine goes idle.
#[tokio::test]
async fn invalid_stream_event_terminalizes_the_run_on_the_wire() {
    let workspace = tempfile::tempdir().expect("workspace");
    let provider_listener = TcpListener::bind("127.0.0.1:0").await.expect("mock provider bind");
    let provider_port = provider_listener.local_addr().unwrap().port();
    std::fs::write(workspace.path().join("opencode.json"), user_config(provider_port))
        .expect("write config");
    tokio::spawn(async move {
        seed_provider(provider_listener).await;
    });

    let data = tempfile::tempdir().expect("data tempdir");
    let server = boot(workspace.path().to_path_buf(), data).await;
    let created = server
        .client
        .session()
        .create(&SessionCreateBody {
            location: Some(LocationPublicRef {
                directory: workspace.path().to_string_lossy().into_owned(),
            }),
            ..Default::default()
        })
        .await
        .expect("create session");
    let session_id = created.id.clone();

    // The SSE tap goes up BEFORE the prompt so the terminal event cannot
    // race past it.
    let (_response, mut body) = support::request_streaming(
        server.addr,
        "GET",
        "/api/event",
        &[("accept", "text/event-stream")],
        b"",
    )
    .await
    .expect("subscribe /api/event");

    tokio::time::timeout(
        Duration::from_secs(60),
        server.client.session().prompt(
            &session_id,
            &SessionPromptBody { text: "empty".to_owned(), ..Default::default() },
        ),
    )
    .await
    .expect("prompt admits")
    .expect("prompt is a session outcome, not an HTTP error");

    // THE FACE: a terminal execution event MUST arrive on the wire (the
    // sandbox defect had `session.step.failed` and then silence — the
    // TUI spun busy forever while the ref settled idle).
    let seen = tokio::time::timeout(Duration::from_secs(45), async {
        let mut collected = String::new();
        loop {
            let chunk = body
                .read_until("\n\n", Duration::from_secs(5))
                .await
                .unwrap_or_else(|_| String::new());
            collected.push_str(&chunk);
            if collected.contains("session.execution.failed")
                || collected.contains("session.execution.succeeded")
                || collected.contains("session.execution.interrupted")
            {
                return Ok(collected);
            }
            if chunk.is_empty() {
                return Err(collected);
            }
        }
    })
    .await
    .expect("bounded read window");
    let seen = seen.unwrap_or_else(|collected| {
        panic!("no terminal execution event within 45 s — collected SSE:\n{collected}")
    });
    assert!(
        seen.contains("session.execution.failed"),
        "the invalid-stream-event failure is a FAILED terminal, got: {seen}"
    );
    // The engine idles afterwards (active map drained — the busy verdict
    // in any TUI attached to the running map clears).
    let workspace_dir = workspace.path().to_path_buf();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let active = server
            .state
            .engine
            .location(oc_core::location::LocationRef::new(
                workspace_dir.clone(),
                None,
            ))
            .await
            .expect("location services")
            .execution
            .active();
        if active.is_empty() || std::time::Instant::now() > deadline {
            assert!(active.is_empty(), "the engine idles after the failed turn (active={active:?})");
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
