//! Round-10 core-gap regressions (lane3): G-107/G-108 (session.move
//! subpath + delivery + orphan fast path), G-111 (pairing browser 401),
//! G-113 (stats unknown timezone).

mod support;

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
    addr: std::net::SocketAddr,
}

async fn boot() -> TestServer {
    let dir = tempfile::tempdir().expect("tempdir");
    let data = tempfile::tempdir().expect("data tempdir");
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
    // Keep the tempdirs alive for the process lifetime (the engine holds
    // open handles into them).
    std::mem::forget(dir);
    std::mem::forget(data);
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, state, addr }
}

async fn create_session(client: &Client, title: &str) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some(title.to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
}

async fn post_move(server: &TestServer, session_id: &str, directory: &str) -> support::RawResponse {
    let body = format!("{{\"directory\": {directory:?}}}");
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

async fn session_row(server: &TestServer, session_id: &str) -> (String, Option<String>) {
    let id = session_id.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            Ok(tx
                .conn()
                .query_row("SELECT directory, path FROM session_v2 WHERE id = ?1", &[&id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .expect("session row"))
        })
        .await
        .expect("read session row")
}

async fn pending_inbox_items(server: &TestServer, session_id: &str) -> Vec<(String, String)> {
    let id = session_id.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            let mut statement = tx
                .conn()
                .prepare("SELECT payload, delivery FROM session_inbox WHERE session_id = ?1")?;
            let rows = statement
                .query_map(&[&id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .expect("read inbox rows")
}

/// G-107 + G-108①: the queued move's subpath is relative to the
/// DESTINATION project root (`""` → absent, never the session's current
/// `row.path`), and the delivery defaults to steer (ref move.ts:89,127).
#[tokio::test]
async fn g107_g108_move_subpath_and_delivery() {
    let server = boot().await;
    let session_id = create_session(&server.client, "mover").await;

    // The session lives in a `sub/` worktree of project A: row.path = "sub"
    // (the OLD bug copied this value into the destination subpath).
    let workspace = std::env::current_dir().expect("workspace");
    let sub = workspace.join("sub");
    std::fs::create_dir_all(&sub).expect("sub dir");
    let id = session_id.clone();
    let sub_dir = sub.to_string_lossy().to_string();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute(
                "UPDATE session_v2 SET directory = ?2, path = ?3 WHERE id = ?1",
                &[&id, &sub_dir, "sub"],
            )?;
            Ok(())
        })
        .await
        .expect("seed subdir session");

    // Destination: project B's root (a fresh checkout root).
    let project_b = tempfile::tempdir_in("/tmp/opencode").expect("project b");
    let b_root = project_b.path().to_string_lossy().to_string();
    std::fs::create_dir_all(project_b.path().join(".git")).expect("git marker");

    let response = post_move(&server, &session_id, &b_root).await;
    assert_eq!(response.status, 204, "move accepted: {}", response.text());

    // The queued item: subpath absent (project B root ⇒ ""), delivery
    // steer even though the request body omitted it.
    let items = pending_inbox_items(&server, &session_id).await;
    assert!(!items.is_empty(), "a move item is queued");
    let (raw_payload, delivery) = &items[0];
    let payload: Value = serde_json::from_str(raw_payload).expect("payload json");
    assert_eq!(delivery, "steer", "G-108① steer default");
    let subpath = payload["subpath"].as_str();
    assert!(
        subpath.is_none_or(str::is_empty),
        "G-107: subpath relative to the destination project root (\"\"), not row.path: {payload}"
    );
}

/// G-108②: an orphan session (inactive, source directory deleted)
/// migrates immediately — the move completes without queueing an item
/// the dead location could never drain (ref move.ts:104-116,134-147).
#[tokio::test]
async fn g108_orphan_session_moves_immediately() {
    let server = boot().await;
    let session_id = create_session(&server.client, "orphan").await;

    // The session's source directory is gone.
    let workspace = std::env::current_dir().expect("workspace");
    let gone = workspace.join("gone");
    let id = session_id.clone();
    let gone_dir = gone.to_string_lossy().to_string();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute(
                "UPDATE session_v2 SET directory = ?2, path = ?3 WHERE id = ?1",
                &[&id, &gone_dir, "gone"],
            )?;
            Ok(())
        })
        .await
        .expect("seed orphan session");

    let project_b = tempfile::tempdir_in("/tmp/opencode").expect("project b");
    let b_root = project_b.path().to_string_lossy().to_string();
    std::fs::create_dir_all(project_b.path().join(".git")).expect("git marker");

    let response = post_move(&server, &session_id, &b_root).await;
    assert_eq!(response.status, 204, "orphan move accepted: {}", response.text());

    // The session row landed at the destination immediately.
    let (directory, path) = session_row(&server, &session_id).await;
    assert_eq!(directory, b_root, "session.moved relocated the orphan");
    assert!(path.as_ref().is_none_or(|value| value.is_empty()), "root subpath: {path:?}");

    // Nothing was queued.
    let items = pending_inbox_items(&server, &session_id).await;
    assert!(items.is_empty(), "no queued move survives the orphan fast path: {items:?}");
}

/// G-111: the browser-branch pairing 401 carries NO WWW-Authenticate
/// challenge (ref handlers/server.ts:39-42) — a challenge triggers the
/// native browser auth popup.
#[tokio::test]
async fn g111_pairing_browser_401_has_no_challenge() {
    let server = boot().await;
    let response = support::request(
        server.addr,
        "GET",
        "/auth/connect/bogus-code",
        &[("accept", "text/html")],
        &[],
    )
    .await
    .expect("connect request");
    assert_eq!(response.status, 401);
    assert!(
        response.header("www-authenticate").is_none(),
        "G-111: no WWW-Authenticate on the browser branch"
    );
    assert_eq!(response.header("content-type"), Some("text/plain"));
}

/// G-113: an unknown IANA zone is a 400 "Invalid time zone" (the ref
/// constructs an Intl.DateTimeFormat and lets the RangeError surface —
/// handlers/session.ts:109-111), never a silent UTC fallback.
#[tokio::test]
async fn g113_stats_unknown_timezone_is_400() {
    let server = boot().await;
    let response = server.raw_get("/api/experimental/session/stats?timezone=Invalid%2FZone").await;
    assert_eq!(response.status, 400, "unknown timezone rejected: {}", response.text());
    let body: Value = serde_json::from_slice(&response.body).expect("json error body");
    assert_eq!(body["_tag"], "InvalidRequestError", "{body}");
    assert!(
        body["message"].as_str().is_some_and(|message| message.contains("Invalid time zone")),
        "ref-style message: {body}"
    );
}

impl TestServer {
    async fn raw_get(&self, target: &str) -> support::RawResponse {
        support::request(self.addr, "GET", target, &[], &[]).await.expect("raw request")
    }
}
