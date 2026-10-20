//! Config `plugins[]` source resolution — the boot-time glue shared by
//! both embedders (oc-server's engine boot and the TUI's app boot).
//!
//! oc-core's config loader NORMALIZES the `plugins` array (spec
//! strings + `{package, options}` objects, v1 `plugin` migrated in);
//! this module turns those directives into loadable entry paths:
//!
//! * **local paths** (`./rel`, `../x`, `file://…`, absolute, or an
//!   existing path) — the file itself, or a directory resolved
//!   through its `package.json` exports (the same `./server` → `.` →
//!   `main` → `index.js` probing as [`super::context::install`]);
//! * **package specs** (`pkg`, `pkg@1.2.3`, scoped `@org/pkg@ver`) —
//!   resolved against the v2-compatible install cache
//!   `<cache>/opencode/npm/<pkg>@<version>/<timestamp>/node_modules/<pkg>`
//!   (what `opencode plugin add` lays down). The NEWEST timestamped
//!   install wins; an unpinned spec scans every `<pkg>@*` version.
//!
//! Resolution is OFFLINE by design (boot must never hit the network):
//! a spec without an install is a recorded failure, not an install
//! attempt — `opencode plugin add` owns installation. Git specs
//! (`github:…`) resolve the same way when `plugin add` installed them
//! (the cache key is the raw spec), else they fail with the hint.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::context::install::{default_root as install_root, latest_install, resolve_entry};

/// One classified config directive (v2 `ConfigPluginSource.parse`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginOp {
    /// Add a package (spec string or `{package, options}`).
    Add { target: String, options: Value },
    /// Remove a previously added package (`-id`, `-*`).
    Remove { target: String },
}

/// Parse the raw `plugins` array entries (the normalized JSON shape:
/// `"spec"` strings and `{package, options?}` objects). The bare `"-"`,
/// non-string/non-object shapes, and `{package}` without a package are
/// errors — v2 rejects them at source parse.
pub fn parse_ops(entries: &[Value]) -> Result<Vec<PluginOp>, String> {
    let mut ops = Vec::new();
    for entry in entries {
        match entry {
            Value::String(spec) => {
                if !spec.starts_with('-') {
                    ops.push(PluginOp::Add { target: spec.clone(), options: Value::Null });
                } else if spec.len() == 1 {
                    return Err("Plugin remove operation requires a target".to_owned());
                } else {
                    ops.push(PluginOp::Remove { target: spec[1..].to_owned() });
                }
            }
            Value::Object(object) => {
                let Some(package) = object.get("package").and_then(Value::as_str) else {
                    return Err("Plugin entry object requires a `package` string".to_owned());
                };
                ops.push(PluginOp::Add {
                    target: package.to_owned(),
                    options: object.get("options").cloned().unwrap_or(Value::Null),
                });
            }
            other => return Err(format!("Plugin entry must be a string or object: {other}")),
        }
    }
    Ok(ops)
}

/// How one resolved plugin was found (the v2 `Plugin.Source` shape's
/// driver — `/api/plugin` renders it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginSourceKind {
    /// An installed package (`target`, resolved `version`).
    Package { target: String, version: Option<String> },
    /// A local file or directory.
    Local { path: String },
}

/// One plugin that resolved to a loadable entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlugin {
    /// The configured spec (dedup/remove key).
    pub target: String,
    /// The config `options` (v2 `{package, options}`; `Null` = none).
    pub options: Value,
    /// The absolute entrypoint the host loads.
    pub entry: PathBuf,
    pub source: PluginSourceKind,
}

/// The boot-time resolution outcome.
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    pub plugins: Vec<ResolvedPlugin>,
    /// `(target, error)` for every add that did not resolve — surfaced
    /// through `/api/plugin` as `state: failed`.
    pub failures: Vec<(String, String)>,
}

impl Resolution {
    /// The entry paths (the host's `entries`).
    #[must_use]
    pub fn entries(&self) -> Vec<PathBuf> {
        self.plugins.iter().map(|plugin| plugin.entry.clone()).collect()
    }

    /// `entry_options` keyed by entry path (the host's `ctx.options`).
    #[must_use]
    pub fn entry_options(&self) -> std::collections::BTreeMap<String, Value> {
        fn options_is_none(options: &Value) -> bool {
            options.is_null() || options.as_object().is_some_and(serde_json::Map::is_empty)
        }
        self.plugins
            .iter()
            .filter(|plugin| !options_is_none(&plugin.options))
            .map(|plugin| (plugin.entry.display().to_string(), plugin.options.clone()))
            .collect()
    }
}

/// v2 `PluginSourceDirectory.names` (`plugin/source-directory.ts`): the
/// subdirectory names every config layer's root scans for drop-in
/// plugins.
pub const PLUGIN_SOURCE_DIRECTORIES: &[&str] = &["plugin", "plugins"];

/// Whether a path is a plugin source file (v2 `.ts`/`.js` only).
fn is_source_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "ts" || ext == "js")
}

/// PLUG-D01 — v2 `PluginSourceDirectory.discover` +
/// `ConfigPluginSource.scan`: every config directory's
/// [`PLUGIN_SOURCE_DIRECTORIES`] children auto-add — `.ts`/`.js` files,
/// subdirectories, and symlinks resolving to either — sorted by target
/// path. The discovered `Add`s come BEFORE the document `plugins[]`
/// ops (the caller's ordering) so an explicit remove (`-name`) drops a
/// discovered plugin exactly like v2's "explicit config is applied
/// last".
#[must_use]
pub fn discover_directory_ops(directories: &[PathBuf]) -> Vec<PluginOp> {
    let mut targets: Vec<PathBuf> = Vec::new();
    for directory in directories {
        for name in PLUGIN_SOURCE_DIRECTORIES {
            let source = directory.join(name);
            let Ok(entries) = std::fs::read_dir(&source) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let accepted = match entry.file_type() {
                    Ok(file_type) if file_type.is_dir() => true,
                    Ok(file_type) if file_type.is_file() => is_source_file(&path),
                    Ok(file_type) if file_type.is_symlink() => {
                        // v2: a symlink counts when it resolves to a
                        // source file or a directory.
                        std::fs::metadata(&path).is_ok_and(|resolved| {
                            resolved.is_dir() || (resolved.is_file() && is_source_file(&path))
                        })
                    }
                    _ => false,
                };
                if accepted && !targets.contains(&path) {
                    targets.push(path);
                }
            }
        }
    }
    targets.sort();
    targets
        .into_iter()
        .map(|target| PluginOp::Add { target: target.display().to_string(), options: Value::Null })
        .collect()
}

/// Resolve the directive list (applies removes in order — v2 semantics:
/// explicit removes override auto-discovered/earlier adds; `-*` clears
/// every previous add). `cache_root` defaults to the install cache.
#[must_use]
pub fn resolve(ops: &[PluginOp], cache_root: Option<PathBuf>) -> Resolution {
    let root = cache_root.unwrap_or_else(install_root);
    // Directive reduction FIRST (v2 applies the ordered operations over
    // the source list, then loads what survived): removes drop earlier
    // adds by id/package/path-stem; `-*` clears; duplicate specs load once.
    let mut targets: Vec<(String, Value)> = Vec::new();
    for op in ops {
        match op {
            PluginOp::Add { target, options } => {
                if targets.iter().any(|(seen, _)| seen == target) {
                    continue; // same spec twice: one load (v2 dedup by id)
                }
                targets.push((target.clone(), options.clone()));
            }
            PluginOp::Remove { target } => {
                if target == "*" {
                    targets.clear();
                } else {
                    targets.retain(|(seen, _)| !remove_matches(seen, target));
                }
            }
        }
    }
    let mut resolution = Resolution::default();
    for (target, options) in targets {
        match resolve_one(&target, &root) {
            Ok(mut plugin) => {
                plugin.options = options;
                resolution.plugins.push(plugin);
            }
            Err(error) => resolution.failures.push((target, error)),
        }
    }
    resolution
}

/// v2 remove matching: the directive id matches the spec, the package
/// name of the spec, or a local path stem.
fn remove_matches(target: &str, directive: &str) -> bool {
    if target == directive {
        return true;
    }
    let (package, _) = split_spec(target);
    if package == directive {
        return true;
    }
    let stem = Path::new(target).file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    stem == directive
}

/// `pkg` | `pkg@version` | `@org/pkg` | `@org/pkg@version` →
/// (package, Some(version)?).
fn split_spec(spec: &str) -> (String, Option<String>) {
    match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => {
            (name.to_owned(), Some(version.to_owned()))
        }
        _ => (spec.to_owned(), None),
    }
}

/// v2 `isLocal`: relative, absolute, `file://`, or an existing path.
fn is_local_spec(spec: &str) -> bool {
    spec.starts_with("./")
        || spec.starts_with("../")
        || spec.starts_with('/')
        || spec.starts_with("file://")
        || Path::new(spec).exists()
}

fn resolve_one(target: &str, root: &Path) -> Result<ResolvedPlugin, String> {
    if is_local_spec(target) {
        let stripped = target.strip_prefix("file://").unwrap_or(target);
        let path = Path::new(stripped);
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
        };
        let entry = if absolute.is_file() {
            absolute.clone()
        } else if absolute.is_dir() {
            if absolute.join("package.json").is_file() {
                resolve_entry(&absolute)
                    .map_err(|error| format!("local plugin {}: {error}", absolute.display()))?
            } else {
                // A bare directory without a manifest: probe the common
                // entry names (v2 loads the path as a module directly).
                ["index.ts", "index.js", "plugin.ts", "plugin.js"]
                    .iter()
                    .map(|name| absolute.join(name))
                    .find(|candidate| candidate.is_file())
                    .ok_or_else(|| {
                        format!(
                            "local plugin {} has no package.json or index/plugin entry",
                            absolute.display()
                        )
                    })?
            }
        } else {
            return Err(format!("local plugin path does not exist: {}", absolute.display()));
        };
        return Ok(ResolvedPlugin {
            target: target.to_owned(),
            options: Value::Null,
            entry,
            source: PluginSourceKind::Local { path: absolute.display().to_string() },
        });
    }

    // Package spec: the install cache (offline; `plugin add` installs).
    let (package, requested) = split_spec(target);
    if package.is_empty() || package.starts_with("github:") || package.starts_with("git") {
        return Err(format!(
            "plugin {target:?} is not installed; run `opencode plugin add {target}`"
        ));
    }
    let installed = match &requested {
        Some(version) => latest_install(root, &package, version),
        None => latest_any_install(root, &package),
    };
    let installed = installed.ok_or_else(|| {
        format!("plugin {target:?} is not installed; run `opencode plugin add {target}`")
    })?;
    Ok(ResolvedPlugin {
        target: target.to_owned(),
        options: Value::Null,
        entry: installed.entry,
        source: PluginSourceKind::Package {
            target: target.to_owned(),
            version: Some(installed.version),
        },
    })
}

/// The newest timestamped install of `package` across every
/// `<pkg>@<version>` directory (unpinned specs — the live layout keys
/// them `@latest`, `plugin add` pins to the resolved version; the
/// newest stamp under the greatest version-sort key wins).
fn latest_any_install(root: &Path, package: &str) -> Option<super::context::install::Installed> {
    let mut versions: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&format!("{package}@")))
        })
        .collect();
    versions.sort();
    let mut best: Option<(String, super::context::install::Installed)> = None;
    for version_dir in versions.into_iter().rev() {
        let version = version_dir
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.rsplit_once('@'))
            .map(|(_, version)| version.to_owned())
            .unwrap_or_default();
        if let Some(installed) = latest_install(root, package, &version) {
            let stamp = installed
                .root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            // Newest stamp wins; ties prefer the first (version order).
            if best.as_ref().is_none_or(|(best_stamp, _)| stamp > *best_stamp) {
                best = Some((stamp, installed));
            }
        }
    }
    best.map(|(_, installed)| installed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cache_with(package: &str, version: &str, stamp: &str, main: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ocph-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        let pkg = root
            .join(format!("{package}@{version}"))
            .join(stamp)
            .join("node_modules")
            .join(package);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            json!({ "name": package, "version": version, "main": main }).to_string(),
        )
        .unwrap();
        std::fs::write(pkg.join(main), "export default {}").unwrap();
        root
    }

    #[test]
    fn parses_adds_removes_and_entry_options() {
        let ops = parse_ops(&[
            json!("pkg-a"),
            json!({ "package": "pkg-b", "options": { "flag": true } }),
            json!("-pkg-a"),
            json!("-*"),
        ])
        .expect("ops parse");
        assert_eq!(
            ops,
            vec![
                PluginOp::Add { target: "pkg-a".into(), options: Value::Null },
                PluginOp::Add { target: "pkg-b".into(), options: json!({ "flag": true }) },
                PluginOp::Remove { target: "pkg-a".into() },
                PluginOp::Remove { target: "*".into() },
            ]
        );
        assert!(parse_ops(&[json!("-")]).is_err(), "the bare remove is invalid");
        assert!(parse_ops(&[json!(17)]).is_err());
        assert!(parse_ops(&[json!({ "options": {} })]).is_err());
    }

    #[test]
    fn removes_apply_in_order_and_star_clears() {
        let ops = parse_ops(&[json!("a"), json!("b"), json!("-a"), json!("c")]).unwrap();
        let resolution = resolve(&ops, Some(PathBuf::from("/nonexistent-cache")));
        // None resolve (no cache) — but the removes removed `a`.
        let failed: Vec<&str> = resolution.failures.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(failed, ["b", "c"], "`a` was removed before resolution");
        assert!(resolution.plugins.is_empty(), "{:?}", resolution.plugins);

        let ops = parse_ops(&[json!("a"), json!("-*"), json!("b")]).unwrap();
        let resolution = resolve(&ops, Some(PathBuf::from("/nonexistent-cache")));
        assert_eq!(resolution.failures.len(), 1, "`-*` cleared `a`");
        assert_eq!(resolution.failures[0].0, "b");
    }

    #[test]
    fn local_paths_resolve_files_and_directories() {
        let dir = std::env::temp_dir().join(format!("ocph-resolve-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.ts"), "export default {}").unwrap();
        let ops = parse_ops(&[json!(dir.join("plugin.ts").display().to_string())]).unwrap();
        let resolution = resolve(&ops, Some(PathBuf::from("/nonexistent-cache")));
        assert_eq!(resolution.failures.len(), 0);
        assert!(resolution.plugins[0].entry.ends_with("plugin.ts"));
        assert!(matches!(resolution.plugins[0].source, PluginSourceKind::Local { .. }));

        // A relative path against the cwd.
        let ops = parse_ops(&[json!("./definitely-missing-plugin-file.ts")]).unwrap();
        let resolution = resolve(&ops, Some(PathBuf::from("/nonexistent-cache")));
        assert_eq!(resolution.failures.len(), 1, "missing local paths record a failure");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_specs_resolve_the_newest_cached_install() {
        let root = cache_with("demo-pkg", "1.2.3", "1000", "plugin.js");
        // Pinned …
        let ops = parse_ops(&[json!("demo-pkg@1.2.3")]).unwrap();
        let resolution = resolve(&ops, Some(root.clone()));
        assert_eq!(resolution.failures.len(), 0, "{:?}", resolution.failures);
        assert_eq!(resolution.plugins.len(), 1);
        assert!(
            resolution.plugins[0].entry.ends_with("node_modules/demo-pkg/plugin.js"),
            "{}",
            resolution.plugins[0].entry.display()
        );
        assert_eq!(
            resolution.plugins[0].source,
            PluginSourceKind::Package {
                target: "demo-pkg@1.2.3".into(),
                version: Some("1.2.3".into())
            }
        );
        // Unpinned finds the same install …
        let ops = parse_ops(&[json!("demo-pkg")]).unwrap();
        let resolution = resolve(&ops, Some(root.clone()));
        assert_eq!(resolution.plugins.len(), 1, "{:?}", resolution.failures);
        // Entry options keyed by entry path, absent options skipped.
        let ops = parse_ops(&[json!({ "package": "demo-pkg", "options": { "k": 1 } })]).unwrap();
        let resolution = resolve(&ops, Some(root.clone()));
        assert_eq!(resolution.entry_options().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstalled_specs_fail_with_the_hint() {
        let ops = parse_ops(&[json!("never-installed-pkg")]).unwrap();
        let resolution = resolve(&ops, Some(PathBuf::from("/nonexistent-cache")));
        assert!(resolution.plugins.is_empty(), "{:?}", resolution.plugins);
        assert!(resolution.failures[0].1.contains("opencode plugin add"));
    }
}
