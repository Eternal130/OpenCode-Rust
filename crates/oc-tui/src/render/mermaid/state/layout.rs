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

//! `state/layout.ts` — rank/main-path layout, composite frames, and
//! note placement. The TypeScript shares ONE mutable bounds object
//! between `layout.bounds` and `layout.compositeBounds`; the port
//! mirrors that aliasing with an index arena (`boxes` + two id maps
//! pointing at the same slots).

use std::collections::{HashMap, HashSet};

use crate::render::mermaid::core::geometry::{orthogonal_path_points, Point};
use crate::render::mermaid::core::spatial::{
    spatial_path_claim, spatial_rect_claim, Clearance, CollisionPolicy, SpatialIndex, SpatialRole,
};
use crate::render::mermaid::core::text::{diagram_text_width, measure_diagram_text_box};
use crate::render::mermaid::core::text_lines::split_diagram_lines;
use crate::render::mermaid::state::note::{
    state_diagram_note_connector, NoteConnectorGeometry, StateDiagramNoteConnector,
    TargetConnectorGeometry,
};
use crate::render::mermaid::state::routing::{
    create_state_transition_render_plans, has_reverse_transition, is_state_horizontal_feedback,
    measure_state_transition_label, StateTransitionRenderOptions, StateTransitionRenderPlan,
};
use crate::render::mermaid::state::search::{
    create_state_search_budget, create_state_search_space, find_state_manhattan_path,
    StateSearchBounds, StateSearchBudget, StateSearchSpace,
};
use crate::render::mermaid::state::types::{
    NotePosition, StateDiagram, StateDiagramCompositeState, StateDiagramNote, StateDiagramState,
    StateDiagramTransition, StateDirection, StateKind, StateRenderError,
};

const MAX_STRICT_NOTE_PLACEMENTS: usize = 24;

/// `StateDiagramBoxBounds`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDiagramBoxBounds {
    pub id: String,
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
    pub center_x: i64,
    pub center_y: i64,
}

impl StateDiagramBoxBounds {
    fn from_rect(id: &str, left: i64, top: i64, width: i64, height: i64) -> Self {
        Self {
            id: id.to_string(),
            left,
            top,
            width,
            height,
            center_x: left + width.div_euclid(2),
            center_y: top + height.div_euclid(2),
        }
    }
}

/// The shared state/composite/note size entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSize {
    pub width: i64,
    pub height: i64,
    pub lines: Vec<String>,
}

/// `StateDiagramNoteBounds` — a box bound carrying its note payload.
#[derive(Debug, Clone, PartialEq)]
pub struct StateDiagramNoteBounds {
    pub bound: StateDiagramBoxBounds,
    pub note: StateDiagramNote,
    pub lines: Vec<String>,
    pub connector: Option<StateDiagramNoteConnector>,
}

/// A reference into the layout's bounds (arena slot or note entry) —
/// the shared-identity handles `shiftBounds`/`uniqueBounds` move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BoundRef {
    Arena(usize),
    Note(usize),
}

/// `StateDiagramLayout`.
#[derive(Debug, Clone, Default)]
pub struct StateDiagramLayout {
    pub boxes: Vec<StateDiagramBoxBounds>,
    pub bounds: HashMap<String, usize>,
    pub sizes: HashMap<String, StateSize>,
    pub composite_bounds: HashMap<String, usize>,
    pub note_bounds: Vec<StateDiagramNoteBounds>,
}

impl StateDiagramLayout {
    fn push_box(&mut self, bound: StateDiagramBoxBounds) -> usize {
        let index = self.boxes.len();
        self.boxes.push(bound.clone());
        self.bounds.insert(bound.id, index);
        index
    }

    /// `layout.bounds.get(id)`.
    #[must_use]
    pub fn box_of(&self, id: &str) -> Option<&StateDiagramBoxBounds> {
        self.bounds.get(id).map(|&index| &self.boxes[index])
    }

    #[must_use]
    pub fn box_mut(&mut self, id: &str) -> Option<&mut StateDiagramBoxBounds> {
        let index = *self.bounds.get(id)?;
        Some(&mut self.boxes[index])
    }
}

/// `StateDiagramLayoutOptions`.
#[derive(Debug, Clone, Copy)]
pub struct StateDiagramLayoutOptions {
    pub min_state_gap: i64,
    pub search_budget: Option<StateSearchBudget>,
}

/// `computeRanks`.
fn compute_ranks(diagram: &StateDiagram) -> HashMap<String, i64> {
    let mut ranks: HashMap<String, i64> = HashMap::new();
    let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
    for transition in &diagram.transitions {
        outgoing.entry(transition.from.as_str()).or_default().push(transition.to.as_str());
    }

    let first = diagram
        .states
        .iter()
        .find(|state| state.kind == StateKind::Start)
        .map(|state| state.id.as_str())
        .or_else(|| diagram.states.first().map(|state| state.id.as_str()));
    let Some(first) = first else { return ranks };
    ranks.insert(first.to_string(), 0);
    let mut queue: Vec<String> = vec![first.to_string()];
    let mut cursor = 0usize;
    while cursor < queue.len() {
        let id = queue[cursor].clone();
        cursor += 1;
        let rank = ranks.get(&id).copied().unwrap_or(0);
        for to in outgoing.get(id.as_str()).map_or(&[] as &[&str], |list| list.as_slice()) {
            let next_rank = rank + 1;
            if ranks.get(*to).copied().unwrap_or(i64::MAX) <= next_rank {
                continue;
            }
            ranks.insert((*to).to_string(), next_rank);
            queue.push((*to).to_string());
        }
    }

    for state in &diagram.states {
        let fallback = ranks.len() as i64;
        ranks.entry(state.id.clone()).or_insert(fallback);
    }
    ranks
}

fn outgoing_transitions(diagram: &StateDiagram) -> HashMap<&str, Vec<usize>> {
    let mut outgoing: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, transition) in diagram.transitions.iter().enumerate() {
        outgoing.entry(transition.from.as_str()).or_default().push(index);
    }
    outgoing
}

fn reaches(
    outgoing: &HashMap<&str, Vec<usize>>,
    diagram: &StateDiagram,
    from: &str,
    target: &str,
) -> bool {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = vec![from];
    while let Some(id) = stack.pop() {
        if id == target {
            return true;
        }
        if visited.contains(id) {
            continue;
        }
        visited.insert(id);
        for index in outgoing.get(id).map_or(&[] as &[usize], |list| list.as_slice()) {
            stack.push(diagram.transitions[*index].to.as_str());
        }
    }
    false
}

/// `computeMainPath`.
fn compute_main_path(diagram: &StateDiagram) -> Vec<String> {
    let outgoing = outgoing_transitions(diagram);
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let Some(start) = diagram
        .states
        .iter()
        .find(|state| state.kind == StateKind::Start)
        .map(|state| state.id.clone())
        .or_else(|| diagram.states.first().map(|state| state.id.clone()))
    else {
        return Vec::new();
    };

    let mut path = vec![start.clone()];
    let mut visited: HashSet<String> = HashSet::from([start.clone()]);
    let mut current = start;
    loop {
        let candidates: Vec<usize> = outgoing
            .get(current.as_str())
            .map_or(&[] as &[usize], |list| list.as_slice())
            .iter()
            .copied()
            .filter(|index| !visited.contains(&diagram.transitions[*index].to))
            .collect();
        if candidates.is_empty() {
            break;
        }
        let next = candidates
            .iter()
            .copied()
            .find(|index| {
                states_by_id
                    .get(diagram.transitions[*index].to.as_str())
                    .is_some_and(|s| s.kind == StateKind::End)
            })
            .or_else(|| {
                candidates.iter().copied().find(|index| {
                    !reaches(&outgoing, diagram, &diagram.transitions[*index].to, &current)
                })
            })
            .or_else(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|index| !has_reverse_transition(diagram, &diagram.transitions[*index]))
            })
            .or_else(|| {
                candidates.iter().copied().find(|index| {
                    let transition = &diagram.transitions[*index];
                    let from_parent =
                        states_by_id.get(current.as_str()).and_then(|s| s.parent_id.as_deref());
                    let to_parent = states_by_id
                        .get(transition.to.as_str())
                        .and_then(|s| s.parent_id.as_deref());
                    from_parent.is_some() && to_parent.is_some() && from_parent != to_parent
                })
            })
            .or_else(|| (candidates.len() == 1).then_some(candidates[0]));
        let Some(next) = next else { break };
        path.push(diagram.transitions[next].to.clone());
        visited.insert(diagram.transitions[next].to.clone());
        current.clone_from(&diagram.transitions[next].to);
    }

    path
}

/// `stateSize`.
fn state_size(state: &StateDiagramState) -> StateSize {
    if state.kind != StateKind::State {
        return StateSize { width: 1, height: 1, lines: vec![state.label.clone()] };
    }
    let size = measure_diagram_text_box(&state.label, 2, 1, 1);
    StateSize { width: size.width as i64, height: size.height as i64, lines: size.lines }
}

/// `noteLines`.
fn note_lines(note: &StateDiagramNote) -> Vec<String> {
    let lines: Vec<String> = note
        .lines
        .iter()
        .flat_map(|line| split_diagram_lines(line))
        .map(|line| line.trim().to_string())
        .collect();
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

/// `noteSize`.
fn note_size(note: &StateDiagramNote) -> StateSize {
    let lines = note_lines(note);
    let inner_width =
        lines.iter().map(|line| diagram_text_width(line) as i64).max().unwrap_or(1).max(1);
    StateSize { width: inner_width + 4, height: lines.len() as i64 + 2, lines }
}

fn empty_layout(
    bounds: HashMap<String, usize>,
    sizes: HashMap<String, StateSize>,
    boxes: Vec<StateDiagramBoxBounds>,
) -> StateDiagramLayout {
    StateDiagramLayout {
        boxes,
        bounds,
        sizes,
        composite_bounds: HashMap::new(),
        note_bounds: Vec::new(),
    }
}

/// `translateDiagramBounds` for one arena box.
fn translate_box_bounds(bound: &mut StateDiagramBoxBounds, dx: i64, dy: i64) {
    bound.left += dx;
    bound.top += dy;
    bound.center_x += dx;
    bound.center_y += dy;
}

/// `shiftBounds` — shifts the referenced boxes and translates note
/// connectors alongside their bounds.
fn shift_bounds(layout: &mut StateDiagramLayout, refs: &[BoundRef], dx: i64, dy: i64) {
    for bound_ref in refs {
        match *bound_ref {
            BoundRef::Arena(index) => translate_box_bounds(&mut layout.boxes[index], dx, dy),
            BoundRef::Note(index) => {
                let note_bound = &mut layout.note_bounds[index];
                translate_box_bounds(&mut note_bound.bound, dx, dy);
                if let Some(connector) = &mut note_bound.connector {
                    connector.connector_y += dy;
                    for point in &mut connector.points {
                        point.x += dx;
                        point.y += dy;
                    }
                }
            }
        }
    }
}

/// `translateStateDiagramLayout`.
pub fn translate_state_diagram_layout(layout: &mut StateDiagramLayout, dx: i64, dy: i64) {
    let refs = unique_all_bounds(layout);
    shift_bounds(layout, &refs, dx, dy);
}

/// `uniqueBounds(bounds.values(), compositeBounds.values(), noteBounds)`
/// — the arena dedupes composite boxes shared between the two maps.
fn unique_all_bounds(layout: &StateDiagramLayout) -> Vec<BoundRef> {
    let mut seen: HashSet<usize> = HashSet::new();
    let mut refs: Vec<BoundRef> = Vec::new();
    for index in layout.bounds.values() {
        if seen.insert(*index) {
            refs.push(BoundRef::Arena(*index));
        }
    }
    for index in 0..layout.note_bounds.len() {
        refs.push(BoundRef::Note(index));
    }
    refs
}

/// `normalizeLayout`.
fn normalize_layout(layout: &mut StateDiagramLayout) {
    if layout.boxes.is_empty() && layout.note_bounds.is_empty() {
        return;
    }
    let mut min_x = 0;
    let mut min_y = 0;
    for index in layout.bounds.values() {
        let bound = &layout.boxes[*index];
        min_x = min_x.min(bound.left);
        min_y = min_y.min(bound.top);
    }
    for note_bound in &layout.note_bounds {
        min_x = min_x.min(note_bound.bound.left);
        min_y = min_y.min(note_bound.bound.top);
        if let Some(connector) = &note_bound.connector {
            for point in &connector.points {
                min_x = min_x.min(point.x);
                min_y = min_y.min(point.y);
            }
        }
    }
    if min_x == 0 && min_y == 0 {
        return;
    }
    let refs = unique_all_bounds(layout);
    shift_bounds(layout, &refs, -min_x, -min_y);
}

/// `addCompositeBounds`.
fn add_composite_bounds(diagram: &StateDiagram, layout: &mut StateDiagramLayout) {
    let mut states_by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut composites_by_parent: HashMap<&str, Vec<&StateDiagramCompositeState>> = HashMap::new();
    for state in &diagram.states {
        if let Some(parent) = state.parent_id.as_deref() {
            states_by_parent.entry(parent).or_default().push(state.id.as_str());
        }
    }
    for composite in &diagram.composites {
        if let Some(parent) = composite.parent_id.as_deref() {
            composites_by_parent.entry(parent).or_default().push(composite);
        }
    }

    fn add(
        composite: &StateDiagramCompositeState,
        layout: &mut StateDiagramLayout,
        states_by_parent: &HashMap<&str, Vec<&str>>,
        composites_by_parent: &HashMap<&str, Vec<&StateDiagramCompositeState>>,
    ) -> Option<usize> {
        if let Some(&existing) = layout.composite_bounds.get(&composite.id) {
            return Some(existing);
        }

        if let Some(children) = composites_by_parent.get(composite.id.as_str()) {
            for child in children {
                add(child, layout, states_by_parent, composites_by_parent);
            }
        }

        let mut child_indexes: Vec<usize> = Vec::new();
        if let Some(states) = states_by_parent.get(composite.id.as_str()) {
            for id in states {
                if let Some(&index) = layout.bounds.get(*id) {
                    child_indexes.push(index);
                }
            }
        }
        if let Some(children) = composites_by_parent.get(composite.id.as_str()) {
            for child in children {
                if let Some(&index) = layout.bounds.get(child.id.as_str()) {
                    child_indexes.push(index);
                }
            }
        }
        if child_indexes.is_empty() {
            return None;
        }

        let left =
            child_indexes.iter().map(|&index| layout.boxes[index].left).min().unwrap_or(0) - 2;
        let top = child_indexes.iter().map(|&index| layout.boxes[index].top).min().unwrap_or(0) - 2;
        let right = child_indexes
            .iter()
            .map(|&index| layout.boxes[index].left + layout.boxes[index].width)
            .max()
            .unwrap_or(0)
            + 2;
        let bottom = child_indexes
            .iter()
            .map(|&index| layout.boxes[index].top + layout.boxes[index].height)
            .max()
            .unwrap_or(0)
            + 2;
        let width = (right - left).max(diagram_text_width(&composite.label) as i64 + 5);
        let bound = StateDiagramBoxBounds {
            id: composite.id.clone(),
            left,
            top,
            width,
            height: bottom - top,
            center_x: left + width.div_euclid(2),
            center_y: top + (bottom - top).div_euclid(2),
        };
        let index = layout.push_box(bound);
        layout.composite_bounds.insert(composite.id.clone(), index);
        Some(index)
    }

    for composite in &diagram.composites {
        add(composite, layout, &states_by_parent, &composites_by_parent);
    }
}

/// `intersects`.
fn box_intersects(
    left: i64,
    top: i64,
    width: i64,
    height: i64,
    bound: &StateDiagramBoxBounds,
    padding: i64,
) -> bool {
    left < bound.left + bound.width + padding
        && left + width + padding > bound.left
        && top < bound.top + bound.height + padding
        && top + height + padding > bound.top
}

/// `createNoteBound`.
fn create_note_bound(
    note: &StateDiagramNote,
    index: usize,
    left: i64,
    top: i64,
    size: &StateSize,
) -> StateDiagramNoteBounds {
    StateDiagramNoteBounds {
        bound: StateDiagramBoxBounds::from_rect(
            &format!("{}-note-{index}", note.target),
            left,
            top,
            size.width,
            size.height,
        ),
        note: note.clone(),
        lines: size.lines.clone(),
        connector: None,
    }
}

/// `findNoteConnector`.
fn find_note_connector(
    search: &StateSearchSpace,
    bounds: &StateDiagramNoteBounds,
    target: &StateDiagramBoxBounds,
    budget: &mut StateSearchBudget,
) -> Option<StateDiagramNoteConnector> {
    let connector_y =
        (bounds.bound.top + 1).max(target.center_y.min(bounds.bound.top + bounds.bound.height - 2));
    let end = Point {
        x: match bounds.note.position {
            NotePosition::Right => bounds.bound.left - 1,
            NotePosition::Left => bounds.bound.left + bounds.bound.width,
        },
        y: connector_y,
    };
    let goal = Point {
        x: end.x + if bounds.note.position == NotePosition::Right { -1 } else { 1 },
        y: end.y,
    };
    let target_x = match bounds.note.position {
        NotePosition::Right => target.left + target.width,
        NotePosition::Left => target.left - 1,
    };
    let mut preferred_starts = vec![
        Point { x: target_x, y: target.center_y },
        Point { x: target_x, y: target.top - 1 },
        Point { x: target_x, y: target.top + target.height },
    ];
    let mut perimeter_starts: Vec<Point> = Vec::new();
    for offset in 0..target.height {
        perimeter_starts.push(Point { x: target.left - 1, y: target.top + offset });
        perimeter_starts.push(Point { x: target.left + target.width, y: target.top + offset });
    }
    for offset in 0..target.width {
        perimeter_starts.push(Point { x: target.left + offset, y: target.top - 1 });
        perimeter_starts.push(Point { x: target.left + offset, y: target.top + target.height });
    }
    preferred_starts.extend(perimeter_starts);
    let starts = preferred_starts;
    let is_free = |point: Point| -> bool {
        point.x >= 0
            && !search.blocked.contains(&(point.x, point.y))
            && !(point.x >= bounds.bound.left
                && point.x < bounds.bound.left + bounds.bound.width
                && point.y >= bounds.bound.top
                && point.y < bounds.bound.top + bounds.bound.height)
    };
    if !is_free(end) || !is_free(goal) {
        return None;
    }

    for start in starts.iter().copied().filter(|point| is_free(*point)) {
        let direct_paths: [[Point; 4]; 2] = [
            [start, Point { x: start.x, y: goal.y }, goal, end],
            [start, Point { x: goal.x, y: start.y }, goal, end],
        ];
        if let Some(direct) = direct_paths
            .iter()
            .find(|points| orthogonal_path_points(points.as_slice()).iter().copied().all(&is_free))
        {
            return Some(StateDiagramNoteConnector { connector_y, points: direct.to_vec() });
        }
    }

    let margin = 8;
    let min_y = target.top.min(bounds.bound.top).min(search.min_y) - margin;
    let max_x =
        (target.left + target.width).max(bounds.bound.left + bounds.bound.width).max(search.max_x)
            + margin;
    let max_y =
        (target.top + target.height).max(bounds.bound.top + bounds.bound.height).max(search.max_y)
            + margin;
    let path = find_state_manhattan_path(
        &starts,
        goal,
        search,
        StateSearchBounds { min_x: 0, min_y, max_x, max_y },
        budget,
        &is_free,
    )?;
    let mut points = path;
    points.push(end);
    Some(StateDiagramNoteConnector { points, connector_y })
}

/// `belongsToComposite`.
fn belongs_to_composite(
    id: &str,
    composite_id: &str,
    states_by_id: &HashMap<&str, &StateDiagramState>,
    composites_by_id: &HashMap<&str, &StateDiagramCompositeState>,
) -> bool {
    let mut parent_id = states_by_id
        .get(id)
        .and_then(|state| state.parent_id.as_deref())
        .or_else(|| composites_by_id.get(id).and_then(|composite| composite.parent_id.as_deref()));

    while let Some(parent) = parent_id {
        if parent == composite_id {
            return true;
        }
        parent_id =
            composites_by_id.get(parent).and_then(|composite| composite.parent_id.as_deref());
    }

    false
}

/// `expandCompositeBoundsForNotes`.
fn expand_composite_bounds_for_notes(diagram: &StateDiagram, layout: &mut StateDiagramLayout) {
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let composites_by_id: HashMap<&str, &StateDiagramCompositeState> =
        diagram.composites.iter().map(|composite| (composite.id.as_str(), composite)).collect();

    for composite in diagram.composites.iter().rev() {
        let Some(&bound_index) = layout.composite_bounds.get(&composite.id) else { continue };

        let descendant_notes: Vec<usize> = layout
            .note_bounds
            .iter()
            .enumerate()
            .filter(|(_, note_bound)| {
                belongs_to_composite(
                    &note_bound.note.target,
                    &composite.id,
                    &states_by_id,
                    &composites_by_id,
                )
            })
            .map(|(index, _)| index)
            .collect();
        if descendant_notes.is_empty() {
            continue;
        }

        let mut left = layout.boxes[bound_index].left;
        let mut top = layout.boxes[bound_index].top;
        let mut right = layout.boxes[bound_index].left + layout.boxes[bound_index].width;
        let mut bottom = layout.boxes[bound_index].top + layout.boxes[bound_index].height;
        for &note_index in &descendant_notes {
            let note_bound = &layout.note_bounds[note_index];
            left = left.min(note_bound.bound.left - 2);
            top = top.min(note_bound.bound.top - 1);
            right = right.max(note_bound.bound.left + note_bound.bound.width + 2);
            bottom = bottom.max(note_bound.bound.top + note_bound.bound.height + 1);
            if let Some(connector) = &note_bound.connector {
                for point in &connector.points {
                    left = left.min(point.x - 1);
                    top = top.min(point.y - 1);
                    right = right.max(point.x + 2);
                    bottom = bottom.max(point.y + 2);
                }
            }
        }

        let bound = &mut layout.boxes[bound_index];
        bound.left = left;
        bound.top = top;
        bound.width = (right - left).max(diagram_text_width(&composite.label) as i64 + 5);
        bound.height = bottom - top;
        bound.center_x = bound.left + bound.width.div_euclid(2);
        bound.center_y = bound.top + bound.height.div_euclid(2);
    }
}

/// `innermostCommonCompositeId`.
fn innermost_common_composite_id(
    transition: &StateDiagramTransition,
    states_by_id: &HashMap<&str, &StateDiagramState>,
    composites_by_id: &HashMap<&str, &StateDiagramCompositeState>,
) -> Option<String> {
    let containers = |id: &str| -> Vec<&str> {
        let mut ids: Vec<&str> = Vec::new();
        let mut parent_id =
            states_by_id.get(id).and_then(|state| state.parent_id.as_deref()).or_else(|| {
                composites_by_id.get(id).and_then(|composite| composite.parent_id.as_deref())
            });
        while let Some(parent) = parent_id {
            ids.push(parent);
            parent_id =
                composites_by_id.get(parent).and_then(|composite| composite.parent_id.as_deref());
        }
        ids
    };
    let target: HashSet<&str> = containers(&transition.to).into_iter().collect();
    containers(&transition.from).into_iter().find(|id| target.contains(id)).map(str::to_string)
}

/// `expandCompositeBoundsForInternalRouting`.
fn expand_composite_bounds_for_internal_routing(
    diagram: &StateDiagram,
    layout: &mut StateDiagramLayout,
) {
    if diagram.direction == StateDirection::Lr || diagram.direction == StateDirection::Rl {
        return;
    }
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let composites_by_id: HashMap<&str, &StateDiagramCompositeState> =
        diagram.composites.iter().map(|composite| (composite.id.as_str(), composite)).collect();

    for composite in diagram.composites.iter().rev() {
        let Some(&bound_index) = layout.composite_bounds.get(&composite.id) else { continue };
        let internal: Vec<&StateDiagramTransition> = diagram
            .transitions
            .iter()
            .filter(|transition| {
                transition.from != transition.to
                    && innermost_common_composite_id(transition, &states_by_id, &composites_by_id)
                        .as_deref()
                        == Some(composite.id.as_str())
            })
            .collect();
        let mut endpoint_occurrences: HashMap<String, usize> = HashMap::new();
        let has_nested_composite = diagram
            .composites
            .iter()
            .any(|candidate| candidate.parent_id.as_deref() == Some(composite.id.as_str()));
        let side_routes: Vec<&StateDiagramTransition> = internal
            .iter()
            .copied()
            .filter(|transition| {
                let Some(from) = layout.bounds.get(&transition.from) else { return false };
                let Some(to) = layout.bounds.get(&transition.to) else { return false };
                let from = &layout.boxes[*from];
                let to = &layout.boxes[*to];
                let key = format!("{}\0{}", transition.from, transition.to);
                let occurrence = endpoint_occurrences.get(&key).copied().unwrap_or(0);
                endpoint_occurrences.insert(key, occurrence + 1);
                let from_parent =
                    states_by_id.get(transition.from.as_str()).and_then(|s| s.parent_id.as_deref());
                let to_parent =
                    states_by_id.get(transition.to.as_str()).and_then(|s| s.parent_id.as_deref());
                occurrence > 0
                    || (from.center_y > to.center_y
                        && (!has_nested_composite
                            || from.left - 1 <= layout.boxes[bound_index].left))
                    || (from_parent != to_parent
                        && (from.center_x != to.center_x
                            || states_by_id
                                .get(transition.from.as_str())
                                .is_some_and(|s| s.kind != StateKind::State)
                            || states_by_id
                                .get(transition.to.as_str())
                                .is_some_and(|s| s.kind != StateKind::State)))
            })
            .collect();
        if side_routes.is_empty() {
            continue;
        }
        let child_right = diagram
            .states
            .iter()
            .filter_map(|state| {
                if !belongs_to_composite(&state.id, &composite.id, &states_by_id, &composites_by_id)
                {
                    return None;
                }
                layout
                    .bounds
                    .get(&state.id)
                    .map(|&index| layout.boxes[index].left + layout.boxes[index].width)
            })
            .chain(diagram.composites.iter().filter_map(|child| {
                if child.parent_id.as_deref() != Some(composite.id.as_str()) {
                    return None;
                }
                layout
                    .composite_bounds
                    .get(&child.id)
                    .map(|&index| layout.boxes[index].left + layout.boxes[index].width)
            }))
            .max();
        let Some(child_right) = child_right else { continue };
        let label_width = side_routes
            .iter()
            .map(|transition| measure_state_transition_label(&transition.label).width)
            .max()
            .unwrap_or(0);
        let right = child_right + label_width + side_routes.len() as i64 * 3 + 6;
        if right <= layout.boxes[bound_index].left + layout.boxes[bound_index].width {
            continue;
        }
        let bound = &mut layout.boxes[bound_index];
        bound.width = right - bound.left;
        bound.center_x = bound.left + bound.width.div_euclid(2);
    }

    enforce_composite_margins(diagram, layout);
}

/// `enforceCompositeMargins`.
fn enforce_composite_margins(diagram: &StateDiagram, layout: &mut StateDiagramLayout) {
    let mut composites_by_parent: HashMap<&str, Vec<&StateDiagramCompositeState>> = HashMap::new();
    for composite in &diagram.composites {
        if let Some(parent) = composite.parent_id.as_deref() {
            composites_by_parent.entry(parent).or_default().push(composite);
        }
    }

    fn expand(
        composite: &StateDiagramCompositeState,
        layout: &mut StateDiagramLayout,
        composites_by_parent: &HashMap<&str, Vec<&StateDiagramCompositeState>>,
    ) -> Option<usize> {
        let bound_index = *layout.composite_bounds.get(&composite.id)?;
        let children: Vec<usize> = composites_by_parent
            .get(composite.id.as_str())
            .map_or(&[] as &[&StateDiagramCompositeState], |list| list.as_slice())
            .iter()
            .filter_map(|child| expand(child, layout, composites_by_parent))
            .collect();
        if children.is_empty() {
            return Some(bound_index);
        }
        let left = children
            .iter()
            .map(|&index| layout.boxes[index].left - 2)
            .min()
            .unwrap_or(layout.boxes[bound_index].left)
            .min(layout.boxes[bound_index].left);
        let top = children
            .iter()
            .map(|&index| layout.boxes[index].top - 2)
            .min()
            .unwrap_or(layout.boxes[bound_index].top)
            .min(layout.boxes[bound_index].top);
        let right = children
            .iter()
            .map(|&index| layout.boxes[index].left + layout.boxes[index].width + 2)
            .max()
            .unwrap_or(0)
            .max(layout.boxes[bound_index].left + layout.boxes[bound_index].width);
        let bottom = children
            .iter()
            .map(|&index| layout.boxes[index].top + layout.boxes[index].height + 2)
            .max()
            .unwrap_or(0)
            .max(layout.boxes[bound_index].top + layout.boxes[bound_index].height);
        let bound = &mut layout.boxes[bound_index];
        bound.left = left;
        bound.top = top;
        bound.width = right - left;
        bound.height = bottom - top;
        bound.center_x = left + bound.width.div_euclid(2);
        bound.center_y = top + bound.height.div_euclid(2);
        Some(bound_index)
    }

    for composite in diagram.composites.iter().filter(|composite| composite.parent_id.is_none()) {
        expand(composite, layout, &composites_by_parent);
    }
}

fn bounds_intersect(left: &StateDiagramBoxBounds, right: &StateDiagramBoxBounds) -> bool {
    box_intersects(left.left, left.top, left.width, left.height, right, 0)
}

/// `separateExternalBoundsFromComposites`.
pub fn separate_external_bounds_from_composites(
    diagram: &StateDiagram,
    layout: &mut StateDiagramLayout,
) -> bool {
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let composites_by_id: HashMap<&str, &StateDiagramCompositeState> =
        diagram.composites.iter().map(|composite| (composite.id.as_str(), composite)).collect();
    let mut shifted = false;

    for composite in &diagram.composites {
        let Some(&composite_index) = layout.composite_bounds.get(&composite.id) else { continue };

        for state in &diagram.states {
            if belongs_to_composite(&state.id, &composite.id, &states_by_id, &composites_by_id) {
                continue;
            }
            let Some(&bound_index) = layout.bounds.get(&state.id) else { continue };
            if !bounds_intersect(&layout.boxes[bound_index], &layout.boxes[composite_index]) {
                continue;
            }

            let dx = layout.boxes[composite_index].left + layout.boxes[composite_index].width + 4
                - layout.boxes[bound_index].left;
            if dx <= 0 {
                continue;
            }
            let left_threshold = layout.boxes[bound_index].left;
            let mut bounds_to_shift: Vec<BoundRef> = Vec::new();

            for candidate in &diagram.states {
                if belongs_to_composite(
                    &candidate.id,
                    &composite.id,
                    &states_by_id,
                    &composites_by_id,
                ) {
                    continue;
                }
                if let Some(&candidate_index) = layout.bounds.get(&candidate.id) {
                    if layout.boxes[candidate_index].left >= left_threshold {
                        bounds_to_shift.push(BoundRef::Arena(candidate_index));
                    }
                }
            }

            for candidate in &diagram.composites {
                if candidate.id == composite.id
                    || belongs_to_composite(
                        &candidate.id,
                        &composite.id,
                        &states_by_id,
                        &composites_by_id,
                    )
                {
                    continue;
                }
                if let Some(&candidate_index) = layout.composite_bounds.get(&candidate.id) {
                    if layout.boxes[candidate_index].left >= left_threshold {
                        bounds_to_shift.push(BoundRef::Arena(candidate_index));
                    }
                }
            }

            let shift_targets: HashSet<usize> = bounds_to_shift
                .iter()
                .filter_map(|bound_ref| match bound_ref {
                    BoundRef::Arena(index) => Some(*index),
                    BoundRef::Note(_) => None,
                })
                .collect();
            for (note_index, note_bound) in layout.note_bounds.iter().enumerate() {
                if let Some(&target_index) = layout.bounds.get(&note_bound.note.target) {
                    if shift_targets.contains(&target_index) {
                        bounds_to_shift.push(BoundRef::Note(note_index));
                    }
                }
            }

            let mut unique: Vec<BoundRef> = Vec::new();
            let mut seen: HashSet<BoundRef> = HashSet::new();
            for bound_ref in bounds_to_shift {
                if seen.insert(bound_ref) {
                    unique.push(bound_ref);
                }
            }
            shift_bounds(layout, &unique, dx, 0);
            shifted = true;
        }
    }
    shifted
}

/// `finalizeLayout`.
fn finalize_layout(
    diagram: &StateDiagram,
    layout: &mut StateDiagramLayout,
    budget: &mut StateSearchBudget,
) -> Result<(), StateRenderError> {
    if diagram.composites.is_empty() && diagram.notes.is_empty() {
        return Ok(());
    }
    add_composite_bounds(diagram, layout);
    normalize_layout(layout);
    expand_composite_bounds_for_internal_routing(diagram, layout);
    if !diagram.notes.is_empty() {
        let feedback_lane_y =
            layout.boxes.iter().map(|bound| bound.top + bound.height).max().unwrap_or(0).max(0) + 3;
        let feedback_top_y =
            layout.boxes.iter().map(|bound| bound.top).min().unwrap_or(0).min(0) - 3;
        let transition_plans = create_state_transition_render_plans(
            diagram,
            layout,
            feedback_lane_y,
            StateTransitionRenderOptions {
                feedback_top_y: Some(feedback_top_y),
                repair_routes: Some(false),
                ..Default::default()
            },
            budget,
        )?;
        place_state_diagram_notes_around_transitions(diagram, layout, &transition_plans, budget)?;
    }
    expand_composite_bounds_for_notes(diagram, layout);
    expand_composite_bounds_for_internal_routing(diagram, layout);
    separate_external_bounds_from_composites(diagram, layout);
    normalize_layout(layout);
    Ok(())
}

/// `createStateDiagramLayout`.
pub fn create_state_diagram_layout(
    diagram: &StateDiagram,
    options: &StateDiagramLayoutOptions,
) -> Result<StateDiagramLayout, StateRenderError> {
    let mut budget = options.search_budget.unwrap_or_else(create_state_search_budget);
    if diagram.direction == StateDirection::Lr || diagram.direction == StateDirection::Rl {
        let mut layout = create_horizontal_layout(diagram, options);
        finalize_layout(diagram, &mut layout, &mut budget)?;
        return Ok(layout);
    }

    let ranks = compute_ranks(diagram);
    let max_rank = ranks.values().copied().max().unwrap_or(0).max(0);
    let mut by_rank: std::collections::BTreeMap<i64, Vec<&StateDiagramState>> =
        std::collections::BTreeMap::new();
    for state in &diagram.states {
        let rank = if diagram.direction == StateDirection::Bt {
            max_rank - ranks.get(&state.id).copied().unwrap_or(0)
        } else {
            ranks.get(&state.id).copied().unwrap_or(0)
        };
        by_rank.entry(rank).or_default().push(state);
    }

    let rank_keys: Vec<i64> = by_rank.keys().copied().collect();
    let sizes: HashMap<String, StateSize> =
        diagram.states.iter().map(|state| (state.id.clone(), state_size(state))).collect();
    let mut boxes: Vec<StateDiagramBoxBounds> = Vec::new();
    let mut bounds: HashMap<String, usize> = HashMap::new();
    let mut outgoing_label_rows: HashMap<String, i64> = HashMap::new();
    let mut self_transition_counts: HashMap<String, i64> = HashMap::new();
    for transition in &diagram.transitions {
        let rows = measure_state_transition_label(&transition.label).height;
        let entry = outgoing_label_rows.entry(transition.from.clone()).or_insert(0);
        *entry = (*entry).max(rows);
        if transition.from == transition.to {
            let count = self_transition_counts.entry(transition.from.clone()).or_insert(0);
            *count += 1;
        }
    }

    let single_column_center = rank_keys
        .iter()
        .filter_map(|rank| {
            let states = &by_rank[rank];
            (states.len() == 1)
                .then(|| sizes.get(states[0].id.as_str()).unwrap().width.div_euclid(2))
        })
        .max()
        .unwrap_or(0)
        .max(0);
    let mut y = 0i64;
    for rank in &rank_keys {
        let states = &by_rank[rank];
        let row_height = states
            .iter()
            .map(|state| sizes.get(state.id.as_str()).unwrap().height)
            .max()
            .unwrap_or(0);
        let mut x = 0i64;
        for state in states {
            let size = sizes.get(state.id.as_str()).unwrap();
            let top = y + (row_height - size.height).div_euclid(2);
            let left =
                if states.len() == 1 { single_column_center - size.width.div_euclid(2) } else { x };
            let index = boxes.len();
            boxes.push(StateDiagramBoxBounds::from_rect(
                &state.id,
                left,
                top,
                size.width,
                size.height,
            ));
            bounds.insert(state.id.clone(), index);
            x += size.width + options.min_state_gap + 8;
        }
        let label_rows = states
            .iter()
            .map(|state| outgoing_label_rows.get(state.id.as_str()).copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
        let self_transition_rows = states
            .iter()
            .map(|state| {
                self_transition_counts.get(state.id.as_str()).copied().unwrap_or(0) * 3 + 1
            })
            .max()
            .unwrap_or(0);
        let pseudo_state_approach_clearance =
            i64::from(states.iter().any(|state| state.kind == StateKind::Choice)) * 2;
        y += row_height
            + 4.max(label_rows + 3).max(self_transition_rows)
            + pseudo_state_approach_clearance;
    }

    let mut layout = empty_layout(bounds, sizes, boxes);
    finalize_layout(diagram, &mut layout, &mut budget)?;
    Ok(layout)
}

fn insert_bound(
    boxes: &mut Vec<StateDiagramBoxBounds>,
    bounds: &mut HashMap<String, usize>,
    id: &str,
    left: i64,
    top: i64,
    width: i64,
    height: i64,
) {
    let index = boxes.len();
    boxes.push(StateDiagramBoxBounds::from_rect(id, left, top, width, height));
    bounds.insert(id.to_string(), index);
}

/// `createHorizontalLayout`.
fn create_horizontal_layout(
    diagram: &StateDiagram,
    options: &StateDiagramLayoutOptions,
) -> StateDiagramLayout {
    let sizes: HashMap<String, StateSize> =
        diagram.states.iter().map(|state| (state.id.clone(), state_size(state))).collect();
    let mut boxes: Vec<StateDiagramBoxBounds> = Vec::new();
    let mut bounds: HashMap<String, usize> = HashMap::new();
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let main_path = compute_main_path(diagram);
    let main_ids: HashSet<&str> = main_path.iter().map(String::as_str).collect();
    let main_path_index: HashMap<&str, usize> =
        main_path.iter().enumerate().map(|(index, id)| (id.as_str(), index)).collect();
    let baseline_y = diagram
        .transitions
        .iter()
        .map(|transition| measure_state_transition_label(&transition.label).height)
        .max()
        .unwrap_or(0)
        .max(1);
    let default_gap = options.min_state_gap + 8;
    let mut x = 0i64;

    for (index, id) in main_path.iter().enumerate() {
        let Some(state) = states_by_id.get(id.as_str()) else { continue };
        let Some(size) = sizes.get(id.as_str()) else { continue };
        let top = if state.kind == StateKind::State {
            baseline_y - size.height.div_euclid(2)
        } else {
            baseline_y
        };
        insert_bound(&mut boxes, &mut bounds, id, x, top, size.width, size.height);
        let next_id = main_path.get(index + 1);
        let adjacent_label_width = diagram
            .transitions
            .iter()
            .filter(|transition| {
                transition.from.as_str() == id.as_str()
                    && next_id.is_some_and(|next| transition.to.as_str() == next.as_str())
            })
            .map(|transition| measure_state_transition_label(&transition.label).width)
            .max()
            .unwrap_or(0);
        let crosses_composite_boundary = next_id.is_some_and(|next| {
            states_by_id.get(id.as_str()).and_then(|s| s.parent_id.as_deref())
                != states_by_id.get(next.as_str()).and_then(|s| s.parent_id.as_deref())
        });
        x += size.width
            + default_gap
                .max(adjacent_label_width + if crosses_composite_boundary { 6 } else { 2 });
    }

    let mut branches_by_parent: Vec<(String, Vec<String>)> = Vec::new();
    for transition in &diagram.transitions {
        if main_ids.contains(transition.from.as_str()) && !main_ids.contains(transition.to.as_str())
        {
            if let Some(entry) =
                branches_by_parent.iter_mut().find(|(parent, _)| parent == &transition.from)
            {
                if !entry.1.contains(&transition.to) {
                    entry.1.push(transition.to.clone());
                }
            } else {
                branches_by_parent.push((transition.from.clone(), vec![transition.to.clone()]));
            }
        }
    }

    for (parent_id, branch_ids) in &branches_by_parent {
        let Some(&parent_index) = bounds.get(parent_id.as_str()) else { continue };
        let parent = boxes[parent_index].clone();
        let branch_gap = default_gap;
        let branch_sizes: Vec<&StateSize> =
            branch_ids.iter().filter_map(|id| sizes.get(id.as_str())).collect();
        let total_width = branch_sizes.iter().map(|size| size.width).sum::<i64>()
            + 0.max(branch_sizes.len() as i64 - 1) * branch_gap;
        let parent_index_in_path = main_path_index.get(parent_id.as_str()).copied();
        let join_ids: Vec<Option<&str>> = branch_ids
            .iter()
            .map(|id| {
                diagram.transitions.iter().find_map(|transition| {
                    let to_index = main_path_index.get(transition.to.as_str()).copied();
                    (transition.from.as_str() == id.as_str()
                        && main_ids.contains(transition.to.as_str())
                        && parent_index_in_path.is_some()
                        && to_index.unwrap_or(0) > parent_index_in_path.unwrap_or(0) + 1)
                        .then_some(transition.to.as_str())
                })
            })
            .collect();
        let common_join = join_ids
            .first()
            .and_then(|first| {
                first.map(|first| join_ids.iter().all(|id| *id == Some(first)).then_some(first))
            })
            .flatten();
        let parallel_lane =
            parent_index_in_path.filter(|_| common_join.is_some()).and_then(|index| {
                bounds.get(main_path[index + 1].as_str()).map(|&lane| boxes[lane].clone())
            });
        let mut left = parallel_lane.as_ref().map_or(parent.center_x, |lane| lane.center_x)
            - total_width.div_euclid(2);
        for branch_id in branch_ids {
            if bounds.contains_key(branch_id.as_str()) {
                continue;
            }
            let Some(size) = sizes.get(branch_id.as_str()) else { continue };
            let top = available_state_top(
                &boxes,
                left,
                baseline_y + if parallel_lane.is_some() { 6 } else { 5 },
                size.width,
                size.height,
            );
            insert_bound(&mut boxes, &mut bounds, branch_id, left, top, size.width, size.height);
            left += size.width + branch_gap;
        }
    }

    let ranks = compute_ranks(diagram);
    let mut fallback_states: Vec<&StateDiagramState> =
        diagram.states.iter().filter(|state| !bounds.contains_key(state.id.as_str())).collect();
    fallback_states.sort_by_key(|state| ranks.get(state.id.as_str()).copied().unwrap_or(0));
    for state in fallback_states {
        let size = sizes.get(state.id.as_str()).unwrap();
        let top = baseline_y + 5;
        let rank = ranks.get(state.id.as_str()).copied().unwrap_or(bounds.len() as i64);
        let mut left = rank * (size.width + default_gap);
        loop {
            let collision = boxes.iter().find(|bound| {
                left < bound.left + bound.width + default_gap
                    && left + size.width + default_gap > bound.left
                    && top < bound.top + bound.height
                    && top + size.height > bound.top
            });
            match collision {
                Some(collision) => left = collision.left + collision.width + default_gap,
                None => break,
            }
        }
        insert_bound(&mut boxes, &mut bounds, &state.id, left, top, size.width, size.height);
    }

    let min_x = boxes.iter().map(|bound| bound.left).min().unwrap_or(0).min(0);
    if min_x < 0 {
        for bound in &mut boxes {
            bound.left -= min_x;
            bound.center_x -= min_x;
        }
    }

    if diagram.direction == StateDirection::Rl {
        let right = boxes.iter().map(|bound| bound.left + bound.width).max().unwrap_or(0).max(0);
        for bound in &mut boxes {
            bound.left = right - bound.left - bound.width;
            bound.center_x = bound.left + bound.width.div_euclid(2);
        }

        let branch_label_gutter = diagram
            .transitions
            .iter()
            .filter_map(|transition| {
                let from = bounds.get(transition.from.as_str()).map(|&index| &boxes[index])?;
                let to = bounds.get(transition.to.as_str()).map(|&index| &boxes[index])?;
                (from.center_y != to.center_y)
                    .then_some(measure_state_transition_label(&transition.label).width + 2)
            })
            .max()
            .unwrap_or(0)
            .max(0);
        if branch_label_gutter > 0 {
            for bound in &mut boxes {
                translate_box_bounds(bound, branch_label_gutter, 0);
            }
        }
    }

    empty_layout(bounds, sizes, boxes)
}

/// `availableStateTop`.
fn available_state_top(
    boxes: &[StateDiagramBoxBounds],
    left: i64,
    top: i64,
    width: i64,
    height: i64,
) -> i64 {
    let mut available = top;
    loop {
        let collision =
            boxes.iter().find(|bound| box_intersects(left, available, width, height, bound, 0));
        match collision {
            Some(collision) => available = collision.top + collision.height + 3,
            None => return available,
        }
    }
}

/// `expandCompositeBoundsForFeedback`.
pub fn expand_composite_bounds_for_feedback(
    diagram: &StateDiagram,
    layout: &mut StateDiagramLayout,
    feedback_lane_y: i64,
) {
    if diagram.direction != StateDirection::Lr && diagram.direction != StateDirection::Rl {
        return;
    }
    let states_by_id = layout_states_by_id(diagram);
    let composites_by_id = layout_composites_by_id(diagram);

    for composite in &diagram.composites {
        let Some(&composite_index) = layout.composite_bounds.get(&composite.id) else { continue };

        let has_internal_feedback = diagram.transitions.iter().any(|transition| {
            let Some(&from_index) = layout.bounds.get(transition.from.as_str()) else {
                return false;
            };
            let Some(&to_index) = layout.bounds.get(transition.to.as_str()) else { return false };
            belongs_to_composite(&transition.from, &composite.id, &states_by_id, &composites_by_id)
                && belongs_to_composite(
                    &transition.to,
                    &composite.id,
                    &states_by_id,
                    &composites_by_id,
                )
                && is_state_horizontal_feedback(
                    diagram,
                    &layout.boxes[from_index],
                    &layout.boxes[to_index],
                )
        });
        if !has_internal_feedback {
            continue;
        }

        let bound = &mut layout.boxes[composite_index];
        let bottom = (bound.top + bound.height).max(feedback_lane_y + 2);
        bound.height = bottom - bound.top;
        bound.center_y = bound.top + bound.height.div_euclid(2);
    }
}

fn layout_states_by_id(diagram: &StateDiagram) -> HashMap<&str, &StateDiagramState> {
    diagram.states.iter().map(|state| (state.id.as_str(), state)).collect()
}

fn layout_composites_by_id(diagram: &StateDiagram) -> HashMap<&str, &StateDiagramCompositeState> {
    diagram.composites.iter().map(|composite| (composite.id.as_str(), composite)).collect()
}

fn path_points(path: &[(i64, i64)]) -> Vec<Point> {
    path.iter().map(|&(x, y)| Point { x, y }).collect()
}

fn rect_claim_bounds(
    bound: &StateDiagramBoxBounds,
) -> crate::render::mermaid::core::geometry::Bounds {
    crate::render::mermaid::core::geometry::bounds_from_rect(
        bound.left,
        bound.top,
        bound.width,
        bound.height,
    )
}

fn note_geometry(note_bound: &StateDiagramNoteBounds) -> NoteConnectorGeometry {
    NoteConnectorGeometry {
        left: note_bound.bound.left,
        top: note_bound.bound.top,
        width: note_bound.bound.width,
        height: note_bound.bound.height,
        center_y: note_bound.bound.center_y,
        position: note_bound.note.position,
    }
}

fn target_geometry(bound: &StateDiagramBoxBounds) -> TargetConnectorGeometry {
    TargetConnectorGeometry {
        left: bound.left,
        top: bound.top,
        width: bound.width,
        height: bound.height,
        center_y: bound.center_y,
    }
}

/// The memoized-or-default connector for a placed note bound.
pub(crate) fn placed_note_connector(
    note_bound: &StateDiagramNoteBounds,
    target: &StateDiagramBoxBounds,
) -> StateDiagramNoteConnector {
    if let Some(connector) = &note_bound.connector {
        return connector.clone();
    }
    state_diagram_note_connector(&note_geometry(note_bound), &target_geometry(target))
}

/// `placeStateDiagramNotesAroundTransitions`.
fn place_state_diagram_notes_around_transitions(
    diagram: &StateDiagram,
    layout: &mut StateDiagramLayout,
    transition_plans: &[StateTransitionRenderPlan],
    budget: &mut StateSearchBudget,
) -> Result<(), StateRenderError> {
    if diagram.notes.is_empty() {
        return Ok(());
    }
    let composite_ids: HashSet<&str> =
        diagram.composites.iter().map(|composite| composite.id.as_str()).collect();
    let note_space_claims: Vec<_> = layout
        .bounds
        .values()
        .filter(|&&index| !composite_ids.contains(layout.boxes[index].id.as_str()))
        .map(|&index| {
            let bound = &layout.boxes[index];
            spatial_rect_claim(
                format!("state:{}", bound.id),
                format!("state:{}", bound.id),
                SpatialRole::Body,
                &rect_claim_bounds(bound),
            )
        })
        .collect();
    let mut note_space = SpatialIndex::empty().add(note_space_claims);

    let mut terminal_claims: Vec<_> = Vec::new();
    for (plan_index, plan) in transition_plans.iter().enumerate() {
        let head = path_points(&plan.path[..plan.path.len().min(2)]);
        terminal_claims.push(spatial_path_claim(
            format!("transition-terminal:{plan_index}:source"),
            format!("transition-terminal:{plan_index}:source"),
            SpatialRole::Route,
            &head,
        ));
        let tail = path_points(&plan.path[plan.path.len().saturating_sub(2)..]);
        terminal_claims.push(spatial_path_claim(
            format!("transition-terminal:{plan_index}:target"),
            format!("transition-terminal:{plan_index}:target"),
            SpatialRole::Route,
            &tail,
        ));
    }
    let mut reserved = note_space.clone().add(terminal_claims);

    let mut space_claims: Vec<_> = Vec::new();
    for (plan_index, plan) in transition_plans.iter().enumerate() {
        let path = path_points(&plan.path);
        space_claims.push(spatial_path_claim(
            format!("transition:{plan_index}"),
            format!("transition:{plan_index}"),
            SpatialRole::Route,
            &path,
        ));
        if let Some(label) = &plan.label {
            let width = label
                .lines
                .iter()
                .map(|line| diagram_text_width(line) as i64)
                .max()
                .unwrap_or(0)
                .max(0);
            space_claims.push(spatial_rect_claim(
                format!("transition-label:{plan_index}"),
                format!("transition-label:{plan_index}"),
                SpatialRole::Label,
                &crate::render::mermaid::core::geometry::bounds_from_rect(
                    label.x,
                    label.y,
                    width,
                    label.lines.len() as i64,
                ),
            ));
        }
    }
    let mut space = reserved.clone().add(space_claims);
    let mut placed_note_bounds: Vec<StateDiagramNoteBounds> = Vec::new();

    for (index, note) in diagram.notes.iter().enumerate() {
        let Some(&target_index) = layout.bounds.get(note.target.as_str()) else { continue };
        let target = layout.boxes[target_index].clone();
        let size = note_size(note);
        let gap = 4;
        let base_left = match note.position {
            NotePosition::Right => target.left + target.width + gap,
            NotePosition::Left => target.left - size.width - gap,
        };
        let base_top = target.center_y - size.height.div_euclid(2);
        let mut candidates: Vec<i64> =
            vec![base_top, target.top - size.height - 2, target.top + target.height + 2];
        for distance in 0..12 {
            candidates.push(base_top - distance - 1);
            candidates.push(base_top + distance + 1);
        }
        let mut candidate_bounds: Vec<StateDiagramNoteBounds> = Vec::new();
        for outward in 0..5 {
            for top in &candidates {
                candidate_bounds.push(create_note_bound(
                    note,
                    index,
                    base_left
                        + if note.position == NotePosition::Right { 1 } else { -1 }
                            * outward
                            * (size.width + 2),
                    *top,
                    &size,
                ));
            }
        }
        let find_placement =
            |candidate_space: &SpatialIndex,
             limit: usize,
             budget: &mut StateSearchBudget|
             -> Option<(StateDiagramNoteBounds, StateDiagramNoteConnector)> {
                let connector_search = create_state_search_space(candidate_space, |role| {
                    i64::from(role == SpatialRole::Label)
                });
                for bound in candidate_bounds.iter().take(limit) {
                    if bound.bound.left < 0 {
                        continue;
                    }
                    let owner = format!("note:{index}");
                    let policy = CollisionPolicy {
                        clearance: Some(Clearance { x: 1, y: 1 }),
                        ..Default::default()
                    };
                    let body_claim = spatial_rect_claim(
                        format!("{owner}:body"),
                        owner.clone(),
                        SpatialRole::Body,
                        &rect_claim_bounds(&bound.bound),
                    );
                    if !candidate_space.is_free(&body_claim, &policy) {
                        continue;
                    }
                    let connector = find_note_connector(&connector_search, bound, &target, budget)?;
                    return Some((
                        StateDiagramNoteBounds {
                            connector: Some(connector.clone()),
                            ..bound.clone()
                        },
                        connector,
                    ));
                }
                None
            };
        let placement = find_placement(&space, 1, budget)
            .or_else(|| find_placement(&note_space, 1, budget))
            .or_else(|| find_placement(&space, MAX_STRICT_NOTE_PLACEMENTS, budget))
            .or_else(|| find_placement(&reserved, candidate_bounds.len(), budget))
            .or_else(|| outside_note_placement(&note_space, note, index, &target, &size, budget));
        let (mut bound, connector) = placement.ok_or_else(|| {
            StateRenderError::Layout(format!("State {} has no exterior note corridor", note.target))
        })?;
        bound.connector = Some(connector.clone());
        let owner = format!("note:{index}");
        let claims = vec![
            spatial_rect_claim(
                format!("{owner}:body"),
                owner.clone(),
                SpatialRole::Body,
                &rect_claim_bounds(&bound.bound),
            ),
            spatial_path_claim(
                format!("{owner}:connector"),
                owner.clone(),
                SpatialRole::Boundary,
                &connector.points,
            ),
        ];
        placed_note_bounds.push(bound);
        note_space = note_space.add(claims.clone());
        reserved = reserved.add(claims.clone());
        space = space.add(claims);
    }

    layout.note_bounds = placed_note_bounds;
    Ok(())
}

/// `outsideNotePlacement`.
fn outside_note_placement(
    space: &SpatialIndex,
    note: &StateDiagramNote,
    index: usize,
    target: &StateDiagramBoxBounds,
    size: &StateSize,
    _budget: &mut StateSearchBudget,
) -> Option<(StateDiagramNoteBounds, StateDiagramNoteConnector)> {
    let search = create_state_search_space(space, |_| 0);
    let top = search.max_y + 4;
    let owner = format!("note:{index}:outside");
    let default_policy = CollisionPolicy::default();
    let body_policy =
        CollisionPolicy { clearance: Some(Clearance { x: 1, y: 1 }), ..Default::default() };

    for position in [
        note.position,
        if note.position == NotePosition::Left { NotePosition::Right } else { NotePosition::Left },
    ] {
        let aligned = create_note_bound(
            &StateDiagramNote { position, ..note.clone() },
            index,
            if position == NotePosition::Left {
                search.min_x - size.width - 4
            } else {
                search.max_x + 4
            },
            target.center_y - size.height.div_euclid(2),
            size,
        );
        let aligned_note_x = if position == NotePosition::Left {
            aligned.bound.left + aligned.bound.width
        } else {
            aligned.bound.left - 1
        };
        let aligned_connector_y = (aligned.bound.top + 1)
            .max(target.center_y.min(aligned.bound.top + aligned.bound.height - 2));
        let aligned_target_x = if position == NotePosition::Left {
            target.left - 1
        } else {
            target.left + target.width
        };
        let aligned_connector = StateDiagramNoteConnector {
            connector_y: aligned_connector_y,
            points: vec![
                Point { x: aligned_target_x, y: target.center_y },
                Point { x: aligned_note_x, y: aligned_connector_y },
            ],
        };
        if space.is_free(
            &spatial_rect_claim(
                format!("{owner}:body"),
                owner.clone(),
                SpatialRole::Body,
                &rect_claim_bounds(&aligned.bound),
            ),
            &body_policy,
        ) && space.is_free(
            &spatial_path_claim(
                format!("{owner}:connector"),
                owner.clone(),
                SpatialRole::Boundary,
                &aligned_connector.points,
            ),
            &default_policy,
        ) {
            return Some((
                StateDiagramNoteBounds { connector: Some(aligned_connector.clone()), ..aligned },
                aligned_connector,
            ));
        }

        let rail_x =
            if position == NotePosition::Left { search.min_x - 2 } else { search.max_x + 2 };
        let bound = create_note_bound(
            &StateDiagramNote { position, ..note.clone() },
            index,
            if position == NotePosition::Left { rail_x - size.width - 2 } else { rail_x + 3 },
            top,
            size,
        );
        let note_x = if position == NotePosition::Left {
            bound.bound.left + bound.bound.width
        } else {
            bound.bound.left - 1
        };
        let connector_y = bound.bound.top + 1;
        let side_x = if position == NotePosition::Left {
            target.left - 1
        } else {
            target.left + target.width
        };
        let mut escapes: Vec<[Point; 2]> = Vec::new();
        for y in [target.top, target.center_y, target.top + target.height - 1] {
            escapes.push([Point { x: side_x, y }, Point { x: rail_x, y }]);
        }
        escapes.push([
            Point { x: target.center_x, y: target.top - 1 },
            Point { x: rail_x, y: target.top - 1 },
        ]);
        escapes.push([
            Point { x: target.center_x, y: target.top + target.height },
            Point { x: rail_x, y: target.top + target.height },
        ]);

        for escape in escapes {
            let mut points: Vec<Point> = escape.to_vec();
            points.push(Point { x: rail_x, y: connector_y });
            points.push(Point { x: note_x, y: connector_y });
            let connector = StateDiagramNoteConnector { points, connector_y };
            if space.is_free(
                &spatial_rect_claim(
                    format!("{owner}:body"),
                    owner.clone(),
                    SpatialRole::Body,
                    &rect_claim_bounds(&bound.bound),
                ),
                &body_policy,
            ) && space.is_free(
                &spatial_path_claim(
                    format!("{owner}:connector"),
                    owner.clone(),
                    SpatialRole::Boundary,
                    &connector.points,
                ),
                &default_policy,
            ) {
                return Some((
                    StateDiagramNoteBounds { connector: Some(connector.clone()), ..bound },
                    connector,
                ));
            }
        }
    }

    for vertical in [Vertical::Below, Vertical::Above] {
        let rail_y = if vertical == Vertical::Below { search.max_y + 2 } else { search.min_y - 2 };
        let bound = create_note_bound(
            note,
            index,
            if note.position == NotePosition::Left {
                search.min_x - size.width - 4
            } else {
                search.max_x + 4
            },
            if vertical == Vertical::Below { rail_y + 2 } else { rail_y - size.height - 2 },
            size,
        );
        let note_x = if note.position == NotePosition::Left {
            bound.bound.left + bound.bound.width
        } else {
            bound.bound.left - 1
        };
        let connector_y = bound.bound.top + 1;
        let target_y =
            if vertical == Vertical::Below { target.top + target.height } else { target.top - 1 };
        let mut escapes: Vec<Vec<Point>> = Vec::new();
        for x in [target.left, target.center_x, target.left + target.width - 1] {
            escapes.push(vec![Point { x, y: target_y }, Point { x, y: rail_y }]);
        }
        escapes.push(vec![
            Point { x: target.left, y: target_y },
            Point { x: target.left - 1, y: target_y },
            Point { x: target.left - 1, y: rail_y },
        ]);
        escapes.push(vec![
            Point { x: target.left + target.width - 1, y: target_y },
            Point { x: target.left + target.width, y: target_y },
            Point { x: target.left + target.width, y: rail_y },
        ]);

        for escape in escapes {
            let mut points = escape;
            points.push(Point { x: note_x, y: rail_y });
            points.push(Point { x: note_x, y: connector_y });
            let connector = StateDiagramNoteConnector { points, connector_y };
            if space.is_free(
                &spatial_rect_claim(
                    format!("{owner}:body"),
                    owner.clone(),
                    SpatialRole::Body,
                    &rect_claim_bounds(&bound.bound),
                ),
                &body_policy,
            ) && space.is_free(
                &spatial_path_claim(
                    format!("{owner}:connector"),
                    owner.clone(),
                    SpatialRole::Boundary,
                    &connector.points,
                ),
                &default_policy,
            ) {
                return Some((
                    StateDiagramNoteBounds { connector: Some(connector.clone()), ..bound },
                    connector,
                ));
            }
        }
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vertical {
    Below,
    Above,
}

/// `expandCompositeBoundsForInternalTransitions`.
pub fn expand_composite_bounds_for_internal_transitions(
    diagram: &StateDiagram,
    layout: &mut StateDiagramLayout,
    transition_plans: &[StateTransitionRenderPlan],
) {
    let states_by_id = layout_states_by_id(diagram);
    let composites_by_id = layout_composites_by_id(diagram);

    for composite in &diagram.composites {
        let Some(&bound_index) = layout.composite_bounds.get(&composite.id) else { continue };
        let internal_plans: Vec<&StateTransitionRenderPlan> = transition_plans
            .iter()
            .filter(|plan| {
                let transition = &diagram.transitions[plan.route.transition_index];
                belongs_to_composite(
                    &transition.from,
                    &composite.id,
                    &states_by_id,
                    &composites_by_id,
                ) && belongs_to_composite(
                    &transition.to,
                    &composite.id,
                    &states_by_id,
                    &composites_by_id,
                )
            })
            .collect();
        let mut occupied: Vec<Point> = Vec::new();
        for plan in &internal_plans {
            for cell in &plan.cells {
                occupied.push(Point { x: cell.x, y: cell.y });
            }
            if let Some(label) = &plan.label {
                for (row, line) in label.lines.iter().enumerate() {
                    for column in 0..diagram_text_width(line) as i64 {
                        occupied.push(Point { x: label.x + column, y: label.y + row as i64 });
                    }
                }
            }
        }
        if occupied.is_empty() {
            continue;
        }

        let left = layout.boxes[bound_index]
            .left
            .min(occupied.iter().map(|point| point.x).min().unwrap_or(0) - 1);
        let top = layout.boxes[bound_index]
            .top
            .min(occupied.iter().map(|point| point.y).min().unwrap_or(0) - 1);
        let right = (layout.boxes[bound_index].left + layout.boxes[bound_index].width)
            .max(occupied.iter().map(|point| point.x).max().unwrap_or(0) + 2);
        let bottom = (layout.boxes[bound_index].top + layout.boxes[bound_index].height)
            .max(occupied.iter().map(|point| point.y).max().unwrap_or(0) + 2);
        let bound = &mut layout.boxes[bound_index];
        bound.left = left;
        bound.top = top;
        bound.width = right - left;
        bound.height = bottom - top;
        bound.center_x = bound.left + bound.width.div_euclid(2);
        bound.center_y = bound.top + bound.height.div_euclid(2);
    }

    enforce_composite_margins(diagram, layout);
}
