//! `sequence/` — types, parser, placement, drawing, style.

pub mod drawing;
pub mod parser;
pub mod placement;
pub mod style;
pub mod types;

pub use drawing::draw_sequence_diagram_grid;
pub use parser::{is_mermaid_sequence_diagram, parse_mermaid_sequence_diagram};

/// `renderSequenceDiagram` — the trimmed text form (tests).
pub fn render_sequence_diagram(
    content: &str,
    options: &types::SequenceRenderOptions,
) -> Result<String, crate::render::mermaid::diagnostics::MermaidSyntaxError> {
    let diagram = parse_mermaid_sequence_diagram(content)?;
    let grid = draw_sequence_diagram_grid(&diagram, options).map_err(|_| {
        crate::render::mermaid::diagnostics::MermaidSyntaxError::unsupported(
            crate::render::mermaid::diagnostics::MERMAID_SEQUENCE,
            0,
            "",
        )
    })?;
    Ok(grid.to_text(crate::render::mermaid::core::canvas::TextOptions::default()))
}
