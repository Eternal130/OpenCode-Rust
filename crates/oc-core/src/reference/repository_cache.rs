//! Port of `packages/core/src/repository-cache.ts` (v2.0.24): local
//! tracking checkouts for remote Git references, one per remote and
//! branch. Each checkout permanently tracks a single ref — the requested
//! branch when the cache key has one, otherwise the remote default
//! branch. Content follows "newest wins": refresh fetches and
//! hard-resets, so readers may observe the checkout move underneath them.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use oc_util::determinism::Clock;
use parking_lot::Mutex;

use super::repository::{
    cache_path, parse, same, validate_branch, Reference, RemoteReference, RepositoryError,
};

/// `refreshInterval` — one day (repository-cache.ts:23).
pub const REFRESH_INTERVAL_MS: i64 = 86_400_000;

/// `EnsureInput.refresh` — `daily` throttles existing checkouts, `true`
/// forces a refresh, `false` only materializes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// `refresh: "daily"`.
    Daily,
    /// `refresh: true`.
    Forced,
    /// `refresh` unset.
    IfMissing,
}

/// The `ensure` input.
pub struct EnsureInput {
    pub reference: RemoteReference,
    pub branch: Option<String>,
    pub refresh: Refresh,
}

/// `RepositoryCache.Result`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheResult {
    pub repository: String,
    pub host: String,
    pub remote: String,
    pub local_path: PathBuf,
    pub status: &'static str, // "cached" | "cloned" | "refreshed"
    pub head: Option<String>,
    pub branch: Option<String>,
}

/// The cache's error surface (`RepositoryCache.Error`).
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("invalid branch {branch:?}: {message}")]
    InvalidBranch { branch: String, message: String },
    #[error("failed to clone {repository:?}: {message}")]
    CloneFailed { repository: String, message: String },
    #[error("failed to fetch {repository:?}: {message}")]
    FetchFailed { repository: String, message: String },
    #[error("failed to checkout {branch:?} of {repository:?}: {message}")]
    CheckoutFailed { repository: String, branch: String, message: String },
    #[error("failed to reset {repository:?}: {message}")]
    ResetFailed { repository: String, message: String },
    #[error("failed to lock {path:?}: {message}")]
    LockFailed { path: PathBuf, message: String },
    #[error("{operation} failed at {path:?}: {message}")]
    CacheOperation { operation: String, path: PathBuf, message: String },
}

/// The persisted refresh-attempt records (`KV` with key
/// `repository-cache:<localPath>`); injectable so tests avoid the shared
/// store and network.
pub trait RefreshStore: Send + Sync {
    /// The persisted `attemptedAt` for a cache key.
    fn attempted_at(&self, key: &str) -> Option<i64>;
    /// Records an attempt (before any network work, so failures obey the
    /// same refresh interval).
    fn set_attempted_at(&self, key: &str, unix_ms: i64);
    /// Drops a record.
    fn remove(&self, key: &str);
}

/// In-memory store (tests).
#[derive(Default)]
pub struct MemoryRefreshStore(Mutex<std::collections::BTreeMap<String, i64>>);

impl RefreshStore for MemoryRefreshStore {
    fn attempted_at(&self, key: &str) -> Option<i64> {
        self.0.lock().get(key).copied()
    }
    fn set_attempted_at(&self, key: &str, unix_ms: i64) {
        self.0.lock().insert(key.to_owned(), unix_ms);
    }
    fn remove(&self, key: &str) {
        self.0.lock().remove(key);
    }
}

/// File-backed store: `<repos>/.repository-cache.json`, mirroring the ref's
/// shared KV records under the same `repository-cache:` keys.
pub struct FileRefreshStore {
    path: PathBuf,
    state: Mutex<std::collections::BTreeMap<String, i64>>,
}

impl FileRefreshStore {
    pub fn new(repos_root: &Path) -> Self {
        Self {
            path: repos_root.join(".repository-cache.json"),
            state: Mutex::new(Self::load(&repos_root.join(".repository-cache.json"))),
        }
    }

    fn load(path: &Path) -> std::collections::BTreeMap<String, i64> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn persist(&self, state: &std::collections::BTreeMap<String, i64>) {
        if let Ok(text) = serde_json::to_string(state) {
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&self.path, text);
        }
    }
}

impl RefreshStore for FileRefreshStore {
    fn attempted_at(&self, key: &str) -> Option<i64> {
        self.state.lock().get(key).copied()
    }
    fn set_attempted_at(&self, key: &str, unix_ms: i64) {
        let mut state = self.state.lock();
        state.insert(key.to_owned(), unix_ms);
        self.persist(&state);
    }
    fn remove(&self, key: &str) {
        let mut state = self.state.lock();
        state.remove(key);
        self.persist(&state);
    }
}

/// The cache service: `ensure` materializes/refreshes one checkout under
/// the repos root, serialized per path by an exclusive lock.
pub struct RepositoryCache {
    repos_root: PathBuf,
    clock: Arc<dyn Clock>,
    store: Arc<dyn RefreshStore>,
    lock_timeout: std::time::Duration,
}

impl RepositoryCache {
    pub fn new(repos_root: PathBuf, clock: Arc<dyn Clock>, store: Arc<dyn RefreshStore>) -> Self {
        Self { repos_root, clock, store, lock_timeout: std::time::Duration::from_secs(60) }
    }

    /// `ensure` (repository-cache.ts:128-222).
    pub fn ensure(&self, input: &EnsureInput) -> Result<CacheResult, CacheError> {
        if let Some(branch) = &input.branch {
            validate_branch(branch).map_err(|error| CacheError::InvalidBranch {
                branch: error.branch,
                message: error.message,
            })?;
        }

        let repository = input.reference.label.clone();
        let local_path = cache_path(
            &self.repos_root,
            &Reference::Remote(input.reference.clone()),
            input.branch.as_deref(),
        );
        let key = format!("repository-cache:{}", local_path.to_string_lossy());
        let clone_target =
            parse(&input.reference.remote).unwrap_or(Reference::Remote(input.reference.clone()));

        let lock_path = {
            let mut name = local_path.clone().into_os_string();
            name.push(".lock");
            PathBuf::from(name)
        };
        let _lock = oc_util::flock::FileLock::exclusive(&lock_path, self.lock_timeout).map_err(
            |error| CacheError::LockFailed { path: lock_path.clone(), message: error.to_string() },
        )?;

        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| CacheError::CacheOperation {
                operation: "ensure cache directory".to_owned(),
                path: parent.to_owned(),
                message: error.to_string(),
            })?;
        }

        let existing = discover(&local_path);
        let origin =
            existing.as_ref().and_then(|path| git_at(path, &["remote", "get-url", "origin"]).ok());
        let origin_reference = origin.as_deref().and_then(parse);
        // Discovery walks upward, so an enclosing repository with a
        // matching origin could masquerade as the cache entry; reuse
        // requires the checkout to live exactly at the cache path.
        let worktree =
            existing.is_some().then(|| std::fs::canonicalize(&local_path).ok()).flatten();
        let reuse = matches!(
            (&existing, &origin_reference, &worktree),
            (Some(existing), Some(origin_reference), Some(worktree))
                if *existing == *worktree && same(origin_reference, &clone_target)
        );
        if !reuse && local_path.exists() {
            std::fs::remove_dir_all(&local_path).map_err(|error| CacheError::CacheOperation {
                operation: "remove stale cache".to_owned(),
                path: local_path.clone(),
                message: error.to_string(),
            })?;
        }

        let now = self.clock.unix_ms();
        let previous = self.store.attempted_at(&key);
        let refresh = match input.refresh {
            Refresh::Daily => previous.is_none_or(|at| now - at >= REFRESH_INTERVAL_MS),
            Refresh::Forced => true,
            Refresh::IfMissing => false,
        };
        let status: &'static str = if !reuse {
            "cloned"
        } else if refresh {
            "refreshed"
        } else {
            "cached"
        };

        if status != "cached" {
            // Record attempts before network work so failures obey the
            // same refresh interval.
            self.store.set_attempted_at(&key, now);

            if status == "cloned" {
                let mut args = vec!["clone".to_owned(), input.reference.remote.clone()];
                if let Some(branch) = &input.branch {
                    args.push("--branch".to_owned());
                    args.push(branch.clone());
                }
                args.push(local_path.to_string_lossy().into_owned());
                git(None, &args).map_err(|error| CacheError::CloneFailed {
                    repository: repository.clone(),
                    message: error,
                })?;
            }

            if status == "refreshed" {
                let existing = existing.clone().ok_or_else(|| CacheError::FetchFailed {
                    repository: repository.clone(),
                    message: "Repository is unavailable".to_owned(),
                })?;
                git_at(&existing, &["fetch", "origin"]).map_err(|error| {
                    CacheError::FetchFailed { repository: repository.clone(), message: error }
                })?;
                if let Some(branch) = &input.branch {
                    git_at(&existing, &["fetch", "origin", branch]).map_err(|error| {
                        CacheError::FetchFailed { repository: repository.clone(), message: error }
                    })?;
                }

                // Checking out the tracked ref before resetting keeps the
                // checkout self-healing even if it was left on another
                // branch.
                let branch = match &input.branch {
                    Some(branch) => Some(branch.clone()),
                    None => default_remote_branch(&existing),
                };
                if let Some(branch) = &branch {
                    git_at(&existing, &["checkout", "-B", branch, &format!("origin/{branch}")])
                        .map_err(|error| CacheError::CheckoutFailed {
                            repository: repository.clone(),
                            branch: branch.clone(),
                            message: error,
                        })?;
                }

                let target = branch.clone().or_else(|| current_branch(&existing));
                let reference = target
                    .as_deref()
                    .map(|target| format!("origin/{target}"))
                    .unwrap_or_else(|| "HEAD".to_owned());
                git_at(&existing, &["reset", "--hard", &reference]).map_err(|error| {
                    CacheError::ResetFailed { repository: repository.clone(), message: error }
                })?;
            }
        }

        let checkout = discover(&local_path);
        Ok(CacheResult {
            repository,
            host: input.reference.host.clone(),
            remote: input.reference.remote.clone(),
            local_path,
            status,
            head: checkout.as_deref().and_then(|path| git_at(path, &["rev-parse", "HEAD"]).ok()),
            branch: checkout
                .as_deref()
                .and_then(|path| git_at(path, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()),
        })
    }
}

/// `git.repo.discover` bounded to the exact directory (the cache requires
/// the checkout to live at the path; upward discovery results are checked
/// by the caller).
fn discover(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    let top = git_at(path, &["rev-parse", "--show-toplevel"]).ok()?;
    let top = PathBuf::from(top.trim());
    let top = std::fs::canonicalize(&top).ok()?;
    let here = std::fs::canonicalize(path).ok()?;
    (top == here).then_some(here)
}

/// `git.history.defaultRemoteBranch` — `origin/HEAD` if configured.
fn default_remote_branch(path: &Path) -> Option<String> {
    let reference = git_at(path, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).ok()?;
    reference.strip_prefix("origin/").map(str::to_owned)
}

fn current_branch(path: &Path) -> Option<String> {
    git_at(path, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()
}

/// One git invocation in `path` (`git -C path args…`).
fn git(path: Option<&Path>, args: &[String]) -> Result<String, String> {
    let mut command = Command::new("git");
    if let Some(path) = path {
        command.arg("-C").arg(path);
    }
    run(command.args(args))
}

fn git_at(path: &Path, args: &[&str]) -> Result<String, String> {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    git(Some(path), &owned)
}

fn run(command: &mut Command) -> Result<String, String> {
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

impl From<RepositoryError> for CacheError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::InvalidBranch(error) => {
                CacheError::InvalidBranch { branch: error.branch, message: error.message }
            }
            RepositoryError::UnsupportedLocal(error) => {
                CacheError::CloneFailed { repository: error.repository, message: error.message }
            }
            RepositoryError::InvalidReference(error) => {
                CacheError::CloneFailed { repository: error.repository, message: error.message }
            }
        }
    }
}
