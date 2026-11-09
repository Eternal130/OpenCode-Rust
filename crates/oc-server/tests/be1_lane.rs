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
        assert!(
            seen.len() <= 500,
            "{context}: frame not found after 500 frames:\n{}",
            seen.join("\n")
        );
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

/// G-57 — removing a NON-active credential (credential.ts:268-269,
/// `{switched:false}`) publishes only `credential.updated`; no
/// `credential.switched` may follow.
#[tokio::test]
async fn be1_sse_credential_remove_non_active_does_not_switch() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let client = &server.client;
    let mut stream = feed(&server.state).subscribe();

    let dormant = client
        .credential()
        .create(&CredentialCreateInput {
            id: None,
            integration_id: "github".to_owned(),
            label: Some("dormant".to_owned()),
            activate: Some(false),
            value: CredentialValue::Key(CredentialKey {
                key: "sk-dormant".to_owned(),
                ..CredentialKey::default()
            }),
        })
        .await
        .expect("dormant create");
    // G-268 — the FIRST credential of an integration is FORCE-activated
    // (`activate !== false || !current`, credential.ts:121): its create
    // emits `credential.updated` AND `credential.switched`. Drain both.
    expect_frame(&mut stream, "\"type\":\"credential.updated\"", "dormant create updated").await;
    expect_frame(&mut stream, "\"type\":\"credential.switched\"", "dormant forced switched").await;
    let active = client
        .credential()
        .create(&CredentialCreateInput {
            id: None,
            integration_id: "github".to_owned(),
            label: Some("active".to_owned()),
            activate: Some(true),
            value: CredentialValue::Key(CredentialKey {
                key: "sk-active".to_owned(),
                ..CredentialKey::default()
            }),
        })
        .await
        .expect("active create");
    // Drain the create-time events.
    expect_frame(&mut stream, "\"type\":\"credential.updated\"", "active create updated").await;
    expect_frame(&mut stream, "\"type\":\"credential.switched\"", "active create switched").await;

    // Removing the dormant row: updated only, no switched.
    client.credential().remove(&dormant.id).await.expect("remove dormant");
    expect_frame(&mut stream, "\"type\":\"credential.updated\"", "dormant remove updated").await;
    let mut saw_switched = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let Ok(frame) = tokio::time::timeout(remaining, stream.recv()).await else { break };
        let frame = frame.expect("stream ended").expect("ok frame");
        if frame.to_string().contains("\"type\":\"credential.switched\"") {
            saw_switched = true;
            break;
        }
    }
    assert!(!saw_switched, "non-active removal must not emit credential.switched");
    let _ = active;
}

/// (active, `time_created`, `time_updated`) for one credential row, read
/// straight off the engine db (the be1 SQL-probe pattern).
async fn credential_times(server: &TestServer, id: &str) -> (i64, i64, i64) {
    let id = id.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            Ok(tx
                .conn()
                .query_row(
                    "SELECT active, time_created, time_updated FROM credential WHERE id = ?1",
                    [&id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
                )
                .expect("credential row"))
        })
        .await
        .expect("times lookup")
}

/// Pins `time_updated` to a sentinel so a later stability assert cannot
/// pass by clock coincidence (create and mutate within the same ms).
async fn pin_credential_time_updated(server: &TestServer, id: &str, value: i64) {
    let id = id.to_owned();
    server
        .state
        .engine
        .db
        .transaction(move |tx| {
            tx.conn()
                .execute("UPDATE credential SET time_updated = ?2 WHERE id = ?1", (&id, value))
                .expect("pin time_updated");
            Ok(())
        })
        .await
        .expect("pin tx");
}

/// G-117 (HTTP half) — PATCH update and POST activate never write
/// `time_updated`: the ref's HTTP handlers route through
/// Credential.Service whose drizzle `.set({...})` writes only the listed
/// columns (credential.ts:124-130/:187/:227-228), so the column keeps
/// its insert-time value while label edits and selection flips land.
#[tokio::test]
async fn be1_credential_update_activate_keep_time_updated_stable() {
    let server = boot(tempfile::tempdir().expect("workspace").keep()).await;
    let client = &server.client;
    let key = |text: &str| {
        CredentialValue::Key(CredentialKey { key: text.to_owned(), ..CredentialKey::default() })
    };

    let first = client
        .credential()
        .create(&CredentialCreateInput {
            id: None,
            integration_id: "github".to_owned(),
            label: Some("first".to_owned()),
            activate: Some(true),
            value: key("sk-1"),
        })
        .await
        .expect("first create");
    let second = client
        .credential()
        .create(&CredentialCreateInput {
            id: None,
            integration_id: "github".to_owned(),
            label: Some("second".to_owned()),
            activate: Some(false),
            value: key("sk-2"),
        })
        .await
        .expect("second create");

    // Insert seeds time_updated == time_created (initial value preserved).
    for (id, expect_active) in [(&first.id, 1i64), (&second.id, 0)] {
        let (active, created, updated) = credential_times(&server, id).await;
        assert_eq!(active, expect_active, "{id}: selection at insert");
        assert!(created > 0 && updated == created, "{id}: insert sets time_updated = time_created");
    }

    // Pin sentinels BEFORE the mutations (see pin_credential_time_updated).
    pin_credential_time_updated(&server, &first.id, 111_111).await;
    pin_credential_time_updated(&server, &second.id, 222_222).await;

    // PATCH label: the label lands in the list output, everything else
    // (ids, integration, selection) is unchanged, mtime stays pinned.
    client
        .credential()
        .update(&second.id, &CredentialUpdateBody { label: "renamed".to_owned() })
        .await
        .expect("label update");
    let listed = client.credential().list().await.expect("list after update");
    let second_listed = listed.iter().find(|entry| entry.id == second.id).expect("second listed");
    assert_eq!(second_listed.label, "renamed", "label edit lands");
    assert_eq!(second_listed.integration_id, "github", "integration unchanged");
    assert!(!second_listed.active, "selection unchanged by a label edit");
    assert!(
        listed.iter().find(|entry| entry.id == first.id).is_some_and(|entry| entry.active),
        "first stays the selected row"
    );
    assert_eq!(credential_times(&server, &second.id).await.2, 222_222, "label update keeps mtime");

    // POST activate: selection flips, both rows keep their mtimes (the
    // deactivated sibling's write is the same column-limited set).
    client.credential().activate(&second.id).await.expect("activate");
    let listed = client.credential().list().await.expect("list after activate");
    assert!(
        listed.iter().find(|entry| entry.id == second.id).is_some_and(|entry| entry.active),
        "second is now the selected row"
    );
    assert!(
        !listed.iter().find(|entry| entry.id == first.id).is_some_and(|entry| entry.active),
        "first is deselected"
    );
    let (first_active, _, first_updated) = credential_times(&server, &first.id).await;
    let (second_active, _, second_updated) = credential_times(&server, &second.id).await;
    assert_eq!((first_active, first_updated), (0, 111_111), "deactivated row keeps its mtime");
    assert_eq!((second_active, second_updated), (1, 222_222), "activated row keeps its mtime");
}

/// worktree.updated publishes on a row-owning create (worktree.ts:228).
#[tokio::test]
async fn be1_sse_worktree_updated_on_create() {
    // BE-2a made worktree.create run the real `git worktree add`, so the
    // fixture needs an actual git source checkout.
    let workspace = tempfile::tempdir().expect("workspace").keep();
    std::fs::write(workspace.join("seed.txt"), "seed\n").expect("seed file");
    for args in [
        vec!["init", "--initial-branch=main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
        vec!["add", "."],
        vec!["commit", "-m", "seed"],
    ] {
        let output = std::process::Command::new("git")
            .args(&args)
            .current_dir(&workspace)
            .output()
            .expect("git spawn");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let server = boot(workspace).await;
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
    // The announcer's branch subscription (the production shell starts
    // this; the bare test harness does not).
    oc_server::announce::ensure_started(&server.state);
    let location = oc_core::location::LocationRef::new(workspace.path().to_path_buf(), None);
    // Assemble the location so the ENGINE watcher (the sole
    // filesystem.changed publisher) is live.
    server.state.engine.location(location.clone()).await.expect("assemble");
    assert!(
        server.state.engine.has_location_watcher(&location),
        "engine watcher armed at assembly"
    );
    // Let the native watch arm before moving HEAD.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut stream = feed(&server.state).subscribe();
    assert!(std::process::Command::new("git")
        .args(["checkout", "-b", "feature-lane-be1"])
        .current_dir(workspace.path())
        .output()
        .expect("git checkout")
        .status
        .success());

    let changed =
        expect_frame(&mut stream, "\"type\":\"filesystem.changed\"", "filesystem.changed").await;
    assert!(changed.contains("\"event\":\"change\""), "{changed}");
    assert!(changed.contains("HEAD"), "payload carries the watched file: {changed}");
    let branch =
        expect_frame(&mut stream, "\"type\":\"vcs.branch.updated\"", "vcs.branch.updated").await;
    assert!(branch.contains("\"branch\":\"feature-lane-be1\""), "{branch}");

    // G-218: exactly one filesystem.changed for the HEAD write — drain a
    // quiet window and count (the old dual-publisher shape produced a
    // second copy from the announce poller; the pump's coalescing merges
    // one write's truncate+data notify pair).
    let mut filesystem_changed = 1usize;
    while let Ok(Some(Ok(frame))) =
        tokio::time::timeout(std::time::Duration::from_millis(700), stream.recv()).await
    {
        if frame.contains("\"type\":\"filesystem.changed\"") {
            filesystem_changed += 1;
        }
    }
    assert_eq!(
        filesystem_changed, 1,
        "G-218: the engine watcher is the SOLE filesystem.changed publisher"
    );
}
