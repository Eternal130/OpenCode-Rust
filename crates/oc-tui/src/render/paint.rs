//! Paint a layout's placed text runs into a ratatui `Buffer` and convert
//! it to a [`Frame`] (absorbed from the wave-7.0 spike, generalized from
//! the single tool-wrap fixture to any paint list).
//!
//! Wave 0 (gate-9): placed lines carry per-run styles
//! ([`crate::layout::PlacedLine`]); painting walks each run's clusters
//! (the same [`crate::wrap::flatten`] seam as layout) and stamps every
//! cell with its run's style — so the canonical ANSI (cells carry
//! style) is unchanged while styled-segment leaves gain per-cell color.
//!
//! Paint coordinates are bounded by terminal dimensions (far below
//! 2^16), so the pedantic usize→u16 cast lints carry no real risk.

#![allow(clippy::cast_possible_truncation)]

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::layout::Placed;
use crate::render::{Cell, CellColor, Frame};
use crate::wrap::flatten;

/// Paint `placed` runs over `size`, first filling the whole area with
/// `background` (the reference renderer fills the frame with the theme
/// style).
#[must_use]
pub fn paint_placed(placed: &[Placed], size: (u16, u16), background: Style) -> Frame {
    let (width, height) = size;
    let area = Rect { x: 0, y: 0, width, height };
    let mut buffer = Buffer::empty(area);
    buffer.set_style(area, background);
    for item in placed {
        paint_run(&mut buffer, item, width, height);
    }
    to_frame(&buffer)
}

fn paint_run(buffer: &mut Buffer, item: &Placed, width: u16, height: u16) {
    for (line_index, line) in item.lines.iter().enumerate() {
        let y = item.y as u16 + line_index as u16;
        if y >= height {
            break;
        }
        let mut x = item.x as u16;
        for run in &line.runs {
            // Per-cell styles from segments: each run paints its clusters
            // (never splitting one across cells) under the run's style.
            for atom in flatten(&run.text, run.style) {
                if x >= width {
                    break;
                }
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_symbol(&atom.text);
                    cell.set_style(run.style);
                }
                x += atom.width.max(1) as u16;
            }
        }
    }
}

/// Convert a ratatui buffer into a [`Frame`].
#[must_use]
pub fn to_frame(buffer: &Buffer) -> Frame {
    let width = buffer.area.width as usize;
    let height = buffer.area.height as usize;
    let mut cells = vec![vec![Cell::default(); width]; height];
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x as u16, y as u16)];
            cells[y][x] = Cell {
                ch: cell.symbol().chars().next().unwrap_or(' '),
                attrs: modifier_bits(cell.modifier),
                fg: map_color(cell.fg),
                bg: map_color(cell.bg),
            };
        }
    }
    Frame { cells, width, height }
}

fn map_color(color: Color) -> CellColor {
    match color {
        Color::Indexed(index) => CellColor::Indexed(index),
        Color::Rgb(r, g, b) => CellColor::Rgb(r, g, b),
        // Reset and every non-color variant map to the terminal default.
        _ => CellColor::Default,
    }
}

fn modifier_bits(modifier: Modifier) -> u16 {
    let mut bits = 0u16;
    if modifier.contains(Modifier::BOLD) {
        bits |= 1 << 0;
    }
    if modifier.contains(Modifier::DIM) {
        bits |= 1 << 1;
    }
    if modifier.contains(Modifier::ITALIC) {
        bits |= 1 << 2;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        bits |= 1 << 3;
    }
    if modifier.contains(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK) {
        bits |= 1 << 4;
    }
    if modifier.contains(Modifier::REVERSED) {
        bits |= 1 << 5;
    }
    if modifier.contains(Modifier::HIDDEN) {
        bits |= 1 << 6;
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        bits |= 1 << 7;
    }
    bits
}
