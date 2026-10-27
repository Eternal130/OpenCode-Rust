//! Location activity detection — port of `filesystem/location-watcher.ts`
//! + `filesystem/location-watcher-policy.ts` (v2.0.24).
//!
//! REF semantics: per location, resolve the VCS "target" file — git:
//! `<gitdir>/HEAD` (gitdir realpath'd, discovered via the repo), hg:
//! `<store>/branch` — and watch exactly that file. Updates publish
//! `FileSystem.Event.Changed` `{file, event}` on the location's bus and
//! re-arm the location's activity so idle locations reset their TTL
//! window. A configurable ignore list (`watcher.ignore` config) can
//! suppress the watch; policy changes reconcile the subscription. The
//! handle stops cleanly on drop (`stop()`), tearing the native watch down
//! with it — no leaked threads.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::bus::Bus;
use crate::config::watch::WatchTarget;
use crate::filesystem::watcher::{publish_changed, FileWatcher};
use crate::location::LocationRef;
use crate::location_services::LocationActivity;
use crate::project::Vcs;

// =============================================================================
// Policy (location-watcher-policy.ts)
// =============================================================================

/// The mutable ignore list (`LocationWatcherPolicy`): `watcher.ignore`
/// entries collected from config documents; changes notify observers so
/// the location watcher reconciles.
#[derive(Default)]
pub struct LocationWatcherPolicy {
    ignore: RwLock<Vec<String>>,
    version: tokio::sync::watch::Sender<u64>,
}

impl LocationWatcherPolicy {
    /// Empty policy.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            ignore: RwLock::new(Vec::new()),
            version: tokio::sync::watch::Sender::new(0),
        })
    }

    /// Adds ignore entries (deduped, sorted — REF's plan keys normalize
    /// the same way) and notifies observers.
    pub fn add(&self, entries: &[String]) {
        if entries.is_empty() {
            return;
        }
        let mut ignore = self.ignore.write();
        for entry in entries {
            if !ignore.contains(entry) {
                ignore.push(entry.clone());
            }
        }
        ignore.sort();
        drop(ignore);
        self.version.send_modify(|version| *version += 1);
    }

    /// Current ignore list.
    #[must_use]
    pub fn current(&self) -> Vec<String> {
        self.ignore.read().clone()
    }

    /// Observer of policy changes (a `watch` receiver that fires on every
    /// `add`).
    pub fn observe(self: &Arc<Self>) -> tokio::sync::watch::Receiver<u64> {
        self.version.subscribe()
    }
}

// =============================================================================
// Target resolution (location-watcher.ts:33-51)
// =============================================================================

/// The resolved VCS watch target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchTargetFile {
    /// The file to watch (`<gitdir>/HEAD` / `<store>/branch`).
    pub path: PathBuf,
    /// Path spellings the ignore list may match against (REF `aliases`).
    pub aliases: Vec<String>,
}

/// Resolves the location's VCS target: git → discovered git directory
/// (realpath'd) + `HEAD`; hg → store (realpath'd) + `branch`. `None` when
/// the location has no VCS or discovery fails (REF logs and resolves
/// `undefined`).
#[must_use]
pub fn resolve_target(directory: &Path, vcs: Option<&Vcs>) -> Option<WatchTargetFile> {
    match vcs? {
        Vcs::Git { .. } => {
            let repo = git2::Repository::discover(directory).ok()?;
            let resolved = repo.path().to_path_buf();
            let vcs_dir = real_path(&resolved);
            let mut aliases = alias_strings(&[vcs_dir.clone(), resolved.clone()]);
            aliases.push(".git".to_string());
            Some(WatchTargetFile { path: vcs_dir.join("HEAD"), aliases })
        }
        Vcs::Hg { store } => {
            let vcs_dir = real_path(store);
            let mut aliases = alias_strings(std::slice::from_ref(&vcs_dir));
            aliases.push(".hg".to_string());
            Some(WatchTargetFile { path: vcs_dir.join("branch"), aliases })
        }
    }
}

/// `fs.realPath` with the input as fallback (REF `orElseSucceed`).
fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn alias_strings(paths: &[PathBuf]) -> Vec<String> {
    let mut aliases: Vec<String> =
        paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
    aliases.sort();
    aliases.dedup();
    aliases
}

// =============================================================================
// The watcher (location-watcher.ts:19-101)
// =============================================================================

/// Spawns the location watcher for one location.
pub struct LocationWatcher;

impl LocationWatcher {
    /// Starts the watcher task: resolve target → file-watch it → publish
    /// `filesystem.changed` + touch the location's activity, reconciling
    /// on policy changes. `stop()` (or dropping the handle) tears
    /// everything down.
    #[must_use]
    pub fn spawn(input: LocationWatcherInput) -> LocationWatcherHandle {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let pump_shutdown = shutdown_rx.clone();
        let task = tokio::spawn(async move {
            let mut policy_rx = input.policy.observe();
            let mut shutdown = shutdown_rx;
            let mut active: Option<ActiveWatch> = None;

            reconcile(&input, &mut active, pump_shutdown.clone());
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            break;
                        }
                    }
                    _ = policy_rx.changed() => {
                        reconcile(&input, &mut active, pump_shutdown.clone());
                    }
                }
            }
            if let Some(active) = active.take() {
                active.pump.abort();
            }
        });
        LocationWatcherHandle { task: Some(task), shutdown: shutdown_tx }
    }
}

/// Per-location watcher inputs.
pub struct LocationWatcherInput {
    pub watcher: Arc<FileWatcher>,
    pub bus: Bus,
    pub activity: Arc<LocationActivity>,
    pub location: LocationRef,
    pub vcs: Option<Vcs>,
    pub policy: Arc<LocationWatcherPolicy>,
}

struct ActiveWatch {
    #[allow(dead_code)]
    path: PathBuf,
    pump: tokio::task::JoinHandle<()>,
}

/// The subscription reconciliation (location-watcher.ts:55-79): resolve →
/// check the ignore aliases → swap the active watch if the target moved.
fn reconcile(
    input: &LocationWatcherInput,
    active: &mut Option<ActiveWatch>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let ignore = input.policy.current();
    let next = resolve_target(&input.location.directory, input.vcs.as_ref())
        .filter(|target| !target.aliases.iter().any(|alias| ignore.contains(alias)))
        .map(|target| target.path);
    if active.as_ref().map(|active| &active.path) == next.as_ref() {
        return;
    }
    if let Some(active) = active.take() {
        // Aborting the pump drops its `Subscription`, which tears the
        // native watch down (no leaked watches on retarget).
        active.pump.abort();
    }
    let Some(path) = next else { return };
    let pump = tokio::spawn(pump(
        Arc::clone(&input.watcher),
        path.clone(),
        input.bus.clone(),
        Arc::clone(&input.activity),
        input.location.clone(),
        shutdown,
    ));
    *active = Some(ActiveWatch { path, pump });
}

/// The subscription pump (REF's `Stream.runForEach(updates, publish)`):
/// each update publishes `filesystem.changed` and re-arms the location's
/// activity (only for already-tracked locations — REF touches only when
/// the location is active in the service map).
async fn pump(
    watcher: Arc<FileWatcher>,
    path: PathBuf,
    bus: Bus,
    activity: Arc<LocationActivity>,
    location: LocationRef,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut subscription = watcher.subscribe(&WatchTarget::File { path });
    let schema_ref = schema_location(&location);
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            update = subscription.recv() => {
                let Some(update) = update else { break };
                publish_changed(&bus, Some(&schema_ref), &update).await;
                if activity.last_touch(&location).is_some() {
                    activity.touch(&location);
                }
            }
        }
    }
}

/// `LocationRef` → wire `Location.Ref` (bus publish stamp).
fn schema_location(location: &LocationRef) -> oc_schema::location::Ref {
    oc_schema::location::Ref {
        directory: oc_schema::common::AbsolutePath::parse(
            location.directory.to_string_lossy().into_owned(),
        ),
        workspace_id: location
            .workspace_id
            .as_ref()
            .and_then(|id| oc_schema::id::WorkspaceId::parse(id.clone()).ok()),
    }
}

impl Drop for LocationWatcherHandle {
    fn drop(&mut self) {
        // Signal first (clean unwind through the select), abort as the
        // belt-and-braces backstop — no leaked tasks on engine teardown.
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Owns the watcher task; `stop()` or drop ends it and its subscription.
pub struct LocationWatcherHandle {
    task: Option<tokio::task::JoinHandle<()>>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl LocationWatcherHandle {
    /// Signals the task to stop and awaits its end (idempotent).
    pub async fn stop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    /// Whether the task has ended.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.task.as_ref().is_none_or(tokio::task::JoinHandle::is_finished)
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::assert_is_empty,
        clippy::match_wild_err_arm,
        reason = "the assert!/Err(_) spellings mirror the timeout loops"
    )]
    use super::*;
    use crate::filesystem::watcher::FileWatcher;
    use futures::StreamExt;
    use oc_util::determinism::{Clock, SystemClock};
    use std::time::Duration;

    async fn test_bus() -> (Bus, tempfile::TempDir) {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-locwatch-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir under /tmp/opencode");
        let db = crate::database::Database::open_file(dir.path().join("bus.db"))
            .await
            .expect("open database");
        (Bus::new(db, crate::bus::BusOptions::deterministic()), dir)
    }

    fn git_repo(dir: &Path) -> PathBuf {
        let repo = git2::Repository::init(dir).expect("git init");
        repo.path().to_path_buf()
    }

    #[test]
    fn resolve_target_git_points_at_head() {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-target-git-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir");
        let git_dir = git_repo(dir.path());
        let target = resolve_target(dir.path(), Some(&Vcs::Git { store: git_dir.clone() }));
        let target = target.expect("git target resolves");
        assert!(target.path.ends_with("HEAD"));
        assert!(target
            .path
            .starts_with(dir.path().canonicalize().unwrap_or(dir.path().to_path_buf())));
        assert!(target.aliases.iter().any(|alias| alias == ".git"));
    }

    #[test]
    fn resolve_target_hg_points_at_branch() {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-target-hg-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir");
        let store = dir.path().join(".hg");
        std::fs::create_dir_all(&store).expect("hg store");
        let target = resolve_target(dir.path(), Some(&Vcs::Hg { store: store.clone() }));
        let target = target.expect("hg target resolves");
        assert_eq!(target.path, store.join("branch"));
        assert!(target.aliases.iter().any(|alias| alias == ".hg"));
    }

    #[test]
    fn resolve_target_none_without_vcs() {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-target-none-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir");
        assert!(resolve_target(dir.path(), None).is_none());
    }

    struct Fixture {
        watcher: Arc<FileWatcher>,
        policy: Arc<LocationWatcherPolicy>,
        bus: Bus,
        activity: Arc<LocationActivity>,
        location: LocationRef,
        head: PathBuf,
    }

    async fn fixture(vcs_store: Option<PathBuf>) -> (Fixture, tempfile::TempDir) {
        let (bus, dir) = test_bus().await;
        let git_dir = vcs_store.unwrap_or_else(|| git_repo(dir.path()));
        let head = git_dir.join("HEAD");
        let watcher = FileWatcher::new();
        let policy = LocationWatcherPolicy::new();
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let activity = Arc::new(LocationActivity::new(clock));
        let location = LocationRef::new(dir.path().to_path_buf(), None);
        // REF touches only locations already active in the service map.
        activity.touch_at(&location, 0);
        (Fixture { watcher, policy, bus, activity, location: location.clone(), head }, dir)
    }

    fn spawn(fixture: &Fixture, vcs: Option<Vcs>) -> LocationWatcherHandle {
        LocationWatcher::spawn(LocationWatcherInput {
            watcher: Arc::clone(&fixture.watcher),
            bus: fixture.bus.clone(),
            activity: Arc::clone(&fixture.activity),
            location: fixture.location.clone(),
            vcs,
            policy: Arc::clone(&fixture.policy),
        })
    }

    async fn wait_for_watches(fixture: &Fixture, count: usize) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while fixture.watcher.active_keys().len() != count && std::time::Instant::now() < deadline {
            // The spawn task reconciles asynchronously.
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[tokio::test]
    async fn publishes_filesystem_changed_and_touches_activity() {
        let (fixture, _dir) = fixture(None).await;
        let mut events = fixture.bus.subscribe_type(crate::bus::registry::FILESYSTEM_CHANGED);
        let mut handle =
            spawn(&fixture, Some(Vcs::Git { store: fixture.head.parent().unwrap().to_path_buf() }));
        wait_for_watches(&fixture, 1).await;
        assert_eq!(fixture.watcher.active_keys().len(), 1, "HEAD file watched");

        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let event = loop {
            let attempt = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            std::fs::write(&fixture.head, format!("ref: refs/heads/lane-{attempt}\n"))
                .expect("rewrite HEAD");
            match tokio::time::timeout(Duration::from_millis(400), events.next()).await {
                Ok(Some(event)) => break event,
                Ok(None) => panic!("subscription ended"),
                Err(_) if std::time::Instant::now() > deadline => {
                    panic!("no filesystem.changed within 10s")
                }
                Err(_) => {}
            }
        };
        assert_eq!(event.type_, crate::bus::registry::FILESYSTEM_CHANGED);
        assert_eq!(event.data["file"], fixture.head.to_string_lossy().into_owned());
        assert_eq!(event.data["event"], "change");
        // Location-stamped (REF publishes on the location-scoped bus).
        assert_eq!(
            event.location.as_ref().map(|location| location.directory.0.clone()),
            Some(fixture.location.directory.to_string_lossy().into_owned())
        );
        // Activity re-armed: the seeded 0 timestamp advanced.
        assert!(fixture.activity.last_touch(&fixture.location).unwrap_or(0) > 0);

        handle.stop().await;
        assert!(handle.is_stopped());
        assert!(fixture.watcher.active_keys().is_empty(), "watch torn down on stop");
    }

    #[tokio::test]
    async fn ignore_policy_suppresses_and_reconciles() {
        let (fixture, _dir) = fixture(None).await;
        let store = fixture.head.parent().unwrap().to_path_buf();
        let mut events = fixture.bus.subscribe_type(crate::bus::registry::FILESYSTEM_CHANGED);
        let mut handle = spawn(&fixture, Some(Vcs::Git { store: store.clone() }));
        wait_for_watches(&fixture, 1).await;

        // `watcher.ignore: [".git"]` covers the alias set — the watch
        // must reconcile to nothing (REF: target filtered by aliases).
        fixture.policy.add(&[".git".to_string()]);
        wait_for_watches(&fixture, 0).await;
        assert!(fixture.watcher.active_keys().is_empty(), "ignored target unwatches");

        // And a synthetic event under .git stays unpublished (belt and
        // braces — the watch is already gone).
        std::fs::write(&fixture.head, "ref: refs/heads/x\n").expect("rewrite HEAD");
        assert!(tokio::time::timeout(Duration::from_millis(300), events.next()).await.is_err());
        handle.stop().await;
    }

    #[tokio::test]
    async fn no_vcs_means_no_watch() {
        let (fixture, _dir) = fixture(None).await;
        let mut handle = spawn(&fixture, None);
        assert!(fixture.watcher.active_keys().is_empty());
        handle.stop().await;
        assert!(handle.is_stopped());
    }
}
