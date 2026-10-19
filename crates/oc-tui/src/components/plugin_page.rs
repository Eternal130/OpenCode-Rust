//! The plugin route page (PLUG-TUI-SLOT's router surface): the
//! native full-screen page a plugin's registered route renders into —
//! the guest's `render({data})` ran through the host's bounded
//! `route.render` call and the flattened `{title, rows}` crossed
//! once; this module paints it (no per-frame IPC). The ref's shape:
//! a full-box page over the app (feature-plugins/system/stats.tsx:
//! 140-186 — `StatsPage` paints its box edge-to-edge with the plugin
//! page's own keymap layer providing the escape/back bind).

use crate::render::{Cell, CellColor, Frame};
use crate::theme::ThemeStyles;

/// Write `text` at (`row`, `x`), clipped to `width`.
fn put_raw(cells: &mut [Vec<Cell>], row: usize, x: usize, text: &str, fg: CellColor, width: usize) {
    let Some(line) = cells.get_mut(row) else { return };
    for (index, ch) in text.chars().enumerate() {
        let at = x + index;
        if at >= width.saturating_sub(1) {
            break;
        }
        if let Some(cell) = line.get_mut(at) {
            cell.ch = ch;
            cell.fg = fg;
        }
    }
}

/// Paint one plugin page: the route name as the header, the
/// materialized rows under it (the theme's text/muted pair).
#[must_use]
pub fn render_plugin_page(
    title: &str,
    rows: &[String],
    styles: &ThemeStyles,
    size: (u16, u16),
) -> Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let mut cells: Vec<Vec<Cell>> = vec![vec![Cell::default(); width.max(1)]; height.max(1)];
    let text = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    put_raw(&mut cells, 1, 2, title, text, width);
    // The page hint (right-aligned on the header row — the ref's
    // pages carry their own keymap layer's back bind).
    let hint = "ctrl+c exit";
    let hint_x = width.saturating_sub(hint.chars().count() + 2);
    if hint_x > 4 {
        put_raw(&mut cells, 1, hint_x, hint, muted, width + 1);
    }
    let body = height.saturating_sub(4);
    for (offset, row) in rows.iter().take(body).enumerate() {
        put_raw(&mut cells, 3 + offset, 2, row, text, width);
    }
    Frame { cells, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    #[test]
    fn page_paints_title_and_rows() {
        let theme = ThemeState::v2_dark();
        let frame = render_plugin_page(
            "stats",
            &["Usage statistics".to_owned(), "sessions: 12".to_owned()],
            &theme.styles(),
            (40, 10),
        );
        let grid = frame.text_grid();
        assert!(grid[1].contains("stats"), "the route title: {:?}", grid[1]);
        assert!(grid[3].contains("Usage statistics"), "row 1: {:?}", grid[3]);
        assert!(grid[4].contains("sessions: 12"), "row 2: {:?}", grid[4]);
    }
}
