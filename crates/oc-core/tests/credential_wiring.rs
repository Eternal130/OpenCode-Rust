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
fn wire_header(resolved: &ResolvedModel, name: &str) -> Result<Option<String>, String> {
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
    let bound = resolver
        .resolve_selected(&session, &model_ref("anthropic", "claude-opus-4-5"))
        .expect("anthropic resolves");
    let header = wire_header(&bound, "x-api-key")
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
    let bound = resolver.resolve_selected(&session, &reference).expect("resolve first");
    assert_eq!(
        wire_header(&bound, "x-api-key").expect("wire first").as_deref(),
        Some("sk-ant-first")
    );

    // A second credential SWITCHES the active selection (credential.create
    // auto-activates); the engine's credential.switched refresh must swap
    // the snapshot so the resolver never serves the stale key.
    host.connect_key("anthropic", "sk-ant-second", &FormAnswer::new(), Some("second"))
        .await
        .expect("connect second");
    host.refresh_provider_bindings().await;
    let bound = resolver.resolve_selected(&session, &reference).expect("resolve second");
    assert_eq!(
        wire_header(&bound, "x-api-key").expect("wire second").as_deref(),
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
        metadata: Some(BTreeMap::from([("server".to_owned(), Value::String(mock.base.clone()))])),
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
    let bound = resolver
        .resolve_selected(&session, &model_ref("acme-ai", "acme-large"))
        .expect("console provider resolves");
    let header = wire_header(&bound, "authorization")
        .unwrap_or_else(|error| panic!("console provider request: {error}"));
    assert_eq!(
        header.as_deref(),
        Some("Bearer console-access-token"),
        "console-issued providers must carry the Console OAuth token"
    );
}

// ---------------------------------------------------------------------------
// CRED-CONSOLE-KEYAUTH: Method::Key console credentials load providers
// ---------------------------------------------------------------------------

/// A capturing console mock: records one request's request-line + headers
/// (the `authorization` / `x-org-id` assertions of the key path).
async fn mock_console_capture(
    body: Value,
    seen: tokio::sync::mpsc::UnboundedSender<String>,
) -> MockConsole {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let seen = seen.clone();
            let body = body.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let authorization = request.lines().find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                        .map(|(_, value)| value.trim().to_owned())
                });
                let org = request.lines().find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("x-org-id"))
                        .map(|(_, value)| value.trim().to_owned())
                });
                if let Some(authorization) = authorization {
                    let _ = seen.send(format!("{authorization}|{}", org.unwrap_or_default()));
                }
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

/// CRED-CONSOLE-KEYAUTH (ref provider/opencode.ts:535-541
/// `credentialHeaders`: `Bearer oauth.access OR key`): a service-account
/// API-key credential on the `opencode` integration must resolve the
/// console connection, fetch `/api/v2/config` with the key bearer +
/// `x-org-id`, and key the console-issued providers with the same token.
#[tokio::test]
async fn console_key_credential_loads_providers() {
    let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
    let mock = mock_console_capture(
        json!({
            "providers": {
                "acme-key": {
                    "settings": { "baseURL": "https://gw.acme.test/v1" },
                    "models": { "acme-key-large": {} }
                }
            }
        }),
        seen_tx,
    )
    .await;
    let (host, _dir) = host().await;
    // A service-account key credential (Method::Key path), server = mock.
    let value = CredentialValue::Key(oc_core::integration::types::KeyValue {
        key: "sa-console-key".into(),
        metadata: Some(BTreeMap::from([
            ("server".to_owned(), Value::String(mock.base.clone())),
            ("orgID".to_owned(), Value::String("org_9".into())),
        ])),
        configuration: None,
    });
    host.store()
        .create(CreateInput {
            id: None,
            integration_id: "opencode",
            label: Some("service account"),
            value: &value,
            activate: None,
        })
        .await
        .expect("create key console credential");
    host.refresh_console().await;

    // The console fetch ran with the KEY credential's headers.
    let seen = tokio::time::timeout(std::time::Duration::from_secs(5), seen_rx.recv())
        .await
        .expect("timeout waiting for console fetch")
        .expect("captured request");
    assert_eq!(seen, "Bearer sa-console-key|org_9", "key credentials key the console fetch");

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
    let bound = resolver
        .resolve_selected(&session, &model_ref("acme-key", "acme-key-large"))
        .expect("console provider loads for key credentials");
    let header = wire_header(&bound, "authorization")
        .unwrap_or_else(|error| panic!("console key provider request: {error}"));
    assert_eq!(
        header.as_deref(),
        Some("Bearer sa-console-key"),
        "console-issued providers must carry the service-account key token"
    );
}

// ---------------------------------------------------------------------------
// CRED-AUTHNONE-DOWNGRADE: activation-enabled keyless providers resolve
// with Auth.none (generic, not zen-only)
// ---------------------------------------------------------------------------

/// A synthetic activation-ENABLED keyless provider (no catalog provider
/// exercises this today): with no credential, no env, and no configured
/// auth, resolution must build an anonymous request (ref
/// model-resolver.ts:379-385) instead of failing on the missing env key.
#[tokio::test]
async fn generic_keyless_downgrades_to_auth_none() {
    let _env = env_guard().await;
    // SAFETY: test-scoped env scrub — the keyless leg.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("KEYLESS_API_KEY");
    }
    let body = r#"{
        "keyless": {
            "id": "keyless",
            "name": "Keyless Gateway",
            "env": ["KEYLESS_API_KEY"],
            "npm": "@ai-sdk/groq",
            "api": "https://gw.keyless.test/v1",
            "models": {
                "keyless-1": {
                    "id": "keyless-1",
                    "name": "Keyless One",
                    "release_date": "2026-01-01",
                    "attachment": false,
                    "reasoning": false,
                    "tool_call": true,
                    "limit": {"context": 8192, "output": 1024}
                }
            }
        }
    }"#;
    let catalog = Arc::new(
        Catalog::parse(body)
            .expect("synthetic catalog parses")
            .with_activation("keyless", oc_schema::provider::Activation::Enabled),
    );
    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    );

    let session = session_row();
    let bound = resolver
        .resolve_selected(&session, &model_ref("keyless", "keyless-1"))
        .expect("activation-enabled keyless provider resolves");
    let header = wire_header(&bound, "authorization")
        .unwrap_or_else(|error| panic!("keyless request must build anonymously, got: {error}"));
    assert_eq!(
        header, None,
        "no credential, no env, no configured auth → Auth.none (no authorization header)"
    );
}

/// CRED-AUTHNONE-DOWNGRADE v2 delta (ref model-resolver.ts:456): the v2
/// `usesAPIKeyAuth` list includes the `@opencode/ai/providers/*` npm names
/// (fireworks, openai-compatible, google at model-resolver.ts:455-457) —
/// v2-era models.dev ships these, so an A1 refresh can land one live. An
/// activation-enabled keyless provider under one of those npm names must
/// downgrade to Auth.none exactly like the generic keyless leg above.
#[tokio::test]
async fn v2_npm_openai_compatible_keyless_downgrades_to_auth_none() {
    let _env = env_guard().await;
    // SAFETY: test-scoped env scrub — the keyless leg.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("V2_KEYLESS_API_KEY");
    }
    let body = r#"{
        "v2keyless": {
            "id": "v2keyless",
            "name": "V2 Keyless Gateway",
            "env": ["V2_KEYLESS_API_KEY"],
            "npm": "@opencode/ai/providers/openai-compatible",
            "api": "https://gw.v2keyless.test/v1",
            "models": {
                "v2keyless-1": {
                    "id": "v2keyless-1",
                    "name": "V2 Keyless One",
                    "release_date": "2026-01-01",
                    "attachment": false,
                    "reasoning": false,
                    "tool_call": true,
                    "limit": {"context": 8192, "output": 1024}
                }
            }
        }
    }"#;
    let catalog = Arc::new(
        Catalog::parse(body)
            .expect("synthetic catalog parses")
            .with_activation("v2keyless", oc_schema::provider::Activation::Enabled),
    );
    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    );

    let session = session_row();
    let bound = resolver
        .resolve_selected(&session, &model_ref("v2keyless", "v2keyless-1"))
        .expect("activation-enabled v2-npm keyless provider resolves");
    let header = wire_header(&bound, "authorization")
        .unwrap_or_else(|error| panic!("v2 keyless request must build anonymously, got: {error}"));
    assert_eq!(
        header, None,
        "v2 npm name + no credential, no env, no configured auth → Auth.none (no authorization header)"
    );
}

// ---------------------------------------------------------------------------
// CRED-A1: catalog refresh propagates to resolver + integrations
// ---------------------------------------------------------------------------

/// One `api.json` server on 127.0.0.1:0 (the refresh loop's source).
async fn mock_catalog_api(body: String) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let body = body.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                let _ = socket.read(&mut buffer).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    (format!("http://{addr}"), handle)
}

fn tiny_catalog_body() -> String {
    serde_json::json!({
        "tiny": {
            "id": "tiny",
            "name": "Tiny",
            "env": ["TINY_API_KEY"],
            "npm": "@ai-sdk/openai-compatible",
            "api": "https://api.tiny.test/v1",
            "models": {
                "tiny-1": {
                    "id": "tiny-1",
                    "name": "Tiny One",
                    "release_date": "2026-01-01",
                    "attachment": false,
                    "reasoning": false,
                    "tool_call": true,
                    "limit": {"context": 8192, "output": 1024}
                }
            }
        }
    })
    .to_string()
}

/// CRED-A1 (ref models-dev.ts:61-66: on `models-dev.refreshed` the
/// integration + provider registries re-read the live catalog): after
/// the boot refresh loop swaps the served catalog, the engine resolver
/// binds the fetched provider and the integration registry carries its
/// env method — no re-boot.
#[tokio::test]
async fn catalog_refresh_propagates() {
    let (url, server) = mock_catalog_api(tiny_catalog_body()).await;
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox home");
    let engine = Engine::boot(EngineOptions {
        catalog_fetch: true,
        catalog_url: Some(url),
        ..EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;

    // Wait for the refresh loop to land the fetched catalog.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while engine.catalog_service.get().provider_count() != 1 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "refresh loop never applied the fetched catalog"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    // CRED-A1: the resolver's shared catalog cell follows the refresh —
    // `tiny/tiny-1` (unknown to the boot snapshot) binds.
    let session = session_row();
    let services = engine
        .location(oc_core::location::LocationRef::new(dir.path().to_owned(), None))
        .await
        .expect("location services");
    let reference = model_ref("tiny", "tiny-1");
    let bound = loop {
        match services.resolver.resolve_selected(&session, &reference) {
            Ok(bound) => break bound,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(error) => panic!("refreshed provider never reached the resolver: {error}"),
        }
    };
    assert_eq!(bound.model.provider.as_str(), "tiny");
    assert_eq!(
        bound.model.route.endpoint.base_url.as_deref(),
        Some("https://api.tiny.test/v1"),
        "the FETCHED catalog binds (not the boot snapshot's provider set)"
    );

    // The integration registry re-installed the fetched provider's
    // key/env methods (v2 ctx.integration.reload()).
    let integration = engine.integration.get("tiny").await.expect("get").expect("integration row");
    let env_names = integration
        .methods
        .iter()
        .filter_map(|method| match method {
            oc_core::integration::types::Method::Env { names } => Some(names.clone()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(env_names, vec!["TINY_API_KEY"], "env method follows the refreshed catalog");
    server.abort();
}

// ---------------------------------------------------------------------------
// CRED-A3: variant overlays merge into the resolved request
// ---------------------------------------------------------------------------

/// A deepseek-family catalog model with a toggle reasoning lane: the
/// `thinking`/`none` variants carry BODY overlays (`thinking.type`),
/// which must reach the wire body of the resolved request (v2
/// withVariant, model-resolver.ts:137-161 + 196-257).
#[tokio::test]
async fn variant_overlay_request() {
    let body = r#"{
        "deepseek": {
            "id": "deepseek",
            "name": "Deep Variant",
            "npm": "@ai-sdk/openai-compatible",
            "models": {
                "dv-1": {
                    "id": "dv-1",
                    "name": "DV One",
                    "release_date": "2026-01-01",
                    "attachment": false,
                    "reasoning": true,
                    "reasoning_options": [{"type": "toggle"}],
                    "tool_call": true,
                    "limit": {"context": 8192, "output": 1024}
                }
            }
        }
    }"#;
    let catalog = Arc::new(Catalog::parse(body).expect("catalog parses"));
    let model = catalog.model("deepseek", "dv-1").expect("model");
    let variants: Vec<String> = model.variants.iter().map(|v| v.id.0.clone()).collect();
    assert!(
        variants.iter().any(|id| id == "thinking") && variants.iter().any(|id| id == "none"),
        "toggle variants resolve: {variants:?}"
    );

    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::from([(
            "deepseek".to_owned(),
            oc_core::provider_catalog::facade::ModelConnection {
                api_key: Some("dv-key".to_owned()),
                ..Default::default()
            },
        )]),
        Vec::new(),
        None,
    );
    let session = session_row();

    let mut reference = model_ref("deepseek", "dv-1");
    reference.variant = Some(oc_schema::id::ModelVariantId::parse("thinking".to_owned()));
    let bound = resolver.resolve_selected(&session, &reference).expect("variant resolves");
    let wire = wire_body(&bound);
    assert_eq!(
        wire.get("thinking").and_then(|value| value.get("type")),
        Some(&serde_json::json!("enabled")),
        "the `thinking` variant's body overlay reaches the request body"
    );

    reference.variant = Some(oc_schema::id::ModelVariantId::parse("none".to_owned()));
    let bound = resolver.resolve_selected(&session, &reference).expect("none variant resolves");
    let wire = wire_body(&bound);
    assert_eq!(
        wire.get("thinking").and_then(|value| value.get("type")),
        Some(&serde_json::json!("disabled")),
        "the `none` variant's body overlay reaches the request body"
    );

    reference.variant = Some(oc_schema::id::ModelVariantId::parse("bogus".to_owned()));
    let error = resolver.resolve_selected(&session, &reference).expect_err("unknown variant fails");
    assert!(
        error.to_string().contains("variant `bogus`"),
        "variant-unavailable error names the variant: {error}"
    );
}

/// Renders the resolved model's request body (the wire-shape seam).
fn wire_body(resolved: &ResolvedModel) -> serde_json::Value {
    let route = Arc::clone(&resolved.model.route);
    let mut request = oc_ai::schema::LLMRequest::new(resolved.model.clone());
    request.messages = vec![oc_ai::schema::Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (_, body_text) = route.prepare_transport(&prepared, &body).expect("wire");
    serde_json::from_str(&body_text).expect("json wire body")
}

// ---------------------------------------------------------------------------
// CRED-A4: config provider apiKey/baseURL overlay semantics
// ---------------------------------------------------------------------------

/// apiKey-only entries on catalog ids inject auth without rebinding the
/// protocol; baseURL-only entries override the endpoint only (v2 merges
/// settings onto the provider record — config/plugin/provider.ts:70-79,
/// configured.ts:14-18).
#[tokio::test]
async fn config_provider_overlay() {
    let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let project = root.path().join("proj");
    let home = root.path().join("home");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        project.join("opencode.json"),
        r#"{
            "providers": {
                "anthropic": { "settings": { "apiKey": "cfg-ant-key" } },
                "groq": { "settings": { "baseURL": "https://gw.groq.test/v1" } }
            }
        }"#,
    )
    .unwrap();
    let loaded = oc_core::config::Loader::new(
        project,
        oc_core::config::GlobalDirs {
            config: home.join(".config").join("opencode"),
            home: home.clone(),
        },
    )
    .load();
    let providers = oc_core::provider_catalog::providers_from_config(&loaded);

    let catalog = Arc::new(Catalog::embedded().clone());
    let groq_catalog = catalog.provider("groq").expect("groq present");
    let groq_model = groq_catalog.models.first().expect("groq model").id.0.clone();
    let session = session_row();

    // The shadow leg (v2 config/plugin/provider.ts:70-79): the config
    // entry's custom record must NOT rebind the catalog provider to
    // openai-compatible — the resolver overlays it onto the NATIVE
    // binding (baseURL from the entry, protocol from the catalog).
    {
        let transport: oc_ai::route::SharedTransport =
            Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
        let resolver = CatalogModelResolver::new(
            FacadeContext::new(transport),
            Arc::new(Catalog::embedded().clone()),
            BTreeMap::new(),
            providers.custom.clone(),
            None,
        );
        let bound = resolver
            .resolve_selected(&session, &model_ref("groq", &groq_model))
            .expect("groq resolves");
        assert_eq!(
            bound.model.route.id, "groq-chat",
            "a baseURL entry on a catalog id keeps the native protocol"
        );
        assert_eq!(
            bound.model.route.endpoint.base_url.as_deref(),
            Some("https://gw.groq.test/v1"),
            "the entry's baseURL still overrides the endpoint"
        );
    }

    // The connection leg (what providers_from_config now extracts —
    // `overlay_entries_without_base_url_become_connections` pins the
    // extraction): apiKey/baseURL ride the connection overlay; the
    // apiKey reaches the native anthropic header, the baseURL overrides
    // the groq endpoint only.
    let connections = BTreeMap::from([
        (
            "anthropic".to_owned(),
            oc_core::provider_catalog::facade::ModelConnection {
                api_key: Some("cfg-ant-key".to_owned()),
                ..Default::default()
            },
        ),
        (
            "groq".to_owned(),
            oc_core::provider_catalog::facade::ModelConnection {
                base_url: Some("https://gw.groq.test/v1".to_owned()),
                ..Default::default()
            },
        ),
    ]);
    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        connections,
        providers.custom.clone(),
        None,
    );
    let bound = resolver
        .resolve_selected(&session, &model_ref("anthropic", "claude-opus-4-5"))
        .expect("anthropic resolves");
    assert!(
        bound.model.route.id.contains("anthropic"),
        "apiKey-only keeps the native anthropic protocol (route {})",
        bound.model.route.id
    );
    assert_eq!(
        wire_header(&bound, "x-api-key").expect("wire").as_deref(),
        Some("cfg-ant-key"),
        "the config apiKey reaches the anthropic request header"
    );
    let bound = resolver
        .resolve_selected(&session, &model_ref("groq", &groq_model))
        .expect("groq resolves");
    assert_eq!(bound.model.route.id, "groq-chat", "baseURL-only keeps the groq protocol");
    assert_eq!(
        bound.model.route.endpoint.base_url.as_deref(),
        Some("https://gw.groq.test/v1"),
        "the config baseURL overrides the endpoint only"
    );
}

// ---------------------------------------------------------------------------
// CRED-A5: env method filtering (vertex/bedrock/cloudflare)
// ---------------------------------------------------------------------------

/// v2's environmentNames filter + the Bedrock plugin's env override
/// (models-dev.ts:77-87, amazon-bedrock.ts:22-27): only the listed names
/// become env credentials — ambient AWS/Vertex configuration never makes
/// a provider fake-connected.
#[tokio::test]
async fn env_method_filter() {
    async fn env_names(host: &IntegrationHost, id: &str) -> Vec<String> {
        let integration = host.get(id).await.expect("get").expect("integration row");
        integration
            .methods
            .iter()
            .filter_map(|method| match method {
                oc_core::integration::types::Method::Env { names } => Some(names.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    let _env = env_guard().await;
    // SAFETY: test-scoped env scrub — the "ambient AWS config" leg.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("AWS_BEARER_TOKEN_BEDROCK");
        std::env::set_var("AWS_ACCESS_KEY_ID", "AKIA-fake");
    }
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox tempdir");
    let db = Arc::new(
        oc_core::database::Database::open_file(dir.path().join("t.db")).await.expect("db"),
    );
    let bus = Arc::new(oc_core::bus::Bus::new(
        (*db).clone(),
        oc_core::bus::BusOptions { persist: false, ..oc_core::bus::BusOptions::deterministic() },
    ));
    let host = IntegrationHost::new(db, bus, Catalog::embedded(), "opencode-test");

    // Vertex: project/location/ADC vars drop; only the Express key stays.
    assert_eq!(env_names(&host, "google-vertex").await, vec!["GOOGLE_VERTEX_API_KEY"]);
    // Cloudflare Workers AI: the token trio replaces the raw env list.
    assert_eq!(
        env_names(&host, "cloudflare-workers-ai").await,
        vec!["CLOUDFLARE_API_KEY", "CLOUDFLARE_WORKERS_AI_TOKEN", "CLOUDFLARE_API_TOKEN"]
    );
    // Bedrock: only the bearer token is a key credential.
    assert_eq!(env_names(&host, "amazon-bedrock").await, vec!["AWS_BEARER_TOKEN_BEDROCK"]);
    // …and ambient SigV4 env no longer fakes a connected bedrock.
    assert!(
        host.connection_active("amazon-bedrock").await.expect("active").is_none(),
        "AWS_ACCESS_KEY_ID alone must not connect bedrock"
    );
    // The azure filter keeps its shape (regression guard for the fold).
    let azure = env_names(&host, "azure").await;
    assert!(azure.iter().all(|name| name.ends_with("_API_KEY")), "{azure:?}");
    assert!(azure.contains(&"AZURE_COGNITIVE_SERVICES_API_KEY".to_owned()));
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
    let engine =
        Engine::boot(EngineOptions::new(dir.path().join("data"), dir.path().join("home"))).await;

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
    let bound = services
        .resolver
        .resolve_selected(&session, &model_ref("opencode", &free.id.0.clone()))
        .expect("zen model resolves keyless");
    let header = wire_header(&bound, "authorization")
        .unwrap_or_else(|error| panic!("keyless zen request failed to build: {error}"));
    assert_eq!(
        header.as_deref(),
        Some("Bearer public"),
        "keyless zen must downgrade to the public free-tier key"
    );
}
