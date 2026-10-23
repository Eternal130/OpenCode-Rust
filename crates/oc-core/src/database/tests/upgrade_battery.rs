//! Pre-2.0.24 upgrade-path validation battery (GAPS §1.2, lane DBMIG).
//!
//! The migration engine's validation previously covered only two shapes:
//! databases produced by v2.0.24 itself (48/48 journal) and empty databases
//! (single-transaction bootstrap). Every INTERMEDIATE version — a database
//! created by an older release whose head was migration k — took the
//! incremental path untested. This battery synthesizes those databases and
//! drives them to head through the real runner:
//!
//! - **Stepwise integrity** ([`stepwise_upgrade_from_every_prefix_converges_to_bootstrap_schema`]):
//!   for EVERY k in 1..=48, build a database whose schema and journal are
//!   exactly what a release with head k would have left, run the full
//!   classifier entrypoint (`apply`), and assert the resulting schema is
//!   semantically equal to the bootstrap snapshot (same tables, same column
//!   sets with types/NOT NULL/defaults/PK flags, same foreign keys, same
//!   explicit indexes) — modulo the documented legacy residue the
//!   incremental path intentionally keeps.
//! - **Canary survival** (the `canaries_*` tests): at four hand-picked
//!   boundaries, minimal valid rows are inserted into the affected tables
//!   and must remain readable — with designed column mappings applied —
//!   after the upgrade to head. The session-table lineage is covered
//!   explicitly: the mainline `session` table (k=1/21/38) and the split
//!   `session_v2` table (k=39).
//!
//! What this battery does NOT prove:
//!
//! - The intermediates are synthetic (real migration files through the real
//!   runner, journal trimmed by hand) — not databases produced by real
//!   historical binaries. The real-corpus gate stays `OC_P10`.
//! - The `loose_psylocke` marker branch (pre-split V2 lineage, journal
//!   containing `20260730195856_optional_session_title`, `session` RENAMED
//!   to `session_v2`) is not exercised: a mainline prefix journal never
//!   contains the marker id.
//! - `v1_migration` (the v1→v2 CONTENT migration) is out of scope; this
//!   battery only proves the DDL/journal path.
//!
//! All databases are `:memory:`; the `MigrationOptions::global_data`
//! directory is an empty tempdir so `20260805200742_import_legacy_credentials`
//! runs its no-`auth.json` skip path. No real user data directory is touched.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use rusqlite::OptionalExtension;

use super::super::bootstrap::{MIGRATION_IDS, MIGRATION_TABLE_IF_NOT_EXISTS_SQL};
use super::super::migrations::{self, ApplyReport, MigrationOptions};

/// Legacy tables the incremental path keeps at head but the fresh-database
/// bootstrap never creates. `20260804233008_loose_psylocke`'s no-marker
/// branch creates `session_v2` ALONGSIDE the mainline `session` table (only
/// the marker branch renames), and no migration ever drops the v1-era
/// tables. Real migrated mainline databases therefore carry this residue —
/// the battery pins the exact set so any new residue fails loudly.
const LEGACY_RESIDUE_TABLES: &[&str] = &["message", "part", "session", "session_share", "todo"];

/// Explicit indexes belonging to [`LEGACY_RESIDUE_TABLES`] (or to the old
/// `session` table's dropped-at-split index set) that survive at head.
const LEGACY_RESIDUE_INDEXES: &[&str] = &[
    "message_session_time_created_id_idx",
    "part_message_id_id_idx",
    "part_session_idx",
    "session_parent_idx",
    "session_project_idx",
    "session_workspace_idx",
    "todo_session_idx",
];

/// Documented, accepted column-level divergences between the bootstrap
/// snapshot DDL and the migration-produced head schema. Both texts are
/// transliterated verbatim from the reference, so the reference itself has
/// these differences; pinning them keeps every NEW divergence a failure.
///
/// `account_state.id`: `schema.gen.ts` says `integer PRIMARY KEY`
/// (`table_info.notnull = 0`) while migration `20260228203230_blue_harpoon`
/// says `integer PRIMARY KEY NOT NULL` (`notnull = 1`) — cosmetic for a
/// rowid-alias column. Entries are `(table, column, bootstrap_notnull,
/// upgraded_notnull)`.
const ACCEPTED_COLUMN_DIVERGENCES: &[(&str, &str, i64, i64)] = &[("account_state", "id", 0, 1)];

fn battery_options(dir: &tempfile::TempDir) -> MigrationOptions {
    MigrationOptions { global_data: Some(dir.path().to_path_buf()) }
}

fn journal(conn: &Connection) -> BTreeSet<String> {
    let mut statement = conn.prepare("SELECT id FROM migration").expect("prepare journal");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query journal")
        .collect::<Result<BTreeSet<_>, _>>()
        .expect("journal rows")
}

fn expected_journal(k: usize) -> BTreeSet<String> {
    MIGRATION_IDS[..k].iter().map(|id| (*id).to_string()).collect()
}

/// Builds the synthetic intermediate-version database: applies migrations
/// `1..=k` through the REAL incremental runner (`apply_only` — the same
/// entrypoint `apply` dispatches to for databases with a session table),
/// with the not-yet-existing ids `k+1..=48` pre-seeded into the journal so
/// the runner stops at k. The pre-seeded tail is then trimmed, leaving the
/// journal holding exactly ids `1..k` — the state a release whose head was
/// migration k would have left behind.
fn intermediate_db(k: usize, opts: &MigrationOptions) -> Connection {
    assert!(k >= 1 && k <= MIGRATION_IDS.len(), "k out of range: {k}");
    let mut conn = Connection::open_in_memory().expect("memory connection");
    conn.execute_batch(MIGRATION_TABLE_IF_NOT_EXISTS_SQL).expect("journal table");
    let tail = &MIGRATION_IDS[k..];
    for id in tail {
        conn.execute(
            "INSERT INTO migration (id, time_completed) VALUES (?1, ?2)",
            rusqlite::params![*id, 0_i64],
        )
        .expect("pre-seed tail journal id");
    }
    let report = migrations::apply_only(&mut conn, opts)
        .unwrap_or_else(|err| panic!("k={k}: stepwise apply 1..={k} failed: {err}"));
    assert_eq!(report.applied, MIGRATION_IDS[..k].to_vec(), "k={k}: runner order");
    for id in tail {
        conn.execute("DELETE FROM migration WHERE id = ?1", rusqlite::params![*id])
            .expect("trim pre-seeded tail id");
    }
    assert_eq!(journal(&conn), expected_journal(k), "k={k}: journal is exactly ids 1..={k}");
    conn
}

/// Runs the full classifier entrypoint (`apply`) on an intermediate
/// database and asserts it takes the incremental path to a complete journal.
fn upgrade_to_head(conn: &mut Connection, k: usize, opts: &MigrationOptions) -> ApplyReport {
    let report = migrations::apply(conn, opts)
        .unwrap_or_else(|err| panic!("k={k}: upgrade to head failed: {err}"));
    assert!(!report.bootstrapped, "k={k}: intermediate DB must not re-bootstrap");
    assert!(!report.seeded_legacy, "k={k}: no legacy drizzle journal expected");
    assert_eq!(
        report.applied,
        MIGRATION_IDS[k..].to_vec(),
        "k={k}: head run applies exactly the remaining ids"
    );
    assert_eq!(journal(conn), expected_journal(MIGRATION_IDS.len()), "k={k}: journal complete");
    report
}

/// Semantic signature of one table: column attributes keyed by name
/// (physical order is deliberately ignored — real migrated databases keep
/// the ALTER-order, see `null_tolerance.rs`), foreign-key set, and explicit
/// (CREATE INDEX) indexes with their ordered columns.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TableSig {
    /// column name -> (type, `notnull`, `dflt_value`, `pk`)
    columns: BTreeMap<String, (String, i64, Option<String>, i64)>,
    /// (`from`, `to`, parent table, `on_update`, `on_delete`)
    fks: BTreeSet<(String, Option<String>, String, String, String)>,
    /// index name -> (unique, partial, ordered columns)
    indexes: BTreeMap<String, (bool, bool, Vec<String>)>,
}

/// Captures the semantic schema of every opencode table (internal
/// `sqlite_%` tables and the `migration` journal are excluded; the journal
/// contents differ between paths by design).
fn schema_signature(conn: &Connection) -> BTreeMap<String, TableSig> {
    let mut tables: Vec<String> = {
        let mut statement = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> 'migration'",
            )
            .expect("prepare table list");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query tables")
            .collect::<Result<Vec<_>, _>>()
            .expect("table rows")
    };
    tables.sort();
    let mut signature = BTreeMap::new();
    for table in tables {
        let mut columns = BTreeMap::new();
        let mut statement =
            conn.prepare(&format!("PRAGMA table_info({table})")).expect("table_info");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?.to_lowercase(),
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .expect("query table_info")
            .collect::<Result<Vec<_>, _>>()
            .expect("table_info rows");
        for (name, sql_type, notnull, dflt, pk) in rows {
            columns.insert(name, (sql_type, notnull, dflt, pk));
        }

        let mut fks = BTreeSet::new();
        let mut statement =
            conn.prepare(&format!("PRAGMA foreign_key_list({table})")).expect("foreign_key_list");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .expect("query foreign_key_list")
            .collect::<Result<Vec<_>, _>>()
            .expect("foreign_key rows");
        for (from, to, parent, on_update, on_delete) in rows {
            fks.insert((from, to, parent, on_update, on_delete));
        }

        let mut indexes = BTreeMap::new();
        let mut statement =
            conn.prepare(&format!("PRAGMA index_list({table})")).expect("index_list");
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            })
            .expect("query index_list")
            .collect::<Result<Vec<_>, _>>()
            .expect("index rows");
        for (name, unique, origin, partial) in rows {
            // Only explicit CREATE INDEX objects; UNIQUE/PK auto-indexes are
            // implied by the column signature.
            if origin != "c" {
                continue;
            }
            let mut info = conn.prepare(&format!("PRAGMA index_info({name})")).expect("index_info");
            let columns_out = info
                .query_map([], |row| row.get::<_, String>(2))
                .expect("query index_info")
                .collect::<Result<Vec<_>, _>>()
                .expect("index_info rows");
            indexes.insert(name, (unique, partial, columns_out));
        }
        signature.insert(table, TableSig { columns, fks, indexes });
    }
    signature
}

fn assert_head_schema_matches_bootstrap(
    k: usize,
    upgraded: &BTreeMap<String, TableSig>,
    bootstrap: &BTreeMap<String, TableSig>,
) {
    for (table, want) in bootstrap {
        let got = upgraded
            .get(table)
            .unwrap_or_else(|| panic!("k={k}: bootstrap table {table} missing at head"));
        assert_eq!(
            got.columns.len(),
            want.columns.len(),
            "k={k}: column count of {table} (bootstrap {:?} vs upgraded {:?})",
            want.columns.keys().collect::<Vec<_>>(),
            got.columns.keys().collect::<Vec<_>>()
        );
        for (column, want_col) in &want.columns {
            let got_col = &got.columns[column];
            if let Some(&(_, _, want_nn, got_nn)) = ACCEPTED_COLUMN_DIVERGENCES
                .iter()
                .find(|(t, c, _, _)| *t == table.as_str() && *c == column.as_str())
            {
                assert_eq!(want_col.1, want_nn, "k={k}: {table}.{column} bootstrap notnull");
                assert_eq!(got_col.1, got_nn, "k={k}: {table}.{column} upgraded notnull");
                assert_eq!(
                    (got_col.0.as_str(), &got_col.2, got_col.3),
                    (want_col.0.as_str(), &want_col.2, want_col.3),
                    "k={k}: {table}.{column} diverges beyond the documented notnull delta"
                );
                continue;
            }
            assert_eq!(got_col, want_col, "k={k}: column {table}.{column}");
        }
        assert_eq!(got.fks, want.fks, "k={k}: foreign keys of {table}");
        assert_eq!(got.indexes, want.indexes, "k={k}: indexes of {table}");
    }
    let residue_tables: BTreeSet<&str> =
        upgraded.keys().map(String::as_str).filter(|t| !bootstrap.contains_key(*t)).collect();
    assert_eq!(
        residue_tables,
        LEGACY_RESIDUE_TABLES.iter().copied().collect::<BTreeSet<_>>(),
        "k={k}: unexpected legacy table residue"
    );
    let mut residue_indexes: BTreeSet<&str> = BTreeSet::new();
    for table in &residue_tables {
        residue_indexes.extend(upgraded[*table].indexes.keys().map(String::as_str));
    }
    assert_eq!(
        residue_indexes,
        LEGACY_RESIDUE_INDEXES.iter().copied().collect::<BTreeSet<_>>(),
        "k={k}: unexpected legacy index residue"
    );
}

// ---------------------------------------------------------------------------
// (a) Stepwise integrity — exhaustive k = 1..=48
// ---------------------------------------------------------------------------

#[test]
fn stepwise_upgrade_from_every_prefix_converges_to_bootstrap_schema() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let opts = battery_options(&dir);

    // Reference: the direct-latest schema a fresh v2.0.24 database gets
    // from the single-transaction bootstrap (the migration FILES never run
    // on an empty database).
    let mut fresh = Connection::open_in_memory().expect("memory connection");
    let report = migrations::apply(&mut fresh, &opts).expect("bootstrap reference");
    assert!(report.bootstrapped, "empty database takes the bootstrap path");
    let bootstrap_sig = schema_signature(&fresh);

    for k in 1..=MIGRATION_IDS.len() {
        let mut conn = intermediate_db(k, &opts);
        upgrade_to_head(&mut conn, k, &opts);
        assert_head_schema_matches_bootstrap(k, &schema_signature(&conn), &bootstrap_sig);
    }
}

// ---------------------------------------------------------------------------
// (b) Canary survival at key boundaries
// ---------------------------------------------------------------------------

fn row<T, F>(conn: &Connection, sql: &str, map: F) -> T
where
    F: FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    conn.query_row(sql, [], map).unwrap_or_else(|err| panic!("query failed ({sql}): {err}"))
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap_or_else(|err| panic!("count({table}) failed: {err}"))
}

/// Compiles a zero-row SELECT and returns its column names — a shape probe
/// for tables that are deliberately empty at head.
fn probe_columns(conn: &Connection, sql: &str) -> Vec<String> {
    let statement = conn.prepare(sql).unwrap_or_else(|err| panic!("prepare failed ({sql}): {err}"));
    statement.column_names().iter().map(|name| (*name).to_string()).collect()
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        rusqlite::params![table],
        |_| Ok(()),
    )
    .optional()
    .expect("sqlite_master probe")
    .is_some()
}

/// Boundary k=1 — the base v1-era schema (`project`/`session`/`message`/
/// `part`/`todo`/`session_share`/old `permission`).
fn seed_k1(conn: &mut Connection) {
    conn.execute_batch(
        r#"
        INSERT INTO project (id, worktree, vcs, name, icon_url, icon_color, time_created, time_updated, time_initialized, sandboxes)
        VALUES ('prj_b1', '/proj/b1', 'git', 'B1 Project', 'http://icon/b1.png', '#fff', 100, 200, 300, '[]');

        INSERT INTO session (id, project_id, parent_id, slug, directory, title, version, share_url,
            summary_additions, summary_deletions, summary_files, summary_diffs, revert, permission,
            time_created, time_updated, time_compacting, time_archived)
        VALUES ('ses_b1', 'prj_b1', NULL, 'slug-b1', '/proj/b1', 'B1 Session', '1.0.0', 'http://share/b1',
            1, 2, 3, 'diffs-b1', 'rev-b1', 'perm-b1', 1000, 1100, NULL, NULL);

        INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES
            ('msg_b1_user', 'ses_b1', 1200, 1200, '{"role":"user"}'),
            ('msg_b1_asst', 'ses_b1', 1300, 1300, '{"role":"assistant","cost":1.5,"tokens":{"input":100,"output":200,"reasoning":10,"cache":{"read":30,"write":40}}}');

        INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
        VALUES ('prt_b1', 'msg_b1_asst', 'ses_b1', 1310, 1310, '{"type":"text"}');

        INSERT INTO todo (session_id, content, status, priority, position, time_created, time_updated)
        VALUES ('ses_b1', 'todo-b1', 'pending', 'high', 0, 1400, 1400);

        INSERT INTO session_share (session_id, id, secret, url, time_created, time_updated)
        VALUES ('ses_b1', 'shr_b1', 'sec-b1', 'http://share-url/b1', 1500, 1500);

        INSERT INTO permission (project_id, time_created, time_updated, data)
        VALUES ('prj_b1', 1600, 1600, '{"legacy":true}');
        "#,
    )
    .expect("seed k=1 canaries");
}

fn assert_k1_head(conn: &Connection) {
    // project: original columns preserved; ALTERs backfilled
    // (commands NULL, icon_url_override := icon_url, time_active := time_updated).
    let project: (String, Option<String>, Option<String>, Option<String>, i64) = row(
        conn,
        "SELECT worktree, commands, icon_url, icon_url_override, time_active
         FROM project WHERE id = 'prj_b1'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    );
    assert_eq!(
        project,
        (
            "/proj/b1".to_string(),
            None,
            Some("http://icon/b1.png".to_string()),
            Some("http://icon/b1.png".to_string()),
            200
        )
    );

    // Session lineage: the v1 row survives in the legacy `session` table;
    // migration 20260510033149 aggregates the assistant message usage into
    // the cost/tokens columns; later ALTER columns arrive as NULL.
    let identity: (String, Option<String>, Option<String>, Option<String>) = row(
        conn,
        "SELECT title, metadata, permission, revert FROM session WHERE id = 'ses_b1'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    );
    assert_eq!(identity.0, "B1 Session");
    assert_eq!(identity.1, None, "metadata column added empty");
    assert_eq!(identity.2, Some("perm-b1".to_string()), "legacy permission column intact");
    assert_eq!(identity.3, Some("rev-b1".to_string()));
    let added: (Option<String>, Option<String>, Option<String>, Option<String>) = row(
        conn,
        "SELECT path, agent, model, workspace_id FROM session WHERE id = 'ses_b1'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    );
    assert_eq!(
        added,
        (None, None, None, None),
        "path/agent/model/workspace_id columns added empty"
    );
    let usage: (f64, i64, i64, i64, i64, i64) = row(
        conn,
        "SELECT cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write
         FROM session WHERE id = 'ses_b1'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    );
    assert_eq!(usage, (1.5, 100, 200, 10, 30, 40), "usage aggregated from assistant message JSON");

    // Legacy message/part/todo/session_share rows readable and intact.
    assert_eq!(count(conn, "message"), 2);
    let part: (String, String, String) =
        row(conn, "SELECT message_id, session_id, data FROM part WHERE id = 'prt_b1'", |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
    assert_eq!(
        part,
        ("msg_b1_asst".to_string(), "ses_b1".to_string(), "{\"type\":\"text\"}".to_string())
    );
    let todo: (String, String, i64) =
        row(conn, "SELECT content, status, position FROM todo WHERE session_id = 'ses_b1'", |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
    assert_eq!(todo, ("todo-b1".to_string(), "pending".to_string(), 0));
    let share: (String, String) =
        row(conn, "SELECT secret, url FROM session_share WHERE session_id = 'ses_b1'", |r| {
            Ok((r.get(0)?, r.get(1)?))
        });
    assert_eq!(share, ("sec-b1".to_string(), "http://share-url/b1".to_string()));

    // Old `permission` (project_id-PK shape) is dropped at 20260601202201 and
    // recreated empty in the new id/action/resource shape — designed loss.
    assert_eq!(
        count(conn, "permission"),
        0,
        "legacy permission rows do not survive the table swap"
    );
    assert_eq!(
        probe_columns(conn, "SELECT id, project_id, action, resource FROM permission LIMIT 0"),
        vec![
            "id".to_string(),
            "project_id".to_string(),
            "action".to_string(),
            "resource".to_string()
        ],
        "new permission shape"
    );
}

/// Boundary k=21 — post `session-metadata`, pre `normalize_storage_paths`:
/// windows-style paths get normalized during the upgrade.
fn seed_k21(conn: &mut Connection) {
    conn.execute_batch(
        r#"
        INSERT INTO project (id, worktree, vcs, name, time_created, time_updated, sandboxes)
        VALUES ('prj_b2', 'C:\proj\b2', 'git', 'B2 Project', 2100, 2200, '[]');

        INSERT INTO session (id, project_id, slug, directory, title, version, path, agent, model,
            metadata, cost, tokens_input, time_created, time_updated)
        VALUES ('ses_b2', 'prj_b2', 'slug-b2', 'C:\proj\b2\sub', 'B2 Session', '2.0.0',
            'C:\proj\b2\sub\deep', 'build', '{"id":"m-b2"}', '{"m":"b2"}', 9.5, 111, 2300, 2400);

        INSERT INTO workspace (id, type, name, branch, directory, extra, project_id, time_used)
        VALUES ('wsp_b2', 'main', 'B2 WS', NULL, NULL, NULL, 'prj_b2', 5);
        "#,
    )
    .expect("seed k=21 canaries");
}

fn assert_k21_head(conn: &Connection) {
    let project: (String,) =
        row(conn, "SELECT worktree FROM project WHERE id = 'prj_b2'", |r| Ok((r.get(0)?,)));
    assert_eq!(project.0, "C:/proj/b2", "worktree backslashes normalized");

    // Session lineage: directory/path normalized; the usage columns are NOT
    // recomputed (20260510033149 already ran before the canary was seeded).
    let session: (String, String, Option<String>, Option<String>, Option<String>, f64, i64) = row(
        conn,
        "SELECT directory, path, metadata, agent, model, cost, tokens_input
         FROM session WHERE id = 'ses_b2'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    );
    assert_eq!(session.0, "C:/proj/b2/sub", "directory normalized");
    assert_eq!(session.1, "C:/proj/b2/sub/deep", "path normalized");
    assert_eq!(session.2, Some("{\"m\":\"b2\"}".to_string()), "metadata intact");
    assert_eq!(session.3, Some("build".to_string()), "agent intact");
    assert_eq!(session.4, Some("{\"id\":\"m-b2\"}".to_string()), "model intact");
    assert_eq!(session.5, 9.5, "cost not recomputed");
    assert_eq!(session.6, 111, "tokens_input not recomputed");

    // workspace canary: designed loss — 20260622202450 wipes the table and
    // 20260808023530 replaces it with the provider/binding shape.
    assert_eq!(count(conn, "workspace"), 0, "workspace rows are reset by design");
    assert_eq!(
        probe_columns(conn, "SELECT provider, binding, created_at FROM workspace LIMIT 0"),
        vec!["provider".to_string(), "binding".to_string(), "created_at".to_string()],
        "new workspace shape"
    );
}

/// Boundary k=38 — the eve of `loose_psylocke`: the fully-evolved mainline
/// `session` table with the complete v1 lineage.
fn seed_k38(conn: &mut Connection) {
    conn.execute_batch(
        r#"
        INSERT INTO project (id, worktree, vcs, name, icon_url, icon_color, time_created, time_updated, time_initialized, sandboxes, commands)
        VALUES ('prj_b3', '/proj/b3', 'git', 'B3 Project', NULL, NULL, 3000, 3100, NULL, '[]', '{"start":"dev-b3"}');

        INSERT INTO session (id, project_id, parent_id, slug, directory, title, version, share_url,
            summary_additions, summary_deletions, summary_files, summary_diffs, revert, permission,
            time_created, time_updated, time_compacting, time_archived, workspace_id, path, agent, model,
            cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, metadata)
        VALUES ('ses_b3', 'prj_b3', NULL, 'slug-b3', '/proj/b3', 'B3 Session', '2.0.0', NULL,
            1, 2, 3, 'diffs-b3', 'rev-b3', 'ask', 3200, 3300, NULL, NULL, NULL, 'sub/b3', 'plan',
            '{"id":"m-b3"}', 3.25, 5, 6, 7, 8, 9, '{"b":3}');

        INSERT INTO message (id, session_id, time_created, time_updated, data)
        VALUES ('msg_b3', 'ses_b3', 3400, 3400, '{"role":"user","text":"b3"}');

        INSERT INTO event_sequence (aggregate_id, seq, owner_id) VALUES ('ses_b3', 42, 'own-b3');
        INSERT INTO event (id, aggregate_id, seq, type, data) VALUES ('evt_b3', 'ses_b3', 42, 'b3-type', '{"e":3}');
        "#,
    )
    .expect("seed k=38 canaries");
}

fn assert_k38_head(conn: &Connection) {
    // Session lineage, split boundary: the mainline no-marker branch of
    // `loose_psylocke` keeps the legacy `session` table in place (it does
    // NOT rename it) and creates `session_v2` fresh — v1 rows stay readable
    // in the legacy table; carrying their CONTENT into `session_v2` is the
    // v1_migration module's job, not the schema journal's.
    let session: (String, Option<String>, Option<String>) =
        row(conn, "SELECT title, metadata, permission FROM session WHERE id = 'ses_b3'", |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
    assert_eq!(session.0, "B3 Session");
    assert_eq!(session.1, Some("{\"b\":3}".to_string()), "metadata intact");
    assert_eq!(
        session.2,
        Some("ask".to_string()),
        "legacy permission NOT cleared (20260910120000 targets session_v2)"
    );
    let session_lineage: (Option<String>, Option<String>, Option<String>) =
        row(conn, "SELECT agent, model, path FROM session WHERE id = 'ses_b3'", |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
    assert_eq!(session_lineage.0, Some("plan".to_string()), "agent intact");
    assert_eq!(session_lineage.1, Some("{\"id\":\"m-b3\"}".to_string()), "model intact");
    assert_eq!(session_lineage.2, Some("sub/b3".to_string()), "path intact");
    let session_usage: (f64, i64) =
        row(conn, "SELECT cost, tokens_input FROM session WHERE id = 'ses_b3'", |r| {
            Ok((r.get(0)?, r.get(1)?))
        });
    assert_eq!(session_usage, (3.25, 5), "usage columns intact");
    assert_eq!(
        count(conn, "session_v2"),
        0,
        "session_v2 created fresh (no rename in this lineage)"
    );

    let message: (String, String) =
        row(conn, "SELECT session_id, data FROM message WHERE id = 'msg_b3'", |r| {
            Ok((r.get(0)?, r.get(1)?))
        });
    assert_eq!(message, ("ses_b3".to_string(), "{\"role\":\"user\",\"text\":\"b3\"}".to_string()));

    // Events survive the split with the `created` column backfilled to 0.
    let event: (String, i64, String, i64) =
        row(conn, "SELECT type, seq, data, created FROM event WHERE id = 'evt_b3'", |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        });
    assert_eq!(event, ("b3-type".to_string(), 42, "{\"e\":3}".to_string(), 0));
    let sequence: (i64, Option<String>) =
        row(conn, "SELECT seq, owner_id FROM event_sequence WHERE aggregate_id = 'ses_b3'", |r| {
            Ok((r.get(0)?, r.get(1)?))
        });
    assert_eq!(sequence, (42, Some("own-b3".to_string())));

    // Tables retired by the split are gone; the split's new tables exist.
    for table in ["data_migration", "session_context_epoch", "session_input"] {
        assert!(!table_exists(conn, table), "{table} must be dropped at the split");
    }
    for table in
        ["kv", "instruction_blob", "instruction_entry", "instruction_state", "session_pending"]
    {
        assert!(table_exists(conn, table), "{table} must exist after the split");
    }
}

/// Boundary k=39 — post-split (`session_v2` live), pre head: the v2 canary
/// set across the tables the remaining migrations touch.
fn seed_k39(conn: &mut Connection) {
    conn.execute_batch(
        r#"
        INSERT INTO project (id, worktree, vcs, name, icon_url, icon_color, time_created, time_updated, time_initialized, sandboxes, commands)
        VALUES ('prj_b4', '/proj/b4', 'git', 'B4 Project', NULL, NULL, 4000, 4100, NULL, '[]', NULL);

        INSERT INTO session_v2 (id, project_id, workspace_id, parent_id, fork_session_id, fork_boundary,
            slug, directory, path, title, version, share_url, summary_additions, summary_deletions,
            summary_files, summary_diffs, metadata, cost, tokens_input, tokens_output, tokens_reasoning,
            tokens_cache_read, tokens_cache_write, revert, permission, agent, model, time_created,
            time_updated, time_compacting, time_archived, time_suspended)
        VALUES ('ses_b4', 'prj_b4', NULL, NULL, NULL, NULL, 'slug-b4', '/proj/b4', 'p4', 'B4 Session',
            '2.0.24', NULL, 1, 2, 3, 'diffs-b4', '{"b":4}', 4.5, 5, 6, 7, 8, 9, NULL, 'allow', 'build',
            NULL, 4200, 4300, NULL, NULL, NULL);

        INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
        VALUES ('smsg_b4', 'ses_b4', 'message', 1, 4400, 4400, '{"role":"assistant"}');

        INSERT INTO event_sequence (aggregate_id, seq, owner_id) VALUES ('ses_b4', 9, 'own-b4');
        INSERT INTO event (id, aggregate_id, seq, type, data) VALUES ('evt_b4', 'ses_b4', 9, 'b4-type', '{"e":4}');

        INSERT INTO kv (key, value, time_created, time_updated) VALUES ('k-b4', '{"v":4}', 4500, 4500);

        INSERT INTO instruction_blob (hash, value) VALUES ('hash-b4', 'blob-b4');
        INSERT INTO instruction_entry (session_id, key, value, time_created, time_updated)
        VALUES ('ses_b4', 'key-b4', 'entry-b4', 4600, 4600);
        INSERT INTO instruction_state (session_id, epoch_start, through_seq, initial_values, current_values)
        VALUES ('ses_b4', 1, 2, '{"i":1}', '{"c":2}');

        INSERT INTO session_pending (id, session_id, type, data, delivery, admitted_seq, time_created)
        VALUES ('sp-b4', 'ses_b4', 'compaction', '{"p":1}', NULL, 1, 4700);

        INSERT INTO project_directory (project_id, directory, type, strategy, time_created)
        VALUES ('prj_b4', '/wt/b4', 'git_worktree', NULL, 4800);
        "#,
    )
    .expect("seed k=39 canaries");
}

fn assert_k39_head(conn: &Connection) {
    // Session lineage, v2 side: the row survives with the designed column
    // mutations of migrations 42/45/47 (resume_attempts backfill, viewed
    // state columns added, permission cleared).
    let session: (String, Option<String>, Option<String>, Option<String>) = row(
        conn,
        "SELECT title, metadata, permission, agent FROM session_v2 WHERE id = 'ses_b4'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    );
    assert_eq!(session.0, "B4 Session");
    assert_eq!(session.1, Some("{\"b\":4}".to_string()), "metadata intact");
    assert_eq!(session.2, None, "permission cleared by 20260910120000");
    assert_eq!(session.3, Some("build".to_string()), "agent intact");
    let viewed: (Option<i64>, Option<i64>, Option<String>) = row(
        conn,
        "SELECT time_idle, time_viewed, idle_outcome FROM session_v2 WHERE id = 'ses_b4'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    );
    assert_eq!(viewed, (None, None, None), "viewed-state columns added NULL");
    let session_usage: (f64, i64, i64) = row(
        conn,
        "SELECT cost, tokens_input, resume_attempts FROM session_v2 WHERE id = 'ses_b4'",
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    );
    assert_eq!(session_usage, (4.5, 5, 0), "usage intact, resume_attempts backfilled to 0");

    let message: (String, i64, String) =
        row(conn, "SELECT type, seq, data FROM session_message WHERE id = 'smsg_b4'", |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        });
    assert_eq!(message, ("message".to_string(), 1, "{\"role\":\"assistant\"}".to_string()));

    let event: (i64, i64) = row(conn, "SELECT seq, created FROM event WHERE id = 'evt_b4'", |r| {
        Ok((r.get(0)?, r.get(1)?))
    });
    assert_eq!(event, (9, 0), "event intact, created stays 0");
    let sequence: (i64, Option<String>) =
        row(conn, "SELECT seq, owner_id FROM event_sequence WHERE aggregate_id = 'ses_b4'", |r| {
            Ok((r.get(0)?, r.get(1)?))
        });
    assert_eq!(sequence, (9, Some("own-b4".to_string())));

    let kv: (String, i64) =
        row(conn, "SELECT value, time_created FROM kv WHERE key = 'k-b4'", |r| {
            Ok((r.get(0)?, r.get(1)?))
        });
    assert_eq!(kv, ("{\"v\":4}".to_string(), 4500));
    let blob: (Option<String>,) =
        row(conn, "SELECT value FROM instruction_blob WHERE hash = 'hash-b4'", |r| {
            Ok((r.get(0)?,))
        });
    assert_eq!(blob, (Some("blob-b4".to_string()),));
    let entry: (Option<String>, i64) = row(
        conn,
        "SELECT value, time_created FROM instruction_entry WHERE session_id = 'ses_b4' AND key = 'key-b4'",
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    assert_eq!(entry, (Some("entry-b4".to_string()), 4600));
    let state: (i64, i64) = row(
        conn,
        "SELECT epoch_start, through_seq FROM instruction_state WHERE session_id = 'ses_b4'",
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    assert_eq!(state, (1, 2));
    let pending: (String, i64) =
        row(conn, "SELECT type, admitted_seq FROM session_pending WHERE id = 'sp-b4'", |r| {
            Ok((r.get(0)?, r.get(1)?))
        });
    assert_eq!(pending, ("compaction".to_string(), 1));

    // 20260812213948_worktree maps project_directory rows into the new
    // worktree table (type/strategy -> strategy vocabulary).
    let worktree: (Option<String>, i64) = row(
        conn,
        "SELECT strategy, time_created FROM worktree WHERE project_id = 'prj_b4' AND directory = '/wt/b4'",
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    assert_eq!(worktree, (Some("git".to_string()), 4800), "git_worktree type maps to git strategy");
    let directory: (Option<String>, Option<String>) = row(
        conn,
        "SELECT type, strategy FROM project_directory WHERE project_id = 'prj_b4' AND directory = '/wt/b4'",
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    assert_eq!(directory, (Some("git_worktree".to_string()), None), "source row kept");

    assert!(table_exists(conn, "session_inbox"), "session_inbox created at head");
}

#[test]
fn canaries_k1_base_schema_survive_upgrade() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let opts = battery_options(&dir);
    let mut conn = intermediate_db(1, &opts);
    seed_k1(&mut conn);
    upgrade_to_head(&mut conn, 1, &opts);
    assert_k1_head(&conn);
}

#[test]
fn canaries_k21_pre_normalize_paths_survive_upgrade() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let opts = battery_options(&dir);
    let mut conn = intermediate_db(21, &opts);
    seed_k21(&mut conn);
    upgrade_to_head(&mut conn, 21, &opts);
    assert_k21_head(&conn);
}

#[test]
fn canaries_k38_eve_of_split_survive_upgrade() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let opts = battery_options(&dir);
    let mut conn = intermediate_db(38, &opts);
    seed_k38(&mut conn);
    upgrade_to_head(&mut conn, 38, &opts);
    assert_k38_head(&conn);
}

#[test]
fn canaries_k39_post_split_survive_upgrade() {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let opts = battery_options(&dir);
    let mut conn = intermediate_db(39, &opts);
    seed_k39(&mut conn);
    upgrade_to_head(&mut conn, 39, &opts);
    assert_k39_head(&conn);
}
