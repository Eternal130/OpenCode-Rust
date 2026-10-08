//! Open Responses provider options (port of
//! `src/protocols/utils/open-responses-options.ts`).
//!
//! Every field decodes **leniently** and independently (port of the `lenient`
//! schema helper): a malformed `topLogprobs` is dropped without discarding
//! `store` or `reasoningEffort`. Several fields are open string enums
//! (`TextVerbosity`, `ServiceTier`, `ResponseIncludable`, `ReasoningEffort`):
//! known values are documented, any string is accepted at runtime.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::schema::LLMRequest;

/// Known text verbosity values; providers may send any string.
pub const TEXT_VERBOSITIES: [&str; 3] = ["low", "medium", "high"];
/// Known response includable values; providers may send any string.
pub const RESPONSE_INCLUDABLES: [&str; 8] = [
    "file_search_call.results",
    "web_search_call.results",
    "web_search_call.action.sources",
    "message.input_image.image_url",
    "computer_call_output.output.image_url",
    "code_interpreter_call.outputs",
    "reasoning.encrypted_content",
    "message.output_text.logprobs",
];
/// Known service tier values; providers may send any string.
pub const SERVICE_TIERS: [&str; 4] = ["auto", "default", "flex", "priority"];
/// Known truncation values (closed enum on the wire).
pub const TRUNCATIONS: [&str; 2] = ["auto", "disabled"];
/// Known reasoning summary values (closed enum on the wire).
pub const REASONING_SUMMARIES: [&str; 3] = ["auto", "concise", "detailed"];

/// `stream_options` body field.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamOptions {
    pub include_obfuscation: Option<bool>,
}

/// `allowedTools` provider option.
#[derive(Debug, Clone, PartialEq)]
pub struct AllowedTools {
    pub tool_names: Vec<String>,
    pub mode: String,
}

/// Leniently resolved provider options (port of `OpenResponsesOptions.Resolved`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resolved {
    pub store: Option<bool>,
    pub metadata: Option<BTreeMap<String, String>>,
    pub safety_identifier: Option<String>,
    pub stream_options: Option<StreamOptions>,
    pub top_logprobs: Option<i64>,
    pub reasoning_effort: Option<String>,
    pub reasoning_summary: Option<String>,
    pub include: Option<Vec<String>>,
    pub text_verbosity: Option<String>,
    pub service_tier: Option<String>,
    pub truncation: Option<String>,
    pub allowed_tools: Option<AllowedTools>,
    pub max_tool_calls: Option<i64>,
    pub parallel_tool_calls: Option<bool>,
}

fn as_bool(value: Option<&Value>) -> Option<bool> {
    value.and_then(Value::as_bool)
}

fn as_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// Integral check without float equality (clippy): a number is integral
/// when truncation is lossless, i.e. `trunc(x) == x` via subtraction.
fn is_integral(number: f64) -> bool {
    number.trunc() - number == 0.0
}

fn as_int_in(value: Option<&Value>, range: std::ops::RangeInclusive<i64>) -> Option<i64> {
    let number = value.and_then(Value::as_f64)?;
    if !is_integral(number) || number > i64::MAX as f64 || number < i64::MIN as f64 {
        return None;
    }
    let truncated = number as i64;
    range.contains(&truncated).then_some(truncated)
}

fn as_int(value: Option<&Value>) -> Option<i64> {
    let number = value.and_then(Value::as_f64)?;
    if !is_integral(number) || number > i64::MAX as f64 || number < i64::MIN as f64 {
        return None;
    }
    Some(number as i64)
}

fn as_string_array(value: Option<&Value>) -> Option<Vec<String>> {
    value?.as_array()?.iter().map(|item| item.as_str().map(str::to_string)).collect()
}

fn as_one_of(value: Option<&Value>, known: &[&str]) -> Option<String> {
    let text = value.and_then(Value::as_str)?;
    known.contains(&text).then(|| text.to_string())
}

fn as_string_record(value: Option<&Value>) -> Option<BTreeMap<String, String>> {
    let object = value?.as_object()?;
    object
        .iter()
        .map(|(key, entry)| entry.as_str().map(|text| (key.clone(), text.to_string())))
        .collect()
}

/// Resolve the per-request options, dropping malformed fields one at a time
/// (port of `OpenResponsesOptions.resolve`).
#[must_use]
pub fn resolve(request: &LLMRequest) -> Resolved {
    let empty = serde_json::Map::new();
    let options = request.provider_options.as_ref().unwrap_or(&empty);

    let stream_options = options.get("streamOptions").and_then(|value| {
        let object = value.as_object()?;
        Some(StreamOptions { include_obfuscation: as_bool(object.get("includeObfuscation")) })
    });

    let allowed_tools = options.get("allowedTools").and_then(|value| {
        let object = value.as_object()?;
        let tool_names = as_string_array(object.get("toolNames"))?;
        if tool_names.is_empty() {
            return None;
        }
        let mode = object
            .get("mode")
            .and_then(|mode| as_one_of(Some(mode), &["auto", "none", "required"]))
            .unwrap_or_else(|| "auto".to_string());
        Some(AllowedTools { tool_names, mode })
    });

    Resolved {
        store: as_bool(options.get("store")),
        metadata: as_string_record(options.get("metadata")),
        safety_identifier: as_string(options.get("safetyIdentifier")),
        stream_options,
        top_logprobs: as_int_in(options.get("topLogprobs"), 0..=20),
        reasoning_effort: as_string(options.get("reasoningEffort")),
        reasoning_summary: as_one_of(options.get("reasoningSummary"), &REASONING_SUMMARIES),
        include: as_string_array(options.get("include")).filter(|items| !items.is_empty()),
        text_verbosity: as_string(options.get("textVerbosity")),
        service_tier: as_string(options.get("serviceTier")),
        truncation: as_one_of(options.get("truncation"), &TRUNCATIONS),
        allowed_tools,
        max_tool_calls: as_int(options.get("maxToolCalls")),
        parallel_tool_calls: as_bool(options.get("parallelToolCalls")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::LLMRequestPatch;
    use crate::test_support::request_for_tests;
    use serde_json::json;

    fn request_with_options(options: serde_json::Map<String, Value>) -> LLMRequest {
        request_for_tests(vec![])
            .update(LLMRequestPatch { provider_options: Some(options), ..Default::default() })
    }

    #[test]
    fn open_responses_options_resolves_valid_fields() {
        let options: serde_json::Map<String, Value> = [
            ("store", json!(false)),
            ("metadata", json!({"user": "abc"})),
            ("safetyIdentifier", json!("user-1")),
            ("streamOptions", json!({"includeObfuscation": true})),
            ("topLogprobs", json!(5)),
            ("reasoningEffort", json!("experimental")),
            ("reasoningSummary", json!("detailed")),
            ("include", json!(["reasoning.encrypted_content"])),
            ("textVerbosity", json!("verbose")),
            ("serviceTier", json!("priority")),
            ("truncation", json!("auto")),
            ("allowedTools", json!({"toolNames": ["read"], "mode": "required"})),
            ("maxToolCalls", json!(3)),
            ("parallelToolCalls", json!(true)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let resolved = resolve(&request_with_options(options));
        assert_eq!(resolved.store, Some(false));
        assert_eq!(resolved.metadata.as_ref().unwrap()["user"], "abc");
        assert_eq!(resolved.safety_identifier.as_deref(), Some("user-1"));
        assert_eq!(resolved.stream_options.unwrap().include_obfuscation, Some(true));
        assert_eq!(resolved.top_logprobs, Some(5));
        assert_eq!(resolved.reasoning_effort.as_deref(), Some("experimental"));
        assert_eq!(resolved.reasoning_summary.as_deref(), Some("detailed"));
        assert_eq!(
            resolved.include.as_deref(),
            Some(&["reasoning.encrypted_content".to_string()][..])
        );
        assert_eq!(resolved.text_verbosity.as_deref(), Some("verbose"));
        assert_eq!(resolved.service_tier.as_deref(), Some("priority"));
        assert_eq!(resolved.truncation.as_deref(), Some("auto"));
        assert_eq!(resolved.allowed_tools.unwrap().mode, "required");
        assert_eq!(resolved.max_tool_calls, Some(3));
        assert_eq!(resolved.parallel_tool_calls, Some(true));
    }

    #[test]
    fn open_responses_options_drops_malformed_fields_independently() {
        let options: serde_json::Map<String, Value> = [
            ("store", json!("yes")),              // not a boolean
            ("topLogprobs", json!(21)),           // out of range
            ("reasoningSummary", json!("ultra")), // not a known summary
            ("truncation", json!("maybe")),       // not a known truncation
            ("include", json!([])),               // empty array → undefined
            ("allowedTools", json!({"toolNames": []})),
            ("maxToolCalls", json!(1.5)), // not an integer
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let resolved = resolve(&request_with_options(options));
        assert_eq!(resolved.store, None);
        assert_eq!(resolved.top_logprobs, None);
        assert_eq!(resolved.reasoning_summary, None);
        assert_eq!(resolved.truncation, None);
        assert_eq!(resolved.include, None);
        assert_eq!(resolved.allowed_tools, None);
        assert_eq!(resolved.max_tool_calls, None);
    }

    #[test]
    fn open_responses_options_defaults_allowed_tools_mode_to_auto() {
        let allowed = serde_json::json!({ "toolNames": ["read", "bash"] });
        let options: serde_json::Map<String, Value> =
            [("allowedTools", allowed)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let resolved = resolve(&request_with_options(options));
        let allowed = resolved.allowed_tools.unwrap();
        assert_eq!(allowed.mode, "auto");
        assert_eq!(allowed.tool_names.len(), 2);
    }
}
