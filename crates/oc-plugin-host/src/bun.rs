//! Guest runtime resolution (spike-plugin-runtime risk table: the
//! guest binary must exist on the user machine → boot-time detection,
//! graceful degradation — the plugin subsystem disables itself with a
//! clear log, never crashes the host).
//!
//! v2 ships TWO first-class runtime tracks (packages/plugin
//! `#plugin-source` conditions: `source.bun.ts` / `source.node.ts`,
//! plus node SEA builds — build-node.ts). The Rust port mirrors that
//! with [`GuestRuntime`]: bun is preferred, node is the fallback, so
//! a bun-less box with a system node keeps the subsystem alive.
//!
//! Resolution order ([`resolve_runtime`], G-333):
//!
//! 1. `OC_PLUGIN_BUN` set → the bun track; an override pointing at a
//!    missing binary is FINAL (disable — same explicit-override
//!    semantics as `oc_util::bunman`). If BOTH overrides are set, bun
//!    wins (v2's source conditions prefer the bun build).
//! 2. `OC_PLUGIN_NODE` set → the node track; explicit-final on
//!    failure, symmetric with `OC_PLUGIN_BUN`, and it genuinely
//!    FORCES node even when a bun exists (bounded deviation from the
//!    lane's first ordering sketch: the node-mode e2e suite — and any
//!    user who wants the node distribution — needs the override to be
//!    meaningful on bunful machines).
//! 3. The bun chain (managed cache → `/usr/local/bin/bun` → `PATH` →
//!    `~/.bun/bin/bun`).
//! 4. `node` on the `PATH` (presence only, no probe subprocess —
//!    symmetric with bun resolution; a broken node surfaces as a
//!    spawn failure → disabled, exactly like a broken bun today).
//!
//! When NEITHER binary exists and plugins were requested,
//! [`crate::PluginHost::start_detached_with`] falls back to
//! `oc_util::bunman::ensure`'s pinned download (MIGRATION §2.3) —
//! the node track takes precedence over the download, so an offline
//! box with node never touches the network.

pub use oc_util::bunman::{BunSource, BUN_ENV};

/// Environment override forcing the node track (symmetric with
/// [`BUN_ENV`]; explicit-final on failure).
pub const NODE_ENV: &str = "OC_PLUGIN_NODE";

/// Which binary spawns the guest `host.js` (G-333: the runtime track).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestRuntime {
    /// Bun (preferred — the v2 default track).
    Bun(std::path::PathBuf),
    /// Node (the v2 `source.node.ts` fallback track).
    Node(std::path::PathBuf),
}

impl GuestRuntime {
    /// The binary to spawn.
    #[must_use]
    pub fn binary(&self) -> &std::path::Path {
        match self {
            Self::Bun(path) | Self::Node(path) => path,
        }
    }

    /// One-word flavor name for logs.
    #[must_use]
    pub fn flavor(&self) -> &'static str {
        match self {
            Self::Bun(_) => "bun",
            Self::Node(_) => "node",
        }
    }
}

/// Resolve the bun executable (install-tooling use; the guest track
/// uses [`resolve_runtime`]). `None` = absent.
#[must_use]
pub fn resolve_bun() -> Option<std::path::PathBuf> {
    oc_util::bunman::resolve()
}

/// Where a resolution found bun (diagnostics).
#[must_use]
pub fn locate_bun() -> Option<(std::path::PathBuf, BunSource)> {
    oc_util::bunman::locate()
}

/// Resolve the guest runtime track (the order documented at the
/// module head). `None` = neither bun nor node → the caller disables
/// the plugin subsystem (one clear log line), or bootstraps the
/// managed bun download when plugins were requested.
#[must_use]
pub fn resolve_runtime() -> Option<GuestRuntime> {
    resolve_runtime_with(oc_util::bunman::locate)
}

/// [`resolve_runtime`] with the natural bun chain injected (unit
/// tests stub it to model a bun-less machine while `/usr/local/bin/bun`
/// exists on the CI box).
#[must_use]
pub fn resolve_runtime_with(
    locate_bun: impl Fn() -> Option<(std::path::PathBuf, BunSource)>,
) -> Option<GuestRuntime> {
    // 1. An explicit bun override is final either way, and beats a
    //    node override when both are set (bun precedence).
    if std::env::var_os(BUN_ENV).is_some() {
        return locate_bun().map(|(path, _)| GuestRuntime::Bun(path));
    }
    // 2. The explicit node override — genuinely forces the node track
    //    (even when a bun exists); final on failure too.
    if let Some(node) = std::env::var_os(NODE_ENV) {
        let node = std::path::PathBuf::from(node);
        if is_executable(&node) {
            return Some(GuestRuntime::Node(node));
        }
        return None; // an explicit override that fails is final
    }
    // 3. The natural bun chain.
    if let Some((path, _)) = locate_bun() {
        return Some(GuestRuntime::Bun(path));
    }
    // 4. `node` on the PATH.
    on_path("node").map(GuestRuntime::Node)
}

/// `<name>` via the `PATH` entries (no `which` dependency).
fn on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env juggling follows the existing bun.rs test style: save,
    /// mutate, restore (nextest runs process-per-test; under
    /// `cargo test` the env tests stay small and quick).
    struct Env {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl Env {
        fn lock(vars: &[&'static str]) -> Self {
            let saved = vars.iter().map(|&name| (name, std::env::var_os(name))).collect::<Vec<_>>();
            for (name, _) in &saved {
                std::env::remove_var(name);
            }
            Self { saved }
        }

        #[allow(clippy::unused_self)] // keeps call sites reading like a harness
        fn set(&self, name: &str, value: &str) {
            std::env::set_var(name, value);
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    fn fake_bin(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::create_dir_all(dir).expect("mkdir fake bin dir");
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\necho hi\n").expect("write fake bin");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake bin");
        path
    }

    /// Resolution unit tests (G-333): the four ruling rows plus the
    /// explicit-final semantics. The bun chain is stubbed to model a
    /// bun-less machine (this box has /usr/local/bin/bun, which the
    /// natural chain would always find); PATH still selects the stub
    /// node binaries.
    #[test]
    fn runtime_resolution_prefers_bun_and_honors_overrides() {
        let env = Env::lock(&[
            BUN_ENV,
            NODE_ENV,
            "HOME",
            "XDG_CACHE_HOME",
            "OPENCODE_TEST_HOME",
            "OPENCODE_CONFIG_DIR",
            "PATH",
        ]);
        let home = tempfile::tempdir().expect("home");
        env.set("HOME", home.path().to_str().expect("path"));
        env.set("PATH", "");

        let bun_dir = tempfile::tempdir().expect("bun dir");
        let bun = fake_bin(bun_dir.path(), "bun");
        let node_dir = tempfile::tempdir().expect("node dir");
        let node = fake_bin(node_dir.path(), "node");
        let forced = tempfile::tempdir().expect("forced");
        let forced_node = fake_bin(forced.path(), "node");

        // The stub chain: honors OC_PLUGIN_BUN when set (explicit
        // executability, exactly like bunman::locate), otherwise
        // optionally reports a naturally-found bun (`natural_bun`) —
        // or a bun-less machine.
        let natural_bun = std::cell::Cell::new(false);
        let stub = || {
            if let Some(override_path) = std::env::var_os(BUN_ENV).map(std::path::PathBuf::from) {
                return is_executable(&override_path)
                    .then_some((override_path, BunSource::EnvOverride));
            }
            natural_bun.get().then(|| (bun.clone(), BunSource::Path))
        };

        // bun "present" (the chain finds it) → bun track; node on
        // PATH is never reached.
        natural_bun.set(true);
        assert_eq!(resolve_runtime_with(stub), Some(GuestRuntime::Bun(bun.clone())));

        // OC_PLUGIN_NODE genuinely FORCES the node track even when a
        // bun exists (documented deviation — the node-mode e2e suite
        // needs it on bunful machines).
        env.set(NODE_ENV, forced_node.to_str().expect("path"));
        assert_eq!(resolve_runtime_with(stub), Some(GuestRuntime::Node(forced_node.clone())));
        std::env::remove_var(NODE_ENV);

        // bun absent, node on PATH → node fallback (no probe
        // subprocess, no download — presence on PATH is enough).
        natural_bun.set(false);
        env.set("PATH", node_dir.path().to_str().expect("path"));
        assert_eq!(resolve_runtime_with(stub), Some(GuestRuntime::Node(node.clone())));

        // OC_PLUGIN_NODE forces the node track when bun is absent…
        env.set(NODE_ENV, forced_node.to_str().expect("path"));
        assert_eq!(resolve_runtime_with(stub), Some(GuestRuntime::Node(forced_node.clone())));

        // …and is explicit-final: a missing override target disables
        // rather than falling through to PATH.
        env.set(NODE_ENV, "/definitely/not/node");
        assert_eq!(
            resolve_runtime_with(stub),
            None,
            "broken node override must disable, not fall through"
        );
        std::env::remove_var(NODE_ENV);

        // OC_PLUGIN_BUN is explicit-final (and beats OC_PLUGIN_NODE
        // when both are set — the documented ruling).
        env.set(BUN_ENV, "/definitely/not/bun");
        env.set(NODE_ENV, forced_node.to_str().expect("path"));
        assert_eq!(
            resolve_runtime_with(stub),
            None,
            "broken bun override must win over a working node override (bun precedence)"
        );
        env.set(BUN_ENV, bun.to_str().expect("path"));
        assert_eq!(resolve_runtime_with(stub), Some(GuestRuntime::Bun(bun.clone())));

        // Neither binary anywhere (PATH scrubbed, no overrides) →
        // disabled; the bunman download is NOT part of resolution
        // (start_detached_with owns that fallback).
        std::env::remove_var(BUN_ENV);
        std::env::remove_var(NODE_ENV);
        let empty = tempfile::tempdir().expect("empty");
        env.set("PATH", empty.path().to_str().expect("path"));
        assert_eq!(resolve_runtime_with(stub), None);
    }
}
