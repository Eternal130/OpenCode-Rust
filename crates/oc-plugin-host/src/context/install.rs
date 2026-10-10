//! The pinned, frozen plugin install path (`opencode plugin add`).
//!
//! Installs npm package plugins into the v2-compatible cache layout
//! `<cache>/opencode/npm/<pkg>@<version>/<timestamp>/node_modules/<pkg>`
//! (the live `oh-my-opencode-slim` install's shape), version pinned,
//! lockfile-frozen when one exists:
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
    /// Cache root (default [`default_root`]: the opencode npm cache).
    pub root: Option<PathBuf>,
    /// Pass `--offline` to the installer (hermetic re-installs).
    pub offline: bool,
    /// Reuse an existing `<pkg>@<version>` installation instead of
    /// touching the network (the bring-up test's pre-staged tree).
    pub allow_existing: bool,
    /// Hard timeout for the install command.
    pub timeout: Duration,
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

/// The default cache root: `$XDG_CACHE_HOME|$HOME/.cache` + `opencode/npm`
/// (mirrors the v2 `Global` data layout the live install uses).
#[must_use]
pub fn default_root() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("opencode").join("npm")
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

    let dir = install_dir(&root, &package, &version);
    write_install_manifest(&dir, &package, &version)?;
    run_installer(&dir, options)?;
    verify_and_resolve(&dir, &package, version)
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
fn run_installer(dir: &Path, options: &InstallOptions) -> Result<(), String> {
    let bun = options.bun.clone().or_else(crate::bun::resolve_bun);
    let (program, args): (PathBuf, Vec<String>) = if dir.join("bun.lock").exists()
        || dir.join("bun.lockb").exists()
    {
        let bun = bun.ok_or("bun.lock present but bun is unavailable")?;
        installer_args(bun, &["install", "--frozen-lockfile"], options.offline)
    } else if dir.join("package-lock.json").exists() {
        let npm = find_npm().ok_or("package-lock.json present but npm is unavailable")?;
        installer_args(npm, &["ci"], options.offline)
    } else if let Some(bun) = bun {
        installer_args(bun, &["install"], options.offline)
    } else {
        let npm = find_npm().ok_or("neither bun nor npm is available to install plugins")?;
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

fn pin_latest(package: &str, options: &InstallOptions) -> Result<String, String> {
    let npm = find_npm().ok_or("cannot pin an unpinned spec without npm")?;
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
}
