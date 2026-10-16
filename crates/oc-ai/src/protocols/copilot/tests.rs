//! Copilot chat tests: header policy (client + per-request), interaction
//! vocabulary, auth chain, and the wire shape over the openai-compatible
//! chat protocol.

use serde_json::json;

use super::{
    copilot_chat_route, copilot_client_headers, copilot_request_headers, interaction_type,
    request_metadata, CopilotConfig, SessionRequestKind, COPILOT_BASE_URL,
};
use crate::route::SharedTransport;
use crate::schema::{ContentPart, LLMRequest, Message, MessageRole, ToolResultValue};
use crate::test_support::MockTransport;
use std::sync::Arc;

fn transport() -> SharedTransport {
    Arc::new(MockTransport::from_interactions(vec![]))
}

fn media_part() -> ContentPart {
    ContentPart::Media {
        media: crate::schema::Asset {
            source: crate::schema::media::Source::Base64 {
                data: "aGk=".into(),
                media_type: "image/png".into(),
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
