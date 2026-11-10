//! The pinned, frozen plugin install path (`opencode plugin add`).
//!
//! Installs npm package plugins into the app-isolated cache layout
//! `<cache>/opencode-rust/npm/<pkg>@<version>/<timestamp>/node_modules/<pkg>`
//! (the v2-compatible shape under OUR `opencode-rust` XDG suffix — the
//! one-way v2 migration copies an upstream `opencode` cache across on
//! first launch), version pinned, lockfile-frozen when one exists:
//!
//! * `bun install --frozen-lockfile` when `bun.lock` is present,
//! * `npm ci` when `package-lock.json` is present,
//! * otherwise `bun install` (preferred — it writes `bun.lock`) or
//!   `npm install --no-save` as the fallback.
//!
//! Entry resolution probes the package `exports` the way the v2 host
//! does (`host.ts`): `./server` first (the v2 server plugin), then
//! `.`, then `main`/`index.js`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

/// Install tuning.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Cache root (default [`default_root`]: the opencode-rust npm cache).
    pub root: Option<PathBuf>,
    /// Pass `--offline` to the installer (hermetic re-installs).
    pub offline: bool,
    /// Reuse an existing `<pkg>@<version>` installation instead of
    /// touching the network (the bring-up test's pre-staged tree).
    pub allow_existing: bool,
    /// Hard timeout for the install command.
    pub timeout: Duration,
    /// Explicit npm binary (tests / the update path); `None` = [`find_npm`].
    pub npm: Option<PathBuf>,
    /// Explicit bun binary (tests); `None` = [`crate::bun::resolve_bun`].
    pub bun: Option<PathBuf>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            root: None,
            offline: false,
            allow_existing: true,
            timeout: Duration::from_secs(300),
            npm: None,
            bun: None,
        }
    }
}

/// One completed installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The timestamped install directory (the one containing
    /// `node_modules`).
    pub root: PathBuf,
    pub package: String,
    pub version: String,
    /// The resolved plugin entrypoint (`.js` file to load).
    pub entry: PathBuf,
    /// `true` when an existing installation was reused.
    pub reused: bool,
}

/// The default cache root: `$XDG_CACHE_HOME|$HOME/.cache` +
/// `opencode-rust/npm` (the v2 `Global` data layout under the port's
/// isolated XDG suffix — see `oc_util::global_roots::APP_NAME`).
#[must_use]
pub fn default_root() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join(oc_util::global_roots::APP_NAME).join("npm")
}

/// Split `pkg` / `pkg@version`; a missing version is pinned via
/// `npm view <pkg> version` (network) unless `allow_existing` finds an
/// installation to reuse.
pub fn install(spec: &str, options: &InstallOptions) -> Result<Installed, String> {
    let spec = spec.trim();
    if spec.is_empty() || spec.starts_with('-') {
        return Err(format!("invalid plugin spec {spec:?}"));
    }
    let (package, requested) = split_spec(spec);
    let root = options.root.clone().unwrap_or_else(default_root);

    // Reuse path (hermetic re-installs / `plugin add` idempotence).
    if options.allow_existing {
        if let Some(version) = requested.as_ref() {
            if let Some(installed) = latest_install(&root, &package, version) {
                return Ok(installed);
            }
        }
    }

    let version = match requested {
        Some(version) => version,
        None => pin_latest(&package, options)?,
    };

    // A concurrent install may have landed the version while we pinned.
    if options.allow_existing {
        if let Some(installed) = latest_install(&root, &package, &version) {
            return Ok(installed);
        }
    }

    let mut dir = install_dir(&root, &package, &version);
    // G-365 — atomicity: the install runs in a STAGING directory
    // (`.staging-<startedAt>-<pid>-<seq>` — the layout the update
    // path's GC already assumes: `.staging-<startedAt>-…` leftovers
    // sweep after 1 h and never count as generations) and only a
    // fully verified install is RENAMED into the final timestamped
    // location. A partial failure leaves at most a `.staging-*`
    // residue `latest_install` ignores (non-digit names) and GC can
    // sweep — never a reusable half-installed generation. The
    // `<pid>-<seq>` suffix keeps two concurrent installs from
    // trampling each other's staging tree; the rename retries on a
    // stamp collision (same-millisecond installs).
    let mut last_error = None;
    for _ in 0..3 {
        let staging = staging_dir(&root, &package, &version);
        let outcome = write_install_manifest(&staging, &package, &version)
            .and_then(|()| run_installer(&staging, options))
            .and_then(|()| verify_and_resolve(&staging, &package, version.clone()));
        match outcome {
            Ok(mut installed) => {
                match std::fs::rename(&staging, &dir) {
                    Ok(()) => {
                        // The verified entry pointed into staging; the
                        // rename moved the tree — rebase it onto the
                        // final location.
                        if let Ok(relative) = installed.entry.strip_prefix(&staging) {
                            installed.entry = dir.join(relative);
                        }
                        installed.root = dir;
                        return Ok(installed);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        // Same-millisecond stamp collision: pick a
                        // fresh stamp and rename again.
                        let _ = std::fs::remove_dir_all(&staging);
                        dir = install_dir(&root, &package, &version);
                        last_error = Some(format!("install stamp collision: {error}"));
                    }
                    Err(error) => {
                        let _ = std::fs::remove_dir_all(&staging);
                        return Err(format!("finalize install {}: {error}", dir.display()));
                    }
                }
            }
            Err(error) => {
                // A failed install leaves NO reusable partial state:
                // best-effort sweep of the staging tree (GC catches
                // what survives).
                let _ = std::fs::remove_dir_all(&staging);
                return Err(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "install could not finalize a generation".to_owned()))
}

/// `pkg` | `pkg@version` → (package, Some(version)?).
fn split_spec(spec: &str) -> (String, Option<String>) {
    match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => {
            (name.to_owned(), Some(version.to_owned()))
        }
        _ => (spec.to_owned(), None),
    }
}

/// `<root>/<pkg>@<version>/<timestamp-ms>/` (the v2 cache layout).
fn install_dir(root: &Path, package: &str, version: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    root.join(format!("{package}@{version}")).join(stamp.to_string())
}

/// G-365 — the staging directory an install runs in before its
/// atomic rename: `.staging-<startedAt>-<pid>-<seq>` under the same
/// `<pkg>@<version>` key. The prefix is the layout the GC already
/// sweeps (`.staging-<startedAt>-…`, 1 h retention, never a
/// generation); the pid+seq suffix de-conflicts concurrent installs.
fn staging_dir(root: &Path, package: &str, version: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    root.join(format!("{package}@{version}"))
        .join(format!(".staging-{started}-{}-{seq}", std::process::id()))
}

fn write_install_manifest(dir: &Path, package: &str, version: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir.join("node_modules"))
        .map_err(|error| format!("create {}: {error}", dir.display()))?;
    let manifest = serde_json::json!({
        "name": "opencode-plugin-install",
        "private": true,
        "dependencies": { package: version },
    });
    std::fs::write(
        dir.join("package.json"),
        serde_json::to_string_pretty(&manifest).expect("manifest serializes"),
    )
    .map_err(|error| format!("write manifest: {error}"))
}

/// Pick the runner + freeze policy: `bun install --frozen-lockfile`
/// over `bun.lock`, `npm ci` over `package-lock.json`, else
/// `bun install` (preferred) / `npm install --no-save`.
///
/// A missing bun triggers the managed pinned bootstrap once
/// (`opencode plugin add` is a plugins-requested path) before the npm
/// fallback — a failed bootstrap degrades to npm with a log.
fn run_installer(dir: &Path, options: &InstallOptions) -> Result<(), String> {
    let mut bun = options.bun.clone().or_else(crate::bun::resolve_bun);
    if bun.is_none() {
        match oc_util::bunman::ensure() {
            Ok(path) => {
                eprintln!(
                    "opencode: no system bun; using the managed pinned bun for plugin install"
                );
                bun = Some(path);
            }
            Err(error) => {
                eprintln!("opencode: bun bootstrap unavailable ({error}); falling back to npm");
            }
        }
    }
    let (program, args): (PathBuf, Vec<String>) = if dir.join("bun.lock").exists()
        || dir.join("bun.lockb").exists()
    {
        let bun = bun.ok_or("bun.lock present but bun is unavailable")?;
        installer_args(bun, &["install", "--frozen-lockfile"], options.offline)
    } else if dir.join("package-lock.json").exists() {
        let npm = npm_for(options).ok_or("package-lock.json present but npm is unavailable")?;
        installer_args(npm, &["ci"], options.offline)
    } else if let Some(bun) = bun {
        installer_args(bun, &["install"], options.offline)
    } else {
        let npm = npm_for(options).ok_or("neither bun nor npm is available to install plugins")?;
        installer_args(npm, &["install", "--no-save", "--no-audit", "--no-fund"], options.offline)
    };

    let output = Command::new(&program)
        .args(&args)
        .current_dir(dir)
        .env("NPM_CONFIG_FUND", "false")
        .env("NPM_CONFIG_AUDIT", "false")
        .output()
        .map_err(|error| format!("run {} {:?}: {error}", program.display(), args))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String =
            stderr.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!(
            "{} {:?} failed ({}):\n{tail}",
            program.display(),
            args,
            output.status
        ));
    }
    Ok(())
}

fn installer_args(program: PathBuf, base: &[&str], offline: bool) -> (PathBuf, Vec<String>) {
    let mut args: Vec<String> = base.iter().map(|arg| (*arg).to_owned()).collect();
    if offline {
        args.push("--offline".into());
    }
    (program, args)
}

fn verify_and_resolve(dir: &Path, package: &str, version: String) -> Result<Installed, String> {
    let package_root = dir.join("node_modules").join(package);
    if !package_root.join("package.json").exists() {
        return Err(format!("install succeeded but {} is missing", package_root.display()));
    }
    let entry = resolve_entry(&package_root)?;
    Ok(Installed {
        root: dir.to_owned(),
        package: package.to_owned(),
        version,
        entry,
        reused: false,
    })
}

/// The newest existing installation of `pkg@version` under `root`, if
/// any (timestamp-named directories, newest wins).
#[must_use]
pub fn latest_install(root: &Path, package: &str, version: &str) -> Option<Installed> {
    let base = root.join(format!("{package}@{version}"));
    let mut stamps: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        // G-365 — staging residue (`.staging-*`) and any other
        // non-generation name never counts as an install.
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| name.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|entry| entry.path())
        .collect();
    stamps.sort();
    for dir in stamps.into_iter().rev() {
        let package_root = dir.join("node_modules").join(package);
        if package_root.join("package.json").exists() {
            if let Ok(entry) = resolve_entry(&package_root) {
                return Some(Installed {
                    root: dir,
                    package: package.to_owned(),
                    version: version.to_owned(),
                    entry,
                    reused: true,
                });
            }
        }
    }
    None
}

/// Resolve a package's plugin entrypoint: `exports["./server"]` →
/// `exports["."]` → `main` → `index.js` (v2 `Host.resolve` probing).
pub fn resolve_entry(package_root: &Path) -> Result<PathBuf, String> {
    let manifest: Value = {
        let text = std::fs::read_to_string(package_root.join("package.json"))
            .map_err(|error| format!("read {}: {error}", package_root.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("parse package.json: {error}"))?
    };
    let exports = manifest.get("exports").and_then(Value::as_object);
    let mut candidates: Vec<String> = Vec::new();
    if let Some(exports) = exports {
        for key in ["./server", "."] {
            if let Some(entry) = exports.get(key) {
                collect_export_paths(entry, &mut candidates);
            }
        }
    }
    if let Some(main) = manifest.get("main").and_then(Value::as_str) {
        candidates.push(main.to_owned());
    }
    candidates.push("index.js".to_owned());
    for candidate in candidates {
        let path = package_root.join(candidate);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!("no entrypoint found for {}", package_root.display()))
}

/// Resolve ONE named `exports` subpath (e.g. `./tui` — the TUI-plugin
/// entry the v2 host resolves separately from the server entry).
/// Falls back to `main`/`index.js` like the server probe.
pub fn resolve_subpath_entry(package_root: &Path, subpath: &str) -> Result<PathBuf, String> {
    let manifest: Value = {
        let text = std::fs::read_to_string(package_root.join("package.json"))
            .map_err(|error| format!("read {}: {error}", package_root.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("parse package.json: {error}"))?
    };
    let mut candidates: Vec<String> = Vec::new();
    if let Some(entry) =
        manifest.get("exports").and_then(Value::as_object).and_then(|exports| exports.get(subpath))
    {
        collect_export_paths(entry, &mut candidates);
    }
    if let Some(main) = manifest.get("main").and_then(Value::as_str) {
        candidates.push(main.to_owned());
    }
    candidates.push("index.js".to_owned());
    for candidate in candidates {
        let path = package_root.join(candidate);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!("no {subpath} entrypoint found for {}", package_root.display()))
}

/// The nearest enclosing package root for an entry: the closest
/// ancestor directory holding a `package.json` (ref `Host.resolve`
/// probes subpaths against the package; the tui companion loader walks
/// the same way).
#[must_use]
pub fn package_root(entry: &Path) -> Option<PathBuf> {
    let mut root = entry.parent()?;
    loop {
        if root.join("package.json").is_file() {
            return Some(root.to_path_buf());
        }
        root = root.parent()?;
    }
}

/// `Host.resolve`'s feature entrypoints (host.ts:41-43): the `tui` and
/// `rpc` subpaths — `exports["./<subpath>"]` plus classic
/// `<root>/<subpath>.<ext>` / `<root>/<subpath>/index.<ext>` resolution.
/// Unlike the server probe there is NO `main`/`index.js` fallback: only
/// the subpath's own file counts (G-280③ — ref resolves `pkg/tui` /
/// `dir/tui` through Node resolution and swallows
/// `ERR_PACKAGE_PATH_NOT_EXPORTED`, so a main-only package advertises
/// neither feature).
#[must_use]
pub fn probe_feature_entry(entry: &Path, subpath: &str) -> Option<PathBuf> {
    let root = package_root(entry)?;
    let manifest = std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    if let Some(export) = manifest
        .as_ref()
        .and_then(|manifest| manifest.get("exports"))
        .and_then(Value::as_object)
        .and_then(|exports| exports.get(subpath))
    {
        let mut candidates: Vec<String> = Vec::new();
        collect_export_paths(export, &mut candidates);
        for candidate in candidates {
            let path = root.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    for extension in ["js", "mjs", "ts"] {
        let file = root.join(format!("{subpath}.{extension}"));
        if file.is_file() {
            return Some(file);
        }
        let index = root.join(subpath).join(format!("index.{extension}"));
        if index.is_file() {
            return Some(index);
        }
    }
    None
}

/// One `exports` clause → importable relative paths (strings and
/// condition objects, `import`/`default` preferred).
fn collect_export_paths(entry: &Value, out: &mut Vec<String>) {
    match entry {
        Value::String(path) => out.push(path.clone()),
        Value::Object(map) => {
            for key in ["import", "default", "require", "types"] {
                if let Some(nested) = map.get(key) {
                    collect_export_paths(nested, out);
                }
            }
            for (key, nested) in map {
                if !matches!(key.as_str(), "import" | "default" | "require" | "types") {
                    collect_export_paths(nested, out);
                }
            }
        }
        _ => {}
    }
}

fn find_npm() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join("npm")).find(|candidate| candidate.is_file())
}

/// The npm to use: the explicit override, else the first `npm` on PATH.
fn npm_for(options: &InstallOptions) -> Option<PathBuf> {
    options.npm.clone().or_else(find_npm)
}

/// `npm view <pkg> version` — the registry probe (`plugin add`'s
/// version pin AND the update path's latest resolution).
pub fn pin_latest(package: &str, options: &InstallOptions) -> Result<String, String> {
    let npm = npm_for(options).ok_or("cannot pin an unpinned spec without npm")?;
    let mut args = vec!["view".to_owned(), package.to_owned(), "version".to_owned()];
    if options.offline {
        args.push("--offline".into());
    }
    let output = Command::new(&npm)
        .args(&args)
        .output()
        .map_err(|error| format!("npm view {package}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "npm view {package} version failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if version.is_empty() || version.contains('\n') {
        return Err(format!("npm view {package} returned an unusable version {version:?}"));
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package_with_exports(exports: &Value) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ocph-install-{}-{}",
            std::process::id(),
            exports.to_string().len()
        ));
        let pkg = dir.join("node_modules").join("demo-plugin");
        std::fs::create_dir_all(pkg.join("dist").join("server")).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            serde_json::to_string(&serde_json::json!({
                "name": "demo-plugin",
                "version": "1.0.0",
                "exports": exports,
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(pkg.join("dist/server/index.js"), "export default {}").unwrap();
        std::fs::write(pkg.join("dist/index.js"), "export default {}").unwrap();
        pkg
    }

    #[test]
    fn resolve_entry_probes_server_export_first() {
        let pkg = package_with_exports(&serde_json::json!({
            ".": { "import": "./dist/index.js" },
            "./server": { "import": "./dist/server/index.js" },
        }));
        let entry = resolve_entry(&pkg).expect("entry resolves");
        assert!(entry.ends_with("dist/server/index.js"), "{}", entry.display());
    }

    /// The TUI entry resolves independently (`exports["./tui"]` — the
    /// flagship's `./tui` → `dist/tui2.js` shape).
    #[test]
    fn resolve_subpath_entry_probes_the_tui_export() {
        let pkg = package_with_exports(&serde_json::json!({
            ".": { "import": "./dist/index.js" },
            "./server": { "import": "./dist/server/index.js" },
            "./tui": { "import": "./dist/tui2.js", "types": "./dist/v2/tui.d.ts" },
        }));
        std::fs::write(pkg.join("dist/tui2.js"), "export default {}").unwrap();
        let entry = resolve_subpath_entry(&pkg, "./tui").expect("tui entry resolves");
        assert!(entry.ends_with("dist/tui2.js"), "{}", entry.display());
        // An absent subpath falls back to main (a package without a
        // TUI entry is not an error — probing is cheap).
        let bare = package_with_exports(&serde_json::json!({
            "./server": { "import": "./dist/server/index.js" },
        }));
        std::fs::write(
            bare.join("package.json"),
            serde_json::to_string(&serde_json::json!({
                "name": "demo-plugin",
                "version": "1.0.0",
                "exports": { "./server": { "import": "./dist/server/index.js" } },
                "main": "./dist/server/index.js",
            }))
            .unwrap(),
        )
        .unwrap();
        let entry = resolve_subpath_entry(&bare, "./tui").expect("fallback resolves");
        assert!(entry.ends_with("dist/server/index.js"), "{}", entry.display());
    }

    #[test]
    fn resolve_entry_falls_back_to_main() {
        let pkg = package_with_exports(&serde_json::json!({}));
        let manifest = serde_json::json!({
            "name": "demo-plugin",
            "version": "1.0.0",
            "main": "./dist/index.js",
        });
        std::fs::write(pkg.join("package.json"), serde_json::to_string(&manifest).unwrap())
            .unwrap();
        let entry = resolve_entry(&pkg).expect("entry resolves");
        assert!(entry.ends_with("dist/index.js"), "{}", entry.display());
    }

    /// G-280③ — the feature entrypoints (host.ts:41-43): `./tui` and
    /// `./rpc` resolve from their OWN subpath only — no `main`/`index`
    /// fallback (ref swallows `ERR_PACKAGE_PATH_NOT_EXPORTED`, so a
    /// main-only package advertises neither feature).
    #[test]
    fn g280_probe_feature_entry_detects_tui_and_rpc_subpaths() {
        let pkg = g280_package(
            "exports",
            &serde_json::json!({
                ".": { "import": "./dist/index.js" },
                "./server": { "import": "./dist/server/index.js" },
                "./tui": { "import": "./dist/tui2.js" },
                "./rpc": { "import": "./dist/rpc.js" },
            }),
        );
        std::fs::write(pkg.join("dist/tui2.js"), "export default {}").unwrap();
        std::fs::write(pkg.join("dist/rpc.js"), "export default {}").unwrap();
        let entry = resolve_entry(&pkg).expect("server entry");
        let tui = probe_feature_entry(&entry, "./tui").expect("tui entry");
        assert!(tui.ends_with("dist/tui2.js"), "{}", tui.display());
        let rpc = probe_feature_entry(&entry, "./rpc").expect("rpc entry");
        assert!(rpc.ends_with("dist/rpc.js"), "{}", rpc.display());
        // A subpath the package does not ship → None (no fallback).
        assert!(probe_feature_entry(&entry, "./hook").is_none());
    }

    /// G-280③ — the classic-resolution half: a package without an
    /// exports map still advertises `tui`/`rpc` when the subpath files
    /// exist (`<root>/rpc.js`, `<root>/tui/index.mjs`), and a main-only
    /// package advertises NEITHER (the loader probe's main fallback is
    /// deliberately absent here).
    #[test]
    fn g280_probe_feature_entry_classic_files_without_main_fallback() {
        let pkg = g280_package("main-only", &serde_json::json!(null));
        std::fs::write(
            pkg.join("package.json"),
            serde_json::to_string(&serde_json::json!({
                "name": "demo-plugin",
                "version": "1.0.0",
                "main": "./dist/index.js",
            }))
            .unwrap(),
        )
        .unwrap();
        let entry = resolve_entry(&pkg).expect("main entry");
        assert!(probe_feature_entry(&entry, "./tui").is_none(), "main must not fake a tui feature");
        assert!(
            probe_feature_entry(&entry, "./rpc").is_none(),
            "main must not fake an rpc feature"
        );
        // Classic files turn the features on.
        std::fs::write(pkg.join("rpc.js"), "export default {}").unwrap();
        std::fs::create_dir_all(pkg.join("tui")).unwrap();
        std::fs::write(pkg.join("tui/index.mjs"), "export default {}").unwrap();
        assert!(probe_feature_entry(&entry, "./rpc").is_some());
        assert!(probe_feature_entry(&entry, "./tui").is_some());
    }

    /// A uniquely-named package fixture for the G-280③ probes (the
    /// shared `package_with_exports` helper keys its tempdir by the
    /// exports JSON length — parallel tests would collide).
    fn g280_package(tag: &str, exports: &Value) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ocph-g280-{tag}-{}-{}",
            std::process::id(),
            exports.to_string().len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let pkg = dir.join("demo-plugin");
        std::fs::create_dir_all(pkg.join("dist/server")).unwrap();
        let mut manifest = serde_json::json!({
            "name": "demo-plugin",
            "version": "1.0.0",
        });
        if !exports.is_null() {
            manifest["exports"] = exports.clone();
        }
        std::fs::write(pkg.join("package.json"), manifest.to_string()).unwrap();
        std::fs::write(pkg.join("dist/server/index.js"), "export default {}").unwrap();
        std::fs::write(pkg.join("dist/index.js"), "export default {}").unwrap();
        pkg
    }

    #[test]
    fn latest_install_reuses_pre_staged_trees() {
        let root = std::env::temp_dir().join(format!("ocph-reuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pkg =
            root.join("demo-plugin@9.9.9").join("12345").join("node_modules").join("demo-plugin");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            serde_json::json!({ "name": "demo-plugin", "version": "9.9.9", "main": "plugin.js" })
                .to_string(),
        )
        .unwrap();
        std::fs::write(pkg.join("plugin.js"), "export default {}").unwrap();

        let found = latest_install(&root, "demo-plugin", "9.9.9").expect("pre-staged found");
        assert!(found.reused);
        assert!(found.entry.ends_with("plugin.js"));

        let options = InstallOptions { root: Some(root.clone()), ..InstallOptions::default() };
        let installed = install("demo-plugin@9.9.9", &options).expect("no network needed");
        assert!(installed.reused);
        assert_eq!(installed.version, "9.9.9");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_rejects_bad_specs() {
        let options = InstallOptions::default();
        assert!(install("", &options).is_err());
        assert!(install("-nope", &options).is_err());
    }

    /// G-365 — the install path's atomicity: everything (manifest,
    /// installer, verification) runs in a `.staging-*` directory and
    /// only a verified install is renamed into the final timestamped
    /// location. A failing installer leaves NO reusable state.
    #[test]
    fn g365_failing_install_leaves_no_reusable_state() {
        let root = std::env::temp_dir().join(format!("ocph-g365-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let options = InstallOptions {
            root: Some(root.clone()),
            allow_existing: false,
            bun: Some(failing_installer_shim("g365fail")),
            ..InstallOptions::default()
        };
        let error = install("demo-plugin@1.0.0", &options).expect_err("the shim fails");
        assert!(error.contains("failed"), "the installer failure surfaces: {error}");
        let key = root.join("demo-plugin@1.0.0");
        // No final (digit-named) generation exists…
        let names = dir_names(&key);
        assert!(
            names.iter().all(|name| !name.chars().all(|c| c.is_ascii_digit())),
            "no partial final generation: {names:?}"
        );
        // …and latest_install finds nothing (staging residue, if any,
        // is invisible).
        assert!(latest_install(&root, "demo-plugin", "1.0.0").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// G-365 — a successful install lands in the final directory
    /// (populated), and the staging tree is gone.
    #[test]
    fn g365_successful_install_renames_out_of_staging() {
        let root = std::env::temp_dir().join(format!("ocph-g365-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let options = InstallOptions {
            root: Some(root.clone()),
            allow_existing: false,
            bun: Some(successful_installer_shim("g365ok")),
            ..InstallOptions::default()
        };
        let installed = install("demo-plugin@1.0.0", &options).expect("the shim installs");
        assert!(!installed.reused);
        // The final generation exists and is populated…
        assert!(
            installed.root.join("node_modules").join("demo-plugin").join("package.json").is_file(),
            "final dir populated: {}",
            installed.root.display()
        );
        assert!(installed.entry.is_file(), "entry resolves in the FINAL location");
        let names = dir_names(&root.join("demo-plugin@1.0.0"));
        assert!(
            names.iter().any(|name| name.chars().all(|c| c.is_ascii_digit())),
            "a digit-named generation exists: {names:?}"
        );
        assert!(
            names.iter().all(|name| !name.starts_with(".staging-")),
            "staging is cleaned on success: {names:?}"
        );
        // And it is reusable from here on.
        assert!(latest_install(&root, "demo-plugin", "1.0.0").is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn dir_names(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(std::result::Result::ok)
                    .map(|entry| entry.file_name().into_string().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A fake `bun` that always fails (the install seam the tests use).
    fn failing_installer_shim(tag: &str) -> PathBuf {
        let shim =
            std::env::temp_dir().join(format!("ocph-shim-fail-{tag}-{}", std::process::id()));
        std::fs::write(&shim, "#!/bin/sh\nexit 1\n").unwrap();
        make_executable(&shim);
        shim
    }

    /// A fake `bun install` that materializes the package tree (runs
    /// with cwd = the staging directory).
    fn successful_installer_shim(tag: &str) -> PathBuf {
        let shim = std::env::temp_dir().join(format!("ocph-shim-ok-{tag}-{}", std::process::id()));
        std::fs::write(
            &shim,
            concat!(
                "#!/bin/sh\n",
                "mkdir -p node_modules/demo-plugin\n",
                "printf '{\"name\":\"demo-plugin\",\"version\":\"1.0.0\",\"main\":\"plugin.js\"}' ",
                "> node_modules/demo-plugin/package.json\n",
                "echo 'export default {}' > node_modules/demo-plugin/plugin.js\n",
                "exit 0\n",
            ),
        )
        .unwrap();
        make_executable(&shim);
        shim
    }

    fn make_executable(shim: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = shim;
        }
    }
}
