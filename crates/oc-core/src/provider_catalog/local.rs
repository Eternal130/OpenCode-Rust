//! RF-01 — runtime discovery of RUNNING local providers (port of
//! `packages/core/src/plugin/provider/{local,ollama,lmstudio,vllm}.ts`).
//!
//! The reference ships one local-provider plugin per server: `ollama`
//! (GET/POST `:11434/api/tags` + `/api/show`), `lmstudio`
//! (GET `:1234/api/v1/models`), and `vllm` (GET `:8000/health` +
//! `/v1/models`). Each refreshes on a 30 s spaced schedule, replaces
//! the provider registry entry's models wholesale (openai-compatible
//! package, cost-free, capability flags from the server's own
//! metadata), keeps the LAST successful inventory through transient
//! outages, and honors a `providers.<id>.settings.baseURL/apiKey`
//! override (the configured endpoint's known path suffix — `/v1`,
//! ollama's `/api` — is stripped before the discovery endpoints are
//! appended).
//!
//! The Rust port keeps the same observable shape at the provider
//! catalog seam: discovery produces [`CustomProvider`] entries bound
//! through the openai-compatible facade, held in a shared
//! [`LocalDiscovery`] snapshot the engine resolver consults FIRST for
//! the three ids (the reference transform removes the provider's
//! existing models before injecting, so a running server's inventory
//! is authoritative for its id). HTTP calls carry a 1 s per-request
//! timeout (ref `Effect.timeout("1 second")`); the refresh loop lives
//! in the server composition root (never on the boot path), so an
//! unreachable server is a silent no-op that keeps the last inventory.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::provider_catalog::facade::{
    CustomModel, CustomProvider, ModelConnection, PartialCapabilities, PartialModelLimit,
};

/// One local provider server spec (ref `createLocalProviderPlugin`
/// inputs: id/name/origin + the path suffix stripped from a configured
/// baseURL before discovery endpoints are appended).
pub struct LocalSpec {
    /// The models.dev provider id the inventory binds under.
    pub id: &'static str,
    /// Display name (`provider.name` in the ref transform).
    pub name: &'static str,
    /// Default origin when no config override exists.
    pub origin: &'static str,
    /// Path suffixes stripped from a configured baseURL's path
    /// (ollama `/(?:v1|api)`, lmstudio `/(?:api/)?v1`, vllm `/v1`).
    pub strip_suffixes: &'static [&'static str],
}

/// Ollama (ref plugin/provider/ollama.ts).
pub const OLLAMA: LocalSpec = LocalSpec {
    id: "ollama",
    name: "Ollama",
    origin: "http://127.0.0.1:11434",
    strip_suffixes: &["/v1", "/api"],
};

/// LM Studio (ref plugin/provider/lmstudio.ts).
pub const LMSTUDIO: LocalSpec = LocalSpec {
    id: "lmstudio",
    name: "LM Studio",
    origin: "http://127.0.0.1:1234",
    strip_suffixes: &["/api/v1", "/v1"],
};

/// vLLM (ref plugin/provider/vllm.ts).
pub const VLLM: LocalSpec = LocalSpec {
    id: "vllm",
    name: "vLLM",
    origin: "http://127.0.0.1:8000",
    strip_suffixes: &["/v1"],
};

/// Every local provider the port discovers (order = listing order).
pub const LOCAL_SPECS: &[&LocalSpec] = &[&OLLAMA, &LMSTUDIO, &VLLM];

/// Per-request timeout (ref local.ts:155-171 `Effect.timeout("1 second")`).
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// The discovery endpoints of one provider (ref `configured()`): the
/// effective baseURL (config override or `origin + /v1`) plus the
/// suffix-stripped root the discovery paths append onto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The full openai-compatible baseURL the provider binds with.
    pub base_url: String,
    /// `<scheme>://<authority><prefix>` — discovery paths append here.
    pub root: String,
}

/// Resolves the endpoint for one spec (ref local.ts:175-193): a config
/// `baseURL` wins over the default `origin + /v1`; trailing slashes
/// trim; a non-http(s) or unparseable baseURL disables discovery for
/// the provider (`None`, ref `if (!URL.canParse ...) return`).
#[must_use]
pub fn resolve_endpoint(spec: &LocalSpec, base_url_override: Option<&str>) -> Option<Endpoint> {
    let raw = base_url_override
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}/v1", spec.origin));
    let base_url = raw.trim_end_matches('/').to_owned();
    let Some((scheme, rest)) = base_url.split_once("://") else {
        return None;
    };
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, String::new()),
    };
    let trimmed = path.trim_end_matches('/');
    let prefix = spec
        .strip_suffixes
        .iter()
        .find_map(|suffix| trimmed.strip_suffix(suffix))
        .unwrap_or(trimmed);
    Some(Endpoint { root: format!("{scheme}://{authority}{prefix}"), base_url })
}

/// The shared runtime-discovery snapshot: the engine resolver and the
/// model listing surfaces read it; the server's refresh loop (30 s
/// spaced, ref `Schedule.spaced(interval)`) keeps it fresh. A failed
/// refresh keeps the last successful inventory (ref local.ts:115 — no
/// flickering model availability through transient outages).
pub struct LocalDiscovery {
    client: reqwest::Client,
    entries: parking_lot::RwLock<BTreeMap<String, CustomProvider>>,
}

impl Default for LocalDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalDiscovery {
    /// An empty snapshot over a shared HTTP client.
    #[must_use]
    pub fn new() -> Self {
        let client = reqwest::Client::builder().build().unwrap_or_else(|_| reqwest::Client::new());
        Self { client, entries: parking_lot::RwLock::new(BTreeMap::new()) }
    }

    /// The discovered provider for an id, if a running server answered.
    #[must_use]
    pub fn provider(&self, id: &str) -> Option<CustomProvider> {
        self.entries.read().get(id).cloned()
    }

    /// Every discovered provider (id order, deterministic listing).
    #[must_use]
    pub fn providers(&self) -> Vec<CustomProvider> {
        self.entries.read().values().cloned().collect()
    }

    /// One refresh pass over every local spec: resolve the endpoint
    /// against the config `connections` overlay (baseURL/apiKey),
    /// discover, and swap the snapshot entry when the inventory
    /// changed. Returns whether anything changed (the caller publishes
    /// `model.updated`). Unreachable/unparseable servers keep the last
    /// entry FOR THE SAME ENDPOINT (ref local.ts:115); a changed
    /// connection (baseURL/apiKey repoint) clears the entry before
    /// discovery (ref local.ts:126-127 — clear-on-repoint); a server
    /// answering an EMPTY inventory removes it (the ref transform only
    /// applies with models to inject, and a zero-model reload resets
    /// `loaded.models`).
    pub async fn refresh(&self, connections: &BTreeMap<String, ModelConnection>) -> bool {
        let mut changed = false;
        for spec in LOCAL_SPECS {
            let connection = connections.get(spec.id);
            let Some(endpoint) =
                resolve_endpoint(spec, connection.and_then(|c| c.base_url.as_deref()))
            else {
                continue;
            };
            let api_key = connection.and_then(|c| c.api_key.clone());
            {
                // Gate-2 F1 — clear-on-repoint (ref local.ts:117-131
                // `reload`: when the configured endpoint or credential
                // changes, `loaded.models = []` resets BEFORE any
                // discovery, so stale models bound to the OLD URL stop
                // being listable/usable immediately — even if the new
                // endpoint never answers. The retained inventory is
                // only for the SAME endpoint through transient outages.
                let mut entries = self.entries.write();
                if let Some(existing) = entries.get(spec.id) {
                    if existing.base_url != endpoint.base_url || existing.api_key != api_key {
                        entries.remove(spec.id);
                        changed = true;
                    }
                }
            }
            match discover(&self.client, spec, &endpoint, api_key.as_deref()).await {
                Ok(models) => {
                    let stamp = fingerprint(&endpoint.base_url, api_key.as_deref(), &models);
                    let mut entries = self.entries.write();
                    if models.is_empty() {
                        changed |= entries.remove(spec.id).is_some();
                    } else {
                        let provider = CustomProvider {
                            id: spec.id.to_owned(),
                            name: spec.name.to_owned(),
                            base_url: endpoint.base_url.clone(),
                            api_key: api_key.clone(),
                            models,
                            ..CustomProvider::default()
                        };
                        let existing = entries.get(spec.id).and_then(|p| {
                            (fingerprint(&p.base_url, p.api_key.as_deref(), &p.models) == stamp)
                                .then(|| p.id.clone())
                        });
                        if existing.is_none() {
                            entries.insert(spec.id.to_owned(), provider);
                            changed = true;
                        }
                    }
                }
                // Unreachable/unhealthy/malformed: keep the last
                // successful inventory (ref local.ts:115).
                Err(_) => {}
            }
        }
        changed
    }
}

/// Change-detection fingerprint: endpoint + credential + the ordered
/// model inventory (ids, names, limits, capabilities) — the ref
/// compares the JSON hash of the discovered list (local.ts:107-109).
fn fingerprint(base_url: &str, api_key: Option<&str>, models: &[CustomModel]) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    base_url.hash(&mut hash);
    api_key.hash(&mut hash);
    for model in models {
        model.id.hash(&mut hash);
        model.name.hash(&mut hash);
        model.family.hash(&mut hash);
        model
            .capabilities
            .as_ref()
            .map(|caps| {
                format!(
                    "{:?}|{:?}|{:?}",
                    caps.tools,
                    caps.input.as_ref().map(|list| list.join(",")),
                    caps.output.as_ref().map(|list| list.join(","))
                )
            })
            .unwrap_or_default()
            .hash(&mut hash);
        model.limit.map(|limit| (limit.context, limit.input, limit.output)).hash(&mut hash);
    }
    hash.finish()
}

/// Dispatches one spec's discovery protocol.
async fn discover(
    client: &reqwest::Client,
    spec: &LocalSpec,
    endpoint: &Endpoint,
    api_key: Option<&str>,
) -> Result<Vec<CustomModel>, String> {
    match spec.id {
        "ollama" => discover_ollama(client, endpoint, api_key).await,
        "lmstudio" => discover_lmstudio(client, endpoint, api_key).await,
        "vllm" => discover_vllm(client, endpoint, api_key).await,
        _ => Ok(Vec::new()),
    }
}

async fn get_json(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    api_key: Option<&str>,
    path: &str,
) -> Result<Value, String> {
    let url = format!("{}{}", endpoint.root, path);
    let mut request = client.get(&url).timeout(REQUEST_TIMEOUT);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("GET {url} answered {}", response.status()));
    }
    response.json::<Value>().await.map_err(|error| error.to_string())
}

async fn post_json(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    api_key: Option<&str>,
    path: &str,
    body: &Value,
) -> Result<Value, String> {
    let url = format!("{}{}", endpoint.root, path);
    let mut request = client.post(&url).json(body).timeout(REQUEST_TIMEOUT);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("POST {url} answered {}", response.status()));
    }
    response.json::<Value>().await.map_err(|error| error.to_string())
}

/// Builds the listing/binding model entry (ref local.ts:63-77
/// `providers.models.update`): id, display name, family, capability
/// flags, and the context limit — cost stays free (the transform never
/// sets one; the custom lane lists `cost: []`).
fn local_model(
    id: String,
    name: Option<String>,
    family: Option<String>,
    tools: bool,
    vision: bool,
    context: Option<i64>,
) -> CustomModel {
    let input =
        if vision { vec!["text".to_owned(), "image".to_owned()] } else { vec!["text".to_owned()] };
    CustomModel {
        id,
        name,
        family,
        capabilities: Some(PartialCapabilities {
            tools: Some(tools),
            input: Some(input),
            output: Some(vec!["text".to_owned()]),
        }),
        limit: Some(PartialModelLimit {
            context: context.filter(|value| *value > 0),
            ..PartialModelLimit::default()
        }),
        ..CustomModel::default()
    }
}

/// Ollama (ref ollama.ts): `GET /api/tags`, then `POST /api/show` per
/// model (cached by digest in the ref; the port re-asks each refresh —
/// bounded by the 1 s timeout and one refresh every 30 s). Only models
/// whose show response advertises the `completion` capability list;
/// the FIRST `*.context_length` model_info entry sets the context
/// limit; tools/vision come from the capability list.
async fn discover_ollama(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    api_key: Option<&str>,
) -> Result<Vec<CustomModel>, String> {
    let tags = get_json(client, endpoint, api_key, "/api/tags").await?;
    let Some(models) = tags.get("models").and_then(Value::as_array) else {
        return Err("ollama /api/tags: missing models array".to_owned());
    };
    // Sorted by model id, empty ids dropped (ref toSorted + filter).
    let mut summaries: Vec<&Value> = models
        .iter()
        .filter(|model| model.get("model").and_then(Value::as_str).is_some_and(|id| !id.is_empty()))
        .collect();
    summaries.sort_by_key(|model| model.get("model").and_then(Value::as_str).unwrap_or_default());
    let mut out = Vec::new();
    for summary in summaries {
        let id = summary.get("model").and_then(Value::as_str).unwrap_or_default().to_owned();
        // A failing show drops just this model (ref orElseSucceed(undefined)).
        let Ok(show) =
            post_json(client, endpoint, api_key, "/api/show", &serde_json::json!({ "model": id }))
                .await
        else {
            continue;
        };
        let capabilities = show.get("capabilities").and_then(Value::as_array);
        let completion = capabilities
            .is_some_and(|list| list.iter().any(|item| item.as_str() == Some("completion")));
        if !completion {
            continue;
        }
        let tools =
            capabilities.is_some_and(|list| list.iter().any(|item| item.as_str() == Some("tools")));
        let vision = capabilities
            .is_some_and(|list| list.iter().any(|item| item.as_str() == Some("vision")));
        let family = show
            .get("details")
            .and_then(|details| details.get("family"))
            .or_else(|| summary.get("details").and_then(|details| details.get("family")))
            .and_then(Value::as_str)
            .filter(|family| !family.is_empty())
            .map(str::to_owned);
        let context = show.get("model_info").and_then(Value::as_object).and_then(|info| {
            info.iter()
                .find(|(key, value)| {
                    key.ends_with(".context_length")
                        && value.as_i64().is_some_and(|value| value > 0)
                })
                .and_then(|(_, value)| value.as_i64())
        });
        let name = summary
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        out.push(local_model(id, name, family, tools, vision, context));
    }
    Ok(out)
}

/// LM Studio (ref lmstudio.ts): `GET /api/v1/models`, type `llm` only;
/// the context limit is the MINIMUM loaded instance's context (else
/// `max_context_length`); tool/vision flags from the capabilities
/// record; family from the architecture.
async fn discover_lmstudio(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    api_key: Option<&str>,
) -> Result<Vec<CustomModel>, String> {
    let response = get_json(client, endpoint, api_key, "/api/v1/models").await?;
    let Some(models) = response.get("models").and_then(Value::as_array) else {
        return Err("lmstudio /api/v1/models: missing models array".to_owned());
    };
    let mut llms: Vec<&Value> = models
        .iter()
        .filter(|model| {
            model.get("type").and_then(Value::as_str) == Some("llm")
                && model.get("key").and_then(Value::as_str).is_some_and(|key| !key.is_empty())
        })
        .collect();
    llms.sort_by_key(|model| model.get("key").and_then(Value::as_str).unwrap_or_default());
    let mut out = Vec::new();
    for entry in llms {
        let id = entry.get("key").and_then(Value::as_str).unwrap_or_default().to_owned();
        let name = entry
            .get("display_name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        let family = entry
            .get("architecture")
            .and_then(Value::as_str)
            .filter(|family| !family.is_empty())
            .map(str::to_owned);
        let capabilities = entry.get("capabilities");
        let tools = capabilities
            .and_then(|caps| caps.get("trained_for_tool_use"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let vision = capabilities
            .and_then(|caps| caps.get("vision"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let loaded = entry.get("loaded_instances").and_then(Value::as_array);
        let context = match loaded {
            Some(instances) if !instances.is_empty() => instances
                .iter()
                .filter_map(|instance| instance.get("config")?.get("context_length")?.as_i64())
                .min(),
            _ => entry.get("max_context_length").and_then(Value::as_i64),
        };
        out.push(local_model(id, name, family, tools, vision, context));
    }
    Ok(out)
}

/// vLLM (ref vllm.ts): `GET /health` (any 2xx) then `GET /v1/models`,
/// `owned_by == "vllm"` only; `max_model_len` sets the context limit;
/// no name/family/tools (tool calling depends on server flags
/// discovery cannot report).
async fn discover_vllm(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    api_key: Option<&str>,
) -> Result<Vec<CustomModel>, String> {
    let url = format!("{}/health", endpoint.root);
    let mut request = client.get(&url).timeout(REQUEST_TIMEOUT);
    if let Some(key) = api_key {
        request = request.bearer_auth(key);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("GET {url} answered {}", response.status()));
    }
    let response = get_json(client, endpoint, api_key, "/v1/models").await?;
    let Some(models) = response.get("data").and_then(Value::as_array) else {
        return Err("vllm /v1/models: missing data array".to_owned());
    };
    let mut owned: Vec<&Value> = models
        .iter()
        .filter(|model| {
            model.get("owned_by").and_then(Value::as_str) == Some("vllm")
                && model.get("id").and_then(Value::as_str).is_some_and(|id| !id.is_empty())
        })
        .collect();
    owned.sort_by_key(|model| model.get("id").and_then(Value::as_str).unwrap_or_default());
    Ok(owned
        .iter()
        .map(|entry| {
            local_model(
                entry.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                None,
                None,
                false,
                false,
                entry.get("max_model_len").and_then(Value::as_i64),
            )
        })
        .collect())
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Shared loopback HTTP mock (hand-rolled HTTP over `TcpListener`,
    //! ADR-0006 loopback-only, no real network) for the local-discovery
    //! tests in this module and the resolver end-to-end test.
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    pub struct HttpMock {
        pub port: u16,
        pub hits: std::sync::Arc<std::sync::Mutex<Vec<(String, String, Option<String>)>>>,
    }

    pub async fn http_mock(
        routes: HashMap<(&'static str, &'static str), (u16, String)>,
    ) -> HttpMock {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let port = listener.local_addr().expect("addr").port();
        let hits = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let hit_log = std::sync::Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let routes = routes.clone();
                let hits = std::sync::Arc::clone(&hit_log);
                tokio::spawn(async move {
                    let mut buffer = [0_u8; 8192];
                    let mut raw = Vec::new();
                    let head_end = loop {
                        let read = socket.read(&mut buffer).await.unwrap_or(0);
                        if read == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buffer[..read]);
                        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            break at + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
                    let mut lines = head.lines();
                    let request_line = lines.next().unwrap_or_default().to_owned();
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or_default().to_owned();
                    let path = parts.next().unwrap_or_default().to_owned();
                    let mut auth = None;
                    for line in lines {
                        if let Some((name, value)) = line.split_once(':') {
                            if name.eq_ignore_ascii_case("authorization") {
                                auth = Some(value.trim().to_owned());
                            }
                        }
                    }
                    let content_length: usize = head
                        .lines()
                        .find_map(|line| {
                            let value = line.strip_prefix("Content-Length:")?;
                            value.trim().parse().ok()
                        })
                        .unwrap_or(0);
                    while raw.len() < head_end + content_length {
                        let read = socket.read(&mut buffer).await.unwrap_or(0);
                        if read == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buffer[..read]);
                    }
                    hits.lock().expect("hits").push((method.clone(), path.clone(), auth));
                    let not_found: &(u16, String) = &(404, "{}".to_owned());
                    let (status, body) =
                        routes.get(&(method.as_str(), path.as_str())).unwrap_or(not_found);
                    let reason = if *status == 200 { "OK" } else { "Not Found" };
                    let response = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        HttpMock { port, hits }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::http_mock;
    use super::*;
    use std::collections::HashMap;

    fn connection(port: u16, path: &str, key: Option<&str>) -> BTreeMap<String, ModelConnection> {
        let mut map = BTreeMap::new();
        map.insert(
            "ollama".to_owned(),
            ModelConnection {
                base_url: Some(format!("http://127.0.0.1:{port}{path}")),
                api_key: key.map(str::to_owned),
                headers: BTreeMap::new(),
            },
        );
        map
    }

    #[test]
    fn endpoint_resolution_mirrors_the_reference() {
        // Default: origin + /v1, discovery root strips the suffix.
        let endpoint = resolve_endpoint(&OLLAMA, None).expect("default endpoint");
        assert_eq!(endpoint.base_url, "http://127.0.0.1:11434/v1");
        assert_eq!(endpoint.root, "http://127.0.0.1:11434");
        // Config override with /v1 suffix: suffix stripped for discovery,
        // full URL kept for binding.
        let endpoint =
            resolve_endpoint(&OLLAMA, Some("http://box:9000/v1/")).expect("override endpoint");
        assert_eq!(endpoint.base_url, "http://box:9000/v1");
        assert_eq!(endpoint.root, "http://box:9000");
        // Ollama's /api suffix strips too; lmstudio's /api/v1; unknown
        // paths stay.
        assert_eq!(
            resolve_endpoint(&OLLAMA, Some("http://box:11434/api")).expect("api").root,
            "http://box:11434"
        );
        assert_eq!(
            resolve_endpoint(&LMSTUDIO, Some("http://box:1234/api/v1")).expect("api v1").root,
            "http://box:1234"
        );
        assert_eq!(
            resolve_endpoint(&VLLM, Some("http://box:8000/custom")).expect("custom").root,
            "http://box:8000/custom"
        );
        // Non-http(s) or unparseable baseURLs disable discovery.
        assert!(resolve_endpoint(&VLLM, Some("ftp://box")).is_none());
        assert!(resolve_endpoint(&VLLM, Some("not a url")).is_none());
    }

    #[tokio::test]
    async fn ollama_tags_and_show_map_into_models() {
        let mut routes = HashMap::new();
        routes.insert(
            ("GET", "/api/tags"),
            (
                200,
                serde_json::json!({
                    "models": [
                        { "name": "Vision 12b", "model": "llava", "digest": "d2", "details": { "family": "llama" } },
                        { "name": "", "model": "llama3", "digest": "d1", "details": { "family": "llama" } },
                        { "name": "skip", "model": "", "digest": "d0", "details": {} }
                    ]
                })
                .to_string(),
            ),
        );
        routes.insert(
            ("POST", "/api/show"),
            (
                200,
                serde_json::json!({
                    "capabilities": ["completion", "tools"],
                    "details": { "family": "llava" },
                    "model_info": { "llava.context_length": 8192, "general.size": 7 }
                })
                .to_string(),
            ),
        );
        let mock = http_mock(routes).await;
        let discovery = LocalDiscovery::new();
        let connections = connection(mock.port, "/v1", Some("k-1"));
        assert!(discovery.refresh(&connections).await, "first refresh inserts");
        assert!(!discovery.refresh(&connections).await, "unchanged inventory is a no-op");
        let provider = discovery.provider("ollama").expect("ollama injected");
        assert_eq!(provider.base_url, format!("http://127.0.0.1:{}/v1", mock.port));
        assert_eq!(provider.models.len(), 2, "empty ids drop");
        // Sorted by model id; names fall back to the id; capabilities
        // and the context limit map through.
        assert_eq!(provider.models[0].id, "llama3");
        assert_eq!(provider.models[0].name, None);
        assert_eq!(provider.models[1].id, "llava");
        assert_eq!(provider.models[1].name.as_deref(), Some("Vision 12b"));
        let caps = provider.models[1].capabilities.clone().expect("capabilities");
        assert_eq!(caps.tools, Some(true));
        assert_eq!(caps.input, Some(vec!["text".to_owned()]));
        assert_eq!(caps.output, Some(vec!["text".to_owned()]));
        assert_eq!(provider.models[1].limit.expect("limit").context, Some(8192));
        // The Authorization header rode the apiKey override.
        let hits = mock.hits.lock().expect("hits").clone();
        assert!(hits.iter().any(|(method, path, auth)| method == "GET"
            && path == "/api/tags"
            && auth.as_deref().is_some_and(|value| value.starts_with("Bearer k-1"))));
    }

    #[tokio::test]
    async fn ollama_filters_non_completion_models() {
        let mut routes = HashMap::new();
        routes.insert(
            ("GET", "/api/tags"),
            (
                200,
                serde_json::json!({ "models": [
                    { "name": "n", "model": "embed", "digest": "d", "details": {} }
                ] })
                .to_string(),
            ),
        );
        routes.insert(
            ("POST", "/api/show"),
            (200, serde_json::json!({ "capabilities": ["embedding"] }).to_string()),
        );
        let mock = http_mock(routes).await;
        let discovery = LocalDiscovery::new();
        discovery.refresh(&connection(mock.port, "/v1", None)).await;
        // A server answering zero completion models removes its entry
        // (the transform only applies with models to inject).
        assert!(discovery.provider("ollama").is_none());
    }

    #[tokio::test]
    async fn lmstudio_lists_llm_models_with_min_loaded_context() {
        let mut routes = HashMap::new();
        routes.insert(
            ("GET", "/api/v1/models"),
            (
                200,
                serde_json::json!({ "models": [
                    { "type": "embedding", "key": "e", "display_name": "E",
                      "loaded_instances": [], "max_context_length": 10 },
                    { "type": "llm", "key": "qwen3", "display_name": "Qwen3 Coder",
                      "architecture": "qwen", "max_context_length": 131072,
                      "loaded_instances": [
                          { "config": { "context_length": 131072 } },
                          { "config": { "context_length": 8192 } }
                      ],
                      "capabilities": { "vision": false, "trained_for_tool_use": true } },
                    { "type": "llm", "key": "gpt-oss", "display_name": "GPT OSS",
                      "architecture": null, "max_context_length": 131072,
                      "loaded_instances": [] }
                ] })
                .to_string(),
            ),
        );
        let mock = http_mock(routes).await;
        let discovery = LocalDiscovery::new();
        let mut connections = connection(mock.port, "/api/v1", None);
        connections.insert(
            "lmstudio".to_owned(),
            ModelConnection {
                base_url: Some(format!("http://127.0.0.1:{}/api/v1", mock.port)),
                api_key: None,
                headers: BTreeMap::new(),
            },
        );
        discovery.refresh(&connections).await;
        let provider = discovery.provider("lmstudio").expect("lmstudio injected");
        assert_eq!(provider.models.len(), 2, "embedding type drops");
        assert_eq!(provider.models[0].id, "gpt-oss");
        assert_eq!(provider.models[0].limit.expect("limit").context, Some(131072));
        assert_eq!(provider.models[0].family, None, "null architecture stays unset");
        assert_eq!(provider.models[0].capabilities.as_ref().expect("caps").tools, Some(false));
        assert_eq!(provider.models[1].id, "qwen3");
        // The MINIMUM loaded instance context wins.
        assert_eq!(provider.models[1].limit.expect("limit").context, Some(8192));
        assert_eq!(provider.models[1].family.as_deref(), Some("qwen"));
        let caps = provider.models[1].capabilities.clone().expect("caps");
        assert_eq!(caps.tools, Some(true));
        assert_eq!(caps.input, Some(vec!["text".to_owned()]));
    }

    #[tokio::test]
    async fn vllm_health_gates_the_model_list() {
        let mut routes = HashMap::new();
        routes.insert(("GET", "/health"), (200, "{}".to_owned()));
        routes.insert(
            ("GET", "/v1/models"),
            (
                200,
                serde_json::json!({ "data": [
                    { "id": "meta/llama-3", "owned_by": "vllm", "max_model_len": 65536 },
                    { "id": "other", "owned_by": "openai", "max_model_len": 1 }
                ] })
                .to_string(),
            ),
        );
        let mock = http_mock(routes).await;
        let discovery = LocalDiscovery::new();
        let mut connections = BTreeMap::new();
        connections.insert(
            "vllm".to_owned(),
            ModelConnection {
                base_url: Some(format!("http://127.0.0.1:{}/v1", mock.port)),
                api_key: None,
                headers: BTreeMap::new(),
            },
        );
        discovery.refresh(&connections).await;
        let provider = discovery.provider("vllm").expect("vllm injected");
        assert_eq!(provider.models.len(), 1, "owned_by filter");
        assert_eq!(provider.models[0].id, "meta/llama-3");
        assert_eq!(provider.models[0].limit.expect("limit").context, Some(65536));
        assert_eq!(provider.models[0].capabilities.as_ref().expect("caps").tools, Some(false));
    }

    #[tokio::test]
    async fn unreachable_server_is_a_silent_no_op_that_keeps_the_last_inventory() {
        // A port that was released: nothing listens.
        let gone = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let discovery = LocalDiscovery::new();
        let mut connections = connection(gone, "/v1", None);
        connections.insert(
            "vllm".to_owned(),
            ModelConnection {
                base_url: Some(format!("http://127.0.0.1:{gone}/v1")),
                api_key: None,
                headers: BTreeMap::new(),
            },
        );
        assert!(!discovery.refresh(&connections).await, "no change on outage");
        assert!(discovery.providers().is_empty());
        // A previously discovered inventory survives the outage.
        discovery.entries.write().insert(
            "ollama".to_owned(),
            CustomProvider {
                id: "ollama".to_owned(),
                name: "Ollama".to_owned(),
                base_url: format!("http://127.0.0.1:{gone}/v1"),
                models: vec![local_model("llama3".to_owned(), None, None, false, false, None)],
                ..CustomProvider::default()
            },
        );
        assert!(!discovery.refresh(&connections).await);
        assert!(discovery.provider("ollama").is_some(), "last inventory kept");
    }

    /// Gate-2 F1 — clear-on-repoint: when the config connection for a
    /// local spec changes (baseURL repoint), the retained inventory is
    /// dropped IMMEDIATELY (ref local.ts:126-127 resets `loaded.models`
    /// on endpoint change), so a dead new endpoint leaves the provider
    /// EMPTY — the stale models bound to the OLD URL never stay
    /// listable/usable until the new endpoint answers.
    #[tokio::test]
    async fn config_base_url_repoint_clears_the_inventory_immediately() {
        // A live ollama answering one model.
        let mut routes = HashMap::new();
        routes.insert(
            ("GET", "/api/tags"),
            (
                200,
                serde_json::json!({ "models": [
                    { "name": "Llama 3", "model": "llama3", "digest": "d1",
                      "details": { "family": "llama" } }
                ] })
                .to_string(),
            ),
        );
        routes.insert(
            ("POST", "/api/show"),
            (
                200,
                serde_json::json!({
                    "capabilities": ["completion"],
                    "details": { "family": "llama" },
                    "model_info": { "llama3.context_length": 8192 }
                })
                .to_string(),
            ),
        );
        let mock = http_mock(routes).await;
        let discovery = LocalDiscovery::new();
        discovery.refresh(&connection(mock.port, "/v1", None)).await;
        let provider = discovery.provider("ollama").expect("inventory populated");
        assert_eq!(provider.models.len(), 1);
        assert_eq!(provider.base_url, format!("http://127.0.0.1:{}/v1", mock.port));

        // The config repoints at a dead endpoint (a port that was
        // released): the refresh clears the stale inventory, and the
        // unreachable new target does NOT resurrect it.
        let gone = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let repointed = connection(gone, "/v1", None);
        assert!(
            discovery.refresh(&repointed).await,
            "the repoint itself is a change (inventory cleared)"
        );
        assert!(
            discovery.provider("ollama").is_none(),
            "no stale models bound to the OLD base URL stay listable/usable"
        );
        assert!(discovery.providers().is_empty());
        // Same-endpoint refreshes still retain through outages (the
        // previous test) — the clear only fires on an endpoint change.
    }
}
