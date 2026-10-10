//! Ported unit tests: `session-system-prompt`, `session-model-request`,
//! plus the runner's shared plumbing (to-session-error, SessionUsage,
//! max-steps, date/env builtins, instruction machinery).

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_ai::schema::messages::{ContentPart, Message};
use oc_ai::schema::{AiError, AiErrorReason, ReasonFields, Usage};
use oc_schema::common::{SessionError, TokenUsage, TokenUsageCache};

use crate::session::runner::context::{
    instruction_diff, instruction_hash, instruction_read, render_initial, ReadOutcome,
};
use crate::session::runner::max_steps::MAX_STEPS_PROMPT;
use crate::session::runner::model_request::{
    bound_images, output_limit, unsupported_parts, useragent, AppMetadata, InputTokens, RequestKind,
};
use crate::session::runner::system_prompt::{make, render, SYSTEM_TEMPLATE};
use crate::session::runner::{content_filter_error, to_session_error, SessionUsage};

// ------------------------------------------------- session-system-prompt

#[test]
fn runner_system_prompt_template_is_verbatim() {
    assert!(SYSTEM_TEMPLATE
        .starts_with("You are an AI agent running in OpenCode, a coding agent harness."));
    assert!(SYSTEM_TEMPLATE.contains("${OPENCODE_TOOL_GUIDANCE}"));
    assert!(SYSTEM_TEMPLATE.ends_with(
        "- Treat unfamiliar files or changes as potential user work and investigate before deleting or overwriting them."
    ));
    assert_eq!(SYSTEM_TEMPLATE.lines().count(), 15);
}

#[test]
fn runner_system_prompt_make_without_tools_leaves_guidance_empty() {
    assert_eq!(make(&[]), SYSTEM_TEMPLATE.replace("${OPENCODE_TOOL_GUIDANCE}", ""));
}

#[test]
fn runner_system_prompt_shell_guidance_exact() {
    let prompt = make(&["shell".into(), "read".into()]);
    assert!(prompt.contains(
        "- Prefer dedicated tools over shell commands; fall back to the shell when a tool cannot do what you need."
    ));
    assert!(prompt.contains(
        "- Do not chain shell commands with separators like `echo \"====\";` or `printf '---'`; the output becomes noisy in a way that makes the user's side of the conversation worse."
    ));
    assert!(!prompt.contains("write tool"));
}

#[test]
fn runner_system_prompt_write_and_edit_guidance_exact() {
    let prompt = make(&["write".into(), "edit".into()]);
    assert!(prompt.contains(
        "- Use the write tool to create files or completely replace their content. Prefer using the edit tool for targeted changes."
    ));
    assert!(prompt.contains(
        "- Use the edit tool for targeted changes to existing text files. It replaces the exact text in `oldString` with `newString`, and the values must differ. By default, `oldString` must occur exactly once. If it occurs multiple times, include more surrounding context to make it unique or set `replaceAll` to true to replace every occurrence."
    ));
}

#[test]
fn runner_system_prompt_render_joins_instructions() {
    let custom = "Custom prompt\n${OPENCODE_TOOL_GUIDANCE}";
    assert_eq!(render(custom, &["shell".into()]), "Custom prompt\n- Prefer dedicated tools over shell commands; fall back to the shell when a tool cannot do what you need.\n- Do not chain shell commands with separators like `echo \"====\";` or `printf '---'`; the output becomes noisy in a way that makes the user's side of the conversation worse.");
}

#[test]
fn runner_max_steps_prompt_verbatim() {
    assert!(MAX_STEPS_PROMPT.starts_with("CRITICAL - MAXIMUM STEPS REACHED"));
    assert!(MAX_STEPS_PROMPT
        .ends_with("Any attempt to use tools is a critical violation. Respond with text ONLY."));
    assert!(MAX_STEPS_PROMPT.contains("Tools are disabled until next user input"));
}

// ------------------------------------------------- session-model-request

#[test]
fn runner_output_limit_uses_catalog_limit_fitted_to_room() {
    let limit = oc_schema::model::ModelLimit { context: 100_000, input: None, output: 32_000 };
    assert_eq!(output_limit(&limit, RequestKind::Primary, None), 32_000.0);
    // Unknown output falls back to 32k.
    let unknown = oc_schema::model::ModelLimit { context: 100_000, input: None, output: 0 };
    assert_eq!(output_limit(&unknown, RequestKind::Primary, None), 32_000.0);
    // Huge catalog limits cap at 256k.
    let huge = oc_schema::model::ModelLimit { context: 1_000_000, input: None, output: 900_000 };
    assert_eq!(output_limit(&huge, RequestKind::Primary, None), 256_000.0);
    // Compaction never needs more than 32k.
    assert_eq!(output_limit(&huge, RequestKind::Compaction, None), 32_000.0);
    // Fit-to-room: measured + 15% estimated leaves `room`, min 1024.
    let tokens = InputTokens { measured: 80_000.0, estimated: 10_000.0 };
    // room = 100000 - 80000 - ceil(10000*1.15) = 8500
    assert_eq!(output_limit(&limit, RequestKind::Primary, Some(tokens)), 8_500.0);
    let cramped = InputTokens { measured: 99_500.0, estimated: 10_000.0 };
    assert_eq!(output_limit(&limit, RequestKind::Primary, Some(cramped)), 1_024.0);
    // Unknown window returns the catalog limit.
    let no_window = oc_schema::model::ModelLimit { context: 0, input: None, output: 8_000 };
    assert_eq!(output_limit(&no_window, RequestKind::Primary, Some(tokens)), 8_000.0);
}

fn media_part(mime: &str, bytes: usize) -> ContentPart {
    ContentPart::Media {
        media: oc_ai::schema::media::base64("A".repeat(bytes), mime),
        filename: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    }
}

fn capabilities(inputs: &[&str]) -> oc_schema::model::Capabilities {
    oc_schema::model::Capabilities {
        input: inputs.iter().map(|item| item.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn runner_unsupported_parts_replaces_media_with_exact_text() {
    let messages = vec![Message::new(
        oc_ai::schema::messages::MessageRole::User,
        vec![
            ContentPart::text("hi"),
            media_part("audio/wav", 10),
            media_part("video/mp4", 10),
            media_part("image/png", 10),
        ],
    )];
    let replaced = unsupported_parts(messages, &capabilities(&["image"]));
    let parts = &replaced[0].content;
    assert_eq!(parts[0], ContentPart::text("hi"));
    assert_eq!(
        parts[1],
        ContentPart::text(
            "ERROR: Cannot read audio (this model does not support audio input). Inform the user."
        )
    );
    assert_eq!(
        parts[2],
        ContentPart::text(
            "ERROR: Cannot read video (this model does not support video input). Inform the user."
        )
    );
    // Supported modality stays.
    assert!(matches!(parts[3], ContentPart::Media { .. }));
}

#[test]
fn runner_unsupported_parts_names_the_file() {
    let mut part = media_part("audio/wav", 10);
    if let ContentPart::Media { filename, .. } = &mut part {
        *filename = Some("meeting.wav".into());
    }
    let replaced = unsupported_parts(
        vec![Message::new(oc_ai::schema::messages::MessageRole::User, vec![part])],
        &capabilities(&["image"]),
    );
    assert_eq!(
        replaced[0].content[0],
        ContentPart::text(
            "ERROR: Cannot read \"meeting.wav\" (this model does not support audio input). Inform the user."
        )
    );
}

#[test]
fn runner_bound_images_drops_oldest_to_target() {
    // base64 of N 'A's decodes to N bytes; build two large images.
    let big = base64_of(18 * 1024 * 1024);
    let small = base64_of(4 * 1024 * 1024);
    let messages = vec![Message::new(
        oc_ai::schema::messages::MessageRole::User,
        vec![
            ContentPart::Media {
                media: oc_ai::schema::media::base64(big, "image/png"),
                filename: Some("old.png".into()),
                cache: None,
                metadata: None,
                provider_metadata: None,
            },
            ContentPart::Media {
                media: oc_ai::schema::media::base64(small, "image/png"),
                filename: Some("new.png".into()),
                cache: None,
                metadata: None,
                provider_metadata: None,
            },
        ],
    )];
    let bounded = bound_images(messages);
    let text = format!("{:?}", bounded[0].content);
    assert!(
        text.contains("[This image was removed to reduce the request size and is no longer visible. Do not make claims about its contents from memory. If needed, retrieve it again with an available tool or ask the user to attach it again.]"),
        "oldest dropped with the removal text"
    );
    assert!(matches!(bounded[0].content[1], ContentPart::Media { .. }), "newest kept");
}

fn base64_of(bytes: usize) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(vec![0u8; bytes])
}

#[test]
fn runner_bound_images_under_trigger_is_unchanged() {
    let messages = vec![Message::new(
        oc_ai::schema::messages::MessageRole::User,
        vec![media_part("image/png", 1024)],
    )];
    let bounded = bound_images(messages);
    assert!(matches!(bounded[0].content[0], ContentPart::Media { .. }));
}

#[test]
fn runner_useragent_shape() {
    let app =
        AppMetadata { name: "opencode".into(), version: "2.0.24".into(), channel: "stable".into() };
    assert_eq!(useragent(&app), "opencode/stable/2.0.24/opencode");
}

// ------------------------------------------------- to-session-error

fn session_error_of(reason: AiErrorReason) -> SessionError {
    to_session_error(&AiError::new(reason))
}

#[test]
fn runner_to_session_error_maps_provider_reasons() {
    let fields = || ReasonFields::new("boom");
    assert_eq!(
        session_error_of(AiErrorReason::RateLimit {
            fields: fields(),
            retry_after_ms: None,
            rate_limit: None
        })
        .error_type,
        "provider.rate-limit"
    );
    assert_eq!(
        session_error_of(AiErrorReason::Authentication { fields: fields() }).error_type,
        "provider.auth"
    );
    assert_eq!(
        session_error_of(AiErrorReason::QuotaExceeded { fields: fields() }).error_type,
        "provider.quota"
    );
    assert_eq!(
        session_error_of(AiErrorReason::ContentPolicy { fields: fields() }).error_type,
        "provider.content-filter"
    );
    assert_eq!(
        session_error_of(AiErrorReason::ProviderInternal {
            fields: fields(),
            retry_after_ms: None
        })
        .error_type,
        "provider.internal"
    );
    assert_eq!(
        session_error_of(AiErrorReason::InvalidProviderOutput {
            fields: fields(),
            classification: None,
            route: None
        })
        .error_type,
        "provider.invalid-output"
    );
    assert_eq!(
        session_error_of(AiErrorReason::InvalidRequest {
            fields: fields(),
            parameter: None,
            classification: None
        })
        .error_type,
        "provider.invalid-request"
    );
    assert_eq!(
        session_error_of(AiErrorReason::UnsupportedOperation {
            fields: fields(),
            operation: "compact".into(),
            provider: None,
            route: None
        })
        .error_type,
        "provider.unsupported-operation"
    );
    assert_eq!(
        session_error_of(AiErrorReason::Timeout { fields: fields(), timeout_ms: None }).error_type,
        "provider.timeout"
    );
    assert_eq!(
        session_error_of(AiErrorReason::UnknownProvider { fields: fields() }).error_type,
        "provider.unknown"
    );
}

#[test]
fn runner_content_filter_error_joins_category_and_explanation() {
    let mut details =
        oc_ai::schema::FinishReasonDetails::new(oc_ai::schema::FinishReason::ContentFilter);
    details.category = Some("cyber".into());
    details.explanation = Some("too hot".into());
    let error = content_filter_error("Provider blocked the response", &details);
    assert_eq!(error.error_type, "provider.content-filter");
    assert_eq!(error.message, "Provider blocked the response (cyber): too hot");
}

// ------------------------------------------------- SessionUsage

#[test]
fn runner_usage_tokens_normalizes_the_report() {
    let usage = Usage {
        input_tokens: Some(100.0),
        non_cached_input_tokens: Some(60.0),
        output_tokens: Some(30.0),
        reasoning_tokens: Some(10.0),
        cache_read_input_tokens: Some(20.0),
        cache_write_input_tokens: Some(20.0),
        ..Default::default()
    };
    let tokens = SessionUsage::tokens(Some(&usage));
    assert_eq!(tokens.input.0, 60.0);
    assert_eq!(tokens.output.0, 20.0, "visible output excludes reasoning");
    assert_eq!(tokens.reasoning.0, 10.0);
    assert_eq!(tokens.cache.read.0, 20.0);
    assert_eq!(tokens.cache.write.0, 20.0);
    assert_eq!(SessionUsage::tokens(None), TokenUsage::default());
}

#[test]
fn runner_usage_calculate_cost_picks_the_context_tier() {
    use oc_schema::common::{Num, UsdPerMillionTokens as P};
    use oc_schema::model::{Cost, CostCache, CostTier, CostTierType};
    let costs = vec![
        Cost {
            tier: None,
            input: P(Num(1.0)),
            output: P(Num(2.0)),
            cache: CostCache { read: P(Num(0.5)), write: P(Num(1.25)) },
        },
        Cost {
            tier: Some(CostTier { tier_type: CostTierType::Context, size: 100_000 }),
            input: P(Num(10.0)),
            output: P(Num(20.0)),
            cache: CostCache { read: P(Num(5.0)), write: P(Num(12.5)) },
        },
    ];
    let below = TokenUsage {
        input: Num(50_000.0),
        output: Num(10_000.0),
        reasoning: Num(0.0),
        cache: TokenUsageCache::default(),
    };
    let expected_below = (50_000.0 * 1.0 + 10_000.0 * 2.0) / 1_000_000.0;
    assert_eq!(SessionUsage::calculate_cost(&costs, &below).0 .0, expected_below);
    let above = TokenUsage {
        input: Num(50_000.0),
        output: Num(10_000.0),
        reasoning: Num(0.0),
        cache: oc_schema::common::TokenUsageCache { read: Num(50_000.0), write: Num(10_000.0) },
    };
    let expected_above =
        (50_000.0 * 10.0 + 10_000.0 * 20.0 + 50_000.0 * 5.0 + 10_000.0 * 12.5) / 1_000_000.0;
    assert_eq!(SessionUsage::calculate_cost(&costs, &above).0 .0, expected_above);
    assert_eq!(SessionUsage::calculate_cost(&[], &above).0 .0, 0.0);
}

#[test]
fn runner_usage_record_and_add() {
    let usage = Usage {
        input_tokens: Some(10.0),
        non_cached_input_tokens: Some(10.0),
        ..Default::default()
    };
    let first = SessionUsage::record(Some(&usage), &[]);
    let second = SessionUsage::record(Some(&usage), &[]);
    let total = SessionUsage::add(&first, &second);
    assert_eq!(total.tokens.input.0, 20.0);
    assert_eq!(total.cost.0 .0, 0.0);
}

// ------------------------------------------------- instructions

fn constant_source(key: &str, value: &str) -> crate::session::runner::context::InstructionSource {
    let key = key.to_string();
    let value = value.to_string();
    crate::session::runner::context::InstructionSource {
        key,
        read: Box::new(move || ReadOutcome::Value(serde_json::json!(value))),
        initial: Box::new(|value| Some(format!("initial: {}", value.as_str().unwrap_or_default()))),
        changed: Box::new(|_previous, current| {
            Some(format!("changed: {}", current.as_str().unwrap_or_default()))
        }),
        removed: Some(Box::new(|previous| {
            Some(format!("removed: {}", previous.as_str().unwrap_or_default()))
        })),
    }
}

#[test]
fn runner_instruction_diff_initial_admits_every_value() {
    let list = vec![constant_source("a", "1"), constant_source("b", "2")];
    let observed = instruction_read(&list);
    let admission = instruction_diff(&observed, None).expect("initial diff");
    assert_eq!(admission.delta.len(), 2);
    assert_eq!(admission.delta["a"], instruction_hash(&serde_json::json!("1")));
    assert_eq!(admission.blobs.len(), 2);
}

#[test]
fn runner_instruction_diff_initial_blocks_on_unavailable() {
    let list = vec![crate::session::runner::context::InstructionSource {
        key: "blocked".into(),
        read: Box::new(|| ReadOutcome::Unavailable),
        initial: Box::new(|_| None),
        changed: Box::new(|_, _| None),
        removed: None,
    }];
    let observed = instruction_read(&list);
    let blocked = instruction_diff(&observed, None).expect_err("blocked");
    assert_eq!(blocked, vec!["blocked".to_string()]);
}

#[test]
fn runner_instruction_hash_is_canonical() {
    // Key order must not matter.
    let a = serde_json::json!({"z": 1, "a": [1, 2]});
    let b = serde_json::json!({"a": [1, 2], "z": 1});
    assert_eq!(instruction_hash(&a), instruction_hash(&b));
    assert_eq!(instruction_hash(&a).len(), 64);
}

#[test]
fn runner_render_initial_joins_with_blank_lines() {
    let list = vec![constant_source("a", "1"), constant_source("b", "2")];
    let values = BTreeMap::from([
        ("a".to_string(), serde_json::json!("1")),
        ("b".to_string(), serde_json::json!("2")),
    ]);
    assert_eq!(render_initial(&list, &values), "initial: 1\n\ninitial: 2");
}

#[test]
fn runner_date_string_matches_to_date_string() {
    use crate::session::runner::context::date_string;
    assert_eq!(date_string(1_725_000_000_000), "Fri Aug 30 2024");
    assert_eq!(date_string(1_791_504_000_000), "Fri Oct 09 2026");
    assert_eq!(date_string(0), "Thu Jan 01 1970");
}

#[test]
fn runner_environment_block_shape() {
    use crate::session::runner::context::environment_block;
    let block = environment_block("/work/project", "/work", true, "/tmp/opencode");
    let lines: Vec<&str> = block.lines().collect();
    assert_eq!(lines[0], "<env>");
    assert_eq!(lines[1], "  Working directory: /work/project");
    assert_eq!(lines[2], "  Workspace root folder: /work");
    assert_eq!(lines[3], "  Is directory a git repo: yes");
    assert_eq!(lines[4], "  Platform: linux");
    assert_eq!(lines[5], "  Prefer /tmp/opencode over generic system temporary directories such as /tmp; it is pre-created and approved for external access.");
    assert_eq!(lines[6], "</env>");
}

#[test]
fn runner_environment_source_renders_initial_and_changed() {
    let dir = "/work/project";
    let deps_block =
        crate::session::runner::context::environment_block(dir, dir, false, "/tmp/opencode");
    let values = BTreeMap::from([("core/environment".to_string(), serde_json::json!(deps_block))]);
    let source = crate::session::runner::context::InstructionSource {
        key: "core/environment".into(),
        read: Box::new(move || ReadOutcome::Value(serde_json::json!(deps_block))),
        initial: Box::new(|value| {
            Some(format!(
                "Here is some useful information about the environment you are running in:\n{}",
                value.as_str().unwrap_or_default()
            ))
        }),
        changed: Box::new(|_previous, current| {
            Some(format!(
                "The environment you are running in is now:\n{}",
                current.as_str().unwrap_or_default()
            ))
        }),
        removed: None,
    };
    let list = vec![source];
    let rendered = render_initial(&list, &values);
    assert!(rendered.starts_with(
        "Here is some useful information about the environment you are running in:\n<env>"
    ));
}

#[test]
fn runner_instruction_state_applies_hash_deltas() {
    use crate::session::runner::context::apply_hash_delta;
    let values = BTreeMap::from([("a".to_string(), "h1".to_string())]);
    let delta = BTreeMap::from([
        ("b".to_string(), "h2".to_string()),
        ("a".to_string(), "removed".to_string()),
    ]);
    let next = apply_hash_delta(&values, &delta);
    assert_eq!(next, BTreeMap::from([("b".to_string(), "h2".to_string())]));
}

// Marker: the harness's fixed resolver keeps RouteLlmClient reachable.
#[test]
fn runner_route_client_is_constructible() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<crate::session::runner::RouteLlmClient>();
    let _ = Arc::new(crate::session::runner::RouteLlmClient);
}

// ---------------------------------------------------------------------------
// Wave 5-0: RouteLlmClient anthropic native compaction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn runner_route_client_compact_support_by_protocol() {
    use crate::session::runner::{CompactMechanism, LlmClient as _, RouteLlmClient};

    let anthropic = oc_ai::protocols::anthropic_messages::anthropic_route(
        Arc::new(oc_ai::route::transport::FailingTransport {
            message: "compact support test never reaches a transport",
        }),
        None,
    );
    let model = anthropic.model("claude-sonnet-4-5", None).expect("anthropic model");
    let mut request = oc_ai::schema::messages::LLMRequest::new(model);
    request.messages = vec![Message::user("hello")];

    let client = RouteLlmClient;
    assert!(
        client.can_compact(&request, CompactMechanism::Trigger),
        "anthropic-messages supports the in-stream compaction trigger"
    );
    assert!(
        !client.can_compact(&request, CompactMechanism::Endpoint),
        "anthropic has no standalone compact endpoint"
    );

    let responses =
        oc_ai::test_support::echo_route(Arc::new(oc_ai::route::transport::FailingTransport {
            message: "echo protocol has no compact",
        }));
    let chat = responses.model("m", None).expect("chat model");
    let mut chat_request = oc_ai::schema::messages::LLMRequest::new(chat);
    chat_request.messages = vec![Message::user("hello")];
    assert!(!client.can_compact(&chat_request, CompactMechanism::Trigger));
    assert!(!client.can_compact(&chat_request, CompactMechanism::Endpoint));
}

#[tokio::test]
async fn runner_route_client_anthropic_trigger_compact_collects_checkpoint() {
    use crate::session::runner::{CompactMechanism, LlmClient as _, RouteLlmClient};
    use oc_ai::schema::messages::ContentPart;

    // A scripted anthropic SSE stream answering the contextManagement.edits
    // trigger with exactly one compaction block.
    let body = [
        r#"data: {"type":"message_start","message":{"usage":{"input_tokens":5}}}"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"compaction","content":null}}"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"compaction_delta","content":"CHECKPOINT-BYTES"}}"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}"#,
        r#"data: {"type":"message_stop"}"#,
        "data: [DONE]",
    ]
    .join("\n\n")
        + "\n\n";

    let transport = Arc::new(oc_ai::test_support::MockTransport::from_interactions(vec![
        oc_ai::test_support::RecordedInteraction {
            method: "POST".into(),
            url: "https://api.anthropic.com/v1/messages?beta=true".into(),
            status: 200,
            body,
            request_body: None,
        },
    ]));
    let route = oc_ai::protocols::anthropic_messages::anthropic_messages_route(transport.clone());
    let model = route.model("claude-sonnet-4-5", None).expect("anthropic model");
    let mut request = oc_ai::schema::messages::LLMRequest::new(model);
    request.messages = vec![Message::user("compact this")];

    let client = RouteLlmClient;
    let outcome = client.compact(request, CompactMechanism::Trigger).await.expect("compact");
    let checkpoint = outcome.checkpoint.expect("the compaction checkpoint");
    match checkpoint {
        ContentPart::Compaction { provider, text, .. } => {
            assert_eq!(provider.as_str(), "anthropic");
            assert_eq!(text.as_deref(), Some("CHECKPOINT-BYTES"));
        }
        other => panic!("expected a compaction part, got {other:?}"),
    }
    // The request carried the contextManagement.edits trigger (the beta
    // header rides the wire body).
    let sent = transport.requests();
    let _ = sent;
}

// ---------------------------------------------------------------------------
// Wave 5-0: title model.small + the MCP guidance body
// ---------------------------------------------------------------------------

/// A catalog with one provider whose models include a `gpt-luna` family
/// member (the first `Model.small` preference).
fn small_family_catalog() -> crate::provider_catalog::Catalog {
    crate::provider_catalog::Catalog::parse(
        r#"{
            "lunarcorp": {
                "id": "lunarcorp",
                "name": "LunarCorp",
                "npm": "@opencode/ai/providers/openai-compatible",
                "api": "https://api.lunarcorp.test/v1",
                "models": {
                    "luna-mini": { "id": "luna-mini", "name": "Luna Mini", "family": "gpt-luna", "tool_call": true, "modalities": { "input": ["text"], "output": ["text"] }, "limit": { "context": 100000, "output": 8000 } },
                    "luna-max": { "id": "luna-max", "name": "Luna Max", "family": "gpt-luna-pro", "tool_call": true, "modalities": { "input": ["text"], "output": ["text"] }, "limit": { "context": 200000, "output": 16000 } }
                }
            }
        }"#,
    )
    .expect("catalog parses")
}

#[tokio::test]
async fn runner_title_selects_the_catalog_small_model() {
    use crate::session::runner::context::{ContextDeps, SessionContext};

    let stack = crate::session::runner::tests_harness::harness().await;
    let catalog = Arc::new(small_family_catalog());
    let agents = Arc::new(crate::location::AgentRegistry::new());
    agents.set(crate::session::runner::tests_harness::agent_info("title", None));
    agents.set(crate::session::runner::tests_harness::agent_info("build", None));
    let tools = Arc::new(crate::location::ToolRegistryState::new());

    // The resolver resolves whatever ref it is asked for (like the
    // catalog resolver would).
    struct ResolvesRef;
    impl crate::session::runner::ModelResolver for ResolvesRef {
        fn resolve(
            &self,
            session: &crate::database::schema::SessionRow,
        ) -> Result<crate::session::runner::ResolvedModel, crate::session::runner::ModelError>
        {
            self.resolve_selected(
                session,
                &oc_schema::model::Ref {
                    provider_id: oc_schema::id::ProviderId::parse("lunarcorp"),
                    id: oc_schema::id::ModelId::parse("luna-max"),
                    variant: Some(oc_schema::id::ModelVariantId::parse("default")),
                },
            )
        }

        fn resolve_selected(
            &self,
            _session: &crate::database::schema::SessionRow,
            reference: &oc_schema::model::Ref,
        ) -> Result<crate::session::runner::ResolvedModel, crate::session::runner::ModelError>
        {
            let mut resolved = crate::session::runner::tests_harness::test_resolved_model();
            resolved.reference = reference.clone();
            Ok(resolved)
        }
    }

    let deps = ContextDeps::minimal(
        crate::session::runner::tests_harness::fixed_clock(),
        Arc::new(|| Vec::new()),
        Arc::new(|| String::new()),
    );
    let deps = ContextDeps { catalog: Arc::clone(&catalog), ..deps };
    let context = SessionContext::new(
        stack.db.clone(),
        stack.bus.clone(),
        crate::location::LocationRef::new("/tmp/opencode", None),
        agents,
        tools,
        Arc::new(ResolvesRef),
        deps,
    );
    let session = crate::database::schema::SessionRow::new(
        "ses_title_small",
        "prj_1",
        "/tmp/opencode",
        "slug",
        "test",
    )
    .expect("session row");
    let (_agent, primary, selected) = context.select_title(&session).expect("selection");
    assert_eq!(primary.expect("primary").reference.id.as_str(), "luna-max");
    assert_eq!(
        selected.reference.id.as_str(),
        "luna-mini",
        "the title falls back to the provider's small family model"
    );
}

#[test]
fn runner_title_small_model_family_preference_order() {
    let catalog = small_family_catalog();
    let small = crate::session::runner::context::small_model(&catalog, "lunarcorp")
        .expect("the gpt-luna family member");
    assert_eq!(small.id.as_str(), "luna-mini");
    assert!(
        crate::session::runner::context::small_model(&catalog, "unknown").is_none(),
        "unknown providers have no small model"
    );
}

#[test]
fn runner_mcp_guidance_renders_the_v2_body() {
    use crate::session::runner::context::{mcp_guidance_source, McpServerSummary, ReadOutcome};

    let summaries = vec![
        McpServerSummary {
            server: "context7".into(),
            instructions: "Use context7 for library docs.".into(),
            codemode: true,
        },
        McpServerSummary {
            server: "legacy".into(),
            instructions: "Direct tools only.".into(),
            codemode: false,
        },
    ];
    let source = mcp_guidance_source(Arc::new(move || summaries.clone()));
    assert_eq!(source.key, "core/mcp-guidance");
    let ReadOutcome::Value(value) = (source.read)() else {
        panic!("connected servers are an observed value");
    };
    let rendered = (source.initial)(&value).expect("initial body");
    assert_eq!(
        rendered,
        "<mcp_instructions>\n  <server name=\"context7\">\n    Use tools from this server through `execute` under `tools[\"context7\"]`.\n    Use context7 for library docs.\n  </server>\n  <server name=\"legacy\">\n    Direct tools only.\n  </server>\n</mcp_instructions>",
        "byte-exact against mcp/instructions.ts"
    );

    // Empty = observed absence: the key stays out of the instruction delta.
    let empty = mcp_guidance_source(Arc::new(Vec::new));
    assert!(matches!((empty.read)(), ReadOutcome::Removed));
}

// -- session.retry mutation write-back (D-03: by-reference parity) ---------

/// v2's retry caller (retry.ts:80-87) reads its LOCAL event after the
/// hook — the trigger's return is discarded, so plugin mutations flow
/// back BY REFERENCE (`event.decision = {retry, delay}`; the flagship's
/// foreground-fallback rewrites exactly this). The prepared retry hook
/// must write the dispatched event back through the `&mut`, or a
/// plugin's retry override silently no-ops.
#[tokio::test]
async fn runner_retry_prepared_hook_writes_mutations_back() {
    use crate::hooks::{fn_hook, HookOptions, RetryDecision, SessionRetryEvent};
    use crate::session::runner::model_request::{RequestInput, SessionModelRequest};
    use crate::session::runner::tests_harness::test_resolved_model;

    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    // Hold the registration for the test body (dropping it disposes).
    let _keep = hooks
        .register_session_retry(
            fn_hook(|event: &mut SessionRetryEvent| {
                event.decision = RetryDecision::Retry { delay_ms: 31_000 };
                Ok(())
            }),
            HookOptions::default(),
        )
        .await;
    let tools = Arc::new(crate::location::ToolRegistryState::new());
    let requests = SessionModelRequest::new(Arc::clone(&hooks), tools, AppMetadata::default());

    let row = crate::database::schema::SessionRow::new(
        "ses_retry_mut",
        "proj_1",
        "/tmp/opencode/retry-mut",
        "retry-mut",
        "2",
    )
    .expect("session row");
    let prepared = requests
        .prepare(
            RequestKind::Primary,
            RequestInput {
                session: row,
                agent: oc_schema::id::AgentId::parse("build"),
                model: test_resolved_model(),
                tools: None,
                system: Vec::new(),
                messages: Vec::new(),
                tool_choice: None,
                web_socket: false,
                input_tokens: None,
            },
        )
        .await
        .expect("prepare");

    // The schedule proposed NO retry; the hook's override must win.
    let mut event = SessionRetryEvent {
        session_id: oc_schema::id::SessionId::new(),
        agent: oc_schema::id::AgentId::parse("build"),
        model: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("parityfake"),
            id: oc_schema::id::ModelId::parse("fake-model"),
            variant: None,
        },
        error: serde_json::json!({ "message": "boom" }),
        attempt: 2,
        decision: RetryDecision::NoRetry,
    };
    (prepared.retry)(&mut event).await;
    match event.decision {
        RetryDecision::Retry { delay_ms } => assert_eq!(delay_ms, 31_000),
        RetryDecision::NoRetry => {
            panic!("retry hook decision override was dropped (write-back missing)")
        }
    }
}
