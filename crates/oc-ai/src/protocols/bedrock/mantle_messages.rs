//! Bedrock Mantle Messages (port of the `messagesRoute` slice of
//! `packages/ai/src/providers/amazon-bedrock-mantle.ts`): the Anthropic
//! Messages protocol deployed on the Mantle host —
//! `https://bedrock-mantle.{region}.api.aws/anthropic/v1/messages`, the
//! `anthropic-version` header, bearer/`SigV4` auth for the
//! `bedrock-mantle` service, and one protocol override: Mantle rejects
//! mid-conversation `output_config` on Opus 5.0, so effort-update support
//! starts at Claude 5.1 (the base protocol enables it for every Opus 5).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use super::sigv4::{resolve_auth, resolve_region, BedrockAuthOptions};
use crate::protocols::anthropic_messages::{
    claude_version, compatibility_of, framing, required_beta_headers_of_body, AnthropicMessages,
};
use crate::route::client::{Route, RouteInput};
use crate::route::endpoint::Endpoint;
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::{
    SharedTransport, Transport, TransportError, TransportRequest, TransportResponse,
};
use crate::route::RouteDefaults;
use crate::schema::options::SanitizerCompatibility;
use crate::schema::{AiError, AiErrorReason, LLMEvent, LLMRequest, ProviderId};

/// Route id (TS `Route.make({ id: "bedrock-mantle-messages" })`).
pub const ADAPTER: &str = "bedrock-mantle-messages";
/// Provider id (TS `ProviderID.make("amazon-bedrock")`).
pub const PROVIDER: &str = "amazon-bedrock";
/// The messages route base template suffix (TS `configuredRoute` messages
/// default `(region) => https://bedrock-mantle.${region}.api.aws/anthropic/v1`).
const ANTHROPIC_ROOT: &str = "anthropic/v1";
/// The `anthropic-version` request header (TS `headers`).
pub const HEADER_VERSION: &str = "2023-06-01";

/// Effort-update support on the Mantle Messages lane: Claude 5.1+ only —
/// Mantle rejects mid-conversation `output_config` on Opus 5.0 (port of the
/// protocol's overridden `supportsEffortUpdates`; compatibility overrides
/// still win).
pub fn supports_effort_updates(request: &LLMRequest) -> bool {
    if let Some(value) = compatibility_of(&request.model)
        .and_then(|compatibility| compatibility.supports_effort_updates)
    {
        return value;
    }
    let Some(version) = claude_version(&request.model.id.0) else { return false };
    version.major > 5 || (version.major == 5 && version.minor >= 1)
}

/// The Messages protocol with the Mantle effort-update gate (TS
/// `protocol: { ...AnthropicMessages.protocol, supportsEffortUpdates: … }`).
struct MantleMessagesProtocol {
    messages: AnthropicMessages,
}

impl Protocol for MantleMessagesProtocol {
    fn id(&self) -> &str {
        // TS keeps the underlying protocol id (`AnthropicMessages.protocol.id`).
        AnthropicMessages.id()
    }

    fn body_from(&self, request: &LLMRequest) -> Result<Value, AiError> {
        self.messages.body_from(request)
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
        supports_effort_updates(request)
    }

    fn sanitizer(&self) -> Option<SanitizerCompatibility> {
        self.messages.sanitizer()
    }
}

/// The Messages beta-header transport wrapper over the shared transport
/// (TS `transport: AnthropicMessages.transport<…>()`): the required betas
/// of the built body union with any caller `anthropic-beta` header at send
/// time. Same behavior as the wrappers in `anthropic_messages.rs` /
/// `vertex_messages.rs`, which stay encapsulated there.
struct MantleMessagesTransport {
    inner: SharedTransport,
}

#[async_trait::async_trait]
impl Transport for MantleMessagesTransport {
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

/// Mantle Messages route configuration (the messages-route slice of the TS
/// `Config`).
#[derive(Default)]
pub struct MantleMessagesConfig {
    /// Bedrock API key (`AWS_BEARER_TOKEN_BEDROCK` fallback); bearer wins
    /// over `SigV4`.
    pub api_key: Option<String>,
    /// Restrict to one auth mode.
    pub mode: Option<super::sigv4::BedrockAuthMode>,
    pub base_url: Option<String>,
    pub region: Option<String>,
    /// Static `SigV4` credentials; the default chain resolves per request.
    pub credentials: Option<super::sigv4::Credentials>,
}

impl MantleMessagesConfig {
    fn auth_options(&self) -> BedrockAuthOptions {
        BedrockAuthOptions {
            api_key: self.api_key.clone(),
            mode: self.mode,
            region: self.region.clone(),
            credentials: self.credentials.clone(),
            profile: None,
        }
    }
}

/// The configured Mantle Messages route (port of `messagesRoute` +
/// `configuredRoute`): `baseURL ??
/// https://bedrock-mantle.{region}.api.aws/anthropic/v1` with the
/// `/messages` path, bearer/`SigV4` auth for the `bedrock-mantle`
/// service, and the static `anthropic-version` header.
#[must_use]
pub fn bedrock_mantle_messages_route(
    transport: SharedTransport,
    config: MantleMessagesConfig,
) -> Route {
    let region = resolve_region(
        config
            .region
            .as_deref()
            .or_else(|| config.credentials.as_ref().map(|credentials| credentials.region.as_str())),
        config.credentials.as_ref(),
    );
    let base_url = config
        .base_url
        .clone()
        .unwrap_or_else(|| format!("https://bedrock-mantle.{region}.api.aws/{ANTHROPIC_ROOT}"));
    Route::new(RouteInput {
        id: ADAPTER.to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some("mantle".to_string()),
        protocol: Arc::new(MantleMessagesProtocol { messages: AnthropicMessages }),
        endpoint: Endpoint::path(crate::protocols::anthropic_messages::PATH)
            .with_base_url(base_url),
        auth: Some(resolve_auth(&config.auth_options(), "bedrock-mantle", "Bedrock Mantle")),
        framing: framing(),
        headers: Some(Arc::new(|_request: &LLMRequest| {
            BTreeMap::from([("anthropic-version".to_string(), HEADER_VERSION.to_string())])
        })),
        transport: Arc::new(MantleMessagesTransport { inner: transport }),
        defaults: RouteDefaults::default(),
    })
}

/// Provider-configuration validation for the mantle package (TS
/// `configure` guards; shared by all mantle routes, kept here for the
/// package seam).
pub fn validate_config(
    api_key: Option<&str>,
    mode: Option<super::sigv4::BedrockAuthMode>,
) -> Result<(), AiError> {
    if matches!(mode, Some(super::sigv4::BedrockAuthMode::Bearer))
        && api_key.is_none_or(str::is_empty)
        && std::env::var("AWS_BEARER_TOKEN_BEDROCK").unwrap_or_default().is_empty()
    {
        return Err(AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new(PROVIDER),
            message: "Amazon Bedrock Mantle bearer auth requires apiKey".into(),
        }));
    }
    if matches!(mode, Some(super::sigv4::BedrockAuthMode::SigV4))
        && api_key.is_some_and(|k| !k.is_empty())
    {
        return Err(AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new(PROVIDER),
            message: "Amazon Bedrock Mantle SigV4 auth does not accept apiKey".into(),
        }));
    }
    Ok(())
}
