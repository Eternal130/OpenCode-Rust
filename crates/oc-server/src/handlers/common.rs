//! Handler-shared plumbing: the error facade over the shell lane's
//! `errors.rs` registry, response envelopes, location resolution (riding
//! `middleware::request_location`), and the cursor codecs for session /
//! message pagination.
//!
//! Wire shapes are byte-identical to the shell lane's registry — the
//! inherent-impl facade here only adds ergonomic one-argument
//! constructors for the handler modules.

#![allow(clippy::must_use_candidate, clippy::return_self_not_must_use)]
use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use oc_core::location::LocationRef;
use serde_json::{json, Value};

pub use crate::errors::ApiError;
pub type SharedState = std::sync::Arc<crate::state::AppState>;

// The axum `IntoResponse` bridge for ApiError lives in `errors.rs` next to
// the inherent renderer (one impl crate-wide; Phase 5 integration).

// The facade mirrors the shell registry's free functions (which carry
// their own lint posture); constructor candidates are consumed
// immediately at every call site.
#[allow(clippy::must_use_candidate, clippy::return_self_not_must_use)]
impl ApiError {
    // 400s
    pub fn invalid_request(message: impl Into<String>) -> Self {
        crate::errors::invalid_request(message)
    }
    pub fn invalid_request_kind(message: impl Into<String>, kind: &str) -> Self {
        crate::errors::invalid_request_kind(message, kind)
    }
    pub fn invalid_cursor(message: impl Into<String>) -> Self {
        crate::errors::invalid_cursor(message)
    }
    pub fn rpc(message: impl Into<String>, error_type: impl Into<String>) -> Self {
        crate::errors::rpc_error(error_type, message)
    }
    /// Attachment failures spread a `files` array (§3).
    pub fn attachment(files: Value) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, spread_tag("InvalidRequestError", files))
    }
    pub fn form_invalid(message: impl Into<String>) -> Self {
        crate::errors::invalid_request(message)
    }
    pub fn form_invalid_answer(id: &str, message: impl Into<String>) -> Self {
        crate::errors::form_invalid_answer(id, message)
    }
    pub fn instruction_too_large(actual: i64, max: i64) -> Self {
        ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({
                "_tag": "InstructionEntryValueTooLargeError",
                "message": "Instruction entry value too large",
                "actualBytes": actual,
                "maxBytes": max,
            }),
        )
    }

    // 401 / 403
    pub fn unauthorized(message: impl Into<String>) -> Self {
        crate::errors::unauthorized(message)
    }
    pub fn forbidden(message: impl Into<String>) -> Self {
        crate::errors::forbidden(message)
    }

    // 404s
    pub fn session_not_found(session_id: &str) -> Self {
        crate::errors::session_not_found(session_id)
    }
    pub fn message_not_found(session_id: &str, message_id: &str) -> Self {
        crate::errors::message_not_found(session_id, message_id, "Message not found")
    }
    pub fn location_not_found(directory: &str) -> Self {
        crate::errors::location_not_found_path(directory)
    }
    pub fn agent_not_found(agent_id: &str) -> Self {
        crate::errors::agent_not_found(agent_id, "Agent not found")
    }
    pub fn provider_not_found(provider_id: &str) -> Self {
        crate::errors::provider_not_found(provider_id, "Provider not found")
    }
    pub fn project_not_found(project_id: &str) -> Self {
        crate::errors::project_not_found(project_id, "Project not found")
    }
    pub fn file_not_found(path: &str) -> Self {
        crate::errors::file_not_found(path, "File not found")
    }
    pub fn skill_not_found(skill: &str) -> Self {
        crate::errors::skill_not_found(skill, "Skill not found")
    }
    pub fn mcp_server_not_found(server: &str) -> Self {
        crate::errors::mcp_server_not_found(server, "MCP server not found")
    }
    pub fn command_not_found(command: &str) -> Self {
        crate::errors::command_not_found(command, "Command not found")
    }
    pub fn permission_not_found(request_id: &str) -> Self {
        crate::errors::permission_not_found(request_id, "Permission request not found")
    }
    pub fn form_not_found(id: &str) -> Self {
        crate::errors::form_not_found(id, "Form not found")
    }
    pub fn pty_not_found(pty_id: &str) -> Self {
        crate::errors::pty_not_found(pty_id, "Pty not found")
    }
    pub fn shell_not_found(id: &str) -> Self {
        crate::errors::shell_not_found(id, "Shell not found")
    }
    pub fn integration_not_found(integration_id: &str) -> Self {
        crate::errors::integration_not_found(integration_id, "Integration not found")
    }
    pub fn integration_attempt_not_found(integration_id: &str, attempt_id: &str) -> Self {
        crate::errors::integration_attempt_not_found(
            integration_id,
            attempt_id,
            "Integration attempt not found",
        )
    }
    pub fn integration_method_not_found(integration_id: &str, method_id: &str) -> Self {
        crate::errors::integration_method_not_found(
            integration_id,
            method_id,
            "Integration method not found",
        )
    }

    // 409s
    pub fn conflict(message: impl Into<String>) -> Self {
        crate::errors::conflict(message)
    }
    pub fn conflict_resource(message: impl Into<String>, resource: &str) -> Self {
        crate::errors::conflict_resource(message, resource)
    }
    pub fn session_busy(session_id: &str) -> Self {
        crate::errors::session_busy(session_id, "Session is busy")
    }
    pub fn form_already_settled(id: &str) -> Self {
        crate::errors::form_already_settled(id, "Form already settled")
    }

    // 500s
    pub fn command_execution(command: &str, message: impl Into<String>) -> Self {
        crate::errors::command_execution_error(command, message)
    }
    pub fn rpc_internal(message: impl Into<String>, error_type: impl Into<String>) -> Self {
        crate::errors::rpc_internal(error_type, message)
    }
    pub fn unknown_ref() -> Self {
        crate::errors::unknown_with_ref()
    }
    pub fn unknown() -> Self {
        crate::errors::unknown_with_ref()
    }
    /// The `SchemaError` middleware outcome for payload/query rejections
    /// (`{message, kind}`, no `field` — P10-E1/E2).
    pub fn schema_rejection(reason: impl Into<String>, kind: &str) -> Self {
        crate::errors::schema_rejection(&reason.into(), kind)
    }

    // 503 / 501 / 400-worktree
    pub fn service_unavailable(message: impl Into<String>, service: Option<&str>) -> Self {
        match service {
            Some(service) => crate::errors::service_unavailable_named(message, service),
            None => crate::errors::service_unavailable(message),
        }
    }
    pub fn vcs_init_not_supported(provider_id: &str) -> Self {
        ApiError::new(
            StatusCode::NOT_IMPLEMENTED,
            json!({
                "_tag": "VcsInitNotSupportedError",
                "providerID": provider_id,
                "message": "VCS init not supported",
            }),
        )
    }
    pub fn worktree(name: &str, message: impl Into<String>, force_required: Option<bool>) -> Self {
        let mut data = json!({ "message": message.into() });
        if let (Some(force), Value::Object(map)) = (force_required, &mut data) {
            map.insert("forceRequired".to_owned(), json!(force));
        }
        ApiError::new(
            StatusCode::BAD_REQUEST,
            json!({ "_tag": "WorktreeError", "name": name, "data": data }),
        )
    }
}

fn spread_tag(tag: &str, fields: Value) -> Value {
    let mut object = BTreeMap::new();
    object.insert("_tag".to_owned(), json!(tag));
    if let Value::Object(map) = fields {
        for (key, value) in map {
            object.insert(key, value);
        }
    }
    Value::Object(object.into_iter().collect())
}

/// Handler result alias.
pub type ApiResult = Result<Response, ApiError>;

/// The state clock's now (ms).
pub fn now_ms(state: &SharedState) -> u64 {
    u64::try_from(state.clock.unix_ms()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Response envelopes (§3)
// ---------------------------------------------------------------------------
// Builders are consumed immediately at handler call sites.

/// `{ data }`-enveloped 200 JSON.
#[allow(clippy::needless_pass_by_value)] // builder API: the Value moves into the envelope
pub fn json_data(data: Value) -> Response {
    axum::Json(json!({ "data": data })).into_response()
}

/// `{ location: { directory }, data }`-enveloped 200 JSON.
#[allow(clippy::needless_pass_by_value)] // builder API: the Value moves into the envelope
pub fn json_location_data(directory: &str, data: Value) -> Response {
    axum::Json(json!({ "location": { "directory": directory }, "data": data })).into_response()
}

/// 204 No Content, truly empty body.
pub fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

/// Raw bytes with a content type (fs.read).
pub fn bytes_response(bytes: Vec<u8>, content_type: &str) -> Response {
    ([(axum::http::header::CONTENT_TYPE, content_type.to_owned())], bytes).into_response()
}

// ---------------------------------------------------------------------------
// Location resolution (§1) — rides the shell lane's derivation
// ---------------------------------------------------------------------------

/// Resolves the request location through `middleware::request_location`
/// (query `location[directory]` → `x-opencode-directory` → cwd) and
/// validates the directory exists (404 `LocationNotFoundError`).
#[allow(clippy::must_use_candidate)]
pub fn resolve_location(query: &str, headers: &HeaderMap) -> Result<LocationRef, ApiError> {
    let reference = crate::middleware::request_location(
        &axum::http::Uri::try_from(format!("http://localhost/?{query}"))
            .unwrap_or_else(|_| axum::http::Uri::from_static("http://localhost/")),
        headers,
    );
    let directory = reference.directory.as_str().to_owned();
    let path = std::path::PathBuf::from(&directory);
    if !path.is_dir() {
        return Err(ApiError::location_not_found(&directory));
    }
    Ok(LocationRef::new(path, reference.workspace_id.map(|value| value.0)))
}

/// Extracts `name=value` from a raw query string (raw percent-encoded).
pub fn parse_query_field(query: &str, name: &str) -> Option<String> {
    crate::auth::query_param(Some(query), name)
}

/// Percent-decodes a query/header value (malformed → raw, §1).
pub fn percent_decode(input: &str) -> String {
    crate::auth::percent_decode(input)
}

// ---------------------------------------------------------------------------
// Session-row → location (§1 "session routes resolve the session row")
// ---------------------------------------------------------------------------

/// Resolves a session's location from its row. The row read goes through
/// the engine's global database directly (v2's `SessionLocationMiddleware`
/// resolves via the global `Session.Service`) — it must NOT assemble the
/// process-cwd location, whose boot cascade is SSE-visible.
pub async fn session_location(
    state: &SharedState,
    session_id: &str,
) -> Result<(LocationRef, oc_core::database::schema::SessionRow), ApiError> {
    let store = oc_core::session::store::SessionStore::new((*state.engine.db).clone());
    let row = store
        .get(session_id)
        .await
        .map_err(|error| {
            crate::errors::debug_cause("common:unknown_ref", &error);
            ApiError::unknown_ref()
        })?
        .ok_or_else(|| ApiError::session_not_found(session_id))?;
    let location =
        LocationRef::new(std::path::PathBuf::from(&row.directory), row.workspace_id.clone());
    Ok((location, row))
}

// ---------------------------------------------------------------------------
// Cursor codecs (§3)
// ---------------------------------------------------------------------------

/// base64url (no padding) — v2 `Input.encode`.
pub fn b64url_encode(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// base64url decode (no padding).
pub fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(input).ok()
}

/// Session-list cursor: base64url JSON
/// `{directory|project|all, anchor:{id,time,direction}}`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionCursor {
    pub scope: SessionCursorScope,
    pub anchor: Option<SessionAnchor>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionCursorScope {
    Directory(String),
    Project(String),
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionAnchor {
    pub id: String,
    pub time: i64,
    pub direction: CursorDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorDirection {
    Next,
    Previous,
}

impl CursorDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }
}

pub fn encode_session_cursor(cursor: &SessionCursor) -> Option<String> {
    let mut object = json!({});
    let map = object.as_object_mut()?;
    match &cursor.scope {
        SessionCursorScope::Directory(directory) => {
            map.insert("directory".into(), json!(directory));
        }
        SessionCursorScope::Project(project) => {
            map.insert("project".into(), json!(project));
        }
        SessionCursorScope::All => {
            map.insert("all".into(), json!(true));
        }
    }
    if let Some(anchor) = &cursor.anchor {
        map.insert(
            "anchor".into(),
            json!({
                "id": anchor.id,
                "time": anchor.time,
                "direction": anchor.direction.as_str(),
            }),
        );
    }
    let encoded = serde_json::to_string(&object).ok()?;
    Some(b64url_encode(encoded.as_bytes()))
}

pub fn decode_session_cursor(input: &str) -> Option<SessionCursor> {
    let bytes = b64url_decode(input.trim())?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let object = value.as_object()?;
    let scope = if let Some(directory) = object.get("directory").and_then(Value::as_str) {
        SessionCursorScope::Directory(directory.to_owned())
    } else if let Some(project) = object.get("project").and_then(Value::as_str) {
        SessionCursorScope::Project(project.to_owned())
    } else if object.contains_key("all") {
        SessionCursorScope::All
    } else {
        return None;
    };
    let anchor = object.get("anchor").and_then(|anchor| {
        Some(SessionAnchor {
            id: anchor.get("id")?.as_str()?.to_owned(),
            time: anchor.get("time")?.as_i64()?,
            direction: match anchor.get("direction")?.as_str()? {
                "next" => CursorDirection::Next,
                "previous" => CursorDirection::Previous,
                _ => return None,
            },
        })
    });
    Some(SessionCursor { scope, anchor })
}

/// Message cursor: base64url JSON `{id, order, direction}`.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageCursor {
    pub id: String,
    pub order: ListOrderWire,
    pub direction: CursorDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOrderWire {
    Asc,
    Desc,
}

impl ListOrderWire {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

pub fn encode_message_cursor(cursor: &MessageCursor) -> Option<String> {
    let object = json!({
        "id": cursor.id,
        "order": cursor.order.as_str(),
        "direction": cursor.direction.as_str(),
    });
    let encoded = serde_json::to_string(&object).ok()?;
    Some(b64url_encode(encoded.as_bytes()))
}

pub fn decode_message_cursor(input: &str) -> Option<MessageCursor> {
    let bytes = b64url_decode(input.trim())?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    Some(MessageCursor {
        id: value.get("id")?.as_str()?.to_owned(),
        order: match value.get("order")?.as_str()? {
            "asc" => ListOrderWire::Asc,
            "desc" => ListOrderWire::Desc,
            _ => return None,
        },
        direction: match value.get("direction")?.as_str()? {
            "next" => CursorDirection::Next,
            "previous" => CursorDirection::Previous,
            _ => return None,
        },
    })
}

// ---------------------------------------------------------------------------
// Session row → wire SessionInfo
// ---------------------------------------------------------------------------

/// Maps a `session_v2` row onto the wire `Session.Info` shape.
pub fn session_row_to_info(row: &oc_core::database::schema::SessionRow) -> Value {
    let mut info = json!({
        "id": row.id,
        "projectID": row.project_id,
        "location": { "directory": row.directory },
        "cost": row.cost,
        "tokens": {
            "input": row.tokens_input,
            "output": row.tokens_output,
            "reasoning": row.tokens_reasoning,
            "cache": { "read": row.tokens_cache_read, "write": row.tokens_cache_write },
        },
        "time": {
            "created": row.time_created,
            "updated": row.time_updated,
        },
    });
    let Value::Object(map) = &mut info else { return info };
    let set = |map: &mut serde_json::Map<String, Value>, key: &str, value: Value| {
        map.insert(key.to_owned(), value);
    };
    if let Some(title) = &row.title {
        set(map, "title", json!(title));
    }
    if !row.slug.is_empty() {
        set(map, "slug", json!(row.slug));
    }
    if let Some(parent) = &row.parent_id {
        set(map, "parentID", json!(parent));
    }
    if let Some(metadata) = decode_json_or_none(row.metadata.as_deref()) {
        set(map, "metadata", metadata);
    }
    if let Some(agent) = &row.agent {
        set(map, "agent", json!(agent));
    }
    if let Some(model) = decode_json_or_none(row.model.as_deref()) {
        set(map, "model", model);
    }
    if let Some(permissions) = decode_json_or_none(row.permission.as_deref()) {
        set(map, "permissions", permissions);
    }
    if let Some(revert) = decode_json_or_none(row.revert.as_deref()) {
        set(map, "revert", revert);
    }
    if let Some(path) = &row.path {
        set(map, "subpath", json!(path));
    }
    if let (Some(fork_id), boundary) = (&row.fork_session_id, row.fork_boundary.as_deref()) {
        let boundary_value = boundary
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or(json!(null));
        set(map, "fork", json!({ "sessionID": fork_id, "boundary": boundary_value }));
    }
    if let Some(outcome) = &row.idle_outcome {
        let outcome = match outcome.as_str() {
            "failed" => "failed",
            "interrupted" => "interrupted",
            _ => "succeeded",
        };
        set(map, "outcome", json!(outcome));
    }
    for (column, key) in
        [(row.time_viewed, "viewed"), (row.time_idle, "idle"), (row.time_archived, "archived")]
    {
        if let Some(value) = column {
            if let Some(time) = map.get_mut("time").and_then(Value::as_object_mut) {
                time.insert(key.to_owned(), json!(value));
            }
        }
    }
    info
}

fn decode_json_or_none(text: Option<&str>) -> Option<Value> {
    text.filter(|text| !text.is_empty()).and_then(|text| serde_json::from_str(text).ok())
}

/// Maps a `session_message` row onto the wire message shape: merges
/// `{id, type}` back into the projected `data` JSON.
pub fn message_row_to_info(row: &oc_core::database::schema::SessionMessageRow) -> Value {
    let mut data: Value = serde_json::from_str(&row.data).unwrap_or_else(|_| json!({}));
    if let Value::Object(map) = &mut data {
        map.insert("id".to_owned(), json!(row.id));
        map.insert("type".to_owned(), json!(row.type_));
    }
    data
}

/// Engine error-string → `ApiError` for the revert family (§3):
/// Busy → 409, snapshot errors → 500 UnknownError+ref.
pub fn revert_error(session_id: &str, message: &str) -> ApiError {
    if message.contains("busy") || message.contains("Session is busy") {
        return ApiError::session_busy(session_id);
    }
    ApiError::unknown_ref()
}

/// Engine error-string → `ApiError` for prompt paths: lifecycle conflicts
/// (409 resource=messageID), the rest → 500.
pub fn prompt_error(message: &str) -> ApiError {
    if message.contains("session inbox lifecycle conflict")
        || message.contains("Session is currently running")
    {
        let id = message.rsplit(':').next().map_or("", str::trim);
        return ApiError::conflict_resource("Session is currently running", id);
    }
    ApiError::unknown_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cursor_round_trip() {
        let cursor = SessionCursor {
            scope: SessionCursorScope::Directory("/tmp/w".into()),
            anchor: Some(SessionAnchor {
                id: "ses_1".into(),
                time: 1234,
                direction: CursorDirection::Previous,
            }),
        };
        let encoded = encode_session_cursor(&cursor).unwrap();
        assert!(!encoded.contains('='));
        assert_eq!(decode_session_cursor(&encoded).unwrap(), cursor);
    }

    #[test]
    fn session_cursor_all_scope() {
        let cursor = SessionCursor { scope: SessionCursorScope::All, anchor: None };
        let encoded = encode_session_cursor(&cursor).unwrap();
        assert_eq!(decode_session_cursor(&encoded).unwrap(), cursor);
    }

    #[test]
    fn message_cursor_round_trip() {
        let cursor = MessageCursor {
            id: "msg_9".into(),
            order: ListOrderWire::Desc,
            direction: CursorDirection::Next,
        };
        let encoded = encode_message_cursor(&cursor).unwrap();
        assert_eq!(decode_message_cursor(&encoded).unwrap(), cursor);
    }

    #[test]
    fn garbage_cursor_is_none() {
        assert!(decode_session_cursor("!!!").is_none());
        assert!(decode_message_cursor("not-base64!").is_none());
        assert!(decode_session_cursor(&b64url_encode(b"not json")).is_none());
    }

    #[test]
    fn unknown_error_shape() {
        let error = ApiError::unknown_ref();
        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.body["_tag"], "UnknownError");
        assert_eq!(
            error.body["message"],
            "Unexpected server error. Check server logs for details."
        );
        let reference = error.body["ref"].as_str().unwrap();
        assert!(reference.starts_with("err_"));
        assert_eq!(reference.len(), 12);
    }
}
