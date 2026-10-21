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
