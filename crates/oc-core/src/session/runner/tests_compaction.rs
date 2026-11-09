//! Ported unit tests: `session-compaction` / `session-compaction-transport`
//! — ceiling math, due, conversation split, message formats, the summary
//! template, and a manual compaction run through the inbox.

use oc_ai::testing::{TestLlm, TestResponse};
use oc_schema::id::{MessageId, SkillId, SkillName};
use oc_schema::session_inbox::{Delivery, Item};
use oc_schema::session_message::{
    AssistantContent, AssistantTime, Compaction, CompactionReason, ToolState, ToolTime,
};

use super::tests_harness::{harness_with, Harness};
use crate::session::runner::compaction::{
    build_prompt, calculate_ceiling, message_to_text, oldest_to_drop, split_conversation,
    token_estimate, truncate_tool_output, NUDGE, SUMMARY_RULES, SUMMARY_TEMPLATE,
};

fn mid(value: &str) -> MessageId {
    MessageId::parse(value).unwrap_or_else(|error| MessageId(error.value))
}

fn user(text: &str) -> oc_schema::session_message::Info {
    oc_schema::session_message::Info::User {
        id: mid(&format!("msg_u_{text}")),
        metadata: None,
        time: oc_schema::session_message::Time { created: 1 },
        text: text.to_string(),
        files: None,
        agents: None,
        skills: None,
    }
}

fn assistant(text: &str) -> oc_schema::session_message::Info {
    oc_schema::session_message::Info::Assistant {
        id: mid(&format!("msg_a_{text}")),
        metadata: None,
        time: AssistantTime { created: 1, streamed: Some(1), completed: Some(2) },
        agent: oc_schema::id::AgentId::parse("build"),
        model: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("parityfake"),
            id: oc_schema::id::ModelId::parse("fake-model"),
            variant: None,
        },
        content: vec![AssistantContent::Text { text: text.to_string(), state: None }],
        snapshot: None,
        finish: None,
        raw_finish: None,
        provider_state: None,
        cost: None,
        tokens: None,
        error: None,
        retry: None,
    }
}

#[test]
fn runner_compaction_ceiling_defaults_to_ten_percent_or_reserve() {
    let limit = oc_schema::model::ModelLimit { context: 1_000_000, input: None, output: 32_000 };
    assert_eq!(calculate_ceiling(&limit, None), 900_000.0);
    // At 100k the 16k reserve beats 10%.
    let small = oc_schema::model::ModelLimit { context: 100_000, input: None, output: 4_000 };
    assert_eq!(calculate_ceiling(&small, None), 84_000.0);
    // Below 32k windows keep 10%.
    let tiny = oc_schema::model::ModelLimit { context: 30_000, input: None, output: 4_000 };
    assert_eq!(calculate_ceiling(&tiny, None), 27_000.0);
    let medium = oc_schema::model::ModelLimit { context: 150_000, input: None, output: 4_000 };
    assert_eq!(calculate_ceiling(&medium, None), 134_000.0, "16k reserve beats 10% at 150k");
    // Buffer overrides the reserve.
    assert_eq!(calculate_ceiling(&limit, Some(50_000)), 950_000.0);
    // Unknown limits are infinite.
    let unknown = oc_schema::model::ModelLimit { context: 0, input: None, output: 1_000 };
    assert!(calculate_ceiling(&unknown, None).is_infinite());
    // The input limit wins when set.
    let input =
        oc_schema::model::ModelLimit { context: 1_000_000, input: Some(100_000), output: 4_000 };
    assert_eq!(calculate_ceiling(&input, None), 84_000.0);
}

#[test]
fn runner_compaction_token_estimate_rounds() {
    assert_eq!(token_estimate(""), 0);
    assert_eq!(token_estimate("abcd"), 1);
    assert_eq!(token_estimate("abc"), 1, "Math.round semantics");
    assert_eq!(token_estimate("abcde"), 1);
    assert_eq!(token_estimate("abcdef"), 2);
}

#[test]
fn runner_compaction_oldest_to_drop_keeps_the_prefix_that_fits() {
    let sizes = [10, 10, 10];
    assert_eq!(oldest_to_drop(&sizes, 35), 0, "everything fits");
    assert_eq!(oldest_to_drop(&sizes, 25), 1, "the oldest no longer fits");
    assert_eq!(oldest_to_drop(&sizes, 20), 1);
    assert_eq!(oldest_to_drop(&sizes, 5), 3, "nothing fits");
    assert_eq!(oldest_to_drop(&sizes, 0), 3);
}

#[test]
fn runner_compaction_split_conversation_keeps_latest_exchange() {
    let messages = vec![
        user("first question"),
        assistant("first answer"),
        user("second question"),
        assistant("second answer"),
    ];
    let (older, recent) = split_conversation(&messages, 1_000).expect("split");
    assert_eq!(older.len(), 2, "the first exchange summarizes");
    assert_eq!(recent, "[User]: second question\n\n[Assistant]: second answer");
}

#[test]
fn runner_compaction_split_one_exchange_summarizes_everything() {
    let messages = vec![user("only question"), assistant("only answer")];
    let (older, recent) = split_conversation(&messages, 1_000).expect("split");
    assert_eq!(older.len(), 2);
    assert_eq!(recent, "");
}

#[test]
fn runner_compaction_split_empty_returns_none() {
    assert!(split_conversation(&[], 1_000).is_none());
}

#[test]
fn runner_compaction_message_to_text_user_with_skills_and_files() {
    let info = oc_schema::session_message::Info::User {
        id: mid("msg_x"),
        metadata: None,
        time: oc_schema::session_message::Time { created: 1 },
        text: "hello".into(),
        files: Some(vec![oc_schema::prompt::FileAttachment {
            data: String::new(),
            mime: "image/png".into(),
            source: oc_schema::prompt::FileSource::Uri { uri: "file:///pic.png".into() },
            name: Some("pic.png".into()),
            description: None,
            mention: None,
        }]),
        agents: None,
        skills: Some(vec![oc_schema::prompt::SkillAttachment {
            id: SkillId::parse("sk_1"),
            name: SkillName::parse("deploy"),
            text: Some("Skill body".into()),
            mention: None,
        }]),
    };
    assert_eq!(
        message_to_text(&info),
        "[Skill activated: deploy]\nSkill body\n[User]: hello\n[Attached image/png: pic.png]"
    );
}

#[test]
fn runner_compaction_message_to_text_assistant_tools() {
    let info = oc_schema::session_message::Info::Assistant {
        id: mid("msg_a2"),
        metadata: None,
        time: AssistantTime { created: 1, streamed: None, completed: Some(2) },
        agent: oc_schema::id::AgentId::parse("build"),
        model: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("parityfake"),
            id: oc_schema::id::ModelId::parse("fake-model"),
            variant: None,
        },
        content: vec![
            AssistantContent::Text { text: "thinking out loud".into(), state: None },
            AssistantContent::Reasoning { text: "reasoning trace".into(), state: None, time: None },
            AssistantContent::Tool {
                id: "call_1".into(),
                name: "bash".into(),
                executed: Some(false),
                provider_state: None,
                provider_result_state: None,
                state: ToolState::Completed {
                    input: [("command".to_string(), serde_json::json!("ls"))].into_iter().collect(),
                    content: vec![oc_schema::tool::Content::Text { text: "file.txt".into() }],
                    metadata: None,
                },
                time: ToolTime { created: 1, ran: Some(2), completed: Some(3) },
            },
        ],
        snapshot: None,
        finish: None,
        raw_finish: None,
        provider_state: None,
        cost: None,
        tokens: None,
        error: None,
        retry: None,
    };
    assert_eq!(
        message_to_text(&info),
        "[Assistant]: thinking out loud\n[Assistant reasoning]: reasoning trace\n[Assistant tool call]: bash({\"command\":\"ls\"})\n[Tool result]: file.txt"
    );
}

#[test]
fn runner_compaction_message_to_text_skips_compaction_and_system() {
    let compaction = oc_schema::session_message::Info::Compaction(Compaction::Completed {
        id: mid("msg_c"),
        metadata: None,
        time: oc_schema::session_message::Time { created: 1 },
        reason: CompactionReason::Auto,
        model: None,
        provider_state: None,
        summary: "S".into(),
        recent: "R".into(),
        provider_context: None,
        cost: None,
        tokens: None,
    });
    assert_eq!(message_to_text(&compaction), "");
    let system = oc_schema::session_message::Info::System {
        id: mid("msg_s"),
        metadata: None,
        time: oc_schema::session_message::Time { created: 1 },
        text: "update".into(),
        description: None,
    };
    assert_eq!(message_to_text(&system), "");
}

#[test]
fn runner_compaction_message_to_text_synthetic_and_skill_and_shell() {
    let synthetic = oc_schema::session_message::Info::Synthetic {
        id: mid("msg_syn"),
        metadata: None,
        time: oc_schema::session_message::Time { created: 1 },
        text: "note".into(),
        description: None,
    };
    assert_eq!(message_to_text(&synthetic), "[Synthetic context]: note");
    let skill = oc_schema::session_message::Info::Skill {
        id: mid("msg_sk"),
        metadata: None,
        time: oc_schema::session_message::Time { created: 1 },
        skill: SkillId::parse("sk_2"),
        name: SkillName::parse("review"),
        text: "Skill text".into(),
    };
    assert_eq!(message_to_text(&skill), "[Skill activated: review]\nSkill text");
    let shell = oc_schema::session_message::Info::Shell {
        id: mid("msg_sh"),
        metadata: None,
        shell_id: oc_schema::id::ShellId::parse("sh_2").unwrap(),
        command: "ls".into(),
        status: oc_schema::shell::Status::Exited,
        exit: None,
        output: Some(oc_schema::shell::Output {
            output: "a\nb".into(),
            cursor: 4,
            size: 4,
            truncated: false,
        }),
        time: oc_schema::session_message::ShellTime { created: 1, completed: Some(2) },
    };
    assert_eq!(message_to_text(&shell), "[Shell]: ls\na\nb");
    let mut background = shell_clone(&shell);
    if let oc_schema::session_message::Info::Shell { metadata, .. } = &mut background {
        *metadata =
            Some([("background".to_string(), serde_json::json!(true))].into_iter().collect());
    }
    assert_eq!(message_to_text(&background), "");
}

fn shell_clone(info: &oc_schema::session_message::Info) -> oc_schema::session_message::Info {
    let oc_schema::session_message::Info::Shell {
        id,
        metadata,
        shell_id,
        command,
        status,
        exit,
        output,
        time,
    } = info
    else {
        panic!("shell")
    };
    oc_schema::session_message::Info::Shell {
        id: id.clone(),
        metadata: metadata.clone(),
        shell_id: shell_id.clone(),
        command: command.clone(),
        status: *status,
        exit: *exit,
        output: output.clone(),
        time: *time,
    }
}

#[test]
fn runner_compaction_truncate_tool_output_counts_code_points() {
    let short = "x".repeat(1_250);
    assert_eq!(truncate_tool_output(&short), short);
    let long = "é".repeat(1_251);
    let truncated = truncate_tool_output(&long);
    assert!(truncated.ends_with("\n[truncated]"));
    assert_eq!(truncated.trim_end_matches("\n[truncated]").chars().count(), 1_250);
}

#[test]
fn runner_compaction_summary_template_and_rules_verbatim() {
    assert!(SUMMARY_TEMPLATE.starts_with(
        "You MUST use this format for your response (you may omit sections that aren't applicable). Do not include the <template> tags in your response."
    ));
    assert!(SUMMARY_TEMPLATE.contains("## Objective"));
    assert!(SUMMARY_TEMPLATE.contains("### Blocked"));
    assert!(SUMMARY_TEMPLATE.contains("Include at most 15, most important first."));
    assert!(SUMMARY_TEMPLATE.ends_with("</template>"));
    assert!(SUMMARY_RULES.starts_with("Rules:\n- Keep each section concise."));
    assert!(SUMMARY_RULES
        .ends_with("- Do not mention the summary process or that context was compacted."));
    assert!(
        NUDGE.starts_with("The previous response did not fill in the required summary template.")
    );
}

#[test]
fn runner_compaction_build_prompt_shapes() {
    let initial = build_prompt(false, false);
    assert!(initial.starts_with(
        "You MUST summarize the conversation above into a structured summary that will be given to another agent to resume the work."
    ));
    assert!(!initial.contains("Update the existing checkpoint"));
    let update = build_prompt(true, false);
    assert!(update.starts_with(
        "Update the existing checkpoint in the conversation above into one consolidated summary."
    ));
    assert!(!update.contains("earlier format"));
    let legacy = build_prompt(true, true);
    assert!(legacy.contains("The existing checkpoint was written with an earlier format"));
}

#[tokio::test]
async fn runner_compaction_manual_run_through_the_inbox() {
    // Script: the summary request gets a filled template back.
    let llm = TestLlm::new();
    llm.always(TestResponse {
        events: vec![
            oc_ai::schema::events::LLMEvent::StepStart { index: 0.0 },
            oc_ai::schema::events::LLMEvent::TextStart { id: "t1".into(), provider_metadata: None },
            oc_ai::schema::events::LLMEvent::TextDelta {
                id: "t1".into(),
                text: "## Objective\n- ship the runner".into(),
                provider_metadata: None,
            },
            oc_ai::schema::events::LLMEvent::TextEnd {
                id: "t1".into(),
                text: None,
                provider_metadata: None,
            },
            oc_ai::schema::events::LLMEvent::StepFinish {
                index: 0.0,
                reason: oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            },
            oc_ai::schema::events::LLMEvent::Finish {
                reason: oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::Stop),
                usage: None,
                provider_metadata: None,
            },
        ],
        error: None,
    });
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_before_compact", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    // Admit a manual compaction item and drain it.
    harness
        .inbox
        .admit(
            "msg_compact_1",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let events = harness.durable_events().await;
    let types: Vec<&str> = events.iter().map(|(type_, _, _)| type_.as_str()).collect();
    let started = types
        .iter()
        .position(|type_| *type_ == "session.compaction.started")
        .expect("manual compaction started");
    let delivered = types
        .iter()
        .position(|type_| {
            *type_ == "session.inbox.delivered"
                && events[started].2["inputID"] == serde_json::json!("msg_compact_1")
        })
        .unwrap_or(started);
    assert!(delivered <= started, "the item is delivered before the started event");
    let ended = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.ended")
        .expect("compaction ended");
    assert_eq!(ended.2["reason"], serde_json::json!("manual"));
    assert_eq!(ended.2["text"], serde_json::json!("## Objective\n- ship the runner"));
    // One exchange: v2 summarizes everything and keeps no verbatim recent.
    assert_eq!(ended.2["recent"], serde_json::json!(""));
    // The compaction message projects as a completed compaction row.
    let messages = harness.messages().await;
    assert!(messages.iter().any(|(_, type_, data)| type_ == "compaction"
        && data["status"] == serde_json::json!("completed")
        && data["summary"] == serde_json::json!("## Objective\n- ship the runner")));
}

// -- RD-03: `session.compaction` / `session.title` hook `result` ---------

/// The text parts of a request message (test-side inspection of what the
/// LLM was asked).
fn message_texts(message: &oc_ai::schema::messages::Message) -> Vec<String> {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            oc_ai::schema::messages::ContentPart::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn is_summary_request(request: &oc_ai::schema::messages::LLMRequest) -> bool {
    request
        .messages
        .last()
        .map(message_texts)
        .is_some_and(|texts| texts.iter().any(|text| text.starts_with("You MUST summarize")))
}

#[tokio::test]
async fn runner_compaction_hook_result_short_circuits_the_model() {
    use crate::hooks::{fn_hook, CompactionResult, HookOptions, SessionCompactionEvent};

    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let harness = harness_with(llm.clone()).await;
    let _keep = harness
        .hooks
        .register_session_compaction(
            fn_hook(|event: &mut SessionCompactionEvent| {
                event.result = Some(CompactionResult {
                    summary: "## Hook-supplied summary".into(),
                    provider_state: None,
                    metadata: None,
                    tokens: Some(serde_json::json!({
                        "input": 5.0,
                        "output": 7.0,
                        "reasoning": 0.0,
                        "cache": { "read": 0.0, "write": 0.0 }
                    })),
                });
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;
    harness.create_session().await;
    harness.admit_prompt("msg_hook_c_1", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    // The summary request never reaches the model.
    let summary_requests =
        llm.requests().iter().filter(|request| is_summary_request(request)).count();
    assert_eq!(summary_requests, 0);

    harness
        .inbox
        .admit(
            "msg_hook_c_2",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    let events = harness.durable_events().await;
    let ended = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.ended")
        .expect("compaction ended");
    assert_eq!(ended.2["text"], serde_json::json!("## Hook-supplied summary"));
    // One exchange: the hook result keeps the same verbatim `recent` the
    // split produced (empty for a single exchange).
    assert_eq!(ended.2["recent"], serde_json::json!(""));
    // The hook-supplied tokens are billed onto the compaction.
    assert_eq!(ended.2["tokens"]["input"], serde_json::json!(5));
    assert_eq!(ended.2["tokens"]["output"], serde_json::json!(7));

    // The persisted compaction row carries the hook's summary verbatim.
    let messages = harness.messages().await;
    assert!(messages.iter().any(|(_, type_, data)| type_ == "compaction"
        && data["status"] == serde_json::json!("completed")
        && data["summary"] == serde_json::json!("## Hook-supplied summary")));

    // The model was never asked for a summary.
    let summary_requests =
        llm.requests().iter().filter(|request| is_summary_request(request)).count();
    assert_eq!(summary_requests, 0, "the hook result must skip the LLM");
}

#[tokio::test]
async fn runner_compaction_without_hook_result_uses_the_model() {
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("## Objective\n- model summary"));
    let harness = harness_with(llm.clone()).await;
    harness.create_session().await;
    harness.admit_prompt("msg_nohook_1", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    harness
        .inbox
        .admit(
            "msg_nohook_2",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    let events = harness.durable_events().await;
    let ended = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.ended")
        .expect("compaction ended");
    assert_eq!(ended.2["text"], serde_json::json!("## Objective\n- model summary"));
    let summary_requests =
        llm.requests().iter().filter(|request| is_summary_request(request)).count();
    assert_eq!(summary_requests, 1, "without a hook result the model summarizes");
}

// -- RD-09: compaction retry honors the `session.retry` hook --------------

#[tokio::test]
async fn runner_compaction_retry_hook_override_stops_the_retry() {
    use crate::hooks::{fn_hook, HookOptions, RetryDecision, SessionRetryEvent};

    let llm = TestLlm::new();
    let rate_limit = || {
        oc_ai::schema::AiError::new(oc_ai::schema::AiErrorReason::RateLimit {
            fields: oc_ai::schema::ReasonFields::new("slow down"),
            retry_after_ms: None,
            rate_limit: None,
        })
    };
    // Everything answers "ok" except the compaction summary request,
    // which always fails with a retryable rate limit.
    llm.serve(move |request: &oc_ai::schema::messages::LLMRequest| {
        if is_summary_request(request) {
            TestResponse::error(rate_limit())
        } else {
            oc_ai::testing::text("ok")
        }
    });
    let harness = harness_with(llm.clone()).await;
    // The prepared retry hook's NoRetry override applies to compaction
    // (compaction.ts:444-451 passes `prepared.retry` into the policy).
    let _keep = harness
        .hooks
        .register_session_retry(
            fn_hook(|event: &mut SessionRetryEvent| {
                event.decision = RetryDecision::NoRetry;
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;
    harness.create_session().await;
    harness.admit_prompt("msg_retry_c_1", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    harness
        .inbox
        .admit(
            "msg_retry_c_2",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    // The turn settles with the compaction failure; the drain surfaces it.
    let _ = harness.execution.resume(harness.session_id().as_str()).await;

    // The default policy would retry a rate limit; the hook's NoRetry
    // override must win, so exactly one summary attempt happens and the
    // compaction fails.
    let events = harness.durable_events().await;
    let failed = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.failed")
        .expect("compaction failed");
    assert_eq!(failed.2["reason"], serde_json::json!("manual"));
    assert_eq!(
        failed.2["error"]["type"],
        serde_json::json!("provider.rate-limit"),
        "the failing summary surfaces as the provider error"
    );
    let summary_requests =
        llm.requests().iter().filter(|request| is_summary_request(request)).count();
    assert_eq!(summary_requests, 1, "the retry hook override stopped the resend");
}

// -- RD-03 (title): `session.title` hook `result` --------------------------

#[tokio::test]
async fn runner_title_hook_result_short_circuits_the_model() {
    use crate::hooks::{fn_hook, HookOptions, SessionTitleEvent};

    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let harness = harness_with(llm.clone()).await;
    let _keep = harness
        .hooks
        .register_session_title(
            fn_hook(|event: &mut SessionTitleEvent| {
                event.result = Some("Hook-supplied title".into());
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;
    harness.create_session().await;
    harness.admit_prompt("msg_hook_t_1", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    // Title generation rides a forked task that can race the message
    // rows; drive it directly (the drain path only fires it).
    harness.title.generate(harness.session_id().as_str()).await;

    let events = harness.durable_events().await;
    let renamed =
        events.iter().find(|(type_, _, _)| type_ == "session.renamed").expect("session renamed");
    assert_eq!(renamed.2["title"], serde_json::json!("Hook-supplied title"));
    // Only the chat turn hit the model; the title request was skipped.
    assert_eq!(llm.requests().len(), 1, "the hook title must skip the LLM");
}

/// RV-B9: compaction clears the spent accounting ONLY for the compacted
/// session. REF compaction.ts:671 — `Effect.ensuring(Effect.sync(() =>
/// spent.delete(trigger.context.session.id)))` — a map delete, not a clear.
#[tokio::test]
async fn runner_compaction_clears_spent_only_for_the_compacted_session() {
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("unused"));
    let harness = harness_with(llm).await;
    let session = harness.create_session().await;
    harness.compaction.test_spend(session.id.as_str()).await;
    harness.compaction.test_spend("ses_other").await;
    // Auto compaction off ⇒ the trigger skips the model path entirely, but
    // the ensuring finalizer (spent delete) still runs.
    harness.compaction.configure(crate::session::runner::compaction::Settings {
        auto: false,
        buffer: None,
        keep: 15_000,
    });
    let context = crate::session::runner::context::Loaded {
        session: session.clone(),
        agent_id: oc_schema::id::AgentId::parse("build"),
        agent: crate::session::runner::tests_harness::agent_info("build", None),
        model: crate::session::runner::tests_harness::test_resolved_model(),
        initial: String::new(),
        messages: Vec::new(),
        tools: Vec::new(),
    };
    let status = harness
        .compaction
        .compact(crate::session::runner::compaction::Trigger {
            reason: crate::session::runner::compaction::Reason::Auto,
            context,
            input_id: None,
        })
        .await;
    assert!(matches!(status, crate::session::runner::compaction::CompactStatus::Skipped));
    let spent = harness.compaction.test_spent();
    assert!(!spent.contains(&session.id.clone()), "the compacted session's spent is cleared");
    assert!(spent.contains(&"ses_other".to_string()), "the other session's spent survives");
}

/// F3 (B9): REF clears per-session spent through `Effect.ensuring`, which
/// ALSO runs on interrupt. The drain's cancellation arm drops the in-flight
/// compact future (skipping `compact`'s own tail), so it must clear the
/// session's spent itself — per-session, never global.
#[tokio::test]
async fn runner_compaction_interrupt_clears_spent_for_the_interrupted_session() {
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("## Objective\n- never reached"));
    // Hold the summary stream after registration: the compaction is
    // genuinely in flight when the shutdown arm fires.
    let gate = llm.gate();
    let llm = llm.clone();
    let harness = harness_with(llm.clone()).await;
    let session = harness.create_session().await;
    // Seed spent: the interrupted session plus an unrelated one.
    harness.compaction.test_spend(session.id.as_str()).await;
    harness.compaction.test_spend("ses_other").await;

    let loaded = crate::session::runner::context::Loaded {
        session: session.clone(),
        agent_id: oc_schema::id::AgentId::parse("build"),
        agent: crate::session::runner::tests_harness::agent_info("build", None),
        model: crate::session::runner::tests_harness::test_resolved_model(),
        initial: String::new(),
        messages: vec![oc_schema::session_message::Info::User {
            id: oc_schema::id::MessageId::parse("msg_interrupt_1").unwrap(),
            metadata: None,
            time: oc_schema::session_message::Time { created: 1 },
            text: "Summarize me".into(),
            files: None,
            agents: None,
            skills: None,
        }],
        tools: Vec::new(),
    };

    // The drain's manual-compaction cancellation arm (drain.rs RD-11):
    // shutdown races the compact future; on interrupt the future is
    // dropped and the arm clears the session's spent itself.
    let interrupted = tokio::select! {
        outcome = harness.compaction.compact(crate::session::runner::compaction::Trigger {
            reason: crate::session::runner::compaction::Reason::Manual,
            context: loaded,
            input_id: None,
        }) => {
            panic!("the gated compaction must not settle before the interrupt: {outcome:?}")
        }
        () = async {
            llm.wait(1).await;
            gate.started().await;
        } => {
            // Mirrors drain.rs: the compact future is dropped here — its
            // own spent-clearing tail never runs.
            harness.compaction.clear_spent(session.id.as_str());
            true
        }
    };
    assert!(interrupted, "the shutdown arm won the select");

    let spent = harness.compaction.test_spent();
    assert!(
        !spent.contains(&session.id.clone()),
        "the interrupted session's spent is cleared (Effect.ensuring semantics)"
    );
    assert!(spent.contains(&"ses_other".to_string()), "the other session's spent survives");
}

#[allow(dead_code)]
fn unused(_: &Harness) {}

// -- AUD-04: native compaction trigger passthrough (compaction.ts:324-372,
// `deliver(trigger, …)` — the reason rides through, never hard-coded) ------

/// A scripted client whose route supports the in-stream compaction
/// trigger: normal requests stream from the shared `TestLlm`, `compact`
/// returns a fixed encrypted checkpoint.
struct NativeCompactClient {
    stream: TestLlm,
}

#[async_trait::async_trait]
impl crate::session::runner::LlmClient for NativeCompactClient {
    async fn stream(
        &self,
        request: oc_ai::schema::messages::LLMRequest,
    ) -> Result<crate::session::runner::LlmEventStream, oc_ai::schema::AiError> {
        Ok(Box::pin(self.stream.stream(request)))
    }

    fn can_compact(
        &self,
        _request: &oc_ai::schema::messages::LLMRequest,
        mechanism: crate::session::runner::CompactMechanism,
    ) -> bool {
        mechanism == crate::session::runner::CompactMechanism::Trigger
    }

    async fn compact(
        &self,
        _request: oc_ai::schema::messages::LLMRequest,
        _mechanism: crate::session::runner::CompactMechanism,
    ) -> Result<crate::session::runner::CompactOutcome, oc_ai::schema::AiError> {
        Ok(crate::session::runner::CompactOutcome {
            replacement: Vec::new(),
            checkpoint: Some(oc_ai::schema::messages::ContentPart::compaction_encrypted(
                "parityfake",
                "NATIVE-CHECKPOINT",
            )),
            usage: None,
        })
    }
}

/// The native-compaction harness: the session model carries
/// `compaction: native` (a static test endpoint keeps provenance stable).
async fn native_harness() -> Harness {
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("ok"));
    let mut model = super::tests_harness::test_resolved_model();
    model.compaction = Some(oc_schema::provider::Compaction::Native);
    super::tests_harness::harness_with_client(
        std::sync::Arc::new(NativeCompactClient { stream: llm }),
        model,
    )
    .await
}

/// A manual `/compact` on a native model must NOT publish
/// `session.compaction.started` (ref deliver: `trigger.reason !==
/// "manual"` skips the started event) and must settle with
/// `ended(reason:"manual")` + the encrypted provider context.
#[tokio::test]
async fn runner_compaction_native_manual_publishes_no_started_and_manual_reason() {
    let harness = native_harness().await;
    harness.create_session().await;
    harness.admit_prompt("msg_before_native_compact", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    harness
        .inbox
        .admit(
            "msg_native_compact_1",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    let events = harness.durable_events().await;
    // The drain's eager publish (ref runner/llm.ts:91) is the ONLY started
    // event: reason "manual", tied to the `/compact` inbox item. Pre-fix the
    // native path added a second `started` with reason "auto".
    let started: Vec<_> =
        events.iter().filter(|(type_, _, _)| type_ == "session.compaction.started").collect();
    assert_eq!(
        started.len(),
        1,
        "exactly one started event (got: {:?})",
        started.iter().map(|(_, _, data)| data.clone()).collect::<Vec<_>>()
    );
    assert_eq!(started[0].2["reason"], serde_json::json!("manual"));
    assert_eq!(started[0].2["inputID"], serde_json::json!("msg_native_compact_1"));
    let ended = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.ended")
        .expect("native compaction ended");
    assert_eq!(ended.2["reason"], serde_json::json!("manual"));
    assert!(
        ended.2.get("providerContext").is_some_and(|context| context["messages"].is_array()),
        "the native window rides the ended event"
    );
}

/// An auto compaction on a native model keeps the started event with
/// `reason:"auto"` and settles with `ended(reason:"auto")` — the fix must
/// not disturb the automatic path.
#[tokio::test]
async fn runner_compaction_native_auto_keeps_auto_started_and_ended() {
    let harness = native_harness().await;
    // Force `due`: a buffer of window-1 pins the ceiling at one token.
    harness.compaction.configure(crate::session::runner::compaction::Settings {
        auto: true,
        buffer: Some(199_999),
        keep: 15_000,
    });
    harness.create_session().await;
    harness.admit_prompt("msg_native_auto", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    let events = harness.durable_events().await;
    let started = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.started")
        .expect("auto native compaction publishes a started event");
    assert_eq!(started.2["reason"], serde_json::json!("auto"));
    let ended = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.ended")
        .expect("native compaction ended");
    assert_eq!(ended.2["reason"], serde_json::json!("auto"));
    assert!(ended.2.get("providerContext").is_some());
}

// -- G-194: due() one-beat window (ref compaction.ts:242-251 —
// `measured < findLastIndex(isCheckpoint)` with findLastIndex's -1) --------

/// A completed compaction message; `provider_context: Some(_)` makes it a
/// checkpoint (native encrypted window).
fn completed_compaction(checkpoint: bool) -> oc_schema::session_message::Info {
    use oc_schema::session_support::{Provenance, ProviderContext};
    oc_schema::session_message::Info::Compaction(Compaction::Completed {
        id: mid("msg_c_due"),
        metadata: None,
        time: oc_schema::session_message::Time { created: 1 },
        reason: CompactionReason::Auto,
        model: None,
        provider_state: None,
        summary: "## Objective\n- done".into(),
        recent: String::new(),
        provider_context: checkpoint.then(|| ProviderContext {
            version: (),
            provenance: Provenance {
                provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                provider: "parityfake".into(),
                model_id: "fake-model".into(),
                route: "r".into(),
                protocol: "proto".into(),
                endpoint: "e".into(),
            },
            messages: serde_json::to_value(vec![oc_ai::schema::messages::Message::user("kept")])
                .unwrap(),
        }),
        cost: None,
        tokens: None,
    })
}

/// An assistant reply carrying measured prompt tokens for the context's
/// model (`hasMeasuredPrompt` trusts it).
fn measured_assistant() -> oc_schema::session_message::Info {
    let mut message = assistant("measured reply");
    if let oc_schema::session_message::Info::Assistant { tokens, .. } = &mut message {
        *tokens = Some(oc_schema::common::TokenUsage {
            input: oc_schema::common::Num(100.0),
            output: oc_schema::common::Num(1.0),
            reasoning: oc_schema::common::Num(0.0),
            cache: oc_schema::common::TokenUsageCache {
                read: oc_schema::common::Num(0.0),
                write: oc_schema::common::Num(0.0),
            },
        });
    }
    message
}

/// The one-beat window: a compaction completed, one more message arrived,
/// the estimate still sits over the ceiling. With a checkpoint (native
/// encrypted window) and no measured reply after it, REF is NOT due —
/// the runner must first rebuild the request from the checkpoint and
/// measure the reply.
#[tokio::test]
async fn runner_compaction_due_waits_for_measured_reply_after_checkpoint() {
    use crate::session::runner::compaction::due;
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("unused"));
    let harness = harness_with(llm).await;
    let session = harness.create_session().await;
    let context =
        |messages: Vec<oc_schema::session_message::Info>| crate::session::runner::context::Loaded {
            session: session.clone(),
            agent_id: oc_schema::id::AgentId::parse("build"),
            agent: super::tests_harness::agent_info("build", None),
            model: super::tests_harness::test_resolved_model(),
            initial: String::new(),
            messages,
            tools: Vec::new(),
        };

    // Checkpoint present, no measured reply after it → NOT due, even with
    // the ceiling pinned at one token (estimate far over it).
    assert!(!due(
        &context(vec![user("hello there"), completed_compaction(true), user("one more beat")]),
        1.0
    ));
    // Same shape WITHOUT the encrypted window (plain summary compaction,
    // not a checkpoint) → due again: measured = checkpoint = -1.
    assert!(due(
        &context(vec![user("hello there"), completed_compaction(false), user("one more beat")]),
        1.0
    ));
    // A measured reply after the checkpoint re-arms the trigger.
    assert!(due(
        &context(vec![
            user("hello there"),
            completed_compaction(true),
            measured_assistant(),
            user("one more beat"),
        ]),
        1.0
    ));
}

// -- G-195: native compaction deviations (compaction.ts:335+569-590,
// :545-547) ---------------------------------------------------------------

/// The native path prepares its request through the session WebSocket
/// seam (ref `prepare(..., "session")`); the summarize path passes none.
#[tokio::test]
async fn runner_compaction_native_prepares_through_the_session_ws_seam() {
    let harness = native_harness().await;
    harness.create_session().await;
    harness.admit_prompt("msg_ws_native", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    harness
        .inbox
        .admit(
            "msg_ws_native_compact",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    assert!(
        harness.compaction.test_requests().test_last_web_socket(),
        "native compaction must pass web_socket = session seam (ref compaction.ts:335)"
    );
}

/// The summarize path prepares its request WITHOUT the session WebSocket
/// seam (ref `prepare` leaves `webSocket` undefined).
#[tokio::test]
async fn runner_compaction_summarize_prepares_without_the_session_ws_seam() {
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("## Objective\n- model summary"));
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_ws_summary", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    harness
        .inbox
        .admit(
            "msg_ws_summary_compact",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    assert!(
        !harness.compaction.test_requests().test_last_web_socket(),
        "summarize compaction must not use the session WebSocket seam"
    );
}

/// REF compaction.ts:545-547: an `error` finish during compaction maps to
/// `AIError(UnknownProviderError)` — retryable — so the deliver loop
/// resends under the session retry policy instead of failing once.
#[tokio::test]
async fn runner_compaction_error_finish_is_retryable_and_recovers() {
    use crate::hooks::{fn_hook, HookOptions, RetryDecision, SessionRetryEvent};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let llm = TestLlm::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&attempts);
    llm.serve(move |request: &oc_ai::schema::messages::LLMRequest| {
        if is_summary_request(request) {
            if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                // Ends with an `error` finish reason.
                oc_ai::testing::complete(
                    oc_ai::schema::events::FinishReason::Error,
                    vec![oc_ai::schema::events::LLMEvent::TextDelta {
                        id: "t1".into(),
                        text: "broken".to_string(),
                        provider_metadata: None,
                    }],
                )
            } else {
                oc_ai::testing::text("## Objective\n- recovered summary")
            }
        } else {
            oc_ai::testing::text("ok")
        }
    });
    let harness = harness_with(llm.clone()).await;
    // Zero-delay retries keep the test deterministic.
    let _keep = harness
        .hooks
        .register_session_retry(
            fn_hook(|event: &mut SessionRetryEvent| {
                event.decision = RetryDecision::Retry { delay_ms: 0 };
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;
    harness.create_session().await;
    harness.admit_prompt("msg_errfin_1", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    harness
        .inbox
        .admit(
            "msg_errfin_2",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();

    let events = harness.durable_events().await;
    let ended = events
        .iter()
        .find(|(type_, _, _)| type_ == "session.compaction.ended")
        .expect("compaction recovered via retry");
    assert_eq!(ended.2["text"], serde_json::json!("## Objective\n- recovered summary"));
    let summary_requests =
        llm.requests().iter().filter(|request| is_summary_request(request)).count();
    assert_eq!(summary_requests, 2, "the error finish was retried, not terminal");
}

// -- G-256: token estimation counts UTF-16 code units, not bytes ---------

/// G-256 — ref util/token.ts:5 `Math.round(input.length / 4)` counts
/// UTF-16 code units (JS `String.length`). Byte-based counting estimated
/// CJK content ~3x high (4 CJK chars = 12 UTF-8 bytes = 3 tokens by
/// bytes, 1 token by units), systematically drifting the compaction
/// trigger threshold. Same shape as the G-106④ codemode.rs fix.
#[test]
fn runner_compaction_token_estimate_counts_utf16_units_not_bytes() {
    // ASCII: units == bytes.
    assert_eq!(token_estimate("abcd"), 1);
    assert_eq!(token_estimate("abcdefgh"), 2);
    // CJK: 4 chars = 12 UTF-8 bytes but 4 UTF-16 units → 1 token, not 3.
    assert_eq!(token_estimate("你好世界"), 1);
    assert_eq!(token_estimate("你好世界你好世界"), 2);
    // Astral plane (emoji): 2 UTF-16 code units per char (surrogate
    // pair) — a JS `length` of 4 for two emoji.
    assert_eq!(token_estimate("😀😀"), 1);
    // Empty input: zero (ref `Math.max(0, …)` clamps; round is never
    // negative anyway).
    assert_eq!(token_estimate(""), 0);
    // Rounding: half rounds away from zero exactly like `Math.round`.
    assert_eq!(token_estimate("ab"), 1); // 0.5 → 1
    assert_eq!(token_estimate("a"), 0); // 0.25 → 0
}

// -- G-252②: the wire reason always stays inside ["auto","manual"] -------

/// G-252② — an overflow-triggered compaction folds its reason to "auto"
/// on the wire (ref compaction.ts:629 `trigger.reason === "manual" ?
/// "manual" : "auto"`). Both sides' event schemas only allow
/// `["auto","manual"]` (ref session-event.ts:588; session_message.rs
/// `CompactionReason`), so the internal "overflow" would break typed
/// clients and the projected row's strict decode.
#[tokio::test]
async fn runner_compaction_overflow_reason_folds_to_auto_on_the_wire() {
    let llm = TestLlm::new();
    // Never matches the template headings → the summarize path fails
    // after the nudge, so the failed publish carries the folded reason.
    llm.always(oc_ai::testing::text("no template headings here"));
    let harness = harness_with(llm).await;
    let session = harness.create_session().await;
    let loaded = crate::session::runner::context::Loaded {
        session: session.clone(),
        agent_id: oc_schema::id::AgentId::parse("build"),
        agent: super::tests_harness::agent_info("build", None),
        model: super::tests_harness::test_resolved_model(),
        initial: String::new(),
        messages: vec![user("hello overflow")],
        tools: Vec::new(),
    };
    let status = harness
        .compaction
        .compact(crate::session::runner::compaction::Trigger {
            reason: crate::session::runner::compaction::Reason::Overflow,
            context: loaded,
            input_id: None,
        })
        .await;
    assert!(
        matches!(status, crate::session::runner::compaction::CompactStatus::Failed { .. }),
        "the template-mismatch summary fails: {status:?}"
    );

    let events = harness.durable_events().await;
    let failed: Vec<&(String, i64, serde_json::Value)> =
        events.iter().filter(|(type_, _, _)| type_ == "session.compaction.failed").collect();
    assert_eq!(failed.len(), 1, "exactly one failed event: {events:#?}");
    // The wire reason is the folded enum value, never "overflow".
    assert_eq!(failed[0].2["reason"], serde_json::json!("auto"));
    // Strict decode through the typed event schema succeeds (a typed
    // client must be able to consume it).
    let decoded: oc_schema::session_event::CompactionFailed =
        serde_json::from_value(failed[0].2.clone()).expect("typed decode");
    assert_eq!(decoded.reason, CompactionReason::Auto);

    // The projected compaction row strictly decodes too (the message
    // updater consumed the event; the stored row must round-trip).
    let messages = harness.messages().await;
    let row = messages
        .iter()
        .find(|(_, type_, _)| type_ == "compaction")
        .expect("projected compaction row");
    let mut value = row.2.clone();
    value["id"] = serde_json::json!(row.0);
    value["type"] = serde_json::json!(row.1);
    let info: oc_schema::session_message::Info =
        serde_json::from_value(value).expect("strict row decode");
    match info {
        oc_schema::session_message::Info::Compaction(Compaction::Failed { reason, .. }) => {
            assert_eq!(reason, CompactionReason::Auto);
        }
        other => panic!("expected a failed compaction row, got {other:?}"),
    }
}

// -- G-252①: exactly one durable failure per failed manual /compact ------

/// G-252① — a failed manual `/compact` publishes `compaction.failed`
/// exactly ONCE. Pre-fix the compaction service's publish plus the
/// drain error arm's re-publish produced two durable events (and the
/// second appended a duplicate failed compaction row: the first had
/// already consumed the running row). REF compaction.ts:237-243 matchEffect
/// — the service publish is the only one; llm.ts:137-152 covers only
/// interrupts and pre-compact setup failures.
#[tokio::test]
async fn runner_compaction_manual_failure_publishes_exactly_one_failed_event() {
    let llm = TestLlm::new();
    // "ok" has no template headings: the manual summary fails after the
    // nudge retry. The chat turn itself accepts the same text fine.
    llm.always(oc_ai::testing::text("ok"));
    let harness = harness_with(llm).await;
    harness.create_session().await;
    harness.admit_prompt("msg_g252_1", "hello there").await;
    harness.execution.resume(harness.session_id().as_str()).await.unwrap();
    harness
        .inbox
        .admit(
            "msg_g252_compact",
            harness.session_id().as_str(),
            Item::Compaction {
                payload: oc_schema::session_inbox::CompactionPayload {},
                delivery: Delivery::Steer,
            },
        )
        .await
        .unwrap();
    // The drain surfaces the compaction failure as the turn's error —
    // the execution exit is a failure, not a panic.
    let exit = harness.execution.resume(harness.session_id().as_str()).await;
    assert!(exit.is_err(), "the failed manual compaction fails the drain");

    let events = harness.durable_events().await;
    let failed: Vec<&(String, i64, serde_json::Value)> =
        events.iter().filter(|(type_, _, _)| type_ == "session.compaction.failed").collect();
    assert_eq!(
        failed.len(),
        1,
        "exactly one durable compaction.failed (got {}): {events:#?}",
        failed.len()
    );
    assert_eq!(failed[0].2["reason"], serde_json::json!("manual"));
    assert_eq!(failed[0].2["inputID"], serde_json::json!("msg_g252_compact"));

    // And exactly ONE compaction message row (the updated running row),
    // not a running row plus a duplicate appended failure.
    let messages = harness.messages().await;
    let compaction_rows: Vec<&(String, String, serde_json::Value)> =
        messages.iter().filter(|(_, type_, _)| type_ == "compaction").collect();
    assert_eq!(compaction_rows.len(), 1, "one compaction row: {messages:#?}");
    assert_eq!(compaction_rows[0].2["status"], serde_json::json!("failed"));
}

/// G-279⑤ — estimation uses the EFFECTIVE (namespaced) tool name the
/// request carries (`registry::effective_name`), not the registered
/// bare name (ref compaction.ts:931-937 estimates the request's tools).
#[test]
fn g279_estimate_uses_effective_tool_name() {
    let tool = crate::location::ToolRegistration {
        name: "create".into(),
        description: "create a thing".into(),
        input: serde_json::json!({ "type": "object", "properties": {} }),
        namespace: Some("slack.admin".into()),
        permission: None,
        codemode: true,
    };
    let entry = crate::session::runner::compaction::tool_entry_of(&tool);
    let oc_ai::schema::messages::ToolEntry::Definition(definition) = &entry else {
        panic!("definition entry")
    };
    assert_eq!(definition.name, "slack_admin_create", "effective name");
}
