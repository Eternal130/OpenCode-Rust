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
