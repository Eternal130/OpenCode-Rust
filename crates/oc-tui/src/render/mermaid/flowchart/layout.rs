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

//! `flowchart/layout.ts` — rank assignment, crossing reduction,
//! subgraph frames, and the top-level layout driver.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::render::mermaid::core::geometry::{
    bounds_from_bounds, bounds_from_points, segment_between, segment_span, translate_bounds, Axis,
    Bounds, Point,
};
use crate::render::mermaid::core::text::{
    diagram_text_width, measure_diagram_text_box, split_diagram_lines,
};
use crate::render::mermaid::flowchart::labels::{
    flowchart_edge_label_layout, flowchart_horizontal_label_rank_gap, flowchart_label_width,
    flowchart_route_label_layout, flowchart_vertical_branch_label_gap,
};
use crate::render::mermaid::flowchart::routing::{
    avoid_flowchart_frame_borders, route_flowchart_edges,
};
use crate::render::mermaid::flowchart::types::{
    EdgeRoute, FlowchartDiagram, FlowchartDirection, FlowchartEdge, FlowchartNode,
    FlowchartNodeShape, FlowchartRenderOptions, NodeBounds, NodeSize, SubgraphBounds,
};

pub const DEFAULT_MIN_NODE_GAP: i64 = 5;
pub const DEFAULT_MIN_BRANCH_LABEL_GAP: i64 = 12;
const DEFAULT_MAX_UNLABELED_RANK_WIDTH: i64 = 120;
pub const DEFAULT_MIN_RANK_GAP: i64 = 7;
pub const DEFAULT_MIN_VERTICAL_RANK_GAP: i64 = 4;
pub const COMPACT_MIN_RANK_GAP: i64 = 4;
pub const COMPACT_MIN_VERTICAL_RANK_GAP: i64 = 2;
const SUBGRAPH_PADDING_X: i64 = 2;
const SUBGRAPH_PADDING_TOP: i64 = 1;
const SUBGRAPH_PADDING_BOTTOM: i64 = 1;

/// `FlowchartLayout`.
pub struct FlowchartLayout {
    pub diagram: FlowchartDiagram,
    pub bounds: HashMap<String, NodeBounds>,
    pub routes: Vec<EdgeRoute>,
    pub subgraph_bounds: HashMap<String, SubgraphBounds>,
    pub width: i64,
    pub height: i64,
}

#[must_use]
pub fn is_horizontal_direction(direction: FlowchartDirection) -> bool {
    matches!(direction, FlowchartDirection::Lr | FlowchartDirection::Rl)
}

/// `visualLength`.
#[must_use]
pub fn visual_length(value: &str) -> i64 {
    diagram_text_width(value) as i64
}

/// `normalizePositiveInt`.
fn normalize_positive_int(value: Option<i64>, fallback: i64) -> i64 {
    value.map_or(fallback, |value| value.max(1))
}

/// `nodeSize`.
fn node_size(node: &FlowchartNode) -> NodeSize {
    let size = measure_diagram_text_box(&node.label, 2, 0, 1);
    let inner_width = size.width as i64 - 4;
    let lines = size.lines;
    let line_count = lines.len() as i64;
    match node.shape {
        FlowchartNodeShape::Decision => {
            let width = inner_width + 6;
            NodeSize {
                width: if width % 2 == 0 { width + 1 } else { width },
                height: 5.max(line_count + 4),
                lines,
            }
        }
        FlowchartNodeShape::Database => {
            NodeSize { width: inner_width + 4, height: line_count + 4, lines }
        }
        FlowchartNodeShape::Subroutine => {
            NodeSize { width: inner_width + 6, height: line_count + 2, lines }
        }
        _ => NodeSize { width: inner_width + 4, height: line_count + 2, lines },
    }
}

fn horizontal_rank_gaps(
    diagram: &FlowchartDiagram,
    normalized_ranks: &HashMap<String, i64>,
    rank_keys: &[i64],
    fallback: i64,
) -> Vec<i64> {
    let mut gaps = vec![fallback; rank_keys.len().saturating_sub(1)];
    let rank_indexes: HashMap<i64, usize> =
        rank_keys.iter().enumerate().map(|(index, rank)| (*rank, index)).collect();

    for edge in &diagram.edges {
        if edge.label.is_empty() {
            continue;
        }
        let Some(from_index) =
            normalized_ranks.get(&edge.from).and_then(|rank| rank_indexes.get(rank)).copied()
        else {
            continue;
        };
        let Some(to_index) =
            normalized_ranks.get(&edge.to).and_then(|rank| rank_indexes.get(rank)).copied()
        else {
            continue;
        };
        if from_index == to_index {
            continue;
        }
        let label_gap = flowchart_horizontal_label_rank_gap(flowchart_label_width(&edge.label));
        for index in from_index.min(to_index)..from_index.max(to_index) {
            gaps[index] = gaps[index].max(label_gap);
        }
    }
    gaps
}

fn vertical_rank_gaps(
    diagram: &FlowchartDiagram,
    normalized_ranks: &HashMap<String, i64>,
    rank_keys: &[i64],
    fallback: i64,
) -> Vec<i64> {
    let mut gaps = vec![fallback; rank_keys.len().saturating_sub(1)];
    let rank_indexes: HashMap<i64, usize> =
        rank_keys.iter().enumerate().map(|(index, rank)| (*rank, index)).collect();

    for edge in &diagram.edges {
        if edge.label.is_empty() {
            continue;
        }
        let Some(from_index) =
            normalized_ranks.get(&edge.from).and_then(|rank| rank_indexes.get(rank)).copied()
        else {
            continue;
        };
        let Some(to_index) =
            normalized_ranks.get(&edge.to).and_then(|rank| rank_indexes.get(rank)).copied()
        else {
            continue;
        };
        if from_index == to_index {
            continue;
        }
        let label_height = split_diagram_lines(&edge.label).len() as i64;
        for index in from_index.min(to_index)..from_index.max(to_index) {
            gaps[index] = gaps[index].max(label_height + 2);
        }
    }
    gaps
}

/// `rankNodes` — BFS ranks with the order-only edge expansion.
fn rank_nodes(diagram: &FlowchartDiagram) -> HashMap<String, i64> {
    let mut ranks: HashMap<String, i64> = HashMap::new();
    let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();
    let mut incoming: HashSet<String> = HashSet::new();
    let mut incoming_counts: HashMap<String, i64> =
        diagram.nodes.iter().map(|node| (node.id.clone(), 0i64)).collect();

    let node_ids: HashSet<String> = diagram.nodes.iter().map(|node| node.id.clone()).collect();
    let collect_subgraph_node_ids = |subgraph_id: &str| -> Vec<String> {
        diagram
            .subgraphs
            .iter()
            .filter(|subgraph| subgraph.id == subgraph_id)
            .flat_map(|subgraph| subgraph.node_ids.iter().cloned())
            .collect()
    };
    let mut rank_edges: Vec<(String, String)> = Vec::new();
    for edge in &diagram.edges {
        if !edge.order_only {
            rank_edges.push((edge.from.clone(), edge.to.clone()));
            continue;
        }
        let from_ids = if node_ids.contains(&edge.from) {
            vec![edge.from.clone()]
        } else {
            collect_subgraph_node_ids(&edge.from)
        };
        let to_ids = if node_ids.contains(&edge.to) {
            vec![edge.to.clone()]
        } else {
            collect_subgraph_node_ids(&edge.to)
        };
        for from in from_ids {
            for to in &to_ids {
                rank_edges.push((from.clone(), to.clone()));
            }
        }
    }

    for (from, to) in &rank_edges {
        outgoing.entry(from.clone()).or_default().push(to.clone());
        incoming.insert(to.clone());
        *incoming_counts.entry(to.clone()).or_default() += 1;
    }

    let mut starts: Vec<String> = diagram
        .nodes
        .iter()
        .filter(|node| !incoming.contains(&node.id))
        .map(|node| node.id.clone())
        .collect();
    if starts.is_empty() {
        if let Some(first) = diagram.nodes.first() {
            starts.push(first.id.clone());
        }
    }

    let mut queue = starts.clone();
    for node in &starts {
        ranks.insert(node.clone(), 0);
    }
    let mut cursor = 0usize;
    while cursor < queue.len() {
        let id = queue[cursor].clone();
        cursor += 1;
        let rank = ranks.get(&id).copied().unwrap_or(0);
        for to in outgoing.get(&id).map_or(&[] as &[String], |list| list.as_slice()) {
            let next_rank = rank + 1;
            if ranks.get(to).copied().unwrap_or(i64::MAX) <= next_rank {
                continue;
            }
            ranks.insert(to.clone(), next_rank);
            queue.push(to.clone());
        }
    }

    for node in &diagram.nodes {
        let fallback_rank = ranks.len() as i64;
        ranks.entry(node.id.clone()).or_insert(fallback_rank);
    }

    // Acyclic re-rank for feedback edges (topological longest path).
    let mut acyclic_ranks: HashMap<String, i64> = HashMap::new();
    let mut remaining = incoming_counts.clone();
    let mut acyclic_queue: Vec<String> = diagram
        .nodes
        .iter()
        .filter(|node| remaining.get(&node.id).copied().unwrap_or(0) == 0)
        .map(|node| node.id.clone())
        .collect();
    for id in &acyclic_queue {
        acyclic_ranks.insert(id.clone(), 0);
    }
    let mut cursor = 0usize;
    while cursor < acyclic_queue.len() {
        let id = acyclic_queue[cursor].clone();
        cursor += 1;
        let rank = acyclic_ranks.get(&id).copied().unwrap_or(0);
        for to in outgoing.get(&id).map_or(&[] as &[String], |list| list.as_slice()) {
            let candidate = (acyclic_ranks.get(to).copied().unwrap_or(0)).max(rank + 1);
            acyclic_ranks.insert(to.clone(), candidate);
            let remaining_incoming = remaining.get_mut(to).unwrap();
            *remaining_incoming -= 1;
            if *remaining_incoming == 0 {
                acyclic_queue.push(to.clone());
            }
        }
    }
    for (id, rank) in acyclic_ranks {
        ranks.insert(id, rank);
    }

    ranks
}

fn bounds_from_children(children: &[Bounds]) -> Option<Bounds> {
    bounds_from_bounds(children)
}

/// `subgraphBoundFromChildren`.
fn subgraph_bound_from_children(id: &str, label: &str, children: &[Bounds]) -> SubgraphBounds {
    let label_lines = split_diagram_lines(label);
    let label_height = label_lines.len() as i64;
    let left = children.iter().map(|child| child.left).min().unwrap_or(0) - SUBGRAPH_PADDING_X;
    let top = children.iter().map(|child| child.top).min().unwrap_or(0)
        - SUBGRAPH_PADDING_TOP.max(label_height);
    let right = children.iter().map(|child| child.left + child.width).max().unwrap_or(0)
        + SUBGRAPH_PADDING_X;
    let bottom = children.iter().map(|child| child.top + child.height).max().unwrap_or(0)
        + SUBGRAPH_PADDING_BOTTOM.max(label_height);
    let min_width = label_lines.iter().map(|line| visual_length(line)).max().unwrap_or(0) + 5;

    let (left, right) = if right - left < min_width {
        let extra = min_width - (right - left);
        (left - extra.div_euclid(2), right + extra.div_euclid(2) + extra.rem_euclid(2))
    } else {
        (left, right)
    };

    let width = right - left;
    let height = 3.max(bottom - top);
    SubgraphBounds {
        id: id.to_string(),
        label: label.to_string(),
        bounds: crate::render::mermaid::core::geometry::bounds_from_rect(left, top, width, height),
        label_side_top: true,
    }
}

fn spans_overlap(left_start: i64, left_end: i64, right_start: i64, right_end: i64) -> bool {
    left_start <= right_end && right_start <= left_end
}

#[derive(Debug, Clone, Copy)]
struct LabelSlot {
    left: i64,
    top: i64,
    width: i64,
    height: i64,
}

fn label_slot(bounds: &SubgraphBounds, top_side: bool) -> LabelSlot {
    let lines = split_diagram_lines(&bounds.label);
    let left = bounds.bounds.left + 2;
    let height = lines.len() as i64;
    let top = if top_side {
        bounds.bounds.top
    } else {
        bounds.bounds.top + bounds.bounds.height - height
    };
    let width = lines.iter().map(|line| visual_length(&format!(" {line} "))).max().unwrap_or(0);
    LabelSlot { left, top, width, height }
}

fn segment_overlaps_slot(from: Point, to: Point, slot: LabelSlot) -> bool {
    let Some(segment) = segment_between(from, to) else { return false };
    let slot_right = slot.left + slot.width - 1;
    let slot_bottom = slot.top + slot.height - 1;
    let span = segment_span(segment);
    if segment.axis == Axis::X {
        return segment.from.y >= slot.top
            && segment.from.y <= slot_bottom
            && spans_overlap(span.start, span.end, slot.left, slot_right);
    }
    segment.from.x >= slot.left
        && segment.from.x <= slot_right
        && spans_overlap(span.start, span.end, slot.top, slot_bottom)
}

fn route_overlaps_slot(route: &EdgeRoute, diagram: &FlowchartDiagram, slot: LabelSlot) -> bool {
    for window in route.points.windows(2) {
        if segment_overlaps_slot(window[0], window[1], slot) {
            return true;
        }
    }
    let Some(label) = route_label_bounds(route, diagram) else { return false };
    if !spans_overlap(label.top, label.top + label.height - 1, slot.top, slot.top + slot.height - 1)
    {
        return false;
    }
    spans_overlap(label.left, label.left + label.width - 1, slot.left, slot.left + slot.width - 1)
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    left: i64,
    top: i64,
    width: i64,
    height: i64,
}

fn route_label_bounds(route: &EdgeRoute, diagram: &FlowchartDiagram) -> Option<Rect> {
    let edge = &diagram.edges[route.edge_index];
    if edge.label.is_empty() {
        return None;
    }
    let label = flowchart_route_label_layout(
        route.label_point,
        &route.points,
        &edge.label,
        route.label_axis,
    );
    Some(Rect { left: label.point.x, top: label.point.y, width: label.width, height: label.height })
}

/// `chooseSubgraphLabelSide`.
fn choose_subgraph_label_side(
    bounds: &SubgraphBounds,
    routes: &[EdgeRoute],
    diagram: &FlowchartDiagram,
) -> bool {
    let top_slot = label_slot(bounds, true);
    if !routes.iter().any(|route| route_overlaps_slot(route, diagram, top_slot)) {
        return true;
    }
    let bottom_slot = label_slot(bounds, false);
    !routes.iter().any(|route| route_overlaps_slot(route, diagram, bottom_slot))
}

fn path_bounds(points: &[Point]) -> Option<Bounds> {
    bounds_from_points(points)
}

fn subgraph_route_bounds(
    subgraph_node_ids: &HashSet<String>,
    routes: &[EdgeRoute],
    diagram: &FlowchartDiagram,
) -> Vec<Bounds> {
    routes
        .iter()
        .filter(|route| {
            let edge = &diagram.edges[route.edge_index];
            subgraph_node_ids.contains(&edge.from) && subgraph_node_ids.contains(&edge.to)
        })
        .flat_map(route_render_bounds)
        .collect()
}

fn route_render_bounds(route: &EdgeRoute) -> Vec<Bounds> {
    let mut bounds = Vec::new();
    if let Some(route_bounds) = path_bounds(&route.points) {
        bounds.push(route_bounds);
    }
    // label bounds resolved later by the drawing pass; upstream computes
    // them here via flowchartRouteLabelLayout — mirrored by callers that
    // have the diagram handy.
    bounds
}

fn route_render_bounds_labeled(route: &EdgeRoute, diagram: &FlowchartDiagram) -> Vec<Bounds> {
    let mut bounds = route_render_bounds(route);
    if let Some(label) = route_label_bounds(route, diagram) {
        bounds.push(crate::render::mermaid::core::geometry::bounds_from_rect(
            label.left,
            label.top,
            label.width,
            label.height,
        ));
    }
    bounds
}

const CROSSING_SWEEPS: usize = 8;

/// `reduceRankCrossings` — barycenter sweeps (skipped when subgraphs
/// exist, exactly like upstream).
fn reduce_rank_crossings(
    diagram: &FlowchartDiagram,
    ranks: &HashMap<String, i64>,
    normalized_ranks: &HashMap<String, i64>,
    ranks_by_index: &mut BTreeMap<i64, Vec<usize>>,
) {
    if !diagram.subgraphs.is_empty() {
        return;
    }
    let rank_keys: Vec<i64> = ranks_by_index.keys().copied().collect();
    let layer_by_rank: HashMap<i64, usize> =
        rank_keys.iter().enumerate().map(|(index, rank)| (*rank, index)).collect();
    // Feedback edges route around the diagram: only forward edges between
    // adjacent ranks can cross.
    let mut links: Vec<(usize, String, String)> = Vec::new();
    for edge in &diagram.edges {
        let Some(from_rank) = ranks.get(&edge.from).copied() else { continue };
        let Some(to_rank) = ranks.get(&edge.to).copied() else { continue };
        if edge.order_only || to_rank != from_rank + 1 {
            continue;
        }
        let from = normalized_ranks.get(&edge.from).copied().unwrap_or(0);
        let to = normalized_ranks.get(&edge.to).copied().unwrap_or(0);
        if (from - to).abs() != 1 {
            continue;
        }
        if from < to {
            links.push((layer_by_rank[&from], edge.from.clone(), edge.to.clone()));
        } else {
            links.push((layer_by_rank[&to], edge.to.clone(), edge.from.clone()));
        }
    }

    let original: Vec<Vec<String>> = rank_keys
        .iter()
        .map(|rank| {
            ranks_by_index[rank].iter().map(|index| diagram.nodes[*index].id.clone()).collect()
        })
        .collect();
    let position: HashMap<&str, usize> = original
        .iter()
        .flat_map(|layer| layer.iter().enumerate().map(|(index, id)| (id.as_str(), index)))
        .collect();
    let initial_crossings = layer_crossings(&original, &links, &position);
    if initial_crossings == 0 {
        return;
    }

    let mut current = original.clone();
    let mut best = original.clone();
    let mut best_crossings = initial_crossings;
    for sweep in 0..CROSSING_SWEEPS {
        current = sweep_layers(&current, &links, sweep % 2 == 0);
        let position: HashMap<&str, usize> = current
            .iter()
            .flat_map(|layer| layer.iter().enumerate().map(|(index, id)| (id.as_str(), index)))
            .collect();
        let crossings = layer_crossings(&current, &links, &position);
        if crossings < best_crossings {
            best_crossings = crossings;
            best.clone_from(&current);
        }
    }
    if best == original {
        return;
    }

    let node_by_id: HashMap<&str, usize> =
        diagram.nodes.iter().enumerate().map(|(index, node)| (node.id.as_str(), index)).collect();
    for (index, rank) in rank_keys.iter().enumerate() {
        ranks_by_index
            .insert(*rank, best[index].iter().map(|id| node_by_id[id.as_str()]).collect());
    }
}

fn sweep_layers(
    layers: &[Vec<String>],
    links: &[(usize, String, String)],
    down: bool,
) -> Vec<Vec<String>> {
    let mut next = layers.to_vec();
    let indices: Vec<usize> = (0..next.len()).collect();
    let order: Vec<usize> = if down {
        indices.into_iter().skip(1).collect()
    } else {
        let mut reversed: Vec<usize> = indices[..next.len().saturating_sub(1)].to_vec();
        reversed.reverse();
        reversed
    };
    for index in order {
        let fixed_index = if down { index - 1 } else { index + 1 };
        let fixed: Vec<String> = next[fixed_index].clone();
        let mut keys: HashMap<String, f64> = HashMap::new();
        for (position, id) in next[index].iter().enumerate() {
            let mut neighbors: Vec<usize> = Vec::new();
            for (layer, upper, lower) in links {
                if down && *layer == index - 1 && lower == id {
                    neighbors
                        .push(fixed.iter().position(|candidate| candidate == upper).unwrap_or(0));
                }
                if !down && *layer == index && upper == id {
                    neighbors
                        .push(fixed.iter().position(|candidate| candidate == lower).unwrap_or(0));
                }
            }
            if neighbors.is_empty() {
                keys.insert(id.clone(), position as f64);
            } else {
                keys.insert(
                    id.clone(),
                    neighbors.iter().map(|value| *value as f64).sum::<f64>()
                        / neighbors.len() as f64,
                );
            }
        }
        next[index].sort_by(|a, b| keys[a].total_cmp(&keys[b]));
    }
    next
}

fn layer_crossings(
    layers: &[Vec<String>],
    links: &[(usize, String, String)],
    position: &HashMap<&str, usize>,
) -> i64 {
    let mut total = 0i64;
    for (index, link) in links.iter().enumerate() {
        for other in links.iter().skip(index + 1) {
            if other.0 != link.0 {
                continue;
            }
            let upper_a = position[link.1.as_str()] as i64;
            let lower_a = position[link.2.as_str()] as i64;
            let upper_b = position[other.1.as_str()] as i64;
            let lower_b = position[other.2.as_str()] as i64;
            if (upper_a - upper_b) * (lower_a - lower_b) < 0 {
                total += 1;
            }
        }
        let _ = layers;
    }
    total
}

/// `collectSubgraphNodeIds`.
fn collect_subgraph_node_ids(diagram: &FlowchartDiagram, subgraph_id: &str) -> HashSet<String> {
    fn walk(diagram: &FlowchartDiagram, subgraph_id: &str, ids: &mut HashSet<String>) {
        for subgraph in &diagram.subgraphs {
            if subgraph.id != subgraph_id && subgraph.parent_id.as_deref() != Some(subgraph_id) {
                continue;
            }
            for node_id in &subgraph.node_ids {
                ids.insert(node_id.clone());
            }
            if subgraph.parent_id.as_deref() == Some(subgraph_id) {
                walk(diagram, &subgraph.id, ids);
            }
        }
    }
    let mut ids = HashSet::new();
    walk(diagram, subgraph_id, &mut ids);
    ids
}

/// `rankGraphComponents`.
fn rank_graph_components(
    ids: &[String],
    outgoing: &HashMap<String, HashSet<String>>,
) -> HashMap<String, i64> {
    let mut reachable: HashMap<String, HashSet<String>> = HashMap::new();
    for id in ids {
        let mut seen = HashSet::new();
        let mut queue = vec![id.clone()];
        let mut cursor = 0usize;
        while cursor < queue.len() {
            let current = queue[cursor].clone();
            cursor += 1;
            if seen.contains(&current) {
                continue;
            }
            seen.insert(current.clone());
            for to in outgoing.get(&current).into_iter().flatten() {
                queue.push(to.clone());
            }
        }
        reachable.insert(id.clone(), seen);
    }

    let mut component_by_id: HashMap<String, usize> = HashMap::new();
    let mut components: Vec<Vec<String>> = Vec::new();
    for id in ids {
        if component_by_id.contains_key(id) {
            continue;
        }
        let component: Vec<String> = ids
            .iter()
            .filter(|candidate| {
                !component_by_id.contains_key(*candidate)
                    && reachable[id].contains(*candidate)
                    && reachable[candidate.as_str()].contains(id)
            })
            .cloned()
            .collect();
        let component_index = components.len();
        components.push(component.clone());
        for member in &component {
            component_by_id.insert(member.clone(), component_index);
        }
    }

    let mut component_outgoing: Vec<HashSet<usize>> = vec![HashSet::new(); components.len()];
    let mut incoming: Vec<i64> = vec![0; components.len()];
    for (from, targets) in outgoing {
        let Some(from_component) = component_by_id.get(from).copied() else { continue };
        for to in targets {
            let Some(to_component) = component_by_id.get(to).copied() else { continue };
            if from_component == to_component
                || component_outgoing[from_component].contains(&to_component)
            {
                continue;
            }
            component_outgoing[from_component].insert(to_component);
            incoming[to_component] += 1;
        }
    }

    let mut component_ranks: HashMap<usize, i64> = HashMap::new();
    let mut queue: Vec<usize> =
        (0..components.len()).filter(|index| incoming[*index] == 0).collect();
    for component in &queue {
        component_ranks.insert(*component, 0);
    }
    let mut cursor = 0usize;
    while cursor < queue.len() {
        let component = queue[cursor];
        cursor += 1;
        for to in &component_outgoing[component] {
            let candidate =
                component_ranks.get(to).copied().unwrap_or(0).max(component_ranks[&component] + 1);
            component_ranks.insert(*to, candidate);
            incoming[*to] -= 1;
            if incoming[*to] == 0 {
                queue.push(*to);
            }
        }
    }

    ids.iter()
        .map(|id| (id.clone(), component_ranks.get(&component_by_id[id]).copied().unwrap_or(0)))
        .collect()
}

struct RankedLayout {
    bounds: HashMap<String, NodeBounds>,
    wrapped: bool,
}

/// `layoutRankedNodes`.
fn layout_ranked_nodes(
    diagram: &FlowchartDiagram,
    direction: FlowchartDirection,
    sizes: &HashMap<String, NodeSize>,
    min_node_gap: i64,
    requested_min_rank_gap: i64,
    target_width: Option<i64>,
) -> RankedLayout {
    let horizontal = is_horizontal_direction(direction);
    let ranks = rank_nodes(diagram);
    let max_rank = ranks.values().copied().max().unwrap_or(0);
    let mut ranks_by_index: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    let mut normalized_ranks: HashMap<String, i64> = HashMap::new();
    for (index, node) in diagram.nodes.iter().enumerate() {
        let rank = ranks.get(&node.id).copied().unwrap_or(0);
        let normalized_rank =
            if direction == FlowchartDirection::Rl || direction == FlowchartDirection::Bt {
                max_rank - rank
            } else {
                rank
            };
        normalized_ranks.insert(node.id.clone(), normalized_rank);
        ranks_by_index.entry(normalized_rank).or_default().push(index);
    }
    reduce_rank_crossings(diagram, &ranks, &normalized_ranks, &mut ranks_by_index);

    let spacious_node_gap = min_node_gap.max(DEFAULT_MIN_BRANCH_LABEL_GAP);
    let widest_unlabeled_rank = ranks_by_index
        .values()
        .map(|nodes| {
            nodes.iter().map(|index| sizes[&diagram.nodes[*index].id].width).sum::<i64>()
                + 0.max(nodes.len() as i64 - 1) * spacious_node_gap
        })
        .max()
        .unwrap_or(0);

    let rank_keys: Vec<i64> = ranks_by_index.keys().copied().collect();
    let horizontal_gaps = if horizontal {
        horizontal_rank_gaps(diagram, &normalized_ranks, &rank_keys, requested_min_rank_gap)
    } else {
        Vec::new()
    };
    let vertical_gaps = if horizontal {
        Vec::new()
    } else {
        vertical_rank_gaps(diagram, &normalized_ranks, &rank_keys, requested_min_rank_gap)
    };
    let mut bounds: HashMap<String, NodeBounds> = HashMap::new();
    let mut wrapped = false;

    if horizontal {
        let column_widths: Vec<i64> = rank_keys
            .iter()
            .map(|rank| {
                ranks_by_index[rank]
                    .iter()
                    .map(|index| sizes[&diagram.nodes[*index].id].width)
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let column_heights: Vec<i64> = rank_keys
            .iter()
            .map(|rank| {
                let nodes = &ranks_by_index[rank];
                nodes.iter().map(|index| sizes[&diagram.nodes[*index].id].height).sum::<i64>()
                    + 0.max(nodes.len() as i64 - 1) * min_node_gap
            })
            .collect();
        let canvas_height = column_heights.iter().copied().max().unwrap_or(1).max(1);
        let center_baseline = canvas_height.div_euclid(2);
        let mut x = 0i64;
        for (rank_index, rank) in rank_keys.iter().enumerate() {
            let nodes = ranks_by_index[rank].clone();
            let column_width = column_widths[rank_index];
            let mut y = center_baseline - column_heights[rank_index].div_euclid(2);
            for index in nodes {
                let node = &diagram.nodes[index];
                let size = &sizes[&node.id];
                let left = x + (column_width - size.width).div_euclid(2);
                bounds.insert(
                    node.id.clone(),
                    NodeBounds {
                        id: node.id.clone(),
                        size: size.clone(),
                        bounds: crate::render::mermaid::core::geometry::bounds_from_rect(
                            left,
                            y,
                            size.width,
                            size.height,
                        ),
                    },
                );
                y += size.height + min_node_gap;
            }
            x += column_width + horizontal_gaps.get(rank_index).copied().unwrap_or(0);
        }
    } else {
        // Per-rank vertical band layout with optional wrapping.
        struct Band {
            node_indexes: Vec<usize>,
            width: i64,
            height: i64,
        }
        struct RankBands {
            bands: Vec<Band>,
            node_gap: i64,
        }
        let mut rank_bands: Vec<RankBands> = Vec::new();
        for rank in &rank_keys {
            let nodes = ranks_by_index[rank].clone();
            let roomy_node_gap = {
                let widest_label = diagram
                    .edges
                    .iter()
                    .filter(|edge| {
                        !edge.label.is_empty()
                            && (normalized_ranks.get(&edge.from).copied() == Some(*rank)
                                || normalized_ranks.get(&edge.to).copied() == Some(*rank))
                    })
                    .map(|edge| flowchart_label_width(&edge.label))
                    .max()
                    .unwrap_or(0);
                if widest_label > 0 {
                    spacious_node_gap.max(flowchart_vertical_branch_label_gap(widest_label))
                } else if widest_unlabeled_rank > DEFAULT_MAX_UNLABELED_RANK_WIDTH {
                    min_node_gap
                } else {
                    spacious_node_gap
                }
            };
            let natural_width =
                nodes.iter().map(|index| sizes[&diagram.nodes[*index].id].width).sum::<i64>()
                    + 0.max(nodes.len() as i64 - 1) * roomy_node_gap;
            let labeled_edges: Vec<&FlowchartEdge> = diagram
                .edges
                .iter()
                .filter(|edge| {
                    !edge.label.is_empty()
                        && (normalized_ranks.get(&edge.from).copied() == Some(*rank)
                            || normalized_ranks.get(&edge.to).copied() == Some(*rank))
                })
                .collect();
            let needs_label_lanes = labeled_edges.len() > 1
                && labeled_edges.iter().any(|edge| {
                    let targets: HashSet<&str> = labeled_edges
                        .iter()
                        .filter(|candidate| candidate.from == edge.from)
                        .map(|candidate| candidate.to.as_str())
                        .collect();
                    let sources: HashSet<&str> = labeled_edges
                        .iter()
                        .filter(|candidate| candidate.to == edge.to)
                        .map(|candidate| candidate.from.as_str())
                        .collect();
                    let grouped = |ids: Vec<&str>| -> bool {
                        diagram.subgraphs.is_empty()
                            || diagram.subgraphs.iter().any(|subgraph| {
                                ids.iter().all(|id| subgraph.node_ids.iter().any(|node| node == id))
                            })
                    };
                    let mut with_from = vec![edge.from.as_str()];
                    with_from.extend(targets.iter().copied());
                    let mut with_to = vec![edge.to.as_str()];
                    with_to.extend(sources.iter().copied());
                    (targets.len() > 1 && grouped(with_from))
                        || (sources.len() > 1 && grouped(with_to))
                });
            let node_gap = if target_width.is_some()
                && natural_width > target_width.unwrap()
                && (!needs_label_lanes || diagram.subgraphs.is_empty())
            {
                min_node_gap
            } else {
                roomy_node_gap
            };
            let mut bands: Vec<Band> = Vec::new();
            for index in nodes {
                let size = &sizes[&diagram.nodes[index].id];
                match bands.last_mut() {
                    Some(current) => {
                        let width = current.width + node_gap + size.width;
                        if target_width.is_some() && width > target_width.unwrap() {
                            wrapped = true;
                            bands.push(Band {
                                node_indexes: vec![index],
                                width: size.width,
                                height: size.height,
                            });
                            continue;
                        }
                        current.node_indexes.push(index);
                        current.width = width;
                        current.height = current.height.max(size.height);
                    }
                    None => bands.push(Band {
                        node_indexes: vec![index],
                        width: size.width,
                        height: size.height,
                    }),
                }
            }
            rank_bands.push(RankBands { bands, node_gap });
        }
        let canvas_width = rank_bands
            .iter()
            .flat_map(|rank| rank.bands.iter().map(|band| band.width))
            .max()
            .unwrap_or(0)
            .max(1);
        let mut y = 0i64;
        for (rank_index, rank) in rank_bands.iter().enumerate() {
            let band_count = rank.bands.len();
            for (band_index, band) in rank.bands.iter().enumerate() {
                let mut x = (canvas_width - band.width).div_euclid(2);
                for index in &band.node_indexes {
                    let node = &diagram.nodes[*index];
                    let size = &sizes[&node.id];
                    let top = y + (band.height - size.height).div_euclid(2);
                    bounds.insert(
                        node.id.clone(),
                        NodeBounds {
                            id: node.id.clone(),
                            size: size.clone(),
                            bounds: crate::render::mermaid::core::geometry::bounds_from_rect(
                                x,
                                top,
                                size.width,
                                size.height,
                            ),
                        },
                    );
                    x += size.width + rank.node_gap;
                }
                y += band.height + if band_index < band_count - 1 { min_node_gap } else { 0 };
            }
            y += vertical_gaps.get(rank_index).copied().unwrap_or(0);
        }
    }

    RankedLayout { bounds, wrapped }
}

/// `layoutLocalSubgraphDirections`.
fn layout_local_subgraph_directions(
    diagram: &FlowchartDiagram,
    node_bounds: &mut HashMap<String, NodeBounds>,
    min_node_gap: i64,
    requested_min_rank_gap: i64,
    target_width: Option<i64>,
) -> bool {
    let mut wrapped = false;
    let subgraphs = &diagram.subgraphs;
    let subgraph_by_id: HashMap<&str, usize> = subgraphs
        .iter()
        .enumerate()
        .map(|(index, subgraph)| (subgraph.id.as_str(), index))
        .collect();

    for position in (0..subgraphs.len()).rev() {
        let subgraph = &subgraphs[position];
        // nearest ancestor with a direction
        let mut parent =
            subgraph.parent_id.as_deref().and_then(|id| subgraph_by_id.get(id).copied());
        while let Some(parent_index) = parent {
            if subgraphs[parent_index].direction.is_some() {
                break;
            }
            parent = subgraphs[parent_index]
                .parent_id
                .as_deref()
                .and_then(|id| subgraph_by_id.get(id).copied());
        }
        let direction = subgraph.direction.or_else(|| {
            parent.and_then(|parent_index| {
                let siblings = subgraphs
                    .iter()
                    .filter(|child| child.parent_id.as_deref() == Some(&subgraphs[parent_index].id))
                    .count();
                if siblings > 1 {
                    subgraphs[parent_index].direction
                } else {
                    None
                }
            })
        });
        let Some(direction) = direction else { continue };
        if direction == diagram.direction {
            continue;
        }
        let child_subgraphs: Vec<usize> = subgraphs
            .iter()
            .enumerate()
            .filter(|(_, child)| child.parent_id.as_deref() == Some(&subgraph.id))
            .map(|(index, _)| index)
            .collect();
        let covered_node_ids: HashSet<String> = child_subgraphs
            .iter()
            .flat_map(|child_index| collect_subgraph_node_ids(diagram, &subgraphs[*child_index].id))
            .collect();

        #[derive(Clone)]
        struct Item {
            id: String,
            node_ids: Vec<String>,
            bounds: Bounds,
            child_id: Option<String>,
        }
        let mut items: Vec<Item> = Vec::new();
        for child_index in &child_subgraphs {
            let child = &subgraphs[*child_index];
            let node_ids: Vec<String> =
                collect_subgraph_node_ids(diagram, &child.id).into_iter().collect();
            let children: Vec<Bounds> = node_ids
                .iter()
                .filter_map(|id| node_bounds.get(id).map(|bound| bound.bounds))
                .collect();
            if let Some(content) = bounds_from_children(&children) {
                let bounds =
                    subgraph_bound_from_children(&child.id, &child.label, &[content]).bounds;
                items.push(Item {
                    id: format!("subgraph:{}", child.id),
                    node_ids,
                    bounds,
                    child_id: Some(child.id.clone()),
                });
            }
        }
        for node_id in &subgraph.node_ids {
            if covered_node_ids.contains(node_id) {
                continue;
            }
            if let Some(bound) = node_bounds.get(node_id) {
                items.push(Item {
                    id: node_id.clone(),
                    node_ids: vec![node_id.clone()],
                    bounds: bound.bounds,
                    child_id: None,
                });
            }
        }
        if items.is_empty() {
            continue;
        }
        let item_bounds: Vec<Bounds> = items.iter().map(|item| item.bounds).collect();
        let Some(current_bounds) = bounds_from_children(&item_bounds) else { continue };

        let mut item_by_endpoint: HashMap<String, String> = HashMap::new();
        for item in &items {
            for node_id in &item.node_ids {
                item_by_endpoint.insert(node_id.clone(), item.id.clone());
            }
            if let Some(child_id) = &item.child_id {
                item_by_endpoint.insert(child_id.clone(), item.id.clone());
            }
        }
        let local_nodes: Vec<FlowchartNode> = items
            .iter()
            .map(|item| FlowchartNode {
                id: item.id.clone(),
                label: item.id.clone(),
                shape: FlowchartNodeShape::Box,
            })
            .collect();
        let local_edges: Vec<FlowchartEdge> = diagram
            .edges
            .iter()
            .filter_map(|edge| {
                let from = item_by_endpoint.get(&edge.from)?;
                let to = item_by_endpoint.get(&edge.to)?;
                if from == to {
                    None
                } else {
                    Some(FlowchartEdge { from: from.clone(), to: to.clone(), ..edge.clone() })
                }
            })
            .collect();
        let local_diagram = FlowchartDiagram {
            direction,
            nodes: local_nodes,
            edges: local_edges,
            subgraphs: Vec::new(),
        };
        let mut item_sizes: HashMap<String, NodeSize> = HashMap::new();
        for item in &items {
            item_sizes.insert(
                item.id.clone(),
                NodeSize {
                    width: item.bounds.width,
                    height: item.bounds.height,
                    lines: vec![item.id.clone()],
                },
            );
        }
        let local_layout = layout_ranked_nodes(
            &local_diagram,
            direction,
            &item_sizes,
            min_node_gap.max(SUBGRAPH_PADDING_X * 2 + 1),
            requested_min_rank_gap,
            target_width,
        );
        let local_bounds = local_layout.bounds;
        wrapped |= local_layout.wrapped;
        let Some(local_extent) = bounds_from_children(
            &local_bounds.values().map(|bound| bound.bounds).collect::<Vec<_>>(),
        ) else {
            continue;
        };

        let target_left =
            current_bounds.left + (current_bounds.width - local_extent.width).div_euclid(2);
        let target_top =
            current_bounds.top + (current_bounds.height - local_extent.height).div_euclid(2);
        let dx = target_left - local_extent.left;
        let dy = target_top - local_extent.top;

        let mut translations: HashMap<String, (i64, i64)> = HashMap::new();
        for item in &items {
            let bound = &local_bounds[&item.id];
            let item_dx = bound.bounds.left + dx - item.bounds.left;
            let item_dy = bound.bounds.top + dy - item.bounds.top;
            for node_id in &item.node_ids {
                translations.insert(node_id.clone(), (item_dx, item_dy));
            }
        }

        let mut group_offset = (0i64, 0i64);
        if let Some(target_width) = target_width {
            let _ = target_width;
            let local_node_ids: HashSet<String> = translations.keys().cloned().collect();
            let external: Vec<Bounds> = node_bounds
                .iter()
                .filter(|(id, _)| !local_node_ids.contains(*id))
                .map(|(_, bound)| bound.bounds)
                .collect();
            let overlaps = |offset: (i64, i64)| -> bool {
                translations.iter().any(|(id, translation)| {
                    let bound = &node_bounds[id];
                    let left = bound.bounds.left + translation.0 + offset.0;
                    let top = bound.bounds.top + translation.1 + offset.1;
                    external.iter().any(|other| {
                        left < other.left + other.width + min_node_gap
                            && left + bound.bounds.width + min_node_gap > other.left
                            && top < other.top + other.height + min_node_gap
                            && top + bound.bounds.height + min_node_gap > other.top
                    })
                })
            };
            if overlaps(group_offset) {
                let vertical = !is_horizontal_direction(diagram.direction);
                let sign: i64 = if diagram.direction == FlowchartDirection::Rl
                    || diagram.direction == FlowchartDirection::Bt
                {
                    -1
                } else {
                    1
                };
                let mut found = false;
                'search: for distance in 1..1000i64 {
                    let candidates: [(i64, i64); 4] = if vertical {
                        [(0, sign * distance), (0, -sign * distance), (distance, 0), (-distance, 0)]
                    } else {
                        [(sign * distance, 0), (-sign * distance, 0), (0, distance), (0, -distance)]
                    };
                    for candidate in candidates {
                        if overlaps(candidate) {
                            continue;
                        }
                        group_offset = candidate;
                        found = true;
                        break 'search;
                    }
                }
                assert!(
                    found,
                    "Subgraph {} has no collision-free responsive position",
                    subgraph.id
                );
            }
        }

        for (node_id, translation) in &translations {
            if let Some(bound) = node_bounds.get_mut(node_id) {
                bound.bounds = translate_bounds(
                    bound.bounds,
                    translation.0 + group_offset.0,
                    translation.1 + group_offset.1,
                );
            }
        }
    }
    wrapped
}

fn edge_direction_for(diagram: &FlowchartDiagram, edge: &FlowchartEdge) -> FlowchartDirection {
    for subgraph in diagram.subgraphs.iter().rev() {
        let Some(direction) = subgraph.direction else { continue };
        if subgraph.node_ids.contains(&edge.from) && subgraph.node_ids.contains(&edge.to) {
            return direction;
        }
    }
    diagram.direction
}

fn translate_routes(routes: &mut [EdgeRoute], dx: i64, dy: i64) {
    for route in routes {
        for point in &mut route.points {
            point.x += dx;
            point.y += dy;
        }
        if let Some(label_point) = &mut route.label_point {
            label_point.x += dx;
            label_point.y += dy;
        }
    }
}

fn freeze_route_label_points(routes: &mut [EdgeRoute], diagram: &FlowchartDiagram) {
    for route in routes {
        let label = &diagram.edges[route.edge_index].label;
        if label.is_empty() || route.label_point.is_some() {
            continue;
        }
        route.label_point =
            Some(flowchart_edge_label_layout(&route.points, label, route.label_axis, None).point);
    }
}

/// `layoutSubgraphs`.
fn layout_subgraphs(
    diagram: &FlowchartDiagram,
    node_bounds: &HashMap<String, NodeBounds>,
    routes: &[EdgeRoute],
) -> HashMap<String, SubgraphBounds> {
    let mut subgraph_bounds: HashMap<String, SubgraphBounds> = HashMap::new();
    for position in (0..diagram.subgraphs.len()).rev() {
        let subgraph = &diagram.subgraphs[position];
        let mut children: Vec<Bounds> = Vec::new();
        for node_id in &subgraph.node_ids {
            if let Some(bound) = node_bounds.get(node_id) {
                children.push(bound.bounds);
            }
        }
        let node_id_set: HashSet<String> = subgraph.node_ids.iter().cloned().collect();
        children.extend(subgraph_route_bounds(&node_id_set, routes, diagram));
        for child_subgraph in &diagram.subgraphs {
            if child_subgraph.parent_id.as_deref() != Some(&subgraph.id) {
                continue;
            }
            if let Some(bound) = subgraph_bounds.get(&child_subgraph.id) {
                children.push(bound.bounds);
            }
        }
        if !children.is_empty() {
            let mut bound = subgraph_bound_from_children(&subgraph.id, &subgraph.label, &children);
            bound.label_side_top = choose_subgraph_label_side(&bound, routes, diagram);
            subgraph_bounds.insert(subgraph.id.clone(), bound);
        }
    }
    subgraph_bounds
}

/// `separateTopLevelItems`.
fn separate_top_level_items(
    diagram: &FlowchartDiagram,
    node_bounds: &mut HashMap<String, NodeBounds>,
    subgraph_bounds: &mut HashMap<String, SubgraphBounds>,
    gap: i64,
    target_width: Option<i64>,
) -> bool {
    let has_local_direction = diagram
        .subgraphs
        .iter()
        .any(|subgraph| subgraph.direction.is_some_and(|direction| direction != diagram.direction));
    let mut covered_node_ids: HashSet<String> = HashSet::new();
    #[derive(Clone)]
    struct Item {
        id: String,
        bounds: Bounds,
        node_ids: HashSet<String>,
        rank: i64,
    }
    let mut items: Vec<Item> = Vec::new();
    let mut item_by_endpoint: HashMap<String, String> = HashMap::new();
    let top_level_ids: HashSet<String> = diagram
        .subgraphs
        .iter()
        .filter(|subgraph| subgraph.parent_id.is_none())
        .map(|subgraph| subgraph.id.clone())
        .collect();
    let subgraph_by_id: HashMap<&str, usize> = diagram
        .subgraphs
        .iter()
        .enumerate()
        .map(|(index, subgraph)| (subgraph.id.as_str(), index))
        .collect();
    let top_level_subgraph_id = |id: &str| -> String {
        let mut current = subgraph_by_id.get(id).copied();
        while let Some(index) = current {
            if diagram.subgraphs[index].parent_id.is_some() {
                current = diagram.subgraphs[index]
                    .parent_id
                    .as_deref()
                    .and_then(|parent| subgraph_by_id.get(parent).copied());
            } else {
                return diagram.subgraphs[index].id.clone();
            }
        }
        id.to_string()
    };
    for subgraph in &diagram.subgraphs {
        if subgraph.parent_id.is_some() {
            continue;
        }
        let Some(bounds) = subgraph_bounds.get(&subgraph.id) else { continue };
        let node_ids = collect_subgraph_node_ids(diagram, &subgraph.id);
        if node_ids.is_empty() {
            continue;
        }
        items.push(Item {
            id: subgraph.id.clone(),
            bounds: bounds.bounds,
            node_ids: node_ids.clone(),
            rank: 0,
        });
        item_by_endpoint.insert(subgraph.id.clone(), subgraph.id.clone());
        for node_id in &node_ids {
            covered_node_ids.insert(node_id.clone());
            item_by_endpoint.insert(node_id.clone(), subgraph.id.clone());
        }
    }
    for subgraph in &diagram.subgraphs {
        item_by_endpoint.insert(subgraph.id.clone(), top_level_subgraph_id(&subgraph.id));
    }
    for node in &diagram.nodes {
        if covered_node_ids.contains(&node.id) {
            continue;
        }
        let Some(bound) = node_bounds.get(&node.id) else { continue };
        items.push(Item {
            id: node.id.clone(),
            bounds: bound.bounds,
            node_ids: HashSet::from([node.id.clone()]),
            rank: 0,
        });
        item_by_endpoint.insert(node.id.clone(), node.id.clone());
    }
    if items.len() < 2 {
        return false;
    }

    let horizontal = is_horizontal_direction(diagram.direction);
    let move_item = |item: &Item,
                     dx: i64,
                     dy: i64,
                     node_bounds: &mut HashMap<String, NodeBounds>,
                     subgraph_bounds: &mut HashMap<String, SubgraphBounds>| {
        for node_id in &item.node_ids {
            if let Some(bound) = node_bounds.get_mut(node_id) {
                bound.bounds = translate_bounds(bound.bounds, dx, dy);
            }
        }
        if !top_level_ids.contains(&item.id) {
            return;
        }
        for subgraph in &diagram.subgraphs {
            if top_level_subgraph_id(&subgraph.id) != item.id {
                continue;
            }
            if let Some(bound) = subgraph_bounds.get_mut(&subgraph.id) {
                bound.bounds = translate_bounds(bound.bounds, dx, dy);
            }
        }
    };

    if has_local_direction {
        let mut outgoing: HashMap<String, HashSet<String>> =
            items.iter().map(|item| (item.id.clone(), HashSet::new())).collect();
        for edge in &diagram.edges {
            let Some(from) = item_by_endpoint.get(&edge.from) else { continue };
            let Some(to) = item_by_endpoint.get(&edge.to) else { continue };
            if from != to {
                outgoing.entry(from.clone()).or_default().insert(to.clone());
            }
        }
        let ids: Vec<String> = items.iter().map(|item| item.id.clone()).collect();
        let ranks = rank_graph_components(&ids, &outgoing);
        items.sort_by(|a, b| {
            ranks[&a.id].cmp(&ranks[&b.id]).then(if horizontal {
                a.bounds.center_x.cmp(&b.bounds.center_x)
            } else {
                a.bounds.center_y.cmp(&b.bounds.center_y)
            })
        });
        let mut cursor: Option<i64> = None;
        let mut moved = false;
        for item in &items {
            let (start, size) = if horizontal {
                (item.bounds.left, item.bounds.width)
            } else {
                (item.bounds.top, item.bounds.height)
            };
            let Some(current_cursor) = cursor else {
                cursor = Some(start + size + gap);
                continue;
            };
            let shift = current_cursor - start;
            moved |= shift != 0;
            if horizontal {
                move_item(item, shift, 0, node_bounds, subgraph_bounds);
            } else {
                move_item(item, 0, shift, node_bounds, subgraph_bounds);
            }
            cursor = Some(start + shift + size + gap);
        }
        return moved;
    }

    let ranked_items: Vec<&Item> =
        items.iter().filter(|item| top_level_ids.contains(&item.id)).collect();
    if ranked_items.len() < 2 {
        return false;
    }

    let item_by_id: HashMap<&str, &Item> =
        ranked_items.iter().map(|item| (item.id.as_str(), *item)).collect();
    let mut outgoing: HashMap<String, HashSet<String>> =
        ranked_items.iter().map(|item| (item.id.clone(), HashSet::new())).collect();
    for edge in &diagram.edges {
        let Some(from) = item_by_endpoint.get(&edge.from) else { continue };
        let Some(to) = item_by_endpoint.get(&edge.to) else { continue };
        if from == to
            || !item_by_id.contains_key(from.as_str())
            || !item_by_id.contains_key(to.as_str())
        {
            continue;
        }
        outgoing.entry(from.clone()).or_default().insert(to.clone());
    }

    let ids: Vec<String> = ranked_items.iter().map(|item| item.id.clone()).collect();
    let ranks = rank_graph_components(&ids, &outgoing);
    let updates: Vec<(String, i64)> =
        ranked_items.iter().map(|item| (item.id.clone(), ranks[&item.id])).collect();
    for (id, rank) in updates {
        items.iter_mut().find(|candidate| candidate.id == id).expect("item").rank = rank;
    }

    let reversed =
        diagram.direction == FlowchartDirection::Rl || diagram.direction == FlowchartDirection::Bt;
    let primary_start = |item: &Item| -> i64 {
        let start = if horizontal { item.bounds.left } else { item.bounds.top };
        let size = if horizontal { item.bounds.width } else { item.bounds.height };
        if reversed {
            -(start + size)
        } else {
            start
        }
    };

    let mut items_by_rank: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        if top_level_ids.contains(&item.id) {
            items_by_rank.entry(item.rank).or_default().push(index);
        }
    }
    let mut cursor: Option<i64> = None;
    let mut moved = false;
    for (_, rank_items) in &items_by_rank {
        let start = rank_items.iter().map(|index| primary_start(&items[*index])).min().unwrap_or(0);
        let end = rank_items
            .iter()
            .map(|index| {
                let item = &items[*index];
                primary_start(item)
                    + if horizontal { item.bounds.width } else { item.bounds.height }
            })
            .max()
            .unwrap_or(0);
        let Some(current_cursor) = cursor else {
            cursor = Some(end + gap);
            continue;
        };
        let shift = 0.max(current_cursor - start);
        if shift > 0 {
            moved = true;
            for index in rank_items {
                let offset = if reversed { -shift } else { shift };
                if horizontal {
                    move_item(&items[*index].clone(), offset, 0, node_bounds, subgraph_bounds);
                } else {
                    move_item(&items[*index].clone(), 0, offset, node_bounds, subgraph_bounds);
                }
            }
        }
        cursor = Some(end + shift + gap);
    }

    for (_, rank_items) in &items_by_rank {
        let mut sorted: Vec<usize> = rank_items.clone();
        sorted.sort_by(|left, right| {
            let (a, b) = (&items[*left], &items[*right]);
            if horizontal {
                a.bounds.top.cmp(&b.bounds.top)
            } else {
                a.bounds.left.cmp(&b.bounds.left)
            }
        });
        let mut cross_cursor: Option<i64> = None;
        for index in sorted {
            let item = &items[index];
            let (start, size) = if horizontal {
                (item.bounds.top, item.bounds.height)
            } else {
                (item.bounds.left, item.bounds.width)
            };
            let Some(current_cursor) = cross_cursor else {
                cross_cursor = Some(start + size + gap);
                continue;
            };
            let shift = 0.max(current_cursor - start);
            if shift > 0 {
                moved = true;
                if horizontal {
                    move_item(&item.clone(), 0, shift, node_bounds, subgraph_bounds);
                } else {
                    move_item(&item.clone(), shift, 0, node_bounds, subgraph_bounds);
                }
            }
            cross_cursor = Some(start + shift + size + gap);
        }
    }

    if target_width.is_some() && !horizontal {
        let intersects = |left: &Item,
                          right: &Item,
                          node_bounds: &HashMap<String, NodeBounds>|
         -> bool {
            left.node_ids.iter().any(|left_id| {
                let Some(left_bounds) = node_bounds.get(left_id).map(|bound| bound.bounds) else {
                    return false;
                };
                right.node_ids.iter().any(|right_id| {
                    let Some(right_bounds) = node_bounds.get(right_id).map(|bound| bound.bounds)
                    else {
                        return false;
                    };
                    left_bounds.left < right_bounds.left + right_bounds.width
                        && left_bounds.left + left_bounds.width > right_bounds.left
                        && left_bounds.top < right_bounds.top + right_bounds.height
                        && left_bounds.top + left_bounds.height > right_bounds.top
                })
            })
        };
        for right_index in 1..items.len() {
            for left_index in 0..right_index {
                if !intersects(&items[left_index], &items[right_index], node_bounds) {
                    continue;
                }
                let left_bounds = bounds_from_children(
                    &items[left_index]
                        .node_ids
                        .iter()
                        .filter_map(|id| node_bounds.get(id).map(|bound| bound.bounds))
                        .collect::<Vec<_>>(),
                )
                .expect("left bounds");
                let right_bounds = bounds_from_children(
                    &items[right_index]
                        .node_ids
                        .iter()
                        .filter_map(|id| node_bounds.get(id).map(|bound| bound.bounds))
                        .collect::<Vec<_>>(),
                )
                .expect("right bounds");
                let shift = if reversed {
                    left_bounds.top - gap - (right_bounds.top + right_bounds.height)
                } else {
                    left_bounds.top + left_bounds.height + gap - right_bounds.top
                };
                moved |= shift != 0;
                move_item(&items[right_index].clone(), 0, shift, node_bounds, subgraph_bounds);
            }
        }
    }
    moved
}

/// `layoutFlowchartWithDirection`.
fn layout_flowchart_with_direction(
    source_diagram: &FlowchartDiagram,
    options: &FlowchartRenderOptions,
    direction: FlowchartDirection,
    responsive_fallback: bool,
) -> FlowchartLayout {
    let mut diagram = source_diagram.clone();
    if direction != source_diagram.direction {
        diagram.direction = direction;
    }
    let horizontal = is_horizontal_direction(direction);
    let compact = options.compact.unwrap_or(true);
    let min_node_gap = normalize_positive_int(options.min_node_gap, DEFAULT_MIN_NODE_GAP);
    let default_rank_gap = if compact {
        if horizontal {
            COMPACT_MIN_RANK_GAP
        } else {
            COMPACT_MIN_VERTICAL_RANK_GAP
        }
    } else if horizontal {
        DEFAULT_MIN_RANK_GAP
    } else {
        DEFAULT_MIN_VERTICAL_RANK_GAP
    };
    let requested_min_rank_gap = normalize_positive_int(options.min_rank_gap, default_rank_gap);
    let sizes: HashMap<String, NodeSize> =
        diagram.nodes.iter().map(|node| (node.id.clone(), node_size(node))).collect();
    let target_width =
        if horizontal { None } else { options.layout_max_width.map(|width| 1.max(width)) };
    let ranked = layout_ranked_nodes(
        &diagram,
        direction,
        &sizes,
        min_node_gap,
        requested_min_rank_gap,
        target_width,
    );
    let mut bounds = ranked.bounds;
    let responsive = layout_local_subgraph_directions(
        &diagram,
        &mut bounds,
        min_node_gap,
        requested_min_rank_gap,
        target_width,
    );
    let direction_aligned = responsive_fallback || responsive || ranked.wrapped;

    let route = |bounds: &HashMap<String, NodeBounds>,
                 frames: Option<&HashMap<String, SubgraphBounds>>|
     -> Vec<EdgeRoute> {
        route_flowchart_edges(
            &diagram,
            bounds,
            |edge| edge_direction_for(&diagram, edge),
            frames,
            target_width,
            direction_aligned,
        )
    };

    let mut subgraph_bounds: HashMap<String, SubgraphBounds> = HashMap::new();
    let mut routes: Vec<EdgeRoute>;
    if diagram.subgraphs.is_empty() {
        routes = route(&bounds, None);
    } else {
        routes = route(&bounds, None);
        subgraph_bounds = layout_subgraphs(&diagram, &bounds, &routes);
        let gap = 1.max(requested_min_rank_gap.div_euclid(2));
        let moved = separate_top_level_items(
            &diagram,
            &mut bounds,
            &mut subgraph_bounds,
            gap,
            target_width,
        );
        if moved {
            routes = route(&bounds, None);
            subgraph_bounds = layout_subgraphs(&diagram, &bounds, &routes);
        }
        routes = route(&bounds, Some(&subgraph_bounds));
        subgraph_bounds = layout_subgraphs(&diagram, &bounds, &routes);
        let expanded = separate_top_level_items(
            &diagram,
            &mut bounds,
            &mut subgraph_bounds,
            gap,
            target_width,
        );
        if expanded {
            routes = route(&bounds, Some(&subgraph_bounds));
        }
        avoid_flowchart_frame_borders(&mut routes, &diagram, &bounds, &subgraph_bounds);
    }
    freeze_route_label_points(&mut routes, &diagram);

    let mut all_bounds: Vec<Bounds> = bounds.values().map(|bound| bound.bounds).collect();
    all_bounds.extend(subgraph_bounds.values().map(|bound| bound.bounds));
    for route in &routes {
        all_bounds.extend(route_render_bounds_labeled(route, &diagram));
    }
    let dx = 0.max(-all_bounds.iter().map(|bound| bound.left).min().unwrap_or(0));
    let dy = 0.max(-all_bounds.iter().map(|bound| bound.top).min().unwrap_or(0));
    if dx > 0 || dy > 0 {
        for bound in bounds.values_mut() {
            bound.bounds = translate_bounds(bound.bounds, dx, dy);
        }
        for bound in subgraph_bounds.values_mut() {
            bound.bounds = translate_bounds(bound.bounds, dx, dy);
        }
        translate_routes(&mut routes, dx, dy);
    }

    let max_x = 0.max(all_bounds.iter().map(|bound| bound.left + bound.width).max().unwrap_or(0));
    let max_y = 0.max(all_bounds.iter().map(|bound| bound.top + bound.height).max().unwrap_or(0));
    FlowchartLayout {
        diagram,
        bounds,
        routes,
        subgraph_bounds,
        width: max_x + 4,
        height: max_y + 4,
    }
}

/// `layoutFlowchartDiagram`.
pub fn layout_flowchart_diagram(
    source_diagram: &FlowchartDiagram,
    options: &FlowchartRenderOptions,
) -> FlowchartLayout {
    let direction = options.direction.unwrap_or(source_diagram.direction);
    let layout = layout_flowchart_with_direction(source_diagram, options, direction, false);
    let Some(max_width) = options.layout_max_width else { return layout };
    if !is_horizontal_direction(direction) || max_width < 1 {
        return layout;
    }
    if layout.width <= 1.max(max_width) {
        return layout;
    }

    layout_flowchart_with_direction(
        source_diagram,
        options,
        if direction == FlowchartDirection::Rl {
            FlowchartDirection::Bt
        } else {
            FlowchartDirection::Td
        },
        true,
    )
}
