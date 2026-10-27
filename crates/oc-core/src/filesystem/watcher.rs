//! The general file-watcher service — port of `filesystem/watcher.ts`
//! (v2.0.24) on top of the notify backend the port already uses for config
//! hot-reload (`config/watch.rs`).
//!
//! REF shape (`Watcher.Service`): refcounted subscriptions keyed by target
//! (`file` / `entries` / `directory` + ignore list) over a native backend
//! (@parcel/watcher on REF; `notify` here). Equivalent targets share one
//! native watch; the last dropped subscription tears it down. Event
//! reduction matches REF's `Update { type: create|update|delete, path }`,
//! and directory watches apply REF's ignore rules (the same
//! `node_modules`/`.git` exclusions the config watcher uses).
//!
//! The bus side (`FileSystem.Event.Changed`) lives in
//! [`publish_changed`] / [`spawn_publisher`]: REF's `location-watcher.ts`
//! maps watcher updates onto `filesystem.changed` `{file, event}` — the
//! mapping is shared here so any directory watch can publish the same
//! event.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::bus::registry;
use crate::config::watch::{
    FsChangeKind, FsUpdate, WatchBackend, WatchGuard, WatchTarget, DIRECTORY_IGNORE,
};
use oc_schema::filesystem::{Change, Changed};

/// The default directory-ignore list (REF watch inputs carry
/// `node_modules`/`.git` exclusions; `config/watch.rs` pins the same list).
#[must_use]
pub fn default_ignore() -> Vec<String> {
    DIRECTORY_IGNORE.iter().map(|entry| (*entry).to_string()).collect()
}

/// The subscriber fan-out for one shared watch.
type Senders = Arc<Mutex<Vec<(u64, tokio::sync::mpsc::UnboundedSender<FsUpdate>)>>>;

/// One live shared watch: the native guard plus the subscriber senders.
struct SharedTarget {
    _guard: Box<dyn WatchGuard>,
    senders: Senders,
}

/// `Watcher.Service` — refcounted watch subscriptions over a backend.
pub struct FileWatcher {
    backend: Arc<dyn WatchBackend>,
    targets: Mutex<HashMap<String, SharedTarget>>,
    next_id: AtomicU64,
    enabled: bool,
}

impl FileWatcher {
    /// Watcher over a native (notify) backend.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::with_backend(crate::config::watch::NotifyBackend::new(), true))
    }

    /// Watcher that supports no backend (`Watcher.layer({ enabled: false })`
    /// — subscribe returns an immediately-ended stream).
    #[must_use]
    pub fn disabled() -> Arc<Self> {
        Arc::new(Self::with_backend(Arc::new(DisabledBackend), false))
    }

    fn with_backend(backend: Arc<dyn WatchBackend>, enabled: bool) -> Self {
        Self { backend, targets: Mutex::new(HashMap::new()), next_id: AtomicU64::new(1), enabled }
    }

    /// Whether the watcher serves subscriptions.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Live watch keys (diagnostics/tests).
    #[must_use]
    pub fn active_keys(&self) -> Vec<String> {
        self.targets.lock().unwrap().keys().cloned().collect()
    }

    /// Subscribes to a target; equivalent targets share one native watch
    /// (`RcMap` with structural keys in REF). Dropping the returned
    /// subscription detaches; the last drop tears the native watch down.
    pub fn subscribe(self: &Arc<Self>, target: WatchTarget) -> Subscription {
        let key = target.key();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut targets = self.targets.lock().unwrap();
        // Unsupported backend (disabled watcher): REF ends the subscriber
        // stream instead of hanging it — drop the sender so `recv`
        // returns `None` and register nothing.
        if let Some(existing) = targets.get_mut(&key) {
            existing.senders.lock().unwrap().push((id, tx));
            return Subscription { watcher: Arc::clone(self), key, id, rx };
        }
        let senders: Senders = Arc::new(Mutex::new(Vec::new()));
        let forward = Arc::clone(&senders);
        let Some(guard) = self.backend.subscribe(
            &target,
            Box::new(move |update: FsUpdate| {
                let senders = forward.lock().unwrap();
                for (_, sender) in senders.iter() {
                    let _ = sender.send(update.clone());
                }
            }),
        ) else {
            drop(tx);
            return Subscription { watcher: Arc::clone(self), key, id, rx };
        };
        senders.lock().unwrap().push((id, tx));
        targets.insert(key.clone(), SharedTarget { _guard: guard, senders });
        Subscription { watcher: Arc::clone(self), key, id, rx }
    }
}

impl Default for FileWatcher {
    fn default() -> Self {
        Self::with_backend(crate::config::watch::NotifyBackend::new(), true)
    }
}

/// A watch subscription (`Watcher` stream consumer). Dropping detaches;
/// the receiver ends (`None`) once the watch tears down.
pub struct Subscription {
    watcher: Arc<FileWatcher>,
    key: String,
    id: u64,
    rx: tokio::sync::mpsc::UnboundedReceiver<FsUpdate>,
}

impl Subscription {
    /// Next update, if the watch is still live.
    pub async fn recv(&mut self) -> Option<FsUpdate> {
        self.rx.recv().await
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut targets = self.watcher.targets.lock().unwrap();
        let Some(entry) = targets.get_mut(&self.key) else { return };
        entry.senders.lock().unwrap().retain(|(id, _)| *id != self.id);
        if entry.senders.lock().unwrap().is_empty() {
            targets.remove(&self.key);
        }
    }
}

/// Watches a directory tree and publishes `filesystem.changed` for every
/// update (REF's publish mapping from `location-watcher.ts`, applied to a
/// general directory watch). The returned task ends when `shutdown` fires.
pub fn spawn_publisher(
    watcher: &Arc<FileWatcher>,
    directory: PathBuf,
    ignore: Vec<String>,
    bus: crate::bus::Bus,
    location: Option<oc_schema::location::Ref>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let watcher = Arc::clone(watcher);
    tokio::spawn(async move {
        let mut subscription =
            watcher.subscribe(WatchTarget::Directory { path: directory, ignore });
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                update = subscription.recv() => {
                    let Some(update) = update else { break };
                    publish_changed(&bus, location.as_ref(), &update).await;
                }
            }
        }
    })
}

/// Maps one watcher update onto `FileSystem.Event.Changed`
/// (`filesystem.changed` `{file, event}` — location-watcher.ts:28-32:
/// create → `add`, update → `change`, delete → `unlink`) and publishes it.
pub async fn publish_changed(
    bus: &crate::bus::Bus,
    location: Option<&oc_schema::location::Ref>,
    update: &FsUpdate,
) {
    let Some(definition) = registry::by_type(registry::FILESYSTEM_CHANGED) else { return };
    let data = Changed {
        file: update.path.to_string_lossy().into_owned(),
        event: match update.kind {
            FsChangeKind::Create => Change::Add,
            FsChangeKind::Update => Change::Change,
            FsChangeKind::Delete => Change::Unlink,
        },
    };
    let options = crate::bus::PublishOptions { location: location.cloned(), ..Default::default() };
    let _ = bus
        .publish(
            definition,
            serde_json::to_value(&data).unwrap_or(serde_json::Value::Null),
            options,
        )
        .await;
}

/// Backend that supports no watches (`enabled: false`).
struct DisabledBackend;

impl WatchBackend for DisabledBackend {
    fn subscribe(
        &self,
        _target: &WatchTarget,
        _on_event: Box<dyn Fn(FsUpdate) + Send + Sync>,
    ) -> Option<Box<dyn WatchGuard>> {
        None
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
    use crate::config::watch::FakeBackend;
    use futures::StreamExt;
    use std::time::Duration;

    fn fake_watcher() -> (Arc<FileWatcher>, Arc<FakeBackend>) {
        let backend = FakeBackend::new();
        let watcher = Arc::new(FileWatcher::with_backend(
            Arc::clone(&backend) as Arc<dyn WatchBackend>,
            true,
        ));
        (watcher, backend)
    }

    fn dir_target(path: PathBuf) -> WatchTarget {
        WatchTarget::Directory { path, ignore: default_ignore() }
    }

    #[tokio::test]
    async fn subscriptions_share_one_native_watch_and_teardown_on_last_drop() {
        let (watcher, backend) = fake_watcher();
        let target = dir_target(PathBuf::from("/repo"));
        let first = watcher.subscribe(target.clone());
        let second = watcher.subscribe(target);
        assert_eq!(backend.subscriptions().len(), 1, "equivalent targets share one watch");
        drop(second);
        assert_eq!(watcher.active_keys().len(), 1, "last-drop teardown only");
        drop(first);
        assert!(watcher.active_keys().is_empty(), "native watch torn down");
    }

    #[tokio::test]
    async fn disabled_watcher_ends_immediately() {
        let watcher = FileWatcher::disabled();
        assert!(!watcher.enabled());
        let mut subscription = watcher.subscribe(dir_target(PathBuf::from("/repo")));
        assert!(subscription.recv().await.is_none());
        assert!(watcher.active_keys().is_empty());
    }

    #[tokio::test]
    async fn ignore_rules_exclude_node_modules_and_git() {
        let (watcher, backend) = fake_watcher();
        let target = dir_target(PathBuf::from("/repo"));
        let mut subscription = watcher.subscribe(target);
        backend.emit(&FsUpdate {
            kind: FsChangeKind::Create,
            path: PathBuf::from("/repo/node_modules/pkg/index.js"),
        });
        backend
            .emit(&FsUpdate { kind: FsChangeKind::Create, path: PathBuf::from("/repo/.git/HEAD") });
        // Ignored trees never reach the subscriber (REF ignore rules).
        assert!(tokio::time::timeout(Duration::from_millis(100), subscription.recv())
            .await
            .is_err());
        backend.emit(&FsUpdate {
            kind: FsChangeKind::Create,
            path: PathBuf::from("/repo/src/main.rs"),
        });
        let update = tokio::time::timeout(Duration::from_millis(100), subscription.recv())
            .await
            .expect("tracked update delivered")
            .expect("watch still live");
        assert_eq!(update.path, PathBuf::from("/repo/src/main.rs"));
    }

    #[tokio::test]
    async fn publisher_publishes_filesystem_changed_on_change() {
        let dir = tempfile::Builder::new()
            .prefix("oc-core-fswatch-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir under /tmp/opencode");
        let db = crate::database::Database::open_file(dir.path().join("bus.db"))
            .await
            .expect("open database");
        let bus = crate::bus::Bus::new(db, crate::bus::BusOptions::deterministic());
        let mut events = bus.subscribe_type(registry::FILESYSTEM_CHANGED);

        let watcher = FileWatcher::new();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let publisher = spawn_publisher(
            &watcher,
            dir.path().to_path_buf(),
            default_ignore(),
            bus.clone(),
            None,
            shutdown_rx,
        );
        let file = dir.path().join("notes.txt");
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let event = loop {
            std::fs::write(&file, format!("content {}", std::process::id())).expect("write file");
            match tokio::time::timeout(Duration::from_millis(400), events.next()).await {
                Ok(Some(event)) => break event,
                Ok(None) => panic!("subscription ended"),
                Err(_) if std::time::Instant::now() > deadline => {
                    panic!("no filesystem.changed event within 10s")
                }
                Err(_) => {}
            }
        };
        assert_eq!(event.type_, registry::FILESYSTEM_CHANGED);
        assert_eq!(event.data["file"], file.to_string_lossy().into_owned());
        assert!(matches!(event.data["event"].as_str(), Some("add" | "change")));
        // Clean shutdown: the pump ends and the native watch is gone.
        drop(shutdown_tx);
        tokio::time::timeout(Duration::from_secs(5), publisher)
            .await
            .expect("publisher stops")
            .unwrap();
        assert!(watcher.active_keys().is_empty());
    }
}
