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

impl From<crate::style::Color> for CellColor {
    fn from(color: crate::style::Color) -> Self {
        match color {
            crate::style::Color::Rgb(r, g, b) => Self::Rgb(r, g, b),
            crate::style::Color::Indexed(index) => Self::Indexed(index),
            crate::style::Color::Reset => Self::Default,
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
#[derive(Debug, Clone)]
pub struct Frame {
    pub cells: Vec<Vec<Cell>>,
    pub width: usize,
    pub height: usize,
    /// Emission sidecar (lane B3b): the full grapheme-cluster symbol
    /// planted at `(y, x)` for every cell whose paint symbol holds
    /// MORE than one codepoint (`ZWJ` families, `VS16` flags, `NFD`
    /// combining sequences). `Cell::ch` keeps only the cluster's
    /// first char — the golden vocabulary is char-per-cell and stays
    /// byte-identical — while the `GridEmitter` prints the full symbol
    /// (the retired ratatui backend's observable behavior: it wrote
    /// the whole buffer symbol per cell). `None` = no multi-codepoint
    /// clusters (the common frame; boxed to keep clones cheap).
    pub clusters: Option<Box<ClusterMap>>,
}

/// Multi-codepoint cluster positions of a [`Frame`]: `(y, x)` → the
/// cell's full paint symbol.
pub type ClusterMap = std::collections::BTreeMap<(usize, usize), String>;

impl PartialEq for Frame {
    /// Cells-only: the cluster sidecar is emission metadata, not
    /// golden vocabulary, so frame equality (the golden rigs and the
    /// component tests) ignores it.
    fn eq(&self, other: &Self) -> bool {
        self.cells == other.cells && self.width == other.width && self.height == other.height
    }
}

impl Eq for Frame {}

/// The wide-cluster continuation marker: a cell holding this char is
/// the second (or later) cell of a double-width glyph and emits
/// nothing in either golden format.
pub const CONTINUATION: char = '\u{0}';

impl Frame {
    /// Paste `region`'s cells into `self` at (`x`, `y`), clipped;
    /// anything left keeps its fill. The region's cluster sidecar
    /// rides along (offset by the paste), and any stale target
    /// cluster inside the pasted rectangle is dropped — the cells it
    /// described were just overwritten (lane `B3b`: the `GridEmitter`
    /// prints sidecar clusters, so the frame assembly must not lose
    /// them on the way up).
    pub fn paste_region(&mut self, region: &Frame, x: usize, y: usize) {
        for (row, cells) in region.cells.iter().enumerate() {
            let Some(slot) = self.cells.get_mut(y + row) else { break };
            let count = cells.len().min(slot.len().saturating_sub(x));
            slot[x..x + count].clone_from_slice(&cells[..count]);
        }
        if region.clusters.is_none() && self.clusters.is_none() {
            return;
        }
        let mut merged = self.clusters.take().unwrap_or_default();
        for (row, cells) in region.cells.iter().enumerate() {
            let ty = y + row;
            if ty >= self.height {
                break;
            }
            for col in 0..cells.len() {
                let tx = x + col;
                if tx >= self.width {
                    break;
                }
                match region.cluster_at(row, col) {
                    Some(symbol) => {
                        merged.insert((ty, tx), symbol.to_owned());
                    }
                    None => {
                        merged.remove(&(ty, tx));
                    }
                }
            }
        }
        if !merged.is_empty() {
            self.clusters = Some(merged);
        }
    }

    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.cells.get(row).and_then(|line| line.get(col)).copied().unwrap_or_default()
    }

    /// The full cluster symbol planted at `(y, x)`, if any (the
    /// emission sidecar; `None` = a single-codepoint cell — print
    /// [`Cell::ch`]).
    #[must_use]
    pub fn cluster_at(&self, y: usize, x: usize) -> Option<&str> {
        self.clusters.as_ref()?.get(&(y, x)).map(String::as_str)
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
            clusters: None,
        };
        assert_eq!(frame.text_grid(), vec!["a ".to_owned()]);
        assert_eq!(frame.cell(0, 0).ch, 'a');
        assert_eq!(frame.cell(9, 9), Cell::default(), "out of bounds reads as blank");
    }
}
