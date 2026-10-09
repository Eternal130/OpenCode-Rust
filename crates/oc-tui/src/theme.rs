//! Theme wiring: consume an [`oc_theme::ResolvedTheme`] and map its
//! tokens to flat layout style defaults (the wave-7.0 spike's flat-style
//! mapping, now fed from the real resolver).
//!
//! 7.1 scope: the builtin **v2-dark default** (`assets/v2/opencode.json`
//! dark mode), a mode switch, and a system dark/light detection stub.
//! Custom theme discovery (`~/.config/opencode/themes/*.json` + config
//! dirs) and terminal-background probing land in wave 7.2 (TODOs below).

use oc_theme::builtins::BUILTIN_V2_OPENCODE;
use oc_theme::resolve::{parse_theme_document, resolve_theme_document, ThemeDocument};
use oc_theme::types::{Mode, ResolvedTheme};
use oc_theme::Rgba;
use ratatui::style::{Color, Style};

/// A loaded theme document plus the resolved view for the active mode.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeState {
    document: ThemeDocument,
    pub mode: Mode,
    pub resolved: ResolvedTheme,
}

impl Default for ThemeState {
    fn default() -> Self {
        Self::v2_dark()
    }
}

impl ThemeState {
    /// The default theme: builtin v2 `opencode` resolved in dark mode.
    #[must_use]
    pub fn v2_dark() -> Self {
        Self::from_builtin(Mode::Dark).expect("builtin v2 theme resolves")
    }

    /// Parse + resolve the builtin v2 document in `mode`.
    #[must_use]
    pub fn from_builtin(mode: Mode) -> Option<Self> {
        Self::from_json_str(BUILTIN_V2_OPENCODE, mode, "builtin:v2/opencode")
    }

    /// Parse + resolve a theme document from raw JSON.
    #[must_use]
    pub fn from_json_str(source: &str, mode: Mode, name: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(source).ok()?;
        let document = parse_theme_document(&value, name).ok()?;
        let resolved = resolve_theme_document(&document, Some(mode)).ok()?;
        Some(Self { document, mode, resolved })
    }

    /// Switch mode (re-resolves the same document); no-op when the
    /// document lacks the mode.
    pub fn switch_mode(&mut self, mode: Mode) {
        if !self.document.supports_mode(mode) {
            return;
        }
        if let Ok(resolved) = resolve_theme_document(&self.document, Some(mode)) {
            self.mode = mode;
            self.resolved = resolved;
        }
    }

    /// The flat style defaults components paint with.
    #[must_use]
    pub fn styles(&self) -> ThemeStyles {
        ThemeStyles::from_resolved(&self.resolved)
    }
}

/// Token → style mapping (the spike's flat-style mapping): every role a
/// 7.x component paints, resolved from the theme's RGBA tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeStyles {
    pub text: Style,
    pub text_muted: Style,
    pub background: Color,
    pub background_raised: Color,
    pub action_selected: Color,
    pub action_hovered: Color,
    pub accent: Color,
    pub error: Color,
    pub border: Color,
    /// Base text painted over the base background — the whole-frame fill
    /// style (`opentui` fills the frame with the theme style).
    pub base: Style,
}

impl ThemeStyles {
    #[must_use]
    pub fn from_resolved(theme: &ResolvedTheme) -> Self {
        let tokens = &theme.tokens;
        let fg = rgba(tokens.text.base);
        let bg = rgba(tokens.background.base);
        Self {
            text: Style::new().fg(fg).bg(bg),
            text_muted: Style::new().fg(rgba(tokens.text.muted)).bg(bg),
            background: bg,
            background_raised: rgba(tokens.background.raised.base),
            action_selected: rgba(tokens.background.action.primary.selected),
            action_hovered: rgba(tokens.background.raised.high),
            accent: rgba(tokens.semantic(oc_theme::types::SemanticHue::Accent).s200),
            error: rgba(tokens.text.feedback.error.base),
            border: rgba(tokens.border.base),
            base: Style::new().fg(fg).bg(bg),
        }
    }
}

fn rgba(color: Rgba) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// System dark/light detection. The reference probes the terminal
/// background (OSC 10/11) — deferred to a later wave; `None` means
/// "unknown, keep the current mode".
//
// TODO(7.2): query the terminal background via OSC 11 and classify
// luminance (`theme/system.ts` `terminalMode()`).
#[must_use]
pub fn detect_system_mode() -> Option<Mode> {
    None
}

// TODO(7.2): custom theme discovery — `~/.config/opencode/themes/*.json`
// + cwd config dirs, registration, and hot-reload (`subscribeThemes`).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_dark_default_resolves() {
        let state = ThemeState::v2_dark();
        assert_eq!(state.mode, Mode::Dark);
        let styles = state.styles();
        assert_eq!(
            styles.background,
            Color::Rgb(
                state.resolved.tokens.background.base.r,
                state.resolved.tokens.background.base.g,
                state.resolved.tokens.background.base.b
            )
        );
    }

    #[test]
    fn mode_switch_reresolves() {
        let mut state = ThemeState::v2_dark();
        let dark_bg = state.resolved.tokens.background.base;
        state.switch_mode(Mode::Light);
        assert_eq!(state.mode, Mode::Light);
        assert_ne!(state.resolved.tokens.background.base, dark_bg, "light base differs");
        state.switch_mode(Mode::Dark);
        assert_eq!(state.resolved.tokens.background.base, dark_bg);
    }

    #[test]
    fn unknown_mode_is_ignored() {
        let mut state = ThemeState::v2_dark();
        state.switch_mode(Mode::Light);
        state.switch_mode(Mode::Dark);
        assert_eq!(state.mode, Mode::Dark);
    }

    #[test]
    fn system_detection_stub_returns_unknown() {
        assert_eq!(detect_system_mode(), None);
    }
}
