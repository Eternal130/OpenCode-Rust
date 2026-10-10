//! The ACP service (port of `acp/service.ts`): the 13 request methods
//! (agent.ts:68-119) plus the cancel notification.

use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthMethod, AuthMethodAgent, AuthenticateRequest, AuthenticateResponse,
    CloseSessionRequest, CloseSessionResponse, DeleteSessionRequest, DeleteSessionResponse,
    ForkSessionRequest, ForkSessionResponse, Implementation, InitializeRequest, InitializeResponse,
    ListSessionsRequest, ListSessionsResponse, LoadSessionRequest, LoadSessionResponse,
    McpCapabilities, NewSessionRequest, NewSessionResponse, PromptCapabilities, PromptRequest,
    PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SessionAdditionalDirectoriesCapabilities, SessionCapabilities, SessionCloseCapabilities,
    SessionDeleteCapabilities, SessionForkCapabilities, SessionInfo, SessionListCapabilities,
    SessionResumeCapabilities, SetSessionConfigOptionRequest, SetSessionConfigOptionResponse,
    SetSessionModeRequest, SetSessionModeResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use serde_json::Value;

use crate::acp::capabilities::{Capabilities, CHILD_SESSION_UPDATES};
use crate::acp::config_option::{self, Change};
use crate::acp::directories;
use crate::acp::error::{classify, decode_session_id, AcpError};
use crate::acp::prompt;
use crate::acp::replay;
use crate::acp::sessions::{supported_mcp_servers, Attached};
use crate::acp::{App, Conn};

/// `AuthMethodID` (service.ts:48).
const AUTH_METHOD_ID: &str = "opencode-login";

/// `initialize` (service.ts:106-130).
pub async fn initialize(app: &Arc<App>, request: InitializeRequest) -> InitializeResponse {
    *app.capabilities.write().await = Capabilities::parse(Some(&request.client_capabilities));
    // terminal-auth meta (service.ts:113-117).
    let meta =
        if request.client_capabilities.meta.as_ref().and_then(|meta| meta.get("terminal-auth"))
            == Some(&Value::Bool(true))
        {
            let mut meta = serde_json::Map::new();
            meta.insert(
                "terminal-auth".to_owned(),
                serde_json::json!({
                    "command": "opencode",
                    "args": ["auth", "login"],
                    "label": "OpenCode Login",
                }),
            );
            Some(meta)
        } else {
            None
        };
    let auth_method = AuthMethod::Agent(
        AuthMethodAgent::new(AUTH_METHOD_ID, "Login with opencode")
            .description("Run `opencode auth login` in the terminal")
            .meta(meta),
    );
    let mut agent_meta = serde_json::Map::new();
    agent_meta.insert(CHILD_SESSION_UPDATES.to_owned(), Value::Bool(true));
    InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(PromptCapabilities::new().embedded_context(true).image(true))
                .mcp_capabilities(McpCapabilities::new().http(true).sse(false))
                .session_capabilities(
                    SessionCapabilities::new()
                        .list(SessionListCapabilities::new())
                        .delete(SessionDeleteCapabilities::new())
                        .additional_directories(SessionAdditionalDirectoriesCapabilities::new())
                        .fork(SessionForkCapabilities::new())
                        .resume(SessionResumeCapabilities::new())
                        .close(SessionCloseCapabilities::new()),
                )
                .meta(agent_meta),
        )
        .auth_methods(vec![auth_method])
        .agent_info(Implementation::new("OpenCode", crate::util::VERSION))
}

/// `authenticate` (service.ts:131-135).
pub fn authenticate(request: &AuthenticateRequest) -> Result<AuthenticateResponse, AcpError> {
    if request.method_id.0.as_ref() != AUTH_METHOD_ID {
        return Err(AcpError::UnknownAuthMethod { method_id: request.method_id.to_string() });
    }
    Ok(AuthenticateResponse::new())
}

/// `newSession` (service.ts:136-146).
pub async fn new_session(
    app: &Arc<App>,
    conn: &Conn,
    request: NewSessionRequest,
) -> Result<NewSessionResponse, AcpError> {
    let cwd = request.cwd.to_string_lossy().into_owned();
    let additional = request
        .additional_directories
        .iter()
        .map(|directory| directory.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let directories = directories::parse(&cwd, &additional)?;
    let mcp_servers = supported_mcp_servers(&request.mcp_servers)?;
    // Load first so a catalog failure leaves no session (service.ts:139).
    app.catalogs.get(&cwd).await?;
    let (permissions, metadata) = directories::grant(&directories);
    let created = app
        .client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef { directory: cwd.clone() }),
            permissions: permissions.and_then(|value| serde_json::from_value(value).ok()),
            metadata: metadata.and_then(|value| serde_json::from_value(value).ok()),
            ..Default::default()
        })
        .await
        .map_err(|error| classify(&error))?;
    let (attached, options, selected_rx) =
        app.sessions.attach(&created, &mcp_servers, &app.catalogs).await?;
    spawn_watch(app, conn, &attached, selected_rx);
    Ok(NewSessionResponse::new(attached.id.clone()).config_options(options))
}

/// `loadSession` (service.ts:147-163).
pub async fn load_session(
    app: &Arc<App>,
    conn: &Conn,
    request: LoadSessionRequest,
) -> Result<LoadSessionResponse, AcpError> {
    let cwd = request.cwd.to_string_lossy().into_owned();
    let additional = request
        .additional_directories
        .iter()
        .map(|directory| directory.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let directories = directories::parse(&cwd, &additional)?;
    let mcp_servers = supported_mcp_servers(&request.mcp_servers)?;
    let session = get_session(app, &request.session_id.to_string(), &cwd).await?;
    activate_directories(app, &session, &directories).await?;
    let (attached, _options, selected_rx) =
        app.sessions.attach(&session, &mcp_servers, &app.catalogs).await?;
    let capabilities = *app.capabilities.read().await;
    let replayed =
        replay::history(&app.client, conn, &attached.id, &attached.cwd, capabilities).await;
    if let Err(error) = replayed {
        // onError releases the attachment (service.ts:161).
        app.sessions.release(&attached).await;
        return Err(error);
    }
    let catalog = app.catalogs.get(&attached.cwd).await?;
    let options = config_option::config_options(&catalog, &attached.selection().await);
    spawn_watch(app, conn, &attached, selected_rx);
    Ok(LoadSessionResponse::new().config_options(options))
}

/// `listSessions` (service.ts:164-189).
pub async fn list_sessions(
    app: &Arc<App>,
    request: ListSessionsRequest,
) -> Result<ListSessionsResponse, AcpError> {
    let query = oc_client::types::SessionListQuery {
        directory: request.cwd.as_ref().map(|cwd| cwd.to_string_lossy().into_owned()),
        order: Some(oc_client::types::SessionListQueryOrder::Desc),
        limit: Some("100".to_owned()),
        cursor: request.cursor.clone(),
        ..Default::default()
    };
    let page = app.client.session().list(&query).await.map_err(|error| classify(&error))?;
    let sessions = page
        .data
        .iter()
        .map(|session| {
            let additional = directories::list(
                session
                    .metadata
                    .as_ref()
                    .map(|metadata| serde_json::to_value(metadata).unwrap_or(Value::Null))
                    .as_ref(),
            );
            let mut info = SessionInfo::new(session.id.clone(), session.location.directory.clone())
                .title(with_timestamped_fallback(session))
                .updated_at(iso_timestamp(session.time.updated));
            if !additional.is_empty() {
                info = info.additional_directories(
                    additional.into_iter().map(std::path::PathBuf::from).collect(),
                );
            }
            info
        })
        .collect();
    let mut response = ListSessionsResponse::new(sessions);
    if let Some(next) = page.cursor.next.clone() {
        response = response.next_cursor(next);
    }
    Ok(response)
}

/// `deleteSession` (service.ts:190-198).
pub async fn delete_session(
    app: &Arc<App>,
    request: DeleteSessionRequest,
) -> Result<DeleteSessionResponse, AcpError> {
    let session_id = request.session_id.to_string();
    let decoded = decode_session_id(&session_id);
    if let Ok(id) = &decoded {
        let result = app.client.session().remove(id).await;
        match result {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.api_name(),
                    Some("SessionNotFoundError" | "InvalidRequestError")
                ) => {}
            Err(error) => return Err(classify(&error)),
        }
    }
    app.sessions.detach(&session_id).await;
    Ok(DeleteSessionResponse::new())
}

/// `resumeSession` (service.ts:199-206).
pub async fn resume_session(
    app: &Arc<App>,
    conn: &Conn,
    request: ResumeSessionRequest,
) -> Result<ResumeSessionResponse, AcpError> {
    let cwd = request.cwd.to_string_lossy().into_owned();
    let additional = request
        .additional_directories
        .iter()
        .map(|directory| directory.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let directories = directories::parse(&cwd, &additional)?;
    let mcp_servers = supported_mcp_servers(&request.mcp_servers)?;
    let session = get_session(app, &request.session_id.to_string(), &cwd).await?;
    activate_directories(app, &session, &directories).await?;
    let (attached, _options, selected_rx) =
        app.sessions.attach(&session, &mcp_servers, &app.catalogs).await?;
    let catalog = app.catalogs.get(&attached.cwd).await?;
    let options = config_option::config_options(&catalog, &attached.selection().await);
    spawn_watch(app, conn, &attached, selected_rx);
    Ok(ResumeSessionResponse::new().config_options(options))
}

/// `closeSession` (service.ts:207-211).
pub async fn close_session(
    app: &Arc<App>,
    request: CloseSessionRequest,
) -> Result<CloseSessionResponse, AcpError> {
    let session_id = request.session_id.to_string();
    crate::acp::turn::close(app, &session_id).await?;
    app.sessions.detach(&session_id).await;
    Ok(CloseSessionResponse::new())
}

/// `forkSession` (service.ts:212-221).
pub async fn fork_session(
    app: &Arc<App>,
    conn: &Conn,
    request: ForkSessionRequest,
) -> Result<ForkSessionResponse, AcpError> {
    let cwd = request.cwd.to_string_lossy().into_owned();
    let additional = request
        .additional_directories
        .iter()
        .map(|directory| directory.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let directories = directories::parse(&cwd, &additional)?;
    let mcp_servers = supported_mcp_servers(&request.mcp_servers)?;
    let session_id = decode_session_id(&request.session_id.to_string())?;
    let forked = app
        .client
        .session()
        .fork(&session_id, &oc_client::types::SessionForkBody::default())
        .await
        .map_err(|error| classify(&error))?;
    // Forks inherit the source's grants; replace them with this
    // request's (service.ts:217-218).
    activate_directories(app, &forked, &directories).await?;
    let (attached, options, selected_rx) =
        app.sessions.attach(&forked, &mcp_servers, &app.catalogs).await?;
    spawn_watch(app, conn, &attached, selected_rx);
    Ok(ForkSessionResponse::new(attached.id.clone()).config_options(options))
}

/// `setSessionConfigOption` (service.ts:222-228).
pub async fn set_session_config_option(
    app: &Arc<App>,
    request: SetSessionConfigOptionRequest,
) -> Result<SetSessionConfigOptionResponse, AcpError> {
    let session_id = request.session_id.to_string();
    let attached = app.sessions.require(&session_id).await?;
    let Some(value) = request.value.as_value_id().map(ToString::to_string) else {
        return Err(AcpError::InvalidConfigOption { config_id: request.config_id.to_string() });
    };
    select_with_reload(app, &attached, &request.config_id.to_string(), &value).await?;
    let catalog = app.catalogs.get(&attached.cwd).await?;
    let options = config_option::config_options(&catalog, &attached.selection().await);
    Ok(SetSessionConfigOptionResponse::new(options))
}

/// `setSessionMode` (service.ts:229-233).
pub async fn set_session_mode(
    app: &Arc<App>,
    request: SetSessionModeRequest,
) -> Result<SetSessionModeResponse, AcpError> {
    let session_id = request.session_id.to_string();
    let attached = app.sessions.require(&session_id).await?;
    select_with_reload(app, &attached, "mode", &request.mode_id.to_string()).await?;
    Ok(SetSessionModeResponse::new())
}

/// `prompt` (service.ts:234 → turn.prompt).
pub async fn prompt(
    app: &Arc<App>,
    conn: &Conn,
    request: PromptRequest,
) -> Result<PromptResponse, AcpError> {
    let session_id = request.session_id.to_string();
    let attached = app.sessions.require(&session_id).await?;
    let catalog = app.catalogs.get(&attached.cwd).await?;
    let prepared = prompt::prepare(&catalog, &request.prompt)?;
    crate::acp::turn::prompt(app, conn, &attached, &prepared).await
}

/// `cancel` (service.ts:235 → turn.cancel).
pub async fn cancel(app: &Arc<App>, session_id: &str) {
    if std::env::var("OPENCODE_ACP_DEBUG").is_ok() {
        eprintln!("[acp] cancel requested for {session_id}");
    }
    app.turns.cancel(session_id).await;
}

// ---------------------------------------------------------------------
// Shared internals
// ---------------------------------------------------------------------

/// `withReload` (service.ts:80-85): retry once after a catalog reload
/// when the selection names a model/mode/effort the catalog lacked.
async fn select_with_reload(
    app: &Arc<App>,
    attached: &Arc<Attached>,
    config_id: &str,
    value: &str,
) -> Result<(), AcpError> {
    async fn select(
        app: &Arc<App>,
        attached: &Arc<Attached>,
        config_id: &str,
        value: &str,
    ) -> Result<(), AcpError> {
        let catalog = app.catalogs.get(&attached.cwd).await?;
        let selection = attached.selection().await;
        let change = config_option::resolve_change(&catalog, &selection, config_id, value)?;
        app.sessions.select(attached, &change).await
    }
    let app = Arc::clone(app);
    match select(&app, attached, config_id, value).await {
        Err(
            AcpError::InvalidModel { .. }
            | AcpError::InvalidMode { .. }
            | AcpError::InvalidEffort { .. },
        ) => {
            app.catalogs.reload(&attached.cwd).await?;
            select(&app, attached, config_id, value).await
        }
        other => other,
    }
}

/// `getSession` (service.ts:97-103): fetch + directory match.
async fn get_session(
    app: &Arc<App>,
    session_id: &str,
    cwd: &str,
) -> Result<oc_client::types::SessionInfo, AcpError> {
    let id = decode_session_id(session_id)?;
    let session = app.client.session().get(&id).await.map_err(|error| classify(&error))?;
    let session_dir = std::fs::canonicalize(&session.location.directory)
        .unwrap_or_else(|_| std::path::PathBuf::from(&session.location.directory));
    let requested = std::fs::canonicalize(cwd).unwrap_or_else(|_| std::path::PathBuf::from(cwd));
    if session_dir != requested {
        return Err(AcpError::SessionDirectoryMismatch {
            session_id: session_id.to_owned(),
            cwd: cwd.to_owned(),
        });
    }
    Ok(session)
}

/// `ACPDirectories.activate` wrapper (service.ts:151, 203, 218).
async fn activate_directories(
    app: &Arc<App>,
    session: &oc_client::types::SessionInfo,
    directories: &[String],
) -> Result<(), AcpError> {
    let metadata = session
        .metadata
        .as_ref()
        .map(|metadata| serde_json::to_value(metadata).unwrap_or(Value::Null));
    let permissions: Option<Vec<Value>> = session.permissions.as_ref().and_then(|rules| {
        serde_json::to_value(rules).ok().and_then(|value| serde_json::from_value(value).ok())
    });
    let Some(body) =
        directories::activate(&session.id, permissions.as_deref(), metadata.as_ref(), directories)
    else {
        return Ok(());
    };
    let body: oc_client::types::SessionUpdateBody =
        serde_json::from_value(body).map_err(|error| AcpError::ServiceFailure {
            safe_message: format!("session update encode failed: {error}"),
            service: Some("session".to_owned()),
            error_name: None,
        })?;
    app.client.session().update(&session.id, &body).await.map_err(|error| classify(&error))
}

/// Start the per-attach follow-up watcher after the response went out
/// (`responded`, sessions.ts:141-146).
fn spawn_watch(
    app: &Arc<App>,
    conn: &Conn,
    attached: &Arc<Attached>,
    selected_rx: tokio::sync::mpsc::UnboundedReceiver<Change>,
) {
    let sessions = app.sessions.clone();
    let catalogs = Arc::clone(&app.catalogs);
    let attached = Arc::clone(attached);
    let conn = conn.clone();
    tokio::spawn(async move {
        sessions.start_watch(attached, conn, catalogs, selected_rx).await;
    });
}

/// `withTimestampedFallback` (util/session-title-fallback.ts).
fn with_timestamped_fallback(session: &oc_client::types::SessionInfo) -> String {
    if let Some(title) = &session.title {
        return title.clone();
    }
    let kind = if session.parent_id.is_some() { "Child" } else { "New" };
    format!("{kind} session - {}", iso_from_epoch_ms(seconds_to_ms(session.time.created)))
}

/// `DateTime.formatIso` (service.ts:184): epoch seconds → ISO 8601
/// (`YYYY-MM-DDTHH:MM:SS.sssZ`, v2's `toISOString()`).
fn iso_timestamp(epoch_seconds: f64) -> String {
    iso_from_epoch_ms(seconds_to_ms(epoch_seconds))
}

/// Seconds → milliseconds with the JS `Date` truncation semantics.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn seconds_to_ms(epoch_seconds: f64) -> i64 {
    let millis = (epoch_seconds * 1000.0).trunc();
    millis.clamp(i64::MIN as f64, i64::MAX as f64) as i64
}

/// Civil-from-days (Howard Hinnant's algorithm) — no chrono dependency.
fn iso_from_epoch_ms(epoch_ms: i64) -> String {
    let seconds_total = epoch_ms.div_euclid(1000);
    let millis = epoch_ms.rem_euclid(1000);
    let days = seconds_total.div_euclid(86_400);
    let seconds_of_day = seconds_total.rem_euclid(86_400);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(title: Option<&str>, created: f64) -> oc_client::types::SessionInfo {
        oc_client::types::SessionInfo {
            id: "ses_1".to_owned(),
            title: title.map(str::to_owned),
            time: oc_client::types::SessionInfoTime { created, ..Default::default() },
            parent_id: Some("ses_parent".to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn missing_titles_fall_back_to_timestamps() {
        let title = with_timestamped_fallback(&session(None, 1_767_225_600.0));
        assert!(title.starts_with("Child session - 2026-01-01T"), "{title}");
        assert_eq!(with_timestamped_fallback(&session(Some("Kept"), 0.0)), "Kept");
    }

    #[test]
    fn updated_at_is_iso() {
        assert_eq!(iso_timestamp(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_timestamp(1_767_225_600.0), "2026-01-01T00:00:00.000Z");
    }
}
