#![allow(clippy::doc_markdown, reason = "OpenResponses/Azure identifiers in prose stay unquoted")]

//! OpenResponses channel transport (port of
//! `packages/ai/src/protocols/open-responses-channel.ts`).
//!
//! Prepares the WebSocket form of an Open Responses request: the HTTP SSE
//! request (kept as the channel's fallback), and — when the endpoint offers
//! WS — the ws URL (converted), handshake headers (mapped), and the driver
//! chain (base channel driver wrapped by
//! [`crate::protocols::open_responses_continuation`]). The create message is
//! the JSON body with `stream`/`stream_options`/`background` stripped and
//! `type: "response.create"` added; execution goes through a
//! [`WebSocketChannelExecutor`].

use std::pin::Pin;
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::protocols::open_responses::{self, provider_failure};
use crate::protocols::open_responses_continuation::{
    self, ContinuationDriverInput, ContinuationShape,
};
use crate::protocols::shared::invalid_request;
use crate::route::Route;
use crate::route::auth::Headers;
use crate::route::client::{execute_http_frames, frames_to_events};
use crate::route::transport::websocket::{
    self, ChannelConnect, FallbackFn, WebSocketChannelDriver, WebSocketChannelExchange,
    WebSocketChannelExecution, WebSocketChannelExecutor,
};
use crate::schema::{
    AiError, AiErrorReason, HttpContext, HttpOptions, LLMEvent, LLMRequest, ReasonFields,
};

/// Channel transport options (v2 `Options`): identity, WS availability gate,
/// URL/header mapping, rotation age, and the continuation shape.
#[derive(Clone)]
pub struct ChannelOptions {
    pub id: String,
    pub name: String,
    pub rotate_after_ms: Option<u64>,
    /// Offer the channel for this HTTP URL (default: always).
    pub enabled: Option<Arc<dyn Fn(&str) -> bool + Send + Sync>>,
    /// Map the HTTP URL before ws conversion (default: identity).
    pub url: Option<Arc<dyn Fn(&str) -> String + Send + Sync>>,
    /// Map the handshake headers (default: identity).
    pub headers: Option<Arc<dyn Fn(Headers) -> Headers + Send + Sync>>,
    /// Provider continuation field selection (default: identity).
    pub continuation: Option<ContinuationShape>,
}

impl ChannelOptions {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            rotate_after_ms: None,
            enabled: None,
            url: None,
            headers: None,
            continuation: None,
        }
    }

    #[must_use]
    pub fn with_rotate_after_ms(mut self, ms: u64) -> Self {
        self.rotate_after_ms = Some(ms);
        self
    }

    #[must_use]
    pub fn with_enabled(mut self, enabled: Arc<dyn Fn(&str) -> bool + Send + Sync>) -> Self {
        self.enabled = Some(enabled);
        self
    }

    #[must_use]
    pub fn with_url(mut self, url: Arc<dyn Fn(&str) -> String + Send + Sync>) -> Self {
        self.url = Some(url);
        self
    }

    #[must_use]
    pub fn with_headers(mut self, headers: Arc<dyn Fn(Headers) -> Headers + Send + Sync>) -> Self {
        self.headers = Some(headers);
        self
    }
}

/// The prepared channel form of one request (v2 `Prepared`).
pub struct ChannelPrepared {
    /// The ws:// form of the endpoint.
    pub ws_url: String,
    /// Handshake headers (content-length stripped, options-mapped).
    pub headers: Vec<(String, String)>,
    pub rotate_after_ms: Option<u64>,
    pub driver: Arc<dyn WebSocketChannelDriver>,
    /// The HTTP SSE request for fallback.
    pub http: crate::route::transport::TransportRequest,
}

/// Build the create message (v2 `message`): strip `stream`,
/// `stream_options`, `background`, add `type: "response.create"`.
pub fn channel_message(body: &Value) -> Result<(Map<String, Value>, String), AiError> {
    let Some(object) = body.as_object() else {
        return Err(invalid_request("Open Responses WebSocket body must be a JSON object"));
    };
    let mut request = object.clone();
    request.remove("stream");
    request.remove("stream_options");
    request.remove("background");
    request.insert("type".to_string(), Value::String("response.create".to_string()));
    let message = serde_json::to_string(&Value::Object(request.clone()))
        .map_err(|error| invalid_request(format!("Failed to encode channel message: {error}")))?;
    Ok((request, message))
}

/// The base channel driver (v2 `driver` in open-responses-channel.ts):
/// response-id tracking, terminal classification, and the protocol
/// invariants (duplicate `response.created`, id changes, events before
/// `response.created` or after a terminal event).
pub struct BaseChannelDriver {
    id: String,
    name: String,
    message: String,
    state: std::sync::Mutex<BaseState>,
}

#[derive(Default)]
struct BaseState {
    response_id: Option<String>,
    terminal: bool,
}

impl BaseChannelDriver {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            message: message.into(),
            state: std::sync::Mutex::new(BaseState::default()),
        }
    }

    fn event_error(&self, message: impl Into<String>, frame: &str) -> AiError {
        AiError::new(AiErrorReason::InvalidProviderOutput {
            route: Some(self.id.clone()),
            fields: ReasonFields {
                message: message.into(),
                body: Some(frame.to_string()),
                cause: None,
                http: None,
            },
            classification: None,
        })
    }
}

impl WebSocketChannelDriver for BaseChannelDriver {
    fn create(
        &self,
        _checkpoint: Option<&websocket::ChannelCheckpoint>,
    ) -> Result<websocket::ChannelCreate, AiError> {
        if let Ok(mut state) = self.state.lock() {
            state.response_id = None;
            state.terminal = false;
        }
        Ok(websocket::ChannelCreate {
            message: self.message.clone(),
            mode: websocket::ChannelMode::Full,
        })
    }

    fn observe(
        &self,
        _create: &websocket::ChannelCreate,
        frame: &str,
    ) -> Result<websocket::ChannelObservation, AiError> {
        let event = open_responses::decode_channel_event(frame).map_err(|cause| {
            AiError::new(AiErrorReason::InvalidProviderOutput {
                route: Some(self.id.clone()),
                fields: ReasonFields {
                    message: format!("Invalid {} WebSocket event", self.name),
                    body: Some(frame.to_string()),
                    cause: Some(cause.message().to_string()),
                    http: None,
                },
                classification: None,
            })
        })?;
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.terminal {
            return Err(self.event_error(
                format!("{} emitted {} after a terminal event", self.name, event.kind),
                frame,
            ));
        }
        if event.kind == "error" {
            state.terminal = true;
            return Ok(websocket::ChannelObservation::ProviderFailure {
                error: provider_failure(
                    &event,
                    &format!("{} stream error", self.name),
                    Some(frame),
                ),
            });
        }
        if event.kind == "response.failed" {
            state.terminal = true;
            if let (Some(current), Some(next)) = (state.response_id.as_deref(), event.response_id())
            {
                if next != current {
                    return Err(self.event_error(
                        format!("{} response ID changed during execution", self.name),
                        frame,
                    ));
                }
            }
            return Ok(websocket::ChannelObservation::ProviderFailure {
                error: provider_failure(
                    &event,
                    &format!("{} response failed", self.name),
                    Some(frame),
                ),
            });
        }
        if event.kind == "response.created" {
            if state.response_id.is_some() {
                return Err(self.event_error(
                    format!("{} emitted duplicate response.created", self.name),
                    frame,
                ));
            }
            let Some(created) = event.response_id() else {
                return Err(self.event_error(
                    format!("{} response.created is missing response.id", self.name),
                    frame,
                ));
            };
            state.response_id = Some(created.to_string());
            return Ok(websocket::ChannelObservation::Frame { frame: frame.to_string() });
        }
        // Keepalives and provider notifications carry no response state and
        // may precede response.created.
        if !event.kind.starts_with("response.") {
            return Ok(websocket::ChannelObservation::Frame { frame: frame.to_string() });
        }
        if state.response_id.is_none() {
            return Err(self.event_error(
                format!("{} emitted {} before response.created", self.name, event.kind),
                frame,
            ));
        }
        if let Some(next) = event.response_id() {
            if Some(next) != state.response_id.as_deref() {
                return Err(self.event_error(
                    format!("{} response ID changed during execution", self.name),
                    frame,
                ));
            }
        }
        if event.kind == "response.completed" {
            state.terminal = true;
            return Ok(websocket::ChannelObservation::Completed {
                frame: frame.to_string(),
                checkpoint: None,
            });
        }
        if event.kind == "response.incomplete" {
            state.terminal = true;
            return Ok(websocket::ChannelObservation::Incomplete { frame: frame.to_string() });
        }
        Ok(websocket::ChannelObservation::Frame { frame: frame.to_string() })
    }
}

/// Prepare the channel form of one already-prepared request (v2
/// `OpenResponsesChannel.transport.prepare`): reuses the HTTP pipeline's
/// transport request (URL, auth headers, merged body), maps URL + headers,
/// and builds the driver chain. `None` when the endpoint does not offer WS.
fn prepare_channel_parts(
    route: &Route,
    prepared: &LLMRequest,
    body: &Value,
    options: &ChannelOptions,
) -> Result<Option<ChannelPrepared>, AiError> {
    let (transport_request, body_text) = route.prepare_transport(prepared, body)?;
    let http_url = transport_request.url.clone();
    if options.enabled.as_ref().is_some_and(|enabled| !enabled(&http_url)) {
        return Ok(None);
    }
    let mut headers = Headers::from_pairs(transport_request.headers.clone());
    if let Some(map) = options.headers.as_ref() {
        headers = map(headers);
    }
    headers.remove("content-length");
    let json_body: Value = serde_json::from_str(&body_text)
        .map_err(|error| invalid_request(format!("Channel body is not JSON: {error}")))?;
    let (channel_request, message) = channel_message(&json_body)?;
    let base = Arc::new(BaseChannelDriver::new(options.id.clone(), options.name.clone(), message));
    let driver = open_responses_continuation::driver(ContinuationDriverInput {
        id: options.id.clone(),
        name: options.name.clone(),
        request: channel_request,
        base,
        continuation: options.continuation.clone(),
    });
    let ws_url = websocket::to_websocket_url(
        &options.url.as_ref().map_or_else(|| http_url.clone(), |map| map(&http_url)),
    )?;
    Ok(Some(ChannelPrepared {
        ws_url,
        headers: headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
        rotate_after_ms: options.rotate_after_ms,
        driver,
        http: transport_request,
    }))
}

/// Build the HTTP fallback closure for one prepared channel exchange (the
/// SSE frames stream of the same request).
pub fn http_fallback(
    route: &Route,
    transport_request: crate::route::transport::TransportRequest,
    http_options: Option<HttpOptions>,
) -> FallbackFn {
    let transport = Arc::clone(&route.transport);
    let framing = route.framing.clone();
    Arc::new(move || {
        let transport = Arc::clone(&transport);
        let framing = framing.clone();
        let request = transport_request.clone();
        let http_options = http_options.clone();
        Box::pin(async move {
            execute_http_frames(&transport, &framing, &request, http_options.as_ref()).await
        })
    })
}

/// The channel stream result: provider-neutral events plus the continuation
/// checkpoint commit handle (call `complete` after the events stream ended
/// successfully — v2 `execution.complete`).
pub struct ChannelStream {
    pub events: Pin<Box<dyn futures::Stream<Item = Result<LLMEvent, AiError>> + Send>>,
    pub http: Option<HttpContext>,
    pub complete: websocket::CompleteFn,
}

/// Stream one request through the channel path (v2 `Route.stream` with
/// `webSocket` set): prepare → channel exchange → protocol state machine.
/// When the endpoint offers no WS channel, this falls back to the plain
/// HTTP SSE path ([`Route::stream`]).
pub async fn stream_channel(
    route: &Route,
    request: LLMRequest,
    executor: Arc<dyn WebSocketChannelExecutor>,
    options: &ChannelOptions,
) -> Result<ChannelStream, AiError> {
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared)?;
    let http_options = prepared.http.clone();
    let channel = prepare_channel_parts(route, &prepared, &body, options)?;
    let Some(channel) = channel else {
        let events = route.stream(request).await?;
        return Ok(ChannelStream {
            events: Box::pin(events),
            http: None,
            complete: Arc::new(|| None),
        });
    };
    let fallback = http_fallback(route, channel.http, http_options);
    let exchange = WebSocketChannelExchange {
        id: format!("{}/{}", prepared.model.provider, route.id),
        connect: ChannelConnect {
            url: channel.ws_url,
            headers: channel.headers,
            rotate_after_ms: channel.rotate_after_ms,
        },
        fallback,
        driver: channel.driver,
    };
    let execution: WebSocketChannelExecution = executor.execute(exchange).await?;
    let initial_state = route.protocol.initial(&prepared);
    let events = frames_to_events(
        route.id.clone(),
        format!("{}/{}", prepared.model.provider, route.id),
        Arc::clone(&route.protocol),
        initial_state,
        execution.frames,
    );
    Ok(ChannelStream {
        events: Box::pin(events),
        http: execution.http,
        complete: execution.complete,
    })
}

/// The channel options a route offers (v2: the openai/xai/azure built-ins
/// default `transport: "websocket"`, but only OpenResponses WS-capable
/// protocols carry a channel — xai-responses and every non-responses
/// protocol return `None` and stay on HTTP SSE). Azure-flavored
/// OpenResponses routes use the Azure-native gate + `api-version`-stripped
/// WS URL.
#[must_use]
pub fn channel_options_for_route(route: &Route) -> Option<ChannelOptions> {
    if route.protocol.id() != crate::protocols::openai_responses::ADAPTER {
        return None;
    }
    if route.provider.as_ref().is_some_and(|p| p.as_str() == crate::protocols::azure::ID) {
        return Some(crate::protocols::azure::responses_channel_options());
    }
    Some(crate::protocols::openai_responses::channel_transport())
}

/// Prepare the channel form of one request (used by tests and callers that
/// need the exchange parts without executing).
pub fn prepare_channel(
    route: &Route,
    request: &LLMRequest,
    options: &ChannelOptions,
) -> Result<Option<ChannelPrepared>, AiError> {
    let prepared = route.prepare_request(request);
    let body = route.build_body(&prepared)?;
    prepare_channel_parts(route, &prepared, &body, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn channel_message_strips_stream_fields_and_tags_the_create() {
        let body = json!({
            "model": "gpt-test",
            "stream": true,
            "stream_options": {"include_usage": true},
            "background": false,
            "input": [],
        });
        let (request, message) = channel_message(&body).unwrap();
        assert_eq!(request["type"], "response.create");
        assert!(request.get("stream").is_none());
        assert!(request.get("stream_options").is_none());
        assert!(request.get("background").is_none());
        let decoded: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(decoded["model"], "gpt-test");
        assert!(channel_message(&json!([])).is_err());
    }
}
