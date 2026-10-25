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

async fn boot(workspace: std::path::PathBuf) -> TestServer {
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
            Ok(tx.execute(
                "UPDATE worktree SET strategy = 'hg' WHERE project_id = ?1 AND directory = ?2",
                (update_project, update_directory),
            )?)
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
