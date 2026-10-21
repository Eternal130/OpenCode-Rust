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
