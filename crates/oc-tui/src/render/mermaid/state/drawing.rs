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

//! `state/drawing.ts` — the grid painter: composite frames, state
//! boxes, transition routes with departure ramps, junctions, and notes.

use crate::render::mermaid::core::canvas::{DiagramCanvas, TextOptions};
use crate::render::mermaid::core::drawing::{
    draw_diagram_frame, fill_diagram_frame_interior, ArrowHeadStyle, BORDER_DOUBLE, BORDER_ROUNDED,
};
use crate::render::mermaid::core::geometry::{direction_between, orthogonal_path_points};
use crate::render::mermaid::core::text::{diagram_text_width, DIAGRAM_LABEL_PADDING_X};
use crate::render::mermaid::state::endpoint::{
    normalize_state_min_state_gap, DEFAULT_STATE_ARROW_HEAD_STYLE, DEFAULT_STATE_BORDER_STYLE,
};
use crate::render::mermaid::state::layout::{
    expand_composite_bounds_for_feedback, expand_composite_bounds_for_internal_transitions,
    placed_note_connector, separate_external_bounds_from_composites,
    translate_state_diagram_layout, StateDiagramBoxBounds, StateDiagramLayout,
    StateDiagramNoteBounds,
};
use crate::render::mermaid::state::routing::{
    create_state_transition_junction_plans, create_state_transition_render_plans,
    measure_state_transition_label, merge_state_cells, render_cell_glyph,
    StateTransitionRenderOptions, StateTransitionRenderPlan,
};
use crate::render::mermaid::state::search::create_state_search_budget;
use crate::render::mermaid::state::types::{
    NotePosition, StateCellStyle, StateDiagram, StateDiagramRenderOptions, StateKind,
    StateRenderError,
};
use crate::render::mermaid::state::visible_model::{
    is_hidden_composite_marker, prepare_visible_state_diagram,
};

/// `StateGrid`.
pub type StateGrid = DiagramCanvas<StateCellStyle>;

fn border_chars(
    style: crate::render::mermaid::state::types::StateBorderStyle,
) -> &'static crate::render::mermaid::core::drawing::BorderCharacters {
    match style {
        crate::render::mermaid::state::types::StateBorderStyle::Rounded => &BORDER_ROUNDED,
        crate::render::mermaid::state::types::StateBorderStyle::Double => &BORDER_DOUBLE,
    }
}

/// `translateTransitionPlans`.
fn translate_transition_plans(
    plans: &[StateTransitionRenderPlan],
    dx: i64,
    dy: i64,
) -> Vec<StateTransitionRenderPlan> {
    plans
        .iter()
        .map(|plan| StateTransitionRenderPlan {
            route: plan.route.clone(),
            cells: plan
                .cells
                .iter()
                .map(|cell| crate::render::mermaid::state::routing::StateTransitionRenderCell {
                    x: cell.x + dx,
                    y: cell.y + dy,
                    payload: cell.payload,
                })
                .collect(),
            path: plan.path.iter().map(|&(x, y)| (x + dx, y + dy)).collect(),
            label: plan.label.as_ref().map(|label| {
                crate::render::mermaid::state::routing::StateTransitionRenderLabel {
                    x: label.x + dx,
                    y: label.y + dy,
                    lines: label.lines.clone(),
                }
            }),
            path_repaired: plan.path_repaired,
        })
        .collect()
}

/// `makeGrid`.
fn make_grid(width: i64, height: i64) -> Result<StateGrid, StateRenderError> {
    DiagramCanvas::new(width, height, Some(merge_state_cells)).map_err(StateRenderError::from)
}

/// `drawBox`.
fn draw_box(
    grid: &mut StateGrid,
    state: &crate::render::mermaid::state::types::StateDiagramState,
    bounds: &StateDiagramBoxBounds,
    lines: &[String],
    border_style: crate::render::mermaid::state::types::StateBorderStyle,
) {
    if is_hidden_composite_marker(Some(state)) {
        return;
    }

    if state.kind != StateKind::State {
        grid.set_cell(bounds.left, bounds.top, &state.label, Some(state_kind_style(state.kind)));
        return;
    }
    fill_diagram_frame_interior(bounds_as_rect(bounds), &mut |x, y| {
        grid.set_cell(x, y, " ", Some(StateCellStyle::State));
    });
    draw_state_frame(grid, bounds, border_chars(border_style), StateCellStyle::StateBorder);
    for (index, line) in lines.iter().enumerate() {
        grid.set_text(
            bounds.left + 2,
            bounds.top + 1 + index as i64,
            line,
            Some(StateCellStyle::State),
        );
    }
}

fn state_kind_style(kind: StateKind) -> StateCellStyle {
    match kind {
        StateKind::Start => StateCellStyle::Start,
        StateKind::End => StateCellStyle::End,
        StateKind::Choice => StateCellStyle::Choice,
        StateKind::State => StateCellStyle::State,
    }
}

/// `drawStateFrame`.
fn draw_state_frame(
    grid: &mut StateGrid,
    bounds: &StateDiagramBoxBounds,
    chars: &crate::render::mermaid::core::drawing::BorderCharacters,
    style: StateCellStyle,
) {
    draw_diagram_frame(bounds_as_rect(bounds), chars, &mut |x, y, char| {
        grid.set_cell(x, y, char, Some(style));
    });
}

/// `drawContainerFrame`.
fn draw_container_frame(
    grid: &mut StateGrid,
    bounds: &StateDiagramBoxBounds,
    label: &str,
    chars: &crate::render::mermaid::core::drawing::BorderCharacters,
    style: StateCellStyle,
) {
    draw_diagram_frame(bounds_as_rect(bounds), chars, &mut |x, y, char| {
        grid.set_cell(x, y, char, Some(style));
    });
    draw_container_label(grid, bounds, label, style);
}

/// `drawContainerLabel`.
fn draw_container_label(
    grid: &mut StateGrid,
    bounds: &StateDiagramBoxBounds,
    label: &str,
    style: StateCellStyle,
) {
    if !label.is_empty() {
        grid.set_text(bounds.left + 2, bounds.top, &format!(" {label} "), Some(style));
    }
}

/// `noteConnectorGlyph`.
fn note_connector_glyph(
    directions: &std::collections::HashSet<crate::render::mermaid::core::geometry::Direction>,
) -> &'static str {
    let chars = &BORDER_DOUBLE;
    use crate::render::mermaid::core::geometry::Direction::{Down, Left, Right, Up};
    let up = directions.contains(&Up);
    let down = directions.contains(&Down);
    let left = directions.contains(&Left);
    let right = directions.contains(&Right);
    if up && down && left && right {
        return chars.cross;
    }
    if up && down && right {
        return chars.left_t;
    }
    if up && down && left {
        return chars.right_t;
    }
    if left && right && down {
        return chars.top_t;
    }
    if left && right && up {
        return chars.bottom_t;
    }
    if up && right {
        return chars.bottom_left;
    }
    if up && left {
        return chars.bottom_right;
    }
    if down && right {
        return chars.top_left;
    }
    if down && left {
        return chars.top_right;
    }
    if up || down {
        return chars.vertical;
    }
    chars.horizontal
}

/// `drawNote`.
fn draw_note(
    grid: &mut StateGrid,
    bounds: &StateDiagramNoteBounds,
    target: &StateDiagramBoxBounds,
) {
    let chars = &BORDER_DOUBLE;
    let connector = placed_note_connector(bounds, target);
    let points = orthogonal_path_points(&connector.points);
    for (index, point) in points.iter().enumerate() {
        let mut directions: std::collections::HashSet<
            crate::render::mermaid::core::geometry::Direction,
        > = std::collections::HashSet::new();
        if index > 0 {
            if let Some(direction) = direction_between(*point, points[index - 1]) {
                directions.insert(direction);
            }
        }
        if let Some(next) = points.get(index + 1) {
            if let Some(direction) = direction_between(*point, *next) {
                directions.insert(direction);
            }
        }
        let distance_from_note = points.len() as i64 - index as i64 - 1;
        let style = if distance_from_note < 3 {
            match distance_from_note {
                0 => StateCellStyle::NoteConnectorRamp3,
                1 => StateCellStyle::NoteConnectorRamp2,
                _ => StateCellStyle::NoteConnectorRamp1,
            }
        } else {
            StateCellStyle::NoteConnector
        };
        grid.set_cell(point.x, point.y, note_connector_glyph(&directions), Some(style));
    }

    draw_container_frame(grid, &bounds.bound, "", chars, StateCellStyle::NoteBorder);
    grid.set_cell(
        if bounds.note.position == NotePosition::Right {
            bounds.bound.left
        } else {
            bounds.bound.left + bounds.bound.width - 1
        },
        connector.connector_y,
        if bounds.note.position == NotePosition::Right {
            BORDER_DOUBLE.right_t
        } else {
            BORDER_DOUBLE.left_t
        },
        Some(StateCellStyle::NoteBorder),
    );
    for (index, line) in bounds.lines.iter().enumerate() {
        grid.set_text(
            bounds.bound.left + 2,
            bounds.bound.top + 1 + index as i64,
            line,
            Some(StateCellStyle::NoteText),
        );
    }
}

/// `drawTransitionRenderPlan`.
fn draw_transition_render_plan(
    grid: &mut StateGrid,
    plan: &StateTransitionRenderPlan,
    arrow_head_style: ArrowHeadStyle,
    ramp_departure: bool,
) {
    let mut departure: std::collections::HashMap<(i64, i64), StateCellStyle> =
        std::collections::HashMap::new();
    if ramp_departure {
        for (index, &(x, y)) in plan.path.iter().take(3).enumerate() {
            departure.insert(
                (x, y),
                [
                    StateCellStyle::StateDepartureRamp1,
                    StateCellStyle::StateDepartureRamp2,
                    StateCellStyle::StateDepartureRamp3,
                ][index],
            );
        }
    }
    for cell in &plan.cells {
        let char = render_cell_glyph(cell, arrow_head_style);
        let style = departure.get(&(cell.x, cell.y)).copied().unwrap_or(StateCellStyle::Transition);
        grid.set_cell(cell.x, cell.y, char, Some(style));
    }
    if let Some(label) = &plan.label {
        for (index, line) in label.lines.iter().enumerate() {
            let y = label.y + index as i64;
            let left_x = label.x - DIAGRAM_LABEL_PADDING_X as i64;
            if grid.get_cell(left_x, y).is_some_and(|cell| cell.text() == " ") {
                grid.set_cell(left_x, y, " ", Some(StateCellStyle::Label));
            }
            grid.set_text(label.x, y, line, Some(StateCellStyle::Label));
            let right_x =
                label.x + diagram_text_width(line) as i64 + DIAGRAM_LABEL_PADDING_X as i64 - 1;
            if grid.get_cell(right_x, y).is_some_and(|cell| cell.text() == " ") {
                grid.set_cell(right_x, y, " ", Some(StateCellStyle::Label));
            }
        }
    }
}

/// `drawTransitionJunctionPlans`.
fn draw_transition_junction_plans(
    grid: &mut StateGrid,
    diagram: &StateDiagram,
    layout: &StateDiagramLayout,
    render_plans: &[StateTransitionRenderPlan],
) {
    for plan in create_state_transition_junction_plans(diagram, layout, render_plans) {
        let style = if plan.kind == crate::render::mermaid::state::routing::JunctionKind::Choice {
            StateCellStyle::Choice
        } else {
            StateCellStyle::Transition
        };
        let char = if plan.kind == crate::render::mermaid::state::routing::JunctionKind::Choice {
            "◆"
        } else {
            crate::render::mermaid::core::drawing::diagram_line_glyph(
                &plan.connections,
                crate::render::mermaid::core::drawing::CornerStyle::Rounded,
                crate::render::mermaid::core::drawing::LineStyle::Single,
            )
        };
        grid.set_cell(plan.bounds.left, plan.bounds.top, char, Some(style));
    }
}

fn bounds_as_rect(bound: &StateDiagramBoxBounds) -> crate::render::mermaid::core::geometry::Bounds {
    crate::render::mermaid::core::geometry::bounds_from_rect(
        bound.left,
        bound.top,
        bound.width,
        bound.height,
    )
}

/// `createStateDiagramDrawing`'s return payload.
pub struct StateDiagramDrawing {
    pub grid: StateGrid,
    pub diagram: StateDiagram,
    pub layout: StateDiagramLayout,
    pub transition_plans: Vec<StateTransitionRenderPlan>,
}

/// `drawStateDiagramGrid`.
pub fn draw_state_diagram_grid(
    source_diagram: &StateDiagram,
    options: &StateDiagramRenderOptions,
) -> Result<StateGrid, StateRenderError> {
    Ok(create_state_diagram_drawing(source_diagram, options)?.grid)
}

/// `createStateDiagramDrawing` — horizontal layouts fold vertically
/// when the width target would overflow the canvas budget.
pub fn create_state_diagram_drawing(
    source_diagram: &StateDiagram,
    options: &StateDiagramRenderOptions,
) -> Result<StateDiagramDrawing, StateRenderError> {
    let direction = options.direction.unwrap_or(source_diagram.direction);
    if direction != crate::render::mermaid::state::types::StateDirection::Lr
        && direction != crate::render::mermaid::state::types::StateDirection::Rl
    {
        return create_state_diagram_drawing_with_direction(source_diagram, options, direction);
    }
    let Some(layout_max_width_input) = options.layout_max_width else {
        return create_state_diagram_drawing_with_direction(source_diagram, options, direction);
    };
    let fallback_direction =
        if direction == crate::render::mermaid::state::types::StateDirection::Rl {
            crate::render::mermaid::state::types::StateDirection::Bt
        } else {
            crate::render::mermaid::state::types::StateDirection::Tb
        };
    let max_width = 1.max(layout_max_width_input);
    let drawing = create_state_diagram_drawing_with_direction(source_diagram, options, direction);
    let drawing = match drawing {
        Ok(drawing) => Some(drawing),
        Err(StateRenderError::CanvasSize(_)) => None,
        Err(error) => return Err(error),
    };
    let Some(drawing) = drawing else {
        return create_state_diagram_drawing_with_direction(
            source_diagram,
            options,
            fallback_direction,
        );
    };
    if drawing.grid.get_text_size(TextOptions { trim_top: true, trim_bottom: true }).width as i64
        <= max_width
    {
        return Ok(drawing);
    }
    let fallback =
        create_state_diagram_drawing_with_direction(source_diagram, options, fallback_direction)?;
    if fallback.grid.get_text_size(TextOptions { trim_top: true, trim_bottom: true }).width
        >= drawing.grid.get_text_size(TextOptions { trim_top: true, trim_bottom: true }).width
    {
        return Ok(drawing);
    }
    Ok(fallback)
}

/// `createStateDiagramDrawingWithDirection`.
fn create_state_diagram_drawing_with_direction(
    source_diagram: &StateDiagram,
    options: &StateDiagramRenderOptions,
    direction: crate::render::mermaid::state::types::StateDirection,
) -> Result<StateDiagramDrawing, StateRenderError> {
    let mut diagram = if direction == source_diagram.direction {
        source_diagram.clone()
    } else {
        StateDiagram { direction, ..source_diagram.clone() }
    };
    diagram = prepare_visible_state_diagram(&diagram);
    let border_style = options.border_style.unwrap_or(DEFAULT_STATE_BORDER_STYLE);
    let arrow_head_style = options.arrow_head_style.unwrap_or(DEFAULT_STATE_ARROW_HEAD_STYLE);
    let min_state_gap = normalize_state_min_state_gap(options.min_state_gap);
    let mut search_budget = create_state_search_budget();
    let mut layout = crate::render::mermaid::state::layout::create_state_diagram_layout(
        &diagram,
        &crate::render::mermaid::state::layout::StateDiagramLayoutOptions {
            min_state_gap,
            search_budget: None,
        },
    )?;

    let all_bounds_max_y = layout
        .boxes
        .iter()
        .map(|bound| bound.top + bound.height)
        .chain(layout.note_bounds.iter().map(|bound| bound.bound.top + bound.bound.height))
        .max()
        .unwrap_or(0)
        .max(0);
    let feedback_lane_y = all_bounds_max_y + 3;
    let feedback_top_y = layout
        .boxes
        .iter()
        .map(|bound| bound.top)
        .chain(layout.note_bounds.iter().map(|bound| bound.bound.top))
        .min()
        .unwrap_or(0)
        .min(0)
        - 3;
    expand_composite_bounds_for_feedback(&diagram, &mut layout, feedback_lane_y);
    let mut transition_plans: Vec<StateTransitionRenderPlan> = Vec::new();
    let separation_attempts = diagram.states.len() + diagram.composites.len() + 1;
    for attempt in 0..separation_attempts {
        transition_plans = create_state_transition_render_plans(
            &diagram,
            &layout,
            feedback_lane_y,
            StateTransitionRenderOptions {
                feedback_top_y: Some(feedback_top_y),
                note_bounds: &layout.note_bounds,
                repair_routes: None,
            },
            &mut search_budget,
        )?;
        expand_composite_bounds_for_internal_transitions(&diagram, &mut layout, &transition_plans);
        if !separate_external_bounds_from_composites(&diagram, &mut layout) {
            break;
        }
        if attempt == separation_attempts - 1 {
            return Err(StateRenderError::Layout(
                "State composite separation did not converge".to_string(),
            ));
        }
    }
    let content_left = layout
        .boxes
        .iter()
        .map(|bound| bound.left)
        .chain(layout.note_bounds.iter().map(|bound| bound.bound.left))
        .chain(
            layout
                .note_bounds
                .iter()
                .flat_map(|bound| {
                    bound.connector.iter().flat_map(|connector| connector.points.iter())
                })
                .map(|point| point.x),
        )
        .chain(transition_plans.iter().flat_map(|plan| {
            plan.cells
                .iter()
                .map(|cell| cell.x)
                .chain(plan.label.iter().map(|label| label.x - DIAGRAM_LABEL_PADDING_X as i64))
        }))
        .min()
        .unwrap_or(0)
        .min(0);
    let content_top = layout
        .boxes
        .iter()
        .map(|bound| bound.top)
        .chain(layout.note_bounds.iter().map(|bound| bound.bound.top))
        .chain(
            layout
                .note_bounds
                .iter()
                .flat_map(|bound| {
                    bound.connector.iter().flat_map(|connector| connector.points.iter())
                })
                .map(|point| point.y),
        )
        .chain(transition_plans.iter().flat_map(|plan| {
            plan.cells.iter().map(|cell| cell.y).chain(plan.label.iter().map(|label| label.y))
        }))
        .min()
        .unwrap_or(0)
        .min(0);
    if content_left < 0 || content_top < 0 {
        translate_state_diagram_layout(&mut layout, -content_left, -content_top);
        transition_plans =
            translate_transition_plans(&transition_plans, -content_left, -content_top);
    }
    let translated_max_x = layout
        .boxes
        .iter()
        .map(|bound| bound.left + bound.width)
        .chain(layout.note_bounds.iter().map(|bound| bound.bound.left + bound.bound.width))
        .chain(
            layout
                .note_bounds
                .iter()
                .flat_map(|bound| {
                    bound.connector.iter().flat_map(|connector| connector.points.iter())
                })
                .map(|point| point.x + 1),
        )
        .max()
        .unwrap_or(0)
        .max(0);
    let translated_max_y = layout
        .boxes
        .iter()
        .map(|bound| bound.top + bound.height)
        .chain(layout.note_bounds.iter().map(|bound| bound.bound.top + bound.bound.height))
        .chain(
            layout
                .note_bounds
                .iter()
                .flat_map(|bound| {
                    bound.connector.iter().flat_map(|connector| connector.points.iter())
                })
                .map(|point| point.y + 1),
        )
        .max()
        .unwrap_or(0)
        .max(0);
    let transition_label_sizes: Vec<_> = diagram
        .transitions
        .iter()
        .map(|transition| measure_state_transition_label(&transition.label))
        .collect();
    let max_transition_label_width =
        transition_label_sizes.iter().map(|size| size.width).max().unwrap_or(0).max(0);
    let max_transition_label_lines =
        transition_label_sizes.iter().map(|size| size.height).max().unwrap_or(0).max(0);
    let transition_right = translated_max_x.max(
        transition_plans
            .iter()
            .flat_map(|plan| {
                plan.cells.iter().map(|cell| cell.x + 1).chain(plan.label.iter().map(|label| {
                    label.x
                        + measure_state_transition_label(
                            &diagram.transitions[plan.route.transition_index].label,
                        )
                        .width
                        + DIAGRAM_LABEL_PADDING_X as i64
                }))
            })
            .max()
            .unwrap_or(0),
    );
    let transition_bottom = translated_max_y.max(
        transition_plans
            .iter()
            .flat_map(|plan| {
                plan.cells
                    .iter()
                    .map(|cell| cell.y + 1)
                    .chain(plan.label.iter().map(|label| label.y + label.lines.len() as i64))
            })
            .max()
            .unwrap_or(0),
    );
    let grid = make_grid(
        (translated_max_x + 24.max(max_transition_label_width + 4)).max(transition_right + 2),
        (translated_max_y + 8 + max_transition_label_lines).max(transition_bottom + 2),
    )?;
    let mut grid = grid;

    for composite in &diagram.composites {
        if let Some(&bound_index) = layout.composite_bounds.get(composite.id.as_str()) {
            draw_container_frame(
                &mut grid,
                &layout.boxes[bound_index],
                &composite.label,
                border_chars(border_style),
                StateCellStyle::Composite,
            );
        }
    }

    for state in &diagram.states {
        let Some(bound_index) = layout.bounds.get(state.id.as_str()) else { continue };
        let Some(size) = layout.sizes.get(state.id.as_str()) else { continue };
        draw_box(&mut grid, state, &layout.boxes[*bound_index], &size.lines, border_style);
    }

    for plan in &transition_plans {
        let source_kind = diagram
            .states
            .iter()
            .find(|state| state.id == diagram.transitions[plan.route.transition_index].from)
            .map(|state| state.kind);
        draw_transition_render_plan(
            &mut grid,
            plan,
            arrow_head_style,
            source_kind == Some(StateKind::State),
        );
    }

    draw_transition_junction_plans(&mut grid, &diagram, &layout, &transition_plans);

    for composite in &diagram.composites {
        if let Some(&bound_index) = layout.composite_bounds.get(composite.id.as_str()) {
            draw_container_label(
                &mut grid,
                &layout.boxes[bound_index],
                &composite.label,
                StateCellStyle::CompositeLabel,
            );
        }
    }

    for note_bound in &layout.note_bounds {
        if let Some(&target_index) = layout.bounds.get(note_bound.note.target.as_str()) {
            draw_note(&mut grid, note_bound, &layout.boxes[target_index]);
        }
    }

    Ok(StateDiagramDrawing { grid, diagram, layout, transition_plans })
}
