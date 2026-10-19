#![allow(clippy::doc_markdown, reason = "OpenResponses identifiers in prose stay unquoted")]

//! OpenResponses WebSocket continuation (port of
//! `packages/ai/src/protocols/open-responses-continuation.ts`).
//!
//! Wraps a base channel driver with checkpoint-based incremental sends: a
//! completed exchange stages `{protocol: "open-responses.websocket.v1",
//! version: 1, responseID, request, output}`; the next exchange whose input
//! is the checkpoint's baseline (`[previous input, ...output]`) plus a
//! suffix sends only that suffix with `previous_response_id`, everything
//! else resends in full. Provider rejections map to recovery hints
//! (`previous_response_not_found` → retry-full,
//! `websocket_connection_limit_reached` → rotate-and-retry-full, and an
//! unclassified invalid request while incremental → retry-full).

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use crate::protocols::open_responses::{self as open_responses, error_detail};
use crate::route::transport::websocket::{
    ChannelCheckpoint, ChannelCreate, ChannelMode, ChannelObservation, WebSocketChannelDriver,
};
use crate::schema::{
    AiError, AiErrorReason, ReasonFields, TransportDelivery, TransportOperation, TransportPhase,
    TransportRecovery, TransportType,
};

/// The continuation checkpoint protocol tag (v2 `PROTOCOL`).
pub const PROTOCOL: &str = "open-responses.websocket.v1";

/// The continuation checkpoint version (v2 `VERSION`).
pub const VERSION: i64 = 1;

/// Fields to send next to `previous_response_id` on an incremental step, or
/// `None` to send the step in full (v2 `Shape`). Whether omitted fields
/// carry over from the continued response is provider behavior the route
/// must know.
pub type ContinuationShape =
    Arc<dyn Fn(&Map<String, Value>) -> Option<Map<String, Value>> + Send + Sync>;

/// The staged continuation value (v2 `CheckpointValue`).
#[derive(Debug, Clone)]
pub struct CheckpointValue {
    pub response_id: String,
    pub request: Map<String, Value>,
    pub output: Vec<Value>,
}

/// Decode a checkpoint this module produced (v2 `checkpointValue`): wrong
/// protocol/version or malformed fields return `None` (send in full).
#[must_use]
pub fn checkpoint_value(checkpoint: Option<&ChannelCheckpoint>) -> Option<CheckpointValue> {
    let checkpoint = checkpoint?;
    if checkpoint.protocol != PROTOCOL {
        return None;
    }
    let value = checkpoint.value.as_object()?;
    if value.get("version").and_then(Value::as_i64) != Some(VERSION) {
        return None;
    }
    let response_id = value.get("responseID").and_then(Value::as_str)?;
    if response_id.trim().is_empty() {
        return None;
    }
    let request = value.get("request").and_then(Value::as_object)?.clone();
    let output = value.get("output").and_then(Value::as_array)?.clone();
    Some(CheckpointValue { response_id: response_id.to_string(), request, output })
}

/// Canonical structural rendering for baseline comparison (v2 `canonical`):
/// arrays element-wise, objects as sorted `key:canonical(value)` pairs,
/// scalars as compact JSON.
fn canonical(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            format!("[{}]", items.iter().map(canonical).collect::<Vec<_>>().join(","))
        }
        Value::Object(object) => {
            format!(
                "{{{}}}",
                object
                    .iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_default(),
                        canonical(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        other => compact(other),
    }
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// v2 `json`: a string field that parses as JSON compares structurally
/// (function-call arguments may arrive as JSON strings).
fn json(value: &Value) -> Value {
    match value {
        Value::String(text) => {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| value.clone())
        }
        other => other.clone(),
    }
}

/// v2 `comparable`: strip fields that describe the response but not the
/// replayed model input — assistant message annotations/logprobs,
/// function-call extras, reasoning extras beyond summary/encrypted content.
fn comparable(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    if object.get("type").and_then(Value::as_str) == Some("message")
        && object.get("role").and_then(Value::as_str) == Some("assistant")
    {
        let mut stripped = Map::new();
        stripped.insert("role".into(), Value::String("assistant".into()));
        if let Some(content) = object.get("content") {
            let content = match content {
                Value::Array(parts) => Value::Array(
                    parts
                        .iter()
                        .map(|part| {
                            if part.get("type").and_then(Value::as_str) == Some("output_text") {
                                let mut text_only = Map::new();
                                if let Some(kind) = part.get("type") {
                                    text_only.insert("type".into(), kind.clone());
                                }
                                if let Some(text) = part.get("text") {
                                    text_only.insert("text".into(), text.clone());
                                }
                                Value::Object(text_only)
                            } else {
                                part.clone()
                            }
                        })
                        .collect(),
                ),
                other => other.clone(),
            };
            stripped.insert("content".into(), content);
        }
        if let Some(phase) = object.get("phase") {
            stripped.insert("phase".into(), phase.clone());
        }
        return Value::Object(stripped);
    }
    if object.get("type").and_then(Value::as_str) == Some("function_call") {
        let mut stripped = Map::new();
        for key in ["type", "call_id", "name"] {
            if let Some(item) = object.get(key) {
                stripped.insert(key.into(), item.clone());
            }
        }
        if let Some(arguments) = object.get("arguments") {
            stripped.insert("arguments".into(), json(arguments));
        }
        return Value::Object(stripped);
    }
    if object.get("type").and_then(Value::as_str) == Some("reasoning") {
        let mut stripped = Map::new();
        stripped.insert("type".into(), Value::String("reasoning".into()));
        if let Some(summary) = object.get("summary") {
            stripped.insert("summary".into(), summary.clone());
        }
        if let Some(encrypted) = object.get("encrypted_content") {
            stripped.insert("encrypted_content".into(), encrypted.clone());
        }
        return Value::Object(stripped);
    }
    value.clone()
}

/// v2 `invariant`: request fields that must match between the checkpoint and
/// the next request (everything except `type`, `input`,
/// `previous_response_id`).
fn invariant(request: &Map<String, Value>) -> Map<String, Value> {
    request
        .iter()
        .filter(|(key, _)| {
            key.as_str() != "type"
                && key.as_str() != "input"
                && key.as_str() != "previous_response_id"
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// The input suffix beyond the checkpoint baseline when the request extends
/// it (v2 `incremental`); `None` resends in full.
fn incremental(request: &Map<String, Value>, checkpoint: &CheckpointValue) -> Option<Vec<Value>> {
    let input = request.get("input")?.as_array()?;
    let previous_input = checkpoint.request.get("input")?.as_array()?;
    if canonical(&Value::Object(invariant(request)))
        != canonical(&Value::Object(invariant(&checkpoint.request)))
    {
        return None;
    }
    let mut baseline: Vec<Value> = previous_input.clone();
    baseline.extend(checkpoint.output.iter().cloned());
    if input.len() <= baseline.len() {
        return None;
    }
    if !(0..baseline.len()).all(|index| {
        canonical(&comparable(&baseline[index])) == canonical(&comparable(&input[index]))
    }) {
        return None;
    }
    Some(input[baseline.len()..].to_vec())
}

/// v2 `rejected`: a provider failure the continuation can recover from by
/// resending, with the recovery hint on a websocket transport error.
fn rejected(observation: ChannelObservation, recovery: TransportRecovery) -> ChannelObservation {
    let ChannelObservation::ProviderFailure { error } = observation else {
        return observation;
    };
    let fields = match &error.reason {
        AiErrorReason::Transport { fields, .. } => {
            ReasonFields { message: error.message().to_string(), ..fields.clone() }
        }
        _ => ReasonFields::new(error.message()),
    };
    let error = AiError::new(AiErrorReason::Transport {
        fields,
        transport: TransportType::Websocket,
        operation: TransportOperation::Read,
        code: None,
        url: None,
        phase: Some(TransportPhase::Receive),
        delivery: Some(TransportDelivery::Rejected),
        recovery: Some(recovery),
    });
    ChannelObservation::Rejected { error, recovery }
}

/// Inputs for [`driver`] (v2 `DriverInput`).
pub struct ContinuationDriverInput {
    pub id: String,
    pub name: String,
    /// The channel request (including `type`), with `previous_response_id`
    /// stripped — the continuation owns that field.
    pub request: Map<String, Value>,
    pub base: Arc<dyn WebSocketChannelDriver>,
    pub continuation: Option<ContinuationShape>,
}

/// The continuation driver (v2 `OpenResponsesContinuation.driver`).
pub struct ContinuationDriver {
    id: String,
    name: String,
    request: Map<String, Value>,
    base: Arc<dyn WebSocketChannelDriver>,
    continuation: Option<ContinuationShape>,
    /// `response.output_item.done` items of the running exchange.
    output: Mutex<Vec<Value>>,
}

/// Build the continuation driver over `base`.
pub fn driver(input: ContinuationDriverInput) -> Arc<dyn WebSocketChannelDriver> {
    let ContinuationDriverInput { id, name, mut request, base, continuation } = input;
    request.remove("previous_response_id");
    Arc::new(ContinuationDriver {
        id,
        name,
        request,
        base,
        continuation,
        output: Mutex::new(Vec::new()),
    })
}

impl WebSocketChannelDriver for ContinuationDriver {
    fn create(&self, checkpoint: Option<&ChannelCheckpoint>) -> Result<ChannelCreate, AiError> {
        let mut output = self.output.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *output = Vec::new();
        drop(output);
        let previous = checkpoint_value(checkpoint);
        // Ask the route first: diffing the whole history is wasted when it
        // declines the continuation (v2 default shape = identity).
        let fields = previous.as_ref().and_then(|_| {
            self.continuation
                .as_ref()
                .map_or_else(|| Some(self.request.clone()), |shape| shape(&self.request))
        });
        let delta = match (&previous, &fields) {
            (Some(previous), Some(_fields)) => incremental(&self.request, previous),
            _ => None,
        };
        match (previous, fields, delta) {
            (Some(previous), Some(fields), Some(delta)) => {
                let mut message = fields;
                message.insert("input".into(), Value::Array(delta));
                message.insert(
                    "previous_response_id".into(),
                    Value::String(previous.response_id.clone()),
                );
                Ok(ChannelCreate {
                    message: serde_json::to_string(&Value::Object(message)).map_err(|error| {
                        AiError::invalid_request(format!(
                            "Failed to encode {} continuation message: {error}",
                            self.name
                        ))
                    })?,
                    mode: ChannelMode::Incremental,
                })
            }
            _ => Ok(ChannelCreate {
                message: serde_json::to_string(&Value::Object(self.request.clone())).map_err(
                    |error| {
                        AiError::invalid_request(format!(
                            "Failed to encode {} message: {error}",
                            self.name
                        ))
                    },
                )?,
                mode: ChannelMode::Full,
            }),
        }
    }

    fn observe(&self, create: &ChannelCreate, frame: &str) -> Result<ChannelObservation, AiError> {
        let event = open_responses::decode_channel_event(frame).map_err(|error| {
            channel_event_error(&self.id, &self.name, "Invalid WebSocket event", frame, error)
        })?;
        let observation = self.base.observe(create, frame)?;
        if event.kind == "response.output_item.done" {
            if let Some(item) = event.item() {
                self.output
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(item.raw.clone());
            }
        }
        if let ChannelObservation::ProviderFailure { .. } = &observation {
            let (_message, code) = error_detail(&event);
            if code.as_deref() == Some("previous_response_not_found") {
                return Ok(rejected(observation, TransportRecovery::RetryFull));
            }
            if code.as_deref() == Some("websocket_connection_limit_reached") {
                return Ok(rejected(observation, TransportRecovery::RotateAndRetryFull));
            }
            // Only the continuation distinguishes an incremental send from a
            // full one, so an unclassified invalid request there is retried
            // full; Codex reports a stale previous_response_id that way, with
            // no code. Classified failures such as context overflow keep
            // their runner-owned recovery.
            if create.mode == ChannelMode::Incremental {
                if let AiErrorReason::InvalidRequest { classification: None, .. } =
                    &observation_error_reason(&observation)
                {
                    return Ok(rejected(observation, TransportRecovery::RetryFull));
                }
            }
        }
        if !matches!(observation, ChannelObservation::Completed { .. }) {
            return Ok(observation);
        }
        // A compaction trigger installs a different context window: clear
        // the append baseline, retaining the socket.
        if self.request.get("input").and_then(Value::as_array).is_some_and(|input| {
            input.iter().any(|item| {
                item.as_object().is_some_and(|object| {
                    object.get("type").and_then(Value::as_str) == Some("compaction_trigger")
                })
            })
        }) {
            return Ok(observation);
        }
        let response_id = event
            .response()
            .and_then(|response| response.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if response_id.trim().is_empty() {
            return Ok(observation);
        }
        let done_items =
            self.output.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        // Completion can re-encrypt reasoning: callers replay the item
        // already emitted by output_item.done.
        let output: Vec<Value> = match event
            .response()
            .and_then(|response| response.get("output"))
            .and_then(Value::as_array)
        {
            Some(items) => items
                .iter()
                .map(|item| {
                    let kind = item.get("type").and_then(Value::as_str);
                    let id = item.get("id").and_then(Value::as_str);
                    if kind == Some("reasoning") {
                        if let Some(id) = id {
                            if let Some(done) = done_items.iter().find(|done| {
                                done.get("type").and_then(Value::as_str) == kind
                                    && done.get("id").and_then(Value::as_str) == Some(id)
                            }) {
                                return done.clone();
                            }
                        }
                    }
                    item.clone()
                })
                .collect(),
            None => done_items,
        };
        let ChannelObservation::Completed { frame, .. } = observation else {
            unreachable!("completed checked above");
        };
        Ok(ChannelObservation::Completed {
            frame,
            checkpoint: Some(ChannelCheckpoint {
                protocol: PROTOCOL.to_string(),
                value: serde_json::json!({
                    "version": VERSION,
                    "responseID": response_id,
                    "request": Value::Object(self.request.clone()),
                    "output": Value::Array(output),
                }),
            }),
        })
    }
}

/// `error.reason` accessor for the InvalidRequest classification check.
fn observation_error_reason(observation: &ChannelObservation) -> &AiErrorReason {
    match observation {
        ChannelObservation::ProviderFailure { error }
        | ChannelObservation::Rejected { error, .. } => &error.reason,
        _ => unreachable!("provider failure checked by caller"),
    }
}

/// v2 `ProviderShared.eventError(id, message, frame, cause)` — keep the raw
/// frame as the body and the decode failure as the cause.
fn channel_event_error(id: &str, name: &str, label: &str, frame: &str, cause: AiError) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        route: Some(id.to_string()),
        fields: ReasonFields {
            message: format!("{name} {label}"),
            body: Some(frame.to_string()),
            cause: Some(cause.message().to_string()),
            http: None,
        },
        classification: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::transport::websocket::ChannelCreate;
    use serde_json::json;

    fn base_driver() -> Arc<dyn WebSocketChannelDriver> {
        Arc::new(FullEchoBase)
    }

    struct FullEchoBase;

    impl WebSocketChannelDriver for FullEchoBase {
        fn create(
            &self,
            _checkpoint: Option<&ChannelCheckpoint>,
        ) -> Result<ChannelCreate, AiError> {
            Ok(ChannelCreate { message: "{}".into(), mode: ChannelMode::Full })
        }

        fn observe(
            &self,
            _create: &ChannelCreate,
            frame: &str,
        ) -> Result<ChannelObservation, AiError> {
            Ok(ChannelObservation::Frame { frame: frame.to_string() })
        }
    }

    fn request(input: Value) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("model".into(), json!("gpt-test"));
        map.insert("input".into(), input);
        map.insert("type".into(), json!("response.create"));
        map
    }

    fn build_driver(request: Map<String, Value>) -> Arc<dyn WebSocketChannelDriver> {
        driver(ContinuationDriverInput {
            id: "openai-responses".into(),
            name: "OpenAI Responses".into(),
            request,
            base: base_driver(),
            continuation: None,
        })
    }

    #[test]
    fn full_send_without_checkpoint_and_incremental_with_one() {
        let input = json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
        ]);
        let driver = build_driver(request(input.clone()));
        let create = driver.create(None).unwrap();
        assert_eq!(create.mode, ChannelMode::Full);
        let message: Value = serde_json::from_str(&create.message).unwrap();
        assert_eq!(message["type"], "response.create");

        // checkpoint: previous input + output emitted back
        let output = json!([
            {"type": "message", "role": "assistant",
             "content": [{"type": "output_text", "text": "hello"}], "id": "m1", "status": "completed"}
        ]);
        let checkpoint = ChannelCheckpoint {
            protocol: PROTOCOL.into(),
            value: json!({
                "version": VERSION,
                "responseID": "resp_1",
                "request": Value::Object(request(input)),
                "output": output,
            }),
        };
        let mut next = request(json!([
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
            {"type": "message", "role": "assistant",
             "content": [{"type": "output_text", "text": "hello"}], "id": "m1", "status": "completed"},
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "again"}]}
        ]));
        next.insert("previous_response_id".into(), json!("stale"));
        let driver = build_driver(next);
        let create = driver.create(Some(&checkpoint)).unwrap();
        assert_eq!(create.mode, ChannelMode::Incremental);
        let message: Value = serde_json::from_str(&create.message).unwrap();
        assert_eq!(message["previous_response_id"], "resp_1");
        // only the suffix is sent; assistant annotations (id/status) ignored
        assert_eq!(message["input"].as_array().unwrap().len(), 1);
        assert_eq!(message["input"][0]["content"][0]["text"], "again");
        // invariant fields carry over; previous_response_id is ours to set
        assert_eq!(message["model"], "gpt-test");
    }

    #[test]
    fn changed_invariant_resends_in_full() {
        let input = json!([{"type": "message", "role": "user", "content": "hi"}]);
        let checkpoint = ChannelCheckpoint {
            protocol: PROTOCOL.into(),
            value: json!({
                "version": VERSION,
                "responseID": "resp_1",
                "request": Value::Object(request(input.clone())),
                "output": [],
            }),
        };
        let mut changed = request(json!([
            {"type": "message", "role": "user", "content": "hi"},
            {"type": "message", "role": "user", "content": "more"}
        ]));
        changed.insert("model".into(), json!("gpt-other"));
        let driver = build_driver(changed);
        assert_eq!(driver.create(Some(&checkpoint)).unwrap().mode, ChannelMode::Full);
    }

    #[test]
    fn checkpoint_decode_rejects_foreign_and_malformed() {
        assert!(checkpoint_value(None).is_none());
        let foreign = ChannelCheckpoint { protocol: "other".into(), value: json!({}) };
        assert!(checkpoint_value(Some(&foreign)).is_none());
        let bad = ChannelCheckpoint {
            protocol: PROTOCOL.into(),
            value: json!({"version": 2, "responseID": "r", "request": {}, "output": []}),
        };
        assert!(checkpoint_value(Some(&bad)).is_none());
        let empty_id = ChannelCheckpoint {
            protocol: PROTOCOL.into(),
            value: json!({"version": 1, "responseID": "  ", "request": {}, "output": []}),
        };
        assert!(checkpoint_value(Some(&empty_id)).is_none());
    }
}
