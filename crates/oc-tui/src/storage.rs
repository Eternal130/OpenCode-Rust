//! Channel storage — the port of v2's `useStorage` contract
//! (`packages/tui/src/context/storage.tsx`).
//!
//! ## The contract (verified against the reference source)
//!
//! v2's `useStorage().store(key, …)` does **not** talk to the server:
//! it is a **local-filesystem** store rooted at
//! `<state-root>/<channel>/tui/<key>.json` where
//!
//! * `state-root` = `useTuiPaths().state` =
//!   `${XDG_STATE_HOME:-~/.local/state}/opencode`
//!   (`packages/util/src/global.ts` + `global-roots.ts`), and
//! * `channel` = `OPENCODE_TUI_CHANNEL ?? OPENCODE_CHANNEL` — the
//!   build-channel constant. A **shipped** build bakes
//!   `OPENCODE_CHANNEL="stable"` (the pinned v2.0.24 reference build
//!   compiles with exactly that), so the shipped TUI reads/writes
//!   `<state>/stable/tui/…`; only from-source dev builds fall back to
//!   `"local"`.
//!
//! The Rust port ships release-style binaries (the release build
//! reports 2.0.24 and corresponds to the pinned stable reference), so
//! the storage channel defaults to **`stable`** — the baked release
//! channel (gate-12 §3.2). `OPENCODE_TUI_CHANNEL` overrides (the TUI
//! kill-switch), then `OPENCODE_CHANNEL` when the operator pinned a
//! process-wide channel (the runtime stand-in for v2's build constant;
//! the DB/engine channel source reads the same variable).
//!
//! **Migration note (documented residual)**: earlier dev builds of the
//! port wrote `<state>/local/tui/…`. That state is deliberately
//! IGNORED — the channel change is a one-way rename (tabs/session-list
//! prefs re-seed on first use); copy the `local` directory to `stable`
//! by hand to carry it over.
//!
//! Writes are atomic (temp file + rename, compact
//! `JSON.stringify`-shaped JSON, no trailing newline) and keys are
//! validated (`segment()`: `[a-zA-Z0-9][a-zA-Z0-9._-]*`, not `.` or
//! `..`). The TUI persists `tabs` there
//! (`context/session-tabs.tsx:67-72`) and `session-list` prefs there
//! (`component/dialog-session-list.tsx:42`).
//!
//! The Rust port mirrors that byte-for-byte so a v2 TUI and the Rust
//! TUI pointed at the same state directory interoperate on the same
//! tab list. Cross-process flock serialization and fs.watch
//! live-reload are v2 implementation details not required for the
//! single-owner TUI path (documented divergence; safe because writes
//! are atomic renames either way).
//!
//! No oc-server endpoint exists or is needed — v2 has none.

use std::path::{Path, PathBuf};

/// The baked release channel (v2's shipped builds compile with
/// `OPENCODE_CHANNEL="stable"`; the port's release binaries correspond
/// to the pinned stable reference — gate-12 §3.2's residual).
pub const DEFAULT_CHANNEL: &str = "stable";

/// The TUI's channel: `OPENCODE_TUI_CHANNEL` → `OPENCODE_CHANNEL` (when
/// the operator pinned one) → the baked release channel.
/// (v2: `process.env.OPENCODE_TUI_CHANNEL ?? OPENCODE_CHANNEL` with the
/// constant baked at build time.)
#[must_use]
pub fn channel() -> String {
    if let Ok(value) = std::env::var("OPENCODE_TUI_CHANNEL") {
        if !value.is_empty() {
            return value;
        }
    }
    if let Ok(value) = std::env::var("OPENCODE_CHANNEL") {
        if !value.is_empty() {
            return value;
        }
    }
    DEFAULT_CHANNEL.to_owned()
}

/// `segment()` — validate one path segment of a storage key/channel
/// (the reference's regex `^[a-zA-Z0-9][a-zA-Z0-9._-]*$`, rejecting
/// `.` and `..`).
fn segment(value: &str) -> Result<&str, String> {
    let valid = !value.is_empty()
        && value != "."
        && value != ".."
        && value.chars().next().is_some_and(|ch| ch.is_ascii_alphanumeric())
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'));
    if valid {
        Ok(value)
    } else {
        Err(format!("Invalid storage segment: {value}"))
    }
}

/// The persisted `tabs` document (`PersistedState` in
/// `context/session-tabs.tsx`): `global` + per-cwd scopes, each
/// `{ tabs: SessionTab[], unread: {} }` (the `unread` field is kept
/// empty for rollback compatibility with older readers — v2 comment).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TabsDocument {
    #[serde(default)]
    pub global: TabsScope,
    #[serde(default)]
    pub cwd: std::collections::BTreeMap<String, TabsScope>,
}

/// One scope's persisted tabs (`TabsState`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TabsScope {
    #[serde(default)]
    pub tabs: Vec<TabEntry>,
    /// Always `{}` — kept for shape compatibility (v2 writes it empty).
    #[serde(default)]
    pub unread: std::collections::BTreeMap<String, serde_json::Value>,
}

/// One persisted tab (`SessionTab`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TabEntry {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// The `session-list` prefs document (`dialog-session-list.tsx:42`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionListPrefs {
    #[serde(default = "default_all_projects")]
    pub all_projects: bool,
}

fn default_all_projects() -> bool {
    true
}

impl Default for SessionListPrefs {
    fn default() -> Self {
        Self { all_projects: default_all_projects() }
    }
}

/// The storage keys the TUI persists (v2 names).
pub const KEY_TABS: &str = "tabs";
pub const KEY_SESSION_LIST: &str = "session-list";

/// One channel's storage directory (`createStorage(root, channel)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelStorage {
    directory: PathBuf,
}

impl ChannelStorage {
    /// Open (and create) `<state-root>/<channel>/tui`.
    pub fn open(state_root: &Path, channel: &str) -> Result<Self, String> {
        let channel = segment(channel)?;
        let mut directory = state_root.to_path_buf();
        directory.push(channel);
        directory.push("tui");
        std::fs::create_dir_all(&directory).map_err(|error| format!("storage dir: {error}"))?;
        Ok(Self { directory })
    }

    /// The production root: `oc_util::global_roots().state` + the
    /// resolved channel.
    pub fn open_default() -> Result<Self, String> {
        Self::open(&oc_util::global_roots::resolve().state, &channel())
    }

    /// `<directory>/<key>.json` (validated).
    #[must_use]
    pub fn path_for(&self, key: &str) -> PathBuf {
        let key = segment(key).unwrap_or("invalid");
        let mut path = self.directory.clone();
        path.push(format!("{key}.json"));
        path
    }

    /// Read one key; `None` when absent or unparseable (v2's `load()`
    /// falls back to `options.initial` on any read/parse error).
    #[must_use]
    pub fn read(&self, key: &str) -> Option<serde_json::Value> {
        let bytes = std::fs::read(self.path_for(key)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Write one key atomically (temp file + rename; compact JSON like
    /// v2's `writeJsonAtomic`).
    pub fn write(&self, key: &str, value: &serde_json::Value) -> Result<(), String> {
        let path = self.path_for(key);
        let body = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        let temporary =
            path.with_extension(format!("{}.{}.tmp", std::process::id(), write_counter()));
        std::fs::write(&temporary, &body).and_then(|()| std::fs::rename(&temporary, &path)).map_err(
            |error| {
                let _ = std::fs::remove_file(&temporary);
                format!("storage write: {error}")
            },
        )
    }

    /// Read + decode one typed key; `None` falls back to default (the
    /// reference's `initial`).
    #[must_use]
    pub fn read_or_default<T: serde::de::DeserializeOwned + Default>(&self, key: &str) -> T {
        self.read(key).and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default()
    }

    /// Encode + write one typed key.
    pub fn write_typed<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<(), String> {
        let encoded = serde_json::to_value(value).map_err(|error| error.to_string())?;
        self.write(key, &encoded)
    }
}

/// Per-process write counter so concurrent atomic writes to one key
/// never share a temp filename (v2 uses pid + uuid).
fn write_counter() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Expose nothing further — [`ChannelStorage`] is the whole surface.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-tui-storage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The default channel is the baked RELEASE channel (`stable`, the
    /// pinned v2.0.24 reference build's `OPENCODE_CHANNEL` — gate-12
    /// §3.2), with `OPENCODE_TUI_CHANNEL` and `OPENCODE_CHANNEL`
    /// overriding in that order (v2's `??` chain).
    #[test]
    fn channel_defaults_to_the_release_channel() {
        // Serialize env mutations (other tests in this binary may read
        // the channel).
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved_tui = std::env::var_os("OPENCODE_TUI_CHANNEL");
        let saved_channel = std::env::var_os("OPENCODE_CHANNEL");
        let restore = move || {
            match saved_tui {
                Some(value) => std::env::set_var("OPENCODE_TUI_CHANNEL", value),
                None => std::env::remove_var("OPENCODE_TUI_CHANNEL"),
            }
            match saved_channel {
                Some(value) => std::env::set_var("OPENCODE_CHANNEL", value),
                None => std::env::remove_var("OPENCODE_CHANNEL"),
            }
        };
        std::env::remove_var("OPENCODE_TUI_CHANNEL");
        std::env::remove_var("OPENCODE_CHANNEL");
        assert_eq!(channel(), "stable", "the shipped-build default");
        std::env::set_var("OPENCODE_CHANNEL", "latest");
        assert_eq!(channel(), "latest", "a pinned process channel wins over the bake");
        std::env::set_var("OPENCODE_TUI_CHANNEL", "edge9");
        assert_eq!(channel(), "edge9", "the TUI-specific override wins");
        std::env::set_var("OPENCODE_TUI_CHANNEL", "");
        assert_eq!(channel(), "latest", "empty values are ignored");
        restore();
    }

    /// The exact v2 tabs document shape round-trips: `global.tabs[]`
    /// with `sessionID`/`title` keys, the empty `unread` map present,
    /// and the `cwd` scope object present.
    #[test]
    fn tabs_document_round_trips_the_reference_shape() {
        let document: TabsDocument = serde_json::from_value(json!({
            "global": {
                "tabs": [
                    { "sessionID": "ses_1", "title": "Bug hunt" },
                    { "sessionID": "ses_2" }
                ],
                "unread": {}
            },
            "cwd": {}
        }))
        .expect("v2 shape decodes");
        assert_eq!(document.global.tabs.len(), 2);
        assert_eq!(document.global.tabs[0].session_id, "ses_1");
        assert_eq!(document.global.tabs[0].title.as_deref(), Some("Bug hunt"));
        assert_eq!(document.global.tabs[1].title, None);
        // Re-encode: field names + the kept-empty unread map survive.
        let encoded = serde_json::to_value(&document).unwrap();
        assert_eq!(
            encoded,
            json!({
                "global": {
                    "tabs": [
                        { "sessionID": "ses_1", "title": "Bug hunt" },
                        { "sessionID": "ses_2" }
                    ],
                    "unread": {}
                },
                "cwd": {}
            })
        );
    }

    /// The file layout matches v2: `<state>/<channel>/tui/<key>.json`,
    /// keys validate, writes are atomic-safe, reads fall back.
    #[test]
    fn channel_storage_layout_matches_v2() {
        let root = temp_root("layout");
        let storage = ChannelStorage::open(&root, "local").expect("open");
        let path = storage.path_for(KEY_TABS);
        assert_eq!(path, root.join("local").join("tui").join("tabs.json"));

        storage.write(KEY_TABS, &json!({"global": {"tabs": []}})).expect("write");
        assert_eq!(storage.read(KEY_TABS), Some(json!({"global": {"tabs": []}})));
        // A v2-written file (compact, no trailing newline) reads back.
        std::fs::write(&path, r#"{"global":{"tabs":[{"sessionID":"s1"}],"unread":{}},"cwd":{}}"#)
            .unwrap();
        let document: TabsDocument = storage.read_or_default(KEY_TABS);
        assert_eq!(document.global.tabs[0].session_id, "s1");
        assert!(document.global.unread.is_empty(), "unread shape present");

        // Absent/unparseable keys fall back to default (v2 `initial`).
        assert_eq!(storage.read("never-written"), None);
        let prefs: SessionListPrefs = storage.read_or_default(KEY_SESSION_LIST);
        assert!(prefs.all_projects);
        std::fs::write(storage.path_for(KEY_SESSION_LIST), "not json").unwrap();
        let prefs: SessionListPrefs = storage.read_or_default(KEY_SESSION_LIST);
        assert!(prefs.all_projects, "corrupt file falls back to initial");

        // Invalid segments are rejected (v2 throws TypeError).
        assert!(ChannelStorage::open(&root, "../escape").is_err());
        assert!(ChannelStorage::open(&root, "").is_err());
        assert!(segment("_leading-dash").is_err());
        assert!(segment("a b").is_err());
        assert_eq!(segment("opencode.0.24"), Ok("opencode.0.24"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Round-trip of the whole persistence flow: store tabs → disk →
    /// a fresh storage instance reads the same document.
    #[test]
    fn typed_write_then_read_round_trips() {
        let root = temp_root("roundtrip");
        let storage = ChannelStorage::open(&root, "local").expect("open");
        let document = TabsDocument {
            global: TabsScope {
                tabs: vec![
                    TabEntry { session_id: "ses_a".into(), title: Some("A".into()) },
                    TabEntry { session_id: "ses_b".into(), title: None },
                ],
                unread: std::collections::BTreeMap::new(),
            },
            cwd: std::collections::BTreeMap::new(),
        };
        storage.write_typed(KEY_TABS, &document).expect("write");
        let back: TabsDocument = storage.read_or_default(KEY_TABS);
        assert_eq!(back, document);
        let _ = std::fs::remove_dir_all(&root);
    }
}
