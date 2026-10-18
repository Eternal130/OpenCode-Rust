//! CRED cluster wire-level regressions over the real server surface:
//! a stored credential (auth login) must reach the resolved request's
//! headers, `/api/model` availability must read the credential store
//! (not env alone), and the no-credential path must produce the typed
//! authentication error (the pre-send shape of the provider 401).
//!
//! Hermetic: sandboxed roots, in-process server on `127.0.0.1:0`, no
//! real network — provider requests are asserted at the route's
//! header-building seam, never sent.

use std::sync::Arc;
use std::time::Duration;

use oc_client::types::*;
use oc_client::Client;
use oc_core::database::schema::SessionRow;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

/// These tests scrub process-global credential env vars; serialize them
/// (the register's CD-9 pattern — nextest isolates per process anyway).
static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn env_guard() -> tokio::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK.lock().await
}

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    _workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    let data = tempfile::tempdir_in("/tmp/opencode").expect("data tempdir");
    let workspace = data.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::env::set_current_dir(&workspace).expect("chdir to workspace");
    let engine =
        Engine::boot(EngineOptions::new(data.path().join("data"), data.path().join("home")))
            .await;
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
    TestServer { client, state, _workspace: workspace, _data: data }
}

fn cwd_location() -> Option<QueryLocation> {
    Some(QueryLocation {
        directory: Some(
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        ),
    })
}

fn session_row() -> SessionRow {
    SessionRow::new("ses_cred_req", "global", "/tmp/opencode", "cred-req", "test")
        .expect("session row")
}

fn model_ref(provider: &str, model: &str) -> oc_schema::model::Ref {
    oc_schema::model::Ref {
        provider_id: oc_schema::id::ProviderId::parse(provider.to_owned()),
        id: oc_schema::id::ModelId::parse(model.to_owned()),
        variant: None,
    }
}

fn wire_header(
    resolved: &oc_core::session::runner::ResolvedModel,
    name: &str,
) -> Result<Option<String>, String> {
    let route = Arc::clone(&resolved.model.route);
    let mut request = oc_ai::schema::LLMRequest::new(resolved.model.clone());
    request.messages = vec![oc_ai::schema::Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).map_err(|error| error.to_string())?;
    let (transport_request, _) =
        route.prepare_transport(&prepared, &body).map_err(|error| error.to_string())?;
    Ok(transport_request.header(name).map(str::to_owned))
}

/// Waits for the engine's credential-event refresh loop (boot-spawned
/// subscription over `credential.switched`/`credential.updated`) to swap
/// the shared binding snapshot — the production cache-invalidation path.
async fn wait_for_binding(server: &TestServer, provider: &str, key: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if server
            .state
            .engine
            .integration
            .credential_bindings()
            .get(provider)
            .is_some_and(|binding| binding.api_key == key)
        {
            return;
        }
        assert!(
            deadline > tokio::time::Instant::now(),
            "credential binding for {provider} never refreshed to {key:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------
// CRED-B1: auth login → credential store → resolver → request headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stored_credential_reaches_request_headers() {
    let _env = env_guard().await;
    // SAFETY: test-scoped env scrub (availability/auth fall back to env).
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
    let server = boot().await;
    // The real login surface writes the credential store.
    server
        .client
        .integration()
        .connect()
        .key(
            "anthropic",
            &IntegrationConnectKeyQuery { location: cwd_location() },
            &IntegrationConnectKeyBody {
                key: "sk-ant-wire".into(),
                answer: None,
                label: Some("work key".into()),
            },
        )
        .await
        .expect("connect key");
    // The engine refresh loop picks the credential.switched event up.
    wait_for_binding(&server, "anthropic", "sk-ant-wire").await;

    let session = session_row();
    let services = server
        .state
        .engine
        .location(oc_core::location::LocationRef::new(
            std::env::current_dir().expect("cwd"),
            None,
        ))
        .await
        .expect("location services");
    let resolved = services
        .resolver
        .resolve_selected(&session, &model_ref("anthropic", "claude-opus-4-5"))
        .expect("anthropic resolves");
    let header = wire_header(&resolved, "x-api-key")
        .unwrap_or_else(|error| panic!("stored credential never reached the request: {error}"));
    assert_eq!(header.as_deref(), Some("sk-ant-wire"));
}

// ---------------------------------------------------------------------------
// CRED-B1: `/api/model` availability reads credentials, not env alone
// ---------------------------------------------------------------------------

#[tokio::test]
async fn model_availability_matches_credentials() {
    let _env = env_guard().await;
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
    let server = boot().await;
    let query = ModelListQuery { location: cwd_location() };

    // No credential, no env: the anthropic catalog is unavailable.
    let before = server.client.model().list(&query).await.expect("list before");
    assert!(
        !before.data.iter().any(|model| model.provider_id == "anthropic"),
        "anthropic must be unavailable without a credential or env key"
    );

    server
        .client
        .integration()
        .connect()
        .key(
            "anthropic",
            &IntegrationConnectKeyQuery { location: cwd_location() },
            &IntegrationConnectKeyBody { key: "sk-ant-avail".into(), answer: None, label: None },
        )
        .await
        .expect("connect key");

    // A stored credential is an integration connection (v2 provider fold,
    // provider.ts:393-406): availability flips without any env change.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let after = server.client.model().list(&query).await.expect("list after");
        if after.data.iter().any(|model| model.provider_id == "anthropic") {
            break;
        }
        assert!(
            deadline > tokio::time::Instant::now(),
            "/api/model availability never picked up the stored credential"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---------------------------------------------------------------------------
// CRED-B1 error path: no credential → the typed authentication failure
// (the pre-send shape of the provider 401)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_credential_error_shape() {
    let _env = env_guard().await;
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
    let server = boot().await;
    let session = session_row();
    let services = server
        .state
        .engine
        .location(oc_core::location::LocationRef::new(
            std::env::current_dir().expect("cwd"),
            None,
        ))
        .await
        .expect("location services");
    let resolved = services
        .resolver
        .resolve_selected(&session, &model_ref("anthropic", "claude-opus-4-5"))
        .expect("the model still resolves (credential applies at request time)");
    let error = wire_header(&resolved, "x-api-key")
        .expect_err("the wire request must fail without any credential");
    assert!(
        error.contains("ANTHROPIC_API_KEY"),
        "the failure must name the missing credential source: {error}"
    );
}
