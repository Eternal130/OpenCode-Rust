//! BE-1 lane regressions (auditD/auditE/auditF): the newly published SSE
//! event types (publish → stream sees them), fork-copies-history E2E,
//! session.shell EXECUTION (never an LLM prompt), config.update
//! JSONC-preserving round-trip, and the /api/info capabilities shape.

use std::sync::Arc;
use std::time::Duration;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct TestServer {
    client: Client,
    state: Arc<AppState>,
    #[allow(dead_code)]
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

/// A live SSE feed tapped onto the engine bus — exactly what
/// `GET /api/event` streams (`handlers/server.rs::shared_feed`).
fn feed(state: &Arc<AppState>) -> Arc<oc_server::sse::EventFeed> {
    let feed = Arc::new(oc_server::sse::EventFeed::new());
    feed.spawn_tap(&state.engine.bus);
    feed
}

/// Reads frames until one matches `needle` (timeout → panic with the
/// frames seen so far).
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
        if seen.len() > 500 {
            panic!("{context}: frame not found after 500 frames:\n{}", seen.join("\n"));
        }
    }
}

async fn create_session(client: &Client, title: &str) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some(title.to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
}

// ---------------------------------------------------------------------------
// SSE: newly published event types (publish → stream sees them)
// ---------------------------------------------------------------------------

/// credential.updated + credential.switched fire on an activating create,
/// with v2's payload shapes (credential.ts:160-168).
#[tokio::test]
async fn be1_sse_credential_events_on_create_activate_and_remove() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let client = &server.client;
    let mut stream = feed(&server.state).subscribe();

    let entry = client
        .credential()
        .create(&CredentialCreateInput {
            id: None,
            integration_id: "github".to_owned(),
            label: Some("bot".to_owned()),
            activate: Some(true),
            value: CredentialValue::Key(CredentialKey {
                key: "sk-test".to_owned(),
                ..CredentialKey::default()
            }),
        })
        .await
        .expect("credential create");

    let updated =
        expect_frame(&mut stream, "\"type\":\"credential.updated\"", "credential.updated").await;
    assert!(updated.contains("\"data\":{}"), "payload is the empty object: {updated}");
    let switched =
        expect_frame(&mut stream, "\"type\":\"credential.switched\"", "credential.switched").await;
    assert!(switched.contains("\"integrationID\":\"github\""), "{switched}");
    assert!(switched.contains(&format!("\"credentialID\":\"{}\"", entry.id)), "{switched}");

    // Removal of the active row switches to the replacement (or null).
    client.credential().remove(&entry.id).await.expect("remove");
    expect_frame(&mut stream, "\"type\":\"credential.updated\"", "remove updated").await;
    let switched =
        expect_frame(&mut stream, "\"type\":\"credential.switched\"", "remove switched").await;
    assert!(
        switched.contains("\"credentialID\":null") || switched.contains("\"credentialID\":\""),
        "replacement or null: {switched}"
    );
}

/// worktree.updated publishes on a row-owning create (worktree.ts:228).
#[tokio::test]
async fn be1_sse_worktree_updated_on_create() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let client = &server.client;
    let session_id = create_session(client, "worktree-anchor").await;
    // The location assembly created the project row; find its id.
    let project_id: String = server
        .state
        .engine
        .db
        .transaction(|tx| {
            Ok(tx
                .conn()
                .query_row("SELECT id FROM project LIMIT 1", [], |row| row.get(0))
                .expect("project row"))
        })
        .await
        .expect("project lookup");
    let _ = session_id;
    let mut stream = feed(&server.state).subscribe();

    client
        .worktree()
        .create(&WorktreeCreateInput {
            project_id: project_id.clone(),
            name: Some("lane-be1".to_owned()),
            ..Default::default()
        })
        .await
        .expect("worktree create");
    let frame =
        expect_frame(&mut stream, "\"type\":\"worktree.updated\"", "worktree.updated").await;
    assert!(frame.contains(&format!("\"projectID\":\"{project_id}\"")), "{frame}");
}

/// installation.* frames flow from the process-callback port
/// (process.ts:115-117).
#[tokio::test]
async fn be1_sse_installation_events_from_updater_callback() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let mut stream = feed(&server.state).subscribe();
    oc_server::announce::publish_installation_event(&server.state.engine.bus, true, "9.9.9").await;
    let frame =
        expect_frame(&mut stream, "\"type\":\"installation.update-available\"", "available").await;
    assert!(frame.contains("\"version\":\"9.9.9\""), "{frame}");
    oc_server::announce::publish_installation_event(&server.state.engine.bus, false, "9.9.9").await;
    let frame = expect_frame(&mut stream, "\"type\":\"installation.updated\"", "updated").await;
    assert!(frame.contains("\"version\":\"9.9.9\""), "{frame}");
}

/// The tui.* remote-control frames pass the public filter once published
/// (the plugin host's emission path in v2).
#[tokio::test]
async fn be1_sse_tui_frames_pass_the_public_filter() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let mut stream = feed(&server.state).subscribe();
    let bus = &server.state.engine.bus;
    for (type_, data) in [
        ("tui.prompt.append", serde_json::json!({ "text": "hi" })),
        ("tui.command.execute", serde_json::json!({ "command": "session.list" })),
        (
            "tui.toast.show",
            serde_json::json!({ "message": "m", "variant": "info", "duration": 5000 }),
        ),
    ] {
        let definition = oc_core::bus::registry::EventDefinition::ephemeral(type_);
        bus.publish(&definition, data, oc_core::bus::PublishOptions::default())
            .await
            .expect("publish");
        let frame = expect_frame(&mut stream, &format!("\"type\":\"{type_}\""), type_).await;
        assert!(frame.starts_with("data: "), "{frame}");
    }
}

/// filesystem.changed + vcs.branch.updated fire from the location HEAD
/// watcher when the checked-out branch moves (location-watcher.ts +
/// vcs.ts:148).
#[tokio::test]
async fn be1_sse_filesystem_changed_and_branch_updated_on_head_move() {
    let workspace = tempfile::tempdir().expect("workspace");
    assert!(std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(workspace.path())
        .output()
        .expect("git init")
        .status
        .success());
    let server = boot(workspace.path().to_path_buf()).await;
    let location = oc_core::location::LocationRef::new(workspace.path().to_path_buf(), None);

    // Baseline pass: observes the current HEAD, publishes nothing.
    oc_server::announce::watch_location_head(&server.state, &location).await;

    let mut stream = feed(&server.state).subscribe();
    assert!(std::process::Command::new("git")
        .args(["checkout", "-b", "feature-lane-be1"])
        .current_dir(workspace.path())
        .output()
        .expect("git checkout")
        .status
        .success());
    oc_server::announce::watch_location_head(&server.state, &location).await;

    let changed =
        expect_frame(&mut stream, "\"type\":\"filesystem.changed\"", "filesystem.changed").await;
    assert!(changed.contains("\"event\":\"change\""), "{changed}");
    assert!(changed.contains("HEAD"), "payload carries the watched file: {changed}");
    let branch =
        expect_frame(&mut stream, "\"type\":\"vcs.branch.updated\"", "vcs.branch.updated").await;
    assert!(branch.contains("\"branch\":\"feature-lane-be1\""), "{branch}");
}

/// session.status busy/idle (+ deprecated session.idle) ride a real turn:
/// prompt → mock provider rejects → execution.failed → idle.
#[tokio::test]
async fn be1_sse_session_status_busy_then_idle_over_a_real_turn() {
    let workspace = tempfile::tempdir().expect("workspace");
    let provider = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("provider bind");
    let port = provider.local_addr().unwrap().port();
    std::fs::write(
        workspace.path().join("opencode.json"),
        format!(
            r#"{{
                "model": "mock/mock-1",
                "providers": {{
                    "mock": {{
                        "name": "Mock",
                        "npm": "@ai-sdk/openai-compatible",
                        "settings": {{
                            "baseURL": "http://127.0.0.1:{port}/v1",
                            "apiKey": "sk-dummy"
                        }},
                        "models": {{ "mock-1": {{ "name": "Mock 1" }} }}
                    }}
                }}
            }}"#
        ),
    )
    .expect("config");
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = tokio::select! {
                accepted = provider.accept() => match accepted {
                    Ok(accepted) => accepted,
                    Err(_) => return,
                },
                _ = shutdown_rx.changed() => return,
            };
            // Drain the request, then reject with a client error (not
            // retryable — the turn fails immediately).
            let mut buf = [0u8; 8192];
            let _ = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
            let reject = "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n\
                {\"error\":{\"message\":\"nope\",\"type\":\"invalid_request_error\"}}";
            let _ = stream.write_all(reject.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });

    let server = boot(workspace.path().to_path_buf()).await;
    let client = &server.client;
    let mut stream = feed(&server.state).subscribe();
    let session_id = create_session(client, "status").await;
    let _ = client
        .session()
        .prompt(&session_id, &SessionPromptBody { text: "hello".to_owned(), ..Default::default() })
        .await
        .expect("prompt");

    let busy = expect_frame(&mut stream, "\"type\":\"session.status\"", "busy").await;
    assert!(busy.contains("\"status\":{\"type\":\"busy\"}"), "{busy}");
    let idle = expect_frame(&mut stream, "\"status\":{\"type\":\"idle\"}", "idle").await;
    assert!(idle.contains(&format!("\"sessionID\":\"{session_id}\"")), "{idle}");
    let marker = expect_frame(&mut stream, "\"type\":\"session.idle\"", "deprecated marker").await;
    assert!(marker.contains(&format!("\"sessionID\":\"{session_id}\"")), "{marker}");
    shutdown_tx.send(true).expect("stop provider");
}

// ---------------------------------------------------------------------------
// session.fork copies history (auditE core #3)
// ---------------------------------------------------------------------------

/// Seeds one settled user + one settled assistant message on `session_id`.
async fn seed_settled_history(state: &Arc<AppState>, session_id: &str) {
    use oc_core::database::schema::SessionMessageRow;
    let session_id = session_id.to_owned();
    state
        .engine
        .db
        .transaction(move |tx| {
            SessionMessageRow::insert(
                tx,
                &SessionMessageRow {
                    id: "msg_user_root".to_owned(),
                    session_id: session_id.clone(),
                    type_: "user".to_owned(),
                    seq: 0,
                    time_created: 0,
                    time_updated: 0,
                    data: r#"{"text":"root question","time":{"created":0}}"#.to_owned(),
                },
            )?;
            SessionMessageRow::insert(
                tx,
                &SessionMessageRow {
                    id: "msg_assistant_root".to_owned(),
                    session_id,
                    type_: "assistant".to_owned(),
                    seq: 1,
                    time_created: 0,
                    time_updated: 0,
                    data: r#"{"agent":"build","model":{"id":"m","providerID":"p"},"content":[],"time":{"created":0,"completed":9}}"#.to_owned(),
                },
            )?;
            Ok(())
        })
        .await
        .expect("seed history");
}

#[tokio::test]
async fn be1_fork_copies_history_and_fork_styles_the_title() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let client = &server.client;
    let parent = create_session(client, "fork me").await;
    seed_settled_history(&server.state, &parent).await;

    let fork = client.session().fork(&parent, &SessionForkBody::default()).await.expect("fork");
    // v2 wire shape for forks (info.ts:21-27): parent linkage rides the
    // `fork` object (`forkSessionId` column), not `parentID` (NULL for
    // forks — projector.ts:151-152).
    let fork_info = fork.fork.as_ref().expect("fork linkage");
    assert_eq!(fork_info.session_id, parent, "fork points at the parent");
    match &fork_info.boundary {
        SessionForkBoundary::Through(variant) => {
            assert_eq!(variant.message_id, "msg_assistant_root", "through = newest message");
        }
        other => panic!("expected through boundary, got {other:?}"),
    }
    assert_eq!(fork.title.as_deref(), Some("fork me (fork #1)"), "fork-styled title");

    // The projector copied BOTH settled messages.
    let copied = client
        .message()
        .list(&fork.id, &SessionMessageListQuery::default())
        .await
        .expect("fork messages");
    assert_eq!(copied.data.len(), 2, "fork carries the parent's settled history");

    // `before` excludes the boundary message and its successors.
    let fork_before = client
        .session()
        .fork(&parent, &SessionForkBody { before: Some("msg_assistant_root".to_owned()) })
        .await
        .expect("fork before");
    let copied_before = client
        .message()
        .list(&fork_before.id, &SessionMessageListQuery::default())
        .await
        .expect("fork before messages");
    assert_eq!(copied_before.data.len(), 1, "boundary type=before copies strictly below");
    // v2 derives the counter from the PARENT title (projector.ts:159,
    // forkTitle(parent.title)) — the parent title never mutates, so every
    // fork of "fork me" is "(fork #1)".
    assert_eq!(fork_before.title.as_deref(), Some("fork me (fork #1)"), "parent-derived title");

    // v2 error contracts: unknown boundary message → 404; empty session →
    // 400 empty_session.
    let error = client
        .session()
        .fork(&parent, &SessionForkBody { before: Some("msg_missing".to_owned()) })
        .await
        .expect_err("404");
    match &error {
        oc_client::error::ClientError::Api { status, name, .. } => {
            assert_eq!(*status, 404, "{status}");
            assert_eq!(name, "MessageNotFoundError", "{name}");
        }
        other => panic!("expected Api(404), got {other:?}"),
    }
    let empty = create_session(client, "empty").await;
    let error = client.session().fork(&empty, &SessionForkBody::default()).await.expect_err("400");
    match &error {
        oc_client::error::ClientError::Api { status, fields, .. } => {
            assert_eq!(*status, 400, "{status}");
            assert_eq!(fields["kind"], "empty_session", "{fields:?}");
        }
        other => panic!("expected Api(400), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// session.shell EXECUTES (auditE core #2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn be1_session_shell_executes_and_notifies_instead_of_prompting_the_model() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let client = &server.client;
    let session_id = create_session(client, "shell").await;
    let mut stream = feed(&server.state).subscribe();

    client
        .session()
        .shell(
            &session_id,
            &SessionShellBody {
                command: "printf be1-shell-ran".to_owned(),
                id: Some("msg_shell_1".to_owned()),
            },
        )
        .await
        .expect("shell 204");

    // started carries the spawned shell info; ended carries the executed
    // output — proof the command RAN (the old path sent it to the LLM).
    let started =
        expect_frame(&mut stream, "\"type\":\"session.shell.started\"", "shell.started").await;
    assert!(started.contains(&format!("\"sessionID\":\"{session_id}\"")), "{started}");
    assert!(started.contains("\"status\":\"running\""), "{started}");
    let ended = expect_frame(&mut stream, "\"type\":\"session.shell.ended\"", "shell.ended").await;
    assert!(ended.contains("be1-shell-ran"), "ended carries the command output: {ended}");
    assert!(ended.contains("\"status\":\"exited\""), "{ended}");

    // The user-notification synthetic sits in the inbox (resume:false —
    // nothing reached the model; no step events exist for the session).
    let inbox = client.session().inbox().list(&session_id).await.expect("inbox");
    let notified = inbox.iter().any(|item| match item {
        SessionInboxInfo::Synthetic(synthetic) => {
            synthetic.payload.text.contains("be1-shell-ran")
                && synthetic.payload.text.contains("executed by the user")
        }
        _ => false,
    });
    assert!(notified, "user notification admitted: {inbox:?}");
}

// ---------------------------------------------------------------------------
// config.update round-trip (auditE core #8) — JSONC comment preservation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn be1_config_update_writes_shell_preserving_comments() {
    let workspace = tempfile::tempdir().expect("workspace");
    let server = boot(workspace.path().to_path_buf()).await;
    let client = &server.client;
    let config_dir = server.state.engine.home.join(".config").join("opencode");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("opencode.jsonc");
    std::fs::write(&config_path, "{\n  // keep me around\n  \"username\": \"lane-be1\"\n}\n")
        .expect("seed config");

    client
        .config()
        .update(&ConfigPatch { shell: Some("/bin/zsh".to_owned()) })
        .await
        .expect("patch 204");
    let updated = std::fs::read_to_string(&config_path).expect("read back");
    assert!(updated.contains("// keep me around"), "comment preserved: {updated}");
    assert!(updated.contains("\"username\": \"lane-be1\""), "unrelated key intact: {updated}");
    assert!(updated.contains("\"shell\": \"/bin/zsh\""), "shell written: {updated}");

    // null removes the key, comments still intact.
    client.config().update(&ConfigPatch { shell: None }).await.expect("patch null 204");
    let removed = std::fs::read_to_string(&config_path).expect("read back");
    assert!(removed.contains("// keep me around"), "comment still preserved: {removed}");
    assert!(!removed.contains("\"shell\""), "shell key removed: {removed}");
}

// ---------------------------------------------------------------------------
// /api/info capabilities (auditD #9)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
#[tokio::test]
async fn be1_info_carries_persistent_pty_capability() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let info = server.client.server().info().await.expect("info");
    assert_eq!(
        info.capabilities.expect("capabilities").persistent_pty,
        Some(true),
        "persistentPty is true off Windows (binary.node.ts:2)"
    );
}
