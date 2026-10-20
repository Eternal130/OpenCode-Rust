//! C7c AI-MEDIA — mantle responses store/include defaults (ledger
//! `AI-B2`): Claude-on-mantle encrypted reasoning replay requires
//! `store: false` + `include: ["reasoning.encrypted_content"]` as route
//! defaults (providers/amazon-bedrock-mantle.ts:43-50).
//!
//! Row status: written as the ledger-named regression test; the
//! production behavior was verified to already hold at main@66aa5ff
//! (BE-4's mantle arm routes through `openai_compatible_responses_route`,
//! which bakes the same defaults) — reported back to the orchestrator
//! rather than claimed as a red→green fix.

use std::sync::Arc;

use oc_ai::protocols::openai_compatible::openai_compatible_responses_route;
use oc_ai::route::transport::FailingTransport;
use oc_ai::schema::{LLMRequest, Message};

/// The ledger test row (`crates/oc-ai/tests/mantle_defaults.rs::
/// store_include`): the bedrock-mantle responses lane carries the
/// store/include defaults onto every request body.
#[test]
fn store_include() {
    let transport: oc_ai::route::transport::SharedTransport =
        Arc::new(FailingTransport { message: "defaults only" });
    let route =
        openai_compatible_responses_route(Arc::clone(&transport)).with(oc_ai::route::RoutePatch {
            id: Some("bedrock-mantle-responses".to_string()),
            provider: Some("amazon-bedrock".to_string()),
            provider_metadata_key: Some("mantle".to_string()),
            endpoint: Some(oc_ai::route::EndpointPatch {
                base_url: Some("https://bedrock-mantle.us-east-1.api.aws/v1".to_string()),
                path: None,
                query: None,
            }),
            ..Default::default()
        });

    // The route defaults survive the mantle patch (merged, not replaced).
    let defaults = route.defaults.provider_options.as_ref().expect("providerOptions defaults");
    assert_eq!(defaults.get("store"), Some(&serde_json::json!(false)));
    assert_eq!(defaults.get("include"), Some(&serde_json::json!(["reasoning.encrypted_content"])));

    // And they reach the built body of a plain request.
    let mut request = LLMRequest::new(route.model("claude-opus-4-6", None).expect("model binds"));
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body builds");
    assert_eq!(body["store"], serde_json::json!(false));
    assert_eq!(body["include"], serde_json::json!(["reasoning.encrypted_content"]));
    assert_eq!(route.provider_metadata_key.as_deref(), Some("mantle"));
}
