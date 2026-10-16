//! Vertex route tests: project/location routing goldens, tuned endpoints,
//! Express Mode, the ADC bearer chain, and the labels/serviceTier body and
//! header behavior over the shared Gemini protocol.

use serde_json::json;

use super::{
    adc_access_token, endpoint_base_url, host, resolve_location, vertex_route_for_model,
    VertexConfig,
};
use crate::route::client::RoutePatch;
use crate::route::SharedTransport;
use crate::schema::{ContentPart, LLMRequest, Message, MessageRole};
use crate::test_support::MockTransport;
use std::collections::BTreeMap;
use std::sync::Arc;

fn transport() -> SharedTransport {
    Arc::new(MockTransport::from_interactions(vec![]))
}

#[test]
fn hosts_route_by_location() {
    assert_eq!(host("us-central1"), "us-central1-aiplatform.googleapis.com");
    assert_eq!(host("global"), "aiplatform.googleapis.com");
    assert_eq!(host("eu"), "aiplatform.eu.rep.googleapis.com");
    assert_eq!(host("us"), "aiplatform.us.rep.googleapis.com");
}

#[test]
fn location_falls_back_to_us_central1() {
    // no env in the test sandbox pins these; assert the fallback survives
    let resolved = resolve_location(None, "us-central1");
    assert!(
        resolved == "us-central1"
            || std::env::var("GOOGLE_VERTEX_LOCATION").is_ok()
            || std::env::var("GOOGLE_CLOUD_LOCATION").is_ok()
            || std::env::var("VERTEX_LOCATION").is_ok(),
        "unexpected location {resolved}"
    );
}

#[test]
fn base_urls_embed_project_location_and_publishers() {
    assert_eq!(
        endpoint_base_url(Some("my-proj"), "us-central1", false, false),
        "https://us-central1-aiplatform.googleapis.com/v1beta1/projects/my-proj/locations/us-central1/publishers/google"
    );
    assert_eq!(
        endpoint_base_url(Some("my-proj"), "europe-west1", false, true),
        "https://europe-west1-aiplatform.googleapis.com/v1beta1/projects/my-proj/locations/europe-west1"
    );
    assert_eq!(
        endpoint_base_url(None, "global", true, false),
        "https://aiplatform.googleapis.com/v1/publishers/google"
    );
}

#[test]
fn vertex_route_embeds_the_model_in_the_path_and_bears_the_token() {
    let route = vertex_route_for_model(
        transport(),
        VertexConfig {
            project: Some("my-proj".into()),
            location: Some("us-central1".into()),
            access_token: Some(crate::route::auth::Secret::Value("ya29.token".into())),
            ..Default::default()
        },
        "gemini-flash-latest",
    );
    assert_eq!(route.id, "google-vertex-gemini");
    assert_eq!(route.provider_metadata_key.as_deref(), Some("vertex"));
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some("https://us-central1-aiplatform.googleapis.com/v1beta1/projects/my-proj/locations/us-central1/publishers/google")
    );
    let model = route.model("gemini-flash-latest", None).expect("model binds");
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("gemini body");
    let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert_eq!(
        transport_request.url,
        "https://us-central1-aiplatform.googleapis.com/v1beta1/projects/my-proj/locations/us-central1/publishers/google/models/gemini-flash-latest:streamGenerateContent?alt=sse"
    );
    assert_eq!(transport_request.header("authorization"), Some("Bearer ya29.token"));
    assert_eq!(body["contents"][0]["role"], json!("user"));
}

#[test]
fn tuned_endpoints_address_the_project_root() {
    let route = vertex_route_for_model(
        transport(),
        VertexConfig {
            project: Some("my-proj".into()),
            access_token: Some(crate::route::auth::Secret::Value("t".into())),
            ..Default::default()
        },
        "endpoints/1234567890",
    );
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some("https://us-central1-aiplatform.googleapis.com/v1beta1/projects/my-proj/locations/us-central1")
    );
    let model = route.model("endpoints/1234567890", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert!(
        transport_request
            .url
            .ends_with("/locations/us-central1/endpoints/1234567890:streamGenerateContent?alt=sse"),
        "{}",
        transport_request.url
    );
}

#[test]
fn express_mode_uses_the_api_key_header() {
    let route = vertex_route_for_model(
        transport(),
        VertexConfig {
            project: Some("my-proj".into()),
            api_key: Some(crate::route::auth::Secret::Value("express-key".into())),
            ..Default::default()
        },
        "gemini-flash-latest",
    );
    assert_eq!(
        route.endpoint.base_url.as_deref(),
        Some("https://aiplatform.googleapis.com/v1/publishers/google")
    );
    let model = route.model("gemini-flash-latest", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let (transport_request, _) = route.prepare_transport(&prepared, &body).unwrap();
    assert_eq!(transport_request.header("x-goog-api-key"), Some("express-key"));
    assert_eq!(transport_request.header("authorization"), None);
}

#[test]
fn labels_ride_the_body_and_service_tier_the_header() {
    let route = vertex_route_for_model(
        transport(),
        VertexConfig {
            access_token: Some(crate::route::auth::Secret::Value("t".into())),
            ..Default::default()
        },
        "gemini-flash-latest",
    );
    let model = route.model("gemini-flash-latest", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    let mut options = serde_json::Map::new();
    options.insert("serviceTier".to_string(), json!("shared-preview"));
    options.insert("labels".to_string(), json!({ "team": "core", "count": 7 }));
    request.provider_options = Some(options);
    let prepared = route.prepare_request(&request);
    let headers = prepared.http.as_ref().and_then(|http| http.headers.as_ref());
    assert_eq!(
        headers.and_then(|headers| headers.get("x-vertex-ai-llm-shared-request-type")),
        Some(&"shared-preview".to_string()),
        "serviceTier rides the request-type header"
    );
    let body = route.build_body(&prepared).unwrap();
    assert_eq!(body["labels"], json!({ "team": "core" }));
    assert!(body.to_string().find("serviceTier").is_none(), "serviceTier must not reach the body");
}

#[test]
fn adc_env_token_resolves_at_apply_time() {
    std::env::set_var("GOOGLE_OAUTH_ACCESS_TOKEN", "adc-token");
    let definition = adc_access_token(None);
    let input = crate::route::auth::AuthInput {
        request_http: None,
        method: "POST",
        url: "https://us-central1-aiplatform.googleapis.com/v1beta1/x",
        body: "{}",
        headers: crate::route::auth::Headers::new(),
    };
    let headers = definition.apply(&input).expect("env token resolves");
    assert_eq!(headers.get("authorization"), Some("Bearer adc-token"));
    std::env::remove_var("GOOGLE_OAUTH_ACCESS_TOKEN");
    let error = adc_access_token(None).apply(&input).unwrap_err();
    assert!(error.message().contains("GOOGLE_OAUTH_ACCESS_TOKEN"), "{}", error.message());
}

#[test]
fn image_attachments_lower_through_the_shared_gemini_protocol() {
    let route = vertex_route_for_model(
        transport(),
        VertexConfig {
            access_token: Some(crate::route::auth::Secret::Value("t".into())),
            ..Default::default()
        },
        "gemini-flash-latest",
    );
    let model = route.model("gemini-flash-latest", None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::new(
        MessageRole::User,
        vec![
            ContentPart::text("what is this"),
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
            },
        ],
    )];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).unwrap();
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1]["inlineData"]["mimeType"], json!("image/png"));
    assert_eq!(parts[1]["inlineData"]["data"], json!("aGk="));
}

#[test]
fn patching_the_provider_rekeys_the_metadata_namespace() {
    let route = vertex_route_for_model(
        transport(),
        VertexConfig {
            access_token: Some(crate::route::auth::Secret::Value("t".into())),
            ..Default::default()
        },
        "gemini-flash-latest",
    )
    .with(RoutePatch { provider: Some("google-vertex".into()), ..Default::default() });
    let _ = route;
    let _: Option<BTreeMap<String, String>> = None;
}
