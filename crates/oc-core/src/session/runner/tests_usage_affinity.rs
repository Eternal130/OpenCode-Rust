//! RB-09/RB-10a targeted parity tests for two uncovered session-runner
//! subdomains:
//!
//! - **Usage billing fan-out** (RB-09a): a priced model + scripted LLM
//!   usage reports flow through the step settlement (`SessionUsage::record`
//!   / `calculate_cost` / `add` — step.rs:531, usage.ts:22-56) into the
//!   assistant message and the accumulated `session_v2` counters,
//!   hand-computed against the ref semantics.
//! - **Session affinity** (RB-09b): the `x-session-affinity` /
//!   `X-Session-Id` / `x-opencode-session` headers + `promptCacheKey`
//!   (model-request.ts:261-278, affinity.ts:7-8) stick to one key per
//!   session and re-key when the parent link appears.
//! - **RB-10a**: `calculateCost` tier tie-break — equal tier sizes keep
//!   the FIRST entry in the original order (ref usage.ts:24-26 uses a
//!   stable descending sort and takes index 0; `max_by_key` took the
//!   last).

use oc_ai::testing::TestLlm;
use oc_schema::common::{Num, TokenUsage, UsdPerMillionTokens as P};
use oc_schema::model::{Cost, CostCache, CostTier, CostTierType};

use crate::session::runner::tests_harness::{harness_with, harness_with_model, Harness};
use crate::session::runner::{ResolvedModel, SessionUsage};

// ------------------------------------------------------- RB-10a tie-break

fn tiered_cost(size: i64, input: f64) -> Cost {
    Cost {
        tier: Some(CostTier { tier_type: CostTierType::Context, size }),
        input: P(Num(input)),
        output: P(Num(2.0)),
        cache: CostCache { read: P(Num(1.0)), write: P(Num(1.0)) },
    }
}

/// Ref usage.ts:24-26 — `toSorted` is STABLE, so with duplicate tier
/// sizes the tier appearing EARLIER in `costs` wins the `[0]` pick. The
/// port previously used `max_by_key`, which takes the LAST on ties.
#[test]
fn runner_usage_calculate_cost_duplicate_tier_sizes_first_wins() {
    let costs = vec![
        tiered_cost(100_000, 3.0), // earlier entry — must win the tie
        tiered_cost(100_000, 7.0), // same size, higher price — must NOT win
        tiered_cost(50_000, 11.0), // smaller tier, not selected
    ];
    let usage = TokenUsage {
        input: Num(60_000.0),
        output: Num(10_000.0),
        reasoning: Num(0.0),
        cache: oc_schema::common::TokenUsageCache { read: Num(60_000.0), write: Num(0.0) },
    };
    let cost = SessionUsage::calculate_cost(&costs, &usage).0 .0;
    // First tier priced at 3/2/1/1 per million.
    let expected = (60_000.0 * 3.0 + 10_000.0 * 2.0 + 60_000.0 * 1.0) / 1_000_000.0;
    assert_eq!(cost, expected, "equal tier sizes keep the FIRST tier's price");
}

// --------------------------------------------------- RB-09a billing fan-out

/// The priced parity model: a base (untiered) rate plus one context tier
/// above 100k context tokens (mirrors the ref tier shape).
fn priced_resolved_model() -> ResolvedModel {
    let mut resolved = crate::session::runner::tests_harness::test_resolved_model();
    resolved.cost = vec![
        Cost {
            tier: None,
            input: P(Num(1.0)),
            output: P(Num(2.0)),
            cache: CostCache { read: P(Num(0.5)), write: P(Num(1.25)) },
        },
        tiered_cost(100_000, 10.0),
    ];
    // Match the priced output rate of tiered_cost for hand-computation
    // clarity: tier output 20, cache 5/12.5.
    resolved.cost[1].output = P(Num(20.0));
    resolved.cost[1].cache = CostCache { read: P(Num(5.0)), write: P(Num(12.5)) };
    resolved
}

/// A complete text response carrying an explicit usage report (all
/// counters set — the fan-out must bill every one of them).
fn usage_response(
    text: &str,
    non_cached_input: f64,
    output: f64,
    reasoning: f64,
    cache_read: f64,
    cache_write: f64,
) -> oc_ai::testing::TestResponse {
    use oc_ai::schema::events::{FinishReason, LLMEvent, Usage};
    let usage = Usage {
        input_tokens: Some(non_cached_input + cache_read + cache_write),
        non_cached_input_tokens: Some(non_cached_input),
        output_tokens: Some(output),
        reasoning_tokens: Some(reasoning),
        cache_read_input_tokens: Some(cache_read),
        cache_write_input_tokens: Some(cache_write),
        total_tokens: Some(non_cached_input + cache_read + cache_write + output),
        ..Default::default()
    };
    let mut response = oc_ai::testing::complete(
        FinishReason::Stop,
        vec![
            LLMEvent::TextStart { id: "text_1".into(), provider_metadata: None },
            LLMEvent::TextDelta { id: "text_1".into(), text: text.into(), provider_metadata: None },
            LLMEvent::TextEnd { id: "text_1".into(), text: None, provider_metadata: None },
        ],
    );
    for event in &mut response.events {
        match event {
            LLMEvent::StepFinish { usage: slot, .. } | LLMEvent::Finish { usage: slot, .. } => {
                *slot = Some(usage.clone());
            }
            _ => {}
        }
    }
    response
}

/// One priced turn: the message tree carries the recorded usage/cost and
/// `session_v2` accumulates it (usage.ts `record` + `add` semantics).
#[tokio::test]
async fn runner_usage_bills_priced_turn_into_message_and_session_counters() {
    let llm = TestLlm::new();
    // Below the tier: context = 60k + 10k + 10k = 80k ≤ 100k → base rate.
    llm.always(usage_response("base turn", 60_000.0, 8_000.0, 1_000.0, 10_000.0, 10_000.0));
    let harness = harness_with_model(llm.clone(), priced_resolved_model()).await;
    harness.create_session().await;
    harness.admit_prompt("msg_bill_1", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution exit");

    // Ref tokens(): input/output/reasoning/cache normalized; visible
    // output excludes reasoning.
    let messages = harness.messages().await;
    let assistant =
        messages.iter().find(|(_id, type_, _)| type_ == "assistant").expect("assistant message");
    assert_eq!(assistant.2["tokens"]["input"].as_f64(), Some(60_000.0));
    assert_eq!(assistant.2["tokens"]["output"].as_f64(), Some(7_000.0));
    assert_eq!(assistant.2["tokens"]["reasoning"].as_f64(), Some(1_000.0));
    assert_eq!(assistant.2["tokens"]["cache"]["read"].as_f64(), Some(10_000.0));
    assert_eq!(assistant.2["tokens"]["cache"]["write"].as_f64(), Some(10_000.0));
    // Base rate (1/2/0.5/1.25 per million): (60_000·1 + 8_000·2 +
    // 10_000·0.5 + 10_000·1.25) / 1e6 = 0.0935.
    assert_eq!(assistant.2["cost"].as_f64(), Some(0.0935));

    let (cost, input, output, read, write, reasoning) = session_counters(&harness).await;
    assert_eq!(
        (cost, input, output, read, write, reasoning),
        (0.0935, 60_000, 7_000, 10_000, 10_000, 1_000)
    );
}

/// Cross the tier on the second turn: the accumulated session cost is the
/// SUM of the per-turn records (`SessionUsage::add`, usage.ts:45-56).
#[tokio::test]
async fn runner_usage_accumulates_across_turns_with_tier_escalation() {
    let llm = TestLlm::new();
    llm.always(usage_response("tiered turn", 70_000.0, 8_000.0, 1_000.0, 20_000.0, 10_001.0));
    let harness = harness_with_model(llm.clone(), priced_resolved_model()).await;
    harness.create_session().await;
    harness.admit_prompt("msg_bill_2", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution exit");

    // Context = 70k + 20k + 10_001 = 100_001 > 100k → the context tier
    // (10/20/5/12.5 per million): (70_000·10 + 8_000·20 + 20_000·5 +
    // 10_001·12.5) / 1e6 = 1.0850125.
    let tiered = 1.085_012_5_f64;

    // Turn 2 below the tier — total must be tiered + base(0.0935).
    llm.always(usage_response("base turn", 60_000.0, 8_000.0, 1_000.0, 10_000.0, 10_000.0));
    harness.admit_prompt("msg_bill_3", "again").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution exit");

    let (cost, input, output, read, write, reasoning) = session_counters(&harness).await;
    assert!((cost - (tiered + 0.0935)).abs() < 1e-9, "sum of both turns' records: {cost}");
    assert_eq!(input, 130_000);
    assert_eq!(output, 14_000);
    assert_eq!(read, 30_000);
    assert_eq!(write, 20_001);
    assert_eq!(reasoning, 2_000);
}

async fn session_counters(harness: &Harness) -> (f64, i64, i64, i64, i64, i64) {
    let id = harness.session_id().to_string();
    harness
        .db
        .transaction(move |tx| {
            Ok(tx.conn().query_row(
                "SELECT cost, tokens_input, tokens_output, tokens_cache_read, tokens_cache_write, tokens_reasoning \
                 FROM session_v2 WHERE id = ?1",
                rusqlite::params![id],
                |row| {
                    Ok((
                        row.get::<_, f64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            ))
        })
        .await
        .expect("counter query")
        .expect("session row")
}

// --------------------------------------------------- RB-09b session affinity

/// `SessionAffinity.get` (affinity.ts:7-8): own id → parent's → fork
/// source's, in that precedence.
#[test]
fn runner_session_affinity_precedence_parent_then_fork_then_self() {
    use crate::session::runner::model_request::session_affinity;
    let mut row = crate::database::schema::SessionRow::new(
        "ses_self",
        "prj_1",
        "/tmp/opencode/x",
        "s",
        "test",
    )
    .expect("row");
    assert_eq!(session_affinity(&row), "ses_self", "no links → own id");
    row.fork_session_id = Some("ses_fork_src".into());
    assert_eq!(session_affinity(&row), "ses_fork_src", "fork → fork source");
    row.parent_id = Some("ses_parent".into());
    assert_eq!(session_affinity(&row), "ses_parent", "parent beats fork");
}

/// Ref session-runner.test.ts:4707-4741 — the affinity headers stick to
/// one key per session and re-key to the parent when the link appears
/// (invalidation); `x-opencode-session-id` keeps the session's own id and
/// a non-hex64 affinity key passes through `promptCacheKey` unchanged.
#[tokio::test]
async fn runner_affinity_headers_stick_then_rekey_to_parent() {
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let harness = harness_with(llm.clone()).await;
    harness.create_session().await;

    let headers_of = |request: &oc_ai::schema::messages::LLMRequest| {
        request.http.as_ref().and_then(|http| http.headers.clone()).unwrap_or_default()
    };

    // Turn 1: sticky to the session's own id.
    harness.admit_prompt("msg_aff_1", "hi").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution exit");
    let requests = llm.requests();
    assert_eq!(requests.len(), 1);
    let headers = headers_of(&requests[0]);
    assert_eq!(headers.get("x-opencode-session-id").map(String::as_str), Some("ses_runner_1"));
    assert_eq!(headers.get("x-session-affinity").map(String::as_str), Some("ses_runner_1"));
    assert_eq!(headers.get("X-Session-Id").map(String::as_str), Some("ses_runner_1"));
    assert_eq!(headers.get("x-opencode-session").map(String::as_str), Some("ses_runner_1"));
    assert!(!headers.contains_key("x-opencode-parent-session-id"));
    // Not ses_[0-9a-f]{64} → promptCacheKey is the raw key.
    assert_eq!(requests[0].prompt_cache_key.as_deref(), Some("ses_runner_1"));

    // Link the parent — the affinity key invalidates to the parent id.
    let parent = "ses_runner_parent".to_string();
    harness
        .db
        .transaction({
            let session_id = harness.session_id().to_string();
            move |tx| {
                let mut row =
                    crate::database::schema::SessionRow::get(tx, session_id)?.expect("session row");
                row.parent_id = Some(parent.clone());
                crate::database::schema::SessionRow::update(tx, &row)?;
                Ok(())
            }
        })
        .await
        .expect("parent link");

    harness.admit_prompt("msg_aff_2", "again").await;
    harness.execution.resume(harness.session_id().as_str()).await.expect("execution exit");
    let requests = llm.requests();
    assert_eq!(requests.len(), 2, "one request per turn");
    let headers = headers_of(&requests[1]);
    assert_eq!(
        headers.get("x-opencode-session-id").map(String::as_str),
        Some("ses_runner_1"),
        "correlation id keeps the session's own id"
    );
    assert_eq!(headers.get("x-session-affinity").map(String::as_str), Some("ses_runner_parent"));
    assert_eq!(headers.get("X-Session-Id").map(String::as_str), Some("ses_runner_parent"));
    assert_eq!(headers.get("x-opencode-session").map(String::as_str), Some("ses_runner_parent"));
    assert_eq!(
        headers.get("x-opencode-parent-session-id").map(String::as_str),
        Some("ses_runner_parent")
    );
    assert_eq!(headers.get("x-parent-session-id").map(String::as_str), Some("ses_runner_parent"));
    assert_eq!(requests[1].prompt_cache_key.as_deref(), Some("ses_runner_parent"));
}
