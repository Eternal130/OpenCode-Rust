//! Turn-range session diff tests (session-diff semantics from
//! `session/diff.ts`): anchor resolution, the idle-marker turn model, the
//! snapshot bracket, and the location-switch guard.

use std::path::PathBuf;

use oc_schema::session_support::{FileDiff, FileDiffStatus};

use crate::database::Database;
use crate::snapshot::SnapshotService as _;

struct DiffStack {
    _tmp: tempfile::TempDir,
    db: Database,
    worktree: PathBuf,
    store: std::sync::Arc<crate::snapshot::GitSnapshotStore>,
}

async fn diff_stack() -> DiffStack {
    let tmp = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let db = Database::open_file(tmp.path().join("opencode.db")).await.expect("db");
    let worktree = tmp.path().join("repo");
    std::fs::create_dir_all(&worktree).expect("worktree");
    git2::Repository::init(&worktree).expect("init");
    let store = std::sync::Arc::new(
        crate::snapshot::GitSnapshotStore::open(
            &tmp.path().join("data"),
            "proj_diff",
            &worktree,
            ".",
        )
        .expect("store"),
    );
    DiffStack { _tmp: tmp, db, worktree, store }
}

async fn seed_session(db: &Database, session_id: &str, directory: &str) {
    let (id, directory) = (session_id.to_string(), directory.to_string());
    db.transaction(move |tx| {
        tx.conn().execute(
            "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) \
             VALUES ('global', '', 0, 0, '')",
            [],
        )?;
        tx.conn().execute(
            "INSERT INTO session_v2 (id, project_id, slug, directory, path, version, cost, \
             tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, \
             time_created, time_updated, resume_attempts) \
             VALUES (?1, 'global', 'diff-test', ?2, NULL, 'test', 0, 0, 0, 0, 0, 0, 0, 0, 0)",
            rusqlite::params![id, directory],
        )?;
        Ok(())
    })
    .await
    .expect("seed session");
}

/// One `session_message` row with arbitrary `data`.
async fn insert(db: &Database, session_id: &str, id: &str, kind: &str, data: serde_json::Value) {
    let (session, id, kind) = (session_id.to_string(), id.to_string(), kind.to_string());
    db.transaction(move |tx| {
        tx.conn().execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) \
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(seq), 0) + 1 FROM session_message WHERE session_id = ?2), 0, 0, ?4)",
            rusqlite::params![id, session, kind, data.to_string()],
        )?;
        Ok(())
    })
    .await
    .expect("insert message");
}

async fn user(db: &Database, session: &str, id: &str) {
    insert(db, session, id, "user", serde_json::json!({ "time": { "created": 0 } })).await;
}

async fn idle(db: &Database, session: &str, id: &str) {
    insert(db, session, id, "idle", serde_json::json!({ "time": { "created": 0 } })).await;
}

async fn assistant(
    db: &Database,
    session: &str,
    id: &str,
    start: Option<&str>,
    end: Option<&str>,
    completed: Option<i64>,
) {
    let payload = serde_json::json!({
        "time": { "created": 0, "streamed": 0, "completed": completed },
        "snapshot": { "start": start, "end": end, "files": [] },
    });
    insert(db, session, id, "assistant", payload).await;
}

fn session_row(directory: &str) -> crate::database::schema::SessionRow {
    crate::database::schema::SessionRow::new("ses_diff_1", "global", directory, "diff-test", "test")
        .expect("session row")
}

#[tokio::test]
async fn turn_diffs_the_snapshot_bracket_of_the_last_turn() {
    let stack = diff_stack().await;
    let session = "ses_diff_1";
    seed_session(&stack.db, session, &stack.worktree.to_string_lossy()).await;

    // Turn 1: file written; turn 2: file edited.
    std::fs::write(stack.worktree.join("notes.txt"), "v1\n").expect("write");
    let start = stack.store.capture().await.expect("capture 1");
    assistant(&stack.db, session, "msg_a1", Some(start.as_str()), Some(start.as_str()), Some(1))
        .await;
    user(&stack.db, session, "msg_u2").await;
    std::fs::write(stack.worktree.join("notes.txt"), "v1\nv2\n").expect("edit");
    let end = stack.store.capture().await.expect("capture 2");
    assistant(&stack.db, session, "msg_a2", Some(start.as_str()), Some(end.as_str()), Some(2))
        .await;

    let row = session_row(&stack.worktree.to_string_lossy());
    let diffs = crate::session::diff::turn(
        &stack.db,
        &(stack.store.clone() as std::sync::Arc<dyn crate::snapshot::SnapshotService>),
        &row,
        false,
        None,
        None,
        None,
    )
    .await
    .expect("turn diff");
    assert_eq!(diffs.len(), 1);
    assert_eq!(
        diffs[0],
        FileDiff {
            file: "notes.txt".to_string(),
            patch: diffs[0].patch.clone(),
            additions: 1,
            deletions: 0,
            status: FileDiffStatus::Modified,
        }
    );
    assert!(diffs[0].patch.contains("+v2"));
}

#[tokio::test]
async fn idle_markers_bound_turns_and_steered_prompts_share_one() {
    let stack = diff_stack().await;
    let session = "ses_diff_1";
    seed_session(&stack.db, session, &stack.worktree.to_string_lossy()).await;

    // Empty baseline, then turn 1 adds a.txt, turn 2 adds b.txt.
    let base = stack.store.capture().await.expect("capture base");
    std::fs::write(stack.worktree.join("a.txt"), "one\n").expect("write");
    let first = stack.store.capture().await.expect("capture 1");
    user(&stack.db, session, "msg_u1").await;
    assistant(&stack.db, session, "msg_a1", Some(base.as_str()), Some(first.as_str()), Some(1))
        .await;
    idle(&stack.db, session, "msg_idle1").await;

    std::fs::write(stack.worktree.join("b.txt"), "two\n").expect("write");
    let second = stack.store.capture().await.expect("capture 2");
    user(&stack.db, session, "msg_u2").await;
    // A prompt steered in while busy belongs to the same turn.
    user(&stack.db, session, "msg_u2_steer").await;
    assistant(&stack.db, session, "msg_a2", Some(first.as_str()), Some(second.as_str()), Some(2))
        .await;
    idle(&stack.db, session, "msg_idle2").await;

    let row = session_row(&stack.worktree.to_string_lossy());
    // The latest turn covers only b.txt (first→second), not a.txt.
    let diffs = crate::session::diff::turn(
        &stack.db,
        &(stack.store.clone() as std::sync::Arc<dyn crate::snapshot::SnapshotService>),
        &row,
        false,
        None,
        None,
        None,
    )
    .await
    .expect("turn diff");
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].file, "b.txt");
    assert_eq!(diffs[0].status, FileDiffStatus::Added);

    // `from` + `to` extends the range through both turns:
    // first→second spans both files.
    let both = crate::session::diff::turn(
        &stack.db,
        &(stack.store.clone() as std::sync::Arc<dyn crate::snapshot::SnapshotService>),
        &row,
        false,
        Some("msg_u1"),
        Some("msg_u2"),
        None,
    )
    .await
    .expect("turn diff from");
    assert_eq!(both.len(), 2);
}

#[tokio::test]
async fn range_errors_are_typed() {
    let stack = diff_stack().await;
    let session = "ses_diff_1";
    seed_session(&stack.db, session, &stack.worktree.to_string_lossy()).await;
    let first = stack.store.capture().await.expect("capture");
    user(&stack.db, session, "msg_u1").await;
    assistant(&stack.db, session, "msg_a1", Some(first.as_str()), Some(first.as_str()), Some(1))
        .await;
    user(&stack.db, session, "msg_u2").await;
    let row = session_row(&stack.worktree.to_string_lossy());

    // Unknown message id.
    let error = crate::session::diff::resolve_turn(&stack.db, &row, Some("msg_nope"), None, None)
        .await
        .expect_err("unknown anchor");
    assert!(matches!(error, crate::session::diff::DiffError::MessageNotFound { .. }));

    // Non-user anchors are rejected.
    let error = crate::session::diff::resolve_turn(&stack.db, &row, Some("msg_a1"), None, None)
        .await
        .expect_err("assistant anchor");
    assert!(matches!(
        error,
        crate::session::diff::DiffError::TurnRange(ref range)
            if range.message == "Message msg_a1 is not a user message"
    ));

    // `to` before `from`.
    let error =
        crate::session::diff::resolve_turn(&stack.db, &row, Some("msg_u2"), Some("msg_u1"), None)
            .await
            .expect_err("reversed range");
    assert!(matches!(
        error,
        crate::session::diff::DiffError::TurnRange(ref range)
            if range.message == "Message msg_u1 precedes msg_u2"
    ));
}

#[tokio::test]
async fn a_range_spanning_a_location_change_is_rejected() {
    let stack = diff_stack().await;
    let session = "ses_diff_1";
    let directory = stack.worktree.to_string_lossy().into_owned();
    seed_session(&stack.db, session, &directory).await;
    let first = stack.store.capture().await.expect("capture");
    user(&stack.db, session, "msg_u1").await;
    assistant(&stack.db, session, "msg_a1", Some(first.as_str()), None, None).await;
    // The switch lands between the turn's steps.
    insert(
        &stack.db,
        session,
        "msg_switch",
        "location-switched",
        serde_json::json!({
            "time": { "created": 0 },
            "location": { "directory": "/other/repo" },
            "previous": { "location": { "directory": directory } },
        }),
    )
    .await;
    assistant(&stack.db, session, "msg_a2", Some(first.as_str()), Some(first.as_str()), Some(2))
        .await;
    let row = session_row(&directory);
    let error = crate::session::diff::resolve_turn(&stack.db, &row, None, None, None)
        .await
        .expect_err("spanning switch");
    assert!(matches!(
        error,
        crate::session::diff::DiffError::TurnRange(ref range)
            if range.message == "Turn range spans a location change"
    ));
}

#[tokio::test]
async fn sessions_without_snapshots_or_prompts_diff_empty() {
    let stack = diff_stack().await;
    let session = "ses_diff_1";
    seed_session(&stack.db, session, &stack.worktree.to_string_lossy()).await;
    let row = session_row(&stack.worktree.to_string_lossy());

    // No messages at all.
    assert!(crate::session::diff::resolve_turn(&stack.db, &row, None, None, None)
        .await
        .expect("resolve")
        .is_none());

    // A prompt without any assistant steps.
    user(&stack.db, session, "msg_u1").await;
    assert!(crate::session::diff::resolve_turn(&stack.db, &row, None, None, None)
        .await
        .expect("resolve")
        .is_none());

    // A step without a start snapshot.
    assistant(&stack.db, session, "msg_a1", None, None, Some(1)).await;
    assert!(crate::session::diff::resolve_turn(&stack.db, &row, None, None, None)
        .await
        .expect("resolve")
        .is_none());
}

#[tokio::test]
async fn legacy_sessions_without_idle_markers_span_to_the_next_prompt() {
    let stack = diff_stack().await;
    let session = "ses_diff_1";
    seed_session(&stack.db, session, &stack.worktree.to_string_lossy()).await;
    let first = stack.store.capture().await.expect("capture");
    user(&stack.db, session, "msg_u1").await;
    assistant(&stack.db, session, "msg_a1", Some(first.as_str()), Some(first.as_str()), Some(1))
        .await;
    user(&stack.db, session, "msg_u2").await;
    assistant(&stack.db, session, "msg_a2", Some(first.as_str()), Some(first.as_str()), Some(2))
        .await;
    let row = session_row(&stack.worktree.to_string_lossy());

    // Legacy: the latest prompt's turn ends at the next prompt, so only
    // the second assistant step is in range — and its bracket compares the
    // same tree to itself (no changes).
    let plan = crate::session::diff::resolve_turn(&stack.db, &row, None, None, None)
        .await
        .expect("resolve")
        .expect("plan");
    assert_eq!(plan.from, first.as_str());
    assert_eq!(plan.recorded.as_deref(), Some(first.as_str()));
    assert!(!plan.capture);
}
