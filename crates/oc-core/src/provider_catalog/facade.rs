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
//! - everything whose protocol is not ported (Bedrock, Vertex native, the
//!   `aisdk:` shims) → the typed [`ProviderError::UnsupportedProtocol`]
//!   clean error (oracle gate 3, Task 5 acceptance).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use oc_ai::protocols::anthropic_messages::{anthropic_messages_route, anthropic_route};
use oc_ai::protocols::gemini::{google_route, GoogleConfig};
use oc_ai::protocols::openai_chat::{
    alibaba_chat_route, deepseek_chat_route, groq_chat_route, openai_chat_route,
    openrouter_chat_route, venice_chat_route, zai_chat_route,
};
use oc_ai::protocols::openai_compatible::openai_compatible_chat_configured;
use oc_ai::protocols::openai_responses::{openai_responses_route, OpenAIResponsesRouteSpec};
use oc_ai::route::auth::{self, Secret};
use oc_ai::route::client::{Route, RouteDefaults, RoutePatch};
use oc_ai::route::endpoint::EndpointPatch;
use oc_ai::route::{AuthDefinition, SharedTransport};
use oc_ai::LanguageModel;
use oc_schema::model::Info as ModelInfo;
use oc_schema::provider::Settings as ProviderSettings;

use crate::provider_catalog::Catalog;

/// Typed provider-binding failures (gate 3, Task 5: selecting a
/// catalog-listed but unported provider must fail cleanly).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProviderError {
    /// The provider's protocol is not ported to oc-ai (bedrock, vertex, and
    /// the `aisdk:` shims for now).
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

/// A user-defined custom provider (v2 `provider` config custom entries):
/// always bound through the openai-compatible facade.
#[derive(Debug, Clone)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<String>,
}

/// The ported protocol families a catalog package can bind to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolFamily {
    AnthropicMessages,
    OpenAIChat,
    OpenAIResponses,
    Gemini,
    OpenAICompatible,
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
        | "@opencode/ai/providers/zai-coding-plan/responses"
        | "@opencode/ai/providers/xai" => ProtocolFamily::OpenAIResponses,

        "@opencode/ai/providers/azure/chat"
        | "@opencode/ai/providers/google-vertex/chat"
        | "@opencode/ai/providers/alibaba/chat"
        | "@opencode/ai/providers/baseten"
        | "@opencode/ai/providers/cerebras"
        | "@opencode/ai/providers/cohere/chat"
        | "@opencode/ai/providers/deepinfra"
        | "@opencode/ai/providers/deepseek"
        | "@opencode/ai/providers/digitalocean"
        | "@opencode/ai/providers/fireworks"
        | "@opencode/ai/providers/groq"
        | "@opencode/ai/providers/meta/chat"
        | "@opencode/ai/providers/minimax/chat"
        | "@opencode/ai/providers/mistral"
        | "@opencode/ai/providers/moonshot/chat"
        | "@opencode/ai/providers/togetherai"
        | "@opencode/ai/providers/venice"
        | "@opencode/ai/providers/zai/chat"
        | "@opencode/ai/providers/zai-coding-plan/chat"
        | "@opencode/ai/providers/openrouter"
        | "@opencode/ai/providers/cloudflare-workers-ai"
        | "@opencode/ai/providers/cloudflare-ai-gateway"
        | "@opencode/ai/providers/vercel-ai-gateway" => ProtocolFamily::OpenAIChat,

        "@opencode/ai/providers/google" => ProtocolFamily::Gemini,
        "@opencode/ai/providers/openai-compatible" => ProtocolFamily::OpenAICompatible,

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
    let package = model.package.as_deref().unwrap_or(&snapshot.info.package);
    let family = family_of(package).ok_or_else(|| ProviderError::UnsupportedProtocol {
        provider: provider_id.to_string(),
        package: package.to_string(),
    })?;

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

    let route = build_route(context, family, provider_id, package, base_url, env, connection)?;
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

fn build_route(
    context: &FacadeContext,
    family: ProtocolFamily,
    provider_id: &str,
    package: &str,
    base_url: Option<String>,
    env: &[String],
    connection: &ModelConnection,
) -> Result<Route, ProviderError> {
    let transport = Arc::clone(&context.transport);
    let api_key = connection.secret();
    match family {
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

/// Fold `Model.Info` overlays (settings.rest + body + headers — e.g.
/// experimental mode models) into the route defaults.
fn apply_model_overlays(route: &Route, model: &ModelInfo) -> Route {
    let mut provider_options = serde_json::Map::new();
    if let Some(settings) = &model.settings {
        provider_options.extend(settings.rest.clone());
    }
    if let Some(body) = &model.body {
        provider_options.extend(body.clone());
    }
    let headers = model.headers.clone();
    route.with(RoutePatch {
        defaults: RouteDefaults {
            headers,
            provider_options: (!provider_options.is_empty()).then_some(provider_options),
            ..Default::default()
        },
        ..Default::default()
    })
}

/// Bind a user-defined custom provider (config `provider.<id>` with
/// `baseURL`/`apiKey`/npm-less models) through the openai-compatible
/// facade.
pub fn custom_language_model(
    context: &FacadeContext,
    custom: &CustomProvider,
    model_id: &str,
) -> Result<LanguageModel, ProviderError> {
    if !custom.models.iter().any(|id| id == model_id) {
        return Err(ProviderError::UnknownModel(custom.id.clone(), model_id.to_string()));
    }
    let route = openai_compatible_chat_configured(
        Arc::clone(&context.transport),
        &custom.id,
        &custom.base_url,
    )
    .with(RoutePatch {
        auth: Some(bearer_auth(custom.api_key.clone().map(Secret::Value), &[])),
        ..Default::default()
    });
    route.model(model_id, None).map_err(|error| ProviderError::Configuration {
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
        // bedrock: catalog-listed, protocol not ported
        let bedrock = catalog.provider("amazon-bedrock").expect("bedrock in catalog");
        let model_id = bedrock.models.first().expect("bedrock has models").id.as_str().to_string();
        let error = language_model(
            &context,
            catalog,
            "amazon-bedrock",
            &model_id,
            &ModelConnection::default(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ProviderError::UnsupportedProtocol {
                provider: "amazon-bedrock".into(),
                package: bedrock
                    .models
                    .first()
                    .and_then(|m| m.package.clone())
                    .unwrap_or_else(|| bedrock.info.package.clone())
            },
            "clean typed error for unported protocol"
        );
        assert!(error.to_string().contains("no ported protocol"));
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
            models: vec!["acme-large".into()],
        };
        let model =
            custom_language_model(&context, &custom, "acme-large").expect("custom provider binds");
        assert_eq!(model.id.as_str(), "acme-large");
        assert_eq!(model.provider.as_str(), "acme");
        assert_eq!(model.route.id, "openai-compatible-chat");
        assert_eq!(model.route.endpoint.base_url.as_deref(), Some("https://api.acme.test/v1"));
        assert_eq!(
            custom_language_model(&context, &custom, "acme-small").unwrap_err(),
            ProviderError::UnknownModel("acme".into(), "acme-small".into())
        );
    }
}
