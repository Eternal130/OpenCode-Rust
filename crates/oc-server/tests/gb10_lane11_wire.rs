//! gb10-lane11 wire-semantics regressions:
//! - G-264 — `permission.saved.list` defaults to the REQUEST location's
//!   project when `projectID` is absent.
//! - G-268 — credential create/list parity with the core store
//!   (first-credential activation, label default, selected-last listing,
//!   decode-fail-the-read, duplicate-id `ConflictError`).
//! - G-270① — agent.list returns EVERY agent with the default first.
//! - G-270② — reference.list reads the assembled per-location service.
//! - G-270③ — fs.find returns location-relative paths.
//! - G-270④ — vcs.init error mapping (unknown provider 400 / conflict
//!   resource / success side effects).
//! - G-270⑤ — /api/info carries `capabilities` only when ready.
//! - G-270⑥ — pty.connect 404s an unresolvable location pre-upgrade.
//! - G-270⑦ — worktree.list serves any KNOWN project's rows.
//! - G-270⑧ — a duplicate form create id is `ConflictError`.

mod support;

use std::sync::Arc;

use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::{json, Value};

use support::request;

struct TestServer {
    addr: std::net::SocketAddr,
    workspace: std::path::PathBuf,
    state: Arc<AppState>,
    _workspace: tempfile::TempDir,
    _data: tempfile::TempDir,
}

/// In-process server over a fresh workspace (no app shell: the plain
/// handler registry — the surfaces under test live there).
async fn boot() -> TestServer {
    // Hermetic (R3): the in-process pty backend — the parity tests'
    // one-release escape hatch (no daemon spawn from these tests).
    std::env::set_var("OC_PTY_DAEMON", "0");
    let workspace = tempfile::tempdir_in("/tmp/opencode").expect("workspace tempdir");
    let data = tempfile::tempdir_in("/tmp/opencode").expect("data tempdir");
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
    TestServer {
        addr,
        workspace: workspace.path().to_path_buf(),
        state,
        _workspace: workspace,
        _data: data,
    }
}

fn workspace_headers(server: &TestServer) -> Vec<(&'static str, String)> {
    vec![("x-opencode-directory", server.workspace.display().to_string())]
}

fn owned_headers<'a>(headers: &'a [(&'a str, String)]) -> Vec<(&'a str, &'a str)> {
    headers.iter().map(|(k, v)| (*k, v.as_str())).collect()
}
async fn get(server: &TestServer, target: &str) -> support::RawResponse {
    let headers = workspace_headers(server);
    request(server.addr, "GET", target, &owned_headers(&headers), &[]).await.expect("GET")
}

async fn post_json(server: &TestServer, target: &str, body: &Value) -> support::RawResponse {
    let headers = workspace_headers(server);
    let mut owned = owned_headers(&headers);
    owned.push(("content-type", "application/json"));
    request(server.addr, "POST", target, &owned, body.to_string().as_bytes()).await.expect("POST")
}

/// The project id the resolver derives for the workspace (same machinery
/// the G-264 default runs).
async fn workspace_project_id(server: &TestServer) -> String {
    let resolver = oc_core::project::Resolver::new(
        Arc::clone(&server.state.engine.projects) as Arc<dyn oc_core::project::ProjectStore>,
        Arc::new(oc_core::project::NoEvents),
    );
    resolver.resolve(&server.workspace).await.id
}

// ---------------------------------------------------------------------------
// G-264
// ---------------------------------------------------------------------------

#[tokio::test]
async fn permission_saved_list_defaults_to_the_request_locations_project() {
    let server = boot().await;
    let project = workspace_project_id(&server).await;
    let expected_project = project.clone();
    let other = "some-other-project".to_owned();
    // Seed one saved grant for the workspace's project and one for a
    // different project directly in the store (FKs: both project rows).
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute(
                "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) \
                 VALUES (?2, '/tmp/opencode/other-wt', 1, 1, '')",
                (&project, &other),
            )?;
            tx.execute(
                "INSERT INTO permission (id, project_id, action, resource, time_created, time_updated) \
                 VALUES ('per_here', ?1, 'bash', 'rm -rf *', 1, 1), \
                        ('per_there', ?2, 'bash', 'ls', 1, 1)",
                (&project, &other),
            )?;
            Ok(())
        })
        .await
        .expect("seed rows");

    // No query: ONLY the request location's project rows.
    let response = get(&server, "/api/permission/saved").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json()["data"].as_array().expect("data").clone();
    assert_eq!(data.len(), 1, "only the current project: {data:?}");
    assert_eq!(data[0]["id"], json!("per_here"));
    assert_eq!(data[0]["projectID"], json!(expected_project));

    // Explicit query still addresses ANY project (unchanged surface).
    let response = get(&server, "/api/permission/saved?projectID=some-other-project").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json()["data"].as_array().expect("data").clone();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], json!("per_there"));
}

// ---------------------------------------------------------------------------
// G-268
// ---------------------------------------------------------------------------

fn key_credential(key: &str) -> Value {
    json!({ "type": "key", "key": key })
}

#[tokio::test]
async fn credential_first_create_defaults_active_and_label_default() {
    let server = boot().await;
    // No `activate`, no `label` — ref create: label ?? "default", and
    // `activate !== false || !current` makes the FIRST credential active.
    let response = post_json(
        &server,
        "/api/credential",
        &json!({ "integrationID": "dummy", "value": key_credential("k1") }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let created = response.json()["data"].clone();
    assert_eq!(created["label"], json!("default"), "{created}");
    assert_eq!(created["active"], json!(true), "first credential must activate: {created}");
    // The response keeps the port's `time` wire superset (G-270⑩).
    assert!(created["time"]["created"].is_u64(), "{created}");

    // A second credential with activate=false stays inactive and keeps
    // its label; the first stays selected.
    let response = post_json(
        &server,
        "/api/credential",
        &json!({ "integrationID": "dummy", "value": key_credential("k2"), "label": "second", "activate": false }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let second = response.json()["data"].clone();
    assert_eq!(second["label"], json!("second"));
    assert_eq!(second["active"], json!(false));

    // List: rows of the integration, the SELECTED row flagged active
    // (selected-last — the newest active row wins).
    let response = get(&server, "/api/credential").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let rows = response.json()["data"].as_array().expect("data").clone();
    let dummy: Vec<&Value> =
        rows.iter().filter(|row| row["integrationID"] == json!("dummy")).collect();
    assert_eq!(dummy.len(), 2, "{rows:?}");
    let k1 = dummy.iter().find(|row| row["label"] == json!("default")).expect("k1");
    let k2 = dummy.iter().find(|row| row["label"] == json!("second")).expect("k2");
    assert_eq!(k1["active"], json!(true), "selected row: {rows:?}");
    assert_eq!(k2["active"], json!(false));
    assert!(k1["time"]["created"].is_u64(), "time superset kept: {k1}");
}

#[tokio::test]
async fn credential_first_create_with_activate_false_is_forced_active() {
    let server = boot().await;
    // `activate: false` on an integration WITHOUT a current credential is
    // forced active (credential.ts:121) — otherwise the integration would
    // have no selected credential and be unusable after login.
    let response = post_json(
        &server,
        "/api/credential",
        &json!({ "integrationID": "forced", "value": key_credential("k"), "activate": false }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["data"]["active"], json!(true));

    // Legacy pin: a NULL-active legacy row gets pinned when a newer
    // inactive credential is inserted on top.
    server
        .state
        .engine
        .db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                 VALUES ('cred_legacy', 'legacy', 'default', ?, NULL, 5, 5)",
                (key_credential("old").to_string(),),
            )?;
            Ok(())
        })
        .await
        .expect("seed legacy");
    let response = post_json(
        &server,
        "/api/credential",
        &json!({ "integrationID": "legacy", "value": key_credential("new"), "activate": false }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["data"]["active"], json!(false), "second stays inactive");
    let response = get(&server, "/api/credential").await;
    let rows = response.json()["data"].as_array().expect("data").clone();
    let legacy = rows.iter().find(|row| row["id"] == json!("cred_legacy")).expect("legacy row");
    assert_eq!(legacy["active"], json!(true), "legacy NULL-active row pinned: {rows:?}");
}

/// G-305 — ref's legacy pin is `!current.active` (credential.ts:129):
/// true for NULL AND false. A top row with active=false is pinned back
/// to true when a newer inactive credential is created on top; a top row
/// already active=true is left untouched (no pin, existing semantics).
#[tokio::test]
async fn credential_legacy_pin_covers_false_and_true_active() {
    let server = boot().await;
    // After create(activate=false) over a seeded top row, the row ALWAYS
    // ends active (false pinned by G-305; true already active).
    for (case, seed_active) in [("false", 0), ("true", 1)] {
        let id = format!("cred_legacy_{case}");
        let insert_id = id.clone();
        server
            .state
            .engine
            .db
            .transaction(move |tx| {
                tx.execute(
                    "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                     VALUES (?1, 'legacy305', 'default', ?2, ?3, 5, 5)",
                    (&insert_id, key_credential("old").to_string(), seed_active),
                )?;
                Ok(())
            })
            .await
            .expect("seed legacy row");

        let response = post_json(
            &server,
            "/api/credential",
            &json!({ "integrationID": "legacy305", "value": key_credential("new"), "activate": false }),
        )
        .await;
        assert_eq!(response.status, 200, "{}", response.text());
        assert_eq!(response.json()["data"]["active"], json!(false), "{case}: stays inactive");

        let response = get(&server, "/api/credential").await;
        let rows = response.json()["data"].as_array().expect("data").clone();
        let legacy = rows.iter().find(|row| row["id"] == json!(id)).expect("legacy row");
        assert_eq!(
            legacy["active"],
            json!(true),
            "{case}: the top row ends active after the newer inactive create: {rows:?}"
        );
    }
}

#[tokio::test]
async fn credential_duplicate_id_conflicts_with_resource() {
    let server = boot().await;
    let first = post_json(
        &server,
        "/api/credential",
        &json!({ "id": "cred_dup", "integrationID": "dummy", "value": key_credential("k") }),
    )
    .await;
    assert_eq!(first.status, 200, "{}", first.text());
    let second = post_json(
        &server,
        "/api/credential",
        &json!({ "id": "cred_dup", "integrationID": "dummy", "value": key_credential("k") }),
    )
    .await;
    assert_eq!(second.status, 409, "{}", second.text());
    let error = second.json();
    assert_eq!(error["_tag"], json!("ConflictError"), "{error}");
    assert_eq!(error["resource"], json!("cred_dup"), "{error}");
    assert_eq!(error["message"], json!("Credential already exists: cred_dup"), "{error}");
}

#[tokio::test]
async fn credential_list_fails_the_read_on_an_undecodable_value() {
    let server = boot().await;
    // A corrupt value row FAILS the whole list (G-225 core semantics the
    // HTTP handler now mirrors) instead of degrading to a Null value.
    server
        .state
        .engine
        .db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO credential (id, integration_id, label, value, active, time_created, time_updated) \
                 VALUES ('cred_bad', 'dummy', 'default', 'not-json', NULL, 1, 1), \
                        ('cred_null_integration', NULL, 'default', ?, NULL, 1, 1)",
                (key_credential("k").to_string(),),
            )?;
            Ok(())
        })
        .await
        .expect("seed rows");
    let response = get(&server, "/api/credential").await;
    assert_eq!(response.status, 500, "decode failure fails the read: {}", response.text());
}

// ---------------------------------------------------------------------------
// G-270①
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_list_returns_every_agent_with_the_default_first() {
    let server = boot().await;
    let response = get(&server, "/api/agent").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json()["data"].as_array().expect("data").clone();
    assert_ne!(data, [] as [serde_json::Value; 0]);
    // Default-first (core agent.ts:123-129 puts selectedDefault first).
    assert_eq!(
        data[0]["id"],
        json!("build"),
        "default agent first: {:?}",
        data.iter().map(|a| a["id"].clone()).collect::<Vec<_>>()
    );
    // Hidden agents (title/summary builtins) stay visible — the ref list
    // does NOT filter them.
    assert!(
        data.iter().any(|agent| agent["hidden"] == json!(true)),
        "hidden agents present: {:?}",
        data.iter().map(|a| (a["id"].clone(), a["hidden"].clone())).collect::<Vec<_>>()
    );
    // G-396 — every agent rides the `Agent.Editor.update` creation seed
    // (`Agent.Info.default` baseline + the four global external_directory
    // allows), so the wire rulesets carry the baseline `*,*,allow` and the
    // ref entry counts (explore's mutator re-appends the globals at the
    // tail → 25).
    let expected: &[(&str, usize)] = &[
        ("build", 10),
        ("general", 11),
        ("explore", 25),
        ("plan", 13),
        ("compaction", 9),
        ("title", 10),
        ("summary", 10),
    ];
    for (id, len) in expected {
        let agent = data
            .iter()
            .find(|a| a["id"] == json!(id))
            .unwrap_or_else(|| panic!("{id} missing: {data:?}"));
        let perms = agent["permissions"].as_array().unwrap_or_else(|| panic!("{id} perms"));
        assert_eq!(perms.len(), *len, "{id} wire permission count (G-396): {perms:?}");
        assert_eq!(
            perms[0],
            json!({"action": "*", "resource": "*", "effect": "allow"}),
            "{id} baseline rule"
        );
    }
}

// ---------------------------------------------------------------------------
// G-270②
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reference_list_reads_the_assembled_service() {
    let server = boot().await;
    let workspace = &server.workspace;
    std::fs::create_dir_all(workspace.join("docs")).expect("docs");
    // Local git remote that cannot hang: connection refused on loopback.
    let config = json!({
        "references": {
            "rel": { "type": "local", "path": "./docs", "description": "Local docs" },
            "git": { "type": "git", "repository": "https://127.0.0.1:1/a/b", "branch": "main", "hidden": true },
            "git-shorthand": "a/b",
            "git-bad-branch": { "type": "git", "repository": "https://127.0.0.1:1/a/b", "branch": "not..valid" },
            "bad/name": "./docs",
            "with space": "./docs"
        }
    });
    std::fs::write(workspace.join("opencode.json"), config.to_string()).expect("config");

    let response = get(&server, "/api/reference").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json()["data"].as_array().expect("data").clone();
    let by_name = |name: &str| {
        data.iter()
            .find(|entry| entry["name"] == json!(name))
            .unwrap_or_else(|| panic!("{name} missing: {data:?}"))
            .clone()
    };
    let names: Vec<&str> = data.iter().map(|e| e["name"].as_str().unwrap_or_default()).collect();

    // Local: path resolved against the location, description present,
    // hidden omitted.
    let rel = by_name("rel");
    assert_eq!(rel["path"], json!(workspace.join("docs").display().to_string()), "{rel}");
    assert_eq!(rel["description"], json!("Local docs"));
    assert!(rel.get("hidden").is_none(), "hidden omitted: {rel}");
    assert_eq!(
        rel["source"],
        json!({ "type": "local", "path": workspace.join("docs").display().to_string() })
    );

    // Git: path = the materialized cache path (NOT the URL), source keeps
    // repository/branch; hidden present.
    let git = by_name("git");
    let expected_cache = {
        let reference = oc_core::reference::parse("https://127.0.0.1:1/a/b").expect("parse");
        oc_core::reference::cache_path(
            &oc_util::global_roots::resolve().repos(),
            &reference,
            Some("main"),
        )
        .display()
        .to_string()
    };
    assert_eq!(git["path"], json!(expected_cache), "{git}");
    assert_ne!(git["path"], json!("https://127.0.0.1:1/a/b"), "URL never leaks as path");
    assert_eq!(git["hidden"], json!(true));
    assert_eq!(
        git["source"],
        json!({ "type": "git", "repository": "https://127.0.0.1:1/a/b", "branch": "main" })
    );

    // String shorthand without ./ / ~ is a GIT source.
    let shorthand = by_name("git-shorthand");
    assert_eq!(shorthand["source"]["type"], json!("git"), "{shorthand}");
    assert_eq!(shorthand["source"]["repository"], json!("a/b"));

    // Invalid branch dropped; invalid aliases dropped.
    assert!(!names.contains(&"git-bad-branch"), "{data:?}");
    assert!(!names.contains(&"bad/name"), "{data:?}");
    assert!(!names.contains(&"with space"), "{data:?}");
}

// ---------------------------------------------------------------------------
// G-270③
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fs_find_returns_location_relative_paths() {
    let server = boot().await;
    let workspace = &server.workspace;
    std::fs::create_dir_all(workspace.join("docs")).expect("docs");
    std::fs::write(workspace.join("docs").join("guide.md"), "x").expect("guide");
    std::fs::write(workspace.join("readme.md"), "x").expect("readme");

    let response = get(&server, "/api/fs/find?query=readme").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json()["data"].as_array().expect("data").clone();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["path"], json!("readme.md"), "relative, no workspace prefix: {data:?}");
    assert_eq!(data[0]["type"], json!("file"));

    let response = get(&server, "/api/fs/find?query=docs&type=directory").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json()["data"].as_array().expect("data").clone();
    assert!(
        data.iter().any(|entry| entry["path"] == json!("docs/")),
        "directories relative with a trailing separator: {data:?}"
    );
}

// ---------------------------------------------------------------------------
// G-270④
// ---------------------------------------------------------------------------

#[tokio::test]
async fn vcs_init_unknown_provider_is_a_400_with_provider_field() {
    let server = boot().await;
    let response = post_json(&server, "/api/vcs/init?provider=darcs", &json!({})).await;
    assert_eq!(response.status, 400, "{}", response.text());
    let error = response.json();
    assert_eq!(error["_tag"], json!("InvalidRequestError"), "{error}");
    assert_eq!(error["field"], json!("provider"), "{error}");
    assert_eq!(error["message"], json!("Unknown VCS provider"), "{error}");
}

#[tokio::test]
async fn vcs_init_conflict_carries_the_resource_directory() {
    let server = boot().await;
    let workspace = &server.workspace;
    std::fs::create_dir_all(workspace.join(".git")).expect("git marker");
    let response = post_json(&server, "/api/vcs/init", &json!({})).await;
    assert_eq!(response.status, 409, "{}", response.text());
    let error = response.json();
    assert_eq!(error["_tag"], json!("ConflictError"), "{error}");
    assert_eq!(error["message"], json!("Project already has version control"), "{error}");
    let canonical = std::fs::canonicalize(workspace).expect("canonical");
    assert_eq!(error["resource"], json!(canonical.display().to_string()), "{error}");
}

#[tokio::test]
async fn vcs_init_git_succeeds_then_conflicts_on_reinit() {
    let server = boot().await;
    let workspace = &server.workspace;
    assert!(!workspace.join(".git").exists());
    let response = post_json(&server, "/api/vcs/init", &json!({})).await;
    assert_eq!(response.status, 204, "{}", response.text());
    assert!(workspace.join(".git").exists(), "git init ran");
    // The success path invalidated the stale vcs=None bundle: a second
    // call re-assembles and 409s with the resource.
    let response = post_json(&server, "/api/vcs/init", &json!({})).await;
    assert_eq!(response.status, 409, "{}", response.text());
    let error = response.json();
    assert_eq!(error["_tag"], json!("ConflictError"), "{error}");
}

// ---------------------------------------------------------------------------
// G-270⑤
// ---------------------------------------------------------------------------

#[tokio::test]
async fn info_carries_capabilities_only_when_ready() {
    let server = boot().await;
    // Not ready (starting): the minimal pre-boot probe body.
    let _ = server.state.readiness.send(oc_server::state::Readiness::Starting);
    let response = request(server.addr, "GET", "/api/info", &[], &[]).await.expect("info");
    assert_eq!(response.status, 503, "{}", response.text());
    let body = response.json();
    assert!(body.get("version").is_some(), "{body}");
    assert!(body.get("pid").is_some(), "{body}");
    assert!(body.get("urls").is_some(), "{body}");
    assert!(body.get("paths").is_some(), "{body}");
    assert!(body.get("capabilities").is_none(), "not-ready body is minimal: {body}");

    // Ready: the full handler body with the capability gate.
    let _ = server.state.readiness.send(oc_server::state::Readiness::Ready);
    let response = request(server.addr, "GET", "/api/info", &[], &[]).await.expect("info");
    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(
        body["capabilities"]["persistentPty"],
        json!(cfg!(not(target_os = "windows"))),
        "{body}"
    );
}

// ---------------------------------------------------------------------------
// G-270⑥
// ---------------------------------------------------------------------------

/// Minimal WS upgrade probe: returns the HTTP status (101 when upgraded).
async fn ws_status(addr: std::net::SocketAddr, target: &str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("tcp");
    let key = "dGhpcyBpcyBhIHRlc3Qga2V5IQ==";
    let request = format!(
        "GET {target} HTTP/1.1\r\nhost: {addr}\r\nupgrade: websocket\r\nconnection: Upgrade\r\n\
         sec-websocket-key: {key}\r\nsec-websocket-version: 13\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let read = stream.read(&mut chunk).await.expect("read");
        if read == 0 {
            return 0;
        }
        head.extend_from_slice(&chunk[..read]);
        if let Some(position) = head.windows(4).position(|window| window == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(&head[..position]).into_owned();
            return text.split_whitespace().nth(1).and_then(|code| code.parse().ok()).unwrap_or(0);
        }
    }
}

#[tokio::test]
async fn pty_connect_unresolvable_location_is_a_404_before_upgrade() {
    let server = boot().await;
    // Create a real pty in the workspace.
    let response =
        post_json(&server, "/api/pty", &json!({ "command": "sh", "args": ["-c", "sleep 30"] }))
            .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let pty_id = response.json()["data"]["id"].as_str().expect("pty id").to_owned();

    // A location whose directory does not exist must 404 BEFORE the
    // upgrade (ref locationMiddleware first) — never a forged
    // unvalidated LocationRef riding into the socket.
    let status = ws_status(
        server.addr,
        &format!("/api/pty/{pty_id}/connect?location%5Bdirectory%5D=/definitely-not-a-dir"),
    )
    .await;
    assert_eq!(status, 404, "unresolvable location must not upgrade");

    // The same pty with the VALID workspace location still upgrades.
    let status = ws_status(
        server.addr,
        &format!(
            "/api/pty/{pty_id}/connect?location%5Bdirectory%5D={}",
            urlencoding_encode(&server.workspace.display().to_string())
        ),
    )
    .await;
    assert_eq!(status, 101, "valid location upgrades");
}

/// Tiny %-encoder for the query value (path + separator chars).
fn urlencoding_encode(input: &str) -> String {
    use std::fmt::Write as _; // format_push_string: write! into the existing String
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// G-270⑦
// ---------------------------------------------------------------------------

#[tokio::test]
async fn worktree_list_serves_any_known_project() {
    let server = boot().await;
    // A DIFFERENT project's rows (not the workspace's project).
    server
        .state
        .engine
        .db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO project (id, worktree, vcs, time_created, time_updated, sandboxes, time_active) \
                 VALUES ('proj_other', '/tmp/opencode/other-wt', 'git', 1, 1, '', 1)",
                [],
            )?;
            tx.execute(
                "INSERT INTO worktree (project_id, directory, strategy, time_created) \
                 VALUES ('proj_other', '/tmp/opencode/other-wt', NULL, 1)",
                [],
            )?;
            Ok(())
        })
        .await
        .expect("seed");

    // Requested FROM the workspace location: still 200 + the rows (the
    // old code forced projectID == the current location's project).
    let response = get(&server, "/api/worktree?projectID=proj_other").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json().as_array().expect("bare array").clone();
    assert_eq!(data.len(), 1, "{data:?}");
    assert_eq!(data[0]["directory"], json!("/tmp/opencode/other-wt"));

    // An unknown project id is still a 404 (Project.NotFoundError).
    let response = get(&server, "/api/worktree?projectID=nope").await;
    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(response.json()["_tag"], json!("ProjectNotFoundError"));
}

/// G-322 — `Worktree.list` (worktree.ts:138-141) orders
/// `desc(time_created), asc(directory)`: newest worktree first, the
/// directory as the tiebreaker — NOT directory-only.
#[tokio::test]
async fn worktree_list_orders_newest_first_with_directory_tiebreak() {
    let server = boot().await;
    server
        .state
        .engine
        .db
        .transaction(|tx| {
            tx.execute(
                "INSERT INTO project (id, worktree, vcs, time_created, time_updated, sandboxes, time_active) \
                 VALUES ('proj_order', '/tmp/opencode/order-wt', 'git', 1, 1, '', 1)",
                [],
            )?;
            // time_created deliberately inverse to alphabetical order,
            // plus a same-timestamp pair for the tiebreak.
            for (directory, created) in [
                ("/tmp/opencode/b-old", 10),
                ("/tmp/opencode/z-mid", 20),
                ("/tmp/opencode/c-new", 30),
                ("/tmp/opencode/a-new", 30),
            ] {
                tx.execute(
                    "INSERT INTO worktree (project_id, directory, strategy, time_created) \
                     VALUES ('proj_order', ?1, NULL, ?2)",
                    (directory, created),
                )?;
            }
            Ok(())
        })
        .await
        .expect("seed");

    let response = get(&server, "/api/worktree?projectID=proj_order").await;
    assert_eq!(response.status, 200, "{}", response.text());
    let data = response.json().as_array().expect("bare array").clone();
    let directories: Vec<&str> =
        data.iter().map(|row| row["directory"].as_str().expect("directory")).collect();
    assert_eq!(
        directories,
        vec![
            "/tmp/opencode/a-new", // newest (30), directory tiebreak
            "/tmp/opencode/c-new", // newest (30), later directory
            "/tmp/opencode/z-mid", // 20
            "/tmp/opencode/b-old", // 10 — alphabetically first but OLDEST
        ],
        "newest first, directory as tiebreaker"
    );
}

// ---------------------------------------------------------------------------
// G-270⑧
// ---------------------------------------------------------------------------

#[tokio::test]
async fn form_duplicate_create_is_a_conflict_error() {
    let server = boot().await;
    let response = post_json(&server, "/api/session", &json!({ "title": "forms" })).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let session_id = response.json()["data"]["id"].as_str().expect("session id").to_owned();

    let form_body = json!({
        "id": "frm_dup",
        "title": "Pick one",
        "fields": [{ "type": "string", "key": "name", "required": true }],
    });
    let first = post_json(&server, &format!("/api/session/{session_id}/form"), &form_body).await;
    assert_eq!(first.status, 200, "{}", first.text());
    let second = post_json(&server, &format!("/api/session/{session_id}/form"), &form_body).await;
    assert_eq!(second.status, 409, "{}", second.text());
    let error = second.json();
    assert_eq!(
        error["_tag"],
        json!("ConflictError"),
        "typed clients classify duplicates as conflicts: {error}"
    );
    assert_eq!(error["resource"], json!("frm_dup"), "{error}");
    assert_eq!(error["message"], json!("Form already exists: frm_dup"), "{error}");
}
