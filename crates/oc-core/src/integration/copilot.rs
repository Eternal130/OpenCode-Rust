//! The copilot transport binding (BE-3b item 2): resolves the stored
//! GitHub-Copilot credential into provider-route material — base URL,
//! bearer token, and the Copilot client headers — as a synchronous
//! snapshot the model resolver reads at bind time (v2 resolves credentials
//! per attempt through the plugin context, github-copilot.ts:165-207).
//! CRED-D6 adds the `ChatGPT` credential → `openai` provider reshape
//! (openai.ts / chatgpt.ts) to the same snapshot.

use super::types::{ConnectionInfo, CredentialValue};
use super::IntegrationHost;
use dashmap::DashMap;
use oc_schema::model::Info as ModelInfo;
use oc_schema::provider::Compaction;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// v2's API version header (github-copilot.ts:13).
pub const COPILOT_API_VERSION: &str = "2026-08-01";
/// The Copilot API origin (github-copilot.ts:359-360).
pub const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";
/// The codex backend the `ChatGPT` logins route the openai provider to
/// (openai.ts:22 `codexBaseURL`).
pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
/// The token-sharing resource (chatgpt.ts:23 `resource`).
pub const CHATGPT_RESOURCE_URL: &str = "https://api.openai.com/v1";

/// Which `ChatGPT` credential drives the `openai` provider reshape
/// (CRED-D6): v2's openai and chatgpt plugins both re-list the
/// provider off the active `openai` oauth credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatGPTShaping {
    /// `chatgpt-browser` / `chatgpt-headless` — the codex backend
    /// reshape (openai.ts:256-315).
    Codex,
    /// `chatgpt-token-sharing` — the resource-URL reshape over the
    /// fallback model set (chatgpt.ts:267-306).
    TokenSharing,
}

/// ChatGPT-plan tokens only authorize the codex-eligible models
/// (openai.ts:25-27).
const CODEX_ALLOWED: &[&str] = &["gpt-5.5", "gpt-5.3-codex-spark"];
const CODEX_DISALLOWED: &[&str] = &["gpt-5.5-pro", "gpt-5.6"];
/// The token-sharing fallback models (chatgpt.ts:29-46).
const CHATGPT_FALLBACK_MODELS: &[&str] = &[
    "gpt-5.5",
    "gpt-5.5-fast",
    "gpt-5.6-luna",
    "gpt-5.6-luna-fast",
    "gpt-5.6-sol",
    "gpt-5.6-sol-fast",
    "gpt-5.6-terra",
    "gpt-5.6-terra-fast",
    "gpt-6-astra",
    "gpt-6-astra-fast",
    "gpt-6-luna",
    "gpt-6-luna-fast",
    "gpt-6-sol",
    "gpt-6-sol-fast",
    "gpt-6.1-sol",
    "gpt-6.1-sol-fast",
];

/// Route material for one provider derived from a stored credential.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderBinding {
    pub base_url: Option<String>,
    pub api_key: String,
    pub headers: BTreeMap<String, String>,
    /// The credential-driven `openai` provider reshape this binding
    /// triggers (CRED-D6); `None` for plain transport bindings.
    pub chatgpt: Option<ChatGPTShaping>,
}

/// The shared credential snapshot: provider id → binding. Refreshed from
/// the host (boot + credential events); read synchronously by resolvers.
#[derive(Default)]
pub struct CredentialBindings {
    entries: DashMap<String, ProviderBinding>,
}

impl CredentialBindings {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    #[must_use]
    pub fn get(&self, provider_id: &str) -> Option<ProviderBinding> {
        self.entries.get(provider_id).map(|entry| entry.value().clone())
    }

    /// Swap the snapshot (host refresh + tests).
    pub(crate) fn swap(&self, next: BTreeMap<String, ProviderBinding>) {
        self.entries.clear();
        for (id, binding) in next {
            self.entries.insert(id, binding);
        }
    }
}

/// The copilot client headers (github-copilot.ts:464-505 `applyHeaders`):
/// api version + intent + initiator; the per-request vision/interaction
/// metadata stays with the request layer.
#[must_use]
pub fn copilot_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("X-GitHub-Api-Version".to_owned(), COPILOT_API_VERSION.to_owned()),
        ("Openai-Intent".to_owned(), "conversation-edits".to_owned()),
        ("x-initiator".to_owned(), "user".to_owned()),
    ])
}

/// `https://copilot-api.{domain}` for enterprise domains
/// (github-copilot.ts:359-360); explicit `http(s)://` origins pass
/// through (self-hosted/loopback parity with BE-3's authorize).
#[must_use]
pub fn copilot_base_url(credential: &super::types::OAuthValue) -> String {
    let metadata = credential.metadata.as_ref();
    if let Some(endpoint) = metadata
        .and_then(|metadata| metadata.get("apiEndpoint"))
        .and_then(serde_json::Value::as_str)
        .filter(|endpoint| !endpoint.is_empty())
    {
        return endpoint.to_owned();
    }
    if let Some(enterprise) = metadata
        .and_then(|metadata| metadata.get("enterpriseUrl"))
        .and_then(serde_json::Value::as_str)
        .filter(|enterprise| !enterprise.is_empty())
    {
        if enterprise.starts_with("http://") || enterprise.starts_with("https://") {
            return enterprise.trim_end_matches('/').to_owned();
        }
        let domain = enterprise
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        return format!("https://copilot-api.{domain}");
    }
    COPILOT_BASE_URL.to_owned()
}

impl IntegrationHost {
    /// Test seam: install a fixed provider-binding snapshot without
    /// standing up real credentials (BE-4 vertex ADC tests).
    #[cfg(test)]
    pub fn provider_bindings_shared_for_test(
        &self,
    ) -> &Arc<crate::integration::copilot::CredentialBindings> {
        &self.provider_bindings_shared
    }

    /// Recomputes the credential-driven provider bindings into the
    /// shared snapshot. Best-effort per provider: a failing resolve
    /// keeps the previous entry.
    ///
    /// CRED-B1: the fold covers EVERY registered integration with a
    /// stored credential, not just the named transports — v2 resolves
    /// the active connection per attempt
    /// (`integrations.connection.active(provider.integrationID ?? id)`
    /// → `resolve`, model-resolver.ts:364-371) and maps it onto the
    /// provider settings (`nativeCredentialSettings`: key → apiKey,
    /// oauth access → apiKey, model-resolver.ts:308-319). The snapshot
    /// keeps the resolver's `bind()` synchronous; env connections stay
    /// with the facade's lazy env chain. The special transports computed
    /// first (copilot client headers, vertex ADC) win over the generic
    /// fold.
    pub async fn refresh_provider_bindings(self: &Arc<Self>) {
        let mut next = BTreeMap::new();
        if let Some(binding) = copilot_binding(self).await {
            next.insert("github-copilot".to_owned(), binding);
        }
        // Google Vertex ADC (BE-4): the refreshed authorized-user token
        // rides the same snapshot; failures keep the previous entry.
        if let Some(binding) = self.vertex_binding().await {
            next.insert("google-vertex".to_owned(), binding);
        }
        // CRED-D6: an active ChatGPT credential reshapes the `openai`
        // provider (the codex backend / token-sharing resource).
        if let Some(binding) = chatgpt_binding(self).await {
            next.insert("openai".to_owned(), binding);
        }
        let registered: Vec<String> = self.entries.read().keys().cloned().collect();
        if let Ok(rows) = self.store().all().await {
            let with_credentials: std::collections::HashSet<String> =
                rows.into_iter().map(|row| row.integration_id).collect();
            for id in registered {
                if !with_credentials.contains(&id) {
                    continue;
                }
                if let Some(binding) = integration_binding(self, &id).await {
                    next.entry(id).or_insert(binding);
                }
            }
        }
        self.provider_bindings_shared.swap(next);
    }

    /// The shared snapshot resolvers read (engine wires it at boot).
    #[must_use]
    pub fn credential_bindings(&self) -> &Arc<CredentialBindings> {
        &self.provider_bindings_shared
    }
}

async fn copilot_binding(host: &IntegrationHost) -> Option<ProviderBinding> {
    let connection = host.connection_active("github-copilot").await.ok()??;
    let ConnectionInfo::Credential { id, .. } = connection else {
        return None;
    };
    let stored = host.store().get(&id).await.ok()??;
    let CredentialValue::OAuth(oauth) = &stored.value else {
        return None;
    };
    if oauth.method_id != "device" {
        return None;
    }
    Some(ProviderBinding {
        base_url: Some(copilot_base_url(oauth)),
        // v2 sends the GitHub token itself as the bearer
        // (github-copilot.ts:169,257-261); stored as both access+refresh.
        api_key: oauth.refresh.clone(),
        headers: copilot_headers(),
        chatgpt: None,
    })
}

/// The openai-integration credential → `openai` provider reshape
/// (CRED-D6): an active `ChatGPT` oauth credential redirects the openai
/// provider — the codex backend + client headers for the
/// browser/headless logins (openai.ts:256-271), the resource URL for
/// token sharing (chatgpt.ts:267-277).
async fn chatgpt_binding(host: &IntegrationHost) -> Option<ProviderBinding> {
    let connection = host.connection_active("openai").await.ok()??;
    let ConnectionInfo::Credential { id, .. } = connection else {
        return None;
    };
    let stored = host.store().get(&id).await.ok()??;
    let CredentialValue::OAuth(oauth) = &stored.value else {
        return None;
    };
    let shaping = match oauth.method_id.as_str() {
        "chatgpt-browser" | "chatgpt-headless" => ChatGPTShaping::Codex,
        "chatgpt-token-sharing" => ChatGPTShaping::TokenSharing,
        _ => return None,
    };
    if oauth.access.is_empty() {
        return None;
    }
    let mut headers = BTreeMap::new();
    let base_url = match shaping {
        ChatGPTShaping::Codex => {
            // The codex client headers (openai.ts:266-270); the account
            // header only when the token carried the claim.
            headers.insert("originator".to_owned(), "opencode".to_owned());
            headers.insert("x-codex-beta-features".to_owned(), "remote_compaction_v2".to_owned());
            if let Some(account) = oauth
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("accountID"))
                .and_then(Value::as_str)
                .filter(|account| !account.is_empty())
            {
                headers.insert("chatgpt-account-id".to_owned(), account.to_owned());
            }
            CODEX_BASE_URL
        }
        ChatGPTShaping::TokenSharing => CHATGPT_RESOURCE_URL,
    };
    Some(ProviderBinding {
        base_url: Some(base_url.to_owned()),
        api_key: oauth.access.clone(),
        headers,
        chatgpt: Some(shaping),
    })
}

/// Whether the model stays part of the provider under this shaping
/// (CRED-D6): token sharing re-lists the provider with the fallback
/// models only (chatgpt.ts:280-287); the codex reshape keeps every
/// model — ineligible ones merely disable (openai.ts:273-297).
#[must_use]
pub fn chatgpt_model_available(info: &ModelInfo, shaping: ChatGPTShaping) -> bool {
    match shaping {
        ChatGPTShaping::Codex => true,
        ChatGPTShaping::TokenSharing => CHATGPT_FALLBACK_MODELS.contains(&info.id.0.as_str()),
    }
}

/// Folds the credential-driven reshape into the resolved model info
/// (CRED-D6): both logins zero the cost (the subscription covers
/// usage); the codex reshape pins the Codex CLI limits and disables
/// ineligible models (openai.ts:273-297); token sharing forces summary
/// compaction (chatgpt.ts:289-304).
pub fn apply_chatgpt_shaping(info: &mut ModelInfo, shaping: ChatGPTShaping) {
    if shaping == ChatGPTShaping::TokenSharing {
        // Token sharing does not support native /responses/compact.
        let mut settings = info.settings.clone().unwrap_or_default();
        settings.compaction = Some(Compaction::Summary);
        info.settings = Some(settings);
    }
    if pro_reasoning(info) {
        info.enabled = false;
        return;
    }
    match shaping {
        ChatGPTShaping::Codex => {
            if !codex_eligible(&info.id.0) {
                info.enabled = false;
                return;
            }
            info.cost.clear();
            // Match Codex CLI so context consumption and subscription
            // usage stay consistent between clients (openai.ts:295-296).
            info.limit.context = 400_000;
            info.limit.input = Some(272_000);
        }
        ChatGPTShaping::TokenSharing => {
            info.cost.clear();
        }
    }
}

/// A `reasoning: { mode: "pro" }` body overlay marks the pro tier both
/// plugins disable (openai.ts:278-281, chatgpt.ts:295-298).
fn pro_reasoning(info: &ModelInfo) -> bool {
    info.body
        .as_ref()
        .and_then(|body| body.get("reasoning"))
        .and_then(|reasoning| reasoning.get("mode"))
        .and_then(Value::as_str)
        .is_some_and(|mode| mode == "pro")
}

/// The codex model filter (openai.ts:283-293): the allow-list passes,
/// the deny-list fails, and otherwise only a `gpt-5.<minor>>4` (or a
/// later major) stays — a prefix match, trailing suffixes ignored.
fn codex_eligible(api_id: &str) -> bool {
    if CODEX_ALLOWED.contains(&api_id) {
        return true;
    }
    if CODEX_DISALLOWED.contains(&api_id) {
        return false;
    }
    let Some(rest) = api_id.strip_prefix("gpt-") else {
        return false;
    };
    let major_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if major_end == 0 {
        return false; // `^gpt-(\d+)` needs at least one digit
    }
    let major: u64 = rest[..major_end].parse().unwrap_or(0);
    let minor: u64 = rest[major_end..].strip_prefix('.').map_or(0, |tail| {
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect::<String>();
        digits.parse().unwrap_or(0)
    });
    major > 5 || (major == 5 && minor > 4)
}

/// The generic credential → route fold for one integration (CRED-B1):
/// the ACTIVE stored credential becomes the provider's api key — key
/// credentials contribute `key.key`, OAuth credentials their `access`
/// token (`nativeCredentialSettings`, model-resolver.ts:308-319) — and
/// a `baseURL` in the credential's metadata (or a key credential's
/// configuration form answer) overrides the endpoint (the `configured`
/// settings fold, model-resolver.ts:202-203). Env connections are left
/// to the facade's lazy env chain.
async fn integration_binding(
    host: &IntegrationHost,
    integration_id: &str,
) -> Option<ProviderBinding> {
    let connection = host.connection_active(integration_id).await.ok()??;
    let ConnectionInfo::Credential { id, .. } = connection else {
        return None;
    };
    let stored = host.store().get(&id).await.ok()??;
    let (token, metadata, configuration) = match &stored.value {
        CredentialValue::Key(key) => {
            (key.key.clone(), key.metadata.as_ref(), key.configuration.as_ref())
        }
        CredentialValue::OAuth(oauth) => (oauth.access.clone(), oauth.metadata.as_ref(), None),
    };
    if token.is_empty() {
        return None;
    }
    let from_map = |map: Option<&std::collections::BTreeMap<String, Value>>| {
        map.and_then(|map| map.get("baseURL"))
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(str::to_owned)
    };
    let from_value = |value: Option<&Value>| {
        value
            .and_then(|value| value.get("baseURL"))
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(str::to_owned)
    };
    let base_url = from_map(metadata).or_else(|| from_value(configuration));
    Some(ProviderBinding { base_url, api_key: token, headers: BTreeMap::new(), chatgpt: None })
}
