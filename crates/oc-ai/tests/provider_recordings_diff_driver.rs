//! Provider protocol-decode differential driver (rust side, channel 7).
//!
//! Env-gated (no-op when unset; nextest sees a passing no-op):
//!   PROVIDER_DIFF_MANIFEST=<file> PROVIDER_DIFF_OUT=<file> \
//!     cargo nextest run -p oc-ai --test provider_recordings_diff_driver
//!
//! For every manifest case ((cassette, response-body) pair staged by
//! tools/diff/provider_recordings.py) the recorded body is replayed through
//! the production oc-ai route stack (the same `MockTransport` + `route.stream`
//! entry the cassette parity tests use) and the emitted LLMEvent sequence —
//! or the failing error — is recorded. Output schema matches
//! tools/diff/drivers/provider_recordings_ref.ts exactly.

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use serde_json::Value;

use oc_ai::protocols::anthropic_messages::anthropic_messages_route;
use oc_ai::protocols::azure::{azure_chat_route, azure_responses_route, AzureConfig};
use oc_ai::protocols::gemini::gemini_route;
use oc_ai::protocols::google_interactions::{google_interactions_route, GoogleInteractionsConfig};
use oc_ai::protocols::meta_responses::{meta_responses_route, MetaResponsesRouteSpec};
use oc_ai::protocols::mistral_chat::mistral_route;
use oc_ai::protocols::open_responses::{open_responses_route, OpenResponsesRouteSpec};
use oc_ai::protocols::openai_chat::{
    alibaba_chat_compatibility, alibaba_chat_route, cerebras_chat_compatibility,
    cohere_chat_compatibility, deepinfra_chat_compatibility, digitalocean_chat_compatibility,
    digitalocean_chat_route, groq_chat_compatibility, groq_chat_route, meta_chat_compatibility,
    minimax_chat_compatibility, minimax_chat_route, moonshot_chat_compatibility,
    moonshot_chat_route, openai_chat_route, venice_chat_compatibility, venice_chat_route,
    zai_chat_compatibility, zai_chat_route,
};
use oc_ai::protocols::openai_responses::{openai_responses_route, OpenAIResponsesRouteSpec};
use oc_ai::protocols::xai_responses::{xai_responses_route, XaiResponsesRouteSpec};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::SharedTransport;
use oc_ai::route::{Route, RouteDefaults, RoutePatch};
use oc_ai::schema::{LLMRequest, LanguageModelCompatibility, Message};
use oc_ai::test_support::{MockTransport, RecordedInteraction};

struct Case {
    name: String,
    family: String,
    group: String,
    provider: String,
    meta_key: String,
    model: String,
    status: u16,
    url: String,
    body_file: PathBuf,
}

fn compat_for(group: &str) -> Option<LanguageModelCompatibility> {
    let compat = match group {
        "groq-chat" => groq_chat_compatibility(),
        "cerebras-chat" => cerebras_chat_compatibility(),
        "deepinfra-chat" => deepinfra_chat_compatibility(),
        "digitalocean-chat" => digitalocean_chat_compatibility(),
        "meta-chat" => meta_chat_compatibility(),
        "minimax-chat" => minimax_chat_compatibility(),
        "moonshot-chat" => moonshot_chat_compatibility(),
        "venice-chat" => venice_chat_compatibility(),
        "zai-chat" => zai_chat_compatibility(),
        "alibaba-chat" => alibaba_chat_compatibility(),
        "cohere" => cohere_chat_compatibility(),
        _ => return None,
    };
    Some(compat)
}

fn azure_config() -> AzureConfig {
    AzureConfig {
        resource_name: Some("replay-resource".into()),
        base_url: None,
        api_key: Some(Secret::Value("fixture".into())),
        api_version: None,
        query_params: Default::default(),
        use_deployment_based_urls: false,
        auth: None,
        defaults: RouteDefaults::default(),
    }
}

fn build_route(case: &Case, transport: SharedTransport) -> Route {
    let patch = |provider: &str, key: &str| RoutePatch {
        provider: Some(provider.to_string()),
        provider_metadata_key: Some(key.to_string()),
        ..Default::default()
    };
    match case.family.as_str() {
        "anthropic-messages" => {
            anthropic_messages_route(transport).with(patch(&case.provider, &case.meta_key))
        }
        "openai-responses" => {
            if case.group == "azure" {
                azure_responses_route(&azure_config(), transport, &case.model)
                    .expect("azure responses route builds")
            } else {
                openai_responses_route(OpenAIResponsesRouteSpec {
                    base_url: Some("https://replay.test/v1".into()),
                    auth: None,
                    transport,
                    defaults: RouteDefaults::default(),
                })
            }
        }
        "xai-responses" => xai_responses_route(XaiResponsesRouteSpec {
            base_url: Some("https://replay.test/v1".into()),
            auth: None,
            transport,
            defaults: RouteDefaults::default(),
        }),
        "open-responses" => open_responses_route(OpenResponsesRouteSpec {
            id: format!("diff-{}", case.group),
            provider: case.provider.clone(),
            provider_metadata_key: Some(case.meta_key.clone()),
            base_url: "https://replay.test/v1".into(),
            auth: None,
            transport,
            defaults: RouteDefaults::default(),
        }),
        "meta-responses" => meta_responses_route(MetaResponsesRouteSpec {
            base_url: Some("https://replay.test/v1".into()),
            auth: None,
            transport,
            defaults: RouteDefaults::default(),
        }),
        "mistral-chat" => mistral_route(transport),
        "cohere-chat" => oc_ai::protocols::cohere_chat::cohere_route(transport),
        "gemini" => {
            let route = gemini_route(transport);
            if case.group == "google-vertex" {
                // Vertex-hosted Gemini shares the decode machine; only the
                // provider identity/metadata namespace differ (ref
                // providers/google-vertex.ts wraps Gemini.protocol).
                route.with(RoutePatch {
                    provider: Some("google-vertex".into()),
                    provider_metadata_key: Some("vertex".into()),
                    ..Default::default()
                })
            } else {
                route
            }
        }
        "google-interactions" => google_interactions_route(
            transport,
            GoogleInteractionsConfig {
                api_key: Some(Secret::Value("fixture".into())),
                base_url: None,
            },
        ),
        "openai-chat" => match case.group.as_str() {
            "groq-chat" => groq_chat_route(transport, None),
            "venice-chat" => venice_chat_route(transport, None),
            "zai-chat" => zai_chat_route(transport, None),
            "alibaba-chat" => alibaba_chat_route(transport, None),
            "minimax-chat" => minimax_chat_route(transport, None),
            "moonshot-chat" => moonshot_chat_route(transport, None),
            "digitalocean-chat" => digitalocean_chat_route(transport, None),
            "azure" => azure_chat_route(&azure_config(), transport, &case.model)
                .expect("azure chat route builds"),
            _ => openai_chat_route(transport).with(patch(&case.provider, &case.meta_key)),
        },
        other => panic!("unknown family {other}"),
    }
}

#[tokio::test]
async fn provider_recordings_diff_driver() {
    let Some(manifest) = std::env::var_os("PROVIDER_DIFF_MANIFEST") else {
        return; // no manifest staged: driver inactive
    };
    let out = PathBuf::from(std::env::var_os("PROVIDER_DIFF_OUT").expect("PROVIDER_DIFF_OUT"));
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&manifest).expect("read manifest"))
            .expect("parse manifest");

    let mut results: Vec<Value> = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap_or("?").to_string();
        let group = case["group"].as_str().unwrap_or("").to_string();
        let family = case["family"].as_str().unwrap_or("").to_string();
        let body = std::fs::read_to_string(case["body_file"].as_str().expect("body_file"))
            .unwrap_or_else(|error| panic!("{name}: read body: {error}"));
        let entry = Case {
            name: name.clone(),
            family,
            group,
            provider: case["provider"].as_str().unwrap_or("").to_string(),
            meta_key: case["meta_key"].as_str().unwrap_or("").to_string(),
            model: case["model"].as_str().unwrap_or("test-model").to_string(),
            status: case["status"].as_u64().unwrap_or(200) as u16,
            url: case["url"].as_str().unwrap_or("https://replay.test/v1").to_string(),
            body_file: PathBuf::from(case["body_file"].as_str().unwrap()),
        };
        let _ = entry.body_file; // body already read above

        let transport: SharedTransport =
            Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
                method: "POST".into(),
                url: entry.url.clone(),
                status: entry.status,
                body,
                request_body: None,
            }]));
        let route = build_route(&entry, Arc::clone(&transport));
        let mut model = route
            .model(entry.model.clone(), None)
            .unwrap_or_else(|error| panic!("{name}: model builds: {}", error.message()));
        if let Some(compatibility) = compat_for(&entry.group) {
            model = model.with_compatibility(compatibility);
        }
        let mut request = LLMRequest::new(model);
        request.messages = vec![Message::user("hi")];

        let mut events: Vec<Value> = Vec::new();
        let mut error: Option<Value> = None;
        match route.stream(request).await {
            Ok(stream) => {
                futures::pin_mut!(stream);
                while let Some(item) = stream.next().await {
                    match item {
                        Ok(event) => events.push(
                            serde_json::to_value(&event).unwrap_or_else(|e| panic!("{name}: {e}")),
                        ),
                        Err(err) => {
                            error = Some(serde_json::json!({
                                "message": err.message(),
                                "tag": err.tag(),
                            }));
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                error = Some(serde_json::json!({
                    "message": err.message(),
                    "tag": err.tag(),
                }));
            }
        }
        results.push(serde_json::json!({
            "name": name,
            "ok": error.is_none(),
            "events": events,
            "error": error,
        }));
    }

    std::fs::write(&out, serde_json::to_vec(&results).expect("serialize")).expect("write out");
    println!("rust driver: {} cases -> {}", results.len(), out.display());
}
