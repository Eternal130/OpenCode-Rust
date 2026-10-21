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

//! `state/parser.ts` — states, aliases, composites, choice pseudo
//! states, notes, and the `-->` transition scanner. The upstream
//! regexes are hand-rolled here; the JS greedy-backtracking groups are
//! reproduced by scanning `-->` occurrences right-to-left (the greedy
//! from-group resolves to the last arrow that still matches) and by
//! handing the first in-token `:` to the label separator.

use std::collections::{HashMap, HashSet};

use crate::render::mermaid::core::mermaid::{
    decode_mermaid_text, first_meaningful_mermaid_line, numbered_mermaid_lines,
};
use crate::render::mermaid::core::text_lines::split_diagram_lines;
use crate::render::mermaid::diagnostics::{MermaidDiagramKind, MermaidSyntaxError};
use crate::render::mermaid::state::endpoint::{
    normalize_state_diagram_endpoint, state_diagram_end_marker_id, state_diagram_start_marker_id,
    EndpointPosition, DEFAULT_STATE_DIRECTION,
};
use crate::render::mermaid::state::types::{
    NotePosition, StateDiagram, StateDiagramCompositeState, StateDiagramNote, StateDiagramState,
    StateDiagramTransition, StateDirection, StateKind,
};

fn is_mermaid_header(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower == "statediagram-v2" || lower == "statediagram"
}

/// Case-insensitive literal prefix (ASCII keywords over trimmed lines).
fn strip_keyword_ci<'a>(rest: &'a str, keyword: &str) -> Option<&'a str> {
    let head = rest.get(..keyword.len())?;
    if head.eq_ignore_ascii_case(keyword) {
        Some(&rest[keyword.len()..])
    } else {
        None
    }
}

fn is_space(character: char) -> bool {
    character.is_whitespace()
}

/// `keyword\s+rest` — the leading case-insensitive keyword with at
/// least one whitespace after it.
fn keyword_rest<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = strip_keyword_ci(line, keyword)?;
    let rest = rest.strip_prefix(is_space)?;
    Some(rest.trim_start())
}

/// `(\S+)\s*\{$` tail — strip the trailing `{` and surrounding spaces,
/// leaving a single whitespace-free id.
fn composite_id_tail(rest: &str) -> Option<&str> {
    let body = rest.strip_suffix('{')?;
    let id = body.trim_end();
    if id.is_empty() || id.chars().any(is_space) {
        return None;
    }
    Some(id)
}

/// `\s+AS\s+(\S+)$` — the separator between a quoted label and the id.
fn quoted_as_tail(after: &str) -> Option<&str> {
    let trimmed = after.trim_start();
    if trimmed.len() == after.len() {
        return None; // `\s+` before `as` requires whitespace
    }
    let rest = strip_keyword_ci(trimmed, "as")?;
    let rest = rest.strip_prefix(is_space)?; // `\s+` after `as`
    let id = rest.trim_start();
    if id.is_empty() || id.chars().any(is_space) {
        return None; // `(\S+)$`
    }
    Some(id)
}

/// `STATE_RE = ^state\s+"([^"]+)"\s+as\s+(\S+)$`.
fn parse_state_alias(line: &str) -> Option<(String, String)> {
    let rest = keyword_rest(line, "state")?;
    let rest = rest.strip_prefix('"')?;
    let close = rest.find('"')?;
    if close == 0 {
        return None; // `[^"]+`
    }
    let label = &rest[..close];
    let id = quoted_as_tail(&rest[close + 1..])?;
    Some((label.to_string(), id.to_string()))
}

/// `COMPOSITE_STATE_RE = ^state\s+(?:"([^"]+)"\s+as\s+)?(\S+)\s*\{$`.
fn parse_composite(line: &str) -> Option<(Option<String>, String)> {
    let rest = keyword_rest(line, "state")?;
    if rest.starts_with('"') {
        let after_quote = &rest[1..];
        let close = after_quote.find('"')?;
        if close == 0 {
            return None;
        }
        let label = &after_quote[..close];
        let id = quoted_as_composite_tail(&after_quote[close + 1..])?;
        return Some((Some(label.to_string()), id));
    }
    let id = composite_id_tail(rest)?;
    Some((None, id.to_string()))
}

/// `\s+as\s+(\S+)\s*\{$` — the quoted-label composite tail.
fn quoted_as_composite_tail(after: &str) -> Option<String> {
    let trimmed = after.trim_start();
    if trimmed.len() == after.len() {
        return None; // `\s+` before `as` requires whitespace
    }
    let rest = strip_keyword_ci(trimmed, "as")?;
    let rest = rest.strip_prefix(is_space)?; // `\s+` after `as`
    composite_id_tail(rest.trim_start()).map(str::to_string)
}

/// `CHOICE_STATE_RE = ^state\s+(\S+)\s+<<choice>>$`.
fn parse_choice(line: &str) -> Option<String> {
    let rest = keyword_rest(line, "state")?;
    let split = rest.find(is_space)?;
    let id = &rest[..split];
    if id.is_empty() {
        return None;
    }
    let tail = rest[split..].trim_start();
    if tail.eq_ignore_ascii_case("<<choice>>") {
        Some(id.to_string())
    } else {
        None
    }
}

/// `DIRECTION_RE = ^direction\s+(TB|TD|BT|LR|RL)$`.
fn parse_direction_line(line: &str) -> Option<StateDirection> {
    let rest = keyword_rest(line, "direction")?;
    StateDirection::parse(rest)
}

/// `NOTE_INLINE_RE = ^note\s+(left|right)\s+of\s+(\S+)\s*:\s*(.*)$`.
fn parse_note_inline(line: &str) -> Option<(NotePosition, String, String)> {
    let rest = keyword_rest(line, "note")?;
    let (position, rest) = parse_left_right(rest)?;
    let rest = of_rest(rest)?;
    let (target, label) = colon_tail(rest)?;
    Some((position, target, label.trim().to_string()))
}

/// `NOTE_START_RE = ^note\s+(left|right)\s+of\s+(\S+)\s*$`.
fn parse_note_start(line: &str) -> Option<(NotePosition, String)> {
    let rest = keyword_rest(line, "note")?;
    let (position, rest) = parse_left_right(rest)?;
    let rest = of_rest(rest)?;
    let target = rest.trim_end();
    if target.is_empty() || target.chars().any(is_space) {
        return None;
    }
    Some((position, target.to_string()))
}

/// `NOTE_END_RE = ^end\s+note$`.
fn is_note_end(line: &str) -> bool {
    keyword_rest(line, "end").is_some_and(|rest| rest.eq_ignore_ascii_case("note"))
}

/// `(left|right)\s+` — the captured position plus the remainder.
fn parse_left_right(rest: &str) -> Option<(NotePosition, &str)> {
    for (word, position) in [("left", NotePosition::Left), ("right", NotePosition::Right)] {
        if let Some(after) = strip_keyword_ci(rest, word) {
            if after.starts_with(is_space) {
                return Some((position, after.trim_start()));
            }
        }
    }
    None
}

/// `of\s+`.
fn of_rest(rest: &str) -> Option<&str> {
    let after = strip_keyword_ci(rest, "of")?;
    after.strip_prefix(is_space).map(str::trim_start)
}

/// `(\S+)\s*:\s*(.*)$` — greedy `\S+` backtracks until the explicit
/// `:` can match (the label may itself contain colons).
fn colon_tail(rest: &str) -> Option<(String, String)> {
    let run_end = rest.find(is_space).unwrap_or(rest.len());
    let run = &rest[..run_end];
    let after_run = &rest[run_end..];

    // Longest group: the explicit `:` sits right after the full run.
    let after_spaces = after_run.trim_start();
    if after_spaces.starts_with(':') {
        if run.is_empty() {
            return None;
        }
        return Some((run.to_string(), after_spaces[1..].trim_start().to_string()));
    }
    // Backtracking hands the first in-run `:` to the literal separator.
    if let Some(colon) = run.find(':') {
        if colon > 0 {
            let label = rest[colon + 1..].trim_start();
            return Some((run[..colon].to_string(), label.to_string()));
        }
    }
    None
}

/// `TRANSITION_RE = ^(\[\*\]|[^\s:]+)\s*-->\s*(\[\*\]|[^\s:]+)(?:\s*:\s*(.*))?$`.
///
/// The greedy from-group means the LAST `-->` that still yields a full
/// line match wins, so occurrences are scanned right-to-left.
fn parse_transition_line(line: &str) -> Option<(String, String, Option<String>)> {
    let mut arrows: Vec<usize> = Vec::new();
    let mut cursor = 0;
    while let Some(found) = line[cursor..].find("-->") {
        arrows.push(cursor + found);
        cursor = cursor + found + 1;
    }
    for &arrow in arrows.iter().rev() {
        let prefix = line[..arrow].trim_end();
        if prefix.is_empty() || prefix.chars().any(|c| c.is_whitespace() || c == ':') {
            continue;
        }
        let rest = line[arrow + 3..].trim_start();
        let token_end = rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(rest.len());
        if token_end == 0 {
            continue;
        }
        let token = &rest[..token_end];
        let after = &rest[token_end..];
        if after.is_empty() {
            return Some((prefix.to_string(), token.to_string(), None));
        }
        let after_spaces = after.trim_start();
        if let Some(label) = after_spaces.strip_prefix(':') {
            let label = label.trim_start();
            return Some((prefix.to_string(), token.to_string(), Some(label.to_string())));
        }
    }
    None
}

/// Insertion-ordered `Map<string, StateDiagramState>` — `[...states.values()]`
/// order drives the layout.
struct OrderedStates {
    states: Vec<StateDiagramState>,
    index: HashMap<String, usize>,
}

impl OrderedStates {
    fn new() -> Self {
        Self { states: Vec::new(), index: HashMap::new() }
    }

    fn has(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    /// `ensureState`.
    fn ensure(&mut self, id: &str, label: &str, kind: StateKind, parent_id: Option<&str>) {
        if let Some(&position) = self.index.get(id) {
            let existing = &mut self.states[position];
            if existing.label == existing.id && label != id {
                existing.label = label.to_string();
            }
            if let Some(parent) = parent_id {
                if existing.parent_id.is_none() {
                    existing.parent_id = Some(parent.to_string());
                }
            }
            if kind != StateKind::State {
                existing.kind = kind;
                existing.label = label.to_string();
            }
            return;
        }
        let position = self.states.len();
        self.states.push(StateDiagramState {
            id: id.to_string(),
            label: label.to_string(),
            kind,
            parent_id: parent_id.map(str::to_string),
        });
        self.index.insert(id.to_string(), position);
    }

    fn into_values(self) -> Vec<StateDiagramState> {
        self.states
    }
}

/// `isMermaidStateDiagram`.
#[must_use]
pub fn is_mermaid_state_diagram(content: &str) -> bool {
    first_meaningful_mermaid_line(content).is_some_and(is_mermaid_header)
}

/// `resolveCompositeTransitionEndpoint`.
fn resolve_composite_transition_endpoint(
    id: &str,
    marker: fn(Option<&str>) -> String,
    composite_ids: &HashSet<String>,
    states: &OrderedStates,
) -> String {
    if !composite_ids.contains(id) {
        return id.to_string();
    }
    let marker_id = marker(Some(id));
    if states.has(&marker_id) {
        marker_id
    } else {
        id.to_string()
    }
}

/// `parseMermaidStateDiagram`.
pub fn parse_mermaid_state_diagram(content: &str) -> Result<StateDiagram, MermaidSyntaxError> {
    let unsupported = |line_number: usize, line: &str| {
        MermaidSyntaxError::unsupported(MermaidDiagramKind::State, line_number, line)
    };
    let syntax = |line_number: usize, line: &str, message: &str| {
        MermaidSyntaxError::new(MermaidDiagramKind::State, line_number, line, message)
    };

    let mut states = OrderedStates::new();
    let mut transitions: Vec<StateDiagramTransition> = Vec::new();
    let mut composites: Vec<StateDiagramCompositeState> = Vec::new();
    let mut notes: Vec<StateDiagramNote> = Vec::new();
    let mut parent_stack: Vec<(String, usize, String)> = Vec::new();
    let mut pending_note: Option<(String, NotePosition, Vec<String>, usize, String)> = None;
    let mut direction = DEFAULT_STATE_DIRECTION;

    for source in numbered_mermaid_lines(content) {
        let line = source.text;
        if let Some((target, position, lines, _, _)) = &mut pending_note {
            if is_note_end(line) {
                notes.push(StateDiagramNote {
                    target: target.clone(),
                    position: *position,
                    lines: lines.iter().map(|line| decode_mermaid_text(line)).collect(),
                });
                pending_note = None;
            } else if !line.is_empty() || !lines.is_empty() {
                lines.push(line.to_string());
            }
            continue;
        }

        if line.is_empty() || line.starts_with("%%") || is_mermaid_header(line) {
            continue;
        }

        if line == "}" {
            if parent_stack.is_empty() {
                return Err(syntax(
                    source.line_number,
                    line,
                    "Unexpected \"}\" without an open composite state",
                ));
            }
            parent_stack.pop();
            continue;
        }

        let parent_id: Option<&str> = parent_stack.last().map(|(id, _, _)| id.as_str());

        if let Some(parsed) = parse_direction_line(line) {
            if !parent_stack.is_empty() {
                return Err(syntax(
                    source.line_number,
                    line,
                    "Composite-local direction is not supported",
                ));
            }
            direction = parsed;
            continue;
        }

        if let Some((position, target, label)) = parse_note_inline(line) {
            notes.push(StateDiagramNote {
                position,
                target,
                lines: split_diagram_lines(&decode_mermaid_text(&label)),
            });
            continue;
        }

        if let Some((position, target)) = parse_note_start(line) {
            pending_note =
                Some((target, position, Vec::new(), source.line_number, line.to_string()));
            continue;
        }

        if let Some((label, id)) = parse_composite(line) {
            composites.push(StateDiagramCompositeState {
                id: id.clone(),
                label: decode_mermaid_text(&label.unwrap_or_else(|| id.clone())),
                parent_id: parent_id.map(str::to_string),
            });
            parent_stack.push((id, source.line_number, line.to_string()));
            continue;
        }

        if let Some((label, id)) = parse_state_alias(line) {
            states.ensure(&id, &decode_mermaid_text(&label), StateKind::State, parent_id);
            continue;
        }

        if let Some(id) = parse_choice(line) {
            states.ensure(&id, "", StateKind::Choice, parent_id);
            continue;
        }

        if let Some((raw_from, raw_to, label)) = parse_transition_line(line) {
            if raw_from.contains("-->") || raw_to.contains("-->") {
                return Err(unsupported(source.line_number, line));
            }
            let from =
                normalize_state_diagram_endpoint(&raw_from, EndpointPosition::From, parent_id);
            let to = normalize_state_diagram_endpoint(&raw_to, EndpointPosition::To, parent_id);
            states.ensure(
                &from,
                if raw_from == "[*]" { "●" } else { &from },
                if raw_from == "[*]" { StateKind::Start } else { StateKind::State },
                parent_id,
            );
            states.ensure(
                &to,
                if raw_to == "[*]" { "◎" } else { &to },
                if raw_to == "[*]" { StateKind::End } else { StateKind::State },
                parent_id,
            );
            transitions.push(StateDiagramTransition {
                from,
                to,
                label: decode_mermaid_text(label.as_deref().map(str::trim).unwrap_or("")),
            });
            continue;
        }

        return Err(unsupported(source.line_number, line));
    }

    if let Some((_, _, _, line_number, source_line)) = pending_note {
        return Err(syntax(line_number, &source_line, "Unclosed note; expected \"end note\""));
    }

    if let Some((_, line_number, source_line)) = parent_stack.last() {
        return Err(syntax(*line_number, source_line, "Unclosed composite state; expected \"}\""));
    }

    let all_states = states.into_values();
    if composites.is_empty() {
        return Ok(StateDiagram { direction, states: all_states, transitions, composites, notes });
    }

    let composite_ids: HashSet<String> =
        composites.iter().map(|composite| composite.id.clone()).collect();
    let states_lookup = {
        let mut lookup = OrderedStates::new();
        for state in &all_states {
            lookup.ensure(&state.id, &state.label, state.kind, state.parent_id.as_deref());
        }
        lookup
    };
    let resolved_transitions: Vec<StateDiagramTransition> = transitions
        .iter()
        .map(|transition| StateDiagramTransition {
            from: resolve_composite_transition_endpoint(
                &transition.from,
                state_diagram_end_marker_id,
                &composite_ids,
                &states_lookup,
            ),
            to: resolve_composite_transition_endpoint(
                &transition.to,
                state_diagram_start_marker_id,
                &composite_ids,
                &states_lookup,
            ),
            label: transition.label.clone(),
        })
        .collect();

    Ok(StateDiagram {
        direction,
        states: all_states.into_iter().filter(|state| !composite_ids.contains(&state.id)).collect(),
        transitions: resolved_transitions,
        composites,
        notes,
    })
}
