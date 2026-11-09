//! `opencode session *` (handlers/session/*).

use crate::cli::{ServerParams, SessionCommands, SessionListFormat};
use crate::handlers::connect;
use crate::util;
use crate::util::error_message;

pub async fn run(command: &SessionCommands, server: &ServerParams) -> i32 {
    match command {
        SessionCommands::List { max_count, format } => list(server, *max_count, *format).await,
        SessionCommands::Delete { session_id } => delete(server, session_id).await,
        SessionCommands::Export { session, sanitize } => {
            export(server, session.as_deref(), *sanitize).await
        }
        SessionCommands::Import { file, directory } => {
            import(server, file, directory.as_deref()).await
        }
    }
}

async fn list(server: &ServerParams, max_count: Option<u32>, format: SessionListFormat) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let directory =
        std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
    let location = match client
        .location()
        .get(&oc_client::types::LocationGetQuery {
            location: Some(oc_client::types::QueryLocation { directory: Some(directory) }),
        })
        .await
    {
        Ok(location) => location,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let page = match client
        .session()
        .list(&oc_client::types::SessionListQuery {
            project: Some(location.project.id.clone()),
            parent_id: Some("null".to_owned()),
            order: Some(oc_client::types::SessionListQueryOrder::Desc),
            limit: Some(max_count.unwrap_or(100).to_string()),
            ..Default::default()
        })
        .await
    {
        Ok(page) => page,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    if format == SessionListFormat::Table && page.data.is_empty() {
        return 0;
    }
    if format == SessionListFormat::Json {
        let rows: Vec<serde_json::Value> = page
            .data
            .iter()
            .map(|session| {
                serde_json::json!({
                    "id": session.id,
                    "title": session.title,
                    "updated": session.time.updated,
                    "created": session.time.created,
                    "projectId": session.project_id,
                    "directory": session.location.directory,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows).expect("serializes"));
        return 0;
    }
    let lines: Vec<String> = page
        .data
        .iter()
        .map(|session| {
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| "Untitled session".to_owned())
                .replace(['\r', '\n', '\t'], " ");
            format!("{}\t{}\t{}", session.id, title, format_local_time(session.time.updated))
        })
        .collect();
    let output = lines.join("\n");
    // session/list.ts: interactive table output goes through a pager
    // (`less -R -S`; `cmd /c more` on Windows) unless --max-count pinned a
    // page. Spawn failure falls back to the plain write.
    if max_count.is_none() && util::stdout_is_tty() {
        let (program, args): (&str, Vec<&str>) =
            if cfg!(windows) { ("cmd", vec!["/c", "more"]) } else { ("less", vec!["-R", "-S"]) };
        if let Ok(mut child) = std::process::Command::new(program)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            use std::io::Write as _;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(output.as_bytes());
                let _ = stdin.write_all(b"\n");
            }
            drop(child.stdin.take());
            let _ = child.wait();
            return 0;
        }
    }
    println!("{output}");
    0
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn format_local_time(millis: f64) -> String {
    // `new Date(...).toLocaleString()` — the local-timezone wall clock
    // (same TZ-honoring approach as the stats handler's `local_timezone`).
    let millis = millis as i64;
    let zone = std::env::var("TZ")
        .ok()
        .and_then(|tz| jiff::tz::TimeZone::get(&tz).ok())
        .unwrap_or_else(jiff::tz::TimeZone::system);
    match jiff::Timestamp::from_millisecond(millis) {
        Ok(instant) => instant.to_zoned(zone).strftime("%Y-%m-%d %H:%M:%S").to_string(),
        Err(_) => "1970-01-01 00:00:00".to_owned(),
    }
}

async fn delete(server: &ServerParams, session_id: &str) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    if let Err(error) = resolved.endpoint.client().session().remove(session_id).await {
        eprintln!("{}", error_message(&error.to_string()));
        return 1;
    }
    println!("Session {session_id} deleted");
    0
}

async fn export(server: &ServerParams, session: Option<&str>, sanitize: bool) -> i32 {
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    // session/export.ts: without an explicit ID, pick a session
    // interactively from the 50 most recent (TTY only).
    let session_id = match session {
        Some(session_id) => session_id.to_owned(),
        None => match select_session_for_export(&client).await {
            Ok(ExportPick::Picked(id)) => id,
            // log.error("No sessions found") → outro("Done"), exit 0.
            Ok(ExportPick::NoSessions) => return 0,
            // export.ts:62-66 — `cancel("Cancelled")` + `process.exitCode
            // = 130` (Ctrl-C convention).
            Ok(ExportPick::Cancelled) => return 130,
            Err(message) => {
                eprintln!("{message}");
                return 1;
            }
        },
    };
    let data = match client
        .session()
        .export(
            &session_id,
            &oc_client::types::ExperimentalSessionExportQuery {
                sanitize: Some(if sanitize {
                    oc_client::types::ExperimentalSessionExportQuerySanitize::True
                } else {
                    oc_client::types::ExperimentalSessionExportQuerySanitize::False
                }),
            },
        )
        .await
    {
        Ok(data) => data,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    println!("{}", serde_json::to_string_pretty(&data).expect("serializes"));
    0
}

/// The export.ts interactive selector (`autocomplete` over the 50 most
/// recent sessions; hint = local timestamp - last 8 of the ID).
/// `Err` on transport failure.
enum ExportPick {
    Picked(String),
    /// "No sessions found" → Done, exit 0.
    NoSessions,
    /// Cancelled → exit 130 (export.ts:62-66).
    Cancelled,
}

#[allow(clippy::too_many_lines)]
async fn select_session_for_export(client: &oc_client::Client) -> Result<ExportPick, String> {
    if !(util::stdin_is_tty() && util::stderr_is_tty()) {
        return Err("Pass a session ID when running without an interactive terminal".to_owned());
    }
    let directory =
        std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
    let page = client
        .session()
        .list(&oc_client::types::SessionListQuery {
            directory: Some(directory.clone()),
            parent_id: Some("null".to_owned()),
            order: Some(oc_client::types::SessionListQueryOrder::Desc),
            limit: Some("50".to_owned()),
            ..Default::default()
        })
        .await
        .map_err(|error| error_message(&error.to_string()))?;
    if page.data.is_empty() {
        eprintln!("No sessions found");
        return Ok(ExportPick::NoSessions);
    }
    let items: Vec<(String, String)> = page
        .data
        .iter()
        .map(|session| {
            let title = session
                .title
                .clone()
                .unwrap_or_else(|| "Untitled session".to_owned())
                .replace(['\r', '\n', '\t'], " ");
            let hint = format!(
                "{} - {}",
                format_local_time(session.time.updated),
                session
                    .id
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<String>()
            );
            (session.id.clone(), format!("{title} ({hint})"))
        })
        .collect();
    // G-75/export.ts: ALL clack UI goes to stderr — stdout is reserved
    // for the exported session content.
    eprintln!("◆ Export session");
    let picked = dialoguer::FuzzySelect::new()
        .with_prompt("Select session to export")
        .items(&items.iter().map(|(_, label)| label).collect::<Vec<_>>())
        .default(0)
        .max_length(10)
        .interact_opt();
    match picked {
        Ok(Some(index)) => {
            eprintln!("◇ Exporting session...");
            Ok(ExportPick::Picked(items[index].0.clone()))
        }
        _ => {
            eprintln!("Cancelled");
            Ok(ExportPick::Cancelled)
        }
    }
}

/// `Schema.decodeUnknownEffect(SessionTransfer.Data)` (import.ts:25-26):
/// validate the imported structure against the SessionTransfer shape —
/// top-level `info` (Session.Info) and `messages` (array) required.
fn validate_transfer(data: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<oc_schema::session::TransferData>(data.clone())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn import(server: &ServerParams, file: &str, directory: Option<&str>) -> i32 {
    let text = if file.starts_with("http://") || file.starts_with("https://") {
        match oc_http::client().get(file).send().await {
            Ok(response) if response.status().is_success() => {
                response.text().await.unwrap_or_default()
            }
            Ok(response) => {
                eprintln!("Error: Failed to fetch session data: {}", response.status());
                return 1;
            }
            Err(error) => {
                eprintln!("Error: Failed to read session data: {error}");
                return 1;
            }
        }
    } else {
        match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("Error: Failed to read session data: {error}");
                return 1;
            }
        }
    };
    let data: serde_json::Value = if let Ok(data) = serde_json::from_str(&text) {
        data
    } else {
        eprintln!("Error: Failed to read session data: invalid JSON");
        return 1;
    };
    // import.ts:25-26 — the payload is validated against the
    // SessionTransfer schema shape ({info: Session.Info, messages: []})
    // BEFORE import; a bad payload rejects with a clear error.
    if let Err(error) = validate_transfer(&data) {
        eprintln!("Error: Invalid session data: {error}");
        return 1;
    }
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    // G-288② — ref import.ts:38 `path.resolve(input.directory ?? cwd)`:
    // a relative --directory resolves against the cwd (absolute inputs
    // pass through), with `..`/`.` components folded lexically like
    // path.resolve — never the raw flag text.
    let directory = directory.map_or_else(
        || std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        |directory| resolve_directory(directory),
    );
    let location = match client
        .location()
        .get(&oc_client::types::LocationGetQuery {
            location: Some(oc_client::types::QueryLocation { directory: Some(directory) }),
        })
        .await
    {
        Ok(location) => location,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let mut body = data;
    if let Some(object) = body.as_object_mut() {
        object
            .insert("location".to_owned(), serde_json::json!({ "directory": location.directory }));
    }
    let response = oc_http::client()
        .post(format!("{}/api/experimental/session/import", resolved.endpoint.url))
        .header(
            "authorization",
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!(
                    "opencode:{}",
                    resolved.endpoint.password.clone().unwrap_or_default()
                ))
            ),
        )
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            eprintln!("Error: Failed to import session: {error}");
            return 1;
        }
    };
    if response.status().as_u16() == 409 {
        eprintln!("Session already exists");
        return 0;
    }
    if !response.status().is_success() {
        eprintln!("Error: Failed to import session: {}", response.status());
        return 1;
    }
    let imported: serde_json::Value = response.json().await.unwrap_or_default();
    println!("Imported session: {}", imported["data"]["id"].as_str().unwrap_or_default());
    0
}

use base64::Engine as _;

/// G-288② — `path.resolve(cwd, input)` (import.ts:38): absolute inputs
/// pass through, relative ones join the cwd, and `.`/`..` fold lexically
/// (no filesystem touch; `..` at a root stays at the root).
fn resolve_directory(input: &str) -> String {
    let joined = if std::path::Path::new(input).is_absolute() {
        std::path::PathBuf::from(input)
    } else {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join(input)
    };
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{format_local_time, resolve_directory, validate_transfer};
    use serde_json::json;

    /// A minimal valid SessionTransfer payload (the export endpoint's
    /// shape): `{info: Session.Info, messages: []}`.
    fn transfer() -> serde_json::Value {
        let info = oc_schema::session::Info {
            id: oc_schema::id::SessionId::parse("ses_x").unwrap(),
            parent_id: None,
            fork: None,
            project_id: oc_schema::id::ProjectId::global(),
            agent: None,
            model: None,
            cost: oc_schema::common::Usd::zero(),
            tokens: Default::default(),
            outcome: None,
            time: oc_schema::session::Time {
                created: 1,
                updated: 2,
                idle: None,
                viewed: None,
                archived: None,
            },
            title: None,
            location: oc_schema::location::Ref {
                directory: oc_schema::common::AbsolutePath::parse("/repo"),
                workspace_id: None,
            },
            subpath: None,
            metadata: None,
            permissions: None,
            revert: None,
        };
        serde_json::to_value(oc_schema::session::TransferData { info, messages: Vec::new() })
            .expect("serializes")
    }

    /// import.ts:25-26 — the payload must match the SessionTransfer
    /// schema shape before anything is imported.
    #[test]
    fn session_import_accepts_a_well_formed_transfer() {
        assert!(validate_transfer(&transfer()).is_ok(), "{:?}", validate_transfer(&transfer()));
    }

    #[test]
    fn session_import_rejects_malformed_transfers() {
        // Missing top-level fields.
        let error = validate_transfer(&json!({})).expect_err("no fields");
        assert!(error.contains("missing field `info`"), "{error}");
        // messages must be an array.
        let error = validate_transfer(&json!({
            "info": transfer()["info"],
            "messages": "nope",
        }))
        .expect_err("messages type");
        assert!(error.contains("sequence") || error.contains("messages"), "{error}");
        // info must be a Session.Info (id is required and shaped).
        let error = validate_transfer(&json!({
            "info": { "id": "not a session" },
            "messages": [],
        }))
        .expect_err("bad info");
        assert!(!error.is_empty());
    }

    /// `toLocaleString` parity: TZ env pins the zone (each nextest test
    /// runs in its own process, so the env write is isolated).
    #[test]
    fn local_time_honors_tz_env() {
        // 2026-10-28T00:30:00Z
        let millis = 1_793_147_400_000.0_f64;
        std::env::set_var("TZ", "UTC");
        assert_eq!(format_local_time(millis), "2026-10-28 00:30:00");
        std::env::set_var("TZ", "Asia/Tokyo");
        assert_eq!(format_local_time(millis), "2026-10-28 09:30:00");
        std::env::set_var("TZ", "America/New_York");
        assert_eq!(format_local_time(millis), "2026-10-27 20:30:00");
    }

    /// Negative/absurd inputs don't panic — they clamp to the epoch.
    #[test]
    fn local_time_handles_bad_input() {
        std::env::set_var("TZ", "UTC");
        assert_eq!(format_local_time(0.0), "1970-01-01 00:00:00");
        assert_eq!(format_local_time(f64::NAN), "1970-01-01 00:00:00");
        assert_eq!(format_local_time(-1.0), "1969-12-31 23:59:59");
    }

    // --- G-288②: session import --directory absolutizes ---------------

    #[test]
    fn import_directory_resolves_relative_and_folds_dots() {
        let cwd = std::env::current_dir().expect("cwd");
        // Absolute passes through (with `.`/`..` folded, path.resolve-style).
        assert_eq!(resolve_directory("/a/b/../c"), "/a/c");
        assert_eq!(resolve_directory("/a/./b"), "/a/b");
        // Relative joins the cwd.
        assert_eq!(resolve_directory("sub"), cwd.join("sub").display().to_string());
        // Relative with `..` folds after joining.
        assert_eq!(resolve_directory("sub/../other"), cwd.join("other").display().to_string());
    }
}
