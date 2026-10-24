//! G-17: `session.prompt` delivery validation — absent ⇒ steer,
//! `"steer"`/`"queue"` accepted, anything else (unknown string or
//! non-string) is a 400 `InvalidRequestError` with kind `ParseError`
//! (ref session-inbox.ts:11 allows only `steer | queue`; the generated
//! client cannot express the invalid payloads, so those legs go over
//! raw HTTP).

use std::sync::Arc;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

mod support;

struct TestServer {
    client: Client,
    addr: std::net::SocketAddr,
    _workspace: tempfile::TempDir,
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
    TestServer { client, addr, _workspace: dir, _data: data }
}

async fn create_session(client: &Client) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some("delivery".to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
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

#[tokio::test]
async fn prompt_delivery_defaults_and_valid_values() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client).await;

    let body = |delivery: Option<SessionInboxDelivery>| SessionPromptBody {
        text: "hi".to_owned(),
        resume: Some(false),
        delivery,
        ..Default::default()
    };

    // Absent ⇒ steer (the ref schema default).
    let inbox_user = client.session().prompt(&session_id, &body(None)).await.expect("absent");
    assert_eq!(inbox_user.delivery, SessionInboxDelivery::Steer);

    // Explicit "steer" ⇒ steer.
    let inbox_user = client
        .session()
        .prompt(&session_id, &body(Some(SessionInboxDelivery::Steer)))
        .await
        .expect("steer");
    assert_eq!(inbox_user.delivery, SessionInboxDelivery::Steer);

    // Explicit "queue" ⇒ queue.
    let inbox_user = client
        .session()
        .prompt(&session_id, &body(Some(SessionInboxDelivery::Queue)))
        .await
        .expect("queue");
    assert_eq!(inbox_user.delivery, SessionInboxDelivery::Queue);
}

#[tokio::test]
async fn prompt_delivery_rejects_unknown_and_non_string_values() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client).await;

    for delivery in [serde_json::json!("garbage"), serde_json::json!(5)] {
        let response = prompt_raw(
            server.addr,
            &session_id,
            &serde_json::json!({ "text": "hi", "resume": false, "delivery": delivery }),
        )
        .await;
        assert_eq!(response.status, 400, "delivery {delivery} must 400");
        let body = response.json();
        assert_eq!(body["_tag"], "InvalidRequestError", "body: {body}");
        assert_eq!(body["kind"], "ParseError", "body: {body}");
        assert_eq!(body["message"], "Expected \"steer\" or \"queue\" for delivery");
    }
}
