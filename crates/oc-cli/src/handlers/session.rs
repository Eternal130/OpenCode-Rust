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
            Ok(Some(id)) => id,
            Ok(None) => return 0, // nothing found / cancelled
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
/// recent sessions; hint = local timestamp - last 8 of the ID). Returns
/// `Ok(None)` for "no sessions"/"cancelled", `Err` on transport failure.
async fn select_session_for_export(client: &oc_client::Client) -> Result<Option<String>, String> {
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
        return Ok(None);
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
    println!("◆ Export session");
    let picked = dialoguer::FuzzySelect::new()
        .with_prompt("Select session to export")
        .items(&items.iter().map(|(_, label)| label).collect::<Vec<_>>())
        .default(0)
        .max_length(10)
        .interact_opt();
    if let Ok(Some(index)) = picked {
        println!("◇ Exporting session...");
        Ok(Some(items[index].0.clone()))
    } else {
        println!("Cancelled");
        Ok(None)
    }
}

async fn import(server: &ServerParams, file: &str, directory: Option<&str>) -> i32 {
    let text = if file.starts_with("http://") || file.starts_with("https://") {
        match reqwest::Client::new().get(file).send().await {
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
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();
    let directory = directory.map_or_else(
        || std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default(),
        str::to_owned,
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
    let response = reqwest::Client::new()
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

#[cfg(test)]
mod tests {
    use super::format_local_time;

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
}
