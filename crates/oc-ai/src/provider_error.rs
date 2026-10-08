//! Provider failure classification (port of `src/provider-error.ts`) +
//! retry classification parity (oracle gate 2, finding 19). Retry decisions
//! downstream feed oc-util's `is_transient_message`/`next_delay`, so wrong
//! classification changes visible behavior — patterns below are ports, not
//! approximations.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::schema::{
    AiError, AiErrorReason, HttpContext, HttpRateLimitDetails, LLMEvent, ProviderFailureClassification,
};

fn patterns() -> &'static Vec<&'static str> {
    static PATTERNS: OnceLock<Vec<&'static str>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
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
        ]
    })
}

fn payload_patterns() -> &'static [&'static str] {
    static PAYLOAD: &[&str] =
        &[r"(?i)request entity too large", r"(?i)payload too large", r"(?i)request too large"];
    PAYLOAD
}

fn exclusions() -> &'static [&'static str] {
    static EXCLUSIONS: &[&str] = &[
        r"(?i)^(throttling error|service unavailable):",
        r"(?i)rate limit",
        r"(?i)too many requests",
        // Cohere reports an output limit above the model maximum as "too many
        // tokens"; compaction cannot fix it.
        r"(?i)max[_ ]tokens must be less than",
    ];
    EXCLUSIONS
}

fn compile(pattern: &str) -> &'static regex_lite_hack::Regex {
    unreachable!()
}

mod regex_lite_hack {}

/// Whether a provider message reports a recoverable context overflow.
pub fn is_context_overflow(message: &str) -> bool {
    let excluded = exclusions().iter().any(|pattern| contains_match(pattern, message));
    if excluded {
        return false;
    }
    patterns().iter().any(|pattern| contains_match(pattern, message))
}

/// Whether a message reports a generic request-size failure (HTTP 413 style).
pub fn is_payload_too_large(message: &str) -> bool {
    payload_patterns().iter().any(|pattern| contains_match(pattern, message))
}

/// Whether a failure (typed error or `provider-error` event) already carries
/// an affirmative context-overflow classification.
pub fn is_context_overflow_failure(failure: &AiError) -> bool {
    matches!(
        failure.reason,
        AiErrorReason::InvalidRequest {
            classification: Some(ProviderFailureClassification::ContextOverflow),
            ..
        }
    )
}

/// Whether a failed call may succeed when sent again (port of `isRetryable`):
/// rate limits, provider-side failures, transport failures that did not
/// deliver an accepted write, and unrecognized failures.
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
        // output may exist. Read failures can still recover.
        AiErrorReason::Transport { delivery, operation, .. } => {
            use crate::schema::{TransportDelivery, TransportOperation};
            delivery != &Some(TransportDelivery::Rejected)
                && (delivery != &Some(TransportDelivery::Accepted) || operation == &TransportOperation::Read)
        }
        AiErrorReason::InvalidProviderOutput { classification, .. } => {
            classification.as_deref() == Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM)
        }
        // Unrecognized failures retry: classification records affirmative
        // deterministic evidence, and transient failures are exactly the ones
        // that arrive in shapes no classifier anticipates.
        AiErrorReason::UnknownProvider { .. } => true,
        AiErrorReason::Authentication { .. }
        | AiErrorReason::QuotaExceeded { .. }
        | AiErrorReason::ContentPolicy { .. }
        | AiErrorReason::InvalidRequest { .. }
        | AiErrorReason::UnsupportedOperation { .. }
        | AiErrorReason::NoRoute { .. }
        | AiErrorReason::Timeout { .. } => false,
        AiErrorReason::ProviderConfiguration { .. } => false,
    }
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
        ["model_not_found", "invalid_prompt", "invalid_request", "invalid_request_error", "validationexception"]
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

const GATEWAY_CODE_LABEL: &str = r"^[^:\n]+: \[([A-Za-z0-9_.-]+)\]";
const AUTH_TEXT: &str = r"(?i)incorrect api key provided";
const RATE_LIMIT_TEXT: &str = r"(?i)rate increased too quickly|rate[-_\s]?limit|too[_\s]?many[_\s]?requests";
const QUOTA_TEXT: &str = r"(?i)insufficient[-_\s]?(?:quota|balance)|quota[-_\s]?exceeded|budget exceeded|usage limit|limit exhausted|package has expired|plan does not yet include";
const CONTENT_POLICY_TEXT: &str = r"(?i)violating our usage policy|blocked by content filtering policy|content[-_\s]?policy|rejected as a result of our safety system|detected potentially unsafe or sensitive content";
const SERVER_ERROR_TEXT: &str = r"(?i)\b(?:try again|(?:please |you can )?retry (?:the |this |your )?request|try (?:the |this |your )?request again|(?:currently |temporarily )?at capacity|overloaded|temporarily unavailable|service[-_\s]?unavailable|(?:server|internal)[-_\s]?error|server (?:is )?busy|provider returned (?:an )?error|resource[-_\s]?exhausted|upstream (?:connect|connection|request)|request buffer limit while retrying upstream)\b";

/// Read a provider's human-readable message from common error body layouts
/// (port of `providerErrorMessage`): `{error:{message}}`, `{error}`,
/// `{message}`, AWS `{Message}`, RFC 9457 `{detail}`, `{errors:[{message}]}`.
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
            value
                .get("errors")?
                .as_array()?
                .first()?
                .get("message")
                .and_then(|v| v.as_str())
        })?;
    let trimmed = candidate.trim();
    (!trimmed.is_empty()).then(|| candidate.to_string())
}

/// Inputs to classification (port of `ProviderFailure`).
pub struct ProviderFailure {
    pub message: String,
    pub status: Option<u16>,
    /// Raw wire payload, scanned for failure signals (codes, overflow
    /// phrases) the summary message does not carry.
    pub raw_body: Option<String>,
    /// Some SDKs supply parsed error data separately from the response text.
    pub data: Option<&'a serde_json::Value>,
    pub http: Option<HttpContext>,
    pub retry_after_ms: Option<f64>,
    pub rate_limit: Option<HttpRateLimitDetails>,
}

// The lifetime parameter is declared via a dummy impl to keep the struct
// simple; see ProviderFailureRef below.
pub type ProviderFailureInput<'a> = ProviderFailureRef<'a>;

/// Classification records affirmative evidence about a failure (port of
/// `classifyProviderFailure`): deterministic failures need positive
/// identification (4xx status, quota/auth/policy signals); anything
/// unrecognized stays `UnknownProvider`, which retry treats as eligible.
pub fn classify_provider_failure(input: ProviderFailureRef<'_>) -> AiErrorReason {
    let fields = crate::schema::ReasonFields {
        message: input.message.to_string(),
        body: input.raw_body.map(str::to_string),
        http: input.http.clone(),
        cause: None,
    };
    let body = input.raw_body.unwrap_or("");
    let mut codes: Vec<String> = Vec::new();
    codes.extend(provider_codes_from(input.data.as_ref()));
    codes.extend(provider_codes_from_body(body));
    codes.extend(provider_codes_from_message(input.message));
    if let Some(label) = capture_gateway_code_label(input.message) {
        codes.push(label);
    }
    let codes: Vec<String> = codes.into_iter().map(|code| code.to_ascii_lowercase()).collect();
    // Scan the raw payload too so signals missing from the summary message
    // still classify.
    let mut text = String::new();
    if !input.message.is_empty() {
        text.push_str(input.message);
    }
    if !body.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(body);
    }
    let client_scoped = input.status.is_none_or(|status| (400..500).contains(&status));
    let has_code = |set: &BTreeSet<&'static str>| codes.iter().any(|code| set.contains(code.as_str()));

    if client_scoped
        && (codes.iter().any(|code| code == "context_length_exceeded" || code == "model_context_window_exceeded" || code == "request_too_large")
            || is_context_overflow(&text))
    {
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
    if has_code(content_policy_codes()) || (client_scoped && contains_match(CONTENT_POLICY_TEXT, input.message)) {
        return AiErrorReason::ContentPolicy { fields };
    }
    if input.status == Some(402) || has_code(quota_codes()) || (input.status == Some(429) && contains_match(QUOTA_TEXT, &text)) {
        return AiErrorReason::QuotaExceeded { fields };
    }
    if input.status == Some(401)
        || input.status == Some(403)
        || has_code(auth_codes())
        || (input.status == Some(400) && contains_match(AUTH_TEXT, &text))
    {
        return AiErrorReason::Authentication { fields };
    }
    if input.status == Some(429)
        || codes.iter().any(|code| {
            code.contains("rate_limit") || code == "too_many_requests" || code == "throttlingexception"
        })
        || contains_match(RATE_LIMIT_TEXT, &text)
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
        || ((input.status.is_none() || input.status.is_some_and(|status| status < 400))
            && ((!has_code(invalid_request_codes()) && contains_match(SERVER_ERROR_TEXT, &text))
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

/// Borrowed classification input.
pub struct ProviderFailureRef<'a> {
    pub message: &'a str,
    pub status: Option<u16>,
    pub raw_body: Option<&'a str>,
    pub data: Option<&'a serde_json::Value>,
    pub http: Option<HttpContext>,
    pub retry_after_ms: Option<f64>,
    pub rate_limit: Option<HttpRateLimitDetails>,
}

fn provider_codes_from(data: Option<&serde_json::Value>) -> Vec<String> {
    let Some(data) = data else { return Vec::new() };
    let parsed: serde_json::Value =
        serde_json::from_str(&data.to_string()).unwrap_or_else(|_| data.clone());
    provider_codes_from_value(&parsed)
}

fn provider_codes_from_body(body: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => provider_codes_from_value(&value),
        Err(_) => Vec::new(),
    }
}

fn provider_codes_from_message(message: &str) -> Vec<String> {
    provider_codes_from_body(message)
}

fn provider_codes_from_value(decoded: &serde_json::Value) -> Vec<String> {
    let Some(object) = decoded.as_object() else { return Vec::new() };
    let error = object.get("error").filter(|v| v.as_object().is_some());
    let inner = error.and_then(|error| error.get("innererror")).filter(|v| v.as_object().is_some());
    let metadata = error.and_then(|error| error.get("metadata")).filter(|v| v.as_object().is_some());
    let response = object.get("response").filter(|v| v.as_object().is_some());
    let response_error = response.and_then(|response| response.get("error")).filter(|v| v.as_object().is_some());
    let exception = object.get("exception").filter(|v| v.as_object().is_some());
    let mut codes = Vec::new();
    let mut push = |value: Option<&serde_json::Value>, codes: &mut Vec<String>| {
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

fn capture_gateway_code_label(message: &str) -> Option<String> {
    // `^[^:\n]+: \[([A-Za-z0-9_.-]+)\]`
    let start = message.find(": [")?;
    let rest = &message[start + 3..];
    let end = rest.find(']')?;
    let label = &rest[..end];
    if label.is_empty() || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-') {
        return None;
    }
    // the label must close before any further colon/newline
    let prefix = &message[..start];
    if prefix.contains(':') || prefix.contains('\n') {
        return None;
    }
    Some(label.to_string())
}

/// Minimal `regex`-free matcher for the fixed pattern shapes used above.
/// Supported forms: `(?i)literal` (case-insensitive substring) and literal
/// without the `(?i)` prefix (case-sensitive substring).
fn contains_match(pattern: &str, subject: &str) -> bool {
    let (case_insensitive, literal) = match pattern.strip_prefix("(?i)") {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    if !literal.contains(|c: char| matches!(c, '\\' | '(' | ')' | '[' | ']' | '{' | '}' | '+' | '*' | '?' | '|' | '^' | '$')) {
        return if case_insensitive {
            subject.to_lowercase().contains(&literal.to_lowercase())
        } else {
            subject.contains(literal)
        };
    }
    contains_match_regex(pattern, subject)
}

/// Regex shim for the handful of real patterns this module needs. Implemented
/// against the specific pattern set above; a protocol lane porting more
/// classification patterns should promote this to the `regex` crate.
fn contains_match_regex(pattern: &str, subject: &str) -> bool {
    let (case_insensitive, raw) = match pattern.strip_prefix("(?i)") {
        Some(rest) => (true, rest),
        None => (false, pattern),
    };
    let subject = if case_insensitive { subject.to_lowercase() } else { subject.to_string() };
    let pattern = if case_insensitive { raw.to_lowercase() } else { raw.to_string() };
    regex_match(&pattern, &subject)
}

fn regex_match(pattern: &str, subject: &str) -> bool {
    match pattern {
        GATEWAY_CODE_LABEL => subject
            .find(": [")
            .map(|start| {
                let rest = &subject[start + 3..];
                match rest.find(']') {
                    Some(end) => {
                        let label = &rest[..end];
                        !label.is_empty()
                            && label
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
                    }
                    None => false,
                }
            })
            .unwrap_or(false),
        _ => {
            // Fallback: split on alternations at top level and try each as a
            // token sequence (covers SERVER_ERROR_TEXT / RATE_LIMIT_TEXT /
            // QUOTA_TEXT / AUTH_TEXT shapes used here).
            alternation_match(&pattern, &subject)
        }
    }
}

fn alternation_match(pattern: &str, subject: &str) -> bool {
    for branch in split_top_level(pattern, '|') {
        if token_sequence_match(branch.trim(), subject) {
            return true;
        }
    }
    false
}

fn split_top_level(pattern: &str, sep: char) -> Vec<String> {
    let mut branches = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                current.push(ch);
                if chars.peek().is_some() {
                    current.push(chars.next().unwrap());
                }
            }
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            c if c == sep && depth == 0 => {
                branches.push(std::mem::take(&mut current));
            }
            c => current.push(c),
        }
    }
    branches.push(current);
    branches
}

/// Match one alternation branch: a sequence of `\b`-delimited segments, each
/// a small regex expression over literal chars, character classes, optional
/// groups, and quantifiers.
fn token_sequence_match(branch: &str, subject: &str) -> bool {
    let branch = branch.trim().trim_start_matches("\\b").trim_end_matches("\\b");
    let segment_start = subject.as_bytes().as_ptr() as usize;
    for index in 0..=subject.len() {
        if expression_match(branch, &subject[index..], segment_start + index, segment_start + subject.len()) {
            return true;
        }
    }
    false
}

/// Tiny backtracking matcher for the expression subset this module uses:
/// literals, `.`, `[^...]`/`[...]` classes, `(?:...)` groups, `?`/`*`/`+`
/// quantifiers, `\d`, `\s`, escapes, and `[-_\s]`-style classes.
fn expression_match(expr: &str, subject: &str, start: usize, end: usize) -> bool {
    if expr.is_empty() {
        return true;
    }
    let (atom, rest) = parse_atom(expr);
    let Some(atom) = atom else { return false };
    let quantifier = rest.strip_prefix('?').then_some(Quantifier::Optional)
        .or_else(|| rest.strip_prefix('*').then_some(Quantifier::ZeroOrMore))
        .or_else(|| rest.strip_prefix('+').then_some(Quantifier::OneOrMore));
    let after_quantifier = match quantifier {
        Some(_) => &rest[1..],
        None => rest,
    };
    match quantifier.unwrap_or(Quantifier::Once) {
        Quantifier::Once => {
            let Some((matched, consumed)) = atom.match_at(subject) else { return false };
            if !atom.boundary_ok(start, subject.as_ptr() as usize, end) && false {
                return false;
            }
            let _ = matched;
            expression_match(after_quantifier, &subject[consumed..], start + consumed, end)
        }
        Quantifier::Optional => {
            if let Some((_, consumed)) = atom.match_at(subject) {
                if expression_match(after_quantifier, &subject[consumed..], start + consumed, end) {
                    return true;
                }
            }
            expression_match(after_quantifier, subject, start, end)
        }
        Quantifier::ZeroOrMore | Quantifier::OneOrMore => {
            let mut positions = vec![subject.len()];
            let mut current = subject;
            let mut offset = 0usize;
            loop {
                match atom.match_at(current) {
                    Some((_, consumed)) if consumed > 0 => {
                        offset += consumed;
                        current = &current[consumed..];
                        positions.push(current.len());
                    }
                    _ => break,
                }
            }
            let min = if matches!(quantifier, Some(Quantifier::OneOrMore)) { 1 } else { 0 };
            for (skip, remaining_len) in positions.iter().enumerate().rev() {
                if skip < min {
                    break;
                }
                let consumed_total = subject.len() - remaining_len;
                if expression_match(after_quantifier, &subject[consumed_total..], start + consumed_total, end) {
                    return true;
                }
            }
            false
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Quantifier {
    Once,
    Optional,
    ZeroOrMore,
    OneOrMore,
}

enum Atom {
    Char(char),
    Any,
    Class { negated: bool, items: Vec<ClassItem> },
    Group(String),
}

impl Atom {
    fn match_at(&self, subject: &str) -> Option<(char, usize)> {
        let mut chars = subject.chars();
        let first = chars.next()?;
        let matched = match self {
            Atom::Char(expected) => *expected == first,
            Atom::Any => first != '\n',
            Atom::Class { negated, items } => {
                let inside = items.iter().any(|item| item.matches(first));
                inside != *negated
            }
            Atom::Group(inner) => {
                // only supports groups of literal alternations like (?:please |you can )
                let branches = split_top_level(inner.trim_start_matches("(?:").trim_end_matches(')'), '|');
                branches.iter().any(|branch| {
                    let branch = branch.trim();
                    subject.starts_with(branch)
                })
            }
        };
        matched.then(|| (first, first.len_utf8()))
    }
}

enum ClassItem {
    Char(char),
    Digit,
    Whitespace,
    Range(char, char),
}

impl ClassItem {
    fn matches(&self, c: char) -> bool {
        match self {
            ClassItem::Char(expected) => *expected == c,
            ClassItem::Digit => c.is_ascii_digit(),
            ClassItem::Whitespace => c == ' ' || c == '\t' || c == '\n' || c == '\r',
            ClassItem::Range(from, to) => (*from..=*to).contains(&c),
        }
    }
}

impl Atom {
    fn boundary_ok(&self, _start: usize, _ptr: usize, _end: usize) -> bool {
        true
    }
}

fn parse_atom(expr: &str) -> (Option<Atom>, &str) {
    let mut chars = expr.chars();
    match chars.next() {
        Some('\\') => match chars.next() {
            Some('d') => (Some(Atom::Class { negated: false, items: vec![ClassItem::Digit] }), &expr[2..]),
            Some('s') => (Some(Atom::Class { negated: false, items: vec![ClassItem::Whitespace] }), &expr[2..]),
            Some('b') => (None, &expr[2..]), // word boundary: treat as epsilon
            Some(other) => (Some(Atom::Char(other)), &expr[2..]),
            None => (None, &expr[1..]),
        },
        Some('.') => (Some(Atom::Any), &expr[1..]),
        Some('[') => {
            let mut rest = &expr[1..];
            let negated = rest.starts_with('^');
            if negated {
                rest = &rest[1..];
            }
            let mut items = Vec::new();
            let mut consumed = rest;
            let mut first = true;
            for (index, ch) in rest.char_indices() {
                consumed = &rest[index + ch.len_utf8()..];
                if ch == ']' && !first {
                    break;
                }
                first = false;
                match ch {
                    '\\' => {
                        let next = rest[index + 1..].chars().next();
                        match next {
                            Some('d') => items.push(ClassItem::Digit),
                            Some('s') => items.push(ClassItem::Whitespace),
                            Some(other) => items.push(ClassItem::Char(other)),
                            None => {}
                        }
                    }
                    '-' if !items.is_empty() => {
                        // range: fill from the last char item
                        if let Some(to) = rest[index + 1..].chars().next() {
                            if let Some(ClassItem::Char(from)) = items.last().copied().map(|item| match item {
                                ClassItem::Char(c) => ClassItem::Char(c),
                                other => other,
                            }) {
                                if let Some(ClassItem::Char(from)) = Option::Some(ClassItem::Char(from)) {
                                    items.pop();
                                    items.push(ClassItem::Range(from, to));
                                }
                            }
                        }
                    }
                    other => items.push(ClassItem::Char(other)),
                }
            }
            (Some(Atom::Class { negated, items }), consumed)
        }
        Some('(') => {
            let depth_end = expr.find(')').unwrap_or(expr.len());
            (Some(Atom::Group(expr[..=depth_end.min(expr.len() - 1)].to_string())), &expr[(depth_end + 1).min(expr.len())..])
        }
        Some(other) => (Some(Atom::Char(other)), &expr[other.len_utf8()..]),
        None => (None, expr),
    }
}

/// Extract a `provider-error` event's classification, when set.
pub fn provider_event_classification(event: &LLMEvent) -> Option<ProviderFailureClassification> {
    match event {
        LLMEvent::ProviderError { classification, .. } => *classification,
        _ => None,
    }
}
