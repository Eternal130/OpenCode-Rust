//! GitHub Copilot responses lane (ledger `AI-COPILOT-C1`, cluster C7b):
//! the per-model endpoint selection between the `/responses` (`OpenAI`
//! Responses) and `/chat/completions` (openai-compatible chat) deployments
//! of the Copilot API.
//!
//! v2 selects the lane in the `aisdk.language` hook
//! (`packages/core/src/plugin/provider/github-copilot.ts:317-344`): the
//! advertised endpoint option (`settings.endpoint` from the dynamic
//! `/models` sync — `responses`/`chat`/`messages`) wins when set; otherwise
//! the model id decides — GPT ≥ 5 (except `gpt-5-mini*`), Grok, Gemini, and
//! MAI Code models are served **only** on `/responses`:
//!
//! ```text
//! const responses =
//!   (gpt !== null && Number(gpt[1]) >= 5 && !id.startsWith("gpt-5-mini")) ||
//!   id.startsWith("grok-") || id.startsWith("gemini-") ||
//!   id.startsWith("mai-code-")
//! ```
//!
//! The Rust route pipeline binds one route per provider
//! (`facade → copilot_chat_route → route.model(id)`), so the selection runs
//! per request: [`CopilotDispatchProtocol`] picks the sub-protocol from
//! `request.model.id` (+ the `endpoint` provider option), and the route's
//! endpoint path is dynamic over the same predicate. Both lanes share the
//! Copilot header policy and bearer transport of the enclosing route
//! (v2's single `copilotFetch` + `createOpenaiCompatible` provider).

use std::sync::Arc;

use serde_json::Value;

use crate::route::endpoint::Endpoint;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::SharedTransport;
use crate::route::{Framing, Route};
use crate::schema::{AiError, LLMEvent, LLMRequest};

use super::copilot::{copilot_chat_protocol, copilot_route_with_protocol, CopilotConfig};
use super::openai_responses::{OpenAIResponsesProtocol, PATH as RESPONSES_PATH};

/// The `/responses` sub-protocol terminal types (borrowed from the `OpenAI`
/// Responses machine for the dispatch terminal predicate).
const RESPONSES_TERMINAL_TYPES: [&str; 4] =
    ["error", "response.completed", "response.incomplete", "response.failed"];

/// The two Copilot API deployments (v2 `sdk.responses` vs `sdk.chat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotLane {
    /// `POST {base}/responses` — the `OpenAI` Responses protocol.
    Responses,
    /// `POST {base}/chat/completions` — the openai-compatible chat protocol.
    Chat,
}

/// The endpoint option key: model settings overlays (`settings.endpoint`
/// from the Copilot `/models` sync, `"responses" | "chat" | "messages"`)
/// land in the resolved request's provider options.
pub const ENDPOINT_OPTION_KEY: &str = "endpoint";

/// The advertised endpoint option for a resolved request, if any
/// (route/model defaults merge into `request.provider_options` before the
/// body build, so both catalog overlays and caller options are honored).
#[must_use]
pub fn endpoint_option(request: &LLMRequest) -> Option<&str> {
    let options = request.provider_options.as_ref()?;
    let value = options.get(ENDPOINT_OPTION_KEY)?;
    // `Schema.optional(Schema.String)` — a non-string (or null) overlay is
    // ignored rather than failing the request.
    value.as_str().filter(|value| !value.is_empty())
}

/// The model-id half of the selection (v2's regex, verbatim): GPT with
/// major version ≥ 5 — except the `gpt-5-mini*` chat-only family — plus the
/// `grok-`, `gemini-`, and `mai-code-` prefixes are responses-only.
#[must_use]
pub fn model_id_uses_responses(model_id: &str) -> bool {
    if let Some(rest) = model_id.strip_prefix("gpt-") {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            let major = digits.parse::<u64>().unwrap_or(0);
            return major >= 5 && !model_id.starts_with("gpt-5-mini");
        }
    }
    model_id.starts_with("grok-")
        || model_id.starts_with("gemini-")
        || model_id.starts_with("mai-code-")
}

/// The v2 lane selection: the advertised `endpoint` option wins whenever it
/// names a lane this provider serves (`responses`/`chat`); anything else
/// (`messages`, malformed, absent) falls through to the model-id predicate
/// (github-copilot.ts:325-342).
#[must_use]
pub fn copilot_lane(model_id: &str, endpoint: Option<&str>) -> CopilotLane {
    match endpoint {
        Some("responses") => CopilotLane::Responses,
        Some("chat") => CopilotLane::Chat,
        _ if model_id_uses_responses(model_id) => CopilotLane::Responses,
        _ => CopilotLane::Chat,
    }
}

/// The lane serving one resolved request.
#[must_use]
pub fn copilot_lane_for_request(request: &LLMRequest) -> CopilotLane {
    copilot_lane(request.model.id.as_str(), endpoint_option(request))
}

/// The wire path for a lane (the dynamic endpoint of the dispatch route).
#[must_use]
pub fn lane_path(lane: CopilotLane) -> &'static str {
    match lane {
        CopilotLane::Responses => RESPONSES_PATH,
        CopilotLane::Chat => super::openai_chat::PATH,
    }
}

/// Per-response dispatch state: the chosen lane plus the delegated parser
/// state of that lane's protocol.
struct CopilotDispatchState {
    lane: CopilotLane,
    inner: ProtocolState,
}

/// The Copilot dispatch protocol: one route-level protocol that serves both
/// Copilot deployments, delegating per request to the openai-compatible
/// chat protocol (with the Copilot `reasoning_opaque` capture/replay) or
/// the `OpenAI` Responses protocol (with the Copilot store/include defaults
/// carried by the route).
pub struct CopilotDispatchProtocol {
    chat: Arc<dyn Protocol>,
    responses: Arc<dyn Protocol>,
}

impl CopilotDispatchProtocol {
    #[must_use]
    pub fn new() -> Self {
        Self {
            chat: Arc::new(copilot_chat_protocol()),
            responses: Arc::new(OpenAIResponsesProtocol),
        }
    }

    fn lane_protocol(&self, lane: CopilotLane) -> &Arc<dyn Protocol> {
        match lane {
            CopilotLane::Chat => &self.chat,
            CopilotLane::Responses => &self.responses,
        }
    }
}

impl Default for CopilotDispatchProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl Protocol for CopilotDispatchProtocol {
    fn id(&self) -> &str {
        "copilot"
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        self.lane_protocol(copilot_lane_for_request(request)).body_from(request)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        let lane = copilot_lane_for_request(request);
        let inner = self.lane_protocol(lane).initial(request);
        Box::new(CopilotDispatchState { lane, inner })
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        // The shared `sseWithDone` framing hands the `[DONE]` sentinel to
        // the protocol; the chat lane stops on it and the responses lane
        // finished on its terminal event — both no-op here.
        if frame == "[DONE]" {
            return Ok((state, Vec::new()));
        }
        let state =
            *state.downcast::<CopilotDispatchState>().expect("CopilotDispatchProtocol state");
        let protocol = self.lane_protocol(state.lane);
        let (inner, events) = protocol.step(state.inner, frame)?;
        Ok((Box::new(CopilotDispatchState { lane: state.lane, inner }), events))
    }

    fn terminal(&self, frame: &str) -> bool {
        // The chat lane ends on `[DONE]`; the responses lane on its
        // `response.*` terminal events. Checking both is safe: a chat
        // chunk never carries a responses terminal `type`, and `[DONE]`
        // is not a JSON event frame.
        if frame == "[DONE]" {
            return true;
        }
        let kind = serde_json::from_str::<Value>(frame)
            .ok()
            .and_then(|event| event.get("type").and_then(Value::as_str).map(str::to_string));
        kind.is_some_and(|kind| RESPONSES_TERMINAL_TYPES.contains(&kind.as_str()))
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        let state =
            *state.downcast::<CopilotDispatchState>().expect("CopilotDispatchProtocol state");
        let protocol = self.lane_protocol(state.lane);
        let (inner, events) = protocol.on_halt(state.inner)?;
        Ok((Box::new(CopilotDispatchState { lane: state.lane, inner }), events))
    }

    fn supports_effort_updates(&self, request: &LLMRequest) -> bool {
        match copilot_lane_for_request(request) {
            CopilotLane::Chat => self.chat.supports_effort_updates(request),
            CopilotLane::Responses => self.responses.supports_effort_updates(request),
        }
    }
}

/// Build the Copilot dispatch route: the openai-compatible chat protocol
/// and the `OpenAI` Responses protocol behind one dynamic endpoint
/// (`/responses` vs `/chat/completions` per model), sharing the Copilot
/// bearer auth, client headers, and per-request header policy
/// (v2: one `createOpenaiCompatible` provider; `sdk.responses(id)` vs
/// `sdk.chat(id)` per model). The Responses lane rides the `OpenAI` defaults
/// (`store: false`, `include: ["reasoning.encrypted_content"]` — the
/// Copilot responses model forces the same include).
#[must_use]
pub fn copilot_responses_route(transport: SharedTransport, config: CopilotConfig) -> Route {
    copilot_route_with_protocol(
        transport,
        config,
        Arc::new(CopilotDispatchProtocol::new()),
        Endpoint::dynamic_path(|input: &crate::route::endpoint::EndpointInput<'_>| {
            lane_path(copilot_lane_for_request(input.request)).to_string()
        }),
        Framing::sse_with_done(),
    )
}
