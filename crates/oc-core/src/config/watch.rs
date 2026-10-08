//! Watch planning + hot reload — port of `config/watch.ts` and the reload
//! loop in `config.ts`.
//!
//! [`plan`] turns discovered sources into watch targets: recursive directory
//! watches for the global dir and present `.opencode` roots (ignoring
//! `node_modules`/`.git`), and parent "entries" watches for loose files so
//! deletion/recreation stays observable. [`WatchHub`] drives a debounced
//! (100ms) serialized reload: re-discover → deep-equal check →
//! [`ConfigUpdated`] only on change.

use crate::config::unpoison;
use notify::Watcher as _;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::discovery::Sources;

/// Debounce window for config reloads.
pub const DEBOUNCE: Duration = Duration::from_millis(100);

/// Watch inputs that must not be traversed.
pub const DIRECTORY_IGNORE: [&str; 3] = ["node_modules", ".git", "**/{node_modules,.git}/**"];

/// Filesystem update event (`Watcher.Update`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsUpdate {
    pub kind: FsChangeKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsChangeKind {
    Create,
    Update,
    Delete,
}

/// `config.updated` receiver; the bus lands in a later lane.
pub trait ConfigUpdated: Send + Sync {
    fn on_config_updated(&self);
}

/// Raw change feed subscriber (`Config.changes()`).
pub trait ChangePublisher: Send + Sync {
    fn publish(&self, update: &FsUpdate);
}

/// A watch target (`Watcher.WatchInput`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchTarget {
    /// Exact file (instructions).
    File { path: PathBuf },
    /// Watch the parent for these basenames.
    Entries { path: PathBuf, names: Vec<String> },
    /// Recursive directory watch with ignore list.
    Directory { path: PathBuf, ignore: Vec<String> },
}

impl WatchTarget {
    fn key(&self) -> String {
        match self {
            WatchTarget::File { path } => format!("file:{}", path.display()),
            WatchTarget::Entries { path, names } => {
                format!("entries:{}:{}", path.display(), names.join(","))
            }
            WatchTarget::Directory { path, ignore } => {
                format!("directory:{}:{}", path.display(), ignore.join(","))
            }
        }
    }
}

/// `ConfigWatch.plan`: directory watches for global + present project roots;
/// parent entries watches for loose files not covered by a directory watch.
#[must_use]
pub fn plan(sources: &Sources) -> Vec<WatchTarget> {
    let directories: Vec<PathBuf> = sources
        .global
        .iter()
        .cloned()
        .chain(sources.project.iter().filter(|root| root.present).map(|root| root.path.clone()))
        .collect();
    let mut files: Vec<PathBuf> = sources.direct.clone();
    files.extend(sources.project.iter().map(|root| root.path.clone()));
    files.extend(sources.claude.iter().cloned());
    files.extend(sources.agents.iter().cloned());
    if let Some(explicit) = &sources.explicit {
        files.push(explicit.clone());
    }

    let mut targets: Vec<WatchTarget> = directories
        .iter()
        .map(|path| WatchTarget::Directory {
            path: path.clone(),
            ignore: DIRECTORY_IGNORE.iter().map(ToString::to_string).collect(),
        })
        .collect();

    // Group loose files by parent so deletion/recreation is observable.
    let mut parents: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for file in files {
        let covered =
            directories.iter().any(|directory| file != *directory && file.starts_with(directory));
        if covered {
            continue;
        }
        let parent = file.parent().map_or_else(|| PathBuf::from("/"), Path::to_path_buf);
        let name =
            file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        match parents.iter_mut().find(|(path, _)| *path == parent) {
            Some((_, names)) => {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            None => parents.push((parent, vec![name])),
        }
    }
    for (path, mut names) in parents {
        names.sort();
        targets.push(WatchTarget::Entries { path, names });
    }

    // Dedupe identical targets (plan keys).
    let mut unique: Vec<WatchTarget> = Vec::new();
    for target in targets {
        if !unique.iter().any(|existing| existing.key() == target.key()) {
            unique.push(target);
        }
    }
    unique
}

/// A live watch; dropping the guard unsubscribes.
pub trait WatchGuard: Send {}

/// Watch backend abstraction (notify-based in production, fake in tests).
pub trait WatchBackend: Send + Sync {
    fn subscribe(
        &self,
        target: &WatchTarget,
        on_event: Box<dyn Fn(FsUpdate) + Send + Sync>,
    ) -> Option<Box<dyn WatchGuard>>;
}

enum HubEvent {
    Update(FsUpdate),
    Reload,
}

/// Owns subscriptions and the debounced reload loop.
pub struct WatchHub {
    backend: Arc<dyn WatchBackend>,
    subscriptions: Mutex<HashMap<String, Box<dyn WatchGuard>>>,
    tx: std::sync::mpsc::SyncSender<HubEvent>,
    rx: Mutex<Option<std::sync::mpsc::Receiver<HubEvent>>>,
    publishers: Mutex<Vec<Arc<dyn ChangePublisher>>>,
}

impl WatchHub {
    #[must_use]
    pub fn new(backend: Arc<dyn WatchBackend>) -> Arc<Self> {
        let (tx, rx) = std::sync::mpsc::sync_channel(1024);
        Arc::new(Self {
            backend,
            subscriptions: Mutex::new(HashMap::new()),
            tx,
            rx: Mutex::new(Some(rx)),
            publishers: Mutex::new(Vec::new()),
        })
    }

    pub fn add_publisher(&self, publisher: Arc<dyn ChangePublisher>) {
        self.publishers.lock().unwrap_or_else(unpoison).push(publisher);
    }

    /// Adds missing watches and tears down stale ones (plan reconciliation).
    pub fn reconcile(&self, targets: &[WatchTarget]) {
        let mut subscriptions = self.subscriptions.lock().unwrap_or_else(unpoison);
        let plan_keys: Vec<String> = targets.iter().map(WatchTarget::key).collect();
        subscriptions.retain(|key, _| plan_keys.contains(key));
        for target in targets {
            let key = target.key();
            if subscriptions.contains_key(&key) {
                continue;
            }
            let tx = self.tx.clone();
            let guard = self.backend.subscribe(
                target,
                Box::new(move |update| {
                    let _ = tx.try_send(HubEvent::Update(update));
                }),
            );
            if let Some(guard) = guard {
                subscriptions.insert(key, guard);
            }
        }
    }

    /// Requests a debounced reload (watch events, credential switches,
    /// wellknown refreshes, readiness rescans).
    pub fn request_reload(&self) {
        let _ = self.tx.try_send(HubEvent::Reload);
    }

    /// Runs the debounce loop on a background thread. `reload` returns
    /// whether the observed state changed; changes fire `updated`.
    pub fn run(
        self: &Arc<Self>,
        debounce: Duration,
        reload: Arc<dyn Fn() -> bool + Send + Sync>,
        updated: Arc<dyn ConfigUpdated>,
    ) -> std::thread::JoinHandle<()> {
        let hub = Arc::clone(self);
        let rx = hub.rx.lock().unwrap_or_else(unpoison).take().expect("watch hub already running");
        std::thread::Builder::new()
            .name("oc-config-watch".to_string())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    if let HubEvent::Update(update) = event {
                        let publishers = hub.publishers.lock().unwrap_or_else(unpoison).clone();
                        for publisher in publishers {
                            publisher.publish(&update);
                        }
                    }
                    let deadline = std::time::Instant::now() + debounce;
                    // Extend the window while events keep arriving.
                    loop {
                        let now = std::time::Instant::now();
                        if now >= deadline {
                            break;
                        }
                        match rx.recv_timeout(deadline - now) {
                            Ok(HubEvent::Update(update)) => {
                                let publishers =
                                    hub.publishers.lock().unwrap_or_else(unpoison).clone();
                                for publisher in publishers {
                                    publisher.publish(&update);
                                }
                            }
                            Ok(HubEvent::Reload) => {}
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    if reload() {
                        updated.on_config_updated();
                    }
                }
            })
            .expect("spawn config watch thread")
    }
}

/// notify-based backend.
pub struct NotifyBackend {
    state: Arc<NotifyState>,
}

struct NotifyState {
    watcher: Mutex<notify::RecommendedWatcher>,
    routes: Mutex<Vec<Route>>,
}

#[derive(Clone)]
struct Route {
    key: String,
    target: WatchTarget,
    callback: Arc<dyn Fn(FsUpdate) + Send + Sync>,
}

impl NotifyBackend {
    fn spawn() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            if let Ok(event) = result {
                let _ = tx.send(event);
            }
        })
        .expect("notify watcher");
        let state =
            Arc::new(NotifyState { watcher: Mutex::new(watcher), routes: Mutex::new(Vec::new()) });
        let dispatcher = Arc::clone(&state);
        std::thread::Builder::new()
            .name("oc-config-notify".to_string())
            .spawn(move || {
                for event in rx {
                    let kind = match event.kind {
                        notify::EventKind::Create(_) => FsChangeKind::Create,
                        notify::EventKind::Remove(_) => FsChangeKind::Delete,
                        _ => FsChangeKind::Update,
                    };
                    for path in event.paths {
                        let update = FsUpdate { kind, path };
                        let routes = dispatcher.routes.lock().unwrap_or_else(unpoison).clone();
                        for route in routes {
                            if route_matches(&route.target, &update.path) {
                                (route.callback)(update.clone());
                            }
                        }
                    }
                }
            })
            .expect("spawn notify dispatcher");
        Self { state }
    }

    #[must_use]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::spawn())
    }
}

impl Default for NotifyBackend {
    fn default() -> Self {
        Self::spawn()
    }
}

struct NotifyGuard {
    state: Arc<NotifyState>,
    key: String,
    path: PathBuf,
}

impl Drop for NotifyGuard {
    fn drop(&mut self) {
        self.state.routes.lock().unwrap_or_else(unpoison).retain(|route| route.key != self.key);
        if let Ok(mut watcher) = self.state.watcher.lock() {
            let _ = watcher.unwatch(&self.path);
        }
    }
}

impl WatchGuard for NotifyGuard {}

impl WatchBackend for NotifyBackend {
    fn subscribe(
        &self,
        target: &WatchTarget,
        on_event: Box<dyn Fn(FsUpdate) + Send + Sync>,
    ) -> Option<Box<dyn WatchGuard>> {
        let (path, recursive) = match target {
            WatchTarget::File { path } => {
                // Watch the parent so missing files stay observable.
                (path.parent().map(Path::to_path_buf)?, notify::RecursiveMode::NonRecursive)
            }
            WatchTarget::Entries { path, .. } => {
                (path.clone(), notify::RecursiveMode::NonRecursive)
            }
            WatchTarget::Directory { path, .. } => (path.clone(), notify::RecursiveMode::Recursive),
        };
        self.state.watcher.lock().ok()?.watch(&path, recursive).ok()?;
        let key = format!("{}#{}", target.key(), self.state.routes.lock().ok()?.len());
        let route =
            Route { key: key.clone(), target: target.clone(), callback: Arc::from(on_event) };
        self.state.routes.lock().ok()?.push(route);
        Some(Box::new(NotifyGuard { state: self.state.clone(), key, path }))
    }
}

fn route_matches(target: &WatchTarget, path: &Path) -> bool {
    match target {
        WatchTarget::File { path: file } => path == file,
        WatchTarget::Entries { path: dir, names } => {
            path.parent() == Some(dir.as_path())
                && path
                    .file_name()
                    .is_some_and(|name| names.contains(&name.to_string_lossy().into_owned()))
        }
        WatchTarget::Directory { path: dir, .. } => {
            path != dir
                && path.starts_with(dir)
                && !path.components().any(|component| {
                    matches!(component.as_os_str().to_str(), Some("node_modules" | ".git"))
                })
        }
    }
}

/// Test backend recording subscriptions and emitting synthetic updates.
pub struct FakeBackend {
    state: Arc<FakeState>,
}

type FakeRoutes = Vec<(String, WatchTarget, Arc<dyn Fn(FsUpdate) + Send + Sync>)>;

#[derive(Default)]
struct FakeState {
    routes: Mutex<FakeRoutes>,
}

impl FakeBackend {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self { state: Arc::new(FakeState::default()) })
    }

    /// Active watch targets in subscription order.
    #[must_use]
    pub fn subscriptions(&self) -> Vec<WatchTarget> {
        self.state
            .routes
            .lock()
            .unwrap_or_else(unpoison)
            .iter()
            .map(|(_, target, _)| target.clone())
            .collect()
    }

    /// Emits an update to every matching subscription.
    pub fn emit(&self, update: &FsUpdate) {
        let routes = self.state.routes.lock().unwrap_or_else(unpoison).clone();
        for (_, target, callback) in routes {
            if route_matches(&target, &update.path) {
                callback(update.clone());
            }
        }
    }
}

struct FakeGuard {
    state: Arc<FakeState>,
    key: String,
}

impl Drop for FakeGuard {
    fn drop(&mut self) {
        self.state.routes.lock().unwrap_or_else(unpoison).retain(|(key, _, _)| *key != self.key);
    }
}

impl WatchGuard for FakeGuard {}

impl WatchBackend for FakeBackend {
    fn subscribe(
        &self,
        target: &WatchTarget,
        on_event: Box<dyn Fn(FsUpdate) + Send + Sync>,
    ) -> Option<Box<dyn WatchGuard>> {
        let key = format!("{}#{}", target.key(), self.state.routes.lock().ok()?.len());
        self.state.routes.lock().ok()?.push((key.clone(), target.clone(), Arc::from(on_event)));
        Some(Box::new(FakeGuard { state: self.state.clone(), key }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::discovery::ProjectRoot;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[allow(clippy::needless_pass_by_value)]
    fn sources_fixture(project: PathBuf, present: bool) -> Sources {
        Sources {
            global: None,
            explicit: None,
            direct: vec![project.join("opencode.json"), project.join("opencode.jsonc")],
            project: vec![ProjectRoot { path: project.join(".opencode"), present }],
            claude: vec![project.join(".claude")],
            agents: vec![project.join(".agents")],
        }
    }

    #[test]
    fn config_watch_plan_groups_missing_candidates_under_parent_entries() {
        let project = PathBuf::from("/watch-plan-project");
        let sources = sources_fixture(project.clone(), false);
        let missing = plan(&sources);
        assert_eq!(
            missing,
            vec![WatchTarget::Entries {
                path: project.clone(),
                names: vec![
                    ".agents".to_string(),
                    ".claude".to_string(),
                    ".opencode".to_string(),
                    "opencode.json".to_string(),
                    "opencode.jsonc".to_string()
                ]
            }]
        );
        let present = plan(&sources_fixture(project.clone(), true));
        assert_eq!(
            present,
            vec![
                WatchTarget::Directory {
                    path: project.join(".opencode"),
                    ignore: DIRECTORY_IGNORE.iter().map(ToString::to_string).collect()
                },
                missing[0].clone(),
            ]
        );
    }

    #[test]
    fn config_watch_plan_covers_explicit_inside_watched_dir() {
        let project = PathBuf::from("/watch-plan-project");
        let mut sources = sources_fixture(project.clone(), true);
        // Explicit file inside the watched .opencode root: no extra watch.
        sources.explicit = Some(project.join(".opencode").join("custom.json"));
        let covered = plan(&sources);
        assert_eq!(covered, plan(&sources_fixture(project.clone(), true)));
        // Explicit outside: parent entries watch with its basename.
        sources.explicit = Some(PathBuf::from("/watch-plan-external/custom.json"));
        let external = plan(&sources);
        assert!(external.contains(&WatchTarget::Entries {
            path: PathBuf::from("/watch-plan-external"),
            names: vec!["custom.json".to_string()]
        }));
    }

    struct CountingHandler {
        updates: AtomicUsize,
    }

    impl ConfigUpdated for CountingHandler {
        fn on_config_updated(&self) {
            self.updates.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct RecordingPublisher {
        changes: Mutex<Vec<FsUpdate>>,
    }

    impl ChangePublisher for RecordingPublisher {
        fn publish(&self, update: &FsUpdate) {
            self.changes.lock().unwrap_or_else(unpoison).push(update.clone());
        }
    }

    #[test]
    fn config_watch_hub_debounces_and_coalesces_events() {
        let backend = FakeBackend::new();
        let hub = WatchHub::new(backend.clone());
        let publisher = Arc::new(RecordingPublisher { changes: Mutex::new(Vec::new()) });
        hub.add_publisher(publisher.clone());
        hub.reconcile(&[WatchTarget::Entries {
            path: PathBuf::from("/cfg"),
            names: vec!["opencode.json".to_string()],
        }]);
        assert_eq!(backend.subscriptions().len(), 1);

        let handler = Arc::new(CountingHandler { updates: AtomicUsize::new(0) });
        let changed = Arc::new(AtomicUsize::new(0));
        let changed_for_reload = Arc::clone(&changed);
        let reload = Arc::new(move || {
            let count = changed_for_reload.fetch_add(1, Ordering::SeqCst) + 1;
            count % 2 == 1 // first reload "changes", second doesn't
        });
        hub.run(Duration::from_millis(20), reload, handler.clone());

        backend.emit(&FsUpdate {
            kind: FsChangeKind::Create,
            path: PathBuf::from("/cfg/opencode.json"),
        });
        backend.emit(&FsUpdate {
            kind: FsChangeKind::Update,
            path: PathBuf::from("/cfg/opencode.json"),
        });
        backend.emit(&FsUpdate {
            kind: FsChangeKind::Update,
            path: PathBuf::from("/cfg/opencode.json"),
        });
        std::thread::sleep(Duration::from_millis(120));

        assert_eq!(handler.updates.load(Ordering::SeqCst), 1, "one coalesced event");
        assert_eq!(changed.load(Ordering::SeqCst), 1, "one debounced reload");
        assert_eq!(publisher.changes.lock().unwrap().len(), 3, "raw updates stream un-debounced");

        // A no-change reload emits nothing.
        backend.emit(&FsUpdate {
            kind: FsChangeKind::Update,
            path: PathBuf::from("/cfg/opencode.json"),
        });
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(handler.updates.load(Ordering::SeqCst), 1);
        assert_eq!(changed.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn config_watch_hub_reconcile_tears_down_stale_watches() {
        let backend = FakeBackend::new();
        let hub = WatchHub::new(backend.clone());
        let first = vec![WatchTarget::Directory { path: PathBuf::from("/a"), ignore: Vec::new() }];
        hub.reconcile(&first);
        assert_eq!(backend.subscriptions().len(), 1);
        hub.reconcile(&first);
        assert_eq!(backend.subscriptions().len(), 1, "reconcile is idempotent");
        hub.reconcile(&[WatchTarget::Entries {
            path: PathBuf::from("/b"),
            names: vec!["opencode.json".to_string()],
        }]);
        assert_eq!(backend.subscriptions().len(), 1, "stale watch removed");
        assert!(matches!(backend.subscriptions()[0], WatchTarget::Entries { .. }));
    }

    #[test]
    fn config_watch_route_matching() {
        let dir = WatchTarget::Directory { path: PathBuf::from("/repo"), ignore: Vec::new() };
        assert!(route_matches(&dir, Path::new("/repo/.opencode/opencode.json")));
        assert!(!route_matches(&dir, Path::new("/repo/.git/config")));
        assert!(!route_matches(&dir, Path::new("/repo")));
        assert!(!route_matches(&dir, Path::new("/outside/file")));
        let entries = WatchTarget::Entries {
            path: PathBuf::from("/repo"),
            names: vec!["opencode.json".to_string()],
        };
        assert!(route_matches(&entries, Path::new("/repo/opencode.json")));
        assert!(!route_matches(&entries, Path::new("/repo/opencode.jsonc")));
        assert!(!route_matches(&entries, Path::new("/repo/sub/opencode.json")));
    }
}
