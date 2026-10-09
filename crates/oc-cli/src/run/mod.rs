//! The `run` command (non-interactive one-shot): port of
//! `cli/src/run/run.ts` (input preparation, session targeting, dispatch)
//! driving [`noninteractive`] for the SSE event loop.

pub mod noninteractive;
pub mod tool_render;

use oc_client::types::{
    LocationPublicInfo, ModelRef, SessionCreateBody, SessionInfo, SessionUpdateBody,
};
use oc_client::Client;

use crate::run::noninteractive::{Format, PromptInput};
use crate::services::server_connection::Resolved;
use crate::util;

/// `RunCommandInput`.
#[allow(clippy::too_many_arguments, clippy::struct_excessive_bools)]
pub struct RunInput {
    pub server: Resolved,
    pub message: Vec<String>,
    pub continue_: bool,
    pub session: Option<String>,
    pub fork: bool,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub format: Format,
    pub file: Vec<String>,
    pub title: Option<String>,
    pub thinking: bool,
    pub auto: bool,
}

type FilePart = noninteractive::PreparedFile;

/// `formatMessage(message)`: quote parts containing spaces, join with spaces.
#[must_use]
pub fn format_message(message: &[String]) -> Option<String> {
    let value = message
        .iter()
        .map(|part| {
            if part.contains(' ') {
                format!("\"{}\"", part.replace('"', "\\\""))
            } else {
                part.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    (!value.is_empty()).then_some(value)
}

/// `mergeInput(message, piped)`.
#[must_use]
pub fn merge_input(message: Option<String>, piped: Option<String>) -> Option<String> {
    match (message, piped) {
        (None, piped) => piped,
        (Some(message), None) => Some(message),
        (Some(message), Some(piped)) => Some(format!("{message}\n{piped}")),
    }
}

/// `parseRunModel` → `Model.Ref.parse` ("provider/model#variant").
#[must_use]
pub fn parse_run_model(value: Option<&str>) -> Option<(String, String, Option<String>)> {
    let value = value?;
    let (reference, variant) = match value.split_once('#') {
        Some((reference, variant)) => (reference, Some(variant.to_owned())),
        None => (value, None),
    };
    let (provider, model) = reference.split_once('/')?;
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider.to_owned(), model.to_owned(), variant))
}

const ATTACH_FILE_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// `prepareFile`: read, cap, and classify an attachment (text files become
/// `<file>` blocks; binary/image/pdf become data URLs).
fn prepare_file(input: &str, directory: &std::path::Path) -> Result<FilePart, String> {
    let file = directory.join(input);
    let metadata = std::fs::metadata(&file).map_err(|_| format!("File not found: {input}"))?;
    if !metadata.is_file() || metadata.len() > ATTACH_FILE_MAX_BYTES {
        return Err(format!(
            "Cannot attach a directory, special file, or file larger than 10 MiB: {input}"
        ));
    }
    let bytes = std::fs::read(&file).map_err(|_| format!("File not found: {input}"))?;
    let filename = file
        .file_name()
        .map_or_else(|| input.to_owned(), |name| name.to_string_lossy().into_owned());
    let detected = detect_mime(&file, &bytes);
    let is_binary = bytes.contains(&0)
        || bytes.iter().filter(|b| **b < 9 || (**b > 13 && **b < 32)).count() * 10
            > bytes.len() * 3;
    let mime = if detected.starts_with("image/") || detected == "application/pdf" {
        detected
    } else if !is_binary && String::from_utf8(bytes.clone()).is_ok() {
        "text/plain".to_owned()
    } else {
        detected
    };
    if mime == "text/plain" {
        let content = String::from_utf8(bytes).expect("checked utf8 above");
        Ok(FilePart::Text { filename, content })
    } else {
        use base64::Engine as _;
        let uri = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        Ok(FilePart::Attachment { filename, uri })
    }
}

fn detect_mime(file: &std::path::Path, bytes: &[u8]) -> String {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "image/png".to_owned();
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "image/jpeg".to_owned();
    }
    if bytes.starts_with(b"GIF8") {
        return "image/gif".to_owned();
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp".to_owned();
    }
    if bytes.starts_with(b"%PDF") {
        return "application/pdf".to_owned();
    }
    match file.extension().and_then(|ext| ext.to_str()) {
        Some("png") => "image/png".to_owned(),
        Some("jpg" | "jpeg") => "image/jpeg".to_owned(),
        Some("gif") => "image/gif".to_owned(),
        Some("webp") => "image/webp".to_owned(),
        Some("pdf") => "application/pdf".to_owned(),
        _ => "application/octet-stream".to_owned(),
    }
}

/// The resolved prompt target (`resolveSessionTarget` output).
struct SessionTarget {
    session: SessionInfo,
    directory: String,
    model: Option<ModelRef>,
    agent: Option<String>,
    resume: bool,
}

fn client_error(error: &oc_client::ClientError) -> String {
    util::error_message(&error.to_string())
}

async fn find_session(client: &Client, session_id: &str) -> Option<SessionInfo> {
    match client.session().get(session_id).await {
        Ok(session) => Some(session),
        Err(error) if error.api_name() == Some("SessionNotFoundError") => None,
        Err(_) => None,
    }
}

async fn resolve_location(client: &Client, directory: &str) -> Result<LocationPublicInfo, String> {
    client
        .location()
        .get(&oc_client::types::LocationGetQuery {
            location: Some(oc_client::types::QueryLocation {
                directory: Some(directory.to_owned()),
            }),
        })
        .await
        .map_err(|error| client_error(&error))
}

async fn latest_session(
    client: &Client,
    directory: &str,
    cursor: Option<String>,
) -> Result<Option<SessionInfo>, String> {
    let page = client
        .session()
        .list(&oc_client::types::SessionListQuery {
            directory: Some(directory.to_owned()),
            parent_id: Some("null".to_owned()),
            limit: Some("50".to_owned()),
            order: Some(oc_client::types::SessionListQueryOrder::Desc),
            cursor,
            ..Default::default()
        })
        .await
        .map_err(|error| client_error(&error))?;
    let next = page.cursor.next.clone();
    if let Some(selected) = page.data.iter().find(|session| session.location.directory == directory)
    {
        return Ok(Some(selected.clone()));
    }
    if next.is_none() || page.data.is_empty() {
        return Ok(None);
    }
    Box::pin(latest_session(client, directory, next)).await
}

async fn resolve_session_target(
    client: &Client,
    input: &RunInput,
    directory: &str,
    environment: Option<std::collections::BTreeMap<String, String>>,
) -> Result<SessionTarget, String> {
    let explicit = match &input.session {
        Some(id) => find_session(client, id).await,
        None => None,
    };
    if input.session.is_some() && explicit.is_none() && input.fork {
        return Err("Session not found".to_owned());
    }
    // A missing --session ID becomes the new session's ID (falls
    // through to creation with the requested id).
    let selected = match explicit {
        Some(session) => {
            if input.fork {
                client
                    .session()
                    .fork(&session.id, &oc_client::types::SessionForkBody { ..Default::default() })
                    .await
                    .map_err(|error| client_error(&error))?
            } else {
                session
            }
        }
        None => {
            if input.continue_ {
                match latest_session(client, directory, None).await? {
                    Some(session) if !input.fork => session,
                    Some(session) => client
                        .session()
                        .fork(
                            &session.id,
                            &oc_client::types::SessionForkBody { ..Default::default() },
                        )
                        .await
                        .map_err(|error| client_error(&error))?,
                    None => SessionInfo::default(),
                }
            } else {
                SessionInfo::default()
            }
        }
    };
    let resume = !selected.id.is_empty();
    let location = if resume {
        resolve_location(client, &selected.location.directory).await?
    } else {
        resolve_location(client, directory).await?
    };

    let model = parse_run_model(input.model.as_deref())
        .map(|(provider_id, id, variant)| ModelRef { provider_id, id, variant });
    // v2's `prepare` only falls back to a default for NEW sessions
    // (`session.create` carries the model); a resumed session keeps
    // `session.model` and a plain run never switches it. Falling back here
    // unconditionally would reset every `run -c`/`run -s` session to the
    // location default (gate-7 F3).
    let model = match model {
        Some(model) => Some(model),
        None if resume => None,
        None => client
            .model()
            .default(&oc_client::types::ModelDefaultQuery {
                location: Some(oc_client::types::QueryLocation {
                    directory: Some(directory.to_owned()),
                }),
            })
            .await
            .ok()
            .and_then(|response| response.data)
            .map(|info| ModelRef { provider_id: info.provider_id, id: info.id, variant: None }),
    };
    let agent = input.agent.clone();

    let session = if resume {
        selected
    } else {
        client
            .session()
            .create(&SessionCreateBody {
                id: input.session.clone(),
                agent: agent.clone(),
                model: model.clone(),
                location: Some(oc_client::types::LocationPublicRef {
                    directory: location.directory.clone(),
                }),
                ..Default::default()
            })
            .await
            .map_err(|error| client_error(&error))?
    };
    if let Some(environment) = environment {
        let _ = client
            .session()
            .environment(
                &session.id,
                &oc_client::types::SessionEnvironmentBody { variables: environment },
            )
            .await;
    }
    Ok(SessionTarget { session, directory: location.directory, model, agent, resume })
}

/// `runNonInteractive` — returns the process exit code.
pub async fn run(input: RunInput) -> i32 {
    match run_inner(input).await {
        Ok(code) => code,
        Err(message) => {
            report_error("", &message);
            1
        }
    }
}

fn report_error(session_id: &str, message: &str) {
    crate::run_ui::error(message);
    let _ = session_id;
}

async fn run_inner(input: RunInput) -> Result<i32, String> {
    if input.fork && !input.continue_ && input.session.is_none() {
        return Err("--fork requires --continue or --session".to_owned());
    }
    // v2 chdirs to `$PWD ?? cwd()` (the shell's logical path) before
    // continuing (`run.ts:73`). Under a shell `$PWD` always names the
    // current directory (the chdir is a logical-path no-op), but a spawned
    // process can inherit a STALE `$PWD` (nextest, CI) that would silently
    // relocate this run — and its standalone child — away from the project
    // config. So: honor `$PWD` only when it already IS the current
    // directory, which makes the physical chdir unnecessary.
    let root = match std::env::var("PWD") {
        Ok(stated)
            if std::env::current_dir().is_ok_and(|current| {
                std::fs::canonicalize(&stated).ok() == current.canonicalize().ok()
            }) =>
        {
            stated
        }
        _ => std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
    };
    let Some(local) = std::env::current_dir().ok() else {
        return Err(format!("Failed to change directory to {root}"));
    };

    let message = format_message(&input.message);
    let piped = if util::stdin_is_tty() { None } else { util::read_stdin().ok() };
    let message = merge_input(message, piped).filter(|text| !text.trim().is_empty());
    let Some(message) = message else {
        return Err("You must provide a message".to_owned());
    };

    let mut files = Vec::new();
    for file in &input.file {
        files.push(prepare_file(file, &local)?);
    }

    let client = input.server.endpoint.client();
    // `Env.session()` rides along only on the managed service connection
    // (v2 gates on `input.server.service`): an explicit `--server` target
    // never receives this client's local environment (gate-7 F7).
    let environment = input.server.managed.then(util::env_session);
    let target =
        resolve_session_target(&client, &input, &local.display().to_string(), environment).await?;

    if !target.resume {
        if let Some(title) = &input.title {
            let effective = if title.is_empty() {
                let mut suffix = message.clone();
                if suffix.chars().count() > 50 {
                    suffix = suffix.chars().take(50).collect();
                    suffix.push_str("...");
                }
                suffix
            } else {
                title.clone()
            };
            let _ = client
                .session()
                .update(
                    &target.session.id,
                    &SessionUpdateBody { title: Some(effective), ..Default::default() },
                )
                .await;
        }
    }

    let prompt = PromptInput {
        client,
        session_id: target.session.id.clone(),
        directory: target.directory.clone(),
        message,
        files,
        agent: target.agent.clone(),
        model: target.model.clone(),
        thinking: input.thinking,
        format: input.format,
        auto: input.auto,
    };
    Ok(noninteractive::run_prompt(prompt).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_formatting_quotes_spaces() {
        assert_eq!(format_message(&[]), None);
        assert_eq!(
            format_message(&["hello".to_owned(), "two words".to_owned()]).as_deref(),
            Some("hello \"two words\"")
        );
    }

    #[test]
    fn merge_input_orders_message_before_piped() {
        assert_eq!(merge_input(Some("a".into()), Some("b".into())).as_deref(), Some("a\nb"));
        assert_eq!(merge_input(None, Some("b".into())).as_deref(), Some("b"));
    }

    #[test]
    fn model_refs_parse() {
        assert_eq!(parse_run_model(None), None);
        assert_eq!(
            parse_run_model(Some("anthropic/claude")),
            Some(("anthropic".into(), "claude".into(), None))
        );
        assert_eq!(
            parse_run_model(Some("openai/gpt#turbo")),
            Some(("openai".into(), "gpt".into(), Some("turbo".into())))
        );
        assert_eq!(parse_run_model(Some("nonsense")), None);
    }
}
