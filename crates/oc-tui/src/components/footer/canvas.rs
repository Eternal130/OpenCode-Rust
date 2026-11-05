//! Footer painting canvas: the frame the footer family renders into.
//!
//! The mini footer renders over the terminal fill (default foreground
//! over the terminal background — black in the golden dump harness),
//! paints **surface rows** (panel backgrounds), **transparent regions**
//! (the composer textarea; canonical bg `49`), and finally text spans
//! whose grapheme clusters advance by cluster width (the same
//! [`crate::wrap::flatten`] seam as the layout layer).

use crate::render::{Cell, CellColor, Frame};
use crate::style::Style;
use crate::wrap::{display_width, flatten};

/// The dump harness renderer fill: default foreground over the
/// terminal background (black in the reference capture).
pub const FILL_FG: CellColor = CellColor::Rgb(255, 255, 255);
pub const FILL_BG: CellColor = CellColor::Rgb(0, 0, 0);

/// One styled run of a footer line: text under a foreground,
/// background and BOLD attribute (the canonical cell vocabulary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub fg: CellColor,
    pub bg: CellColor,
    pub bold: bool,
}

impl Span {
    #[must_use]
    pub fn new(text: impl Into<String>, fg: CellColor, bg: CellColor) -> Self {
        Self { text: text.into(), fg, bg, bold: false }
    }

    #[must_use]
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    #[must_use]
    pub fn width(&self) -> usize {
        display_width(&self.text)
    }
}

/// One recorded painting operation — the painter-agnostic op stream
/// the opentui-solid bridge (lane R2 step 2) replays as absolute
/// elements. D4.1a: the op stream IS the canvas (no write-beside
/// cell buffer); `into_frame` replays the ops when the mini footer
/// (D4.1c pending) needs cells.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasOp {
    /// `fill_bg`/`clear_bg` — a 1-row background segment (`bg ==
    /// Default` marks a TRANSPARENT region, applied as a bridge-level
    /// fixup: opentui cannot express bg-reset).
    Fill { y: usize, x: usize, width: usize, bg: CellColor },
    /// `spans` — one styled line at `(x, y)`.
    Spans { y: usize, x: usize, spans: Vec<Span> },
}

/// A footer frame under construction (D4.1a: an OP STREAM, not a
/// double-painted buffer — the cells materialize only behind
/// [`Canvas::into_frame`], the mini footer's channel until lane
/// D4.1c; the footer family's live path consumes [`Canvas::ops`]
/// alone).
#[derive(Clone)]
pub struct Canvas {
    width: usize,
    height: usize,
    ops: Vec<CanvasOp>,
}

/// Replay the op stream over a pre-filled frame — the exact writes
/// the retired write-beside painting performed, in record order.
fn paint_ops(frame: &mut Frame, ops: &[CanvasOp]) {
    for op in ops {
        match op {
            CanvasOp::Fill { y, x, width, bg } => {
                if let Some(line) = frame.cells.get_mut(*y) {
                    for cell in line.iter_mut().skip(*x).take(*width) {
                        cell.bg = *bg;
                    }
                }
            }
            CanvasOp::Spans { y, x, spans } => {
                let mut x = *x;
                for span in spans {
                    let attrs = u16::from(span.bold);
                    for atom in flatten(&span.text, Style::new()) {
                        if let Some(line) = frame.cells.get_mut(*y) {
                            if let Some(cell) = line.get_mut(x) {
                                *cell = Cell {
                                    ch: atom.text.chars().next().unwrap_or(' '),
                                    attrs,
                                    fg: span.fg,
                                    bg: span.bg,
                                };
                            }
                        }
                        x += atom.width.max(1);
                    }
                }
            }
        }
    }
}

impl Canvas {
    /// A `width` × `height` frame pre-filled with the terminal style
    /// (default fg white over black).
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self { width, height, ops: Vec::new() }
    }

    /// The canvas dimensions.
    #[must_use]
    pub fn dims(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    /// The recorded op stream (the solid bridge's input).
    #[must_use]
    pub fn ops(&self) -> &[CanvasOp] {
        &self.ops
    }

    /// Paint a row segment's background (surface / pane / focused),
    /// keeping the fill foreground.
    pub fn fill_bg(&mut self, y: usize, x: usize, width: usize, bg: CellColor) {
        self.ops.push(CanvasOp::Fill { y, x, width, bg });
    }

    /// Paint a transparent region: canonical background `49` (the
    /// composer textarea resets the cell background).
    pub fn clear_bg(&mut self, y: usize, x: usize, width: usize) {
        self.fill_bg(y, x, width, CellColor::Default);
    }

    /// Paint one line of spans at `(x, y)`, advancing by grapheme
    /// cluster width (leading cell carries the cluster's first char).
    pub fn spans(&mut self, y: usize, x: usize, spans: &[Span]) {
        self.ops.push(CanvasOp::Spans { y, x, spans: spans.to_vec() });
    }

    /// Right-align `spans` so the last cell lands at `right_x`.
    pub fn spans_right(&mut self, y: usize, right_x: usize, spans: &[Span]) -> usize {
        let total: usize = spans.iter().map(Span::width).sum();
        let start = right_x.saturating_sub(total.saturating_sub(1));
        self.spans(y, start, spans);
        start
    }

    /// Materialize the canvas's cells (D4.1c pending: the MINI
    /// footer's channel; the footer family rides the shared tree via
    /// `ops`). The pre-filled terminal style + the op replay —
    /// byte-identical to the retired write-beside painting.
    #[must_use]
    pub fn into_frame(self) -> Frame {
        let fill = Cell { ch: ' ', attrs: 0, fg: FILL_FG, bg: FILL_BG };
        let mut frame = Frame {
            cells: vec![vec![fill; self.width]; self.height],
            width: self.width,
            height: self.height,
            clusters: None,
        };
        paint_ops(&mut frame, &self.ops);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_advance_by_cluster_width() {
        let mut canvas = Canvas::new(10, 1);
        canvas.spans(0, 0, &[Span::new("a数据b", CellColor::Rgb(1, 2, 3), FILL_BG)]);
        let frame = canvas.into_frame();
        // a(1) + 数(2) + 据(2) … b lands at x=5.
        assert_eq!(frame.cell(0, 0).ch, 'a');
        assert_eq!(frame.cell(0, 1).ch, '数');
        assert_eq!(frame.cell(0, 2).ch, ' ', "wide cluster continuation");
        assert_eq!(frame.cell(0, 5).ch, 'b');
        assert_eq!(frame.cell(0, 5).fg, CellColor::Rgb(1, 2, 3));
    }

    #[test]
    fn right_alignment_lands_the_last_cell() {
        let mut canvas = Canvas::new(20, 1);
        let start = canvas.spans_right(0, 10, &[Span::new("current", CellColor::Default, FILL_BG)]);
        assert_eq!(start, 4, "'current' ends at x=10");
        let frame = canvas.into_frame();
        assert_eq!(frame.cell(0, 10).ch, 't');
    }
}
