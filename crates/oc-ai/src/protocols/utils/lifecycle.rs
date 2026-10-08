//! Shared fragment lifecycle emission (port of `src/protocols/utils/lifecycle.ts`):
//! `*-start/delta/end` triples with `step-start` seeding and open-block
//! closing at finish.

use std::collections::HashSet;

use crate::schema::{FinishReasonDetails, LLMEvent, ProviderMetadata, Usage};

/// Lifecycle emission state (port of `Lifecycle.State`).
#[derive(Debug, Clone, Default)]
pub struct State {
    step_started: bool,
    text: HashSet<String>,
    reasoning: HashSet<String>,
}

impl State {
    #[must_use]
    pub fn initial() -> Self {
        Self::default()
    }
}

pub fn step_start(state: &State, events: &mut Vec<LLMEvent>) -> State {
    if state.step_started {
        return state.clone();
    }
    events.push(LLMEvent::step_start(0.0));
    State { step_started: true, ..state.clone() }
}

pub fn text_start(
    state: &State,
    events: &mut Vec<LLMEvent>,
    id: &str,
    provider_metadata: Option<ProviderMetadata>,
) -> State {
    if state.text.contains(id) {
        return state.clone();
    }
    let stepped = step_start(state, events);
    events.push(LLMEvent::TextStart { id: id.to_string(), provider_metadata });
    let mut text = stepped.text.clone();
    text.insert(id.to_string());
    State { text, ..stepped }
}

pub fn text_delta(
    state: &State,
    events: &mut Vec<LLMEvent>,
    id: &str,
    text: &str,
    provider_metadata: Option<ProviderMetadata>,
) -> State {
    let started = text_start(state, events, id, None);
    events.push(LLMEvent::TextDelta {
        id: id.to_string(),
        text: text.to_string(),
        provider_metadata,
    });
    started
}

pub fn text_end(
    state: &State,
    events: &mut Vec<LLMEvent>,
    id: &str,
    provider_metadata: Option<ProviderMetadata>,
    text: Option<String>,
) -> State {
    if !state.text.contains(id) {
        return state.clone();
    }
    let stepped = step_start(state, events);
    events.push(LLMEvent::TextEnd { id: id.to_string(), text, provider_metadata });
    let mut open = stepped.text.clone();
    open.remove(id);
    State { text: open, ..stepped }
}

pub fn reasoning_start(
    state: &State,
    events: &mut Vec<LLMEvent>,
    id: &str,
    provider_metadata: Option<ProviderMetadata>,
) -> State {
    if state.reasoning.contains(id) {
        return state.clone();
    }
    let stepped = step_start(state, events);
    events.push(LLMEvent::ReasoningStart { id: id.to_string(), provider_metadata });
    let mut reasoning = stepped.reasoning.clone();
    reasoning.insert(id.to_string());
    State { reasoning, ..stepped }
}

pub fn reasoning_delta(
    state: &State,
    events: &mut Vec<LLMEvent>,
    id: &str,
    text: &str,
    provider_metadata: Option<ProviderMetadata>,
) -> State {
    let started = reasoning_start(state, events, id, None);
    events.push(LLMEvent::ReasoningDelta {
        id: id.to_string(),
        text: text.to_string(),
        provider_metadata,
    });
    started
}

pub fn reasoning_end(
    state: &State,
    events: &mut Vec<LLMEvent>,
    id: &str,
    provider_metadata: Option<ProviderMetadata>,
    text: Option<String>,
) -> State {
    if !state.reasoning.contains(id) {
        return state.clone();
    }
    let stepped = step_start(state, events);
    events.push(LLMEvent::ReasoningEnd { id: id.to_string(), text, provider_metadata });
    let mut reasoning = stepped.reasoning.clone();
    reasoning.remove(id);
    State { reasoning, ..stepped }
}

fn close_open_blocks(state: &State, events: &mut Vec<LLMEvent>) -> State {
    let mut reasoning: Vec<&String> = state.reasoning.iter().collect();
    reasoning.sort();
    for id in reasoning {
        events.push(LLMEvent::ReasoningEnd { id: id.clone(), text: None, provider_metadata: None });
    }
    let mut text: Vec<&String> = state.text.iter().collect();
    text.sort();
    for id in text {
        events.push(LLMEvent::TextEnd { id: id.clone(), text: None, provider_metadata: None });
    }
    State { text: HashSet::new(), reasoning: HashSet::new(), ..state.clone() }
}

/// Input to [`finish`].
pub struct FinishInput {
    pub reason: FinishReasonDetails,
    pub usage: Option<Usage>,
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Emit `step-finish` + `finish`, closing any open blocks first (port of
/// `Lifecycle.finish`).
pub fn finish(state: &State, events: &mut Vec<LLMEvent>, input: FinishInput) -> State {
    let stepped = step_start(state, events);
    let closed = close_open_blocks(&stepped, events);
    events.push(LLMEvent::StepFinish {
        index: 0.0,
        reason: input.reason.clone(),
        usage: input.usage.clone(),
        provider_metadata: input.provider_metadata.clone(),
    });
    events.push(LLMEvent::Finish {
        reason: input.reason,
        usage: input.usage,
        provider_metadata: input.provider_metadata,
    });
    State { step_started: false, ..closed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FinishReason;

    #[test]
    fn deltas_seed_step_and_block_starts_exactly_once() {
        let mut events = Vec::new();
        let state = State::initial();
        let state = text_delta(&state, &mut events, "t1", "hello", None);
        let state = text_delta(&state, &mut events, "t1", " world", None);
        let state = reasoning_delta(&state, &mut events, "r1", "hmm", None);
        assert_eq!(
            events.iter().map(LLMEvent::kind).collect::<Vec<_>>(),
            [
                "step-start",
                "text-start",
                "text-delta",
                "text-delta",
                "reasoning-start",
                "reasoning-delta"
            ]
        );
        assert_eq!(state.text.len(), 1);
        assert_eq!(state.reasoning.len(), 1);
    }

    #[test]
    fn end_removes_the_block_but_only_once() {
        let mut events = Vec::new();
        let state = text_delta(&State::initial(), &mut events, "t1", "hi", None);
        events.clear();
        let state = text_end(&state, &mut events, "t1", None, Some("HI".into()));
        let state = text_end(&state, &mut events, "t1", None, None); // second end is a no-op
        assert_eq!(state.text.len(), 0);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], LLMEvent::TextEnd { text: Some(text), .. } if text == "HI"));
    }

    #[test]
    fn finish_closes_open_blocks_then_emits_both_finishes() {
        let mut events = Vec::new();
        let state = text_delta(&State::initial(), &mut events, "t1", "hi", None);
        let state = reasoning_delta(&state, &mut events, "r1", "hmm", None);
        events.clear();
        let state = finish(
            &state,
            &mut events,
            FinishInput {
                reason: FinishReasonDetails::new(FinishReason::Stop),
                usage: Some(Usage { input_tokens: Some(3.0), ..Default::default() }),
                provider_metadata: None,
            },
        );
        assert_eq!(
            events.iter().map(LLMEvent::kind).collect::<Vec<_>>(),
            ["reasoning-end", "text-end", "step-finish", "finish"]
        );
        assert!(!state.step_started);
        assert_eq!(state.text.len() + state.reasoning.len(), 0);
        assert!(
            matches!(&events[3], LLMEvent::Finish { usage, .. } if usage.as_ref().unwrap().input_tokens == Some(3.0))
        );
    }

    #[test]
    fn a_delta_without_a_prior_start_still_seeds_step_start_on_finish() {
        let mut events = Vec::new();
        let state = finish(
            &State::initial(),
            &mut events,
            FinishInput {
                reason: FinishReasonDetails::new(FinishReason::Length),
                usage: None,
                provider_metadata: None,
            },
        );
        assert_eq!(
            events.iter().map(LLMEvent::kind).collect::<Vec<_>>(),
            ["step-start", "step-finish", "finish"]
        );
        assert!(!state.step_started);
    }
}
