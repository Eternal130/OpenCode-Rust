//! Message handlers (`session.message.list` / `session.message.get`):
//! limit 50, order desc default, cursor `{id, order, direction}` with the
//! cursor-XOR-order → `InvalidCursorError` 400 rule (deep-map §3).

#![allow(
    clippy::must_use_candidate,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use oc_core::session::store::{ListDirection, ListOrder, MessagesInput};
use serde_json::{json, Value};

use super::common::{
    decode_message_cursor, encode_message_cursor, json_data, message_row_to_info, ApiError,
    CursorDirection, ListOrderWire, SharedState,
};
use super::session::{query_field, session_services, DEFAULT_LIMIT};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/session/:sessionID/message", axum::routing::get(list))
        .route("/api/session/:sessionID/message/:messageID", axum::routing::get(get))
}

async fn list(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    uri: Uri,
    _headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;

    let limit = query_field(&uri, "limit")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(DEFAULT_LIMIT);
    if limit <= 0 {
        return Err(ApiError::invalid_request("limit must be a positive integer"));
    }
    let requested = query_field(&uri, "order").unwrap_or_else(|| "desc".to_owned());
    let order = match requested.as_str() {
        "asc" => ListOrder::Asc,
        "desc" => ListOrder::Desc,
        other => {
            return Err(ApiError::invalid_request(format!("invalid order: {other}")));
        }
    };
    let type_ = query_field(&uri, "type");

    let cursor_param = query_field(&uri, "cursor");
    let cursor = match &cursor_param {
        Some(encoded) => {
            let decoded = decode_message_cursor(encoded)
                .ok_or_else(|| ApiError::invalid_cursor("Invalid cursor"))?;
            // cursor XOR order → InvalidCursorError (§3).
            let wire_order = match decoded.order {
                ListOrderWire::Asc => "asc",
                ListOrderWire::Desc => "desc",
            };
            if wire_order != requested {
                return Err(ApiError::invalid_cursor(format!(
                    "Cursor order {wire_order} does not match requested order {requested}"
                )));
            }
            Some((decoded.id, direction_of(decoded.direction)))
        }
        None => None,
    };

    let rows = services
        .store
        .messages(MessagesInput {
            session_id: session_id.clone(),
            limit: Some(limit + 1),
            order: Some(order),
            type_,
            cursor,
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("message:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();

    let wire_order = match order {
        ListOrder::Asc => ListOrderWire::Asc,
        ListOrder::Desc => ListOrderWire::Desc,
    };
    let mut next = None;
    let mut previous = None;
    if has_more {
        if let Some(last) = rows.last() {
            next = encode_message_cursor(&super::common::MessageCursor {
                id: last.id.clone(),
                order: wire_order,
                direction: CursorDirection::Next,
            });
        }
    }
    if cursor_param.is_some() {
        if let Some(first) = rows.first() {
            previous = encode_message_cursor(&super::common::MessageCursor {
                id: first.id.clone(),
                order: wire_order,
                direction: CursorDirection::Previous,
            });
        }
    }

    let data: Vec<Value> = rows.iter().map(message_row_to_info).collect();
    Ok(axum::Json(json!({
        "data": data,
        "cursor": { "previous": previous, "next": next },
    }))
    .into_response())
}

fn direction_of(direction: CursorDirection) -> ListDirection {
    match direction {
        CursorDirection::Next => ListDirection::Next,
        CursorDirection::Previous => ListDirection::Previous,
    }
}

async fn get(
    State(state): State<SharedState>,
    Path((session_id, message_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let rows = services
        .store
        .messages(MessagesInput { session_id: session_id.clone(), ..Default::default() })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("message:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let row = rows
        .into_iter()
        .find(|row| row.id == message_id)
        .ok_or_else(|| ApiError::message_not_found(&session_id, &message_id))?;
    Ok(json_data(message_row_to_info(&row)))
}
