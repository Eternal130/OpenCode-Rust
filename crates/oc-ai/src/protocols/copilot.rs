//! GitHub Copilot chat (port of the LLM slice of
//! `plugin/provider/github-copilot.ts`): the OpenAI-compatible chat protocol
//! over the credential-bound transport with the Copilot client header
//! policy — `applyHeaders` (bearer token, `Openai-Intent`,
//! `X-GitHub-Api-Version`, the `x-initiator` user/agent escalation,
//! `Copilot-Vision-Request` for image turns) and the `X-Interaction-Type`
//! vocabulary. The credential side (device OAuth, base-URL resolution,
//! `CredentialBindings`) landed with BE-3/BE-3b in oc-core.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::copilot_responses::copilot_responses_route;
use crate::protocols::openai_chat::{OpenAiChat, OpenAiChatExt, PATH as CHAT_PATH};
use crate::route::auth::{self, Definition, Secret};
use crate::route::client::{Route, RouteDefaults, RouteInput};
use crate::route::endpoint::Endpoint;
use crate::route::protocol::Protocol;
use crate::route::{Framing, SharedTransport};
use crate::schema::{ContentPart, LLMRequest, Message, MessageRole};
use serde_json::{Map, Value};

/// v2's API version header (github-copilot.ts:13).
pub const COPILOT_API_VERSION: &str = "2026-08-01";
/// The Copilot API origin (github-copilot.ts `baseURL()`).
pub const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";

/// The static Copilot client headers (github-copilot.ts `applyHeaders`):
/// api version + intent + initiator; the per-request vision/interaction
/// metadata stays with [`copilot_request_headers`].
#[must_use]
pub fn copilot_client_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("X-GitHub-Api-Version".to_owned(), COPILOT_API_VERSION.to_owned()),
        ("Openai-Intent".to_owned(), "conversation-edits".to_owned()),
        ("x-initiator".to_owned(), "user".to_owned()),
    ])
}

/// The session request kinds that map onto Copilot's
/// `X-Interaction-Type` vocabulary (github-copilot.ts `interactionType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRequestKind {
    Chat,
    Title,
    Compaction,
}

/// `X-Interaction-Type` for one request (github-copilot.ts
/// `interactionType`): the agent loop is the default, nested sessions are
/// subagents, and title/compaction are the two utility overrides.
#[must_use]
pub fn interaction_type(kind: SessionRequestKind, child: bool) -> &'static str {
    match kind {
        SessionRequestKind::Title => "conversation-background",
        SessionRequestKind::Compaction => "conversation-compaction",
        SessionRequestKind::Chat if child => "conversation-subagent",
        SessionRequestKind::Chat => "conversation-agent",
    }
}

/// Per-request metadata derived from the lowered request
/// (github-copilot.ts `requestMetadata`): `agent` when the last turn is not
/// a plain user turn (the model continues from assistant/tool content — the
/// initiator can only escalate to agent, never back to user), `vision` when
/// any turn carries an image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestMetadata {
    pub agent: bool,
    pub vision: bool,
}

#[must_use]
pub fn request_metadata(request: &LLMRequest) -> RequestMetadata {
    let agent = match request.messages.last() {
        None => false,
        Some(message) => {
            let plain_user_content =
                message.content.iter().any(|part| !matches!(part, ContentPart::ToolResult { .. }));
            message.role != MessageRole::User || !plain_user_content
        }
    };
    let vision = request.messages.iter().any(|message| {
        message.content.iter().any(|part| {
            matches!(part, ContentPart::Media { media, .. } if media.media_type.starts_with("image/"))
        })
    });
    RequestMetadata { agent, vision }
}

/// The per-request Copilot headers (github-copilot.ts `applyHeaders` over
/// `requestMetadata`): the `x-initiator` escalation and the vision flag.
/// Client headers (api version, intent) ride the route defaults.
#[must_use]
pub fn copilot_request_headers(request: &LLMRequest) -> BTreeMap<String, String> {
    let metadata = request_metadata(request);
    let mut headers = BTreeMap::new();
    if metadata.agent {
        headers.insert("x-initiator".to_owned(), "agent".to_owned());
    }
    if metadata.vision {
        headers.insert("Copilot-Vision-Request".to_owned(), "true".to_owned());
    }
    headers
}

/// Copilot `reasoning_opaque` replay (ledger AI-COPILOT-C2; port of
/// `convert-to-openai-compatible-chat-messages.ts:69-124` assistant case):
/// the captured opaque handle from any prior part's
/// `{copilot: {reasoningOpaque}}` metadata re-threads as the assistant
/// message's `reasoning_opaque` — and `reasoning_text` rides along **only**
/// when the handle exists; without one neither field is replayed.
fn copilot_assistant_overlay(source: &Message, message: &mut Map<String, Value>) {
    let mut opaque: Option<String> = None;
    let mut reasoning_text = String::new();
    for part in &source.content {
        let part_opaque = part
            .provider_metadata()
            .and_then(|metadata| metadata.get("copilot"))
            .and_then(|object| object.get("reasoningOpaque"))
            .and_then(Value::as_str)
            .map(str::to_string);
        if part_opaque.is_some() && opaque.is_none() {
            opaque = part_opaque;
        }
        if let ContentPart::Reasoning { text, .. } = part {
            reasoning_text.push_str(text);
        }
    }
    if let Some(opaque) = opaque {
        message.insert("reasoning_opaque".to_string(), Value::String(opaque));
        message.insert("reasoning_text".to_string(), Value::String(reasoning_text));
    } else {
        // No captured handle: the Copilot chat wire carries neither field —
        // prior visible thinking is not replayed on its own (ref line 120:
        // `reasoning_text: reasoningOpaque ? reasoningText : undefined`).
        message.remove("reasoning_content");
        message.remove("reasoning_text");
    }
}

/// The Copilot chat protocol: the openai-compatible chat machine with the
/// Copilot provider id and the `reasoning_opaque` capture/replay ext
/// (AI-COPILOT-C2).
#[must_use]
pub fn copilot_chat_protocol() -> OpenAiChat {
    OpenAiChat::new(
        OpenAiChatExt::base()
            .with_id("copilot-chat")
            .with_reasoning_opaque()
            .with_assistant_overlay(copilot_assistant_overlay),
    )
}

/// Copilot route configuration: the resolved base URL (BE-3b's
/// `copilot_base_url`) and the bound token (`GITHUB_TOKEN` env fallback —
/// the catalog env list).
#[derive(Default)]
pub struct CopilotConfig {
    pub base_url: Option<String>,
    pub token: Option<Secret>,
}

/// The Copilot route: one deployment serving both Copilot API lanes — the
/// openai-compatible chat protocol for the base models and the `OpenAI`
/// Responses protocol for the responses-only families (GPT ≥ 5 except
/// `gpt-5-mini*`, Grok, Gemini, MAI Code; ledger AI-COPILOT-C1) — behind
/// the dynamic `/responses` vs `/chat/completions` endpoint. Bearer auth
/// over the bound token, the static client headers in the route defaults,
/// and the per-request header policy (`copilotFetch`'s header half) on the
/// route headers seam.
///
/// v2 picks the lane per model in the `aisdk.language` hook
/// (github-copilot.ts:317-344); the Rust facade binds every Copilot model
/// through this one factory, so the selection runs per request
/// ([`crate::protocols::copilot_responses`]).
#[must_use]
pub fn copilot_chat_route(transport: SharedTransport, config: CopilotConfig) -> Route {
    copilot_responses_route(transport, config)
}

/// The shared Copilot route construction: bearer auth over the bound token,
/// Copilot client headers, the per-request header seam, and the caller's
/// protocol/endpoint/framing (the chat-only and dispatch routes differ
/// exactly there).
#[must_use]
pub fn copilot_route_with_protocol(
    transport: SharedTransport,
    config: CopilotConfig,
    protocol: Arc<dyn Protocol>,
    endpoint: Endpoint,
    framing: Framing,
) -> Route {
    let base_url = config.base_url.unwrap_or_else(|| COPILOT_BASE_URL.to_string());
    let token = config.token.unwrap_or_else(|| Secret::Env("GITHUB_TOKEN".into()));
    let auth: Definition = auth::optional(Some(token)).bearer();
    Route::new(RouteInput {
        id: "copilot-chat".to_string(),
        provider: Some("github-copilot".to_string()),
        provider_metadata_key: Some("copilot".to_string()),
        protocol,
        endpoint: endpoint.with_base_url(base_url),
        auth: Some(auth),
        framing,
        headers: Some(Arc::new(copilot_request_headers)),
        transport,
        defaults: RouteDefaults { headers: Some(copilot_client_headers()), ..Default::default() },
    })
}

/// The chat-only Copilot route (the pre-C1 shape): the openai-compatible
/// chat protocol on the fixed `/chat/completions` path.
#[must_use]
pub fn copilot_chat_only_route(transport: SharedTransport, config: CopilotConfig) -> Route {
    copilot_route_with_protocol(
        transport,
        config,
        Arc::new(copilot_chat_protocol()),
        Endpoint::path(CHAT_PATH),
        Framing::sse_with_done(),
    )
}

#[cfg(test)]
mod tests;
