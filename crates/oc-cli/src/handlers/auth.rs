//! `opencode auth *` — integrations + credentials against the server
//! (handlers/auth/*).

use dialoguer::{Input, Password, Select};
use serde_json::Value;

use crate::cli::{AuthCommands, OutputFormat, ServerParams};
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

/// `util/open.ts openUrl` + `ui/prompt.ts openUrl` (login.ts:177): open an
/// http(s) URL with the platform browser opener. Plain subprocess spawn —
/// non-fatal on failure (REF pipes through `Effect.ignore`); the URL itself
/// is always printed first as the manual fallback.
pub(crate) fn open_url(url: &str) {
    // Only http and https links can be opened in the browser (open.ts).
    let Some((scheme, _rest)) = url.split_once("://") else { return };
    if !matches!(scheme, "http" | "https") {
        return;
    }
    // The `open` npm package's platform targets: xdg-open on Linux, `open`
    // on macOS, `cmd /c start` on Windows (empty title arg first so quoted
    // URLs are not swallowed as the console title).
    let (program, prefix): (&str, Vec<&str>) = match std::env::consts::OS {
        "macos" => ("open", Vec::new()),
        "windows" => ("cmd", vec!["/c", "start", ""]),
        _ => ("xdg-open", Vec::new()),
    };
    let _ = std::process::Command::new(program)
        .args(&prefix)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub(crate) fn require_interactive(message: &str) -> Result<(), String> {
    if util::stdin_is_tty() && util::stdout_is_tty() {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

pub async fn run(command: &AuthCommands, server: &ServerParams) -> i32 {
    match command {
        AuthCommands::List { format } => list(server, *format).await,
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

async fn list(server: &ServerParams, format: OutputFormat) -> i32 {
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
    if format == OutputFormat::Json {
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

/// G-150 note: answers now carry the wire `FormAnswer` built by
/// [`crate::handlers::auth_form::answer_form`] (typed values, not
/// stringified key=value pairs).

#[allow(clippy::too_many_lines)]
async fn login(
    server: &ServerParams,
    target: Option<&str>,
    method: Option<&str>,
    answers: &[String],
) -> i32 {
    if target.is_none() {
        // ref auth/login.ts `requireInteractive` — selector only on a TTY.
        if let Err(message) = require_interactive(
            "Pass an integration ID or name when running without an interactive terminal",
        ) {
            eprintln!("Error: {message}");
            return 1;
        }
    }
    println!("◆ Connect an integration");
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();

    // Well-known provider URL discovery.
    if let Some(target) = target {
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
    }
    let integrations = match load_integrations(&client).await {
        Ok(integrations) => integrations,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let integration = if let Some(target) = target {
        match resolve_integration(&integrations, target) {
            Ok(integration) => integration,
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        }
    } else {
        // ref findIntegration: no target → interactive selector
        // (auth/login.ts:46-110 `loginChoices` + `selectIntegration`).
        let choices = login_choices(&integrations);
        if choices.is_empty() {
            eprintln!("Error: No authentication integrations are available");
            return 1;
        }
        let items: Vec<String> =
            choices.iter().map(|choice| choice.display()).collect::<Vec<_>>().to_vec();
        println!("◇ Select integration");
        match Select::new()
            .with_prompt("Select integration")
            .items(&items)
            .default(0)
            .max_length(10)
            .interact_opt()
        {
            Ok(Some(index)) => {
                let id = choices[index].id.clone();
                match resolve_integration(&integrations, &id) {
                    Ok(integration) => integration,
                    Err(message) => {
                        eprintln!("{message}");
                        return 1;
                    }
                }
            }
            Ok(None) => {
                println!("Cancelled");
                return 1;
            }
            Err(error) => {
                eprintln!("Error: {error}");
                return 1;
            }
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
    // G-150: the full form surface (ref answerForm) — schema-driven
    // validation of --answer values (unknown/inactive field rejection,
    // JSON values for boolean/numeric fields, `when` conditional
    // activation) plus interactive prompting for missing answers.
    let answer = if kind == "command" {
        None
    } else {
        let form = method.get("form").and_then(Value::as_array).map(Vec::as_slice);
        match crate::handlers::auth_form::answer_form(form, answers) {
            Ok(answer) => answer,
            Err(message) => {
                eprintln!("Error: {message}");
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
    answer: Option<&oc_client::types::FormAnswer>,
) -> Option<oc_client::types::FormAnswer> {
    answer.cloned()
}

async fn key_login(
    client: &oc_client::Client,
    integration_id: &str,
    integration_name: &str,
    answer: Option<&oc_client::types::FormAnswer>,
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
    answer: Option<&oc_client::types::FormAnswer>,
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
        // login.ts:177 — open the browser when both ends are interactive.
        if util::stdin_is_tty() && util::stdout_is_tty() {
            open_url(url);
        }
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
    // G-150 (ref chooseIntegration/chooseCredential): both halves get
    // TTY selectors when the target/credential is omitted — only a
    // NON-interactive terminal hard-errors.
    let resolved = connect(server, None).await.map_err(|_| String::new())?;
    let client = resolved.endpoint.client();
    let integrations = load_integrations(&client).await?;
    let integration = match target {
        Some(target) => resolve_integration(&integrations, target)?,
        None => {
            require_interactive(
                "Pass an integration ID or name when running without an interactive terminal",
            )?;
            choose_integration(&integrations, "use")?
        }
    };
    if credential.is_none() {
        require_interactive(
            "Pass a credential ID or label when running without an interactive terminal",
        )?;
    }
    let credential_id = choose_credential(&integration, "use", credential)?;
    Ok((client, integration, credential_id))
}

/// G-150 — the TTY integration selector for logout/switch (ref
/// `chooseIntegration` over the connected integrations; the G-61
/// `loginChoices` pattern restricted to rows with credentials).
fn choose_integration(integrations: &[Value], action: &str) -> Result<Value, String> {
    let choices: Vec<&Value> = integrations
        .iter()
        .filter(|integration| {
            integration
                .get("connections")
                .and_then(Value::as_array)
                .is_some_and(|connections| !connections.is_empty())
        })
        .collect();
    if choices.is_empty() {
        return Err("No connected integrations found".to_owned());
    }
    let labels: Vec<String> = choices
        .iter()
        .map(|integration| {
            integration.get("name").and_then(Value::as_str).unwrap_or_default().to_owned()
        })
        .collect();
    match Select::new()
        .with_prompt(format!("Select integration to {action}"))
        .items(&labels)
        .default(0)
        .interact_opt()
    {
        Ok(Some(index)) => Ok(choices[index].clone()),
        _ => Err("Cancelled".to_owned()),
    }
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
        // G-149 (ref auth/import.ts:47-50 `isConflictError`): ONLY a
        // conflict (HTTP 409) counts as "already present" — any other
        // failure (network, 5xx, validation) is a HARD error so a
        // partial import is detectable.
        match client.credential().create(&body).await {
            Ok(_) => imported += 1,
            Err(error) if error.status() == Some(409) => {}
            Err(error) => {
                eprintln!("{}", error_message(&error.to_string()));
                return 1;
            }
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

/// A selectable login integration (ref auth/login.ts `loginChoices` +
/// ui/integration-picker.ts `IntegrationChoice`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct LoginChoice {
    id: String,
    label: String,
    category: &'static str,
    connected: bool,
}

impl LoginChoice {
    /// Selector row (integration-picker.ts): `label ✓` with a connected
    /// marker; category headings are conveyed by sort order.
    fn display(&self) -> String {
        if self.connected {
            format!("{} ✓", self.label)
        } else {
            self.label.clone()
        }
    }
}

/// `integrationPriority` (login.ts:26-33): MCP-source integrations first,
/// then well-known providers, then the rest (name, then id, as tiebreak).
const LOGIN_PRIORITY: [(&str, usize); 6] = [
    ("opencode-go", 0),
    ("opencode", 1),
    ("openai", 2),
    ("github-copilot", 3),
    ("anthropic", 4),
    ("google", 5),
];

/// `loginChoices` (ref auth/login.ts:46-110): integrations with at least one
/// interactive method, sorted MCP → Popular → Services, with connected
/// markers derived from existing connections.
fn login_choices(integrations: &[Value]) -> Vec<LoginChoice> {
    let mut entries: Vec<((usize, usize), LoginChoice)> = integrations
        .iter()
        .filter(|integration| !connect_methods(integration).is_empty())
        .map(|integration| {
            let id = integration.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            let label =
                integration.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
            let is_mcp = integration
                .get("metadata")
                .and_then(|metadata| metadata.get("source"))
                .and_then(Value::as_str)
                == Some("mcp");
            let priority = LOGIN_PRIORITY
                .iter()
                .find(|(key, _)| *key == id)
                .map_or(LOGIN_PRIORITY.len(), |&(_, priority)| priority);
            let popular = priority < LOGIN_PRIORITY.len();
            let category = if is_mcp {
                "MCP"
            } else if popular {
                "Popular"
            } else {
                "Services"
            };
            let rank = if is_mcp { 0 } else { 1 };
            let connected = integration
                .get("connections")
                .and_then(Value::as_array)
                .is_some_and(|connections| !connections.is_empty());
            ((rank, priority), LoginChoice { id, label, category, connected })
        })
        .collect();
    entries.sort_by(|left, right| {
        let ((left_rank, left_priority), left) = left;
        let ((right_rank, right_priority), right) = right;
        left_rank
            .cmp(right_rank)
            .then(left_priority.cmp(right_priority))
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    entries.into_iter().map(|(_, choice)| choice).collect()
}

#[cfg(test)]
mod tests {
    use super::{connect_methods, login_choices, open_url, require_interactive, util};
    use serde_json::json;
    use std::path::PathBuf;

    /// `openUrl` spawns the platform opener (linux: xdg-open) with the URL
    /// as argv[1]. A stub opener on PATH records what it received; the
    /// nextest process isolation makes the PATH rewrite safe.
    #[test]
    #[cfg(target_os = "linux")]
    fn open_url_spawns_the_platform_opener_with_the_url() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir"); // dev-dependency of the crate
        let log = dir.path().join("urls.log");
        let script = format!("printf '%s\\n' \"$1\" >> {}\nexit 0\n", log.display());
        let opener = dir.path().join("xdg-open");
        std::fs::write(&opener, format!("#!/bin/sh\n{script}")).expect("write opener");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&opener).expect("stat").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&opener, perms).expect("chmod");
        }
        let mut path: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        path.insert(0, dir.path().to_path_buf());
        std::env::set_var("PATH", std::env::join_paths(path).expect("join"));

        open_url("https://auth.example.com/oauth/authorize?x=1");
        open_url("http://auth.example.com/start");
        // Give the spawned openers a moment to append, then verify.
        for _ in 0..50 {
            if std::fs::read_to_string(&log).is_ok_and(|text| text.lines().count() >= 2) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let logged = std::fs::read_to_string(&log).expect("opener ran");
        assert!(logged.contains("https://auth.example.com/oauth/authorize?x=1"), "{logged}");
        assert!(logged.contains("http://auth.example.com/start"), "{logged}");
    }

    /// open.ts: only http/https links are opened; anything else is
    /// rejected without spawning (nothing new lands in the log).
    #[test]
    #[cfg(target_os = "linux")]
    fn open_url_rejects_non_http_links() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let log = dir.path().join("reject.log");
        let script = format!("printf '%s\\n' \"$1\" >> {}\nexit 0\n", log.display());
        let opener = dir.path().join("xdg-open");
        std::fs::write(&opener, format!("#!/bin/sh\n{script}")).expect("write opener");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&opener).expect("stat").permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&opener, perms).expect("chmod");
        }
        let mut path: Vec<PathBuf> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        path.insert(0, dir.path().to_path_buf());
        std::env::set_var("PATH", std::env::join_paths(path).expect("join"));

        open_url("file:///etc/passwd");
        open_url("javascript:alert(1)");
        open_url("not a url");
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!log.exists(), "no opener spawn for non-http links");
    }

    fn integration(
        id: &str,
        name: &str,
        methods: usize,
        source: Option<&str>,
        connected: bool,
    ) -> serde_json::Value {
        let mut value = json!({
            "id": id,
            "name": name,
            "methods": (0..methods)
                .map(|index| json!({ "id": format!("m{index}"), "type": "oauth", "label": format!("method {index}") }))
                .collect::<Vec<_>>(),
            "connections": if connected { json!([{ "type": "credential" }]) } else { json!([]) },
        });
        if let Some(source) = source {
            value["metadata"] = json!({ "source": source });
        }
        value
    }

    /// `loginChoices` (login.ts:46-110): MCP first, then priority order,
    /// then name; env-only integrations are excluded; connected markers
    /// derive from connections.
    #[test]
    fn login_choices_sorts_mcp_then_popular_then_services() {
        let integrations = vec![
            integration("zeta", "Zeta", 1, None, false),
            integration("anthropic", "Anthropic", 1, None, true),
            integration("custom-mcp", "Custom MCP", 1, Some("mcp"), false),
            integration("openai", "OpenAI", 1, None, false),
            integration("opencode", "OpenCode", 1, None, false),
            integration("env-only", "Env Only", 0, None, false),
        ];
        let choices = login_choices(&integrations);
        let ids: Vec<&str> = choices.iter().map(|choice| choice.id.as_str()).collect();
        assert_eq!(ids, vec!["custom-mcp", "opencode", "openai", "anthropic", "zeta"]);
        assert_eq!(choices[0].category, "MCP");
        assert_eq!(choices[1].category, "Popular");
        assert_eq!(choices[4].category, "Services");
        assert!(choices.iter().all(|choice| choice.id != "env-only"));
    }

    /// Connected integrations render the `✓` marker (integration-picker.ts).
    #[test]
    fn login_choice_display_marks_connected() {
        let connected = super::LoginChoice {
            id: "id".into(),
            label: "Anthropic".into(),
            category: "Popular",
            connected: true,
        };
        assert_eq!(connected.display(), "Anthropic ✓");
        let disconnected = super::LoginChoice { connected: false, ..connected };
        assert_eq!(disconnected.display(), "Anthropic");
    }

    /// `connectMethods` parity sanity for the selector filter: env methods
    /// are excluded, key sorts last.
    #[test]
    fn connect_methods_excludes_env_and_orders_key_last() {
        let value = json!({
            "methods": [
                { "id": "key", "type": "key", "label": "API key" },
                { "id": "env", "type": "env" },
                { "id": "oauth", "type": "oauth", "label": "OAuth" },
            ]
        });
        let methods = connect_methods(&value);
        let kinds: Vec<&str> =
            methods.iter().map(|method| method["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["oauth", "key"]);
    }

    /// `require_interactive` is the non-TTY gate for `auth login` without a
    /// target (login.ts `requireInteractive`): under the plain nextest
    /// profile stdin/stdout are pipes (gated), under the PTY profile they
    /// are TTYs (allowed) — assert whichever environment we run in.
    #[test]
    fn require_interactive_gates_non_tty_login() {
        let tty = util::stdin_is_tty() && util::stdout_is_tty();
        let gated = super::require_interactive(
            "Pass an integration ID or name when running without an interactive terminal",
        );
        assert_eq!(gated.is_ok(), tty);
    }
}
