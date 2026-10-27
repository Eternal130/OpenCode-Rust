//! `opencode mcp *` — MCP server management (handlers/mcp/*).

use serde_json::{json, Value};

use crate::cli::McpCommands;
use crate::handlers::connect;
use crate::util::error_message;

fn location_query() -> oc_client::types::QueryLocation {
    oc_client::types::QueryLocation {
        directory: Some(
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        ),
    }
}

pub async fn run(command: &McpCommands) -> i32 {
    match command {
        McpCommands::List => list().await,
        McpCommands::Add { name, command, url, header, env, global, force } => {
            add(name, command, url.as_deref(), header.as_deref(), env.as_deref(), *global, *force)
                .await
        }
        McpCommands::Auth { name } => auth(name.as_deref()).await,
        McpCommands::Logout { name } => logout(name).await,
    }
}

async fn list() -> i32 {
    let resolved = match connect(&crate::cli::ServerParams::default(), None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let servers = match resolved
        .endpoint
        .client()
        .mcp()
        .list(&oc_client::types::McpListQuery { location: Some(location_query()) })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let mut servers: Vec<Value> = servers
        .data
        .iter()
        .map(|server| serde_json::to_value(server).expect("server serializes"))
        .collect();
    servers.sort_by(|a, b| {
        let name_a = a.get("name").and_then(Value::as_str).unwrap_or_default();
        let name_b = b.get("name").and_then(Value::as_str).unwrap_or_default();
        oc_util::localecmp::locale_cmp(name_a, name_b)
    });
    if servers.is_empty() {
        println!("No MCP servers configured");
        return 0;
    }
    let width = servers
        .iter()
        .map(|server| server.get("name").and_then(Value::as_str).map_or(0, str::len))
        .max()
        .unwrap_or(0);
    for server in &servers {
        let name = server.get("name").and_then(Value::as_str).unwrap_or_default();
        let status = server.get("status").cloned().unwrap_or(Value::Null);
        let status_kind = status.get("status").and_then(Value::as_str).unwrap_or_default();
        let (icon, describe) = match status_kind {
            "connected" => ("✓", status_kind.to_owned()),
            "needs_auth" => ("⚠", "needs authentication".to_owned()),
            "failed" => (
                "✗",
                format!(
                    "failed: {}",
                    status.get("error").and_then(Value::as_str).unwrap_or_default()
                ),
            ),
            other => ("○", other.to_owned()),
        };
        println!("{icon} {name:<width$}  {describe}");
    }
    0
}

/// `resolveConfigPath` (mcp/add.ts): the first existing config file among
/// the four candidates, else the first candidate.
fn resolve_config_path(directory: &std::path::Path) -> std::path::PathBuf {
    resolve_config_path_pub(directory)
}

/// Public re-export for the plugin config editor.
pub(crate) fn resolve_config_path_pub(directory: &std::path::Path) -> std::path::PathBuf {
    let candidates = [
        directory.join("opencode.json"),
        directory.join("opencode.jsonc"),
        directory.join(".opencode").join("opencode.json"),
        directory.join(".opencode").join("opencode.jsonc"),
    ];
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
}

fn read_config_json(path: &std::path::Path) -> Result<Value, String> {
    read_config_json_pub(path)
}

/// Public re-export for the plugin config editor.
pub(crate) fn read_config_json_pub(path: &std::path::Path) -> Result<Value, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{}".to_owned(),
        Err(error) => return Err(error.to_string()),
    };
    // json5 = the loader's JSONC tolerance class: comments, trailing
    // commas, relaxed quoting (v2 reads configs with jsonc-parser's
    // `parse(..., { allowTrailingComma: true })`).
    json5::from_str(&text)
        .map_err(|error| format!("Invalid configuration {}: {error}", path.display()))
}

/// Public re-export for the plugin config editor.
///
/// The strip rewrite: pretty-print the whole document (comments lost).
/// Only reached as the explicit `--force` override — v2 has no --force;
/// the default write path is the comment-preserving
/// [`write_config_set_pub`] (rust-extra documented divergence).
pub(crate) fn write_config_json_pub(
    path: &std::path::Path,
    config: &Value,
    force: bool,
) -> Result<(), String> {
    debug_assert!(force, "non-force writes go through write_config_set_pub");
    write_config_text(path, &{
        let mut text = serde_json::to_string_pretty(config).expect("config serializes");
        text.push('\n');
        text
    })
}

fn write_config_text(path: &std::path::Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, text).map_err(|error| error.to_string())
}

/// D-01 comment-preserving config write (v2 parity): applies the
/// set-path mutation surgically to the ORIGINAL config text via
/// `oc_core::config::jsonc_edit` (a port of jsonc-parser's
/// `modify`/`applyEdits` with `tabSize: 2, insertSpaces: true`) —
/// comments, formatting, and unrelated keys survive byte-for-byte.
///
/// Matches v2's writers: `mcp/add.ts` `write` (sets
/// `["mcp","servers",<name>]`, no trailing-newline normalization) and
/// plugin `add.ts`/`remove.ts` (`["plugins"]`, newline normalized).
/// `force` keeps the rust-extra "discard comments" strip rewrite of the
/// full mutated `full_config`.
///
/// Registered edge: when the existing file has no JSON object at its
/// root the surgical edit is impossible — v2 throws (`Can not add
/// property to parent of type …`); the port refuses with the old
/// --force escape hatch instead.
pub(crate) fn write_config_set_pub(
    path: &std::path::Path,
    segments: &[&str],
    value: &Value,
    full_config: &Value,
    force: bool,
    ensure_trailing_newline: bool,
) -> Result<(), String> {
    if force {
        return write_config_json_pub(path, full_config, true);
    }
    let existing = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".to_owned());
    match oc_core::config::jsonc_edit::set_path(&existing, segments, Some(value)) {
        Ok(Some(mut updated)) => {
            if ensure_trailing_newline && !updated.ends_with('\n') {
                updated.push('\n');
            }
            write_config_text(path, &updated)
        }
        // No-op (removal target absent): leave the file alone.
        Ok(None) => Ok(()),
        Err(_) => Err(format!(
            "{} does not contain a JSON object at the root, so it cannot be edited in place. \
             Re-run with --force to rewrite it anyway (comments are lost) or edit the file manually",
            path.display()
        )),
    }
}

/// The server value `mcp add` persists (mcp/add.ts:22-31): the mutual
/// exclusion / validity checks, then `{type: "remote", url, headers?}` or
/// `{type: "local", command, environment?}` (the command carries its own
/// flags — captured by trailing_var_arg after `--`).
fn build_server(
    command: &[String],
    url: Option<&str>,
    header: Option<&str>,
    env: Option<&str>,
) -> Result<Value, String> {
    let has_command = !command.is_empty();
    if url.is_some() && has_command {
        return Err("Provide either --url <url> or a command after --, not both".to_owned());
    }
    if url.is_none() && !has_command {
        return Err("Provide either --url <url> or a command after --".to_owned());
    }
    if let Some(url) = url {
        if reqwest::Url::parse(url).is_err() {
            return Err(format!("Invalid URL: {url}"));
        }
        if env.is_some() {
            return Err("--env is only valid for local MCP servers".to_owned());
        }
        let mut server = json!({ "type": "remote", "url": url });
        if let Some(header) = header {
            let Some((header_name, header_value)) = header.split_once('=') else {
                return Err(format!("Invalid header, expected name=value: {header}"));
            };
            server
                .as_object_mut()
                .unwrap()
                .insert("headers".to_owned(), json!({ header_name: header_value }));
        }
        Ok(server)
    } else {
        if header.is_some() {
            return Err("--header is only valid for remote MCP servers".to_owned());
        }
        let mut server = json!({ "type": "local", "command": command });
        if let Some(env) = env {
            let Some((env_name, env_value)) = env.split_once('=') else {
                return Err(format!("Invalid env, expected name=value: {env}"));
            };
            server
                .as_object_mut()
                .unwrap()
                .insert("environment".to_owned(), json!({ env_name: env_value }));
        }
        Ok(server)
    }
}

#[allow(clippy::unused_async)]
async fn add(
    name: &str,
    command: &[String],
    url: Option<&str>,
    header: Option<&str>,
    env: Option<&str>,
    global: bool,
    force: bool,
) -> i32 {
    let server = match build_server(command, url, header, env) {
        Ok(server) => server,
        Err(message) => {
            eprintln!("Error: {message}");
            return 1;
        }
    };
    let directory = if global {
        oc_util::global_roots::resolve().config
    } else {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    };
    let config_path = resolve_config_path(&directory);
    let mut config = match read_config_json(&config_path) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("Error: {message}");
            return 1;
        }
    };
    if !config.is_object() {
        eprintln!("Error: Invalid configuration {}", config_path.display());
        return 1;
    }
    let root = config.as_object_mut().expect("checked object");
    root.entry("mcp".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("mcp is an object")
        .entry("servers".to_owned())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .expect("servers is an object")
        .insert(name.to_owned(), server);
    // v2 mcp/add.ts `write`: surgical jsonc edit, no newline fix-up.
    let server_value = root
        .get("mcp")
        .and_then(|mcp| mcp.get("servers"))
        .and_then(|servers| servers.get(name))
        .cloned()
        .expect("server inserted");
    if let Err(message) = write_config_set_pub(
        &config_path,
        &["mcp", "servers", name],
        &server_value,
        &config,
        force,
        false,
    ) {
        eprintln!("Error: {message}");
        return 1;
    }
    println!("MCP server \"{name}\" added to {}", config_path.display());
    0
}

#[allow(clippy::too_many_lines)]
async fn auth(name: Option<&str>) -> i32 {
    let Some(name) = name else {
        eprintln!("Error: Pass an MCP server name when running without an interactive terminal");
        return 1;
    };
    println!("◆ Authenticate an MCP server");
    let resolved = match connect(&crate::cli::ServerParams::default(), None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let servers = match client
        .mcp()
        .list(&oc_client::types::McpListQuery { location: Some(location_query()) })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let server = servers.data.iter().find(|server| server.name == name);
    let Some(server) = server else {
        eprintln!("Error: MCP server not found: {name}");
        return 1;
    };
    let Some(integration_id) = &server.integration_id else {
        eprintln!("Error: MCP server \"{name}\" is not an OAuth-capable remote server");
        return 1;
    };
    if matches!(server.status, oc_client::types::McpServerStatus::NeedsAuth(_)) {
        println!("│ {name} has expired credentials. Re-authenticating...");
    }
    // PLUG-D30: the server config drives the flow (its URL is the OAuth
    // resource identity). Resolved from the same config files `mcp add`
    // writes — the project candidates first, then the global config.
    let Some(config) = resolve_remote_config(name) else {
        eprintln!("Error: MCP server \"{name}\" is not an OAuth-capable remote server");
        return 1;
    };
    if config.oauth == Some(oc_schema::mcp::OAuthValue::Disabled(false)) {
        eprintln!("Error: MCP server \"{name}\" is not an OAuth-capable remote server");
        return 1;
    }

    // The v2 flow (McpOAuth.authorize): discovery → client registration →
    // the authorization URL (PKCE + state) → the loopback callback → the
    // token exchange. The loopback listener runs in THIS process; the
    // browser on the same machine lands on it exactly like v2's
    // server-side listener.
    println!("◇ Starting authorization...");
    let authorization = match oc_core::mcp::oauth::authorize(oc_core::mcp::oauth::AuthorizeInput {
        name: name.to_owned(),
        config: (*config).clone(),
        method_id: integration_id.clone(),
        http: reqwest::Client::new(),
    })
    .await
    {
        Ok(authorization) => authorization,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    println!("│ Authorization started");
    println!("│ {}", authorization.instructions);
    println!("│ {}", authorization.url);
    let oc_core::mcp::oauth::AuthMode::Auto(callback) = authorization.mode else {
        eprintln!("Error: MCP server \"{name}\" did not provide an authorization URL");
        return 1;
    };
    println!("│ Waiting for authorization...");
    let credential = match callback.await {
        Ok(credential) => credential,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };

    // Re-authentication replaces the previous sign-in (mcp/auth.ts): the
    // new credential stores first, the old ones only then go away.
    let previous: Vec<String> = match client.credential().list().await {
        Ok(rows) => rows
            .into_iter()
            .filter(|row| &row.integration_id == integration_id)
            .map(|row| row.id)
            .collect(),
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let value = credential_wire_value(&credential);
    if let Err(error) = client
        .credential()
        .create(&oc_client::types::CredentialCreateInput {
            integration_id: integration_id.clone(),
            label: Some(name.to_owned()),
            value,
            activate: Some(true),
            id: None,
        })
        .await
    {
        eprintln!("{}", error_message(&error.to_string()));
        return 1;
    }
    for credential_id in previous {
        let _ = client.credential().remove(&credential_id).await;
    }
    // The reconnect (index.ts:577-599 fires it on credential.switched;
    // the explicit connect pins it even where the event races): the
    // server reconnects with the stored credential — connected, or back
    // to needs_auth for a rejected one.
    let _ = client
        .mcp()
        .connect(
            name,
            &oc_client::types::ExperimentalMcpConnectQuery { location: Some(location_query()) },
        )
        .await;
    println!("└  Done");
    0
}

/// The credential's HTTP wire value (the client SDK's `Credential.OAuth`
/// shape — camelCase fields, the metadata map carried as-is).
fn credential_wire_value(
    credential: &oc_core::integration::types::CredentialValue,
) -> oc_client::types::CredentialValue {
    let oc_core::integration::types::CredentialValue::OAuth(value) = credential else {
        unreachable!("the MCP OAuth flow yields OAuth credentials");
    };
    oc_client::types::CredentialValue::OAuth(oc_client::types::CredentialOAuth {
        method_id: value.method_id.clone(),
        refresh: value.refresh.clone(),
        access: value.access.clone(),
        expires: value.expires,
        metadata: value.metadata.clone(),
        type_: oc_client::types::CredentialOAuthType::Oauth,
    })
}

/// The remote MCP server config from the local config files (`mcp add`'s
/// resolution: project candidates first, then the global config). Local
/// servers and absent entries are `None` (not OAuth-capable); the first
/// file defining the name wins (a local entry shadows the global one).
fn resolve_remote_config(name: &str) -> Option<Box<oc_schema::mcp::RemoteConfig>> {
    let directory = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let global = oc_util::global_roots::resolve().config;
    for base in [&directory, &global] {
        let path = resolve_config_path(base);
        let Ok(config) = read_config_json(&path) else { continue };
        let Some(server) = config
            .get("mcp")
            .and_then(|mcp| mcp.get("servers"))
            .and_then(|servers| servers.get(name))
        else {
            continue;
        };
        let Ok(parsed) = serde_json::from_value::<oc_schema::mcp::ServerConfig>(server.clone())
        else {
            continue;
        };
        if let oc_schema::mcp::ServerConfig::Remote(remote) = parsed {
            return Some(remote);
        }
        return None;
    }
    None
}

async fn logout(name: &str) -> i32 {
    let resolved = match connect(&crate::cli::ServerParams::default(), None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let servers = match client
        .mcp()
        .list(&oc_client::types::McpListQuery { location: Some(location_query()) })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let Some(server) = servers.data.iter().find(|server| server.name == name) else {
        // RD-18 — resolveIntegration fails on an unknown server
        // (mcp/resolve.ts:14-15: `Effect.fail(new Error("MCP server
        // not found: …"))`), a command failure, not the soft
        // no-credentials line.
        eprintln!("{}", error_message(&format!("MCP server not found: {name}")));
        return 1;
    };
    let Some(integration_id) = &server.integration_id else {
        println!("No stored credentials for {name}");
        return 0;
    };
    // RD-18 — resolveIntegration's semantics (mcp/resolve.ts:8-24 +
    // logout.ts:18-24): the MCP-owned integrationID resolves through
    // `integration.get` (NEVER an integration-name match — provider/
    // plugin integrations could collide), then the response's
    // `connections` filter `type === "credential"` names the removals.
    let integration = match client
        .integration()
        .get(integration_id, &oc_client::types::IntegrationGetQuery {
            location: Some(location_query()),
        })
        .await
    {
        Ok(response) => response.data,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let credentials: Vec<String> = integration
        .connections
        .iter()
        .filter_map(|connection| match connection {
            oc_client::types::ConnectionInfo::CredentialInfo(credential) => {
                Some(credential.id.clone())
            }
            oc_client::types::ConnectionInfo::EnvInfo(_) => None,
        })
        .collect();
    if credentials.is_empty() {
        println!("No stored credentials for {name}");
        return 0;
    }
    for credential_id in credentials {
        if let Err(error) = client.credential().remove(&credential_id).await {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    }
    println!("Removed OAuth credentials for {name}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_rewrite_preserves_comments_without_force() {
        // D-01 inversion: the old gate-7 F5 refusal is gone. An
        // auth-login-style config mutation (mcp add) against a commented
        // jsonc file now applies surgically, v2-style — comments intact.
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{\n  // my carefully annotated config\n  \"mcp\": {}\n}\n")
            .expect("write jsonc");

        write_config_set_pub(
            &path,
            &["mcp", "servers", "github"],
            &json!({"type": "remote", "url": "https://example.com/sse"}),
            &json!({"mcp": {"servers": {"github": {"type": "remote", "url": "https://example.com/sse"}}}}),
            false,
            false,
        )
        .expect("surgical write succeeds");

        let text = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(
            text,
            "{\n  // my carefully annotated config\n  \"mcp\": {\n    \"servers\": {\n      \"github\": {\n        \"type\": \"remote\",\n        \"url\": \"https://example.com/sse\"\n      }\n    }\n  }\n}\n",
            "v2 jsonc modify byte shape"
        );
        assert!(text.contains("my carefully annotated config"), "comments survive");
        let value: Value = json5::from_str(&text).expect("result parses");
        assert_eq!(value["mcp"]["servers"]["github"]["url"], json!("https://example.com/sse"));
    }

    #[test]
    fn jsonc_rewrite_with_force_strips_comments() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{\n  /* notes */\n  \"mcp\": {}\n}\n").expect("write jsonc");

        write_config_set_pub(
            &path,
            &["mcp"],
            &json!({"servers": {}}),
            &json!({"mcp": {"servers": {}}}),
            true,
            true,
        )
        .expect("forced rewrite");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(!text.contains("notes"), "force proceeds with the stripping rewrite");
        assert!(text.contains("servers"));
    }

    #[test]
    fn plugin_style_set_normalizes_trailing_newline() {
        // v2 writePluginConfig appends a missing trailing newline.
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{\n  // plugins\n  \"plugins\": [\"a\"]\n}").expect("write jsonc");

        write_config_set_pub(
            &path,
            &["plugins"],
            &json!(["a", "b"]),
            &json!({"plugins": ["a", "b"]}),
            false,
            true,
        )
        .expect("plugin write");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(text, "{\n  // plugins\n  \"plugins\": [\n    \"a\",\n    \"b\"\n  ]\n}\n");
    }

    #[test]
    fn non_object_root_is_refused_without_force() {
        // Registered edge (v2 throws `Can not add property to parent of
        // type array`): the port keeps the refuse + --force escape.
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "[\n  // not an object\n]\n").expect("write array root");

        let error = write_config_set_pub(
            &path,
            &["mcp"],
            &json!({"servers": {}}),
            &json!({"mcp": {"servers": {}}}),
            false,
            false,
        )
        .expect_err("refused");
        assert!(error.contains("--force"), "the error must mention --force: {error}");
        // The commented file is untouched.
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(text.contains("not an object"), "refusal leaves the file alone");
    }

    #[test]
    fn plain_json_rewrite_needs_no_force() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{\n  \"mcp\": {}\n}\n").expect("write json");

        write_config_set_pub(
            &path,
            &["mcp"],
            &json!({"servers": {}}),
            &json!({"mcp": {"servers": {}}}),
            false,
            false,
        )
        .expect("plain rewrite");
        assert!(std::fs::read_to_string(&path).expect("read back").contains("servers"));
    }

    #[test]
    fn missing_file_rewrite_needs_no_force() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("nested").join("opencode.json");
        write_config_set_pub(&path, &["mcp"], &json!({}), &json!({"mcp": {}}), false, false)
            .expect("creates the config");
        assert!(path.is_file());
    }

    // --- RD-01: `mcp add <name> -- <command>` local stdio servers ----

    fn build(args: &[&str]) -> Result<Value, String> {
        use clap::Parser;
        let parsed = crate::cli::Cli::try_parse_from(
            std::iter::once("opencode").chain(args.iter().copied()),
        )
        .expect("cli parses");
        let Some(crate::cli::Commands::Mcp {
            command: crate::cli::McpCommands::Add { name, command, url, header, env, .. },
        }) = parsed.command
        else {
            panic!("expected mcp add, got {:?}", parsed.command);
        };
        let _ = name;
        build_server(&command, url.as_deref(), header.as_deref(), env.as_deref())
    }

    #[test]
    fn local_server_from_command_after_dash_dash() {
        // RD-01: the previously-dead path — a command without --url must
        // construct {type: "local", command: [...]} instead of erroring.
        let server = build(&["mcp", "add", "example", "--", "npx", "-y", "@example/server"])
            .expect("local server builds");
        assert_eq!(server, json!({"type": "local", "command": ["npx", "-y", "@example/server"]}));
    }

    #[test]
    fn local_server_with_env_and_flagged_command() {
        // The command carries its own flags (trailing_var_arg after `--`)
        // plus an --env pair → mcp/add.ts:31's `{...environment}`.
        let server = build(&[
            "mcp",
            "add",
            "example",
            "--env",
            "API_KEY=secret",
            "--",
            "npx",
            "-y",
            "@example/server",
            "--port",
            "8080",
        ])
        .expect("local server builds");
        assert_eq!(
            server,
            json!({
                "type": "local",
                "command": ["npx", "-y", "@example/server", "--port", "8080"],
                "environment": {"API_KEY": "secret"},
            })
        );
    }

    #[test]
    fn local_server_persists_valid_server_config() {
        // The persisted shape must round-trip through ServerConfig::Local.
        let server = build(&["mcp", "add", "example", "--", "npx", "-y", "@example/server"])
            .expect("builds");
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        write_config_set_pub(
            &path,
            &["mcp", "servers", "example"],
            &server,
            &json!({"mcp": {"servers": {"example": server}}}),
            false,
            false,
        )
        .expect("persist");
        let config: Value =
            json5::from_str(&std::fs::read_to_string(&path).expect("read back")).expect("parses");
        let parsed: oc_schema::mcp::ServerConfig =
            serde_json::from_value(config["mcp"]["servers"]["example"].clone())
                .expect("ServerConfig");
        let oc_schema::mcp::ServerConfig::Local(local) = parsed else {
            panic!("expected local server config");
        };
        assert_eq!(
            local.command,
            vec!["npx".to_owned(), "-y".to_owned(), "@example/server".to_owned()]
        );
    }

    #[test]
    fn add_error_legs() {
        // mcp/add.ts:22-27 error legs, in order.
        assert_eq!(
            build(&["mcp", "add", "x", "--url", "https://example.com", "--", "npx"]).unwrap_err(),
            "Provide either --url <url> or a command after --, not both"
        );
        // Neither url nor command: clap yields an empty command vec.
        assert_eq!(
            build(&["mcp", "add", "x"]).unwrap_err(),
            "Provide either --url <url> or a command after --"
        );
        assert_eq!(
            build(&["mcp", "add", "x", "--url", "not-a-url"]).unwrap_err(),
            "Invalid URL: not-a-url"
        );
        assert_eq!(
            build(&["mcp", "add", "x", "--url", "https://example.com", "--env", "A=B"])
                .unwrap_err(),
            "--env is only valid for local MCP servers"
        );
        assert_eq!(
            build(&["mcp", "add", "x", "--header", "A=B", "--", "npx"]).unwrap_err(),
            "--header is only valid for remote MCP servers"
        );
        assert_eq!(
            build(&["mcp", "add", "x", "--url", "https://example.com", "--header", "A"])
                .unwrap_err(),
            "Invalid header, expected name=value: A"
        );
        assert_eq!(
            build(&["mcp", "add", "x", "--env", "NOEQUALS", "--", "npx"]).unwrap_err(),
            "Invalid env, expected name=value: NOEQUALS"
        );
    }

    #[test]
    fn remote_server_still_builds() {
        let server =
            build(&["mcp", "add", "x", "--url", "https://example.com/sse", "--header", "A=B"])
                .expect("remote builds");
        assert_eq!(
            server,
            json!({"type": "remote", "url": "https://example.com/sse", "headers": {"A": "B"}})
        );
    }
}
