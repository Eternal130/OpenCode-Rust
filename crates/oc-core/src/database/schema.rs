//! Typed row structs and basic CRUD for every table in
//! `packages/core/schema.json` (the drizzle-kit snapshot; DDL in
//! `bootstrap.rs`).
//!
//! Conventions (mirroring `packages/core/src/database/path.ts` and the
//! drizzle table definitions):
//!
//! - **JSON columns** (`text({ mode: "json" })`) are stored as `TEXT`; row
//!   fields hold the raw JSON string and [`encode_json`]/[`decode_json`]
//!   round-trip them via `serde_json`.
//! - **Path columns** (`absoluteColumn`/`directoryColumn`/`pathColumn`)
//!   normalize to forward-slash storage form via [`absolute_storage`] /
//!   [`directory_storage`] / [`storage_path`]; write-side validation rejects
//!   non-absolute values (except `pathColumn`, which allows any relative
//!   subpath, and empty legacy directories).
//! - **Timestamps** are `time_*` `INTEGER` columns holding milliseconds.
//! - Rust keywords used as column names (`type`) get a trailing underscore in
//!   Rust (`type_`); the SQL name strips it.
//!
//! Column order in every struct matches the DDL declaration order (primary-key
//! columns lead in every table), so positional `SELECT *` mapping works.

use rusqlite::OptionalExtension;

use super::error::{DbError, Result};
use super::migrations::now_ms;
use super::{Database, Tx};

// ---------------------------------------------------------------------------
// JSON + path column helpers
// ---------------------------------------------------------------------------

/// Serializes a JSON column value to its TEXT storage form.
#[must_use]
pub fn encode_json(value: &serde_json::Value) -> String {
    value.to_string()
}

/// Parses a JSON column from its TEXT storage form.
pub fn decode_json(value: &str) -> Result<serde_json::Value> {
    Ok(serde_json::from_str(value)?)
}

/// Parses an optional JSON column; `None` stays `None`.
pub fn decode_json_opt(value: Option<&str>) -> Result<Option<serde_json::Value>> {
    value.map(decode_json).transpose()
}

/// Forward-slash storage normalization (`path.ts storagePath`): on Windows the
/// input's backslashes become slashes; elsewhere the input is unchanged.
#[must_use]
pub fn storage_path(input: &str) -> String {
    if cfg!(windows) {
        input.replace('\\', "/")
    } else {
        input.to_string()
    }
}

/// A storage path written by a Windows host (`C:/...` or `//...`).
#[must_use]
pub fn is_windows_storage_path(input: &str) -> bool {
    input.len() >= 3
        && input.as_bytes()[1] == b':'
        && input.as_bytes()[2] == b'/'
        && input.as_bytes()[0].is_ascii_alphabetic()
        || input.starts_with("//")
}

/// `absoluteColumn.toDriver`: normalize to storage form, rejecting values
/// that are absolute under neither POSIX nor Windows conventions.
pub fn absolute_storage(input: &str) -> Result<String> {
    let result = storage_path(input);
    let absolute = result.starts_with('/') || is_windows_storage_path(&result);
    if !absolute {
        return Err(DbError::NotAbsolute(input.to_string()));
    }
    Ok(result)
}

/// `directoryColumn.toDriver`: like [`absolute_storage`] but an empty legacy
/// directory is preserved as-is.
pub fn directory_storage(input: &str) -> Result<String> {
    if input.is_empty() {
        Ok(input.to_string())
    } else {
        absolute_storage(input)
    }
}

// ---------------------------------------------------------------------------
// Table CRUD
// ---------------------------------------------------------------------------

/// Generates a row struct plus `insert` / `insert_or_ignore` / `get` / `list`
/// / `update` / `delete` / `count` associated functions on it.
macro_rules! table {
    (
        $(#[$meta:meta])*
        $row:ident => $table:literal,
        pk { $($pk:ident : $pkty:ty),+ $(,)? }
        cols { $($col:ident : $colty:ty),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $row {
            $(pub $pk: $pkty,)+
            $(pub $col: $colty,)*
        }

        impl $row {
            /// SQLite table name.
            pub const TABLE: &'static str = $table;

            /// SQL name for a Rust field (trailing `_` stripped for keywords
            /// like `type_` → `type`).
            fn sql_name(field: &'static str) -> &'static str {
                field.trim_end_matches('_')
            }

            fn columns() -> Vec<&'static str> {
                let mut columns = Vec::new();
                $(columns.push(Self::sql_name(stringify!($pk)));)+
                $(columns.push(Self::sql_name(stringify!($col)));)*
                columns
            }

            fn pk_where() -> String {
                Self::columns()[..Self::PK_COLUMNS.len()]
                    .iter()
                    .enumerate()
                    .map(|(index, column)| format!("{column} = ?{}", index + 1))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            }

            /// Primary-key column names, in declaration order.
            pub const PK_COLUMNS: &'static [&'static str] =
                &[$(stringify!($pk)),+];

            fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
                let mut index = 0usize;
                $(
                    let $pk: $pkty = row.get(index)?;
                    index += 1;
                )+
                $(
                    let $col: $colty = row.get(index)?;
                    index += 1;
                )*
                let _ = index;
                Ok(Self { $($pk,)+ $($col,)* })
            }

            /// Inserts the row; errors on primary-key conflicts.
            pub fn insert(tx: &mut Tx<'_>, row: &Self) -> Result<()> {
                let columns = Self::columns();
                let placeholders = columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    Self::TABLE,
                    columns.join(", "),
                    placeholders,
                );
                let values: Vec<&dyn rusqlite::ToSql> = vec![$(&row.$pk,)+ $(&row.$col,)*];
                tx.execute(&sql, values.as_slice())?;
                Ok(())
            }

            /// Inserts the row, ignoring primary-key conflicts
            /// (drizzle `onConflictDoNothing`).
            pub fn insert_or_ignore(tx: &mut Tx<'_>, row: &Self) -> Result<()> {
                let columns = Self::columns();
                let placeholders = columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "INSERT OR IGNORE INTO {} ({}) VALUES ({})",
                    Self::TABLE,
                    columns.join(", "),
                    placeholders,
                );
                let values: Vec<&dyn rusqlite::ToSql> = vec![$(&row.$pk,)+ $(&row.$col,)*];
                tx.execute(&sql, values.as_slice())?;
                Ok(())
            }

            /// Fetches one row by primary key.
            pub fn get(tx: &Tx<'_>, $($pk: $pkty),+) -> Result<Option<Self>> {
                let sql = format!("SELECT * FROM {} WHERE {}", Self::TABLE, Self::pk_where());
                let values: Vec<&dyn rusqlite::ToSql> = vec![$(&$pk),+];
                Ok(tx.conn().query_row(&sql, values.as_slice(), Self::from_row).optional()?)
            }

            /// Lists every row ordered by primary key.
            pub fn list(tx: &Tx<'_>) -> Result<Vec<Self>> {
                let order = Self::PK_COLUMNS.join(", ");
                let sql = format!("SELECT * FROM {} ORDER BY {order}", Self::TABLE);
                let mut statement = tx.conn().prepare(&sql)?;
                let rows = statement
                    .query_map([], Self::from_row)?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            }

            /// Updates every non-primary-key column of the row.
            pub fn update(tx: &mut Tx<'_>, row: &Self) -> Result<()> {
                let set_columns = Self::columns()[Self::PK_COLUMNS.len()..].to_vec();
                let assignments =
                    set_columns.iter().map(|c| format!("{c} = ?")).collect::<Vec<_>>().join(", ");
                let pk_columns = Self::columns()[..Self::PK_COLUMNS.len()].to_vec();
                let predicate =
                    pk_columns.iter().map(|c| format!("{c} = ?")).collect::<Vec<_>>().join(" AND ");
                let sql =
                    format!("UPDATE {} SET {assignments} WHERE {predicate}", Self::TABLE);
                // Bind order matches the bare `?` order: SET columns, then pk.
                let mut values: Vec<&dyn rusqlite::ToSql> = Vec::new();
                $(values.push(&row.$col);)*
                $(values.push(&row.$pk);)+
                tx.execute(&sql, values.as_slice())?;
                Ok(())
            }

            /// Deletes one row by primary key.
            pub fn delete(tx: &mut Tx<'_>, $($pk: $pkty),+) -> Result<()> {
                let sql = format!("DELETE FROM {} WHERE {}", Self::TABLE, Self::pk_where());
                let values: Vec<&dyn rusqlite::ToSql> = vec![$(&$pk),+];
                tx.execute(&sql, values.as_slice())?;
                Ok(())
            }

            /// Counts rows in the table.
            pub fn count(tx: &Tx<'_>) -> Result<i64> {
                let sql = format!("SELECT count(*) FROM {}", Self::TABLE);
                Ok(tx.conn().query_row(&sql, [], |row| row.get(0))?)
            }
        }
    };
}

table! {
    /// `project` — one row per resolved project.
    ProjectRow => "project",
    pk { id: String }
    cols {
        worktree: String,             // absoluteColumn
        vcs: Option<String>,
        name: Option<String>,
        icon_url: Option<String>,
        icon_url_override: Option<String>,
        icon_color: Option<String>,
        time_created: i64,
        time_updated: i64,
        time_initialized: Option<i64>,
        time_active: i64,
        sandboxes: String,            // JSON array of absolute paths
        commands: Option<String>,     // JSON { start?: string }
    }
}

table! {
    /// `project_directory` (deprecated by `worktree`, still written).
    ProjectDirectoryRow => "project_directory",
    pk { project_id: String, directory: String }
    cols {
        type_: Option<String>,        // "main" | "root" | "git_worktree"
        strategy: Option<String>,
        time_created: i64,
    }
}

table! {
    /// `workspace` — workspace bindings.
    WorkspaceRow => "workspace",
    pk { id: String }
    cols {
        provider: String,
        binding: Option<String>,      // JSON WorkspaceDriver.Binding
        created_at: i64,
        last_used_at: i64,
    }
}

table! {
    /// `worktree` — directories belonging to a project.
    WorktreeRow => "worktree",
    pk { project_id: String, directory: String }
    cols {
        strategy: Option<String>,
        time_created: i64,
    }
}

table! {
    /// `session_v2` — event-projected sessions.
    SessionRow => "session_v2",
    pk { id: String }
    cols {
        project_id: String,
        workspace_id: Option<String>,
        parent_id: Option<String>,
        fork_session_id: Option<String>,
        fork_boundary: Option<String>,   // JSON
        slug: String,
        directory: String,               // directoryColumn
        path: Option<String>,            // pathColumn
        title: Option<String>,
        version: String,
        share_url: Option<String>,
        summary_additions: Option<i64>,
        summary_deletions: Option<i64>,
        summary_files: Option<i64>,
        summary_diffs: Option<String>,   // JSON
        metadata: Option<String>,        // JSON
        cost: f64,
        tokens_input: i64,
        tokens_output: i64,
        tokens_reasoning: i64,
        tokens_cache_read: i64,
        tokens_cache_write: i64,
        revert: Option<String>,          // JSON
        permission: Option<String>,      // JSON ruleset
        agent: Option<String>,
        model: Option<String>,           // JSON
        time_created: i64,
        time_updated: i64,
        time_idle: Option<i64>,
        time_viewed: Option<i64>,
        idle_outcome: Option<String>,
        time_compacting: Option<i64>,
        time_archived: Option<i64>,
        time_suspended: Option<i64>,
        resume_attempts: i64,
    }
}

table! {
    /// `session_message` — message/part projections (`seq` = durable order).
    SessionMessageRow => "session_message",
    pk { id: String }
    cols {
        session_id: String,
        type_: String,
        seq: i64,
        time_created: i64,
        time_updated: i64,
        data: String,                    // JSON
    }
}

table! {
    /// `session_pending` — admitted-but-unprocessed inbox entries.
    SessionPendingRow => "session_pending",
    pk { id: String }
    cols {
        session_id: String,
        type_: String,
        data: String,                    // JSON
        delivery: Option<String>,
        admitted_seq: i64,
        time_created: i64,
    }
}

table! {
    /// `session_inbox` — durable user/synthetic input inbox.
    SessionInboxRow => "session_inbox",
    pk { id: String }
    cols {
        session_id: String,
        type_: String,
        payload: String,                 // JSON
        delivery: String,
        enqueued_seq: i64,
        time_created: i64,
    }
}

table! {
    /// `instruction_entry` — per-session instruction key/value state.
    InstructionEntryRow => "instruction_entry",
    pk { session_id: String, key: String }
    cols {
        value: Option<String>,           // JSON
        removed: bool,
        time_created: i64,
        time_updated: i64,
    }
}

table! {
    /// `instruction_blob` — content-addressed instruction blobs.
    InstructionBlobRow => "instruction_blob",
    pk { hash: String }
    cols {
        value: Option<String>,           // JSON
    }
}

table! {
    /// `instruction_state` — instruction projection checkpoint.
    InstructionStateRow => "instruction_state",
    pk { session_id: String }
    cols {
        epoch_start: i64,
        through_seq: i64,
        initial_values: String,          // JSON
        current_values: String,          // JSON
    }
}

table! {
    /// `event` — durable event log rows.
    EventRow => "event",
    pk { id: String }
    cols {
        aggregate_id: String,
        seq: i64,
        created: i64,
        type_: String,
        data: String,                    // JSON
    }
}

table! {
    /// `event_sequence` — per-aggregate sequence counters.
    EventSequenceRow => "event_sequence",
    pk { aggregate_id: String }
    cols {
        seq: i64,
        owner_id: Option<String>,
    }
}

table! {
    /// `permission` — saved "always" grants.
    PermissionRow => "permission",
    pk { id: String }
    cols {
        project_id: String,
        action: String,
        resource: String,
        time_created: i64,
        time_updated: i64,
    }
}

table! {
    /// `credential` — stored provider credentials.
    CredentialRow => "credential",
    pk { id: String }
    cols {
        integration_id: Option<String>,
        label: String,
        value: String,                   // JSON Credential.Value
        connector_id: Option<String>,
        method_id: Option<String>,
        active: Option<bool>,
        time_created: i64,
        time_updated: i64,
    }
}

table! {
    /// `account` — cloud accounts.
    AccountRow => "account",
    pk { id: String }
    cols {
        email: String,
        url: String,
        access_token: String,
        refresh_token: String,
        token_expiry: Option<i64>,
        time_created: i64,
        time_updated: i64,
    }
}

table! {
    /// `account_state` — singleton active-account pointer.
    AccountStateRow => "account_state",
    pk { id: i64 }
    cols {
        active_account_id: Option<String>,
        active_org_id: Option<String>,
    }
}

table! {
    /// `control_account` — legacy account table.
    ControlAccountRow => "control_account",
    pk { email: String, url: String }
    cols {
        access_token: String,
        refresh_token: String,
        token_expiry: Option<i64>,
        active: bool,
        time_created: i64,
        time_updated: i64,
    }
}

table! {
    /// `kv` — global key/value store (values are JSON).
    KvRow => "kv",
    pk { key: String }
    cols {
        value: String,                   // JSON
        time_created: i64,
        time_updated: i64,
    }
}

table! {
    /// `migration` — the migration journal.
    MigrationRow => "migration",
    pk { id: String }
    cols {
        time_completed: i64,
    }
}

// ---------------------------------------------------------------------------
// Row constructors and JSON accessors
// ---------------------------------------------------------------------------

impl ProjectRow {
    /// New project row with storage-normalized `worktree`, fresh timestamps,
    /// `time_active = 0` and an empty sandbox list.
    pub fn new(id: impl Into<String>, worktree: &str, vcs: Option<&str>) -> Result<Self> {
        let now = now_ms();
        Ok(Self {
            id: id.into(),
            worktree: absolute_storage(worktree)?,
            vcs: vcs.map(str::to_string),
            name: None,
            icon_url: None,
            icon_url_override: None,
            icon_color: None,
            time_created: now,
            time_updated: now,
            time_initialized: None,
            time_active: 0,
            sandboxes: encode_json(&serde_json::Value::Array(Vec::new())),
            commands: None,
        })
    }

    /// Decoded `sandboxes` JSON array.
    pub fn sandboxes_json(&self) -> Result<serde_json::Value> {
        decode_json(&self.sandboxes)
    }

    /// Decoded `commands` JSON object, when set.
    pub fn commands_json(&self) -> Result<Option<serde_json::Value>> {
        decode_json_opt(self.commands.as_deref())
    }
}

impl SessionRow {
    /// New session row with storage-normalized `directory`, fresh timestamps
    /// and zeroed usage counters.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        project_id: impl Into<String>,
        directory: &str,
        slug: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self> {
        let now = now_ms();
        Ok(Self {
            id: id.into(),
            project_id: project_id.into(),
            workspace_id: None,
            parent_id: None,
            fork_session_id: None,
            fork_boundary: None,
            slug: slug.into(),
            directory: directory_storage(directory)?,
            path: None,
            title: None,
            version: version.into(),
            share_url: None,
            summary_additions: None,
            summary_deletions: None,
            summary_files: None,
            summary_diffs: None,
            metadata: None,
            cost: 0.0,
            tokens_input: 0,
            tokens_output: 0,
            tokens_reasoning: 0,
            tokens_cache_read: 0,
            tokens_cache_write: 0,
            revert: None,
            permission: None,
            agent: None,
            model: None,
            time_created: now,
            time_updated: now,
            time_idle: None,
            time_viewed: None,
            idle_outcome: None,
            time_compacting: None,
            time_archived: None,
            time_suspended: None,
            resume_attempts: 0,
        })
    }

    /// Decoded `metadata` JSON, when set.
    pub fn metadata_json(&self) -> Result<Option<serde_json::Value>> {
        decode_json_opt(self.metadata.as_deref())
    }
}

impl EventRow {
    /// Decoded `data` JSON payload.
    pub fn data_json(&self) -> Result<serde_json::Value> {
        decode_json(&self.data)
    }
}

// ---------------------------------------------------------------------------
// Additive row helpers (session lane) — public positional decoders so other
// modules can map ad-hoc `SELECT *` queries through the same constructor
// the macro keeps private.
// ---------------------------------------------------------------------------

impl SessionRow {
    /// Positional `SELECT *` row decoder.
    pub fn decode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Self::from_row(row)
    }
}

impl SessionMessageRow {
    /// Positional `SELECT *` row decoder.
    pub fn decode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Self::from_row(row)
    }
}

impl SessionInboxRow {
    /// Positional `SELECT *` row decoder.
    pub fn decode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Self::from_row(row)
    }
}

// ---------------------------------------------------------------------------
// Domain-shaped helpers on Tx
// ---------------------------------------------------------------------------

impl Tx<'_> {
    /// Upserts a kv entry (JSON value), refreshing `time_updated`.
    pub fn kv_set(&mut self, key: &str, value: &serde_json::Value) -> Result<()> {
        let now = now_ms();
        self.execute(
            "INSERT INTO kv (key, value, time_created, time_updated) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value, time_updated = excluded.time_updated",
            rusqlite::params![key, encode_json(value), now, now],
        )?;
        Ok(())
    }

    /// Reads a kv entry as JSON.
    pub fn kv_get(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let stored: Option<String> = self
            .conn()
            .query_row("SELECT value FROM kv WHERE key = ?1", rusqlite::params![key], |row| {
                row.get(0)
            })
            .optional()?;
        stored.as_deref().map(decode_json).transpose()
    }

    /// Deletes a kv entry.
    pub fn kv_delete(&mut self, key: &str) -> Result<()> {
        self.execute("DELETE FROM kv WHERE key = ?1", rusqlite::params![key])?;
        Ok(())
    }

    /// Journal ids currently recorded (unordered).
    pub fn migration_ids(&self) -> Result<Vec<String>> {
        let mut statement = self.conn().prepare("SELECT id FROM migration")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Advances an aggregate's sequence counter monotonically, keeping the
    /// existing owner when the new one is `None` (`bus.ts:404-415` shape).
    pub fn event_sequence_upsert(
        &mut self,
        aggregate_id: &str,
        seq: i64,
        owner_id: Option<&str>,
    ) -> Result<()> {
        self.execute(
            "INSERT INTO event_sequence (aggregate_id, seq, owner_id) VALUES (?1, ?2, ?3)
             ON CONFLICT (aggregate_id) DO UPDATE SET
               seq = max(excluded.seq, event_sequence.seq),
               owner_id = coalesce(excluded.owner_id, event_sequence.owner_id)",
            rusqlite::params![aggregate_id, seq, owner_id],
        )?;
        Ok(())
    }

    /// Saved grants for a project (read back as `effect: "allow"` rules).
    pub fn permissions_for_project(&self, project_id: &str) -> Result<Vec<PermissionRow>> {
        let mut statement = self
            .conn()
            .prepare("SELECT * FROM permission WHERE project_id = ?1 ORDER BY action, resource")?;
        let rows = statement
            .query_map(rusqlite::params![project_id], PermissionRow::from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Sessions for a project, oldest first.
    pub fn sessions_for_project(&self, project_id: &str) -> Result<Vec<SessionRow>> {
        let mut statement = self
            .conn()
            .prepare("SELECT * FROM session_v2 WHERE project_id = ?1 ORDER BY time_created, id")?;
        let rows = statement
            .query_map(rusqlite::params![project_id], SessionRow::from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// A session's messages in durable order, optionally after `after_seq`.
    pub fn session_messages(
        &self,
        session_id: &str,
        after_seq: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SessionMessageRow>> {
        let mut statement = self.conn().prepare(
            "SELECT * FROM session_message WHERE session_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = statement
            .query_map(
                rusqlite::params![session_id, after_seq.unwrap_or(-1), limit],
                SessionMessageRow::from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// One aggregate's event page in sequence order (512/page upstream).
    pub fn events_page(
        &self,
        aggregate_id: &str,
        after_seq: Option<i64>,
        limit: i64,
    ) -> Result<Vec<EventRow>> {
        let mut statement = self.conn().prepare(
            "SELECT * FROM event WHERE aggregate_id = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = statement
            .query_map(
                rusqlite::params![aggregate_id, after_seq.unwrap_or(-1), limit],
                EventRow::from_row,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// Async conveniences on Database
// ---------------------------------------------------------------------------

impl Database {
    /// Upserts a kv entry.
    pub async fn kv_set(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        let key = key.to_string();
        let value = value.clone();
        self.transaction(move |tx| tx.kv_set(&key, &value)).await
    }

    /// Reads a kv entry.
    pub async fn kv_get(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let key = key.to_string();
        self.transaction(move |tx| tx.kv_get(&key)).await
    }

    /// Journal ids currently recorded.
    pub async fn migration_ids(&self) -> Result<Vec<String>> {
        self.transaction(|tx| tx.migration_ids()).await
    }

    /// Count of rows per opencode table (diagnostics; unordered map).
    pub async fn table_counts(&self) -> Result<Vec<(String, i64)>> {
        self.with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND substr(name, 1, 1) <> '_' ORDER BY name",
            )?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            drop(statement);
            let mut counts = Vec::new();
            for name in names {
                let count: i64 =
                    conn.query_row(&format!("SELECT count(*) FROM {name}"), [], |row| row.get(0))?;
                counts.push((name, count));
            }
            Ok(counts)
        })
        .await
    }
}
