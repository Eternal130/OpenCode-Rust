//! AI-V1 — Vertex OpenAI-compatibility URL parity (port of
//! `packages/ai/src/providers/google-vertex-chat.ts:50-60` +
//! `google-vertex-responses.ts:52-61`): both lanes deploy on the fixed
//! aiplatform `OpenAPI` root —
//! `https://aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/endpoints/openapi`
//! — with the ADC bearer (API keys rejected). The catalog's maas models
//! carry `${GOOGLE_VERTEX_*}` baseURL templates that expand against the
//! resolved project/location (plugin/provider/google-vertex.ts:32-39).
//!
//! Nothing dials a real endpoint — `prepare_*` only (ADR-0006).

use std::sync::Arc;

use oc_ai::protocols::vertex::{vertex_chat_route, vertex_responses_route, VertexEndpointConfig};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::FailingTransport;
use oc_ai::schema::{LLMRequest, Message};
use serde_json::json;

fn transport() -> Arc<FailingTransport> {
    Arc::new(FailingTransport { message: "vertex openapi parity test (nothing dials)" })
}

fn config(project: &str, location: &str) -> VertexEndpointConfig {
    VertexEndpointConfig {
        base_url: None,
        project: Some(project.to_string()),
        location: Some(location.to_string()),
        access_token: Some(Secret::Value("ya29.vertex-token".into())),
        api_key: None,
    }
}

fn wire_of(
    route: &oc_ai::route::Route,
    model_id: &str,
) -> (String, serde_json::Value, oc_ai::route::transport::TransportRequest) {
    let model = route.model(model_id, None).expect("model binds");
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (wire, _) = route.prepare_transport(&prepared, &body).expect("wire");
    (wire.url.clone(), body, wire)
}

/// The named regression: chat and responses both compose the
/// project/location `OpenAPI` URL (never "needs a baseURL" and never
/// `UnsupportedProtocol`), with ADC bearer auth and the `x-goog-api-key`
/// Express header absent.
#[test]
fn vertex_chat_responses_url() {
    let chat = vertex_chat_route(transport(), config("my-proj", "us-central1"))
        .expect("vertex chat binds");
    assert_eq!(chat.id, "google-vertex-chat");
    assert_eq!(chat.provider_metadata_key.as_deref(), Some("vertex"));
    assert_eq!(
        chat.endpoint.base_url.as_deref(),
        Some("https://aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/endpoints/openapi")
    );
    let (url, body, wire) = wire_of(&chat, "meta/llama-4-maverick-17b-128e-instruct-maas");
    assert_eq!(
        url,
        "https://aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/endpoints/openapi/chat/completions"
    );
    assert_eq!(wire.header("authorization"), Some("Bearer ya29.vertex-token"));
    assert_eq!(wire.header("x-goog-api-key"), None, "Express keys are the Gemini arm");
    assert!(body["messages"].is_array(), "the OpenAI chat body shape: {body}");

    let responses = vertex_responses_route(transport(), config("my-proj", "us-central1"))
        .expect("vertex responses binds");
    assert_eq!(responses.id, "google-vertex-responses");
    assert_eq!(responses.provider_metadata_key.as_deref(), Some("vertex"));
    assert_eq!(
        responses.endpoint.base_url.as_deref(),
        Some("https://aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/endpoints/openapi")
    );
    let (url, body, wire) = wire_of(&responses, "xai/grok-4.6");
    assert_eq!(
        url,
        "https://aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/endpoints/openapi/responses"
    );
    assert_eq!(wire.header("authorization"), Some("Bearer ya29.vertex-token"));
    assert_eq!(body["store"], json!(false));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert!(body["input"].is_array(), "the Responses body shape: {body}");
}

/// The package-level `global` location fallback and the maas baseURL
/// template expansion (`${GOOGLE_VERTEX_*}` → resolved project/location,
/// plugin/provider/google-vertex.ts:32-39).
#[test]
fn global_fallback_and_catalog_template_expansion() {
    let chat = vertex_chat_route(transport(), config("p", "global")).expect("chat binds");
    assert_eq!(
        chat.endpoint.base_url.as_deref(),
        Some("https://aiplatform.googleapis.com/v1/projects/p/locations/global/endpoints/openapi")
    );

    let template =
        "https://${GOOGLE_VERTEX_ENDPOINT}/v1/projects/${GOOGLE_VERTEX_PROJECT}/locations/${GOOGLE_VERTEX_LOCATION}/endpoints/openapi";
    let chat = vertex_chat_route(
        transport(),
        VertexEndpointConfig {
            base_url: Some(template.into()),
            ..config("proj-2", "europe-west1")
        },
    )
    .expect("chat binds");
    assert_eq!(
        chat.endpoint.base_url.as_deref(),
        Some("https://europe-west1-aiplatform.googleapis.com/v1/projects/proj-2/locations/europe-west1/endpoints/openapi")
    );
    // global maps onto the multi-region root host in templates too
    let chat = vertex_chat_route(
        transport(),
        VertexEndpointConfig { base_url: Some(template.into()), ..config("proj-2", "global") },
    )
    .expect("chat binds");
    assert_eq!(
        chat.endpoint.base_url.as_deref(),
        Some("https://aiplatform.googleapis.com/v1/projects/proj-2/locations/global/endpoints/openapi")
    );
}

/// API keys are rejected on both lanes and a project is required when no
/// baseURL is configured (GoogleVertexShared.requireProject).
#[test]
fn api_keys_rejected_and_project_required_without_base_url() {
    let error = vertex_chat_route(
        transport(),
        VertexEndpointConfig { api_key: Some(Secret::Value("k".into())), ..config("p", "global") },
    )
    .unwrap_err();
    assert!(error.to_string().contains("Google Vertex Chat does not support API keys"), "{error}");
    let error = vertex_responses_route(
        transport(),
        VertexEndpointConfig { api_key: Some(Secret::Value("k".into())), ..config("p", "global") },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("Google Vertex Responses does not support API keys"),
        "{error}"
    );

    let project_envs =
        ["GOOGLE_VERTEX_PROJECT", "GOOGLE_CLOUD_PROJECT", "GCP_PROJECT", "GCLOUD_PROJECT"];
    if project_envs.iter().all(|name| std::env::var_os(name).is_none()) {
        let error = vertex_chat_route(
            transport(),
            VertexEndpointConfig {
                base_url: None,
                project: None,
                location: Some("global".into()),
                access_token: Some(Secret::Value("t".into())),
                api_key: None,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires a project"), "{error}");
    }
}
