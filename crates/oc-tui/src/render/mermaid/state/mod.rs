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

//! `state/diagram.ts` + `state/render-grid.ts` — the top-level state
//! renderer entry points.

pub mod drawing;
pub mod endpoint;
pub mod layout;
pub mod note;
pub mod parser;
pub mod routing;
pub mod search;
pub mod style;
pub mod types;
pub mod visible_model;

#[cfg(test)]
mod tests;

use crate::render::mermaid::core::canvas::TextOptions;
pub use crate::render::mermaid::state::drawing::{
    create_state_diagram_drawing, draw_state_diagram_grid, StateDiagramDrawing,
};
pub use crate::render::mermaid::state::parser::{
    is_mermaid_state_diagram, parse_mermaid_state_diagram,
};
pub use crate::render::mermaid::state::types::{StateDiagram, StateDiagramRenderOptions};

/// `renderStateGridText`.
#[must_use]
pub fn render_state_grid_text(grid: &drawing::StateGrid) -> String {
    grid.to_text(TextOptions { trim_top: true, trim_bottom: true })
}

/// `renderStateDiagram`.
pub fn render_state_diagram(
    content: &str,
    options: &StateDiagramRenderOptions,
) -> Result<String, crate::render::mermaid::state::types::StateRenderError> {
    let diagram = parse_mermaid_state_diagram(content)?;
    let grid = draw_state_diagram_grid(&diagram, options)?;
    Ok(render_state_grid_text(&grid))
}
