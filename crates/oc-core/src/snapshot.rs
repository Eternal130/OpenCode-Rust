//! Port of `snapshot.ts` + the git tree operations — the git2-based
//! content-addressed snapshot store at
//! `~/.local/share/opencode/snapshot/<projectID>/<Hash.fast(worktree)>`
//! (core-runner-map §2.6).
//!
//! - `Hash.fast` is SHA-1 of the worktree path (packages/util/src/hash.ts)
//!   — unified in `oc_util::hash::sha1_hex`.
//! - The store is a bare git object database; `capture` walks the scope,
//!   honors the SOURCE repository's ignore rules (tracked files always
//!   included), skips untracked files over 2 MiB, and writes a tree.
//!   G-219: the store's odb registers the SOURCE repository's objects as
//!   an alternate (`objects/info/alternates`, ref git.ts:345-382) and
//!   `capture` maintains a stat-cache sidecar — stat-identical files
//!   reuse their oid without re-reading (steady state is O(changed)
//!   reads like `git add --all`; the port's stand-in for the ref's git
//!   index copy).
//! - `files(from,to)` lists changed project-relative paths; `diff` renders
//!   per-file unified diffs; `restore` writes blobs back and deletes paths
//!   absent from their tree.

//!
//! Lint posture: this port keeps the mechanical-idiom allows the runner
//! established (numeric casts, exhaustive tables, ownership queries).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity,
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening,
    clippy::large_futures,
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unused_self,
    clippy::doc_lazy_continuation,
    clippy::min_ident_chars,
    clippy::many_single_char_names,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dashmap::DashMap;
use oc_schema::common::RelativePath;
use oc_schema::id::SnapshotId;
use oc_schema::session_support::FileDiff;

/// G-255 — per-repository serialization (ref git.ts:170-172
/// `locks.withLock(repository.gitDirectory)` wrapping the tree
/// capture/write/restore entrypoints): operations on the SAME bare
/// store must serialize across ALL `GitSnapshotStore` instances — two
/// sessions in one repository share `<data>/snapshot/<project>/<hash>`
/// and concurrent capture/restore could otherwise interleave tree
/// writes and stat-cache sidecar updates.
///
/// Entries evict when the last holder releases (`Arc::strong_count ==
/// 1`: only the map itself holds the lock), so the table cannot leak
/// one entry per visited repository.
static REPOSITORY_LOCKS: std::sync::LazyLock<
    DashMap<PathBuf, std::sync::Arc<tokio::sync::Mutex<()>>>,
> = std::sync::LazyLock::new(DashMap::new);

/// G-255 test seam: the concurrency high-water mark observed under the
/// repository lock (tests assert serialization: max stays 1).
#[cfg(test)]
pub(crate) static LOCK_MAX_CONCURRENCY: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
static LOCK_ACTIVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Untracked-file cap (`maximumUntrackedFileBytes`).
pub const MAX_UNTRACKED_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// G-198 — `MAX_TOTAL_PATCH_BYTES` (vcs/patch.ts:11): the cumulative
/// 10MiB patch budget for tree diffs (ref git.ts:522-573).
pub const MAX_TOTAL_PATCH_BYTES: usize = 10_000_000;

// ---------------------------------------------------------------------------
// Hash.fast (SHA-1)
// ---------------------------------------------------------------------------

/// `Hash.fast(input)` — SHA-1 hex digest (G-162②: unified in
/// `oc_util::hash::sha1_hex`; previously hand-rolled here).
#[must_use]
pub fn hash_fast(input: &[u8]) -> String {
    oc_util::hash::sha1_hex(input)
}

// ---------------------------------------------------------------------------
// The service
// ---------------------------------------------------------------------------

/// `Snapshot.Error`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    #[error("Snapshot failed ({operation}): {message}")]
    Operation { operation: &'static str, message: String },
}

/// The async snapshot service port (the runner + revert consumers).
#[async_trait::async_trait]
pub trait SnapshotService: Send + Sync {
    /// Capture the current scoped filesystem state; `None` when disabled or
    /// the best-effort capture fails.
    async fn capture(&self) -> Option<SnapshotId>;
    /// Changed project-relative paths between two trees.
    async fn files(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
    ) -> Result<Vec<RelativePath>, SnapshotError>;
    /// Structured per-file diffs between two trees.
    async fn diff(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
        context: Option<u32>,
        paths: Option<&[RelativePath]>,
    ) -> Result<Vec<FileDiff>, SnapshotError>;
    /// Restore selected paths from their trees (absent ⇒ delete).
    async fn restore(&self, files: &BTreeMap<String, SnapshotId>) -> Result<(), SnapshotError>;
    /// Enable/disable (`Editor.configure`).
    fn configure(&self, enabled: bool);
}

/// The git2-backed snapshot store for one project worktree.
pub struct GitSnapshotStore {
    /// The real worktree (`location.project.directory`).
    worktree: PathBuf,
    /// The location directory scopes capture (`relative(worktree, dir)`).
    scope: String,
    /// `<data>/snapshot/<projectID>/<sha1(worktree)>` — a bare object db.
    git_directory: PathBuf,
    enabled: std::sync::atomic::AtomicBool,
    /// Best-effort logging latch (capture failures degrade to `None`).
    last_error: std::sync::Mutex<Option<String>>,
}

impl std::fmt::Debug for GitSnapshotStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitSnapshotStore")
            .field("worktree", &self.worktree)
            .field("git_directory", &self.git_directory)
            .finish_non_exhaustive()
    }
}

fn operation_error(operation: &'static str, message: impl Into<String>) -> SnapshotError {
    SnapshotError::Operation { operation, message: message.into() }
}

/// G-219② test seam: counts actual file reads (fs::read + read_link)
/// performed by `capture_blocking` — tests assert steady-state captures
/// do ZERO reads and one touched file costs exactly one.
#[cfg(test)]
pub(crate) static CAPTURE_FILE_READS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// G-219① — the SOURCE repository's objects directory (the common dir's
/// `objects`, so linked worktrees share the main store's objects).
fn source_objects_directory(source: &git2::Repository) -> Option<std::path::PathBuf> {
    let path = source.path().to_path_buf();
    let text = path.to_string_lossy();
    let common = match text.rfind("/worktrees/") {
        Some(index) => std::path::PathBuf::from(&text[..=index]),
        None => path,
    };
    let objects = common.join("objects");
    objects.is_dir().then_some(objects)
}

/// G-219② — the stat-cache sidecar (the port's stand-in for the ref's
/// source-index copy, git.ts:364-374): path → (oid, size, mtime, mode).
/// A matching stat (size + mtime + mode, blob still present) reuses the
/// oid without reading the file — `git add --all` stat-identical
/// semantics (snapshot.ts:425-436).
#[derive(Debug, Clone, Copy)]
struct StatEntry {
    oid: git2::Oid,
    size: u64,
    mtime_secs: u64,
    mtime_nanos: u32,
    mode: i32,
}

impl Default for StatEntry {
    fn default() -> Self {
        Self { oid: git2::Oid::zero(), size: 0, mtime_secs: 0, mtime_nanos: 0, mode: 0 }
    }
}

impl StatEntry {
    fn of(oid: git2::Oid, meta: &std::fs::Metadata, mode: i32) -> Self {
        let (mtime_secs, mtime_nanos) = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
            .unwrap_or((0, 0));
        Self { oid, size: meta.len(), mtime_secs, mtime_nanos, mode }
    }

    fn matches(&self, meta: &std::fs::Metadata, mode: i32) -> bool {
        if self.mode != mode || self.size != meta.len() {
            return false;
        }
        let Ok(modified) = meta.modified() else { return false };
        let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) else { return false };
        self.mtime_secs == duration.as_secs() && self.mtime_nanos == duration.subsec_nanos()
    }
}

/// The cache map persisted at `<store>/opencode-stat-cache` — a
/// line-oriented format (`<oid> <size> <secs> <nanos> <mode> <path>`);
/// unknown lines are skipped so a corrupt sidecar degrades to a full
/// (uncached) capture, never a wrong tree.
#[derive(Debug, Default)]
struct StatCache {
    entries: std::collections::BTreeMap<String, StatEntry>,
}

impl StatCache {
    fn parse(text: &str) -> Self {
        let mut entries = std::collections::BTreeMap::new();
        for line in text.lines() {
            let mut parts = line.splitn(6, ' ');
            let (Some(oid), Some(size), Some(secs), Some(nanos), Some(mode), Some(path)) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) else {
                continue;
            };
            let (Ok(oid), Ok(size), Ok(secs), Ok(nanos), Ok(mode)) = (
                git2::Oid::from_str(oid),
                size.parse::<u64>(),
                secs.parse::<u64>(),
                nanos.parse::<u32>(),
                mode.parse::<i32>(),
            ) else {
                continue;
            };
            if path.contains(char::from(0)) || path.is_empty() {
                continue;
            }
            entries.insert(
                path.to_owned(),
                StatEntry { oid, size, mtime_secs: secs, mtime_nanos: nanos, mode },
            );
        }
        Self { entries }
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (path, entry) in &self.entries {
            use std::fmt::Write as _;
            let _ = writeln!(
                out,
                "{} {} {} {} {} {}",
                entry.oid, entry.size, entry.mtime_secs, entry.mtime_nanos, entry.mode, path
            );
        }
        out
    }
}

/// Pins the snapshot store's git configuration (G-30): the ref ships an
/// `opencode.gitconfig` include (`[core] autocrlf = false`, git.ts:23-33)
/// and forces `-c core.quotepath=false` for patch diffs (git.ts:535), so
/// snapshot operations never inherit the HOST's `~/.gitconfig` settings.
/// `repo.config()` opens the full level stack, but `set_str` writes to
/// the highest-priority writable level — the repo-local config file —
/// which then overrides the global/system levels for every libgit2
/// operation on this repository. Already-correct values are left alone
/// so repeated opens do not rewrite the file.
fn apply_snapshot_config(repo: &git2::Repository) -> Result<(), git2::Error> {
    let mut config = repo.config()?;
    for (key, value) in [("core.autocrlf", "false"), ("core.quotepath", "false")] {
        if config.get_string(key).ok().as_deref() != Some(value) {
            config.set_str(key, value)?;
        }
    }
    Ok(())
}

/// G-255 test seam: the process-wide repository lock for `dir`.
#[cfg(test)]
pub(crate) fn repository_lock_for_test(dir: &Path) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    REPOSITORY_LOCKS
        .entry(dir.to_path_buf())
        .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

impl GitSnapshotStore {
    /// Opens (creating) the store for `worktree` under `data_root`.
    /// `scope` is the location directory relative to the worktree (`"."`
    /// for the root).
    pub fn open(
        data_root: &Path,
        project_id: &str,
        worktree: &Path,
        scope: &str,
    ) -> Result<Self, SnapshotError> {
        // Ref snapshot.ts:93-96 hashes `fs.realPath(source.worktree)` —
        // the canonical worktree path, so alias/symlinked invocations
        // share one store. (Resolver's `canonical` is lexical; only the
        // OS realpath used here resolves symlinks.) Fall back to the
        // input path when canonicalization fails (e.g. it does not
        // exist yet).
        let canonical = std::fs::canonicalize(worktree).unwrap_or_else(|_| worktree.to_path_buf());
        let git_directory = data_root
            .join("snapshot")
            .join(project_id)
            .join(hash_fast(canonical.to_string_lossy().as_bytes()));
        // SE-F3 (deferred init): the bare store is created on FIRST
        // USE (`open_repo` below), not here — the reference's
        // Snapshot module is lazy (session-scoped captures only; zero
        // snapshot writes in the boot window). The eager init cost
        // two bare-repo creations per boot even in an EMPTY project
        // (boot fs-churn + E2/TTI coupling).
        Ok(Self {
            worktree: worktree.to_path_buf(),
            scope: scope.to_string(),
            git_directory,
            enabled: std::sync::atomic::AtomicBool::new(true),
            last_error: std::sync::Mutex::new(None),
        })
    }

    /// The store's git directory (diagnostics).
    #[must_use]
    pub fn git_directory(&self) -> &Path {
        &self.git_directory
    }

    /// G-255 — serialize store operations per git directory (ref
    /// git.ts:170-172 `locked()`): every mutating or reading tree op
    /// for this store runs under the process-wide repository lock, so
    /// two sessions sharing a repository cannot interleave
    /// capture/write/restore. The entry self-evicts when this is the
    /// last holder (see [`REPOSITORY_LOCKS`]).
    async fn with_repository_lock<T>(&self, operation: impl std::future::Future<Output = T>) -> T {
        let lock = REPOSITORY_LOCKS
            .entry(self.git_directory.clone())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let guard = lock.lock().await;
        #[cfg(test)]
        {
            let active = LOCK_ACTIVE.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            LOCK_MAX_CONCURRENCY.fetch_max(active, std::sync::atomic::Ordering::SeqCst);
        }
        let result = operation.await;
        #[cfg(test)]
        {
            LOCK_ACTIVE.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
        drop(guard);
        // Drop our clone FIRST: eviction is safe only when no other task
        // holds the Arc (strong count 1 = only the map).
        drop(lock);
        // Eviction: safe only when no other task cloned the Arc (a
        // cloned Arc pins the entry; a task between clone and lock
        // keeps the count > 1, so a fresh inserter can never fork the
        // lock behind a live holder).
        REPOSITORY_LOCKS
            .remove_if(&self.git_directory, |_, held| std::sync::Arc::strong_count(held) == 1);
        result
    }

    fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    /// The full enable gate (ref snapshot.ts:115):
    /// `location.vcs?.type === "git" && state.get().enabled`. The port has
    /// no VCS plugin registry here; a discoverable Git source repository is
    /// the `type === "git"` oracle.
    fn enabled_gate(&self) -> bool {
        self.enabled() && self.source_repo().is_some()
    }

    fn open_repo(&self) -> Result<git2::Repository, git2::Error> {
        if let Ok(repo) = git2::Repository::open_bare(&self.git_directory) {
            apply_snapshot_config(&repo)?;
            self.apply_alternates(&repo);
            return Ok(repo);
        }
        // First use: initialize the bare store (SE-F3's deferred init).
        // A race with another first-use initializer lands in the
        // "attempt to reinitialize" error — fall through to a re-open
        // either way, so exactly one init wins.
        let _ = git2::Repository::init_opts(
            &self.git_directory,
            git2::RepositoryInitOptions::new().bare(true).external_template(false).mkdir(true),
        );
        let repo = git2::Repository::open_bare(&self.git_directory)?;
        apply_snapshot_config(&repo)?;
        self.apply_alternates(&repo);
        Ok(repo)
    }

    /// G-219① — alternates: the store's odb reaches the SOURCE
    /// repository's objects (ref git.ts:345-382 writes
    /// `objects/info/alternates` when seeding a repo from a source; the
    /// snapshot store is the same seed relationship, snapshot.ts:95-105).
    /// The file is written for ref layout parity AND registered
    /// functionally (`Odb::add_disk_alternate`) so reachability holds
    /// even where libgit2 does not auto-load the file — a blob present
    /// only in the source odb must read through the snapshot odb, and a
    /// restore of a seeded snapshot must not re-copy those blobs.
    /// G-30 cross-reference: the store's config isolation
    /// (`apply_snapshot_config`) is the same seed-time hygiene.
    ///
    /// Bounded deviation (G-30/G-219 family): the ref ALSO copies the
    /// source git index so `git add --all` can stat-skip; this port
    /// maintains its own stat-cache sidecar instead (see
    /// [`stat_cache_path`]) — alternates here are for layout parity and
    /// cross-store blob reachability, not for skipping the first
    /// capture's reads.
    fn apply_alternates(&self, repo: &git2::Repository) {
        let Some(source) = self.source_repo() else { return };
        let Some(objects) = source_objects_directory(&source) else { return };
        let line = objects.to_string_lossy().into_owned();
        let info = self.git_directory.join("objects").join("info");
        if std::fs::create_dir_all(&info).is_ok() {
            let alternates = info.join("alternates");
            let current = std::fs::read_to_string(&alternates).unwrap_or_default();
            if current.trim() != line {
                let _ = std::fs::write(&alternates, format!("{line}\n"));
            }
        }
        if let Ok(odb) = repo.odb() {
            let _ = odb.add_disk_alternate(&line);
        }
    }

    /// The SOURCE repository (ignore-rule oracle): `git.repo.discover`.
    fn source_repo(&self) -> Option<git2::Repository> {
        git2::Repository::discover(&self.worktree).ok()
    }

    /// Whether `path` (worktree-relative, `/`-separated) is ignored by the
    /// SOURCE repository's rules (`check-ignore --no-index` semantics: pure
    /// pathname rules; libgit2's `is_ignored` matches).
    fn is_ignored(&self, source: &git2::Repository, path: &str) -> bool {
        source.is_path_ignored(Path::new(path)).unwrap_or(false)
    }

    /// Whether `path` is tracked in the source repository's index or HEAD —
    /// tracked files are always snapshotted regardless of ignore rules
    /// (`git diff-files` + `add --all` semantics).
    fn is_tracked(&self, source: &git2::Repository, path: &str) -> bool {
        if let Ok(index) = source.index() {
            if index.get_path(Path::new(path), 0).is_some() {
                return true;
            }
        }
        false
    }

    /// `capture` — walk the scope, write blobs, build the tree.
    pub fn capture_blocking(&self) -> Result<SnapshotId, SnapshotError> {
        if !self.enabled() {
            return Err(operation_error("capture", "Snapshots are disabled"));
        }
        // v2's snapshot service resolves the SOURCE repository first
        // (`git.repo.discover(project.directory)`); outside a Git
        // repository capture is undefined — no snapshot fields ride the
        // step events (parity goldens pin the shapes).
        if self.source_repo().is_none() {
            return Err(operation_error("capture", "Project is not a Git repository"));
        }
        let repo = self.open_repo().map_err(|error| operation_error("capture", error.message()))?;
        let source = self.source_repo();
        let scope_path = if self.scope == "." {
            self.worktree.clone()
        } else {
            self.worktree.join(self.scope.trim_end_matches('/'))
        };
        // Entries under the scope, relative to the worktree.
        let mut files: BTreeMap<String, (git2::Oid, i32)> = BTreeMap::new();
        // G-219② — the stat-cache sidecar stands in for the ref's git
        // index copy (git.ts:364-374): a file whose stat matches the
        // cached entry reuses its oid, skipping the read + blob write —
        // steady-state capture is O(changed) reads like `git add --all`
        // (snapshot.ts:425-436).
        let cache = self.load_stat_cache();
        let mut next_cache = StatCache::default();
        let mut stack: Vec<PathBuf> = vec![scope_path.clone()];
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(&dir)
                .map_err(|error| operation_error("capture", error.to_string()))?;
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy().into_owned();
                if name == ".git" {
                    continue;
                }
                let absolute = dir.join(&name);
                let Ok(file_type) = entry.file_type() else { continue };
                let relative = absolute
                    .strip_prefix(&self.worktree)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| name.clone());
                if file_type.is_dir() {
                    stack.push(absolute);
                    continue;
                }
                if let Some(source) = &source {
                    let tracked = self.is_tracked(source, &relative);
                    if !tracked && self.is_ignored(source, &relative) {
                        continue;
                    }
                    if !tracked {
                        if let Ok(meta) = entry.metadata() {
                            if meta.is_file() && meta.len() > MAX_UNTRACKED_FILE_BYTES {
                                continue;
                            }
                        }
                    }
                }
                // The stat comes from the DIRENT (lstat semantics): for
                // symlinks size/mtime describe the LINK, matching the
                // blob content the cache keys on.
                let Ok(meta) = std::fs::symlink_metadata(&absolute) else { continue };
                let mode = if file_type.is_symlink() {
                    0o120000
                } else {
                    use std::os::unix::fs::PermissionsExt as _;
                    if meta.permissions().mode() & 0o111 != 0 {
                        0o100755
                    } else {
                        0o100644
                    }
                };
                // G-219② — stat-identical file: reuse the cached oid
                // (blob existence re-checked so a pruned store cannot
                // fabricate a tree), skip the read entirely.
                if let Some(entry) = cache.entries.get(&relative) {
                    if entry.matches(&meta, mode) && repo.find_blob(entry.oid).is_ok() {
                        files.insert(relative.clone(), (entry.oid, mode));
                        next_cache.entries.insert(relative, *entry);
                        continue;
                    }
                }
                // G-50: symlink entries snapshot the LINK TARGET string as
                // the blob content (mode 120000) — `git checkout <tree>`
                // semantics; a plain `fs::read` would follow the link.
                #[cfg(test)]
                CAPTURE_FILE_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let bytes: Vec<u8> = if file_type.is_symlink() {
                    match std::fs::read_link(&absolute) {
                        Ok(target) => target.to_string_lossy().into_owned().into_bytes(),
                        Err(_) => continue,
                    }
                } else {
                    match std::fs::read(&absolute) {
                        Ok(bytes) => bytes,
                        Err(_) => continue,
                    }
                };
                let Ok(blob) = repo.blob(&bytes) else { continue };
                let entry = StatEntry::of(blob, &meta, mode);
                next_cache.entries.insert(relative.clone(), entry);
                files.insert(relative, (blob, mode));
            }
        }
        // Persist the fresh cache (entries for vanished paths dropped by
        // construction — only walked paths are re-inserted).
        self.save_stat_cache(&next_cache);
        let tree = build_tree(&repo, &files, "")?;
        Ok(SnapshotId::parse(tree.to_string()))
    }

    /// G-219② — the sidecar path inside the store's git directory.
    fn stat_cache_path(&self) -> PathBuf {
        self.git_directory.join("opencode-stat-cache")
    }

    fn load_stat_cache(&self) -> StatCache {
        let Ok(text) = std::fs::read_to_string(self.stat_cache_path()) else {
            return StatCache::default();
        };
        StatCache::parse(&text)
    }

    fn save_stat_cache(&self, cache: &StatCache) {
        let target = self.stat_cache_path();
        let temporary = self.git_directory.join("opencode-stat-cache.tmp");
        if std::fs::write(&temporary, cache.render()).is_ok() {
            let _ = std::fs::rename(&temporary, &target);
        }
    }

    fn tree_diff_paths(
        &self,
        repo: &git2::Repository,
        from: &str,
        to: &str,
    ) -> Result<Vec<String>, SnapshotError> {
        let from_tree = find_tree(repo, from)?;
        let to_tree = find_tree(repo, to)?;
        // Paths present in either tree with differing blob ids (no renames:
        // both sides of a rename list separately).
        let from_map = tree_map(repo, &from_tree);
        let to_map = tree_map(repo, &to_tree);
        let mut changed: Vec<String> = from_map
            .keys()
            .chain(to_map.keys())
            .filter(|path| from_map.get(*path) != to_map.get(*path))
            .cloned()
            .collect();
        changed.sort();
        changed.dedup();
        Ok(changed)
    }

    /// Filters `paths` through the SOURCE repository's ignore rules.
    fn filter_ignored(&self, paths: Vec<String>) -> Vec<String> {
        let Some(source) = self.source_repo() else { return paths };
        paths.into_iter().filter(|path| !self.is_ignored(&source, path)).collect()
    }

    /// `files` — changed paths, source-ignored paths dropped.
    pub fn files_blocking(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
    ) -> Result<Vec<RelativePath>, SnapshotError> {
        // F-16a: ref's `files` resolves the repository first
        // (snapshot.ts:130-137 comparison → `git.repo` discover/create);
        // on a non-git location that resolution FAILS (`failure("files",
        // …)`) — it never initializes a bare store. Gate on the source
        // repo before `open_repo` so a bogus call cannot create one.
        if self.source_repo().is_none() {
            return Err(operation_error("files", "Project is not a Git repository"));
        }
        let repo = self.open_repo().map_err(|error| operation_error("files", error.message()))?;
        let changed = self.tree_diff_paths(&repo, from.as_str(), to.as_str())?;
        Ok(self.filter_ignored(changed).into_iter().map(RelativePath::parse).collect())
    }

    /// `diff` — per-file unified diffs with additions/deletions counts.
    /// `context` is the `-U`/`--unified` width (`git diff --unified=${n}`,
    /// ref git.ts:535).
    pub fn diff_blocking(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
        context: u32,
        paths: Option<&[RelativePath]>,
    ) -> Result<Vec<FileDiff>, SnapshotError> {
        if let Some([]) = paths {
            return Ok(Vec::new());
        }
        // F-16a: same repository-resolution gate as `files` — ref's
        // `comparison("diff", …)` fails on a non-git location
        // (snapshot.ts:155-163) before any store access.
        if self.source_repo().is_none() {
            return Err(operation_error("diff", "Project is not a Git repository"));
        }
        let repo = self.open_repo().map_err(|error| operation_error("diff", error.message()))?;
        let from_tree = find_tree(&repo, from.as_str())?;
        let to_tree = find_tree(&repo, to.as_str())?;
        let selected: Option<Vec<String>> =
            paths.map(|paths| paths.iter().map(|p| p.0.clone()).collect());
        let changed = self.tree_diff_paths(&repo, from.as_str(), to.as_str())?;
        let mut diffs: Vec<FileDiff> = Vec::new();
        // G-198 — `MAX_TOTAL_PATCH_BYTES` (vcs/patch.ts:11): the
        // cumulative patch budget; files past the cap get an empty
        // patch header (ref git.ts:522-573 treeDiff — the capped git
        // diff output leaves their chunk missing → emptyPatch).
        let mut total: usize = 0;
        let mut capped = false;
        for path in changed {
            if let Some(selected) = &selected {
                if !selected.contains(&path) {
                    continue;
                }
            }
            let before = blob_content(&repo, &from_tree, &path);
            let after = blob_content(&repo, &to_tree, &path);
            let status = match (&before, &after) {
                (None, Some(_)) => oc_schema::session_support::FileDiffStatus::Added,
                (Some(_), None) => oc_schema::session_support::FileDiffStatus::Deleted,
                _ => oc_schema::session_support::FileDiffStatus::Modified,
            };
            // G-198 — binary detection (ref's numstat "-"): a binary
            // blob on either side yields `{additions: 0, deletions: 0,
            // patch: ""}` (git.ts:560-573), never a garbage text diff.
            let binary =
                blob_is_binary(&repo, &from_tree, &path) || blob_is_binary(&repo, &to_tree, &path);
            if binary {
                diffs.push(FileDiff {
                    file: path,
                    patch: String::new(),
                    additions: 0,
                    deletions: 0,
                    status,
                });
                continue;
            }
            let diff = super::tool::file_diff::file_diff_ctx(
                &path,
                before.as_deref().unwrap_or(""),
                after.as_deref().unwrap_or(""),
                status,
                context as usize,
            );
            let diff = if capped {
                // Past the budget: real counts (numstat side), empty
                // patch header — ref `patches.get(file) ?? emptyPatch`.
                FileDiff {
                    patch: super::tool::file_diff::two_files_patch_ctx(&path, "", "", 0),
                    ..diff
                }
            } else if total + diff.patch.len() > MAX_TOTAL_PATCH_BYTES {
                capped = true;
                FileDiff {
                    patch: super::tool::file_diff::two_files_patch_ctx(&path, "", "", 0),
                    ..diff
                }
            } else {
                total += diff.patch.len();
                capped = total >= MAX_TOTAL_PATCH_BYTES;
                diff
            };
            diffs.push(FileDiff { file: path, ..diff });
        }
        Ok(diffs)
    }

    /// `restore` — write each path from its tree; absent ⇒ delete.
    pub fn restore_blocking(
        &self,
        files: &BTreeMap<String, SnapshotId>,
    ) -> Result<(), SnapshotError> {
        if !self.enabled_gate() {
            return Err(operation_error("restore", "Snapshots are disabled"));
        }
        let repo = self.open_repo().map_err(|error| operation_error("restore", error.message()))?;
        for (path, tree_id) in files {
            let absolute =
                crate::file_access::lexical_resolve(Path::new("/"), &self.worktree.join(path));
            if !absolute.starts_with(&self.worktree) {
                return Err(operation_error(
                    "restore",
                    format!("Path escapes the project: {path}"),
                ));
            }
            match find_tree(&repo, tree_id.as_str())
                .ok()
                .and_then(|tree| tree.get_path(Path::new(path)).ok())
                .filter(|entry| entry.kind() == Some(git2::ObjectType::Blob))
            {
                Some(entry) => {
                    let blob = repo
                        .find_blob(entry.id())
                        .map_err(|error| operation_error("restore", error.message()))?;
                    if let Some(parent) = absolute.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|error| operation_error("restore", error.to_string()))?;
                    }
                    // G-50: honor the tree entry's filemode, matching
                    // `git checkout <tree> -- <path>` (ref git.ts:601-606):
                    // 120000 ⇒ symlink whose target is the blob content;
                    // 100755 ⇒ executable bit; anything else ⇒ plain file.
                    let mode = entry.filemode();
                    #[cfg(unix)]
                    if mode == 0o120000 {
                        use std::os::unix::ffi::OsStringExt as _;
                        let target = std::ffi::OsString::from_vec(blob.content().to_vec());
                        // Remove whatever occupies the path first (git
                        // checkout overwrites files/symlinks alike).
                        remove_recursive(&absolute)
                            .map_err(|error| operation_error("restore", error.to_string()))?;
                        std::os::unix::fs::symlink(target, &absolute)
                            .map_err(|error| operation_error("restore", error.to_string()))?;
                        continue;
                    }
                    std::fs::write(&absolute, blob.content())
                        .map_err(|error| operation_error("restore", error.to_string()))?;
                    #[cfg(unix)]
                    if mode == 0o100755 {
                        use std::os::unix::fs::PermissionsExt as _;
                        let perms = std::fs::Permissions::from_mode(0o755);
                        std::fs::set_permissions(&absolute, perms)
                            .map_err(|error| operation_error("restore", error.to_string()))?;
                    }
                }
                None => {
                    // Path absent from its tree ⇒ remove (recursive force).
                    remove_recursive(&absolute)
                        .map_err(|error| operation_error("restore", error.to_string()))?;
                }
            }
        }
        Ok(())
    }
}

fn remove_recursive(path: &Path) -> std::io::Result<()> {
    if path.symlink_metadata().is_ok() {
        if path.is_dir() && !path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        }
    } else {
        Ok(())
    }
}

fn find_tree<'repo>(
    repo: &'repo git2::Repository,
    id: &str,
) -> Result<git2::Tree<'repo>, SnapshotError> {
    let oid = git2::Oid::from_str(id).map_err(|error| operation_error("files", error.message()))?;
    let object = repo
        .find_object(oid, Some(git2::ObjectType::Tree))
        .map_err(|error| operation_error("files", error.message()))?;
    object.peel_to_tree().map_err(|error| operation_error("files", error.message()))
}

fn tree_map(repo: &git2::Repository, tree: &git2::Tree<'_>) -> BTreeMap<String, git2::Oid> {
    let mut map = BTreeMap::new();
    walk_tree_paths(repo, tree, "", &mut |path, oid| {
        map.insert(path.to_string(), oid);
    });
    map
}

fn walk_tree_paths(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
    visit: &mut dyn FnMut(&str, git2::Oid),
) {
    for entry in tree.iter() {
        let name = entry.name().unwrap_or_default();
        let path = if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
        match entry.kind() {
            Some(git2::ObjectType::Tree) => {
                if let Ok(subtree) = entry.to_object(repo).and_then(|o| o.peel_to_tree()) {
                    walk_tree_paths(repo, &subtree, &path, visit);
                }
            }
            _ => visit(&path, entry.id()),
        }
    }
}

fn blob_content(repo: &git2::Repository, tree: &git2::Tree<'_>, path: &str) -> Option<String> {
    let entry = tree.get_path(Path::new(path)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    Some(String::from_utf8_lossy(blob.content()).into_owned())
}

/// G-198 — whether the tree's blob at `path` is binary (git2's
/// NUL-sniff, the same signal as `git diff --numstat` printing `-`).
fn blob_is_binary(repo: &git2::Repository, tree: &git2::Tree<'_>, path: &str) -> bool {
    tree.get_path(Path::new(path))
        .ok()
        .and_then(|entry| repo.find_blob(entry.id()).ok())
        .is_some_and(|blob| blob.is_binary())
}

fn build_tree(
    repo: &git2::Repository,
    files: &BTreeMap<String, (git2::Oid, i32)>,
    prefix: &str,
) -> Result<git2::Oid, SnapshotError> {
    // Group entries one level under `prefix` and recurse.
    let mut direct: Vec<(String, git2::Oid, i32)> = Vec::new();
    let mut subtrees: BTreeMap<String, BTreeMap<String, (git2::Oid, i32)>> = BTreeMap::new();
    let prefix_len = if prefix.is_empty() { 0 } else { prefix.len() + 1 };
    for (path, (oid, mode)) in files {
        if !prefix.is_empty() && !path.starts_with(&format!("{prefix}/")) {
            continue;
        }
        let rest = &path[prefix_len..];
        match rest.split_once('/') {
            Some((dir, _)) => {
                subtrees.entry(dir.to_string()).or_default().insert(path.clone(), (*oid, *mode));
            }
            None => direct.push((rest.to_string(), *oid, *mode)),
        }
    }
    let mut builder =
        repo.treebuilder(None).map_err(|error| operation_error("capture", error.message()))?;
    for (name, oid, mode) in direct {
        builder
            .insert(name, oid, mode)
            .map_err(|error| operation_error("capture", error.message()))?;
    }
    for (dir, children) in subtrees {
        let child_prefix = if prefix.is_empty() { dir.clone() } else { format!("{prefix}/{dir}") };
        let subtree = build_tree(repo, &children, &child_prefix)?;
        builder
            .insert(dir, subtree, 0o040000)
            .map_err(|error| operation_error("capture", error.message()))?;
    }
    builder.write().map_err(|error| operation_error("capture", error.message()))
}

/// Async wrapper (the service port). Blocking git work runs on the caller's
/// runtime through [`tokio::task::block_place`]... simplest correct shape:
/// the async trait methods delegate synchronously (git ops here are
/// directory-sized, not network).
#[async_trait::async_trait]
impl SnapshotService for GitSnapshotStore {
    async fn capture(&self) -> Option<SnapshotId> {
        // Ref snapshot.ts:117-118: a non-git VCS (or a disabled snapshot
        // state) captures NOTHING — `undefined` without even a warning.
        if !self.enabled_gate() {
            return None;
        }
        // G-255 — capture runs under the per-repository lock.
        self.with_repository_lock(async {
            match self.capture_blocking() {
                Ok(id) => Some(id),
                Err(error) => {
                    *self.last_error.lock().unwrap_or_else(|p| p.into_inner()) =
                        Some(error.to_string());
                    None
                }
            }
        })
        .await
    }

    async fn files(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
    ) -> Result<Vec<RelativePath>, SnapshotError> {
        // G-255 — ref `locked()` wraps the read-side tree ops too.
        let (from, to) = (from.clone(), to.clone());
        self.with_repository_lock(async move { self.files_blocking(&from, &to) }).await
    }

    async fn diff(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
        context: Option<u32>,
        paths: Option<&[RelativePath]>,
    ) -> Result<Vec<FileDiff>, SnapshotError> {
        // `--unified=${input.context ?? 3}` (ref git.ts:535).
        let context = context.unwrap_or(super::tool::file_diff::GIT_DEFAULT_CONTEXT as u32);
        // G-255 — ref `locked()` wraps the read-side tree ops too.
        let (from, to) = (from.clone(), to.clone());
        let paths = paths.map(|paths| paths.to_vec());
        self.with_repository_lock(async move {
            self.diff_blocking(&from, &to, context, paths.as_deref())
        })
        .await
    }

    async fn restore(&self, files: &BTreeMap<String, SnapshotId>) -> Result<(), SnapshotError> {
        // G-255 — restore runs under the per-repository lock.
        let files = files.clone();
        self.with_repository_lock(async move { self.restore_blocking(&files) }).await
    }

    fn configure(&self, enabled: bool) {
        self.enabled.store(enabled, std::sync::atomic::Ordering::Release);
    }
}

/// The location scope relative to the worktree (`snapshot.ts scope()`).
#[must_use]
pub fn location_scope(worktree: &Path, directory: &Path) -> Option<String> {
    let relative = directory.strip_prefix(worktree).ok()?;
    let text = relative.to_string_lossy().replace('\\', "/");
    Some(if text.is_empty() { ".".to_string() } else { text })
}

/// Test/inspection helper: the paths inside a captured tree.
#[cfg(test)]
pub(crate) fn snapshot_files_of(
    data_root: &Path,
    worktree: &Path,
    tree: &SnapshotId,
) -> Vec<String> {
    let git_directory = data_root
        .join("snapshot")
        .join("proj_snap")
        .join(hash_fast(worktree.to_string_lossy().as_bytes()));
    let repo = git2::Repository::open_bare(&git_directory).expect("open store");
    let oid = git2::Oid::from_str(tree.as_str()).expect("tree id");
    let object = repo.find_object(oid, Some(git2::ObjectType::Tree)).expect("tree");
    let tree = object.peel_to_tree().expect("peel");
    let mut paths = Vec::new();
    fn walk(repo: &git2::Repository, tree: &git2::Tree<'_>, prefix: &str, out: &mut Vec<String>) {
        for entry in tree.iter() {
            let name = entry.name().unwrap_or_default();
            let path =
                if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
            match entry.kind() {
                Some(git2::ObjectType::Tree) => {
                    if let Ok(sub) = entry.to_object(repo).and_then(|o| o.peel_to_tree()) {
                        walk(repo, &sub, &path, out);
                    }
                }
                _ => out.push(path),
            }
        }
    }
    walk(&repo, &tree, "", &mut paths);
    paths.sort();
    paths
}

#[cfg(test)]
mod tests;
