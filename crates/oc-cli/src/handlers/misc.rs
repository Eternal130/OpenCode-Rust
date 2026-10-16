//! The root handler (the TUI launch), the real upgrade/uninstall
//! (Phase 10), acp/mini placeholders, models, reload, pair, and the
//! debug subcommands.

use crate::cli::{DebugCommands, ServerParams};
use crate::handlers::connect;
use crate::services::server_connection::{self, Mismatch};
use crate::util;

/// Root `opencode [directory]` — chdir, ensure the server (mismatch:
/// replace), then launch the TUI against the resolved endpoint
/// (the Phase-7.2 port; errors clearly without a TTY). The boot
/// flags follow v2's route rules (`app.tsx:364-371`): default = home,
/// `--continue`/`--session` route a session.
pub async fn root(
    directory: Option<&str>,
    continue_last: bool,
    session: Option<String>,
    server: &crate::cli::ServerParams,
) -> i32 {
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
    // The TUI owns the terminal from here; the ensured server stays
    // alive for the duration (the guard drops after).
    let client = resolved.endpoint.client();
    let boot = oc_tui::app::BootArgs { continue_last, session };
    match oc_tui::app::run(client, boot).await {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("Error: {message}");
            1
        }
    }
}

/// `opencode upgrade` — REAL self-download-and-replace (Phase 10
/// packaging): `local-file` installs from a path on disk, `github`
/// downloads the Rust port's own release tarball (network-optional,
/// `OPENCODE_RELEASE_URL`), package-manager methods explain themselves.
/// The background AUTO-update loop stays default-OFF (oracle gate 1 /
/// the incident-era decision — only this explicit command replaces
/// the binary).
pub async fn upgrade(target: Option<&str>, method: Option<&str>) -> i32 {
    crate::services::updater::upgrade(crate::services::updater::UpgradeRequest {
        method: method.unwrap_or("auto").to_owned(),
        target: target.map(str::to_owned),
    })
    .await
}

/// `opencode uninstall` — REAL (Phase 10): stops every registered
/// background service, removes the global roots (respecting
/// `--keep-config`/`--keep-data`), cleans the shell PATH entry, then
/// removes the binary itself (unix self-unlink). `--dry-run` previews,
/// `--force` skips the confirmation prompt (non-TTY without either is
/// an error, v2's `requireInteractive` message).
#[allow(clippy::fn_params_excessive_bools)] // the v2 flag surface, mirrored
pub async fn uninstall(dry_run: bool, keep_config: bool, keep_data: bool, force: bool) -> i32 {
    let plan = crate::services::updater::removal_plan(keep_config, keep_data);
    let services = crate::services::updater::registration_files();
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("opencode"));

    println!("Uninstall OpenCode");
    println!(
        "The following global files will be removed (shared by OpenCode versions and channels):"
    );
    for removal in &plan {
        if removal.path.exists() {
            println!(
                "  {}: {}{}",
                removal.label,
                removal.path.display(),
                if removal.keep { " (keeping)" } else { "" }
            );
        }
    }
    for file in &services {
        println!("  Stop background service and persistent terminals: {}", file.display());
    }
    println!("  Binary: {}", exe.display());

    if dry_run {
        println!("Dry run - no changes made");
        return 0;
    }
    if !force {
        if !is_tty() {
            eprintln!(
                "Error: Use --force to uninstall without an interactive terminal, or --dry-run to preview."
            );
            return 1;
        }
        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Are you sure you want to uninstall?")
            .default(false)
            .interact()
            .unwrap_or(false);
        if !confirmed {
            println!("Cancelled");
            return 0;
        }
    }
    if !services.is_empty() {
        println!("Stopping background services...");
        crate::services::updater::stop_services(&services).await;
        println!("Background services stopped");
    }

    let mut errors = crate::services::updater::execute_removals(&plan);
    for config in crate::services::updater::shell_configs() {
        let Ok(content) = std::fs::read_to_string(&config) else { continue };
        let cleaned = crate::services::updater::clean_shell_config(&content);
        if cleaned == content {
            continue;
        }
        if let Err(error) = std::fs::write(&config, cleaned) {
            errors.push(format!("Shell config {}: {error}", config.display()));
        }
    }
    // Self-unlink (unix keeps the running inode alive until exit).
    if let Err(error) = std::fs::remove_file(&exe) {
        if exe.exists() {
            errors.push(format!("Binary: {error}"));
        }
    }
    if errors.is_empty() {
        println!("Done");
        0
    } else {
        for error in &errors {
            eprintln!("Error: {error}");
        }
        1
    }
}

/// stdin-on-a-tty check (`requireInteractive`'s gate).
fn is_tty() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdin().is_terminal()
}

/// `opencode acp` — REAL (R-03): the ACP agent over stdio (see
/// `crate::acp`).
pub async fn acp() -> i32 {
    crate::acp::run().await
}

/// `opencode mini` — the split-footer interactive frontend (R-02):
/// [`crate::mini::run_mini`] (the `runMini` port) over the resolved
/// server endpoint.
pub async fn mini(input: crate::mini::MiniInput, server: &crate::cli::ServerParams) -> i32 {
    crate::mini::run_mini(input, server).await
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
