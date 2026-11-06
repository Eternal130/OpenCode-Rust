//! Gateway protocol wrapper (port of
//! `src/protocols/utils/gateway-protocol.ts`).
//!
//! Wraps an inner [`Protocol`] with the Vercel AI Gateway's two cross-cutting
//! behaviors:
//!
//! - **Request side**: `prepare` folds the gateway-specific provider options
//!   (`gateway`, `upstream`, and per-API extras like `cache_ttl` or the chat
//!   `reasoning` shape) into the request body sent through the inner
//!   protocol.
//! - **Stream side**: gateway usage/cost metadata arrives as
//!   `provider_metadata.gateway` on raw frames (root, `response`, or
//!   `choices[].delta`); it is merged across frames and attached to the
//!   finish event's `providerMetadata.gateway` (usage accounting).

use serde_json::{Map, Value};

use crate::route::protocol::{Protocol, ProtocolState};
use crate::schema::options::merge_json_records;
use crate::schema::{AiError, LLMEvent, LLMRequest};

/// The wrapped API surface (port of the `prepare(api)` discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayApi {
    /// Anthropic Messages (`/messages`).
    Messages,
    /// Open Responses (`/responses`).
    Responses,
    /// OpenAI Chat Completions (`/chat/completions`).
    Chat,
}

/// The `providerMetadata.gateway` namespace key.
const GATEWAY_KEY: &str = "gateway";

/// Wrapper state: the inner protocol state plus the merged gateway record.
pub struct GatewayState {
    inner: ProtocolState,
    gateway: Option<Map<String, Value>>,
}

/// The decoded gateway provider options (port of `GatewayOptionsSchema`).
#[derive(Debug, Default)]
struct GatewayOptions {
    gateway: Option<Map<String, Value>>,
    upstream: Option<Map<String, Value>>,
    reasoning_effort: Option<String>,
    thinking: Option<Map<String, Value>>,
    cache_ttl: Option<String>,
    cache_anchor_items: Option<Value>,
}

fn as_record(value: Option<&Value>) -> Option<Map<String, Value>> {
    value.and_then(Value::as_object).cloned()
}

/// Decode the gateway options (port of `decodeOptions`); a malformed
/// `thinking` shape is a typed invalid-request error, matching the strict
/// `Schema.Struct` decode in the reference.
fn decode_options(request: &LLMRequest) -> Result<GatewayOptions, AiError> {
    let empty = Map::new();
    let options = request.provider_options.as_ref().unwrap_or(&empty);
    let thinking = match options.get("thinking") {
        None | Some(Value::Null) => None,
        Some(value @ Value::Object(_)) => Some(value.as_object().unwrap().clone()),
        Some(other) => {
            return Err(crate::protocols::shared::invalid_request(format!(
                "Expected an object, received {other}"
            )));
        }
    };
    Ok(GatewayOptions {
        gateway: as_record(options.get("gateway")),
        upstream: as_record(options.get("upstream")),
        reasoning_effort: options
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::to_string),
        thinking,
        cache_ttl: options.get("cacheTTL").and_then(Value::as_str).map(str::to_string),
        cache_anchor_items: options
            .get("cacheAnchorItems")
            .filter(|value| value.is_number())
            .cloned(),
    })
}

/// `providerOptions` body value: `{ ...upstream, gateway }` — `None` only
/// when neither option is set (port of the `providerOptions` closure: the
/// gateway-only case still emits `{ gateway }`).
fn provider_options_body(options: &GatewayOptions) -> Option<Map<String, Value>> {
    if options.gateway.is_none() && options.upstream.is_none() {
        return None;
    }
    let mut merged = options.upstream.clone().unwrap_or_default();
    if let Some(gateway) = &options.gateway {
        merged.insert(GATEWAY_KEY.to_string(), Value::Object(gateway.clone()));
    }
    Some(merged)
}

/// The prepared request/body pair (port of `prepare`): the possibly-updated
/// request (the messages lane lowers `reasoningEffort` into `effort` +
/// `thinking`) and the extra body fields spread over the inner body.
fn prepare(
    request: &LLMRequest,
    api: GatewayApi,
) -> Result<(Option<LLMRequest>, Map<String, Value>), AiError> {
    let options = decode_options(request)?;
    let mut body = Map::new();
    if let Some(provider_options) = provider_options_body(&options) {
        body.insert("providerOptions".to_string(), Value::Object(provider_options));
    }
    match api {
        GatewayApi::Messages => {
            let Some(effort) = options.reasoning_effort else {
                return Ok((None, body));
            };
            let enabled = effort != "none";
            let mut next = request.clone();
            let mut provider_options = request.provider_options.clone().unwrap_or_default();
            if enabled {
                provider_options.insert("effort".to_string(), Value::String(effort));
            } else {
                provider_options.remove("effort");
            }
            provider_options.entry("thinking".to_string()).or_insert_with(|| {
                let mut thinking = Map::new();
                thinking.insert(
                    "type".to_string(),
                    Value::String(if enabled { "adaptive" } else { "disabled" }.to_string()),
                );
                Value::Object(thinking)
            });
            next.provider_options = Some(provider_options);
            Ok((Some(next), body))
        }
        GatewayApi::Responses => {
            if let Some(ttl) = options.cache_ttl {
                body.insert("cache_ttl".to_string(), Value::String(ttl));
            }
            if let Some(anchor) = options.cache_anchor_items {
                body.insert("cache_anchor_items".to_string(), anchor);
            }
            Ok((None, body))
        }
        GatewayApi::Chat => {
            let Some(thinking) = options.thinking else {
                return Ok((None, body));
            };
            let enabled = thinking.get("type").and_then(Value::as_str) != Some("disabled");
            let mut reasoning = Map::new();
            reasoning.insert("enabled".to_string(), Value::Bool(enabled));
            let budget = thinking
                .get("budgetTokens")
                .or_else(|| thinking.get("budget_tokens"))
                .filter(|value| value.is_number());
            if let Some(budget) = budget {
                reasoning.insert("max_tokens".to_string(), budget.clone());
            }
            body.insert("reasoning".to_string(), Value::Object(reasoning));
            Ok((None, body))
        }
    }
}

/// Push the `gateway` object of a `provider_metadata` value when present.
fn push_gateway(metadata: Option<&Value>, candidates: &mut Vec<Map<String, Value>>) {
    if let Some(gateway) = metadata
        .and_then(|metadata| metadata.get(GATEWAY_KEY))
        .filter(|gateway| gateway.is_object())
    {
        candidates.push(gateway.as_object().unwrap().clone());
    }
}

/// Attach the merged gateway record to finish events (port of
/// `attachGatewayMetadata`): the gateway-side usage/cost accounting rides
/// `providerMetadata.gateway`.
fn attach_gateway_metadata(
    events: Vec<LLMEvent>,
    gateway: Option<&Map<String, Value>>,
) -> Vec<LLMEvent> {
    let Some(gateway) = gateway else {
        return events;
    };
    if !events.iter().any(|event| matches!(event, LLMEvent::Finish { .. })) {
        return events;
    }
    events
        .into_iter()
        .map(|event| match event {
            LLMEvent::Finish { reason, usage, provider_metadata } => {
                let mut metadata = provider_metadata.unwrap_or_default();
                let entry = metadata.entry(GATEWAY_KEY.to_string()).or_default();
                for (key, value) in gateway {
                    entry.insert(key.clone(), value.clone());
                }
                LLMEvent::Finish { reason, usage, provider_metadata: Some(metadata) }
            }
            other => other,
        })
        .collect()
}

/// The gateway protocol wrapper (port of `gatewayProtocol`).
pub struct GatewayProtocol {
    id: String,
    inner: std::sync::Arc<dyn Protocol>,
    api: GatewayApi,
}

impl GatewayProtocol {
    /// Wrap `inner` for one gateway route id and API surface.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        inner: std::sync::Arc<dyn Protocol>,
        api: GatewayApi,
    ) -> Self {
        Self { id: id.into(), inner, api }
    }
}

impl Protocol for GatewayProtocol {
    fn id(&self) -> &str {
        &self.id
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        let (prepared, extra) = prepare(request, self.api)?;
        let lowered = self.inner.body_from(prepared.as_ref().unwrap_or(request))?;
        let Value::Object(mut body) = lowered else {
            return Ok(lowered);
        };
        for (key, value) in extra {
            body.insert(key, value);
        }
        Ok(Value::Object(body))
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(GatewayState { inner: self.inner.initial(request), gateway: None })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<GatewayState>().expect("gateway parser state");
        // Decode the raw frame's gateway records before the inner step (the
        // TS wrapper decodes the event ahead of `protocol.stream.step`).
        let mut candidates = Vec::new();
        if let Ok(event) = serde_json::from_str::<Value>(frame) {
            push_gateway(event.get("provider_metadata"), &mut candidates);
            push_gateway(
                event.get("response").and_then(|response| response.get("provider_metadata")),
                &mut candidates,
            );
            if let Some(choices) = event.get("choices").and_then(Value::as_array) {
                for choice in choices {
                    push_gateway(
                        choice.get("delta").and_then(|delta| delta.get("provider_metadata")),
                        &mut candidates,
                    );
                }
            }
        }
        let mut records: Vec<Option<&Map<String, Value>>> = Vec::new();
        if let Some(current) = state.gateway.as_ref() {
            records.push(Some(current));
        }
        for candidate in &candidates {
            records.push(Some(candidate));
        }
        let merged = merge_json_records(&records);
        let (inner, events) = self.inner.step(state.inner, frame)?;
        let events = attach_gateway_metadata(events, merged.as_ref());
        Ok((Box::new(GatewayState { inner, gateway: merged }), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        self.inner.terminal(frame)
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<GatewayState>().expect("gateway parser state");
        let (inner, events) = self.inner.on_halt(state.inner)?;
        let events = attach_gateway_metadata(events, state.gateway.as_ref());
        Ok((Box::new(GatewayState { inner, gateway: state.gateway }), events))
    }

    fn supports_effort_updates(&self, request: &LLMRequest) -> bool {
        self.inner.supports_effort_updates(request)
    }

    fn sanitizer(&self) -> Option<crate::schema::options::SanitizerCompatibility> {
        self.inner.sanitizer()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::openai_chat::OpenAiChat;
    use serde_json::json;
    use std::sync::Arc;

    fn chat_wrapper() -> GatewayProtocol {
        GatewayProtocol::new(
            "vercel-ai-gateway-chat",
            Arc::new(OpenAiChat::new(crate::protocols::openai_chat::OpenAiChatExt::base())),
            GatewayApi::Chat,
        )
    }

    fn request(options: Value) -> LLMRequest {
        let mut request =
            crate::test_support::request_for_tests(vec![crate::schema::Message::user("hi")]);
        if let Value::Object(map) = options {
            request.provider_options = Some(map);
        }
        request
    }

    #[test]
    fn chat_gateway_options_inject_into_the_body() {
        let protocol = chat_wrapper();
        let request = request(json!({
            "gateway": { "caching": "auto", "only": ["anthropic"] },
            "thinking": { "type": "enabled", "budgetTokens": 2048 }
        }));
        let body = protocol.body_from(&request).unwrap();
        assert_eq!(
            body["providerOptions"]["gateway"],
            json!({ "caching": "auto", "only": ["anthropic"] })
        );
        assert_eq!(body["reasoning"]["enabled"], json!(true));
        assert_eq!(body["reasoning"]["max_tokens"], json!(2048));
    }

    #[test]
    fn chat_without_gateway_options_leaves_the_body_untouched() {
        let protocol = chat_wrapper();
        let body = protocol.body_from(&request(json!({}))).unwrap();
        assert!(body.get("providerOptions").is_none());
        assert!(body.get("reasoning").is_none());
    }

    #[test]
    fn responses_cache_fields_ride_the_body() {
        let protocol = GatewayProtocol::new(
            "vercel-ai-gateway-responses",
            Arc::new(crate::protocols::open_responses::OpenResponsesProtocol::new(Arc::new(
                crate::protocols::open_responses::BaseAdapter,
            ))),
            GatewayApi::Responses,
        );
        let request = request(json!({
            "upstream": { "openai": { "store": false } },
            "cacheTTL": "1h",
            "cacheAnchorItems": 4
        }));
        let body = protocol.body_from(&request).unwrap();
        assert_eq!(body["providerOptions"]["openai"]["store"], json!(false));
        assert_eq!(body["cache_ttl"], json!("1h"));
        assert_eq!(body["cache_anchor_items"], json!(4));
    }

    #[test]
    fn messages_effort_updates_the_request_options() {
        let protocol = GatewayProtocol::new(
            "vercel-ai-gateway-messages",
            Arc::new(crate::protocols::anthropic_messages::AnthropicMessages),
            GatewayApi::Messages,
        );
        let body = protocol.body_from(&request(json!({ "reasoningEffort": "low" }))).unwrap();
        // the inner Anthropic Messages body lowers effort + thinking
        assert_eq!(body["thinking"]["type"], json!("adaptive"));
        let body = protocol.body_from(&request(json!({ "reasoningEffort": "none" }))).unwrap();
        assert_eq!(body["thinking"]["type"], json!("disabled"));
    }

    #[test]
    fn gateway_metadata_merges_across_frames_into_the_finish_event() {
        let protocol = chat_wrapper();
        let request = request(json!({}));
        let state = protocol.initial(&request);
        let delta = json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "model": "openai/gpt-5.2",
            "choices": [{ "index": 0, "delta": {
                "content": "hello",
                "provider_metadata": { "gateway": { "usage": { "input_tokens": 3 } } }
            }}]
        })
        .to_string();
        let (state, events) = protocol.step(state, &delta).unwrap();
        assert!(events.is_empty() || !events.iter().any(|e| e.kind() == "finish"));

        let done = json!({
            "id": "chatcmpl-1",
            "object": "chat.completion.chunk",
            "model": "openai/gpt-5.2",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
            "provider_metadata": { "gateway": {
                "usage": { "input_tokens": 3, "output_tokens": 5 },
                "cost": { "total": 0.001 }
            }}
        })
        .to_string();
        let (state, _events) = protocol.step(state, &done).unwrap();
        // OpenAI Chat emits the finish on halt (the [DONE] sentinel itself
        // carries no payload)
        let (_, events) = protocol.on_halt(state).unwrap();
        let finish = events
            .iter()
            .find(|event| matches!(event, LLMEvent::Finish { .. }))
            .expect("finish event");
        let LLMEvent::Finish { provider_metadata, .. } = finish else {
            unreachable!();
        };
        let gateway = provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get(GATEWAY_KEY))
            .expect("gateway metadata");
        assert_eq!(gateway.get("cost").and_then(|cost| cost.get("total")), Some(&json!(0.001)));
        // the frame record wins per key over the delta record
        assert_eq!(
            gateway.get("usage").and_then(|usage| usage.get("input_tokens")),
            Some(&json!(3))
        );
    }

    #[test]
    fn no_gateway_metadata_leaves_finish_events_untouched() {
        let protocol = chat_wrapper();
        let request = request(json!({}));
        let state = protocol.initial(&request);
        let done = json!({
            "id": "chatcmpl-2",
            "object": "chat.completion.chunk",
            "model": "openai/gpt-5.2",
            "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }]
        })
        .to_string();
        let (state, _events) = protocol.step(state, &done).unwrap();
        let (_, events) = protocol.on_halt(state).unwrap();
        let finish = events
            .iter()
            .find(|event| matches!(event, LLMEvent::Finish { .. }))
            .expect("finish event");
        let LLMEvent::Finish { provider_metadata, usage: _, .. } = finish else {
            unreachable!();
        };
        assert!(
            provider_metadata.is_none()
                || !provider_metadata.as_ref().unwrap().contains_key(GATEWAY_KEY)
        );
    }
}
