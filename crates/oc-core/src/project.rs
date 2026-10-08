//! Project / workspace resolution — port of `packages/core/src/project.ts`
//! (§3f of the core deep map).
//!
//! `resolve(cwd)`: walk up to the nearest `.git`/`.hg` marker (git preferred
//! within a directory). Git identity = remote-derived (`https`/scp URL →
//! lowercased host + path-minus-`.git`, hashed) ?? cached commondir id ??
//! first root commit; canonical directory = the main worktree. Hg identity =
//! cached id ?? first root node (`hg` shelled out, graceful when absent).
//! No VCS: identity hashes `"directory:<path>"`. Persistence goes through
//! the [`ProjectStore`] trait (the sqlite implementation lands with the
//! database lane; [`MemoryProjectStore`] serves tests).

use crate::config::unpoison;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use sha2::{Digest, Sha256};

/// `Hash.fast` (v2 uses sha1; any stable content hash preserves identity
/// semantics — v2 cross-language ids are never persisted across hosts).
#[must_use]
pub fn hash_fast(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
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
}

/// Persistence boundary for project resolution.
pub trait ProjectStore: Send + Sync {
    fn project(&self, id: &str) -> Option<ProjectRow>;
    /// Insert-or-update: only the vcs label refreshes; the canonical
    /// directory is deliberately preserved (clones share the id).
    fn upsert_project(&self, row: &ProjectRow);
    fn reassign_canonical(&self, id: &str, canonical: &Path);
    fn worktree(&self, project_id: &str, directory: &Path) -> Option<WorktreeRow>;
    fn insert_worktree(&self, row: &WorktreeRow);
    #[must_use]
    fn list_projects(&self) -> Vec<ProjectRow>;
    #[must_use]
    fn list_worktrees(&self) -> Vec<WorktreeRow>;
}

/// In-memory store (tests; sqlite impl TODO(database-lane)).
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

impl ProjectStore for MemoryProjectStore {
    fn project(&self, id: &str) -> Option<ProjectRow> {
        self.projects.lock().unwrap_or_else(unpoison).iter().find(|row| row.id == id).cloned()
    }

    fn upsert_project(&self, row: &ProjectRow) {
        let mut projects = self.projects.lock().unwrap_or_else(unpoison);
        match projects.iter_mut().find(|existing| existing.id == row.id) {
            Some(existing) => existing.vcs.clone_from(&row.vcs),
            None => projects.push(row.clone()),
        }
    }

    fn reassign_canonical(&self, id: &str, canonical: &Path) {
        let mut projects = self.projects.lock().unwrap_or_else(unpoison);
        if let Some(row) = projects.iter_mut().find(|row| row.id == id) {
            row.canonical = canonical.to_path_buf();
        }
    }

    fn worktree(&self, project_id: &str, directory: &Path) -> Option<WorktreeRow> {
        self.worktrees
            .lock()
            .unwrap_or_else(unpoison)
            .iter()
            .find(|row| row.project_id == project_id && row.directory == directory)
            .cloned()
    }

    fn insert_worktree(&self, row: &WorktreeRow) {
        let mut worktrees = self.worktrees.lock().unwrap_or_else(unpoison);
        if !worktrees.iter().any(|existing| {
            existing.project_id == row.project_id && existing.directory == row.directory
        }) {
            worktrees.push(row.clone());
        }
    }

    fn list_projects(&self) -> Vec<ProjectRow> {
        self.projects.lock().unwrap_or_else(unpoison).clone()
    }

    fn list_worktrees(&self) -> Vec<WorktreeRow> {
        self.worktrees.lock().unwrap_or_else(unpoison).clone()
    }
}

/// Resolution events (bus integration lands later).
pub trait ProjectEvents: Send + Sync {
    fn project_updated(&self, _row: &ProjectRow) {}
    fn worktree_resolved(&self, _event: &WorktreeResolved) {}
}

/// No-op events.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoEvents;

impl ProjectEvents for NoEvents {}

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
    #[must_use]
    pub fn resolve(&self, input: &Path) -> Resolved {
        let directory = crate::config::resolve_path(input);
        let marker = walk_to_marker(&directory);
        if let Some((dir, _)) = marker.as_ref().filter(|(_, marker)| *marker == ".git") {
            if let Some(resolved) = Self::resolve_git(dir) {
                return self.persist(resolved);
            }
        }
        if let Some((dir, _)) = marker.filter(|(_, marker)| *marker == ".hg") {
            let store = dir.join(".hg");
            let previous = cached_id(&store);
            let id = previous
                .clone()
                .or_else(|| hg_root_node(&dir))
                .unwrap_or_else(|| "global".to_string());
            return self.persist(Resolved {
                previous,
                id,
                directory: dir.clone(),
                canonical: dir,
                vcs: Some(Vcs::Hg { store }),
            });
        }
        let id = hash_fast(&format!("directory:{}", directory.display()));
        self.persist(Resolved {
            previous: None,
            id,
            directory: directory.clone(),
            canonical: directory,
            vcs: None,
        })
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

    fn persist(&self, project: Resolved) -> Resolved {
        let previous_canonical = self.store.project(&project.id).map(|row| row.canonical);
        self.store.upsert_project(&ProjectRow {
            id: project.id.clone(),
            canonical: project.canonical.clone(),
            vcs: project.vcs.as_ref().map(|vcs| vcs.kind().to_string()),
        });
        // Clones share a project id; only replace a canonical directory that
        // is gone.
        if let Some(previous) = &previous_canonical {
            if *previous != project.canonical && !previous.exists() {
                self.store.reassign_canonical(&project.id, &project.canonical);
                if let Some(row) = self.store.project(&project.id) {
                    self.events.project_updated(&row);
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
            if self.store.worktree(&project.id, &directory).is_some() {
                continue;
            }
            let resolved_directory = crate::config::resolve_path(&directory);
            self.store.insert_worktree(&WorktreeRow {
                project_id: project.id.clone(),
                directory: resolved_directory,
                strategy,
            });
            self.events.worktree_resolved(&WorktreeResolved {
                project_id: project.id.clone(),
                directory,
                previous: project.previous.clone().unwrap_or_else(|| "global".to_string()),
            });
        }
        project
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

    #[test]
    fn config_project_normalize_remote_urls() {
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

    #[test]
    fn config_project_markerless_directory_identity_is_deterministic() {
        let dir = tempdir("project-markerless");
        let nested = dir.join("notes").join("drafts");
        fs::create_dir_all(&nested).unwrap();
        let resolver = resolver();
        let result = resolver.resolve(&dir);
        let repeated = resolver.resolve(&dir.join("."));
        let child = resolver.resolve(&nested);
        assert_ne!(result.id, "global");
        assert_eq!(repeated.id, result.id);
        assert_ne!(child.id, result.id);
        assert_eq!(result.directory, crate::config::resolve_path(&dir));
        assert_eq!(result.canonical, result.directory);
        assert_eq!(result.previous, None);
        assert_eq!(result.vcs, None);
    }

    #[test]
    fn config_project_remote_identity_wins_with_https_scp_equivalence() {
        let ssh = tempdir("project-remote-ssh");
        let https = tempdir("project-remote-https");
        init_repo(&ssh, true, Some("git@github.com:owner/repo.git"));
        init_repo(&https, true, Some("https://github.com/owner/repo.git"));
        let resolver = resolver();
        let a = resolver.resolve(&ssh);
        let b = resolver.resolve(&https);
        assert_eq!(a.id, remote_identity("github.com/owner/repo"));
        assert_eq!(b.id, a.id);
        assert_eq!(a.vcs.as_ref().map(Vcs::kind), Some("git"));
    }

    #[test]
    fn config_project_falls_back_to_root_commit_without_remote() {
        let dir = tempdir("project-root-commit");
        init_repo(&dir, true, None);
        let resolver = resolver();
        let result = resolver.resolve(&dir);
        // Same oid the git CLI reports for `rev-list --max-parents=0 HEAD`.
        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(result.id, repo.head().unwrap().target().unwrap().to_string());
        assert_eq!(result.directory, crate::config::resolve_path(&dir));
        assert_eq!(result.canonical, result.directory);
    }

    #[test]
    fn config_project_file_remote_ignored() {
        let dir = tempdir("project-file-remote");
        init_repo(&dir, true, Some(&format!("file://{}", dir.display())));
        let result = resolver().resolve(&dir);
        let repo = git2::Repository::open(&dir).unwrap();
        assert_eq!(result.id, repo.head().unwrap().target().unwrap().to_string());
    }

    #[test]
    fn config_project_global_for_empty_repo() {
        let dir = tempdir("project-empty-repo");
        init_repo(&dir, false, None);
        let result = resolver().resolve(&dir);
        assert_eq!(result.id, "global");
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("git"));
    }

    #[test]
    fn config_project_cached_commondir_id_reported_as_previous() {
        let dir = tempdir("project-cached");
        init_repo(&dir, true, Some("git@github.com:owner/repo.git"));
        let store = common_directory(&git2::Repository::open(&dir).unwrap());
        fs::write(store.join("opencode"), "old-id\n").unwrap();
        let result = resolver().resolve(&dir);
        assert_eq!(result.previous.as_deref(), Some("old-id"));
        assert_eq!(result.id, remote_identity("github.com/owner/repo"));
        // Resolution never writes the cache.
        assert_eq!(fs::read_to_string(store.join("opencode")).unwrap(), "old-id\n");
    }

    #[test]
    fn config_project_resolves_from_nested_directories() {
        let dir = tempdir("project-nested");
        init_repo(&dir, true, None);
        let nested = dir.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        let result = resolver().resolve(&nested);
        assert_eq!(result.directory, crate::config::resolve_path(&dir));
    }

    #[test]
    fn config_project_git_preferred_over_hg_in_same_dir() {
        let dir = tempdir("project-git-hg");
        init_repo(&dir, true, None);
        fs::create_dir_all(dir.join(".hg")).unwrap();
        let result = resolver().resolve(&dir);
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("git"));
    }

    #[test]
    fn config_project_nearest_marker_wins() {
        let dir = tempdir("project-nearest");
        let nested = dir.join("nested");
        init_repo(&dir, true, None);
        fs::create_dir_all(nested.join(".hg")).unwrap();
        fs::create_dir_all(nested.join("app")).unwrap();
        let result = resolver().resolve(&nested.join("app"));
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("hg"));
        assert_eq!(result.directory, crate::config::resolve_path(&nested));
    }

    #[test]
    fn config_project_unreadable_hg_falls_back_to_global() {
        let dir = tempdir("project-hg-global");
        fs::create_dir_all(dir.join(".hg")).unwrap();
        let result = resolver().resolve(&dir);
        assert_eq!(result.vcs.as_ref().map(Vcs::kind), Some("hg"));
        assert_eq!(result.id, "global");
    }

    #[test]
    fn config_project_persists_project_and_worktree_rows() {
        struct Recorder {
            resolved: std::sync::Mutex<Vec<WorktreeResolved>>,
        }
        impl ProjectEvents for Recorder {
            fn worktree_resolved(&self, event: &WorktreeResolved) {
                self.resolved.lock().unwrap().push(event.clone());
            }
        }
        let dir = tempdir("project-persist");
        init_repo(&dir, true, Some("git@github.com:owner/repo.git"));
        let store = Arc::new(MemoryProjectStore::new());
        let events = Arc::new(Recorder { resolved: std::sync::Mutex::new(Vec::new()) });
        let resolver = Resolver::new(store.clone(), events.clone());
        let first = resolver.resolve(&dir);
        let id = remote_identity("github.com/owner/repo");
        assert_eq!(first.id, id);
        assert_eq!(store.list_projects().len(), 1);
        assert_eq!(store.list_worktrees().len(), 1);
        assert_eq!(store.list_worktrees()[0].directory, crate::config::resolve_path(&dir));
        // Repeated resolution publishes nothing new.
        let _ = resolver.resolve(&dir);
        assert_eq!(events.resolved.lock().unwrap().len(), 1);
        assert_eq!(store.list_projects().len(), 1);
    }

    #[test]
    fn config_project_repoints_canonical_only_when_stored_one_vanished() {
        struct Updated {
            count: std::sync::Mutex<usize>,
        }
        impl ProjectEvents for Updated {
            fn project_updated(&self, _row: &ProjectRow) {
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
        let initial = resolver.resolve(&before);
        assert_eq!(*events.count.lock().unwrap(), 0);

        // Rename the canonical checkout and resolve the new path (same repo:
        // the remote, hence the identity, is unchanged by the rename).
        fs::rename(&before, &after).unwrap();
        let renamed = resolver.resolve(&after);
        assert_eq!(renamed.id, initial.id);
        assert_eq!(renamed.canonical, crate::config::resolve_path(&after));
        assert_eq!(*events.count.lock().unwrap(), 1, "canonical re-point announced");
        assert_eq!(
            store.project(&initial.id).unwrap().canonical,
            crate::config::resolve_path(&after)
        );
    }

    #[test]
    fn config_project_root_finds_nearest_vcs_ancestor() {
        let dir = tempdir("project-root-fn");
        let repo = dir.join("repo");
        init_repo(&repo, true, None);
        let nested = repo.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        assert_eq!(root(&nested), Some(crate::config::resolve_path(&repo)));
        assert_eq!(root(&dir), None);
    }
}
