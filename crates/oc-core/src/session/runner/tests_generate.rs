//! One-shot generate tests (session/generate.ts semantics): the request
//! carries the session transcript + prompt, and the session is not
//! mutated.

#![allow(clippy::too_many_lines)]

use oc_ai::schema::LLMEvent;

use crate::session::runner::tests_harness::{harness, harness_with};

#[tokio::test]
async fn generate_returns_text_without_mutating_the_session() {
    let llm = oc_ai::testing::TestLlm::new();
    llm.always(oc_ai::testing::text("here is your answer"));
    let harness = harness_with(llm.clone()).await;
    harness.create_session().await;
    // Settle one ordinary exchange first so the transcript is non-empty.
    harness.admit_prompt("msg_user_1", "first question").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution exit");

    let messages_before = harness.messages().await;
    let text = harness
        .runner
        .generation()
        .generate(&harness.session_id().0, "summarize this session")
        .await
        .expect("generation succeeds");
    assert_eq!(text, "here is your answer");

    // No session side effects: the message log is unchanged.
    let messages_after = harness.messages().await;
    assert_eq!(messages_before.len(), messages_after.len());

    // The request saw the transcript and the prompt as the final user
    // message, through the `session.generate` request path.
    let requests = llm.requests();
    let request = requests.last().expect("generate request observed");
    let last = request.messages.last().expect("messages present");
    assert_eq!(last.role, oc_ai::schema::messages::MessageRole::User);
    let text_part = last
        .content
        .iter()
        .find_map(|part| match part {
            oc_ai::schema::messages::ContentPart::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("text part");
    assert_eq!(text_part, "summarize this session");
    // The settled history rides along before the prompt.
    assert!(request.messages.len() >= 2);
}

#[tokio::test]
async fn generate_streams_deltas_into_one_string() {
    let llm = oc_ai::testing::TestLlm::new();
    let reason = oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::Stop);
    llm.push([oc_ai::testing::TestResponse::events(vec![
        LLMEvent::TextDelta { id: "r1".into(), text: "one ".into(), provider_metadata: None },
        LLMEvent::TextDelta { id: "r1".into(), text: "two".into(), provider_metadata: None },
        LLMEvent::StepFinish {
            index: 0.0,
            reason: reason.clone(),
            usage: None,
            provider_metadata: None,
        },
        LLMEvent::Finish { reason, usage: None, provider_metadata: None },
    ])]);
    let harness = harness_with(llm).await;
    harness.create_session().await;
    let text = harness
        .runner
        .generation()
        .generate(&harness.session_id().0, "count")
        .await
        .expect("generation succeeds");
    assert_eq!(text, "one two");
}

#[tokio::test]
async fn generate_fails_typed_for_missing_sessions() {
    let harness = harness().await;
    let error = harness
        .runner
        .generation()
        .generate("ses_missing", "hello")
        .await
        .expect_err("missing session");
    assert!(matches!(error, crate::session::runner::GenerateError::NotFound(_)));
}
