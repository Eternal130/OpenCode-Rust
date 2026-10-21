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

//! `state/routing.ts` — route classification (feedback rails, parallel
//! lanes, elbows), the ASCII route painters, body-safe repair, and
//! transition-label placement.

use std::collections::{HashMap, HashSet};

use crate::render::mermaid::core::drawing::{
    diagram_arrow_head, diagram_line_glyph, merge_diagram_line_glyph, ArrowHeadStyle,
    BORDER_ROUNDED,
};
use crate::render::mermaid::core::geometry::{orthogonal_path_points, Direction, Point};
use crate::render::mermaid::core::spatial::{
    spatial_path_claim, spatial_rect_claim, Clearance, CollisionPolicy, SpatialIndex, SpatialRole,
};
use crate::render::mermaid::core::text::diagram_text_width;
use crate::render::mermaid::state::layout::{
    placed_note_connector, StateDiagramBoxBounds, StateDiagramLayout, StateDiagramNoteBounds,
};
use crate::render::mermaid::state::search::{
    create_state_search_space, find_state_manhattan_path, StateSearchBounds, StateSearchBudget,
    StateSearchSpace,
};
use crate::render::mermaid::state::types::{
    StateCellStyle, StateDiagram, StateDiagramState, StateDiagramTransition, StateDirection,
    StateKind, StateRenderError,
};
use crate::render::mermaid::state::visible_model::is_hidden_composite_marker;

/// `measureStateTransitionLabel`'s result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTransitionLabelMetrics {
    pub lines: Vec<String>,
    pub width: i64,
    pub height: i64,
}

/// `measureStateTransitionLabel`.
#[must_use]
pub fn measure_state_transition_label(label: &str) -> StateTransitionLabelMetrics {
    if label.is_empty() {
        return StateTransitionLabelMetrics { lines: Vec::new(), width: 0, height: 0 };
    }
    let lines = crate::render::mermaid::core::text_lines::split_diagram_lines(label);
    let width = lines.iter().map(|line| diagram_text_width(line) as i64).max().unwrap_or(0);
    let height = lines.len() as i64;
    StateTransitionLabelMetrics { lines, width, height }
}

/// `hasReverseTransition`.
#[must_use]
pub fn has_reverse_transition(diagram: &StateDiagram, transition: &StateDiagramTransition) -> bool {
    diagram
        .transitions
        .iter()
        .any(|other| other.from == transition.to && other.to == transition.from)
}

/// `isStateHorizontalFeedback`.
#[must_use]
pub fn is_state_horizontal_feedback(
    diagram: &StateDiagram,
    from: &StateDiagramBoxBounds,
    to: &StateDiagramBoxBounds,
) -> bool {
    if diagram.direction == StateDirection::Rl {
        return to.center_x > from.center_x;
    }
    to.center_x < from.center_x
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedbackSide {
    Bottom,
    Top,
}

/// `FeedbackAllocation`.
#[derive(Debug, Clone, Copy)]
struct FeedbackAllocation {
    side: FeedbackSide,
    rail_y: i64,
}

#[derive(Debug, Clone)]
struct FeedbackInterval {
    transition_index: usize,
    left: i64,
    right: i64,
    side: Option<FeedbackSide>,
}

fn feedback_intervals_overlap(left: (i64, i64), right: (i64, i64)) -> bool {
    left.0 <= right.1 && right.0 <= left.1
}

fn feedback_intervals_cross(left: (i64, i64), right: (i64, i64)) -> bool {
    (left.0 < right.0 && right.0 < left.1 && left.1 < right.1)
        || (right.0 < left.0 && left.0 < right.1 && right.1 < left.1)
}

/// `createFeedbackAllocations`.
fn create_feedback_allocations(
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    feedback_lane_y: i64,
    lane_gap: i64,
    feedback_top_y: Option<i64>,
) -> HashMap<usize, FeedbackAllocation> {
    if diagram.direction != StateDirection::Lr && diagram.direction != StateDirection::Rl {
        return HashMap::new();
    }
    let mut allocations: HashMap<usize, FeedbackAllocation> = HashMap::new();
    let mut sided_intervals: [Vec<FeedbackInterval>; 2] = [Vec::new(), Vec::new()];
    let mut canonical_sides: HashMap<String, FeedbackSide> = HashMap::new();
    let top_lane_y = feedback_top_y
        .unwrap_or_else(|| layout.boxes.iter().map(|bound| bound.top).min().unwrap_or(0) - 3);
    let mut intervals: Vec<FeedbackInterval> = Vec::new();

    for (index, transition) in diagram.transitions.iter().enumerate() {
        let Some(&from_index) = layout.bounds.get(&transition.from) else { continue };
        let Some(&to_index) = layout.bounds.get(&transition.to) else { continue };
        let (from, to) = (&layout.boxes[from_index], &layout.boxes[to_index]);
        if transition.from == transition.to || !is_state_horizontal_feedback(diagram, from, to) {
            continue;
        }
        if from.center_y < to.center_y {
            continue;
        }
        intervals.push(FeedbackInterval {
            transition_index: index,
            left: from.center_x.min(to.center_x),
            right: from.center_x.max(to.center_x),
            side: None,
        });
    }

    for interval in intervals {
        let endpoint_key = format!(
            "{}\0{}",
            diagram.transitions[interval.transition_index].from,
            diagram.transitions[interval.transition_index].to
        );
        let span = (interval.left, interval.right);
        let side = canonical_sides.get(&endpoint_key).copied().or_else(|| {
            [FeedbackSide::Bottom, FeedbackSide::Top].into_iter().find(|&candidate| {
                let list = match candidate {
                    FeedbackSide::Bottom => &sided_intervals[0],
                    FeedbackSide::Top => &sided_intervals[1],
                };
                !list
                    .iter()
                    .any(|existing| feedback_intervals_cross((existing.left, existing.right), span))
            })
        });
        let Some(side) = side else { continue };
        canonical_sides.insert(endpoint_key, side);
        let mut interval = interval;
        interval.side = Some(side);
        match side {
            FeedbackSide::Bottom => sided_intervals[0].push(interval),
            FeedbackSide::Top => sided_intervals[1].push(interval),
        }
    }

    for (side_index, side) in [FeedbackSide::Bottom, FeedbackSide::Top].into_iter().enumerate() {
        let mut occupied: Vec<(i64, i64, i64)> = Vec::new(); // (left, right, lane)
        let mut intervals_by_width = sided_intervals[side_index].clone();
        intervals_by_width.sort_by_key(|interval| interval.right - interval.left);
        for interval in intervals_by_width {
            let mut lane = 0i64;
            while occupied.iter().any(|existing| {
                existing.2 == lane
                    && feedback_intervals_overlap(
                        (existing.0, existing.1),
                        (interval.left, interval.right),
                    )
            }) {
                lane += 1;
            }
            let rail_y = match side {
                FeedbackSide::Bottom => feedback_lane_y + lane * lane_gap,
                FeedbackSide::Top => top_lane_y - lane * lane_gap,
            };
            occupied.push((interval.left, interval.right, lane));
            allocations.insert(interval.transition_index, FeedbackAllocation { side, rail_y });
        }
    }

    allocations
}

/// `hasOpposingTopConnector`.
fn has_opposing_top_connector(
    diagram: &StateDiagram,
    transition_index: usize,
    layout: &StateDiagramLayout,
) -> bool {
    let transition = &diagram.transitions[transition_index];
    let Some(&from_index) = layout.bounds.get(&transition.from) else { return false };
    let Some(&to_index) = layout.bounds.get(&transition.to) else { return false };
    let (from, to) = (&layout.boxes[from_index], &layout.boxes[to_index]);
    if from.center_y == to.center_y {
        return false;
    }

    let (lower_id, lower_index) = if from.center_y > to.center_y {
        (transition.from.as_str(), from_index)
    } else {
        (transition.to.as_str(), to_index)
    };
    let leaves_lower = transition.from.as_str() == lower_id;
    let lower = &layout.boxes[lower_index];
    diagram.transitions.iter().enumerate().any(|(other_index, other)| {
        if other_index == transition_index {
            return false;
        }
        if leaves_lower {
            if other.to != lower_id {
                return false;
            }
        } else if other.from != lower_id {
            return false;
        }
        let Some(&other_from_index) = layout.bounds.get(other.from.as_str()) else { return false };
        let Some(&other_to_index) = layout.bounds.get(other.to.as_str()) else { return false };
        let (other_from, other_to) =
            (&layout.boxes[other_from_index], &layout.boxes[other_to_index]);
        if other_from.center_y.max(other_to.center_y) != lower.center_y {
            return false;
        }
        if other.from != lower_id {
            return true;
        }
        !((diagram.direction == StateDirection::Lr || diagram.direction == StateDirection::Rl)
            && is_state_horizontal_feedback(diagram, other_from, other_to))
    })
}

/// `verticalCorridorCrossesUnrelatedState`.
fn vertical_corridor_crosses_unrelated_state(
    diagram: &StateDiagram,
    transition_index: usize,
    from: &StateDiagramBoxBounds,
    to: &StateDiagramBoxBounds,
    layout: &StateDiagramLayout,
) -> bool {
    let transition = &diagram.transitions[transition_index];
    let top = (from.top + from.height).min(to.top + to.height);
    let bottom = (from.top - 1).max(to.top - 1);
    diagram.states.iter().any(|state| {
        if state.id == transition.from || state.id == transition.to {
            return false;
        }
        if is_hidden_composite_marker(Some(state)) {
            return false;
        }
        let Some(&bound_index) = layout.bounds.get(state.id.as_str()) else { return false };
        let bound = &layout.boxes[bound_index];
        from.center_x >= bound.left
            && from.center_x < bound.left + bound.width
            && top < bound.top + bound.height
            && bottom >= bound.top
    })
}

/// `hasVerticalCorridor`.
fn has_vertical_corridor(from: &StateDiagramBoxBounds, to: &StateDiagramBoxBounds) -> bool {
    if from.center_y < to.center_y {
        return from.top + from.height < to.top;
    }
    from.top > to.top + to.height
}

/// `horizontalCorridorCrossesUnrelatedState`.
fn horizontal_corridor_crosses_unrelated_state(
    diagram: &StateDiagram,
    transition_index: usize,
    from: &StateDiagramBoxBounds,
    to: &StateDiagramBoxBounds,
    layout: &StateDiagramLayout,
) -> bool {
    let transition = &diagram.transitions[transition_index];
    let left_to_right = from.center_x <= to.center_x;
    let start_x = if left_to_right { from.left + from.width } else { from.left - 1 };
    let end_x = if left_to_right { to.left - 1 } else { to.left + to.width };
    let mut claims: Vec<_> = Vec::new();
    for state in &diagram.states {
        if state.id == transition.from || state.id == transition.to {
            continue;
        }
        if is_hidden_composite_marker(Some(state)) {
            continue;
        }
        if let Some(&bound_index) = layout.bounds.get(state.id.as_str()) {
            let bound = &layout.boxes[bound_index];
            claims.push(crate::render::mermaid::core::spatial::spatial_rect_claim(
                format!("state:{}", state.id),
                format!("state:{}", state.id),
                SpatialRole::Body,
                &crate::render::mermaid::core::geometry::bounds_from_rect(
                    bound.left,
                    bound.top,
                    bound.width,
                    bound.height,
                ),
            ));
        }
    }
    let space = SpatialIndex::empty().add(claims);
    let corridor = spatial_path_claim(
        format!("corridor:{}:{}", transition.from, transition.to),
        format!("transition:{}:{}", transition.from, transition.to),
        SpatialRole::Route,
        &[Point { x: start_x, y: from.center_y }, Point { x: end_x, y: from.center_y }],
    );
    !space.is_free(&corridor, &CollisionPolicy::default())
}

/// `bottomApproachX`.
fn bottom_approach_x(
    diagram: &StateDiagram,
    transition_index: usize,
    from: &StateDiagramBoxBounds,
    to: &StateDiagramBoxBounds,
    layout: &StateDiagramLayout,
    rail_y: i64,
) -> i64 {
    let transition = &diagram.transitions[transition_index];
    let target_x = if to.width > 1 {
        if from.center_x > to.center_x {
            to.left + 1
        } else {
            to.left + to.width - 2
        }
    } else {
        to.center_x
    };
    let target_bottom_y = to.top + to.height;
    let top = target_bottom_y.min(rail_y);
    let bottom = target_bottom_y.max(rail_y);
    let is_clear = |x: i64| -> bool {
        !diagram.states.iter().any(|state| {
            if state.id == transition.from || state.id == transition.to {
                return false;
            }
            if is_hidden_composite_marker(Some(state)) {
                return false;
            }
            let Some(&bound_index) = layout.bounds.get(state.id.as_str()) else { return false };
            let bound = &layout.boxes[bound_index];
            x >= bound.left
                && x < bound.left + bound.width
                && top < bound.top + bound.height
                && bottom >= bound.top
        })
    };

    if is_clear(target_x) {
        return target_x;
    }
    let max_x = target_x
        .max(layout.boxes.iter().map(|bound| bound.left + bound.width).max().unwrap_or(0))
        + 1;
    for distance in 1..=max_x {
        let right = target_x + distance;
        if is_clear(right) {
            return right;
        }
        let left = target_x - distance;
        if left >= 0 && is_clear(left) {
            return left;
        }
    }
    target_x
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetApproach {
    Top,
    Bottom,
}

/// `sideParallelTargetApproach`.
fn side_parallel_target_approach(
    diagram: &StateDiagram,
    transition_index: usize,
    from: &StateDiagramBoxBounds,
    to: &StateDiagramBoxBounds,
    layout: &StateDiagramLayout,
    rail_x: i64,
) -> Option<TargetApproach> {
    let transition = &diagram.transitions[transition_index];
    let mut claims: Vec<_> = Vec::new();
    for state in &diagram.states {
        if state.id == transition.from || state.id == transition.to {
            continue;
        }
        if is_hidden_composite_marker(Some(state)) {
            continue;
        }
        if let Some(&bound_index) = layout.bounds.get(state.id.as_str()) {
            let bound = &layout.boxes[bound_index];
            claims.push(crate::render::mermaid::core::spatial::spatial_rect_claim(
                format!("state:{}", state.id),
                format!("state:{}", state.id),
                SpatialRole::Body,
                &crate::render::mermaid::core::geometry::bounds_from_rect(
                    bound.left,
                    bound.top,
                    bound.width,
                    bound.height,
                ),
            ));
        }
    }
    let space = SpatialIndex::empty().add(claims);
    let target_side_x = to.left + to.width;
    let claim = |points: &[Point]| {
        spatial_path_claim(
            format!("side-target:{}:{}", transition.from, transition.to),
            "side-target",
            SpatialRole::Route,
            points,
        )
    };
    let policy = CollisionPolicy::default();

    if space.is_free(
        &claim(&[Point { x: rail_x, y: to.center_y }, Point { x: target_side_x, y: to.center_y }]),
        &policy,
    ) {
        return None;
    }

    let target_x = inner_connector_x(to, from.center_x);
    let preferred =
        if from.center_y > to.center_y { TargetApproach::Top } else { TargetApproach::Bottom };
    [
        preferred,
        if preferred == TargetApproach::Top { TargetApproach::Bottom } else { TargetApproach::Top },
    ]
    .into_iter()
    .find(|&side| {
        let (rail_y, target_y) = match side {
            TargetApproach::Top => (to.top - 2, to.top - 1),
            TargetApproach::Bottom => (to.top + to.height + 1, to.top + to.height),
        };
        space.is_free(
            &claim(&[
                Point { x: rail_x, y: rail_y },
                Point { x: target_x, y: rail_y },
                Point { x: target_x, y: target_y },
            ]),
            &policy,
        )
    })
}

/// `containingCompositeIds`.
fn containing_composite_ids<'a>(diagram: &'a StateDiagram, id: &str) -> Vec<&'a str> {
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let composites_by_id: HashMap<
        &str,
        &crate::render::mermaid::state::types::StateDiagramCompositeState,
    > = diagram.composites.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut ids: Vec<&str> = Vec::new();
    let mut parent_id = states_by_id
        .get(id)
        .and_then(|s| s.parent_id.as_deref())
        .or_else(|| composites_by_id.get(id).and_then(|c| c.parent_id.as_deref()));
    while let Some(parent) = parent_id {
        ids.push(parent);
        parent_id = composites_by_id.get(parent).and_then(|c| c.parent_id.as_deref());
    }
    ids
}

/// `innermostCommonComposite`.
fn innermost_common_composite(diagram: &StateDiagram, transition_index: usize) -> Option<String> {
    let transition = &diagram.transitions[transition_index];
    let target: HashSet<&str> =
        containing_composite_ids(diagram, &transition.to).into_iter().collect();
    containing_composite_ids(diagram, &transition.from)
        .into_iter()
        .find(|id| target.contains(id))
        .map(str::to_string)
}

#[derive(Debug, Clone)]
pub enum RouteKind {
    SelfLoop { lane: i64 },
    HorizontalForward { left_to_right: bool },
    BottomFeedback { rail_y: i64, approach_x: i64 },
    TopFeedback { rail_y: i64 },
    BottomParallel { rail_y: i64, approach_x: i64 },
    VerticalElbow { has_reverse: bool, offset_connector: bool },
    SideParallel { rail_x: i64, target_approach: Option<TargetApproach> },
    Vertical,
}

/// `StateTransitionRoutePlan`.
#[derive(Debug, Clone)]
pub struct StateTransitionRoutePlan {
    pub transition_index: usize,
    /// Arena slots — the boxes mutate in place after planning, exactly
    /// like the upstream object references.
    pub from: usize,
    pub to: usize,
    pub target_is_choice: bool,
    pub target_is_hidden_marker: bool,
    pub kind: RouteKind,
}

/// `StateTransitionRenderCell` — a glyph or an arrow head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateTransitionRenderCell {
    pub x: i64,
    pub y: i64,
    pub payload: RenderCellPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderCellPayload {
    Char(&'static str),
    Arrow(Direction),
}

/// `StateTransitionRenderLabel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTransitionRenderLabel {
    pub x: i64,
    pub y: i64,
    pub lines: Vec<String>,
}

/// `StateTransitionRenderPlan`.
#[derive(Debug, Clone)]
pub struct StateTransitionRenderPlan {
    pub route: StateTransitionRoutePlan,
    pub cells: Vec<StateTransitionRenderCell>,
    pub path: Vec<(i64, i64)>,
    pub label: Option<StateTransitionRenderLabel>,
    pub path_repaired: bool,
}

/// `StateTransitionRenderOptions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StateTransitionRenderOptions<'a> {
    pub feedback_top_y: Option<i64>,
    pub note_bounds: &'a [StateDiagramNoteBounds],
    pub repair_routes: Option<bool>,
}

/// `createStateTransitionRoutePlans`.
fn create_state_transition_route_plans(
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    feedback_lane_y: i64,
    feedback_top_y: Option<i64>,
) -> Vec<StateTransitionRoutePlan> {
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let mut endpoint_occurrences: HashMap<String, usize> = HashMap::new();
    let mut self_occurrences: HashMap<String, i64> = HashMap::new();
    let parallel_lane_gap = 3.max(
        diagram
            .transitions
            .iter()
            .map(|transition| measure_state_transition_label(&transition.label).height + 2)
            .max()
            .unwrap_or(0),
    );
    let mut next_side_rail_x =
        layout.boxes.iter().map(|bound| bound.left + bound.width).max().unwrap_or(0).max(0) + 3;
    let mut side_rail_lanes: HashMap<String, i64> = HashMap::new();
    let feedback_allocations = create_feedback_allocations(
        diagram,
        layout,
        feedback_lane_y,
        parallel_lane_gap,
        feedback_top_y,
    );
    let max_bottom_rail = feedback_allocations
        .values()
        .filter(|allocation| allocation.side == FeedbackSide::Bottom)
        .map(|allocation| allocation.rail_y)
        .max()
        .unwrap_or(i64::MIN);
    let mut next_bottom_rail_y =
        (feedback_lane_y - parallel_lane_gap).max(max_bottom_rail) + parallel_lane_gap;

    let mut allocate_side_rail = |transition_index: usize| -> i64 {
        let composite_id = innermost_common_composite(diagram, transition_index);
        if let Some(composite_slot) =
            composite_id.as_deref().and_then(|id| layout.composite_bounds.get(id))
        {
            let composite = &layout.boxes[*composite_slot];
            let lane: i64 = side_rail_lanes.get(composite.id.as_str()).copied().unwrap_or(0);
            side_rail_lanes.insert(composite.id.clone(), lane + 1);
            let descendant_right = (composite.left + 1).max(
                diagram
                    .states
                    .iter()
                    .filter_map(|state| {
                        if !containing_composite_ids(diagram, &state.id)
                            .contains(&composite.id.as_str())
                        {
                            return None;
                        }
                        layout
                            .bounds
                            .get(state.id.as_str())
                            .map(|&index| layout.boxes[index].left + layout.boxes[index].width)
                    })
                    .max()
                    .unwrap_or(composite.left + 1),
            );
            return (descendant_right + 2 + lane * 3).min(composite.left + composite.width - 2);
        }
        let rail_x = next_side_rail_x;
        next_side_rail_x += 3;
        rail_x
    };
    let mut allocate_bottom_rail = || -> i64 {
        let rail_y = next_bottom_rail_y;
        next_bottom_rail_y += parallel_lane_gap;
        rail_y
    };

    let mut plans: Vec<StateTransitionRoutePlan> = Vec::new();
    for transition_index in 0..diagram.transitions.len() {
        let transition = &diagram.transitions[transition_index];
        let Some(&from_slot) = layout.bounds.get(&transition.from) else { continue };
        let Some(&to_slot) = layout.bounds.get(&transition.to) else { continue };
        let from = &layout.boxes[from_slot];
        let to = &layout.boxes[to_slot];

        let target_state = states_by_id.get(transition.to.as_str());
        let target_is_choice = target_state.is_some_and(|state| state.kind == StateKind::Choice);
        let target_is_hidden_marker = is_hidden_composite_marker(target_state.copied());
        let base = StateTransitionRoutePlan {
            transition_index,
            from: from_slot,
            to: to_slot,
            target_is_choice,
            target_is_hidden_marker,
            kind: RouteKind::Vertical,
        };
        let side_parallel = |base: StateTransitionRoutePlan,
                             allocate_side_rail: &mut dyn FnMut(usize) -> i64|
         -> StateTransitionRoutePlan {
            let rail_x = allocate_side_rail(base.transition_index);
            StateTransitionRoutePlan {
                kind: RouteKind::SideParallel {
                    rail_x,
                    target_approach: side_parallel_target_approach(
                        diagram,
                        base.transition_index,
                        &layout.boxes[base.from],
                        &layout.boxes[base.to],
                        layout,
                        rail_x,
                    ),
                },
                ..base
            }
        };
        if transition.from == transition.to {
            let lane: i64 = self_occurrences.get(&transition.from).copied().unwrap_or(0);
            self_occurrences.insert(transition.from.clone(), lane + 1);
            plans.push(StateTransitionRoutePlan { kind: RouteKind::SelfLoop { lane }, ..base });
            continue;
        }
        let endpoint_key = format!("{}\0{}", transition.from, transition.to);
        let parallel_index = endpoint_occurrences.get(&endpoint_key).copied().unwrap_or(0);
        endpoint_occurrences.insert(endpoint_key, parallel_index + 1);
        let feedback = (diagram.direction == StateDirection::Lr
            || diagram.direction == StateDirection::Rl)
            && is_state_horizontal_feedback(diagram, from, to);
        if let Some(allocation) = feedback_allocations.get(&transition_index) {
            if allocation.side == FeedbackSide::Bottom {
                plans.push(StateTransitionRoutePlan {
                    kind: RouteKind::BottomFeedback {
                        rail_y: allocation.rail_y,
                        approach_x: bottom_approach_x(
                            diagram,
                            transition_index,
                            from,
                            to,
                            layout,
                            allocation.rail_y,
                        ),
                    },
                    ..base
                });
            } else {
                plans.push(StateTransitionRoutePlan {
                    kind: RouteKind::TopFeedback { rail_y: allocation.rail_y },
                    ..base
                });
            }
            continue;
        }
        if parallel_index > 0 {
            if from.center_y == to.center_y {
                let rail_y = allocate_bottom_rail();
                plans.push(StateTransitionRoutePlan {
                    kind: RouteKind::BottomParallel {
                        rail_y,
                        approach_x: bottom_approach_x(
                            diagram,
                            transition_index,
                            from,
                            to,
                            layout,
                            rail_y,
                        ),
                    },
                    ..base
                });
            } else {
                plans.push(side_parallel(base, &mut allocate_side_rail));
            }
            continue;
        }
        if diagram.direction != StateDirection::Lr && diagram.direction != StateDirection::Rl {
            let from_parent = states_by_id
                .get(transition.from.as_str())
                .and_then(|state| state.parent_id.as_deref());
            let to_parent = states_by_id
                .get(transition.to.as_str())
                .and_then(|state| state.parent_id.as_deref());
            if from_parent.is_some() && to_parent.is_some() && from_parent != to_parent {
                plans.push(side_parallel(base, &mut allocate_side_rail));
                continue;
            }
            if vertical_corridor_crosses_unrelated_state(
                diagram,
                transition_index,
                from,
                to,
                layout,
            ) {
                plans.push(side_parallel(base, &mut allocate_side_rail));
                continue;
            }
            let vertical_feedback = if diagram.direction == StateDirection::Bt {
                from.center_y < to.center_y
            } else {
                from.center_y > to.center_y
            };
            if vertical_feedback {
                plans.push(side_parallel(base, &mut allocate_side_rail));
                continue;
            }
            if from.center_y == to.center_y {
                if has_reverse_transition(diagram, transition) && from.center_x > to.center_x {
                    let rail_y = allocate_bottom_rail();
                    plans.push(StateTransitionRoutePlan {
                        kind: RouteKind::BottomParallel {
                            rail_y,
                            approach_x: bottom_approach_x(
                                diagram,
                                transition_index,
                                from,
                                to,
                                layout,
                                rail_y,
                            ),
                        },
                        ..base
                    });
                } else {
                    plans.push(StateTransitionRoutePlan {
                        kind: RouteKind::HorizontalForward {
                            left_to_right: from.center_x <= to.center_x,
                        },
                        ..base
                    });
                }
                continue;
            }
            if !has_vertical_corridor(from, to) {
                plans.push(side_parallel(base, &mut allocate_side_rail));
                continue;
            }
            if from.center_x == to.center_x {
                plans.push(base);
            } else {
                plans.push(StateTransitionRoutePlan {
                    kind: RouteKind::VerticalElbow { has_reverse: false, offset_connector: false },
                    ..base
                });
            }
            continue;
        }

        if from.center_y != to.center_y {
            if !has_vertical_corridor(from, to) {
                plans.push(side_parallel(base, &mut allocate_side_rail));
                continue;
            }
            if from.center_y > to.center_y && feedback {
                plans.push(StateTransitionRoutePlan {
                    kind: RouteKind::BottomFeedback {
                        rail_y: feedback_lane_y,
                        approach_x: bottom_approach_x(
                            diagram,
                            transition_index,
                            from,
                            to,
                            layout,
                            feedback_lane_y,
                        ),
                    },
                    ..base
                });
                continue;
            }
            let has_reverse = has_reverse_transition(diagram, transition);
            plans.push(StateTransitionRoutePlan {
                kind: RouteKind::VerticalElbow {
                    has_reverse,
                    offset_connector: has_reverse
                        || has_opposing_top_connector(diagram, transition_index, layout),
                },
                ..base
            });
            continue;
        }
        if feedback {
            plans.push(StateTransitionRoutePlan {
                kind: RouteKind::BottomFeedback {
                    rail_y: feedback_lane_y,
                    approach_x: bottom_approach_x(
                        diagram,
                        transition_index,
                        from,
                        to,
                        layout,
                        feedback_lane_y,
                    ),
                },
                ..base
            });
            continue;
        }
        if horizontal_corridor_crosses_unrelated_state(diagram, transition_index, from, to, layout)
        {
            let rail_y = allocate_bottom_rail();
            plans.push(StateTransitionRoutePlan {
                kind: RouteKind::BottomParallel {
                    rail_y,
                    approach_x: bottom_approach_x(
                        diagram,
                        transition_index,
                        from,
                        to,
                        layout,
                        rail_y,
                    ),
                },
                ..base
            });
            continue;
        }
        plans.push(StateTransitionRoutePlan {
            kind: RouteKind::HorizontalForward { left_to_right: from.center_x <= to.center_x },
            ..base
        });
    }
    plans
}

/// `StateTransitionRenderBuilder`.
struct RenderBuilder<'a> {
    diagram: &'a StateDiagram,
    layout: &'a StateDiagramLayout,
    route: StateTransitionRoutePlan,
    cells: Vec<StateTransitionRenderCell>,
    path: Vec<(i64, i64)>,
    label: Option<StateTransitionRenderLabel>,
}

impl RenderBuilder<'_> {
    fn from(&self) -> &StateDiagramBoxBounds {
        &self.layout.boxes[self.route.from]
    }

    fn to(&self) -> &StateDiagramBoxBounds {
        &self.layout.boxes[self.route.to]
    }

    fn transition(&self) -> &StateDiagramTransition {
        &self.diagram.transitions[self.route.transition_index]
    }

    fn add_cell(&mut self, x: i64, y: i64, payload: RenderCellPayload) {
        self.cells.push(StateTransitionRenderCell { x, y, payload });
        self.path.push((x, y));
    }

    fn add_char(&mut self, x: i64, y: i64, char: &'static str) {
        self.add_cell(x, y, RenderCellPayload::Char(char));
    }

    fn add_arrow(&mut self, x: i64, y: i64, direction: Direction) {
        self.add_cell(x, y, RenderCellPayload::Arrow(direction));
    }

    fn add_path_point(&mut self, x: i64, y: i64) {
        self.path.push((x, y));
    }

    fn add_label(&mut self, x: i64, y: i64, label: &str) {
        let metrics = measure_state_transition_label(label);
        if !metrics.lines.is_empty() {
            self.label = Some(StateTransitionRenderLabel { x, y, lines: metrics.lines });
        }
    }

    fn add_horizontal_line(&mut self, from_x: i64, to_x: i64, y: i64, direction: i64) {
        let mut x = from_x;
        while if direction == 1 { x <= to_x } else { x >= to_x } {
            self.add_char(x, y, "─");
            x += direction;
        }
    }

    fn add_vertical_line(&mut self, x: i64, from_y: i64, to_y: i64, direction: i64) {
        let mut y = from_y;
        while if direction == 1 { y <= to_y } else { y >= to_y } {
            self.add_char(x, y, "│");
            y += direction;
        }
    }

    fn add_right_departure(&mut self, bounds: &StateDiagramBoxBounds) {
        if bounds.width <= 1 || bounds.height <= 1 {
            return;
        }
        self.add_char(bounds.left + bounds.width - 1, bounds.center_y, BORDER_ROUNDED.left_t);
    }

    fn add_left_departure(&mut self, bounds: &StateDiagramBoxBounds) {
        if bounds.width <= 1 || bounds.height <= 1 {
            return;
        }
        self.add_char(bounds.left, bounds.center_y, BORDER_ROUNDED.right_t);
    }

    fn add_bottom_departure(&mut self, bounds: &StateDiagramBoxBounds, x: i64) {
        if bounds.width <= 1 || bounds.height <= 1 {
            return;
        }
        self.add_char(x, bounds.top + bounds.height - 1, BORDER_ROUNDED.top_t);
    }

    fn add_top_departure(&mut self, bounds: &StateDiagramBoxBounds, x: i64) {
        if bounds.width <= 1 || bounds.height <= 1 {
            return;
        }
        self.add_char(x, bounds.top, BORDER_ROUNDED.bottom_t);
    }
}

/// `innerConnectorX`.
fn inner_connector_x(bounds: &StateDiagramBoxBounds, preferred_x: i64) -> i64 {
    if bounds.width <= 2 {
        return bounds.center_x;
    }
    (bounds.left + 1).max((bounds.left + bounds.width - 2).min(preferred_x))
}

/// `addHorizontalForward`.
fn add_horizontal_forward(builder: &mut RenderBuilder<'_>, left_to_right: bool) {
    let from = builder.from().clone();
    let to = builder.to().clone();
    let target_is_choice = builder.route.target_is_choice;
    let target_is_hidden_marker = builder.route.target_is_hidden_marker;
    let label = builder.transition().label.clone();
    let y = from.center_y;
    if left_to_right {
        builder.add_right_departure(&from);
    } else {
        builder.add_left_departure(&from);
    }
    let step: i64 = if left_to_right { 1 } else { -1 };
    let start_x = if left_to_right { from.left + from.width } else { from.left - 1 };
    let end_x = if left_to_right { to.left - 1 } else { to.left + to.width };
    builder.add_horizontal_line(start_x, end_x - step, y, step);
    if target_is_hidden_marker {
        builder.add_char(end_x, y, "─");
    } else {
        builder.add_arrow(end_x, y, if left_to_right { Direction::Right } else { Direction::Left });
    }
    if target_is_choice || target_is_hidden_marker {
        builder.add_path_point(to.left, y);
    }
    if label.is_empty() {
        return;
    }
    let metrics = measure_state_transition_label(&label);
    let label_x =
        start_x.min(end_x) + 1.max(((end_x - start_x).abs() - metrics.width).div_euclid(2));
    builder.add_label(label_x, 0.max(y - metrics.height), &label);
}

/// `addSelfTransition`.
fn add_self_transition(builder: &mut RenderBuilder<'_>, lane: i64) {
    let bounds = builder.from().clone();
    let label = builder.transition().label.clone();
    if bounds.width <= 1 || bounds.height <= 1 {
        let rail_x = bounds.left + 4 + lane * 4;
        let rail_y = bounds.top + 2 + lane * 2;
        builder.add_horizontal_line(bounds.left + 1, rail_x - 1, bounds.top, 1);
        builder.add_char(rail_x, bounds.top, "╮");
        builder.add_vertical_line(rail_x, bounds.top + 1, rail_y - 1, 1);
        builder.add_char(rail_x, rail_y, "╯");
        builder.add_horizontal_line(rail_x - 1, bounds.left + 1, rail_y, -1);
        builder.add_char(bounds.left, rail_y, "╰");
        let mut y = rail_y - 1;
        while y > bounds.top + 1 {
            builder.add_char(bounds.left, y, "│");
            y -= 1;
        }
        builder.add_arrow(bounds.left, bounds.top + 1, Direction::Up);
        builder.add_path_point(bounds.left, bounds.top);
        if !label.is_empty() {
            builder.add_label(
                rail_x + 2,
                if lane == 0 { bounds.top + 1 } else { rail_y - 1 },
                &label,
            );
        }
        return;
    }
    let source_x = bounds.left + 2.max(bounds.width.div_euclid(3));
    let bottom_y = bounds.top + bounds.height - 1;
    let rail_y = bottom_y + 2 + lane * 3;
    let target_x = (source_x + 3)
        .max(bounds.left + (bounds.width - 3).min((bounds.width * 2 + 2).div_euclid(3)))
        + lane * 4;

    builder.add_bottom_departure(&bounds, source_x);
    builder.add_vertical_line(source_x, bottom_y + 1, rail_y - 1, 1);
    builder.add_char(source_x, rail_y, "╰");
    for x in (source_x + 1)..target_x {
        builder.add_char(x, rail_y, "─");
    }
    builder.add_char(target_x, rail_y, "╯");
    let mut y = rail_y - 1;
    while y > bottom_y + 1 {
        builder.add_char(target_x, y, "│");
        y -= 1;
    }
    builder.add_arrow(target_x, bottom_y + 1, Direction::Up);
    if !label.is_empty() {
        builder.add_label(target_x + 2, if lane == 0 { bottom_y + 1 } else { rail_y - 1 }, &label);
    }
}

fn outside_bottom_y(bounds: &StateDiagramBoxBounds) -> i64 {
    bounds.top + bounds.height
}

fn outside_top_y(bounds: &StateDiagramBoxBounds) -> i64 {
    bounds.top - 1
}

/// `addBottomLaneTransition` (bottom-feedback + bottom-parallel).
fn add_bottom_lane_transition(builder: &mut RenderBuilder<'_>, rail_y: i64, approach_x: i64) {
    let from = builder.from().clone();
    let to = builder.to().clone();
    let target_is_choice = builder.route.target_is_choice;
    let target_is_hidden_marker = builder.route.target_is_hidden_marker;
    let label = builder.transition().label.clone();
    let source_x = from.center_x;
    let target_x = if to.width > 1 {
        if source_x > to.center_x {
            to.left + 1
        } else {
            to.left + to.width - 2
        }
    } else {
        to.center_x
    };
    let target_rail_cuts_source = target_x >= from.left && target_x < from.left + from.width;
    let rail_target_x = if target_rail_cuts_source {
        (from.left + from.width).max(to.left + to.width) + 2
    } else {
        approach_x
    };
    let source_bottom_y = outside_bottom_y(&from);
    let target_bottom_y = outside_bottom_y(&to);
    builder.add_bottom_departure(&from, source_x);
    builder.add_vertical_line(source_x, source_bottom_y, rail_y - 1, 1);
    builder.add_char(source_x, rail_y, if source_x > rail_target_x { "╯" } else { "╰" });
    if source_x != rail_target_x {
        let horizontal_step: i64 = if source_x < rail_target_x { 1 } else { -1 };
        let mut x = source_x + horizontal_step;
        while x != rail_target_x {
            builder.add_char(x, rail_y, "─");
            x += horizontal_step;
        }
    }
    builder.add_char(rail_target_x, rail_y, if source_x > rail_target_x { "╰" } else { "╯" });
    let mut y = rail_y - 1;
    while y > target_bottom_y {
        builder.add_char(rail_target_x, y, "│");
        y -= 1;
    }
    if rail_target_x != target_x {
        builder.add_char(
            rail_target_x,
            target_bottom_y,
            if rail_target_x < target_x { "╭" } else { "╮" },
        );
        let horizontal_step: i64 = if rail_target_x < target_x { 1 } else { -1 };
        let mut x = rail_target_x + horizontal_step;
        while x != target_x {
            builder.add_char(x, target_bottom_y, "─");
            x += horizontal_step;
        }
    }
    if target_is_hidden_marker {
        builder.add_char(target_x, target_bottom_y, "│");
    } else {
        builder.add_arrow(target_x, target_bottom_y, Direction::Up);
    }
    if target_is_choice || target_is_hidden_marker {
        builder.add_path_point(to.left, to.top);
    }
    if label.is_empty() {
        return;
    }
    let metrics = measure_state_transition_label(&label);
    let horizontal_room = (source_x - rail_target_x).abs() - 2;
    let label_x = if metrics.width <= horizontal_room {
        source_x.min(rail_target_x)
            + 1.max(((source_x - rail_target_x).abs() - metrics.width).div_euclid(2))
    } else {
        rail_target_x + 2
    };
    builder.add_label(label_x, 0.max(rail_y - metrics.height), &label);
}

/// `addTopFeedbackTransition`.
fn add_top_feedback_transition(builder: &mut RenderBuilder<'_>, rail_y: i64) {
    let from = builder.from().clone();
    let to = builder.to().clone();
    let target_is_choice = builder.route.target_is_choice;
    let target_is_hidden_marker = builder.route.target_is_hidden_marker;
    let label = builder.transition().label.clone();
    let source_x = from.center_x;
    let target_x = if to.width > 1 {
        if source_x > to.center_x {
            to.left + to.width - 2
        } else {
            to.left + 1
        }
    } else {
        to.center_x
    };
    let source_top_y = outside_top_y(&from);
    let target_top_y = outside_top_y(&to);
    builder.add_top_departure(&from, source_x);
    builder.add_vertical_line(source_x, source_top_y, rail_y + 1, -1);
    builder.add_char(source_x, rail_y, if source_x > target_x { "╮" } else { "╭" });
    if source_x != target_x {
        let horizontal_step: i64 = if source_x < target_x { 1 } else { -1 };
        let mut x = source_x + horizontal_step;
        while x != target_x {
            builder.add_char(x, rail_y, "─");
            x += horizontal_step;
        }
    }
    builder.add_char(target_x, rail_y, if source_x > target_x { "╭" } else { "╮" });
    for y in (rail_y + 1)..target_top_y {
        builder.add_char(target_x, y, "│");
    }
    if target_is_hidden_marker {
        builder.add_char(target_x, target_top_y, "│");
    } else {
        builder.add_arrow(target_x, target_top_y, Direction::Down);
    }
    if target_is_choice || target_is_hidden_marker {
        builder.add_path_point(to.left, to.top);
    }
    if label.is_empty() {
        return;
    }
    let metrics = measure_state_transition_label(&label);
    let horizontal_room = (source_x - target_x).abs() - 2;
    let label_x = if metrics.width <= horizontal_room {
        source_x.min(target_x) + 1.max(((source_x - target_x).abs() - metrics.width).div_euclid(2))
    } else {
        target_x + 2
    };
    builder.add_label(label_x, rail_y - metrics.height, &label);
}

/// `addSideParallelTransition`.
fn add_side_parallel_transition(
    builder: &mut RenderBuilder<'_>,
    rail_x: i64,
    target_approach: Option<TargetApproach>,
) {
    let from = builder.from().clone();
    let to = builder.to().clone();
    let target_is_choice = builder.route.target_is_choice;
    let target_is_hidden_marker = builder.route.target_is_hidden_marker;
    let label = builder.transition().label.clone();
    let start_x = from.left + from.width;
    let start_y = from.center_y;
    let end_y = match target_approach {
        Some(TargetApproach::Top) => to.top - 2,
        Some(TargetApproach::Bottom) => to.top + to.height + 1,
        None => to.center_y,
    };
    let vertical_step: i64 = if start_y <= end_y { 1 } else { -1 };
    builder.add_right_departure(&from);
    builder.add_horizontal_line(start_x, rail_x - 1, start_y, 1);
    builder.add_char(rail_x, start_y, if vertical_step == 1 { "╮" } else { "╯" });
    let mut y = start_y + vertical_step;
    while y != end_y {
        builder.add_char(rail_x, y, "│");
        y += vertical_step;
    }
    builder.add_char(rail_x, end_y, if vertical_step == 1 { "╯" } else { "╮" });
    if let Some(approach) = target_approach {
        let target_x = inner_connector_x(&to, from.center_x);
        let mut x = rail_x - 1;
        while x > target_x {
            builder.add_char(x, end_y, "─");
            x -= 1;
        }
        builder.add_char(target_x, end_y, if approach == TargetApproach::Top { "╭" } else { "╰" });
        builder.add_arrow(
            target_x,
            if approach == TargetApproach::Top { end_y + 1 } else { end_y - 1 },
            if approach == TargetApproach::Top { Direction::Down } else { Direction::Up },
        );
        if target_is_choice || target_is_hidden_marker {
            builder.add_path_point(to.left, to.top);
        }
        if !label.is_empty() {
            let metrics = measure_state_transition_label(&label);
            builder.add_label(
                rail_x + 2,
                0.max((start_y + to.center_y - metrics.height + 1).div_euclid(2)),
                &label,
            );
        }
        return;
    }
    let end_x = to.left + to.width;
    let mut x = rail_x - 1;
    while x > end_x {
        builder.add_char(x, end_y, "─");
        x -= 1;
    }
    if target_is_hidden_marker {
        builder.add_char(end_x, end_y, "─");
    } else {
        builder.add_arrow(end_x, end_y, Direction::Left);
    }
    if target_is_choice || target_is_hidden_marker {
        builder.add_path_point(to.left, to.top);
    }
    if !label.is_empty() {
        let metrics = measure_state_transition_label(&label);
        let label_y = 0.max((start_y + end_y - metrics.height + 1).div_euclid(2));
        builder.add_label(rail_x + 2, label_y, &label);
    }
}

/// `addVerticalElbowTransition`.
fn add_vertical_elbow_transition(
    builder: &mut RenderBuilder<'_>,
    has_reverse: bool,
    offset_connector: bool,
) {
    let from = builder.from().clone();
    let to = builder.to().clone();
    let target_is_choice = builder.route.target_is_choice;
    let target_is_hidden_marker = builder.route.target_is_hidden_marker;
    let label = builder.transition().label.clone();
    let top_to_bottom = from.center_y < to.center_y;
    let offset = if offset_connector {
        if top_to_bottom {
            -2
        } else {
            2
        }
    } else {
        0
    };
    let start_x = inner_connector_x(&from, from.center_x + offset);
    let end_x = inner_connector_x(&to, to.center_x + offset);
    let start_y = if top_to_bottom { from.top + from.height } else { from.top - 1 };
    let end_y = if top_to_bottom { to.top - 1 } else { to.top + to.height };
    let vertical_step: i64 = if top_to_bottom { 1 } else { -1 };
    if top_to_bottom {
        builder.add_bottom_departure(&from, start_x);
    } else {
        builder.add_top_departure(&from, start_x);
    }
    let available_approach = 0.max((end_y - start_y).abs() - 1);
    let bend_y = if start_x == end_x {
        end_y
    } else if top_to_bottom {
        end_y - vertical_step * 2.min(available_approach)
    } else {
        start_y
    };
    let target_approach_length = (end_y - bend_y).abs();
    let has_target_approach = target_approach_length > 0;
    if start_y != bend_y {
        builder.add_vertical_line(start_x, start_y, bend_y - vertical_step, vertical_step);
    }
    if start_x != end_x {
        let horizontal_step: i64 = if start_x < end_x { 1 } else { -1 };
        builder.add_char(
            start_x,
            bend_y,
            match (top_to_bottom, start_x < end_x) {
                (true, true) => "╰",
                (true, false) => "╯",
                (false, true) => "╭",
                (false, false) => "╮",
            },
        );
        let mut x = start_x + horizontal_step;
        while x != end_x {
            builder.add_char(x, bend_y, "─");
            x += horizontal_step;
        }
        if has_target_approach {
            builder.add_char(
                end_x,
                bend_y,
                match (top_to_bottom, start_x < end_x) {
                    (true, true) => "╮",
                    (true, false) => "╭",
                    (false, true) => "╯",
                    (false, false) => "╰",
                },
            );
            for distance in 1..target_approach_length {
                builder.add_char(end_x, bend_y + vertical_step * distance, "│");
            }
        }
    }
    let target_char: Option<&'static str> = if target_is_hidden_marker {
        if has_target_approach || start_x == end_x {
            Some("│")
        } else if top_to_bottom {
            Some("┬")
        } else {
            Some("┴")
        }
    } else {
        None
    };
    match target_char {
        Some(char) => builder.add_char(end_x, end_y, char),
        None => builder.add_arrow(
            end_x,
            end_y,
            if top_to_bottom { Direction::Down } else { Direction::Up },
        ),
    }
    if target_is_choice || target_is_hidden_marker {
        builder.add_path_point(to.left, to.top);
    }
    if label.is_empty() {
        return;
    }
    let metrics = measure_state_transition_label(&label);
    if top_to_bottom {
        let left_label_x = start_x - metrics.width - 2;
        let label_x = if from.width == 1 && (end_x - start_x).abs() >= metrics.width + 2 {
            start_x.min(end_x) + ((end_x - start_x).abs() - metrics.width).div_euclid(2)
        } else if has_reverse || end_x < start_x {
            if left_label_x >= 0 {
                left_label_x
            } else {
                start_x + 4
            }
        } else {
            start_x + 2
        };
        builder.add_label(
            label_x,
            if has_target_approach {
                0.max(bend_y - metrics.height)
            } else {
                (start_y + 1).min(end_y)
            },
            &label,
        );
    } else {
        let label_x =
            start_x.min(end_x) + 1.max(((end_x - start_x).abs() - metrics.width).div_euclid(2));
        builder.add_label(
            if start_x == end_x { start_x + 3 } else { label_x },
            if has_target_approach { 0.max(bend_y - metrics.height) } else { 0.max(start_y) },
            &label,
        );
    }
}

/// `addVerticalTransition`.
fn add_vertical_transition(builder: &mut RenderBuilder<'_>) {
    let from = builder.from().clone();
    let to = builder.to().clone();
    let target_is_choice = builder.route.target_is_choice;
    let target_is_hidden_marker = builder.route.target_is_hidden_marker;
    let label = builder.transition().label.clone();
    let top_to_bottom = from.center_y <= to.center_y;
    let x = from.center_x;
    let start_y = if top_to_bottom { from.top + from.height } else { from.top - 1 };
    let end_y = if top_to_bottom { to.top - 1 } else { to.top + to.height };
    let step: i64 = if top_to_bottom { 1 } else { -1 };
    if top_to_bottom {
        builder.add_bottom_departure(&from, x);
    } else {
        builder.add_top_departure(&from, x);
    }
    if start_y != end_y {
        builder.add_vertical_line(x, start_y, end_y - step, step);
    }
    if target_is_hidden_marker {
        builder.add_char(x, end_y, "│");
    } else {
        builder.add_arrow(x, end_y, if top_to_bottom { Direction::Down } else { Direction::Up });
    }
    if target_is_choice || target_is_hidden_marker {
        builder.add_path_point(to.left, to.top);
    }
    if !label.is_empty() {
        builder.add_label(x + 2, start_y.min(end_y) + 1, &label);
    }
}

/// `createStateTransitionRenderPlan`.
fn create_state_transition_render_plan(
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    route: StateTransitionRoutePlan,
) -> StateTransitionRenderPlan {
    let mut builder = RenderBuilder {
        diagram,
        layout,
        route: route.clone(),
        cells: Vec::new(),
        path: Vec::new(),
        label: None,
    };
    match route.kind {
        RouteKind::SelfLoop { lane } => add_self_transition(&mut builder, lane),
        RouteKind::HorizontalForward { left_to_right } => {
            add_horizontal_forward(&mut builder, left_to_right);
        }
        RouteKind::BottomFeedback { rail_y, approach_x }
        | RouteKind::BottomParallel { rail_y, approach_x } => {
            add_bottom_lane_transition(&mut builder, rail_y, approach_x);
        }
        RouteKind::TopFeedback { rail_y } => add_top_feedback_transition(&mut builder, rail_y),
        RouteKind::VerticalElbow { has_reverse, offset_connector } => {
            add_vertical_elbow_transition(&mut builder, has_reverse, offset_connector);
        }
        RouteKind::Vertical => add_vertical_transition(&mut builder),
        RouteKind::SideParallel { rail_x, target_approach } => {
            add_side_parallel_transition(&mut builder, rail_x, target_approach);
        }
    }
    StateTransitionRenderPlan {
        route,
        cells: builder.cells,
        path: builder.path,
        label: builder.label,
        path_repaired: false,
    }
}

fn point_is_inside_bounds(point: (i64, i64), bounds: &StateDiagramBoxBounds) -> bool {
    point.0 >= bounds.left
        && point.0 < bounds.left + bounds.width
        && point.1 >= bounds.top
        && point.1 < bounds.top + bounds.height
}

/// `routeIntersectsUnrelatedState`.
fn route_intersects_unrelated_state(
    plan: &StateTransitionRenderPlan,
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    note_bounds: &[StateDiagramNoteBounds],
) -> bool {
    let transition = &diagram.transitions[plan.route.transition_index];
    if diagram.states.iter().any(|state| {
        if state.id == transition.from || state.id == transition.to {
            return false;
        }
        if is_hidden_composite_marker(Some(state)) {
            return false;
        }
        let Some(&bound_index) = layout.bounds.get(state.id.as_str()) else { return false };
        plan.path.iter().any(|point| point_is_inside_bounds(*point, &layout.boxes[bound_index]))
    }) {
        return true;
    }

    note_bounds.iter().any(|note_bound| {
        if plan.path.iter().any(|point| point_is_inside_bounds(*point, &note_bound.bound)) {
            return true;
        }
        let Some(&target_index) = layout.bounds.get(note_bound.note.target.as_str()) else {
            return false;
        };
        let connector = placed_note_connector(note_bound, &layout.boxes[target_index]);
        plan.path.iter().any(|(x, y)| {
            connector
                .spans()
                .iter()
                .any(|span| span.y == *y && *x >= span.from_x && *x <= span.to_x)
        })
    })
}

impl crate::render::mermaid::state::note::StateDiagramNoteConnector {
    fn spans(&self) -> Vec<crate::render::mermaid::core::spatial::SpatialSpan> {
        crate::render::mermaid::core::spatial::spatial_path_spans(&self.points)
    }
}

/// `findBodySafePath`.
#[allow(clippy::type_complexity)]
fn find_body_safe_path(
    start: (i64, i64),
    end: (i64, i64),
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    plan: &StateTransitionRenderPlan,
    search: &StateSearchSpace,
    budget: &mut StateSearchBudget,
) -> Option<Vec<(i64, i64)>> {
    let margin = 8.max(layout.bounds.len() as i64 * 2);
    let composite_id = innermost_common_composite(diagram, plan.route.transition_index);
    let composite = composite_id
        .as_deref()
        .and_then(|id| layout.composite_bounds.get(id))
        .map(|&index| &layout.boxes[index]);
    let path_min = plan.path.iter().map(|point| point.0).min().unwrap_or(0);
    let path_max = plan.path.iter().map(|point| point.0).max().unwrap_or(0);
    let path_y_min = plan.path.iter().map(|point| point.1).min().unwrap_or(0);
    let path_y_max = plan.path.iter().map(|point| point.1).max().unwrap_or(0);
    let search_bounds = StateSearchBounds {
        min_x: composite.map_or(search.min_x - margin, |composite| composite.left + 1),
        min_y: composite
            .map_or(search.min_y.min(path_y_min) - margin, |composite| composite.top + 1),
        max_x: composite.map_or(search.max_x.max(path_max) + margin, |composite| {
            composite.left + composite.width - 2
        }),
        max_y: composite.map_or(search.max_y.max(path_y_max) + margin, |composite| {
            composite.top + composite.height - 2
        }),
    };
    let is_free = |point: (i64, i64)| -> bool {
        point.0 >= search_bounds.min_x
            && point.0 <= search_bounds.max_x
            && point.1 >= search_bounds.min_y
            && point.1 <= search_bounds.max_y
            && !search.blocked.contains(&(point.0, point.1))
    };
    let path_min_x = path_min;
    let path_min_y = path_y_min;
    let path_max_x = path_max;
    let path_max_y = path_y_max;
    let mut direct_candidates: Vec<Vec<(i64, i64)>> =
        vec![vec![start, (start.0, end.1), end], vec![start, (end.0, start.1), end]];
    for offset in 1..=4i64 {
        direct_candidates.push(vec![
            start,
            (start.0, path_min_y - offset),
            (end.0, path_min_y - offset),
            end,
        ]);
        direct_candidates.push(vec![
            start,
            (start.0, path_max_y + offset),
            (end.0, path_max_y + offset),
            end,
        ]);
        direct_candidates.push(vec![
            start,
            (path_min_x - offset, start.1),
            (path_min_x - offset, end.1),
            end,
        ]);
        direct_candidates.push(vec![
            start,
            (path_max_x + offset, start.1),
            (path_max_x + offset, end.1),
            end,
        ]);
    }
    let expanded: Vec<Vec<Point>> = direct_candidates
        .iter()
        .map(|points| {
            orthogonal_path_points(&points.iter().map(|&(x, y)| Point { x, y }).collect::<Vec<_>>())
        })
        .collect();
    let mut direct: Option<&Vec<Point>> = None;
    for candidate in &expanded {
        if !candidate.iter().copied().all(|point| is_free((point.x, point.y))) {
            continue;
        }
        match direct {
            None => direct = Some(candidate),
            Some(current) if candidate.len() < current.len() => direct = Some(candidate),
            Some(_) => {}
        }
    }
    if let Some(direct) = direct {
        return Some(direct.iter().map(|point| (point.x, point.y)).collect());
    }
    let path = find_state_manhattan_path(
        &[Point { x: start.0, y: start.1 }],
        Point { x: end.0, y: end.1 },
        search,
        search_bounds,
        budget,
        &|point| is_free((point.x, point.y)),
    )?;
    Some(path.into_iter().map(|point| (point.x, point.y)).collect())
}

/// `bodySafeTransitionPlan`.
fn body_safe_transition_plan(
    plan: &StateTransitionRenderPlan,
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    note_bounds: &[StateDiagramNoteBounds],
    search: &StateSearchSpace,
    budget: &mut StateSearchBudget,
    force_repair: bool,
) -> StateTransitionRenderPlan {
    if !force_repair && !route_intersects_unrelated_state(plan, diagram, layout, note_bounds) {
        return plan.clone();
    }
    let from = &layout.boxes[plan.route.from];
    let to = &layout.boxes[plan.route.to];
    let source_outside_index =
        plan.path.iter().position(|point| !point_is_inside_bounds(*point, from));
    let target_outside_index =
        plan.path.iter().rposition(|point| !point_is_inside_bounds(*point, to));
    let (Some(source_outside_index), Some(target_outside_index)) =
        (source_outside_index, target_outside_index)
    else {
        return plan.clone();
    };
    if target_outside_index < source_outside_index {
        return plan.clone();
    }
    let safe_path = find_body_safe_path(
        plan.path[source_outside_index],
        plan.path[target_outside_index],
        diagram,
        layout,
        plan,
        search,
        budget,
    );
    let Some(safe_path) = safe_path else {
        let (alternate, found) =
            alternate_body_safe_transition_plan(plan, diagram, layout, note_bounds, search, budget);
        return if found { alternate } else { plan.clone() };
    };

    let prefix = plan.path[..source_outside_index].to_vec();
    let suffix = plan.path[target_outside_index + 1..].to_vec();
    let repaired =
        render_body_safe_transition_plan(plan, &safe_path, &prefix, &suffix, None, layout);
    if safe_path.len() <= plan.path.len() + 4 {
        return repaired;
    }
    let (alternate, found) =
        alternate_body_safe_transition_plan(plan, diagram, layout, note_bounds, search, budget);
    if found && alternate.path.len() < repaired.path.len() {
        return alternate;
    }
    repaired
}

/// `alternateBodySafeTransitionPlan` — the repaired plan plus whether
/// any candidate was found (`alternate !== plan` upstream).
fn alternate_body_safe_transition_plan(
    plan: &StateTransitionRenderPlan,
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    note_bounds: &[StateDiagramNoteBounds],
    search: &StateSearchSpace,
    budget: &mut StateSearchBudget,
) -> (StateTransitionRenderPlan, bool) {
    let mut candidates: Vec<StateTransitionRenderPlan> = Vec::new();
    for source in state_route_ports(&layout.boxes[plan.route.from]) {
        for target in state_route_ports(&layout.boxes[plan.route.to]) {
            let Some(safe_path) = find_body_safe_path(
                source.outside,
                target.outside,
                diagram,
                layout,
                plan,
                search,
                budget,
            ) else {
                continue;
            };
            let from = &layout.boxes[plan.route.from];
            let to = &layout.boxes[plan.route.to];
            let prefix: Vec<(i64, i64)> =
                if from.width > 1 && from.height > 1 { vec![source.border] } else { Vec::new() };
            let suffix: Vec<(i64, i64)> =
                if plan.route.target_is_choice || plan.route.target_is_hidden_marker {
                    vec![(to.left, to.top)]
                } else {
                    Vec::new()
                };
            let repaired = render_body_safe_transition_plan(
                plan,
                &safe_path,
                &prefix,
                &suffix,
                Some(source.char),
                layout,
            );
            if !route_intersects_unrelated_state(&repaired, diagram, layout, note_bounds) {
                candidates.push(repaired);
            }
        }
    }
    let mut best: Option<StateTransitionRenderPlan> = None;
    for candidate in candidates {
        match &best {
            None => best = Some(candidate),
            Some(current) if candidate.path.len() < current.path.len() => best = Some(candidate),
            Some(_) => {}
        }
    }
    match best {
        Some(best) => (best, true),
        None => (plan.clone(), false),
    }
}

struct StateRoutePort {
    outside: (i64, i64),
    border: (i64, i64),
    char: &'static str,
}

/// `stateRoutePorts`.
fn state_route_ports(bounds: &StateDiagramBoxBounds) -> Vec<StateRoutePort> {
    [
        StateRoutePort {
            outside: (bounds.center_x, bounds.top - 1),
            border: (bounds.center_x, bounds.top),
            char: BORDER_ROUNDED.bottom_t,
        },
        StateRoutePort {
            outside: (bounds.center_x, bounds.top + bounds.height),
            border: (bounds.center_x, bounds.top + bounds.height - 1),
            char: BORDER_ROUNDED.top_t,
        },
        StateRoutePort {
            outside: (bounds.left - 1, bounds.center_y),
            border: (bounds.left, bounds.center_y),
            char: BORDER_ROUNDED.right_t,
        },
        StateRoutePort {
            outside: (bounds.left + bounds.width, bounds.center_y),
            border: (bounds.left + bounds.width - 1, bounds.center_y),
            char: BORDER_ROUNDED.left_t,
        },
    ]
    .into_iter()
    .filter(|port| port.outside.0 >= 0)
    .collect()
}

/// `renderBodySafeTransitionPlan`.
fn render_body_safe_transition_plan(
    plan: &StateTransitionRenderPlan,
    safe_path: &[(i64, i64)],
    prefix: &[(i64, i64)],
    suffix: &[(i64, i64)],
    source_char: Option<&'static str>,
    layout: &StateDiagramLayout,
) -> StateTransitionRenderPlan {
    let prefix_keys: HashSet<(i64, i64)> = prefix.iter().copied().collect();
    let to = &layout.boxes[plan.route.to];
    let mut cells: Vec<StateTransitionRenderCell> = match source_char {
        Some(source_char) => prefix
            .iter()
            .map(|&(x, y)| StateTransitionRenderCell {
                x,
                y,
                payload: RenderCellPayload::Char(source_char),
            })
            .collect(),
        None => plan
            .cells
            .iter()
            .filter(|cell| prefix_keys.contains(&(cell.x, cell.y)))
            .copied()
            .collect(),
    };
    let mut full_path: Vec<(i64, i64)> = Vec::new();
    full_path.extend_from_slice(prefix);
    full_path.extend_from_slice(safe_path);
    full_path.extend_from_slice(suffix);
    let previous = prefix.last().copied();
    for (index, &point) in safe_path.iter().enumerate() {
        if index == safe_path.len() - 1 {
            let target_direction = connection_direction(point, (to.center_x, to.center_y));
            if plan.route.target_is_hidden_marker {
                let char = match target_direction {
                    Direction::Left | Direction::Right => "─",
                    _ => "│",
                };
                cells.push(StateTransitionRenderCell {
                    x: point.0,
                    y: point.1,
                    payload: RenderCellPayload::Char(char),
                });
            } else {
                cells.push(StateTransitionRenderCell {
                    x: point.0,
                    y: point.1,
                    payload: RenderCellPayload::Arrow(target_direction),
                });
            }
            continue;
        }
        let before = if index == 0 { previous } else { Some(safe_path[index - 1]) };
        let after = safe_path[index + 1];
        let mut connections: HashSet<Direction> = HashSet::new();
        if let Some(before) = before {
            connections.insert(connection_direction(point, before));
        }
        connections.insert(connection_direction(point, after));
        cells.push(StateTransitionRenderCell {
            x: point.0,
            y: point.1,
            payload: RenderCellPayload::Char(diagram_line_glyph(
                &connections,
                crate::render::mermaid::core::drawing::CornerStyle::Rounded,
                crate::render::mermaid::core::drawing::LineStyle::Single,
            )),
        });
    }

    StateTransitionRenderPlan {
        route: plan.route.clone(),
        cells,
        path: full_path,
        label: plan.label.clone(),
        path_repaired: true,
    }
}

/// `connectionDirection`.
fn connection_direction(from: (i64, i64), to: (i64, i64)) -> Direction {
    let delta_x = to.0 - from.0;
    let delta_y = to.1 - from.1;
    if delta_x.abs() >= delta_y.abs() && delta_x != 0 {
        return if delta_x > 0 { Direction::Right } else { Direction::Left };
    }
    if delta_y != 0 {
        return if delta_y > 0 { Direction::Down } else { Direction::Up };
    }
    Direction::Right
}

/// `labelDistanceToPath`.
fn label_distance_to_path(x: i64, y: i64, width: i64, height: i64, path: &[(i64, i64)]) -> i64 {
    path.iter()
        .map(|&(path_x, path_y)| {
            let dx = if path_x < x {
                x - path_x
            } else if path_x >= x + width {
                path_x - (x + width - 1)
            } else {
                0
            };
            let dy = if path_y < y {
                y - path_y
            } else if path_y >= y + height {
                path_y - (y + height - 1)
            } else {
                0
            };
            dx + dy
        })
        .min()
        .unwrap_or(0)
}

/// `stateTransitionLabelCandidates`.
fn state_transition_label_candidates(
    plan: &StateTransitionRenderPlan,
    width: i64,
    height: i64,
) -> Vec<(i64, i64)> {
    let mut candidates: Vec<((i64, i64), i64)> = Vec::new(); // (point, insertion order)
    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    let mut order = 0i64;
    let mut add = |candidates: &mut Vec<((i64, i64), i64)>,
                   seen: &mut HashSet<(i64, i64)>,
                   x: i64,
                   y: i64| {
        if seen.insert((x, y)) {
            candidates.push(((x, y), order));
            order += 1;
        }
    };
    if let Some(label) = &plan.label {
        if !plan.path_repaired
            || label_distance_to_path(label.x, label.y, width, height, &plan.path) <= 4
        {
            add(&mut candidates, &mut seen, label.x, label.y);
        }
    }
    for &(x, y) in &plan.path {
        add(&mut candidates, &mut seen, x + 2, y - height.div_euclid(2));
        add(&mut candidates, &mut seen, x - width - 2, y - height.div_euclid(2));
        add(&mut candidates, &mut seen, x - width.div_euclid(2), y - height - 1);
        add(&mut candidates, &mut seen, x - width.div_euclid(2), y + 2);
    }
    let preferred = plan
        .label
        .as_ref()
        .map(|label| (label.x, label.y))
        .unwrap_or_else(|| plan.path.first().copied().unwrap_or((0, 0)));
    let mut sorted = candidates;
    sorted.sort_by(|left, right| {
        let left_distance = (left.0 .0 - preferred.0).abs() + (left.0 .1 - preferred.1).abs();
        let right_distance = (right.0 .0 - preferred.0).abs() + (right.0 .1 - preferred.1).abs();
        left_distance.cmp(&right_distance).then(left.1.cmp(&right.1))
    });
    sorted.into_iter().map(|(point, _)| point).collect()
}

/// `placeStateTransitionLabels`.
fn place_state_transition_labels(
    plans: &[StateTransitionRenderPlan],
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    note_bounds: &[StateDiagramNoteBounds],
) -> Result<Vec<StateTransitionRenderPlan>, StateRenderError> {
    let mut space_claims: Vec<_> = Vec::new();
    for state in &diagram.states {
        if let Some(&bound_index) = layout.bounds.get(state.id.as_str()) {
            if !is_hidden_composite_marker(Some(state)) {
                let bound = &layout.boxes[bound_index];
                space_claims.push(crate::render::mermaid::core::spatial::spatial_rect_claim(
                    format!("state:{}", state.id),
                    format!("state:{}", state.id),
                    SpatialRole::Body,
                    &crate::render::mermaid::core::geometry::bounds_from_rect(
                        bound.left,
                        bound.top,
                        bound.width,
                        bound.height,
                    ),
                ));
            }
        }
    }
    for (index, plan) in plans.iter().enumerate() {
        space_claims.push(spatial_path_claim(
            format!("route:{index}"),
            format!("route:{index}"),
            SpatialRole::Route,
            &plan.path.iter().map(|&(x, y)| Point { x, y }).collect::<Vec<_>>(),
        ));
    }
    for composite in &diagram.composites {
        if let Some(&bound_index) = layout.composite_bounds.get(composite.id.as_str()) {
            let bound = &layout.boxes[bound_index];
            space_claims.push(spatial_path_claim(
                format!("composite:{}", composite.id),
                format!("composite:{}", composite.id),
                SpatialRole::Boundary,
                &[
                    Point { x: bound.left, y: bound.top },
                    Point { x: bound.left + bound.width - 1, y: bound.top },
                    Point { x: bound.left + bound.width - 1, y: bound.top + bound.height - 1 },
                    Point { x: bound.left, y: bound.top + bound.height - 1 },
                    Point { x: bound.left, y: bound.top },
                ],
            ));
        }
    }
    for note_bound in note_bounds {
        space_claims.push(crate::render::mermaid::core::spatial::spatial_rect_claim(
            format!("note:{}", note_bound.bound.id),
            format!("note:{}", note_bound.bound.id),
            SpatialRole::Body,
            &crate::render::mermaid::core::geometry::bounds_from_rect(
                note_bound.bound.left,
                note_bound.bound.top,
                note_bound.bound.width,
                note_bound.bound.height,
            ),
        ));
        if let Some(&target_index) = layout.bounds.get(note_bound.note.target.as_str()) {
            let connector = placed_note_connector(note_bound, &layout.boxes[target_index]);
            space_claims.push(spatial_path_claim(
                format!("note-connector:{}", note_bound.bound.id),
                format!("note-connector:{}", note_bound.bound.id),
                SpatialRole::Boundary,
                &connector.points,
            ));
        }
    }
    let mut space = SpatialIndex::empty().add(space_claims);

    let mut placed: HashMap<usize, StateTransitionRenderPlan> = HashMap::new();
    let mut endpoint_counts: HashMap<String, usize> = HashMap::new();
    for plan in plans {
        let key = format!(
            "{}\0{}",
            diagram.transitions[plan.route.transition_index].from,
            diagram.transitions[plan.route.transition_index].to
        );
        *endpoint_counts.entry(key).or_insert(0) += 1;
    }
    let mut placement_order: Vec<usize> = (0..plans.len()).collect();
    placement_order.sort_by_key(|&index| u8::from(plans[index].path_repaired));

    for plan_index in placement_order {
        let plan = &plans[plan_index];
        let Some(original_label) = &plan.label else {
            placed.insert(plan_index, plan.clone());
            continue;
        };
        let width = original_label
            .lines
            .iter()
            .map(|line| diagram_text_width(line) as i64)
            .max()
            .unwrap_or(0);
        let transition = &diagram.transitions[plan.route.transition_index];
        let endpoint_key = format!("{}\0{}", transition.from, transition.to);
        let needs_lane_clearance = endpoint_counts.get(&endpoint_key).copied().unwrap_or(0) > 1;
        let state_padding: i64 = i64::from(needs_lane_clearance || original_label.lines.len() > 1);
        let label_claim = |x: i64, y: i64| {
            spatial_rect_claim(
                format!("label:{plan_index}"),
                format!("label:{plan_index}"),
                SpatialRole::Label,
                &crate::render::mermaid::core::geometry::bounds_from_rect(
                    x,
                    y,
                    width,
                    original_label.lines.len() as i64,
                ),
            )
        };
        let shares_loop_corridor = plans.iter().any(|candidate| {
            let candidate_transition = &diagram.transitions[candidate.route.transition_index];
            candidate_transition.from == transition.from
                && matches!(
                    (&candidate.route.kind, &plan.route.kind),
                    (RouteKind::Vertical, RouteKind::SelfLoop { .. })
                        | (RouteKind::SelfLoop { .. }, RouteKind::Vertical)
                )
        });
        let from = &layout.boxes[plan.route.from];
        let to = &layout.boxes[plan.route.to];
        let corridor_top: Option<i64> = if matches!(plan.route.kind, RouteKind::SideParallel { .. })
            && plans.iter().enumerate().any(|(candidate_index, candidate)| {
                candidate_index != plan_index
                    && diagram.transitions[candidate.route.transition_index].to == transition.to
            }) {
            plan.path.iter().map(|point| point.1).min()
        } else if shares_loop_corridor
            && (matches!(plan.route.kind, RouteKind::SelfLoop { .. })
                || (matches!(plan.route.kind, RouteKind::Vertical) && from.center_y < to.center_y))
        {
            Some(from.top + from.height)
        } else {
            None
        };
        let corridor_bottom =
            corridor_top.map(|_| plan.path.iter().map(|point| point.1).max().unwrap_or(0));
        let route_clearance = Clearance { x: 1, y: 0 };
        let is_clear = |x: i64, y: i64, space: &SpatialIndex| -> bool {
            if x < 0 || y < 0 {
                return false;
            }
            if let (Some(top), Some(bottom)) = (corridor_top, corridor_bottom) {
                if y < top || y > bottom {
                    return false;
                }
            }
            let mut policy = CollisionPolicy::default();
            policy
                .clearance_by_role
                .insert(SpatialRole::Body, Clearance { x: state_padding, y: state_padding });
            policy.clearance_by_role.insert(SpatialRole::Label, Clearance { x: 1, y: 1 });
            if plan.path_repaired
                || matches!(plan.route.kind, RouteKind::SideParallel { .. })
                || needs_lane_clearance
                || corridor_top.is_some()
            {
                policy.clearance_by_role.insert(SpatialRole::Route, route_clearance);
            }
            space.is_free(&label_claim(x, y), &policy)
        };

        let label_height = original_label.lines.len() as i64;
        let candidates = state_transition_label_candidates(plan, width, label_height);
        let nearby = candidates.iter().copied().find(|&(x, y)| {
            let relaxed = !plan.path_repaired
                && !matches!(plan.route.kind, RouteKind::SideParallel { .. })
                && !needs_lane_clearance
                && corridor_top.is_none();
            (relaxed || label_distance_to_path(x, y, width, label_height, &plan.path) >= 2)
                && is_clear(x, y, &space)
        });
        let mut x = nearby
            .map(|(x, _)| x)
            .or_else(|| candidates.first().map(|&(x, _)| x))
            .unwrap_or(original_label.x);
        let mut y = nearby
            .map(|(_, y)| y)
            .or_else(|| candidates.first().map(|&(_, y)| y))
            .unwrap_or(original_label.y);
        if nearby.is_none() {
            'search: for distance in 1..500i64 {
                for dx in -distance..=distance {
                    let dy = distance - dx.abs();
                    let candidate_ys: Vec<i64> =
                        if dy == 0 { vec![y] } else { vec![y - dy, y + dy] };
                    for candidate_y in candidate_ys {
                        let candidate_x = x + dx;
                        if !is_clear(candidate_x, candidate_y, &space) {
                            continue;
                        }
                        let path_distance = label_distance_to_path(
                            candidate_x,
                            candidate_y,
                            width,
                            label_height,
                            &plan.path,
                        );
                        if !(2..=8).contains(&path_distance) {
                            continue;
                        }
                        x = candidate_x;
                        y = candidate_y;
                        break 'search;
                    }
                }
            }
        }
        if !is_clear(x, y, &space) {
            'fallback: for distance in 1..500i64 {
                for dx in -distance..=distance {
                    let dy = distance - dx.abs();
                    let candidate_ys: Vec<i64> =
                        if dy == 0 { vec![y] } else { vec![y - dy, y + dy] };
                    for candidate_y in candidate_ys {
                        let candidate_x = x + dx;
                        if !is_clear(candidate_x, candidate_y, &space) {
                            continue;
                        }
                        x = candidate_x;
                        y = candidate_y;
                        break 'fallback;
                    }
                }
            }
        }
        if !is_clear(x, y, &space) {
            return Err(StateRenderError::Routing(format!(
                "Transition {endpoint_key} has no clear label position"
            )));
        }

        space = space.add(vec![label_claim(x, y)]);
        placed.insert(
            plan_index,
            StateTransitionRenderPlan {
                label: Some(StateTransitionRenderLabel { x, y, ..original_label.clone() }),
                ..plan.clone()
            },
        );
    }
    Ok(plans
        .iter()
        .enumerate()
        .map(|(index, plan)| placed.remove(&index).unwrap_or_else(|| plan.clone()))
        .collect())
}

/// `createStateTransitionRenderPlans`.
pub fn create_state_transition_render_plans(
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    feedback_lane_y: i64,
    options: StateTransitionRenderOptions<'_>,
    budget: &mut StateSearchBudget,
) -> Result<Vec<StateTransitionRenderPlan>, StateRenderError> {
    let note_bounds = options.note_bounds;
    let plans = create_state_transition_route_plans(
        diagram,
        layout,
        feedback_lane_y,
        options.feedback_top_y,
    )
    .into_iter()
    .map(|route| create_state_transition_render_plan(diagram, layout, route))
    .collect::<Vec<_>>();
    if options.repair_routes == Some(false) {
        return place_state_transition_labels(&plans, diagram, layout, note_bounds);
    }
    let base_obstacles = transition_obstacles(diagram, layout, note_bounds);
    let mut repaired: Vec<StateTransitionRenderPlan> = Vec::new();
    for (index, plan) in plans.iter().enumerate() {
        let disjoint: Vec<&StateTransitionRenderPlan> = repaired
            .iter()
            .filter(|previous| transitions_have_disjoint_endpoints(previous, plan, diagram))
            .collect();
        let route_obstacles: Vec<&StateTransitionRenderPlan> = repaired
            .iter()
            .filter(|previous| {
                disjoint.iter().any(|disjoint_plan| std::ptr::eq(*disjoint_plan, *previous))
                    || transitions_are_reciprocal(previous, plan, diagram)
            })
            .collect();
        let mut obstacle_claims: Vec<_> = Vec::new();
        for (previous_index, previous) in route_obstacles.iter().enumerate() {
            obstacle_claims.push(spatial_path_claim(
                format!("transition:{index}:obstacle:{previous_index}"),
                format!("transition:{index}:obstacle:{previous_index}"),
                SpatialRole::Route,
                &previous.path.iter().map(|&(x, y)| Point { x, y }).collect::<Vec<_>>(),
            ));
        }
        let route_space = create_state_search_space(&base_obstacles.add(obstacle_claims), |_| 0);
        let force_repair =
            disjoint.iter().any(|previous| paths_intersect(&previous.path, &plan.path));
        repaired.push(body_safe_transition_plan(
            plan,
            diagram,
            layout,
            note_bounds,
            &route_space,
            budget,
            force_repair,
        ));
    }
    place_state_transition_labels(&repaired, diagram, layout, note_bounds)
}

fn transitions_have_disjoint_endpoints(
    left: &StateTransitionRenderPlan,
    right: &StateTransitionRenderPlan,
    diagram: &StateDiagram,
) -> bool {
    let left_transition = &diagram.transitions[left.route.transition_index];
    let right_transition = &diagram.transitions[right.route.transition_index];
    left_transition.from != right_transition.from
        && left_transition.to != right_transition.from
        && left_transition.from != right_transition.to
        && left_transition.to != right_transition.to
}

fn transitions_are_reciprocal(
    left: &StateTransitionRenderPlan,
    right: &StateTransitionRenderPlan,
    diagram: &StateDiagram,
) -> bool {
    let left_transition = &diagram.transitions[left.route.transition_index];
    let right_transition = &diagram.transitions[right.route.transition_index];
    left_transition.from == right_transition.to && left_transition.to == right_transition.from
}

fn paths_intersect(left: &[(i64, i64)], right: &[(i64, i64)]) -> bool {
    let occupied: HashSet<(i64, i64)> = left.iter().copied().collect();
    right.iter().any(|point| occupied.contains(point))
}

/// `transitionObstacles` — states plus note bodies/connectors.
fn transition_obstacles(
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    note_bounds: &[StateDiagramNoteBounds],
) -> SpatialIndex {
    let mut claims: Vec<_> = Vec::new();
    for state in &diagram.states {
        if let Some(&bound_index) = layout.bounds.get(state.id.as_str()) {
            if !is_hidden_composite_marker(Some(state)) {
                let bound = &layout.boxes[bound_index];
                claims.push(crate::render::mermaid::core::spatial::spatial_rect_claim(
                    format!("state:{}", state.id),
                    format!("state:{}", state.id),
                    SpatialRole::Body,
                    &crate::render::mermaid::core::geometry::bounds_from_rect(
                        bound.left,
                        bound.top,
                        bound.width,
                        bound.height,
                    ),
                ));
            }
        }
    }
    for note_bound in note_bounds {
        claims.push(crate::render::mermaid::core::spatial::spatial_rect_claim(
            format!("note:{}", note_bound.bound.id),
            format!("note:{}", note_bound.bound.id),
            SpatialRole::Body,
            &crate::render::mermaid::core::geometry::bounds_from_rect(
                note_bound.bound.left,
                note_bound.bound.top,
                note_bound.bound.width,
                note_bound.bound.height,
            ),
        ));
        if let Some(&target_index) = layout.bounds.get(note_bound.note.target.as_str()) {
            let connector = placed_note_connector(note_bound, &layout.boxes[target_index]);
            claims.push(spatial_path_claim(
                format!("note-connector:{}", note_bound.bound.id),
                format!("note-connector:{}", note_bound.bound.id),
                SpatialRole::Boundary,
                &connector.points,
            ));
        }
    }
    SpatialIndex::empty().add(claims)
}

/// `StateTransitionJunctionPlan`.
#[derive(Debug, Clone)]
pub struct StateTransitionJunctionPlan {
    pub state: StateDiagramState,
    pub bounds: StateDiagramBoxBounds,
    pub connections: HashSet<Direction>,
    pub transitions: Vec<usize>,
    pub kind: JunctionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionKind {
    Choice,
    HiddenCompositeMarker,
}

/// `createStateTransitionJunctionPlans`.
#[must_use]
pub fn create_state_transition_junction_plans(
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    render_plans: &[StateTransitionRenderPlan],
) -> Vec<StateTransitionJunctionPlan> {
    let render_plan_by_transition: HashMap<usize, &StateTransitionRenderPlan> =
        render_plans.iter().map(|plan| (plan.route.transition_index, plan)).collect();
    let mut junctions: Vec<StateTransitionJunctionPlan> = Vec::new();
    for state in &diagram.states {
        let kind = if state.kind == StateKind::Choice {
            Some(JunctionKind::Choice)
        } else if is_hidden_composite_marker(Some(state)) {
            Some(JunctionKind::HiddenCompositeMarker)
        } else {
            None
        };
        let Some(kind) = kind else { continue };
        let Some(&state_bound_index) = layout.bounds.get(state.id.as_str()) else { continue };
        let state_bounds = &layout.boxes[state_bound_index];

        let mut connections: HashSet<Direction> = HashSet::new();
        let mut transitions: Vec<usize> = Vec::new();
        for (transition_index, transition) in diagram.transitions.iter().enumerate() {
            let render_plan = render_plan_by_transition.get(&transition_index);
            let mut connected = false;
            if transition.to == state.id {
                if let Some(plan) = render_plan {
                    if let (Some(junction), Some(neighbor)) = (
                        plan.path.last(),
                        plan.path.len().checked_sub(2).and_then(|i| plan.path.get(i)),
                    ) {
                        connections.insert(connection_direction(*junction, *neighbor));
                    }
                }
                connected = true;
            }
            if transition.from == state.id {
                if let Some(plan) = render_plan {
                    if let Some(neighbor) = plan.path.first() {
                        connections.insert(connection_direction(
                            (state_bounds.left, state_bounds.top),
                            *neighbor,
                        ));
                    }
                }
                connected = true;
            }
            if connected {
                transitions.push(transition_index);
            }
        }

        junctions.push(StateTransitionJunctionPlan {
            state: state.clone(),
            bounds: state_bounds.clone(),
            connections,
            transitions,
            kind,
        });
    }
    junctions
}

/// The cell the drawing pass paints for a route cell.
#[must_use]
pub fn render_cell_glyph(
    cell: &StateTransitionRenderCell,
    arrow_head_style: ArrowHeadStyle,
) -> &'static str {
    match cell.payload {
        RenderCellPayload::Char(char) => char,
        RenderCellPayload::Arrow(direction) => diagram_arrow_head(direction, arrow_head_style),
    }
}

/// The grid merge hook for state route glyphs (`makeGrid`'s mergeCell).
#[must_use]
pub fn merge_state_cells(
    existing: &crate::render::mermaid::core::canvas::Cell<StateCellStyle>,
    incoming: &crate::render::mermaid::core::canvas::Cell<StateCellStyle>,
) -> crate::render::mermaid::core::canvas::Cell<StateCellStyle> {
    let existing_is_transition = existing.style == Some(StateCellStyle::Transition)
        || matches!(
            existing.style,
            Some(
                StateCellStyle::StateDepartureRamp1
                    | StateCellStyle::StateDepartureRamp2
                    | StateCellStyle::StateDepartureRamp3
            )
        );
    let incoming_is_transition = incoming.style == Some(StateCellStyle::Transition)
        || matches!(
            incoming.style,
            Some(
                StateCellStyle::StateDepartureRamp1
                    | StateCellStyle::StateDepartureRamp2
                    | StateCellStyle::StateDepartureRamp3
            )
        );
    let should_merge = incoming_is_transition && existing_is_transition;
    let char: &str = if should_merge {
        match merge_diagram_line_glyph(
            existing.text(),
            incoming.text(),
            crate::render::mermaid::core::drawing::CornerStyle::Rounded,
        ) {
            Some(merged) => merged,
            None => incoming.text(),
        }
    } else {
        incoming.text()
    };
    crate::render::mermaid::core::canvas::Cell::build(char, incoming.style, incoming.attributes)
}

/// Re-exported for the drawing pass: the note bounds type it draws.
pub use crate::render::mermaid::state::layout::StateDiagramNoteBounds as NoteBoundsForDrawing;
