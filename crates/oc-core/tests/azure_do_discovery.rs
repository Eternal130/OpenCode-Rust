//! G-259 — the azure deployment discovery + `DigitalOcean` routers syncs
//! over stub HTTP: the exact request shapes (endpoint URLs, query
//! params, auth headers, the Resource Graph POST body), the response
//! folding into the provider snapshots, the management-lane fallback
//! and `nextLink` origin guard, and the offline (no-credential)
//! inertness. Hermetic: every request hits a loopback stub on
//! `127.0.0.1:0`; no credential is ever stored.

use std::sync::Arc;

use oc_core::database::schema::SessionRow;
use oc_core::engine::resolver::CatalogModelResolver;
use oc_core::provider_catalog::azure_sync::{
    merge_azure_snapshot, parse_resource_deployments, AzureAuth, AzureDeployment, AzureEndpoints,
    AzureSync,
};
use oc_core::provider_catalog::catalog::Catalog;
use oc_core::provider_catalog::digitalocean_sync::{
    merge_digitalocean_snapshot, DigitalOceanSync, DIGITALOCEAN_ROUTERS_ENDPOINT,
};
use oc_core::provider_catalog::facade::FacadeContext;
use oc_core::session::runner::ModelResolver as _;
use serde_json::{json, Value};

fn transport() -> oc_ai::route::SharedTransport {
    Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new())
}

/// One recorded stub request: method, path+query, headers, body.
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

type Handler = Arc<dyn Fn(&str, &Recorded) -> (u16, String) + Send + Sync>;

/// A loopback HTTP stub: records every request and answers through the
/// per-path handler (path = pathname + query); the handler receives
/// the stub's own origin (for same-origin `nextLink`s).
async fn stub(handler: Handler) -> (String, Arc<std::sync::Mutex<Vec<Recorded>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("port").port();
    let origin = format!("http://127.0.0.1:{port}");
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let stub_origin = origin.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            let handler = Arc::clone(&handler);
            let recorded = Arc::clone(&recorded);
            let origin = stub_origin.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                let mut stream = stream;
                let mut buffer = vec![0u8; 0];
                loop {
                    // Read one request (headers + content-length body).
                    let mut chunk = [0u8; 8192];
                    let read = stream.read(&mut chunk).await.unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    let text = String::from_utf8_lossy(&buffer).into_owned();
                    let Some((head, rest)) = text.split_once("\r\n\r\n") else { continue };
                    let content_length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            (name.trim().eq_ignore_ascii_case("content-length"))
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    if rest.len() < content_length {
                        continue; // wait for the rest of the body
                    }
                    let mut lines = head.lines();
                    let request_line = lines.next().unwrap_or_default().to_owned();
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or_default().to_owned();
                    let path = parts.next().unwrap_or_default().to_owned();
                    let headers: Vec<(String, String)> = head
                        .lines()
                        .skip(1)
                        .filter_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            Some((name.trim().to_owned(), value.trim().to_owned()))
                        })
                        .collect();
                    let body = rest[..content_length].to_owned();
                    let request = Recorded { method, path, headers, body };
                    recorded.lock().unwrap().push(request.clone());
                    let (status, response_body) = handler(&origin, &request);
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let reply = format!(
                        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    if stream.write_all(reply.as_bytes()).await.is_err() {
                        break;
                    }
                    // Consume the handled bytes; keep any pipelined rest.
                    buffer = rest.as_bytes()[content_length..].to_vec();
                }
            });
        }
    });
    (origin, requests)
}

fn catalog() -> Arc<Catalog> {
    Arc::new(Catalog::embedded().clone())
}

/// G-259 — the API-key lane (azure.ts:236-260, 272 over
/// provider-azure.test.ts:261-304): `GET
/// {resource}/deployments?api-version=2022-12-01` with the `api-key`
/// header, and only the succeeded deployments fold into the narrowed
/// snapshot — deployed-name ids, catalog facts carried, undeployed
/// catalog models gone.
#[tokio::test]
async fn azure_api_key_lane_narrows_to_deployments() {
    let (origin, requests) = stub(Arc::new(|_origin: &str, request: &Recorded| {
        if request.path.starts_with("/openai/deployments") {
            let body = json!({ "data": [
                { "id": "gpt-5-mini", "model": "gpt-5-mini", "status": "succeeded" },
                { "id": "prod-mini", "model": "gpt-5-mini", "status": "succeeded" },
                { "id": "GPT-5-Nano", "model": "gpt-5-nano", "status": "succeeded" },
                { "id": "pending", "model": "gpt-5", "status": "creating" },
            ]})
            .to_string();
            (200, body)
        } else {
            (404, r#"{"error":"not found"}"#.to_owned())
        }
    }))
    .await;
    let endpoints = AzureEndpoints {
        resource_template: format!("{origin}/openai"),
        management: origin.clone(),
    };
    let sync = AzureSync::new(transport());
    sync.refresh(
        &catalog(),
        &endpoints,
        "test-resource".to_owned(),
        AzureAuth::ApiKey("secret".to_owned()),
        "opencode-test",
    )
    .await;

    let state = sync.state().expect("the discovery stored its state");
    assert_eq!(state.resource, "test-resource");
    let mut listed: Vec<&str> = state.merged.models.iter().map(|m| m.id.as_str()).collect();
    listed.sort_unstable();
    assert_eq!(listed, vec!["gpt-5-mini", "gpt-5-nano", "prod-mini"], "the narrowed surface");

    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "the key lane hits only the resource list");
    let request = &requests[0];
    assert_eq!(request.method, "GET");
    assert_eq!(
        request.path, "/openai/deployments?api-version=2022-12-01",
        "the data-plane deployment list (azure.ts:296)"
    );
    assert_eq!(request.header("api-key"), Some("secret"), "the key credential header");
    assert_eq!(request.header("accept"), Some("application/json"));
    assert_eq!(request.header("user-agent"), Some("opencode-test"));

    // The name disambiguation: a deployment named after another model
    // keeps the catalog model's facts under a `(deployment)` name.
    let prod =
        state.merged.models.iter().find(|m| m.id.as_str() == "prod-mini").expect("prod-mini");
    let embedded = catalog();
    let catalog_model = embedded
        .provider("azure")
        .and_then(|snapshot| snapshot.models.iter().find(|m| m.id.as_str() == "gpt-5-mini"))
        .expect("the catalog model");
    assert_eq!(prod.model_id.as_str(), "prod-mini");
    assert!(prod.name.starts_with(&catalog_model.name), "{} carries the model's name", prod.name);
    assert!(prod.name.ends_with("(prod-mini)"));
    assert_eq!(prod.limit.context, catalog_model.limit.context, "limits carry from the catalog");
}

/// G-259 — the Entra lane (azure.ts:188-234, 269-271 over
/// provider-azure.test.ts:306-325): Resource Graph resolves the ARM id
/// (POST with the management bearer + the KQL query body), the
/// deployments endpoint paginates through same-origin `nextLink`s, and
/// only the management plane is touched.
#[tokio::test]
async fn azure_entra_lane_paginates_the_management_inventory() {
    let resource_id = "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.CognitiveServices/accounts/test-resource";
    let (origin, requests) = stub(Arc::new(move |origin: &str, request: &Recorded| {
        if request.path.starts_with("/providers/Microsoft.ResourceGraph/") {
            return (200, json!({ "data": [{ "id": resource_id }] }).to_string());
        }
        if request.path.starts_with(&format!("{resource_id}/deployments")) {
            return if request.path.contains("page=2") {
                (
                    200,
                    json!({
                        "value": [
                            { "name": "nano", "properties": { "model": { "name": "gpt-5-nano" }, "provisioningState": "Succeeded" } },
                        ],
                    })
                    .to_string(),
                )
            } else {
                (
                    200,
                    json!({
                        "value": [
                            { "name": "mini", "properties": { "model": { "name": "gpt-5-mini" }, "provisioningState": "Succeeded" } },
                            { "name": "failed", "properties": { "model": { "name": "gpt-5" }, "provisioningState": "Failed" } },
                        ],
                        "nextLink": format!("{origin}{resource_id}/deployments?api-version=2024-10-01&page=2"),
                    })
                    .to_string(),
                )
            };
        }
        (404, r#"{"error":"not found"}"#.to_owned())
    }))
    .await;
    let endpoints =
        AzureEndpoints { resource_template: format!("{origin}/openai"), management: origin };
    let sync = AzureSync::new(transport());
    sync.refresh(
        &catalog(),
        &endpoints,
        "test-resource".to_owned(),
        AzureAuth::Entra {
            management: Some("mgmt-token".to_owned()),
            resource: "data-plane-token".to_owned(),
        },
        "opencode-test",
    )
    .await;

    let state = sync.state().expect("the discovery stored its state");
    let listed: Vec<&str> = state.merged.models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(listed, vec!["mini", "nano"], "the union of both pages, failed rows dropped");

    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 3, "graph + page 1 + page 2, never the resource list");
    let graph = &requests[0];
    assert_eq!(graph.method, "POST");
    assert_eq!(graph.path, "/providers/Microsoft.ResourceGraph/resources?api-version=2022-10-01");
    assert_eq!(graph.header("authorization"), Some("Bearer mgmt-token"));
    let body: Value = serde_json::from_str(&graph.body).expect("the query body is JSON");
    let query = body["query"].as_str().unwrap_or_default();
    assert!(query.contains("resources"), "{query}");
    assert!(query.contains("test-resource"), "the resource name reaches the KQL filter");
    let page1 = &requests[1];
    assert_eq!(
        page1.path,
        format!("{resource_id}/deployments?api-version=2024-10-01"),
        "the ARM deployments list (azure.ts:209)"
    );
    assert_eq!(page1.header("authorization"), Some("Bearer mgmt-token"));
    assert!(requests.iter().all(|r| !r.path.starts_with("/openai/deployments")));
}

/// G-259 — the Entra fallback (azure.ts:271 over
/// provider-azure.test.ts:327-343): when the management plane is
/// unreadable (Resource Graph finds nothing), the resource's own list
/// serves with the data-plane bearer token.
#[tokio::test]
async fn azure_entra_lane_falls_back_to_the_resource_list() {
    let (origin, requests) = stub(Arc::new(|_origin: &str, request: &Recorded| {
        if request.path.starts_with("/providers/Microsoft.ResourceGraph/") {
            (200, json!({ "data": [] }).to_string()) // no resource found → lane fails
        } else if request.path.starts_with("/openai/deployments") {
            (
                200,
                json!({ "data": [{ "id": "gpt-5-mini", "model": "gpt-5-mini", "status": "succeeded" }] })
                    .to_string(),
            )
        } else {
            (404, r#"{"error":"not found"}"#.to_owned())
        }
    }))
    .await;
    let endpoints =
        AzureEndpoints { resource_template: format!("{origin}/openai"), management: origin };
    let sync = AzureSync::new(transport());
    sync.refresh(
        &catalog(),
        &endpoints,
        "test-resource".to_owned(),
        AzureAuth::Entra {
            management: Some("mgmt-token".to_owned()),
            resource: "data-plane-token".to_owned(),
        },
        "opencode-test",
    )
    .await;
    let state = sync.state().expect("the fallback stored its state");
    assert_eq!(
        state.merged.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["gpt-5-mini"]
    );
    let requests = requests.lock().unwrap();
    let fallback = requests
        .iter()
        .find(|r| r.path.starts_with("/openai/deployments"))
        .expect("the fallback ran");
    assert_eq!(fallback.header("authorization"), Some("Bearer data-plane-token"));
}

/// G-259 — the `nextLink` origin guard (azure.ts:214-217 over
/// provider-azure.test.ts:414-433): a page link outside the management
/// endpoint fails the management lane (the token never follows it) and
/// the data-plane fallback serves instead.
#[tokio::test]
async fn azure_nextlink_outside_management_falls_back() {
    let resource_id = "/subscriptions/sub/providers/Microsoft.CognitiveServices/accounts/test";
    let (origin, requests) = stub(Arc::new(move |_origin: &str, request: &Recorded| {
        if request.path.starts_with("/providers/Microsoft.ResourceGraph/") {
            return (200, json!({ "data": [{ "id": resource_id }] }).to_string());
        }
        if request.path.starts_with(&format!("{resource_id}/deployments")) {
            return (
                200,
                json!({
                    "value": [
                        { "name": "mini", "properties": { "model": { "name": "gpt-5-mini" }, "provisioningState": "Succeeded" } },
                    ],
                    "nextLink": "https://attacker.example/page-2",
                })
                .to_string(),
            );
        }
        if request.path.starts_with("/openai/deployments") {
            return (
                200,
                json!({ "data": [{ "id": "from-resource", "model": "gpt-5-nano", "status": "succeeded" }] })
                    .to_string(),
            );
        }
        (404, r#"{"error":"not found"}"#.to_owned())
    }))
    .await;
    let endpoints =
        AzureEndpoints { resource_template: format!("{origin}/openai"), management: origin };
    let sync = AzureSync::new(transport());
    sync.refresh(
        &catalog(),
        &endpoints,
        "test".to_owned(),
        AzureAuth::Entra {
            management: Some("mgmt-token".to_owned()),
            resource: "data-plane-token".to_owned(),
        },
        "opencode-test",
    )
    .await;
    let state = sync.state().expect("the fallback stored its state");
    assert_eq!(
        state.merged.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        vec!["from-resource"],
        "the off-origin page failed the lane and the resource list served"
    );
    let requests = requests.lock().unwrap();
    assert!(
        requests.iter().all(|r| !r.path.contains("attacker.example")),
        "no request follows the off-origin link"
    );
}

/// G-259 — failure inertness (azure.ts:315-317): a failed discovery
/// keeps the previous state, and an invalid resource name refreshes
/// nothing at all (azure.ts:295).
#[tokio::test]
async fn azure_failures_keep_the_previous_state() {
    let sync = AzureSync::new(transport());
    sync.seed_for_test(
        &catalog(),
        "test-resource",
        vec![AzureDeployment { name: "gpt-5-mini".into(), model: "gpt-5-mini".into() }],
    );
    // A dead endpoint: the fetch fails, the state stays.
    let dead = AzureEndpoints {
        resource_template: "http://127.0.0.1:1/openai".to_owned(),
        management: "http://127.0.0.1:1".to_owned(),
    };
    sync.refresh(
        &catalog(),
        &dead,
        "test-resource".to_owned(),
        AzureAuth::ApiKey("secret".to_owned()),
        "opencode-test",
    )
    .await;
    let state = sync.state().expect("the previous state survives the failure");
    assert_eq!(state.merged.models.len(), 1);

    // An invalid resource name never leaves the process.
    sync.refresh(
        &catalog(),
        &dead,
        "under_score".to_owned(),
        AzureAuth::ApiKey("secret".to_owned()),
        "opencode-test",
    )
    .await;
    assert_eq!(sync.state().expect("still seeded").merged.models.len(), 1);
}

/// G-259 — the `DigitalOcean` routers refresh (digitalocean.ts:137-153
/// over provider-digitalocean.test.ts:96-153): GET the routers
/// endpoint with the bearer token, fold the routers into the merged
/// snapshot, keep the last success through a 503, and return to the
/// static surface on an empty inventory.
#[tokio::test]
async fn digitalocean_routers_refresh_folds_and_retains() {
    let status = Arc::new(std::sync::Mutex::new(200u16));
    let body = Arc::new(std::sync::Mutex::new(
        json!({ "model_routers": [{ "name": "alpha" }, { "name": "configured" }] }).to_string(),
    ));
    let (origin, requests) = stub({
        let status = Arc::clone(&status);
        let body = Arc::clone(&body);
        Arc::new(move |_origin: &str, request: &Recorded| {
            if request.path.starts_with("/v2/gen-ai/models/routers") {
                let code = *status.lock().unwrap();
                let payload = body.lock().unwrap().clone();
                if (200..300).contains(&code) {
                    return (code, payload);
                }
                return (code, r#"{"error":"unavailable"}"#.to_owned());
            }
            (404, r#"{"error":"not found"}"#.to_owned())
        })
    })
    .await;
    let endpoint = format!("{origin}/v2/gen-ai/models/routers");
    let sync = DigitalOceanSync::new(transport());
    let catalog = catalog();
    sync.refresh(&catalog, &endpoint, "do-token".to_owned(), 0, "opencode-test").await;

    let requests = requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, "/v2/gen-ai/models/routers");
    assert_eq!(requests[0].header("authorization"), Some("Bearer do-token"));
    assert_eq!(requests[0].header("accept"), Some("application/json"));
    assert_eq!(requests[0].header("user-agent"), Some("opencode-test"));

    let state = sync.state().expect("the routers stored their state");
    let alpha = state
        .merged
        .models
        .iter()
        .find(|model| model.id.as_str() == "router:alpha")
        .expect("router:alpha injected");
    assert_eq!(alpha.name, "alpha");
    assert_eq!(alpha.family.as_deref(), Some("digitalocean-inference-routers"));
    assert_eq!(alpha.limit.context, 128_000);
    assert_eq!(alpha.limit.output, 8_192);
    assert!(
        state.merged.models.iter().any(|model| model.id.as_str() != "router:alpha"),
        "the static catalog models stay"
    );

    // A 503 keeps the last successful inventory.
    *status.lock().unwrap() = 503;
    sync.refresh(&catalog, &endpoint, "do-token".to_owned(), 0, "opencode-test").await;
    assert!(
        sync.state()
            .expect("retained")
            .merged
            .models
            .iter()
            .any(|model| model.id.as_str() == "router:alpha"),
        "the last success survives a failed refresh"
    );

    // An empty inventory returns the provider to its static surface.
    *status.lock().unwrap() = 200;
    *body.lock().unwrap() = json!({ "model_routers": [] }).to_string();
    sync.refresh(&catalog, &endpoint, "do-token".to_owned(), 0, "opencode-test").await;
    assert!(
        !sync
            .state()
            .expect("state")
            .merged
            .models
            .iter()
            .any(|model| model.id.as_str() == "router:alpha"),
        "the router left with its inventory"
    );

    // A token change empties the inventory until its own listing
    // succeeds (digitalocean.ts:132-135).
    *status.lock().unwrap() = 503;
    sync.refresh(&catalog, &endpoint, "other-token".to_owned(), 0, "opencode-test").await;
    assert!(sync.state().is_none(), "a foreign token's stale inventory drops");
}

/// G-259 — an expired token stops discovery while retaining its last
/// successful inventory (digitalocean.ts:136, :195-207).
#[tokio::test]
async fn digitalocean_expired_token_keeps_its_inventory() {
    let (origin, requests) = stub(Arc::new(|_: &str, _: &Recorded| {
        (200, json!({ "model_routers": [{ "name": "alpha" }] }).to_string())
    }))
    .await;
    let endpoint = format!("{origin}/v2/gen-ai/models/routers");
    let sync = DigitalOceanSync::new(transport());
    let catalog = catalog();
    // Boot with a far-future expiry: the listing runs.
    sync.refresh(&catalog, &endpoint, "do-token".to_owned(), i64::MAX, "opencode-test").await;
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(sync.state().is_some());
    // The same token, now expired (`expires !== 0` in the past): no
    // fetch, the inventory stays.
    sync.refresh(&catalog, &endpoint, "do-token".to_owned(), 1, "opencode-test").await;
    assert_eq!(requests.lock().unwrap().len(), 1, "no second request");
    assert!(sync
        .state()
        .expect("retained")
        .merged
        .models
        .iter()
        .any(|model| model.id.as_str() == "router:alpha"));
}

/// G-259 — offline zero-behavior: without a credential neither sync
/// has state, and the merged-with-nothing snapshots equal the static
/// catalog records.
#[test]
fn offline_without_credentials_nothing_changes() {
    let catalog = catalog();
    let azure = AzureSync::new(transport());
    let digitalocean = DigitalOceanSync::new(transport());
    assert!(azure.state().is_none());
    assert!(digitalocean.state().is_none());

    let azure_static = catalog.provider("azure").expect("azure in the catalog");
    let narrowed = merge_azure_snapshot(azure_static, &[]);
    // Ref semantics (azure.ts:319 `if (!found) return` — `[]` is
    // truthy): a discovery that found ZERO succeeded deployments
    // legitimately empties the azure surface; the offline guard is the
    // sync having NO state at all (asserted above), which keeps the
    // resolver on the static record.
    assert_eq!(narrowed.models.len(), 0, "an empty deployment inventory empties the surface");

    let do_static = catalog.provider("digitalocean").expect("digitalocean in the catalog");
    let merged = merge_digitalocean_snapshot(do_static, &[]);
    assert_eq!(merged.models.len(), do_static.models.len(), "no routers → no injection");

    // The parse seams stay strict offline.
    assert!(parse_resource_deployments("offline noise").is_err());
    // The production endpoint constants stay the ref's.
    assert_eq!(
        DIGITALOCEAN_ROUTERS_ENDPOINT,
        "https://api.digitalocean.com/v2/gen-ai/models/routers"
    );
}

fn model_ref(provider: &str, model: &str) -> oc_schema::model::Ref {
    oc_schema::model::Ref {
        provider_id: oc_schema::id::ProviderId::parse(provider.to_owned()),
        id: oc_schema::id::ModelId::parse(model.to_owned()),
        variant: None,
    }
}

fn session_row() -> SessionRow {
    SessionRow::new("ses_azure_do", "global", "/tmp/opencode", "azure-do", "test")
        .expect("session row")
}

/// G-259 — production resolution serves the synced surfaces (the ref's
/// model/provider transforms, azure.ts:382-411 +
/// digitalocean.ts:158-170): the azure resolver binds a deployment
/// name and REFUSES an undeployed catalog model (the gap's "shows
/// everything, fails on selection" defect), and the digitalocean
/// resolver binds an injected `router:*` entry.
#[tokio::test]
async fn resolver_serves_the_synced_snapshots() {
    let catalog = Arc::new(Catalog::embedded().clone());
    let azure = AzureSync::new(transport());
    azure.seed_for_test(
        &catalog,
        "test-resource",
        vec![AzureDeployment { name: "prod-mini".to_owned(), model: "gpt-5-mini".to_owned() }],
    );
    let digitalocean = DigitalOceanSync::new(transport());
    digitalocean.seed_for_test(&catalog, "do-token", vec!["alpha".to_owned()]);

    // The azure lane needs the resource endpoint (facade.rs:146 —
    // `AtLeastOne<{resourceName, baseURL}>`).
    let connections = std::collections::BTreeMap::from([(
        "azure".to_owned(),
        oc_core::provider_catalog::facade::ModelConnection {
            base_url: Some("https://test-resource.openai.azure.com/openai".to_owned()),
            api_key: Some("secret".to_owned()),
            ..Default::default()
        },
    )]);

    // Without the syncs attached the static catalog serves both ids.
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport()),
        Arc::clone(&catalog),
        connections.clone(),
        Vec::new(),
        None,
    )
    .with_azure_sync(Arc::new(AzureSync::new(transport())))
    .with_digitalocean_sync(Arc::new(DigitalOceanSync::new(transport())));
    let session = session_row();
    resolver
        .resolve_selected(&session, &model_ref("azure", "gpt-5-mini"))
        .expect("the static azure catalog still binds");
    resolver
        .resolve_selected(&session, &model_ref("digitalocean", "deepseek-3.2"))
        .expect("the static digitalocean catalog still binds");

    // With the syncs attached the narrowed/injected surfaces serve.
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport()),
        catalog,
        connections,
        Vec::new(),
        None,
    )
    .with_azure_sync(Arc::new(azure))
    .with_digitalocean_sync(Arc::new(digitalocean));
    let session = session_row();
    let bound = resolver
        .resolve_selected(&session, &model_ref("azure", "prod-mini"))
        .expect("the deployment name resolves");
    assert_eq!(bound.model.id.as_str(), "prod-mini");
    let gone = resolver
        .resolve_selected(&session, &model_ref("azure", "gpt-5-mini"))
        .expect_err("an undeployed catalog model no longer resolves");
    assert!(matches!(gone, oc_core::session::runner::ModelError::Unavailable(_, _)));

    let router = resolver
        .resolve_selected(&session, &model_ref("digitalocean", "router:alpha"))
        .expect("the injected router resolves");
    assert_eq!(router.model.id.as_str(), "router:alpha");
}
