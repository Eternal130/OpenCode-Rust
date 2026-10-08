//! `openai-compatible` re-badging routes (port of `openai-compatible-chat.ts`
//! / `openai-compatible-responses.ts`).
//!
//! These are **not** protocols: they reuse [`crate::protocols::openai_chat`]
//! (and later `open_responses`) verbatim and only patch the route identity so
//! catalog providers can be resolved per-family without colliding with the
//! native OpenAI routes. This is the seam for the ~25 catalog-only providers
//! (`providers/openai-compatible.ts`).

// Wire identifiers in prose docs (`openai-compatible`, `providerMetadataKey`)
// read better without backtick sprinkling; same policy as `openai_chat.rs`.
#![allow(clippy::doc_markdown)]

use std::sync::Arc;

use crate::protocols::openai_chat::{OpenAiChat, OpenAiChatExt, PATH};
use crate::route::client::{Route, RouteInput, RoutePatch};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::transport::SharedTransport;

pub const OPENAI_COMPATIBLE_CHAT_ID: &str = "openai-compatible-chat";

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
        endpoint: Endpoint::path(PATH),
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
    openai_compatible_chat_route(transport).with(RoutePatch {
        provider: Some(provider.to_string()),
        endpoint: Some(crate::route::endpoint::EndpointPatch {
            base_url: Some(base_url.to_string()),
            path: None,
            query: None,
        }),
        ..Default::default()
    })
}

// TODO(responses-lane): `openai_compatible_responses_route` re-badging
// `open_responses::protocol` with
// `defaults { store: false, include: ["reasoning.encrypted_content"] }`
// (port of `openai-compatible-responses.ts`) — owned by the open-responses
// lane; intentionally not implemented here.
