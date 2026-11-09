//! Azure deployment discovery (G-259; port of
//! `plugin/provider/azure.ts:179-340` + `:496-537` over the models.dev
//! catalog): when an Azure credential exists, list the resource's actual
//! model deployments and NARROW the azure provider's model surface to
//! them — only catalog models with a matching deployment stay visible,
//! each re-keyed by its deployment name.
//!
//! Two inventory lanes (azure.ts:262-272): an Entra ID (Azure CLI)
//! credential reads the Azure Resource Manager inventory first —
//! Resource Graph resolves the resource's ARM id, then the management
//! deployments endpoint paginates — falling back to the resource's own
//! data-plane list; an API key reads the data-plane list directly
//! (`/deployments?api-version=2022-12-01`, the only version that has
//! it).
//!
//! The fetch itself is [`AzureSync::refresh`] — production callers
//! (engine boot) run it against the resolved credential; tests seed the
//! state directly through [`AzureSync::seed_for_test`] or point
//! [`AzureEndpoints`] at a loopback stub, so no network access beyond
//! the stub ever happens in the test harness (ADR-0006).

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_ai::route::transport::{read_body_limited, SharedTransport, TransportRequest};
use oc_schema::id::ModelId;
use oc_schema::model as schema_model;
use serde_json::Value;

use crate::provider_catalog::{Catalog, Snapshot};

/// The azure provider id (azure.ts `Provider.ID.azure`).
pub const AZURE_PROVIDER_ID: &str = "azure";

/// The management endpoint (azure.ts:60).
pub const AZURE_MANAGEMENT_ENDPOINT: &str = "https://management.azure.com";

/// The resource data-plane URL template (azure.ts:59
/// `https://{name}.openai.azure.com/openai`); `{name}` substitutes the
/// resource name. A template (rather than the ref's closure) keeps the
/// seam data-only — tests point it at a loopback stub.
pub const AZURE_RESOURCE_URL_TEMPLATE: &str = "https://{name}.openai.azure.com/openai";

/// Resource Graph query API version (azure.ts:192).
pub const AZURE_RESOURCEGRAPH_API_VERSION: &str = "2022-10-01";

/// The management deployments list API version (azure.ts:209).
pub const AZURE_MANAGEMENT_DEPLOYMENTS_API_VERSION: &str = "2024-10-01";

/// The resource data-plane deployments API version (azure.ts:296).
pub const AZURE_RESOURCE_DEPLOYMENTS_API_VERSION: &str = "2022-12-01";

/// Every discovery request's timeout (azure.ts:197, 212).
pub const AZURE_DISCOVERY_TIMEOUT_MS: u64 = 10_000;

/// The response body cap for discovery fetches.
pub const AZURE_DISCOVERY_BODY_LIMIT: usize = 4 * 1024 * 1024;

/// The Azure CLI OAuth method id (azure.ts:22).
pub const AZURE_CLI_METHOD_ID: &str = "azure-cli";

/// The resource-name env var (azure.ts:476).
pub const AZURE_RESOURCE_NAME_ENV: &str = "AZURE_RESOURCE_NAME";

/// The legacy resource-name env var (azure.ts:476).
pub const AZURE_LEGACY_RESOURCE_NAME_ENV: &str = "AZURE_COGNITIVE_SERVICES_RESOURCE_NAME";

/// A page-chain guard the ref gets from Effect's stream machinery; a
/// malformed `nextLink` loop must not spin forever.
const AZURE_DEPLOYMENTS_PAGE_LIMIT: usize = 1_000;

// ---------------------------------------------------------------------------
// Endpoints (azure.ts:57-62 `make(endpoints)` — the test seam)
// ---------------------------------------------------------------------------

/// The discovery endpoints (azure.ts:57-62): the resource data-plane
/// URL template and the management origin. Tests substitute loopback
/// stubs for both.
#[derive(Debug, Clone)]
pub struct AzureEndpoints {
    /// `https://{name}.openai.azure.com/openai` by default.
    pub resource_template: String,
    /// `https://management.azure.com` by default.
    pub management: String,
}

impl Default for AzureEndpoints {
    fn default() -> Self {
        Self {
            resource_template: AZURE_RESOURCE_URL_TEMPLATE.to_owned(),
            management: AZURE_MANAGEMENT_ENDPOINT.to_owned(),
        }
    }
}

impl AzureEndpoints {
    /// The resource's data-plane base (azure.ts:59).
    #[must_use]
    pub fn resource_url(&self, name: &str) -> String {
        self.resource_template.replace("{name}", name)
    }

    /// The resource's own deployment list URL (azure.ts:296).
    #[must_use]
    pub fn resource_deployments_url(&self, name: &str) -> String {
        format!(
            "{}/deployments?api-version={AZURE_RESOURCE_DEPLOYMENTS_API_VERSION}",
            self.resource_url(name)
        )
    }
}

// ---------------------------------------------------------------------------
// The deployment inventory (azure.ts:35-55, decoded tolerantly)
// ---------------------------------------------------------------------------

/// One deployment: its name (the resource list's `id`, the management
/// list's `name`) and the model it deploys (azure.ts:55).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AzureDeployment {
    pub name: String,
    pub model: String,
}

/// `decodeResourceDeployment` (azure.ts:36-38, 253-258): a
/// `{data: [...]}` body keeps only `status === "succeeded"` rows with
/// non-empty `id`/`model`; one malformed row never discards the batch.
#[must_use]
pub fn parse_resource_deployments(body: &str) -> Result<Vec<AzureDeployment>, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| format!("Azure deployments response is not JSON: {error}"))?;
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return Err("Azure deployments response has no data array".to_owned());
    };
    Ok(data
        .iter()
        .filter(|raw| raw.get("status").and_then(Value::as_str) == Some("succeeded"))
        .filter_map(|raw| {
            let name = raw.get("id")?.as_str()?;
            let model = raw.get("model")?.as_str()?;
            (!name.is_empty() && !model.is_empty())
                .then(|| AzureDeployment { name: name.to_owned(), model: model.to_owned() })
        })
        .collect())
}

/// One management deployments page (azure.ts:39-42, 204-234): the
/// `provisioningState === "Succeeded"` deployments plus the optional
/// `nextLink` for the next page.
#[must_use]
pub fn parse_management_deployments_page(
    body: &str,
) -> Result<(Vec<AzureDeployment>, Option<String>), String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| format!("Azure deployments page is not JSON: {error}"))?;
    let Some(entries) = value.get("value").and_then(Value::as_array) else {
        return Err("Azure deployments page has no value array".to_owned());
    };
    let deployments = entries
        .iter()
        .filter_map(|raw| {
            let name = raw.get("name")?.as_str()?;
            let properties = raw.get("properties")?;
            let model = properties.get("model")?.get("name")?.as_str()?;
            let state = properties.get("provisioningState")?.as_str()?;
            (state == "Succeeded" && !name.is_empty() && !model.is_empty())
                .then(|| AzureDeployment { name: name.to_owned(), model: model.to_owned() })
        })
        .collect();
    let next_link = value
        .get("nextLink")
        .and_then(Value::as_str)
        .filter(|link| !link.is_empty())
        .map(str::to_owned);
    Ok((deployments, next_link))
}

/// The Resource Graph response (azure.ts:52-53, 199-201): the first
/// `data[].id`, if any.
#[must_use]
pub fn parse_resource_graph_response(body: &str) -> Result<Option<String>, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| format!("Azure Resource Graph response is not JSON: {error}"))?;
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return Err("Azure Resource Graph response has no data array".to_owned());
    };
    Ok(data
        .iter()
        .find_map(|row| row.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()))
        .map(str::to_owned))
}

/// The Resource Graph KQL query for one resource name (azure.ts:496-508):
/// cognitiveservices accounts of the AIServices/OpenAI kinds whose custom
/// subdomain is the resource name — subdomains are globally unique, so a
/// name matches at most one resource.
#[must_use]
pub fn resource_query(resource: &str) -> String {
    [
        "resources",
        "| where type =~ 'microsoft.cognitiveservices/accounts' and kind in~ ('AIServices', 'OpenAI')",
        "| extend resourceName = tostring(properties.customSubDomainName)",
        &format!("| where resourceName =~ '{resource}'"),
        "| project id",
        "| take 1",
    ]
    .join(" ")
}

/// A resource name becomes a hostname label and a query literal, so
/// anything else never leaves the process (azure.ts:23-26): letters,
/// digits, and hyphens, not starting with a hyphen.
#[must_use]
pub fn valid_resource_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z' | 'A'..='Z' | '0'..='9'))
        && chars.all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-'))
}

/// `resolveResourceName` (azure.ts:473-477): the configured name (a
/// trimmed non-empty string) wins, then the connection's fallback, then
/// the environment chain.
#[must_use]
pub fn resolve_resource_name(configured: Option<&str>, fallback: Option<&str>) -> Option<String> {
    let non_empty = |value: Option<&str>| {
        value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
    };
    non_empty(configured).or_else(|| non_empty(fallback)).or_else(env_resource_name)
}

/// The environment half of `resolveResourceName` (azure.ts:476):
/// `AZURE_RESOURCE_NAME`, then the legacy
/// `AZURE_COGNITIVE_SERVICES_RESOURCE_NAME`.
#[must_use]
pub fn env_resource_name() -> Option<String> {
    [AZURE_RESOURCE_NAME_ENV, AZURE_LEGACY_RESOURCE_NAME_ENV]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// The narrowing transform (azure.ts:382-411 + 513-537)
// ---------------------------------------------------------------------------

/// `catalogModel` (azure.ts:535-537): a deployment maps to the catalog
/// model named by its MODEL first (Azure spells some models unlike the
/// catalog), then by its own name.
fn catalog_model<'a>(
    models: &BTreeMap<String, &'a schema_model::Info>,
    deployment: &AzureDeployment,
) -> Option<&'a schema_model::Info> {
    models
        .get(&deployment.model.to_lowercase())
        .or_else(|| models.get(&deployment.name.to_lowercase()))
        .copied()
}

/// `deployedModels` (azure.ts:513-529): each deployment that maps to a
/// catalog model becomes that model re-keyed by its deployment name —
/// `id` lowercase (Azure compares names without case), `modelID` the
/// deployment's spelling, and the name disambiguated when the id is not
/// the catalog model's own. Deployments of models outside the catalog
/// need explicit configuration and drop (azure.ts:331-337).
#[must_use]
pub fn deployed_models(
    deployments: &[AzureDeployment],
    catalog: &[schema_model::Info],
) -> BTreeMap<String, schema_model::Info> {
    let by_lower: BTreeMap<String, &schema_model::Info> =
        catalog.iter().map(|model| (model.id.0.to_lowercase(), model)).collect();
    let mut deployed: BTreeMap<String, schema_model::Info> = BTreeMap::new();
    for deployment in deployments {
        let Some(model) = catalog_model(&by_lower, deployment) else {
            continue; // "models outside the catalog need explicit configuration"
        };
        let id = deployment.name.to_lowercase();
        let mut info = model.clone();
        info.id = ModelId::parse(id.clone());
        info.model_id = ModelId::parse(deployment.name.clone());
        info.name = if id == model.id.0.to_lowercase() {
            model.name.clone()
        } else {
            format!("{} ({})", model.name, deployment.name)
        };
        deployed.insert(id, info);
    }
    deployed
}

/// The model transform (azure.ts:400-411): catalog models without a
/// deployment are REMOVED; the deployed ones replace/append. Provider
/// info/environment stay the static record's (the narrowing is
/// resolution-visible only — every catalog model stays available as
/// the base of a user-configured model, azure.ts:401-402).
#[must_use]
pub fn merge_azure_snapshot(snapshot: &Snapshot, deployments: &[AzureDeployment]) -> Snapshot {
    let deployed = deployed_models(deployments, &snapshot.models);
    let mut models = Vec::new();
    let mut appended = std::collections::BTreeSet::new();
    // The removal pass keeps the catalog's order for surviving ids.
    for model in &snapshot.models {
        if let Some(info) = deployed.get(&model.id.0) {
            models.push(info.clone());
            appended.insert(model.id.0.clone());
        }
    }
    // `models.update` on an id the catalog lacks creates it (appended).
    for (id, info) in &deployed {
        if !appended.contains(id) {
            models.push(info.clone());
        }
    }
    Snapshot { info: snapshot.info.clone(), models, environment: snapshot.environment.clone() }
}

// ---------------------------------------------------------------------------
// The sync (the plugin's `discover` + transforms, offline-testable)
// ---------------------------------------------------------------------------

/// The resolved credential the discovery authenticates with
/// (azure.ts:236-272): an API key reads the resource's own inventory;
/// an Entra ID token reads the management inventory first, falling
/// back to the resource list.
#[derive(Debug, Clone)]
pub enum AzureAuth {
    /// `credential.type === "key"` — the `api-key` header, data-plane
    /// only (azure.ts:245-246, 272).
    ApiKey(String),
    /// `credential.type === "oauth"` — bearer tokens: `management` for
    /// the ARM inventory (azure.ts:107-118; `None` when the CLI cannot
    /// mint it, leaving the data-plane fallback), `resource` for the
    /// data-plane lane (azure.ts:269-271).
    Entra { management: Option<String>, resource: String },
}

/// The engine-side credential probe (azure.ts:180-186, 289-311): the
/// active azure connection's stored credential decides the lane — an
/// API key (its form answer carries the resource name) or an azure-cli
/// OAuth credential (its metadata carries the resource name; the
/// per-scope tokens are minted through the CLI). An env connection
/// resolves to a key credential (integration.ts:696-699) with the
/// resource from the environment chain.
pub struct AzureDiscoveryInput {
    pub resource: String,
    pub auth: AzureAuth,
}

/// The scopes the discovery mints CLI tokens for (azure.ts:19-21).
pub const AZURE_MANAGEMENT_SCOPE: &str = "https://management.azure.com/.default";
/// azure.ts:19.
pub const AZURE_COGNITIVE_SCOPE: &str = "https://cognitiveservices.azure.com/.default";

/// Resolve the discovery input from the integration host's active
/// azure connection (azure.ts:179-186 `load` + :289-311 `discover`'s
/// credential half). `None` = no drivable credential — the sync stays
/// inert and the static catalog serves.
pub async fn resolve_azure_discovery_input(
    host: &crate::integration::IntegrationHost,
) -> Option<AzureDiscoveryInput> {
    use crate::integration::types::{ConnectionInfo, CredentialValue};
    let non_empty = |value: Option<String>| value.filter(|value| !value.is_empty());
    let credential_resource = |configuration: Option<&serde_json::Value>| {
        non_empty(
            configuration
                .and_then(|config| config.get("resourceName"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .map(str::to_owned),
        )
    };
    let metadata_resource =
        |metadata: Option<&std::collections::BTreeMap<String, serde_json::Value>>| {
            non_empty(
                metadata
                    .and_then(|metadata| metadata.get("resourceName"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .map(str::to_owned),
            )
        };
    match host.connection_active(AZURE_PROVIDER_ID).await.ok()? {
        Some(ConnectionInfo::Credential { id, .. }) => {
            let stored = host.store().get(&id).await.ok()??;
            match &stored.value {
                // azure.ts:272 — a key reads the resource's own list;
                // the form answer carries the resource name
                // (azure.ts:159-160, 485-494).
                CredentialValue::Key(key) => Some(AzureDiscoveryInput {
                    resource: credential_resource(key.configuration.as_ref())
                        .or_else(env_resource_name)?,
                    auth: AzureAuth::ApiKey(key.key.clone()),
                }),
                // azure.ts:269-271 — the azure-cli method reads ARM
                // first; other oauth methods never drive discovery
                // (azure.ts:311).
                CredentialValue::OAuth(oauth) if oauth.method_id == AZURE_CLI_METHOD_ID => {
                    let resource =
                        metadata_resource(oauth.metadata.as_ref()).or_else(env_resource_name)?;
                    // azure.ts:92-105 — per-scope tokens through the
                    // CLI; a failed management mint still leaves the
                    // data-plane lane.
                    let management = crate::integration::providers_enterprise::azure_cli_token(
                        AZURE_MANAGEMENT_SCOPE,
                    )
                    .await
                    .ok()
                    .map(|(access, _)| access);
                    let data_plane = crate::integration::providers_enterprise::azure_cli_token(
                        AZURE_COGNITIVE_SCOPE,
                    )
                    .await
                    .ok()
                    .map(|(access, _)| access)?;
                    Some(AzureDiscoveryInput {
                        resource,
                        auth: AzureAuth::Entra { management, resource: data_plane },
                    })
                }
                _ => None,
            }
        }
        // An env connection resolves to a key credential
        // (integration.ts:696-699): the var's value is the key and the
        // resource comes from the environment chain
        // (azure.ts:473-477).
        Some(ConnectionInfo::Env { name, .. }) => {
            let key = std::env::var(&name).ok().filter(|value| !value.is_empty())?;
            Some(AzureDiscoveryInput {
                resource: env_resource_name()?,
                auth: AzureAuth::ApiKey(key),
            })
        }
        None => None,
    }
}

/// One successful sync: the resource it listed, its normalized
/// deployment inventory, and the narrowed provider snapshot.
pub struct AzureSyncState {
    pub resource: String,
    pub deployments: Vec<AzureDeployment>,
    pub merged: Snapshot,
}

/// The synced azure surface shared by the resolver (deployment
/// narrowing): refreshed by the engine against the resolved
/// credential. A missing/invalid resource name or a failed fetch keeps
/// the previous state (azure.ts:293-318 — the catalog serves until a
/// discovery succeeds).
pub struct AzureSync {
    transport: SharedTransport,
    state: parking_lot::RwLock<Option<Arc<AzureSyncState>>>,
    /// A resource keeps its ARM id until it is deleted, so discovery
    /// looks it up once (azure.ts:73-74); a failed listing evicts it
    /// (azure.ts:231-233).
    resource_ids: parking_lot::Mutex<BTreeMap<String, String>>,
}

impl AzureSync {
    #[must_use]
    pub fn new(transport: SharedTransport) -> Self {
        Self {
            transport,
            state: parking_lot::RwLock::new(None),
            resource_ids: parking_lot::Mutex::new(BTreeMap::new()),
        }
    }

    /// The current sync state (shared handle, cloned out of the cell).
    #[must_use]
    pub fn state(&self) -> Option<Arc<AzureSyncState>> {
        self.state.read().clone()
    }

    /// Drop the loaded state — the resolver serves the full static
    /// catalog again (azure.ts:300-306's clear on a changed target).
    pub fn clear(&self) {
        *self.state.write() = None;
    }

    /// Production refresh (azure.ts `discover` + the fetch half):
    /// authenticate per the credential kind, list the deployments
    /// (management lane with Resource Graph lookup + pagination for
    /// Entra, data-plane for keys and as the Entra fallback), then
    /// rebuild the narrowed snapshot. Any failure keeps the previous
    /// state (azure.ts:315-317).
    pub async fn refresh(
        &self,
        catalog: &Catalog,
        endpoints: &AzureEndpoints,
        resource: String,
        auth: AzureAuth,
        user_agent: &str,
    ) {
        // azure.ts:295 — an unparsable resource name never leaves the
        // process; a custom baseURL keeps the catalog (the caller's
        // gate).
        if !valid_resource_name(&resource) {
            return;
        }
        let found = match &auth {
            // azure.ts:272 — a key goes straight to the resource list.
            AzureAuth::ApiKey(key) => {
                self.resource_deployments(
                    &endpoints.resource_deployments_url(&resource),
                    "api-key",
                    key,
                    user_agent,
                )
                .await
            }
            // azure.ts:269-271 — Entra reads ARM first, falling back to
            // the resource's own list when the management API is
            // unreadable (or the CLI never minted its token).
            AzureAuth::Entra { management, resource: data_plane } => {
                let from_management = match management.as_deref() {
                    Some(token) => {
                        self.management_deployments(endpoints, &resource, token, user_agent).await
                    }
                    None => None,
                };
                match from_management {
                    Some(list) => Some(list),
                    None => {
                        self.resource_deployments(
                            &endpoints.resource_deployments_url(&resource),
                            "Authorization",
                            &format!("Bearer {data_plane}"),
                            user_agent,
                        )
                        .await
                    }
                }
            }
        };
        let Some(mut found) = found else {
            return; // azure.ts:315-317 — log + keep the previous state
        };
        // azure.ts:313-314 — Azure promises no order; normalize so a
        // reordered response does not rebuild the model list. (The ref
        // compares with `localeCompare`; the port's ASCII fold matches
        // for the deployment names Azure allows.)
        found.sort_by(|a, b| {
            a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name))
        });
        self.store(catalog, resource, found);
    }

    /// The resource's own data-plane inventory (azure.ts:236-260):
    /// `{resource}/deployments?api-version=2022-12-01` with the
    /// `api-key` header or a bearer token.
    async fn resource_deployments(
        &self,
        url: &str,
        auth_header: &str,
        auth_value: &str,
        user_agent: &str,
    ) -> Option<Vec<AzureDeployment>> {
        let body = self.get_json(url, &[(auth_header, auth_value)], user_agent).await?;
        parse_resource_deployments(&body).ok()
    }

    /// The ARM inventory (azure.ts:188-234): Resource Graph resolves
    /// the resource's ARM id (cached per resource name), then the
    /// management deployments endpoint paginates. A `nextLink` off the
    /// management origin fails the whole lane (azure.ts:214-217) — the
    /// token must never leave the management endpoint.
    async fn management_deployments(
        &self,
        endpoints: &AzureEndpoints,
        resource: &str,
        management_token: &str,
        user_agent: &str,
    ) -> Option<Vec<AzureDeployment>> {
        let key = resource.to_lowercase();
        // Hoisted out of a `match` scrutinee: a parking_lot guard is
        // not Send, so it must not live across the lookup await.
        let cached = self.resource_ids.lock().get(&key).cloned();
        let id = match cached {
            Some(id) => id,
            None => {
                let found =
                    self.find_resource(endpoints, resource, management_token, user_agent).await?;
                self.resource_ids.lock().insert(key.clone(), found.clone());
                found
            }
        };
        let result = self
            .paginate_management_deployments(
                &format!(
                    "{management}{id}/deployments?api-version={AZURE_MANAGEMENT_DEPLOYMENTS_API_VERSION}",
                    management = endpoints.management,
                ),
                &url_origin(&endpoints.management)?,
                management_token,
                user_agent,
            )
            .await;
        if result.is_none() {
            // azure.ts:231-233 — a moved/recreated resource has a new
            // id, so the next discovery looks it up again.
            self.resource_ids.lock().remove(&key);
        }
        result
    }

    /// `findResource` (azure.ts:190-202): the Resource Graph query for
    /// the resource name; the first `data[].id` or nothing.
    async fn find_resource(
        &self,
        endpoints: &AzureEndpoints,
        resource: &str,
        management_token: &str,
        user_agent: &str,
    ) -> Option<String> {
        let url = format!(
            "{management}/providers/Microsoft.ResourceGraph/resources?api-version={AZURE_RESOURCEGRAPH_API_VERSION}",
            management = endpoints.management
        );
        let body = serde_json::to_string(&serde_json::json!({ "query": resource_query(resource) }))
            .ok()?;
        let response = self
            .send(TransportRequest {
                method: "POST".to_owned(),
                url,
                headers: vec![
                    ("Authorization".to_owned(), format!("Bearer {management_token}")),
                    ("Accept".to_owned(), "application/json".to_owned()),
                    ("User-Agent".to_owned(), user_agent.to_owned()),
                ],
                body: body.into(),
            })
            .await?;
        parse_resource_graph_response(&response).ok().flatten()
    }

    /// The `Stream.paginate` loop (azure.ts:209-233): each page's
    /// `nextLink` continues the chain while it stays on the management
    /// origin.
    async fn paginate_management_deployments(
        &self,
        first: &str,
        origin: &str,
        management_token: &str,
        user_agent: &str,
    ) -> Option<Vec<AzureDeployment>> {
        let mut deployments = Vec::new();
        let mut url = first.to_owned();
        for _ in 0..AZURE_DEPLOYMENTS_PAGE_LIMIT {
            let body = self
                .get_json(
                    &url,
                    &[("Authorization", &format!("Bearer {management_token}"))],
                    user_agent,
                )
                .await?;
            let (page, next) = parse_management_deployments_page(&body).ok()?;
            deployments.extend(page);
            match next {
                Some(link) => {
                    // azure.ts:214-217 — a page link outside the
                    // management endpoint fails the lane.
                    if url_origin(&link).as_deref() != Some(origin) {
                        return None;
                    }
                    url = link;
                }
                None => return Some(deployments),
            }
        }
        None // a malformed nextLink loop must not spin forever
    }

    /// One GET with the given auth header, 2xx-checked and body-capped
    /// (`HttpClient.filterStatusOk`, azure.ts:71).
    async fn get_json(
        &self,
        url: &str,
        authorization: &[(&str, &str)],
        user_agent: &str,
    ) -> Option<String> {
        let mut headers: Vec<(String, String)> = authorization
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        headers.push(("Accept".to_owned(), "application/json".to_owned()));
        headers.push(("User-Agent".to_owned(), user_agent.to_owned()));
        self.send(TransportRequest {
            method: "GET".to_owned(),
            url: url.to_owned(),
            headers,
            body: Default::default(),
        })
        .await
    }

    /// The shared send: 10s timeout, 2xx status gate, bounded body.
    async fn send(&self, request: TransportRequest) -> Option<String> {
        let response = tokio::time::timeout(
            std::time::Duration::from_millis(AZURE_DISCOVERY_TIMEOUT_MS),
            Arc::clone(&self.transport).execute(request),
        )
        .await
        .ok()?
        .ok()?;
        if !(200..300).contains(&response.status) {
            return None;
        }
        read_body_limited(response.body, AZURE_DISCOVERY_BODY_LIMIT).await.ok()
    }

    /// Rebuild + store the narrowed state over an already-loaded
    /// inventory (the offline seam: tests seed state without network).
    pub fn store(&self, catalog: &Catalog, resource: String, deployments: Vec<AzureDeployment>) {
        let Some(static_snapshot) = catalog.provider(AZURE_PROVIDER_ID) else {
            return;
        };
        let merged = merge_azure_snapshot(static_snapshot, &deployments);
        *self.state.write() = Some(Arc::new(AzureSyncState { resource, deployments, merged }));
    }

    /// Test seam: seed the synced state directly (ADR-0006 — the
    /// production fetch path must not run during tests).
    pub fn seed_for_test(
        &self,
        catalog: &Catalog,
        resource: &str,
        deployments: Vec<AzureDeployment>,
    ) {
        self.store(catalog, resource.to_owned(), deployments);
    }
}

/// The `new URL(x).origin` subset the port needs: scheme://host[:port].
fn url_origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog() -> Arc<Catalog> {
        Arc::new(Catalog::embedded().clone())
    }

    fn transport() -> SharedTransport {
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new())
    }

    /// A minimal azure snapshot with the four catalog models the ref
    /// test seeds (provider-azure.test.ts:184-204).
    fn seeded_snapshot() -> Snapshot {
        let snapshot = catalog().provider(AZURE_PROVIDER_ID).expect("azure in catalog").clone();
        let pick = |id: &str| {
            snapshot
                .models
                .iter()
                .find(|model| model.id.as_str() == id)
                .cloned()
                .unwrap_or_else(|| panic!("catalog model {id}"))
        };
        let mut gpt5_mini = pick("gpt-5-mini");
        gpt5_mini.name = "GPT-5 Mini".to_owned();
        gpt5_mini.limit.context = 400_000;
        gpt5_mini.limit.output = 128_000;
        let mut gpt5_nano = pick("gpt-5-nano");
        gpt5_nano.name = "GPT-5 Nano".to_owned();
        gpt5_nano.limit.context = 300_000;
        gpt5_nano.limit.output = 64_000;
        Snapshot { models: vec![pick("gpt-5"), gpt5_mini, gpt5_nano], ..snapshot }
    }

    /// G-259 — the narrowing transform (azure.ts:400-411 + 513-537 over
    /// provider-azure.test.ts:261-304): only catalog models with a
    /// succeeded deployment stay, re-keyed by deployment name —
    /// lowercase id, original-case modelID, the `(deployment)` name
    /// suffix when the id is not the catalog model's own — while a
    /// deployment of an unknown model drops.
    #[test]
    fn narrowing_matches_ref_deployments() {
        let snapshot = seeded_snapshot();
        let deployments = vec![
            AzureDeployment { name: "gpt-5-mini".into(), model: "gpt-5-mini".into() },
            // A deployment named after another model deploys that model.
            AzureDeployment { name: "prod-mini".into(), model: "gpt-5-mini".into() },
            // Azure may return another case than the one created.
            AzureDeployment { name: "GPT-5-Nano".into(), model: "gpt-5-nano".into() },
            // Azure spells some models unlike the catalog.
            AzureDeployment { name: "deepseek-v4-flash".into(), model: "DeepSeek-V4-Flash".into() },
            // A model outside the catalog needs explicit configuration.
            AzureDeployment { name: "ft-legal".into(), model: "gpt-4o-mini.ft-123".into() },
        ];
        let merged = merge_azure_snapshot(&snapshot, &deployments);
        let listed: Vec<(String, String, String)> = merged
            .models
            .iter()
            .map(|model| (model.id.0.clone(), model.model_id.0.clone(), model.name.clone()))
            .collect();
        assert_eq!(
            listed,
            vec![
                ("gpt-5-mini".to_owned(), "gpt-5-mini".to_owned(), "GPT-5 Mini".to_owned()),
                ("gpt-5-nano".to_owned(), "GPT-5-Nano".to_owned(), "GPT-5 Nano".to_owned()),
                (
                    "prod-mini".to_owned(),
                    "prod-mini".to_owned(),
                    "GPT-5 Mini (prod-mini)".to_owned()
                ),
            ],
            "catalog order for survivors; unknown-model deployments drop"
        );
        // Limits and the rest of the info carry from the catalog model.
        let prod = merged.models.iter().find(|m| m.id.as_str() == "prod-mini").expect("prod");
        assert_eq!(prod.limit.context, 400_000);
        // A model spelt unlike the catalog still maps through the model
        // name (azure.ts:535-537) when the catalog carries it.
        let catalog = catalog();
        let snapshot_with_deepseek = {
            let azure = catalog.provider(AZURE_PROVIDER_ID).expect("azure");
            let mut models = seeded_snapshot().models;
            models.push(
                azure
                    .models
                    .iter()
                    .find(|m| m.id.as_str() == "deepseek-v4-flash")
                    .cloned()
                    .map(|mut m| {
                        m.name = "DeepSeek-V4-Flash".to_owned();
                        m
                    })
                    .expect("deepseek-v4-flash in the embedded catalog"),
            );
            Snapshot { models, ..seeded_snapshot() }
        };
        let merged = merge_azure_snapshot(
            &snapshot_with_deepseek,
            &[AzureDeployment {
                name: "deepseek-v4-flash".into(),
                model: "DeepSeek-V4-Flash-2026".into(),
            }],
        );
        assert!(
            merged.models.iter().any(|m| m.id.as_str() == "deepseek-v4-flash"),
            "the deployment maps through the catalog by its model spelling"
        );
    }

    /// G-259 — decode tolerances (azure.ts:35-53): non-succeeded rows
    /// and malformed entries drop; the envelope is strict.
    #[test]
    fn decode_tolerances_match_ref() {
        let resource = parse_resource_deployments(
            &json!({ "data": [
                { "id": "live", "model": "gpt-5-mini", "status": "succeeded" },
                { "id": "pending", "model": "gpt-5", "status": "creating" },
                { "id": 42 },
                "not an object",
            ]})
            .to_string(),
        )
        .expect("decodes");
        assert_eq!(
            resource,
            vec![AzureDeployment { name: "live".into(), model: "gpt-5-mini".into() }]
        );

        let (page, next) = parse_management_deployments_page(
            &json!({ "value": [
                { "name": "mini", "properties": { "model": { "name": "gpt-5-mini" }, "provisioningState": "Succeeded" } },
                { "name": "failed", "properties": { "model": { "name": "gpt-5" }, "provisioningState": "Failed" } },
                { "name": "shallow" },
            ]})
            .to_string(),
        )
        .expect("decodes");
        assert_eq!(page, vec![AzureDeployment { name: "mini".into(), model: "gpt-5-mini".into() }]);
        assert_eq!(next, None);

        let (_, next) = parse_management_deployments_page(
            &json!({ "value": [], "nextLink": "http://m.example/page-2" }).to_string(),
        )
        .expect("decodes");
        assert_eq!(next.as_deref(), Some("http://m.example/page-2"));

        let graph = parse_resource_graph_response(
            &json!({ "data": [{ "id": "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.CognitiveServices/accounts/test" }] })
                .to_string(),
        )
        .expect("decodes");
        assert_eq!(
            graph.as_deref(),
            Some("/subscriptions/sub/resourceGroups/rg/providers/Microsoft.CognitiveServices/accounts/test")
        );
        assert_eq!(
            parse_resource_graph_response(&json!({ "data": [] }).to_string()).unwrap(),
            None
        );
        assert!(parse_resource_deployments("not json").is_err());
        assert!(parse_management_deployments_page("{}").is_err());
    }

    /// G-259 — the Resource Graph query text (azure.ts:496-508) and the
    /// resource-name gate (azure.ts:23-26).
    #[test]
    fn query_and_name_gate_match_ref() {
        assert_eq!(
            resource_query("test-resource"),
            "resources | where type =~ 'microsoft.cognitiveservices/accounts' and kind in~ ('AIServices', 'OpenAI') | extend resourceName = tostring(properties.customSubDomainName) | where resourceName =~ 'test-resource' | project id | take 1"
        );
        assert!(valid_resource_name("my-models"));
        assert!(valid_resource_name("A1-b"));
        assert!(!valid_resource_name("-lead"));
        assert!(!valid_resource_name("under_score"));
        assert!(!valid_resource_name(""));
        assert!(!valid_resource_name("sp ace"));
    }

    /// `resolveResourceName` priority (azure.ts:473-477): configured
    /// (trimmed non-empty) → fallback → environment. The environment
    /// half is exercised with the same chain the helper reads.
    #[test]
    fn resource_name_priority() {
        assert_eq!(
            resolve_resource_name(Some(" from-config "), Some("fallback")),
            Some("from-config".into())
        );
        assert_eq!(resolve_resource_name(Some("   "), Some("fallback")), Some("fallback".into()));
        // No configured name: the environment chain (or None without
        // the vars — the ambient environment decides, exactly like the
        // ref reading `process.env`).
        let env = env_resource_name();
        assert_eq!(resolve_resource_name(None, None), env);
    }

    /// G-259 — offline inertness: an unseeded sync has no state (the
    /// resolver keeps the static azure catalog), `clear` empties a
    /// loaded one, and an invalid resource name refreshes nothing
    /// (azure.ts:295).
    #[test]
    fn absent_credential_is_inert() {
        let sync = AzureSync::new(transport());
        assert!(sync.state().is_none(), "no credential → no state");
        sync.seed_for_test(
            &catalog(),
            "test-resource",
            vec![AzureDeployment { name: "gpt-5-mini".into(), model: "gpt-5-mini".into() }],
        );
        let state = sync.state().expect("seeded");
        assert_eq!(state.resource, "test-resource");
        assert_eq!(state.merged.models.len(), 1);
        sync.clear();
        assert!(sync.state().is_none(), "clear → inert again");
    }

    /// The URL origin subset (the nextLink guard, azure.ts:214-217).
    #[test]
    fn url_origin_extraction() {
        assert_eq!(
            url_origin("https://management.azure.com/x?y=1").as_deref(),
            Some("https://management.azure.com")
        );
        assert_eq!(
            url_origin("http://127.0.0.1:8080/page-2").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(url_origin("ftp://example/"), None);
        assert_eq!(url_origin("management.azure.com"), None);
    }
}
