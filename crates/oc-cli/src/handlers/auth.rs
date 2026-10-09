//! `opencode auth *` — integrations + credentials against the server
//! (handlers/auth/*).

use dialoguer::{Input, Password, Select};
use serde_json::Value;

use crate::cli::{AuthCommands, ServerParams};
use crate::handlers::connect;
use crate::util;
use crate::util::error_message;

fn location_query() -> oc_client::types::QueryLocation {
    oc_client::types::QueryLocation {
        directory: Some(
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        ),
    }
}

/// `loadIntegrations`: model.default is the readiness boundary, then
/// integration.list sorted with `opencode-go` first.
async fn load_integrations(client: &oc_client::Client) -> Result<Vec<Value>, String> {
    let _ = client
        .model()
        .default(&oc_client::types::ModelDefaultQuery { location: Some(location_query()) })
        .await
        .map_err(|error| error_message(&error.to_string()))?;
    let integrations = client
        .integration()
        .list(&oc_client::types::IntegrationListQuery { location: Some(location_query()) })
        .await
        .map_err(|error| error_message(&error.to_string()))?;
    let mut values: Vec<Value> = integrations
        .data
        .iter()
        .map(|integration| serde_json::to_value(integration).expect("integration serializes"))
        .collect();
    values.sort_by_key(|integration| {
        let id = integration.get("id").and_then(Value::as_str).unwrap_or_default();
        std::cmp::Reverse(id == "opencode-go")
    });
    Ok(values)
}

/// `resolveIntegration`: by id, else unique case-insensitive name.
fn resolve_integration(integrations: &[Value], target: &str) -> Result<Value, String> {
    let normalized = target.trim_end_matches('/');
    if let Some(found) = integrations
        .iter()
        .find(|integration| integration.get("id").and_then(Value::as_str) == Some(normalized))
    {
        return Ok(found.clone());
    }
    let matches: Vec<&Value> = integrations
        .iter()
        .filter(|integration| {
            integration
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case(normalized))
        })
        .collect();
    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err(format!("Integration not found: {target}")),
        _ => {
            let ids: Vec<String> = matches
                .iter()
                .filter_map(|integration| {
                    integration.get("id").and_then(Value::as_str).map(str::to_owned)
                })
                .collect();
            Err(format!("Integration name \"{target}\" is ambiguous: {}", ids.join(", ")))
        }
    }
}

/// `connectMethods`: non-env methods, key last.
fn connect_methods(integration: &Value) -> Vec<Value> {
    let mut methods: Vec<Value> = integration
        .get("methods")
        .and_then(Value::as_array)
        .map(|methods| {
            methods
                .iter()
                .filter(|method| method.get("type").and_then(Value::as_str) != Some("env"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    methods.sort_by_key(|method| method.get("type").and_then(Value::as_str) == Some("key"));
    methods
}

/// `resolveMethod`: by id or label ("key" for key methods).
fn resolve_method<'a>(methods: &'a [Value], target: &str) -> Result<&'a Value, String> {
    let normalized = target.to_ascii_lowercase();
    let matches: Vec<&Value> = methods
        .iter()
        .filter(|method| {
            let kind = method.get("type").and_then(Value::as_str).unwrap_or_default();
            if kind == "key" {
                normalized == "key"
                    || method
                        .get("label")
                        .and_then(Value::as_str)
                        .is_some_and(|label| label.to_ascii_lowercase() == normalized)
            } else {
                method.get("id").and_then(Value::as_str) == Some(target)
                    || method
                        .get("label")
                        .and_then(Value::as_str)
                        .is_some_and(|label| label.to_ascii_lowercase() == normalized)
            }
        })
        .collect();
    if matches.len() == 1 {
        Ok(matches[0])
    } else {
        let available: Vec<String> = methods
            .iter()
            .map(|method| {
                let kind = method.get("type").and_then(Value::as_str).unwrap_or_default();
                if kind == "key" {
                    "key".to_owned()
                } else {
                    method.get("id").and_then(Value::as_str).unwrap_or_default().to_owned()
                }
            })
            .collect();
        Err(format!(
            "Authentication method not found: {target}{}. Available: {}",
            if matches.len() > 1 { " is ambiguous" } else { "" },
            available.join(", ")
        ))
    }
}

fn require_interactive(message: &str) -> Result<(), String> {
    if util::stdin_is_tty() && util::stdout_is_tty() {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

pub async fn run(command: &AuthCommands, server: &ServerParams) -> i32 {
    match command {
        AuthCommands::List { format } => list(server, format).await,
        AuthCommands::Login { target, method, answer } => {
            login(server, target.as_deref(), method.as_deref(), answer).await
        }
        AuthCommands::Logout { target, credential } => {
            logout(server, target.as_deref(), credential.as_deref()).await
        }
        AuthCommands::Export { target } => export(server, target.as_deref()).await,
        AuthCommands::Import { file } => import(server, file.as_deref()).await,
        AuthCommands::Switch { target, credential } => {
            switch(server, target.as_deref(), credential.as_deref()).await
        }
    }
}

async fn list(server: &ServerParams, format: &str) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let integrations = match load_integrations(&client).await {
        Ok(integrations) => integrations,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let integrations: Vec<Value> = integrations
        .into_iter()
        .filter(|integration| {
            integration
                .get("connections")
                .and_then(Value::as_array)
                .is_some_and(|connections| !connections.is_empty())
        })
        .collect();
    if format == "json" {
        let rows: Vec<Value> = integrations
            .iter()
            .map(|integration| {
                serde_json::json!({
                    "id": integration.get("id"),
                    "name": integration.get("name"),
                    "connections": integration.get("connections"),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).expect("serializes"));
        return 0;
    }
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for integration in &integrations {
        let name = integration.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
        for connection in integration
            .get("connections")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let kind = connection.get("type").and_then(Value::as_str).unwrap_or_default();
            rows.push((
                name.clone(),
                if kind == "credential" {
                    connection.get("label").and_then(Value::as_str).unwrap_or_default().to_owned()
                } else {
                    connection.get("name").and_then(Value::as_str).unwrap_or_default().to_owned()
                },
                if kind == "credential" { "stored".to_owned() } else { "environment".to_owned() },
            ));
        }
    }
    if rows.is_empty() {
        println!("No authenticated integrations");
        return 0;
    }
    let width = rows.iter().map(|row| row.0.len()).max().unwrap_or(0) + 2;
    for (integration, source, kind) in &rows {
        println!("{integration:<width$}{source:<28}{kind}");
    }
    0
}

fn parse_answers(answers: &[String]) -> Result<Option<serde_json::Map<String, Value>>, String> {
    if answers.is_empty() {
        return Ok(None);
    }
    let mut map = serde_json::Map::new();
    for answer in answers {
        let Some((key, value)) = answer.split_once('=') else {
            return Err(format!("Invalid answer, expected name=value: {answer}"));
        };
        map.insert(key.to_owned(), Value::String(value.to_owned()));
    }
    Ok(Some(map))
}

#[allow(clippy::too_many_lines)]
async fn login(
    server: &ServerParams,
    target: Option<&str>,
    method: Option<&str>,
    answers: &[String],
) -> i32 {
    let Some(target) = target else {
        eprintln!(
            "Error: Pass an integration ID or name when running without an interactive terminal"
        );
        return 1;
    };
    println!("◆ Connect an integration");
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();

    // Well-known provider URL discovery.
    if target.starts_with("http://") || target.starts_with("https://") {
        if let Err(error) = client
            .integration()
            .wellknown_add(
                &oc_client::types::ExperimentalIntegrationWellknownAddQuery {
                    location: Some(location_query()),
                },
                &oc_client::types::ExperimentalIntegrationWellknownAddBody {
                    url: target.to_owned(),
                },
            )
            .await
        {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    }
    let integrations = match load_integrations(&client).await {
        Ok(integrations) => integrations,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let integration = match resolve_integration(&integrations, target) {
        Ok(integration) => integration,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let integration_name =
        integration.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
    let integration_id =
        integration.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    let methods = connect_methods(&integration);
    if methods.is_empty() {
        eprintln!("Error: {integration_name} has no interactive login methods");
        return 1;
    }
    let method = match method {
        Some(target) => match resolve_method(&methods, target) {
            Ok(method) => method.clone(),
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        },
        None => {
            if methods.len() == 1 {
                methods[0].clone()
            } else {
                if let Err(message) = require_interactive(
                    "Pass --method when running without an interactive terminal",
                ) {
                    eprintln!("{message}");
                    return 1;
                }
                let labels: Vec<String> = methods
                    .iter()
                    .map(|method| {
                        let kind = method.get("type").and_then(Value::as_str).unwrap_or_default();
                        let label = method.get("label").and_then(Value::as_str).unwrap_or_default();
                        if kind == "key" {
                            "API key".to_owned()
                        } else {
                            label.to_owned()
                        }
                    })
                    .collect();
                if let Ok(Some(index)) = Select::new()
                    .with_prompt("Select login method")
                    .items(&labels)
                    .default(0)
                    .interact_opt()
                {
                    methods[index].clone()
                } else {
                    println!("Cancelled");
                    return 0;
                }
            }
        }
    };
    let kind = method.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
    let answer = if kind == "command" {
        None
    } else {
        match parse_answers(answers) {
            Ok(answer) => answer,
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        }
    };
    let outcome = match kind.as_str() {
        "key" => key_login(&client, &integration_id, &integration_name, answer.as_ref()).await,
        "command" => command_login(&client, &integration_id, &integration_name, &method).await,
        _ => {
            oauth_login(&client, &integration_id, &integration_name, &method, answer.as_ref(), None)
                .await
        }
    };
    match outcome {
        Ok(()) => {
            println!("└  Done");
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

fn form_answer(
    answer: Option<&serde_json::Map<String, Value>>,
) -> Option<oc_client::types::FormAnswer> {
    let map = answer?;
    Some(
        map.iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    oc_client::types::FormValue::String(
                        value.as_str().unwrap_or_default().to_owned(),
                    ),
                )
            })
            .collect(),
    )
}

async fn key_login(
    client: &oc_client::Client,
    integration_id: &str,
    integration_name: &str,
    answer: Option<&serde_json::Map<String, Value>>,
) -> Result<(), String> {
    require_interactive("This login requires an interactive terminal")?;
    let key = Password::new()
        .with_prompt(format!("Enter your {integration_name} API key"))
        .interact()
        .map_err(|error| error.to_string())?;
    println!("◇ Saving credential...");
    client
        .integration()
        .connect()
        .key(
            integration_id,
            &oc_client::types::IntegrationConnectKeyQuery { location: Some(location_query()) },
            &oc_client::types::IntegrationConnectKeyBody {
                key,
                answer: form_answer(answer),
                label: None,
            },
        )
        .await
        .map_err(|error| error_message(&error.to_string()))
}

pub(crate) async fn oauth_login(
    client: &oc_client::Client,
    integration_id: &str,
    integration_name: &str,
    method: &Value,
    answer: Option<&serde_json::Map<String, Value>>,
    label: Option<String>,
) -> Result<(), String> {
    let method_id = method.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    println!("◇ Starting authorization...");
    let started = client
        .integration()
        .oauth()
        .connect(
            integration_id,
            &oc_client::types::IntegrationOauthConnectQuery { location: Some(location_query()) },
            &oc_client::types::IntegrationOauthConnectBody {
                answer: form_answer(answer),
                method_id,
                label,
            },
        )
        .await
        .map_err(|error| error_message(&error.to_string()))?;
    let attempt = serde_json::to_value(&started.data).expect("attempt serializes");
    let attempt_id =
        attempt.get("attemptID").and_then(Value::as_str).unwrap_or_default().to_owned();
    let cancel = |client: oc_client::Client, integration_id: String, attempt_id: String| async move {
        let _ = client
            .integration()
            .oauth()
            .cancel(
                &integration_id,
                &attempt_id,
                &oc_client::types::IntegrationOauthCancelQuery { location: Some(location_query()) },
            )
            .await;
    };
    println!("│ Authorization started");
    if let Some(instructions) = attempt.get("instructions").and_then(Value::as_str) {
        println!("│ {instructions}");
    }
    if let Some(url) = attempt.get("url").and_then(Value::as_str) {
        println!("│ {url}");
    }
    if attempt.get("mode").and_then(Value::as_str) == Some("code") {
        if let Err(message) = require_interactive(
            "This login requires an interactive terminal to enter the authorization code",
        ) {
            cancel(client.clone(), integration_id.to_owned(), attempt_id.clone()).await;
            return Err(message);
        }
        let code = Input::<String>::new()
            .with_prompt("Paste the authorization code")
            .interact_text()
            .map_err(|error| error.to_string())?;
        println!("◇ Completing authorization...");
        return client
            .integration()
            .oauth()
            .complete(
                integration_id,
                &attempt_id,
                &oc_client::types::IntegrationOauthCompleteQuery {
                    location: Some(location_query()),
                },
                &oc_client::types::IntegrationOauthCompleteBody { code: Some(code) },
            )
            .await
            .map_err(|error| error_message(&error.to_string()));
    }
    println!("│ Waiting for authorization...");
    loop {
        let status = client
            .integration()
            .oauth()
            .status(
                integration_id,
                &attempt_id,
                &oc_client::types::IntegrationOauthStatusQuery { location: Some(location_query()) },
            )
            .await
            .map_err(|error| error_message(&error.to_string()))?;
        let status = serde_json::to_value(&status.data).expect("status serializes");
        let state = status.get("status").and_then(Value::as_str).unwrap_or_default();
        if state != "pending" {
            cancel(client.clone(), integration_id.to_owned(), attempt_id.clone()).await;
            return match state {
                "complete" => {
                    println!("└  Connected to {integration_name}");
                    Ok(())
                }
                "failed" => Err(status
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()),
                _ => Err("Authorization expired".to_owned()),
            };
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

async fn command_login(
    client: &oc_client::Client,
    integration_id: &str,
    integration_name: &str,
    method: &Value,
) -> Result<(), String> {
    let method_id = method.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
    println!("◇ Starting authentication command...");
    println!("│ Waiting for authentication command...");
    let started = client
        .integration()
        .command()
        .connect(
            integration_id,
            &oc_client::types::IntegrationCommandConnectQuery { location: Some(location_query()) },
            &oc_client::types::IntegrationCommandConnectBody { method_id, ..Default::default() },
        )
        .await
        .map_err(|error| error_message(&error.to_string()))?;
    let attempt = serde_json::to_value(&started.data).expect("attempt serializes");
    let attempt_id =
        attempt.get("attemptID").and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut printed = 0usize;
    loop {
        let status = client
            .integration()
            .command()
            .status(
                integration_id,
                &attempt_id,
                &oc_client::types::IntegrationCommandStatusQuery {
                    location: Some(location_query()),
                },
            )
            .await
            .map_err(|error| error_message(&error.to_string()))?;
        let status = serde_json::to_value(&status.data).expect("status serializes");
        let state = status.get("status").and_then(Value::as_str).unwrap_or_default();
        if state != "pending" {
            let _ = client
                .integration()
                .command()
                .cancel(
                    integration_id,
                    &attempt_id,
                    &oc_client::types::IntegrationCommandCancelQuery {
                        location: Some(location_query()),
                    },
                )
                .await;
            return match state {
                "complete" => {
                    println!("└  Connected to {integration_name}");
                    Ok(())
                }
                "failed" => Err(status
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()),
                _ => Err("Authentication expired".to_owned()),
            };
        }
        if let Some(message) = status.get("message").and_then(Value::as_str) {
            if let Some(end) =
                message.rfind('\n').map(|index| index + 1).filter(|end| *end > printed)
            {
                let output = message[printed..end].trim();
                if !output.is_empty() {
                    println!("│ {output}");
                }
                printed = end;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

fn choose_credential(
    integration: &Value,
    action: &str,
    target: Option<&str>,
) -> Result<String, String> {
    let name = integration.get("name").and_then(Value::as_str).unwrap_or_default();
    let credentials: Vec<&Value> = integration
        .get("connections")
        .and_then(Value::as_array)
        .map(|connections| {
            connections
                .iter()
                .filter(|connection| {
                    connection.get("type").and_then(Value::as_str) == Some("credential")
                })
                .collect()
        })
        .unwrap_or_default();
    if credentials.is_empty() {
        let environment: Vec<&str> = integration
            .get("connections")
            .and_then(Value::as_array)
            .map(|connections| {
                connections
                    .iter()
                    .filter(|connection| {
                        connection.get("type").and_then(Value::as_str) == Some("env")
                    })
                    .filter_map(|connection| connection.get("name").and_then(Value::as_str))
                    .collect()
            })
            .unwrap_or_default();
        if !environment.is_empty() && action == "log out" {
            return Err(format!(
                "{name} is authenticated through {}; unset the environment variable to disconnect",
                environment.join(", ")
            ));
        }
        return Err(format!("No stored credentials for {name}"));
    }
    if let Some(target) = target {
        if let Some(found) = credentials
            .iter()
            .find(|credential| credential.get("id").and_then(Value::as_str) == Some(target))
        {
            return Ok(found.get("id").and_then(Value::as_str).unwrap_or_default().to_owned());
        }
        let matches: Vec<&&Value> = credentials
            .iter()
            .filter(|credential| {
                credential
                    .get("label")
                    .and_then(Value::as_str)
                    .is_some_and(|label| label.eq_ignore_ascii_case(target))
            })
            .collect();
        return match matches.len() {
            1 => Ok(matches[0].get("id").and_then(Value::as_str).unwrap_or_default().to_owned()),
            0 => Err(format!("Credential not found for {name}: {target}")),
            _ => {
                let ids: Vec<String> = matches
                    .iter()
                    .filter_map(|credential| {
                        credential.get("id").and_then(Value::as_str).map(str::to_owned)
                    })
                    .collect();
                Err(format!(
                    "Credential label \"{target}\" is ambiguous. Use an ID: {}",
                    ids.join(", ")
                ))
            }
        };
    }
    let labels: Vec<String> = credentials
        .iter()
        .enumerate()
        .map(|(index, credential)| {
            let label = credential.get("label").and_then(Value::as_str).unwrap_or_default();
            if index == 0 {
                format!("{label} (active)")
            } else {
                label.to_owned()
            }
        })
        .collect();
    match Select::new()
        .with_prompt(format!("Select {name} account to {action}"))
        .items(&labels)
        .default(0)
        .interact_opt()
    {
        Ok(Some(index)) => {
            Ok(credentials[index].get("id").and_then(Value::as_str).unwrap_or_default().to_owned())
        }
        _ => Err("Cancelled".to_owned()),
    }
}

async fn resolve_account(
    server: &ServerParams,
    target: Option<&str>,
    credential: Option<&str>,
) -> Result<(oc_client::Client, Value, String), String> {
    let Some(target) = target else {
        return Err("Pass an integration ID or name when running without an interactive terminal"
            .to_owned());
    };
    let Some(credential) = credential else {
        return Err(
            "Pass a credential ID or label when running without an interactive terminal".to_owned()
        );
    };
    let resolved = connect(server, None).await.map_err(|_| String::new())?;
    let client = resolved.endpoint.client();
    let integrations = load_integrations(&client).await?;
    let integration = resolve_integration(&integrations, target)?;
    let credential_id = choose_credential(&integration, "use", Some(credential))?;
    Ok((client, integration, credential_id))
}

async fn logout(server: &ServerParams, target: Option<&str>, credential: Option<&str>) -> i32 {
    println!("◆ Log out of an account");
    let (client, integration, credential_id) =
        match resolve_account(server, target, credential).await {
            Ok(account) => account,
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        };
    let name = integration.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
    println!("◇ Removing credential...");
    if let Err(error) = client.credential().remove(&credential_id).await {
        eprintln!("{}", error_message(&error.to_string()));
        return 1;
    }
    println!("◇ Removed account from {name}");
    println!("└  Done");
    0
}

async fn switch(server: &ServerParams, target: Option<&str>, credential: Option<&str>) -> i32 {
    println!("◆ Switch account");
    let (client, integration, credential_id) =
        match resolve_account(server, target, credential).await {
            Ok(account) => account,
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        };
    let name = integration.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
    println!("◇ Switching account...");
    if let Err(error) = client.credential().activate(&credential_id).await {
        eprintln!("{}", error_message(&error.to_string()));
        return 1;
    }
    println!("◇ Switched account for {name}");
    println!("└  Done");
    0
}

async fn export(server: &ServerParams, target: Option<&str>) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let integration_id = match target {
        Some(target) => {
            let integrations = match load_integrations(&client).await {
                Ok(integrations) => integrations,
                Err(message) => {
                    eprintln!("{message}");
                    return 1;
                }
            };
            match resolve_integration(&integrations, target) {
                Ok(integration) => integration.get("id").and_then(Value::as_str).map(str::to_owned),
                Err(message) => {
                    eprintln!("{message}");
                    return 1;
                }
            }
        }
        None => None,
    };
    let credentials = match client.credential().list().await {
        Ok(credentials) => credentials,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let filtered: Vec<_> = credentials
        .into_iter()
        .filter(|credential| {
            integration_id.as_deref().is_none_or(|id| credential.integration_id == id)
        })
        .collect();
    if util::stdout_is_tty() {
        eprintln!(
            "Warning: the output contains secrets; redirect it to a file or pipe it to auth import"
        );
    }
    println!("{}", serde_json::to_string_pretty(&filtered).expect("serializes"));
    0
}

async fn import(server: &ServerParams, file: Option<&str>) -> i32 {
    let text = if let Some(file) = file {
        match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("Error: Failed to read credentials: {error}");
                return 1;
            }
        }
    } else {
        if util::stdin_is_tty() {
            eprintln!("Error: Pipe auth export output into stdin or pass a file to import");
            return 1;
        }
        match util::read_stdin() {
            Ok(text) => text,
            Err(error) => {
                eprintln!("Error: Failed to read credentials: {error}");
                return 1;
            }
        }
    };
    let credentials: Vec<Value> = if let Ok(credentials) = serde_json::from_str(&text) {
        credentials
    } else {
        eprintln!("Error: Invalid credentials JSON");
        return 1;
    };
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let existing = client.credential().list().await.unwrap_or_default();
    let ids: std::collections::HashSet<String> =
        existing.iter().map(|credential| credential.id.clone()).collect();
    let known_integrations: std::collections::HashSet<String> =
        existing.iter().map(|credential| credential.integration_id.clone()).collect();
    let mut imported = 0usize;
    for credential in &credentials {
        let id = credential.get("id").and_then(Value::as_str).unwrap_or_default();
        if ids.contains(id) {
            continue;
        }
        let integration_id =
            credential.get("integrationID").and_then(Value::as_str).unwrap_or_default().to_owned();
        let body = oc_client::types::CredentialCreateInput {
            id: credential.get("id").and_then(Value::as_str).map(str::to_owned),
            integration_id: integration_id.clone(),
            label: credential.get("label").and_then(Value::as_str).map(str::to_owned),
            value: serde_json::from_value(credential.get("value").cloned().unwrap_or(Value::Null))
                .unwrap_or(oc_client::types::CredentialValue::Key(
                    oc_client::types::CredentialKey::default(),
                )),
            activate: Some(
                credential.get("active").and_then(Value::as_bool).unwrap_or(false)
                    && !known_integrations.contains(&integration_id),
            ),
        };
        if client.credential().create(&body).await.is_ok() {
            imported += 1;
        }
    }
    let skipped = credentials.len().saturating_sub(imported);
    eprintln!(
        "Imported {imported} {}{}",
        if imported == 1 { "credential" } else { "credentials" },
        if skipped > 0 { format!(", skipped {skipped} already present") } else { String::new() }
    );
    0
}
