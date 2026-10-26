//! Lane R1 — the renderer console overlay on the shared
//! opentui-solid chrome bridge ([`crate::components::chrome_solid`]):
//! the bordered `console` box with the ring's tail lines as absolute
//! text units over a default (transparent) background. The parity
//! gate pins the solid frame cell-for-cell against the old painter
//! ([`super::console::render_console_overlay`]).

use super::chrome_solid::{chrome_frame, chrome_run_units};
use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;

/// The new-path twin of [`super::console::render_console_overlay`].
#[must_use]
pub fn render_console_overlay_solid(lines: &[String], styles: &ThemeStyles, width: u16) -> Frame {
    let width = usize::from(width).max(6);
    let height = super::console::CONSOLE_ROWS.min(lines.len() + 2);
    let rows = height.max(2);
    let text = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let mut units = Vec::new();
    // The top border + the riding title.
    let top: String = std::iter::once('┌')
        .chain(std::iter::repeat_n('─', width.saturating_sub(2)))
        .chain(std::iter::once('┐'))
        .collect();
    units.extend(chrome_run_units("console.top", 0, 0, &top, muted, CellColor::Default, 0));
    units.extend(chrome_run_units(
        "console.title",
        2,
        0,
        " console ",
        muted,
        CellColor::Default,
        0,
    ));
    // The bottom border (the cells' last row).
    let bottom: String = std::iter::once('└')
        .chain(std::iter::repeat_n('─', width.saturating_sub(2)))
        .chain(std::iter::once('┘'))
        .collect();
    units.extend(chrome_run_units(
        "console.bottom",
        0,
        rows - 1,
        &bottom,
        muted,
        CellColor::Default,
        0,
    ));
    // The side borders (rows 1..rows-1; absent on a 2-row box).
    for row in 1..rows.saturating_sub(1) {
        units.extend(chrome_run_units("console.side", 0, row, "│", muted, CellColor::Default, 0));
        units.extend(chrome_run_units(
            "console.side.r",
            width - 1,
            row,
            "│",
            muted,
            CellColor::Default,
            0,
        ));
    }
    // The tail: the newest `rows - 2` lines, one per row, clipped at
    // column width − 2 (the old `at + 1 >= width` break).
    let body = height.saturating_sub(2);
    let tail: Vec<&String> = lines.iter().rev().take(body).collect();
    for (offset, line) in tail.iter().rev().enumerate() {
        let max = width.saturating_sub(2);
        let clipped: String = line.chars().take(max).collect();
        units.extend(chrome_run_units(
            "console.line",
            1,
            1 + offset,
            &clipped,
            text,
            CellColor::Default,
            0,
        ));
    }
    chrome_frame("console_overlay", width, rows, CellColor::Default, &units)
}
