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

use std::path::{Path, PathBuf};
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

/// `validAlias` (config/plugin/reference.ts:51-53): non-empty, and none
/// of `/`, whitespace, `` ` ``, `,`.
fn valid_alias(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c == '/' || c == '`' || c == ',' || c.is_whitespace())
}

/// `localPath` (config/plugin/reference.ts:63-66): `~/…` joins `home`,
/// absolute stays, relative resolves (lexically, Node `path.resolve`)
/// against the document directory.
fn local_path(directory: &std::path::Path, home: &std::path::Path, value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        let joined = home.join(rest);
        return lexical_normalize(&joined);
    }
    if std::path::Path::new(value).is_absolute() {
        return PathBuf::from(value);
    }
    lexical_normalize(&directory.join(value))
}

/// Node `path.resolve` lexical half: drop `.` components, apply `..`
/// without touching the filesystem.
fn lexical_normalize(input: &std::path::Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in input.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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
    pub fn new(repos_root: &Path, clock: Arc<dyn oc_util::determinism::Clock>) -> Self {
        Self::with_cache(
            repos_root.to_owned(),
            RepositoryCache::new(
                repos_root.to_owned(),
                clock,
                Arc::new(repository_cache::FileRefreshStore::new(repos_root)),
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

    /// Bulk insert-or-replace WITHOUT the notify leg — seeding a fresh
    /// service from an existing table (e.g. the engine's plugin-applied
    /// sources when a per-location service is assembled, G-270②).
    pub fn install(self: &Arc<Self>, entries: Vec<(String, Source)>) {
        let mut sources = self.sources.lock();
        for (name, source) in entries {
            match sources.iter_mut().find(|(existing, _)| *existing == name) {
                Some(slot) => slot.1 = source,
                None => sources.push((name, source)),
            }
        }
    }

    /// Bulk insert-or-replace + ONE notify — the shape of ref's single
    /// `ctx.reference.transform` per config load (G-270②): the config
    /// plugin applies every document's entries inside one editor call,
    /// so assembly-time seeding forks ONE refresh and publishes ONE
    /// `reference.updated`.
    pub fn apply(self: &Arc<Self>, entries: Vec<(String, Source)>) {
        self.install(entries);
        self.notify();
    }

    /// The config-plugin transform (core config/plugin/reference.ts:23-52,
    /// G-270②): every document's `references` map → `(name, source)`
    /// pairs. Mirrors the ref exactly:
    /// - `validAlias`: a name with `/`, whitespace, `` ` `` or `,` (or an
    ///   empty one) is skipped;
    /// - a bare string entry is LOCAL when it starts with `.`, `/` or
    ///   `~`, and a git shorthand otherwise;
    /// - `localPath`: `~/…` resolves against `home`, absolute paths stay,
    ///   relative paths resolve against the DOCUMENT's directory (its
    ///   `path` dirname) or the location directory for path-less docs.
    #[must_use]
    pub fn sources_from_config(
        entries: &[oc_schema::config::Entry],
        location: &std::path::Path,
        home: &std::path::Path,
    ) -> Vec<(String, Source)> {
        let mut out = Vec::new();
        for entry in entries {
            let oc_schema::config::Entry::Document(document) = entry else { continue };
            let directory = document.path.as_deref().map_or_else(
                || location.to_path_buf(),
                |path| {
                    std::path::Path::new(path)
                        .parent()
                        .map_or_else(|| location.to_path_buf(), std::path::Path::to_path_buf)
                },
            );
            let Some(map) = document.info.references.as_ref() else { continue };
            for (name, entry) in map {
                if !valid_alias(name) {
                    continue;
                }
                let (local, git) = match entry {
                    oc_schema::config::reference::Entry::Path(path) => {
                        if path.starts_with('.') || path.starts_with('/') || path.starts_with('~') {
                            (Some(path.as_str()), None)
                        } else {
                            (None, Some(path.as_str()))
                        }
                    }
                    oc_schema::config::reference::Entry::Git(git) => {
                        (None, Some(git.repository.as_str()))
                    }
                    oc_schema::config::reference::Entry::Local(local) => {
                        (Some(local.path.as_str()), None)
                    }
                };
                let description = match entry {
                    oc_schema::config::reference::Entry::Path(_) => None,
                    oc_schema::config::reference::Entry::Git(git) => git.description.clone(),
                    oc_schema::config::reference::Entry::Local(local) => local.description.clone(),
                };
                let hidden = match entry {
                    oc_schema::config::reference::Entry::Path(_) => None,
                    oc_schema::config::reference::Entry::Git(git) => git.hidden,
                    oc_schema::config::reference::Entry::Local(local) => local.hidden,
                };
                let branch = match entry {
                    oc_schema::config::reference::Entry::Git(git) => git.branch.clone(),
                    _ => None,
                };
                let source = if let Some(path) = local {
                    Source::Local(LocalSource {
                        path: local_path(&directory, home, path),
                        description,
                        hidden,
                    })
                } else {
                    let repository = git.unwrap_or_default().to_owned();
                    Source::Git(GitSource { repository, branch, description, hidden })
                };
                out.push((name.clone(), source));
            }
        }
        out
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
                depth: None,
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
