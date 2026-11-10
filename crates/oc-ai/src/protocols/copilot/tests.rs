//! Copilot chat tests: header policy (client + per-request), interaction
//! vocabulary, auth chain, and the wire shape over the openai-compatible
//! chat protocol.

use serde_json::json;

use super::{
    copilot_chat_route, copilot_client_headers, copilot_interaction_headers,
    copilot_request_headers, interaction_context, interaction_type, request_metadata,
    CopilotConfig, SessionRequestKind, COPILOT_BASE_URL,
};
use crate::route::SharedTransport;
use crate::schema::options::HttpOptions;
use crate::schema::{ContentPart, LLMRequest, Message, MessageRole, ToolResultValue};
use crate::test_support::MockTransport;
use std::collections::BTreeMap;
use std::sync::Arc;

fn transport() -> SharedTransport {
    Arc::new(MockTransport::from_interactions(vec![]))
}

fn media_part() -> ContentPart {
    ContentPart::Media {
        media: Box::new(crate::schema::Asset {
            source: crate::schema::media::Source::Base64 {
                data: "aGk=".into(),
                media_type: "image/png".into(),
            },
            media_type: "image/png".into(),
            info: None,
            expires_at: None,
            provider_metadata: None,
            headers: None,
        }),
        filename: None,
        cache: None,
        metadata: None,
        provider_metadata: None,
    }
}

#[test]
fn interaction_type_vocabulary_matches_the_copilot_client() {
    assert_eq!(interaction_type(SessionRequestKind::Chat, false), "conversation-agent");
    assert_eq!(interaction_type(SessionRequestKind::Chat, true), "conversation-subagent");
    assert_eq!(interaction_type(SessionRequestKind::Title, false), "conversation-background");
    assert_eq!(interaction_type(SessionRequestKind::Compaction, false), "conversation-compaction");
}

#[test]
fn plain_user_turns_stay_user_initiated() {
    let route = copilot_chat_route(
        transport(),
        CopilotConfig {
            token: Some(crate::route::auth::Secret::Value("gh-token".into())),
            ..Default::default()
        },
    );
    let model = route.model("gpt-5.4", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    assert_eq!(request_metadata(&request), super::RequestMetadata { agent: false, vision: false });
    assert!(copilot_request_headers(&request).is_empty());
}

#[test]
fn tool_result_turns_escalate_to_agent_and_images_flag_vision() {
    let route = copilot_chat_route(
        transport(),
        CopilotConfig {
            token: Some(crate::route::auth::Secret::Value("t".into())),
            ..Default::default()
        },
    );
    let model = route.model("gpt-5.4", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![
        Message::new(
            MessageRole::Assistant,
            vec![ContentPart::ToolCall {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                input: json!({}),
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
        ),
        Message::new(
            MessageRole::Tool,
            vec![ContentPart::ToolResult {
                id: "c1".into(),
                name: "lookup".into(),
                namespace: None,
                result: ToolResultValue::Text { value: json!("found") },
                provider_executed: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }],
        ),
        Message::new(MessageRole::User, vec![media_part()]),
    ];
    // last turn IS a user turn (with media, not only tool results) → user
    // initiator; the image anywhere in the request flags vision
    assert_eq!(request_metadata(&request), super::RequestMetadata { agent: false, vision: true });
    let headers = copilot_request_headers(&request);
    assert_eq!(headers.get("Copilot-Vision-Request"), Some(&"true".to_string()));
    assert!(!headers.contains_key("x-initiator"));

    request.messages.pop();
    // last turn is a tool result → agent initiator
    assert!(request_metadata(&request).agent);
    assert_eq!(copilot_request_headers(&request).get("x-initiator"), Some(&"agent".to_string()));
}

#[test]
fn copilot_wire_request_carries_the_client_headers_and_bearer() {
    let route = copilot_chat_route(
        transport(),
        CopilotConfig {
            base_url: Some("https://copilot-api.company.ghe.com".into()),
            token: Some(crate::route::auth::Secret::Value("ghu_bound".into())),
        },
    );
    assert_eq!(route.id, "copilot-chat");
    assert_eq!(route.provider_metadata_key.as_deref(), Some("copilot"));
    let model = route.model("claude-sonnet-4.6", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hello")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("chat body");
    assert_eq!(body["model"], json!("claude-sonnet-4.6"));
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(transport_request.url, "https://copilot-api.company.ghe.com/chat/completions");
    assert_eq!(transport_request.header("authorization"), Some("Bearer ghu_bound"));
    assert_eq!(transport_request.header("X-GitHub-Api-Version"), Some("2026-08-01"));
    assert_eq!(transport_request.header("Openai-Intent"), Some("conversation-edits"));
    assert_eq!(transport_request.header("x-initiator"), Some("user"));
}

#[test]
fn default_base_url_and_github_token_env_fallback() {
    let route = copilot_chat_route(transport(), CopilotConfig::default());
    let base = route.endpoint.base_url.as_deref().unwrap_or_default().to_string();
    assert_eq!(base, COPILOT_BASE_URL);
    std::env::set_var("GITHUB_TOKEN", "env-token");
    let input = crate::route::auth::AuthInput {
        request_http: None,
        method: "POST",
        url: "https://api.githubcopilot.com/chat/completions",
        body: "{}",
        headers: crate::route::auth::Headers::new(),
    };
    let headers = route.auth.apply(&input).expect("env token");
    assert_eq!(headers.get("authorization"), Some("Bearer env-token"));
    std::env::remove_var("GITHUB_TOKEN");
    let client = copilot_client_headers();
    assert_eq!(client["X-GitHub-Api-Version"], "2026-08-01");
}

// ===========================================================================
// G-298 — the `model.request` interaction vocabulary on all three lanes
// (github-copilot.ts:267-280)
// ===========================================================================
//
// FINAL-G52 — the session runner's request-kind seam header
// (`x-opencode-request-kind`) is authoritative: title requests classify as
// `conversation-background` exactly like the ref's `evt.kind` read.

#[test]
fn interaction_kind_seam_header_overrides_the_body_derivation() {
    // title kind via the seam → conversation-background + agent initiator
    let request = session_request(
        &[("x-session-affinity", "ses_affinity_1"), ("x-opencode-request-kind", "title")],
        vec![Message::user("Summarize: hello world")],
    );
    assert_eq!(interaction_context(&request).kind, SessionRequestKind::Title);
    let headers = copilot_interaction_headers(&request);
    assert_eq!(headers.get("X-Interaction-Type"), Some(&"conversation-background".to_string()));
    assert_eq!(headers.get("x-initiator"), Some(&"agent".to_string()));

    // compaction kind via the seam (no compaction part in the body)
    let request = session_request(
        &[("x-session-affinity", "ses_affinity_1"), ("x-opencode-request-kind", "compaction")],
        vec![Message::user("history")],
    );
    assert_eq!(interaction_context(&request).kind, SessionRequestKind::Compaction);
    let headers = copilot_interaction_headers(&request);
    assert_eq!(headers.get("X-Interaction-Type"), Some(&"conversation-compaction".to_string()));

    // primary/generate map back to the agent-loop default
    let request = session_request(
        &[("x-session-affinity", "ses_affinity_1"), ("x-opencode-request-kind", "generate")],
        vec![Message::user("hi")],
    );
    assert_eq!(interaction_context(&request).kind, SessionRequestKind::Chat);
}

fn session_request(route_headers: &[(&str, &str)], messages: Vec<Message>) -> LLMRequest {
    let route = copilot_chat_route(
        transport(),
        CopilotConfig {
            token: Some(crate::route::auth::Secret::Value("t".into())),
            ..Default::default()
        },
    );
    let model = route.model("claude-sonnet-4.6", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = messages;
    if !route_headers.is_empty() {
        request.http = Some(HttpOptions {
            headers: Some(
                route_headers
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect::<BTreeMap<String, String>>(),
            ),
            ..Default::default()
        });
    }
    request
}

#[test]
fn interaction_headers_matrix_matches_the_ref_hook() {
    // Primary agent loop (not a child): conversation-agent, no forced
    // initiator (the client default `user` stays).
    let request = session_request(
        &[("x-session-affinity", "ses_affinity_1"), ("x-opencode-session-id", "ses_abc")],
        vec![Message::user("hi")],
    );
    assert_eq!(
        interaction_context(&request),
        super::InteractionContext {
            kind: SessionRequestKind::Chat,
            child: false,
            interaction_id: Some("ses_affinity_1".to_string()),
        }
    );
    let headers = copilot_interaction_headers(&request);
    assert_eq!(headers.get("X-Interaction-Type"), Some(&"conversation-agent".to_string()));
    assert_eq!(headers.get("X-Interaction-Id"), Some(&"ses_affinity_1".to_string()));
    assert!(!headers.contains_key("x-initiator"));

    // Child session (parent header present): conversation-subagent and the
    // forced agent initiator.
    let request = session_request(
        &[("x-session-affinity", "ses_affinity_1"), ("x-opencode-parent-session-id", "ses_parent")],
        vec![Message::user("hi")],
    );
    let headers = copilot_interaction_headers(&request);
    assert_eq!(headers.get("X-Interaction-Type"), Some(&"conversation-subagent".to_string()));
    assert_eq!(headers.get("X-Interaction-Id"), Some(&"ses_affinity_1".to_string()));
    assert_eq!(headers.get("x-initiator"), Some(&"agent".to_string()));

    // Compaction window replay: conversation-compaction + agent.
    let request = session_request(
        &[("x-opencode-session-id", "ses_abc")],
        vec![Message::new(
            MessageRole::User,
            vec![ContentPart::Compaction {
                provider: crate::schema::ProviderId::new("anthropic"),
                id: None,
                encrypted: Some("window".into()),
                text: None,
            }],
        )],
    );
    assert_eq!(interaction_context(&request).kind, SessionRequestKind::Compaction);
    let headers = copilot_interaction_headers(&request);
    assert_eq!(headers.get("X-Interaction-Type"), Some(&"conversation-compaction".to_string()));
    assert_eq!(headers.get("X-Interaction-Id"), Some(&"ses_abc".to_string()));
    assert_eq!(headers.get("x-initiator"), Some(&"agent".to_string()));
}

#[test]
fn interaction_id_falls_back_to_the_session_id_without_affinity() {
    // `SessionAffinity.get(session) ?? sessionID` — with only the session
    // id header the id header still rides the request.
    let request =
        session_request(&[("x-opencode-session-id", "ses_abc")], vec![Message::user("hi")]);
    let headers = copilot_interaction_headers(&request);
    assert_eq!(headers.get("X-Interaction-Id"), Some(&"ses_abc".to_string()));
    assert_eq!(headers.get("X-Interaction-Type"), Some(&"conversation-agent".to_string()));
}

#[test]
fn interaction_headers_absent_without_session_context() {
    // No seam headers (direct route use): like the ref hook not running,
    // no interaction headers; the pre-existing per-request policy is
    // unchanged.
    let request = session_request(&[], vec![Message::user("hi")]);
    assert!(copilot_interaction_headers(&request).is_empty());
    assert!(copilot_request_headers(&request).is_empty());
}

#[test]
fn chat_lane_wire_carries_the_interaction_headers() {
    let request =
        session_request(&[("x-session-affinity", "ses_affinity_1")], vec![Message::user("hello")]);
    let route = request.model.route.clone();
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("chat body");
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(transport_request.url, "https://api.githubcopilot.com/chat/completions");
    assert_eq!(transport_request.header("X-Interaction-Type"), Some("conversation-agent"));
    assert_eq!(transport_request.header("X-Interaction-Id"), Some("ses_affinity_1"));
    assert_eq!(transport_request.header("x-initiator"), Some("user"));
}

#[test]
fn responses_lane_wire_carries_the_interaction_headers() {
    // gpt-5.x models serve on /responses (the dispatch lane).
    let route = copilot_chat_route(
        transport(),
        CopilotConfig {
            token: Some(crate::route::auth::Secret::Value("t".into())),
            ..Default::default()
        },
    );
    let model = route.model("gpt-5.4", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hello")];
    request.http = Some(HttpOptions {
        headers: Some(BTreeMap::from([
            ("x-session-affinity".to_string(), "ses_affinity_2".to_string()),
            ("x-opencode-parent-session-id".to_string(), "ses_parent".to_string()),
        ])),
        ..Default::default()
    });
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("responses body");
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(transport_request.url, "https://api.githubcopilot.com/responses");
    assert_eq!(transport_request.header("X-Interaction-Type"), Some("conversation-subagent"));
    assert_eq!(transport_request.header("X-Interaction-Id"), Some("ses_affinity_2"));
    assert_eq!(transport_request.header("x-initiator"), Some("agent"));
}

#[test]
fn messages_lane_wire_carries_the_interaction_headers() {
    // The Anthropic escape lane (anthropic-messages route patched to the
    // copilot provider) also injects the vocabulary; plain anthropic
    // requests stay untouched.
    let route = crate::protocols::anthropic_messages::anthropic_messages_route(transport()).with(
        crate::route::RoutePatch {
            provider: Some("github-copilot".to_string()),
            ..Default::default()
        },
    );
    let model = route.model("claude-sonnet-4.6", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hello")];
    request.http = Some(HttpOptions {
        headers: Some(BTreeMap::from([(
            "x-session-affinity".to_string(),
            "ses_affinity_3".to_string(),
        )])),
        ..Default::default()
    });
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("messages body");
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(transport_request.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(transport_request.header("X-Interaction-Type"), Some("conversation-agent"));
    assert_eq!(transport_request.header("X-Interaction-Id"), Some("ses_affinity_3"));

    // Same route shape without the copilot provider: no interaction headers.
    let plain = crate::protocols::anthropic_messages::anthropic_messages_route(transport());
    let model = plain.model("claude-sonnet-4.6", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hello")];
    request.http = Some(HttpOptions {
        headers: Some(BTreeMap::from([(
            "x-session-affinity".to_string(),
            "ses_affinity_3".to_string(),
        )])),
        ..Default::default()
    });
    let prepared = plain.prepare_request(&request);
    let body = plain.build_body(&prepared).expect("messages body");
    let (transport_request, _) = plain.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(transport_request.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(transport_request.header("X-Interaction-Type"), None);
}
