//! Painted-frame output types: the cell grid every renderer produces
//! and the frame tests compare (absorbed from the wave-7.0 spike).

/// Cell color in the canonical (golden) vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl From<ratatui::style::Color> for CellColor {
    fn from(color: ratatui::style::Color) -> Self {
        match color {
            ratatui::style::Color::Rgb(r, g, b) => Self::Rgb(r, g, b),
            ratatui::style::Color::Indexed(index) => Self::Indexed(index),
            _ => Self::Default,
        }
    }
}

/// One styled cell of an output frame (same shape as the parity golden
/// cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// ECMA-48 attribute bits (1=bold .. 9=crossed out), matching the
    /// canonical serializer's bit order.
    pub attrs: u16,
    pub fg: CellColor,
    pub bg: CellColor,
}

impl Default for Cell {
    fn default() -> Self {
        Self { ch: ' ', attrs: 0, fg: CellColor::Default, bg: CellColor::Default }
    }
}

/// A painted frame: `height` rows of `width` cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub cells: Vec<Vec<Cell>>,
    pub width: usize,
    pub height: usize,
}

/// The wide-cluster continuation marker: a cell holding this char is
/// the second (or later) cell of a double-width glyph and emits
/// nothing in either golden format.
pub const CONTINUATION: char = '\u{0}';

impl Frame {
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.cells.get(row).and_then(|line| line.get(col)).copied().unwrap_or_default()
    }

    /// The frame as a text grid (glyphs only), one line per row — the
    /// `.txt` golden format. Wide-cluster continuation cells (the
    /// cells after a double-width glyph) carry `'\0'` and are skipped,
    /// exactly like the exporter's per-cell walk.
    #[must_use]
    pub fn text_grid(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|line| {
                line.iter().filter(|cell| cell.ch != CONTINUATION).map(|cell| cell.ch).collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_grid_is_glyph_only() {
        let frame = Frame {
            cells: vec![vec![Cell { ch: 'a', ..Cell::default() }, Cell::default()]],
            width: 2,
            height: 1,
        };
        assert_eq!(frame.text_grid(), vec!["a ".to_owned()]);
        assert_eq!(frame.cell(0, 0).ch, 'a');
        assert_eq!(frame.cell(9, 9), Cell::default(), "out of bounds reads as blank");
    }
}
