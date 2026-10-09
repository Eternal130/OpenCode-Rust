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
    /// 7.2 lane C (message flow): the markdown token palette
    /// (`theme.markdown.*`) and the extra roles the transcript paints.
    pub markdown: MarkdownStyles,
    /// `text.feedback.warning.base` (the reasoning-group header tint
    /// derives from it at 0.6 alpha).
    pub warning: Color,
    /// `text.action.secondary.base` (the diff-viewer source switch
    /// label; 7.2 wave 2 diff lane).
    pub action_secondary: Color,
    /// The `theme.diff.*` palette (7.2 wave 2 diff viewer).
    pub diff: DiffStyles,
    /// `text.formfield.base` (the rail add-slot's `idleNumber` tint
    /// derives from it at 0.55).
    pub formfield_base: Color,
    /// `background.action.primary.focused` — the autocomplete
    /// selected-row background (7.2 wave 2 composer).
    pub action_focused_bg: Color,
    /// `text.action.primary.focused` — the selected row's text.
    pub action_focused_fg: Color,
    /// `theme.decrease(background.raised.base)` — the composer's prompt
    /// background (`promptBg`, also the selected tab pill's lift in the
    /// horizontal bar).
    pub background_prompt: Color,
    /// Agent identity colors (`local.agent.color`: categorical hues at
    /// step 200, deduped, index-modulo).
    pub agent_colors: [Color; 5],
}

/// The `theme.diff.*` token palette — the diff viewer's gutter, sign
/// and row-background colors (`feature-plugins/system/diff-viewer.tsx`
/// consumes exactly these).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffStyles {
    /// `diff.text.added` — the `+N` count glyphs.
    pub text_added: Color,
    /// `diff.text.removed` — the `-N` count glyphs.
    pub text_removed: Color,
    /// `diff.text.hunkHeader` — the `@@ -a,b +c,d @@` rows.
    pub hunk_header: Color,
    /// `diff.background.added` — added-row text background.
    pub background_added: Color,
    /// `diff.background.removed` — removed-row text background.
    pub background_removed: Color,
    /// `diff.background.context` — the patch card + context rows.
    pub background_context: Color,
    /// `diff.highlight.added` — the ` +` sign.
    pub highlight_added: Color,
    /// `diff.highlight.removed` — the ` -` sign.
    pub highlight_removed: Color,
    /// `diff.lineNumber.text` — gutter numbers.
    pub line_number: Color,
    /// `diff.lineNumber.background.added` — added-row gutter.
    pub line_number_added: Color,
    /// `diff.lineNumber.background.removed` — removed-row gutter.
    pub line_number_removed: Color,
}

/// The markdown token palette (`theme.markdown.*`) — 7.2 lane C's
/// markdown text renderer paints every block from these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkdownStyles {
    pub text: Color,
    pub heading: Color,
    pub strong: Color,
    pub emphasis: Color,
    pub code: Color,
    pub code_block: Color,
    pub link: Color,
    pub link_text: Color,
    pub block_quote: Color,
    pub horizontal_rule: Color,
}

impl ThemeStyles {
    #[must_use]
    pub fn from_resolved(theme: &ResolvedTheme) -> Self {
        let tokens = &theme.tokens;
        let fg = rgba(tokens.text.base);
        let bg = rgba(tokens.background.base);
        let md = &tokens.markdown;
        let mut agent_colors = [bg; 5];
        for (slot, scale) in tokens.categorical.iter().take(5).enumerate() {
            agent_colors[slot] = rgba(scale.s200);
        }
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
            markdown: MarkdownStyles {
                text: rgba(md.text),
                heading: rgba(md.heading),
                strong: rgba(md.strong),
                emphasis: rgba(md.emphasis),
                code: rgba(md.code),
                code_block: rgba(md.code_block),
                link: rgba(md.link),
                link_text: rgba(md.link_text),
                block_quote: rgba(md.block_quote),
                horizontal_rule: rgba(md.horizontal_rule),
            },
            warning: rgba(tokens.text.feedback.warning.base),
            formfield_base: rgba(tokens.text.formfield.base),
            action_focused_bg: rgba(tokens.background.action.primary.focused),
            action_focused_fg: rgba(tokens.text.action.primary.focused),
            background_prompt: {
                let decreased = tokens.decrease(tokens.background.raised.base, 1.0);
                rgba(decreased)
            },
            action_secondary: rgba(tokens.text.action.secondary.base),
            diff: DiffStyles {
                text_added: rgba(tokens.diff.text.added),
                text_removed: rgba(tokens.diff.text.removed),
                hunk_header: rgba(tokens.diff.text.hunk_header),
                background_added: rgba(tokens.diff.background.added),
                background_removed: rgba(tokens.diff.background.removed),
                background_context: rgba(tokens.diff.background.context),
                highlight_added: rgba(tokens.diff.highlight.added),
                highlight_removed: rgba(tokens.diff.highlight.removed),
                line_number: rgba(tokens.diff.line_number.text),
                line_number_added: rgba(tokens.diff.line_number.background.added),
                line_number_removed: rgba(tokens.diff.line_number.background.removed),
            },
            agent_colors,
        }
    }

    /// The agent identity color (`local.agent.color`: the categorical
    /// palette indexed by the agent's position, modulo).
    #[must_use]
    pub fn agent_color(&self, index: usize) -> Color {
        self.agent_colors[index % self.agent_colors.len()]
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
