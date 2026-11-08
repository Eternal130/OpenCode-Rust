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

/// G-234② — the two gitlab tests pin loopback 8080 and nextest runs
/// every test in its OWN process, so the in-process ENV_TEST_LOCK
/// cannot serialize their fixed-port listeners. They contend through
/// flock(2) instead (the lock auto-releases when a process dies).
async fn gitlab_port_lock() -> std::fs::File {
    use std::os::fd::AsRawFd as _;
    let dir = std::path::Path::new("/tmp/opencode");
    let _ = std::fs::create_dir_all(dir);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .read(true)
        .open(dir.join("gb10-gitlab-8080.lock"))
        .expect("lock file");
    loop {
        #[allow(unsafe_code)] // contained flock(2) on the lock file above
        let held =
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if held {
            return file;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
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
                        lowered.starts_with("authorization:")
                            || lowered.starts_with("x-org-id:")
                            || lowered.starts_with("accept:")
                            || lowered.starts_with("user-agent:")
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
        (
            "/api/user",
            vec![json!({"id": "user_77", "email": "dev@acme.test"})],
        ),
        (
            "/api/orgs",
            vec![json!([
                {"id": "org_1", "name": "Zeta Corp"},
                {"id": "org_9", "name": "Acme Corp"}
            ])],
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

    // Request shapes (opencode.ts:107-113, 567-590, G-244): the device
    // and poll posts carry JSON bodies — client_id + org scope on the
    // device request (supports_org_scope a boolean), the device grant on
    // the poll.
    let requests = mock.requests().join("\n");
    assert!(requests.contains("\"client_id\":\"opencode-cli\""), "{requests}");
    assert!(requests.contains("\"supports_org_scope\":true"), "{requests}");
    assert!(requests.contains("\"grant_type\":\"urn:ietf:params:oauth:grant-type:device_code\""), "{requests}");
    assert!(requests.contains("\"device_code\":\"dev-123\""), "{requests}");
    // The two discovery GETs ride the fresh bearer (opencode.ts:592-600).
    assert!(requests.contains("authorization: Bearer zen-access"), "{requests}");

    // The credential (G-244, opencode.ts:608-621): label derives from the
    // org NAME resolved through /api/orgs (never the raw org_id), and the
    // metadata records the full five-key set.
    let info = host.get("opencode").await.expect("get").expect("present");
    let ConnectionInfo::Credential { label, method, .. } = &info.connections[0] else { panic!() };
    assert_eq!(label, "Acme Corp");
    assert_eq!(method, "oauth");
    let active = host.connection_active("opencode").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "zen-access");
    assert_eq!(oauth.refresh, "zen-refresh");
    let metadata = oauth.metadata.expect("metadata");
    assert_eq!(metadata.get("server").and_then(Value::as_str), Some(mock.base.as_str()));
    assert_eq!(metadata.get("accountID").and_then(Value::as_str), Some("user_77"));
    assert_eq!(metadata.get("email").and_then(Value::as_str), Some("dev@acme.test"));
    assert_eq!(metadata.get("orgID").and_then(Value::as_str), Some("org_9"));
    assert_eq!(metadata.get("orgName").and_then(Value::as_str), Some("Acme Corp"));
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
    // Refresh request shape (opencode.ts:130-136 + G-244 JSON encoding,
    // :644-650): refresh grant + client id in a JSON body.
    let requests = mock.requests().join("\n");
    assert!(requests.contains("\"grant_type\":\"refresh_token\""), "{requests}");
    assert!(requests.contains("\"refresh_token\":\"old-refresh\""), "{requests}");
    assert!(requests.contains("\"client_id\":\"opencode-cli\""), "{requests}");
}

/// G-244 end-to-end: a device login against a (self-hosted) answer.server
/// records `metadata.server`, so a later refresh of the expired token hits
/// THAT server — not the default `https://opencode.ai/console`.
#[tokio::test]
async fn zen_device_login_then_refresh_hits_answer_server() {
    let mock = MockOAuth::start(vec![
        (
            "/auth/device/code",
            vec![json!({
                "device_code": "dev-e2e",
                "user_code": "E2E-0001",
                "verification_uri": "/verify",
                "expires_in": 900,
                "interval": 0
            })],
        ),
        (
            "/auth/device/token",
            vec![
                json!({"access_token": "first-access", "refresh_token": "first-refresh", "expires_in": 3600}),
                json!({"access_token": "second-access", "refresh_token": "second-refresh", "expires_in": 3600}),
            ],
        ),
        ("/api/user", vec![json!({"id": "u1", "email": "u@t.test"})]),
        ("/api/orgs", vec![json!([{"id": "o1", "name": "One"}])]),
    ])
    .await;
    let host = host().await;
    let attempt = host
        .oauth_connect("opencode", "device", &answer(&[("server", mock.base.as_str())]), None)
        .await
        .expect("connect");
    let status = wait_complete(&host, "opencode", &attempt, 400).await;
    assert!(matches!(status, AttemptStatus::Complete { .. }), "{status:?}");

    // Expire the stored credential; resolving refreshes through
    // metadata.server (the mock), never the official console.
    let active = host.connection_active("opencode").await.expect("active").expect("some");
    let ConnectionInfo::Credential { id, .. } = &active else { panic!() };
    let stored = host.store().get(id).await.expect("get").expect("some");
    let CredentialValue::OAuth(mut oauth) = stored.value else { panic!() };
    oauth.expires = crate::database::migrations::now_ms() - 1000;
    host.store()
        .update(id, None, Some(&CredentialValue::OAuth(oauth.clone())))
        .await
        .expect("expire");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(rotated) = resolved else { panic!() };
    assert_eq!(rotated.access, "second-access");
    assert_eq!(rotated.refresh, "second-refresh");
    // The metadata (incl. server) survives the rotation.
    assert_eq!(
        rotated.metadata.as_ref().and_then(|m| m.get("server")).and_then(Value::as_str),
        Some(mock.base.as_str())
    );
    // Exactly one refresh grant reached the mock — a fallback to
    // opencode.ai would never appear here.
    let refreshes = mock
        .requests()
        .iter()
        .filter(|request| request.contains("\"grant_type\":\"refresh_token\""))
        .count();
    assert_eq!(refreshes, 1);
}

/// G-244 org resolution (opencode.ts:601-607): a token `org_id` absent
/// from `/api/orgs` fails the login; without one the name-then-id
/// sorted first org wins.
#[tokio::test]
async fn zen_org_resolution_matches_org_id_or_sorts_by_name() {
    let mock = MockOAuth::start(vec![
        (
            "/auth/device/code",
            vec![json!({
                "device_code": "dev-org",
                "user_code": "ORG-0001",
                "verification_uri": "/verify",
                "expires_in": 900,
                "interval": 0
            })],
        ),
        (
            "/auth/device/token",
            vec![json!({"access_token": "a", "refresh_token": "r", "expires_in": 3600, "org_id": "org_missing"})],
        ),
        ("/api/user", vec![json!({"id": "u1", "email": "u@t.test"})]),
        ("/api/orgs", vec![json!([{"id": "org_1", "name": "A"}])]),
    ])
    .await;
    let host = host().await;
    let attempt = host
        .oauth_connect("opencode", "device", &answer(&[("server", mock.base.as_str())]), None)
        .await
        .expect("connect");
    let status = wait_complete(&host, "opencode", &attempt, 400).await;
    match status {
        AttemptStatus::Failed { message, .. } => {
            assert!(message.contains("OpenCode organization not found: org_missing"), "{message}");
        }
        other => panic!("expected failed, got {other:?}"),
    }

    // No org_id → the sorted-first org (name then id).
    let mock = MockOAuth::start(vec![
        (
            "/auth/device/code",
            vec![json!({
                "device_code": "dev-org2",
                "user_code": "ORG-0002",
                "verification_uri": "/verify",
                "expires_in": 900,
                "interval": 0
            })],
        ),
        (
            "/auth/device/token",
            vec![json!({"access_token": "a2", "refresh_token": "r2", "expires_in": 3600})],
        ),
        (
            "/api/user",
            vec![json!({"id": "u2", "email": "u2@t.test"})],
        ),
        (
            "/api/orgs",
            vec![json!([
                {"id": "z-org", "name": "Beta"},
                {"id": "a-org", "name": "Beta"},
                {"id": "m-org", "name": "Alpha"}
            ])],
        ),
    ])
    .await;
    let attempt = host
        .oauth_connect("opencode", "device", &answer(&[("server", mock.base.as_str())]), None)
        .await
        .expect("connect 2");
    let status = wait_complete(&host, "opencode", &attempt, 400).await;
    assert!(matches!(status, AttemptStatus::Complete { .. }), "{status:?}");
    let info = host.get("opencode").await.expect("get").expect("present");
    let ConnectionInfo::Credential { label, .. } = &info.connections[0] else { panic!() };
    // "Alpha" sorts before "Beta"; the id tie-break picks a-org.
    assert_eq!(label, "Alpha");
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
    // G-234②: success_page delegates to the shared branded module.
    assert!(text.contains("OpenCode is now connected to Test."), "{text}");
    assert!(text.contains("Authorization successful"), "{text}");
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
    let _port = gitlab_port_lock().await;
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

    // Act as the browser: hit the fixed loopback redirect. G-234② — the
    // response is HELD until the exchange settles (the 200 answers with
    // the branded success card afterwards).
    let callback =
        reqwest::get(format!("http://127.0.0.1:8080/callback?code=gl-code&state={state}"))
            .await
            .expect("callback");
    assert_eq!(callback.status(), 200);
    let text = callback.text().await.expect("body");
    assert!(text.contains("OpenCode is now connected to GitLab."), "{text}");

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
async fn gitlab_bundled_client_rejected_off_gitlab_com() {    let _env = env_guard().await;
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
async fn gitlab_refresh_rotates_via_mock() {    let mock = MockOAuth::start(vec![(
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

/// G-260① — the keyed single-flight RETAINS a successful refresh for
/// 60s (gitlab.ts:50-51): concurrent and immediately-following callers
/// holding the ALREADY-ROTATED refresh token reuse the result instead
/// of re-exchanging (which GitLab would answer invalid_grant — misread
/// as revoked).
#[tokio::test]
async fn gitlab_refresh_single_flight_retains_success() {
    let mock = MockOAuth::start(vec![(
        "/oauth/token",
        vec![json!({"access_token": "gl-rotated", "refresh_token": "gl-next", "expires_in": 7200})],
    )])
    .await;
    let value = OAuthValue {
        method_id: "pkce".into(),
        refresh: format!("gl-shared-{}", mock.base),
        access: "gl-old".into(),
        expires: crate::database::migrations::now_ms() - 1000,
        metadata: Some(BTreeMap::from([
            ("instanceUrl".to_owned(), Value::String(mock.base.clone())),
            ("clientID".to_owned(), Value::String("legacy-client".into())),
        ])),
    };
    // Concurrent second consumer + a follow-up within the retention
    // window: ONE exchange total.
    let (first, second) = tokio::join!(
        super::providers_enterprise::gitlab_refresh(value.clone()),
        super::providers_enterprise::gitlab_refresh(value.clone()),
    );
    let first = first.expect("first refresh");
    let second = second.expect("second refresh");
    assert_eq!(first, second);
    let third = super::providers_enterprise::gitlab_refresh(value).await.expect("third");
    assert_eq!(third, first);
    let exchanges = mock
        .requests()
        .iter()
        .filter(|request| request.contains("grant_type=refresh_token"))
        .count();
    assert_eq!(exchanges, 1, "the rotated token is never spent twice");
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
/// nonce (the `id_token` must echo it). Path-routed replies (the JWKS
/// document) take precedence over the token-endpoint slot.
struct SlotMock {
    base: String,
    slot: Arc<parking_lot::Mutex<Option<(u16, Value)>>>,
    routes: Arc<parking_lot::Mutex<BTreeMap<String, Value>>>,
    log: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl SlotMock {
    async fn start() -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let slot: Arc<parking_lot::Mutex<Option<(u16, Value)>>> = Arc::default();
        let routes: Arc<parking_lot::Mutex<BTreeMap<String, Value>>> = Arc::default();
        let log: Arc<parking_lot::Mutex<Vec<String>>> = Arc::default();
        let (slot_handle, routes_handle, log_handle) =
            (Arc::clone(&slot), Arc::clone(&routes), Arc::clone(&log));
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or_default().to_owned();
                let body_start = request.find("\r\n\r\n").map_or(read, |index| index + 4);
                log_handle.lock().push(format!("{path} {}", &request[body_start..]));
                let (status, body) = routes_handle
                    .lock()
                    .get(&path)
                    .cloned()
                    .map(|body| (200, body))
                    .or_else(|| slot_handle.lock().clone())
                    .unwrap_or((404, Value::Null));
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self { base: format!("http://127.0.0.1:{port}"), slot, routes, log }
    }

    /// Serves `body` (200) for every request hitting `path`.
    fn serve(&self, path: &str, body: Value) {
        self.routes.lock().insert(path.to_owned(), body);
    }
}

#[tokio::test]
async fn chatgpt_connector_round_trip_against_mock() {
    let mock = SlotMock::start().await;
    // authorize binds the loopback; the token + JWKS URLs point at the mock.
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
        &format!("{}/.well-known/jwks.json", mock.base),
        &CHATGPT_TEST_HOST_ID,
        None,
    )
    .await
    .expect("authorize");
    // The authorize URL carries the dynamic-client registration params
    // (chatgpt.ts:498-531) — G-260②: the stable agent host id rides the
    // request; agent_name_hint only when no saved client exists.
    assert!(authorization.url.contains("client_id=dynamic_agent_client"), "{}", authorization.url);
    assert!(authorization.url.contains("agent_name_hint=OpenCode"), "{}", authorization.url);
    assert!(
        authorization.url.contains("ext_agent_host_id=urn%3Auuid%3A"),
        "{}",
        authorization.url
    );
    assert!(authorization.url.contains("nonce="), "{}", authorization.url);
    assert!(authorization.url.contains("chatgpt.tokens.use.direct"), "{}", authorization.url);
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let nonce = parse_query_param(&authorization.url, "nonce").expect("nonce");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");

    // The signed id_token must echo the attempt nonce; aud = the client
    // the callback will carry (chatgpt.ts:165-171,439-461 claims subset).
    mock.serve("/.well-known/jwks.json", jwks_document("sign-in", SIGNING_KEY_N));
    let id_token = signed_id_token(
        &valid_claims(&nonce, "client_reg_1", 9_999_999_999),
        "sign-in",
        SIGNING_KEY_PEM,
    );
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
    // G-234② — the response is HELD until the exchange settles, so the
    // GET runs concurrently with the callback future.
    let callback_get = tokio::spawn(async move {
        reqwest::get(format!("{redirect}?code=cg-code&state={state}&client_id=client_reg_1"))
            .await
            .expect("callback")
    });
    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto") };
    let value = callback_future.await.expect("callback value");
    let callback = callback_get.await.expect("join");
    assert_eq!(callback.status(), 200);
    let text = callback.text().await.expect("body");
    assert!(text.contains("OpenCode is now connected to ChatGPT."), "{text}");
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

/// G-260② — reauthorization reuses the connection's registered client
/// (chatgpt.ts:107,165-171): a saved client id takes precedence in the
/// authorize URL (no agent_name_hint), a callback without one still
/// completes, and a DIFFERENT returned client rejects the attempt.
#[tokio::test]
async fn chatgpt_reauthorization_reuses_saved_client() {
    let mock = SlotMock::start().await;
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
        &format!("{}/.well-known/jwks.json", mock.base),
        &CHATGPT_TEST_HOST_ID,
        Some("client_reg_1"),
    )
    .await
    .expect("authorize");
    assert!(
        authorization.url.contains("client_id=client_reg_1"),
        "{}",
        authorization.url
    );
    assert!(
        !authorization.url.contains("agent_name_hint"),
        "no registration hint when a saved client exists: {}",
        authorization.url
    );
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let nonce = parse_query_param(&authorization.url, "nonce").expect("nonce");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");
    mock.serve("/.well-known/jwks.json", jwks_document("sign-in", SIGNING_KEY_N));
    let id_token =
        signed_id_token(&valid_claims(&nonce, "client_reg_1", 9_999_999_999), "sign-in", SIGNING_KEY_PEM);
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

    // The callback omits client_id — the saved client completes the flow
    // (G-234②: the redirect responds only after the exchange).
    let callback_get =
        tokio::spawn(async move { reqwest::get(format!("{redirect}?code=cg-code&state={state}")).await });
    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto") };
    let value = callback_future.await.expect("callback value");
    let response = callback_get.await.expect("join").expect("callback");
    assert_eq!(response.status(), 200);
    let CredentialValue::OAuth(oauth) = value else { panic!() };
    assert_eq!(
        oauth.metadata.as_ref().expect("metadata")["clientID"],
        "client_reg_1",
        "the saved client id persists"
    );

    // A DIFFERENT returned client rejects the reauthorization.
    let mock = SlotMock::start().await;
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
        &format!("{}/.well-known/jwks.json", mock.base),
        &CHATGPT_TEST_HOST_ID,
        Some("client_reg_1"),
    )
    .await
    .expect("authorize 2");
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");
    let callback_get = tokio::spawn(async move {
        reqwest::get(format!("{redirect}?code=other&state={state}&client_id=someone_else")).await
    });
    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto 2") };
    let error = callback_future.await.expect_err("client mismatch");
    assert!(error.contains("returned a different client"), "{error}");
    let response = callback_get.await.expect("join 2").expect("callback 2");
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn chatgpt_connector_rejects_missing_token_sharing_scope() {
    let mock = SlotMock::start().await;
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
        &format!("{}/.well-known/jwks.json", mock.base),
        &CHATGPT_TEST_HOST_ID,
        None,
    )
    .await
    .expect("authorize");
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let nonce = parse_query_param(&authorization.url, "nonce").expect("nonce");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");
    // Verification never runs (the sharing-scope check fails first), so an
    // unsigned token fixture suffices here.
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
    // G-234② — the held redirect answers the branded ERROR card (400)
    // once the flow fails (the sharing-scope check).
    let callback_get = tokio::spawn(async move {
        reqwest::get(format!("{redirect}?code=cg-code&state={state}&client_id=client_reg_1"))
            .await
            .expect("callback")
    });
    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto") };
    let error = callback_future.await.expect_err("missing sharing scope");
    assert!(error.contains("without token sharing"), "{error}");
    let callback = callback_get.await.expect("join");
    assert_eq!(callback.status(), 400);
    let text = callback.text().await.expect("body");
    assert!(text.contains("OpenCode couldn't finish connecting to ChatGPT."), "{text}");
    assert!(text.contains("without token sharing"), "{text}");
}

#[tokio::test]
async fn chatgpt_connector_rejects_nonce_mismatch() {
    let mock = SlotMock::start().await;
    let authorization = super::providers_enterprise::chatgpt_authorize_at(
        BTreeMap::new(),
        &format!("{}/oauth/token", mock.base),
        &format!("{}/.well-known/jwks.json", mock.base),
        &CHATGPT_TEST_HOST_ID,
        None,
    )
    .await
    .expect("authorize");
    let state = parse_query_param(&authorization.url, "state").expect("state");
    let redirect = parse_query_param(&authorization.url, "redirect_uri").expect("redirect");
    // The signed id_token echoes the WRONG nonce (chatgpt.ts:449-450) —
    // every verification failure carries v2's single generic message.
    mock.serve("/.well-known/jwks.json", jwks_document("sign-in", SIGNING_KEY_N));
    let id_token = signed_id_token(
        &valid_claims("attacker-nonce", "client_reg_1", 9_999_999_999),
        "sign-in",
        SIGNING_KEY_PEM,
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
    // G-234② — deferred respond: the nonce-mismatch failure answers the
    // branded error card.
    let callback_get = tokio::spawn(async move {
        reqwest::get(format!("{redirect}?code=cg-code&state={state}&client_id=client_reg_1"))
            .await
            .expect("callback")
    });
    let AuthMode::Auto(callback_future) = authorization.mode else { panic!("auto") };
    let error = callback_future.await.expect_err("nonce mismatch");
    assert_eq!(error, "ChatGPT sign-in returned an invalid ID token.");
    let callback = callback_get.await.expect("join");
    assert_eq!(callback.status(), 400);
    let text = callback.text().await.expect("body");
    assert!(text.contains("invalid ID token"), "{text}");
}

// -- BE-3b: chatgpt ID-token JWKS verification (chatgpt.ts:439-459) --------------------------

/// G-260② — the stable agent host id fixture for the authorize seam.
const CHATGPT_TEST_HOST_ID: &str = "urn:uuid:00000000-1111-4222-8333-444444444444";

/// The single v2 failure surface (chatgpt.ts:457): every verification
/// failure — signature, issuer, audience, expiry, nonce, subject —
/// carries this message; the specific cause rides the server-side log.
const INVALID_ID_TOKEN: &str = "ChatGPT sign-in returned an invalid ID token.";

/// Committed RSA-2048 PKCS#8 fixtures: `signing` is the JWKS-served key;
/// `rogue` signs forged tokens and the rotated-in key after a JWKS roll.
const SIGNING_KEY_PEM: &str = include_str!("testdata/id_token_signing_key.pem");
const ROGUE_KEY_PEM: &str = include_str!("testdata/id_token_rogue_key.pem");
/// Each fixture's JWK `n` plus the shared `e` (65537), base64url — cut
/// once with `openssl rsa -modulus` when the fixtures were generated.
const SIGNING_KEY_N: &str = "vFp_fPHAjLAqQUpCQY8OgRVhBRwiZYhfEnF49DO9M9MgiZDYJ_K-WPjwroou9JGJy5rmxnMMu9CDtRxM4TBcWojiNnGOVQxa4FT2WkQlE-ztQ1t1A-1Rc4KEBszafUdvz6nn6TBzoTLxqezq0r8HOOmJ7E1z5HMxCe-uJq7KF8uUbWmfgVNALxzuuLJJrtwMcvhpzSKtuAZOyA8DfgxYDCWJ5onNu4Gfmy5QkWkeqSe9WSLzHFw54GdPZ6d4GyubAP9KUOBZYwywDAEtUcS1yaEb8I0Tq0MFfzJHT_q7jOkGOATkHGhTsEl6r0yrKyvKWoMgibabP5E45Lw_PFm4fQ";
const ROGUE_KEY_N: &str = "vaTeQ_dAcpc52kCnFRw_US22BGb2MKPdpQV8i3b491ED99qsaZuuToO6vQPn0jSQ4LnB0CHoM0x8T8f7Hh4Lzus_MjPMqegKUbDQ2VAnnwzkkXhoXDGG9TLJ1rtlONVl4RYngt0pBfIeiJVT8rGxb1lmQvPRrpQsEol-T51MaxGZQ-B3iweZFOyvbHX-8eZHYHpWlznARswKKYVs106PpwmBGbBFhsXExCOFDD1EBJt2PSR-t2q_IAbgQEnQ1sW516-mQDazofl50nzJrRbC5LS1srVwEGtFLXA37268sLDWo_JHHZepfpnyRxFS8rsHW_XyWqcDw4hZeFzv6B1cgQ";
/// `0x010001` → base64url `AQAB`.
const RSA_E_65537: &str = "AQAB";

/// A one-key JWKS document (the v2 test server's shape).
fn jwks_document(kid: &str, n: &str) -> Value {
    json!({ "keys": [ { "kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid, "n": n, "e": RSA_E_65537 } ] })
}

/// The DER bytes inside a single-block PEM envelope (fixture loader).
fn pem_der(pem: &str) -> Vec<u8> {
    use base64::Engine;
    let body: String =
        pem.lines().filter(|line| !line.starts_with("-----")).flat_map(str::chars).collect();
    base64::engine::general_purpose::STANDARD.decode(body).expect("fixture PEM decodes")
}

/// RS256-signs `header.payload` with a fixture key (the v2 `SignJWT`).
fn sign_jwt(header: &Value, payload: &Value, key_pem: &str) -> String {
    use base64::Engine;
    let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let signing_input =
        format!("{}.{}", b64(header.to_string().as_bytes()), b64(payload.to_string().as_bytes()));
    let key_pair = ring::signature::RsaKeyPair::from_pkcs8(&pem_der(key_pem)).expect("fixture key");
    let rng = ring::rand::SystemRandom::new();
    let mut signature = vec![0u8; key_pair.public().modulus_len()];
    key_pair
        .sign(&ring::signature::RSA_PKCS1_SHA256, &rng, signing_input.as_bytes(), &mut signature)
        .expect("fixture signs");
    format!("{signing_input}.{}", b64(&signature))
}

/// A well-formed RS256 ID token under `kid` (the v2 `sign` helper).
fn signed_id_token(payload: &Value, kid: &str, key_pem: &str) -> String {
    sign_jwt(&json!({ "alg": "RS256", "kid": kid }), payload, key_pem)
}

/// The valid claim set (v2: issuer/audience/subject/nonce/expiry).
fn valid_claims(nonce: &str, audience: &str, exp: i64) -> Value {
    json!({ "iss": "https://auth.openai.com", "aud": audience, "exp": exp, "nonce": nonce, "sub": "user_123" })
}

/// A mock JWKS endpoint on 127.0.0.1:0 whose served document can be
/// swapped mid-test (key rotation).
struct JwksMock {
    url: String,
    body: Arc<parking_lot::Mutex<Value>>,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for JwksMock {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl JwksMock {
    async fn start(body: Value) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let body = Arc::new(parking_lot::Mutex::new(body));
        let served = Arc::clone(&body);
        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let _request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let text = served.lock().to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        Self { url: format!("http://127.0.0.1:{port}/.well-known/jwks.json"), body, handle }
    }

    /// Swaps the served document; the next verification fetches it fresh.
    fn rotate(&self, body: Value) {
        *self.body.lock() = body;
    }
}

async fn verify_id_token(
    token: &str,
    client_id: &str,
    nonce: &str,
    jwks: &JwksMock,
) -> Result<(), String> {
    super::providers_enterprise::verify_id_token(token, client_id, nonce, &jwks.url).await
}

fn exp_in_60s() -> i64 {
    crate::database::migrations::now_ms() / 1000 + 60
}

#[tokio::test]
async fn chatgpt_id_token_verifies_signed_token() {
    let jwks = JwksMock::start(jwks_document("sign-in", SIGNING_KEY_N)).await;
    let token = signed_id_token(
        &valid_claims("expected", "oaiapp_issued", exp_in_60s()),
        "sign-in",
        SIGNING_KEY_PEM,
    );
    verify_id_token(&token, "oaiapp_issued", "expected", &jwks)
        .await
        .expect("valid token verifies");
}

#[tokio::test]
async fn chatgpt_id_token_rejects_claim_failures() {
    let jwks = JwksMock::start(jwks_document("sign-in", SIGNING_KEY_N)).await;
    let exp = exp_in_60s();
    let sign = |claims: &Value| signed_id_token(claims, "sign-in", SIGNING_KEY_PEM);
    // Wrong issuer.
    let mut claims = valid_claims("expected", "oaiapp_issued", exp);
    claims["iss"] = json!("https://someone-else.example");
    assert_eq!(
        verify_id_token(&sign(&claims), "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // Wrong audience — both a mismatched claim and a mismatched client id.
    let wrong_audience = sign(&valid_claims("expected", "someone-else", exp));
    assert_eq!(
        verify_id_token(&wrong_audience, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    let valid = sign(&valid_claims("expected", "oaiapp_issued", exp));
    assert_eq!(
        verify_id_token(&valid, "wrong-client", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // Expired (v2 signs at `expires - 120`).
    let expired = sign(&valid_claims("expected", "oaiapp_issued", exp - 120));
    assert_eq!(
        verify_id_token(&expired, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // Missing exp.
    let mut claims = valid_claims("expected", "oaiapp_issued", exp);
    claims.as_object_mut().expect("object").remove("exp");
    assert_eq!(
        verify_id_token(&sign(&claims), "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // Missing subject.
    let mut claims = valid_claims("expected", "oaiapp_issued", exp);
    claims.as_object_mut().expect("object").remove("sub");
    assert_eq!(
        verify_id_token(&sign(&claims), "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // Blank subject.
    let mut claims = valid_claims("expected", "oaiapp_issued", exp);
    claims["sub"] = json!(" ");
    assert_eq!(
        verify_id_token(&sign(&claims), "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // Wrong nonce.
    let wrong_nonce = sign(&valid_claims("attacker-nonce", "oaiapp_issued", exp));
    assert_eq!(
        verify_id_token(&wrong_nonce, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
}

#[tokio::test]
async fn chatgpt_id_token_rejects_unverified_signatures() {
    let jwks = JwksMock::start(jwks_document("sign-in", SIGNING_KEY_N)).await;
    let claims = valid_claims("expected", "oaiapp_issued", exp_in_60s());
    // Forged: a different key pair signs while the JWKS serves `sign-in`.
    let forged = signed_id_token(&claims, "sign-in", ROGUE_KEY_PEM);
    assert_eq!(
        verify_id_token(&forged, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // Alg confusion: the header LIES (`HS256`, then a `none` variant)
    // while the signature is a VALID RS256 signature under the trusted
    // `sign-in` key — kid selection, the JWKS fetch, and ring
    // verification would all succeed, so ONLY the RS256 allowlist can
    // reject these (jose `algorithms: ["RS256"]`; the alg check runs
    // after JWKS fetch + kid selection, matching jose).
    let hs256 = sign_jwt(&json!({ "alg": "HS256", "kid": "sign-in" }), &claims, SIGNING_KEY_PEM);
    assert_eq!(
        verify_id_token(&hs256, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    let none = sign_jwt(&json!({ "alg": "none", "kid": "sign-in" }), &claims, SIGNING_KEY_PEM);
    assert_eq!(
        verify_id_token(&none, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    // A header without `kid` cannot select a JWK.
    let no_kid = sign_jwt(&json!({ "alg": "RS256" }), &claims, SIGNING_KEY_PEM);
    assert_eq!(
        verify_id_token(&no_kid, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
}

#[tokio::test]
async fn chatgpt_id_token_unknown_kid_then_rotation_succeeds() {
    // v2 builds `createRemoteJWKSet` fresh per verification (no cache):
    // an unknown `kid` fails, then the SAME token verifies once the
    // endpoint rotates the key in (ruling 2026-10-23).
    let jwks = JwksMock::start(jwks_document("sign-in", SIGNING_KEY_N)).await;
    let token = signed_id_token(
        &valid_claims("expected", "oaiapp_issued", exp_in_60s()),
        "rotated-key",
        ROGUE_KEY_PEM,
    );
    assert_eq!(
        verify_id_token(&token, "oaiapp_issued", "expected", &jwks).await.unwrap_err(),
        INVALID_ID_TOKEN
    );
    jwks.rotate(jwks_document("rotated-key", ROGUE_KEY_N));
    verify_id_token(&token, "oaiapp_issued", "expected", &jwks)
        .await
        .expect("rotated JWKS verifies the same token");
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
                        "acme-large": {
                            "name": "Acme Large",
                            "settings": {
                                "baseURL": "https://model.acme.test/v1",
                                "reasoningEffort": "high",
                                "apiKey": "model-secret-never"
                            }
                        },
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
    let config = super::console::fetch_config(&CredentialValue::OAuth(credential))
        .await
        .expect("fetch")
        .expect("some");
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
    // F1: model settings beyond baseURL project credential-stripped
    // (opencode.ts:277-280 withoutCredentials(config.settings)); the
    // baseURL overlay rides its own field.
    let model = &provider.models[0];
    assert_eq!(model.base_url.as_deref(), Some("https://model.acme.test/v1"));
    let rest = model.settings.as_ref().expect("settings projected").rest.clone();
    assert_eq!(rest.get("reasoningEffort"), Some(&serde_json::json!("high")));
    assert!(!rest.contains_key("apiKey"), "credentials never ride model settings");
    // Credentials never ride provider settings (withoutCredentials).
    assert!(provider.body.is_none());
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
        metadata: Some(BTreeMap::from([
            ("server".to_owned(), Value::String(mock.base.clone())),
            // G-244 — the SSO message names the credential's org
            // (opencode.ts:69-70 organizationName, :493-494).
            ("orgName".to_owned(), Value::String("Acme Corp".into())),
        ])),
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
    assert!(
        status.message.contains("Sign in with SSO again to use Acme Corp"),
        "{}",
        status.message
    );
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

// -- Task 3 (CONSOLE lane): decode + projection of the v2 merge fields
// (opencode.ts:245-315, config/provider.ts:53-101) ------------------------------

/// `models.<id>.variants` + `models.<id>.package` decode
/// (config/provider.ts:83,85-89) and projection (opencode.ts:272,
/// 282-293): variants carry per id with settings/headers/body overlays,
/// and variant settings never carry credentials
/// (`withoutCredentials(variant.settings)`, opencode.ts:289).
#[test]
fn console_decode_and_project_model_variants_and_package() {
    let config: super::console::ConsoleConfig = serde_json::from_value(json!({
        "providers": {
            "gw": {
                "settings": { "baseURL": "https://gw.test/v1" },
                "models": {
                    "acme-opus": {
                        "package": "@opencode/ai/providers/openai-compatible",
                        "variants": [
                            {
                                "id": "high",
                                "settings": { "temperature": 0.3, "apiKey": "leak" },
                                "headers": { "x-v": "1" },
                                "body": { "top_k": 7 }
                            },
                            { "id": "turbo" }
                        ]
                    }
                }
            }
        }
    }))
    .expect("decode");
    let model = &config.providers["gw"].models.as_ref().unwrap()["acme-opus"];
    assert_eq!(
        model.package.as_deref(),
        Some("@opencode/ai/providers/openai-compatible"),
        "model-level package decodes (config/provider.ts:83, schema key `package` only)"
    );
    let variants = model.variants.as_ref().unwrap();
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].id, "high");
    assert!(variants[0].settings.is_some());
    assert!(variants[1].headers.is_none(), "variant overlays are all optional");

    let providers = super::console::to_custom_providers(&config);
    let projected = &providers[0].models[0];
    assert_eq!(projected.package.as_deref(), Some("@opencode/ai/providers/openai-compatible"));
    assert_eq!(projected.variants.len(), 2, "both variants ride the projection");
    let high = &projected.variants[0];
    assert_eq!(high.id, "high");
    let settings = high.settings.as_ref().expect("variant settings decode");
    assert_eq!(settings.rest.get("temperature"), Some(&serde_json::json!(0.3)));
    assert!(
        !settings.rest.contains_key("apiKey"),
        "credentials never ride variant settings (opencode.ts:289)"
    );
    assert_eq!(high.headers.as_ref().and_then(|h| h.get("x-v")).cloned(), Some("1".to_owned()));
    assert_eq!(high.body.as_ref().unwrap().get("top_k"), Some(&serde_json::json!(7)));
}

/// `disabled` actually removes the model (opencode.ts:304
/// `model.enabled = !config.disabled`; consumers filter `enabled` —
/// model.ts:221 `available: all.filter(m => m.enabled)`, :264): the
/// projection drops disabled entries, and a fully-disabled provider
/// drops entirely.
#[test]
fn console_projection_drops_disabled_models() {
    let config: super::console::ConsoleConfig = serde_json::from_value(json!({
        "providers": {
            "gw": {
                "settings": { "baseURL": "https://gw.test/v1" },
                "models": {
                    "live": {},
                    "hidden": { "disabled": true }
                }
            },
            "all-hidden": {
                "settings": { "baseURL": "https://gw.test/v1" },
                "models": { "gone": { "disabled": true } }
            }
        }
    }))
    .expect("decode");
    let providers = super::console::to_custom_providers(&config);
    assert_eq!(providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["gw"]);
    assert_eq!(
        providers[0].models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["live"],
        "the disabled model is removed; the all-disabled provider drops"
    );
}

/// The v2 console emits the provider package under `package`
/// (config/provider.ts:97); our decode used the wrong key (`type`).
/// `package` is primary, `type` stays accepted for legacy payloads.
#[test]
fn console_decode_package_primary_with_type_fallback() {
    let primary: super::console::ConsoleProvider =
        serde_json::from_value(json!({ "package": "@opencode/ai/providers/anthropic" }))
            .expect("decode package");
    assert_eq!(primary.package().as_deref(), Some("@opencode/ai/providers/anthropic"));
    let legacy: super::console::ConsoleProvider =
        serde_json::from_value(json!({ "type": "@opencode/ai/providers/anthropic" }))
            .expect("decode legacy type");
    assert_eq!(
        legacy.package().as_deref(),
        Some("@opencode/ai/providers/anthropic"),
        "type stays accepted for payloads already in the wild"
    );
    let both: super::console::ConsoleProvider = serde_json::from_value(json!({
        "type": "@opencode/ai/providers/openai-compatible",
        "package": "@opencode/ai/providers/anthropic"
    }))
    .expect("decode both");
    assert_eq!(
        both.package().as_deref(),
        Some("@opencode/ai/providers/anthropic"),
        "package wins over the legacy type key regardless of order"
    );
}

/// The `Config.Model` merge fields decode (config/provider.ts:69-91):
/// family, compatibility, partial capabilities, cost single-or-array
/// with optional cache, partial limit, disabled.
#[test]
fn console_decode_model_merge_fields() {
    let config: super::console::ConsoleConfig = serde_json::from_value(json!({
        "providers": {
            "gw": {
                "canonical": "anthropic",
                "package": "@opencode/ai/providers/anthropic",
                "settings": { "baseURL": "https://gw.test/v1" },
                "models": {
                    "acme-opus": {
                        "modelID": "claude-opus-4-5",
                        "name": "Acme Opus",
                        "family": "acme-opus",
                        "disabled": false,
                        "compatibility": { "reasoningField": "thinking" },
                        "capabilities": { "tools": false },
                        "cost": { "input": 3, "output": 15, "cache": { "read": 0.3 } },
                        "limit": { "context": 400_000, "output": 64_000 },
                        "settings": { "baseURL": "https://model.test/v1" }
                    },
                    "acme-array": {
                        "cost": [
                            {
                                "tier": { "type": "context", "size": 200_000 },
                                "input": 1,
                                "output": 2,
                                "cache": { "read": 0.1, "write": 0.2 }
                            }
                        ]
                    },
                    "acme-hidden": { "disabled": true }
                }
            }
        }
    }))
    .expect("decode");
    let provider = &config.providers["gw"];
    assert_eq!(provider.canonical.as_deref(), Some("anthropic"));
    assert_eq!(provider.package().as_deref(), Some("@opencode/ai/providers/anthropic"));
    let opus = &provider.models.as_ref().unwrap()["acme-opus"];
    assert_eq!(opus.model_id.as_deref(), Some("claude-opus-4-5"));
    assert_eq!(opus.name.as_deref(), Some("Acme Opus"));
    assert_eq!(opus.family.as_deref(), Some("acme-opus"));
    assert_eq!(opus.disabled, Some(false));
    assert_eq!(
        opus.compatibility.as_ref().and_then(|c| c.reasoning_field.as_deref()),
        Some("thinking")
    );
    assert_eq!(opus.capabilities.as_ref().unwrap().tools, Some(false));
    assert!(opus.capabilities.as_ref().unwrap().input.is_none());
    let single = opus.cost.as_ref().unwrap().entries();
    assert_eq!(single.len(), 1, "single cost normalizes to a one-entry array");
    assert_eq!(single[0].input, oc_schema::common::UsdPerMillionTokens(3.0.into()));
    assert_eq!(
        single[0].cache.as_ref().unwrap().read,
        Some(oc_schema::common::UsdPerMillionTokens(0.3.into()))
    );
    assert!(single[0].cache.as_ref().unwrap().write.is_none());
    let limit = opus.limit.unwrap();
    assert_eq!(limit.context, Some(400_000));
    assert_eq!(limit.input, None);
    assert_eq!(limit.output, Some(64_000));
    let array = &provider.models.as_ref().unwrap()["acme-array"];
    let entries = array.cost.as_ref().unwrap().entries();
    assert_eq!(entries.len(), 1, "an array cost stays an array");
    assert_eq!(
        entries[0].tier,
        Some(oc_schema::model::CostTier {
            tier_type: oc_schema::model::CostTierType::Context,
            size: 200_000
        })
    );
    assert_eq!(
        entries[0].cache.as_ref().unwrap().write,
        Some(oc_schema::common::UsdPerMillionTokens(0.2.into()))
    );
    assert_eq!(provider.models.as_ref().unwrap()["acme-hidden"].disabled, Some(true));
}

/// Projection carries the merge material (opencode.ts:263-306): cost
/// normalized single-or-array → array with cache read/write defaulting
/// to 0.0, partial limit/capabilities, family, and the per-model
/// `settings.baseURL` overlay.
#[test]
fn console_projection_carries_merge_fields() {
    let config: super::console::ConsoleConfig = serde_json::from_value(json!({
        "providers": {
            "acme-ai": {
                "name": "Acme AI",
                "settings": { "baseURL": "https://gw.acme.test/v1" },
                "models": {
                    "acme-large": {
                        "name": "Acme Large",
                        "family": "acme",
                        "capabilities": { "tools": false },
                        "cost": { "input": 3, "output": 15 },
                        "limit": { "context": 400_000 },
                        "settings": { "baseURL": "https://model.acme.test/v1" }
                    }
                }
            }
        }
    }))
    .expect("decode");
    let providers = super::console::to_custom_providers(&config);
    assert_eq!(providers.len(), 1);
    let provider = &providers[0];
    assert_eq!(provider.base_url, "https://gw.acme.test/v1");
    let model = &provider.models[0];
    assert_eq!(model.id, "acme-large");
    assert_eq!(model.family.as_deref(), Some("acme"));
    assert_eq!(model.capabilities.as_ref().unwrap().tools, Some(false));
    assert_eq!(
        model.limit.unwrap(),
        crate::provider_catalog::facade::PartialModelLimit {
            context: Some(400_000),
            input: None,
            output: None
        }
    );
    // Cost REPLACES (single → array) with cache read/write defaulting to 0.0.
    let cost = model.cost.as_ref().unwrap();
    assert_eq!(cost.len(), 1);
    assert_eq!(cost[0].input, oc_schema::common::UsdPerMillionTokens(3.0.into()));
    assert_eq!(cost[0].output, oc_schema::common::UsdPerMillionTokens(15.0.into()));
    assert_eq!(cost[0].cache.read, oc_schema::common::UsdPerMillionTokens::zero());
    assert_eq!(cost[0].cache.write, oc_schema::common::UsdPerMillionTokens::zero());
    // The per-model baseURL overlay rides the model (precedence applies at bind).
    assert_eq!(model.base_url.as_deref(), Some("https://model.acme.test/v1"));
}

/// G-260⑤ — endpoint-less providers survive projection: the ref merges
/// a baseURL-less console provider onto its catalog base
/// (opencode.ts:245-261); a model-level `settings.baseURL` still rides
/// the model (precedence applies at bind, opencode.ts:273-281).
#[test]
fn console_projection_keeps_endpoint_less_providers() {
    let config: super::console::ConsoleConfig = serde_json::from_value(json!({
        "providers": {
            "model-endpoint": {
                "models": {
                    "m1": { "settings": { "baseURL": "https://m1.test/v1" } },
                    "m2": {}
                }
            },
            "no-endpoint": { "models": { "m3": {} } }
        }
    }))
    .expect("decode");
    let providers = super::console::to_custom_providers(&config);
    assert_eq!(
        providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec!["model-endpoint", "no-endpoint"],
        "both providers survive; the resolver merges the catalog base at bind"
    );
    let provider = &providers[0];
    assert_eq!(provider.base_url, "", "no provider-level baseURL");
    assert_eq!(provider.models.len(), 2, "the endpoint-less sibling model is kept");
    let m1 = provider.models.iter().find(|m| m.id == "m1").unwrap();
    assert_eq!(m1.base_url.as_deref(), Some("https://m1.test/v1"));
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

// -- G-115: metadata projection, incremental stderr, cancellation ----------

/// G-115①: `entry.ref.metadata` projects into Info (ref
/// integration.ts:389 — e.g. the MCP registry's `{source: "mcp"}`).
#[tokio::test]
async fn g115_integration_metadata_projects_into_info() {
    let host = host().await;
    host.update_integration("mcp-remote", "My MCP");
    host.update_method(
        "mcp-remote",
        Method::Command {
            id: "login".into(),
            label: "Log in".into(),
            command: vec!["true".into()],
        },
        None,
    );
    let metadata: std::collections::BTreeMap<String, Value> =
        [("source".to_owned(), Value::String("mcp".to_owned()))].into_iter().collect();
    host.set_integration_metadata("mcp-remote", Some(metadata));

    let listed = host.list().await.expect("list");
    let info = listed.iter().find(|info| info.id == "mcp-remote").expect("listed");
    assert_eq!(info.metadata.as_ref().and_then(|map| map.get("source")), Some(&json!("mcp")));

    let got = host.get("mcp-remote").await.expect("get").expect("some");
    assert_eq!(got.metadata.as_ref().and_then(|map| map.get("source")), Some(&json!("mcp")));

    // A rename preserves the metadata (the ref's editor.update only
    // mutates the fields it touches).
    host.update_integration("mcp-remote", "Renamed");
    let got = host.get("mcp-remote").await.expect("get").expect("some");
    assert_eq!(got.name, "Renamed");
    assert_eq!(got.metadata.as_ref().and_then(|map| map.get("source")), Some(&json!("mcp")));
}

/// G-115②: stderr streams INCREMENTALLY into the pending attempt's
/// message (ref integration.ts:645-670) — a polling status sees chunks
/// while the command is still running.
#[tokio::test]
async fn g115_command_stderr_streams_incrementally() {
    let host = host().await;
    host.update_integration("cmd-stream", "Streaming");
    host.update_method(
        "cmd-stream",
        Method::Command {
            id: "slow".into(),
            label: "Slow".into(),
            command: vec![
                "sh".into(),
                "-c".into(),
                "echo part1 >&2; sleep 1; echo part2 >&2; sleep 1; echo final-key".into(),
            ],
        },
        None,
    );
    let attempt = host.command_connect("cmd-stream", "slow", None).await.expect("connect");

    // Poll while pending: within the first second the message must
    // already contain part1 (the old `.output()` implementation only
    // produced the message at process exit).
    let mut saw_early_message = false;
    for _ in 0..20 {
        if let Ok(CommandAttemptStatus::Pending { message: Some(message), .. }) =
            host.command_status("cmd-stream", &attempt.attempt_id).await
        {
            if message.contains("part1") {
                saw_early_message = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(saw_early_message, "stderr chunk visible while the command still runs");

    // Terminal: the full accumulated stderr rides the failure-less path
    // (stdout becomes the key).
    for _ in 0..50 {
        if matches!(
            host.command_status("cmd-stream", &attempt.attempt_id).await,
            Ok(CommandAttemptStatus::Complete { .. })
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(matches!(
        host.command_status("cmd-stream", &attempt.attempt_id).await,
        Ok(CommandAttemptStatus::Complete { .. })
    ));
}

/// G-115③: cancelling a pending command attempt aborts it — the entry
/// disappears and no credential is ever persisted (ref
/// integration.ts:814-824 `Scope.close`).
#[tokio::test]
async fn g115_command_cancel_aborts_in_flight_attempt() {
    let host = host().await;
    host.update_integration("cmd-cancel", "Cancellable");
    host.update_method(
        "cmd-cancel",
        Method::Command {
            id: "sleep".into(),
            label: "Sleeps".into(),
            command: vec!["sh".into(), "-c".into(), "sleep 30; echo leaked-key".into()],
        },
        None,
    );
    let attempt = host.command_connect("cmd-cancel", "sleep", None).await.expect("connect");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    host.command_cancel("cmd-cancel", &attempt.attempt_id).await;

    assert!(matches!(
        host.command_status("cmd-cancel", &attempt.attempt_id).await,
        Err(IntegrationError::AttemptNotFound)
    ));
    // The killed command never settles a credential.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(host.connection_active("cmd-cancel").await.expect("active").is_none());
}

/// G-115③: cancelling a pending oauth attempt aborts the in-flight
/// exchange (the callback future is dropped — its Drop guard flips) and
/// never stores a credential (ref integration.ts:785-795).
#[tokio::test]
async fn g115_oauth_cancel_aborts_in_flight_exchange() {
    let host = host().await;
    let aborted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&aborted);
    let authorize: super::AuthorizeFn = Arc::new(move |_answer| {
        let flag = Arc::clone(&flag);
        Box::pin(async move {
            struct AbortOnDrop(Arc<std::sync::atomic::AtomicBool>);
            impl Drop for AbortOnDrop {
                fn drop(&mut self) {
                    self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
            let _guard = AbortOnDrop(Arc::clone(&flag));
            Ok(Authorization {
                url: "https://example.test/device".into(),
                instructions: "visit".into(),
                expires_at: None,
                mode: AuthMode::Auto(Box::pin(async {
                    // The exchange "hangs" until the browser answers —
                    // long enough that the cancel below must abort it.
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    Ok(CredentialValue::OAuth(OAuthValue {
                        method_id: "hang".into(),
                        refresh: "r".into(),
                        access: "stale-access".into(),
                        expires: 0,
                        metadata: None,
                    }))
                })),
            })
        }) as BoxFuture<'static, Result<Authorization, String>>
    });
    host.update_integration("oauth-hang", "Hanging");
    host.update_method(
        "oauth-hang",
        Method::OAuth { id: "hang".into(), label: "Hang login".into(), form: None },
        Some(OAuthImplementation { authorize, refresh: None, label: None }),
    );

    let attempt =
        host.oauth_connect("oauth-hang", "hang", &BTreeMap::new(), None).await.expect("connect");
    assert_eq!(attempt.mode, "auto");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    host.oauth_cancel("oauth-hang", &attempt.attempt_id).await;

    assert!(matches!(
        host.oauth_status("oauth-hang", &attempt.attempt_id).await,
        Err(IntegrationError::AttemptNotFound)
    ));
    // The in-flight exchange was aborted (the future dropped), and no
    // stale credential was stored.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        aborted.load(std::sync::atomic::Ordering::SeqCst),
        "the auto-callback future was dropped by the cancel"
    );
    assert!(host.connection_active("oauth-hang").await.expect("active").is_none());
}

// ===========================================================================
// G-225 — credential value decode failures never degrade to an empty key
// (ref credential.ts:63/66-78: `decode(row.value)` throws; a corrupt row
// fails the read; rows without integration_id are skipped).
// ===========================================================================

#[tokio::test]
async fn g225_corrupt_credential_value_fails_the_read_not_silently_empties() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db = Arc::new(
        crate::database::Database::open_file(dir.path().join("g225a.db")).await.expect("db"),
    );
    let bus = Arc::new(crate::bus::Bus::new(
        (*db).clone(),
        crate::bus::BusOptions { persist: false, ..BusOptions::deterministic() },
    ));
    let store = credential::CredentialStore::new(db.clone(), bus);
    // A healthy credential loads.
    store
        .create(credential::CreateInput {
            id: None,
            integration_id: "github",
            label: None,
            value: &CredentialValue::Key(KeyValue { key: "k-1".into(), metadata: None, configuration: None }),
            activate: None,
        })
        .await
        .expect("healthy create");
    // A corrupted row lands in the table (db-level corruption).
    {
        let db = db.clone();
        db.transaction(move |tx| {
            tx.conn()
                .execute(
                    "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                     VALUES ('cred_bad', 'github', 'corrupt', '{not json', 0, 0, 0)",
                    [],
                )
                .map_err(crate::database::DbError::Sqlite)?;
            Ok(())
        })
        .await
        .expect("corrupt row");
    }

    // all/list/get FAIL with the Decode error — the ref's decode throws
    // (credential.ts:67) and no fake empty-key credential is produced.
    assert!(matches!(store.all().await, Err(credential::StoreError::Decode(_))), "all fails");
    assert!(
        matches!(store.list("github").await, Err(credential::StoreError::Decode(_))),
        "list fails"
    );
    assert!(
        matches!(store.get("cred_bad").await, Err(credential::StoreError::Decode(_))),
        "get of the corrupt row fails"
    );
    // An integration whose rows are all healthy still reads fine.
    store
        .create(credential::CreateInput {
            id: None,
            integration_id: "linear",
            label: None,
            value: &CredentialValue::Key(KeyValue { key: "k-2".into(), metadata: None, configuration: None }),
            activate: None,
        })
        .await
        .expect("healthy create 2");
    let listed = store.list("linear").await.expect("healthy integration reads");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].value, CredentialValue::Key(KeyValue { key: "k-2".into(), metadata: None, configuration: None }));
}

#[tokio::test]
async fn g225_row_without_integration_id_is_skipped() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db = Arc::new(
        crate::database::Database::open_file(dir.path().join("g225b.db")).await.expect("db"),
    );
    let bus = Arc::new(crate::bus::Bus::new(
        (*db).clone(),
        crate::bus::BusOptions { persist: false, ..BusOptions::deterministic() },
    ));
    let store = credential::CredentialStore::new(db.clone(), bus);
    store
        .create(credential::CreateInput {
            id: Some("cred_kept"),
            integration_id: "github",
            label: None,
            value: &CredentialValue::Key(KeyValue { key: "k".into(), metadata: None, configuration: None }),
            activate: None,
        })
        .await
        .expect("healthy create");
    {
        let db = db.clone();
        db.transaction(move |tx| {
            tx.conn()
                .execute(
                    "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                     VALUES ('cred_orphan', NULL, 'orphan', '\"key\"', 0, 0, 0)",
                    [],
                )
                .map_err(crate::database::DbError::Sqlite)?;
            Ok(())
        })
        .await
        .expect("orphan row");
    }
    // stored(row) returns undefined for a row without integration_id
    // (credential.ts:66-67) — flatMap drops it.
    let all = store.all().await.expect("all");
    assert_eq!(all.len(), 1, "orphan row skipped: {all:?}");
    assert_eq!(all[0].id, "cred_kept");
    assert!(matches!(store.get("cred_orphan").await, Ok(None)), "orphan get is None");
}

// ===========================================================================
// G-117 — activate/update never write `time_updated`: the ref's drizzle
// `.set({...})` updates only the listed columns (credential.ts:124-130 for
// activate, :227-228 for update), so the column keeps its insert-time
// value across activation switches and label/value edits.
// ===========================================================================

/// (active, time_created, time_updated) for one credential row.
async fn row_times(db: &Arc<crate::database::Database>, id: &str) -> (i64, i64, i64) {
    let id = id.to_owned();
    let db = db.clone();
    db.transaction(move |tx| {
        tx.conn()
            .query_row(
                "SELECT active, time_created, time_updated FROM credential WHERE id = ?1",
                [&id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .map_err(crate::database::DbError::Sqlite)
    })
    .await
    .expect("row read")
}

/// Pins `time_updated` to a sentinel so a later stability assert cannot
/// pass by clock coincidence (create and mutate within the same ms).
async fn pin_time_updated(db: &Arc<crate::database::Database>, id: &str, value: i64) {
    let id = id.to_owned();
    let db = db.clone();
    db.transaction(move |tx| {
        tx.conn()
            .execute("UPDATE credential SET time_updated = ?2 WHERE id = ?1", (&id, value))
            .map_err(crate::database::DbError::Sqlite)?;
        Ok(())
    })
    .await
    .expect("pin time_updated");
}

#[tokio::test]
async fn g117_activate_and_update_never_touch_time_updated() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let db = Arc::new(
        crate::database::Database::open_file(dir.path().join("g117.db")).await.expect("db"),
    );
    let bus = Arc::new(crate::bus::Bus::new(
        (*db).clone(),
        crate::bus::BusOptions { persist: false, ..BusOptions::deterministic() },
    ));
    let store = credential::CredentialStore::new(db.clone(), bus);
    let key = |text: &str| {
        CredentialValue::Key(KeyValue { key: text.into(), metadata: None, configuration: None })
    };

    // Insert DOES set time_updated (== time_created, the insert-time value).
    store
        .create(credential::CreateInput {
            id: Some("cred_a"),
            integration_id: "github",
            label: None,
            value: &key("k-a"),
            activate: None,
        })
        .await
        .expect("create a");
    store
        .create(credential::CreateInput {
            id: Some("cred_b"),
            integration_id: "github",
            label: None,
            value: &key("k-b"),
            activate: Some(false),
        })
        .await
        .expect("create b");
    for (id, expect_active) in [("cred_a", 1), ("cred_b", 0)] {
        let (active, created, updated) = row_times(&db, id).await;
        assert_eq!(active, expect_active, "{id}: selection at insert");
        assert!(created > 0 && updated == created, "{id}: insert sets time_updated = time_created");
    }

    // Pin sentinels BEFORE mutating (see pin_time_updated).
    pin_time_updated(&db, "cred_a", 111_111).await;
    pin_time_updated(&db, "cred_b", 222_222).await;

    // activate flips the selection WITHOUT touching time_updated on either
    // row — the sibling deactivation is the same column-limited
    // `.set({ active: false })` (credential.ts:182-187).
    store.activate("cred_b").await.expect("activate b");
    let (a_active, _, a_updated) = row_times(&db, "cred_a").await;
    let (b_active, _, b_updated) = row_times(&db, "cred_b").await;
    assert_eq!((a_active, a_updated), (0, 111_111), "deactivated row keeps its mtime");
    assert_eq!((b_active, b_updated), (1, 222_222), "activated row keeps its mtime");

    // Label-only update: label lands, mtime stays.
    store.update("cred_b", Some("renamed"), None).await.expect("label update");
    let stored = store.get("cred_b").await.expect("get").expect("row");
    assert_eq!(stored.label, "renamed");
    assert_eq!(row_times(&db, "cred_b").await.2, 222_222, "label update keeps mtime");

    // Value-only update (the OAuth refresh path, integration/mod.rs): the
    // rotated value lands, mtime stays.
    store.update("cred_b", None, Some(&key("k-b-rotated"))).await.expect("value update");
    assert_eq!(store.get("cred_b").await.expect("get").expect("row").value, key("k-b-rotated"));
    assert_eq!(row_times(&db, "cred_b").await.2, 222_222, "value update keeps mtime");

    // Label + value together: same.
    store
        .update("cred_b", Some("renamed-again"), Some(&key("k-b-3")))
        .await
        .expect("label+value update");
    let stored = store.get("cred_b").await.expect("get").expect("row");
    assert_eq!(stored.label, "renamed-again");
    assert_eq!(stored.value, key("k-b-3"));
    assert_eq!(row_times(&db, "cred_b").await.2, 222_222, "label+value update keeps mtime");
}

// ===========================================================================
// G-234 — ① device/token form posts carry Accept + User-Agent; ② the
// loopback callback serves the shared branded oauth page (success AND
// error variants).
// ===========================================================================

/// ① — the outbound device-code POST carries `Accept: application/json`
/// and the app User-Agent (ref headers(), github-copilot.ts:376-385).
#[tokio::test]
async fn g234_device_post_carries_accept_and_user_agent() {
    let mock = MockOAuth::start(vec![(
        "/login/device/code",
        vec![json!({
            "device_code": "dc", "user_code": "UC", "verification_uri": "https://x/verify"
        })],
    )])
    .await;
    let http = oc_http::client();
    let _ = super::flows::request_device_code(
        &http,
        &format!("{}/login/device/code", mock.base),
        vec![("client_id", "cid".to_owned())],
    )
    .await
    .expect("device code");
    let requests = mock.requests();
    assert!(!requests.is_empty(), "request recorded");
    let recorded = requests.join("\n");
    let lowered = recorded.to_ascii_lowercase();
    assert!(lowered.contains("accept: application/json"), "Accept header: {recorded}");
    assert!(lowered.contains("user-agent: opencode/"), "app User-Agent: {recorded}");
}

/// ② — the branded loopback callback: success (200) and OAuth-error
/// (400) both answer the shared page module's HTML.
#[tokio::test]
async fn g234_loopback_serves_branded_success_and_error_pages() {
    async fn get(url: &str) -> (String, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(url).await.expect("connect");
        stream.write_all(b"GET /callback HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").await.expect("write");
        let mut body = Vec::new();
        stream.read_to_end(&mut body).await.expect("read");
        let text = String::from_utf8_lossy(&body).into_owned();
        let status = text.split_whitespace().nth(1).unwrap_or_default().to_string();
        (status, text)
    }

    // Success redirect: the branded success card, 200.
    let listener = super::flows::listen_loopback(super::flows::LoopbackOptions {
        page: super::flows::CallbackPage::Branded { provider: "Poe".to_owned() },
        ..Default::default()
    })
    .await
    .expect("listener");
    let (status, text) = get(&format!("127.0.0.1:{}", listener.port)).await;
    assert_eq!(status, "200");
    assert!(text.contains("Authorization successful"), "branded success: {text}");
    assert!(text.contains("OpenCode is now connected to Poe."), "provider subtitle: {text}");
    assert!(text.contains("oc-card"), "branded document markers");

    // Error redirect: the branded error card, 400, with the detail.
    let listener = super::flows::listen_loopback(super::flows::LoopbackOptions {
        page: super::flows::CallbackPage::Branded { provider: "Poe".to_owned() },
        ..Default::default()
    })
    .await
    .expect("listener 2");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", listener.port))
        .await
        .expect("connect 2");
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(b"GET /callback?error=access_denied&error_description=nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .expect("write 2");
    let mut body = Vec::new();
    stream.read_to_end(&mut body).await.expect("read 2");
    let text = String::from_utf8_lossy(&body).into_owned();
    assert!(text.starts_with("HTTP/1.1 400"), "error status: {text}");
    assert!(text.contains("Authorization failed"), "branded error: {text}");
    assert!(text.contains("nope"), "error detail surfaced: {text}");
    // And the flow still observes the failure params.
    let params = listener.wait().await.expect("hit");
    assert_eq!(params.get("error").map(String::as_str), Some("access_denied"));
}

/// ② — the implicit-flow loopback serves the shared bootstrap page.
#[tokio::test]
async fn g234_loopback_bootstrap_is_the_shared_page() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = super::flows::listen_loopback(super::flows::LoopbackOptions {
        page: super::flows::CallbackPage::Bootstrap {
            token_path: "/callback/token".to_owned(),
            provider: "xAI".to_owned(),
        },
        ..Default::default()
    })
    .await
    .expect("listener");
    let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", listener.port))
        .await
        .expect("connect");
    stream
        .write_all(b"GET /callback HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .expect("write");
    let mut body = Vec::new();
    stream.read_to_end(&mut body).await.expect("read");
    let text = String::from_utf8_lossy(&body).into_owned();
    assert!(text.starts_with("HTTP/1.1 200"), "status: {text}");
    assert!(text.contains("Finishing sign-in"), "branded pending card: {text}");
    assert!(text.contains("Completing your xAI authorization."));
    assert!(text.contains("var TOKEN_URL=new URL(\"/callback/token\",window.location.origin).href;"));
}

// -- G-260③: host-side form validation (ref form.ts:238-360) ------------------------

fn form_field(key: &str, required: bool) -> super::types::FormField {
    super::types::FormField {
        type_: "string".into(),
        key: key.into(),
        title: None,
        description: None,
        placeholder: None,
        default: None,
        pattern: None,
        required,
        hidden: false,
        options: None,
        when: None,
        format: None,
        min_length: None,
        max_length: None,
        custom: None,
    }
}

/// G-260③ (MANDATED) — `neq` conditions: a field conditioned `neq` on
/// another is visible exactly when the values DIFFER; the old port only
/// recognized `op == "eq"`, so every `neq`-conditioned field was judged
/// invisible and silently hidden.
#[test]
fn g260_field_visible_handles_neq() {
    let mut field = form_field("advanced", false);
    field.when = Some(vec![super::types::FormCondition {
        key: "plan".into(),
        op: "neq".into(),
        value: Value::String("free".into()),
    }]);
    let form = vec![form_field("plan", false), field];
    // plan=free → neq fails → invisible: answering it rejects.
    let mut answer: FormAnswer = FormAnswer::new();
    answer.insert("plan".into(), Value::String("free".into()));
    answer.insert("advanced".into(), Value::String("yes".into()));
    assert_eq!(
        super::validate_form(&form, &answer),
        Some("Form field is not active: advanced".to_owned())
    );
    // plan=pro → neq holds → visible and answerable.
    let mut answer: FormAnswer = FormAnswer::new();
    answer.insert("plan".into(), Value::String("pro".into()));
    answer.insert("advanced".into(), Value::String("yes".into()));
    assert_eq!(super::validate_form(&form, &answer), None);
    // An UNANSWERED referenced field is false for both ops (form.ts:273).
    let mut answer: FormAnswer = FormAnswer::new();
    answer.insert("advanced".into(), Value::String("yes".into()));
    assert_eq!(
        super::validate_form(&form, &answer),
        Some("Form field is not active: advanced".to_owned())
    );
}

/// Unknown answer keys reject (form.ts:240-242).
#[test]
fn g260_unknown_form_field_rejects() {
    let mut answer: FormAnswer = FormAnswer::new();
    answer.insert("nope".into(), Value::String("x".into()));
    assert_eq!(
        super::validate_form(&[form_field("known", false)], &answer),
        Some("Unknown form field: nope".to_owned())
    );
}

/// The options closed set (form.ts:335-337) — open with `custom: true`.
#[test]
fn g260_options_form_a_closed_set() {
    let mut field = form_field("region", true);
    field.options = Some(vec![super::types::FormSelectOption {
        label: "EU".into(),
        value: "eu".into(),
        description: None,
    }]);
    let mut answer: FormAnswer = FormAnswer::new();
    answer.insert("region".into(), Value::String("mars".into()));
    assert_eq!(
        super::validate_form(&[field.clone()], &answer),
        Some("Invalid option for form field: region".to_owned())
    );
    field.custom = Some(true);
    assert_eq!(super::validate_form(&[field], &answer), None);
}

/// minLength + format checks (form.ts:317-331), pattern message included.
#[test]
fn g260_min_length_format_and_pattern_checks() {
    let mut field = form_field("name", true);
    field.min_length = Some(3);
    let mut answer: FormAnswer = FormAnswer::new();
    answer.insert("name".into(), Value::String("ab".into()));
    assert_eq!(
        super::validate_form(&[field.clone()], &answer),
        Some("Form field is too short: name".to_owned())
    );

    field.min_length = None;
    field.format = Some("email".into());
    answer.insert("name".into(), Value::String("not-an-email".into()));
    assert_eq!(
        super::validate_form(&[field.clone()], &answer),
        Some("Expected email for form field: name".to_owned())
    );
    answer.insert("name".into(), Value::String("a@b.co".into()));
    assert_eq!(super::validate_form(&[field.clone()], &answer), None);

    field.format = Some("uri".into());
    answer.insert("name".into(), Value::String("not a uri".into()));
    assert_eq!(
        super::validate_form(&[field.clone()], &answer),
        Some("Expected uri for form field: name".to_owned())
    );

    field.format = None;
    field.pattern = Some(r"^v\d+$".into());
    assert_eq!(
        super::validate_form(&[field.clone()], &answer),
        Some("Form field does not match pattern: name".to_owned())
    );
}

/// Required-missing keeps the ref message; a declared default satisfies.
#[test]
fn g260_required_message_and_default_satisfaction() {
    let mut answer: FormAnswer = FormAnswer::new();
    assert_eq!(
        super::validate_form(&[form_field("token", true)], &answer),
        Some("Missing required form field: token".to_owned())
    );
    let mut defaulted = form_field("token", true);
    defaulted.default = Some(Value::String("fallback".into()));
    assert_eq!(super::validate_form(&[defaulted], &answer), None);
}

// -- G-260④: azure form short-circuit + token cache -------------------------------

#[tokio::test]
async fn g260_azure_form_omitted_and_token_cached_when_configured() {
    let _env = env_guard().await;
    // A fake `az` that COUNTS invocations.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let counter = dir.path().join("count");
    let expires_on = crate::database::migrations::now_ms() / 1000 + 3600;
    let counter_path = counter.display().to_string();
    std::fs::write(
        dir.path().join("az"),
        format!(
            "#!/bin/sh\nprintf x >> {counter_path}\necho '{{\"accessToken\":\"az-cached\",\"expires_on\":{expires_on}}}'"
        ),
    )
    .expect("write az");
    #[allow(unsafe_code)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.path().join("az"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("chmod");
    }
    let old_path = std::env::var("PATH").unwrap_or_default();
    // G-260④ — a configured resource omits the form (azure.ts:121-133).
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("AZURE_RESOURCE_NAME", "my-models");
        std::env::set_var("PATH", format!("{}:{old_path}", dir.path().display()));
    }
    super::providers_enterprise::azure_token_cache_clear_for_test();
    let host = host().await;
    let info = host.get("azure").await.expect("get").expect("present");
    assert!(
        info.methods.iter().any(|method| matches!(
            method,
            super::types::Method::Key { form: None, .. }
        )),
        "the key method carries no form once the resource is configured"
    );

    // No answer at all: the env-configured resource satisfies the flow
    // (azure.ts:155-158).
    let attempt = host
        .oauth_connect("azure", "azure-cli", &FormAnswer::new(), None)
        .await
        .expect("connect");
    let status = wait_complete(&host, "azure", &attempt, 200).await;
    assert!(matches!(status, AttemptStatus::Complete { .. }), "{status:?}");
    let active = host.connection_active("azure").await.expect("active").expect("some");
    let resolved = host.connection_resolve(&active).await.expect("resolve").expect("value");
    let CredentialValue::OAuth(oauth) = resolved else { panic!() };
    assert_eq!(oauth.access, "az-cached");
    assert_eq!(oauth.metadata.as_ref().unwrap()["resourceName"], "my-models");

    // G-260④ — the per-scope cache: a SECOND mint within 5 minutes runs
    // `az` exactly once total (azure.ts:92-105).
    let attempt = host
        .oauth_connect("azure", "azure-cli", &FormAnswer::new(), None)
        .await
        .expect("connect 2");
    let status = wait_complete(&host, "azure", &attempt, 200).await;
    assert!(matches!(status, AttemptStatus::Complete { .. }), "{status:?}");
    let invocations = std::fs::read_to_string(&counter).unwrap_or_default();
    assert_eq!(invocations.len(), 1, "one `az` run across both mints");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("AZURE_RESOURCE_NAME");
        std::env::set_var("PATH", old_path);
    }
}

/// G-234② — the deferred branded page answers an OAuth error redirect
/// with the shared ERROR card (400) immediately (gitlab.ts:196-203).
#[tokio::test]
async fn gitlab_callback_error_redirect_serves_branded_error_card() {
    let _env = env_guard().await;
    let _port = gitlab_port_lock().await;
    if tokio::net::TcpListener::bind(("127.0.0.1", 8080)).await.is_err() {
        eprintln!("skipping: port 8080 busy (gitlab pins it)");
        return;
    }
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("GITLAB_OAUTH_CLIENT_ID", "test-client-id");
    }
    let host = host().await;
    let attempt = host
        .oauth_connect("gitlab", "pkce", &answer(&[("instanceUrl", "https://gitlab.com")]), None)
        .await
        .expect("connect");
    let state = parse_query_param(&attempt.url, "state").expect("state");
    // The listener's accept task races the first GET (bind returns
    // before accept starts) — retry briefly on connection refused.
    let mut callback = None;
    for _ in 0..50 {
        match reqwest::get(format!(
            "http://127.0.0.1:8080/callback?error=access_denied&error_description=User+denied&state={state}"
        ))
        .await
        {
            Ok(response) => {
                callback = Some(response);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
        }
    }
    let callback = callback.expect("callback");
    assert_eq!(callback.status(), 400);
    let text = callback.text().await.expect("body");
    assert!(text.contains("OpenCode couldn't finish connecting to GitLab."), "{text}");
    assert!(text.contains("User+denied"), "{text}");
    let status = wait_complete(&host, "gitlab", &attempt, 100).await;
    assert!(matches!(status, AttemptStatus::Failed { .. }), "{status:?}");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("GITLAB_OAUTH_CLIENT_ID");
    }
}
