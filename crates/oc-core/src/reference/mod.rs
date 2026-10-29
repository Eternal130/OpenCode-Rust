//! Port of `packages/core/src/reference.ts` (v2.0.24): the project
//! reference registry — named local/git sources, git checkout derivation,
//! daily refresh on mutation, and an hourly sweep.
//!
//! Mutations publish the `reference.updated` ephemeral bus event on every
//! mutation (reference.ts:104-108: refresh forked, then the notify
//! publish) once a bus is attached via [`ReferenceService::set_bus`].

pub mod instructions;
pub mod repository;
pub mod repository_cache;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::bus::{registry, PublishOptions};

pub use repository::{
    cache_path, parse, parse_remote, same, validate_branch, FileReference, Reference,
    RemoteReference,
};
use repository_cache::{EnsureInput, Refresh, RepositoryCache};

/// `Reference.LocalSource`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSource {
    pub path: PathBuf,
    pub description: Option<String>,
    pub hidden: Option<bool>,
}

/// `Reference.GitSource`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSource {
    pub repository: String,
    pub branch: Option<String>,
    pub description: Option<String>,
    pub hidden: Option<bool>,
}

/// `Reference.Source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Local(LocalSource),
    Git(GitSource),
}

impl Source {
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        match self {
            Source::Local(source) => source.description.as_deref(),
            Source::Git(source) => source.description.as_deref(),
        }
    }

    #[must_use]
    pub fn hidden(&self) -> Option<bool> {
        match self {
            Source::Local(source) => source.hidden,
            Source::Git(source) => source.hidden,
        }
    }
}

/// `Reference.Info` — a source resolved to its materialized path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub name: String,
    pub path: PathBuf,
    pub description: Option<String>,
    pub hidden: Option<bool>,
    pub source: Source,
}

/// The injectable materialization seam (the ref reference.test.ts mocks the
/// whole `RepositoryCache` layer).
pub type EnsureFn = Arc<
    dyn Fn(
            &repository_cache::EnsureInput,
        ) -> Result<repository_cache::CacheResult, repository_cache::CacheError>
        + Send
        + Sync,
>;

/// The reference service (`Reference.Service`): the editor state plus the
/// git materialization loop.
pub struct ReferenceService {
    /// Insertion-ordered `(name, source)` pairs (v2 `Map` order).
    sources: Mutex<Vec<(String, Source)>>,
    repos_root: PathBuf,
    ensure: EnsureFn,
    refresh_on_mutation: bool,
    /// The notify-leg bus (`reference.updated`, reference.ts:107) — `None`
    /// until the engine attaches its location bus.
    bus: Mutex<Option<Arc<crate::bus::Bus>>>,
}

impl ReferenceService {
    /// The production shape over the global repos root and a file-backed
    /// refresh store.
    #[must_use]
    pub fn new(repos_root: PathBuf, clock: Arc<dyn oc_util::determinism::Clock>) -> Self {
        Self::with_cache(
            repos_root.clone(),
            RepositoryCache::new(
                repos_root.clone(),
                clock,
                Arc::new(repository_cache::FileRefreshStore::new(&repos_root)),
            ),
        )
    }

    /// Test shape: explicit cache (fixture roots, fake clock, injected
    /// refresh store) and mutation-time refresh control.
    #[must_use]
    pub fn with_cache(repos_root: PathBuf, cache: RepositoryCache) -> Self {
        let ensure: EnsureFn = Arc::new(move |input| cache.ensure(input));
        Self {
            sources: Mutex::new(Vec::new()),
            repos_root,
            ensure,
            refresh_on_mutation: true,
            bus: Mutex::new(None),
        }
    }

    /// Test shape with a fully mocked cache (reference.test.ts's
    /// `Layer.mock(RepositoryCache.Service, …)`).
    #[must_use]
    pub fn with_ensure(repos_root: PathBuf, ensure: EnsureFn) -> Self {
        Self {
            sources: Mutex::new(Vec::new()),
            repos_root,
            ensure,
            refresh_on_mutation: true,
            bus: Mutex::new(None),
        }
    }

    /// Disables the spawned refresh on mutation (hermetic tests drive
    /// [`Self::refresh_all`] directly).
    pub fn set_refresh_on_mutation(&mut self, enabled: bool) {
        self.refresh_on_mutation = enabled;
    }

    /// Attaches the bus for the notify leg (`reference.updated` on every
    /// mutation, reference.ts:104-108).
    pub fn set_bus(&self, bus: Arc<crate::bus::Bus>) {
        *self.bus.lock() = Some(bus);
    }

    /// `editor.add(name, source)` + the notify leg (reference.ts:95-109):
    /// refresh is forked, never blocking the mutation.
    pub fn add(self: &Arc<Self>, name: impl Into<String>, source: Source) {
        let name = name.into();
        let mut sources = self.sources.lock();
        match sources.iter_mut().find(|(existing, _)| *existing == name) {
            Some(slot) => slot.1 = source,
            None => sources.push((name, source)),
        }
        drop(sources);
        self.notify();
    }

    /// `editor.remove(name)` + notify.
    pub fn remove(self: &Arc<Self>, name: &str) {
        self.sources.lock().retain(|(existing, _)| existing != name);
        self.notify();
    }

    /// `editor.get(name)`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Source> {
        self.sources
            .lock()
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, source)| source.clone())
    }

    /// `editor.list()`.
    #[must_use]
    pub fn sources(&self) -> Vec<(String, Source)> {
        self.sources.lock().clone()
    }

    /// `list()` (reference.ts:51-75): every source resolved to an [`Info`];
    /// unparsable/non-remote git sources and invalid branches are dropped.
    #[must_use]
    pub fn list(&self) -> Vec<Info> {
        self.sources
            .lock()
            .iter()
            .filter_map(|(name, source)| {
                let info = |path: PathBuf| {
                    Some(Info {
                        name: name.clone(),
                        path,
                        description: source.description().map(str::to_owned),
                        hidden: source.hidden(),
                        source: source.clone(),
                    })
                };
                match source {
                    Source::Local(local) => info(local.path.clone()),
                    Source::Git(git) => {
                        let reference = parse(&git.repository)?;
                        if !repository::is_remote(&reference) {
                            return None;
                        }
                        if let Some(branch) = &git.branch {
                            validate_branch(branch).ok()?;
                        }
                        let path = cache_path(&self.repos_root, &reference, git.branch.as_deref());
                        info(path)
                    }
                }
            })
            .collect()
    }

    /// `refresh` (reference.ts:76-94): ensure every git reference with
    /// `refresh: "daily"`; failures log-and-continue.
    pub fn refresh_all(&self) {
        for info in self.list() {
            let Source::Git(source) = &info.source else {
                continue;
            };
            let Ok(reference) = parse_remote(&source.repository) else {
                continue;
            };
            let result = (self.ensure)(&EnsureInput {
                reference,
                branch: source.branch.clone(),
                refresh: Refresh::Daily,
            });
            if let Err(error) = result {
                eprintln!("[reference] failed to materialize {}: {error}", info.name);
            }
        }
    }

    /// The notify leg (reference.ts:95-109): fork the refresh off the
    /// mutation (v2 `Effect.forkIn`), then publish `reference.updated`.
    /// Both fall back to no-ops outside a runtime.
    fn notify(self: &Arc<Self>) {
        self.spawn_refresh();
        self.publish_updated();
    }

    /// `bus.publish(Reference.Event.Updated, {})` (reference.ts:107).
    fn publish_updated(self: &Arc<Self>) {
        let Some(bus) = self.bus.lock().clone() else { return };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let service = Arc::clone(self);
            handle.spawn(async move {
                let _ = bus
                    .publish(
                        registry::by_type(registry::REFERENCE_UPDATED)
                            .unwrap_or_else(|| unreachable!("reference.updated registered")),
                        serde_json::json!({}),
                        PublishOptions { global: true, ..Default::default() },
                    )
                    .await;
                drop(service);
            });
        }
    }

    /// Forks the refresh off the mutation (v2 `Effect.forkIn`). Falls back
    /// to a no-op outside a runtime.
    fn spawn_refresh(self: &Arc<Self>) {
        if !self.refresh_on_mutation {
            return;
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let service = Arc::clone(self);
            handle.spawn(async move {
                service.refresh_all();
            });
        }
    }

    /// The hourly sweep (reference.ts:111-112): refresh forever, one hour
    /// apart, independent of session activity — the shared cache throttles
    /// the actual Git work daily. Returns the task handle so the host can
    /// stop it.
    pub fn spawn_hourly(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                service.refresh_all();
            }
        })
    }
}
