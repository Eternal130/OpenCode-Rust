//! AUD-09: `session.prompt` hook timing — the ref fires the hook at
//! SUBMIT time, before the inbox row exists, so a failure rejects the
//! HTTP request (ref core/src/session/prompt.ts:29-52 via
//! session/session.ts:155-172: prepare runs ahead of
//! `admission.admit`). The Rust handler now dispatches the seam on the
//! admission path (`PromptPrepare::prepare_input` via
//! `services.runner`): a failing hook ⇒ 500 (ref leaves the hook error
//! uncaught in the handler), an unknown skill ⇒ 400 `field: "skills"`
//! (ref handlers/session.ts:334-337), and NO inbox row is admitted in
//! either case (nothing to poison/cancel). The drain-side
//! `prepare_pending` remains as the backstop for engine-internal
//! admits (covered by oc-core's prompt_prepare_hooks battery).
//!
//! Hermetic: sandbox tempdirs, no network beyond 127.0.0.1:0, the
//! prompt submits use `resume: false` so no model turn is needed.

use std::sync::Arc;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_core::hooks::{fn_hook, HookError, HookOptions, SessionPromptEvent};
use oc_core::location::LocationRef;
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

mod support;

struct TestServer {
    client: Client,
    addr: std::net::SocketAddr,
    state: Arc<AppState>,
    workspace: tempfile::TempDir,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    let dir = tempfile::tempdir().expect("workspace");
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
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, addr, state, workspace: dir, _data: data }
}

async fn create_session(client: &Client) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some("aud9".to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
}

/// The session's location services (the same registry the production
/// admission-path dispatch consults) — resolved through the session
/// ROW's directory like the server's session-location middleware, not
/// the caller's cwd spelling.
async fn location_services(server: &TestServer, session_id: &str) -> Arc<oc_core::engine::LocationServices> {
    let store = oc_core::session::store::SessionStore::new((*server.state.engine.db).clone());
    let row = store.get(session_id).await.expect("session row").expect("row exists");
    let location =
        LocationRef::new(std::path::PathBuf::from(&row.directory), row.workspace_id.clone());
    server.state.engine.location(location).await.expect("location services")
}

async fn prompt_raw(
    addr: std::net::SocketAddr,
    session_id: &str,
    body: &serde_json::Value,
) -> support::RawResponse {
    support::request(
        addr,
        "POST",
        &format!("/api/session/{session_id}/prompt"),
        &[("content-type", "application/json")],
        serde_json::to_string(body).expect("serialize").as_bytes(),
    )
    .await
    .expect("raw prompt")
}

// A failing `session.prompt` hook rejects the HTTP request (500 — ref
// leaves the hook failure uncaught in handlers/session.ts:308-341) and
// admits NO inbox row (nothing to poison or cancel downstream).
#[tokio::test]
async fn failing_hook_rejects_submit_without_admitting_a_row() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    let services = location_services(&server, &session_id).await;
    let _registration = services
        .graph
        .hooks
        .register_session_prompt(
            fn_hook(|_event: &mut SessionPromptEvent| {
                Err(HookError::Handler("prompt rejected by policy".into()))
            }),
            HookOptions::default(),
        )
        .await;

    let response = prompt_raw(
        server.addr,
        &session_id,
        &serde_json::json!({ "text": "hi", "resume": false }),
    )
    .await;
    assert_eq!(response.status, 500, "hook failure must reject the submit");
    assert_eq!(response.json()["_tag"], "UnknownError", "body: {}", response.text());

    let inbox = services.inbox.list(&session_id).await.expect("inbox list");
    assert!(inbox.is_empty(), "no inbox row may be admitted for a rejected prompt");
}

// A shaping hook's copy-out lands on the ADMITTED row (ref prompt.ts:52
// — v2 admits `event.prompt`'s fields), observable in the prompt
// response payload.
#[tokio::test]
async fn shaping_hook_output_is_admitted() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    let services = location_services(&server, &session_id).await;
    let _registration = services
        .graph
        .hooks
        .register_session_prompt(
            fn_hook(|event: &mut SessionPromptEvent| {
                event.prompt.text = "shaped-by-hook".into();
                event.delivery = oc_schema::session_inbox::Delivery::Queue;
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;

    let response = prompt_raw(
        server.addr,
        &session_id,
        &serde_json::json!({ "text": "original", "resume": false }),
    )
    .await;
    assert_eq!(response.status, 200, "body: {}", response.text());
    let data = response.json()["data"].clone();
    let payload = data["payload"].clone();
    assert_eq!(payload["text"], "shaped-by-hook", "admitted row carries the hook's prompt");
    assert_eq!(data["delivery"], "queue", "admitted row carries the hook's delivery");

    let inbox = services.inbox.list(&session_id).await.expect("inbox list");
    assert_eq!(inbox.len(), 1, "exactly one admitted row");
}

// An unknown skill fails the submit at admission time with ref's 400
// (`field: "skills"`, handlers/session.ts:334-337; SkillNotFoundError
// at prompt.ts:66) — no admitted row.
#[tokio::test]
async fn unknown_skill_rejects_submit_with_400() {
    let server = boot().await;
    let session_id = create_session(&server.client).await;
    let services = location_services(&server, &session_id).await;

    let response = prompt_raw(
        server.addr,
        &session_id,
        &serde_json::json!({
            "text": "hi",
            "resume": false,
            "skills": [ { "id": "no-such-skill", "name": "no-such-skill" } ]
        }),
    )
    .await;
    assert_eq!(response.status, 400, "body: {}", response.text());
    let body = response.json();
    assert_eq!(body["_tag"], "InvalidRequestError");
    assert_eq!(body["field"], "skills", "body: {body}");
    assert!(body["message"].as_str().unwrap_or("").contains("no-such-skill"));

    let inbox = services.inbox.list(&session_id).await.expect("inbox list");
    assert!(inbox.is_empty(), "no inbox row may be admitted for a rejected prompt");
}

