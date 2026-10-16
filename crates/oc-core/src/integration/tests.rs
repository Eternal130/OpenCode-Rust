//! Integration-host tests: registry projections, credential store
//! semantics + events, attempt lifecycles, command sourcing, and the
//! device/loopback flows against local mock servers (no real network).

use super::credential;
use super::types::*;
use super::{AuthMode, Authorization, IntegrationHost, OAuthImplementation};
use crate::bus::BusOptions;
use futures::future::BoxFuture;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

async fn host() -> Arc<IntegrationHost> {
    let (host, _bus) = host_with_bus().await;
    host
}

async fn host_with_bus() -> (Arc<IntegrationHost>, Arc<crate::bus::Bus>) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db = std::sync::Arc::new(
        crate::database::Database::open_file(dir.path().join("t.db")).await.expect("db"),
    );
    // keep the dir alive for the process of the test
    std::mem::forget(dir);
    let bus = Arc::new(crate::bus::Bus::new(
        (*db).clone(),
        crate::bus::BusOptions { persist: false, ..BusOptions::deterministic() },
    ));
    let host = IntegrationHost::new(
        db,
        bus.clone(),
        crate::provider_catalog::catalog::Catalog::embedded(),
        "opencode-test",
    );
    (host, bus)
}

fn answer(pairs: &[(&str, &str)]) -> FormAnswer {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
        .collect()
}

// -- registry --------------------------------------------------------------

#[tokio::test]
async fn registry_lists_named_and_catalog_integrations() {
    let host = host().await;
    let list = host.list().await.expect("list");
    let by_id = |id: &str| list.iter().find(|info| info.id == id);

    let opencode = by_id("opencode").expect("opencode integration");
    assert_eq!(opencode.name, "OpenCode Console");
    assert!(opencode
        .methods
        .iter()
        .any(|method| matches!(method, Method::OAuth { id, .. } if id == "device")));
    assert!(opencode.methods.iter().any(|method| matches!(method, Method::Key { label: Some(label), .. } if label == "API key (service account)")));

    let copilot = by_id("github-copilot").expect("copilot integration");
    // v2 removes the catalog key method for copilot (github-copilot.ts:213).
    assert!(!copilot.methods.iter().any(|method| matches!(method, Method::Key { .. })));
    assert!(copilot
        .methods
        .iter()
        .any(|method| matches!(method, Method::OAuth { id, .. } if id == "device")));

    // Catalog-wide key/env methods (models-dev.ts:33-52).
    let anthropic = by_id("anthropic").expect("anthropic integration");
    assert!(anthropic.methods.iter().any(|method| matches!(method, Method::Key { .. })));
    assert!(anthropic.methods.iter().any(|method| match method {
        Method::Env { names } => names.contains(&"ANTHROPIC_API_KEY".to_string()),
        _ => false,
    }));
}

#[tokio::test]
async fn env_connections_surface_when_variable_set() {
    // SAFETY: test-scoped env mutation; ANTHROPIC_API_KEY is not read by
    // anything else in this test process.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "sk-test");
    };
    let host = host().await;
    let info = host.get("anthropic").await.expect("get").expect("present");
    let env_connections: Vec<_> = info
        .connections
        .iter()
        .filter(|connection| matches!(connection, ConnectionInfo::Env { name, .. } if name == "ANTHROPIC_API_KEY"))
        .collect();
    assert_eq!(env_connections.len(), 1);
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    };
}

// -- credential store --------------------------------------------------------

#[tokio::test]
async fn credential_create_activates_and_emits_events() {
    let (host, bus) = host_with_bus().await;
    let mut subscription = bus.subscribe();
    let value =
        CredentialValue::Key(KeyValue { key: "k1".into(), metadata: None, configuration: None });
    let (first, activated) = host
        .store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "anthropic",
            label: Some("first"),
            value: &value,
            activate: None,
        })
        .await
        .expect("create");
    assert!(activated, "first credential activates");

    // Second create without activate=false takes over the selection.
    let value2 =
        CredentialValue::Key(KeyValue { key: "k2".into(), metadata: None, configuration: None });
    let (_, activated2) = host
        .store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "anthropic",
            label: Some("second"),
            value: &value2,
            activate: None,
        })
        .await
        .expect("create 2");
    assert!(activated2);

    let active = host.connection_active("anthropic").await.expect("active").expect("some");
    let ConnectionInfo::Credential { label, .. } = &active else { panic!("credential connection") };
    assert_eq!(label, "second");

    // Events: updated (x2) + switched (x2) — drained via the bus feed.
    let mut updated = 0;
    let mut switched = 0;
    for _ in 0..4 {
        let frame =
            tokio::time::timeout(std::time::Duration::from_secs(5), subscription.next()).await;
        let Ok(Some(frame)) = frame else { break };
        match frame.type_.as_str() {
            "credential.updated" => updated += 1,
            "credential.switched" => switched += 1,
            _ => {}
        }
    }
    assert!(updated >= 2, "credential.updated emitted: {updated}");
    assert!(switched >= 2, "credential.switched emitted: {switched}");
    let _ = first;
}

#[tokio::test]
async fn connect_key_derives_integration_label() {
    let host = host().await;
    host.connect_key("anthropic", "sk-derived-1", &BTreeMap::new(), None)
        .await
        .expect("connect key");
    let info = host.get("anthropic").await.expect("get").expect("present");
    let ConnectionInfo::Credential { label, method, .. } = &info.connections[0] else { panic!() };
    assert_eq!(label, "Anthropic");
    assert_eq!(method, "key");
    // Second connect derives "Anthropic 2" (integration.ts:358-367).
    host.connect_key("anthropic", "sk-derived-2", &BTreeMap::new(), None)
        .await
        .expect("connect key 2");
    let info = host.get("anthropic").await.expect("get 2").expect("present");
    let labels: Vec<String> = info
        .connections
        .iter()
        .filter_map(|connection| match connection {
            ConnectionInfo::Credential { label, .. } => Some(label.clone()),
            ConnectionInfo::Env { .. } => None,
        })
        .collect::<Vec<String>>();
    assert!(
        labels.contains(&"Anthropic".to_string()) && labels.contains(&"Anthropic 2".to_string()),
        "{labels:?}"
    );
}

// -- attempt lifecycles (scripted methods; no network) -----------------------

fn script_method(host: &IntegrationHost, id: &'static str, mode: &ScriptedMode) {
    let authorize: super::AuthorizeFn = match *mode {
        ScriptedMode::Code => Arc::new(|_answer| {
            Box::pin(async {
                Ok(Authorization {
                    url: "https://example.test/authorize".into(),
                    instructions: "enter the code".into(),
                    expires_at: None,
                    mode: AuthMode::Code(Arc::new(|code: String| {
                        Box::pin(async move {
                            if code == "good" {
                                Ok(CredentialValue::OAuth(OAuthValue {
                                    method_id: "scripted".into(),
                                    refresh: "r".into(),
                                    access: format!("access-{code}"),
                                    expires: 0,
                                    metadata: None,
                                }))
                            } else {
                                Err(format!("bad code: {code}"))
                            }
                        })
                    })),
                })
            }) as BoxFuture<'static, Result<Authorization, String>>
        }),
        ScriptedMode::Auto => Arc::new(|_answer| {
            Box::pin(async {
                Ok(Authorization {
                    url: "https://example.test/device".into(),
                    instructions: "visit".into(),
                    expires_at: None,
                    mode: AuthMode::Auto(Box::pin(async {
                        Ok(CredentialValue::OAuth(OAuthValue {
                            method_id: "scripted".into(),
                            refresh: "r".into(),
                            access: "auto-access".into(),
                            expires: 0,
                            metadata: None,
                        }))
                    })),
                })
            }) as BoxFuture<'static, Result<Authorization, String>>
        }),
    };
    host.update_integration(id, "Scripted");
    host.update_method(
        id,
        Method::OAuth { id: "scripted".into(), label: "Scripted login".into(), form: None },
        Some(OAuthImplementation { authorize, refresh: None, label: None }),
    );
}

enum ScriptedMode {
    Code,
    Auto,
}

#[tokio::test]
async fn oauth_code_attempt_lifecycle() {
    let host = host().await;
    script_method(&host, "scripted-code", &ScriptedMode::Code);
    let attempt = host
        .oauth_connect("scripted-code", "scripted", &BTreeMap::new(), None)
        .await
        .expect("connect");
    assert_eq!(attempt.mode, "code");
    assert!(attempt.attempt_id.starts_with("con_"));

    // Complete without a code → CodeRequired (integration.ts:772-774).
    let error = host.oauth_complete("scripted-code", &attempt.attempt_id, None).await.unwrap_err();
    assert!(matches!(error, IntegrationError::CodeRequired { .. }), "{error:?}");

    // Status still pending; complete with the code stores the credential.
    assert!(matches!(
        host.oauth_status("scripted-code", &attempt.attempt_id).await,
        Ok(AttemptStatus::Pending { .. })
    ));
    host.oauth_complete("scripted-code", &attempt.attempt_id, Some("good"))
        .await
        .expect("complete");
    assert!(matches!(
        host.oauth_status("scripted-code", &attempt.attempt_id).await,
        Ok(AttemptStatus::Complete { .. })
    ));

    let active = host.connection_active("scripted-code").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!("oauth") };
    assert_eq!(oauth.access, "access-good");

    // Foreign integration → AttemptNotFound.
    assert!(matches!(
        host.oauth_status("other", &attempt.attempt_id).await,
        Err(IntegrationError::AttemptNotFound)
    ));
}

#[tokio::test]
async fn oauth_auto_attempt_settles() {
    let host = host().await;
    script_method(&host, "scripted-auto", &ScriptedMode::Auto);
    let attempt = host
        .oauth_connect("scripted-auto", "scripted", &BTreeMap::new(), None)
        .await
        .expect("connect");
    assert_eq!(attempt.mode, "auto");
    for _ in 0..50 {
        if matches!(
            host.oauth_status("scripted-auto", &attempt.attempt_id).await,
            Ok(AttemptStatus::Complete { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(matches!(
        host.oauth_status("scripted-auto", &attempt.attempt_id).await,
        Ok(AttemptStatus::Complete { .. })
    ));
    let active = host.connection_active("scripted-auto").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "auto-access");
}

#[tokio::test]
async fn oauth_failure_records_message() {
    let host = host().await;
    host.update_integration("scripted-fail", "Failing");
    host.update_method(
        "scripted-fail",
        Method::OAuth { id: "boom".into(), label: "Always fails".into(), form: None },
        Some(OAuthImplementation {
            authorize: Arc::new(|_answer| {
                Box::pin(async {
                    Ok(Authorization {
                        url: "https://example.test".into(),
                        instructions: "nope".into(),
                        expires_at: None,
                        mode: AuthMode::Code(Arc::new(|code: String| {
                            Box::pin(async move { Err(format!("rejected: {code}")) })
                        })),
                    })
                }) as BoxFuture<'static, Result<Authorization, String>>
            }),
            refresh: None,
            label: None,
        }),
    );
    let attempt =
        host.oauth_connect("scripted-fail", "boom", &BTreeMap::new(), None).await.expect("connect");
    let error =
        host.oauth_complete("scripted-fail", &attempt.attempt_id, Some("nope")).await.unwrap_err();
    assert!(error.to_string().contains("rejected: nope"), "{error}");
    match host.oauth_status("scripted-fail", &attempt.attempt_id).await {
        Ok(AttemptStatus::Failed { message, .. }) => assert!(message.contains("rejected")),
        other => panic!("expected failed, got {other:?}"),
    }
    // No credential was stored.
    assert!(host.connection_active("scripted-fail").await.expect("active").is_none());
}

#[tokio::test]
async fn command_attempt_sources_key_from_stdout() {
    let host = host().await;
    host.update_integration("cmd-integration", "Commanded");
    host.update_method(
        "cmd-integration",
        Method::Command {
            id: "echo".into(),
            label: "Echo login".into(),
            command: vec!["echo".into(), "sourced-key-42".into()],
        },
        None,
    );
    let attempt = host.command_connect("cmd-integration", "echo", None).await.expect("connect");
    assert!(attempt.attempt_id.starts_with("con_"));
    for _ in 0..50 {
        if matches!(
            host.command_status("cmd-integration", &attempt.attempt_id).await,
            Ok(CommandAttemptStatus::Complete { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(matches!(
        host.command_status("cmd-integration", &attempt.attempt_id).await,
        Ok(CommandAttemptStatus::Complete { .. })
    ));
    let active = host.connection_active("cmd-integration").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::Key(key) = resolved else { panic!() };
    assert_eq!(key.key, "sourced-key-42");
}

#[tokio::test]
async fn command_attempt_failure_carries_message() {
    let host = host().await;
    host.update_integration("cmd-fail", "Failing command");
    host.update_method(
        "cmd-fail",
        Method::Command {
            id: "false".into(),
            label: "Fails".into(),
            command: vec!["sh".into(), "-c".into(), "echo boom >&2; exit 3".into()],
        },
        None,
    );
    let attempt = host.command_connect("cmd-fail", "false", None).await.expect("connect");
    for _ in 0..50 {
        if matches!(
            host.command_status("cmd-fail", &attempt.attempt_id).await,
            Ok(CommandAttemptStatus::Failed { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    match host.command_status("cmd-fail", &attempt.attempt_id).await {
        Ok(CommandAttemptStatus::Failed { message, .. }) => assert_eq!(message, "boom"),
        other => panic!("expected failed, got {other:?}"),
    }
}

// -- refresh ------------------------------------------------------------------

#[tokio::test]
async fn resolve_refreshes_expiring_oauth_token() {
    let host = host().await;
    // Insert an oauth credential that expires within the 5-minute margin.
    let expiring = CredentialValue::OAuth(OAuthValue {
        method_id: "refreshable".into(),
        refresh: "r1".into(),
        access: "stale".into(),
        expires: crate::database::migrations::now_ms() + 1000,
        metadata: None,
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "refresher",
            label: None,
            value: &expiring,
            activate: None,
        })
        .await
        .expect("create");
    host.update_integration("refresher", "Refresher");
    host.update_method(
        "refresher",
        Method::OAuth { id: "refreshable".into(), label: "Refreshable".into(), form: None },
        Some(OAuthImplementation {
            authorize: Arc::new(|_answer| {
                Box::pin(async { Err("unused".to_string()) })
                    as BoxFuture<'static, Result<Authorization, String>>
            }),
            refresh: Some(Arc::new(|value: OAuthValue| {
                Box::pin(async move {
                    Ok(CredentialValue::OAuth(OAuthValue {
                        access: format!("refreshed-from-{}", value.access),
                        ..value
                    }))
                })
            })),
            label: None,
        }),
    );
    let active = host.connection_active("refresher").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "refreshed-from-stale");
    // The refreshed value was persisted.
    let ConnectionInfo::Credential { id, .. } = active else { panic!() };
    let stored = host.store().get(&id).await.expect("get").expect("stored");
    let CredentialValue::OAuth(persisted) = stored.value else { panic!() };
    assert_eq!(persisted.access, "refreshed-from-stale");
}

// -- device flow against a local mock server -----------------------------------

/// A single-shot mock HTTP server for OAuth endpoints.
struct MockOAuth {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl MockOAuth {
    async fn start(device_body: Value, token_bodies: Vec<Value>) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = tokio::spawn(async move {
            let mut token_index = 0;
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let body_start = request.find("\r\n\r\n").map_or(read, |index| index + 4);
                let form_body = request[body_start..].to_owned();
                let (status, response_body) = if path.starts_with("/auth/device/code") {
                    (200, device_body.clone())
                } else if path.starts_with("/auth/device/token") {
                    let body = token_bodies[token_index.min(token_bodies.len() - 1)].clone();
                    token_index += 1;
                    (200, body)
                } else {
                    (404, json!({"error": "not_found"}))
                };
                // Capture the request for shape assertions via stdout of the test.
                eprintln!("MOCK {path} <- {form_body}");
                let response = format!(
                    "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    response_body.to_string().len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), handle }
    }
}

impl Drop for MockOAuth {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[tokio::test]
async fn zen_device_flow_round_trip_against_mock() {
    let mock = MockOAuth::start(
        json!({
            "device_code": "dev-123",
            "user_code": "ABCD-1234",
            "verification_uri_complete": "/verify?code=ABCD-1234",
            "expires_in": 900,
            "interval": 0
        }),
        vec![
            json!({"error": "authorization_pending"}),
            json!({"access_token": "zen-access", "refresh_token": "zen-refresh", "expires_in": 3600, "org_id": "org_9"}),
        ],
    )
    .await;

    let host = host().await;
    let attempt = host
        .oauth_connect("opencode", "device", &answer(&[("server", mock.base.as_str())]), None)
        .await
        .expect("connect");
    assert_eq!(attempt.mode, "auto");
    assert_eq!(attempt.url, format!("{}/verify?code=ABCD-1234", mock.base));
    assert_eq!(attempt.instructions, "Enter code: ABCD-1234");
    // The device code exchange drives expiresAt through (integration.ts:580).
    assert!(attempt.time.expires > crate::database::migrations::now_ms());

    for _ in 0..200 {
        if matches!(
            host.oauth_status("opencode", &attempt.attempt_id).await,
            Ok(AttemptStatus::Complete { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    match host.oauth_status("opencode", &attempt.attempt_id).await {
        Ok(AttemptStatus::Complete { .. }) => {}
        other => panic!("expected complete, got {other:?}"),
    }
    // The credential: label derives from orgName (opencode.ts:152-164).
    let info = host.get("opencode").await.expect("get").expect("present");
    let ConnectionInfo::Credential { label, method, .. } = &info.connections[0] else { panic!() };
    assert_eq!(label, "org_9");
    assert_eq!(method, "oauth");
    let active = host.connection_active("opencode").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "zen-access");
    assert_eq!(oauth.refresh, "zen-refresh");
}

// -- loopback listener ---------------------------------------------------------

#[tokio::test]
async fn loopback_listener_serves_callback_and_resolves_params() {
    let listener = super::flows::listen_loopback(super::flows::LoopbackOptions {
        port: 0,
        host: "127.0.0.1".into(),
        callback_path: "/callback",
        page: super::flows::CallbackPage::Html(super::flows::success_page("Test")),
    })
    .await
    .expect("listen");
    let port = listener.port;
    let waiter = tokio::spawn(async move { listener.wait().await });
    // Act as the browser redirect.
    let response = reqwest::get(format!("http://127.0.0.1:{port}/callback?code=abc&state=xyz"))
        .await
        .expect("get");
    assert_eq!(response.status(), 200);
    let text = response.text().await.expect("body");
    assert!(text.contains("Connected to Test"), "{text}");
    let params = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
        .await
        .expect("timeout")
        .expect("join")
        .expect("params");
    assert_eq!(params.get("code").map(String::as_str), Some("abc"));
    assert_eq!(params.get("state").map(String::as_str), Some("xyz"));
}

// -- wire shapes -----------------------------------------------------------------

#[tokio::test]
async fn info_serializes_to_v2_wire_shape() {
    let host = host().await;
    host.connect_key("anthropic", "sk-wire", &BTreeMap::new(), None).await.expect("key");
    let info = host.get("anthropic").await.expect("get").expect("present");
    let value = serde_json::to_value(&info).expect("serialize");
    assert_eq!(value["id"], "anthropic");
    assert_eq!(value["name"], "Anthropic");
    assert!(value["methods"].as_array().is_some_and(|methods| !methods.is_empty()));
    let connection = &value["connections"][0];
    assert_eq!(connection["type"], "credential");
    assert_eq!(connection["label"], "Anthropic");
    assert_eq!(connection["method"], "key");
    // Env method shape.
    let methods = value["methods"].as_array().unwrap();
    let env = methods.iter().find(|method| method["type"] == "env").expect("env method");
    assert!(env["names"]
        .as_array()
        .is_some_and(|names| names.iter().any(|name| name == "ANTHROPIC_API_KEY")));
}
