//! AGENTS.md instruction discovery — port of `config/plugin/instruction.ts`.
//!
//! Global `Global.config/AGENTS.md` plus the AGENTS.md walk-up from the
//! location directory, stopping at the project root (or `$HOME` when the
//! location sits under home). Discovery is all-or-nothing: any *unwalkable*
//! discovered file marks the whole state unavailable.

use std::path::{Path, PathBuf};

use crate::config::resolve_path;

/// One discovered instruction file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFile {
    pub path: PathBuf,
    pub content: String,
}

/// All-or-nothing discovery state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionState {
    Available(Vec<InstructionFile>),
    Unavailable,
}

impl InstructionState {
    #[must_use]
    pub fn files(&self) -> Option<&[InstructionFile]> {
        match self {
            InstructionState::Available(files) => Some(files),
            InstructionState::Unavailable => None,
        }
    }
}

/// Scope flags (`InstructionDiscovery.Options`): `project: false` skips the
/// walk-up, `global: false` skips the global file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiscoveryFlags {
    pub project: bool,
    pub global: bool,
}

impl DiscoveryFlags {
    #[must_use]
    pub fn all() -> Self {
        Self { project: true, global: true }
    }
}

/// Inputs to [`discover`]; `project_root` is the resolved project directory
/// the walk stops at (`location.project.directory`).
#[derive(Debug, Clone)]
pub struct Scope<'a> {
    /// Location directory (client spelling).
    pub start: &'a Path,
    pub home: &'a Path,
    pub project_root: Option<&'a Path>,
    pub global_file: &'a Path,
}

/// Discovers instruction files. The global file comes first, then walked
/// files nearest-to-farthest (matching v2 render order).
#[must_use]
pub fn discover(scope: &Scope, flags: DiscoveryFlags) -> InstructionState {
    if !flags.project && !flags.global {
        return InstructionState::Available(Vec::new());
    }
    let start = resolve_path(scope.start);
    let home = resolve_path(scope.home);
    let global_file = resolve_path(scope.global_file);

    let project = flags.project
        && scope.project_root.is_some_and(|root| contains(&resolve_path(root), &start));
    // The walk stops at home when inside it (so home-level AGENTS.md isn't
    // re-read as a project file), else at the project root.
    let stop =
        if contains(&home, &start) { Some(home) } else { scope.project_root.map(resolve_path) };

    let mut candidates: Vec<PathBuf> = Vec::new();
    if flags.global {
        candidates.push(global_file.clone());
    }
    if project {
        for directory in ancestor_directories(&start, stop.as_deref()) {
            let file = directory.join("AGENTS.md");
            if flags.global || file != global_file {
                candidates.push(file);
            }
        }
    }

    let mut files: Vec<InstructionFile> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        let resolved = resolve_path(&candidate);
        if seen.contains(&resolved) {
            continue;
        }
        seen.push(resolved);
        if !candidate.is_file() {
            continue;
        }
        match std::fs::read_to_string(&candidate) {
            Ok(content) => files.push(InstructionFile { path: candidate, content }),
            Err(_) => return InstructionState::Unavailable,
        }
    }
    InstructionState::Available(files)
}

fn contains(parent: &Path, child: &Path) -> bool {
    // Inclusive, matching FSUtil.contains: the start itself counts as inside
    // the project root / home.
    child.starts_with(parent)
}

/// Ancestors of `start` down to (and including) `stop`, nearest first.
fn ancestor_directories(start: &Path, stop: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut current = Some(start.to_path_buf());
    while let Some(directory) = current {
        out.push(directory.clone());
        if let Some(stop) = stop {
            let stop = resolve_path(stop);
            if directory == stop {
                break;
            }
        }
        current = directory.parent().map(Path::to_path_buf);
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)] // deliberate empty-state assertions
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

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn config_instruction_discovers_global_then_nearest_to_farthest() {
        let dir = tempdir("instr-order");
        let global = dir.join("global");
        let project = dir.join("project");
        write(&global.join("AGENTS.md"), "global rules");
        write(&project.join("AGENTS.md"), "project rules");
        write(&project.join("sub").join("AGENTS.md"), "sub rules");
        let scope = Scope {
            start: &project.join("sub"),
            home: &dir.join("home"),
            project_root: Some(&project),
            global_file: &global.join("AGENTS.md"),
        };
        let state = discover(&scope, DiscoveryFlags::all());
        let paths: Vec<&str> =
            state.files().unwrap().iter().map(|file| file.content.as_str()).collect();
        assert_eq!(paths, vec!["global rules", "sub rules", "project rules"]);
    }

    #[test]
    fn config_instruction_walk_stops_at_project_root() {
        let dir = tempdir("instr-stop");
        let project = dir.join("project");
        write(&dir.join("AGENTS.md"), "outside");
        write(&project.join("AGENTS.md"), "inside");
        write(&project.join("a").join("AGENTS.md"), "deep");
        let scope = Scope {
            start: &project.join("a"),
            home: &dir.join("home"),
            project_root: Some(&project),
            global_file: &dir.join("global").join("AGENTS.md"),
        };
        let state = discover(&scope, DiscoveryFlags { project: true, global: false });
        let contents: Vec<&str> =
            state.files().unwrap().iter().map(|f| f.content.as_str()).collect();
        assert_eq!(contents, vec!["deep", "inside"]);
    }

    #[test]
    fn config_instruction_walk_stops_at_home_inside_home() {
        let dir = tempdir("instr-home");
        let global = dir.join("global");
        let home = global.join("home");
        write(&global.join("AGENTS.md"), "global rules");
        write(&home.join("AGENTS.md"), "home rules");
        write(&home.join("project").join("AGENTS.md"), "project rules");
        let project = home.join("project");
        let scope = Scope {
            start: &project,
            home: &home,
            project_root: Some(&project),
            global_file: &global.join("AGENTS.md"),
        };
        let state = discover(&scope, DiscoveryFlags::all());
        let contents: Vec<&str> =
            state.files().unwrap().iter().map(|f| f.content.as_str()).collect();
        assert_eq!(contents, vec!["global rules", "project rules", "home rules"]);
    }

    #[test]
    fn config_instruction_flags_gate_scopes() {
        let dir = tempdir("instr-flags");
        let global = dir.join("global");
        let project = dir.join("project");
        write(&global.join("AGENTS.md"), "global");
        write(&project.join("AGENTS.md"), "project");
        let scope = Scope {
            start: &project,
            home: &dir.join("home"),
            project_root: Some(&project),
            global_file: &global.join("AGENTS.md"),
        };
        let none = discover(&scope, DiscoveryFlags { project: false, global: false });
        assert_eq!(none.files().map(<[InstructionFile]>::len), Some(0));
        let global_only = discover(&scope, DiscoveryFlags { project: false, global: true });
        assert_eq!(global_only.files().unwrap()[0].content, "global");
        let project_only = discover(&scope, DiscoveryFlags { project: true, global: false });
        assert_eq!(project_only.files().unwrap()[0].content, "project");
    }

    #[test]
    fn config_instruction_start_outside_project_root_skips_walk() {
        let dir = tempdir("instr-outside");
        let project = dir.join("project");
        write(&project.join("AGENTS.md"), "inside");
        let scope = Scope {
            start: &dir, // outside the project root
            home: &dir.join("home"),
            project_root: Some(&project),
            global_file: &dir.join("global").join("AGENTS.md"),
        };
        let state = discover(&scope, DiscoveryFlags { project: true, global: false });
        assert!(state.files().unwrap().is_empty());
    }

    #[test]
    fn config_instruction_unreadable_walked_file_makes_all_unavailable() {
        let dir = tempdir("instr-unreadable");
        let project = dir.join("project");
        write(&project.join("AGENTS.md"), "inside");
        let unreadable = project.join("sub").join("AGENTS.md");
        write(&unreadable, "locked");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        }
        let scope = Scope {
            start: &project.join("sub"),
            home: &dir.join("home"),
            project_root: Some(&project),
            global_file: &dir.join("global").join("AGENTS.md"),
        };
        let state = discover(&scope, DiscoveryFlags { project: true, global: false });
        // Root can read 0o000 files, so the all-or-nothing downgrade only
        // applies when the permission actually blocks the read.
        let read_blocked = std::fs::read_to_string(&unreadable).is_err();
        if read_blocked {
            assert_eq!(state, InstructionState::Unavailable);
        } else {
            assert!(state.files().is_some());
        }
    }

    #[test]
    fn config_instruction_missing_files_are_skipped_not_fatal() {
        let dir = tempdir("instr-missing");
        let project = dir.join("project");
        fs::create_dir_all(project.join("a")).unwrap();
        write(&project.join("a").join("AGENTS.md"), "present");
        let scope = Scope {
            start: &project.join("a"),
            home: &dir.join("home"),
            project_root: Some(&project),
            global_file: &dir.join("global").join("AGENTS.md"), // absent
        };
        let state = discover(&scope, DiscoveryFlags::all());
        let contents: Vec<&str> =
            state.files().unwrap().iter().map(|f| f.content.as_str()).collect();
        assert_eq!(contents, vec!["present"]);
    }
}
