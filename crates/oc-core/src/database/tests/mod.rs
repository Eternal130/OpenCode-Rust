//! Unit tests for the database layer (`src/database/`).
//!
//! bun-dependent acceptance tests live in [`super::bun`].

mod bun;

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use super::error::DbError;
use super::migrations::MIGRATIONS;
use super::schema::{self, EventRow, EventSequenceRow, KvRow, ProjectRow, SessionRow};
use super::{Begin, Database, Tx};

fn tmp_dir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("oc-core-db-{tag}-"))
        .tempdir_in("/tmp/opencode")
        .expect("create temp dir under /tmp/opencode")
}

async fn open(path: &Path) -> Database {
    Database::open_file(path).await.expect("open database")
}

#[tokio::test]
async fn database_bootstraps_fresh_file_with_journal() {
    let dir = tmp_dir("bootstrap");
    let db_path = dir.path().join("opencode.db");
    let db = open(&db_path).await;

    let report = db.apply_report().expect("report present");
    assert!(report.bootstrapped);
    assert!(!report.seeded_legacy);
    assert_eq!(report.applied, Vec::<&str>::new());

    let ids = db.migration_ids().await.unwrap();
    let expected = MIGRATIONS.iter().map(|m| m.id.to_string()).collect::<Vec<_>>();
    assert_eq!(ids, expected, "journal holds all 48 ids in order");

    // Reopening changes nothing.
    drop(db);
    let db = open(&db_path).await;
    let report = db.apply_report().expect("report present");
    assert!(!report.bootstrapped);
    assert!(report.applied.is_empty(), "no migrations re-run: {:?}", report.applied);
    assert_eq!(db.migration_ids().await.unwrap().len(), 48);

    // journal table shape
    db.with_conn(|conn| {
        let sql: String = conn
            .query_row("SELECT sql FROM sqlite_master WHERE name = 'migration'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            sql,
            "CREATE TABLE \"migration\" (id TEXT PRIMARY KEY, time_completed INTEGER NOT NULL)"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn database_pragmas_applied() {
    let dir = tmp_dir("pragmas");
    let db = open(&dir.path().join("x.db")).await;
    db.with_conn(|conn| {
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        let synchronous: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
        assert_eq!(synchronous, 1, "NORMAL");
        let busy: i64 = conn.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).unwrap();
        assert_eq!(busy, 5000);
        let cache: i64 = conn.query_row("PRAGMA cache_size", [], |r| r.get(0)).unwrap();
        assert_eq!(cache, -64000);
        let fk: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1, "foreign_keys ON");
        Ok(())
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn database_file_modes_are_0600() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tmp_dir("modes");
    let db_path = dir.path().join("fresh.db");
    assert!(!db_path.exists());
    let db = open(&db_path).await;

    let mode = |path: &Path| path.metadata().unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&db_path), 0o600, "db created 0600");

    // A WAL write materializes sidecars; they must inherit 0600 while open.
    db.kv_set("k", &serde_json::json!(1)).await.unwrap();
    db.with_conn(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    })
    .await
    .unwrap();
    for suffix in ["-wal", "-shm"] {
        let sidecar = dir.path().join(format!("fresh.db{suffix}"));
        if sidecar.exists() {
            assert_eq!(mode(&sidecar), 0o600, "{} must be 0600", sidecar.display());
        }
    }

    // Pre-existing loose modes are tightened on open.
    drop(db);
    let loose = dir.path().join("loose.db");
    std::fs::write(&loose, b"").unwrap();
    std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).unwrap();
    let _db = open(&loose).await;
    assert_eq!(mode(&loose), 0o600);
}

#[tokio::test]
async fn database_memory_is_private_and_bootstrapped() {
    let a = Database::open_in_memory().await.unwrap();
    let b = Database::open_in_memory().await.unwrap();
    assert!(a.apply_report().unwrap().bootstrapped);
    assert_eq!(a.migration_ids().await.unwrap().len(), 48);

    a.kv_set("only-a", &serde_json::json!(true)).await.unwrap();
    assert!(b.kv_get("only-a").await.unwrap().is_none(), "memory dbs are independent");

    a.with_conn(|conn| {
        let tables: i64 = conn
            .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 20, "19 snapshot tables + migration");
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn database_rejects_foreign_tables_without_session() {
    let dir = tmp_dir("foreign");
    let path = dir.path().join("x.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE somebody_elses_table (id INTEGER)").unwrap();
    }
    let err = match Database::open_file(&path).await {
        Ok(_db) => panic!("expected open failure, got success"),
        Err(err) => err,
    };
    assert!(matches!(err, DbError::NotEmptyNoSession), "expected NotEmptyNoSession, got {err:?}");
    assert_eq!(err.to_string(), "Database is not empty and has no session table");
}

fn legacy_fixture(path: &Path, make_drizzle: fn(&Connection)) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE session_v2 (id TEXT PRIMARY KEY)").unwrap();
    make_drizzle(&conn);
}

#[tokio::test]
async fn database_legacy_seeding_name_variant() {
    let dir = tmp_dir("legacy-name");
    let path = dir.path().join("x.db");
    legacy_fixture(&path, |conn| {
        conn.execute_batch(
            "CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash TEXT, name TEXT)",
        )
        .unwrap();
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            conn.execute(
                "INSERT INTO __drizzle_migrations (id, hash, name) VALUES (?1, 'h', ?2)",
                rusqlite::params![i64::try_from(index).unwrap(), migration.id],
            )
            .unwrap();
        }
    });

    let db = Database::open_file(&path).await.unwrap();
    let report = db.apply_report().unwrap();
    assert!(!report.bootstrapped);
    assert!(report.seeded_legacy, "name-column variant seeded");
    assert!(report.applied.is_empty(), "all known names skipped: {:?}", report.applied);
    assert_eq!(db.migration_ids().await.unwrap().len(), 48);
}

#[tokio::test]
async fn database_legacy_seeding_timestamp_variant() {
    let dir = tmp_dir("legacy-ts");
    let path = dir.path().join("x.db");
    legacy_fixture(&path, |conn| {
        conn.execute_batch(
            "CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash TEXT, created_at INTEGER)",
        )
        .unwrap();
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            // id = YYYYMMDDHHMMSS_slug → created_at ms at that instant.
            let stamp = &migration.id[..14];
            let iso = format!(
                "{}-{}-{} {}:{}:{}",
                &stamp[0..4],
                &stamp[4..6],
                &stamp[6..8],
                &stamp[8..10],
                &stamp[10..12],
                &stamp[12..14]
            );
            conn.execute(
                "INSERT INTO __drizzle_migrations (id, hash, created_at)
                 VALUES (?1, 'h', CAST(strftime('%s', ?2) AS INTEGER) * 1000)",
                rusqlite::params![i64::try_from(index).unwrap(), iso],
            )
            .unwrap();
        }
    });

    let db = Database::open_file(&path).await.unwrap();
    let report = db.apply_report().unwrap();
    assert!(report.seeded_legacy, "created_at variant seeded");
    assert!(report.applied.is_empty(), "prefix-matched everything: {:?}", report.applied);
    assert_eq!(db.migration_ids().await.unwrap().len(), 48);
}

#[tokio::test]
async fn database_legacy_seeding_unmatched_timestamp_dies() {
    let dir = tmp_dir("legacy-bad");
    let path = dir.path().join("x.db");
    legacy_fixture(&path, |conn| {
        conn.execute_batch(
            "CREATE TABLE __drizzle_migrations (id INTEGER PRIMARY KEY, hash TEXT, created_at INTEGER)",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO __drizzle_migrations (id, hash, created_at) VALUES (1, 'h', 1577836800000)", // 2020-01-01
            [],
        )
        .unwrap();
    });

    let err = match Database::open_file(&path).await {
        Ok(_db) => panic!("expected open failure"),
        Err(err) => err,
    };
    assert!(
        matches!(err, DbError::LegacyTimestampUnmatched(1_577_836_800_000)),
        "expected unmatched legacy timestamp, got {err:?}"
    );
}

#[tokio::test]
async fn database_transaction_rolls_back_on_commit_failure() {
    let dir = tmp_dir("commit-fail");
    let db = open(&dir.path().join("x.db")).await;

    // Deferred FK violation: COMMIT fails and SQLite keeps the tx open.
    // (Tables are created up-front so the rollback under test only discards
    // the inserted row.)
    db.transaction_deferred(|tx| {
        tx.execute_batch(
            "CREATE TABLE cf_parent (id INTEGER PRIMARY KEY);
             CREATE TABLE cf_child (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES cf_parent(id));",
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let result = db
        .transaction_deferred(|tx| {
            tx.execute_batch("PRAGMA defer_foreign_keys = ON")?;
            tx.execute("INSERT INTO cf_child (id, parent_id) VALUES (1, 99)", rusqlite::params![])?;
            Ok(())
        })
        .await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("FOREIGN KEY"), "expected FK constraint on commit, got {err}");

    // The explicit ROLLBACK must have restored autocommit…
    db.with_conn(|conn| {
        assert!(conn.is_autocommit(), "connection left inside a transaction");
        Ok(())
    })
    .await
    .unwrap();
    // …and the failed write must be gone.
    db.with_conn(|conn| {
        let count: i64 = conn.query_row("SELECT count(*) FROM cf_child", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
        Ok(())
    })
    .await
    .unwrap();

    // And a follow-up transaction succeeds.
    db.kv_set("after", &serde_json::json!("ok")).await.unwrap();
    assert_eq!(db.kv_get("after").await.unwrap().unwrap(), serde_json::json!("ok"));
}

#[tokio::test]
async fn database_nested_savepoints_commit_and_rollback() {
    let dir = tmp_dir("savepoints");
    let db = open(&dir.path().join("x.db")).await;

    db.transaction(|tx| {
        tx.kv_set("outer", &serde_json::json!(1))?;
        assert_eq!(tx.depth(), 0);

        // Inner rollback discards only the savepoint scope.
        let inner: super::Result<()> = tx.transaction(|inner| {
            assert_eq!(inner.depth(), 1);
            inner.kv_set("inner-rollback", &serde_json::json!(1))?;
            Err(DbError::NotAbsolute("boom".into()))
        });
        assert!(inner.is_err());

        tx.transaction(|inner| {
            inner.kv_set("inner-commit", &serde_json::json!(2))?;
            // Third level nesting.
            inner.transaction(|innermost| {
                assert_eq!(innermost.depth(), 2);
                innermost.kv_set("innermost", &serde_json::json!(3))
            })?;
            Ok(())
        })?;
        Ok(())
    })
    .await
    .unwrap();

    assert_eq!(db.kv_get("outer").await.unwrap().unwrap(), serde_json::json!(1));
    assert!(db.kv_get("inner-rollback").await.unwrap().is_none(), "savepoint rolled back");
    assert_eq!(db.kv_get("inner-commit").await.unwrap().unwrap(), serde_json::json!(2));
    assert_eq!(db.kv_get("innermost").await.unwrap().unwrap(), serde_json::json!(3));
}

#[tokio::test]
async fn database_busy_timeout_waits_for_writer() {
    let dir = tmp_dir("busy");
    let path = dir.path().join("x.db");
    let holder = open(&path).await;
    let waiter = open(&path).await;

    let hold = Duration::from_millis(800);
    let holder_task = tokio::spawn(async move {
        holder
            .transaction(move |tx| {
                tx.kv_set("hold", &serde_json::json!(1))?;
                std::thread::sleep(hold);
                Ok(())
            })
            .await
    });
    // Let the holder acquire the write lock first.
    tokio::time::sleep(Duration::from_millis(250)).await;

    let started = Instant::now();
    waiter.kv_set("waiter", &serde_json::json!(2)).await.unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed + Duration::from_millis(250) >= hold,
        "second writer should have blocked ~{hold:?}, got {elapsed:?}"
    );
    assert_eq!(waiter.kv_get("hold").await.unwrap().unwrap(), serde_json::json!(1));
    holder_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn database_schema_crud_roundtrip() {
    let dir = tmp_dir("crud");
    let db = open(&dir.path().join("x.db")).await;

    db.transaction(|tx| {
        let project = ProjectRow::new("proj_1", "/tmp/opencode/project", Some("git"))?;
        ProjectRow::insert(tx, &project)?;
        let fetched = ProjectRow::get(tx, "proj_1".to_string())?.expect("project row");
        assert_eq!(fetched, project);
        assert_eq!(fetched.sandboxes_json()?, serde_json::Value::Array(vec![]));

        let mut session =
            SessionRow::new("ses_1", "proj_1", "/tmp/opencode/project", "s1", "2.0.24")?;
        session.cost = 1.5;
        session.tokens_input = 12;
        SessionRow::insert(tx, &session)?;
        session.cost = 2.5;
        SessionRow::update(tx, &session)?;
        assert_eq!(SessionRow::get(tx, "ses_1".to_string())?.unwrap().cost, 2.5);

        schema::SessionMessageRow::insert(
            tx,
            &schema::SessionMessageRow {
                id: "msg_1".into(),
                session_id: "ses_1".into(),
                type_: "user".into(),
                seq: 1,
                time_created: 1,
                time_updated: 1,
                data: serde_json::json!({"text": "hi"}).to_string(),
            },
        )?;
        schema::SessionMessageRow::insert(
            tx,
            &schema::SessionMessageRow {
                id: "msg_2".into(),
                session_id: "ses_1".into(),
                type_: "assistant".into(),
                seq: 2,
                time_created: 2,
                time_updated: 2,
                data: serde_json::json!({"text": "hello"}).to_string(),
            },
        )?;
        let messages = tx.session_messages("ses_1", Some(0), 10)?;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].id, "msg_1");
        assert_eq!(messages[0].type_, "user");

        // Event log + sequence.
        tx.event_sequence_upsert("ses_1", 2, Some("owner-1"))?;
        tx.event_sequence_upsert("ses_1", 1, None)?; // monotonic, keeps owner
        let seq = EventSequenceRow::get(tx, "ses_1".to_string())?.unwrap();
        assert_eq!((seq.seq, seq.owner_id.as_deref()), (2, Some("owner-1")));

        EventRow::insert(
            tx,
            &EventRow {
                id: "evt_1".into(),
                aggregate_id: "ses_1".into(),
                seq: 1,
                created: 0,
                type_: "session.created.1".into(),
                data: serde_json::json!({"id": "ses_1"}).to_string(),
            },
        )?;
        let events = tx.events_page("ses_1", None, 512)?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data_json()?["id"], "ses_1");

        // Permission insert-or-ignore dedupe.
        let permission = schema::PermissionRow {
            id: "per_1".into(),
            project_id: "proj_1".into(),
            action: "bash".into(),
            resource: "rm -rf *".into(),
            time_created: 1,
            time_updated: 1,
        };
        schema::PermissionRow::insert_or_ignore(tx, &permission)?;
        schema::PermissionRow::insert_or_ignore(tx, &permission)?;
        assert_eq!(tx.permissions_for_project("proj_1")?.len(), 1);

        // kv roundtrip.
        tx.kv_set("theme", &serde_json::json!({"dark": true}))?;
        assert_eq!(tx.kv_get("theme")?.unwrap(), serde_json::json!({"dark": true}));

        // MigrationRow CRUD reflects the journal.
        assert_eq!(schema::MigrationRow::count(tx)?, 48);

        // Delete cascades: project removal removes the session.
        ProjectRow::delete(tx, "proj_1".to_string())?;
        assert!(SessionRow::get(tx, "ses_1".to_string())?.is_none());
        assert_eq!(
            KvRow::list(tx)?.iter().filter(|row| row.key == "theme").count(),
            1,
            "kv survives project delete"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn database_begin_modes_toggle() {
    let dir = tmp_dir("begin");
    let db = open(&dir.path().join("x.db")).await;
    db.transaction_with(Begin::Immediate, |tx| tx.kv_set("a", &serde_json::json!(1)))
        .await
        .unwrap();
    db.transaction_with(Begin::Deferred, |tx| tx.kv_set("b", &serde_json::json!(2))).await.unwrap();
    assert!(db.kv_get("a").await.unwrap().is_some());
    assert!(db.kv_get("b").await.unwrap().is_some());
}

#[test]
fn database_savepoint_names_use_oc_core_prefix() {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    {
        let mut tx = Tx { conn: &mut conn, depth: 0 };
        tx.execute_batch("SAVEPOINT oc_core_1").unwrap();
        tx.execute_batch("ROLLBACK TO SAVEPOINT oc_core_1").unwrap();
        tx.execute_batch("RELEASE SAVEPOINT oc_core_1").unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
}
