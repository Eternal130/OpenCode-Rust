//! Well-known config-graph resolution (BE-3b item 3, config.ts:130-170,
//! 292-311; wellknown.ts:70-95): resolves persisted well-known manifests
//! into config documents (`manifest.config` + `remote_config` with `{{env:NAME}}`
//! substitution using the integration's key credential) and feeds them to
//! the config Loader through its `WellknownSource` seam — the documents
//! are the LOWEST-priority layer, exactly v2's load order.

use crate::config::{WellknownDocument, WellknownSource};
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

/// The kv key the server's wellknown.add persists (wellknown.ts:97).
pub const WELLKNOWN_SOURCES_KEY: &str = "wellknown:sources";

/// `{origin}/.well-known/opencode` manifest (wellknown.ts:31-45).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub auth: Option<ManifestAuth>,
    #[serde(default)]
    pub config: Option<Value>,
    #[serde(default, rename = "remote_config")]
    pub remote_config: Option<RemoteConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestAuth {
    #[serde(default)]
    pub env: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteConfig {
    pub url: String,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, String>>,
}

/// The resolved document snapshot: origin → documents. Refreshed by the
/// host (credential events + a 10-minute interval, config.ts:303-317);
/// read synchronously by the Loader.
#[derive(Default)]
pub struct WellknownDocuments {
    entries: DashMap<String, Vec<WellknownDocument>>,
}

impl WellknownDocuments {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn swap(&self, next: BTreeMap<String, Vec<WellknownDocument>>) {
        self.entries.clear();
        for (origin, documents) in next {
            self.entries.insert(origin, documents);
        }
    }

    /// Test seam: install documents directly.
    pub fn push_for_test(&self, documents: Vec<WellknownDocument>) {
        for document in documents {
            self.entries.insert(document.source.clone(), vec![document]);
        }
    }
}

/// The Loader-facing source over the host's snapshot.
pub struct HostWellknownSource {
    documents: Arc<WellknownDocuments>,
}

impl HostWellknownSource {
    #[must_use]
    pub fn new(documents: Arc<WellknownDocuments>) -> Self {
        Self { documents }
    }
}

impl WellknownSource for HostWellknownSource {
    fn documents(&self, _env: &dyn Fn(&str) -> Option<String>) -> Vec<WellknownDocument> {
        self.documents.entries.iter().flat_map(|entry| entry.value().clone()).collect()
    }
}

/// `{env:NAME}` substitution against the credential variables then the
/// process environment (wellknown.ts:74-79).
fn substitute(value: &str, variables: &BTreeMap<String, String>) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("{env:") {
        let (head, tail) = rest.split_at(start);
        output.push_str(head);
        let after = &tail[5..];
        if let Some(end) = after.find('}') {
            let name = &after[..end];
            let resolved = variables
                .get(name)
                .cloned()
                .or_else(|| std::env::var(name).ok())
                .unwrap_or_default();
            output.push_str(&resolved);
            rest = &after[end + 1..];
        } else {
            output.push_str(tail);
            return output;
        }
    }
    output.push_str(rest);
    output
}

async fn fetch_manifest(client: &reqwest::Client, origin: &str) -> Result<Manifest, String> {
    let response = client
        .get(format!("{origin}/.well-known/opencode"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("manifest request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("manifest returned {}", response.status()));
    }
    response.json().await.map_err(|error| format!("manifest decode failed: {error}"))
}

impl crate::integration::IntegrationHost {
    /// The shared snapshot the Loader reads.
    #[must_use]
    pub fn wellknown_documents(&self) -> &Arc<WellknownDocuments> {
        &self.wellknown_documents_shared
    }

    /// One resolution pass over the persisted sources
    /// (config.ts:145-170 `loadWellknownEntry`): sources without auth or
    /// without a stored KEY credential contribute nothing; the newest
    /// credential supplies the `{auth.env}` variable.
    pub async fn refresh_wellknown_documents(self: &Arc<Self>) {
        let db = self.database();
        let Ok(sources) = db.transaction(|tx| tx.kv_get(WELLKNOWN_SOURCES_KEY)).await else {
            return;
        };
        let Some(sources) = sources else { return };
        let origins: Vec<String> = sources
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default();
        let client = reqwest::Client::new();
        let mut next = BTreeMap::new();
        for origin in origins {
            let origin = origin.trim_end_matches('/').to_owned();
            let Ok(manifest) = fetch_manifest(&client, &origin).await else {
                continue;
            };
            let Some(auth) = manifest.auth else { continue };
            // v2 takes the LAST credential of the asc-ordered list
            // (the newest) and only KEY credentials (config.ts:149-151).
            let Ok(stored) = self.store().list(&origin).await else { continue };
            let Some(newest) = stored.last() else { continue };
            let crate::integration::types::CredentialValue::Key(credential) = &newest.value else {
                continue;
            };
            let mut variables = BTreeMap::new();
            if let Some(env) = &auth.env {
                variables.insert(env.clone(), credential.key.clone());
            }
            let mut documents = Vec::new();
            let push = |documents: &mut Vec<WellknownDocument>, config: &Value| {
                documents.push(WellknownDocument {
                    source: origin.clone(),
                    dir: PathBuf::from(&origin),
                    text: config.to_string(),
                    env: variables.clone(),
                });
            };
            if let Some(config) = manifest.config.as_ref().filter(|config| config.is_object()) {
                push(&mut documents, config);
            }
            if let Some(remote) = &manifest.remote_config {
                let url = substitute(&remote.url, &variables);
                let mut request = client.get(&url).header("Accept", "application/json");
                for (key, value) in remote.headers.iter().flatten() {
                    request = request.header(key, substitute(value, &variables));
                }
                if let Ok(response) = request.send().await {
                    if response.status().is_success() {
                        if let Ok(remote_config) = response.json::<Value>().await {
                            // A Config record (provider id → json) or the
                            // whole body — both are valid documents
                            // (wellknown.ts:92-95).
                            if remote_config.is_object() {
                                push(&mut documents, &remote_config);
                            }
                        }
                    }
                }
            }
            if !documents.is_empty() {
                next.insert(origin, documents);
            }
        }
        self.wellknown_documents_shared.swap(next);
    }
}
