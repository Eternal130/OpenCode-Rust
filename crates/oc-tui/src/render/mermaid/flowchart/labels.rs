//! `flowchart/labels.ts` — edge-label geometry.

use crate::render::mermaid::core::geometry::{
    centered_span_start, clamp_point, inset_span, midpoint, point, point_on_segment,
    segment_between, segment_span, shift_point, span_capacity, Axis, Direction, Point, Segment,
    Side,
};
use crate::render::mermaid::core::text::{
    diagram_text_width, measure_diagram_label_width, pad_diagram_label_line,
};
use crate::render::mermaid::core::text_lines::split_diagram_lines;

const LABEL_BUS_CLEARANCE: i64 = 3;
const LABEL_NODE_CLEARANCE: i64 = 2;
const LABEL_LINE_CLEARANCE: i64 = 2;
const LABEL_TERMINAL_CLEARANCE: i64 = 1;

/// `FlowchartEdgeLabelLayout`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeLabelLayout {
    pub lines: Vec<String>,
    pub point: Point,
    pub width: i64,
    pub height: i64,
}

#[must_use]
pub fn flowchart_label_text(label: &str) -> String {
    pad_diagram_label_line(label)
}

#[must_use]
pub fn flowchart_label_width(label: &str) -> i64 {
    measure_diagram_label_width(label) as i64
}

fn minimum_inline_label_length(label_width: i64) -> i64 {
    label_width + LABEL_LINE_CLEARANCE * 2 - 1
}

#[must_use]
pub fn flowchart_horizontal_label_rank_gap(label_width: i64) -> i64 {
    minimum_inline_label_length(label_width) + LABEL_BUS_CLEARANCE + 1
}

#[must_use]
pub fn flowchart_vertical_branch_label_gap(label_width: i64) -> i64 {
    minimum_inline_label_length(label_width) + LABEL_BUS_CLEARANCE + LABEL_NODE_CLEARANCE
}

fn inline_label_slot(segment: Segment, label_width: i64) -> (i64, bool) {
    let slot = inset_span(segment_span(segment), LABEL_LINE_CLEARANCE);
    (centered_span_start(slot, label_width), span_capacity(slot) >= label_width)
}

fn segment_label_point(segment: Segment, label_width: i64, label_height: i64) -> Point {
    if segment.axis == Axis::X {
        let slot = inline_label_slot(segment, label_width);
        if label_height == 1 && slot.1 {
            return point(slot.0, segment.from.y);
        }
        if label_height > 1 {
            return shift_point(point(slot.0, segment.from.y), Direction::Up, label_height);
        }
        return clamp_point(
            shift_point(
                shift_point(segment.from, segment.direction, LABEL_LINE_CLEARANCE),
                Direction::Up,
                label_height,
            ),
            point(0, 0),
        );
    }

    let slot = inset_span(segment_span(segment), LABEL_TERMINAL_CLEARANCE);
    if span_capacity(slot) >= label_height {
        return clamp_point(
            point(segment.from.x + 1, centered_span_start(slot, label_height)),
            point(0, 0),
        );
    }

    let center = shift_point(
        point_on_segment(segment, midpoint(segment_span(segment))),
        Direction::Right,
        1,
    );
    clamp_point(shift_point(center, Direction::Up, (label_height - 1).div_euclid(2)), point(0, 0))
}

fn best_label_segment(
    points: &[Point],
    label_width: i64,
    preferred_axis: Option<Axis>,
    preferred_segment: Option<usize>,
) -> Option<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for window in points.windows(2) {
        if let Some(segment) = segment_between(window[0], window[1]) {
            segments.push(segment);
        }
    }
    if let Some(preferred) = preferred_segment.filter(|index| *index < segments.len()) {
        return Some(segments[preferred]);
    }
    if let Some(preferred) = preferred_axis {
        if let Some(segment) = segments.iter().find(|segment| segment.axis == preferred) {
            return Some(*segment);
        }
    }

    let mut roomy_horizontal: Option<Segment> = None;
    let mut vertical_bus: Option<Segment> = None;
    let mut longest: Option<Segment> = None;

    for segment in &segments {
        if roomy_horizontal.is_none()
            && segment.axis == Axis::X
            && inline_label_slot(*segment, label_width).1
        {
            roomy_horizontal = Some(*segment);
        }
        if vertical_bus.is_none() && segment.axis == Axis::Y {
            vertical_bus = Some(*segment);
        }
        if longest.is_none_or(|current| segment.length > current.length) {
            longest = Some(*segment);
        }
    }

    roomy_horizontal.or(vertical_bus).or(longest)
}

fn flowchart_label_point(
    points: &[Point],
    label_width: i64,
    label_height: i64,
    preferred_axis: Option<Axis>,
    preferred_segment: Option<usize>,
) -> Point {
    match best_label_segment(points, label_width, preferred_axis, preferred_segment) {
        Some(segment) => segment_label_point(segment, label_width, label_height),
        None => points.first().copied().unwrap_or_else(|| point(0, 0)),
    }
}

/// `flowchartEdgeLabelLayout`.
#[must_use]
pub fn flowchart_edge_label_layout(
    points: &[Point],
    label: &str,
    preferred_axis: Option<Axis>,
    preferred_segment: Option<usize>,
) -> EdgeLabelLayout {
    let lines: Vec<String> =
        split_diagram_lines(label).into_iter().map(|line| flowchart_label_text(&line)).collect();
    let width = flowchart_label_width(label);
    let height = lines.len() as i64;
    EdgeLabelLayout {
        lines,
        point: flowchart_label_point(points, width, height, preferred_axis, preferred_segment),
        width,
        height,
    }
}

/// `flowchartRouteLabelLayout` — the frozen `label_point` wins.
#[must_use]
pub fn flowchart_route_label_layout(
    route_label: Option<Point>,
    points: &[Point],
    label: &str,
    label_axis: Option<Axis>,
) -> EdgeLabelLayout {
    let lines: Vec<String> =
        split_diagram_lines(label).into_iter().map(|line| flowchart_label_text(&line)).collect();
    let width = flowchart_label_width(label);
    let height = lines.len() as i64;
    EdgeLabelLayout {
        lines,
        point: route_label
            .unwrap_or_else(|| flowchart_label_point(points, width, height, label_axis, None)),
        width,
        height,
    }
}

/// The measure used by `avoidLabelOverlap`'s inline-width math.
#[must_use]
pub fn label_inner_width(line: &str) -> i64 {
    (diagram_text_width(line) as i64).saturating_sub(2)
}

/// Re-exported so the layout module shares the measure.
pub use crate::render::mermaid::core::geometry::Direction as LabelDirection;

#[allow(dead_code)]
fn _side_unused(side: Side) -> Side {
    let _ = side;
    side
}
