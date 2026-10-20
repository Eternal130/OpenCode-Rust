//! Project / workspace resolution — port of `packages/core/src/project.ts`
//! (§3f of the core deep map).
//!
//! `resolve(cwd)`: walk up to the nearest `.git`/`.hg` marker (git preferred
//! within a directory). Git identity = remote-derived (`https`/scp URL →
//! lowercased host + path-minus-`.git`, hashed) ?? cached commondir id ??
//! first root commit; canonical directory = the main worktree. Hg identity =
//! cached id ?? first root node (`hg` shelled out, graceful when absent).
//! No VCS: identity hashes `"directory:<path>"`. Persistence goes through
//! the async [`ProjectStore`] trait — [`MemoryProjectStore`] for tests,
//! [`SqliteProjectStore`] over the `project`/`worktree` tables for
//! production (graph v2).

use crate::config::unpoison;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use sha1::{Digest as _, Sha1};

/// `Hash.fast` (util/hash.ts:4-7): SHA-1 hex (40 chars). DB-D1: the
/// project id format is v2's exact `Hash.fast` output — v2 databases
/// shared with (or imported into) the port must land on the SAME project
/// row per repo, never a SHA-256 twin row.
#[must_use]
pub fn hash_fast(input: &str) -> String {
    let digest = Sha1::digest(input.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// `Project.Vcs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vcs {
    Git { store: PathBuf },
    Hg { store: PathBuf },
}

impl Vcs {
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Vcs::Git { .. } => "git",
            Vcs::Hg { .. } => "hg",
        }
    }
}

/// `Project.Resolved`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Previously cached project id (from `<store>/opencode`), if any.
    pub previous: Option<String>,
    pub id: String,
    /// This checkout's directory.
    pub directory: PathBuf,
    /// The project's main worktree; may be another clone.
    pub canonical: PathBuf,
    pub vcs: Option<Vcs>,
}

/// Persisted project row (subset used by resolution).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub id: String,
    pub canonical: PathBuf,
    pub vcs: Option<String>,
}

/// `Project.Icon` (schema/project.ts:24-28) — optional triple.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectIcon {
    pub url: Option<String>,
    pub r#override: Option<String>,
    pub color: Option<String>,
}

/// `Project.Info` (schema/project.ts:40-49) — the full `project` row in
/// its wire shape. Event payloads (`project.updated`) and update
/// responses carry this, not the resolution subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub id: String,
    pub canonical: PathBuf,
    pub vcs: Option<String>,
    pub name: Option<String>,
    pub icon: Option<ProjectIcon>,
    /// Decoded `commands` JSON (`{start?}`), when set.
    pub commands: Option<serde_json::Value>,
    pub sandboxes: Vec<String>,
    pub time_created: i64,
    pub time_updated: i64,
    pub time_active: i64,
}

impl ProjectInfo {
    /// The v2 `Project.Info` wire JSON — optional members are ABSENT
    /// when unset (never explicit `null`; HTTP-H2 semantics).
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        let icon = self.icon.as_ref().map(|icon| {
            let mut value = json!({});
            if let Some(url) = &icon.url {
                value["url"] = json!(url);
            }
            if let Some(override_) = &icon.r#override {
                value["override"] = json!(override_);
            }
            if let Some(color) = &icon.color {
                value["color"] = json!(color);
            }
            value
        });
        let mut value = json!({
            "id": self.id,
            "canonical": self.canonical.to_string_lossy(),
            "time": {
                "created": self.time_created,
                "updated": self.time_updated,
                "active": self.time_active,
            },
            "sandboxes": self.sandboxes,
        });
        if let Some(vcs) = &self.vcs {
            value["vcs"] = json!(vcs);
        }
        if let Some(name) = &self.name {
            value["name"] = json!(name);
        }
        if let Some(icon) = icon {
            value["icon"] = icon;
        }
        if let Some(commands) = &self.commands {
            value["commands"] = commands.clone();
        }
        value
    }
}

/// `Project.UpdateInput` (schema/project.ts:52-58): `None` leaves a
/// column untouched; an empty string (or a `commands` object without a
/// non-empty `start`) CLEARS it (project.ts:219-226).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateInput {
    pub project_id: String,
    pub canonical: Option<PathBuf>,
    pub name: Option<String>,
    pub icon_override: Option<String>,
    pub icon_color: Option<String>,
    /// `Some` when the request carried `commands`: the inner value is
    /// `Some({start})` for a non-empty start, `None` to clear.
    pub commands: Option<Option<serde_json::Value>>,
}

/// Persisted worktree row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRow {
    pub project_id: String,
    pub directory: PathBuf,
    pub strategy: Option<String>,
}

/// `Worktree.Event.Resolved` payload (durable fact: directory → project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeResolved {
    pub project_id: String,
    pub directory: PathBuf,
    pub previous: String,
    /// Markerless directory-keyed projects nested under `directory`
    /// whose nearest VCS marker resolves to it — their sessions are
    /// re-owned by this event's project (project.ts:175-192).
    pub adopted: Option<Vec<String>>,
}

/// Persistence boundary for project resolution. Async since the sqlite
/// implementation shares the serialized database connection with the bus
/// and event machinery (`SqliteProjectStore`).
#[async_trait]
pub trait ProjectStore: Send + Sync {
    async fn project(&self, id: &str) -> Option<ProjectRow>;
    /// Insert-or-update: only the vcs label refreshes; the canonical
    /// directory is deliberately preserved (clones share the id).
    async fn upsert_project(&self, row: &ProjectRow);
    async fn reassign_canonical(&self, id: &str, canonical: &Path);
    async fn worktree(&self, project_id: &str, directory: &Path) -> Option<WorktreeRow>;
    async fn insert_worktree(&self, row: &WorktreeRow);
    async fn list_projects(&self) -> Vec<ProjectRow>;
    async fn list_worktrees(&self) -> Vec<WorktreeRow>;
    /// Full-row read (`Project.Info`) for event payloads and updates.
    async fn project_info(&self, id: &str) -> Option<ProjectInfo>;
    /// Markerless (`vcs IS NULL`) project rows whose canonical directory
    /// falls under `directory` (the string range `dir .. dir+"\uffff"`)
    /// — candidates for the adoption scan (project.ts:178-186).
    async fn markerless_projects_under(&self, directory: &Path) -> Vec<(String, PathBuf)> {
        let _ = directory;
        Vec::new()
    }
}

/// In-memory store (tests).
#[derive(Default)]
pub struct MemoryProjectStore {
    projects: std::sync::Mutex<Vec<ProjectRow>>,
    worktrees: std::sync::Mutex<Vec<WorktreeRow>>,
}

impl MemoryProjectStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
#[allow(
    clippy::unused_async,
    clippy::unused_async_trait_impl,
    reason = "async surface parity with the sqlite store; memory table is sync"
)]
impl ProjectStore for MemoryProjectStore {
    async fn project(&self, id: &str) -> Option<ProjectRow> {
        self.projects.lock().unwrap_or_else(unpoison).iter().find(|row| row.id == id).cloned()
    }

    async fn upsert_project(&self, row: &ProjectRow) {
        let mut projects = self.projects.lock().unwrap_or_else(unpoison);
        match projects.iter_mut().find(|existing| existing.id == row.id) {
            Some(existing) => existing.vcs.clone_from(&row.vcs),
            None => projects.push(row.clone()),
        }
    }

    async fn reassign_canonical(&self, id: &str, canonical: &Path) {
        let mut projects = self.projects.lock().unwrap_or_else(unpoison);
        if let Some(row) = projects.iter_mut().find(|row| row.id == id) {
            row.canonical = canonical.to_path_buf();
        }
    }

    async fn worktree(&self, project_id: &str, directory: &Path) -> Option<WorktreeRow> {
        self.worktrees
            .lock()
            .unwrap_or_else(unpoison)
            .iter()
            .find(|row| row.project_id == project_id && row.directory == directory)
            .cloned()
    }

    async fn insert_worktree(&self, row: &WorktreeRow) {
        let mut worktrees = self.worktrees.lock().unwrap_or_else(unpoison);
        if !worktrees.iter().any(|existing| {
            existing.project_id == row.project_id && existing.directory == row.directory
        }) {
            worktrees.push(row.clone());
        }
    }

    async fn list_projects(&self) -> Vec<ProjectRow> {
        self.projects.lock().unwrap_or_else(unpoison).clone()
    }

    async fn project_info(&self, id: &str) -> Option<ProjectInfo> {
        self.projects.lock().unwrap_or_else(unpoison).iter().find(|row| row.id == id).map(|row| {
            ProjectInfo {
                id: row.id.clone(),
                canonical: row.canonical.clone(),
                vcs: row.vcs.clone(),
                name: None,
                icon: None,
                commands: None,
                sandboxes: Vec::new(),
                time_created: 0,
                time_updated: 0,
                time_active: 0,
            }
        })
    }

    async fn list_worktrees(&self) -> Vec<WorktreeRow> {
        self.worktrees.lock().unwrap_or_else(unpoison).clone()
    }

    async fn markerless_projects_under(&self, directory: &Path) -> Vec<(String, PathBuf)> {
        let prefix = directory.to_string_lossy().into_owned();
        self.projects
            .lock()
            .unwrap_or_else(unpoison)
            .iter()
            .filter(|row| row.vcs.is_none())
            .filter(|row| {
                let worktree = row.canonical.to_string_lossy();
                worktree.starts_with(&prefix)
            })
            .map(|row| (row.id.clone(), row.canonical.clone()))
            .collect()
    }
}

// =============================================================================
// Sqlite-backed project store (graph v2, oracle gate 4 finding 5)
// =============================================================================

/// Sqlite-backed [`ProjectStore`] over the `project` / `worktree` tables
/// (schema per `database/schema.rs`; DDL in the bootstrap). The
/// deprecated-but-present `project_directory` table is readable for
/// legacy databases; v2's resolver writes only `worktree` rows
/// (`project.ts:193-199`).
///
/// Row mapping: the local [`ProjectRow::canonical`] is the `worktree`
/// column (`project/sql.ts` `upsertProject`), and the upsert mirrors v2 —
/// on id conflict only the `vcs` label refreshes (canonical preserved,
/// clones share the id).
pub struct SqliteProjectStore {
    db: crate::database::Database,
    /// DB-D2 rate limiter: last `activate` write per project id
    /// (`activated` map, project.ts:237).
    last_activated: std::sync::Mutex<std::collections::HashMap<String, i64>>,
    /// `ACTIVATE_INTERVAL` (project.ts:70) in ms.
    activate_interval_ms: i64,
}

/// `ACTIVATE_INTERVAL = 60_000` (project.ts:70).
pub const ACTIVATE_INTERVAL_MS: i64 = 60_000;

impl SqliteProjectStore {
    /// Store over an open database with the v2 activate interval.
    #[must_use]
    pub fn new(db: crate::database::Database) -> Self {
        Self::new_with_activate_interval(db, ACTIVATE_INTERVAL_MS)
    }

    /// Store with an explicit activate interval (the v2 constant in
    /// production; tests collapse it to exercise the two sides of the
    /// rate limit deterministically).
    #[must_use]
    pub fn new_with_activate_interval(db: crate::database::Database, interval_ms: i64) -> Self {
        Self {
            db,
            last_activated: std::sync::Mutex::new(std::collections::HashMap::new()),
            activate_interval_ms: interval_ms,
        }
    }

    fn db_worktree(row: &ProjectRow) -> Result<crate::database::schema::ProjectRow, String> {
        crate::database::schema::ProjectRow::new(
            row.id.clone(),
            &row.canonical.to_string_lossy(),
            row.vcs.as_deref(),
        )
        .map_err(|error| error.to_string())
    }

    /// `Project.activate` (project.ts:241-255): bump `time_active` to
    /// now, at most once per [`ACTIVATE_INTERVAL_MS`] per project, and
    /// return the updated row for the `project.updated` announcement.
    /// `None` = suppressed by the rate limit, or no such project. The
    /// bump deliberately keeps `time_updated` out (activity is not a
    /// metadata edit).
    pub async fn activate(&self, id: &str) -> Option<ProjectInfo> {
        let now = crate::database::migrations::now_ms();
        {
            let mut last = self.last_activated.lock().unwrap_or_else(unpoison);
            if now - last.get(id).copied().unwrap_or(0) < self.activate_interval_ms {
                return None;
            }
            last.insert(id.to_string(), now);
        }
        let id = id.to_string();
        self.db
            .transaction(move |tx| {
                let row = tx.conn().query_row(
                    "UPDATE project SET time_active = ?2, time_updated = time_updated \
                     WHERE id = ?1 \
                     RETURNING id, worktree, vcs, name, icon_url, icon_url_override, \
                     icon_color, commands, sandboxes, time_created, time_updated, time_active",
                    rusqlite::params![id, now],
                    read_project_info,
                );
                Ok(row.ok())
            })
            .await
            .ok()
            .flatten()
    }

    /// `Project.update` (project.ts:215-239, DB-D4): update
    /// canonical/name/icon/commands — `None` input fields stay
    /// untouched, empty strings (and a `commands` without a non-empty
    /// `start`) CLEAR the column — then return the REAL updated row
    /// (`Err` = `Project.NotFoundError`).
    pub async fn update(&self, input: &UpdateInput) -> Result<ProjectInfo, String> {
        let canonical = match &input.canonical {
            Some(path) => Some(
                crate::database::schema::absolute_storage(&path.to_string_lossy())
                    .map_err(|error| error.to_string())?,
            ),
            None => None,
        };
        let id = input.project_id.clone();
        let now = crate::database::migrations::now_ms();
        let mut assignments: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::new();
        // v2 drizzle `set`: an `undefined` field keeps the column (no set
        // key); a present value writes it, with the empty string CLEARING
        // to NULL (`input.name || null` — project.ts:219-226).
        let clear = |value: Option<String>| value.filter(|value| !value.is_empty());
        let mut push = |column: &str,
                        value: Option<String>,
                        params: &mut Vec<Box<dyn rusqlite::ToSql + Send>>| {
            params.push(Box::new(value));
            assignments.push(format!("{column} = ?{}", params.len()));
        };
        if let Some(canonical) = canonical {
            push("worktree", Some(canonical), &mut params);
        }
        if input.name.is_some() {
            push("name", clear(input.name.clone()), &mut params);
        }
        if input.icon_override.is_some() {
            push("icon_url_override", clear(input.icon_override.clone()), &mut params);
        }
        if input.icon_color.is_some() {
            push("icon_color", clear(input.icon_color.clone()), &mut params);
        }
        if let Some(commands) = &input.commands {
            let encoded = commands
                .as_ref()
                .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "{}".into()));
            push("commands", encoded, &mut params);
        }
        push("time_updated", Some(now.to_string()), &mut params);
        params.push(Box::new(id.clone()));
        let sql = format!(
            "UPDATE project SET {} WHERE id = ?{} \
             RETURNING id, worktree, vcs, name, icon_url, icon_url_override, \
             icon_color, commands, sandboxes, time_created, time_updated, time_active",
            assignments.join(", "),
            params.len()
        );
        self.db
            .transaction(move |tx| {
                let placeholders: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|value| value.as_ref() as &dyn rusqlite::ToSql).collect();
                let row = tx.conn().query_row(&sql, placeholders.as_slice(), read_project_info);
                match row {
                    Ok(info) => Ok(Some(info)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(error) => Err(crate::database::DbError::Sqlite(error)),
                }
            })
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("project not found: {id}"))
    }
}

/// Row reader building the full [`ProjectInfo`] (column order fixed by
/// the SELECT/RETURNING lists above).
fn read_project_info(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectInfo> {
    let icon_url: Option<String> = row.get(4)?;
    let icon_override: Option<String> = row.get(5)?;
    let icon_color: Option<String> = row.get(6)?;
    let commands: Option<String> = row.get(7)?;
    let sandboxes: String = row.get(8)?;
    Ok(ProjectInfo {
        id: row.get(0)?,
        canonical: PathBuf::from(row.get::<_, String>(1)?),
        vcs: row.get(2)?,
        name: row.get(3)?,
        icon: (icon_url.is_some() || icon_override.is_some() || icon_color.is_some())
            .then_some(ProjectIcon { url: icon_url, r#override: icon_override, color: icon_color }),
        commands: commands.as_deref().and_then(|raw| serde_json::from_str(raw).ok()),
        sandboxes: serde_json::from_str(&sandboxes).unwrap_or_default(),
        time_created: row.get(9)?,
        time_updated: row.get(10)?,
        time_active: row.get(11)?,
    })
}

#[async_trait]
impl ProjectStore for SqliteProjectStore {
    async fn project(&self, id: &str) -> Option<ProjectRow> {
        let id = id.to_string();
        self.db
            .transaction(move |tx| {
                let row = tx.conn().query_row(
                    "SELECT id, worktree, vcs FROM project WHERE id = ?1",
                    rusqlite::params![id],
                    |row| {
                        Ok(ProjectRow {
                            id: row.get(0)?,
                            canonical: PathBuf::from(row.get::<_, String>(1)?),
                            vcs: row.get(2)?,
                        })
                    },
                );
                Ok(row.ok())
            })
            .await
            .ok()
            .flatten()
    }

    async fn project_info(&self, id: &str) -> Option<ProjectInfo> {
        let id = id.to_string();
        self.db
            .transaction(move |tx| {
                let row = tx.conn().query_row(
                    "SELECT id, worktree, vcs, name, icon_url, icon_url_override, icon_color, \
                     commands, sandboxes, time_created, time_updated, time_active \
                     FROM project WHERE id = ?1",
                    rusqlite::params![id],
                    read_project_info,
                );
                Ok(row.ok())
            })
            .await
            .ok()
            .flatten()
    }

    async fn upsert_project(&self, row: &ProjectRow) {
        // Non-absolute canonical: degrade like v2's driver rejection.
        let Ok(db_row) = Self::db_worktree(row) else { return };
        let _ = self
            .db
            .transaction(move |tx| {
                // v2 upsertProject: on id conflict only the vcs label
                // refreshes (set when it differs or was null; a None vcs
                // clears a stale label). Canonical preserved.
                tx.execute(
                    "INSERT INTO project (id, worktree, vcs, name, icon_url, \
                     icon_url_override, icon_color, time_created, time_updated, \
                     time_initialized, time_active, sandboxes, commands) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
                     ON CONFLICT (id) DO UPDATE SET vcs = excluded.vcs \
                     WHERE project.vcs IS NOT excluded.vcs",
                    rusqlite::params![
                        db_row.id,
                        db_row.worktree,
                        db_row.vcs,
                        db_row.name,
                        db_row.icon_url,
                        db_row.icon_url_override,
                        db_row.icon_color,
                        db_row.time_created,
                        db_row.time_updated,
                        db_row.time_initialized,
                        db_row.time_active,
                        db_row.sandboxes,
                        db_row.commands,
                    ],
                )?;
                Ok(())
            })
            .await;
    }

    async fn reassign_canonical(&self, id: &str, canonical: &Path) {
        let id = id.to_string();
        let Ok(canonical) = crate::database::schema::absolute_storage(&canonical.to_string_lossy())
        else {
            return;
        };
        let now = crate::database::migrations::now_ms();
        let _ = self
            .db
            .transaction(move |tx| {
                tx.execute(
                    "UPDATE project SET worktree = ?2, time_updated = ?3 WHERE id = ?1",
                    rusqlite::params![id, canonical, now],
                )?;
                Ok(())
            })
            .await;
    }

    async fn worktree(&self, project_id: &str, directory: &Path) -> Option<WorktreeRow> {
        let project_id = project_id.to_string();
        let directory =
            crate::database::schema::directory_storage(&directory.to_string_lossy()).ok()?;
        self.db
            .transaction(move |tx| {
                let row = tx.conn().query_row(
                    "SELECT project_id, directory, strategy FROM worktree \
                     WHERE project_id = ?1 AND directory = ?2",
                    rusqlite::params![project_id, directory],
                    |row| {
                        Ok(WorktreeRow {
                            project_id: row.get(0)?,
                            directory: PathBuf::from(row.get::<_, String>(1)?),
                            strategy: row.get(2)?,
                        })
                    },
                );
                Ok(row.ok())
            })
            .await
            .ok()
            .flatten()
    }

    async fn insert_worktree(&self, row: &WorktreeRow) {
        let directory =
            crate::database::schema::directory_storage(&row.directory.to_string_lossy());
        let Ok(directory) = directory else { return };
        let project_id = row.project_id.clone();
        let strategy = row.strategy.clone();
        let now = crate::database::migrations::now_ms();
        let _ = self
            .db
            .transaction(move |tx| {
                // v2 commits the row with the Worktree.Resolved event
                // (onConflictDoNothing over the (project_id, directory) pk).
                tx.execute(
                    "INSERT OR IGNORE INTO worktree (project_id, directory, strategy, time_created) \
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![project_id, directory, strategy, now],
                )?;
                Ok(())
            })
            .await;
    }

    async fn list_projects(&self) -> Vec<ProjectRow> {
        self.db
            .transaction(|tx| {
                let mut statement =
                    tx.conn().prepare("SELECT id, worktree, vcs FROM project ORDER BY id")?;
                let rows = statement
                    .query_map([], |row| {
                        Ok(ProjectRow {
                            id: row.get(0)?,
                            canonical: PathBuf::from(row.get::<_, String>(1)?),
                            vcs: row.get(2)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .unwrap_or_default()
    }

    async fn list_worktrees(&self) -> Vec<WorktreeRow> {
        self.db
            .transaction(|tx| {
                let mut statement = tx
                    .conn()
                    .prepare("SELECT project_id, directory, strategy FROM worktree ORDER BY project_id, directory")?;
                let rows = statement
                    .query_map([], |row| {
                        Ok(WorktreeRow {
                            project_id: row.get(0)?,
                            directory: PathBuf::from(row.get::<_, String>(1)?),
                            strategy: row.get(2)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .unwrap_or_default()
    }

    async fn markerless_projects_under(&self, directory: &Path) -> Vec<(String, PathBuf)> {
        let prefix = crate::database::schema::absolute_storage(&directory.to_string_lossy())
            .unwrap_or_default();
        let upper = format!("{prefix}\u{ffff}");
        self.db
            .transaction(move |tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT id, worktree FROM project \
                     WHERE vcs IS NULL AND worktree >= ?1 AND worktree <= ?2",
                )?;
                let rows = statement
                    .query_map(rusqlite::params![prefix, upper], |row| {
                        Ok((row.get::<_, String>(0)?, PathBuf::from(row.get::<_, String>(1)?)))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .unwrap_or_default()
    }
}

impl SqliteProjectStore {
    /// Legacy `project_directory` rows (deprecated by `worktree`; kept for
    /// databases written by older versions).
    pub async fn list_project_directories(
        &self,
    ) -> Vec<(String, PathBuf, Option<String>, Option<String>)> {
        self.db
            .transaction(|tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT project_id, directory, type, strategy FROM project_directory \
                     ORDER BY project_id, directory",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            PathBuf::from(row.get::<_, String>(1)?),
                            row.get(2)?,
                            row.get(3)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .unwrap_or_default()
    }

    /// Insert-or-ignore a `workspace` binding row.
    pub async fn insert_workspace(&self, id: &str, provider: &str, binding: Option<&str>) {
        let id = id.to_string();
        let provider = provider.to_string();
        let binding = binding.map(str::to_string);
        let now = crate::database::migrations::now_ms();
        let _ = self
            .db
            .transaction(move |tx| {
                tx.execute(
                    "INSERT OR IGNORE INTO workspace (id, provider, binding, created_at, last_used_at) \
                     VALUES (?1, ?2, ?3, ?4, ?4)",
                    rusqlite::params![id, provider, binding, now],
                )?;
                Ok(())
            })
            .await;
    }

    /// `workspace` rows (`(id, provider, binding JSON, created, last used)`).
    pub async fn list_workspaces(&self) -> Vec<(String, String, Option<String>, i64, i64)> {
        self.db
            .transaction(|tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT id, provider, binding, created_at, last_used_at FROM workspace ORDER BY id",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .unwrap_or_default()
    }
}

/// Resolution events (bus integration lands later).
///
/// DB-D3: async because the production implementation publishes onto
/// the engine bus.
#[async_trait]
pub trait ProjectEvents: Send + Sync {
    async fn project_updated(&self, _row: &ProjectInfo) {}
    async fn worktree_resolved(&self, _event: &WorktreeResolved) {}
}

/// No-op events.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEvents;

#[async_trait]
#[allow(
    clippy::unused_async,
    clippy::unused_async_trait_impl,
    reason = "async surface parity with the bus-backed producer; no-op body"
)]
impl ProjectEvents for NoEvents {}

/// Bus-backed [`ProjectEvents`] (DB-D3): the production producer.
///
/// - `project_updated` → ephemeral `project.updated` (registry
///   [`registry::PROJECT_UPDATED`]) with the full `Project.Info` payload
///   (`fromRow(row)`, project.ts:60-61 + 129). v2's Project service is
///   global, so the publish carries no location — exactly like the ref.
/// - `worktree_resolved` → durable `worktree.resolved` (aggregate
///   `projectID`, version 1) with `{projectID, directory, previous,
///   adopted?}` — the session projector's re-own trigger
///   (session/projector.rs `project_worktree`).
pub struct BusProjectEvents {
    bus: crate::bus::Bus,
}

impl BusProjectEvents {
    #[must_use]
    pub fn new(bus: crate::bus::Bus) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl ProjectEvents for BusProjectEvents {
    async fn project_updated(&self, row: &ProjectInfo) {
        let Some(definition) = crate::bus::registry::by_type(crate::bus::registry::PROJECT_UPDATED)
        else {
            return;
        };
        let _ = self
            .bus
            .publish(definition, row.to_json(), crate::bus::PublishOptions::default())
            .await;
    }

    async fn worktree_resolved(&self, event: &WorktreeResolved) {
        let Some(definition) = crate::bus::registry::by_type("worktree.resolved") else {
            return;
        };
        let mut data = serde_json::json!({
            "projectID": event.project_id,
            "directory": event.directory.to_string_lossy(),
            "previous": event.previous,
        });
        if let Some(adopted) = &event.adopted {
            data["adopted"] = serde_json::json!(adopted);
        }
        let _ = self.bus.publish(definition, data, crate::bus::PublishOptions::default()).await;
    }
}

/// `Project.root` — nearest `.git`/`.hg` ancestor directory (filesystem only;
/// permission checks rely on this never executing VCS commands).
#[must_use]
pub fn root(input: &Path) -> Option<PathBuf> {
    let resolved = crate::config::resolve_path(input);
    let mut current = resolved.as_path();
    loop {
        for marker in [".git", ".hg"] {
            if current.join(marker).exists() {
                return Some(current.to_path_buf());
            }
        }
        current = current.parent()?;
    }
}

/// The project resolver.
pub struct Resolver {
    store: Arc<dyn ProjectStore>,
    events: Arc<dyn ProjectEvents>,
}

impl Resolver {
    #[must_use]
    pub fn new(store: Arc<dyn ProjectStore>, events: Arc<dyn ProjectEvents>) -> Self {
        Self { store, events }
    }

    /// Resolves and persists the owning project for `input`.
    pub async fn resolve(&self, input: &Path) -> Resolved {
        let directory = crate::config::resolve_path(input);
        let marker = walk_to_marker(&directory);
        if let Some((dir, _)) = marker.as_ref().filter(|(_, marker)| *marker == ".git") {
            if let Some(resolved) = Self::resolve_git(dir) {
                return self.persist(resolved).await;
            }
        }
        if let Some((dir, _)) = marker.filter(|(_, marker)| *marker == ".hg") {
            let store = dir.join(".hg");
            let previous = cached_id(&store);
            let id = previous
                .clone()
                .or_else(|| hg_root_node(&dir))
                .unwrap_or_else(|| "global".to_string());
            return self
                .persist(Resolved {
                    previous,
                    id,
                    directory: dir.clone(),
                    canonical: dir,
                    vcs: Some(Vcs::Hg { store }),
                })
                .await;
        }
        let id = hash_fast(&format!("directory:{}", directory.display()));
        self.persist(Resolved {
            previous: None,
            id,
            directory: directory.clone(),
            canonical: directory,
            vcs: None,
        })
        .await
    }

    fn resolve_git(worktree: &Path) -> Option<Resolved> {
        let repo = git2::Repository::open(worktree).ok()?;
        let common_directory = common_directory(&repo);
        let git_directory = repo.path().to_path_buf();
        let workdir = repo.workdir()?.to_path_buf();
        let workdir = crate::config::resolve_path(&workdir);

        let previous = cached_id(&common_directory);
        let id = remote_id(&repo)
            .or(previous.clone())
            .or_else(|| root_commit_id(&repo))
            .unwrap_or_else(|| "global".to_string());

        // Canonical = the main worktree: the checkout owning the common dir.
        let canonical = if git_directory == common_directory {
            workdir.clone()
        } else {
            common_directory.parent().map_or_else(|| workdir.clone(), crate::config::resolve_path)
        };

        Some(Resolved {
            previous,
            id,
            directory: workdir,
            canonical,
            vcs: Some(Vcs::Git { store: common_directory }),
        })
    }

    async fn persist(&self, project: Resolved) -> Resolved {
        let previous_canonical = self.store.project(&project.id).await.map(|row| row.canonical);
        self.store
            .upsert_project(&ProjectRow {
                id: project.id.clone(),
                canonical: project.canonical.clone(),
                vcs: project.vcs.as_ref().map(|vcs| vcs.kind().to_string()),
            })
            .await;
        // Clones share a project id; only replace a canonical directory that
        // is gone.
        if let Some(previous) = &previous_canonical {
            if *previous != project.canonical && !previous.exists() {
                self.store.reassign_canonical(&project.id, &project.canonical).await;
                if let Some(row) = self.store.project_info(&project.id).await {
                    self.events.project_updated(&row).await;
                }
            }
        }
        let Some(vcs) = &project.vcs else {
            return project;
        };
        let mut directories = vec![(project.canonical.clone(), None)];
        if project.directory != project.canonical {
            directories.push((
                project.directory.clone(),
                (vcs.kind() == "git").then(|| "git".to_string()),
            ));
        }
        for (directory, strategy) in directories {
            if self.store.worktree(&project.id, &directory).await.is_some() {
                continue;
            }
            let resolved_directory = crate::config::resolve_path(&directory);
            self.store
                .insert_worktree(&WorktreeRow {
                    project_id: project.id.clone(),
                    directory: resolved_directory.clone(),
                    strategy,
                })
                .await;
            // Adoption scan (project.ts:175-192): markerless
            // directory-keyed projects nested under the newly resolved
            // directory whose nearest VCS marker IS this directory's
            // marker — their sessions get re-owned by the event below.
            let adopted: Vec<String> = self
                .store
                .markerless_projects_under(&resolved_directory)
                .await
                .into_iter()
                .filter(|(id, candidate)| {
                    id != &project.id
                        && contains_path(&resolved_directory, candidate)
                        && marker_resolves_to(candidate, &resolved_directory)
                })
                .map(|(id, _)| id)
                .collect();
            self.events
                .worktree_resolved(&WorktreeResolved {
                    project_id: project.id.clone(),
                    directory,
                    previous: project.previous.clone().unwrap_or_else(|| "global".to_string()),
                    adopted: (!adopted.is_empty()).then_some(adopted),
                })
                .await;
        }
        project
    }
}

/// `FSUtil.contains` (util/fs-util.ts:280-283): equal or nested under.
fn contains_path(parent: &Path, child: &Path) -> bool {
    child.starts_with(parent)
}

/// project.ts:184-190: walking up from `candidate` (stop inclusive at
/// `directory`), the first `.git`/`.hg` marker sits in `directory`
/// itself — the candidate has no nearer repository of its own.
fn marker_resolves_to(candidate: &Path, directory: &Path) -> bool {
    let mut current = candidate.to_path_buf();
    loop {
        for marker in [".git", ".hg"] {
            if current.join(marker).exists() {
                return crate::config::resolve_path(&current) == directory;
            }
        }
        if current == directory {
            return false;
        }
        let Some(parent) = current.parent() else {
            return false;
        };
        current = parent.to_path_buf();
    }
}

/// Walks up to the nearest `.git`/`.hg` marker (git preferred per directory).
fn walk_to_marker(start: &Path) -> Option<(PathBuf, &'static str)> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some((current.clone(), ".git"));
        }
        if current.join(".hg").exists() {
            return Some((current.clone(), ".hg"));
        }
        current = current.parent()?.to_path_buf();
    }
}

/// `<repo>/.git` for main worktrees, `<main>/.git/` for linked ones
/// (git2 0.19 does not expose `git_repository_commondir`).
fn common_directory(repo: &git2::Repository) -> PathBuf {
    let path = repo.path().to_path_buf();
    let text = path.to_string_lossy();
    if let Some(index) = text.rfind("/worktrees/") {
        return PathBuf::from(&text[..=index]);
    }
    path
}

/// Cached project id stored at `<store>/opencode`.
fn cached_id(store: &Path) -> Option<String> {
    let value = std::fs::read_to_string(store.join("opencode")).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Remote-derived identity: normalized `git-remote:<host>/<path>` hash.
fn remote_id(repo: &git2::Repository) -> Option<String> {
    let origin = repo.find_remote("origin").ok()?;
    let url = origin.url()?;
    let normalized = normalize_remote_url(url)?;
    Some(hash_fast(&format!("git-remote:{normalized}")))
}

/// `https://Host/path.git` and `user@host:path` (scp) forms normalize to
/// `host/path` (host lowercased, `.git` suffix and slashes trimmed).
fn normalize_remote_url(input: &str) -> Option<String> {
    let value = input.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("file://") {
        let _ = rest;
        return None;
    }
    // URL form: scheme://[user@]host[:port]/path
    if let Some((scheme, rest)) = value.split_once("://") {
        if scheme.eq_ignore_ascii_case("http")
            || scheme.eq_ignore_ascii_case("https")
            || scheme.eq_ignore_ascii_case("ssh")
            || scheme.eq_ignore_ascii_case("git")
        {
            let authority = rest.split(['/', ':']).next().unwrap_or_default();
            let host = authority.rsplit('@').next().unwrap_or_default();
            let path = rest.find('/').map(|index| &rest[index + 1..]).unwrap_or_default();
            return parts(host, path);
        }
        return None;
    }
    // scp form: [user@]host:path
    let scp = value.strip_suffix(".git").unwrap_or(value);
    if let Some((authority, path)) = scp.split_once(':') {
        if !authority.contains('/') && !path.starts_with('/') && !authority.starts_with('.') {
            let host = authority.rsplit('@').next().unwrap_or_default();
            return parts(host, path);
        }
    }
    None
}

fn parts(host: &str, name: &str) -> Option<String> {
    let pathname =
        name.trim_start_matches('/').trim_end_matches('/').strip_suffix(".git").map_or_else(
            || name.trim_start_matches('/').trim_end_matches('/'),
            |stripped| stripped.trim_end_matches('/'),
        );
    if host.is_empty() || pathname.is_empty() {
        return None;
    }
    Some(format!("{}/{}", host.to_lowercase(), pathname))
}

/// First root commit (sorted, like `rev-list --max-parents=0 HEAD` + sort).
fn root_commit_id(repo: &git2::Repository) -> Option<String> {
    let head = repo.head().ok()?;
    let target = head.target()?;
    let mut walk = repo.revwalk().ok()?;
    walk.push(target).ok()?;
    let mut roots: Vec<String> = Vec::new();
    for oid in walk {
        let Ok(oid) = oid else { continue };
        let commit = repo.find_commit(oid).ok()?;
        if commit.parent_count() == 0 {
            roots.push(commit.id().to_string());
        }
    }
    roots.sort();
    roots.into_iter().next()
}

/// `hg log -r roots(all()) -T {node}\n` — first node; `None` when `hg` is
/// absent or the repo is unreadable.
fn hg_root_node(worktree: &Path) -> Option<String> {
    let output = Command::new("hg")
        .args(["log", "-r", "roots(all())", "-T", "{node}\n"])
        .current_dir(worktree)
        .env("HGPLAIN", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut nodes: Vec<&str> =
        text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    nodes.sort_unstable();
    nodes.first().map(|node| (*node).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("oc-core-config-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn init_repo(dir: &Path, commit: bool, remote: Option<&str>) {
        let repo = git2::Repository::init(dir).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.email", "test@opencode.test").unwrap();
        config.set_str("user.name", "Test").unwrap();
        if let Some(remote) = remote {
            repo.remote("origin", remote).unwrap();
        }
        if commit {
            let signature = git2::Signature::now("Test", "test@opencode.test").unwrap();
            let tree_id = {
                let mut index = repo.index().unwrap();
                let path = dir.join("README.md");
                fs::write(path, "readme").unwrap();
                index.add_path(Path::new("README.md")).unwrap();
                index.write_tree().unwrap()
            };
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &signature, &signature, "root", &tree, &[]).unwrap();
        }
    }

    fn resolver() -> Resolver {
        Resolver::new(Arc::new(MemoryProjectStore::new()), Arc::new(NoEvents))
    }

    fn remote_identity(normalized: &str) -> String {
        hash_fast(&format!("git-remote:{normalized}"))
    }

    #[tokio::test]
    async fn config_project_normalize_remote_urls() {
        assert_eq!(
            normalize_remote_url("https://github.com/owner/repo.git").as_deref(),
            Some("github.com/owner/repo")
        );
        assert_eq!(
            normalize_remote_url("git@github.com:owner/repo.git").as_deref(),
            Some("github.com/owner/repo")
        );
        assert_eq!(
            normalize_remote_url("git@github.com:Acme/App.git").as_deref(),
            Some("github.com/Acme/App")
        );
        assert_eq!(
            normalize_remote_url("ssh://git@GitHub.com:22/owner/repo").as_deref(),
            Some("github.com/owner/repo")
        );
        assert_eq!(normalize_remote_url("file:///tmp/repo"), None);
        assert_eq!(normalize_remote_url(""), None);
        assert_eq!(normalize_remote_url("/local/path"), None);
    }

    #[tokio::test]
    async fn config_project_markerless_directory_identity_is_deterministic() {
        let dir = tempdir("project-markerless");
        let nested = dir.join("notes").join("drafts");
        fs::create_dir_all(&nested).unwrap();
        let resolver = resolver();
        let result = resolver.resolve(&dir).await;
        let repeated = resolver.resolve(&dir.join(".")).await;
        let child = resolver.resolve(&nested).await;
        assert_ne!(result.id, "global");
        assert_eq!(repeated.id, result.id);
        assert_ne!(child.id, result.id);
        assert_eq!(result.directory, crate::config::resolve_path(&dir));
        assert_eq!(result.canonical, result.directory);
        assert_eq!(result.previous, None);
        assert_eq!(result.vcs, None);
    }

    #[tokio::test]
    async fn config_project_remote_identity_wins_with_https_scp_equivalence() {
        let ssh = tempdir("project-remote-ssh");
        let https = tempdir("project-remote-https");
        init_repo(&ssh, true, Some("git@github.com:owner/repo.git"));
        init_repo(&https, true, Some("https://github.com/owner/repo.git"));
        let resolver = resolver();
        let a = resolver.resolve(&ssh).await;
        let b = resolver.resolve(&https).await;
        assert_eq!(a.id, remote_identity("github.com/owner/repo"));
        assert_eq!(b.id, a.id);
        assert_eq!(a.vcs.as_ref().map(Vcs::kind), Some("git"));
    }

    #[tokio::test]
    async fn config_project_falls_back_to_root_commit_without_remote() {
        let dir = tempdir("project-root-commit");
        init_repo(&dir, true, None);
        let resolver = resolver();
        let result = resolver.resolve(&dir).await;
        // Same oid the git CLI reports for `rev-list --max-parents=0 HEAD`.
        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(result.id, repo.head().unwrap().target().unwrap().to_string());
        assert_eq!(result.directory, crate::config::resolve_path(&dir));
        assert_eq!(result.canonical, result.directory);
    }

    #[tokio::test]
    async fn config_project_file_remote_ignored() {
        let dir = tempdir("project-file-remote");
        init_repo(&dir, true, Some(&format!("file://{}", dir.display())));
        let result = resolver().resolve(&dir).await;
        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(result.id, repo.head().unwrap().target().unwrap().to_string());
    }

    #[tokio::test]
    async fn config_project_global_for_empty_repo() {
        let dir = tempdir("project-empty-repo");
        init_repo(&dir, false, None);
        let result = resolver().resolve(&dir).await;
        assert_eq!(result.id, "global");
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("git"));
    }

    #[tokio::test]
    async fn config_project_cached_commondir_id_reported_as_previous() {
        let dir = tempdir("project-cached");
        init_repo(&dir, true, Some("git@github.com:owner/repo.git"));
        let store = common_directory(&git2::Repository::open(&dir).unwrap());
        fs::write(store.join("opencode"), "old-id\n").unwrap();
        let result = resolver().resolve(&dir).await;
        assert_eq!(result.previous.as_deref(), Some("old-id"));
        assert_eq!(result.id, remote_identity("github.com/owner/repo"));
        // Resolution never writes the cache.
        assert_eq!(fs::read_to_string(store.join("opencode")).unwrap(), "old-id\n");
    }

    #[tokio::test]
    async fn config_project_resolves_from_nested_directories() {
        let dir = tempdir("project-nested");
        init_repo(&dir, true, None);
        let nested = dir.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        let result = resolver().resolve(&nested).await;
        assert_eq!(result.directory, crate::config::resolve_path(&dir));
    }

    #[tokio::test]
    async fn config_project_git_preferred_over_hg_in_same_dir() {
        let dir = tempdir("project-git-hg");
        init_repo(&dir, true, None);
        fs::create_dir_all(dir.join(".hg")).unwrap();
        let result = resolver().resolve(&dir).await;
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("git"));
    }

    #[tokio::test]
    async fn config_project_nearest_marker_wins() {
        let dir = tempdir("project-nearest");
        let nested = dir.join("nested");
        init_repo(&dir, true, None);
        fs::create_dir_all(nested.join(".hg")).unwrap();
        fs::create_dir_all(nested.join("app")).unwrap();
        let result = resolver().resolve(&nested.join("app")).await;
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("hg"));
        assert_eq!(result.directory, crate::config::resolve_path(&nested));
    }

    #[tokio::test]
    async fn config_project_unreadable_hg_falls_back_to_global() {
        let dir = tempdir("project-hg-global");
        fs::create_dir_all(dir.join(".hg")).unwrap();
        let result = resolver().resolve(&dir).await;
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("hg"));
        assert_eq!(result.id, "global");
    }

    #[tokio::test]
    async fn config_project_persists_project_and_worktree_rows() {
        struct Recorder {
            resolved: std::sync::Mutex<Vec<WorktreeResolved>>,
        }
        #[async_trait]
        impl ProjectEvents for Recorder {
            async fn worktree_resolved(&self, event: &WorktreeResolved) {
                self.resolved.lock().unwrap().push(event.clone());
            }
        }
        let dir = tempdir("project-persist");
        init_repo(&dir, true, Some("git@github.com:owner/repo.git"));
        let store = Arc::new(MemoryProjectStore::new());
        let events = Arc::new(Recorder { resolved: std::sync::Mutex::new(Vec::new()) });
        let resolver = Resolver::new(store.clone(), events.clone());
        let first = resolver.resolve(&dir).await;
        let id = remote_identity("github.com/owner/repo");
        assert_eq!(first.id, id);
        assert_eq!(store.list_projects().await.len(), 1);
        assert_eq!(store.list_worktrees().await.len(), 1);
        assert_eq!(store.list_worktrees().await[0].directory, crate::config::resolve_path(&dir));
        // Repeated resolution publishes nothing new.
        let _ = resolver.resolve(&dir).await;
        assert_eq!(events.resolved.lock().unwrap().len(), 1);
        assert_eq!(store.list_projects().await.len(), 1);
    }

    #[tokio::test]
    async fn config_project_repoints_canonical_only_when_stored_one_vanished() {
        struct Updated {
            count: std::sync::Mutex<usize>,
        }
        #[async_trait]
        impl ProjectEvents for Updated {
            async fn project_updated(&self, _row: &ProjectInfo) {
                *self.count.lock().unwrap() += 1;
            }
        }
        let dir = tempdir("project-repoint");
        let before = dir.join("before");
        let after = dir.join("after");
        fs::create_dir_all(&before).unwrap();
        init_repo(&before, true, Some("git@github.com:owner/repo.git"));
        let store = Arc::new(MemoryProjectStore::new());
        let events = Arc::new(Updated { count: std::sync::Mutex::new(0) });
        let resolver = Resolver::new(store.clone(), events.clone());
        let initial = resolver.resolve(&before).await;
        assert_eq!(*events.count.lock().unwrap(), 0);

        // Rename the canonical checkout and resolve the new path (same repo:
        // the remote, hence the identity, is unchanged by the rename).
        fs::rename(&before, &after).unwrap();
        let renamed = resolver.resolve(&after).await;
        assert_eq!(renamed.id, initial.id);
        assert_eq!(renamed.canonical, crate::config::resolve_path(&after));
        assert_eq!(*events.count.lock().unwrap(), 1, "canonical re-point announced");
        assert_eq!(
            store.project(&initial.id).await.unwrap().canonical,
            crate::config::resolve_path(&after)
        );
    }

    #[tokio::test]
    async fn config_project_root_finds_nearest_vcs_ancestor() {
        let dir = tempdir("project-root-fn");
        let repo = dir.join("repo");
        init_repo(&repo, true, None);
        let nested = repo.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(root(&nested), Some(crate::config::resolve_path(&repo)));
        assert_eq!(root(&dir), None);
    }

    // ------------------------------------------------- sqlite store

    use crate::database::Database;

    async fn sqlite_dir(tag: &str) -> (crate::database::Database, PathBuf) {
        let root = std::env::temp_dir()
            .join("oc-core-config-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let db = Database::open_file(root.join("opencode.db")).await.unwrap();
        (db, root)
    }

    #[tokio::test]
    async fn project_sql_store_upsert_preserves_canonical_and_refreshes_vcs() {
        let (db, _root) = sqlite_dir("project-sql-upsert").await;
        let store = SqliteProjectStore::new(db);
        let id = remote_identity("github.com/owner/repo");
        store
            .upsert_project(&ProjectRow {
                id: id.clone(),
                canonical: PathBuf::from("/gone/checkout"),
                vcs: Some("git".into()),
            })
            .await;
        // Clone shares the id: canonical preserved, vcs refreshed only.
        store
            .upsert_project(&ProjectRow {
                id: id.clone(),
                canonical: PathBuf::from("/new/checkout"),
                vcs: Some("git".into()),
            })
            .await;
        let row = store.project(&id).await.expect("row");
        assert_eq!(row.canonical, PathBuf::from("/gone/checkout"), "canonical preserved");
        assert_eq!(row.vcs.as_deref(), Some("git"));
        assert_eq!(store.list_projects().await.len(), 1);

        // A vcs label change refreshes; explicit reassign moves the canonical.
        store
            .upsert_project(&ProjectRow { id: id.clone(), canonical: row.canonical, vcs: None })
            .await;
        assert_eq!(store.project(&id).await.unwrap().vcs, None, "stale label cleared");
        store.reassign_canonical(&id, Path::new("/moved")).await;
        assert_eq!(store.project(&id).await.unwrap().canonical, PathBuf::from("/moved"));
    }

    #[tokio::test]
    async fn project_sql_store_worktree_rows_and_legacy_directories() {
        let (db, _root) = sqlite_dir("project-sql-worktree").await;
        let store = SqliteProjectStore::new(db);
        store
            .upsert_project(&ProjectRow {
                id: "p1".into(),
                canonical: PathBuf::from("/repo"),
                vcs: Some("git".into()),
            })
            .await;
        store
            .insert_worktree(&WorktreeRow {
                project_id: "p1".into(),
                directory: crate::config::resolve_path(&PathBuf::from("/repo")),
                strategy: None,
            })
            .await;
        // Conflict-do-nothing over the (project_id, directory) pk.
        store
            .insert_worktree(&WorktreeRow {
                project_id: "p1".into(),
                directory: crate::config::resolve_path(&PathBuf::from("/repo")),
                strategy: Some("git".into()),
            })
            .await;
        assert!(store
            .worktree("p1", &crate::config::resolve_path(&PathBuf::from("/repo")))
            .await
            .is_some());
        let worktrees = store.list_worktrees().await;
        assert_eq!(worktrees.len(), 1, "duplicate insert ignored");
        assert_eq!(worktrees[0].strategy, None, "first row kept");
        assert!(store.worktree("p1", Path::new("/elsewhere")).await.is_none());
        assert!(store.list_project_directories().await.is_empty(), "legacy table unwritten");
    }

    #[tokio::test]
    async fn project_sql_store_workspace_rows() {
        let (db, _root) = sqlite_dir("project-sql-workspace").await;
        let store = SqliteProjectStore::new(db);
        store.insert_workspace("wrk_1", "driver", Some(r#"{"kind":"x"}"#)).await;
        store.insert_workspace("wrk_1", "driver", None).await; // ignored
        let rows = store.list_workspaces().await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "wrk_1");
        assert_eq!(rows[0].2.as_deref(), Some(r#"{"kind":"x"}"#));
    }

    #[tokio::test]
    async fn project_sql_store_survives_restart() {
        let (db, root) = sqlite_dir("project-sql-restart").await;
        let store = SqliteProjectStore::new(db.clone());
        store
            .upsert_project(&ProjectRow {
                id: "p1".into(),
                canonical: PathBuf::from("/repo"),
                vcs: Some("git".into()),
            })
            .await;
        store
            .insert_worktree(&WorktreeRow {
                project_id: "p1".into(),
                directory: crate::config::resolve_path(&PathBuf::from("/repo")),
                strategy: None,
            })
            .await;
        drop(store);
        drop(db);

        let db = Database::open_file(root.join("opencode.db")).await.unwrap();
        let store = SqliteProjectStore::new(db);
        assert_eq!(store.project("p1").await.unwrap().vcs.as_deref(), Some("git"));
        assert_eq!(store.list_worktrees().await.len(), 1);
        // The resolver drives the sqlite store end-to-end and persists
        // across a restart.
        let dir = tempdir("project-sql-resolver");
        init_repo(&dir, true, Some("git@github.com:owner/repo.git"));
        let resolver = Resolver::new(Arc::new(store), Arc::new(NoEvents));
        let outcome = resolver.resolve(&dir).await;
        assert_eq!(outcome.id, remote_identity("github.com/owner/repo"));
    }
}
