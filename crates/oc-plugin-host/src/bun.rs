//! Bun runtime resolution (spike-plugin-runtime risk table: "Bun must
//! exist on the user machine" → boot-time detection, graceful
//! degradation — the plugin subsystem disables itself with a clear
//! log, never crashes the host).
//!
//! The managed pinned-download decision (MIGRATION §2.3, Phase 10
//! packaging) lives in `oc_util::bunman`: resolution order
//! `OC_PLUGIN_BUN` override → `~/.cache/opencode/bin/bun` (the
//! managed pinned 1.3.10 download) → `/usr/local/bin/bun` → `PATH`
//! → `~/.bun/bin/bun`; [`crate::PluginHost::start_detached_with`]
//! bootstraps the pinned download when plugins are requested and no
//! bun exists, degrading to a clear message on failure.

pub use oc_util::bunman::{BunSource, BUN_ENV};

/// Resolve the bun executable. `None` = absent → plugin subsystem
/// disabled (one clear log line at startup, every host op no-ops).
#[must_use]
pub fn resolve_bun() -> Option<std::path::PathBuf> {
    oc_util::bunman::resolve()
}

/// Where a resolution found bun (diagnostics).
#[must_use]
pub fn locate_bun() -> Option<(std::path::PathBuf, BunSource)> {
    oc_util::bunman::locate()
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
