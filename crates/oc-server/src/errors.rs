//! Error-envelope registry (`packages/protocol/src/errors.ts` +
//! server-deep-map §6).
//!
//! Every `HttpApi` failure is a JSON envelope `{"_tag": "<ClassName>", ...}`
//! with the camelCase fields of the v2 tagged error. Readiness 503s are the
//! one deliberate exception: they carry a `code` key (`service_starting` /
//! `service_stopping` / `service_failed`) instead of `_tag`.
//!
//! Non-JSON bodies from v2 that stay non-JSON here: the raw dispatch 401 is
//! still an envelope (produced by [`crate::auth`]), unmatched API routes are
//! an empty 404, and browser pairing failures are plain text.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

/// A rendered HTTP error: status plus the exact JSON envelope.
#[derive(Debug, Clone)]
pub struct ApiError {
    /// HTTP status.
    pub status: StatusCode,
    /// Envelope body (`_tag` + fields, or the readiness `code` shape).
    pub body: Value,
}

impl ApiError {
    /// Builds an error from parts.
    #[must_use]
    pub fn new(status: StatusCode, body: Value) -> Self {
        Self { status, body }
    }

    /// Renders the envelope into an axum response (application/json).
    ///
    /// 5xx envelopes are ALWAYS logged (one line, stderr + the
    /// `<data>/log/opencode.log` sink) — the catch-all guarantee that a 500
    /// is never silent even at call sites without a `debug_cause` map:
    /// the envelope carries the `err_<…>` ref that correlates with the
    /// `[api-cause]` cause line. v2 parity: the TS server logs every
    /// `HttpApi` failure; the silent port was the gap.
    #[must_use]
    pub fn into_response(self) -> Response {
        if self.status.as_u16() >= 500 && !is_expected_readiness(&self.body) {
            append_server_log(&format!("[api-error] {} {}", self.status.as_u16(), self.body));
        }
        let Self { status, body } = self;
        let mut response = (status, axum::body::Body::from(body.to_string())).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        response
    }

    /// HTTP status of this error.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

/// The axum trait bridge so handler signatures can return
/// `Result<Response, ApiError>`; the inherent method above stays the
/// canonical renderer (this is the envelope type's single `IntoResponse`
/// impl — the former duplicate in `handlers/common.rs` was removed in the
/// Phase 5 integration).
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        ApiError::into_response(self)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.status.as_u16(), self.body)
    }
}

impl std::error::Error for ApiError {}

#[allow(clippy::needless_pass_by_value)] // every caller builds a fresh json! literal
fn tagged(tag: &str, status: StatusCode, fields: Value) -> ApiError {
    let mut body = json!({ "_tag": tag });
    if let (Some(target), Some(source)) = (body.as_object_mut(), fields.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    ApiError { status, body }
}

// ---------------------------------------------------------------------------
// 401 / 403
// ---------------------------------------------------------------------------

/// `UnauthorizedError` 401.
#[must_use]
pub fn unauthorized(message: impl Into<String>) -> ApiError {
    tagged("UnauthorizedError", StatusCode::UNAUTHORIZED, json!({ "message": message.into() }))
}

/// `ForbiddenError` 403.
#[must_use]
pub fn forbidden(message: impl Into<String>) -> ApiError {
    tagged("ForbiddenError", StatusCode::FORBIDDEN, json!({ "message": message.into() }))
}

// ---------------------------------------------------------------------------
// 400
// ---------------------------------------------------------------------------

/// `InvalidRequestError` 400 (schema rejection).
#[must_use]
pub fn invalid_request(message: impl Into<String>) -> ApiError {
    tagged("InvalidRequestError", StatusCode::BAD_REQUEST, json!({ "message": message.into() }))
}

/// `InvalidRequestError` 400 with a `kind`.
#[must_use]
pub fn invalid_request_kind(message: impl Into<String>, kind: impl Into<String>) -> ApiError {
    tagged(
        "InvalidRequestError",
        StatusCode::BAD_REQUEST,
        json!({ "message": message.into(), "kind": kind.into() }),
    )
}

/// `InvalidRequestError` 400 with `field` but no `kind` — the shape ref
/// uses where the error is constructed directly rather than from a schema
/// parse (e.g. server/src/location.ts:51-53's `Invalid session ID`).
#[must_use]
pub fn invalid_request_message_field(
    message: impl Into<String>,
    field: impl Into<String>,
) -> ApiError {
    tagged(
        "InvalidRequestError",
        StatusCode::BAD_REQUEST,
        json!({ "message": message.into(), "field": field.into() }),
    )
}

/// `InvalidRequestError` 400 with `kind` + `field`.
#[must_use]
pub fn invalid_request_field(
    message: impl Into<String>,
    kind: impl Into<String>,
    field: impl Into<String>,
) -> ApiError {
    tagged(
        "InvalidRequestError",
        StatusCode::BAD_REQUEST,
        json!({ "message": message.into(), "kind": kind.into(), "field": field.into() }),
    )
}

/// `InvalidCursorError` 400.
#[must_use]
pub fn invalid_cursor(message: impl Into<String>) -> ApiError {
    tagged("InvalidCursorError", StatusCode::BAD_REQUEST, json!({ "message": message.into() }))
}

/// `RpcError` 400 with an optional `data` payload (errors.ts:21-29 —
/// `data` is `Schema.optional(Schema.Unknown)`; absent stays absent on
/// the wire).
#[must_use]
pub fn rpc_error(type_: impl Into<String>, message: impl Into<String>) -> ApiError {
    rpc_error_data(type_, message, None)
}

/// `RpcError` 400 with a `data` value.
#[must_use]
pub fn rpc_error_data(
    type_: impl Into<String>,
    message: impl Into<String>,
    data: Option<Value>,
) -> ApiError {
    tagged(
        "RpcError",
        StatusCode::BAD_REQUEST,
        envelope_data(json!({ "type": type_.into(), "message": message.into() }), data),
    )
}

/// `FormInvalidAnswerError` 400.
#[must_use]
pub fn form_invalid_answer(id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "FormInvalidAnswerError",
        StatusCode::BAD_REQUEST,
        json!({ "id": id.into(), "message": message.into() }),
    )
}

// ---------------------------------------------------------------------------
// 404
// ---------------------------------------------------------------------------

/// `LocationNotFoundError` 404.
#[must_use]
pub fn location_not_found(directory: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "LocationNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "location": { "directory": directory.into() }, "message": message.into() }),
    )
}

/// `LocationNotFoundError` for a missing directory (`Location not found: <dir>`).
#[must_use]
pub fn location_not_found_path(directory: impl std::fmt::Display) -> ApiError {
    let directory = directory.to_string();
    location_not_found(directory.clone(), format!("Location not found: {directory}"))
}

/// `ProviderNotFoundError` 404.
#[must_use]
pub fn provider_not_found(provider_id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "ProviderNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "providerID": provider_id.into(), "message": message.into() }),
    )
}

/// `IntegrationNotFoundError` 404.
#[must_use]
pub fn integration_not_found(
    integration_id: impl Into<String>,
    message: impl Into<String>,
) -> ApiError {
    tagged(
        "IntegrationNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "integrationID": integration_id.into(), "message": message.into() }),
    )
}

/// `IntegrationAttemptNotFoundError` 404.
#[must_use]
pub fn integration_attempt_not_found(
    integration_id: impl Into<String>,
    attempt_id: impl Into<String>,
    message: impl Into<String>,
) -> ApiError {
    tagged(
        "IntegrationAttemptNotFoundError",
        StatusCode::NOT_FOUND,
        json!({
            "integrationID": integration_id.into(),
            "attemptID": attempt_id.into(),
            "message": message.into()
        }),
    )
}

/// `IntegrationMethodNotFoundError` 404.
#[must_use]
pub fn integration_method_not_found(
    integration_id: impl Into<String>,
    method_id: impl Into<String>,
    message: impl Into<String>,
) -> ApiError {
    tagged(
        "IntegrationMethodNotFoundError",
        StatusCode::NOT_FOUND,
        json!({
            "integrationID": integration_id.into(),
            "methodID": method_id.into(),
            "message": message.into()
        }),
    )
}

/// `ProjectNotFoundError` 404.
#[must_use]
pub fn project_not_found(project_id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "ProjectNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "projectID": project_id.into(), "message": message.into() }),
    )
}

/// `FileNotFoundError` 404.
#[must_use]
pub fn file_not_found(path: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "FileNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "path": path.into(), "message": message.into() }),
    )
}

/// `AgentNotFoundError` 404.
#[must_use]
pub fn agent_not_found(agent_id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "AgentNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "agentID": agent_id.into(), "message": message.into() }),
    )
}

/// `SessionNotFoundError` 404 (`Session not found: <id>`).
#[must_use]
pub fn session_not_found(session_id: impl Into<String>) -> ApiError {
    let session_id = session_id.into();
    tagged(
        "SessionNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "sessionID": session_id.clone(), "message": format!("Session not found: {session_id}") }),
    )
}

/// `MessageNotFoundError` 404.
#[must_use]
pub fn message_not_found(
    session_id: impl Into<String>,
    message_id: impl Into<String>,
    message: impl Into<String>,
) -> ApiError {
    tagged(
        "MessageNotFoundError",
        StatusCode::NOT_FOUND,
        json!({
            "sessionID": session_id.into(),
            "messageID": message_id.into(),
            "message": message.into()
        }),
    )
}

/// `SkillNotFoundError` 404.
#[must_use]
pub fn skill_not_found(skill: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "SkillNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "skill": skill.into(), "message": message.into() }),
    )
}

/// `McpServerNotFoundError` 404.
#[must_use]
pub fn mcp_server_not_found(server: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "McpServerNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "server": server.into(), "message": message.into() }),
    )
}

/// `CommandNotFoundError` 404.
#[must_use]
pub fn command_not_found(command: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "CommandNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "command": command.into(), "message": message.into() }),
    )
}

/// `PermissionNotFoundError` 404.
#[must_use]
pub fn permission_not_found(request_id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "PermissionNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "requestID": request_id.into(), "message": message.into() }),
    )
}

/// `FormNotFoundError` 404.
#[must_use]
pub fn form_not_found(id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "FormNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "id": id.into(), "message": message.into() }),
    )
}

/// `PtyNotFoundError` 404.
#[must_use]
pub fn pty_not_found(pty_id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "PtyNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "ptyID": pty_id.into(), "message": message.into() }),
    )
}

/// `ShellNotFoundError` 404.
#[must_use]
pub fn shell_not_found(id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "ShellNotFoundError",
        StatusCode::NOT_FOUND,
        json!({ "id": id.into(), "message": message.into() }),
    )
}

// ---------------------------------------------------------------------------
// 409 / 500 / 503
// ---------------------------------------------------------------------------

/// `ConflictError` 409.
#[must_use]
pub fn conflict(message: impl Into<String>) -> ApiError {
    tagged("ConflictError", StatusCode::CONFLICT, json!({ "message": message.into() }))
}

/// `ConflictError` 409 with a `resource`.
#[must_use]
pub fn conflict_resource(message: impl Into<String>, resource: impl Into<String>) -> ApiError {
    tagged(
        "ConflictError",
        StatusCode::CONFLICT,
        json!({ "message": message.into(), "resource": resource.into() }),
    )
}

/// `SessionBusyError` 409.
#[must_use]
pub fn session_busy(session_id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "SessionBusyError",
        StatusCode::CONFLICT,
        json!({ "sessionID": session_id.into(), "message": message.into() }),
    )
}

/// `FormAlreadySettledError` 409.
#[must_use]
pub fn form_already_settled(id: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "FormAlreadySettledError",
        StatusCode::CONFLICT,
        json!({ "id": id.into(), "message": message.into() }),
    )
}

/// `RpcInternalError` 500 (`rpc.internal` / `rpc.invalid_output`) with
/// an optional `data` payload (errors.ts:31-38).
#[must_use]
pub fn rpc_internal(type_: impl Into<String>, message: impl Into<String>) -> ApiError {
    rpc_internal_data(type_, message, None)
}

/// `RpcInternalError` 500 with a `data` value.
#[must_use]
pub fn rpc_internal_data(
    type_: impl Into<String>,
    message: impl Into<String>,
    data: Option<Value>,
) -> ApiError {
    tagged(
        "RpcInternalError",
        StatusCode::INTERNAL_SERVER_ERROR,
        envelope_data(json!({ "type": type_.into(), "message": message.into() }), data),
    )
}

/// Merges an optional `data` member into the base fields (absent stays
/// absent — v2 `Schema.optional` drops undefined on the wire).
fn envelope_data(mut fields: Value, data: Option<Value>) -> Value {
    if let (Some(target), Some(data)) = (fields.as_object_mut(), data) {
        target.insert("data".to_owned(), data);
    }
    fields
}

/// `CommandExecutionError` 500.
#[must_use]
pub fn command_execution_error(command: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "CommandExecutionError",
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "command": command.into(), "message": message.into() }),
    )
}

/// `UnknownError` 500 with a generated `err_<8 uuid chars>` ref.
#[must_use]
pub fn unknown_with_ref() -> ApiError {
    unknown(format!("err_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]))
}

/// `UnknownError` 500 (v2 default message).
#[must_use]
pub fn unknown_with_default_message() -> ApiError {
    unknown("Unexpected server error. Check server logs for details.")
}

/// `UnknownError` 500 with an explicit ref.
#[must_use]
pub fn unknown(ref_id: impl Into<String>) -> ApiError {
    unknown_message_ref("Unexpected server error. Check server logs for details.", ref_id)
}

/// `UnknownError` 500 with explicit message + ref.
#[must_use]
pub fn unknown_message_ref(message: impl Into<String>, ref_id: impl Into<String>) -> ApiError {
    tagged(
        "UnknownError",
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "message": message.into(), "ref": ref_id.into() }),
    )
}

/// `ServiceUnavailableError` 503.
#[must_use]
pub fn service_unavailable(message: impl Into<String>) -> ApiError {
    tagged(
        "ServiceUnavailableError",
        StatusCode::SERVICE_UNAVAILABLE,
        json!({ "message": message.into() }),
    )
}

/// `ServiceUnavailableError` 503 with a `service`.
#[must_use]
pub fn service_unavailable_named(
    message: impl Into<String>,
    service: impl Into<String>,
) -> ApiError {
    tagged(
        "ServiceUnavailableError",
        StatusCode::SERVICE_UNAVAILABLE,
        json!({ "message": message.into(), "service": service.into() }),
    )
}

// ---------------------------------------------------------------------------
// Spec-pinned wire-only envelopes (formerly inline in handlers/common.rs)
// ---------------------------------------------------------------------------

/// `WorktreeError` 400 (protocol/groups/worktree.ts:9-24): the literal
/// `name: "WorktreeError"` and a nested `data: { message, forceRequired? }`
/// — the only envelope whose payload nests under `data`. `forceRequired`
/// surfaces only when a dirty tree demands `force` (undefined is dropped
/// on the wire).
#[must_use]
pub fn worktree_error(message: impl Into<String>, force_required: Option<bool>) -> ApiError {
    let mut data = json!({ "message": message.into() });
    if force_required == Some(true) {
        if let Value::Object(map) = &mut data {
            map.insert("forceRequired".to_owned(), json!(true));
        }
    }
    ApiError::new(
        StatusCode::BAD_REQUEST,
        json!({ "_tag": "WorktreeError", "name": "WorktreeError", "data": data }),
    )
}

/// `VcsInitNotSupportedError` 501 (protocol/groups/vcs.ts:15-19).
#[must_use]
pub fn vcs_init_not_supported(provider_id: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::NOT_IMPLEMENTED,
        json!({
            "_tag": "VcsInitNotSupportedError",
            "providerID": provider_id.into(),
            "message": "VCS init not supported",
        }),
    )
}

/// `InstructionEntryValueTooLargeError` 413 (schema/instruction-entry.ts).
#[must_use]
pub fn instruction_entry_too_large(actual: i64, max: i64) -> ApiError {
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

// ---------------------------------------------------------------------------
// Readiness 503 envelopes (`code` key, NOT `_tag`) — process.ts `unavailable`
// ---------------------------------------------------------------------------

/// `{"code":"service_failed", message, action}` — 503, no retry-after.
#[must_use]
pub fn service_failed() -> ApiError {
    ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: json!({
            "code": "service_failed",
            "message": "The background service could not start.",
            "action": "Run `opencode service restart` after checking the service logs."
        }),
    }
}

/// `{"code":"service_starting"}` — 503 + `retry-after: 1`.
#[must_use]
pub fn service_starting() -> ApiError {
    ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: json!({ "code": "service_starting" }),
    }
}

/// `{"code":"service_stopping"}` — 503 + `retry-after: 1`.
#[must_use]
pub fn service_stopping() -> ApiError {
    ApiError {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: json!({ "code": "service_stopping" }),
    }
}

// ---------------------------------------------------------------------------
// Schema-rejection helper (middleware/schema-error.ts)
// ---------------------------------------------------------------------------

/// `REASON_LIMIT` from `middleware/schema-error.ts`.
const REASON_LIMIT: usize = 1024;

/// Truncates a schema-rejection reason: first 1024 chars plus
/// `"... (<N> more chars)"`.
#[must_use]
pub fn truncate_reason(reason: &str) -> String {
    let count = reason.chars().count();
    if count <= REASON_LIMIT {
        return reason.to_owned();
    }
    let head: String = reason.chars().take(REASON_LIMIT).collect();
    format!("{head}... ({} more chars)", count - REASON_LIMIT)
}

/// The `SchemaError` middleware outcome: warn-log + `InvalidRequestError{kind}`
/// (v2's middleware fails with exactly `{message, kind}` — no `field`).
#[must_use]
pub fn schema_rejection(reason: &str, kind: &str) -> ApiError {
    let truncated = truncate_reason(reason);
    tracing_like_warn(kind, &truncated);
    invalid_request_kind(truncated, kind)
}

/// Minimal warn log (the crate has no tracing dep; v2 logs via Effect).
/// G-387: routed through `oc_util::logging` — the line ALWAYS lands in
/// the shared log file and reaches stderr only under
/// `OPENCODE_PRINT_LOGS=1` (v2's `stderrLogger`), gated by the
/// `OPENCODE_LOG_LEVEL` floor.
fn tracing_like_warn(kind: &str, reason: &str) {
    oc_util::logging::log(
        oc_util::logging::Level::Warn,
        &format!("[schema-error] schema rejection kind={kind} reason={reason}"),
    );
}

/// Debug-level cause capture for the handler `map_err(|_| …)` sites
/// (G11-F3): the public error shape intentionally discards the cause
/// (leak-prevention, mirroring v2). The one-line cause is ALWAYS captured
/// now — appended to the server log file under `<data>/log/` in v2's
/// layout via [`oc_util::logging::log`] (G-387: stderr copy only under
/// `OPENCODE_PRINT_LOGS=1` — the daemon's stderr is /dev/null otherwise;
/// the floor default of Info keeps the file line always-on).
/// `OPENCODE_DEBUG_ERRORS` ∈ {1,true} additionally prints the verbose
/// `{error:#?}` form to stderr; the wire behavior is unchanged either way.
pub fn debug_cause(site: &str, error: impl std::fmt::Debug) {
    // G-387: the line ALWAYS lands in the shared log file (the
    // managed/standalone daemon is spawned with stderr redirected to
    // /dev/null unless `OPENCODE_PRINT_LOGS=1`, so stderr alone loses
    // the line — "500s were silent"); the stderr copy is gated by
    // `OPENCODE_PRINT_LOGS` per message, the whole line by the
    // `OPENCODE_LOG_LEVEL` floor (Info level keeps it always-on at the
    // default floor). `OPENCODE_DEBUG_ERRORS` ∈ {1,true} additionally
    // prints the verbose `{error:#?}` form to stderr; the wire behavior
    // is unchanged either way.
    let line = format!("[api-cause] {site}: {error:?}");
    oc_util::logging::log(oc_util::logging::Level::Info, &line);
    if debug_errors_enabled() {
        eprintln!("[api-cause:verbose] {site}: {error:#?}");
    }
}

/// The benign readiness probes (`service_starting`/`service_stopping`)
/// poll repeatedly during lifecycle transitions — expected control flow,
/// not failures, so they stay out of the always-on 5xx sink.
/// `service_failed` and every real 500/503 envelope still logs.
fn is_expected_readiness(body: &Value) -> bool {
    matches!(
        body.get("code").and_then(Value::as_str),
        Some("service_starting" | "service_stopping")
    )
}

// The log sink (path/append/trim) lives in `oc_util::logging` (G-387
// hoist so oc-core/oc-server share one gate); re-exported here so
// every existing caller and test keeps working unchanged.
pub use oc_util::logging::{append_server_log, log_filename};

/// `OPENCODE_DEBUG_ERRORS` (resolved once; env reads are not free on the
/// hot path).
fn debug_errors_enabled() -> bool {
    static ENABLED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
        matches!(std::env::var("OPENCODE_DEBUG_ERRORS").as_deref(), Ok("1" | "true"))
    });
    *ENABLED
}

/// An empty-bodied response (unmatched API 404, WS handshake errors).
#[must_use]
pub fn empty(status: StatusCode) -> Response {
    Response::builder().status(status).body(axum::body::Body::empty()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(error: &ApiError) -> Value {
        error.body.clone()
    }

    #[test]
    fn server_errors_unauthorized_envelope() {
        let error = unauthorized("Authentication required");
        assert_eq!(error.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            body(&error),
            json!({"_tag": "UnauthorizedError", "message": "Authentication required"})
        );
    }

    #[test]
    fn server_errors_unknown_ref_shape() {
        let error = unknown("err_ab12cd34");
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body(&error),
            json!({
                "_tag": "UnknownError",
                "message": "Unexpected server error. Check server logs for details.",
                "ref": "err_ab12cd34"
            })
        );
    }

    #[test]
    fn server_errors_unknown_ref_prefix() {
        let error = unknown_with_ref();
        let reference = error.body["ref"].as_str().unwrap().to_owned();
        assert!(reference.starts_with("err_"), "ref was {reference}");
        assert_eq!(reference.len(), 12, "err_ + 8 chars");
    }

    #[test]
    fn server_errors_invalid_request_kind() {
        let error = invalid_request_kind("bad body", "ParseError");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body(&error),
            json!({"_tag": "InvalidRequestError", "message": "bad body", "kind": "ParseError"})
        );
    }

    #[test]
    fn server_errors_location_not_found() {
        let error = location_not_found_path("/gone");
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(&error),
            json!({
                "_tag": "LocationNotFoundError",
                "location": { "directory": "/gone" },
                "message": "Location not found: /gone"
            })
        );
    }

    #[test]
    fn server_errors_session_not_found() {
        let error = session_not_found("ses_1");
        assert_eq!(
            body(&error),
            json!({"_tag": "SessionNotFoundError", "sessionID": "ses_1", "message": "Session not found: ses_1"})
        );
    }

    #[test]
    fn server_errors_message_not_found() {
        let error = message_not_found("ses_1", "msg_2", "Message not found: msg_2");
        assert_eq!(
            body(&error),
            json!({
                "_tag": "MessageNotFoundError",
                "sessionID": "ses_1", "messageID": "msg_2", "message": "Message not found: msg_2"
            })
        );
    }

    #[test]
    fn server_errors_session_busy() {
        let error = session_busy("ses_1", "busy");
        assert_eq!(error.status(), StatusCode::CONFLICT);
        assert_eq!(
            body(&error),
            json!({"_tag": "SessionBusyError", "sessionID": "ses_1", "message": "busy"})
        );
    }

    #[test]
    fn server_errors_conflict_resource() {
        let error = conflict_resource("prompt conflict", "msg_1");
        assert_eq!(
            body(&error),
            json!({"_tag": "ConflictError", "message": "prompt conflict", "resource": "msg_1"})
        );
    }

    #[test]
    fn server_errors_service_unavailable_named() {
        let error = service_unavailable_named("no pty", "opencode-pty");
        assert_eq!(error.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body(&error),
            json!({"_tag": "ServiceUnavailableError", "message": "no pty", "service": "opencode-pty"})
        );
    }

    #[test]
    fn server_errors_readiness_uses_code_key() {
        assert_eq!(body(&service_starting()), json!({"code": "service_starting"}));
        assert_eq!(body(&service_stopping()), json!({"code": "service_stopping"}));
        assert_eq!(
            body(&service_failed()),
            json!({
                "code": "service_failed",
                "message": "The background service could not start.",
                "action": "Run `opencode service restart` after checking the service logs."
            })
        );
    }

    #[test]
    fn server_errors_forbidden_and_cursor() {
        assert_eq!(forbidden("no").status(), StatusCode::FORBIDDEN);
        assert_eq!(
            body(&invalid_cursor("bad cursor")),
            json!({"_tag": "InvalidCursorError", "message": "bad cursor"})
        );
    }

    #[test]
    fn server_errors_not_found_family() {
        assert_eq!(provider_not_found("anthropic", "x").status(), StatusCode::NOT_FOUND);
        assert_eq!(pty_not_found("pty_1", "x").status(), StatusCode::NOT_FOUND);
        assert_eq!(
            body(&pty_not_found("pty_1", "gone")),
            json!({"_tag": "PtyNotFoundError", "ptyID": "pty_1", "message": "gone"})
        );
        assert_eq!(
            body(&integration_method_not_found("github", "m1", "gone")),
            json!({
                "_tag": "IntegrationMethodNotFoundError",
                "integrationID": "github", "methodID": "m1", "message": "gone"
            })
        );
    }

    #[test]
    fn server_errors_truncates_schema_reasons() {
        let short = "x".repeat(100);
        assert_eq!(truncate_reason(&short), short);
        let long = "y".repeat(1500);
        let truncated = truncate_reason(&long);
        assert!(truncated.starts_with(&"y".repeat(1024)));
        assert!(truncated.ends_with("... (476 more chars)"));
        let error = schema_rejection(&long, "ParseError");
        assert_eq!(error.body["message"].as_str().unwrap(), truncated);
        assert_eq!(error.body["kind"], "ParseError");
    }

    #[test]
    fn server_errors_rpc_family() {
        assert_eq!(rpc_error("rpc.custom", "boom").status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body(&rpc_internal("rpc.internal", "boom")),
            json!({"_tag": "RpcInternalError", "type": "rpc.internal", "message": "boom"})
        );
        assert_eq!(
            body(&command_execution_error("git", "exit 1")),
            json!({"_tag": "CommandExecutionError", "command": "git", "message": "exit 1"})
        );
    }

    /// errors.ts:21-38: `data` is optional on both Rpc envelopes —
    /// absent stays absent, present is a top-level member.
    #[test]
    fn server_errors_rpc_data_optional() {
        assert_eq!(rpc_error("rpc.custom", "m").body.get("data"), None);
        assert_eq!(rpc_internal("rpc.internal", "m").body.get("data"), None);
        assert_eq!(
            rpc_error_data("rpc.custom", "m", Some(json!({"code": 7}))).body,
            json!({"_tag": "RpcError", "type": "rpc.custom", "message": "m", "data": {"code": 7}})
        );
        assert_eq!(
            rpc_internal_data("rpc.internal", "m", Some(json!(null))).body["data"],
            json!(null)
        );
    }

    /// The three envelopes consolidated from handlers/common.rs inline
    /// literals — wire bytes pinned against the ref protocol files.
    #[test]
    fn server_errors_worktree_vcs_init_instruction_envelopes() {
        assert_eq!(
            worktree_error("Worktree source not found: /x", None).body,
            json!({
                "_tag": "WorktreeError",
                "name": "WorktreeError",
                "data": {"message": "Worktree source not found: /x"}
            })
        );
        assert_eq!(worktree_error("dirty", None).status, StatusCode::BAD_REQUEST);
        assert_eq!(worktree_error("dirty", Some(true)).body["data"]["forceRequired"], json!(true));
        // `Some(false)` and `None` both drop the key (undefined dropped).
        assert_eq!(worktree_error("dirty", Some(false)).body["data"].get("forceRequired"), None);

        assert_eq!(
            vcs_init_not_supported("fallback").body,
            json!({
                "_tag": "VcsInitNotSupportedError",
                "providerID": "fallback",
                "message": "VCS init not supported"
            })
        );
        assert_eq!(vcs_init_not_supported("fallback").status, StatusCode::NOT_IMPLEMENTED);

        assert_eq!(
            instruction_entry_too_large(5000, 1024).body,
            json!({
                "_tag": "InstructionEntryValueTooLargeError",
                "message": "Instruction entry value too large",
                "actualBytes": 5000,
                "maxBytes": 1024
            })
        );
        assert_eq!(instruction_entry_too_large(5000, 1024).status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    // ------------------------------------------------------------------
    // Cause logging (always-on one-line capture + file sink)
    // ------------------------------------------------------------------

    /// Serializes env mutation for the log-sink tests (one process, many
    /// parallel tests).
    static LOG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn server_errors_debug_cause_always_writes_the_sink() {
        let _guard = LOG_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let log = dir.path().join("server.log");
        let saved = std::env::var_os("OPENCODE_LOG_FILE");

        std::env::set_var("OPENCODE_LOG_FILE", &log);
        debug_cause("site:probe", "sqlite error: boom");
        if let Some(saved) = saved {
            std::env::set_var("OPENCODE_LOG_FILE", saved);
        } else {
            std::env::remove_var("OPENCODE_LOG_FILE");
        }

        let contents = std::fs::read_to_string(&log).expect("sink file written");
        assert!(contents.contains("[api-cause] site:probe: \"sqlite error: boom\""), "{contents}");
        assert!(contents.contains("[ts "), "unix-ms timestamp prefix present");
    }

    #[test]
    fn server_errors_5xx_envelope_logged_on_render() {
        let _guard = LOG_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let log = dir.path().join("server.log");
        let saved = std::env::var_os("OPENCODE_LOG_FILE");

        std::env::set_var("OPENCODE_LOG_FILE", &log);
        let error = unknown("err_deadbeef");
        let response = error.into_response();
        if let Some(saved) = saved {
            std::env::set_var("OPENCODE_LOG_FILE", saved);
        } else {
            std::env::remove_var("OPENCODE_LOG_FILE");
        }

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let contents = std::fs::read_to_string(&log).expect("sink file written");
        assert!(contents.contains("[api-error] 500"), "{contents}");
        assert!(
            contents.contains("err_deadbeef"),
            "ref correlates with the cause line: {contents}"
        );
    }

    #[test]
    fn server_errors_4xx_envelope_not_logged() {
        let _guard = LOG_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let log = dir.path().join("server.log");
        let saved = std::env::var_os("OPENCODE_LOG_FILE");

        std::env::set_var("OPENCODE_LOG_FILE", &log);
        let response = session_not_found("ses_x").into_response();
        if let Some(saved) = saved {
            std::env::set_var("OPENCODE_LOG_FILE", saved);
        } else {
            std::env::remove_var("OPENCODE_LOG_FILE");
        }

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!log.exists(), "4xx envelopes are not logged");
    }

}
