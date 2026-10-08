//! Compaction-checkpoint collection over a `compaction_trigger` response
//! (port of `src/protocols/utils/responses-checkpoint.ts`).
//!
//! The trigger appends `{type: "compaction_trigger"}` to the canonical input
//! and streams through the normal Responses transport; this machine watches
//! the stream, collects the single encrypted checkpoint the provider emits,
//! and lets no generation output escape.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::protocols::open_responses::{
    self, decode_event, map_usage, metadata_key, provider_failure, Event,
};
use crate::route::endpoint::{self, Endpoint, EndpointInput, EndpointPart};
use crate::route::framing::FramingDecoder;
use crate::route::transport::read_body_limited;
use crate::route::{executor, Route};
use crate::schema::{
    AiError, AiErrorReason, FinishReason, FinishReasonDetails, LLMEvent, LLMRequest, ReasonFields,
    Usage,
};
use futures::StreamExt;

/// One collected checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    pub provider: crate::schema::ProviderId,
    pub id: String,
    pub encrypted: String,
}

/// The trigger's result.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointOutcome {
    pub checkpoint: Checkpoint,
    pub response_id: String,
    pub usage: Option<Usage>,
}

/// Checkpoint machine state (port of `State`).
#[derive(Debug, Clone)]
pub struct CheckpointState {
    pub id: String,
    pub provider: crate::schema::ProviderId,
    pub output_items: BTreeMap<u64, String>,
    pub checkpoints: BTreeMap<String, Checkpoint>,
    pub response_id: Option<String>,
    /// Deterministic stand-in for the TS UUID mint.
    pub minted: u64,
    /// Set when `response.completed` produced exactly one checkpoint.
    pub outcome: Option<CheckpointOutcome>,
}

impl CheckpointState {
    #[must_use]
    pub fn new(request: &LLMRequest, route_id: &str) -> Self {
        Self {
            id: route_id.to_string(),
            provider: request.model.provider.clone(),
            output_items: BTreeMap::new(),
            checkpoints: BTreeMap::new(),
            response_id: None,
            minted: 0,
            outcome: None,
        }
    }
}

fn event_error(route: &str, message: impl Into<String>) -> AiError {
    crate::protocols::shared::event_error(route, message)
}

/// Collect compaction items from one output-item frame (port of
/// `onOutputItem`): register output slots, validate checkpoint invariants.
fn on_output_item(state: CheckpointState, event: &Event) -> Result<CheckpointState, AiError> {
    let Some(item) = event.item() else {
        return Ok(state);
    };
    let mut item = item;
    if item.id.is_none() {
        if let Some(slot) = event.output_index().and_then(|index| state.output_items.get(&index)) {
            item.id = Some(slot.clone());
        }
    }
    if item.id.is_none() {
        item.id = Some(format!("cmp_000000000000000000000000{:012x}", state.minted + 1));
    }
    let item_id = item.id.clone().unwrap_or_default();

    let mut state = state;
    if let Some(output_index) = event.output_index() {
        if state.output_items.get(&output_index).map(String::as_str) != Some(item_id.as_str()) {
            state.output_items.insert(output_index, item_id.clone());
        }
    }
    if event.kind == "response.output_item.added" || item.kind != "compaction" {
        return Ok(state);
    }
    if let Some(output_index) = event.output_index() {
        let duplicated = state
            .output_items
            .iter()
            .any(|(index, id)| *id == item_id.as_str() && *index != output_index);
        if duplicated {
            return Err(event_error(
                &state.id,
                "Compaction checkpoint appeared in multiple output slots",
            ));
        }
    }
    let Some(encrypted) = item.encrypted_content.clone() else {
        return Err(event_error(&state.id, "Compaction output is missing its encrypted content"));
    };
    if let Some(previous) = state.checkpoints.get(&item_id) {
        if previous.encrypted != encrypted {
            return Err(event_error(&state.id, "Compaction output changed after completion"));
        }
        return Ok(state);
    }
    state.checkpoints.insert(
        item_id.clone(),
        Checkpoint { provider: state.provider.clone(), id: item_id, encrypted },
    );
    Ok(state)
}

/// The checkpoint step machine (port of the `Protocol.make` step).
/// `metadata_key` resolves the usage provider-metadata bucket.
pub fn checkpoint_step(
    state: CheckpointState,
    event: &Event,
    metadata_key: &str,
) -> Result<(CheckpointState, Vec<LLMEvent>), AiError> {
    let mut state = state;
    if let Some(response_id) = event.response_id() {
        if let Some(previous) = state.response_id.clone() {
            if previous != response_id {
                return Err(event_error(
                    &state.id,
                    "Compaction response ID changed during execution",
                ));
            }
        }
    }
    match event.kind.as_str() {
        "response.created" => {
            state.response_id = event.response_id().map(str::to_string);
            return Ok((state, Vec::new()));
        }
        "error" | "response.failed" => {
            return Err(provider_failure(event, "Compaction request failed", None));
        }
        "response.incomplete" => {
            return Err(event_error(&state.id, "Compaction response was incomplete"));
        }
        "response.output_item.added" | "response.output_item.done" => {
            return Ok((on_output_item(state, event)?, Vec::new()));
        }
        _ if event.kind != "response.completed" => return Ok((state, Vec::new())),
        _ => {}
    }
    let response_id = event.response_id().unwrap_or_default().trim().to_string();
    if response_id.is_empty() {
        return Err(event_error(&state.id, "Compaction response is missing its response ID"));
    }
    if let Some(status) = event
        .response()
        .and_then(|response| response.get("status"))
        .filter(|status| !status.is_null())
    {
        if status.as_str() != Some("completed") {
            return Err(event_error(
                &state.id,
                "Compaction response did not complete successfully",
            ));
        }
    }
    for (index, raw) in event.response_output().iter().enumerate() {
        let done = Event {
            kind: "response.output_item.done".into(),
            raw: serde_json::json!({
                "type": "response.output_item.done",
                "output_index": index,
                "item": raw,
            }),
        };
        state = on_output_item(state, &done)?;
    }
    if state.checkpoints.len() != 1 {
        return Err(event_error(
            &state.id,
            "Compaction response must contain exactly one checkpoint",
        ));
    }
    let checkpoint = state.checkpoints.values().next().cloned().unwrap_or_else(|| Checkpoint {
        provider: state.provider.clone(),
        id: String::new(),
        encrypted: String::new(),
    });
    let usage = map_usage(event.response_usage(), metadata_key);
    state.outcome = Some(CheckpointOutcome { checkpoint, response_id, usage });
    Ok((
        state,
        vec![LLMEvent::Finish {
            reason: FinishReasonDetails::new(FinishReason::Stop),
            usage: None,
            provider_metadata: None,
        }],
    ))
}

/// Execute a `compaction_trigger` request against a route (port of
/// `ResponsesCheckpoint.make`): stream the response through the checkpoint
/// machine and return the single collected checkpoint.
pub async fn trigger_compaction(
    route: &Route,
    request: &LLMRequest,
    body: &Value,
) -> Result<CheckpointOutcome, AiError> {
    // Trigger compaction requires the full canonical body; the request's own
    // http.body overlay has already been folded into `body` by the builder.
    let stripped = strip_http_body(request);
    let (transport_request, _body_text) = route.prepare_transport(&stripped, body)?;
    let response = route
        .transport
        .execute(transport_request.clone())
        .await
        .map_err(crate::route::transport::TransportError::into_ai_error)?;

    if response.status >= 400 {
        let body_text =
            read_body_limited(response.body, executor::MAX_BODY_CHARS).await.unwrap_or_default();
        let message = executor::provider_message(response.status, Some(&body_text));
        let response_headers: Vec<(String, String)> =
            response.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        return Err(executor::http_failure(executor::HttpFailureInput {
            message: &message,
            url: Some(&transport_request.url),
            status: Some(response.status),
            data: None,
            response_headers: &response_headers,
            response_body: Some(&body_text),
        }));
    }

    let key = metadata_key(request);
    let mut state = CheckpointState::new(request, &route.id);
    let mut decoder = FramingDecoder::new(route.framing.clone());
    let mut stream = response.body;
    let mut finished = false;
    let drive =
        |frame: &str, state: &mut CheckpointState, finished: &mut bool| -> Result<(), AiError> {
            let event = decode_event(frame)?;
            let is_terminal = open_responses::terminal(&event);
            let (next, _events) = checkpoint_step(
                std::mem::replace(state, CheckpointState::new(request, &route.id)),
                &event,
                &key,
            )?;
            *state = next;
            if is_terminal {
                *finished = true;
            }
            Ok(())
        };
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(crate::route::transport::TransportError::into_ai_error)?;
        for frame in decoder.feed(&chunk)? {
            drive(&frame, &mut state, &mut finished)?;
            if finished {
                break;
            }
        }
        if finished {
            break;
        }
    }
    if !finished {
        for frame in decoder.finish()? {
            drive(&frame, &mut state, &mut finished)?;
            if finished {
                break;
            }
        }
    }
    if let Some(outcome) = state.outcome {
        return Ok(outcome);
    }
    Err(event_error(&route.id, "Compaction response ended without a checkpoint"))
}

fn strip_http_body(request: &LLMRequest) -> LLMRequest {
    let Some(http) = request.http.as_ref() else {
        return request.clone();
    };
    request.clone().update(crate::schema::LLMRequestPatch {
        http: Some(crate::schema::HttpOptions { body: None, ..http.clone() }),
        ..Default::default()
    })
}

/// Build the `/{path}/compact` endpoint for the standalone compaction API.
#[must_use]
pub fn compact_endpoint(route: &Route, request: &LLMRequest, native: &Value) -> Endpoint {
    let rendered = endpoint::render(&route.endpoint, &EndpointInput { request, body: native });
    let base = format!("{}/compact", rendered.base.trim_end_matches('/'));
    Endpoint {
        base_url: None,
        path: EndpointPart::Static(base),
        query: route.endpoint.query.clone(),
    }
}

/// A typed wrapper keeping the error taxonomy readable for callers.
#[must_use]
pub fn invalid_output(route: &str, message: &str, body: &str) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields {
            message: message.to_string(),
            body: Some(body.to_string()),
            http: None,
            cause: None,
        },
        classification: None,
        route: Some(route.to_string()),
    })
}
