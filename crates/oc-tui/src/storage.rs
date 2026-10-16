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
//! **Migration (D-06, mirroring v2's v1→v2 idiom)**: earlier dev
//! builds of the port wrote `<state>/local/tui/…`. On state load the
//! one-time [`migrate_legacy_channel`] copies each persisted key from
//! the legacy `local` channel to the expected channel's directory when
//! the target is absent — the SOURCE is never touched (the v1→v2 DB
//! migration opens its source read-only the same way), the copy is
//! stamped with a `"migrated-from-local"` marker field, and the copy's
//! presence is the idempotence latch (a second boot is a no-op).
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
///
/// ## Versioning (the open-tabs model fix)
///
/// The port's earlier builds treated the resync's `session.list` as a
/// tab source, persisting EVERY session in history into
/// `global.tabs` — the "30+ crammed tabs" artifact. The open-tabs
/// model writes a **versioned** document ([`TABS_VERSION`]); on load
/// a file without the current version is the legacy all-history shape
/// and is reset (one-line log + the empty document written back),
/// never crashed on. (v2's own shape carries no version field; the
/// stamped `version` is an additive field v2 readers ignore.)
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TabsDocument {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub global: TabsScope,
    #[serde(default)]
    pub cwd: std::collections::BTreeMap<String, TabsScope>,
}

/// The document version the open-tabs model writes. Anything else on
/// disk (including the field's absence) is the legacy all-history
/// shape and resets on load.
pub const TABS_VERSION: u32 = 2;

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
/// The session sidebar's visibility mode (`session.sidebar` — v2
/// persists the toggle through the config doc; the channel key is the
/// TUI-side stand-in, last toggle wins).
pub const KEY_SIDEBAR: &str = "sidebar";

/// The sidebar onboarding card's dismissal (`getting-started` — v2's
/// `storage.store("getting-started", { initial: { dismissed: false } })`,
/// feature-plugins/sidebar/footer.tsx:11-13; auditC's state table
/// `<state>/<ch>/tui/getting-started.json`).
pub const KEY_GETTING_STARTED: &str = "getting-started";

/// The persisted onboarding document (`{"dismissed": bool}` — the
/// v2 storage draft's shape).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GettingStartedDoc {
    #[serde(default)]
    pub dismissed: bool,
}

/// The persisted sidebar document (`{"mode": "auto"|"hide"}`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SidebarDoc {
    /// The `session.sidebar` value the last toggle wrote.
    pub mode: crate::components::sidebar::SidebarMode,
}

/// The legacy channel earlier dev builds of the port wrote before the
/// factory default moved to `stable` (the migration SOURCE).
pub const LEGACY_CHANNEL: &str = "local";

/// Marker field stamped into every migrated document (inside the state
/// JSON — decoders ignore unknown fields, so v2-shape documents keep
/// round-tripping; a later typed write legitimately drops it, because
/// the migrated file's PRESENCE is the idempotence latch, not the
/// marker).
pub const MIGRATED_FROM_FIELD: &str = "migrated-from-local";

/// The keys [`ChannelStorage::open_default_migrated`] carries over from
/// the legacy channel (everything the TUI persists).
pub const MIGRATED_KEYS: &[&str] = &[KEY_TABS, KEY_SESSION_LIST, KEY_GETTING_STARTED];

/// The one-time `local`→expected-channel migration (D-06), mirroring
/// v2's v1→v2 idiom: source read-only, completion observable at the
/// destination, idempotent. For each key: when the expected channel's
/// file is ABSENT and the legacy channel has one, copy it over with
/// the [`MIGRATED_FROM_FIELD`] marker. The source file is never
/// modified, moved, or removed. Returns how many keys migrated (the
/// boot logs it; tests assert it).
fn migrate_legacy_channel(state_root: &Path, channel: &str, keys: &[&str]) -> usize {
    if channel == LEGACY_CHANNEL {
        // The operator pinned the legacy channel — nothing to migrate.
        return 0;
    }
    let Ok(target_dir) = segment(channel) else { return 0 };
    let mut migrated = 0usize;
    for key in keys {
        let Ok(key) = segment(key) else { continue };
        let mut source = state_root.to_path_buf();
        source.push(LEGACY_CHANNEL);
        source.push("tui");
        source.push(format!("{key}.json"));
        let mut target = state_root.to_path_buf();
        target.push(target_dir);
        target.push("tui");
        target.push(format!("{key}.json"));
        if target.exists() || !source.exists() {
            continue;
        }
        // Read-only on the source; unparseable legacy files stay
        // ignored (the load falls back to `initial`, as v2's `load()`
        // does on any read/parse error).
        let Ok(bytes) = std::fs::read(&source) else { continue };
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { continue };
        let Some(object) = value.as_object_mut() else { continue };
        object.insert(MIGRATED_FROM_FIELD.to_owned(), serde_json::Value::Bool(true));
        if std::fs::create_dir_all(target.parent().expect("channel/tui parent")).is_err() {
            continue;
        }
        let body = serde_json::to_vec(&value).unwrap_or_default();
        let temporary =
            target.with_extension(format!("{}.{}.tmp", std::process::id(), write_counter()));
        if std::fs::write(&temporary, &body)
            .and_then(|()| std::fs::rename(&temporary, &target))
            .is_ok()
        {
            migrated += 1;
        } else {
            let _ = std::fs::remove_file(&temporary);
        }
    }
    migrated
}

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

    /// The production root WITH the one-time legacy-channel migration
    /// (D-06): runs [`migrate_legacy_channel`] over [`MIGRATED_KEYS`]
    /// before opening, so the first state load after the
    /// `local`→`stable` factory-default change carries the old tabs /
    /// session-list prefs over. Idempotent — the migrated file's
    /// presence latches it.
    pub fn open_default_migrated() -> Result<Self, String> {
        let root = oc_util::global_roots::resolve().state;
        let channel = channel();
        let migrated = migrate_legacy_channel(&root, &channel, MIGRATED_KEYS);
        if migrated > 0 {
            eprintln!(
                "opencode tui: migrated {migrated} state file(s) from the legacy \
                 '{LEGACY_CHANNEL}' channel to '{channel}' (source kept)"
            );
        }
        Self::open(&root, &channel)
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

    /// Read the `tabs` document under the open-tabs model: a file
    /// stamped with the current [`TABS_VERSION`] decodes normally;
    /// anything else (the field's absence, an older version) is not
    /// our shape — it loads the EMPTY document in memory but is left
    /// UNTOUCHED on disk. v2's TUI writes this same file without a
    /// version stamp in its own shape; rewriting it here would
    /// destroy v2's state (auditC S1) — one-way destructive interop
    /// is forbidden. Never panics on any on-disk content.
    pub fn read_tabs(&self) -> TabsDocument {
        let Some(value) = self.read(KEY_TABS) else { return TabsDocument::default() };
        if value.get("version").and_then(serde_json::Value::as_u64) == Some(u64::from(TABS_VERSION))
        {
            return serde_json::from_value(value).unwrap_or_default();
        }
        eprintln!(
            "opencode tui: foreign/legacy session-tabs state found; leaving the file untouched"
        );
        TabsDocument { version: TABS_VERSION, ..TabsDocument::default() }
    }

    /// Write the `tabs` document stamped with the current
    /// [`TABS_VERSION`].
    pub fn write_tabs(&self, document: &TabsDocument) -> Result<(), String> {
        let stamped = TabsDocument { version: TABS_VERSION, ..document.clone() };
        self.write_typed(KEY_TABS, &stamped)
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
    /// and the `cwd` scope object present (the `version` stamp rides
    /// along — v2 readers ignore the additive field).
    #[test]
    fn tabs_document_round_trips_the_reference_shape() {
        let document: TabsDocument = serde_json::from_value(json!({
            "version": 2,
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
                "version": 2,
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

    /// The onboarding doc's round trip (dogfood3 / auditC S2): the
    /// dismissal writes v2's `getting-started` shape
    /// (`{"dismissed":true}`) and the boot read seeds back — the
    /// restart leg of the PTY dismiss cycle (a second live boot in
    /// one process can't re-arm crossterm's input source, so the
    /// file contract is proven here).
    #[test]
    fn getting_started_doc_round_trips() {
        let root = temp_root("onboarding");
        let storage = ChannelStorage::open(&root, "stable").expect("open");
        assert_eq!(
            storage.read(KEY_GETTING_STARTED),
            None,
            "a fresh channel has no onboarding doc"
        );
        let doc: GettingStartedDoc = storage.read_or_default(KEY_GETTING_STARTED);
        assert!(!doc.dismissed, "the default initial value (footer.tsx:12)");
        storage.write_typed(KEY_GETTING_STARTED, &GettingStartedDoc { dismissed: true })
            .expect("write");
        let body =
            std::fs::read_to_string(storage.path_for(KEY_GETTING_STARTED)).expect("read back");
        assert_eq!(body, r#"{"dismissed":true}"#, "v2's compact storage-draft shape");
        let back: GettingStartedDoc = storage.read_or_default(KEY_GETTING_STARTED);
        assert!(back.dismissed, "the boot read seeds the dismissal");
        // A v2-written file (no whitespace) reads back too.
        std::fs::write(storage.path_for(KEY_GETTING_STARTED), r#"{"dismissed":true}"#)
            .unwrap();
        assert!(storage.read_or_default::<GettingStartedDoc>(KEY_GETTING_STARTED).dismissed);
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
            version: TABS_VERSION,
            global: TabsScope {
                tabs: vec![
                    TabEntry { session_id: "ses_a".into(), title: Some("A".into()) },
                    TabEntry { session_id: "ses_b".into(), title: None },
                ],
                unread: std::collections::BTreeMap::new(),
            },
            cwd: std::collections::BTreeMap::new(),
        };
        storage.write_tabs(&document).expect("write");
        let back: TabsDocument = storage.read_or_default(KEY_TABS);
        assert_eq!(back, document);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The open-tabs model's version gate: a versioned file loads
    /// through [`ChannelStorage::read_tabs`]; the legacy all-history
    /// shape (no `version` field — every session in `global.tabs`)
    /// is ignored, logged, and RESET on disk; corrupt files still
    /// fall back without crashing.
    #[test]
    fn read_tabs_leaves_foreign_shapes_untouched_on_disk() {
        let root = temp_root("foreign-untouched");
        let storage = ChannelStorage::open(&root, "stable").expect("open");
        // The legacy artifact: 30+ sessions in global.tabs, no version.
        let legacy_tabs: Vec<String> =
            (0..30).map(|i| format!(r#"{{"sessionID":"ses_{i}"}}"#)).collect();
        let legacy = format!(
            r#"{{"cwd":{{}},"global":{{"tabs":[{}],"unread":{{}}}}}}"#,
            legacy_tabs.join(",")
        );
        std::fs::write(storage.path_for(KEY_TABS), &legacy).unwrap();

        // Load: ignored → empty in memory; the file on disk stays
        // BYTE-IDENTICAL to what the foreign writer left (auditC S1:
        // v2 writes this file without a version stamp — rewriting it
        // would destroy v2's state).
        let document = storage.read_tabs();
        assert!(document.global.tabs.is_empty(), "foreign tabs ignored");
        assert!(document.cwd.is_empty());
        let on_disk = std::fs::read(storage.path_for(KEY_TABS)).unwrap();
        assert_eq!(on_disk, legacy.as_bytes(), "foreign file is never rewritten");

        // A current-version document round-trips untouched.
        let current = TabsDocument {
            version: TABS_VERSION,
            global: TabsScope {
                tabs: vec![TabEntry { session_id: "ses_kept".into(), title: Some("Kept".into()) }],
                unread: std::collections::BTreeMap::new(),
            },
            cwd: std::collections::BTreeMap::new(),
        };
        storage.write_tabs(&current).expect("write");
        assert_eq!(storage.read_tabs(), current);

        // Corrupt content still falls back to default (no crash), and
        // an unknown future version is treated as legacy (reset).
        std::fs::write(storage.path_for(KEY_TABS), "not json").unwrap();
        assert_eq!(storage.read_tabs().global.tabs, Vec::<TabEntry>::new());
        std::fs::write(
            storage.path_for(KEY_TABS),
            r#"{"version":99,"global":{"tabs":[{"sessionID":"x"}]}}"#,
        )
        .unwrap();
        assert!(storage.read_tabs().global.tabs.is_empty(), "future version resets");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-06: staged OLD layout (`<state>/local/tui/…`) → migration →
    /// the stable location is populated with a marker, and the source
    /// is byte-for-byte untouched (copy, never move).
    #[test]
    fn legacy_local_state_migrates_into_the_stable_channel() {
        let root = temp_root("migrate");
        let legacy_tabs =
            r#"{"global":{"tabs":[{"sessionID":"ses_old","title":"Kept"}],"unread":{}},"cwd":{}}"#;
        let legacy_prefs = r#"{"all_projects":false}"#;
        std::fs::create_dir_all(root.join("local").join("tui")).unwrap();
        std::fs::write(root.join("local").join("tui").join("tabs.json"), legacy_tabs).unwrap();
        std::fs::write(root.join("local").join("tui").join("session-list.json"), legacy_prefs)
            .unwrap();

        let migrated = migrate_legacy_channel(&root, "stable", &[KEY_TABS, KEY_SESSION_LIST]);
        assert_eq!(migrated, 2, "both persisted keys carry over");

        let stable_tabs = root.join("stable").join("tui").join("tabs.json");
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&stable_tabs).unwrap()).unwrap();
        assert_eq!(value[MIGRATED_FROM_FIELD], json!(true), "the marker rides along");
        assert_eq!(value["global"]["tabs"][0]["sessionID"], "ses_old");
        let prefs: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("stable").join("tui").join("session-list.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(prefs[MIGRATED_FROM_FIELD], json!(true));
        assert_eq!(prefs["all_projects"], json!(false), "the payload survives verbatim");

        // The source is untouched — same bytes, still in place.
        assert_eq!(
            std::fs::read_to_string(root.join("local").join("tui").join("tabs.json")).unwrap(),
            legacy_tabs
        );
        assert_eq!(
            std::fs::read_to_string(root.join("local").join("tui").join("session-list.json"))
                .unwrap(),
            legacy_prefs
        );

        // The migrated tabs decode through the typed reader (the marker
        // is an unknown field — the v2 shape round-trips).
        let storage = ChannelStorage::open(&root, "stable").expect("open");
        let document: TabsDocument = storage.read_or_default(KEY_TABS);
        assert_eq!(document.global.tabs[0].session_id, "ses_old");
        assert_eq!(document.global.tabs[0].title.as_deref(), Some("Kept"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// D-06 idempotence: the second boot is a no-op (the stable file's
    /// presence latches), a PRE-EXISTING stable file is never
    /// overwritten, the legacy channel itself never self-migrates, and
    /// corrupt legacy sources stay ignored.
    #[test]
    fn legacy_local_migration_is_idempotent_and_guarded() {
        let root = temp_root("migrate-idem");
        std::fs::create_dir_all(root.join("local").join("tui")).unwrap();
        std::fs::write(
            root.join("local").join("tui").join("tabs.json"),
            r#"{"global":{"tabs":[{"sessionID":"ses_old"}],"unread":{}},"cwd":{}}"#,
        )
        .unwrap();
        // Corrupt legacy prefs: skipped, not migrated.
        std::fs::write(root.join("local").join("tui").join("session-list.json"), "not json")
            .unwrap();

        let first = migrate_legacy_channel(&root, "stable", MIGRATED_KEYS);
        assert_eq!(first, 1, "only the parseable key carries over");
        let stable_tabs = root.join("stable").join("tui").join("tabs.json");
        let first_pass = std::fs::read_to_string(&stable_tabs).unwrap();
        assert!(
            !root.join("stable").join("tui").join("session-list.json").exists(),
            "corrupt source not copied"
        );

        // Second boot: stable exists → nothing re-migrates, byte-stable.
        assert_eq!(migrate_legacy_channel(&root, "stable", MIGRATED_KEYS), 0);
        assert_eq!(std::fs::read_to_string(&stable_tabs).unwrap(), first_pass);

        // A pre-existing stable file wins over the legacy source.
        std::fs::write(&stable_tabs, r#"{"global":{"tabs":[{"sessionID":"ses_new"}]}}"#).unwrap();
        assert_eq!(migrate_legacy_channel(&root, "stable", MIGRATED_KEYS), 0);
        assert!(
            std::fs::read_to_string(&stable_tabs).unwrap().contains("ses_new"),
            "the live stable state is never clobbered"
        );

        // The legacy channel pinned via env is a self-no-op.
        assert_eq!(migrate_legacy_channel(&root, "local", MIGRATED_KEYS), 0);

        // The source survived everything above.
        assert!(root.join("local").join("tui").join("tabs.json").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("local").join("tui").join("tabs.json")).unwrap(),
            r#"{"global":{"tabs":[{"sessionID":"ses_old"}],"unread":{}},"cwd":{}}"#
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
