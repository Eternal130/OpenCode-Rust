//! Google Vertex Claude Messages (port of
//! `packages/ai/src/providers/google-vertex-messages.ts` + the shared
//! `google-vertex-shared.ts` helpers): the Anthropic Messages protocol
//! deployed on Vertex — project/location routing on the
//! `publishers/anthropic/models` root, the model riding the
//! `:streamRawPredict` path (never the body), the body
//! `anthropic_version: vertex-2023-10-16`, the `anthropic-version` header,
//! and OAuth accessToken/ADC auth. API keys are rejected on this lane
//! (`Google Vertex Messages does not support API keys`); the
//! `x-goog-api-key` Express header belongs to the Gemini arm
//! ([`super::vertex`]), never this one.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::anthropic_messages::{framing, required_beta_headers_of_body, AnthropicMessages};
use super::vertex::{adc_access_token, host, resolve_location, resolve_project};
use crate::route::auth::Secret;
use crate::route::client::{Route, RouteInput};
use crate::route::endpoint::Endpoint;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::{
    SharedTransport, Transport, TransportError, TransportRequest, TransportResponse,
};
use crate::route::RouteDefaults;
use crate::schema::options::SanitizerCompatibility;
use crate::schema::{AiError, AiErrorReason, LLMEvent, LLMRequest, ProviderId};

/// Route id (TS `Route.make({ id: "google-vertex-messages" })`).
pub const ADAPTER: &str = "google-vertex-messages";
/// Provider id (TS `ProviderID.make("google-vertex")`).
pub const PROVIDER: &str = "google-vertex";
/// The body `anthropic_version` Vertex requires (TS `VERSION`).
pub const BODY_ANTHROPIC_VERSION: &str = "vertex-2023-10-16";
/// The `anthropic-version` request header (TS `HEADER_VERSION`).
pub const HEADER_VERSION: &str = "2023-06-01";

/// `GoogleVertexShared.requireProject`: without a baseURL there is no URL
/// to compose — the project (option or the env aliases, already folded by
/// [`resolve_project`]) becomes a configuration requirement.
fn require_project(project: Option<String>) -> Result<String, AiError> {
    project.ok_or_else(|| {
        AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new(PROVIDER),
            message: "Google Vertex requires a project when baseURL is not configured".into(),
        })
    })
}

/// The Messages protocol with the Vertex body rewrite (TS
/// `Protocol.make({ id: AnthropicMessages.protocol.id, body: … })`): the
/// stock Messages body minus `model`, plus
/// `anthropic_version: vertex-2023-10-16` — the model rides the URL.
struct VertexMessagesProtocol {
    messages: AnthropicMessages,
}

impl Protocol for VertexMessagesProtocol {
    fn id(&self) -> &str {
        // TS keeps the underlying protocol id (`AnthropicMessages.protocol.id`).
        AnthropicMessages.id()
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        let mut body = self.messages.body_from(request)?;
        if let Value::Object(object) = &mut body {
            object.remove("model");
            object.insert("anthropic_version".into(), json!(BODY_ANTHROPIC_VERSION));
        }
        Ok(body)
    }

    fn initial(&self, request: &LLMRequest) -> ProtocolState {
        self.messages.initial(request)
    }

    fn step(
        &self,
        state: ProtocolState,
        frame: &str,
    ) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        self.messages.step(state, frame)
    }

    fn on_halt(&self, state: ProtocolState) -> Result<(ProtocolState, Vec<LLMEvent>), AiError> {
        self.messages.on_halt(state)
    }

    fn supports_effort_updates(&self, request: &LLMRequest) -> bool {
        self.messages.supports_effort_updates(request)
    }

    fn sanitizer(&self) -> Option<SanitizerCompatibility> {
        self.messages.sanitizer()
    }
}

/// The Messages beta-header transport wrapper over the shared transport
/// (TS `transport: AnthropicMessages.transport<…>()`): the required betas
/// of the built body union with any caller `anthropic-beta` header at
/// send time. Same behavior as the private wrapper in
/// `anthropic_messages.rs`, which stays encapsulated there.
struct VertexMessagesTransport {
    inner: SharedTransport,
}

#[async_trait::async_trait]
impl Transport for VertexMessagesTransport {
    async fn execute(
        &self,
        mut request: TransportRequest,
    ) -> Result<TransportResponse, TransportError> {
        let required = serde_json::from_slice::<Value>(&request.body).map_or_else(
            |_| vec!["interleaved-thinking-2025-05-14".to_string()],
            |body| required_beta_headers_of_body(&body),
        );
        let existing = request.header("anthropic-beta").unwrap_or_default();
        let mut betas: Vec<String> = existing
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        for beta in required {
            if !betas.contains(&beta) {
                betas.push(beta);
            }
        }
        request.headers.retain(|(name, _)| !name.eq_ignore_ascii_case("anthropic-beta"));
        request.headers.push(("anthropic-beta".to_string(), betas.join(",")));
        self.inner.execute(request).await
    }
}

/// Vertex Messages route configuration (port of the TS `Config`).
#[derive(Default)]
pub struct VertexMessagesConfig {
    pub base_url: Option<String>,
    pub location: Option<String>,
    pub project: Option<String>,
    /// The OAuth access token (the ADC credential binding's resolved
    /// token or an explicit config token).
    pub access_token: Option<Secret>,
    /// Rejected: this lane authenticates through OAuth/ADC only.
    pub api_key: Option<Secret>,
}

/// The configured Vertex Messages route (port of `configuredRoute`):
/// `baseURL ?? https://{host(location)}/v1/projects/{project}/locations/{location}/publishers/anthropic/models`
/// with the `global` location fallback, the model riding the
/// `/{model}:streamRawPredict` path, the ADC bearer chain, and the static
/// `anthropic-version` header. Fails typed when an API key is configured
/// or no baseURL/project resolves.
pub fn vertex_messages_route(
    transport: SharedTransport,
    config: VertexMessagesConfig,
) -> Result<Route, AiError> {
    if config.api_key.is_some() {
        return Err(AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new(PROVIDER),
            message: "Google Vertex Messages does not support API keys".into(),
        }));
    }
    let location = resolve_location(config.location.as_deref(), "global");
    let base_url = if let Some(base_url) = config.base_url {
        base_url
    } else {
        let project = require_project(resolve_project(config.project.as_deref()))?;
        format!(
            "https://{}/v1/projects/{project}/locations/{location}/publishers/anthropic/models",
            host(&location)
        )
    };
    Ok(Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some("vertex".to_string()),
        protocol: Arc::new(VertexMessagesProtocol { messages: AnthropicMessages }),
        endpoint: Endpoint::dynamic_path(|input: &crate::route::endpoint::EndpointInput<'_>| {
            format!("/{}:streamRawPredict", input.request.model.id.0)
        })
        .with_base_url(base_url),
        auth: Some(adc_access_token(config.access_token)),
        framing: framing(),
        headers: Some(Arc::new(|_request: &LLMRequest| {
            BTreeMap::from([("anthropic-version".to_string(), HEADER_VERSION.to_string())])
        })),
        transport: Arc::new(VertexMessagesTransport { inner: transport }),
        defaults: RouteDefaults::default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_keeps_the_messages_id_and_rewrites_the_body() {
        let protocol = VertexMessagesProtocol { messages: AnthropicMessages };
        assert_eq!(protocol.id(), "anthropic-messages");
    }
}
