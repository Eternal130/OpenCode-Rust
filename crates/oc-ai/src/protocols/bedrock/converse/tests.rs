//! Bedrock Converse protocol tests: request-shape goldens (synthetic),
//! streaming decode fixtures (encoded AWS event-stream frames through the
//! real framing decoder), typed error mappings, and the route wire shape.

use serde_json::json;

use super::{body_from, BedrockConverseProtocol};
use crate::protocols::bedrock::{
    event_stream::encode, resolve_auth, BedrockAuthOptions, Credentials,
};
use crate::route::client::{Route, RouteInput, RoutePatch};
use crate::route::framing::{Framing, FramingDecoder};
use crate::route::protocol::Protocol;
use crate::schema::options::CacheHintKind;
use crate::schema::{
    Asset, CacheHint, ContentPart, FinishReason, LLMRequest, Message, MessageRole, ToolDefinition,
    ToolResultValue,
};
use crate::test_support::MockTransport;
use std::sync::Arc;

fn request_with(route: &Route, messages: Vec<Message>) -> LLMRequest {
    let mut request =
        LLMRequest::new(route.model("anthropic.claude-sonnet-4-20250514-v1:0", None).unwrap());
    request.messages = messages;
    request
}

fn route() -> Route {
    let transport: crate::route::SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![]));
    super::bedrock_converse_route(Arc::clone(&transport), BedrockAuthOptions::default())
}

fn media_part(mime: &str, base64: &str) -> ContentPart {
    ContentPart::Media {
        media: Asset {
            source: crate::schema::media::Source::Base64 {
                data: base64.to_string(),
                media_type: mime.to_string(),
            },
            media_type: mime.to_string(),
            info: None,
            expires_at: None,
            provider_metadata: None,
        },
        filename: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    }
}

// ===========================================================================
// Request-shape goldens
// ===========================================================================

#[test]
fn golden_user_assistant_tool_round_trip() {
    let route = route();
    let mut request = request_with(
        &route,
        vec![
            Message::user("hello"),
            Message::new(
                MessageRole::Assistant,
                vec![
                    ContentPart::text("working"),
                    ContentPart::ToolCall {
                        id: "tool-123".into(),
                        name: "lookup".into(),
                        namespace: None,
                        input: json!({ "query": "rust" }),
                        provider_executed: None,
                        cache: None,
                        metadata: None,
                        provider_metadata: None,
                    },
                ],
            ),
            Message::new(
                MessageRole::Tool,
                vec![ContentPart::ToolResult {
                    id: "tool-123".into(),
                    name: "lookup".into(),
                    namespace: None,
                    result: ToolResultValue::Text { value: json!("found it") },
                    provider_executed: None,
                    cache: None,
                    metadata: None,
                    provider_metadata: None,
                }],
            ),
        ],
    );
    request.system = vec![crate::schema::SystemPart::new("be brief")];
    request.generation = Some(crate::schema::GenerationOptions {
        max_tokens: Some(128.0),
        temperature: Some(0.5),
        stop: Some(vec!["END".into()]),
        ..Default::default()
    });
    request.tools = vec![crate::schema::ToolEntry::Definition(ToolDefinition {
        name: "lookup".into(),
        description: "Look things up".into(),
        input_schema: json!({ "type": "object", "properties": {} }),
        output_schema: None,
        cache: None,
        metadata: None,
        native: None,
    })];
    let body = body_from(&request).expect("body");
    assert_eq!(body["modelId"], json!("anthropic.claude-sonnet-4-20250514-v1:0"));
    assert_eq!(body["system"], json!([{ "text": "be brief" }]));
    assert_eq!(
        body["inferenceConfig"],
        json!({ "maxTokens": 128.0, "temperature": 0.5, "stopSequences": ["END"] })
    );
    assert_eq!(
        body["toolConfig"]["tools"][0],
        json!({ "toolSpec": {
            "name": "lookup",
            "description": "Look things up",
            "inputSchema": { "json": { "type": "object", "properties": {} } }
        }})
    );
    assert_eq!(
        body["messages"],
        json!([
            { "role": "user", "content": [{ "text": "hello" }] },
            { "role": "assistant", "content": [
                { "text": "working" },
                { "toolUse": { "toolUseId": "tool-123", "name": "lookup", "input": { "query": "rust" } } }
            ]},
            { "role": "user", "content": [
                { "toolResult": { "toolUseId": "tool-123", "content": [{ "text": "found it" }], "status": "success" } }
            ]}
        ])
    );
}

#[test]
fn thinking_budget_flows_through_additional_fields() {
    let route = route();
    let mut request = request_with(&route, vec![Message::user("hi")]);
    request.generation =
        Some(crate::schema::GenerationOptions { max_tokens: Some(4096.0), ..Default::default() });
    let mut options = serde_json::Map::new();
    options.insert("thinking".to_string(), json!({ "type": "enabled", "budgetTokens": 2048 }));
    request.provider_options = Some(options);
    let body = body_from(&request).unwrap();
    assert_eq!(
        body["additionalModelRequestFields"]["thinking"],
        json!({ "type": "enabled", "budget_tokens": 2048.0 })
    );
}

#[test]
fn image_media_lowers_to_bytes_and_cache_points_emit_positionally() {
    let route = route();
    let request = request_with(
        &route,
        vec![Message::new(
            MessageRole::User,
            vec![
                ContentPart::Text {
                    text: "look".into(),
                    cache: Some(CacheHint { kind: CacheHintKind::Ephemeral, ttl_seconds: None }),
                    metadata: None,
                    provider_metadata: None,
                },
                media_part("image/png", "aGk="),
            ],
        )],
    );
    let body = body_from(&request).unwrap();
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0], json!({ "text": "look" }));
    assert_eq!(content[1], json!({ "cachePoint": { "type": "default" } }));
    assert_eq!(content[2], json!({ "image": { "format": "png", "source": { "bytes": "aGk=" } } }));
}

#[test]
fn url_or_ref_media_fails_cleanly() {
    let route = route();
    let part = ContentPart::Media {
        media: Asset {
            source: crate::schema::media::Source::Url {
                url: "https://example.test/x.png".into(),
                media_type: Some("image/png".into()),
                expires_at: None,
            },
            media_type: "image/png".into(),
            info: None,
            expires_at: None,
            provider_metadata: None,
        },
        filename: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    };
    let request = request_with(&route, vec![Message::new(MessageRole::User, vec![part])]);
    let error = body_from(&request).unwrap_err();
    assert!(error.message().contains("inline image and document media"), "{}", error.message());
}

#[test]
fn mistral_tool_ids_project_to_valid_nine_char_ids() {
    let transport: crate::route::SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![]));
    let route =
        super::bedrock_converse_route(Arc::clone(&transport), BedrockAuthOptions::default());
    let model = route.model("mistral.mistral-large-2407-v1:0", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::new(
        MessageRole::Assistant,
        vec![ContentPart::ToolCall {
            id: "call_ABC-123_xyz".into(),
            name: "t".into(),
            namespace: None,
            input: json!({}),
            provider_executed: None,
            cache: None,
            metadata: None,
            provider_metadata: None,
        }],
    )];
    let body = body_from(&request).unwrap();
    let tool_use_id = body["messages"][0]["content"][0]["toolUse"]["toolUseId"].as_str().unwrap();
    assert_eq!(tool_use_id.len(), 9, "{tool_use_id}");
    assert!(tool_use_id.bytes().all(|byte| byte.is_ascii_alphanumeric()));
}

// ===========================================================================
// Streaming decode fixtures through the real framing decoder
// ===========================================================================

fn decode_events(frames: &[Vec<u8>]) -> Vec<crate::schema::LLMEvent> {
    let mut decoder = FramingDecoder::new(Framing::aws_event_stream());
    let protocol = BedrockConverseProtocol;
    let route = route();
    let request = request_with(&route, vec![Message::user("hi")]);
    let mut state = protocol.initial(&request);
    let mut events = Vec::new();
    for frame in frames {
        for json_frame in decoder.feed(frame).unwrap() {
            let (next, mut frame_events) = protocol.step(state, &json_frame).unwrap();
            state = next;
            events.append(&mut frame_events);
        }
    }
    // drain: no more frames; on_halt synthesizes the finish
    for json_frame in decoder.finish().unwrap() {
        let (next, mut frame_events) = protocol.step(state, &json_frame).unwrap();
        state = next;
        events.append(&mut frame_events);
    }
    let (_, mut halt_events) = protocol.on_halt(state).unwrap();
    events.append(&mut halt_events);
    events
}

#[test]
fn streams_text_reasoning_tools_and_usage_to_a_single_finish() {
    let frames = vec![
        encode::event("messageStart", r#"{"role":"assistant"}"#),
        encode::event(
            "contentBlockDelta",
            r#"{"contentBlockIndex":0,"delta":{"reasoningContent":{"text":"thinking..."}}}"#,
        ),
        encode::event("contentBlockDelta", r#"{"contentBlockIndex":1,"delta":{"text":"Hello"}}"#),
        encode::event(
            "contentBlockStart",
            r#"{"contentBlockIndex":2,"start":{"toolUse":{"toolUseId":"tu-1","name":"lookup"}}}"#,
        ),
        encode::event(
            "contentBlockDelta",
            r#"{"contentBlockIndex":2,"delta":{"toolUse":{"input":"{\"q\":"}}}"#,
        ),
        encode::event(
            "contentBlockDelta",
            r#"{"contentBlockIndex":2,"delta":{"toolUse":{"input":"\"rust\"}"}}}"#,
        ),
        encode::event("contentBlockStop", r#"{"contentBlockIndex":2}"#),
        encode::event("messageStop", r#"{"stopReason":"tool_use"}"#),
        encode::event(
            "metadata",
            r#"{"usage":{"inputTokens":10,"outputTokens":5,"cacheReadInputTokens":2,"totalTokens":17}}"#,
        ),
    ];
    let events = decode_events(&frames);
    let kinds: Vec<&str> = events.iter().map(crate::schema::events::LLMEvent::kind).collect();
    assert!(kinds.contains(&"reasoning-delta"), "{kinds:?}");
    assert!(kinds.contains(&"text-delta"), "{kinds:?}");
    assert!(kinds.contains(&"tool-input-start"), "{kinds:?}");
    assert!(kinds.contains(&"tool-call"), "{kinds:?}");
    assert_eq!(kinds.iter().filter(|kind| **kind == "finish").count(), 1);
    let finish = events
        .iter()
        .find_map(|event| match event {
            crate::schema::LLMEvent::Finish { reason, usage, .. } => {
                Some((reason.clone(), usage.clone()))
            }
            _ => None,
        })
        .expect("finish event");
    assert_eq!(finish.0.normalized, FinishReason::ToolCalls);
    let usage = finish.1.expect("usage");
    // inclusive input: 10 + 2 cache reads
    assert_eq!(usage.input_tokens, Some(12.0));
    assert_eq!(usage.non_cached_input_tokens, Some(10.0));
    assert_eq!(usage.cache_read_input_tokens, Some(2.0));
    assert_eq!(usage.total_tokens, Some(17.0));
    let tool_call = events
        .iter()
        .find_map(|event| match event {
            crate::schema::LLMEvent::ToolCall { input, .. } => Some(input.clone()),
            _ => None,
        })
        .expect("tool call assembled");
    assert_eq!(tool_call, json!({ "q": "rust" }));
}

#[test]
fn mid_frame_chunk_splits_preserve_the_stream() {
    let frames = vec![
        encode::event("messageStart", r#"{"role":"assistant"}"#),
        encode::event("contentBlockDelta", r#"{"contentBlockIndex":0,"delta":{"text":"Hel"}}"#),
        encode::event("contentBlockDelta", r#"{"contentBlockIndex":0,"delta":{"text":"lo"}}"#),
        encode::event("messageStop", r#"{"stopReason":"end_turn"}"#),
    ];
    let whole = decode_events(&frames);
    let text: String = whole
        .iter()
        .filter_map(|event| match event {
            crate::schema::LLMEvent::TextDelta { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello");
    // split the byte stream at several offsets incl. inside frames: the same
    // text events must come out
    let mut concatenated = Vec::new();
    for frame in &frames {
        concatenated.extend_from_slice(frame);
    }
    for split in [1usize, 4, 9, 21] {
        let (a, b) = concatenated.split_at(split.min(concatenated.len()));
        let mut decoder = FramingDecoder::new(Framing::aws_event_stream());
        let mut json_frames = Vec::new();
        for chunk in [a, b] {
            json_frames.extend(decoder.feed(chunk).unwrap());
        }
        json_frames.extend(decoder.finish().unwrap());
        let protocol = BedrockConverseProtocol;
        let route = route();
        let request = request_with(&route, vec![Message::user("hi")]);
        let mut state = protocol.initial(&request);
        let mut text = String::new();
        for frame in json_frames {
            let (next, events) = protocol.step(state, &frame).unwrap();
            state = next;
            for event in events {
                if let crate::schema::LLMEvent::TextDelta { text: delta, .. } = event {
                    text.push_str(&delta);
                }
            }
        }
        assert_eq!(text, "Hello", "split at {split} lost text");
    }
}

#[test]
fn malformed_stop_reasons_are_typed_errors() {
    let route = route();
    let protocol = BedrockConverseProtocol;
    let request = request_with(&route, vec![Message::user("hi")]);
    let state = protocol.initial(&request);
    let error =
        protocol.step(state, r#"{"messageStop":{"stopReason":"malformed_tool_use"}}"#).unwrap_err();
    assert!(error.message().contains("malformed_tool_use"), "{}", error.message());
}

#[test]
fn stream_exceptions_surface_as_provider_errors() {
    let route = route();
    let protocol = BedrockConverseProtocol;
    let request = request_with(&route, vec![Message::user("hi")]);
    let state = protocol.initial(&request);
    let error = protocol
        .step(
            state,
            r#"{"exception":{"type":"ValidationException","details":{"message":"input is too long"}}}"#,
        )
        .unwrap_err();
    assert!(error.message().contains("input is too long"), "{}", error.message());
}

#[test]
fn incomplete_frames_at_stream_end_fail_as_incomplete_stream() {
    let mut decoder = FramingDecoder::new(Framing::aws_event_stream());
    let frame = encode::event("messageStart", r#"{"role":"assistant"}"#);
    decoder.feed(&frame[..frame.len() - 3]).unwrap();
    let error = decoder.finish().unwrap_err();
    assert!(error.message().contains("buffered bytes remain"), "{}", error.message());
}

// ===========================================================================
// Route wire shapes + auth resolution
// ===========================================================================

#[test]
fn converse_route_signs_with_sigv4_over_env_credentials() {
    std::env::set_var("OC_AI_BEDROCK_TEST_AK", "AKIDEXAMPLE");
    let transport: crate::route::SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![]));
    let route = Route::new(RouteInput {
        id: "bedrock-converse".into(),
        provider: Some("amazon-bedrock".into()),
        provider_metadata_key: None,
        protocol: Arc::new(BedrockConverseProtocol),
        endpoint: crate::route::endpoint::Endpoint::dynamic_path(|input| {
            format!("/model/{}/converse-stream", input.request.model.id.as_str())
        })
        .with_base_url("https://bedrock-runtime.us-east-1.amazonaws.com"),
        auth: Some(resolve_auth(
            &BedrockAuthOptions {
                credentials: Some(Credentials {
                    region: "us-east-1".into(),
                    access_key_id: "AKIDEXAMPLE".into(),
                    secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
                    session_token: None,
                }),
                ..Default::default()
            },
            "bedrock",
            "Bedrock Converse",
        )),
        framing: Framing::aws_event_stream(),
        headers: None,
        transport: Arc::clone(&transport),
        defaults: Default::default(),
    });
    let request = request_with(&route, vec![Message::user("hi")]);
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert_eq!(
        transport_request.url,
        "https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.claude-sonnet-4-20250514-v1:0/converse-stream"
    );
    let authorization = transport_request.header("authorization").expect("signed");
    assert!(
        authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"),
        "{authorization}"
    );
    assert!(authorization.contains("/us-east-1/bedrock/aws4_request"));
    let () = std::env::remove_var("OC_AI_BEDROCK_TEST_AK");
}

#[test]
fn bearer_api_key_wins_over_sigv4() {
    let definition = resolve_auth(
        &BedrockAuthOptions { api_key: Some("bedrock-bearer".into()), ..Default::default() },
        "bedrock",
        "Bedrock Converse",
    );
    let headers = crate::route::auth::Headers::new();
    let input = crate::route::auth::AuthInput {
        request_http: None,
        method: "POST",
        url: "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/converse-stream",
        body: "{}",
        headers,
    };
    let applied = definition.apply(&input).unwrap();
    assert_eq!(applied.get("authorization"), Some("Bearer bedrock-bearer"));
}

#[test]
fn invoke_route_points_at_invoke_stream_with_anthropic_protocol() {
    let transport: crate::route::SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![]));
    let route = super::bedrock_invoke_route(
        Arc::clone(&transport),
        BedrockAuthOptions {
            region: Some("eu-central-1".into()),
            credentials: Some(Credentials {
                region: "eu-central-1".into(),
                access_key_id: "AKIDEXAMPLE".into(),
                secret_access_key: "secret".into(),
                session_token: None,
            }),
            ..Default::default()
        },
    );
    assert_eq!(route.id, "bedrock-invoke");
    assert_eq!(route.protocol.id(), "anthropic-messages");
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some("https://bedrock-runtime.eu-central-1.amazonaws.com")
    );
    let model = route.model("anthropic.claude-sonnet-4-20250514-v1:0", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    assert_eq!(body["model"], json!("anthropic.claude-sonnet-4-20250514-v1:0"));
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert!(transport_request.url.ends_with("/invoke-stream"), "{}", transport_request.url);
    assert!(transport_request.url.contains("/model/"), "{}", transport_request.url);
}

#[test]
fn missing_credentials_error_names_the_requirement() {
    let definition = resolve_auth(&BedrockAuthOptions::default(), "bedrock", "Bedrock Converse");
    let input = crate::route::auth::AuthInput {
        request_http: None,
        method: "POST",
        url: "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/converse-stream",
        body: "{}",
        headers: crate::route::auth::Headers::new(),
    };
    let error = definition.apply(&input).unwrap_err();
    assert!(
        error.message().contains("requires either route bearer auth or AWS credentials"),
        "{}",
        error.message()
    );
}

#[test]
fn cache_point_cap_drops_fifth_breakpoint() {
    let route = route();
    let cache = || Some(CacheHint { kind: CacheHintKind::Ephemeral, ttl_seconds: None });
    let mut request = request_with(
        &route,
        vec![Message::new(
            MessageRole::User,
            (0..6)
                .map(|index| ContentPart::Text {
                    text: format!("part {index}"),
                    cache: cache(),
                    metadata: None,
                    provider_metadata: None,
                })
                .collect(),
        )],
    );
    request.system =
        vec![crate::schema::SystemPart { text: "system".into(), cache: cache(), metadata: None }];
    let body = body_from(&request).unwrap();
    let count = body.to_string().matches("cachePoint").count();
    assert_eq!(count, 4, "breakpoint cap enforced: {body}");
    // non-claude models never emit cache points
    let transport: crate::route::SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![]));
    let other =
        super::bedrock_converse_route(Arc::clone(&transport), BedrockAuthOptions::default());
    let model = other.model("meta.llama3-3-70b-instruct-v1:0", None).unwrap();
    let mut llama = LLMRequest::new(model);
    llama.system = request.system.clone();
    llama.messages = request.messages.clone();
    let body = body_from(&llama).unwrap();
    assert!(!body.to_string().contains("cachePoint"));
}

#[test]
fn region_defaults_and_headers_on_the_converse_route() {
    let transport: crate::route::SharedTransport =
        Arc::new(MockTransport::from_interactions(vec![]));
    let route =
        super::bedrock_converse_route(Arc::clone(&transport), BedrockAuthOptions::default());
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some("https://bedrock-runtime.us-east-1.amazonaws.com"),
        "region default us-east-1"
    );
    let patched = route.with(RoutePatch {
        auth: Some(crate::route::auth::Definition::bearer(crate::route::auth::Secret::Value(
            "k".into(),
        ))),
        ..Default::default()
    });
    assert_eq!(patched.endpoint.base_url.as_deref(), route.endpoint.base_url.as_deref());
}
