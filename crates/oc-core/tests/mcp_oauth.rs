//! PLUG-D30 — the remote MCP OAuth login flow (port of
//! `packages/core/src/mcp/oauth.ts`): discovery through the resource
//! server's 401 challenge, dynamic client registration, PKCE, the
//! loopback callback, the authorization-code exchange, credential
//! persistence, the connect-time bearer, refresh with rotation, and
//! the credential-switch reconnect.
//!
//! Hermetic: a stub authorization + MCP server on `127.0.0.1:0`, an
//! in-memory database, the PRODUCTION connector (real rmcp streamable
//! HTTP client), no real network.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use oc_core::integration::types::CredentialValue;
use oc_core::mcp::oauth::{self, AuthMode, PreparedToken};
use oc_core::mcp::registry::CredentialStore as _;
use oc_core::mcp::registry::{integration_suffix, McpRegistry};
use oc_schema::mcp::{RemoteConfig, ServerConfig, Status};

fn remote(url: &str) -> RemoteConfig {
    RemoteConfig {
        url: url.to_owned(),
        headers: None,
        oauth: None,
        disabled: None,
        codemode: None,
        timeout: None,
        protocol: None,
    }
}

// ---------------------------------------------------------------------------
// The stub: one HTTP listener serving the authorization server AND the
// resource (MCP) server, routed by path.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StubState {
    /// Bearer tokens the MCP endpoint accepted.
    bearers: Vec<String>,
    /// Authorization-code grants (verifier, `client_id`).
    code_grants: Vec<(String, String)>,
    /// Refresh grants (presented refresh token).
    refresh_grants: Vec<String>,
    /// The `code_challenge` the authorization request carried.
    code_challenge: Option<String>,
    /// Unauthenticated initialize probes answered with the challenge.
    challenged: usize,
    /// DCR `POST /register` hits.
    register_hits: usize,
    /// Advertise `client_id_metadata_document_supported`.
    cimd: bool,
    /// Overrides for `token_endpoint_auth_methods_supported` (None =
    /// omit the field entirely).
    auth_methods: Option<Vec<String>>,
    /// Drop `registration_endpoint` from the metadata (a CIMD-only
    /// server with no open DCR).
    omit_registration: bool,
}

struct Stub {
    base: String,
    state: Arc<Mutex<StubState>>,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// One parsed HTTP request.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn query(&self) -> BTreeMap<String, String> {
        let Some((_, query)) = self.target.split_once('?') else { return BTreeMap::new() };
        query
            .split('&')
            .filter_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                Some((key.to_owned(), urldecode(value)))
            })
            .collect()
    }

    fn form(&self) -> BTreeMap<String, String> {
        self.body
            .split('&')
            .filter_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                Some((key.to_owned(), urldecode(value)))
            })
            .collect()
    }
}

fn urldecode(value: &str) -> String {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    index += 3;
                } else {
                    out.push(bytes[index]);
                    index += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Request {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let read = stream.read(&mut chunk).await.unwrap_or(0);
        if read == 0 {
            break None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(at) = find_subslice(&buffer, b"\r\n\r\n") {
            break Some(at + 4);
        }
    };
    let Some(header_end) = header_end else {
        return Request {
            method: String::new(),
            target: "/".into(),
            headers: Vec::new(),
            body: String::new(),
        };
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_owned();
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or("").to_owned();
    let target = parts.next().unwrap_or("/").to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_owned(), value.trim().to_owned()))
        })
        .collect();
    let content_length = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let read = stream.read(&mut chunk).await.unwrap_or(0);
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8_lossy(&buffer[header_end..]).into_owned();
    Request { method, target, headers, body }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

async fn respond(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &str,
) {
    let mut response = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        let _ = write!(response, "{name}: {value}\r\n");
    }
    let _ = write!(response, "Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn respond_json(stream: &mut tokio::net::TcpStream, status: &str, body: &Value) {
    let text = body.to_string();
    respond(stream, status, &[("Content-Type", "application/json")], &text).await;
}

/// Boots the stub authorization + MCP server on `127.0.0.1:0`.
async fn stub() -> Stub {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("stub bind");
    let port = listener.local_addr().expect("addr").port();
    let base = format!("http://127.0.0.1:{port}");
    let state = Arc::new(Mutex::new(StubState::default()));
    let state_for_task = Arc::clone(&state);
    let base_for_task = base.clone();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let state = Arc::clone(&state_for_task);
            let base = base_for_task.clone();
            tokio::spawn(async move {
                let request = read_request(&mut stream).await;
                serve(&mut stream, &request, &base, &state).await;
            });
        }
    });
    Stub { base, state, handle }
}

async fn serve(
    stream: &mut tokio::net::TcpStream,
    request: &Request,
    base: &str,
    state: &Arc<Mutex<StubState>>,
) {
    let path = request.target.split('?').next().unwrap_or("/");
    if std::env::var("MCP_OAUTH_STUB_TRACE").is_ok() {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let started = START.get_or_init(std::time::Instant::now);
        eprintln!(
            "[stub t+{:>7.2}s] {} {} (body {} bytes) {}",
            started.elapsed().as_secs_f32(),
            request.method,
            request.target,
            request.body.len(),
            request.body
        );
    }
    match (request.method.as_str(), path) {
        ("POST", "/mcp") => serve_mcp(stream, request, base, state).await,
        ("GET", "/.well-known/oauth-protected-resource") => {
            respond_json(stream, "200 OK", &json!({ "authorization_servers": [base] })).await;
        }
        ("GET", "/.well-known/oauth-authorization-server") => {
            let (cimd, auth_methods, omit_registration) = {
                let state = state.lock();
                (state.cimd, state.auth_methods.clone(), state.omit_registration)
            };
            let mut metadata = json!({
                "issuer": base,
                "authorization_endpoint": format!("{base}/authorize"),
                "token_endpoint": format!("{base}/token"),
                "scopes_supported": ["mcp:stub"],
            });
            if !omit_registration {
                metadata["registration_endpoint"] = json!(format!("{base}/register"));
            }
            if cimd {
                metadata["client_id_metadata_document_supported"] = json!(true);
            }
            if let Some(methods) = &auth_methods {
                metadata["token_endpoint_auth_methods_supported"] = json!(methods);
            }
            respond_json(stream, "200 OK", &metadata).await;
        }
        ("POST", "/register") => {
            state.lock().register_hits += 1;
            respond_json(stream, "200 OK", &json!({ "client_id": "stub-client" })).await;
        }
        ("GET", "/authorize") => {
            let query = request.query();
            state.lock().code_challenge = query.get("code_challenge").cloned();
            let redirect = query.get("redirect_uri").cloned().unwrap_or_default();
            let state_value = query.get("state").cloned().unwrap_or_default();
            let separator = if redirect.contains('?') { '&' } else { '?' };
            respond(
                stream,
                "302 Found",
                &[(
                    "Location",
                    &format!("{redirect}{separator}code=stub-authz-code&state={state_value}"),
                )],
                "",
            )
            .await;
        }
        ("POST", "/token") => serve_token(stream, request, state).await,
        _ => {
            respond(stream, "404 Not Found", &[], "Not found").await;
        }
    }
}

// A scripted MCP+OAuth peer: one linear request loop (initialize → 401 →
// re-auth → tools/list → expiry → refresh), kept whole so the wire
// choreography reads top-to-bottom.
#[allow(clippy::too_many_lines)]
async fn serve_mcp(
    stream: &mut tokio::net::TcpStream,
    request: &Request,
    base: &str,
    state: &Arc<Mutex<StubState>>,
) {
    let bearer = request
        .header("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned);
    let Some(bearer) = bearer else {
        state.lock().challenged += 1;
        respond(
            stream,
            "401 Unauthorized",
            &[
                (
                    "WWW-Authenticate",
                    &format!(
                        "Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\", scope=\"mcp:stub\""
                    ),
                ),
                ("Content-Type", "application/json"),
            ],
            "{\"error\":\"unauthorized\"}",
        )
        .await;
        return;
    };
    state.lock().bearers.push(bearer);
    let body: Value = serde_json::from_str(&request.body).unwrap_or(Value::Null);
    let method = body.get("method").and_then(Value::as_str).unwrap_or_default();
    let id = body.get("id").cloned();
    match method {
        "initialize" => {
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "jsonrpc": "2.0",
                    "id": id.unwrap_or(json!(0)),
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": { "name": "stub-mcp", "version": "1" },
                        "instructions": "stub oauth server"
                    }
                }),
            )
            .await;
        }
        "tools/list" => {
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "jsonrpc": "2.0",
                    "id": id.unwrap_or(json!(0)),
                    "result": { "tools": [ {
                        "name": "stub_tool",
                        "description": "A stub tool",
                        "inputSchema": { "type": "object", "properties": {} }
                    } ] }
                }),
            )
            .await;
        }
        "resources/list" => {
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "jsonrpc": "2.0",
                    "id": id.unwrap_or(json!(0)),
                    "result": { "resources": [] }
                }),
            )
            .await;
        }
        "resources/templates/list" => {
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "jsonrpc": "2.0",
                    "id": id.unwrap_or(json!(0)),
                    "result": { "resourceTemplates": [] }
                }),
            )
            .await;
        }
        "prompts/list" => {
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "jsonrpc": "2.0",
                    "id": id.unwrap_or(json!(0)),
                    "result": { "prompts": [] }
                }),
            )
            .await;
        }
        // notifications carry no id; the streamable protocol answer is 202.
        _ => {
            respond(stream, "202 Accepted", &[], "").await;
        }
    }
}

async fn serve_token(
    stream: &mut tokio::net::TcpStream,
    request: &Request,
    state: &Arc<Mutex<StubState>>,
) {
    let form = request.form();
    match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            if form.get("code").map(String::as_str) != Some("stub-authz-code") {
                respond_json(
                    stream,
                    "400 Bad Request",
                    &json!({ "error": "invalid_grant", "error_description": "unknown code" }),
                )
                .await;
                return;
            }
            let verifier = form.get("code_verifier").cloned().unwrap_or_default();
            let client_id = form.get("client_id").cloned().unwrap_or_default();
            if verifier.is_empty() {
                respond_json(
                    stream,
                    "400 Bad Request",
                    &json!({ "error": "invalid_request", "error_description": "PKCE code_verifier required" }),
                )
                .await;
                return;
            }
            state.lock().code_grants.push((verifier, client_id));
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "access_token": "stub-access-1",
                    "refresh_token": "stub-refresh-1",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "scope": "mcp:stub",
                }),
            )
            .await;
        }
        Some("refresh_token") => {
            let presented = form.get("refresh_token").cloned().unwrap_or_default();
            state.lock().refresh_grants.push(presented);
            respond_json(
                stream,
                "200 OK",
                &json!({
                    "access_token": "stub-access-2",
                    "refresh_token": "stub-refresh-2",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "scope": "mcp:stub",
                }),
            )
            .await;
        }
        _ => {
            respond_json(stream, "400 Bad Request", &json!({ "error": "unsupported_grant_type" }))
                .await;
        }
    }
}

// ---------------------------------------------------------------------------
// The flow under test
// ---------------------------------------------------------------------------

async fn wait_until(
    registry: &McpRegistry,
    server: &str,
    predicate: impl Fn(&Status) -> bool,
    what: &str,
) {
    for _ in 0..400 {
        let found =
            registry.servers().into_iter().find(|info| info.name == server).map(|info| info.status);
        if found.as_ref().is_some_and(&predicate) {
            return;
        }
        // async sleep: start_server runs on spawned tasks that a blocking
        // sleep would starve on the current-thread test runtime.
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("server {server} never reached {what}: {:?}", registry.servers());
}

async fn open_test_db() -> oc_core::database::Database {
    oc_core::database::Database::open_in_memory().await.expect("database")
}

/// The CLI-equivalent credential write: an active `credential` row for
/// the derived integration id (what `POST /api/credential` persists).
async fn store_credential(
    db: &oc_core::database::Database,
    integration: &str,
    value: &CredentialValue,
) {
    let encoded = serde_json::to_string(value).expect("credential serializes");
    let integration = integration.to_owned();
    db.transaction(move |tx| {
        tx.conn()
            .execute(
                "INSERT OR REPLACE INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
                rusqlite::params![
                    format!("cred_{integration}"),
                    integration,
                    "stub mcp",
                    encoded,
                    oc_core::database::migrations::now_ms()
                ],
            )
            .map_err(oc_core::database::DbError::Sqlite)?;
        Ok(())
    })
    .await
    .expect("credential stored");
}

async fn read_credential_value(
    db: &oc_core::database::Database,
    integration: &str,
) -> CredentialValue {
    let integration = integration.to_owned();
    let raw = db
        .transaction(move |tx| {
            tx.conn()
                .query_row(
                    "SELECT value FROM credential WHERE integration_id = ?1",
                    rusqlite::params![integration],
                    |row| row.get::<_, String>(0),
                )
                .map_err(oc_core::database::DbError::Sqlite)
        })
        .await
        .expect("credential read");
    serde_json::from_str(&raw).expect("credential value parses")
}

/// The full OAuth login + connect: 401 dead-end → authorize → callback
/// → exchange → stored credential → reconnect with the bearer → expiry
/// → refresh rotation.
// End-to-end scripted flow (stub → challenge → DCR → PKCE → callback →
// exchange → stored credential → refresh); splitting would hide the
// sequence under test.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn oauth_connect() {
    let stub = stub().await;
    let server_url = format!("{}/mcp", stub.base);
    let name = "oauthstub";
    let integration = integration_suffix(name, &server_url);
    let config = remote(&server_url);

    let db = open_test_db().await;
    let bus = oc_core::bus::Bus::new(db.clone(), oc_core::bus::BusOptions::deterministic());
    let registry = McpRegistry::new(bus, db.clone())
        .with_config_source(Arc::new(move || {
            BTreeMap::from([(
                name.to_string(),
                ServerConfig::Remote(Box::new(remote(&server_url))),
            )])
        }))
        .with_directory("/tmp");

    // (1) The pre-login dead-end: no credential, the resource server
    // answers 401, the server settles needs_auth (the v2 connect
    // without a login).
    registry.reconcile().await;
    wait_until(&registry, name, |status| matches!(status, Status::NeedsAuth { .. }), "needs_auth")
        .await;
    {
        let status = registry.servers().into_iter().find(|info| info.name == name).unwrap().status;
        let Status::NeedsAuth { error } = status else { panic!("needs_auth: {status:?}") };
        assert!(
            error.contains("401") || error.contains("Unauthorized"),
            "the 401 names the failure: {error}"
        );
    }
    assert!(stub.state.lock().challenged >= 1, "the resource server saw the unauthenticated probe");

    // (2) The authorize flow (v2 `McpOAuth.authorize`): discovery through
    // the 401 challenge, DCR, PKCE, the loopback listener.
    let authorization = oauth::authorize(oauth::AuthorizeInput {
        name: name.to_owned(),
        config: config.clone(),
        method_id: integration.clone(),
        http: reqwest::Client::new(),
    })
    .await
    .expect("the authorize flow builds");
    assert!(
        authorization.url.starts_with(&format!("{}/authorize?", stub.base)),
        "the authorization URL targets the stub's endpoint: {}",
        authorization.url
    );
    let AuthMode::Auto(callback) = authorization.mode else {
        panic!("the loopback flow is the auto mode");
    };

    // The browser: hit the authorization URL (no redirect follow — the
    // "browser" then lands on the loopback redirect).
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("browser client");
    let response = browser.get(&authorization.url).send().await.expect("authorize GET");
    assert_eq!(response.status(), 302, "the stub redirects to the loopback callback");
    let redirect = response.headers()["location"].clone();
    let redirect = redirect.to_str().expect("location").to_owned();
    assert!(redirect.starts_with("http://127.0.0.1:"), "the redirect is the loopback: {redirect}");

    // The callback lands on the flow's listener; the exchange resolves
    // the credential.
    let callback = tokio::spawn(callback);
    let page = reqwest::Client::new().get(&redirect).send().await.expect("callback GET");
    assert_eq!(page.status(), 200, "the callback page answers success");
    let credential = callback.await.expect("callback task").expect("the exchange resolves");
    let CredentialValue::OAuth(value) = &credential else { panic!("oauth credential") };
    assert_eq!(value.access, "stub-access-1");
    assert_eq!(value.refresh, "stub-refresh-1");
    assert!(value.expires > 0, "the stub's expires_in became an absolute deadline");

    // PKCE + the registered client rode the grant.
    {
        let state = stub.state.lock();
        let (verifier, client_id) = &state.code_grants[0];
        assert!(!verifier.is_empty(), "the code_verifier was presented");
        assert_eq!(client_id, "stub-client", "the DCR-registered client exchanged the code");
        let challenge =
            state.code_challenge.as_deref().expect("the authorize request carried the challenge");
        assert_eq!(challenge.len(), 43, "S256 challenges are 43 base64url chars");
    }

    // (3) The credential persists (the CLI's POST /api/credential leg)
    // and the reconnect carries the bearer: Connected.
    store_credential(&db, &integration, &credential).await;
    registry.connect(name).await.expect("reconnect");
    wait_until(&registry, name, |status| matches!(status, Status::Connected), "connected").await;
    {
        let tools = registry.tools();
        assert!(
            tools.iter().any(|tool| tool.name == "stub_tool"),
            "the production connector discovered the stub's tool: {tools:?}"
        );
    }
    assert!(
        stub.state.lock().bearers.iter().any(|bearer| bearer == "stub-access-1"),
        "the MCP endpoint saw the exchanged bearer"
    );

    // (4) Expiry → the connect-time refresh rotates the tokens and the
    // rotated bearer reaches the server (the connectProvider contract).
    let CredentialValue::OAuth(mut aged) = read_credential_value(&db, &integration).await else {
        panic!("oauth row")
    };
    aged.expires = oc_core::database::migrations::now_ms() - 1000; // past expiry
    store_credential(&db, &integration, &CredentialValue::OAuth(aged)).await;

    // The refresh decision itself (prepare_token): the stored row is
    // past expiry, the grant must rotate.
    let http = reqwest::Client::new();
    let stored = oc_core::mcp::registry::DatabaseCredentials::new(db.clone())
        .oauth_credential(&oc_schema::id::IntegrationId(integration.clone()))
        .await
        .expect("the stored credential reads back");
    match oauth::prepare_token(&http, &config, &stored).await {
        Ok(PreparedToken::Refreshed { access, tokens }) => {
            assert_eq!(access, "stub-access-2");
            assert_eq!(tokens.refresh_token.as_deref(), Some("stub-refresh-2"));
        }
        other => panic!("the expired credential refreshes: {other:?}"),
    }
    assert_eq!(
        stub.state.lock().refresh_grants.last().map(String::as_str),
        Some("stub-refresh-1"),
        "the rotation presented the previous refresh token"
    );

    // The reconnect path persists the rotation and connects with it.
    registry.connect(name).await.expect("reconnect after expiry");
    wait_until(&registry, name, |status| matches!(status, Status::Connected), "connected").await;
    assert!(
        stub.state.lock().bearers.iter().any(|bearer| bearer == "stub-access-2"),
        "the rotated bearer reached the MCP endpoint"
    );
    let CredentialValue::OAuth(rotated) = read_credential_value(&db, &integration).await else {
        panic!("oauth row")
    };
    assert_eq!(rotated.access, "stub-access-2", "the rotation was persisted");
    assert_eq!(rotated.refresh, "stub-refresh-2", "the refresh token rotated on the row");
}

// ---------------------------------------------------------------------------
// RD-04 — CIMD (client_id_metadata_document_supported, oauth.ts:431-445)
// ---------------------------------------------------------------------------

/// Drives authorize → browser redirect → loopback callback → exchange,
/// returning the resolved credential.
async fn login(authorization: oauth::Authorization) -> CredentialValue {
    let AuthMode::Auto(callback) = authorization.mode else {
        panic!("the loopback flow is the auto mode");
    };
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("browser client");
    let response = browser.get(&authorization.url).send().await.expect("authorize GET");
    assert_eq!(response.status(), 302, "the stub redirects to the loopback callback");
    let redirect = response.headers()["location"].to_str().expect("location").to_owned();
    let callback = tokio::spawn(callback);
    let page = reqwest::Client::new().get(&redirect).send().await.expect("callback GET");
    assert_eq!(page.status(), 200, "the callback page answers success");
    callback.await.expect("callback task").expect("the exchange resolves")
}

/// A CIMD-advertised server: DCR is skipped (no `/register` hit), the
/// client id IS the metadata-document URL (oauth.ts:431-445 + SDK 2.0.0
/// `auth()`: `client_id: clientMetadataUrl`), and the login completes
/// end to end without any registration request.
#[tokio::test]
async fn cimd_server_skips_dcr_and_uses_the_metadata_document_client_id() {
    let stub = stub().await;
    {
        let mut state = stub.state.lock();
        state.cimd = true;
        state.auth_methods = Some(vec!["none".to_owned()]);
        state.omit_registration = true; // CIMD-only server: no open DCR at all
    }
    let server_url = format!("{}/mcp", stub.base);
    let authorization = oauth::authorize(oauth::AuthorizeInput {
        name: "cimdstub".to_owned(),
        config: remote(&server_url),
        method_id: "mcp_cimd".to_owned(),
        http: reqwest::Client::new(),
    })
    .await
    .expect("the authorize flow builds without a registration endpoint");

    assert!(
        authorization
            .url
            .contains(&format!("client_id={}", urlencoding::encode(oauth::CLIENT_METADATA_URL)))
            || authorization
                .url
                .contains("client_id=https%3A%2F%2Fopencode.ai%2Foauth%2Fopencode%2Fclient.json"),
        "the authorization URL carries the metadata-document client_id: {}",
        authorization.url
    );

    let credential = login(authorization).await;
    let CredentialValue::OAuth(value) = &credential else { panic!("oauth credential") };
    assert_eq!(value.access, "stub-access-1", "the CIMD login exchanged the code");

    let state = stub.state.lock();
    assert_eq!(state.register_hits, 0, "CIMD pre-selection skips dynamic client registration");
    assert_eq!(
        state.code_grants[0].1,
        oauth::CLIENT_METADATA_URL,
        "the token exchange named the metadata-document client_id"
    );
    // The credential row keeps the CIMD client so refresh presents it too.
    let client = value.metadata.as_ref().unwrap().get("client").cloned().unwrap_or_default();
    assert_eq!(client["client_id"], json!(oauth::CLIENT_METADATA_URL));
}

/// A DCR-only server (no CIMD flag) is unchanged: `/register` answers
/// and the registered client exchanges the code (oauth.ts:438 "dcr").
#[tokio::test]
async fn dcr_only_server_still_registers() {
    let stub = stub().await; // no cimd flags: plain DCR metadata
    let server_url = format!("{}/mcp", stub.base);
    let authorization = oauth::authorize(oauth::AuthorizeInput {
        name: "dcrstub".to_owned(),
        config: remote(&server_url),
        method_id: "mcp_dcr".to_owned(),
        http: reqwest::Client::new(),
    })
    .await
    .expect("the authorize flow builds");

    let credential = login(authorization).await;
    let CredentialValue::OAuth(value) = &credential else { panic!("oauth credential") };
    assert_eq!(value.access, "stub-access-1");
    let state = stub.state.lock();
    assert_eq!(state.register_hits, 1, "the DCR leg posted /register exactly once");
    assert_eq!(state.code_grants[0].1, "stub-client");
}

/// CIMD advertised but the token endpoint does not accept public
/// clients (`none` missing from `token_endpoint_auth_methods_supported`):
/// the flow falls back to DCR (oauth.ts:435 gate).
#[tokio::test]
async fn cimd_without_public_client_falls_back_to_dcr() {
    let stub = stub().await;
    {
        let mut state = stub.state.lock();
        state.cimd = true;
        state.auth_methods = Some(vec!["client_secret_post".to_owned()]);
    }
    let server_url = format!("{}/mcp", stub.base);
    let authorization = oauth::authorize(oauth::AuthorizeInput {
        name: "cimdconf".to_owned(),
        config: remote(&server_url),
        method_id: "mcp_cimd_conf".to_owned(),
        http: reqwest::Client::new(),
    })
    .await
    .expect("the authorize flow builds");

    let credential = login(authorization).await;
    let CredentialValue::OAuth(value) = &credential else { panic!("oauth credential") };
    assert_eq!(value.access, "stub-access-1");
    let state = stub.state.lock();
    assert_eq!(state.register_hits, 1, "no `none` auth method: DCR runs instead of CIMD");
    assert_eq!(state.code_grants[0].1, "stub-client");
}

/// Failure leg: a server that neither advertises usable CIMD nor
/// publishes a registration endpoint cannot serve any client id — the
/// flow fails with the v2-shaped registration error.
#[tokio::test]
async fn cimd_only_server_with_confined_redirect_uri_fails_without_dcr() {
    let stub = stub().await;
    {
        let mut state = stub.state.lock();
        state.cimd = true;
        state.omit_registration = true;
        // Public-client method missing → CIMD gate closed, DCR wanted,
        // but the CIMD-only server publishes no registration endpoint.
    }
    let server_url = format!("{}/mcp", stub.base);
    let error = oauth::authorize(oauth::AuthorizeInput {
        name: "cimdfail".to_owned(),
        config: remote(&server_url),
        method_id: "mcp_cimd_fail".to_owned(),
        http: reqwest::Client::new(),
    })
    .await
    .err()
    .expect("no CIMD pre-selection and no registration endpoint");
    assert!(
        error.contains("registration_endpoint"),
        "the failure names the missing registration endpoint: {error}"
    );
    assert_eq!(stub.state.lock().register_hits, 0);
}

/// A configured `redirect_uri` disables CIMD pre-selection even when
/// the server advertises it (oauth.ts:433 — the published metadata
/// document only lists the loopback redirect).
#[tokio::test]
async fn configured_redirect_uri_disables_cimd() {
    let stub = stub().await;
    {
        let mut state = stub.state.lock();
        state.cimd = true;
        state.auth_methods = Some(vec!["none".to_owned()]);
        state.omit_registration = true;
    }
    let server_url = format!("{}/mcp", stub.base);
    // Reserve a free port for the pinned redirect_uri (binding it is the
    // flow's job; pointing at the stub's own port would be a different
    // failure).
    let reserved = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("reserve");
    let free_port = reserved.local_addr().expect("addr").port();
    drop(reserved);
    let mut config = remote(&server_url);
    config.oauth = Some(oc_schema::mcp::OAuthValue::Config(oc_schema::mcp::OAuthConfig {
        redirect_uri: Some(format!("http://127.0.0.1:{free_port}/cb")),
        ..Default::default()
    }));
    let result = oauth::authorize(oauth::AuthorizeInput {
        name: "cimdredir".to_owned(),
        config,
        method_id: "mcp_cimd_redir".to_owned(),
        http: reqwest::Client::new(),
    })
    .await;
    // The pinned redirect forces DCR, which this CIMD-only server
    // cannot serve: the same v2-shaped registration failure.
    match result {
        Err(error) => {
            assert!(error.contains("registration_endpoint"), "{error}");
            assert_eq!(stub.state.lock().register_hits, 0);
        }
        Ok(_) => panic!("a pinned redirect_uri must not take the CIMD path"),
    }
}
