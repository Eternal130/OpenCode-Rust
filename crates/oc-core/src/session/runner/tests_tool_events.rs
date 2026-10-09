//! Ported unit tests: `session-runner-tool-events` + the publisher record
//! (`session-runner`) — tool lifecycle events, defects, settlement capture.

use oc_ai::schema::events::{FinishReason, FinishReasonDetails, LLMEvent, Usage};
use oc_ai::schema::ProviderFailureClassification;
use oc_schema::common::{FinishReason as WireFinish, SessionError};
use oc_schema::id::{AgentId, MessageId};

use super::tests_harness::{harness, Harness};
use crate::session::runner::publisher::{LlmEventPublisher, SettlementDetails};

fn started_details() -> FinishReasonDetails {
    FinishReasonDetails::new(FinishReason::Stop)
}

pub(crate) fn publisher(harness: &Harness, message: &str) -> LlmEventPublisher {
    LlmEventPublisher::new(
        harness.bus.clone(),
        harness.session_id().as_str().to_string(),
        MessageId::parse(message).unwrap(),
        AgentId::parse("build"),
        oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("parityfake"),
            id: oc_schema::id::ModelId::parse("fake-model"),
            variant: None,
        },
        "parityfake",
        None,
        0,
    )
}

fn tool_call(id: &str, name: &str) -> LLMEvent {
    LLMEvent::ToolCall {
        id: id.into(),
        name: name.into(),
        namespace: None,
        input: serde_json::json!({ "text": "hi" }),
        provider_executed: None,
        provider_metadata: None,
    }
}

#[tokio::test]
async fn runner_tool_events_lifecycle() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_tool_1");

    publisher
        .publish(&LLMEvent::ToolInputStart {
            id: "call_1".into(),
            name: "echo".into(),
            namespace: None,
            provider_executed: None,
            provider_metadata: None,
        })
        .await
        .unwrap();
    publisher
        .publish(&LLMEvent::ToolInputDelta {
            id: "call_1".into(),
            name: "echo".into(),
            namespace: None,
            text: "{\"text\":".into(),
            input: None,
        })
        .await
        .unwrap();
    publisher
        .publish(&LLMEvent::ToolInputEnd {
            id: "call_1".into(),
            name: "echo".into(),
            namespace: None,
            provider_metadata: None,
        })
        .await
        .unwrap();
    publisher.publish(&tool_call("call_1", "echo")).await.unwrap();
    publisher
        .tool_execution(
            "call_1",
            "echo",
            &crate::location::ToolRunResult {
                content: vec![oc_schema::tool::Content::Text { text: "echo: ok".into() }],
                metadata: None,
            },
        )
        .await
        .unwrap();
    publisher
        .publish(&LLMEvent::StepFinish {
            index: 0.0,
            reason: started_details(),
            usage: None,
            provider_metadata: None,
        })
        .await
        .unwrap();

    let types: Vec<String> =
        harness.durable_events().await.iter().map(|(type_, _, _)| type_.clone()).collect();
    assert_eq!(
        types,
        vec![
            "session.created",
            "session.step.started",
            "session.tool.input.started",
            "session.tool.input.ended",
            "session.tool.called",
            "session.tool.success",
        ]
    );
    // The projected assistant carries the completed tool part.
    let messages = harness.messages().await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1, "assistant");
    let tool = &messages[0].2["content"][0];
    assert_eq!(tool["state"]["status"], serde_json::json!("completed"));
    assert_eq!(tool["state"]["content"][0]["text"], serde_json::json!("echo: ok"));
}

#[tokio::test]
async fn runner_tool_events_duplicate_call_is_a_defect() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_dup");
    publisher.publish(&tool_call("call_1", "echo")).await.unwrap();
    let defect = publisher.publish(&tool_call("call_1", "echo")).await;
    assert!(defect.is_err());
    assert_eq!(
        defect.unwrap_err().0,
        "Duplicate tool call: call_1",
        "the mark lands before the second publish"
    );
}

#[tokio::test]
async fn runner_tool_events_result_before_call_is_a_defect() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_early");
    let defect = publisher
        .publish(&LLMEvent::ToolResult {
            id: "call_9".into(),
            name: "echo".into(),
            namespace: None,
            result: oc_ai::schema::messages::ToolResultValue::Text {
                value: serde_json::json!("early"),
            },
            output: None,
            provider_executed: None,
            provider_metadata: None,
        })
        .await;
    assert!(defect.is_err());
    assert!(defect.unwrap_err().0.contains("Tool result before call"));
}

#[tokio::test]
async fn runner_tool_events_malformed_input_settles_with_input_json_error() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_malformed");
    publisher
        .publish(&LLMEvent::ToolInputError {
            id: "call_m".into(),
            name: "echo".into(),
            namespace: None,
            raw: "{\"broken".into(),
        })
        .await
        .unwrap();
    let events = harness.durable_events().await;
    let failed = events.iter().find(|(type_, _, _)| type_ == "session.tool.failed").unwrap();
    assert_eq!(failed.2["error"]["type"], serde_json::json!("tool.input-json"));
    assert_eq!(
        failed.2["error"]["message"],
        serde_json::json!(
            "Tool call arguments were malformed JSON and were not executed. Retry with valid JSON."
        )
    );
    assert_eq!(failed.2["executed"], serde_json::json!(false));
}

#[tokio::test]
async fn runner_tool_events_hosted_result_error_and_success() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_hosted");
    publisher.publish(&tool_call("call_h", "echo")).await.unwrap();
    publisher
        .publish(&LLMEvent::ToolResult {
            id: "call_h".into(),
            name: "echo".into(),
            namespace: None,
            result: oc_ai::schema::messages::ToolResultValue::Error {
                value: serde_json::json!("host blew up"),
            },
            output: None,
            provider_executed: Some(true),
            provider_metadata: None,
        })
        .await
        .unwrap();
    let events = harness.durable_events().await;
    let failed = events.iter().find(|(type_, _, _)| type_ == "session.tool.failed").unwrap();
    assert_eq!(failed.2["error"]["type"], serde_json::json!("tool.execution"));
    assert_eq!(failed.2["error"]["message"], serde_json::json!("host blew up"));
    assert_eq!(failed.2["executed"], serde_json::json!(true));
}

#[tokio::test]
async fn runner_tool_events_late_error_result_is_a_benign_straggler() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_straggler");
    publisher.publish(&tool_call("call_s", "echo")).await.unwrap();
    publisher
        .tool_execution(
            "call_s",
            "echo",
            &crate::location::ToolRunResult {
                content: vec![oc_schema::tool::Content::Text { text: "done".into() }],
                metadata: None,
            },
        )
        .await
        .unwrap();
    // The late error result is swallowed; a late success would die.
    publisher
        .publish(&LLMEvent::ToolResult {
            id: "call_s".into(),
            name: "echo".into(),
            namespace: None,
            result: oc_ai::schema::messages::ToolResultValue::Error {
                value: serde_json::json!("late"),
            },
            output: None,
            provider_executed: None,
            provider_metadata: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn runner_tool_events_unknown_tool_error_type() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_unknown");
    publisher.publish(&tool_call("call_u", "echo")).await.unwrap();
    publisher
        .publish(&LLMEvent::ToolError {
            id: "call_u".into(),
            name: "echo".into(),
            namespace: None,
            message: "Unknown tool: echo".into(),
            error: None,
            provider_metadata: None,
        })
        .await
        .unwrap();
    let events = harness.durable_events().await;
    let failed = events.iter().find(|(type_, _, _)| type_ == "session.tool.failed").unwrap();
    assert_eq!(failed.2["error"]["type"], serde_json::json!("tool.unknown"));
}

#[tokio::test]
async fn runner_tool_events_subagent_failure_appends_child_session() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_sub");
    publisher
        .publish(&LLMEvent::ToolInputStart {
            id: "call_sub".into(),
            name: "subagent".into(),
            namespace: None,
            provider_executed: None,
            provider_metadata: None,
        })
        .await
        .unwrap();
    publisher.publish(&tool_call("call_sub", "subagent")).await.unwrap();
    publisher
        .progress(
            "call_sub",
            oc_schema::common::Record::from([(
                "sessionID".to_string(),
                serde_json::json!("ses_child_1"),
            )]),
        )
        .await
        .unwrap();
    let settled = publisher
        .fail_tool(
            "call_sub",
            SessionError {
                error_type: "aborted".into(),
                message: "Tool execution interrupted".into(),
                status: None,
                response: None,
            },
            None,
        )
        .await
        .unwrap();
    assert!(settled);
    let events = harness.durable_events().await;
    let failed = events.iter().find(|(type_, _, _)| type_ == "session.tool.failed").unwrap();
    assert_eq!(
        failed.2["error"]["message"],
        serde_json::json!("Tool execution interrupted (sessionID: ses_child_1)")
    );
    assert_eq!(failed.2["metadata"]["sessionID"], serde_json::json!("ses_child_1"));
}

#[tokio::test]
async fn runner_publisher_step_settlement_captures_finish_and_usage() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_settle");
    publisher
        .publish(&LLMEvent::TextStart { id: "t1".into(), provider_metadata: None })
        .await
        .unwrap();
    publisher
        .publish(&LLMEvent::TextDelta {
            id: "t1".into(),
            text: "Hello".into(),
            provider_metadata: None,
        })
        .await
        .unwrap();
    publisher
        .publish(&LLMEvent::TextEnd { id: "t1".into(), text: None, provider_metadata: None })
        .await
        .unwrap();
    let usage = Usage {
        input_tokens: Some(11.0),
        non_cached_input_tokens: Some(11.0),
        output_tokens: Some(7.0),
        ..Default::default()
    };
    publisher
        .publish(&LLMEvent::StepFinish {
            index: 0.0,
            reason: started_details(),
            usage: Some(usage),
            provider_metadata: None,
        })
        .await
        .unwrap();
    publisher.streamed().await.unwrap();
    let record = publisher.record();
    let settlement = record.finish.expect("settlement");
    assert_eq!(settlement.finish, WireFinish::Stop);
    assert_eq!(settlement.tokens.input.0, 11.0);
    assert_eq!(settlement.tokens.output.0, 7.0);
    assert!(record.output_started);
    assert!(!record.needs_continuation);
    let settlement = publisher.record().finish.expect("settlement");
    publisher
        .publish_step_ended(&crate::session::runner::publisher::SettlementDetails {
            cost: Some(oc_schema::common::Usd(oc_schema::common::Num(0.0))),
            tokens: Some(settlement.tokens.clone()),
            snapshot: None,
            files: None,
        })
        .await
        .unwrap();
    let events = harness.durable_events().await;
    let ended = events.iter().find(|(type_, _, _)| type_ == "session.step.ended").unwrap();
    assert_eq!(ended.2["finish"], serde_json::json!("stop"));
}

#[tokio::test]
async fn runner_publisher_needs_continuation_tracks_unsettled_tools() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_cont");
    publisher.publish(&tool_call("call_1", "echo")).await.unwrap();
    let record = publisher.record();
    assert!(record.needs_continuation, "a called-but-unsettled local tool needs continuation");
    publisher
        .tool_execution(
            "call_1",
            "echo",
            &crate::location::ToolRunResult {
                content: vec![oc_schema::tool::Content::Text { text: "ok".into() }],
                metadata: None,
            },
        )
        .await
        .unwrap();
    assert!(publisher.record().needs_continuation, "settled-but-local still continues");
}

#[tokio::test]
async fn runner_publisher_provider_error_fails_the_assistant() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_perr");
    publisher
        .publish(&LLMEvent::ProviderError {
            message: "500 from provider".into(),
            classification: None,
            provider_metadata: None,
        })
        .await
        .unwrap();
    assert!(publisher.has_provider_error());
    let record = publisher.record();
    assert_eq!(record.failure.as_ref().unwrap().error_type, "provider.unknown");
    publisher.publish_step_failure(&SettlementDetails::default()).await.unwrap();
    let events = harness.durable_events().await;
    let failed = events.iter().find(|(type_, _, _)| type_ == "session.step.failed").unwrap();
    assert_eq!(failed.2["error"]["type"], serde_json::json!("provider.unknown"));
}

#[tokio::test]
async fn runner_publisher_content_filter_finish_fails_with_category() {
    let harness = harness().await;
    harness.create_session().await;
    let mut publisher = publisher(&harness, "msg_cf");
    let mut details = FinishReasonDetails::new(FinishReason::ContentFilter);
    details.category = Some("cyber".into());
    publisher
        .publish(&LLMEvent::StepFinish {
            index: 0.0,
            reason: details,
            usage: None,
            provider_metadata: None,
        })
        .await
        .unwrap();
    assert!(publisher.has_provider_error());
    let record = publisher.record();
    assert_eq!(record.failure.as_ref().unwrap().message, "Provider blocked the response (cyber)");
    publisher.publish_step_failure(&SettlementDetails::default()).await.unwrap();
    let events = harness.durable_events().await;
    let failed = events.iter().find(|(type_, _, _)| type_ == "session.step.failed").unwrap();
    // The content-filter marker now rides `finish` (Wave C1 oc-schema
    // round-trip fix; the TS wire keeps `finish: "content-filter"`).
    assert_eq!(failed.2["finish"], serde_json::json!("content-filter"));
}

#[tokio::test]
async fn runner_publisher_overflow_error_is_parkable() {
    // The parked-overflow behavior is exercised end-to-end in tests_step;
    // here the classification predicate is the unit under test.
    let error = oc_ai::schema::AiError::invalid_request("context too long");
    let classified = {
        let mut error = error.clone();
        if let oc_ai::schema::AiErrorReason::InvalidRequest { classification, .. } =
            &mut error.reason
        {
            *classification = Some(ProviderFailureClassification::ContextOverflow);
        }
        error
    };
    assert!(crate::session::runner::is_context_overflow_failure(&classified));
}
