//! gb10-lane10 wire regressions:
//!
//! - G-253 — corrupted `session_message.data` (and corrupted session-row
//!   JSON columns) fail the read with a 500 `UnknownError` (ref
//!   history.ts:62-77 `MessageDecodeError` → session-error.ts
//!   `failedMessageDecode`), never a 200 with an `{id,type}`-only empty
//!   shell or a silently-dropped column.
//! - G-254 — `session.move` destination resolution (ref move.ts:75-81):
//!   trim, `~`/`~/x` expansion against the engine home, relative paths
//!   resolved against the SESSION's current directory (not the server
//!   process cwd), and `workspaceID` passthrough into the queued
//!   `MovePayload` (move.ts:84).

use std::sync::Arc;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::Value;

mod support;

struct TestServer {
    client: Client,
    addr: std::net::SocketAddr,
    state: Arc<AppState>,
    workspace: std::path::PathBuf,
    home: std::path::PathBuf,
    _dir: tempfile::TempDir,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    let dir = tempfile::Builder::new()
        .prefix("gb10-lane10-ws-")
        .tempdir_in("/tmp/opencode")
        .expect("workspace");
    let data = tempfile::Builder::new()
        .prefix("gb10-lane10-data-")
        .tempdir_in("/tmp/opencode")
        .expect("data tempdir");
    std::env::set_current_dir(dir.path()).expect("chdir to workspace");
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
        axum::serve(listener, router).await.expect("server");
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer {
        client,
        addr,
        state,
        workspace: dir.path().to_path_buf(),
        home: data.path().join("home"),
        _dir: dir,
        _data: data,
    }
}

async fn create_session(client: &Client) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some("lane10".to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
}

async fn seed_message(server: &TestServer, session_id: &str, id: &str, seq: i64, data: &str) {
    let session = session_id.to_owned();
    let message_id = id.to_owned();
    let payload = data.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, \
                 time_updated, data) VALUES (?1, ?2, 'user', ?3, ?4, ?4, ?5)",
                (message_id, session, seq, 1_700_000_000_000i64, payload),
            )?;
            Ok(())
        })
        .await
        .expect("seed message");
}

// ---------------------------------------------------------------------------
// G-253: decode failures are explicit errors, never silent shells
// ---------------------------------------------------------------------------

#[tokio::test]
async fn g253_corrupt_message_data_returns_500_not_an_empty_shell() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    seed_message(&server, &session_id, "msg_bad", 1, r#"{"role":"user", broken json"#).await;
    seed_message(&server, &session_id, "msg_ok", 2, r#"{"role":"user"}"#).await;

    // GET one corrupted message: 500 UnknownError with the log-ref
    // message (ref failedMessageDecode), NOT a 200 `{id,type}` shell.
    let response = support::request(
        server.addr,
        "GET",
        &format!("/api/session/{session_id}/message/msg_bad"),
        &[],
        b"",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 500, "corrupt data must 500: {}", response.text());
    let envelope = response.json();
    assert_eq!(envelope["message"], "Unexpected server error. Check server logs for details.");
    assert!(envelope["ref"].as_str().is_some_and(|r| r.starts_with("err_")), "{envelope}");

    // The list read fails the same way (any corrupt row poisons the
    // page, mirroring the ref decode pipeline).
    let response = support::request(
        server.addr,
        "GET",
        &format!("/api/session/{session_id}/message"),
        &[],
        b"",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 500, "list with a corrupt row must 500: {}", response.text());

    // Sanity: the healthy row still decodes.
    let response = support::request(
        server.addr,
        "GET",
        &format!("/api/session/{session_id}/message/msg_ok"),
        &[],
        b"",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 200, "healthy row: {}", response.text());
    let body = response.json();
    assert_eq!(body["data"]["id"], "msg_ok");
    assert_eq!(body["data"]["role"], "user");
}

#[tokio::test]
async fn g253_non_object_message_data_is_a_decode_error() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    // JSON-valid but not an object: the ref schema decode fails the row.
    seed_message(&server, &session_id, "msg_scalar", 1, r#""just a string""#).await;
    let response = support::request(
        server.addr,
        "GET",
        &format!("/api/session/{session_id}/message/msg_scalar"),
        &[],
        b"",
    )
    .await
    .expect("raw");
    assert_eq!(response.status, 500, "non-object data must 500: {}", response.text());
}

#[tokio::test]
async fn g253_corrupt_session_column_returns_500_not_silent_drop() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    let id = session_id.clone();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute(
                "UPDATE session_v2 SET model = ?2 WHERE id = ?1",
                [&id, r#"{"providerID":"broken"#],
            )?;
            Ok(())
        })
        .await
        .expect("corrupt the model column");

    let response =
        support::request(server.addr, "GET", &format!("/api/session/{session_id}"), &[], b"")
            .await
            .expect("raw");
    assert_eq!(response.status, 500, "corrupt session column must 500: {}", response.text());
    let envelope = response.json();
    assert_eq!(envelope["message"], "Unexpected server error. Check server logs for details.");
}

// ---------------------------------------------------------------------------
// G-254: move destination resolution + workspaceID passthrough
// ---------------------------------------------------------------------------

async fn post_move(server: &TestServer, session_id: &str, body: &str) -> support::RawResponse {
    support::request(
        server.addr,
        "POST",
        &format!("/api/session/{session_id}/move"),
        &[("content-type", "application/json")],
        body.as_bytes(),
    )
    .await
    .expect("move request")
}

async fn first_move_payload(server: &TestServer, session_id: &str) -> Value {
    let id = session_id.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            let payload: String = tx
                .conn()
                .query_row(
                    "SELECT payload FROM session_inbox WHERE session_id = ?1 AND type = 'move'",
                    [&id],
                    |row| row.get(0),
                )
                .expect("move item");
            Ok(serde_json::from_str(&payload).expect("payload json"))
        })
        .await
        .expect("read move payload")
}

/// `~/x` expands against the engine home (ref move.ts:79-80) and the
/// whitespace is trimmed (:78).
#[tokio::test]
async fn g254_tilde_expansion_and_trim() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    let dest = server.home.join("lane10-tilde-dest");
    std::fs::create_dir_all(&dest).expect("dest dir");

    // Whitespace around a ~ path: trimmed then expanded.
    let response =
        post_move(&server, &session_id, r#"{"directory": "  ~/lane10-tilde-dest  "}"#).await;
    assert_eq!(response.status, 204, "tilde+trim move accepted: {}", response.text());

    let payload = first_move_payload(&server, &session_id).await;
    assert_eq!(
        payload["location"]["directory"].as_str().unwrap(),
        dest.to_string_lossy(),
        "~ expands against the engine home after trimming: {payload}"
    );

    // An unexpanded relative "~nowhere" stays relative → resolved
    // against the session directory → does not exist → 400 with the
    // RESOLVED path in the message (G-233 shape).
    let response = post_move(&server, &session_id, r#"{"directory": "~nowhere"}"#).await;
    assert_eq!(response.status, 400, "unresolvable destination: {}", response.text());
    let envelope = response.json();
    let message = envelope["message"].as_str().unwrap().to_owned();
    assert!(message.starts_with("Directory does not exist: "), "{message}");
    assert!(message.contains("nowhere"), "{message}");
}

/// Relative destinations resolve against the SESSION's current
/// directory — with the process cwd moved elsewhere to prove it is not
/// the resolver base (ref move.ts:81 `path.resolve(session.location.
/// directory, expanded)`).
#[tokio::test]
async fn g254_relative_directory_resolves_against_session_directory() {
    // The process cwd is global state shared with parallel tests in
    // this binary (every boot() chdirs) — hold the guard for the
    // cwd-sensitive section and restore before releasing.
    static CWD_GUARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = CWD_GUARD.lock().await;

    let server = boot().await;
    let session_id = create_session(&server.client).await;
    let workspace = server.workspace.canonicalize().unwrap_or(server.workspace.clone());
    let sub = workspace.join("lane10-sub");
    let sibling = workspace.join("lane10-sibling");
    std::fs::create_dir_all(&sub).expect("sub");
    std::fs::create_dir_all(&sibling).expect("sibling");
    let id = session_id.clone();
    let sub_dir = sub.to_string_lossy().to_string();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute("UPDATE session_v2 SET directory = ?2 WHERE id = ?1", [&id, &sub_dir])?;
            Ok(())
        })
        .await
        .expect("relocate session");

    // Move the PROCESS cwd away: a cwd-based resolution would land in
    // /tmp/opencode (where ../lane10-sibling does not exist → 400).
    std::env::set_current_dir("/tmp/opencode").expect("chdir away");
    let response = post_move(&server, &session_id, r#"{"directory": "../lane10-sibling"}"#).await;
    std::env::set_current_dir(&workspace).expect("restore cwd");

    assert_eq!(
        response.status,
        204,
        "relative move resolves against the session dir: {}",
        response.text()
    );
    let payload = first_move_payload(&server, &session_id).await;
    assert_eq!(
        payload["location"]["directory"].as_str().unwrap(),
        sibling.to_string_lossy(),
        "resolved against session.location.directory: {payload}"
    );
}

/// `workspaceID` passes through into the queued `MovePayload` (ref
/// move.ts:84 `workspaceID: input.workspaceID`).
#[tokio::test]
async fn g254_workspace_id_passthrough() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    let workspace = server.workspace.canonicalize().unwrap_or(server.workspace.clone());
    let body = format!(
        r#"{{"directory": {}, "workspaceID": "wrk_lane10"}}"#,
        serde_json::to_string(&workspace.to_string_lossy().to_string()).unwrap()
    );
    let response = post_move(&server, &session_id, &body).await;
    assert_eq!(response.status, 204, "move with workspaceID: {}", response.text());
    let payload = first_move_payload(&server, &session_id).await;
    assert_eq!(
        payload["location"]["workspaceID"], "wrk_lane10",
        "workspaceID passthrough: {payload}"
    );
}
