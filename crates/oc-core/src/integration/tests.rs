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

/// CD-9 (scan-cd): several flow tests mutate PROCESS-GLOBAL environment
/// variables (`GITLAB_OAUTH_CLIENT_ID` in the pkce/bundled pair,
/// `GOOGLE_APPLICATION_CREDENTIALS` in the adc/service-account pair).
/// Under default parallelism the mutations interleave: the bundled test
/// sees the pkce test's client id (its rejection expectation flips and
/// its connect grabs loopback 8080 out from under the pkce test), and
/// the adc test reads the service-account file mid-flight. Serializing
/// the env-mutating tests on one lock keeps the suite green under
/// default threads WITHOUT weakening any assertion (the register's
/// repro: 3/41 fail parallel, 41/41 pass serial).
static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Guards a test body against concurrent process-env mutation: hold the
/// CD-9 lock for the duration.
async fn env_guard() -> tokio::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK.lock().await
}

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

/// A single-shot mock HTTP server: path-prefix → sequential JSON bodies,
/// with a shared request log for shape assertions.
struct MockOAuth {
    base: String,
    log: Arc<parking_lot::Mutex<Vec<String>>>,
    #[allow(dead_code)]
    handle: tokio::task::JoinHandle<()>,
}

impl MockOAuth {
    #[allow(clippy::type_complexity)]
    async fn start(routes: Vec<(&'static str, Vec<Value>)>) -> Self {
        Self::start_with_status(
            routes
                .into_iter()
                .map(|(prefix, bodies)| {
                    (prefix, bodies.into_iter().map(|body| (200u16, body)).collect())
                })
                .collect(),
        )
        .await
    }

    #[allow(clippy::type_complexity)]
    async fn start_with_status(routes: Vec<(&'static str, Vec<(u16, Value)>)>) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let log: Arc<parking_lot::Mutex<Vec<String>>> = Arc::default();
        let log_handle = Arc::clone(&log);
        let mut cursors: Vec<(&'static str, std::collections::VecDeque<(u16, Value)>)> =
            routes.into_iter().map(|(prefix, bodies)| (prefix, bodies.into())).collect();
        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = Vec::with_capacity(4096);
                let mut chunk = [0u8; 4096];
                // Read until the header terminator AND any content-length
                // body (single reads can split the head).
                loop {
                    let read = stream.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    let Some(head_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
                    else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.trim()
                                .eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if buffer.len() >= head_end + 4 + length {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&buffer).into_owned();
                let read = request.len();
                let path = request.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let body_start = request.find("\r\n\r\n").map_or(read, |index| index + 4);
                let form_body = request[body_start..].to_owned();
                let headers = request
                    .lines()
                    .skip(1)
                    .take_while(|line| !line.is_empty())
                    .filter(|line| {
                        let lowered = line.to_ascii_lowercase();
                        lowered.starts_with("authorization:") || lowered.starts_with("x-org-id:")
                    })
                    .map(std::borrow::ToOwned::to_owned)
                    .collect::<Vec<_>>()
                    .join(" | ");
                log_handle.lock().push(format!("{path} {form_body} | {headers}"));
                let route = cursors.iter_mut().find(|(prefix, _)| path.starts_with(prefix));
                let (status, response_body) = match route {
                    Some((_, bodies)) => bodies.pop_front().unwrap_or((404, Value::Null)),
                    None => (404, serde_json::json!({"error": "not_found"})),
                };
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response_body.to_string().len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), log, handle }
    }

    fn requests(&self) -> Vec<String> {
        self.log.lock().clone()
    }
}

// -- device-flow round-trips against the mock -----------------------------------

#[tokio::test]
async fn zen_device_flow_round_trip_against_mock() {
    let mock = MockOAuth::start(vec![
        (
            "/auth/device/code",
            vec![json!({
                "device_code": "dev-123",
                "user_code": "ABCD-1234",
                "verification_uri_complete": "/verify?code=ABCD-1234",
                "expires_in": 900,
                "interval": 0
            })],
        ),
        (
            "/auth/device/token",
            vec![
                json!({"error": "authorization_pending"}),
                json!({"access_token": "zen-access", "refresh_token": "zen-refresh", "expires_in": 3600, "org_id": "org_9"}),
            ],
        ),
    ])
    .await;

    let host = host().await;
    let attempt = host
        .oauth_connect("opencode", "device", &answer(&[("server", mock.base.as_str())]), None)
        .await
        .expect("connect");
    assert_eq!(attempt.mode, "auto");
    assert_eq!(attempt.url, format!("{}/verify?code=ABCD-1234", mock.base));
    assert_eq!(attempt.instructions, "Enter code: ABCD-1234");
    assert!(attempt.time.expires > crate::database::migrations::now_ms());

    for _ in 0..400 {
        if matches!(
            host.oauth_status("opencode", &attempt.attempt_id).await,
            Ok(AttemptStatus::Complete { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(matches!(
        host.oauth_status("opencode", &attempt.attempt_id).await,
        Ok(AttemptStatus::Complete { .. })
    ));

    // Request shapes (opencode.ts:96-99, 30-37): client_id + org scope on
    // the device request; the device grant on the poll.
    let requests = mock.requests().join("\n");
    assert!(requests.contains("client_id=opencode-cli"), "{requests}");
    assert!(requests.contains("supports_org_scope=true"), "{requests}");
    assert!(requests.contains("grant_type=urn"), "{requests}");
    assert!(requests.contains("device_code=dev-123"), "{requests}");

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

#[tokio::test]
async fn copilot_device_flow_round_trip_against_mock() {
    let mock = MockOAuth::start(vec![
        (
            "/login/device/code",
            vec![json!({
                "device_code": "gh-dev",
                "user_code": "FFFF-GGGG",
                "verification_uri": "https://github.com/login/device",
                "expires_in": 900,
                "interval": 0
            })],
        ),
        (
            "/login/oauth/access_token",
            vec![json!({"access_token": "gho_copilot", "expires_in": 0})],
        ),
        (
            "/copilot_internal/user",
            vec![json!({"chat_enabled": true, "endpoints": {"api": "https://api.githubcopilot.com/"}})],
        ),
    ])
    .await;
    let host = host().await;
    // The device endpoints are github.com fixed (github-copilot.ts:73);
    // only the entitlement lookup is mockable through the api domain →
    // drive the flow with the enterprise path pointed at the mock host.
    let enterprise = mock.base.clone();
    let attempt = host
        .oauth_connect(
            "github-copilot",
            "device",
            &answer(&[("deploymentType", "enterprise"), ("enterpriseUrl", enterprise.as_str())]),
            None,
        )
        .await
        .expect("connect");
    assert_eq!(attempt.url, "https://github.com/login/device");
    assert!(attempt.instructions.contains("FFFF-GGGG"));

    for _ in 0..400 {
        if matches!(
            host.oauth_status("github-copilot", &attempt.attempt_id).await,
            Ok(AttemptStatus::Complete { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(matches!(
        host.oauth_status("github-copilot", &attempt.attempt_id).await,
        Ok(AttemptStatus::Complete { .. })
    ));

    // Request shapes (github-copilot.ts:71-73, 92-98): client id + scope;
    // the device grant; the entitlement probe with the api version header.
    let requests = mock.requests().join("\n");
    assert!(requests.contains("client_id=Ov23li8tweQw6odWQebz"), "{requests}");
    assert!(requests.contains("scope=read"), "{requests}");
    assert!(requests.contains("device_code=gh-dev"), "{requests}");

    // Credential: access == refresh (the token is both), expires 0,
    // metadata carries the enterprise URL + api endpoint.
    let active = host.connection_active("github-copilot").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "gho_copilot");
    assert_eq!(oauth.refresh, "gho_copilot");
    assert_eq!(oauth.expires, 0);
    let metadata = oauth.metadata.expect("metadata");
    assert_eq!(metadata.get("enterpriseUrl").and_then(Value::as_str), Some(enterprise.as_str()));
    assert_eq!(
        metadata.get("apiEndpoint").and_then(Value::as_str),
        Some("https://api.githubcopilot.com")
    );
}

#[tokio::test]
async fn copilot_entitlement_denial_fails_the_attempt() {
    let mock = MockOAuth::start(vec![
        (
            "/login/device/code",
            vec![json!({
                "device_code": "gh-dev-2",
                "user_code": "AAAA-BBBB",
                "verification_uri": "https://github.com/login/device",
                "expires_in": 900,
                "interval": 0
            })],
        ),
        ("/login/oauth/access_token", vec![json!({"access_token": "gho_denied"})]),
        ("/copilot_internal/user", vec![json!({"chat_enabled": false})]),
    ])
    .await;
    let host = host().await;
    let enterprise = mock.base.clone();
    let attempt = host
        .oauth_connect(
            "github-copilot",
            "device",
            &answer(&[("deploymentType", "enterprise"), ("enterpriseUrl", enterprise.as_str())]),
            None,
        )
        .await
        .expect("connect");
    for _ in 0..400 {
        if matches!(
            host.oauth_status("github-copilot", &attempt.attempt_id).await,
            Ok(AttemptStatus::Failed { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    match host.oauth_status("github-copilot", &attempt.attempt_id).await {
        Ok(AttemptStatus::Failed { message, .. }) => {
            assert!(message.contains("not enabled"), "{message}");
        }
        other => panic!("expected failed, got {other:?}"),
    }
    assert!(host.connection_active("github-copilot").await.expect("active").is_none());
}

#[tokio::test]
async fn zen_refresh_round_trip_against_mock() {
    let mock = MockOAuth::start(vec![(
        "/auth/device/token",
        vec![json!({"access_token": "rotated-access", "refresh_token": "rotated-refresh", "expires_in": 1800})],
    )])
    .await;
    let host = host().await;
    // An expired zen credential pointing at the mock console (metadata.server).
    let expiring = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "old-refresh".into(),
        access: "old-access".into(),
        expires: crate::database::migrations::now_ms() - 1000,
        metadata: Some(BTreeMap::from([("server".to_owned(), Value::String(mock.base.clone()))])),
    });
    let (stored, _) = host
        .store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "opencode",
            label: Some("expiring"),
            value: &expiring,
            activate: None,
        })
        .await
        .expect("create");
    let _ = stored;
    let active = host.connection_active("opencode").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "rotated-access");
    assert_eq!(oauth.refresh, "rotated-refresh");
    // Refresh request shape (opencode.ts:115-123): refresh grant + client id.
    let requests = mock.requests().join("\n");
    assert!(requests.contains("grant_type=refresh_token"), "{requests}");
    assert!(requests.contains("refresh_token=old-refresh"), "{requests}");
    assert!(requests.contains("client_id=opencode-cli"), "{requests}");
}

#[tokio::test]
async fn zen_refresh_invalid_grant_reports_reconnect() {
    let mock = MockOAuth::start_with_status(vec![(
        "/auth/device/token",
        vec![(400, json!({"error": "invalid_grant"}))],
    )])
    .await;
    let host = host().await;
    let expiring = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "dead-refresh".into(),
        access: "old".into(),
        expires: crate::database::migrations::now_ms() - 1000,
        metadata: Some(BTreeMap::from([("server".to_owned(), Value::String(mock.base.clone()))])),
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "opencode",
            label: Some("dead"),
            value: &expiring,
            activate: None,
        })
        .await
        .expect("create");
    let active = host.connection_active("opencode").await.expect("active").expect("some");
    let error = host.connection_resolve(&active).await.expect_err("refresh fails");
    assert!(error.to_string().contains("Reconnect OpenCode Console"), "{error}");
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

// -- BE-3b: gitlab ---------------------------------------------------------------

fn parse_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(
                urlencoding::decode(v).map_or_else(|_| v.to_owned(), std::borrow::Cow::into_owned),
            );
        }
    }
    None
}

async fn wait_complete(
    host: &Arc<IntegrationHost>,
    integration: &str,
    attempt: &Attempt,
    spins: usize,
) -> AttemptStatus {
    for _ in 0..spins {
        if let Ok(status) = host.oauth_status(integration, &attempt.attempt_id).await {
            if !matches!(status, AttemptStatus::Pending { .. }) {
                return status;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    host.oauth_status(integration, &attempt.attempt_id).await.expect("status")
}

#[tokio::test]
async fn gitlab_pkce_flow_round_trip_against_mock() {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let _env = env_guard().await;
    // GitLab pins redirect port 8080 — a busy port is an environment
    // conflict (v2 fails identically), documented-skip.
    if tokio::net::TcpListener::bind(("127.0.0.1", 8080)).await.is_err() {
        eprintln!("skipping: port 8080 busy (gitlab pins it)");
        return;
    }
    let mock = MockOAuth::start(vec![(
        "/oauth/token",
        vec![
            json!({"access_token": "gl-access", "refresh_token": "gl-refresh", "expires_in": 7200}),
        ],
    )])
    .await;
    // SAFETY: test-scoped env (self-managed-instance client override).
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("GITLAB_OAUTH_CLIENT_ID", "test-client-id");
    };
    let host = host().await;
    let attempt = host
        .oauth_connect("gitlab", "pkce", &answer(&[("instanceUrl", mock.base.as_str())]), None)
        .await
        .expect("connect");
    assert_eq!(attempt.mode, "auto");
    // The authorize URL: fixed redirect + S256 challenge + api scope.
    assert!(attempt.url.starts_with(&format!("{}/oauth/authorize?", mock.base)), "{}", attempt.url);
    assert!(
        attempt.url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fcallback"),
        "{}",
        attempt.url
    );
    assert!(attempt.url.contains("scope=api"), "{}", attempt.url);
    assert!(attempt.url.contains("code_challenge_method=S256"), "{}", attempt.url);
    let state = parse_query_param(&attempt.url, "state").expect("state");

    // Act as the browser: hit the fixed loopback redirect.
    let callback =
        reqwest::get(format!("http://127.0.0.1:8080/callback?code=gl-code&state={state}"))
            .await
            .expect("callback");
    assert_eq!(callback.status(), 200);

    let status = wait_complete(&host, "gitlab", &attempt, 400).await;
    assert!(matches!(status, AttemptStatus::Complete { .. }), "{status:?}");

    // Request shapes: authorization_code + verifier + fixed redirect +
    // client id; S256(verifier) == the authorize challenge.
    let requests = mock.requests().join("\n");
    assert!(requests.contains("grant_type=authorization_code"), "{requests}");
    assert!(requests.contains("code=gl-code"), "{requests}");
    assert!(requests.contains("code_verifier="), "{requests}");
    assert!(
        requests.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8080%2Fcallback"),
        "{requests}"
    );
    assert!(requests.contains("client_id=test-client-id"), "{requests}");
    let verifier = requests
        .lines()
        .find_map(|line| line.split("code_verifier=").nth(1)?.split('&').next().map(String::from))
        .expect("verifier");
    let challenge = parse_query_param(&attempt.url, "code_challenge").expect("challenge");
    let recomputed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    assert_eq!(recomputed, challenge, "PKCE S256 consistency");

    // Credential: instance + client recorded; label = instance host.
    let info = host.get("gitlab").await.expect("get").expect("present");
    let ConnectionInfo::Credential { label, .. } = &info.connections[0] else { panic!() };
    assert_eq!(label, "127.0.0.1");
    let active = host.connection_active("gitlab").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "gl-access");
    assert_eq!(oauth.metadata.as_ref().unwrap()["clientID"], "test-client-id");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("GITLAB_OAUTH_CLIENT_ID");
    };
}

#[tokio::test]
async fn gitlab_bundled_client_rejected_off_gitlab_com() {
    let _env = env_guard().await;
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("GITLAB_OAUTH_CLIENT_ID");
    };
    let host = host().await;
    let error = host
        .oauth_connect(
            "gitlab",
            "pkce",
            &answer(&[("instanceUrl", "https://gitlab.example.corp")]),
            None,
        )
        .await
        .expect_err("non-gitlab.com rejected");
    assert!(error.to_string().contains("bundled GitLab OAuth application"), "{error}");
}

#[tokio::test]
async fn gitlab_refresh_rotates_via_mock() {
    let mock = MockOAuth::start(vec![(
        "/oauth/token",
        vec![json!({"access_token": "gl-rotated", "refresh_token": "gl-rotated-refresh", "expires_in": 7200})],
    )])
    .await;
    let host = host().await;
    let expiring = CredentialValue::OAuth(OAuthValue {
        method_id: "pkce".into(),
        refresh: "gl-old-refresh".into(),
        access: "gl-old".into(),
        expires: crate::database::migrations::now_ms() - 1000,
        metadata: Some(BTreeMap::from([
            ("instanceUrl".to_owned(), Value::String(mock.base.clone())),
            ("clientID".to_owned(), Value::String("legacy-client".into())),
        ])),
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "gitlab",
            label: Some("expiring"),
            value: &expiring,
            activate: None,
        })
        .await
        .expect("create");
    let active = host.connection_active("gitlab").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "gl-rotated");
    let requests = mock.requests().join("\n");
    assert!(requests.contains("grant_type=refresh_token"), "{requests}");
    assert!(requests.contains("refresh_token=gl-old-refresh"), "{requests}");
    assert!(requests.contains("client_id=legacy-client"), "{requests}");
}

// -- BE-3b: azure CLI ---------------------------------------------------------------

#[tokio::test]
async fn azure_cli_flow_with_fake_az() {
    // A fake `az` on PATH (v2's endpoint-injection analog).
    let dir = tempfile::TempDir::new().expect("tempdir");
    let bin = dir.path().join("az");
    let expires_on = crate::database::migrations::now_ms() / 1000 + 3600;
    std::fs::write(
        &bin,
        format!("#!/bin/sh\necho '{{\"accessToken\":\"az-token\",\"expires_on\":{expires_on}}}'"),
    )
    .expect("write az");
    #[allow(unsafe_code)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let old_path = std::env::var("PATH").unwrap_or_default();
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("PATH", format!("{}:{old_path}", dir.path().display()));
    };
    let host = host().await;
    let info = host.get("azure").await.expect("get").expect("present");
    assert!(
        info.methods
            .iter()
            .any(|method| matches!(method, Method::OAuth { id, .. } if id == "azure-cli")),
        "cli method registered"
    );

    let attempt = host
        .oauth_connect("azure", "azure-cli", &answer(&[("resourceName", "my-models")]), None)
        .await
        .expect("connect");
    assert_eq!(attempt.url, "");
    assert_eq!(attempt.instructions, "Sign in with `az login` before continuing.");
    let status = wait_complete(&host, "azure", &attempt, 200).await;
    assert!(matches!(status, AttemptStatus::Complete { .. }), "{status:?}");
    let active = host.connection_active("azure").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "az-token");
    assert_eq!(oauth.refresh, "azure-cli");
    assert_eq!(oauth.metadata.as_ref().unwrap()["resourceName"], "my-models");
    assert!(oauth.expires > crate::database::migrations::now_ms());

    // PATH without `az`: the CLI method is absent (azure.ts:124).
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("PATH", "/nonexistent");
    };
    let bare_host = host_with_bus().await.0;
    let info = bare_host.get("azure").await.expect("get 2").expect("present");
    assert!(
        !info.methods.iter().any(|method| matches!(method, Method::OAuth { .. })),
        "no cli method without az"
    );
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("PATH", old_path);
    };
}

// -- BE-3b: snowflake ---------------------------------------------------------------

#[tokio::test]
async fn snowflake_token_exchange_against_mock() {
    use base64::Engine;
    let mock = MockOAuth::start(vec![(
        "/oauth/token-request",
        vec![json!({"access_token": "sf-access", "expires_in": 600})],
    )])
    .await;
    // No refresh_token in the response → falls back to the form value
    // (snowflake.ts:68).
    let tokens = super::providers_enterprise::snowflake_token_at(
        &mock.base,
        "myorg-myaccount",
        vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", "sf-code".into()),
            ("code_verifier", "sf-verifier".into()),
            ("refresh_token", "sf-refresh".into()),
        ],
    )
    .await
    .expect("token");
    assert_eq!(tokens.access, "sf-access");
    assert_eq!(tokens.refresh, "sf-refresh");
    assert_eq!(tokens.metadata.as_ref().unwrap()["account"], "myorg-myaccount");
    let requests = mock.requests().join("\n");
    assert!(requests.contains("client_id=LOCAL_APPLICATION"), "{requests}");
    assert!(requests.contains("grant_type=authorization_code"), "{requests}");
    // Basic LOCAL_APPLICATION:LOCAL_APPLICATION (snowflake.ts:57-58).
    let expected =
        base64::engine::general_purpose::STANDARD.encode("LOCAL_APPLICATION:LOCAL_APPLICATION");
    let lowered = requests.to_ascii_lowercase();
    assert!(
        lowered.contains("authorization: basic ")
            && requests.contains(&format!("Basic {expected}")),
        "{requests}"
    );
}

#[tokio::test]
async fn snowflake_account_normalization_and_scope() {
    use super::providers_enterprise::snowflake_account;
    assert_eq!(
        snowflake_account(Some("https://myorg-myaccount.snowflakecomputing.com/")),
        "myorg-myaccount"
    );
    assert_eq!(snowflake_account(Some("myorg.snowflakecomputing.com")), "myorg");
    assert_eq!(snowflake_account(Some("  plain-account ")), "plain-account");
    assert_eq!(snowflake_account(None), "");

    // Scope encoding (snowflake.ts:151-155): simple role, encoded role.
    let host = host().await;
    let simple = host
        .oauth_connect(
            "snowflake-cortex",
            "browser",
            &answer(&[("account", "acct"), ("role", "analyst")]),
            None,
        )
        .await
        .expect("connect");
    assert!(
        simple.url.contains("scope=refresh_token%20session%3Arole%3Aanalyst"),
        "{}",
        simple.url
    );
    host.oauth_cancel("snowflake-cortex", &simple.attempt_id).await;
    let encoded = host
        .oauth_connect(
            "snowflake-cortex",
            "browser",
            &answer(&[("account", "acct"), ("role", "data eng")]),
            None,
        )
        .await
        .expect("connect 2");
    assert!(encoded.url.contains("session%3Arole-encoded%3Adata%2520eng"), "{}", encoded.url);
    host.oauth_cancel("snowflake-cortex", &encoded.attempt_id).await;
}

// -- BE-3b: openai codex ---------------------------------------------------------------

#[tokio::test]
async fn openai_exchange_extracts_account_from_id_token_claims() {
    use base64::Engine;
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"chatgpt_account_id":"acct-9","exp":9999999999}"#);
    let id_token = format!("{header}.{payload}.sig");
    let mock = MockOAuth::start(vec![(
        "/oauth/token",
        vec![json!({"access_token": "oi-access", "refresh_token": "oi-refresh", "id_token": id_token, "expires_in": 3600})],
    )])
    .await;
    let (access, refresh, returned_id, expires_in) =
        super::providers_enterprise::openai_exchange_at(
            &mock.base,
            vec![
                ("grant_type", "authorization_code".to_owned()),
                ("code", "oi-code".into()),
                ("client_id", "app_EMoamEEZ73f0CkXaXp7hrann".into()),
                ("code_verifier", "oi-verifier".into()),
            ],
        )
        .await
        .expect("exchange");
    #[allow(clippy::cast_possible_truncation)]
    let expires = expires_in as i64;
    assert_eq!((access.as_str(), refresh.as_str(), expires), ("oi-access", "oi-refresh", 3600));
    assert!(returned_id.contains("acct-9") || !returned_id.is_empty());
    let requests = mock.requests().join("\n");
    assert!(requests.contains("grant_type=authorization_code"), "{requests}");
    // The claim extraction (openai.ts:415-430) — via the credential builder.
    let credential = super::providers_enterprise::openai_credential(
        "chatgpt-browser",
        access,
        refresh,
        &returned_id,
        expires_in,
    );
    let CredentialValue::OAuth(oauth) = credential else { panic!() };
    assert_eq!(oauth.metadata.as_ref().unwrap()["accountID"], "acct-9");
}

#[tokio::test]
async fn openai_headless_flow_against_mock() {
    let mock = MockOAuth::start(vec![
        (
            "/api/accounts/deviceauth/usercode",
            vec![json!({"device_auth_id": "da-1", "user_code": "ZZZZ-9999", "interval": "1"})],
        ),
        (
            "/api/accounts/deviceauth/token",
            vec![json!({"authorization_code": "oi-auth-code", "code_verifier": "server-verifier"})],
        ),
        (
            "/oauth/token",
            vec![json!({"access_token": "oi-headless", "refresh_token": "oi-refresh-2", "expires_in": 3600})],
        ),
    ])
    .await;
    let authorization =
        super::providers_enterprise::openai_headless_authorize_at(BTreeMap::new(), &mock.base)
            .await
            .expect("authorize");
    assert_eq!(authorization.url, format!("{}/codex/device", mock.base));
    assert_eq!(authorization.instructions, "Enter code: ZZZZ-9999");
    let AuthMode::Auto(callback) = authorization.mode else { panic!("auto") };
    let value = callback.await.expect("callback");
    let CredentialValue::OAuth(oauth) = value else { panic!() };
    assert_eq!(oauth.access, "oi-headless");
    assert_eq!(oauth.method_id, "chatgpt-headless");
    let requests = mock.requests().join("\n");
    assert!(requests.contains("deviceauth/usercode"), "{requests}");
    assert!(
        requests.contains("device_auth_id=da-1") || requests.contains("device_auth_id"),
        "{requests}"
    );
    assert!(requests.contains("code_verifier=server-verifier"), "{requests}");
    assert!(requests.contains("redirect_uri="), "{requests}");
}

#[tokio::test]
async fn listen_pinned_falls_back_when_primary_busy() {
    let primary_holder = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("primary");
    let primary = primary_holder.local_addr().expect("addr").port();
    let fallback_listener =
        tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("fallback probe");
    let fallback = fallback_listener.local_addr().expect("addr").port();
    drop(fallback_listener);
    let listener = super::flows::listen_pinned(
        primary,
        Some(fallback),
        |port| super::flows::LoopbackOptions {
            port,
            host: "127.0.0.1".into(),
            callback_path: "/cb",
            page: super::flows::CallbackPage::Html(super::flows::success_page("T")),
        },
        3,
        std::time::Duration::from_millis(20),
        |p, f| format!("conflict {p}/{f}"),
    )
    .await
    .expect("fallback bind");
    assert_eq!(listener.port, fallback);
    let _ = primary_holder;
}

// -- BE-3b: chatgpt connector ---------------------------------------------------------------

fn fake_jwt(claims: &str) -> String {
    use base64::Engine;
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.as_bytes());
    format!("{header}.{payload}.sig")
}

/// A slot-valued mock: the response is set AFTER authorize produced its
/// nonce (the `id_token` must echo it).
struct SlotMock {
    base: String,
    slot: Arc<parking_lot::Mutex<Option<(u16, Value)>>>,
    log: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl SlotMock {
    async fn start() -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let slot: Arc<parking_lot::Mutex<Option<(u16, Value)>>> = Arc::default();
        let log: Arc<parking_lot::Mutex<Vec<String>>> = Arc::default();
        let (slot_handle, log_handle) = (Arc::clone(&slot), Arc::clone(&log));
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let body_start = request.find("\r\n\r\n").map_or(read, |index| index + 4);
                log_handle.lock().push(format!("{path} {}", &request[body_start..]));
                let (status, body) = slot_handle.lock().clone().unwrap_or((404, Value::Null));
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), slot, log }
    }
}

#[tokio::test]
async fn chatgpt_connector_round_trip_against_mock() {
    let mock = SlotMock::start().await;
    // authorize binds the loopback; the token URL points at the mock.
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
    )
    .await
    .expect("authorize");
    // The authorize URL carries the dynamic-client registration params
    // (chatgpt.ts:498-519).
    assert!(authorization.url.contains("client_id=dynamic_agent_client"), "{}", authorization.url);
    assert!(authorization.url.contains("agent_name_hint=OpenCode"), "{}", authorization.url);
    assert!(authorization.url.contains("nonce="), "{}", authorization.url);
    assert!(authorization.url.contains("chatgpt.tokens.use.direct"), "{}", authorization.url);
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let nonce = parse_query_param(&authorization.url, "nonce").expect("nonce");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");

    // The id_token must echo the attempt nonce; aud = the client the
    // callback will carry (chatgpt.ts:165-171,439-461 claims subset).
    let id_token = fake_jwt(&format!(
        r#"{{"iss":"https://auth.openai.com","aud":"client_reg_1","exp":9999999999,"nonce":"{nonce}","sub":"user|1"}}"#
    ));
    *mock.slot.lock() = Some((
        200,
        json!({
            "access_token": "cg-access",
            "refresh_token": "cg-refresh",
            "id_token": id_token,
            "scope": "openid profile email offline_access chatgpt.tokens.use.direct",
            "expires_in": 3600
        }),
    ));

    // Act as the browser: redirect with code + the registered client id.
    let callback =
        reqwest::get(format!("{redirect}?code=cg-code&state={state}&client_id=client_reg_1"))
            .await
            .expect("callback");
    assert_eq!(callback.status(), 200);

    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto") };
    let value = callback_future.await.expect("callback value");
    let CredentialValue::OAuth(oauth) = value else { panic!() };
    assert_eq!(oauth.access, "cg-access");
    assert_eq!(oauth.method_id, "chatgpt-token-sharing");
    let metadata = oauth.metadata.expect("metadata");
    assert_eq!(metadata["clientID"], "client_reg_1");
    assert!(metadata["scopes"]
        .as_array()
        .is_some_and(|scopes| scopes.iter().any(|scope| scope == "chatgpt.tokens.use.direct")));

    // Exchange request shape (chatgpt.ts:353-366).
    let requests = mock.log.lock().join("\n");
    assert!(requests.contains("grant_type=authorization_code"), "{requests}");
    assert!(requests.contains("client_id=client_reg_1"), "{requests}");
    assert!(requests.contains("code_verifier="), "{requests}");
    assert!(requests.contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"), "{requests}");
}

#[tokio::test]
async fn chatgpt_connector_rejects_missing_token_sharing_scope() {
    let mock = SlotMock::start().await;
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
    )
    .await
    .expect("authorize");
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let nonce = parse_query_param(&authorization.url, "nonce").expect("nonce");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");
    let id_token = fake_jwt(&format!(
        r#"{{"iss":"https://auth.openai.com","aud":"client_reg_1","exp":9999999999,"nonce":"{nonce}","sub":"user|1"}}"#
    ));
    *mock.slot.lock() = Some((
        200,
        json!({
            "access_token": "cg-access",
            "refresh_token": "cg-refresh",
            "id_token": id_token,
            "scope": "openid profile email",
            "expires_in": 3600
        }),
    ));
    let _ =
        reqwest::get(format!("{redirect}?code=cg-code&state={state}&client_id=client_reg_1")).await;
    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto") };
    let error = callback_future.await.expect_err("missing sharing scope");
    assert!(error.contains("without token sharing"), "{error}");
}

#[tokio::test]
async fn chatgpt_connector_rejects_nonce_mismatch() {
    let mock = SlotMock::start().await;
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
    )
    .await
    .expect("authorize");
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");
    // The id_token echoes the WRONG nonce (chatgpt.ts:449-450).
    let id_token = fake_jwt(
        r#"{"iss":"https://auth.openai.com","aud":"client_reg_1","exp":9999999999,"nonce":"attacker-nonce","sub":"user|1"}"#,
    );
    *mock.slot.lock() = Some((
        200,
        json!({
            "access_token": "cg-access",
            "refresh_token": "cg-refresh",
            "id_token": id_token,
            "scope": "openid offline_access chatgpt.tokens.use.direct",
            "expires_in": 3600
        }),
    ));
    let _ =
        reqwest::get(format!("{redirect}?code=cg-code&state={state}&client_id=client_reg_1")).await;
    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto") };
    let error = callback_future.await.expect_err("nonce mismatch");
    assert!(error.contains("nonce"), "{error}");
}

// -- BE-3b: copilot transport binding -------------------------------------------

#[tokio::test]
async fn copilot_binding_resolves_token_base_url_and_headers() {
    let host = host().await;
    // Enterprise credential with an explicit api endpoint (BE-3 metadata).
    let value = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "gho_transport".into(),
        access: "gho_transport".into(),
        expires: 0,
        metadata: Some(BTreeMap::from([(
            "apiEndpoint".to_owned(),
            Value::String("https://api.githubcopilot.com".into()),
        )])),
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "github-copilot",
            label: Some("copilot"),
            value: &value,
            activate: None,
        })
        .await
        .expect("create");
    host.refresh_provider_bindings().await;
    let bindings = host.credential_bindings();
    let binding = bindings.get("github-copilot").expect("copilot binding");
    assert_eq!(binding.api_key, "gho_transport");
    assert_eq!(binding.base_url.as_deref(), Some("https://api.githubcopilot.com"));
    assert_eq!(binding.headers.get("X-GitHub-Api-Version").map(String::as_str), Some("2026-08-01"));
    assert_eq!(
        binding.headers.get("Openai-Intent").map(String::as_str),
        Some("conversation-edits")
    );

    // Removing the credential clears the binding on refresh.
    let ConnectionInfo::Credential { id, .. } =
        host.connection_active("github-copilot").await.expect("active").expect("some")
    else {
        panic!()
    };
    host.store().remove(&id).await.expect("remove");
    host.refresh_provider_bindings().await;
    assert!(bindings.get("github-copilot").is_none());
}

#[tokio::test]
async fn copilot_base_url_prefers_api_endpoint_then_enterprise() {
    let endpoint = OAuthValue {
        method_id: "device".into(),
        refresh: "t".into(),
        access: "t".into(),
        expires: 0,
        metadata: Some(BTreeMap::from([
            ("apiEndpoint".to_owned(), Value::String("https://copilot.example/api".into())),
            ("enterpriseUrl".to_owned(), Value::String("company.ghe.com".into())),
        ])),
    };
    assert_eq!(super::copilot::copilot_base_url(&endpoint), "https://copilot.example/api");
    let enterprise = OAuthValue {
        method_id: "device".into(),
        refresh: "t".into(),
        access: "t".into(),
        expires: 0,
        metadata: Some(BTreeMap::from([(
            "enterpriseUrl".to_owned(),
            Value::String("company.ghe.com".into()),
        )])),
    };
    assert_eq!(
        super::copilot::copilot_base_url(&enterprise),
        "https://copilot-api.company.ghe.com"
    );
    let origin = OAuthValue {
        method_id: "device".into(),
        refresh: "t".into(),
        access: "t".into(),
        expires: 0,
        metadata: Some(BTreeMap::from([(
            "enterpriseUrl".to_owned(),
            Value::String("http://127.0.0.1:9/".into()),
        )])),
    };
    assert_eq!(super::copilot::copilot_base_url(&origin), "http://127.0.0.1:9");
    let plain = OAuthValue {
        method_id: "device".into(),
        refresh: "t".into(),
        access: "t".into(),
        expires: 0,
        metadata: None,
    };
    assert_eq!(super::copilot::copilot_base_url(&plain), "https://api.githubcopilot.com");
}

#[tokio::test]
async fn copilot_resolver_binds_route_from_credential() {
    use crate::engine::resolver::CatalogModelResolver;
    use crate::provider_catalog::facade::FacadeContext;
    let host = host().await;
    let value = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "gho_route".into(),
        access: "gho_route".into(),
        expires: 0,
        metadata: Some(BTreeMap::from([(
            "apiEndpoint".to_owned(),
            Value::String("https://api.githubcopilot.com".into()),
        )])),
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "github-copilot",
            label: Some("copilot"),
            value: &value,
            activate: None,
        })
        .await
        .expect("create");
    host.refresh_provider_bindings().await;

    let catalog_text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/provider_catalog/models-dev-snapshot.txt"
    ))
    .expect("snapshot");
    let catalog = std::sync::Arc::new(
        crate::provider_catalog::catalog::Catalog::parse(&catalog_text).expect("catalog"),
    );
    let copilot = catalog.provider("github-copilot").expect("copilot in catalog");
    let model_id = copilot.models.first().expect("copilot has models").id.0.clone();
    let transport: oc_ai::route::SharedTransport =
        std::sync::Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    )
    .with_credentials(std::sync::Arc::clone(host.credential_bindings()));

    // Without the credential the openai-compatible arm fails (no baseURL);
    // with it the route binds (facade.rs build_route OpenAICompatible).
    let bound = resolver.bind(&oc_schema::model::Ref {
        provider_id: oc_schema::id::ProviderId::parse("github-copilot".to_owned()),
        id: oc_schema::id::ModelId::parse(model_id),
        variant: None,
    });
    let (model, _info, _canonical) = bound.expect("copilot route should bind from the credential");
    // BE-4: the binding now lands on the dedicated Copilot chat route with
    // the credential token and client headers.
    assert_eq!(model.route.id, "copilot-chat");
    let route = std::sync::Arc::clone(&model.route);
    let mut request = oc_ai::schema::LLMRequest::new(model);
    request.messages = vec![oc_ai::schema::Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(transport_request.header("authorization"), Some("Bearer gho_route"));
    assert_eq!(transport_request.header("X-GitHub-Api-Version"), Some("2026-08-01"));
}

// -- BE-3b: console ingestion + status reporters (items 4+5) -----------------------

#[tokio::test]
async fn console_config_fetch_and_provider_projection() {
    let mock = MockOAuth::start(vec![(
        "/api/v2/config",
        vec![json!({
            "providers": {
                "acme-ai": {
                    "name": "Acme AI",
                    "settings": { "baseURL": "https://gw.acme.test/v1", "apiKey": "secret-never" },
                    "headers": { "x-acme-team": "platform" },
                    "models": {
                        "acme-large": { "name": "Acme Large" },
                        "acme-hidden": { "name": "Hidden", "disabled": true }
                    }
                }
            }
        })],
    )])
    .await;
    let credential = OAuthValue {
        method_id: "device".into(),
        refresh: "r".into(),
        access: "console-access".into(),
        expires: crate::database::migrations::now_ms() + 3_600_000,
        metadata: Some(BTreeMap::from([
            ("server".to_owned(), Value::String(mock.base.clone())),
            ("orgID".to_owned(), Value::String("org_1".into())),
        ])),
    };
    let config = super::console::fetch_config(&credential).await.expect("fetch").expect("some");
    let providers = super::console::to_custom_providers(&config);
    assert_eq!(providers.len(), 1);
    let provider = &providers[0];
    assert_eq!(provider.id, "acme-ai");
    assert_eq!(provider.name, "Acme AI");
    assert_eq!(provider.base_url, "https://gw.acme.test/v1");
    let headers = provider.headers.clone().unwrap_or_default();
    assert_eq!(headers.get("x-acme-team").map(String::as_str), Some("platform"));
    // Disabled models drop; enabled models carry their names.
    assert_eq!(provider.models.len(), 1);
    assert_eq!(provider.models[0].id, "acme-large");
    assert_eq!(provider.models[0].name.as_deref(), Some("Acme Large"));
    // Request shape: bearer + org header against the metadata server.
    let requests = mock.requests().join("\n").to_ascii_lowercase();
    assert!(requests.contains("authorization: bearer console-access"), "{requests}");
    assert!(requests.contains("x-org-id: org_1"), "{requests}");
    // Credentials never ride provider settings (withoutCredentials).
    assert!(provider.body.is_none());
}

#[tokio::test]
async fn console_refresh_reports_sso_required_status() {
    let mock = MockOAuth::start_with_status(vec![(
        "/api/v2/config",
        vec![(403, json!({ "_tag": "SsoRequired", "orgId": "org_9", "connectionId": "conn_42" }))],
    )])
    .await;
    let host = host().await;
    let value = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "r".into(),
        access: "console-access".into(),
        expires: crate::database::migrations::now_ms() + 3_600_000,
        metadata: Some(BTreeMap::from([("server".to_owned(), Value::String(mock.base.clone()))])),
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "opencode",
            label: Some("org"),
            value: &value,
            activate: None,
        })
        .await
        .expect("create");
    host.refresh_console().await;
    let info = host.get("opencode").await.expect("get").expect("present");
    let ConnectionInfo::Credential { status: Some(status), .. } = &info.connections[0] else {
        panic!("expected a reported status, got {:?}", info.connections)
    };
    assert_eq!(status.status, "needs_auth");
    assert!(status.message.contains("Sign in with SSO"), "{}", status.message);
    let expected_url = format!("{}/auth/sso/conn_42/start?redirectTo=%2F", mock.base);
    assert_eq!(status.url.as_deref(), Some(expected_url.as_str()));
}

#[tokio::test]
async fn console_refresh_reports_signed_out_and_clears_on_success() {
    let mock = MockOAuth::start_with_status(vec![(
        "/api/v2/config",
        vec![(401, json!({ "_tag": "Unauthorized" }))],
    )])
    .await;
    let host = host().await;
    let value = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "r".into(),
        access: "console-access".into(),
        expires: crate::database::migrations::now_ms() + 3_600_000,
        metadata: Some(BTreeMap::from([("server".to_owned(), Value::String(mock.base.clone()))])),
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: "opencode",
            label: Some("org"),
            value: &value,
            activate: None,
        })
        .await
        .expect("create");
    host.refresh_console().await;
    let info = host.get("opencode").await.expect("get").expect("present");
    let ConnectionInfo::Credential { status: Some(status), .. } = &info.connections[0] else {
        panic!("expected status")
    };
    assert!(status.message.contains("Reconnect OpenCode Console"), "{}", status.message);

    // A healthy 404 (no console config) clears the status (item 5).
    let healthy =
        MockOAuth::start_with_status(vec![("/api/v2/config", vec![(404, Value::Null)])]).await;
    let cleared = CredentialValue::OAuth(OAuthValue {
        method_id: "device".into(),
        refresh: "r".into(),
        access: "console-access".into(),
        expires: crate::database::migrations::now_ms() + 3_600_000,
        metadata: Some(BTreeMap::from([(
            "server".to_owned(),
            Value::String(healthy.base.clone()),
        )])),
    });
    let ConnectionInfo::Credential { id, .. } =
        host.connection_active("opencode").await.expect("active").expect("some")
    else {
        panic!()
    };
    host.store().update(&id, None, Some(&cleared)).await.expect("update");
    host.refresh_console().await;
    let info = host.get("opencode").await.expect("get 2").expect("present");
    let ConnectionInfo::Credential { status, .. } = &info.connections[0] else { panic!() };
    assert!(status.is_none(), "cleared: {status:?}");
}

#[tokio::test]
async fn console_provider_resolves_through_the_snapshot() {
    use crate::engine::resolver::CatalogModelResolver;
    use crate::provider_catalog::facade::FacadeContext;
    let host = host().await;
    let config = serde_json::from_value::<super::console::ConsoleConfig>(json!({
        "providers": {
            "acme-ai": {
                "settings": { "baseURL": "https://gw.acme.test/v1" },
                "models": { "acme-large": {} }
            }
        }
    }))
    .expect("config");
    host.console_providers().swap_for_test(super::console::to_custom_providers(&config));
    let catalog_text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/provider_catalog/models-dev-snapshot.txt"
    ))
    .expect("snapshot");
    let catalog = std::sync::Arc::new(
        crate::provider_catalog::catalog::Catalog::parse(&catalog_text).expect("catalog"),
    );
    let transport: oc_ai::route::SharedTransport =
        std::sync::Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    )
    .with_console(std::sync::Arc::clone(host.console_providers()));
    resolver
        .bind(&oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("acme-ai".to_owned()),
            id: oc_schema::id::ModelId::parse("acme-large".to_owned()),
            variant: None,
        })
        .expect("console provider binds");
}

// -- BE-3b: wellknown config resolution (item 3) ---------------------------------

#[tokio::test]
async fn wellknown_documents_resolve_with_credential_variables() {
    use crate::config::WellknownSource as _;
    // Manifest server: auth env + static config + a remote_config whose
    // URL embeds the credential token via {env:ACME_TOKEN}.
    let remote = MockOAuth::start(vec![(
        "/BE3B_SECRET_TOKEN/config.json",
        vec![json!({ "acme-remote": { "npm": "@acme/remote" } })],
    )])
    .await;
    let remote_url = format!("{}/{{env:ACME_TOKEN}}/config.json", remote.base);
    let manifest = json!({
        "auth": { "command": ["echo", "key"], "env": "ACME_TOKEN" },
        "config": { "acme": { "npm": "@acme/sdk", "models": {} } },
        "remote_config": { "url": remote_url }
    });
    let manifest_server = MockOAuth::start(vec![("/.well-known/opencode", vec![manifest])]).await;

    let host = host().await;
    let origin = manifest_server.base.clone();
    host.database()
        .transaction(move |tx| {
            tx.kv_set(
                super::wellknown_config::WELLKNOWN_SOURCES_KEY,
                &Value::Array(vec![Value::String(origin.clone())]),
            )
        })
        .await
        .expect("kv set");
    let key = CredentialValue::Key(super::types::KeyValue {
        key: "BE3B_SECRET_TOKEN".into(),
        metadata: None,
        configuration: None,
    });
    host.store()
        .create(credential::CreateInput {
            id: None,
            integration_id: &manifest_server.base,
            label: Some("acme"),
            value: &key,
            activate: None,
        })
        .await
        .expect("create");

    host.refresh_wellknown_documents().await;
    let source = super::wellknown_config::HostWellknownSource::new(std::sync::Arc::clone(
        host.wellknown_documents(),
    ));
    let documents = source.documents(&|_| None);
    assert_eq!(documents.len(), 2, "static config + remote config");
    let joined =
        documents.iter().map(|document| document.text.as_str()).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("\"acme\""), "{joined}");
    assert!(joined.contains("\"acme-remote\""), "{joined}");
    // The credential variable is exposed to the loader for substitution.
    assert!(documents
        .iter()
        .all(|document| document.env.get("ACME_TOKEN") == Some(&"BE3B_SECRET_TOKEN".to_owned())));
    // The remote fetch carried the substituted token in its URL.
    let requests = remote.requests().join("\n");
    assert!(requests.contains("/BE3B_SECRET_TOKEN/config.json"), "{requests}");
}

#[tokio::test]
async fn wellknown_documents_skip_sources_without_key_credentials() {
    use crate::config::WellknownSource as _;
    let mock = MockOAuth::start(vec![(
        "/.well-known/opencode",
        vec![json!({ "auth": { "command": ["echo", "x"], "env": "T" }, "config": { "p": {} } })],
    )])
    .await;
    let host = host().await;
    let origin = mock.base.clone();
    host.database()
        .transaction(move |tx| {
            tx.kv_set(
                super::wellknown_config::WELLKNOWN_SOURCES_KEY,
                &Value::Array(vec![Value::String(origin.clone())]),
            )
        })
        .await
        .expect("kv set");
    // No credential stored → no documents.
    host.refresh_wellknown_documents().await;
    let source = super::wellknown_config::HostWellknownSource::new(std::sync::Arc::clone(
        host.wellknown_documents(),
    ));
    assert!(source.documents(&|_| None).is_empty());
}

#[tokio::test]
async fn wellknown_documents_feed_the_config_loader() {
    use crate::config::Loader;
    let host = host().await;
    let documents = crate::config::WellknownDocument {
        source: "https://acme.test".into(),
        dir: "https://acme.test".into(),
        text: r#"{"provider":{"acme":{"name":"Acme","npm":"@acme/sdk"}}}"#.into(),
        env: BTreeMap::new(),
    };
    host.wellknown_documents().push_for_test(vec![documents]);
    let dir = tempfile::TempDir::new().expect("tempdir");
    let loader = Loader::new(
        dir.path().to_path_buf(),
        crate::config::GlobalDirs {
            config: dir.path().join("cfg"),
            home: dir.path().to_path_buf(),
        },
    )
    .with_wellknown(std::sync::Arc::new(super::wellknown_config::HostWellknownSource::new(
        std::sync::Arc::clone(host.wellknown_documents()),
    )));
    let snapshot = loader.load();
    let providers = snapshot
        .entries
        .iter()
        .filter_map(|entry| match entry {
            oc_schema::config::Entry::Document(document) => Some(&document.info),
            oc_schema::config::Entry::Directory(_) => None,
        })
        .filter_map(|info| info.providers.as_ref())
        .flat_map(|providers| providers.keys())
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert!(
        providers.contains(&"acme"),
        "wellknown provider reached the config entries: {providers:?}"
    );
}

// -- BE-4: Vertex ADC credential binding → resolver route (item 2) --------

#[tokio::test]
async fn vertex_adc_binding_refreshes_and_binds_the_native_route() {
    use crate::engine::resolver::CatalogModelResolver;
    use crate::provider_catalog::facade::FacadeContext;

    let _env = env_guard().await;
    // 1. an authorized-user ADC file in a temp HOME + explicit
    //    GOOGLE_APPLICATION_CREDENTIALS
    let dir = tempfile::TempDir::new().expect("tempdir");
    let adc_path = dir.path().join("adc.json");
    std::fs::write(
        &adc_path,
        serde_json::json!({
            "type": "authorized_user",
            "client_id": "id.apps.googleusercontent.com",
            "client_secret": "secret",
            "refresh_token": "refresh-grant",
        })
        .to_string(),
    )
    .expect("write adc");
    std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", &adc_path);

    // 2. the token endpoint replayed through the mock
    let mock = MockOAuth::start(vec![(
        "/token",
        vec![json!({ "access_token": "ya29 refreshed", "expires_in": 3600 })],
    )])
    .await;
    let host = host().await;
    let binding = host
        .vertex_binding_with_token_url(&format!("{}/token", mock.base))
        .await
        .expect("ADC binding resolves");
    assert_eq!(binding.api_key, "ya29 refreshed");
    assert!(binding.base_url.is_none());

    // 3. the binding rides the shared snapshot into the resolver and the
    //    native vertex family binds with the token as the bearer
    host.credential_bindings()
        .swap(std::collections::BTreeMap::from([("google-vertex".to_owned(), binding)]));
    let catalog_text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/provider_catalog/models-dev-snapshot.txt"
    ))
    .expect("snapshot");
    let catalog = std::sync::Arc::new(
        crate::provider_catalog::catalog::Catalog::parse(&catalog_text).expect("catalog"),
    );
    let native_model_id = {
        let vertex = catalog.provider("google-vertex").expect("vertex in catalog");
        let native = vertex
            .models
            .iter()
            .find(|model| {
                model.package.as_deref().unwrap_or(&vertex.info.package)
                    == "@opencode/ai/providers/google-vertex"
            })
            .expect("a native-package vertex model");
        native.id.0.clone()
    };
    let transport: oc_ai::route::SharedTransport =
        std::sync::Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    )
    .with_credentials(std::sync::Arc::clone(host.credential_bindings()));
    let model_id = oc_schema::id::ModelId::parse(native_model_id);
    let (model, _info, _canonical) = resolver
        .bind(&oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("google-vertex".to_owned()),
            id: model_id,
            variant: None,
        })
        .expect("vertex native binds from the ADC credential");
    assert_eq!(model.route.id, "google-vertex-gemini");
    let route = std::sync::Arc::clone(&model.route);
    let mut request = oc_ai::schema::LLMRequest::new(model);
    request.messages = vec![oc_ai::schema::Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(transport_request.header("authorization"), Some("Bearer ya29 refreshed"));
    assert!(transport_request.url.contains(":streamGenerateContent?alt=sse"));
    std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
}

#[tokio::test]
async fn vertex_service_account_adc_defers_to_env_adc() {
    let _env = env_guard().await;
    let dir = tempfile::TempDir::new().expect("tempdir");
    let adc_path = dir.path().join("sa.json");
    std::fs::write(
        &adc_path,
        serde_json::json!({ "type": "service_account", "private_key": "…" }).to_string(),
    )
    .expect("write sa");
    std::env::set_var("GOOGLE_APPLICATION_CREDENTIALS", &adc_path);
    let host = host().await;
    // RS256 self-signed JWT grant is the registered remainder: no binding,
    // request-time GOOGLE_OAUTH_ACCESS_TOKEN env ADC still applies.
    assert!(host.vertex_binding().await.is_none());
    std::env::remove_var("GOOGLE_APPLICATION_CREDENTIALS");
}
