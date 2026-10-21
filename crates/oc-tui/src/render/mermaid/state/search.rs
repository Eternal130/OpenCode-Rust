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

//! `state/search.ts` — the blocked-cell search space and the
//! budgeted greedy-priority BFS used for note connectors and
//! body-safe transition repairs.

use std::collections::{HashMap, HashSet};

use crate::render::mermaid::core::geometry::Point;
use crate::render::mermaid::core::spatial::{SpatialIndex, SpatialRole};

const MAX_RENDER_SEARCH_VISITS: i64 = 250_000;

/// `StateSearchBudget`.
#[derive(Debug, Clone, Copy)]
pub struct StateSearchBudget {
    pub remaining: i64,
}

/// `createStateSearchBudget`.
#[must_use]
pub fn create_state_search_budget() -> StateSearchBudget {
    StateSearchBudget { remaining: MAX_RENDER_SEARCH_VISITS }
}

/// `StateSearchSpace`.
#[derive(Debug, Clone, Default)]
pub struct StateSearchSpace {
    pub blocked: HashSet<(i64, i64)>,
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}

fn sign(value: i64) -> i64 {
    match value {
        value if value > 0 => 1,
        value if value < 0 => -1,
        _ => 0,
    }
}

fn point_key(point: Point) -> (i64, i64) {
    (point.x, point.y)
}

/// `createStateSearchSpace`.
#[must_use]
pub fn create_state_search_space(
    space: &SpatialIndex,
    clearance: impl Fn(SpatialRole) -> i64,
) -> StateSearchSpace {
    let mut blocked: HashSet<(i64, i64)> = HashSet::new();
    let mut min_x = 0;
    let mut min_y = 0;
    let mut max_x = 0;
    let mut max_y = 0;

    for claim in space.claims() {
        let padding = clearance(claim.role);
        for span in &claim.spans {
            for y in (span.y - padding)..=(span.y + padding) {
                for x in (span.from_x - padding)..=(span.to_x + padding) {
                    blocked.insert((x, y));
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
    }

    StateSearchSpace { blocked, min_x, min_y, max_x, max_y }
}

/// The search-space window `findStateManhattanPath` clamps to.
#[derive(Debug, Clone, Copy)]
pub struct StateSearchBounds {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}

/// `findStateManhattanPath` — BFS that tries goal-heading steps first;
/// returns the path from a start to `goal`, or `None`.
pub fn find_state_manhattan_path(
    starts: &[Point],
    goal: Point,
    _space: &StateSearchSpace,
    bounds: StateSearchBounds,
    budget: &mut StateSearchBudget,
    is_free: &dyn Fn(Point) -> bool,
) -> Option<Vec<Point>> {
    let mut queue: Vec<Point> = Vec::new();
    for point in starts {
        if is_free(*point) && !queue.contains(point) {
            queue.push(*point);
        }
    }
    let mut parents: HashMap<(i64, i64), Option<(i64, i64)>> =
        queue.iter().map(|point| (point_key(*point), None)).collect();
    let mut points: HashMap<(i64, i64), Point> =
        queue.iter().map(|point| (point_key(*point), *point)).collect();

    let mut cursor = 0usize;
    while cursor < queue.len() && budget.remaining > 0 {
        let current = queue[cursor];
        cursor += 1;
        budget.remaining -= 1;
        if current.x == goal.x && current.y == goal.y {
            break;
        }
        let dx = sign(goal.x - current.x);
        let dy = sign(goal.y - current.y);
        let mut candidates: Vec<Point> = Vec::new();
        if dx != 0 {
            candidates.push(Point { x: current.x + dx, y: current.y });
        }
        if dy != 0 {
            candidates.push(Point { x: current.x, y: current.y + dy });
        }
        candidates.push(Point { x: current.x + 1, y: current.y });
        candidates.push(Point { x: current.x, y: current.y + 1 });
        candidates.push(Point { x: current.x - 1, y: current.y });
        candidates.push(Point { x: current.x, y: current.y - 1 });
        for candidate in candidates {
            if candidate.x < bounds.min_x
                || candidate.x > bounds.max_x
                || candidate.y < bounds.min_y
                || candidate.y > bounds.max_y
            {
                continue;
            }
            let key = point_key(candidate);
            if parents.contains_key(&key) || !is_free(candidate) {
                continue;
            }
            parents.insert(key, Some(point_key(current)));
            points.insert(key, candidate);
            queue.push(candidate);
        }
    }

    parents.get(&point_key(goal))?;
    let mut path: Vec<Point> = Vec::new();
    let mut cursor = Some(point_key(goal));
    while let Some(key) = cursor {
        path.push(*points.get(&key)?);
        cursor = parents.get(&key).copied().flatten();
    }
    path.reverse();
    Some(path)
}
