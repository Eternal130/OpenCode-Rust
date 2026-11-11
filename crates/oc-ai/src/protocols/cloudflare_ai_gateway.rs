//! Cloudflare AI Gateway (port of `providers/cloudflare-ai-gateway.ts`).
//!
//! One provider, three routes — the REST-API chat route plus the
//! provider-native passthrough Messages/Responses routes on
//! `gateway.ai.cloudflare.com`. URL synthesis is account-id driven:
//! the REST root is
//! `https://api.cloudflare.com/client/v4/accounts/{accountId}/ai/v1`,
//! the passthrough roots are
//! `https://gateway.ai.cloudflare.com/v1/{accountId}/{gatewayId}/{anthropic/v1|openai}`.
//! A custom `baseURL` stands in for the REST API, so every model keeps
//! it with the full gateway model ids (cloudflare-ai-gateway.ts:108-121).
//!
//! Auth is the optional `gatewayApiKey ?? apiKey` →
//! `CLOUDFLARE_API_TOKEN` → `CF_AIG_TOKEN` chain. The REST route sends
//! it as a plain `Authorization: Bearer`; the passthrough routes send
//! the upstream provider credential through the gateway as
//! `cf-aig-authorization: Bearer …` (`Auth.bearerHeader`). The gateway
//! control headers (`cf-aig-*`) ride every request: `cf-aig-gateway-id`
//! on the REST route and the cache/log knobs everywhere
//! (cloudflare-ai-gateway.ts:88-104).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::protocols::anthropic_messages::{self, AnthropicMessages};
use crate::protocols::open_responses::{BaseAdapter, OpenResponsesProtocol};
use crate::protocols::openai_chat::{self, OpenAiChat, OpenAiChatExt};
use crate::route::auth::{self, Credential, Definition as AuthDefinition, Secret};
use crate::route::endpoint::Endpoint;
use crate::route::framing::Framing;
use crate::route::transport::SharedTransport;
use crate::route::{merge_route_defaults, Route, RouteDefaults, RouteInput};

/// Stable provider id (TS `id = ProviderID.make("cloudflare-ai-gateway")`).
pub const PROVIDER: &str = "cloudflare-ai-gateway";
/// The chat (REST API) route id (`route`).
pub const CHAT_ID: &str = "cloudflare-ai-gateway-chat";
/// The messages passthrough route id (`messagesRoute`).
pub const MESSAGES_ID: &str = "cloudflare-ai-gateway-messages";
/// The responses passthrough route id (`responsesRoute`).
pub const RESPONSES_ID: &str = "cloudflare-ai-gateway-responses";
/// The auth env chain (`authEnvVars`).
pub const AUTH_ENV: [&str; 2] = ["CLOUDFLARE_API_TOKEN", "CF_AIG_TOKEN"];

/// The REST API root (`baseURL`, cloudflare-ai-gateway.ts:43-46).
pub const REST_ROOT: &str = "https://api.cloudflare.com/client/v4/accounts";
/// The provider-native gateway root (`passthroughURL`, :50-51).
pub const GATEWAY_ROOT: &str = "https://gateway.ai.cloudflare.com/v1";

/// Percent-encode a URL path segment the way the ref
/// `encodeURIComponent` does (RFC 3986 unreserved set kept).
fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The REST API base URL
/// (`https://api.cloudflare.com/client/v4/accounts/{accountId}/ai/v1`).
#[must_use]
pub fn rest_base_url(account_id: &str) -> String {
    format!("{REST_ROOT}/{}/ai/v1", encode_segment(account_id))
}

/// The provider-native passthrough base URL
/// (`https://gateway.ai.cloudflare.com/v1/{accountId}/{gatewayId}/{provider}`).
#[must_use]
pub fn passthrough_base_url(account_id: &str, gateway_id: &str, provider: &str) -> String {
    format!(
        "{GATEWAY_ROOT}/{}/{}/{}",
        encode_segment(account_id),
        encode_segment(gateway_id),
        provider
    )
}

/// `gatewayId` — the trimmed setting or `"default"`
/// (cloudflare-ai-gateway.ts:61).
#[must_use]
pub fn gateway_id(configured: Option<&str>) -> String {
    configured.map(str::trim).filter(|id| !id.is_empty()).unwrap_or("default").to_string()
}

/// The wire model id for one gateway model (`configure().model()`'s
/// id rewrite, cloudflare-ai-gateway.ts:124-135): `openai/` and
/// `anthropic/` strip their prefix on the native endpoints (anthropic
/// also dashes its dots), `workers-ai/` always strips, and a custom
/// baseURL keeps the full id.
#[must_use]
pub fn wire_model_id(model_id: &str, native: bool) -> String {
    if let Some(id) = model_id.strip_prefix("openai/") {
        return if native { id.to_string() } else { model_id.to_string() };
    }
    if let Some(id) = model_id.strip_prefix("anthropic/") {
        return if native { id.replace('.', "-") } else { model_id.to_string() };
    }
    if let Some(id) = model_id.strip_prefix("workers-ai/") {
        return id.to_string();
    }
    model_id.to_string()
}

/// The optional gateway cache/log knobs (`metadata`, `cacheTtl`,
/// `cacheKey`, `skipCache`, `collectLog`).
#[derive(Debug, Clone, Default)]
pub struct GatewayOptions {
    pub metadata: Option<Value>,
    pub cache_ttl: Option<i64>,
    pub cache_key: Option<String>,
    pub skip_cache: Option<bool>,
    pub collect_log: Option<bool>,
}

/// The `cf-aig-*` control headers (`headers()`,
/// cloudflare-ai-gateway.ts:88-97): JSON-encoded metadata plus the
/// cache/log knobs, each only when configured.
#[must_use]
pub fn gateway_headers(options: &GatewayOptions) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    if let Some(metadata) = &options.metadata {
        // `Schema.encodeSync(Schema.fromJsonString(Schema.Unknown))` —
        // a JSON string re-encoded to its canonical compact form.
        let encoded = match metadata {
            Value::String(text) => serde_json::from_str::<Value>(text)
                .map(|value| value.to_string())
                .unwrap_or_else(|_| metadata.to_string()),
            other => other.to_string(),
        };
        headers.insert("cf-aig-metadata".to_string(), encoded);
    }
    if let Some(ttl) = options.cache_ttl {
        headers.insert("cf-aig-cache-ttl".to_string(), ttl.to_string());
    }
    if let Some(key) = &options.cache_key {
        headers.insert("cf-aig-cache-key".to_string(), key.clone());
    }
    if let Some(skip) = options.skip_cache {
        headers.insert("cf-aig-skip-cache".to_string(), skip.to_string());
    }
    if let Some(collect) = options.collect_log {
        headers.insert("cf-aig-collect-log".to_string(), collect.to_string());
    }
    headers
}

/// The credential chain (`credential()`, :83-86):
/// `gatewayApiKey ?? apiKey` → `CLOUDFLARE_API_TOKEN` → `CF_AIG_TOKEN`.
#[must_use]
pub fn credential(api_key: Option<Secret>, env: &[String]) -> Credential {
    let mut chain = auth::optional(api_key);
    for name in AUTH_ENV {
        if !env.iter().any(|existing| existing.as_str() == name) {
            chain = auth::or_else(&chain, &auth::config(name.to_string()));
        }
    }
    chain
}

/// Inputs for the route factories.
pub struct CloudflareGatewaySpec {
    /// `accountId` — required unless `base_url` is supplied.
    pub account_id: Option<String>,
    /// A custom baseURL standing in for the REST API (every model then
    /// keeps it with the full gateway model ids).
    pub base_url: Option<String>,
    pub gateway_id: Option<String>,
    /// `gatewayApiKey ?? apiKey` (both fold into one chain).
    pub api_key: Option<Secret>,
    /// Extra catalog env names ahead of the provider's own chain.
    pub env: Vec<String>,
    pub options: GatewayOptions,
    pub transport: SharedTransport,
}

impl CloudflareGatewaySpec {
    /// The REST base: the custom baseURL or the account-synthesized
    /// root (`baseURL()`; `requireAccountId` throws without either,
    /// cloudflare-ai-gateway.ts:53-59).
    fn rest_base(&self) -> Result<String, String> {
        match (&self.base_url, &self.account_id) {
            (Some(url), _) => Ok(url.clone()),
            (None, Some(account)) => Ok(rest_base_url(account)),
            (None, None) => Err(
                "CloudflareAIGateway.configure requires accountId unless baseURL is supplied"
                    .to_string(),
            ),
        }
    }

    /// `native` — no custom baseURL, so the provider-native gateway
    /// endpoints apply (cloudflare-ai-gateway.ts:109).
    fn native(&self) -> bool {
        self.base_url.is_none()
    }
}

/// The REST chat route defaults: the `cf-aig-gateway-id` header plus
/// the shared `cf-aig-*` knobs (`configure`'s `rest.headers`, :104).
fn rest_headers(spec: &CloudflareGatewaySpec) -> BTreeMap<String, String> {
    let mut headers = gateway_headers(&spec.options);
    headers.insert("cf-aig-gateway-id".to_string(), gateway_id(spec.gateway_id.as_deref()));
    headers
}

/// The REST chat route — the OpenAI chat protocol at the REST base
/// with the plain bearer credential (`route.with(rest)`, :121).
fn chat_route(spec: &CloudflareGatewaySpec, base: &str, id: &str) -> Route {
    Route::new(RouteInput {
        id: id.to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some(PROVIDER.to_string()),
        protocol: Arc::new(OpenAiChat::new(OpenAiChatExt::base())),
        endpoint: Endpoint::path(openai_chat::PATH).with_base_url(base),
        auth: Some(credential(spec.api_key.clone(), &spec.env).bearer()),
        framing: Framing::sse_with_done(),
        headers: None,
        transport: Arc::clone(&spec.transport),
        defaults: RouteDefaults { headers: Some(rest_headers(spec)), ..Default::default() },
    })
}

/// Passthrough auth (`Auth.bearerHeader("cf-aig-authorization",
/// credential(input))`, :115): the upstream provider credential rides
/// a gateway header, never the gateway's own `Authorization`.
fn passthrough_auth(spec: &CloudflareGatewaySpec) -> AuthDefinition {
    credential(spec.api_key.clone(), &spec.env).bearer_header("cf-aig-authorization")
}

/// The messages passthrough route (NATIVE only): the Anthropic
/// Messages protocol at the `{account}/{gateway}/anthropic/v1` root
/// with the upstream credential forwarded as `cf-aig-authorization`
/// plus the `anthropic-version` header.
fn messages_route(spec: &CloudflareGatewaySpec) -> Route {
    let account = spec.account_id.as_deref().unwrap_or_default();
    let base =
        passthrough_base_url(account, &gateway_id(spec.gateway_id.as_deref()), "anthropic/v1");
    Route::new(RouteInput {
        id: MESSAGES_ID.to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some(PROVIDER.to_string()),
        protocol: Arc::new(AnthropicMessages),
        endpoint: Endpoint::path(anthropic_messages::PATH).with_base_url(base),
        auth: Some(passthrough_auth(spec)),
        framing: anthropic_messages::framing(),
        headers: None,
        transport: Arc::clone(&spec.transport),
        defaults: merge_route_defaults(
            Some(&RouteDefaults {
                headers: Some(BTreeMap::from([(
                    "anthropic-version".to_string(),
                    "2023-06-01".to_string(),
                )])),
                ..Default::default()
            }),
            &RouteDefaults { headers: Some(gateway_headers(&spec.options)), ..Default::default() },
        ),
    })
}

/// The responses passthrough route (NATIVE only): the Open Responses
/// protocol at the `{account}/{gateway}/openai` root with the same
/// passthrough auth.
fn responses_route(spec: &CloudflareGatewaySpec) -> Route {
    let account = spec.account_id.as_deref().unwrap_or_default();
    let base = passthrough_base_url(account, &gateway_id(spec.gateway_id.as_deref()), "openai");
    Route::new(RouteInput {
        id: RESPONSES_ID.to_string(),
        provider: Some(PROVIDER.to_string()),
        provider_metadata_key: Some(PROVIDER.to_string()),
        protocol: Arc::new(OpenResponsesProtocol::new(Arc::new(BaseAdapter))),
        endpoint: Endpoint::path(crate::protocols::open_responses::PATH).with_base_url(base),
        auth: Some(passthrough_auth(spec)),
        framing: Framing::sse(),
        headers: None,
        transport: Arc::clone(&spec.transport),
        defaults: RouteDefaults {
            headers: Some(gateway_headers(&spec.options)),
            ..Default::default()
        },
    })
}

/// Per-model route dispatch (`configure().model()`,
/// cloudflare-ai-gateway.ts:124-135): `openai/` rides the responses
/// translation, `anthropic/` the messages translation, `workers-ai/`
/// and unprefixed ids the REST chat route. Returns the route plus the
/// wire model id (prefix stripped — and dots dashed for anthropic —
/// only on the native endpoints; a custom baseURL keeps full ids,
/// riding the REST config with the route's own id).
pub fn route_for_model(
    spec: &CloudflareGatewaySpec,
    model_id: &str,
) -> Result<(Route, String), String> {
    let rest = spec.rest_base()?;
    let native = spec.native();
    if model_id.starts_with("openai/") {
        let route = if native {
            responses_route(spec)
        } else {
            // Non-native `passthrough()` returns `rest`, so the
            // responses route rides the REST config (:118-119).
            chat_route(spec, &rest, RESPONSES_ID)
        };
        return Ok((route, wire_model_id(model_id, native)));
    }
    if model_id.starts_with("anthropic/") {
        let route = if native {
            messages_route(spec)
        } else {
            chat_route(spec, &rest, MESSAGES_ID)
        };
        return Ok((route, wire_model_id(model_id, native)));
    }
    if let Some(id) = model_id.strip_prefix("workers-ai/") {
        // The workers-ai prefix strips unconditionally (:132-133).
        return Ok((chat_route(spec, &rest, CHAT_ID), id.to_string()));
    }
    Ok((chat_route(spec, &rest, CHAT_ID), model_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::transport::FailingTransport;
    use crate::schema::LLMRequest;

    fn spec(account: Option<&str>, base_url: Option<&str>) -> CloudflareGatewaySpec {
        CloudflareGatewaySpec {
            account_id: account.map(str::to_string),
            base_url: base_url.map(str::to_string),
            gateway_id: None,
            api_key: Some(Secret::Value("gw-key".into())),
            env: Vec::new(),
            options: GatewayOptions {
                cache_ttl: Some(300),
                skip_cache: Some(false),
                ..Default::default()
            },
            transport: Arc::new(FailingTransport { message: "unit" }),
        }
    }

    fn request() -> LLMRequest {
        crate::test_support::request_for_tests(vec![crate::schema::Message::user("hi")])
    }

    fn wire(route: &Route) -> crate::route::transport::TransportRequest {
        let prepared = route.prepare_request(&request());
        let (wire, _) = route
            .prepare_transport(&prepared, &serde_json::json!({}))
            .unwrap_or_else(|error| panic!("transport prep failed: {error}"));
        wire
    }

    #[test]
    fn url_synthesis_from_account_id() {
        assert_eq!(
            rest_base_url("acc 1"),
            "https://api.cloudflare.com/client/v4/accounts/acc%201/ai/v1"
        );
        assert_eq!(
            passthrough_base_url("acc 1", "team gw", "anthropic/v1"),
            "https://gateway.ai.cloudflare.com/v1/acc%201/team%20gw/anthropic/v1"
        );
        assert_eq!(gateway_id(Some("  ")), "default");
        assert_eq!(gateway_id(Some(" team ")), "team");
    }

    #[test]
    fn missing_account_without_base_url_is_a_configuration_error() {
        let error = route_for_model(&spec(None, None), "anthropic/claude-4").unwrap_err();
        assert!(error.contains("requires accountId unless baseURL is supplied"), "{error}");
    }

    #[test]
    fn prefix_dispatch_picks_the_native_routes() {
        let spec = spec(Some("acc"), None);
        let (responses, id) = route_for_model(&spec, "openai/gpt-5.2").unwrap();
        assert_eq!(responses.id, RESPONSES_ID);
        assert_eq!(id, "gpt-5.2");
        let (messages, id) = route_for_model(&spec, "anthropic/claude-4.5").unwrap();
        assert_eq!(messages.id, MESSAGES_ID);
        assert_eq!(id, "claude-4-5");
        let (chat, id) = route_for_model(&spec, "workers-ai/llama-3").unwrap();
        assert_eq!(chat.id, CHAT_ID);
        assert_eq!(id, "llama-3");
        let (chat, id) = route_for_model(&spec, "qwen-max").unwrap();
        assert_eq!(chat.id, CHAT_ID);
        assert_eq!(id, "qwen-max");
    }

    #[test]
    fn native_routes_hit_the_passthrough_urls_with_gateway_headers() {
        let spec = spec(Some("acc"), None);
        let (messages, _) = route_for_model(&spec, "anthropic/claude-4.5").unwrap();
        let wire_req = wire(&messages);
        assert_eq!(
            wire_req.url,
            "https://gateway.ai.cloudflare.com/v1/acc/default/anthropic/v1/messages"
        );
        assert_eq!(wire_req.header("anthropic-version"), Some("2023-06-01"));
        assert_eq!(wire_req.header("cf-aig-cache-ttl"), Some("300"));
        assert_eq!(wire_req.header("cf-aig-skip-cache"), Some("false"));
        // passthrough auth: the credential rides the gateway header
        assert_eq!(wire_req.header("cf-aig-authorization"), Some("Bearer gw-key"));

        let (responses, _) = route_for_model(&spec, "openai/gpt-5.2").unwrap();
        assert_eq!(
            wire(&responses).url,
            "https://gateway.ai.cloudflare.com/v1/acc/default/openai/responses"
        );

        let (chat, _) = route_for_model(&spec, "qwen-max").unwrap();
        let wire_req = wire(&chat);
        assert_eq!(
            wire_req.url,
            "https://api.cloudflare.com/client/v4/accounts/acc/ai/v1/chat/completions"
        );
        assert_eq!(wire_req.header("cf-aig-gateway-id"), Some("default"));
        assert_eq!(wire_req.header("authorization"), Some("Bearer gw-key"));
    }

    #[test]
    fn custom_base_url_keeps_rest_routes_and_full_model_ids() {
        let spec = spec(Some("ignored"), Some("https://gateway.test/v1"));
        let (messages, id) = route_for_model(&spec, "anthropic/claude-4.5").unwrap();
        assert_eq!(messages.id, MESSAGES_ID);
        assert_eq!(id, "anthropic/claude-4.5");
        let wire_req = wire(&messages);
        assert_eq!(wire_req.url, "https://gateway.test/v1/chat/completions");
        assert_eq!(wire_req.header("authorization"), Some("Bearer gw-key"));
        assert!(wire_req.header("cf-aig-authorization").is_none());
    }

    #[test]
    fn gateway_metadata_header_is_json_encoded() {
        let mut options = GatewayOptions::default();
        options.metadata = Some(serde_json::json!({ "team": "core" }));
        let headers = gateway_headers(&options);
        assert_eq!(headers.get("cf-aig-metadata").map(String::as_str), Some(r#"{"team":"core"}"#));
        assert!(gateway_headers(&GatewayOptions::default()).is_empty());
    }

    #[test]
    fn env_chain_appends_the_provider_auth_vars() {
        std::env::set_var("CF_AIG_TOKEN", "env-token");
        let spec = CloudflareGatewaySpec {
            api_key: None,
            env: vec!["DASHSCOPE_API_KEY".to_string()],
            ..spec(Some("acc"), None)
        };
        let (chat, _) = route_for_model(&spec, "qwen-max").unwrap();
        assert_eq!(wire(&chat).header("authorization"), Some("Bearer env-token"));
        std::env::remove_var("CF_AIG_TOKEN");
    }
}
