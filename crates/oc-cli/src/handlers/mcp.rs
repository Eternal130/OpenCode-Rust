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
        a.get("name").and_then(Value::as_str).cmp(&b.get("name").and_then(Value::as_str))
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
    // Strip // and /* */ comments so JSONC configs survive the parse (the
    // rewrite itself is guarded by write_config_json_pub's JSONC refusal
    // unless --force is passed — REGISTERED divergence from v2's jsonc
    // applyEdits).
    let mut cleaned = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escape = false;
    let mut line_comment = false;
    let mut block_comment = false;
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        let current = bytes[index];
        let next = bytes.get(index + 1).copied();
        if line_comment {
            if current == '\n' {
                line_comment = false;
                cleaned.push('\n');
            }
            index += 1;
            continue;
        }
        if block_comment {
            if current == '*' && next == Some('/') {
                block_comment = false;
                index += 2;
                continue;
            }
            index += 1;
            continue;
        }
        if in_string {
            cleaned.push(current);
            if escape {
                escape = false;
            } else if current == '\\' {
                escape = true;
            } else if current == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        match current {
            '"' => {
                in_string = true;
                cleaned.push('"');
            }
            '/' if next == Some('/') => line_comment = true,
            '/' if next == Some('*') => block_comment = true,
            _ => cleaned.push(current),
        }
        index += 1;
    }
    serde_json::from_str(&cleaned)
        .map_err(|error| format!("Invalid configuration {}: {error}", path.display()))
}

/// Naive JSONC comment scan: any `//` or `/*` outside a string literal
/// (gate-7 F5). Used to refuse rewrites that would strip user comments.
fn jsonc_has_comments(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut in_string = false;
    let mut escape = false;
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        if in_string {
            if escape {
                escape = false;
            } else if current == '\\' {
                escape = true;
            } else if current == '"' {
                in_string = false;
            }
        } else {
            match current {
                '"' => in_string = true,
                '/' if next == Some('/') || next == Some('*') => return true,
                _ => {}
            }
        }
        index += 1;
    }
    false
}

fn write_config_json(path: &std::path::Path, config: &Value, force: bool) -> Result<(), String> {
    write_config_json_pub(path, config, force)
}

/// Public re-export for the plugin config editor.
///
/// Gate-7 F5: when the target file contains JSONC comments and `force` is
/// false, REFUSE the rewrite — pretty-printing the parsed value would
/// silently delete the user's comments (v2 preserves them via jsonc
/// applyEdits, which the Rust port does not implement). `--force` opts into
/// the stripping rewrite.
pub(crate) fn write_config_json_pub(
    path: &std::path::Path,
    config: &Value,
    force: bool,
) -> Result<(), String> {
    if !force {
        if let Ok(existing) = std::fs::read_to_string(path) {
            if jsonc_has_comments(&existing) {
                return Err(format!(
                    "{} contains JSONC comments; this command's rewrite would strip them. \
                     Re-run with --force to rewrite anyway (comments are lost) or edit the file manually",
                    path.display()
                ));
            }
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut text = serde_json::to_string_pretty(config).expect("config serializes");
    text.push('\n');
    std::fs::write(path, text).map_err(|error| error.to_string())
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
    let has_command = !command.is_empty();
    if url.is_some() && has_command {
        eprintln!("Error: Provide either --url <url> or a command after --, not both");
        return 1;
    }
    if url.is_none() && !has_command {
        eprintln!("Error: Provide either --url <url> or a command after --");
        return 1;
    }
    let Some(url) = url else {
        eprintln!("Error: Provide either --url <url> or a command after --");
        return 1;
    };
    if reqwest::Url::parse(url).is_err() {
        eprintln!("Error: Invalid URL: {url}");
        return 1;
    }
    if env.is_some() {
        eprintln!("Error: --env is only valid for local MCP servers");
        return 1;
    }
    if has_command && header.is_some() {
        eprintln!("Error: --header is only valid for remote MCP servers");
        return 1;
    }
    let mut server = json!({ "type": "remote", "url": url });
    if let Some(header) = header {
        let Some((header_name, header_value)) = header.split_once('=') else {
            eprintln!("Error: Invalid header, expected name=value: {header}");
            return 1;
        };
        server
            .as_object_mut()
            .unwrap()
            .insert("headers".to_owned(), json!({ header_name: header_value }));
    }
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
    if let Err(message) = write_config_json(&config_path, &config, force) {
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
    let integrations = match client
        .integration()
        .list(&oc_client::types::IntegrationListQuery { location: Some(location_query()) })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let Some(integration) = integrations.data.iter().find(|item| &item.id == integration_id) else {
        eprintln!("Error: MCP server \"{name}\" is not an OAuth-capable remote server");
        return 1;
    };
    let integration_value = serde_json::to_value(integration).expect("integration serializes");
    let method = integration_value
        .get("methods")
        .and_then(Value::as_array)
        .and_then(|methods| {
            methods
                .iter()
                .find(|method| method.get("type").and_then(Value::as_str) == Some("oauth"))
        })
        .cloned();
    let Some(method) = method else {
        eprintln!("Error: MCP server \"{name}\" is not an OAuth-capable remote server");
        return 1;
    };
    // Re-authentication replaces the previous sign-in (mcp/auth.ts).
    let previous: Vec<String> = integration
        .connections
        .iter()
        .filter(|connection| {
            matches!(connection, oc_client::types::ConnectionInfo::CredentialInfo(_))
        })
        .filter_map(|connection| match connection {
            oc_client::types::ConnectionInfo::CredentialInfo(credential) => {
                Some(credential.id.clone())
            }
            oc_client::types::ConnectionInfo::EnvInfo(_) => None,
        })
        .collect();
    let label = integration.connections.iter().find_map(|connection| match connection {
        oc_client::types::ConnectionInfo::CredentialInfo(credential) => {
            Some(credential.label.clone())
        }
        oc_client::types::ConnectionInfo::EnvInfo(_) => None,
    });
    match crate::handlers::auth::oauth_login(
        &client,
        integration_id,
        &integration.name,
        &method,
        None,
        label,
    )
    .await
    {
        Ok(()) => {
            for credential_id in previous {
                let _ = client.credential().remove(&credential_id).await;
            }
            println!("└  Done");
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
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
        println!("No stored credentials for {name}");
        return 0;
    };
    let Some(integration_id) = &server.integration_id else {
        println!("No stored credentials for {name}");
        return 0;
    };
    let integrations = match client
        .integration()
        .list(&oc_client::types::IntegrationListQuery { location: Some(location_query()) })
        .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let Some(integration) = integrations.data.iter().find(|item| &item.id == integration_id) else {
        println!("No stored credentials for {name}");
        return 0;
    };
    let credentials: Vec<&oc_client::types::ConnectionCredentialInfo> = integration
        .connections
        .iter()
        .filter_map(|connection| match connection {
            oc_client::types::ConnectionInfo::CredentialInfo(credential) => Some(credential),
            oc_client::types::ConnectionInfo::EnvInfo(_) => None,
        })
        .collect();
    if credentials.is_empty() {
        println!("No stored credentials for {name}");
        return 0;
    }
    for credential in credentials {
        if let Err(error) = client.credential().remove(&credential.id).await {
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
    fn comment_scan_ignores_strings_and_urls() {
        assert!(jsonc_has_comments("// line comment\n{}"));
        assert!(jsonc_has_comments("{ /* block */ }"));
        assert!(!jsonc_has_comments("{\"url\": \"https://example.com/x//y\"}"));
        assert!(!jsonc_has_comments("{\"a\": 1}"));
        assert!(!jsonc_has_comments(""));
    }

    #[test]
    fn jsonc_rewrite_is_refused_without_force() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{\n  // my carefully annotated config\n  \"mcp\": {}\n}\n")
            .expect("write jsonc");

        let error = write_config_json(&path, &json!({"mcp": {}}), false).expect_err("refused");
        assert!(error.contains("JSONC comments"), "{error}");
        assert!(error.contains("--force"), "the error must mention --force: {error}");
        // The commented file is untouched.
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(
            text.contains("my carefully annotated config"),
            "comments must survive the refusal"
        );
    }

    #[test]
    fn jsonc_rewrite_with_force_strips_comments() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{\n  /* notes */\n  \"mcp\": {}\n}\n").expect("write jsonc");

        write_config_json(&path, &json!({"mcp": {"servers": {}}}), true).expect("forced rewrite");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert!(!text.contains("notes"), "force proceeds with the stripping rewrite");
        assert!(text.contains("servers"));
    }

    #[test]
    fn plain_json_rewrite_needs_no_force() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("opencode.json");
        std::fs::write(&path, "{\n  \"mcp\": {}\n}\n").expect("write json");

        write_config_json(&path, &json!({"mcp": {"servers": {}}}), false).expect("plain rewrite");
        assert!(std::fs::read_to_string(&path).expect("read back").contains("servers"));
    }

    #[test]
    fn missing_file_rewrite_needs_no_force() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let path = dir.path().join("nested").join("opencode.json");
        write_config_json(&path, &json!({"mcp": {}}), false).expect("creates the config");
        assert!(path.is_file());
    }
}
