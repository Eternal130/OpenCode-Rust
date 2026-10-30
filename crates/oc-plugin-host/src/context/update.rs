//! The plugin UPDATE path (`POST /api/plugin/update`, RF-02) — the port
//! of v2's `PluginUpdate` service (core/plugin/update.ts) over the
//! port's own install layout (`context::install`).
//!
//! * [`UpdateCache::check`] — outdated detection with the 24 h
//!   throttle (update.ts:34-49): the per-target answer is memoized
//!   in-process for `CHECK_INTERVAL_MS`; a failing registry probe
//!   keeps the last known value (or `false`), matching the ref's
//!   `Effect.option` + `getOrElse(current ?? false)`. The ref caches
//!   in a service-local `Map` (NOT persisted) — the port mirrors that
//!   exactly: in-memory only, per process.
//! * [`update`] — the update flow (update.ts:52-68 + npm.ts:386-398):
//!   a pinned spec is immutable, so it degrades to a plain (idempotent)
//!   install; an unpinned spec resolves the registry latest, installs
//!   it as a NEW timestamped generation (reusing an equal installed
//!   generation — the ref's same-revision dedupe), clears the cached
//!   check for the target, then garbage-collects stale generations.
//! * [`collect_stale`] — generation GC (npm.ts:323-342): per
//!   `<pkg>@<version>` cache key the newest `KEEP_GENERATIONS` (2)
//!   timestamped generations always survive, older ones are dropped
//!   once past the 7-day retention, and `.staging-*` leftovers older
//!   than 1 h are swept. Removal failures are logged, never fatal.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use super::install::{self, InstallOptions, Installed};
use crate::resolve::resolve_installed_package;

/// The outdated-check throttle (update.ts:9: 24 h).
pub const CHECK_INTERVAL_MS: u128 = 24 * 60 * 60 * 1_000;
/// Completed-generation retention (npm.ts:131: 7 d).
pub const RETENTION_MS: u128 = 7 * 24 * 60 * 60 * 1_000;
/// Staging-directory retention (npm.ts:132: 1 h).
pub const STAGING_RETENTION_MS: u128 = 60 * 60 * 1_000;
/// Always-kept newest generations per cache key (npm.ts:326:
/// `completed.slice(-2)`).
pub const KEEP_GENERATIONS: usize = 2;

/// The per-process check cache (the ref's service-local `Map` —
/// in-memory by design; a restart simply re-probes).
#[derive(Default)]
pub struct UpdateCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
}

#[derive(Clone, Copy)]
struct CacheEntry {
    outdated: bool,
    checked_at_ms: u128,
}

impl UpdateCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The memoized outdated answer for `target`, probing the registry
    /// at most once per [`CHECK_INTERVAL_MS`] unless `refresh`.
    pub fn check(
        &self,
        target: &str,
        root: &Path,
        options: &InstallOptions,
        refresh: bool,
    ) -> bool {
        let now = now_ms();
        self.check_at(target, root, options, refresh, now)
    }

    /// [`check`] with an injected clock (tests).
    pub fn check_at(
        &self,
        target: &str,
        root: &Path,
        options: &InstallOptions,
        refresh: bool,
        now: u128,
    ) -> bool {
        let current = self.entries.lock().expect("update cache").get(target).copied();
        if !refresh {
            if let Some(entry) = current {
                if now.saturating_sub(entry.checked_at_ms) < CHECK_INTERVAL_MS {
                    return entry.outdated;
                }
            }
        }
        let probed = check_outdated(target, root, options);
        let value = match probed {
            Ok(outdated) => outdated,
            Err(cause) => {
                // update.ts:41 — a failed probe logs a warning and keeps
                // the last known answer (false when never checked).
                eprintln!("opencode plugin: failed to check plugin update for {target}: {cause}");
                current.map_or(false, |entry| entry.outdated)
            }
        };
        self.entries
            .lock()
            .expect("update cache")
            .insert(target.to_owned(), CacheEntry { outdated: value, checked_at_ms: now });
        value
    }

    /// Drop the cached answer after a successful update
    /// (update.ts:58 `status.delete(target)`).
    pub fn clear(&self, target: &str) {
        self.entries.lock().expect("update cache").remove(target);
    }
}

/// The process-global cache (the ref's global `PluginUpdate` service
/// node — one per process, shared by every location's handler).
pub fn global_cache() -> &'static UpdateCache {
    static CACHE: OnceLock<UpdateCache> = OnceLock::new();
    CACHE.get_or_init(UpdateCache::new)
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis())
}

/// The actual installed version of `package` under `root` (read from
/// the installed tree's `package.json` — the `@latest` cache key does
/// not carry the real version).
pub fn installed_version(root: &Path, package: &str) -> Option<String> {
    let installed = resolve_installed_package(root, &package)?;
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(installed.join("package.json")).ok()?,
    )
    .ok()?;
    manifest.get("version").and_then(|value| value.as_str()).map(str::to_owned)
}

/// Outdated = the installed version differs from the registry latest
/// (npm.ts:360-384 for registry specs: `installed !== available`).
fn check_outdated(target: &str, root: &Path, options: &InstallOptions) -> Result<bool, String> {
    let (package, requested) = split_spec(target);
    // A pinned spec is immutable (npm.ts:368 `!target.mutable → false`).
    if requested.is_some() {
        return Ok(false);
    }
    let Some(installed) = installed_version(root, &package) else {
        return Err(format!("package is not installed: {target}"));
    };
    let available = registry_latest(&package, options)?;
    Ok(installed != available)
}

/// `npm view <pkg> version` (the registry probe).
fn registry_latest(package: &str, options: &InstallOptions) -> Result<String, String> {
    install::pin_latest(package, options)
}

/// The update flow (update.ts:52-68 → npm.ts update). Blocking — the
/// HTTP handler runs it on the blocking pool.
pub fn update(
    target: &str,
    root: &Path,
    cache: &UpdateCache,
    options: &InstallOptions,
) -> Result<Installed, String> {
    let target = target.trim();
    let (package, requested) = split_spec(target);
    // npm.ts:394 — an immutable (pinned) spec just re-adds (idempotent:
    // the reuse path returns the existing generation).
    let installed = if requested.is_some() {
        install::install(target, &InstallOptions { root: Some(root.to_owned()), ..options.clone() })?
    } else {
        let latest = registry_latest(&package, options)?;
        let spec = format!("{package}@{latest}");
        install::install(&spec, &InstallOptions {
            root: Some(root.to_owned()),
            ..options.clone()
        })?
    };
    // update.ts:58 — the cached check is stale the moment the tree changed.
    cache.clear(target);
    // npm.ts:396 — sweep old generations after every update.    collect_stale(root);
    Ok(installed)
}

/// Generation GC (npm.ts:323-342) over every `<pkg>@<version>` cache
/// key under `root`. Best-effort: removal failures are logged and
/// skipped, never propagated.
pub fn collect_stale(root: &Path) {
    let now = now_ms();
    let Ok(keys) = std::fs::read_dir(root) else { return };
    for key in keys.filter_map(std::result::Result::ok).map(|entry| entry.path()) {
        if !key.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&key) else { continue };
        let mut generations: Vec<(u128, String)> = Vec::new();
        let mut staging: Vec<(u128, String)> = Vec::new();
        for entry in entries.filter_map(std::result::Result::ok) {
            let Some(name) = entry.file_name().into_string().ok() else { continue };
            if let Some(stamp) = name.strip_prefix(".staging-") {
                // `.staging-<startedAt>-<uuid>` — the ref's interrupted
                // install leftovers (1 h retention).
                let Some(started) = stamp.split('-').next().and_then(|digits| digits.parse::<u128>().ok())
                else {
                    continue;
                };
                staging.push((started, name));
            } else if name.chars().all(|c| c.is_ascii_digit()) {
                generations.push((name.parse::<u128>().unwrap_or(0), name));
            } else {
                // Unknown junk (e.g. a stray file) — leave it alone.
                continue;
            }
        }
        generations.sort_unstable();
        let mut keep = 0;
        let total = generations.len();
        for (stamp, name) in generations {
            keep += 1;
            // The newest KEEP_GENERATIONS always survive; the rest go
            // once past the retention window.
            if total - keep < KEEP_GENERATIONS {
                break;
            }
            if now.saturating_sub(stamp) <= RETENTION_MS {
                continue;
            }
            remove_logged(&key.join(&name), &name);
        }
        for (started, name) in staging {
            if now.saturating_sub(started) > STAGING_RETENTION_MS {
                remove_logged(&key.join(&name), &name);
            }
        }
    }
}

fn remove_logged(dir: &Path, name: &str) {
    if let Err(cause) = std::fs::remove_dir_all(dir) {
        // npm.ts:337 — log and continue.
        eprintln!("opencode plugin: failed to remove stale generation {}: {cause}", name);
    }
}

/// `pkg` | `pkg@version` → (package, Some(version)?) — the same split
/// the install path uses.
fn split_spec(spec: &str) -> (String, Option<String>) {
    match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => {
            (name.to_owned(), Some(version.to_owned()))
        }
        _ => (spec.to_owned(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn stage(root: &Path, package: &str, version: &str, stamp: &str) -> PathBuf {
        let pkg = root.join(format!("{package}@{version}")).join(stamp).join("node_modules").join(package);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            json!({ "name": package, "version": version, "main": "plugin.js" }).to_string(),
        )
        .unwrap();
        std::fs::write(pkg.join("plugin.js"), "export default {}").unwrap();
        pkg
    }

    fn tmp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("ocph-update-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn throttle_reuses_the_cached_answer_within_24h() {
        let root = tmp_root("throttle");
        stage(&root, "demo-plugin", "1.0.0", "1000");
        let options = InstallOptions {
            npm: Some(PathBuf::from("/nonexistent/npm-rf02")),
            ..InstallOptions::default()
        };
        let cache = UpdateCache::new();
        // First probe fails (no npm): never-checked → false (ref
        // getOrElse(current ?? false)).
        assert!(!cache.check_at("demo-plugin", &root, &options, false, 5_000));
        // Still throttled at t+1h: the cached false comes back without
        // a probe even though the answer stayed false.
        assert!(!cache.check_at("demo-plugin", &root, &options, false, 5_000 + 60 * 60 * 1_000));
        // refresh=true bypasses the throttle (handler's check passes it).
        assert!(!cache.check_at("demo-plugin", &root, &options, true, 5_000 + 1_000));
        // Past 24 h the cache expires and re-probes.
        assert!(!cache.check_at(
            "demo-plugin",
            &root,
            &options,
            false,
            5_000 + CHECK_INTERVAL_MS + 1
        ));
    }

    #[test]
    fn check_reports_outdated_against_the_registry() {
        let root = tmp_root("outdated");
        stage(&root, "demo-plugin", "1.0.0", "1000");
        let shim = stage_npm_shim("2.0.0");
        let options = InstallOptions {
            npm: Some(shim),
            root: Some(root.clone()),
            ..InstallOptions::default()
        };
        let cache = UpdateCache::new();
        assert!(cache.check("demo-plugin", &root, &options, true));
        // A pinned spec is immutable — never outdated.
        assert!(!cache.check("demo-plugin@1.0.0", &root, &options, true));
        // Not installed → probe failure → false (never fatal).
        assert!(!cache.check("other-plugin", &root, &options, true));
    }

    /// A minimal fake `npm` shim: `view <pkg> version` echoes the
    /// version written to `version.txt` next to the shim.
    fn stage_npm_shim(version: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ocph-npm-shim-{}-{}", std::process::id(), version));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("version.txt"), version).unwrap();
        let shim = dir.join("npm");
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\n[ \"$1\" = \"view\" ] && cat \"{}/version.txt\" && exit 0\nexit 1\n",
                dir.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        shim
    }

    #[test]
    fn collect_stale_keeps_two_newest_and_sweeps_old() {
        let root = tmp_root("gc");
        let now = now_ms();
        let day = 24 * 60 * 60 * 1_000u128;
        // Five generations: two recent, three older than the 7-day
        // retention.
        stage(&root, "demo-plugin", "1.0.0", &(now - day).to_string());
        stage(&root, "demo-plugin", "1.0.0", &(now - 2 * day).to_string());
        stage(&root, "demo-plugin", "1.0.0", &(now - 8 * day).to_string());
        stage(&root, "demo-plugin", "1.0.0", &(now - 9 * day).to_string());
        stage(&root, "demo-plugin", "1.0.0", &(now - 10 * day).to_string());
        // An interrupted staging dir (2 h old) and a fresh one.
        std::fs::create_dir_all(root.join("demo-plugin@1.0.0").join(format!(".staging-{}-deadbeef", now - 2 * 60 * 60 * 1_000))).unwrap();
        std::fs::create_dir_all(root.join("demo-plugin@1.0.0").join(format!(".staging-{now}-live"))).unwrap();

        collect_stale(&root);

        let key = root.join("demo-plugin@1.0.0");
        let names: Vec<String> = std::fs::read_dir(&key)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name().into_string().unwrap())
            .collect();
        assert!(!names.contains(&(now - 8 * day).to_string()), "stale gen removed: {names:?}");
        assert!(!names.contains(&(now - 9 * day).to_string()), "stale gen removed: {names:?}");
        assert!(!names.contains(&(now - 10 * day).to_string()), "stale gen removed: {names:?}");
        assert!(names.contains(&(now - day).to_string()), "newest kept: {names:?}");
        assert!(names.contains(&(now - 2 * day).to_string()), "second-newest kept: {names:?}");
        assert!(!names.iter().any(|name| name.starts_with(".staging-") && name.contains("deadbeef")));
        assert!(names.iter().any(|name| name.starts_with(".staging-") && name.contains("live")));
    }

    #[test]
    fn collect_stale_never_drops_below_the_keep_floor() {
        let root = tmp_root("gc-floor");
        let now = now_ms();
        // Three old generations: only the newest two survive even
        // though all three are past retention? No — keep-2 always
        // protects the newest two; only the oldest goes.
        let day = 24 * 60 * 60 * 1_000u128;
        stage(&root, "demo-plugin", "1.0.0", &(now - 8 * day).to_string());
        stage(&root, "demo-plugin", "1.0.0", &(now - 9 * day).to_string());
        stage(&root, "demo-plugin", "1.0.0", &(now - 10 * day).to_string());
        collect_stale(&root);
        let key = root.join("demo-plugin@1.0.0");
        let count =
            std::fs::read_dir(&key).unwrap().filter_map(|entry| entry.ok()).count();
        assert_eq!(count, 2, "keep-2 floor");
    }

    #[test]
    fn update_of_a_pinned_spec_reuses_the_existing_generation() {
        let root = tmp_root("pinned");
        stage(&root, "demo-plugin", "1.0.0", "1000");
        let cache = UpdateCache::new();
        let options = InstallOptions { root: Some(root.clone()), ..InstallOptions::default() };
        let installed = update("demo-plugin@1.0.0", &root, &cache, &options)
            .expect("pinned update reuses the install");
        assert!(installed.reused);
        assert_eq!(installed.version, "1.0.0");
    }
}
