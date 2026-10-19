#![allow(clippy::doc_markdown, reason = "Azure OpenAI identifiers in prose stay unquoted")]

//! Azure OpenAI provider wiring (port of `packages/ai/src/providers/azure.ts`,
//! ledger AI-AZURE).
//!
//! Azure needs the customer's resource URL: supply either `resourceName`
//! (helper builds `https://{name}.openai.azure.com/openai`) or `baseURL`
//! directly — neither is a configuration error. The endpoint is
//! `{base}/v1` with `api-version=v1` (queryParams merge over it), or
//! `{base}/deployments/{model}` when `useDeploymentBasedUrls` is set; a
//! non-Azure `baseURL` passes through with the caller's query only (no
//! `api-version`). Auth removes any gateway `authorization` header and
//! sends the key as `api-key:` (config `apiKey` → `AZURE_OPENAI_API_KEY`).
//!
//! The responses route offers the OpenResponses WebSocket channel only for
//! the Azure-native endpoint shape (`https`, `*.openai.azure.com`, path
//! ending `/openai/v1/responses`, `api-version=v1`) with `api-version`
//! stripped from the WS URL (v2 `responsesRoute` channel transport).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::protocols::open_responses_channel::ChannelOptions;
use crate::protocols::openai_chat::{chat_route, OpenAiChat, OpenAiChatExt};
use crate::protocols::openai_responses::{
    self, OpenAIResponsesRouteSpec, WEBSOCKET_ROTATE_AFTER_MS,
};
use crate::route::auth::{self, Secret};
use crate::route::endpoint::{trim_base_url, EndpointPatch};
use crate::route::framing::Framing;
use crate::route::transport::SharedTransport;
use crate::route::{AuthDefinition, Route, RouteDefaults, RoutePatch};
use crate::schema::{AiError, AiErrorReason, ProviderId};

/// v2 `id = "azure"`.
pub const ID: &str = "azure";

/// `https://{resourceName}.openai.azure.com/openai` (v2 `resourceBaseURL`).
#[must_use]
pub fn resource_base_url(resource_name: &str) -> String {
    format!("https://{}.openai.azure.com/openai", resource_name.trim())
}

/// Azure provider configuration (v2 `Settings`/`Config`).
#[derive(Clone, Default)]
pub struct AzureConfig {
    /// Customer resource name (helper builds the URL).
    pub resource_name: Option<String>,
    /// Direct base URL (wins over `resource_name` like v2 `input.baseURL ??`).
    pub base_url: Option<String>,
    /// Static API key; falls back to `AZURE_OPENAI_API_KEY`.
    pub api_key: Option<Secret>,
    /// `api-version` query parameter (default `v1`).
    pub api_version: Option<String>,
    /// Extra query parameters (merge over `api-version`).
    pub query_params: BTreeMap<String, String>,
    /// `{base}/deployments/{model}` endpoints.
    pub use_deployment_based_urls: bool,
    /// Explicit auth override (v2 `"auth" in input`).
    pub auth: Option<AuthDefinition>,
    /// Caller route defaults merged over the OpenAI defaults.
    pub defaults: RouteDefaults,
}

impl AzureConfig {
    #[must_use]
    pub fn with_resource_name(mut self, name: impl Into<String>) -> Self {
        self.resource_name = Some(name.into());
        self
    }

    #[must_use]
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    #[must_use]
    pub fn with_api_key(mut self, key: Secret) -> Self {
        self.api_key = Some(key);
        self
    }

    /// The resolved base URL: `baseURL` wins, else `resourceName` builds it,
    /// else configuration error (v2 `ProviderConfigurationError`).
    fn require_base_url(&self) -> Result<String, AiError> {
        if let Some(base_url) = self.base_url.as_deref() {
            return Ok(trim_base_url(base_url).to_string());
        }
        if let Some(resource_name) = self.resource_name.as_deref() {
            return Ok(resource_base_url(resource_name));
        }
        Err(AiError::new(AiErrorReason::ProviderConfiguration {
            provider: ProviderId::new(ID),
            message: "Azure requires resourceName or baseURL".to_string(),
        }))
    }
}

/// v2 `auth(input)`: explicit override, else remove `authorization` then
/// `api-key:` from optional config key → `AZURE_OPENAI_API_KEY`.
#[must_use]
pub fn azure_auth(config: &AzureConfig) -> AuthDefinition {
    if let Some(auth) = config.auth.clone() {
        return auth;
    }
    let mut credential = auth::optional(config.api_key.clone());
    credential = auth::or_else(&credential, &auth::config("AZURE_OPENAI_API_KEY"));
    auth::Definition::remove("authorization").and_then(credential.header("api-key"))
}

/// Whether a base URL host is an Azure OpenAI resource (v2 hostname check).
fn is_azure_host(base_url: &str) -> bool {
    let host = base_url.split_once("://").map_or(base_url, |(_, rest)| rest);
    let host = host.split('/').next().unwrap_or_default();
    let host = host.rsplit_once(':').map_or(host, |(host, _port)| host);
    host.ends_with(".openai.azure.com")
}

/// The endpoint parts for one model (v2 `endpoint(input, modelID)`): base +
/// query with `api-version` default, deployment URLs, and the non-Azure
/// passthrough. The route's protocol supplies the path.
pub fn endpoint_parts(
    config: &AzureConfig,
    model_id: &str,
) -> Result<(String, BTreeMap<String, String>), AiError> {
    let base = config.require_base_url()?;
    if config.use_deployment_based_urls {
        let mut query = BTreeMap::new();
        query.insert(
            "api-version".to_string(),
            config.api_version.clone().unwrap_or_else(|| "v1".to_string()),
        );
        query.extend(config.query_params.clone());
        let encoded_model = endpoint_model_segment(model_id);
        return Ok((format!("{base}/deployments/{encoded_model}"), query));
    }
    if config.base_url.is_some() && !is_azure_host(config.base_url.as_deref().unwrap_or_default()) {
        // custom gateway: passthrough base + caller query only (no api-version)
        return Ok((base, config.query_params.clone()));
    }
    let mut query = BTreeMap::new();
    query.insert(
        "api-version".to_string(),
        config.api_version.clone().unwrap_or_else(|| "v1".to_string()),
    );
    query.extend(config.query_params.clone());
    Ok((format!("{base}/v1"), query))
}

/// encodeURIComponent for model ids in deployment paths.
fn endpoint_model_segment(model_id: &str) -> String {
    let mut out = String::with_capacity(model_id.len());
    for byte in model_id.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The Azure responses channel gate (v2 `responsesRoute` transport
/// `enabled`): Azure-native endpoint only — https, `*.openai.azure.com`
/// host, path ending `/openai/v1/responses`, `api-version=v1`.
pub fn responses_channel_enabled(url: &str) -> bool {
    let Some(("https", rest)) = url.split_once("://") else {
        return false;
    };
    let (host, path_query) = rest.split_once('/').map_or((rest, ""), |(host, path)| (host, path));
    let host = host.rsplit_once(':').map_or(host, |(host, _port)| host);
    let (path, query) = path_query.split_once('?').unwrap_or((path_query, ""));
    if !host.ends_with(".openai.azure.com") {
        return false;
    }
    // JS `url.pathname` carries the leading slash the split removed
    if !format!("/{path}").ends_with("/openai/v1/responses") {
        return false;
    }
    query.split('&').any(|pair| pair == "api-version=v1")
}

/// Strip `api-version` from a URL's query (v2 `responsesRoute` transport
/// `url` map — the WS handshake takes it from the handshake headers, not
/// the query).
#[must_use]
pub fn responses_channel_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| !pair.is_empty() && !pair.starts_with("api-version="))
        .collect();
    if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    }
}

/// The Azure responses channel options (v2 `responsesRoute` transport):
/// rotate after 55 minutes, Azure-native gate, `api-version` stripped.
#[must_use]
pub fn responses_channel_options() -> ChannelOptions {
    ChannelOptions::new("azure-openai-responses", "Azure OpenAI Responses")
        .with_rotate_after_ms(WEBSOCKET_ROTATE_AFTER_MS)
        .with_enabled(Arc::new(|url: &str| crate::protocols::azure::responses_channel_enabled(url)))
        .with_url(Arc::new(|url: &str| crate::protocols::azure::responses_channel_url(url)))
}

/// Build the `azure-openai-responses` route (v2 `responsesRoute` +
/// `configuredRoute`): the OpenAI Responses protocol on the Azure endpoint
/// with `api-key` auth and the Azure WS channel gate.
pub fn azure_responses_route(
    config: &AzureConfig,
    transport: SharedTransport,
    model_id: &str,
) -> Result<Route, AiError> {
    let (base, query) = endpoint_parts(config, model_id)?;
    let route = self::openai_responses::openai_responses_route(OpenAIResponsesRouteSpec {
        base_url: Some(base),
        auth: Some(azure_auth(config)),
        transport,
        defaults: RouteDefaults::default(),
    });
    Ok(route
        .with(RoutePatch {
            id: Some("azure-openai-responses".to_string()),
            provider: Some(ID.to_string()),
            provider_metadata_key: Some("azure".to_string()),
            endpoint: Some(EndpointPatch { base_url: None, path: None, query: Some(query) }),
            ..Default::default()
        })
        .with(RoutePatch { defaults: config.defaults.clone(), ..Default::default() }))
}

/// Build the `azure-openai-chat` route (v2 `chatRoute` + `configuredRoute`):
/// the OpenAI Chat protocol on the Azure endpoint with `api-key` auth.
pub fn azure_chat_route(
    config: &AzureConfig,
    transport: SharedTransport,
    model_id: &str,
) -> Result<Route, AiError> {
    let (base, query) = endpoint_parts(config, model_id)?;
    Ok(chat_route(
        "azure-openai-chat",
        Some(ID),
        Some(ID),
        Some(&base),
        Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
        Framing::sse_with_done(),
        Some(azure_auth(config)),
        transport,
    )
    .with(RoutePatch {
        endpoint: Some(EndpointPatch { base_url: None, path: None, query: Some(query) }),
        ..Default::default()
    })
    .with(RoutePatch { defaults: config.defaults.clone(), ..Default::default() }))
}

/// v2 `Settings → Config` (`config(...)`): split the provider-package
/// settings into the route config; missing `resourceName`/`baseURL` fails
/// at endpoint build time with the typed error.
#[must_use]
pub fn azure_config(
    resource_name: Option<String>,
    base_url: Option<String>,
    api_key: Option<Secret>,
) -> AzureConfig {
    AzureConfig { resource_name, base_url, api_key, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_name_builds_the_azure_openai_base() {
        assert_eq!(
            resource_base_url(" my-resource "),
            "https://my-resource.openai.azure.com/openai"
        );
    }

    #[test]
    fn endpoint_shapes_match_v2() {
        let config = azure_config(Some("res".into()), None, None);
        let (base, query) = endpoint_parts(&config, "gpt-4.1").unwrap();
        assert_eq!(base, "https://res.openai.azure.com/openai/v1");
        assert_eq!(query.get("api-version").map(String::as_str), Some("v1"));

        let deployments = AzureConfig {
            use_deployment_based_urls: true,
            ..azure_config(Some("res".into()), None, None)
        };
        let (base, query) = endpoint_parts(&deployments, "gpt-4.1").unwrap();
        assert_eq!(base, "https://res.openai.azure.com/openai/deployments/gpt-4.1");
        assert_eq!(query.get("api-version").map(String::as_str), Some("v1"));

        // custom gateway: caller query only, no api-version
        let gateway = AzureConfig {
            query_params: BTreeMap::from([("x".to_string(), "1".to_string())]),
            ..azure_config(None, Some("https://proxy.internal/api/".into()), None)
        };
        let (base, query) = endpoint_parts(&gateway, "gpt-4.1").unwrap();
        assert_eq!(base, "https://proxy.internal/api");
        assert_eq!(query.get("api-version"), None);
        assert_eq!(query.get("x").map(String::as_str), Some("1"));

        // neither resourceName nor baseURL
        endpoint_parts(&AzureConfig::default(), "gpt-4.1").unwrap_err();
    }

    #[test]
    fn query_params_merge_over_api_version() {
        let config = AzureConfig {
            api_version: Some("preview".into()),
            query_params: BTreeMap::from([("api-version".to_string(), "v2".to_string())]),
            ..azure_config(Some("res".into()), None, None)
        };
        let (_base, query) = endpoint_parts(&config, "m").unwrap();
        assert_eq!(query.get("api-version").map(String::as_str), Some("v2"));
    }

    #[test]
    fn channel_gate_accepts_only_the_azure_native_shape() {
        let base = "https://res.openai.azure.com";
        assert!(responses_channel_enabled(&format!("{base}/openai/v1/responses?api-version=v1")));
        assert!(!responses_channel_enabled(&format!(
            "{base}/openai/v1/responses?api-version=preview"
        )));
        assert!(!responses_channel_enabled(&format!(
            "{base}/openai/deployments/m/responses?api-version=v1"
        )));
        assert!(!responses_channel_enabled(
            "https://proxy.internal/openai/v1/responses?api-version=v1"
        ));
        assert!(!responses_channel_enabled(
            "http://res.openai.azure.com/openai/v1/responses?api-version=v1"
        ));
    }

    #[test]
    fn channel_url_strips_only_api_version() {
        assert_eq!(
            responses_channel_url(
                "https://res.openai.azure.com/openai/v1/responses?api-version=v1&x=1"
            ),
            "https://res.openai.azure.com/openai/v1/responses?x=1"
        );
        assert_eq!(
            responses_channel_url(
                "https://res.openai.azure.com/openai/v1/responses?api-version=v1"
            ),
            "https://res.openai.azure.com/openai/v1/responses"
        );
    }

    #[test]
    fn auth_removes_authorization_and_sends_api_key_header() {
        use crate::route::auth::AuthInput;
        let config = azure_config(Some("res".into()), None, Some(Secret::Value("k-1".into())));
        let auth = azure_auth(&config);
        let headers = crate::route::auth::Headers::from_pairs([(
            "authorization".to_string(),
            "Bearer gateway".to_string(),
        )]);
        let input = AuthInput {
            request_http: None,
            method: "POST",
            url: "https://res.openai.azure.com/openai/v1/responses",
            body: "{}",
            headers,
        };
        let applied = auth.apply(&input).unwrap();
        assert_eq!(applied.get("api-key"), Some("k-1"));
        assert_eq!(applied.get("authorization"), None);
    }
}
