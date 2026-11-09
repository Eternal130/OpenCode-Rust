//! `DigitalOcean` inference-router discovery (G-259; port of
//! `plugin/provider/digitalocean.ts:114-180`): when a `DigitalOcean`
//! browser-OAuth credential exists, list the account's model routers
//! (`/v2/gen-ai/models/routers`) and inject each as a `router:*` model
//! entry on the digitalocean provider — the credential-driven model
//! list the ref's provider transform adds over the static catalog.
//!
//! The fetch itself is [`DigitalOceanSync::refresh`] — production
//! callers (engine boot + its 5-minute repeat) run it against the
//! resolved credential; tests seed the state directly through
//! [`DigitalOceanSync::seed_for_test`] or point the endpoint at a
//! loopback stub, so no network access beyond the stub ever happens in
//! the test harness (ADR-0006).

use std::sync::Arc;

use oc_ai::route::transport::{read_body_limited, SharedTransport, TransportRequest};
use oc_schema::id::{ModelId, ProviderId};
use oc_schema::model as schema_model;
use oc_schema::model::{Capabilities, ModelLimit};
use oc_schema::provider::Info as ProviderInfo;

use crate::provider_catalog::{Catalog, Snapshot};

/// The digitalocean provider id (digitalocean.ts:15).
pub const DIGITALOCEAN_PROVIDER_ID: &str = "digitalocean";

/// The routers inventory endpoint (digitalocean.ts:21).
pub const DIGITALOCEAN_ROUTERS_ENDPOINT: &str =
    "https://api.digitalocean.com/v2/gen-ai/models/routers";

/// The browser OAuth method id (digitalocean.ts:17).
pub const DIGITALOCEAN_BROWSER_METHOD_ID: &str = "browser";

/// The routers fetch timeout (digitalocean.ts:147).
pub const DIGITALOCEAN_ROUTERS_TIMEOUT_MS: u64 = 10_000;

/// The routers response body cap.
pub const DIGITALOCEAN_ROUTERS_BODY_LIMIT: usize = 4 * 1024 * 1024;

/// The router model family (digitalocean.ts:165).
pub const DIGITALOCEAN_ROUTER_FAMILY: &str = "digitalocean-inference-routers";

/// The router model id prefix (digitalocean.ts:161).
pub const DIGITALOCEAN_ROUTER_PREFIX: &str = "router:";

// ---------------------------------------------------------------------------
// The routers inventory (digitalocean.ts:25-27 — a STRICT decode)
// ---------------------------------------------------------------------------

/// `RoutersResponse` (digitalocean.ts:25-27): every `model_routers`
/// row must carry a non-empty string name — one bad row fails the
/// whole response (the ref's `schemaBodyJson` throws and the previous
/// inventory is retained, provider-digitalocean.test.ts:138-143).
pub fn parse_routers_response(body: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| format!("DigitalOcean routers response is not JSON: {error}"))?;
    let Some(rows) = value.get("model_routers").and_then(|v| v.as_array()) else {
        return Err("DigitalOcean routers response has no model_routers array".to_owned());
    };
    rows.iter()
        .map(|row| {
            row.get("name")
                .and_then(|name| name.as_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| "DigitalOcean routers response has an invalid router".to_owned())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The provider transform (digitalocean.ts:158-170)
// ---------------------------------------------------------------------------

/// One injected router model (digitalocean.ts:160-168): id
/// `router:{name}`, the router's name, the inference-router family,
/// text-only tool-capable, 128k/8k limits — created over the
/// provider's own package/settings (a model created under a provider
/// inherits them, exactly like the ref's `evt.models.update` on a
/// missing id; the provider's `api`-derived `baseURL` rides its
/// settings record).
#[must_use]
pub fn router_model_info(provider: &ProviderInfo, router: &str) -> schema_model::Info {
    schema_model::Info {
        id: ModelId::parse(format!("{DIGITALOCEAN_ROUTER_PREFIX}{router}")),
        model_id: ModelId::parse(format!("{DIGITALOCEAN_ROUTER_PREFIX}{router}")),
        provider_id: ProviderId::parse(provider.id.0.clone()),
        name: router.to_owned(),
        family: Some(DIGITALOCEAN_ROUTER_FAMILY.to_owned()),
        package: Some(provider.package.clone()),
        settings: provider.settings.as_ref().map(|settings| schema_model::Settings {
            compaction: settings.compaction,
            rest: settings.rest.clone(),
        }),
        capabilities: Capabilities {
            tools: true,
            input: vec!["text".to_owned()],
            output: vec!["text".to_owned()],
        },
        limit: ModelLimit { context: 128_000, input: None, output: 8_192 },
        ..schema_model::Info::default_for(
            ProviderId::parse(provider.id.0.clone()),
            ModelId::parse(format!("{DIGITALOCEAN_ROUTER_PREFIX}{router}")),
        )
    }
}

/// The provider transform (digitalocean.ts:158-170): each router
/// joins the provider's model list as `router:{name}` unless an entry
/// with that id already exists (a configured/catalog model keeps its
/// own shape). Provider info/environment stay the static record's.
#[must_use]
pub fn merge_digitalocean_snapshot(snapshot: &Snapshot, routers: &[String]) -> Snapshot {
    let mut models = snapshot.models.clone();
    for router in routers {
        let id = format!("{DIGITALOCEAN_ROUTER_PREFIX}{router}");
        if models.iter().any(|model| model.id.0 == id) {
            continue; // digitalocean.ts:162 — a configured model keeps its shape
        }
        models.push(router_model_info(&snapshot.info, router));
    }
    Snapshot { info: snapshot.info.clone(), models, environment: snapshot.environment.clone() }
}

// ---------------------------------------------------------------------------
// The sync (the plugin's `load` + transform, offline-testable)
// ---------------------------------------------------------------------------

/// One successful sync: the credential it listed for, the raw router
/// inventory, and the merged provider snapshot.
pub struct DigitalOceanSyncState {
    pub access: String,
    pub routers: Vec<String>,
    pub merged: Snapshot,
}

/// The synced `DigitalOcean` surface shared by the resolver (router
/// injection): refreshed by the engine against the resolved browser
/// OAuth credential and repeated every 5 minutes
/// (digitalocean.ts:171-178). A non-OAuth (or expired-token) state
/// keeps the last successful inventory for its credential through
/// transient failures; a missing credential clears.
pub struct DigitalOceanSync {
    transport: SharedTransport,
    state: parking_lot::RwLock<Option<Arc<DigitalOceanSyncState>>>,
}

impl DigitalOceanSync {
    #[must_use]
    pub fn new(transport: SharedTransport) -> Self {
        Self { transport, state: parking_lot::RwLock::new(None) }
    }

    /// The current sync state (shared handle, cloned out of the cell).
    #[must_use]
    pub fn state(&self) -> Option<Arc<DigitalOceanSyncState>> {
        self.state.read().clone()
    }

    /// No credential (or a non-OAuth one): the previous account's
    /// routers drop — the resolver serves the static catalog
    /// (digitalocean.ts:127-131).
    pub fn clear(&self) {
        *self.state.write() = None;
    }

    /// Production refresh (digitalocean.ts `load` + the provider
    /// transform): list the routers for the browser-OAuth credential.
    /// A different token starts from an empty inventory; an expired
    /// token (`expires != 0` in the past) skips the fetch but keeps
    /// the current state (digitalocean.ts:132-136); a failed fetch
    /// keeps the last success (digitalocean.ts:148-150).
    pub async fn refresh(
        &self,
        catalog: &Catalog,
        endpoint: &str,
        access: String,
        expires: i64,
        user_agent: &str,
    ) {
        // digitalocean.ts:132-135 — a token change empties the
        // inventory until its own listing succeeds.
        if self.state.read().as_ref().is_some_and(|state| state.access != access) {
            *self.state.write() = None;
        }
        // digitalocean.ts:136 — `expires === 0` never expires.
        if expires != 0 && expires <= now_millis() {
            return;
        }
        let Some(routers) = self.fetch_routers(endpoint, &access, user_agent).await else {
            return; // "failed to sync DigitalOcean routers" — keep the previous state
        };
        self.store(catalog, access, routers);
    }

    /// The routers fetch (digitalocean.ts:137-151): GET the endpoint
    /// with the bearer token, 10s timeout, strict decode.
    async fn fetch_routers(
        &self,
        endpoint: &str,
        access: &str,
        user_agent: &str,
    ) -> Option<Vec<String>> {
        let response = tokio::time::timeout(
            std::time::Duration::from_millis(DIGITALOCEAN_ROUTERS_TIMEOUT_MS),
            Arc::clone(&self.transport).execute(TransportRequest {
                method: "GET".to_owned(),
                url: endpoint.to_owned(),
                headers: vec![
                    ("Authorization".to_owned(), format!("Bearer {access}")),
                    ("Accept".to_owned(), "application/json".to_owned()),
                    ("User-Agent".to_owned(), user_agent.to_owned()),
                ],
                body: bytes::Bytes::default(),
            }),
        )
        .await
        .ok()?
        .ok()?;
        if !(200..300).contains(&response.status) {
            return None;
        }
        let body = read_body_limited(response.body, DIGITALOCEAN_ROUTERS_BODY_LIMIT).await.ok()?;
        parse_routers_response(&body).ok()
    }

    /// Rebuild + store the merged state over an already-loaded router
    /// inventory (the offline seam: tests seed state without network).
    pub fn store(&self, catalog: &Catalog, access: String, routers: Vec<String>) {
        let Some(static_snapshot) = catalog.provider(DIGITALOCEAN_PROVIDER_ID) else {
            return;
        };
        let merged = merge_digitalocean_snapshot(static_snapshot, &routers);
        *self.state.write() = Some(Arc::new(DigitalOceanSyncState { access, routers, merged }));
    }

    /// Test seam: seed the synced state directly (ADR-0006 — the
    /// production fetch path must not run during tests).
    pub fn seed_for_test(&self, catalog: &Catalog, access: &str, routers: Vec<String>) {
        self.store(catalog, access.to_owned(), routers);
    }
}

/// `Clock.currentTimeMillis` (process epoch millis).
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or_default())
        .unwrap_or_default()
}

/// Resolve the browser-OAuth credential from the integration host's
/// active digitalocean connection (digitalocean.ts:122-136 `load`):
/// only the `browser` oauth method drives discovery — a key/env
/// credential or no connection at all means `None` (the sync clears,
/// digitalocean.ts:127-131).
pub async fn resolve_digitalocean_credential(
    host: &crate::integration::IntegrationHost,
) -> Option<(String, i64)> {
    use crate::integration::types::{ConnectionInfo, CredentialValue};
    let connection = host.connection_active(DIGITALOCEAN_PROVIDER_ID).await.ok()??;
    let ConnectionInfo::Credential { id, .. } = connection else {
        return None; // env connections resolve to key credentials — not oauth
    };
    let stored = host.store().get(&id).await.ok()??;
    let CredentialValue::OAuth(oauth) = &stored.value else { return None };
    (oauth.method_id == DIGITALOCEAN_BROWSER_METHOD_ID)
        .then(|| (oauth.access.clone(), oauth.expires))
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

    /// G-259 — the strict routers decode (digitalocean.ts:25-27): the
    /// envelope and every row must decode; one bad row fails the whole
    /// response (the ref's schemaBodyJson, whose failure retains the
    /// previous inventory).
    #[test]
    fn routers_decode_is_strict() {
        assert_eq!(
            parse_routers_response(
                &json!({ "model_routers": [{ "name": "alpha" }, { "name": "beta" }] }).to_string()
            )
            .expect("decodes"),
            vec!["alpha".to_owned(), "beta".to_owned()]
        );
        assert_eq!(
            parse_routers_response(&json!({ "model_routers": [] }).to_string()).expect("decodes"),
            Vec::<String>::new()
        );
        assert!(parse_routers_response(&json!({ "model_routers": [{ "name": 123 }] }).to_string())
            .is_err());
        assert!(parse_routers_response(&json!({ "model_routers": [{ "other": 1 }] }).to_string())
            .is_err());
        assert!(parse_routers_response(&json!({ "model_routers": [{ "name": "" }] }).to_string())
            .is_err());
        assert!(parse_routers_response(&json!({}).to_string()).is_err());
        assert!(parse_routers_response("not json").is_err());
    }

    /// G-259 — the router injection (digitalocean.ts:158-170 over
    /// provider-digitalocean.test.ts:109-121): each router joins as
    /// `router:{name}` with the exact ref shape (name, family,
    /// capabilities, 128k/8k limits, the provider's package and
    /// baseURL settings), and an id already in the list keeps its own
    /// shape.
    #[test]
    fn routers_inject_over_the_static_catalog() {
        let static_snapshot =
            catalog().provider(DIGITALOCEAN_PROVIDER_ID).expect("digitalocean in catalog").clone();
        let mut with_configured = static_snapshot.clone();
        with_configured.models.push(schema_model::Info {
            name: "Configured router".to_owned(),
            limit: ModelLimit { context: 256_000, ..with_configured.models[0].limit },
            ..schema_model::Info::default_for(
                ProviderId::parse(DIGITALOCEAN_PROVIDER_ID),
                ModelId::parse("router:configured"),
            )
        });
        let merged = merge_digitalocean_snapshot(
            &with_configured,
            &["alpha".to_owned(), "configured".to_owned()],
        );

        let alpha = merged
            .models
            .iter()
            .find(|model| model.id.as_str() == "router:alpha")
            .expect("the new router joined");
        assert_eq!(alpha.name, "alpha");
        assert_eq!(alpha.family.as_deref(), Some(DIGITALOCEAN_ROUTER_FAMILY));
        assert!(alpha.capabilities.tools);
        assert_eq!(alpha.capabilities.input, vec!["text".to_owned()]);
        assert_eq!(alpha.capabilities.output, vec!["text".to_owned()]);
        assert_eq!(alpha.limit.context, 128_000);
        assert_eq!(alpha.limit.output, 8_192);
        // The provider record's package + baseURL carry (models created
        // under a provider inherit them).
        assert_eq!(alpha.package.as_deref(), Some(static_snapshot.info.package.as_str()));
        let settings = alpha.settings.as_ref().expect("settings");
        assert_eq!(
            settings.rest.get("baseURL"),
            Some(&json!("https://inference.do-ai.run/v1")),
            "the provider api-derived baseURL carries"
        );

        let configured = merged
            .models
            .iter()
            .find(|model| model.id.as_str() == "router:configured")
            .expect("the configured router stays");
        assert_eq!(configured.name, "Configured router");
        assert_eq!(configured.limit.context, 256_000, "its own shape wins (digitalocean.ts:162)");

        // An empty inventory changes nothing.
        let unchanged = merge_digitalocean_snapshot(&with_configured, &[]);
        assert_eq!(unchanged.models.len(), with_configured.models.len());
    }

    /// G-259 — offline inertness: an unseeded sync has no state (the
    /// resolver keeps the static catalog) and `clear` empties a loaded
    /// one (the non-OAuth credential path, digitalocean.ts:127-131).
    #[test]
    fn absent_credential_is_inert() {
        let sync = DigitalOceanSync::new(transport());
        assert!(sync.state().is_none(), "no credential → no state");
        sync.seed_for_test(&catalog(), "do-token", vec!["alpha".to_owned()]);
        let state = sync.state().expect("seeded");
        assert_eq!(state.access, "do-token");
        assert!(state.merged.models.iter().any(|m| m.id.as_str() == "router:alpha"));
        sync.clear();
        assert!(sync.state().is_none(), "clear → inert again");
    }
}
