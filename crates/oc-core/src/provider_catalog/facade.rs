//! Provider facade binding: catalog provider → oc-ai route factory (port of
//! the binding half of `packages/core/src/provider.ts` + ai-map §4-5).
//!
//! Maps the resolved package (native entrypoint from the catalog) onto the
//! ported oc-ai protocols:
//!
//! - `anthropic`-family → `anthropic_messages_route` (x-api-key auth for
//!   Anthropic, bearer for the messages-family hosts),
//! - `openai`-family responses → `openai_responses_route`,
//! - chat-family packages → the ported chat factories
//!   (`openai_chat_route`, `deepseek/groq/openrouter/venice/zai/alibaba/
//!   mistral/cohere` variants) with the catalog base URL,
//! - `google` → the Gemini route (`google_route`),
//! - `openai-compatible` → `openai_compatible_chat_configured` — the seam
//!   for the ~25 catalog-only providers and user-defined custom providers,
//! - Bedrock (BE-4): the plain package → the Converse route (SigV4 or
//!   bearer auth, AWS event-stream framing); the Mantle split → the
//!   `OpenAI` chat / open-responses protocols on the mantle base URL,
//! - Vertex native (BE-4) → the aiplatform deployment of the Gemini
//!   protocol (project/location routing, tuned endpoints, ADC bearer),
//! - `github-copilot` (BE-4) → the Copilot chat route (openai-compatible
//!   protocol + the Copilot client/request header policy),
//! - everything whose protocol is not ported (the `aisdk:` shims) → the
//!   typed [`ProviderError::UnsupportedProtocol`] clean error (oracle gate
//!   3, Task 5 acceptance).

#![allow(
    clippy::doc_markdown, // package/provider identifiers in prose stay unquoted
    clippy::too_many_lines, // build_route mirrors the v2 family dispatch table
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use oc_ai::protocols::alibaba_responses::{alibaba_responses_route, AlibabaResponsesRouteSpec};
use oc_ai::protocols::meta_responses::{
    meta_auth, meta_responses_route, MetaResponsesRouteSpec,
};
use oc_ai::protocols::anthropic_messages::{anthropic_messages_route, anthropic_route};
use oc_ai::protocols::bedrock::{self as bedrock_lane, BedrockAuthOptions, MantleMessagesConfig};
use oc_ai::protocols::cohere_chat::{cohere_route_for_package, CohereConfig};
use oc_ai::protocols::copilot::{copilot_chat_route, CopilotConfig};
use oc_ai::protocols::gemini::{google_route, GoogleConfig};
use oc_ai::protocols::google_interactions::{google_interactions_route, GoogleInteractionsConfig};
use oc_ai::protocols::mistral_chat::{mistral_route_for_package, MistralConfig};
use oc_ai::protocols::openai_chat::{
    alibaba_chat_route, cerebras_chat_compatibility, deepinfra_chat_compatibility,
    deepseek_chat_route, digitalocean_chat_route, digitalocean_chat_compatibility,
    fireworks_chat_compatibility, groq_chat_route, openai_chat_route, openrouter_chat_route,
    togetherai_chat_compatibility, venice_chat_route, zai_chat_route,
};
use oc_ai::protocols::openai_compatible::{
    openai_compatible_chat_configured, openai_compatible_responses_route,
};
use oc_ai::protocols::openai_responses::{openai_responses_route, OpenAIResponsesRouteSpec};
use oc_ai::protocols::vertex::{
    vertex_chat_route, vertex_responses_route, vertex_route_for_model, VertexConfig,
    VertexEndpointConfig,
};
use oc_ai::protocols::vertex_messages::{vertex_messages_route, VertexMessagesConfig};
use oc_ai::protocols::vercel_gateway::{
    route_for_model as vercel_gateway_route_for_model, VercelGatewayRouteSpec,
};
use oc_ai::protocols::xai_responses::{xai_auth, xai_responses_route, XaiResponsesRouteSpec};
use oc_ai::route::auth::{self, Secret};
use oc_ai::route::client::{Route, RouteDefaults, RoutePatch};
use oc_ai::route::endpoint::EndpointPatch;
use oc_ai::route::{AuthDefinition, SharedTransport};
use oc_ai::schema::options::{
    merge_json_records, merge_string_records, HttpOptions, LanguageModelCompatibility,
    MaxTokensFieldCompatibility,
};
use oc_ai::LanguageModel;
use oc_schema::model::Info as ModelInfo;
use oc_schema::provider::{Activation, Settings as ProviderSettings};

use crate::provider_catalog::native::{self, package_name};
use crate::provider_catalog::plugin_headers::plugin_provider_headers;
use crate::provider_catalog::{Catalog, Snapshot};

/// Typed provider-binding failures (gate 3, Task 5: selecting a
/// catalog-listed but unported provider must fail cleanly).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProviderError {
    /// The provider's protocol is not ported to oc-ai (the `aisdk:` shims
    /// and any future package without a native binding).
    #[error("provider `{provider}` uses package `{package}`, which has no ported protocol (yet)")]
    UnsupportedProtocol { provider: String, package: String },
    /// No such provider in the catalog.
    #[error("unknown provider `{0}`")]
    UnknownProvider(String),
    /// No such model for the provider.
    #[error("unknown model `{1}` for provider `{0}`")]
    UnknownModel(String, String),
    /// The provider exists but cannot be configured (e.g. an
    /// openai-compatible provider without a base URL).
    #[error("provider `{provider}` is misconfigured: {message}")]
    Configuration { provider: String, message: String },
}

/// Execution context for facade binding: the transport every route runs on
/// (the process-agnostic seam, oracle gate 2 finding 7), plus the
/// integration host consulted by the provider header plugins (the
/// llmgateway gate; `None` = no integrations registered, AUD-10②).
pub struct FacadeContext {
    pub transport: SharedTransport,
    pub integrations: Option<Arc<crate::integration::IntegrationHost>>,
}

impl FacadeContext {
    /// Context over one shared transport (no integrations).
    pub fn new(transport: SharedTransport) -> Self {
        Self { transport, integrations: None }
    }

    /// Attach the integration host (the llmgateway header gate).
    #[must_use]
    pub fn with_integrations(mut self, host: Arc<crate::integration::IntegrationHost>) -> Self {
        self.integrations = Some(host);
        self
    }

    fn integration_registered(&self, provider_id: &str) -> bool {
        self.integrations.as_ref().is_some_and(|host| host.has_integration(provider_id))
    }
}

/// User connection settings for one provider (from `opencode.json`
/// `provider.<id>`): explicit values win; env fallback comes from the
/// catalog's `environment` list and is resolved lazily by the route auth.
#[derive(Debug, Clone, Default)]
pub struct ModelConnection {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub headers: BTreeMap<String, String>,
}

impl ModelConnection {
    /// Extract `baseURL` / `apiKey` from schema provider settings (the
    /// arbitrary-keys `rest` record).
    #[must_use]
    pub fn from_settings(settings: &ProviderSettings) -> Self {
        let string = |key: &str| settings.rest.get(key).and_then(Value::as_str).map(str::to_string);
        Self { base_url: string("baseURL"), api_key: string("apiKey"), headers: BTreeMap::new() }
    }

    fn secret(&self) -> Option<Secret> {
        self.api_key.clone().map(Secret::Value)
    }
}

/// Partial capabilities overlay (`Config.Model.Capabilities`,
/// schema config/provider.ts:76-83): unset fields fall back to the
/// base model's capabilities, then the defaults
/// `{tools:true, input:["text","image"], output:["text"]}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PartialCapabilities {
    pub tools: Option<bool>,
    pub input: Option<Vec<String>>,
    pub output: Option<Vec<String>>,
}

/// Partial limit overlay (`Config.Model.Limit`, schema
/// config/provider.ts:63-67): unset fields keep the base model's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct PartialModelLimit {
    pub context: Option<i64>,
    pub input: Option<i64>,
    pub output: Option<i64>,
}

/// One config-defined model entry (`providers.<id>.models.<model>`),
/// keeping the per-model overlays v2 merges onto the effective model
/// (core/src/model.ts:204-209 — provider overlays first, model overlays
/// win per key) plus the console merge material (opencode.ts:263-306):
/// partial capabilities, replacement cost, per-field limit, and the
/// model-level `settings.baseURL` overlay.
#[derive(Debug, Clone, Default)]
pub struct CustomModel {
    pub id: String,
    /// The config display name (`models.<id>.name`,
    /// config/plugin/provider.ts:112).
    pub name: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub body: Option<BTreeMap<String, Value>>,
    /// `models.<id>.modelID` — the catalog base-model key
    /// (opencode.ts:264 `config.modelID ?? modelID`).
    pub model_id: Option<String>,
    /// `models.<id>.family` (opencode.ts:267).
    pub family: Option<String>,
    /// `models.<id>.compatibility` — shallow-spread merged over the
    /// base (opencode.ts:270-271).
    pub compatibility: Option<oc_schema::model::Compatibility>,
    /// `models.<id>.capabilities` — per-field merged over the base
    /// (opencode.ts:274-275, model.ts:32-39).
    pub capabilities: Option<PartialCapabilities>,
    /// `models.<id>.cost` — REPLACES the base cost wholesale, already
    /// normalized single-or-array → array with cache read/write
    /// defaulting to 0.0 (opencode.ts:294-303).
    pub cost: Option<Vec<oc_schema::model::Cost>>,
    /// `models.<id>.limit` — per-field override (opencode.ts:305).
    pub limit: Option<PartialModelLimit>,
    /// `models.<id>.settings` (config/provider.ts:120; console
    /// opencode.ts:277-280) — the model-level settings overlay beyond
    /// `baseURL`, credential-stripped at projection
    /// (`withoutCredentials(config.settings)`, opencode.ts:279). The
    /// binding lanes feed it into native provider options via
    /// `native_settings` (CORE_KEYS strip at bind, provider.ts:141-151).
    pub settings: Option<oc_schema::model::Settings>,
    /// The model-level `settings.baseURL` overlay (opencode.ts:276-279):
    /// outranks the provider-level endpoint at bind.
    pub base_url: Option<String>,
    /// `models.<id>.package` (config/provider.ts:83) — overrides the
    /// base model's package; when absent a PROVIDER-level package
    /// clears the base model package instead (opencode.ts:272).
    pub package: Option<String>,
    /// `models.<id>.variants` (config/provider.ts:85-89) — `{id,
    /// settings, headers, body}` entries merged per id over the base
    /// model's variants (opencode.ts:282-293); variant settings are
    /// already credential-stripped at projection
    /// (`withoutCredentials(variant.settings)`, opencode.ts:289).
    pub variants: Vec<CustomModelVariant>,
}

/// `Config.Model`'s variant entry (config/provider.ts:85-89): a
/// `VariantID` plus the model overlay triple.
#[derive(Debug, Clone, Default)]
pub struct CustomModelVariant {
    pub id: String,
    /// Model overlay settings (`compaction` + arbitrary rest).
    pub settings: Option<oc_schema::model::Settings>,
    pub headers: Option<BTreeMap<String, String>>,
    pub body: Option<BTreeMap<String, Value>>,
}

/// A user-defined custom provider (v2 `provider` config custom entries):
/// always bound through the openai-compatible facade.
#[derive(Debug, Clone, Default)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    /// The provider-level `settings.baseURL` endpoint; empty when a
    /// model-level overlay or a catalog base supplies the endpoint.
    pub base_url: String,
    pub api_key: Option<String>,
    /// Provider-level overlays (`providers.<id>.headers|body`) — merged
    /// under every model's own overlays.
    pub headers: Option<BTreeMap<String, String>>,
    pub body: Option<BTreeMap<String, Value>>,
    pub models: Vec<CustomModel>,
    /// Console `canonical` (opencode.ts:247,252): the catalog provider
    /// the base models resolve against.
    pub canonical: Option<String>,
    /// Console `package` (config/provider.ts:97; legacy `type` accepted)
    /// (opencode.ts:254). Decoded but UNCONSUMED until the native-
    /// protocol-binding lane: this lane binds console providers through
    /// the openai-compatible facade, so the package's only observable
    /// effect today is clearing the merged Info's model-level package
    /// (opencode.ts:272, applied in `custom_model_info`).
    pub package: Option<String>,
}

/// The ported protocol families a catalog package can bind to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolFamily {
    AnthropicMessages,
    OpenAIChat,
    OpenAIResponses,
    Gemini,
    OpenAICompatible,
    /// Cohere native v2 chat (`cohere_route_for_package` native lane)
    /// plus the `/chat` OpenAI-compatibility lane.
    CohereChat,
    /// Mistral native chat-completions (`mistral_route_for_package`).
    MistralChat,
    /// Bedrock Converse (SigV4/bearer + AWS event-stream framing).
    BedrockConverse,
    /// Bedrock Mantle: the OpenAI chat protocol on `bedrock-mantle`.
    BedrockMantleChat,
    /// Bedrock Mantle: the open-responses protocol on `bedrock-mantle`.
    BedrockMantleResponses,
    /// Bedrock Mantle: the Anthropic Messages protocol on the mantle
    /// `anthropic/v1` root (`bedrock-mantle-messages`).
    BedrockMantleMessages,
    /// Vertex native: Gemini generateContent on aiplatform.
    VertexGemini,
    /// Vertex Claude Messages (AI-VERTEX-V2): the Anthropic Messages
    /// protocol on the publishers/anthropic Vertex root with ADC auth —
    /// previously misrouted to the generic messages arm (api.anthropic.com).
    VertexMessages,
    /// Vertex OpenAI Chat (AI-V1): the openai-chat protocol on the
    /// aiplatform `endpoints/openapi` root with ADC auth — previously the
    /// generic chat arm (which failed "needs a baseURL" without one).
    VertexChat,
    /// Vertex Open Responses (AI-V1): the open-responses protocol on the
    /// aiplatform `endpoints/openapi` root with ADC auth — previously no
    /// family arm at all (UnsupportedProtocol).
    VertexResponses,
    /// xAI Responses (AI-M5): the Open Responses baseline with the Grok
    /// hosted tool vocabulary and explicit compaction-only context
    /// management.
    XaiResponses,
    /// Google Interactions (AI-M3): the Gemini Interactions lane on
    /// generativelanguage — thought-signature stateless replay and
    /// `previous_interaction_id` continuation.
    GoogleInteractions,
    /// GitHub Copilot: openai-compatible chat + the Copilot header policy.
    CopilotChat,
    /// Alibaba Responses (G-84): the Open Responses baseline with the Model
    /// Studio dialect (`enable_thinking` / `previous_response_id` /
    /// `conversation`, named tool_choice → `allowed_tools`, the
    /// web_search / web_extractor / code_interpreter hosted tools).
    AlibabaResponses,
    /// Meta Responses (G-84 sibling): the Open Responses baseline with the
    /// `native.meta` hosted tool vocabulary (web_search /
    /// image_generation), the signed-ID replay transform for
    /// provider-executed image results, and terminal-only item recovery.
    MetaResponses,
    /// Vercel AI Gateway (G-81): Messages / Responses / Chat routes wrapped
    /// by the gateway protocol seam (`providerOptions.gateway` body
    /// injection + gateway usage/cost metadata on finish events).
    VercelGateway,
}

/// Classify a native package into a ported family (`None` → unsupported).
#[must_use]
pub fn family_of(package: &str) -> Option<ProtocolFamily> {
    Some(match package {
        "@opencode/ai/providers/anthropic"
        | "@opencode/ai/providers/alibaba/messages"
        | "@opencode/ai/providers/meta/messages"
        | "@opencode/ai/providers/minimax/messages"
        | "@opencode/ai/providers/moonshot/messages"
        | "@opencode/ai/providers/zai-coding-plan/messages" => ProtocolFamily::AnthropicMessages,

        // Vertex Claude Messages (AI-VERTEX-V2): its own native lane —
        // the publishers/anthropic Vertex URL + ADC bearer, NOT the
        // generic messages arm (providers/google-vertex-messages.ts).
        "@opencode/ai/providers/google-vertex/messages" => ProtocolFamily::VertexMessages,

        "@opencode/ai/providers/openai"
        | "@opencode/ai/providers/openai/responses"
        | "@opencode/ai/providers/azure"
        | "@opencode/ai/providers/azure/responses"
        | "@opencode/ai/providers/minimax/responses"
        | "@opencode/ai/providers/moonshot/responses"
        | "@opencode/ai/providers/zai-coding-plan/responses" => ProtocolFamily::OpenAIResponses,

        // Alibaba Responses (G-84): its own native lane — the Model Studio
        // responses dialect, NOT the plain OpenAI Responses family
        // (providers/alibaba.ts responsesRoute).
        "@opencode/ai/providers/alibaba/responses" => ProtocolFamily::AlibabaResponses,

        // Meta Responses (G-84 sibling): its own native lane — the Meta
        // responses dialect with `native.meta` hosted tools and the
        // signed-ID image replay, NOT the plain OpenAI Responses family
        // (providers/meta.ts responsesRoute).
        "@opencode/ai/providers/meta/responses" => ProtocolFamily::MetaResponses,

        // xAI (AI-M5): the native `xai-responses` protocol — the Open
        // Responses baseline with the Grok hosted tool vocabulary and the
        // `xai` metadata namespace (previously misrouted to the plain
        // OpenAI Responses family).
        "@opencode/ai/providers/xai" => ProtocolFamily::XaiResponses,

        // `openai/chat` (builtin-table reconciliation): the chat-model
        // re-export of the openai package (providers/openai/chat.ts:1)
        // — the OpenAI chat protocol at the baked api.openai.com base.
        "@opencode/ai/providers/openai/chat"
        | "@opencode/ai/providers/azure/chat"
        | "@opencode/ai/providers/alibaba/chat"
        | "@opencode/ai/providers/baseten"
        | "@opencode/ai/providers/cerebras"
        | "@opencode/ai/providers/deepinfra"
        | "@opencode/ai/providers/deepseek"
        | "@opencode/ai/providers/digitalocean"
        | "@opencode/ai/providers/fireworks"
        | "@opencode/ai/providers/groq"
        | "@opencode/ai/providers/meta/chat"
        | "@opencode/ai/providers/minimax/chat"
        | "@opencode/ai/providers/moonshot/chat"
        | "@opencode/ai/providers/togetherai"
        | "@opencode/ai/providers/venice"
        | "@opencode/ai/providers/zai/chat"
        | "@opencode/ai/providers/zai-coding-plan/chat"
        | "@opencode/ai/providers/openrouter"
        | "@opencode/ai/providers/cloudflare-workers-ai"
        | "@opencode/ai/providers/cloudflare-ai-gateway" => ProtocolFamily::OpenAIChat,

        // Vercel AI Gateway (G-81): its own native lane — per-model dispatch
        // over the gateway-wrapped Messages/Responses/Chat routes
        // (providers/vercel-ai-gateway.ts:205-229, 272-277).
        "@opencode/ai/providers/vercel-ai-gateway" | "@ai-sdk/gateway" => {
            ProtocolFamily::VercelGateway
        }

        // Cohere (AI-COHERE-MISTRAL): BOTH the native package and the
        // `/chat` OpenAI-compatibility lane route through the landed
        // oc-ai seam (`cohere_route_for_package`) — the compat lane is
        // the OpenAI chat protocol at the Cohere COMPATIBILITY_BASE_URL,
        // not a generic chat binding (providers/cohere.ts:21-44).
        "@opencode/ai/providers/cohere" | "@opencode/ai/providers/cohere/chat" => {
            ProtocolFamily::CohereChat
        }
        // Mistral (AI-COHERE-MISTRAL): the native mistral-chat protocol
        // (providers/mistral.ts:23-30) — previously misrouted to
        // OpenAIChat.
        "@opencode/ai/providers/mistral" => ProtocolFamily::MistralChat,

        "@opencode/ai/providers/google" => ProtocolFamily::Gemini,
        // Google Interactions (AI-M3): the pinned interactions package —
        // the native `google-interactions` protocol (provider.ts:77
        // registry entry, model-resolver.ts:458 auth list).
        "@opencode/ai/providers/google/interactions" => ProtocolFamily::GoogleInteractions,
        // Vertex native (BE-4): the Gemini protocol deployed on aiplatform.
        // `google-vertex/gemini` is the gemini-model re-export of the
        // same package (providers/google-vertex/gemini.ts:1).
        "@opencode/ai/providers/google-vertex" | "@opencode/ai/providers/google-vertex/gemini" => {
            ProtocolFamily::VertexGemini
        }
        // Vertex OpenAPI lanes (AI-V1): chat and responses on the
        // aiplatform `endpoints/openapi` root (providers/
        // google-vertex-chat.ts + google-vertex-responses.ts).
        "@opencode/ai/providers/google-vertex/chat" => ProtocolFamily::VertexChat,
        "@opencode/ai/providers/google-vertex/responses" => ProtocolFamily::VertexResponses,
        "@opencode/ai/providers/openai-compatible" => ProtocolFamily::OpenAICompatible,

        // Bedrock (BE-4): Converse for the plain package, the Mantle
        // OpenAI-chat / open-responses deployments for the mantle split
        // (native.rs `mantle()` — gpt-oss models chat, the rest responses).
        "@opencode/ai/providers/amazon-bedrock"
        | "@opencode/ai/providers/amazon-bedrock/mantle" => ProtocolFamily::BedrockConverse,
        "@opencode/ai/providers/amazon-bedrock/mantle/chat" => ProtocolFamily::BedrockMantleChat,
        "@opencode/ai/providers/amazon-bedrock/mantle/responses" => {
            ProtocolFamily::BedrockMantleResponses
        }
        "@opencode/ai/providers/amazon-bedrock/mantle/messages" => {
            ProtocolFamily::BedrockMantleMessages
        }

        _ => return None,
    })
}

/// Bind `provider_id/model_id` from the catalog onto a configured oc-ai
/// [`LanguageModel`] (the v2 `provider.configure(opts).model(modelID)`
/// path, catalog-driven).
pub fn language_model(
    context: &FacadeContext,
    catalog: &Catalog,
    provider_id: &str,
    model_id: &str,
    connection: &ModelConnection,
) -> Result<LanguageModel, ProviderError> {
    let snapshot = catalog
        .provider(provider_id)
        .ok_or_else(|| ProviderError::UnknownProvider(provider_id.to_string()))?;
    let model =
        snapshot.models.iter().find(|model| model.id.as_str() == model_id).ok_or_else(|| {
            ProviderError::UnknownModel(provider_id.to_string(), model_id.to_string())
        })?;
    language_model_for_model(context, snapshot, model, connection)
}

/// Bind one catalog model whose [`ModelInfo`] the caller already
/// resolved (CRED-A3: the model resolver applies variant overlays to the
/// catalog info BEFORE binding — v2 `withVariant`,
/// model-resolver.ts:137-161 — so variant settings/headers/body reach
/// the route defaults like the model's own overlays).
pub fn language_model_for_model(
    context: &FacadeContext,
    snapshot: &Snapshot,
    model: &ModelInfo,
    connection: &ModelConnection,
) -> Result<LanguageModel, ProviderError> {
    let provider_id = snapshot.info.id.0.as_str();
    let stored = model.package.as_deref().unwrap_or(&snapshot.info.package);
    // `loadPackage` (provider.ts:112-115) unwraps the `aisdk:` storage
    // form via `packageName` before the builtin lookup — a v2-era
    // models.dev npm name (`@opencode/ai/providers/*`, stored as
    // `aisdk:…` by nativePackage) binds to the same native builtin as
    // its `@ai-sdk` counterpart (CRED-AUTHNONE-DOWNGRADE v2 delta).
    let package = package_name(stored);
    // Copilot: the catalog entry is plain openai-compatible, but the bound
    // credential transport (BE-3b CredentialBindings) + the Copilot header
    // policy make it its own family (github-copilot.ts:434-505).
    let family = if provider_id == "github-copilot"
        && package == "@opencode/ai/providers/openai-compatible"
    {
        ProtocolFamily::CopilotChat
    } else {
        family_of(&package).ok_or_else(|| ProviderError::UnsupportedProtocol {
            provider: provider_id.to_string(),
            package: stored.to_string(),
        })?
    };

    // catalog api (settings.baseURL) is the default; user settings override
    let catalog_base = snapshot
        .info
        .settings
        .as_ref()
        .and_then(|settings| settings.rest.get("baseURL"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let base_url = connection.base_url.clone().or(catalog_base);
    let env = &snapshot.environment;

    let route = build_route(
        context,
        family,
        provider_id,
        &package,
        base_url.clone(),
        env,
        connection,
        model,
    )?;
    // Provider header plugins (AUD-10②): the ref cerebras/kilo/nvidia/
    // llmgateway plugins spread their static sets over `provider.headers`
    // after resolution, injected headers winning per key. Applied under
    // the model overlays (which still win per key, matching the ref
    // spread order plugin → model merge).
    let mut headers = connection.headers.clone();
    for (name, value) in plugin_provider_headers(
        &package,
        base_url.as_deref(),
        snapshot.info.activation,
        context.integration_registered(provider_id),
        &headers,
    ) {
        headers.insert(name, value);
    }
    // Connection headers ride the route defaults (transport headers) —
    // the seam credential-driven providers (copilot) use for their
    // client headers; model overlays apply after and win per key.
    let route = if headers.is_empty() {
        route
    } else {
        route.with(RoutePatch {
            defaults: RouteDefaults { headers: Some(headers), ..Default::default() },
            ..Default::default()
        })
    };
    let route = apply_model_overlays(&route, model);
    // G-119/G-121: the package compatibility presets the TS provider files
    // inject at `configure().model()`, under the model-level overlay
    // (opencode.ts:270-271).
    bind_model_with_compat(&route, model.id.as_str(), &package, model.compatibility.as_ref(), provider_id)
}

/// Bearer chain: explicit key, then each catalog env var (lazy env reads).
fn bearer_auth(api_key: Option<Secret>, env: &[String]) -> AuthDefinition {
    let mut credential = auth::optional(api_key);
    for name in env {
        credential = auth::or_else(&credential, &auth::config(name.clone()));
    }
    credential.bearer()
}

/// Patch a route's endpoint base URL.
fn with_base_url(route: Route, base_url: Option<String>) -> Route {
    match base_url {
        Some(base_url) => route.with(RoutePatch {
            endpoint: Some(EndpointPatch { base_url: Some(base_url), path: None, query: None }),
            ..Default::default()
        }),
        None => route,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_route(
    context: &FacadeContext,
    family: ProtocolFamily,
    provider_id: &str,
    package: &str,
    base_url: Option<String>,
    env: &[String],
    connection: &ModelConnection,
    model: &ModelInfo,
) -> Result<Route, ProviderError> {
    let transport = Arc::clone(&context.transport);
    let api_key = connection.secret();
    match family {
        ProtocolFamily::BedrockConverse => {
            let auth = bedrock_lane::BedrockAuthOptions {
                api_key: connection.api_key.clone(),
                ..Default::default()
            };
            let mut route = bedrock_lane::bedrock_converse_route(transport, auth);
            if let Some(base_url) = base_url {
                route = with_base_url(route, Some(base_url));
            }
            Ok(route)
        }
        ProtocolFamily::BedrockMantleChat => {
            let region = bedrock_lane::resolve_region(None, None);
            let auth = bedrock_lane::resolve_auth(
                &BedrockAuthOptions { api_key: connection.api_key.clone(), ..Default::default() },
                "bedrock-mantle",
                "Bedrock Mantle",
            );
            let base =
                base_url.unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws/v1"));
            Ok(openai_chat_route(transport).with(RoutePatch {
                id: Some("bedrock-mantle-chat".to_string()),
                provider: Some(provider_id.to_string()),
                provider_metadata_key: Some("mantle".to_string()),
                auth: Some(auth),
                endpoint: Some(EndpointPatch { base_url: Some(base), path: None, query: None }),
                ..Default::default()
            }))
        }
        ProtocolFamily::BedrockMantleResponses => {
            let region = bedrock_lane::resolve_region(None, None);
            let auth = bedrock_lane::resolve_auth(
                &BedrockAuthOptions { api_key: connection.api_key.clone(), ..Default::default() },
                "bedrock-mantle",
                "Bedrock Mantle",
            );
            let base =
                base_url.unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws/v1"));
            Ok(openai_compatible_responses_route(transport).with(RoutePatch {
                id: Some("bedrock-mantle-responses".to_string()),
                provider: Some(provider_id.to_string()),
                provider_metadata_key: Some("mantle".to_string()),
                auth: Some(auth),
                endpoint: Some(EndpointPatch { base_url: Some(base), path: None, query: None }),
                ..Default::default()
            }))
        }
        ProtocolFamily::BedrockMantleMessages => {
            let region = bedrock_lane::resolve_region(None, None);
            let auth = bedrock_lane::resolve_auth(
                &BedrockAuthOptions { api_key: connection.api_key.clone(), ..Default::default() },
                "bedrock-mantle",
                "Bedrock Mantle",
            );
            let base = base_url
                .unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws/anthropic/v1"));
            Ok(bedrock_lane::bedrock_mantle_messages_route(
                transport,
                MantleMessagesConfig {
                    api_key: connection.api_key.clone(),
                    base_url: Some(base),
                    ..Default::default()
                },
            )
            .with(RoutePatch {
                provider: Some(provider_id.to_string()),
                auth: Some(auth),
                ..Default::default()
            }))
        }
        ProtocolFamily::VertexGemini => {
            // connection.api_key carries the resolved access token (the
            // integration ADC binding or an explicit config token); the
            // Express API key rides its own env chain.
            let access_token =
                api_key.or_else(|| env_first(env, "GOOGLE_OAUTH_ACCESS_TOKEN").map(Secret::Env));
            let route = vertex_route_for_model(
                transport,
                VertexConfig {
                    access_token,
                    api_key: env_first(env, "GOOGLE_VERTEX_API_KEY").map(Secret::Env),
                    base_url,
                    ..Default::default()
                },
                model.id.as_str(),
            );
            Ok(route
                .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() }))
        }
        ProtocolFamily::VertexMessages => {
            // AI-VERTEX-V2: the Claude-on-Vertex lane — ADC bearer (the
            // resolved access token from the connection binding or the
            // env stand-in; API keys are rejected upstream in the
            // factory), the `global` location fallback, and the
            // project-composed publishers/anthropic base URL.
            let access_token =
                api_key.or_else(|| env_first(env, "GOOGLE_OAUTH_ACCESS_TOKEN").map(Secret::Env));
            vertex_messages_route(
                transport,
                VertexMessagesConfig {
                    base_url,
                    location: None,
                    project: None,
                    access_token,
                    api_key: None,
                },
            )
            .map_err(|error| ProviderError::Configuration {
                provider: provider_id.to_string(),
                message: error.to_string(),
            })
        }
        ProtocolFamily::VertexChat => {
            // AI-V1: the OpenAI-compat chat lane on the aiplatform
            // `endpoints/openapi` root — the composed project/location URL
            // (or the expanded catalog template), ADC bearer, API keys
            // rejected.
            let access_token =
                api_key.or_else(|| env_first(env, "GOOGLE_OAUTH_ACCESS_TOKEN").map(Secret::Env));
            vertex_chat_route(
                transport,
                VertexEndpointConfig {
                    base_url: vertex_lane_base_url(connection, model, base_url),
                    location: None,
                    project: None,
                    access_token,
                    api_key: None,
                },
            )
            .map_err(|error| ProviderError::Configuration {
                provider: provider_id.to_string(),
                message: error.to_string(),
            })
        }
        ProtocolFamily::VertexResponses => {
            // AI-V1: the open-responses lane on the same root with the
            // store/include defaults, ADC bearer, API keys rejected.
            let access_token =
                api_key.or_else(|| env_first(env, "GOOGLE_OAUTH_ACCESS_TOKEN").map(Secret::Env));
            vertex_responses_route(
                transport,
                VertexEndpointConfig {
                    base_url: vertex_lane_base_url(connection, model, base_url),
                    location: None,
                    project: None,
                    access_token,
                    api_key: None,
                },
            )
            .map_err(|error| ProviderError::Configuration {
                provider: provider_id.to_string(),
                message: error.to_string(),
            })
        }
        ProtocolFamily::XaiResponses => {
            // AI-M5: the native xai-responses lane at api.x.ai/v1 with the
            // optional XAI_API_KEY bearer chain (providers/xai.ts:36-49).
            Ok(xai_responses_route(XaiResponsesRouteSpec {
                base_url,
                auth: Some(xai_auth(api_key)),
                transport,
                defaults: RouteDefaults::default(),
            })
            .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() }))
        }
        ProtocolFamily::GoogleInteractions => {
            // AI-M3: the native interactions lane with the Google
            // api-key auth chain (`Google.configure(...).interactions`).
            let access_token =
                api_key.or_else(|| env_first(env, "GOOGLE_OAUTH_ACCESS_TOKEN").map(Secret::Env));
            Ok(google_interactions_route(
                transport,
                GoogleInteractionsConfig {
                    api_key: access_token
                        .or_else(|| env.first().map(|name| Secret::Env(name.clone()))),
                    base_url,
                },
            )
            .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() }))
        }
        ProtocolFamily::CopilotChat => Ok(copilot_chat_route(
            transport,
            CopilotConfig {
                base_url: base_url.clone(),
                token: api_key.or_else(|| {
                    env.iter()
                        .find(|name| name.as_str() == "GITHUB_TOKEN")
                        .map(|name| Secret::Env(name.clone()))
                }),
            },
        )
        .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() })),
        ProtocolFamily::AnthropicMessages => {
            if provider_id == "anthropic" {
                // v2 anthropic facade: x-api-key, ANTHROPIC_API_KEY fallback
                let route = anthropic_route(transport, api_key);
                Ok(with_base_url(route, base_url))
            } else {
                let route = anthropic_messages_route(transport).with(RoutePatch {
                    provider: Some(provider_id.to_string()),
                    auth: Some(bearer_auth(api_key, env)),
                    ..Default::default()
                });
                Ok(with_base_url(route, base_url))
            }
        }
        ProtocolFamily::OpenAIResponses => Ok(openai_responses_route(OpenAIResponsesRouteSpec {
            base_url,
            auth: Some(bearer_auth(api_key, env)),
            transport,
            defaults: RouteDefaults::default(),
        })
        .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() })),
        ProtocolFamily::Gemini => Ok(google_route(
            transport,
            GoogleConfig {
                api_key: api_key.or_else(|| env.first().map(|name| Secret::Env(name.clone()))),
                base_url,
            },
        )
        .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() })),
        ProtocolFamily::VercelGateway => {
            // G-81: per-model dispatch over the gateway-wrapped routes;
            // auth is the optional key → AI_GATEWAY_API_KEY →
            // VERCEL_OIDC_TOKEN bearer chain (the catalog env list rides
            // `bearer_auth`).
            let auth = bearer_auth(api_key, env);
            Ok(vercel_gateway_route_for_model(
                &VercelGatewayRouteSpec {
                    base_url,
                    auth: Some(auth),
                    transport,
                    defaults: RouteDefaults::default(),
                },
                model.id.as_str(),
            )
            .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() }))
        }
        ProtocolFamily::AlibabaResponses => {
            // G-84: the Model Studio responses lane at the dashscope
            // compatible-mode root (the catalog `api` / provider baseURL).
            let base_url = base_url.ok_or_else(|| ProviderError::Configuration {
                provider: provider_id.to_string(),
                message: format!(
                    "package `{package}` needs a baseURL (set provider.{provider_id}.baseURL)"
                ),
            })?;
            Ok(alibaba_responses_route(AlibabaResponsesRouteSpec {
                base_url,
                auth: Some(bearer_auth(api_key, env)),
                transport,
            })
            .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() }))
        }
        ProtocolFamily::MetaResponses => {
            // G-84 sibling: the native meta-responses lane at the baked
            // api.meta.ai/v1 root with the optional META_API_KEY bearer
            // chain (providers/meta.ts:89-98).
            Ok(meta_responses_route(MetaResponsesRouteSpec {
                base_url,
                auth: Some(meta_auth(api_key)),
                transport,
                defaults: RouteDefaults::default(),
            })
            .with(RoutePatch { provider: Some(provider_id.to_string()), ..Default::default() }))
        }
        ProtocolFamily::OpenAIChat => {
            // G-123: the provider env fallback chains hardcoded in the TS
            // provider files that the models-dev snapshot doesn't carry
            // (togetherai.ts:37, digitalocean.ts:36) — appended after the
            // catalog order.
            let env_chain = package_env_chain(package, env);
            let auth = bearer_auth(api_key, &env_chain);
            // G-119: deepinfra normalizes every configured baseURL — trim
            // trailing slashes, append `/openai` unless already suffixed
            // (deepinfra.ts:38-44).
            let base_url = if package == "@opencode/ai/providers/deepinfra" {
                base_url.map(|url| deepinfra_base_url(&url))
            } else {
                base_url
            };
            let baked = package_has_baked_base(package);
            let route = match package {
                "@opencode/ai/providers/deepseek" => deepseek_chat_route(transport, Some(auth)),
                "@opencode/ai/providers/groq" => groq_chat_route(transport, Some(auth)),
                "@opencode/ai/providers/openrouter" => openrouter_chat_route(transport, Some(auth)),
                "@opencode/ai/providers/venice" => venice_chat_route(transport, Some(auth)),
                "@opencode/ai/providers/zai/chat"
                | "@opencode/ai/providers/zai-coding-plan/chat" => {
                    zai_chat_route(transport, Some(auth))
                }
                "@opencode/ai/providers/alibaba/chat" => alibaba_chat_route(transport, Some(auth)),
                // G-118: DigitalOcean's own route — the OpenAI chat protocol
                // with `cache_control` breakpoint lowering and the route id
                // `"digitalocean"` cache-policy's RESPECTS_INLINE_HINTS
                // matches (digitalocean.ts:19-35).
                "@opencode/ai/providers/digitalocean" => {
                    digitalocean_chat_route(transport, Some(auth))
                }
                // builtin-table reconciliation: the chat-model re-export
                // rides the same factory with the baked api.openai.com
                // base (providers/openai/chat.ts:1).
                "@opencode/ai/providers/openai/chat" => {
                    openai_chat_route(transport).with(RoutePatch {
                        provider: Some(provider_id.to_string()),
                        auth: Some(auth),
                        ..Default::default()
                    })
                }
                _ => {
                    // generic chat: the provider's own endpoint is required
                    // (falling back to the OpenAI default URL would silently
                    // send another provider's traffic to api.openai.com) —
                    // except the packages with a baked default the snapshot
                    // doesn't carry (G-119/G-120: deepinfra.ts:10,
                    // togetherai.ts:10, cerebras.ts:10).
                    let base = base_url
                        .clone()
                        .or_else(|| baked_chat_base(package).map(str::to_string))
                        .ok_or_else(|| ProviderError::Configuration {
                            provider: provider_id.to_string(),
                            message: format!(
                                "package `{package}` needs a baseURL (set provider.{provider_id}.baseURL)"
                            ),
                        })?;
                    openai_chat_route(transport).with(RoutePatch {
                        provider: Some(provider_id.to_string()),
                        auth: Some(auth),
                        endpoint: Some(EndpointPatch {
                            base_url: Some(base),
                            path: None,
                            query: None,
                        }),
                        ..Default::default()
                    })
                }
            };
            let route = if baked { with_base_url(route, base_url) } else { route };
            Ok(route)
        }
        ProtocolFamily::CohereChat => {
            // AI-COHERE-MISTRAL: both Cohere lanes through the landed
            // oc-ai seam — the native package runs the Cohere v2 chat
            // protocol at api.cohere.com/v2, `/chat` rides the OpenAI
            // chat protocol at the COMPATIBILITY_BASE_URL; optional
            // bearer (explicit key → COHERE_API_KEY).
            cohere_route_for_package(package, transport, CohereConfig { api_key, base_url })
                .ok_or_else(|| ProviderError::Configuration {
                    provider: provider_id.to_string(),
                    message: format!("package `{package}` is not a Cohere package"),
                })
        }
        ProtocolFamily::MistralChat => {
            // AI-COHERE-MISTRAL: the native mistral-chat protocol at
            // api.mistral.ai/v1 with optional bearer (explicit key →
            // MISTRAL_API_KEY).
            mistral_route_for_package(package, transport, MistralConfig { api_key, base_url })
                .ok_or_else(|| ProviderError::Configuration {
                    provider: provider_id.to_string(),
                    message: format!("package `{package}` is not a Mistral package"),
                })
        }
        ProtocolFamily::OpenAICompatible => {
            let base_url = base_url.ok_or_else(|| ProviderError::Configuration {
                provider: provider_id.to_string(),
                message: format!(
                    "openai-compatible provider `{provider_id}` needs a baseURL (set provider.{provider_id}.baseURL)"
                ),
            })?;
            Ok(openai_compatible_chat_configured(transport, provider_id, &base_url)
                .with(RoutePatch { auth: Some(bearer_auth(api_key, env)), ..Default::default() }))
        }
    }
}

/// First env name in the catalog list matching `name` (the catalog env
/// order is the provider's documented precedence).
fn env_first(env: &[String], name: &str) -> Option<String> {
    env.iter().find(|candidate| candidate.as_str() == name).cloned()
}

/// The Vertex lanes' baseURL precedence (v2 folds the model's catalog
/// `api` into the settings the provider package reads): explicit
/// connection baseURL, then the model-level catalog template, then the
/// provider-level catalog api (the `base_url` param already collapsed
/// connection → provider-level).
fn vertex_lane_base_url(
    connection: &ModelConnection,
    model: &ModelInfo,
    base_url: Option<String>,
) -> Option<String> {
    connection.base_url.clone().or_else(|| {
        model
            .settings
            .as_ref()
            .and_then(|settings| settings.rest.get("baseURL"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(base_url)
    })
}

/// Chat factories with their own baked base URL (no catalog api needed).
fn package_has_baked_base(package: &str) -> bool {
    matches!(
        package,
        "@opencode/ai/providers/deepseek"
            | "@opencode/ai/providers/groq"
            | "@opencode/ai/providers/openrouter"
            | "@opencode/ai/providers/venice"
            | "@opencode/ai/providers/zai/chat"
            | "@opencode/ai/providers/zai-coding-plan/chat"
            | "@opencode/ai/providers/alibaba/chat"
            | "@opencode/ai/providers/openai/chat"
            // G-118/G-119/G-120: the packages whose baked defaults the
            // models-dev snapshot's missing `api` field dropped.
            | "@opencode/ai/providers/digitalocean"
            | "@opencode/ai/providers/deepinfra"
            | "@opencode/ai/providers/togetherai"
            | "@opencode/ai/providers/cerebras"
    )
}

/// Baked chat default base URLs the models-dev snapshot doesn't carry
/// (G-118/G-119/G-120): digitalocean.ts:11, deepinfra.ts:10,
/// togetherai.ts:10, cerebras.ts:10. The factory-baked families
/// (deepseek/groq/…) bake theirs inside their route constructors.
fn baked_chat_base(package: &str) -> Option<&'static str> {
    Some(match package {
        "@opencode/ai/providers/digitalocean" => "https://inference.do-ai.run/v1",
        "@opencode/ai/providers/deepinfra" => "https://api.deepinfra.com/v1/openai",
        "@opencode/ai/providers/togetherai" => "https://api.together.xyz/v1",
        "@opencode/ai/providers/cerebras" => "https://api.cerebras.ai/v1",
        _ => return None,
    })
}

/// deepinfra baseURL normalization (deepinfra.ts:38-44; G-119): trim
/// trailing slashes, append `/openai` unless the URL already ends with it.
fn deepinfra_base_url(url: &str) -> String {
    let root = url.trim_end_matches('/');
    if root.ends_with("/openai") {
        root.to_string()
    } else {
        format!("{root}/openai")
    }
}

/// Provider env fallback chains hardcoded in the TS provider files that the
/// models-dev snapshot's single-entry env lists don't carry (G-123):
/// togetherai.ts:37 `["TOGETHER_API_KEY", "TOGETHER_AI_API_KEY"]`,
/// digitalocean.ts:36 `["DIGITALOCEAN_ACCESS_TOKEN", "DIGITALOCEAN_API_KEY",
/// "DO_INFERENCE_API_KEY"]`. Appended after the catalog order (a snapshot
/// that later grows the entries degrades to no duplication).
fn package_env_chain(package: &str, env: &[String]) -> Vec<String> {
    let mut chain = env.to_vec();
    let fallbacks: &[&str] = match package {
        "@opencode/ai/providers/togetherai" => &["TOGETHER_AI_API_KEY"],
        "@opencode/ai/providers/digitalocean" => &["DIGITALOCEAN_API_KEY", "DO_INFERENCE_API_KEY"],
        _ => &[],
    };
    for name in fallbacks {
        if !chain.iter().any(|existing| existing.as_str() == *name) {
            chain.push((*name).to_string());
        }
    }
    chain
}

/// Per-package model compatibility the TS provider files inject at
/// `configure().model({... compatibility })` (G-119/G-121): deepinfra.ts:46-52,
/// togetherai.ts:44-49, cerebras.ts:41-48, fireworks.ts:49-51,
/// digitalocean.ts:44-50. The facade-side counterpart of the baked families'
/// `*_chat_compatibility()` presets.
fn package_chat_compatibility(package: &str) -> Option<LanguageModelCompatibility> {
    match package {
        "@opencode/ai/providers/deepinfra" => Some(deepinfra_chat_compatibility()),
        "@opencode/ai/providers/togetherai" => Some(togetherai_chat_compatibility()),
        "@opencode/ai/providers/cerebras" => Some(cerebras_chat_compatibility()),
        "@opencode/ai/providers/fireworks" => Some(fireworks_chat_compatibility()),
        "@opencode/ai/providers/digitalocean" => Some(digitalocean_chat_compatibility()),
        _ => None,
    }
}

/// Spread the model-level compatibility overlay over the package base
/// (opencode.ts:270-271 `{...base, ...config}` — per-field, model wins).
fn overlay_model_compatibility(
    mut base: LanguageModelCompatibility,
    overlay: Option<&oc_schema::model::Compatibility>,
) -> LanguageModelCompatibility {
    let Some(overlay) = overlay else { return base };
    if let Some(field) = &overlay.reasoning_field {
        base.reasoning_field = Some(field.clone());
    }
    if let Some(value) = overlay.require_reasoning {
        base.require_reasoning = Some(value);
    }
    if let Some(value) = overlay.max_tokens_field {
        base.max_tokens_field = Some(match value {
            oc_schema::model::MaxTokensField::MaxTokens => {
                MaxTokensFieldCompatibility::MaxTokens
            }
            oc_schema::model::MaxTokensField::MaxCompletionTokens => {
                MaxTokensFieldCompatibility::MaxCompletionTokens
            }
        });
    }
    if let Some(value) = overlay.require_finish_reason {
        base.require_finish_reason = Some(value);
    }
    if let Some(value) = overlay.require_assistant_after_tool {
        base.require_assistant_after_tool = Some(value);
    }
    if let Some(value) = overlay.supports_prompt_cache_key {
        base.supports_prompt_cache_key = Some(value);
    }
    base
}

/// Bind the route model with the package compatibility (G-119/G-121) under
/// the model-level overlay.
fn bind_model_with_compat(
    route: &Route,
    model_id: &str,
    package: &str,
    overlay: Option<&oc_schema::model::Compatibility>,
    provider_id: &str,
) -> Result<LanguageModel, ProviderError> {
    let bound = route.model(model_id, None).map_err(|error| ProviderError::Configuration {
        provider: provider_id.to_string(),
        message: error.to_string(),
    })?;
    Ok(match package_chat_compatibility(package) {
        Some(compat) => bound.with_compatibility(overlay_model_compatibility(compat, overlay)),
        None => bound,
    })
}

/// Fold `Model.Info` overlays into the route defaults. `settings.rest`
/// lands in provider options; `body` lands in the HTTP body overlay and
/// `headers` in the transport headers — v2 resolves the same overlay
/// through the provider package (`settings.body` → `http: { body }`,
/// openai-compatible.ts:48-59 / anthropic.ts:55-68) and merges it into
/// every request body at send time (route/transport/http.ts:41-49).
fn apply_model_overlays(route: &Route, model: &ModelInfo) -> Route {
    let mut provider_options = serde_json::Map::new();
    if let Some(settings) = &model.settings {
        // Credential/Core keys never reach native provider options
        // (provider.ts:141-151 CORE_KEYS, model-resolver.ts:226).
        provider_options.extend(native_settings(&settings.rest));
    }
    let http = HttpOptions {
        body: model.body.as_ref().map(|body| {
            serde_json::Map::from_iter(body.iter().map(|(k, v)| (k.clone(), v.clone())))
        }),
        ..Default::default()
    };
    route.with(RoutePatch {
        defaults: RouteDefaults {
            headers: model.headers.clone(),
            provider_options: (!provider_options.is_empty()).then_some(provider_options),
            http: (http != HttpOptions::default()).then_some(http),
            ..Default::default()
        },
        ..Default::default()
    })
}

/// opencode settings consumed in Core; native packages never receive
/// them (provider.ts:141-151 `CORE_KEYS` / `nativeSettings`).
const CORE_KEYS: &[&str] =
    &["chunkTimeout", "compaction", "fetch", "headerTimeout", "timeout", "transport"];

/// Credential keys never passed raw into native settings
/// (model-resolver.ts:226 `Struct.omit(mapped, ["accessToken",
/// "apiKey", "authToken"])` + console projection withoutCredentials,
/// opencode.ts:547-551).
const CREDENTIAL_KEYS: &[&str] = &["accessToken", "apiKey", "authToken"];

/// Strip the Core-consumed and credential keys from a settings record
/// before it reaches native provider options (provider.ts:148
/// nativeSettings + model-resolver.ts:226).
fn native_settings(rest: &BTreeMap<String, Value>) -> serde_json::Map<String, Value> {
    rest.iter()
        .filter(|(key, _)| {
            !CORE_KEYS.contains(&key.as_str()) && !CREDENTIAL_KEYS.contains(&key.as_str())
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// The custom model's settings record reaching native provider options
/// (opencode.ts:277-280 `model.settings = mergeOverlay(...,
/// withoutCredentials(config.settings))` fed to the package at
/// model-resolver.ts:220-226): credential-stripped at projection,
/// CORE_KEYS-stripped here; `baseURL` is consumed by the endpoint
/// precedence ladder and never re-rides the options (opencode.ts:276
/// deletes it from the base when superseded).
fn custom_provider_options(model: &CustomModel) -> Option<serde_json::Map<String, Value>> {
    let rest = model.settings.as_ref().map(|settings| &settings.rest)?;
    let options = native_settings(rest)
        .into_iter()
        .filter(|(key, _)| key != "baseURL" && key != "base_url")
        .collect::<serde_json::Map<String, Value>>();
    (!options.is_empty()).then_some(options)
}

/// Bind a user-defined custom provider (config `provider.<id>` /
/// console-issued provider). When the binding package — the MODEL-level
/// `package` first, else the PROVIDER-level one (provider.ts:351
/// `model.package ?? record.provider.package`) — resolves to a native
/// `@opencode/ai/providers/*` entrypoint (native::resolve_specifier,
/// the port of aisdk-native.ts:147-155 + the loadPackage builtin table),
/// it binds through the SAME lane-specific family dispatch as the
/// catalog (build_route — same-family-different-lane packages like
/// `cohere/chat` vs `cohere` or `google-vertex/messages` pick their own
/// constructors, never raw string matching). Without a package the
/// openai-compatible facade remains the seam. The provider- and
/// model-level `headers`/`body` overlays ride the route defaults; a
/// model-level `settings.baseURL` overlay outranks the provider's
/// endpoint (opencode.ts:276-279).
pub fn custom_language_model(
    context: &FacadeContext,
    custom: &CustomProvider,
    model_id: &str,
) -> Result<LanguageModel, ProviderError> {
    let Some(model) = custom.models.iter().find(|model| model.id == model_id) else {
        return Err(ProviderError::UnknownModel(custom.id.clone(), model_id.to_string()));
    };
    // The per-model baseURL overlay wins per key over the provider's
    // (opencode.ts:276-279 mergeOverlay).
    let base_url =
        model.base_url.as_deref().filter(|url| !url.is_empty()).unwrap_or(custom.base_url.as_str());
    // Native-protocol binding (provider.ts:351): the model package
    // overrides the provider's; resolution is context-aware (HOSTS
    // redirects are host-conditional, the mantle split consults the
    // model id). `None` resolution = no native counterpart → the
    // registered exemption's clean typed error (the `aisdk:` npm-install
    // bridge is not ported).
    let specifier = model.package.as_deref().or(custom.package.as_deref());
    if let Some(specifier) = specifier {
        return custom_native_language_model(context, custom, model, specifier, base_url);
    }
    if base_url.is_empty() {
        return Err(ProviderError::Configuration {
            provider: custom.id.clone(),
            message: format!(
                "custom provider `{provider}` needs a baseURL (set provider.{provider}.baseURL or the model's baseURL)",
                provider = custom.id
            ),
        });
    }
    // Effective overlays: provider-level under model-level, the model
    // winning per key (v2 core/src/model.ts:204-209 + provider.ts
    // mergeOverlay/mergeHeaders:172-205).
    let provider_body = custom
        .body
        .as_ref()
        .map(|body| serde_json::Map::from_iter(body.iter().map(|(k, v)| (k.clone(), v.clone()))));
    let model_body = model
        .body
        .as_ref()
        .map(|body| serde_json::Map::from_iter(body.iter().map(|(k, v)| (k.clone(), v.clone()))));
    let body = merge_json_records(&[provider_body.as_ref(), model_body.as_ref()]);
    // Provider header plugins (AUD-10②): custom providers bind through
    // the openai-compatible package, so a custom provider pointed at the
    // kilo/nvidia/llmgateway gate URLs gets the same injected sets
    // (spread under the model overlay, which wins per key like the ref
    // spread order plugin → model merge).
    //
    // F-10a TODO(activation-gate): `Activation::Enabled` is hardcoded
    // because config-defined custom providers have no catalog snapshot —
    // a disabled provider config never reaches binding in the current
    // call graph, so the llmgateway `activation !== Disabled` gate
    // (plugin/provider/llmgateway.ts:12-15) is moot here. If a path ever
    // binds a disabled custom provider, thread the real activation
    // through `FacadeContext` instead.
    let plugin_headers = plugin_provider_headers(
        "@opencode/ai/providers/openai-compatible",
        Some(base_url),
        Activation::Enabled,
        context.integration_registered(&custom.id),
        custom.headers.as_ref().unwrap_or(&BTreeMap::new()),
    );
    let headers = merge_string_records(&[
        custom.headers.as_ref(),
        Some(&plugin_headers),
        model.headers.as_ref(),
    ]);
    let http = HttpOptions { body, ..Default::default() };
    let route =
        openai_compatible_chat_configured(Arc::clone(&context.transport), &custom.id, base_url)
            .with(RoutePatch {
                auth: Some(match custom.api_key.clone() {
                    Some(key) => bearer_auth(Some(Secret::Value(key)), &[]),
                    // No configured key: anonymous — v2's openai-compatible
                    // providers send unauthenticated requests when no credential
                    // is configured (local/loopback providers rely on it).
                    None => auth::Definition::none(),
                }),
                defaults: RouteDefaults {
                    headers: headers.clone(),
                    provider_options: custom_provider_options(model),
                    http: (http != HttpOptions::default()).then_some(http),
                    ..Default::default()
                },
                ..Default::default()
            });
    route.model(&model.id, None).map_err(|error| ProviderError::Configuration {
        provider: custom.id.clone(),
        message: error.to_string(),
    })
}

/// Bind the native-package custom lane's model with the same package
/// compatibility injection as the catalog lane (G-119/G-121), under the
/// config model's compatibility overlay.
#[allow(clippy::missing_errors_doc)]
fn bind_custom_native_model(
    route: &Route,
    wire_id: &str,
    package: &str,
    model: &CustomModel,
    provider_id: &str,
) -> Result<LanguageModel, ProviderError> {
    bind_model_with_compat(
        route,
        wire_id,
        package,
        model.compatibility.as_ref(),
        provider_id,
    )
}

/// The native-package custom binding: route through the catalog family
/// dispatch with a synthesized route model (the custom lane has no
/// catalog Info) and the merged overlays. Azure needs the customer's
/// resource URL (azure.ts:22-23 `AzureURL` throws otherwise) — no
/// endpoint is a configuration error, not a silent api.openai.com
/// misroute.
fn custom_native_language_model(
    context: &FacadeContext,
    custom: &CustomProvider,
    model: &CustomModel,
    specifier: &str,
    base_url: &str,
) -> Result<LanguageModel, ProviderError> {
    // `useCompletionUrls === true` switches the azure package to the
    // chat lane exactly like the completions shape does
    // (aisdk-native.ts:126-127 azureChat, :44-45).
    let use_completion_urls = model
        .settings
        .as_ref()
        .and_then(|settings| settings.rest.get("useCompletionUrls"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let package = native::resolve_specifier(
        specifier,
        &custom.id,
        Some(&model.id),
        None,
        use_completion_urls,
    )
    .ok_or_else(|| ProviderError::UnsupportedProtocol {
        provider: custom.id.clone(),
        package: specifier.to_string(),
    })?;
    let family = family_of(&package).ok_or_else(|| ProviderError::UnsupportedProtocol {
        provider: custom.id.clone(),
        package: package.clone(),
    })?;
    let base_url = (!base_url.is_empty()).then(|| base_url.to_string());
    if base_url.is_none() && package.starts_with("@opencode/ai/providers/azure") {
        return Err(ProviderError::Configuration {
            provider: custom.id.clone(),
            message: format!(
                "package `{package}` needs a baseURL (azure requires resourceName or baseURL, set provider.{}.baseURL)",
                custom.id
            ),
        });
    }
    // The wire model id is the config `modelID` overlay when present —
    // v2 binds `resolved.modelID ?? resolved.id` (model-resolver.ts:233).
    let wire_id = model.model_id.as_deref().unwrap_or(&model.id);
    // Synthesized route Info: build_route only reads the identity and
    // the catalog settings baseURL (the Vertex lanes) — the endpoint
    // precedence already collapsed into `base_url`.
    let mut route_info = oc_schema::model::Info {
        id: oc_schema::id::ModelId::parse(wire_id.to_string()),
        model_id: oc_schema::id::ModelId::parse(wire_id.to_string()),
        provider_id: oc_schema::id::ProviderId::parse(custom.id.clone()),
        canonical: None,
        family: None,
        name: model.name.clone().unwrap_or_else(|| model.id.clone()),
        compatibility: None,
        package: Some(package.clone()),
        settings: None,
        headers: None,
        body: None,
        capabilities: oc_schema::model::Capabilities::default(),
        variants: Vec::new(),
        time: oc_schema::model::ModelTime { released: oc_schema::common::Num(0.0) },
        cost: Vec::new(),
        status: oc_schema::model::Status::Active,
        enabled: true,
        limit: oc_schema::model::ModelLimit { context: 0, input: None, output: 0 },
    };
    if let Some(base) = &base_url {
        let mut rest = BTreeMap::new();
        rest.insert("baseURL".to_string(), Value::String(base.clone()));
        route_info.settings = Some(oc_schema::model::Settings { rest, ..Default::default() });
    }
    let connection = ModelConnection {
        base_url: base_url.clone(),
        api_key: custom.api_key.clone(),
        headers: BTreeMap::new(),
    };
    let route = build_route(
        context,
        family,
        &custom.id,
        &package,
        base_url.clone(),
        &[],
        &connection,
        &route_info,
    )?;
    // The same overlay fold as the fallback lane: provider-level under
    // model-level (model.ts:204-209).
    let provider_body = custom
        .body
        .as_ref()
        .map(|body| serde_json::Map::from_iter(body.iter().map(|(k, v)| (k.clone(), v.clone()))));
    let model_body = model
        .body
        .as_ref()
        .map(|body| serde_json::Map::from_iter(body.iter().map(|(k, v)| (k.clone(), v.clone()))));
    let body = merge_json_records(&[provider_body.as_ref(), model_body.as_ref()]);
    // AUD-10②: same provider header plugins as the catalog lane — the
    // cerebras package match (and the openai-compatible gate URLs) apply
    // to custom native bindings too.
    //
    // F-10a TODO(activation-gate): same as the openai-compatible custom
    // lane above — config-defined providers carry no activation state;
    // thread the real activation through `FacadeContext` if a disabled
    // custom provider can ever reach binding.
    let plugin_headers = plugin_provider_headers(
        &package,
        base_url.as_deref(),
        Activation::Enabled,
        context.integration_registered(&custom.id),
        custom.headers.as_ref().unwrap_or(&BTreeMap::new()),
    );
    let headers = merge_string_records(&[
        custom.headers.as_ref(),
        Some(&plugin_headers),
        model.headers.as_ref(),
    ]);
    let http = HttpOptions { body, ..Default::default() };
    let route = route.with(RoutePatch {
        defaults: RouteDefaults {
            headers: headers.clone(),
            provider_options: custom_provider_options(model),
            http: (http != HttpOptions::default()).then_some(http),
            ..Default::default()
        },
        ..Default::default()
    });
    bind_custom_native_model(&route, wire_id, &package, model, &custom.id)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use oc_ai::route::transport::FailingTransport;

    fn context() -> FacadeContext {
        FacadeContext::new(Arc::new(FailingTransport { message: "facade unit test" }))
    }

    #[test]
    fn provider_facade_anthropic_binding() {
        let context = context();
        let catalog = Catalog::embedded();
        let model = language_model(
            &context,
            catalog,
            "anthropic",
            "claude-opus-4-5",
            &ModelConnection::default(),
        )
        .expect("anthropic binds");
        assert_eq!(model.id.as_str(), "claude-opus-4-5");
        assert_eq!(model.provider.as_str(), "anthropic");
        assert!(model.route.id.contains("anthropic"));
        assert!(model.route.endpoint.base_url.is_some());
    }

    #[test]
    fn provider_facade_openai_binding() {
        let context = context();
        let catalog = Catalog::embedded();
        let model = language_model(
            &context,
            catalog,
            "openai",
            "gpt-5.4",
            &ModelConnection {
                base_url: Some("https://proxy.test/v1".into()),
                ..Default::default()
            },
        )
        .expect("openai binds");
        assert_eq!(model.provider.as_str(), "openai");
        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some("https://proxy.test/v1"),
            "user baseURL overrides the default"
        );
    }

    #[test]
    fn provider_facade_gemini_binding() {
        let context = context();
        let catalog = Catalog::embedded();
        let model = language_model(
            &context,
            catalog,
            "google",
            "gemini-3-pro-preview",
            &ModelConnection::default(),
        );
        // whichever gemini id exists, the provider must bind or cleanly fail
        match model {
            Ok(model) => assert_eq!(model.provider.as_str(), "google"),
            Err(ProviderError::UnknownModel(_, _)) => {
                // snapshot drift: pick the first google model instead
                let google = catalog.provider("google").expect("google present");
                let id = google.models.first().expect("google has models").id.as_str().to_string();
                let model =
                    language_model(&context, catalog, "google", &id, &ModelConnection::default())
                        .expect("google model binds");
                assert_eq!(model.provider.as_str(), "google");
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn provider_facade_openai_compatible_catalog_provider() {
        // A catalog-only provider with an api: fireworks-ai redirects to the
        // native chat package; a generic one (no host entry) stays on
        // openai-compatible. Find one of the ~25.
        let context = context();
        let catalog = Catalog::embedded();
        let generic = catalog
            .providers()
            .iter()
            .find(|snapshot| snapshot.info.package == "@opencode/ai/providers/openai-compatible")
            .expect("catalog-only openai-compatible provider exists");
        let model_id = generic.models.first().expect("has models").id.as_str().to_string();
        let model = language_model(
            &context,
            catalog,
            generic.info.id.as_str(),
            &model_id,
            &ModelConnection::default(),
        )
        .expect("binds via openai-compatible");
        assert_eq!(model.route.id, "openai-compatible-chat");
        assert!(model.route.endpoint.base_url.is_some(), "catalog api supplies baseURL");
    }

    #[test]
    fn provider_facade_unsupported_protocol_typed_error() {
        let context = context();
        let catalog = Catalog::embedded();
        // Find a catalog provider whose package still has no ported
        // protocol (the `aisdk:` shims) — bedrock/vertex/copilot now bind.
        let unsupported = catalog
            .providers()
            .iter()
            .find_map(|snapshot| {
                let model = snapshot.models.first()?;
                let package = model.package.as_deref().unwrap_or(&snapshot.info.package);
                (package.starts_with("aisdk:") && family_of(package).is_none()).then_some((
                    snapshot.info.id.0.clone(),
                    package.to_string(),
                    model.id.as_str().to_string(),
                ))
            })
            .expect("an aisdk: shim provider exists in the catalog");
        let error = language_model(
            &context,
            catalog,
            &unsupported.0,
            &unsupported.2,
            &ModelConnection::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ProviderError::UnsupportedProtocol {
                provider: unsupported.0.clone(),
                package: unsupported.1.clone(),
            },
            "clean typed error for unported protocol"
        );
        assert!(error.to_string().contains("no ported protocol"));
    }

    /// BE-4 item 1: Bedrock catalog models bind — the mantle split routes
    /// gpt-oss models to the Mantle chat deployment and the rest to the
    /// Mantle open-responses deployment, both over the shared Bedrock auth
    /// (bearer `AWS_BEARER_TOKEN_BEDROCK` / SigV4) and mantle base URL.
    #[test]
    fn provider_facade_bedrock_bindings() {
        let context = context();
        let catalog = Catalog::embedded();
        let bedrock = catalog.provider("amazon-bedrock").expect("bedrock in catalog");
        let chat_model = bedrock
            .models
            .iter()
            .find(|model| {
                model.package.as_deref().unwrap_or(&bedrock.info.package)
                    == "@opencode/ai/providers/amazon-bedrock/mantle/chat"
            })
            .expect("a mantle-chat (gpt-oss) bedrock model exists");
        let model = language_model(
            &context,
            catalog,
            "amazon-bedrock",
            chat_model.id.as_str(),
            &ModelConnection { api_key: Some("bedrock-bearer".into()), ..Default::default() },
        )
        .expect("gpt-oss bedrock binds to mantle chat");
        assert_eq!(model.route.id, "bedrock-mantle-chat");
        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some("https://bedrock-mantle.us-east-1.api.aws/v1")
        );

        let other = bedrock
            .models
            .iter()
            .find(|model| {
                model.package.as_deref().unwrap_or(&bedrock.info.package)
                    == "@opencode/ai/providers/amazon-bedrock/mantle/responses"
            })
            .expect("a mantle-responses bedrock model exists");
        let model = language_model(
            &context,
            catalog,
            "amazon-bedrock",
            other.id.as_str(),
            &ModelConnection { api_key: Some("bedrock-bearer".into()), ..Default::default() },
        )
        .expect("bedrock binds to mantle responses");
        assert_eq!(model.route.id, "bedrock-mantle-responses");
        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some("https://bedrock-mantle.us-east-1.api.aws/v1")
        );
    }

    /// BE-4 item 2: Vertex native models bind through the aiplatform
    /// deployment with the access token from the connection (the ADC
    /// credential binding) as the bearer.
    #[test]
    fn provider_facade_vertex_native_binding() {
        let context = context();
        let catalog = Catalog::embedded();
        let vertex = catalog.provider("google-vertex").expect("vertex in catalog");
        let gemini_model = vertex
            .models
            .iter()
            .find(|model| {
                model.package.as_deref().unwrap_or(&vertex.info.package)
                    == "@opencode/ai/providers/google-vertex"
            })
            .expect("a native-package vertex model exists");
        let model = language_model(
            &context,
            catalog,
            "google-vertex",
            gemini_model.id.as_str(),
            &ModelConnection { api_key: Some("ya29.adc".into()), ..Default::default() },
        )
        .expect("vertex native binds");
        assert_eq!(model.route.id, "google-vertex-gemini");
        let route = Arc::clone(&model.route);
        let base = route.endpoint.base_url.as_deref().unwrap_or_default().to_string();
        assert!(base.contains("-aiplatform.googleapis.com/v1beta1/projects/"), "{base}");
        // authorization comes from the resolved access token at request time
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(transport_request.header("authorization"), Some("Bearer ya29.adc"));
        assert!(transport_request.url.contains(":streamGenerateContent?alt=sse"));
    }

    /// AI-VERTEX-V2: the catalog's Claude-on-Vertex models (per-model npm
    /// `@ai-sdk/google-vertex/anthropic` → `google-vertex/messages`) bind
    /// the native Vertex Messages lane — the project/location URL, the
    /// `:streamRawPredict` path, ADC bearer, and never api.anthropic.com.
    #[test]
    fn provider_facade_vertex_messages_binding() {
        let context = context();
        let catalog = Catalog::embedded();
        let vertex = catalog.provider("google-vertex").expect("vertex in catalog");
        let claude = vertex
            .models
            .iter()
            .find(|model| {
                model.package.as_deref().unwrap_or(&vertex.info.package)
                    == "@opencode/ai/providers/google-vertex/messages"
            })
            .expect("a claude-on-vertex (messages) model exists");
        // The composed URL needs a project (requireProject); no other test
        // in this binary touches the project env aliases.
        std::env::set_var("GOOGLE_VERTEX_PROJECT", "facade-test-proj");
        let model = language_model(
            &context,
            catalog,
            "google-vertex",
            claude.id.as_str(),
            &ModelConnection { api_key: Some("ya29.adc".into()), ..Default::default() },
        )
        .expect("vertex messages binds");
        std::env::remove_var("GOOGLE_VERTEX_PROJECT");
        assert_eq!(model.route.id, "google-vertex-messages");
        let route = Arc::clone(&model.route);
        assert!(
            route
                .endpoint
                .base_url
                .as_deref()
                .is_some_and(|base| base.contains("/v1/projects/facade-test-proj/locations/")
                    && base.ends_with("/publishers/anthropic/models")),
            "the publishers/anthropic base URL: {:?}",
            route.endpoint.base_url
        );
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert!(transport_request.url.ends_with(":streamRawPredict"), "{}", transport_request.url);
        assert!(!transport_request.url.contains("api.anthropic.com"));
        assert_eq!(transport_request.header("authorization"), Some("Bearer ya29.adc"));
        assert_eq!(transport_request.header("x-goog-api-key"), None);
        assert_eq!(body["anthropic_version"], "vertex-2023-10-16");
        assert!(body.get("model").is_none());
    }

    /// AI-V1: the catalog's Vertex maas models (npm
    /// `@ai-sdk/openai-compatible` → `google-vertex/chat`) bind the
    /// OpenAPI chat lane — the `${GOOGLE_VERTEX_*}` catalog baseURL
    /// template expands against the resolved project/location instead of
    /// failing "needs a baseURL".
    #[test]
    fn provider_facade_vertex_chat_responses_bindings() {
        let context = context();
        let catalog = Catalog::embedded();
        let vertex = catalog.provider("google-vertex").expect("vertex in catalog");
        let maas = vertex
            .models
            .iter()
            .find(|model| {
                model.package.as_deref().unwrap_or(&vertex.info.package)
                    == "@opencode/ai/providers/google-vertex/chat"
            })
            .expect("an openapi-chat (maas) vertex model exists");
        std::env::set_var("GOOGLE_VERTEX_PROJECT", "facade-test-proj");
        std::env::set_var("GOOGLE_VERTEX_LOCATION", "us-central1");
        let model = language_model(
            &context,
            catalog,
            "google-vertex",
            maas.id.as_str(),
            &ModelConnection { api_key: Some("ya29.adc".into()), ..Default::default() },
        )
        .expect("vertex chat binds");
        std::env::remove_var("GOOGLE_VERTEX_PROJECT");
        std::env::remove_var("GOOGLE_VERTEX_LOCATION");
        assert_eq!(model.route.id, "google-vertex-chat");
        let route = Arc::clone(&model.route);
        assert_eq!(
            route.endpoint.base_url.as_deref(),
            Some(
                "https://us-central1-aiplatform.googleapis.com/v1/projects/facade-test-proj/locations/us-central1/endpoints/openapi"
            ),
            "the catalog template expanded"
        );
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert!(transport_request.url.ends_with("/chat/completions"), "{}", transport_request.url);
        assert_eq!(transport_request.header("authorization"), Some("Bearer ya29.adc"));

        // The responses lane binds through the same seam when a model
        // carries the responses package (config-pinned packages).
        assert_eq!(
            family_of("@opencode/ai/providers/google-vertex/responses"),
            Some(ProtocolFamily::VertexResponses),
            "the responses package has its own family arm"
        );
    }

    /// BE-4 item 3: Copilot chat binds the openai-compatible protocol with
    /// the Copilot header policy over the credential-bound transport.
    #[test]
    fn provider_facade_copilot_chat_binding() {
        let context = context();
        let catalog = Catalog::embedded();
        let copilot = catalog.provider("github-copilot").expect("copilot in catalog");
        let model_id = copilot.models.first().expect("copilot has models").id.as_str().to_string();
        let model = language_model(
            &context,
            catalog,
            "github-copilot",
            &model_id,
            &ModelConnection {
                base_url: None,
                api_key: Some("ghu_bound_token".into()),
                headers: BTreeMap::new(),
            },
        )
        .expect("copilot binds");
        assert_eq!(model.route.id, "copilot-chat");
        assert_eq!(model.route.endpoint.base_url.as_deref(), Some("https://api.githubcopilot.com"));
        let route = Arc::clone(&model.route);
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(transport_request.url, "https://api.githubcopilot.com/chat/completions");
        assert_eq!(transport_request.header("authorization"), Some("Bearer ghu_bound_token"));
        assert_eq!(transport_request.header("X-GitHub-Api-Version"), Some("2026-08-01"));
    }

    /// AI-COHERE-MISTRAL (production mapping half): the catalog's
    /// `cohere` (npm `@ai-sdk/cohere` → native package) and `mistral`
    /// providers must bind through the landed oc-ai seams — native
    /// protocol ids and default base URLs, with the bearer chain
    /// (explicit key over the provider env var) reaching the wire.
    #[test]
    fn provider_facade_cohere_mistral_native_bindings() {
        let context = context();
        let catalog = Catalog::embedded();

        let cohere = catalog.provider("cohere").expect("cohere in catalog");
        let model_id = cohere.models.first().expect("cohere has models").id.as_str().to_string();
        let package = cohere
            .models
            .first()
            .and_then(|model| model.package.clone())
            .unwrap_or_else(|| cohere.info.package.clone());
        assert_eq!(
            package, "@opencode/ai/providers/cohere",
            "catalog maps npm @ai-sdk/cohere natively"
        );
        let model = language_model(
            &context,
            catalog,
            "cohere",
            &model_id,
            &ModelConnection { api_key: Some("cohere-key".into()), ..Default::default() },
        )
        .expect("cohere binds natively");
        assert_eq!(model.route.protocol.id(), "cohere-chat");
        assert_eq!(model.route.endpoint.base_url.as_deref(), Some("https://api.cohere.com/v2"));
        let route = Arc::clone(&model.route);
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(transport_request.url, "https://api.cohere.com/v2/chat");
        assert_eq!(transport_request.header("authorization"), Some("Bearer cohere-key"));

        let mistral = catalog.provider("mistral").expect("mistral in catalog");
        let model_id = mistral.models.first().expect("mistral has models").id.as_str().to_string();
        let package = mistral
            .models
            .first()
            .and_then(|model| model.package.clone())
            .unwrap_or_else(|| mistral.info.package.clone());
        assert_eq!(
            package, "@opencode/ai/providers/mistral",
            "catalog maps npm @ai-sdk/mistral natively"
        );
        let model = language_model(
            &context,
            catalog,
            "mistral",
            &model_id,
            &ModelConnection { api_key: Some("mistral-key".into()), ..Default::default() },
        )
        .expect("mistral binds natively");
        assert_eq!(model.route.protocol.id(), "mistral-chat");
        let route = Arc::clone(&model.route);
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(transport_request.url, "https://api.mistral.ai/v1/chat/completions");
        assert_eq!(transport_request.header("authorization"), Some("Bearer mistral-key"));
    }

    /// AI-COHERE-MISTRAL: the `/chat` OpenAI-compatibility lane binds at
    /// the Cohere COMPATIBILITY_BASE_URL through the same seam (the
    /// host-table redirect `cohere` + `@ai-sdk/openai-compatible`).
    #[test]
    fn provider_facade_cohere_chat_compatibility_lane() {
        assert_eq!(
            family_of("@opencode/ai/providers/cohere/chat"),
            Some(ProtocolFamily::CohereChat),
            "the compat lane routes through the Cohere seam, not generic chat"
        );
        let route = cohere_route_for_package(
            "@opencode/ai/providers/cohere/chat",
            Arc::new(oc_ai::route::transport::FailingTransport { message: "facade unit test" }),
            CohereConfig::default(),
        )
        .expect("compat lane maps");
        assert_eq!(route.id, "cohere-chat-completions");
        assert_eq!(
            route.endpoint.base_url.as_deref(),
            Some("https://api.cohere.ai/compatibility/v1")
        );
    }

    /// AI-M5: the `xai` package binds the native `xai-responses` protocol
    /// at api.x.ai/v1/responses with the store/include defaults and the
    /// XAI_API_KEY bearer chain (providers/xai.ts:36-49).
    #[test]
    fn provider_facade_xai_responses_family() {
        assert_eq!(
            family_of("@opencode/ai/providers/xai"),
            Some(ProtocolFamily::XaiResponses),
            "xai maps to its native responses family, not the plain OpenAI one"
        );
        let route = xai_responses_route(XaiResponsesRouteSpec {
            base_url: None,
            auth: Some(xai_auth(Some(oc_ai::route::auth::Secret::Value("x-key".into())))),
            transport: Arc::new(oc_ai::route::transport::FailingTransport {
                message: "facade unit test",
            }),
            defaults: RouteDefaults::default(),
        });
        assert_eq!(route.id, "xai-responses");
        assert_eq!(route.protocol.id(), "xai-responses");
        assert_eq!(route.provider_metadata_key.as_deref(), Some("xai"));
        assert_eq!(route.endpoint.base_url.as_deref(), Some("https://api.x.ai/v1"));
        let mut request =
            oc_ai::schema::LLMRequest::new(route.model("grok-4", None).expect("model binds"));
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        assert_eq!(body["store"], serde_json::json!(false));
        assert_eq!(body["include"], serde_json::json!(["reasoning.encrypted_content"]));
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(transport_request.url, "https://api.x.ai/v1/responses");
        assert_eq!(transport_request.header("authorization"), Some("Bearer x-key"));
    }

    /// AI-M3: the pinned `google/interactions` package binds the native
    /// `google-interactions` protocol at `/interactions` with the Google
    /// api-key auth chain (provider.ts:77 registry entry).
    #[test]
    fn provider_facade_google_interactions_family() {
        assert_eq!(
            family_of("@opencode/ai/providers/google/interactions"),
            Some(ProtocolFamily::GoogleInteractions),
            "the interactions package maps to its native family"
        );
        assert_eq!(
            family_of("@opencode/ai/providers/google"),
            Some(ProtocolFamily::Gemini),
            "the plain google package stays on Gemini"
        );
        let route = google_interactions_route(
            Arc::new(oc_ai::route::transport::FailingTransport { message: "facade unit test" }),
            GoogleInteractionsConfig {
                api_key: Some(oc_ai::route::auth::Secret::Value("g-key".into())),
                base_url: None,
            },
        );
        assert_eq!(route.id, "google-interactions");
        assert_eq!(route.protocol.id(), "google-interactions");
        assert_eq!(
            route.endpoint.base_url.as_deref(),
            Some(oc_ai::protocols::google_interactions::DEFAULT_BASE_URL)
        );
        let mut request = oc_ai::schema::LLMRequest::new(
            route.model("gemini-3.8-flash", None).expect("model binds"),
        );
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(
            transport_request.url,
            "https://generativelanguage.googleapis.com/v1beta/interactions"
        );
        assert_eq!(transport_request.header("x-goog-api-key"), Some("g-key"));
    }

    #[test]
    fn provider_facade_unknown_provider_and_model() {
        let context = context();
        let catalog = Catalog::embedded();
        assert_eq!(
            language_model(&context, catalog, "nope", "m", &ModelConnection::default())
                .unwrap_err(),
            ProviderError::UnknownProvider("nope".into())
        );
        assert_eq!(
            language_model(&context, catalog, "anthropic", "nope", &ModelConnection::default())
                .unwrap_err(),
            ProviderError::UnknownModel("anthropic".into(), "nope".into())
        );
    }

    #[test]
    fn provider_facade_chat_family_bind() {
        let context = context();
        let catalog = Catalog::embedded();
        let deepseek = catalog.provider("deepseek").expect("deepseek present");
        let model_id =
            deepseek.models.first().expect("deepseek has models").id.as_str().to_string();
        let model =
            language_model(&context, catalog, "deepseek", &model_id, &ModelConnection::default())
                .expect("deepseek binds over its baked base URL");
        assert_eq!(model.provider.as_str(), "deepseek");
    }

    /// G-118: DigitalOcean binds its own chat route — the `cache_control`
    /// breakpoint lowering is enabled AND `apply_cache_policy` actually
    /// places hints (route id "digitalocean" hits RESPECTS_INLINE_HINTS;
    /// on the old generic "openai-chat" route the policy was a no-op and
    /// the dead snapshot entry never matched).
    #[test]
    fn provider_facade_digitalocean_cache_control_end_to_end() {
        let context = context();
        let catalog = Catalog::embedded();
        let provider = catalog.provider("digitalocean").expect("digitalocean in catalog");
        let model_id = provider.models.first().expect("has models").id.as_str().to_string();
        let model = language_model(
            &context,
            catalog,
            "digitalocean",
            &model_id,
            &ModelConnection { api_key: Some("do-key".into()), ..Default::default() },
        )
        .expect("digitalocean binds");
        assert_eq!(model.route.id, "digitalocean");
        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some("https://inference.do-ai.run/v1"),
            "the baked/catalog base, no user config needed"
        );
        let route = Arc::clone(&model.route);
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.system = vec![oc_ai::schema::SystemPart::new("You are concise.")];
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        // auto cache policy → hints on the system boundary
        let prepared = oc_ai::cache_policy::apply_cache_policy(&request);
        let body = route.build_body(&prepared).expect("body");
        let wire = serde_json::to_string(&body).expect("json");
        assert!(
            wire.contains("cache_control"),
            "cache_control breakpoints emitted on the digitalocean route: {wire}"
        );
        // G-121: digitalocean.ts:52 supportsPromptCacheKey
        let mut keyed = request.clone();
        keyed.prompt_cache_key = Some("session_do".into());
        let body = route.build_body(&keyed).expect("body");
        assert_eq!(body.get("prompt_cache_key"), Some(&serde_json::json!("session_do")));
    }

    /// G-118 second layer regression: the generic openai-chat route keeps
    /// cache_policy a no-op (OpenAI implicit prefix caching).
    #[test]
    fn provider_facade_generic_chat_route_has_no_inline_cache_hints() {
        let context = context();
        let catalog = Catalog::embedded();
        let provider = catalog.provider("togetherai").expect("togetherai in catalog");
        let model_id = provider.models.first().expect("has models").id.as_str().to_string();
        let model = language_model(
            &context,
            catalog,
            "togetherai",
            &model_id,
            &ModelConnection::default(),
        )
        .expect("togetherai binds");
        assert_eq!(model.route.id, "openai-chat");
        let route = Arc::clone(&model.route);
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.system = vec![oc_ai::schema::SystemPart::new("You are concise.")];
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = oc_ai::cache_policy::apply_cache_policy(&request);
        let body = route.build_body(&prepared).expect("body");
        let wire = serde_json::to_string(&body).expect("json");
        assert!(!wire.contains("cache_control"), "generic chat stays hint-free: {wire}");
    }

    /// G-119/G-120: deepinfra/togetherai/cerebras bind with zero config over
    /// their baked default base URLs (the snapshot has no `api` field for
    /// them; the old generic branch errored "needs a baseURL").
    #[test]
    fn provider_facade_deepinfra_togetherai_cerebras_baked_defaults() {
        let context = context();
        let catalog = Catalog::embedded();
        for (provider_id, base) in [
            ("deepinfra", "https://api.deepinfra.com/v1/openai"),
            ("togetherai", "https://api.together.xyz/v1"),
            ("cerebras", "https://api.cerebras.ai/v1"),
        ] {
            let provider = catalog.provider(provider_id).unwrap_or_else(|| {
                panic!("{provider_id} in catalog")
            });
            let model_id =
                provider.models.first().expect("has models").id.as_str().to_string();
            let model = language_model(
                &context,
                catalog,
                provider_id,
                &model_id,
                &ModelConnection::default(),
            )
            .unwrap_or_else(|error| panic!("{provider_id} binds over its baked base: {error}"));
            assert_eq!(
                model.route.endpoint.base_url.as_deref(),
                Some(base),
                "{provider_id} baked default"
            );
        }
    }

    /// G-119: deepinfra custom baseURLs get trailing slashes trimmed and
    /// `/openai` appended unless already suffixed (deepinfra.ts:38-44).
    #[test]
    fn provider_facade_deepinfra_base_url_normalization() {
        let context = context();
        let catalog = Catalog::embedded();
        let provider = catalog.provider("deepinfra").expect("deepinfra in catalog");
        let model_id = provider.models.first().expect("has models").id.as_str().to_string();
        for (input, expected) in [
            ("https://proxy.test/v1", "https://proxy.test/v1/openai"),
            ("https://proxy.test/v1/", "https://proxy.test/v1/openai"),
            ("https://proxy.test/v1///", "https://proxy.test/v1/openai"),
            ("https://proxy.test/v1/openai", "https://proxy.test/v1/openai"),
            ("https://proxy.test/v1/openai/", "https://proxy.test/v1/openai"),
        ] {
            let model = language_model(
                &context,
                catalog,
                "deepinfra",
                &model_id,
                &ModelConnection {
                    base_url: Some(input.to_string()),
                    ..Default::default()
                },
            )
            .expect("deepinfra binds");
            assert_eq!(
                model.route.endpoint.base_url.as_deref(),
                Some(expected),
                "{input} → {expected}"
            );
        }
    }

    /// G-121: cerebras `reasoningField: "reasoning"` + promptCacheKey reach
    /// the wire body via the facade-injected compatibility (cerebras.ts:41-48).
    #[test]
    fn provider_facade_cerebras_reasoning_field_and_prompt_cache_key() {
        let context = context();
        let catalog = Catalog::embedded();
        let provider = catalog.provider("cerebras").expect("cerebras in catalog");
        let model_id = provider.models.first().expect("has models").id.as_str().to_string();
        let model = language_model(
            &context,
            catalog,
            "cerebras",
            &model_id,
            &ModelConnection { api_key: Some("cb-key".into()), ..Default::default() },
        )
        .expect("cerebras binds");
        let compatibility = model.compatibility.as_ref().expect("compatibility injected");
        assert_eq!(compatibility.reasoning_field.as_deref(), Some("reasoning"));
        assert_eq!(compatibility.supports_prompt_cache_key, Some(true));
        let route = Arc::clone(&model.route);
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![
            oc_ai::schema::Message::user("hi"),
            oc_ai::schema::Message::assistant(vec![oc_ai::schema::ContentPart::Reasoning {
                text: "thinking it through".into(),
                encrypted: None,
                cache: None,
                metadata: None,
                provider_metadata: None,
            }]),
            oc_ai::schema::Message::user("and?"),
        ];
        request.prompt_cache_key = Some("session_cerebras".into());
        let body = route.build_body(&request).expect("body");
        let wire = serde_json::to_string(&body).expect("json");
        assert!(
            wire.contains("\"reasoning\":\"thinking it through\""),
            "assistant reasoning mapped to the cerebras `reasoning` field: {wire}"
        );
        assert!(
            !wire.contains("\"reasoning_content\""),
            "no deepseek-style reasoning field: {wire}"
        );
        assert_eq!(body.get("prompt_cache_key"), Some(&serde_json::json!("session_cerebras")));
        // store is stripped (supportsStore: false)
        assert!(body.get("store").is_none());
    }

    /// G-121: fireworks + deepinfra prompt cache keys (fireworks.ts:51,
    /// deepinfra.ts:51); deepinfra reasoning rides `reasoning_content`.
    #[test]
    fn provider_facade_fireworks_deepinfra_prompt_cache_key() {
        let context = context();
        let catalog = Catalog::embedded();
        for (provider_id, reasoning_field) in
            [("fireworks-ai", "reasoning_content"), ("deepinfra", "reasoning_content")]
        {
            // fireworks' package compat sets only promptCacheKey
            // (fireworks.ts:51); the `reasoning_content` field here comes
            // from the model's catalog `interleaved` overlay spread over
            // the package base (opencode.ts:270-271) — deepinfra's from
            // the package compat itself (deepinfra.ts:49).
            let provider = catalog.provider(provider_id).unwrap_or_else(|| {
                panic!("{provider_id} in catalog")
            });
            let model_id =
                provider.models.first().expect("has models").id.as_str().to_string();
            let model = language_model(
                &context,
                catalog,
                provider_id,
                &model_id,
                &ModelConnection::default(),
            )
            .unwrap_or_else(|error| panic!("{provider_id} binds: {error}"));
            let compatibility = model.compatibility.as_ref().expect("compatibility injected");
            assert_eq!(compatibility.supports_prompt_cache_key, Some(true), "{provider_id}");
            assert_eq!(compatibility.reasoning_field.as_deref(), Some(reasoning_field), "{provider_id}");
            let route = Arc::clone(&model.route);
            let mut request = oc_ai::schema::LLMRequest::new(model);
            request.messages = vec![oc_ai::schema::Message::user("hi")];
            request.prompt_cache_key = Some("session_key".into());
            let body = route.build_body(&request).expect("body");
            assert_eq!(
                body.get("prompt_cache_key"),
                Some(&serde_json::json!("session_key")),
                "{provider_id} prompt_cache_key in body"
            );
        }
    }

    /// G-123: the env fallback chains the snapshot doesn't carry — a
    /// togetherai key via TOGETHER_AI_API_KEY (not the catalog's
    /// TOGETHER_API_KEY) reaches the wire as the bearer.
    #[test]
    fn provider_facade_togetherai_env_fallback_chain() {
        std::env::set_var("TOGETHER_AI_API_KEY", "together-fallback-key");
        std::env::remove_var("TOGETHER_API_KEY");
        let context = context();
        let catalog = Catalog::embedded();
        let provider = catalog.provider("togetherai").expect("togetherai in catalog");
        let model_id = provider.models.first().expect("has models").id.as_str().to_string();
        let model = language_model(
            &context,
            catalog,
            "togetherai",
            &model_id,
            &ModelConnection::default(),
        )
        .expect("togetherai binds");
        let route = Arc::clone(&model.route);
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let body = route.build_body(&prepared).expect("body");
        let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
        assert_eq!(
            transport_request.header("authorization"),
            Some("Bearer together-fallback-key"),
            "the TOGETHER_AI_API_KEY fallback is consulted"
        );
        std::env::remove_var("TOGETHER_AI_API_KEY");
    }

    /// G-123 unit: the chain composition (catalog order first, hardcoded
    /// fallbacks appended, no duplicates).
    #[test]
    fn provider_facade_env_chain_composition() {
        assert_eq!(
            package_env_chain(
                "@opencode/ai/providers/togetherai",
                &["TOGETHER_API_KEY".to_string()]
            ),
            vec!["TOGETHER_API_KEY".to_string(), "TOGETHER_AI_API_KEY".to_string()]
        );
        assert_eq!(
            package_env_chain(
                "@opencode/ai/providers/digitalocean",
                &["DIGITALOCEAN_ACCESS_TOKEN".to_string()]
            ),
            vec![
                "DIGITALOCEAN_ACCESS_TOKEN".to_string(),
                "DIGITALOCEAN_API_KEY".to_string(),
                "DO_INFERENCE_API_KEY".to_string()
            ]
        );
        // a snapshot that already carries the full chain stays unchanged
        assert_eq!(
            package_env_chain(
                "@opencode/ai/providers/digitalocean",
                &[
                    "DIGITALOCEAN_ACCESS_TOKEN".to_string(),
                    "DIGITALOCEAN_API_KEY".to_string(),
                    "DO_INFERENCE_API_KEY".to_string()
                ]
            ),
            vec![
                "DIGITALOCEAN_ACCESS_TOKEN".to_string(),
                "DIGITALOCEAN_API_KEY".to_string(),
                "DO_INFERENCE_API_KEY".to_string()
            ]
        );
        assert_eq!(
            package_env_chain("@opencode/ai/providers/groq", &["GROQ_API_KEY".to_string()]),
            vec!["GROQ_API_KEY".to_string()]
        );
    }

    #[test]
    fn provider_facade_custom_provider() {
        let context = context();
        let custom = CustomProvider {
            id: "acme".into(),
            name: "Acme".into(),
            base_url: "https://api.acme.test/v1".into(),
            api_key: Some("secret".into()),
            headers: None,
            body: None,
            models: vec![CustomModel {
                id: "acme-large".into(),
                name: None,
                headers: None,
                body: None,
                ..CustomModel::default()
            }],
            ..CustomProvider::default()
        };
        let model =
            custom_language_model(&context, &custom, "acme-large").expect("custom provider binds");
        assert_eq!(model.id.as_str(), "acme-large");
        assert_eq!(model.provider.as_str(), "acme");
        assert_eq!(model.route.id, "openai-compatible-chat");
        assert_eq!(model.route.endpoint.base_url.as_deref(), Some("https://api.acme.test/v1"));
        assert!(
            custom_language_model(&context, &custom, "acme-small").is_err(),
            "unknown model fails to bind"
        );
    }

    /// Request-path parity #26: the config model `body` overlay must
    /// reach the WIRE body of every main request — v2 folds it into the
    /// route's HTTP body defaults (openai-compatible.ts:48-59
    /// `settings.body` → `http: { body }`), and the transport merges it
    /// over the protocol body at send time (route/transport/http.ts:41-49).
    #[test]
    fn provider_facade_custom_model_body_overlay_reaches_the_wire() {
        let context = context();
        let custom = CustomProvider {
            id: "acme".into(),
            name: "Acme".into(),
            base_url: "https://api.acme.test/v1".into(),
            api_key: Some("secret".into()),
            // Provider-level overlay under the model-level one (v2
            // core/src/model.ts:204-209): both land, model wins per key.
            headers: None,
            body: Some(BTreeMap::from([("top_k".to_string(), serde_json::json!(7))])),
            models: vec![CustomModel {
                id: "acme-large".into(),
                name: None,
                headers: None,
                body: Some(BTreeMap::from([("temperature".to_string(), serde_json::json!(0))])),
                ..CustomModel::default()
            }],
            ..CustomProvider::default()
        };
        let model =
            custom_language_model(&context, &custom, "acme-large").expect("custom provider binds");
        let route = Arc::clone(&model.route);
        assert_eq!(
            route.defaults.http.as_ref().and_then(|http| http.body.as_ref()),
            Some(&serde_json::Map::from_iter([
                ("top_k".to_string(), serde_json::json!(7)),
                ("temperature".to_string(), serde_json::json!(0)),
            ])),
            "route defaults carry the merged body overlay"
        );

        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let built = route.build_body(&prepared).expect("protocol body");
        let (_transport, body_text) = route.prepare_transport(&prepared, &built).expect("wire");
        let wire: serde_json::Value = serde_json::from_str(&body_text).expect("json wire body");
        assert_eq!(wire.get("temperature"), Some(&serde_json::json!(0)), "model body overlay");
        assert_eq!(wire.get("top_k"), Some(&serde_json::json!(7)), "provider body overlay");
        assert_eq!(wire.get("model"), Some(&serde_json::json!("acme-large")));
    }

    /// Same seam on the anthropic-compatible (catalog) path: the model
    /// `body` overlay rides `defaults.http.body` for EVERY protocol
    /// family (anthropic.ts:55-68 folds `settings.body` the same way)
    /// and overlays the anthropic messages body at send time.
    #[test]
    fn provider_facade_catalog_body_overlay_reaches_anthropic_wire() {
        let context = context();
        let catalog = Catalog::embedded();
        let mut info =
            catalog.model("anthropic", "claude-opus-4-5").expect("anthropic model info").clone();
        info.body = Some(BTreeMap::from([("temperature".to_string(), serde_json::json!(0))]));
        let bound = language_model(
            &context,
            catalog,
            "anthropic",
            "claude-opus-4-5",
            &ModelConnection {
                base_url: Some("https://api.anthropic.test".into()),
                api_key: Some("test-key".into()),
                headers: BTreeMap::new(),
            },
        )
        .expect("anthropic binds");
        let route = Arc::new(apply_model_overlays(&bound.route, &info));
        let model = route.model("claude-opus-4-5", None).expect("model binds");
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];
        let prepared = route.prepare_request(&request);
        let built = route.build_body(&prepared).expect("protocol body");
        let (_transport, body_text) = route.prepare_transport(&prepared, &built).expect("wire");
        let wire: serde_json::Value = serde_json::from_str(&body_text).expect("json wire body");
        assert_eq!(wire.get("temperature"), Some(&serde_json::json!(0)));
    }

    // =========================================================================
    // Native-protocol custom binding (provider/model `package`)
    // =========================================================================

    fn native_custom(package: Option<&str>, model_package: Option<&str>) -> CustomProvider {
        CustomProvider {
            id: "myai".into(),
            name: "My AI".into(),
            base_url: String::new(),
            api_key: Some("native-key".into()),
            package: package.map(str::to_string),
            models: vec![CustomModel {
                id: "my-model".into(),
                package: model_package.map(str::to_string),
                ..CustomModel::default()
            }],
            ..CustomProvider::default()
        }
    }

    /// The binding matrix: each native package family binds through the
    /// catalog family dispatch with the package's own default endpoint —
    /// no baseURL required (provider.ts:351 + loadPackage builtins,
    /// provider.ts:48-110).
    #[test]
    fn provider_facade_custom_native_binding_matrix() {
        let context = context();
        for (package, route_id, base) in [
            (
                "@opencode/ai/providers/anthropic",
                "anthropic-messages",
                "https://api.anthropic.com/v1",
            ),
            ("@opencode/ai/providers/openai/chat", "openai-chat", "https://api.openai.com/v1"),
            (
                "@opencode/ai/providers/openai/responses",
                "openai-responses",
                "https://api.openai.com/v1",
            ),
            (
                "@opencode/ai/providers/google",
                "gemini",
                "https://generativelanguage.googleapis.com/v1beta",
            ),
            (
                "@opencode/ai/providers/amazon-bedrock",
                "bedrock-converse",
                "https://bedrock-runtime.us-east-1.amazonaws.com",
            ),
        ] {
            let custom = native_custom(Some(package), None);
            let model =
                custom_language_model(&context, &custom, "my-model").expect("{package} binds");
            assert!(model.route.id.contains(route_id), "{package}: {}", model.route.id);
            let base_url = model.route.endpoint.base_url.as_deref().unwrap_or_default().to_string();
            assert!(base_url.starts_with(base), "{package}: {base_url}");
        }
    }

    /// npm aliases resolve through the same PACKAGES table (config
    /// `package: "@ai-sdk/anthropic"`, aisdk-native.ts:59-75) and the
    /// HOSTS redirects stay host-conditional (aisdk-native.ts:88-120):
    /// a `cohere`-id custom provider with `@ai-sdk/openai-compatible`
    /// takes the Cohere COMPATIBILITY lane, never generic chat.
    #[test]
    fn provider_facade_custom_native_npm_alias_and_host_redirect() {
        let context = context();
        let custom = native_custom(Some("@ai-sdk/anthropic"), None);
        let model = custom_language_model(&context, &custom, "my-model").expect("npm alias binds");
        assert!(model.route.id.contains("anthropic-messages"), "{}", model.route.id);

        let mut cohere = native_custom(Some("@ai-sdk/openai-compatible"), None);
        cohere.id = "cohere".into();
        let model =
            custom_language_model(&context, &cohere, "my-model").expect("host redirect binds");
        assert_eq!(model.route.id, "cohere-chat-completions", "lane-specific constructor");
        assert_eq!(
            model.route.endpoint.base_url.as_deref(),
            Some("https://api.cohere.ai/compatibility/v1")
        );
    }

    /// The model-level package overrides the provider's
    /// (provider.ts:351 `model.package ?? record.provider.package`).
    #[test]
    fn provider_facade_custom_model_package_overrides_provider() {
        let context = context();
        let custom = native_custom(
            Some("@opencode/ai/providers/anthropic"),
            Some("@opencode/ai/providers/openai/responses"),
        );
        let model = custom_language_model(&context, &custom, "my-model").expect("binds");
        assert!(model.route.id.contains("openai-responses"), "{}", model.route.id);
    }

    /// Without a package the openai-compatible fallback stays (with the
    /// baseURL requirement and anonymous/bearer auth).
    #[test]
    fn provider_facade_custom_without_package_stays_openai_compatible() {
        let context = context();
        let mut custom = native_custom(None, None);
        custom.base_url = "https://api.acme.test/v1".into();
        let model = custom_language_model(&context, &custom, "my-model").expect("fallback binds");
        assert_eq!(model.route.id, "openai-compatible-chat");
        assert_eq!(model.route.endpoint.base_url.as_deref(), Some("https://api.acme.test/v1"));
    }

    /// Unknown and `aisdk:` packages fail with the precise typed error
    /// naming the package (the npm-install bridge stays unported —
    /// registered exemption).
    #[test]
    fn provider_facade_custom_unknown_package_precise_error() {
        let context = context();
        for package in ["aisdk:@ai-sdk/perplexity", "not-a-pkg", "@ai-sdk/unknown"] {
            let custom = native_custom(Some(package), None);
            assert_eq!(
                custom_language_model(&context, &custom, "my-model").unwrap_err(),
                ProviderError::UnsupportedProtocol {
                    provider: "myai".into(),
                    package: package.into(),
                },
                "{package}"
            );
        }
    }

    /// Azure needs the customer's resource URL (azure.ts:22-23) — a
    /// package binding without an endpoint is a configuration error,
    /// never a silent api.openai.com misroute.
    #[test]
    fn provider_facade_custom_azure_requires_base_url() {
        let context = context();
        let custom = native_custom(Some("@opencode/ai/providers/azure"), None);
        let error = custom_language_model(&context, &custom, "my-model").unwrap_err();
        assert!(matches!(error, ProviderError::Configuration { .. }), "{error}");
        assert!(error.to_string().contains("needs a baseURL"), "{error}");
    }

    /// Credential/Core keys never reach native provider options
    /// (provider.ts:141-151 CORE_KEYS, model-resolver.ts:226 credential
    /// omit) — arbitrary settings ride through stripped.
    #[test]
    fn provider_facade_native_settings_strips_core_and_credentials() {
        let mut rest = BTreeMap::new();
        rest.insert("apiKey".to_string(), serde_json::json!("s3cret"));
        rest.insert("authToken".to_string(), serde_json::json!("tok"));
        rest.insert("accessToken".to_string(), serde_json::json!("acc"));
        rest.insert("fetch".to_string(), serde_json::json!(false));
        rest.insert("transport".to_string(), serde_json::json!("x"));
        rest.insert("reasoningEffort".to_string(), serde_json::json!("high"));
        let stripped = native_settings(&rest);
        assert_eq!(
            stripped,
            serde_json::Map::from_iter([(
                "reasoningEffort".to_string(),
                serde_json::json!("high")
            )])
        );
        // And the fold into route defaults keeps the strip.
        let catalog = Catalog::embedded();
        let mut info =
            catalog.model("anthropic", "claude-opus-4-5").expect("anthropic model info").clone();
        info.settings =
            Some(oc_schema::model::Settings { rest: rest.clone(), ..Default::default() });
        let route = apply_model_overlays(
            &openai_chat_route(Arc::new(oc_ai::route::transport::FailingTransport {
                message: "facade unit test",
            })),
            &info,
        );
        assert_eq!(
            route.defaults.provider_options,
            Some(stripped),
            "credentials and Core keys never ride provider options"
        );
    }

    /// Builtin-table reconciliation snapshot (provider.ts:48-110): every
    /// ref builtin entrypoint must classify into a ported family — no
    /// silent gap between the ref load table and `family_of`.
    #[test]
    fn provider_facade_builtin_table_snapshot() {
        use crate::provider_catalog::native::BUILTIN_ENTRYPOINTS;
        use std::collections::BTreeSet;
        // The exact ref builtin table (provider.ts builtins map keys).
        assert_eq!(BUILTIN_ENTRYPOINTS.len(), 52, "ref table has 52 entrypoints");
        let mut seen = BTreeSet::new();
        for package in BUILTIN_ENTRYPOINTS {
            seen.insert(*package);
            let family = family_of(package)
                .unwrap_or_else(|| panic!("builtin `{package}` has no ported family"));
            // every family a builtin can reach (the full ported dispatch)
            assert!(
                matches!(
                    family,
                    ProtocolFamily::AnthropicMessages
                        | ProtocolFamily::OpenAIChat
                        | ProtocolFamily::OpenAIResponses
                        | ProtocolFamily::Gemini
                        | ProtocolFamily::OpenAICompatible
                        | ProtocolFamily::CohereChat
                        | ProtocolFamily::MistralChat
                        | ProtocolFamily::BedrockConverse
                        | ProtocolFamily::BedrockMantleChat
                        | ProtocolFamily::BedrockMantleResponses
                        | ProtocolFamily::BedrockMantleMessages
                        | ProtocolFamily::VertexGemini
                        | ProtocolFamily::VertexMessages
                        | ProtocolFamily::VertexChat
                        | ProtocolFamily::VertexResponses
                        | ProtocolFamily::XaiResponses
                        | ProtocolFamily::GoogleInteractions
                        | ProtocolFamily::AlibabaResponses
                        | ProtocolFamily::MetaResponses
                        | ProtocolFamily::VercelGateway
                ),
                "`{package}` maps to a real family"
            );
        }
        assert_eq!(seen.len(), 52, "no duplicates");
        // The reconciliation additions (previously UnsupportedProtocol):
        assert_eq!(
            family_of("@opencode/ai/providers/azure"),
            Some(ProtocolFamily::OpenAIResponses),
            "azure.ts:72 defaults to the responses route"
        );
        assert_eq!(
            family_of("@opencode/ai/providers/google-vertex/gemini"),
            Some(ProtocolFamily::VertexGemini),
            "google-vertex/gemini.ts:1 re-exports the vertex model"
        );
        // G-84: alibaba/responses is the Model Studio responses dialect,
        // not the plain OpenAI Responses family.
        assert_eq!(
            family_of("@opencode/ai/providers/alibaba/responses"),
            Some(ProtocolFamily::AlibabaResponses),
            "alibaba responsesRoute uses the AlibabaResponses protocol"
        );
        // G-84 sibling: meta/responses is the Meta responses dialect
        // (native.meta hosted tools + signed-ID image replay), not the
        // plain OpenAI Responses family.
        assert_eq!(
            family_of("@opencode/ai/providers/meta/responses"),
            Some(ProtocolFamily::MetaResponses),
            "meta responsesRoute uses the MetaResponses protocol"
        );
        // G-81: the gateway dispatches per model family over the
        // gateway-wrapped Messages/Responses/Chat routes.
        assert_eq!(
            family_of("@opencode/ai/providers/vercel-ai-gateway"),
            Some(ProtocolFamily::VercelGateway),
            "vercel-ai-gateway routes through the gateway protocol wrapper"
        );
        assert_eq!(
            family_of("@opencode/ai/providers/openai/chat"),
            Some(ProtocolFamily::OpenAIChat),
            "openai/chat.ts:1 re-exports the chat model"
        );
        assert_eq!(
            family_of("@opencode/ai/providers/openai/responses"),
            Some(ProtocolFamily::OpenAIResponses),
            "openai/responses.ts:1 re-exports the responses model"
        );
    }

    // =========================================================================
    // Model settings beyond baseURL (F1) + azure useCompletionUrls (F2)
    // =========================================================================

    fn settings_model(rest: &[(&str, Value)]) -> CustomModel {
        CustomModel {
            id: "my-model".into(),
            settings: Some(oc_schema::model::Settings {
                compaction: None,
                rest: rest.iter().map(|(key, value)| (key.to_string(), value.clone())).collect(),
            }),
            ..CustomModel::default()
        }
    }

    /// F1: `models.<id>.settings` beyond baseURL reach native provider
    /// options in BOTH custom lanes (opencode.ts:277-280 →
    /// model-resolver.ts:220-226), credential-stripped
    /// (withoutCredentials, opencode.ts:547-551) and CORE_KEYS-stripped
    /// at bind (provider.ts:141-151); baseURL never re-rides the options
    /// (consumed by the endpoint ladder, opencode.ts:276).
    #[test]
    fn provider_facade_custom_settings_reach_provider_options_both_lanes() {
        let context = context();
        let rest = vec![
            ("reasoningEffort", serde_json::json!("high")),
            ("temperature", serde_json::json!(0.2)),
            ("useCompletionUrls", serde_json::json!(false)),
            ("apiKey", serde_json::json!("s3cret")),
            ("authToken", serde_json::json!("tok")),
            ("accessToken", serde_json::json!("acc")),
            ("fetch", serde_json::json!(true)),
            ("compaction", serde_json::json!("drop")),
            ("timeout", serde_json::json!(1000)),
            ("baseURL", serde_json::json!("https://ignored.test/v1")),
        ];
        let expected = serde_json::Map::from_iter([
            ("reasoningEffort".to_string(), serde_json::json!("high")),
            ("temperature".to_string(), serde_json::json!(0.2)),
            ("useCompletionUrls".to_string(), serde_json::json!(false)),
        ]);

        // native lane
        let custom = CustomProvider {
            id: "myai".into(),
            name: "My AI".into(),
            package: Some("@opencode/ai/providers/anthropic".into()),
            models: vec![settings_model(&rest)],
            ..CustomProvider::default()
        };
        let model = custom_language_model(&context, &custom, "my-model").expect("binds");
        assert!(model.route.id.contains("anthropic-messages"));
        assert_eq!(
            Arc::clone(&model.route).defaults.provider_options,
            Some(expected.clone()),
            "native lane: settings reach provider options stripped"
        );

        // openai-compatible fallback lane
        let custom = CustomProvider {
            id: "acme".into(),
            name: "Acme".into(),
            base_url: "https://api.acme.test/v1".into(),
            models: vec![settings_model(&rest)],
            ..CustomProvider::default()
        };
        let model = custom_language_model(&context, &custom, "my-model").expect("binds");
        assert_eq!(model.route.id, "openai-compatible-chat");
        assert_eq!(
            Arc::clone(&model.route).defaults.provider_options,
            Some(expected),
            "fallback lane: settings reach provider options stripped"
        );
    }

    /// F2: `useCompletionUrls: true` switches the azure package to the
    /// chat-completions URL lane exactly like the completions shape
    /// (aisdk-native.ts:126-127 azureChat — shape OR settings — :44-45).
    #[test]
    fn provider_facade_custom_azure_use_completion_urls_switches_lane() {
        let context = context();
        let base = "https://res.openai.azure.com/openai";
        let wire_url = |settings: &[(&str, Value)]| {
            let custom = CustomProvider {
                id: "azureish".into(),
                name: "Azure".into(),
                base_url: base.into(),
                api_key: Some("azure-key".into()),
                package: Some("@ai-sdk/azure".into()),
                models: vec![settings_model(settings)],
                ..CustomProvider::default()
            };
            let model = custom_language_model(&context, &custom, "my-model").expect("binds");
            let route = Arc::clone(&model.route);
            let mut request = oc_ai::schema::LLMRequest::new(model);
            request.messages = vec![oc_ai::schema::Message::user("hi")];
            let prepared = route.prepare_request(&request);
            let body = route.build_body(&prepared).expect("body");
            let (transport_request, _) = route.prepare_transport(&prepared, &body).expect("wire");
            (route.id.clone(), transport_request.url.clone())
        };
        // default: the responses package → /responses URLs
        let (id, url) = wire_url(&[]);
        assert_eq!(id, "openai-responses");
        assert!(url.ends_with("/responses"), "{url}");
        // useCompletionUrls: true → azure/chat package → /chat/completions
        let (id, url) = wire_url(&[("useCompletionUrls", serde_json::json!(true))]);
        assert_eq!(id, "openai-chat", "the settings arm switched the package");
        assert!(url.ends_with("/chat/completions"), "{url}");
    }
}
