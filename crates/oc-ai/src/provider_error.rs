//! Provider failure classification (port of `src/provider-error.ts`) +
//! retry classification parity (oracle gate 2, finding 19). Retry decisions
//! downstream feed oc-util's `is_transient_message`/`next_delay`, so wrong
//! classification changes visible behavior — patterns below are ports, not
//! approximations.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::schema::{
    AiError, AiErrorReason, HttpContext, HttpRateLimitDetails, LLMEvent,
    ProviderFailureClassification, TransportDelivery, TransportOperation,
};

fn compiled(patterns: &[&'static str]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|pattern| Regex::new(pattern).expect("static classification pattern"))
        .collect()
}

fn context_overflow_patterns() -> &'static Vec<Regex> {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        compiled(&[
            r"(?i)prompt is too long",
            r"(?i)input is too long for requested model",
            // Cloudflare Workers AI reports this as HTTP 413.
            r"(?i)exceeded this model context window limit",
            r"(?i)exceeds the context window",
            r"(?i)exceeds (?:the )?(?:model'?s )?maximum context length(?: of [\d,]+ tokens?|\s*\([\d,]+\))",
            r"(?i)input token count.*exceeds the maximum",
            // Amazon Nova on Bedrock reports this as a mid-stream validationException.
            r"(?i)number of input tokens exceeds maximum length",
            r"(?i)tokens in request more than max tokens allowed",
            r"(?i)maximum prompt length is \d+",
            r"(?i)reduce the length of the messages",
            // DeepInfra
            r"(?i)requested input length \d+ exceeds maximum input length",
            r"(?i)maximum context length is \d+ tokens",
            r"(?i)exceeds (?:the )?maximum allowed input length of [\d,]+ tokens?",
            // Novita omits the token counts.
            r"(?i)input(?: \(\d+ tokens\))? is longer than the model'?s context length",
            r"(?i)exceeds the limit of \d+",
            r"(?i)exceeds the available context size",
            r"(?i)greater than the context length",
            // Hugging Face Text Generation Inference, e.g. Together
            r"(?i)`inputs` tokens \+ `max_new_tokens` must be <= \d+",
            r"(?i)context window exceeds limit",
            r"(?i)exceeded model token limit",
            r"(?i)context[_ ]length[_ ]exceeded",
            r"(?i)context length is only \d+ tokens",
            r"(?i)input length.*exceeds.*context length",
            // Z.ai code 1261 arrives as `Prompt too long` or `Prompt 超长`.
            r"(?i)prompt (?:too long|超长)",
            r"(?i)too large for model with \d+ maximum context length",
            r"(?i)prompt has [\d,]+ tokens?, but the configured context size is [\d,]+ tokens?",
            r"(?i)model_context_window_exceeded",
            r"(?i)range of input length should be",
            r"(?i)too many tokens",
            r"(?i)token limit exceeded",
            r"(?i)request_too_large",
            // Anthropic body-less 4xx rejections arrive as bare status text.
            r"(?i)^4(?:00|13)\s*(?:status code)?\s*\(no body\)",
        ])
    })
}

fn payload_patterns() -> &'static Vec<Regex> {
    static PAYLOAD: OnceLock<Vec<Regex>> = OnceLock::new();
    PAYLOAD.get_or_init(|| {
        compiled(&[
            r"(?i)request entity too large",
            r"(?i)payload too large",
            r"(?i)request too large",
        ])
    })
}

fn exclusion_patterns() -> &'static Vec<Regex> {
    static EXCLUSIONS: OnceLock<Vec<Regex>> = OnceLock::new();
    EXCLUSIONS.get_or_init(|| {
        compiled(&[
            r"(?i)^(throttling error|service unavailable):",
            r"(?i)rate limit",
            r"(?i)too many requests",
            // Cohere reports an output limit above the model maximum as "too
            // many tokens"; compaction cannot fix it.
            r"(?i)max[_ ]tokens must be less than",
        ])
    })
}

fn gateway_code_label() -> &'static Regex {
    // OpenCode Zen replaces upstream codes outside its allow-list but keeps
    // the original as a `[code]` label at the start of the rewritten message.
    static LABEL: OnceLock<Regex> = OnceLock::new();
    LABEL.get_or_init(|| Regex::new(r"^[^:\n]+: \[([A-Za-z0-9_.-]+)\]").unwrap())
}

fn auth_text() -> &'static Regex {
    // xAI reports an invalid API key as HTTP 400 with a generic code.
    static TEXT: OnceLock<Regex> = OnceLock::new();
    TEXT.get_or_init(|| Regex::new(r"(?i)incorrect api key provided").unwrap())
}

fn rate_limit_text() -> &'static Regex {
    static TEXT: OnceLock<Regex> = OnceLock::new();
    TEXT.get_or_init(|| {
        Regex::new(r"(?i)rate increased too quickly|rate[-_\s]?limit|too[_\s]?many[_\s]?requests")
            .unwrap()
    })
}

fn quota_text() -> &'static Regex {
    // Only consulted on 429, where throttles and account caps share a status.
    // Z.ai reports balance, plan expiry, plan limits, and plan model access
    // on 429.
    static TEXT: OnceLock<Regex> = OnceLock::new();
    TEXT.get_or_init(|| {
        Regex::new(r"(?i)insufficient[-_\s]?(?:quota|balance)|quota[-_\s]?exceeded|budget exceeded|usage limit|limit exhausted|package has expired|plan does not yet include").unwrap()
    })
}

fn content_policy_text() -> &'static Regex {
    // Policy rejections without a dedicated code, matched against the
    // provider's own explanation only.
    static TEXT: OnceLock<Regex> = OnceLock::new();
    TEXT.get_or_init(|| {
        Regex::new(r"(?i)violating our usage policy|blocked by content filtering policy|content[-_\s]?policy|rejected as a result of our safety system|detected potentially unsafe or sensitive content").unwrap()
    })
}

fn server_error_text() -> &'static Regex {
    static TEXT: OnceLock<Regex> = OnceLock::new();
    TEXT.get_or_init(|| {
        Regex::new(r"(?i)\b(?:try again|(?:please |you can )?retry (?:the |this |your )?request|try (?:the |this |your )?request again|(?:currently |temporarily )?at capacity|overloaded|temporarily unavailable|service[-_\s]?unavailable|(?:server|internal)[-_\s]?error|server (?:is )?busy|provider returned (?:an )?error|resource[-_\s]?exhausted|upstream (?:connect|connection|request)|request buffer limit while retrying upstream)\b").unwrap()
    })
}

fn quota_codes() -> &'static BTreeSet<&'static str> {
    static CODES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    CODES.get_or_init(|| {
        // OpenCode Zen reports account caps as typed 429/402 errors that are
        // not throttles.
        [
            "insufficient_quota",
            "usage_not_included",
            "billing_error",
            "gousagelimiterror",
            "freeusagelimiterror",
            "creditlimitexceeded",
        ]
        .into_iter()
        .collect()
    })
}

fn auth_codes() -> &'static BTreeSet<&'static str> {
    static CODES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    // Google reports an invalid API key as HTTP 400 INVALID_ARGUMENT with a
    // `details[].reason`. Z.ai's Responses API reports account and plan
    // rejections mid-stream as `permission_denied`.
    CODES.get_or_init(|| {
        ["authentication_error", "permission_error", "permission_denied", "api_key_invalid"]
            .into_iter()
            .collect()
    })
}

fn server_codes() -> &'static BTreeSet<&'static str> {
    static CODES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    CODES.get_or_init(|| {
        [
            "api_error",
            "internal_error",
            "internalserverexception",
            "modelstreamerrorexception",
            "overloaded_error",
            "server_error",
            "server_is_overloaded",
            "slow_down",
            "serviceunavailableexception",
        ]
        .into_iter()
        .collect()
    })
}

fn invalid_request_codes() -> &'static BTreeSet<&'static str> {
    static CODES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    // `invalid_request` is the Vercel AI Gateway's code for an upstream
    // request rejection.
    CODES.get_or_init(|| {
        [
            "model_not_found",
            "invalid_prompt",
            "invalid_request",
            "invalid_request_error",
            "validationexception",
        ]
        .into_iter()
        .collect()
    })
}

fn content_policy_codes() -> &'static BTreeSet<&'static str> {
    static CODES: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    // Azure OpenAI reports `content_filter` with `innererror.code`
    // ResponsibleAIPolicyViolation. OpenRouter tags provider failures with a
    // typed `error_type`.
    CODES.get_or_init(|| {
        [
            "content_filter",
            "content_moderation",
            "responsibleaipolicyviolation",
            "content_policy_violation",
            "image_content_policy_violation",
            "refusal",
        ]
        .into_iter()
        .collect()
    })
}

/// Whether a provider message reports a recoverable context overflow (port
/// of `isContextOverflow`).
#[must_use]
pub fn is_context_overflow(message: &str) -> bool {
    let excluded = exclusion_patterns().iter().any(|pattern| pattern.is_match(message));
    if excluded {
        return false;
    }
    context_overflow_patterns().iter().any(|pattern| pattern.is_match(message))
}

/// Whether a message reports a generic request-size failure (port of
/// `isPayloadTooLarge`).
#[must_use]
pub fn is_payload_too_large(message: &str) -> bool {
    payload_patterns().iter().any(|pattern| pattern.is_match(message))
}

/// Whether a typed failure already carries an affirmative context-overflow
/// classification (port of `isContextOverflowFailure`).
#[must_use]
pub fn is_context_overflow_failure(error: &AiError) -> bool {
    matches!(
        error.reason,
        AiErrorReason::InvalidRequest {
            classification: Some(ProviderFailureClassification::ContextOverflow),
            ..
        }
    )
}

/// Whether a failed call may succeed when sent again (port of `isRetryable`):
/// rate limits, provider-side failures, transport failures that did not
/// deliver an accepted write, and unrecognized failures. Callers decide which
/// calls are safe to repeat.
pub fn is_retryable(error: &AiError) -> bool {
    if let Some(HttpContext { headers, .. }) = error.reason.http() {
        match headers.get("x-should-retry").map(String::as_str) {
            Some("true") => return true,
            Some("false") => return false,
            _ => {}
        }
    }
    match &error.reason {
        AiErrorReason::RateLimit { .. } | AiErrorReason::ProviderInternal { .. } => true,
        // A WebSocket acknowledgment marks delivery accepted before model
        // output may exist. Read failures can still recover; the caller
        // chooses retry versus continuation from durable output.
        AiErrorReason::Transport { delivery, operation, .. } => {
            delivery != &Some(TransportDelivery::Rejected)
                && (delivery != &Some(TransportDelivery::Accepted)
                    || operation == &TransportOperation::Read)
        }
        AiErrorReason::InvalidProviderOutput { classification, .. } => {
            classification.as_deref() == Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM)
        }
        // Unrecognized failures retry: classification records affirmative
        // deterministic evidence, and transient failures are exactly the ones
        // that arrive in shapes no classifier anticipates.
        AiErrorReason::UnknownProvider { .. } => true,
        _ => false,
    }
}

/// Read a provider's human-readable message from common error body layouts
/// (port of `providerErrorMessage`), in priority order:
/// `{error:{message}}`, `{error}`, `{message}`, AWS `{Message}`, RFC 9457
/// `{detail}`, `{errors:[{message}]}`. Blank/non-string/non-JSON bodies
/// yield `None`.
#[must_use]
pub fn provider_error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body.trim()).ok()?;
    let candidate = value
        .pointer("/error/message")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("error").and_then(|v| v.as_str()))
        .or_else(|| value.get("message").and_then(|v| v.as_str()))
        .or_else(|| value.get("Message").and_then(|v| v.as_str()))
        .or_else(|| value.get("detail").and_then(|v| v.as_str()))
        .or_else(|| {
            value.get("errors")?.as_array()?.first()?.get("message").and_then(|v| v.as_str())
        })?;
    let trimmed = candidate.trim();
    (!trimmed.is_empty()).then(|| candidate.to_string())
}

/// Borrowed classification input (port of `ProviderFailure`).
pub struct ProviderFailure<'a> {
    pub message: &'a str,
    pub status: Option<u16>,
    /// Raw wire payload, scanned for failure signals (codes, overflow
    /// phrases) the summary message does not carry. Not shown to users.
    pub raw_body: Option<&'a str>,
    /// Some SDKs supply parsed error data separately from the response text.
    pub data: Option<&'a serde_json::Value>,
    pub http: Option<HttpContext>,
    pub retry_after_ms: Option<f64>,
    pub rate_limit: Option<HttpRateLimitDetails>,
}

/// Classification records affirmative evidence about a failure (port of
/// `classifyProviderFailure`): deterministic failures need positive
/// identification (a 4xx status, quota/auth/policy signals); anything
/// unrecognized stays `UnknownProvider`, which the session retry policy
/// treats as retry-eligible.
pub fn classify_provider_failure(input: ProviderFailure<'_>) -> AiErrorReason {
    let fields = crate::schema::ReasonFields {
        message: input.message.to_string(),
        body: input.raw_body.map(str::to_string),
        http: input.http.clone(),
        cause: None,
    };
    let body = input.raw_body.unwrap_or("");
    let mut codes: Vec<String> = Vec::new();
    codes.extend(provider_codes(input.data));
    codes.extend(provider_codes_from_str(body));
    codes.extend(provider_codes_from_str(input.message));
    if let Some(label) = gateway_code_label().captures(input.message).and_then(|c| c.get(1)) {
        codes.push(label.as_str().to_string());
    }
    let codes: Vec<String> = codes.into_iter().map(|code| code.to_ascii_lowercase()).collect();
    // Scan the raw payload too so signals missing from the summary message
    // (e.g. overflow phrases nested in a JSON error body) still classify.
    let text = [input.message, body]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let client_scoped = input.status.is_none_or(|status| (400..500).contains(&status));
    let has_code =
        |set: &BTreeSet<&'static str>| codes.iter().any(|code| set.contains(code.as_str()));
    let overflow_code = codes.iter().any(|code| {
        code == "context_length_exceeded"
            || code == "model_context_window_exceeded"
            || code == "request_too_large"
    });

    if client_scoped && (overflow_code || is_context_overflow(&text)) {
        return AiErrorReason::InvalidRequest {
            fields,
            parameter: None,
            classification: Some(ProviderFailureClassification::ContextOverflow),
        };
    }
    if input.status == Some(413) || is_payload_too_large(&text) {
        return AiErrorReason::InvalidRequest {
            fields,
            parameter: None,
            classification: Some(ProviderFailureClassification::PayloadTooLarge),
        };
    }
    if has_code(content_policy_codes())
        || (client_scoped && content_policy_text().is_match(input.message))
    {
        return AiErrorReason::ContentPolicy { fields };
    }
    if input.status == Some(402)
        || has_code(quota_codes())
        || (input.status == Some(429) && quota_text().is_match(&text))
    {
        return AiErrorReason::QuotaExceeded { fields };
    }
    if input.status == Some(401)
        || input.status == Some(403)
        || has_code(auth_codes())
        || (input.status == Some(400) && auth_text().is_match(&text))
    {
        return AiErrorReason::Authentication { fields };
    }
    if input.status == Some(429)
        || codes.iter().any(|code| {
            code.contains("rate_limit")
                || code == "too_many_requests"
                || code == "throttlingexception"
        })
        || rate_limit_text().is_match(&text)
    {
        return AiErrorReason::RateLimit {
            fields,
            retry_after_ms: input.retry_after_ms,
            rate_limit: input.rate_limit,
        };
    }
    if input.status == Some(408)
        || input.status == Some(409)
        || input.status.is_some_and(|status| status >= 500)
        // Server codes and phrasing only decide when no HTTP status
        // contradicts them: gateways such as OpenCode Zen substitute
        // `server_error` for codes they do not forward, so a 4xx with a
        // server code is still a rejected request.
        || (input.status.is_none_or(|status| status < 400)
            && ((!has_code(invalid_request_codes()) && server_error_text().is_match(&text))
                || codes.iter().any(|code| {
                    server_codes().contains(code.as_str()) || code.contains("exhausted") || code.contains("unavailable")
                })))
    {
        return AiErrorReason::ProviderInternal { fields, retry_after_ms: input.retry_after_ms };
    }
    if has_code(invalid_request_codes()) {
        return AiErrorReason::InvalidRequest { fields, parameter: None, classification: None };
    }
    // Any remaining 4xx is a deterministic rejection of this request.
    if input.status.is_some_and(|status| (400..500).contains(&status)) {
        return AiErrorReason::InvalidRequest { fields, parameter: None, classification: None };
    }
    AiErrorReason::UnknownProvider { fields }
}

/// Collect provider failure codes from a payload (port of `providerCodes`):
/// top-level `code`/`error_type`, Stability's `errors[].name`, nested
/// `error.code/type/status/error_type` + Google `error.details[].reason`,
/// Azure `error.innererror.code`, `error.metadata.error_type`, Responses
/// `response.error.code` / `response.error_type`, `exception.type`.
fn provider_codes(data: Option<&serde_json::Value>) -> Vec<String> {
    let Some(data) = data else { return Vec::new() };
    // A string input may itself be JSON.
    if let Some(text) = data.as_str() {
        return match serde_json::from_str::<serde_json::Value>(text) {
            Ok(parsed) => provider_codes_from_value(&parsed),
            Err(_) => Vec::new(),
        };
    }
    provider_codes_from_value(data)
}

fn provider_codes_from_str(body: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => provider_codes_from_value(&value),
        Err(_) => Vec::new(),
    }
}

fn provider_codes_from_value(decoded: &serde_json::Value) -> Vec<String> {
    let Some(object) = decoded.as_object() else { return Vec::new() };
    let error = object.get("error").filter(|v| v.as_object().is_some());
    let inner = error.and_then(|error| error.get("innererror")).filter(|v| v.as_object().is_some());
    let metadata =
        error.and_then(|error| error.get("metadata")).filter(|v| v.as_object().is_some());
    let response = object.get("response").filter(|v| v.as_object().is_some());
    let response_error =
        response.and_then(|response| response.get("error")).filter(|v| v.as_object().is_some());
    let exception = object.get("exception").filter(|v| v.as_object().is_some());
    let mut codes = Vec::new();
    let push = |value: Option<&serde_json::Value>, codes: &mut Vec<String>| {
        if let Some(text) = value.and_then(|v| v.as_str()) {
            codes.push(text.to_string());
        }
    };
    push(object.get("code"), &mut codes);
    // Stability's `{ id, name, errors }` bodies carry the code in `name`.
    if object.get("errors").and_then(|v| v.as_array()).is_some() {
        push(object.get("name"), &mut codes);
    }
    push(object.get("error_type"), &mut codes);
    if let Some(error) = error {
        push(error.get("code"), &mut codes);
        push(error.get("type"), &mut codes);
        push(error.get("status"), &mut codes);
        push(error.get("error_type"), &mut codes);
        // Google `google.rpc.ErrorInfo` details carry the specific reason.
        if let Some(details) = error.get("details").and_then(|v| v.as_array()) {
            for detail in details {
                push(detail.get("reason"), &mut codes);
            }
        }
    }
    push(inner.and_then(|inner| inner.get("code")), &mut codes);
    push(metadata.and_then(|metadata| metadata.get("error_type")), &mut codes);
    push(response_error.and_then(|error| error.get("code")), &mut codes);
    push(response.and_then(|response| response.get("error_type")), &mut codes);
    push(exception.and_then(|exception| exception.get("type")), &mut codes);
    codes
}

/// Extract a `provider-error` event's classification, when set.
#[must_use]
pub fn provider_event_classification(event: &LLMEvent) -> Option<ProviderFailureClassification> {
    match event {
        LLMEvent::ProviderError { classification, .. } => *classification,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(message: &str) -> String {
        classify_provider_failure(ProviderFailure {
            message,
            status: None,
            raw_body: None,
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        })
        .tag()
        .to_string()
    }

    fn classify_status(message: &str, status: u16) -> String {
        classify_provider_failure(ProviderFailure {
            message,
            status: Some(status),
            raw_body: None,
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        })
        .tag()
        .to_string()
    }

    fn reason_for(message: &str, status: u16) -> AiErrorReason {
        classify_provider_failure(ProviderFailure {
            message,
            status: Some(status),
            raw_body: None,
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        })
    }

    fn classify_body(message: &str, status: Option<u16>, raw_body: &str) -> AiErrorReason {
        classify_provider_failure(ProviderFailure {
            message,
            status,
            raw_body: Some(raw_body),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        })
    }

    #[test]
    fn classifies_provider_token_limit_messages_as_context_overflow() {
        let messages = [
            "tokens in request more than max tokens allowed",
            "Requested token count exceeds the model's maximum context length of 131072 tokens.",
            "Requested input length 600010 exceeds maximum input length 131071",
            "Input length (265330) exceeds model's maximum context length (262144).",
            "Input length 131393 exceeds the maximum allowed input length of 131040 tokens.",
            "The input (516368 tokens) is longer than the model's context length (262144 tokens).",
            "The input is longer than the model's context length trace_id: 39d8c3a5c6c91dbc8dd9055f0b37e084",
            "Prompt has 5,958,968 tokens, but the configured context size is 256,000 tokens",
            "Range of input length should be [1, 129024]",
            "Too many tokens",
            "Input validation error: `inputs` tokens + `max_new_tokens` must be <= 131073. Given: 600035 `inputs` tokens and 16 `max_new_tokens`",
            "Token limit exceeded",
        ];
        assert!(messages.iter().all(|message| is_context_overflow(message)));
    }

    #[test]
    fn classifies_anthropic_request_too_large_as_recoverable_overflow() {
        // the message itself carries the request_too_large code envelope
        let reason = classify_provider_failure(ProviderFailure {
            message: r#"{"error":{"type":"request_too_large","message":"Request exceeds the maximum size"}}"#,
            status: Some(400),
            raw_body: None,
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        assert!(matches!(
            reason,
            AiErrorReason::InvalidRequest {
                classification: Some(ProviderFailureClassification::ContextOverflow),
                ..
            }
        ));
        assert!(is_context_overflow("413 status code (no body)"));
    }

    #[test]
    fn classifies_generic_request_size_failures_separately_from_context_overflow() {
        let failures = [
            classify_status("request too large", 413),
            classify_status("upstream request entity too large", 502),
        ];
        assert!(failures.iter().all(|tag| tag == "InvalidRequest"));
        let reason = classify_status("request too large", 413);
        assert!(matches!(reason.as_str(), "InvalidRequest"));
    }

    #[test]
    fn payload_classification_via_status_413() {
        let reason = classify_provider_failure(ProviderFailure {
            message: "request too large",
            status: Some(413),
            raw_body: None,
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        match reason {
            AiErrorReason::InvalidRequest { classification, .. } => {
                assert_eq!(classification, Some(ProviderFailureClassification::PayloadTooLarge));
            }
            other => panic!("expected InvalidRequest, got {}", other.tag()),
        }
    }

    #[test]
    fn does_not_classify_rate_limits_as_context_overflow() {
        let messages = [
            "Throttling error: Too many tokens, please wait before trying again.",
            "Rate limit exceeded, please retry after 30 seconds.",
            "Too many requests. Please slow down.",
        ];
        assert!(messages.iter().all(|message| !is_context_overflow(message)));
    }

    #[test]
    fn classifies_plain_text_rate_limit_fallbacks() {
        assert_eq!(
            [
                "Request rate increased too quickly",
                "Rate limit exceeded, please try again later",
                "Too many requests, please slow down",
            ]
            .map(classify),
            ["RateLimit", "RateLimit", "RateLimit"]
        );
    }

    #[test]
    fn classifies_json_rate_limit_fallbacks() {
        assert_eq!(
            [
                r#"{"type":"error","error":{"type":"too_many_requests"}}"#,
                r#"{"type":"error","error":{"code":"rate_limit_exceeded"}}"#,
                r#"{"code":"bad_request","error":{"code":"rate_limit_exceeded"}}"#,
                r#"{"type":"error","error":{"code":"unknown","type":"too_many_requests"}}"#,
            ]
            .map(classify),
            ["RateLimit", "RateLimit", "RateLimit", "RateLimit"]
        );
    }

    #[test]
    fn classifies_overloaded_provider_codes() {
        assert_eq!(
            [
                r#"{"code":"resource_exhausted"}"#,
                r#"{"code":"service_unavailable"}"#,
                r#"{"code":"slow_down"}"#
            ]
            .map(classify),
            ["ProviderInternal", "ProviderInternal", "ProviderInternal"]
        );
    }

    #[test]
    fn classifies_retryable_server_messages_as_provider_internal() {
        let message = "The model is currently at capacity due to high demand. Please try again in a few minutes, or use a higher service tier for priority processing.";
        assert_eq!(
            [
                message,
                "Try again",
                "Please retry your request shortly.",
                "You can retry the request.",
                "Try your request again.",
                "The service is temporarily at capacity.",
                "The model is overloaded.",
                "Service unavailable",
                "Internal server error",
                "The server is busy.",
                "Provider returned error",
                "Provider returned an error",
                "ResourceExhausted",
                "Upstream connection failed",
                "Exceeded request buffer limit while retrying upstream",
            ]
            .map(classify),
            ["ProviderInternal"; 15]
        );
        let reason = classify_provider_failure(ProviderFailure {
            message: "Provider request failed",
            status: None,
            raw_body: Some("Please try again later."),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        assert_eq!(reason.tag(), "ProviderInternal");
    }

    #[test]
    fn prioritizes_specific_failures_over_retryable_server_text() {
        assert_eq!(
            [
                classify_status("Invalid credentials, try again", 401),
                classify_status("Quota exceeded, try again", 429),
                classify("Rate limit exceeded, try again"),
                classify_status("Upstream request failed: validation failed", 400),
                classify_status("Try again", 200),
            ],
            ["Authentication", "QuotaExceeded", "RateLimit", "InvalidRequest", "ProviderInternal"]
        );
    }

    #[test]
    fn classifies_azure_content_filter_rejections_by_structured_codes() {
        let message = "The response was filtered";
        let azure = serde_json::json!({
            "error": {
                "message": message,
                "type": null,
                "param": "prompt",
                "code": "content_filter",
                "status": 400,
                "innererror": {"code": "ResponsibleAIPolicyViolation", "content_filter_result": {}}
            }
        });
        let inner_only = serde_json::json!({
            "error": {"message": message, "code": null, "innererror": {"code": "ResponsibleAIPolicyViolation"}}
        });
        assert_eq!(
            classify_body(message, Some(400), &serde_json::to_string(&azure).unwrap()).tag(),
            "ContentPolicy"
        );
        assert_eq!(
            classify_body(message, Some(400), &serde_json::to_string(&inner_only).unwrap()).tag(),
            "ContentPolicy"
        );
    }

    #[test]
    fn classifies_policy_rejections_reusing_generic_request_codes() {
        let openai = serde_json::json!({
            "error": {
                "message": "Invalid prompt: your prompt was flagged as potentially violating our usage policy. Please try again with a different prompt.",
                "type": "invalid_request_error",
                "param": null,
                "code": "invalid_prompt"
            }
        });
        let anthropic = serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "Output blocked by content filtering policy"}
        });
        for body in [openai, anthropic] {
            let message = body.pointer("/error/message").unwrap().as_str().unwrap();
            assert_eq!(
                classify_body(message, Some(400), &serde_json::to_string(&body).unwrap()).tag(),
                "ContentPolicy"
            );
        }
    }

    #[test]
    fn classifies_openrouter_typed_policy_errors_across_api_skins() {
        let chat = serde_json::json!({
            "id": "gen-abc123",
            "error": {
                "code": 403,
                "message": "Your input was flagged",
                "metadata": {"error_type": "content_policy_violation", "reasons": ["violence"]}
            }
        });
        let responses = serde_json::json!({
            "type": "response.failed",
            "response": {
                "error": {"code": "image_content_policy_violation", "message": "Your input was flagged"},
                "error_type": "content_policy_violation"
            }
        });
        let messages = serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "Claude refused to respond", "error_type": "refusal"}
        });
        let chat_message = chat.pointer("/error/message").unwrap().as_str().unwrap();
        assert_eq!(
            classify_body(chat_message, Some(403), &serde_json::to_string(&chat).unwrap()).tag(),
            "ContentPolicy"
        );
        assert_eq!(
            classify_body(
                "Your input was flagged",
                None,
                &serde_json::to_string(&responses).unwrap()
            )
            .tag(),
            "ContentPolicy"
        );
        assert_eq!(
            classify_body(
                "Claude refused to respond",
                None,
                &serde_json::to_string(&messages).unwrap()
            )
            .tag(),
            "ContentPolicy"
        );
        let stability = serde_json::json!({"id": "a1b2", "name": "content_moderation", "errors": ["Your request was flagged"]});
        assert_eq!(
            classify_body("Forbidden", Some(403), &serde_json::to_string(&stability).unwrap())
                .tag(),
            "ContentPolicy"
        );
    }

    #[test]
    fn recovers_policy_codes_preserved_only_in_gateway_message_labels() {
        let zen = serde_json::json!({
            "error": {
                "type": "server_error",
                "message": "Upstream request failed: [content_filter] The response was filtered"
            }
        });
        let message = zen.pointer("/error/message").unwrap().as_str().unwrap();
        assert_eq!(
            classify_body(message, Some(400), &serde_json::to_string(&zen).unwrap()).tag(),
            "ContentPolicy"
        );
    }

    #[test]
    fn keeps_invalid_prompt_schema_validation_failures_as_invalid_requests() {
        let message = [
            "SubmitRequestFailure: code=-32602, msg=219 validation errors for ResponsesRequest",
            "input.list[union[EasyInputMessageParam,ResponseComputerToolCallParam]].2.ResponseComputerToolCallParam.pending_safety_checks",
            "  Field required [type=missing, input_value={'content': [...], 'role': 'assistant', 'type': 'message'}, input_type=dict]",
        ].join("\n");
        let reason = classify_provider_failure(ProviderFailure {
            message: &message,
            status: Some(200),
            raw_body: Some(&serde_json::to_string(&serde_json::json!({
                "response": {"status": "failed", "error": {"code": "invalid_prompt", "message": message}}
            })).unwrap()),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        assert_eq!(reason.tag(), "InvalidRequest");
    }

    #[test]
    fn does_not_infer_content_policy_from_phrases_outside_the_provider_explanation() {
        let reason = classify_provider_failure(ProviderFailure {
            message: "Provider request failed with HTTP 400",
            status: Some(400),
            raw_body: Some(r#"{"error":{"message":"Unknown parameter: content_policy_id"}}"#),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        assert_eq!(reason.tag(), "InvalidRequest");
    }

    #[test]
    fn classifies_transient_client_statuses_as_provider_internal() {
        assert_eq!(
            [408, 409].map(|status| classify_status(&format!("HTTP {status}"), status)),
            ["ProviderInternal"; 2]
        );
    }

    #[test]
    fn classifies_any_remaining_4xx_status_as_an_invalid_request() {
        assert_eq!(
            [400, 404, 418, 422, 451]
                .map(|status| classify_status(&format!("HTTP {status}"), status)),
            ["InvalidRequest"; 5]
        );
    }

    #[test]
    fn classifies_402_as_exhausted_quota() {
        assert_eq!(classify_status("Payment Required", 402), "QuotaExceeded");
    }

    #[test]
    fn classifies_opencode_zen_account_limits_as_quota() {
        let typed = |kind: &str, message: &str| serde_json::json!({"type": "error", "error": {"type": kind, "message": message}});
        let substituted = |message: &str| serde_json::json!({"error": {"type": "server_error", "message": format!("Upstream request failed: {message}")}});
        let cases = [
            (429u16, typed("GoUsageLimitError", "Go usage limit exceeded")),
            (429, typed("FreeUsageLimitError", "Rate limit exceeded. Please try again later.")),
            (402, typed("CreditLimitExceeded", "Credit limit exceeded.")),
            (402, substituted("Insufficient account funds")),
            (402, substituted("Account invoice is overdue")),
            (429, substituted("Account budget exceeded")),
        ];
        for (status, body) in cases {
            let message = body.pointer("/error/message").unwrap().as_str().unwrap().to_string();
            let reason =
                classify_body(&message, Some(status), &serde_json::to_string(&body).unwrap());
            assert_eq!(reason.tag(), "QuotaExceeded", "case: {message}");
        }
    }

    #[test]
    fn classifies_zai_plan_and_balance_limits_as_quota() {
        let cases = [
            r#"{"error":{"code":"1113","message":"Insufficient balance or no resource package. Please recharge."}}"#,
            r#"{"error":{"code":"1308","message":"Usage limit reached for 5 hours. Your limit will reset at 2026-10-01 00:00:00"}}"#,
            r#"{"error":{"code":"1309","message":"Your GLM Coding Plan package has expired and is temporarily unavailable. You can resume using it after renewing the subscription on the official website."}}"#,
            r#"{"error":{"code":"1310","message":"Weekly/Monthly Limit Exhausted. Your limit will reset at 2026-10-01 00:00:00"}}"#,
            r#"{"error":{"code":"1311","message":"Your current subscription plan does not yet include access to glm-5"}}"#,
            r#"{"error":{"code":"1314","message":"Your enterprise package has expired. Please contact your enterprise administrator."}}"#,
            r#"{"type":"error","error":{"type":"rate_limit_error","code":"1309","message":"[1309][Your GLM Coding Plan package has expired and is temporarily unavailable. You can resume using it after renewing the subscription on the official website.][20260929132151e73af01340d54b58]"}}"#,
        ];
        for body in cases {
            let value: serde_json::Value = serde_json::from_str(body).unwrap();
            let message = value.pointer("/error/message").unwrap().as_str().unwrap().to_string();
            let reason = classify_body(&message, Some(429), body);
            assert_eq!(reason.tag(), "QuotaExceeded", "case: {message}");
        }
    }

    #[test]
    fn classifies_zai_prompt_length_rejections_as_context_overflow() {
        let cases = [
            r#"{"error":{"code":"1261","message":"Prompt 超长"}}"#,
            r#"{"error":{"code":"1261","message":"Prompt too long"}}"#,
            r#"{"type":"error","error":{"type":"invalid_request_error","code":"1261","message":"[1261][Prompt too long][2026092913]"}}"#,
        ];
        for body in cases {
            let value: serde_json::Value = serde_json::from_str(body).unwrap();
            let message = value.pointer("/error/message").unwrap().as_str().unwrap().to_string();
            let reason = classify_body(&message, Some(400), body);
            match reason {
                AiErrorReason::InvalidRequest { classification, .. } => {
                    assert_eq!(
                        classification,
                        Some(ProviderFailureClassification::ContextOverflow),
                        "case: {message}"
                    );
                }
                other => panic!("expected InvalidRequest, got {} for {message}", other.tag()),
            }
        }
    }

    #[test]
    fn classifies_zai_sensitive_content_rejections_as_content_policy() {
        let message = "System detected potentially unsafe or sensitive content in input or generation. Please avoid using prompts that may generate sensitive content. Thank you for your cooperation.";
        let reason = classify_provider_failure(ProviderFailure {
            message,
            status: Some(400),
            raw_body: Some(r#"{"error":{"code":"1301","message":"..."}}"#),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        assert_eq!(reason.tag(), "ContentPolicy");
    }

    #[test]
    fn keeps_zai_throttling_and_overload_retryable() {
        let cases = [
            r#"{"error":{"code":"1302","message":"Rate limit reached for requests"}}"#,
            r#"{"error":{"code":"1305","message":"The service may be temporarily overloaded, please try again later"}}"#,
        ];
        for body in cases {
            let value: serde_json::Value = serde_json::from_str(body).unwrap();
            let message = value.pointer("/error/message").unwrap().as_str().unwrap().to_string();
            let reason = classify_body(&message, Some(429), body);
            assert_eq!(reason.tag(), "RateLimit", "case: {message}");
        }
    }

    #[test]
    fn does_not_let_substituted_server_codes_make_a_4xx_retryable() {
        let openai = r#"{"error":{"type":"server_error","message":"Upstream request failed: Model is unavailable."}}"#;
        let anthropic = r#"{"type":"error","error":{"type":"api_error","message":"Upstream request failed: Model is unavailable."}}"#;
        for body in [openai, anthropic] {
            let value: serde_json::Value = serde_json::from_str(body).unwrap();
            let message = value.pointer("/error/message").unwrap().as_str().unwrap();
            assert_eq!(classify_body(message, Some(400), body).tag(), "InvalidRequest");
        }
        // Without a contradicting status the same codes still mark provider trouble.
        let value: serde_json::Value = serde_json::from_str(openai).unwrap();
        let message = value.pointer("/error/message").unwrap().as_str().unwrap();
        assert_eq!(classify_body(message, None, &value.to_string()).tag(), "ProviderInternal");
        assert_eq!(classify_body(message, Some(200), &value.to_string()).tag(), "ProviderInternal");
    }

    #[test]
    fn classifies_nested_provider_codes_when_a_top_level_code_is_also_present() {
        assert_eq!(
            [
                r#"{"code":"bad_request","error":{"code":"usage_not_included"}}"#,
                r#"{"code":"bad_request","error":{"code":"server_error"}}"#,
                r#"{"code":"bad_request","error":{"type":"invalid_request_error"}}"#,
            ]
            .map(classify),
            ["QuotaExceeded", "ProviderInternal", "InvalidRequest"]
        );
    }

    #[test]
    fn leaves_unrecognized_failures_unclassified_for_the_retry_default() {
        assert_eq!(classify(r#"{"error":{"message":"no_kv_space"}}"#), "UnknownProvider");
        assert_eq!(classify(r#"{"type":"error","error":{"code":123}}"#), "UnknownProvider");
        assert_eq!(classify("not-json"), "UnknownProvider");
        assert_eq!(classify("network error"), "UnknownProvider");
    }

    #[test]
    fn classifies_provider_envelopes_from_the_raw_body() {
        let cases = [
            (
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Try again"}}"#,
                "ProviderInternal",
            ),
            (
                r#"{"error":{"code":"insufficient_quota","message":"Request failed"}}"#,
                "QuotaExceeded",
            ),
            (
                r#"{"type":"response.failed","response":{"error":{"code":"authentication_error","message":"Denied"}}}"#,
                "Authentication",
            ),
            (
                r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"Try again"}}"#,
                "ProviderInternal",
            ),
            (
                r#"{"exception":{"type":"throttlingException","details":{"message":"Try again"}}}"#,
                "RateLimit",
            ),
        ];
        for (raw_body, expected) in cases {
            let reason = classify_body("Request failed", None, raw_body);
            assert_eq!(reason.tag(), expected, "body: {raw_body}");
            let fields = reason.fields();
            assert_eq!(fields.body.as_deref(), Some(raw_body), "raw body retained on the reason");
        }
    }

    #[test]
    fn classifies_separately_supplied_sdk_data_without_replacing_the_body() {
        let data = serde_json::json!({"error": {"code": "authentication_error"}});
        let raw_body = r#"{"message":"Request failed"}"#;
        for value in [&data, &serde_json::Value::String(data.to_string())] {
            let reason = classify_provider_failure(ProviderFailure {
                message: "Request failed",
                status: Some(400),
                raw_body: Some(raw_body),
                data: Some(value),
                http: None,
                retry_after_ms: None,
                rate_limit: None,
            });
            assert_eq!(reason.tag(), "Authentication");
            assert_eq!(reason.fields().body.as_deref(), Some(raw_body));
        }
    }

    #[test]
    fn classifies_workers_ai_context_window_rejections_despite_http_413() {
        let message = "AiError: Ai: The estimated number of input and maximum output tokens (900047) exceeded this model context window limit (32000). (17851589-f2e6-405d-b67c-5550e52ee1c1)";
        let reason = classify_provider_failure(ProviderFailure {
            message,
            status: Some(413),
            raw_body: Some(
                &serde_json::to_string(&serde_json::json!({
                    "errors": [{"message": message, "code": 5021}],
                    "success": false
                }))
                .unwrap(),
            ),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        match reason {
            AiErrorReason::InvalidRequest { classification, .. } => {
                assert_eq!(classification, Some(ProviderFailureClassification::ContextOverflow));
            }
            other => panic!("expected InvalidRequest, got {}", other.tag()),
        }
    }

    #[test]
    fn classifies_amazon_nova_input_token_rejections_as_context_overflow() {
        let message = "The model returned the following errors: Input Tokens Exceeded: Number of input tokens exceeds maximum length. Please update the input to try again.";
        let reason = classify_provider_failure(ProviderFailure {
            message,
            status: None,
            raw_body: Some(&serde_json::to_string(&serde_json::json!({
                "headers": {":exception-type": {"type": "string", "value": "validationException"}},
                "body": serde_json::to_string(&serde_json::json!({"message": message})).unwrap()
            })).unwrap()),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        match reason {
            AiErrorReason::InvalidRequest { classification, .. } => {
                assert_eq!(classification, Some(ProviderFailureClassification::ContextOverflow));
            }
            other => panic!("expected InvalidRequest, got {}", other.tag()),
        }
    }

    #[test]
    fn separates_cohere_prompt_overflow_from_output_limit_rejections() {
        let classify_message = |message: &str| {
            let reason = classify_provider_failure(ProviderFailure {
                message,
                status: Some(400),
                raw_body: Some(
                    &serde_json::to_string(&serde_json::json!({
                        "error_type": "TOO_MANY_TOKENS",
                        "message": message
                    }))
                    .unwrap(),
                ),
                data: None,
                http: None,
                retry_after_ms: None,
                rate_limit: None,
            });
            match reason {
                AiErrorReason::InvalidRequest { classification, .. } => classification,
                other => panic!("expected InvalidRequest, got {} for {message}", other.tag()),
            }
        };
        assert_eq!(
            classify_message(
                "too many tokens: size limit exceeded by 168512 tokens. Try using shorter or fewer inputs. The limit for this model is 132000 tokens."
            ),
            Some(ProviderFailureClassification::ContextOverflow)
        );
        // output-limit rejection: not context overflow; falls through to 4xx InvalidRequest without classification
        let output_limit = "too many tokens: max tokens must be less than or equal to 4096, the maximum output length for this model - received 1000000.";
        let reason = classify_provider_failure(ProviderFailure {
            message: output_limit,
            status: Some(400),
            raw_body: Some(
                &serde_json::to_string(&serde_json::json!({
                    "error_type": "TOO_MANY_TOKENS",
                    "message": output_limit
                }))
                .unwrap(),
            ),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        assert!(
            matches!(reason, AiErrorReason::InvalidRequest { classification: None, .. }),
            "got {}",
            reason.tag()
        );
    }

    #[test]
    fn classifies_invalid_api_keys_reported_as_http_400_as_authentication() {
        let raw_body = serde_json::to_string(&serde_json::json!({
            "error": {
                "code": 400,
                "message": "API key not valid. Please pass a valid API key.",
                "status": "INVALID_ARGUMENT",
                "details": [
                    {"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "API_KEY_INVALID", "domain": "googleapis.com"},
                    {"@type": "type.googleapis.com/google.rpc.LocalizedMessage", "locale": "en-US", "message": "API key not valid. Please pass a valid API key."}
                ]
            }
        })).unwrap();
        assert_eq!(
            classify_body("API key not valid. Please pass a valid API key.", Some(400), &raw_body)
                .tag(),
            "Authentication"
        );
        // xAI
        assert_eq!(
            classify_body(
                "Incorrect API key provided. You can obtain an API key from https://console.x.ai.",
                Some(400),
                r#"{"code":"invalid-argument","error":"Incorrect API key provided."}"#
            )
            .tag(),
            "Authentication"
        );
    }

    #[test]
    fn classifies_overflow_signals_buried_in_the_raw_payload() {
        let reason = classify_provider_failure(ProviderFailure {
            message: "Request failed",
            status: None,
            raw_body: Some(
                r#"{"error":{"message":"This model's maximum context length is 40960 tokens"}}"#,
            ),
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        });
        match reason {
            AiErrorReason::InvalidRequest { classification, .. } => {
                assert_eq!(classification, Some(ProviderFailureClassification::ContextOverflow));
            }
            other => panic!("expected InvalidRequest, got {}", other.tag()),
        }
    }

    #[test]
    fn extracts_nested_codes_from_the_raw_payload() {
        assert_eq!(
            classify_body("Request failed", None, r#"{"error":{"code":"insufficient_quota"}}"#)
                .tag(),
            "QuotaExceeded"
        );
        assert_eq!(
            classify_body(
                "Unknown Model, please check the model code.",
                None,
                r#"{"type":"response.failed","response":{"error":{"code":"model_not_found"}}}"#
            )
            .tag(),
            "InvalidRequest"
        );
        assert_eq!(
            classify_body(
                "Your GLM Coding Plan package has expired and is temporarily unavailable.",
                None,
                r#"{"type":"response.failed","response":{"error":{"code":"permission_denied"}}}"#
            )
            .tag(),
            "Authentication"
        );
    }

    #[test]
    fn reads_messages_from_common_error_body_layouts() {
        assert_eq!(
            [
                r#"{"error":{"message":"Invalid API Key","type":"invalid_request_error"}}"#,
                r#"{"code":"invalid-argument","error":"Incorrect API key provided."}"#,
                r#"{"message":"1 validation error detected"}"#,
                r#"{"Message":"Invalid API Key format: Must start with pre-defined prefix"}"#,
                r#"{"type":"about:blank","title":"Gone","status":410,"detail":"The model has reached its end of life"}"#,
                r#"{"result":null,"success":false,"errors":[{"code":10000,"message":"Authentication error"}]}"#,
            ]
            .map(provider_error_message),
            [
                Some("Invalid API Key".to_string()),
                Some("Incorrect API key provided.".to_string()),
                Some("1 validation error detected".to_string()),
                Some("Invalid API Key format: Must start with pre-defined prefix".to_string()),
                Some("The model has reached its end of life".to_string()),
                Some("Authentication error".to_string()),
            ]
        );
    }

    #[test]
    fn prefers_the_nested_error_message_over_a_top_level_message() {
        assert_eq!(
            provider_error_message(
                r#"{"message":"Bad Request","error":{"message":"model not found"}}"#
            ),
            Some("model not found".to_string())
        );
    }

    #[test]
    fn ignores_blank_non_string_and_non_json_messages() {
        assert_eq!(
            [
                r#"{"error":{"message":"  "}}"#,
                r#"{"message":{"detail":[{"msg":"too high"}]}}"#,
                r#"{"errors":[]}"#,
                "invalid parameter",
            ]
            .map(provider_error_message),
            [None, None, None, None]
        );
    }

    #[test]
    fn is_retryable_follows_the_taxonomy() {
        let rate_limit = AiError::new(reason_for("Rate limit exceeded", 429));
        let internal = AiError::new(reason_for("HTTP 503", 503));
        assert!(is_retryable(&rate_limit));
        assert!(is_retryable(&internal));

        let auth = AiError::new(reason_for("Invalid credentials", 401));
        let quota = AiError::new(reason_for("Payment Required", 402));
        let policy = AiError::new(classify_body(
            "The response was filtered",
            Some(400),
            r#"{"error":{"code":"content_filter"}}"#,
        ));
        let invalid = AiError::new(reason_for("HTTP 400", 400));
        assert!(!is_retryable(&auth));
        assert!(!is_retryable(&quota));
        assert!(!is_retryable(&policy));
        assert!(!is_retryable(&invalid));

        let unknown = AiError::new(classify_provider_failure(ProviderFailure {
            message: "network error",
            status: None,
            raw_body: None,
            data: None,
            http: None,
            retry_after_ms: None,
            rate_limit: None,
        }));
        assert!(is_retryable(&unknown));

        let incomplete = AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: crate::schema::ReasonFields::new("The provider response ended unexpectedly."),
            classification: Some("incomplete-stream".into()),
            route: Some("test".into()),
        });
        assert!(is_retryable(&incomplete));

        let transport_read = AiError::new(AiErrorReason::Transport {
            fields: crate::schema::ReasonFields::new("Connection lost"),
            transport: crate::schema::TransportType::Http,
            operation: TransportOperation::Read,
            code: None,
            url: None,
            phase: None,
            delivery: Some(TransportDelivery::Ambiguous),
            recovery: None,
        });
        assert!(is_retryable(&transport_read));
        let transport_rejected = AiError::new(AiErrorReason::Transport {
            fields: crate::schema::ReasonFields::new("Rejected"),
            transport: crate::schema::TransportType::Websocket,
            operation: TransportOperation::Write,
            code: None,
            url: None,
            phase: None,
            delivery: Some(TransportDelivery::Rejected),
            recovery: None,
        });
        assert!(!is_retryable(&transport_rejected));
    }

    #[test]
    fn is_retryable_honors_the_should_retry_header_override() {
        let make = |value: &str| {
            AiError::new(AiErrorReason::InvalidRequest {
                fields: crate::schema::ReasonFields {
                    message: "no".into(),
                    body: None,
                    http: Some(HttpContext {
                        url: "https://x.test".into(),
                        status: 400,
                        headers: [("x-should-retry".to_string(), value.to_string())].into(),
                    }),
                    cause: None,
                },
                parameter: None,
                classification: None,
            })
        };
        assert!(is_retryable(&make("true")));
        assert!(!is_retryable(&make("false")));
    }
}
