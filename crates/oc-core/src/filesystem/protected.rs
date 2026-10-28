//! Sensitive-directory exclusion table — port of
//! `packages/core/src/filesystem/protected.ts` (v2.0.24).
//!
//! REF semantics (mirrored exactly):
//! - `is_home`: `path.resolve(directory) === path.resolve(home)` — the
//!   scan root is the user's home directory itself (component-wise
//!   equality on lexically resolved paths; REF `path.resolve` does not
//!   touch the filesystem either).
//! - `names()`: directory *basenames* to skip when scanning the home
//!   directory (used as ripgrep `exclude` globs `NAME/**` in REF's
//!   `search.ts`). Empty on linux.
//! - `paths()`: absolute paths that should never be watched, stated, or
//!   scanned. Empty on linux.
//!
//! The table is platform-gated exactly like REF (`process.platform`):
//! darwin / win32 tables below; every other platform (linux included)
//! gets empty sets.

#![allow(clippy::must_use_candidate)]

/// REF `DARWIN_HOME`.
#[cfg(target_os = "macos")]
const DARWIN_HOME: &[&str] = &[
    "Music",
    "Pictures",
    "Movies",
    "Downloads",
    "Desktop",
    "Documents",
    "Public",
    "Applications",
    "Library",
];

/// REF `DARWIN_LIBRARY` (paths relative to `~/Library`).
#[cfg(target_os = "macos")]
const DARWIN_LIBRARY: &[&str] = &[
    "Application Support/AddressBook",
    "Calendars",
    "Mail",
    "Messages",
    "Safari",
    "Cookies",
    "Application Support/com.apple.TCC",
    "PersonalizationPortrait",
    "Metadata/CoreSpotlight",
    "Suggestions",
];

/// REF `DARWIN_ROOT`.
#[cfg(target_os = "macos")]
const DARWIN_ROOT: &[&str] =
    &["/.DocumentRevisions-V100", "/.Spotlight-V100", "/.Trashes", "/.fseventsd"];

/// REF `WIN32_HOME`.
#[cfg(target_os = "windows")]
const WIN32_HOME: &[&str] =
    &["AppData", "Downloads", "Desktop", "Documents", "Pictures", "Music", "Videos", "OneDrive"];

/// `os.homedir()`: `$HOME` on unix, `%USERPROFILE%` on win32 (Node falls
/// back to getpwuid/SYSTEMDRIVE+HOMEPATH; the env-var path covers every
/// environment this server runs in).
fn home() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let value = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let value = std::env::var_os("HOME");
    value.map(std::path::PathBuf::from)
}

/// Lexical `path.resolve` (Node): absolutize + normalize `.`/`..`
/// without touching the filesystem.
pub fn resolve_lexical(input: &std::path::Path) -> std::path::PathBuf {
    let joined = if input.is_absolute() {
        input.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(input),
            Err(_) => input.to_path_buf(),
        }
    };
    let mut out = std::path::PathBuf::new();
    for component in joined.components() {
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

/// REF `isHome(directory)`.
pub fn is_home(directory: &str) -> bool {
    match home() {
        Some(home) => resolve_lexical(std::path::Path::new(directory)) == resolve_lexical(&home),
        None => false,
    }
}

/// REF `names()`: directory basenames to skip when scanning home.
/// Empty on non-darwin/non-win32 platforms.
pub fn names() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        DARWIN_HOME
    }
    #[cfg(target_os = "windows")]
    {
        WIN32_HOME
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        &[]
    }
}

/// REF `paths()`: absolute paths that should never be watched, stated, or
/// scanned. Empty on non-darwin/non-win32 platforms.
pub fn paths() -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let Some(home) = home() else { return Vec::new() };
        let mut out: Vec<std::path::PathBuf> =
            DARWIN_HOME.iter().map(|name| home.join(name)).collect();
        out.extend(DARWIN_LIBRARY.iter().map(|name| home.join("Library").join(name)));
        out.extend(DARWIN_ROOT.iter().map(std::path::PathBuf::from));
        out
    }
    #[cfg(target_os = "windows")]
    {
        match home() {
            Some(home) => WIN32_HOME.iter().map(|name| home.join(name)).collect(),
            None => Vec::new(),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Whether `directory` (an absolute candidate being scanned) must be
/// skipped: it is under one of REF's absolute protected `paths()`, OR the
/// scan root is home and the basename is in `names()`.
pub fn is_protected(scan_root: &std::path::Path, directory: &std::path::Path) -> bool {
    let resolved = resolve_lexical(directory);
    for protected in paths() {
        if resolved == protected || resolved.starts_with(&protected) {
            return true;
        }
    }
    if is_home(&scan_root.to_string_lossy()) {
        if let Some(name) = resolved.file_name().and_then(|n| n.to_str()) {
            if names().contains(&name) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_home_matches_resolved_home() {
        let home = std::env::var("HOME").expect("HOME set in test env");
        assert!(is_home(&home));
        assert!(is_home(&format!("{home}/")));
        assert!(is_home(&format!("{home}/./x/..")));
        assert!(!is_home(&format!("{home}/sub")));
    }

    #[test]
    fn is_home_without_home_env_is_false() {
        // sanity guard for the None branch (not reachable on CI, which
        // always sets HOME/USERPROFILE, but exercises the fallback).
        if home().is_none() {
            assert!(!is_home("/anywhere"));
        }
    }

    #[test]
    fn names_table_matches_platform_gate() {
        #[cfg(target_os = "macos")]
        assert_eq!(
            names(),
            &[
                "Music",
                "Pictures",
                "Movies",
                "Downloads",
                "Desktop",
                "Documents",
                "Public",
                "Applications",
                "Library"
            ]
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            names(),
            &[
                "AppData",
                "Downloads",
                "Desktop",
                "Documents",
                "Pictures",
                "Music",
                "Videos",
                "OneDrive"
            ]
        );
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert!(names().is_empty());
    }

    #[test]
    fn paths_table_shape() {
        let paths = paths();
        #[cfg(target_os = "macos")]
        {
            assert_eq!(paths.len(), DARWIN_HOME.len() + DARWIN_LIBRARY.len() + DARWIN_ROOT.len());
            assert!(paths.contains(&std::path::PathBuf::from("/.Trashes")));
        }
        #[cfg(target_os = "windows")]
        assert_eq!(paths.len(), WIN32_HOME.len());
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert!(paths.is_empty());
    }

    #[test]
    fn is_protected_uses_basename_only_when_root_is_home() {
        let home = std::env::var("HOME").expect("HOME");
        // On linux the tables are empty, so nothing is protected by name.
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert!(!is_protected(
                std::path::Path::new(&home),
                std::path::Path::new(&format!("{home}/Desktop"))
            ));
            assert!(!is_protected(
                std::path::Path::new("/elsewhere"),
                std::path::Path::new("/elsewhere/Desktop")
            ));
        }
        // Table-bearing platforms: home-rooted Desktop is protected,
        // non-home Desktop is not.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            assert!(is_protected(
                std::path::Path::new(&home),
                std::path::Path::new(&format!("{home}/Desktop"))
            ));
            assert!(!is_protected(
                std::path::Path::new("/elsewhere"),
                std::path::Path::new("/elsewhere/Desktop")
            ));
        }
    }
}
