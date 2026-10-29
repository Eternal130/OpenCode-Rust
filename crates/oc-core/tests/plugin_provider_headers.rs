//! AUD-10②: the four ref static header-injection provider plugins
//! (cerebras/kilo/nvidia/llmgateway) reach the actual wire request when
//! the resolved package + `settings.baseURL` match, and inject nothing
//! when they do not. Header values are byte-exact vs
//! `plugin/provider/{cerebras,kilo,nvidia,llmgateway}.ts` (ref v2.0.24).
//!
//! Hermetic: asserted at the route's header-building seam
//! (`Route::prepare_transport`), never sent.

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_core::engine::resolver::CatalogModelResolver;
use oc_core::integration::IntegrationHost;
use oc_core::provider_catalog::catalog::Catalog;
use oc_core::provider_catalog::facade::{FacadeContext, ModelConnection};
use oc_core::session::runner::ModelResolver as _;
use oc_core::session::runner::ResolvedModel;

/// An integration host over a sandboxed database (no real data dirs).
async fn host() -> (Arc<IntegrationHost>, tempfile::TempDir) {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox tempdir");
    let db = Arc::new(
        oc_core::database::Database::open_file(dir.path().join("t.db")).await.expect("db"),
    );
    let bus = Arc::new(oc_core::bus::Bus::new(
        (*db).clone(),
        oc_core::bus::BusOptions { persist: false, ..oc_core::bus::BusOptions::deterministic() },
    ));
    let host = IntegrationHost::new(db, bus, Catalog::embedded(), "opencode-test");
    (host, dir)
}

fn header(resolved: &ResolvedModel, name: &str) -> Option<String> {
    let route = Arc::clone(&resolved.model.route);
    let mut request = oc_ai::schema::LLMRequest::new(resolved.model.clone());
    request.messages = vec![oc_ai::schema::Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("transport");
    transport_request.header(name).map(str::to_owned)
}

fn assert_header(resolved: &ResolvedModel, name: &str, expected: Option<&str>) {
    assert_eq!(
        header(resolved, name).as_deref(),
        expected,
        "header {name} (got {:?}, want {expected:?})",
        header(resolved, name)
    );
}

async fn resolve(
    integrations: Option<Arc<IntegrationHost>>,
    connections: BTreeMap<String, ModelConnection>,
    provider: &str,
    model: &str,
) -> ResolvedModel {
    let transport: oc_ai::route::SharedTransport =
        Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new());
    let mut context = FacadeContext::new(transport);
    if let Some(host) = integrations {
        context = context.with_integrations(host);
    }
    let resolver = CatalogModelResolver::new(
        context,
        Arc::new(Catalog::embedded().clone()),
        connections,
        Vec::new(),
        None,
    );
    let session = oc_core::database::schema::SessionRow::new(
        "ses_plugin_headers",
        "global",
        "/tmp/opencode",
        "plugin-headers",
        "test",
    )
    .expect("session row");
    resolver
        .resolve_selected(
            &session,
            &oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse(provider.to_owned()),
                id: oc_schema::id::ModelId::parse(model.to_owned()),
                variant: None,
            },
        )
        .unwrap_or_else(|error| panic!("{provider}/{model} resolves: {error:?}"))
}

fn connection(base_url: Option<&str>) -> BTreeMap<String, ModelConnection> {
    BTreeMap::from([(
        "kilo".into(),
        ModelConnection {
            base_url: base_url.map(str::to_string),
            api_key: Some("test-key".into()),
            headers: BTreeMap::new(),
        },
    )])
}

#[tokio::test]
async fn kilo_gateway_baseurl_gets_referer_and_title() {
    // kilo.ts:10-12 — the catalog api is https://api.kilo.ai/api/gateway.
    let resolved = resolve(None, connection(None), "kilo", "sao10k/l3-lunaris-8b").await;
    assert_header(&resolved, "HTTP-Referer", Some("https://opencode.ai/"));
    assert_header(&resolved, "X-Title", Some("opencode"));
    assert_header(&resolved, "authorization", Some("Bearer test-key"));
}

#[tokio::test]
async fn kilo_non_matching_baseurl_injects_nothing() {
    let resolved = resolve(
        None,
        connection(Some("https://api.kilo.ai/api/gateway/v1")),
        "kilo",
        "sao10k/l3-lunaris-8b",
    )
    .await;
    assert_header(&resolved, "HTTP-Referer", None);
    assert_header(&resolved, "X-Title", None);
}

#[tokio::test]
async fn nvidia_gets_billing_origin_default() {
    // nvidia.ts:10-17 — catalog api https://integrate.api.nvidia.com/v1,
    // no user header → default "OpenCode".
    let mut connections = connection(None);
    connections.insert(
        "nvidia".into(),
        ModelConnection {
            base_url: None,
            api_key: Some("nv-key".into()),
            headers: BTreeMap::new(),
        },
    );
    let resolved = resolve(None, connections, "nvidia", "baai/bge-m3").await;
    assert_header(&resolved, "HTTP-Referer", Some("https://opencode.ai/"));
    assert_header(&resolved, "X-Title", Some("opencode"));
    assert_header(&resolved, "X-BILLING-INVOKE-ORIGIN", Some("OpenCode"));
}

#[tokio::test]
async fn llmgateway_requires_registered_integration() {
    // llmgateway.ts:12-22 — the header set only lands when an integration
    // is registered under the provider id. A bare context (no integration
    // host) must not inject; the catalog-loaded host registers it (like
    // the ref plugin's Integration registration), unlocking the set.
    let mut connections = connection(None);
    connections.insert(
        "llmgateway".into(),
        ModelConnection {
            base_url: None,
            api_key: Some("lgw-key".into()),
            headers: BTreeMap::new(),
        },
    );
    let resolved =
        resolve(None, connections.clone(), "llmgateway", "grok-4-1-fast-reasoning").await;
    assert_eq!(header(&resolved, "X-Source"), None, "unregistered integration must not inject");

    let (host, _dir) = host().await;
    assert!(
        host.has_integration("llmgateway"),
        "catalog registers the llmgateway integration (ref plugin parity)"
    );
    let resolved = resolve(Some(host), connections, "llmgateway", "grok-4-1-fast-reasoning").await;
    assert_header(&resolved, "HTTP-Referer", Some("https://opencode.ai/"));
    assert_header(&resolved, "X-Title", Some("opencode"));
    assert_header(&resolved, "X-Source", Some("opencode"));
}

#[tokio::test]
async fn cerebras_package_gets_third_party_integration_header() {
    // cerebras.ts:9-12 — package match only, no baseURL gate.
    let mut connections = connection(None);
    connections.insert(
        "cerebras".into(),
        ModelConnection {
            // The ref cerebras package bakes its API base; the plugin has
            // no baseURL gate, any base exercises the package match.
            base_url: Some("https://api.cerebras.ai/v1".into()),
            api_key: Some("csk-key".into()),
            headers: BTreeMap::new(),
        },
    );
    let resolved = resolve(None, connections, "cerebras", "qwen-3.8-27b").await;
    assert_header(&resolved, "X-Cerebras-3rd-Party-Integration", Some("opencode"));
    // and the other plugin sets stay absent
    assert_header(&resolved, "HTTP-Referer", None);
    assert_header(&resolved, "X-Title", None);
}

#[tokio::test]
async fn unmatched_providers_get_no_plugin_headers() {
    // groq: a baked-base openai-family provider — none of the four
    // plugins' gates can match.
    let mut connections = connection(None);
    connections.insert(
        "groq".into(),
        ModelConnection {
            base_url: None,
            api_key: Some("gsk-key".into()),
            headers: BTreeMap::new(),
        },
    );
    let (host, _dir) = host().await;
    host.update_integration("groq", "Groq");
    let resolved = resolve(Some(host), connections, "groq", "llama-3.3-70b-versatile").await;
    assert_header(&resolved, "HTTP-Referer", None);
    assert_header(&resolved, "X-Title", None);
    assert_header(&resolved, "X-Source", None);
    assert_header(&resolved, "X-Cerebras-3rd-Party-Integration", None);
}
