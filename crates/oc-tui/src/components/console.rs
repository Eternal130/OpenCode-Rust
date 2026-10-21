//! TUI-29 — the renderer console overlay (app.tsx:242-244 + 1110-
//! 1118's `renderer.console`): the captured diagnostics tail painted
//! at the top of the screen while toggled on, with the console's own
//! `copy-selection` binding (ctrl+y → clipboard + toast + clear).

use crate::render::{Cell, CellColor, Frame};
use crate::theme::ThemeStyles;

/// The ring's capacity (the console keeps the last 256 lines).
pub const CONSOLE_RING: usize = 256;

/// The overlay's row budget (opentui's console clamps to a top strip;
/// 12 rows — the border pair + 10 lines of tail).
pub const CONSOLE_ROWS: usize = 12;

/// Paint the console strip: a bordered box titled `console` carrying
/// the ring's TAIL lines (the newest at the bottom), docked at the
/// top of the screen.
#[must_use]
pub fn render_console_overlay(lines: &[String], styles: &ThemeStyles, width: u16) -> Frame {
    let width = usize::from(width).max(6);
    let height = CONSOLE_ROWS.min(lines.len() + 2);
    let mut cells: Vec<Vec<Cell>> = vec![vec![Cell::default(); width]; height.max(2)];
    let text = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);

    // The border: a single-line box with the title riding the top.
    let title = " console ";
    if let Some(top) = cells.first_mut() {
        for (x, cell) in top.iter_mut().enumerate() {
            cell.ch = if x == 0 { '┌' } else if x + 1 == width { '┐' } else { '─' };
            cell.fg = muted;
        }
        for (index, ch) in title.chars().enumerate() {
            let at = 2 + index;
            if let Some(cell) = top.get_mut(at) {
                cell.ch = ch;
                cell.fg = muted;
            }
        }
    }
    if let Some(bottom) = cells.last_mut() {
        if height >= 2 {
            for (x, cell) in bottom.iter_mut().enumerate() {
                cell.ch = if x == 0 { '└' } else if x + 1 == width { '┘' } else { '─' };
                cell.fg = muted;
            }
        }
    }
    for row in cells.iter_mut().take(height.saturating_sub(1)).skip(1) {
        let last = row.len().saturating_sub(1);
        if last > 0 {
            row[0].ch = '│';
            row[0].fg = muted;
            row[last].ch = '│';
            row[last].fg = muted;
        }
    }
    // The tail: the newest `height - 2` lines, one per row.
    let body = height.saturating_sub(2);
    let tail: Vec<&String> = lines.iter().rev().take(body).collect();
    for (offset, line) in tail.iter().rev().enumerate() {
        let row = 1 + offset;
        if let Some(cells_row) = cells.get_mut(row) {
            for (index, ch) in line.chars().enumerate() {
                let at = 1 + index;
                if at + 1 >= width {
                    break;
                }
                if let Some(cell) = cells_row.get_mut(at) {
                    cell.ch = ch;
                    cell.fg = text;
                }
            }
        }
    }
    Frame { cells, width, height: height.max(2) }
}
