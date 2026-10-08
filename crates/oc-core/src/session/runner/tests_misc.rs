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
