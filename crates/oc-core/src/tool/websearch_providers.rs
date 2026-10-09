//! Ports of `packages/core/src/plugin/websearch/*` — the search providers
//! (exa, firecrawl, parallel, tinyfish, tavily; the MCP-hosted provider is
//! TODO(wave-D)). Each provider implements the v2 HTTP contracts; API keys
//! resolve through a credential seam that defaults to keyless modes.

use std::time::Duration;

use oc_schema::misc::WebSearchResult;

/// A provider failure: HTTP status when available, else a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProviderError {
    pub status: Option<u16>,
    pub message: String,
}

impl ProviderError {
    fn request(status: Option<u16>, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

/// `WebSearchMcp.MAX_RESPONSE_BYTES`.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MCP_TIMEOUT_SECONDS: u64 = 25;

/// One provider implementation (`ProviderImplementation`).
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError>;
}

/// Resolves an API key for a provider id (integration connection seam).
/// `None` ⇒ keyless mode.
pub type Credential = Option<String>;

/// `WebSearchMcp.call` — a `tools/call` JSON-RPC POST over HTTP.
async fn mcp_call(
    url: &str,
    tool: &str,
    arguments: &serde_json::Value,
    headers: &[(&str, String)],
) -> Result<serde_json::Value, ProviderError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(MCP_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| ProviderError::request(None, error.to_string()))?;
    let mut request = client
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments },
        }));
    for (key, value) in headers {
        request = request.header(*key, value);
    }
    let response =
        request.send().await.map_err(|error| ProviderError::request(None, error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(ProviderError::request(
            Some(status.as_u16()),
            format!("{tool} request failed (HTTP {status})"),
        ));
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) =
        response.chunk().await.map_err(|error| ProviderError::request(None, error.to_string()))?
    {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(ProviderError::request(
                None,
                format!("{tool} response exceeded {MAX_RESPONSE_BYTES} bytes"),
            ));
        }
    }
    let text = String::from_utf8_lossy(&body).to_string();
    parse_mcp_response(&text, tool)
}

/// `WebSearchMcp.parseResponse` — direct `{result}` or SSE `data:` lines.
fn parse_mcp_response(body: &str, tool: &str) -> Result<serde_json::Value, ProviderError> {
    let parse = |payload: &str| -> Option<serde_json::Value> {
        let trimmed = payload.trim();
        if !trimmed.starts_with('{') {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .and_then(|value| value.get("result").cloned())
            .filter(|result| !result.is_null())
    };
    if let Some(result) = parse(body) {
        return Ok(result);
    }
    for line in body.split('\n') {
        let Some(data) = line.strip_prefix("data: ") else { continue };
        if let Some(result) = parse(data) {
            return Ok(result);
        }
    }
    Err(ProviderError::request(None, format!("{tool} request failed")))
}

fn authorization(key: &Credential) -> Vec<(&'static str, String)> {
    match key {
        Some(key) => vec![("Authorization", format!("Bearer {key}"))],
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Exa
// ---------------------------------------------------------------------------

pub struct ExaProvider {
    pub key: Credential,
}

pub const EXA_ENDPOINT: &str = "https://mcp.exa.ai/mcp";

#[async_trait::async_trait]
impl SearchProvider for ExaProvider {
    fn id(&self) -> &'static str {
        "exa"
    }
    fn name(&self) -> &'static str {
        "Exa"
    }
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
        let mut url = EXA_ENDPOINT.to_string();
        if let Some(key) = &self.key {
            url = format!("{EXA_ENDPOINT}?exaApiKey={}", urlencoding::encode(key));
        }
        let result = mcp_call(
            &url,
            "web_search_exa",
            &serde_json::json!({ "query": query, "numResults": 8 }),
            &[],
        )
        .await?;
        let content =
            result.get("content").and_then(|content| content.as_array()).and_then(|items| {
                items.iter().find(|item| {
                    item.get("text").and_then(|text| text.as_str()).is_some_and(|t| !t.is_empty())
                })
            });
        let Some(text) = content.and_then(|item| item.get("text")).and_then(|text| text.as_str())
        else {
            return Ok(Vec::new());
        };
        Ok(parse_exa_results(text))
    }
}

/// Exa's text block format: `URL:`/`Title:`/`Published:`/`Highlights:` lines
/// separated by blank `---` rules.
pub fn parse_exa_results(text: &str) -> Vec<WebSearchResult> {
    text.split("\n\n---\n\n")
        .filter_map(|block| {
            let field = |prefix: &str| -> Option<String> {
                block
                    .lines()
                    .find(|line| line.starts_with(prefix))
                    .map(|line| line[prefix.len()..].trim().to_string())
                    .filter(|value| !value.is_empty())
            };
            let url = field("URL:")?;
            let title = field("Title:").filter(|title| title != "N/A");
            let published_text = field("Published:").filter(|published| published != "N/A");
            let published = published_text.and_then(|published| parse_date_millis(&published));
            let content = ["Highlights:", "Text:"]
                .iter()
                .find_map(|prefix| {
                    let position = block.find(prefix)?;
                    let after = &block[position + prefix.len()..];
                    let after =
                        after.strip_prefix('\n').unwrap_or(after.trim_start_matches([' ', '\t']));
                    Some(after.trim().to_string())
                })
                .filter(|content| !content.is_empty());
            Some(WebSearchResult {
                url,
                title,
                content,
                time: oc_schema::misc::WebSearchResultTime { published },
            })
        })
        .collect()
}

fn parse_date_millis(value: &str) -> Option<u64> {
    // Date.parse approximation for ISO-8601 dates (UTC midnight).
    value.trim().parse::<chrono_like::Naive>().ok().map(|parsed| parsed.0)
}

mod chrono_like {
    pub struct Naive(pub u64);
}

impl std::str::FromStr for chrono_like::Naive {
    type Err = ();
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        // YYYY-MM-DD or with time — seconds since epoch (UTC).
        let date = &value[..10.min(value.len())];
        let mut parts = date.split('-');
        let year: i64 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        let month: i64 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        let day: i64 = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(());
        }
        // days since epoch (civil algorithm)
        let years = if month <= 2 { year - 1 } else { year };
        let era = if years >= 0 { years } else { years - 399 } / 400;
        let yoe = years - era * 400;
        let mp = (month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Ok(chrono_like::Naive((days * 86_400).max(0) as u64 * 1000))
    }
}

// ---------------------------------------------------------------------------
// Tavily
// ---------------------------------------------------------------------------

pub struct TavilyProvider {
    pub key: Credential,
}

pub const TAVILY_ENDPOINT: &str = "https://api.tavily.com/search";

#[async_trait::async_trait]
impl SearchProvider for TavilyProvider {
    fn id(&self) -> &'static str {
        "tavily"
    }
    fn name(&self) -> &'static str {
        "Tavily"
    }
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(MCP_TIMEOUT_SECONDS))
            .build()
            .map_err(|error| ProviderError::request(None, error.to_string()))?;
        let mut request = client
            .post(TAVILY_ENDPOINT)
            .header("Accept", "application/json")
            .header("X-Client-Name", "opencode2")
            .json(&serde_json::json!({
                "query": query,
                "search_depth": "basic",
                "chunks_per_source": 3,
                "max_results": 8,
            }));
        request = match &self.key {
            Some(key) => request.header("Authorization", format!("Bearer {key}")),
            None => request.header("X-Tavily-Access-Mode", "keyless"),
        };
        let response = request
            .send()
            .await
            .map_err(|error| ProviderError::request(None, error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ProviderError::request(
                Some(status.as_u16()),
                "Tavily web search request failed".to_string(),
            ));
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| ProviderError::request(None, error.to_string()))?;
        let results =
            body.get("results").and_then(|results| results.as_array()).cloned().unwrap_or_default();
        Ok(results
            .into_iter()
            .map(|item| WebSearchResult {
                url: item.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                title: item.get("title").and_then(|v| v.as_str()).map(String::from),
                content: item
                    .get("content")
                    .and_then(|v| v.as_str())
                    .filter(|content| !content.is_empty())
                    .map(String::from),
                time: oc_schema::misc::WebSearchResultTime { published: None },
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Firecrawl
// ---------------------------------------------------------------------------

pub struct FirecrawlProvider {
    pub key: Credential,
}

pub const FIRECRAWL_ENDPOINT: &str = "https://mcp.firecrawl.dev/v2/mcp";

#[async_trait::async_trait]
impl SearchProvider for FirecrawlProvider {
    fn id(&self) -> &'static str {
        "firecrawl"
    }
    fn name(&self) -> &'static str {
        "Firecrawl"
    }
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
        let headers = authorization(&self.key);
        let result = mcp_call(
            FIRECRAWL_ENDPOINT,
            "firecrawl_search",
            &serde_json::json!({ "query": query, "limit": 8 }),
            &headers,
        )
        .await?;
        let content = result
            .get("content")
            .and_then(|content| content.as_array())
            .and_then(|items| items.iter().find(|item| item.get("text").is_some()));
        let Some(text) = content.and_then(|item| item.get("text")).and_then(|text| text.as_str())
        else {
            return Ok(Vec::new());
        };
        let payload: Option<serde_json::Value> = serde_json::from_str(text).ok();
        Ok(payload
            .and_then(|payload| {
                payload.pointer("/data/web").and_then(|web| web.as_array()).cloned()
            })
            .unwrap_or_default()
            .into_iter()
            .map(|item| WebSearchResult {
                url: item.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                title: item.get("title").and_then(|v| v.as_str()).map(String::from),
                content: item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .filter(|description| !description.is_empty())
                    .map(String::from),
                time: oc_schema::misc::WebSearchResultTime { published: None },
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Parallel
// ---------------------------------------------------------------------------

pub struct ParallelProvider {
    pub key: Credential,
}

pub const PARALLEL_ENDPOINT: &str = "https://search.parallel.ai/mcp";

#[async_trait::async_trait]
impl SearchProvider for ParallelProvider {
    fn id(&self) -> &'static str {
        "parallel"
    }
    fn name(&self) -> &'static str {
        "Parallel"
    }
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
        let headers = authorization(&self.key);
        let result = mcp_call(
            PARALLEL_ENDPOINT,
            "web_search",
            &serde_json::json!({ "objective": query, "search_queries": [query] }),
            &headers,
        )
        .await?;
        let results = result
            .get("structuredContent")
            .and_then(|structured| structured.get("results"))
            .and_then(|results| results.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(results
            .into_iter()
            .map(|item| {
                let published = item
                    .get("publish_date")
                    .and_then(|v| v.as_str())
                    .and_then(|date| parse_date_millis(date));
                WebSearchResult {
                    url: item.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    title: item.get("title").and_then(|v| v.as_str()).map(String::from),
                    content: item
                        .get("excerpts")
                        .and_then(|v| v.as_array())
                        .filter(|excerpts| !excerpts.is_empty())
                        .map(|excerpts| {
                            excerpts
                                .iter()
                                .filter_map(|excerpt| excerpt.as_str())
                                .collect::<Vec<_>>()
                                .join("\n\n")
                        })
                        .filter(|content| !content.is_empty()),
                    time: oc_schema::misc::WebSearchResultTime { published },
                }
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// TinyFish
// ---------------------------------------------------------------------------

pub struct TinyFishProvider {
    pub key: Credential,
}

pub const TINYFISH_ENDPOINT: &str = "https://agent.tinyfish.ai/mcp";

#[async_trait::async_trait]
impl SearchProvider for TinyFishProvider {
    fn id(&self) -> &'static str {
        "tinyfish"
    }
    fn name(&self) -> &'static str {
        "TinyFish"
    }
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
        let headers: Vec<(&'static str, String)> = match &self.key {
            Some(key) => vec![("X-API-Key", key.clone())],
            None => vec![("X-TinyFish-Access-Mode", "keyless".to_string())],
        };
        let result =
            mcp_call(TINYFISH_ENDPOINT, "search", &serde_json::json!({ "query": query }), &headers)
                .await?;
        let content = result
            .get("content")
            .and_then(|content| content.as_array())
            .and_then(|items| items.iter().find(|item| item.get("text").is_some()));
        let Some(text) = content.and_then(|item| item.get("text")).and_then(|text| text.as_str())
        else {
            return Ok(Vec::new());
        };
        let payload: Option<serde_json::Value> = serde_json::from_str(text).ok();
        Ok(payload
            .and_then(|payload| {
                payload.get("results").and_then(|results| results.as_array()).cloned()
            })
            .unwrap_or_default()
            .into_iter()
            .map(|item| WebSearchResult {
                url: item.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                title: item.get("title").and_then(|v| v.as_str()).map(String::from),
                content: item
                    .get("snippet")
                    .and_then(|v| v.as_str())
                    .filter(|snippet| !snippet.is_empty())
                    .map(String::from),
                time: oc_schema::misc::WebSearchResultTime { published: None },
            })
            .collect())
    }
}

/// The default provider set (keyless until credentials land with wave D's
/// integration lane).
pub fn default_providers() -> Vec<std::sync::Arc<dyn SearchProvider>> {
    vec![
        std::sync::Arc::new(ExaProvider { key: None }),
        std::sync::Arc::new(FirecrawlProvider { key: None }),
        std::sync::Arc::new(ParallelProvider { key: None }),
        std::sync::Arc::new(TavilyProvider { key: None }),
        std::sync::Arc::new(TinyFishProvider { key: None }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_websearch_providers_parse_exa_blocks() {
        let text = "URL: https://a.example\nTitle: A\nPublished: 2024-01-15\nHighlights:\ngreat content\n\n---\n\nURL: https://b.example\nTitle: N/A\nText:\nbody";
        let results = parse_exa_results(text);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://a.example");
        assert_eq!(results[0].title.as_deref(), Some("A"));
        assert_eq!(results[0].content.as_deref(), Some("great content"));
        assert_eq!(results[0].time.published, Some(1_705_276_800_000));
        assert_eq!(results[1].title, None);
        assert_eq!(results[1].content.as_deref(), Some("body"));
    }

    #[test]
    fn tool_websearch_providers_parse_mcp_sse_and_direct() {
        let direct = r#"{"result":{"content":[{"type":"text","text":"x"}]}}"#;
        assert!(parse_mcp_response(direct, "t").unwrap().get("content").is_some());
        let sse = "event: message\ndata: {\"result\":{\"content\":[]}}\n";
        assert!(parse_mcp_response(sse, "t").unwrap().get("content").is_some());
        assert!(parse_mcp_response("garbage", "t").is_err());
    }

    #[test]
    fn tool_websearch_providers_default_set_sorted_by_service() {
        let providers = default_providers();
        let ids: Vec<&str> = providers.iter().map(|provider| provider.id()).collect();
        assert_eq!(ids, vec!["exa", "firecrawl", "parallel", "tavily", "tinyfish"]);
    }
}
