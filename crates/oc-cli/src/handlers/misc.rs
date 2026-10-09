//! The root handler (TUI placeholder), upgrade/uninstall no-ops, acp/mini
//! placeholders, models, reload, pair, and the debug subcommands.

use crate::cli::{DebugCommands, ServerParams};
use crate::handlers::connect;
use crate::services::server_connection::{self, Mismatch};
use crate::util;

/// Root `opencode [directory]` — chdir, ensure the server (mismatch:
/// replace), then the registered Phase-7 TUI placeholder.
pub async fn root(directory: Option<&str>, server: &crate::cli::ServerParams) -> i32 {
    if let Some(directory) = directory {
        if let Err(error) = std::env::set_current_dir(directory) {
            eprintln!("Error: Failed to change directory to {directory}: {error}");
            return 1;
        }
    }
    let resolved = match server_connection::resolve(server_connection::Args {
        server: server.server.clone(),
        standalone: server.standalone,
        mismatch: Mismatch::Replace,
    })
    .await
    {
        Ok(resolved) => resolved,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    // REGISTERED INTERIM (phase 7): the TUI itself is not yet ported; keep
    // the ensured server alive only long enough to report it.
    println!("TUI not yet ported (phase 7); server running at {}", resolved.endpoint.url);
    drop(resolved);
    0
}

/// `opencode upgrade` — REGISTERED no-op: self-upgrade is disabled in the
/// Rust port (oracle gate 1: the 2.0.24 pin must never adopt newer TS
/// releases; auto-update is disabled).
pub fn upgrade(target: Option<&str>) -> i32 {
    match target {
        Some(target) => {
            println!("Upgrade to {target} skipped: self-upgrade is disabled in the Rust port (pinned to v{}).", util::VERSION);
        }
        None => {
            println!(
                "Upgrade skipped: self-upgrade is disabled in the Rust port (pinned to v{}).",
                util::VERSION
            );
        }
    }
    println!("Auto-update is disabled; install updates with your package manager.");
    0
}

/// `opencode uninstall` — REGISTERED no-op with the same rationale.
pub fn uninstall(dry_run: bool, keep_config: bool, keep_data: bool) -> i32 {
    if dry_run {
        println!("Dry run - no changes made (uninstall is disabled in the Rust port)");
        return 0;
    }
    println!(
        "Uninstall is disabled in the Rust port; remove the binary and these directories manually:"
    );
    let roots = oc_util::global_roots::resolve();
    println!("  Data: {}{}", roots.data.display(), if keep_data { " (keeping)" } else { "" });
    println!("  Cache: {}", roots.cache.display());
    println!("  Config: {}{}", roots.config.display(), if keep_config { " (keeping)" } else { "" });
    println!("  State: {}", roots.state.display());
    0
}

/// `opencode acp` — REGISTERED interim (deferred until after the phase-7 TUI
/// foundation; see MIGRATION.md phase 6/7 notes).
pub fn acp() -> i32 {
    eprintln!("Error: acp is not yet implemented (deferred; see MIGRATION.md)");
    1
}

/// `opencode mini` — REGISTERED interim (phase 8).
pub fn mini() -> i32 {
    eprintln!("Error: mini is not yet ported (phase 8)");
    1
}

/// `opencode models` — `provider/model` lines, sorted.
pub async fn models(server: &ServerParams) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let directory =
        std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
    let response = match resolved
        .endpoint
        .client()
        .model()
        .list(&oc_client::types::ModelListQuery {
            location: Some(oc_client::types::QueryLocation { directory: Some(directory) }),
        })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", util::error_message(&error.to_string()));
            return 1;
        }
    };
    let mut models: Vec<String> =
        response.data.iter().map(|model| format!("{}/{}", model.provider_id, model.id)).collect();
    models.sort();
    if !models.is_empty() {
        for model in models {
            println!("{model}");
        }
    }
    0
}

/// `opencode reload`.
pub async fn reload(server: &ServerParams) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    if let Err(error) = resolved.endpoint.client().location().reload().await {
        eprintln!("{}", util::error_message(&error.to_string()));
        return 1;
    }
    println!("Configuration reloaded");
    0
}

/// `opencode pair` — one-time pairing links. QR rendering is deferred
/// (REGISTERED: links print without the uqr block).
pub async fn pair(url: Option<&str>) -> i32 {
    if crate::services::service_config::read().disabled == Some(true) {
        eprintln!(
            "Error: Pairing requires the background service; run `opencode service unset disabled` first"
        );
        return 1;
    }
    let endpoint = match crate::services::server_connection::ensure_managed(false).await {
        Ok(endpoint) => endpoint,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let client = endpoint.client();
    let urls: Vec<String> = match url {
        Some(url) => vec![url.trim_end_matches('/').to_owned()],
        None => match client.server().info().await {
            Ok(info) => info.urls,
            Err(error) => {
                eprintln!("{}", util::error_message(&error.to_string()));
                return 1;
            }
        },
    };
    let pairing = match client.server().pair().await {
        Ok(pairing) => pairing,
        Err(error) => {
            eprintln!("{}", util::error_message(&error.to_string()));
            return 1;
        }
    };
    let links: Vec<String> =
        urls.iter().map(|url| format!("{url}/auth/connect/{}", pairing.code)).collect();
    println!();
    println!(
        "  Open a link to connect. Links work once and expire in {} minutes.",
        pairing.expires_in / 60
    );
    println!();
    if links.is_empty() {
        println!("  (no server URLs)");
    } else {
        for link in &links {
            println!("  {link}");
        }
    }
    println!();
    if url.is_none() {
        if let Ok(endpoint_url) = reqwest::Url::parse(&endpoint.url) {
            if matches!(endpoint_url.host_str(), Some("localhost" | "127.0.0.1" | "::1")) {
                let port = endpoint_url.port().unwrap_or(80);
                let host = endpoint_url.host_str().unwrap_or("127.0.0.1");
                eprint!(
                    "  Over SSH? Forward the port, then open the link on your machine:\n  \
                     ssh -L {port}:{host}:{port} <host>\n  \
                     If port {port} is busy locally, forward another port and use it in the link.\n\n  \
                     To connect from other devices, run `opencode service set hostname 0.0.0.0`.\n\n"
                );
            }
        }
    }
    0
}

/// `opencode debug` subcommands.
pub async fn debug(command: &DebugCommands) -> i32 {
    match command {
        DebugCommands::Agents => {
            let resolved = match connect(&ServerParams::default(), None).await {
                Ok(resolved) => resolved,
                Err(code) => return code,
            };
            let directory =
                std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
            let agents = match resolved
                .endpoint
                .client()
                .agent()
                .list(&oc_client::types::AgentListQuery {
                    location: Some(oc_client::types::QueryLocation { directory: Some(directory) }),
                })
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("{}", util::error_message(&error.to_string()));
                    return 1;
                }
            };
            let mut agents = agents.data;
            agents.sort_by(|a, b| a.id.cmp(&b.id));
            println!("{}", serde_json::to_string_pretty(&agents).expect("agents serialize"));
            0
        }
        DebugCommands::Config => {
            let resolved = match connect(&ServerParams::default(), None).await {
                Ok(resolved) => resolved,
                Err(code) => return code,
            };
            let directory =
                std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
            let config = match resolved
                .endpoint
                .client()
                .config()
                .get(&oc_client::types::ConfigGetQuery {
                    location: Some(oc_client::types::QueryLocation { directory: Some(directory) }),
                })
                .await
            {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("{}", util::error_message(&error.to_string()));
                    return 1;
                }
            };
            let redacted = redact_config(serde_json::to_value(&config).expect("config serializes"));
            println!("{}", serde_json::to_string_pretty(&redacted).expect("config serializes"));
            0
        }
        DebugCommands::Paths { name } => {
            let roots = oc_util::global_roots::resolve();
            let paths: Vec<(&str, std::path::PathBuf)> = vec![
                ("home", roots.home.clone()),
                ("data", roots.data.clone()),
                ("config", roots.config.clone()),
                ("state", roots.state.clone()),
                ("cache", roots.cache.clone()),
                ("tmp", roots.tmp.clone()),
                ("db", roots.data.join(oc_util::global_roots::DB_FILENAME)),
                ("bin", roots.cache.join("bin")),
                ("log", roots.data.join("log")),
                ("repos", roots.data.join("repos")),
            ];
            match name.as_deref() {
                Some(name) => {
                    if let Some((_, path)) = paths.iter().find(|(key, _)| *key == name) {
                        println!("{}", path.display());
                    } else {
                        eprintln!("Error: Unknown path: {name}");
                        return 1;
                    }
                }
                None => {
                    for (key, path) in &paths {
                        println!("{key:<10} {}", path.display());
                    }
                }
            }
            0
        }
    }
}

/// `redactConfig` (debug/redact.ts): mask secret-ish values.
fn redact_config(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            serde_json::Map::from_iter(map.into_iter().map(|(key, value)| {
                let redacted =
                    if is_secret_key(&key) && value.as_str().is_some_and(|s| !s.is_empty()) {
                        serde_json::Value::String("***".to_owned())
                    } else {
                        redact_config(value)
                    };
                (key, redacted)
            }))
            .into()
        }
        serde_json::Value::Array(items) => items.into_iter().map(redact_config).collect(),
        other => other,
    }
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["apikey", "api_key", "secret", "token", "password"].iter().any(|needle| key.contains(needle))
}
