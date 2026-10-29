//! AUD-03 (HTTP FormLocation "global" escape hatch): the session-scoped
//! form routes resolve `sessionID === "global"` straight to the request's
//! location (ref server/src/middleware/form-location.ts:27-38) instead of
//! the session table, so MCP elicitation forms stored under the sentinel
//! session id can be listed and answered; every other nonexistent session
//! id still 404s.

use std::sync::Arc;

use oc_client::types::*;
use oc_client::{Client, ClientError};
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

async fn boot() -> (Client, Arc<AppState>, tempfile::TempDir) {
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    let data = tempfile::tempdir().expect("data tempdir");
    std::env::set_current_dir(workspace.path()).expect("chdir to workspace");
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
    (client, state, workspace)
}

fn string_field(key: &str) -> FormField {
    let raw = serde_json::json!({ "type": "string", "key": key });
    serde_json::from_value(raw).expect("string field")
}

/// A global elicitation form can be created, listed, fetched and replied
/// through the session-scoped form endpoints — the "global" sentinel
/// bypasses the session row lookup but stays location-scoped.
#[tokio::test]
async fn aud3_global_form_roundtrip_through_session_endpoints() {
    let (client, _state, _workspace) = boot().await;

    let created = client
        .session()
        .form()
        .create(
            oc_core::mcp::GLOBAL_ELICITATION_SESSION_ID,
            &FormCreatePayload {
                fields: vec![string_field("elicitation")],
                id: Some("frm_global_aud3".into()),
                metadata: None,
                title: "global elicitation".into(),
            },
        )
        .await
        .expect("global form create");
    assert_eq!(created.session_id, "global");
    assert_eq!(created.id, "frm_global_aud3");

    let listed = client
        .session()
        .form()
        .list(oc_core::mcp::GLOBAL_ELICITATION_SESSION_ID)
        .await
        .expect("global form list");
    assert!(
        listed.iter().any(|form| form.id == "frm_global_aud3"),
        "the global form is listed: {listed:?}"
    );

    let detail = client
        .session()
        .form()
        .get(oc_core::mcp::GLOBAL_ELICITATION_SESSION_ID, "frm_global_aud3")
        .await
        .expect("global form get");
    assert_eq!(detail.session_id, "global");

    let mut answer = std::collections::BTreeMap::new();
    answer.insert("elicitation".to_owned(), FormValue::String("yes".into()));
    client
        .session()
        .form()
        .reply(
            oc_core::mcp::GLOBAL_ELICITATION_SESSION_ID,
            "frm_global_aud3",
            &FormReply { answer },
        )
        .await
        .expect("global form reply");
}

/// The escape hatch is exactly `"global"`: a well-formed but missing
/// session id (`ses_…`) still resolves through the session table and
/// 404s. A malformed id ("global-ish", no `ses` prefix) is 400 per ref
/// (form-location.ts:30-38 falls through to sessionInfo, whose
/// `Session.ID` decode fails → `InvalidRequestError`,
/// server/src/location.ts:49-53) — AUD-18 aligned the port.
#[tokio::test]
async fn aud3_nonexistent_session_form_still_404s() {
    let (client, _state, _workspace) = boot().await;
    let error = client
        .session()
        .form()
        .list("ses_nonexistent_aud3")
        .await
        .expect_err("nonexistent session form list must 404");
    let ClientError::Api { status, .. } = &error else { panic!("expected api error, got {error}") };
    assert_eq!(*status, 404);

    let error = client
        .session()
        .form()
        .list("global-ish")
        .await
        .expect_err("near-miss sentinel must 400 (malformed session id)");
    let ClientError::Api { status, .. } = &error else { panic!("expected api error, got {error}") };
    assert_eq!(*status, 400);
}
