//! Snapshot tests (Wave C1) — `git.test.ts` tree cases +
//! `session-revert.test.ts` staging flow against a real git source
//! repository (git2 `Repository::init`), with SHA-1 vectors for
//! `Hash.fast`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oc_schema::id::{MessageId, SessionId};

use crate::snapshot::{location_scope, GitSnapshotStore, SnapshotService};

fn temp_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let worktree = tmp.path().join("repo");
    std::fs::create_dir_all(&worktree).expect("worktree");
    git2::Repository::init(&worktree).expect("init source repo");
    (tmp, worktree.clone(), worktree)
}

fn store(data_root: &Path, worktree: &Path, scope: &str) -> GitSnapshotStore {
    GitSnapshotStore::open(data_root, "proj_snap", worktree, scope).expect("open store")
}

#[test]
fn snapshot_hash_fast_matches_sha1() {
    // Vectors from FIPS 180-1.
    assert_eq!(crate::snapshot::hash_fast(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        crate::snapshot::hash_fast(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
    );
    assert_eq!(crate::snapshot::hash_fast(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    // Hash.fast(worktree) — the store directory derivation.
    let digest = crate::snapshot::hash_fast(b"/repo/worktree");
    assert_eq!(digest.len(), 40);
}

#[test]
fn snapshot_location_scope_is_worktree_relative() {
    let worktree = Path::new("/repo");
    assert_eq!(location_scope(worktree, worktree).as_deref(), Some("."));
    assert_eq!(location_scope(worktree, &worktree.join("sub")).as_deref(), Some("sub"));
    assert_eq!(location_scope(worktree, Path::new("/elsewhere")), None);
}

#[tokio::test]
async fn snapshot_capture_files_diff_restore_cycle() {
    let (tmp, worktree, _scope_root) = temp_project();
    std::fs::write(worktree.join("first.txt"), "one\n").expect("write");
    std::fs::create_dir_all(worktree.join("nested")).expect("dir");
    std::fs::write(worktree.join("nested/second.txt"), "two\n").expect("write");
    let service = store(tmp.path(), &worktree, ".");

    let first = service.capture().await.expect("capture 1");
    // Same content ⇒ same tree (content-addressed).
    let again = service.capture().await.expect("capture 2");
    assert_eq!(first, again);

    // Mutate + capture.
    std::fs::write(worktree.join("first.txt"), "one edited\n").expect("edit");
    std::fs::write(worktree.join("third.txt"), "three\n").expect("add");
    std::fs::remove_file(worktree.join("nested/second.txt")).expect("delete");
    let second = service.capture().await.expect("capture 2b");
    assert_ne!(first, second);

    // files: both sides of the nested delete list as changes.
    let files = service.files(&first, &second).await.expect("files");
    let mut paths: Vec<String> = files.iter().map(|p| p.0.clone()).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec!["first.txt".to_string(), "nested/second.txt".to_string(), "third.txt".to_string()]
    );

    // diff: statuses + counts + patches.
    let diffs = service.diff(&first, &second, None, None).await.expect("diff");
    let by_file: BTreeMap<&str, &oc_schema::session_support::FileDiff> =
        diffs.iter().map(|d| (d.file.as_str(), d)).collect();
    assert_eq!(by_file["first.txt"].status, oc_schema::session_support::FileDiffStatus::Modified);
    assert_eq!(by_file["first.txt"].additions, 1);
    assert_eq!(by_file["first.txt"].deletions, 1);
    assert!(by_file["first.txt"].patch.contains("-one\n+one edited"));
    assert_eq!(by_file["third.txt"].status, oc_schema::session_support::FileDiffStatus::Added);
    assert_eq!(
        by_file["nested/second.txt"].status,
        oc_schema::session_support::FileDiffStatus::Deleted
    );
    // Explicit path selection.
    let selected = service
        .diff(&first, &second, None, Some(&[oc_schema::common::RelativePath("third.txt".into())]))
        .await
        .expect("diff selected");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].file, "third.txt");

    // restore: first.txt back to the original tree; absent path deletes.
    let mut restore = BTreeMap::new();
    restore.insert("first.txt".to_string(), first.clone());
    restore.insert("third.txt".to_string(), first.clone());
    service.restore(&restore).await.expect("restore");
    assert_eq!(std::fs::read_to_string(worktree.join("first.txt")).unwrap(), "one\n");
    assert!(!worktree.join("third.txt").exists(), "absent from tree ⇒ deleted");
    assert!(!worktree.join("nested/second.txt").exists(), "untouched path stays deleted");

    // Path escaping the project is rejected.
    let mut escape = BTreeMap::new();
    escape.insert("../outside.txt".to_string(), first.clone());
    let error = service.restore(&escape).await.expect_err("escape");
    assert!(error.to_string().contains("Path escapes the project"), "{error}");
}

#[tokio::test]
async fn snapshot_respects_source_ignores_and_untracked_cap() {
    let (tmp, worktree, _) = temp_project();
    // Seed the source repo index: tracked.txt committed.
    let repo = git2::Repository::open(&worktree).expect("open");
    let signature = git2::Signature::now("t", "t@t").unwrap();
    let mut index = repo.index().unwrap();
    std::fs::write(worktree.join("tracked.txt"), "tracked\n").unwrap();
    index.add_path(Path::new("tracked.txt")).unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "init", &tree, &[]).unwrap();

    std::fs::write(worktree.join(".gitignore"), "ignored.txt\nbig.bin\n").unwrap();
    std::fs::write(worktree.join("ignored.txt"), "nope\n").unwrap();
    std::fs::write(worktree.join("big.bin"), vec![0u8; 3 * 1024 * 1024]).unwrap();
    std::fs::write(worktree.join("kept.txt"), "kept\n").unwrap();

    let service = store(tmp.path(), &worktree, ".");
    let capture = service.capture().await.expect("capture");
    let repo = crate::snapshot::snapshot_files_of(tmp.path(), &worktree, &capture);
    assert!(repo.contains(&"tracked.txt".to_string()));
    assert!(repo.contains(&"kept.txt".to_string()));
    assert!(!repo.contains(&"ignored.txt".to_string()), "source-ignored untracked file skipped");
    assert!(!repo.contains(&"big.bin".to_string()), "untracked file over 2MiB skipped");
    assert!(!repo.contains(&".git".to_string()));
    drop(service);

    // tracked-but-ignored files stay in the snapshot (diff-files semantics).
    std::fs::write(worktree.join(".gitignore"), "ignored.txt\ntracked.txt\n").unwrap();
    let service = store(tmp.path(), &worktree, ".");
    std::fs::write(worktree.join("tracked.txt"), "tracked edited\n").unwrap();
    let capture = service.capture().await.expect("capture 2");
    let files = crate::snapshot::snapshot_files_of(tmp.path(), &worktree, &capture);
    assert!(files.contains(&"tracked.txt".to_string()), "tracked files are always captured");
}

#[tokio::test]
async fn snapshot_scope_limits_capture_to_the_location_directory() {
    let (tmp, worktree, _) = temp_project();
    std::fs::create_dir_all(worktree.join("sub")).expect("dir");
    std::fs::write(worktree.join("root.txt"), "root\n").unwrap();
    std::fs::write(worktree.join("sub/inner.txt"), "inner\n").unwrap();
    let service = store(tmp.path(), &worktree, "sub");
    let capture = service.capture().await.expect("capture");
    let files = crate::snapshot::snapshot_files_of(tmp.path(), &worktree, &capture);
    assert_eq!(files, vec!["sub/inner.txt".to_string()]);
}

#[tokio::test]
async fn snapshot_disabled_capture_returns_none() {
    let (tmp, worktree, _) = temp_project();
    std::fs::write(worktree.join("a.txt"), "a\n").unwrap();
    let service = store(tmp.path(), &worktree, ".");
    service.configure(false);
    assert!(service.capture().await.is_none());
    service.configure(true);
    assert!(service.capture().await.is_some());
}

// ---------------------------------------------------------------------------
// Revert flow (session-revert.test.ts)
// ---------------------------------------------------------------------------

struct RevertStack {
    _tmp: tempfile::TempDir,
    db: crate::database::Database,
    bus: crate::bus::Bus,
    worktree: PathBuf,
    store: std::sync::Arc<GitSnapshotStore>,
}

async fn revert_stack() -> RevertStack {
    let tmp = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let db =
        crate::database::Database::open_file(tmp.path().join("opencode.db")).await.expect("db");
    let bus = crate::bus::Bus::new(
        db.clone(),
        crate::bus::BusOptions {
            clock: std::sync::Arc::new(oc_util::determinism::FixedClock::epoch()),
            ids: std::sync::Arc::new(oc_util::determinism::SequentialIdGen::new()),
            persist: true,
            log_read_page_size: 512,
        },
    );
    crate::session::projector::install_projectors(&bus);
    let worktree = tmp.path().join("repo");
    std::fs::create_dir_all(&worktree).expect("worktree");
    git2::Repository::init(&worktree).expect("init");
    let store = std::sync::Arc::new(
        GitSnapshotStore::open(&tmp.path().join("data"), "proj_revert", &worktree, ".")
            .expect("store"),
    );
    RevertStack { _tmp: tmp, db, bus, worktree, store }
}

/// Inserts one assistant message carrying a snapshot bracket.
async fn insert_assistant(
    db: &crate::database::Database,
    session_id: &str,
    id: &str,
    snapshot_start: Option<&str>,
    files: &[&str],
) {
    let session = session_id.to_string();
    let id = id.to_string();
    let payload = serde_json::json!({
        "time": { "created": 1, "streamed": 2, "completed": 3 },
        "snapshot": {
            "start": snapshot_start,
            "end": snapshot_start,
            "files": files,
        }
    });
    db.transaction(move |tx| {
        tx.conn().execute(
            "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) \
             VALUES (?1, ?2, 'assistant', (SELECT COALESCE(MAX(seq), 0) + 1 FROM session_message WHERE session_id = ?2), 0, 0, ?3)",
            rusqlite::params![id, session, payload.to_string()],
        )?;
        Ok(())
    })
    .await
    .expect("insert assistant");
}

/// RD-12: a missing boundary message is a `MessageNotFoundError` (ref
/// revert.ts:91), never a silent empty plan / staged event.
#[tokio::test]
async fn snapshot_revert_stage_missing_boundary_errors() {
    let stack = revert_stack().await;
    let session_id = SessionId("ses_revert_missing".into());
    seed_session(&stack.db, &session_id).await;
    let missing = MessageId::parse("msg_nope").expect("missing id");
    let error = crate::session::revert::stage(
        &stack.db,
        &stack.bus,
        stack.store.as_ref(),
        &session_id,
        &missing,
        true,
    )
    .await
    .expect_err("missing boundary must error");
    assert_eq!(
        error,
        crate::session::revert::RevertError::MessageNotFound {
            session_id: session_id.as_str().to_string(),
            message_id: missing.as_str().to_string(),
        }
    );
    // Nothing staged: no session.revert.staged event, no revert state.
    assert!(crate::session::revert::read_revert(&stack.db, &session_id)
        .await
        .expect("read")
        .is_none());
    // The valid-id path is unchanged (see the flow test below).
}

#[tokio::test]
async fn snapshot_revert_stage_clear_commit_flow() {
    let stack = revert_stack().await;
    let session_id = SessionId("ses_revert_1".into());
    seed_session(&stack.db, &session_id).await;

    // Pre-turn state, then an assistant turn that edits the file.
    std::fs::write(stack.worktree.join("first.txt"), "first\n").unwrap();
    let pre_edit = stack.store.capture().await.expect("pre-edit tree");
    let boundary = MessageId::parse("msg_boundary").expect("boundary id");
    insert_assistant(&stack.db, session_id.as_str(), "msg_boundary", None, &[]).await;
    std::fs::write(stack.worktree.join("first.txt"), "first edited\n").unwrap();
    insert_assistant(
        &stack.db,
        session_id.as_str(),
        "msg_after",
        Some(pre_edit.as_str()),
        &["first.txt"],
    )
    .await;

    // stage: the planned files restore to their per-message START trees.
    let revert = crate::session::revert::stage(
        &stack.db,
        &stack.bus,
        stack.store.as_ref(),
        &session_id,
        &boundary,
        true,
    )
    .await
    .expect("stage");
    assert_eq!(revert.message_id, boundary);
    let staged_tree = revert.snapshot.clone().expect("stage-time snapshot");
    assert_eq!(
        std::fs::read_to_string(stack.worktree.join("first.txt")).unwrap(),
        "first\n",
        "planned files restored to their snapshot trees"
    );
    let files = revert.files.clone().expect("files");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file, "first.txt");
    assert!(
        files[0].patch.contains("-first edited\n+first"),
        "patch shows the revert: {}",
        files[0].patch
    );

    // The session row carries the staged revert.
    let stored = crate::session::revert::read_revert(&stack.db, &session_id)
        .await
        .expect("read")
        .expect("staged");
    assert_eq!(stored.message_id, boundary);
    assert_eq!(stored.snapshot.as_ref(), Some(&staged_tree));

    // clear: the staged (post-edit) state is restored again.
    crate::session::revert::clear(&stack.db, &stack.bus, stack.store.as_ref(), &session_id)
        .await
        .expect("clear");
    assert_eq!(
        std::fs::read_to_string(stack.worktree.join("first.txt")).unwrap(),
        "first edited\n",
        "clear restores the staged tree"
    );
    assert!(crate::session::revert::read_revert(&stack.db, &session_id)
        .await
        .expect("read")
        .is_none());

    // Re-stage then commit: publishes session.revert.committed with the
    // boundary id; the projector deletes messages at/after the boundary.
    crate::session::revert::stage(
        &stack.db,
        &stack.bus,
        stack.store.as_ref(),
        &session_id,
        &boundary,
        true,
    )
    .await
    .expect("stage 2");
    let state = crate::session::revert::read_revert(&stack.db, &session_id)
        .await
        .expect("read")
        .expect("staged");
    crate::session::revert::commit(&stack.bus, &session_id, &state).await.expect("commit");
    assert!(crate::session::revert::read_revert(&stack.db, &session_id)
        .await
        .expect("read")
        .is_none());
    let boundary_exists: bool = stack
        .db
        .transaction({
            let session = session_id.as_str().to_string();
            move |tx| {
                let count: i64 = tx.conn().query_row(
                    "SELECT COUNT(*) FROM session_message WHERE session_id = ?1 AND id = 'msg_after'",
                    rusqlite::params![session],
                    |row| row.get(0),
                )?;
                Ok(count > 0)
            }
        })
        .await
        .expect("count");
    assert!(!boundary_exists, "messages after the boundary are deleted on commit");
}

async fn seed_session(db: &crate::database::Database, session_id: &SessionId) {
    let id = session_id.as_str().to_string();
    db.transaction(move |tx| {
        // session_v2.project_id is a foreign key into the project table.
        tx.conn().execute(
            "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) \
             VALUES ('global', '', 0, 0, '')",
            [],
        )?;
        tx.conn().execute(
            "INSERT INTO session_v2 (id, project_id, slug, directory, path, version, cost, \
             tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, \
             time_created, time_updated, resume_attempts) \
             VALUES (?1, 'global', 'revert-test', '/repo', NULL, 'test', 0, 0, 0, 0, 0, 0, 0, 0, 0)",
            rusqlite::params![id],
        )?;
        Ok(())
    })
    .await
    .expect("seed session");
}
