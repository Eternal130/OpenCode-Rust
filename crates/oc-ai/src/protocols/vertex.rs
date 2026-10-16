//! Google Vertex AI native (port of `providers/google-vertex.ts` +
//! `google-vertex-shared.ts`): the Gemini generateContent protocol deployed
//! on Vertex — project/region routing, tuned `endpoints/` models, Express
//! Mode API keys, ADC bearer auth, the `labels` provider option, and the
//! `serviceTier` request-type header (stripped from the body like v2's
//! `fromRequest`).

use std::sync::Arc;

use serde_json::Value;

use crate::protocols::gemini::GeminiProtocol;
use crate::route::auth::{self, Definition, Secret};
use crate::route::client::{Route, RouteInput};
use crate::route::endpoint::Endpoint;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::{Framing, SharedTransport};
use crate::schema::options::SanitizerCompatibility;
use crate::schema::{AiError, LLMEvent, LLMRequest};

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

#[cfg(test)]
mod tests;
