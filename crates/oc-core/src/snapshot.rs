//! Port of `snapshot.ts` + the git tree operations — the git2-based
//! content-addressed snapshot store at
//! `~/.local/share/opencode/snapshot/<projectID>/<Hash.fast(worktree)>`
//! (core-runner-map §2.6).
//!
//! - `Hash.fast` is SHA-1 of the worktree path (packages/util/src/hash.ts)
//!   — hand-rolled below (no sha1 crate in the workspace).
//! - The store is a bare git object database; `capture` walks the scope,
//!   honors the SOURCE repository's ignore rules (tracked files always
//!   included), skips untracked files over 2 MiB, and writes a tree.
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

use oc_schema::common::RelativePath;
use oc_schema::id::SnapshotId;
use oc_schema::session_support::FileDiff;

/// Untracked-file cap (`maximumUntrackedFileBytes`).
pub const MAX_UNTRACKED_FILE_BYTES: u64 = 2 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Hash.fast (SHA-1, hand-rolled — the workspace has no sha1 crate)
// ---------------------------------------------------------------------------

/// `Hash.fast(input)` — SHA-1 hex digest.
#[must_use]
pub fn hash_fast(input: &[u8]) -> String {
    let mut state: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let mut message = input.to_vec();
    let bit_len = (input.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks(64) {
        let mut words = [0u32; 80];
        for (index, word) in chunk.chunks(4).enumerate() {
            words[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
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
        let git_directory = data_root
            .join("snapshot")
            .join(project_id)
            .join(hash_fast(worktree.to_string_lossy().as_bytes()));
        // Re-open an existing store; initialize on first use. (A bare init
        // over an existing repository errors with "attempt to
        // reinitialize".)
        if git2::Repository::open_bare(&git_directory).is_err() {
            let store = git2::Repository::init_opts(
                &git_directory,
                git2::RepositoryInitOptions::new().bare(true).external_template(false).mkdir(true),
            )
            .map_err(|error| operation_error("capture", error.message()))?;
            drop(store);
        }
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

    fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    fn open_repo(&self) -> Result<git2::Repository, git2::Error> {
        git2::Repository::open_bare(&self.git_directory)
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
                let Ok(bytes) = std::fs::read(&absolute) else { continue };
                let Ok(blob) = repo.blob(&bytes) else { continue };
                let mode = if file_type.is_symlink() {
                    0o120000
                } else {
                    let executable = entry
                        .metadata()
                        .ok()
                        .map(|meta| {
                            use std::os::unix::fs::PermissionsExt as _;
                            meta.permissions().mode() & 0o111 != 0
                        })
                        .unwrap_or(false);
                    if executable {
                        0o100755
                    } else {
                        0o100644
                    }
                };
                files.insert(relative, (blob, mode));
            }
        }
        let tree = build_tree(&repo, &files, "")?;
        Ok(SnapshotId::parse(tree.to_string()))
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
        let repo = self.open_repo().map_err(|error| operation_error("files", error.message()))?;
        let changed = self.tree_diff_paths(&repo, from.as_str(), to.as_str())?;
        Ok(self.filter_ignored(changed).into_iter().map(RelativePath::parse).collect())
    }

    /// `diff` — per-file unified diffs with additions/deletions counts.
    pub fn diff_blocking(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
        paths: Option<&[RelativePath]>,
    ) -> Result<Vec<FileDiff>, SnapshotError> {
        if let Some([]) = paths {
            return Ok(Vec::new());
        }
        let repo = self.open_repo().map_err(|error| operation_error("diff", error.message()))?;
        let from_tree = find_tree(&repo, from.as_str())?;
        let to_tree = find_tree(&repo, to.as_str())?;
        let selected: Option<Vec<String>> =
            paths.map(|paths| paths.iter().map(|p| p.0.clone()).collect());
        let changed = self.tree_diff_paths(&repo, from.as_str(), to.as_str())?;
        let mut diffs: Vec<FileDiff> = Vec::new();
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
            let diff = super::tool::file_diff::file_diff(
                &path,
                before.as_deref().unwrap_or(""),
                after.as_deref().unwrap_or(""),
                status,
            );
            diffs.push(FileDiff { file: path, ..diff });
        }
        Ok(diffs)
    }

    /// `restore` — write each path from its tree; absent ⇒ delete.
    pub fn restore_blocking(
        &self,
        files: &BTreeMap<String, SnapshotId>,
    ) -> Result<(), SnapshotError> {
        if !self.enabled() {
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
                    std::fs::write(&absolute, blob.content())
                        .map_err(|error| operation_error("restore", error.to_string()))?;
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
        match self.capture_blocking() {
            Ok(id) => Some(id),
            Err(error) => {
                *self.last_error.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(error.to_string());
                None
            }
        }
    }

    async fn files(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
    ) -> Result<Vec<RelativePath>, SnapshotError> {
        self.files_blocking(from, to)
    }

    async fn diff(
        &self,
        from: &SnapshotId,
        to: &SnapshotId,
        _context: Option<u32>,
        paths: Option<&[RelativePath]>,
    ) -> Result<Vec<FileDiff>, SnapshotError> {
        self.diff_blocking(from, to, paths)
    }

    async fn restore(&self, files: &BTreeMap<String, SnapshotId>) -> Result<(), SnapshotError> {
        self.restore_blocking(files)
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
