//! NULL-tolerance and physical-column-order tests for the row mappers
//! (`schema.rs` `table!` macro), pinned against the census of the user's
//! REAL migrated v2 database:
//!
//! - every census column (drizzle-nullable OR observed NULL in practice)
//!   decodes `NULL → None` without panicking;
//! - physical column order does not matter: a database created by an
//!   older v2 and altered forward keeps the added columns at the END
//!   (`resume_attempts` … `time_idle`, `time_viewed`, `idle_outcome`
//!   last), while the fresh-DDL order interleaves them — both shapes
//!   must decode identically, because v2's drizzle layer selects by
//!   name and every real migrated database has the alter order;
//! - `event_sequence.owner_id` is NULL for 893/893 real rows and must
//!   not break sequence upserts (the replay/reconciliation path);
//! - writers keep writing NULL (not sentinel strings) for absent values.

use rusqlite::Connection;

use super::super::schema::{
    CredentialRow, EventSequenceRow, ProjectDirectoryRow, ProjectRow, SessionRow, WorktreeRow,
};
use super::super::{run_top_tx, Begin};

/// The `session_v2` physical column order of a REAL migrated v2 database
/// (created by an older v2 DDL, then `ALTER TABLE ADD`ed forward — the
/// census database's `PRAGMA table_info` order). The tail columns differ
/// from the fresh-DDL order in `bootstrap.rs`.
const SESSION_V2_MIGRATED_ORDER: &str = "CREATE TABLE session_v2 (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    workspace_id TEXT,
    parent_id TEXT,
    fork_session_id TEXT,
    fork_boundary TEXT,
    slug TEXT NOT NULL,
    directory TEXT NOT NULL,
    path TEXT,
    title TEXT,
    version TEXT NOT NULL,
    share_url TEXT,
    summary_additions INTEGER,
    summary_deletions INTEGER,
    summary_files INTEGER,
    summary_diffs TEXT,
    metadata TEXT,
    cost REAL NOT NULL DEFAULT 0,
    tokens_input INTEGER NOT NULL DEFAULT 0,
    tokens_output INTEGER NOT NULL DEFAULT 0,
    tokens_reasoning INTEGER NOT NULL DEFAULT 0,
    tokens_cache_read INTEGER NOT NULL DEFAULT 0,
    tokens_cache_write INTEGER NOT NULL DEFAULT 0,
    revert TEXT,
    permission TEXT,
    agent TEXT,
    model TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_compacting INTEGER,
    time_archived INTEGER,
    time_suspended INTEGER,
    resume_attempts INTEGER NOT NULL DEFAULT 0,
    time_idle INTEGER,
    time_viewed INTEGER,
    idle_outcome TEXT
)";

/// The `project` physical column order of the real migrated database:
/// `icon_color` precedes the timestamps, and `commands`,
/// `icon_url_override`, `time_active` trail `sandboxes` — unlike the
/// fresh-DDL order.
const PROJECT_MIGRATED_ORDER: &str = "CREATE TABLE project (
    id TEXT PRIMARY KEY,
    worktree TEXT NOT NULL,
    vcs TEXT,
    name TEXT,
    icon_url TEXT,
    icon_color TEXT,
    time_created INTEGER NOT NULL,
    time_updated INTEGER NOT NULL,
    time_initialized INTEGER,
    sandboxes TEXT NOT NULL,
    commands TEXT,
    icon_url_override TEXT,
    time_active INTEGER NOT NULL DEFAULT 0
)";

fn migrated_shape_conn() -> Connection {
    let conn = Connection::open_in_memory().expect("memory connection");
    conn.execute_batch(SESSION_V2_MIGRATED_ORDER).expect("session_v2 (migrated order)");
    conn.execute_batch(PROJECT_MIGRATED_ORDER).expect("project (migrated order)");
    conn.execute_batch(
        "CREATE TABLE credential (
            id TEXT PRIMARY KEY, integration_id TEXT, label TEXT NOT NULL, value TEXT NOT NULL,
            connector_id TEXT, method_id TEXT, active INTEGER,
            time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL)",
    )
    .expect("credential");
    conn.execute_batch(
        "CREATE TABLE project_directory (
            project_id TEXT NOT NULL, directory TEXT NOT NULL, type TEXT, strategy TEXT,
            time_created INTEGER NOT NULL,
            PRIMARY KEY (project_id, directory))",
    )
    .expect("project_directory");
    conn.execute_batch(
        "CREATE TABLE worktree (
            project_id TEXT NOT NULL, directory TEXT NOT NULL, strategy TEXT,
            time_created INTEGER NOT NULL,
            PRIMARY KEY (project_id, directory))",
    )
    .expect("worktree");
    conn.execute_batch(
        "CREATE TABLE event_sequence (aggregate_id TEXT PRIMARY KEY, seq INTEGER NOT NULL, owner_id TEXT)",
    )
    .expect("event_sequence");
    conn
}

/// A `session_v2` row with EVERY census column NULL — the dominant real
/// shape (e.g. `idle_outcome` 803/891 NULL, `agent`/`model` 882/891 NULL).
const SESSION_ALL_CENSUS_NULL: &str = "INSERT INTO session_v2
    (id, project_id, slug, directory, version, cost, tokens_input, tokens_output,
     tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, time_updated,
     resume_attempts)
VALUES
    ('ses_null', 'prj_1', 'slug', '/proj', '2.0.24', 0, 0, 0, 0, 0, 0, 1000, 1000, 0)";

/// A mixed row: some census columns set, others NULL, with DISTINCT
/// non-default values in the order-sensitive tail columns so a positional
/// misread cannot pass unnoticed.
const SESSION_MIXED: &str = "INSERT INTO session_v2
    (id, project_id, workspace_id, parent_id, slug, directory, path, title, version,
     summary_additions, metadata, revert, agent, model,
     time_created, time_updated, time_compacting, time_archived, time_suspended,
     resume_attempts, time_idle, time_viewed, idle_outcome)
VALUES
    ('ses_mix', 'prj_1', NULL, 'ses_null', 'slug2', '/proj', 'sub/dir', 'A title', '2.0.24',
     42, '{\"k\":\"v\"}', NULL, 'build', '{\"id\":\"m\",\"providerID\":\"p\"}',
     2000, 3000, 111, NULL, 222,
     7, 333, 444, 'failed')";

fn select_star(conn: &Connection, table: &str, id: &str) -> Vec<SessionRow> {
    // The macro's decoder is private; each row struct re-exports it as
    // `decode_row` — used here through SessionRow like the store does.
    let mut statement =
        conn.prepare(&format!("SELECT * FROM {table} WHERE id = ?1")).expect("prepare");
    let rows = statement
        .query_map(rusqlite::params![id], SessionRow::decode_row)
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("decode rows");
    rows
}

/// Reads a whole table through the public `Tx::list` accessors (which use
/// the same name-based decoder as `SELECT *` everywhere else).
macro_rules! list_rows {
    ($conn:expr, $row:ident) => {{
        run_top_tx($conn, Begin::Deferred, |tx| $row::list(tx)).expect("list rows")
    }};
}

#[test]
fn session_row_decodes_all_census_null_in_migrated_order() {
    let conn = migrated_shape_conn();
    conn.execute_batch(SESSION_ALL_CENSUS_NULL).expect("seed all-NULL row");

    let rows = select_star(&conn, "session_v2", "ses_null");
    let row = rows.first().expect("one row");
    assert_eq!(row.id, "ses_null");
    // Every census column is NULL → None.
    assert_eq!(row.workspace_id, None);
    assert_eq!(row.parent_id, None);
    assert_eq!(row.fork_session_id, None);
    assert_eq!(row.fork_boundary, None);
    assert_eq!(row.path, None);
    assert_eq!(row.title, None);
    assert_eq!(row.share_url, None);
    assert_eq!(row.summary_additions, None);
    assert_eq!(row.summary_deletions, None);
    assert_eq!(row.summary_files, None);
    assert_eq!(row.summary_diffs, None);
    assert_eq!(row.metadata, None);
    assert_eq!(row.revert, None);
    assert_eq!(row.permission, None);
    assert_eq!(row.agent, None);
    assert_eq!(row.model, None);
    assert_eq!(row.time_compacting, None);
    assert_eq!(row.time_archived, None);
    assert_eq!(row.time_suspended, None);
    assert_eq!(row.time_idle, None);
    assert_eq!(row.time_viewed, None);
    assert_eq!(row.idle_outcome, None);
    // NOT NULL columns survive the different physical order.
    assert_eq!(row.resume_attempts, 0);
    assert_eq!(row.cost, 0.0);
    assert_eq!(row.slug, "slug");
    assert_eq!(row.time_created, 1000);
}

#[test]
fn session_row_decodes_mixed_null_and_values_by_name() {
    let conn = migrated_shape_conn();
    conn.execute_batch(SESSION_ALL_CENSUS_NULL).expect("seed parent row");
    conn.execute_batch(SESSION_MIXED).expect("seed mixed row");

    let row = select_star(&conn, "session_v2", "ses_mix").pop().expect("one row");
    // Set columns land in THEIR fields despite the migrated tail order
    // (resume_attempts at 31, time_idle/time_viewed/idle_outcome at
    // 32/33/34 — where fresh-order decoding would read them as
    // time_compacting/time_archived/time_suspended).
    assert_eq!(row.resume_attempts, 7, "resume_attempts must not read idle_outcome");
    assert_eq!(row.time_idle, Some(333));
    assert_eq!(row.time_viewed, Some(444));
    assert_eq!(row.idle_outcome, Some("failed".to_string()));
    assert_eq!(row.time_compacting, Some(111));
    assert_eq!(row.time_archived, None);
    assert_eq!(row.time_suspended, Some(222));
    // The rest of the census mix.
    assert_eq!(row.workspace_id, None);
    assert_eq!(row.parent_id.as_deref(), Some("ses_null"));
    assert_eq!(row.path.as_deref(), Some("sub/dir"));
    assert_eq!(row.title.as_deref(), Some("A title"));
    assert_eq!(row.summary_additions, Some(42));
    assert_eq!(row.metadata.as_deref(), Some("{\"k\":\"v\"}"));
    assert_eq!(row.agent.as_deref(), Some("build"));
    assert!(row.model.is_some(), "model JSON present");
    assert_eq!(row.revert, None);
}

#[test]
fn session_row_decodes_identically_in_fresh_ddl_order() {
    // The fresh-bootstrap order interleaves time_idle/time_viewed/
    // idle_outcome BEFORE time_compacting (bootstrap.rs DDL) — the same
    // logical row must decode identically under either physical order.
    let conn = Connection::open_in_memory().expect("memory connection");
    conn.execute_batch(
        "CREATE TABLE session_v2 (
            id TEXT PRIMARY KEY, project_id TEXT NOT NULL, workspace_id TEXT, parent_id TEXT,
            fork_session_id TEXT, fork_boundary TEXT, slug TEXT NOT NULL, directory TEXT NOT NULL,
            path TEXT, title TEXT, version TEXT NOT NULL, share_url TEXT,
            summary_additions INTEGER, summary_deletions INTEGER, summary_files INTEGER,
            summary_diffs TEXT, metadata TEXT, cost REAL NOT NULL DEFAULT 0,
            tokens_input INTEGER NOT NULL DEFAULT 0, tokens_output INTEGER NOT NULL DEFAULT 0,
            tokens_reasoning INTEGER NOT NULL DEFAULT 0, tokens_cache_read INTEGER NOT NULL DEFAULT 0,
            tokens_cache_write INTEGER NOT NULL DEFAULT 0, revert TEXT, permission TEXT,
            agent TEXT, model TEXT, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL,
            time_idle INTEGER, time_viewed INTEGER, idle_outcome TEXT,
            time_compacting INTEGER, time_archived INTEGER, time_suspended INTEGER,
            resume_attempts INTEGER NOT NULL DEFAULT 0)",
    )
    .expect("fresh-order session_v2");
    conn.execute_batch(SESSION_MIXED).expect("seed mixed row");

    let row = select_star(&conn, "session_v2", "ses_mix").pop().expect("one row");
    assert_eq!(row.resume_attempts, 7);
    assert_eq!(row.time_idle, Some(333));
    assert_eq!(row.time_viewed, Some(444));
    assert_eq!(row.idle_outcome, Some("failed".to_string()));
    assert_eq!(row.time_compacting, Some(111));
    assert_eq!(row.time_suspended, Some(222));
}

#[test]
fn project_row_decodes_census_nulls_in_migrated_order() {
    let mut conn = migrated_shape_conn();
    conn.execute_batch(
        "INSERT INTO project (id, worktree, vcs, name, icon_url, icon_color, time_created,
             time_updated, time_initialized, sandboxes, commands, icon_url_override, time_active)
         VALUES ('prj_1', '/proj', NULL, NULL, NULL, NULL, 1, 2, NULL, '[]', NULL, NULL, 0),
                ('prj_2', '/other', 'git', 'Named', 'http://i', '#fff', 3, 4, 5, '[]',
                 '{\"start\":\"dev\"}', 'http://o', 9)",
    )
    .expect("seed project rows");

    let rows = list_rows!(&mut conn, ProjectRow);
    assert_eq!(rows.len(), 2);

    let all_null = &rows[0];
    assert_eq!(all_null.id, "prj_1");
    assert_eq!(all_null.worktree, "/proj");
    assert_eq!(all_null.vcs, None);
    assert_eq!(all_null.name, None);
    assert_eq!(all_null.icon_url, None);
    assert_eq!(all_null.icon_color, None);
    assert_eq!(all_null.icon_url_override, None);
    assert_eq!(all_null.time_initialized, None);
    assert_eq!(all_null.commands, None);
    assert_eq!(all_null.time_active, 0);

    let mixed = &rows[1];
    assert_eq!(mixed.vcs.as_deref(), Some("git"));
    assert_eq!(mixed.name.as_deref(), Some("Named"));
    assert_eq!(mixed.icon_url.as_deref(), Some("http://i"));
    assert_eq!(mixed.icon_color.as_deref(), Some("#fff"));
    assert_eq!(mixed.icon_url_override.as_deref(), Some("http://o"));
    assert_eq!(mixed.time_initialized, Some(5));
    assert_eq!(mixed.commands.as_deref(), Some("{\"start\":\"dev\"}"));
    assert_eq!(mixed.time_active, 9);
}

#[test]
fn credential_project_directory_worktree_rows_decode_census_nulls() {
    let mut conn = migrated_shape_conn();
    conn.execute_batch(
        "INSERT INTO credential (id, integration_id, label, value, connector_id, method_id,
             active, time_created, time_updated)
         VALUES ('crd_1', NULL, 'l', '{}', NULL, NULL, NULL, 1, 1),
                ('crd_2', 'int', 'l2', '{}', 'con', 'mth', 1, 2, 2)",
    )
    .expect("seed credential");
    conn.execute_batch(
        "INSERT INTO project_directory (project_id, directory, type, strategy, time_created)
         VALUES ('prj_1', '/proj', NULL, NULL, 1)",
    )
    .expect("seed project_directory");
    conn.execute_batch(
        "INSERT INTO worktree (project_id, directory, strategy, time_created)
         VALUES ('prj_1', '/proj', NULL, 1), ('prj_1', '/wt', 'git', 2)",
    )
    .expect("seed worktree");

    let credentials = list_rows!(&mut conn, CredentialRow);
    assert_eq!(credentials.len(), 2);
    let null_row = &credentials[0];
    assert_eq!(null_row.integration_id, None);
    assert_eq!(null_row.connector_id, None);
    assert_eq!(null_row.method_id, None);
    assert_eq!(null_row.active, None);
    let mixed = &credentials[1];
    assert_eq!(mixed.integration_id.as_deref(), Some("int"));
    assert_eq!(mixed.connector_id.as_deref(), Some("con"));
    assert_eq!(mixed.method_id.as_deref(), Some("mth"));
    assert_eq!(mixed.active, Some(true));

    let directories = list_rows!(&mut conn, ProjectDirectoryRow);
    let directory = directories.first().expect("row");
    assert_eq!(directory.type_, None);
    assert_eq!(directory.strategy, None);

    let worktrees = list_rows!(&mut conn, WorktreeRow);
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0].strategy, None);
    assert_eq!(worktrees[1].strategy.as_deref(), Some("git"));
}

#[test]
fn event_sequence_null_owner_id_survives_upsert_and_read() {
    // The real census: owner_id NULL on 893/893 rows. The sequence
    // upsert must preserve NULL (coalesce semantics) and the replay-side
    // read (seq, owner_id) must yield None — the strict-owner check only
    // engages when a stored owner EXISTS, exactly like v2.
    let mut conn = migrated_shape_conn();
    conn.execute_batch(
        "INSERT INTO event_sequence (aggregate_id, seq, owner_id) VALUES ('ses_x', 5, NULL)",
    )
    .expect("seed all-NULL owner");

    run_top_tx(&mut conn, Begin::Immediate, |tx| tx.event_sequence_upsert("ses_x", 6, None))
        .expect("upsert with None owner");

    let (seq, owner): (i64, Option<String>) = conn
        .query_row(
            "SELECT seq, owner_id FROM event_sequence WHERE aggregate_id = 'ses_x'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read back");
    assert_eq!(seq, 6, "sequence advanced");
    assert_eq!(owner, None, "NULL owner stays NULL (all-NULL census row)");

    // The mapper decodes the row the same way.
    run_top_tx(&mut conn, Begin::Deferred, |tx| {
        let row = EventSequenceRow::get(tx, "ses_x".to_string())?.expect("row present");
        assert_eq!(row.seq, 6);
        assert_eq!(row.owner_id, None);
        Ok(())
    })
    .expect("mapper read");

    // A replay that advances with a concrete owner sets it (coalesce
    // takes the new value); NULL rows before that must not have broken
    // anything.
    run_top_tx(&mut conn, Begin::Immediate, |tx| {
        tx.event_sequence_upsert("ses_x", 7, Some("own_1"))
    })
    .expect("upsert with owner");
    let owner: Option<String> = conn
        .query_row("SELECT owner_id FROM event_sequence WHERE aggregate_id = 'ses_x'", [], |row| {
            row.get(0)
        })
        .expect("read owner");
    assert_eq!(owner.as_deref(), Some("own_1"));
}

#[test]
fn session_row_writer_emits_null_for_absent_values() {
    // Writers must store NULL (v2 behavior), never sentinel strings —
    // round-trip a freshly-constructed row and assert the stored SQL
    // values are NULL for every absent census column.
    let mut conn = migrated_shape_conn();
    let row = SessionRow::new("ses_new", "prj_1", "/proj", "slug", "2.0.24").expect("row");
    run_top_tx(&mut conn, Begin::Immediate, |tx| SessionRow::insert(tx, &row)).expect("insert");

    let stored: (Option<String>, Option<String>, Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT title, agent, time_idle, idle_outcome FROM session_v2 WHERE id = 'ses_new'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read stored");
    assert_eq!(stored.0, None, "title stored as NULL");
    assert_eq!(stored.1, None, "agent stored as NULL");
    assert_eq!(stored.2, None, "time_idle stored as NULL");
    assert_eq!(stored.3, None, "idle_outcome stored as NULL");
}
