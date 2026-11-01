//! Port of `packages/core/src/tool/plugin/webfetch.ts` — fetch a URL and
//! return text/markdown/html (§4.9).

use std::sync::Arc;
use std::time::Duration;

use super::html_markdown::{convert_html_to_markdown, extract_text_from_html, MAX_BYTES};
use crate::permission::{AssertInput, PermissionError};
use crate::tool::{
    permission_cause, BuiltinTool, ToolExecuteWithContext, ToolFailure, ToolOutcome, ToolServices,
};

pub const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
pub const MAX_TIMEOUT_SECONDS: u64 = 120;

const OPENCODE_USER_AGENT: &str =
    "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko); compatible; OpenCode-User/1.0; +https://opencode.ai";

pub fn description() -> String {
    "Fetch content from an HTTP or HTTPS URL and return it as text, markdown, or HTML. Markdown is the default.\n\nUse a more targeted tool when one is available. This tool is read-only. Large text results may be replaced with a preview while the complete output is retained in managed storage.".to_string()
}

fn accept_header(format: &str) -> &'static str {
    match format {
        "markdown" => "text/markdown;q=1.0, text/x-markdown;q=0.9, text/plain;q=0.8, text/html;q=0.7, */*;q=0.1",
        "text" => "text/plain;q=1.0, text/markdown;q=0.9, text/html;q=0.8, */*;q=0.1",
        _ => "text/html;q=1.0, application/xhtml+xml;q=0.9, text/plain;q=0.8, text/markdown;q=0.7, */*;q=0.1",
    }
}

fn mime_from(content_type: &str) -> String {
    content_type.split(';').next().unwrap_or("").trim().to_lowercase()
}

fn is_image_attachment(mime: &str) -> bool {
    mime.starts_with("image/") && mime != "image/svg+xml" && mime != "image/vnd.fastbidsheet"
}

fn is_textual_mime(mime: &str) -> bool {
    mime.is_empty()
        || mime.starts_with("text/")
        || mime == "application/json"
        || mime.ends_with("+json")
        || mime == "application/xml"
        || mime.ends_with("+xml")
        || mime == "application/javascript"
        || mime == "application/x-javascript"
}

/// The webfetch leaf executor.
pub struct WebFetchExecute;

fn convert(content: &str, content_type: &str, format: &str) -> String {
    if !content_type.contains("text/html") {
        return content.to_string();
    }
    match format {
        "markdown" => convert_html_to_markdown(content),
        "text" => extract_text_from_html(content),
        _ => content.to_string(),
    }
}

#[derive(serde::Deserialize)]
struct Input {
    url: String,
    #[serde(default = "default_format")]
    format: String,
    timeout: Option<u64>,
}

/// The permission-assert metadata (G-74): ref webfetch.ts:128
/// `metadata: input` — the FULL input record (url, format, timeout).
fn permission_metadata(input: &Input) -> serde_json::Value {
    serde_json::json!({
        "url": input.url,
        "format": input.format,
        "timeout": input.timeout,
    })
}

fn default_format() -> String {
    "markdown".into()
}

/// AUD-19②: ref validates `timeout` against the input schema
/// (webfetch.ts:29 — Finite, > 0, ≤ MAX_TIMEOUT_SECONDS) instead of
/// clamping; out-of-range values fail the decode.
fn resolve_timeout(timeout: Option<u64>) -> Result<u64, String> {
    let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    if timeout == 0 || timeout > MAX_TIMEOUT_SECONDS {
        return Err(format!(
            "Invalid arguments: timeout must be a number greater than 0 and at most {MAX_TIMEOUT_SECONDS}"
        ));
    }
    Ok(timeout)
}

/// AUD-19①: a declared Content-Length above the cap rejects without
/// downloading (http-body.ts:11-15). Non-numeric headers parse to `None`
/// (ref's NaN → undefined) and fall through to the streamed cap.
fn content_length_exceeds(headers: &reqwest::header::HeaderMap, maximum: usize) -> bool {
    headers
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|declared| declared > maximum as u64)
}

async fn fetch(url: &str, format: &str, timeout: u64) -> Result<(Vec<u8>, String), String> {
    let client = oc_http::builder()
        // G-89 — the reqwest client timeout is a BACKSTOP above the
        // whole-flow budget (execute wraps fetch+convert in a tokio
        // timeout below, the port of webfetch.ts:146-148
        // `Effect.timeoutOrElse`): the budget must fire first so the
        // elapsed case surfaces as "Request timed out", not the
        // client's per-request error.
        .timeout(Duration::from_secs(timeout.saturating_add(5)))
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = client
        .get(url)
        .header("User-Agent", OPENCODE_USER_AGENT)
        .header("Accept", accept_header(format))
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    // cf-mitigated challenge (403 + `cf-mitigated: challenge`) retries with
    // the `opencode` UA.
    let status = response.status();
    let mitigated = status.as_u16() == 403
        && response
            .headers()
            .get("cf-mitigated")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("challenge"));
    if mitigated {
        response = client
            .get(url)
            .header("User-Agent", "opencode")
            .header("Accept", accept_header(format))
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await
            .map_err(|error| error.to_string())?;
    }
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HttpClientError: {status}"));
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mime = mime_from(&content_type);
    if is_image_attachment(&mime) {
        return Err(format!("Unsupported fetched image content type: {mime}"));
    }
    if !is_textual_mime(&mime) {
        return Err(format!("Unsupported fetched file content type: {mime}"));
    }
    // AUD-19①: reject on the declared Content-Length before reading the
    // body (http-body.ts:11-15) — no streaming to the cap first.
    if content_length_exceeds(response.headers(), MAX_BYTES) {
        return Err(format!("Response too large (exceeds {MAX_BYTES} byte limit)"));
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            return Err(format!("Response too large (exceeds {MAX_BYTES} byte limit)"));
        }
    }
    Ok((body, content_type))
}

impl ToolExecuteWithContext for WebFetchExecute {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a serde_json::Value,
        context: &'a crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        let input = input.clone();
        let context = context.clone();
        Box::pin(async move {
            let input: Input = serde_json::from_value(input)
                .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            // URL must use http:// or https://. `new URL(...)` lowercases the
            // scheme (webfetch.ts:70-72), so `HTTP://` is accepted — compare
            // case-insensitively.
            let lowered = input.url.to_ascii_lowercase();
            if !lowered.starts_with("http://") && !lowered.starts_with("https://") {
                return Err(ToolFailure {
                    message: format!("Unable to fetch {}", input.url),
                    cause: Some(super::FailureCause::Nested(
                        "URL must use http:// or https://".into(),
                    )),
                });
            }
            services
                .permission
                .assert(AssertInput {
                    id: None,
                    session_id: context.session_id.clone(),
                    agent: Some(context.agent.clone()),
                    action: "webfetch".into(),
                    resources: vec![input.url.clone()],
                    save: Some(vec!["*".into()]),
                    metadata: Some(
                        // ref webfetch.ts:128 `metadata: input` — the FULL
                        // input record (url, format, timeout), not a
                        // hand-picked subset (G-74).
                        serde_json::from_value(permission_metadata(&input))
                            .unwrap_or_default(),
                    ),
                    source: Some(oc_schema::permission::Source::Tool {
                        message_id: context.message_id.clone(),
                        id: context.call_id.clone(),
                    }),
                })
                .await
                .map_err(|error: PermissionError| ToolFailure {
                    message: format!("Unable to fetch {}", input.url),
                    cause: Some(permission_cause(&error)),
                })?;

            // AUD-19②: schema-style validation, never clamping
            // (webfetch.ts:29).
            let timeout = resolve_timeout(input.timeout).map_err(ToolFailure::message)?;
            // G-89 — ref webfetch.ts:146-148: `Effect.timeoutOrElse` wraps
            // the WHOLE fetch + collect flow; an elapsed budget fails with
            // "Request timed out" (not the reqwest client timeout's
            // per-request error). Convert runs inside the same budget;
            // non-timeout failures keep the url-specific message.
            let (content_type, output) = match tokio::time::timeout(
                Duration::from_secs(timeout),
                async {
                    let (body, content_type) =
                        fetch(&input.url, &input.format, timeout).await.map_err(|error| {
                            ToolFailure {
                                message: format!("Unable to fetch {}", input.url),
                                cause: Some(super::FailureCause::Nested(error)),
                            }
                        })?;
                    let content = String::from_utf8_lossy(&body).to_string();
                    let output = convert(&content, &content_type, &input.format);
                    Ok::<_, ToolFailure>((content_type, output))
                },
            )
            .await
            {
                Ok(result) => result?,
                Err(_elapsed) => {
                    return Err(ToolFailure {
                        message: "Request timed out".into(),
                        cause: Some(super::FailureCause::Nested("Request timed out".into())),
                    })
                }
            };
            Ok(ToolOutcome {
                content: vec![oc_schema::tool::Content::Text { text: output.clone() }],
                metadata: Some(
                    serde_json::from_value(serde_json::json!({ "contentType": content_type }))
                        .unwrap_or_default(),
                ),
                output: Some(serde_json::json!({
                    "url": input.url,
                    "contentType": content_type,
                    "format": input.format,
                    "output": output,
                })),
            })
        })
    }
}

/// The webfetch builtin registration.
pub fn tool() -> BuiltinTool {
    BuiltinTool {
        name: "webfetch",
        description: description(),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The HTTP or HTTPS URL to fetch content from" },
                "format": { "type": "string", "enum": ["text", "markdown", "html"], "default": "markdown", "description": "The format to return the content in. Defaults to markdown." },
                "timeout": { "type": "integer", "minimum": 1, "maximum": MAX_TIMEOUT_SECONDS, "description": "Optional timeout in seconds (maximum: 120)" }
            },
            "required": ["url"]
        }),
        permission: Some("webfetch"),
        codemode: false,
        namespace: None,
        execute: Arc::new(WebFetchExecute),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_webfetch_extract_text_skips_active_content() {
        let html = "<h1>Hello</h1><script>bad()</script><p>world <strong>wide</strong> <product-name>today</product-name></p><style>.bad {}</style>";
        assert_eq!(extract_text_from_html(html), "Helloworld wide today");
        assert_eq!(convert_html_to_markdown(html), "# Hello\n\nworld **wide** today");
    }

    #[test]
    fn tool_webfetch_mime_classification() {
        assert!(is_image_attachment("image/png"));
        assert!(!is_image_attachment("image/svg+xml"));
        assert!(is_textual_mime("text/html"));
        assert!(is_textual_mime("application/json"));
        assert!(is_textual_mime(""));
        assert!(!is_textual_mime("application/octet-stream"));
    }

    /// G-74: the permission-assert metadata is the FULL input record
    /// (ref webfetch.ts:128 `metadata: input`) — timeout included.
    #[test]
    fn tool_webfetch_permission_metadata_carries_full_input() {
        let explicit = Input { url: "https://example.com".into(), format: "html".into(), timeout: Some(9) };
        assert_eq!(
            super::permission_metadata(&explicit),
            serde_json::json!({ "url": "https://example.com", "format": "html", "timeout": 9 })
        );
        let defaulted = Input { url: "https://example.com".into(), format: "markdown".into(), timeout: None };
        assert_eq!(
            super::permission_metadata(&defaulted)["timeout"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn tool_webfetch_convert_dispatch() {
        assert_eq!(convert("<p>a</p>", "text/html", "markdown"), "a");
        assert_eq!(convert("<p>a</p>", "text/html", "html"), "<p>a</p>");
        assert_eq!(convert("plain", "text/plain", "markdown"), "plain");
    }

    #[test]
    fn tool_webfetch_accept_headers() {
        assert!(accept_header("markdown").starts_with("text/markdown;q=1.0"));
        assert!(accept_header("html").starts_with("text/html;q=1.0"));
    }

    #[test]
    fn tool_webfetch_timeout_validates_not_clamps() {
        // AUD-19②: ref's schema (webfetch.ts:29) rejects instead of
        // clamping — 0 and > MAX_TIMEOUT_SECONDS fail, 1/MAX/default pass.
        assert_eq!(resolve_timeout(None), Ok(DEFAULT_TIMEOUT_SECONDS));
        assert_eq!(resolve_timeout(Some(1)), Ok(1));
        assert_eq!(resolve_timeout(Some(MAX_TIMEOUT_SECONDS)), Ok(MAX_TIMEOUT_SECONDS));
        assert!(resolve_timeout(Some(0)).is_err());
        assert!(resolve_timeout(Some(MAX_TIMEOUT_SECONDS + 1)).is_err());
        assert!(resolve_timeout(Some(u64::MAX)).is_err());
    }

    #[test]
    fn tool_webfetch_content_length_precheck() {        // AUD-19①: a declared Content-Length over the cap rejects without
        // downloading (http-body.ts:11-15); under/at the cap, absent or
        // non-numeric headers fall through to the streamed cap.
        let headers = |value: &str| {
            let mut map = reqwest::header::HeaderMap::new();
            map.insert(
                "content-length",
                reqwest::header::HeaderValue::from_str(value).expect("header value"),
            );
            map
        };
        assert!(content_length_exceeds(&headers(&(MAX_BYTES + 1).to_string()), MAX_BYTES));
        assert!(content_length_exceeds(&headers("999999999999"), MAX_BYTES));
        assert!(!content_length_exceeds(&headers(&MAX_BYTES.to_string()), MAX_BYTES));
        assert!(!content_length_exceeds(&headers("1024"), MAX_BYTES));
        let mut empty = reqwest::header::HeaderMap::new();
        empty.insert("x-other", reqwest::header::HeaderValue::from_static("1"));
        assert!(!content_length_exceeds(&empty, MAX_BYTES));
    }

    /// G-89 — the whole-flow budget (fetch + collect + convert,
    /// webfetch.ts:146-148): an elapsed budget fails with the ref
    /// message "Request timed out", while a refused connection keeps
    /// the url-specific "Unable to fetch {url}".
    #[tokio::test]
    async fn tool_webfetch_timeout_budget_message() {
        let harness = crate::tool::tests::harness().await;
        // webfetch is an engine plugin (not in register_all) — register
        // the leaf directly.
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        crate::tool::register_builtin(
            &harness.tools,
            &hooks,
            &harness.services,
            crate::tool::webfetch::tool(),
        )
        .expect("register webfetch");
        // A server that accepts and then stalls (holds the socket open,
        // never responds) — the budget must elapse first.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                std::thread::sleep(std::time::Duration::from_secs(10));
                drop(stream);
            }
        });
        let stalled = format!("http://127.0.0.1:{port}/slow");
        let failure = crate::tool::tests::call(
            &harness.tools,
            "webfetch",
            serde_json::json!({ "url": stalled, "timeout": 1 }),
        )
        .await
        .expect_err("budget elapses");
        assert_eq!(failure.message(), "Request timed out");

        // Non-timeout failures keep the url-specific message.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let refused = format!("http://127.0.0.1:{port}/x");
        let failure = crate::tool::tests::call(
            &harness.tools,
            "webfetch",
            serde_json::json!({ "url": refused, "timeout": 5 }),
        )
        .await
        .expect_err("connection refused");
        // Non-timeout failures keep the url-specific ToolFailure: the
        // nested transport cause (carrying the url) surfaces, not the
        // timeout message.
        let message = failure.message();
        assert!(
            message.contains(&port.to_string()) && !message.contains("Request timed out"),
            "unexpected refused message: {message}"
        );
    }
}
