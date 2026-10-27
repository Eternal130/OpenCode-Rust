//! `openai-compatible` re-badging routes (port of `openai-compatible-chat.ts`
//! / `openai-compatible-responses.ts`).
//!
//! These are **not** protocols: they reuse [`crate::protocols::openai_chat`]
//! and [`crate::protocols::open_responses`] verbatim and only patch the route
//! identity so catalog providers can be resolved per-family without
//! colliding with the native OpenAI routes. This is the seam for the ~25
//! catalog-only providers (`providers/openai-compatible.ts`).

// Wire identifiers in prose docs (`openai-compatible`, `providerMetadataKey`)
// read better without backtick sprinkling; same policy as `openai_chat.rs`.
#![allow(clippy::doc_markdown)]

use std::sync::Arc;

use crate::protocols::open_responses::{BaseAdapter, OpenResponsesProtocol, PATH};
use crate::protocols::openai_chat::{OpenAiChat, OpenAiChatExt, PATH as CHAT_PATH};
use crate::route::client::{Route, RouteDefaults, RouteInput, RoutePatch};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::transport::SharedTransport;
use crate::schema::ProviderOptions;

pub const OPENAI_COMPATIBLE_CHAT_ID: &str = "openai-compatible-chat";
pub const OPENAI_COMPATIBLE_RESPONSES_ID: &str = "openai-compatible-responses";

/// Route for providers exposing an OpenAI Chat-compatible
/// `/chat/completions` endpoint (port of `openai-compatible-chat.ts`):
/// the `openai-chat` protocol end-to-end, re-badged with
/// `providerMetadataKey: "openai"`. Provider helpers configure the endpoint
/// (`.with({ provider, endpoint, auth })`) before model selection — patching
/// the provider re-keys the metadata namespace to match.
pub fn openai_compatible_chat_route(transport: SharedTransport) -> Route {
    Route::new(RouteInput {
        id: OPENAI_COMPATIBLE_CHAT_ID.into(),
        provider: None,
        provider_metadata_key: Some("openai".into()),
        protocol: Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
        endpoint: Endpoint::path(CHAT_PATH),
        auth: None,
        framing: Framing::sse_with_done(),
        headers: None,
        transport,
        defaults: Default::default(),
    })
}

/// `providers/openai-compatible.ts` `configure`: bind the generic route to a
/// concrete provider + base URL. The metadata key follows the provider
/// (mirroring TS `Route.with` semantics).
pub fn openai_compatible_chat_configured(
    transport: SharedTransport,
    provider: &str,
    base_url: &str,
) -> Route {
    let mut patch = RoutePatch {
        provider: Some(provider.to_string()),
        endpoint: Some(crate::route::endpoint::EndpointPatch {
            base_url: Some(base_url.to_string()),
            path: None,
            query: None,
        }),
        ..Default::default()
    };
    // Per-provider seam (RC-06): providers registered with a
    // `chat_metadata_key` customization override the re-badged metadata
    // namespace; everyone else keeps the default `"openai"` key derived by
    // `Route::with`.
    if let Some(key) = crate::customization::for_route(provider).chat_metadata_key {
        patch.provider_metadata_key = Some(key.to_string());
    }
    openai_compatible_chat_route(transport).with(patch)
}

/// Route for providers exposing an Open Responses-compatible `/responses`
/// endpoint (port of `openai-compatible-responses.ts`): the `open-responses`
/// protocol end-to-end (`HttpTransport.sseJson` framing), re-badged with
/// `providerMetadataKey: "openresponses"` and route defaults
/// `{ providerOptions: { store: false, include: ["reasoning.encrypted_content"] } }`.
/// Provider helpers configure identity/endpoint/auth before model selection;
/// patching the provider re-keys the metadata namespace to match.
pub fn openai_compatible_responses_route(transport: SharedTransport) -> Route {
    let mut provider_options = ProviderOptions::new();
    provider_options.insert("store".to_string(), serde_json::json!(false));
    provider_options
        .insert("include".to_string(), serde_json::json!(["reasoning.encrypted_content"]));
    Route::new(RouteInput {
        id: OPENAI_COMPATIBLE_RESPONSES_ID.into(),
        provider: None,
        provider_metadata_key: Some("openresponses".into()),
        protocol: Arc::new(OpenResponsesProtocol::new(Arc::new(BaseAdapter))),
        endpoint: Endpoint::path(PATH),
        auth: None,
        framing: Framing::sse(),
        headers: None,
        transport,
        defaults: RouteDefaults { provider_options: Some(provider_options), ..Default::default() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::transport::FailingTransport;
    use crate::schema::LLMRequest;

    /// Port of the identity assertions over `openai-compatible-responses.ts`
    /// (oracle gate 3 finding 3): id, metadata key, protocol reuse, endpoint
    /// path, and the `store`/`include` route defaults surviving a provider
    /// patch.
    #[test]
    fn openai_compatible_responses_rebadges_the_open_responses_route() {
        let failing: SharedTransport = Arc::new(FailingTransport { message: "no transport" });
        let route = openai_compatible_responses_route(Arc::clone(&failing));
        assert_eq!(route.id, OPENAI_COMPATIBLE_RESPONSES_ID);
        assert_eq!(route.protocol.id(), "open-responses");
        assert_eq!(route.provider_metadata_key.as_deref(), Some("openresponses"));
        assert!(route.endpoint.base_url.is_none());
        let defaults =
            route.defaults.provider_options.as_ref().expect("providerOptions defaults present");
        assert_eq!(defaults.get("store"), Some(&serde_json::json!(false)));
        assert_eq!(
            defaults.get("include"),
            Some(&serde_json::json!(["reasoning.encrypted_content"]))
        );

        // `.with({ provider, endpoint })` re-keys the metadata namespace and
        // merges (not replaces) the defaults, mirroring TS Route.with.
        let configured = route.with(RoutePatch {
            provider: Some("moonshot".to_string()),
            endpoint: Some(crate::route::endpoint::EndpointPatch {
                base_url: Some("https://api.moonshot.test/v1".to_string()),
                path: None,
                query: None,
            }),
            ..Default::default()
        });
        assert_eq!(configured.provider.as_ref().map(|p| p.0.as_str()), Some("moonshot"));
        assert_eq!(configured.provider_metadata_key.as_deref(), Some("moonshot"));
        let merged =
            configured.defaults.provider_options.as_ref().expect("defaults merge through .with");
        assert_eq!(merged.get("store"), Some(&serde_json::json!(false)));
        assert_eq!(
            merged.get("include"),
            Some(&serde_json::json!(["reasoning.encrypted_content"]))
        );

        // The route builds a model and a body carrying the lowered defaults.
        let model = configured.model("kimi-k2", None).expect("model builds");
        let request = LLMRequest::new(model).with_prompt("hi");
        let prepared = configured.prepare_request(&request);
        let body = configured.build_body(&prepared).expect("body builds");
        assert_eq!(body["model"], "kimi-k2");
        assert_eq!(body["store"], false);
        assert_eq!(body["include"], serde_json::json!(["reasoning.encrypted_content"]));
    }
}
