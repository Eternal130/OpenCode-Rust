// Upstream-mirror lint posture: the merman port keeps the TypeScript
// arithmetic shape (JS Number semantics on an i64 grid, the same cast
// sites and long single-responsibility functions), so the pedantic
// numeric/style lints that would rewrite that shape are quieted here.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::explicit_iter_loop,
    clippy::for_kv_map,
    clippy::map_unwrap_or,
    clippy::manual_strip,
    clippy::range_plus_one,
    clippy::option_option,
    clippy::implicit_hasher,
    clippy::match_same_arms
)]

//! `core/canvas.ts` — the cell grid every diagram type draws onto.

use crate::render::mermaid::core::text::{diagram_text_graphemes, diagram_text_width};

/// The per-cell style tag (each diagram's closed enum) + the flowchart
/// italic-attribute bit (`FlowchartCellMetadata`). The glyph is an
/// owned string: entity-decoded labels produce runtime text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cell<S> {
    char: String,
    pub style: Option<S>,
    pub attributes: Option<u8>,
}

impl<S> Cell<S> {
    #[must_use]
    pub fn text(&self) -> &str {
        &self.char
    }

    /// Builds a cell with the given glyph and carried tags.
    #[must_use]
    pub fn build(char: &str, style: Option<S>, attributes: Option<u8>) -> Self {
        Self { char: char.to_string(), style, attributes }
    }

    fn space() -> Self {
        Self { char: " ".to_string(), style: None, attributes: None }
    }
}

/// `DiagramCanvasRun`.
#[derive(Debug, Clone)]
pub struct Run<S> {
    pub text: String,
    pub style: Option<S>,
    pub attributes: Option<u8>,
}

/// `DiagramCanvasTextOptions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextOptions {
    pub trim_top: bool,
    pub trim_bottom: bool,
}

/// `DiagramCanvasTextSize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSize {
    pub width: usize,
    pub height: usize,
}

pub const MAX_DIAGRAM_CELLS: usize = 250_000;

/// `DiagramCanvasSizeError` — modeled as a flag the caller maps (the
/// renderer treats it as "no diagram, fall back").
#[derive(Debug)]
pub struct CanvasSizeError {
    pub width: i64,
    pub height: i64,
}

/// `DiagramCanvas<Style, Metadata>` — `merge_cell` mirrors the JS
/// `mergeCell` option (flowchart line-glyph merging, state
/// transition-merging).
pub struct DiagramCanvas<S>
where
    S: Copy + PartialEq,
{
    width: usize,
    height: usize,
    cells: Vec<Cell<S>>,
    row_ends: Vec<usize>,
    merge_cell: Option<MergeCellFn<S>>,
}

/// Cell merge hook used when two draws claim the same cell.
type MergeCellFn<S> = fn(&Cell<S>, &Cell<S>) -> Cell<S>;

impl<S> DiagramCanvas<S>
where
    S: Copy + PartialEq,
{
    /// Creates the grid; `Err` mirrors `DiagramCanvasSizeError`.
    pub fn new(
        width: i64,
        height: i64,
        merge_cell: Option<MergeCellFn<S>>,
    ) -> Result<Self, CanvasSizeError> {
        if width < 0 || height < 0 {
            return Err(CanvasSizeError { width, height });
        }
        let width = width as usize;
        let height = height as usize;
        if width.checked_mul(height).is_none_or(|cells| cells > MAX_DIAGRAM_CELLS) {
            return Err(CanvasSizeError { width: width as i64, height: height as i64 });
        }
        Ok(Self {
            width,
            height,
            cells: vec![Cell::space(); width * height],
            row_ends: vec![0; height],
            merge_cell,
        })
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    fn row_text(&self, row: usize, row_end: usize) -> String {
        let mut text = String::new();
        for column in 0..row_end {
            text.push_str(self.cells[row * self.width + column].text());
        }
        text
    }

    fn text_row_range(&self, trim_top: bool, trim_bottom: bool) -> (usize, usize) {
        let mut start = 0;
        let mut end = self.height;
        if trim_top {
            while start < end && self.row_ends[start] == 0 {
                start += 1;
            }
        }
        if trim_bottom {
            while end > start && self.row_ends[end - 1] == 0 {
                end -= 1;
            }
        }
        (start, end)
    }

    /// `setCell` (merge on write).
    pub fn set_cell(&mut self, x: i64, y: i64, char: &str, style: Option<S>) {
        self.write_cell(x, y, char, style, None, true);
    }

    /// `setCell` with the italic-attribute metadata.
    pub fn set_cell_attributes(
        &mut self,
        x: i64,
        y: i64,
        char: &str,
        style: Option<S>,
        attributes: Option<u8>,
    ) {
        self.write_cell(x, y, char, style, attributes, true);
    }

    /// `replaceCell`.
    pub fn replace_cell(&mut self, x: i64, y: i64, char: &str, style: Option<S>) {
        self.write_cell(x, y, char, style, None, false);
    }

    fn write_cell(
        &mut self,
        x: i64,
        y: i64,
        char: &str,
        style: Option<S>,
        attributes: Option<u8>,
        merge: bool,
    ) {
        if y < 0 || y >= self.height as i64 || x < 0 || x >= self.width as i64 {
            return;
        }
        let x = x as usize;
        let y = y as usize;
        let index = y * self.width + x;
        let incoming = Cell { char: char.to_string(), style, attributes };
        let cell = if merge {
            self.merge_cell
                .map_or_else(|| incoming.clone(), |merge| merge(&self.cells[index], &incoming))
        } else {
            incoming
        };
        self.cells[index] = cell;
        let is_space = self.cells[index].text() == " ";
        if !is_space {
            self.row_ends[y] = self.row_ends[y].max(x + 1);
        } else if self.row_ends[y] == x + 1 {
            let mut end = x;
            while end > 0 && self.cells[y * self.width + end - 1].text() == " " {
                end -= 1;
            }
            self.row_ends[y] = end;
        }
    }

    /// `getCell`.
    #[must_use]
    pub fn get_cell(&self, x: i64, y: i64) -> Option<&Cell<S>> {
        if y < 0 || y >= self.height as i64 || x < 0 || x >= self.width as i64 {
            return None;
        }
        Some(&self.cells[y as usize * self.width + x as usize])
    }

    /// `setText` — ASCII fast path plus grapheme walk with continuation
    /// cells for wide glyphs.
    pub fn set_text(&mut self, x: i64, y: i64, text: &str, style: Option<S>) {
        self.set_text_with_metadata(x, y, text, style, |_, _| None);
    }

    pub fn set_text_with_metadata(
        &mut self,
        x: i64,
        y: i64,
        text: &str,
        style: Option<S>,
        attributes: impl Fn(i64, i64) -> Option<u8>,
    ) {
        if !text.is_empty() && text.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
            for (index, char) in text.chars().enumerate() {
                let text: String = char.to_string();
                self.write_cell(
                    x + index as i64,
                    y,
                    &text,
                    style,
                    attributes(x + index as i64, y),
                    true,
                );
            }
            return;
        }

        let mut offset = 0i64;
        for grapheme in diagram_text_graphemes(text) {
            let width = diagram_text_width(grapheme).max(1) as i64;
            if x + offset < 0 || x + offset + width > self.width as i64 {
                offset += width;
                continue;
            }
            self.write_cell(x + offset, y, grapheme, style, attributes(x + offset, y), true);
            for continuation in 1..width {
                self.write_cell(
                    x + offset + continuation,
                    y,
                    "",
                    style,
                    attributes(x + offset + continuation, y),
                    true,
                );
            }
            offset += width;
        }
    }

    /// `toString`.
    #[must_use]
    pub fn to_text(&self, options: TextOptions) -> String {
        let (start, end) = self.text_row_range(options.trim_top, options.trim_bottom);
        let mut lines = Vec::with_capacity(end - start);
        for row in start..end {
            lines.push(self.row_text(row, self.row_ends[row]));
        }
        lines.join("\n")
    }

    /// `getTextSize`.
    #[must_use]
    pub fn get_text_size(&self, options: TextOptions) -> TextSize {
        let (start, end) = self.text_row_range(options.trim_top, options.trim_bottom);
        let mut width = 0;
        for row in start..end {
            if self.row_ends[row] > 0 {
                width = width.max(diagram_text_width(&self.row_text(row, self.row_ends[row])));
            }
        }
        TextSize { width, height: end - start }
    }

    /// `getTextHeight`.
    #[must_use]
    pub fn get_text_height(&self, options: TextOptions) -> usize {
        let (start, end) = self.text_row_range(options.trim_top, options.trim_bottom);
        end - start
    }

    /// `forEachRun` — run-splits by `(style, attributes)` key; visits
    /// rows in the trimmed range, invoking the visitor per run and with
    /// `None` between rows (the `on_line_end` seam).
    pub fn for_each_run(&self, options: TextOptions, visit: &mut dyn FnMut(Option<Run<S>>)) {
        let (start, end) = self.text_row_range(options.trim_top, options.trim_bottom);

        for row in start..end {
            let mut row_end = self.row_ends[row];
            while row_end < self.width && self.cells[row * self.width + row_end].style.is_some() {
                row_end += 1;
            }

            let mut current: Option<(Option<S>, Option<u8>, String)> = None;
            for x in 0..row_end {
                let cell = &self.cells[row * self.width + x];
                let same_run = current.as_ref().is_some_and(|(style, attributes, _)| {
                    *style == cell.style && *attributes == cell.attributes
                });
                if !same_run {
                    if let Some((style, attributes, text)) = current.take() {
                        if !text.is_empty() {
                            visit(Some(Run { text, style, attributes }));
                        }
                    }
                    current = Some((cell.style, cell.attributes, String::new()));
                }
                if let Some((_, _, text)) = current.as_mut() {
                    text.push_str(cell.text());
                }
            }
            if let Some((style, attributes, text)) = current.take() {
                if !text.is_empty() {
                    visit(Some(Run { text, style, attributes }));
                }
            }
            if row + 1 < end {
                visit(None);
            }
        }
    }
}
