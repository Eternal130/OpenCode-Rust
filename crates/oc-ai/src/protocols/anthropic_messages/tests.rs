//! Port of `test/provider/anthropic-messages.test.ts` (request lowering,
//! stream machine, and cache-policy interactions), split out of
//! `anthropic_messages.rs` to keep the protocol implementation readable.

use super::*;
use crate::route::endpoint::EndpointPatch;
use crate::route::transport::FailingTransport;
use crate::schema::{CachePolicy, GenerationOptions, LLMResponse};
use crate::test_support::{MockTransport, RecordedInteraction};
use serde_json::json;

fn test_route() -> Route {
    anthropic_messages_route(Arc::new(FailingTransport { message: "unit test route" })).with(
        RoutePatch {
            endpoint: Some(EndpointPatch {
                base_url: Some("https://api.anthropic.test/v1/".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
}

fn model_of(id: &str) -> LanguageModel {
    test_route().model(id, None).expect("model builds")
}

fn compile(request: &LLMRequest) -> Result<Value, AiError> {
    let route = request.model.route.clone();
    let prepared = route.prepare_request(request);
    route.build_body(&prepared)
}

fn base_request(model: LanguageModel) -> LLMRequest {
    let mut request = LLMRequest::new(model);
    request.system = vec![SystemPart {
        text: "You are concise.".into(),
        cache: Some(CacheHint::ephemeral()),
        metadata: None,
    }];
    request.messages = vec![Message::user("Say hello.")];
    request.cache = Some(CachePolicy::None);
    request.generation = Some(GenerationOptions {
        max_tokens: Some(20.0),
        temperature: Some(0.0),
        ..Default::default()
    });
    request
}

fn classification_of(error: &AiError) -> Option<String> {
    match &error.reason {
        AiErrorReason::InvalidProviderOutput { classification, .. } => classification.clone(),
        AiErrorReason::InvalidRequest {
            classification: Some(crate::schema::ProviderFailureClassification::ContextOverflow),
            ..
        } => Some("context-overflow".into()),
        AiErrorReason::InvalidRequest {
            classification: Some(crate::schema::ProviderFailureClassification::PayloadTooLarge),
            ..
        } => Some("payload-too-large".into()),
        _ => None,
    }
}

fn sse(chunks: &[Value]) -> String {
    let mut out = String::new();
    for chunk in chunks {
        out.push_str(&format!("data: {chunk}\n\n"));
    }
    out.push_str("data: [DONE]\n\n");
    out
}

fn sse_raw(events: &[&str]) -> String {
    let mut out = String::new();
    for event in events {
        out.push_str(event);
        out.push_str("\n\n");
    }
    out
}

/// JSON equality with numbers compared as `f64` (the only TS/Rust
/// serialization difference: `20` vs `20.0`).
#[allow(clippy::float_cmp)]
fn jeq(actual: &Value, expected: &Value) {
    fn equal(actual: &Value, expected: &Value) -> bool {
        match (actual, expected) {
            (Value::Number(a), Value::Number(b)) => {
                a.as_f64().is_some_and(|a| b.as_f64().is_some_and(|b| a == b))
            }
            (Value::Object(a), Value::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(key, value)| b.get(key).is_some_and(|other| equal(value, other)))
            }
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
            }
            _ => actual == expected,
        }
    }
    assert!(equal(actual, expected), "value mismatch:\n rust: {actual}\nexpect: {expected}");
}

/// Serialize an event for wire-shape assertions. The `LLMEvent` tree
/// serializes in the TS contract shape natively (`camelCase` fields —
/// oracle gate 3 finding 1); no key renaming is needed for events.
fn wire(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("serializes")
}

/// `ContentPart` (the assembled message tree, NOT the `LLMEvent` tree)
/// still serializes `provider_metadata` as `snake_case`; its wire shape is
/// owned by the session-persistence lane, so rename per-site here.
fn content_wire(value: &impl serde::Serialize) -> Value {
    fn rename(value: &mut Value) {
        match value {
            Value::Object(map) => {
                if let Some(metadata) = map.remove("provider_metadata") {
                    map.insert("providerMetadata".to_string(), metadata);
                }
                for (_, item) in map.iter_mut() {
                    rename(item);
                }
            }
            Value::Array(items) => {
                for item in items {
                    rename(item);
                }
            }
            _ => {}
        }
    }
    let mut out = serde_json::to_value(value).expect("serializes");
    rename(&mut out);
    out
}

async fn generate_with(body: String, request: LLMRequest) -> Result<LLMResponse, AiError> {
    let route = request.model.route.clone();
    let transport = MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://api.anthropic.test/v1/messages".into(),
        status: 200,
        body,
        request_body: None,
    }]);
    let replay_route = route.with(RoutePatch {
        transport: Some(Arc::new(transport) as SharedTransport),
        ..Default::default()
    });
    replay_route.generate(request).await
}

// -- request lowering -----------------------------------------------------

#[test]
fn prepares_anthropic_messages_target() {
    let prepared = compile(&base_request(model_of("claude-sonnet-4-5"))).unwrap();
    jeq(
        &prepared,
        &json!({
            "model": "claude-sonnet-4-5",
            "system": [{"type": "text", "text": "You are concise.", "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": [{"type": "text", "text": "Say hello."}]}],
            "stream": true,
            "max_tokens": 20,
            "temperature": 0,
        }),
    );
}

#[test]
fn omits_empty_system_text_while_preserving_whitespace() {
    let base = base_request(model_of("claude-sonnet-4-5"));
    let empty = base
        .update(LLMRequestPatch { system: Some(vec![SystemPart::new("")]), ..Default::default() });
    let whitespace = base
        .update(LLMRequestPatch { system: Some(vec![SystemPart::new(" ")]), ..Default::default() });
    assert_eq!(compile(&empty).unwrap().get("system"), None);
    assert_eq!(
        compile(&whitespace).unwrap().get("system"),
        Some(&json!([{"type": "text", "text": " "}]))
    );
}

#[test]
fn filters_whitespace_only_text_and_removes_empty_messages() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user(" \n\t"),
            Message::new(MessageRole::User, vec![]),
            Message::new(
                MessageRole::User,
                vec![
                    ContentPart::text(""),
                    ContentPart::text("  Keep this spacing.  "),
                    ContentPart::text(" \n\t"),
                ],
            ),
            Message::assistant(vec![ContentPart::text(" \n\t")]),
            Message::assistant(vec![]),
            Message::assistant(vec![ContentPart::Reasoning {
                text: String::new(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }]),
            Message::assistant(vec![
                ContentPart::text(""),
                ContentPart::Reasoning {
                    text: String::new(),
                    encrypted: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: Some(provider_metadata(
                        "anthropic",
                        json!({"signature": "sig_1"}),
                    )),
                },
            ]),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [{"type": "text", "text": "  Keep this spacing.  "}]},
            {"role": "assistant", "content": [{"type": "thinking", "thinking": "", "signature": "sig_1"}]},
        ])
    );
}

#[test]
fn lowers_adaptive_thinking_settings_with_effort() {
    let request = base_request(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        provider_options: Some(
            serde_json::from_value(json!({
                "thinking": {"type": "adaptive", "display": "summarized"},
                "effort": "low",
            }))
            .unwrap(),
        ),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(body["thinking"], json!({"type": "adaptive", "display": "summarized"}));
    assert_eq!(body["output_config"], json!({"effort": "low"}));
}

#[test]
fn normalizes_enabled_and_disabled_thinking_settings() {
    let base = base_request(model_of("claude-sonnet-4-5"));
    let with = |options: Value| {
        base.update(LLMRequestPatch {
            provider_options: Some(serde_json::from_value(options).unwrap()),
            ..Default::default()
        })
    };
    let with4096 = |options: Value| {
        with(options).update(LLMRequestPatch {
            generation: Some(GenerationOptions { max_tokens: Some(4_096.0), ..Default::default() }),
            ..Default::default()
        })
    };
    let enabled = compile(&with4096(json!({
        "thinking": {"type": "enabled", "budgetTokens": 1024}
    })))
    .unwrap();
    let legacy = compile(&with4096(json!({
        "thinking": {"type": "enabled", "budget_tokens": 2048}
    })))
    .unwrap();
    let disabled = compile(&with(json!({"thinking": {"type": "disabled"}}))).unwrap();
    jeq(&enabled["thinking"], &json!({"type": "enabled", "budget_tokens": 1024}));
    jeq(&legacy["thinking"], &json!({"type": "enabled", "budget_tokens": 2048}));
    assert_eq!(disabled["thinking"], json!({"type": "disabled"}));
}

#[test]
fn fits_the_thinking_budget_to_half_the_output_limit() {
    let base = base_request(model_of("claude-sonnet-4-5"));
    let thinking = |max_tokens: f64| {
        let request = base.update(LLMRequestPatch {
            generation: Some(GenerationOptions {
                max_tokens: Some(max_tokens),
                ..Default::default()
            }),
            provider_options: Some(
                serde_json::from_value(json!({
                    "thinking": {"type": "enabled", "budgetTokens": 31999}
                }))
                .unwrap(),
            ),
            ..Default::default()
        });
        compile(&request).unwrap()["thinking"].clone()
    };
    jeq(&thinking(64_000.0), &json!({"type": "enabled", "budget_tokens": 31999}));
    jeq(&thinking(20_000.0), &json!({"type": "enabled", "budget_tokens": 10000}));
    jeq(&thinking(1_500.0), &json!({"type": "enabled", "budget_tokens": 1024}));
}

#[test]
fn rejects_enabled_thinking_without_a_budget() {
    let request = base_request(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        provider_options: Some(
            serde_json::from_value(json!({"thinking": {"type": "enabled"}})).unwrap(),
        ),
        ..Default::default()
    });
    let error = compile(&request).unwrap_err();
    assert_eq!(error.tag(), "InvalidRequest");
    assert!(error.message().contains("budgetTokens"), "got: {}", error.message());
}

#[test]
fn lowers_passthrough_provider_options_and_accepts_either_key_spelling() {
    let base = base_request(model_of("claude-sonnet-4-5"));
    let with = |options: Value| {
        base.update(LLMRequestPatch {
            provider_options: Some(serde_json::from_value(options).unwrap()),
            ..Default::default()
        })
    };
    let snake = compile(&with(json!({
        "service_tier": "auto",
        "metadata": {"user_id": "user_1"},
        "container": {"id": "container_1"},
        "inference_geo": "us",
        "cache_control": {"type": "ephemeral", "ttl": "1h"},
        "output_config": {"format": {"type": "json_schema", "schema": {"type": "object"}}},
    })))
    .unwrap();
    let camel = compile(&with(json!({
        "serviceTier": "standard_only",
        "container": "container_2",
        "inferenceGeo": "eu",
        "cacheControl": {"type": "ephemeral"},
        "outputConfig": {"effort": "low"},
    })))
    .unwrap();
    assert_eq!(snake["service_tier"], json!("auto"));
    assert_eq!(snake["metadata"], json!({"user_id": "user_1"}));
    assert_eq!(snake["container"], json!({"id": "container_1"}));
    assert_eq!(snake["inference_geo"], json!("us"));
    assert_eq!(snake["cache_control"], json!({"type": "ephemeral", "ttl": "1h"}));
    assert_eq!(
        snake["output_config"],
        json!({"format": {"type": "json_schema", "schema": {"type": "object"}}})
    );
    assert_eq!(camel["service_tier"], json!("standard_only"));
    assert_eq!(camel["container"], json!("container_2"));
    assert_eq!(camel["inference_geo"], json!("eu"));
    assert_eq!(camel["cache_control"], json!({"type": "ephemeral"}));
    assert_eq!(camel["output_config"], json!({"effort": "low"}));
}

#[test]
fn forwards_unknown_values_for_passthrough_string_enums() {
    let request = base_request(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        provider_options: Some(
            serde_json::from_value(json!({
                "service_tier": "future-tier",
                "thinking": {"type": "adaptive", "display": "future-display"},
            }))
            .unwrap(),
        ),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(body["service_tier"], json!("future-tier"));
    assert_eq!(body["thinking"], json!({"type": "adaptive", "display": "future-display"}));
}

#[test]
fn ignores_unknown_provider_options_and_rejects_malformed_known_ones() {
    let base = base_request(model_of("claude-sonnet-4-5"));
    let prepared = compile(&base.update(LLMRequestPatch {
        provider_options: Some(serde_json::from_value(json!({"unknownOption": true})).unwrap()),
        ..Default::default()
    }))
    .unwrap();
    assert!(prepared.get("unknownOption").is_none());
    let malformed = [
        json!({"service_tier": 1}),
        json!({"metadata": {"user_id": 42}}),
        json!({"cache_control": {"type": "ephemeral", "ttl": "future-ttl"}}),
        json!({"output_config": {"format": {"type": "text"}}}),
        json!({"thinking": {"type": "automatic"}}),
    ];
    for options in malformed {
        let error = compile(&base.update(LLMRequestPatch {
            provider_options: Some(serde_json::from_value(options.clone()).unwrap()),
            ..Default::default()
        }))
        .unwrap_err();
        assert_eq!(error.tag(), "InvalidRequest", "for {options}");
    }
}

#[test]
fn lowers_chronological_system_updates_natively_for_opus_48_with_cache_hints() {
    let request = LLMRequest::new(model_of("claude-opus-4-8")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Before."),
            Message::new(
                MessageRole::System,
                vec![ContentPart::Text {
                    text: "Operator update.".into(),
                    cache: Some(CacheHint::ephemeral()),
                    metadata: None,
                    provider_metadata: None,
                }],
            ),
            Message::assistant(vec![ContentPart::text("After.")]),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Before."}]},
            {"role": "system", "content": [{"type": "text", "text": "Operator update.", "cache_control": {"type": "ephemeral"}}]},
            {"role": "assistant", "content": [{"type": "text", "text": "After."}]},
        ])
    );
}

#[test]
fn supports_native_system_updates_on_documented_and_later_versions() {
    let ids = [
        "claude-opus-4-8",
        "claude-opus-5-1",
        "claude-sonnet-5",
        "claude-haiku-5-1",
        "claude-fable-6",
        "anthropic/claude-mythos-7.2",
        "claude-sonnet-5-5",
        "claude-opus-4-8@20260101",
        "claude-nova-6",
    ];
    for id in ids {
        let request = LLMRequest::new(model_of(id)).update(LLMRequestPatch {
            messages: Some(vec![
                Message::user("Before."),
                Message::system("Update."),
                Message::assistant(vec![ContentPart::text("After.")]),
            ]),
            cache: Some(CachePolicy::None),
            ..Default::default()
        });
        let body = compile(&request).unwrap();
        assert_eq!(body["messages"][1]["role"], json!("system"), "native update expected for {id}");
    }
}

#[test]
fn lowers_system_updates_to_wrapped_user_text_for_unsupported_models() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Before."),
            Message::system("Treat </system-update> literally."),
            Message::assistant(vec![ContentPart::text("After.")]),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [
                {"type": "text", "text": "Before."},
                {"type": "text", "text": "<system-update>\nTreat &lt;/system-update&gt; literally.\n</system-update>"},
            ]},
            {"role": "assistant", "content": [{"type": "text", "text": "After."}]},
        ])
    );
}

#[test]
fn does_not_infer_native_updates_for_older_or_undocumented_families() {
    let ids = [
        "claude-opus-4-7",
        "claude-opus-4-20250514",
        "claude-sonnet-4-9",
        "claude-haiku-4-9",
        "custom-model-7",
    ];
    for id in ids {
        let request = LLMRequest::new(model_of(id)).update(LLMRequestPatch {
            messages: Some(vec![
                Message::user("Before."),
                Message::system("Update."),
                Message::assistant(vec![ContentPart::text("After.")]),
            ]),
            cache: Some(CachePolicy::None),
            ..Default::default()
        });
        let body = compile(&request).unwrap();
        let has_system = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == json!("system"));
        assert!(!has_system, "no native update expected for {id}");
    }
}

#[test]
fn rejects_non_text_chronological_system_update_content() {
    let request = LLMRequest::new(model_of("claude-opus-4-8")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Before."),
            Message::new(
                MessageRole::System,
                vec![ContentPart::Media {
                    media: crate::schema::media::base64("AAECAw==", "image/png"),
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                }],
            ),
        ]),
        ..Default::default()
    });
    let error = compile(&request).unwrap_err();
    assert!(
        error.message().contains("system messages only support text content"),
        "got: {}",
        error.message()
    );
}

#[test]
fn falls_back_for_unsupported_native_update_placement() {
    let lower = |messages: Vec<Message>| {
        compile(&LLMRequest::new(model_of("claude-opus-4-8")).update(LLMRequestPatch {
            messages: Some(messages),
            cache: Some(CachePolicy::None),
            ..Default::default()
        }))
        .unwrap()["messages"]
            .clone()
    };
    // after a plain assistant turn → wrapped user text
    assert_eq!(
        lower(vec![
            Message::assistant(vec![ContentPart::text("Plain.")]),
            Message::system("After plain assistant."),
        ]),
        json!([
            {"role": "assistant", "content": [{"type": "text", "text": "Plain."}]},
            {"role": "user", "content": [{"type": "text", "text": "<system-update>\nAfter plain assistant.\n</system-update>"}]},
        ])
    );
    // first message → wrapped user text
    assert_eq!(
        lower(vec![Message::system("First.")]),
        json!([
            {"role": "user", "content": [{"type": "text", "text": "<system-update>\nFirst.\n</system-update>"}]}
        ])
    );
    // held between assistant turns → released at the next assistant turn
    assert_eq!(
        lower(vec![
            Message::user("Start."),
            Message::assistant(vec![ContentPart::text("One.")]),
            Message::system("Update."),
            Message::assistant(vec![ContentPart::text("Two.")]),
        ]),
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Start."}]},
            {"role": "assistant", "content": [{"type": "text", "text": "One."}]},
            {"role": "user", "content": [{"type": "text", "text": "<system-update>\nUpdate.\n</system-update>"}]},
            {"role": "assistant", "content": [{"type": "text", "text": "Two."}]},
        ])
    );
}

#[test]
fn moves_system_updates_to_the_next_assistant_turn_and_sends_consecutive_updates_together() {
    let lower = |messages: Vec<Message>| {
        compile(&LLMRequest::new(model_of("claude-opus-4-8")).update(LLMRequestPatch {
            messages: Some(messages),
            cache: Some(CachePolicy::None),
            ..Default::default()
        }))
        .unwrap()["messages"]
            .clone()
    };
    assert_eq!(
        lower(vec![
            Message::user("Fix it."),
            Message::assistant(vec![ContentPart::text("Done.")]),
            Message::system("Update."),
            Message::user("Next."),
        ]),
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Fix it."}]},
            {"role": "assistant", "content": [{"type": "text", "text": "Done."}]},
            {"role": "user", "content": [{"type": "text", "text": "Next."}]},
            {"role": "system", "content": [{"type": "text", "text": "Update."}]},
        ])
    );
    assert_eq!(
        lower(vec![Message::user("Before."), Message::system("One."), Message::system("Two."),]),
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Before."}]},
            {"role": "system", "content": [{"type": "text", "text": "One."}]},
            {"role": "system", "content": [{"type": "text", "text": "Two."}]},
        ])
    );
    assert_eq!(
        lower(vec![
            Message::user("Fix it."),
            Message::assistant(vec![ContentPart::text("Done.")]),
            Message::system("One."),
            Message::user("Next."),
            Message::system("Two."),
            Message::assistant(vec![ContentPart::text("After.")]),
        ]),
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Fix it."}]},
            {"role": "assistant", "content": [{"type": "text", "text": "Done."}]},
            {"role": "user", "content": [{"type": "text", "text": "Next."}]},
            {"role": "system", "content": [{"type": "text", "text": "One."}]},
            {"role": "system", "content": [{"type": "text", "text": "Two."}]},
            {"role": "assistant", "content": [{"type": "text", "text": "After."}]},
        ])
    );
    // tool results count as user turns: the update goes native, last
    assert_eq!(
        lower(vec![
            Message::user("Use the tool."),
            Message::tool_call("call_1", "lookup", json!({})),
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Json { value: json!("Done.") }
            ),
            Message::system("Update."),
            Message::user("Also check tests."),
        ]),
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Use the tool."}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "call_1", "name": "lookup", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "\"Done.\""}]},
            {"role": "user", "content": [{"type": "text", "text": "Also check tests."}]},
            {"role": "system", "content": [{"type": "text", "text": "Update."}]},
        ])
    );
}

#[test]
fn keeps_wrapped_updates_in_place_for_models_without_native_updates() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Fix it."),
            Message::assistant(vec![ContentPart::text("Done.")]),
            Message::system("Update."),
            Message::user("Next."),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Fix it."}]},
            {"role": "assistant", "content": [{"type": "text", "text": "Done."}]},
            {"role": "user", "content": [{"type": "text", "text": "<system-update>\nUpdate.\n</system-update>"}]},
            {"role": "user", "content": [{"type": "text", "text": "Next."}]},
        ])
    );
}

#[test]
fn keeps_a_terminal_system_update_native_after_local_tool_results() {
    let request = LLMRequest::new(model_of("claude-opus-4-8")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::tool_call("call_1", "lookup", json!({})),
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Json { value: json!("Done.") },
            ),
            Message::system("Operator update."),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([
            {"role": "assistant", "content": [{"type": "tool_use", "id": "call_1", "name": "lookup", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "\"Done.\""}]},
            {"role": "system", "content": [{"type": "text", "text": "Operator update."}]},
        ])
    );
}

#[test]
fn moves_a_system_update_between_tool_call_and_result_after_the_result() {
    let request = LLMRequest::new(model_of("claude-opus-4-8")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Use the tool."),
            Message::tool_call("call_1", "lookup", json!({})),
            Message::system("Too early."),
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Json { value: json!("Done.") },
            ),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"].as_array().unwrap().iter().skip(1).collect::<Vec<_>>(),
        vec![
            &json!({"role": "assistant", "content": [{"type": "tool_use", "id": "call_1", "name": "lookup", "input": {}}]}),
            &json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "\"Done.\""}]}),
            &json!({"role": "system", "content": [{"type": "text", "text": "Too early."}]}),
        ]
    );
}

#[test]
fn rejects_system_updates_that_split_local_tool_results() {
    // Direct body lowering: through the route pipeline the tool-history
    // normalizer moves the update out of the split first.
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::tool_call("call_1", "lookup", json!({})),
            Message::system("Too early."),
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Json { value: json!("Done.") },
            ),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let error = body_from_request(&request).unwrap_err();
    assert!(error.message().contains("cannot split a local tool call"), "got: {}", error.message());
}

#[test]
fn prepares_tool_call_and_tool_result_messages() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("What is the weather?"),
            Message::tool_call("call_1", "lookup", json!({"query": "weather"})),
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Json { value: json!({"forecast": "sunny"}) },
            ),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    jeq(
        &compile(&request).unwrap(),
        &json!({
            "model": "claude-sonnet-4-5",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "What is the weather?"}]},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "call_1", "name": "lookup", "input": {"query": "weather"}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "{\"forecast\":\"sunny\"}"}]},
            ],
            "stream": true,
            "max_tokens": 32000,
        }),
    );
}

#[test]
fn scrubs_outbound_tool_call_ids_without_truncating() {
    let id = format!("functions.lookup:1|{}", "x".repeat(64));
    let scrubbed = format!("functions_lookup_1_{}", "x".repeat(64));
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::tool_call(id.clone(), "lookup", json!({})),
            Message::tool_result(&id, "lookup", ToolResultValue::Text { value: json!("done") }),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(body["messages"][0]["content"][0]["id"], json!(scrubbed));
    assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], json!(scrubbed));
    assert!(scrubbed.len() > 64);
}

#[test]
fn batches_parallel_tool_results_into_one_user_message() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Check both cities."),
            Message::assistant(vec![
                ContentPart::text("I'll check both."),
                ContentPart::ToolCall {
                    id: "call_paris".into(),
                    name: "weather".into(),
                    namespace: None,
                    input: json!({"city": "Paris"}),
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::ToolCall {
                    id: "call_london".into(),
                    name: "weather".into(),
                    namespace: None,
                    input: json!({"city": "London"}),
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
            ]),
            Message::tool_result(
                "call_paris",
                "weather",
                ToolResultValue::Json { value: json!({"temperature": 22}) },
            ),
            Message::tool_result(
                "call_london",
                "weather",
                ToolResultValue::Json { value: json!({"temperature": 18}) },
            ),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[2]["content"],
        json!([
            {"type": "tool_result", "tool_use_id": "call_paris", "content": "{\"temperature\":22}"},
            {"type": "tool_result", "tool_use_id": "call_london", "content": "{\"temperature\":18}"},
        ])
    );
}

#[test]
fn keeps_tools_and_sends_tool_choice_none() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5"))
        .with_tool(ToolDefinition::new(
            "lookup",
            "Look things up",
            json!({"type": "object", "properties": {}}),
        ))
        .update(LLMRequestPatch {
            tool_choice: Some(ToolChoice::none()),
            messages: Some(vec![
                Message::user("What is the weather?"),
                Message::tool_call("call_1", "lookup", json!({"query": "weather"})),
                Message::tool_result(
                    "call_1",
                    "lookup",
                    ToolResultValue::Json { value: json!({"forecast": "sunny"}) },
                ),
            ]),
            cache: Some(CachePolicy::None),
            ..Default::default()
        });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["tools"],
        json!([{"name": "lookup", "description": "Look things up", "input_schema": {"type": "object", "properties": {}}}])
    );
    assert_eq!(body["tool_choice"], json!({"type": "none"}));
}

#[test]
fn lowers_all_tool_choice_variants() {
    let base = LLMRequest::new(model_of("claude-sonnet-4-5"))
        .with_tool(ToolDefinition::new("lookup", "Look things up", json!({"type": "object"})))
        .update(LLMRequestPatch { cache: Some(CachePolicy::None), ..Default::default() });
    let choice = |tool_choice: ToolChoice| {
        compile(
            &base.update(LLMRequestPatch { tool_choice: Some(tool_choice), ..Default::default() }),
        )
        .unwrap()["tool_choice"]
            .clone()
    };
    assert_eq!(choice(ToolChoice::auto()), json!({"type": "auto"}));
    assert_eq!(choice(ToolChoice::required()), json!({"type": "any"}));
    assert_eq!(choice(ToolChoice::named("lookup")), json!({"type": "tool", "name": "lookup"}));
    assert_eq!(
        choice(ToolChoice::Auto { disable_parallel_tool_use: Some(true) }),
        json!({"type": "auto", "disable_parallel_tool_use": true})
    );
    assert_eq!(
        choice(ToolChoice::Required { disable_parallel_tool_use: Some(true) }),
        json!({"type": "any", "disable_parallel_tool_use": true})
    );
    assert_eq!(
        choice(ToolChoice::Named { name: "lookup".into(), disable_parallel_tool_use: Some(false) }),
        json!({"type": "tool", "name": "lookup", "disable_parallel_tool_use": false})
    );
    let error = compile(&base.update(LLMRequestPatch {
        tool_choice: Some(ToolChoice::named("")),
        ..Default::default()
    }))
    .unwrap_err();
    assert!(error.message().contains("requires a tool name"));
}

#[test]
fn lowers_media_tool_result_content_as_structured_blocks() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
            messages: Some(vec![
                Message::user("Show me the screenshot."),
                Message::tool_call("call_1", "read", json!({"filePath": "shot.png"})),
                Message::tool_result(
                    "call_1",
                    "read",
                    ToolResultValue::Content {
                        value: crate::schema::ToolContentItems(vec![
                            json!({"type": "text", "text": "Image read successfully"}),
                            json!({"type": "file", "uri": "data:image/png;base64,AAECAw==", "mime": "image/png"}),
                            json!({"type": "file", "uri": "data:application/pdf;base64,JVBERi0xLjQ=", "mime": "application/pdf"}),
                        ]),
                    },
                ),
            ]),
            cache: Some(CachePolicy::None),
            ..Default::default()
        });
    let body = compile(&request).unwrap();
    let tool_result = body["messages"][2]["content"][0].clone();
    assert_eq!(
        tool_result["content"],
        json!([
            {"type": "text", "text": "Image read successfully"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAECAw=="}},
            {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": "JVBERi0xLjQ="}},
        ])
    );
}

#[test]
fn lowers_single_image_tool_result_content_as_a_structured_image_block() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::tool_call("call_1", "screenshot", json!({})),
            Message::tool_result(
                "call_1",
                "screenshot",
                ToolResultValue::Content {
                    value: crate::schema::ToolContentItems(vec![json!({
                        "type": "file",
                        "uri": "data:image/jpeg;base64,/9j/AA==",
                        "mime": "image/jpeg"
                    })]),
                },
            ),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"][1]["content"][0]["content"],
        json!([{"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "/9j/AA=="}}])
    );
}

#[test]
fn rejects_tool_result_media_that_cannot_be_lowered() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::tool_call("call_1", "fetch", json!({})),
            Message::tool_result(
                "call_1",
                "fetch",
                ToolResultValue::Content {
                    value: crate::schema::ToolContentItems(vec![json!({
                        "type": "file",
                        "uri": "data:audio/mpeg;base64,AAECAw==",
                        "mime": "audio/mpeg"
                    })]),
                },
            ),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let error = compile(&request).unwrap_err();
    assert!(
        error.message().contains("does not support media type audio/mpeg"),
        "got: {}",
        error.message()
    );
}

#[test]
fn lowers_preserved_reasoning_signature_metadata() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![Message::assistant(vec![ContentPart::Reasoning {
            text: "thinking".into(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: Some(provider_metadata("anthropic", json!({"signature": "sig_1"}))),
        }])]),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([{"role": "assistant", "content": [{"type": "thinking", "thinking": "thinking", "signature": "sig_1"}]}])
    );
}

#[test]
fn demotes_unsigned_reasoning_when_signatures_are_required() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![Message::assistant(vec![ContentPart::Reasoning {
            text: "unsigned reasoning".into(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }])]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([{"role": "assistant", "content": [{"type": "text", "text": "unsigned reasoning"}]}])
    );
}

#[test]
fn infers_empty_signature_compatibility_across_kimi_providers() {
    let unsigned = |model: LanguageModel| {
        compile(&LLMRequest::new(model).update(LLMRequestPatch {
            messages: Some(vec![Message::assistant(vec![ContentPart::Reasoning {
                text: "unsigned reasoning".into(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }])]),
            cache: Some(CachePolicy::None),
            ..Default::default()
        }))
        .unwrap()["messages"]
            .clone()
    };
    let coding = test_route()
        .with(RoutePatch { provider: Some("kimi-for-coding".into()), ..Default::default() })
        .model("k3", None)
        .unwrap();
    let moonshot = test_route()
        .with(RoutePatch {
            provider: Some("moonshotai".into()),
            endpoint: Some(EndpointPatch {
                base_url: Some("https://api.moonshot.ai/anthropic".into()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .model("kimi-k2.6", None)
        .unwrap();
    let expected = json!([{"role": "assistant", "content": [{"type": "thinking", "thinking": "unsigned reasoning", "signature": ""}]}]);
    assert_eq!(unsigned(coding), expected);
    assert_eq!(unsigned(moonshot), expected);
}

#[test]
fn lets_an_explicit_signature_requirement_override_inference() {
    let compatible = test_route()
        .with(RoutePatch {
            provider: Some("kimi-for-coding".into()),
            endpoint: Some(EndpointPatch {
                base_url: Some("https://api.kimi.com/coding/v1/".into()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .model("k3", None)
        .unwrap()
        .with_compatibility(LanguageModelCompatibility {
            require_signature: Some(true),
            ..Default::default()
        });
    let request = LLMRequest::new(compatible).update(LLMRequestPatch {
        messages: Some(vec![Message::assistant(vec![ContentPart::Reasoning {
            text: "unsigned reasoning".into(),
            encrypted: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }])]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([{"role": "assistant", "content": [{"type": "text", "text": "unsigned reasoning"}]}])
    );
}

#[test]
fn round_trips_redacted_thinking_as_redacted_thinking_blocks() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![Message::assistant(vec![
            ContentPart::Reasoning {
                text: String::new(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: Some(provider_metadata(
                    "anthropic",
                    json!({"redactedData": "opaque_1"}),
                )),
            },
            ContentPart::Reasoning {
                text: "visible".into(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: Some(provider_metadata(
                    "anthropic",
                    json!({"signature": "sig_1"}),
                )),
            },
        ])]),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([{"role": "assistant", "content": [
            {"type": "redacted_thinking", "data": "opaque_1"},
            {"type": "thinking", "thinking": "visible", "signature": "sig_1"},
        ]}])
    );
}

#[test]
fn round_trips_provider_executed_assistant_content_into_server_tool_blocks() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Search for something."),
            Message::assistant(vec![
                ContentPart::ToolCall {
                    id: "srvtoolu.abc".into(),
                    name: "web_search".into(),
                    namespace: None,
                    input: json!({"query": "effect 4"}),
                    provider_executed: Some(true),
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::ToolResult {
                    id: "srvtoolu.abc".into(),
                    name: "web_search".into(),
                    namespace: None,
                    result: ToolResultValue::Json {
                        value: json!([{"url": "https://example.com"}]),
                    },
                    provider_executed: Some(true),
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::text("Found it."),
            ]),
            Message::user("Thanks."),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Search for something."}]},
            {"role": "assistant", "content": [
                {"type": "server_tool_use", "id": "srvtoolu_abc", "name": "web_search", "input": {"query": "effect 4"}},
                {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_abc", "content": [{"url": "https://example.com"}]},
                {"type": "text", "text": "Found it."},
            ]},
            {"role": "user", "content": [{"type": "text", "text": "Thanks."}]},
        ])
    );
}

#[test]
fn rejects_round_trip_for_unknown_server_tool_names() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![Message::assistant(vec![ContentPart::ToolResult {
            id: "srvtoolu_abc".into(),
            name: "future_server_tool".into(),
            namespace: None,
            result: ToolResultValue::Json { value: json!({}) },
            provider_executed: Some(true),
            cache: None,
            metadata: None,
            provider_metadata: None,
        }])]),
        ..Default::default()
    });
    let error = compile(&request).unwrap_err();
    assert!(error.message().contains("future_server_tool"), "got: {}", error.message());
}

#[test]
fn continues_a_conversation_with_user_media_content() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        cache: Some(CachePolicy::None),
        messages: Some(vec![Message::new(
            MessageRole::User,
            vec![
                ContentPart::text("What is in this image?"),
                ContentPart::Media {
                    media: crate::schema::media::base64("AAECAw==", "image/png"),
                    filename: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
                ContentPart::Media {
                    media: crate::schema::media::base64("JVBERi0xLjQ=", "application/pdf"),
                    filename: Some("report.pdf".into()),
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                },
            ],
        )]),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"],
        json!([{"role": "user", "content": [
            {"type": "text", "text": "What is in this image?"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAECAw=="}},
            {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": "JVBERi0xLjQ="}, "title": "report.pdf"},
        ]}])
    );
}

#[test]
fn maps_long_ttl_to_the_one_hour_bucket() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        system: Some(vec![SystemPart {
            text: "system".into(),
            cache: Some(CacheHint::ephemeral_ttl(3600.0)),
            metadata: None,
        }]),
        messages: Some(vec![Message::user("hi")]),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["system"],
        json!([{"type": "text", "text": "system", "cache_control": {"type": "ephemeral", "ttl": "1h"}}])
    );
}

#[test]
fn emits_cache_control_on_tool_definitions_and_tool_result_blocks() {
    let hint = CacheHint::ephemeral();
    let request = LLMRequest::new(model_of("claude-sonnet-4-5"))
        .with_tool(ToolDefinition {
            cache: Some(hint.clone()),
            ..ToolDefinition::new(
                "lookup",
                "lookup tool",
                json!({"type": "object", "properties": {}}),
            )
        })
        .update(LLMRequestPatch {
            messages: Some(vec![
                Message::user("What's the weather?"),
                Message::tool_call("call_1", "lookup", json!({})),
                Message::new(
                    MessageRole::Tool,
                    vec![ContentPart::ToolResult {
                        id: "call_1".into(),
                        name: "lookup".into(),
                        namespace: None,
                        result: ToolResultValue::Json { value: json!({"temp": 72}) },
                        provider_executed: None,
                        cache: Some(hint),
                        metadata: None,
                        provider_metadata: None,
                    }],
                ),
            ]),
            ..Default::default()
        });
    let body = compile(&request).unwrap();
    assert_eq!(body["tools"][0]["cache_control"], json!({"type": "ephemeral"}));
    assert_eq!(body["messages"][2]["content"][0]["cache_control"], json!({"type": "ephemeral"}));
}

#[test]
fn drops_cache_breakpoints_past_the_four_per_request_cap() {
    let hint = CacheHint::ephemeral();
    let system: Vec<SystemPart> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .map(|text| SystemPart {
            text: (*text).to_string(),
            cache: Some(hint.clone()),
            metadata: None,
        })
        .collect();
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        system: Some(system),
        messages: Some(vec![Message::user("hi")]),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    let system = body["system"].as_array().unwrap();
    let marked = system.iter().filter(|part| part.get("cache_control").is_some()).count();
    assert_eq!(marked, 4);
    assert!(system[4].get("cache_control").is_none());
    assert!(system[5].get("cache_control").is_none());
}

#[test]
fn spends_breakpoint_budget_on_tools_before_system_before_messages() {
    let hint = CacheHint::ephemeral();
    let mut request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        system: Some(vec![SystemPart {
            text: "system-tail".into(),
            cache: Some(hint.clone()),
            metadata: None,
        }]),
        messages: Some(vec![Message::new(
            MessageRole::User,
            vec![ContentPart::Text {
                text: "message-tail".into(),
                cache: Some(hint.clone()),
                metadata: None,
                provider_metadata: None,
            }],
        )]),
        ..Default::default()
    });
    for name in ["t1", "t2", "t3", "t4"] {
        request = request.with_tool(ToolDefinition {
            cache: Some(hint.clone()),
            ..ToolDefinition::new(name, name, json!({"type": "object", "properties": {}}))
        });
    }
    let body = compile(&request).unwrap();
    assert!(body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|tool| tool.get("cache_control").is_some()));
    assert!(body["system"][0].get("cache_control").is_none());
    assert!(body["messages"][0]["content"][0].get("cache_control").is_none());
}

#[test]
fn lowers_context_management_and_compaction_replay() {
    let options = serde_json::from_value(json!({
            "contextManagement": {"edits": [{"type": "compact_20260112", "trigger": {"type": "input_tokens", "value": 60000}}]}
        }))
        .unwrap();
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        provider_options: Some(options),
        messages: Some(vec![
            Message::assistant(vec![ContentPart::compaction_summary(
                "anthropic",
                Some("summary".into()),
            )]),
            Message::user("Continue."),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    jeq(
        &body["context_management"],
        &json!({"edits": [{"type": "compact_20260112", "trigger": {"type": "input_tokens", "value": 60000}}]}),
    );
    assert_eq!(
        body["messages"][0]["content"][0],
        json!({"type": "compaction", "content": "summary"})
    );
}

#[test]
fn rejects_compaction_replay_from_another_provider() {
    let request = LLMRequest::new(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![Message::assistant(vec![ContentPart::compaction_summary(
            "other-provider",
            Some("summary".into()),
        )])]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let error = compile(&request).unwrap_err();
    assert!(error.message().contains("originating provider"), "got: {}", error.message());
}

#[test]
fn rejects_malformed_context_management() {
    for options in [
        json!({"contextManagement": {"edits": [{"type": "compact_20260112", "trigger": {"type": "input_tokens", "value": 100}}]}}),
        json!({"contextManagement": {"edits": [{"type": "future_compact"}]}}),
    ] {
        let request = base_request(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
            provider_options: Some(serde_json::from_value(options.clone()).unwrap()),
            ..Default::default()
        });
        let error = compile(&request).unwrap_err();
        assert_eq!(error.tag(), "InvalidRequest", "for {options}");
    }
}

// -- beta headers + route plumbing ---------------------------------------

async fn sent_headers(request: LLMRequest) -> Vec<(String, String)> {
    let route = request.model.route.clone();
    let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://api.anthropic.test/v1/messages".into(),
        status: 200,
        body: sse(&[
            json!({
                "type": "message_start", "message": {"usage": {"input_tokens": 5}}
            }),
            json!({
                "type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}
            }),
            json!({"type": "message_stop"}),
        ]),
        request_body: None,
    }]));
    let replay = route.with(RoutePatch {
        transport: Some(BetaHeadersTransport::wrap(Arc::clone(&transport) as SharedTransport)),
        ..Default::default()
    });
    replay.generate(request).await.expect("stream completes");
    // headers ride on the request passed to the (beta-wrapping) transport
    transport.requests()[0].headers.clone()
}

#[tokio::test]
async fn always_sends_interleaved_thinking_and_version_headers() {
    let headers = sent_headers(base_request(model_of("claude-sonnet-4-5"))).await;
    let get = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    };
    assert_eq!(get("anthropic-version").as_deref(), Some("2023-06-01"));
    assert_eq!(get("anthropic-beta").as_deref(), Some("interleaved-thinking-2025-05-14"));
}

#[tokio::test]
async fn adds_compact_beta_when_compaction_is_requested_or_replayed() {
    let options = serde_json::from_value(json!({
        "contextManagement": {"edits": [{"type": "compact_20260112"}]}
    }))
    .unwrap();
    let requested = base_request(model_of("claude-sonnet-4-5"))
        .update(LLMRequestPatch { provider_options: Some(options), ..Default::default() });
    let headers = sent_headers(requested).await;
    let beta = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.clone())
        .unwrap();
    assert!(beta.contains("interleaved-thinking-2025-05-14"));
    assert!(beta.contains("compact-2026-01-12"));

    let replayed = base_request(model_of("claude-sonnet-4-5")).update(LLMRequestPatch {
        messages: Some(vec![Message::assistant(vec![ContentPart::compaction_summary(
            "anthropic",
            Some("summary".into()),
        )])]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let headers = sent_headers(replayed).await;
    let beta = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.clone())
        .unwrap();
    assert!(beta.contains("compact-2026-01-12"));
}

#[tokio::test]
async fn adds_output_config_beta_for_effort_update_markers() {
    let options = serde_json::from_value(json!({"effort": "low"})).unwrap();
    let request = LLMRequest::new(model_of("claude-sonnet-5-1")).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("hi"),
            Message::effort(Some("low".into()), Some("high".into())),
        ]),
        provider_options: Some(options),
        ..Default::default()
    });
    let body = compile(&request).unwrap();
    assert_eq!(
        body["messages"][1],
        json!({"role": "system", "content": [], "output_config": {"effort": "low"}})
    );
    // the top-level effort resolves to the marker's `previous` value
    assert_eq!(body["output_config"], json!({"effort": "high"}));
    let headers = sent_headers(request).await;
    let beta = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.clone())
        .unwrap();
    assert!(beta.contains("mid-conversation-output-config-2026-07-01"), "got: {beta}");
}

#[tokio::test]
async fn adds_thinking_binding_beta_on_supported_models() {
    // claude-sonnet-5-5 supports the controls; an unset thinking field
    // defaults to adaptive + block_binding.
    let request = base_request(model_of("claude-sonnet-5-5"));
    let body = compile(&request).unwrap();
    assert_eq!(
        body["thinking"],
        json!({
            "type": "adaptive",
            "block_binding": {"prefix_mismatch_behavior": "drop_block"}
        })
    );
    let headers = sent_headers(base_request(model_of("claude-sonnet-5-5"))).await;
    let beta = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.clone())
        .unwrap();
    assert!(beta.contains(THINKING_BINDING_BETA), "got: {beta}");

    // unsupported models keep thinking unset and omit the beta
    let headers = sent_headers(base_request(model_of("claude-sonnet-4-5"))).await;
    let beta = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.clone())
        .unwrap();
    assert!(!beta.contains(THINKING_BINDING_BETA), "got: {beta}");
}

#[tokio::test]
async fn merges_caller_betas_with_the_required_set() {
    let mut request = base_request(model_of("claude-sonnet-4-5"));
    request.http = Some(crate::schema::HttpOptions {
        headers: Some(BTreeMap::from([(
            "anthropic-beta".to_string(),
            "custom-beta-1".to_string(),
        )])),
        ..Default::default()
    });
    let headers = sent_headers(request).await;
    let beta = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.clone())
        .unwrap();
    assert_eq!(beta, "custom-beta-1,interleaved-thinking-2025-05-14");
}

#[test]
fn targets_the_beta_query_parameter_for_the_native_provider_only() {
    use crate::route::endpoint::{render, EndpointInput};
    let request = base_request(model_of("claude-sonnet-4-5"));
    let body = json!({});
    let url =
        render(&request.model.route.endpoint, &EndpointInput { request: &request, body: &body });
    assert_eq!(url.to_string(), "https://api.anthropic.test/v1/messages?beta=true");
    let compatible = test_route()
        .with(RoutePatch { provider: Some("minimax".into()), ..Default::default() })
        .model("MiniMax-M3", None)
        .unwrap();
    let request = base_request(compatible);
    let url =
        render(&request.model.route.endpoint, &EndpointInput { request: &request, body: &body });
    assert_eq!(url.to_string(), "https://api.anthropic.test/v1/messages");
}

#[test]
fn anthropic_facade_auth_rejects_api_key_plus_auth_token() {
    let error = anthropic_route_auth(
        Some(Secret::Value("api".into())),
        Some(Secret::Value("token".into())),
    )
    .unwrap_err();
    assert_eq!(error.tag(), "ProviderConfiguration");
    assert!(error.message().contains("cannot be combined"));
    let bearer = anthropic_route_auth(None, Some(Secret::Value("token".into()))).unwrap();
    let input = auth::AuthInput {
        request_http: None,
        method: "POST",
        url: "https://x.test",
        body: "",
        headers: auth::Headers::new(),
    };
    let headers = bearer.apply(&input).unwrap();
    assert_eq!(headers.get("authorization"), Some("Bearer token"));
    // api key path renders x-api-key
    let header = anthropic_route_auth(Some(Secret::Value("k-1".into())), None).unwrap();
    let headers = header.apply(&input).unwrap();
    assert_eq!(headers.get("x-api-key"), Some("k-1"));
}

// -- stream parsing -------------------------------------------------------

#[tokio::test]
async fn parses_text_reasoning_and_usage_stream_fixtures() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5, "cache_read_input_tokens": 1}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "!"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "thinking", "thinking": ""}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "thinking_delta", "thinking": "thinking"}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "signature_delta", "signature": "sig_1"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": "\n\nHuman:"}, "usage": {"output_tokens": 2}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(response.text(), "Hello!");
    assert_eq!(response.reasoning(), "thinking");
    let usage = response.usage.clone().unwrap();
    assert_eq!(usage.input_tokens, Some(6.0));
    assert_eq!(usage.output_tokens, Some(2.0));
    assert_eq!(usage.non_cached_input_tokens, Some(5.0));
    assert_eq!(usage.cache_read_input_tokens, Some(1.0));
    assert_eq!(usage.total_tokens, Some(8.0));
    let reasoning_ends: Vec<&LLMEvent> =
        response.events.iter().filter(|event| event.kind() == "reasoning-end").collect();
    assert_eq!(reasoning_ends.len(), 1);
    assert_eq!(
        wire(reasoning_ends[0])["providerMetadata"],
        json!({"anthropic": {"signature": "sig_1"}})
    );
    assert!(response
        .events
        .iter()
        .all(|event| !matches!(event, LLMEvent::ReasoningDelta { text, .. } if text.is_empty())));
    let last = wire(response.events.last().unwrap());
    assert_eq!(last["type"], "finish");
    assert_eq!(last["reason"], json!({"normalized": "stop", "raw": "end_turn"}));
    assert_eq!(last["providerMetadata"], json!({"anthropic": {"stopSequence": "\n\nHuman:"}}));
}

#[tokio::test]
async fn preserves_terminal_state_across_usage_only_message_deltas() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": "X"}, "usage": {"output_tokens": 8}}),
        json!({"type": "message_delta", "delta": {}, "usage": {"output_tokens": 10}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    let usage = response.usage.clone().unwrap();
    assert_eq!(usage.input_tokens, Some(5.0));
    assert_eq!(usage.output_tokens, Some(10.0));
    assert_eq!(usage.total_tokens, Some(15.0));
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "stop", "raw": "end_turn"})
    );
    let step_finish: Vec<&LLMEvent> =
        response.events.iter().filter(|event| event.kind() == "step-finish").collect();
    assert_eq!(step_finish.len(), 1);
    let step_finish = wire(step_finish[0]);
    jeq(&step_finish["usage"]["inputTokens"], &json!(5));
    jeq(&step_finish["usage"]["outputTokens"], &json!(10));
    jeq(&step_finish["usage"]["totalTokens"], &json!(15));
    assert_eq!(step_finish["providerMetadata"], json!({"anthropic": {"stopSequence": "X"}}));
}

#[tokio::test]
async fn requires_message_stop_before_completing_a_streamed_message() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
    ]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.tag(), "InvalidProviderOutput");
    assert_eq!(
        classification_of(&error).as_deref(),
        Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM)
    );
    assert_eq!(error.message(), "The provider response ended unexpectedly.");
}

#[tokio::test]
async fn ignores_unknown_named_sse_events() {
    let body = sse_raw(&[
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5}}}",
            "event: proxy.stats\ndata: not json",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}",
            "event: proxy.done\ndata: still not json",
        ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(response.text(), "Hello");
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "stop", "raw": "end_turn"})
    );
}

#[tokio::test]
async fn ignores_unknown_content_block_and_delta_variants() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "future_event", "content_block": 42, "delta": 42}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "future_block", "text": 42}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"text": "ignored"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "future_delta", "text": 42}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hidden"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "hidden"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "hidden"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Hello"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(response.text(), "Hello");
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "stop", "raw": "end_turn"})
    );
}

#[tokio::test]
async fn rejects_malformed_recognized_content_block_variants() {
    for block in [
        json!({"type": "text", "text": 42}),
        json!({"type": "thinking", "thinking": 42}),
        json!({"type": "tool_use", "id": 42}),
    ] {
        let body = sse(&[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": block}),
        ]);
        let error =
            generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
        assert_eq!(
            error.message(),
            "Invalid anthropic/anthropic-messages stream event",
            "for block {block}"
        );
    }
}

#[tokio::test]
async fn rejects_malformed_recognized_content_delta_variants() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": 42}}),
    ]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.message(), "Invalid anthropic/anthropic-messages stream event");
}

#[tokio::test]
async fn rejects_malformed_payloads_on_unrelated_stream_events() {
    let events = [
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 1}}, "delta": 42}),
        json!({"type": "content_block_start", "index": 0}),
        json!({"type": "content_block_delta", "index": 0}),
        json!({"type": "content_block_stop", "index": 0, "content_block": {"type": "text", "text": 42}}),
        json!({"type": "message_delta"}),
        json!({"type": "message_delta", "delta": {"stop_reason": 42}}),
        json!({"type": "message_stop", "delta": {"text": 42}}),
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}, "content_block": 42}),
    ];
    for event in events {
        let error = generate_with(
            sse(std::slice::from_ref(&event)),
            base_request(model_of("claude-sonnet-4-5")),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.message(),
            "Invalid anthropic/anthropic-messages stream event",
            "for {event}"
        );
    }
}

#[tokio::test]
async fn rejects_malformed_recognized_sse_events() {
    let body = sse_raw(&["event: message_start\ndata: [DONE]"]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.message(), "Invalid anthropic/anthropic-messages stream event");
}

#[tokio::test]
async fn maps_nullable_input_tokens_and_preserves_unknown_usage_fields() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {
            "input_tokens": 5,
            "cache_read_input_tokens": 2,
            "service_tier": "standard",
            "cache_creation": {"ephemeral_5m_input_tokens": 1},
            "server_tool_use": {"web_search_requests": 1, "start_counter": 2},
            "output_tokens_details": {"thinking_tokens": 3, "start_detail": "preserved"},
        }}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {
            "input_tokens": null,
            "output_tokens": 8,
            "server_tool_use": {"web_search_requests": 2, "terminal_counter": 3},
            "output_tokens_details": {"terminal_detail": "preserved"},
            "future_terminal": {"requests": 4},
        }}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(7.0));
    assert_eq!(usage.output_tokens, Some(8.0));
    assert_eq!(usage.reasoning_tokens, Some(3.0));
    assert_eq!(usage.total_tokens, Some(15.0));
    let raw = usage.provider_metadata.unwrap()["anthropic"].clone();
    assert_eq!(raw["input_tokens"], json!(null));
    assert_eq!(raw["cache_read_input_tokens"], json!(2));
    assert_eq!(raw["service_tier"], json!("standard"));
    assert_eq!(raw["cache_creation"], json!({"ephemeral_5m_input_tokens": 1}));
    assert_eq!(
        raw["server_tool_use"],
        json!({"web_search_requests": 2, "start_counter": 2, "terminal_counter": 3})
    );
    assert_eq!(raw["output_tokens"], json!(8));
    assert_eq!(
        raw["output_tokens_details"],
        json!({"thinking_tokens": 3, "start_detail": "preserved", "terminal_detail": "preserved"})
    );
    assert_eq!(raw["future_terminal"], json!({"requests": 4}));
}

#[tokio::test]
async fn round_trips_omitted_thinking_carried_only_by_a_signature_delta() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig_1"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let model = model_of("claude-sonnet-4-5");
    let response = generate_with(body, base_request(model.clone())).await.unwrap();
    assert_eq!(
        content_wire(&response.message)["content"],
        json!([{"type": "reasoning", "text": "", "providerMetadata": {"anthropic": {"signature": "sig_1"}}}])
    );
    // ...and the message round-trips back into a thinking block
    let request = LLMRequest::new(model).update(LLMRequestPatch {
        messages: Some(vec![response.message]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let lowered = compile(&request).unwrap();
    assert_eq!(
        lowered["messages"],
        json!([{"role": "assistant", "content": [{"type": "thinking", "thinking": "", "signature": "sig_1"}]}])
    );
}

#[tokio::test]
async fn retains_a_thinking_signature_supplied_in_content_block_start() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": "sig_1"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(
        content_wire(&response.message)["content"],
        json!([{"type": "reasoning", "text": "", "providerMetadata": {"anthropic": {"signature": "sig_1"}}}])
    );
    let reasoning_end =
        response.events.iter().find(|event| event.kind() == "reasoning-end").unwrap();
    assert_eq!(
        wire(reasoning_end)["providerMetadata"],
        json!({"anthropic": {"signature": "sig_1"}})
    );
}

#[tokio::test]
async fn retains_complete_tool_input_from_content_block_start() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "call_1", "name": "lookup", "input": {"query": "weather"}}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(response.tool_calls().len(), 1);
    let call = wire(response.tool_calls()[0]);
    assert_eq!(call["input"], json!({"query": "weather"}));
}

#[tokio::test]
async fn retains_empty_text_blocks() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(content_wire(&response.message)["content"], json!([{"type": "text", "text": ""}]));
}

#[tokio::test]
async fn parses_redacted_thinking_into_empty_reasoning_with_redacted_metadata() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "redacted_thinking", "data": "opaque_1"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Hello"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    let start = response.events.iter().find(|event| event.kind() == "reasoning-start").unwrap();
    assert_eq!(wire(start)["providerMetadata"], json!({"anthropic": {"redactedData": "opaque_1"}}));
    assert_eq!(
        content_wire(&response.message)["content"],
        json!([
            {"type": "reasoning", "text": "", "providerMetadata": {"anthropic": {"redactedData": "opaque_1"}}},
            {"type": "text", "text": "Hello"},
        ])
    );
}

#[tokio::test]
async fn round_trips_streamed_redacted_thinking_with_tool_use() {
    let redacted_data = "cmVkYWN0ZWQtdGhpbmtpbmc=";
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "redacted_thinking", "data": redacted_data}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "call_1", "name": "lookup"}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"weather\"}"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let model = model_of("claude-sonnet-4-5");
    let response = generate_with(body, base_request(model.clone())).await.unwrap();
    let request = LLMRequest::new(model).update(LLMRequestPatch {
        messages: Some(vec![
            Message::user("Say hello."),
            response.message,
            Message::tool_result(
                "call_1",
                "lookup",
                ToolResultValue::Text { value: json!("sunny") },
            ),
        ]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let lowered = compile(&request).unwrap();
    assert_eq!(
        lowered["messages"],
        json!([
            {"role": "user", "content": [{"type": "text", "text": "Say hello."}]},
            {"role": "assistant", "content": [
                {"type": "redacted_thinking", "data": redacted_data},
                {"type": "tool_use", "id": "call_1", "name": "lookup", "input": {"query": "weather"}},
            ]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "sunny"}]},
        ])
    );
}

#[tokio::test]
async fn maps_context_window_truncation_to_length() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "model_context_window_exceeded"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "length", "raw": "model_context_window_exceeded"})
    );
}

#[tokio::test]
async fn preserves_pause_turn_while_normalizing_it_to_stop() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "pause_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "stop", "raw": "pause_turn"})
    );
}

#[tokio::test]
async fn carries_a_refusals_category_and_explanation_on_the_finish() {
    async fn refusal(stop_details: Value) -> crate::schema::FinishReasonDetails {
        let body = sse(&[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "refusal", "stop_details": stop_details}, "usage": {"output_tokens": 0}}),
            json!({"type": "message_stop"}),
        ]);
        generate_with(body, base_request(model_of("claude-sonnet-4-5")))
            .await
            .unwrap()
            .finish_reason
            .clone()
    }
    assert_eq!(
        serde_json::to_value(
            refusal(json!({
                "type": "refusal",
                "category": "cyber",
                "explanation": "This request was declined because it could enable cyber harm."
            }))
            .await
        )
        .unwrap(),
        json!({
            "normalized": "content-filter",
            "raw": "refusal",
            "category": "cyber",
            "explanation": "This request was declined because it could enable cyber harm."
        })
    );
    assert_eq!(
        serde_json::to_value(
            refusal(json!({"type": "refusal", "category": null, "explanation": null})).await
        )
        .unwrap(),
        json!({"normalized": "content-filter", "raw": "refusal"})
    );
}

#[tokio::test]
async fn assembles_streamed_tool_call_input_event_for_event() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "call_1", "name": "lookup"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"query\""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": ":\"weather\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let model = model_of("claude-sonnet-4-5");
    let request = base_request(model.clone()).with_tool(ToolDefinition::new(
        "lookup",
        "Lookup data",
        json!({"type": "object"}),
    ));
    let response = generate_with(body, request).await.unwrap();
    let usage = json!({
        "inputTokens": 5,
        "outputTokens": 1,
        "nonCachedInputTokens": 5,
        "totalTokens": 6,
        "providerMetadata": {"anthropic": {"input_tokens": 5, "output_tokens": 1}},
    });
    let expected = json!([
        {"type": "step-start", "index": 0},
        {"type": "tool-input-start", "id": "call_1", "name": "lookup"},
        {"type": "tool-input-delta", "id": "call_1", "name": "lookup", "text": "{\"query\"", "input": {}},
        {"type": "tool-input-delta", "id": "call_1", "name": "lookup", "text": ":\"weather\"}", "input": {"query": "weather"}},
        {"type": "tool-input-end", "id": "call_1", "name": "lookup"},
        {"type": "tool-call", "id": "call_1", "name": "lookup", "input": {"query": "weather"}},
        {"type": "step-finish", "index": 0, "reason": {"normalized": "tool-calls", "raw": "tool_use"}, "usage": usage},
        {"type": "finish", "reason": {"normalized": "tool-calls", "raw": "tool_use"}, "usage": usage},
    ]);
    // The TS fixture lists `providerMetadata: undefined` (absent on the
    // wire); treat explicit null and absent as equal.
    let actual: Vec<Value> = response.events.iter().map(wire).collect();
    assert_eq!(actual.len(), expected.as_array().unwrap().len());
    for (actual, expected) in actual.iter().zip(expected.as_array().unwrap()) {
        jeq(actual, expected);
    }
}

#[tokio::test]
async fn ignores_tool_input_deltas_without_a_matching_tool_start() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello"}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"orphaned\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(response.text(), "Hello");
    assert_eq!(response.tool_calls().len(), 0);
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "stop", "raw": "end_turn"})
    );
}

#[tokio::test]
async fn settles_pending_tool_calls_at_message_stop() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "call_1", "name": "lookup"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"weather\"}"}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    assert_eq!(response.tool_calls().len(), 1);
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "tool-calls", "raw": "tool_use"})
    );
}

#[tokio::test]
async fn assembles_and_persists_multiple_tool_calls_from_one_response() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "call_paris", "name": "weather", "input": {}}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"city\":\"Paris\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "call_london", "name": "weather", "input": {}}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"city\":\"London\"}"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 2}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    let calls: Vec<Value> = response.tool_calls().iter().map(wire).collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["input"], json!({"city": "Paris"}));
    assert_eq!(calls[1]["input"], json!({"city": "London"}));
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "tool-calls", "raw": "tool_use"})
    );
}

#[tokio::test]
async fn keeps_malformed_server_tool_input_terminal() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "server_tool_use", "id": "call_1", "name": "web_search"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"partial"}}),
        json!({"type": "content_block_stop", "index": 0}),
    ]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert!(
        error.message().contains("Invalid JSON input for anthropic-messages tool call web_search"),
        "got: {}",
        error.message()
    );
}

#[tokio::test]
async fn fails_with_a_classified_provider_error_for_stream_error_frames() {
    let body = sse(&[json!({
        "type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}
    })]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.tag(), "ProviderInternal");
    assert_eq!(error.message(), "overloaded_error: Overloaded");
}

#[tokio::test]
async fn classifies_prompt_too_long_provider_errors() {
    let body = sse(&[json!({
        "type": "error",
        "error": {"type": "invalid_request_error", "message": "prompt is too long: 210000 tokens"}
    })]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.tag(), "InvalidRequest");
    assert_eq!(
        classification_of(&error).as_deref(),
        Some(crate::schema::CLASSIFICATION_CONTEXT_OVERFLOW)
    );
    assert_eq!(error.message(), "invalid_request_error: prompt is too long: 210000 tokens");
}

#[tokio::test]
async fn falls_back_to_error_type_when_no_message_is_present() {
    let body = sse(&[json!({
        "type": "error", "error": {"type": "overloaded_error", "message": ""}
    })]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.tag(), "ProviderInternal");
    assert_eq!(error.message(), "overloaded_error");
}

#[tokio::test]
async fn falls_back_to_a_stable_default_when_error_payload_is_absent() {
    let body = sse(&[json!({"type": "error"})]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.tag(), "UnknownProvider");
    assert_eq!(error.message(), "Anthropic Messages stream error");
}

#[tokio::test]
async fn fails_http_provider_errors_before_stream_parsing() {
    let route = test_route();
    let transport = MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://api.anthropic.test/v1/messages".into(),
        status: 400,
        body:
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"Bad request"}}"#
                .into(),
        request_body: None,
    }]);
    let replay = route.with(RoutePatch {
        transport: Some(Arc::new(transport) as SharedTransport),
        ..Default::default()
    });
    let error = replay.generate(base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.tag(), "InvalidRequest");
    assert_eq!(error.message(), "Bad request");
}

#[tokio::test]
async fn decodes_server_tool_use_and_web_search_results_as_provider_executed() {
    let result =
        json!([{"type": "web_search_result", "url": "https://example.com", "title": "Example"}]);
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "server_tool_use", "id": "srvtoolu_abc", "name": "web_search"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"effect 4\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_abc", "content": result}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "content_block_start", "index": 2, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 2, "delta": {"type": "text_delta", "text": "Found it."}}),
        json!({"type": "content_block_stop", "index": 2}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 8}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    let call = response.events.iter().find(|event| event.kind() == "tool-call").map(wire).unwrap();
    jeq(
        &call,
        &json!({
            "type": "tool-call",
            "id": "srvtoolu_abc",
            "name": "web_search",
            "input": {"query": "effect 4"},
            "providerExecuted": true,
        }),
    );
    let tool_result =
        response.events.iter().find(|event| event.kind() == "tool-result").map(wire).unwrap();
    jeq(
        &tool_result,
        &json!({
            "type": "tool-result",
            "id": "srvtoolu_abc",
            "name": "web_search",
            "result": {"type": "json", "value": result},
            "providerExecuted": true,
            "providerMetadata": {
                "anthropic": {"blockType": "web_search_tool_result", "result": result}
            },
        }),
    );
    assert_eq!(response.text(), "Found it.");
    assert_eq!(
        serde_json::to_value(&response.finish_reason).unwrap(),
        json!({"normalized": "stop", "raw": "end_turn"})
    );
}

#[tokio::test]
async fn decodes_web_search_tool_result_error_as_error_result() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "server_tool_use", "id": "srvtoolu_x", "name": "web_search"}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"query\":\"q\"}"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_x", "content": {"type": "web_search_tool_result_error", "error_code": "max_uses_exceeded"}}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    let tool_result =
        response.events.iter().find(|event| event.kind() == "tool-result").map(wire).unwrap();
    assert_eq!(tool_result["result"]["type"], json!("error"));
    assert_eq!(tool_result["providerExecuted"], json!(true));
}

#[tokio::test]
async fn decodes_compaction_blocks_through_content_block_lifecycle() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "compaction", "content": null}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "compaction_delta", "content": "summarized context"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let response = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap();
    let compaction =
        response.events.iter().find(|event| event.kind() == "compaction").map(wire).unwrap();
    assert_eq!(
        compaction,
        json!({"type": "compaction", "provider": "anthropic", "text": "summarized context"})
    );
}

#[tokio::test]
async fn rejects_an_incomplete_compaction_block_at_message_stop() {
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "compaction", "content": null}}),
        json!({"type": "message_stop"}),
    ]);
    let error = generate_with(body, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.message(), "Response ended with an incomplete compaction block");
}

#[tokio::test]
async fn rejects_tool_use_blocks_without_index_or_id() {
    let missing_index = sse(&[
        json!({"type": "content_block_start", "content_block": {"type": "tool_use", "id": "call_1", "name": "x"}}),
    ]);
    let error = generate_with(missing_index, base_request(model_of("claude-sonnet-4-5")))
        .await
        .unwrap_err();
    assert_eq!(error.message(), "Anthropic tool_use missing index");

    let missing_id = sse(&[json!({
        "type": "content_block_start", "index": 3,
        "content_block": {"type": "server_tool_use", "name": "x"}
    })]);
    let error =
        generate_with(missing_id, base_request(model_of("claude-sonnet-4-5"))).await.unwrap_err();
    assert_eq!(error.message(), "Anthropic tool_use missing id at index 3");
}

#[tokio::test]
async fn round_trips_compatible_provider_metadata_in_its_own_namespace() {
    let compatible_route =
        anthropic_messages_route(Arc::new(FailingTransport { message: "custom route" })).with(
            RoutePatch {
                id: Some("custom-anthropic-messages".into()),
                provider: Some("custom-anthropic".into()),
                provider_metadata_key: Some("custom-anthropic".into()),
                endpoint: Some(EndpointPatch {
                    base_url: Some("https://compatible.test/v1".into()),
                    path: Some(crate::route::endpoint::EndpointPart::Static("/messages".into())),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    let model = compatible_route.model("custom-model", None).unwrap();
    let result = json!([{
        "type": "web_search_result",
        "url": "https://example.com",
        "citations": [{"type": "web_search_result_location", "cited_text": "Example"}],
    }]);
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5, "custom_start": true}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "Thinking."}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "custom_sig"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "redacted_thinking", "data": "custom_redacted"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "content_block_start", "index": 2, "content_block": {"type": "server_tool_use", "id": "custom_tool", "name": "web_search", "input": {"query": "example"}}}),
        json!({"type": "content_block_stop", "index": 2}),
        json!({"type": "content_block_start", "index": 3, "content_block": {"type": "web_search_tool_result", "tool_use_id": "custom_tool", "content": result}}),
        json!({"type": "content_block_stop", "index": 3}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": "custom_stop"}, "usage": {"output_tokens": 2, "custom_terminal": true}}),
        json!({"type": "message_stop"}),
    ]);
    let mut request = LLMRequest::new(model.clone());
    request.messages = vec![Message::user("Search.")];
    let route = request.model.route.clone();
    let transport = MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://compatible.test/v1/messages".into(),
        status: 200,
        body,
        request_body: None,
    }]);
    let replay = route.with(RoutePatch {
        transport: Some(Arc::new(transport) as SharedTransport),
        ..Default::default()
    });
    let response = replay.generate(request).await.unwrap();
    let content = content_wire(&response.message)["content"].clone();
    assert_eq!(
        content,
        json!([
            {"type": "reasoning", "text": "Thinking.", "providerMetadata": {"custom-anthropic": {"signature": "custom_sig"}}},
            {"type": "reasoning", "text": "", "providerMetadata": {"custom-anthropic": {"redactedData": "custom_redacted"}}},
            {"type": "tool-call", "id": "custom_tool", "name": "web_search", "input": {"query": "example"}, "providerExecuted": true},
            {"type": "tool-result", "id": "custom_tool", "name": "web_search", "result": {"type": "json", "value": result}, "providerExecuted": true, "providerMetadata": {"custom-anthropic": {"blockType": "web_search_tool_result", "result": result}}},
        ])
    );
    assert_eq!(
        response.usage.clone().unwrap().provider_metadata.unwrap(),
        provider_metadata(
            "custom-anthropic",
            json!({
                "input_tokens": 5, "custom_start": true, "output_tokens": 2, "custom_terminal": true
            })
        )
    );
    let last = wire(response.events.last().unwrap());
    assert_eq!(
        last["providerMetadata"],
        json!({"custom-anthropic": {"stopSequence": "custom_stop"}})
    );
    // ...and the whole assistant message round-trips back
    let request = LLMRequest::new(model).update(LLMRequestPatch {
        messages: Some(vec![response.message]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let lowered = compile(&request).unwrap();
    assert_eq!(
        lowered["messages"],
        json!([{"role": "assistant", "content": [
            {"type": "thinking", "thinking": "Thinking.", "signature": "custom_sig"},
            {"type": "redacted_thinking", "data": "custom_redacted"},
            {"type": "server_tool_use", "id": "custom_tool", "name": "web_search", "input": {"query": "example"}},
            {"type": "web_search_tool_result", "tool_use_id": "custom_tool", "content": result},
        ]}])
    );
}

#[tokio::test]
async fn preserves_a_reasoning_signature_when_message_stop_closes_the_block() {
    let compatible_route =
        anthropic_messages_route(Arc::new(FailingTransport { message: "custom route" })).with(
            RoutePatch {
                provider: Some("custom-anthropic".into()),
                provider_metadata_key: Some("custom-anthropic".into()),
                endpoint: Some(EndpointPatch {
                    base_url: Some("https://compatible.test/v1".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    let model = compatible_route.model("custom-model", None).unwrap();
    let body = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Reasoning."}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig_1"}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}}),
        json!({"type": "message_stop"}),
    ]);
    let mut request = LLMRequest::new(model.clone());
    request.messages = vec![Message::user("Think.")];
    let route = request.model.route.clone();
    let transport = MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://compatible.test/v1/messages".into(),
        status: 200,
        body,
        request_body: None,
    }]);
    let replay = route.with(RoutePatch {
        transport: Some(Arc::new(transport) as SharedTransport),
        ..Default::default()
    });
    let response = replay.generate(request).await.unwrap();
    let reasoning_ends: Vec<&LLMEvent> =
        response.events.iter().filter(|event| event.kind() == "reasoning-end").collect();
    assert_eq!(reasoning_ends.len(), 1);
    assert_eq!(
        wire(reasoning_ends[0])["providerMetadata"],
        json!({"custom-anthropic": {"signature": "sig_1"}})
    );
    let request = LLMRequest::new(model).update(LLMRequestPatch {
        messages: Some(vec![response.message]),
        cache: Some(CachePolicy::None),
        ..Default::default()
    });
    let lowered = compile(&request).unwrap();
    assert_eq!(
        lowered["messages"],
        json!([{"role": "assistant", "content": [{"type": "thinking", "thinking": "Reasoning.", "signature": "sig_1"}]}])
    );
}

// =============================================================================
// Per-provider Anthropic body wrappers (RW-01): alibaba/zai thinking
// re-attach without a mandatory budgetTokens, meta native.meta → web_search.
// Mirrors protocols/{alibaba,zai,meta}-messages.ts (ref).
// =============================================================================

fn provider_model(provider: &str, id: &str) -> LanguageModel {
    anthropic_messages_route(Arc::new(FailingTransport { message: "unit test route" })).with(
        RoutePatch {
            provider: Some(provider.into()),
            endpoint: Some(EndpointPatch {
                base_url: Some("https://provider.test/v1/".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .model(id, None)
    .expect("model builds")
}

#[test]
fn alibaba_messages_accepts_enabled_thinking_without_a_budget() {
    let model = provider_model("alibaba", "qwen3.5-flash");
    let mut request = base_request(model);
    request.provider_options = Some(serde_json::from_value(json!({
        "thinking": {"type": "enabled"}
    })).unwrap());
    let body = compile(&request).unwrap();
    assert_eq!(body["thinking"], json!({"type": "enabled"}));
}

#[test]
fn alibaba_messages_fits_an_explicit_budget_against_max_tokens() {
    let model = provider_model("alibaba", "qwen3.5-flash");
    let mut request = base_request(model);
    request.generation = Some(GenerationOptions {
        max_tokens: Some(30_000.0),
        ..Default::default()
    });
    request.provider_options = Some(serde_json::from_value(json!({"thinking": {"type": "enabled", "budgetTokens": 20_000}})).unwrap());
    let body = compile(&request).unwrap();
    assert_eq!(body["thinking"], json!({"type": "enabled", "budget_tokens": 15_000}));

    // the wire spelling `budget_tokens` is accepted too
    request.provider_options = Some(serde_json::from_value(json!({"thinking": {"type": "enabled", "budget_tokens": 10_000}})).unwrap());
    let body = compile(&request).unwrap();
    assert_eq!(body["thinking"], json!({"type": "enabled", "budget_tokens": 10_000}));
}

#[test]
fn alibaba_messages_without_thinking_omits_the_key() {
    let model = provider_model("alibaba", "qwen3.5-flash");
    let request = base_request(model);
    let body = compile(&request).unwrap();
    assert!(body.get("thinking").is_none());
}

#[test]
fn zai_messages_accepts_enabled_thinking_without_a_budget() {
    let model = provider_model("zai-coding-plan", "glm-5.2");
    let mut request = base_request(model);
    request.provider_options = Some(serde_json::from_value(json!({"thinking": {"type": "enabled"}})).unwrap());
    let body = compile(&request).unwrap();
    assert_eq!(body["thinking"], json!({"type": "enabled"}));

    // absent thinking → no key (mirrors `thinking: options.thinking`)
    let model = provider_model("zai-coding-plan", "glm-5.2");
    let request = base_request(model);
    let body = compile(&request).unwrap();
    assert!(body.get("thinking").is_none());
}

#[test]
fn unregistered_providers_keep_the_generic_budget_requirement() {
    // ref moonshot/minimax messages routes run the plain Anthropic Messages
    // protocol (moonshot.ts messagesRoute / minimax.ts messagesRoute): the
    // thinking-budgetTokens exemption does NOT apply there.
    for provider in ["moonshotai", "minimax", "anthropic"] {
        let model = provider_model(provider, "kimi-k2.7");
        let mut request = base_request(model);
        request.provider_options =
            Some(serde_json::from_value(json!({"thinking": {"type": "enabled"}})).unwrap());
        let error = compile(&request).unwrap_err();
        assert!(
            error.to_string().contains("requires a budgetTokens"),
            "{provider}: unexpected error {error}"
        );
    }
}

#[test]
fn meta_messages_remaps_native_meta_to_web_search() {
    let model = provider_model("meta", "llama-4-maverick");
    let mut request = base_request(model);
    let mut search = ToolDefinition::new("web_search", "Search the web", json!({"type": "object"}));
    search.native = Some(serde_json::from_value(json!({
            "meta": {
                "type": "web_search",
                "user_location": {"type": "approximate", "city": "Shanghai", "country": "CN"}
            }
        })
        ).unwrap());
    let plain = ToolDefinition::new("lookup", "Look things up", json!({"type": "object"}));
    request = request.with_tool(search).with_tool(plain);
    let body = compile(&request).unwrap();
    // `flatten_tools` sorts by name: "lookup" < "web_search"
    assert_eq!(body["tools"][0]["name"], json!("lookup"));
    assert_eq!(
        body["tools"][1],
        json!({
            "type": "web_search",
            "name": "web_search",
            "user_location": {"type": "approximate", "city": "Shanghai", "country": "CN"}
        })
    );
    // tools without natives lower through the generic path untouched
    assert_eq!(body["tools"][0]["description"], json!("Look things up"));
}

#[test]
fn meta_messages_rejects_search_context_size() {
    let model = provider_model("meta", "llama-4-maverick");
    let mut request = base_request(model);
    let mut search = ToolDefinition::new("web_search", "Search the web", json!({"type": "object"}));
    search.native = Some(
        serde_json::from_value(json!({"meta": {"type": "web_search", "search_context_size": "medium"}}))
            .unwrap(),
    );
    request = request.with_tool(search);
    let error = compile(&request).unwrap_err();
    assert!(error.to_string().contains("does not support searchContextSize"), "{error}");
}

#[test]
fn meta_messages_without_tools_or_natives_is_a_no_op() {
    let model = provider_model("meta", "llama-4-maverick");
    let request = base_request(model);
    let body = compile(&request).unwrap();
    assert!(body.get("tools").is_none());
}
