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

use super::chrome_solid::{
    blank_frame, chrome_run_units, composite_chrome_tree, reset_hub, ChromeUnit,
};
use super::overlays_solid::{mount_overlay_surface, OVERLAYS_WHICH_KEY};
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
/// The panel's shaping (surface-local): the same walk the old
/// painter ran, now feeding the tree mount (D2.2 overlays lane — the
/// `surface-which-key` family wrapper at skeleton index 6,
/// BLANK-underlaid: the old `paste_frame` overwrote the rect's every
/// cell, transparent interior included).
#[must_use]
pub fn which_key_units(
    view: &WhichKeyView,
    styles: &ThemeStyles,
    width: u16,
) -> (usize, usize, Vec<ChromeUnit>) {
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
    (width, rows, units)
}

/// Mount the which-key panel into the shared tree (the live path —
/// called from `render_app` before the compositor seam; the wrapper
/// seats at the screen bottom, the old paste's y).
pub fn mount_which_key(view: &WhichKeyView, styles: &ThemeStyles, width: u16, height: usize) {
    let (w, rows, units) = which_key_units(view, styles, width);
    let y = height.saturating_sub(rows);
    mount_overlay_surface("which-key", OVERLAYS_WHICH_KEY, 0, y, w, rows, &units, true, &[]);
}

/// The headless twin (the golden/unit harness shape): mount ONE
/// panel into a fresh hub and read it back as a standalone Frame —
/// the bytes the old scratch copy returned, by construction.
#[must_use]
pub fn which_key_frame(view: &WhichKeyView, styles: &ThemeStyles, width: u16) -> Frame {
    let (w, rows, units) = which_key_units(view, styles, width);
    reset_hub();
    mount_overlay_surface("which-key", OVERLAYS_WHICH_KEY, 0, 0, w, rows, &units, true, &[]);
    let mut frame = blank_frame(w, rows);
    composite_chrome_tree(&mut frame);
    frame
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
        let frame = which_key_frame(&view, &styles, 40);
        let grid = frame.text_grid();
        assert!(grid[0].contains("<leader>"), "the prefix rides the top border");
        assert!(grid[1].contains("n") && grid[1].contains("Create a new session"));
        assert!(grid[2].contains("w") && grid[2].contains("Close current tab"));
        assert!(grid[3].starts_with('└'), "the box closes");
    }
}
