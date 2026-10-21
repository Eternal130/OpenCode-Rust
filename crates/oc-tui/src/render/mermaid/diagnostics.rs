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

//! `diagnostics.ts` — the unsupported-syntax error + diagram kinds.

/// `MermaidDiagramKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MermaidDiagramKind {
    Flowchart,
    Sequence,
    State,
    Timeline,
    GitGraph,
    Gantt,
}

pub const MERMAID_FLOWCHART: MermaidDiagramKind = MermaidDiagramKind::Flowchart;
pub const MERMAID_SEQUENCE: MermaidDiagramKind = MermaidDiagramKind::Sequence;
pub const MERMAID_STATE: MermaidDiagramKind = MermaidDiagramKind::State;

/// `MermaidSyntaxError` — an otherwise valid diagram contains syntax
/// this renderer does not support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidSyntaxError {
    pub kind: MermaidDiagramKind,
    pub line_number: usize,
    pub source_line: String,
    pub message: String,
}

impl MermaidSyntaxError {
    pub fn new(
        kind: MermaidDiagramKind,
        line_number: usize,
        source_line: &str,
        reason: &str,
    ) -> Self {
        Self {
            kind,
            line_number,
            source_line: source_line.to_string(),
            message: format!(
                "{reason} in {kind:?} diagram at line {line_number}: \"{source_line}\""
            ),
        }
    }

    pub fn unsupported(kind: MermaidDiagramKind, line_number: usize, source_line: &str) -> Self {
        Self::new(kind, line_number, source_line, "Unsupported syntax")
    }
}

impl std::fmt::Display for MermaidSyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MermaidSyntaxError {}
