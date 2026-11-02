//! Paint a layout's placed text runs into the offscreen canvas
//! ([`Buffer`]) and convert it to a [`Frame`] (absorbed from the
//! wave-7.0 spike, generalized from the single tool-wrap fixture to
//! any paint list).
//!
//! Wave 0 (gate-9): placed lines carry per-run styles
//! ([`crate::layout::PlacedLine`]); painting walks each run's clusters
//! (the same [`crate::wrap::flatten`] seam as layout) and stamps every
//! cell with its run's style — so the canonical ANSI (cells carry
//! style) is unchanged while styled-segment leaves gain per-cell color.
//!
//! Lane B2: the canvas is oc-tui's own [`Buffer`] — the ratatui 0.29
//! `buffer` semantics (`empty` fills with blank `Reset` cells,
//! `set_style` merges per cell, `cell_mut` is bounds-checked) copied
//! verbatim (each method notes its upstream source).
//!
//! Paint coordinates are bounded by terminal dimensions (far below
//! 2^16), so the pedantic usize→u16 cast lints carry no real risk.

#![allow(clippy::cast_possible_truncation)]

use crate::layout::Placed;
use crate::render::{Cell, CellColor, Frame};
use crate::style::{Color, Modifier, Style};
use crate::wrap::flatten;

/// A rectangular area (ratatui 0.29 `layout::Rect` semantics: origin
/// + size, all `u16`, inclusive-origin / exclusive-end edges).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    #[must_use]
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self { x, y, width, height }
    }

    /// The intersecting area (ratatui `Rect::intersection`: the
    /// overlapping rectangle, empty when disjoint).
    #[must_use]
    pub fn intersection(self, other: Self) -> Self {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);
        Self { x: x1, y: y1, width: x2.saturating_sub(x1), height: y2.saturating_sub(y1) }
    }
}

/// One styled cell of the offscreen canvas (ratatui 0.29
/// `buffer::Cell` semantics: the symbol may hold a multi-char
/// grapheme, unset parts stay `Reset`, and `set_style` MERGES).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasCell {
    pub symbol: String,
    pub fg: Color,
    pub bg: Color,
    pub modifier: Modifier,
}

impl Default for CanvasCell {
    /// ratatui `Cell::DEFAULT`: a blank `Reset`-colored cell.
    fn default() -> Self {
        Self {
            symbol: " ".to_owned(),
            fg: Color::Reset,
            bg: Color::Reset,
            modifier: Modifier::empty(),
        }
    }
}

impl CanvasCell {
    /// Set the symbol (ratatui `Cell::set_symbol`: replaces the
    /// symbol wholesale; multi-char graphemes allowed).
    pub fn set_symbol(&mut self, symbol: &str) -> &mut Self {
        self.symbol = symbol.to_owned();
        self
    }

    /// Merge a style into the cell (ratatui 0.29 `Cell::set_style`:
    /// `Some` colors overlay, `None` colors keep the current ones, the
    /// add word is OR-ed in and the sub word cleared).
    pub fn set_style(&mut self, style: Style) -> &mut Self {
        if let Some(color) = style.fg {
            self.fg = color;
        }
        if let Some(color) = style.bg {
            self.bg = color;
        }
        self.modifier.insert(style.add_modifier);
        self.modifier.remove(style.sub_modifier);
        self
    }
}

/// The offscreen cell grid (ratatui 0.29 `buffer::Buffer` semantics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buffer {
    pub area: Rect,
    pub content: Vec<CanvasCell>,
}

impl Buffer {
    /// A buffer of blank `Reset` cells covering `area` (ratatui
    /// `Buffer::empty`).
    #[must_use]
    pub fn empty(area: Rect) -> Self {
        let size = area.width as usize * area.height as usize;
        Self { area, content: vec![CanvasCell::default(); size] }
    }

    fn index_of(&self, x: u16, y: u16) -> Option<usize> {
        if x < self.area.x
            || x >= self.area.x + self.area.width
            || y < self.area.y
            || y >= self.area.y + self.area.height
        {
            return None;
        }
        Some(
            (y - self.area.y) as usize * self.area.width as usize + (x - self.area.x) as usize,
        )
    }

    /// Bounds-checked mutable cell access (ratatui `Buffer::cell_mut`:
    /// `None` outside the buffer's area).
    pub fn cell_mut(&mut self, position: (u16, u16)) -> Option<&mut CanvasCell> {
        let index = self.index_of(position.0, position.1)?;
        self.content.get_mut(index)
    }

    /// Apply `style` to every cell of `area` ∩ the buffer's area
    /// (ratatui `Buffer::set_style`: intersection, then per-cell
    /// merge).
    pub fn set_style(&mut self, area: Rect, style: Style) {
        let area = self.area.intersection(area);
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some(cell) = self.cell_mut((x, y)) {
                    cell.set_style(style);
                }
            }
        }
    }
}

/// A background fill rectangle: `height` rows of `width` cells stamped
/// with `style` before any text paints over them (opentui box
/// `backgroundColor` — fills never carry glyphs). Painted top-down in
/// list order; text placed afterwards wins per cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub style: Style,
}

/// Paint `placed` runs over `size`, first filling the whole area with
/// `background` (the reference renderer fills the frame with the theme
/// style).
#[must_use]
pub fn paint_placed(placed: &[Placed], size: (u16, u16), background: Style) -> Frame {
    paint_placed_with_fills(placed, &[], size, background)
}

/// [`paint_placed`] with explicit background-fill rectangles (box
/// backgrounds) applied after the whole-frame fill, before text.
#[must_use]
pub fn paint_placed_with_fills(
    placed: &[Placed],
    fills: &[Fill],
    size: (u16, u16),
    background: Style,
) -> Frame {
    let (width, height) = size;
    let area = Rect { x: 0, y: 0, width, height };
    let mut buffer = Buffer::empty(area);
    buffer.set_style(area, background);
    for fill in fills {
        let clip = Rect {
            x: fill.x.min(width),
            y: fill.y.min(height),
            width: fill.width.min(width.saturating_sub(fill.x.min(width))),
            height: fill.height.min(height.saturating_sub(fill.y.min(height))),
        };
        if clip.width > 0 && clip.height > 0 {
            buffer.set_style(clip, fill.style);
        }
    }
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
                let atom_width = atom.width.max(1) as u16;
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_symbol(&atom.text);
                    cell.set_style(run.style);
                }
                // Wide clusters plant `'\0'` continuation cells under
                // their span (lane M4): the ref binary's opentui
                // raster does exactly this (buffer.rs's continuation
                // planting — the composer's `paint_text` already
                // matched it), carrying the run's style; the previous
                // leave-the-fill behavior diverged from the reference
                // on every multi-cell grapheme. No golden or rig
                // corpus carried wide clusters, so nothing pinned the
                // old bytes.
                let span_end = (x + atom_width).min(width);
                for continuation in (x + 1)..span_end {
                    if let Some(cell) = buffer.cell_mut((continuation, y)) {
                        cell.set_symbol("\u{0}");
                        cell.set_style(run.style);
                    }
                }
                x += atom_width;
            }
        }
    }
}

/// Convert the offscreen canvas into a [`Frame`].
#[must_use]
pub fn to_frame(buffer: &Buffer) -> Frame {
    let width = buffer.area.width as usize;
    let height = buffer.area.height as usize;
    let mut cells = vec![vec![Cell::default(); width]; height];
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer.content[y * width + x];
            cells[y][x] = Cell {
                ch: cell.symbol.chars().next().unwrap_or(' '),
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
        // Reset (and every non-color variant) maps to the terminal
        // default.
        Color::Reset => CellColor::Default,
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
