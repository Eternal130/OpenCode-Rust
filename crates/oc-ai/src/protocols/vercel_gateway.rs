//! Vercel AI Gateway (port of `providers/vercel-ai-gateway.ts`).
//!
//! One provider, three routes — Anthropic Messages, Open Responses, and
//! OpenAI Chat — each wrapped by the [`GatewayProtocol`] seam
//! (`utils/gateway-protocol.ts`): `providerOptions.gateway` rides the
//! request body, and the gateway's `provider_metadata.gateway`
//! usage/cost accounting is merged into the finish events. Each model
//! family uses the API whose Gateway translation carries its reasoning
//! state across turns (providers/vercel-ai-gateway.ts:272-277):
//! `openai/gpt-*` / `spacexai/grok-*` → responses, `meta/muse-*` → chat,
//! everything else → messages.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;

use crate::protocols::anthropic_messages::{self, AnthropicMessages};
use crate::protocols::open_responses::{BaseAdapter, OpenResponsesProtocol};
use crate::protocols::openai_chat::{OpenAiChat, OpenAiChatExt};
use crate::protocols::utils::gateway_protocol::{GatewayApi, GatewayProtocol};
use crate::route::auth::Definition as AuthDefinition;
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::transport::SharedTransport;
use crate::route::{merge_route_defaults, Route, RouteDefaults, RouteInput};
use crate::schema::LLMRequest;

/// Stable provider id (TS `id = ProviderID.make("vercel-ai-gateway")`).
pub const PROVIDER: &str = "vercel-ai-gateway";
/// The gateway's default deployment root.
pub const DEFAULT_BASE_URL: &str = "https://ai-gateway.vercel.sh/v1";
/// The messages route id (`messagesRoute`).
pub const MESSAGES_ID: &str = "vercel-ai-gateway-messages";
/// The responses route id (`responsesRoute`).
pub const RESPONSES_ID: &str = "vercel-ai-gateway-responses";
/// The chat route id (`chatRoute`).
pub const CHAT_ID: &str = "vercel-ai-gateway-chat";

/// The gateway auth env chain (`AuthOptions.bearer(input,
/// ["AI_GATEWAY_API_KEY", "VERCEL_OIDC_TOKEN"])`).
pub const AUTH_ENV: [&str; 2] = ["AI_GATEWAY_API_KEY", "VERCEL_OIDC_TOKEN"];

/// Session-affinity header from the request's prompt cache key
/// (`headers: ({ request }) => …`, providers/vercel-ai-gateway.ts:200-202).
fn session_affinity_headers(request: &LLMRequest) -> BTreeMap<String, String> {
    match &request.prompt_cache_key {
        Some(key) => BTreeMap::from([("x-session-affinity".to_string(), key.clone())]),
        None => BTreeMap::new(),
    }
}

/// The responses route defaults (`store: false` + encrypted reasoning
/// include, providers/vercel-ai-gateway.ts:219).
#[must_use]
pub fn responses_route_defaults() -> RouteDefaults {
    RouteDefaults {
        provider_options: Some(
            [
                ("store".to_string(), json!(false)),
                ("include".to_string(), json!(["reasoning.encrypted_content"])),
            ]
            .into_iter()
            .collect(),
        ),
        ..Default::default()
    }
}

/// The messages route defaults (the `anthropic-version` header).
#[must_use]
pub fn messages_route_defaults() -> RouteDefaults {
    RouteDefaults {
        headers: Some(BTreeMap::from([(
            "anthropic-version".to_string(),
            "2023-06-01".to_string(),
        )])),
        ..Default::default()
    }
}

/// Inputs for the vercel gateway route factories.
pub struct VercelGatewayRouteSpec {
    pub base_url: Option<String>,
    pub auth: Option<AuthDefinition>,
    pub transport: SharedTransport,
    /// Caller route defaults merged over the per-API defaults.
    pub defaults: RouteDefaults,
}

fn route(spec: &VercelGatewayRouteSpec, api: GatewayApi) -> Route {
    let (id, protocol, path, framing, defaults): (
        &str,
        Arc<dyn crate::route::protocol::Protocol>,
        &str,
        Framing,
        RouteDefaults,
    ) = match api {
        GatewayApi::Messages => (
            MESSAGES_ID,
            Arc::new(GatewayProtocol::new(MESSAGES_ID, Arc::new(AnthropicMessages), api)),
            anthropic_messages::PATH,
            anthropic_messages::framing(),
            messages_route_defaults(),
        ),
        GatewayApi::Responses => (
            RESPONSES_ID,
            Arc::new(GatewayProtocol::new(
                RESPONSES_ID,
                Arc::new(OpenResponsesProtocol::new(Arc::new(BaseAdapter))),
                api,
            )),
            crate::protocols::open_responses::PATH,
            Framing::sse(),
            responses_route_defaults(),
        ),
        GatewayApi::Chat => (
            CHAT_ID,
            Arc::new(GatewayProtocol::new(
                CHAT_ID,
                Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
                api,
            )),
            crate::protocols::openai_chat::PATH,
            Framing::sse_with_done(),
            RouteDefaults::default(),
        ),
    };
    Route::new(RouteInput {
        id: id.to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some(PROVIDER.to_string()),
        protocol,
        endpoint: Endpoint::path(path)
            .with_base_url(spec.base_url.clone().unwrap_or_else(|| DEFAULT_BASE_URL.to_string())),
        auth: spec.auth.clone(),
        framing,
        headers: Some(Arc::new(session_affinity_headers)),
        transport: Arc::clone(&spec.transport),
        defaults: merge_route_defaults(Some(&defaults), &spec.defaults),
    })
}

/// Build the messages route (`/messages`, port of `messagesRoute`).
#[must_use]
pub fn messages_route(spec: &VercelGatewayRouteSpec) -> Route {
    route(spec, GatewayApi::Messages)
}

/// Build the responses route (`/responses`, port of `responsesRoute`).
#[must_use]
pub fn responses_route(spec: &VercelGatewayRouteSpec) -> Route {
    route(spec, GatewayApi::Responses)
}

/// Build the chat route (`/chat/completions`, port of `chatRoute`).
#[must_use]
pub fn chat_route(spec: &VercelGatewayRouteSpec) -> Route {
    route(spec, GatewayApi::Chat)
}

/// Per-model API dispatch (port of `model(modelID)`,
/// providers/vercel-ai-gateway.ts:272-277): `openai/gpt-*` and
/// `spacexai/grok-*` ride the responses translation, `meta/muse-*` the
/// chat translation, everything else messages.
#[must_use]
pub fn route_for_model(spec: &VercelGatewayRouteSpec, model_id: &str) -> Route {
    if model_id.starts_with("openai/gpt-") || model_id.starts_with("spacexai/grok-") {
        responses_route(spec)
    } else if model_id.starts_with("meta/muse-") {
        chat_route(spec)
    } else {
        messages_route(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::transport::FailingTransport;
    use crate::schema::LLMRequest;

    fn spec() -> VercelGatewayRouteSpec {
        VercelGatewayRouteSpec {
            base_url: Some("https://gateway.test/v1".into()),
            auth: None,
            transport: Arc::new(FailingTransport { message: "unit" }),
            defaults: RouteDefaults::default(),
        }
    }

    fn request() -> LLMRequest {
        crate::test_support::request_for_tests(vec![crate::schema::Message::user("hi")])
    }

    #[test]
    fn model_families_pick_their_api() {
        assert_eq!(route_for_model(&spec(), "openai/gpt-5.2").id, RESPONSES_ID);
        assert_eq!(route_for_model(&spec(), "spacexai/grok-4").id, RESPONSES_ID);
        assert_eq!(route_for_model(&spec(), "meta/muse-1").id, CHAT_ID);
        assert_eq!(route_for_model(&spec(), "anthropic/claude-5").id, MESSAGES_ID);
        assert_eq!(route_for_model(&spec(), "alibaba/qwen3.8-max").id, MESSAGES_ID);
    }

    #[test]
    fn routes_carry_the_gateway_identity_and_defaults() {
        let responses = responses_route(&spec());
        assert_eq!(responses.provider_metadata_key.as_deref(), Some(PROVIDER));
        let prepared = responses.prepare_request(&request());
        let body = responses.build_body(&prepared).unwrap();
        assert_eq!(body["store"], json!(false));
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        let (wire, _) = responses.prepare_transport(&prepared, &body).unwrap();
        assert_eq!(wire.url, "https://gateway.test/v1/responses");

        let messages = messages_route(&spec());
        let (wire, _) =
            messages.prepare_transport(&messages.prepare_request(&request()), &json!({})).unwrap();
        assert_eq!(wire.url, "https://gateway.test/v1/messages");
        assert_eq!(wire.header("anthropic-version"), Some("2023-06-01"));

        let chat = chat_route(&spec());
        let (wire, _) =
            chat.prepare_transport(&chat.prepare_request(&request()), &json!({})).unwrap();
        assert_eq!(wire.url, "https://gateway.test/v1/chat/completions");
    }

    #[test]
    fn prompt_cache_key_sets_session_affinity() {
        let mut req = request();
        req.prompt_cache_key = Some("session-1".to_string());
        assert_eq!(
            session_affinity_headers(&req).get("x-session-affinity"),
            Some(&"session-1".to_string())
        );
        assert!(session_affinity_headers(&request()).is_empty());
    }
}
