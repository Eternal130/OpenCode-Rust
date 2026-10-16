//! One-way, first-launch migration from upstream opencode v2 (TS) data
//! directories into the port's isolated `opencode-rust` roots.
//!
//! Divergence registered in `PORT_REPORT` §3 / tools/RELEASE.md: v2's own
//! v1→v2 migration was IN-PLACE (same directories); the Rust port
//! isolates its XDG suffix (`opencode-rust`, see
//! `oc_util::global_roots::APP_NAME`), so adopting v2 data must be a
//! COPY — sources under `~/.config/opencode`, `~/.local/share/opencode`,
//! `~/.cache/opencode` are NEVER moved, deleted, or modified. Running
//! an upstream v2 install next to the port stays safe (one-way door).
//!
//! Semantics of [`ensure_migrated`]:
//!
//! 1. Resolve our roots and the legacy v2 roots under the SAME
//!    home/env rules (`OPENCODE_TEST_HOME`, `XDG_*`,
//!    `OPENCODE_CONFIG_DIR` all honored — under redirected sandbox envs
//!    the legacy roots land inside the sandbox, don't exist, and the
//!    whole thing degrades to writing the marker: a cheap no-op).
//! 2. Marker `<our data>/v2-migration.json` present → return
//!    immediately (one stat; safe to call from every entry point).
//! 3. Take the exclusive lock `<our state>/locks/v2-migration.lock`
//!    (atomically created with `create_new`; contention waits-polls
//!    ≤30s; a lock older than 10 minutes is stale and gets removed).
//!    Re-check the marker under the lock.
//! 4. Legacy data dir absent → fresh install: write the marker, return.
//! 5. Copy (sources are read-only inputs):
//!    * databases: every `opencode.db` / `opencode-<channel>.db` in the
//!      legacy data dir, via the rusqlite backup API (consistent
//!      snapshot even with a live `-wal`); per-file failure falls back
//!      to a plain byte copy of db + `-wal` + `-shm`;
//!    * `auth.json` from the legacy data dir;
//!    * config: every regular FILE directly in the legacy config dir
//!      (no recursion, no `*.lock`/`*.tmp`/dotfiles);
//!    * plugin caches: the legacy `npm/` and managed-bun `bin/`
//!      subtrees (recursive, skip existing, skip `*.lock`);
//!    * STATE ROOT (auditF 3.3 — the migration hole that silently lost
//!      tabs, prefs, and registrations): `service[-<channel>].json`
//!      registrations; the model-preference doc `model.json` (written
//!      to BOTH v2's canonical `<state>/model.json` and the port's
//!      current read seam `<data>/tui/model.json` — auditC S3's
//!      split-brain bridge); the pinned-sessions doc `session.json`;
//!      every `<channel>/tui/*.json` channel-storage document (the
//!      `tabs` doc is CONVERTED into the port's versioned shape —
//!      `read_tabs` leaves foreign docs untouched, so the migrator
//!      adopting v2's shape explicitly is the sanctioned path; every
//!      other key copies verbatim); and the prompt history family
//!      (`prompt-history.jsonl`, `frecency.jsonl`,
//!      `prompt-stash.jsonl` — consumed by the TUI lane's readers).
//!    * `log/`, `repos/`, and everything else are intentionally NOT
//!      copied.
//!      All copies are idempotent: a destination that already exists is
//!      left untouched. Per-file errors are logged and skipped.
//! 6. Write the marker JSON `{completed_at, source_data,
//!    copied_databases, copied_files}` — the one-way door's latch.
//! 7. Hard failures (lock/io) return [`MigrationError`] WITHOUT writing
//!    the marker; callers log a warning and continue boot (never crash
//!    the app over a migration).

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oc_util::global_roots::{self, Roots};
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use walkdir::WalkDir;

/// Marker filename under our data root — presence means "migration
/// decided and done (or decided unnecessary); never run again".
pub const MARKER_FILENAME: &str = "v2-migration.json";

/// Exclusive-migration lock filename under our `state/locks` dir.
pub const LOCK_FILENAME: &str = "v2-migration.lock";

/// A lock file older than this is stale (crashed holder) and gets removed.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(600);

/// How long a contender waits for the lock holder before giving up
/// (the caller then logs a warning and continues boot unmigrated).
const LOCK_WAIT_MAX: Duration = Duration::from_secs(30);

/// Lock contention poll interval.
const LOCK_POLL: Duration = Duration::from_millis(100);

/// What was migrated (also the marker document's shape).
#[derive(Debug, Default, Clone, Serialize)]
pub struct Report {
    /// Unix seconds at completion.
    pub completed_at: u64,
    /// The legacy v2 data dir things were copied FROM.
    pub source_data: PathBuf,
    /// Database files copied into our data dir.
    pub copied_databases: Vec<PathBuf>,
    /// Every other file copied (auth, config, npm/bin cache files).
    pub copied_files: Vec<PathBuf>,
    /// State-root artifacts carried into our state/data roots
    /// (service registrations, model/session docs, channel-storage
    /// documents, prompt history).
    #[serde(default)]
    pub copied_state: Vec<PathBuf>,
}

impl Report {
    /// Whether anything was actually copied (drives the one-line summary).
    #[must_use]
    pub fn copied_anything(&self) -> bool {
        !self.copied_databases.is_empty()
            || !self.copied_files.is_empty()
            || !self.copied_state.is_empty()
    }
}

/// A migration hard failure. Per-file copy problems are NOT failures —
/// they are logged and skipped; only lock/io fundamentals land here.
#[derive(Debug)]
pub struct MigrationError(pub String);

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for MigrationError {}

/// Runs the one-way v2→rust migration if it has not run yet. Cheap when
/// done (one marker stat), lock-guarded against concurrent entry points.
pub fn ensure_migrated() -> Result<Report, MigrationError> {
    let ours = global_roots::resolve();
    let marker = ours.data.join(MARKER_FILENAME);
    if marker.is_file() {
        return Ok(Report::default());
    }

    let _lock = LockGuard::acquire(&ours)?;
    // Double-checked: a concurrent process may have finished while we
    // waited for the lock.
    if marker.is_file() {
        return Ok(Report::default());
    }

    let legacy = global_roots::legacy_v2_roots();
    if !legacy.data.is_dir() {
        // Fresh install (or a redirected sandbox): nothing to migrate.
        write_marker(
            &marker,
            &Report {
                completed_at: unix_now(),
                source_data: legacy.data.clone(),
                ..Report::default()
            },
        )?;
        return Ok(Report::default());
    }

    let mut report = Report {
        completed_at: unix_now(),
        source_data: legacy.data.clone(),
        copied_databases: Vec::new(),
        copied_files: Vec::new(),
        copied_state: Vec::new(),
    };
    copy_databases(&legacy, &ours, &mut report);
    copy_auth(&legacy, &ours, &mut report);
    copy_config_files(&legacy, &ours, &mut report);
    copy_cache_subtree(&legacy.cache.join("npm"), &ours.cache.join("npm"), &mut report);
    copy_cache_subtree(&legacy.cache.join("bin"), &ours.cache.join("bin"), &mut report);
    copy_state_root(&legacy, &ours, &mut report);

    report.completed_at = unix_now();
    write_marker(&marker, &report)?;
    Ok(report)
}

/// [`ensure_migrated`] with the caller-side reporting contract: a
/// one-line summary when something was copied, a one-line warning (and
/// continued boot) on hard failure.
pub fn ensure_migrated_logged() {
    match ensure_migrated() {
        Ok(report) if report.copied_anything() => {
            eprintln!(
                "opencode-rust: migrated from opencode v2 data ({} files, {} databases)",
                report.copied_files.len(),
                report.copied_databases.len(),
            );
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("opencode-rust: v2 data migration skipped: {error}");
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

fn write_marker(marker: &Path, report: &Report) -> Result<(), MigrationError> {
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| MigrationError(format!("create {}: {error}", parent.display())))?;
    }
    let body = serde_json::to_vec_pretty(report)
        .map_err(|error| MigrationError(format!("serialize marker: {error}")))?;
    fs::write(marker, body)
        .map_err(|error| MigrationError(format!("write {}: {error}", marker.display())))
}

// --- the lock -------------------------------------------------------------

/// Exclusive migration lock: a `create_new` file removed on drop.
struct LockGuard {
    path: PathBuf,
    file: Option<File>,
}

impl LockGuard {
    fn acquire(ours: &Roots) -> Result<Self, MigrationError> {
        let dir = ours.locks();
        fs::create_dir_all(&dir)
            .map_err(|error| MigrationError(format!("create {}: {error}", dir.display())))?;
        let path = dir.join(LOCK_FILENAME);
        let deadline = Instant::now() + LOCK_WAIT_MAX;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok(Self { path, file: Some(file) }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if Self::is_stale(&path) {
                        // Crashed holder: break the lock and retry once
                        // (the loop then either wins the create or waits
                        // for whoever removed/holds it now).
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(MigrationError(format!(
                            "migration lock {} still held after {}s",
                            path.display(),
                            LOCK_WAIT_MAX.as_secs()
                        )));
                    }
                    std::thread::sleep(LOCK_POLL);
                }
                Err(error) => {
                    return Err(MigrationError(format!("create {}: {error}", path.display())));
                }
            }
        }
    }

    fn is_stale(path: &Path) -> bool {
        match fs::metadata(path).and_then(|meta| meta.modified()) {
            Ok(modified) => SystemTime::now()
                .duration_since(modified)
                .map_or(true, |age| age > LOCK_STALE_AFTER),
            Err(_) => true,
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Close first (drop the handle), then unlink — best-effort.
        self.file = None;
        let _ = fs::remove_file(&self.path);
    }
}

// --- copy steps -----------------------------------------------------------

/// Is `filename` one of the flat global databases v2 writes
/// (`opencode.db` plus channel-suffixed variants)?
fn is_database_filename(name: &str) -> bool {
    let path = Path::new(name);
    let is_db = path.extension() == Some(std::ffi::OsStr::new("db"));
    name == "opencode.db" || (name.starts_with("opencode-") && is_db)
}

fn copy_databases(legacy: &Roots, ours: &Roots, report: &mut Report) {
    let Ok(entries) = fs::read_dir(&legacy.data) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if !is_database_filename(&name) {
            continue;
        }
        let source = entry.path();
        let dest = ours.data.join(&name);
        if dest.exists() {
            continue; // idempotent: ours wins
        }
        if backup_database(&source, &dest) {
            report.copied_databases.push(dest);
        }
    }
}

/// Consistent copy of a sqlite database via the backup API; on failure
/// falls back to a plain byte copy of db + `-wal` + `-shm`.
fn backup_database(source: &Path, dest: &Path) -> bool {
    if let Some(parent) = dest.parent() {
        if fs::create_dir_all(parent).is_err() {
            return false;
        }
    }
    let via_backup = (|| -> Result<(), rusqlite::Error> {
        let source_conn = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut target = Connection::open(dest)?;
        let backup = Backup::new(&source_conn, &mut target)?;
        backup.run_to_completion(64, Duration::from_millis(5), None)?;
        Ok(())
    })();
    if via_backup.is_ok() {
        return true;
    }
    eprintln!(
        "opencode-rust: v2 migration: sqlite backup of {} failed ({}), byte-copying instead",
        source.display(),
        via_backup.err().map_or("unknown".to_owned(), |e| e.to_string())
    );
    byte_copy_sidecars(source, dest)
}

/// Plain copy of `source` → `dest` plus its `-wal`/`-shm` sidecars.
fn byte_copy_sidecars(source: &Path, dest: &Path) -> bool {
    let copied = fs::copy(source, dest).is_ok();
    let Some(stem) = source.file_name().map(|name| name.to_string_lossy().into_owned()) else {
        return copied;
    };
    let Some(dest_stem) = dest.file_name().map(|name| name.to_string_lossy().into_owned()) else {
        return copied;
    };
    for suffix in ["-wal", "-shm"] {
        let side_source = source.with_file_name(format!("{stem}{suffix}"));
        if side_source.is_file() {
            let side_dest = dest.with_file_name(format!("{dest_stem}{suffix}"));
            let _ = fs::copy(&side_source, &side_dest);
        }
    }
    copied
}

fn copy_auth(legacy: &Roots, ours: &Roots, report: &mut Report) {
    let source = legacy.data.join("auth.json");
    let dest = ours.data.join("auth.json");
    if source.is_file() && !dest.exists() {
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::copy(&source, &dest) {
            Ok(_) => report.copied_files.push(dest),
            Err(error) => {
                eprintln!(
                    "opencode-rust: v2 migration: copy {}: {error} (skipped)",
                    source.display()
                );
            }
        }
    }
}

/// Should a flat config-dir file be migrated? Regular files only, no
/// `*.lock`/`*.tmp`, no dotfiles.
fn is_config_filename(name: &str) -> bool {
    let path = Path::new(name);
    !name.starts_with('.')
        && !matches!(path.extension().and_then(|ext| ext.to_str()), Some("lock" | "tmp" | "swp"))
}

fn copy_config_files(legacy: &Roots, ours: &Roots, report: &mut Report) {
    let Ok(entries) = fs::read_dir(&legacy.config) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue; // no recursion into subdirectories
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if !is_config_filename(&name) {
            continue;
        }
        let source = entry.path();
        let dest = ours.config.join(&name);
        if dest.exists() {
            continue; // idempotent: ours wins
        }
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::copy(&source, &dest) {
            Ok(_) => report.copied_files.push(dest),
            Err(error) => {
                eprintln!(
                    "opencode-rust: v2 migration: copy {}: {error} (skipped)",
                    source.display()
                );
            }
        }
    }
}

// --- the state root --------------------------------------------------------

/// The version stamp the port's open-tabs `tabs` document carries
/// (oc-tui `storage::TABS_VERSION`; a doc without it is v2's shape and
/// is left untouched by the TUI's reader).
const TABS_VERSION: u32 = 2;

/// Is `name` a valid channel/storage segment (the reference's
/// `^[a-zA-Z0-9][a-zA-Z0-9._-]*`, rejecting `.` and `..`)?
fn is_state_segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric())
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// Carry v2's STATE root into ours (auditF 3.3): service
/// registrations, the model-preference doc (both canonical and
/// back-compat destinations), pinned sessions, every channel's TUI
/// storage documents (tabs ADOPTED into the port's versioned shape),
/// and the prompt-history family. Sources are read-only; destinations
/// that already exist win (idempotence).
fn copy_state_root(legacy: &Roots, ours: &Roots, report: &mut Report) {
    if !legacy.state.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(&legacy.state) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        if meta.is_file() {
            match name.as_str() {
                // Service registrations (`service[-<channel>].json`,
                // service-config.ts:30-43): custom hostname/port/env
                // settings survive the switch. The version field inside
                // matches our 2.0.24 build, so the port's
                // `version_belongs_to_channel` check accepts them.
                _ if is_service_filename(&name) => {
                    copy_state_file(&entry.path(), ours.state.join(&name), report);
                }
                // The model-preference doc (`local.tsx:154`): v2's
                // canonical state path AND the port's current read seam
                // (`<data>/tui/model.json`, auditC S3) — the split-brain
                // bridge until the TUI lane unifies the reader on the
                // state path.
                "model.json" => {
                    copy_state_file(&entry.path(), ours.state.join(&name), report);
                    copy_state_file(&entry.path(), ours.data.join("tui").join(&name), report);
                }
                // Pinned sessions (`{pinned: [...]}`, local.tsx:504-523:
                // the file lands where the unified reader will look — the
                // TUI currently keeps pins in memory, TUI-lane half) and
                // the prompt history family (`prompt/history.tsx:93`,
                // `frecency.tsx:42`, `stash.tsx:40`), preserved for the
                // TUI lane's readers.
                "session.json"
                | "prompt-history.jsonl"
                | "frecency.jsonl"
                | "prompt-stash.jsonl" => {
                    copy_state_file(&entry.path(), ours.state.join(&name), report);
                }
                _ => {}
            }
            continue;
        }
        if !meta.is_dir() || !is_state_segment(&name) || name == "locks" {
            continue;
        }
        // A channel directory: carry its `tui/` storage documents.
        let channel_tui = entry.path().join("tui");
        if !channel_tui.is_dir() {
            continue;
        }
        let Ok(documents) = fs::read_dir(&channel_tui) else { continue };
        for document in documents.flatten() {
            let Ok(document_meta) = document.metadata() else { continue };
            if !document_meta.is_file() {
                continue;
            }
            let Some(document_name) = document.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if matches!(
                Path::new(&document_name).extension().and_then(|ext| ext.to_str()),
                Some("lock" | "tmp")
            ) {
                continue;
            }
            let target = ours.state.join(&name).join("tui").join(&document_name);
            if document_name == "tabs.json" {
                // ADOPT v2's shape (`{global, cwd}` without a version)
                // into the port's versioned document — the sanctioned
                // conversion (the TUI's `read_tabs` leaves foreign docs
                // untouched, so without this the tab list would reset).
                // A doc already stamped with our version (or any other
                // shape) copies verbatim only when the field is absent;
                // anything unrecognizable is left for the tolerant
                // reader.
                if target.exists() {
                    continue;
                }
                if let Some(adopted) = adopt_v2_tabs(&document.path()) {
                    if let Some(parent) = target.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    match serde_json::to_vec_pretty(&adopted)
                        .map_err(|error| error.to_string())
                        .and_then(|body| {
                            fs::write(&target, body)
                                .map_err(|error| format!("write {}: {error}", target.display()))
                        }) {
                        Ok(()) => report.copied_state.push(target),
                        Err(error) => {
                            eprintln!("opencode-rust: v2 migration: {error} (skipped)");
                            let _ = fs::remove_file(&target);
                        }
                    }
                    continue;
                }
            }
            copy_state_file(&document.path(), target, report);
        }
    }
}

/// `service.json` or `service-<channel>.json` (sanitized channel).
fn is_service_filename(name: &str) -> bool {
    name == "service.json"
        || (name.starts_with("service-")
            && Path::new(name).extension() == Some(std::ffi::OsStr::new("json"))
            && is_state_segment(name.trim_start_matches("service-").trim_end_matches(".json")))
}

/// Copy one state file when the destination is absent (idempotent,
/// ours-wins); record it under `copied_state`.
fn copy_state_file(source: &Path, dest: PathBuf, report: &mut Report) {
    if dest.exists() || !source.is_file() {
        return;
    }
    if let Some(parent) = dest.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    match fs::copy(source, &dest) {
        Ok(_) => report.copied_state.push(dest),
        Err(error) => {
            eprintln!("opencode-rust: v2 migration: copy {}: {error} (skipped)", source.display());
        }
    }
}

/// Convert v2's tabs document (`{global, cwd}` — session-tabs.tsx's
/// `PersistedState`) into the port's versioned shape: parse, verify
/// the v2 fields exist, stamp `version: TABS_VERSION`. Returns `None`
/// for anything that is not a v2 tabs doc (already-versioned,
/// malformed, or unrelated JSON — copied verbatim by the caller
/// instead).
fn adopt_v2_tabs(source: &Path) -> Option<serde_json::Value> {
    let bytes = fs::read(source).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let object = value.as_object()?;
    let looks_like_v2 = object.contains_key("global") || object.contains_key("cwd");
    if !looks_like_v2 || object.contains_key("version") {
        return None;
    }
    let mut adopted = object.clone();
    adopted.insert("version".to_owned(), serde_json::Value::from(u64::from(TABS_VERSION)));
    Some(serde_json::Value::Object(adopted))
}

/// Recursive copy of a cache subtree (`npm/`, managed-bun `bin/`),
/// skipping destinations that already exist and `*.lock` files.
fn copy_cache_subtree(source: &Path, dest: &Path, report: &mut Report) {
    if !source.is_dir() {
        return;
    }
    for entry in WalkDir::new(source).follow_links(false) {
        let Ok(entry) = entry else { continue };
        let Ok(relative) = entry.path().strip_prefix(source) else { continue };
        let target = dest.join(relative);
        if entry.file_type().is_dir() {
            let _ = fs::create_dir_all(&target);
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if matches!(
            Path::new(entry.file_name()).extension().and_then(|ext| ext.to_str()),
            Some("lock")
        ) {
            continue;
        }
        if target.exists() {
            continue; // idempotent
        }
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::copy(entry.path(), &target) {
            Ok(_) => report.copied_files.push(target),
            Err(error) => {
                eprintln!(
                    "opencode-rust: v2 migration: copy {}: {error} (skipped)",
                    entry.path().display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    // Environment mutation must be serialized within the process (the
    // same discipline as `oc_util::global_roots` tests).
    fn lock() -> MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    struct Env {
        _guard: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl Env {
        fn redirect(home: &Path) -> Self {
            let guard = lock();
            let vars = [
                "HOME",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "XDG_CACHE_HOME",
                "OPENCODE_CONFIG_DIR",
                "OPENCODE_TEST_HOME",
                "OPENCODE_DB",
                "TMPDIR",
            ];
            let saved = vars.iter().map(|&name| (name, std::env::var(name).ok())).collect();
            for name in vars {
                std::env::remove_var(name);
            }
            std::env::set_var("HOME", home);
            Self { _guard: guard, saved }
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    /// A fake v2 home: real sqlite `opencode.db` + channel db, auth.json,
    /// flat config files (+ junk that must NOT copy), npm/bin caches, the
    /// full v2 STATE root (service registrations, model prefs, pinned
    /// sessions, per-channel TUI storage, prompt history), and log/repos
    /// dirs that must never be copied.
    struct LegacyHome {
        root: PathBuf,
    }

    impl LegacyHome {
        fn stage() -> Self {
            std::fs::create_dir_all("/tmp/opencode").expect("scratch root");
            let root = tempfile::tempdir_in("/tmp/opencode").expect("sandbox root").keep();
            let home = root.join("home");
            let data = home.join(".local/share/opencode");
            let config = home.join(".config/opencode");
            let cache = home.join(".cache/opencode/npm/oh-my-opencode-slim@latest/1/node_modules");
            std::fs::create_dir_all(&data).expect("data");
            std::fs::create_dir_all(data.join("log")).expect("log");
            std::fs::write(data.join("log/app.log"), "log").expect("log file");
            std::fs::create_dir_all(data.join("repos")).expect("repos");
            std::fs::write(data.join("repos/x"), "x").expect("repo file");
            std::fs::create_dir_all(&config).expect("config");
            std::fs::create_dir_all(config.join("subdir")).expect("config subdir");
            std::fs::write(config.join("subdir/nope.json"), "{}").expect("config subdir file");
            std::fs::create_dir_all(&cache).expect("cache");

            let legacy = Self { root: root.clone() };
            legacy.write_db("opencode.db", "CREATE TABLE t (x); INSERT INTO t VALUES (42);");
            legacy.write_db("opencode-stable.db", "CREATE TABLE s (x); INSERT INTO s VALUES (7);");
            std::fs::write(data.join("auth.json"), r#"{"github":"token"}"#).expect("auth");
            std::fs::write(config.join("opencode.json"), r#"{"theme":"v2"}"#).expect("config");
            std::fs::write(config.join("preset.json"), r#"{"plugin":1}"#).expect("preset");
            std::fs::write(config.join("scratch.lock"), "lock").expect("lock junk");
            std::fs::write(config.join("editor.tmp"), "tmp").expect("tmp junk");
            std::fs::write(cache.join("plugin.js"), "// plugin").expect("cache file");
            let bun_dir = home.join(".cache/opencode/bin");
            std::fs::create_dir_all(&bun_dir).expect("bun dir");
            std::fs::write(bun_dir.join("bun"), "#!/bin/sh\necho bun\n").expect("managed bun");
            legacy.stage_state_root();
            legacy
        }

        /// The v2 state root: `<state>/opencode/…`.
        fn stage_state_root(&self) {
            let state = self.state_root();
            std::fs::create_dir_all(&state).expect("state");
            // Service registrations (global + channel).
            std::fs::write(
                state.join("service.json"),
                r#"{"id":"srv","version":"2.0.24","url":"http://127.0.0.1:49343","pid":1}"#,
            )
            .expect("service.json");
            std::fs::write(
                state.join("service-stable.json"),
                r#"{"id":"srv2","version":"0.0.0-stable-2.0.24","url":"http://127.0.0.1:49344","pid":2}"#,
            )
            .expect("channel service");
            // Model preferences (v2 canonical state path).
            std::fs::write(
                state.join("model.json"),
                r#"{"recent":[{"providerID":"anthropic","modelID":"claude"}],"favorite":[],"variant":{"anthropic/claude":"think"}}"#,
            )
            .expect("model.json");
            // Pinned sessions.
            std::fs::write(state.join("session.json"), r#"{"pinned":["ses_a","ses_b"]}"#)
                .expect("session.json");
            // Prompt history family.
            std::fs::write(state.join("prompt-history.jsonl"), "{\"text\":\"fix the build\"}\n")
                .expect("history");
            std::fs::write(state.join("frecency.jsonl"), "{\"text\":\"tests\",\"count\":3}\n")
                .expect("frecency");
            std::fs::write(state.join("prompt-stash.jsonl"), "{\"text\":\"draft\"}\n")
                .expect("stash");
            // Channel storage: v2-shaped tabs + plain prefs keys.
            let stable_tui = state.join("stable/tui");
            std::fs::create_dir_all(&stable_tui).expect("stable tui");
            std::fs::write(
                stable_tui.join("tabs.json"),
                r#"{"global":{"tabs":[{"sessionID":"ses_a","title":"Build fix"},{"sessionID":"ses_b"}],"unread":{}},"cwd":{"/work":{"tabs":[{"sessionID":"ses_c"}],"unread":{}}}}"#,
            )
            .expect("v2 tabs");
            std::fs::write(stable_tui.join("session-list.json"), r#"{"all_projects":false}"#)
                .expect("session-list");
            std::fs::write(stable_tui.join("getting-started.json"), r#"{"dismissed":true}"#)
                .expect("getting-started");
            std::fs::write(stable_tui.join("view.json"), r#"{"mcp":true}"#).expect("view");
            std::fs::write(stable_tui.join("scratch.tmp"), "tmp").expect("tmp junk");
            // A second channel directory + a junk directory that is NOT a
            // valid channel segment (must not copy).
            let local_tui = state.join("local/tui");
            std::fs::create_dir_all(&local_tui).expect("local tui");
            std::fs::write(local_tui.join("tabs.json"), r#"{"global":{"tabs":[],"unread":{}}}"#)
                .expect("local tabs");
            std::fs::create_dir_all(state.join("we ird/tui")).expect("junk channel");
            std::fs::write(state.join("we ird/tui/tabs.json"), "{}").expect("junk tabs");
            // Locks directory (never copied).
            std::fs::create_dir_all(state.join("locks")).expect("locks");
            std::fs::write(state.join("locks/cli-json.lock"), "").expect("lock");
        }

        fn write_db(&self, name: &str, seed: &str) {
            let path = self.root.join("home/.local/share/opencode").join(name);
            let conn = Connection::open(&path).expect("open seed db");
            conn.execute_batch(seed).expect("seed db");
        }

        fn home(&self) -> PathBuf {
            self.root.join("home")
        }

        fn legacy_data(&self) -> PathBuf {
            self.home().join(".local/share/opencode")
        }

        fn state_root(&self) -> PathBuf {
            self.home().join(".local/state/opencode")
        }
    }

    fn snapshot_tree(root: &Path) -> Vec<(PathBuf, SystemTime, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in WalkDir::new(root) {
            let entry = entry.expect("walk");
            if !entry.file_type().is_file() {
                continue;
            }
            let meta = entry.metadata().expect("meta");
            out.push((
                entry.path().to_path_buf(),
                meta.modified().expect("mtime"),
                std::fs::read(entry.path()).expect("read"),
            ));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    #[test]
    fn fresh_install_writes_marker_only() {
        let sandbox = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let home = sandbox.path().join("home");
        std::fs::create_dir_all(&home).expect("home");
        let _env = Env::redirect(&home);

        let report = ensure_migrated().expect("fresh install migrates");
        assert!(!report.copied_anything());
        let data = home.join(".local/share/opencode-rust");
        assert!(data.join(MARKER_FILENAME).is_file(), "marker written");
        let entries: Vec<_> = std::fs::read_dir(&data).expect("data dir").flatten().collect();
        assert_eq!(entries.len(), 1, "marker is the only data-dir entry");
        let marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(data.join(MARKER_FILENAME)).expect("marker"),
        )
        .expect("marker json");
        assert!(marker["completed_at"].is_u64());
        assert_eq!(marker["source_data"], serde_json::json!(home.join(".local/share/opencode")));
        // The lock is cleaned up.
        assert!(!home.join(".local/state/opencode-rust/locks").join(LOCK_FILENAME).exists());
    }

    #[test]
    fn migrates_the_v2_layout_without_touching_sources() {
        let legacy = LegacyHome::stage();
        let home = legacy.home();
        let legacy_roots = [
            legacy.legacy_data(),
            home.join(".config/opencode"),
            home.join(".cache/opencode"),
            legacy.state_root(),
        ];
        let before: Vec<_> = legacy_roots.iter().map(|root| snapshot_tree(root)).collect();
        let _env = Env::redirect(&home);

        let report = ensure_migrated().expect("migration succeeds");
        assert_eq!(
            report.copied_databases.len(),
            2,
            "both channel dbs: {:?}",
            report.copied_databases
        );
        // Databases: copied AND readable as sqlite with the seeded rows.
        for (name, table, expected) in [("opencode.db", "t", 42), ("opencode-stable.db", "s", 7)] {
            let path = home.join(".local/share/opencode-rust").join(name);
            let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .expect("copied db opens");
            let value: i64 = conn
                .query_row(&format!("SELECT x FROM {table}"), [], |row| row.get(0))
                .expect("seeded row survived");
            assert_eq!(value, expected);
        }
        // auth.json copied.
        assert_eq!(
            std::fs::read_to_string(home.join(".local/share/opencode-rust/auth.json"))
                .expect("auth"),
            r#"{"github":"token"}"#
        );
        // Config: the two real files copied; junk + subdirs not.
        let our_config = home.join(".config/opencode-rust");
        assert_eq!(
            std::fs::read_to_string(our_config.join("opencode.json")).expect("config"),
            r#"{"theme":"v2"}"#
        );
        assert!(our_config.join("preset.json").is_file());
        assert!(!our_config.join("scratch.lock").exists());
        assert!(!our_config.join("editor.tmp").exists());
        assert!(!our_config.join("subdir").exists());
        // Caches: npm subtree + managed bun copied.
        assert!(home
            .join(".cache/opencode-rust/npm/oh-my-opencode-slim@latest/1/node_modules/plugin.js")
            .is_file());
        assert!(home.join(".cache/opencode-rust/bin/bun").is_file());
        // log/ and repos/ are never copied.
        assert!(!home.join(".local/share/opencode-rust/log").exists());
        assert!(!home.join(".local/share/opencode-rust/repos").exists());
        // Marker written with the real source path.
        let marker: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(home.join(".local/share/opencode-rust").join(MARKER_FILENAME))
                .expect("marker"),
        )
        .expect("marker json");
        assert_eq!(marker["source_data"], serde_json::json!(legacy.legacy_data()));
        assert_eq!(marker["copied_databases"].as_array().map(Vec::len), Some(2));
        // Sources untouched: byte-identical, mtimes included.
        for (index, root) in legacy_roots.iter().enumerate() {
            assert_eq!(snapshot_tree(root), before[index], "{} not modified", root.display());
        }
        // Lock cleaned up.
        assert!(!home.join(".local/state/opencode-rust/locks").join(LOCK_FILENAME).exists());
    }

    /// The state-root carry (auditF 3.3): every v2 state artifact lands
    /// in our shapes — tabs ADOPTED (version stamped, data preserved),
    /// model prefs written to BOTH destinations, junk skipped.
    #[test]
    fn migrates_the_v2_state_root_end_to_end() {
        let legacy = LegacyHome::stage();
        let home = legacy.home();
        let _env = Env::redirect(&home);

        let report = ensure_migrated().expect("migration succeeds");
        let state = home.join(".local/state/opencode-rust");

        // Service registrations copied (global + channel).
        assert!(state.join("service.json").is_file());
        assert!(state.join("service-stable.json").is_file());

        // Model preferences: v2's canonical path AND the port's current
        // read seam (auditC S3's split-brain bridge).
        let model = r#"{"recent":[{"providerID":"anthropic","modelID":"claude"}],"favorite":[],"variant":{"anthropic/claude":"think"}}"#;
        assert_eq!(std::fs::read_to_string(state.join("model.json")).expect("state model"), model);
        assert_eq!(
            std::fs::read_to_string(home.join(".local/share/opencode-rust/tui/model.json"))
                .expect("data/tui model"),
            model
        );

        // Pinned sessions + the prompt history family.
        assert_eq!(
            std::fs::read_to_string(state.join("session.json")).expect("pinned"),
            r#"{"pinned":["ses_a","ses_b"]}"#
        );
        assert!(state.join("prompt-history.jsonl").is_file());
        assert!(state.join("frecency.jsonl").is_file());
        assert!(state.join("prompt-stash.jsonl").is_file());

        // Tabs: v2's shape ADOPTED into the port's versioned document —
        // every scope and tab preserved, `version: 2` stamped.
        let tabs: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(state.join("stable/tui/tabs.json")).expect("tabs"),
        )
        .expect("tabs json");
        assert_eq!(tabs["version"], serde_json::json!(2), "stamped with our version");
        assert_eq!(tabs["global"]["tabs"][0]["sessionID"], serde_json::json!("ses_a"));
        assert_eq!(tabs["global"]["tabs"][0]["title"], serde_json::json!("Build fix"));
        assert_eq!(tabs["global"]["tabs"][1]["sessionID"], serde_json::json!("ses_b"));
        assert_eq!(tabs["cwd"]["/work"]["tabs"][0]["sessionID"], serde_json::json!("ses_c"));

        // The other channel-storage keys copy verbatim; junk does not.
        assert_eq!(
            std::fs::read_to_string(state.join("stable/tui/session-list.json")).expect("prefs"),
            r#"{"all_projects":false}"#
        );
        assert_eq!(
            std::fs::read_to_string(state.join("stable/tui/getting-started.json"))
                .expect("onboarding"),
            r#"{"dismissed":true}"#
        );
        assert!(state.join("stable/tui/view.json").is_file(), "foreign keys preserved");
        assert!(!state.join("stable/tui/scratch.tmp").exists());
        assert!(state.join("local/tui/tabs.json").is_file(), "every channel carries");
        assert!(!state.join("we ird").exists(), "invalid channel segments skipped");
        assert!(
            !state.join("locks/cli-json.lock").exists(),
            "the legacy lock file is never copied (our own empty locks/ dir is fine)"
        );

        // The report counts the state carries.
        assert!(
            report.copied_state.iter().any(|path| path == &state.join("stable/tui/tabs.json")),
            "state carries recorded: {:?}",
            report.copied_state
        );

        // Ours-wins: a pre-existing state artifact is NOT overwritten.
        let marker = home.join(".local/share/opencode-rust").join(MARKER_FILENAME);
        let _ = std::fs::remove_file(&marker);
        std::fs::write(state.join("session-list.json"), r#"{"all_projects":true}"#).expect("ours");
        ensure_migrated().expect("second run after marker reset");
        assert_eq!(
            std::fs::read_to_string(state.join("stable/tui/session-list.json")).expect("prefs"),
            r#"{"all_projects":false}"#,
            "the stable channel's file is still v2's (ours won earlier)"
        );
        assert_eq!(
            std::fs::read_to_string(state.join("session-list.json")).expect("ours wins"),
            r#"{"all_projects":true}"#
        );
    }

    #[test]
    fn rerun_after_migration_is_a_noop() {
        let legacy = LegacyHome::stage();
        let home = legacy.home();
        let _env = Env::redirect(&home);

        ensure_migrated().expect("first run");
        let data = home.join(".local/share/opencode-rust");
        let marker = data.join(MARKER_FILENAME);
        let before_data = snapshot_tree(&data);
        let before_marker_mtime =
            std::fs::metadata(&marker).expect("marker").modified().expect("mtime");
        let before_sources = snapshot_tree(&home.join(".local/share/opencode"));

        let report = ensure_migrated().expect("second run");
        assert!(!report.copied_anything(), "nothing copied twice");
        assert_eq!(snapshot_tree(&data), before_data, "no new writes");
        assert_eq!(
            std::fs::metadata(&marker).expect("marker").modified().expect("mtime"),
            before_marker_mtime,
            "marker not rewritten"
        );
        assert_eq!(snapshot_tree(&home.join(".local/share/opencode")), before_sources);
    }

    #[test]
    fn existing_marker_skips_migration_entirely() {
        let legacy = LegacyHome::stage();
        let home = legacy.home();
        let _env = Env::redirect(&home);
        // Pre-latch the marker (already migrated on an earlier launch).
        let data = home.join(".local/share/opencode-rust");
        std::fs::create_dir_all(&data).expect("data");
        std::fs::write(data.join(MARKER_FILENAME), "{}").expect("marker");

        let report = ensure_migrated().expect("marker short-circuits");
        assert!(!report.copied_anything());
        assert!(!data.join("auth.json").exists(), "legacy auth never copied");
        assert!(!data.join("opencode.db").exists(), "legacy db never copied");
    }

    #[test]
    fn corrupt_db_falls_back_to_byte_copy() {
        let legacy = LegacyHome::stage();
        let home = legacy.home();
        let _env = Env::redirect(&home);
        // A file that looks like a channel db but is not sqlite.
        let garbage = legacy.legacy_data().join("opencode-garbage.db");
        std::fs::write(&garbage, b"definitely not sqlite").expect("garbage db");

        let report = ensure_migrated().expect("migration continues past the bad db");
        let copied = home.join(".local/share/opencode-rust/opencode-garbage.db");
        assert_eq!(
            std::fs::read(&copied).expect("byte-copied"),
            b"definitely not sqlite",
            "fallback byte copy ran"
        );
        assert!(
            report.copied_databases.iter().any(|path| path == &copied),
            "fallback counts as a copied database"
        );
        // The two healthy dbs still migrated.
        assert!(home.join(".local/share/opencode-rust/opencode.db").is_file());
    }

    #[test]
    fn stale_lock_is_broken_and_taken_over() {
        let legacy = LegacyHome::stage();
        let home = legacy.home();
        let _env = Env::redirect(&home);
        let locks = home.join(".local/state/opencode-rust/locks");
        std::fs::create_dir_all(&locks).expect("locks");
        let lock = locks.join(LOCK_FILENAME);
        std::fs::write(&lock, "").expect("stale lock");
        // Age it past LOCK_STALE_AFTER (std since 1.75).
        File::open(&lock)
            .and_then(|file| {
                file.set_modified(SystemTime::now() - LOCK_STALE_AFTER - Duration::from_secs(60))
            })
            .expect("age lock");

        let report = ensure_migrated().expect("stale lock broken, migration ran");
        assert!(report.copied_anything());
        assert!(!lock.exists(), "lock cleaned up after completion");
    }
}
