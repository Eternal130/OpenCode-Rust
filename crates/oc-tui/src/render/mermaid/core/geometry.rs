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

//! `core/geometry.ts` — points, bounds, orthogonal-path helpers.

/// `DiagramAxis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
}

/// `DiagramDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// `DiagramSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

/// `DiagramPoint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}

pub fn point(x: i64, y: i64) -> Point {
    Point { x, y }
}

/// `DiagramBounds`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
    pub center_x: i64,
    pub center_y: i64,
}

/// `DiagramSegment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub from: Point,
    pub to: Point,
    pub axis: Axis,
    pub direction: Direction,
    pub length: i64,
}

/// `DiagramLane`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
    pub axis: Axis,
    pub coordinate: i64,
}

/// `DiagramSpan` (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: i64,
    pub end: i64,
}

pub fn direction_axis(direction: Direction) -> Axis {
    match direction {
        Direction::Left | Direction::Right => Axis::X,
        Direction::Up | Direction::Down => Axis::Y,
    }
}

pub fn direction_sign(direction: Direction) -> i64 {
    match direction {
        Direction::Left | Direction::Up => -1,
        Direction::Right | Direction::Down => 1,
    }
}

pub fn side_for_direction(direction: Direction) -> Side {
    match direction {
        Direction::Left => Side::Left,
        Direction::Right => Side::Right,
        Direction::Up => Side::Top,
        Direction::Down => Side::Bottom,
    }
}

pub fn opposite_side(side: Side) -> Side {
    match side {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
        Side::Top => Side::Bottom,
        Side::Bottom => Side::Top,
    }
}

/// `diagramBoundsFromRect`.
#[must_use]
pub fn bounds_from_rect(left: i64, top: i64, width: i64, height: i64) -> Bounds {
    Bounds {
        left,
        top,
        width,
        height,
        center_x: left + width.div_euclid(2),
        center_y: top + height.div_euclid(2),
    }
}

/// `translateDiagramBounds` — in-place mutation upstream; we return the
/// translated copy.
#[must_use]
pub fn translate_bounds(bounds: Bounds, dx: i64, dy: i64) -> Bounds {
    Bounds {
        left: bounds.left + dx,
        top: bounds.top + dy,
        center_x: bounds.center_x + dx,
        center_y: bounds.center_y + dy,
        ..bounds
    }
}

/// `diagramBoundsFromBounds`.
#[must_use]
pub fn bounds_from_bounds(bounds: &[Bounds]) -> Option<Bounds> {
    let left = bounds.iter().map(|bound| bound.left).min()?;
    let top = bounds.iter().map(|bound| bound.top).min()?;
    let right = bounds.iter().map(|bound| bound.left + bound.width).max()?;
    let bottom = bounds.iter().map(|bound| bound.top + bound.height).max()?;
    Some(bounds_from_rect(left, top, right - left, bottom - top))
}

/// `diagramBoundsFromPoints`.
#[must_use]
pub fn bounds_from_points(points: &[Point]) -> Option<Bounds> {
    let left = points.iter().map(|point| point.x).min()?;
    let top = points.iter().map(|point| point.y).min()?;
    let right = points.iter().map(|point| point.x).max()?;
    let bottom = points.iter().map(|point| point.y).max()?;
    Some(bounds_from_rect(left, top, right - left + 1, bottom - top + 1))
}

/// `coordinate(point, axis)`.
#[must_use]
pub fn coordinate(point: Point, axis: Axis) -> i64 {
    match axis {
        Axis::X => point.x,
        Axis::Y => point.y,
    }
}

/// `withCoordinate`.
#[must_use]
pub fn with_coordinate(point: Point, axis: Axis, value: i64) -> Point {
    match axis {
        Axis::X => Point { x: value, y: point.y },
        Axis::Y => Point { x: point.x, y: value },
    }
}

/// `shiftPoint`.
#[must_use]
pub fn shift_point(point: Point, direction: Direction, distance: i64) -> Point {
    match direction {
        Direction::Left => Point { x: point.x - distance, y: point.y },
        Direction::Right => Point { x: point.x + distance, y: point.y },
        Direction::Up => Point { x: point.x, y: point.y - distance },
        Direction::Down => Point { x: point.x, y: point.y + distance },
    }
}

/// `clampPoint`.
#[must_use]
pub fn clamp_point(point: Point, min: Point) -> Point {
    Point { x: point.x.max(min.x), y: point.y.max(min.y) }
}

#[must_use]
pub fn same_point(left: Point, right: Point) -> bool {
    left.x == right.x && left.y == right.y
}

/// `directionBetween`.
#[must_use]
pub fn direction_between(from: Point, to: Point) -> Option<Direction> {
    if from.y == to.y {
        if to.x > from.x {
            return Some(Direction::Right);
        }
        if to.x < from.x {
            return Some(Direction::Left);
        }
    }
    if from.x == to.x {
        if to.y > from.y {
            return Some(Direction::Down);
        }
        if to.y < from.y {
            return Some(Direction::Up);
        }
    }
    None
}

/// `boundsCenter`.
#[must_use]
pub fn bounds_center(bounds: Bounds) -> Point {
    point(bounds.center_x, bounds.center_y)
}

/// `boundsSidePoint` — outside/border surface.
#[must_use]
pub fn bounds_side_point(bounds: Bounds, side: Side, surface: bool) -> Point {
    // surface=true is the border surface, false the outside (upstream
    // `"border" | "outside"` with outside default).
    match side {
        Side::Left => Point { x: bounds.left - i64::from(!surface), y: bounds.center_y },
        Side::Right => {
            Point { x: bounds.left + bounds.width - i64::from(surface), y: bounds.center_y }
        }
        Side::Top => Point { x: bounds.center_x, y: bounds.top - i64::from(!surface) },
        Side::Bottom => {
            Point { x: bounds.center_x, y: bounds.top + bounds.height - i64::from(surface) }
        }
    }
}

/// `centerCoordinate`.
#[must_use]
pub fn center_coordinate(bounds: Bounds, axis: Axis) -> i64 {
    match axis {
        Axis::X => bounds.center_x,
        Axis::Y => bounds.center_y,
    }
}

/// `snapCoordinate`.
#[must_use]
pub fn snap_coordinate(source: i64, target: i64, tolerance: i64) -> i64 {
    if (source - target).abs() <= tolerance {
        target
    } else {
        source
    }
}

/// `pathThrough` — dedups consecutive duplicates.
#[must_use]
pub fn path_through(points: &[Point]) -> Vec<Point> {
    let mut path = Vec::with_capacity(points.len());
    for next in points {
        if path.last().is_none_or(|last| !same_point(*last, *next)) {
            path.push(*next);
        }
    }
    path
}

pub fn lane(axis: Axis, coordinate: i64) -> Lane {
    Lane { axis, coordinate }
}

/// `pathViaLane`.
#[must_use]
pub fn path_via_lane(start: Point, route_lane: Lane, end: Point) -> Vec<Point> {
    path_through(&[
        start,
        with_coordinate(start, route_lane.axis, route_lane.coordinate),
        with_coordinate(end, route_lane.axis, route_lane.coordinate),
        end,
    ])
}

fn dominant_axis(start: Point, end: Point) -> Axis {
    if (end.x - start.x).abs() >= (end.y - start.y).abs() {
        Axis::X
    } else {
        Axis::Y
    }
}

fn terminal_lane(start: Point, end: Point, axis: Axis, terminal_clearance: i64) -> i64 {
    let start_coordinate = coordinate(start, axis);
    let end_coordinate = coordinate(end, axis);
    let delta = end_coordinate - start_coordinate;
    // Upstream keeps the ternary chain `delta > 0 ? 1 : delta < 0 ? -1 : return`.
    #[allow(clippy::comparison_chain)]
    let sign = if delta > 0 {
        1
    } else if delta < 0 {
        -1
    } else {
        return end_coordinate;
    };
    end_coordinate - sign * terminal_clearance.min((delta.abs() - 1).max(1))
}

/// `orthogonalPath`.
#[must_use]
pub fn orthogonal_path(
    start: Point,
    end: Point,
    preferred_axis: Option<Axis>,
    terminal_clearance: i64,
) -> Vec<Point> {
    if start.x == end.x || start.y == end.y {
        return path_through(&[start, end]);
    }
    let lane_axis = preferred_axis.unwrap_or_else(|| dominant_axis(start, end));
    let route = lane(lane_axis, terminal_lane(start, end, lane_axis, terminal_clearance));
    path_via_lane(start, route, end)
}

/// `segmentBetween`.
#[must_use]
pub fn segment_between(from: Point, to: Point) -> Option<Segment> {
    let direction = direction_between(from, to)?;
    let axis = direction_axis(direction);
    Some(Segment {
        from,
        to,
        axis,
        direction,
        length: (coordinate(to, axis) - coordinate(from, axis)).abs(),
    })
}

/// `walkOrthogonalSegment` — visits every cell from start (inclusive per
/// `include_start`) to `to` (exclusive); `false` from the callback stops.
pub fn walk_orthogonal_segment(
    from: Point,
    to: Point,
    include_start: bool,
    visit: &mut dyn FnMut(Point) -> bool,
) {
    let Some(direction) = direction_between(from, to) else { return };
    let (dx, dy) = match direction {
        Direction::Right => (1, 0),
        Direction::Left => (-1, 0),
        Direction::Down => (0, 1),
        Direction::Up => (0, -1),
    };
    let mut cursor = if include_start { from } else { point(from.x + dx, from.y + dy) };
    while !same_point(cursor, to) {
        if !visit(cursor) {
            return;
        }
        cursor = point(cursor.x + dx, cursor.y + dy);
    }
}

/// `orthogonalPathPoints` — every cell of the polyline.
#[must_use]
pub fn orthogonal_path_points(points: &[Point]) -> Vec<Point> {
    let mut path = Vec::new();
    for window in points.windows(2) {
        let from = window[0];
        let to = window[1];
        let Some(direction) = direction_between(from, to) else { continue };
        let (dx, dy) = match direction {
            Direction::Right => (1, 0),
            Direction::Left => (-1, 0),
            Direction::Down => (0, 1),
            Direction::Up => (0, -1),
        };
        let mut cursor = if path.is_empty() { from } else { point(from.x + dx, from.y + dy) };
        loop {
            path.push(cursor);
            if same_point(cursor, to) {
                break;
            }
            cursor = point(cursor.x + dx, cursor.y + dy);
        }
    }
    path
}

/// `orderedSpan`.
#[must_use]
pub fn ordered_span(left: i64, right: i64) -> Span {
    if left <= right {
        Span { start: left, end: right }
    } else {
        Span { start: right, end: left }
    }
}

/// `segmentSpan`.
#[must_use]
pub fn segment_span(segment: Segment) -> Span {
    ordered_span(coordinate(segment.from, segment.axis), coordinate(segment.to, segment.axis))
}

/// `pointOnSegment`.
#[must_use]
pub fn point_on_segment(segment: Segment, coordinate_value: i64) -> Point {
    with_coordinate(segment.from, segment.axis, coordinate_value)
}

/// `insetSpan`.
#[must_use]
pub fn inset_span(span: Span, amount: i64) -> Span {
    Span { start: span.start + amount, end: span.end - amount }
}

/// `spanCapacity`.
#[must_use]
pub fn span_capacity(span: Span) -> i64 {
    (span.end - span.start + 1).max(0)
}

/// `centeredSpanStart`.
#[must_use]
pub fn centered_span_start(span: Span, width: i64) -> i64 {
    span.start + (span_capacity(span) - width).div_euclid(2)
}

/// `midpoint` — JS `Math.round` (half away from zero toward +∞).
#[must_use]
pub fn midpoint(span: Span) -> i64 {
    let value = (span.start + span.end) as f64 / 2.0;
    value.round() as i64
}

/// `advanceCoordinate`.
#[must_use]
pub fn advance_coordinate(origin: i64, direction: Direction, distance: i64) -> i64 {
    origin + direction_sign(direction) * distance
}

/// `beforeNearestCoordinate`.
#[must_use]
pub fn before_nearest_coordinate(
    points: &[Point],
    axis: Axis,
    direction: Direction,
    clearance: i64,
) -> i64 {
    let nearest = if direction_sign(direction) > 0 {
        points.iter().map(|point| coordinate(*point, axis)).min().unwrap_or(0)
    } else {
        points.iter().map(|point| coordinate(*point, axis)).max().unwrap_or(0)
    };
    advance_coordinate(nearest, direction, -clearance)
}

/// `afterFarthestCoordinate`.
#[must_use]
pub fn after_farthest_coordinate(
    points: &[Point],
    axis: Axis,
    direction: Direction,
    clearance: i64,
) -> i64 {
    let farthest = if direction_sign(direction) > 0 {
        points.iter().map(|point| coordinate(*point, axis)).max().unwrap_or(0)
    } else {
        points.iter().map(|point| coordinate(*point, axis)).min().unwrap_or(0)
    };
    advance_coordinate(farthest, direction, clearance)
}

/// `keepBefore`.
#[must_use]
pub fn keep_before(preferred: i64, boundary: i64, direction: Direction) -> i64 {
    if direction_sign(direction) > 0 {
        preferred.min(boundary)
    } else {
        preferred.max(boundary)
    }
}

/// `keepAfter`.
#[must_use]
pub fn keep_after(preferred: i64, boundary: i64, direction: Direction) -> i64 {
    if direction_sign(direction) > 0 {
        preferred.max(boundary)
    } else {
        preferred.min(boundary)
    }
}
