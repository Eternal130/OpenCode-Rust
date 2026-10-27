//! RD-11 + RD-13 pinning tests: compaction interrupt settles immediately
//! (ref compaction.ts:664-672 `Effect.onInterrupt`), and the SSE
//! text-delta batching timer fires on its own 100ms deadline even while
//! the provider stream is quiet (ref publish-llm-event.ts:162-177).

use std::sync::Arc;

use oc_ai::schema::events::LLMEvent;
use oc_ai::schema::messages::LLMRequest;
use oc_ai::schema::AiError;
use oc_ai::testing::{TestLlm, TestResponse};
use oc_schema::session_inbox::{Delivery, Item};
use tokio::sync::Notify;

use super::tests_harness::{
    harness, harness_with, harness_with_client, test_resolved_model, Harness,
};
use crate::session::runner::{CompactMechanism, CompactOutcome, LlmClient, LlmEventStream};

/// Poll the durable log until an event of `type_` appears (bounded).
async fn until_event(harness: &Harness, type_: &str) -> Option<serde_json::Value> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if let Some(found) = harness
            .durable_events()
            .await
            .into_iter()
            .find(|(kind, _, _)| kind == type_)
            .map(|(_, _, data)| data)
        {
            return Some(found);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    None
}

/// Spawns `execution.resume` on a shared harness (the drain runs while
/// the test observes the durable log).
fn spawn_resume(harness: Arc<Harness>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let _ = harness.execution.resume(harness.session_id().as_str()).await;
    })
}

// ---------------------------------------------------------------------------
// RD-11: interrupt mid-compaction settles the running compaction row now
// ---------------------------------------------------------------------------

#[tokio::test]
async fn runner_compaction_interrupt_settles_immediately() {
    // The summary response is scripted, but the stream is held on a gate
    // so the compaction is mid-flight when the interrupt lands.
    let llm = TestLlm::new();
    llm.always(TestResponse::events(vec![
        LLMEvent::StepStart { index: 0.0 },
        LLMEvent::TextStart { id: "t1".into(), provider_metadata: None },
        LLMEvent::TextDelta {
            id: "t1".into(),
            text: "## Objective\n- gated".into(),
            provider_metadata: None,
        },
        LLMEvent::TextEnd { id: "t1".into(), text: None, provider_metadata: None },
        LLMEvent::StepFinish {
            index: 0.0,
            reason: oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::Stop),
            usage: None,
            provider_metadata: None,
        },
        LLMEvent::Finish {
            reason: oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::Stop),
            usage: None,
            provider_metadata: None,
        },
    ]));
    let harness = Arc::new(harness_with(llm).await);
    harness.create_session().await;
    harness.admit_prompt("msg_rd11", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    // Gate every subsequent stream (the compaction's summary request).
    let gate = harness.llm.as_ref().expect("scripted harness").0.gate();
    harness
        .inbox
        .admit(
            "msg_rd11_compact",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    let drain = spawn_resume(Arc::clone(&harness));
    until_event(&harness, "session.compaction.started").await.expect("compaction started");

    // Interrupt mid-compaction and wait out cleanup.
    let interrupted = harness
        .execution
        .interrupt(
            harness.session_id().as_str(),
            crate::session::execution::InterruptOptions {
                await_settlement: true,
                ..Default::default()
            },
        )
        .await;
    assert!(interrupted, "active execution was interrupted");
    gate.release();
    let _ = drain.await;

    // The settle is immediate: a failure event landed (the manual inbox
    // item's own "Compaction cancelled" terminal) ...
    let failed = until_event(&harness, "session.compaction.failed")
        .await
        .expect("compaction settled with a failure event");
    assert_eq!(failed["error"]["type"], serde_json::json!("aborted"));
    assert_eq!(failed["error"]["message"], serde_json::json!("Compaction cancelled"));
    // ... and the transcript carries NO running compaction row.
    let messages = harness.messages().await;
    let running = messages
        .iter()
        .filter(|(_, type_, data)| *type_ == "compaction" && data["status"] == "running")
        .count();
    assert_eq!(running, 0, "no orphaned status=running compaction row after interrupt");
}

// ---------------------------------------------------------------------------
// RD-13: the 100ms delta-batch timer flushes while the stream is quiet
// ---------------------------------------------------------------------------

/// An LLM client whose stream parks between the first delta and the end:
/// `TextStart`, `TextDelta`, <quiet — parked on a Notify>, `TextEnd`,
/// terminal. If the publisher's batching only flushed on the NEXT event,
/// the delta would not be durable until the park is released.
struct ParkedStreamClient {
    release: Arc<Notify>,
}

#[async_trait::async_trait]
impl LlmClient for ParkedStreamClient {
    async fn stream(&self, _request: LLMRequest) -> Result<LlmEventStream, AiError> {
        let release = self.release.clone();
        Ok(Box::pin(async_stream::stream! {
            yield Ok(LLMEvent::StepStart { index: 0.0 });
            yield Ok(LLMEvent::TextStart { id: "t1".into(), provider_metadata: None });
            yield Ok(LLMEvent::TextDelta {
                id: "t1".into(),
                text: "quiet batch".into(),
                provider_metadata: None,
            });
            // Quiet period: no events until the test releases the park.
            release.notified().await;
            yield Ok(LLMEvent::TextEnd { id: "t1".into(), text: None, provider_metadata: None });
            yield Ok(LLMEvent::StepFinish {
                index: 0.0,
                reason: oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            });
            yield Ok(LLMEvent::Finish {
                reason: oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            });
        }))
    }

    fn can_compact(&self, _request: &LLMRequest, _mechanism: CompactMechanism) -> bool {
        false
    }

    async fn compact(
        &self,
        _request: LLMRequest,
        _mechanism: CompactMechanism,
    ) -> Result<CompactOutcome, AiError> {
        Err(AiError::unsupported_operation("compact", "parked client"))
    }
}

#[tokio::test]
async fn runner_delta_batch_flushes_on_its_own_timer() {
    let release = Arc::new(Notify::new());
    let client = ParkedStreamClient { release: Arc::clone(&release) };
    let harness = Arc::new(harness_with_client(Arc::new(client), test_resolved_model()).await);
    harness.create_session().await;
    harness.admit_prompt("msg_rd13", "hi").await;

    // `session.text.delta` is an EPHEMERAL event (not in the durable
    // log) — observe it on a live subscription.
    let mut text_events =
        harness.bus.subscribe_types(&["session.text.started", "session.text.delta"]);
    let drain = spawn_resume(Arc::clone(&harness));

    let mut started_seen = false;
    let delta = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        use futures::StreamExt as _;
        loop {
            let Some(event) = text_events.next().await else { continue };
            match event.type_.as_str() {
                "session.text.started" => started_seen = true,
                "session.text.delta" => return event.data,
                _ => {}
            }
        }
    })
    .await
    .expect("delta flushed while the stream was quiet (started seen first)");
    assert!(started_seen, "text.started arrived before the delta");
    assert_eq!(delta["delta"], serde_json::json!("quiet batch"));

    // Release the park; the turn completes normally.
    release.notify_one();
    let _ = drain.await;
    let events = harness.durable_events().await;
    assert!(events.iter().any(|(kind, _, _)| kind == "session.text.ended"));
    assert!(events.iter().any(|(kind, _, _)| kind == "session.step.ended"));
}

/// Control: an ungated plain turn still batches the same way (one delta
/// frame, unchanged content) — guards against wiring regressions that
/// double-publish or drop batches.
#[tokio::test]
async fn runner_delta_batch_plain_turn_unchanged() {
    let harness = harness().await;
    harness.create_session().await;
    harness.admit_prompt("msg_rd13b", "hi").await;
    let mut deltas = harness.bus.subscribe_types(&["session.text.delta"]);
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    use futures::StreamExt as _;
    let frames: Vec<_> = (&mut deltas).take(1).collect().await;
    assert_eq!(frames.len(), 1, "one coalesced delta frame");
    assert_eq!(frames[0].data["delta"], serde_json::json!("ok"));
}
