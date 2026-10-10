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

use std::path::{Path, PathBuf};

use serde_json::Value;

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

/// The TUI-relevant slice of `cli.json`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TuiConfig {
    pub keybinds: Value,
    pub theme: ThemeConfig,
    pub leader_timeout_ms: Option<u64>,
    /// `session.image_preview` (R-07): show user attachment and
    /// tool-result images in the transcript (default off — v2's falsy
    /// check at session/index.tsx:2459).
    pub session_image_preview: bool,
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
        let session_image_preview =
            value.pointer("/session/image_preview").and_then(Value::as_bool).unwrap_or(false);
        Self {
            keybinds: value.get("keybinds").cloned().unwrap_or(Value::Null),
            theme,
            leader_timeout_ms,
            session_image_preview,
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
