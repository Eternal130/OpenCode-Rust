//! AUD-10② + G-261①/G-285③/G-245: the ref static header-injection
//! provider plugins (cerebras/kilo/nvidia/llmgateway/openrouter/zenmux)
//! reach the actual wire request when the resolved package +
//! `settings.baseURL` match, and inject nothing when they do not. Header
//! values are byte-exact vs `plugin/provider/{cerebras,kilo,nvidia,
//! llmgateway,openrouter,zenmux}.ts` (ref v2.0.24). The G-245 tests pin
//! the websocket transport defaults (`plugin/provider/{openai,xai,azure}
//! .ts`).
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

fn resolve(
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
    keyed_connection("kilo", base_url, None)
}

fn keyed_connection(
    provider: &str,
    base_url: Option<&str>,
    transport: Option<oc_schema::provider::Transport>,
) -> BTreeMap<String, ModelConnection> {
    BTreeMap::from([(
        provider.into(),
        ModelConnection {
            base_url: base_url.map(str::to_string),
            api_key: Some("test-key".into()),
            transport,
            ..ModelConnection::default()
        },
    )])
}

/// A connection spread over several providers (header tests).
fn multi_connection(entries: &[(&str, ModelConnection)]) -> BTreeMap<String, ModelConnection> {
    entries.iter().map(|(id, connection)| (id.to_string(), connection.clone())).collect()
}

#[tokio::test]
async fn kilo_gateway_baseurl_gets_referer_and_title() {
    // kilo.ts:10-12 — the catalog api is https://api.kilo.ai/api/gateway.
    let resolved = resolve(None, connection(None), "kilo", "sao10k/l3-lunaris-8b");
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
    );
    assert_header(&resolved, "HTTP-Referer", None);
    assert_header(&resolved, "X-Title", None);
}

#[tokio::test]
async fn nvidia_gets_billing_origin_default() {
    // nvidia.ts:10-17 — catalog api https://integrate.api.nvidia.com/v1,
    // no user header → default "OpenCode".
    let connections = multi_connection(&[(
        "nvidia",
        ModelConnection {
            base_url: None,
            api_key: Some("nv-key".into()),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let resolved = resolve(None, connections, "nvidia", "baai/bge-m3");
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
    let connections = multi_connection(&[(
        "llmgateway",
        ModelConnection {
            base_url: None,
            api_key: Some("lgw-key".into()),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let resolved = resolve(None, connections.clone(), "llmgateway", "grok-4-1-fast-reasoning");
    assert_eq!(header(&resolved, "X-Source"), None, "unregistered integration must not inject");

    let (host, _dir) = host().await;
    assert!(
        host.has_integration("llmgateway"),
        "catalog registers the llmgateway integration (ref plugin parity)"
    );
    let resolved = resolve(Some(host), connections, "llmgateway", "grok-4-1-fast-reasoning");
    assert_header(&resolved, "HTTP-Referer", Some("https://opencode.ai/"));
    assert_header(&resolved, "X-Title", Some("opencode"));
    assert_header(&resolved, "X-Source", Some("opencode"));
}

#[tokio::test]
async fn cerebras_package_gets_third_party_integration_header() {
    // cerebras.ts:9-12 — package match only, no baseURL gate.
    let connections = multi_connection(&[(
        "cerebras",
        ModelConnection {
            // The ref cerebras package bakes its API base; the plugin has
            // no baseURL gate, any base exercises the package match.
            base_url: Some("https://api.cerebras.ai/v1".into()),
            api_key: Some("csk-key".into()),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let resolved = resolve(None, connections, "cerebras", "qwen-3.8-27b");
    assert_header(&resolved, "X-Cerebras-3rd-Party-Integration", Some("opencode"));
    // and the other plugin sets stay absent
    assert_header(&resolved, "HTTP-Referer", None);
    assert_header(&resolved, "X-Title", None);
}

#[tokio::test]
async fn unmatched_providers_get_no_plugin_headers() {
    // groq: a baked-base openai-family provider — none of the plugin
    // gates can match.
    let connections = multi_connection(&[(
        "groq",
        ModelConnection {
            base_url: None,
            api_key: Some("gsk-key".into()),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let (host, _dir) = host().await;
    host.update_integration("groq", "Groq");
    let resolved = resolve(Some(host), connections, "groq", "llama-3.3-70b-versatile");
    assert_header(&resolved, "HTTP-Referer", None);
    assert_header(&resolved, "X-Title", None);
    assert_header(&resolved, "X-Source", None);
    assert_header(&resolved, "X-Cerebras-3rd-Party-Integration", None);
}

/// G-261① — openrouter.ts:9-14: every provider bound to the openrouter
/// package gets the referer/title pair (package gate only).
#[tokio::test]
async fn openrouter_package_gets_referer_and_title() {
    let connections = keyed_connection("openrouter", None, None);
    let resolved = resolve(None, connections, "openrouter", "anthropic/claude-sonnet-4.5");
    assert_header(&resolved, "HTTP-Referer", Some("https://opencode.ai/"));
    assert_header(&resolved, "X-Title", Some("opencode"));
}

/// G-285③ — zenmux.ts:10-17: the catalog api is exactly
/// `https://zenmux.ai/api/v1`, so the pair injects — but user headers
/// win (the ref spreads the injected pair FIRST).
#[tokio::test]
async fn zenmux_catalog_baseurl_gets_pair_under_user_headers() {
    // No user HTTP-Referer: the injected default rides the wire.
    let connections = keyed_connection("zenmux", None, None);
    let resolved = resolve(None, connections, "zenmux", "anthropic/claude-sonnet-4.5");
    assert_header(&resolved, "HTTP-Referer", Some("https://opencode.ai/"));
    assert_header(&resolved, "X-Title", Some("opencode"));

    // A user-set HTTP-Referer survives the zenmux spread (the opposite of
    // the other plugins' injected-wins order).
    let connections = multi_connection(&[(
        "zenmux",
        ModelConnection {
            base_url: None,
            api_key: Some("zenmux-key".into()),
            headers: BTreeMap::from([(
                "HTTP-Referer".to_string(),
                "https://mine.example/".to_string(),
            )]),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let resolved = resolve(None, connections, "zenmux", "anthropic/claude-sonnet-4.5");
    assert_header(&resolved, "HTTP-Referer", Some("https://mine.example/"));
    assert_header(&resolved, "X-Title", Some("opencode"));
}

/// G-245 — the websocket transport defaults: openai/xai default to
/// `websocket` (openai.ts:262, xai.ts:102), azure only while the
/// responses channel is usable (azure.ts:358-365); the user's explicit
/// `settings.transport` wins; every other provider stays HTTP.
#[tokio::test]
async fn websocket_transport_defaults() {
    use oc_schema::provider::Transport;

    // openai — default websocket.
    let resolved = resolve(None, keyed_connection("openai", None, None), "openai", "gpt-5.5");
    assert_eq!(resolved.transport, Some(Transport::Websocket), "openai defaults to websocket");
    // xai — default websocket.
    let resolved = resolve(None, keyed_connection("xai", None, None), "xai", "grok-4.5");
    assert_eq!(resolved.transport, Some(Transport::Websocket), "xai defaults to websocket");
    // azure — resource name (Azure-native endpoint) → websocket.
    let connections = multi_connection(&[(
        "azure",
        ModelConnection {
            base_url: None,
            api_key: Some("az-key".into()),
            resource_name: Some("my-resource".into()),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let resolved = resolve(None, connections, "azure", "gpt-5.5");
    assert_eq!(
        resolved.transport,
        Some(Transport::Websocket),
        "azure defaults to websocket while Azure-native"
    );
    // azure over a non-Azure gateway baseURL → stays HTTP (capable=false).
    let connections = multi_connection(&[(
        "azure",
        ModelConnection {
            base_url: Some("https://gateway.example.com/v1".into()),
            api_key: Some("az-key".into()),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let resolved = resolve(None, connections, "azure", "gpt-5.5");
    assert_eq!(resolved.transport, None, "gateway azure stays on HTTP");
    // azure over an Azure-native custom baseURL → websocket.
    let connections = multi_connection(&[(
        "azure",
        ModelConnection {
            base_url: Some("https://my-resource.openai.azure.com/openai".into()),
            api_key: Some("az-key".into()),
            transport: None,
            ..ModelConnection::default()
        },
    )]);
    let resolved = resolve(None, connections, "azure", "gpt-5.5");
    assert_eq!(resolved.transport, Some(Transport::Websocket));

    // Explicit user transport wins over the default (both directions).
    for provider in ["openai", "xai", "azure"] {
        let model = match provider {
            "xai" => "grok-4.5",
            _ => "gpt-5.5",
        };
        let connections = multi_connection(&[(
            provider,
            ModelConnection {
                base_url: None,
                api_key: Some("k".into()),
                resource_name: (provider == "azure").then(|| "my-resource".to_string()),
                transport: Some(Transport::Http),
                ..ModelConnection::default()
            },
        )]);
        let resolved = resolve(None, connections, provider, model);
        assert_eq!(
            resolved.transport,
            Some(Transport::Http),
            "{provider}: explicit settings.transport wins"
        );
    }

    // Non-defaulted providers stay on HTTP.
    let resolved =
        resolve(None, keyed_connection("groq", None, None), "groq", "llama-3.3-70b-versatile");
    assert_eq!(resolved.transport, None, "groq keeps HTTP");
}
