//! `opencode plugin *` — plugin management (handlers/plugin/*). `add`
//! performs a real pinned install (wave 2: the oc-plugin-host
//! install path — frozen when a lockfile exists, reuse-first); update
//! remains config-managed (REGISTERED divergence).

use serde_json::{json, Value};

use crate::cli::PluginCommands;
use crate::cli::ServerParams;
use crate::handlers::connect;
use crate::util::error_message;

fn location_query() -> oc_client::types::QueryLocation {
    oc_client::types::QueryLocation {
        directory: Some(
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        ),
    }
}

pub async fn run(command: &PluginCommands, server: &ServerParams) -> i32 {
    match command {
        PluginCommands::List { builtin } => list(*builtin, server).await,
        PluginCommands::Add { package, force } => add(package, *force),
        PluginCommands::Check { target } => check(target.as_deref(), server).await,
        PluginCommands::Update { target } => update(target.as_deref(), server).await,
        PluginCommands::Remove { package, force } => remove(package, *force),
    }
}

async fn list(builtin: bool, server: &ServerParams) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let plugins = match resolved
        .endpoint
        .client()
        .plugin()
        .list(&oc_client::types::PluginListQuery { location: Some(location_query()) })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for plugin in &plugins.data {
        let value = serde_json::to_value(plugin).expect("plugin serializes");
        let source = value.get("source").cloned().unwrap_or(Value::Null);
        let source_type = source.get("type").and_then(Value::as_str).unwrap_or_default();
        if !builtin && source_type == "builtin" {
            continue;
        }
        let id = value.get("id").and_then(Value::as_str).unwrap_or("-").to_owned();
        let (version, target) = match source_type {
            "package" => (
                source.get("version").and_then(Value::as_str).unwrap_or("-").to_owned(),
                source.get("target").and_then(Value::as_str).unwrap_or("-").to_owned(),
            ),
            "local" => (
                "local".to_owned(),
                source.get("path").and_then(Value::as_str).unwrap_or("").to_owned(),
            ),
            other => ("-".to_owned(), other.to_owned()),
        };
        rows.push((id, version, target));
    }
    // The TUI rows (v2 list.ts:24-60): the cli.json `plugins[]`
    // entries — the TUI plugin configuration — whose installed package
    // ships a `./tui` entrypoint. They join the table with id `-`
    // (no server registration to name), unless the server inventory
    // already lists the same target.
    for (target, version) in configured_tui_plugins() {
        if rows.iter().any(|(_, _, row_target)| row_target == &target) {
            continue;
        }
        rows.push(("-".to_owned(), version, target));
    }
    if rows.is_empty() {
        println!("No plugins found");
        return 0;
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(&b.2)));
    let mut table: Vec<Vec<String>> =
        vec![vec!["ID".to_owned(), "VERSION".to_owned(), "SOURCE".to_owned()]];
    for (id, version, target) in &rows {
        let version = if version.len() == 40 || version.len() == 64 {
            version.chars().take(7).collect()
        } else {
            version.clone()
        };
        table.push(vec![id.clone(), version, target.clone()]);
    }
    let width0 = table.iter().map(|row| row[0].len()).max().unwrap_or(0);
    let width1 = table.iter().map(|row| row[1].len()).max().unwrap_or(0);
    for row in &table {
        println!("{:<w0$}  {:<w1$}  {}", row[0], row[1], row[2], w0 = width0, w1 = width1);
    }
    0
}

fn global_config_path() -> std::path::PathBuf {
    let config = oc_util::global_roots::resolve().config;
    crate::handlers::mcp::resolve_config_path_pub(&config)
}

fn read_global_config() -> Result<Value, String> {
    crate::handlers::mcp::read_config_json_pub(&global_config_path())
}

/// D-01: v2 `writePluginConfig`/`removePluginConfig` — set `["plugins"]`
/// surgically over the original text (comments preserved), with the
/// trailing-newline normalization; `force` is the rust-extra strip
/// rewrite (v2 has no --force).
fn write_global_plugins(plugins: &Value, full_config: &Value, force: bool) -> Result<(), String> {
    crate::handlers::mcp::write_config_set_pub(
        &global_config_path(),
        &["plugins"],
        plugins,
        full_config,
        force,
        true,
    )
}

fn matches_entry(entry: &Value, spec: &str) -> bool {
    entry.as_str() == Some(spec) || entry.get("package").and_then(Value::as_str) == Some(spec)
}

fn configured(plugins: Option<&Vec<Value>>, spec: &str) -> bool {
    plugins.is_some_and(|plugins| plugins.iter().any(|entry| matches_entry(entry, spec)))
}

// --- entrypoint routing (v2 plugin/add.ts + plugin/host.ts) ---------------

/// `pkg` | `pkg@version` → (package, Some(version)?) — the same split
/// the installer uses (`context/install.rs` `split_spec`).
fn split_spec(spec: &str) -> (String, Option<String>) {
    match spec.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => {
            (name.to_owned(), Some(version.to_owned()))
        }
        _ => (spec.to_owned(), None),
    }
}

/// The newest existing installation of `spec`'s package (any version)
/// under the plugin-host install root → `(version, package_root)`.
fn newest_installed(spec: &str) -> Option<(String, std::path::PathBuf)> {
    let (package, _) = split_spec(spec);
    let root = oc_plugin_host::install_root();
    let mut best: Option<(std::time::SystemTime, String, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(version) = name.strip_prefix(&format!("{package}@")) else { continue };
        let Ok(stamps) = std::fs::read_dir(entry.path()) else { continue };
        for stamp in stamps.flatten() {
            let package_root = stamp.path().join("node_modules").join(&package);
            if !package_root.join("package.json").is_file() {
                continue;
            }
            let modified = stamp
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(time, _, _)| modified > *time) {
                best = Some((modified, version.to_owned(), package_root));
            }
        }
    }
    best.map(|(_, version, package_root)| (version, package_root))
}

/// `Host.resolve` (plugin/host.ts:17-44) entrypoint probing for one
/// installed package: `server` = `exports["./server"]` or `exports["."]`
/// (an `exports` object WITHOUT those keys hides `main` — Node fails
/// `pkg` with `ERR_PACKAGE_PATH_NOT_EXPORTED`); no `exports` at all falls
/// back to `main`/`index.js`. `tui` = `exports["./tui"]` only.
fn entrypoints(package_root: &std::path::Path) -> (bool, bool) {
    let Ok(text) = std::fs::read_to_string(package_root.join("package.json")) else {
        return (false, false);
    };
    let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
        return (false, false);
    };
    let export_exists = |key: &str| -> bool {
        manifest.get("exports").and_then(Value::as_object).is_some_and(|exports| {
            exports.get(key).is_some_and(|entry| {
                let paths = export_paths(entry);
                paths.iter().any(|path| package_root.join(path).is_file())
            })
        })
    };
    let server = if manifest.get("exports").and_then(Value::as_object).is_some() {
        export_exists("./server") || export_exists(".")
    } else {
        let main = manifest.get("main").and_then(Value::as_str);
        main.is_some_and(|main| package_root.join(main).is_file())
            || package_root.join("index.js").is_file()
    };
    let tui = export_exists("./tui");
    (server, tui)
}

/// One `exports` clause → importable relative paths (strings and
/// condition objects, `import`/`default`/`require` preferred — the
/// same walk the installer's export probe uses).
fn export_paths(entry: &Value) -> Vec<String> {
    fn collect(entry: &Value, out: &mut Vec<String>) {
        match entry {
            Value::String(path) => out.push(path.clone()),
            Value::Object(map) => {
                for key in ["import", "default", "require"] {
                    if let Some(nested) = map.get(key) {
                        collect(nested, out);
                    }
                }
                for (key, nested) in map {
                    if !matches!(key.as_str(), "import" | "default" | "require" | "types") {
                        collect(nested, out);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    collect(entry, &mut out);
    out
}

/// The cli.json (`Config.Service`) document handle.
fn cli_config() -> oc_core::cli_config::CliConfig {
    oc_core::cli_config::CliConfig::open(&oc_util::global_roots::resolve())
}

/// The TUI-plugin targets configured in cli.json `plugins[]` whose
/// installed package ships a `./tui` entrypoint (v2 list.ts:28-46 —
/// directives and builtins are skipped; unresolvable/uninstalled
/// targets degrade to skipped, the offline shape of `npm.resolve`).
fn configured_tui_plugins() -> Vec<(String, String)> {
    let info = cli_config().get();
    let Some(plugins) = info.plugins else { return Vec::new() };
    let mut rows = Vec::new();
    for spec in plugins {
        let target = match &spec {
            oc_core::cli_config::PluginSpec::Directive(target) => target.clone(),
            oc_core::cli_config::PluginSpec::Entry { package, .. } => package.clone(),
        };
        if target.starts_with('-')
            || target == "*"
            || target.ends_with(".*")
            || target.starts_with("opencode.")
        {
            continue;
        }
        if let Some((version, package_root)) = newest_installed(&target) {
            if entrypoints(&package_root).1 {
                rows.push((target, version));
            }
        }
    }
    rows
}

fn add(package: &str, force: bool) -> i32 {
    if package.starts_with('-')
        || package == "*"
        || package.ends_with(".*")
        || package.starts_with("opencode.")
    {
        eprintln!("Error: Plugin target must be an npm registry package or Git package specifier");
        return 1;
    }
    // v2 add.ts: install FIRST (`npm.add`), then route by the package's
    // entrypoints — an unroutable package errors without touching any
    // configuration.
    let installed =
        match oc_plugin_host::install(package, &oc_plugin_host::InstallOptions::default()) {
            Ok(installed) => installed,
            Err(error) => {
                eprintln!("Error: {error}");
                return 1;
            }
        };
    let (name, _) = split_spec(package);
    let package_root = installed.root.join("node_modules").join(name);
    let (server_entry, tui_entry) = entrypoints(&package_root);
    // `configurationTarget(entrypoints.server, entrypoints.tui)`:
    // a server entrypoint wins; otherwise a TUI entrypoint routes the
    // plugin to cli.json (the TUI's plugin configuration); neither is
    // an error (add.ts:23-25).
    if !server_entry && !tui_entry {
        eprintln!("Error: Plugin package has no server or TUI entrypoint: {package}");
        return 1;
    }
    if server_entry {
        let config_path = global_config_path();
        let mut config = match read_global_config() {
            Ok(config) => config,
            Err(message) => {
                eprintln!("Error: {message}");
                return 1;
            }
        };
        let plugins = config.get("plugins").and_then(Value::as_array).cloned();
        if configured(plugins.as_ref(), package) {
            println!("Plugin \"{package}\" is already configured in {}", config_path.display());
        } else {
            let root = config.as_object_mut().expect("config object");
            let entry = root.entry("plugins".to_owned()).or_insert_with(|| json!([]));
            entry.as_array_mut().expect("plugins array").push(json!(package));
            let plugins = entry.clone();
            if let Err(message) = write_global_plugins(&plugins, &config, force) {
                eprintln!("Error: {message}");
                return 1;
            }
            println!("Plugin \"{package}\" installed and added to {}", config_path.display());
        }
        return 0;
    }
    // The TUI arm: `config.update` on cli.json — append to `plugins[]`
    // when not already configured (add.ts:33-40), then the fixed
    // `TUI plugin …` message naming the config path.
    let config = cli_config();
    let mut already = false;
    let updated = config.update(|draft| {
        let plugins = draft
            .as_object_mut()
            .unwrap_or_else(|| panic!("config draft is an object"))
            .entry("plugins".to_owned())
            .or_insert_with(|| json!([]));
        if plugins.as_array().is_some_and(|entries| configured(Some(entries), package)) {
            already = true;
            return;
        }
        plugins.as_array_mut().expect("plugins array").push(json!(package));
    });
    match updated {
        Ok(_) => {
            println!("TUI plugin \"{package}\" installed and added to {}", config.path().display());
            0
        }
        Err(message) => {
            eprintln!("Error: {message}");
            1
        }
    }
}

fn remove(package: &str, force: bool) -> i32 {
    // v2 remove.ts: remove from the global (server) configuration AND
    // from cli.json when configured there; the message names every file
    // that changed.
    let config_path = global_config_path();
    let mut config = match read_global_config() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("Error: {message}");
            return 1;
        }
    };
    let mut removed = Vec::new();
    let plugins = config.get("plugins").and_then(Value::as_array).cloned();
    if configured(plugins.as_ref(), package) {
        if let Some(entries) = config
            .as_object_mut()
            .expect("config object")
            .get_mut("plugins")
            .and_then(Value::as_array_mut)
        {
            entries.retain(|entry| !matches_entry(entry, package));
        }
        let filtered = config.get("plugins").cloned().unwrap_or_else(|| json!([]));
        if let Err(message) = write_global_plugins(&filtered, &config, force) {
            eprintln!("Error: {message}");
            return 1;
        }
        removed.push(config_path.display().to_string());
    }
    // The cli.json leg: filter `plugins[]` when the package is a
    // configured TUI plugin (remove.ts:15-27).
    let cli = cli_config();
    let info = cli.get();
    let tui_configured = info.plugins.is_some_and(|plugins| {
        plugins.iter().any(|spec| {
            matches!(
                spec,
                oc_core::cli_config::PluginSpec::Directive(target)
                    | oc_core::cli_config::PluginSpec::Entry { package: target, .. }
                    if target == package
            )
        })
    });
    if tui_configured {
        match cli.update(|draft| {
            if let Some(entries) = draft
                .as_object_mut()
                .expect("draft object")
                .get_mut("plugins")
                .and_then(Value::as_array_mut)
            {
                entries.retain(|entry| !matches_entry(entry, package));
            }
        }) {
            Ok(_) => removed.push(cli.path().display().to_string()),
            Err(message) => {
                eprintln!("Error: {message}");
                return 1;
            }
        }
    }
    if removed.is_empty() {
        println!("Plugin \"{package}\" is not configured");
        return 0;
    }
    println!("Plugin \"{package}\" removed from {}", removed.join(", "));
    0
}

async fn check(target: Option<&str>, server: &ServerParams) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let response = match resolved
        .endpoint
        .client()
        .plugin()
        .check(
            &oc_client::types::PluginCheckQuery { location: Some(location_query()) },
            &oc_client::types::PluginCheckBody { target: target.map(str::to_owned) },
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let value = serde_json::to_value(&response.data).expect("check serializes");
    let items = value.as_array().cloned().unwrap_or_default();
    let mut output = Vec::new();
    let mut errors = false;
    for item in &items {
        if item.get("error").is_some_and(|error| !error.is_null()) {
            errors = true;
        }
        let name = item.get("name").and_then(Value::as_str).unwrap_or("-");
        let current = item.get("version").and_then(Value::as_str).unwrap_or("-");
        let latest = item
            .get("latest")
            .and_then(Value::as_str)
            .map_or_else(|| current.to_owned(), str::to_owned);
        output.push(format!("{name} {current} → {latest}"));
    }
    if output.is_empty() {
        println!("No package plugins found");
    } else {
        for line in output {
            println!("{line}");
        }
    }
    i32::from(errors)
}

async fn update(target: Option<&str>, server: &ServerParams) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let response = match client
        .plugin()
        .check(
            &oc_client::types::PluginCheckQuery { location: Some(location_query()) },
            &oc_client::types::PluginCheckBody { target: target.map(str::to_owned) },
        )
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let value = serde_json::to_value(&response.data).expect("check serializes");
    let items = value.as_array().cloned().unwrap_or_default();
    let mut updated = 0usize;
    let mut errors = 0usize;
    for item in &items {
        if item.get("error").is_some_and(|error| !error.is_null()) {
            eprintln!(
                "Failed to check Server plugin \"{}\": {}",
                item.get("name").and_then(Value::as_str).unwrap_or("-"),
                item.get("error").and_then(Value::as_str).unwrap_or_default()
            );
            errors += 1;
            continue;
        }
        if !item.get("outdated").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let target_id = item.get("target").and_then(Value::as_str).unwrap_or_default().to_owned();
        match client
            .plugin()
            .update(
                &oc_client::types::PluginUpdateQuery { location: Some(location_query()) },
                &oc_client::types::PluginUpdateBody { targets: vec![target_id] },
            )
            .await
        {
            Ok(()) => {
                println!(
                    "Updated Server plugin \"{}\"",
                    item.get("name").and_then(Value::as_str).unwrap_or("-")
                );
                updated += 1;
            }
            Err(error) => {
                eprintln!(
                    "Failed to update Server plugin \"{}\": {error}",
                    item.get("name").and_then(Value::as_str).unwrap_or("-")
                );
                errors += 1;
            }
        }
    }
    if updated == 0 && errors == 0 {
        println!("No plugin updates available");
    }
    i32::from(errors > 0)
}
