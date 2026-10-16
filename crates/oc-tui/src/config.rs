//! TUI config load (`packages/cli/src/config` + `tui/src/config`): the
//! `<config-root>/cli.json` document carries the TUI's user
//! configuration — `keybinds` (the v2 override table, see
//! [`crate::keymap::parse_keybind_overrides`]), `theme`
//! (`{ name, mode: "dark" | "light" | "system" }`), and
//! `leader.timeout`. Invalid JSON is logged and skipped (defaults
//! stand) — a bad config file never bricks the TUI.
//!
//! Reload: [`TuiConfig::reload`] re-reads the file; the run loop calls
//! it when the file's mtime changes and re-merges keybinds + theme
//! through the store (the reference watches the config dir).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::attention::{AttentionConfig, AttentionSoundName};

/// The `theme` config section (`config.theme`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeConfig {
    /// Theme registry name (`opencode`, a v1/custom name, `system`).
    pub name: Option<String>,
    /// `dark` | `light` | `system` (locks the mode; `system` follows
    /// the terminal).
    pub mode: Option<String>,
}

impl ThemeConfig {
    fn from_json(value: &Value) -> Self {
        Self {
            name: value.get("name").and_then(Value::as_str).map(str::to_owned),
            mode: value.get("mode").and_then(Value::as_str).map(str::to_owned),
        }
    }
}

/// Parse the `attention` section of the config document (v2's
/// `packages/tui/src/config/index.tsx:99` — "System notification and
/// sound settings"). Resolution applies v2's defaults
/// (`notifications ?? false, sound ?? false, volume ?? 0.4,
/// sound_pack ?? "opencode.default", sounds ?? {}`).
fn attention_from_json(value: Option<&Value>) -> AttentionConfig {
    let Some(value) = value else { return AttentionConfig::default() };
    let volume = value
        .get("volume")
        .and_then(Value::as_f64)
        .filter(|volume| volume.is_finite() && (0.0..=1.0).contains(volume))
        .unwrap_or(0.4);
    let mut sounds = HashMap::new();
    if let Some(overrides) = value.get("sounds").and_then(Value::as_object) {
        for (name, path) in overrides {
            if let (Some(name), Some(path)) = (AttentionSoundName::parse(name), path.as_str()) {
                sounds.insert(name, PathBuf::from(path));
            }
        }
    }
    AttentionConfig {
        notifications: value.get("notifications").and_then(Value::as_bool).unwrap_or(false),
        sound: value.get("sound").and_then(Value::as_bool).unwrap_or(false),
        volume,
        sound_pack: value
            .get("sound_pack")
            .and_then(Value::as_str)
            .unwrap_or("opencode.default")
            .to_owned(),
        sounds,
    }
}

/// `tabs.scope` — where the tab list persists
/// (`config/index.tsx:328`: `input.tabs?.scope ?? "cwd"`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TabsScope {
    #[default]
    Cwd,
    Global,
}

/// The resolved `tabs` config (v2 `config/index.tsx:285-329`):
/// `mode` ("on"/"off"/"auto", the legacy `enabled` boolean mapping
/// on/off), `scope` (cwd/global), and the `HERDR_ENV` auto-gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabsConfig {
    pub enabled: bool,
    pub scope: TabsScope,
}

impl Default for TabsConfig {
    fn default() -> Self {
        Self { enabled: true, scope: TabsScope::default() }
    }
}

impl TabsConfig {
    /// Resolve from the raw `tabs` object (v2 `tabsMode` chain:
    /// `mode ?? (enabled === undefined ? "auto" : enabled ? "on" : "off")`,
    /// then `enabled = mode === "on" || (mode === "auto" && HERDR_ENV !== "1")`).
    #[must_use]
    pub fn resolve(tabs: Option<&Value>) -> Self {
        let tabs = tabs.and_then(Value::as_object);
        let mode = tabs
            .and_then(|object| object.get("mode"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                tabs.and_then(|object| object.get("enabled")).and_then(Value::as_bool).map(
                    |enabled| {
                        if enabled {
                            "on".to_owned()
                        } else {
                            "off".to_owned()
                        }
                    },
                )
            })
            .unwrap_or_else(|| "auto".to_owned());
        let herdr = std::env::var("HERDR_ENV").is_ok_and(|value| value == "1");
        let enabled = mode == "on" || (mode == "auto" && !herdr);
        let scope = match tabs.and_then(|object| object.get("scope")).and_then(Value::as_str) {
            Some("global") => TabsScope::Global,
            _ => TabsScope::Cwd,
        };
        Self { enabled, scope }
    }
}

/// The TUI-relevant slice of `cli.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiConfig {
    pub keybinds: Value,
    pub theme: ThemeConfig,
    pub leader_timeout_ms: Option<u64>,
    /// The `attention` section (sounds + OS notifications).
    pub attention: AttentionConfig,
    /// `session.image_preview` (R-07): show user attachment and
    /// tool-result images in the transcript (default off — v2's falsy
    /// check at session/index.tsx:2459).
    pub session_image_preview: bool,
    /// The `tabs` section (open-tabs model): enabled defaults ON
    /// (mode "auto", `HERDR_ENV=1` turns it off), scope defaults cwd.
    pub tabs: TabsConfig,
    /// `session.sidebar` (config/index.tsx:151-152): "auto" shows the
    /// pane when space permits (>120 cols), "hide" never — default
    /// auto.
    pub session_sidebar: crate::components::sidebar::SidebarMode,
    /// `mouse` (config/index.tsx:247, default true at :311): the
    /// terminal mouse-capture gate — `false` keeps the terminal's
    /// native text selection (v2's documented opt-out; see
    /// `crate::mouse_modes`).
    pub mouse: bool,
    /// The file this was loaded from (`None` when no config file
    /// exists — all defaults).
    pub path: Option<PathBuf>,
}

// (Historical note, kept from the pre-merge layout.) Parse the `model`
// field's two shapes (string `provider/model`, or the
// `{providerID, model, variant}` object — local.tsx:177-184).
/// The derived-default exception: `mouse` defaults TRUE (v2's
/// `input.mouse ?? true`, config/index.tsx:311) — a derive would
/// give `false`, silently disabling capture for every boot without a
/// cli.json (the missing-file `load` path returns the default).
impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            keybinds: Value::Null,
            theme: ThemeConfig::default(),
            leader_timeout_ms: None,
            attention: AttentionConfig::default(),
            session_image_preview: false,
            tabs: TabsConfig::default(),
            session_sidebar: crate::components::sidebar::SidebarMode::default(),
            mouse: true,
            path: None,
        }
    }
}

impl TuiConfig {
    /// Load `<config_root>/cli.json` (missing file = defaults; invalid
    /// JSON = defaults + a logged skip).
    #[must_use]
    pub fn load(config_root: &Path) -> Self {
        let path = config_root.join("cli.json");
        let Ok(source) = std::fs::read_to_string(&path) else { return Self::default() };
        Self::parse(&source, Some(path))
    }

    /// Parse a config document (invalid = defaults + logged skip).
    #[must_use]
    pub fn parse(source: &str, path: Option<PathBuf>) -> Self {
        let value: Value = match serde_json::from_str(source) {
            Ok(value) => value,
            Err(error) => {
                crate::keymap::log_skip(&format!(
                    "config {}: {error} (defaults apply)",
                    path.as_ref().map_or("memory".to_owned(), |path| path.display().to_string())
                ));
                return Self { path, ..Self::default() };
            }
        };
        let theme = value.get("theme").map(ThemeConfig::from_json).unwrap_or_default();
        let leader_timeout_ms =
            value.pointer("/leader/timeout").and_then(Value::as_u64).filter(|ms| *ms > 0);
        let attention = attention_from_json(value.get("attention"));
        let session_image_preview =
            value.pointer("/session/image_preview").and_then(Value::as_bool).unwrap_or(false);
        let tabs = TabsConfig::resolve(value.get("tabs"));
        let session_sidebar = match value.pointer("/session/sidebar").and_then(Value::as_str) {
            Some("hide") => crate::components::sidebar::SidebarMode::Hide,
            _ => crate::components::sidebar::SidebarMode::Auto,
        };
        // `mouse` (config/index.tsx:247 + :311): absent = true (v2's
        // `input.mouse ?? true`).
        let mouse = value.get("mouse").and_then(Value::as_bool).unwrap_or(true);
        Self {
            keybinds: value.get("keybinds").cloned().unwrap_or(Value::Null),
            theme,
            leader_timeout_ms,
            attention,
            session_image_preview,
            tabs,
            session_sidebar,
            mouse,
            path,
        }
    }

    /// Re-read from disk (config reload); `None` when the file does
    /// not exist.
    #[must_use]
    pub fn reload(&self) -> Option<Self> {
        let path = self.path.as_ref()?;
        let source = std::fs::read_to_string(path).ok()?;
        Some(Self::parse(&source, Some(path.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_session_image_preview_defaulting_off() {
        // R-07: `session.image_preview` gates the transcript strip;
        // absent (v2's falsy default) or false keeps it off.
        assert!(!TuiConfig::parse("{}", None).session_image_preview);
        assert!(
            !TuiConfig::parse(r#"{"session": {"image_preview": false}}"#, None)
                .session_image_preview
        );
        assert!(
            TuiConfig::parse(r#"{"session": {"image_preview": true}}"#, None).session_image_preview
        );
    }

    /// `session.sidebar` (config/index.tsx:151-152): "auto" default,
    /// "hide" the only other value — anything else falls back to auto.
    #[test]
    fn parses_session_sidebar_mode() {
        assert_eq!(
            TuiConfig::parse("{}", None).session_sidebar,
            crate::components::sidebar::SidebarMode::Auto
        );
        assert_eq!(
            TuiConfig::parse(r#"{"session": {"sidebar": "hide"}}"#, None).session_sidebar,
            crate::components::sidebar::SidebarMode::Hide
        );
        assert_eq!(
            TuiConfig::parse(r#"{"session": {"sidebar": "auto"}}"#, None).session_sidebar,
            crate::components::sidebar::SidebarMode::Auto
        );
        assert_eq!(
            TuiConfig::parse(r#"{"session": {"sidebar": "bogus"}}"#, None).session_sidebar,
            crate::components::sidebar::SidebarMode::Auto,
            "unknown values degrade to the default"
        );
    }

    /// `mouse` (config/index.tsx:247, default `true` at :311 — v2's
    /// `input.mouse ?? true`): the terminal mouse-capture gate, the
    /// selection escape hatch auditC P1 says the port silently
    /// dropped.
    #[test]
    fn parses_the_mouse_gate() {
        assert!(TuiConfig::parse("{}", None).mouse, "absent = the true default");
        assert!(TuiConfig::parse(r#"{"mouse": true}"#, None).mouse);
        assert!(!TuiConfig::parse(r#"{"mouse": false}"#, None).mouse);
        assert!(
            TuiConfig::parse(r#"{"mouse": "bogus"}"#, None).mouse,
            "non-bool values degrade to the default"
        );
        // The missing-file path (`load` with no cli.json) returns the
        // DEFAULT — which must still carry the true default (the
        // derived-default trap the PTY proof caught).
        assert!(TuiConfig::default().mouse, "the Default impl keeps v2's true default");
    }

    #[test]
    fn parses_the_tui_slice() {
        let config = TuiConfig::parse(
            r#"{
              "theme": { "name": "catppuccin", "mode": "system" },
              "keybinds": { "app.exit": "ctrl+q", "bogus.key": "f9" },
              "leader": { "timeout": 500 },
              "mouse": false
            }"#,
            None,
        );
        assert_eq!(config.theme.name.as_deref(), Some("catppuccin"));
        assert_eq!(config.theme.mode.as_deref(), Some("system"));
        assert_eq!(config.leader_timeout_ms, Some(500));
        let overrides = crate::keymap::parse_keybind_overrides(&config.keybinds);
        assert_eq!(overrides.len(), 2, "unknown keys parse but are skipped at apply time");
    }

    /// The `attention` section: v2 defaults when absent, explicit
    /// values when set, out-of-range volume falls back, unknown sound
    /// names skip their override entry.
    #[test]
    fn parses_attention_with_v2_defaults() {
        let default = TuiConfig::parse("{}", None);
        assert_eq!(default.attention, AttentionConfig::default());
        assert!(
            !default.attention.notifications && !default.attention.sound,
            "both off by default"
        );
        assert_eq!(default.attention.volume, 0.4);
        assert_eq!(default.attention.sound_pack, "opencode.default");

        let config = TuiConfig::parse(
            r#"{
              "attention": {
                "notifications": true,
                "sound": true,
                "volume": 0.8,
                "sound_pack": "custom.pack",
                "sounds": {
                  "error": "/sounds/custom-error.mp3",
                  "bogus_name": "/sounds/ignored.mp3"
                }
              }
            }"#,
            None,
        );
        let attention = config.attention;
        assert!(attention.notifications && attention.sound);
        assert_eq!(attention.volume, 0.8);
        assert_eq!(attention.sound_pack, "custom.pack");
        assert_eq!(
            attention.sounds.get(&AttentionSoundName::Error),
            Some(&PathBuf::from("/sounds/custom-error.mp3"))
        );
        assert_eq!(attention.sounds.len(), 1, "unknown names skipped");

        // Out-of-range volume → v2's schema rejects > 1; we fall back
        // to the default.
        let clamped = TuiConfig::parse(r#"{ "attention": { "volume": 2 } }"#, None);
        assert_eq!(clamped.attention.volume, 0.4);
    }

    #[test]
    fn missing_and_invalid_files_fall_back_to_defaults() {
        assert_eq!(TuiConfig::load(Path::new("/nonexistent-opencode")), TuiConfig::default());
        let config = TuiConfig::parse("{ broken", Some(PathBuf::from("/tmp/x.json")));
        assert_eq!(
            config,
            TuiConfig { path: Some(PathBuf::from("/tmp/x.json")), ..TuiConfig::default() }
        );
    }

    /// The `tabs` section resolves exactly like v2's `tabsMode` chain
    /// (`config/index.tsx:285-329`): absent → auto (enabled unless
    /// `HERDR_ENV=1`) + cwd scope; the legacy `enabled` boolean maps
    /// on/off; explicit `mode` wins; `scope` only accepts
    /// cwd/global.
    #[test]
    fn tabs_section_resolves_the_v2_mode_chain() {
        // Serialize HERDR_ENV reads (the auto gate).
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved = std::env::var_os("HERDR_ENV");
        let restore = move || match saved {
            Some(value) => std::env::set_var("HERDR_ENV", value),
            None => std::env::remove_var("HERDR_ENV"),
        };

        let tabs = TuiConfig::parse("{}", None).tabs;
        assert!(tabs.enabled, "auto mode defaults on");
        assert_eq!(tabs.scope, TabsScope::Cwd);

        std::env::set_var("HERDR_ENV", "1");
        assert!(!TabsConfig::resolve(None).enabled, "auto mode off under HERDR_ENV=1");
        assert!(
            TabsConfig::resolve(serde_json::from_str::<Value>(r#"{"mode":"on"}"#).ok().as_ref())
                .enabled,
            "explicit on wins over HERDR_ENV"
        );
        restore();

        let config = TuiConfig::parse(r#"{ "tabs": { "mode": "off" } }"#, None);
        assert!(!config.tabs.enabled);

        // The legacy boolean maps on/off (v2: enabled ? "on" : "off").
        let config = TuiConfig::parse(r#"{ "tabs": { "enabled": false } }"#, None);
        assert!(!config.tabs.enabled);
        let config =
            TuiConfig::parse(r#"{ "tabs": { "enabled": true, "scope": "global" } }"#, None);
        assert!(config.tabs.enabled);
        assert_eq!(config.tabs.scope, TabsScope::Global);

        // An unknown scope falls back to cwd (v2's schema enum).
        let config = TuiConfig::parse(r#"{ "tabs": { "scope": "everywhere" } }"#, None);
        assert_eq!(config.tabs.scope, TabsScope::Cwd);
    }
}
