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

//! `sequence/parser.ts` — participants, messages, notes, activations,
//! fragments, autonumber, box groups.

use crate::render::mermaid::core::mermaid::{
    first_meaningful_mermaid_line, meaningful_numbered_mermaid_lines, strip_mermaid_quotes,
};
use crate::render::mermaid::diagnostics::{MermaidSyntaxError, MERMAID_SEQUENCE};
use crate::render::mermaid::sequence::types::{
    FragmentKind, NotePosition, SequenceArrowHead, SequenceDiagram, SequenceFragment,
    SequenceMessage, SequenceNote, SequenceParticipant, SequenceParticipantGroup, SequenceStep,
};

/// `MESSAGE_OPERATOR_RE` — the arrow set.
const MESSAGE_OPERATORS: [&str; 8] = ["-->>", "->>", "--x", "-x", "--)", "-)", "-->", "->"];

fn contains_message_operator(value: &str) -> bool {
    MESSAGE_OPERATORS.iter().any(|operator| value.contains(operator))
}

/// `MESSAGE_RE` — lazy from, arrow, [+-], lazy to, `:`, label.
fn parse_message(line: &str) -> Option<SequenceMessage> {
    // Find the leftmost arrow occurrence (alternation order at one spot
    // does not matter: the operators are mutually prefix-ordered).
    let chars: Vec<char> = line.chars().collect();
    let mut best: Option<(usize, usize)> = None; // (start, end) char indices
    for start in 0..chars.len() {
        for operator in MESSAGE_OPERATORS {
            let operator: Vec<char> = operator.chars().collect();
            if start + operator.len() <= chars.len()
                && chars[start..start + operator.len()] == operator[..]
            {
                best = Some((start, start + operator.len()));
                break;
            }
        }
        if best.is_some() {
            break;
        }
    }
    let (arrow_start, arrow_end) = best?;
    let arrow: String = chars[arrow_start..arrow_end].iter().collect();

    let from: String = chars[..arrow_start].iter().collect();
    let from = from.trim_end();
    if from.is_empty() {
        return None;
    }

    let mut cursor = arrow_end;
    let activation_marker = chars.get(cursor).copied();
    let activation_marker = if activation_marker == Some('+') || activation_marker == Some('-') {
        cursor += 1;
        activation_marker
    } else {
        None
    };
    while chars.get(cursor).copied().is_some_and(char::is_whitespace) {
        cursor += 1;
    }

    // lazy (.+?)\s*:\s*(.*)$ — first ':' after at least one char
    let mut colon = None;
    for index in (cursor + 1)..=chars.len() {
        if index == chars.len() || chars[index] == ':' {
            // strip trailing whitespace before the colon
            let mut end = index;
            while end > cursor && chars[end - 1].is_whitespace() {
                end -= 1;
            }
            if end > cursor {
                colon = Some((end, index));
                break;
            }
            if index == chars.len() {
                break;
            }
        }
    }
    let (to_end, colon_index) = colon?;
    let to: String = chars[cursor..to_end].iter().collect();
    if to.is_empty() || contains_message_operator(&to) || contains_message_operator(from) {
        return None;
    }
    let mut label_start = colon_index + 1;
    while chars.get(label_start).is_some_and(|char| char.is_whitespace() && *char != '\n') {
        label_start += 1;
    }
    let label: String = chars[label_start..].iter().collect();

    let head = arrow_head_for_syntax(&arrow);
    let mut message = SequenceMessage {
        from: strip_mermaid_quotes(from),
        to: strip_mermaid_quotes(&to),
        label: strip_mermaid_quotes(&label),
        dashed: arrow.starts_with("--"),
        head,
        number: None,
        activate: None,
        deactivate: None,
    };
    if activation_marker == Some('+') {
        message.activate = Some(message.to.clone());
    } else if activation_marker == Some('-') {
        message.deactivate = Some(message.from.clone());
    }
    Some(message)
}

fn arrow_head_for_syntax(arrow: &str) -> Option<SequenceArrowHead> {
    if arrow.ends_with('x') {
        return Some(SequenceArrowHead::Cross);
    }
    if arrow.ends_with(')') {
        return Some(SequenceArrowHead::Async);
    }
    if arrow.ends_with('>') && !arrow.ends_with(">>") {
        return Some(SequenceArrowHead::Open);
    }
    None
}

/// `NOTE_RE = ^note\s+(over|left\s+of|right\s+of)\s+(.+?)\s*:\s*(.*)$` (i).
fn parse_note(line: &str) -> Option<(Option<NotePosition>, String, String)> {
    let lower = line.to_lowercase();
    if !lower.starts_with("note") || !line[4..].starts_with(char::is_whitespace) {
        return None;
    }
    let body = line[4..].trim_start();
    let lower_body = body.to_lowercase();

    let (position, consumed) = if lower_body.starts_with("over")
        && body.len() > 4
        && body[4..].starts_with(char::is_whitespace)
    {
        (None, 4usize)
    } else if lower_body.starts_with("left") || lower_body.starts_with("right") {
        let position =
            if lower_body.starts_with("left") { NotePosition::Left } else { NotePosition::Right };
        let after = body[4..].trim_start();
        let lower_after = after.to_lowercase();
        if !(lower_after.starts_with("of") && after[2..].starts_with(char::is_whitespace)) {
            return None;
        }
        let after_of = after[2..].trim_start();
        (Some(position), body.len() - after_of.len())
    } else {
        return None;
    };

    let rest = &body[consumed..];
    let colon = rest.find(':')?;
    let mut to_end = colon;
    while to_end > 0 && rest.as_bytes()[to_end - 1].is_ascii_whitespace() {
        to_end -= 1;
    }
    if to_end == 0 {
        return None;
    }
    let over = rest[..to_end].to_string();
    let label = rest[colon + 1..].trim_start().to_string();
    Some((position, over, label))
}

/// `PARTICIPANT_RE` — `(?:participant|actor)\s+(\S+)(?:\s+as\s+(.+))?`.
fn parse_participant(line: &str) -> Option<(String, Option<String>)> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("participant").or_else(|| lower.strip_prefix("actor"))?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let body = &line[line.len() - rest.len()..];
    let body = body.trim_start();
    let (id, alias) = match body.find(char::is_whitespace) {
        Some(space) => {
            let id = &body[..space];
            let tail = body[space..].trim_start();
            let alias = if tail.len() >= 2 && tail[..2].eq_ignore_ascii_case("as") {
                let after = &tail[2..];
                if after.starts_with(char::is_whitespace) {
                    Some(after.trim_start().to_string())
                } else {
                    None
                }
            } else {
                None
            };
            alias.as_ref()?;
            (id, alias)
        }
        None => (body, None),
    };
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    Some((id.to_string(), alias))
}

/// `ACTIVATION_RE` — `(activate|deactivate)\s+(.+)`.
fn parse_activation(line: &str) -> Option<(bool, String)> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("activate").or_else(|| lower.strip_prefix("deactivate"))?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let body = &line[line.len() - rest.len()..];
    let participant = body.trim_start();
    if participant.is_empty() {
        return None;
    }
    Some((lower.starts_with("activate"), participant.to_string()))
}

/// `BOX_RE` — `box(?:\s+(.+))?`.
fn parse_box(line: &str) -> Option<Option<String>> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("box")?;
    if rest.is_empty() {
        return Some(None);
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let body = &line[3..];
    Some(Some(body.trim_start().to_string()))
}

/// `AUTONUMBER_RE` — `autonumber(?:\s+(\d+)(?:\s+(\d+))?)?`.
fn parse_autonumber(line: &str) -> Option<(i64, i64)> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("autonumber")?;
    if rest.is_empty() {
        return Some((1, 1));
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let body = rest.trim();
    let mut parts = body.split_whitespace();
    let start = parts.next()?.parse::<i64>().ok()?;
    let increment = parts.next().map_or(1, |value| value.parse::<i64>().unwrap_or(1));
    if parts.next().is_some() {
        return None;
    }
    Some((start, increment))
}

fn parse_alt(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("alt")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(line[3..].trim_start().to_string())
}

fn parse_loop(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("loop")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(line[4..].trim_start().to_string())
}

fn parse_else(line: &str) -> Option<Option<String>> {
    let lower = line.to_lowercase();
    let rest = lower.strip_prefix("else")?;
    if rest.is_empty() {
        return Some(None);
    }
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(Some(line[4..].trim_start().to_string()))
}

/// CSS color-name rejection set (`isBoxColorToken` subset that matters:
/// boxLabel parsing strips a leading color token).
const CSS_COLOR_NAMES: [&str; 26] = [
    "black",
    "white",
    "red",
    "green",
    "blue",
    "yellow",
    "cyan",
    "magenta",
    "silver",
    "gray",
    "grey",
    "maroon",
    "olive",
    "lime",
    "aqua",
    "teal",
    "navy",
    "fuchsia",
    "purple",
    "orange",
    "brightblack",
    "brightred",
    "brightgreen",
    "brightblue",
    "brightyellow",
    "brightwhite",
];

fn is_box_color_token(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower == "transparent"
        || CSS_COLOR_NAMES.contains(&lower.as_str())
        || (value.starts_with('#')
            && value.len() >= 4
            && value[1..].chars().all(|char| char.is_ascii_hexdigit())
            && matches!(value.len(), 4 | 5 | 7 | 8 | 9 | 10))
        || (lower.starts_with("rgb(") || lower.starts_with("rgba(")) && lower.ends_with(')')
}

/// `boxLabelText` — strip a leading color token from the box label.
fn box_label_text(value: Option<&str>) -> String {
    let raw = value.unwrap_or("").trim();
    if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        return strip_mermaid_quotes(raw);
    }
    let label = strip_mermaid_quotes(raw);
    if label.is_empty() {
        return String::new();
    }
    // splitLeadingBoxToken
    let (token, rest): (&str, String) =
        if label.to_lowercase().starts_with("rgba(") || label.to_lowercase().starts_with("rgb(") {
            match label.find(')') {
                Some(close) => (&label[..=close], label[close + 1..].trim().to_string()),
                None => (label.as_str(), String::new()),
            }
        } else {
            match label.find(char::is_whitespace) {
                Some(space) => (&label[..space], label[space + 1..].trim_start().to_string()),
                None => (label.as_str(), String::new()),
            }
        };
    if is_box_color_token(token) {
        strip_mermaid_quotes(&rest)
    } else {
        label
    }
}

/// `isMermaidSequenceDiagram`.
#[must_use]
pub fn is_mermaid_sequence_diagram(content: &str) -> bool {
    first_meaningful_mermaid_line(content)
        .is_some_and(|line| line.to_lowercase() == "sequencediagram")
}

/// `parseMermaidSequenceDiagram`.
pub fn parse_mermaid_sequence_diagram(
    content: &str,
) -> Result<SequenceDiagram, MermaidSyntaxError> {
    let mut participants: Vec<SequenceParticipant> = Vec::new();
    let mut messages: Vec<SequenceMessage> = Vec::new();
    let mut steps: Vec<SequenceStep> = Vec::new();
    let mut groups: Vec<SequenceParticipantGroup> = Vec::new();
    let mut block_stack: Vec<(BlockKind, usize, String)> = Vec::new();
    let mut group_stack: Vec<usize> = Vec::new();
    let mut next_message_number: Option<i64> = None;
    let mut message_number_increment = 1i64;

    macro_rules! ensure_participant {
        ($id:expr, $label:expr, $replace:expr) => {{
            let id = $id;
            let label: Option<String> = $label;
            if let Some(existing) = participants.iter_mut().find(|participant| participant.id == id)
            {
                if $replace {
                    if let Some(label) = label {
                        existing.label = label;
                    }
                }
            } else {
                participants.push(SequenceParticipant {
                    id: id.clone(),
                    label: label.unwrap_or_else(|| id.clone()),
                });
            }
        }};
    }

    for source in meaningful_numbered_mermaid_lines(content) {
        let line = source.text;
        if line.to_lowercase() == "sequencediagram" {
            continue;
        }
        if line.contains("<<->>") || line.contains("<<-->>") {
            return Err(MermaidSyntaxError::unsupported(
                MERMAID_SEQUENCE,
                source.line_number,
                line,
            ));
        }

        if let Some((start, increment)) = parse_autonumber(line) {
            next_message_number = Some(start);
            message_number_increment = increment;
            continue;
        }

        if let Some(box_label) = parse_box(line) {
            groups.push(SequenceParticipantGroup {
                label: box_label_text(box_label.as_deref()),
                participant_ids: Vec::new(),
            });
            group_stack.push(groups.len() - 1);
            block_stack.push((BlockKind::Box, source.line_number, line.to_string()));
            continue;
        }

        if let Some((id, alias)) = parse_participant(line) {
            let id = strip_mermaid_quotes(&id);
            let label = alias.map(|alias| strip_mermaid_quotes(&alias));
            ensure_participant!(id.clone(), label.or(Some(id.clone())), true);
            if let Some(group) = group_stack.last().copied() {
                if !groups[group].participant_ids.contains(&id) {
                    groups[group].participant_ids.push(id);
                }
            }
            continue;
        }

        if let Some((position, over, label)) = parse_note(line) {
            let over: Vec<String> = over
                .split(',')
                .map(strip_mermaid_quotes)
                .filter(|participant| !participant.is_empty())
                .collect();
            for participant in &over {
                ensure_participant!(participant.clone(), Option::<String>::None, false);
            }
            steps.push(SequenceStep::Note(SequenceNote {
                over,
                label: strip_mermaid_quotes(&label),
                position,
            }));
            continue;
        }

        if let Some((active, participant)) = parse_activation(line) {
            let participant = strip_mermaid_quotes(&participant);
            ensure_participant!(participant.clone(), Option::<String>::None, false);
            steps.push(SequenceStep::Activation(
                crate::render::mermaid::sequence::types::SequenceActivation { participant, active },
            ));
            continue;
        }

        if let Some(label) = parse_alt(line) {
            block_stack.push((BlockKind::Alt, source.line_number, line.to_string()));
            steps.push(SequenceStep::Fragment(SequenceFragment {
                kind: FragmentKind::Alt,
                label: strip_mermaid_quotes(&label),
            }));
            continue;
        }
        if let Some(label) = parse_loop(line) {
            block_stack.push((BlockKind::Loop, source.line_number, line.to_string()));
            steps.push(SequenceStep::Fragment(SequenceFragment {
                kind: FragmentKind::Loop,
                label: strip_mermaid_quotes(&label),
            }));
            continue;
        }
        if let Some(label) = parse_else(line) {
            if block_stack.last().map(|block| block.0) != Some(BlockKind::Alt) {
                return Err(MermaidSyntaxError::new(
                    MERMAID_SEQUENCE,
                    source.line_number,
                    line,
                    "Unexpected \"else\" without an open \"alt\" block",
                ));
            }
            steps.push(SequenceStep::Fragment(SequenceFragment {
                kind: FragmentKind::Else,
                label: strip_mermaid_quotes(&label.unwrap_or_default()),
            }));
            continue;
        }

        if line.to_lowercase() == "end" {
            match block_stack.pop() {
                None => {
                    return Err(MermaidSyntaxError::new(
                        MERMAID_SEQUENCE,
                        source.line_number,
                        line,
                        "Unexpected \"end\" without an open block",
                    ));
                }
                Some((BlockKind::Box, _, _)) => {
                    group_stack.pop();
                    continue;
                }
                Some((kind, _, _)) => {
                    steps.push(SequenceStep::Fragment(SequenceFragment {
                        kind: FragmentKind::End,
                        label: match kind {
                            BlockKind::Alt => "alt".to_string(),
                            BlockKind::Loop => "loop".to_string(),
                            BlockKind::Box => "box".to_string(),
                        },
                    }));
                    continue;
                }
            }
        }

        if let Some(mut message) = parse_message(line) {
            let active_group = group_stack.last().copied();
            ensure_participant!(message.from.clone(), Option::<String>::None, false);
            ensure_participant!(message.to.clone(), Option::<String>::None, false);
            for participant in [&message.from, &message.to] {
                if let Some(group) = active_group {
                    if !groups[group].participant_ids.contains(participant) {
                        groups[group].participant_ids.push(participant.clone());
                    }
                }
            }
            if let Some(number) = next_message_number {
                message.number = Some(number);
                next_message_number = Some(number + message_number_increment);
            }
            messages.push(message.clone());
            steps.push(SequenceStep::Message(message));
            continue;
        }

        return Err(MermaidSyntaxError::unsupported(MERMAID_SEQUENCE, source.line_number, line));
    }

    if let Some((kind, line_number, source_line)) = block_stack.last() {
        return Err(MermaidSyntaxError::new(
            MERMAID_SEQUENCE,
            *line_number,
            source_line,
            &format!("Unclosed {kind:?} block; expected \"end\""),
        ));
    }

    Ok(SequenceDiagram { participants, messages, steps, groups })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Box,
    Alt,
    Loop,
}
