//! `opencode plugin *` — plugin management (handlers/plugin/*). `add`
//! performs a real pinned install (wave 2: the oc-plugin-host
//! install path — frozen when a lockfile exists, reuse-first); update
//! remains config-managed (REGISTERED divergence).

use serde_json::{json, Value};

use crate::cli::PluginCommands;
use crate::handlers::connect;
use crate::util::error_message;

fn location_query() -> oc_client::types::QueryLocation {
    oc_client::types::QueryLocation {
        directory: Some(
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        ),
    }
}

pub async fn run(command: &PluginCommands) -> i32 {
    match command {
        PluginCommands::List { builtin } => list(*builtin).await,
        PluginCommands::Add { package, force } => add(package, *force),
        PluginCommands::Check { target } => check(target.as_deref()).await,
        PluginCommands::Update { target } => update(target.as_deref()).await,
        PluginCommands::Remove { package, force } => remove(package, *force),
    }
}

async fn list(builtin: bool) -> i32 {
    let resolved = match connect(&crate::cli::ServerParams::default(), None).await {
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

fn write_global_config(config: &Value, force: bool) -> Result<(), String> {
    crate::handlers::mcp::write_config_json_pub(&global_config_path(), config, force)
}

fn matches_entry(entry: &Value, spec: &str) -> bool {
    entry.as_str() == Some(spec) || entry.get("package").and_then(Value::as_str) == Some(spec)
}

fn configured(plugins: Option<&Vec<Value>>, spec: &str) -> bool {
    plugins.is_some_and(|plugins| plugins.iter().any(|entry| matches_entry(entry, spec)))
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
    let config_path = global_config_path();
    let mut config = match read_global_config() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("Error: {message}");
            return 1;
        }
    };
    let plugins = config.get("plugins").and_then(Value::as_array).cloned();
    let already = configured(plugins.as_ref(), package);
    if already {
        println!("Plugin \"{package}\" is already configured in {}", config_path.display());
    } else {
        let root = config.as_object_mut().expect("config object");
        let entry = root.entry("plugins".to_owned()).or_insert_with(|| json!([]));
        entry.as_array_mut().expect("plugins array").push(json!(package));
        if let Err(message) = write_global_config(&config, force) {
            eprintln!("Error: {message}");
            return 1;
        }
    }
    // Real installation (wave 2): pinned + frozen npm/bun install via
    // the plugin host's install path. An existing installation for the
    // pinned version is reused; unpinned specs are pinned via
    // `npm view <pkg> version` first.
    match oc_plugin_host::install(package, &oc_plugin_host::InstallOptions::default()) {
        Ok(installed) => {
            println!(
                "Plugin \"{package}\" added to {} ({} {} → {})",
                config_path.display(),
                if installed.reused { "already installed" } else { "installed" },
                installed.version,
                installed.entry.display()
            );
            0
        }
        Err(error) => {
            eprintln!("Plugin \"{package}\" configured, but installation failed: {error}");
            1
        }
    }
}

fn remove(package: &str, force: bool) -> i32 {
    let config_path = global_config_path();
    let mut config = match read_global_config() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("Error: {message}");
            return 1;
        }
    };
    let plugins = config.get("plugins").and_then(Value::as_array).cloned();
    if !configured(plugins.as_ref(), package) {
        println!("Plugin \"{package}\" is not configured");
        return 0;
    }
    if let Some(entries) = config
        .as_object_mut()
        .expect("config object")
        .get_mut("plugins")
        .and_then(Value::as_array_mut)
    {
        entries.retain(|entry| !matches_entry(entry, package));
    }
    if let Err(message) = write_global_config(&config, force) {
        eprintln!("Error: {message}");
        return 1;
    }
    println!("Plugin \"{package}\" removed from {}", config_path.display());
    0
}

async fn check(target: Option<&str>) -> i32 {
    let resolved = match connect(&crate::cli::ServerParams::default(), None).await {
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

async fn update(target: Option<&str>) -> i32 {
    let resolved = match connect(&crate::cli::ServerParams::default(), None).await {
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
