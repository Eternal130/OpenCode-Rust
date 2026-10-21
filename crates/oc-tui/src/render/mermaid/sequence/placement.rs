//! `sequence/placement.ts` — participant centers, group frames,
//! fragment frames, and step placement.

use std::collections::HashMap;

use crate::render::mermaid::core::text::diagram_text_width;
use crate::render::mermaid::sequence::types::{
    FragmentKind, NotePosition, SequenceDiagram, SequenceMessage, SequenceParticipant,
    SequenceRenderOptions, SequenceStep, DEFAULT_MIN_PARTICIPANT_GAP,
};

const NOTE_HORIZONTAL_PADDING: i64 = 2;
const GROUP_HORIZONTAL_PADDING: i64 = 2;
const FRAGMENT_HORIZONTAL_OVERHANG: i64 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HorizontalBounds {
    pub left_x: i64,
    pub right_x: i64,
}

#[derive(Debug, Clone)]
pub struct ParticipantPlacement {
    pub participant: SequenceParticipant,
    pub center_x: i64,
    pub header_left_x: i64,
    pub header_right_x: i64,
    pub label_lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GroupPlacement {
    pub label_text: String,
    pub left_x: i64,
    pub right_x: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct WallPlacement {
    pub bounds: HorizontalBounds,
    pub start_y: i64,
    pub end_y: i64,
}

#[derive(Debug, Clone)]
pub struct ActivationPlacement {
    pub participant: String,
    pub center_x: i64,
    pub start_y: i64,
    pub end_y: i64,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub enum StepPlacement {
    Note {
        text_lines: Vec<String>,
        text_x: i64,
        text_y: i64,
    },
    Fragment {
        kind: FragmentKind,
        label_text: String,
        bounds: HorizontalBounds,
        y: i64,
        walls_before: Option<WallPlacement>,
    },
    Message {
        message: SequenceMessage,
        label_lines: Vec<String>,
        label_x: i64,
        label_y: i64,
        arrow_y: i64,
        from_x: i64,
        to_x: i64,
        left_x: i64,
        right_x: i64,
        head_x: i64,
        direction: i64,
        inline_label: Option<String>,
    },
    SelfMessage {
        message: SequenceMessage,
        label_lines: Vec<String>,
        center_x: i64,
        right_x: i64,
        top_y: i64,
        bottom_y: i64,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PlanRows {
    pub participant_header_top_y: i64,
    pub participant_header_y: i64,
    pub participant_rule_y: i64,
    pub lifeline_start_y: i64,
    pub lifeline_end_y: i64,
}

#[derive(Debug, Clone, Default)]
pub struct SequencePlacementPlan {
    pub width: i64,
    pub height: i64,
    pub rows: PlanRows,
    pub participants: Vec<ParticipantPlacement>,
    pub groups: Vec<GroupPlacement>,
    pub activations: Vec<ActivationPlacement>,
    pub steps: Vec<StepPlacement>,
}

fn visual_length(value: &str) -> i64 {
    diagram_text_width(value) as i64
}

/// `mermaidLabelLines` — `<br>` or `\n` splits, right-trimmed.
fn mermaid_label_lines(label: &str) -> Vec<String> {
    // /(?:<br\s*\/?\s*>|\\n)/i
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = label.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        let rest: String = chars[index..].iter().collect();
        let lower = rest.to_lowercase();
        if lower.starts_with("<br") {
            let after = &lower[3..];
            let mut offset = 0usize;
            let bytes = after.as_bytes();
            while offset < bytes.len() && (bytes[offset] as char).is_whitespace() {
                offset += 1;
            }
            if offset < bytes.len() && bytes[offset] == b'/' {
                offset += 1;
                while offset < bytes.len() && (bytes[offset] as char).is_whitespace() {
                    offset += 1;
                }
            }
            if offset < bytes.len() && bytes[offset] == b'>' {
                lines.push(std::mem::take(&mut current));
                index += 3 + offset + 1;
                continue;
            }
        }
        if chars[index] == '\\' && chars.get(index + 1) == Some(&'n') {
            lines.push(std::mem::take(&mut current));
            index += 2;
            continue;
        }
        current.push(chars[index]);
        index += 1;
    }
    lines.push(current);
    let trimmed: Vec<String> = lines.into_iter().map(|line| line.trim_end().to_string()).collect();
    if trimmed.is_empty() {
        vec![String::new()]
    } else {
        trimmed
    }
}

fn note_label_lines(label: &str) -> Vec<String> {
    let lines = mermaid_label_lines(label);
    let width = label_lines_width(&lines);
    lines
        .into_iter()
        .map(|line| {
            let pad = " ".repeat(NOTE_HORIZONTAL_PADDING as usize);
            format!(
                "{pad}{line}{}{pad}",
                " ".repeat((width - visual_length(&line)).max(0) as usize)
            )
        })
        .collect()
}

fn message_label_text(message: &SequenceMessage) -> String {
    match message.number {
        None => message.label.clone(),
        Some(number) => format!("{number}. {}", message.label),
    }
}

fn participant_header_width(label: &str, compact: bool) -> i64 {
    let width = label_lines_width(&mermaid_label_lines(label));
    if compact {
        width
    } else {
        3.max(width)
    }
}

fn fragment_label_text(kind: FragmentKind, label: &str) -> String {
    if kind == FragmentKind::End {
        return String::new();
    }
    let prefix = match kind {
        FragmentKind::Loop => "↻ loop",
        FragmentKind::Alt => "alt",
        FragmentKind::Else => "else",
        FragmentKind::End => "",
    };
    format!(" {prefix}: {label} ")
}

fn label_lines_width(lines: &[String]) -> i64 {
    lines.iter().map(|line| visual_length(line)).max().unwrap_or(0)
}

fn message_width(message: &SequenceMessage) -> i64 {
    label_lines_width(&mermaid_label_lines(&message_label_text(message)))
}

fn self_message_loop_width_for_lines(label_lines: &[String]) -> i64 {
    10.max(label_lines_width(label_lines) + 4)
}

fn arrow_head_x(
    to_x: i64,
    direction: i64,
    head: Option<crate::render::mermaid::sequence::types::SequenceArrowHead>,
) -> i64 {
    if head.is_none() {
        to_x
    } else {
        to_x - direction
    }
}

fn inline_message_label(
    message: &SequenceMessage,
    label_lines: &[String],
    from_x: i64,
    to_x: i64,
    compact: bool,
) -> Option<String> {
    if !compact || message.from == message.to || label_lines.len() != 1 {
        return None;
    }
    let label = format!(" {} ", label_lines[0]);
    (visual_length(&label) <= (to_x - from_x).abs() - 3).then_some(label)
}

#[allow(clippy::too_many_lines)]
fn get_step_height(
    step: &SequenceStep,
    centers: &[i64],
    participant_indexes: &HashMap<String, usize>,
    compact: bool,
) -> i64 {
    match step {
        SequenceStep::Note(note) => note_label_lines(&note.label).len() as i64 + 2,
        SequenceStep::Activation(_) => 0,
        SequenceStep::Fragment(_) => 2,
        SequenceStep::Message(message) => {
            let label_lines = mermaid_label_lines(&message_label_text(message));
            let from_x = participant_indexes
                .get(&message.from)
                .and_then(|index| centers.get(*index))
                .copied();
            let to_x =
                participant_indexes.get(&message.to).and_then(|index| centers.get(*index)).copied();
            if let (Some(from_x), Some(to_x)) = (from_x, to_x) {
                if inline_message_label(message, &label_lines, from_x, to_x, compact).is_some() {
                    return 2;
                }
            }
            label_lines.len() as i64 + if message.from == message.to { 3 } else { 2 }
        }
    }
}

fn get_participant_indexes(
    participant_indexes: &HashMap<String, usize>,
    participant_ids: &[String],
) -> Vec<usize> {
    participant_ids
        .iter()
        .map(|id| participant_indexes.get(id).copied().unwrap_or(usize::MAX))
        .filter(|index| *index != usize::MAX)
        .collect()
}

struct GroupRange {
    start_index: usize,
    end_index: usize,
}

fn get_group_ranges(
    diagram: &SequenceDiagram,
    participant_indexes: &HashMap<String, usize>,
) -> Vec<(usize, GroupRange)> {
    diagram
        .groups
        .iter()
        .enumerate()
        .filter_map(|(group_index, group)| {
            let indexes = get_participant_indexes(participant_indexes, &group.participant_ids);
            if indexes.is_empty() {
                return None;
            }
            Some((
                group_index,
                GroupRange {
                    start_index: *indexes.iter().min()?,
                    end_index: *indexes.iter().max()?,
                },
            ))
        })
        .collect()
}

fn get_step_content_bounds(
    step: &SequenceStep,
    centers: &[i64],
    participant_indexes: &HashMap<String, usize>,
) -> Option<HorizontalBounds> {
    if let SequenceStep::Message(message) = step {
        let from_index = participant_indexes.get(&message.from).copied()?;
        let to_index = participant_indexes.get(&message.to).copied()?;
        let from_x = *centers.get(from_index)?;
        let to_x = *centers.get(to_index)?;
        if from_index == to_index {
            return Some(HorizontalBounds {
                left_x: from_x,
                right_x: from_x + self_message_loop_width(message),
            });
        }
        let left_x = from_x.min(to_x);
        let right_x = from_x.max(to_x);
        let label_width = message_width(message);
        let label_left_x = left_x + 2;
        return Some(HorizontalBounds {
            left_x: left_x.min(label_left_x),
            right_x: right_x.max(label_left_x + label_width - 1),
        });
    }
    let SequenceStep::Note(note) = step else { return None };
    let indexes = get_participant_indexes(participant_indexes, &note.over);
    if indexes.is_empty() {
        return None;
    }
    let center_x =
        (centers[*indexes.iter().min()?] + centers[*indexes.iter().max()?]).div_euclid(2);
    let text_lines = note_label_lines(&note.label);
    let width = label_lines_width(&text_lines);
    let left_x = match note.position {
        Some(NotePosition::Left) => center_x - width - 2,
        Some(NotePosition::Right) => center_x + 2,
        None => center_x - width.div_euclid(2),
    };
    Some(HorizontalBounds { left_x, right_x: left_x + width - 1 })
}

fn expand_horizontal_bounds(bounds: &mut HorizontalBounds, left_x: i64, right_x: i64) {
    bounds.left_x = bounds.left_x.min(left_x);
    bounds.right_x = bounds.right_x.max(right_x);
}

fn resolve_group_bounds(
    diagram: &SequenceDiagram,
    centers: &[i64],
    participant_indexes: &HashMap<String, usize>,
    ranges: &[(usize, GroupRange)],
    compact: bool,
) -> Vec<GroupPlacement> {
    ranges
        .iter()
        .map(|(group_index, range)| {
            let group = &diagram.groups[*group_index];
            let mut content_left_x = centers[range.start_index];
            let mut content_right_x = centers[range.end_index];
            for index in range.start_index..=range.end_index {
                let header_width =
                    participant_header_width(&diagram.participants[index].label, compact);
                let header_start_x = centers[index] - header_width.div_euclid(2);
                content_left_x = content_left_x.min(header_start_x);
                content_right_x = content_right_x.max(header_start_x + header_width - 1);
            }
            for step in &diagram.steps {
                let indexes = match step {
                    SequenceStep::Message(message) => get_participant_indexes(
                        participant_indexes,
                        &[message.from.clone(), message.to.clone()],
                    ),
                    SequenceStep::Note(note) => {
                        get_participant_indexes(participant_indexes, &note.over)
                    }
                    _ => Vec::new(),
                };
                if indexes.is_empty()
                    || !indexes
                        .iter()
                        .all(|index| *index >= range.start_index && *index <= range.end_index)
                {
                    continue;
                }
                if let Some(bounds) = get_step_content_bounds(step, centers, participant_indexes) {
                    content_left_x = content_left_x.min(bounds.left_x);
                    content_right_x = content_right_x.max(bounds.right_x);
                }
            }
            let label_text =
                if group.label.is_empty() { String::new() } else { format!(" {} ", group.label) };
            let mut left_x = content_left_x - GROUP_HORIZONTAL_PADDING;
            let mut right_x = content_right_x + GROUP_HORIZONTAL_PADDING;
            let extra_width = 0.max(visual_length(&label_text) + 4 - (right_x - left_x + 1));
            left_x -= extra_width.div_euclid(2);
            right_x += extra_width.div_euclid(2) + extra_width.rem_euclid(2);
            GroupPlacement { label_text, left_x, right_x }
        })
        .collect()
}

fn resolve_participant_centers(
    diagram: &SequenceDiagram,
    participant_indexes: &HashMap<String, usize>,
    min_participant_gap: i64,
    compact: bool,
) -> Vec<i64> {
    let mut gaps: Vec<i64> = (0..diagram.participants.len().saturating_sub(1))
        .map(|index| {
            let left = &diagram.participants[index];
            let right = &diagram.participants[index + 1];
            min_participant_gap.max(
                (participant_header_width(&left.label, compact) + 1).div_euclid(2)
                    + (participant_header_width(&right.label, compact) + 1).div_euclid(2)
                    + 6,
            )
        })
        .collect();
    for message in &diagram.messages {
        let from_index = participant_indexes.get(&message.from).copied().unwrap_or(usize::MAX);
        let to_index = participant_indexes.get(&message.to).copied().unwrap_or(usize::MAX);
        if from_index == to_index && from_index < diagram.participants.len() - 1 {
            if let Some(gap) = gaps.get_mut(from_index) {
                let next_header = mermaid_label_lines(&diagram.participants[from_index + 1].label)
                    .iter()
                    .map(|line| visual_length(line))
                    .max()
                    .unwrap_or(0);
                *gap = (*gap)
                    .max(self_message_loop_width(message) + (next_header + 1).div_euclid(2) + 2);
            }
            continue;
        }
        if from_index == usize::MAX
            || to_index == usize::MAX
            || (from_index as i64 - to_index as i64).abs() != 1
        {
            continue;
        }
        let gap_index = from_index.min(to_index);
        if let Some(gap) = gaps.get_mut(gap_index) {
            *gap = (*gap).max(message_width(message) + 6);
        }
    }
    for step in &diagram.steps {
        let SequenceStep::Note(note) = step else { continue };
        let indexes = get_participant_indexes(participant_indexes, &note.over);
        if indexes.len() == 1 && note.position.is_some() {
            let participant_index = indexes[0] as i64;
            let gap_index = if note.position == Some(NotePosition::Left) {
                participant_index - 1
            } else {
                participant_index
            };
            if gap_index >= 0 && (gap_index as usize) < gaps.len() {
                let gap = &mut gaps[gap_index as usize];
                *gap = (*gap).max(label_lines_width(&note_label_lines(&note.label)) + 4);
            }
            continue;
        }
        if indexes.len() != 2 || (indexes[0] as i64 - indexes[1] as i64).abs() != 1 {
            continue;
        }
        let gap_index = indexes[0].min(indexes[1]);
        if let Some(gap) = gaps.get_mut(gap_index) {
            *gap = (*gap).max(label_lines_width(&note_label_lines(&note.label)) + 4);
        }
    }
    let mut centers = Vec::with_capacity(diagram.participants.len());
    let first = participant_header_width(
        diagram.participants.first().map_or("", |participant| &participant.label),
        compact,
    );
    centers.push(1.max(first.div_euclid(2)));
    for index in 1..diagram.participants.len() {
        let previous = centers[index - 1] + gaps[index - 1];
        centers.push(previous);
    }
    centers
}

fn separate_expanded_groups(
    diagram: &SequenceDiagram,
    centers: &[i64],
    participant_indexes: &HashMap<String, usize>,
    ranges: &[(usize, GroupRange)],
    compact: bool,
) -> Vec<i64> {
    let mut adjusted = centers.to_vec();
    for boundary in 0..adjusted.len().saturating_sub(1) {
        let groups = resolve_group_bounds(diagram, &adjusted, participant_indexes, ranges, compact);
        let left_width = participant_header_width(&diagram.participants[boundary].label, compact);
        let right_width =
            participant_header_width(&diagram.participants[boundary + 1].label, compact);
        let mut left_right = adjusted[boundary] - left_width.div_euclid(2) + left_width - 1;
        let mut right_left = adjusted[boundary + 1] - right_width.div_euclid(2);
        let mut borders_group = false;
        for (index, (_, range)) in ranges.iter().enumerate() {
            if range.end_index == boundary {
                left_right = left_right.max(groups[index].right_x);
                borders_group = true;
            }
            if range.start_index == boundary + 1 {
                right_left = right_left.min(groups[index].left_x);
                borders_group = true;
            }
        }
        if !borders_group {
            continue;
        }
        let shift = left_right + GROUP_HORIZONTAL_PADDING + 1 - right_left;
        if shift <= 0 {
            continue;
        }
        for participant_index in (boundary + 1)..adjusted.len() {
            adjusted[participant_index] += shift;
        }
    }
    adjusted
}

fn get_fragment_frame_bounds(
    centers: &[i64],
    label: &str,
    nesting_depth: i64,
) -> Option<HorizontalBounds> {
    let left_participant_x = *centers.first()?;
    let right_participant_x = *centers.last()?;
    let left_x = left_participant_x - FRAGMENT_HORIZONTAL_OVERHANG + nesting_depth;
    let participant_right_x = right_participant_x + FRAGMENT_HORIZONTAL_OVERHANG - nesting_depth;
    Some(HorizontalBounds {
        left_x,
        right_x: participant_right_x.max(left_x + 2 + visual_length(label) + 1),
    })
}

fn get_fragment_frame_bounds_by_step(
    centers: &[i64],
    steps: &[SequenceStep],
    participant_indexes: &HashMap<String, usize>,
) -> HashMap<usize, HorizontalBounds> {
    let mut bounds_by_step: HashMap<usize, HorizontalBounds> = HashMap::new();
    let mut active_frames: Vec<(usize, HorizontalBounds)> = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if !matches!(step, SequenceStep::Fragment(_)) {
            if let Some(content) = get_step_content_bounds(step, centers, participant_indexes) {
                for (_, frame) in active_frames.iter_mut() {
                    expand_horizontal_bounds(frame, content.left_x - 1, content.right_x + 1);
                }
            }
            continue;
        }
        let SequenceStep::Fragment(fragment) = step else { unreachable!() };
        let Some(mut bounds) = get_fragment_frame_bounds(
            centers,
            &fragment_label_text(fragment.kind, &fragment.label),
            active_frames.len() as i64,
        ) else {
            continue;
        };
        if matches!(fragment.kind, FragmentKind::Alt | FragmentKind::Loop) {
            active_frames.push((index, bounds));
            continue;
        }
        let Some(frame_position) = active_frames.len().checked_sub(1) else { continue };
        {
            let (_, frame_bounds) = &mut active_frames[frame_position];
            expand_horizontal_bounds(frame_bounds, bounds.left_x, bounds.right_x);
            bounds = *frame_bounds;
        }
        if fragment.kind != FragmentKind::End {
            continue;
        }
        let (start_index, frame_bounds) = active_frames.pop().expect("frame");
        bounds_by_step.insert(start_index, frame_bounds);
        if let Some((_, parent)) = active_frames.last_mut() {
            expand_horizontal_bounds(parent, frame_bounds.left_x - 1, frame_bounds.right_x + 1);
        }
    }
    for (start_index, bounds) in active_frames {
        bounds_by_step.insert(start_index, bounds);
    }
    bounds_by_step
}

/// `createSequencePlacementPlan`.
#[allow(clippy::too_many_lines)]
pub fn create_sequence_placement_plan(
    diagram: &SequenceDiagram,
    options: &SequenceRenderOptions,
) -> SequencePlacementPlan {
    if diagram.participants.is_empty() {
        return SequencePlacementPlan {
            rows: PlanRows {
                participant_header_top_y: 0,
                participant_header_y: 0,
                participant_rule_y: 0,
                lifeline_start_y: 0,
                lifeline_end_y: 0,
            },
            ..Default::default()
        };
    }
    let indexes: HashMap<String, usize> = diagram
        .participants
        .iter()
        .enumerate()
        .map(|(index, participant)| (participant.id.clone(), index))
        .collect();
    let compact = options.compact;
    let min_gap = options.min_participant_gap.unwrap_or(DEFAULT_MIN_PARTICIPANT_GAP).max(1);
    let mut centers = resolve_participant_centers(diagram, &indexes, min_gap, compact);
    let ranges = get_group_ranges(diagram, &indexes);
    centers = separate_expanded_groups(diagram, &centers, &indexes, &ranges, compact);
    let mut groups = resolve_group_bounds(diagram, &centers, &indexes, &ranges, compact);

    let mut content_bounds = HorizontalBounds { left_x: 0, right_x: 0 };
    for index in 0..diagram.participants.len() {
        let header_width = participant_header_width(&diagram.participants[index].label, compact);
        let label_start_x = centers[index] - header_width.div_euclid(2);
        expand_horizontal_bounds(
            &mut content_bounds,
            label_start_x,
            label_start_x + header_width - 1,
        );
    }
    for step in &diagram.steps {
        if let Some(content) = get_step_content_bounds(step, &centers, &indexes) {
            expand_horizontal_bounds(&mut content_bounds, content.left_x, content.right_x);
        }
    }
    let mut frame_bounds = get_fragment_frame_bounds_by_step(&centers, &diagram.steps, &indexes);
    let fragment_bounds = |frame_bounds: &HashMap<usize, HorizontalBounds>| -> HorizontalBounds {
        let mut result = HorizontalBounds { left_x: 0, right_x: 0 };
        for bounds in frame_bounds.values() {
            expand_horizontal_bounds(&mut result, bounds.left_x, bounds.right_x);
        }
        result
    };
    let mut fragments = fragment_bounds(&frame_bounds);
    let left_overflow = groups
        .iter()
        .map(|group| group.left_x)
        .min()
        .unwrap_or(0)
        .min(content_bounds.left_x)
        .min(fragments.left_x)
        .min(0);
    if left_overflow < 0 {
        centers = centers.iter().map(|center| center - left_overflow).collect();
        groups = resolve_group_bounds(diagram, &centers, &indexes, &ranges, compact);
        let mut updated_content = HorizontalBounds { left_x: 0, right_x: 0 };
        for index in 0..diagram.participants.len() {
            let header_width =
                participant_header_width(&diagram.participants[index].label, compact);
            let label_start_x = centers[index] - header_width.div_euclid(2);
            expand_horizontal_bounds(
                &mut updated_content,
                label_start_x,
                label_start_x + header_width - 1,
            );
        }
        for step in &diagram.steps {
            if let Some(content) = get_step_content_bounds(step, &centers, &indexes) {
                expand_horizontal_bounds(&mut updated_content, content.left_x, content.right_x);
            }
        }
        content_bounds = updated_content;
        frame_bounds = get_fragment_frame_bounds_by_step(&centers, &diagram.steps, &indexes);
        fragments = fragment_bounds(&frame_bounds);
    }
    let has_groups = !groups.is_empty();
    let participant_label_height = 1i64.max(
        diagram
            .participants
            .iter()
            .map(|participant| mermaid_label_lines(&participant.label).len() as i64)
            .max()
            .unwrap_or(1),
    );
    let participant_header_top_y = i64::from(has_groups);
    let participant_header_y = participant_header_top_y;
    let participant_rule_y = participant_header_top_y
        + if compact { participant_label_height - 1 } else { participant_label_height };
    let lifeline_start_y = participant_rule_y + 1;
    let step_start_y = lifeline_start_y + 1;
    let width = (content_bounds.right_x + 1)
        .max(groups.iter().map(|group| group.right_x + 1).max().unwrap_or(0))
        .max(fragments.right_x + 1);
    let base_height = step_start_y
        + diagram
            .steps
            .iter()
            .map(|step| get_step_height(step, &centers, &indexes, compact))
            .sum::<i64>();
    let height = if has_groups { 5.max(base_height + 1) } else { 3.max(base_height) };
    let lifeline_end_y = if has_groups { height - 2 } else { height - 1 };
    let participants: Vec<ParticipantPlacement> = diagram
        .participants
        .iter()
        .enumerate()
        .map(|(index, participant)| {
            let center_x = centers[index];
            let width = participant_header_width(&participant.label, compact);
            let header_left_x = center_x - width.div_euclid(2);
            ParticipantPlacement {
                participant: participant.clone(),
                center_x,
                header_left_x,
                header_right_x: header_left_x + width - 1,
                label_lines: mermaid_label_lines(&participant.label),
            }
        })
        .collect();

    let mut steps: Vec<StepPlacement> = Vec::new();
    let mut activations: Vec<ActivationPlacement> = Vec::new();
    struct OpenActivation {
        start_y: i64,
        depth: usize,
    }
    let mut active_by_participant: HashMap<String, Vec<OpenActivation>> = HashMap::new();
    let mut last_event_y_by_participant: HashMap<String, i64> = HashMap::new();
    let open_activation =
        |participant: &str,
         y: i64,
         active_by_participant: &mut HashMap<String, Vec<OpenActivation>>| {
            let active = active_by_participant.entry(participant.to_string()).or_default();
            let depth = active.len();
            active.push(OpenActivation { start_y: y, depth });
        };
    let close_activation = |participant: &str,
                            y: i64,
                            active_by_participant: &mut HashMap<String, Vec<OpenActivation>>,
                            activations: &mut Vec<ActivationPlacement>,
                            centers: &[i64],
                            indexes: &HashMap<String, usize>| {
        let opened = active_by_participant.get_mut(participant).and_then(Vec::pop);
        let Some(opened) = opened else { return };
        let Some(participant_index) = indexes.get(participant) else { return };
        activations.push(ActivationPlacement {
            participant: participant.to_string(),
            center_x: centers[*participant_index],
            start_y: opened.start_y,
            end_y: y,
            depth: opened.depth,
        });
    };

    let mut step_y = step_start_y;
    struct ActiveFrame {
        bounds: HorizontalBounds,
        boundary_y: i64,
    }
    let mut active_frames: Vec<ActiveFrame> = Vec::new();
    for (step_index, step) in diagram.steps.iter().enumerate() {
        if let SequenceStep::Activation(activation) = step {
            let event_y = (last_event_y_by_participant
                .get(&activation.participant)
                .copied()
                .unwrap_or(step_y))
            .min(lifeline_end_y);
            if activation.active {
                open_activation(&activation.participant, event_y, &mut active_by_participant);
            } else {
                close_activation(
                    &activation.participant,
                    event_y,
                    &mut active_by_participant,
                    &mut activations,
                    &centers,
                    &indexes,
                );
            }
            continue;
        }
        let step_height = get_step_height(step, &centers, &indexes, compact);
        if let SequenceStep::Note(note) = step {
            let note_indexes = get_participant_indexes(&indexes, &note.over);
            if !note_indexes.is_empty() {
                let center_x = (centers[*note_indexes.iter().min().unwrap()]
                    + centers[*note_indexes.iter().max().unwrap()])
                .div_euclid(2);
                let text_lines = note_label_lines(&note.label);
                let width = label_lines_width(&text_lines);
                let text_x = match note.position {
                    Some(NotePosition::Left) => center_x - width - 2,
                    Some(NotePosition::Right) => center_x + 2,
                    None => center_x - width.div_euclid(2),
                };
                steps.push(StepPlacement::Note { text_lines, text_x, text_y: step_y + 1 });
            }
            step_y += step_height;
            continue;
        }
        if let SequenceStep::Fragment(fragment) = step {
            let mut bounds = frame_bounds.get(&step_index).copied().unwrap_or_else(|| {
                get_fragment_frame_bounds(
                    &centers,
                    &fragment_label_text(fragment.kind, &fragment.label),
                    0,
                )
                .unwrap_or(HorizontalBounds { left_x: 0, right_x: 0 })
            });
            let mut walls_before = None;
            if matches!(fragment.kind, FragmentKind::Alt | FragmentKind::Loop) {
                active_frames.push(ActiveFrame { bounds, boundary_y: step_y });
            } else if let Some(frame) = active_frames.last_mut() {
                bounds = frame.bounds;
                walls_before = Some(WallPlacement {
                    bounds: frame.bounds,
                    start_y: frame.boundary_y + 1,
                    end_y: step_y - 1,
                });
                if fragment.kind == FragmentKind::End {
                    active_frames.pop();
                } else {
                    frame.boundary_y = step_y;
                }
            }
            steps.push(StepPlacement::Fragment {
                kind: fragment.kind,
                label_text: fragment_label_text(fragment.kind, &fragment.label),
                bounds,
                y: step_y,
                walls_before,
            });
            step_y += step_height;
            continue;
        }
        let SequenceStep::Message(message) = step else { unreachable!() };
        let from_index = indexes.get(&message.from).copied().unwrap_or(usize::MAX);
        let to_index = indexes.get(&message.to).copied().unwrap_or(usize::MAX);
        if from_index == usize::MAX || to_index == usize::MAX {
            continue;
        }
        let label_lines = mermaid_label_lines(&message_label_text(message));
        if from_index == to_index {
            let center_x = centers[from_index];
            let loop_width = self_message_loop_width_for_lines(&label_lines);
            let bottom_y = step_y + label_lines.len() as i64 + 1;
            steps.push(StepPlacement::SelfMessage {
                message: message.clone(),
                label_lines,
                center_x,
                right_x: center_x + loop_width,
                top_y: step_y,
                bottom_y,
            });
            if let Some(activate) = &message.activate {
                open_activation(activate, bottom_y, &mut active_by_participant);
            }
            if let Some(deactivate) = &message.deactivate {
                close_activation(
                    deactivate,
                    bottom_y,
                    &mut active_by_participant,
                    &mut activations,
                    &centers,
                    &indexes,
                );
            }
            last_event_y_by_participant.insert(message.from.clone(), bottom_y);
        } else {
            let from_x = centers[from_index];
            let to_x = centers[to_index];
            let direction = if to_x > from_x { 1 } else { -1 };
            let left_x = from_x.min(to_x);
            let right_x = from_x.max(to_x);
            let inline_label = inline_message_label(message, &label_lines, from_x, to_x, compact);
            let arrow_y =
                if inline_label.is_some() { step_y } else { step_y + label_lines.len() as i64 };
            let rendered_label_width = match &inline_label {
                Some(label) => visual_length(label),
                None => label_lines_width(&label_lines),
            };
            let label_x = if inline_label.is_some() {
                (left_x + right_x - rendered_label_width).div_euclid(2)
            } else {
                left_x + 2
            };
            steps.push(StepPlacement::Message {
                message: message.clone(),
                label_lines,
                label_x,
                label_y: step_y,
                arrow_y,
                from_x,
                to_x,
                left_x,
                right_x,
                head_x: arrow_head_x(to_x, direction, message.head),
                direction,
                inline_label,
            });
            if let Some(activate) = &message.activate {
                open_activation(activate, arrow_y, &mut active_by_participant);
            }
            if let Some(deactivate) = &message.deactivate {
                close_activation(
                    deactivate,
                    arrow_y,
                    &mut active_by_participant,
                    &mut activations,
                    &centers,
                    &indexes,
                );
            }
            last_event_y_by_participant.insert(message.from.clone(), arrow_y);
            last_event_y_by_participant.insert(message.to.clone(), arrow_y);
        }
        step_y += step_height;
    }
    for participant in active_by_participant.keys().cloned().collect::<Vec<_>>() {
        let Some(active) = active_by_participant.get(&participant) else { continue };
        let mut remaining = active.len();
        while remaining > 0 {
            close_activation(
                &participant,
                lifeline_end_y,
                &mut active_by_participant,
                &mut activations,
                &centers,
                &indexes,
            );
            remaining -= 1;
        }
    }
    SequencePlacementPlan {
        width,
        height,
        rows: PlanRows {
            participant_header_top_y,
            participant_header_y,
            participant_rule_y,
            lifeline_start_y,
            lifeline_end_y,
        },
        participants,
        groups,
        activations,
        steps,
    }
}

fn self_message_loop_width(message: &SequenceMessage) -> i64 {
    self_message_loop_width_for_lines(&mermaid_label_lines(&message_label_text(message)))
}
