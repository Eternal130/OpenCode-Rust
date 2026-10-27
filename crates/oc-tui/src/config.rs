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
        // `attention.bell` — default false: v2 never emits BEL (P11).
        bell: value.get("bell").and_then(Value::as_bool).unwrap_or(false),
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
// The bool count mirrors v2's `cli.json` surface 1:1 (config/index.tsx);
// grouping them would diverge from the ported shape (HYGIENE-CLIPPY-TUI).
#[allow(clippy::struct_excessive_bools)]
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
    /// `session.permissions` (config/index.tsx:176 — CONFIG-T1's read
    /// side): `autoaccept` arms the permission auto-answer at boot
    /// exactly like `--auto` (context/permission.tsx:14's
    /// `args.auto ? "autoaccept" : config.data.session.permissions`);
    /// default/absent = `prompt`.
    pub session_permissions: Option<String>,
    /// `session.thinking === "show"` (index.tsx:229 — CONFIG-T3):
    /// thoughts render inline instead of the collapsed disclosure.
    pub session_thinking: bool,
    /// `session.verbosity` (index.tsx:234): `low`/`medium`/`high`.
    pub session_verbosity: Option<String>,
    /// `session.tps` (index.tsx:1892): the tok/s footer span.
    pub session_tps: bool,
    /// `session.markdown === "source"` (index.tsx:231): raw text parts.
    pub session_markdown_source: bool,
    /// `session.grouping !== "none"` (index.tsx:233): exploration
    /// grouping on (the boot seed of the runtime toggle).
    pub session_grouping: bool,
    /// The `cursor` section (`cursor.style`/`cursor.blinking` — v2's
    /// config/index.tsx:263-266; lane TAIL's DECSCUSR/OSC 12 emission).
    pub cursor: crate::terminal_proto::CursorConfig,
    /// TUI-16 — `terminal.copy` (app.tsx:546-547): `"select"` copies
    /// a finished drag to the clipboard (the default off Windows),
    /// `"manual"` leaves copying to ctrl+y/ctrl+c.
    pub terminal_copy: CopyBehavior,
    /// RD-06 — `prompt.paste !== "full"` (config.prompt?.paste, app.
    /// tsx:573): the paste-summary fold arms while compact (absent /
    /// `"compact"`); `"full"` pastes verbatim.
    pub prompt_paste_compact: bool,
    /// The file this was loaded from (`None` when no config file
    /// exists — all defaults).
    pub path: Option<PathBuf>,
}

/// `terminal.copy`'s values (dialog-config's manual/select pair).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CopyBehavior {
    /// `"select"` — copy-on-release (the non-win32 default).
    #[default]
    Select,
    /// `"manual"` — explicit copy only (the win32 default).
    Manual,
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
            session_permissions: None,
            session_thinking: false,
            session_verbosity: None,
            session_tps: false,
            session_markdown_source: false,
            session_grouping: true,
            tabs: TabsConfig::default(),
            session_sidebar: crate::components::sidebar::SidebarMode::default(),
            mouse: true,
            cursor: crate::terminal_proto::CursorConfig::default(),
            terminal_copy: CopyBehavior::Select,
            prompt_paste_compact: true,
            path: None,
        }
    }
}

impl TuiConfig {
    /// The boot permission mode (CONFIG-T1, context/permission.tsx:14):
    /// `autoaccept` when the `--auto` flag is set OR the cli.json
    /// `session.permissions` key says so; `prompt` otherwise.
    #[must_use]
    pub fn permission_autoaccept(&self, auto_flag: bool) -> bool {
        auto_flag || self.session_permissions.as_deref() == Some("autoaccept")
    }

    /// Load through oc-core's `CliConfig` service (SURF-2, F-1's
    /// registered delegation): JSONC tolerance, the v1→v2 migration
    /// and the `OPENCODE_CLI_CONFIG_CONTENT` env overlay all ride the
    /// shared reader; a missing file = defaults.
    #[must_use]
    pub fn load(roots: &oc_util::global_roots::Roots) -> Self {
        let service = oc_core::cli_config::CliConfig::open(roots);
        let info = service.get();
        Self::from_info(&info, Some(service.path().to_owned()))
    }

    /// Map the decoded `Info` onto the TUI slice (the per-key v2
    /// defaults apply where the section is absent).
    // One mapping arm per v2 config key — kept whole to mirror the ref
    // decoder (HYGIENE-CLIPPY-TUI).
    #[allow(clippy::too_many_lines)]
    #[must_use]
    pub fn from_info(info: &oc_core::cli_config::Info, path: Option<PathBuf>) -> Self {
        let theme = info.theme.as_ref().map_or_else(ThemeConfig::default, |theme| ThemeConfig {
            name: theme.name.clone(),
            mode: theme.mode.clone(),
        });
        let leader_timeout_ms =
            info.leader.as_ref().and_then(|leader| leader.timeout).filter(|ms| *ms > 0);
        let attention = info.attention.as_ref().map_or_else(AttentionConfig::default, |source| {
            AttentionConfig {
                bell: source.bell.unwrap_or(false),
                notifications: source.notifications.unwrap_or(false),
                sound: source.sound.unwrap_or(false),
                volume: source
                    .volume
                    .filter(|volume| volume.is_finite() && (0.0..=1.0).contains(volume))
                    .unwrap_or(0.4),
                sound_pack: source
                    .sound_pack
                    .clone()
                    .unwrap_or_else(|| "opencode.default".to_owned()),
                sounds: source
                    .sounds
                    .as_ref()
                    .map(|sounds| {
                        sounds
                            .iter()
                            .filter_map(|(name, value)| {
                                let name = AttentionSoundName::parse(name)?;
                                let value = value.as_ref()?;
                                Some((name, PathBuf::from(value)))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        });
        let session_image_preview =
            info.session.as_ref().and_then(|session| session.image_preview).unwrap_or(false);
        let tabs = TabsConfig {
            enabled: {
                let section = info.tabs.as_ref();
                let mode = section
                    .and_then(|tabs| tabs.mode.clone())
                    .or_else(|| {
                        section.and_then(|tabs| tabs.enabled).map(|enabled| {
                            if enabled {
                                "on".to_owned()
                            } else {
                                "off".to_owned()
                            }
                        })
                    })
                    .unwrap_or_else(|| "auto".to_owned());
                let herdr = std::env::var("HERDR_ENV").is_ok_and(|value| value == "1");
                mode == "on" || (mode == "auto" && !herdr)
            },
            scope: match info.tabs.as_ref().and_then(|tabs| tabs.scope.as_deref()) {
                Some("global") => TabsScope::Global,
                _ => TabsScope::Cwd,
            },
        };
        let session_sidebar =
            match info.session.as_ref().and_then(|session| session.sidebar.as_deref()) {
                Some("hide") => crate::components::sidebar::SidebarMode::Hide,
                _ => crate::components::sidebar::SidebarMode::Auto,
            };
        // `mouse` (config/index.tsx:247, default `true` at :311 — v2's
        // `input.mouse ?? true`).
        let mouse = info.mouse.unwrap_or(true);
        // `cursor` (:263-266, :314): style defaults `block`, blinking
        // default true.
        let cursor = info.cursor.as_ref().map_or_else(
            crate::terminal_proto::CursorConfig::default,
            |cursor| crate::terminal_proto::CursorConfig {
                style: crate::terminal_proto::CursorShape::parse(
                    cursor.style.as_deref().unwrap_or("block"),
                ),
                blinking: cursor.blinking.unwrap_or(true),
            },
        );
        Self {
            keybinds: info.keybinds.as_ref().map_or(serde_json::Value::Null, |binds| {
                serde_json::Value::Object(
                    binds.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
                )
            }),
            theme,
            leader_timeout_ms,
            attention,
            session_image_preview,
            tabs,
            session_sidebar,
            mouse,
            session_permissions: info
                .session
                .as_ref()
                .and_then(|session| session.permissions.clone()),
            session_thinking: info.session.as_ref().and_then(|session| session.thinking.as_deref())
                == Some("show"),
            session_verbosity: info.session.as_ref().and_then(|session| session.verbosity.clone()),
            session_tps: info.session.as_ref().and_then(|session| session.tps).unwrap_or(false),
            session_markdown_source: info
                .session
                .as_ref()
                .and_then(|session| session.markdown.as_deref())
                == Some("source"),
            session_grouping: info.session.as_ref().and_then(|session| session.grouping.as_deref())
                != Some("none"),
            terminal_copy: if info.terminal.as_ref().and_then(|terminal| terminal.copy.as_deref())
                == Some("manual")
            {
                CopyBehavior::Manual
            } else {
                CopyBehavior::Select
            },
            prompt_paste_compact: info.prompt.as_ref().and_then(|prompt| prompt.paste.as_deref())
                != Some("full"),
            cursor,
            path,
        }
    }

    /// Re-read from disk (config reload); `None` when the file does
    /// not exist.
    #[must_use]
    pub fn reload(&self) -> Option<Self> {
        let path = self.path.as_ref()?;
        let roots = oc_util::global_roots::resolve();
        let service = oc_core::cli_config::CliConfig::open(&roots);
        if service.path() != path {
            // The original load came from a test-injected root: read
            // THAT document (same service semantics, different roots).
            let mut test_roots = roots;
            let fallback = test_roots.config.clone();
            test_roots.config = path.parent().map_or(fallback, Path::to_path_buf);
            let service = oc_core::cli_config::CliConfig::open(&test_roots);
            if !service.path().exists() {
                return None;
            }
            let info = service.get();
            return Some(Self::from_info(&info, Some(service.path().to_owned())));
        }
        if !path.is_file() {
            return None;
        }
        let info = service.get();
        Some(Self::from_info(&info, Some(path.clone())))
    }

    /// Parse a config document (invalid = defaults + logged skip) —
    /// kept for the direct-parse tests.
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
        // `cursor` (config/index.tsx:263-266 + :314): `style` defaults
        // `block`, `blinking` defaults true (v2 emits `CSI 1 SP q`).
        let cursor = value.get("cursor").and_then(Value::as_object).map_or_else(
            crate::terminal_proto::CursorConfig::default,
            |cursor| crate::terminal_proto::CursorConfig {
                style: crate::terminal_proto::CursorShape::parse(
                    cursor.get("style").and_then(Value::as_str).unwrap_or("block"),
                ),
                blinking: cursor.get("blinking").and_then(Value::as_bool).unwrap_or(true),
            },
        );
        Self {
            keybinds: value.get("keybinds").cloned().unwrap_or(Value::Null),
            theme,
            leader_timeout_ms,
            attention,
            session_image_preview,
            tabs,
            session_sidebar,
            mouse,
            session_permissions: value
                .get("session")
                .and_then(|session| session.get("permissions"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            session_thinking: value
                .get("session")
                .and_then(|session| session.get("thinking"))
                .and_then(Value::as_str)
                == Some("show"),
            session_verbosity: value
                .get("session")
                .and_then(|session| session.get("verbosity"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            session_tps: value
                .get("session")
                .and_then(|session| session.get("tps"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            session_markdown_source: value
                .get("session")
                .and_then(|session| session.get("markdown"))
                .and_then(Value::as_str)
                == Some("source"),
            session_grouping: value
                .get("session")
                .and_then(|session| session.get("grouping"))
                .and_then(Value::as_str)
                != Some("none"),
            terminal_copy: if value
                .get("terminal")
                .and_then(|terminal| terminal.get("copy"))
                .and_then(Value::as_str)
                == Some("manual")
            {
                CopyBehavior::Manual
            } else {
                CopyBehavior::Select
            },
            prompt_paste_compact: value.pointer("/prompt/paste").and_then(Value::as_str)
                != Some("full"),
            cursor,
            path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test roots with an injected config dir (state/cache/tmp beside
    /// it — hermetic; the `CliConfig` service only touches `config` +
    /// `state/locks`).
    fn test_roots(config: &Path) -> oc_util::global_roots::Roots {
        oc_util::global_roots::Roots {
            home: config.to_path_buf(),
            data: config.join("data"),
            cache: config.join("cache"),
            config: config.to_path_buf(),
            state: config.join("state"),
            tmp: config.join("tmp"),
        }
    }

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
        // Missing file: defaults, with the service's path recorded.
        assert_eq!(
            TuiConfig::load(&test_roots(Path::new("/nonexistent-opencode"))),
            TuiConfig {
                path: Some(PathBuf::from("/nonexistent-opencode/cli.json")),
                ..TuiConfig::default()
            }
        );
        let config = TuiConfig::parse("{ broken", Some(PathBuf::from("/tmp/x.json")));
        assert_eq!(
            config,
            TuiConfig { path: Some(PathBuf::from("/tmp/x.json")), ..TuiConfig::default() }
        );
    }

    /// The F-1 delegation: JSONC comments, the v1 `tui.json` adoption
    /// and the `OPENCODE_CLI_CONFIG_CONTENT` overlay all flow through
    /// the shared `CliConfig` reader now.
    #[test]
    fn load_delegates_to_the_cli_config_service() {
        let root = std::env::temp_dir().join(format!(
            "oc-tui-config-delegation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        std::fs::create_dir_all(&root).expect("root");
        std::fs::write(
            root.join("cli.json"),
            r#"{
              // JSONC comment — the old reader choked on this
              "theme": { "name": "catppuccin", "mode": "light" },
              "leader": { "timeout": 750 },
              "session": { "sidebar": "hide", "image_preview": true },
              "mouse": false
            }"#,
        )
        .expect("seed");
        let config = TuiConfig::load(&test_roots(&root));
        assert_eq!(config.theme.name.as_deref(), Some("catppuccin"));
        assert_eq!(config.theme.mode.as_deref(), Some("light"));
        assert_eq!(config.leader_timeout_ms, Some(750));
        assert!(config.session_image_preview);
        assert_eq!(config.session_sidebar, crate::components::sidebar::SidebarMode::Hide);
        assert!(!config.mouse, "the mouse gate reads through the service");
        let _ = std::fs::remove_dir_all(&root);
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

#[cfg(test)]
mod surf2_toggle_tests {
    use crate::reactive::Runtime;
    use crate::store::{RecordingConfigSink, Store};

    /// The theme-mode pin + lock cycle and the sidebar toggle all
    /// ride the config sink (F-1's consumption halves).
    #[test]
    fn theme_mode_and_sidebar_toggles_persist_through_the_sink() {
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingConfigSink::default());
        store.set_config_sink(sink.clone());

        // switch_mode: dark → light, pinned.
        store.set_theme_mode_inputs(Some("system".to_owned()), oc_theme::types::Mode::Dark);
        store.theme_switch_mode();
        assert_eq!(store.theme().mode, oc_theme::types::Mode::Light, "toggled");
        assert_eq!(store.theme_mode_pref().as_deref(), Some("light"), "pinned");

        // lock: a pinned mode unlocks back to system (the probe says
        // dark).
        store.theme_mode_lock();
        assert_eq!(store.theme_mode_pref().as_deref(), Some("system"));
        assert_eq!(store.theme().mode, oc_theme::types::Mode::Dark, "re-resolved to the probe");

        // lock again: pins the current resolved mode.
        store.theme_mode_lock();
        assert_eq!(store.theme_mode_pref().as_deref(), Some("dark"));

        // The sidebar toggle writes session.sidebar (route-gated).
        store.set_terminal_size((160, 40));
        store.open_tab("s1", None);
        store.toggle_sidebar();
        assert_eq!(
            store.sidebar_mode(),
            crate::components::sidebar::SidebarMode::Hide,
            "visible → hide"
        );
        store.toggle_sidebar();
        assert_eq!(store.sidebar_mode(), crate::components::sidebar::SidebarMode::Auto);

        // The theme name persists on selection (themes.set).
        store.set_theme_by_name("opencode");
        let calls = sink.calls.borrow().clone();
        let expected: Vec<(&str, String)> = vec![
            ("theme.mode", "light".to_owned()),
            ("theme.mode", "system".to_owned()),
            ("theme.mode", "dark".to_owned()),
            ("session.sidebar", "hide".to_owned()),
            ("session.sidebar", "auto".to_owned()),
            ("theme.name", "opencode".to_owned()),
        ];
        assert_eq!(calls, expected, "the full write order");
    }
}
