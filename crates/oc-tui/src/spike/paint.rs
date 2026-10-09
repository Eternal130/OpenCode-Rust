//! Paint a laid-out fixture into a ratatui `Buffer` and serialize the buffer
//! through the canonical golden format (identical bytes to the exporter and
//! to `parity::tui_frame::render_ansi` — cross-checked in tests).
//!
//! Paint coordinates are bounded by terminal dimensions, so the pedantic
//! int-cast lints do not carry real risk here.
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::layout::LaidOutFixture;

/// The preload default theme paints the whole frame with an explicit
/// white-on-black RGB style (observed in the golden ANSI stream).
pub const BASE_FG: (u8, u8, u8) = (255, 255, 255);
pub const BASE_BG: (u8, u8, u8) = (0, 0, 0);

#[must_use]
pub fn base_style() -> Style {
    Style::new()
        .fg(Color::Rgb(BASE_FG.0, BASE_FG.1, BASE_FG.2))
        .bg(Color::Rgb(BASE_BG.0, BASE_BG.1, BASE_BG.2))
}

/// One styled cell of the spike output frame (same shape as the parity
/// golden cell; kept local so the spike does not depend on the parity crate
/// at lib level).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpikeCell {
    pub ch: char,
    pub attrs: u16,
    pub fg: SpikeColor,
    pub bg: SpikeColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpikeColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// A painted frame.
#[derive(Debug, Clone)]
pub struct SpikeFrame {
    pub cells: Vec<Vec<SpikeCell>>,
    pub width: usize,
    pub height: usize,
}

impl SpikeFrame {
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> SpikeCell {
        self.cells.get(row).and_then(|line| line.get(col)).copied().unwrap_or(SpikeCell {
            ch: ' ',
            attrs: 0,
            fg: SpikeColor::Default,
            bg: SpikeColor::Default,
        })
    }

    /// The frame as a text grid (glyphs only), one line per row.
    #[must_use]
    pub fn text_grid(&self) -> Vec<String> {
        self.cells.iter().map(|line| line.iter().map(|cell| cell.ch).collect()).collect()
    }
}

/// Paint the fixture at the given terminal size.
#[must_use]
pub fn paint(fixture: &LaidOutFixture, width: u16, height: u16) -> Buffer {
    let area = Rect { x: 0, y: 0, width, height };
    let mut buffer = Buffer::empty(area);
    // The reference renderer fills the whole buffer with the theme style.
    buffer.set_style(area, base_style());
    for row in &fixture.rows {
        let icon = row.icon.to_string();
        if let Some(cell) = buffer.cell_mut((row.icon_x as u16, row.y as u16)) {
            cell.set_symbol(&icon);
        }
        for (line_index, line) in row.lines.iter().enumerate() {
            let y = row.y as u16 + line_index as u16;
            if y >= height {
                break;
            }
            let mut x = row.label_x as u16;
            for ch in line.chars() {
                if x >= width {
                    break;
                }
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_symbol(ch.encode_utf8(&mut [0u8; 4]));
                }
                x += char_width(ch) as u16;
            }
        }
    }
    buffer
}

fn char_width(ch: char) -> usize {
    use unicode_width::UnicodeWidthChar;
    ch.width().unwrap_or(1)
}

/// Convert a ratatui buffer into a spike frame.
#[must_use]
pub fn to_spike_frame(buffer: &Buffer) -> SpikeFrame {
    let width = buffer.area.width as usize;
    let height = buffer.area.height as usize;
    let mut cells =
        vec![
            vec![
                SpikeCell { ch: ' ', attrs: 0, fg: SpikeColor::Default, bg: SpikeColor::Default };
                width
            ];
            height
        ];
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x as u16, y as u16)];
            cells[y][x] = SpikeCell {
                ch: cell.symbol().chars().next().unwrap_or(' '),
                attrs: modifier_bits(cell.modifier),
                fg: map_color(cell.fg),
                bg: map_color(cell.bg),
            };
        }
    }
    SpikeFrame { cells, width, height }
}

fn map_color(color: Color) -> SpikeColor {
    match color {
        Color::Indexed(index) => SpikeColor::Indexed(index),
        Color::Rgb(r, g, b) => SpikeColor::Rgb(r, g, b),
        // Reset and every non-color variant map to the terminal default.
        _ => SpikeColor::Default,
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

/// Serialize a spike frame to the canonical golden byte format.
#[must_use]
pub fn serialize_canonical(frame: &SpikeFrame) -> String {
    let mut out = String::new();
    for line in &frame.cells {
        let mut current = (0u16, SpikeColor::Default, SpikeColor::Default);
        let mut serialized = String::new();
        for cell in line {
            let style = (cell.attrs, cell.fg, cell.bg);
            if style != current {
                serialized.push_str(&sgr(cell));
                current = style;
            }
            serialized.push(cell.ch);
        }
        if current != (0, SpikeColor::Default, SpikeColor::Default) {
            serialized.push_str("\x1b[0m");
        }
        out.push_str(&serialized);
        out.push('\n');
    }
    out
}

fn sgr(cell: &SpikeCell) -> String {
    let mut params: Vec<String> = vec!["0".into()];
    if cell.attrs & (1 << 0) != 0 {
        params.push("1".into());
    }
    if cell.attrs & (1 << 1) != 0 {
        params.push("2".into());
    }
    if cell.attrs & (1 << 2) != 0 {
        params.push("3".into());
    }
    if cell.attrs & (1 << 3) != 0 {
        params.push("4".into());
    }
    if cell.attrs & (1 << 4) != 0 {
        params.push("5".into());
    }
    if cell.attrs & (1 << 5) != 0 {
        params.push("7".into());
    }
    if cell.attrs & (1 << 6) != 0 {
        params.push("8".into());
    }
    if cell.attrs & (1 << 7) != 0 {
        params.push("9".into());
    }
    params.extend(color_params(cell.fg, 30));
    params.extend(color_params(cell.bg, 40));
    format!("\x1b[{}m", params.join(";"))
}

fn color_params(color: SpikeColor, base: u8) -> Vec<String> {
    match color {
        SpikeColor::Default => vec![(base + 9).to_string()],
        SpikeColor::Indexed(index) => vec![(base + 8).to_string(), "5".into(), index.to_string()],
        SpikeColor::Rgb(r, g, b) => {
            vec![(base + 8).to_string(), "2".into(), r.to_string(), g.to_string(), b.to_string()]
        }
    }
}
