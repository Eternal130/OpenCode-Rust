//! WIRE-cluster parity tests (ledger C4a): public event payload shapes.
//!
//! - EV-1 `persistent_pty_added_shape` — the `persistent-pty.added` bus
//!   event's `terminal` must be the wire `PersistentPty.Info`: camelCase,
//!   `size: {cols, rows}` and `output: {head, tail}` objects, `exitCode`
//!   only once exited — `packages/core/src/persistent-pty/index.ts:178`
//!   publishes `toInfo(...)`; `schema/src/persistent-pty.ts:13-19`.
//!   The frame is captured off the production bus tap (the same feed
//!   `GET /api/event` rides).

use std::sync::Arc;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::Value;

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    #[allow(dead_code)] // the workspace root the server was booted in
    workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    // Hermetic (R3): persistent-terminal tests pin the in-process
    // backend — the one-release OC_PTY_DAEMON=0 escape hatch.
    std::env::set_var("OC_PTY_DAEMON", "0");
    let dir = tempfile::tempdir().expect("tempdir");
    let data = tempfile::tempdir().expect("data tempdir");
    let workspace = dir.path().to_path_buf();
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
    TestServer { client, state, workspace, _data: data }
}

// ---------------------------------------------------------------------------
// EV-1 — persistent-pty.added terminal shape
// ---------------------------------------------------------------------------

#[tokio::test]
async fn persistent_pty_added_shape() {
    let server = boot().await;
    let session_id = server
        .client
        .session()
        .create(&SessionCreateBody { title: Some("ev1".to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id;

    // The production bus tap (the SSE handler's own feed source).
    let feed = Arc::new(oc_server::sse::EventFeed::new());
    feed.spawn_tap(&server.state.engine.bus);
    let mut subscription = feed.subscribe();

    // Create a terminal through the production REST route (the typed client
    // doubles as the wire oracle for the REST shape).
    let created = server
        .client
        .persistent_pty()
        .create(
            &session_id,
            &PersistentPtyCreateInput {
                title: "ev1-terminal".to_owned(),
                command: Some("sh".to_owned()),
                args: vec!["-c".to_owned(), "sleep 30".to_owned()],
                ..Default::default()
            },
        )
        .await
        .expect("terminal create");

    // Wait for the persistent-pty.added frame.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let event: Value = loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "persistent-pty.added not observed within 10s"
        );
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), subscription.recv())
            .await
            .expect("frame within 5s")
            .expect("feed alive")
            .expect("ok frame");
        if let Some(payload) = frame.strip_prefix("data: ").filter(|payload| {
            serde_json::from_str::<Value>(payload.trim())
                .is_ok_and(|event| event["type"] == "persistent-pty.added")
        }) {
            // Feed frames are SSE-encoded: `data: {json}\n\n`.
            break serde_json::from_str(payload.trim()).expect("event JSON");
        }
    };

    assert_eq!(event["type"], "persistent-pty.added");
    assert_eq!(event["data"]["sessionID"], session_id, "sessionID at the payload root");
    let terminal = &event["data"]["terminal"];
    assert_eq!(terminal["id"], created.id, "the created terminal id");
    assert_eq!(terminal["title"], "ev1-terminal");
    assert_eq!(terminal["command"], "sh");
    assert_eq!(terminal["sessionID"], session_id, "camelCase sessionID on the terminal");
    assert!(terminal["cwd"].is_string(), "cwd is a string");
    assert_eq!(terminal["status"], "running");
    assert!(terminal["pid"].is_u64(), "pid is a non-negative number: {}", terminal["pid"]);
    assert_eq!(terminal["foregroundProcess"], Value::Null, "foregroundProcess nulls early");
    // size / output are OBJECTS ({cols,rows} / {head,tail}) — never pairs.
    let size = terminal["size"].as_object().unwrap_or_else(|| {
        panic!("size must be an object {{cols,rows}}, got: {}", terminal["size"])
    });
    assert!(size.contains_key("cols") && size.contains_key("rows"), "size keys: {size:?}");
    let output = terminal["output"].as_object().unwrap_or_else(|| {
        panic!("output must be an object {{head,tail}}, got: {}", terminal["output"])
    });
    assert!(output.contains_key("head") && output.contains_key("tail"), "output keys: {output:?}");
    assert_eq!(terminal["output"]["head"], 0, "fresh terminal output window starts at 0");
    // exitCode is optional: absent while running.
    assert!(
        terminal.get("exitCode").is_none(),
        "exitCode must be omitted while running: {terminal}"
    );
    // No snake_case leakage from the internal struct serialization.
    let keys: Vec<&str> = terminal
        .as_object()
        .map(|map| map.keys().map(String::as_str).collect())
        .unwrap_or_default();
    for snake in ["session_id", "foreground_process", "exit_code"] {
        assert!(!keys.contains(&snake), "snake_case key {snake} leaked: {keys:?}");
    }
}
