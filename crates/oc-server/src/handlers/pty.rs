//! PTY handlers (v2 `handlers/pty.ts`): REST CRUD + connect-token + the
//! WS attach protocol (replay + metaFrame cursor + 4404 attach failure,
//! deep-map §3).

#![allow(
    clippy::must_use_candidate,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use oc_core::pty::{CreateInput, UpdateInput};
use serde_json::{json, Value};

use super::common::{
    json_location_data, no_content, parse_query_field, percent_decode, resolve_location, ApiError,
    SharedState,
};
use super::pty_state;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/pty", axum::routing::get(list))
        .route("/api/pty", axum::routing::post(create))
        .route("/api/pty/:ptyID", axum::routing::delete(remove))
        .route("/api/pty/:ptyID", axum::routing::get(get))
        .route("/api/pty/:ptyID", axum::routing::put(update))
        .route("/api/pty/:ptyID/connect", axum::routing::get(connect))
        .route("/api/pty/:ptyID/connect-token", axum::routing::post(connect_token))
}

fn info_json(info: &oc_core::pty::Info) -> Value {
    json!({
        "id": info.id.0,
        "title": info.title,
        "command": info.command,
        "args": info.args,
        "cwd": info.cwd,
        "status": match info.status {
            oc_core::pty::Status::Running => "running",
            oc_core::pty::Status::Exited => "exited",
        },
        "pid": info.pid,
        "exitCode": info.exit_code,
    })
}

// ---------------------------------------------------------------------------
// REST
// ---------------------------------------------------------------------------

async fn list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let shared = pty_state::shared(&state);
    let data: Vec<Value> = shared.pty.list().iter().map(info_json).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn create(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let shared = pty_state::shared(&state);
    let input = CreateInput {
        command: body["command"].as_str().map(String::from),
        args: body["args"]
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect()),
        cwd: body["cwd"]
            .as_str()
            .map(String::from)
            .or_else(|| Some(location.as_path().to_string_lossy().into_owned())),
        title: body["title"].as_str().map(String::from),
        env: body["env"].as_object().map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        }),
        size: body["size"]
            .as_object()
            .and_then(|size| Some((size["cols"].as_u64()? as u16, size["rows"].as_u64()? as u16))),
    };
    let info = shared.pty.create(&input).map_err(|_| ApiError::unknown_ref())?;
    Ok(json_location_data(&location.as_path().to_string_lossy(), info_json(&info)))
}

async fn get(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let shared = pty_state::shared(&state);
    let info = shared
        .pty
        .get(&oc_core::pty::PtyId(pty_id.clone()))
        .map_err(|_| ApiError::pty_not_found(&pty_id))?;
    Ok(json_location_data(&location.as_path().to_string_lossy(), info_json(&info)))
}

async fn update(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let shared = pty_state::shared(&state);
    let input = UpdateInput {
        title: body["title"].as_str().map(String::from),
        size: body["size"]
            .as_object()
            .and_then(|size| Some((size["cols"].as_u64()? as u16, size["rows"].as_u64()? as u16))),
    };
    let info = shared
        .pty
        .update(&oc_core::pty::PtyId(pty_id.clone()), input)
        .await
        .map_err(|_| ApiError::pty_not_found(&pty_id))?;
    Ok(json_location_data(&location.as_path().to_string_lossy(), info_json(&info)))
}

async fn remove(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
) -> Result<Response, ApiError> {
    let shared = pty_state::shared(&state);
    shared
        .pty
        .remove(&oc_core::pty::PtyId(pty_id.clone()))
        .await
        .map_err(|_| ApiError::pty_not_found(&pty_id))?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /api/pty/:ptyID/connect-token (x-opencode-ticket:1 + origin policy)
// ---------------------------------------------------------------------------

async fn connect_token(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if headers.get("x-opencode-ticket").and_then(|value| value.to_str().ok()) != Some("1") {
        return Err(ApiError::forbidden("missing x-opencode-ticket header"));
    }
    if !origin_allowed(&state, &headers) {
        return Err(ApiError::forbidden("origin not allowed"));
    }
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let shared = pty_state::shared(&state);
    if shared.pty.get(&oc_core::pty::PtyId(pty_id.clone())).is_err() {
        return Err(ApiError::pty_not_found(&pty_id));
    }
    let issued = crate::tickets::issue(
        &state,
        crate::tickets::Scope {
            pty_id: pty_id.clone(),
            directory: location.as_path().to_path_buf(),
            workspace_id: None,
        },
    );
    Ok(axum::Json(json!({
        "location": { "directory": location.as_path().to_string_lossy() },
        "data": issued,
    }))
    .into_response())
}

/// Origin policy: no Origin header (server-to-server) or an allowed CORS
/// origin / same-host browser origin (deep-map §2 "forces preflight").
pub(super) fn origin_allowed(state: &SharedState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    if state.cors_origins.iter().any(|allowed| allowed == origin) {
        return true;
    }
    // Same-host: match the Host header's authority.
    if let Some(host) = headers.get(header::HOST).and_then(|value| value.to_str().ok()) {
        let host = host.split(':').next().unwrap_or(host);
        return origin
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or(origin)
            == host;
    }
    false
}

/// Consumes a connect ticket (single-use, scoped, 60s TTL) through the
/// shell lane's primitive.
pub(super) fn consume_ticket(
    state: &SharedState,
    ticket: &str,
    pty_id: &str,
    location: &oc_core::location::LocationRef,
) -> bool {
    crate::tickets::consume(
        state,
        &crate::tickets::Scope {
            pty_id: pty_id.to_owned(),
            directory: location.as_path().to_path_buf(),
            workspace_id: None,
        },
        ticket,
        state.clock.unix_ms(),
    )
}

// ---------------------------------------------------------------------------
// GET /api/pty/:ptyID/connect — WS upgrade
// ---------------------------------------------------------------------------

fn empty_status(status: StatusCode) -> Response {
    (status, "").into_response()
}

async fn connect(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let query = uri.query().unwrap_or_default();
    let directory =
        parse_query_field(query, "location[directory]").map(|value| percent_decode(&value));
    let location = match resolve_location(query, &headers) {
        Ok(location) => location,
        Err(_) if directory.is_some() => {
            let path = std::path::PathBuf::from(directory.clone().unwrap_or_default());
            oc_core::location::LocationRef::new(path, None)
        }
        Err(error) => return error.into_response(),
    };

    let shared = pty_state::shared(&state);
    if shared.pty.get(&oc_core::pty::PtyId(pty_id.clone())).is_err() {
        return empty_status(StatusCode::NOT_FOUND);
    }
    if let Some(ticket) = parse_query_field(query, "ticket") {
        if !consume_ticket(&state, &ticket, &pty_id, &location) {
            return empty_status(StatusCode::FORBIDDEN);
        }
    }

    let cursor = parse_query_field(query, "cursor")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= -1);

    upgrade.on_upgrade(move |socket| async move {
        pty_session(shared, socket, pty_id, cursor, state.shutdown.subscribe()).await;
    })
}

/// One attached PTY socket: replay → metaFrame(cursor) → live output;
/// attach failure closes with 4404 after a 1s drain window (§3). The
/// shutdown watch ends the loop when the server stops (gate-6 F2).
async fn pty_session(
    shared: std::sync::Arc<pty_state::PtyShared>,
    mut socket: WebSocket,
    pty_id: String,
    cursor: Option<i64>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    use futures::{SinkExt as _, StreamExt as _};
    let (outbox_tx, mut outbox_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let id = oc_core::pty::PtyId(pty_id.clone());

    let on_data: Box<dyn Fn(&str) + Send + Sync> = {
        let outbox = outbox_tx.clone();
        Box::new(move |data: &str| {
            let _ = outbox.send(Message::Text(data.to_owned()));
        })
    };
    let on_end: Box<dyn FnOnce(Option<i32>) + Send + Sync> = {
        let outbox = outbox_tx.clone();
        Box::new(move |exit_code| {
            let _ = outbox.send(Message::Close(Some(axum::extract::ws::CloseFrame {
                code: 4404,
                reason: format!("pty exited{exit_code:?}").into(),
            })));
        })
    };

    let attach = shared.pty.attach(&id, cursor, on_data, on_end);
    let attachment = if let Ok(attachment) = attach {
        attachment.activate();
        std::sync::Arc::new(attachment)
    } else {
        // Attach failure: close frame 4404 (§3).
        let _ = socket
            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                code: 4404,
                reason: "attach failed".into(),
            })))
            .await;
        return;
    };
    let (replay, replay_cursor) = (attachment.replay.clone(), attachment.cursor);

    // Replay chunks → metaFrame(cursor) first, then the replayed bytes.
    let meta = json!({ "type": "meta", "cursor": replay_cursor });
    let _ = outbox_tx.send(Message::Text(meta.to_string()));
    for chunk in replay.as_bytes().chunks(16 * 1024) {
        let _ = outbox_tx.send(Message::Text(String::from_utf8_lossy(chunk).into_owned()));
    }

    let (mut sender, mut receiver) = socket_split(socket);
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            outbound = outbox_rx.recv() => {
                match outbound {
                    Some(message) => {
                        if sender.send(message).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        attachment.write(&text);
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        attachment.write(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    _ => {}
                }
            }
        }
    }
}

/// Splits a `WebSocket` into send/receive halves (`WebSocket::split` needs
/// `futures::StreamExt` — this local helper avoids the import dance).
fn socket_split(
    socket: WebSocket,
) -> (
    impl futures::Sink<Message> + Send + 'static,
    impl futures::Stream<Item = Result<Message, axum::Error>> + Send + 'static,
) {
    use futures::StreamExt as _;
    socket.split()
}
