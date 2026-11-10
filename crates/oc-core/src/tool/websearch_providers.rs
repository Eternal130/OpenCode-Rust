//! Ports of `packages/core/src/plugin/websearch/*` — the search providers
//! (exa, firecrawl, parallel, tinyfish, tavily). Four of them are
//! "MCP-hosted" in v2 terms: `plugin/websearch/mcp.ts` (`WebSearchMcp.call`)
//! is a plain HTTP `tools/call` JSON-RPC POST to the provider's fixed
//! endpoint (NOT a user-configured MCP server — v2.0.24 never routes
//! websearch through `Mcp.Service`; that seam does not exist upstream, so
//! this port does not invent one). Each provider implements the v2 HTTP
//! contracts; API keys resolve through the v2 env connection-method names
//! (the integration host itself is a later lane).

use std::time::Duration;

use oc_schema::misc::WebSearchResult;

/// A provider failure: HTTP status when available, else a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ProviderError {
    pub status: Option<u16>,
    pub message: String,
    /// G-224① — the raw `Retry-After` response header (seconds or
    /// HTTP-date), consumed by the cooldown rotation
    /// (`websearch.ts:204` reads `cause.response.headers["retry-after"]`).
    pub retry_after: Option<String>,
}

impl ProviderError {
    fn request(status: Option<u16>, message: impl Into<String>) -> Self {
        Self { status, message: message.into(), retry_after: None }
    }

    /// A non-success response carrying its `Retry-After` header.
    fn request_with_retry_after(
        status: Option<u16>,
        message: impl Into<String>,
        retry_after: Option<String>,
    ) -> Self {
        Self { status, message: message.into(), retry_after }
    }
}

/// The raw `Retry-After` header off a response, when present.
fn retry_after_header(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers.get("retry-after").and_then(|value| value.to_str().ok()).map(str::to_string)
}

/// `WebSearchMcp.MAX_RESPONSE_BYTES`.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MCP_TIMEOUT_SECONDS: u64 = 25;

/// One provider implementation (`ProviderImplementation`).
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    /// `&str` (not `&'static`) so plugin-registered providers (G-24's
    /// guest-backed `GuestWebSearchProvider`) can carry owned ids.
    fn id(&self) -> &str;
    fn name(&self) -> &str;
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
    let client = oc_http::builder()
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
        let retry_after = retry_after_header(response.headers());
        return Err(ProviderError::request_with_retry_after(
            Some(status.as_u16()),
            format!("{tool} request failed (HTTP {status})"),
            retry_after,
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
    /// Overridable for tests; defaults to [`EXA_ENDPOINT`].
    pub endpoint: String,
}

pub const EXA_ENDPOINT: &str = "https://mcp.exa.ai/mcp";

impl ExaProvider {
    pub fn new(key: Credential) -> Self {
        Self { key, endpoint: EXA_ENDPOINT.to_string() }
    }
}

#[async_trait::async_trait]
impl SearchProvider for ExaProvider {
    fn id(&self) -> &'static str {
        "exa"
    }
    fn name(&self) -> &'static str {
        "Exa"
    }
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
        let mut url = self.endpoint.clone();
        if let Some(key) = &self.key {
            url = format!("{}?exaApiKey={}", url, urlencoding::encode(key));
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

/// G-282④ — ref uses `Date.parse(publishedText)` (plugin/websearch/exa.ts:71),
/// which accepts far more than an ISO date prefix: ISO datetimes with
/// optional time/offset, RFC-822/1123 (`Mon, 09 Nov 2026 12:00:00 GMT`),
/// and long-month forms (`November 9, 2026`). The old parser only read
/// the leading `YYYY-MM-DD`. Returns milliseconds since the Unix epoch.
fn parse_date_millis(value: &str) -> Option<u64> {
    let value = value.trim();
    parse_iso_datetime(value).or_else(|| parse_loose_date(value))
}

/// `YYYY-MM-DD` or `YYYY-MM-DD[T ]HH:MM[:SS[.frac]][Z|±HH[:MM]]`.
fn parse_iso_datetime(value: &str) -> Option<u64> {
    let (date, rest) = value.split_once(['T', ' ']).unwrap_or((value, ""));
    let days = parse_ymd(date)?;
    if rest.trim().is_empty() {
        return days_to_millis(days, 0, 0, 0, 0);
    }
    // Strip a trailing timezone off the clock: `Z`, `+HH:MM`, `-HHMM`, or
    // a zone word (GMT/UTC treated as +0000 like V8's fallback).
    let (clock, offset_minutes) = split_offset(rest)?;
    let (hour, minute, second) = parse_clock(clock)?;
    days_to_millis(days, hour, minute, second, offset_minutes)
}

/// `(clock, offset-minutes)` — an unrecognized trailing token (a named
/// zone we do not model) rejects like V8's parser would for ambiguous
/// input; GMT/UTC/Z and numeric offsets decode.
fn split_offset(rest: &str) -> Option<(&str, i64)> {
    let rest = rest.trim();
    if rest.is_empty() {
        // No offset: V8 treats a date-time without one as LOCAL time;
        // the sandboxed process runs UTC, so ±0 is the honest decode.
        return Some((rest, 0));
    }
    let upper = rest.to_ascii_uppercase();
    if upper.ends_with("GMT") || upper.ends_with("UTC") {
        return Some((&rest[..rest.len() - 3], 0));
    }
    if let Some(clock) = rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
        return Some((clock, 0));
    }
    // Numeric offset ±HH[:MM] / ±HHMM AFTER the clock (clocks contain
    // no `+`/`-`; the found position is a char boundary since both are
    // ASCII).
    let position = rest.find(['+', '-'])?;
    let (clock, offset_text) = rest.split_at(position);
    let sign = if offset_text.starts_with('+') { 1_i64 } else { -1_i64 };
    let digits = offset_text[1..].trim();
    let (hours, minutes) = if let Some((h, m)) = digits.split_once(':') {
        (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)
    } else if digits.len() == 4 && digits.is_ascii() {
        (digits[..2].parse::<i64>().ok()?, digits[2..].parse::<i64>().ok()?)
    } else if digits.len() <= 2 {
        (digits.parse::<i64>().ok()?, 0)
    } else {
        return None;
    };
    Some((clock, sign * (hours * 60 + minutes)))
}

fn parse_clock(clock: &str) -> Option<(i64, i64, i64)> {
    let clock = clock.trim();
    // Fractional seconds truncate to whole seconds (millisecond
    // precision; the result is ms-epoch anyway).
    let seconds = clock.split('.').next().unwrap_or(clock);
    let mut parts = seconds.split(':');
    let hour: i64 = parts.next()?.trim().parse().ok()?;
    let minute: i64 = parts.next().map(str::trim).map(str::parse).transpose().ok()??;
    let second: i64 = parts.next().map(str::trim).map(str::parse).transpose().ok()?.unwrap_or(0);
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }
    Some((hour, minute, second))
}

/// `[Www, ]D Mon YYYY[ HH:MM[:SS]][ zone]` and `Mon D, YYYY` (the V8
/// legacy `Date.parse` classes Exa's human-readable dates land in).
fn parse_loose_date(value: &str) -> Option<u64> {
    let mut text = value.trim();
    // Optional weekday prefix `Mon`/`Monday` (with or without a comma).
    let first_word: String = text.chars().take_while(|char| char.is_alphabetic()).collect();
    if is_weekday_prefix(&first_word) {
        text = text.strip_prefix(&first_word).unwrap_or(text);
        text = text.trim_start();
        text = text.strip_prefix(',').map(str::trim_start).unwrap_or(text);
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    // `D Mon YYYY [clock]` or `Mon D, YYYY [clock]`.
    let (day, month, year, clock) = if tokens.len() >= 3 {
        if let Some(month) = month_number(tokens[0]) {
            let day: i64 = tokens[1].trim_matches(',').parse().ok()?;
            let year: i64 = tokens[2].parse().ok()?;
            (day, month, year, tokens.get(3..))
        } else {
            let month = month_number(tokens[1].trim_matches(','))?;
            let day: i64 = tokens[0].parse().ok()?;
            let year: i64 = tokens[2].parse().ok()?;
            (day, month, year, tokens.get(3..))
        }
    } else {
        return None;
    };
    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return None;
    }
    let days = civil_days(year, month, day)?;
    let (hour, minute, second) = match clock {
        Some([clock, ..]) => parse_clock(clock)?,
        _ => (0, 0, 0),
    };
    days_to_millis(days, hour, minute, second, 0)
}

/// A weekday prefix for stripping: the 3-letter form or a full name
/// (`Mon`, `Monday`, …).
fn is_weekday_prefix(word: &str) -> bool {
    let lowered = word.to_ascii_lowercase();
    lowered.len() >= 3
        && ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"]
            .iter()
            .any(|day| lowered == day[..3] || *day == lowered)
}

/// Month number for a full or 3-letter English month name.
fn month_number(token: &str) -> Option<i64> {
    let lowered = token.to_ascii_lowercase();
    if lowered.is_empty() {
        return None;
    }
    const MONTHS: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    MONTHS
        .iter()
        .position(|month| *month == lowered || (lowered.len() == 3 && month.starts_with(&lowered)))
        .map(|index| index as i64 + 1)
}

fn parse_ymd(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    civil_days(year, month, day)
}

/// Days since the Unix epoch for a civil (year, month, day) — the
/// Howard Hinnant civil algorithm (UTC midnight).
fn civil_days(year: i64, month: i64, day: i64) -> Option<i64> {
    let years = if month <= 2 { year - 1 } else { year };
    let era = if years >= 0 { years } else { years - 399 } / 400;
    let yoe = years - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn days_to_millis(
    days: i64,
    hour: i64,
    minute: i64,
    second: i64,
    offset_minutes: i64,
) -> Option<u64> {
    let millis =
        ((days * 86_400) + hour * 3_600 + minute * 60 + second - offset_minutes * 60) * 1000;
    u64::try_from(millis).ok()
}

// ---------------------------------------------------------------------------
// Tavily
// ---------------------------------------------------------------------------

pub struct TavilyProvider {
    pub key: Credential,
    /// Overridable for tests; defaults to [`TAVILY_ENDPOINT`].
    pub endpoint: String,
    /// G-224③ — the app user-agent (ref tavily.ts:53-54 sends
    /// `App.useragent(ctx.app)` alongside `X-Client-Name`).
    pub user_agent: String,
    /// The request budget (ref tavily.ts:72 `Duration.seconds(25)`);
    /// overridable for tests.
    pub timeout: Duration,
}

pub const TAVILY_ENDPOINT: &str = "https://api.tavily.com/search";

impl TavilyProvider {
    pub fn new(key: Credential) -> Self {
        Self {
            key,
            endpoint: TAVILY_ENDPOINT.to_string(),
            user_agent: crate::session::runner::model_request::useragent(
                &crate::session::runner::model_request::AppMetadata::detected(),
            ),
            timeout: Duration::from_secs(MCP_TIMEOUT_SECONDS),
        }
    }

    /// G-224③ — stamp the engine's app identity (`App.useragent`).
    #[must_use]
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }
}

#[async_trait::async_trait]
impl SearchProvider for TavilyProvider {
    fn id(&self) -> &'static str {
        "tavily"
    }
    fn name(&self) -> &'static str {
        "Tavily"
    }
    async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
        let client = oc_http::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|error| ProviderError::request(None, error.to_string()))?;
        let mut request = client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .header("User-Agent", &self.user_agent)
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
        let response = request.send().await.map_err(|error| {
            // G-310 — the tavily branch uses the dedicated timeout
            // message (ref tavily.ts:71-74), not the generic transport
            // display.
            if error.is_timeout() {
                ProviderError::request(None, "Tavily web search request timed out".to_string())
            } else {
                ProviderError::request(None, error.to_string())
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            let retry_after = retry_after_header(response.headers());
            return Err(ProviderError::request_with_retry_after(
                Some(status.as_u16()),
                "Tavily web search request failed".to_string(),
                retry_after,
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
    /// Overridable for tests; defaults to [`FIRECRAWL_ENDPOINT`].
    pub endpoint: String,
}

pub const FIRECRAWL_ENDPOINT: &str = "https://mcp.firecrawl.dev/v2/mcp";

impl FirecrawlProvider {
    pub fn new(key: Credential) -> Self {
        Self { key, endpoint: FIRECRAWL_ENDPOINT.to_string() }
    }
}

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
            &self.endpoint,
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
    /// Overridable for tests; defaults to [`PARALLEL_ENDPOINT`].
    pub endpoint: String,
}

pub const PARALLEL_ENDPOINT: &str = "https://search.parallel.ai/mcp";

impl ParallelProvider {
    pub fn new(key: Credential) -> Self {
        Self { key, endpoint: PARALLEL_ENDPOINT.to_string() }
    }
}

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
            &self.endpoint,
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
    /// Overridable for tests; defaults to [`TINYFISH_ENDPOINT`].
    pub endpoint: String,
}

pub const TINYFISH_ENDPOINT: &str = "https://agent.tinyfish.ai/mcp";

impl TinyFishProvider {
    pub fn new(key: Credential) -> Self {
        Self { key, endpoint: TINYFISH_ENDPOINT.to_string() }
    }
}

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
            mcp_call(&self.endpoint, "search", &serde_json::json!({ "query": query }), &headers)
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

/// The v2 integration `env` connection-method names — one provider is
/// eligible when its key resolves through one of these
/// (`plugin/websearch/*.ts`: `editor.method.update({ integrationID,
/// method: { type: "env", names: [...] } })`). The integration host is a
/// later lane, so this port resolves the same names from the process
/// environment.
pub fn env_key_name(id: &str) -> Option<&'static str> {
    match id {
        "exa" => Some("EXA_API_KEY"),
        "firecrawl" => Some("FIRECRAWL_API_KEY"),
        "parallel" => Some("PARALLEL_API_KEY"),
        "tavily" => Some("TAVILY_API_KEY"),
        "tinyfish" => Some("TINYFISH_API_KEY"),
        _ => None,
    }
}

/// The configured provider set: every provider whose API key resolves is
/// registered (v2 registers the provider unconditionally and resolves the
/// credential at execute time through `integration.connection.active`;
/// without the integration host, key presence is the eligibility gate and
/// an empty set leaves the typed `ProviderRequired` error path).
pub fn providers_with_keys(
    key_for: impl Fn(&str) -> Option<String>,
) -> Vec<std::sync::Arc<dyn SearchProvider>> {
    providers_with_keys_and_useragent(key_for, None)
}

/// G-224③ — [`providers_with_keys`] with the engine's app user-agent
/// stamped on the tavily provider (`App.useragent(ctx.app)`).
pub fn providers_with_keys_and_useragent(
    key_for: impl Fn(&str) -> Option<String>,
    user_agent: Option<String>,
) -> Vec<std::sync::Arc<dyn SearchProvider>> {
    let mut providers = Vec::new();
    for id in ["exa", "firecrawl", "parallel", "tavily", "tinyfish"] {
        let Some(key) = key_for(id) else { continue };
        let provider: std::sync::Arc<dyn SearchProvider> = match id {
            "exa" => std::sync::Arc::new(ExaProvider::new(Some(key))),
            "firecrawl" => std::sync::Arc::new(FirecrawlProvider::new(Some(key))),
            "parallel" => std::sync::Arc::new(ParallelProvider::new(Some(key))),
            "tavily" => {
                let mut tavily = TavilyProvider::new(Some(key));
                if let Some(user_agent) = &user_agent {
                    tavily = tavily.with_user_agent(user_agent.clone());
                }
                std::sync::Arc::new(tavily)
            }
            _ => std::sync::Arc::new(TinyFishProvider::new(Some(key))),
        };
        providers.push(provider);
    }
    providers
}

/// The wired default: env-key resolution (v2 env connection methods).
pub fn providers_from_env() -> Vec<std::sync::Arc<dyn SearchProvider>> {
    providers_from_env_with_useragent(None)
}

/// [`providers_from_env`] with the engine's app user-agent (G-224③).
pub fn providers_from_env_with_useragent(
    user_agent: Option<&str>,
) -> Vec<std::sync::Arc<dyn SearchProvider>> {
    providers_with_keys_and_useragent(
        |id| {
            env_key_name(id)
                .and_then(|name| std::env::var(name).ok())
                .filter(|key| !key.trim().is_empty())
        },
        user_agent.map(str::to_string),
    )
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

    /// G-282④ — `Date.parse` format classes beyond the bare ISO date
    /// prefix (exa.ts:71): datetimes with offsets, RFC-822 forms, and
    /// long-month forms all decode.
    #[test]
    fn tool_websearch_exa_dates_parse_dateparse_classes() {
        // ISO datetime, Z.
        assert_eq!(parse_date_millis("2024-01-15T12:30:45Z"), Some(1_705_321_845_000));
        // ISO datetime with a numeric offset (+02:00 shifts back).
        assert_eq!(parse_date_millis("2024-01-15T12:30:45+02:00"), Some(1_705_314_645_000));
        // Space separator + GMT zone word.
        assert_eq!(parse_date_millis("2024-01-15 12:30:45 GMT"), Some(1_705_321_845_000));
        // RFC-822/1123.
        assert_eq!(parse_date_millis("Mon, 15 Jan 2024 12:30:45 GMT"), Some(1_705_321_845_000));
        // Long month, day-first and month-first.
        assert_eq!(parse_date_millis("15 January 2024"), Some(1_705_276_800_000));
        assert_eq!(parse_date_millis("January 15, 2024"), Some(1_705_276_800_000));
        // The legacy date-only shape keeps its epoch.
        assert_eq!(parse_date_millis("2024-01-15"), Some(1_705_276_800_000));
        // Garbage still rejects.
        assert_eq!(parse_date_millis("not a date"), None);
        assert_eq!(parse_date_millis("2024-13-99"), None);
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
        // v2 order (`WebSearchPlugins`): exa, firecrawl, parallel, tavily,
        // tinyfish — filtered to the providers whose key resolves.
        let providers = providers_with_keys(|id| (id != "parallel").then(|| "k".into()));
        let ids: Vec<&str> = providers.iter().map(|provider| provider.id()).collect();
        assert_eq!(ids, vec!["exa", "firecrawl", "tavily", "tinyfish"]);
        // No configured key anywhere ⇒ no eligible provider (the typed
        // `ProviderRequired` path).
        assert!(providers_with_keys(|_| None).is_empty());
        // The env-key table matches the v2 integration env method names.
        for (id, name) in [
            ("exa", "EXA_API_KEY"),
            ("firecrawl", "FIRECRAWL_API_KEY"),
            ("parallel", "PARALLEL_API_KEY"),
            ("tavily", "TAVILY_API_KEY"),
            ("tinyfish", "TINYFISH_API_KEY"),
        ] {
            assert_eq!(env_key_name(id), Some(name));
        }
    }

    // -------------------------------------------------------------------------
    // MCP-hosted path against a fake in-process MCP HTTP server
    // (the repo's hand-rolled TcpListener harness pattern, e.g.
    // engine/skills_url.rs `CatalogServer`).
    // -------------------------------------------------------------------------

    struct FakeMcpServer {
        base: String,
        state: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        handle: tokio::task::JoinHandle<()>,
    }

    impl Drop for FakeMcpServer {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    /// Serves `body` with `status` on every POST, recording each raw
    /// request (request line + headers + body). `sse` wraps the body in a
    /// `data:` line like real streamable-HTTP MCP responses.
    async fn fake_mcp(status: &str, body: String, sse: bool) -> FakeMcpServer {
        fake_mcp_with_headers(status, body, sse, "").await
    }

    /// G-224 — the fake server with extra response headers (e.g.
    /// `Retry-After`).
    async fn fake_mcp_with_headers(
        status: &str,
        body: String,
        sse: bool,
        extra_headers: &str,
    ) -> FakeMcpServer {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let state = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = std::sync::Arc::clone(&state);
        let status = status.to_string();
        let extra_headers = extra_headers.to_string();
        let handle = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = vec![0u8; 65_536];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                recorded.lock().unwrap().push(request);
                let payload =
                    if sse { format!("event: message\ndata: {body}\n\n") } else { body.clone() };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        FakeMcpServer { base: format!("http://127.0.0.1:{port}"), state, handle }
    }

    fn last_request(server: &FakeMcpServer) -> String {
        server.state.lock().unwrap().last().expect("recorded request").clone()
    }

    /// G-224③ — the tavily request carries the app user-agent
    /// (`App.useragent`, ref tavily.ts:53-54) alongside X-Client-Name.
    #[tokio::test]
    async fn g224_tavily_sends_app_user_agent() {
        let body = r#"{"results":[]}"#.to_string();
        let server = fake_mcp("200 OK", body, false).await;
        let mut provider = TavilyProvider::new(None);
        provider.endpoint = server.base.clone();
        let _ = provider.execute("q").await.unwrap();
        let request = last_request(&server);
        assert!(
            request.to_ascii_lowercase().contains("user-agent: opencode/"),
            "app UA on the wire: {request}"
        );
        assert!(
            request.contains("x-client-name: opencode2")
                || request.contains("X-Client-Name: opencode2")
        );
    }

    /// G-224① — a 429's `Retry-After` header survives into the
    /// ProviderError (the cooldown's input).
    #[tokio::test]
    async fn g224_provider_error_carries_retry_after() {
        let body = "{}".to_string();
        let server =
            fake_mcp_with_headers("429 Too Many Requests", body, false, "Retry-After: 12\r\n")
                .await;
        let mut provider = TavilyProvider::new(None);
        provider.endpoint = server.base.clone();
        let error = provider.execute("q").await.unwrap_err();
        assert_eq!(error.status, Some(429));
        assert_eq!(error.retry_after.as_deref(), Some("12"));
    }

    /// G-310 — the tavily timeout surfaces the DEDICATED message
    /// (ref tavily.ts:71-74 "Tavily web search request timed out"),
    /// not the generic transport error display.
    #[tokio::test]
    async fn g310_tavily_timeout_uses_dedicated_message() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // A server that accepts, swallows the request, and never answers.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buffer = vec![0u8; 65_536];
                let _ = stream.read(&mut buffer).await;
                let _ = stream.write(b"").await;
                std::future::pending::<()>().await;
            }
        });
        let mut provider = TavilyProvider::new(None);
        provider.endpoint = format!("http://127.0.0.1:{port}");
        provider.timeout = Duration::from_millis(150);
        let error = provider.execute("q").await.unwrap_err();
        assert_eq!(error.status, None);
        assert_eq!(error.message, "Tavily web search request timed out");
    }

    #[tokio::test]
    async fn tool_websearch_mcp_exa_routes_tools_call_and_maps_results() {
        let body = r#"{"result":{"content":[{"type":"text","text":"URL: https://a.example\nTitle: A\nPublished: 2024-01-15\nHighlights:\ngreat\n\n---\n\nURL: https://b.example\nTitle: N/A\nText:\nbody"}]}}"#.to_string();
        let server = fake_mcp("200 OK", body, false).await;
        let mut provider = ExaProvider::new(Some("secret".into()));
        provider.endpoint = server.base.clone();
        let results = provider.execute("rust mcp").await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://a.example");
        assert_eq!(results[0].time.published, Some(1_705_276_800_000));
        assert_eq!(results[1].title, None);
        // The v2 `WebSearchMcp.call` request contract: a `tools/call`
        // JSON-RPC POST with the key in the URL query (`exaApiKey`) and
        // the JSON/SSE Accept header.
        let request = last_request(&server);
        assert!(request.starts_with("POST /?exaApiKey=secret HTTP/"));
        assert!(
            request.contains("accept: application/json, text/event-stream")
                || request.contains("Accept: application/json, text/event-stream")
        );
        let body_start = request.split("\r\n\r\n").nth(1).expect("body");
        let rpc: serde_json::Value = serde_json::from_str(body_start.trim()).unwrap();
        assert_eq!(rpc["jsonrpc"], "2.0");
        assert_eq!(rpc["id"], 1);
        assert_eq!(rpc["method"], "tools/call");
        assert_eq!(rpc["params"]["name"], "web_search_exa");
        assert_eq!(
            rpc["params"]["arguments"],
            serde_json::json!({ "query": "rust mcp", "numResults": 8 })
        );
    }

    #[tokio::test]
    async fn tool_websearch_mcp_exa_keyless_has_no_query() {
        let body =
            r#"{"result":{"content":[{"type":"text","text":"URL: https://k.example\nText:\nx"}]}}"#
                .to_string();
        let server = fake_mcp("200 OK", body, false).await;
        let mut provider = ExaProvider::new(None);
        provider.endpoint = server.base.clone();
        assert_eq!(provider.execute("q").await.unwrap().len(), 1);
        assert!(last_request(&server).starts_with("POST / HTTP/"));
    }

    #[tokio::test]
    async fn tool_websearch_mcp_firecrawl_maps_data_web_and_bearer() {
        let body = r#"{"result":{"content":[{"type":"text","text":"{\"success\":true,\"data\":{\"web\":[{\"url\":\"https://f\",\"title\":\"F\",\"description\":\"d\"}]}}"}]}}"#.to_string();
        let server = fake_mcp("200 OK", body, false).await;
        let mut provider = FirecrawlProvider::new(Some("fc-key".into()));
        provider.endpoint = server.base.clone();
        let results = provider.execute("q").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://f");
        assert_eq!(results[0].content.as_deref(), Some("d"));
        let request = last_request(&server);
        assert!(
            request.contains("authorization: Bearer fc-key")
                || request.contains("Authorization: Bearer fc-key")
        );
        let body_start = request.split("\r\n\r\n").nth(1).expect("body");
        let rpc: serde_json::Value = serde_json::from_str(body_start.trim()).unwrap();
        assert_eq!(rpc["params"]["name"], "firecrawl_search");
        assert_eq!(rpc["params"]["arguments"], serde_json::json!({ "query": "q", "limit": 8 }));
    }

    #[tokio::test]
    async fn tool_websearch_mcp_parallel_maps_structured_content_over_sse() {
        // Streamable-HTTP MCP responses arrive as SSE `data:` lines —
        // `WebSearchMcp.parseResponse` scans them.
        let body = r#"{"result":{"content":[],"structuredContent":{"search_id":"s","session_id":"x","results":[{"url":"https://p","title":"P","publish_date":"2024-06-01","excerpts":["e1","e2"]}]}}}"#.to_string();
        let server = fake_mcp("200 OK", body, true).await;
        let mut provider = ParallelProvider::new(Some("par".into()));
        provider.endpoint = server.base.clone();
        let results = provider.execute("objective").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content.as_deref(), Some("e1\n\ne2"));
        assert_eq!(results[0].time.published, Some(1_717_200_000_000));
        let request = last_request(&server);
        let body_start = request.split("\r\n\r\n").nth(1).expect("body");
        let rpc: serde_json::Value = serde_json::from_str(body_start.trim()).unwrap();
        assert_eq!(rpc["params"]["name"], "web_search");
        assert_eq!(
            rpc["params"]["arguments"],
            serde_json::json!({ "objective": "objective", "search_queries": ["objective"] })
        );
    }

    #[tokio::test]
    async fn tool_websearch_mcp_tinyfish_key_and_keyless_headers() {
        let body = r#"{"result":{"content":[{"type":"text","text":"{\"results\":[{\"url\":\"https://t\",\"title\":\"T\",\"snippet\":\"s\"}]}"}]}}"#.to_string();
        let server = fake_mcp("200 OK", body, false).await;
        let mut provider = TinyFishProvider::new(Some("tf".into()));
        provider.endpoint = server.base.clone();
        let results = provider.execute("q").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content.as_deref(), Some("s"));
        let request = last_request(&server);
        assert!(request.contains("x-api-key: tf") || request.contains("X-API-Key: tf"));
        let body_start = request.split("\r\n\r\n").nth(1).expect("body");
        let rpc: serde_json::Value = serde_json::from_str(body_start.trim()).unwrap();
        assert_eq!(rpc["params"]["name"], "search");
        assert_eq!(rpc["params"]["arguments"], serde_json::json!({ "query": "q" }));

        // Keyless ⇒ `X-TinyFish-Access-Mode: keyless`, no API key.
        let mut provider = TinyFishProvider::new(None);
        provider.endpoint = server.base.clone();
        assert_eq!(provider.execute("q").await.unwrap().len(), 1);
        let request = last_request(&server);
        assert!(
            request.contains("x-tinyfish-access-mode: keyless")
                || request.contains("X-TinyFish-Access-Mode: keyless")
        );
        assert!(!request.contains("tf"));
    }

    #[tokio::test]
    async fn tool_websearch_mcp_http_error_surfaces_status() {
        // The typed error carries the HTTP status so the random-selection
        // cooldown rotation (429) can classify it (websearch.rs).
        let server =
            fake_mcp("429 Too Many Requests", r#"{"error":"limited"}"#.into(), false).await;
        let mut provider = ParallelProvider::new(None);
        provider.endpoint = server.base.clone();
        let error = provider.execute("q").await.unwrap_err();
        assert_eq!(error.status, Some(429));

        let server = fake_mcp("500 Internal Server Error", "broken".into(), false).await;
        let mut provider = FirecrawlProvider::new(None);
        provider.endpoint = server.base.clone();
        assert_eq!(provider.execute("q").await.unwrap_err().status, Some(500));
    }

    #[tokio::test]
    async fn tool_websearch_mcp_unparseable_body_is_request_error() {
        let server = fake_mcp("200 OK", "event: message\ndata: not-json\n".into(), false).await;
        let mut provider = ExaProvider::new(None);
        provider.endpoint = server.base.clone();
        let error = provider.execute("q").await.unwrap_err();
        assert_eq!(error.status, None);
        assert!(error.message.contains("web_search_exa"));
    }
}
