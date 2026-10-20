//! C7c AI-MEDIA — `xai-responses` (ledger `AI-M5`).
//!
//! v2's xai provider speaks the Open Responses baseline with its own
//! hosted tool vocabulary: `x_search_call` and `image_generation_call`
//! replay as provider-executed tool call/result pairs, and in-band
//! `contextManagement` fails typed (xAI requires explicit compaction).
//! The rows drive the production route stack against the mock transport
//! (ADR-0006: no live network).

use std::sync::Arc;

use oc_ai::protocols::xai_responses::{xai_auth, xai_responses_route, XaiResponsesRouteSpec};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::SharedTransport;
use oc_ai::schema::{AiErrorReason, LLMEvent, LLMRequest, Message, ToolResultValue};
use oc_ai::test_support::{MockTransport, RecordedInteraction};
use serde_json::json;

fn sse(events: &[serde_json::Value]) -> String {
    let mut body = String::new();
    for event in events {
        let kind = event["type"].as_str().unwrap_or_default();
        body.push_str("event: ");
        body.push_str(kind);
        body.push_str("\ndata: ");
        body.push_str(&event.to_string());
        body.push_str("\n\n");
    }
    body.push_str("event: done\ndata: [DONE]\n\n");
    body
}

/// The ledger repro row (`crates/oc-ai/tests/xai_responses.rs::
/// hosted_tools`): a Grok stream completing `x_search_call` and
/// `image_generation_call` items emits provider-executed tool call +
/// result pairs, and the wire carries the route defaults.
#[tokio::test]
async fn hosted_tools() {
    let stream = sse(&[
        json!({ "type": "response.created", "response": { "id": "resp_1" } }),
        json!({
            "type": "response.output_item.done",
            "output_index": 1,
            "item": {
                "type": "x_search_call",
                "id": "xs_1",
                "status": "completed",
                "action": { "query": "grok news", "x_scope": "all" },
            }
        }),
        json!({
            "type": "response.output_item.done",
            "output_index": 2,
            "item": {
                "type": "image_generation_call",
                "id": "ig_1",
                "status": "completed",
            }
        }),
        json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "usage": { "input_tokens": 12, "output_tokens": 34, "total_tokens": 46 },
            }
        }),
    ]);
    let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://api.x.ai/v1/responses".into(),
        status: 200,
        body: stream,
        request_body: None,
    }]));
    let route = xai_responses_route(XaiResponsesRouteSpec {
        base_url: None,
        auth: Some(xai_auth(Some(Secret::Value("x-key".into())))),
        transport: Arc::clone(&transport) as SharedTransport,
        defaults: oc_ai::route::RouteDefaults::default(),
    });

    let mut request = LLMRequest::new(route.model("grok-4", None).expect("model binds"));
    request.messages = vec![Message::user("search x for grok news")];
    let response = route.generate(request).await.expect("stream completes");

    // x_search_call: provider-executed call + result, input from the action
    let search_calls: Vec<&LLMEvent> = response
        .events
        .iter()
        .filter(|event| {
            matches!(
                event,
                LLMEvent::ToolCall { name, provider_executed: Some(true), .. } if name == "x_search"
            )
        })
        .collect();
    assert_eq!(search_calls.len(), 1, "one x_search call");
    match &search_calls[0] {
        LLMEvent::ToolCall { id, input, provider_metadata, .. } => {
            assert_eq!(id, "xs_1");
            assert_eq!(input["query"], json!("grok news"));
            assert_eq!(input["x_scope"], json!("all"));
            assert!(provider_metadata
                .as_ref()
                .is_some_and(|metadata| metadata["xai"].contains_key("itemId")));
        }
        _ => panic!("tool call shape"),
    }
    assert!(response.events.iter().any(|event| {
        matches!(
            event,
            LLMEvent::ToolResult { name, provider_executed: Some(true), .. } if name == "x_search"
        )
    }));

    // image_generation_call: provider-executed pair; the result stays the
    // whole item JSON (no base64 file content on the xAI lane)
    assert!(response.events.iter().any(|event| {
        matches!(
            event,
            LLMEvent::ToolCall { name, provider_executed: Some(true), .. } if name == "image_generation"
        )
    }));
    let image_result = response
        .events
        .iter()
        .find_map(|event| match event {
            LLMEvent::ToolResult { name, result, .. } if name == "image_generation" => {
                Some(result.clone())
            }
            _ => None,
        })
        .expect("image result");
    match image_result {
        ToolResultValue::Json { value } => {
            assert_eq!(value["type"], json!("image_generation_call"));
            assert_eq!(value["id"], json!("ig_1"));
        }
        _ => panic!("xai image results stay item JSON"),
    }

    assert_wire_defaults(&transport);
}

/// The wire: the route defaults ride the body and the bearer chain signs it.
fn assert_wire_defaults(transport: &Arc<MockTransport>) {
    let sent = &transport.requests()[0];
    assert_eq!(sent.url, "https://api.x.ai/v1/responses");
    assert_eq!(sent.header("authorization"), Some("Bearer x-key"));
    let body: serde_json::Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(body["store"], json!(false));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
}

/// In-band `contextManagement` fails typed before any request goes out.
#[tokio::test]
async fn context_management_rejects_without_network() {
    let transport = Arc::new(MockTransport::from_interactions(vec![]));
    let route = xai_responses_route(XaiResponsesRouteSpec {
        base_url: None,
        auth: None,
        transport: Arc::clone(&transport) as SharedTransport,
        defaults: oc_ai::route::RouteDefaults::default(),
    });
    let mut request = LLMRequest::new(route.model("grok-4", None).expect("model binds"));
    request.messages = vec![Message::user("hi")];
    request.provider_options = Some(
        [("contextManagement".to_string(), json!([{ "type": "compaction" }]))]
            .into_iter()
            .collect(),
    );
    let Err(error) = route.generate(request).await else { panic!("contextManagement must fail") };
    let AiErrorReason::UnsupportedOperation { operation, fields, .. } = &error.reason else {
        panic!("wrong reason: {}", error.tag());
    };
    assert_eq!(operation, "in-band-compaction");
    assert!(fields.message.contains("explicit compaction through LLMClient.compact"));
    assert!(transport.requests().is_empty(), "nothing dialed");
}
