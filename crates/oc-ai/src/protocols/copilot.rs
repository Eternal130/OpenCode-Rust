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
use crate::route::transport::{read_body_limited, TransportRequest};
use crate::route::{Framing, SharedTransport};
use crate::schema::{ContentPart, LLMRequest, Message, MessageRole};
use bytes::Bytes;
use serde_json::{Map, Value};

/// v2's API version header (github-copilot.ts:13).
pub const COPILOT_API_VERSION: &str = "2026-08-01";
/// The Copilot API origin (github-copilot.ts `baseURL()`).
pub const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";
/// The `/models` fetch budget (models.ts:84 `AbortSignal.timeout(5_000)`).
pub const COPILOT_MODELS_TIMEOUT_MS: u64 = 5_000;
/// The `/models` body read bound (the listing is a small JSON document).
pub const COPILOT_MODELS_BODY_LIMIT: usize = 16 * 1024 * 1024;

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

// ===========================================================================
// Dynamic /models sync (AI-P06; port of github-copilot/models.ts + the
// load() half of plugin/provider/github-copilot.ts:172-251)
// ===========================================================================

/// One decoded `/models` entry (port of `RemoteModel`): decode failures
/// drop the entry (TS `decodeUnknownOption`), so every field the usable
/// filter and derivation need is required here, optionals defaulted.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemoteModel {
    pub model_picker_enabled: bool,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub supported_endpoints: Vec<String>,
    #[serde(default)]
    pub policy: Option<CopilotRemotePolicy>,
    #[serde(default)]
    pub billing: Option<CopilotRemoteBilling>,
    pub capabilities: CopilotRemoteCapabilities,
}

/// `policy` (models.ts:14): only the state matters.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemotePolicy {
    #[serde(default)]
    pub state: Option<String>,
}

/// `capabilities.limits` (models.ts:34-45) — the usable filter keys off
/// the presence of the output/prompt limits.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemoteLimits {
    #[serde(default)]
    pub max_output_tokens: Option<f64>,
    #[serde(default)]
    pub max_prompt_tokens: Option<f64>,
    #[serde(default)]
    pub max_context_window_tokens: Option<f64>,
    /// `limits.vision` (models.ts:38-44) — only the media types matter.
    #[serde(default)]
    pub vision: Option<CopilotRemoteVision>,
}

/// `capabilities.limits.vision` (models.ts:38-44).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemoteVision {
    #[serde(default)]
    pub supported_media_types: Vec<String>,
}

/// `capabilities.supports` (models.ts:47-56) — only `tool_calls` gates
/// usability; the rest feed the derived fields.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemoteSupports {
    #[serde(default)]
    pub tool_calls: Option<bool>,
    #[serde(default)]
    pub vision: Option<bool>,
    #[serde(default)]
    pub adaptive_thinking: Option<bool>,
    #[serde(default)]
    pub max_thinking_budget: Option<f64>,
    #[serde(default)]
    pub reasoning_effort: Vec<String>,
}

/// `capabilities` (models.ts:31-57). `family` is required on the wire —
/// a listing without it fails TS decode and drops the entry, so it does
/// here too (`serde` decode failure → the loader skips the item).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemoteCapabilities {
    pub family: String,
    #[serde(default)]
    pub limits: Option<CopilotRemoteLimits>,
    pub supports: CopilotRemoteSupports,
}

/// `billing.token_prices.default` (models.ts:23-30) — the per-batch
/// AIC prices.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemoteDefaultPrices {
    #[serde(default)]
    pub input_price: Option<f64>,
    #[serde(default)]
    pub output_price: Option<f64>,
    /// The pre-2026-08-01 spelling (models.ts:24).
    #[serde(default)]
    pub cache_price: Option<f64>,
    /// API version 2026-08-01 renamed `cache_price` (models.ts:25).
    #[serde(default)]
    pub cache_read_price: Option<f64>,
}

/// `billing.token_prices` (models.ts:17-31): the AIC batch size plus
/// the default price tier.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemotePrices {
    #[serde(default)]
    pub batch_size: f64,
    #[serde(default, rename = "default")]
    pub default_prices: Option<CopilotRemoteDefaultPrices>,
}

/// `billing` (models.ts:16-32).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CopilotRemoteBilling {
    #[serde(default)]
    pub token_prices: Option<CopilotRemotePrices>,
}

impl CopilotRemoteModel {
    /// The usable filter (models.ts:120-127): not disabled, and the
    /// output/prompt limits plus the tool-call support are all
    /// advertised — a partial entry cannot create a broken model.
    #[must_use]
    pub fn usable(&self) -> bool {
        self.policy.as_ref().and_then(|policy| policy.state.as_deref()) != Some("disabled")
            && self.capabilities.limits.as_ref().and_then(|l| l.max_output_tokens).is_some()
            && self.capabilities.limits.as_ref().and_then(|l| l.max_prompt_tokens).is_some()
            && self.capabilities.supports.tool_calls.is_some()
    }
}

/// The advertised API endpoint for one model (models.ts:130-137): the
/// `settings.endpoint` overlay the Copilot dispatch lane reads
/// ([`super::copilot_responses`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotEndpoint {
    /// `/v1/messages` — the Anthropic lane (base URL gains `/v1`).
    Messages,
    /// `/responses` — the `OpenAI` Responses lane.
    Responses,
    /// `/chat/completions` — the openai-compatible chat lane.
    Chat,
}

impl CopilotEndpoint {
    /// The `settings.endpoint` value (the dispatch option vocabulary).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Responses => "responses",
            Self::Chat => "chat",
        }
    }
}

/// One reasoning variant of a remote model (models.ts `variants()`):
/// a variant id plus the settings overlay it merges.
#[derive(Debug, Clone, PartialEq)]
pub struct CopilotModelVariant {
    pub id: String,
    /// The raw settings overlay (a JSON object: `reasoningEffort`,
    /// `thinking`, `include`, `effort`, …).
    pub settings: Value,
}

/// The derived cost of a remote model (models.ts:180-193): USD per
/// million tokens after the AIC → USD conversion.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CopilotDerivedCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

/// The derived limits of a remote model (models.ts:196-200).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopilotDerivedLimit {
    pub context: i64,
    pub input: i64,
    pub output: i64,
}

/// The `package` a copilot model binds through (models.ts:164): the
/// Anthropic Messages lane for `/v1/messages` models, the copilot
/// ai-sdk package otherwise.
pub const COPILOT_PACKAGE_ANTHROPIC: &str = "@opencode/ai/providers/anthropic";
pub const COPILOT_PACKAGE_AISDK: &str = "@ai-sdk/github-copilot";

/// One derived model entry (models.ts `build`): the facts the caller
/// folds into its `Model.Info` — identity (`id`/`model_id`/`name`),
/// the endpoint selection and settings `baseURL` overlay, the package
/// lane, and the derived family/modalities/variants/released/cost/
/// limit fields.
#[derive(Debug, Clone, PartialEq)]
pub struct CopilotModelEntry {
    /// The listing id (an alias keeps its id, models.ts:103-108).
    pub id: String,
    /// The advertised API model id (`modelID`).
    pub model_id: String,
    pub name: String,
    /// `model_picker_enabled` (models.ts:189).
    pub enabled: bool,
    /// The selected endpoint, `None` when none is advertised
    /// (models.ts:130-137).
    pub endpoint: Option<CopilotEndpoint>,
    /// The settings `baseURL` overlay: `{base}/v1` for the messages
    /// lane, the Copilot root otherwise (models.ts:163-166).
    pub base_url: String,
    /// The package lane (models.ts:164): Anthropic Messages for the
    /// `/v1/messages` models — the copilot hook deliberately does not
    /// consume the `messages` endpoint option, the package rewrite is
    /// what steers Claude-on-Copilot through the standard anthropic
    /// machinery (github-copilot.ts:325-342).
    pub package: &'static str,
    /// The derived family (`capabilities.family`, models.ts:158).
    pub family: Option<String>,
    /// The derived input modalities (models.ts:139-143): text always,
    /// image/pdf from the vision capability.
    pub input: Vec<String>,
    /// The derived reasoning variants (models.ts `variants()`).
    pub variants: Vec<CopilotModelVariant>,
    /// The derived release time in epoch **milliseconds**
    /// (`Date.parse(version)`, models.ts:175-176; 0 when unparseable).
    pub released: f64,
    /// The derived cost (AIC → USD per million, models.ts:177-193).
    pub cost: CopilotDerivedCost,
    /// The derived limits (models.ts:195-199).
    pub limit: CopilotDerivedLimit,
}

impl CopilotModelEntry {
    /// Whether this entry binds through the Anthropic Messages lane.
    #[must_use]
    pub fn is_anthropic(&self) -> bool {
        self.package == COPILOT_PACKAGE_ANTHROPIC
    }
}

/// `Date.parse` for the version shapes GitHub advertises (models.ts:175):
/// `YYYY`, `YYYY-MM`, or `YYYY-MM-DD` → epoch milliseconds (UTC, like
/// the ISO-only fast path of the JS parser); anything else is `None`
/// (the caller then stores 0, mirroring `Number.isFinite` → 0).
fn date_parse_ms(input: &str) -> Option<f64> {
    let parse_num = |part: &str| -> Option<i64> { part.parse::<i64>().ok() };
    let days_from_civil = |y: i64, m: i64, d: i64| -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    };
    let parts: Vec<&str> = input.split('-').collect();
    match parts.as_slice() {
        [year] => {
            let year = parse_num(year)?;
            Some((days_from_civil(year, 1, 1) * 86_400_000) as f64)
        }
        [year, month] => {
            let (year, month) = (parse_num(year)?, parse_num(month)?);
            if !(1..=12).contains(&month) {
                return None;
            }
            Some((days_from_civil(year, month, 1) * 86_400_000) as f64)
        }
        [year, month, day] => {
            let (year, month, day) = (parse_num(year)?, parse_num(month)?, parse_num(day)?);
            if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                return None;
            }
            Some((days_from_civil(year, month, day) * 86_400_000) as f64)
        }
        _ => None,
    }
}

/// `load` (models.ts:81-94): GET `{baseURL}/models` under a 5s timeout
/// with the credential headers, keep only usable entries keyed by id.
/// Non-OK responses fail with the v2 message shape; the caller treats a
/// failure as "no refresh" (github-copilot.ts:196-199).
pub async fn load_copilot_models(
    transport: SharedTransport,
    base_url: &str,
    headers: Vec<(String, String)>,
) -> Result<BTreeMap<String, CopilotRemoteModel>, String> {
    let url = format!("{base_url}/models");
    let request = TransportRequest {
        method: "GET".to_string(),
        url: url.clone(),
        headers,
        body: Bytes::new(),
    };
    let response = tokio::time::timeout(
        std::time::Duration::from_millis(COPILOT_MODELS_TIMEOUT_MS),
        transport.execute(request),
    )
    .await
    .map_err(|_| "Failed to fetch Copilot models: timed out after 5s".to_string())?
    .map_err(|error| format!("Failed to fetch Copilot models: {}", error.message))?;
    if !(200..300).contains(&response.status) {
        // TS `!response.ok` (any non-2xx fails).
        return Err(format!("Failed to fetch Copilot models: {}", response.status));
    }
    let body = read_body_limited(response.body, COPILOT_MODELS_BODY_LIMIT)
        .await
        .map_err(|error| format!("Failed to fetch Copilot models: {}", error.message))?;
    let value: Value = serde_json::from_str(&body)
        .map_err(|error| format!("Failed to fetch Copilot models: {error}"))?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "Failed to fetch Copilot models: malformed response".to_string())?;
    let mut snapshot = BTreeMap::new();
    for raw in data {
        // Decode failures drop the entry (TS `decodeUnknownOption`).
        let Ok(model) = serde_json::from_value::<CopilotRemoteModel>(raw.clone()) else {
            continue;
        };
        if model.usable() {
            snapshot.insert(model.id.clone(), model);
        }
    }
    Ok(snapshot)
}

/// Locally kept facts a rebuild preserves (models.ts `build`'s
/// `previous`): the local alias keeps its display name, family, and
/// release time when the remote rebuild would lose them.
#[derive(Debug, Clone, Default)]
pub struct CopilotPrevious {
    pub name: Option<String>,
    pub family: Option<String>,
    /// Epoch milliseconds (kept verbatim, ref `previous?.time.released`).
    pub released: Option<f64>,
}

/// `derive` (models.ts:97-118): combine the remote snapshot with the
/// current listing — an existing entry survives only while its
/// advertised API model still exists (aliases keep their id, facts
/// rebuild from remote); every unseen remote model joins. Never derived
/// from a previously transformed result.
#[must_use]
pub fn derive_copilot_models(
    base_url: &str,
    remote: &BTreeMap<String, CopilotRemoteModel>,
    existing: &[(String, String)],
) -> Vec<CopilotModelEntry> {
    let mut derived = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (id, model_id) in existing {
        let Some(current) = remote.get(model_id) else {
            continue; // the advertised API model is gone → drop
        };
        derived.push(copilot_model_entry(id, current, base_url, CopilotPrevious::default()));
        seen.insert(id.clone());
    }
    for (id, model) in remote {
        if !seen.contains(id) {
            derived.push(copilot_model_entry(id, model, base_url, CopilotPrevious::default()));
        }
    }
    derived
}

/// `build` (models.ts:129-200), the full port: endpoint selection from
/// `supported_endpoints` (`/v1/messages` wins, then `/responses`, then
/// `/chat/completions`), the settings `baseURL` overlay, the package
/// lane (Anthropic for the messages endpoint), and the derived
/// family/modalities/variants/released/cost/limit fields.
#[must_use]
pub fn copilot_model_entry(
    id: &str,
    remote: &CopilotRemoteModel,
    base_url: &str,
    previous: CopilotPrevious,
) -> CopilotModelEntry {
    let endpoint = if remote.supported_endpoints.iter().any(|e| e == "/v1/messages") {
        Some(CopilotEndpoint::Messages)
    } else if remote.supported_endpoints.iter().any(|e| e == "/responses") {
        Some(CopilotEndpoint::Responses)
    } else if remote.supported_endpoints.iter().any(|e| e == "/chat/completions") {
        Some(CopilotEndpoint::Chat)
    } else {
        None
    };
    let messages = endpoint == Some(CopilotEndpoint::Messages);
    let base_url = if messages { format!("{base_url}/v1") } else { base_url.to_string() };

    // Modalities (models.ts:139-143): text always; image from the
    // vision flag OR any image/* media type; pdf only through the
    // vision media list.
    let media = remote
        .capabilities
        .limits
        .as_ref()
        .and_then(|limits| limits.vision.as_ref())
        .map_or([].as_slice(), |vision| vision.supported_media_types.as_slice());
    let image = remote.capabilities.supports.vision.unwrap_or(false)
        || media.iter().any(|kind| kind.starts_with("image/"));
    let pdf = remote.capabilities.supports.vision.unwrap_or(false)
        && media.iter().any(|kind| kind == "application/pdf");
    let mut input = vec!["text".to_string()];
    if image {
        input.push("image".to_string());
    }
    if pdf {
        input.push("pdf".to_string());
    }

    // Cost (models.ts:152-153, 177-193): Copilot reports AIC per
    // billing batch; OpenCode stores USD per million tokens.
    let prices = remote
        .billing
        .as_ref()
        .and_then(|billing| billing.token_prices.as_ref())
        .and_then(|prices| prices.default_prices.as_ref());
    let usd_per_million = if remote
        .billing
        .as_ref()
        .and_then(|billing| billing.token_prices.as_ref())
        .is_some_and(|prices| prices.batch_size > 0.0)
    {
        10_000.0
            / remote
                .billing
                .as_ref()
                .and_then(|billing| billing.token_prices.as_ref())
                .map(|prices| prices.batch_size)
                .unwrap_or_default()
    } else {
        0.0
    };
    let cost = CopilotDerivedCost {
        input: prices.and_then(|p| p.input_price).unwrap_or(0.0) * usd_per_million,
        output: prices.and_then(|p| p.output_price).unwrap_or(0.0) * usd_per_million,
        // `cache_read_price ?? cache_price ?? 0` (models.ts:188).
        cache_read: prices.and_then(|p| p.cache_read_price.or(p.cache_price)).unwrap_or(0.0)
            * usd_per_million,
    };

    // Released (models.ts:154-155, 175-176): a kept local release time
    // wins, else `Date.parse` of the version with the `{id}-` prefix
    // stripped; an unparseable version stores 0.
    let version =
        remote.version.strip_prefix(&format!("{}-", remote.id)).unwrap_or(remote.version.as_str());
    let released = previous
        .released
        .filter(|released| *released != 0.0)
        .or_else(|| date_parse_ms(version))
        .unwrap_or(0.0);

    // Limits (models.ts:195-199): the usable filter guarantees the
    // prompt/output pair; the context window falls back to the prompt
    // limit.
    let limits = remote.capabilities.limits.as_ref();
    let prompt_tokens = limits.and_then(|l| l.max_prompt_tokens).unwrap_or_default();
    let limit = CopilotDerivedLimit {
        context: limits.and_then(|l| l.max_context_window_tokens).unwrap_or(prompt_tokens) as i64,
        input: prompt_tokens as i64,
        output: limits.and_then(|l| l.max_output_tokens).unwrap_or_default() as i64,
    };

    CopilotModelEntry {
        id: id.to_string(),
        model_id: remote.id.clone(),
        name: previous.name.unwrap_or_else(|| remote.name.clone()),
        enabled: remote.model_picker_enabled,
        endpoint,
        base_url,
        package: if messages { COPILOT_PACKAGE_ANTHROPIC } else { COPILOT_PACKAGE_AISDK },
        family: previous.family.or(Some(remote.capabilities.family.clone())),
        input,
        variants: copilot_variants(remote, messages),
        released,
        cost,
        limit,
    }
}

/// `variants` (models.ts:202-236): reasoning-effort variants for the
/// non-messages lanes, adaptive-thinking variants when the model
/// supports both, else the max/high thinking-budget pair.
fn copilot_variants(remote: &CopilotRemoteModel, messages: bool) -> Vec<CopilotModelVariant> {
    let efforts = &remote.capabilities.supports.reasoning_effort;
    if !messages && !efforts.is_empty() {
        return efforts
            .iter()
            .map(|effort| CopilotModelVariant {
                id: effort.clone(),
                settings: serde_json::json!({
                    "reasoningEffort": effort,
                    "reasoningSummary": "auto",
                    "include": ["reasoning.encrypted_content"],
                }),
            })
            .collect();
    }
    if !efforts.is_empty() && remote.capabilities.supports.adaptive_thinking.unwrap_or(false) {
        return efforts
            .iter()
            .map(|effort| CopilotModelVariant {
                id: effort.clone(),
                settings: serde_json::json!({
                    "thinking": { "type": "adaptive", "display": "summarized" },
                    "effort": effort,
                }),
            })
            .collect();
    }
    let max = remote.capabilities.supports.max_thinking_budget;
    let Some(max) = max else { return Vec::new() };
    vec![
        CopilotModelVariant {
            id: "max".to_string(),
            settings: serde_json::json!({ "thinking": { "type": "enabled", "budgetTokens": max - 1.0 } }),
        },
        CopilotModelVariant {
            id: "high".to_string(),
            settings: serde_json::json!({
                "thinking": { "type": "enabled", "budgetTokens": (max / 2.0).floor() },
            }),
        },
    ]
}

// ===========================================================================
// The free utility-title channel (port of github-copilot.ts:398-475): a
// stateless, non-streaming `/chat/completions` call against the small
// utility model — session naming is a free, rate-limited utility
// scenario per GitHub's integration guide.
// ===========================================================================

/// The "small utility" models (github-copilot.ts:399-401): gpt-4o-mini
/// is the Copilot client's; when an account lacks it the caller falls
/// through to the regular billable title path instead of guessing.
pub const UTILITY_TITLE_MODELS: [&str; 1] = ["gpt-4o-mini"];

/// The utility-title budget (github-copilot.ts:470 `Effect.timeout("20
/// seconds")`).
pub const UTILITY_TITLE_TIMEOUT_MS: u64 = 20_000;

/// One utility-title conversation turn (role + flattened text).
#[derive(Debug, Clone)]
pub struct UtilityTitleMessage {
    pub role: String,
    pub text: String,
}

/// The utility-title request (github-copilot.ts:404-475): its own
/// `/chat/completions` call instead of steering core's title request —
/// the endpoint is fixed before the http hooks run and gpt-4o-mini
/// only serves `/chat/completions`. Stateless, non-streaming, free.
#[derive(Debug, Clone)]
pub struct UtilityTitleInput<'a> {
    pub base_url: &'a str,
    pub token: &'a str,
    pub model: &'a str,
    /// `App.useragent` stamp.
    pub user_agent: &'a str,
    /// `X-Interaction-Id` — the session the title names.
    pub session_id: &'a str,
    /// The flattened system prompt, when the request carried one.
    pub system: Option<&'a str>,
    pub messages: &'a [UtilityTitleMessage],
    /// `options.maxTokens`, when the request carried one.
    pub max_tokens: Option<u64>,
}

/// Build the utility-title wire body (github-copilot.ts:414-422): the
/// system turn first, then the flattened conversation, `stream: false`
/// and the optional `max_tokens`.
#[must_use]
pub fn utility_title_body(input: &UtilityTitleInput<'_>) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = input.system {
        messages.push(serde_json::json!({ "role": "system", "content": system }));
    }
    for message in input.messages {
        messages.push(serde_json::json!({
            "role": message.role,
            "content": message.text,
        }));
    }
    let mut body = serde_json::json!({
        "model": input.model,
        "messages": messages,
        "stream": false,
    });
    if let Some(max_tokens) = input.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    body
}

/// The utility-title headers (github-copilot.ts:427-437): bearer auth,
/// the api version/intent client headers, and the utility interaction
/// vocabulary (`agent-session-name-generation`, initiator `agent`).
#[must_use]
pub fn utility_title_headers(input: &UtilityTitleInput<'_>) -> Vec<(String, String)> {
    vec![
        ("Authorization".to_owned(), format!("Bearer {}", input.token)),
        ("Content-Type".to_owned(), "application/json".to_owned()),
        ("User-Agent".to_owned(), input.user_agent.to_string()),
        ("X-GitHub-Api-Version".to_owned(), COPILOT_API_VERSION.to_owned()),
        ("Openai-Intent".to_owned(), "conversation-edits".to_owned()),
        ("X-Interaction-Type".to_owned(), "agent-session-name-generation".to_owned()),
        ("X-Interaction-Id".to_owned(), input.session_id.to_string()),
        ("x-initiator".to_owned(), "agent".to_owned()),
    ]
}

/// Extract the title from a utility completion response
/// (github-copilot.ts:472-476): `choices[0].message.content`, trimmed;
/// an empty (or missing) content is a failure so the caller's normal
/// title path still runs.
pub fn utility_title_from_response(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| format!("Utility title response was malformed: {error}"))?;
    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if content.is_empty() {
        return Err("Utility title response was empty".to_string());
    }
    Ok(content)
}

/// Issue the utility-title request (github-copilot.ts:404-475): POST
/// `{baseURL}/chat/completions` under a 20s timeout; any failure is an
/// `Err` the hook turns into "result unset" so the billable title path
/// still runs.
pub async fn copilot_utility_title(
    transport: SharedTransport,
    input: &UtilityTitleInput<'_>,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", input.base_url);
    let request = TransportRequest {
        method: "POST".to_string(),
        url: url.clone(),
        headers: utility_title_headers(input),
        body: Bytes::from(utility_title_body(input).to_string()),
    };
    let response = tokio::time::timeout(
        std::time::Duration::from_millis(UTILITY_TITLE_TIMEOUT_MS),
        transport.execute(request),
    )
    .await
    .map_err(|_| "Utility title request timed out after 20s".to_string())?
    .map_err(|error| format!("Utility title request failed: {}", error.message))?;
    if !(200..300).contains(&response.status) {
        return Err(format!("Utility title request failed: {}", response.status));
    }
    let body = read_body_limited(response.body, COPILOT_MODELS_BODY_LIMIT)
        .await
        .map_err(|error| format!("Utility title request failed: {}", error.message))?;
    utility_title_from_response(&body)
}

#[cfg(test)]
mod tests;
