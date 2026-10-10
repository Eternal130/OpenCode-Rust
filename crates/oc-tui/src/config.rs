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

/// The TUI-relevant slice of `cli.json`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TuiConfig {
    pub keybinds: Value,
    pub theme: ThemeConfig,
    pub leader_timeout_ms: Option<u64>,
    /// The `attention` section (sounds + OS notifications).
    pub attention: AttentionConfig,
    /// The file this was loaded from (`None` when no config file
    /// exists — all defaults).
    pub path: Option<PathBuf>,
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
        Self {
            keybinds: value.get("keybinds").cloned().unwrap_or(Value::Null),
            theme,
            leader_timeout_ms,
            attention,
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
}
