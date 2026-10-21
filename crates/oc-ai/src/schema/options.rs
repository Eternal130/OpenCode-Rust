//! Request/model options (port of `src/schema/options.ts`).
//!
//! Wire shapes use camelCase to match the TS contract exactly. Usage math
//! direction semantics (oracle gate 2, finding 17) live with
//! [`crate::protocols::shared`] token helpers and [`crate::schema::events::Usage`].

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::{ModelId, ProviderId};

/// A JSON object (record) on the wire.
pub type JsonRecord = serde_json::Map<String, Value>;

/// JSON Schema document describing a tool input/output.
pub type JsonSchema = Value;

/// Arbitrary per-provider request options (`Record<string, unknown>`).
pub type ProviderOptions = serde_json::Map<String, Value>;

/// Escape hatch for fields this contract does not normalize, keyed by
/// provider name (`{ openai: … }`, `{ anthropic: … }`).
///
/// Note: key order is sorted (`BTreeMap`) — byte-exact *values* round-trip, but
/// re-serialized key order is not insertion-stable vs TS. Known non-goal
/// (oracle gate 2, finding 9).
pub type ProviderMetadata = BTreeMap<String, BTreeMap<String, Value>>;

fn is_record(value: &Value) -> bool {
    value.is_object()
}

/// Deep-merge JSON records: later items win per key; record values merge
/// recursively (port of `mergeJsonRecords`). Returns `None` when no input is
/// defined or the merged result is empty.
#[must_use]
pub fn merge_json_records(items: &[Option<&JsonRecord>]) -> Option<JsonRecord> {
    let defined: Vec<&JsonRecord> = items.iter().flatten().copied().collect();
    if defined.is_empty() {
        return None;
    }
    if defined.len() == 1 {
        return Some(defined[0].clone());
    }
    let mut result: JsonRecord = serde_json::Map::new();
    for item in defined {
        for (key, value) in item {
            let merged = match (result.get(key), value) {
                (Some(existing), value) if is_record(existing) && is_record(value) => {
                    let nested = merge_json_records(&[
                        Some(existing.as_object().unwrap()),
                        Some(value.as_object().unwrap()),
                    ]);
                    Value::Object(nested.unwrap_or_default())
                }
                _ => value.clone(),
            };
            result.insert(key.clone(), merged);
        }
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Shallow-merge string records; later items win (port of `mergeStringRecords`).
#[must_use]
pub fn merge_string_records(
    items: &[Option<&BTreeMap<String, String>>],
) -> Option<BTreeMap<String, String>> {
    let defined: Vec<&BTreeMap<String, String>> = items.iter().flatten().copied().collect();
    if defined.is_empty() {
        return None;
    }
    if defined.len() == 1 {
        return Some(defined[0].clone());
    }
    let mut result = BTreeMap::new();
    for item in defined {
        result.extend(item.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Merge provider options (deep record merge, `None` when nothing defined).
#[must_use]
pub fn merge_provider_options(items: &[Option<&ProviderOptions>]) -> Option<ProviderOptions> {
    merge_json_records(items)
}

/// Milliseconds for an HTTP timeout, or `false` to disable it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpTimeout {
    Ms(f64),
    Disabled,
}

impl Serialize for HttpTimeout {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            HttpTimeout::Ms(ms) => serializer.serialize_f64(*ms),
            HttpTimeout::Disabled => serializer.serialize_bool(false),
        }
    }
}

impl<'de> Deserialize<'de> for HttpTimeout {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Bool(false) => Ok(HttpTimeout::Disabled),
            Value::Number(n) if n.as_f64().is_some_and(|v| v > 0.0) => {
                Ok(HttpTimeout::Ms(n.as_f64().unwrap()))
            }
            _ => Err(serde::de::Error::custom("expected a positive number or false")),
        }
    }
}

/// Applied to `headerTimeout` and `chunkTimeout` when a request leaves them unset.
pub const DEFAULT_HTTP_TIMEOUT_MS: f64 = 300_000.0;

/// Per-request HTTP overrides (port of `HttpOptions`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpOptions {
    /// Overlay merged into the JSON request body before encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<BTreeMap<String, String>>,
    /// Time allowed for the whole request. Unbounded when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<HttpTimeout>,
    /// Time allowed for response headers to arrive.
    #[serde(default, rename = "headerTimeout", skip_serializing_if = "Option::is_none")]
    pub header_timeout: Option<HttpTimeout>,
    /// Time allowed between streamed chunks once headers arrived.
    #[serde(default, rename = "chunkTimeout", skip_serializing_if = "Option::is_none")]
    pub chunk_timeout: Option<HttpTimeout>,
}

/// Merge HTTP options: maps merge, timeouts take the **last** defined value
/// (port of `mergeHttpOptions`, `findLast` semantics). `None` when nothing
/// is defined.
#[must_use]
pub fn merge_http_options(items: &[Option<&HttpOptions>]) -> Option<HttpOptions> {
    let body = merge_json_records(
        &items.iter().map(|o| o.and_then(|o| o.body.as_ref())).collect::<Vec<_>>(),
    );
    let headers = merge_string_records(
        &items.iter().map(|o| o.and_then(|o| o.headers.as_ref())).collect::<Vec<_>>(),
    );
    let query = merge_string_records(
        &items.iter().map(|o| o.and_then(|o| o.query.as_ref())).collect::<Vec<_>>(),
    );
    let timeout = items.iter().flatten().rev().find_map(|o| o.timeout).map(|v| match v {
        HttpTimeout::Ms(ms) => HttpTimeout::Ms(ms),
        HttpTimeout::Disabled => HttpTimeout::Disabled,
    });
    let header_timeout = items.iter().flatten().rev().find_map(|o| o.header_timeout);
    let chunk_timeout = items.iter().flatten().rev().find_map(|o| o.chunk_timeout);
    if body.is_none()
        && headers.is_none()
        && query.is_none()
        && timeout.is_none()
        && header_timeout.is_none()
        && chunk_timeout.is_none()
    {
        return None;
    }
    Some(HttpOptions { body, headers, query, timeout, header_timeout, chunk_timeout })
}

/// Sampling/generation knobs (port of `GenerationOptions`). Numbers are `f64`
/// to match JS number semantics exactly (oracle gate 2, finding 17).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

impl GenerationOptions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Merge generation options per key; each key takes the **last** defined
/// value (port of `mergeGenerationOptions`). `None` when every field is unset.
#[must_use]
pub fn merge_generation_options(items: &[Option<&GenerationOptions>]) -> Option<GenerationOptions> {
    let last = |get: fn(&GenerationOptions) -> Option<f64>| -> Option<f64> {
        items.iter().flatten().rev().find_map(|o| get(o))
    };
    let stop = items.iter().flatten().rev().find_map(|o| o.stop.clone());
    let merged = GenerationOptions {
        max_tokens: last(|o| o.max_tokens),
        temperature: last(|o| o.temperature),
        top_p: last(|o| o.top_p),
        top_k: last(|o| o.top_k),
        frequency_penalty: last(|o| o.frequency_penalty),
        presence_penalty: last(|o| o.presence_penalty),
        seed: last(|o| o.seed),
        stop,
    };
    if merged.is_empty() {
        None
    } else {
        Some(merged)
    }
}

/// Selected-model request defaults without precedence (route ⊕ model merge
/// happens at request compile time).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<GenerationOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpOptions>,
}

/// Known reasoning-effort values; providers may send any string
/// (TS `OpenString<Known>`).
pub const REASONING_EFFORTS: [&str; 7] =
    ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Tool-schema sanitizer for a model family; `None` opts out of protocol and
/// model-name defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SanitizerCompatibility {
    Gemini,
    Moonshot,
    None,
}

/// `maxTokens` wire field name preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaxTokensFieldCompatibility {
    #[serde(rename = "max_completion_tokens")]
    MaxCompletionTokens,
    #[serde(rename = "max_tokens")]
    MaxTokens,
}

/// Wire compatibility knobs consulted inside protocol body builders/parsers
/// (port of `LanguageModelCompatibility` — all 14 fields from options.ts:189).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageModelCompatibility {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sanitizer: Option<SanitizerCompatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_field: Option<String>,
    /// Require every assistant message to include its reasoning field, even empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_field: Option<MaxTokensFieldCompatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_finish_reason: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_assistant_after_tool: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_store: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_usage_in_streaming: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_strict_mode: Option<bool>,
    /// Accepts `prompt_cache_key` in the Chat Completions body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_prompt_cache_key: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zai_tool_stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_signature: Option<bool>,
    /// Anthropic thinking-prefix mismatch controls; overrides model-ID detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_thinking_block_binding: Option<bool>,
    /// Per-message effort updates; overrides model-ID detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_effort_updates: Option<bool>,
}

/// A fully-configured model reference carrying its route (port of
/// `LanguageModel`). Not serializable: the route is behavioral.
#[derive(Clone)]
pub struct LanguageModel {
    pub id: ModelId,
    pub provider: ProviderId,
    pub route: Arc<crate::route::Route>,
    pub defaults: Option<LanguageModelDefaults>,
    pub compatibility: Option<LanguageModelCompatibility>,
}

impl std::fmt::Debug for LanguageModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageModel")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("route", &self.route.id)
            .finish_non_exhaustive()
    }
}

/// Patch accepted by [`LanguageModel::update`]; `None` fields keep the current
/// value.
#[derive(Default)]
pub struct LanguageModelPatch {
    pub id: Option<ModelId>,
    pub provider: Option<ProviderId>,
    pub route: Option<Arc<crate::route::Route>>,
    pub defaults: Option<Option<LanguageModelDefaults>>,
    pub compatibility: Option<Option<LanguageModelCompatibility>>,
}

impl LanguageModel {
    pub fn new(
        id: impl Into<String>,
        provider: impl Into<String>,
        route: Arc<crate::route::Route>,
    ) -> Self {
        Self {
            id: ModelId::new(id),
            provider: ProviderId::new(provider),
            route,
            defaults: None,
            compatibility: None,
        }
    }

    #[must_use]
    pub fn with_defaults(mut self, defaults: LanguageModelDefaults) -> Self {
        self.defaults = Some(defaults);
        self
    }

    #[must_use]
    pub fn with_compatibility(mut self, compatibility: LanguageModelCompatibility) -> Self {
        self.compatibility = Some(compatibility);
        self
    }

    /// Immutable update; empty patches return the same value conceptually
    /// (a clone — Rust has no reference identity to preserve).
    #[must_use]
    pub fn update(&self, patch: LanguageModelPatch) -> Self {
        let mut next = self.clone();
        if let Some(id) = patch.id {
            next.id = id;
        }
        if let Some(provider) = patch.provider {
            next.provider = provider;
        }
        if let Some(route) = patch.route {
            next.route = route;
        }
        if let Some(defaults) = patch.defaults {
            next.defaults = defaults;
        }
        if let Some(compatibility) = patch.compatibility {
            next.compatibility = compatibility;
        }
        next
    }
}

/// Inline cache marker on a part/tool (`CacheHint`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheHint {
    #[serde(rename = "type")]
    pub kind: CacheHintKind,
    #[serde(default, rename = "ttlSeconds", skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheHintKind {
    #[serde(rename = "ephemeral")]
    Ephemeral,
    #[serde(rename = "persistent")]
    Persistent,
}

impl CacheHint {
    #[must_use]
    pub fn ephemeral() -> Self {
        Self { kind: CacheHintKind::Ephemeral, ttl_seconds: None }
    }

    #[must_use]
    pub fn ephemeral_ttl(ttl_seconds: f64) -> Self {
        Self { kind: CacheHintKind::Ephemeral, ttl_seconds: Some(ttl_seconds) }
    }
}

/// Where auto cache breakpoints land on conversation messages:
/// `"latest-user-message"`, `"latest-assistant"`, or `{"tail": n}`.
///
/// Serialization is manual: `untagged` cannot rename unit variants.
#[derive(Debug, Clone, PartialEq)]
pub enum CachePolicyMessages {
    LatestUserMessage,
    LatestAssistant,
    Tail { tail: u32 },
}

impl Serialize for CachePolicyMessages {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            CachePolicyMessages::LatestUserMessage => {
                serializer.serialize_str("latest-user-message")
            }
            CachePolicyMessages::LatestAssistant => serializer.serialize_str("latest-assistant"),
            CachePolicyMessages::Tail { tail } => {
                use serde::ser::SerializeStruct;
                let mut state = serializer.serialize_struct("CachePolicyMessages", 1)?;
                state.serialize_field("tail", tail)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for CachePolicyMessages {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(text) if text == "latest-user-message" => Ok(CachePolicyMessages::LatestUserMessage),
            Value::String(text) if text == "latest-assistant" => Ok(CachePolicyMessages::LatestAssistant),
            Value::Object(object) => {
                let tail = object.get("tail").cloned().unwrap_or(Value::Null);
                u32::deserialize(tail).map(|tail| CachePolicyMessages::Tail { tail }).map_err(serde::de::Error::custom)
            }
            other => Err(serde::de::Error::custom(format!(
                "expected \"latest-user-message\", \"latest-assistant\", or {{\"tail\": n}}, got {other}"
            ))),
        }
    }
}

/// Granular object form of a [`CachePolicy`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePolicyObject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<CachePolicyMessages>,
    #[serde(default, rename = "ttlSeconds", skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<f64>,
}

impl CachePolicyObject {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Auto-placement policy for prompt caching. `"auto"` places breakpoints at
/// the last tool definition, first/last distinct system parts, and the
/// conversation tail; `"none"` opts out; the object form overrides choices.
///
/// Serialization is manual: `untagged` cannot rename unit variants, and the
/// wire forms are exactly `"auto"` / `"none"` / the object shape.
#[derive(Debug, Clone, PartialEq)]
pub enum CachePolicy {
    Auto,
    None,
    Object(CachePolicyObject),
}

impl Serialize for CachePolicy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            CachePolicy::Auto => serializer.serialize_str("auto"),
            CachePolicy::None => serializer.serialize_str("none"),
            CachePolicy::Object(object) => object.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CachePolicy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(text) if text == "auto" => Ok(CachePolicy::Auto),
            Value::String(text) if text == "none" => Ok(CachePolicy::None),
            Value::Object(_) => CachePolicyObject::deserialize(value)
                .map(CachePolicy::Object)
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "expected \"auto\", \"none\", or a cache-policy object, got {other}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// The WebSocket hook seams (PLUG-M02 — v2 `StreamOptions.webSocket`,
// model-request.ts:351-375)
// ---------------------------------------------------------------------------

/// The connection a channel executor is about to open (the
/// `experimental.ws.handshake` event's view).
#[derive(Debug, Clone)]
pub struct WsHandshake {
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// One seam callback: async, fallible, returning the (possibly rewritten)
/// value — v2's `Effect` trigger semantics (`url`/`headers`/`frame`).
pub type WsHandshakeHook = Arc<
    dyn Fn(WsHandshake) -> futures::future::BoxFuture<'static, Result<WsHandshake, String>>
        + Send
        + Sync,
>;
/// One outbound/inbound text frame rewrite (`experimental.ws.send` /
/// `experimental.ws.receive`).
pub type WsFrameHook = Arc<
    dyn Fn(String) -> futures::future::BoxFuture<'static, Result<String, String>> + Send + Sync,
>;

/// The three per-connection WebSocket seams (model-request.ts:353-372):
/// the handshake rewrite before the socket opens, the outbound frame
/// rewrite per sent message, the inbound frame rewrite per received
/// message. Built only when the session owns a stateful WebSocket AND
/// hooks are registered — the channel executors ride them when the
/// durable-session transport carries the request.
pub struct WebSocketSeams {
    pub handshake: WsHandshakeHook,
    pub send: WsFrameHook,
    pub receive: WsFrameHook,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pairs: &[(&str, Value)]) -> JsonRecord {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn merge_json_records_deep_merges_with_later_items_winning() {
        let nested: JsonRecord =
            [("x".to_string(), Value::from(1)), ("keep".to_string(), Value::from(true))]
                .into_iter()
                .collect();
        let a = record(&[("model", Value::from("gpt")), ("nested", Value::Object(nested))]);
        let b_nested: JsonRecord = [("x".to_string(), Value::from(2))].into_iter().collect();
        let b = record(&[
            ("model", Value::from("gpt-4")),
            ("nested", Value::Object(b_nested)),
            ("extra", Value::from(2)),
        ]);
        let merged = merge_json_records(&[Some(&a), Some(&b)]).unwrap();
        assert_eq!(merged["model"], Value::from("gpt-4"));
        assert_eq!(merged["extra"], Value::from(2));
        // nested records merge per key instead of being replaced wholesale
        assert_eq!(merged["nested"]["x"], Value::from(2));
        assert_eq!(merged["nested"]["keep"], Value::from(true));
        assert_eq!(merge_json_records(&[None, None]), None);
        assert_eq!(merge_json_records(&[]), None);
    }

    #[test]
    fn merge_generation_options_takes_the_last_defined_value_per_key() {
        let route = GenerationOptions {
            max_tokens: Some(1024.0),
            temperature: Some(0.5),
            ..Default::default()
        };
        let model =
            GenerationOptions { temperature: Some(0.9), top_p: Some(0.1), ..Default::default() };
        let request = GenerationOptions {
            temperature: None,
            stop: Some(vec!["END".into()]),
            ..Default::default()
        };
        let merged =
            merge_generation_options(&[Some(&route), Some(&model), Some(&request)]).unwrap();
        assert_eq!(merged.max_tokens, Some(1024.0)); // only route defines it
        assert_eq!(merged.temperature, Some(0.9)); // model wins over route (later)
        assert_eq!(merged.top_p, Some(0.1));
        assert_eq!(merged.stop.as_deref(), Some(&["END".into()][..]));
        assert!(merge_generation_options(&[None, Some(&GenerationOptions::default())]).is_none());
    }

    #[test]
    fn merge_http_options_merges_maps_and_takes_last_timeouts() {
        let base = HttpOptions {
            headers: Some(BTreeMap::from([("x-a".into(), "1".into())])),
            timeout: Some(HttpTimeout::Ms(1000.0)),
            ..Default::default()
        };
        let patch = HttpOptions {
            headers: Some(BTreeMap::from([("x-b".into(), "2".into())])),
            query: Some(BTreeMap::from([("alt".into(), "sse".into())])),
            timeout: Some(HttpTimeout::Disabled),
            ..Default::default()
        };
        let merged = merge_http_options(&[Some(&base), Some(&patch)]).unwrap();
        assert_eq!(merged.headers.as_ref().unwrap()["x-a"], "1");
        assert_eq!(merged.headers.as_ref().unwrap()["x-b"], "2");
        assert_eq!(merged.query.as_ref().unwrap()["alt"], "sse");
        assert_eq!(merged.timeout, Some(HttpTimeout::Disabled));
        assert_eq!(merge_http_options(&[None, None]), None);
    }

    #[test]
    fn http_timeout_serializes_as_number_or_false() {
        assert_eq!(serde_json::to_value(HttpTimeout::Ms(1.5)).unwrap(), serde_json::json!(1.5));
        assert_eq!(serde_json::to_value(HttpTimeout::Disabled).unwrap(), serde_json::json!(false));
        assert_eq!(
            serde_json::from_value::<HttpTimeout>(serde_json::json!(30_000)).unwrap(),
            HttpTimeout::Ms(30_000.0)
        );
        assert_eq!(
            serde_json::from_value::<HttpTimeout>(serde_json::json!(false)).unwrap(),
            HttpTimeout::Disabled
        );
        assert!(serde_json::from_value::<HttpTimeout>(serde_json::json!(true)).is_err());
        assert!(serde_json::from_value::<HttpTimeout>(serde_json::json!(0)).is_err());
    }

    #[test]
    fn cache_policy_wire_shapes_match_ts() {
        assert_eq!(serde_json::to_value(&CachePolicy::Auto).unwrap(), serde_json::json!("auto"));
        assert_eq!(serde_json::to_value(&CachePolicy::None).unwrap(), serde_json::json!("none"));
        let object = CachePolicyObject {
            tools: Some(true),
            system: Some(false),
            messages: Some(CachePolicyMessages::Tail { tail: 1 }),
            ttl_seconds: None,
        };
        assert_eq!(
            serde_json::to_value(CachePolicy::Object(object)).unwrap(),
            serde_json::json!({"tools": true, "system": false, "messages": {"tail": 1}})
        );
        let decoded: CachePolicy =
            serde_json::from_value(serde_json::json!({"messages": "latest-user-message"})).unwrap();
        assert_eq!(
            decoded,
            CachePolicy::Object(CachePolicyObject {
                tools: None,
                system: None,
                messages: Some(CachePolicyMessages::LatestUserMessage),
                ttl_seconds: None,
            })
        );
    }

    #[test]
    fn compatibility_carries_all_fourteen_fields() {
        let json = serde_json::json!({
            "sanitizer": "gemini",
            "reasoningField": "reasoning_content",
            "requireReasoning": true,
            "maxTokensField": "max_completion_tokens",
            "requireFinishReason": true,
            "requireAssistantAfterTool": true,
            "supportsStore": false,
            "supportsUsageInStreaming": true,
            "supportsStrictMode": true,
            "supportsPromptCacheKey": true,
            "zaiToolStream": false,
            "requireSignature": true,
            "supportsThinkingBlockBinding": true,
            "supportsEffortUpdates": true
        });
        let compat: LanguageModelCompatibility = serde_json::from_value(json).unwrap();
        assert_eq!(compat.sanitizer, Some(SanitizerCompatibility::Gemini));
        assert_eq!(compat.reasoning_field.as_deref(), Some("reasoning_content"));
        assert_eq!(compat.max_tokens_field, Some(MaxTokensFieldCompatibility::MaxCompletionTokens));
        assert_eq!(compat.supports_effort_updates, Some(true));
        assert_eq!(compat.supports_thinking_block_binding, Some(true));
        assert_eq!(compat.require_signature, Some(true));
    }
}
