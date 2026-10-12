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
use oc_ai::schema::options::{merge_json_records, merge_string_records, HttpOptions};
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
