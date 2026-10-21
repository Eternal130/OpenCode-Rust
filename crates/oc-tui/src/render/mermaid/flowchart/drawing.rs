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

//! `flowchart/drawing.ts` — the grid painter.

use crate::render::mermaid::core::canvas::DiagramCanvas;
use crate::render::mermaid::core::drawing::{
    diagram_arrow_head_between, diagram_line_glyph, diamond_characters_from_border,
    draw_diagram_diamond, draw_diagram_frame, draw_orthogonal_path, fill_diagram_frame_interior,
    is_arrow_head, BorderCharacters, BORDER_ROUNDED,
};
use crate::render::mermaid::core::geometry::walk_orthogonal_segment;
use crate::render::mermaid::core::geometry::Direction;
use crate::render::mermaid::core::text::DIAGRAM_LABEL_PADDING_X;
use crate::render::mermaid::core::text_lines::parse_diagram_text_lines;
use crate::render::mermaid::flowchart::flowchart_routing_direction_between as flowchart_direction_between;
use crate::render::mermaid::flowchart::labels::flowchart_route_label_layout;
use crate::render::mermaid::flowchart::layout::{layout_flowchart_diagram, visual_length};
use crate::render::mermaid::flowchart::routing::flowchart_source_connector;
use crate::render::mermaid::flowchart::style::{merge_flowchart_cell, FlowchartStyle};
use crate::render::mermaid::flowchart::types::{
    EdgeRoute, FlowchartDiagram, FlowchartNode, FlowchartNodeShape, FlowchartRenderOptions,
    NodeBounds,
};
use std::collections::{HashMap, HashSet};

pub const ITALIC_ATTRIBUTE: u8 = 0x04;

fn set_rich_text(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    x: i64,
    y: i64,
    runs: &[crate::render::mermaid::core::text_lines::TextRun],
    style: Option<FlowchartStyle>,
) -> i64 {
    let mut offset = 0i64;
    for run in runs {
        let italic = run.italic;
        grid.set_text_with_metadata(x + offset, y, &run.text, style, |_, _| {
            italic.then_some(ITALIC_ATTRIBUTE)
        });
        offset += visual_length(&run.text);
    }
    offset
}

fn draw_node(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    node: &FlowchartNode,
    bounds: &NodeBounds,
    border_chars: &BorderCharacters,
) {
    let style: Option<FlowchartStyle> = Some(if node.shape == FlowchartNodeShape::Database {
        FlowchartStyle::Database
    } else {
        FlowchartStyle::Node
    });
    let border_style: Option<FlowchartStyle> =
        Some(if node.shape == FlowchartNodeShape::Database {
            FlowchartStyle::DatabaseBorder
        } else {
            FlowchartStyle::NodeBorder
        });
    let rect = bounds.bounds;

    match node.shape {
        FlowchartNodeShape::Decision => {
            let chars = diamond_characters_from_border(border_chars);
            draw_diagram_diamond(
                rect,
                &mut |x, y, char| grid.set_cell(x, y, char, border_style),
                &chars,
            );
        }
        FlowchartNodeShape::Subroutine => {
            fill_diagram_frame_interior(rect, &mut |x, y| grid.set_cell(x, y, " ", style));
            draw_subroutine_node(grid, bounds, border_chars, border_style);
        }
        FlowchartNodeShape::Database => {
            fill_diagram_frame_interior(rect, &mut |x, y| grid.set_cell(x, y, " ", style));
            draw_database_node(grid, bounds, border_chars, border_style);
        }
        _ => {
            fill_diagram_frame_interior(rect, &mut |x, y| grid.set_cell(x, y, " ", style));
            draw_diagram_frame(rect, border_chars, &mut |x, y, char| {
                grid.set_cell(x, y, char, border_style);
            });
        }
    }

    let text_top = match node.shape {
        FlowchartNodeShape::Decision => {
            rect.top + (rect.height - bounds.size.lines.len() as i64).div_euclid(2)
        }
        FlowchartNodeShape::Database => rect.top + 2,
        _ => rect.top + 1,
    };
    let lines = parse_diagram_text_lines(&node.label);
    for (index, measured) in bounds.size.lines.iter().enumerate() {
        let line_width = visual_length(measured);
        let line_x = if node.shape == FlowchartNodeShape::Subroutine {
            rect.left + 3
        } else {
            rect.left + 1.max((rect.width - line_width).div_euclid(2))
        };
        let runs = lines.get(index).map(|line| line.runs.as_slice()).unwrap_or(&[]);
        set_rich_text(grid, line_x, text_top + index as i64, runs, style);
    }
}

fn draw_subroutine_node(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    bounds: &NodeBounds,
    chars: &BorderCharacters,
    style: Option<FlowchartStyle>,
) {
    let rect = bounds.bounds;
    draw_diagram_frame(rect, chars, &mut |x, y, char| grid.set_cell(x, y, char, style));
    let left_rail_x = rect.left + 2;
    let right_rail_x = rect.left + rect.width - 3;
    grid.set_cell(left_rail_x, rect.top, chars.top_t, style);
    grid.set_cell(right_rail_x, rect.top, chars.top_t, style);
    grid.set_cell(left_rail_x, rect.top + rect.height - 1, chars.bottom_t, style);
    grid.set_cell(right_rail_x, rect.top + rect.height - 1, chars.bottom_t, style);
    for y in (rect.top + 1)..(rect.top + rect.height - 1) {
        grid.set_cell(left_rail_x, y, chars.vertical, style);
        grid.set_cell(right_rail_x, y, chars.vertical, style);
    }
}

fn draw_database_node(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    bounds: &NodeBounds,
    chars: &BorderCharacters,
    style: Option<FlowchartStyle>,
) {
    let rect = bounds.bounds;
    draw_diagram_frame(rect, chars, &mut |x, y, char| grid.set_cell(x, y, char, style));
    let top_rail_y = rect.top + 1;
    let bottom_rail_y = rect.top + rect.height - 2;
    for y in [top_rail_y, bottom_rail_y] {
        grid.set_cell(rect.left, y, chars.left_t, style);
        grid.set_cell(rect.left + rect.width - 1, y, chars.right_t, style);
        for x in (rect.left + 1)..(rect.left + rect.width - 1) {
            grid.set_cell(x, y, chars.horizontal, style);
        }
    }
}

fn draw_subgraph_frame(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    bounds: &crate::render::mermaid::flowchart::types::SubgraphBounds,
    border_chars: &BorderCharacters,
) {
    draw_diagram_frame(bounds.bounds, border_chars, &mut |x, y, char| {
        grid.set_cell(x, y, char, Some(FlowchartStyle::Group));
    });
}

fn draw_subgraph_label(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    bounds: &crate::render::mermaid::flowchart::types::SubgraphBounds,
) {
    if !bounds.label.is_empty() {
        let lines = parse_diagram_text_lines(&bounds.label);
        let label_y = if bounds.label_side_top {
            bounds.bounds.top
        } else {
            bounds.bounds.top + bounds.bounds.height - lines.len() as i64
        };
        for (index, line) in lines.iter().enumerate() {
            grid.set_text(
                bounds.bounds.left + 2,
                label_y + index as i64,
                " ",
                Some(FlowchartStyle::GroupLabel),
            );
            let width = set_rich_text(
                grid,
                bounds.bounds.left + 3,
                label_y + index as i64,
                &line.runs,
                Some(FlowchartStyle::GroupLabel),
            );
            grid.set_text(
                bounds.bounds.left + width + 3,
                label_y + index as i64,
                " ",
                Some(FlowchartStyle::GroupLabel),
            );
        }
    }
}

fn draw_edge_label(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    route: &EdgeRoute,
    label: &str,
    style: FlowchartStyle,
) {
    let layout =
        flowchart_route_label_layout(route.label_point, &route.points, label, route.label_axis);
    let padding = " ".repeat(DIAGRAM_LABEL_PADDING_X);
    for (index, line) in parse_diagram_text_lines(label).into_iter().enumerate() {
        grid.set_text(layout.point.x, layout.point.y + index as i64, &padding, Some(style));
        let width = set_rich_text(
            grid,
            layout.point.x + DIAGRAM_LABEL_PADDING_X as i64,
            layout.point.y + index as i64,
            &line.runs,
            Some(style),
        );
        grid.set_text(
            layout.point.x + width + DIAGRAM_LABEL_PADDING_X as i64,
            layout.point.y + index as i64,
            &padding,
            Some(style),
        );
    }
}

fn draw_routed_edge(grid: &mut DiagramCanvas<FlowchartStyle>, route: &EdgeRoute, thick: bool) {
    let points = &route.points;
    if points.len() < 2 {
        return;
    }
    let style = Some(FlowchartStyle::Edge);

    draw_orthogonal_path(
        points,
        &mut |x, y, char| grid.set_cell(x, y, char, style),
        crate::render::mermaid::core::drawing::CornerStyle::Rounded,
        if thick {
            crate::render::mermaid::core::drawing::LineStyle::Heavy
        } else {
            crate::render::mermaid::core::drawing::LineStyle::Single
        },
    );
    let end = points[points.len() - 1];
    let arrow_from = points[points.len() - 2];
    grid.set_cell(
        end.x,
        end.y,
        diagram_arrow_head_between(
            arrow_from,
            end,
            crate::render::mermaid::core::drawing::ArrowHeadStyle::Filled,
        ),
        style,
    );
}

#[allow(dead_code)]
fn draw_routed_edge_tail(grid: &mut DiagramCanvas<FlowchartStyle>, route: &EdgeRoute) {
    let points = &route.points;
    if points.len() >= 2 {
        let end = points[points.len() - 1];
        if let Some(end_direction) = flowchart_direction_between(points[points.len() - 2], end) {
            let mut directions = std::collections::HashSet::new();
            directions.insert(end_direction);
            grid.set_cell(
                end.x,
                end.y,
                diagram_line_glyph(
                    &directions,
                    crate::render::mermaid::core::drawing::CornerStyle::Square,
                    crate::render::mermaid::core::drawing::LineStyle::Single,
                ),
                Some(FlowchartStyle::Edge),
            );
        }
    }
}

fn source_fade_styles(database: bool) -> [FlowchartStyle; 5] {
    let mut styles = Vec::with_capacity(5);
    for step in 1..=5u8 {
        styles.push(if database {
            FlowchartStyle::DatabaseEdgeFade(step)
        } else {
            FlowchartStyle::NodeEdgeFade(step)
        });
    }
    styles.try_into().unwrap()
}

fn route_cell_occupancy(routes: &[EdgeRoute]) -> HashMap<(i64, i64), i64> {
    let mut occupancy: HashMap<(i64, i64), i64> = HashMap::new();
    for route in routes {
        let mut route_cells: HashSet<(i64, i64)> = HashSet::new();
        for window in route.points.windows(2) {
            let first = window[0] == route.points[0]
                && window[1] == route.points.get(1).copied().unwrap_or(window[1]);
            walk_orthogonal_segment(window[0], window[1], first, &mut |point| {
                route_cells.insert((point.x, point.y));
                true
            });
        }
        for key in route_cells {
            *occupancy.entry(key).or_default() += 1;
        }
    }
    occupancy
}

#[allow(clippy::too_many_lines)]
fn draw_source_connectors(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    routes: &[EdgeRoute],
) {
    let nodes_by_id: HashMap<&str, &FlowchartNode> =
        diagram.nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    let occupancy = route_cell_occupancy(routes);
    let mut source_junctions: HashMap<(String, i64, i64), (HashSet<Direction>, bool)> =
        HashMap::new();
    for route in routes {
        let Some(source_point) = route.points.first() else { continue };
        let Some(next) = route.points.get(1) else { continue };
        let from = &diagram.edges[route.edge_index].from;
        let key = (from.clone(), source_point.x, source_point.y);
        let direction = flowchart_direction_between(*source_point, *next);
        let junction = source_junctions.entry(key).or_insert_with(|| (HashSet::new(), true));
        if let Some(direction) = direction {
            junction.0.insert(direction);
        }
        junction.1 &= diagram.edges[route.edge_index].style
            == Some(crate::render::mermaid::flowchart::types::EdgeStyle::Thick);
    }

    for route in routes {
        let edge = &diagram.edges[route.edge_index];
        let Some(from) = bounds.get(&edge.from) else { continue };
        let Some(source_point) = route.points.first() else { continue };
        let database = nodes_by_id
            .get(edge.from.as_str())
            .is_some_and(|node| node.shape == FlowchartNodeShape::Database);
        let styles = source_fade_styles(database);
        let (connector_x, connector_y, connector_char) =
            flowchart_source_connector(from, *source_point);
        grid.set_cell(connector_x, connector_y, connector_char, Some(FlowchartStyle::Edge));
        if let Some(connector_direction) = flowchart_direction_between(
            *source_point,
            crate::render::mermaid::core::geometry::point(connector_x, connector_y),
        ) {
            let cell = grid.get_cell(source_point.x, source_point.y);
            if cell.is_some_and(|cell| {
                cell.style != Some(FlowchartStyle::Label) && !is_arrow_head(cell.text())
            }) {
                let key = (edge.from.clone(), source_point.x, source_point.y);
                let junction = source_junctions.get(&key);
                let mut directions =
                    junction.map_or_else(HashSet::new, |junction| junction.0.clone());
                directions.insert(connector_direction);
                let heavy = junction.is_some_and(|junction| junction.1)
                    || edge.style
                        == Some(crate::render::mermaid::flowchart::types::EdgeStyle::Thick);
                grid.replace_cell(
                    source_point.x,
                    source_point.y,
                    diagram_line_glyph(
                        &directions,
                        crate::render::mermaid::core::drawing::CornerStyle::Rounded,
                        if heavy {
                            crate::render::mermaid::core::drawing::LineStyle::Heavy
                        } else {
                            crate::render::mermaid::core::drawing::LineStyle::Single
                        },
                    ),
                    Some(FlowchartStyle::Edge),
                );
            }
        }
        fade_source_path(grid, connector_x, connector_y, &route.points, &styles, &occupancy);
        if edge.source_arrowhead {
            if let Some(next) = route.points.get(1) {
                grid.set_cell(
                    source_point.x,
                    source_point.y,
                    diagram_arrow_head_between(
                        *next,
                        *source_point,
                        crate::render::mermaid::core::drawing::ArrowHeadStyle::Filled,
                    ),
                    Some(FlowchartStyle::Edge),
                );
            }
        }
    }
}

fn fade_source_path(
    grid: &mut DiagramCanvas<FlowchartStyle>,
    connector_x: i64,
    connector_y: i64,
    points: &[PointAlias],
    styles: &[FlowchartStyle; 5],
    occupancy: &HashMap<(i64, i64), i64>,
) {
    use crate::render::mermaid::core::geometry::Point;
    let Some(from) = points.first() else { return };
    let Some(to) = points.get(1) else { return };
    let connector = Point { x: connector_x, y: connector_y };
    let mut private_cells: Vec<Point> = vec![connector];

    walk_orthogonal_segment(*from, *to, true, &mut |point| {
        let key = (point.x, point.y);
        let cell = grid.get_cell(point.x, point.y);
        if occupancy.get(&key) != Some(&1)
            || cell.is_none_or(|cell| {
                !"─━│┃".contains(cell.text()) || cell.style == Some(FlowchartStyle::Label)
            })
        {
            return false;
        }
        private_cells.push(point);
        true
    });

    for (index, point) in private_cells.iter().enumerate() {
        let style_index = (styles.len().saturating_sub(1))
            .min(((index + 1) * styles.len() / (private_cells.len() + 1)) as usize);
        if let Some(text) = grid.get_cell(point.x, point.y).map(|cell| cell.text().to_string()) {
            grid.set_cell(point.x, point.y, &text, Some(styles[style_index]));
        }
    }
}

type PointAlias = crate::render::mermaid::core::geometry::Point;

/// `drawFlowchartDiagramGrid`.
#[allow(clippy::too_many_lines)]
pub fn draw_flowchart_diagram_grid(
    diagram: &FlowchartDiagram,
    options: &FlowchartRenderOptions,
) -> Result<DiagramCanvas<FlowchartStyle>, crate::render::mermaid::core::canvas::CanvasSizeError> {
    let border_chars = &BORDER_ROUNDED;
    let layout = layout_flowchart_diagram(diagram, options);
    let width = layout.width;
    let height = layout.height;
    let diagram = &layout.diagram;
    let mut grid = DiagramCanvas::new(width, height, Some(merge_flowchart_cell))?;

    for subgraph in &diagram.subgraphs {
        if let Some(bound) = layout.subgraph_bounds.get(&subgraph.id) {
            draw_subgraph_frame(&mut grid, bound, border_chars);
        }
    }
    for route in &layout.routes {
        let edge = &diagram.edges[route.edge_index];
        draw_routed_edge(
            &mut grid,
            route,
            edge.style == Some(crate::render::mermaid::flowchart::types::EdgeStyle::Thick),
        );
        if !edge.arrowhead {
            draw_routed_edge_tail(&mut grid, route);
        }
        if !edge.label.is_empty() {
            draw_edge_label(&mut grid, route, &edge.label, FlowchartStyle::Label);
        }
    }
    for node in &diagram.nodes {
        if let Some(bound) = layout.bounds.get(&node.id) {
            draw_node(&mut grid, node, bound, border_chars);
        }
    }
    draw_source_connectors(&mut grid, diagram, &layout.bounds, &layout.routes);
    for subgraph in &diagram.subgraphs {
        if let Some(bound) = layout.subgraph_bounds.get(&subgraph.id) {
            draw_subgraph_label(&mut grid, bound);
        }
    }
    Ok(grid)
}
