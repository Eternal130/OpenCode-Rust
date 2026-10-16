//! The media protocol seam (port of `route/media-protocol.ts`).
//!
//! v2 defines three protocol kinds — `inline`, `stream`, `queued` — through
//! constructor functions closing over an [`Identity`]. Rust mirrors the
//! shape with the [`InlineProtocol`], [`StreamProtocol`], and
//! [`QueuedProtocol`] traits; each concrete protocol module implements the
//! trait its family uses with typed internals (requests/responses stay
//! per-modality; no erasure needed until a route lane drives them
//! uniformly).
//!
//! The identity helpers below are the **error-mapping contract**: every
//! message, `_tag`, and field matches v2 byte-for-byte so consumers can
//! branch on `reason` instead of message text.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::media::body::Body;
use crate::media::model::{Addressed, DecodeContext, Mode, ResponseContext, Snapshot, Started, Status};
use crate::schema::media::{self, Asset};
use crate::schema::{AiError, AiErrorReason, HttpContext, ProviderId};

// ---------------------------------------------------------------------------
// Identity (media-protocol.ts:207-307)
// ---------------------------------------------------------------------------

/// One protocol's route id, display name, and provider
/// (media-protocol.ts:207-210).
#[derive(Debug, Clone, Copy)]
pub struct Identity {
    pub id: &'static str,
    pub name: &'static str,
    pub provider: &'static str,
}

impl Identity {
    #[must_use]
    pub const fn new(id: &'static str, name: &'static str, provider: &'static str) -> Self {
        Self { id, name, provider }
    }

    /// `frameError` (media-protocol.ts:295): an `InvalidProviderOutput`
    /// carrying the route id; the frame stays on `reason.body`.
    pub fn frame_error(&self, message: impl Into<String>, body: impl Into<String>) -> AiError {
        AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: crate::schema::ReasonFields {
                message: message.into(),
                body: Some(body.into()),
                http: None,
                cause: None,
            },
            classification: None,
            route: Some(self.id.to_string()),
        })
    }

    /// `incomplete` (media-protocol.ts:296-303).
    pub fn incomplete(&self) -> AiError {
        AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: crate::schema::ReasonFields {
                message: "The provider response ended unexpectedly.".to_string(),
                body: None,
                http: None,
                cause: None,
            },
            classification: Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM.to_string()),
            route: Some(self.id.to_string()),
        })
    }

    /// `unsupported` (media-protocol.ts:304-306).
    pub fn unsupported(&self, operation: impl Into<String>, message: impl Into<String>) -> AiError {
        AiError::new(AiErrorReason::UnsupportedOperation {
            fields: crate::schema::ReasonFields::new(message),
            operation: operation.into(),
            provider: Some(ProviderId::new(self.provider)),
            route: Some(self.id.to_string()),
        })
    }

    /// The `rejectUnsupported` route-layer error (route/media.ts:423-448):
    /// a present field the protocol declared unsupported fails typed.
    /// `false` counts as present because some booleans mean something when
    /// false (video `audio`); empty arrays count as absent.
    pub fn reject_unsupported(&self, present: &[&str]) -> Option<AiError> {
        if present.is_empty() {
            return None;
        }
        Some(AiError::new(AiErrorReason::UnsupportedOperation {
            fields: crate::schema::ReasonFields::new(format!(
                "{}/{} does not support {}",
                self.provider,
                self.id,
                present.join(", ")
            )),
            operation: format!("media.{}", present[0]),
            provider: Some(ProviderId::new(self.provider)),
            route: Some(self.id.to_string()),
        }))
    }
}

/// Reasons a provider can report for a `failed` generation; anything it
/// does not classify is `ProviderInternal` (media-protocol.ts:194-201).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    InvalidRequest,
    Authentication,
    RateLimit,
    ProviderInternal,
}

impl Failure {
    fn error(self, message: String, body: String, http: Option<HttpContext>) -> AiError {
        let fields = crate::schema::ReasonFields {
            message,
            body: Some(body),
            http,
            cause: None,
        };
        AiError::new(match self {
            Self::InvalidRequest => AiErrorReason::InvalidRequest {
                fields,
                parameter: None,
                classification: None,
            },
            Self::Authentication => AiErrorReason::Authentication { fields },
            Self::RateLimit => AiErrorReason::RateLimit {
                fields,
                retry_after_ms: None,
                rate_limit: None,
            },
            Self::ProviderInternal => AiErrorReason::ProviderInternal {
                fields,
                retry_after_ms: None,
            },
        })
    }
}

/// The read side of one provider response (the `text` output of
/// `MediaProtocol.identity`, media-protocol.ts:219-260): the body plus the
/// HTTP context every downstream error carries.
pub struct TextOutput<'a> {
    pub identity: &'a Identity,
    pub body: &'a str,
    pub http: Option<&'a HttpContext>,
}

impl<'a> TextOutput<'a> {
    fn fields(&self, message: impl Into<String>) -> crate::schema::ReasonFields {
        crate::schema::ReasonFields {
            message: message.into(),
            body: Some(self.body.to_string()),
            http: self.http.cloned(),
            cause: None,
        }
    }

    /// A malformed provider document (media-protocol.ts:237-239).
    pub fn invalid(&self, message: impl Into<String>) -> AiError {
        AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: self.fields(message),
            classification: None,
            route: Some(self.identity.id.to_string()),
        })
    }

    /// A generation that reached a terminal status without output
    /// (media-protocol.ts:239-249). `failed` carries the provider's
    /// classification (defaulting to `ProviderInternal`);
    /// `cancelled`/`expired` mean the result will never exist.
    pub fn ended(&self, status: Status, message: impl Into<String>, failure: Failure) -> AiError {
        match status {
            Status::Failed => failure.error(message.into(), self.body.to_string(), self.http.cloned()),
            _ => AiError::new(AiErrorReason::InvalidRequest {
                fields: self.fields(message),
                parameter: None,
                classification: None,
            }),
        }
    }

    /// A `result()` read before the generation finished — caller misuse
    /// (media-protocol.ts:250-257).
    pub fn pending(&self, id: &str) -> AiError {
        AiError::new(AiErrorReason::InvalidRequest {
            fields: self.fields(format!(
                "{} generation {id} has not finished; await it before reading the result",
                self.identity.name
            )),
            parameter: None,
            classification: None,
        })
    }

    /// A moderated result (media-protocol.ts:258).
    pub fn content_policy(&self, message: impl Into<String>) -> AiError {
        AiError::new(AiErrorReason::ContentPolicy { fields: self.fields(message) })
    }

    /// Read and decode a JSON body, keeping decode failures as `invalid`
    /// with the raw body (media-protocol.ts:263-272).
    pub fn decode_json(&self, value: &str) -> Result<serde_json::Value, AiError> {
        serde_json::from_str(value).map_err(|cause| {
            let mut error = self.invalid(format!("{} returned an invalid response", self.identity.name));
            if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                fields.cause = Some(cause.to_string());
            }
            error
        })
    }

    /// Schema-decode one JSON stream frame; decode failures keep the frame
    /// as `reason.body` (media-protocol.ts:287-293).
    pub fn decode_frame(&self, frame: &str) -> Result<serde_json::Value, AiError> {
        serde_json::from_str(frame).map_err(|cause| {
            let mut error = self
                .identity
                .frame_error(format!("{} sent an invalid stream event", self.identity.name), frame);
            if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                fields.cause = Some(cause.to_string());
            }
            error
        })
    }
}

/// Map a provider status string through the protocol's table; unknown
/// values are an invalid provider document (media-protocol.ts:314-321).
pub fn status(table: &BTreeMap<&str, Status>, raw: &str, output: &TextOutput) -> Result<Status, AiError> {
    table.get(raw).copied().ok_or_else(|| output.invalid(format!("Unknown generation status \"{raw}\"")))
}

/// Map a provider error code through the protocol's table; missing or
/// unmapped codes are `ProviderInternal` (media-protocol.ts:323-325).
#[must_use]
pub fn failure<K: std::hash::Hash + Eq + Copy>(table: &std::collections::HashMap<K, Failure>, code: Option<K>) -> Failure {
    code.and_then(|code| table.get(&code).copied()).unwrap_or(Failure::ProviderInternal)
}

/// Milliseconds since the epoch (the `Clock.currentTimeMillis` seam; tests
/// inject through `expiring_url_at`).
#[must_use]
pub fn now_millis() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

/// A `url` asset whose provider-declared retention window starts `now`
/// (media-protocol.ts:328-331).
#[must_use]
pub fn expiring_url(
    url: impl Into<String>,
    retention_ms: f64,
    media_type: Option<String>,
    headers: Option<&BTreeMap<String, String>>,
) -> Asset {
    expiring_url_at(url, now_millis(), retention_ms, media_type, headers)
}

/// `expiringUrl` with an injected clock (test seam).
#[must_use]
pub fn expiring_url_at(
    url: impl Into<String>,
    now: f64,
    retention_ms: f64,
    media_type: Option<String>,
    headers: Option<&BTreeMap<String, String>>,
) -> Asset {
    let mut asset = media::url(url, media_type.clone(), Some(now + retention_ms));
    if let Some(headers) = headers.filter(|headers| !headers.is_empty()) {
        let mut namespace = std::collections::BTreeMap::new();
        namespace.insert(
            "headers".to_string(),
            serde_json::Value::Object(headers.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect()),
        );
        let mut metadata = asset.provider_metadata.take().unwrap_or_default();
        metadata.insert("route".to_string(), namespace);
        asset.provider_metadata = Some(metadata);
    }
    asset
}

// ---------------------------------------------------------------------------
// Protocol kinds (media-protocol.ts:64-187)
// ---------------------------------------------------------------------------

/// Common identity + lowering every protocol kind shares.
pub trait MediaProtocolBase: Send + Sync {
    /// Stable wire id (e.g. `"openai-images"`).
    fn identity(&self) -> Identity;

    /// Common request fields this protocol cannot lower; the route rejects
    /// them before `body_from` runs (media-protocol.ts:69-70).
    fn unsupported(&self) -> &'static [&'static str];

    /// Default deployment base URL (the TS model factory's `baseURL`).
    fn base_url(&self) -> String;

    /// Request-deployment headers (e.g. Runway's `X-Runway-Version`).
    fn headers(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// One request, one response. JSON or multipart in; JSON or raw bytes out
/// (media-protocol.ts:65-78).
pub trait InlineProtocol: MediaProtocolBase {
    type Request: Send + Sync;
    type Response: Send + Sync;

    /// Request side (the route runs [`Identity::reject_unsupported`]
    /// first).
    fn body_from(&self, request: &Self::Request) -> Result<Body, AiError>;

    /// The endpoint path for one request.
    fn path(&self, request: &Self::Request) -> String;

    /// Response side: decode the read body.
    fn decode(
        &self,
        output: &TextOutput,
        context: &DecodeContext<Self::Request>,
    ) -> Result<Self::Response, AiError>;
}

/// One request whose body is parsed incrementally, like LLM protocols:
/// `frames` → `step`\* → `finish` (media-protocol.ts:167-182). `generate`
/// and `stream` share this state machine; `request.mode` lets a protocol
/// pick a different body, path, or framing.
pub trait StreamProtocol: MediaProtocolBase {
    type Request: Send + Sync;
    type Event: Send + Sync;
    type State: Default + Send + Sync;

    fn body_from(&self, request: &Addressed<Self::Request>) -> Result<Body, AiError>;
    fn path(&self, request: &Addressed<Self::Request>) -> String;

    /// How the raw response bytes frame into [`Frame`]s (the TS `frames`
    /// combinator picked per request/body).
    fn framing(&self, request: &Addressed<Self::Request>, body: &Body) -> Framing;

    /// Emit exactly one terminal event, or fail when the provider stopped
    /// before completing (media-protocol.ts:180-181).
    fn finish(
        &self,
        state: &Self::State,
        context: &ResponseContext<Self::Request>,
    ) -> Result<Vec<Self::Event>, AiError>;
}

/// Submit, then poll (media-protocol.ts:109-148). `start` posts the body to
/// the route endpoint; `status`, `result`, and `cancel` are follow-up
/// calls addressed by the token.
pub trait QueuedProtocol: MediaProtocolBase {
    type Request: Send + Sync;
    type Response: Send + Sync;
    type Token: Send + Sync + Clone;

    fn body_from(&self, request: &Self::Request) -> Result<Body, AiError>;
    fn start_path(&self, request: &Self::Request) -> String;

    /// Decode a submission response into the token and first snapshot.
    fn decode_start(&self, output: &TextOutput) -> Result<Started<Self::Token>, AiError>;

    fn status_path(&self, token: &Self::Token) -> String;
    fn decode_status(
        &self,
        output: &TextOutput,
        context: &crate::media::model::PollContext<Self::Token>,
    ) -> Result<Snapshot, AiError>;

    fn result_path(&self, token: &Self::Token) -> String;
    fn decode_result(
        &self,
        output: &TextOutput,
        context: &crate::media::model::PollContext<Self::Token>,
    ) -> Result<Self::Response, AiError>;

    /// Optional cancel endpoint (media-protocol.ts:139-147).
    fn cancel(&self) -> Option<CancelSpec> {
        None
    }
}

/// A queued protocol's cancel endpoint (media-protocol.ts:139-147).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelSpec {
    pub method: &'static str,
    /// Fetch a fresh status first and skip the call for terminal
    /// generations, for providers whose cancel endpoint destroys finished
    /// work (Runway's `DELETE /v1/tasks/{id}`).
    pub active_only: bool,
}

/// How raw response bytes frame (the `frames` combinator's choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// `Framing.sse` — `data:`-prefixed SSE events.
    Sse,
    /// `Framing.lines` — newline-delimited JSON.
    Lines,
    /// `Framing.document` — the whole body as one document.
    Document,
    /// Raw bytes passed through unframed (audio chunks).
    Bytes,
}

/// One decoded frame (the TS `Frame` unions: `string`, `Uint8Array`, or
/// `{ document, requested }`).
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Text(String),
    Bytes(Vec<u8>),
    /// A whole-document read carrying the requested `output_format` for
    /// responses that omit it (openai-images:89-90).
    Document { document: String, requested: Option<String> },
}

impl Frame {
    /// Parse a complete transport payload into frames per [`Framing`]
    /// (through the route layer's incremental decoder, so SSE edge cases —
    /// multi-line `data:`, `[DONE]`, keepalives — match the LLM routes).
    /// For `Document` the caller supplies the body's requested-format hint.
    #[must_use]
    pub fn parse_all(kind: Framing, bytes: &[u8], requested: Option<String>) -> Vec<Self> {
        use crate::route::framing::{FramingDecoder, Framing as RouteFraming};
        if matches!(kind, Framing::Bytes) {
            return vec![Self::Bytes(bytes.to_vec())];
        }
        let route_framing = match kind {
            Framing::Sse => RouteFraming::sse(),
            Framing::Lines => RouteFraming::lines(),
            _ => RouteFraming::document(),
        };
        let mut decoder = FramingDecoder::new(route_framing);
        let mut texts = decoder.feed(bytes).unwrap_or_default();
        texts.extend(decoder.finish().unwrap_or_default());
        match kind {
            Framing::Document => texts
                .into_iter()
                .map(|document| Self::Document { document, requested: requested.clone() })
                .collect(),
            _ => texts.into_iter().map(Self::Text).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_error_carries_operation_provider_and_route() {
        let identity = Identity::new("bfl-images", "Black Forest Labs", "black-forest-labs");
        let error = identity.unsupported("media.n", "flux-pro generates one image per request");
        match error.reason {
            AiErrorReason::UnsupportedOperation { operation, provider, route, fields } => {
                assert_eq!(operation, "media.n");
                assert_eq!(provider.as_ref().map(|p| p.0.as_str()), Some("black-forest-labs"));
                assert_eq!(route.as_deref(), Some("bfl-images"));
                assert_eq!(fields.message, "flux-pro generates one image per request");
            }
            other => panic!("wrong reason: {other:?}"),
        }
    }

    #[test]
    fn reject_unsupported_lists_present_fields() {
        let identity = Identity::new("zai-images", "Z.ai Images", "zai");
        let error = identity.reject_unsupported(&["images", "mask"]).unwrap();
        let AiErrorReason::UnsupportedOperation { operation, fields, .. } = error.reason else {
            panic!("wrong reason");
        };
        assert_eq!(operation, "media.images");
        assert_eq!(fields.message, "zai/zai-images does not support images, mask");
        assert!(identity.reject_unsupported(&[]).is_none());
    }

    #[test]
    fn ended_maps_failure_taxonomy() {
        let identity = Identity::new("google-video", "Google Veo", "google");
        let output = TextOutput { identity: &identity, body: "{}", http: None };
        let failed = output.ended(Status::Failed, "op failed", Failure::RateLimit);
        assert!(matches!(failed.reason, AiErrorReason::RateLimit { .. }));
        let cancelled = output.ended(Status::Cancelled, "gone", Failure::ProviderInternal);
        assert!(matches!(cancelled.reason, AiErrorReason::InvalidRequest { .. }));
    }

    #[test]
    fn pending_and_content_policy_messages_match_v2() {
        let identity = Identity::new("bfl-images", "Black Forest Labs", "black-forest-labs");
        let output = TextOutput { identity: &identity, body: "", http: None };
        let pending = output.pending("abc123");
        let AiErrorReason::InvalidRequest { fields, .. } = pending.reason else { panic!() };
        assert_eq!(
            fields.message,
            "Black Forest Labs generation abc123 has not finished; await it before reading the result"
        );
        let policy = output.content_policy("moderated");
        assert!(matches!(policy.reason, AiErrorReason::ContentPolicy { .. }));
    }

    #[test]
    fn status_table_rejects_unknown_values() {
        let identity = Identity::new("replicate-images", "Replicate", "replicate");
        let output = TextOutput { identity: &identity, body: "", http: None };
        let table: BTreeMap<&str, Status> =
            [("starting", Status::Queued), ("succeeded", Status::Completed)].into_iter().collect();
        assert_eq!(status(&table, "starting", &output).unwrap(), Status::Queued);
        let error = status(&table, "exploded", &output).unwrap_err();
        let AiErrorReason::InvalidProviderOutput { fields, .. } = error.reason else { panic!() };
        assert_eq!(fields.message, "Unknown generation status \"exploded\"");
    }

    #[test]
    fn failure_table_defaults_to_provider_internal() {
        let mut table = std::collections::HashMap::new();
        table.insert(3, Failure::InvalidRequest);
        assert_eq!(failure(&table, Some(3)), Failure::InvalidRequest);
        assert_eq!(failure(&table, Some(99)), Failure::ProviderInternal);
        assert_eq!(failure(&table, None), Failure::ProviderInternal);
    }

    #[test]
    fn incomplete_carries_the_classification() {
        let identity = Identity::new("openai-speech", "OpenAI Speech", "openai");
        let error = identity.incomplete();
        let AiErrorReason::InvalidProviderOutput { classification, route, fields, .. } = error.reason
        else {
            panic!()
        };
        assert_eq!(classification.as_deref(), Some("incomplete-stream"));
        assert_eq!(route, Some("openai-speech".to_string()));
        assert_eq!(fields.message, "The provider response ended unexpectedly.");
    }

    #[test]
    fn expiring_url_sets_deadline_and_optional_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer k".to_string());
        let asset = expiring_url_at("https://x/v.mp4", 1_000.0, 86_400_000.0, Some("video/mp4".into()), Some(&headers));
        assert_eq!(asset.expires_at, Some(86_401_000.0));
        assert_eq!(asset.media_type, "video/mp4");
        assert_eq!(
            asset.provider_metadata.as_ref().unwrap()["route"]["headers"]["authorization"],
            serde_json::json!("Bearer k")
        );
    }

    #[test]
    fn frames_parse_per_framing_kind() {
        let sse = Frame::parse_all(Framing::Sse, b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n", None);
        assert_eq!(sse.len(), 2);
        let lines = Frame::parse_all(Framing::Lines, b"{\"a\":1}\n{\"b\":2}\n", None);
        assert_eq!(lines.len(), 2);
        let document = Frame::parse_all(Framing::Document, b"{\"ok\":true}", Some("png".into()));
        assert_eq!(
            document,
            vec![Frame::Document { document: "{\"ok\":true}".to_string(), requested: Some("png".to_string()) }]
        );
        let bytes = Frame::parse_all(Framing::Bytes, &[1, 2, 3], None);
        assert_eq!(bytes, vec![Frame::Bytes(vec![1, 2, 3])]);
    }
}
