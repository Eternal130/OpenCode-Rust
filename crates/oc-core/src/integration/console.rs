//! Console provider-catalog ingestion + connection-status reporting
//! (BE-3b items 4+5, opencode.ts:29-45,165-207,472-520): fetches the
//! org config from the `OpenCode` Console with the stored credential,
//! maps Console error shapes (`SsoRequired` 403 / `Unauthorized` 401)
//! onto the runtime `connection.status`, and projects console-issued
//! providers onto custom-provider route material the resolver binds.

use super::types::{ConnectionInfo, ConnectionStatus, CredentialValue};
use super::IntegrationHost;
use crate::provider_catalog::facade::CustomModel;
use crate::provider_catalog::facade::CustomProvider;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

const DEFAULT_SERVER: &str = "https://opencode.ai/console";

/// The console's well-formed failure shapes (opencode.ts:63-70).
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConsoleError {
    /// 403 `_tag: SsoRequired` — the org enforces SSO and the proof is
    /// missing/expired (opencode.ts:73-77,494-507).
    #[error("{}", sso_message(organization.as_deref()))]
    SsoRequired { organization: Option<String>, url: String },
    /// 401 `_tag: Unauthorized` — the session behind the token was
    /// revoked/expired (opencode.ts:71-72).
    #[error("Reconnect OpenCode Console to continue")]
    SignedOut,
    #[error("console request failed: {0}")]
    Http(String),
    #[error("console config did not decode: {0}")]
    Parse(String),
}

fn sso_message(organization: Option<&str>) -> String {
    format!(
        "Sign in with SSO again to use {}",
        organization.unwrap_or("your OpenCode Console organization")
    )
}

/// `ConfigProvider.Info` (config/provider.ts:94-101) — tolerant decode.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConsoleProvider {
    #[serde(default, rename = "canonical")]
    pub canonical: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "type")]
    pub package: Option<String>,
    #[serde(default)]
    pub settings: Option<Value>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub body: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    pub models: Option<BTreeMap<String, ConsoleModel>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConsoleModel {
    #[serde(default, rename = "modelID")]
    pub model_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub settings: Option<Value>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub body: Option<BTreeMap<String, Value>>,
}

/// `RemoteResponse` (opencode.ts:29-45) — providers today; websearch /
/// mcp stay on the deferred list. `experimental.policies` is the
/// organization policy compiled for the authenticated caller
/// (CONFIG-C7: the Console managed-policy source).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConsoleConfig {
    #[serde(default)]
    pub providers: BTreeMap<String, ConsoleProvider>,
    #[serde(default)]
    pub experimental: Option<ConsoleExperimental>,
}

/// `RemoteResponse.experimental` (opencode.ts:37-40).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConsoleExperimental {
    /// Organization policy statements; omitted when there is none.
    #[serde(default)]
    pub policies: Option<Vec<oc_schema::config::policy::Policy>>,
}

/// The console-relevant projection of either credential type
/// (CRED-CONSOLE-KEYAUTH): OAuth tokens and service-account keys both
/// key the console fetch — the token and the metadata record differ by
/// type (`credentialHeaders` reads `oauth.access` OR `key`,
/// opencode.ts:535-541).
struct ConsoleCredential<'a> {
    token: &'a str,
    metadata: Option<&'a BTreeMap<String, Value>>,
}

impl<'a> ConsoleCredential<'a> {
    fn of(value: &'a CredentialValue) -> Option<Self> {
        match value {
            CredentialValue::OAuth(oauth) => Some(Self {
                token: &oauth.access,
                metadata: oauth.metadata.as_ref(),
            }),
            CredentialValue::Key(key) => Some(Self { token: &key.key, metadata: key.metadata.as_ref() }),
        }
    }

    fn server(&self) -> String {
        self.metadata
            .and_then(|metadata| metadata.get("server"))
            .and_then(Value::as_str)
            .filter(|server| !server.is_empty())
            .map_or_else(|| DEFAULT_SERVER.to_owned(), str::to_owned)
    }
}

/// `credentialHeaders` (opencode.ts:535-541): the bearer token is the
/// OAuth access token OR the service-account key; `x-org-id` rides the
/// credential metadata when present.
fn credential_headers(credential: &ConsoleCredential<'_>) -> Vec<(String, String)> {
    let mut headers =
        vec![("authorization".to_owned(), format!("Bearer {}", credential.token))];
    if let Some(org) = credential
        .metadata
        .and_then(|metadata| metadata.get("orgID"))
        .and_then(Value::as_str)
    {
        headers.push(("x-org-id".to_owned(), org.to_owned()));
    }
    headers
}

/// Fetches the console config; `Ok(None)` = 404 (no console config for
/// this account, opencode.ts:480).
pub async fn fetch_config(credential: &CredentialValue) -> Result<Option<ConsoleConfig>, ConsoleError> {
    let view = ConsoleCredential::of(credential).ok_or_else(|| {
        ConsoleError::Http("unsupported console credential type".to_owned())
    })?;
    let server = view.server();
    let client = reqwest::Client::new();
    let mut request = client
        .get(format!("{}/api/v2/config", server.trim_end_matches('/')))
        .header("Accept", "application/json");
    for (key, value) in credential_headers(&view) {
        request = request.header(key, value);
    }
    let response = request.send().await.map_err(|error| ConsoleError::Http(error.to_string()))?;
    let status = response.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if status == 401 || status == 403 {
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let tag = body["_tag"].as_str().unwrap_or_default();
        if status == 401 && tag == "Unauthorized" {
            return Err(ConsoleError::SignedOut);
        }
        if status == 403 && tag == "SsoRequired" {
            let connection = body["connectionId"].as_str().unwrap_or_default();
            let base = server.trim_end_matches('/');
            let redirect_to = match reqwest::Url::parse(base).ok() {
                Some(url) => {
                    let path = url.path().trim_end_matches('/');
                    format!("{path}/")
                }
                None => "/".to_owned(),
            };
            let url = format!(
                "{base}/auth/sso/{}/start?redirectTo={}",
                urlencoding::encode(connection),
                urlencoding::encode(&redirect_to)
            );
            return Err(ConsoleError::SsoRequired { organization: None, url });
        }
        return Err(ConsoleError::Http(format!("console returned {status}")));
    }
    if !(200..300).contains(&status) {
        return Err(ConsoleError::Http(format!("console returned {status}")));
    }
    let config: ConsoleConfig =
        response.json().await.map_err(|error| ConsoleError::Parse(error.to_string()))?;
    Ok(Some(config))
}

/// Credentials never ride provider settings (opencode.ts:547-552).
fn without_credentials(settings: &Value) -> Value {
    match settings {
        Value::Object(map) => {
            let filtered: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "apiKey" | "authToken" | "accessToken"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            Value::Object(filtered)
        }
        other => other.clone(),
    }
}

/// Projects console providers onto custom-provider route material.
/// Provider-level `settings.baseURL` becomes the endpoint (the
/// openai-compatible binding needs one); per-model baseURL overlays and
/// capability/cost/limit merges stay deferred (logged in be3b.log).
#[must_use]
pub fn to_custom_providers(config: &ConsoleConfig) -> Vec<CustomProvider> {
    config
        .providers
        .iter()
        .filter_map(|(id, provider)| {
            let settings = provider.settings.as_ref().map_or(Value::Null, without_credentials);
            let base_url = settings.get("baseURL").and_then(Value::as_str)?.to_owned();
            let models: Vec<CustomModel> = provider
                .models
                .as_ref()
                .map(|models| {
                    models
                        .iter()
                        .filter(|(_, model)| model.disabled != Some(true))
                        .map(|(model_id, model)| CustomModel {
                            id: model_id.clone(),
                            name: model.name.clone(),
                            headers: model.headers.clone(),
                            body: model.body.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            if models.is_empty() {
                return None;
            }
            Some(CustomProvider {
                id: id.clone(),
                name: provider.name.clone().unwrap_or_else(|| id.clone()),
                base_url,
                api_key: None,
                headers: provider.headers.clone(),
                body: provider.body.clone(),
                models,
            })
        })
        .collect()
}

/// The resolver-visible console provider snapshot (id → provider).
#[derive(Default)]
pub struct ConsoleProviders {
    entries: DashMap<String, CustomProvider>,
    /// The resolved Console token (CRED-B2): the ACTIVE `opencode`
    /// credential's OAuth access token. Console-issued providers
    /// authenticate with it — their integrationID is "opencode"
    /// (opencode.ts:251) and the resolver's credential fold maps the
    /// oauth access token onto the api key
    /// (model-resolver.ts:308-319,364-371).
    token: parking_lot::RwLock<Option<String>>,
    /// The Console managed-policy state (CONFIG-C7, managed-policy.ts):
    /// the organization statements + organization name the config
    /// policy runtime consults on both execution surfaces.
    managed: std::sync::Arc<crate::config::managed_policy::ManagedPolicy>,
}

impl ConsoleProviders {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<CustomProvider> {
        self.entries.get(id).map(|entry| entry.value().clone())
    }

    /// The resolved Console token console-issued providers carry as
    /// their bearer key (CRED-B2).
    #[must_use]
    pub fn token(&self) -> Option<String> {
        self.token.read().clone()
    }

    /// The Console managed-policy state (CONFIG-C7): the organization
    /// statements the config policy runtime merges after every
    /// authored document's.
    #[must_use]
    pub fn managed(&self) -> &std::sync::Arc<crate::config::managed_policy::ManagedPolicy> {
        &self.managed
    }

    fn set_token(&self, token: Option<String>) {
        *self.token.write() = token;
    }

    /// Case-insensitive id iteration for the resolver's prefix match.
    pub fn entries_iter(&self) -> impl Iterator<Item = (String, CustomProvider)> + '_ {
        self.entries.iter().map(|entry| (entry.key().clone(), entry.value().clone()))
    }

    fn swap(&self, next: Vec<CustomProvider>) {
        self.entries.clear();
        for provider in next {
            self.entries.insert(provider.id.clone(), provider);
        }
    }

    /// Test seam: swap from outside the host.
    pub fn swap_for_test(&self, next: Vec<CustomProvider>) {
        self.swap(next);
    }
}

impl IntegrationHost {
    /// The shared console snapshot resolvers read.
    #[must_use]
    pub fn console_providers(&self) -> &Arc<ConsoleProviders> {
        &self.console_providers_shared
    }

    /// One console refresh: fetch the config with the ACTIVE credential,
    /// project providers, and report connection statuses on the console
    /// error shapes (opencode.ts:181-207 `connectionStatus`). The active
    /// credential's token rides the shared snapshot (CRED-B2) so
    /// resolvers bind console-issued providers with it; a missing
    /// connection clears both.
    pub async fn refresh_console(self: &Arc<Self>) {
        let Some(connection) = self.connection_active("opencode").await.ok().flatten() else {
            self.console_providers_shared.set_token(None);
            self.console_providers_shared
                .managed()
                .set(crate::config::managed_policy::ManagedState::default());
            return;
        };
        let ConnectionInfo::Credential { id, .. } = &connection else {
            self.console_providers_shared.set_token(None);
            self.console_providers_shared
                .managed()
                .set(crate::config::managed_policy::ManagedState::default());
            return;
        };
        let Some(stored) = self.store().get(id).await.ok().flatten() else {
            self.console_providers_shared.set_token(None);
            self.console_providers_shared
                .managed()
                .set(crate::config::managed_policy::ManagedState::default());
            return;
        };
        // CRED-CONSOLE-KEYAUTH: BOTH credential types resolve the
        // console connection — OAuth access tokens and service-account
        // keys (`credentialHeaders` accepts `oauth.access` OR `key`,
        // opencode.ts:535-541); the token keys the shared snapshot so
        // resolvers bind console-issued providers with it.
        let Some(view) = ConsoleCredential::of(&stored.value) else {
            self.console_providers_shared.set_token(None);
            self.console_providers_shared
                .managed()
                .set(crate::config::managed_policy::ManagedState::default());
            return;
        };
        let token = view.token.to_owned();
        // `organizationName` (opencode.ts:531-533): the credential's
        // `orgName` metadata, when present.
        let organization = view
            .metadata
            .and_then(|metadata| metadata.get("orgName"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        self.console_providers_shared.set_token(Some(token));
        match fetch_config(&stored.value).await {
            Ok(Some(config)) => {
                // `publish` (opencode.ts:244-245): statements ride the
                // snapshot — `managed.set` REPLACES the state (a load,
                // 404, or disconnect swaps them wholesale).
                self.console_providers_shared.managed().set(
                    crate::config::managed_policy::ManagedState {
                        statements: config
                            .experimental
                            .as_ref()
                            .and_then(|experimental| experimental.policies.clone())
                            .unwrap_or_default(),
                        organization: organization.clone(),
                    },
                );
                self.console_providers_shared.swap(to_custom_providers(&config));
                // A healthy load clears any previously reported status.
                self.connection_status("opencode", &connection, None).await;
            }
            Ok(None) => {
                self.console_providers_shared.managed().set(
                    crate::config::managed_policy::ManagedState {
                        statements: Vec::new(),
                        organization: organization.clone(),
                    },
                );
                self.console_providers_shared.swap(Vec::new());
                self.connection_status("opencode", &connection, None).await;
            }
            Err(ConsoleError::SsoRequired { organization, url }) => {
                self.connection_status(
                    "opencode",
                    &connection,
                    Some(ConnectionStatus {
                        status: "needs_auth".into(),
                        message: sso_message(organization.as_deref()),
                        url: Some(url),
                    }),
                )
                .await;
            }
            Err(ConsoleError::SignedOut) => {
                self.connection_status(
                    "opencode",
                    &connection,
                    Some(ConnectionStatus {
                        status: "needs_auth".into(),
                        message: "Reconnect OpenCode Console to continue".into(),
                        url: None,
                    }),
                )
                .await;
            }
            // Transient failures never change the connection status
            // (opencode.ts:188-192) — and keep the last config's
            // statements too: dropping them would lift organization
            // policy while personal credentials keep working
            // (opencode.ts:218-226).
            Err(ConsoleError::Http(_) | ConsoleError::Parse(_)) => {}
        }
    }
}
