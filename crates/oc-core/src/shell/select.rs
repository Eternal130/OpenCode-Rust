//! Port of `packages/core/src/shell/select.ts` — shell discovery and
//! compatibility selection.
//!
//! Unix-focused port: the win32 branches (`FSUtil.windowsPath`, Git Bash
//! discovery, `COMSPEC`) are kept behind `#[cfg(windows)]` where they compile
//! without the util layer, otherwise reduced to their semantic core.

use std::path::Path;

/// Shell metadata: denied (terminal-only) shells, login shells, PowerShell.
fn meta(file: &str) -> Option<&'static str> {
    let name = name(file);
    match name.as_str() {
        "bash" | "dash" | "ksh" | "sh" | "zsh" => Some("login"),
        "fish" => Some("deny login"),
        "nu" => Some("deny"),
        "powershell" | "pwsh" => Some("ps"),
        _ => None,
    }
}

/// `ShellSelect.name` — basename, lowercased (win32-normalized on Windows).
#[must_use]
pub fn name(file: &str) -> String {
    #[cfg(windows)]
    {
        let normalized = windows_path(file);
        let file_name = Path::new(&normalized)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        return file_name.to_lowercase();
    }
    #[cfg(not(windows))]
    {
        Path::new(file)
            .file_name()
            .map_or_else(|| file.to_string(), |n| n.to_string_lossy().to_string())
            .to_lowercase()
    }
}

#[cfg(windows)]
fn windows_path(file: &str) -> String {
    // FSUtil.windowsPath — POSIX-style absolute paths become drive paths.
    file.replace('/', "\\")
}

/// `ShellSelect.login`.
#[must_use]
pub fn login(file: &str) -> bool {
    meta(file).is_some_and(|meta| meta.contains("login"))
}

/// `ShellSelect.ps`.
#[must_use]
pub fn ps(file: &str) -> bool {
    meta(file).is_some_and(|meta| meta == "ps")
}

fn compatible(file: &str) -> bool {
    !meta(file).is_some_and(|meta| meta.contains("deny"))
}

/// `which` — resolve an executable on PATH (unix).
fn find_executable(name: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

fn stat_file(file: &str) -> Option<std::fs::Metadata> {
    std::fs::metadata(file).ok().filter(std::fs::Metadata::is_file)
}

fn rooted(file: &str) -> bool {
    #[cfg(windows)]
    {
        let normalized = windows_path(file);
        Path::new(&normalized).is_absolute()
    }
    #[cfg(not(windows))]
    {
        Path::new(file).is_absolute()
    }
}

/// `executable` — resolve to a concrete executable path.
#[must_use]
pub fn executable(file: &str) -> Option<String> {
    if rooted(file) {
        return stat_file(file).map(|_| file.to_string());
    }
    find_executable(file)
}

/// `ShellSelect.fallback`.
fn fallback() -> String {
    if cfg!(target_os = "macos") {
        return "/bin/zsh".into();
    }
    if let Some(bash) = find_executable("bash") {
        return bash;
    }
    "/bin/sh".into()
}

/// `ShellSelect.select` — resolve `file` (optionally compat-filtered).
fn select(file: Option<&str>, compatible_only: bool) -> Option<String> {
    if let Some(file) = file {
        if !compatible_only || compatible(file) {
            if let Some(shell) = executable(file) {
                return Some(shell);
            }
        }
    }
    #[cfg(windows)]
    {
        return win_first();
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn win_first() -> Option<String> {
    ["pwsh", "powershell"]
        .iter()
        .find_map(|candidate| find_executable(candidate))
        .or_else(|| std::env::var("COMSPEC").ok())
        .or(Some("cmd.exe".into()))
}

/// `/etc/shells` entries (or the classic three).
fn unix_shells() -> Vec<String> {
    match std::fs::read_to_string("/etc/shells") {
        Ok(text) => {
            let mut shells: Vec<String> = text
                .lines()
                .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .map(String::from)
                .collect();
            shells.dedup();
            if shells.is_empty() {
                vec!["/bin/bash".into(), "/bin/zsh".into(), "/bin/sh".into()]
            } else {
                shells
            }
        }
        Err(_) => vec!["/bin/bash".into(), "/bin/zsh".into(), "/bin/sh".into()],
    }
}

/// `ShellSelect.Item`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub path: String,
    pub name: String,
    pub acceptable: bool,
}

/// `ShellSelect.list`.
#[must_use]
pub fn list() -> Vec<Item> {
    #[cfg(windows)]
    let shells = win_list();
    #[cfg(not(windows))]
    let shells = unix_shells();
    shells
        .into_iter()
        .filter_map(|shell| executable(&shell))
        .map(|shell| {
            let n = name(&shell);
            Item {
                name: if executable(&n).is_some() { n.clone() } else { shell.clone() },
                path: shell.clone(),
                acceptable: compatible(&shell),
            }
        })
        .collect()
}

#[cfg(windows)]
fn win_list() -> Vec<String> {
    let mut shells: Vec<String> = Vec::new();
    for candidate in ["pwsh", "powershell"] {
        if let Some(found) = find_executable(candidate) {
            if !shells.contains(&found) {
                shells.push(found);
            }
        }
    }
    if let Some(comspec) = std::env::var("COMSPEC").ok() {
        if !shells.contains(&comspec) {
            shells.push(comspec);
        }
    }
    shells
}

/// `ShellSelect.environment` — `$SHELL` resolved afresh each call.
pub fn environment() -> String {
    select(std::env::var("SHELL").ok().as_deref(), false).unwrap_or_else(fallback)
}

/// `ShellSelect.environment` with a compatibility filter.
pub fn environment_compatible() -> String {
    select(std::env::var("SHELL").ok().as_deref(), true).unwrap_or_else(fallback)
}

/// `ShellSelect.resolve` — configured shell, else the environment shell.
/// The config/compat caches of v2 are process-local caches over
/// [`environment`]; resolve afresh each call (same observable behavior
/// except caching).
#[must_use]
pub fn resolve(config_shell: Option<&str>, priority_compat: bool) -> String {
    if let Some(config_shell) = config_shell {
        if let Some(selected) = select(Some(config_shell), priority_compat) {
            return selected;
        }
        // A configured shell that cannot be resolved falls back to the
        // environment shell (same priority).
        return if priority_compat { environment_compatible() } else { environment() };
    }
    if priority_compat {
        environment_compatible()
    } else {
        environment()
    }
}

/// `ShellSelect.args` — command args per shell family.
#[must_use]
pub fn args(file: &str, command: &str) -> Vec<String> {
    let n = name(file);
    if n == "cmd" {
        return vec!["/c".into(), command.into()];
    }
    if ps(file) {
        return vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            command.into(),
        ];
    }
    vec!["-c".into(), command.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_select_normalizes_shell_names() {
        assert_eq!(name("/bin/bash"), "bash");
        assert_eq!(name("/usr/bin/fish"), "fish");
    }

    #[test]
    fn shell_select_detects_login_shells() {
        assert!(login("/bin/bash"));
        assert!(!login("/bin/pwsh"));
        assert!(!login("C:/tools/pwsh.exe"));
    }

    #[test]
    fn shell_select_denies_terminal_only_shells() {
        assert!(!compatible("fish"));
        assert!(!compatible("nu"));
        assert!(compatible("bash"));
    }

    #[test]
    fn shell_select_falls_back_when_configured_shell_unresolvable() {
        let configured = resolve(None, false);
        let compat = resolve(None, true);
        assert_eq!(resolve(Some("opencode-missing-shell"), false), configured);
        assert_eq!(resolve(Some("opencode-missing-shell"), true), compat);
    }

    #[test]
    fn shell_select_falls_back_for_terminal_only_shells() {
        assert_ne!(name(&resolve(Some("fish"), true)), "fish");
        assert_ne!(name(&resolve(Some("nu"), true)), "nu");
    }

    #[test]
    fn shell_select_builds_command_args_per_shell_family() {
        assert_eq!(args("/bin/sh", "echo hi"), vec!["-c", "echo hi"]);
        assert_eq!(args("/usr/bin/fish", "echo hi"), vec!["-c", "echo hi"]);
        assert_eq!(args("/bin/zsh", "echo hi"), vec!["-c", "echo hi"]);
        assert_eq!(args("/bin/bash", "echo hi"), vec!["-c", "echo hi"]);
        assert_eq!(
            args("pwsh", "Write-Output hi"),
            vec!["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "Write-Output hi"]
        );
    }
}
