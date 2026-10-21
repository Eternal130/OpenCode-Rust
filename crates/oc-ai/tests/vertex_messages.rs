//! AI-VERTEX-V2 — Vertex Claude Messages URL/auth parity (port of
//! `packages/ai/src/providers/google-vertex-messages.ts` + the shared
//! `google-vertex-shared.ts` helpers).
//!
//! v2 semantics: the Anthropic Messages protocol deployed on Vertex —
//! URL `{host(location)}/v1/projects/{project}/locations/{location}/publishers/anthropic/models`
//! with the model riding the path (`/{model}:streamRawPredict`), the
//! `anthropic-version` header, the body `anthropic_version:
//! vertex-2023-10-16` (and no `model` field), and OAuth accessToken/ADC
//! auth (API keys are rejected; the `x-goog-api-key` Express header is the
//! Gemini arm, never this one).
//!
//! Nothing dials a real endpoint — `prepare_*` only (ADR-0006).

use std::sync::Arc;

use oc_ai::protocols::vertex_messages::{
    vertex_messages_route, VertexMessagesConfig, BODY_ANTHROPIC_VERSION, HEADER_VERSION,
};
use oc_ai::route::auth::Secret;
use oc_ai::route::transport::FailingTransport;
use oc_ai::schema::{LLMRequest, Message};
use serde_json::json;

fn transport() -> Arc<FailingTransport> {
    Arc::new(FailingTransport { message: "vertex-messages parity test (nothing dials)" })
}

fn config(project: &str, location: &str) -> VertexMessagesConfig {
    VertexMessagesConfig {
        base_url: None,
        project: Some(project.to_string()),
        location: Some(location.to_string()),
        access_token: Some(Secret::Value("ya29.vertex-token".into())),
        api_key: None,
    }
}

/// The named regression: the google-vertex/messages binding must never
/// fall back to api.anthropic.com — the URL is composed from
/// project/location, the model rides `:streamRawPredict`, auth is the ADC
/// bearer (never x-goog-api-key), and the body carries the Vertex
/// `anthropic_version` instead of `model`.
#[test]
fn never_api_anthropic() {
    let route = vertex_messages_route(transport(), config("my-proj", "us-central1"))
        .expect("vertex messages route binds");
    assert_eq!(route.id, "google-vertex-messages");
    assert_eq!(route.provider.as_ref().map(|id| id.0.as_str()), Some("google-vertex"));
    assert_eq!(route.provider_metadata_key.as_deref(), Some("vertex"));
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/publishers/anthropic/models"
        ),
        "the v2 base URL embeds host(location)/project/location"
    );

    let model = route.model("claude-sonnet-4-5", None).expect("model binds");
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (wire, _) = route.prepare_transport(&prepared, &body).expect("wire");

    assert_eq!(
        wire.url,
        "https://us-central1-aiplatform.googleapis.com/v1/projects/my-proj/locations/us-central1/publishers/anthropic/models/claude-sonnet-4-5:streamRawPredict"
    );
    assert!(!wire.url.contains("api.anthropic.com"), "never the generic messages base");
    assert_eq!(wire.header("authorization"), Some("Bearer ya29.vertex-token"));
    assert_eq!(wire.header("anthropic-version"), Some(HEADER_VERSION));
    assert_eq!(wire.header("x-api-key"), None);
    assert_eq!(wire.header("x-goog-api-key"), None, "the Express header is the Gemini arm");
    assert_eq!(body["anthropic_version"], json!(BODY_ANTHROPIC_VERSION));
    assert!(body.get("model").is_none(), "the model rides the URL, not the body");
    assert!(body["messages"].is_array(), "the Messages body shape survives: {body}");
}

/// `global` (the v2 messages fallback location) uses the multi-region
/// root host, and an explicit baseURL wins over the composed one.
#[test]
fn global_location_and_explicit_base_url() {
    let route = vertex_messages_route(transport(), config("p", "global")).expect("route binds");
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some("https://aiplatform.googleapis.com/v1/projects/p/locations/global/publishers/anthropic/models")
    );
    let route = vertex_messages_route(
        transport(),
        VertexMessagesConfig {
            base_url: Some("https://vertex-proxy.test/v1".into()),
            ..config("p", "us-central1")
        },
    )
    .expect("route binds");
    assert_eq!(route.endpoint.base_url.as_deref(), Some("https://vertex-proxy.test/v1"));
}

/// v2 rejects API keys on this lane (`GoogleVertex Messages does not
/// support API keys`) and requires a project when no baseURL is set.
#[test]
fn api_key_rejected_and_project_required_without_base_url() {
    let error = vertex_messages_route(
        transport(),
        VertexMessagesConfig {
            api_key: Some(Secret::Value("express-key".into())),
            ..config("p", "us-central1")
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("does not support API keys"), "{error}");

    // requireProject: without a baseURL and any project env alias there is
    // no URL to compose — the typed configuration error (test sandbox has
    // none of the four project env aliases set; tolerate a host that does).
    let project_envs =
        ["GOOGLE_VERTEX_PROJECT", "GOOGLE_CLOUD_PROJECT", "GCP_PROJECT", "GCLOUD_PROJECT"];
    if project_envs.iter().all(|name| std::env::var_os(name).is_none()) {
        let error = vertex_messages_route(
            transport(),
            VertexMessagesConfig {
                base_url: None,
                project: None,
                location: Some("us-central1".into()),
                access_token: Some(Secret::Value("t".into())),
                api_key: None,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires a project"), "{error}");
    }
}
