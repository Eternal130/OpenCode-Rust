//! Google Vertex AI native (port of `providers/google-vertex.ts` +
//! `google-vertex-shared.ts`): the Gemini generateContent protocol deployed
//! on Vertex — project/region routing, tuned `endpoints/` models, Express
//! Mode API keys, ADC bearer auth, the `labels` provider option, and the
//! `serviceTier` request-type header (stripped from the body like v2's
//! `fromRequest`) — plus the OpenAI-compatibility deployments
//! (`google-vertex-chat` / `google-vertex-responses`, AI-V1) on the
//! aiplatform `endpoints/openapi` root with ADC auth and `${GOOGLE_VERTEX_*}`
//! catalog baseURL template expansion.

use std::sync::Arc;

use serde_json::Value;

use crate::protocols::gemini::GeminiProtocol;
use crate::protocols::open_responses::{
    BaseAdapter, OpenResponsesProtocol, PATH as RESPONSES_PATH,
};
use crate::protocols::openai_chat::{OpenAiChat, OpenAiChatExt, PATH as CHAT_PATH};
use crate::route::auth::{self, Definition, Secret};
use crate::route::client::{Route, RouteDefaults, RouteInput};
use crate::route::endpoint::Endpoint;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::{Framing, SharedTransport};
use crate::schema::options::SanitizerCompatibility;
use crate::schema::{AiError, AiErrorReason, LLMEvent, LLMRequest, ProviderId};

const ADAPTER: &str = "google-vertex-gemini";
const PROVIDER: &str = "google-vertex";
/// `https://www.googleapis.com/auth/cloud-platform` — the one scope v2
/// requests through ADC.
pub const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

// ===========================================================================
// Project / location routing (port of google-vertex-shared.ts)
// ===========================================================================

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

/// Project resolution (port of `GoogleVertexShared.project`): option, then
/// the provider aliases `GOOGLE_VERTEX_PROJECT` / `GOOGLE_CLOUD_PROJECT` /
/// `GCP_PROJECT` / `GCLOUD_PROJECT`.
#[must_use]
pub fn resolve_project(option: Option<&str>) -> Option<String> {
    option
        .map(str::to_string)
        .or_else(|| env_non_empty("GOOGLE_VERTEX_PROJECT"))
        .or_else(|| env_non_empty("GOOGLE_CLOUD_PROJECT"))
        .or_else(|| env_non_empty("GCP_PROJECT"))
        .or_else(|| env_non_empty("GCLOUD_PROJECT"))
}

/// Location resolution (port of `GoogleVertexShared.location`): option,
/// then `GOOGLE_VERTEX_LOCATION` / `GOOGLE_CLOUD_LOCATION` /
/// `VERTEX_LOCATION`, else the fallback.
#[must_use]
pub fn resolve_location(option: Option<&str>, fallback: &str) -> String {
    option
        .map(str::to_string)
        .or_else(|| env_non_empty("GOOGLE_VERTEX_LOCATION"))
        .or_else(|| env_non_empty("GOOGLE_CLOUD_LOCATION"))
        .or_else(|| env_non_empty("VERTEX_LOCATION"))
        .unwrap_or_else(|| fallback.to_string())
}

/// Endpoint host for one location (port of `GoogleVertexShared.host`):
/// `global` is the multi-region root, `eu`/`us` use the Regional Endpoint
/// Platform domains, everything else is `{location}-aiplatform`.
#[must_use]
pub fn host(location: &str) -> String {
    match location {
        "global" => "aiplatform.googleapis.com".to_string(),
        "eu" | "us" => format!("aiplatform.{location}.rep.googleapis.com"),
        other => format!("{other}-aiplatform.googleapis.com"),
    }
}

/// Whether the model id addresses a tuned/model-garden endpoint (v2
/// `String(modelID).startsWith("endpoints/")`).
#[must_use]
pub fn is_tuned_model(model_id: &str) -> bool {
    model_id.starts_with("endpoints/")
}

/// The Vertex base URL for one binding (port of `configuredRoute`): the
/// Express Mode root when only an API key is used (tuned endpoints reject
/// Express keys upstream), else the regional `v1beta1` project root with
/// `/publishers/google` omitted for tuned endpoints.
#[must_use]
pub fn endpoint_base_url(
    project: Option<&str>,
    location: &str,
    api_key: bool,
    tuned: bool,
) -> String {
    if api_key {
        return "https://aiplatform.googleapis.com/v1/publishers/google".to_string();
    }
    let project = project.unwrap_or_default();
    let publishers = if tuned { "" } else { "/publishers/google" };
    format!(
        "https://{}/v1beta1/projects/{project}/locations/{location}{publishers}",
        host(location)
    )
}

// ===========================================================================
// Auth (port of GoogleVertexShared.oauth / apiKey / adc)
// ===========================================================================

/// ADC token sourcing, sync slice (port of `adc`): the explicit access
/// token, then the `GOOGLE_OAUTH_ACCESS_TOKEN` env — the ADC stand-in
/// gCloud/Cloud Run/Cloud Code inject; the authorized-user file refresh
/// lands as the oc-core integration credential binding.
pub fn adc_access_token(explicit: Option<Secret>) -> Definition {
    match explicit {
        Some(secret) => Definition::bearer(secret),
        None => auth::config("GOOGLE_OAUTH_ACCESS_TOKEN").bearer(),
    }
}

/// The `x-goog-api-key` Express Mode header chain (port of `apiKey`).
pub fn express_api_key(api_key: Option<Secret>) -> Definition {
    auth::or_else(&auth::optional(api_key), &auth::config("GOOGLE_VERTEX_API_KEY"))
        .header("x-goog-api-key")
}

// ===========================================================================
// Protocol: Gemini + labels, minus serviceTier (port of fromRequest)
// ===========================================================================

struct VertexGeminiProtocol {
    gemini: GeminiProtocol,
}

impl Protocol for VertexGeminiProtocol {
    fn id(&self) -> &str {
        ADAPTER
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        let mut body = self.gemini.body_from(request)?;
        if let Value::Object(object) = &mut body {
            // serviceTier rides the request-type header instead.
            if let Some(config) = object.get_mut("generationConfig").and_then(Value::as_object_mut)
            {
                config.remove("serviceTier");
            }
            object.remove("serviceTier");
            if let Some(labels) = request
                .provider_options
                .as_ref()
                .and_then(|options| options.get("labels"))
                .and_then(Value::as_object)
            {
                let labels: serde_json::Map<String, Value> = labels
                    .iter()
                    .filter(|(_, value)| value.is_string())
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                if !labels.is_empty() {
                    object.insert("labels".into(), Value::Object(labels));
                }
            }
        }
        Ok(body)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        self.gemini.initial(request)
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        self.gemini.step(state, frame)
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        self.gemini.on_halt(state)
    }

    fn sanitizer(&self) -> Option<SanitizerCompatibility> {
        self.gemini.sanitizer()
    }
}

// ===========================================================================
// Route
// ===========================================================================

/// Vertex route configuration (port of `google-vertex.ts` `Config`).
#[derive(Default)]
pub struct VertexConfig {
    pub project: Option<String>,
    pub location: Option<String>,
    pub access_token: Option<Secret>,
    pub api_key: Option<Secret>,
    pub base_url: Option<String>,
}

/// The configured Vertex Gemini route (port of `configuredRoute` +
/// `route`): the endpoint embeds project/location (Express Mode when an API
/// key resolves), tuned `endpoints/` models address the project root
/// directly, auth is the ADC bearer chain or the Express API-key header,
/// and the `serviceTier` provider option rides the request-type header.
#[must_use]
pub fn vertex_route_for_model(
    transport: SharedTransport,
    config: VertexConfig,
    model_id: &str,
) -> Route {
    let tuned = is_tuned_model(model_id);
    let api_key = config
        .api_key
        .clone()
        .filter(|key| !matches!(key, Secret::Value(value) if value.trim().is_empty()));
    let location = resolve_location(config.location.as_deref(), "us-central1");
    let base_url = config.base_url.clone().unwrap_or_else(|| {
        if api_key.is_some() {
            endpoint_base_url(None, &location, true, tuned)
        } else {
            let project = resolve_project(config.project.as_deref());
            endpoint_base_url(project.as_deref(), &location, false, tuned)
        }
    });
    let auth_definition = if api_key.is_some() {
        express_api_key(api_key)
    } else {
        adc_access_token(config.access_token.clone())
    };
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some("vertex".to_string()),
        protocol: Arc::new(VertexGeminiProtocol { gemini: GeminiProtocol }),
        endpoint: Endpoint::dynamic_path(|input| {
            let model = input.request.model.id.as_str();
            if is_tuned_model(model) {
                format!("/{model}:streamGenerateContent?alt=sse")
            } else {
                format!("/models/{model}:streamGenerateContent?alt=sse")
            }
        })
        .with_base_url(base_url),
        auth: Some(auth_definition),
        framing: Framing::sse(),
        headers: Some(Arc::new(|request: &LLMRequest| {
            request
                .provider_options
                .as_ref()
                .and_then(|options| options.get("serviceTier"))
                .and_then(Value::as_str)
                .map(|tier| {
                    [("x-vertex-ai-llm-shared-request-type".to_string(), tier.to_string())]
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default()
        })),
        transport,
        defaults: Default::default(),
    })
}

// ===========================================================================
// OpenAI-compatibility deployments (AI-V1: port of google-vertex-chat.ts +
// google-vertex-responses.ts — the aiplatform `endpoints/openapi` root)
// ===========================================================================

/// `GoogleVertexShared.requireProject`: without a baseURL there is no URL
/// to compose — the project becomes a configuration requirement (shared
/// by the Messages and `OpenAPI` lanes).
pub fn require_project_or_fail(project: Option<String>) -> Result<String, AiError> {
    project.ok_or_else(|| {
        AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new(PROVIDER),
            message: "Google Vertex requires a project when baseURL is not configured".into(),
        })
    })
}

/// The endpoint host a catalog baseURL template's
/// `${GOOGLE_VERTEX_ENDPOINT}` resolves to
/// (plugin/provider/google-vertex.ts `vertexEndpoint`): `global` is the
/// multi-region root, everything else `{location}-aiplatform`.
fn template_endpoint_host(location: &str) -> String {
    if location == "global" {
        "aiplatform.googleapis.com".to_string()
    } else {
        format!("{location}-aiplatform.googleapis.com")
    }
}

/// Expand the catalog's `${GOOGLE_VERTEX_*}` baseURL templates against the
/// resolved project/location (port of `replaceVertexVars`): location and
/// endpoint always resolve; an unresolved project keeps its placeholder.
#[must_use]
pub fn expand_vertex_vars(value: &str, project: Option<&str>, location: &str) -> String {
    value
        .replace("${GOOGLE_VERTEX_PROJECT}", project.unwrap_or("${GOOGLE_VERTEX_PROJECT}"))
        .replace("${GOOGLE_VERTEX_LOCATION}", location)
        .replace("${GOOGLE_VERTEX_ENDPOINT}", &template_endpoint_host(location))
}

/// The `OpenAPI` base URL for one binding (port of both packages'
/// `configuredRoute` + the plugin template expansion): an explicit baseURL
/// expands its `${GOOGLE_VERTEX_*}` placeholders (plugin fallback
/// `us-central1`); otherwise the package-composed
/// `https://aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/endpoints/openapi`
/// with the `global` fallback and the project requirement.
fn openapi_base_url(
    base_url: Option<String>,
    project: Option<&str>,
    location: Option<&str>,
) -> Result<String, AiError> {
    if let Some(base_url) = base_url {
        // Template expansion runs after config/env resolution, exactly like
        // the provider plugin (its location fallback is us-central1).
        let location = resolve_location(location, "us-central1");
        let project = resolve_project(project);
        return Ok(expand_vertex_vars(&base_url, project.as_deref(), &location));
    }
    let location = resolve_location(location, "global");
    let project = require_project_or_fail(resolve_project(project))?;
    Ok(format!(
        "https://aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/endpoints/openapi"
    ))
}

/// Vertex OpenAPI-lane configuration (port of both packages' `Config`).
#[derive(Default)]
pub struct VertexEndpointConfig {
    pub base_url: Option<String>,
    pub location: Option<String>,
    pub project: Option<String>,
    /// The OAuth access token (the ADC credential binding's resolved
    /// token or an explicit config token).
    pub access_token: Option<Secret>,
    /// Rejected: the `OpenAPI` lanes authenticate through OAuth/ADC only.
    pub api_key: Option<Secret>,
}

/// The ADC bearer chain shared by the `OpenAPI` lanes (port of
/// `GoogleVertexShared.oauth`): explicit token, else the env stand-in.
fn openapi_auth(config: &VertexEndpointConfig) -> Definition {
    adc_access_token(config.access_token.clone())
}

/// Shared construction for the two `OpenAPI` lanes: reject API keys, compose
/// the base URL, then hand the route identity/protocol to the caller.
fn openapi_route(
    config: &VertexEndpointConfig,
    lane: &str,
) -> Result<(String, Definition), AiError> {
    if config.api_key.is_some() {
        return Err(AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new(PROVIDER),
            message: format!("Google Vertex {lane} does not support API keys"),
        }));
    }
    let base_url = openapi_base_url(
        config.base_url.clone(),
        config.project.as_deref(),
        config.location.as_deref(),
    )?;
    Ok((base_url, openapi_auth(config)))
}

/// The configured Vertex `OpenAI` Chat route (port of
/// `google-vertex-chat.ts`): the `openai-chat` protocol on the
/// `endpoints/openapi` root at `/chat/completions`, ADC bearer, API keys
/// rejected.
pub fn vertex_chat_route(
    transport: SharedTransport,
    config: VertexEndpointConfig,
) -> Result<Route, AiError> {
    let (base_url, auth) = openapi_route(&config, "Chat")?;
    Ok(Route::new(RouteInput {
        id: "google-vertex-chat".to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some("vertex".to_string()),
        protocol: Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
        endpoint: Endpoint::path(CHAT_PATH).with_base_url(base_url),
        auth: Some(auth),
        framing: Framing::sse_with_done(),
        headers: None,
        transport,
        defaults: RouteDefaults::default(),
    }))
}

/// The configured Vertex Open Responses route (port of
/// `google-vertex-responses.ts`): the open-responses protocol on the
/// `endpoints/openapi` root at `/responses` with the `store: false` +
/// `include: ["reasoning.encrypted_content"]` defaults, ADC bearer, API
/// keys rejected.
pub fn vertex_responses_route(
    transport: SharedTransport,
    config: VertexEndpointConfig,
) -> Result<Route, AiError> {
    let (base_url, auth) = openapi_route(&config, "Responses")?;
    let mut provider_options = crate::schema::ProviderOptions::new();
    provider_options.insert("store".to_string(), serde_json::json!(false));
    provider_options
        .insert("include".to_string(), serde_json::json!(["reasoning.encrypted_content"]));
    Ok(Route::new(RouteInput {
        id: "google-vertex-responses".to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some("vertex".to_string()),
        protocol: Arc::new(OpenResponsesProtocol::new(Arc::new(BaseAdapter))),
        endpoint: Endpoint::path(RESPONSES_PATH).with_base_url(base_url),
        auth: Some(auth),
        framing: Framing::sse(),
        headers: None,
        transport,
        defaults: RouteDefaults { provider_options: Some(provider_options), ..Default::default() },
    }))
}

#[cfg(test)]
mod tests;
