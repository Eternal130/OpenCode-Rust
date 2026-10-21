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

//! `flowchart/style.ts` — the cell-style enum + color resolution +
//! `render-grid.ts` (styled rows out of the grid).

use crate::render::mermaid::core::canvas::Cell;
use crate::render::mermaid::core::color::{color_ramp, Rgba, DIAGRAM_FADE_STEPS};

/// `FlowchartCellStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowchartStyle {
    Node,
    NodeBorder,
    Database,
    DatabaseBorder,
    Edge,
    Label,
    Group,
    GroupLabel,
    NodeEdgeFade(u8),
    DatabaseEdgeFade(u8),
}

/// `DEFAULT_THEME_RGB`.
const DEFAULT_THEME: [(u8, u8, u8); 8] = [
    (228, 239, 232), // node
    (141, 163, 151), // nodeBorder
    (228, 239, 232), // database
    (141, 163, 151), // databaseBorder
    (134, 225, 200), // edge
    (134, 225, 200), // label
    (76, 99, 89),    // group
    (76, 99, 89),    // groupLabel
];

fn default_rgb(index: usize) -> Rgba {
    let (r, g, b) = DEFAULT_THEME[index];
    Rgba::from_ints(r, g, b, 255)
}

/// `FlowchartStyleColors` — every style slot resolved.
#[derive(Debug, Clone, Copy)]
pub struct FlowchartColors {
    pub node: Rgba,
    pub node_border: Rgba,
    pub database: Rgba,
    pub database_border: Rgba,
    pub edge: Rgba,
    pub label: Rgba,
    pub group: Rgba,
    pub group_label: Rgba,
    pub node_edge_fades: [Rgba; DIAGRAM_FADE_STEPS],
    pub database_edge_fades: [Rgba; DIAGRAM_FADE_STEPS],
}

/// The per-slot overrides the palette feeds in (all optional).
#[derive(Debug, Clone, Copy, Default)]
pub struct FlowchartColorInput {
    pub node: Option<Rgba>,
    pub node_border: Option<Rgba>,
    pub database: Option<Rgba>,
    pub database_border: Option<Rgba>,
    pub edge: Option<Rgba>,
    pub label: Option<Rgba>,
    pub group: Option<Rgba>,
    pub group_label: Option<Rgba>,
}

impl FlowchartColors {
    #[must_use]
    pub fn resolve(input: FlowchartColorInput) -> Self {
        let node = input.node.unwrap_or_else(|| default_rgb(0));
        let node_border = input.node_border.unwrap_or_else(|| default_rgb(1));
        let database = input.database.unwrap_or_else(|| default_rgb(2));
        let database_border = input.database_border.unwrap_or_else(|| default_rgb(3));
        let edge = input.edge.unwrap_or_else(|| default_rgb(4));
        let group = input.group.unwrap_or_else(|| default_rgb(6));
        let resolve_fades = |from: Rgba| -> [Rgba; DIAGRAM_FADE_STEPS] {
            let ramp = color_ramp(DIAGRAM_FADE_STEPS, Some(from), Some(edge));
            let mut fades = [from; DIAGRAM_FADE_STEPS];
            for (index, color) in ramp.into_iter().enumerate() {
                fades[index] = color.unwrap_or(from);
            }
            fades
        };
        Self {
            node,
            node_border,
            database,
            database_border,
            edge,
            label: input.label.unwrap_or_else(|| default_rgb(5)),
            group,
            group_label: input.group_label.or(input.group).unwrap_or_else(|| default_rgb(7)),
            node_edge_fades: resolve_fades(node_border),
            database_edge_fades: resolve_fades(database_border),
        }
    }

    #[must_use]
    pub fn color(&self, style: Option<FlowchartStyle>) -> Option<Rgba> {
        Some(match style? {
            FlowchartStyle::Node => self.node,
            FlowchartStyle::NodeBorder => self.node_border,
            FlowchartStyle::Database => self.database,
            FlowchartStyle::DatabaseBorder => self.database_border,
            FlowchartStyle::Edge => self.edge,
            FlowchartStyle::Label => self.label,
            FlowchartStyle::Group => self.group,
            FlowchartStyle::GroupLabel => self.group_label,
            FlowchartStyle::NodeEdgeFade(step) => self.node_edge_fades[(step - 1) as usize],
            FlowchartStyle::DatabaseEdgeFade(step) => self.database_edge_fades[(step - 1) as usize],
        })
    }
}

/// One styled run for the markdown seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramSpan {
    pub text: String,
    pub fg: Option<Rgba>,
    pub bg: Option<Rgba>,
    pub attributes: u8,
}

/// The merge function the flowchart canvas uses
/// (`mergeFlowchartCell`).
pub fn merge_flowchart_cell(
    existing: &Cell<FlowchartStyle>,
    incoming: &Cell<FlowchartStyle>,
) -> Cell<FlowchartStyle> {
    use crate::render::mermaid::core::drawing::is_arrow_head;
    if incoming.style != Some(FlowchartStyle::Edge) {
        return incoming.clone();
    }
    if existing.style == Some(FlowchartStyle::Label) {
        return existing.clone();
    }
    if incoming.text() == " " {
        return existing.clone();
    }
    if existing.style != Some(FlowchartStyle::Edge) || existing.text() == " " {
        return incoming.clone();
    }
    if is_arrow_head(existing.text()) {
        return existing.clone();
    }
    if is_arrow_head(incoming.text()) {
        return incoming.clone();
    }
    let merged = crate::render::mermaid::core::drawing::merge_diagram_line_glyph(
        existing.text(),
        incoming.text(),
        crate::render::mermaid::core::drawing::CornerStyle::Rounded,
    );
    match merged {
        Some(glyph) => Cell::build(glyph, incoming.style, incoming.attributes),
        None => incoming.clone(),
    }
}
