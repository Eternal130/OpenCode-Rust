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

//! `flowchart/types.ts` + `options.ts`.

use crate::render::mermaid::core::geometry::{Axis, Bounds, Point};

/// `FlowchartDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlowchartDirection {
    #[default]
    Td,
    Tb,
    Bt,
    Lr,
    Rl,
}

impl FlowchartDirection {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let upper = value.to_uppercase();
        Some(match upper.as_str() {
            "TB" => Self::Tb,
            "TD" => Self::Td,
            "BT" => Self::Bt,
            "LR" => Self::Lr,
            "RL" => Self::Rl,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Td => "TD",
            Self::Tb => "TB",
            Self::Bt => "BT",
            Self::Lr => "LR",
            Self::Rl => "RL",
        }
    }
}

/// `FlowchartNodeShape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowchartNodeShape {
    Box,
    Rounded,
    Database,
    Decision,
    Subroutine,
}

/// `FlowchartNode`.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowchartNode {
    pub id: String,
    pub label: String,
    pub shape: FlowchartNodeShape,
}

/// `FlowchartEdge`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlowchartEdge {
    pub from: String,
    pub to: String,
    pub label: String,
    pub style: Option<EdgeStyle>,
    /// `arrowhead: false` opt-out.
    pub arrowhead: bool,
    /// `sourceArrowhead: true` opt-in.
    pub source_arrowhead: bool,
    pub order_only: bool,
}

impl FlowchartEdge {
    pub fn new(
        from: String,
        to: String,
        label: String,
        style: Option<EdgeStyle>,
        arrowhead: bool,
        source_arrowhead: bool,
    ) -> Self {
        Self { from, to, label, style, arrowhead, source_arrowhead, order_only: false }
    }
}

/// `FlowchartEdgeStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStyle {
    Thick,
    Dashed,
}

/// `FlowchartSubgraph`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowchartSubgraph {
    pub id: String,
    pub label: String,
    pub node_ids: Vec<String>,
    pub parent_id: Option<String>,
    pub direction: Option<FlowchartDirection>,
}

/// `FlowchartDiagram`.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowchartDiagram {
    pub direction: FlowchartDirection,
    pub nodes: Vec<FlowchartNode>,
    pub edges: Vec<FlowchartEdge>,
    pub subgraphs: Vec<FlowchartSubgraph>,
}

/// `FlowchartNodeSize` + `FlowchartNodeBounds`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSize {
    pub width: i64,
    pub height: i64,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeBounds {
    pub id: String,
    pub size: NodeSize,
    pub bounds: Bounds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubgraphBounds {
    pub id: String,
    pub label: String,
    pub bounds: Bounds,
    /// `labelSide: "top" | "bottom"`.
    pub label_side_top: bool,
}

/// `FlowchartEdgeRoute`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EdgeRoute {
    pub edge_index: usize,
    pub points: Vec<Point>,
    pub label_axis: Option<Axis>,
    pub label_point: Option<Point>,
}

/// `FlowchartDiagramRenderOptions` (`options.ts`).
#[derive(Debug, Clone, Copy, Default)]
pub struct FlowchartRenderOptions {
    pub compact: Option<bool>,
    pub direction: Option<FlowchartDirection>,
    pub min_node_gap: Option<i64>,
    pub min_rank_gap: Option<i64>,
    /// Target rendered width; oversized horizontal layouts fold.
    pub layout_max_width: Option<i64>,
}
