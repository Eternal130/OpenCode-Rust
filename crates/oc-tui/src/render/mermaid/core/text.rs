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

//! `core/text.ts` — label measurement and text-box sizing.

pub use crate::render::mermaid::core::text_lines::split_diagram_lines;

/// `diagramTextWidth` — the string-width equivalent (unicode-width over
/// the grapheme stream).
#[must_use]
pub fn diagram_text_width(value: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    value.width()
}

pub const DIAGRAM_LABEL_PADDING_X: usize = 1;

/// `padDiagramLabelLine`.
#[must_use]
pub fn pad_diagram_label_line(value: &str) -> String {
    format!(" {value} ")
}

/// `measureDiagramLabelWidth`.
#[must_use]
pub fn measure_diagram_label_width(value: &str) -> usize {
    split_diagram_lines(value)
        .into_iter()
        .map(|line| diagram_text_width(&line) + DIAGRAM_LABEL_PADDING_X * 2)
        .max()
        .unwrap_or(0)
}

/// `diagramTextGraphemes` — yields &str grapheme clusters.
pub fn diagram_text_graphemes(value: &str) -> impl Iterator<Item = &str> {
    use unicode_segmentation::UnicodeSegmentation;
    value.graphemes(true)
}

/// `DiagramTextBoxSize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBoxSize {
    pub width: usize,
    pub height: usize,
    pub lines: Vec<String>,
}

/// `measureDiagramTextBox`.
#[must_use]
pub fn measure_diagram_text_box(
    value: &str,
    padding_x: usize,
    padding_y: usize,
    min_inner_width: usize,
) -> TextBoxSize {
    let lines = split_diagram_lines(value);
    let inner_width = lines
        .iter()
        .map(|line| diagram_text_width(line))
        .max()
        .map_or(min_inner_width.max(1), |width| width.max(min_inner_width));
    TextBoxSize { width: inner_width + padding_x * 2, height: lines.len() + padding_y * 2, lines }
}
