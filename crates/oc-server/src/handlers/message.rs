//! Message handlers (`session.message.list` / `session.message.get`):
//! limit 50, order desc default, cursor `{id, order, direction}` —
//! cursor+order → `InvalidCursorError`, cursor-only pages take the
//! order from the token (v2 `handlers/message.ts:35-62`).

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
    let order_param = query_field(&uri, "order");
    let order = match order_param.as_deref() {
        Some("asc") => ListOrder::Asc,
        Some(other) if other != "desc" => {
            return Err(ApiError::invalid_request(format!("invalid order: {other}")));
        }
        _ => ListOrder::Desc,
    };
    let type_ = query_field(&uri, "type");

    let cursor_param = query_field(&uri, "cursor");
    // v2 message.ts:35-36 — cursor and order cannot be combined.
    if let (Some(_), Some(_)) = (&cursor_param, &order_param) {
        return Err(ApiError::invalid_cursor("Cursor cannot be combined with order"));
    }
    let cursor = match &cursor_param {
        Some(encoded) => {
            let decoded = decode_message_cursor(encoded)
                .ok_or_else(|| ApiError::invalid_cursor("Invalid cursor"))?;
            // v2 message.ts:40 — with a cursor the order comes from the
            // token (`decoded?.order ?? ctx.query.order ?? "desc"`).
            Some((
                decoded.id,
                direction_of(decoded.direction),
                match decoded.order {
                    ListOrderWire::Asc => ListOrder::Asc,
                    ListOrderWire::Desc => ListOrder::Desc,
                },
            ))
        }
        None => None,
    };
    let (cursor, order) = match cursor {
        Some((id, direction, order)) => (Some((id, direction)), order),
        None => (None, order),
    };

    let rows = services
        .store
        .messages(MessagesInput {
            session_id: session_id.clone(),
            limit: Some(limit),
            order: Some(order),
            type_,
            cursor,
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("message:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;

    let wire_order = match order {
        ListOrder::Asc => ListOrderWire::Asc,
        ListOrder::Desc => ListOrderWire::Desc,
    };
    // v2 message.ts:58-62 — previous/next come from the first/last row of
    // every non-empty page; the keys are OMITTED when absent.
    let previous = rows.first().map(|first| {
        encode_message_cursor(&super::common::MessageCursor {
            id: first.id.clone(),
            order: wire_order,
            direction: CursorDirection::Previous,
        })
    });
    let next = rows.last().map(|last| {
        encode_message_cursor(&super::common::MessageCursor {
            id: last.id.clone(),
            order: wire_order,
            direction: CursorDirection::Next,
        })
    });

    let data: Vec<Value> = rows.iter().map(message_row_to_info).collect();
    let mut cursor_json = serde_json::Map::new();
    if let Some(previous) = previous.flatten() {
        cursor_json.insert("previous".to_owned(), json!(previous));
    }
    if let Some(next) = next.flatten() {
        cursor_json.insert("next".to_owned(), json!(next));
    }
    Ok(axum::Json(json!({
        "data": data,
        "cursor": cursor_json,
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
