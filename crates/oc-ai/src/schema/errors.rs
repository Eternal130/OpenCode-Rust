//! Error taxonomy (port of `src/schema/errors.ts`).
//!
//! `AIError` in TS is a tagged error wrapping one `AIErrorReason`; here
//! [`AiError`] is a thin wrapper over [`AiErrorReason`] so consumers can keep
//! one error type through `Result`, while classification (`is_retryable`,
//! `is_context_overflow`) branches on the reason. Wire tags match the TS
//! `_tag` values exactly (`InvalidRequest`, `RateLimit`, …).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ids::{ModelId, ProviderId, RouteId};

/// Classification attached to `InvalidRequest` failures that a smaller
/// request (or compaction) could recover from.
pub const CLASSIFICATION_CONTEXT_OVERFLOW: &str = "context-overflow";
/// Classification for request-size rejections that are not token-budget
/// overflows (HTTP 413 style).
pub const CLASSIFICATION_PAYLOAD_TOO_LARGE: &str = "payload-too-large";
/// Classification for streams that ended before a terminal event.
pub const CLASSIFICATION_INCOMPLETE_STREAM: &str = "incomplete-stream";

/// Failure classifications carried by `InvalidRequest` and `provider-error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderFailureClassification {
    #[serde(rename = "context-overflow")]
    ContextOverflow,
    #[serde(rename = "payload-too-large")]
    PayloadTooLarge,
}

/// Observed HTTP response context preserved on failures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpContext {
    pub url: String,
    pub status: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// Rate-limit telemetry parsed from response headers.
///
/// `limit` / `remaining` / `reset` are keyed by bucket name: OpenAI
/// `x-ratelimit-{limit,remaining,reset}-<bucket>` and Anthropic
/// `anthropic-ratelimit-<bucket>-{limit,remaining,reset}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HttpRateLimitDetails {
    #[serde(default, rename = "retryAfterMs", skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<BTreeMap<String, String>>,
}

/// Fields shared by every failure reason (port of TS `ReasonFields`).
///
/// `cause` in TS is a serialized defect (`{name, message, stack}`); here it is
/// the defect's message, which is all classification consumes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReasonFields {
    pub message: String,
    /// Complete original response or triggering event before decoding narrowed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

impl ReasonFields {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), ..Default::default() }
    }
}

/// Transport carrying a failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportType {
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "websocket")]
    Websocket,
}

/// Phase of a failed transport operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportOperation {
    #[serde(rename = "request")]
    Request,
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "write")]
    Write,
}

/// Lifecycle phase of a transport failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportPhase {
    #[serde(rename = "prepare")]
    Prepare,
    #[serde(rename = "queue")]
    Queue,
    #[serde(rename = "connect")]
    Connect,
    #[serde(rename = "send")]
    Send,
    #[serde(rename = "receive")]
    Receive,
    #[serde(rename = "decode")]
    Decode,
    #[serde(rename = "complete")]
    Complete,
    #[serde(rename = "fallback")]
    Fallback,
    #[serde(rename = "close")]
    Close,
}

/// Whether a failed request was delivered at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportDelivery {
    #[serde(rename = "not-sent")]
    NotSent,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "ambiguous")]
    Ambiguous,
    #[serde(rename = "accepted")]
    Accepted,
}

/// Recovery hint attached to transport failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportRecovery {
    #[serde(rename = "retry-connect")]
    RetryConnect,
    #[serde(rename = "retry-full")]
    RetryFull,
    #[serde(rename = "rotate-and-retry-full")]
    RotateAndRetryFull,
    #[serde(rename = "fallback-http")]
    FallbackHttp,
    #[serde(rename = "fail")]
    Fail,
}

/// One typed failure reason (port of TS `AIErrorReason`, tagged `_tag`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "_tag")]
pub enum AiErrorReason {
    #[serde(rename = "InvalidRequest")]
    InvalidRequest {
        #[serde(flatten)]
        fields: ReasonFields,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameter: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        classification: Option<ProviderFailureClassification>,
    },
    /// A caller-requested operation the selected route does not implement.
    #[serde(rename = "UnsupportedOperation")]
    UnsupportedOperation {
        #[serde(flatten)]
        fields: ReasonFields,
        operation: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<ProviderId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        route: Option<RouteId>,
    },
    /// Provider settings missing/conflicting, detected before any request
    /// exists (e.g. Azure without `resourceName`).
    #[serde(rename = "ProviderConfiguration")]
    ProviderConfiguration { provider: ProviderId, message: String },
    #[serde(rename = "NoRoute")]
    NoRoute {
        #[serde(flatten)]
        fields: ReasonFields,
        route: RouteId,
        provider: ProviderId,
        model: ModelId,
    },
    #[serde(rename = "Authentication")]
    Authentication {
        #[serde(flatten)]
        fields: ReasonFields,
    },
    #[serde(rename = "RateLimit")]
    RateLimit {
        #[serde(flatten)]
        fields: ReasonFields,
        #[serde(default, rename = "retryAfterMs", skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rate_limit: Option<HttpRateLimitDetails>,
    },
    #[serde(rename = "QuotaExceeded")]
    QuotaExceeded {
        #[serde(flatten)]
        fields: ReasonFields,
    },
    #[serde(rename = "ContentPolicy")]
    ContentPolicy {
        #[serde(flatten)]
        fields: ReasonFields,
    },
    #[serde(rename = "ProviderInternal")]
    ProviderInternal {
        #[serde(flatten)]
        fields: ReasonFields,
        #[serde(default, rename = "retryAfterMs", skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<f64>,
    },
    #[serde(rename = "Transport")]
    Transport {
        #[serde(flatten)]
        fields: ReasonFields,
        transport: TransportType,
        operation: TransportOperation,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<TransportPhase>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delivery: Option<TransportDelivery>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recovery: Option<TransportRecovery>,
    },
    #[serde(rename = "InvalidProviderOutput")]
    InvalidProviderOutput {
        #[serde(flatten)]
        fields: ReasonFields,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        classification: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        route: Option<String>,
    },
    #[serde(rename = "UnknownProvider")]
    UnknownProvider {
        #[serde(flatten)]
        fields: ReasonFields,
    },
    /// A caller-supplied deadline elapsed.
    #[serde(rename = "Timeout")]
    Timeout {
        #[serde(flatten)]
        fields: ReasonFields,
        #[serde(default, rename = "timeoutMs", skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<f64>,
    },
}

impl AiErrorReason {
    /// The `_tag` value this reason serializes to (matches TS).
    pub fn tag(&self) -> &'static str {
        match self {
            Self::InvalidRequest { .. } => "InvalidRequest",
            Self::UnsupportedOperation { .. } => "UnsupportedOperation",
            Self::ProviderConfiguration { .. } => "ProviderConfiguration",
            Self::NoRoute { .. } => "NoRoute",
            Self::Authentication { .. } => "Authentication",
            Self::RateLimit { .. } => "RateLimit",
            Self::QuotaExceeded { .. } => "QuotaExceeded",
            Self::ContentPolicy { .. } => "ContentPolicy",
            Self::ProviderInternal { .. } => "ProviderInternal",
            Self::Transport { .. } => "Transport",
            Self::InvalidProviderOutput { .. } => "InvalidProviderOutput",
            Self::UnknownProvider { .. } => "UnknownProvider",
            Self::Timeout { .. } => "Timeout",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidRequest { fields, .. }
            | Self::UnsupportedOperation { fields, .. }
            | Self::Authentication { fields }
            | Self::RateLimit { fields, .. }
            | Self::QuotaExceeded { fields }
            | Self::ContentPolicy { fields }
            | Self::ProviderInternal { fields, .. }
            | Self::Transport { fields, .. }
            | Self::InvalidProviderOutput { fields, .. }
            | Self::UnknownProvider { fields }
            | Self::Timeout { fields, .. } => &fields.message,
            Self::NoRoute { fields, .. } => &fields.message,
            Self::ProviderConfiguration { message, .. } => message,
        }
    }

    pub fn fields(&self) -> &ReasonFields {
        match self {
            Self::InvalidRequest { fields, .. }
            | Self::UnsupportedOperation { fields, .. }
            | Self::NoRoute { fields, .. }
            | Self::Authentication { fields }
            | Self::RateLimit { fields, .. }
            | Self::QuotaExceeded { fields }
            | Self::ContentPolicy { fields }
            | Self::ProviderInternal { fields, .. }
            | Self::Transport { fields, .. }
            | Self::InvalidProviderOutput { fields, .. }
            | Self::UnknownProvider { fields }
            | Self::Timeout { fields, .. } => fields,
            Self::ProviderConfiguration { .. } => {
                static EMPTY: std::sync::OnceLock<ReasonFields> = std::sync::OnceLock::new();
                EMPTY.get_or_init(ReasonFields::new)
            }
        }
    }

    /// HTTP context observed for the failure, if any.
    pub fn http(&self) -> Option<&HttpContext> {
        self.fields().http.as_ref()
    }
}

/// The AI error type (port of TS `AIError`). `message()` mirrors the reason's
/// message, matching `AIError.message`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiError {
    pub reason: AiErrorReason,
}

impl AiError {
    pub fn new(reason: AiErrorReason) -> Self {
        Self { reason }
    }

    pub fn message(&self) -> &str {
        self.reason.message()
    }

    pub fn tag(&self) -> &'static str {
        self.reason.tag()
    }
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason.message())
    }
}

impl std::error::Error for AiError {}

impl From<AiErrorReason> for AiError {
    fn from(reason: AiErrorReason) -> Self {
        Self { reason }
    }
}

/// Constructors mirroring the TS tagged-error classes.
impl AiError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(AiErrorReason::InvalidRequest { fields: ReasonFields::new(message), parameter: None, classification: None })
    }

    pub fn unsupported_operation(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(AiErrorReason::UnsupportedOperation {
            fields: ReasonFields::new(message),
            operation: operation.into(),
            provider: None,
            route: None,
        })
    }

    pub fn authentication(message: impl Into<String>) -> Self {
        Self::new(AiErrorReason::Authentication { fields: ReasonFields::new(message) })
    }

    pub fn event_error(route: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields::new(message),
            classification: None,
            route: Some(route.into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_round_trip_through_their_ts_tags() {
        let reasons = vec![
            AiErrorReason::InvalidRequest { fields: ReasonFields::new("Invalid request"), parameter: None, classification: None },
            AiErrorReason::UnsupportedOperation {
                fields: ReasonFields::new("Unsupported operation"),
                operation: "compact".into(),
                provider: Some(ProviderId::new("p")),
                route: Some("fake-route".into()),
            },
            AiErrorReason::NoRoute {
                fields: ReasonFields::new("No route"),
                route: "missing".into(),
                provider: ProviderId::new("p"),
                model: ModelId::new("m"),
            },
            AiErrorReason::Authentication { fields: ReasonFields::new("Missing credentials") },
            AiErrorReason::RateLimit {
                fields: ReasonFields::new("Rate limited"),
                retry_after_ms: Some(2000.0),
                rate_limit: None,
            },
            AiErrorReason::QuotaExceeded { fields: ReasonFields::new("Quota exceeded") },
            AiErrorReason::ContentPolicy { fields: ReasonFields::new("Content blocked") },
            AiErrorReason::ProviderInternal { fields: ReasonFields::new("Provider failed"), retry_after_ms: None },
            AiErrorReason::Transport {
                fields: ReasonFields::new("Connection failed"),
                transport: TransportType::Http,
                operation: TransportOperation::Request,
                code: None,
                url: None,
                phase: None,
                delivery: None,
                recovery: None,
            },
            AiErrorReason::InvalidProviderOutput {
                fields: ReasonFields::new("Invalid output"),
                classification: None,
                route: None,
            },
            AiErrorReason::UnknownProvider { fields: ReasonFields::new("Unknown failure") },
            AiErrorReason::Timeout { fields: ReasonFields::new("Timed out"), timeout_ms: None },
        ];
        let expected_tags = [
            "InvalidRequest",
            "UnsupportedOperation",
            "NoRoute",
            "Authentication",
            "RateLimit",
            "QuotaExceeded",
            "ContentPolicy",
            "ProviderInternal",
            "Transport",
            "InvalidProviderOutput",
            "UnknownProvider",
            "Timeout",
        ];
        for (reason, tag) in reasons.iter().zip(expected_tags) {
            assert_eq!(reason.tag(), tag, "tag mismatch");
            assert_eq!(reason.message(), reason.fields().message);
            let json = serde_json::to_value(reason).unwrap();
            assert_eq!(json["_tag"], tag, "serialized _tag mismatch for {tag}");
            let decoded: AiErrorReason = serde_json::from_value(json).unwrap();
            assert_eq!(decoded.tag(), tag, "round-trip tag mismatch");
        }
    }

    #[test]
    fn transport_errors_serialize_execution_facts() {
        let reason = AiErrorReason::Transport {
            fields: ReasonFields::new("Connection closed"),
            transport: TransportType::Websocket,
            operation: TransportOperation::Read,
            code: None,
            url: None,
            phase: Some(TransportPhase::Receive),
            delivery: Some(TransportDelivery::Ambiguous),
            recovery: Some(TransportRecovery::Fail),
        };
        let json = serde_json::to_value(&reason).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "_tag": "Transport",
                "message": "Connection closed",
                "transport": "websocket",
                "operation": "read",
                "phase": "receive",
                "delivery": "ambiguous",
                "recovery": "fail"
            })
        );
        let decoded: AiErrorReason = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, reason);
    }

    #[test]
    fn ai_error_wrapper_exposes_the_reason_message() {
        let error = AiError::invalid_request("invalid");
        assert_eq!(error.message(), "invalid");
        assert_eq!(error.to_string(), "invalid");
        assert_eq!(error.tag(), "InvalidRequest");
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["reason"]["_tag"], "InvalidRequest");
        let decoded: AiError = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, error);
    }
}
