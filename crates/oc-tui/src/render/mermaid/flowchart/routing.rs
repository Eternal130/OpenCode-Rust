//! `flowchart/routing.ts` — edge path selection: parallel routes,
//! fan-in/fan-out buses, back edges, obstacle avoidance, and label
//! collision repair.

use std::collections::{HashMap, HashSet};

use crate::render::mermaid::core::geometry::{
    advance_coordinate, after_farthest_coordinate, before_nearest_coordinate, bounds_center,
    bounds_side_point, center_coordinate, coordinate, keep_after, keep_before, lane, opposite_side,
    orthogonal_path, orthogonal_path_points, path_through, path_via_lane, segment_between,
    segment_span, shift_point, side_for_direction, snap_coordinate, with_coordinate, Axis, Bounds,
    Direction, Lane, Point, Side,
};
use crate::render::mermaid::core::text::{diagram_text_width, split_diagram_lines};
use crate::render::mermaid::flowchart::labels::{
    flowchart_edge_label_layout, flowchart_route_label_layout, EdgeLabelLayout,
};
use crate::render::mermaid::flowchart::types::{
    EdgeRoute, FlowchartDiagram, FlowchartDirection, FlowchartEdge, NodeBounds, SubgraphBounds,
};

const BUS_CLEARANCE: i64 = 3;
const NODE_CLEARANCE: i64 = 2;
const ROUTING_CANDIDATE_BUDGET: i64 = 1024;

pub fn flowchart_direction_between(from: Point, to: Point) -> Option<Direction> {
    crate::render::mermaid::core::geometry::direction_between(from, to)
}

fn is_vertical_direction(direction: FlowchartDirection) -> bool {
    matches!(direction, FlowchartDirection::Tb | FlowchartDirection::Td | FlowchartDirection::Bt)
}

#[derive(Debug, Clone, Copy)]
struct EdgeRecord {
    edge_index: usize,
    source_port: Point,
    target_port: Point,
}

fn vertical_travel(from: &NodeBounds, to: &NodeBounds) -> Direction {
    if center_coordinate(to.bounds, Axis::Y) >= center_coordinate(from.bounds, Axis::Y) {
        Direction::Down
    } else {
        Direction::Up
    }
}

fn is_vertical_back_edge(
    from: &NodeBounds,
    to: &NodeBounds,
    direction: FlowchartDirection,
) -> bool {
    if direction == FlowchartDirection::Bt {
        center_coordinate(to.bounds, Axis::Y) > center_coordinate(from.bounds, Axis::Y)
    } else {
        center_coordinate(to.bounds, Axis::Y) < center_coordinate(from.bounds, Axis::Y)
    }
}

fn is_horizontal_back_edge(
    from: &NodeBounds,
    to: &NodeBounds,
    direction: FlowchartDirection,
) -> bool {
    if direction == FlowchartDirection::Rl {
        center_coordinate(to.bounds, Axis::X) > center_coordinate(from.bounds, Axis::X)
    } else {
        center_coordinate(to.bounds, Axis::X) < center_coordinate(from.bounds, Axis::X)
    }
}

fn horizontal_travel(
    from: &NodeBounds,
    to: &NodeBounds,
    direction: FlowchartDirection,
) -> Direction {
    let target_is_right =
        center_coordinate(to.bounds, Axis::X) > center_coordinate(from.bounds, Axis::X);
    let target_is_same_or_right =
        center_coordinate(to.bounds, Axis::X) >= center_coordinate(from.bounds, Axis::X);
    if direction == FlowchartDirection::Rl {
        if target_is_right {
            Direction::Right
        } else {
            Direction::Left
        }
    } else if target_is_same_or_right {
        Direction::Right
    } else {
        Direction::Left
    }
}

fn vertical_back_edge_path(
    from: &NodeBounds,
    to: &NodeBounds,
    left_boundary: Option<i64>,
) -> Vec<Point> {
    let start = bounds_side_point(from.bounds, Side::Left, false);
    let end = bounds_side_point(to.bounds, Side::Left, false);
    let bus_x = after_farthest_coordinate(&[start, end], Axis::X, Direction::Left, BUS_CLEARANCE)
        .min(left_boundary.map_or(i64::MAX, |boundary| boundary - BUS_CLEARANCE * 2));
    path_via_lane(start, lane(Axis::X, bus_x), end)
}

fn vertical_forward_edge_path(from: &NodeBounds, to: &NodeBounds) -> Vec<Point> {
    let travel = vertical_travel(from, to);
    let start_side = side_for_direction(travel);
    let end_side = opposite_side(start_side);
    let source_center = bounds_center(from.bounds);
    let target_center = bounds_center(to.bounds);
    let start = with_coordinate(
        bounds_side_point(from.bounds, start_side, false),
        Axis::X,
        snap_coordinate(source_center.x, target_center.x, 1),
    );
    let end = bounds_side_point(to.bounds, end_side, false);
    orthogonal_path(start, end, Some(Axis::Y), 4)
}

fn horizontal_back_edge_path(from: &NodeBounds, to: &NodeBounds) -> Vec<Point> {
    let start = bounds_side_point(from.bounds, Side::Top, false);
    let end = bounds_side_point(to.bounds, Side::Top, false);
    let bus_y = after_farthest_coordinate(&[start, end], Axis::Y, Direction::Up, BUS_CLEARANCE);
    path_via_lane(start, lane(Axis::Y, bus_y), end)
}

fn horizontal_edge_path(
    from: &NodeBounds,
    to: &NodeBounds,
    direction: FlowchartDirection,
) -> Vec<Point> {
    let overlaps_horizontally = from.bounds.left < to.bounds.left + to.bounds.width
        && to.bounds.left < from.bounds.left + from.bounds.width;
    if overlaps_horizontally {
        return vertical_forward_edge_path(from, to);
    }
    if is_horizontal_back_edge(from, to, direction) {
        return horizontal_back_edge_path(from, to);
    }
    let travel = horizontal_travel(from, to, direction);
    let start_side = side_for_direction(travel);
    orthogonal_path(
        bounds_side_point(from.bounds, start_side, false),
        bounds_side_point(to.bounds, opposite_side(start_side), false),
        Some(Axis::X),
        4,
    )
}

fn self_edge_path(bounds: &NodeBounds, lane_offset: i64) -> Vec<Point> {
    let start = bounds_side_point(bounds.bounds, Side::Right, false);
    let end = bounds_side_point(bounds.bounds, Side::Bottom, false);
    let right_lane_x = bounds.bounds.left + bounds.bounds.width + BUS_CLEARANCE + lane_offset;
    let bottom_lane_y = bounds.bounds.top + bounds.bounds.height + 1 + lane_offset;
    vec![
        start,
        Point { x: right_lane_x, y: start.y },
        Point { x: right_lane_x, y: bottom_lane_y },
        Point { x: end.x, y: bottom_lane_y },
        end,
    ]
}

fn parallel_edge_path(
    from: &NodeBounds,
    to: &NodeBounds,
    axis: Axis,
    lane_coordinate: i64,
) -> Vec<Point> {
    if axis == Axis::Y {
        let start = bounds_side_point(from.bounds, Side::Bottom, false);
        let end = bounds_side_point(to.bounds, Side::Bottom, false);
        return path_via_lane(start, lane(Axis::Y, lane_coordinate), end);
    }
    let start = bounds_side_point(from.bounds, Side::Right, false);
    let end = bounds_side_point(to.bounds, Side::Right, false);
    path_via_lane(start, lane(Axis::X, lane_coordinate), end)
}

fn label_height(edge: &FlowchartEdge) -> i64 {
    if edge.label.is_empty() {
        0
    } else {
        split_diagram_lines(&edge.label).len() as i64
    }
}

fn right_render_extent(route: &EdgeRoute, diagram: &FlowchartDiagram) -> i64 {
    let mut right = route.points.iter().map(|point| point.x).max().unwrap_or(0);
    let edge = &diagram.edges[route.edge_index];
    if !edge.label.is_empty() {
        let label = flowchart_route_label_layout(
            route.label_point,
            &route.points,
            &edge.label,
            route.label_axis,
        );
        right = right.max(label.point.x + label.width - 1);
    }
    right
}

fn parallel_lane_axis(from: &NodeBounds, to: &NodeBounds) -> Axis {
    if (to.bounds.center_x - from.bounds.center_x).abs()
        >= (to.bounds.center_y - from.bounds.center_y).abs()
    {
        Axis::Y
    } else {
        Axis::X
    }
}

fn edge_path(
    from: &NodeBounds,
    to: &NodeBounds,
    direction: FlowchartDirection,
    left_boundary: Option<i64>,
) -> Vec<Point> {
    if from.id == to.id {
        return self_edge_path(from, 0);
    }
    if !is_vertical_direction(direction) {
        return horizontal_edge_path(from, to, direction);
    }
    let overlaps_vertically = from.bounds.top < to.bounds.top + to.bounds.height
        && to.bounds.top < from.bounds.top + from.bounds.height;
    if overlaps_vertically {
        let travel =
            if center_coordinate(to.bounds, Axis::X) >= center_coordinate(from.bounds, Axis::X) {
                Direction::Right
            } else {
                Direction::Left
            };
        return orthogonal_path(
            bounds_side_point(from.bounds, side_for_direction(travel), false),
            bounds_side_point(to.bounds, opposite_side(side_for_direction(travel)), false),
            None,
            4,
        );
    }
    if is_vertical_back_edge(from, to, direction) {
        vertical_back_edge_path(from, to, left_boundary)
    } else {
        vertical_forward_edge_path(from, to)
    }
}

fn source_fan_out_lane(
    source_port: Point,
    target_ports: &[Point],
    axis: Axis,
    travel: Direction,
) -> i64 {
    let source_coordinate = coordinate(source_port, axis);
    let unclamped = keep_before(
        advance_coordinate(coordinate(source_port, axis), travel, BUS_CLEARANCE),
        before_nearest_coordinate(target_ports, axis, travel, NODE_CLEARANCE),
        travel,
    );
    keep_after(unclamped, source_coordinate, travel)
}

fn reserve_fan_out_lane(
    source_port: Point,
    target_ports: &[Point],
    axis: Axis,
    travel: Direction,
    reserved: &mut HashSet<i64>,
) -> i64 {
    let preferred = source_fan_out_lane(source_port, target_ports, axis, travel);
    let boundary = before_nearest_coordinate(target_ports, axis, travel, NODE_CLEARANCE);
    let source = coordinate(source_port, axis);
    let mut available: Option<i64> = None;
    let mut checked = 0i64;
    let mut offset = 0i64;
    while offset <= (boundary - preferred).abs() && checked < ROUTING_CANDIDATE_BUDGET {
        checked += 1;
        let candidate = advance_coordinate(preferred, travel, offset);
        if keep_before(candidate, boundary, travel) == candidate
            && keep_after(candidate, source, travel) == candidate
            && !reserved.contains(&candidate)
        {
            available = Some(candidate);
            break;
        }
        offset += 1;
    }
    let mut offset = 1i64;
    while offset <= (preferred - source).abs() && checked < ROUTING_CANDIDATE_BUDGET {
        checked += 1;
        let candidate = advance_coordinate(preferred, travel, -offset);
        if keep_before(candidate, boundary, travel) == candidate
            && keep_after(candidate, source, travel) == candidate
            && !reserved.contains(&candidate)
        {
            available = Some(candidate);
            break;
        }
        offset += 1;
    }
    let route_lane = available.unwrap_or(preferred);
    reserved.insert(route_lane);
    route_lane
}

fn target_fan_in_lane(
    source_ports: &[Point],
    target_port: Point,
    axis: Axis,
    travel: Direction,
) -> i64 {
    let target_coordinate = coordinate(target_port, axis);
    let unclamped = keep_after(
        advance_coordinate(coordinate(target_port, axis), travel, -BUS_CLEARANCE),
        after_farthest_coordinate(source_ports, axis, travel, NODE_CLEARANCE),
        travel,
    );
    keep_before(unclamped, advance_coordinate(target_coordinate, travel, -1), travel)
}

fn port_for_travel(bounds: &NodeBounds, travel: Direction, source: bool) -> Point {
    let side =
        if source { side_for_direction(travel) } else { opposite_side(side_for_direction(travel)) };
    bounds_side_point(bounds.bounds, side, false)
}

fn horizontal_forward_records(
    diagram: &FlowchartDiagram,
    edges: &[usize],
    bounds: &HashMap<String, NodeBounds>,
    direction: FlowchartDirection,
) -> Vec<EdgeRecord> {
    let travel =
        if direction == FlowchartDirection::Rl { Direction::Left } else { Direction::Right };
    let mut records = Vec::new();
    for edge_index in edges {
        let edge = &diagram.edges[*edge_index];
        let (Some(source), Some(target)) = (bounds.get(&edge.from), bounds.get(&edge.to)) else {
            continue;
        };
        let forward = if direction == FlowchartDirection::Rl {
            center_coordinate(target.bounds, Axis::X) < center_coordinate(source.bounds, Axis::X)
        } else {
            center_coordinate(target.bounds, Axis::X) > center_coordinate(source.bounds, Axis::X)
        };
        if !forward {
            continue;
        }
        records.push(EdgeRecord {
            edge_index: *edge_index,
            source_port: port_for_travel(source, travel, true),
            target_port: port_for_travel(target, travel, false),
        });
    }
    records
}

fn vertical_forward_records(
    diagram: &FlowchartDiagram,
    edges: &[usize],
    bounds: &HashMap<String, NodeBounds>,
    direction: FlowchartDirection,
) -> Vec<EdgeRecord> {
    let travel = if direction == FlowchartDirection::Bt { Direction::Up } else { Direction::Down };
    let mut records = Vec::new();
    for edge_index in edges {
        let edge = &diagram.edges[*edge_index];
        let (Some(source), Some(target)) = (bounds.get(&edge.from), bounds.get(&edge.to)) else {
            continue;
        };
        let forward = if direction == FlowchartDirection::Bt {
            center_coordinate(target.bounds, Axis::Y) < center_coordinate(source.bounds, Axis::Y)
        } else {
            center_coordinate(target.bounds, Axis::Y) > center_coordinate(source.bounds, Axis::Y)
        };
        if !forward {
            continue;
        }
        records.push(EdgeRecord {
            edge_index: *edge_index,
            source_port: port_for_travel(source, travel, true),
            target_port: port_for_travel(target, travel, false),
        });
    }
    records
}

fn horizontal_subgraph_exit(diagram: &FlowchartDiagram, edge: &FlowchartEdge) -> Option<usize> {
    for subgraph in diagram.subgraphs.iter().rev() {
        if !matches!(
            subgraph.direction,
            Some(FlowchartDirection::Lr) | Some(FlowchartDirection::Rl)
        ) {
            continue;
        }
        if subgraph.node_ids.iter().any(|id| *id == edge.from)
            && !subgraph.node_ids.iter().any(|id| *id == edge.to)
        {
            return Some(
                diagram.subgraphs.iter().position(|candidate| candidate.id == subgraph.id)?,
            );
        }
    }
    None
}

fn horizontal_subgraph_entry(diagram: &FlowchartDiagram, edge: &FlowchartEdge) -> Option<usize> {
    for subgraph in diagram.subgraphs.iter().rev() {
        if !matches!(
            subgraph.direction,
            Some(FlowchartDirection::Lr) | Some(FlowchartDirection::Rl)
        ) {
            continue;
        }
        if subgraph.node_ids.iter().any(|id| *id == edge.to)
            && !subgraph.node_ids.iter().any(|id| *id == edge.from)
        {
            return Some(
                diagram.subgraphs.iter().position(|candidate| candidate.id == subgraph.id)?,
            );
        }
    }
    None
}

fn fan_route(
    edge_index: usize,
    source_port: Point,
    target_port: Point,
    route_lane: Lane,
) -> EdgeRoute {
    EdgeRoute {
        edge_index,
        points: path_via_lane(source_port, route_lane, target_port),
        label_axis: None,
        label_point: None,
    }
}

fn align_clustered_vertical_sources(records: &[EdgeRecord]) -> Vec<EdgeRecord> {
    let xs: Vec<i64> = records.iter().map(|record| record.source_port.x).collect();
    let min_x = xs.iter().copied().min().unwrap_or(0);
    let max_x = xs.iter().copied().max().unwrap_or(0);
    if max_x - min_x > 1 {
        return records.to_vec();
    }
    let average = xs.iter().sum::<i64>() as f64 / xs.len() as f64;
    let x = average.round() as i64;
    records
        .iter()
        .map(|record| EdgeRecord { source_port: Point { x, ..record.source_port }, ..*record })
        .collect()
}

/// `routeFlowchartEdges` — the full pipeline.
pub fn route_flowchart_edges(
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    direction_for_edge: impl Fn(&FlowchartEdge) -> FlowchartDirection,
    subgraph_bounds: Option<&HashMap<String, SubgraphBounds>>,
    target_width: Option<i64>,
    direction_aligned: bool,
) -> Vec<EdgeRoute> {
    let routed_edges: Vec<usize> = diagram
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| !edge.order_only)
        .map(|(index, _)| index)
        .collect();
    let mut handled: HashSet<usize> = HashSet::new();
    let mut routes: Vec<EdgeRoute> = Vec::new();
    let left_boundary = subgraph_bounds.map(|frames| {
        bounds
            .values()
            .map(|bound| bound.bounds.left)
            .chain(frames.values().map(|frame| frame.bounds.left))
            .min()
            .unwrap_or(0)
    });

    route_parallel_edges(
        diagram,
        bounds,
        &direction_for_edge,
        direction_aligned,
        &routed_edges,
        &mut handled,
        &mut routes,
    );

    for direction in [FlowchartDirection::Lr, FlowchartDirection::Rl] {
        let horizontal_edges: Vec<usize> = routed_edges
            .iter()
            .copied()
            .filter(|index| {
                !handled.contains(index) && direction_for_edge(&diagram.edges[*index]) == direction
            })
            .collect();
        if horizontal_edges.is_empty() {
            continue;
        }
        let records = horizontal_forward_records(diagram, &horizontal_edges, bounds, direction);
        route_horizontal_fan_out(diagram, &records, bounds, direction, &mut handled, &mut routes);
        route_horizontal_fan_in(&records, direction, &mut handled, &mut routes);
    }

    route_horizontal_subgraph_exit_fan_in(
        diagram,
        bounds,
        subgraph_bounds,
        &routed_edges,
        &mut handled,
        &mut routes,
    );
    route_horizontal_subgraph_entries(
        diagram,
        bounds,
        subgraph_bounds,
        &routed_edges,
        &mut handled,
        &mut routes,
    );

    for direction in [FlowchartDirection::Td, FlowchartDirection::Tb, FlowchartDirection::Bt] {
        let vertical_edges: Vec<usize> = routed_edges
            .iter()
            .copied()
            .filter(|index| {
                !handled.contains(index) && direction_for_edge(&diagram.edges[*index]) == direction
            })
            .collect();
        if vertical_edges.is_empty() {
            continue;
        }
        let records = vertical_forward_records(diagram, &vertical_edges, bounds, direction);
        route_vertical_fan_out(&records, direction, &mut handled, &mut routes);
        route_vertical_fan_in(&records, direction, &mut handled, &mut routes);
    }

    for edge_index in &routed_edges {
        if handled.contains(edge_index) {
            continue;
        }
        let edge = &diagram.edges[*edge_index];
        let (Some(from), Some(to)) = (bounds.get(&edge.from), bounds.get(&edge.to)) else {
            continue;
        };
        routes.push(EdgeRoute {
            edge_index: *edge_index,
            points: edge_path(from, to, direction_for_edge(edge), left_boundary),
            label_axis: None,
            label_point: None,
        });
    }

    for index in (0..routes.len()).rev() {
        routes[index] = avoid_node_obstacles(
            routes[index].clone(),
            &routes,
            diagram,
            bounds,
            subgraph_bounds,
            index,
        );
    }

    let grouped_label_edge = |edge: &FlowchartEdge| -> bool {
        let containers = |id: &str| -> HashSet<String> {
            let mut ids = HashSet::new();
            let mut current = diagram
                .subgraphs
                .iter()
                .find(|subgraph| subgraph.node_ids.iter().any(|node| node == id));
            while let Some(subgraph) = current {
                ids.insert(subgraph.id.clone());
                current = subgraph.parent_id.as_deref().and_then(|parent| {
                    diagram.subgraphs.iter().find(|candidate| candidate.id == parent)
                });
            }
            ids
        };
        let from_containers = containers(&edge.from);
        if !containers(&edge.to).iter().any(|id| from_containers.contains(id)) {
            return false;
        }
        let targets: HashSet<&str> = routed_edges
            .iter()
            .filter(|index| {
                !diagram.edges[**index].label.is_empty() && diagram.edges[**index].from == edge.from
            })
            .map(|index| diagram.edges[*index].to.as_str())
            .collect();
        let sources: HashSet<&str> = routed_edges
            .iter()
            .filter(|index| {
                !diagram.edges[**index].label.is_empty() && diagram.edges[**index].to == edge.to
            })
            .map(|index| diagram.edges[*index].from.as_str())
            .collect();
        targets.len() > 1 || sources.len() > 1
    };

    let mut resolved: Vec<EdgeRoute> = Vec::with_capacity(routes.len());
    for (index, route) in routes.iter().enumerate() {
        let edge = &diagram.edges[route.edge_index];
        let grouped = grouped_label_edge(edge);
        let effective_target_width =
            if target_width.is_some() && !diagram.subgraphs.is_empty() && grouped {
                Some(1.max(target_width.unwrap() - 5))
            } else {
                target_width
            };
        resolved.push(avoid_label_overlap(
            route.clone(),
            resolved
                .iter()
                .chain(routes.iter().skip(index + 1))
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
            diagram,
            bounds,
            subgraph_bounds,
            effective_target_width,
            diagram.subgraphs.is_empty() || grouped,
        ));
    }
    resolved
}

fn route_parallel_edges(
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    direction_for_edge: &impl Fn(&FlowchartEdge) -> FlowchartDirection,
    direction_aligned: bool,
    routed_edges: &[usize],
    handled: &mut HashSet<usize>,
    routes: &mut Vec<EdgeRoute>,
) {
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for index in routed_edges {
        groups
            .entry(format!("{}:{}", diagram.edges[*index].from, diagram.edges[*index].to))
            .or_default()
            .push(*index);
    }
    for (_, edges) in groups {
        if edges.len() < 2 {
            continue;
        }
        let first = &diagram.edges[edges[0]];
        let (Some(from), Some(to)) = (bounds.get(&first.from), bounds.get(&first.to)) else {
            continue;
        };
        if from.id == to.id {
            let mut lane_offset = 0i64;
            for edge_index in edges {
                routes.push(EdgeRoute {
                    edge_index,
                    points: self_edge_path(from, lane_offset),
                    label_axis: None,
                    label_point: None,
                });
                handled.insert(edge_index);
                lane_offset += 1;
            }
            continue;
        }
        let parallel_axis = if direction_aligned && is_vertical_direction(direction_for_edge(first))
        {
            Axis::X
        } else {
            parallel_lane_axis(from, to)
        };
        let mut previous_route: Option<EdgeRoute> = None;
        for edge_index in edges {
            let edge = &diagram.edges[edge_index];
            let height = label_height(edge);
            let lane_coordinate = if parallel_axis == Axis::X {
                previous_route.as_ref().map_or(
                    bounds_side_point(from.bounds, Side::Right, false)
                        .x
                        .max(bounds_side_point(to.bounds, Side::Right, false).x)
                        + i64::from(direction_aligned),
                    |previous| right_render_extent(previous, diagram) + NODE_CLEARANCE,
                )
            } else {
                previous_route.as_ref().map_or(
                    bounds_side_point(from.bounds, Side::Bottom, false)
                        .y
                        .max(bounds_side_point(to.bounds, Side::Bottom, false).y)
                        + if height > 1 { height } else { 0 },
                    |previous| {
                        previous.points.iter().map(|point| point.y).max().unwrap_or(0)
                            + if height > 1 { height + 1 } else { 1 }
                    },
                )
            };
            let route = EdgeRoute {
                edge_index,
                points: parallel_edge_path(from, to, parallel_axis, lane_coordinate),
                label_axis: Some(if parallel_axis == Axis::X { Axis::Y } else { Axis::X }),
                label_point: None,
            };
            routes.push(route.clone());
            handled.insert(edge_index);
            previous_route = Some(route);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn route_horizontal_fan_out(
    diagram: &FlowchartDiagram,
    records: &[EdgeRecord],
    bounds: &HashMap<String, NodeBounds>,
    direction: FlowchartDirection,
    handled: &mut HashSet<usize>,
    routes: &mut Vec<EdgeRoute>,
) {
    let mut reserved_bus_lanes: HashSet<i64> = HashSet::new();
    let mut target_owners: HashMap<String, String> = HashMap::new();
    let mut groups: HashMap<String, Vec<EdgeRecord>> = HashMap::new();
    for record in records {
        groups
            .entry(diagram.edges[record.edge_index].from.clone())
            .or_default()
            .push(record.clone());
    }
    for (source_id, source_records) in &groups {
        if source_records.len() < 2 {
            continue;
        }
        let travel =
            if direction == FlowchartDirection::Rl { Direction::Left } else { Direction::Right };
        let source_port = source_records[0].source_port;
        let target_ports: Vec<Point> =
            source_records.iter().map(|record| record.target_port).collect();

        let bus_x = reserve_fan_out_lane(
            source_port,
            &target_ports,
            Axis::X,
            travel,
            &mut reserved_bus_lanes,
        );
        for record in source_records {
            let to = diagram.edges[record.edge_index].to.clone();
            let target_owner = target_owners.get(&to).cloned();
            target_owners.entry(to.clone()).or_insert_with(|| source_id.to_string());
            let target = bounds.get(&to);
            if target_owner.is_none()
                || target_owner.as_deref() == Some(source_id.as_str())
                || target.is_none()
            {
                routes.push(fan_route(
                    record.edge_index,
                    source_port,
                    record.target_port,
                    lane(Axis::X, bus_x),
                ));
                handled.insert(record.edge_index);
                continue;
            }

            let target_side =
                if source_port.y < record.target_port.y { Side::Top } else { Side::Bottom };
            let target_point = bounds_side_point(target.unwrap().bounds, target_side, false);
            let approach = shift_point(
                target_point,
                if target_side == Side::Top { Direction::Up } else { Direction::Down },
                1,
            );
            routes.push(EdgeRoute {
                edge_index: record.edge_index,
                points: path_through(&[
                    source_port,
                    Point { x: bus_x, y: source_port.y },
                    Point { x: bus_x, y: approach.y },
                    approach,
                    target_point,
                ]),
                label_axis: None,
                label_point: None,
            });
            handled.insert(record.edge_index);
        }
    }
}

fn route_horizontal_fan_in(
    records: &[EdgeRecord],
    direction: FlowchartDirection,
    handled: &mut HashSet<usize>,
    routes: &mut Vec<EdgeRoute>,
) {
    let mut groups: HashMap<String, Vec<EdgeRecord>> = HashMap::new();
    for record in records.iter().filter(|record| !handled.contains(&record.edge_index)) {
        groups.entry(format!("{}", record.target_key())).or_default().push(record.clone());
    }
    for (_, target_records) in groups {
        if target_records.len() < 2 {
            continue;
        }
        let travel =
            if direction == FlowchartDirection::Rl { Direction::Left } else { Direction::Right };
        let target_port = target_records[0].target_port;
        let source_ports: Vec<Point> =
            target_records.iter().map(|record| record.source_port).collect();
        let bus_x = target_fan_in_lane(&source_ports, target_port, Axis::X, travel);
        for record in &target_records {
            routes.push(fan_route(
                record.edge_index,
                record.source_port,
                target_port,
                lane(Axis::X, bus_x),
            ));
            handled.insert(record.edge_index);
        }
    }
}

fn route_vertical_fan_out(
    records: &[EdgeRecord],
    direction: FlowchartDirection,
    handled: &mut HashSet<usize>,
    routes: &mut Vec<EdgeRoute>,
) {
    let mut groups: HashMap<String, Vec<EdgeRecord>> = HashMap::new();
    for record in records {
        groups.entry(format!("{}", record.source_key())).or_default().push(record.clone());
    }
    for (_, source_records) in groups {
        if source_records.len() < 2 {
            continue;
        }
        let travel =
            if direction == FlowchartDirection::Bt { Direction::Up } else { Direction::Down };
        let source_port = source_records[0].source_port;
        let target_ports: Vec<Point> =
            source_records.iter().map(|record| record.target_port).collect();
        let bus_y = source_fan_out_lane(source_port, &target_ports, Axis::Y, travel);
        for record in &source_records {
            routes.push(fan_route(
                record.edge_index,
                source_port,
                record.target_port,
                lane(Axis::Y, bus_y),
            ));
            handled.insert(record.edge_index);
        }
    }
}

fn route_vertical_fan_in(
    records: &[EdgeRecord],
    direction: FlowchartDirection,
    handled: &mut HashSet<usize>,
    routes: &mut Vec<EdgeRoute>,
) {
    let mut groups: HashMap<String, Vec<EdgeRecord>> = HashMap::new();
    for record in records.iter().filter(|record| !handled.contains(&record.edge_index)) {
        groups.entry(format!("{}", record.target_key())).or_default().push(record.clone());
    }
    for (_, unaligned) in groups {
        let target_records = align_clustered_vertical_sources(&unaligned);
        if target_records.len() < 2 {
            continue;
        }
        let travel =
            if direction == FlowchartDirection::Bt { Direction::Up } else { Direction::Down };
        let target_port = target_records[0].target_port;
        let source_ports: Vec<Point> =
            target_records.iter().map(|record| record.source_port).collect();
        let bus_y = target_fan_in_lane(&source_ports, target_port, Axis::Y, travel);
        for record in &target_records {
            routes.push(fan_route(
                record.edge_index,
                record.source_port,
                target_port,
                lane(Axis::Y, bus_y),
            ));
            handled.insert(record.edge_index);
        }
    }
}

impl EdgeRecord {
    fn source_key(&self) -> usize {
        self.edge_index
    }

    fn target_key(&self) -> usize {
        self.edge_index
    }
}

#[allow(clippy::too_many_arguments)]
fn route_horizontal_subgraph_exit_fan_in(
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: Option<&HashMap<String, SubgraphBounds>>,
    routed_edges: &[usize],
    handled: &mut HashSet<usize>,
    routes: &mut Vec<EdgeRoute>,
) {
    let Some(frames) = subgraph_bounds else { return };
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for edge_index in routed_edges {
        if handled.contains(edge_index) {
            continue;
        }
        let edge = &diagram.edges[*edge_index];
        let Some(subgraph_index) = horizontal_subgraph_exit(diagram, edge) else { continue };
        let Some(source) = bounds.get(&edge.from) else { continue };
        if bounds.get(&edge.to).is_none() {
            continue;
        }
        groups
            .entry(format!("{}:{}", diagram.subgraphs[subgraph_index].id, edge.to))
            .or_default()
            .push(*edge_index);
        let _ = source;
    }

    for group in groups.values() {
        let Some(first) = group.first() else { continue };
        let Some(subgraph_index) = horizontal_subgraph_exit(diagram, &diagram.edges[*first]) else {
            continue;
        };
        let subgraph = &diagram.subgraphs[subgraph_index];
        let Some(subgraph_bound) = frames.get(&subgraph.id) else { continue };
        let Some(target) = bounds.get(&diagram.edges[*first].to) else { continue };

        let travel = if subgraph.direction == Some(FlowchartDirection::Rl) {
            Direction::Left
        } else {
            Direction::Right
        };
        let bus_x = if subgraph.direction == Some(FlowchartDirection::Rl) {
            subgraph_bound.bounds.left - BUS_CLEARANCE
        } else {
            subgraph_bound.bounds.left + subgraph_bound.bounds.width + BUS_CLEARANCE
        };
        let target_subgraph = horizontal_subgraph_entry(diagram, &diagram.edges[*first]);
        let target_subgraph_bound =
            target_subgraph.and_then(|index| frames.get(&diagram.subgraphs[index].id));
        let target_below = target.bounds.center_y >= subgraph_bound.bounds.center_y;
        let target_port = target_subgraph
            .map(|index| {
                port_for_travel(
                    target,
                    horizontal_subgraph_entry_travel(&diagram.subgraphs[index]),
                    false,
                )
            })
            .unwrap_or_else(|| {
                bounds_side_point(
                    target.bounds,
                    if target_below { Side::Top } else { Side::Bottom },
                    false,
                )
            });
        let join_y = if let Some(target_bound) = target_subgraph_bound {
            horizontal_subgraph_join_y(subgraph_bound, target_bound)
        } else {
            horizontal_subgraph_exit_join_y(subgraph_bound, target_port, target_below)
        };
        let entry_x = if let (Some(index), Some(bound)) = (target_subgraph, target_subgraph_bound) {
            horizontal_subgraph_entry_lane(&diagram.subgraphs[index], bound)
        } else {
            target_port.x
        };

        for edge_index in group.iter().copied() {
            let edge = &diagram.edges[edge_index];
            let Some(source) = bounds.get(&edge.from) else { continue };
            let source_port = port_for_travel(source, travel, true);
            routes.push(EdgeRoute {
                edge_index,
                points: path_through(&[
                    source_port,
                    Point { x: bus_x, y: source_port.y },
                    Point { x: bus_x, y: join_y },
                    Point { x: entry_x, y: join_y },
                    Point { x: entry_x, y: target_port.y },
                    target_port,
                ]),
                label_axis: None,
                label_point: None,
            });
            handled.insert(edge_index);
        }
    }
}

fn horizontal_subgraph_entry_travel(
    subgraph: &crate::render::mermaid::flowchart::types::FlowchartSubgraph,
) -> Direction {
    if subgraph.direction == Some(FlowchartDirection::Rl) {
        Direction::Left
    } else {
        Direction::Right
    }
}

fn horizontal_subgraph_entry_lane(
    subgraph: &crate::render::mermaid::flowchart::types::FlowchartSubgraph,
    bound: &SubgraphBounds,
) -> i64 {
    if subgraph.direction == Some(FlowchartDirection::Rl) {
        bound.bounds.left + bound.bounds.width + BUS_CLEARANCE
    } else {
        bound.bounds.left - BUS_CLEARANCE
    }
}

fn horizontal_subgraph_join_y(from: &SubgraphBounds, target: &SubgraphBounds) -> i64 {
    if from.bounds.center_y <= target.bounds.center_y {
        let start = from.bounds.top + from.bounds.height;
        let end = target.bounds.top - 1;
        return if start <= end { (start + end).div_euclid(2) } else { start };
    }
    let start = target.bounds.top + target.bounds.height;
    let end = from.bounds.top - 1;
    if start <= end {
        (start + end).div_euclid(2)
    } else {
        end
    }
}

fn horizontal_subgraph_exit_join_y(
    from: &SubgraphBounds,
    target_port: Point,
    target_below: bool,
) -> i64 {
    if target_below {
        let outside = from.bounds.top + from.bounds.height;
        let before_target = target_port.y - 1;
        let preferred = target_port.y - BUS_CLEARANCE;
        if outside <= before_target {
            outside.max(preferred).min(before_target)
        } else {
            before_target
        }
    } else {
        let outside = from.bounds.top - 1;
        let after_target = target_port.y + 1;
        let preferred = target_port.y + BUS_CLEARANCE;
        if after_target <= outside {
            outside.min(preferred).max(after_target)
        } else {
            after_target
        }
    }
}

fn route_horizontal_subgraph_entries(
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: Option<&HashMap<String, SubgraphBounds>>,
    routed_edges: &[usize],
    handled: &mut HashSet<usize>,
    routes: &mut Vec<EdgeRoute>,
) {
    let Some(frames) = subgraph_bounds else { return };
    for edge_index in routed_edges {
        if handled.contains(edge_index) {
            continue;
        }
        let edge = &diagram.edges[*edge_index];
        let Some(subgraph_index) = horizontal_subgraph_entry(diagram, edge) else { continue };
        let subgraph = &diagram.subgraphs[subgraph_index];
        let Some(subgraph_bound) = frames.get(&subgraph.id) else { continue };
        let (Some(from), Some(to)) = (bounds.get(&edge.from), bounds.get(&edge.to)) else {
            continue;
        };

        let target_port = port_for_travel(to, horizontal_subgraph_entry_travel(subgraph), false);
        let entry_x = horizontal_subgraph_entry_lane(subgraph, subgraph_bound);
        let travel = vertical_travel(from, to);
        let source_port = port_for_travel(from, travel, true);
        routes.push(EdgeRoute {
            edge_index: *edge_index,
            points: path_through(&[
                source_port,
                Point { x: entry_x, y: source_port.y },
                Point { x: entry_x, y: target_port.y },
                target_port,
            ]),
            label_axis: None,
            label_point: None,
        });
        handled.insert(*edge_index);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RectBounds {
    left: i64,
    top: i64,
    width: i64,
    height: i64,
}

impl RectBounds {
    fn right(self) -> i64 {
        self.left + self.width - 1
    }

    fn bottom(self) -> i64 {
        self.top + self.height - 1
    }
}

impl From<Bounds> for RectBounds {
    fn from(bounds: Bounds) -> Self {
        Self { left: bounds.left, top: bounds.top, width: bounds.width, height: bounds.height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllowedContact {
    Source,
    Target,
    Both,
}

fn path_intersects_bounds(
    points: &[Point],
    bounds: RectBounds,
    allowed_contact: Option<AllowedContact>,
) -> bool {
    let right = bounds.right();
    let bottom = bounds.bottom();
    for index in 1..points.len() {
        let from = points[index - 1];
        let to = points[index];
        if from.x == to.x {
            if from.x < bounds.left || from.x > right {
                continue;
            }
            let overlap_top = from.y.min(to.y).max(bounds.top);
            let overlap_bottom = from.y.max(to.y).min(bottom);
            if overlap_top > overlap_bottom {
                continue;
            }
            let source_contact =
                matches!(allowed_contact, Some(AllowedContact::Source | AllowedContact::Both))
                    && index == 1
                    && overlap_top == overlap_bottom
                    && from.x == points[0].x
                    && overlap_top == points[0].y;
            let target_contact =
                matches!(allowed_contact, Some(AllowedContact::Target | AllowedContact::Both))
                    && index == points.len() - 1
                    && overlap_top == overlap_bottom
                    && to.x == points[points.len() - 1].x
                    && overlap_top == points[points.len() - 1].y;
            if !source_contact && !target_contact {
                return true;
            }
            continue;
        }
        if from.y < bounds.top || from.y > bottom {
            continue;
        }
        let overlap_left = from.x.min(to.x).max(bounds.left);
        let overlap_right = from.x.max(to.x).min(right);
        if overlap_left > overlap_right {
            continue;
        }
        let source_contact =
            matches!(allowed_contact, Some(AllowedContact::Source | AllowedContact::Both))
                && index == 1
                && overlap_left == overlap_right
                && overlap_left == points[0].x
                && from.y == points[0].y;
        let target_contact =
            matches!(allowed_contact, Some(AllowedContact::Target | AllowedContact::Both))
                && index == points.len() - 1
                && overlap_left == overlap_right
                && overlap_left == points[points.len() - 1].x
                && to.y == points[points.len() - 1].y;
        if !source_contact && !target_contact {
            return true;
        }
    }
    false
}

fn label_intersects_bounds(label: Option<&EdgeLabelLayout>, bounds: RectBounds) -> bool {
    let Some(label) = label else { return false };
    label.point.x <= bounds.right()
        && label.point.x + label.width - 1 >= bounds.left
        && label.point.y <= bounds.bottom()
        && label.point.y + label.height - 1 >= bounds.top
}

fn label_intersects_subgraph_frame(
    label: Option<&EdgeLabelLayout>,
    bounds: &SubgraphBounds,
) -> bool {
    let Some(label) = label else { return false };
    let label_right = label.point.x + label.width - 1;
    let label_bottom = label.point.y + label.height - 1;
    let right = bounds.bounds.left + bounds.bounds.width - 1;
    let bottom = bounds.bounds.top + bounds.bounds.height - 1;
    (label.point.x <= right
        && label_right >= bounds.bounds.left
        && ((label.point.y <= bounds.bounds.top && label_bottom >= bounds.bounds.top)
            || (label.point.y <= bottom && label_bottom >= bottom)))
        || (label.point.y <= bottom
            && label_bottom >= bounds.bounds.top
            && ((label.point.x <= bounds.bounds.left && label_right >= bounds.bounds.left)
                || (label.point.x <= right && label_right >= right)))
}

fn route_length(route: &EdgeRoute) -> i64 {
    route
        .points
        .windows(2)
        .map(|window| (window[1].x - window[0].x).abs() + (window[1].y - window[0].y).abs())
        .sum()
}

fn label_intersects_labels(
    label: Option<&EdgeLabelLayout>,
    other_labels: &[EdgeLabelLayout],
) -> bool {
    let Some(label) = label else { return false };
    other_labels.iter().any(|other| {
        label_intersects_bounds(
            Some(label),
            RectBounds {
                left: other.point.x,
                top: other.point.y,
                width: other.width,
                height: other.height,
            },
        )
    })
}

fn label_intersects_route_paths(label: Option<&EdgeLabelLayout>, routes: &[EdgeRoute]) -> bool {
    let Some(label) = label else { return false };
    label.lines.iter().enumerate().any(|(line_index, line)| {
        let width = (diagram_text_width(line) as i64).saturating_sub(2);
        if width <= 0 {
            return false;
        }
        routes.iter().any(|route| {
            path_intersects_bounds(
                &route.points,
                RectBounds {
                    left: label.point.x + 1,
                    top: label.point.y + line_index as i64,
                    width,
                    height: 1,
                },
                None,
            )
        })
    })
}

fn route_intersects_labels(route: &EdgeRoute, labels: &[EdgeLabelLayout]) -> bool {
    labels.iter().any(|label| {
        label.lines.iter().enumerate().any(|(line_index, line)| {
            let width = (diagram_text_width(line) as i64).saturating_sub(2);
            width > 0
                && path_intersects_bounds(
                    &route.points,
                    RectBounds {
                        left: label.point.x + 1,
                        top: label.point.y + line_index as i64,
                        width,
                        height: 1,
                    },
                    None,
                )
        })
    })
}

fn paths_intersect(left: &[Point], right: &[Point]) -> bool {
    let occupied: HashSet<(i64, i64)> =
        orthogonal_path_points(left).into_iter().map(|point| (point.x, point.y)).collect();
    orthogonal_path_points(right).into_iter().any(|point| occupied.contains(&(point.x, point.y)))
}

fn endpoint_disjoint(left: &FlowchartEdge, right: &FlowchartEdge) -> bool {
    left.from != right.from && left.from != right.to && left.to != right.from && left.to != right.to
}

fn endpoint_conflicts_with_routes(
    route: &EdgeRoute,
    diagram: &FlowchartDiagram,
    other_routes: &[EdgeRoute],
) -> bool {
    let Some(source) = route.points.first() else { return false };
    let Some(target) = route.points.last() else { return false };
    other_routes.iter().any(|other| {
        let Some(other_source) = other.points.first() else { return false };
        let Some(other_target) = other.points.last() else { return false };
        (target.x == other_source.x && target.y == other_source.y)
            || (source.x == other_target.x && source.y == other_target.y)
            || {
                let _ = diagram;
                false
            }
    })
}

fn path_runs_along_frame(points: &[Point], bounds: &SubgraphBounds) -> bool {
    let right = bounds.bounds.left + bounds.bounds.width - 1;
    let bottom = bounds.bounds.top + bounds.bounds.height - 1;
    for window in points.windows(2) {
        let Some(segment) = segment_between(window[0], window[1]) else { continue };
        let span = segment_span(segment);
        if segment.axis == Axis::X
            && (segment.from.y == bounds.bounds.top || segment.from.y == bottom)
            && span.end.min(right) > span.start.max(bounds.bounds.left)
        {
            return true;
        }
        if segment.axis == Axis::Y
            && (segment.from.x == bounds.bounds.left || segment.from.x == right)
            && span.end.min(bottom) > span.start.max(bounds.bounds.top)
        {
            return true;
        }
    }
    false
}

fn subgraph_title_bounds(bounds: &SubgraphBounds) -> RectBounds {
    let lines = split_diagram_lines(&bounds.label);
    RectBounds {
        left: bounds.bounds.left + 2,
        top: if bounds.label_side_top {
            bounds.bounds.top
        } else {
            bounds.bounds.top + bounds.bounds.height - lines.len() as i64
        },
        width: lines
            .iter()
            .map(|line| diagram_text_width(&format!(" {line} ")) as i64)
            .max()
            .unwrap_or(0),
        height: lines.len() as i64,
    }
}

/// `avoidNodeObstacles` — reroutes colliding routes through candidate
/// bus lanes.
#[allow(clippy::too_many_lines)]
fn avoid_node_obstacles(
    route: EdgeRoute,
    routes: &[EdgeRoute],
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: Option<&HashMap<String, SubgraphBounds>>,
    route_index: usize,
) -> EdgeRoute {
    let edge = &diagram.edges[route.edge_index];
    let all_node_bounds: Vec<RectBounds> =
        bounds.values().map(|bound| RectBounds::from(bound.bounds)).collect();
    let all_subgraph_bounds: Vec<&SubgraphBounds> =
        subgraph_bounds.map_or(Vec::new(), |frames| frames.values().collect());
    let subgraphs = &diagram.subgraphs;
    fn contains(
        subgraphs: &[crate::render::mermaid::flowchart::types::FlowchartSubgraph],
        subgraph: &crate::render::mermaid::flowchart::types::FlowchartSubgraph,
        node_id: &str,
    ) -> bool {
        subgraph.node_ids.iter().any(|id| id == node_id)
            || subgraphs.iter().any(|child| {
                child.parent_id.as_deref() == Some(&subgraph.id)
                    && contains(subgraphs, child, node_id)
            })
    }
    let contains = |subgraph: &crate::render::mermaid::flowchart::types::FlowchartSubgraph,
                    node_id: &str|
     -> bool { contains(subgraphs, subgraph, node_id) };
    let owner = subgraphs.iter().rev().find(|subgraph| {
        if !contains(subgraph, &edge.from) || !contains(subgraph, &edge.to) {
            return false;
        }
        let children: Vec<&_> = subgraphs
            .iter()
            .filter(|child| child.parent_id.as_deref() == Some(&subgraph.id))
            .collect();
        children.iter().any(|child| contains(child, &edge.from))
            && children.iter().any(|child| contains(child, &edge.to))
            && !children
                .iter()
                .any(|child| contains(child, &edge.from) && contains(child, &edge.to))
    });
    let owner_bounds = owner
        .and_then(|subgraph| subgraph_bounds.and_then(|frames| frames.get(&subgraph.id)))
        .map(|bound| bound.bounds);
    let leaves_owner = |candidate: &EdgeRoute| -> bool {
        owner_bounds.is_some_and(|owner| {
            candidate.points.iter().any(|point| {
                point.x <= owner.left
                    || point.x >= owner.left + owner.width - 1
                    || point.y <= owner.top
                    || point.y >= owner.top + owner.height - 1
            })
        })
    };
    let other_routes: Vec<&EdgeRoute> = routes
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != route_index)
        .map(|(_, route)| route)
        .collect();
    let other_routes_cloned: Vec<EdgeRoute> = other_routes.iter().copied().cloned().collect();
    let other_labels: Vec<EdgeLabelLayout> = other_routes
        .iter()
        .filter(|other| !diagram.edges[other.edge_index].label.is_empty())
        .map(|other| {
            let label = &diagram.edges[other.edge_index].label;
            flowchart_route_label_layout(other.label_point, &other.points, label, other.label_axis)
        })
        .collect();
    let intersects_node = |candidate: &EdgeRoute| -> bool {
        bounds.values().any(|node| {
            let bound = RectBounds::from(node.bounds);
            let allowed = if node.id == edge.from && node.id == edge.to {
                Some(AllowedContact::Both)
            } else if node.id == edge.from {
                Some(AllowedContact::Source)
            } else if node.id == edge.to {
                Some(AllowedContact::Target)
            } else {
                None
            };
            path_intersects_bounds(&candidate.points, bound, allowed)
        })
    };
    let intersects_structural_obstacle = |candidate: &EdgeRoute| -> bool {
        intersects_node(candidate)
            || all_subgraph_bounds.iter().any(|bound| {
                path_runs_along_frame(&candidate.points, bound)
                    || (!bound.label.is_empty()
                        && path_intersects_bounds(
                            &candidate.points,
                            subgraph_title_bounds(bound),
                            None,
                        ))
            })
    };
    let intersects_routing_obstacle = |candidate: &EdgeRoute| -> bool {
        intersects_structural_obstacle(candidate)
            || endpoint_conflicts_with_routes(candidate, diagram, &other_routes_cloned)
            || other_routes.iter().any(|other| {
                endpoint_disjoint(
                    &diagram.edges[candidate.edge_index],
                    &diagram.edges[other.edge_index],
                ) && paths_intersect(&candidate.points, &other.points)
            })
    };
    let route_label = |candidate: &EdgeRoute| -> Option<EdgeLabelLayout> {
        let label = &diagram.edges[candidate.edge_index].label;
        if label.is_empty() {
            return None;
        }
        Some(flowchart_route_label_layout(
            candidate.label_point,
            &candidate.points,
            label,
            candidate.label_axis,
        ))
    };
    let intersects_obstacle = |candidate: &EdgeRoute| -> bool {
        let label = route_label(candidate);
        leaves_owner(candidate)
            || intersects_routing_obstacle(candidate)
            || all_node_bounds.iter().any(|bound| label_intersects_bounds(label.as_ref(), *bound))
            || all_subgraph_bounds
                .iter()
                .any(|bound| label_intersects_subgraph_frame(label.as_ref(), bound))
            || label_intersects_labels(label.as_ref(), &other_labels)
            || label_intersects_route_paths(label.as_ref(), &other_routes_cloned)
            || route_intersects_labels(candidate, &other_labels)
    };
    if !intersects_obstacle(&route) {
        return route;
    }

    let (Some(from), Some(to)) = (bounds.get(&edge.from), bounds.get(&edge.to)) else {
        return route;
    };
    let mut routing_bounds: Vec<RectBounds> = all_node_bounds.clone();
    routing_bounds.extend(all_subgraph_bounds.iter().map(|bound| RectBounds::from(bound.bounds)));
    let right_bus_x =
        routing_bounds.iter().map(|bound| bound.right()).max().unwrap_or(0) + BUS_CLEARANCE;
    let left_bus_x =
        routing_bounds.iter().map(|bound| bound.left).min().unwrap_or(0) - BUS_CLEARANCE;
    let top_bus_y = routing_bounds.iter().map(|bound| bound.top).min().unwrap_or(0) - BUS_CLEARANCE;
    let bottom_bus_y =
        routing_bounds.iter().map(|bound| bound.bottom()).max().unwrap_or(0) + BUS_CLEARANCE;

    let mut right_bus_xs: Vec<i64> = vec![right_bus_x];
    if let Some(owner) = owner_bounds {
        right_bus_x_dedup(&mut right_bus_xs, owner.left + owner.width - 2);
    }
    for label in &other_labels {
        let value = 0.max(label.point.x + label.width - 1 + NODE_CLEARANCE);
        right_bus_xs.push(value.max(right_bus_x));
    }
    right_bus_xs.sort_unstable();
    right_bus_xs.dedup();

    let mut left_bus_xs: Vec<i64> = vec![left_bus_x];
    if let Some(owner) = owner_bounds {
        let value = owner.left + 1;
        left_bus_xs.push(value.min(left_bus_x));
    }
    for label in &other_labels {
        left_bus_xs.push((label.point.x - NODE_CLEARANCE).min(left_bus_x));
    }
    left_bus_xs.sort_unstable_by(|left, right| right.cmp(left));
    left_bus_xs.dedup();

    let mut top_bus_ys: Vec<i64> = vec![top_bus_y];
    if let Some(owner) = owner_bounds {
        top_bus_ys.push((owner.top + 1).min(top_bus_y));
    }
    for label in &other_labels {
        top_bus_ys.push((label.point.y - NODE_CLEARANCE).min(top_bus_y));
    }
    top_bus_ys.sort_unstable_by(|left, right| right.cmp(left));
    top_bus_ys.dedup();

    let mut bottom_bus_ys: Vec<i64> = vec![bottom_bus_y];
    if let Some(owner) = owner_bounds {
        bottom_bus_y_dedup(&mut bottom_bus_ys, owner.top + owner.height - 2);
    }
    for label in &other_labels {
        let value = label.point.y + label.height - 1 + NODE_CLEARANCE;
        bottom_bus_ys.push(value.max(bottom_bus_y));
    }
    bottom_bus_ys.sort_unstable();
    bottom_bus_ys.dedup();

    let bus_limit = 1.max((ROUTING_CANDIDATE_BUDGET / 4).isqrt() as i64);
    let take = |lanes: &[i64]| -> Vec<i64> {
        if lanes.len() > bus_limit as usize {
            lanes[..bus_limit as usize].to_vec()
        } else {
            lanes.to_vec()
        }
    };
    let candidate_left_bus_xs = take(&left_bus_xs);
    let candidate_right_bus_xs = take(&right_bus_xs);
    let candidate_top_bus_ys = take(&top_bus_ys);
    let candidate_bottom_bus_ys = take(&bottom_bus_ys);
    let mut buses: Vec<Lane> = Vec::new();
    buses.extend(candidate_left_bus_xs.iter().map(|coordinate| lane(Axis::X, *coordinate)));
    buses.extend(candidate_right_bus_xs.iter().map(|coordinate| lane(Axis::X, *coordinate)));
    buses.extend(candidate_top_bus_ys.iter().map(|coordinate| lane(Axis::Y, *coordinate)));
    buses.extend(candidate_bottom_bus_ys.iter().map(|coordinate| lane(Axis::Y, *coordinate)));

    let route_via_bus = |start: Point, target_side: Side, bus: Lane| -> EdgeRoute {
        let end = bounds_side_point(to.bounds, target_side, false);
        let approach = shift_point(
            end,
            match target_side {
                Side::Left => Direction::Left,
                Side::Right => Direction::Right,
                Side::Top => Direction::Up,
                Side::Bottom => Direction::Down,
            },
            1,
        );
        let label_axis = match (route.label_axis, bus.axis) {
            (None, _) => None,
            (Some(_), Axis::X) => Some(Axis::Y),
            (Some(_), Axis::Y) => Some(Axis::X),
        };
        EdgeRoute {
            label_axis,
            points: if bus.axis == Axis::X {
                path_through(&[
                    start,
                    Point { x: bus.coordinate, y: start.y },
                    Point { x: bus.coordinate, y: approach.y },
                    approach,
                    end,
                ])
            } else {
                path_through(&[
                    start,
                    Point { x: start.x, y: bus.coordinate },
                    Point { x: approach.x, y: bus.coordinate },
                    approach,
                    end,
                ])
            },
            ..route.clone()
        }
    };

    let mut self_loops: Vec<EdgeRoute> = Vec::new();
    if from.id == to.id {
        for bus_x in &candidate_right_bus_xs {
            for bus_y in &candidate_bottom_bus_ys {
                self_loops.push(EdgeRoute {
                    points: path_through(&[
                        bounds_side_point(from.bounds, Side::Right, false),
                        Point { x: *bus_x, y: from.bounds.center_y },
                        Point { x: *bus_x, y: *bus_y },
                        Point { x: from.bounds.center_x, y: *bus_y },
                        bounds_side_point(from.bounds, Side::Bottom, false),
                    ]),
                    ..route.clone()
                });
            }
        }
        for bus_y in &candidate_bottom_bus_ys {
            for bus_x in &candidate_left_bus_xs {
                self_loops.push(EdgeRoute {
                    points: path_through(&[
                        bounds_side_point(from.bounds, Side::Bottom, false),
                        Point { x: from.bounds.center_x, y: *bus_y },
                        Point { x: *bus_x, y: *bus_y },
                        Point { x: *bus_x, y: from.bounds.center_y },
                        bounds_side_point(from.bounds, Side::Left, false),
                    ]),
                    ..route.clone()
                });
            }
        }
        for bus_x in &candidate_left_bus_xs {
            for bus_y in &candidate_top_bus_ys {
                self_loops.push(EdgeRoute {
                    points: path_through(&[
                        bounds_side_point(from.bounds, Side::Left, false),
                        Point { x: *bus_x, y: from.bounds.center_y },
                        Point { x: *bus_x, y: *bus_y },
                        Point { x: from.bounds.center_x, y: *bus_y },
                        bounds_side_point(from.bounds, Side::Top, false),
                    ]),
                    ..route.clone()
                });
            }
        }
        for bus_y in &candidate_top_bus_ys {
            for bus_x in &candidate_right_bus_xs {
                self_loops.push(EdgeRoute {
                    points: path_through(&[
                        bounds_side_point(from.bounds, Side::Top, false),
                        Point { x: from.bounds.center_x, y: *bus_y },
                        Point { x: *bus_x, y: *bus_y },
                        Point { x: *bus_x, y: from.bounds.center_y },
                        bounds_side_point(from.bounds, Side::Right, false),
                    ]),
                    ..route.clone()
                });
            }
        }
    }

    let target_sides = [Side::Left, Side::Right, Side::Top, Side::Bottom];
    let shortest =
        |candidates: &[EdgeRoute], accept: &dyn Fn(&EdgeRoute) -> bool| -> Option<EdgeRoute> {
            let mut best: Option<EdgeRoute> = None;
            for candidate in candidates.iter().filter(|candidate| accept(candidate)) {
                if best.as_ref().is_none_or(|best| route_length(candidate) < route_length(best)) {
                    best = Some(candidate.clone());
                }
            }
            best
        };
    if from.id == to.id {
        return shortest(&self_loops, &|candidate| !intersects_obstacle(candidate))
            .or_else(|| {
                shortest(&self_loops, &|candidate| !intersects_structural_obstacle(candidate))
            })
            .unwrap_or(route);
    }
    let current_target_side = side_for_outside_point(
        to.bounds,
        route.points.last().copied().unwrap_or(Point { x: 0, y: 0 }),
    );
    let preserved_targets: Vec<EdgeRoute> =
        buses.iter().map(|bus| route_via_bus(route.points[0], current_target_side, *bus)).collect();
    let same_sides: Vec<EdgeRoute> = candidate_right_bus_xs
        .iter()
        .flat_map(|bus_x| {
            vec![
                EdgeRoute {
                    label_axis: route.label_axis.map(|_| Axis::Y),
                    points: path_via_lane(
                        bounds_side_point(from.bounds, Side::Right, false),
                        lane(Axis::X, *bus_x),
                        bounds_side_point(to.bounds, Side::Right, false),
                    ),
                    ..route.clone()
                },
                EdgeRoute {
                    label_axis: route.label_axis.map(|_| Axis::Y),
                    points: path_via_lane(
                        bounds_side_point(from.bounds, Side::Left, false),
                        lane(Axis::X, *bus_x),
                        bounds_side_point(to.bounds, Side::Left, false),
                    ),
                    ..route.clone()
                },
            ]
        })
        .chain(candidate_left_bus_xs.iter().map(|bus_x| EdgeRoute {
            label_axis: route.label_axis.map(|_| Axis::Y),
            points: path_via_lane(
                bounds_side_point(from.bounds, Side::Left, false),
                lane(Axis::X, *bus_x),
                bounds_side_point(to.bounds, Side::Left, false),
            ),
            ..route.clone()
        }))
        .chain(candidate_top_bus_ys.iter().map(|bus_y| EdgeRoute {
            label_axis: route.label_axis.map(|_| Axis::X),
            points: path_via_lane(
                bounds_side_point(from.bounds, Side::Top, false),
                lane(Axis::Y, *bus_y),
                bounds_side_point(to.bounds, Side::Top, false),
            ),
            ..route.clone()
        }))
        .chain(candidate_bottom_bus_ys.iter().map(|bus_y| EdgeRoute {
            label_axis: route.label_axis.map(|_| Axis::X),
            points: path_via_lane(
                bounds_side_point(from.bounds, Side::Bottom, false),
                lane(Axis::Y, *bus_y),
                bounds_side_point(to.bounds, Side::Bottom, false),
            ),
            ..route.clone()
        }))
        .collect();
    let preserved_sources: Vec<EdgeRoute> = target_sides
        .iter()
        .flat_map(|target_side| {
            buses.iter().map(|bus| route_via_bus(route.points[0], *target_side, *bus))
        })
        .collect();
    let attachments: Vec<EdgeRoute> = target_sides
        .iter()
        .flat_map(|source_side| {
            target_sides
                .iter()
                .flat_map(|target_side| buses.iter().map(|bus| (*source_side, *target_side, *bus)))
        })
        .map(|(source_side, target_side, bus)| {
            route_via_bus(bounds_side_point(from.bounds, source_side, false), target_side, bus)
        })
        .collect();

    if subgraph_bounds.is_some() {
        return shortest(&preserved_targets, &|candidate| !intersects_obstacle(candidate))
            .or_else(|| shortest(&same_sides, &|candidate| !intersects_obstacle(candidate)))
            .or_else(|| shortest(&preserved_sources, &|candidate| !intersects_obstacle(candidate)))
            .or_else(|| shortest(&attachments, &|candidate| !intersects_obstacle(candidate)))
            .or_else(|| {
                shortest(&preserved_sources, &|candidate| {
                    !intersects_structural_obstacle(candidate)
                })
            })
            .or_else(|| {
                shortest(&attachments, &|candidate| !intersects_structural_obstacle(candidate))
            })
            .unwrap_or(route);
    }
    same_sides
        .iter()
        .find(|candidate| !intersects_obstacle(candidate))
        .cloned()
        .or_else(|| shortest(&preserved_targets, &|candidate| !intersects_obstacle(candidate)))
        .or_else(|| attachments.iter().find(|candidate| !intersects_obstacle(candidate)).cloned())
        .or_else(|| shortest(&preserved_sources, &|candidate| !intersects_obstacle(candidate)))
        .or_else(|| shortest(&attachments, &|candidate| !intersects_structural_obstacle(candidate)))
        .or_else(|| {
            shortest(&preserved_sources, &|candidate| !intersects_structural_obstacle(candidate))
        })
        .unwrap_or(route)
}

fn right_bus_x_dedup(lanes: &mut Vec<i64>, value: i64) {
    let value = 0.max(value);
    lanes.push(value);
}

fn bottom_bus_y_dedup(lanes: &mut Vec<i64>, value: i64) {
    lanes.push(value);
}

fn side_for_outside_point(bounds: Bounds, source_point: Point) -> Side {
    if source_point.x < bounds.left {
        return Side::Left;
    }
    if source_point.x >= bounds.left + bounds.width {
        return Side::Right;
    }
    if source_point.y < bounds.top {
        return Side::Top;
    }
    Side::Bottom
}

/// `avoidLabelOverlap`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn avoid_label_overlap(
    route: EdgeRoute,
    other_routes: &[EdgeRoute],
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: Option<&HashMap<String, SubgraphBounds>>,
    target_width: Option<i64>,
    include_label_width: bool,
) -> EdgeRoute {
    let edge = &diagram.edges[route.edge_index];
    if edge.label.is_empty() {
        return route;
    }
    let node_rects: Vec<RectBounds> =
        bounds.values().map(|bound| RectBounds::from(bound.bounds)).collect();
    let frame_bounds: Vec<&SubgraphBounds> =
        subgraph_bounds.map_or(Vec::new(), |frames| frames.values().collect());
    let other_labels: Vec<EdgeLabelLayout> = other_routes
        .iter()
        .filter(|other| !diagram.edges[other.edge_index].label.is_empty())
        .map(|other| {
            let label = &diagram.edges[other.edge_index].label;
            flowchart_route_label_layout(other.label_point, &other.points, label, other.label_axis)
        })
        .collect();
    let other_connector_bounds: Vec<RectBounds> = other_routes
        .iter()
        .filter_map(|other| {
            let source = bounds.get(&diagram.edges[other.edge_index].from)?;
            let source_point = other.points.first()?;
            let connector = flowchart_source_connector(source, *source_point);
            Some([
                RectBounds { left: connector.0, top: connector.1, width: 1, height: 1 },
                RectBounds { left: source_point.x, top: source_point.y, width: 1, height: 1 },
            ])
        })
        .flatten()
        .collect();
    let has_parallel_route = other_routes.iter().any(|other| {
        diagram.edges[other.edge_index].from == edge.from
            && diagram.edges[other.edge_index].to == edge.to
    });

    let intersects_obstacle = |label: &EdgeLabelLayout| -> bool {
        (target_width.is_some()
            && (if has_parallel_route || !include_label_width {
                label.point.x > target_width.unwrap()
            } else {
                label.point.x + label.width > target_width.unwrap()
            }))
            || node_rects.iter().any(|bound| label_intersects_bounds(Some(label), *bound))
            || frame_bounds.iter().any(|bound| label_intersects_subgraph_frame(Some(label), bound))
            || label_intersects_labels(Some(label), &other_labels)
            || label_intersects_route_paths(Some(label), other_routes)
            || other_connector_bounds
                .iter()
                .any(|bound| label_intersects_bounds(Some(label), *bound))
    };
    let current = flowchart_route_label_layout(
        route.label_point,
        &route.points,
        &edge.label,
        route.label_axis,
    );
    if !intersects_obstacle(&current) {
        return route;
    }

    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    let mut remaining = ROUTING_CANDIDATE_BUDGET;
    let mut available =
        |candidate: Point, remaining: &mut i64, seen: &mut HashSet<(i64, i64)>| -> bool {
            *remaining -= 1;
            let key = (candidate.x, candidate.y);
            if seen.contains(&key) {
                return false;
            }
            seen.insert(key);
            let mut probe = current.clone();
            probe.point = candidate;
            !intersects_obstacle(&probe)
        };

    for index in 1..route.points.len() {
        if remaining <= 0 {
            break;
        }
        let Some(segment) = segment_between(route.points[index - 1], route.points[index]) else {
            continue;
        };
        let label = flowchart_edge_label_layout(
            &route.points,
            &edge.label,
            route.label_axis,
            Some(index - 1),
        );
        let fixed: Vec<Point> = if segment.axis == Axis::Y {
            vec![label.point, Point { x: segment.from.x - label.width, y: label.point.y }]
        } else {
            vec![
                label.point,
                Point { x: label.point.x, y: segment.from.y - label.height },
                Point { x: label.point.x, y: segment.from.y + 1 },
            ]
        };
        for candidate in fixed {
            if remaining <= 0 {
                return route;
            }
            if available(candidate, &mut remaining, &mut seen) {
                let mut moved = route.clone();
                moved.label_point = Some(candidate);
                return moved;
            }
        }
        if segment.axis == Axis::Y {
            let top = segment.from.y.min(segment.to.y);
            let bottom = segment.from.y.max(segment.to.y) - label.height + 1;
            let mut y = top;
            while y <= bottom && remaining > 0 {
                for x in [segment.from.x + 1, segment.from.x - label.width] {
                    let candidate = Point { x, y };
                    if available(candidate, &mut remaining, &mut seen) {
                        let mut moved = route.clone();
                        moved.label_point = Some(candidate);
                        return moved;
                    }
                    if remaining <= 0 {
                        return route;
                    }
                }
                y += 1;
            }
            continue;
        }
        let left = segment.from.x.min(segment.to.x);
        let right = segment.from.x.max(segment.to.x) - label.width + 1;
        let mut x = left;
        while x <= right && remaining > 0 {
            for y in [segment.from.y, segment.from.y - label.height, segment.from.y + 1] {
                let candidate = Point { x, y };
                if available(candidate, &mut remaining, &mut seen) {
                    let mut moved = route.clone();
                    moved.label_point = Some(candidate);
                    return moved;
                }
                if remaining <= 0 {
                    return route;
                }
            }
            x += 1;
        }
    }

    if target_width.is_some()
        && include_label_width
        && current.point.x + current.width > target_width.unwrap()
    {
        let x = 0.max(target_width.unwrap() - current.width);
        for distance in 0..100i64 {
            let candidates: Vec<i64> = if distance == 0 {
                vec![current.point.y]
            } else {
                vec![current.point.y - distance, current.point.y + distance]
            };
            for y in candidates {
                if y < 0 {
                    continue;
                }
                let mut probe = current.clone();
                probe.point = Point { x, y };
                if !intersects_obstacle(&probe) {
                    let mut moved = route.clone();
                    moved.label_point = Some(Point { x, y });
                    return moved;
                }
            }
        }
    }
    route
}

/// `avoidFlowchartFrameBorders` — nudges route segments that run along
/// a subgraph frame border to one side.
pub fn avoid_flowchart_frame_borders(
    routes: &mut [EdgeRoute],
    diagram: &FlowchartDiagram,
    bounds: &HashMap<String, NodeBounds>,
    subgraph_bounds: &HashMap<String, SubgraphBounds>,
) {
    let inside = |node: &NodeBounds, frame: &SubgraphBounds| -> bool {
        node.bounds.left >= frame.bounds.left
            && node.bounds.top >= frame.bounds.top
            && node.bounds.left + node.bounds.width <= frame.bounds.left + frame.bounds.width
            && node.bounds.top + node.bounds.height <= frame.bounds.top + frame.bounds.height
    };

    for route in routes.iter_mut() {
        let edge = &diagram.edges[route.edge_index];
        let source = bounds.get(&edge.from);
        let target = bounds.get(&edge.to);
        for (_, frame) in subgraph_bounds.iter() {
            let inward = source.is_some_and(|source| inside(source, frame))
                && target.is_some_and(|target| inside(target, frame));
            let right = frame.bounds.left + frame.bounds.width - 1;
            let bottom = frame.bounds.top + frame.bounds.height - 1;
            let mut points: Vec<Point> = vec![route.points[0]];
            for (from, to) in
                route.points.windows(2).map(|window| (window[0], window[1])).collect::<Vec<_>>()
            {
                let Some(segment) = segment_between(from, to) else { continue };
                let span = segment_span(segment);
                let horizontal_side = if segment.axis == Axis::X
                    && span.end.min(right) > span.start.max(frame.bounds.left)
                {
                    if segment.from.y == frame.bounds.top {
                        Some(true)
                    } else if segment.from.y == bottom {
                        Some(false)
                    } else {
                        None
                    }
                } else {
                    None
                };
                let vertical_side = if segment.axis == Axis::Y
                    && span.end.min(bottom) > span.start.max(frame.bounds.top)
                {
                    if segment.from.x == frame.bounds.left {
                        Some(true)
                    } else if segment.from.x == right {
                        Some(false)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if horizontal_side.is_none() && vertical_side.is_none() {
                    points.push(to);
                    continue;
                }
                let offset = if let Some(top_side) = horizontal_side {
                    if top_side {
                        frame.bounds.top + i64::from(inward)
                    } else {
                        bottom - i64::from(inward)
                    }
                } else if vertical_side == Some(true) {
                    frame.bounds.left + i64::from(inward)
                } else {
                    right - i64::from(inward)
                };
                if horizontal_side.is_some() {
                    points.push(Point { x: from.x, y: offset });
                    points.push(Point { x: to.x, y: offset });
                    points.push(to);
                } else {
                    points.push(Point { x: offset, y: from.y });
                    points.push(Point { x: offset, y: to.y });
                    points.push(to);
                }
            }
            route.points = path_through(&points);
        }
    }
}

/// `flowchartSourceConnector`.
#[must_use]
pub fn flowchart_source_connector(
    from: &NodeBounds,
    source_point: Point,
) -> (i64, i64, &'static str) {
    let side = side_for_outside_point(from.bounds, source_point);
    let connector = bounds_side_point(from.bounds, side, true);
    let (x, y) = match side {
        Side::Top | Side::Bottom => (source_point.x, connector.y),
        Side::Left | Side::Right => (connector.x, source_point.y),
    };
    let char = match side {
        Side::Left => "┤",
        Side::Right => "├",
        Side::Top => "┴",
        Side::Bottom => "┬",
    };
    (x, y, char)
}
