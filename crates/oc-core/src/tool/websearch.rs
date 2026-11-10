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
    /// G-266① — the random index picker for `randomProvider`
    /// (`available[Math.floor(Math.random() * available.length)]`,
    /// websearch.ts:143): true randomness in production; tests inject a
    /// fixed-seed picker to assert the distribution is not
    /// first-provider-sticky.
    picker: Box<dyn Fn(usize) -> usize + Send + Sync>,
    /// The config-seeded default selection (v2's in-memory
    /// `data.selection`, set by `ConfigWebSearchPlugin` via
    /// `editor.default.set` — config/plugin/websearch.ts). Takes
    /// precedence over the stored KV choice, exactly like v2's
    /// `selection()` (websearch.ts:119-131).
    config_selection: Option<Selection>,
    cooldowns: Mutex<HashMap<String, (u64, WebSearchError)>>,
    preferred: Mutex<HashMap<String, Option<String>>>,
    /// G-24 — plugin-registered providers (`ctx.websearch.transform`,
    /// ref host.ts:489-504): insert-or-replace by id, consulted BEFORE
    /// the env-derived builtins (the transform's add wins id
    /// collisions, matching v2's plugins-after-config order).
    plugin_providers: parking_lot::RwLock<Vec<Arc<dyn SearchProvider>>>,
}

impl WebSearch {
    pub fn new(db: crate::database::Database, providers: Vec<Arc<dyn SearchProvider>>) -> Self {
        Self {
            providers,
            db,
            picker: Box::new(|len| rand::Rng::gen_range(&mut rand::thread_rng(), 0..len)),
            config_selection: None,
            cooldowns: Mutex::new(HashMap::new()),
            preferred: Mutex::new(HashMap::new()),
            plugin_providers: parking_lot::RwLock::new(Vec::new()),
        }
    }

    /// G-24 — `ctx.websearch.transform`'s `editor.add(definition)`
    /// (websearch.ts transform): insert-or-replace by provider id.
    pub fn add_provider(&self, provider: Arc<dyn SearchProvider>) {
        let mut plugins = self.plugin_providers.write();
        match plugins.iter().position(|existing| existing.id() == provider.id()) {
            Some(at) => plugins[at] = provider,
            None => plugins.push(provider),
        }
    }

    /// The lookup order (plugin-registered first, then env builtins).
    fn all(&self) -> Vec<Arc<dyn SearchProvider>> {
        let mut all = self.plugin_providers.read().clone();
        all.extend(self.providers.iter().cloned());
        all
    }

    /// Seed the config-default selection (`websearch: false | {provider}`).
    #[must_use]
    pub fn with_config_selection(mut self, selection: Option<Selection>) -> Self {
        self.config_selection = selection;
        self
    }

    /// G-266① — replace the random index picker (tests inject a
    /// fixed-seed generator; production keeps the default
    /// `gen_range(0..len)`).
    #[must_use]
    pub fn with_picker(mut self, picker: Box<dyn Fn(usize) -> usize + Send + Sync>) -> Self {
        self.picker = picker;
        self
    }

    /// `WebSearch.providers` — id/name pairs sorted by display name.
    pub fn providers(&self) -> Vec<(String, String)> {
        let mut providers: Vec<(String, String)> =
            self.all().iter().map(|p| (p.id().to_string(), p.name().to_string())).collect();
        // G-282① — ref `toSorted((a, b) => a.name.localeCompare(b.name))`
        // (websearch.ts:168): the CLDR-root collation, not byte order
        // (G-154 family; oc-util localecmp).
        providers.sort_by(|a, b| oc_util::localecmp::locale_cmp(&a.1, &b.1));
        providers
    }

    fn provider(&self, id: &str) -> Option<Arc<dyn SearchProvider>> {
        if let Some(found) = self.plugin_providers.read().iter().find(|p| p.id() == id) {
            return Some(found.clone());
        }
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
            // G-282② — ref removes a stored KV entry that fails the
            // Selection decode (websearch.ts:130-133 `kv.remove`):
            // numbers/objects/`true`/null are undecodable garbage, not a
            // selection; drop the entry instead of re-reading it forever.
            _ => {
                let _ = self.db.kv_delete(PROVIDER_KEY).await;
                None
            }
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
                    .or_else(|| self.all().first().cloned())
                    .map(|provider| (provider.id().to_string(), provider.name().to_string())))
            }
            Some(Selection::Provider(id)) => Ok(self
                .provider(&id)
                .map(|provider| (provider.id().to_string(), provider.name().to_string()))),
            None => Ok(None),
        }
    }

    /// The current selection as wire JSON (G-24 residual — the plugin
    /// host's `websearch.default.get`): a provider id, `"random"`,
    /// `false`, or `null`.
    pub async fn selection_json(&self) -> serde_json::Value {
        match self.stored_selection().await {
            Some(Selection::Provider(id)) => serde_json::json!(id),
            Some(Selection::Random) => serde_json::json!("random"),
            Some(Selection::Disabled) => serde_json::json!(false),
            None => serde_json::Value::Null,
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

    /// G-224② — drop one session's preferred-affinity cell
    /// (`preferred.delete(event.data.sessionID)`, websearch.ts:92-97).
    pub fn evict_affinity(&self, session_id: &str) {
        self.preferred.lock().remove(session_id);
    }

    /// G-224② — the eviction subscription (websearch.ts:93-100):
    /// `session.deleted` and `session.moved` drop that session's
    /// affinity entry so the map cannot grow unboundedly. The task stops
    /// with the location bundle's shutdown watch (the ref's subscription
    /// is fork-scoped to the location node; the finalizer parity is the
    /// [`Drop`] clear below).
    pub fn subscribe_affinity_evictions(
        self: &Arc<Self>,
        bus: &crate::bus::Bus,
        mut stop: tokio::sync::watch::Receiver<bool>,
    ) {
        use futures::StreamExt as _;
        let mut subscription = bus.subscribe_types(&["session.deleted", "session.moved"]);
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    event = subscription.next() => {
                        let Some(event) = event else { break };
                        let Some(websearch) = weak.upgrade() else { break };
                        if let Some(session) = event.data.get("sessionID").and_then(serde_json::Value::as_str) {
                            websearch.evict_affinity(session);
                        }
                    }
                    _ = stop.wait_for(|flag| *flag) => break,
                }
            }
        });
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
            .all()
            .iter()
            .filter(|provider| {
                !cooldowns.contains_key(provider.id())
                    && attempted.is_none_or(|map| !map.contains(provider.id()))
            })
            .cloned()
            .collect();
        drop(cooldowns);
        // G-266① — ref websearch.ts:143 picks a uniformly random
        // provider (`Math.floor(Math.random() * available.length)`);
        // `available.first()` made the affinity rotation sticky on the
        // catalog's first provider until it 429s.
        let provider = if available.is_empty() {
            None
        } else {
            Some(available[(self.picker)(available.len())].clone())
        };
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
                        });
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
                        // G-224① — the cooldown honors `Retry-After`
                        // (websearch.ts:204,215-221): seconds or
                        // HTTP-date; invalid/absent falls back to 60s.
                        let now = now_ms();
                        let until = now + cooldown_ms(error.retry_after.as_deref(), now);
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

/// G-224② — the ref's `Effect.addFinalizer(() => preferred.clear())`
/// (websearch.ts:93): the map dies with the service.
impl Drop for WebSearch {
    fn drop(&mut self) {
        self.preferred.lock().clear();
    }
}

/// `cooldownMillis` (websearch.ts:215-221): absent/blank → 60s; an
/// integer number of seconds (negative → the 60s fallback, mirroring
/// the ref's `seconds >= 0` guard); an HTTP-date → `max(0, date - now)`;
/// anything unparsable → 60s.
fn cooldown_ms(value: Option<&str>, now: u64) -> u64 {
    let Some(value) = value.map(str::trim).filter(|text| !text.is_empty()) else {
        return COOLDOWN_MS;
    };
    // `Number(value)`: a plain numeric value (integer or float seconds).
    if let Ok(seconds) = value.parse::<f64>() {
        return if seconds >= 0.0 { (seconds * 1000.0) as u64 } else { COOLDOWN_MS };
    }
    if let Some(date) = http_date_ms(value) {
        return date.saturating_sub(now);
    }
    COOLDOWN_MS
}

/// RFC 7231 IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) — the only
/// `Retry-After` date form (HTTP-dates are GMT-only) — to unix
/// milliseconds.
fn http_date_ms(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let day_name = parts.next()?; // "Sun,"
    if !day_name.ends_with(',') {
        return None;
    }
    let day: i64 = parts.next()?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = parts.next()?.parse().ok()?;
    let time = parts.next()?;
    if parts.next()? != "GMT" {
        return None;
    }
    let mut clock = time.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next()?.parse().ok()?;
    // Days-from-civil (Howard Hinnant's algorithm), no calendar dep.
    let years = if month <= 2 { year - 1 } else { year };
    let era = if years >= 0 { years } else { years - 399 } / 400;
    let year_of_era = years - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    let milliseconds = ((days * 24 + hour) * 60 + minute) * 60 + second;
    u64::try_from(milliseconds.saturating_mul(1000)).ok()
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

/// The websearch progress payload (websearch.ts:63): `{provider}`.
fn provider_progress_payload(provider: &str) -> serde_json::Value {
    serde_json::json!({ "provider": provider })
}

/// The websearch leaf executor (Form-based provider selection included).
pub struct WebSearchExecute {
    pub search: Arc<WebSearch>,
    pub forms: Forms,
    /// Serializes the consent flow across concurrent searches.
    lock: Arc<tokio::sync::Mutex<()>>,
    /// v2 `context.progress({provider})` (websearch.ts:63
    /// `onProvider: provider => context.progress({provider: provider.id})`)
    /// — the LIVE metadata for the running search card.
    progress: Option<super::ProgressFn>,
}

#[derive(serde::Deserialize)]
struct Input {
    query: String,
}

impl WebSearchExecute {
    pub fn new(search: Arc<WebSearch>, forms: Forms) -> Self {
        Self { search, forms, lock: Arc::new(tokio::sync::Mutex::new(())), progress: None }
    }

    /// Attach the runner's `context.progress` channel (websearch.ts:63).
    #[must_use]
    pub fn with_progress(mut self, progress: Option<super::ProgressFn>) -> Self {
        self.progress = progress;
        self
    }
}

async fn select_provider(
    search: &Arc<WebSearch>,
    forms: &Forms,
    lock: &Arc<tokio::sync::Mutex<()>>,
    session_id: &str,
) -> Result<Option<String>, WebSearchError> {
    select_provider_with_budget(
        search,
        forms,
        lock,
        session_id,
        Duration::from_millis(SELECTION_TIMEOUT_MS),
    )
    .await
}

/// G-310 — ONE "1 minute" budget wraps the ENTIRE permit flow
/// (websearch.ts:141-144): the allow/choose/disable form AND the
/// follow-up provider form share it (per-form timeouts could reach
/// 2× the budget on the choose path).
async fn select_provider_with_budget(
    search: &Arc<WebSearch>,
    forms: &Forms,
    lock: &Arc<tokio::sync::Mutex<()>>,
    session_id: &str,
    budget: std::time::Duration,
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
    let cancelled = || WebSearchError::Request {
        provider_id: default_id.clone(),
        status: None,
        message: "Web search cancelled".to_string(),
    };
    // The single outer budget: covers both forms.
    let response = tokio::time::timeout(budget, async {
        let response = forms
            .ask(crate::forms::NewForm {
                session_id: session_id.to_string(),
                title: "Web Search".into(),
                // G-192 — ref websearch.ts:78: the selection form carries
                // `{kind: "websearch.provider"}`.
                metadata: Some(
                    serde_json::from_value(serde_json::json!({
                        "kind": "websearch.provider",
                    }))
                    .unwrap_or_default(),
                ),
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
            .await;
        let Ok(response) = response else {
            return Err(cancelled());
        };
        match response {
            oc_schema::form::State::Cancelled { .. } => Err(cancelled()),
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
                        let selection = forms
                            .ask(crate::forms::NewForm {
                                session_id: session_id.to_string(),
                                title: "Choose a web search provider".into(),
                                // G-192 — ref websearch.ts:111: the
                                // choose-provider form carries the
                                // same kind metadata.
                                metadata: Some(
                                    serde_json::from_value(serde_json::json!({
                                        "kind": "websearch.provider",
                                    }))
                                    .unwrap_or_default(),
                                ),
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
                            .await;
                        let Ok(selection) = selection else {
                            return Err(cancelled());
                        };
                        match selection {
                            oc_schema::form::State::Cancelled { .. } => Err(cancelled()),
                            oc_schema::form::State::Answered { answer } => {
                                let provider =
                                    answer.get("provider").and_then(|value| match value {
                                        oc_schema::form::FormValue::String(text) => {
                                            Some(text.clone())
                                        }
                                        _ => None,
                                    });
                                let Some(provider) = provider else {
                                    return Err(WebSearchError::ProviderRequired);
                                };
                                search.select(&Selection::Provider(provider.clone())).await;
                                Ok(Some(provider))
                            }
                            oc_schema::form::State::Pending => {
                                Err(WebSearchError::ProviderRequired)
                            }
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
    })
    .await;
    // The single budget elapsed across the whole flow (either form) —
    // ref websearch.ts:143-144 fails with the same cancelled error.
    match response {
        Err(_elapsed) => Err(cancelled()),
        Ok(result) => result,
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
        let executor_progress = self.progress.clone();
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
            // websearch.ts:63 — `context.progress({provider: provider.id})`
            // when the provider takes over. The rust `WebSearch.query` seam
            // resolves the provider internally, so the payload fires with
            // the resolved provider at the result boundary.
            if let Some(progress) = &executor_progress {
                progress(&context, provider_progress_payload(&response.provider_id.0));
            }
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
/// G-192 — the websearch disable filter (ref websearch.ts:181-192):
/// a `WebSearch.Disabled` selection deletes the `websearch` entry from
/// the event's tool map on every context/compaction/generate hook (a
/// disabled selection must reach the model's request tool list).
pub struct DisableHook {
    pub search: Arc<WebSearch>,
}

impl DisableHook {
    async fn remove_if_disabled(
        &self,
        tools: &mut std::collections::BTreeMap<String, crate::hooks::ToolDescriptor>,
    ) {
        if matches!(self.search.default_provider().await, Err(WebSearchError::Disabled)) {
            tools.remove("websearch");
        }
    }
}

#[async_trait::async_trait]
impl crate::hooks::HookHandler<crate::hooks::SessionContextEvent> for DisableHook {
    async fn call(
        &self,
        event: &mut crate::hooks::SessionContextEvent,
    ) -> Result<(), crate::hooks::HookError> {
        self.remove_if_disabled(&mut event.tools).await;
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::hooks::HookHandler<crate::hooks::SessionCompactionEvent> for DisableHook {
    async fn call(
        &self,
        event: &mut crate::hooks::SessionCompactionEvent,
    ) -> Result<(), crate::hooks::HookError> {
        self.remove_if_disabled(&mut event.context.tools).await;
        Ok(())
    }
}

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
        namespace: None,
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
        fn id(&self) -> &str {
            self.id
        }
        fn name(&self) -> &str {
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

    /// G-74: the progress payload (websearch.ts:63) is `{provider}`.
    #[test]
    fn tool_websearch_progress_payload_is_provider_id() {
        assert_eq!(provider_progress_payload("exa"), serde_json::json!({ "provider": "exa" }));
    }

    /// G-266① — ref websearch.ts:143: the rotation pick is uniformly
    /// random over the available providers, NOT `available.first()`
    /// (the old sticky behavior starved every provider but the first
    /// until it 429s). Fixed-seed LCG picker proves the distribution
    /// reaches the second provider.
    #[tokio::test]
    async fn tool_websearch_random_provider_picks_uniformly_not_first() {
        let seed = Mutex::new(0x2545F4914F6CDD1Du64);
        let search = service().await.with_picker(Box::new(move |len| {
            let mut state = seed.lock();
            *state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((*state >> 33) as usize) % len
        }));
        let mut picked_exa = 0;
        let mut picked_tavily = 0;
        for _ in 0..128 {
            let mut affinity: Option<String> = None;
            let provider = search.random_provider(0, &mut affinity, None).expect("provider");
            match provider.id() {
                "exa" => picked_exa += 1,
                "tavily" => picked_tavily += 1,
                other => panic!("unexpected provider {other}"),
            }
        }
        assert!(
            picked_tavily > 0,
            "the rotation must reach the second provider ({picked_exa}/{picked_tavily})"
        );
        assert!(
            picked_exa > 0,
            "the rotation must also pick the first provider ({picked_exa}/{picked_tavily})"
        );
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

    /// G-282① — `providers()` sorts display names with the CLDR-root
    /// collation (`localeCompare`, websearch.ts:168), not byte order:
    /// case-insensitive base letters first, so "Apple" < "banana" <
    /// "cherry" (byte order would put the uppercase first).
    #[tokio::test]
    async fn tool_websearch_providers_sort_by_locale() {
        struct Named(&'static str, &'static str);
        #[async_trait::async_trait]
        impl SearchProvider for Named {
            fn id(&self) -> &str {
                self.0
            }
            fn name(&self) -> &str {
                self.1
            }
            async fn execute(&self, _query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
                Ok(Vec::new())
            }
        }
        let db = crate::database::Database::open_in_memory().await.unwrap();
        let search = WebSearch::new(
            db,
            vec![
                Arc::new(Named("c", "cherry")),
                Arc::new(Named("a", "banana")),
                Arc::new(Named("b", "Apple")),
            ],
        );
        let names: Vec<String> = search.providers().into_iter().map(|(_, name)| name).collect();
        assert_eq!(names, vec!["Apple".to_owned(), "banana".to_owned(), "cherry".to_owned()]);
    }

    /// G-282② — an undecodable stored KV selection is REMOVED
    /// (websearch.ts:130-133 `kv.remove`), not silently re-read.
    #[tokio::test]
    async fn tool_websearch_invalid_stored_selection_is_cleaned() {
        let db = crate::database::Database::open_in_memory().await.unwrap();
        let search = WebSearch::new(db.clone(), Vec::new());
        // A stored number decodes as neither string nor false.
        search.db.kv_set(PROVIDER_KEY, &serde_json::json!(42)).await.unwrap();
        assert!(search.stored_selection().await.is_none());
        assert!(
            search.db.kv_get(PROVIDER_KEY).await.unwrap().is_none(),
            "the garbage entry was removed"
        );
        // Valid entries survive untouched.
        search.select(&Selection::Random).await;
        assert!(matches!(search.stored_selection().await, Some(Selection::Random)));
        assert!(search.db.kv_get(PROVIDER_KEY).await.unwrap().is_some());
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
        let search = WebSearch::new(db, vec![stub.clone() as Arc<dyn SearchProvider>])
            .with_config_selection(Some(Selection::Provider("stub".into())));
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
        let search = service().await.with_config_selection(Some(Selection::Provider("exa".into())));
        let response = search.query("q", None, None).await.unwrap();
        assert_eq!(response.provider_id.0, "exa");

        let search = service().await.with_config_selection(Some(Selection::Provider("exa".into())));
        search.select(&Selection::Provider("tavily".into())).await;
        let response = search.query("q", None, None).await.unwrap();
        assert_eq!(response.provider_id.0, "exa", "config selection beats KV");

        let search = service().await.with_config_selection(Some(Selection::Disabled));
        search.select(&Selection::Provider("tavily".into())).await;
        assert!(matches!(
            search.query("q", None, None).await.unwrap_err(),
            WebSearchError::Disabled
        ));

        // An unknown provider id in config does not own the selection —
        // the stored choice applies (v2: `providers.has(id)` gate).
        let search =
            service().await.with_config_selection(Some(Selection::Provider("missing".into())));
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
    /// G-192 — both provider forms carry `{kind: "websearch.provider"}`
    /// (ref websearch.ts:78,111).
    #[tokio::test]
    async fn g192_selection_forms_carry_websearch_provider_kind() {
        let search = Arc::new(service().await);
        let forms = Forms::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
        );
        let asker_forms = forms.clone();
        let asker_search = Arc::clone(&search);
        let asker_lock = Arc::new(tokio::sync::Mutex::new(()));
        let joined = tokio::spawn(async move {
            select_provider(&asker_search, &asker_forms, &asker_lock, "ses_1").await
        });

        // First form: the allow/choose/disable selection.
        let first = await_form(&forms).await;
        assert_eq!(first.title, "Web Search");
        assert_eq!(
            first.metadata.as_ref().map(|record| serde_json::to_value(record).unwrap()),
            Some(serde_json::json!({ "kind": "websearch.provider" })),
            "selection form metadata: {:?}",
            first.metadata
        );
        forms
            .reply(
                &first.id,
                std::iter::once((
                    "choice".to_string(),
                    oc_schema::form::FormValue::String("choose".into()),
                ))
                .collect(),
            )
            .await
            .expect("reply choose");

        // Second form: the provider picker — same kind metadata.
        let second = await_form(&forms).await;
        assert_eq!(second.title, "Choose a web search provider");
        assert_eq!(
            second.metadata.as_ref().map(|record| serde_json::to_value(record).unwrap()),
            Some(serde_json::json!({ "kind": "websearch.provider" })),
            "choose form metadata: {:?}",
            second.metadata
        );
        forms
            .reply(
                &second.id,
                std::iter::once((
                    "provider".to_string(),
                    oc_schema::form::FormValue::String("exa".into()),
                ))
                .collect(),
            )
            .await
            .expect("reply provider");

        let provider = joined.await.expect("join").expect("provider chosen");
        assert_eq!(provider.as_deref(), Some("exa"));
    }

    async fn await_form(forms: &Forms) -> oc_schema::form::Info {
        for _ in 0..400 {
            let pending = forms.list(Some("ses_1"));
            if let Some(info) = pending.into_iter().last() {
                return info;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("form never became pending");
    }

    /// G-310 — ONE budget wraps the ENTIRE permit flow (ref
    /// websearch.ts:141-144 `Effect.timeoutOrElse({ duration: "1
    /// minute" })` around BOTH forms): the choose path that spends most
    /// of the budget on the first form must time out at the single
    /// budget, not budget-per-form (2×). Virtual time makes the split
    /// deterministic.
    #[tokio::test]
    async fn g310_consent_flow_shares_one_timeout_budget_across_both_forms() {
        let search = Arc::new(service().await);
        let forms = Forms::new(
            Arc::new(oc_util::determinism::SequentialIdGen::new()),
            Arc::new(oc_util::determinism::FixedClock::epoch()),
        );
        let asker_forms = forms.clone();
        let asker_search = Arc::clone(&search);
        let asker_lock = Arc::new(tokio::sync::Mutex::new(()));
        let started = std::time::Instant::now();
        let joined = tokio::spawn(async move {
            select_provider_with_budget(
                &asker_search,
                &asker_forms,
                &asker_lock,
                "ses_1",
                std::time::Duration::from_millis(400),
            )
            .await
        });

        // First form: consume most of the budget before answering.
        let first = await_form(&forms).await;
        assert_eq!(first.title, "Web Search");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        forms
            .reply(
                &first.id,
                std::iter::once((
                    "choice".to_string(),
                    oc_schema::form::FormValue::String("choose".into()),
                ))
                .collect(),
            )
            .await
            .expect("reply choose");

        // Second form: never answered — the flow must end at the SINGLE
        // 400ms budget (per-form budgets would run 300ms + 400ms ≥ 700ms).
        let second = await_form(&forms).await;
        assert_eq!(second.title, "Choose a web search provider");
        let error = joined.await.expect("join").unwrap_err();
        match error {
            WebSearchError::Request { message, .. } => {
                assert_eq!(message, "Web search cancelled");
            }
            other => panic!("expected cancelled, got {other:?}"),
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(650),
            "one shared budget (400ms), not per-form (300ms + 400ms): {elapsed:?}"
        );
    }

    /// G-192 — the disable filter hook (websearch.ts:181-192): a disabled
    /// selection removes the `websearch` tool from the context tool list;
    /// an enabled/unset selection keeps it.
    #[tokio::test]
    async fn g192_disabled_selection_removes_websearch_from_the_tool_list() {
        use crate::hooks::{
            HookHandler, HookOptions, HookRegistry, SessionContextEvent, ToolDescriptor,
        };
        use oc_schema::id::{AgentId, SessionId};
        use oc_schema::model::Ref as ModelRef;

        let search = Arc::new(service().await);
        let hook = DisableHook { search: Arc::clone(&search) };

        let mut event = SessionContextEvent {
            session_id: SessionId::parse("ses_1").expect("session"),
            agent: AgentId::parse("build"),
            model: ModelRef {
                id: oc_schema::id::ModelId::parse("m"),
                provider_id: oc_schema::id::ProviderId::parse("test"),
                variant: None,
            },
            system: vec![],
            messages: vec![],
            options: Default::default(),
            tools: std::iter::once((
                "websearch".to_string(),
                ToolDescriptor { description: String::new(), input: serde_json::Value::Null },
            ))
            .collect(),
        };
        // Unset selection: the tool stays.
        hook.call(&mut event).await.expect("hook");
        assert!(event.tools.contains_key("websearch"));

        // Disabled selection: the tool is removed (websearch.ts:189
        // `delete event.tools[name]`).
        search.select(&Selection::Disabled).await;
        hook.call(&mut event).await.expect("hook");
        assert!(!event.tools.contains_key("websearch"), "disabled removes the tool");

        // The compaction surface shares the filter.
        let mut compaction = crate::hooks::SessionCompactionEvent { context: event, result: None };
        HookHandler::<crate::hooks::SessionCompactionEvent>::call(&hook, &mut compaction)
            .await
            .expect("compaction hook");
        assert!(!compaction.context.tools.contains_key("websearch"));

        // Registered through a real registry the hook shapes the dispatched
        // event (the engine's context/compaction/generate registrations).
        let registry = HookRegistry::new();
        let scope = crate::hooks::HookScope::new();
        scope.track(
            registry
                .register_session_context(
                    Arc::new(DisableHook { search: Arc::clone(&search) }),
                    HookOptions::default(),
                )
                .await,
        );
        let dispatched = crate::hooks::SessionContextEvent {
            session_id: SessionId::parse("ses_1").expect("session"),
            agent: AgentId::parse("build"),
            model: ModelRef {
                id: oc_schema::id::ModelId::parse("m"),
                provider_id: oc_schema::id::ProviderId::parse("test"),
                variant: None,
            },
            system: vec![],
            messages: vec![],
            options: Default::default(),
            tools: std::iter::once((
                "websearch".to_string(),
                ToolDescriptor { description: String::new(), input: serde_json::Value::Null },
            ))
            .collect(),
        };
        let shaped = registry.dispatch_session_context(dispatched).await.expect("dispatch");
        assert!(!shaped.tools.contains_key("websearch"));
    }

    // ------------------------------------------------------------------
    // G-224 — cooldown Retry-After + preferred-affinity eviction
    // ------------------------------------------------------------------

    /// `cooldownMillis` (websearch.ts:215-221) with a fixed now.
    #[test]
    fn g224_cooldown_honors_retry_after() {
        // Absent / blank → the 60s fallback.
        assert_eq!(cooldown_ms(None, 1_000), 60_000);
        assert_eq!(cooldown_ms(Some("   "), 1_000), 60_000);
        // Integer seconds.
        assert_eq!(cooldown_ms(Some("30"), 1_000), 30_000);
        assert_eq!(cooldown_ms(Some("0"), 1_000), 0);
        // Negative seconds → the fallback (ref `seconds >= 0` guard).
        assert_eq!(cooldown_ms(Some("-5"), 1_000), 60_000);
        // Unparsable → the fallback.
        assert_eq!(cooldown_ms(Some("soon"), 1_000), 60_000);
        // HTTP-date: the delta to now (clamped at 0 when in the past).
        assert_eq!(
            cooldown_ms(Some("Sun, 06 Nov 1994 08:49:37 GMT"), 784_111_100_000),
            784_111_777_000 - 784_111_100_000
        );
        assert_eq!(
            cooldown_ms(Some("Sun, 06 Nov 1994 08:49:37 GMT"), 1_000_000_000_000),
            0,
            "past date clamps to 0"
        );
    }

    /// The HTTP-date parser is the IMF-fixdate shape.
    #[test]
    fn g224_http_date_parser() {
        assert_eq!(http_date_ms("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777_000));
        assert_eq!(http_date_ms("06 Nov 1994 08:49:37 GMT"), None, "missing day name");
        assert_eq!(http_date_ms("Sun, 06 Nov 1994 08:49:37 PST"), None, "non-GMT");
    }

    struct RateLimitedProvider {
        retry_after: Option<&'static str>,
    }

    #[async_trait::async_trait]
    impl SearchProvider for RateLimitedProvider {
        fn id(&self) -> &'static str {
            "exa"
        }
        fn name(&self) -> &'static str {
            "Exa"
        }
        async fn execute(&self, _query: &str) -> Result<Vec<WebSearchResult>, ProviderError> {
            Err(ProviderError {
                status: Some(429),
                message: "rate limited".into(),
                retry_after: self.retry_after.map(str::to_string),
            })
        }
    }

    /// A 429 with `Retry-After` rotates and surfaces the cooldown error.
    #[tokio::test]
    async fn g224_cooldown_uses_retry_after_rotation() {
        let db = crate::database::Database::open_in_memory().await.unwrap();
        let search =
            WebSearch::new(db, vec![Arc::new(RateLimitedProvider { retry_after: Some("30") })]);
        search.select(&Selection::Random).await;
        let error = search.query("q", None, None).await.unwrap_err();
        assert!(matches!(error, WebSearchError::Request { status: Some(429), .. }));
        // Immediately again: still cooling — no provider is available
        // (the header-vs-60s timing distinction is pinned by cooldown_ms
        // above).
        let again = search.query("q", None, None).await.unwrap_err();
        assert!(matches!(
            again,
            WebSearchError::ProviderRequired | WebSearchError::Request { status: Some(429), .. }
        ));
    }

    /// G-224② — session.deleted/session.moved evict the preferred cell.
    #[tokio::test]
    async fn g224_affinity_evicted_on_session_events() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(
            crate::database::Database::open_file(dir.path().join("g224.db")).await.unwrap(),
        );
        let bus = Arc::new(crate::bus::Bus::new(
            (*db).clone(),
            crate::bus::BusOptions { persist: false, ..crate::bus::BusOptions::deterministic() },
        ));
        let search = Arc::new(WebSearch::new(
            (*db).clone(),
            vec![Arc::new(FixedProvider { id: "exa", name: "Exa" })],
        ));
        let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        search.subscribe_affinity_evictions(&bus, stop_rx);

        async fn publish(bus: &crate::bus::Bus, kind: &str) {
            let definition = crate::bus::registry::by_type_latest(kind)
                .unwrap_or_else(|| panic!("no definition for {kind}"));
            bus.publish(
                definition,
                serde_json::json!({ "sessionID": "ses_g224" }),
                crate::bus::PublishOptions::default(),
            )
            .await
            .expect("publish");
        }

        async fn await_eviction(search: &Arc<WebSearch>, what: &str) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while search.preferred.lock().contains_key("ses_g224") {
                assert!(std::time::Instant::now() < deadline, "{what} never evicted");
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }

        // Seed an affinity cell by querying with a session id.
        search.select(&Selection::Random).await;
        let _ = search.query("q", Some("ses_g224"), None).await;
        assert!(search.preferred.lock().contains_key("ses_g224"), "affinity cell seeded");

        // session.deleted drops it.
        publish(&bus, "session.deleted").await;
        await_eviction(&search, "session.deleted").await;

        // Re-seed; session.moved drops it too.
        let _ = search.query("q", Some("ses_g224"), None).await;
        assert!(search.preferred.lock().contains_key("ses_g224"), "affinity cell re-seeded");
        publish(&bus, "session.moved").await;
        await_eviction(&search, "session.moved").await;
    }
}
