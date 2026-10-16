//! Port of `packages/core/src/websearch.ts` + `tool/plugin/websearch.ts` —
//! provider registry, persisted selection, cooldown rotation, and the tool's
//! provider-selection Form flow (§4.10).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use oc_schema::misc::WebSearchResponse;
use parking_lot::Mutex;

use super::websearch_providers::SearchProvider;
use crate::forms::Forms;
use crate::permission::{AssertInput, PermissionError};
use crate::tool::{
    permission_cause, BuiltinTool, ToolExecuteWithContext, ToolFailure, ToolOutcome, ToolServices,
};

pub const NO_RESULTS: &str = "No search results found. Please try a different query.";
const PROVIDER_KEY: &str = "websearch:provider";
const COOLDOWN_MS: u64 = 60_000;
const SELECTION_TIMEOUT_MS: u64 = 60_000;

/// `WebSearch.Selection` — a provider id, `"random"`, or disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Provider(String),
    Random,
    Disabled,
}

/// `WebSearch.Error`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WebSearchError {
    #[error("A web search provider must be selected")]
    ProviderRequired,
    #[error("Web search provider not found: {0}")]
    ProviderNotFound(String),
    #[error("Web search disabled")]
    Disabled,
    #[error("Web search request to {provider_id} failed: {message}")]
    Request { provider_id: String, status: Option<u16>, message: String },
}

/// The websearch service (per location).
pub struct WebSearch {
    providers: Vec<Arc<dyn SearchProvider>>,
    db: crate::database::Database,
    /// The config-seeded default selection (v2's in-memory
    /// `data.selection`, set by `ConfigWebSearchPlugin` via
    /// `editor.default.set` — config/plugin/websearch.ts). Takes
    /// precedence over the stored KV choice, exactly like v2's
    /// `selection()` (websearch.ts:119-131).
    config_selection: Option<Selection>,
    cooldowns: Mutex<HashMap<String, (u64, WebSearchError)>>,
    preferred: Mutex<HashMap<String, Option<String>>>,
}

impl WebSearch {
    pub fn new(db: crate::database::Database, providers: Vec<Arc<dyn SearchProvider>>) -> Self {
        Self {
            providers,
            db,
            config_selection: None,
            cooldowns: Mutex::new(HashMap::new()),
            preferred: Mutex::new(HashMap::new()),
        }
    }

    /// Seed the config-default selection (`websearch: false | {provider}`).
    #[must_use]
    pub fn with_config_selection(mut self, selection: Option<Selection>) -> Self {
        self.config_selection = selection;
        self
    }

    /// `WebSearch.providers` — id/name pairs sorted by display name.
    pub fn providers(&self) -> Vec<(String, String)> {
        let mut providers: Vec<(String, String)> =
            self.providers.iter().map(|p| (p.id().to_string(), p.name().to_string())).collect();
        providers.sort_by(|a, b| a.1.cmp(&b.1));
        providers
    }

    fn provider(&self, id: &str) -> Option<Arc<dyn SearchProvider>> {
        self.providers.iter().find(|provider| provider.id() == id).cloned()
    }

    async fn stored_selection(&self) -> Option<Selection> {
        // v2 `selection()` (websearch.ts:119-131): the in-memory
        // (config-seeded) selection wins when it is `false`, `"random"`,
        // or a registered provider id; the stored KV choice is the
        // fallback.
        match &self.config_selection {
            Some(Selection::Disabled) => return Some(Selection::Disabled),
            Some(Selection::Random) => return Some(Selection::Random),
            Some(Selection::Provider(id)) if self.provider(id).is_some() => {
                return Some(Selection::Provider(id.clone()));
            }
            _ => {}
        }
        let stored = self.db.kv_get(PROVIDER_KEY).await.ok().flatten()?;
        match stored {
            serde_json::Value::String(id) if id == "random" => Some(Selection::Random),
            serde_json::Value::Bool(false) => Some(Selection::Disabled),
            serde_json::Value::String(id) => Some(Selection::Provider(id)),
            _ => None,
        }
    }

    /// `WebSearch.default` — the active provider under the current selection.
    pub async fn default_provider(&self) -> Result<Option<(String, String)>, WebSearchError> {
        match self.stored_selection().await {
            Some(Selection::Disabled) => Err(WebSearchError::Disabled),
            Some(Selection::Random) => {
                let preferred = self.preferred.lock().get("global").and_then(|p| p.clone());
                Ok(preferred
                    .and_then(|id| self.provider(&id))
                    .or_else(|| self.providers.first().cloned())
                    .map(|provider| (provider.id().to_string(), provider.name().to_string())))
            }
            Some(Selection::Provider(id)) => Ok(self
                .provider(&id)
                .map(|provider| (provider.id().to_string(), provider.name().to_string()))),
            None => Ok(None),
        }
    }

    /// `WebSearch.select`.
    pub async fn select(&self, selection: &Selection) {
        let value = match selection {
            Selection::Provider(id) => serde_json::json!(id),
            Selection::Random => serde_json::json!("random"),
            Selection::Disabled => serde_json::json!(false),
        };
        let _ = self.db.kv_set(PROVIDER_KEY, &value).await;
    }

    fn random_provider(
        &self,
        now: u64,
        affinity: &mut Option<String>,
        attempted: Option<&HashSet<String>>,
    ) -> Option<Arc<dyn SearchProvider>> {
        self.cooldowns.lock().retain(|id, (until, _)| *until > now && self.provider(id).is_some());
        if let Some(current) = affinity.as_ref().and_then(|id| self.provider(id)) {
            let cooled = self.cooldowns.lock().contains_key(current.as_ref().id());
            let attempted = attempted.is_some_and(|map| map.contains(current.as_ref().id()));
            if !cooled && !attempted {
                return Some(current);
            }
        }
        let cooldowns = self.cooldowns.lock();
        let available: Vec<Arc<dyn SearchProvider>> = self
            .providers
            .iter()
            .filter(|provider| {
                !cooldowns.contains_key(provider.id())
                    && attempted.is_none_or(|map| !map.contains(provider.id()))
            })
            .cloned()
            .collect();
        drop(cooldowns);
        let provider = available.first().cloned();
        if let Some(provider) = &provider {
            *affinity = Some(provider.id().to_string());
        }
        provider
    }

    /// `WebSearch.query`.
    pub async fn query(
        &self,
        query: &str,
        session_id: Option<&str>,
        provider_id: Option<&str>,
    ) -> Result<WebSearchResponse, WebSearchError> {
        let choice = if provider_id.is_some() { None } else { self.stored_selection().await };
        let mut provider = match provider_id {
            Some(id) => {
                self.provider(id).ok_or_else(|| WebSearchError::ProviderNotFound(id.to_string()))?
            }
            None => match &choice {
                Some(Selection::Disabled) => return Err(WebSearchError::Disabled),
                Some(Selection::Random) | None => self
                    .default_provider()
                    .await
                    .ok()
                    .flatten()
                    .and_then(|(id, _)| self.provider(&id))
                    .ok_or(WebSearchError::ProviderRequired)?,
                Some(Selection::Provider(id)) => {
                    self.provider(id).ok_or_else(|| WebSearchError::ProviderNotFound(id.clone()))?
                }
            },
        };
        let affinity_key = session_id.unwrap_or("global").to_string();
        let mut affinity: Option<String> =
            self.preferred.lock().get(&affinity_key).and_then(|value| value.clone());
        if matches!(choice, Some(Selection::Random)) {
            // Keep the cell for this query so deletion/movement cannot
            // reinsert an in-flight session's entry.
            self.preferred.lock().insert(affinity_key.clone(), affinity.clone());
            if let Some(random) = self.random_provider(now_ms(), &mut affinity, None) {
                provider = random;
            }
            self.preferred.lock().insert(affinity_key, affinity.clone());
        }
        let mut attempted: HashSet<String> = HashSet::new();
        let mut last_cooldown: Option<WebSearchError> = None;
        loop {
            let cooling = matches!(choice, Some(Selection::Random))
                && self
                    .cooldowns
                    .lock()
                    .get(provider.id())
                    .is_some_and(|(until, _)| *until > now_ms());
            if !cooling {
                attempted.insert(provider.id().to_string());
                match provider.execute(query).await {
                    Ok(results) => {
                        return Ok(WebSearchResponse {
                            provider_id: oc_schema::id::WebSearchId(provider.id().to_string()),
                            results,
                        })
                    }
                    Err(error) => {
                        let websearch_error = WebSearchError::Request {
                            provider_id: provider.id().to_string(),
                            status: error.status,
                            message: error.message.clone(),
                        };
                        // Only the random selection rotates on 429.
                        if !matches!(choice, Some(Selection::Random)) || error.status != Some(429) {
                            return Err(websearch_error);
                        }
                        let until = now_ms() + COOLDOWN_MS;
                        self.cooldowns
                            .lock()
                            .insert(provider.id().to_string(), (until, websearch_error.clone()));
                        last_cooldown = Some(websearch_error);
                    }
                }
            }
            match self.random_provider(now_ms(), &mut affinity, Some(&attempted)) {
                Some(next) => provider = next,
                None => return Err(last_cooldown.unwrap_or(WebSearchError::ProviderRequired)),
            }
        }
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

pub fn description() -> String {
    format!(
        "Search the web using the user's selected search integration. Use this for current information beyond knowledge cutoff.\n\nThe current year is {}. Use this year when searching for recent information or current events.",
        current_year()
    )
}

fn current_year() -> i64 {
    now_ms() as i64 / 31_557_600_000 + 1970
}

/// The websearch leaf executor (Form-based provider selection included).
pub struct WebSearchExecute {
    pub search: Arc<WebSearch>,
    pub forms: Forms,
    /// Serializes the consent flow across concurrent searches.
    lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(serde::Deserialize)]
struct Input {
    query: String,
}

impl WebSearchExecute {
    pub fn new(search: Arc<WebSearch>, forms: Forms) -> Self {
        Self { search, forms, lock: Arc::new(tokio::sync::Mutex::new(())) }
    }
}

async fn select_provider(
    search: &Arc<WebSearch>,
    forms: &Forms,
    lock: &Arc<tokio::sync::Mutex<()>>,
    session_id: &str,
) -> Result<Option<String>, WebSearchError> {
    let _guard = lock.lock().await;
    if search.default_provider().await.is_ok_and(|provider| provider.is_some()) {
        return Ok(None);
    }
    let providers = search.providers();
    let default_provider = providers.first().cloned();
    let Some((default_id, _)) = default_provider else {
        return Err(WebSearchError::ProviderRequired);
    };
    let names = providers.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>().join(", ");
    let response = tokio::time::timeout(Duration::from_millis(SELECTION_TIMEOUT_MS), async {
        forms
            .ask(crate::forms::NewForm {
                session_id: session_id.to_string(),
                title: "Web Search".into(),
                metadata: None,
                fields: vec![oc_schema::form::Field::String {
                    key: "choice".into(),
                    title: None,
                    description: Some(
                        "Allow OpenCode to search the web for up-to-date information?".into(),
                    ),
                    required: Some(true),
                    hidden: None,
                    when: None,
                    format: None,
                    min_length: None,
                    max_length: None,
                    pattern: None,
                    placeholder: None,
                    default: None,
                    options: Some(vec![
                        oc_schema::form::Option_ {
                            value: "allow".into(),
                            label: format!("Allow search via {names}"),
                            description: None,
                        },
                        oc_schema::form::Option_ {
                            value: "choose".into(),
                            label: "Choose another provider".into(),
                            description: None,
                        },
                        oc_schema::form::Option_ {
                            value: "disable".into(),
                            label: "Disable web search".into(),
                            description: None,
                        },
                    ]),
                    custom: Some(false),
                }],
            })
            .await
    })
    .await;
    let Ok(Ok(response)) = response else {
        return Err(WebSearchError::Request {
            provider_id: default_id,
            status: None,
            message: "Web search cancelled".into(),
        });
    };
    match response {
        oc_schema::form::State::Cancelled { .. } => Err(WebSearchError::Request {
            provider_id: default_id,
            status: None,
            message: "Web search cancelled".into(),
        }),
        oc_schema::form::State::Answered { answer } => {
            let choice = answer.get("choice").and_then(|value| match value {
                oc_schema::form::FormValue::String(text) => Some(text.clone()),
                _ => None,
            });
            match choice.as_deref() {
                Some("disable") => {
                    search.select(&Selection::Disabled).await;
                    Err(WebSearchError::Disabled)
                }
                Some("choose") => {
                    let selection =
                        tokio::time::timeout(Duration::from_millis(SELECTION_TIMEOUT_MS), async {
                            forms
                                .ask(crate::forms::NewForm {
                                    session_id: session_id.to_string(),
                                    title: "Choose a web search provider".into(),
                                    metadata: None,
                                    fields: vec![oc_schema::form::Field::String {
                                        key: "provider".into(),
                                        title: None,
                                        description: Some(
                                            "Choose a provider for web search.".into(),
                                        ),
                                        required: Some(true),
                                        hidden: None,
                                        when: None,
                                        format: None,
                                        min_length: None,
                                        max_length: None,
                                        pattern: None,
                                        placeholder: None,
                                        default: None,
                                        options: Some(
                                            providers
                                                .iter()
                                                .map(|(id, name)| oc_schema::form::Option_ {
                                                    value: id.clone(),
                                                    label: name.clone(),
                                                    description: None,
                                                })
                                                .collect(),
                                        ),
                                        custom: Some(false),
                                    }],
                                })
                                .await
                        })
                        .await;
                    let Ok(Ok(selection)) = selection else {
                        return Err(WebSearchError::Request {
                            provider_id: default_id,
                            status: None,
                            message: "Web search cancelled".into(),
                        });
                    };
                    match selection {
                        oc_schema::form::State::Cancelled { .. } => Err(WebSearchError::Request {
                            provider_id: default_id,
                            status: None,
                            message: "Web search cancelled".into(),
                        }),
                        oc_schema::form::State::Answered { answer } => {
                            let provider = answer.get("provider").and_then(|value| match value {
                                oc_schema::form::FormValue::String(text) => Some(text.clone()),
                                _ => None,
                            });
                            let Some(provider) = provider else {
                                return Err(WebSearchError::ProviderRequired);
                            };
                            search.select(&Selection::Provider(provider.clone())).await;
                            Ok(Some(provider))
                        }
                        oc_schema::form::State::Pending => Err(WebSearchError::ProviderRequired),
                    }
                }
                Some(_) => {
                    search.select(&Selection::Random).await;
                    Ok(None)
                }
                None => Err(WebSearchError::ProviderRequired),
            }
        }
        oc_schema::form::State::Pending => Err(WebSearchError::ProviderRequired),
    }
}

impl ToolExecuteWithContext for WebSearchExecute {
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
        let search_service = Arc::clone(&self.search);
        let executor_forms = self.forms.clone();
        let executor_lock = Arc::clone(&self.lock);
        Box::pin(async move {
            let input: Input = serde_json::from_value(input)
                .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            services
                .permission
                .assert(AssertInput {
                    id: None,
                    session_id: context.session_id.clone(),
                    agent: Some(context.agent.clone()),
                    action: "websearch".into(),
                    resources: vec![input.query.clone()],
                    save: Some(vec!["*".into()]),
                    metadata: Some(
                        serde_json::from_value(serde_json::json!({ "query": input.query }))
                            .unwrap_or_default(),
                    ),
                    source: Some(oc_schema::permission::Source::Tool {
                        message_id: context.message_id.clone(),
                        id: context.call_id.clone(),
                    }),
                })
                .await
                .map_err(|error: PermissionError| ToolFailure {
                    message: format!("Unable to search the web for {}", input.query),
                    cause: Some(permission_cause(&error)),
                })?;
            let run_query = |provider_id: Option<String>| {
                let service = Arc::clone(&search_service);
                let query = input.query.clone();
                let session = context.session_id.to_string();
                async move {
                    service.query(&query, Some(session.as_str()), provider_id.as_deref()).await
                }
            };
            let result = match run_query(None).await {
                Ok(result) => Ok(result),
                Err(WebSearchError::ProviderRequired) => {
                    let selected = select_provider(
                        &search_service,
                        &executor_forms,
                        &executor_lock,
                        context.session_id.as_str(),
                    )
                    .await;
                    match selected {
                        Ok(provider_id) => run_query(provider_id).await,
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            };
            let response = result.map_err(|error| {
                let fallback = format!("Unable to search the web for {}", input.query);
                let message = match &error {
                    WebSearchError::Request { status: Some(429), .. } => {
                        "Web search rate limited (HTTP 429)".to_string()
                    }
                    WebSearchError::Request { status: Some(401), .. } => {
                        "Web search authentication failed (HTTP 401)".to_string()
                    }
                    WebSearchError::Request { status: Some(status), .. } => {
                        format!("Web search request failed (HTTP {status})")
                    }
                    _ => fallback,
                };
                ToolFailure { message, cause: Some(super::FailureCause::Nested(error.to_string())) }
            })?;
            let content = if response.results.is_empty() {
                NO_RESULTS.to_string()
            } else {
                response
                    .results
                    .iter()
                    .map(|result| {
                        let title = result.title.clone().unwrap_or_else(|| result.url.clone());
                        let published = result.time.published.map_or_else(String::new, |millis| {
                            format!("\nPublished: {}", iso8601(millis))
                        });
                        let body = result
                            .content
                            .as_deref()
                            .map_or_else(String::new, |content| format!("\n\n{content}"));
                        format!("## [{title}]({}){published}{body}", result.url)
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            };
            Ok(ToolOutcome {
                content: vec![oc_schema::tool::Content::Text { text: content }],
                metadata: Some(
                    serde_json::from_value(
                        serde_json::json!({ "provider": response.provider_id.0 }),
                    )
                    .unwrap_or_default(),
                ),
                output: Some(serde_json::json!({
                    "provider": response.provider_id.0,
                    "results": response.results,
                })),
            })
        })
    }
}

fn iso8601(millis: u64) -> String {
    // Render ISO-8601 UTC (new Date(...).toISOString()).
    let seconds = millis / 1000;
    let days = seconds / 86_400;
    let rem = seconds % 86_400;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.000Z")
}

/// The websearch builtin registration.
pub fn tool(execute: Arc<WebSearchExecute>) -> BuiltinTool {
    BuiltinTool {
        name: "websearch",
        description: description(),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Websearch query" }
            },
            "required": ["query"]
        }),
        permission: Some("websearch"),
        codemode: false,
        execute,
    }
}

#[cfg(test)]
mod tests {
    use crate::tool::websearch_providers::ProviderError;
    use oc_schema::misc::WebSearchResult;

    use super::*;

    struct FixedProvider {
        id: &'static str,
        name: &'static str,
    }

    #[async_trait::async_trait]
    impl SearchProvider for FixedProvider {
        fn id(&self) -> &'static str {
            self.id
        }
        fn name(&self) -> &'static str {
            self.name
        }
        async fn execute(&self, _query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
            Ok(vec![WebSearchResult {
                url: format!("https://{}", self.id),
                title: Some(format!("{} result", self.name)),
                content: Some("content".into()),
                time: oc_schema::misc::WebSearchResultTime { published: Some(1_700_000_000_000) },
            }])
        }
    }

    async fn service() -> WebSearch {
        let db = crate::database::Database::open_in_memory().await.unwrap();
        WebSearch::new(
            db,
            vec![
                Arc::new(FixedProvider { id: "exa", name: "Exa" }),
                Arc::new(FixedProvider { id: "tavily", name: "Tavily" }),
            ],
        )
    }

    #[tokio::test]
    async fn tool_websearch_requires_a_provider_until_selected() {
        let search = service().await;
        let error = search.query("q", None, None).await.unwrap_err();
        assert!(matches!(error, WebSearchError::ProviderRequired));
        search.select(&Selection::Provider("exa".into())).await;
        let response = search.query("q", None, None).await.unwrap();
        assert_eq!(response.provider_id.0, "exa");
        // Explicit provider id bypasses the stored selection.
        let response = search.query("q", None, Some("tavily")).await.unwrap();
        assert_eq!(response.provider_id.0, "tavily");
    }

    #[tokio::test]
    async fn tool_websearch_without_providers_is_typed_provider_required() {
        // auditE #4: zero eligible providers must surface the v2 typed
        // error (`WebSearch.ProviderRequired`), not a hollow failure.
        let db = crate::database::Database::open_in_memory().await.unwrap();
        let search = WebSearch::new(db, Vec::new());
        assert!(matches!(
            search.query("q", None, None).await.unwrap_err(),
            WebSearchError::ProviderRequired
        ));
        assert!(search.providers().is_empty());
    }

    #[tokio::test]
    async fn tool_websearch_stub_provider_receives_query_and_maps_results() {
        // The provider seam: the query text arrives verbatim and the
        // results map straight into `WebSearch.Response`.
        struct StubProvider {
            queries: Mutex<Vec<String>>,
        }

        #[async_trait::async_trait]
        impl SearchProvider for StubProvider {
            fn id(&self) -> &'static str {
                "stub"
            }
            fn name(&self) -> &'static str {
                "Stub"
            }
            async fn execute(&self, query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
                self.queries.lock().push(query.to_string());
                Ok(vec![WebSearchResult {
                    url: "https://example.com/a".into(),
                    title: Some("A".into()),
                    content: None,
                    time: oc_schema::misc::WebSearchResultTime { published: None },
                }])
            }
        }

        let stub = Arc::new(StubProvider { queries: Mutex::new(Vec::new()) });
        let db = crate::database::Database::open_in_memory().await.unwrap();
        let search =
            WebSearch::new(db, vec![stub.clone() as Arc<dyn SearchProvider>]).with_config_selection(
                Some(Selection::Provider("stub".into())),
            );
        let response = search.query("rust async io", None, None).await.unwrap();
        assert_eq!(response.provider_id.0, "stub");
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].url, "https://example.com/a");
        assert_eq!(*stub.queries.lock(), vec!["rust async io".to_string()]);
    }

    #[tokio::test]
    async fn tool_websearch_config_selection_seeds_and_overrides() {
        // v2 ConfigWebSearchPlugin: the config `websearch` selection is the
        // in-memory default and wins over the stored KV choice
        // (websearch.ts:119-131 checks `data.selection` first).
        let search = service()
            .await
            .with_config_selection(Some(Selection::Provider("exa".into())));
        let response = search.query("q", None, None).await.unwrap();
        assert_eq!(response.provider_id.0, "exa");

        let search = service()
            .await
            .with_config_selection(Some(Selection::Provider("exa".into())));
        search.select(&Selection::Provider("tavily".into())).await;
        let response = search.query("q", None, None).await.unwrap();
        assert_eq!(response.provider_id.0, "exa", "config selection beats KV");

        let search = service().await.with_config_selection(Some(Selection::Disabled));
        search.select(&Selection::Provider("tavily".into())).await;
        assert!(matches!(search.query("q", None, None).await.unwrap_err(), WebSearchError::Disabled));

        // An unknown provider id in config does not own the selection —
        // the stored choice applies (v2: `providers.has(id)` gate).
        let search = service()
            .await
            .with_config_selection(Some(Selection::Provider("missing".into())));
        search.select(&Selection::Provider("tavily".into())).await;
        let response = search.query("q", None, None).await.unwrap();
        assert_eq!(response.provider_id.0, "tavily");
    }

    #[tokio::test]
    async fn tool_websearch_disabled_selection_fails_closed() {
        let search = service().await;
        search.select(&Selection::Disabled).await;
        assert!(matches!(
            search.query("q", None, None).await.unwrap_err(),
            WebSearchError::Disabled
        ));
    }

    #[test]
    fn tool_websearch_result_format() {
        let results = [
            WebSearchResult {
                url: "https://a".into(),
                title: Some("A".into()),
                content: Some("body".into()),
                time: oc_schema::misc::WebSearchResultTime { published: Some(1_700_000_000_000) },
            },
            WebSearchResult {
                url: "https://b".into(),
                title: None,
                content: None,
                time: oc_schema::misc::WebSearchResultTime { published: None },
            },
        ];
        let content = results
            .iter()
            .map(|result| {
                let title = result.title.clone().unwrap_or_else(|| result.url.clone());
                let published = result
                    .time
                    .published
                    .map_or_else(String::new, |millis| format!("\nPublished: {}", iso8601(millis)));
                let body = result
                    .content
                    .as_deref()
                    .map_or_else(String::new, |content| format!("\n\n{content}"));
                format!("## [{title}]({}){published}{body}", result.url)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(
            content,
            "## [A](https://a)\nPublished: 2023-11-14T22:13:20.000Z\n\nbody\n\n## [https://b](https://b)"
        );
    }

    #[test]
    fn tool_websearch_http_error_messages() {
        let message = |status: Option<u16>| {
            let error = WebSearchError::Request {
                provider_id: "exa".into(),
                status,
                message: String::new(),
            };
            let fallback = "Unable to search the web for q".to_string();
            match &error {
                WebSearchError::Request { status: Some(429), .. } => {
                    "Web search rate limited (HTTP 429)".to_string()
                }
                WebSearchError::Request { status: Some(401), .. } => {
                    "Web search authentication failed (HTTP 401)".to_string()
                }
                WebSearchError::Request { status: Some(status), .. } => {
                    format!("Web search request failed (HTTP {status})")
                }
                _ => fallback,
            }
        };
        assert_eq!(message(Some(429)), "Web search rate limited (HTTP 429)");
        assert_eq!(message(Some(401)), "Web search authentication failed (HTTP 401)");
        assert_eq!(message(Some(500)), "Web search request failed (HTTP 500)");
        assert_eq!(message(None), "Unable to search the web for q");
    }
}
