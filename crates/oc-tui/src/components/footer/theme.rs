//! Footer theme mapping: `RunFooterTheme` / `RunTheme` from
//! `mini/theme.ts` (`map()` over a resolved theme document) — every
//! color role the footer family paints, as canonical [`CellColor`]s.
//!
//! The goldens pin the **fallback dark** mapping
//! (`RUN_THEME_FALLBACK` = the builtin v2 `opencode` document resolved
//! in dark mode), which is exactly the crate's
//! [`crate::theme::ThemeState::v2_dark`] token set.

use crate::render::CellColor;
use oc_theme::types::{ResolvedThemeTokens, SemanticHue};
use oc_theme::Rgba;

fn color(value: Rgba) -> CellColor {
    CellColor::Rgb(value.r, value.g, value.b)
}

/// The footer role palette (`RunFooterTheme`).
#[derive(Debug, Clone, PartialEq)]
pub struct FooterTheme {
    pub action_secondary_text: CellColor,
    pub action_focused_bg: CellColor,
    pub action_focused_text: CellColor,
    pub formfield_text: CellColor,
    pub formfield_focused_bg: CellColor,
    pub formfield_focused_text: CellColor,
    pub selection: CellColor,
    pub running: CellColor,
    pub question: CellColor,
    pub permission: CellColor,
    pub success: CellColor,
    pub link: CellColor,
    pub categorical: Vec<CellColor>,
    pub warning: CellColor,
    pub error: CellColor,
    pub muted: CellColor,
    pub text: CellColor,
    pub shade: CellColor,
    pub surface: CellColor,
    pub pane: CellColor,
    pub border: CellColor,
    pub line: CellColor,
    /// `theme.background` (base) — the area behind raised surfaces.
    pub background: CellColor,
}

impl FooterTheme {
    /// `map(theme, …).footer` — the role mapping the reference applies
    /// over a resolved theme document (state colors come through the
    /// expand pass: missing `$focused` etc. fall back to `.base`).
    #[must_use]
    pub fn from_resolved(theme: &ResolvedThemeTokens) -> Self {
        let tokens = theme;
        let mut categorical: Vec<CellColor> =
            tokens.categorical.iter().map(|scale| color(scale.s200)).collect();
        categorical.dedup();
        Self {
            action_secondary_text: color(tokens.text.action.secondary.base),
            action_focused_bg: color(tokens.background.action.primary.focused),
            action_focused_text: color(tokens.text.action.primary.focused),
            formfield_text: color(tokens.text.formfield.base),
            formfield_focused_bg: color(tokens.background.formfield.focused),
            formfield_focused_text: color(tokens.text.formfield.focused),
            selection: color(tokens.text.formfield.selected),
            running: color(tokens.semantic(SemanticHue::Interactive).s200),
            question: color(tokens.semantic(SemanticHue::Accent).s200),
            permission: color(tokens.semantic(SemanticHue::Accent).s200),
            success: color(tokens.text.feedback.success.base),
            link: color(tokens.markdown.link),
            categorical,
            warning: color(tokens.text.feedback.warning.base),
            error: color(tokens.text.feedback.error.base),
            muted: color(tokens.text.muted),
            text: color(tokens.text.base),
            shade: color(tokens.background.raised.base),
            surface: color(tokens.background.raised.base),
            pane: color(tokens.background.raised.high),
            border: color(tokens.border.base),
            line: color(tokens.background.raised.high),
            background: color(tokens.background.base),
        }
    }

    /// The fallback dark palette the footer goldens pin.
    #[must_use]
    pub fn fallback_dark() -> Self {
        Self::from_resolved(&crate::theme::ThemeState::v2_dark().resolved.tokens)
    }

    /// The current agent's categorical color (`agentColor`): the index
    /// of the agent among non-hidden agents, modulo the palette.
    #[must_use]
    pub fn agent_color(&self, index: usize) -> CellColor {
        self.categorical[index % self.categorical.len()]
    }
}

/// The feedback base color for a toast variant
/// (`theme.text.feedback[variant].base`).
#[must_use]
pub fn feedback_color(theme: &ResolvedThemeTokens, variant: &str) -> CellColor {
    let rgba = match variant {
        "success" => theme.text.feedback.success.base,
        "warning" => theme.text.feedback.warning.base,
        "error" => theme.text.feedback.error.base,
        _ => theme.text.feedback.info.base,
    };
    color(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_dark_matches_the_golden_palette() {
        let theme = FooterTheme::fallback_dark();
        // Pinned from footer-permission / footer-prompt-idle goldens:
        // permission △ = accent.200 (purple), agent rail = categorical[0]
        // (blue.200), focused action bg = interactive.200 (orange.200),
        // surface = raised.base, pane = raised.high, muted = neutral.400.
        assert_eq!(theme.permission, CellColor::Rgb(157, 124, 216));
        assert_eq!(theme.categorical[0], CellColor::Rgb(92, 156, 245));
        assert_eq!(theme.action_focused_bg, CellColor::Rgb(250, 178, 131));
        assert_eq!(theme.action_focused_text, CellColor::Rgb(10, 10, 10));
        assert_eq!(theme.surface, CellColor::Rgb(20, 20, 20));
        assert_eq!(theme.shade, CellColor::Rgb(20, 20, 20));
        assert_eq!(theme.pane, CellColor::Rgb(30, 30, 30));
        assert_eq!(theme.muted, CellColor::Rgb(128, 128, 128));
        assert_eq!(theme.text, CellColor::Rgb(238, 238, 238));
        assert_eq!(theme.formfield_focused_bg, CellColor::Rgb(10, 10, 10));
        assert_eq!(theme.selection, CellColor::Rgb(250, 178, 131));
        assert_eq!(theme.error, CellColor::Rgb(224, 108, 117));
    }
}
