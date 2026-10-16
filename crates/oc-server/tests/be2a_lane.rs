//! BE-2a lane regressions (auditE #4/#5/#10): the websearch routes over
//! the wired provider registry (typed no-provider error, env-key
//! eligibility), worktree.create running the real `git worktree add`, and
//! the models-dev refresh trigger with the frozen-snapshot fallback.

use std::sync::Arc;
use std::time::Duration;

use oc_client::types::*;
use oc_client::{Client, ClientError};
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    #[allow(dead_code)] // the workspace root the server was booted in
    workspace: std::path::PathBuf,
    _data: tempfile::TempDir,
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
    TestServer { client, state, workspace, _data: data }
}

// ---------------------------------------------------------------------------
// Websearch (auditE #4): the routes now run the location's WebSearch
// service — zero eligible providers is v2's typed 400, not a 503 stub.
// ---------------------------------------------------------------------------

/// `websearch.query` with no configured provider: the v2
/// `WebSearch.ProviderRequired` mapping — 400, kind
/// `websearch_provider_required`, field `providerID`
/// (server/handlers/websearch.ts).
#[tokio::test]
async fn be2a_websearch_query_without_provider_is_typed_400() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let error = server
        .client
        .websearch()
        .query(
            &WebsearchQueryQuery::default(),
            &WebsearchQueryBody { query: "rust async".into(), provider_id: None },
        )
        .await
        .expect_err("no provider configured");
    let ClientError::Api { status, name, fields, .. } = &error else {
        panic!("expected declared api error, got {error}");
    };
    assert_eq!(*status, 400);
    assert_eq!(name, "InvalidRequestError");
    assert_eq!(fields.get("kind"), Some(&serde_json::json!("websearch_provider_required")));
    assert_eq!(fields.get("field"), Some(&serde_json::json!("providerID")));
}

/// `websearch.providers` with no keys configured returns the documented
/// empty list (data is a bare array of `{id, name}`).
#[tokio::test]
async fn be2a_websearch_providers_empty_without_keys() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let response = server
        .client
        .websearch()
        .providers(&WebsearchProvidersQuery::default())
        .await
        .expect("providers");
    assert_eq!(response.data, Vec::new(), "no providers without keys");
}

/// An env API key (the v2 `env` connection method names) makes the
/// provider eligible: the providers route lists it.
#[tokio::test]
async fn be2a_websearch_env_key_lists_provider() {
    std::env::set_var("TAVILY_API_KEY", "tvly-test-key");
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let response = server
        .client
        .websearch()
        .providers(&WebsearchProvidersQuery::default())
        .await
        .expect("providers");
    std::env::remove_var("TAVILY_API_KEY");
    let ids: Vec<&str> = response.data.iter().map(|provider| provider.id.as_str()).collect();
    assert_eq!(ids, vec!["tavily"], "exactly the env-keyed provider is eligible");
}

// ---------------------------------------------------------------------------
// Worktree (auditE #10): create runs the real `git worktree add`.
// ---------------------------------------------------------------------------

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output =
        std::process::Command::new("git").args(args).current_dir(dir).output().expect("git spawn");
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A live SSE feed tapped onto the engine bus (`GET /api/event`).
fn feed(state: &Arc<AppState>) -> Arc<oc_server::sse::EventFeed> {
    let feed = Arc::new(oc_server::sse::EventFeed::new());
    feed.spawn_tap(&state.engine.bus);
    feed
}

/// Reads frames until one matches `needle` (timeout → panic).
async fn expect_frame(
    subscription: &mut oc_server::sse::Subscription,
    needle: &str,
    context: &str,
) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let frame = tokio::time::timeout(remaining, subscription.recv())
            .await
            .expect("timeout waiting for frame")
            .expect("stream ended")
            .expect("ok frame");
        let text = frame.to_string();
        if text.contains(needle) {
            return text;
        }
        seen.push(text);
        assert!(
            seen.len() <= 500,
            "{context}: frame not found after 500 frames:\n{}",
            seen.join("\n")
        );
    }
}

/// worktree.create E2E on a temp git repo: the directory exists on disk,
/// is a real linked git worktree at the source commit (detached), the DB
/// row is owned by the `git` strategy, and `worktree.updated` fires
/// (v2 worktree.ts:192-238 + worktree/git.ts).
#[tokio::test]
async fn be2a_worktree_create_runs_git_add_and_publishes() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    std::fs::write(workspace.join("README.md"), "seed\n").expect("seed file");
    git(&workspace, &["init", "--initial-branch=main"]);
    git(&workspace, &["config", "user.email", "test@example.com"]);
    git(&workspace, &["config", "user.name", "Test"]);
    git(&workspace, &["add", "."]);
    git(&workspace, &["commit", "-m", "seed"]);

    let server = boot(workspace.clone()).await;
    // Assemble the location (registers the project row).
    let _session = server
        .client
        .session()
        .create(&SessionCreateBody { title: Some("wt".to_owned()), ..Default::default() })
        .await
        .expect("create session");

    let (project_id, project_worktree) = server
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
    assert!(project_worktree.starts_with(workspace.to_str().unwrap()), "project canonical");

    let mut subscription = feed(&server.state).subscribe();
    let parent = tempfile::tempdir().expect("worktree parent").keep();
    let created = server
        .client
        .worktree()
        .create(&WorktreeCreateInput {
            project_id: project_id.clone(),
            name: Some("feature-x".to_owned()),
            directory: Some(parent.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect("worktree create");
    let directory = std::path::PathBuf::from(&created.directory);
    assert_eq!(directory, parent.join("feature-x"));
    assert!(directory.is_dir(), "the worktree directory exists on disk");

    // A real linked worktree: same common dir as the source, detached HEAD.
    let common = git(&directory, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let source_common =
        git(&workspace, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    assert_eq!(common.trim(), source_common.trim(), "linked to the source repository");
    assert_eq!(git(&directory, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(), "HEAD");
    assert_eq!(
        git(&directory, &["rev-parse", "HEAD"]).trim(),
        git(&workspace, &["rev-parse", "HEAD"]).trim(),
        "checked out at the source commit"
    );
    assert!(std::fs::read_to_string(directory.join("README.md"))
        .expect("file present")
        .contains("seed"));

    // The row is owned by the git strategy (v2 stores `selected.id`).
    let strategy_project = project_id.clone();
    let strategy_directory = created.directory.clone();
    let strategy = server
        .state
        .engine
        .db
        .transaction(move |tx| {
            Ok(tx.conn().query_row(
                "SELECT strategy FROM worktree WHERE project_id = ?1 AND directory = ?2",
                (&strategy_project, strategy_directory.as_str()),
                |row| row.get::<_, Option<String>>(0),
            )?)
        })
        .await
        .expect("row");
    assert_eq!(strategy.as_deref(), Some("git"));

    let frame = expect_frame(&mut subscription, "worktree.updated", "worktree create").await;
    assert!(frame.contains(&project_id), "event carries the projectID: {frame}");
}

/// The v2 error surfaces: an unregistered `from` directory is
/// `SourceDirectoryNotFoundError`; an existing destination rolls through
/// the `-2` suffix (v2 worktree.ts:207-215).
#[tokio::test]
async fn be2a_worktree_create_source_and_suffix_semantics() {
    let workspace = tempfile::tempdir().expect("workspace").keep();
    std::fs::write(workspace.join("a.txt"), "a\n").expect("seed file");
    git(&workspace, &["init", "--initial-branch=main"]);
    git(&workspace, &["config", "user.email", "test@example.com"]);
    git(&workspace, &["config", "user.name", "Test"]);
    git(&workspace, &["add", "."]);
    git(&workspace, &["commit", "-m", "seed"]);

    let server = boot(workspace.clone()).await;
    let _session = server
        .client
        .session()
        .create(&SessionCreateBody { title: Some("wt".to_owned()), ..Default::default() })
        .await
        .expect("create session");
    let project_id = server
        .state
        .engine
        .db
        .transaction(|tx| {
            Ok(tx
                .conn()
                .query_row("SELECT id FROM project LIMIT 1", [], |row| row.get::<_, String>(0))?)
        })
        .await
        .expect("project row");

    // Unregistered `from` → typed source_not_found (v2
    // SourceDirectoryNotFoundError; the project's own checkout is the
    // implicit registered source).
    let stranger = tempfile::tempdir().expect("stranger").keep();
    let error = server
        .client
        .worktree()
        .create(&WorktreeCreateInput {
            project_id: project_id.clone(),
            name: Some("nope".to_owned()),
            from: Some(stranger.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect_err("unregistered source");
    assert!(
        error.to_string().contains("Worktree source not found"),
        "source error surfaces: {error}"
    );

    // Collision → the `-2` suffix (v2 suffix loop).
    let parent = tempfile::tempdir().expect("parent").keep();
    std::fs::create_dir_all(parent.join("dupe")).expect("occupied name");
    let created = server
        .client
        .worktree()
        .create(&WorktreeCreateInput {
            project_id,
            name: Some("dupe".to_owned()),
            directory: Some(parent.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect("create with suffix");
    assert_eq!(std::path::PathBuf::from(&created.directory), parent.join("dupe-2"));
    assert!(std::path::PathBuf::from(&created.directory).join(".git").exists());
}
