//! Shared protocol helpers (port of `src/protocols/shared.ts` + selected
//! pieces of `src/protocols/utils/`).

//! Shared protocol helpers (port of `src/protocols/utils/` + selected
//! pieces of `src/protocols/shared.ts`).

use serde_json::Value;

use crate::schema::{
    AiError, AiErrorReason, CachePolicy, LLMRequest, ProviderId, ProviderOptions, ReasonFields,
    RouteId,
};

/// `OpenAI` limits `prompt_cache_key` to 64 chars; `DeepSeek` and Zai inherit
/// the same limit via their OpenAI-compatible APIs.
pub const OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH: usize = 64;

/// Encode a JSON value compactly (port of `encodeJson`).
pub fn encode_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| panic!("encodeJson: {error}"))
}

/// Decode a JSON string, returning the parse error message on failure
/// (port of `decodeJson`).
pub fn decode_json(input: &str) -> Result<Value, String> {
    serde_json::from_str(input).map_err(|error| error.to_string())
}

/// Lenient decoders over `serde_json::Value` mirroring the TS schema
/// helpers (`optionalArray` / `optionalNull` / `lenient`): malformed shapes
/// yield `None` instead of failing the enclosing struct.
pub mod lenient {
    use serde_json::Value;

    /// `Schema.optional(Schema.Array(schema))`: the value when it is an
    /// array, `None` otherwise.
    #[must_use]
    pub fn optional_array(value: Option<&Value>) -> Option<&Vec<Value>> {
        value.and_then(|value| value.as_array())
    }

    /// `Schema.optional(Schema.NullOr(schema))`: `None` for JSON null and
    /// absent fields, the value otherwise.
    #[must_use]
    pub fn optional_null(value: Option<&Value>) -> Option<&Value> {
        value.filter(|value| !value.is_null())
    }

    /// Optional field whose malformed value decodes to `None` instead of
    /// failing the enclosing struct (port of `lenient`).
    pub fn lenient<T>(
        value: Option<&Value>,
        decode: impl FnOnce(&Value) -> Option<T>,
    ) -> Option<T> {
        value.and_then(decode)
    }

    /// Provider-defined string enum: known values for autocomplete, any
    /// string accepted at runtime (port of `knownString`).
    pub fn known_string<const KNOWN: usize>(
        value: Option<&Value>,
        _known: [&str; KNOWN],
    ) -> Option<String> {
        value.and_then(|value| value.as_str()).map(str::to_string)
    }
}

/// Clamp `prompt_cacheKey` to 64 chars, unicode-aware (port of
/// `ProviderShared.promptCacheKey`); `None` when caching is off or no key
/// was supplied.
#[must_use]
pub fn prompt_cache_key(request: &LLMRequest) -> Option<String> {
    if matches!(request.cache, Some(CachePolicy::None)) {
        return None;
    }
    let key = request.prompt_cache_key.as_ref()?;
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH {
        return Some(key.clone());
    }
    Some(chars[..OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH].iter().collect())
}

/// `Usage.totalTokens` policy shared by every route (port of
/// `totalTokens`): honor the provider total; otherwise fall back to
/// `input + output` only when at least one is defined.
#[must_use]
pub fn total_tokens(
    input_tokens: Option<f64>,
    output_tokens: Option<f64>,
    total: Option<f64>,
) -> Option<f64> {
    if total.is_some() {
        return total;
    }
    if input_tokens.is_none() && output_tokens.is_none() {
        return None;
    }
    Some(input_tokens.unwrap_or(0.0) + output_tokens.unwrap_or(0.0))
}

/// Subtract `subtrahend` from `total`, clamping to zero against
/// non-sensical provider breakdowns (port of `subtractTokens`). `None`
/// inputs pass through without fabricating counts.
#[must_use]
pub fn subtract_tokens(total: Option<f64>, subtrahend: Option<f64>) -> Option<f64> {
    let total = total?;
    let Some(subtrahend) = subtrahend else { return Some(total) };
    Some((total - subtrahend).max(0.0))
}

/// Sum optional token counts, `None` only when every input is `None` (port
/// of `sumTokens`).
pub fn sum_tokens(values: &[Option<f64>]) -> Option<f64> {
    if values.iter().all(Option::is_none) {
        return None;
    }
    Some(values.iter().flatten().sum())
}

/// Derive the **inclusive** `inputTokens` from a provider that natively
/// reports the non-overlapping breakdown (Anthropic, Bedrock): non-cached +
/// cache-read + cache-write. Direction semantics per oracle gate 2,
/// finding 17 — mappers SUM UP here.
#[must_use]
pub fn sum_non_cached_to_inclusive(
    non_cached_input_tokens: Option<f64>,
    cache_read_input_tokens: Option<f64>,
    cache_write_input_tokens: Option<f64>,
) -> Option<f64> {
    sum_tokens(&[non_cached_input_tokens, cache_read_input_tokens, cache_write_input_tokens])
}

/// Derive `nonCachedInputTokens` from a provider that reports the
/// **inclusive** input total (`OpenAI`, Gemini): input − cacheRead −
/// cacheWrite, clamped at zero via `max(0, …)` (oracle gate 2, finding 17 —
/// mappers SUBTRACT DOWN here).
#[must_use]
pub fn subtract_to_non_cached(
    input_tokens: Option<f64>,
    cache_read_input_tokens: Option<f64>,
    cache_write_input_tokens: Option<f64>,
) -> Option<f64> {
    let once = subtract_tokens(input_tokens, cache_read_input_tokens);
    subtract_tokens(once, cache_write_input_tokens)
}

/// Cap an explicit thinking budget at half the output limit (port of
/// `fitThinkingBudget`).
#[must_use]
pub fn fit_thinking_budget(budget: f64, max_tokens: Option<f64>, minimum: f64) -> f64 {
    match max_tokens {
        None => budget,
        Some(max_tokens) if budget <= max_tokens / 2.0 => budget,
        Some(max_tokens) => (minimum).max((max_tokens / 2.0).floor()),
    }
}

/// Serialize a JS-style number into a JSON value: finite integral floats
/// become JSON integers so wire bytes match the TS encoder (`32000` stays
/// `32000`, never `32000.0` — Go/strict-uint providers reject the float
/// form); genuinely fractional values stay floats. Values at or beyond
/// 2^53 lose no precision in JS and keep float form here.
#[must_use]
pub fn number(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        Value::from(value as i64)
    } else {
        // (cross-lane unblock: qualified path — the bare `json!` was
        // missing its import and broke every downstream crate's build)
        serde_json::json!(value)
    }
}

/// Typed invalid-stream-event error (port of `eventError`).
pub fn event_error(route: impl Into<String>, message: impl Into<String>) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields::new(message),
        classification: None,
        route: Some(route.into()),
    })
}

/// Parse JSON, mapping failures to a typed event error (port of `parseJson`).
pub fn parse_json(route: &str, input: &str, message: &str) -> Result<Value, AiError> {
    decode_json(input).map_err(|cause| {
        let mut error = event_error(route, message);
        if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
            fields.body = Some(input.to_string());
            fields.cause = Some(cause);
        }
        error
    })
}

/// Canonical invalid-request constructor (port of `invalidRequest`).
pub fn invalid_request(message: impl Into<String>) -> AiError {
    AiError::invalid_request(message)
}

/// Canonical constructor for operations the selected route does not
/// implement (port of `unsupportedOperation`).
pub fn unsupported_operation(
    operation: impl Into<String>,
    message: impl Into<String>,
    provider: Option<ProviderId>,
    route: Option<RouteId>,
) -> AiError {
    AiError::new(AiErrorReason::UnsupportedOperation {
        fields: ReasonFields::new(message),
        operation: operation.into(),
        provider,
        route,
    })
}

/// Join the `text` field of parts with newlines (port of `joinText`).
pub fn join_text<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let items: Vec<&str> = parts.into_iter().collect();
    items.join("\n")
}

/// Stable fallback for chronological system updates on routes without the
/// privileged role (port of `wrapSystemUpdate`): visible lower-authority
/// user text, XML-escaped so it cannot close the wrapper.
pub fn wrap_system_update<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let escaped = join_text(parts).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!("<system-update>\n{escaped}\n</system-update>")
}

/// Parse the streamed JSON input of a tool call; an empty string is `"{}"`
/// (port of `parseToolInput`).
pub fn parse_tool_input(route: &str, name: &str, raw: &str) -> Result<Value, AiError> {
    parse_json(
        route,
        if raw.is_empty() { "{}" } else { raw },
        &format!("Invalid JSON input for {route} tool call {name}"),
    )
}

/// Render an unknown error into text (port of `errorText`).
pub fn error_text(error: &dyn std::error::Error) -> String {
    error.to_string()
}

/// Merge provider options (re-export of the options-layer helper).
#[must_use]
pub fn merge_provider_options(items: &[Option<&ProviderOptions>]) -> Option<ProviderOptions> {
    crate::schema::options::merge_provider_options(items)
}

/// Join a namespace path onto leaf tool names for protocols without a
/// native namespace construct (port of `flattenTools`); later duplicates of
/// a full name win.
#[must_use]
pub fn flatten_tools(tools: &[crate::schema::ToolEntry]) -> Vec<crate::schema::ToolDefinition> {
    fn flatten(
        tools: &[crate::schema::ToolEntry],
        path: &[&str],
        out: &mut Vec<crate::schema::ToolDefinition>,
    ) {
        for tool in tools {
            match tool {
                crate::schema::ToolEntry::Namespace(namespace) => {
                    let mut next_path = path.to_vec();
                    next_path.push(&namespace.name);
                    flatten(&namespace.tools, &next_path, out);
                }
                crate::schema::ToolEntry::Definition(definition) => {
                    if path.is_empty() {
                        out.push(definition.clone());
                    } else {
                        let mut name_parts = path.to_vec();
                        name_parts.push(&definition.name);
                        out.push(crate::schema::ToolDefinition {
                            name: name_parts.join("_"),
                            ..definition.clone()
                        });
                    }
                }
            }
        }
    }
    let mut flat = Vec::new();
    flatten(tools, &[], &mut flat);
    let mut by_name = std::collections::BTreeMap::new();
    for tool in flat {
        by_name.insert(tool.name.clone(), tool);
    }
    by_name.into_values().collect()
}

/// Base64 encode without an external dependency (standard alphabet,
/// padded) — used by inline media views and cassette helpers.
#[must_use]
pub fn encode_base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 { ALPHABET[triple as usize & 0x3f] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtract_tokens_clamps_non_sensical_breakdowns_to_zero() {
        assert_eq!(subtract_tokens(Some(5.0), Some(3.0)), Some(2.0));
        assert_eq!(subtract_tokens(Some(5.0), Some(10.0)), Some(0.0));
        assert_eq!(subtract_tokens(Some(5.0), None), Some(5.0));
        assert_eq!(subtract_tokens(None, Some(3.0)), None);
        assert_eq!(subtract_tokens(None, None), None);
    }

    #[test]
    fn sum_tokens_returns_none_only_when_every_input_is_none() {
        assert_eq!(sum_tokens(&[Some(1.0), Some(2.0), Some(3.0)]), Some(6.0));
        assert_eq!(sum_tokens(&[Some(1.0), None, Some(3.0)]), Some(4.0));
        assert_eq!(sum_tokens(&[None, None, None]), None);
        assert_eq!(sum_tokens(&[]), None);
    }

    #[test]
    fn direction_helpers_sum_up_and_subtract_down() {
        // Anthropic direction: breakdown → inclusive
        assert_eq!(sum_non_cached_to_inclusive(Some(40.0), Some(60.0), Some(0.0)), Some(100.0));
        assert_eq!(sum_non_cached_to_inclusive(None, None, None), None);
        // OpenAI direction: inclusive → breakdown, clamped at zero
        assert_eq!(subtract_to_non_cached(Some(100.0), Some(60.0), Some(0.0)), Some(40.0));
        assert_eq!(subtract_to_non_cached(Some(50.0), Some(60.0), Some(0.0)), Some(0.0));
        assert_eq!(subtract_to_non_cached(None, Some(60.0), None), None);
    }

    #[test]
    fn total_tokens_honors_provider_total_or_sums() {
        assert_eq!(total_tokens(Some(10.0), Some(5.0), Some(99.0)), Some(99.0));
        assert_eq!(total_tokens(Some(10.0), Some(5.0), None), Some(15.0));
        assert_eq!(total_tokens(Some(10.0), None, None), Some(10.0));
        assert_eq!(total_tokens(None, None, None), None);
    }

    #[test]
    fn prompt_cache_key_clamps_to_64_unicode_chars() {
        let request = crate::test_support::request_for_tests(vec![]);
        let short = request.update(crate::schema::LLMRequestPatch {
            prompt_cache_key: Some("session-123".into()),
            ..Default::default()
        });
        assert_eq!(prompt_cache_key(&short).as_deref(), Some("session-123"));
        let long_key = "é".repeat(70);
        let long = request.update(crate::schema::LLMRequestPatch {
            prompt_cache_key: Some(long_key),
            ..Default::default()
        });
        assert_eq!(
            prompt_cache_key(&long).map(|key| key.chars().count()),
            Some(OPENAI_PROMPT_CACHE_KEY_MAX_LENGTH)
        );
        // cache: none opts out
        let off = long.update(crate::schema::LLMRequestPatch {
            cache: Some(CachePolicy::None),
            ..Default::default()
        });
        assert_eq!(prompt_cache_key(&off), None);
    }

    #[test]
    fn wrap_system_update_escapes_markup() {
        assert_eq!(
            wrap_system_update(["a < b", "c & d"]),
            "<system-update>\na &lt; b\nc &amp; d\n</system-update>"
        );
    }

    #[test]
    fn parse_tool_input_treats_empty_input_as_empty_object() {
        assert_eq!(parse_tool_input("route", "tool", "").unwrap(), serde_json::json!({}));
        let error = parse_tool_input("route", "tool", "{oops").unwrap_err();
        assert!(error.message().contains("Invalid JSON input for route tool call tool"));
    }

    #[test]
    fn flatten_tools_joins_namespace_paths_with_underscores() {
        let tools = vec![
            crate::schema::ToolEntry::Definition(crate::schema::ToolDefinition::new(
                "bash",
                "Run a command",
                serde_json::json!({"type": "object"}),
            )),
            crate::schema::ToolEntry::Namespace(crate::schema::ToolNamespace {
                name: "search".into(),
                description: None,
                tools: vec![crate::schema::ToolEntry::Definition(
                    crate::schema::ToolDefinition::new(
                        "web",
                        "Web search",
                        serde_json::json!({"type": "object"}),
                    ),
                )],
            }),
        ];
        let flat = flatten_tools(&tools);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[1].name, "search_web");
    }

    #[test]
    fn fit_thinking_budget_caps_at_half_the_output_limit() {
        assert_eq!(fit_thinking_budget(10_000.0, None, 1.0), 10_000.0);
        assert_eq!(fit_thinking_budget(10_000.0, Some(30_000.0), 1.0), 10_000.0);
        assert_eq!(fit_thinking_budget(20_000.0, Some(30_000.0), 1.0), 15_000.0);
        assert_eq!(fit_thinking_budget(-1.0, Some(30_000.0), 1.0), -1.0);
    }

    #[test]
    fn encode_base64_matches_standard_alphabet() {
        assert_eq!(encode_base64(b"\x01\x02\x03"), "AQID");
        assert_eq!(encode_base64(b"ABC"), "QUJD");
        assert_eq!(encode_base64(b"A"), "QQ==");
        assert_eq!(encode_base64(b"AB"), "QUI=");
        assert_eq!(encode_base64(b""), "");
    }

    #[test]
    fn lenient_helpers_mirror_their_ts_schemas() {
        let value = serde_json::json!({"a": [1, 2], "b": null, "c": "x"});
        assert_eq!(lenient::optional_array(value.get("a")).map(Vec::len), Some(2));
        assert_eq!(lenient::optional_array(value.get("c")), None);
        assert_eq!(lenient::optional_null(value.get("b")), None);
        assert_eq!(lenient::optional_null(value.get("c")).and_then(|v| v.as_str()), Some("x"));
        assert_eq!(lenient::known_string(value.get("c"), ["known"]), Some("x".to_string()));
    }
}
