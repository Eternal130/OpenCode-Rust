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
    #[must_use]
    pub fn into_response(self) -> Response {
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

/// `RpcError` 400.
#[must_use]
pub fn rpc_error(type_: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "RpcError",
        StatusCode::BAD_REQUEST,
        json!({ "type": type_.into(), "message": message.into() }),
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

/// `RpcInternalError` 500 (`rpc.internal` / `rpc.invalid_output`).
#[must_use]
pub fn rpc_internal(type_: impl Into<String>, message: impl Into<String>) -> ApiError {
    tagged(
        "RpcInternalError",
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({ "type": type_.into(), "message": message.into() }),
    )
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

/// The `SchemaError` middleware outcome: warn-log + `InvalidRequestError{kind}`.
#[must_use]
pub fn schema_rejection(reason: &str, kind: &str) -> ApiError {
    let truncated = truncate_reason(reason);
    tracing_like_warn(kind, &truncated);
    invalid_request_field(truncated, kind, "")
}

/// Minimal warn log (the crate has no tracing dep; v2 logs via Effect).
fn tracing_like_warn(kind: &str, reason: &str) {
    eprintln!("[schema-error] schema rejection kind={kind} reason={reason}");
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
}
