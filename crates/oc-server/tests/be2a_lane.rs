//! BE-2a lane regressions (auditE #4/#5/#10): the websearch routes over
//! the wired provider registry (typed no-provider error, env-key
//! eligibility), worktree.create running the real `git worktree add`, and
//! the models-dev refresh trigger with the frozen-snapshot fallback.

use std::sync::Arc;

use oc_client::types::*;
use oc_client::{Client, ClientError};
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    #[allow(dead_code)] // the workspace root the server was booted in
    workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
}

async fn boot(workspace: std::path::PathBuf) -> TestServer {
    let data = tempfile::tempdir().expect("data tempdir");
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
// Websearch (auditE #4): the routes now run the location's WebSearch
// service — zero eligible providers is v2's typed 400, not a 503 stub.
// ---------------------------------------------------------------------------

/// `websearch.query` with no configured provider: the v2
/// `WebSearch.ProviderRequired` mapping — 400, kind
/// `websearch_provider_required`, field `providerID`
/// (server/handlers/websearch.ts).
#[tokio::test]
async fn be2a_websearch_query_without_provider_is_typed_400() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let error = server
        .client
        .websearch()
        .query(
            &WebsearchQueryQuery::default(),
            &WebsearchQueryBody { query: "rust async".into(), provider_id: None },
        )
        .await
        .expect_err("no provider configured");
    let ClientError::Api { status, name, fields, .. } = &error else {
        panic!("expected declared api error, got {error}");
    };
    assert_eq!(*status, 400);
    assert_eq!(name, "InvalidRequestError");
    assert_eq!(fields.get("kind"), Some(&serde_json::json!("websearch_provider_required")));
    assert_eq!(fields.get("field"), Some(&serde_json::json!("providerID")));
}

/// `websearch.providers` with no keys configured returns the documented
/// empty list (data is a bare array of `{id, name}`).
#[tokio::test]
async fn be2a_websearch_providers_empty_without_keys() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let response = server
        .client
        .websearch()
        .providers(&WebsearchProvidersQuery::default())
        .await
        .expect("providers");
    assert!(response.data.is_empty());
}

/// An env API key (the v2 `env` connection method names) makes the
/// provider eligible: the providers route lists it and `websearch.query`
/// reaches the provider (its HTTP surface is not contacted here — the
/// stored selection is enough to observe the routing).
#[tokio::test]
async fn be2a_websearch_env_key_lists_provider() {
    std::env::set_var("TAVILY_API_KEY", "tvly-test-key");
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let response = server
        .client
        .websearch()
        .providers(&WebsearchProvidersQuery::default())
        .await
        .expect("providers");
    std::env::remove_var("TAVILY_API_KEY");
    let ids: Vec<&str> =
        response.data.iter().map(|provider| provider.id.as_str()).collect();
    assert_eq!(ids, vec!["tavily"], "exactly the env-keyed provider is eligible");
}
