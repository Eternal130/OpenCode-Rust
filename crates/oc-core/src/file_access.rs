//! Port of `packages/core/src/file-access.ts` — lexical path resolution and
//! external-directory approval for file tools.
//!
//! - `resolve` maps a user-supplied path to an absolute path **lexically**
//!   (tilde expansion, no symlink resolution) and classifies it internal
//!   (inside `location.directory` OR the project root, unless the root is the
//!   filesystem root) or external.
//! - Internal targets carry a posix path relative to the location directory
//!   (`.` for the directory itself).
//! - External targets carry an [`ExternalDirectoryAuthorization`]:
//!   `external_directory` action over `"<dir>/*"` with a save glob anchored at
//!   the target directory's project root (nearest `.git`/`.hg` ancestor).
//! - `authorize_external` dedupes by resource and issues ONE permission
//!   assert carrying every external resource + save glob.
//! - `authorize_read` obtains external approval first, unless the target is a
//!   sibling (same dirname) of an already-authorized target, then asserts
//!   `read` on the target resource with save `["*"]`.

#![allow(clippy::must_use_candidate)] // pure path helpers

use std::fs;
use std::path::{Component, Path, PathBuf};

use oc_schema::common::Record;
use oc_schema::id::{AgentId, SessionId};
use oc_schema::permission::{Effect, Rule, Ruleset, Source};
use oc_util::global_roots::Roots;

use crate::permission::{AssertInput, Permission, PermissionError};

/// `FileAccess` action for external directory approval.
pub const EXTERNAL_DIRECTORY: &str = "external_directory";

/// Marker files identifying a project root (git worktrees use a `.git` FILE).
pub const PROJECT_MARKERS: [&str; 2] = [".git", ".hg"];

/// `FileAccess.Kind` — selects the external approval boundary without
/// validating the target type on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
}

/// `FileAccess.ResolveInput`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveInput {
    pub path: String,
    pub kind: Option<Kind>,
}

/// `FileAccess.ExternalDirectoryAuthorization`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDirectoryAuthorization {
    /// Always [`EXTERNAL_DIRECTORY`].
    pub action: &'static str,
    /// Lexical directory used as the external approval boundary.
    pub directory: PathBuf,
    /// `"<directory>/*"`.
    pub resource: String,
    /// `"<project-root(directory) or directory>/*"`.
    pub save: String,
}

/// `FileAccess.Target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub absolute: PathBuf,
    /// Location-relative posix path for internal targets, absolute posix path
    /// for external ones.
    pub resource: String,
    pub external_directory: Option<ExternalDirectoryAuthorization>,
}

/// Tool invocation context (`FileAccess.Invocation`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub session_id: SessionId,
    pub agent: Option<AgentId>,
    pub message_id: String,
    /// Tool call id.
    pub id: String,
}

impl Invocation {
    fn source(&self) -> Source {
        Source::Tool { message_id: self.message_id.clone(), id: self.id.clone() }
    }
}

/// `FileAccess.ReadOptions` — filename recovery against an already-authorized
/// sibling target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOptions {
    pub sibling_of: Target,
}

/// `resolvePath` — expand a leading `~`, then lexically resolve against
/// `directory` (like `path.resolve`: pure normalization, no fs access).
pub fn resolve_path(directory: &Path, input: &str, home: &Path) -> PathBuf {
    let expanded: PathBuf = if input == "~" {
        home.to_path_buf()
    } else if let Some(rest) = input.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(input)
    };
    lexical_resolve(directory, &expanded)
}

/// Lexical `path.resolve(base, path)`: join when relative, then drop `.` and
/// collapse `..` without touching the filesystem.
pub fn lexical_resolve(base: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::RootDir | Component::Prefix(_) => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

/// Lexical `path.relative(base, path)` with posix separators; `.` when equal.
/// Both inputs must be lexically absolute (no `..` remainders).
pub fn lexical_relative(base: &Path, path: &Path) -> String {
    let base_parts: Vec<Component<'_>> = base.components().collect();
    let path_parts: Vec<Component<'_>> = path.components().collect();
    let mut common = 0;
    while common < base_parts.len()
        && common < path_parts.len()
        && base_parts[common] == path_parts[common]
    {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..base_parts.len() {
        parts.push("..".to_string());
    }
    for component in &path_parts[common..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// `child` equals `parent` or lives under it (lexical, component-wise —
/// mirrors `FSUtil.contains`).
pub fn path_contains(parent: &Path, child: &Path) -> bool {
    child.strip_prefix(parent).is_ok()
}

/// `Project.root` — nearest ancestor of `start` (inclusive) holding a `.git`
/// or `.hg` marker (any type; filesystem probe only). `None` when untracked.
pub fn project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        for marker in PROJECT_MARKERS {
            if current.join(marker).exists() {
                return Some(current.to_path_buf());
            }
        }
        current = current.parent()?;
    }
}

fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn is_fs_root(path: &Path) -> bool {
    path.parent().is_none()
}

/// `FileAccess` service for one location.
#[derive(Clone)]
pub struct FileAccess {
    permission: Permission,
    /// `location.directory`.
    directory: PathBuf,
    /// `location.project.directory` (worktree boundary).
    project_directory: PathBuf,
    home: PathBuf,
}

impl FileAccess {
    /// Assembles the service. All paths must be lexically absolute (no `..`
    /// remainders); `directory`/`project_directory` come from the location,
    /// `home` from the global roots.
    pub fn new(
        permission: Permission,
        directory: PathBuf,
        project_directory: PathBuf,
        home: PathBuf,
    ) -> Self {
        Self { permission, directory, project_directory, home }
    }

    /// Resolve a lexical path and its permission resources without requesting
    /// approval. Never fails (stat errors degrade to the parent directory).
    pub fn resolve(&self, input: &ResolveInput) -> Target {
        let absolute = resolve_path(&self.directory, &input.path, &self.home);
        let worktree = lexical_resolve(Path::new("/"), &self.project_directory);
        let internal = path_contains(&self.directory, &absolute)
            || (!is_fs_root(&worktree) && path_contains(&worktree, &absolute));
        if internal {
            return Target {
                resource: lexical_relative(&self.directory, &absolute),
                absolute,
                external_directory: None,
            };
        }
        let is_dir = match input.kind {
            Some(Kind::Directory) => true,
            Some(Kind::File) => false,
            // fs.stat follows symlinks; any error (missing target, EACCES…)
            // degrades to "not a directory" ⇒ boundary is the parent.
            None => fs::metadata(&absolute).is_ok_and(|meta| meta.is_dir()),
        };
        let directory = if is_dir {
            absolute.clone()
        } else {
            absolute.parent().map_or_else(|| absolute.clone(), Path::to_path_buf)
        };
        let save_root = project_root(&directory).unwrap_or_else(|| directory.clone());
        Target {
            resource: slash(&absolute),
            external_directory: Some(ExternalDirectoryAuthorization {
                action: EXTERNAL_DIRECTORY,
                resource: slash(&directory.join("*")),
                save: slash(&save_root.join("*")),
                directory,
            }),
            absolute,
        }
    }

    /// Approve every external directory referenced by `targets` in ONE
    /// permission assert, preserving first-seen resource order (dedupe by
    /// resource).
    pub async fn authorize_external(
        &self,
        targets: &[Target],
        context: &Invocation,
        metadata: Option<Record>,
    ) -> Result<(), PermissionError> {
        let mut external: Vec<&ExternalDirectoryAuthorization> = Vec::new();
        for target in targets {
            let Some(auth) = target.external_directory.as_ref() else { continue };
            if !external.iter().any(|seen| seen.resource == auth.resource) {
                external.push(auth);
            }
        }
        if external.is_empty() {
            return Ok(());
        }
        self.permission
            .assert(AssertInput {
                id: None,
                session_id: context.session_id.clone(),
                agent: context.agent.clone(),
                action: EXTERNAL_DIRECTORY.to_string(),
                resources: external.iter().map(|auth| auth.resource.clone()).collect(),
                save: Some(external.iter().map(|auth| auth.save.clone()).collect()),
                metadata,
                source: Some(context.source()),
            })
            .await
    }

    /// Resolve a read target and obtain external-directory approval before
    /// read approval. Siblings (same dirname) of an already-authorized target
    /// share its directory approval but still check their own read rules.
    pub async fn authorize_read(
        &self,
        file: &str,
        context: &Invocation,
        options: Option<&ReadOptions>,
    ) -> Result<Target, PermissionError> {
        let target = self
            .resolve(&ResolveInput { path: file.to_string(), kind: options.map(|_| Kind::File) });
        let sibling = options.is_some_and(|opts| {
            target
                .absolute
                .parent()
                .is_some_and(|parent| Some(parent) == opts.sibling_of.absolute.parent())
        });
        if !sibling {
            self.authorize_external(std::slice::from_ref(&target), context, None).await?;
        }
        self.permission
            .assert(AssertInput {
                id: None,
                session_id: context.session_id.clone(),
                agent: context.agent.clone(),
                action: "read".to_string(),
                resources: vec![target.resource.clone()],
                save: Some(vec!["*".to_string()]),
                metadata: None,
                source: Some(context.source()),
            })
            .await?;
        Ok(target)
    }
}

/// `SHELL_OUTPUT_GLOB(data)` — `<data>/shell/*/*`.
pub fn shell_output_glob(data: &Path) -> String {
    slash(&data.join("shell").join("*").join("*"))
}

/// `TOOL_OUTPUT_GLOB(data)` — `<data>/tool-output/*`.
pub fn tool_output_glob(data: &Path) -> String {
    slash(&data.join("tool-output").join("*"))
}

/// Default external-directory allows merged into EVERY agent
/// (`agent.ts` `permissions`): shell output, tool output, tmp, and config.
pub fn default_agent_external_allows(roots: &Roots) -> Ruleset {
    let allow = |resource: String| Rule {
        action: EXTERNAL_DIRECTORY.to_string(),
        resource,
        effect: Effect::Allow,
    };
    vec![
        allow(shell_output_glob(&roots.data)),
        allow(tool_output_glob(&roots.data)),
        allow(slash(&roots.tmp.join("*"))),
        allow(slash(&roots.config.join("*"))),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::too_many_lines, clippy::items_after_statements, clippy::assert_is_empty)]

    use super::*;
    use crate::permission::{
        BroadcastEventSink, MemorySavedGrantStore, Permission, PermissionError, ReplyInput,
        RuleResolver,
    };
    use oc_schema::id::ProjectId;
    use oc_schema::misc_events::PermissionEvent;
    use oc_schema::permission::{Effect, Reply, Request, Rule, Ruleset};
    use oc_util::determinism::{Clock, FixedClock, IdGen, SequentialIdGen};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    fn rule(action: &str, resource: &str, effect: Effect) -> Rule {
        Rule { action: action.into(), resource: resource.into(), effect }
    }

    fn ruleset(entries: &[(&str, &str, Effect)]) -> Ruleset {
        entries.iter().map(|(a, r, e)| rule(a, r, *e)).collect()
    }

    struct FixedResolver {
        entries: HashMap<String, Option<Ruleset>>,
    }

    #[async_trait::async_trait]
    impl RuleResolver for FixedResolver {
        async fn resolve(
            &self,
            session_id: &SessionId,
            _agent: Option<&AgentId>,
        ) -> Result<(Option<Ruleset>, Option<Ruleset>), PermissionError> {
            self.entries
                .get(session_id.as_str())
                .cloned()
                .map(|agent| (agent, None))
                .ok_or(PermissionError::SessionNotFound(session_id.clone()))
        }
    }

    struct Tree {
        _tmp: tempfile::TempDir,
        root: PathBuf,    // tempdir root
        project: PathBuf, // has .git
        work: PathBuf,    // location.directory inside project
        other: PathBuf,   // external repo with its own .git
        home: PathBuf,
    }

    fn tree() -> Tree {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        let project = root.join("proj");
        let work = project.join("work");
        let other = root.join("other");
        std::fs::create_dir_all(work.join("sub")).expect("dirs");
        std::fs::create_dir_all(project.join("lib")).expect("dirs");
        std::fs::create_dir_all(other.join("nested")).expect("dirs");
        std::fs::create_dir(project.join(".git")).expect("git marker");
        std::fs::create_dir(other.join(".git")).expect("git marker");
        std::fs::write(work.join("sub").join("file.txt"), "x").expect("file");
        std::fs::write(project.join("lib").join("x.rs"), "x").expect("file");
        std::fs::write(other.join("nested").join("y.rs"), "y").expect("file");
        let home = root.join("home");
        std::fs::create_dir_all(&home).expect("home");
        Tree { _tmp: tmp, root, project, work, other, home }
    }

    fn access(
        tree: &Tree,
        agent: Option<Ruleset>,
    ) -> (FileAccess, Permission, tokio::sync::broadcast::Receiver<PermissionEvent>) {
        let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
        let events = BroadcastEventSink::new(256);
        let rx = events.subscribe();
        let saved = Arc::new(MemorySavedGrantStore::new(ids.clone(), clock.clone()));
        let resolver = Arc::new(FixedResolver {
            entries: [("ses_1".to_string(), agent)].into_iter().collect(),
        });
        let permission = Permission::new(
            ids,
            clock,
            Arc::new(events),
            saved,
            resolver,
            ProjectId("proj_1".into()),
            None,
        );
        let file_access = FileAccess::new(
            permission.clone(),
            tree.work.clone(),
            tree.project.clone(),
            tree.home.clone(),
        );
        (file_access, permission, rx)
    }

    fn invocation() -> Invocation {
        Invocation {
            session_id: SessionId("ses_1".into()),
            agent: None,
            message_id: "msg_1".into(),
            id: "call_1".into(),
        }
    }

    fn resolve_str(access: &FileAccess, path: &str) -> Target {
        access.resolve(&ResolveInput { path: path.into(), kind: None })
    }

    async fn wait_asked_and_reply(
        perm: &Permission,
        rx: &mut tokio::sync::broadcast::Receiver<PermissionEvent>,
        reply: Reply,
    ) -> Request {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("timeout waiting for asked")
                .expect("channel open");
            // Skip the replied-echo of our own earlier reply.
            let PermissionEvent::Asked(env) = event else { continue };
            let request = env.data.clone();
            perm.reply(ReplyInput { request_id: request.id.clone(), reply, message: None })
                .await
                .expect("reply");
            return request;
        }
    }

    // ------------------------------------------------------------- resolvePath

    #[test]
    fn file_access_resolve_path_tilde_and_lexical() {
        let base = Path::new("/base");
        let home = Path::new("/home/u");
        assert_eq!(resolve_path(base, "~/notes", home), PathBuf::from("/home/u/notes"));
        assert_eq!(resolve_path(base, "~", home), PathBuf::from("/home/u"));
        assert_eq!(resolve_path(base, "rel/../x", home), PathBuf::from("/base/x"));
        assert_eq!(resolve_path(base, "/abs/../a", home), PathBuf::from("/a"));
        assert_eq!(resolve_path(base, "..", home), PathBuf::from("/"));
        assert_eq!(resolve_path(base, ".", home), PathBuf::from("/base"));
    }

    #[test]
    fn file_access_lexical_relative_shapes() {
        let base = Path::new("/a/b");
        assert_eq!(lexical_relative(base, Path::new("/a/b")), ".");
        assert_eq!(lexical_relative(base, Path::new("/a/b/c/d")), "c/d");
        assert_eq!(lexical_relative(base, Path::new("/a/x")), "../x");
        assert_eq!(lexical_relative(base, Path::new("/")), "../..");
        assert_eq!(lexical_relative(Path::new("/"), Path::new("/etc/passwd")), "etc/passwd");
    }

    // ---------------------------------------------------------------- resolve

    #[test]
    fn file_access_resolve_internal_relative() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = resolve_str(&access, "sub/file.txt");
        assert_eq!(target.absolute, tree.work.join("sub").join("file.txt"));
        assert_eq!(target.resource, "sub/file.txt");
        assert!(target.external_directory.is_none());
    }

    #[test]
    fn file_access_resolve_location_root_is_dot() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = resolve_str(&access, ".");
        assert_eq!(target.resource, ".");
        assert!(target.external_directory.is_none());
    }

    #[test]
    fn file_access_resolve_inside_project_outside_directory() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = resolve_str(&access, "../lib/x.rs");
        assert_eq!(target.absolute, tree.project.join("lib").join("x.rs"));
        // Internal via project root, but relative to the LOCATION directory.
        assert_eq!(target.resource, "../lib/x.rs");
        assert!(target.external_directory.is_none());
    }

    #[test]
    fn file_access_resolve_project_root_itself_is_internal() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = resolve_str(&access, "..");
        assert_eq!(target.resource, "..");
        assert!(target.external_directory.is_none());
    }

    #[test]
    fn file_access_resolve_external_file_uses_parent_directory() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = resolve_str(&access, "../../other/nested/y.rs");
        assert_eq!(target.absolute, tree.other.join("nested").join("y.rs"));
        let external = target.external_directory.expect("external");
        assert_eq!(external.action, "external_directory");
        assert_eq!(external.directory, tree.other.join("nested"));
        assert_eq!(external.resource, format!("{}/*", tree.other.join("nested").to_string_lossy()));
        // Save glob anchored at the target directory's own project root.
        assert_eq!(external.save, format!("{}/*", tree.other.to_string_lossy()));
        // Target resource is the absolute posix path.
        assert_eq!(
            target.resource,
            tree.other.join("nested").join("y.rs").to_string_lossy().to_string()
        );
    }

    #[test]
    fn file_access_resolve_external_directory_kind_uses_target() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = access.resolve(&ResolveInput {
            path: "../../other/nested".into(),
            kind: Some(Kind::Directory),
        });
        let external = target.external_directory.expect("external");
        assert_eq!(external.directory, tree.other.join("nested"));
        assert_eq!(external.save, format!("{}/*", tree.other.to_string_lossy()));
    }

    #[test]
    fn file_access_resolve_file_kind_forces_parent_even_for_directories() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = access
            .resolve(&ResolveInput { path: "../../other/nested".into(), kind: Some(Kind::File) });
        let external = target.external_directory.expect("external");
        assert_eq!(external.directory, tree.other);
    }

    #[test]
    fn file_access_resolve_missing_target_degrades_to_parent() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = resolve_str(&access, "../../other/missing.rs");
        let external = target.external_directory.expect("external");
        assert_eq!(external.directory, tree.other);
    }

    #[test]
    fn file_access_resolve_existing_directory_detected_via_stat() {
        let tree = tree();
        let (access, _, _) = access(&tree, None);
        let target = resolve_str(&access, "../../other/nested");
        let external = target.external_directory.expect("external");
        assert_eq!(external.directory, tree.other.join("nested"));
    }

    #[test]
    fn file_access_resolve_symlinked_target_follows_stat_but_lexical_boundary() {
        let tree = tree();
        // Link inside the work dir pointing at the external directory.
        std::os::unix::fs::symlink(tree.other.join("nested"), tree.work.join("ext-link"))
            .expect("symlink");
        let (access, _, _) = access(&tree, None);
        // Lexically inside the work dir ⇒ internal, no external authorization.
        let target = resolve_str(&access, "ext-link/y.rs");
        assert!(target.external_directory.is_none());
        assert_eq!(target.resource, "ext-link/y.rs");

        // An external link (outside the project): stat follows the symlink so
        // the approval boundary is the link path itself, lexically.
        let link = tree.root.join("outside-link");
        std::os::unix::fs::symlink(tree.other.join("nested"), &link).expect("symlink");
        let target =
            access.resolve(&ResolveInput { path: link.to_string_lossy().into(), kind: None });
        let external = target.external_directory.expect("external");
        assert_eq!(external.directory, link);
        assert_eq!(external.resource, format!("{}/*", link.to_string_lossy()));
        // Project root walk is filesystem: `other` has .git but the link path
        // does not, and lexical walking from the link finds no marker ⇒ itself.
        assert_eq!(external.save, format!("{}/*", link.to_string_lossy()));
    }

    #[test]
    fn file_access_resolve_symlinked_location_directory_is_lexical() {
        let tree = tree();
        let real = tree.project.join("real");
        std::fs::create_dir_all(&real).expect("dir");
        let link = tree.project.join("worklink");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
        let events = BroadcastEventSink::new(16);
        let saved = Arc::new(MemorySavedGrantStore::new(ids.clone(), clock.clone()));
        let resolver = Arc::new(FixedResolver { entries: HashMap::new() });
        let permission = Permission::new(
            ids.clone(),
            clock.clone(),
            Arc::new(events),
            saved,
            resolver,
            ProjectId("proj_1".into()),
            None,
        );
        let access =
            FileAccess::new(permission, link.clone(), tree.project.clone(), tree.home.clone());
        let target = resolve_str(&access, "f.txt");
        // Lexical: no symlink resolution, resource stays location-relative.
        assert_eq!(target.absolute, link.join("f.txt"));
        assert_eq!(target.resource, "f.txt");
        assert!(target.external_directory.is_none());
    }

    #[test]
    fn file_access_fs_root_project_is_not_internal_boundary() {
        let tree = tree();
        let (_, permission, _) = access(&tree, None);
        let access = FileAccess::new(
            permission,
            PathBuf::from("/opt-opencode-missing-location"),
            PathBuf::from("/"),
            tree.home.clone(),
        );
        let target = resolve_str(&access, "/etc/passwd");
        let external = target.external_directory.expect("external");
        assert_eq!(external.directory, PathBuf::from("/etc"));
        assert_eq!(external.resource, "/etc/*");
        assert_eq!(external.save, "/etc/*"); // no marker anywhere up from /etc
    }

    // ------------------------------------------------------------- projectRoot

    #[test]
    fn file_access_project_root_walks_git_and_hg_markers() {
        let tree = tree();
        assert_eq!(project_root(&tree.work), Some(tree.project.clone()));
        assert_eq!(project_root(&tree.project.join("lib")), Some(tree.project.clone()));
        assert_eq!(project_root(&tree.project), Some(tree.project.clone()));
        assert_eq!(project_root(&tree.other.join("nested")), Some(tree.other.clone()));

        // `.git` as a FILE (linked worktrees) counts too.
        let hg_root = tree.root.join("hgproj");
        std::fs::create_dir_all(hg_root.join("sub")).expect("dir");
        std::fs::write(hg_root.join(".hg"), "").expect("hg marker");
        assert_eq!(project_root(&hg_root.join("sub")), Some(hg_root.clone()));
        assert_eq!(project_root(&hg_root), Some(hg_root));

        // Untracked tree ⇒ None.
        let plain = tree.root.join("plain");
        std::fs::create_dir_all(plain.join("deep")).expect("dir");
        assert_eq!(project_root(&plain.join("deep")), None);
    }

    // -------------------------------------------------------- authorize flows

    #[tokio::test]
    async fn file_access_authorize_external_dedupes_to_one_assert() {
        let tree = tree();
        let (access, perm, mut rx) = access(&tree, Some(ruleset(&[("*", "*", Effect::Ask)])));
        let a = resolve_str(&access, "../../other/nested/y.rs");
        let b = resolve_str(&access, "../../other/nested"); // same directory
        let c = resolve_str(&access, "../../other/missing.txt"); // dirname = other

        let task = tokio::spawn({
            let access = access.clone();
            let context = invocation();
            async move { access.authorize_external(&[a, b, c], &context, None).await }
        });

        let request = wait_asked_and_reply(&perm, &mut rx, Reply::Once).await;
        assert_eq!(request.action, "external_directory");
        // Two distinct directories (deduped by resource), first-seen order.
        let nested = format!("{}/*", tree.other.join("nested").to_string_lossy());
        let other = format!("{}/*", tree.other.to_string_lossy());
        assert_eq!(request.resources, vec![nested, other]);
        assert_eq!(request.save, Some(vec![format!("{}/*", tree.other.to_string_lossy()); 2]));
        assert_eq!(
            request.source,
            Some(Source::Tool { message_id: "msg_1".into(), id: "call_1".into() })
        );
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn file_access_authorize_external_no_external_targets_noop() {
        let tree = tree();
        let (access, perm, _rx) = access(&tree, Some(ruleset(&[("*", "*", Effect::Ask)])));
        let internal = resolve_str(&access, "sub/file.txt");
        access.authorize_external(&[internal], &invocation(), None).await.unwrap();
        assert!(perm.list().is_empty());
    }

    #[tokio::test]
    async fn file_access_authorize_read_external_then_read() {
        let tree = tree();
        let (access, perm, mut rx) = access(&tree, Some(ruleset(&[("*", "*", Effect::Ask)])));
        let task = tokio::spawn({
            let access = access.clone();
            let context = invocation();
            async move { access.authorize_read("../../other/nested/y.rs", &context, None).await }
        });

        // External directory approval first.
        let external = wait_asked_and_reply(&perm, &mut rx, Reply::Once).await;
        assert_eq!(external.action, "external_directory");
        assert_eq!(
            external.resources,
            vec![format!("{}/*", tree.other.join("nested").to_string_lossy())]
        );
        // Then the read assert on the target resource with universal save.
        let read = wait_asked_and_reply(&perm, &mut rx, Reply::Once).await;
        assert_eq!(read.action, "read");
        assert_eq!(
            read.resources,
            vec![tree.other.join("nested").join("y.rs").to_string_lossy().to_string()]
        );
        assert_eq!(read.save, Some(vec!["*".to_string()]));

        let target = task.await.unwrap().unwrap();
        assert!(target.external_directory.is_some());
    }

    #[tokio::test]
    async fn file_access_authorize_read_sibling_skips_external() {
        let tree = tree();
        let (access, perm, mut rx) = access(&tree, Some(ruleset(&[("*", "*", Effect::Ask)])));
        let authorized = resolve_str(&access, "../../other/nested/y.rs");
        let task = tokio::spawn({
            let access = access.clone();
            let context = invocation();
            let options = ReadOptions { sibling_of: authorized };
            async move {
                access.authorize_read("../../other/nested/z.rs", &context, Some(&options)).await
            }
        });

        // Only the read assert fires — the sibling shares directory approval.
        let read = wait_asked_and_reply(&perm, &mut rx, Reply::Once).await;
        assert_eq!(read.action, "read");
        assert_eq!(
            read.resources,
            vec![tree.other.join("nested").join("z.rs").to_string_lossy().to_string()]
        );
        assert_eq!(read.save, Some(vec!["*".to_string()]));
        task.await.unwrap().unwrap();
        assert!(perm.list().is_empty());
    }

    #[tokio::test]
    async fn file_access_authorize_read_non_sibling_requires_external() {
        let tree = tree();
        let (access, perm, mut rx) = access(&tree, Some(ruleset(&[("*", "*", Effect::Ask)])));
        let authorized = resolve_str(&access, "../../other/nested/y.rs"); // dirname: other/nested
        let task = tokio::spawn({
            let access = access.clone();
            let context = invocation();
            let options = ReadOptions { sibling_of: authorized };
            // Same `other` repo but a different directory ⇒ not a sibling.
            async move { access.authorize_read("../../other/root.rs", &context, Some(&options)).await }
        });

        let external = wait_asked_and_reply(&perm, &mut rx, Reply::Once).await;
        assert_eq!(external.action, "external_directory");
        let read = wait_asked_and_reply(&perm, &mut rx, Reply::Once).await;
        assert_eq!(read.action, "read");
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn file_access_default_agent_allows_external_roots_without_asking() {
        let tree = tree();
        let tmp_file = tree.root.join("global-data").join("shell").join("s1").join("out.txt");
        std::fs::create_dir_all(tmp_file.parent().unwrap()).expect("dirs");
        std::fs::write(&tmp_file, "x").expect("file");

        let roots = Roots {
            home: tree.home.clone(),
            data: tree.root.join("global-data"),
            cache: tree.root.join("cache"),
            config: tree.root.join("cfg"),
            state: tree.root.join("state"),
            tmp: tree.root.join("global-tmp"),
        };
        let mut agent = default_agent_external_allows(&roots);
        agent.push(rule("read", "*", Effect::Allow));

        let (access, perm, mut rx) = access(&tree, Some(agent));
        access.authorize_read(&tmp_file.to_string_lossy(), &invocation(), None).await.unwrap();
        // No ask events: external allow from the agent defaults, read allowed.
        assert!(perm.list().is_empty());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn file_access_default_globs_shape() {
        assert_eq!(shell_output_glob(Path::new("/d")), "/d/shell/*/*");
        assert_eq!(tool_output_glob(Path::new("/d")), "/d/tool-output/*");
    }
}
