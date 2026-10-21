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

//! `sequence/drawing.rs` — the grid painter.

use crate::render::mermaid::core::canvas::{DiagramCanvas, TextOptions};
use crate::render::mermaid::core::drawing::BORDER_ROUNDED;
use crate::render::mermaid::core::text::diagram_text_width;
use crate::render::mermaid::sequence::placement::{create_sequence_placement_plan, StepPlacement};
use crate::render::mermaid::sequence::types::{
    FragmentKind, SequenceArrowHead, SequenceDiagram, SequenceRenderOptions, SequenceStyle,
};

const BORDER: crate::render::mermaid::core::drawing::BorderCharacters = BORDER_ROUNDED;

fn centered_start(center: i64, text: &str) -> i64 {
    center - diagram_text_width(text) as i64 / 2
}

fn arrow_head_char(head: Option<SequenceArrowHead>, direction: i64) -> &'static str {
    match head {
        Some(SequenceArrowHead::Open) => {
            if direction == 1 {
                ">"
            } else {
                "<"
            }
        }
        Some(SequenceArrowHead::Cross) => "✕",
        Some(SequenceArrowHead::Async) => {
            if direction == 1 {
                ")"
            } else {
                "("
            }
        }
        None => {
            if direction == 1 {
                "►"
            } else {
                "◄"
            }
        }
    }
}

fn create_grid(
    width: i64,
    height: i64,
) -> Result<DiagramCanvas<SequenceStyle>, crate::render::mermaid::core::canvas::CanvasSizeError> {
    DiagramCanvas::new(width, height, None)
}

fn set_cell(
    grid: &mut DiagramCanvas<SequenceStyle>,
    x: i64,
    y: i64,
    char: &str,
    style: Option<SequenceStyle>,
) {
    grid.set_cell(x, y, char, style);
}

fn set_text(
    grid: &mut DiagramCanvas<SequenceStyle>,
    x: i64,
    y: i64,
    text: &str,
    style: Option<SequenceStyle>,
) {
    grid.set_text(x.max(0), y, text, style);
}

fn set_arrow_departure_fade(
    grid: &mut DiagramCanvas<SequenceStyle>,
    x: i64,
    y: i64,
    direction: i64,
    style: SequenceStyle,
) {
    let fade = |step: u8| match style {
        SequenceStyle::Request => SequenceStyle::RequestFade(step),
        _ => SequenceStyle::ResponseFade(step),
    };
    set_cell(
        grid,
        x,
        y,
        if direction == 1 { BORDER.left_t } else { BORDER.right_t },
        Some(fade(1)),
    );
    for step in 2..=5u8 {
        set_cell(grid, x + direction * (step as i64 - 1), y, BORDER.horizontal, Some(fade(step)));
    }
}

fn group_vertical_char(existing: Option<&str>) -> Option<&'static str> {
    match existing {
        None | Some(" " | "│") => Some(BORDER.vertical),
        _ => None,
    }
}

fn set_group_vertical_cell(grid: &mut DiagramCanvas<SequenceStyle>, x: i64, y: i64) {
    let existing = grid.get_cell(x, y).map(|cell| char_of(cell.text()));
    if let Some(char) = group_vertical_char(existing) {
        set_cell(grid, x, y, char, Some(SequenceStyle::Group));
    }
}

/// Owned cells (entity-decoded labels) need a comparable view.
fn char_of(text: &str) -> &str {
    text
}

fn render_participant_groups(
    grid: &mut DiagramCanvas<SequenceStyle>,
    group_bounds: &[crate::render::mermaid::sequence::placement::GroupPlacement],
    bottom_y: i64,
) {
    for bounds in group_bounds {
        for x in bounds.left_x..=bounds.right_x {
            set_cell(grid, x, 0, BORDER.horizontal, Some(SequenceStyle::Group));
            set_cell(grid, x, bottom_y, BORDER.horizontal, Some(SequenceStyle::Group));
        }
        set_cell(grid, bounds.left_x, 0, BORDER.top_left, Some(SequenceStyle::Group));
        set_cell(grid, bounds.right_x, 0, BORDER.top_right, Some(SequenceStyle::Group));
        set_cell(grid, bounds.left_x, bottom_y, BORDER.bottom_left, Some(SequenceStyle::Group));
        set_cell(grid, bounds.right_x, bottom_y, BORDER.bottom_right, Some(SequenceStyle::Group));
        for y in 1..bottom_y {
            set_group_vertical_cell(grid, bounds.left_x, y);
            set_group_vertical_cell(grid, bounds.right_x, y);
        }
        if !bounds.label_text.is_empty() {
            set_text(grid, bounds.left_x + 2, 0, &bounds.label_text, Some(SequenceStyle::Group));
        }
    }
}

fn draw_fragment_walls(
    grid: &mut DiagramCanvas<SequenceStyle>,
    bounds: crate::render::mermaid::sequence::placement::HorizontalBounds,
    start_y: i64,
    end_y: i64,
) {
    if end_y < start_y {
        return;
    }
    for y in start_y..=end_y {
        set_cell(grid, bounds.left_x, y, BORDER.vertical, Some(SequenceStyle::Fragment));
        set_cell(grid, bounds.right_x, y, BORDER.vertical, Some(SequenceStyle::Fragment));
    }
}

fn render_fragment(
    grid: &mut DiagramCanvas<SequenceStyle>,
    kind: FragmentKind,
    label: &str,
    bounds: crate::render::mermaid::sequence::placement::HorizontalBounds,
    y: i64,
) {
    let (left_char, right_char) = match kind {
        FragmentKind::Alt | FragmentKind::Loop => (BORDER.top_left, BORDER.top_right),
        FragmentKind::Else => (BORDER.left_t, BORDER.right_t),
        FragmentKind::End => (BORDER.bottom_left, BORDER.bottom_right),
    };
    for x in bounds.left_x..=bounds.right_x {
        set_cell(grid, x, y, BORDER.horizontal, Some(SequenceStyle::Fragment));
    }
    set_cell(grid, bounds.left_x, y, left_char, Some(SequenceStyle::Fragment));
    set_cell(grid, bounds.right_x, y, right_char, Some(SequenceStyle::Fragment));
    if !label.is_empty() {
        set_text(grid, bounds.left_x + 2, y, label, Some(SequenceStyle::FragmentLabel));
    }
}

fn render_self_message(
    grid: &mut DiagramCanvas<SequenceStyle>,
    _placement: &StepPlacement,
    label_lines: &[String],
    center_x: i64,
    right_x: i64,
    top_y: i64,
    bottom_y: i64,
    message: &crate::render::mermaid::sequence::types::SequenceMessage,
    style: SequenceStyle,
) {
    set_arrow_departure_fade(grid, center_x, top_y, 1, style);
    for x in (center_x + 5)..right_x {
        set_cell(grid, x, top_y, BORDER.horizontal, Some(style));
    }
    set_cell(grid, right_x, top_y, BORDER.top_right, Some(style));

    for (line_index, line) in label_lines.iter().enumerate() {
        let y = top_y + line_index as i64 + 1;
        set_cell(grid, center_x, y, BORDER.vertical, Some(SequenceStyle::Lifeline));
        set_text(grid, center_x + 2, y, line, Some(style));
        set_cell(grid, right_x, y, BORDER.vertical, Some(style));
    }

    for x in (center_x + 1)..right_x {
        set_cell(grid, x, bottom_y, BORDER.horizontal, Some(style));
    }
    let head_x = if message.head.is_none() { center_x } else { center_x + 1 };
    set_cell(grid, head_x, bottom_y, arrow_head_char(message.head, -1), Some(style));
    set_cell(grid, right_x, bottom_y, BORDER.bottom_right, Some(style));
}

fn render_note(
    grid: &mut DiagramCanvas<SequenceStyle>,
    text_lines: &[String],
    text_x: i64,
    text_y: i64,
) {
    for (index, line) in text_lines.iter().enumerate() {
        set_text(grid, text_x, text_y + index as i64, line, Some(SequenceStyle::NoteBadge));
    }
}

/// `drawSequenceDiagramGrid`.
#[allow(clippy::too_many_lines)]
pub fn draw_sequence_diagram_grid(
    diagram: &SequenceDiagram,
    options: &SequenceRenderOptions,
) -> Result<DiagramCanvas<SequenceStyle>, crate::render::mermaid::core::canvas::CanvasSizeError> {
    let plan = create_sequence_placement_plan(diagram, options);
    if plan.width == 0 || plan.height == 0 {
        return create_grid(0, 0);
    }
    let mut grid = create_grid(plan.width, plan.height)?;

    if !plan.groups.is_empty() {
        render_participant_groups(&mut grid, &plan.groups, plan.height - 1);
    }

    for placement in &plan.participants {
        let center = placement.center_x;
        let rows = plan.rows;
        if options.compact {
            for (index, line) in placement.label_lines.iter().enumerate() {
                set_text(
                    &mut grid,
                    centered_start(center, line),
                    rows.participant_header_y + index as i64,
                    line,
                    Some(SequenceStyle::Participant),
                );
            }
        } else {
            for (index, line) in placement.label_lines.iter().enumerate() {
                set_text(
                    &mut grid,
                    centered_start(center, line),
                    rows.participant_header_top_y + index as i64,
                    line,
                    Some(SequenceStyle::Participant),
                );
            }
            for x in placement.header_left_x..=placement.header_right_x {
                set_cell(
                    &mut grid,
                    x,
                    rows.participant_rule_y,
                    BORDER.horizontal,
                    Some(SequenceStyle::Participant),
                );
            }
            set_cell(
                &mut grid,
                center,
                rows.participant_rule_y,
                BORDER.top_t,
                Some(SequenceStyle::Participant),
            );
        }

        for y in rows.lifeline_start_y..=rows.lifeline_end_y {
            let distance = y - rows.lifeline_start_y;
            let style = if !options.compact && distance < 3 {
                SequenceStyle::LifelineRamp((distance + 1) as u8)
            } else {
                SequenceStyle::Lifeline
            };
            set_cell(&mut grid, center, y, BORDER.vertical, Some(style));
        }
    }

    for placement in &plan.steps {
        match placement {
            StepPlacement::Note { text_lines, text_x, text_y } => {
                render_note(&mut grid, text_lines, *text_x, *text_y);
            }
            StepPlacement::Fragment { kind, label_text, bounds, y, walls_before } => {
                if let Some(walls) = walls_before {
                    draw_fragment_walls(&mut grid, walls.bounds, walls.start_y, walls.end_y);
                }
                render_fragment(&mut grid, *kind, label_text, *bounds, *y);
            }
            StepPlacement::SelfMessage {
                message,
                label_lines,
                center_x,
                right_x,
                top_y,
                bottom_y,
            } => {
                let style =
                    if message.dashed { SequenceStyle::Response } else { SequenceStyle::Request };
                render_self_message(
                    &mut grid,
                    placement,
                    label_lines,
                    *center_x,
                    *right_x,
                    *top_y,
                    *bottom_y,
                    message,
                    style,
                );
            }
            StepPlacement::Message {
                message,
                label_lines,
                label_x,
                label_y,
                arrow_y,
                from_x,
                left_x,
                right_x,
                head_x,
                direction,
                inline_label,
                ..
            } => {
                let style =
                    if message.dashed { SequenceStyle::Response } else { SequenceStyle::Request };
                if inline_label.is_none() {
                    for (line_index, line) in label_lines.iter().enumerate() {
                        set_text(
                            &mut grid,
                            *label_x,
                            label_y + line_index as i64,
                            line,
                            Some(style),
                        );
                    }
                }
                for x in (left_x + 1)..*right_x {
                    set_cell(&mut grid, x, *arrow_y, BORDER.horizontal, Some(style));
                }
                set_arrow_departure_fade(&mut grid, *from_x, *arrow_y, *direction, style);
                set_cell(
                    &mut grid,
                    *head_x,
                    *arrow_y,
                    arrow_head_char(message.head, *direction),
                    Some(style),
                );
                if let Some(label) = inline_label {
                    set_text(&mut grid, *label_x, *label_y, label, Some(style));
                }
            }
        }
    }

    for activation in &plan.activations {
        for y in activation.start_y..=activation.end_y {
            if grid
                .get_cell(activation.center_x, y)
                .is_some_and(|cell| cell.text() == BORDER.vertical)
            {
                set_cell(&mut grid, activation.center_x, y, "┃", Some(SequenceStyle::Lifeline));
            }
        }
    }
    let _ = TextOptions::default();
    Ok(grid)
}
