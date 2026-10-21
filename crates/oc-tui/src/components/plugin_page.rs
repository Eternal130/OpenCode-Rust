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

/// TUI-TD28 — the `go home` button's rect (`go home` + 2 padding
/// columns, one row, centered two rows under the warning line). Both
/// the renderer and the app's click handler derive it from the size —
/// the same geometry, no recorded state.
#[must_use]
pub fn plugin_route_home_rect(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let button = "go home";
    let w = button.chars().count() + 2;
    let x = width.saturating_sub(w) / 2;
    (x, height / 2 + 2, w, 1)
}

/// TUI-TD28 — the fallback screen for an UNREGISTERED plugin route
/// (component/plugin-route-missing.tsx:3-21): the warning line
/// `Unknown plugin route: {id}/{name}` in the theme's warning color
/// and the `go home` action button, both centered; `hits` (when
/// provided) receives the button's rect — its release navigates home
/// (`onHome={() => route.navigate({type: "home"})}`). One
/// `(x, y, width, height, "plugin.route.home")` entry.
#[must_use]
pub fn render_plugin_route_missing(
    id: &str,
    name: &str,
    styles: &ThemeStyles,
    size: (u16, u16),
    hits: &mut Vec<(usize, usize, usize, usize, &'static str)>,
) -> Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let mut cells: Vec<Vec<Cell>> = vec![vec![Cell::default(); width.max(1)]; height.max(1)];
    let warning = CellColor::from(styles.warning);
    let action = CellColor::from(styles.action_hovered);

    // Both rows centered (the ref's centered column layout).
    let line = format!("Unknown plugin route: {id}/{name}");
    let center_row = height / 2;
    let x = width.saturating_sub(line.chars().count()) / 2;
    put_raw(&mut cells, center_row, x, &line, warning, width + 1);
    let button = "go home";
    let (bx, by, bw, bh) = plugin_route_home_rect(width, height);
    put_raw(&mut cells, by, bx + 1, button, action, width + 1);
    // The button's raised background (the ref's hovered action box).
    if let Some(row) = cells.get_mut(by) {
        for cell in row.iter_mut().skip(bx).take(bw) {
            cell.attrs |= 1 << 7; // reverse video
        }
    }
    hits.push((bx, by, bw, bh, "plugin.route.home"));
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
