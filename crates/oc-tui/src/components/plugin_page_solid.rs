//! Lane R1 — the plugin route page (+ its missing-route fallback) on
//! the shared opentui-solid chrome bridge
//! ([`crate::components::chrome_solid`]): the old painter's
//! `put_raw` walks re-expressed as absolute text units over a
//! default (transparent) background, one unit per run (per-CHAR
//! units when a run is not ASCII — the painter's one-cell-per-char
//! advance, see `chrome_run_units`). The parity gate pins the solid
//! frame cell-for-cell against the old painter.

use super::chrome_solid::{chrome_frame, chrome_run_units, ChromeUnit};
use crate::render::CellColor;
use crate::theme::ThemeStyles;

/// Clip a run to the old `put_raw` bound: writes stop at column
/// `bound_col` (`at >= bound_col` breaks), starting at `x`.
fn clip_run(text: &str, x: usize, bound_col: usize) -> String {
    let max = bound_col.saturating_sub(x);
    text.chars().take(max).collect()
}

/// The registered-route page: the title row (+ the right-aligned
/// `ctrl+c exit` hint), the materialized rows under it — the
/// new-path twin of [`super::plugin_page::render_plugin_page`].
#[must_use]
pub fn render_plugin_page_solid(
    title: &str,
    rows: &[String],
    styles: &ThemeStyles,
    size: (u16, u16),
) -> crate::render::Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let text = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let mut units = Vec::new();
    units.extend(chrome_run_units(
        "plugin.title",
        2,
        1,
        &clip_run(title, 2, width.saturating_sub(1)),
        text,
        CellColor::Default,
        0,
    ));
    let hint = "ctrl+c exit";
    let hint_x = width.saturating_sub(hint.chars().count() + 2);
    if hint_x > 4 {
        units.extend(chrome_run_units(
            "plugin.hint",
            hint_x,
            1,
            &clip_run(hint, hint_x, width),
            muted,
            CellColor::Default,
            0,
        ));
    }
    let body = height.saturating_sub(4);
    for (offset, row) in rows.iter().take(body).enumerate() {
        units.extend(chrome_run_units(
            &format!("plugin.row{offset}"),
            2,
            3 + offset,
            &clip_run(row, 2, width.saturating_sub(1)),
            text,
            CellColor::Default,
            0,
        ));
    }
    chrome_frame("plugin_page", width.max(1), height.max(1), CellColor::Default, units)
}

/// The missing-route fallback: the centered warning line + the
/// reverse-video `go home` button — the new-path twin of
/// [`super::plugin_page::render_plugin_route_missing`]. The button
/// rect (for the click handler) is the shared
/// [`super::plugin_page::plugin_route_home_rect`].
#[must_use]
pub fn render_plugin_route_missing_solid(
    id: &str,
    name: &str,
    styles: &ThemeStyles,
    size: (u16, u16),
) -> crate::render::Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let warning = CellColor::from(styles.warning);
    let action = CellColor::from(styles.action_hovered);
    let reverse = 1u16 << 7; // the button's raised background (reverse video)
    let mut units = Vec::new();
    let line = format!("Unknown plugin route: {id}/{name}");
    let center_row = height / 2;
    let x = width.saturating_sub(line.chars().count()) / 2;
    units.extend(chrome_run_units(
        "plugin.missing",
        x,
        center_row,
        &clip_run(&line, x, width),
        warning,
        CellColor::Default,
        0,
    ));
    let button = "go home";
    let (bx, by, bw, _bh) = super::plugin_page::plugin_route_home_rect(width, height);
    // The button's attrs band covers [bx, bx+bw): the leading pad,
    // the glyphs (fg = action) and the trailing pad — all reverse.
    units.extend(chrome_run_units(
        "plugin.home.pad",
        bx,
        by,
        " ",
        CellColor::Default,
        CellColor::Default,
        reverse,
    ));
    units.extend(chrome_run_units(
        "plugin.home",
        bx + 1,
        by,
        button,
        action,
        CellColor::Default,
        reverse,
    ));
    units.extend(chrome_run_units(
        "plugin.home.tail",
        bx + 1 + button.chars().count(),
        by,
        " ",
        CellColor::Default,
        CellColor::Default,
        reverse,
    ));
    chrome_frame("plugin_route_missing", width.max(1), height.max(1), CellColor::Default, units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::chrome_solid::first_divergence;
    use crate::theme::ThemeState;

    fn styles() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    fn scenarios() -> Vec<(usize, usize)> {
        vec![(40, 10), (60, 24), (80, 30), (120, 40), (26, 8), (10, 6)]
    }
}
