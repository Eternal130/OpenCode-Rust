//! Persistent-PTY handlers (v2 `handlers/persistent-pty.ts`): the
//! session-terminal REST family, handoff/shutdown, connect-token, and the
//! WS attach protocol (`attached/replay_complete/resized` JSON + raw output,
//! framed input `[type u8][cols u16BE][rows u16BE][payload]`, deep-map §3).

#![allow(
    clippy::must_use_candidate,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use oc_core::persistent_pty::{CreateInput, Role, StreamEvent};
use oc_core::pty::PtyId;
use oc_schema::id::SessionId;
use serde_json::{json, Value};

use super::common::{json_data, no_content, parse_query_field, ApiError, SharedState};
use super::pty::origin_allowed;
use super::pty_state;
use super::session::session_services;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/experimental/persistent-pty/handoff", axum::routing::post(handoff))
        .route("/api/experimental/persistent-pty/shutdown", axum::routing::post(shutdown))
        .route("/api/experimental/persistent-pty/:ptyID", axum::routing::delete(remove))
        .route("/api/experimental/persistent-pty/:ptyID", axum::routing::get(get))
        .route("/api/experimental/persistent-pty/:ptyID", axum::routing::put(update))
        .route("/api/experimental/persistent-pty/:ptyID/connect", axum::routing::get(connect))
        .route(
            "/api/experimental/persistent-pty/:ptyID/connect-token",
            axum::routing::post(connect_token),
        )
        .route("/api/experimental/persistent-pty/:ptyID/snapshot", axum::routing::get(snapshot))
        .route("/api/experimental/session/:sessionID/terminal", axum::routing::get(list))
        .route("/api/experimental/session/:sessionID/terminal", axum::routing::post(create))
        .route("/api/experimental/session/:sessionID/terminal/read", axum::routing::get(read))
}

fn parse_session_id(raw: &str) -> SessionId {
    SessionId::parse(raw.to_string()).unwrap_or_else(|error| SessionId(error.value))
}

fn info_json(info: &oc_core::persistent_pty::Info) -> Value {
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
        "sessionID": info.session_id.as_str(),
        "foregroundProcess": info.foreground_process,
        "size": { "cols": info.size.0, "rows": info.size.1 },
        "output": { "head": info.output.0, "tail": info.output.1 },
    })
}

fn unavailable(error: oc_core::persistent_pty::UnavailableError) -> ApiError {
    if error.message.starts_with("PersistentPty.NotFoundError") {
        let id = error.message.rsplit(' ').next().unwrap_or_default();
        return ApiError::pty_not_found(id);
    }
    ApiError::service_unavailable(error.message, Some("opencode-pty"))
}

fn query_field(uri: &Uri, name: &str) -> Option<String> {
    parse_query_field(uri.query().unwrap_or_default(), name)
        .map(|value| super::common::percent_decode(&value))
}

// ---------------------------------------------------------------------------
// REST
// ---------------------------------------------------------------------------

async fn list(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    let shared = pty_state::shared(&state);
    let parsed = parse_session_id(&session_id);
    let infos = shared.persistent.list(Some(&parsed)).map_err(unavailable)?;
    let data: Vec<Value> = infos.iter().map(info_json).collect();
    Ok(json_data(Value::Array(data)))
}

async fn create(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    let shared = pty_state::shared(&state);
    let input = CreateInput {
        command: body["command"].as_str().map(String::from),
        args: body["args"]
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default(),
        cwd: body["cwd"].as_str().map(String::from),
        title: body["title"].as_str().unwrap_or_default().to_owned(),
        env: body["env"]
            .as_object()
            .map(|map| {
                map.iter()
                    .filter_map(|(key, value)| {
                        value.as_str().map(|value| (key.clone(), value.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        size: body["size"]
            .as_object()
            .and_then(|size| Some((size["cols"].as_u64()? as u16, size["rows"].as_u64()? as u16))),
    };
    let info = shared
        .persistent
        .create(&parse_session_id(&session_id), input)
        .await
        .map_err(unavailable)?;
    Ok(json_data(info_json(&info)))
}

async fn read(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    let shared = pty_state::shared(&state);
    let lines = query_field(&uri, "lines").and_then(|value| value.parse::<u32>().ok());
    let result =
        shared.persistent.read(&parse_session_id(&session_id), lines).await.map_err(unavailable)?;
    let data = result.map(|result| {
        json!({
            "ptyID": result.pty_id.0,
            "title": result.title,
            "cwd": result.cwd,
            "foregroundProcess": result.foreground_process,
            "screen": {
                "text": result.screen.text,
                "cols": result.screen.cols,
                "rows": result.screen.rows,
                "cursor": { "x": result.screen.cursor.0, "y": result.screen.cursor.1 },
            },
        })
    });
    Ok(json_data(data.unwrap_or(Value::Null)))
}

async fn get(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
) -> Result<Response, ApiError> {
    let shared = pty_state::shared(&state);
    let info = shared.persistent.get(&PtyId(pty_id.clone())).map_err(unavailable)?;
    Ok(json_data(info_json(&info)))
}

async fn update(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let shared = pty_state::shared(&state);
    let id = PtyId(pty_id.clone());
    if let Some(size) = body["size"].as_object() {
        let cols = size["cols"].as_u64().unwrap_or(80) as u16;
        let rows = size["rows"].as_u64().unwrap_or(24) as u16;
        shared.persistent.resize(&id, cols, rows, None).await.map_err(unavailable)?;
    }
    let info = shared.persistent.get(&id).map_err(unavailable)?;
    Ok(json_data(info_json(&info)))
}

async fn remove(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
) -> Result<Response, ApiError> {
    let shared = pty_state::shared(&state);
    shared.persistent.remove(&PtyId(pty_id.clone())).await.map_err(unavailable)?;
    Ok(no_content())
}

async fn shutdown(State(state): State<SharedState>) -> Result<Response, ApiError> {
    let shared = pty_state::shared(&state);
    shared.persistent.shutdown().await.map_err(unavailable)?;
    Ok(no_content())
}

async fn handoff(State(state): State<SharedState>) -> Result<Response, ApiError> {
    let shared = pty_state::shared(&state);
    let ticket = shared.persistent.handoff().map_err(unavailable)?;
    let handoff = ticket.map(|ticket| {
        json!({
            "directory": ticket.directory,
            "instanceID": ticket.instance_id,
            "ticket": ticket.ticket,
            "expiresAt": ticket.expires_at,
        })
    });
    Ok(axum::Json(json!({ "handoff": handoff })).into_response())
}

async fn snapshot(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
) -> Result<Response, ApiError> {
    use base64::Engine as _;
    let shared = pty_state::shared(&state);
    let snapshot = shared.persistent.snapshot(&PtyId(pty_id.clone())).map_err(unavailable)?;
    Ok(json_data(json!({
        "info": info_json(&snapshot.info),
        "text": snapshot.text,
        "checkpoint": base64::engine::general_purpose::STANDARD.encode(&snapshot.checkpoint),
        "cursor": { "x": snapshot.cursor.0, "y": snapshot.cursor.1 },
    })))
}

// ---------------------------------------------------------------------------
// connect-token ({data} only — no location wrapper, deep-map §2)
// ---------------------------------------------------------------------------

async fn connect_token(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    if headers.get("x-opencode-ticket").and_then(|value| value.to_str().ok()) != Some("1") {
        return Err(ApiError::forbidden("missing x-opencode-ticket header"));
    }
    if !origin_allowed(&state, &headers) {
        return Err(ApiError::forbidden("origin not allowed"));
    }
    let shared = pty_state::shared(&state);
    shared.persistent.get(&PtyId(pty_id.clone())).map_err(unavailable)?;
    let directory = std::env::current_dir().unwrap_or_default();
    let issued = crate::tickets::issue(
        &state,
        crate::tickets::Scope { pty_id: pty_id.clone(), directory, workspace_id: None },
    );
    Ok(json_data(issued))
}

// ---------------------------------------------------------------------------
// GET /:ptyID/connect — WS attach protocol (§3)
// ---------------------------------------------------------------------------

async fn connect(
    State(state): State<SharedState>,
    Path(pty_id): Path<String>,
    uri: Uri,
    headers: axum::http::HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let shared = pty_state::shared(&state);
    let query = uri.query().unwrap_or_default();
    if shared.persistent.get(&PtyId(pty_id.clone())).is_err() {
        return (StatusCode::NOT_FOUND, "").into_response();
    }
    if let Some(ticket) = parse_query_field(query, "ticket") {
        let location =
            oc_core::location::LocationRef::new(std::env::current_dir().unwrap_or_default(), None);
        if !super::pty::consume_ticket(&state, &ticket, &pty_id, &location) {
            return (StatusCode::FORBIDDEN, "").into_response();
        }
    }
    let _ = headers;
    let cursor = match parse_query_field(query, "cursor") {
        None => 0usize,
        Some(raw) => match raw.parse::<usize>() {
            Ok(value) => value,
            Err(_) => return (StatusCode::BAD_REQUEST, "").into_response(),
        },
    };
    let role = match parse_query_field(query, "role").as_deref() {
        Some("observer") => Role::Observer,
        _ => Role::Controller,
    };
    let input_protocol = parse_query_field(query, "input_protocol").as_deref() == Some("1");
    let attachment_id = parse_query_field(query, "attachment_id")
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let takeover = parse_query_field(query, "takeover").as_deref() == Some("true");

    upgrade.on_upgrade(move |socket| async move {
        attach_session(
            shared,
            socket,
            PtyId(pty_id),
            cursor,
            role,
            input_protocol,
            attachment_id,
            takeover,
            state.shutdown.subscribe(),
        )
        .await;
    })
}

#[allow(clippy::too_many_arguments)] // the attach protocol's parameter set
async fn attach_session(
    shared: std::sync::Arc<pty_state::PtyShared>,
    mut socket: WebSocket,
    pty_id: PtyId,
    cursor: usize,
    role: Role,
    input_protocol: bool,
    attachment_id: String,
    takeover: bool,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    use futures::{SinkExt as _, StreamExt as _};
    let (outbox_tx, mut outbox_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    let on_event: Box<dyn Fn(StreamEvent) + Send + Sync> = {
        let outbox = outbox_tx.clone();
        Box::new(move |event: StreamEvent| match event {
            StreamEvent::Output(bytes) => {
                let _ = outbox.send(Message::Binary(bytes));
            }
            StreamEvent::Resized { cols, rows } => {
                let _ = outbox.send(Message::Text(
                    json!({ "type": "resized", "cols": cols, "rows": rows }).to_string(),
                ));
            }
            StreamEvent::Exited { exit_code } => {
                let _ = outbox.send(Message::Text(
                    json!({ "type": "exited", "exitCode": exit_code }).to_string(),
                ));
            }
        })
    };
    let on_end: Box<dyn FnOnce() + Send + Sync> = {
        let outbox = outbox_tx.clone();
        Box::new(move || {
            let _ = outbox.send(Message::Close(Some(axum::extract::ws::CloseFrame {
                code: 1000,
                reason: "".into(),
            })));
        })
    };

    let Ok(attachment) = shared.persistent.attach(
        &pty_id,
        oc_core::persistent_pty::AttachInput {
            cursor,
            attachment_id: &attachment_id,
            role,
            takeover,
            on_event,
            on_end,
        },
    ) else {
        // Unavailable → close 4404 "terminal unavailable" (§3).
        let _ = socket
            .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                code: 4404,
                reason: "terminal unavailable".into(),
            })))
            .await;
        return;
    };

    // The attached frame, then the raw replay, then replay_complete (§3).
    let attached = json!({
        "type": "attached",
        "attachmentID": attachment_id,
        "inputProtocol": u8::from(input_protocol),
        "info": info_json(&attachment.info),
        "role": match role {
            Role::Controller => "controller",
            Role::Observer => "observer",
        },
        "generation": attachment.generation,
        "replay": {
            "requestedOffset": attachment.replay.requested_offset,
            "availableOffset": attachment.replay.available_offset,
            "endOffset": attachment.replay.end_offset,
            "truncated": attachment.replay.truncated,
        },
    });
    let _ = outbox_tx.send(Message::Text(attached.to_string()));
    let replay_end = attachment.replay.end_offset;
    let replay_bytes = attachment.data.clone();
    let _ = outbox_tx.send(Message::Binary(replay_bytes));
    let _ = outbox_tx.send(Message::Text(
        json!({ "type": "replay_complete", "endOffset": replay_end }).to_string(),
    ));
    attachment.activate();

    // Serialized input (Semaphore(1) equivalent).
    let input_lock = std::sync::Arc::new(tokio::sync::Mutex::new(()));
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
                        let guard = input_lock.clone().lock_owned().await;
                        shared
                            .persistent
                            .write(&pty_id, &text, Some(&attachment_id))
                            .await
                            .ok();
                        drop(guard);
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        let guard = input_lock.clone().lock_owned().await;
                        if input_protocol {
                            if let Some(frame) = decode_framed(&bytes) {
                                match frame.kind {
                                    0 => {
                                        shared
                                            .persistent
                                            .control(&pty_id, &attachment_id, frame.cols, frame.rows)
                                            .await
                                            .ok();
                                    }
                                    _ => {
                                        shared
                                            .persistent
                                            .input(&pty_id, &attachment_id, frame.cols, frame.rows, &frame.payload)
                                            .await
                                            .ok();
                                    }
                                }
                            }
                        } else {
                            shared
                                .persistent
                                .write(&pty_id, &String::from_utf8_lossy(&bytes), Some(&attachment_id))
                                .await
                                .ok();
                        }
                        drop(guard);
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    _ => {}
                }
            }
        }
    }
}

struct FramedInput {
    kind: u8,
    cols: u16,
    rows: u16,
    payload: Vec<u8>,
}

/// `[type u8][cols u16BE][rows u16BE][payload]` — ≥5 bytes; kind 0 is a
/// control resize, kind 1+ carries input in `payload[5..]` (§3).
fn decode_framed(bytes: &[u8]) -> Option<FramedInput> {
    if bytes.len() < 5 {
        return None;
    }
    Some(FramedInput {
        kind: bytes[0],
        cols: u16::from_be_bytes([bytes[1], bytes[2]]),
        rows: u16::from_be_bytes([bytes[3], bytes[4]]),
        payload: bytes[5..].to_vec(),
    })
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
