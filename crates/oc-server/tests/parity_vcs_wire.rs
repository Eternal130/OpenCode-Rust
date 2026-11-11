//! WIRE-cluster parity tests (ledger C4a): VCS endpoint wire shapes.
//!
//! - VCS-1 `base_source_and_fallback` — `/api/vcs/base` returns the v2
//!   `Vcs.Base` `{name, ref, source: "reflog"|"default"}` (reflog-derived
//!   base first, default branch fallback, `null` without HEAD, 503 when no
//!   review base exists) — `packages/core/src/plugin/vcs/git.ts:286-317`.
//! - VCS-2 `branch_string_array` — `/api/vcs/branch` returns a bare
//!   `string[]` (local + remote short names, committerdate ordering,
//!   search + limit) — `git.ts:228-241`, `schema/src/vcs.ts:18`.
//! - VCS-3 `diff_file_list` — `/api/vcs/diff` returns `FileDiff.Info[]`
//!   (`{file, patch, additions, deletions, status}`) across the
//!   working/branch/committed modes with the 10 MiB budget — `git.ts:101-117`.
//! - VCS-4 `worktree_remove_runs_git` — `DELETE /api/worktree` canonicalizes,
//!   validates the stored strategy, runs `git worktree remove [--force]`,
//!   then deletes the row and publishes `worktree.updated` —
//!   `packages/core/src/worktree.ts:257-271`.
//! - HTTP-H5 `worktree_error_name_literal` — the 400 error envelope's
//!   `name` is the literal `"WorktreeError"` —
//!   `packages/server/src/handlers/worktree.ts:35-46`.

use std::sync::Arc;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::Value;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use futures::StreamExt as _;
use std::os::unix::fs::PermissionsExt as _;

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    addr: std::net::SocketAddr,
    #[allow(dead_code)] // the workspace root the server was booted in
    workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git spawn");
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// The BOOT_LOCK guard intentionally spans the boot awaits (serialize
// the process-global set_current_dir critical section).
#[allow(clippy::await_holding_lock)]
async fn boot(workspace: std::path::PathBuf) -> TestServer {
    // `set_current_dir` is process-global while the engine captures its
    // launch directory at boot — serialize the critical section so
    // parallel tests cannot boot in another test's workspace.
    static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = BOOT_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
    drop(guard);
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, state, addr, workspace, _data: data }
}

/// A git repo with one commit on `main` (user config pinned for determinism).
fn seed_repo(workspace: &std::path::Path) {
    std::fs::write(workspace.join("README.md"), "seed\n").expect("seed file");
    git(workspace, &["init", "--initial-branch=main"]);
    git(workspace, &["config", "user.email", "test@example.com"]);
    git(workspace, &["config", "user.name", "Test"]);
    git(workspace, &["add", "."]);
    git(workspace, &["commit", "-m", "seed"]);
}

/// Percent-encoded `location[directory]` query value.
fn location_query(workspace: &std::path::Path) -> String {
    let encoded: String = workspace
        .to_string_lossy()
        .chars()
        .flat_map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' | '/' => vec![c],
            _ => format!("%{:02X}", c as u32).chars().collect(),
        })
        .collect();
    format!("location%5Bdirectory%5D={encoded}")
}

/// Raw GET returning `(status, body)`.
async fn raw_get(addr: std::net::SocketAddr, target: &str) -> (u16, Value) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let request = format!("GET {target} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status =
        text.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok()).unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|(_, rest)| rest.to_owned()).unwrap_or_default();
    let json = serde_json::from_str(&body).unwrap_or(Value::Null);
    (status, json)
}

/// Raw request with a JSON body: `(status, body)`.
async fn raw_json(
    addr: std::net::SocketAddr,
    method: &str,
    target: &str,
    body: &Value,
) -> (u16, Value) {
    let payload = body.to_string();
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let request = format!(
        "{method} {target} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\ncontent-type: \
         application/json\r\ncontent-length: {}\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status =
        text.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok()).unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|(_, rest)| rest.to_owned()).unwrap_or_default();
    let json = serde_json::from_str(&body).unwrap_or(Value::Null);
    (status, json)
}

/// Registers the project row (v2 resolves the project on session create).
async fn project_id(server: &TestServer) -> (String, std::path::PathBuf) {
    server
        .client
        .session()
        .create(&SessionCreateBody { title: Some("vcs".to_owned()), ..Default::default() })
        .await
        .expect("create session");
    let (id, worktree) = server
        .state
        .engine
        .db
        .transaction(|tx| {
            Ok(tx.conn().query_row(
                "SELECT id, worktree FROM project ORDER BY time_created LIMIT 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?)
        })
        .await
        .expect("project row");
    (id, std::path::PathBuf::from(worktree))
}

// ---------------------------------------------------------------------------
// VCS-1 — base {name, ref, source} with reflog → default fallback, 503s
// ---------------------------------------------------------------------------

#[tokio::test]
async fn base_source_and_fallback() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);

    // A feature branch checked out from main: the reflog carries
    // "branch: Created from main" → source "reflog".
    git(&workspace, &["checkout", "-b", "feature", "main"]);
    std::fs::write(workspace.join("feature.txt"), "feature\n").expect("feature file");
    git(&workspace, &["add", "."]);
    git(&workspace, &["commit", "-m", "feature"]);

    let server = boot(workspace.clone()).await;
    let query = location_query(&workspace);
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/base?{query}")).await;
    assert_eq!(status, 200, "vcs.base on a feature branch: {body}");
    let base = &body["data"];
    assert_eq!(base["source"], "reflog", "branch-created-from reflog wins: {base}");
    assert_eq!(base["name"], "main");
    assert!(base["ref"].is_string() && !base["ref"].as_str().is_some_and(str::is_empty));

    // Back on main itself: the default-branch fallback applies.
    git(&workspace, &["checkout", "main"]);
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/base?{query}")).await;
    assert_eq!(status, 200, "vcs.base on main: {body}");
    assert_eq!(body["data"]["source"], "default", "default-branch fallback: {body}");
    assert_eq!(body["data"]["name"], "main");

    // An orphan branch (no creation reflog, ≠ default) → 503
    // ServiceUnavailableError (v2 DiffError "Choose a review base").
    git(&workspace, &["checkout", "--orphan", "loner-side"]);
    git(&workspace, &["commit", "--allow-empty", "-m", "loner"]);
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/base?{query}")).await;
    assert_eq!(status, 503, "no review base: {body}");
    assert_eq!(body["_tag"], "ServiceUnavailableError");
    assert_eq!(body["service"], "vcs");
    assert!(
        body["message"].as_str().is_some_and(|message| message.contains("review base")),
        "503 message: {body}"
    );
}

// ---------------------------------------------------------------------------
// VCS-2 — branch list is a bare string[]
// ---------------------------------------------------------------------------

#[tokio::test]
async fn branch_string_array() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    git(&workspace, &["branch", "feature/one"]);
    git(&workspace, &["branch", "feature/two"]);
    git(&workspace, &["branch", "searchhit"]);

    // Remote branches: a bare clone acts as origin.
    let remote = tempfile::tempdir().expect("remote").keep();
    git(&remote, &["init", "--bare", "--initial-branch=main"]);
    git(&workspace, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&workspace, &["push", "origin", "main"]);
    git(&workspace, &["fetch", "origin"]);

    let server = boot(workspace.clone()).await;
    let query = location_query(&workspace);
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/branch?{query}")).await;
    assert_eq!(status, 200, "vcs.branch: {body}");
    let data = &body["data"];
    let list = data
        .as_array()
        .unwrap_or_else(|| panic!("data must be a bare string[] (BranchList), got: {data}"));
    let names: Vec<&str> = list.iter().map(|item| item.as_str().expect("string entry")).collect();
    assert!(names.contains(&"main"), "local branches listed: {names:?}");
    assert!(names.contains(&"feature/one"), "slash branches listed: {names:?}");
    assert!(
        names.iter().any(|name| name.starts_with("origin/")),
        "remote-tracking branches listed: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.ends_with("/HEAD")),
        "symbolic remote HEADs are filtered: {names:?}"
    );

    // search filter + limit (max 100). NOTE: for-each-ref globs do not
    // cross `/` (`refs/heads/*feature*` misses `feature/one`) — the port
    // passes v2's exact patterns through, so search on a flat name.
    let (status, body) =
        raw_get(server.addr, &format!("/api/vcs/branch?{query}&search=searchhit")).await;
    assert_eq!(status, 200);
    let names: Vec<&str> = body["data"]
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string"))
        .collect();
    assert_eq!(names, vec!["searchhit"], "{names:?}");
    let (status, body) =
        raw_get(server.addr, &format!("/api/vcs/branch?{query}&search=searchhit&limit=1")).await;
    assert_eq!(status, 200);
    assert_eq!(body["data"].as_array().expect("array").len(), 1, "limit caps the list");
}

// ---------------------------------------------------------------------------
// VCS-3 — diff returns FileDiff.Info[]
// ---------------------------------------------------------------------------

#[tokio::test]
async fn diff_file_list() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);

    let server = boot(workspace.clone()).await;
    let query = location_query(&workspace);

    // Working mode with no changes → empty array. `mode` is REQUIRED on
    // the wire (v2.0.24 probe, rollout-probe.json: bare /api/vcs/diff →
    // 400 `Missing key\n  at ["mode"]`, kind Query — the HTTP-H0 query
    // rollout enforces it in middleware).
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=working")).await;
    assert_eq!(status, 200, "vcs.diff clean: {body}");
    assert_eq!(body["data"].as_array().expect("array").len(), 0);

    // Modify a tracked file + add an untracked one.
    std::fs::write(workspace.join("README.md"), "seed\nchanged\n").expect("modify");
    std::fs::write(workspace.join("new.txt"), "brand new\n").expect("untracked");

    let (status, body) = raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=working")).await;
    assert_eq!(status, 200, "vcs.diff working: {body}");
    let files = body["data"].as_array().expect("FileDiff[] array").clone();
    let by_file = |name: &str| {
        files
            .iter()
            .find(|entry| entry["file"] == name)
            .unwrap_or_else(|| panic!("no entry for {name}: {files:?}"))
            .clone()
    };
    let readme = by_file("README.md");
    assert_eq!(readme["status"], "modified");
    assert_eq!(readme["additions"], 1, "numstat additions: {readme}");
    assert_eq!(readme["deletions"], 0, "numstat deletions: {readme}");
    assert!(readme["patch"].as_str().is_some_and(|patch| patch.contains("diff --git")));
    assert!(readme["patch"].as_str().is_some_and(|patch| patch.contains("+changed")));
    let new = by_file("new.txt");
    assert_eq!(new["status"], "added");
    assert_eq!(new["additions"], 1, "untracked additions: {new}");
    assert!(
        new["patch"].as_str().is_some_and(|patch| patch.contains("+brand new")),
        "untracked patch synthesized: {new}"
    );

    // Committed / branch modes: move the dirty tree onto a feature
    // branch (main stays behind), commit there, then diff against main.
    git(&workspace, &["checkout", "-b", "feature", "main"]);
    git(&workspace, &["add", "."]);
    git(&workspace, &["commit", "-m", "changes"]);
    let (status, body) =
        raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=committed")).await;
    assert_eq!(status, 200, "committed diff against the default base: {body}");
    let files = body["data"].as_array().expect("FileDiff[]").clone();
    let names: Vec<&str> = files.iter().map(|f| f["file"].as_str().expect("file")).collect();
    assert!(names.contains(&"README.md"), "committed diff lists the change: {names:?}");
    assert!(names.contains(&"new.txt"), "committed diff lists the added file: {names:?}");

    // Branch mode with an explicit base: merge-base(main) → working tree.
    let (status, body) =
        raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=branch&base=main")).await;
    assert_eq!(status, 200, "branch diff against main: {body}");
    let files = body["data"].as_array().expect("FileDiff[]").clone();
    let names: Vec<&str> = files.iter().map(|f| f["file"].as_str().expect("file")).collect();
    assert!(names.contains(&"README.md"), "branch diff lists committed change: {names:?}");
    assert!(names.contains(&"new.txt"), "branch diff lists the added file: {names:?}");
}

// ---------------------------------------------------------------------------
// VCS-4 + HTTP-H5 — worktree.remove runs git; WorktreeError.name literal
// ---------------------------------------------------------------------------

#[tokio::test]
async fn worktree_remove_runs_git() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    let server = boot(workspace.clone()).await;
    let (project_id, _) = project_id(&server).await;
    let parent = tempfile::tempdir().expect("worktree parent").keep();

    let created = server
        .client
        .worktree()
        .create(&WorktreeCreateInput {
            project_id: project_id.clone(),
            name: Some("to-remove".to_owned()),
            directory: Some(parent.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect("worktree create");
    let directory = std::path::PathBuf::from(&created.directory);
    assert!(directory.is_dir(), "worktree exists before remove");

    // v2 worktree.ts:257-271 — remove canonicalizes, runs
    // `git worktree remove`, deletes the row, publishes worktree.updated.
    let (status, body) = raw_json(
        server.addr,
        "DELETE",
        "/api/worktree",
        &serde_json::json!({
            "projectID": project_id,
            "directory": directory.to_string_lossy(),
            "force": false,
        }),
    )
    .await;
    assert_eq!(status, 204, "worktree remove: {body}");
    assert!(!directory.exists(), "git worktree remove deleted the directory from disk");
    let listed = server
        .client
        .worktree()
        .list(&WorktreeListQuery { project_id: project_id.clone() })
        .await
        .expect("list after remove");
    let still_listed = listed.iter().any(|entry| entry.directory == *directory.to_string_lossy());
    assert!(!still_listed, "the row is gone: {listed:?}");

    // Dirty tree + force=false → 400 WorktreeError with forceRequired.
    let dirty = server
        .client
        .worktree()
        .create(&WorktreeCreateInput {
            project_id: project_id.clone(),
            name: Some("dirty".to_owned()),
            directory: Some(parent.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect("worktree create");
    let dirty_dir = std::path::PathBuf::from(&dirty.directory);
    std::fs::write(dirty_dir.join("uncommitted.txt"), "dirt\n").expect("dirty file");

    let (status, body) = raw_json(
        server.addr,
        "DELETE",
        "/api/worktree",
        &serde_json::json!({
            "projectID": project_id,
            "directory": dirty_dir.to_string_lossy(),
            "force": false,
        }),
    )
    .await;
    assert_eq!(status, 400, "dirty remove without force: {body}");
    // HTTP-H5: name is the literal "WorktreeError" (not a machine code).
    assert_eq!(body["_tag"], "WorktreeError");
    assert_eq!(body["name"], "WorktreeError", "name literal: {body}");
    assert_eq!(body["data"]["forceRequired"], true, "dirty tree demands force: {body}");
    assert!(
        body["data"]["message"].as_str().is_some_and(|message| {
            message.contains("modified or untracked") || message.contains("--force")
        }),
        "git's dirty-tree message surfaces: {body}"
    );
    assert!(dirty_dir.exists(), "the dirty worktree was NOT removed");

    // force=true removes it.
    let (status, body) = raw_json(
        server.addr,
        "DELETE",
        "/api/worktree",
        &serde_json::json!({
            "projectID": project_id,
            "directory": dirty_dir.to_string_lossy(),
            "force": true,
        }),
    )
    .await;
    assert_eq!(status, 204, "forced remove: {body}");
    assert!(!dirty_dir.exists(), "forced remove deletes the directory");
}

/// RA-10: a stored strategy that resolves to no registered backend (only
/// `git` exists) is ref `StrategyUnavailableError` (worktree.ts:198-201,
/// 263) → wire `WorktreeError` 400 with `Worktree strategy unavailable:
/// <id>` and NO forceRequired; nothing is removed.
#[tokio::test]
async fn worktree_remove_unknown_strategy_is_unavailable() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    let server = boot(workspace.clone()).await;
    let (project_id, _) = project_id(&server).await;
    let parent = tempfile::tempdir().expect("worktree parent").keep();

    let created = server
        .client
        .worktree()
        .create(&WorktreeCreateInput {
            project_id: project_id.clone(),
            name: Some("strange".to_owned()),
            directory: Some(parent.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect("worktree create");
    let directory = std::path::PathBuf::from(&created.directory);

    // Forge a row with an unregistered stored strategy (v2's plugin-only
    // strategies have no Rust backend).
    let update_project = project_id.clone();
    let update_directory = directory.to_string_lossy().into_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.execute(
                "UPDATE worktree SET strategy = 'hg' WHERE project_id = ?1 AND directory = ?2",
                (update_project, update_directory),
            )
        })
        .await
        .expect("forge strategy");

    let (status, body) = raw_json(
        server.addr,
        "DELETE",
        "/api/worktree",
        &serde_json::json!({
            "projectID": project_id,
            "directory": directory.to_string_lossy(),
            "force": true,
        }),
    )
    .await;
    assert_eq!(status, 400, "unknown strategy: {body}");
    assert_eq!(body["_tag"], "WorktreeError");
    assert_eq!(body["name"], "WorktreeError");
    assert_eq!(
        body["data"]["message"], "Worktree strategy unavailable: hg",
        "ref StrategyUnavailableError message: {body}"
    );
    assert!(body["data"].get("forceRequired").is_none(), "no forceRequired: {body}");
    assert!(directory.exists(), "nothing was removed");
}

/// HTTP-H5 standalone: every 400 worktree error envelope carries the
/// literal name. (The remove test covers the runtime case; this pins the
/// shape for create-path errors too.)
#[tokio::test]
async fn worktree_error_name_literal() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    let server = boot(workspace.clone()).await;
    let (project_id, _) = project_id(&server).await;

    // An empty name → 400 WorktreeError, name literal (worktree.ts name check).
    let (status, body) = raw_json(
        server.addr,
        "POST",
        "/api/worktree",
        &serde_json::json!({ "projectID": project_id, "name": "  " }),
    )
    .await;
    assert_eq!(status, 400, "empty name rejected: {body}");
    assert_eq!(body["_tag"], "WorktreeError");
    assert_eq!(
        body["name"], "WorktreeError",
        "schema requires the literal name, not a machine code: {body}"
    );
    assert!(body["data"]["message"].is_string(), "data.message present: {body}");

    // Unknown directory on remove → Invalid worktree directory (400).
    let (status, body) = raw_json(
        server.addr,
        "DELETE",
        "/api/worktree",
        &serde_json::json!({
            "projectID": project_id,
            "directory": workspace.join("not-a-worktree").to_string_lossy(),
            "force": false,
        }),
    )
    .await;
    assert_eq!(status, 400, "unknown directory rejected: {body}");
    assert_eq!(body["name"], "WorktreeError", "name literal on remove errors: {body}");
}

// ---------------------------------------------------------------------------
// DB-D4 — project.update persists canonical/name/icon/commands, returns the
// REAL row and announces project.updated (project.ts:215-239)
// ---------------------------------------------------------------------------

/// Raw project row for post-update DB assertions.
async fn project_row(
    server: &TestServer,
    id: &str,
) -> (String, Option<String>, Option<String>, Option<String>, Option<String>) {
    let id = id.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            Ok(tx.conn().query_row(
                "SELECT worktree, name, icon_url_override, icon_color, commands \
                 FROM project WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )?)
        })
        .await
        .expect("project row read")
}

#[tokio::test]
async fn project_update_fields() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    let server = boot(workspace.clone()).await;
    let (project_id, _) = project_id(&server).await;
    let mut project_events = server.state.engine.bus.subscribe_types(&["project.updated"]);

    // Full metadata update incl. canonical + commands.
    let new_canonical = workspace.join("moved-checkout");
    let updated = server
        .client
        .project()
        .update(
            &project_id,
            &ProjectUpdateBody {
                canonical: Some(new_canonical.to_string_lossy().into_owned()),
                name: Some("renamed".to_owned()),
                icon: Some(ProjectIcon {
                    override_: Some("cat".to_owned()),
                    color: Some("#ff0000".to_owned()),
                    url: None,
                }),
                commands: Some(ProjectCommands { start: Some("pnpm dev".to_owned()) }),
            },
        )
        .await
        .expect("project update");
    // The REAL row, not a fabricated echo (ledger actual: "响应伪造").
    assert_eq!(updated.canonical, new_canonical.to_string_lossy());
    assert_eq!(updated.name.as_deref(), Some("renamed"));
    assert_eq!(
        updated.commands.as_ref().and_then(|commands| commands.start.as_deref()),
        Some("pnpm dev"),
        "commands.start round-trips: {:?}",
        updated.commands
    );
    assert_eq!(updated.icon.as_ref().and_then(|icon| icon.override_.as_deref()), Some("cat"));
    assert_eq!(updated.icon.as_ref().and_then(|icon| icon.color.as_deref()), Some("#ff0000"));
    assert!(updated.time.updated > 0, "real timestamps, not zeros: {:?}", updated.time);

    // The database row actually changed.
    let (worktree, name, icon_override, icon_color, commands) =
        project_row(&server, &project_id).await;
    assert_eq!(worktree, new_canonical.to_string_lossy(), "canonical persisted");
    assert_eq!(name.as_deref(), Some("renamed"));
    assert_eq!(icon_override.as_deref(), Some("cat"));
    assert_eq!(icon_color.as_deref(), Some("#ff0000"));
    assert_eq!(
        commands.as_deref().and_then(|raw| serde_json::from_str::<Value>(raw).ok()),
        Some(serde_json::json!({ "start": "pnpm dev" })),
        "commands stored as v2 start-object JSON: {commands:?}"
    );

    // project.updated was announced with the updated row.
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), project_events.next())
        .await
        .expect("project.updated within 5s")
        .expect("stream open");
    assert_eq!(event.type_, "project.updated");
    assert_eq!(event.data["id"], project_id);
    assert_eq!(event.data["name"], "renamed");

    // Empty strings CLEAR (project.ts:219-226): name "" → NULL, absent in Info.
    let cleared = server
        .client
        .project()
        .update(&project_id, &ProjectUpdateBody { name: Some(String::new()), ..Default::default() })
        .await
        .expect("clear name");
    assert_eq!(cleared.name, None, "cleared name is absent, not empty/null-string");
    let (_, name, _, _, _) = project_row(&server, &project_id).await;
    assert_eq!(name, None, "empty string clears the column to NULL");

    // commands with an empty start clears the commands column.
    let cleared = server
        .client
        .project()
        .update(
            &project_id,
            &ProjectUpdateBody {
                commands: Some(ProjectCommands { start: Some(String::new()) }),
                ..Default::default()
            },
        )
        .await
        .expect("clear commands");
    assert_eq!(cleared.commands, None, "cleared commands absent from Info");
    let (_, _, _, _, commands) = project_row(&server, &project_id).await;
    assert_eq!(commands, None, "empty start clears commands to NULL");

    // Unknown project → 404 ProjectNotFoundError (v2 NotFoundError).
    let missing = server
        .client
        .project()
        .update(
            "0000000000000000000000000000000000000000",
            &ProjectUpdateBody { name: Some("x".to_owned()), ..Default::default() },
        )
        .await;
    match missing {
        Err(oc_client::ClientError::Api { status, name, .. }) => {
            assert_eq!(status, 404);
            assert_eq!(name, "ProjectNotFoundError");
        }
        other => panic!("expected 404 ProjectNotFoundError, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// DB-D6 — worktree.refresh discovers/prunes rows + worktree.updated
// (worktree.ts:273-327)
// ---------------------------------------------------------------------------

async fn worktree_rows(server: &TestServer, project_id: &str) -> Vec<(String, Option<String>)> {
    let project_id = project_id.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            let mut statement = tx.conn().prepare(
                "SELECT directory, strategy FROM worktree WHERE project_id = ?1 ORDER BY directory",
            )?;
            let rows = statement
                .query_map([project_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .expect("worktree rows read")
}

#[tokio::test]
async fn worktree_refresh() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    let server = boot(workspace.clone()).await;
    let (project_id, _) = project_id(&server).await;

    // A linked git worktree created behind the server's back.
    let linked = workspace
        .parent()
        .expect("parent")
        .join(format!("linked-wt-{}", workspace.file_name().unwrap_or_default().to_string_lossy()));
    git(&workspace, &["worktree", "add", linked.to_str().unwrap(), "-b", "linked"]);
    let linked_real = std::fs::canonicalize(&linked).expect("linked canonical");

    // Refresh discovers it (strategy `git`) and the canonical main
    // checkout (unowned), atomically announcing worktree.updated.
    let mut updated = server.state.engine.bus.subscribe_types(&["worktree.updated"]);
    let (status, body) = raw_json(
        server.addr,
        "POST",
        "/api/worktree/refresh",
        &serde_json::json!({ "projectID": project_id }),
    )
    .await;
    assert_eq!(status, 204, "refresh is 204 NoContent: {body}");
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), updated.next())
        .await
        .expect("worktree.updated within 5s")
        .expect("stream open");
    assert_eq!(event.data["projectID"], project_id);

    let rows = worktree_rows(&server, &project_id).await;
    let linked_row = rows.iter().find(|(directory, _)| directory == linked_real.to_str().unwrap());
    assert_eq!(
        linked_row.and_then(|(_, strategy)| strategy.clone()),
        Some("git".to_owned()),
        "linked worktree discovered with the git strategy: {rows:?}"
    );
    let canonical_row = rows.iter().find(|(directory, _)| {
        directory == &std::fs::canonicalize(&workspace).unwrap().to_string_lossy()
    });
    assert!(
        canonical_row.is_some_and(|(_, strategy)| strategy.is_none()),
        "canonical main checkout listed unowned (root entry): {rows:?}"
    );

    // The wired list surface mirrors the rows (bare array, strategy?).
    let query = location_query(&workspace);
    let (status, body) =
        raw_get(server.addr, &format!("/api/worktree?{query}&projectID={project_id}")).await;
    assert_eq!(status, 200, "worktree list: {body}");
    let listed = body.as_array().cloned().unwrap_or_default();
    assert!(
        listed.iter().any(|entry| {
            entry["directory"].as_str() == Some(linked_real.to_string_lossy().as_ref())
                && entry["strategy"] == "git"
        }),
        "list carries the discovered entry: {listed:?}"
    );

    // The directory vanishing prunes the row on the next refresh and
    // announces again; existing directories survive.
    std::fs::remove_dir_all(&linked).expect("rm linked");
    let (status, body) = raw_json(
        server.addr,
        "POST",
        "/api/worktree/refresh",
        &serde_json::json!({ "projectID": project_id }),
    )
    .await;
    assert_eq!(status, 204, "second refresh: {body}");
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), updated.next())
        .await
        .expect("prune announces worktree.updated")
        .expect("stream open");
    assert_eq!(event.data["projectID"], project_id);
    let rows = worktree_rows(&server, &project_id).await;
    assert!(
        !rows.iter().any(|(directory, _)| directory == linked_real.to_str().unwrap()),
        "vanished directory pruned: {rows:?}"
    );
    assert!(
        rows.iter().any(|(directory, _)| {
            directory == &std::fs::canonicalize(&workspace).unwrap().to_string_lossy()
        }),
        "canonical row survives: {rows:?}"
    );

    // A no-change refresh stays silent (changed(update=false) — no event).
    let (status, body) = raw_json(
        server.addr,
        "POST",
        "/api/worktree/refresh",
        &serde_json::json!({ "projectID": project_id }),
    )
    .await;
    assert_eq!(status, 204, "idempotent refresh: {body}");
    let quiet = tokio::time::timeout(std::time::Duration::from_millis(300), updated.next()).await;
    assert!(quiet.is_err(), "no worktree.updated without a change");

    // Unknown project → 404 ProjectNotFoundError.
    let (status, body) = raw_json(
        server.addr,
        "POST",
        "/api/worktree/refresh",
        &serde_json::json!({ "projectID": "0000000000000000000000000000000000000000" }),
    )
    .await;
    assert_eq!(status, 404, "unknown project: {body}");
    assert_eq!(body["_tag"], "ProjectNotFoundError");
}

// ---------------------------------------------------------------------------
// G-9 — Mercurial adapter wire parity (plugin/vcs/hg.ts): info / status /
// diff working+branch / unsupported base + committed / empty branch list.
// ---------------------------------------------------------------------------

/// Installs a stub `hg` driven by a state directory (no real Mercurial in
/// the test environment) and returns the state dir. The stub implements
/// exactly the commands the adapter shells out: `branch`, `log -r
/// ancestor(., default)`, `cat -r -- `, `status -0`, and `diff --git`.
fn fake_hg() -> std::path::PathBuf {
    let bin = tempfile::tempdir().expect("fake bin").keep();
    let state = tempfile::tempdir().expect("fake hg state").keep();
    let script = bin.join("hg");
    std::fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env bash
set -u
state="{state}"
cmd="${{1:-}}"
case "$cmd" in
  branch)
    if [[ -f "$state/branch" ]]; then cat "$state/branch"; else echo default; fi ;;
  log)
    if [[ "${{3:-}}" == "ancestor(., default)" ]]; then
      echo "aabbccddeeff00112233445566778899aabbccdd"
    else
      echo "0000000000000000000000000000000000000001"
    fi ;;
  cat)
    file="${{5:-}}"
    if [[ -f "$state/old/$file" ]]; then cat "$state/old/$file"; else exit 1; fi ;;
  status)
    if [[ -f "$state/status" ]]; then
      while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        printf '%s %s\0' "${{line:0:1}}" "${{line:2}}"
      done < "$state/status"
    fi ;;
  diff)
    if [[ -f "$state/diff" ]]; then cat "$state/diff"; fi ;;
  *) exit 0 ;;
esac
"#,
            state = state.display(),
        ),
    )
    .expect("write fake hg");
    let mut permissions = std::fs::metadata(&script).expect("stat fake hg").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).expect("chmod fake hg");
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", bin.display(), path));
    std::env::set_var("FAKE_HG_DIR", state.display().to_string());
    state
}

#[tokio::test]
async fn hg_vcs_wire() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    // An hg project: the `.hg` marker drives detection (no git repo here).
    std::fs::create_dir_all(workspace.join(".hg")).expect("hg marker");
    std::fs::write(workspace.join("README.md"), "changed\n").expect("modified file");
    std::fs::write(workspace.join("new.txt"), "brand new\n").expect("untracked file");
    let state = fake_hg();
    // Working-tree facts: M README.md, ? new.txt, ! gone.txt.
    std::fs::write(state.join("status"), "M README.md\n? new.txt\n! gone.txt\n").expect("status");
    std::fs::create_dir_all(state.join("old")).expect("old dir");
    std::fs::write(state.join("old/gone.txt"), "old contents\n").expect("deleted content");
    std::fs::write(
        state.join("diff"),
        "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-readme\n+changed\n",
    )
    .expect("batch diff");

    let server = boot(workspace.clone()).await;
    let query = location_query(&workspace);

    // info (hg.ts:118-120): `hg branch` + the literal default.
    let (status, body) = raw_get(server.addr, &format!("/api/vcs?{query}")).await;
    assert_eq!(status, 200, "vcs info: {body}");
    assert_eq!(body["data"]["branch"]["current"], "default", "hg branch: {body}");
    assert_eq!(body["data"]["branch"]["default"], "default", "hg default branch: {body}");
    assert_eq!(body["data"]["provider"], "hg");

    // base: no hg base op → ref returns 200 with null data (vcs.ts:198-201).
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/base?{query}")).await;
    assert_eq!(status, 200, "hg base null: {body}");
    assert!(body["data"].is_null(), "hg base data null: {body}");

    // branch list: always [] (hg.ts:121-123).
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/branch?{query}")).await;
    assert_eq!(status, 200, "hg branch list: {body}");
    assert_eq!(body["data"].as_array().map(Vec::len), Some(0), "empty list: {body}");

    // status (hg.ts:124-134): diffAgainst(parent) → FileStatus shape.
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/status?{query}")).await;
    assert_eq!(status, 200, "hg status: {body}");
    let entries = body["data"].as_array().cloned().unwrap_or_default();
    let by_file = |name: &str| {
        entries
            .iter()
            .find(|entry| entry["file"] == name)
            .unwrap_or_else(|| panic!("no {name} in status: {entries:?}"))
            .clone()
    };
    assert_eq!(by_file("README.md")["status"], "modified");
    assert_eq!(by_file("README.md")["additions"], 1, "countPatch: {entries:?}");
    assert_eq!(by_file("README.md")["deletions"], 1, "countPatch: {entries:?}");
    assert_eq!(by_file("new.txt")["status"], "added", "? → added: {entries:?}");
    assert_eq!(by_file("gone.txt")["status"], "deleted", "! → deleted: {entries:?}");

    // working diff (hg.ts:135-136): batched patch + synthesized add/delete
    // patches for `?`/`!` files that never appear in `hg diff`.
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=working")).await;
    assert_eq!(status, 200, "hg working diff: {body}");
    let files = body["data"].as_array().cloned().unwrap_or_default();
    let diff_by_file = |name: &str| {
        files
            .iter()
            .find(|entry| entry["file"] == name)
            .unwrap_or_else(|| panic!("no {name} in diff: {files:?}"))
            .clone()
    };
    let readme = diff_by_file("README.md");
    assert!(readme["patch"].as_str().is_some_and(|p| p.contains("+changed")));
    let new = diff_by_file("new.txt");
    assert!(
        new["patch"].as_str().is_some_and(|p| p.contains("+brand new")),
        "untracked add patch synthesized from disk: {new}"
    );
    assert!(
        new["patch"].as_str().is_some_and(|p| p.contains("@@ -0,0 +1,1 @@")),
        "jsdiff addPatch hunk header: {new}"
    );
    let gone = diff_by_file("gone.txt");
    assert!(
        gone["patch"].as_str().is_some_and(|p| p.contains("-old contents")),
        "missing-file delete patch synthesized from `hg cat`: {gone}"
    );

    // committed mode + explicit base → DiffError → 503 {service: vcs}.
    let (status, body) =
        raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=committed")).await;
    assert_eq!(status, 503, "hg committed unsupported: {body}");
    assert_eq!(body["service"], "vcs");
    let (status, body) =
        raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=branch&base=tip")).await;
    assert_eq!(status, 503, "hg explicit base unsupported: {body}");
    assert_eq!(body["service"], "vcs");

    // branch mode on `default` → [] (hg.ts:143-144).
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=branch")).await;
    assert_eq!(status, 200, "hg branch diff on default: {body}");
    assert_eq!(body["data"].as_array().map(Vec::len), Some(0), "no base on default: {body}");

    // branch mode on a feature branch → diff against ancestor(., default).
    std::fs::write(state.join("branch"), "feature\n").expect("feature branch");
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=branch")).await;
    assert_eq!(status, 200, "hg branch diff: {body}");
    let files = body["data"].as_array().cloned().unwrap_or_default();
    assert!(
        files.iter().any(|entry| entry["file"] == "README.md"),
        "branch diff lists tracked change: {files:?}"
    );
}

/// G-198 — numstat "-" (binary) maps to ref's `{binary: true,
/// additions: 0, deletions: 0, patch: ""}` (git.ts:560-573): a binary
/// change gets an EMPTY patch body, never "Binary files differ".
#[tokio::test]
async fn g198_diff_binary_file_yields_empty_patch() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    std::fs::write(workspace.join("blob.bin"), b"\x00\x01\x02seed\x00").expect("seed binary");
    git(&workspace, &["add", "."]);
    git(&workspace, &["commit", "-m", "binary"]);

    let server = boot(workspace.clone()).await;
    let query = location_query(&workspace);

    // No changes → clean.
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=working")).await;
    assert_eq!(status, 200, "clean: {body}");
    assert_eq!(body["data"].as_array().expect("array").len(), 0);

    // Mutate the binary blob.
    std::fs::write(workspace.join("blob.bin"), b"\x00\xff\xfechother\x00").expect("mutate binary");
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/diff?{query}&mode=working")).await;
    assert_eq!(status, 200, "binary diff: {body}");
    let files = body["data"].as_array().expect("FileDiff[] array").clone();
    assert_eq!(files.len(), 1, "{files:?}");
    let entry = &files[0];
    assert_eq!(entry["file"], "blob.bin");
    assert_eq!(entry["additions"], 0, "numstat \"-\" → 0 additions: {entry}");
    assert_eq!(entry["deletions"], 0, "numstat \"-\" → 0 deletions: {entry}");
    assert_eq!(entry["patch"], "", "binary patch body is empty: {entry}");
}

/// G-361 — the git command family's ref parity: every invocation
/// carries the flag group (`--no-optional-locks` + the `-c core.*`
/// overrides, plugin/vcs/git.ts:158-168), the status scan runs scoped
/// to the LOCATION directory (git.ts:81-99's `ctx.directory` + the
/// `--", "."` pathspec), additions/deletions come from `diff --numstat
/// HEAD` with the untracked `--no-index --numstat` fallback, and the
/// status letters map through the shared `kind` matrix (git.ts:171-178
/// — "AM" contains `A` and no `D` → "added").
#[tokio::test]
async fn g361_git_status_flags_scope_and_stats() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    seed_repo(&workspace);
    // AM: staged add + working edit.
    std::fs::write(workspace.join("staged.txt"), "one\n").expect("staged");
    git(&workspace, &["add", "staged.txt"]);
    std::fs::write(workspace.join("staged.txt"), "one\nTWO\nthree\n").expect("edit staged");
    // M: tracked modification.
    std::fs::write(workspace.join("README.md"), "changed\n").expect("modified");
    // ??: untracked at the root + one inside a subdirectory.
    std::fs::write(workspace.join("new.txt"), "brand new\n").expect("untracked");
    std::fs::create_dir_all(workspace.join("sub")).expect("subdir");
    std::fs::write(workspace.join("sub").join("subfile.txt"), "x\ny\n").expect("sub untracked");

    // A fake `git` on PATH capturing (cwd, argv) then execing the real
    // binary — asserts the flag group and the scoped cwd.
    let bin = tempfile::tempdir().expect("fake bin").keep();
    let argv_log = tempfile::tempdir().expect("argv log").keep().join("git-argv.log");
    let script = bin.join("git");
    std::fs::write(
        &script,
        format!(
            "#!/usr/bin/env bash\nprintf '%s\\t%s\\n' \"$PWD\" \"$*\" >> {argv}\nexec /usr/bin/git \"$@\"\n",
            argv = argv_log.display(),
        ),
    )
    .expect("write fake git");
    let mut permissions = std::fs::metadata(&script).expect("stat").permissions();
    use std::os::unix::fs::PermissionsExt as _;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).expect("chmod");
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));

    let server = boot(workspace.clone()).await;
    let query = location_query(&workspace);

    // Status at the project root: mapping matrix + counted stats.
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/status?{query}")).await;
    assert_eq!(status, 200, "git status: {body}");
    let entries = body["data"].as_array().cloned().unwrap_or_default();
    let by_file = |name: &str| {
        entries
            .iter()
            .find(|entry| entry["file"] == name)
            .unwrap_or_else(|| panic!("no {name} in status: {entries:?}"))
            .clone()
    };
    // AM → "added" (the shared kind matrix — NOT the old
    // status_code_to_kind "modified" bucket).
    let staged = by_file("staged.txt");
    assert_eq!(staged["status"], "added", "AM → added: {staged}");
    assert_eq!(staged["additions"], 3, "numstat HEAD counts: {staged}");
    assert_eq!(staged["deletions"], 0, "numstat HEAD counts: {staged}");
    let readme = by_file("README.md");
    assert_eq!(readme["status"], "modified", "M → modified: {readme}");
    assert_eq!(readme["additions"], 1, "numstat HEAD counts: {readme}");
    assert_eq!(readme["deletions"], 1, "numstat HEAD counts: {readme}");
    let new = by_file("new.txt");
    assert_eq!(new["status"], "added", "?? → added: {new}");
    assert_eq!(new["additions"], 1, "untracked --no-index numstat: {new}");
    assert_eq!(new["deletions"], 0, "untracked --no-index numstat: {new}");

    // Status scoped to the SUBDIRECTORY location: only subfile.txt is
    // listed (the old whole-repo scan from the project root leaked the
    // root entries).
    let sub = location_query(&workspace.join("sub"));
    let (status, body) = raw_get(server.addr, &format!("/api/vcs/status?{sub}")).await;
    assert_eq!(status, 200, "subdir status: {body}");
    let entries = body["data"].as_array().cloned().unwrap_or_default();
    assert_eq!(entries.len(), 1, "subdir scope lists only subfile.txt: {entries:?}");
    // porcelain -z paths are REPO-ROOT-relative even from a subdir cwd
    // (git.ts's file ids), so the untracked statUntracked fallback from
    // the worktree root resolves the same path.
    assert_eq!(entries[0]["file"], "sub/subfile.txt", "{entries:?}");
    assert_eq!(entries[0]["status"], "added", "{entries:?}");
    assert_eq!(entries[0]["additions"], 2, "untracked --no-index numstat: {entries:?}");
    assert_eq!(entries[0]["deletions"], 0, "untracked --no-index numstat: {entries:?}");

    // Every captured git invocation from the server carries the ref
    // flag group (plugin/vcs/git.ts:158-168).
    let log = std::fs::read_to_string(&argv_log).unwrap_or_default();
    assert!(!log.is_empty(), "the fake git captured invocations");
    for line in log.lines() {
        assert!(
            line.contains("--no-optional-locks")
                && line.contains("core.autocrlf=false")
                && line.contains("core.fsmonitor=false")
                && line.contains("core.longpaths=true")
                && line.contains("core.symlinks=true")
                && line.contains("core.quotepath=false"),
            "flag group on {line}"
        );
    }
    // The subdir status scan ran with cwd = the location directory.
    assert!(
        log.lines().any(|line| {
            line.starts_with(workspace.join("sub").display().to_string().as_str())
                && line.contains("status")
        }),
        "scoped cwd in {log}"
    );
}
