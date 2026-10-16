//! BE-3 lane tests: the integration/auth host over the real HTTP surface —
//! list/get projections, key connect + credential events, the zen device
//! flow against a local mock OAuth server through the real routes,
//! well-known discovery registering a command method, code-required 400s,
//! and the 404 envelopes.

use std::sync::Arc;
use std::time::Duration;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::{json, Value};

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    _workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    let data = tempfile::tempdir().expect("data tempdir");
    let workspace = data.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
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
    TestServer { client, state, _workspace: workspace, _data: data }
}

fn location_query() -> IntegrationListQuery {
    IntegrationListQuery {
        location: Some(QueryLocation {
            directory: Some(
                std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
            ),
        }),
    }
}

/// A single-process mock OAuth/IdP server: serves the device endpoints and
/// a well-known manifest from one port.
struct MockIdp {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for MockIdp {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn mock_idp(
    device_body: Value,
    token_bodies: Vec<Value>,
    wellknown: Option<Value>,
) -> MockIdp {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        let mut token_index = 0usize;
        let mut requests: Vec<String> = Vec::new();
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0u8; 8192];
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .split('?')
                .next()
                .unwrap_or_default()
                .to_owned();
            let body_start = request.find("\r\n\r\n").map_or(read, |index| index + 4);
            let form_body = request[body_start..].to_owned();
            requests.push(format!("{path} {form_body}"));
            let (response_body, kind) = if path == "/.well-known/opencode" {
                (wellknown.clone().unwrap_or(Value::Null), 200u16)
            } else if path == "/auth/device/code" {
                (device_body.clone(), 200)
            } else if path == "/auth/device/token" {
                let body = token_bodies[token_index.min(token_bodies.len() - 1)].clone();
                token_index += 1;
                (body, 200)
            } else {
                (json!({"error": "not_found"}), 404)
            };
            std::fs::write("/tmp/opencode/be3-mock-requests.log", requests.join("\n")).ok();
            let response = format!(
                "HTTP/1.1 {kind} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.to_string().len(),
                response_body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    MockIdp { base: format!("http://127.0.0.1:{port}"), handle }
}

// -- list/get -----------------------------------------------------------------

#[tokio::test]
async fn integration_list_and_get_round_trip() {
    let server = boot().await;
    let list = server.client.integration().list(&location_query()).await.expect("list");
    let ids: Vec<&str> = list.data.iter().map(|info| info.id.as_str()).collect();
    assert!(ids.contains(&"opencode"), "opencode missing: {ids:?}");
    assert!(ids.contains(&"github-copilot"), "copilot missing: {ids:?}");
    assert!(ids.contains(&"anthropic"), "anthropic missing: {ids:?}");

    let get = server
        .client
        .integration()
        .get("opencode", &IntegrationGetQuery { location: location_query().location })
        .await
        .expect("get");
    assert_eq!(get.data.id, "opencode");
    let method_kinds: Vec<&str> = get
        .data
        .methods
        .iter()
        .map(|method| match method {
            IntegrationMethod::OAuthMethod(_) => "oauth",
            IntegrationMethod::CommandMethod(_) => "command",
            IntegrationMethod::KeyMethod(_) => "key",
            IntegrationMethod::EnvMethod(_) => "env",
        })
        .collect();
    assert!(method_kinds.contains(&"oauth") && method_kinds.contains(&"key"), "{method_kinds:?}");

    // Unknown → 404 IntegrationNotFoundError with the id in the envelope.
    let error = server
        .client
        .integration()
        .get("nope", &IntegrationGetQuery { location: location_query().location })
        .await
        .expect_err("404");
    let body = error.to_string();
    assert!(body.contains("IntegrationNotFoundError"), "{body}");
    assert!(body.contains("nope"), "{body}");
}

// -- key connect ----------------------------------------------------------------

#[tokio::test]
async fn connect_key_stores_credential_and_streams_credential_events() {
    let server = boot().await;
    let feed = Arc::new(oc_server::sse::EventFeed::new());
    feed.spawn_tap(&server.state.engine.bus);
    let mut subscription = feed.subscribe();

    server
        .client
        .integration()
        .connect()
        .key(
            "anthropic",
            &IntegrationConnectKeyQuery { location: location_query().location },
            &IntegrationConnectKeyBody {
                key: "sk-route-key".into(),
                answer: None,
                label: Some("work key".into()),
            },
        )
        .await
        .expect("connect key");

    let info = server
        .client
        .integration()
        .get("anthropic", &IntegrationGetQuery { location: location_query().location })
        .await
        .expect("get");
    assert_eq!(info.data.connections.len(), 1);
    let ConnectionInfo::CredentialInfo(credential) = &info.data.connections[0] else {
        panic!("credential connection")
    };
    assert_eq!(credential.label, "work key");

    // The credential events ride the public stream (BE-1 registry +
    // BE-3 publisher): credential.updated then credential.switched.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut saw_updated = false;
    let mut saw_switched = false;
    while !(saw_updated && saw_switched) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let frame = tokio::time::timeout(remaining, subscription.recv())
            .await
            .expect("timeout waiting for credential events")
            .expect("stream open")
            .expect("ok");
        let text = frame.to_string();
        if text.contains("\"credential.updated\"") {
            saw_updated = true;
        }
        if text.contains("\"credential.switched\"") {
            saw_switched = true;
        }
    }
}

// -- zen device flow over the routes ----------------------------------------------

#[tokio::test]
async fn zen_oauth_flow_round_trip_via_routes() {
    let idp = mock_idp(
        json!({
            "device_code": "dev-xyz",
            "user_code": "WDJB-MJHT",
            "verification_uri_complete": "https://opencode.ai/console/verify?code=WDJB-MJHT",
            "expires_in": 900,
            "interval": 0
        }),
        vec![
            json!({"error": "authorization_pending"}),
            json!({"access_token": "zen-route-access", "refresh_token": "zen-route-refresh", "expires_in": 3600, "org_id": "org_route"}),
        ],
        None,
    )
    .await;
    let server = boot().await;

    let mut answer = FormAnswer::new();
    answer.insert("server".into(), FormValue::String(idp.base.clone()));
    let attempt = server
        .client
        .integration()
        .oauth()
        .connect(
            "opencode",
            &IntegrationOauthConnectQuery { location: location_query().location },
            &IntegrationOauthConnectBody {
                answer: Some(answer),
                label: None,
                method_id: "device".into(),
            },
        )
        .await
        .expect("oauth connect");
    assert_eq!(attempt.data.mode.as_str(), "auto");
    assert!(attempt.data.attempt_id.starts_with("con_"));
    assert!(attempt.data.url.contains("verify"));
    assert!(attempt.data.instructions.contains("WDJB-MJHT"), "{}", attempt.data.instructions);

    // Poll until complete (auto mode settles the forked poll).
    let mut status = None;
    for _ in 0..400 {
        let response = server
            .client
            .integration()
            .oauth()
            .status(
                "opencode",
                &attempt.data.attempt_id,
                &IntegrationOauthStatusQuery { location: location_query().location },
            )
            .await
            .expect("status");
        if !matches!(response.data, IntegrationAttemptStatus::Pending(_)) {
            status = Some(response.data);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        matches!(status, Some(IntegrationAttemptStatus::Complete(_))),
        "expected complete, got {status:?}"
    );

    // The credential connection is stored and labeled from orgName.
    let info = server
        .client
        .integration()
        .get("opencode", &IntegrationGetQuery { location: location_query().location })
        .await
        .expect("get");
    assert_eq!(info.data.connections.len(), 1);
    let ConnectionInfo::CredentialInfo(credential) = &info.data.connections[0] else {
        panic!("credential connection")
    };
    assert_eq!(credential.label, "org_route");
    assert_eq!(credential.method.as_str(), "oauth");

    // The mock saw the v2 request shapes (client_id + device grant).
    let log = std::fs::read_to_string("/tmp/opencode/be3-mock-requests.log").unwrap_or_default();
    assert!(log.contains("/auth/device/code"), "{log}");
    assert!(log.contains("client_id=opencode-cli"), "{log}");
    assert!(log.contains("supports_org_scope=true"), "{log}");
    assert!(
        log.contains("urn:ietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
            || log.contains("grant_type=urn"),
        "{log}"
    );

    // Unknown attempt → 404 IntegrationAttemptNotFoundError.
    let error = server
        .client
        .integration()
        .oauth()
        .status(
            "opencode",
            "con_missing",
            &IntegrationOauthStatusQuery { location: location_query().location },
        )
        .await
        .expect_err("404");
    assert!(error.to_string().contains("IntegrationAttemptNotFoundError"), "{}", error);
}

// -- wellknown discovery ------------------------------------------------------------

#[tokio::test]
async fn wellknown_add_registers_command_integration() {
    let manifest = json!({
        "auth": { "command": ["echo", "wellknown-sourced"], "env": "ACME_TOKEN" },
        "config": { "provider": { "acme": { "npm": "@acme/sdk" } } }
    });
    let idp = mock_idp(Value::Null, Vec::new(), Some(manifest)).await;
    let server = boot().await;

    // No auth at a manifest-less origin → 400 well_known_discovery.
    let bare = mock_idp(json!({}), Vec::new(), Some(json!({ "config": {} }))).await;
    let error = server
        .client
        .integration()
        .wellknown_add(
            &ExperimentalIntegrationWellknownAddQuery { location: location_query().location },
            &ExperimentalIntegrationWellknownAddBody { url: bare.base.clone() },
        )
        .await
        .expect_err("400");
    match &error {
        oc_client::ClientError::Api { status, fields, .. } => {
            assert_eq!(*status, 400);
            assert_eq!(
                fields.get("kind").and_then(|kind| kind.as_str()),
                Some("well_known_discovery"),
                "{error}"
            );
        }
        other => panic!("expected api error, got {other}"),
    }

    server
        .client
        .integration()
        .wellknown_add(
            &ExperimentalIntegrationWellknownAddQuery { location: location_query().location },
            &ExperimentalIntegrationWellknownAddBody { url: idp.base.clone() },
        )
        .await
        .expect("wellknown add");

    // The integration appears with the login command method.
    let get = server
        .client
        .integration()
        .get(&idp.base, &IntegrationGetQuery { location: location_query().location })
        .await
        .expect("get wellknown integration");
    assert_eq!(get.data.id, idp.base);
    assert_eq!(get.data.methods.len(), 1);
    let IntegrationMethod::CommandMethod(method) = &get.data.methods[0] else {
        panic!("command method")
    };
    assert_eq!(method.id, "login");
    assert_eq!(method.command, vec!["echo".to_owned(), "wellknown-sourced".to_owned()]);

    // Running the command method stores the sourced key credential.
    let attempt = server
        .client
        .integration()
        .command()
        .connect(
            &idp.base,
            &IntegrationCommandConnectQuery { location: location_query().location },
            &IntegrationCommandConnectBody { label: None, method_id: "login".into() },
        )
        .await
        .expect("command connect");
    let mut status = None;
    for _ in 0..200 {
        let response = server
            .client
            .integration()
            .command()
            .status(
                &idp.base,
                &attempt.data.attempt_id,
                &IntegrationCommandStatusQuery { location: location_query().location },
            )
            .await
            .expect("command status");
        if !matches!(response.data, IntegrationCommandAttemptStatus::Pending(_)) {
            status = Some(response.data);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(matches!(status, Some(IntegrationCommandAttemptStatus::Complete(_))), "{status:?}");
    let get = server
        .client
        .integration()
        .get(&idp.base, &IntegrationGetQuery { location: location_query().location })
        .await
        .expect("get after command");
    assert_eq!(get.data.connections.len(), 1);
    let ConnectionInfo::CredentialInfo(credential) = &get.data.connections[0] else {
        panic!("credential connection")
    };
    assert_eq!(credential.method.as_str(), "key");
}

// -- code-required mapping --------------------------------------------------------------

#[tokio::test]
async fn oauth_complete_maps_code_required() {
    use oc_core::integration::types::{CredentialValue, Method};
    use oc_core::integration::{AuthMode, Authorization, OAuthImplementation};
    let server = boot().await;
    // Register a code-mode method directly on the host (route boundary test).
    let host = Arc::clone(&server.state.engine.integration);
    host.update_integration("codey", "Code Flow");
    host.update_method(
        "codey",
        Method::OAuth { id: "code".into(), label: "Manual code".into(), form: None },
        Some(OAuthImplementation {
            authorize: Arc::new(|_answer| {
                Box::pin(async {
                    Ok(Authorization {
                        url: "https://example.test/authorize".into(),
                        instructions: "paste the code".into(),
                        expires_at: None,
                        mode: AuthMode::Code(Arc::new(|code: String| {
                            Box::pin(async move {
                                Ok(CredentialValue::Key(oc_core::integration::types::KeyValue {
                                    key: code,
                                    metadata: None,
                                    configuration: None,
                                }))
                            })
                        })),
                    })
                })
            }),
            refresh: None,
            label: None,
        }),
    );
    let attempt = server
        .client
        .integration()
        .oauth()
        .connect(
            "codey",
            &IntegrationOauthConnectQuery { location: location_query().location },
            &IntegrationOauthConnectBody { answer: None, label: None, method_id: "code".into() },
        )
        .await
        .expect("connect");
    assert_eq!(attempt.data.mode.as_str(), "code");

    // complete without a code → 400 integration_code_required.
    let error = server
        .client
        .integration()
        .oauth()
        .complete(
            "codey",
            &attempt.data.attempt_id,
            &IntegrationOauthCompleteQuery { location: location_query().location },
            &IntegrationOauthCompleteBody { code: None },
        )
        .await
        .expect_err("400");
    // The envelope carries `kind: integration_code_required` (v2
    // CodeRequired mapping) — checked through the spread fields.
    match &error {
        oc_client::ClientError::Api { status, fields, .. } => {
            assert_eq!(*status, 400);
            assert_eq!(
                fields.get("kind").and_then(|kind| kind.as_str()),
                Some("integration_code_required"),
                "{error}"
            );
        }
        other => panic!("expected api error, got {other}"),
    }

    // With the code → 204 and the credential stored.
    server
        .client
        .integration()
        .oauth()
        .complete(
            "codey",
            &attempt.data.attempt_id,
            &IntegrationOauthCompleteQuery { location: location_query().location },
            &IntegrationOauthCompleteBody { code: Some("the-code".into()) },
        )
        .await
        .expect("complete");
    let get = server
        .client
        .integration()
        .get("codey", &IntegrationGetQuery { location: location_query().location })
        .await
        .expect("get");
    assert_eq!(get.data.connections.len(), 1);
}
