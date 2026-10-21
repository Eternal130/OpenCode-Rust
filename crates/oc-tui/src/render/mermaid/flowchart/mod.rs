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

//! `flowchart/` — types, parser, labels, layout, routing, style,
//! drawing.

pub mod drawing;
pub mod labels;
pub mod layout;
pub mod parser;
pub mod routing;
pub mod style;
pub mod types;

pub use drawing::draw_flowchart_diagram_grid;
pub use layout::layout_flowchart_diagram;
pub use parser::{is_mermaid_flowchart_diagram, parse_mermaid_flowchart_diagram};

use crate::render::mermaid::core::geometry::Direction;

pub fn flowchart_routing_direction_between(
    from: crate::render::mermaid::core::geometry::Point,
    to: crate::render::mermaid::core::geometry::Point,
) -> Option<Direction> {
    crate::render::mermaid::core::geometry::direction_between(from, to)
}
