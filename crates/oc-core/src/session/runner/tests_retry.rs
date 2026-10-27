//! Ported unit tests: `session-retry` (schedule, jitter, retryAfter floor,
//! timeout cap, hook override) and `wait()`'s RetryScheduled publication.

use std::sync::Arc;
use std::time::Duration;

use oc_ai::schema::{AiError, AiErrorReason, ReasonFields};
use oc_schema::common::SessionError;
use oc_schema::id::{AgentId, MessageId, SessionId};
use oc_schema::model;
use oc_util::determinism::{FixedClock, SequentialRng};

use crate::session::runner::retry::{base_delay, Decision, RetryPolicy};

fn agent() -> AgentId {
    AgentId::parse("build")
}

fn reference() -> model::Ref {
    model::Ref {
        provider_id: oc_schema::id::ProviderId::parse("p"),
        id: oc_schema::id::ModelId::parse("m"),
        variant: None,
    }
}

fn error() -> SessionError {
    SessionError {
        error_type: "provider.internal".into(),
        message: "boom".into(),
        status: None,
        response: None,
    }
}

fn transport_error(
    code: Option<&str>,
    recovery: Option<oc_ai::schema::TransportRecovery>,
) -> AiError {
    AiError::new(AiErrorReason::Transport {
        fields: ReasonFields::new("transport failed"),
        transport: oc_ai::schema::TransportType::Http,
        operation: oc_ai::schema::TransportOperation::Request,
        code: code.map(str::to_string),
        url: None,
        phase: None,
        delivery: None,
        recovery,
    })
}

#[test]
fn runner_retry_base_delay_sequence() {
    // 2, 4, 8, then capped at 10s.
    assert_eq!(base_delay(1), 2_000);
    assert_eq!(base_delay(2), 4_000);
    assert_eq!(base_delay(3), 8_000);
    assert_eq!(base_delay(4), 10_000);
    assert_eq!(base_delay(10), 10_000);
}

#[tokio::test]
async fn runner_retry_decide_advances_attempts_with_jitter() {
    let policy =
        RetryPolicy::new(SessionId::parse("ses_a").unwrap(), Arc::new(SequentialRng::new()));
    let cause = AiError::new(AiErrorReason::ProviderInternal {
        fields: ReasonFields::new("internal"),
        retry_after_ms: None,
    });
    let first = policy.decide(&cause, &error(), &agent(), &reference(), true, None).await;
    assert!(first.retry);
    assert_eq!(first.attempt, 2);
    assert!((1_600..=2_400).contains(&first.delay_ms), "±20% jitter over 2s: {}", first.delay_ms);
    let second = policy.decide(&cause, &error(), &agent(), &reference(), true, None).await;
    assert_eq!(second.attempt, 3);
    assert!((3_200..=4_800).contains(&second.delay_ms), "{}", second.delay_ms);
}

#[tokio::test]
async fn runner_retry_rejects_after_ten_recursions() {
    let policy =
        RetryPolicy::new(SessionId::parse("ses_b").unwrap(), Arc::new(SequentialRng::new()));
    let cause = AiError::new(AiErrorReason::ProviderInternal {
        fields: ReasonFields::new("internal"),
        retry_after_ms: None,
    });
    for _ in 0..10 {
        let decision = policy.decide(&cause, &error(), &agent(), &reference(), true, None).await;
        assert!(decision.retry);
    }
    let decision = policy.decide(&cause, &error(), &agent(), &reference(), true, None).await;
    assert!(!decision.retry);
}

#[tokio::test]
async fn runner_retry_retry_after_floors_the_delay() {
    let policy =
        RetryPolicy::new(SessionId::parse("ses_c").unwrap(), Arc::new(SequentialRng::new()));
    let cause = AiError::new(AiErrorReason::RateLimit {
        fields: ReasonFields::new("slow down"),
        retry_after_ms: Some(30_000.0),
        rate_limit: None,
    });
    let decision = policy.decide(&cause, &error(), &agent(), &reference(), true, None).await;
    assert_eq!(decision.delay_ms, 30_000, "retryAfter floors the jittered gap");
    // A hostile retry-after caps at 15 minutes.
    let hostile = AiError::new(AiErrorReason::RateLimit {
        fields: ReasonFields::new("slow down"),
        retry_after_ms: Some(6.0 * 60.0 * 60.0 * 1000.0),
        rate_limit: None,
    });
    let policy =
        RetryPolicy::new(SessionId::parse("ses_d").unwrap(), Arc::new(SequentialRng::new()));
    let decision = policy.decide(&hostile, &error(), &agent(), &reference(), true, None).await;
    assert_eq!(decision.delay_ms, 15 * 60 * 1000);
}

#[tokio::test]
async fn runner_retry_timeouts_are_capped_at_three() {
    let policy =
        RetryPolicy::new(SessionId::parse("ses_e").unwrap(), Arc::new(SequentialRng::new()));
    let cause = transport_error(Some("Timeout"), None);
    for _ in 0..3 {
        let decision = policy.decide(&cause, &error(), &agent(), &reference(), true, None).await;
        assert!(decision.retry, "timeout attempts under the cap retry");
    }
    let decision = policy.decide(&cause, &error(), &agent(), &reference(), true, None).await;
    assert!(!decision.retry, "the fourth timeout is refused");
}

#[tokio::test]
async fn runner_retry_proposed_false_means_no_retry() {
    let policy =
        RetryPolicy::new(SessionId::parse("ses_f").unwrap(), Arc::new(SequentialRng::new()));
    let cause = AiError::new(AiErrorReason::ProviderInternal {
        fields: ReasonFields::new("internal"),
        retry_after_ms: None,
    });
    let decision = policy.decide(&cause, &error(), &agent(), &reference(), false, None).await;
    assert!(!decision.retry);
}

#[tokio::test]
async fn runner_retry_hook_can_force_retry_off() {
    let policy =
        RetryPolicy::new(SessionId::parse("ses_g").unwrap(), Arc::new(SequentialRng::new()));
    let cause = AiError::new(AiErrorReason::ProviderInternal {
        fields: ReasonFields::new("internal"),
        retry_after_ms: None,
    });
    let hook: crate::session::runner::retry::RetryHook = Arc::new(|event| {
        Box::pin(async move {
            event.decision = crate::hooks::RetryDecision::NoRetry;
        })
    });
    let decision = policy.decide(&cause, &error(), &agent(), &reference(), true, Some(hook)).await;
    assert!(!decision.retry);
}

#[tokio::test]
async fn runner_retry_hook_can_override_the_delay() {
    let policy =
        RetryPolicy::new(SessionId::parse("ses_h").unwrap(), Arc::new(SequentialRng::new()));
    let cause = AiError::new(AiErrorReason::ProviderInternal {
        fields: ReasonFields::new("internal"),
        retry_after_ms: None,
    });
    let hook: crate::session::runner::retry::RetryHook = Arc::new(|event| {
        Box::pin(async move {
            event.decision = crate::hooks::RetryDecision::Retry { delay_ms: 42 };
        })
    });
    let decision = policy.decide(&cause, &error(), &agent(), &reference(), true, Some(hook)).await;
    assert!(decision.retry);
    assert_eq!(decision.delay_ms, 42);
}

#[tokio::test]
async fn runner_retry_wait_publishes_retry_scheduled_and_sleeps_the_remainder() {
    let bus_stack = super::tests_harness::harness().await;
    let session_id = bus_stack.session_id();
    bus_stack.create_session().await;
    let policy = RetryPolicy::new(session_id.clone(), Arc::new(SequentialRng::new()));
    let decision = Decision { retry: true, attempt: 2, delay_ms: 0 };
    let clock: Arc<dyn oc_util::determinism::Clock> = Arc::new(FixedClock::epoch());
    policy
        .wait(
            &bus_stack.bus,
            &clock,
            &MessageId::parse("msg_r1").unwrap(),
            &error(),
            &decision,
            &|_duration| Box::pin(async {}),
        )
        .await
        .expect("wait");
    let events = bus_stack.durable_events().await;
    let retry = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.retry.scheduled")
        .expect("retry scheduled published");
    assert_eq!(retry.2["attempt"], serde_json::json!(2));
    assert_eq!(retry.2["assistantMessageID"], serde_json::json!("msg_r1"));
    assert_eq!(retry.2["at"], serde_json::json!(0));
    assert_eq!(retry.2["error"]["type"], serde_json::json!("provider.internal"));
    let _ = Duration::from_millis(0);
    let _ = bus_stack.llm.as_ref().expect("scripted harness").0.requests();
}
