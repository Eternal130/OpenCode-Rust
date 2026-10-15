//! Capture-name → theme style resolution, mirroring how opentui consumes
//! `generateSyntax` output: the flattened scope → style map from
//! [`oc_theme::convert_theme_to_styles`] is looked up by the tree-sitter
//! capture name with **exact match first, then longest-suffix match** —
//! `variable.parameter` resolves as `variable.parameter`, then
//! `variable` (stripping trailing dotted segments), before falling back
//! to the base style.

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_theme::{Rgba, SyntaxStyleDef};
use ratatui::style::{Color, Modifier, Style};

/// The flattened scope → style map (`convertThemeToStyles` output),
/// shared cheaply across renders.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyntaxStyles(Arc<BTreeMap<String, SyntaxStyleDef>>);

impl SyntaxStyles {
    /// Flatten a theme's generated rules (`generate_syntax` →
    /// `convert_theme_to_styles` — oc-theme/src/syntax.rs:248).
    #[must_use]
    pub fn from_rules(rules: &[oc_theme::SyntaxRule]) -> Self {
        Self(Arc::new(oc_theme::convert_theme_to_styles(rules)))
    }

    /// Wrap an already-flattened map.
    #[must_use]
    pub fn from_map(map: BTreeMap<String, SyntaxStyleDef>) -> Self {
        Self(Arc::new(map))
    }

    /// An empty map (every capture falls back to the base style).
    #[must_use]
    pub fn empty() -> Self {
        Self(Arc::new(BTreeMap::new()))
    }

    /// Iterate the flattened (scope → definition) map — the bridge
    /// the opentui-core markdown pipeline consumes (the retirement
    /// lane re-homes transcript markdown onto opentui's
    /// `MarkdownRenderable`, which resolves captures through its own
    /// `SyntaxStyle` built from the SAME generated rules).
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SyntaxStyleDef)> {
        self.0.iter()
    }

    fn get(&self, scope: &str) -> Option<&SyntaxStyleDef> {
        self.0.get(scope)
    }
}

/// Capture names that predate the scope vocabulary the theme generates;
/// normalized to their modern equivalents before matching (upstream
/// parser-repo queries still emit `@escape` where nvim emits
/// `@string.escape`).
const CAPTURE_ALIASES: &[(&str, &str)] = &[("escape", "string.escape")];

/// Resolve a capture name through `styles`: exact scope, then
/// progressively shorter dotted prefixes (the *longest suffix* of the
/// lookup chain — `variable.parameter` → `variable`), then `None`.
fn resolve_def<'a>(styles: &'a SyntaxStyles, capture: &str) -> Option<&'a SyntaxStyleDef> {
    // Exact (after alias normalization).
    for (from, to) in CAPTURE_ALIASES {
        if capture == *from {
            if let Some(def) = styles.get(to) {
                return Some(def);
            }
        }
    }
    if let Some(def) = styles.get(capture) {
        return Some(def);
    }
    // Longest-suffix fallback: strip the last dotted segment until a
    // scope matches (`variable.parameter` → `variable`).
    let mut scope = capture;
    while let Some((head, _)) = scope.rsplit_once('.') {
        scope = head;
        if let Some(def) = styles.get(scope) {
            return Some(def);
        }
    }
    None
}

/// Apply a [`SyntaxStyleDef`] on top of the base style (fg/bg overlay,
/// attributes added — the same overlay order opentui paints with).
fn apply_def(base: Style, def: &SyntaxStyleDef) -> Style {
    let mut style = base;
    if let Some(fg) = def.fg {
        style = style.fg(rgba_to_color(fg));
    }
    if let Some(bg) = def.bg {
        style = style.bg(rgba_to_color(bg));
    }
    let mut modifier = Modifier::empty();
    if def.bold.unwrap_or(false) {
        modifier |= Modifier::BOLD;
    }
    if def.italic.unwrap_or(false) {
        modifier |= Modifier::ITALIC;
    }
    if def.underline.unwrap_or(false) {
        modifier |= Modifier::UNDERLINED;
    }
    if def.dim.unwrap_or(false) {
        modifier |= Modifier::DIM;
    }
    if !modifier.is_empty() {
        style = style.add_modifier(modifier);
    }
    style
}

fn rgba_to_color(color: Rgba) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

/// Resolve a capture name to a paint style: the matched scope's style
/// overlaid on `base`, or `base` itself when nothing matches.
#[must_use]
pub fn resolve(styles: &SyntaxStyles, capture: &str, base: Style) -> Style {
    resolve_def(styles, capture).map_or(base, |def| apply_def(base, def))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oc_theme::color::Rgba;

    fn def(fg: Rgba, italic: bool) -> SyntaxStyleDef {
        SyntaxStyleDef {
            fg: Some(fg),
            bg: None,
            bold: None,
            italic: Some(italic),
            underline: None,
            dim: None,
        }
    }

    fn map() -> SyntaxStyles {
        let red = Rgba::from_ints(255, 0, 0, 255);
        let blue = Rgba::from_ints(0, 0, 255, 255);
        SyntaxStyles::from_map(BTreeMap::from([
            ("comment".into(), def(red, true)),
            ("variable.parameter".into(), def(blue, false)),
            ("variable".into(), def(red, false)),
        ]))
    }

    #[test]
    fn exact_match_wins() {
        let styles = map();
        let base = Style::new();
        let exact = resolve(&styles, "variable.parameter", base);
        let fg = exact.fg.expect("styled");
        assert_eq!(fg, Color::Rgb(0, 0, 255), "exact variable.parameter wins");
        assert!(!exact.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn longest_suffix_matches() {
        let styles = map();
        let style = resolve(&styles, "variable.other.member", Style::new());
        // variable.other.member → variable.other → variable.
        assert_eq!(style.fg, Some(Color::Rgb(255, 0, 0)));
    }

    #[test]
    fn unknown_capture_falls_back_to_base() {
        let styles = map();
        let base = Style::new().fg(Color::Rgb(9, 9, 9)).add_modifier(Modifier::BOLD);
        assert_eq!(resolve(&styles, "embedded", base), base);
        assert_eq!(resolve(&SyntaxStyles::empty(), "comment", base), base);
    }

    #[test]
    fn capture_alias_normalizes() {
        // "escape" has no scope of its own; the alias routes it to
        // string.escape — build a map with that scope and check.
        let green = Rgba::from_ints(0, 255, 0, 255);
        let styles =
            SyntaxStyles::from_map(BTreeMap::from([("string.escape".into(), def(green, false))]));
        assert_eq!(resolve(&styles, "escape", Style::new()).fg, Some(Color::Rgb(0, 255, 0)));
    }

    #[test]
    fn attributes_overlay_on_base() {
        let styles = map();
        let base = Style::new().bg(Color::Rgb(1, 1, 1));
        let style = resolve(&styles, "comment", base);
        assert_eq!(style.bg, Some(Color::Rgb(1, 1, 1)), "base bg kept");
        assert!(style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(style.fg, Some(Color::Rgb(255, 0, 0)));
    }
}
