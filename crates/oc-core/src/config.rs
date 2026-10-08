//! Config loading pipeline — port of `packages/core/src/config.ts`.
//!
//! Six-source ordered entry list (lowest → highest priority, `latest`/findLast
//! wins): wellknown → globalSupplementary → explicit `OPENCODE_CONFIG` file →
//! direct walk-up files (root-most first, nearest wins; same dir json before
//! jsonc so jsonc wins) → projectSupplementary `.opencode` dirs → inline
//! `OPENCODE_CONFIG_CONTENT`. Parsing per `config/variable.rs` (`{env}`/`{file}`
//! substitution), jsonc tolerance via json5, normalization per
//! `config/normalize.rs`, and final decode into [`Info`] with per-document
//! isolation (a bad document never fails the whole load).

pub mod agent_md;
pub mod discovery;
pub mod instruction;
pub mod markdown;
pub mod normalize;
pub mod variable;
pub mod watch;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use oc_schema::config::{Directory, Document, Entry, Info};

pub use normalize::{Diagnostic, DiagnosticKind, NormalizeResult};
pub use watch::{ChangePublisher, ConfigUpdated, FsUpdate, WatchBackend, WatchHub, WatchTarget};

/// Config file names in load order; the later `jsonc` wins on `latest`.
pub const NAMES: [&str; 2] = ["opencode.json", "opencode.jsonc"];

/// `Config.latest` — last document (highest priority) where `extract` yields a
/// value. Mirrors `entries.findLast(entry => entry.info[key] !== undefined)`.
pub fn latest<T: Clone>(entries: &[Entry], extract: impl Fn(&Info) -> Option<T>) -> Option<T> {
    entries.iter().rev().find_map(|entry| match entry {
        Entry::Document(document) => extract(&document.info),
        Entry::Directory(_) => None,
    })
}

/// Recovers a `MutexGuard` from a poisoned lock; config/watch state is
/// always left consistent enough to read (mirrors v2's tolerate-and-continue).
pub(crate) fn unpoison<T>(error: std::sync::PoisonError<T>) -> T {
    error.into_inner()
}

/// Env lookup used for `{env:}` substitution and wellknown credentials.
pub type EnvLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Global directories feeding discovery (the `Global` service in v2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDirs {
    /// Global config directory (`Global.config`).
    pub config: PathBuf,
    /// User home (`Global.home`).
    pub home: PathBuf,
}

impl GlobalDirs {
    /// Resolves from the process environment via oc-util's XDG rules.
    #[must_use]
    pub fn from_env() -> Self {
        let roots = oc_util::global_roots::resolve();
        Self { config: roots.config, home: roots.home }
    }
}

/// Loader options (`Config.Options`): `project: false` skips the walk-up,
/// `global: false` skips the global config dir and `~/.claude`/`~/.agents`;
/// wellknown, `file`, and `content` entries still load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub project: Option<bool>,
    pub global: Option<bool>,
    pub file: Option<PathBuf>,
    pub content: Option<String>,
}

/// One wellknown-derived document (remote/credential-derived configs).
#[derive(Debug, Clone)]
pub struct WellknownDocument {
    /// Origin identifier used in diagnostics.
    pub source: String,
    /// Base directory for `{file:}` references.
    pub dir: PathBuf,
    /// Serialized config text.
    pub text: String,
    /// Credential-derived env overrides applied before process env.
    pub env: BTreeMap<String, String>,
}

/// Source of wellknown config documents; the real implementation depends on
/// credentials + models.dev manifests that land in later lanes.
pub trait WellknownSource: Send + Sync {
    fn documents(&self, env: &dyn Fn(&str) -> Option<String>) -> Vec<WellknownDocument>;
}

/// Empty wellknown source (tests, vanilla boots).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWellknown;

impl WellknownSource for NoWellknown {
    fn documents(&self, _env: &dyn Fn(&str) -> Option<String>) -> Vec<WellknownDocument> {
        Vec::new()
    }
}

/// A logged normalization diagnostic, tagged with its originating source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDiagnostic {
    pub source: String,
    /// `"$"` for the document root, else `"$.a.b"`.
    pub path: String,
    pub kind: DiagnosticKind,
    pub message: String,
}

/// Ordered JSON value. `serde_json`'s default map is sorted, but v2 relies on
/// document key order (`propertyOrder: "original"`, permission rule order),
/// so parsing and normalization run over this insertion-ordered shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    #[must_use]
    pub fn object(entries: Vec<(String, Json)>) -> Self {
        Json::Object(entries)
    }

    #[must_use]
    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(entries) => Some(entries),
            _ => None,
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object()?.iter().find(|(name, _)| name == key).map(|(_, value)| value)
    }

    pub fn set(&mut self, key: &str, value: Json) {
        if let Json::Object(entries) = self {
            for (name, slot) in entries.iter_mut() {
                if name == key {
                    *slot = value;
                    return;
                }
            }
            entries.push((key.to_string(), value));
        }
    }

    #[must_use]
    pub fn is_object(&self) -> bool {
        matches!(self, Json::Object(_))
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_array(&self) -> bool {
        matches!(self, Json::Array(_))
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Deep clone with `Null` entries dropped, matching v2's `plain()`.
    #[must_use]
    pub fn plain(&self) -> Json {
        match self {
            Json::Array(items) => Json::Array(items.iter().map(Json::plain).collect()),
            Json::Object(entries) => Json::Object(
                entries
                    .iter()
                    .filter(|(_, value)| !matches!(value, Json::Null))
                    .map(|(key, value)| (key.clone(), value.plain()))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

impl From<Json> for serde_json::Value {
    fn from(value: Json) -> Self {
        match value {
            Json::Null => serde_json::Value::Null,
            Json::Bool(value) => serde_json::Value::Bool(value),
            Json::Number(value) => serde_json::Value::Number(value),
            Json::String(value) => serde_json::Value::String(value),
            Json::Array(items) => {
                serde_json::Value::Array(items.into_iter().map(serde_json::Value::from).collect())
            }
            Json::Object(entries) => serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, serde_json::Value::from(value)))
                    .collect(),
            ),
        }
    }
}

impl From<serde_json::Value> for Json {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Json::Null,
            serde_json::Value::Bool(value) => Json::Bool(value),
            serde_json::Value::Number(value) => Json::Number(value),
            serde_json::Value::String(value) => Json::String(value),
            serde_json::Value::Array(items) => {
                Json::Array(items.into_iter().map(Json::from).collect())
            }
            serde_json::Value::Object(entries) => Json::Object(
                entries.into_iter().map(|(key, value)| (key, Json::from(value))).collect(),
            ),
        }
    }
}

impl serde::Serialize for Json {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => serializer.serialize_none(),
            Json::Bool(value) => serializer.serialize_bool(*value),
            Json::Number(value) => value.serialize(serializer),
            Json::String(value) => serializer.serialize_str(value),
            Json::Array(items) => serializer.collect_seq(items),
            Json::Object(entries) => serializer.collect_map(entries.iter().cloned()),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Json {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Json;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("any JSON value")
            }
            fn visit_bool<E>(self, value: bool) -> Result<Json, E> {
                Ok(Json::Bool(value))
            }
            fn visit_i64<E>(self, value: i64) -> Result<Json, E> {
                Ok(Json::Number(serde_json::Number::from(value)))
            }
            fn visit_u64<E>(self, value: u64) -> Result<Json, E> {
                Ok(Json::Number(serde_json::Number::from(value)))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Json, E> {
                Ok(Json::Number(
                    serde_json::Number::from_f64(value)
                        .ok_or_else(|| serde::de::Error::custom("non-finite number"))?,
                ))
            }
            fn visit_str<E>(self, value: &str) -> Result<Json, E> {
                Ok(Json::String(value.to_string()))
            }
            fn visit_none<E>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Json, D::Error> {
                serde::Deserialize::deserialize(deserializer)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element()? {
                    items.push(item);
                }
                Ok(Json::Array(items))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut entries = Vec::new();
                while let Some((key, value)) = map.next_entry()? {
                    entries.push((key, value));
                }
                Ok(Json::Object(entries))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

/// Resolves `path` the way v2's `fs.resolve` does: realpath when it exists,
/// lexical absolutization otherwise.
pub(crate) fn resolve_path(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(real) => real,
        Err(_) => {
            if path.is_absolute() {
                clean_path(path)
            } else {
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
                clean_path(&cwd.join(path))
            }
        }
    }
}

/// Lexically normalizes `.`/`..` components without touching the filesystem.
fn clean_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Ancestors of `start` (inclusive), nearest first.
pub(crate) fn walk_up(start: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut current = Some(start.to_path_buf());
    while let Some(directory) = current {
        out.push(directory.clone());
        current = directory.parent().map(Path::to_path_buf);
    }
    out
}

/// One loaded configuration snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub sources: discovery::Sources,
    pub entries: Vec<Entry>,
    pub diagnostics: Vec<SourceDiagnostic>,
}

/// The config loader (`Config.load`): discover sources, parse each document in
/// isolation, and emit the priority-ordered entry list.
#[derive(Clone)]
pub struct Loader {
    pub location: PathBuf,
    pub globals: GlobalDirs,
    pub options: Options,
    pub wellknown: Arc<dyn WellknownSource>,
    pub env: EnvLookup,
}

impl Loader {
    #[must_use]
    pub fn new(location: PathBuf, globals: GlobalDirs) -> Self {
        Self {
            location,
            globals,
            options: Options::default(),
            wellknown: Arc::new(NoWellknown),
            env: Arc::new(|name| std::env::var(name).ok()),
        }
    }

    #[must_use]
    pub fn with_options(mut self, options: Options) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn with_wellknown(mut self, wellknown: Arc<dyn WellknownSource>) -> Self {
        self.wellknown = wellknown;
        self
    }

    #[must_use]
    pub fn with_env(mut self, env: EnvLookup) -> Self {
        self.env = env;
        self
    }

    /// Discovers and loads all sources.
    #[must_use]
    pub fn load(&self) -> Loaded {
        let sources = discovery::discover(&self.location, &self.globals, &self.options);
        self.load_sources(&sources)
    }

    /// Loads an already-discovered source set.
    #[must_use]
    pub fn load_sources(&self, sources: &discovery::Sources) -> Loaded {
        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();

        // 1. wellknown (lowest priority).
        for document in self.wellknown.documents(&*self.env) {
            let lookup = |name: &str| document.env.get(name).cloned().or_else(|| (self.env)(name));
            let substituted = variable::substitute(&variable::Input {
                source: variable::Source::Virtual {
                    source: document.source.clone(),
                    dir: document.dir.clone(),
                },
                text: &document.text,
                env: &lookup,
                missing: variable::Missing::Error,
            });
            let text = match substituted {
                Ok(text) => text,
                Err(error) => {
                    diagnostics.push(SourceDiagnostic {
                        source: document.source.clone(),
                        path: "$".to_string(),
                        kind: DiagnosticKind::Invalid,
                        message: error.message,
                    });
                    continue;
                }
            };
            if let Some(info) = Self::parse_info(&text, &document.source, &mut diagnostics) {
                entries.push(Entry::Document(Box::new(Document { info, path: None })));
            }
        }

        // 2. global supplementary: global dir documents + Directory entry.
        if let Some(global) = &sources.global {
            self.load_directory(global, &mut entries, &mut diagnostics);
        }

        // 3. explicit OPENCODE_CONFIG file.
        if let Some(explicit) = &sources.explicit {
            if let Some(document) = self.load_file(explicit, &mut diagnostics) {
                entries.push(Entry::Document(document));
            }
        }

        // 4. direct walk-up files (root-most first; nearest last wins).
        for direct in &sources.direct {
            if let Some(document) = self.load_file(direct, &mut diagnostics) {
                entries.push(Entry::Document(document));
            }
        }

        // 5. project supplementary: each present .opencode dir.
        for root in sources.project.iter().filter(|root| root.present) {
            self.load_directory(&root.path, &mut entries, &mut diagnostics);
        }

        // 6. inline OPENCODE_CONFIG content (highest priority).
        if let Some(content) = &self.options.content {
            match variable::substitute(&variable::Input {
                source: variable::Source::Virtual {
                    source: "OPENCODE_CONFIG_CONTENT".to_string(),
                    dir: self.location.clone(),
                },
                text: content,
                env: &*self.env,
                missing: variable::Missing::Error,
            }) {
                Ok(text) => {
                    if let Some(info) =
                        Self::parse_info(&text, "OPENCODE_CONFIG_CONTENT", &mut diagnostics)
                    {
                        entries.push(Entry::Document(Box::new(Document { info, path: None })));
                    }
                }
                Err(error) => diagnostics.push(SourceDiagnostic {
                    source: "OPENCODE_CONFIG_CONTENT".to_string(),
                    path: "$".to_string(),
                    kind: DiagnosticKind::Invalid,
                    message: error.message,
                }),
            }
        }

        Loaded { sources: sources.clone(), entries, diagnostics }
    }

    /// `parseInfo`: jsonc-parse → normalize → final decode; every failure is a
    /// diagnostic plus `None` (per-document isolation).
    fn parse_info(
        text: &str,
        source: &str,
        diagnostics: &mut Vec<SourceDiagnostic>,
    ) -> Option<Info> {
        let Ok(value) = json5::from_str::<Json>(text) else {
            diagnostics.push(SourceDiagnostic {
                source: source.to_string(),
                path: "$".to_string(),
                kind: DiagnosticKind::Invalid,
                message: "rejected malformed JSON or JSONC document".to_string(),
            });
            return None;
        };
        let (encoded, result_diagnostics) = match normalize::normalize(&value) {
            NormalizeResult::Normalized { encoded, diagnostics: result } => (Some(encoded), result),
            NormalizeResult::Rejected { diagnostics: result } => (None, result),
        };
        for diagnostic in &result_diagnostics {
            diagnostics.push(SourceDiagnostic {
                source: source.to_string(),
                path: if diagnostic.path.first().is_some_and(|first| first == "$") {
                    "$".to_string()
                } else {
                    format!("$.{}", diagnostic.path.join("."))
                },
                kind: diagnostic.kind,
                message: diagnostic.message.clone(),
            });
        }
        let encoded = encoded?;
        if let Ok(info) = serde_json::from_value::<Info>(encoded.into()) {
            return Some(info);
        }
        diagnostics.push(SourceDiagnostic {
            source: source.to_string(),
            path: "$".to_string(),
            kind: DiagnosticKind::Invalid,
            message: "rejected canonical configuration after final validation".to_string(),
        });
        None
    }

    fn load_file(
        &self,
        path: &Path,
        diagnostics: &mut Vec<SourceDiagnostic>,
    ) -> Option<Box<Document>> {
        let text = std::fs::read_to_string(path).ok()?;
        let substituted = variable::substitute(&variable::Input {
            source: variable::Source::Path { path: path.to_path_buf() },
            text: &text,
            env: &*self.env,
            missing: variable::Missing::Error,
        });
        let text = match substituted {
            Ok(text) => text,
            Err(error) => {
                diagnostics.push(SourceDiagnostic {
                    source: path.display().to_string(),
                    path: "$".to_string(),
                    kind: DiagnosticKind::Invalid,
                    message: error.message,
                });
                return None;
            }
        };
        let info = Self::parse_info(&text, &path.display().to_string(), diagnostics)?;
        Some(Box::new(Document { info, path: Some(path.display().to_string()) }))
    }

    fn load_directory(
        &self,
        directory: &Path,
        entries: &mut Vec<Entry>,
        diagnostics: &mut Vec<SourceDiagnostic>,
    ) {
        for name in NAMES {
            if let Some(document) = self.load_file(&directory.join(name), diagnostics) {
                entries.push(Entry::Document(document));
            }
        }
        entries.push(Entry::Directory(Directory { path: directory.display().to_string() }));
    }
}

/// Live config with watch-driven hot reload (`Config.layer` + `ConfigWatch`).
///
/// Reloads are debounced 100ms and serialized; a reload that produces
/// deep-equal entries (and unchanged compatibility roots) emits nothing, any
/// other reload fires [`ConfigUpdated::on_config_updated`]. Raw filesystem
/// updates under config roots stream to [`ChangePublisher`] subscribers.
pub struct WatchedConfig {
    loader: Loader,
    state: Mutex<Loaded>,
    hub: Arc<watch::WatchHub>,
    reload_lock: Mutex<()>,
}

impl WatchedConfig {
    /// Builds the service, performs the initial load, reconciles watches, and
    /// starts the debounced reload loop. A readiness rescan is requested so
    /// writes racing watch attachment are picked up.
    #[must_use]
    pub fn start(
        loader: Loader,
        backend: Arc<dyn WatchBackend>,
        updated: Arc<dyn ConfigUpdated>,
        changes: Option<Arc<dyn ChangePublisher>>,
    ) -> Arc<Self> {
        let hub = watch::WatchHub::new(backend);
        if let Some(publisher) = changes {
            hub.add_publisher(publisher);
        }
        let initial = loader.load();
        hub.reconcile(&watch::plan(&initial.sources));
        let service = Arc::new(Self {
            loader,
            state: Mutex::new(initial),
            hub: hub.clone(),
            reload_lock: Mutex::new(()),
        });
        let weak: Weak<Self> = Arc::downgrade(&service);
        let reload = Arc::new(move || weak.upgrade().is_some_and(|service| service.do_reload()));
        hub.run(watch::DEBOUNCE, reload, updated);
        hub.request_reload();
        service
    }

    /// Current priority-ordered entries.
    pub fn entries(&self) -> Vec<Entry> {
        self.state.lock().unwrap_or_else(unpoison).entries.clone()
    }

    /// Current loaded snapshot.
    pub fn loaded(&self) -> Loaded {
        self.state.lock().unwrap_or_else(unpoison).clone()
    }

    /// Requests a debounced reload (watch events, credential switches, tests).
    pub fn request_reload(&self) {
        self.hub.request_reload();
    }

    /// Discover → load → reconcile watches → compare → swap+notify. Returns
    /// whether the configuration changed.
    fn do_reload(&self) -> bool {
        let _guard = self.reload_lock.lock().unwrap_or_else(unpoison);
        let discovered = self.loader.load();
        self.hub.reconcile(&watch::plan(&discovered.sources));
        let mut state = self.state.lock().unwrap_or_else(unpoison);
        let compatibility_changed = state.sources.claude != discovered.sources.claude
            || state.sources.agents != discovered.sources.agents;
        if state.entries == discovered.entries && !compatibility_changed {
            return false;
        }
        *state = discovered;
        true
    }
}

/// `Config.update`: writes only the `shell` patch into the global
/// `opencode.json(c)` while preserving unrelated JSONC content.
pub fn update_shell(globals: &GlobalDirs, shell: Option<&str>) -> std::io::Result<()> {
    let candidates: Vec<PathBuf> = NAMES.iter().map(|name| globals.config.join(name)).collect();
    let filepath = candidates
        .iter()
        .rev()
        .find(|candidate| candidate.is_file())
        .cloned()
        .unwrap_or_else(|| globals.config.join("opencode.jsonc"));
    let text = std::fs::read_to_string(&filepath).unwrap_or_else(|_| "{}\n".to_string());
    let updated = patch_shell(&text, shell);
    if let Some(parent) = filepath.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&filepath, updated)
}

/// Replaces or inserts the top-level `shell` property, preserving surrounding
/// JSONC text (comments, formatting, unrelated keys). A `None` shell writes
/// `null`, matching `modify(text, ["shell"], undefined)` treating the patch
/// value as JSON null.
#[must_use]
pub fn patch_shell(text: &str, shell: Option<&str>) -> String {
    let value = shell.map_or_else(
        || "null".to_string(),
        |shell| serde_json::to_string(shell).unwrap_or_else(|_| "\"\"".to_string()),
    );
    let Some((value_start, value_end)) = locate_top_level_property(text, "shell") else {
        return insert_top_level_property(text, "shell", &value);
    };
    format!("{}{}{}", &text[..value_start], value, &text[value_end..])
}

/// Finds the value region (byte offsets, exclusive of a trailing comma) of a
/// top-level property, skipping strings and comments.
fn locate_top_level_property(text: &str, key: &str) -> Option<(usize, usize)> {
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    // Byte offset helpers over the char vector.
    let byte = |chars: &[char], i: usize| chars.get(i).copied();
    let mut depth = 0usize;
    let needle: Vec<char> = format!("\"{key}\"").chars().collect();
    while index < bytes.len() {
        let c = bytes[index];
        match c {
            '/' if byte(&bytes, index + 1) == Some('/') => {
                while index < bytes.len() && bytes[index] != '\n' {
                    index += 1;
                }
            }
            '/' if byte(&bytes, index + 1) == Some('*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == '*' && bytes[index + 1] == '/') {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            '"' => {
                let start = index;
                index += 1;
                while index < bytes.len() && bytes[index] != '"' {
                    if bytes[index] == '\\' {
                        index += 1;
                    }
                    index += 1;
                }
                index += 1;
                // A key at depth <= 1 followed by ':' is a property name.
                let mut scan = index;
                while scan < bytes.len() && bytes[scan].is_whitespace() {
                    scan += 1;
                }
                if depth <= 1
                    && scan < bytes.len()
                    && bytes[scan] == ':'
                    && bytes[start..index] == needle
                {
                    let mut value_start = scan + 1;
                    while value_start < bytes.len() && bytes[value_start].is_whitespace() {
                        value_start += 1;
                    }
                    let value_end = scan_value_end(&bytes, value_start);
                    return Some((
                        char_index_to_byte(&bytes, value_start),
                        char_index_to_byte(&bytes, value_end),
                    ));
                }
            }
            '{' | '[' => depth += 1,
            '}' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    None
}

/// Index (exclusive) just past the JSON value starting at `start`.
fn scan_value_end(chars: &[char], start: usize) -> usize {
    let mut index = start;
    let mut depth = 0usize;
    while index < chars.len() {
        let c = chars[index];
        match c {
            '/' if chars.get(index + 1) == Some(&'/') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '/' if chars.get(index + 1) == Some(&'*') => {
                index += 2;
                while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                    index += 1;
                }
                index = (index + 2).min(chars.len());
            }
            '"' => {
                index += 1;
                while index < chars.len() && chars[index] != '"' {
                    if chars[index] == '\\' {
                        index += 1;
                    }
                    index += 1;
                }
                index += 1;
                if depth == 0 {
                    return index;
                }
            }
            '{' | '[' => depth += 1,
            '}' | ']' | ',' if depth == 0 => return index,
            '}' | ']' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    chars.len()
}

fn char_index_to_byte(chars: &[char], index: usize) -> usize {
    chars[..index.min(chars.len())].iter().map(|item| item.len_utf8()).sum()
}

fn insert_top_level_property(text: &str, key: &str, value: &str) -> String {
    let entry = format!("\"{key}\": {value}");
    let trimmed = text.trim_end();
    if let (Some(open), Some(close)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if open < close {
            let mut head = trimmed[..close].to_string();
            let inner = trimmed[open + 1..close].trim();
            if !inner.is_empty() {
                head.push(',');
            }
            head.push('\n');
            head.push_str(&entry);
            head.push('\n');
            return format!("{head}}}\n");
        }
    }
    format!("{{\n  {entry}\n}}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("oc-core-config-tests")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn loader_for(dir: &Path, globals: &GlobalDirs) -> Loader {
        Loader::new(dir.to_path_buf(), globals.clone())
    }

    #[test]
    fn config_latest_returns_last_defined_value() {
        let entries = vec![
            Entry::Document(Box::new(Document {
                info: serde_json::from_value(
                    serde_json::json!({"model": "openrouter/openai/gpt-5"}),
                )
                .unwrap(),
                path: None,
            })),
            Entry::Directory(Directory { path: "/skills".to_string() }),
            Entry::Document(Box::new(Document { info: Info::default(), path: None })),
            Entry::Document(Box::new(Document {
                info: serde_json::from_value(
                    serde_json::json!({"model": "openrouter/openai/gpt-5.5"}),
                )
                .unwrap(),
                path: None,
            })),
        ];
        let model = latest(&entries, |info| info.model.clone()).unwrap();
        assert_eq!(model.model.as_str(), "openai/gpt-5.5");
        assert!(latest(&entries, |info| info.default_agent.clone()).is_none());
    }

    #[test]
    fn config_loads_json_and_jsonc_same_dir_jsonc_wins() {
        let dir = tempdir("same-dir");
        write(&dir.join("opencode.json"), r#"{"$schema": "base", "username": "from-json"}"#);
        write(
            &dir.join("opencode.jsonc"),
            "{\n  // comment\n  \"$schema\": \"last\",\n  \"username\": \"from-jsonc\",\n}",
        );
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&dir, &globals).load();
        let documents: Vec<&Entry> = loaded.entries.iter().collect();
        assert_eq!(documents.len(), 3, "two documents + global directory");
        let shells: Vec<Option<String>> = loaded
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Document(document) => Some(document.info.username.clone()),
                Entry::Directory(_) => None,
            })
            .collect();
        assert_eq!(shells, vec![Some("from-json".to_string()), Some("from-jsonc".to_string())]);
        assert_eq!(
            latest(&loaded.entries, |info| info.username.clone()).as_deref(),
            Some("from-jsonc")
        );
        let _ = documents;
    }

    #[test]
    fn config_walk_up_nearest_wins_and_root_most_first() {
        let dir = tempdir("walk-up");
        let root = dir.join("repo");
        let nested = root.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        write(&root.join("opencode.json"), r#"{"username": "root"}"#);
        write(&nested.join("opencode.json"), r#"{"username": "nested"}"#);
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&nested, &globals).load();
        let order: Vec<&str> = loaded
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Document(document) => document.info.username.as_deref(),
                Entry::Directory(_) => None,
            })
            .collect();
        assert_eq!(order, vec!["root", "nested"], "root-most first, nearest last");
        assert_eq!(
            latest(&loaded.entries, |info| info.username.clone()).as_deref(),
            Some("nested")
        );
    }

    #[test]
    fn config_explicit_and_content_priority_order() {
        let dir = tempdir("priority");
        let global = dir.join("global");
        let project = dir.join("project");
        let explicit = dir.join("custom.json");
        fs::create_dir_all(&global).unwrap();
        fs::create_dir_all(&project).unwrap();
        write(&global.join("opencode.json"), r#"{"shell": "global"}"#);
        write(&explicit, r#"{"shell": "explicit"}"#);
        write(&project.join("opencode.json"), r#"{"shell": "project"}"#);
        let globals = GlobalDirs { config: global.clone(), home: global.join("home") };
        let options = Options {
            file: Some(explicit.clone()),
            content: Some(r#"{"shell": "content"}"#.to_string()),
            ..Options::default()
        };
        let loaded = loader_for(&project, &globals).with_options(options).load();
        let shells: Vec<&str> = loaded
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Document(document) => document.info.shell.as_deref(),
                Entry::Directory(_) => None,
            })
            .collect();
        assert_eq!(shells, vec!["global", "explicit", "project", "content"]);
    }

    #[test]
    fn config_project_false_skips_walk_up() {
        let dir = tempdir("no-project");
        let global = dir.join("global");
        let project = dir.join("project");
        fs::create_dir_all(&global).unwrap();
        fs::create_dir_all(&project).unwrap();
        write(&global.join("opencode.json"), r#"{"shell": "global"}"#);
        write(&project.join("opencode.json"), r#"{"shell": "project"}"#);
        let globals = GlobalDirs { config: global, home: dir.join("home") };
        let loaded = loader_for(&project, &globals)
            .with_options(Options { project: Some(false), ..Options::default() })
            .load();
        assert_eq!(latest(&loaded.entries, |info| info.shell.clone()).as_deref(), Some("global"));
        // Only the global document + its directory entry remain; the walk-up
        // (which lists watchable candidates at every ancestor) is skipped.
        assert_eq!(loaded.entries.len(), 2);
    }

    #[test]
    fn config_global_false_excludes_global_config_reached_via_walk() {
        let dir = tempdir("no-global");
        let global = dir.join("global");
        let project = global.join("plugins").join("demo");
        fs::create_dir_all(&project).unwrap();
        write(&global.join("opencode.json"), r#"{"shell": "global-sentinel"}"#);
        let globals = GlobalDirs { config: global.clone(), home: global.join("home") };
        let enabled = loader_for(&project, &globals).load();
        assert_eq!(
            latest(&enabled.entries, |info| info.shell.clone()).as_deref(),
            Some("global-sentinel")
        );
        let disabled = loader_for(&project, &globals)
            .with_options(Options { global: Some(false), ..Options::default() })
            .load();
        assert_eq!(latest(&disabled.entries, |info| info.shell.clone()), None);
    }

    #[test]
    fn config_global_false_excludes_home_claude_and_agents_dirs() {
        let dir = tempdir("no-global-dirs");
        let global = dir.join("global");
        let home = global.join("home");
        let project = home.join("project");
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::create_dir_all(home.join(".agents")).unwrap();
        fs::create_dir_all(&project).unwrap();
        let globals = GlobalDirs { config: global.clone(), home: home.clone() };
        let enabled = discovery::discover(&project, &globals, &Options::default());
        assert!(enabled.claude.contains(&home.join(".claude")));
        assert!(enabled.agents.contains(&home.join(".agents")));
        let disabled = discovery::discover(
            &project,
            &globals,
            &Options { global: Some(false), ..Options::default() },
        );
        // Home-level ecosystem dirs are global roots however the walk reaches
        // them, so global:false excludes them (missing candidates at unrelated
        // ancestors may still be listed for watchability).
        assert!(!disabled.claude.contains(&home.join(".claude")));
        assert!(!disabled.agents.contains(&home.join(".agents")));
        assert!(!disabled.claude.iter().any(|dir| dir.starts_with(&home)));
        assert!(!disabled.agents.iter().any(|dir| dir.starts_with(&home)));
    }

    #[test]
    fn config_dedupes_global_opencode_dir_reached_via_symlink() {
        let dir = tempdir("symlink");
        let real = dir.join("real");
        let link = dir.join("link");
        let global = real.join(".opencode");
        fs::create_dir_all(&global).unwrap();
        write(&global.join("opencode.json"), r#"{"shell": "global"}"#);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let globals = GlobalDirs { config: global.clone(), home: dir.join("home") };
        for location in [&link, &real] {
            let loaded = loader_for(location, &globals).load();
            let directories: Vec<&str> = loaded
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    Entry::Directory(directory) => Some(directory.path.as_str()),
                    Entry::Document(_) => None,
                })
                .collect();
            assert_eq!(directories, vec![global.display().to_string().as_str()]);
            let shells: Vec<&str> = loaded
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    Entry::Document(document) => document.info.shell.as_deref(),
                    Entry::Directory(_) => None,
                })
                .collect();
            assert_eq!(shells, vec!["global"]);
        }
    }

    #[test]
    fn config_project_supplementary_ranks_above_direct() {
        let dir = tempdir("opencode-dir");
        let project = dir.join("project");
        fs::create_dir_all(project.join(".opencode")).unwrap();
        write(&project.join("opencode.json"), r#"{"shell": "direct"}"#);
        write(&project.join(".opencode").join("opencode.json"), r#"{"shell": "supplementary"}"#);
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&project, &globals).load();
        assert_eq!(
            latest(&loaded.entries, |info| info.shell.clone()).as_deref(),
            Some("supplementary")
        );
    }

    #[test]
    fn config_jsonc_trailing_commas_and_comments_parse() {
        let dir = tempdir("jsonc");
        write(
            &dir.join("opencode.jsonc"),
            "{\n  // trailing comma allowed\n  \"username\": \"jsonc-user\",\n}",
        );
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&dir, &globals).load();
        assert_eq!(
            latest(&loaded.entries, |info| info.username.clone()).as_deref(),
            Some("jsonc-user")
        );
    }

    #[test]
    fn config_malformed_document_skipped_not_fatal() {
        let dir = tempdir("malformed");
        write(&dir.join("opencode.json"), "{ \"username\": ");
        write(&dir.join("opencode.jsonc"), r#"{"username": "fine"}"#);
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&dir, &globals).load();
        assert_eq!(latest(&loaded.entries, |info| info.username.clone()).as_deref(), Some("fine"));
        assert!(loaded
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == DiagnosticKind::Invalid
                && diagnostic.message.contains("malformed")));
    }

    #[test]
    fn config_non_object_root_reports_invalid() {
        let dir = tempdir("null-root");
        write(&dir.join("opencode.json"), "null");
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&dir, &globals).load();
        assert!(loaded.entries.iter().all(|entry| !matches!(entry, Entry::Document(_))));
        let diagnostic = loaded
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.source.ends_with("opencode.json"))
            .unwrap();
        assert_eq!(diagnostic.path, "$");
        assert_eq!(diagnostic.kind, DiagnosticKind::Invalid);
    }

    #[test]
    fn config_rejects_document_failing_final_validation_only() {
        let dir = tempdir("final-validation");
        // `update` must be one of disable|notify|auto; the malformed value is
        // dropped with an invalid diagnostic and the document is skipped,
        // while the sibling jsonc still loads (per-document isolation).
        write(&dir.join("opencode.json"), r#"{"update": "sometimes"}"#);
        write(&dir.join("opencode.jsonc"), r#"{"username": "ok"}"#);
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&dir, &globals).load();
        assert_eq!(latest(&loaded.entries, |info| info.username.clone()).as_deref(), Some("ok"));
        let diagnostic = loaded
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.source.ends_with("opencode.json"))
            .unwrap();
        assert_eq!(diagnostic.kind, DiagnosticKind::Invalid);
        assert_eq!(diagnostic.path, "$.update");
    }

    #[test]
    fn config_env_and_file_substitution() {
        let dir = tempdir("substitute");
        write(&dir.join("token.txt"), "file\n\"token\"\n");
        write(
            &dir.join("opencode.jsonc"),
            r#"{
                // Ignored reference: {file:missing.txt}
                "username": "user-{env:OC_CORE_MISSING}",
                "mcp": {"servers": {"remote": {"type": "remote", "url": "https://example.com/mcp",
                  "headers": {"Authorization": "Bearer {env:OC_CORE_TOKEN}", "X-Token": "{file:token.txt}"}}}}
            }"#,
        );
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loader = loader_for(&dir, &globals).with_env(Arc::new(|name| {
            if name == "OC_CORE_TOKEN" {
                Some("secret".to_string())
            } else {
                None
            }
        }));
        let snapshot = loader.load();
        let info = latest(&snapshot.entries, |info| Some(info.clone())).unwrap();
        assert_eq!(info.username.as_deref(), Some("user-"));
        let server = info
            .mcp
            .as_ref()
            .and_then(|mcp| mcp.servers.as_ref())
            .and_then(|servers| servers.get("remote"))
            .cloned()
            .unwrap();
        match server {
            oc_schema::mcp::ServerConfig::Remote(config) => {
                let headers = config.headers.unwrap();
                assert_eq!(headers.get("Authorization").unwrap(), "Bearer secret");
                assert_eq!(headers.get("X-Token").unwrap(), "file\n\"token\"");
            }
            oc_schema::mcp::ServerConfig::Local(_) => panic!("expected remote server"),
        }
    }

    #[test]
    fn config_missing_file_reference_skips_document() {
        let dir = tempdir("missing-file");
        write(&dir.join("opencode.json"), r#"{"username": "{file:gone.txt}"}"#);
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&dir, &globals).load();
        assert_eq!(latest(&loaded.entries, |info| info.username.clone()), None);
        assert!(loaded
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("bad file reference")));
    }

    #[test]
    fn config_wellknown_documents_load_lowest() {
        struct Fixed;
        impl WellknownSource for Fixed {
            fn documents(&self, _env: &dyn Fn(&str) -> Option<String>) -> Vec<WellknownDocument> {
                vec![WellknownDocument {
                    source: "https://example.com".to_string(),
                    dir: PathBuf::from("/tmp"),
                    text: r#"{"username": "wellknown-{env:OC_WK_TOKEN}"}"#.to_string(),
                    env: [("OC_WK_TOKEN".to_string(), "value".to_string())].into_iter().collect(),
                }]
            }
        }
        let dir = tempdir("wellknown");
        let project = dir.join("project");
        fs::create_dir_all(&project).unwrap();
        write(&project.join("opencode.json"), r#"{"username": "project"}"#);
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&project, &globals)
            .with_wellknown(Arc::new(Fixed))
            .with_env(Arc::new(|_| None))
            .load();
        let order: Vec<&str> = loaded
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Document(document) => document.info.username.as_deref(),
                Entry::Directory(_) => None,
            })
            .collect();
        assert_eq!(order, vec!["wellknown-value", "project"]);
    }

    #[test]
    fn config_accepts_schema_key_without_persisting() {
        let dir = tempdir("schema-key");
        let file = dir.join("opencode.json");
        let contents = r#"{"$schema": "https://opencode.ai/config.json", "shell": "/bin/zsh"}"#;
        write(&file, contents);
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        let loaded = loader_for(&dir, &globals).load();
        let info = latest(&loaded.entries, |info| Some(info.clone())).unwrap();
        assert_eq!(info.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(info.schema.as_deref(), Some("https://opencode.ai/config.json"));
        // The loader never rewrites the source file.
        assert_eq!(fs::read_to_string(&file).unwrap(), contents);
    }

    #[test]
    fn config_patch_shell_replaces_and_inserts() {
        assert_eq!(
            patch_shell(
                "{\n  // keep\n  \"shell\": \"/bin/old\",\n  \"username\": \"u\"\n}",
                Some("/bin/new")
            ),
            "{\n  // keep\n  \"shell\": \"/bin/new\",\n  \"username\": \"u\"\n}"
        );
        assert_eq!(patch_shell("{}\n", Some("/bin/zsh")), "{\n\"shell\": \"/bin/zsh\"\n}\n");
        assert_eq!(patch_shell("{\n  \"shell\": \"/bin/old\"\n}", None), "{\n  \"shell\": null\n}");
    }

    struct CountingUpdates {
        count: std::sync::atomic::AtomicUsize,
    }
    impl watch::ConfigUpdated for CountingUpdates {
        fn on_config_updated(&self) {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn wait_until(deadline_ms: u64, condition: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
        while std::time::Instant::now() < deadline {
            if condition() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn config_watched_config_reloads_on_watch_events_only_on_change() {
        let dir = tempdir("watched");
        let global = dir.join("global");
        let project = dir.join("project");
        fs::create_dir_all(&global).unwrap();
        fs::create_dir_all(&project).unwrap();
        write(&project.join("opencode.json"), r#"{"shell": "one"}"#);

        let loader =
            Loader::new(project.clone(), GlobalDirs { config: global, home: dir.join("home") });
        let backend = watch::FakeBackend::new();
        let handler = Arc::new(CountingUpdates { count: std::sync::atomic::AtomicUsize::new(0) });
        let service = WatchedConfig::start(loader, backend.clone(), handler.clone(), None);
        assert_eq!(latest(&service.entries(), |info| info.shell.clone()).as_deref(), Some("one"));
        wait_until(500, || !backend.subscriptions().is_empty());
        let file = project.join("opencode.json");
        assert!(backend.subscriptions().iter().any(|target| matches!(
            target,
            watch::WatchTarget::Entries { path, names } if *path == project && names.contains(&"opencode.json".to_string())
        )));

        // Content change → one reload, one config.updated.
        write(&file, r#"{"shell": "two"}"#);
        backend.emit(&watch::FsUpdate { kind: watch::FsChangeKind::Update, path: file.clone() });
        wait_until(2_000, || {
            latest(&service.entries(), |info| info.shell.clone()).as_deref() == Some("two")
        });
        assert_eq!(handler.count.load(std::sync::atomic::Ordering::SeqCst), 1);

        // No-op write (identical entries) → reload runs, no event.
        write(&file, r#"{"shell": "two"}"#);
        backend.emit(&watch::FsUpdate { kind: watch::FsChangeKind::Update, path: file });
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(latest(&service.entries(), |info| info.shell.clone()).as_deref(), Some("two"));
        assert_eq!(
            handler.count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "deep-equal reload stays silent"
        );

        // Deleting the file drops the document (recreate coverage relies on
        // the parent entries watch keeping the file observable).
        let removed = project.join("opencode.json");
        fs::remove_file(&removed).unwrap();
        backend.emit(&watch::FsUpdate { kind: watch::FsChangeKind::Delete, path: removed });
        wait_until(2_000, || latest(&service.entries(), |info| info.shell.clone()).is_none());
        assert_eq!(handler.count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn config_update_shell_writes_global_file() {
        let dir = tempdir("update-shell");
        let globals = GlobalDirs { config: dir.join("global"), home: dir.join("home") };
        fs::create_dir_all(&globals.config).unwrap();
        fs::write(
            globals.config.join("opencode.jsonc"),
            "{\n  // untouched\n  \"username\": \"u\"\n}",
        )
        .unwrap();
        update_shell(&globals, Some("/bin/fish")).unwrap();
        let text = fs::read_to_string(globals.config.join("opencode.jsonc")).unwrap();
        assert!(text.contains("/bin/fish"));
        assert!(text.contains("untouched"));
        assert!(text.contains("\"username\""));
    }
}
