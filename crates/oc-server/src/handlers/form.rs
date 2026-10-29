//! Form handlers: the global list plus the session-scoped create / get /
//! list / reply / cancel family. Ownership: a form whose `sessionID`
//! mismatches is a 404; `AlreadySettled` → 409; `InvalidAnswer` → 400
//! (deep-map §3).

#![allow(clippy::must_use_candidate)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use oc_core::forms::{CreateInput, FormError};
use oc_schema::form::{Answer, Fields};
use oc_schema::id::FormId;
use serde_json::{json, Value};

use super::common::{
    json_data, json_location_data, no_content, resolve_location, ApiError, SharedState,
};
use super::session::{query_field, services, session_services};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/form", axum::routing::get(list))
        .route("/api/session/:sessionID/form", axum::routing::get(session_list))
        .route("/api/session/:sessionID/form", axum::routing::post(session_create))
        .route("/api/session/:sessionID/form/:formID", axum::routing::delete(session_cancel))
        .route("/api/session/:sessionID/form/:formID", axum::routing::get(session_get))
        .route("/api/session/:sessionID/form/:formID/reply", axum::routing::post(session_reply))
}

/// The form-location resolution (ref middleware/form-location.ts:27-38):
/// every session-scoped form route runs through this instead of the plain
/// session lookup. `sessionID === "global"` is the temporary MCP
/// elicitation escape hatch — still location-scoped, but bypassing the
/// session row lookup because some MCP elicitations cannot be attributed
/// to a real session (they are stored under the sentinel
/// [`oc_core::mcp::GLOBAL_ELICITATION_SESSION_ID`]). The location comes
/// from the request itself (query/header ref), never a session row.
async fn form_services(
    state: &SharedState,
    session_id: &str,
    uri: &Uri,
    headers: &HeaderMap,
) -> Result<std::sync::Arc<oc_core::engine::LocationServices>, ApiError> {
    if session_id == oc_core::mcp::GLOBAL_ELICITATION_SESSION_ID {
        let location = resolve_location(state, uri.query().unwrap_or_default(), headers)?;
        return services(state, &location).await;
    }
    session_services(state, session_id).await.map(|(services, _)| services)
}

fn form_json(form: &oc_schema::form::Info) -> Value {
    serde_json::to_value(form).unwrap_or_else(|_| json!({}))
}

fn form_error(error: FormError) -> ApiError {
    match error {
        FormError::NotFound(error) => ApiError::form_not_found(error.0.as_str()),
        FormError::AlreadySettled(error) => ApiError::form_already_settled(error.0.as_str()),
        FormError::AlreadyExists(error) => ApiError::form_already_settled(error.0.as_str()),
        FormError::InvalidAnswer(error) => {
            ApiError::form_invalid_answer(error.id.as_str(), error.message)
        }
        FormError::InvalidForm(error) => ApiError::form_invalid(error.message),
    }
}

// ---------------------------------------------------------------------------
// GET /api/form — global list ({location, data})
// ---------------------------------------------------------------------------

async fn list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let data: Vec<Value> = services.forms.list(None).iter().map(form_json).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

// ---------------------------------------------------------------------------
// Session-scoped family
// ---------------------------------------------------------------------------

async fn session_list(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let services = form_services(&state, &session_id, &uri, &headers).await?;
    let data: Vec<Value> = services.forms.list(Some(&session_id)).iter().map(form_json).collect();
    Ok(json_data(Value::Array(data)))
}

async fn session_create(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let services = form_services(&state, &session_id, &uri, &headers).await?;
    // RA-01: post-location payload decode (404 first, then schema 400).
    let body = crate::request_validation::validate_at_handler(
        "session.form.create",
        &axum::http::Method::POST,
        &uri,
        &body,
    )?;
    let input = CreateInput {
        session_id: session_id.clone(),
        title: body["title"].as_str().unwrap_or_default().to_owned(),
        metadata: None,
        fields: parse_fields(&body["fields"])?,
    };
    let form = match body["id"].as_str() {
        Some(id) => services.forms.create_with_id(FormId(id.to_owned()), input).await,
        None => services.forms.create(input).await,
    };
    let form = form.map_err(form_error)?;
    Ok(json_data(form_json(&form)))
}

fn parse_fields(value: &Value) -> Result<Fields, ApiError> {
    serde_json::from_value(value.clone())
        .map_err(|error| ApiError::form_invalid(format!("invalid fields: {error}")))
}

fn parse_answer(value: &Value) -> Result<Answer, ApiError> {
    serde_json::from_value(value.clone())
        .map_err(|error| ApiError::form_invalid_answer("", format!("invalid answer: {error}")))
}

/// Resolves the form under the session's ownership (404 otherwise).
fn owned_form(
    services: &std::sync::Arc<oc_core::engine::LocationServices>,
    session_id: &str,
    form_id: &str,
) -> Result<oc_schema::form::Info, ApiError> {
    let info = services
        .forms
        .get(&FormId(form_id.to_owned()))
        .map_err(|error| form_error(FormError::NotFound(error)))?;
    if info.session_id != session_id {
        return Err(ApiError::form_not_found(form_id));
    }
    Ok(info)
}

async fn session_get(
    State(state): State<SharedState>,
    Path((session_id, form_id)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let services = form_services(&state, &session_id, &uri, &headers).await?;
    let info = owned_form(&services, &session_id, &form_id)?;
    let detail_state = services
        .forms
        .state(&FormId(form_id.clone()))
        .map_err(|error| form_error(FormError::NotFound(error)))?;
    let mut detail = form_json(&info);
    if let Value::Object(map) = &mut detail {
        map.insert(
            "state".to_owned(),
            serde_json::to_value(&detail_state).unwrap_or_else(|_| json!({ "status": "pending" })),
        );
    }
    Ok(json_data(detail))
}

async fn session_reply(
    State(state): State<SharedState>,
    Path((session_id, form_id)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, ApiError> {
    let services = form_services(&state, &session_id, &uri, &headers).await?;
    owned_form(&services, &session_id, &form_id)?;
    // RA-01: formLocation resolves the form (404) before payload decode —
    // validate Params (formID ^frm_) + payload only after it.
    let body = crate::request_validation::validate_at_handler(
        "session.form.reply",
        &axum::http::Method::POST,
        &uri,
        &body,
    )?;
    let answer: Answer = parse_answer(&body["answer"])?;
    services.forms.reply(&FormId(form_id.clone()), answer).await.map_err(form_error)?;
    Ok(no_content())
}

async fn session_cancel(
    State(state): State<SharedState>,
    Path((session_id, form_id)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let services = form_services(&state, &session_id, &uri, &headers).await?;
    owned_form(&services, &session_id, &form_id)?;
    // RA-01: form.cancel is form-location-first WITH a query row — the
    // session+form 404s resolve first, then the query decode.
    crate::request_validation::validate_at_handler(
        "session.form.cancel",
        &axum::http::Method::DELETE,
        &uri,
        &[],
    )?;
    let message = query_field(&uri, "message");
    services
        .forms
        .cancel(&FormId(form_id.clone()), message.as_deref())
        .await
        .map_err(form_error)?;
    Ok(no_content())
}

// F-4: the former `_touch(FormValue)` dead guard is REMOVED. Ref's
// form-location middleware (middleware/form-location.ts:27-38) never
// touches location activity — it only resolves location services (with
// the "global" elicitation escape hatch, mirrored in `form_services`
// above); the TTL/activity refresh is owned by the location map, not
// the form routes. Nothing here consumes `FormValue` directly (answers
// decode via `serde` into `Answer`), so the guard kept a phantom
// import alive.
