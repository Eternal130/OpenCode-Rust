//! G-06 battery for the v1→v2 content migration runner
//! (`v1_migration.rs`, port of `v1-migration.bun.ts`): fixture v1 sqlite
//! rows → v2 rows + completed status; no-legacy → not run; a failing db
//! surfaces `error` while the database stays usable; double-run is
//! idempotent (the KV gate).

use std::path::Path;

use super::super::v1_migration::{self, RunOptions, Status};
use super::super::Database;

/// `RUNTIME` is process-global; the battery's assertions on `status()`
/// require the three scenarios to run one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn serialized() -> tokio::sync::MutexGuard<'static, ()> {
    SERIAL.lock().await
}

fn tmp_dir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("oc-core-v1mig-{tag}-"))
        .tempdir_in("/tmp/opencode")
        .expect("create temp dir under /tmp/opencode")
}

async fn open(path: &Path) -> Database {
    Database::open_file(path).await.expect("open database")
}

/// Creates the legacy v1 `session`/`message`/`part` tables (the v1
/// schema the ref's copy logic SELECTs from) on an already-migrated v2
/// database, plus one user/assistant exchange.
fn seed_legacy(conn: &rusqlite::Connection) {
    conn.execute_batch(
        "CREATE TABLE session (\
           id TEXT PRIMARY KEY, project_id TEXT, workspace_id TEXT, parent_id TEXT, \
           slug TEXT, directory TEXT, path TEXT, title TEXT, version TEXT, share_url TEXT, \
           summary_additions INTEGER, summary_deletions INTEGER, summary_files INTEGER, \
           summary_diffs TEXT, metadata TEXT, cost REAL, tokens_input INTEGER, \
           tokens_output INTEGER, tokens_reasoning INTEGER, tokens_cache_read INTEGER, \
           tokens_cache_write INTEGER, revert TEXT, agent TEXT, model TEXT, \
           time_created INTEGER, time_updated INTEGER, time_compacting INTEGER, time_archived INTEGER\
         ); \
         CREATE TABLE message (\
           id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT\
         ); \
         CREATE TABLE part (\
           id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT\
         );",
    )
    .expect("create legacy tables");
    conn.execute(
        "INSERT INTO session (id, project_id, slug, directory, version, cost, tokens_input, \
         tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated) \
         VALUES ('ses_legacy_1', 'missing-project', 's1', '/tmp/opencode/project', '2.0.0', \
         0, 0, 0, 0, 0, 0, 1000, 1001)",
        [],
    )
    .expect("insert legacy session");
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES \
         ('msg_u1', 'ses_legacy_1', 1002, 1002, ?1)",
        rusqlite::params![serde_json::json!({
            "role": "user",
            "time": { "created": 1002 },
            "agent": "build",
            "model": { "providerID": "anthropic", "modelID": "claude-sonnet-4-5" }
        })
        .to_string()],
    )
    .expect("insert user message");
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES \
         ('par_u1', 'msg_u1', 'ses_legacy_1', 1002, 1002, ?1)",
        rusqlite::params![serde_json::json!({
            "type": "text",
            "text": "hello world"
        })
        .to_string()],
    )
    .expect("insert text part");
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES \
         ('msg_a1', 'ses_legacy_1', 1003, 1004, ?1)",
        rusqlite::params![serde_json::json!({
            "role": "assistant",
            "time": { "created": 1003, "completed": 1004 },
            "parentID": "msg_u1",
            "modelID": "claude-sonnet-4-5",
            "providerID": "anthropic",
            "agent": "build",
            "cost": 0.01,
            "tokens": {
                "input": 10, "output": 5, "reasoning": 0,
                "cache": { "read": 2, "write": 1 }
            },
            "finish": "stop"
        })
        .to_string()],
    )
    .expect("insert assistant message");
    conn.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES \
         ('par_a1', 'msg_a1', 'ses_legacy_1', 1003, 1004, ?1)",
        rusqlite::params![serde_json::json!({
            "type": "text",
            "text": "hi there"
        })
        .to_string()],
    )
    .expect("insert assistant text part");
}

fn options(data_root: &Path) -> RunOptions {
    RunOptions {
        next_database_path: None,
        data_root: Some(data_root.to_path_buf()),
        in_memory: false,
    }
}

async fn count(db: &Database, sql: &'static str) -> i64 {
    db.with_conn(|conn| Ok(conn.query_row(sql, [], |row| row.get(0))?)).await.expect("count")
}

#[tokio::test]
async fn v1_migration_copies_fixture_rows_and_reports_completed() {
    let _serial = serialized().await;
    v1_migration::reset_runtime_for_test();
    let dir = tmp_dir("fixture");
    let db = open(&dir.path().join("x.db")).await;
    db.with_conn(|conn| {
        seed_legacy(conn);
        Ok(())
    })
    .await
    .unwrap();

    // Before the run: the legacy table presence makes it `required`.
    assert!(matches!(v1_migration::status(&db).await, Status::Required));

    let result = v1_migration::run(&db, options(dir.path())).await;
    assert_eq!(result.expect("migration run"), "completed");

    // The session landed in session_v2 (reassigned to the global project
    // — its v1 project_id does not exist), the exchange projected into
    // session_message, and the event watermark written.
    assert_eq!(count(&db, "SELECT COUNT(*) FROM session_v2 WHERE id = 'ses_legacy_1'").await, 1);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM session_v2 WHERE project_id = 'global'").await, 1);
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM session_message WHERE session_id = 'ses_legacy_1'").await,
        2
    );
    let user_data: String = db
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT data FROM session_message WHERE id = 'msg_u1'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert!(user_data.contains("hello world"), "user text projected: {user_data}");
    let seq: i64 = db
        .with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT seq FROM event_sequence WHERE aggregate_id = 'ses_legacy_1'",
                [],
                |row| row.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(seq, 1, "watermark = last projected seq");

    // KV checkpoint + status.
    let kv: Option<String> = db
        .with_conn(|conn| {
            Ok(conn
                .query_row("SELECT value FROM kv WHERE key = 'migration.v1-v2'", [], |row| {
                    row.get(0)
                })
                .map(Some)
                .or_else(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    error => Err(error),
                })?)
        })
        .await
        .unwrap();
    assert_eq!(kv.as_deref(), Some(r#"{"phase":"completed"}"#));
    assert!(matches!(v1_migration::status(&db).await, Status::Completed));

    // Idempotency / double-boot: the KV gate short-circuits the second
    // run and nothing duplicates.
    let again = v1_migration::run(&db, options(dir.path())).await;
    assert_eq!(again.expect("second run"), "completed");
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM session_message WHERE session_id = 'ses_legacy_1'").await,
        2
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM session_v2 WHERE id = 'ses_legacy_1'").await, 1);
}

#[tokio::test]
async fn v1_migration_without_legacy_sessions_never_runs() {
    let _serial = serialized().await;
    v1_migration::reset_runtime_for_test();
    let dir = tmp_dir("no-legacy");
    let db = open(&dir.path().join("x.db")).await;

    assert!(matches!(v1_migration::status(&db).await, Status::Completed));
    assert_eq!(v1_migration::run(&db, options(dir.path())).await.expect("run"), "completed");

    // No KV state was written (run exited before the checkpoint).
    let kv: i64 = count(&db, "SELECT COUNT(*) FROM kv WHERE key = 'migration.v1-v2'").await;
    assert_eq!(kv, 0);
    assert!(matches!(v1_migration::status(&db).await, Status::Completed));
}

#[tokio::test]
async fn v1_migration_failure_reports_error_and_keeps_database_alive() {
    let _serial = serialized().await;
    v1_migration::reset_runtime_for_test();
    let dir = tmp_dir("failing");
    let db = open(&dir.path().join("x.db")).await;
    // A v1 `session` table missing a column the copy SELECT references —
    // the INSERT..SELECT inside the walk fails.
    db.with_conn(|conn| {
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, time_created INTEGER, time_updated INTEGER); \
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT); \
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT); \
             INSERT INTO session (id, project_id, time_created, time_updated) VALUES ('ses_bad', 'p', 1, 2);",
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let result = v1_migration::run(&db, options(dir.path())).await;
    assert!(result.is_err(), "failing db must surface the error");

    // status → error (the server stays up: the database still works).
    let Status::Error(message) = v1_migration::status(&db).await else {
        panic!("expected error status");
    };
    assert_ne!(message, "");
    db.kv_set("alive", &serde_json::json!(true)).await.expect("database usable after failure");
}
