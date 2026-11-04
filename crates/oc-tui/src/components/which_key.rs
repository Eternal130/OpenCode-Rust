//! G-83 — the which-key panel: the bounded port of the
//! `@opentui/keymap` addon's pending-chord help surface
//! (upstream TUI: `createDefaultOpenTuiKeymap` installs the panel;
//! the ref's context/keymap.tsx:13,66). When a stroke lands on a
//! PREFIX of registered multi-key bindings (the port's `<leader>`
//! chords, ctrl+x …), the panel lists the reachable continuations
//! (`getActiveKeys` — the engine's [`ActiveKey`] datum) with the
//! table's own labels, until the chord completes, escapes, or an
//! unbound continuation drops the pending sequence. Bounded scope:
//! no timeout config, no layout/scroll surface — the content is the
//! table's continuation bindings, sorted by key.

use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;

/// The panel's row cap (the bounded surface: a compact bottom
/// overlay, never a full-screen listing).
pub const WHICH_KEY_MAX_ENTRIES: usize = 12;

/// One continuation row: the next key's display + the command's
/// label (the table's `description`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhichKeyEntry {
    pub key: String,
    pub label: String,
}

/// The panel's view model (live store state — `Some` only while the
/// panel shows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhichKeyView {
    /// The pending prefix's display ("<leader>" style) — `None` for
    /// the root listing (`which-key.toggle`).
    pub prefix: Option<String>,
    pub entries: Vec<WhichKeyEntry>,
}

/// Render the panel: a bordered box in the console-overlay style
/// (chrome borders, the prefix/title riding the top border, one
/// `key  label` row per continuation).
#[must_use]
pub fn render_which_key_solid(view: &WhichKeyView, styles: &ThemeStyles, width: u16) -> Frame {
    use super::chrome_solid::{chrome_run_units, overlay_frame};
    let width = usize::from(width).max(8);
    let rows = view.entries.len().clamp(1, WHICH_KEY_MAX_ENTRIES) + 2;
    let text = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let accent = CellColor::from(styles.accent);
    let mut units = Vec::new();
    // The bordered box (the console overlay's geometry).
    let top: String = std::iter::once('┌')
        .chain(std::iter::repeat_n('─', width.saturating_sub(2)))
        .chain(std::iter::once('┐'))
        .collect();
    units.extend(chrome_run_units("which-key.top", 0, 0, &top, muted, CellColor::Default, 0));
    let title = match &view.prefix {
        Some(prefix) => format!(" {prefix} "),
        None => " which-key ".to_owned(),
    };
    units.extend(chrome_run_units("which-key.title", 2, 0, &title, muted, CellColor::Default, 0));
    let bottom: String = std::iter::once('└')
        .chain(std::iter::repeat_n('─', width.saturating_sub(2)))
        .chain(std::iter::once('┘'))
        .collect();
    units.extend(chrome_run_units(
        "which-key.bottom",
        0,
        rows - 1,
        &bottom,
        muted,
        CellColor::Default,
        0,
    ));
    for row in 1..rows.saturating_sub(1) {
        units.extend(chrome_run_units("which-key.side", 0, row, "│", muted, CellColor::Default, 0));
        units.extend(chrome_run_units(
            "which-key.side.r",
            width - 1,
            row,
            "│",
            muted,
            CellColor::Default,
            0,
        ));
    }
    // The continuation rows: the key in the accent color, the label
    // in the base text color, clipped at the box interior.
    let max = width.saturating_sub(2);
    for (offset, entry) in view.entries.iter().take(rows - 2).enumerate() {
        let key: String = entry.key.chars().take(max).collect();
        units.extend(chrome_run_units(
            "which-key.key",
            1,
            1 + offset,
            &key,
            accent,
            CellColor::Default,
            0,
        ));
        let room = max.saturating_sub(key.chars().count() + 2);
        let label: String = entry.label.chars().take(room).collect();
        if !label.is_empty() {
            units.extend(chrome_run_units(
                "which-key.label",
                1 + key.chars().count() + 2,
                1 + offset,
                &label,
                text,
                CellColor::Default,
                0,
            ));
        }
    }
    overlay_frame("which_key_panel", width, rows, CellColor::Default, &units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeStyles;

    #[test]
    fn panel_renders_prefix_and_sorted_rows() {
        let styles = ThemeStyles::from_resolved(&crate::theme::ThemeState::v2_dark().resolved);
        let view = WhichKeyView {
            prefix: Some("<leader>".to_owned()),
            entries: vec![
                WhichKeyEntry { key: "n".into(), label: "Create a new session".into() },
                WhichKeyEntry { key: "w".into(), label: "Close current tab".into() },
            ],
        };
        let frame = render_which_key_solid(&view, &styles, 40);
        let grid = frame.text_grid();
        assert!(grid[0].contains("<leader>"), "the prefix rides the top border");
        assert!(grid[1].contains("n") && grid[1].contains("Create a new session"));
        assert!(grid[2].contains("w") && grid[2].contains("Close current tab"));
        assert!(grid[3].starts_with('└'), "the box closes");
    }
}
