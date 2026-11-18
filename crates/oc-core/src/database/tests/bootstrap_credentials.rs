//! W13 — G-445 root-cause family, concern②: the fresh-install bootstrap
//! path used to pre-write the ENTIRE migration journal (including
//! `20260805200742_import_legacy_credentials`) without executing the one
//! migration whose effect lives outside the database — silently dropping
//! a user's existing v2 `auth.json` credentials. The bootstrap now runs
//! the import body inside its transaction, so the journal markers only
//! cover effects that actually happened.

use super::super::migrations;
use super::super::MIGRATIONS;
use rusqlite::Connection;

fn options(dir: &tempfile::TempDir) -> migrations::MigrationOptions {
    migrations::MigrationOptions { global_data: Some(dir.path().to_path_buf()) }
}

/// Fresh file database alongside the (fake) `Global.data` root.
fn fresh_file_db(dir: &tempfile::TempDir) -> Connection {
    Connection::open(dir.path().join("fresh.db")).expect("open db")
}

fn credential_rows(conn: &Connection) -> Vec<(String, String)> {
    let mut statement = conn
        .prepare("SELECT integration_id, label FROM credential ORDER BY integration_id")
        .expect("prepare credential");
    statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .expect("query credential")
        .collect::<Result<Vec<_>, _>>()
        .expect("credential rows")
}

/// Fresh install + an existing v2 `auth.json` with an OAuth entry and an
/// API-key entry: the bootstrap imports BOTH into `credential` and still
/// writes the complete journal (every marker now backed by an effect that
/// actually happened — the SQL via the consolidated bootstrap schema, the
/// import via its executed body).
#[test]
fn bootstrap_imports_existing_auth_json_credentials() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    std::fs::write(
        dir.path().join("auth.json"),
        serde_json::json!({
            "github-copilot": {
                "type": "oauth",
                "refresh": "r-token",
                "access": "a-token",
                "expires": 1_893_456_000_000u64
            },
            "anthropic": {
                "type": "api",
                "key": "sk-ant-legacy"
            }
        })
        .to_string(),
    )
    .expect("write auth.json");
    let mut conn = fresh_file_db(&dir);
    migrations::apply(&mut conn, &options(&dir)).expect("bootstrap");

    let rows = credential_rows(&conn);
    assert_eq!(rows.len(), 2, "both legacy credentials imported: {rows:?}");
    assert!(rows.contains(&("anthropic".to_owned(), "API key".to_owned())), "{rows:?}");
    assert!(rows.contains(&("github-copilot".to_owned(), "OAuth".to_owned())), "{rows:?}");
    // The value column decodes with the v2 camelCase shape (G-445's serde
    // fix is what makes these rows readable by the credential store).
    let value: String = conn
        .query_row("SELECT value FROM credential WHERE integration_id = 'anthropic'", [], |row| {
            row.get(0)
        })
        .expect("select value");
    let decoded: serde_json::Value = serde_json::from_str(&value).expect("decode value");
    assert_eq!(decoded["type"], "key");
    assert_eq!(decoded["key"], "sk-ant-legacy");

    // The journal is complete and the import marker is present exactly once.
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM migration", [], |row| row.get(0))
        .expect("count journal");
    assert_eq!(count, i64::try_from(MIGRATIONS.len()).unwrap());
    let import_marker: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM migration WHERE id = '20260805200742_import_legacy_credentials'",
            [],
            |row| row.get(0),
        )
        .expect("count import marker");
    assert_eq!(import_marker, 1);
}

/// Idempotence: re-running `apply` over a bootstrapped database takes the
/// `apply_only` path, sees the complete journal, and does NOT re-import
/// (the integration-exists guard + the completed marker).
#[test]
fn bootstrap_import_is_idempotent_on_reapply() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    std::fs::write(
        dir.path().join("auth.json"),
        serde_json::json!({ "anthropic": { "type": "api", "key": "sk-ant-legacy" } }).to_string(),
    )
    .expect("write auth.json");
    let mut conn = fresh_file_db(&dir);
    migrations::apply(&mut conn, &options(&dir)).expect("first apply");
    let report = migrations::apply(&mut conn, &options(&dir)).expect("second apply");
    assert!(report.applied.is_empty(), "no migration replays: {:?}", report.applied);
    assert_eq!(credential_rows(&conn).len(), 1, "no duplicate import");
}

/// The no-`auth.json` fresh install is unchanged: full journal, empty
/// credential table.
#[test]
fn bootstrap_without_auth_json_imports_nothing() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let mut conn = fresh_file_db(&dir);
    migrations::apply(&mut conn, &options(&dir)).expect("bootstrap");
    assert_eq!(credential_rows(&conn), Vec::<(String, String)>::new());
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM migration", [], |row| row.get(0))
        .expect("count journal");
    assert_eq!(count, i64::try_from(MIGRATIONS.len()).unwrap());
}

/// W14 backfill — the lying-journal shape: a database bootstrapped by the
/// PRE-fix code (complete journal, import marker present, but the
/// credential rows never landed). `apply_only` must detect the lie and
/// re-run the idempotent import; a second open must not duplicate rows.
#[test]
fn backfill_heals_lying_journal_exactly_once() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    std::fs::write(
        dir.path().join("auth.json"),
        serde_json::json!({
            "anthropic": { "type": "api", "key": "sk-ant-lying" }
        })
        .to_string(),
    )
    .expect("write auth.json");
    let mut conn = fresh_file_db(&dir);
    // Forge the pre-fix shape: full bootstrap + journal WITHOUT the import
    // body (exactly what the old `apply` wrote).
    conn.execute_batch(super::super::bootstrap::BOOTSTRAP_SQL).expect("bootstrap sql");
    conn.execute_batch(super::super::bootstrap::MIGRATION_TABLE_SQL).expect("journal table");
    for id in super::super::bootstrap::MIGRATION_IDS {
        conn.execute(
            "INSERT INTO migration (id, time_completed) VALUES (?1, 0)",
            rusqlite::params![id],
        )
        .expect("insert marker");
    }
    assert!(credential_rows(&conn).is_empty(), "the lying journal dropped the credential");

    // apply_only sees the complete journal and backfills the import.
    migrations::apply_only(&mut conn, &options(&dir)).expect("apply_only");
    let rows = credential_rows(&conn);
    assert_eq!(rows, vec![("anthropic".to_owned(), "API key".to_owned())], "backfilled");

    // Idempotent: a second open changes nothing.
    migrations::apply_only(&mut conn, &options(&dir)).expect("apply_only again");
    assert_eq!(credential_rows(&conn).len(), 1, "no duplicate import");
}

/// The backfill must be a no-op when there is no legacy `auth.json` to
/// read (the import's own ENOENT arm) — a lying journal alone must not
/// fail the boot.
#[test]
fn backfill_is_noop_without_auth_json() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let mut conn = fresh_file_db(&dir);
    conn.execute_batch(super::super::bootstrap::BOOTSTRAP_SQL).expect("bootstrap sql");
    conn.execute_batch(super::super::bootstrap::MIGRATION_TABLE_SQL).expect("journal table");
    for id in super::super::bootstrap::MIGRATION_IDS {
        conn.execute(
            "INSERT INTO migration (id, time_completed) VALUES (?1, 0)",
            rusqlite::params![id],
        )
        .expect("insert marker");
    }
    migrations::apply_only(&mut conn, &options(&dir)).expect("apply_only");
    assert!(credential_rows(&conn).is_empty(), "nothing to import, nothing added");
}
