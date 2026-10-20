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

use serde_json::Value;

use oc_ai::protocols::anthropic_messages::{anthropic_messages_route, anthropic_route};
use oc_ai::protocols::bedrock::{self as bedrock_lane, BedrockAuthOptions};
use oc_ai::protocols::cohere_chat::{cohere_route_for_package, CohereConfig};
use oc_ai::protocols::copilot::{copilot_chat_route, CopilotConfig};
use oc_ai::protocols::gemini::{google_route, GoogleConfig};
use oc_ai::protocols::google_interactions::{google_interactions_route, GoogleInteractionsConfig};
use oc_ai::protocols::mistral_chat::{mistral_route_for_package, MistralConfig};
use oc_ai::protocols::openai_chat::{
    alibaba_chat_route, deepseek_chat_route, groq_chat_route, openai_chat_route,
    openrouter_chat_route, venice_chat_route, zai_chat_route,
};
use oc_ai::protocols::openai_compatible::{
    openai_compatible_chat_configured, openai_compatible_responses_route,
};
use oc_ai::protocols::openai_responses::{openai_responses_route, OpenAIResponsesRouteSpec};
use oc_ai::protocols::vertex::{vertex_route_for_model, VertexConfig};
use oc_ai::protocols::xai_responses::{xai_auth, xai_responses_route, XaiResponsesRouteSpec};
use oc_ai::route::auth::{self, Secret};
use oc_ai::route::client::{Route, RouteDefaults, RoutePatch};
use oc_ai::route::endpoint::EndpointPatch;
use oc_ai::route::{AuthDefinition, SharedTransport};
use oc_ai::schema::options::{merge_json_records, merge_string_records, HttpOptions};
use oc_ai::LanguageModel;
use oc_schema::model::Info as ModelInfo;
use oc_schema::provider::Settings as ProviderSettings;

use crate::provider_catalog::native::package_name;
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
/// (the process-agnostic seam, oracle gate 2 finding 7).
pub struct FacadeContext {
    pub transport: SharedTransport,
}

impl FacadeContext {
    /// Context over one shared transport.
    pub fn new(transport: SharedTransport) -> Self {
        Self { transport }
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

/// One config-defined model entry (`providers.<id>.models.<model>`),
/// keeping the per-model overlays v2 merges onto the effective model
/// (core/src/model.ts:204-209 — provider overlays first, model overlays
/// win per key).
#[derive(Debug, Clone)]
pub struct CustomModel {
    pub id: String,
    /// The config display name (`models.<id>.name`,
    /// config/plugin/provider.ts:112).
    pub name: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub body: Option<BTreeMap<String, Value>>,
}

/// A user-defined custom provider (v2 `provider` config custom entries):
/// always bound through the openai-compatible facade.
#[derive(Debug, Clone)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Provider-level overlays (`providers.<id>.headers|body`) — merged
    /// under every model's own overlays.
    pub headers: Option<BTreeMap<String, String>>,
    pub body: Option<BTreeMap<String, Value>>,
    pub models: Vec<CustomModel>,
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
    /// Vertex native: Gemini generateContent on aiplatform.
    VertexGemini,
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
}

/// Classify a native package into a ported family (`None` → unsupported).
#[must_use]
pub fn family_of(package: &str) -> Option<ProtocolFamily> {
    Some(match package {
        "@opencode/ai/providers/anthropic"
        | "@opencode/ai/providers/google-vertex/messages"
        | "@opencode/ai/providers/alibaba/messages"
        | "@opencode/ai/providers/meta/messages"
        | "@opencode/ai/providers/minimax/messages"
        | "@opencode/ai/providers/moonshot/messages"
        | "@opencode/ai/providers/zai-coding-plan/messages" => ProtocolFamily::AnthropicMessages,

        "@opencode/ai/providers/openai"
        | "@opencode/ai/providers/azure/responses"
        | "@opencode/ai/providers/alibaba/responses"
        | "@opencode/ai/providers/meta/responses"
        | "@opencode/ai/providers/minimax/responses"
        | "@opencode/ai/providers/moonshot/responses"
        | "@opencode/ai/providers/zai-coding-plan/responses" => ProtocolFamily::OpenAIResponses,

        // xAI (AI-M5): the native `xai-responses` protocol — the Open
        // Responses baseline with the Grok hosted tool vocabulary and the
        // `xai` metadata namespace (previously misrouted to the plain
        // OpenAI Responses family).
        "@opencode/ai/providers/xai" => ProtocolFamily::XaiResponses,

        "@opencode/ai/providers/azure/chat"
        | "@opencode/ai/providers/google-vertex/chat"
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
        | "@opencode/ai/providers/cloudflare-ai-gateway"
        | "@opencode/ai/providers/vercel-ai-gateway" => ProtocolFamily::OpenAIChat,

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
        "@opencode/ai/providers/google-vertex" => ProtocolFamily::VertexGemini,
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

    let route =
        build_route(context, family, provider_id, &package, base_url, env, connection, model)?;
    // Connection headers ride the route defaults (transport headers) —
    // the seam credential-driven providers (copilot) use for their
    // client headers; model overlays apply after and win per key.
    let route = if connection.headers.is_empty() {
        route
    } else {
        route.with(RoutePatch {
            defaults: RouteDefaults {
                headers: Some(connection.headers.clone()),
                ..Default::default()
            },
            ..Default::default()
        })
    };
    let route = apply_model_overlays(&route, model);
    route.model(model.id.as_str(), None).map_err(|error| ProviderError::Configuration {
        provider: provider_id.to_string(),
        message: error.to_string(),
    })
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
        ProtocolFamily::OpenAIChat => {
            let auth = bearer_auth(api_key, env);
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
                _ => {
                    // generic chat: the provider's own endpoint is required
                    // (falling back to the OpenAI default URL would silently
                    // send another provider's traffic to api.openai.com)
                    let base = base_url.clone().ok_or_else(|| ProviderError::Configuration {
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
    )
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
        provider_options.extend(settings.rest.clone());
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

/// Bind a user-defined custom provider (config `provider.<id>` with
/// `baseURL`/`apiKey`/npm-less models) through the openai-compatible
/// facade. The provider- and model-level `headers`/`body` overlays ride
/// the route defaults (transport headers + HTTP body overlay), the seam
/// v2's own packages use for config overlays.
pub fn custom_language_model(
    context: &FacadeContext,
    custom: &CustomProvider,
    model_id: &str,
) -> Result<LanguageModel, ProviderError> {
    let Some(model) = custom.models.iter().find(|model| model.id == model_id) else {
        return Err(ProviderError::UnknownModel(custom.id.clone(), model_id.to_string()));
    };
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
    let headers = merge_string_records(&[custom.headers.as_ref(), model.headers.as_ref()]);
    let http = HttpOptions { body, ..Default::default() };
    let route = openai_compatible_chat_configured(
        Arc::clone(&context.transport),
        &custom.id,
        &custom.base_url,
    )
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
            }],
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
            }],
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
}
