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

fn default_format() -> String {
    "markdown".into()
}

async fn fetch(url: &str, format: &str, timeout: u64) -> Result<(Vec<u8>, String), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
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
            // URL must use http:// or https://.
            if !input.url.starts_with("http://") && !input.url.starts_with("https://") {
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
                        serde_json::from_value(
                            serde_json::json!({ "url": input.url, "format": input.format }),
                        )
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

            let timeout =
                input.timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS).clamp(1, MAX_TIMEOUT_SECONDS);
            let (body, content_type) =
                fetch(&input.url, &input.format, timeout).await.map_err(|error| ToolFailure {
                    message: format!("Unable to fetch {}", input.url),
                    cause: Some(super::FailureCause::Nested(error)),
                })?;
            let content = String::from_utf8_lossy(&body).to_string();
            let output = convert(&content, &content_type, &input.format);
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
}
