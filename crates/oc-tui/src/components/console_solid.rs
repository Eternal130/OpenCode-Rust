//! The renderer console overlay on the shared tree
//! ([`crate::components::chrome_solid`], D2.2 overlays lane): the
//! bordered `console` box with the ring's tail lines as absolute
//! text units over a default (transparent) background — mounted as
//! the `surface-console` family wrapper (skeleton index 6,
//! BLANK-underlaid: the old `paste_frame` overwrote the whole rect
//! with default cells, transparent interior included, so the seam
//! blanks the rect before the walk instead of letting the
//! hand-painted pixels show through the holes).
//!
//! The parity gate pins the composited frame cell-for-cell against
//! the old painter ([`super::console::render_console_overlay`]).

use super::chrome_solid::{
    blank_frame, chrome_run_units, composite_chrome_tree, reset_hub, ChromeUnit,
};
use super::overlays_solid::{mount_overlay_surface, OVERLAYS_CONSOLE};
use crate::render::{CellColor, Frame};
use crate::theme::ThemeStyles;

/// The console box's shaping (surface-local): the same walk the old
/// painter ran, now feeding the tree mount.
#[must_use]
pub fn console_units(
    lines: &[String],
    styles: &ThemeStyles,
    width: u16,
) -> (usize, usize, Vec<ChromeUnit>) {
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
    (width, rows, units)
}

/// Mount the console strip into the shared tree (the live path —
/// called from `render_app` before the compositor seam; the wrapper
/// is BLANK-underlaid, the old paste's default-cell overwrite).
pub fn mount_console_overlay(lines: &[String], styles: &ThemeStyles, width: u16) {
    let (w, rows, units) = console_units(lines, styles, width);
    mount_overlay_surface("console", OVERLAYS_CONSOLE, 0, 0, w, rows, &units, true, &[]);
}

/// The headless twin (the golden/unit harness shape): mount ONE
/// console strip into a fresh hub and read it back as a standalone
/// Frame (composite over a blank frame of its own rect) — the bytes
/// the old `render_console_overlay_solid` scratch copy returned, by
/// construction (blank underlay = the old paste's rect overwrite).
#[must_use]
pub fn console_overlay_frame(lines: &[String], styles: &ThemeStyles, width: u16) -> Frame {
    let (w, rows, units) = console_units(lines, styles, width);
    reset_hub();
    mount_overlay_surface("console", OVERLAYS_CONSOLE, 0, 0, w, rows, &units, true, &[]);
    let mut frame = blank_frame(w, rows);
    composite_chrome_tree(&mut frame);
    frame
}
