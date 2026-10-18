//! CRED cluster wiring regressions (ledger CRED-B1/B2/B3): stored
//! credentials must reach the `ModelResolver` and the actual request
//! headers, console-issued providers must authenticate with the Console
//! token, and the keyless zen tier must downgrade to the public key.
//!
//! Hermetic: sandboxed roots, local mock servers on `127.0.0.1:0`, no
//! real network — provider requests are asserted at the route's
//! header-building seam (`Route::prepare_transport`), never sent.

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_core::database::schema::SessionRow;
use oc_core::engine::resolver::CatalogModelResolver;
use oc_core::engine::{Engine, EngineOptions};
use oc_core::integration::credential::CreateInput;
use oc_core::integration::types::{CredentialValue, FormAnswer, OAuthValue};
use oc_core::integration::IntegrationHost;
use oc_core::provider_catalog::catalog::Catalog;
use oc_core::provider_catalog::facade::FacadeContext;
use oc_core::session::runner::ModelResolver as _;
use oc_core::session::runner::ResolvedModel;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// CRED tests mutate PROCESS-GLOBAL environment variables
/// (`OPENCODE_API_KEY`, `ANTHROPIC_API_KEY`); serialize the env-mutating
/// tests on one lock (the register's CD-9 pattern — nextest isolates per
/// process, `cargo test` does not).
static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn env_guard() -> tokio::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK.lock().await
}

/// An integration host over a sandboxed database (no real data dirs).
async fn host() -> (Arc<IntegrationHost>, tempfile::TempDir) {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox tempdir");
    let db = Arc::new(
        oc_core::database::Database::open_file(dir.path().join("t.db")).await.expect("db"),
    );
    let bus = Arc::new(oc_core::bus::Bus::new(
        (*db).clone(),
        oc_core::bus::BusOptions { persist: false, ..oc_core::bus::BusOptions::deterministic() },
    ));
    let host = IntegrationHost::new(db, bus, Catalog::embedded(), "opencode-test");
    (host, dir)
}

fn model_ref(provider: &str, model: &str) -> oc_schema::model::Ref {
    oc_schema::model::Ref {
        provider_id: oc_schema::id::ProviderId::parse(provider.to_owned()),
        id: oc_schema::id::ModelId::parse(model.to_owned()),
        variant: None,
    }
}

fn session_row() -> SessionRow {
    SessionRow::new("ses_cred_wire", "global", "/tmp/opencode", "cred-wire", "test")
        .expect("session row")
}

/// Renders the resolved model's wire request and reads one header — the
/// seam every provider request passes through before the transport.
fn wire_header(
    resolved: &ResolvedModel,
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

// ---------------------------------------------------------------------------
// CRED-B1: the active stored credential reaches the resolver + headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn active_credential_reaches_resolver() {
    let _env = env_guard().await;
    // SAFETY: test-scoped env scrub (the auth chain falls back to env).
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
    let (host, _dir) = host().await;
    // The login method writes the credential store (v2 `connection.key`).
    host.connect_key("anthropic", "sk-ant-from-store", &FormAnswer::new(), None)
        .await
        .expect("connect key");
    // The engine's credential-event refresh (engine/mod.rs boot loop).
    host.refresh_provider_bindings().await;

    let catalog = Arc::new(Catalog::embedded().clone());
    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    )
    .with_credentials(Arc::clone(host.credential_bindings()));

    let session = session_row();
    let resolved = resolver
        .resolve_selected(&session, &model_ref("anthropic", "claude-opus-4-5"))
        .expect("anthropic resolves");
    let header = wire_header(&resolved, "x-api-key")
        .unwrap_or_else(|error| panic!("no stored credential reached the request: {error}"));
    assert_eq!(header.as_deref(), Some("sk-ant-from-store"));
}

#[tokio::test]
async fn credential_switch_invalidates_resolver() {
    let _env = env_guard().await;
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
    let (host, _dir) = host().await;
    let catalog = Arc::new(Catalog::embedded().clone());
    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    )
    .with_credentials(Arc::clone(host.credential_bindings()));
    let session = session_row();
    let reference = model_ref("anthropic", "claude-opus-4-5");

    // First credential activates.
    host.connect_key("anthropic", "sk-ant-first", &FormAnswer::new(), Some("first"))
        .await
        .expect("connect first");
    host.refresh_provider_bindings().await;
    let resolved = resolver.resolve_selected(&session, &reference).expect("resolve first");
    assert_eq!(
        wire_header(&resolved, "x-api-key").expect("wire first").as_deref(),
        Some("sk-ant-first")
    );

    // A second credential SWITCHES the active selection (credential.create
    // auto-activates); the engine's credential.switched refresh must swap
    // the snapshot so the resolver never serves the stale key.
    host.connect_key("anthropic", "sk-ant-second", &FormAnswer::new(), Some("second"))
        .await
        .expect("connect second");
    host.refresh_provider_bindings().await;
    let resolved = resolver.resolve_selected(&session, &reference).expect("resolve second");
    assert_eq!(
        wire_header(&resolved, "x-api-key").expect("wire second").as_deref(),
        Some("sk-ant-second"),
        "credential switch must invalidate the resolver's cached credential"
    );
}

// ---------------------------------------------------------------------------
// CRED-B2: console-issued providers authenticate with the Console token
// ---------------------------------------------------------------------------

/// A one-endpoint local Console: serves `/api/v2/config` on 127.0.0.1:0.
struct MockConsole {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockConsole {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn mock_console(body: Value) -> MockConsole {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let body = body.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let _request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let text = body.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    text.len(),
                    text
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    MockConsole { base: format!("http://127.0.0.1:{port}"), handle }
}

#[tokio::test]
async fn console_provider_gets_token() {
    let mock = mock_console(json!({
        "providers": {
            "acme-ai": {
                "settings": { "baseURL": "https://gw.acme.test/v1" },
                "models": { "acme-large": {} }
            }
        }
    }))
    .await;
    let (host, _dir) = host().await;
    // A logged-in Console session (device OAuth credential, server = mock).
    let value = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "r".into(),
        access: "console-access-token".into(),
        expires: oc_core::database::migrations::now_ms() + 3_600_000,
        metadata: Some(BTreeMap::from([(
            "server".to_owned(),
            Value::String(mock.base.clone()),
        )])),
    });
    host.store()
        .create(CreateInput {
            id: None,
            integration_id: "opencode",
            label: Some("org"),
            value: &value,
            activate: None,
        })
        .await
        .expect("create console credential");
    // The engine's console refresh (boot + credential events).
    host.refresh_console().await;

    let catalog = Arc::new(Catalog::embedded().clone());
    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    )
    .with_console(Arc::clone(host.console_providers()));

    let session = session_row();
    let resolved = resolver
        .resolve_selected(&session, &model_ref("acme-ai", "acme-large"))
        .expect("console provider resolves");
    let header = wire_header(&resolved, "authorization")
        .unwrap_or_else(|error| panic!("console provider request: {error}"));
    assert_eq!(
        header.as_deref(),
        Some("Bearer console-access-token"),
        "console-issued providers must carry the Console OAuth token"
    );
}

// ---------------------------------------------------------------------------
// CRED-B3 (runtime repro): keyless zen downgrades to the public key
// ---------------------------------------------------------------------------

/// Fresh install, no login, no env: the zen default tier must send
/// `Authorization: Bearer public` (v2 opencode.ts:317-325 sets
/// `apiKey: "public"` when no `OPENCODE_API_KEY`, no Console connection,
/// and no configured key exists).
#[tokio::test]
async fn keyless_zen_uses_public_key() {
    let _env = env_guard().await;
    // SAFETY: test-scoped env scrub — the runtime repro's "no env" leg.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("OPENCODE_API_KEY");
    }
    // Sandboxed fresh HOME + data root: no credentials, no logins.
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("fresh home");
    let engine = Engine::boot(EngineOptions::new(
        dir.path().join("data"),
        dir.path().join("home"),
    ))
    .await;

    // A FREE zen model survives the keyless fold (models with a non-zero
    // cost tier are disabled keyless, opencode.ts:327-342).
    let zen = Catalog::embedded().provider("opencode").expect("zen provider");
    let free = zen
        .models
        .iter()
        .find(|model| model.cost.iter().all(|cost| cost.input.0 .0 == 0.0))
        .expect("a free-tier zen model");

    let session = session_row();
    let services = engine
        .location(oc_core::location::LocationRef::new(dir.path().to_owned(), None))
        .await
        .expect("location services");
    let resolved = services
        .resolver
        .resolve_selected(&session, &model_ref("opencode", &free.id.0.clone()))
        .expect("zen model resolves keyless");
    let header = wire_header(&resolved, "authorization")
        .unwrap_or_else(|error| panic!("keyless zen request failed to build: {error}"));
    assert_eq!(
        header.as_deref(),
        Some("Bearer public"),
        "keyless zen must downgrade to the public free-tier key"
    );
}
