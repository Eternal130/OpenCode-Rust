//! The CLI/TUI config document (`cli.json`) — the port of v2's
//! `@opencode/cli/config` service (`packages/cli/src/config/config.ts`)
//! plus the v1→v2 migration (`migrate.ts`).
//!
//! ## What this closes (auditF §2.2)
//!
//! v2 persists the TUI's user settings — all 20 sections / ~70 leaf
//! keys of `Config.Info` (`packages/tui/src/config/index.tsx:69-249`)
//! — in `<config>/cli.json`, reads them at boot, and writes them back
//! through `Config.update` (flock + JSONC-comment-preserving targeted
//! edits). The Rust port previously only READ a 7-section subset (in
//! `oc-tui`'s loader) and never wrote the file: 13 sections / ~40 leaf
//! keys were silently ignored and every settings toggle was
//! divertable to private storage v2 cannot see.
//!
//! This module is the shared persistence half in `oc-core`:
//!
//! * [`Info`] — the full typed document with v2's decode semantics:
//!   an unknown key is ignored, but a type-invalid or
//!   constraint-violating KNOWN key fails the whole document (v2's
//!   `Schema.decodeUnknownOption` → `undefined` → defaults), never
//!   panics.
//! * [`CliConfig::get`] — boot read: run the migration, parse
//!   (`jsonc`, comments + trailing commas tolerated), decode, then
//!   overlay [`env_content`] `OPENCODE_CLI_CONFIG_CONTENT` (the
//!   desktop/mini hosts' injection seam, `config.ts:32-34,120`).
//! * [`CliConfig::update`] — write-through: flock, re-read + migrate,
//!   apply the edit to a JSON draft, deep-diff to per-key edits, and
//!   apply each edit to the RAW text via [`jsonc::modify`] so
//!   comments/formatting/unrelated keys survive (`config.ts:83-110`).
//!   The missing-file seed carries the `$schema` URL.
//!
//! ## Consumers / seam
//!
//! `oc-cli`'s `mini` command resolves its `--replay`/`--replay-limit`
//! fallbacks through here (`handlers/mini.ts:37-38`). The TUI's
//! runtime toggles and boot read delegate to this file through the
//! boundary the TUI already consumes — the `<config>/cli.json`
//! document itself; wiring the toggle handlers to `update` is the
//! TUI lane's half (registered).
//!
//! Divergence vs v2 (registered): v2's in-place v1 keybind surgery
//! (`migrate.ts:62-97`) edits duplicate JSON properties on raw text;
//! this port performs the equivalent transform at the VALUE level and
//! rewrites the document once (pretty-printed) — comments inside a
//! cli.json that still carries v1-legacy keybind names are lost on
//! that one-time migration. Value semantics are identical.

pub mod jsonc;
pub mod v1;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use oc_util::flock::FileLock;
use oc_util::global_roots::Roots;
use serde_json::Value;

/// `https://opencode.ai/v2/cli.json` (`config/schema.ts:4`).
pub const SCHEMA_URL: &str = "https://opencode.ai/v2/cli.json";

/// The config filename (`config.ts:31`).
pub const FILENAME: &str = "cli.json";

/// How long `get`/`update` wait on the config flock before failing
/// (v2's default `timeoutMs` is 5 minutes; the port bounds boot
/// blocking lower — a stuck writer degrades to defaults instead of
/// hanging the app).
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

// --- the typed document ----------------------------------------------------

/// `theme` (`config/index.tsx:72-79`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ThemeInfo {
    /// Theme name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `dark` | `light` | `system`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// `leader` (`config/index.tsx:92-99`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LeaderInfo {
    /// Milliseconds to wait for the next key after the leader
    /// (`Int > 0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
}

/// `scroll` (`config/index.tsx:100-109`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScrollInfo {
    /// Distance scrolled per input tick (`Finite ≥ 0.001`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    /// Accelerate scrolling from repeated input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceleration: Option<bool>,
}

/// `attention` (`config/index.tsx:110-121`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttentionInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifications: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<bool>,
    /// `0 ≤ volume ≤ 1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<f64>,
    #[serde(default, rename = "sound_pack", skip_serializing_if = "Option::is_none")]
    pub sound_pack: Option<String>,
    /// Sound file overrides by attention event (keys ∈ the
    /// `AttentionSoundName` literals).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sounds: Option<BTreeMap<String, Option<String>>>,
}

/// `diffs` (`config/index.tsx:122-139`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DiffsInfo {
    /// `working` | `branch` | `committed` | `turn` (`Vcs.Mode | "turn"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// `word` | `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single: Option<bool>,
    /// `auto` | `split` | `unified`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
}

/// `terminal` (`config/index.tsx:140-149`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerminalInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<bool>,
    /// `manual` | `select`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<String>,
}

/// `prompt` (`config/index.tsx:150-162`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<bool>,
    /// `compact` | `full`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paste: Option<String>,
    #[serde(default, rename = "image_preview", skip_serializing_if = "Option::is_none")]
    pub image_preview: Option<bool>,
}

/// `session` (`config/index.tsx:163-203`) — all 10 leaf keys.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    /// `auto` | `hide`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollbar: Option<bool>,
    /// `show` | `hide`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// `auto` | `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grouping: Option<String>,
    /// `low` | `medium` | `high`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<String>,
    #[serde(default, rename = "image_preview", skip_serializing_if = "Option::is_none")]
    pub image_preview: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tps: Option<bool>,
    /// `source` | `rendered`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    /// `launch` | `inherit`.
    #[serde(default, rename = "new_location", skip_serializing_if = "Option::is_none")]
    pub new_location: Option<String>,
    /// `prompt` | `autoaccept`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<String>,
}

/// `tabs` (`config/index.tsx:204-225`) — all 5 leaf keys incl. the
/// legacy `enabled` boolean.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TabsInfo {
    /// `auto` | `on` | `off`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Legacy tab toggle; `mode` wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `global` | `cwd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// `horizontal` | `vertical`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    /// `status` | `numbers`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indicators: Option<String>,
}

/// `mini` (`config/index.tsx:226-250`) — all 11 leaf keys.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MiniInfo {
    /// `show` | `hide`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// `show` | `hide`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<String>,
    /// `show` | `hide`.
    #[serde(default, rename = "shell_output", skip_serializing_if = "Option::is_none")]
    pub shell_output: Option<String>,
    /// `show` | `hide`.
    #[serde(default, rename = "turn_summary", skip_serializing_if = "Option::is_none")]
    pub turn_summary: Option<String>,
    /// `show` | `hide`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    /// `show` | `hide`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub splash: Option<String>,
    /// One of the `MiniWorkSpinner` literals.
    #[serde(default, rename = "work_spinner", skip_serializing_if = "Option::is_none")]
    pub work_spinner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono: Option<bool>,
    /// Restore session history on resume/resize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<bool>,
    /// Max messages restored during replay (`Int > 0`).
    #[serde(default, rename = "replay_limit", skip_serializing_if = "Option::is_none")]
    pub replay_limit: Option<u64>,
}

/// `debug` (`config/index.tsx:251-259`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DebugInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devtools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<bool>,
    /// `true` | `false` | `"verbose"`.
    #[serde(default, rename = "turn_tokens", skip_serializing_if = "Option::is_none")]
    pub turn_tokens: Option<Value>,
}

/// `cursor` (`config/index.tsx:60-67`).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CursorInfo {
    /// `block` | `underline` | `line` | `default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blinking: Option<bool>,
}

/// One `plugins` entry: a package directive string or
/// `{ package, options }` (`config/index.tsx:47-56`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PluginSpec {
    Directive(String),
    Entry {
        package: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        options: Option<BTreeMap<String, Value>>,
    },
}

/// The full `cli.json` document (`Config.Info`, 20 sections). Every
/// field is optional; defaults are applied by consumers (v2's
/// `resolve` chain), not by this document.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Info {
    #[serde(default, rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeInfo>,
    /// The v2 keybind override table (validated as a record of
    /// binding values; parsed/merged by the keymap layer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keybinds: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<PluginSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<LeaderInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll: Option<ScrollInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<AttentionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffs: Option<DiffsInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<TerminalInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tabs: Option<TabsInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mini: Option<MiniInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<BTreeMap<String, bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animations: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<CursorInfo>,
}

/// The `AttentionSoundName` literals (`config/index.tsx:18-25`).
pub const ATTENTION_SOUND_NAMES: &[&str] =
    &["default", "question", "permission", "error", "done", "subagent_done"];

/// The `MiniWorkSpinner` literals (`config/index.tsx:29-44`).
pub const MINI_WORK_SPINNERS: &[&str] = &[
    "block-soft-slide",
    "block-soft-sweep",
    "block-low-comet",
    "block-low-duet",
    "block-shuttle",
    "block-bridge",
    "block-squeeze",
    "small-toggle",
    "square-toggle",
    "grow-shrink",
    "quadrant-orbit",
    "crosshatch",
    "density-wave",
    "seed",
];

impl Info {
    /// Decode with v2's semantics: `None` when the value is not a
    /// document at all, and `None` when any PRESENT known key violates
    /// the schema (v2's whole-document `Schema.decodeUnknownOption`
    /// failure → `undefined`). Unknown keys are ignored (effect's
    /// struct default), preserving forward compatibility.
    #[must_use]
    pub fn decode(value: &Value) -> Option<Self> {
        let info: Self = serde_json::from_value(value.clone()).ok()?;
        info.validate().then_some(info)
    }

    /// The schema constraints serde cannot express.
    fn validate(&self) -> bool {
        one_of(self.theme.as_ref().and_then(|t| t.mode.as_deref()), &["system", "dark", "light"])
            && self.leader.as_ref().is_none_or(|l| l.timeout.is_none_or(|t| t > 0))
            && self
                .scroll
                .as_ref()
                .is_none_or(|s| s.speed.is_none_or(|speed| speed.is_finite() && speed >= 0.001))
            && self.attention.as_ref().is_none_or(|a| {
                a.volume.is_none_or(|volume| (0.0..=1.0).contains(&volume))
                    && a.sounds.as_ref().is_none_or(|sounds| {
                        sounds.keys().all(|name| ATTENTION_SOUND_NAMES.contains(&name.as_str()))
                    })
            })
            && one_of(
                self.diffs.as_ref().and_then(|d| d.source.as_deref()),
                &["working", "branch", "committed", "turn"],
            )
            && one_of(self.diffs.as_ref().and_then(|d| d.wrap.as_deref()), &["word", "none"])
            && one_of(
                self.diffs.as_ref().and_then(|d| d.view.as_deref()),
                &["auto", "split", "unified"],
            )
            && one_of(self.terminal.as_ref().and_then(|t| t.copy.as_deref()), &["manual", "select"])
            && one_of(self.prompt.as_ref().and_then(|p| p.paste.as_deref()), &["compact", "full"])
            && one_of(self.session.as_ref().and_then(|s| s.sidebar.as_deref()), &["auto", "hide"])
            && one_of(self.session.as_ref().and_then(|s| s.thinking.as_deref()), &["show", "hide"])
            && one_of(self.session.as_ref().and_then(|s| s.grouping.as_deref()), &["auto", "none"])
            && one_of(
                self.session.as_ref().and_then(|s| s.verbosity.as_deref()),
                &["low", "medium", "high"],
            )
            && one_of(
                self.session.as_ref().and_then(|s| s.markdown.as_deref()),
                &["source", "rendered"],
            )
            && one_of(
                self.session.as_ref().and_then(|s| s.new_location.as_deref()),
                &["launch", "inherit"],
            )
            && one_of(
                self.session.as_ref().and_then(|s| s.permissions.as_deref()),
                &["prompt", "autoaccept"],
            )
            && one_of(self.tabs.as_ref().and_then(|t| t.mode.as_deref()), &["auto", "on", "off"])
            && one_of(self.tabs.as_ref().and_then(|t| t.scope.as_deref()), &["global", "cwd"])
            && one_of(
                self.tabs.as_ref().and_then(|t| t.layout.as_deref()),
                &["horizontal", "vertical"],
            )
            && one_of(
                self.tabs.as_ref().and_then(|t| t.indicators.as_deref()),
                &["status", "numbers"],
            )
            && self.mini.as_ref().is_none_or(|m| {
                one_of(m.thinking.as_deref(), &["show", "hide"])
                    && one_of(m.tools.as_deref(), &["show", "hide"])
                    && one_of(m.shell_output.as_deref(), &["show", "hide"])
                    && one_of(m.turn_summary.as_deref(), &["show", "hide"])
                    && one_of(m.footer.as_deref(), &["show", "hide"])
                    && one_of(m.splash.as_deref(), &["show", "hide"])
                    && one_of(m.work_spinner.as_deref(), MINI_WORK_SPINNERS)
                    && m.replay_limit.is_none_or(|limit| limit > 0)
            })
            && self.debug.as_ref().is_none_or(|d| {
                d.turn_tokens
                    .as_ref()
                    .is_none_or(|v| v.is_boolean() || v.as_str() == Some("verbose"))
            })
            && one_of(
                self.cursor.as_ref().and_then(|c| c.style.as_deref()),
                &["block", "underline", "line", "default"],
            )
    }

    /// `normalizeLegacyTabs` (`config/schema.ts:12-18`): the legacy
    /// `tabs.enabled` boolean folds into `mode` (`on`/`off`) when
    /// `mode` is absent, then drops itself.
    #[must_use]
    pub fn normalize_legacy_tabs(&self) -> Self {
        let Some(tabs) = self.tabs.as_ref() else { return self.clone() };
        let Some(enabled) = tabs.enabled else { return self.clone() };
        let mut next = self.clone();
        let tabs = next.tabs.as_mut().expect("checked above");
        if tabs.mode.is_none() {
            tabs.mode = Some(if enabled { "on" } else { "off" }.to_owned());
        }
        tabs.enabled = None;
        next
    }
}

fn one_of(value: Option<&str>, allowed: &[&str]) -> bool {
    value.is_none_or(|value| allowed.contains(&value))
}

// --- the service -----------------------------------------------------------

/// `OPENCODE_CLI_CONFIG_CONTENT` (`config.ts:32-34`): an in-process
/// config overlay the desktop/mini hosts inject. Parsed with the same
/// JSONC tolerance; invalid content is ignored (v2's decode →
/// `undefined`).
#[must_use]
pub fn env_content() -> Option<Value> {
    let raw = std::env::var("OPENCODE_CLI_CONFIG_CONTENT").ok()?;
    let value = jsonc::parse_jsonc(&raw).ok()?;
    value.is_object().then_some(value)
}

/// Deep-merge `overlay` over `base` (`mergeRecords`, `config.ts:130-138`):
/// objects merge recursively, everything else replaces. Both inputs
/// pass through `normalizeLegacyTabs` first (`merge`, `config.ts:118-128`).
#[must_use]
pub fn merge(base: &Value, overlay: &Value) -> Value {
    let normalized = |value: &Value| -> Value {
        Info::decode(value)
            .and_then(|info| serde_json::to_value(info.normalize_legacy_tabs()).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    };
    let mut result = normalized(base);
    if !matches!(&result, Value::Object(_)) {
        result = serde_json::json!({});
    }
    merge_records(&mut result, &normalized(overlay));
    result
}

fn merge_records(base: &mut Value, overlay: &Value) {
    let Some(overlay) = overlay.as_object() else { return };
    let Value::Object(base) = base else { return };
    for (key, value) in overlay {
        match (base.get_mut(key), value) {
            (Some(Value::Object(_)), Value::Object(_)) => {
                let mut base_inner = base.get(key).cloned().expect("checked above");
                merge_records(&mut base_inner, &value.clone());
                base.insert(key.clone(), base_inner);
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

/// The `cli.json` service bound to one root set: `<config>/cli.json`
/// with the flock under `<state>/locks`.
#[derive(Debug, Clone)]
pub struct CliConfig {
    path: PathBuf,
    lock_path: PathBuf,
    legacy_config: PathBuf,
    legacy_state: PathBuf,
}

impl CliConfig {
    /// Open the service for the given roots (`config.ts:31`:
    /// `path.join(global.config, "cli.json")`; the flock lives under
    /// `global.state/locks`, `config.ts:57`).
    #[must_use]
    pub fn open(roots: &Roots) -> Self {
        Self {
            path: roots.config.join(FILENAME),
            lock_path: roots.locks().join("cli-json.lock"),
            legacy_config: roots.config.join("tui.json"),
            legacy_state: roots.state.join("kv.json"),
        }
    }

    /// The document path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> Result<FileLock, String> {
        FileLock::exclusive(&self.lock_path, LOCK_TIMEOUT)
            .map_err(|error| format!("lock {}: {error}", self.lock_path.display()))
    }

    /// Read the effective config: migrate, parse + decode the file,
    /// then overlay [`env_content`]. Mirrors `get` (`config.ts:68-81`):
    /// migration failures log and degrade (a bad config file never
    /// bricks the app); the result is decoded `Info` (defaults when
    /// the file is absent or invalid).
    #[must_use]
    pub fn get(&self) -> Info {
        let _lock = self.lock().ok();
        if let Err(error) = self.run_migration() {
            log_line(&format!("opencode: cli.json migration skipped: {error}"));
        }
        self.load()
    }

    /// The plain file+overlay load (no migration).
    fn load(&self) -> Info {
        let file_value = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| jsonc::parse_jsonc(&text).ok())
            .filter(serde_json::Value::is_object);
        let merged =
            merge(&file_value.unwrap_or(Value::Null), &env_content().unwrap_or(Value::Null));
        Info::decode(&merged).unwrap_or_default()
    }

    /// Write-through update (`update`, `config.ts:83-110`): flock,
    /// migrate, load the current effective document, apply `edit` to
    /// the JSON draft, diff, and apply the per-key edits to the raw
    /// text (comments preserved). A no-change edit writes nothing.
    /// The missing-file seed carries the `$schema` URL.
    ///
    /// # Errors
    ///
    /// Lock/IO failures and invalid results (the edited document must
    /// still decode — v2's "Invalid CLI config update").
    pub fn update(&self, edit: impl FnOnce(&mut Value)) -> Result<Info, String> {
        let _lock = self.lock()?;
        self.run_migration()?;
        let current = self.load_value();
        let mut draft = current.clone();
        edit(&mut draft);
        let edits = diff(&current, &draft, &mut Vec::new());
        if edits.is_empty() {
            return Ok(Info::decode(&current).unwrap_or_default());
        }
        let text = std::fs::read_to_string(&self.path)
            .unwrap_or_else(|_| format!("{{\n  \"$schema\": \"{SCHEMA_URL}\"\n}}"));
        let mut updated = text;
        for (path, value) in edits {
            let segments: Vec<&str> = path.iter().map(String::as_str).collect();
            updated = jsonc::modify(&updated, &segments, value.as_ref())?;
        }
        let parsed =
            jsonc::parse_jsonc(&updated).map_err(|_| "Invalid CLI config update".to_owned())?;
        let decoded = Info::decode(&parsed).ok_or("Invalid CLI config update")?;
        let body = if updated.ends_with('\n') { updated } else { updated + "\n" };
        write_atomic(&self.path, &body)?;
        let effective = merge(&parsed, &env_content().unwrap_or(Value::Null));
        Ok(Info::decode(&effective).unwrap_or(decoded))
    }

    /// The raw merged JSON document (file + env overlay), before decode.
    fn load_value(&self) -> Value {
        let file_value = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| jsonc::parse_jsonc(&text).ok())
            .filter(serde_json::Value::is_object);
        merge(&file_value.unwrap_or(Value::Null), &env_content().unwrap_or(Value::Null))
    }

    /// The raw merged JSON document (file + env overlay), before
    /// decode — consumers that need sections `Info` drops (or the
    /// literal keybind map) read this.
    #[must_use]
    pub fn document(&self) -> Value {
        self.load_value()
    }

    /// The migration pass (`migrate.ts`): fix an existing cli.json's
    /// legacy keys in place (`terminal.copy_on_select`, v1 keybind
    /// names), or adopt v1 `tui.json` + `kv.json` when cli.json does
    /// not exist. Failures are returned (the caller logs); nothing is
    /// written when nothing changes.
    fn run_migration(&self) -> Result<(), String> {
        if self.path.exists() {
            let text = std::fs::read_to_string(&self.path)
                .map_err(|error| format!("read {}: {error}", self.path.display()))?;
            let value = jsonc::parse_jsonc(&text)
                .map_err(|error| format!("parse {}: {error}", self.path.display()))?;
            let Some(object) = value.as_object() else { return Ok(()) };
            if !object.contains_key("terminal") && !object.contains_key("keybinds") {
                return Ok(());
            }
            let Some(migrated) = v1::fix_legacy_document(object) else { return Ok(()) };
            let body = serde_json::to_string_pretty(&migrated).map_err(|e| e.to_string())? + "\n";
            write_atomic(&self.path, &body)?;
            return Ok(());
        }
        // No cli.json: adopt v1's `tui.json` + `kv.json`.
        let legacy_tui = read_json_file(&self.legacy_config);
        let kv = read_json_file(&self.legacy_state);
        if legacy_tui.is_none() && kv.is_none() {
            return Ok(());
        }
        let values = v1::migrate_v1(legacy_tui.as_ref(), kv.as_ref());
        if values.as_object().is_none_or(serde_json::Map::is_empty) {
            return Ok(());
        }
        let mut document = serde_json::json!({ "$schema": SCHEMA_URL });
        if let (Some(target), Some(source)) = (document.as_object_mut(), values.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        let body = serde_json::to_string_pretty(&document).map_err(|e| e.to_string())? + "\n";
        write_atomic(&self.path, &body)?;
        log_line(&format!(
            "opencode: migrated cli config from {} + {}",
            self.legacy_config.display(),
            self.legacy_state.display()
        ));
        Ok(())
    }
}

fn log_line(message: &str) {
    eprintln!("{message}");
}

fn read_json_file(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    jsonc::parse_jsonc(&text).ok().filter(serde_json::Value::is_object)
}

/// Atomic 0600 write (temp + rename — `config.ts:42-47`).
fn write_atomic(path: &Path, body: &str) -> Result<(), String> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let temp = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    #[cfg(unix)]
    let write = || -> std::io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(body.as_bytes())
    };
    #[cfg(not(unix))]
    let write = || -> std::io::Result<()> { std::fs::write(&temp, body) };
    write()
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|error| format!("write {}: {error}", path.display()))
}

/// The deep diff (`changes`, `config.ts:151-168`): one edit per
/// changed leaf, deletions as `None`.
fn diff(
    before: &Value,
    after: &Value,
    path: &mut Vec<String>,
) -> Vec<(Vec<String>, Option<Value>)> {
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            let mut edits = Vec::new();
            let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                match (before.get(key), after.get(key)) {
                    (None, Some(value)) => {
                        path.push(key.clone());
                        edits.push((path.clone(), Some(value.clone())));
                        path.pop();
                    }
                    (Some(_), None) => {
                        path.push(key.clone());
                        edits.push((path.clone(), None));
                        path.pop();
                    }
                    (Some(before_value), Some(after_value)) => {
                        path.push(key.clone());
                        edits.extend(diff(before_value, after_value, path));
                        path.pop();
                    }
                    (None, None) => {}
                }
            }
            edits
        }
        _ if before == after => Vec::new(),
        _ => vec![(path.clone(), Some(after.clone()))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn roots(dir: &Path) -> Roots {
        Roots {
            home: dir.to_path_buf(),
            data: dir.join("data"),
            cache: dir.join("cache"),
            config: dir.join("config"),
            state: dir.join("state"),
            tmp: dir.join("tmp"),
        }
    }

    /// OPENCODE_CLI_CONFIG_CONTENT is process-global: every test that
    /// reads through [`CliConfig`] while another sets the variable must
    /// hold this lock.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn scratch(name: &str) -> (tempfile::TempDir, Roots) {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("scratch");
        std::fs::create_dir_all(dir.path().join("config")).expect("config dir");
        let _ = name;
        let root_path = dir.path().to_path_buf();
        (dir, roots(&root_path))
    }

    #[test]
    fn decodes_every_section_and_defaults_on_schema_violation() {
        let document = json!({
            "$schema": SCHEMA_URL,
            "theme": { "name": "catppuccin", "mode": "system" },
            "keybinds": { "app.exit": "ctrl+q" },
            "plugins": ["oh-my-opencode", { "package": "p", "options": { "a": 1 } }],
            "leader": { "timeout": 500 },
            "scroll": { "speed": 2.5, "acceleration": false },
            "attention": { "notifications": true, "sound": true, "volume": 0.8,
                           "sound_pack": "custom", "sounds": { "error": "/e.mp3" } },
            "diffs": { "source": "turn", "wrap": "word", "tree": true, "single": false, "view": "split" },
            "terminal": { "title": false, "copy": "select" },
            "prompt": { "editor": true, "paste": "compact", "image_preview": true },
            "session": { "sidebar": "hide", "scrollbar": true, "thinking": "show",
                         "grouping": "none", "verbosity": "high", "image_preview": true,
                         "tps": true, "markdown": "rendered", "new_location": "inherit",
                         "permissions": "autoaccept" },
            "tabs": { "mode": "on", "enabled": true, "scope": "global",
                      "layout": "vertical", "indicators": "numbers" },
            "mini": { "thinking": "hide", "tools": "show", "shell_output": "hide",
                      "turn_summary": "show", "footer": "hide", "splash": "hide",
                      "work_spinner": "seed", "mono": true, "replay": false,
                      "replay_limit": 50 },
            "debug": { "devtools": true, "timing": true, "turn_tokens": "verbose" },
            "experimental": { "flag.x": true },
            "animations": false,
            "mouse": false,
            "cursor": { "style": "underline", "blinking": true },
            "unknown-section": { "kept": "for-forward-compat" }
        });
        let info = Info::decode(&document).expect("all 20 sections decode");
        assert_eq!(info.theme.as_ref().and_then(|t| t.mode.as_deref()), Some("system"));
        assert_eq!(
            info.session.as_ref().and_then(|s| s.permissions.as_deref()),
            Some("autoaccept")
        );
        assert_eq!(info.mini.as_ref().and_then(|m| m.work_spinner.as_deref()), Some("seed"));
        assert_eq!(info.mini.as_ref().unwrap().replay_limit, Some(50));
        assert_eq!(info.plugins.as_ref().map(Vec::len), Some(2));
        assert_eq!(info.experimental.as_ref().and_then(|e| e.get("flag.x").copied()), Some(true));

        // A type-invalid or out-of-enum value fails the WHOLE document
        // (v2's decode → undefined → defaults).
        assert!(Info::decode(&json!({ "theme": { "mode": "blue" } })).is_none());
        assert!(Info::decode(&json!({ "theme": 5 })).is_none());
        assert!(Info::decode(&json!({ "leader": { "timeout": 0 } })).is_none());
        assert!(Info::decode(&json!({ "attention": { "volume": 2 } })).is_none());
        assert!(Info::decode(&json!({ "attention": { "sounds": { "nope": "x" } } })).is_none());
        assert!(Info::decode(&json!({ "mini": { "replay_limit": 0 } })).is_none());
        assert!(Info::decode(&json!({ "mouse": "off" })).is_none());
        assert!(Info::decode(&json!(5)).is_none(), "non-documents decode to None");
    }

    #[test]
    fn legacy_tabs_enabled_folds_into_mode() {
        let info = Info::decode(&json!({ "tabs": { "enabled": false, "scope": "global" } }))
            .expect("decodes")
            .normalize_legacy_tabs();
        assert_eq!(info.tabs.as_ref().and_then(|t| t.mode.as_deref()), Some("off"));
        assert_eq!(info.tabs.as_ref().unwrap().enabled, None, "legacy boolean drops");
        assert_eq!(info.tabs.as_ref().and_then(|t| t.scope.as_deref()), Some("global"));
    }

    #[test]
    fn update_writes_through_preserving_comments_and_returns_the_document() {
        let (_dir, roots) = scratch("update");
        std::fs::write(
            roots.config.join(FILENAME),
            "{\n  // my notes\n  \"mouse\": true,\n  \"theme\": { \"name\": \"opencode\" }\n}\n",
        )
        .expect("seed");
        let config = CliConfig::open(&roots);

        let info = config
            .update(|draft| {
                draft["mouse"] = json!(false);
                draft["theme"]["mode"] = json!("dark");
            })
            .expect("update");
        assert_eq!(info.mouse, Some(false));
        assert_eq!(info.theme.as_ref().and_then(|t| t.mode.as_deref()), Some("dark"));

        let text = std::fs::read_to_string(roots.config.join(FILENAME)).expect("read back");
        assert!(text.contains("// my notes"), "comments survive: {text}");
        assert!(text.contains("\"mouse\": false"));
        // Boot precedence: the next get() reads the written values.
        assert_eq!(config.get().mouse, Some(false));
    }

    #[test]
    fn update_seeds_the_schema_url_on_a_missing_file() {
        let (_dir, roots) = scratch("seed");
        let config = CliConfig::open(&roots);
        let info = config
            .update(|draft| {
                draft["animations"] = json!(false);
            })
            .expect("update");
        assert_eq!(info.schema.as_deref(), Some(SCHEMA_URL));
        assert_eq!(info.animations, Some(false));
        let text = std::fs::read_to_string(roots.config.join(FILENAME)).expect("read");
        assert!(text.contains(SCHEMA_URL));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn update_deletes_keys_and_no_change_writes_nothing() {
        let (_dir, roots) = scratch("delete");
        std::fs::write(roots.config.join(FILENAME), "{\n  \"mouse\": false\n}\n").expect("seed");
        let config = CliConfig::open(&roots);
        let before = std::fs::metadata(config.path()).expect("stat").modified().expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(10));

        config.update(|_| {}).expect("no-change update");
        assert_eq!(
            std::fs::metadata(config.path()).expect("stat").modified().expect("mtime"),
            before,
            "a no-op edit does not rewrite the file"
        );

        config
            .update(|draft| {
                draft.as_object_mut().unwrap().remove("mouse");
            })
            .expect("delete");
        let value = jsonc::parse_jsonc(&std::fs::read_to_string(config.path()).unwrap()).unwrap();
        assert!(value.get("mouse").is_none());
    }

    #[test]
    fn get_falls_back_to_defaults_on_invalid_files() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_dir, roots) = scratch("invalid");
        std::fs::write(roots.config.join(FILENAME), "{ broken").expect("seed");
        let config = CliConfig::open(&roots);
        assert_eq!(config.get(), Info::default());
    }

    #[test]
    fn env_content_overlays_the_file_everywhere() {
        // OPENCODE_CLI_CONFIG_CONTENT is process-global: serialize the
        // tests that touch it.
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_dir, roots) = scratch("env");
        std::fs::write(
            roots.config.join(FILENAME),
            "{\n  \"theme\": { \"name\": \"file-theme\", \"mode\": \"light\" },\n  \"mouse\": true\n}\n",
        )
        .expect("seed");
        let config = CliConfig::open(&roots);

        struct EnvGuard;
        impl EnvGuard {
            fn set(value: &str) -> Self {
                std::env::set_var("OPENCODE_CLI_CONFIG_CONTENT", value);
                EnvGuard
            }
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                std::env::remove_var("OPENCODE_CLI_CONFIG_CONTENT");
            }
        }

        let _env = EnvGuard::set(r#"{ "theme": { "mode": "dark" }, "mouse": false }"#);
        let info = config.get();
        // Overlay wins per-leaf; unmentioned leaves survive.
        assert_eq!(info.theme.as_ref().and_then(|t| t.mode.as_deref()), Some("dark"));
        assert_eq!(info.theme.as_ref().and_then(|t| t.name.as_deref()), Some("file-theme"));
        assert_eq!(info.mouse, Some(false));

        // The overlay applies to update() results too (v2's
        // `merge(config, content)`), but writes only persist the file
        // side.
        let info = config
            .update(|draft| {
                draft["animations"] = json!(true);
            })
            .expect("update");
        assert_eq!(info.mouse, Some(false), "overlay still wins after a write");
        let text = std::fs::read_to_string(config.path()).expect("read");
        assert!(text.contains("\"mouse\": true"), "the file's own value is untouched");
        drop(_env);

        // Invalid env content is ignored.
        let _env = EnvGuard::set("{ not json");
        assert_eq!(config.get().theme.as_ref().and_then(|t| t.name.as_deref()), Some("file-theme"));
    }

    #[test]
    fn diff_produces_leaf_edits_and_deletions() {
        let before = json!({ "a": 1, "b": { "c": 2, "d": 3 }, "e": true });
        let after = json!({ "a": 2, "b": { "c": 2, "f": 4 }, "e": true });
        let edits = diff(&before, &after, &mut Vec::new());
        let paths: Vec<(Vec<String>, bool)> =
            edits.into_iter().map(|(path, value)| (path, value.is_some())).collect();
        assert_eq!(
            paths,
            vec![
                (vec!["a".to_owned()], true),
                (vec!["b".to_owned(), "d".to_owned()], false),
                (vec!["b".to_owned(), "f".to_owned()], true),
            ]
        );
    }
}
