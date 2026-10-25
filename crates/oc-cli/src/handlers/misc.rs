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
/// `--continue`/`--session` route a session; `--prompt` seeds the
/// booted composer and auto-submits the first turn, `--auto` arms
/// permission autoaccept (`handlers/default.ts:110-118`'s args).
pub async fn root(
    directory: Option<&str>,
    continue_last: bool,
    session: Option<String>,
    prompt: Option<String>,
    auto: bool,
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
    // RB-02 — the `--session` existence probe (see [`boot_session_ids`]).
    let (session, new_session_id) = match boot_session_ids(&client, session).await {
        Ok(ids) => ids,
        Err(message) => {
            eprintln!("Error: {message}");
            return 1;
        }
    };
    let boot = oc_tui::app::BootArgs { continue_last, session, new_session_id, prompt, auto };
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
    // RB-10c — the "Installation method:" line (ref uninstall.ts:31
    // `updater.method()` + :46 `log.info(\`Installation method: ${method
    // ?? "unknown"}\`)`): detection runs BEFORE the removal preview;
    // an undetectable method prints "unknown" plus the warning below
    // (ref :52 `if (!method) log.warn(...)`).
    let method = installation_method(&exe);
    println!("Installation method: {method}");
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
    if method == "unknown" {
        println!(
            "Could not detect the installation method. Remove the installation manually after cleanup."
        );
    }

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

/// `opencode pair` — one-time pairing links + the QR block.
pub async fn pair(url: Option<&str>) -> i32 {
    // RB-03 — `--url` validation (ref commands.ts:524-532, done at
    // flag-parse time there): HTTP(S) only, no credentials, no query,
    // no fragment; the value is normalized through URL parsing with
    // trailing slashes stripped. Identical rejection message.
    let provided_url = match url {
        Some(url) => match validate_pair_url(url) {
            Ok(normalized) => Some(normalized),
            Err(message) => {
                eprintln!("Error: {message}");
                return 1;
            }
        },
        None => None,
    };
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
    let urls: Vec<String> = match provided_url {
        Some(url) => vec![url],
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
    // The QR block — ref commands/handlers/pair.ts:30-39: only when a
    // first link exists, blank line before, `renderUnicodeCompact`
    // with `border: 2`, every row indented by two spaces (the ref
    // splits uqr's "\n"-joined rows and prefixes "  ").
    if let Some(first) = links.first() {
        println!();
        for line in qr_block(first) {
            println!("{line}");
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
            match name {
                // clap already rejects unknown keys (G-13); the lookup
                // only maps the validated key onto its root.
                Some(name) => {
                    if let Some((_, path)) = paths.iter().find(|(key, _)| *key == name.as_str()) {
                        println!("{}", path.display());
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

/// RB-02 — the root `--session` existence probe (ref
/// packages/cli/src/commands/handlers/default.ts:53-61 + 111-114):
/// `findSession` (`session-target.ts:134-139`) decides continue vs
/// pre-allocate. An EXISTING id routes that session (`sessionID`); a
/// NON-EXISTENT id does NOT error — it becomes the id of the session
/// the first prompt creates (`newSessionID`; the ref's own comment:
/// "A missing --session ID becomes the ID of the session the first
/// prompt creates") and the boot stays HOME. Any lookup error other
/// than `SessionNotFoundError` is fatal (ref's `Effect.tryPromise`
/// rethrows — the handler fails with `errorMessage(cause)`).
async fn boot_session_ids(
    client: &oc_client::Client,
    session: Option<String>,
) -> Result<(Option<String>, Option<String>), String> {
    match session {
        Some(id) => match client.session().get(&id).await {
            Ok(_) => Ok((Some(id), None)),
            Err(error) if error.api_name() == Some("SessionNotFoundError") => Ok((None, Some(id))),
            Err(error) => Err(util::error_message(&error.to_string())),
        },
        None => Ok((None, None)),
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

/// RB-03 — the `pair --url` flag's `mapTryCatch` (ref
/// commands.ts:524-532): parse, require `http:`/`https:`, reject
/// credentials/query/hash (the URL's `username`/`password`/`search`/
/// `hash` are all empty), normalize through the parsed href and strip
/// trailing slashes (`url.href.replace(/\/+$/, "")`).
/// The CLI pair output's QR block (ref commands/handlers/pair.ts
/// :30-39): uqr `renderUnicodeCompact(link, { border: 2 })`, each
/// row prefixed by two spaces. (uqr joins with "\n" on every
/// platform — the ref's EOL-split comment, pair.ts:33.)
fn qr_block(link: &str) -> Vec<String> {
    oc_uqr::render_unicode_compact(link, 2).split('\n').map(|line| format!("  {line}")).collect()
}

fn validate_pair_url(raw: &str) -> Result<String, &'static str> {
    const MESSAGE: &str =
        "Expected an HTTP(S) server URL without credentials, query parameters, or a fragment";
    let url = reqwest::Url::parse(raw).map_err(|_| MESSAGE)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        // ref checks `url.password` truthiness (commands.ts) — an EMPTY
        // password (`http://:@host`) is falsy there and passes; mirror
        // that instead of rejecting any Some("") (oracle gate nit).
        || url.password().is_some_and(|p| !p.is_empty())
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(MESSAGE);
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// RB-10c — the uninstall "Installation method:" line's method name
/// (ref uninstall.ts:31 `updater.method()` + :46
/// `method ?? "unknown"`). Detection lives in
/// [`crate::services::updater::detect_method`].
fn installation_method(exe: &std::path::Path) -> &'static str {
    crate::services::updater::detect_method(exe).unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- F15: the pair QR block ---------------------------------------

    /// ref pair.ts:34 — `renderUnicodeCompact(links[0], { border: 2 })`
    /// with every row indented 2. The block matches the vendored
    /// oc-uqr output (itself golden-tested against the ref-pinned
    /// uqr 0.1.3), so this pins the indentation + border choice.
    #[test]
    fn pair_qr_block_is_border_two_indented() {
        let block = qr_block("http://localhost:4096/auth/connect/pair-code-123");
        assert!(!block.is_empty(), "the QR rows render");
        assert!(block.iter().all(|line| line.starts_with("  ")), "two-space indent");
        let expected: Vec<String> =
            oc_uqr::render_unicode_compact("http://localhost:4096/auth/connect/pair-code-123", 2)
                .split('\n')
                .map(|line| format!("  {line}"))
                .collect();
        assert_eq!(block, expected);
        // border 2 → v3 (29) + 4 = 33 modules wide.
        assert!(block.iter().all(|line| line.chars().count() == 33 + 2));
    }

    // --- RB-03: pair --url validation (ref commands.ts:524-532) -------

    #[test]
    fn pair_url_accepts_http_and_https() {
        assert_eq!(validate_pair_url("http://example.com").as_deref(), Ok("http://example.com"));
        assert_eq!(validate_pair_url("https://example.com").as_deref(), Ok("https://example.com"));
        assert_eq!(
            validate_pair_url("https://example.com:4096").as_deref(),
            Ok("https://example.com:4096")
        );
    }

    #[test]
    fn pair_url_error_message_is_ref_exact() {
        assert_eq!(
            validate_pair_url("ftp://example.com"),
            Err("Expected an HTTP(S) server URL without credentials, query parameters, or a fragment")
        );
    }

    #[test]
    fn pair_url_rejects_credentials_query_and_fragment() {
        let err = Err(
            "Expected an HTTP(S) server URL without credentials, query parameters, or a fragment",
        );
        assert_eq!(validate_pair_url("http://user:pass@example.com"), err);
        // Empty-credential URL passes (ref falsy-password parity); the
        // href normalization drops the empty `:@` (WHATWG serialization).
        assert_eq!(validate_pair_url("http://:@example.com").as_deref(), Ok("http://example.com"),);
        assert_eq!(validate_pair_url("http://example.com/?q=1"), err);
        assert_eq!(validate_pair_url("http://example.com/#frag"), err);
        assert_eq!(validate_pair_url("not a url"), err);
    }

    #[test]
    fn pair_url_normalizes_trailing_slashes() {
        assert_eq!(validate_pair_url("https://example.com/").as_deref(), Ok("https://example.com"));
        assert_eq!(
            validate_pair_url("https://example.com///").as_deref(),
            Ok("https://example.com")
        );
    }

    // --- RB-02: root --session probe (ref default.ts:53-61,111-114) --

    /// One-request HTTP responder: `/api/session/<id>` answered per
    /// the map (status, JSON body).
    async fn fake_session_server(
        routes: std::collections::HashMap<&'static str, (u16, String)>,
    ) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let routes = routes.clone();
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt as _;
                    use tokio::io::AsyncWriteExt as _;
                    let mut buf = [0_u8; 4096];
                    let Ok(read) = socket.read(&mut buf).await else { return };
                    let head = String::from_utf8_lossy(&buf[..read]);
                    let path =
                        head.lines().next().unwrap_or_default().split(' ').nth(1).unwrap_or("");
                    let id = path.rsplit('/').next().unwrap_or("");
                    let (status, body) = routes
                        .get(id)
                        .cloned()
                        .unwrap_or((404, r#"{"_tag":"SessionNotFoundError"}"#.to_owned()));
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn boot_session_probe_mirrors_ref_semantics() {
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            "ses_exists",
            (
                200_u16,
                r#"{"data":{"id":"ses_exists","cost":0,"location":{"directory":"/tmp"},"projectID":"p","time":{"created":0,"updated":0},"tokens":{"cache":{"read":0,"write":0},"input":0,"output":0,"reasoning":0}}}"#.to_owned(),
            ),
        );
        // 404 without a declared tag: transport-level UnexpectedStatus —
        // NOT SessionNotFound, so it must be fatal (ref rethrows).
        routes.insert("ses_error", (500, r#"{"boom":1}"#.to_owned()));
        let base = fake_session_server(routes).await;
        let client = oc_client::Client::new(&base).expect("client");

        // No --session: neither id set.
        assert_eq!(
            boot_session_ids(&client, None).await.expect("none ok"),
            (None::<String>, None::<String>)
        );
        // Existent id → continue that session (sessionID).
        assert_eq!(
            boot_session_ids(&client, Some("ses_exists".to_owned())).await.expect("exists ok"),
            (Some("ses_exists".to_owned()), None)
        );
        // Nonexistent id → ref-mirrored: no error; the id becomes the
        // first-created session's id (newSessionID).
        assert_eq!(
            boot_session_ids(&client, Some("ses_missing".to_owned())).await.expect("missing ok"),
            (None, Some("ses_missing".to_owned()))
        );
        // Any other lookup error is fatal.
        assert!(boot_session_ids(&client, Some("ses_error".to_owned())).await.is_err());
    }

    // --- RB-10c: installation method line (ref uninstall.ts:31,46) ---

    #[test]
    fn installation_method_unknown_off_and_curl_at_the_install_location() {
        // One test (not two): both scenarios mutate HOME — the repo's
        // established pattern (updater's detect_method test) keeps the
        // mutation inside a single test to avoid a parallel race.
        let home = tempfile::tempdir().expect("home");
        let saved = std::env::var("HOME").ok();
        std::env::set_var("HOME", home.path());
        let elsewhere = home.path().join("somewhere/opencode");
        std::fs::create_dir_all(elsewhere.parent().expect("parent")).expect("mkdir");
        std::fs::write(&elsewhere, "x").expect("write");
        assert_eq!(installation_method(&elsewhere), "unknown");
        let installed = crate::services::updater::curl_binary();
        std::fs::create_dir_all(installed.parent().expect("parent")).expect("mkdir");
        std::fs::write(&installed, "x").expect("write");
        // The tempdir is a symlinked /tmp path on some hosts — compare
        // through the same canonicalization detect_method applies.
        let exe = std::fs::canonicalize(&installed).expect("canonical");
        assert_eq!(installation_method(&exe), "curl");
        if let Some(saved) = saved {
            std::env::set_var("HOME", saved);
        } else {
            std::env::remove_var("HOME");
        }
    }
}
