//! The copilot transport binding (BE-3b item 2): resolves the stored
//! GitHub-Copilot credential into provider-route material — base URL,
//! bearer token, and the Copilot client headers — as a synchronous
//! snapshot the model resolver reads at bind time (v2 resolves credentials
//! per attempt through the plugin context, github-copilot.ts:165-207).

use super::types::{ConnectionInfo, CredentialValue};
use super::IntegrationHost;
use dashmap::DashMap;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// v2's API version header (github-copilot.ts:13).
pub const COPILOT_API_VERSION: &str = "2026-08-01";
/// The Copilot API origin (github-copilot.ts:359-360).
pub const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";

/// Route material for one provider derived from a stored credential.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderBinding {
    pub base_url: Option<String>,
    pub api_key: String,
    pub headers: BTreeMap<String, String>,
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
    })
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
    Some(ProviderBinding { base_url, api_key: token, headers: BTreeMap::new() })
}
