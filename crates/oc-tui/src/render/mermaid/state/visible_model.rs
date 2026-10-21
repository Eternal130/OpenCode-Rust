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

//! `state/visible-model.ts` — hides composite-scoped `[*]` markers by
//! splicing their incoming/outgoing transitions together.

use std::collections::{HashMap, HashSet};

use crate::render::mermaid::state::types::{
    StateDiagram, StateDiagramState, StateDiagramTransition,
};

/// `isHiddenCompositeMarker`.
#[must_use]
pub fn is_hidden_composite_marker(state: Option<&StateDiagramState>) -> bool {
    match state {
        Some(state) => {
            state.parent_id.is_some()
                && (state.kind == crate::render::mermaid::state::types::StateKind::Start
                    || state.kind == crate::render::mermaid::state::types::StateKind::End)
        }
        None => false,
    }
}

/// `composeTransitionLabel` — `[incoming.label, outgoing.label].filter(Boolean).join("<br/>")`.
fn compose_transition_label(
    incoming: &StateDiagramTransition,
    outgoing: &StateDiagramTransition,
) -> String {
    match (incoming.label.is_empty(), outgoing.label.is_empty()) {
        (true, true) => String::new(),
        (true, false) => outgoing.label.clone(),
        (false, true) => incoming.label.clone(),
        (false, false) => format!("{}<br/>{}", incoming.label, outgoing.label),
    }
}

/// `collapseHiddenCompositeMarkerTransitionsOnce`.
fn collapse_hidden_composite_marker_transitions_once(
    transitions: &[StateDiagramTransition],
    states: &[StateDiagramState],
) -> (Vec<StateDiagramTransition>, bool) {
    let hidden_markers: Vec<&str> = states
        .iter()
        .filter(|state| is_hidden_composite_marker(Some(state)))
        .map(|state| state.id.as_str())
        .collect();
    if hidden_markers.is_empty() {
        return (transitions.to_vec(), false);
    }

    for marker_id in hidden_markers {
        let incoming: Vec<usize> = transitions
            .iter()
            .enumerate()
            .filter(|(_, transition)| transition.to == marker_id && transition.from != marker_id)
            .map(|(index, _)| index)
            .collect();
        let outgoing: Vec<usize> = transitions
            .iter()
            .enumerate()
            .filter(|(_, transition)| transition.from == marker_id && transition.to != marker_id)
            .map(|(index, _)| index)
            .collect();
        if incoming.is_empty() || outgoing.is_empty() {
            continue;
        }

        let skipped: HashSet<usize> = incoming.iter().chain(outgoing.iter()).copied().collect();
        let mut spliced: Vec<StateDiagramTransition> = transitions
            .iter()
            .enumerate()
            .filter(|(index, _)| !skipped.contains(index))
            .map(|(_, transition)| transition.clone())
            .collect();
        for incoming_index in &incoming {
            for outgoing_index in &outgoing {
                spliced.push(StateDiagramTransition {
                    from: transitions[*incoming_index].from.clone(),
                    to: transitions[*outgoing_index].to.clone(),
                    label: compose_transition_label(
                        &transitions[*incoming_index],
                        &transitions[*outgoing_index],
                    ),
                });
            }
        }
        return (spliced, true);
    }

    (transitions.to_vec(), false)
}

/// `collapseHiddenCompositeMarkerTransitions`.
fn collapse_hidden_composite_marker_transitions(
    diagram: &StateDiagram,
) -> Vec<StateDiagramTransition> {
    let mut transitions: Vec<StateDiagramTransition> = diagram.transitions.clone();

    loop {
        let (next, changed) =
            collapse_hidden_composite_marker_transitions_once(&transitions, &diagram.states);
        transitions = next;
        if !changed {
            return transitions;
        }
    }
}

/// `prepareVisibleStateDiagram`.
#[must_use]
pub fn prepare_visible_state_diagram(diagram: &StateDiagram) -> StateDiagram {
    let transitions = collapse_hidden_composite_marker_transitions(diagram);
    let states_by_id: HashMap<&str, &StateDiagramState> =
        diagram.states.iter().map(|state| (state.id.as_str(), state)).collect();
    let mut referenced_hidden_markers: HashSet<String> = HashSet::new();
    for transition in &transitions {
        if let Some(from) = states_by_id.get(transition.from.as_str()) {
            if is_hidden_composite_marker(Some(from)) {
                referenced_hidden_markers.insert(from.id.clone());
            }
        }
        if let Some(to) = states_by_id.get(transition.to.as_str()) {
            if is_hidden_composite_marker(Some(to)) {
                referenced_hidden_markers.insert(to.id.clone());
            }
        }
    }

    StateDiagram {
        direction: diagram.direction,
        states: diagram
            .states
            .iter()
            .filter(|state| {
                !is_hidden_composite_marker(Some(state))
                    || referenced_hidden_markers.contains(&state.id)
            })
            .cloned()
            .collect(),
        transitions,
        composites: diagram.composites.clone(),
        notes: diagram.notes.clone(),
    }
}
