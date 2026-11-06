//! GitHub Copilot dynamic model surface (G-167; port of the provider
//! transform of `plugin/provider/github-copilot.ts:172-315` over
//! `github-copilot/models.ts`): fold the live `/models` snapshot into
//! the static catalog's github-copilot provider — dynamic entries merge
//! over the static listing (aliases kept while their advertised API
//! model exists, unseen remote models added, `model_picker_enabled`
//! respected), `gpt-5-chat-latest` hides, and the merged provider
//! snapshot is what production resolution binds against.
//!
//! The fetch itself is [`CopilotSync::refresh`] — production callers
//! (engine boot) run it against the credential binding; tests seed the
//! state directly through [`CopilotSync::seed_for_test`], so no network
//! access ever happens in the test harness (ADR-0006).

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_ai::protocols::copilot::{
    copilot_model_entry, CopilotModelEntry, CopilotPrevious, CopilotRemoteModel,
    COPILOT_PACKAGE_ANTHROPIC,
};
use oc_ai::route::transport::SharedTransport;
use oc_schema::common::{Num, UsdPerMillionTokens};
use oc_schema::id::{ModelId, ProviderId};
use oc_schema::model as schema_model;
use oc_schema::model::{Cost, CostCache, ModelLimit, ModelTime, Status};

use crate::provider_catalog::{Catalog, Snapshot};

/// The copilot API origin fallback (github-copilot.ts `baseURL()`).
pub const COPILOT_BASE_URL_FALLBACK: &str = oc_ai::protocols::copilot::COPILOT_BASE_URL;

/// The chat-only alias that conflicts with the Copilot GPT-5 Responses
/// route (github-copilot.ts:259-268) — hidden ONLY for Copilot, not for
/// every provider catalog.
pub const HIDDEN_COPILOT_MODEL: &str = "gpt-5-chat-latest";

/// Fold one derived copilot entry into a full `Model.Info`
/// (models.ts `build` over `Model.Info.default`): the package lane
/// (Anthropic Messages for `/v1/messages` models), the settings
/// `baseURL` + `endpoint` overlays, and the derived
/// family/modalities/variants/released/cost/limit fields.
#[must_use]
pub fn copilot_model_info(entry: &CopilotModelEntry, previous: Option<&schema_model::Info>) -> schema_model::Info {
    let provider_id = ProviderId::parse("github-copilot");
    let mut settings = previous.and_then(|info| info.settings.clone()).unwrap_or_default();
    settings.rest.insert("baseURL".to_string(), serde_json::json!(entry.base_url));
    if let Some(endpoint) = entry.endpoint {
        settings
            .rest
            .insert("endpoint".to_string(), serde_json::json!(endpoint.as_str()));
    }
    schema_model::Info {
        id: ModelId::parse(entry.id.clone()),
        model_id: ModelId::parse(entry.model_id.clone()),
        provider_id,
        canonical: None,
        family: previous.and_then(|info| info.family.clone()).or_else(|| entry.family.clone()),
        name: previous.map_or_else(|| entry.name.clone(), |info| info.name.clone()),
        compatibility: None,
        // The package lane: `/v1/messages` models rewrite to the
        // anthropic package (models.ts:164) so the facade's standard
        // anthropic-messages machinery serves Claude-on-Copilot.
        package: Some(
            if entry.package == COPILOT_PACKAGE_ANTHROPIC {
                COPILOT_PACKAGE_ANTHROPIC
            } else {
                "@opencode/ai/providers/openai-compatible"
            }
            .to_string(),
        ),
        settings: Some(settings),
        headers: previous.and_then(|info| info.headers.clone()),
        body: previous.and_then(|info| info.body.clone()),
        capabilities: oc_schema::model::Capabilities {
            tools: true,
            input: entry.input.clone(),
            output: vec!["text".to_string()],
        },
        variants: entry
            .variants
            .iter()
            .map(|variant| schema_model::Variant {
                id: oc_schema::id::ModelVariantId::parse(variant.id.clone()),
                settings: serde_json::from_value(variant.settings.clone()).ok(),
                headers: None,
                body: None,
            })
            .collect(),
        time: ModelTime { released: Num(entry.released) },
        cost: vec![Cost {
            tier: None,
            input: UsdPerMillionTokens(Num(entry.cost.input)),
            output: UsdPerMillionTokens(Num(entry.cost.output)),
            cache: CostCache {
                read: UsdPerMillionTokens(Num(entry.cost.cache_read)),
                write: UsdPerMillionTokens::zero(),
            },
        }],
        status: Status::Active,
        enabled: entry.enabled,
        limit: ModelLimit {
            context: entry.limit.context,
            input: Some(entry.limit.input),
            output: entry.limit.output,
        },
    }
}

/// Hide `gpt-5-chat-latest` (github-copilot.ts:259-268): the chat-only
/// alias conflicts with the Copilot GPT-5 Responses route, so it is
/// disabled only for Copilot rather than for every provider catalog.
pub fn hide_conflicting_aliases(models: &mut [schema_model::Info]) {
    for model in models {
        if model.id.as_str() == HIDDEN_COPILOT_MODEL {
            model.enabled = false;
        }
    }
}

/// The provider transform (github-copilot.ts:216-234 + 258-268): derive
/// the github-copilot model list over the remote snapshot (aliases kept
/// while their advertised API model still exists, stale entries
/// dropped, unseen remote models added) and hide the conflicting
/// chat-only alias. Provider info/environment stay the static record's.
#[must_use]
pub fn merge_copilot_snapshot(
    snapshot: &Snapshot,
    base_url: &str,
    remote: &BTreeMap<String, CopilotRemoteModel>,
) -> Snapshot {
    let mut models = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for model in &snapshot.models {
        let Some(current) = remote.get(model.model_id.as_str()) else {
            continue; // the advertised API model is gone → drop
        };
        let entry = copilot_model_entry(
            model.id.as_str(),
            current,
            base_url,
            CopilotPrevious {
                name: Some(model.name.clone()),
                family: model.family.clone(),
                released: Some(model.time.released.0),
            },
        );
        models.push(copilot_model_info(&entry, Some(model)));
        seen.insert(model.id.0.clone());
    }
    for (id, model) in remote {
        if !seen.contains(id) {
            let entry = copilot_model_entry(id, model, base_url, CopilotPrevious::default());
            models.push(copilot_model_info(&entry, None));
        }
    }
    hide_conflicting_aliases(&mut models);
    Snapshot { info: snapshot.info.clone(), models, environment: snapshot.environment.clone() }
}

/// The copilot free-title hook (port of the ref `session.title` hook,
/// github-copilot.ts:402-432): when the session's model is a
/// github-copilot one, the sync has loaded its model snapshot, and the
/// account carries a utility model (`gpt-4o-mini`), the title is
/// generated through the FREE utility channel instead of the billable
/// model. Any failure leaves `result` unset so the normal title path
/// still runs (the ref `Effect.orElseSucceed(() => undefined)`).
pub struct CopilotUtilityTitleHook {
    sync: Arc<CopilotSync>,
    transport: SharedTransport,
    user_agent: String,
}

impl CopilotUtilityTitleHook {
    #[must_use]
    pub fn new(
        sync: Arc<CopilotSync>,
        transport: SharedTransport,
        user_agent: impl Into<String>,
    ) -> Self {
        Self { sync, transport, user_agent: user_agent.into() }
    }
}

#[async_trait::async_trait]
impl crate::hooks::HookHandler<crate::hooks::SessionTitleEvent> for CopilotUtilityTitleHook {
    async fn call(
        &self,
        event: &mut crate::hooks::SessionTitleEvent,
    ) -> Result<(), crate::hooks::HookError> {
        // A plugin-supplied title already won — never overwrite it.
        if event.result.is_some() {
            return Ok(());
        }
        // The hook is copilot-scoped (ref registers it with
        // `{providerID: githubCopilot}`).
        if event.request.model.provider_id.as_str() != "github-copilot" {
            return Ok(());
        }
        let Some(state) = self.sync.state() else { return Ok(()) };
        // gpt-4o-mini is the Copilot client's "small utility" model; when
        // the account lacks it the request falls through to the regular
        // title path instead of another guess.
        let Some(model) = oc_ai::protocols::copilot::UTILITY_TITLE_MODELS
            .into_iter()
            .find(|model| state.remote.contains_key(*model))
        else {
            return Ok(());
        };
        // Flatten the shaped request the way the ref does
        // (github-copilot.ts:413-422): system turns first, then the
        // conversation text.
        let system = {
            let text: Vec<&str> =
                event.request.system.iter().map(|part| part.text.as_str()).collect();
            if text.is_empty() { None } else { Some(text.join("\n")) }
        };
        let messages: Vec<oc_ai::protocols::copilot::UtilityTitleMessage> = event
            .request
            .messages
            .iter()
            .map(|message| oc_ai::protocols::copilot::UtilityTitleMessage {
                role: match message.role {
                    oc_ai::schema::messages::MessageRole::System => "system".to_string(),
                    oc_ai::schema::messages::MessageRole::User => "user".to_string(),
                    oc_ai::schema::messages::MessageRole::Assistant => "assistant".to_string(),
                    oc_ai::schema::messages::MessageRole::Tool => "tool".to_string(),
                },
                text: message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        oc_ai::schema::messages::ContentPart::Text { text, .. } => {
                            Some(text.clone())
                        }
                        _ => None,
                    })
                    .collect::<Vec<String>>()
                    .join("\n"),
            })
            .collect();
        let max_tokens = event
            .request
            .options
            .get("maxTokens")
            .and_then(serde_json::Value::as_u64);
        let input = oc_ai::protocols::copilot::UtilityTitleInput {
            base_url: &state.base_url,
            token: &state.token,
            model,
            user_agent: &self.user_agent,
            session_id: event.request.session_id.as_str(),
            system: system.as_deref(),
            messages: &messages,
            max_tokens,
        };
        // Any failure → result stays unset → the billable path runs.
        if let Ok(title) =
            oc_ai::protocols::copilot::copilot_utility_title(Arc::clone(&self.transport), &input)
                .await
        {
            event.result = Some(title);
        }
        Ok(())
    }
}
/// The synced copilot surface shared by the resolver (dynamic model
/// merge) and the free-title hook (the loaded model snapshot): the
/// merged provider snapshot plus the raw remote listing, refreshed by
/// the engine against the credential binding. A failed refresh keeps
/// the previous state (github-copilot.ts:196-199 "no refresh").
pub struct CopilotSync {
    transport: SharedTransport,
    state: parking_lot::RwLock<Option<std::sync::Arc<CopilotSyncState>>>,
}

/// One successful sync: the resolved base URL, the bound token, the
/// raw remote listing, and the merged provider snapshot.
pub struct CopilotSyncState {
    pub base_url: String,
    pub token: String,
    pub remote: BTreeMap<String, CopilotRemoteModel>,
    pub merged: Snapshot,
}

impl CopilotSync {
    #[must_use]
    pub fn new(transport: SharedTransport) -> Self {
        Self { transport, state: parking_lot::RwLock::new(None) }
    }

    /// The current sync state (shared handle, cloned out of the cell).
    #[must_use]
    pub fn state(&self) -> Option<std::sync::Arc<CopilotSyncState>> {
        self.state.read().clone()
    }

    /// Production refresh (github-copilot.ts `load` + the provider
    /// transform): fetch `{base_url}/models` with the credential
    /// headers and rebuild the merged snapshot. No binding, or a failed
    /// fetch, leaves the previous state in place (v2 clears on a
    /// missing credential and keeps stale models on a failed fetch).
    pub async fn refresh(&self, catalog: &Catalog, base_url: String, token: String, user_agent: String) {
        let headers = vec![
            ("Authorization".to_string(), format!("Bearer {token}")),
            ("User-Agent".to_string(), user_agent),
            ("X-GitHub-Api-Version".to_string(), oc_ai::protocols::copilot::COPILOT_API_VERSION.to_string()),
        ];
        let Ok(remote) =
            oc_ai::protocols::copilot::load_copilot_models(Arc::clone(&self.transport), &base_url, headers).await
        else {
            return; // "no refresh" — keep the previous state
        };
        self.store(catalog, base_url, token, remote);
    }

    /// Rebuild + store the merged state over an already-loaded remote
    /// snapshot (the offline seam: tests seed state without network).
    pub fn store(
        &self,
        catalog: &Catalog,
        base_url: String,
        token: String,
        remote: BTreeMap<String, CopilotRemoteModel>,
    ) {
        let Some(static_snapshot) = catalog.provider("github-copilot") else {
            return;
        };
        let merged = merge_copilot_snapshot(static_snapshot, &base_url, &remote);
        *self.state.write() = Some(Arc::new(CopilotSyncState { base_url, token, remote, merged }));
    }

    /// Test seam: seed the synced state directly (ADR-0006 — the
    /// production fetch path must not run during tests).
    pub fn seed_for_test(
        &self,
        catalog: &Catalog,
        base_url: &str,
        token: &str,
        remote: BTreeMap<String, CopilotRemoteModel>,
    ) {
        self.store(catalog, base_url.to_string(), token.to_string(), remote);
    }
}