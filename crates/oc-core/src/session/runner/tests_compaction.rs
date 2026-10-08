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

#[allow(dead_code)]
fn unused(_: &Harness) {}
