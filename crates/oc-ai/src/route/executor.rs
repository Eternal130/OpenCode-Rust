//! Request execution diagnostics (port of `src/route/executor.ts`, minus the
//! Effect service plumbing): `retry-after` parsing, OpenAI/Anthropic
//! rate-limit header extraction, and HTTP failure classification bridging to
//! [`crate::provider_error`]. There is **no auto-retry loop** here — retry
//! decisions live with callers using [`crate::provider_error::is_retryable`].

use std::collections::BTreeMap;

use crate::provider_error::{classify_provider_failure, provider_error_message, ProviderFailure};
use crate::schema::{AiError, HttpContext, HttpRateLimitDetails};

/// Cap on retained error bodies before they are truncated into the message.
pub const MAX_BODY_CHARS: usize = 2000;

/// Parse `retry-after-ms` or `retry-after` (seconds or HTTP-date) into
/// milliseconds, clamped to `>= 0` (port of `retryAfterMs`).
#[must_use]
pub fn retry_after_ms(headers: &BTreeMap<String, String>) -> Option<f64> {
    let get = |name: &str| -> Option<&String> {
        headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    };
    if let Some(value) = get("retry-after-ms") {
        if let Ok(millis) = value.trim().parse::<f64>() {
            if millis.is_finite() {
                return Some(millis.max(0.0));
            }
        }
    }
    let value = get("retry-after")?;
    let trimmed = value.trim();
    if let Ok(seconds) = trimmed.parse::<f64>() {
        if seconds.is_finite() {
            return Some((seconds * 1000.0).max(0.0));
        }
    }
    if let Some(remaining) = parse_http_date(trimmed) {
        return Some(remaining.max(0.0));
    }
    None
}

/// Parse an RFC 1123 / RFC 850 / asctime HTTP date into epoch millis.
/// Supports the three formats `Date.parse` accepts in practice for
/// `retry-after`.
fn parse_http_date(value: &str) -> Option<f64> {
    let value = value.trim();
    let utc_now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as f64;
    // tokenize: weekday names (with optional comma) and the trailing zone
    // name are ignored; find the time (two colons), day, month, year
    let mut time_part: Option<&str> = None;
    let mut day: Option<u32> = None;
    let mut month: Option<u32> = None;
    let mut year: Option<i64> = None;
    for token in value.split_whitespace() {
        let token = token.trim_matches(',');
        if token.matches(':').count() == 2 {
            time_part = Some(token);
            continue;
        }
        if month_index(token).is_some() && month.is_none() {
            month = month_index(token);
            continue;
        }
        if let Ok(parsed) = token.parse::<u32>() {
            if token.len() <= 2 && day.is_none() {
                day = Some(parsed);
                continue;
            }
        }
        if let Ok(parsed) = token.parse::<i64>() {
            year = Some(parsed);
        }
        // weekday or zone names fall through
    }
    let time_part = time_part?;
    let day = day?;
    let month = month?;
    let year = year?;
    let mut time = time_part.split(':');
    let hour: i64 = time.next()?.parse().ok()?;
    let minute: i64 = time.next()?.parse().ok()?;
    let second: i64 = time.next()?.parse().ok()?;
    // days-from-civil algorithm (Howard Hinnant), UTC
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let epoch_seconds = days * 86400 + hour * 3600 + minute * 60 + second;
    Some(epoch_seconds as f64 * 1000.0 - utc_now)
}

fn month_index(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] =
        ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let lower = name.to_ascii_lowercase();
    if lower.len() < 3 {
        return None;
    }
    let prefix = &lower[..3];
    MONTHS.iter().position(|m| *m == prefix).map(|index| index as u32 + 1)
}

/// Extract rate-limit telemetry from response headers (port of
/// `rateLimitDetails`): `OpenAI` `x-ratelimit-{limit,remaining,reset}-<bucket>`
/// and Anthropic `anthropic-ratelimit-<bucket>-{limit,remaining,reset}`.
#[must_use]
pub fn rate_limit_details(
    headers: &BTreeMap<String, String>,
    retry_after: Option<f64>,
) -> Option<HttpRateLimitDetails> {
    let mut limit: BTreeMap<String, String> = BTreeMap::new();
    let mut remaining: BTreeMap<String, String> = BTreeMap::new();
    let mut reset: BTreeMap<String, String> = BTreeMap::new();

    let add = |target: &mut BTreeMap<String, String>, key: &str, value: &str| {
        if !key.is_empty() {
            target.insert(key.to_string(), value.to_string());
        }
    };

    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        if let Some(bucket) = lower.strip_prefix("x-ratelimit-limit-") {
            add(&mut limit, bucket, value);
        } else if let Some(bucket) = lower.strip_prefix("x-ratelimit-remaining-") {
            add(&mut remaining, bucket, value);
        } else if let Some(bucket) = lower.strip_prefix("x-ratelimit-reset-") {
            add(&mut reset, bucket, value);
        } else if let Some(rest) = lower.strip_prefix("anthropic-ratelimit-") {
            if let Some((bucket, kind)) = rest.rsplit_once('-') {
                match kind {
                    "limit" => add(&mut limit, bucket, value),
                    "remaining" => add(&mut remaining, bucket, value),
                    "reset" => add(&mut reset, bucket, value),
                    _ => {}
                }
            }
        }
    }

    if retry_after.is_none() && limit.is_empty() && remaining.is_empty() && reset.is_empty() {
        return None;
    }

    Some(HttpRateLimitDetails {
        retry_after_ms: retry_after,
        limit: (!limit.is_empty()).then_some(limit),
        remaining: (!remaining.is_empty()).then_some(remaining),
        reset: (!reset.is_empty()).then_some(reset),
    })
}

/// Summary message for a failed provider response (port of `providerMessage`):
/// prefer a recognized message from the body; never show gateway HTML pages;
/// truncate everything else to [`MAX_BODY_CHARS`].
#[must_use]
pub fn provider_message(status: u16, body: Option<&str>) -> String {
    let fallback = format!("Provider request failed with HTTP {status}");
    let text = body.unwrap_or("").trim();
    if let Some(message) = provider_error_message(text) {
        return message;
    }
    if text.is_empty()
        || text.to_ascii_lowercase().starts_with("<!doctype")
        || text.to_ascii_lowercase().starts_with("<html")
    {
        return fallback;
    }
    if text.len() > MAX_BODY_CHARS {
        let truncated: String = text.chars().take(MAX_BODY_CHARS).collect();
        format!("{fallback}: {truncated}…")
    } else {
        format!("{fallback}: {text}")
    }
}

/// Build a typed AI error from an HTTP failure (port of `httpFailure`):
/// normalizes headers, parses retry/rate-limit telemetry, and classifies.
#[must_use]
pub fn http_failure(input: HttpFailureInput<'_>) -> AiError {
    let headers: BTreeMap<String, String> =
        input.response_headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect();
    let retry_after = retry_after_ms(&headers);
    let rate_limit = rate_limit_details(&headers, retry_after);
    let http = match (input.status, input.url) {
        (Some(status), Some(url)) => {
            Some(HttpContext { url: url.to_string(), status, headers: headers.clone() })
        }
        _ => None,
    };
    AiError::new(classify_provider_failure(ProviderFailure {
        message: input.message,
        status: input.status,
        raw_body: input.response_body,
        data: input.data,
        retry_after_ms: retry_after,
        rate_limit,
        http,
    }))
}

/// Inputs to [`http_failure`].
#[derive(Default)]
pub struct HttpFailureInput<'a> {
    pub message: &'a str,
    pub url: Option<&'a str>,
    pub status: Option<u16>,
    pub data: Option<&'a serde_json::Value>,
    pub response_headers: &'a [(String, String)],
    pub response_body: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::AiErrorReason;

    fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn retry_after_parses_ms_seconds_and_dates() {
        assert_eq!(retry_after_ms(&headers(&[("retry-after-ms", "1200")])), Some(1200.0));
        assert_eq!(retry_after_ms(&headers(&[("retry-after-ms", "-5")])), Some(0.0));
        assert_eq!(retry_after_ms(&headers(&[("retry-after", "30")])), Some(30_000.0));
        assert_eq!(retry_after_ms(&headers(&[("retry-after", "0.5")])), Some(500.0));
        assert_eq!(retry_after_ms(&headers(&[("retry-after", "garbage")])), None);
        assert_eq!(retry_after_ms(&headers(&[])), None);
        // HTTP date far in the future parses to a positive remaining time
        let future = "Fri, 01 Jan 2199 00:00:00 GMT";
        assert!(retry_after_ms(&headers(&[("retry-after", future)])).unwrap_or(0.0) > 0.0);
        // a date in the past clamps to zero
        let past = "Mon, 01 Jan 2001 00:00:00 GMT";
        assert_eq!(retry_after_ms(&headers(&[("retry-after", past)])), Some(0.0));
    }

    #[test]
    fn openai_rate_limit_headers_split_by_bucket() {
        let headers = headers(&[
            ("x-ratelimit-limit-requests", "10000"),
            ("x-ratelimit-remaining-requests", "9991"),
            ("x-ratelimit-reset-requests", "6m0s"),
            ("x-ratelimit-limit-tokens", "200000"),
            ("x-ratelimit-remaining-tokens", "199984"),
            ("x-ratelimit-reset-tokens", "6m0s"),
        ]);
        let details = rate_limit_details(&headers, None).unwrap();
        assert_eq!(details.limit.as_ref().unwrap()["requests"], "10000");
        assert_eq!(details.limit.as_ref().unwrap()["tokens"], "200000");
        assert_eq!(details.remaining.as_ref().unwrap()["requests"], "9991");
        assert_eq!(details.reset.as_ref().unwrap()["requests"], "6m0s");
        assert_eq!(details.retry_after_ms, None);
    }

    #[test]
    fn anthropic_rate_limit_headers_split_by_kind() {
        let headers = headers(&[
            ("anthropic-ratelimit-requests-limit", "1000"),
            ("anthropic-ratelimit-requests-remaining", "999"),
            ("anthropic-ratelimit-requests-reset", "2026-10-09T00:00:00Z"),
            ("anthropic-ratelimit-tokens-remaining", "99k"),
        ]);
        let details = rate_limit_details(&headers, Some(42.0)).unwrap();
        assert_eq!(details.limit.as_ref().unwrap()["requests"], "1000");
        assert_eq!(details.remaining.as_ref().unwrap()["requests"], "999");
        assert_eq!(details.reset.as_ref().unwrap()["requests"], "2026-10-09T00:00:00Z");
        assert_eq!(details.remaining.as_ref().unwrap()["tokens"], "99k");
        assert_eq!(details.retry_after_ms, Some(42.0));
    }

    #[test]
    fn rate_limit_details_none_without_signals() {
        assert!(
            rate_limit_details(&headers(&[("content-type", "text/event-stream")]), None).is_none()
        );
    }

    #[test]
    fn provider_message_prefers_body_messages_over_html_and_status() {
        assert_eq!(
            provider_message(401, Some("{\"error\":{\"message\":\"Invalid API Key\"}}")),
            "Invalid API Key"
        );
        assert_eq!(
            provider_message(502, Some("<!doctype html><html>gateway</html>")),
            "Provider request failed with HTTP 502"
        );
        assert_eq!(provider_message(500, None), "Provider request failed with HTTP 500");
        assert_eq!(
            provider_message(500, Some("boom")),
            "Provider request failed with HTTP 500: boom"
        );
    }

    #[test]
    fn http_failure_classifies_and_carries_rate_limit_telemetry() {
        let error = http_failure(HttpFailureInput {
            message: "Rate limit exceeded",
            url: Some("https://api.test/v1/chat"),
            status: Some(429),
            data: None,
            response_headers: &[("Retry-After".to_string(), "12".to_string())],
            response_body: None,
        });
        match error.reason {
            AiErrorReason::RateLimit { retry_after_ms, rate_limit, .. } => {
                assert_eq!(retry_after_ms, Some(12_000.0));
                let details = rate_limit.unwrap();
                assert_eq!(details.retry_after_ms, Some(12_000.0));
            }
            other => panic!("expected RateLimit, got {}", other.tag()),
        }
    }
}
