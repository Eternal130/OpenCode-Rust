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

use crate::protocols::openai_chat::{OpenAiChat, OpenAiChatExt, PATH as CHAT_PATH};
use crate::route::auth::{self, Definition, Secret};
use crate::route::client::{Route, RouteDefaults, RouteInput};
use crate::route::endpoint::Endpoint;
use crate::route::{Framing, SharedTransport};
use crate::schema::{ContentPart, LLMRequest, MessageRole};

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

/// Copilot route configuration: the resolved base URL (BE-3b's
/// `copilot_base_url`) and the bound token (`GITHUB_TOKEN` env fallback —
/// the catalog env list).
#[derive(Default)]
pub struct CopilotConfig {
    pub base_url: Option<String>,
    pub token: Option<Secret>,
}

/// The Copilot chat route: the openai-compatible chat protocol re-badged
/// for Copilot with bearer auth over the bound token, the static client
/// headers in the route defaults, and the per-request header policy
/// (`copilotFetch`'s header half) on the route headers seam
/// (`createOpenaiCompatible` + `evt.options.fetch = copilotFetch(...)`).
#[must_use]
pub fn copilot_chat_route(transport: SharedTransport, config: CopilotConfig) -> Route {
    let base_url = config.base_url.unwrap_or_else(|| COPILOT_BASE_URL.to_string());
    let token = config.token.unwrap_or_else(|| Secret::Env("GITHUB_TOKEN".into()));
    let auth: Definition = auth::optional(Some(token)).bearer();
    Route::new(RouteInput {
        id: "copilot-chat".to_string(),
        provider: Some("github-copilot".to_string()),
        provider_metadata_key: Some("copilot".to_string()),
        protocol: Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
        endpoint: Endpoint::path(CHAT_PATH).with_base_url(base_url),
        auth: Some(auth),
        framing: Framing::sse_with_done(),
        headers: Some(Arc::new(copilot_request_headers)),
        transport,
        defaults: RouteDefaults { headers: Some(copilot_client_headers()), ..Default::default() },
    })
}

#[cfg(test)]
mod tests;
