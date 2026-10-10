//! Bun runtime resolution (spike-plugin-runtime risk table: "Bun must
//! exist on the user machine" → boot-time detection, graceful
//! degradation — the plugin subsystem disables itself with a clear
//! log, never crashes the host).
//!
//! Search order: `OC_PLUGIN_BUN` override (tests), then `bun` on the
//! `PATH`, then the system install (`/usr/local/bin/bun`), then the
//! user install (`~/.bun/bin/bun`). Any executable found wins; none
//! found → [`None`] and the caller stays disabled.

use std::path::PathBuf;

/// Environment override for the bun binary (test hook).
pub const BUN_ENV: &str = "OC_PLUGIN_BUN";

/// Resolve the bun executable. `None` = absent → plugin subsystem
/// disabled (one clear log line at startup, every host op no-ops).
#[must_use]
pub fn resolve_bun() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var(BUN_ENV) {
        let path = PathBuf::from(override_path);
        if is_executable(&path) {
            return Some(path);
        }
        eprintln!(
            "opencode plugin-host: {BUN_ENV}={} is not executable; plugin subsystem disabled",
            path.display()
        );
        return None;
    }
    if let Some(path) = on_path() {
        return Some(path);
    }
    [PathBuf::from("/usr/local/bin/bun"), home_bun()]
        .into_iter()
        .find(|candidate| is_executable(candidate))
}

/// `bun` via the `PATH` entries (no `which` dependency).
fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("bun"))
        .find(|candidate| is_executable(candidate))
}

fn home_bun() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from);
    home.join(".bun").join("bin").join("bun")
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, sequential: the env override path and the natural
    /// resolution must not race each other on the process env (the
    /// gate runs nextest process-per-test, but stay safe under
    /// `cargo test` too).
    #[test]
    fn resolves_the_system_bun_or_none() {
        let saved = std::env::var(BUN_ENV).ok();
        std::env::set_var(BUN_ENV, "/definitely/not/bun");
        assert!(resolve_bun().is_none(), "override to a missing binary must disable");
        std::env::remove_var(BUN_ENV);
        let resolved = resolve_bun();
        if let Some(saved) = saved {
            std::env::set_var(BUN_ENV, saved);
        }
        if std::path::Path::new("/usr/local/bin/bun").exists() {
            assert!(resolved.is_some(), "system bun present but not resolved");
        }
    }
}
