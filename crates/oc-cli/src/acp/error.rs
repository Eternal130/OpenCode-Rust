//! Error surface of the ACP agent (port of `acp/error.ts`): every
//! failure an ACP request can produce, mapped 1:1 onto JSON-RPC
//! request errors (`ACPError.toRequestError`, error.ts:88-132).

use agent_client_protocol::schema::v1::Error as RequestError;

/// The ACP error union (error.ts:72-85).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcpError {
    /// `ACPSessionNotFoundError` (error.ts:4).
    #[error("session not found: {session_id}")]
    SessionNotFound { session_id: String },
    /// `ACPSessionDirectoryMismatchError` (error.ts:8).
    #[error("session {session_id} does not belong to cwd: {cwd}")]
    SessionDirectoryMismatch { session_id: String, cwd: String },
    /// `ACPInvalidConfigOptionError` (error.ts:13).
    #[error("unknown config option: {config_id}")]
    InvalidConfigOption { config_id: String },
    /// `ACPInvalidModelError` (error.ts:18).
    #[error("model not found: {model_id}")]
    InvalidModel { model_id: String, provider_id: Option<String> },
    /// `ACPInvalidEffortError` (error.ts:23).
    #[error("effort not found: {effort}")]
    InvalidEffort { effort: String },
    /// `ACPInvalidModeError` (error.ts:27).
    #[error("mode not found: {mode}")]
    InvalidMode { mode: String },
    /// `ACPInvalidAdditionalDirectoryError` (error.ts:31).
    #[error("additional directory must be an absolute path without glob characters: {directory}")]
    InvalidAdditionalDirectory { directory: String },
    /// `ACPAuthRequiredError` (error.ts:36) — provider auth needed.
    #[error("provider authentication required")]
    AuthRequired,
    /// `ACPUnknownAuthMethodError` (error.ts:38).
    #[error("unknown auth method: {method_id}")]
    UnknownAuthMethod { method_id: String },
    /// `ACPInvalidRequestError` (error.ts:42).
    #[error("{message}")]
    InvalidRequest { message: String, field: Option<String> },
    /// `ACPServiceFailureError` (error.ts:47) — safe message, never
    /// leaks internal detail.
    #[error("{safe_message}")]
    ServiceFailure { safe_message: String, service: Option<String>, error_name: Option<String> },
    /// `ACPServerUnavailableError` (error.ts:53).
    #[error("OpenCode server is unavailable")]
    ServerUnavailable,
    /// `ACPCatalogNotReadyError` (error.ts:58) — reason is "models" or
    /// "agents" (message: error.ts:61-63).
    #[error("{reason}")]
    CatalogNotReady { reason: &'static str },
    /// `ACPCatalogLoadError` (error.ts:66) — cause flattened to a
    /// string (the Rust port does not carry defect values).
    #[error("catalog load failed: {0}")]
    CatalogLoad(String),
}

impl AcpError {
    /// `toRequestError` (error.ts:88-132): the JSON-RPC error a failed
    /// ACP request responds with — code, data and message 1:1.
    #[must_use]
    pub fn to_request_error(&self) -> RequestError {
        use agent_client_protocol::schema::v1::ErrorCode;
        let (code, message, data): (ErrorCode, String, serde_json::Value) = match self {
            Self::SessionNotFound { session_id } => (
                ErrorCode::InvalidParams,
                format!("session not found: {session_id}"),
                serde_json::json!({ "sessionId": session_id }),
            ),
            Self::SessionDirectoryMismatch { session_id, cwd } => (
                ErrorCode::InvalidParams,
                format!("session {session_id} does not belong to cwd: {cwd}"),
                serde_json::json!({ "sessionId": session_id, "cwd": cwd }),
            ),
            Self::InvalidConfigOption { config_id } => (
                ErrorCode::InvalidParams,
                format!("unknown config option: {config_id}"),
                serde_json::json!({ "configId": config_id }),
            ),
            Self::InvalidModel { model_id, provider_id } => {
                let mut data = serde_json::Map::new();
                data.insert("modelId".to_owned(), serde_json::json!(model_id));
                if let Some(provider_id) = provider_id {
                    data.insert("providerId".to_owned(), serde_json::json!(provider_id));
                }
                (
                    ErrorCode::InvalidParams,
                    format!("model not found: {model_id}"),
                    serde_json::Value::Object(data),
                )
            }
            Self::InvalidEffort { effort } => (
                ErrorCode::InvalidParams,
                format!("effort not found: {effort}"),
                serde_json::json!({ "effort": effort }),
            ),
            Self::InvalidMode { mode } => (
                ErrorCode::InvalidParams,
                format!("mode not found: {mode}"),
                serde_json::json!({ "mode": mode }),
            ),
            Self::InvalidAdditionalDirectory { directory } => (
                ErrorCode::InvalidParams,
                format!(
                    "additional directory must be an absolute path without glob characters: {directory}"
                ),
                serde_json::json!({ "additionalDirectory": directory }),
            ),
            Self::AuthRequired => (
                ErrorCode::AuthRequired,
                "provider authentication required".to_owned(),
                serde_json::json!({}),
            ),
            Self::UnknownAuthMethod { method_id } => (
                ErrorCode::InvalidParams,
                format!("unknown auth method: {method_id}"),
                serde_json::json!({ "methodId": method_id }),
            ),
            Self::InvalidRequest { message, field } => {
                let data = match field {
                    Some(field) => serde_json::json!({ "field": field }),
                    None => serde_json::json!({}),
                };
                (ErrorCode::InvalidParams, message.clone(), data)
            }
            Self::ServiceFailure { safe_message, service, error_name } => {
                let mut data = serde_json::Map::new();
                if let Some(service) = service {
                    data.insert("service".to_owned(), serde_json::json!(service));
                }
                if let Some(error_name) = error_name {
                    data.insert("errorName".to_owned(), serde_json::json!(error_name));
                }
                (
                    ErrorCode::InternalError,
                    safe_message.clone(),
                    serde_json::Value::Object(data),
                )
            }
            Self::ServerUnavailable => (
                ErrorCode::InternalError,
                "OpenCode server is unavailable".to_owned(),
                serde_json::json!({ "errorName": "ServerUnavailable" }),
            ),
            Self::CatalogNotReady { reason } => (
                ErrorCode::InternalError,
                if *reason == "models" {
                    "No models are available".to_owned()
                } else {
                    "No primary agents are available".to_owned()
                },
                serde_json::json!({ "errorName": "CatalogNotReady" }),
            ),
            Self::CatalogLoad(cause) => (
                ErrorCode::InternalError,
                format!("catalog load failed: {cause}"),
                serde_json::json!({ "errorName": "CatalogLoad" }),
            ),
        };
        RequestError::new(i32::from(code), message).data(data)
    }

    /// `fromUnknown` (error.ts:134-137): collapse any unexpected panic
    /// value into a safe internal failure.
    #[must_use]
    #[allow(dead_code)]
    pub fn from_unknown() -> Self {
        Self::ServiceFailure {
            safe_message: "Internal service failure".to_owned(),
            service: None,
            error_name: None,
        }
    }
}

/// `ACPClient.classify` (client.ts:8-24): translate oc-client errors
/// into the ACP error union. Transport/decode failures mean the server
/// went away; declared errors map by tag; anything else is fatal.
pub fn classify(error: &oc_client::ClientError) -> AcpError {
    match error {
        oc_client::ClientError::Api { name, message, fields, .. } => match name.as_str() {
            "SessionNotFoundError" => AcpError::SessionNotFound {
                session_id: fields
                    .get("sessionID")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            },
            "InvalidRequestError" => AcpError::InvalidRequest {
                message: message.clone(),
                field: fields.get("field").and_then(serde_json::Value::as_str).map(str::to_owned),
            },
            "InvalidCursorError" => AcpError::InvalidRequest {
                message: message.clone(),
                field: Some("cursor".to_owned()),
            },
            _ => AcpError::CatalogLoad(format!("api error: {error}")),
        },
        // A failed body read surfaces as MalformedResponse (client.ts:12-14
        // treats a DecodeError with a cause as transport).
        oc_client::ClientError::MalformedResponse(_) | oc_client::ClientError::Transport(_) => {
            AcpError::ServerUnavailable
        }
        _ => AcpError::CatalogLoad(format!("{error}")),
    }
}

/// `ACPClient.decodeSessionID` (client.ts:26-30): the `Session.ID`
/// schema check — `ses_`-prefixed (`oc_schema::SessionId::parse`).
pub fn decode_session_id(value: &str) -> Result<String, AcpError> {
    oc_schema::SessionId::parse(value).map(|id| id.as_str().to_owned()).map_err(|_| {
        AcpError::InvalidRequest {
            message: "Invalid session ID".to_owned(),
            field: Some("sessionID".to_owned()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_not_found_maps_to_invalid_params() {
        let error = AcpError::SessionNotFound { session_id: "ses_1".to_owned() };
        let request = error.to_request_error();
        assert_eq!(request.code, agent_client_protocol::schema::v1::ErrorCode::InvalidParams);
        assert_eq!(request.message, "session not found: ses_1");
    }

    #[test]
    fn auth_required_maps_to_the_auth_required_code() {
        let request = AcpError::AuthRequired.to_request_error();
        assert_eq!(request.code, agent_client_protocol::schema::v1::ErrorCode::AuthRequired);
    }

    #[test]
    fn additional_directory_error_message_matches_v2() {
        let error = AcpError::InvalidAdditionalDirectory { directory: "rel/path".to_owned() };
        assert_eq!(
            error.to_string(),
            "additional directory must be an absolute path without glob characters: rel/path"
        );
    }

    #[test]
    fn session_ids_are_validated_like_the_schema() {
        assert!(decode_session_id("ses_abc123").is_ok());
        assert!(decode_session_id("").is_err());
        assert!(decode_session_id("msg_abc").is_err());
        assert!(decode_session_id("ses/../../etc").is_err());
    }

    #[test]
    fn transport_failures_mean_server_unavailable() {
        let raw = oc_client::ClientError::UnsupportedContentType("x".to_owned());
        assert!(matches!(classify(&raw), AcpError::CatalogLoad(_)));
    }
}
