//! SQLite database service.
//!
//! Port of `packages/core/src/database/database.ts` + the vendored
//! effect/drizzle bridge's transaction semantics (`core-deep-map.md` §1b, §1e):
//!
//! - One `rusqlite::Connection` serialized by a `parking_lot::Semaphore(1)`
//!   gate; every operation (and every transaction, which holds the permit for
//!   its whole duration) runs on `tokio::task::spawn_blocking`.
//! - Pragmas at open: `journal_mode=WAL` (also applied at open before the
//!   others, like the node driver), `synchronous=NORMAL`,
//!   `busy_timeout=5000`, `cache_size=-64000`, a one-shot
//!   `wal_checkpoint(PASSIVE)` at boot, and `foreign_keys=ON`.
//! - The database file (and `-wal`/`-shm` sidecars) is created synchronously
//!   with mode `0600` and re-`chmod`ed `0600` before open (ENOENT on sidecars
//!   tolerated).
//! - Migrations hold a per-file-path bootstrap semaphore for the whole apply;
//!   `:memory:` databases get a private one.
//! - Transactions: top-level `BEGIN IMMEDIATE` by default (`DEFERRED`
//!   available), nesting via `SAVEPOINT oc_core_${n}`, and an explicit
//!   `ROLLBACK` when `COMMIT` fails (SQLite keeps a deferred transaction open
//!   after a constraint-commit failure).
//!
//! `:memory:` is fully supported with a private semaphore.

mod bootstrap;
pub mod error;
pub mod migrations;
pub mod path;
pub mod schema;
#[cfg(test)]
mod tests;
pub mod v1_migration;

pub use error::{DbError, Result};
pub use migrations::{ApplyReport, Migration, MigrationOptions, MIGRATIONS};
pub use path::{engine_location, DbLocation};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::sync::LazyLock;

/// Top-level transaction start mode. The vendored bridge defaults its drizzle
/// transactions to `deferred`, but every opencode call site passes
/// `immediate`, so the Rust port makes immediate the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Begin {
    /// `BEGIN DEFERRED`.
    Deferred,
    /// `BEGIN IMMEDIATE` (default).
    Immediate,
}

impl Begin {
    const fn statement(self) -> &'static str {
        match self {
            Begin::Deferred => "BEGIN DEFERRED",
            Begin::Immediate => "BEGIN IMMEDIATE",
        }
    }
}

struct Shared {
    conn: Mutex<Connection>,
    /// Binary permit (acquired by every operation; held by transactions for
    /// their whole duration). `parking_lot` 0.12 dropped its `Semaphore`; an
    /// uncontended `Mutex<()>` is the same fair blocking permit.
    gate: Mutex<()>,
}

/// A handle to one serialized SQLite connection.
#[derive(Clone)]
pub struct Database {
    shared: Arc<Shared>,
    path: Option<PathBuf>,
    report: Arc<Mutex<Option<ApplyReport>>>,
}

/// Per-file bootstrap locks (absolute path → permit), mirroring
/// `database.ts:57-65`. `:memory:` never appears here.
static FILE_LOCKS: LazyLock<DashMap<PathBuf, Arc<Mutex<()>>>> =
    LazyLock::new(|| DashMap::with_capacity(4));

fn file_lock(path: &Path) -> Arc<Mutex<()>> {
    FILE_LOCKS.entry(path.to_path_buf()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

impl Database {
    /// Opens a private in-memory database (bootstrapped).
    pub async fn open_in_memory() -> Result<Self> {
        Self::open_location(DbLocation::Memory, MigrationOptions::default()).await
    }

    /// Opens (creating if needed) a file database and applies migrations.
    pub async fn open_file(path: impl Into<PathBuf>) -> Result<Self> {
        Self::open_location(DbLocation::File(path.into()), MigrationOptions::default()).await
    }

    /// Opens a file database with explicit migration options.
    pub async fn open_file_with(
        path: impl Into<PathBuf>,
        options: MigrationOptions,
    ) -> Result<Self> {
        Self::open_location(DbLocation::File(path.into()), options).await
    }

    /// Opens the default database for this process (`Global.data` + the
    /// channel scheme, `OPENCODE_DB` honored).
    pub async fn open_default() -> Result<Self> {
        Self::open_location(path::default_location(), MigrationOptions::with_global_data()).await
    }

    /// Opens a database at [`DbLocation`], applying all boot-time behavior:
    /// owner-restricted file creation, pragmas, then migrations under the
    /// per-file bootstrap lock.
    pub async fn open_location(location: DbLocation, options: MigrationOptions) -> Result<Self> {
        let path = match &location {
            DbLocation::Memory => None,
            DbLocation::File(path) => Some(path.clone()),
        };
        let (shared, report) = tokio::task::spawn_blocking(move || {
            let memory = matches!(location, DbLocation::Memory);
            if let DbLocation::File(path) = &location {
                restrict_to_owner(path)?;
            }
            let mut conn = if memory {
                Connection::open_in_memory()?
            } else {
                let DbLocation::File(path) = &location else { unreachable!() };
                Connection::open(path)?
            };
            apply_pragmas(&mut conn, memory)?;

            let shared = Arc::new(Shared { conn: Mutex::new(conn), gate: Mutex::new(()) });
            // Whole-migration apply holds both the per-file bootstrap lock and
            // the connection gate (the TS layer holds the connection semaphore
            // across DatabaseMigration.apply).
            let bootstrap_lock = if memory {
                Arc::new(Mutex::new(()))
            } else {
                let DbLocation::File(path) = &location else { unreachable!() };
                file_lock(path)
            };
            let bootstrap_permit = bootstrap_lock.lock();
            let gate_permit = shared.gate.lock();
            let report = {
                let mut conn = shared.conn.lock();
                migrations::apply(&mut conn, &options)?
            };
            drop(gate_permit);
            drop(bootstrap_permit);
            Ok::<_, DbError>((shared, report))
        })
        .await
        .map_err(DbError::Join)??;
        Ok(Self { shared, path, report: Arc::new(Mutex::new(Some(report))) })
    }

    /// The file backing this database (`None` for `:memory:`).
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The migration report from open (first caller consumes it).
    #[must_use]
    pub fn apply_report(&self) -> Option<ApplyReport> {
        self.report.lock().take()
    }

    /// Runs one closure with exclusive access to the raw connection on a
    /// blocking thread.
    pub async fn with_conn<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let shared = Arc::clone(&self.shared);
        tokio::task::spawn_blocking(move || {
            let _permit = shared.gate.lock();
            let mut conn = shared.conn.lock();
            f(&mut conn)
        })
        .await
        .map_err(DbError::Join)?
    }

    /// Runs a transaction with `BEGIN IMMEDIATE` (the default).
    pub async fn transaction<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.transaction_with(Begin::Immediate, f).await
    }

    /// Runs a transaction with `BEGIN DEFERRED`.
    pub async fn transaction_deferred<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        self.transaction_with(Begin::Deferred, f).await
    }

    /// Runs a transaction with an explicit begin mode.
    pub async fn transaction_with<T, F>(&self, begin: Begin, f: F) -> Result<T>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let shared = Arc::clone(&self.shared);
        tokio::task::spawn_blocking(move || {
            let _permit = shared.gate.lock();
            let mut conn = shared.conn.lock();
            run_top_tx(&mut conn, begin, f)
        })
        .await
        .map_err(DbError::Join)?
    }
}

/// Runs a top-level transaction over an owned connection handle.
///
/// On `COMMIT` failure the transaction is rolled back explicitly before the
/// error propagates: SQLite keeps a (deferred) transaction open after a
/// constraint-commit failure, and an implicit rollback would leave the
/// connection stuck inside a transaction (`drizzle/effect-sqlite/session.ts:177-186`).
pub(crate) fn run_top_tx<T, F>(conn: &mut Connection, begin: Begin, f: F) -> Result<T>
where
    F: FnOnce(&mut Tx<'_>) -> Result<T>,
{
    conn.execute_batch(begin.statement())?;
    let mut tx = Tx { conn, depth: 0 };
    match f(&mut tx) {
        Ok(value) => match tx.conn.execute_batch("COMMIT") {
            Ok(()) => Ok(value),
            Err(err) => {
                let _ = tx.conn.execute_batch("ROLLBACK");
                Err(err.into())
            }
        },
        Err(err) => {
            let _ = tx.conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

/// A transaction (or nested savepoint scope) over the connection.
///
/// Nesting opens `SAVEPOINT oc_core_${n}` where `n` counts savepoint depth
/// (first nesting level is 1), matching the bridge's `effect_sql_${id}`
/// scheme with the port's own prefix.
pub struct Tx<'c> {
    conn: &'c mut Connection,
    depth: usize,
}

impl Tx<'_> {
    /// Read access to the underlying connection.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        self.conn
    }

    /// Executes one statement with parameters.
    pub fn execute<P: rusqlite::Params>(&mut self, sql: &str, params: P) -> Result<usize> {
        Ok(self.conn.execute(sql, params)?)
    }

    /// Executes a batch of statements (no rows may be returned).
    pub fn execute_batch(&mut self, sql: &str) -> Result<()> {
        Ok(self.conn.execute_batch(sql)?)
    }

    /// Current savepoint depth (0 = top-level transaction).
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Runs a nested scope under `SAVEPOINT oc_core_${depth+1}`; the
    /// savepoint is released on success and rolled back + released on error.
    pub fn transaction<R, F>(&mut self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<R>,
    {
        let name = format!("oc_core_{}", self.depth + 1);
        self.conn.execute_batch(&format!("SAVEPOINT {name}"))?;
        let parent_depth = self.depth;
        self.depth += 1;
        let result = f(self);
        self.depth = parent_depth;
        match result {
            Ok(value) => {
                self.conn.execute_batch(&format!("RELEASE SAVEPOINT {name}"))?;
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn.execute_batch(&format!("ROLLBACK TO SAVEPOINT {name}"));
                let _ = self.conn.execute_batch(&format!("RELEASE SAVEPOINT {name}"));
                Err(err)
            }
        }
    }
}

/// WAL at open (the node driver's native layer), then the tuning pragmas and
/// boot checkpoint (`database.ts:39-47`).
///
/// `busy_timeout` arms FIRST: the WAL journal-mode transition needs the
/// write lock, and without a busy handler a concurrent first-open of the
/// same file (a second standalone server in one project) fails instantly
/// with `database is locked` instead of waiting the 5s budget.
fn apply_pragmas(conn: &mut Connection, memory: bool) -> Result<()> {
    conn.execute_batch("PRAGMA busy_timeout = 5000")?;
    let mode = set_wal_with_retry(conn)?;
    if !memory && mode != "wal" {
        return Err(DbError::Pragma(format!("journal_mode did not stick (got {mode})")));
    }
    conn.execute_batch("PRAGMA synchronous = NORMAL")?;
    conn.execute_batch("PRAGMA cache_size = -64000")?;
    // One-shot boot checkpoint; returns (busy, log, checkpointed) — ignored.
    let _ = conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()));
    conn.execute_batch("PRAGMA foreign_keys = ON")?;
    Ok(())
}

/// `PRAGMA journal_mode = WAL` with a bounded retry. The mode switch can
/// still surface `SQLITE_BUSY` when a concurrent connection holds the
/// database mid-transition (the busy handler does not cover every step of
/// the switch); retrying a few times resolves the overlap — the switch is
/// idempotent once another opener set it.
fn set_wal_with_retry(conn: &mut Connection) -> Result<String> {
    let mut last: Option<rusqlite::Error> = None;
    for attempt in 0u32..5 {
        match conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get::<_, String>(0)) {
            Ok(mode) => return Ok(mode),
            Err(error) => {
                let busy = matches!(error, rusqlite::Error::SqliteFailure(ref code, _)
                        if code.code == rusqlite::ErrorCode::DatabaseBusy
                            || code.code == rusqlite::ErrorCode::DatabaseLocked);
                if !busy || attempt == 4 {
                    return Err(error.into());
                }
                last = Some(error);
                let backoff = 100u64.saturating_mul(u64::from(attempt + 1));
                std::thread::sleep(std::time::Duration::from_millis(backoff));
            }
        }
    }
    Err(last.map_or_else(|| DbError::Pragma("wal retry exhausted".into()), Into::into))
}

/// Creates the database file with mode `0600` if missing and tightens the
/// mode of the file and its `-wal`/`-shm` sidecars (`database.ts:83-98`).
/// Missing sidecars are tolerated (ENOENT); other errors propagate.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    if !path.exists() {
        // Create synchronously so another opener cannot race an unrestricted
        // file into existence (append-mode create mirrors the TS `openSync(…, "a")`).
        fs::OpenOptions::new().append(true).create(true).mode(0o600).open(path)?;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        match fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> io::Result<()> {
    // Windows relies on the user profile directory's inherited ACLs.
    Ok(())
}
