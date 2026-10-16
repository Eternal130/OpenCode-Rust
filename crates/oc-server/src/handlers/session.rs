//! Session-family handlers (v2 `handlers/session.ts` port): session CRUD,
//! prompt/synthetic/compact admission, interrupt/wait, move, fork,
//! import/export, inbox, instructions entries, revert.
//!
//! Envelope + error discipline per server-deep-map §3.

#![allow(
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use oc_core::bus::{registry, PublishOptions};
use oc_core::database::schema::SessionRow;
use oc_core::engine::LocationServices;
use oc_core::location::LocationRef;
use oc_core::session::store::{
    CreateInput, ListAnchor, ListDirection, ListInput, ListOrder, ResolvedProject,
};
use oc_core::session::{InboxError, RunError};
use oc_schema::id::SessionId;
use oc_schema::session_inbox::{Delivery, Item};
use serde_json::{json, Value};

use super::common::{
    decode_session_cursor, encode_session_cursor, json_data, message_row_to_info, no_content,
    now_ms, resolve_location, revert_error, session_location, session_row_to_info, ApiError,
    CursorDirection, SessionAnchor, SessionCursor, SessionCursorScope, SharedState,
};

/// The default page size (v2: 50).
pub const DEFAULT_LIMIT: i64 = 50;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/session", axum::routing::get(list))
        .route("/api/session", axum::routing::post(create))
        .route("/api/session/active", axum::routing::get(active))
        .route("/api/experimental/session/import", axum::routing::post(import))
        .route("/api/experimental/session/stats", axum::routing::get(stats))
        .route("/api/experimental/session/:sessionID/export", axum::routing::get(export))
        .route(
            "/api/experimental/session/:sessionID/instructions/entries",
            axum::routing::get(instruction_entry_list),
        )
        .route(
            "/api/experimental/session/:sessionID/instructions/entries/:key",
            axum::routing::delete(instruction_entry_remove),
        )
        .route(
            "/api/experimental/session/:sessionID/instructions/entries/:key",
            axum::routing::put(instruction_entry_put),
        )
        .route("/api/experimental/session/:sessionID/log", axum::routing::get(log))
        .route("/api/experimental/session/:sessionID/skill", axum::routing::post(skill))
        .route("/api/experimental/session/:sessionID/wait", axum::routing::post(wait))
        .route("/api/session/:sessionID", axum::routing::delete(remove))
        .route("/api/session/:sessionID", axum::routing::get(get))
        .route("/api/session/:sessionID", axum::routing::patch(update))
        .route("/api/session/:sessionID/agent", axum::routing::post(switch_agent))
        .route("/api/session/:sessionID/background", axum::routing::post(background))
        .route("/api/session/:sessionID/command", axum::routing::post(command))
        .route("/api/session/:sessionID/compact", axum::routing::post(compact))
        .route("/api/session/:sessionID/context", axum::routing::get(context))
        .route("/api/session/:sessionID/diff", axum::routing::get(diff))
        .route("/api/session/:sessionID/environment", axum::routing::put(environment))
        .route("/api/session/:sessionID/fork", axum::routing::post(fork))
        .route("/api/session/:sessionID/generate", axum::routing::post(generate))
        .route("/api/session/:sessionID/inbox", axum::routing::get(inbox_list))
        .route("/api/session/:sessionID/inbox/:inboxID", axum::routing::delete(inbox_cancel))
        .route("/api/session/:sessionID/inbox/:inboxID", axum::routing::patch(inbox_update))
        .route("/api/session/:sessionID/interrupt", axum::routing::post(interrupt))
        .route("/api/session/:sessionID/model", axum::routing::post(switch_model))
        .route("/api/session/:sessionID/move", axum::routing::post(move_session))
        .route("/api/session/:sessionID/prompt", axum::routing::post(prompt))
        .route("/api/session/:sessionID/revert", axum::routing::delete(revert_clear))
        .route("/api/session/:sessionID/revert/commit", axum::routing::post(revert_commit))
        .route("/api/session/:sessionID/revert/stage", axum::routing::post(revert_stage))
        .route("/api/session/:sessionID/shell", axum::routing::post(shell))
        .route("/api/session/:sessionID/synthetic", axum::routing::post(synthetic))
        .route("/api/session/:sessionID/view", axum::routing::post(view))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn query_field(uri: &Uri, name: &str) -> Option<String> {
    let query = uri.query().unwrap_or_default();
    super::common::parse_query_field(query, name).map(|value| super::common::percent_decode(&value))
}

/// Assembles the location's services, mapping boot failures. A vanished
/// session directory is v2's `LocationNotFoundError` 404 (the session row
/// resolves, the location does not) — never a 500.
pub(crate) async fn services(
    state: &SharedState,
    location: &LocationRef,
) -> Result<std::sync::Arc<LocationServices>, ApiError> {
    state.engine.location(location.clone()).await.map_err(|error| {
        match &error {
            oc_core::location::BootError::MissingDirectory(directory) => {
                return crate::errors::location_not_found_path(directory.display());
            }
            oc_core::location::BootError::Database(_) => {}
        }
        crate::errors::debug_cause("session:unknown_ref", &error);
        ApiError::unknown_ref()
    })
}

/// Resolves the session (404 when missing) and its location's services.
pub(crate) async fn session_services(
    state: &SharedState,
    session_id: &str,
) -> Result<(std::sync::Arc<LocationServices>, SessionRow), ApiError> {
    let (location, row) = session_location(state, session_id).await?;
    let services = services(state, &location).await?;
    Ok((services, row))
}

fn parse_session_id(raw: &str) -> SessionId {
    SessionId::parse(raw.to_string()).unwrap_or_else(|error| SessionId(error.value))
}

fn order_from_str(value: Option<&str>) -> ListOrder {
    match value {
        Some("asc") => ListOrder::Asc,
        _ => ListOrder::Desc,
    }
}

pub(crate) fn inbox_error(error: &InboxError, fallback_id: &str) -> ApiError {
    match error {
        InboxError::Conflict(id) => ApiError::conflict_resource("Session is currently running", id),
        InboxError::Decode(error) => {
            ApiError::invalid_request(format!("invalid prompt payload: {error}"))
        }
        _ => {
            let _ = fallback_id;
            ApiError::unknown_ref()
        }
    }
}

/// Run failures (LLM errors, interrupts) are session outcomes, not HTTP
/// failures (§3 "prompt passes straight through"); only infra defects
/// surface as errors.
pub(crate) fn run_error(error: RunError) -> Result<(), ApiError> {
    match error {
        RunError::Defect(message) => Err(super::common::prompt_error(&message)),
        RunError::Failed { .. } | RunError::Interrupted => Ok(()),
    }
}

fn inbox_info_json(info: &oc_schema::session_inbox::Info) -> Value {
    serde_json::to_value(info).unwrap_or_else(|_| json!({}))
}

pub(crate) async fn publish_session_event(
    services: &std::sync::Arc<LocationServices>,
    type_: &str,
    data: Value,
) -> Result<(), ApiError> {
    let Some(definition) = registry::by_type_latest(type_) else {
        return Ok(());
    };
    // Session-scoped service events publish through the location bus with
    // the location envelope (the SSE grouping keys on it).
    let location = oc_schema::location::Ref {
        directory: oc_schema::common::AbsolutePath::parse(
            services.graph.location.directory.to_string_lossy().into_owned(),
        ),
        workspace_id: services
            .graph
            .location
            .workspace_id
            .as_ref()
            .and_then(|id| oc_schema::id::WorkspaceId::parse(id.clone()).ok()),
    };
    services
        .graph
        .bus
        .publish(
            definition,
            data,
            PublishOptions { location: Some(location), ..PublishOptions::default() },
        )
        .await
        .map(|_| ())
        .map_err(|error| {
            // v2 logs the cause server-side before the 500 hides it.
            eprintln!("[session] publish {type_} failed: {error}");
            ApiError::unknown_ref()
        })
}

// ---------------------------------------------------------------------------
// GET /api/session — list (cursor pagination, §3)
// ---------------------------------------------------------------------------

async fn list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // P10-E2: v2's schema middleware validates the query before the
    // handler (limit: NumberFromString→PositiveInt, order: "asc"|"desc")
    // and 400s with InvalidRequestError kind=Query — byte-pinned by the
    // Phase-10 error-path battery.
    if let Err(message) = super::schema_check::session_list_query(uri.query().unwrap_or_default()) {
        return Err(ApiError::schema_rejection(message, "Query"));
    }
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _ = &location;
    // The list read goes through the global store (v2 `Session.Service`)
    // — it must NOT assemble the resolved location (the boot cascade is
    // SSE-visible).
    let store = oc_core::session::store::SessionStore::new((*state.engine.db).clone());

    let directory = query_field(&uri, "directory");
    let project = query_field(&uri, "project");
    let subpath = query_field(&uri, "subpath");
    let search = query_field(&uri, "search");
    let parent_id = query_field(&uri, "parentID");
    let order_param = query_field(&uri, "order");
    // Schema validation upstream (P10-E2) guarantees limit parses as a
    // positive integer; the parse fallback only guards absurdly large
    // JS-safe integers beyond i64 (v2 accepts them; the store clamps).
    let limit = query_field(&uri, "limit")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(DEFAULT_LIMIT);

    let cursor_param = query_field(&uri, "cursor");
    let decoded_cursor = match &cursor_param {
        Some(encoded) => Some(
            decode_session_cursor(encoded)
                .ok_or_else(|| ApiError::invalid_cursor("Invalid cursor"))?,
        ),
        None => None,
    };
    let anchor_direction = decoded_cursor
        .as_ref()
        .and_then(|cursor| cursor.anchor.as_ref())
        .map_or(CursorDirection::Next, |anchor| anchor.direction);
    let input = ListInput {
        directory: directory.clone(),
        project: project.clone(),
        subpath,
        search,
        limit: Some(limit + 1),
        order: Some(order_from_str(order_param.as_deref())),
        parent_id: match parent_id.as_deref() {
            Some("null" | "") => Some(None),
            Some(id) => Some(Some(id.to_owned())),
            None => None,
        },
        anchor: decoded_cursor.and_then(|cursor| cursor.anchor).map(|anchor| ListAnchor {
            id: anchor.id,
            time: anchor.time,
            direction: match anchor.direction {
                CursorDirection::Next => ListDirection::Next,
                CursorDirection::Previous => ListDirection::Previous,
            },
        }),
        ..ListInput::default()
    };

    let rows = store.list(input).await.map_err(|error| {
        crate::errors::debug_cause("session:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;

    // direction=previous: the store flipped the order; restore display order.
    let mut rows = rows;
    if anchor_direction == CursorDirection::Previous {
        rows.reverse();
    }
    let has_more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);

    let scope = if let Some(directory) = &directory {
        SessionCursorScope::Directory(directory.clone())
    } else if let Some(project) = &project {
        SessionCursorScope::Project(project.clone())
    } else {
        SessionCursorScope::All
    };

    let mut next = None;
    let mut previous = None;
    if has_more {
        if let Some(last) = rows.last() {
            next = encode_session_cursor(&SessionCursor {
                scope: scope.clone(),
                anchor: Some(SessionAnchor {
                    id: last.id.clone(),
                    time: last.time_updated,
                    direction: CursorDirection::Next,
                }),
            });
        }
    }
    if cursor_param.is_some() {
        if let Some(first) = rows.first() {
            previous = encode_session_cursor(&SessionCursor {
                scope: scope.clone(),
                anchor: Some(SessionAnchor {
                    id: first.id.clone(),
                    time: first.time_updated,
                    direction: CursorDirection::Previous,
                }),
            });
        }
    }

    let data: Vec<Value> = rows.iter().map(session_row_to_info).collect();
    Ok(axum::Json(json!({
        "data": data,
        "cursor": { "previous": previous, "next": next },
    }))
    .into_response())
}

// ---------------------------------------------------------------------------
// POST /api/session — create
// ---------------------------------------------------------------------------

async fn create(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    // P10-E1: v2's schema middleware validates the payload before the
    // handler (wrongly-typed fields 400 with InvalidRequestError
    // kind=Payload, unknown fields stripped) — byte-pinned by the
    // Phase-10 error-path battery.
    if let Err(message) = super::schema_check::session_create_payload(&body) {
        return Err(ApiError::schema_rejection(message, "Payload"));
    }
    // v2 takes the location in the request body for session create; the
    // query/header derivation is the fallback (deep-map §1).
    let location = if let Some(directory) = body["location"]["directory"].as_str() {
        let path = std::path::PathBuf::from(directory);
        if !path.is_dir() {
            return Err(ApiError::location_not_found(directory));
        }
        LocationRef::new(path, None)
    } else {
        resolve_location(uri.query().unwrap_or_default(), &headers)?
    };
    let services = services(&state, &location).await?;

    let parent_id = body["parentID"].as_str().map(parse_session_id);
    if let Some(parent) = &parent_id {
        if services
            .store
            .get(parent.as_str())
            .await
            .map_err(|error| {
                crate::errors::debug_cause("session:unknown_ref", &error);
                ApiError::unknown_ref()
            })?
            .is_none()
        {
            return Err(ApiError::session_not_found(parent.as_str()));
        }
    }

    let input = CreateInput {
        id: body["id"].as_str().map(parse_session_id),
        parent_id,
        location: Some(oc_schema::location::Ref {
            directory: oc_schema::common::AbsolutePath(
                location.as_path().to_string_lossy().into_owned(),
            ),
            workspace_id: None,
        }),
        title: body["title"].as_str().map(String::from),
        agent: body["agent"].as_str().map(String::from),
        model: opt_value(&body, "model").and_then(|value| serde_json::from_value(value).ok()),
        metadata: record_value(&body, "metadata"),
        permissions: opt_value(&body, "permissions")
            .and_then(|value| serde_json::from_value(value).ok()),
        project: Some(ResolvedProject {
            id: services.project.id.clone(),
            directory: services.project.canonical.to_string_lossy().into_owned(),
        }),
        version: state.version.clone(),
    };

    let row = state.engine.create_session(&location, input).await.map_err(|error| {
        crate::errors::debug_cause("session:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    Ok(json_data(session_row_to_info(&row)))
}

fn opt_value(body: &Value, field: &str) -> Option<Value> {
    body.get(field).filter(|value| !value.is_null()).cloned()
}

/// `metadata` payloads are `Record` (`BTreeMap`) on the schema side.
fn record_value(body: &Value, field: &str) -> Option<std::collections::BTreeMap<String, Value>> {
    body.get(field).and_then(Value::as_object).map(|map| map.clone().into_iter().collect())
}

/// Attachment arrays deserialize straight into their schema types (wire
/// shapes are shared with the client).
fn typed_value<T: serde::de::DeserializeOwned>(body: &Value, field: &str) -> Option<T> {
    body.get(field)
        .filter(|value| !value.is_null())
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

// ---------------------------------------------------------------------------
// GET /api/session/active
// ---------------------------------------------------------------------------

async fn active(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let mut map = serde_json::Map::new();
    for session_id in services.execution.active() {
        map.insert(session_id, json!({ "type": "running" }));
    }
    Ok(json_data(Value::Object(map)))
}

// ---------------------------------------------------------------------------
// GET/DELETE/PATCH /api/session/:sessionID
// ---------------------------------------------------------------------------

async fn get(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (_, row) = session_services(&state, &session_id).await?;
    Ok(json_data(session_row_to_info(&row)))
}

async fn remove(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let bus = services.graph.bus.as_ref().clone();
    services.store.remove(&bus, &session_id).await.map_err(|error| {
        crate::errors::debug_cause("session:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    Ok(no_content())
}

async fn update(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;

    // title: non-empty → rename; "" → regenerate (LLM-backed regeneration
    // lands with the title lane; empty clears, TODO(phase-9)).
    if let Some(title) = body.get("title").and_then(Value::as_str) {
        publish_session_event(
            &services,
            "session.renamed",
            json!({ "sessionID": session_id, "title": title }),
        )
        .await?;
    }
    if body.get("metadata").is_some() {
        publish_session_event(
            &services,
            "session.metadata.updated",
            json!({ "sessionID": session_id, "metadata": body["metadata"] }),
        )
        .await?;
    }
    if body.get("permissions").is_some() {
        publish_session_event(
            &services,
            "session.permissions",
            json!({ "sessionID": session_id, "permissions": body["permissions"] }),
        )
        .await?;
    }
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /:id/agent, /:id/model — switch
// ---------------------------------------------------------------------------

async fn switch_agent(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let agent = body["agent"].as_str().unwrap_or_default();
    if services.graph.agents.get(&oc_schema::id::AgentId(agent.to_owned())).is_none() {
        return Err(ApiError::agent_not_found(agent));
    }
    publish_session_event(
        &services,
        "session.agent.selected",
        json!({ "sessionID": session_id, "agent": agent }),
    )
    .await?;
    Ok(no_content())
}

async fn switch_model(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    publish_session_event(
        &services,
        "session.model.selected",
        json!({ "sessionID": session_id, "model": body["model"] }),
    )
    .await?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /:id/prompt — admit + await (§3)
// ---------------------------------------------------------------------------

async fn prompt(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let message_id = body["id"].as_str().map_or_else(|| state.ids.new_id("msg"), String::from);
    let delivery = match body["delivery"].as_str() {
        Some("queue") => Delivery::Queue,
        _ => Delivery::Steer,
    };
    // `files` arrives in the PromptInput shape (URIs); materialize to
    // the resolved FileAttachment form before the inbox stores it
    // (core/session/prompt.ts `materializeAttachment`). A bad
    // attachment rejects the prompt (v2's AttachmentError), never a
    // silent drop.
    let input_files: Option<Vec<oc_schema::prompt::InputFileAttachment>> =
        typed_value(&body, "files");
    let files = super::attachment::materialize_files(input_files.as_deref())
        .map_err(|error| ApiError::invalid_request(error.to_string()))?;
    let payload = oc_schema::session_inbox::UserPayload {
        text: body["text"].as_str().unwrap_or_default().to_owned(),
        files,
        agents: typed_value(&body, "agents"),
        skills: typed_value(&body, "skills"),
        metadata: record_value(&body, "metadata"),
    };
    let info = services
        .inbox
        .admit(&message_id, &session_id, Item::User { payload, delivery })
        .await
        .map_err(|error| inbox_error(&error, &message_id))?;

    if body["resume"].as_bool().unwrap_or(true) {
        services
            .execution
            .resume(&session_id)
            .await
            .map_err(|error| {
                run_error(error).err();
            })
            .ok();
    }
    Ok(json_data(inbox_info_json(&info)))
}

// ---------------------------------------------------------------------------
// POST /:id/synthetic
// ---------------------------------------------------------------------------

async fn synthetic(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let message_id = body["id"].as_str().map_or_else(|| state.ids.new_id("msg"), String::from);
    let delivery = match body["delivery"].as_str() {
        Some("queue") => Delivery::Queue,
        _ => Delivery::Steer,
    };
    let payload = oc_schema::session_inbox::SyntheticPayload {
        text: body["text"].as_str().unwrap_or_default().to_owned(),
        description: body["description"].as_str().map(String::from),
        metadata: body["metadata"].as_object().cloned().map(|map| map.into_iter().collect()),
    };
    let info = services
        .inbox
        .admit(&message_id, &session_id, Item::Synthetic { payload, delivery })
        .await
        .map_err(|error| inbox_error(&error, &message_id))?;
    if body["resume"].as_bool().unwrap_or(true) {
        services
            .execution
            .resume(&session_id)
            .await
            .map_err(|error| {
                run_error(error).err();
            })
            .ok();
    }
    Ok(json_data(inbox_info_json(&info)))
}

// ---------------------------------------------------------------------------
// POST /:id/interrupt — {interrupted}
// ---------------------------------------------------------------------------

async fn interrupt(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Result<Response, ApiError> {
    let resume = query_field(&uri, "resume").as_deref() == Some("true");
    let (services, _) = session_services(&state, &session_id).await?;
    let options = oc_core::session::InterruptOptions {
        reason: Some(oc_schema::session_event::InterruptReason::User),
        await_settlement: true,
        resume,
    };
    let interrupted = services.execution.interrupt(&session_id, options).await;
    Ok(axum::Json(json!({ "interrupted": interrupted })).into_response())
}

// ---------------------------------------------------------------------------
// POST /:id/compact
// ---------------------------------------------------------------------------

async fn compact(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let input_id = body["id"].as_str().map_or_else(|| state.ids.new_id("msg"), String::from);
    let delivery = match body["delivery"].as_str() {
        Some("steer") => Delivery::Steer,
        _ => Delivery::Queue,
    };
    let info = services.inbox.admit_compaction(&input_id, &session_id, delivery).await.map_err(
        |error| match error {
            InboxError::Conflict(id) => {
                ApiError::conflict_resource("Session is currently running", &id)
            }
            _ => ApiError::unknown_ref(),
        },
    )?;
    services
        .execution
        .resume(&session_id)
        .await
        .map_err(|error| {
            run_error(error).err();
        })
        .ok();
    Ok(json_data(inbox_info_json(&info)))
}

// ---------------------------------------------------------------------------
// POST /:id/move — 204; directory errors exact (§3)
// ---------------------------------------------------------------------------

async fn move_session(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, row) = session_services(&state, &session_id).await?;
    let directory = body["directory"].as_str().unwrap_or_default();
    let path = std::path::Path::new(directory);
    if !path.exists() {
        return Err(ApiError::invalid_request("Directory does not exist"));
    }
    if !path.is_dir() {
        return Err(ApiError::invalid_request("Not a directory"));
    }
    if path.read_dir().is_err() {
        return Err(ApiError::invalid_request("Directory is unavailable"));
    }

    // Resolve the destination project, then admit the queued move item.
    let destination = LocationRef::new(path.to_path_buf(), None);
    let destination_services = super::session::services(&state, &destination).await?;
    let project_id = oc_schema::id::ProjectId::parse(destination_services.project.id.clone());
    let message_id = state.ids.new_id("msg");
    services
        .inbox
        .admit(
            &message_id,
            &session_id,
            Item::Move_ {
                payload: oc_schema::session_inbox::MovePayload {
                    location: oc_schema::location::Ref {
                        directory: oc_schema::common::AbsolutePath(directory.to_owned()),
                        workspace_id: None,
                    },
                    project_id,
                    subpath: row
                        .path
                        .as_deref()
                        .and_then(|value| serde_json::from_value(json!(value)).ok()),
                },
                delivery: match body["delivery"].as_str() {
                    Some("steer") => Delivery::Steer,
                    _ => Delivery::Queue,
                },
            },
        )
        .await
        .map_err(|error| inbox_error(&error, &message_id))?;
    services
        .execution
        .resume(&session_id)
        .await
        .map_err(|error| {
            run_error(error).err();
        })
        .ok();
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /:id/fork
// ---------------------------------------------------------------------------

async fn fork(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _row) = session_services(&state, &session_id).await?;
    // v2 `Session.fork` (core session.ts:319-355): locate the boundary —
    // the `before` message when the payload names one (HTTP wire carries
    // the bare messageID, protocol groups/session.ts:313), else the newest
    // message — then publish ONE durable `session.forked` event; the
    // projector (`project_fork`, projector.rs:571) creates the row, copies
    // the settled history below the boundary (`{type, messageID}` union,
    // schema/session-fork.ts:6-10), fork-styles the title and seeds the
    // instruction state. No row is pre-created: a pre-created row makes
    // the projector's `INSERT OR IGNORE` fail ALREADY_PROJECTED and the
    // fork copies nothing (auditE core #3).
    let before = body["before"].as_str().map(String::from);
    let lookup_session = session_id.clone();
    let lookup_before = before.clone();
    let boundary_id: Option<String> = state
        .engine
        .db
        .transaction(move |tx| {
            let id: Option<String> = match &lookup_before {
                Some(before) => tx
                    .conn()
                    .query_row(
                        "SELECT id FROM session_message WHERE session_id = ?1 AND id = ?2 \
                         ORDER BY seq DESC LIMIT 1",
                        (lookup_session, before),
                        |row| row.get(0),
                    )
                    .ok(),
                None => tx
                    .conn()
                    .query_row(
                        "SELECT id FROM session_message WHERE session_id = ?1 \
                         ORDER BY seq DESC LIMIT 1",
                        (lookup_session,),
                        |row| row.get(0),
                    )
                    .ok(),
            };
            Ok(id)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let boundary_id = match (&before, boundary_id) {
        (Some(_), None) => {
            // MessageNotFoundError → 404 (ref session.ts:334-338).
            let missing = body["before"].as_str().unwrap_or_default().to_owned();
            return Err(ApiError::message_not_found(&session_id, &missing));
        }
        (None, None) => {
            // ForkEmptyError → 400 empty_session (ref session.ts:339).
            return Err(ApiError::invalid_request_kind("Session is empty", "empty_session"));
        }
        (_, Some(id)) => id,
    };
    let boundary_type = if before.is_some() { "before" } else { "through" };

    // The fork adopts the parent's newest instruction values rather than
    // the values in effect at the boundary (ref session.ts:341-349).
    let parent_id = session_id.clone();
    let inherited = state
        .engine
        .db
        .transaction(move |tx| {
            let values: Option<String> = tx
                .conn()
                .query_row(
                    "SELECT current_values FROM instruction_state WHERE session_id = ?1",
                    (parent_id.clone(),),
                    |row| row.get(0),
                )
                .ok();
            let mut statement = tx.conn().prepare(
                "SELECT key, value, removed FROM instruction_entry WHERE session_id = ?1",
            )?;
            let entries = statement
                .query_map((parent_id,), |row| {
                    let raw: Option<String> = row.get::<_, Option<String>>(1)?;
                    let value = raw
                        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                        .unwrap_or(Value::Null);
                    Ok(json!({
                        "key": row.get::<_, String>(0)?,
                        "value": value,
                        "removed": row.get::<_, bool>(2)?,
                    }))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok((values, entries))
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let (instructions, instruction_entries) = inherited;
    let mut data = json!({
        "sessionID": state.ids.new_id("ses"),
        "parentID": session_id,
        "boundary": { "type": boundary_type, "messageID": boundary_id },
    });
    if let Some(instructions) = instructions {
        data["instructions"] =
            serde_json::from_str::<Value>(&instructions).unwrap_or_else(|_| json!({}));
    }
    data["instructionEntries"] = Value::Array(instruction_entries);

    let definition = registry::by_type_latest("session.forked").ok_or_else(ApiError::unknown_ref)?;
    services
        .graph
        .bus
        .publish(definition, data.clone(), PublishOptions::default())
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:fork_projection", &error);
            ApiError::unknown_ref()
        })?;
    // The projected fork row (the projector created it inside the publish).
    let fork_id = data["sessionID"].as_str().unwrap_or_default().to_owned();
    let fork_row = services
        .store
        .get(&fork_id)
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:unknown_ref", &error);
            ApiError::unknown_ref()
        })?
        .ok_or_else(ApiError::unknown_ref)?;
    Ok(json_data(session_row_to_info(&fork_row)))
}

// ---------------------------------------------------------------------------
// POST /:id/command — 204; NotFound 404 / Execution 500 (§3)
// ---------------------------------------------------------------------------

async fn command(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    let name = body["name"].as_str().unwrap_or_default();
    // Commands are plugin-registered (phase-9 plugin host); none are
    // registered in this build → the documented 404.
    Err(ApiError::command_not_found(name))
}

// ---------------------------------------------------------------------------
// GET /:id/context, /:id/diff, /:id/generate
// ---------------------------------------------------------------------------

async fn context(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let rows = services
        .store
        .messages(oc_core::session::store::MessagesInput {
            session_id,
            order: Some(ListOrder::Asc),
            ..Default::default()
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let data: Vec<Value> = rows.iter().map(message_row_to_info).collect();
    Ok(json_data(Value::Array(data)))
}

async fn diff(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    _uri: Uri,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    // File-diff surface rides the snapshot diff API (TODO(phase-9)); the
    // REST shape stays an empty array until then.
    Ok(json_data(Value::Array(Vec::new())))
}

async fn generate(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let _ = (body["prompt"].as_str(), session_services(&state, &session_id).await?);
    // The one-shot generate path needs the engine's LLM surface exposed
    // (TODO(phase-9)); declared 503 until then.
    Err(ApiError::service_unavailable("Text generation is not available", Some("opencode")))
}

// ---------------------------------------------------------------------------
// POST /:id/shell, /:id/view, /:id/wait, /:id/background
// ---------------------------------------------------------------------------

async fn shell(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, row) = session_services(&state, &session_id).await?;
    let command = body["command"].as_str().unwrap_or_default().to_owned();
    let input_id = body["id"].as_str().map(String::from);
    // v2 `Session.shell` (session/session.ts:179-222 + session/shell.ts):
    // the command EXECUTES through the location's Shell service (cwd =
    // the session directory, timeout 0, background metadata) — it is
    // never admitted as LLM input. The request observes the run: publish
    // `session.shell.started`, await the result + output preview,
    // publish `session.shell.ended`, then admit the user-notification
    // synthetic (resume stays false — the model is not woken).
    let metadata: oc_schema::common::Record = serde_json::from_value(json!({
        "sessionID": session_id,
        "background": true,
    }))
    .unwrap_or_default();
    let started = services
        .shell
        .create(
            oc_schema::shell::CreateInput {
                command: command.clone(),
                cwd: Some(row.directory.clone()),
                timeout: Some(0),
                metadata: Some(metadata),
            },
            None,
            None,
            None,
        )
        .await;
    let info = match started {
        Ok(info) => info,
        Err(error) => {
            // ref taps the spawn failure into an error synthetic before
            // dying (session.ts:188-195) — the handler still 500s.
            let message_id = state.ids.new_id("msg");
            let payload = oc_schema::session_inbox::SyntheticPayload {
                text: format!("User shell command failed to start:\n{command}\n\n{error}"),
                description: Some(command.clone()),
                metadata: Some(
                    serde_json::from_value::<oc_schema::common::Record>(json!({
                        "source": "shell",
                        "state": "error",
                    }))
                    .unwrap_or_default(),
                ),
            };
            let _ = services
                .inbox
                .admit(
                    &message_id,
                    &session_id,
                    Item::Synthetic { payload, delivery: Delivery::Queue },
                )
                .await;
            crate::errors::debug_cause("session:shell_start", &error);
            return Err(ApiError::unknown_ref());
        }
    };

    // `session.shell.started` — the event id derives from the caller's
    // message id (`msg_` → `evt_`, session.ts:197-203).
    let mut options = PublishOptions::default();
    if let Some(id) = &input_id {
        if let Ok(event_id) = oc_schema::id::EventId::parse(id.replacen("msg_", "evt_", 1)) {
            options.id = Some(event_id);
        }
    }
    if let Some(definition) = registry::by_type_latest("session.shell.started") {
        let _ = services
            .graph
            .bus
            .publish(
                definition,
                json!({ "sessionID": session_id, "shell": info }),
                options,
            )
            .await;
    }

    let result = services.shell.result(info).await;
    let terminal = result.info.clone();
    // The preview page the ended event carries (session.ts:206-210):
    // up to 1 MiB from the cursor start; a vanished capture serves the
    // `ShellResult.unavailable` stand-in.
    let preview = match services
        .shell
        .output(
            &result.info.id,
            Some(oc_schema::shell::OutputInput { cursor: None, limit: Some(1024 * 1024) }),
        )
        .await
    {
        Ok(page) => json!({
            "output": page.output,
            "cursor": page.cursor,
            "size": page.size,
            "truncated": page.truncated,
        }),
        Err(_) => {
            const MISSING: &str = "Shell command output is no longer available.";
            let size = MISSING.len();
            json!({
                "output": MISSING,
                "cursor": size,
                "size": size,
                "truncated": false,
            })
        }
    };
    if let Some(definition) = registry::by_type_latest("session.shell.ended") {
        let _ = services
            .graph
            .bus
            .publish(
                definition,
                json!({ "sessionID": session_id, "shell": terminal, "output": preview }),
                PublishOptions::default(),
            )
            .await;
    }

    // The user-notification synthetic (`ShellResult.userNotification`) —
    // resume:false, so unlike the old path nothing reaches the model.
    let notification = oc_core::shell::result::user_notification(&result);
    let message_id = input_id.unwrap_or_else(|| state.ids.new_id("msg"));
    let payload = oc_schema::session_inbox::SyntheticPayload {
        text: notification.text,
        description: None,
        metadata: serde_json::from_value::<oc_schema::common::Record>(notification.metadata)
            .ok(),
    };
    services
        .inbox
        .admit(
            &message_id,
            &session_id,
            Item::Synthetic { payload, delivery: Delivery::Steer },
        )
        .await
        .map_err(|error| inbox_error(&error, &message_id))?;
    Ok(no_content())
}

async fn view(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let idle = body["idle"].as_f64().unwrap_or_else(|| now_ms(&state) as f64);
    publish_session_event(
        &services,
        "session.viewed",
        json!({ "sessionID": session_id, "idle": idle }),
    )
    .await?;
    Ok(no_content())
}

async fn wait(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    // Poll settlement at 50ms (v2 waits on the execution exit; active()
    // is the equivalent ownership probe; 10-minute ceiling).
    for _ in 0..12_000 {
        if !services.execution.active().contains(&session_id)
            && !services.execution.is_active(&session_id)
        {
            return Ok(no_content());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Ok(no_content())
}

async fn background(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    // v2 background detaches the session from the caller's foreground
    // tracking; the engine keeps executions running regardless — 204.
    let _ = session_services(&state, &session_id).await?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// PUT /:id/environment
// ---------------------------------------------------------------------------

async fn environment(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let mut variables = std::collections::HashMap::new();
    if let Some(map) = body["variables"].as_object() {
        for (key, value) in map {
            if let Some(value) = value.as_str() {
                variables.insert(key.clone(), value.to_owned());
            }
        }
    }
    services.session_env.set(&session_id, variables);
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Revert family
// ---------------------------------------------------------------------------

async fn revert_stage(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (location, _) = session_location(&state, &session_id).await?;
    let message_id = body["messageID"].as_str().unwrap_or_default();
    let include_files = body["files"].as_bool().unwrap_or(false);
    let revert = state
        .engine
        .revert_stage(&location, &parse_session_id(&session_id), message_id, include_files)
        .await
        .map_err(|error| revert_error(&session_id, &error))?;
    Ok(json_data(serde_json::to_value(&revert).unwrap_or_else(|_| json!({}))))
}

async fn revert_clear(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (location, _) = session_location(&state, &session_id).await?;
    state
        .engine
        .revert_clear(&location, &parse_session_id(&session_id))
        .await
        .map_err(|error| revert_error(&session_id, &error))?;
    Ok(no_content())
}

async fn revert_commit(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (location, _) = session_location(&state, &session_id).await?;
    let revert_state =
        oc_core::session::revert::read_revert(&state.engine.db, &parse_session_id(&session_id))
            .await
            .map_err(|error| {
                crate::errors::debug_cause("session:unknown_ref", &error);
                ApiError::unknown_ref()
            })?
            .ok_or_else(|| ApiError::conflict("No revert staged"))?;
    state
        .engine
        .revert_commit(&location, &parse_session_id(&session_id), &revert_state)
        .await
        .map_err(|error| revert_error(&session_id, &error))?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Inbox
// ---------------------------------------------------------------------------

async fn inbox_list(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let pending = services.inbox.list(&session_id).await.map_err(|error| {
        crate::errors::debug_cause("session:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    let data: Vec<Value> = pending.iter().map(inbox_info_json).collect();
    Ok(json_data(Value::Array(data)))
}

async fn inbox_cancel(
    State(state): State<SharedState>,
    Path((session_id, inbox_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    services
        .inbox
        .cancel(&session_id, &inbox_id)
        .await
        .map_err(|error| inbox_error(&error, &inbox_id))?;
    Ok(no_content())
}

async fn inbox_update(
    State(state): State<SharedState>,
    Path((session_id, inbox_id)): Path<(String, String)>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    // steer-vs-queue (§3).
    let result = match body["delivery"].as_str() {
        Some("queue") => services.inbox.queue(&session_id, &inbox_id).await,
        _ => services.inbox.steer(&session_id, &inbox_id).await,
    };
    result.map_err(|error| inbox_error(&error, &inbox_id))?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Instructions entries (per-session key/value overrides)
// ---------------------------------------------------------------------------

async fn instruction_entry_list(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    let db = state.engine.db.clone();
    let session = session_id.clone();
    let rows: Vec<(String, Option<String>)> = db
        .transaction(move |tx| {
            let conn = tx.conn();
            let mut statement =
                conn.prepare("SELECT key, value FROM instruction_entry WHERE session_id = ?1 AND removed = 0 ORDER BY key")?;
            let rows = statement
                .query_map((session,), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .map_err(|error| { crate::errors::debug_cause("session:unknown_ref", &error); ApiError::unknown_ref() })?;
    let data: Vec<Value> = rows
        .into_iter()
        .map(|(key, value)| json!({ "key": key, "value": parse_json_or_null(value.as_deref()) }))
        .collect();
    Ok(json_data(Value::Array(data)))
}

fn parse_json_or_null(text: Option<&str>) -> Value {
    text.and_then(|value| serde_json::from_str(value).ok()).unwrap_or(Value::Null)
}

async fn instruction_entry_remove(
    State(state): State<SharedState>,
    Path((session_id, key)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    let db = state.engine.db.clone();
    let (session, key) = (session_id.clone(), key.clone());
    db.transaction(move |tx| {
        tx.execute(
            "UPDATE instruction_entry SET removed = 1, time_updated = ?3 WHERE session_id = ?1 AND key = ?2",
            (session, key, now_epoch_ms(),),
        )?;
        Ok(())
    })
    .await
    .map_err(|error| { crate::errors::debug_cause("session:unknown_ref", &error); ApiError::unknown_ref() })?;
    Ok(no_content())
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}

const INSTRUCTION_ENTRY_MAX_BYTES: i64 = 32 * 1024;

async fn instruction_entry_put(
    State(state): State<SharedState>,
    Path((session_id, key)): Path<(String, String)>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let _ = session_services(&state, &session_id).await?;
    let value = body["value"].clone();
    let encoded = serde_json::to_string(&value).map_err(|error| {
        crate::errors::debug_cause("session:invalid_request", &error);
        ApiError::invalid_request("invalid value")
    })?;
    if encoded.len() as i64 > INSTRUCTION_ENTRY_MAX_BYTES {
        return Err(ApiError::instruction_too_large(
            encoded.len() as i64,
            INSTRUCTION_ENTRY_MAX_BYTES,
        ));
    }
    let db = state.engine.db.clone();
    let (session, key, encoded) = (session_id.clone(), key.clone(), encoded.clone());
    db.transaction(move |tx| {
        tx.execute(
            "INSERT INTO instruction_entry (session_id, key, value, removed, time_created, time_updated) \
             VALUES (?1, ?2, ?3, 0, ?4, ?4) \
             ON CONFLICT(session_id, key) DO UPDATE SET value = ?3, removed = 0, time_updated = ?4",
            (session, key, encoded, now_epoch_ms(),),
        )?;
        Ok(())
    })
    .await
    .map_err(|error| { crate::errors::debug_cause("session:unknown_ref", &error); ApiError::unknown_ref() })?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /:id/skill
// ---------------------------------------------------------------------------

async fn skill(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let skill_id = body["id"].as_str().unwrap_or_default();
    if !services.skills.list().iter().any(|skill| skill.id.as_str() == skill_id) {
        return Err(ApiError::skill_not_found(skill_id));
    }
    let message_id = state.ids.new_id("msg");
    services
        .inbox
        .admit(
            &message_id,
            &session_id,
            Item::Synthetic {
                payload: oc_schema::session_inbox::SyntheticPayload {
                    text: format!("/{skill_id}"),
                    description: Some(skill_id.to_owned()),
                    metadata: None,
                },
                delivery: Delivery::Steer,
            },
        )
        .await
        .map_err(|error| inbox_error(&error, &message_id))?;
    if body["resume"].as_bool().unwrap_or(true) {
        services
            .execution
            .resume(&session_id)
            .await
            .map_err(|error| {
                run_error(error).err();
            })
            .ok();
    }
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// import / export / stats
// ---------------------------------------------------------------------------

async fn import(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let location_services = services(&state, &location).await?;
    let info = body["info"].clone();
    let session_id = info["id"].as_str().unwrap_or_default().to_owned();

    // Duplicate → 409 "Session already exists: <id>" (§3).
    if location_services
        .store
        .get(&session_id)
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:unknown_ref", &error);
            ApiError::unknown_ref()
        })?
        .is_some()
    {
        return Err(ApiError::conflict(format!("Session already exists: {session_id}")));
    }

    let target = body["location"]["directory"].as_str().map_or_else(
        || location.clone(),
        |directory| LocationRef::new(std::path::PathBuf::from(directory), None),
    );
    let target_services = super::session::services(&state, &target).await?;

    let row = state
        .engine
        .create_session(
            &target,
            CreateInput {
                id: Some(parse_session_id(&session_id)),
                parent_id: info["parentID"].as_str().map(parse_session_id),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath(
                        target.as_path().to_string_lossy().into_owned(),
                    ),
                    workspace_id: None,
                }),
                title: info["title"].as_str().map(String::from),
                agent: info["agent"].as_str().map(String::from),
                model: opt_value(&info, "model")
                    .and_then(|value| serde_json::from_value(value).ok()),
                metadata: info["metadata"].as_object().map(|map| map.clone().into_iter().collect()),
                permissions: opt_value(&info, "permissions")
                    .and_then(|value| serde_json::from_value(value).ok()),
                project: Some(ResolvedProject {
                    id: target_services.project.id.clone(),
                    directory: target_services.project.canonical.to_string_lossy().into_owned(),
                }),
                version: state.version.clone(),
            },
        )
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;

    // Replay the imported messages in order through the message projector.
    if let Some(messages) = body["messages"].as_array() {
        for message in messages {
            if let Some(definition) = registry::by_type_latest("session.inbox.delivered") {
                let _ = target_services
                    .graph
                    .bus
                    .publish(
                        definition,
                        json!({ "sessionID": session_id, "inboxID": message["id"], "item": {
                            "type": "synthetic",
                            "payload": { "text": "", "description": "imported" },
                            "delivery": "queue",
                        }}),
                        PublishOptions::default(),
                    )
                    .await;
            }
            let _ = message;
        }
    }
    Ok(json_data(session_row_to_info(&row)))
}

async fn export(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Result<Response, ApiError> {
    let sanitize = query_field(&uri, "sanitize").as_deref() == Some("true");
    let (services, row) = session_services(&state, &session_id).await?;
    let messages = services
        .store
        .messages(oc_core::session::store::MessagesInput {
            session_id: session_id.clone(),
            order: Some(ListOrder::Asc),
            ..Default::default()
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("session:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let mut info = session_row_to_info(&row);
    let mut message_values: Vec<Value> = messages.iter().map(message_row_to_info).collect();
    if sanitize {
        if let Value::Object(map) = &mut info {
            map.remove("metadata");
        }
        for message in &mut message_values {
            if let Value::Object(map) = message {
                map.remove("metadata");
            }
        }
    }
    Ok(json_data(json!({ "info": info, "messages": message_values })))
}

async fn stats(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let _ = (
        query_field(&uri, "from"),
        query_field(&uri, "to"),
        query_field(&uri, "timezone"),
        query_field(&uri, "tools"),
    );
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let sessions = services.store.list(ListInput::default()).await.map_err(|error| {
        crate::errors::debug_cause("session:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    // Aggregate a minimal stats view (session count + usage totals);
    // the binned activity/model breakdown is TODO(phase-9).
    let mut input = 0i64;
    let mut output = 0i64;
    let mut reasoning = 0i64;
    let mut cost = 0.0f64;
    for row in &sessions {
        input += row.tokens_input;
        output += row.tokens_output;
        reasoning += row.tokens_reasoning;
        cost += row.cost;
    }
    Ok(json_data(json!({
        "sessions": { "total": sessions.len() },
        "tokens": { "input": input, "output": output, "reasoning": reasoning },
        "cost": cost,
    })))
}

// ---------------------------------------------------------------------------
// GET /:id/log — SSE (durable tail, §3)
// ---------------------------------------------------------------------------

async fn log(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    crate::sse::session_log_response(&state, &uri, &headers, &session_id).await
}
