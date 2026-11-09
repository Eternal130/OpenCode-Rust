//! Lane R1 — the plugin route page (+ its missing-route fallback) on
//! the shared opentui-solid chrome bridge
//! ([`crate::components::chrome_solid`]): the old painter's
//! `put_raw` walks re-expressed as absolute text units over a
//! default (transparent) background, one unit per run (per-CHAR
//! units when a run is not ASCII — the painter's one-cell-per-char
//! advance, see `chrome_run_units`). The parity gate pins the solid
//! frame cell-for-cell against the old painter.

use super::chrome_solid::{chrome_mount, chrome_run_units, ChromeUnit};
use crate::render::CellColor;
use crate::theme::ThemeStyles;

/// Clip a run to the old `put_raw` bound: writes stop at column
/// `bound_col` (`at >= bound_col` breaks), starting at `x`.
fn clip_run(text: &str, x: usize, bound_col: usize) -> String {
    let max = bound_col.saturating_sub(x);
    text.chars().take(max).collect()
}

/// Mount the registered-route page into the shared chrome tree
/// (D2.2): absolute text units over a transparent background,
/// covering the screen (the caller blanks the base — today's
/// full-screen paste).
pub fn mount_plugin_page_solid(
    title: &str,
    rows: &[String],
    styles: &ThemeStyles,
    size: (u16, u16),
) {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let units = plugin_page_units(title, rows, styles, width, height);
    chrome_mount("plugin_page", 0, 0, width.max(1), height.max(1), CellColor::Default, &units, &[]);
}

/// The golden/parity twin: the page's standalone Frame (the retired
/// per-surface copy's test form — the integration parity harness
/// reads it too, hence `pub`).
#[must_use]
pub fn plugin_page_solid_frame(
    title: &str,
    rows: &[String],
    styles: &ThemeStyles,
    size: (u16, u16),
) -> crate::render::Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let units = plugin_page_units(title, rows, styles, width, height);
    super::chrome_solid::mounted_surface_frame(
        "plugin_page",
        width.max(1),
        height.max(1),
        CellColor::Default,
        &units,
    )
}

/// The unit walk (the old painter's own geometry, verbatim).
fn plugin_page_units(
    title: &str,
    rows: &[String],
    styles: &ThemeStyles,
    width: usize,
    height: usize,
) -> Vec<ChromeUnit> {
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
    units
}

/// The missing-route fallback: the centered warning line + the
/// reverse-video `go home` button — the new-path twin of
/// [`super::plugin_page::render_plugin_route_missing`]. The button
/// rect (for the click handler) is the shared
/// [`super::plugin_page::plugin_route_home_rect`].
/// Mount the missing-route fallback into the shared chrome tree
/// (D2.2) — the caller blanks the base (today's full-screen paste).
pub fn mount_plugin_route_missing_solid(
    id: &str,
    name: &str,
    styles: &ThemeStyles,
    size: (u16, u16),
) {
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
    let (bx, by, _bw, _bh) = super::plugin_page::plugin_route_home_rect(width, height);
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
    chrome_mount(
        "plugin_route_missing",
        0,
        0,
        width.max(1),
        height.max(1),
        CellColor::Default,
        &units,
        &[],
    );
}

/// The golden twin: the fallback's standalone Frame.
#[cfg(test)]
#[must_use]
pub fn plugin_route_missing_solid_frame(
    id: &str,
    name: &str,
    styles: &ThemeStyles,
    size: (u16, u16),
) -> crate::render::Frame {
    // Shape by mounting into a scratch hub? No — reuse the mount path
    // itself: reset, mount, composite over a blank screen frame.
    super::chrome_solid::reset_hub();
    mount_plugin_route_missing_solid(id, name, styles, size);
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let mut frame = super::chrome_solid::blank_frame(width.max(1), height.max(1));
    super::chrome_solid::composite_chrome_tree(&mut frame);
    frame
}
