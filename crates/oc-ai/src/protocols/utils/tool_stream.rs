//! Tool-call JSON accumulation across stream deltas (port of
//! `src/protocols/utils/tool-stream.ts`, tests from
//! `tool-stream.test.ts`). Providers emit tool identity and JSON argument
//! text across separate chunks; this module accumulates the raw string and
//! emits `tool-input-*` lifecycle events, parsing partially at each delta.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::protocols::shared::event_error;
use crate::protocols::utils::partial_json::{self, allow};
use crate::schema::{AiError, LLMEvent, ProviderMetadata};

/// One pending streamed tool call: raw JSON input string collected so far.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingTool {
    pub id: String,
    pub name: String,
    pub namespace: Option<String>,
    pub input: String,
    pub provider_executed: Option<bool>,
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Sparse parser state keyed by the provider's stream-local tool identifier
/// (numeric content index for `OpenAI` Chat/Anthropic/Bedrock; string
/// `item_id` for `OpenAI` Responses).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    tools: BTreeMap<String, PendingTool>,
}

impl State {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PendingTool> {
        self.tools.get(key)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn values(&self) -> impl Iterator<Item = &PendingTool> {
        self.tools.values()
    }
}

/// Result of adding argument text to one pending tool call: next state,
/// updated tool, and lifecycle/delta events from the append.
#[derive(Debug, Clone)]
pub struct AppendOutcome {
    pub tools: State,
    pub tool: PendingTool,
    pub events: Vec<LLMEvent>,
}

/// Finish result: remaining state and the emitted events.
#[derive(Debug, Clone)]
pub struct FinishOutcome {
    pub tools: State,
    pub events: Vec<LLMEvent>,
}

fn partial_input(raw: &str) -> Value {
    partial_json::parse(raw, allow::ALL).unwrap_or(Value::Object(Default::default()))
}

fn input_start(tool: &PendingTool) -> LLMEvent {
    LLMEvent::ToolInputStart {
        id: tool.id.clone(),
        name: tool.name.clone(),
        namespace: tool.namespace.clone(),
        provider_executed: tool.provider_executed.map(|_| true),
        provider_metadata: tool.provider_metadata.clone(),
    }
}

fn input_delta(tool: &PendingTool, text: &str) -> LLMEvent {
    LLMEvent::ToolInputDelta {
        id: tool.id.clone(),
        name: tool.name.clone(),
        namespace: tool.namespace.clone(),
        text: text.to_string(),
        input: Some(partial_input(&tool.input)),
    }
}

fn tool_call_event(
    route: &str,
    tool: &PendingTool,
    input_override: Option<&str>,
) -> Result<LLMEvent, AiError> {
    let raw = input_override.unwrap_or(&tool.input);
    let parsed = match crate::protocols::shared::parse_tool_input(route, &tool.name, raw) {
        Ok(value) => value,
        Err(error) => {
            if tool.provider_executed == Some(true) {
                return Err(error);
            }
            partial_json::parse(raw, allow::ALL).unwrap_or(Value::Object(Default::default()))
        }
    };
    Ok(LLMEvent::ToolCall {
        id: tool.id.clone(),
        name: tool.name.clone(),
        namespace: tool.namespace.clone(),
        input: parsed,
        provider_executed: tool.provider_executed.map(|_| true),
        provider_metadata: tool.provider_metadata.clone(),
    })
}

fn finish_events(tool: &PendingTool, event: LLMEvent) -> Vec<LLMEvent> {
    vec![
        LLMEvent::ToolInputEnd {
            id: tool.id.clone(),
            name: tool.name.clone(),
            namespace: tool.namespace.clone(),
            provider_metadata: tool.provider_metadata.clone(),
        },
        event,
    ]
}

/// A delta carrying identity for providers that encode it on the first delta
/// (`OpenAI` Chat `tool_calls[].index`).
pub struct Delta<'a> {
    pub id: Option<&'a str>,
    pub name: Option<&'a str>,
    pub text: &'a str,
    pub provider_metadata: Option<ProviderMetadata>,
}

/// Register a tool whose start event arrived before any argument deltas
/// (Anthropic `content_block_start`, `OpenAI` Responses `output_item.added`).
#[must_use]
pub fn start(tools: &State, key: &str, tool: PendingTool) -> State {
    let mut next = tools.clone();
    next.tools.insert(key.to_string(), tool);
    next
}

/// Append a streamed argument delta, starting the tool if this provider
/// encodes identity on the first delta instead of a separate start event.
pub fn append_or_start(
    route: &str,
    tools: &State,
    key: &str,
    delta: Delta<'_>,
    missing_tool_message: &str,
) -> Result<AppendOutcome, AiError> {
    let current = tools.get(key);
    let id = current.map(|tool| tool.id.as_str()).or(delta.id);
    let name = current.map(|tool| tool.name.as_str()).or(delta.name);
    let (Some(id), Some(name)) = (id, name) else {
        return Err(event_error(route, missing_tool_message));
    };
    let accumulated_input =
        format!("{}{}", current.map_or("", |tool| tool.input.as_str()), delta.text);
    let tool = PendingTool {
        id: id.to_string(),
        name: name.to_string(),
        namespace: current.and_then(|tool| tool.namespace.clone()),
        input: accumulated_input,
        provider_executed: current.and_then(|tool| tool.provider_executed),
        provider_metadata: current
            .and_then(|tool| tool.provider_metadata.clone())
            .or(delta.provider_metadata),
    };
    if let Some(current) = current {
        if delta.text.is_empty() && current.id == tool.id && current.name == tool.name {
            return Ok(AppendOutcome {
                tools: tools.clone(),
                tool: current.clone(),
                events: Vec::new(),
            });
        }
    }
    let mut events = Vec::new();
    if current.is_none() {
        events.push(input_start(&tool));
    }
    if !delta.text.is_empty() {
        events.push(input_delta(&tool, delta.text));
    }
    let mut next = tools.clone();
    next.tools.insert(key.to_string(), tool.clone());
    Ok(AppendOutcome { tools: next, tool, events })
}

/// Append argument text to a started tool; `None` when no tool is open under
/// `key` (protocols that ignore orphan deltas).
#[must_use]
pub fn append(tools: &State, key: &str, text: &str) -> Option<AppendOutcome> {
    let current = tools.get(key)?;
    if text.is_empty() {
        return Some(AppendOutcome {
            tools: tools.clone(),
            tool: current.clone(),
            events: Vec::new(),
        });
    }
    let tool = PendingTool { input: format!("{}{}", current.input, text), ..current.clone() };
    let events = vec![input_delta(&tool, text)];
    let mut next = tools.clone();
    next.tools.insert(key.to_string(), tool.clone());
    Some(AppendOutcome { tools: next, tool, events })
}

/// Append argument text to a tool that must already have been started —
/// keeps protocols honest when their stream grammar promises a start event.
pub fn append_existing(
    route: &str,
    tools: &State,
    key: &str,
    text: &str,
    missing_tool_message: &str,
) -> Result<AppendOutcome, AiError> {
    append(tools, key, text).ok_or_else(|| event_error(route, missing_tool_message))
}

/// Finalize one pending tool call (port of `finish`): parse the accumulated
/// JSON, remove it from state. Missing keys are a no-op because some
/// providers emit stop events for non-tool content blocks.
pub fn finish(route: &str, tools: &State, key: &str) -> Result<FinishOutcome, AiError> {
    let Some(tool) = tools.get(key) else {
        return Ok(FinishOutcome { tools: tools.clone(), events: Vec::new() });
    };
    let mut next = tools.clone();
    next.tools.remove(key);
    let call = tool_call_event(route, tool, None)?;
    Ok(FinishOutcome { tools: next, events: finish_events(tool, call) })
}

/// Finalize with an authoritative final input string (port of
/// `finishWithInput`): `OpenAI` Responses repeats the completed arguments on
/// `response.output_item.done`; the final value wins.
pub fn finish_with_input(
    route: &str,
    tools: &State,
    key: &str,
    input: &str,
) -> Result<FinishOutcome, AiError> {
    let Some(tool) = tools.get(key) else {
        return Ok(FinishOutcome { tools: tools.clone(), events: Vec::new() });
    };
    let mut next = tools.clone();
    next.tools.remove(key);
    let call = tool_call_event(route, tool, Some(input))?;
    Ok(FinishOutcome { tools: next, events: finish_events(tool, call) })
}

/// Finalize every pending call at once (port of `finishAll`): `OpenAI` Chat
/// emits no per-tool stop events, so all calls finish at the terminal
/// `finish_reason`.
pub fn finish_all(route: &str, tools: &State) -> Result<FinishOutcome, AiError> {
    let mut events = Vec::new();
    for tool in tools.values() {
        let call = tool_call_event(route, tool, None)?;
        events.extend(finish_events(tool, call));
    }
    Ok(FinishOutcome { tools: State::new(), events })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADAPTER: &str = "test-route";

    fn delta<'a>(id: Option<&'a str>, name: Option<&'a str>, text: &'a str) -> Delta<'a> {
        Delta { id, name, text, provider_metadata: None }
    }

    #[test]
    fn starts_from_openai_style_deltas_and_finalizes_parsed_input() {
        let first = append_or_start(
            ADAPTER,
            &State::new(),
            "0",
            delta(Some("call_1"), Some("lookup"), r#"{"query""#),
            "missing tool",
        )
        .unwrap();
        assert_eq!(
            first.events,
            vec![
                LLMEvent::ToolInputStart {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    provider_executed: None,
                    provider_metadata: None
                },
                LLMEvent::ToolInputDelta {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    text: r#"{"query""#.into(),
                    input: Some(serde_json::json!({}))
                },
            ]
        );
        let second = append_or_start(
            ADAPTER,
            &first.tools,
            "0",
            delta(None, None, r#":"weather"}"#),
            "missing tool",
        )
        .unwrap();
        assert_eq!(
            second.events,
            vec![LLMEvent::ToolInputDelta {
                id: "call_1".into(),
                name: "lookup".into(),
                namespace: None,
                text: r#":"weather"}"#.into(),
                input: Some(serde_json::json!({"query": "weather"})),
            }]
        );
        let finished = finish(ADAPTER, &second.tools, "0").unwrap();
        assert_eq!(
            finished.events,
            vec![
                LLMEvent::ToolInputEnd {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    provider_metadata: None
                },
                LLMEvent::ToolCall {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    input: serde_json::json!({"query": "weather"}),
                    provider_executed: None,
                    provider_metadata: None
                },
            ]
        );
        assert!(finished.tools.is_empty());
    }

    #[test]
    fn exposes_cumulative_partial_string_values() {
        let result = append_or_start(
            ADAPTER,
            &State::new(),
            "0",
            delta(Some("call_1"), Some("lookup"), r#"{"query":"wea"#),
            "missing tool",
        )
        .unwrap();
        match result.events.last() {
            Some(LLMEvent::ToolInputDelta { input, .. }) => {
                assert_eq!(input, &Some(serde_json::json!({"query": "wea"})));
            }
            other => panic!("expected delta, got {other:?}"),
        }
    }

    #[test]
    fn defaults_partial_input_to_an_empty_object_when_unparseable() {
        let result = append_or_start(
            ADAPTER,
            &State::new(),
            "0",
            delta(Some("call_1"), Some("lookup"), "x"),
            "missing tool",
        )
        .unwrap();
        assert_eq!(
            result.events,
            vec![
                LLMEvent::ToolInputStart {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    provider_executed: None,
                    provider_metadata: None
                },
                LLMEvent::ToolInputDelta {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    text: "x".into(),
                    input: Some(serde_json::json!({}))
                },
            ]
        );
    }

    #[test]
    fn keeps_accumulated_identity_when_later_deltas_have_empty_strings() {
        let first = append_or_start(
            ADAPTER,
            &State::new(),
            "0",
            delta(Some("call_1"), Some("lookup"), r#"{"query""#),
            "missing tool",
        )
        .unwrap();
        let second = append_or_start(
            ADAPTER,
            &first.tools,
            "0",
            delta(Some(""), Some(""), r#":"weather"}"#),
            "missing tool",
        )
        .unwrap();
        // empty id/name fall back to the accumulated identity
        assert_eq!(second.tool.id, "call_1");
        assert_eq!(second.tool.name, "lookup");
        let finished = finish(ADAPTER, &second.tools, "0").unwrap();
        assert_eq!(
            finished.events,
            vec![
                LLMEvent::ToolInputEnd {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    provider_metadata: None
                },
                LLMEvent::ToolCall {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    input: serde_json::json!({"query": "weather"}),
                    provider_executed: None,
                    provider_metadata: None
                },
            ]
        );
    }

    #[test]
    fn empty_identity_free_deltas_without_start_events() {
        let result =
            append_or_start(ADAPTER, &State::new(), "0", delta(None, None, "{}"), "missing tool")
                .unwrap_err();
        assert_eq!(result.message(), "missing tool");
    }

    #[test]
    fn fails_append_existing_when_the_provider_skipped_the_tool_start() {
        let error = append_existing(ADAPTER, &State::new(), "0", "{}", "missing tool").unwrap_err();
        assert_eq!(error.message(), "missing tool");
    }

    #[test]
    fn uses_final_input_override_without_losing_accumulated_deltas() {
        let tools = start(
            &State::new(),
            "item_1",
            PendingTool {
                id: "call_1".into(),
                name: "lookup".into(),
                namespace: None,
                input: r#"{"query":"partial"}"#.into(),
                provider_executed: None,
                provider_metadata: None,
            },
        );
        let finished =
            finish_with_input(ADAPTER, &tools, "item_1", r#"{"query":"final"}"#).unwrap();
        assert!(finished.tools.is_empty());
        assert_eq!(
            finished.events,
            vec![
                LLMEvent::ToolInputEnd {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    provider_metadata: None
                },
                LLMEvent::ToolCall {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    namespace: None,
                    input: serde_json::json!({"query": "final"}),
                    provider_executed: None,
                    provider_metadata: None
                },
            ]
        );
    }

    #[test]
    fn finish_all_finalizes_every_pending_call() {
        let tools = start(
            &start(
                &State::new(),
                "0",
                PendingTool {
                    id: "call_1".into(),
                    name: "a".into(),
                    namespace: None,
                    input: "{}".into(),
                    provider_executed: None,
                    provider_metadata: None,
                },
            ),
            "1",
            PendingTool {
                id: "call_2".into(),
                name: "b".into(),
                namespace: None,
                input: "{}".into(),
                provider_executed: None,
                provider_metadata: None,
            },
        );
        let finished = finish_all(ADAPTER, &tools).unwrap();
        assert!(finished.tools.is_empty());
        assert_eq!(finished.events.len(), 4); // end + call per tool
        let calls: Vec<&LLMEvent> =
            finished.events.iter().filter(|e| matches!(e, LLMEvent::ToolCall { .. })).collect();
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn finish_is_a_no_op_for_missing_keys() {
        let finished = finish(ADAPTER, &State::new(), "0").unwrap();
        assert!(finished.events.is_empty());
        assert!(finished.tools.is_empty());
    }
}
