//! The Anthropic Messages protocol (port of
//! `src/protocols/anthropic-messages.ts`, 1,673 LOC).
//!
//! Request lowering turns a common [`LLMRequest`] into the Messages API body
//! (top-level `system`, content blocks, `max_tokens` default 32k,
//! `thinking{type,budget_tokens}`, `context_management.edits` compaction,
//! `metadata`/`stop_sequences` passthrough, `tool_choice` mapping) with the
//! 4-breakpoint `cache_control` budget spent tools → system → messages.
//!
//! Response parsing is a pure state machine over
//! [`crate::route::framing::Framing::sse_events`] frames
//! (`message_start` → `content_block_*` lifecycle → `message_delta` →
//! `message_stop`): per-block reasoning signature preservation, tool-use
//! `input_json_delta` accumulation through
//! [`crate::protocols::utils::tool_stream`], `message_start`+`message_delta`
//! usage merge in the **sum** direction (Anthropic reports the non-cached
//! breakdown natively), raw usage kept under
//! `providerMetadata[<providerMetadataKey>]`, and `error` frames classified
//! through [`crate::provider_error`]. Like the TS protocol, there is no
//! `on_halt` — a stream that ends without `message_stop` fails with
//! `incomplete-stream` at the route layer.
//!
//! The route-level beta-header injection (TS transport wrapper,
//! `anthropic-messages.ts:1613`) lives in [`anthropic_messages_route`]'s
//! headers fn: `interleaved-thinking-2025-05-14` always, plus
//! `compact-2026-01-12` / `mid-conversation-output-config-2026-07-01` /
//! `thinking-binding-controls-2026-08-01` when the request needs them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::effort_updates::resolve_effort_updates;
use crate::protocols::shared::{
    decode_json, encode_json, event_error, fit_thinking_budget, flatten_tools, invalid_request,
    number, sum_non_cached_to_inclusive, sum_tokens, total_tokens,
};
use crate::protocols::utils::{lifecycle, tool_stream};
use crate::provider_error::{classify_provider_failure, ProviderFailure};
use crate::route::auth::{self, Definition as AuthDefinition, Secret};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{Route, RouteInput, RoutePatch};
use crate::schema::options::CacheHintKind;
use crate::schema::{
    AiError, AiErrorReason, CacheHint, ContentPart, FinishReason, FinishReasonDetails, LLMEvent,
    LLMRequest, LLMRequestPatch, LanguageModel, LanguageModelCompatibility, Message, MessageRole,
    ProviderId, ProviderMetadata, SystemPart, ToolChoice, ToolDefinition, ToolResultValue, Usage,
};

/// Stable protocol id (TS `ADAPTER`).
pub const ADAPTER: &str = "anthropic-messages";
/// Default Anthropic Messages base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";
/// Messages API path.
pub const PATH: &str = "/messages";
/// `max_tokens` fallback when the request does not set one.
pub const DEFAULT_MAX_TOKENS: f64 = 32_000.0;
/// Anthropic thinking budgets cannot go below this.
const MIN_THINKING_BUDGET: f64 = 1_024.0;
/// Effort applied to an effort-update marker without an explicit effort.
const DEFAULT_EFFORT: &str = "high";
/// Anthropic accepts at most 4 explicit `cache_control` breakpoints per
/// request across `tools`, `system` and `messages`.
const ANTHROPIC_BREAKPOINT_CAP: u32 = 4;
/// Beta flag for thinking block-binding controls.
pub const THINKING_BINDING_BETA: &str = "thinking-binding-controls-2026-08-01";

/// The `event:` names the Messages SSE stream uses (unnamed events default
/// to `message`, which is why it belongs to the set).
pub const SSE_EVENT_NAMES: [&str; 9] = [
    "message",
    "message_start",
    "message_delta",
    "message_stop",
    "content_block_start",
    "content_block_delta",
    "content_block_stop",
    "ping",
    "error",
];

/// SSE framing restricted to the Messages event names (TS `framing`).
#[must_use]
pub fn framing() -> Framing {
    Framing::sse_events(SSE_EVENT_NAMES)
}

// =============================================================================
// Claude model-version heuristics (port of utils/claude-model.ts)
// =============================================================================

/// Parsed `claude-<family>-<major>[.<minor>]` model version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeVersion {
    pub family: String,
    pub major: u32,
    pub minor: u32,
}

/// Extract the Claude family/version from a model id, accepting gateway
/// namespaces and Vertex suffixes without treating a snapshot date as a
/// minor version (port of `claudeVersion`).
#[must_use]
pub fn claude_version(id: &str) -> Option<ClaudeVersion> {
    // (?:^|[./])claude-(?<family>[a-z]+)-(?<major>\d+)(?:[.-](?<minor>\d{1,2}))?(?:$|[-:@])
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?:^|[./])claude-(?<family>[a-z]+)-(?<major>\d+)(?:[.-](?<minor>\d{1,2}))?(?:$|[-:@])",
        )
        .expect("static regex compiles")
    });
    let lowered = id.to_ascii_lowercase();
    let groups = re.captures(&lowered)?;
    let minor = groups.name("minor").and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
    Some(ClaudeVersion {
        family: groups.name("family")?.as_str().to_string(),
        major: groups.name("major")?.as_str().parse().ok()?,
        minor,
    })
}

fn compatibility_of(model: &LanguageModel) -> Option<&LanguageModelCompatibility> {
    model.compatibility.as_ref()
}

/// Opus 4.8 and every Claude 5 model accept mid-conversation system messages;
/// later versions inherit support (port of `supportsNativeSystemUpdates`).
#[must_use]
pub fn supports_native_system_updates(model: &LanguageModel) -> bool {
    let Some(version) = claude_version(&model.id.0) else { return false };
    if version.family == "opus" && version.major == 4 {
        return version.minor >= 8;
    }
    version.major >= 5
}

/// Per-turn effort started with Claude Opus 5 and every Claude 5.1 model
/// (port of `supportsEffortUpdates`).
#[must_use]
pub fn supports_effort_updates(model: &LanguageModel) -> bool {
    if let Some(value) = compatibility_of(model).and_then(|c| c.supports_effort_updates) {
        return value;
    }
    let Some(version) = claude_version(&model.id.0) else { return false };
    if version.family == "opus" && version.major >= 5 {
        return true;
    }
    version.major > 5 || (version.major == 5 && version.minor >= 1)
}

/// Thinking block-binding controls on Claude 5.1+ (port of
/// `supportsThinkingBlockBinding`).
#[must_use]
pub fn supports_thinking_block_binding(model: &LanguageModel) -> bool {
    if let Some(value) = compatibility_of(model).and_then(|c| c.supports_thinking_block_binding) {
        return value;
    }
    let Some(version) = claude_version(&model.id.0) else { return false };
    version.major > 5 || (version.major == 5 && version.minor >= 1)
}

// =============================================================================
// Provider options (port of the `Options` schema + `decodeOptions`)
// =============================================================================

/// `context_management.edits[]` compaction request.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextManagementEdit {
    pub trigger: Option<Value>,
    pub pause_after_compaction: Option<bool>,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextManagement {
    pub edits: Vec<ContextManagementEdit>,
}

/// `thinking` provider option; callers may spell the budget `budgetTokens`
/// or `budget_tokens` (port of `Thinking`).
#[derive(Debug, Clone, PartialEq)]
pub enum Thinking {
    Enabled {
        budget_tokens: f64,
        display: Option<String>,
        block_binding: Option<ThinkingBlockBinding>,
    },
    Adaptive {
        display: Option<String>,
        block_binding: Option<ThinkingBlockBinding>,
    },
    Disabled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThinkingBlockBinding {
    pub prefix_mismatch_behavior: Option<String>,
}

/// `output_config.format` JSON-schema output mode.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonOutputFormat {
    pub schema: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutputConfig {
    pub effort: Option<String>,
    pub format: Option<JsonOutputFormat>,
}

/// Caller-facing provider options after schema validation; unknown keys are
/// accepted and ignored (port of `Options`).
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub context_management: Option<ContextManagement>,
    pub thinking: Option<Thinking>,
    pub effort: Option<String>,
    pub service_tier: Option<String>,
    pub metadata: Option<Value>,
    pub container: Option<Value>,
    pub inference_geo: Option<String>,
    pub cache_control: Option<Value>,
    pub output_config: Option<OutputConfig>,
}

fn option_error(message: impl Into<String>) -> AiError {
    invalid_request(message)
}

/// `Schema.optional(Schema.String)`: absent ok, any present value must be a
/// string.
fn want_string(value: Option<&Value>) -> Result<Option<String>, AiError> {
    match value {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(other) => Err(option_error(format!("Expected a string, received {other}"))),
    }
}

fn want_bool(value: Option<&Value>) -> Result<Option<bool>, AiError> {
    match value {
        None => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(other) => Err(option_error(format!("Expected a boolean, received {other}"))),
    }
}

fn want_number(value: Option<&Value>) -> Result<Option<f64>, AiError> {
    match value {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .map(Some)
            .ok_or_else(|| option_error(format!("Expected a number, received {v}"))),
    }
}

/// `{type: "ephemeral", ttl?: "5m"|"1h"}` (port of `AnthropicCacheControl`).
fn decode_cache_control(value: &Value) -> Result<Value, AiError> {
    let Some(object) = value.as_object() else {
        return Err(option_error(format!("Expected an object, received {value}")));
    };
    if object.get("type").and_then(Value::as_str) != Some("ephemeral") {
        return Err(option_error("cache_control requires { type: \"ephemeral\" }"));
    }
    match object.get("ttl") {
        None => Ok(json!({"type": "ephemeral"})),
        Some(Value::String(ttl)) if ttl == "5m" || ttl == "1h" => {
            Ok(json!({"type": "ephemeral", "ttl": ttl}))
        }
        Some(other) => Err(option_error(format!("Expected \"5m\" or \"1h\", received {other}"))),
    }
}

fn decode_thinking_fields(
    object: &Map<String, Value>,
) -> Result<(Option<String>, Option<ThinkingBlockBinding>), AiError> {
    let display = want_string(object.get("display"))?;
    let block_binding = match object.get("block_binding") {
        None => None,
        Some(binding) => {
            let Some(binding_object) = binding.as_object() else {
                return Err(option_error(format!("Expected an object, received {binding}")));
            };
            Some(ThinkingBlockBinding {
                prefix_mismatch_behavior: want_string(
                    binding_object.get("prefix_mismatch_behavior"),
                )?,
            })
        }
    };
    Ok((display, block_binding))
}

fn decode_thinking(value: &Value) -> Result<Thinking, AiError> {
    let Some(object) = value.as_object() else {
        return Err(option_error(format!("Expected an object, received {value}")));
    };
    match object.get("type").and_then(Value::as_str) {
        Some("enabled") => {
            // The two budget spellings are disjoint per variant; one must
            // carry a number.
            let budget_tokens = match (object.get("budgetTokens"), object.get("budget_tokens")) {
                (Some(Value::Number(number)), _) => Some(number.as_f64().unwrap_or_default()),
                (None, Some(Value::Number(number))) => Some(number.as_f64().unwrap_or_default()),
                (Some(other), _) | (None, Some(other)) => {
                    return Err(option_error(format!(
                        "Expected a number for budgetTokens, received {other}"
                    )))
                }
                (None, None) => {
                    return Err(option_error(
                        "thinking { type: \"enabled\" } requires a budgetTokens (or budget_tokens) number",
                    ))
                }
            };
            let (display, block_binding) = decode_thinking_fields(object)?;
            Ok(Thinking::Enabled {
                budget_tokens: budget_tokens.unwrap_or_default(),
                display,
                block_binding,
            })
        }
        Some("adaptive") => {
            let (display, block_binding) = decode_thinking_fields(object)?;
            Ok(Thinking::Adaptive { display, block_binding })
        }
        Some("disabled") => Ok(Thinking::Disabled),
        Some(other) => Err(option_error(format!(
            "Expected \"enabled\", \"adaptive\" or \"disabled\", received \"{other}\""
        ))),
        None => Err(option_error("thinking requires a type")),
    }
}

fn decode_output_config(value: &Value) -> Result<OutputConfig, AiError> {
    let Some(object) = value.as_object() else {
        return Err(option_error(format!("Expected an object, received {value}")));
    };
    let effort = match object.get("effort") {
        None | Some(Value::Null) => None,
        Some(Value::String(effort)) => Some(effort.clone()),
        Some(other) => {
            return Err(option_error(format!("Expected a string, received {other}")));
        }
    };
    let format = match object.get("format") {
        None | Some(Value::Null) => None,
        Some(format) => {
            let Some(format_object) = format.as_object() else {
                return Err(option_error(format!("Expected an object, received {format}")));
            };
            if format_object.get("type").and_then(Value::as_str) != Some("json_schema") {
                return Err(option_error(
                    "output_config.format requires { type: \"json_schema\" }",
                ));
            }
            match format_object.get("schema") {
                Some(schema) if schema.is_object() => {
                    Some(JsonOutputFormat { schema: schema.clone() })
                }
                Some(other) => {
                    return Err(option_error(format!("Expected an object, received {other}")));
                }
                None => return Err(option_error("output_config.format requires a schema")),
            }
        }
    };
    Ok(OutputConfig { effort, format })
}

fn decode_context_management(value: &Value) -> Result<ContextManagement, AiError> {
    let Some(object) = value.as_object() else {
        return Err(option_error(format!("Expected an object, received {value}")));
    };
    let edits = match object.get("edits") {
        None => Vec::new(),
        Some(Value::Array(items)) => {
            let mut edits = Vec::new();
            for item in items {
                let Some(item_object) = item.as_object() else {
                    return Err(option_error(format!("Expected an object, received {item}")));
                };
                if item_object.get("type").and_then(Value::as_str) != Some("compact_20260112") {
                    return Err(option_error(
                        "contextManagement.edits requires { type: \"compact_20260112\" }",
                    ));
                }
                let trigger = match item_object.get("trigger") {
                    None | Some(Value::Null) => None,
                    Some(trigger) => {
                        let Some(trigger_object) = trigger.as_object() else {
                            return Err(option_error(format!(
                                "Expected an object, received {trigger}"
                            )));
                        };
                        if trigger_object.get("type").and_then(Value::as_str)
                            != Some("input_tokens")
                        {
                            return Err(option_error(
                                "contextManagement.edits.trigger requires { type: \"input_tokens\" }",
                            ));
                        }
                        let threshold =
                            want_number(trigger_object.get("value"))?.ok_or_else(|| {
                                option_error("contextManagement.edits.trigger requires a value")
                            })?;
                        if threshold < 50_000.0 {
                            return Err(option_error(
                                "contextManagement.edits.trigger.value must be at least 50000",
                            ));
                        }
                        Some(json!({"type": "input_tokens", "value": threshold}))
                    }
                };
                edits.push(ContextManagementEdit {
                    trigger,
                    pause_after_compaction: want_bool(item_object.get("pauseAfterCompaction"))?,
                    instructions: want_string(item_object.get("instructions"))?,
                });
            }
            edits
        }
        Some(other) => return Err(option_error(format!("Expected an array, received {other}"))),
    };
    Ok(ContextManagement { edits })
}

/// Validate the caller-facing provider options (port of `decodeOptions`).
pub fn decode_options(input: Option<&crate::schema::ProviderOptions>) -> Result<Options, AiError> {
    let empty = Map::new();
    let object = input.unwrap_or(&empty);
    let context_management = match object.get("contextManagement") {
        None => None,
        Some(value) => Some(decode_context_management(value)?),
    };
    let thinking = match object.get("thinking") {
        None => None,
        Some(value) => Some(decode_thinking(value)?),
    };
    let effort = want_string(object.get("effort"))?;
    let service_tier =
        want_string(object.get("service_tier"))?.or(want_string(object.get("serviceTier"))?);
    let metadata = match object.get("metadata") {
        None => None,
        Some(value) => {
            let Some(metadata_object) = value.as_object() else {
                return Err(option_error(format!("Expected an object, received {value}")));
            };
            let user_id = want_string(metadata_object.get("user_id"))?;
            let mut out = Map::new();
            if let Some(user_id) = user_id {
                out.insert("user_id".into(), json!(user_id));
            }
            Some(Value::Object(out))
        }
    };
    let container = match object.get("container") {
        None => None,
        Some(value @ Value::String(_)) => Some(value.clone()),
        Some(value) => {
            let Some(container_object) = value.as_object() else {
                return Err(option_error(format!("Expected an object, received {value}")));
            };
            if let Some(skills) = container_object.get("skills") {
                let Value::Array(items) = skills else {
                    return Err(option_error(format!("Expected an array, received {skills}")));
                };
                if !items.iter().all(Value::is_object) {
                    return Err(option_error("container.skills items must be objects"));
                }
            }
            if let Some(id) = container_object.get("id") {
                want_string(Some(id))?;
            }
            Some(value.clone())
        }
    };
    let inference_geo =
        want_string(object.get("inference_geo"))?.or(want_string(object.get("inferenceGeo"))?);
    let cache_control = match object.get("cache_control") {
        Some(value) => Some(decode_cache_control(value)?),
        None => match object.get("cacheControl") {
            Some(value) => Some(decode_cache_control(value)?),
            None => None,
        },
    };
    let output_config = match object.get("output_config") {
        None => match object.get("outputConfig") {
            None => None,
            Some(value) => Some(decode_output_config(value)?),
        },
        Some(value) => Some(decode_output_config(value)?),
    };
    Ok(Options {
        context_management,
        thinking,
        effort,
        service_tier,
        metadata,
        container,
        inference_geo,
        cache_control,
        output_config,
    })
}

// =============================================================================
// Request lowering
// =============================================================================

/// Remaining/dropped `cache_control` breakpoint bookkeeping (port of
/// `Cache.Breakpoints`).
#[derive(Debug, Clone)]
struct Breakpoints {
    remaining: u32,
    dropped: u32,
}

impl Breakpoints {
    fn new(cap: u32) -> Self {
        Self { remaining: cap, dropped: 0 }
    }
}

/// Requests of at least one hour use the explicit `"1h"` bucket (port of
/// `ttlBucket`).
fn ttl_bucket(ttl_seconds: Option<f64>) -> Option<&'static str> {
    ttl_seconds.is_some_and(|seconds| seconds >= 3600.0).then_some("1h")
}

fn cache_control(breakpoints: &mut Breakpoints, cache: Option<&CacheHint>) -> Option<Value> {
    let hint = cache?;
    if !matches!(hint.kind, CacheHintKind::Ephemeral | CacheHintKind::Persistent) {
        return None;
    }
    if breakpoints.remaining == 0 {
        breakpoints.dropped += 1;
        return None;
    }
    breakpoints.remaining -= 1;
    match ttl_bucket(hint.ttl_seconds) {
        Some("1h") => Some(json!({"type": "ephemeral", "ttl": "1h"})),
        _ => Some(json!({"type": "ephemeral"})),
    }
}

fn provider_metadata(key: &str, metadata: Value) -> ProviderMetadata {
    let inner = match metadata {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    BTreeMap::from([(key.to_string(), inner)])
}

fn signature_from_metadata(metadata: Option<&ProviderMetadata>, key: &str) -> Option<String> {
    metadata?.get(key)?.get("signature")?.as_str().map(str::to_string)
}

fn redacted_data_from_metadata(metadata: Option<&ProviderMetadata>, key: &str) -> Option<String> {
    metadata?.get(key)?.get("redactedData")?.as_str().map(str::to_string)
}

fn lower_tool(breakpoints: &mut Breakpoints, tool: &ToolDefinition) -> Value {
    let mut object = Map::new();
    object.insert("name".into(), json!(tool.name));
    object.insert("description".into(), json!(tool.description));
    object.insert("input_schema".into(), tool.input_schema.clone());
    if let Some(control) = cache_control(breakpoints, tool.cache.as_ref()) {
        object.insert("cache_control".into(), control);
    }
    Value::Object(object)
}

fn disable_parallel_tool_use(tool_choice: &ToolChoice) -> Option<bool> {
    match tool_choice {
        ToolChoice::Auto { disable_parallel_tool_use }
        | ToolChoice::Required { disable_parallel_tool_use }
        | ToolChoice::Named { disable_parallel_tool_use, .. } => *disable_parallel_tool_use,
        ToolChoice::None {} => None,
    }
}

fn lower_tool_choice(tool_choice: &ToolChoice) -> Result<Value, AiError> {
    let flag = |object: &mut Map<String, Value>| {
        if let Some(value) = disable_parallel_tool_use(tool_choice) {
            object.insert("disable_parallel_tool_use".into(), json!(value));
        }
    };
    match tool_choice {
        ToolChoice::Auto { .. } => {
            let mut object = Map::new();
            object.insert("type".into(), json!("auto"));
            flag(&mut object);
            Ok(Value::Object(object))
        }
        ToolChoice::None {} => Ok(json!({"type": "none"})),
        ToolChoice::Required { .. } => {
            let mut object = Map::new();
            object.insert("type".into(), json!("any"));
            flag(&mut object);
            Ok(Value::Object(object))
        }
        ToolChoice::Named { name, .. } => {
            if name.is_empty() {
                return Err(invalid_request("Anthropic Messages tool choice requires a tool name"));
            }
            let mut object = Map::new();
            object.insert("type".into(), json!("tool"));
            object.insert("name".into(), json!(name));
            flag(&mut object);
            Ok(Value::Object(object))
        }
    }
}

/// Anthropic tool-use ids are restricted to `[a-zA-Z0-9_-]`; scrub without
/// truncating (port of `scrubToolCallID`).
fn scrub_tool_call_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

fn tool_call_fields(part: &ContentPart) -> Option<(&String, &String, &Value)> {
    match part {
        ContentPart::ToolCall { id, name, input, .. } => Some((id, name, input)),
        _ => None,
    }
}

fn lower_tool_call(part: &ContentPart) -> Value {
    let (id, name, input) = tool_call_fields(part).expect("tool-call part");
    json!({
        "type": "tool_use",
        "id": scrub_tool_call_id(id),
        "name": name,
        "input": input,
    })
}

fn lower_server_tool_call(part: &ContentPart) -> Value {
    let (id, name, input) = tool_call_fields(part).expect("tool-call part");
    json!({
        "type": "server_tool_use",
        "id": scrub_tool_call_id(id),
        "name": name,
        "input": input,
    })
}

/// Server tool result blocks are typed by name; Anthropic ships three today.
fn server_tool_result_type(name: &str) -> Option<&'static str> {
    match name {
        "web_search" => Some("web_search_tool_result"),
        "code_execution" => Some("code_execution_tool_result"),
        "web_fetch" => Some("web_fetch_tool_result"),
        _ => None,
    }
}

fn tool_result_fields(part: &ContentPart) -> Option<(&String, &String, &ToolResultValue)> {
    match part {
        ContentPart::ToolResult { id, name, result, .. } => Some((id, name, result)),
        _ => None,
    }
}

fn lower_server_tool_result(
    part: &ContentPart,
    provider_metadata_key: &str,
) -> Result<Value, AiError> {
    let Some((id, name, _)) = tool_result_fields(part) else {
        return Err(invalid_request("expected a tool-result part"));
    };
    let Some(wire_type) = server_tool_result_type(name) else {
        return Err(invalid_request(format!(
            "Anthropic Messages does not know how to round-trip server tool result for {name}"
        )));
    };
    // Prefer the provider-owned replay payload; fall back to the result value.
    let payload = match part {
        ContentPart::ToolResult { provider_metadata, result, .. } => provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get(provider_metadata_key))
            .and_then(|metadata| metadata.get("result").cloned())
            .or_else(|| match result {
                ToolResultValue::Json { value }
                | ToolResultValue::Text { value }
                | ToolResultValue::Error { value } => Some(value.clone()),
                ToolResultValue::Content { .. } => None,
            }),
        _ => None,
    };
    Ok(json!({
        "type": wire_type,
        "tool_use_id": scrub_tool_call_id(id),
        "content": payload,
    }))
}

/// Read `metadata[<keys...>]` after the `anthropic`-prefixed lookup order.
fn metadata_string(metadata: Option<&BTreeMap<String, Value>>, keys: &[&str]) -> Option<String> {
    let metadata = metadata?;
    if let Some(anthropic) = metadata.get("anthropic").and_then(Value::as_object) {
        for key in keys {
            if let Some(value) = anthropic.get(*key).and_then(Value::as_str) {
                return Some(value.to_string());
            }
        }
    }
    for key in keys {
        if let Some(value) = metadata.get(*key).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    None
}

fn file_id_from_metadata(metadata: Option<&BTreeMap<String, Value>>) -> Option<String> {
    metadata_string(metadata, &["file_id", "fileId"])
}

fn document_title_from_part(
    metadata: Option<&BTreeMap<String, Value>>,
    filename: Option<&String>,
) -> Option<String> {
    metadata_string(metadata, &["title"])
        .or_else(|| filename.filter(|name| !name.is_empty()).cloned())
}

fn document_context_from_metadata(metadata: Option<&BTreeMap<String, Value>>) -> Option<String> {
    metadata_string(metadata, &["context"])
}

fn citations_from_metadata(metadata: Option<&BTreeMap<String, Value>>) -> Option<Value> {
    let metadata = metadata?;
    let raw = metadata
        .get("anthropic")
        .and_then(Value::as_object)
        .and_then(|object| object.get("citations"))
        .or_else(|| metadata.get("citations"));
    let enabled = raw
        .and_then(Value::as_object)
        .and_then(|object| object.get("enabled"))
        .and_then(Value::as_bool)?;
    Some(json!({"enabled": enabled}))
}

fn oversized_image_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_object)
        .and_then(|object| object.get("oversized_image"))
        .and_then(Value::as_str)
        .and_then(|value| (value == "downsize" || value == "error").then_some(value.to_string()))
}

fn transformations_from_metadata(metadata: Option<&BTreeMap<String, Value>>) -> Option<Value> {
    let metadata = metadata?;
    let anthropic_object = metadata.get("anthropic").and_then(Value::as_object);
    let raw = anthropic_object
        .and_then(|object| object.get("transformations"))
        .or_else(|| metadata.get("transformations"));
    if let Some(value) = oversized_image_value(raw) {
        return Some(json!({"oversized_image": value}));
    }
    let direct = anthropic_object
        .and_then(|object| object.get("oversized_image"))
        .and_then(Value::as_str)
        .and_then(|value| (value == "downsize" || value == "error").then_some(value.to_string()));
    direct.map(|value| json!({"oversized_image": value}))
}

fn is_http_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Base64 decode (standard alphabet, lenient about whitespace/padding),
/// mirroring `Buffer.from(input, "base64")` for well-formed payloads.
fn decode_base64(input: &str) -> Vec<u8> {
    const REVERSE: [i8; 256] = {
        let mut table = [-1i8; 256];
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut index = 0;
        while index < 64 {
            table[alphabet[index] as usize] = index as i8;
            index += 1;
        }
        table
    };
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        let value = REVERSE[byte as usize];
        let Some(value) = u8::try_from(value).ok() else {
            continue;
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    out
}

fn image_block(source: Value, cache: Option<Value>, transformations: Option<Value>) -> Value {
    let mut object = Map::new();
    object.insert("type".into(), json!("image"));
    object.insert("source".into(), source);
    if let Some(cache) = cache {
        object.insert("cache_control".into(), cache);
    }
    if let Some(transformations) = transformations {
        object.insert("transformations".into(), transformations);
    }
    Value::Object(object)
}

#[allow(clippy::too_many_arguments)]
fn document_block(
    source: Value,
    cache: Option<Value>,
    title: Option<String>,
    context: Option<String>,
    citations: Option<Value>,
) -> Value {
    let mut object = Map::new();
    object.insert("type".into(), json!("document"));
    object.insert("source".into(), source);
    if let Some(cache) = cache {
        object.insert("cache_control".into(), cache);
    }
    if let Some(title) = title {
        object.insert("title".into(), json!(title));
    }
    if let Some(context) = context {
        object.insert("context".into(), json!(context));
    }
    if let Some(citations) = citations {
        object.insert("citations".into(), citations);
    }
    Value::Object(object)
}

/// Lower one media part into an Anthropic image/document block (port of
/// `lowerMedia`).
fn lower_media(
    media: &crate::schema::media::Asset,
    filename: Option<&String>,
    cache: Option<&CacheHint>,
    metadata: Option<&BTreeMap<String, Value>>,
    breakpoints: Option<&mut Breakpoints>,
) -> Result<Value, AiError> {
    let mime = media.media_type.to_ascii_lowercase();
    let cache_control_value = match breakpoints {
        Some(breakpoints) => cache_control(breakpoints, cache),
        None => None,
    };
    if let Some(file_id) = file_id_from_metadata(metadata) {
        let file_source = json!({"type": "file", "file_id": file_id});
        if mime.starts_with("image/") {
            return Ok(image_block(
                file_source,
                cache_control_value,
                transformations_from_metadata(metadata),
            ));
        }
        return Ok(document_block(
            file_source,
            cache_control_value,
            document_title_from_part(metadata, filename),
            document_context_from_metadata(metadata),
            citations_from_metadata(metadata),
        ));
    }

    if let Some(url) = media.url().map(str::trim).filter(|url| is_http_url(url)) {
        if mime.starts_with("image/") {
            return Ok(image_block(
                json!({"type": "url", "url": url}),
                cache_control_value,
                transformations_from_metadata(metadata),
            ));
        }
        if mime == "application/pdf" {
            return Ok(document_block(
                json!({"type": "url", "url": url}),
                cache_control_value,
                document_title_from_part(metadata, filename),
                document_context_from_metadata(metadata),
                citations_from_metadata(metadata),
            ));
        }
    }

    let Some(inline) = media.inline() else {
        return Err(invalid_request(format!(
            "Anthropic Messages requires inline media (bytes or base64); {} sources must be materialized first",
            media_source_kind(media)
        )));
    };

    if mime == "text/plain" {
        let text_data = String::from_utf8_lossy(&decode_base64(&inline.base64)).into_owned();
        return Ok(document_block(
            json!({"type": "text", "media_type": "text/plain", "data": text_data}),
            cache_control_value,
            document_title_from_part(metadata, filename),
            document_context_from_metadata(metadata),
            citations_from_metadata(metadata),
        ));
    }
    if inline.mime == "application/pdf" {
        return Ok(document_block(
            json!({"type": "base64", "media_type": "application/pdf", "data": inline.base64}),
            cache_control_value,
            document_title_from_part(metadata, filename),
            document_context_from_metadata(metadata),
            citations_from_metadata(metadata),
        ));
    }
    if !inline.mime.starts_with("image/") {
        return Err(invalid_request(format!(
            "Anthropic Messages does not support media type {}",
            media.media_type
        )));
    }
    Ok(image_block(
        json!({"type": "base64", "media_type": inline.mime, "data": inline.base64}),
        cache_control_value,
        transformations_from_metadata(metadata),
    ))
}

fn media_source_kind(media: &crate::schema::media::Asset) -> &'static str {
    match media.source {
        crate::schema::media::Source::Bytes { .. } => "bytes",
        crate::schema::media::Source::Base64 { .. } => "base64",
        crate::schema::media::Source::Url { .. } => "url",
        crate::schema::media::Source::Ref { .. } => "ref",
    }
}

fn lower_media_part(
    part: &ContentPart,
    breakpoints: Option<&mut Breakpoints>,
) -> Result<Value, AiError> {
    match part {
        ContentPart::Media { media, filename, cache, metadata, .. } => {
            lower_media(media, filename.as_ref(), cache.as_ref(), metadata.as_ref(), breakpoints)
        }
        _ => Err(invalid_request("expected a media part")),
    }
}

/// JS `String(value)` for tool-result text (numbers/booleans/null/objects).
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".into(),
        Value::Number(number) => js_number_string(number),
        Value::Object(_) => "[object Object]".into(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
    }
}

fn js_number_string(number: &serde_json::Number) -> String {
    match number.as_f64() {
        Some(value) if value.fract() == 0.0 && value.abs() < 1e21 => {
            format!("{}", value as i64)
        }
        Some(value) => format!("{value}"),
        None => number.to_string(),
    }
}

/// Tool results as string content (port of `toolResultText`).
fn tool_result_text(result: &ToolResultValue) -> String {
    match result {
        ToolResultValue::Text { value } => js_string(value),
        ToolResultValue::Error { value } => {
            let structured = value.is_array() || value.is_object();
            if structured {
                encode_json(value)
            } else {
                js_string(value)
            }
        }
        ToolResultValue::Json { value } => encode_json(value),
        ToolResultValue::Content { .. } => String::new(),
    }
}

/// Lift a tool-result file into a media part (port of `toolFileMedia`): data
/// URLs parse, `http(s)` URLs stay URLs, anything else is raw base64.
fn tool_file_media(item: &Value) -> Result<Value, AiError> {
    let uri = item.get("uri").and_then(Value::as_str).unwrap_or_default();
    let mime = item.get("mime").and_then(Value::as_str).unwrap_or_default().to_string();
    let name = item.get("name").and_then(Value::as_str).map(str::to_string);
    let media = if uri.starts_with("data:") {
        let Some(mut asset) = crate::schema::media::parse_data_url(uri) else {
            return Err(invalid_request("tool file carried an unparseable data URL"));
        };
        asset.media_type = mime.clone();
        if let crate::schema::media::Source::Base64 { media_type, .. } = &mut asset.source {
            *media_type = mime;
        }
        asset
    } else if is_http_url(uri) {
        crate::schema::media::url(uri, Some(mime.clone()), None)
    } else {
        crate::schema::media::base64(uri, mime.clone())
    };
    lower_media(&media, name.as_ref(), None, None, None)
}

/// Tool-result content: string for text/json/error, structured blocks for
/// `content` arrays (port of `lowerToolResultContent`).
fn lower_tool_result_content(part: &ContentPart) -> Result<Value, AiError> {
    let Some((_, _, result)) = tool_result_fields(part) else {
        return Err(invalid_request("expected a tool-result part"));
    };
    match result {
        ToolResultValue::Content { value } => {
            let mut blocks = Vec::new();
            for item in &value.0 {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => blocks.push(json!({
                        "type": "text",
                        "text": item.get("text").and_then(Value::as_str).unwrap_or_default(),
                    })),
                    _ => blocks.push(tool_file_media(item)?),
                }
            }
            Ok(Value::Array(blocks))
        }
        other => Ok(Value::String(tool_result_text(other))),
    }
}

/// `Kimi`/`Moonshot` and `Xiaomi` `MiMo` tolerate empty thinking signatures.
fn require_thinking_signature(request: &LLMRequest) -> bool {
    if let Some(value) =
        compatibility_of(&request.model).and_then(|compatibility| compatibility.require_signature)
    {
        return value;
    }
    let provider = request.model.provider.0.to_ascii_lowercase();
    let model = request.model.id.0.to_ascii_lowercase();
    let base_url =
        request.model.route.endpoint.base_url.as_deref().unwrap_or_default().to_ascii_lowercase();
    if provider == "kimi-for-coding"
        || provider == "moonshotai"
        || provider == "moonshotai-cn"
        || model.starts_with("kimi-")
        || base_url.contains("api.kimi.com/coding")
        || base_url.contains("api.moonshot.ai/anthropic")
        || base_url.contains("api.moonshot.cn/anthropic")
    {
        return false;
    }
    if provider.contains("xiaomi") || model.contains("mimo") || base_url.contains("xiaomimimo.com")
    {
        return false;
    }
    true
}

/// Native system messages must follow a user turn (tool results count) or a
/// paused server-tool turn.
fn accepts_native_system_after(message: Option<&Value>) -> bool {
    let Some(message) = message else { return false };
    match message.get("role").and_then(Value::as_str) {
        Some("user") => true,
        Some("assistant") => {
            message
                .get("content")
                .and_then(Value::as_array)
                .and_then(|blocks| blocks.last())
                .and_then(|block| block.get("type"))
                .and_then(Value::as_str)
                == Some("server_tool_use")
        }
        _ => false,
    }
}

/// A system update between a local tool call and its result would split the
/// pair; reject it (port of `splitsLocalToolResults`).
fn splits_local_tool_results(messages: &[Message], index: usize) -> bool {
    let mut pending: BTreeSet<String> = BTreeSet::new();
    for message in messages.iter().take(index) {
        for part in &message.content {
            match (message.role, part) {
                (MessageRole::Assistant, ContentPart::ToolCall { id, provider_executed, .. })
                    if *provider_executed != Some(true) =>
                {
                    pending.insert(id.clone());
                }
                (MessageRole::Tool, ContentPart::ToolResult { id, .. }) => {
                    pending.remove(id);
                }
                _ => {}
            }
        }
    }
    !pending.is_empty()
}

/// Chronological system updates accept text only.
fn system_update_text(message: &Message) -> Result<Vec<(String, Option<&CacheHint>)>, AiError> {
    let mut content = Vec::new();
    for part in &message.content {
        let ContentPart::Text { text, cache, .. } = part else {
            return Err(invalid_request(
                "Anthropic Messages system messages only support text content for now",
            ));
        };
        content.push((text.clone(), cache.as_ref()));
    }
    Ok(content)
}

fn lower_native_system_update(
    message: &Message,
    breakpoints: &mut Breakpoints,
) -> Result<Value, AiError> {
    let content = system_update_text(message)?;
    let blocks: Vec<Value> = content
        .into_iter()
        .map(|(text, cache)| {
            let mut object = Map::new();
            object.insert("type".into(), json!("text"));
            object.insert("text".into(), json!(text));
            if let Some(control) = cache_control(breakpoints, cache) {
                object.insert("cache_control".into(), control);
            }
            Value::Object(object)
        })
        .collect();
    Ok(json!({"role": "system", "content": blocks}))
}

fn lower_wrapped_system_update(
    message: &Message,
    breakpoints: &mut Breakpoints,
) -> Result<Value, AiError> {
    let content = system_update_text(message)?;
    let text =
        crate::protocols::shared::wrap_system_update(content.iter().map(|(text, _)| text.as_str()));
    let mut object = Map::new();
    object.insert("type".into(), json!("text"));
    object.insert("text".into(), json!(text));
    if let Some(control) = cache_control(breakpoints, content.last().and_then(|(_, cache)| *cache))
    {
        object.insert("cache_control".into(), control);
    }
    Ok(Value::Object(object))
}

fn append_to_user_turn(messages: &mut Vec<Value>, block: Value) {
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(Value::as_str) == Some("user") {
            if let Some(content) = last.get_mut("content").and_then(Value::as_array_mut) {
                content.push(block);
                return;
            }
        }
    }
    messages.push(json!({"role": "user", "content": [block]}));
}

/// Flatten namespaces in tools and tool-call/tool-result part names (port of
/// `flattenToolRequest`).
fn flatten_tool_request(request: &LLMRequest) -> (Vec<ToolDefinition>, LLMRequest) {
    let tools = flatten_tools(&request.tools);
    let mut changed = false;
    let messages: Vec<Message> = request
        .messages
        .iter()
        .map(|message| {
            let content: Vec<ContentPart> = message
                .content
                .iter()
                .map(|part| match part {
                    ContentPart::ToolCall { namespace: Some(_), .. }
                    | ContentPart::ToolResult { namespace: Some(_), .. } => {
                        changed = true;
                        let mut next = part.clone();
                        match &mut next {
                            ContentPart::ToolCall { name, namespace, .. } => {
                                let prefix = namespace.clone().expect("namespace checked above");
                                *name = format!("{prefix}_{name}");
                                *namespace = None;
                            }
                            ContentPart::ToolResult { name, namespace, .. } => {
                                let prefix = namespace.clone().expect("namespace checked above");
                                *name = format!("{prefix}_{name}");
                                *namespace = None;
                            }
                            _ => {}
                        }
                        next
                    }
                    other => other.clone(),
                })
                .collect();
            Message { content, ..message.clone() }
        })
        .collect();
    if !changed {
        return (tools, request.clone());
    }
    (tools, request.update(LLMRequestPatch { messages: Some(messages), ..Default::default() }))
}

/// Lower the message list (port of `lowerMessages`).
fn lower_messages(
    request: &LLMRequest,
    breakpoints: &mut Breakpoints,
) -> Result<Vec<Value>, AiError> {
    // Text updates stay where they are unless a user turn follows them; then
    // they move after the latest user turn, the nearest spot where Anthropic
    // accepts a native system message.
    fn release_held(
        messages: &mut Vec<Value>,
        held: &mut Vec<&Message>,
        breakpoints: &mut Breakpoints,
    ) -> Result<(), AiError> {
        let native = accepts_native_system_after(
            messages
                .iter()
                .rev()
                .find(|message| message.get("role").and_then(Value::as_str) != Some("system")),
        );
        for update in held.drain(..) {
            if native {
                messages.push(lower_native_system_update(update, breakpoints)?);
            } else {
                let block = lower_wrapped_system_update(update, breakpoints)?;
                append_to_user_turn(messages, block);
            }
        }
        Ok(())
    }

    let mut messages: Vec<Value> = Vec::new();
    let provider_metadata_key = request
        .model
        .route
        .provider_metadata_key
        .clone()
        .unwrap_or_else(|| request.model.provider.0.clone());
    let hold_updates = supports_native_system_updates(&request.model);
    let mut held: Vec<&Message> = Vec::new();

    for (index, message) in request.messages.iter().enumerate() {
        match message.role {
            MessageRole::System => {
                if let Some(marker) = crate::effort_updates::effort_update(message) {
                    // Accepted at any position; the placement rules do not
                    // apply.
                    messages.push(json!({
                        "role": "system",
                        "content": [],
                        "output_config": {"effort": marker.effort.unwrap_or_else(|| DEFAULT_EFFORT.into())},
                    }));
                    continue;
                }
                if splits_local_tool_results(&request.messages, index) {
                    return Err(invalid_request(
                        "Anthropic Messages system updates cannot split a local tool call from its tool result",
                    ));
                }
                if hold_updates {
                    held.push(message);
                } else {
                    let block = lower_wrapped_system_update(message, breakpoints)?;
                    append_to_user_turn(&mut messages, block);
                }
            }
            MessageRole::User => {
                let mut content: Vec<Value> = Vec::new();
                for part in &message.content {
                    match part {
                        ContentPart::Text { text, cache, .. } => {
                            if text.trim().is_empty() {
                                continue;
                            }
                            let mut object = Map::new();
                            object.insert("type".into(), json!("text"));
                            object.insert("text".into(), json!(text));
                            if let Some(control) = cache_control(breakpoints, cache.as_ref()) {
                                object.insert("cache_control".into(), control);
                            }
                            content.push(Value::Object(object));
                        }
                        ContentPart::Media { .. } => {
                            content.push(lower_media_part(part, Some(breakpoints))?);
                        }
                        _ => {
                            return Err(invalid_request(
                                "Anthropic Messages user messages only support text and media content for now",
                            ));
                        }
                    }
                }
                if !content.is_empty() {
                    messages.push(json!({"role": "user", "content": content}));
                }
            }
            MessageRole::Assistant => {
                let mut content: Vec<Value> = Vec::new();
                for part in &message.content {
                    match part {
                        ContentPart::Compaction { provider, text, .. } => {
                            if *provider != request.model.provider || text.is_none() {
                                return Err(invalid_request(
                                    "Compaction state must be replayed to its originating provider and API",
                                ));
                            }
                            content.push(json!({"type": "compaction", "content": text}));
                        }
                        ContentPart::Text { text, cache, .. } => {
                            if text.trim().is_empty() {
                                continue;
                            }
                            let mut object = Map::new();
                            object.insert("type".into(), json!("text"));
                            object.insert("text".into(), json!(text));
                            if let Some(control) = cache_control(breakpoints, cache.as_ref()) {
                                object.insert("cache_control".into(), control);
                            }
                            content.push(Value::Object(object));
                        }
                        ContentPart::Reasoning {
                            text,
                            encrypted,
                            cache,
                            provider_metadata,
                            ..
                        } => {
                            // A signature marks visible thinking; only
                            // signature-less parts carrying redactedData
                            // round-trip as opaque redacted_thinking blocks.
                            let signature = encrypted.clone().or_else(|| {
                                signature_from_metadata(
                                    provider_metadata.as_ref(),
                                    &provider_metadata_key,
                                )
                            });
                            let redacted_data = redacted_data_from_metadata(
                                provider_metadata.as_ref(),
                                &provider_metadata_key,
                            );
                            if signature.is_none() && redacted_data.is_some() {
                                content.push(json!({
                                    "type": "redacted_thinking",
                                    "data": redacted_data,
                                }));
                                continue;
                            }
                            let valid_signature =
                                signature.as_deref().is_some_and(|value| !value.trim().is_empty());
                            if !valid_signature {
                                if text.trim().is_empty() {
                                    continue;
                                }
                                if !require_thinking_signature(request) {
                                    content.push(json!({
                                        "type": "thinking",
                                        "thinking": text,
                                        "signature": "",
                                    }));
                                    continue;
                                }
                                // Without a signature this cannot be a valid
                                // thinking block — demote to text so the
                                // conversation remains sendable.
                                let mut object = Map::new();
                                object.insert("type".into(), json!("text"));
                                object.insert("text".into(), json!(text));
                                if let Some(control) = cache_control(breakpoints, cache.as_ref()) {
                                    object.insert("cache_control".into(), control);
                                }
                                content.push(Value::Object(object));
                                continue;
                            }
                            content.push(json!({
                                "type": "thinking",
                                "thinking": text,
                                "signature": signature,
                            }));
                        }
                        ContentPart::ToolCall { provider_executed: Some(true), .. } => {
                            content.push(lower_server_tool_call(part));
                        }
                        ContentPart::ToolCall { .. } => {
                            content.push(lower_tool_call(part));
                        }
                        ContentPart::ToolResult { provider_executed: Some(true), .. } => {
                            content.push(lower_server_tool_result(part, &provider_metadata_key)?);
                        }
                        _ => {
                            return Err(invalid_request(
                                "Anthropic Messages assistant messages only support text, reasoning, and tool-call content for now",
                            ));
                        }
                    }
                }
                if content.is_empty() {
                    continue;
                }
                release_held(&mut messages, &mut held, breakpoints)?;
                messages.push(json!({"role": "assistant", "content": content}));
            }
            MessageRole::Tool => {
                let mut content: Vec<Value> = Vec::new();
                for part in &message.content {
                    let ContentPart::ToolResult { id, result, cache, .. } = part else {
                        return Err(invalid_request(
                            "Anthropic Messages tool messages only support tool-result content for now",
                        ));
                    };
                    let mut object = Map::new();
                    object.insert("type".into(), json!("tool_result"));
                    object.insert("tool_use_id".into(), json!(scrub_tool_call_id(id)));
                    object.insert("content".into(), lower_tool_result_content(part)?);
                    if matches!(result, ToolResultValue::Error { .. }) {
                        object.insert("is_error".into(), json!(true));
                    }
                    if let Some(control) = cache_control(breakpoints, cache.as_ref()) {
                        object.insert("cache_control".into(), control);
                    }
                    content.push(Value::Object(object));
                }
                let mergeable = messages.last().is_some_and(|last| {
                    last.get("role").and_then(Value::as_str) == Some("user")
                        && last.get("content").and_then(Value::as_array).is_some_and(|blocks| {
                            blocks.iter().all(|block| {
                                block.get("type").and_then(Value::as_str) == Some("tool_result")
                            })
                        })
                });
                if mergeable {
                    if let Some(previous) = messages.last_mut() {
                        if let Some(blocks) =
                            previous.get_mut("content").and_then(Value::as_array_mut)
                        {
                            blocks.extend(content);
                            continue;
                        }
                    }
                }
                messages.push(json!({"role": "user", "content": content}));
            }
        }
    }
    release_held(&mut messages, &mut held, breakpoints)?;
    Ok(messages)
}

/// Anthropic requires an explicit thinking budget below `max_tokens` and at
/// or above its minimum.
fn fit_thinking(thinking: Option<Thinking>, max_tokens: f64) -> Option<Thinking> {
    match thinking {
        Some(Thinking::Enabled { budget_tokens, display, block_binding }) => {
            Some(Thinking::Enabled {
                budget_tokens: fit_thinking_budget(
                    budget_tokens,
                    Some(max_tokens),
                    MIN_THINKING_BUDGET,
                ),
                display,
                block_binding,
            })
        }
        other => other,
    }
}

/// Default `block_binding` on models that support the controls (port of
/// `applyThinkingBindingDefault`).
fn apply_thinking_binding_default(
    model: &LanguageModel,
    thinking: Option<Thinking>,
) -> Option<Thinking> {
    if matches!(thinking, Some(Thinking::Disabled)) || !supports_thinking_block_binding(model) {
        return thinking;
    }
    let default_binding =
        || ThinkingBlockBinding { prefix_mismatch_behavior: Some("drop_block".into()) };
    match thinking {
        None => Some(Thinking::Adaptive { display: None, block_binding: Some(default_binding()) }),
        Some(Thinking::Enabled { budget_tokens, display, block_binding }) => {
            Some(Thinking::Enabled {
                budget_tokens,
                display,
                block_binding: Some(block_binding.unwrap_or_else(default_binding)),
            })
        }
        Some(Thinking::Adaptive { display, block_binding }) => Some(Thinking::Adaptive {
            display,
            block_binding: Some(block_binding.unwrap_or_else(default_binding)),
        }),
        Some(Thinking::Disabled) => thinking,
    }
}

fn block_binding_value(binding: &ThinkingBlockBinding) -> Value {
    let mut object = Map::new();
    if let Some(behavior) = &binding.prefix_mismatch_behavior {
        object.insert("prefix_mismatch_behavior".into(), json!(behavior));
    }
    Value::Object(object)
}

fn thinking_to_value(thinking: &Thinking) -> Value {
    match thinking {
        Thinking::Enabled { budget_tokens, display, block_binding } => {
            let mut object = Map::new();
            object.insert("type".into(), json!("enabled"));
            object.insert("budget_tokens".into(), number(*budget_tokens));
            if let Some(display) = display {
                object.insert("display".into(), json!(display));
            }
            if let Some(binding) = block_binding {
                object.insert("block_binding".into(), block_binding_value(binding));
            }
            Value::Object(object)
        }
        Thinking::Adaptive { display, block_binding } => {
            let mut object = Map::new();
            object.insert("type".into(), json!("adaptive"));
            if let Some(display) = display {
                object.insert("display".into(), json!(display));
            }
            if let Some(binding) = block_binding {
                object.insert("block_binding".into(), block_binding_value(binding));
            }
            Value::Object(object)
        }
        Thinking::Disabled => json!({"type": "disabled"}),
    }
}

/// The beta headers a built body needs (port of `requiredBetaHeaders`,
/// anthropic-messages.ts:1641 — computed over the request body exactly like
/// the TS transport wrapper).
#[must_use]
pub fn required_beta_headers_of_body(body: &Value) -> Vec<String> {
    // Always request interleaved thinking. The API accepts the header on any
    // model and ignores it where unsupported, while manual-thinking models
    // need it for thinking between tool calls.
    let mut betas = vec!["interleaved-thinking-2025-05-14".to_string()];
    let requests_compaction = body
        .pointer("/context_management/edits")
        .and_then(Value::as_array)
        .is_some_and(|edits| !edits.is_empty());
    let replays_compaction =
        body.get("messages").and_then(Value::as_array).is_some_and(|messages| {
            messages.iter().any(|message| {
                message.get("content").and_then(Value::as_array).is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        block.get("type").and_then(Value::as_str) == Some("compaction")
                    })
                })
            })
        });
    if requests_compaction || replays_compaction {
        betas.push("compact-2026-01-12".to_string());
    }
    let has_output_config =
        body.get("messages").and_then(Value::as_array).is_some_and(|messages| {
            messages.iter().any(|message| {
                message.get("role").and_then(Value::as_str) == Some("system")
                    && message.get("output_config").is_some()
            })
        });
    if has_output_config {
        betas.push("mid-conversation-output-config-2026-07-01".to_string());
    }
    let thinking = body.get("thinking").filter(|value| !value.is_null());
    let thinking_enabled = thinking
        .is_some_and(|thinking| thinking.get("type").and_then(Value::as_str) != Some("disabled"));
    if thinking_enabled && thinking.is_some_and(|thinking| thinking.get("block_binding").is_some())
    {
        betas.push(THINKING_BINDING_BETA.to_string());
    }
    betas
}

/// Transport wrapper injecting the `anthropic-beta` header at send time
/// (port of the TS `transport()` wrapper, anthropic-messages.ts:1613): reads
/// the built request body, computes the required betas, and unions them with
/// any caller-provided `anthropic-beta` header — the last writer, exactly
/// like the TS prepare seam.
struct BetaHeadersTransport {
    inner: SharedTransport,
}

impl BetaHeadersTransport {
    fn wrap(inner: SharedTransport) -> SharedTransport {
        Arc::new(Self { inner })
    }
}

#[async_trait::async_trait]
impl crate::route::transport::Transport for BetaHeadersTransport {
    async fn execute(
        &self,
        mut request: crate::route::transport::TransportRequest,
    ) -> Result<crate::route::transport::TransportResponse, crate::route::transport::TransportError>
    {
        let required = serde_json::from_slice::<Value>(&request.body).map_or_else(
            |_| vec!["interleaved-thinking-2025-05-14".to_string()],
            |body| required_beta_headers_of_body(&body),
        );
        let existing = request.header("anthropic-beta").unwrap_or_default();
        let mut betas: Vec<String> = existing
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        for beta in required {
            if !betas.contains(&beta) {
                betas.push(beta);
            }
        }
        request.headers.retain(|(name, _)| !name.eq_ignore_ascii_case("anthropic-beta"));
        request.headers.push(("anthropic-beta".to_string(), betas.join(",")));
        self.inner.execute(request).await
    }
}

/// Build the Messages API body (port of `fromRequest`).
pub fn body_from_request(request: &LLMRequest) -> Result<Value, AiError> {
    // Per-provider seam (RW-01): providers with an Anthropic-family body
    // wrapper (alibaba/zai thinking re-attach, meta web_search remap) build
    // the generic body with the raw `thinking` option stripped; the hook
    // then re-attaches the provider's own shape.
    let provider = request.model.provider.0.as_str();
    let wrap = crate::customization::for_route(provider).wrap_anthropic_body;
    let stripped;
    let raw_thinking;
    let request: &LLMRequest = if wrap.is_some() {
        let mut next = request.clone();
        raw_thinking =
            next.provider_options.as_mut().and_then(|options| options.remove("thinking"));
        stripped = next;
        &stripped
    } else {
        raw_thinking = None;
        request
    };
    let options = decode_options(request.provider_options.as_ref())?;
    let management = options.context_management.clone();
    let output_config = options.output_config.clone();
    let format = output_config.as_ref().and_then(|config| config.format.clone());
    let requested_effort = options
        .effort
        .clone()
        .or_else(|| output_config.as_ref().and_then(|config| config.effort.clone()));
    let (request, effort) = resolve_effort_updates(request, requested_effort.as_deref());
    let generation = request.generation.clone().unwrap_or_default();
    // Allocate the 4-breakpoint budget in invalidation order: tools → system
    // → messages.
    let mut breakpoints = Breakpoints::new(ANTHROPIC_BREAKPOINT_CAP);
    let (flattened_tools, flattened_request) = flatten_tool_request(&request);
    let tools = if flattened_tools.is_empty() {
        None
    } else {
        Some(Value::Array(
            flattened_tools.iter().map(|tool| lower_tool(&mut breakpoints, tool)).collect(),
        ))
    };
    // Anthropic rejects tool_choice when tools are absent.
    let tool_choice = match (&tools, &request.tool_choice) {
        (Some(_), Some(choice)) => Some(lower_tool_choice(choice)?),
        _ => None,
    };
    let system_parts: Vec<&SystemPart> =
        request.system.iter().filter(|part| !part.text.is_empty()).collect();
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(Value::Array(
            system_parts
                .iter()
                .map(|part| {
                    let mut object = Map::new();
                    object.insert("type".into(), json!("text"));
                    object.insert("text".into(), json!(part.text));
                    if let Some(control) = cache_control(&mut breakpoints, part.cache.as_ref()) {
                        object.insert("cache_control".into(), control);
                    }
                    Value::Object(object)
                })
                .collect(),
        ))
    };
    let messages = lower_messages(&flattened_request, &mut breakpoints)?;
    let output_config_value = match (&effort, &format) {
        (None, None) => None,
        (effort, format) => {
            let mut object = Map::new();
            if let Some(effort) = effort {
                object.insert("effort".into(), json!(effort));
            }
            if let Some(format) = format {
                object.insert(
                    "format".into(),
                    json!({"type": "json_schema", "schema": format.schema}),
                );
            }
            Some(Value::Object(object))
        }
    };
    let max_tokens = generation.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    let thinking = apply_thinking_binding_default(
        &request.model,
        fit_thinking(options.thinking.clone(), max_tokens),
    );

    let mut body = Map::new();
    body.insert("model".into(), json!(request.model.id.0));
    if let Some(system) = system {
        body.insert("system".into(), system);
    }
    body.insert("messages".into(), Value::Array(messages));
    if let Some(tools) = tools {
        body.insert("tools".into(), tools);
    }
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".into(), tool_choice);
    }
    body.insert("stream".into(), json!(true));
    body.insert("max_tokens".into(), number(max_tokens));
    if let Some(temperature) = generation.temperature {
        body.insert("temperature".into(), number(temperature));
    }
    if let Some(top_p) = generation.top_p {
        body.insert("top_p".into(), number(top_p));
    }
    if let Some(top_k) = generation.top_k {
        body.insert("top_k".into(), number(top_k));
    }
    if let Some(stop) = &generation.stop {
        body.insert("stop_sequences".into(), json!(stop));
    }
    if let Some(thinking) = thinking {
        body.insert("thinking".into(), thinking_to_value(&thinking));
    }
    if let Some(output_config) = output_config_value {
        body.insert("output_config".into(), output_config);
    }
    // Top-level passthrough per SDK MessageCreateParamsBase.
    if let Some(cache_control) = options.cache_control {
        body.insert("cache_control".into(), cache_control);
    }
    if let Some(container) = options.container {
        body.insert("container".into(), container);
    }
    if let Some(inference_geo) = options.inference_geo {
        body.insert("inference_geo".into(), json!(inference_geo));
    }
    if let Some(metadata) = options.metadata {
        body.insert("metadata".into(), metadata);
    }
    if let Some(service_tier) = options.service_tier {
        body.insert("service_tier".into(), json!(service_tier));
    }
    if let Some(management) = management {
        let edits: Vec<Value> = management
            .edits
            .iter()
            .map(|edit| {
                let mut object = Map::new();
                object.insert("type".into(), json!("compact_20260112"));
                if let Some(trigger) = &edit.trigger {
                    object.insert("trigger".into(), trigger.clone());
                }
                if let Some(pause) = edit.pause_after_compaction {
                    object.insert("pause_after_compaction".into(), json!(pause));
                }
                if let Some(instructions) = &edit.instructions {
                    object.insert("instructions".into(), json!(instructions));
                }
                Value::Object(object)
            })
            .collect();
        body.insert("context_management".into(), json!({"edits": edits}));
    }
    let mut body = Value::Object(body);
    if let Some(wrap) = wrap {
        let natives: Vec<Option<&std::collections::BTreeMap<String, Value>>> =
            flattened_tools.iter().map(|tool| tool.native.as_ref()).collect();
        wrap(&mut crate::customization::AnthropicBodyWrap {
            provider,
            raw_thinking: raw_thinking.as_ref(),
            tool_natives: &natives,
            max_tokens,
            body: &mut body,
        })?;
    }
    Ok(body)
}

// =============================================================================
// Stream parsing
// =============================================================================

fn map_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("end_turn" | "stop_sequence" | "pause_turn") => FinishReason::Stop,
        Some("max_tokens" | "model_context_window_exceeded") => FinishReason::Length,
        Some("tool_use") => FinishReason::ToolCalls,
        Some("refusal") => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

/// Anthropic reports the non-overlapping breakdown natively — `input_tokens`
/// is the *non-cached* count; sum up to the inclusive total the contract
/// expects (port of `mapUsage`).
fn map_usage(usage: Option<&Value>, provider_metadata_key: &str) -> Option<Usage> {
    let usage = usage?;
    let iterations: Vec<&Value> = match usage.get("iterations").and_then(Value::as_array) {
        Some(items) if !items.is_empty() => items.iter().collect(),
        _ => vec![usage],
    };
    let last = usage.get("iterations").and_then(Value::as_array).and_then(|items| items.last());
    let nullable_number = |value: &Value, name: &str| {
        value.get(name).filter(|v| !v.is_null()).and_then(Value::as_f64)
    };
    let non_cached = sum_tokens(
        &iterations.iter().map(|item| nullable_number(item, "input_tokens")).collect::<Vec<_>>(),
    );
    let cache_read = sum_tokens(
        &iterations
            .iter()
            .map(|item| nullable_number(item, "cache_read_input_tokens"))
            .collect::<Vec<_>>(),
    );
    let cache_write = sum_tokens(
        &iterations
            .iter()
            .map(|item| nullable_number(item, "cache_creation_input_tokens"))
            .collect::<Vec<_>>(),
    );
    let input_tokens = sum_non_cached_to_inclusive(non_cached, cache_read, cache_write);
    let output_tokens = sum_tokens(
        &iterations
            .iter()
            .map(|item| item.get("output_tokens").and_then(Value::as_f64))
            .collect::<Vec<_>>(),
    );
    let reasoning_tokens = sum_tokens(
        &iterations
            .iter()
            .map(|item| {
                item.get("output_tokens_details")
                    .filter(|value| !value.is_null())
                    .and_then(|details| details.get("thinking_tokens"))
                    .and_then(Value::as_f64)
            })
            .collect::<Vec<_>>(),
    );
    let context_tokens = (last.and_then(|last| last.get("type")).and_then(Value::as_str)
        == Some("message"))
    .then(|| {
        sum_tokens(&[
            last.and_then(|last| nullable_number(last, "input_tokens")),
            last.and_then(|last| nullable_number(last, "cache_read_input_tokens")),
            last.and_then(|last| nullable_number(last, "cache_creation_input_tokens")),
        ])
    })
    .flatten();
    Some(Usage {
        context_tokens,
        input_tokens,
        output_tokens,
        non_cached_input_tokens: non_cached,
        cache_read_input_tokens: cache_read,
        cache_write_input_tokens: cache_write,
        reasoning_tokens,
        total_tokens: total_tokens(input_tokens, output_tokens, None),
        provider_metadata: Some(provider_metadata(provider_metadata_key, usage.clone())),
    })
}

/// Right-biased usage merge across `message_start` + `message_delta`; the
/// inclusive total is recomputed from the merged breakdown (port of
/// `mergeUsage`).
fn merge_usage(
    left: Option<Usage>,
    right: Option<Usage>,
    provider_metadata_key: &str,
) -> Option<Usage> {
    let (Some(left), Some(right)) = (&left, &right) else {
        return left.or(right);
    };
    let non_cached_input_tokens = right.non_cached_input_tokens.or(left.non_cached_input_tokens);
    let cache_read_input_tokens = right.cache_read_input_tokens.or(left.cache_read_input_tokens);
    let cache_write_input_tokens = right.cache_write_input_tokens.or(left.cache_write_input_tokens);
    let input_tokens = sum_non_cached_to_inclusive(
        non_cached_input_tokens,
        cache_read_input_tokens,
        cache_write_input_tokens,
    );
    let output_tokens = right.output_tokens.or(left.output_tokens);
    let reasoning_tokens = right.reasoning_tokens.or(left.reasoning_tokens);
    let raw_record = |usage: &Usage| -> Option<serde_json::Map<String, Value>> {
        let metadata = usage.provider_metadata.as_ref()?.get(provider_metadata_key)?;
        Some(metadata.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
    };
    let merged_raw = crate::schema::options::merge_json_records(&[
        left.provider_metadata.as_ref().and_then(|_| raw_record(left)).as_ref(),
        right.provider_metadata.as_ref().and_then(|_| raw_record(right)).as_ref(),
    ]);
    Some(Usage {
        input_tokens,
        output_tokens,
        context_tokens: right.context_tokens.or(left.context_tokens),
        non_cached_input_tokens,
        cache_read_input_tokens,
        cache_write_input_tokens,
        reasoning_tokens,
        total_tokens: total_tokens(input_tokens, output_tokens, None),
        provider_metadata: Some(provider_metadata(
            provider_metadata_key,
            Value::Object(merged_raw.unwrap_or_default()),
        )),
    })
}

/// Parser state threaded through `step` (port of `ParserState`). The open
/// text/reasoning id mirrors track the shared lifecycle's membership sets
/// (which are private to the lifecycle module).
#[derive(Debug, Clone)]
pub struct ParserState {
    provider: ProviderId,
    compactions: BTreeMap<String, Option<String>>,
    provider_metadata_key: String,
    tools: tool_stream::State,
    reasoning_signatures: BTreeMap<String, String>,
    usage: Option<Usage>,
    pending_finish: Option<PendingFinish>,
    lifecycle: lifecycle::State,
    open_text: BTreeSet<String>,
    open_reasoning: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct PendingFinish {
    reason: FinishReasonDetails,
    provider_metadata: Option<ProviderMetadata>,
}

fn invalid_stream_event_message() -> String {
    "Invalid anthropic/anthropic-messages stream event".to_string()
}

fn invalid_stream_event(value: &Value) -> AiError {
    let mut error = event_error(ADAPTER, invalid_stream_event_message());
    if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
        fields.body = Some(encode_json(value));
    }
    error
}

// -- lenient schema checks over serde_json::Value ---------------------------

/// `Schema.optional(Schema.String)`: absent ok; `null` fails like any
/// non-string.
fn check_optional_string(value: Option<&Value>) -> Result<(), AiError> {
    match value {
        None => Ok(()),
        Some(Value::String(_)) => Ok(()),
        Some(_) => Err(event_error(ADAPTER, invalid_stream_event_message())),
    }
}

/// `optionalNull(String)`.
fn check_optional_null_string(value: Option<&Value>) -> Result<(), AiError> {
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(_)) => Ok(()),
        Some(_) => Err(event_error(ADAPTER, invalid_stream_event_message())),
    }
}

/// `Schema.optional(Schema.Number)`.
fn check_optional_number(value: Option<&Value>) -> Result<(), AiError> {
    match value {
        None => Ok(()),
        Some(v) if v.is_number() => Ok(()),
        Some(_) => Err(event_error(ADAPTER, invalid_stream_event_message())),
    }
}

/// One usage record (port of `AnthropicIterationUsage` + `AnthropicUsage`).
fn check_usage(value: &Value) -> Result<(), AiError> {
    let Some(object) = value.as_object() else {
        return Err(event_error(ADAPTER, invalid_stream_event_message()));
    };
    for field in ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"] {
        check_optional_null_string_number(object.get(field))?;
    }
    check_optional_number(object.get("output_tokens"))?;
    if let Some(server_tool_use) = object.get("server_tool_use") {
        if !server_tool_use.is_null() {
            let Some(server_object) = server_tool_use.as_object() else {
                return Err(event_error(ADAPTER, invalid_stream_event_message()));
            };
            check_optional_number(server_object.get("web_search_requests"))?;
        }
    }
    if let Some(details) = object.get("output_tokens_details") {
        if !details.is_null() {
            let Some(details_object) = details.as_object() else {
                return Err(event_error(ADAPTER, invalid_stream_event_message()));
            };
            check_optional_number(details_object.get("thinking_tokens"))?;
        }
    }
    if let Some(iterations) = object.get("iterations") {
        let Value::Array(items) = iterations else {
            return Err(event_error(ADAPTER, invalid_stream_event_message()));
        };
        for item in items {
            check_usage(item)?;
        }
    }
    Ok(())
}

/// `optionalNull(Number)`.
fn check_optional_null_string_number(value: Option<&Value>) -> Result<(), AiError> {
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(v) if v.is_number() => Ok(()),
        Some(_) => Err(event_error(ADAPTER, invalid_stream_event_message())),
    }
}

/// `AnthropicStreamBlock` (`StructWithRest`: unknown fields allowed).
fn check_stream_block(value: &Value) -> Result<(), AiError> {
    let Some(object) = value.as_object() else {
        return Err(event_error(ADAPTER, invalid_stream_event_message()));
    };
    match object.get("type") {
        Some(Value::String(_)) => {}
        _ => return Err(event_error(ADAPTER, invalid_stream_event_message())),
    }
    for field in ["id", "name", "text", "thinking", "signature", "data", "tool_use_id"] {
        check_optional_string(object.get(field))?;
    }
    Ok(())
}

/// `AnthropicStreamDelta`.
fn check_stream_delta(value: &Value) -> Result<(), AiError> {
    let Some(object) = value.as_object() else {
        return Err(event_error(ADAPTER, invalid_stream_event_message()));
    };
    check_optional_null_string(object.get("content"))?;
    check_optional_string(object.get("type"))?;
    for field in ["text", "thinking", "partial_json", "signature"] {
        check_optional_string(object.get(field))?;
    }
    for field in ["stop_reason", "stop_sequence"] {
        check_optional_null_string(object.get(field))?;
    }
    if let Some(details) = object.get("stop_details") {
        if !details.is_null() {
            let Some(details_object) = details.as_object() else {
                return Err(event_error(ADAPTER, invalid_stream_event_message()));
            };
            for field in ["category", "explanation"] {
                check_optional_null_string(details_object.get(field))?;
            }
        }
    }
    Ok(())
}

/// Top-level `AnthropicEvent` decode + validation.
fn decode_event(frame: &str) -> Result<Value, AiError> {
    let value: Value =
        decode_json(frame).map_err(|_| event_error(ADAPTER, invalid_stream_event_message()))?;
    if !value.is_object() {
        return Err(invalid_stream_event(&value));
    }
    let object = value.as_object().expect("checked record");
    match object.get("type") {
        Some(Value::String(_)) => {}
        _ => return Err(invalid_stream_event(&value)),
    }
    check_optional_number(object.get("index")).map_err(|_| invalid_stream_event(&value))?;
    if let Some(message) = object.get("message") {
        let Some(message_object) = message.as_object() else {
            return Err(invalid_stream_event(&value));
        };
        if let Some(usage) = message_object.get("usage") {
            check_usage(usage).map_err(|_| invalid_stream_event(&value))?;
        }
    }
    if let Some(usage) = object.get("usage") {
        check_usage(usage).map_err(|_| invalid_stream_event(&value))?;
    }
    if let Some(error) = object.get("error") {
        let Some(error_object) = error.as_object() else {
            return Err(invalid_stream_event(&value));
        };
        check_optional_string(error_object.get("type"))
            .map_err(|_| invalid_stream_event(&value))?;
        check_optional_string(error_object.get("message"))
            .map_err(|_| invalid_stream_event(&value))?;
    }
    Ok(value)
}

/// Index map key (Anthropic content indexes are integers).
fn index_key(index: f64) -> String {
    format!("{}", index as i64)
}

fn stream_index(event: &Value) -> Option<f64> {
    event.get("index").and_then(Value::as_f64)
}

/// Server tool result blocks convert to provider-executed `tool-result`
/// events carrying the structured payload as irreducible replay state.
fn server_tool_result_event(block: &Value, provider_metadata_key: &str) -> Option<LLMEvent> {
    let block_type = block.get("type").and_then(Value::as_str)?;
    let name = match block_type {
        "web_search_tool_result" => "web_search",
        "code_execution_tool_result" => "code_execution",
        "web_fetch_tool_result" => "web_fetch",
        _ => return None,
    };
    let content = block.get("content").cloned().unwrap_or(Value::Null);
    let error_payload = content
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let is_error = error_payload.ends_with("_tool_result_error");
    let result = if is_error {
        ToolResultValue::Error { value: content.clone() }
    } else {
        ToolResultValue::Json { value: content.clone() }
    };
    Some(LLMEvent::ToolResult {
        id: block.get("tool_use_id").and_then(Value::as_str).unwrap_or_default().to_string(),
        name: name.to_string(),
        namespace: None,
        result,
        output: None,
        provider_executed: Some(true),
        provider_metadata: Some(provider_metadata(
            provider_metadata_key,
            json!({"blockType": block_type, "result": content}),
        )),
    })
}

type StepOutcome = (ParserState, Vec<LLMEvent>);

fn on_message_start(state: &ParserState, event: &Value) -> StepOutcome {
    let usage = map_usage(
        event.get("message").and_then(|message| message.get("usage")),
        &state.provider_metadata_key,
    );
    let usage = merge_usage(state.usage.clone(), usage, &state.provider_metadata_key);
    (ParserState { usage, ..state.clone() }, Vec::new())
}

fn on_content_block_start(state: &ParserState, event: &Value) -> StepOutcome {
    let block = event.get("content_block").expect("caller validated presence");
    let block_type = block.get("type").and_then(Value::as_str).unwrap_or_default();
    let index = stream_index(event);

    if block_type == "tool_use" || block_type == "server_tool_use" {
        let (Some(index), Some(id)) = (index, block.get("id").and_then(Value::as_str)) else {
            return (state.clone(), Vec::new());
        };
        let mut events = Vec::new();
        let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
        let name = block.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
        // Pre-encoded complete input (non-empty records only); "" = none.
        let input = block.get("input").and_then(|input| {
            let keep =
                !input.is_object() || input.as_object().is_some_and(|object| !object.is_empty());
            keep.then(|| encode_json(input))
        });
        let tools = tool_stream::start(
            &state.tools,
            &index_key(index),
            tool_stream::PendingTool {
                id: id.to_string(),
                name: name.clone(),
                namespace: None,
                input: input.unwrap_or_default(),
                provider_executed: (block_type == "server_tool_use").then_some(true),
                provider_metadata: None,
            },
        );
        events.push(LLMEvent::ToolInputStart {
            id: id.to_string(),
            name,
            namespace: None,
            provider_executed: (block_type == "server_tool_use").then_some(true),
            provider_metadata: None,
        });
        return (ParserState { tools, lifecycle, ..state.clone() }, events);
    }

    if block_type == "text" && block.get("text").is_some() {
        let mut events = Vec::new();
        let id = format!("text-{}", index.unwrap_or(0.0));
        let lifecycle = lifecycle::text_start(&state.lifecycle, &mut events, &id, None);
        let text = block.get("text").and_then(Value::as_str).unwrap_or_default();
        let lifecycle = if text.is_empty() {
            lifecycle
        } else {
            lifecycle::text_delta(&lifecycle, &mut events, &id, text, None)
        };
        let mut open_text = state.open_text.clone();
        open_text.insert(id);
        return (ParserState { lifecycle, open_text, ..state.clone() }, events);
    }

    if block_type == "thinking" && block.get("thinking").is_some() {
        let mut events = Vec::new();
        let id = format!("reasoning-{}", index.unwrap_or(0.0));
        let metadata = block.get("signature").and_then(Value::as_str).map(|signature| {
            provider_metadata(&state.provider_metadata_key, json!({"signature": signature}))
        });
        let lifecycle = lifecycle::reasoning_start(&state.lifecycle, &mut events, &id, metadata);
        let thinking = block.get("thinking").and_then(Value::as_str).unwrap_or_default();
        let lifecycle = if thinking.is_empty() {
            lifecycle
        } else {
            lifecycle::reasoning_delta(&lifecycle, &mut events, &id, thinking, None)
        };
        let mut reasoning_signatures = state.reasoning_signatures.clone();
        if let (Some(index), Some(signature)) =
            (index, block.get("signature").and_then(Value::as_str))
        {
            reasoning_signatures.insert(index_key(index), signature.to_string());
        }
        let mut open_reasoning = state.open_reasoning.clone();
        open_reasoning.insert(id);
        return (
            ParserState { reasoning_signatures, lifecycle, open_reasoning, ..state.clone() },
            events,
        );
    }

    // Redacted thinking surfaces as an empty reasoning part carrying the
    // opaque payload as `redactedData` metadata.
    if block_type == "redacted_thinking" && block.get("data").is_some() {
        let mut events = Vec::new();
        let id = format!("reasoning-{}", index.unwrap_or(0.0));
        let lifecycle = lifecycle::reasoning_start(
            &state.lifecycle,
            &mut events,
            &id,
            Some(provider_metadata(
                &state.provider_metadata_key,
                json!({"redactedData": block.get("data").and_then(Value::as_str).unwrap_or_default()}),
            )),
        );
        let mut open_reasoning = state.open_reasoning.clone();
        open_reasoning.insert(id);
        return (ParserState { lifecycle, open_reasoning, ..state.clone() }, events);
    }

    let Some(result) = server_tool_result_event(block, &state.provider_metadata_key) else {
        return (state.clone(), Vec::new());
    };
    let mut events = Vec::new();
    let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
    events.push(result);
    (ParserState { lifecycle, ..state.clone() }, events)
}

fn on_content_block_delta(state: &ParserState, event: &Value) -> Result<StepOutcome, AiError> {
    let delta = event.get("delta").expect("caller validated presence");
    let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or_default();
    let index = stream_index(event);

    if delta_type == "compaction_delta" {
        let valid = index.is_some_and(|index| {
            state.compactions.contains_key(&index_key(index)) && delta.get("content").is_some()
        });
        if !valid {
            return Err(event_error(ADAPTER, "Compaction delta is missing its block or content"));
        }
        let key = index_key(index.expect("checked above"));
        let content = delta.get("content").and_then(Value::as_str).map(str::to_string);
        let mut compactions = state.compactions.clone();
        compactions.insert(key, content);
        return Ok((ParserState { compactions, ..state.clone() }, Vec::new()));
    }

    if delta_type == "text_delta" {
        let Some(text) = delta.get("text").and_then(Value::as_str).filter(|text| !text.is_empty())
        else {
            return Ok((state.clone(), Vec::new()));
        };
        let id = format!("text-{}", index.unwrap_or(0.0));
        if !state.open_text.contains(&id) {
            return Ok((state.clone(), Vec::new()));
        }
        let mut events = Vec::new();
        let lifecycle = lifecycle::text_delta(&state.lifecycle, &mut events, &id, text, None);
        return Ok((ParserState { lifecycle, ..state.clone() }, events));
    }

    if delta_type == "thinking_delta" {
        let Some(thinking) =
            delta.get("thinking").and_then(Value::as_str).filter(|text| !text.is_empty())
        else {
            return Ok((state.clone(), Vec::new()));
        };
        let id = format!("reasoning-{}", index.unwrap_or(0.0));
        if !state.open_reasoning.contains(&id) {
            return Ok((state.clone(), Vec::new()));
        }
        let mut events = Vec::new();
        let lifecycle =
            lifecycle::reasoning_delta(&lifecycle_state(state), &mut events, &id, thinking, None);
        return Ok((ParserState { lifecycle, ..state.clone() }, events));
    }

    if delta_type == "signature_delta" {
        let Some(signature) =
            delta.get("signature").and_then(Value::as_str).filter(|text| !text.is_empty())
        else {
            return Ok((state.clone(), Vec::new()));
        };
        let index = index.unwrap_or(0.0);
        let id = format!("reasoning-{index}");
        if !state.open_reasoning.contains(&id) {
            return Ok((state.clone(), Vec::new()));
        }
        let mut reasoning_signatures = state.reasoning_signatures.clone();
        reasoning_signatures.insert(index_key(index), signature.to_string());
        return Ok((ParserState { reasoning_signatures, ..state.clone() }, Vec::new()));
    }

    if delta_type == "input_json_delta" {
        let Some(index) = index else { return Ok((state.clone(), Vec::new())) };
        let Some(partial_json) =
            delta.get("partial_json").and_then(Value::as_str).filter(|text| !text.is_empty())
        else {
            return Ok((state.clone(), Vec::new()));
        };
        let key = index_key(index);
        if state.tools.get(&key).is_none() {
            return Ok((state.clone(), Vec::new()));
        }
        let result = tool_stream::append_existing(
            ADAPTER,
            &state.tools,
            &key,
            partial_json,
            "Anthropic Messages tool argument delta is missing its tool call",
        )?;
        let mut events = Vec::new();
        let lifecycle = if result.events.is_empty() {
            state.lifecycle.clone()
        } else {
            lifecycle::step_start(&state.lifecycle, &mut events)
        };
        events.extend(result.events);
        return Ok((ParserState { lifecycle, tools: result.tools, ..state.clone() }, events));
    }

    Ok((state.clone(), Vec::new()))
}

fn lifecycle_state(state: &ParserState) -> lifecycle::State {
    state.lifecycle.clone()
}

fn on_content_block_stop(state: &ParserState, event: &Value) -> Result<StepOutcome, AiError> {
    let Some(index) = stream_index(event) else {
        return Ok((state.clone(), Vec::new()));
    };
    let key = index_key(index);
    if let Some(content) = state.compactions.get(&key).cloned() {
        let mut compactions = state.compactions.clone();
        compactions.remove(&key);
        let mut events = Vec::new();
        let lifecycle = lifecycle::step_start(&state.lifecycle, &mut events);
        events.push(LLMEvent::Compaction {
            provider: state.provider.clone(),
            id: None,
            encrypted: None,
            text: content,
        });
        return Ok((ParserState { compactions, lifecycle, ..state.clone() }, events));
    }
    let result = tool_stream::finish(ADAPTER, &state.tools, &key)?;
    let mut events = Vec::new();
    let result_events = result.events;
    let signature = state.reasoning_signatures.get(&key).cloned();
    let text_id = format!("text-{index}");
    let reasoning_id = format!("reasoning-{index}");
    let lifecycle = if result_events.is_empty() {
        let signature_metadata = signature.as_deref().map(|signature| {
            provider_metadata(&state.provider_metadata_key, json!({"signature": signature}))
        });
        lifecycle::reasoning_end(
            &lifecycle::text_end(&state.lifecycle, &mut events, &text_id, None, None),
            &mut events,
            &reasoning_id,
            signature_metadata,
            None,
        )
    } else {
        lifecycle::step_start(&state.lifecycle, &mut events)
    };
    events.extend(result_events);
    let mut reasoning_signatures = state.reasoning_signatures.clone();
    reasoning_signatures.remove(&key);
    let mut open_text = state.open_text.clone();
    open_text.remove(&text_id);
    let mut open_reasoning = state.open_reasoning.clone();
    open_reasoning.remove(&reasoning_id);
    Ok((
        ParserState {
            lifecycle,
            tools: result.tools,
            reasoning_signatures,
            open_text,
            open_reasoning,
            ..state.clone()
        },
        events,
    ))
}

fn on_message_delta(state: &ParserState, event: &Value) -> StepOutcome {
    let usage = merge_usage(
        state.usage.clone(),
        map_usage(event.get("usage"), &state.provider_metadata_key),
        &state.provider_metadata_key,
    );
    let pending_finish = (|| {
        let delta = event.get("delta");
        let Some(stop_reason) =
            delta.and_then(|delta| delta.get("stop_reason")).and_then(Value::as_str)
        else {
            return state.pending_finish.clone();
        };
        let stop_sequence =
            delta.and_then(|delta| delta.get("stop_sequence")).and_then(Value::as_str);
        let finish_metadata = match stop_sequence {
            Some(stop_sequence) => Some(provider_metadata(
                &state.provider_metadata_key,
                json!({"stopSequence": stop_sequence}),
            )),
            None => {
                state.pending_finish.as_ref().and_then(|finish| finish.provider_metadata.clone())
            }
        };
        let stop_details =
            delta.and_then(|delta| delta.get("stop_details")).filter(|value| !value.is_null());
        let category = stop_details
            .and_then(|details| details.get("category"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let explanation = stop_details
            .and_then(|details| details.get("explanation"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        Some(PendingFinish {
            reason: FinishReasonDetails {
                normalized: map_finish_reason(Some(stop_reason)),
                raw: Some(stop_reason.to_string()),
                category: category.map(str::to_string),
                explanation: explanation.map(str::to_string),
            },
            provider_metadata: finish_metadata,
        })
    })();
    (ParserState { usage, pending_finish, ..state.clone() }, Vec::new())
}

fn on_message_stop(state: &ParserState) -> Result<StepOutcome, AiError> {
    if !state.compactions.is_empty() {
        return Err(event_error(ADAPTER, "Response ended with an incomplete compaction block"));
    }
    let result = tool_stream::finish_all(ADAPTER, &state.tools)?;
    let mut events = Vec::new();
    let lifecycle = if result.events.is_empty() {
        state.lifecycle.clone()
    } else {
        lifecycle::step_start(&state.lifecycle, &mut events)
    };
    events.extend(result.events);
    // Close any blocks whose stop event never arrived, in ascending index
    // order (TS numeric-key iteration order).
    let mut signature_indexes: Vec<(u64, String)> = state
        .reasoning_signatures
        .keys()
        .filter_map(|key| key.parse::<u64>().ok().map(|index| (index, key.clone())))
        .collect();
    signature_indexes.sort();
    let mut lifecycle = lifecycle;
    for (_, key) in &signature_indexes {
        let signature = &state.reasoning_signatures[key];
        lifecycle = lifecycle::reasoning_end(
            &lifecycle,
            &mut events,
            &format!("reasoning-{key}"),
            Some(provider_metadata(&state.provider_metadata_key, json!({"signature": signature}))),
            None,
        );
    }
    let finish_input = lifecycle::FinishInput {
        reason: state.pending_finish.as_ref().map_or_else(
            || FinishReasonDetails::new(FinishReason::Unknown),
            |finish| finish.reason.clone(),
        ),
        usage: state.usage.clone(),
        provider_metadata: state
            .pending_finish
            .as_ref()
            .and_then(|finish| finish.provider_metadata.clone()),
    };
    let lifecycle = lifecycle::finish(&lifecycle, &mut events, finish_input);
    Ok((
        ParserState {
            lifecycle,
            tools: result.tools,
            open_text: BTreeSet::new(),
            open_reasoning: BTreeSet::new(),
            ..state.clone()
        },
        events,
    ))
}

/// Prefix `error.type` so overloads, rate limits, and quota errors are
/// visible even when the provider message is generic or empty.
fn stream_error_message(event: &Value) -> String {
    let error = event.get("error");
    let error_type = error.and_then(|error| error.get("type")).and_then(Value::as_str);
    let message = error.and_then(|error| error.get("message")).and_then(Value::as_str);
    match (error_type, message) {
        (Some(error_type), Some(message)) if !message.is_empty() => {
            format!("{error_type}: {message}")
        }
        _ => message
            .filter(|message| !message.is_empty())
            .or(error_type)
            .unwrap_or("Anthropic Messages stream error")
            .to_string(),
    }
}

fn on_error(event: &Value) -> AiError {
    let message = stream_error_message(event);
    let body = encode_json(event);
    AiError::new(classify_provider_failure(ProviderFailure {
        message: &message,
        status: None,
        raw_body: Some(&body),
        data: None,
        http: None,
        retry_after_ms: None,
        rate_limit: None,
    }))
}

/// The stream block types the parser recognizes.
const STREAM_BLOCK_TYPES: [&str; 6] =
    ["compaction", "text", "thinking", "redacted_thinking", "tool_use", "server_tool_use"];
const STREAM_DELTA_TYPES: [&str; 5] =
    ["compaction_delta", "text_delta", "thinking_delta", "signature_delta", "input_json_delta"];

fn is_server_tool_result_type(block_type: &str) -> bool {
    matches!(
        block_type,
        "web_search_tool_result" | "code_execution_tool_result" | "web_fetch_tool_result"
    )
}

/// One step of the parser state machine (port of `step`): translate one
/// decoded frame into emitted events plus the next state.
pub fn step(state: ParserState, frame: &str) -> Result<(ParserState, Vec<LLMEvent>), AiError> {
    let event = decode_event(frame)?;
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
    if !SSE_EVENT_NAMES.contains(&event_type.as_str()) {
        return Ok((state, Vec::new()));
    }
    if event_type != "content_block_start" {
        if let Some(content_block) = event.get("content_block") {
            if check_stream_block(content_block).is_err() {
                return Err(invalid_stream_event(&event));
            }
        }
    }
    if event_type != "content_block_delta" {
        if let Some(delta) = event.get("delta") {
            if check_stream_delta(delta).is_err() {
                return Err(invalid_stream_event(&event));
            }
        }
    }
    match event_type.as_str() {
        "message_start" => Ok(on_message_start(&state, &event)),
        "content_block_start" => {
            let Some(content_block) = event.get("content_block") else {
                return Err(invalid_stream_event(&event));
            };
            if !content_block.is_object() {
                return Err(invalid_stream_event(&event));
            }
            let block_type = content_block.get("type").and_then(Value::as_str).unwrap_or_default();
            if block_type == "compaction" {
                // {type: "compaction", content: string | null} — content is
                // required.
                match content_block.get("content") {
                    Some(Value::String(_) | Value::Null) => {}
                    _ => return Err(invalid_stream_event(&event)),
                }
                let Some(index) = stream_index(&event) else {
                    return Err(invalid_stream_event(&event));
                };
                let mut compactions = state.compactions.clone();
                compactions.insert(
                    index_key(index),
                    content_block.get("content").and_then(Value::as_str).map(str::to_string),
                );
                return Ok((ParserState { compactions, ..state.clone() }, Vec::new()));
            }
            if !STREAM_BLOCK_TYPES.contains(&block_type) && !is_server_tool_result_type(block_type)
            {
                return Ok((state, Vec::new()));
            }
            if check_stream_block(content_block).is_err() {
                return Err(invalid_stream_event(&event));
            }
            if block_type == "tool_use" || block_type == "server_tool_use" {
                let Some(index) = stream_index(&event) else {
                    return Err(event_error(
                        ADAPTER,
                        format!("Anthropic {block_type} missing index"),
                    ));
                };
                if content_block.get("id").and_then(Value::as_str).is_none_or(str::is_empty) {
                    return Err(event_error(
                        ADAPTER,
                        format!("Anthropic tool_use missing id at index {index}"),
                    ));
                }
            }
            Ok(on_content_block_start(&state, &event))
        }
        "content_block_delta" => {
            let Some(delta) = event.get("delta") else {
                return Err(invalid_stream_event(&event));
            };
            if !delta.is_object() {
                return Err(invalid_stream_event(&event));
            }
            if let Some(delta_type) = delta.get("type").and_then(Value::as_str) {
                if !STREAM_DELTA_TYPES.contains(&delta_type) {
                    return Ok((state, Vec::new()));
                }
            }
            if check_stream_delta(delta).is_err() {
                return Err(invalid_stream_event(&event));
            }
            on_content_block_delta(&state, &event)
        }
        "content_block_stop" => on_content_block_stop(&state, &event),
        "message_delta" => {
            let Some(delta) = event.get("delta") else {
                return Err(invalid_stream_event(&event));
            };
            if check_stream_delta(delta).is_err() {
                return Err(invalid_stream_event(&event));
            }
            Ok(on_message_delta(&state, &event))
        }
        "message_stop" => on_message_stop(&state),
        "error" => Err(on_error(&event)),
        _ => Ok((state, Vec::new())),
    }
}

// =============================================================================
// Protocol + route
// =============================================================================

/// The Anthropic Messages protocol (port of `AnthropicMessages.protocol`).
pub struct AnthropicMessages;

impl Protocol for AnthropicMessages {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        body_from_request(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        Box::new(ParserState {
            provider: request.model.provider.clone(),
            compactions: BTreeMap::new(),
            provider_metadata_key: request
                .model
                .route
                .provider_metadata_key
                .clone()
                .unwrap_or_else(|| request.model.provider.0.clone()),
            tools: tool_stream::State::new(),
            reasoning_signatures: BTreeMap::new(),
            usage: None,
            pending_finish: None,
            lifecycle: lifecycle::State::initial(),
            open_text: BTreeSet::new(),
            open_reasoning: BTreeSet::new(),
        })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state = *state.downcast::<ParserState>().expect("AnthropicMessages parser state");
        let (state, events) = step(state, frame)?;
        Ok((Box::new(state), events))
    }

    fn supports_effort_updates(&self, request: &LLMRequest) -> bool {
        supports_effort_updates(&request.model)
    }
}

/// Auth pattern of the TS Anthropic provider facade: explicit auth wins, then
/// the optional `apiKey`, then `ANTHROPIC_API_KEY`, rendered as `x-api-key`.
#[must_use]
pub fn anthropic_provider_auth(
    api_key: Option<Secret>,
    auth: Option<AuthDefinition>,
) -> AuthDefinition {
    if let Some(auth) = auth {
        return auth;
    }
    auth::or_else(&auth::optional(api_key), &auth::config("ANTHROPIC_API_KEY")).header("x-api-key")
}

/// Facade-level `apiKey`-vs-`authToken` mutual exclusion (port of
/// `providers/anthropic.ts model()`): `authToken` renders as bearer auth.
pub fn anthropic_route_auth(
    api_key: Option<Secret>,
    auth_token: Option<Secret>,
) -> Result<AuthDefinition, AiError> {
    if api_key.is_some() && auth_token.is_some() {
        return Err(AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new("anthropic"),
            message: "Anthropic apiKey cannot be combined with authToken".into(),
        }));
    }
    Ok(match auth_token {
        Some(token) => auth::Definition::bearer(token),
        None => anthropic_provider_auth(api_key, None),
    })
}

/// The Anthropic Messages route (port of `AnthropicMessages.route` + the TS
/// transport wrapper's beta-header injection at anthropic-messages.ts:1613).
///
/// Route headers: `anthropic-version` always, `anthropic-beta` merged from
/// the caller's header plus the betas the request requires.
#[must_use]
pub fn anthropic_messages_route(transport: SharedTransport) -> Route {
    Route::new(RouteInput {
        id: ADAPTER.into(),
        provider: Some("anthropic".into()),
        provider_metadata_key: Some("anthropic".into()),
        protocol: Arc::new(AnthropicMessages),
        endpoint: Endpoint::dynamic_path(|input: &crate::route::endpoint::EndpointInput<'_>| {
            if input.request.model.provider.0 == "anthropic" {
                format!("{PATH}?beta=true")
            } else {
                PATH.to_string()
            }
        })
        .with_base_url(DEFAULT_BASE_URL),
        auth: None,
        framing: framing(),
        headers: Some(Arc::new(|_request: &LLMRequest| {
            BTreeMap::from([("anthropic-version".to_string(), "2023-06-01".to_string())])
        })),
        transport: BetaHeadersTransport::wrap(transport),
        defaults: Default::default(),
    })
}

/// Convenience: the default route patched like the Anthropic provider
/// facade's `configure()` (x-api-key auth over the optional key or env).
#[must_use]
pub fn anthropic_route(transport: SharedTransport, api_key: Option<Secret>) -> Route {
    anthropic_messages_route(transport).with(RoutePatch {
        auth: Some(anthropic_provider_auth(api_key, None)),
        ..Default::default()
    })
}

// =============================================================================
// Tests: anthropic_messages/tests.rs (ported from
// packages/ai/test/provider/anthropic-messages.test.ts)
// =============================================================================

#[cfg(test)]
mod tests;
