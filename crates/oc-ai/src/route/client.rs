//! Route composition and the request pipeline (port of `src/route/client.ts`,
//! minus websocket/media/compaction-endpoint lanes).
//!
//! A [`Route`] composes the four orthogonal pieces of a deployment —
//! [`Protocol`] (what API), [`Endpoint`] (where), [`Auth`](auth) (how
//! authenticated), [`Framing`](framing) (how framed) — plus the
//! process-agnostic [`Transport`](transport) seam. `stream()` runs the full
//! pipeline: defaults merge → pre-body transforms → body build → transport →
//! frame → protocol state machine → [`LLMEvent`] stream with terminal-event
//! enforcement.

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt as _;

use crate::cache_policy::apply_cache_policy;
use crate::effort_updates::apply_effort_updates;
use crate::route::auth::{self, Definition as AuthDefinition, Headers};
use crate::route::endpoint::{self, Endpoint, EndpointInput, EndpointPatch};
use crate::route::executor::{http_failure, provider_message, HttpFailureInput, MAX_BODY_CHARS};
use crate::route::framing::{frame_stream, Framing};
use crate::route::protocol::{Protocol, ProtocolState};
use crate::route::transport::{
    read_body_limited, SharedTransport, TransportError, TransportRequest, TransportResponse,
};
use crate::schema::{
    AiError, AiErrorReason, GenerationOptions, HttpContext, HttpOptions, LLMEvent, LLMRequest,
    LLMRequestPatch, LLMResponse, LanguageModel, ProviderId, ReasonFields, ResponseState,
};
use crate::tool_history::normalize_tool_history;

/// Route-level request defaults (port of `RouteDefaults`).
#[derive(Clone, Debug, Default)]
pub struct RouteDefaults {
    pub headers: Option<BTreeMap<String, String>>,
    pub generation: Option<GenerationOptions>,
    pub provider_options: Option<crate::schema::ProviderOptions>,
    pub http: Option<HttpOptions>,
}

/// Inputs building a [`Route`] (port of `Route.make`).
pub struct RouteInput {
    pub id: String,
    pub provider: Option<String>,
    pub provider_metadata_key: Option<String>,
    pub protocol: Arc<dyn Protocol>,
    pub endpoint: Endpoint,
    pub auth: Option<AuthDefinition>,
    pub framing: Framing,
    pub headers: Option<Arc<dyn Fn(&LLMRequest) -> BTreeMap<String, String> + Send + Sync>>,
    pub transport: SharedTransport,
    pub defaults: RouteDefaults,
}

/// Patch accepted by [`Route::with`]; `None` fields keep the current value.
#[derive(Default)]
pub struct RoutePatch {
    pub id: Option<String>,
    pub provider: Option<String>,
    pub provider_metadata_key: Option<String>,
    pub auth: Option<AuthDefinition>,
    pub transport: Option<SharedTransport>,
    pub endpoint: Option<EndpointPatch>,
    /// Defaults patch: merges into the existing defaults (headers merge per
    /// key, generation/providerOptions/http deep-merge).
    pub defaults: RouteDefaults,
}

/// One runnable deployment of a protocol (port of `Route`; body/prepared
/// generics erased as in TS `AnyRoute`).
#[derive(Clone)]
pub struct Route {
    pub id: String,
    pub provider: Option<ProviderId>,
    pub provider_metadata_key: Option<String>,
    pub protocol: Arc<dyn Protocol>,
    pub endpoint: Endpoint,
    pub auth: AuthDefinition,
    pub framing: Framing,
    pub headers: Option<Arc<dyn Fn(&LLMRequest) -> BTreeMap<String, String> + Send + Sync>>,
    pub transport: SharedTransport,
    pub defaults: RouteDefaults,
}

impl std::fmt::Debug for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Route")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("protocol", &self.protocol.id())
            .finish_non_exhaustive()
    }
}

impl Route {
    pub fn new(input: RouteInput) -> Self {
        Self {
            id: input.id,
            provider: input.provider.map(ProviderId::new),
            provider_metadata_key: input.provider_metadata_key,
            protocol: input.protocol,
            endpoint: input.endpoint,
            auth: input.auth.unwrap_or_else(auth::Definition::none),
            framing: input.framing,
            headers: input.headers,
            transport: input.transport,
            defaults: merge_route_defaults(None, &input.defaults),
        }
    }

    /// Immutable `.with(patch)` (port of `Route.with`): endpoint merges
    /// (base/path replace, query merges), defaults merge, auth/transport
    /// replace, `provider_metadata_key` follows a changed provider unless
    /// explicitly set.
    pub fn with(&self, patch: RoutePatch) -> Self {
        let provider = patch.provider.map(ProviderId::new);
        Self {
            id: patch.id.unwrap_or_else(|| self.id.clone()),
            provider_metadata_key: patch.provider_metadata_key.or_else(|| {
                match (&provider, &self.provider) {
                    (Some(new), Some(current)) if new != current => Some(new.0.clone()),
                    (Some(new), None) => Some(new.0.clone()),
                    _ => self.provider_metadata_key.clone(),
                }
            }),
            provider: provider.or_else(|| self.provider.clone()),
            auth: patch.auth.unwrap_or_else(|| self.auth.clone()),
            endpoint: patch.endpoint.map_or_else(
                || self.endpoint.clone(),
                |endpoint| Endpoint::merge(&self.endpoint, endpoint),
            ),
            transport: patch.transport.unwrap_or_else(|| Arc::clone(&self.transport)),
            defaults: merge_route_defaults(Some(&self.defaults), &patch.defaults),
            protocol: Arc::clone(&self.protocol),
            framing: self.framing.clone(),
            headers: self.headers.clone(),
        }
    }

    /// Build a [`LanguageModel`] on this route (port of `route.model`):
    /// requires a provider and an endpoint `baseURL` (checked synchronously,
    /// as `ProviderConfigurationError` in TS).
    pub fn model(
        &self,
        id: impl Into<String>,
        provider: Option<String>,
    ) -> Result<LanguageModel, AiError> {
        let provider =
            provider.map(ProviderId::new).or_else(|| self.provider.clone()).ok_or_else(|| {
                AiError::new(AiErrorReason::ProviderConfiguration {
                    provider: ProviderId::new("unknown"),
                    message: format!("Route.model({}) requires a provider", self.id),
                })
            })?;
        if self.endpoint.base_url.is_none() {
            return Err(AiError::new(AiErrorReason::ProviderConfiguration {
                provider,
                message: format!(
                    "Route.model({}) requires an endpoint baseURL — configure it on the route first",
                    self.id
                ),
            }));
        }
        Ok(LanguageModel::new(id, provider.0, Arc::new(self.clone())))
    }

    /// Resolve request options: normalize tool history and merge defaults
    /// route ⊕ model ⊕ request (port of `resolveRequestOptions`).
    #[must_use]
    pub fn resolve_request_options(&self, request: &LLMRequest) -> LLMRequest {
        let messages = normalize_tool_history(&request.messages);
        let normalized = if messages.len() == request.messages.len() {
            let mut req = request.clone();
            req.messages = messages;
            req
        } else {
            request.update(LLMRequestPatch { messages: Some(messages), ..Default::default() })
        };
        let route_generation = self.defaults.generation.as_ref();
        let model_generation = request.model.defaults.as_ref().and_then(|d| d.generation.as_ref());
        let request_generation = normalized.generation.as_ref();
        let generation = crate::schema::options::merge_generation_options(&[
            route_generation,
            model_generation,
            request_generation,
        ]);
        let provider_options = crate::schema::options::merge_provider_options(&[
            self.defaults.provider_options.as_ref(),
            request.model.defaults.as_ref().and_then(|d| d.provider_options.as_ref()),
            normalized.provider_options.as_ref(),
        ]);
        let http = crate::schema::options::merge_http_options(&[
            self.defaults.http.as_ref(),
            request.model.defaults.as_ref().and_then(|d| d.http.as_ref()),
            normalized.http.as_ref(),
        ]);
        normalized.update(LLMRequestPatch {
            generation: Some(generation.unwrap_or_default()),
            provider_options,
            http,
            ..Default::default()
        })
    }

    /// Pre-body transforms + tool dedupe (port of `prepareRequest`, minus
    /// `sanitizeSurrogates` which is a Rust-level no-op — Rust `String`s are
    /// always well-formed UTF-8 and cannot carry lone surrogates).
    #[must_use]
    pub fn prepare_request(&self, request: &LLMRequest) -> LLMRequest {
        let original = self.resolve_request_options(request);
        // Deduplicate per sibling level; a tool and a namespace may share a name.
        let tools = dedupe_tools(original.tools.clone());
        let updated = original.update(LLMRequestPatch { tools: Some(tools), ..Default::default() });
        let resolved = apply_cache_policy(&apply_effort_updates(self, &updated));
        match &self.headers {
            Some(headers) if !headers(&resolved).is_empty() => {
                let header_map = headers(&resolved);
                let merged = crate::schema::options::merge_http_options(&[
                    Some(&HttpOptions { headers: Some(header_map), ..Default::default() }),
                    resolved.http.as_ref(),
                ]);
                resolved.update(LLMRequestPatch { http: merged, ..Default::default() })
            }
            _ => resolved,
        }
    }

    /// Build the provider-native body from a prepared request.
    pub fn build_body(&self, request: &LLMRequest) -> Result<serde_json::Value, AiError> {
        self.protocol.body_from(request)
    }

    /// Render the wire request (URL, headers, body text) for a prepared
    /// request + built body (port of `jsonRequestParts`): applies the
    /// caller's `http.query` over the endpoint query, overlays
    /// `http.body` onto the JSON body, and applies auth.
    pub fn prepare_transport(
        &self,
        request: &LLMRequest,
        body: &serde_json::Value,
    ) -> Result<(TransportRequest, String), AiError> {
        let mut url = endpoint::render(&self.endpoint, &EndpointInput { request, body });
        if let Some(query) = request.http.as_ref().and_then(|http| http.query.as_ref()) {
            for (key, value) in query {
                if let Some(entry) = url.query.iter_mut().find(|(k, _)| k == key) {
                    entry.1 = value.clone();
                } else {
                    url.query.push((key.clone(), value.clone()));
                }
            }
        }
        let url_string = url.to_string();

        let json_body = match request.http.as_ref().and_then(|http| http.body.as_ref()) {
            None => body.clone(),
            Some(overlay) => {
                if body.is_object() {
                    let merged = crate::schema::options::merge_json_records(&[
                        Some(body.as_object().unwrap()),
                        Some(overlay),
                    ])
                    .unwrap_or_default();
                    serde_json::Value::Object(merged)
                } else {
                    return Err(AiError::invalid_request(
                        "http.body can only overlay JSON object request bodies",
                    ));
                }
            }
        };
        let body_text = serde_json::to_string(&json_body).map_err(|error| {
            AiError::event_error(&self.id, format!("Failed to encode request body: {error}"))
        })?;

        let mut base_headers = Headers::new();
        if let Some(header_pairs) = self.defaults.headers.as_ref() {
            for (name, value) in header_pairs {
                base_headers.set(name.clone(), value.clone());
            }
        }
        if let Some(http) = request.http.as_ref() {
            if let Some(headers) = http.headers.as_ref() {
                for (name, value) in headers {
                    base_headers.set(name.clone(), value.clone());
                }
            }
        }
        let auth_input = auth::AuthInput {
            request_http: request.http.as_ref(),
            method: "POST",
            url: &url_string,
            body: &body_text,
            headers: base_headers,
        };
        let headers = self.auth.apply(&auth_input)?;
        let mut header_vec: Vec<(String, String)> =
            headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        // JSON body: content-type set after caller headers so routes cannot
        // accidentally send JSON with a stale content type (port of `jsonPost`).
        header_vec.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
        header_vec.push(("content-type".to_string(), "application/json".to_string()));

        Ok((
            TransportRequest::post(url_string)
                .with_headers(header_vec)
                .with_body(Bytes::from(body_text.clone())),
            body_text,
        ))
    }

    /// Stream provider-neutral events for one request through the full
    /// pipeline, enforcing the terminal-event contract (port of
    /// `streamPrepared` + `requireTerminalEvent`): events after `finish` /
    /// `provider-error` fail, and a stream that ends without one fails with
    /// `incomplete-stream`.
    pub async fn stream(
        &self,
        request: LLMRequest,
    ) -> Result<impl futures::Stream<Item = Result<LLMEvent, AiError>> + Send + 'static, AiError>
    {
        let prepared = self.prepare_request(&request);
        let body = self.build_body(&prepared)?;
        let (transport_request, _body_text) = self.prepare_transport(&prepared, &body)?;
        // The v2 HTTP timeout budget (AI-TIMEOUT): header wait, chunk idle,
        // and whole-request budget wrap the real transport call + body
        // stream (ref `route/transport/http.ts` execute).
        let budget = crate::route::transport::timeouts::TimeoutBudget::from_http_options(
            prepared.http.as_ref(),
        );
        let started = tokio::time::Instant::now();
        let response = crate::route::transport::timeouts::execute_with_timeouts(
            Arc::clone(&self.transport),
            transport_request.clone(),
            budget,
        )
        .await
        .map_err(map_transport_failure)?;

        // Wrap the body before status classification so error-body reads
        // run under the same budgets (v2 wraps the stream inside execute).
        let TransportResponse { status, headers, body } = response;
        let http_context =
            HttpContext { url: transport_request.url.clone(), status, headers: headers.clone() };
        let body = crate::route::transport::timeouts::body_with_timeouts(
            body,
            budget,
            started,
            &transport_request.url,
            http_context,
        );
        let response = TransportResponse { status, headers, body };

        if response.status >= 400 {
            return Err(status_failure(self, &transport_request, response).await);
        }

        let route_label = format!("{}/{}", prepared.model.provider, self.id);
        let initial_state = self.protocol.initial(&prepared);
        Ok(stream_events(
            self.id.clone(),
            route_label,
            Arc::clone(&self.protocol),
            self.framing.clone(),
            initial_state,
            response,
        ))
    }

    /// Collect a stream into a completed response (port of `generate`).
    pub async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, AiError> {
        let route = format!("{}/{}", request.model.provider, self.id);
        let stream = self.stream(request).await?;
        let state = stream
            .fold(Ok(ResponseState::empty()), |state, item| async move {
                match (state, item) {
                    (Ok(state), Ok(event)) => Ok(state.reduce(event)),
                    (Ok(_), Err(error)) => Err(error),
                    (Err(error), _) => Err(error),
                }
            })
            .await?;
        state.complete().ok_or_else(|| incomplete_stream_error(&route))
    }

    /// Whether this route's protocol lowers effort-update markers.
    #[must_use]
    pub fn supports_effort_updates(&self, request: &LLMRequest) -> bool {
        self.protocol.supports_effort_updates(request)
    }

    /// The protocol's default tool-schema sanitizer.
    #[must_use]
    pub fn sanitizer(&self) -> Option<crate::schema::SanitizerCompatibility> {
        self.protocol.sanitizer()
    }
}

fn map_transport_failure(error: TransportError) -> AiError {
    error.into_ai_error()
}

async fn status_failure(
    route: &Route,
    transport_request: &TransportRequest,
    response: TransportResponse,
) -> AiError {
    let _ = route;
    let body = read_body_limited(response.body, MAX_BODY_CHARS).await.unwrap_or_default();
    let message = provider_message(response.status, Some(&body));
    let response_headers: Vec<(String, String)> =
        response.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    http_failure(HttpFailureInput {
        message: &message,
        url: Some(&transport_request.url),
        status: Some(response.status),
        data: None,
        response_headers: &response_headers,
        response_body: Some(&body),
    })
}

fn incomplete_stream_error(route: &str) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields::new("The provider response ended unexpectedly."),
        classification: Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM.into()),
        route: Some(route.to_string()),
    })
}

fn event_after_terminal_error(route: &str, event: &LLMEvent) -> AiError {
    AiError::event_error(
        route,
        format!("Provider emitted {} after the terminal event", event.kind()),
    )
}

/// The event stream: frames → protocol state machine → terminal enforcement
/// (port of `streamPrepared` + `requireTerminalEvent`).
///
/// - the terminal frame is processed, then remaining frames are dropped
///   (port of `Stream.takeUntil(terminal)`);
/// - `on_halt` flushes once the framed stream ends;
/// - events after `finish`/`provider-error` fail with a typed error;
/// - a stream that ends without a terminal event fails with
///   `incomplete-stream`.
fn stream_events(
    route_id: String,
    route_label: String,
    protocol: Arc<dyn Protocol>,
    framing: Framing,
    initial_state: ProtocolState,
    response: TransportResponse,
) -> impl futures::Stream<Item = Result<LLMEvent, AiError>> + Send + 'static {
    async_stream::stream! {
        let mut state = initial_state;
        let mut terminal = false;
        let mut frames = Box::pin(frame_stream(framing, response.body));
        while let Some(frame) = frames.next().await {
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };
            match protocol.step(state, &frame) {
                Ok((next, events)) => {
                    state = next;
                    for event in events {
                        if terminal {
                            yield Err(event_after_terminal_error(&route_id, &event));
                            return;
                        }
                        if event.is_terminal() {
                            terminal = true;
                        }
                        yield Ok(event);
                    }
                }
                Err(mut error) => {
                    // Preserve the original frame on decode/parse failures
                    // (port of `frameError`).
                    if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                        if fields.body.is_none() {
                            fields.body = Some(frame.clone());
                        }
                    }
                    yield Err(error);
                    return;
                }
            }
            if protocol.terminal(&frame) {
                break;
            }
        }
        match protocol.on_halt(state) {
            Ok((_, events)) => {
                for event in events {
                    if terminal {
                        yield Err(event_after_terminal_error(&route_id, &event));
                        return;
                    }
                    if event.is_terminal() {
                        terminal = true;
                    }
                    yield Ok(event);
                }
            }
            Err(mut error) => {
                if let AiErrorReason::InvalidProviderOutput { fields, .. } = &mut error.reason {
                    if fields.body.is_none() {
                        fields.body = Some("stream halted".to_string());
                    }
                }
                yield Err(error);
                return;
            }
        }
        if !terminal {
            yield Err(incomplete_stream_error(&route_label));
        }
    }
}

/// Deduplicate tools per sibling level by `type:name` (port of the
/// `prepareRequest` dedupe).
fn dedupe_tools(tools: Vec<crate::schema::ToolEntry>) -> Vec<crate::schema::ToolEntry> {
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for tool in tools {
        let key = format!("{}:{}", tool_kind(&tool), tool.name());
        if !seen.insert(key) {
            continue;
        }
        result.push(match tool {
            crate::schema::ToolEntry::Namespace(mut namespace) => {
                namespace.tools = dedupe_tools(namespace.tools);
                crate::schema::ToolEntry::Namespace(namespace)
            }
            definition => definition,
        });
    }
    result
}

fn tool_kind(entry: &crate::schema::ToolEntry) -> &'static str {
    match entry {
        crate::schema::ToolEntry::Definition(_) => "tool",
        crate::schema::ToolEntry::Namespace(_) => "namespace",
    }
}

/// Merge route defaults (port of `mergeRouteDefaults`): maps merge per key,
/// generation/providerOptions/http deep-merge, and merged `headers` also
/// land in `http.headers`.
#[must_use]
pub fn merge_route_defaults(base: Option<&RouteDefaults>, patch: &RouteDefaults) -> RouteDefaults {
    let headers = crate::schema::options::merge_string_records(&[
        base.and_then(|d| d.headers.as_ref()),
        patch.headers.as_ref(),
    ]);
    let generation = crate::schema::options::merge_generation_options(&[
        base.and_then(|d| d.generation.as_ref()),
        patch.generation.as_ref(),
    ]);
    let provider_options = crate::schema::options::merge_provider_options(&[
        base.and_then(|d| d.provider_options.as_ref()),
        patch.provider_options.as_ref(),
    ]);
    let http = crate::schema::options::merge_http_options(&[
        base.and_then(|d| d.http.as_ref()),
        patch.http.as_ref(),
        headers
            .as_ref()
            .map(|headers| HttpOptions { headers: Some(headers.clone()), ..Default::default() })
            .as_ref(),
    ]);
    RouteDefaults { headers, generation, provider_options, http }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::auth::Secret;
    use crate::schema::{
        AiErrorReason, GenerationOptions, HttpOptions, LanguageModelDefaults, Message,
        ProviderOptions, TransportOperation,
    };
    use crate::test_support::{echo_route, RecordedInteraction};
    use serde_json::json;

    fn interaction(body: &str, status: u16) -> RecordedInteraction {
        RecordedInteraction {
            method: "POST".into(),
            url: "https://echo.test/echo".into(),
            status,
            body: body.into(),
            request_body: None,
        }
    }

    fn request(route: &Route) -> LLMRequest {
        LLMRequest::new(route.model("test-model", None).unwrap()).with_prompt("hi")
    }

    #[tokio::test]
    async fn merges_defaults_route_over_model_over_request() {
        let transport: SharedTransport = Arc::new(crate::route::transport::FailingTransport {
            message: "no interactions in this test",
        });
        let mut route = echo_route(Arc::clone(&transport));
        route = route.with(RoutePatch {
            defaults: RouteDefaults {
                generation: Some(GenerationOptions {
                    temperature: Some(0.7),
                    max_tokens: Some(1024.0),
                    ..Default::default()
                }),
                provider_options: Some(ProviderOptions::from_iter([(
                    "shared".to_string(),
                    json!("route"),
                )])),
                http: Some(HttpOptions {
                    headers: Some([("x-route".to_string(), "1".to_string())].into()),
                    ..Default::default()
                }),
                headers: None,
            },
            ..Default::default()
        });
        // model defaults slot between route and request
        let model = route.model("test-model", None).unwrap().with_defaults(LanguageModelDefaults {
            generation: Some(GenerationOptions {
                temperature: Some(0.3),
                top_p: Some(0.2),
                ..Default::default()
            }),
            provider_options: None,
            http: Some(HttpOptions {
                headers: Some([("x-model".to_string(), "1".to_string())].into()),
                ..Default::default()
            }),
        });
        let mut req = LLMRequest::new(model);
        req.messages = vec![Message::user("hi")];
        req.generation = Some(GenerationOptions { max_tokens: Some(2048.0), ..Default::default() });
        let resolved = route.resolve_request_options(&req);
        let generation = resolved.generation.as_ref().unwrap();
        assert_eq!(generation.temperature, Some(0.3), "model wins over route");
        assert_eq!(generation.max_tokens, Some(2048.0), "request wins over everything");
        assert_eq!(generation.top_p, Some(0.2));
        let http = resolved.http.as_ref().unwrap();
        assert_eq!(http.headers.as_ref().unwrap()["x-route"], "1");
        assert_eq!(http.headers.as_ref().unwrap()["x-model"], "1");
        assert_eq!(resolved.provider_options.as_ref().unwrap()["shared"], json!("route"));
    }

    #[tokio::test]
    async fn with_merges_endpoint_query_and_replaces_auth_and_id() {
        let transport: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "x" });
        let mut query = BTreeMap::new();
        query.insert("keep".to_string(), "base".to_string());
        query.insert("base".to_string(), "1".to_string());
        let base = echo_route(Arc::clone(&transport)).with(RoutePatch {
            id: Some("base-chat".into()),
            endpoint: Some(EndpointPatch {
                base_url: Some("https://api.example.test/v1".into()),
                path: None,
                query: Some(query),
            }),
            auth: Some(auth::Definition::headers([("x-auth".to_string(), "old".to_string())])),
            ..Default::default()
        });
        let mut patch_query = BTreeMap::new();
        patch_query.insert("keep".to_string(), "patch".to_string());
        patch_query.insert("patch".to_string(), "1".to_string());
        let patched = base.with(RoutePatch {
            id: Some("patched-chat".into()),
            endpoint: Some(EndpointPatch { base_url: None, path: None, query: Some(patch_query) }),
            auth: Some(auth::Definition::headers([("x-auth".to_string(), "new".to_string())])),
            ..Default::default()
        });
        assert_eq!(patched.id, "patched-chat");
        let query = patched.endpoint.query.as_ref().unwrap();
        assert_eq!(query["keep"], "patch");
        assert_eq!(query["base"], "1");
        assert_eq!(query["patch"], "1");
    }

    #[tokio::test]
    async fn provider_metadata_key_follows_a_replacement_provider() {
        let transport: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "x" });
        let route = echo_route(Arc::clone(&transport))
            .with(RoutePatch { provider: Some("azure".into()), ..Default::default() });
        assert_eq!(route.provider_metadata_key.as_deref(), Some("azure"));
        let overridden = route
            .with(RoutePatch {
                provider_metadata_key: Some("custom-azure".into()),
                ..Default::default()
            })
            .with(RoutePatch::default());
        assert_eq!(overridden.provider_metadata_key.as_deref(), Some("custom-azure"));
    }

    #[tokio::test]
    async fn model_requires_a_base_url() {
        let transport: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "x" });
        let mut route = echo_route(Arc::clone(&transport));
        route.endpoint = Endpoint {
            base_url: None,
            path: crate::route::endpoint::EndpointPart::Static("/echo".into()),
            query: None,
        };
        let error = route.model("m1", None).unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::ProviderConfiguration { .. }));
        assert!(error.message().contains("requires an endpoint baseURL"));
    }

    #[tokio::test]
    async fn auth_and_json_content_type_flow_onto_the_transport_request() {
        let transport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "data: ok\n\n",
                200,
            )]));
        let mut route = echo_route(Arc::clone(&transport) as SharedTransport);
        route = route.with(RoutePatch {
            auth: Some(auth::Definition::header("x-api-key", Secret::Value("k-1".into()))),
            ..Default::default()
        });
        let response = route.generate(request(&route)).await.unwrap();
        assert_eq!(response.text(), "ok");
        let sent = &transport.requests()[0];
        assert_eq!(sent.header("x-api-key"), Some("k-1"));
        assert_eq!(sent.header("content-type"), Some("application/json"));
        let body: serde_json::Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(body["prompt"], "hi");
    }

    #[tokio::test]
    async fn http_query_overrides_the_endpoint_query() {
        let transport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "data: ok\n\n",
                200,
            )]));
        let mut query = BTreeMap::new();
        query.insert("alt".to_string(), "sse".to_string());
        query.insert("keep".to_string(), "endpoint".to_string());
        let mut route = echo_route(Arc::clone(&transport) as SharedTransport);
        route.endpoint = route.endpoint.with_query(query);
        let mut req = request(&route);
        let mut caller_query = BTreeMap::new();
        caller_query.insert("alt".to_string(), "json".to_string());
        req.http = Some(HttpOptions { query: Some(caller_query), ..Default::default() });
        route.generate(req).await.unwrap();
        let url = &transport.requests()[0].url;
        assert!(url.contains("alt=json"), "caller query wins: {url}");
        assert!(url.contains("keep=endpoint"), "endpoint-only params survive: {url}");
    }

    #[tokio::test]
    async fn http_body_overlays_the_built_body() {
        let transport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "data: ok\n\n",
                200,
            )]));
        let route = echo_route(Arc::clone(&transport) as SharedTransport);
        let mut req = request(&route);
        req.http = Some(HttpOptions {
            body: Some([("extra".to_string(), json!(42))].into_iter().collect()),
            ..Default::default()
        });
        route.generate(req).await.unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&transport.requests()[0].body).unwrap();
        assert_eq!(body["prompt"], "hi");
        assert_eq!(body["extra"], 42);
    }

    #[tokio::test]
    async fn non_2xx_responses_classify_through_provider_error() {
        let transport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "{\"error\":{\"message\":\"Rate limit exceeded\",\"type\":\"rate_limit_error\"}}",
                429,
            )]));
        let route = echo_route(Arc::clone(&transport) as SharedTransport);
        let error = route.generate(request(&route)).await.unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::RateLimit { .. }), "got {}", error.tag());
        assert!(error.message().contains("Rate limit exceeded"));
    }

    /// A protocol that consumes frames but never emits a terminal event —
    /// unlike `EchoProtocol`, whose `on_halt` synthesizes finish (Gemini-style).
    struct BareProtocol;

    impl crate::route::protocol::Protocol for BareProtocol {
        fn id(&self) -> &str {
            "bare"
        }

        fn body_from(&self, _request: &LLMRequest) -> Result<serde_json::Value, AiError> {
            Ok(serde_json::json!({}))
        }

        fn initial(&self, _request: &LLMRequest) -> crate::route::protocol::ProtocolState {
            Box::new(())
        }

        fn step(
            &self,
            state: crate::route::protocol::ProtocolState,
            _frame: &str,
        ) -> Result<(crate::route::protocol::ProtocolState, Vec<LLMEvent>), AiError> {
            Ok((state, Vec::new()))
        }
    }

    fn bare_route(transport: SharedTransport) -> Route {
        Route::new(RouteInput {
            id: "bare".into(),
            provider: Some("test".into()),
            provider_metadata_key: None,
            protocol: Arc::new(BareProtocol),
            endpoint: Endpoint::path("/bare").with_base_url("https://echo.test"),
            auth: None,
            framing: Framing::sse(),
            headers: None,
            transport,
            defaults: Default::default(),
        })
    }

    #[tokio::test]
    async fn missing_terminal_finish_fails_with_incomplete_stream() {
        let transport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "data: partial\n\n",
                200,
            )]));
        let route = bare_route(Arc::clone(&transport) as SharedTransport);
        let error = route.generate(request(&route)).await.unwrap_err();
        match error.reason {
            AiErrorReason::InvalidProviderOutput { classification, .. } => {
                assert_eq!(
                    classification.as_deref(),
                    Some(crate::schema::CLASSIFICATION_INCOMPLETE_STREAM)
                );
            }
            other => panic!("expected InvalidProviderOutput, got {}", other.tag()),
        }
    }

    #[tokio::test]
    async fn events_after_the_terminal_event_fail_the_stream() {
        // The echo protocol's on_halt emits a finish; a frame arriving after a
        // terminal event must fail. Simulated with a protocol that emits
        // finish on the first frame and more events on the second.
        struct FinishThenMore;

        impl crate::route::protocol::Protocol for FinishThenMore {
            fn id(&self) -> &str {
                "finish-then-more"
            }

            fn body_from(&self, _request: &LLMRequest) -> Result<serde_json::Value, AiError> {
                Ok(serde_json::json!({}))
            }

            fn initial(&self, _request: &LLMRequest) -> crate::route::protocol::ProtocolState {
                Box::new(0u32)
            }

            fn step(
                &self,
                state: crate::route::protocol::ProtocolState,
                _frame: &str,
            ) -> Result<(crate::route::protocol::ProtocolState, Vec<LLMEvent>), AiError>
            {
                let count = *state.downcast::<u32>().expect("state");
                let events = if count == 0 {
                    vec![LLMEvent::finish(crate::schema::FinishReasonDetails::new(
                        crate::schema::FinishReason::Stop,
                    ))]
                } else {
                    vec![LLMEvent::TextDelta {
                        id: "t".into(),
                        text: "late".into(),
                        provider_metadata: None,
                    }]
                };
                Ok((Box::new(count + 1), events))
            }
        }

        let transport: SharedTransport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "data: one\n\ndata: two\n\n",
                200,
            )]));
        let route = Route::new(RouteInput {
            id: "finish-then-more".into(),
            provider: Some("test".into()),
            provider_metadata_key: None,
            protocol: Arc::new(FinishThenMore),
            endpoint: Endpoint::path("/x").with_base_url("https://echo.test"),
            auth: None,
            framing: Framing::sse(),
            headers: None,
            transport,
            defaults: Default::default(),
        });
        let stream = route.stream(request(&route)).await.unwrap();
        let collected: Vec<Result<LLMEvent, AiError>> = stream.collect().await;
        let last = collected.last().expect("events");
        let error = last.as_ref().expect_err("terminal violation fails");
        assert!(error.message().contains("after the terminal event"), "got: {error}");
    }

    #[tokio::test]
    async fn transport_failures_surface_as_transport_errors() {
        let transport: SharedTransport =
            Arc::new(crate::route::transport::FailingTransport { message: "socket gone" });
        let route = echo_route(Arc::clone(&transport));
        let error = route.generate(request(&route)).await.unwrap_err();
        match error.reason {
            AiErrorReason::Transport { operation, .. } => {
                assert_eq!(operation, TransportOperation::Request);
            }
            other => panic!("expected Transport, got {}", other.tag()),
        }
    }

    #[tokio::test]
    async fn tool_history_normalization_runs_before_the_body_build() {
        let transport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "data: ok\n\n",
                200,
            )]));
        let route = echo_route(Arc::clone(&transport) as SharedTransport);
        let mut req = request(&route);
        // an unanswered tool call gains a synthesized tool-result message
        req.messages = vec![Message::tool_call("call_1", "lookup", json!({}))];
        route.generate(req).await.unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&transport.requests()[0].body).unwrap();
        let _ = body;
        // the normalization itself is asserted in tool_history tests; here we
        // only pin that compile succeeds through the full pipeline
    }

    #[tokio::test]
    async fn default_headers_from_route_flow_into_requests() {
        let transport =
            Arc::new(crate::test_support::MockTransport::from_interactions(vec![interaction(
                "data: ok\n\n",
                200,
            )]));
        let mut route = echo_route(Arc::clone(&transport) as SharedTransport);
        route = route.with(RoutePatch {
            defaults: RouteDefaults {
                headers: Some([("anthropic-version".to_string(), "2023-06-01".to_string())].into()),
                ..Default::default()
            },
            ..Default::default()
        });
        route.generate(request(&route)).await.unwrap();
        assert_eq!(transport.requests()[0].header("anthropic-version"), Some("2023-06-01"));
    }
}
