//! Source discovery — port of `config/discovery.ts`.
//!
//! Walks up from the location directory collecting `.claude`, `.agents`,
//! `.opencode`, `opencode.jsonc`, `opencode.json` candidates (symlink
//! resolved for visibility), filters global roots out of the project walk,
//! and orders direct files root-most first so the nearest file wins last.

use std::path::{Path, PathBuf};

use crate::config::{walk_up, GlobalDirs, Options, NAMES};

/// Candidates per directory, checked in this order.
pub const CANDIDATES: [&str; 5] =
    [".claude", ".agents", ".opencode", "opencode.jsonc", "opencode.json"];

/// An eligible `.opencode` project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot {
    pub path: PathBuf,
    pub present: bool,
}

/// Eligible sources in priority order, including paths that may appear later.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sources {
    pub global: Option<PathBuf>,
    pub explicit: Option<PathBuf>,
    /// Direct `opencode.json(c)` files, root-most first (nearest wins last).
    pub direct: Vec<PathBuf>,
    /// `.opencode` directories, root-most first.
    pub project: Vec<ProjectRoot>,
    /// `~/.claude` plus discovered `.claude` dirs (nearest last), deduped.
    pub claude: Vec<PathBuf>,
    /// `~/.agents` plus discovered `.agents` dirs (nearest last), deduped.
    pub agents: Vec<PathBuf>,
}

/// Discovers config sources for a location. `project: false` skips the walk;
/// `global: false` excludes global roots (and their subtrees) entirely.
#[must_use]
pub fn discover(location: &Path, globals: &GlobalDirs, options: &Options) -> Sources {
    let global_directory = globals.config.clone();
    let global_agents_directory = globals.home.join(".agents");
    let global_claude_directory = globals.home.join(".claude");

    let global_roots: Vec<PathBuf> = [
        global_directory.clone(),
        global_claude_directory.clone(),
        global_agents_directory.clone(),
    ]
    .iter()
    .map(|root| crate::config::resolve_path(root))
    .collect();

    let skip_walk =
        options.project == Some(false) || crate::config::resolve_path(location) == global_roots[0];
    let directories: Vec<PathBuf> = if skip_walk {
        Vec::new()
    } else {
        walk_up(location).into_iter().filter(|dir| dir.exists()).collect()
    };

    // Resolve each candidate through its resolved parent so missing children
    // still honor symlinked global roots.
    let mut discovered: Vec<(PathBuf, PathBuf)> = Vec::new();
    for directory in &directories {
        let parent = crate::config::resolve_path(directory);
        for name in CANDIDATES {
            let resolved = crate::config::resolve_path(&parent.join(name));
            discovered.push((directory.join(name), resolved));
        }
    }

    let global_enabled = options.global != Some(false);
    let global_files: Vec<PathBuf> = NAMES
        .iter()
        .map(|name| crate::config::resolve_path(&global_directory.join(name)))
        .collect();

    // Global sources must not re-enter through the project walk.
    let visible: Vec<PathBuf> = discovered
        .into_iter()
        .filter(|(_, resolved)| {
            if global_enabled {
                !global_roots.contains(resolved) && !global_files.contains(resolved)
            } else {
                !global_roots.iter().any(|root| resolved == root || resolved.starts_with(root))
            }
        })
        .map(|(item, _)| item)
        .collect();

    let is_config_dir = |item: &Path| {
        matches!(
            item.file_name().and_then(|name| name.to_str()),
            Some(".claude" | ".agents" | ".opencode")
        )
    };
    let basename_is =
        |item: &Path, name: &str| item.file_name().and_then(|n| n.to_str()) == Some(name);

    let mut direct: Vec<PathBuf> =
        visible.iter().filter(|item| !is_config_dir(item)).cloned().collect();
    direct.reverse();

    let mut project_dirs: Vec<PathBuf> =
        visible.iter().filter(|item| basename_is(item, ".opencode")).cloned().collect();
    project_dirs.reverse();
    let project: Vec<ProjectRoot> =
        project_dirs.into_iter().map(|path| ProjectRoot { present: path.is_dir(), path }).collect();

    let mut claude: Vec<PathBuf> = Vec::new();
    if global_enabled {
        claude.push(global_claude_directory);
    }
    let mut claude_discovered: Vec<PathBuf> =
        visible.iter().filter(|item| basename_is(item, ".claude")).cloned().collect();
    claude_discovered.reverse();
    for item in claude_discovered {
        if !claude.contains(&item) {
            claude.push(item);
        }
    }

    let mut agents: Vec<PathBuf> = Vec::new();
    if global_enabled {
        agents.push(global_agents_directory);
    }
    let mut agents_discovered: Vec<PathBuf> =
        visible.iter().filter(|item| basename_is(item, ".agents")).cloned().collect();
    agents_discovered.reverse();
    for item in agents_discovered {
        if !agents.contains(&item) {
            agents.push(item);
        }
    }

    Sources {
        global: global_enabled.then_some(global_directory),
        explicit: options.file.as_ref().map(|file| {
            if file.is_absolute() {
                file.clone()
            } else {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(file)
            }
        }),
        direct,
        project,
        claude,
        agents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("oc-core-config-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_discovery_walk_up_orders_root_most_first() {
        let dir = tempdir("disc-order");
        let root = dir.join("repo");
        let nested = root.join("a");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("opencode.json"), "{}").unwrap();
        std::fs::write(nested.join("opencode.jsonc"), "{}").unwrap();
        std::fs::create_dir_all(root.join(".opencode")).unwrap();
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let sources = discover(&nested, &globals, &Options::default());
        let repo_dot_opencode = root.join(".opencode");
        // Missing candidates at unrelated ancestors stay listed (watchable);
        // the fixture dir itself contributes its (absent) pair too.
        let direct: Vec<PathBuf> =
            sources.direct.iter().filter(|p| p.starts_with(&dir)).cloned().collect();
        assert_eq!(
            direct,
            vec![
                dir.join("opencode.json"),
                dir.join("opencode.jsonc"),
                root.join("opencode.json"),
                root.join("opencode.jsonc"),
                nested.join("opencode.json"),
                nested.join("opencode.jsonc"),
            ]
        );
        let project: Vec<&ProjectRoot> =
            sources.project.iter().filter(|root| root.path == repo_dot_opencode).collect();
        assert_eq!(project.len(), 1);
        assert_eq!(project[0].path, repo_dot_opencode);
        assert!(project[0].present);
    }

    #[test]
    fn config_discovery_missing_candidates_still_listed() {
        let dir = tempdir("disc-missing");
        std::fs::create_dir_all(&dir).unwrap();
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let sources = discover(&dir, &globals, &Options::default());
        // Candidates are listed (watchable) even when absent; scope to the
        // fixture because every existing ancestor contributes candidates.
        let direct: Vec<PathBuf> =
            sources.direct.iter().filter(|p| p.starts_with(&dir)).cloned().collect();
        assert_eq!(direct, vec![dir.join("opencode.json"), dir.join("opencode.jsonc")]);
        let project: Vec<&ProjectRoot> =
            sources.project.iter().filter(|root| root.path.starts_with(&dir)).collect();
        assert_eq!(project.len(), 1);
        assert!(!project[0].present);
    }

    #[test]
    fn config_discovery_location_equal_to_global_dir_skips_walk() {
        let dir = tempdir("disc-at-global");
        let global = dir.join("global");
        std::fs::create_dir_all(&global).unwrap();
        let globals = GlobalDirs { config: global.clone(), home: dir.join("home") };
        let sources = discover(&global, &globals, &Options::default());
        assert!(sources.direct.is_empty() && sources.project.is_empty());
        assert_eq!(sources.global, Some(global));
    }

    #[test]
    fn config_discovery_global_files_filtered_from_walk() {
        let dir = tempdir("disc-global-files");
        let global = dir.join("global");
        let project = global.join("plugins").join("demo");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(global.join("opencode.json"), "{}").unwrap();
        let globals = GlobalDirs { config: global.clone(), home: global.join("home") };
        let sources = discover(&project, &globals, &Options::default());
        // The walk reaches the global dir but the global files are filtered.
        assert!(!sources.direct.contains(&global.join("opencode.json")));
        assert_eq!(sources.global, Some(global));
    }

    #[test]
    fn config_discovery_global_disabled_filters_subtrees() {
        let dir = tempdir("disc-global-off");
        let global = dir.join("global");
        let home = global.join("home");
        let project = home.join("project");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let globals = GlobalDirs { config: global, home };
        let sources =
            discover(&project, &globals, &Options { global: Some(false), ..Options::default() });
        // Home-level ecosystem dirs are global roots however the walk reaches
        // them; unrelated missing candidates may remain listed (watchable).
        assert!(!sources.claude.contains(&globals.home.join(".claude")));
        assert!(!sources.agents.contains(&globals.home.join(".agents")));
        assert!(!sources.claude.iter().any(|dir| dir.starts_with(&globals.home)));
        assert!(!sources.agents.iter().any(|dir| dir.starts_with(&globals.home)));
        assert_eq!(sources.global, None);
    }

    #[test]
    fn config_discovery_dedupes_global_compatibility_dirs() {
        let dir = tempdir("disc-dedupe");
        let global = dir.join("global");
        let home = global.join("home");
        let project = home.join("project");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(home.join(".agents")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        let globals = GlobalDirs { config: global, home };
        let sources = discover(&project, &globals, &Options::default());
        assert_eq!(sources.claude.first(), Some(&globals.home.join(".claude")));
        assert_eq!(sources.agents.first(), Some(&globals.home.join(".agents")));
    }

    #[test]
    fn config_discovery_explicit_file_resolved() {
        let dir = tempdir("disc-explicit");
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let options = Options { file: Some(dir.join("custom.json")), ..Options::default() };
        let sources = discover(&dir, &globals, &options);
        assert_eq!(sources.explicit, Some(dir.join("custom.json")));
    }
}
