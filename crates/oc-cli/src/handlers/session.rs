//! `opencode session *` (handlers/session/*).

use crate::cli::{ServerParams, SessionCommands, SessionListFormat};
use crate::handlers::connect;
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
    println!("{}", lines.join("\n"));
    0
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn format_local_time(millis: f64) -> String {
    // toLocaleString()-ish human timestamp (no chrono dependency).
    let millis = millis as i64;
    let seconds = millis.div_euclid(1000);
    let (year, month, day, hour, minute, second) = unix_to_utc(seconds);
    let _ = (year, month, day, hour, minute, second);
    format!("{year}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn unix_to_utc(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let hour = (time_of_day / 3600) as u32;
    let minute = ((time_of_day % 3600) / 60) as u32;
    let second = (time_of_day % 60) as u32;
    // civil_from_days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, minute, second)
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
    let Some(session_id) = session else {
        eprintln!("Error: Pass a session ID when running without an interactive terminal");
        return 1;
    };
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let data = match resolved
        .endpoint
        .client()
        .session()
        .export(
            session_id,
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
