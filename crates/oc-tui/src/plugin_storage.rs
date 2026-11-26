//! G-475 — the TUI's plugin-host `StoragePort` binding: a LOCAL-FILE
//! port over [`crate::storage::ChannelStorage`] (the byte-faithful
//! port of v2's `useStorage` — `<state>/<channel>/tui/<key>.json`,
//! atomic writes, the `<channel>/locks` flock). The R17 prespec's
//! ruling: ref TUI storage is guest-local file semantics, NOT the
//! server's `DbStoragePort` — the TUI embeds the plugin host, so the
//! `storage.*` RPCs (the raw kv face + the `ctx.storage.store`
//! snapshot/mutate arm) land in the same files a v2 TUI would write.
//!
//! ## Key layout
//!
//! The ref's v2 TUI plugin storage namespaces through the api layer
//! (`plugin.${id}.${key}` — api.tsx:200-202, storage.tsx:57); the
//! port namespaces in the PORT (the (`plugin_id`, `key`) pair composes
//! the file key), so both the kv face and the store arm land at
//! `plugin.<id>.<key>.json` — byte-identical to the ref for
//! ref-shaped plugin ids. Port-derived ids with foreign characters
//! (`tui:omo-slim` — the `:` is not a valid segment char) sanitize
//! to `-` (the ref would throw `TypeError: Invalid storage segment`
//! — the port contains instead, a documented bounded deviation:
//! port-only ids never collide with ref-shaped ones).
//!
//! ## Scan
//!
//! `storage.scan` pages the plugin's keys (`plugin.<id>.<…>.json`
//! stripped to bare keys), sorted, `after` exclusive + `limit`
//! optional — the v2 `{entries: [{key, value}], next?}` shape.

use std::path::Path;

use oc_plugin_host::supervisor::{StoragePort, StorageScanQuery};
use serde_json::{json, Value};

use crate::storage::ChannelStorage;

/// Sanitize one plugin id into a storage-segment-safe shard (see the
/// module docs: ref-shaped ids pass through unchanged).
#[must_use]
fn safe_plugin_id(plugin_id: &str) -> String {
    let mut out = String::with_capacity(plugin_id.len());
    for (index, ch) in plugin_id.chars().enumerate() {
        if index == 0 {
            out.push(if ch.is_ascii_alphanumeric() { ch } else { 'p' });
        } else {
            out.push(if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '-'
            });
        }
    }
    if out.is_empty() {
        "plugin".to_owned()
    } else {
        out
    }
}

/// The local-file plugin storage port.
#[derive(Debug, Clone)]
pub struct PluginFileStorage {
    storage: ChannelStorage,
}

impl PluginFileStorage {
    /// Open at the production root (`<state>/<channel>/tui`, the same
    /// root the TUI's own state uses). The storage directory is
    /// created on demand; a read-only state root degrades to an
    /// error surface the guest sees as failed kv calls.
    pub fn open_default() -> Result<Self, String> {
        let root = oc_util::global_roots::resolve().state;
        let channel = crate::storage::channel();
        Ok(Self { storage: ChannelStorage::open(&root, &channel)? })
    }

    /// Open at an explicit state root (tests).
    #[must_use]
    #[allow(clippy::missing_panics_doc)] // the test helper's expect mirrors open_default
    pub fn open_at(state_root: &Path, channel: &str) -> Self {
        Self { storage: ChannelStorage::open(state_root, channel).expect("plugin storage dir") }
    }

    /// The (`plugin_id`, `key`) → file-key composition.
    fn file_key(plugin_id: &str, key: &str) -> Result<String, String> {
        let namespaced = format!("plugin.{}.{}", safe_plugin_id(plugin_id), key);
        if !ChannelStorage::valid_segment(&namespaced) {
            return Err(format!("Invalid storage segment: {key}"));
        }
        Ok(namespaced)
    }
}

#[async_trait::async_trait]
impl StoragePort for PluginFileStorage {
    async fn get(&self, plugin_id: &str, key: &str) -> Result<Option<Value>, String> {
        let file_key = Self::file_key(plugin_id, key)?;
        Ok(self.storage.read(&file_key))
    }

    async fn set(&self, plugin_id: &str, key: &str, value: Value) -> Result<(), String> {
        let file_key = Self::file_key(plugin_id, key)?;
        self.storage.write(&file_key, &value)
    }

    async fn remove(&self, plugin_id: &str, key: &str) -> Result<(), String> {
        let file_key = Self::file_key(plugin_id, key)?;
        self.storage.remove(&file_key)
    }

    async fn scan(&self, plugin_id: &str, query: StorageScanQuery) -> Result<Value, String> {
        if query.prefix.is_empty() && query.after.is_none() && query.limit.is_none() {
            // A full scan still validates the namespace composes.
            let _ = Self::file_key(plugin_id, "scan")?;
        } else if !ChannelStorage::valid_segment(&format!(
            "plugin.{}.{}",
            safe_plugin_id(plugin_id),
            query.prefix
        )) {
            return Err(format!("Invalid storage segment: {}", query.prefix));
        }
        let namespace = format!("plugin.{}.", safe_plugin_id(plugin_id));
        let mut entries = Vec::new();
        let mut next = Value::Null;
        for key in self.storage.keys() {
            let Some(bare) = key.strip_prefix(&namespace) else { continue };
            if !bare.starts_with(&query.prefix) {
                continue;
            }
            if let Some(after) = &query.after {
                if bare <= after.as_str() {
                    continue;
                }
            }
            entries.push(bare.to_owned());
        }
        // The page window: `limit` bounds the entries; a remainder
        // leaves a `next` cursor (the last emitted key).
        if let Some(limit) = query.limit {
            let limit = usize::try_from(limit).unwrap_or(usize::MAX);
            if entries.len() > limit {
                entries.truncate(limit);
                next = Value::String(entries.last().cloned().unwrap_or_default());
            }
        }
        let rows: Vec<Value> = entries
            .iter()
            .map(|bare| {
                json!({
                    "key": bare,
                    "value": self.storage.read(&format!("{namespace}{bare}")),
                })
            })
            .collect();
        Ok(json!({ "entries": rows, "next": next }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("oc-plugin-storage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp root");
        dir
    }

    /// The file layout matches the ref: `plugin.<id>.<key>.json`
    /// under `<state>/<channel>/tui`, with the kv face and the
    /// namespacing composing ref-identical paths.
    #[tokio::test]
    async fn plugin_keys_land_at_the_ref_layout() {
        let root = temp_root("layout");
        let port = PluginFileStorage::open_at(&root, "stable");
        port.set("opencode.notifications", "dismissed", json!({"seen": 1})).await.expect("set");
        let path =
            root.join("stable").join("tui").join("plugin.opencode.notifications.dismissed.json");
        assert!(path.is_file(), "the ref-shaped namespace: {}", path.display());
        let value = port.get("opencode.notifications", "dismissed").await.unwrap();
        assert_eq!(value, Some(json!({"seen": 1})));
        // remove is idempotent.
        port.remove("opencode.notifications", "dismissed").await.unwrap();
        port.remove("opencode.notifications", "dismissed").await.unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Port-derived ids (the `tui:`/`v1:` prefixes) sanitize instead
    /// of colliding with ref-shaped namespaces; invalid keys error
    /// like the ref's `segment()` `TypeError`.
    #[tokio::test]
    async fn foreign_ids_sanitize_and_invalid_keys_error() {
        let root = temp_root("sanitize");
        let port = PluginFileStorage::open_at(&root, "stable");
        port.set("tui:omo-slim", "kv", json!(true)).await.unwrap();
        let path = root.join("stable").join("tui").join("plugin.tui-omo-slim.kv.json");
        assert!(path.is_file(), "sanitized id: {}", path.display());
        assert_eq!(port.get("tui:omo-slim", "kv").await.unwrap(), Some(json!(true)));
        assert!(port.get("tui:omo-slim", "bad key").await.is_err(), "space is invalid");
        assert!(port.get("tui:omo-slim", "../escape").await.is_err(), "traversal is invalid");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The scan page: prefix filter, exclusive `after`, `limit` + the
    /// `next` cursor, keys stripped to bare names.
    #[tokio::test]
    async fn scan_pages_the_plugin_namespace() {
        let root = temp_root("scan");
        let port = PluginFileStorage::open_at(&root, "stable");
        port.set("p", "alpha", json!(1)).await.unwrap();
        port.set("p", "beta", json!(2)).await.unwrap();
        port.set("p", "gamma", json!(3)).await.unwrap();
        port.set("other", "alpha", json!(9)).await.unwrap();
        let page = port
            .scan("p", StorageScanQuery { prefix: String::new(), after: None, limit: None })
            .await
            .unwrap();
        let keys: Vec<String> = page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["key"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(keys, ["alpha", "beta", "gamma"], "only this plugin's keys, sorted");
        assert!(page["next"].is_null());
        // after excludes the cursor itself; limit pages with next.
        let page = port
            .scan(
                "p",
                StorageScanQuery {
                    prefix: String::new(),
                    after: Some("alpha".into()),
                    limit: Some(1),
                },
            )
            .await
            .unwrap();
        assert_eq!(page["entries"][0]["key"], "beta");
        assert_eq!(page["next"], "beta");
        assert_eq!(page["entries"][0]["value"], 2);
        // prefix filters.
        let page = port
            .scan("p", StorageScanQuery { prefix: "a".into(), after: None, limit: None })
            .await
            .unwrap();
        assert_eq!(page["entries"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
