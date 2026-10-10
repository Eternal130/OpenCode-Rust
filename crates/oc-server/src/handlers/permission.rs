//! Permission handlers: location-wide pending list, saved-grant list /
//! remove, and the session-scoped ask / get / reply family (deep-map §3).

#![allow(clippy::must_use_candidate)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use oc_schema::id::{PermissionId, SessionId};
use oc_schema::permission::Request;
use serde_json::{json, Value};

use super::common::{
    json_data, json_location_data, no_content, resolve_location, ApiError, SharedState,
};
use super::session::{query_field, services, session_services};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/permission/request", axum::routing::get(request_list))
        .route("/api/permission/saved", axum::routing::get(saved_list))
        .route("/api/permission/saved/:id", axum::routing::delete(saved_remove))
        .route("/api/session/:sessionID/permission", axum::routing::get(session_list))
        .route("/api/session/:sessionID/permission", axum::routing::post(session_create))
        .route("/api/session/:sessionID/permission/:requestID", axum::routing::get(session_get))
        .route(
            "/api/session/:sessionID/permission/:requestID/reply",
            axum::routing::post(session_reply),
        )
}

fn parse_session_id(raw: &str) -> SessionId {
    SessionId::parse(raw.to_string()).unwrap_or_else(|error| SessionId(error.value))
}

fn request_json(request: &Request) -> Value {
    serde_json::to_value(request).unwrap_or_else(|_| json!({}))
}

// ---------------------------------------------------------------------------
// GET /api/permission/request — location-wide pending
// ---------------------------------------------------------------------------

async fn request_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let data: Vec<Value> = services.permission.list().iter().map(request_json).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

// ---------------------------------------------------------------------------
// GET /api/permission/saved, DELETE /api/permission/saved/:id
// ---------------------------------------------------------------------------

async fn saved_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // The saved-grant family resolves the request location (v2 location
    // middleware: query → header → cwd) and goes through the location's
    // services — assembling the location when cold (SSE-visible boot).
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let project_id = query_field(&uri, "projectID");
    let rows = state
        .engine
        .db
        .transaction(move |tx| {
            let conn = tx.conn();
            let rows = if let Some(project) = project_id.as_deref() {
                let mut statement = conn.prepare(
                    "SELECT * FROM permission WHERE project_id = ?1 ORDER BY action, resource",
                )?;
                let rows = statement
                    .query_map((project,), |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, i64>(5)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                rows
            } else {
                let mut statement =
                    conn.prepare("SELECT * FROM permission ORDER BY action, resource")?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, i64>(5)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                rows
            };
            Ok(rows)
        })
        .await
        .map_err(|error| {
            crate::errors::debug_cause("permission:unknown_ref", &error);
            ApiError::unknown_ref()
        })?;
    let data: Vec<Value> = rows
        .into_iter()
        .map(|(id, project_id, action, resource, created, updated)| {
            json!({
                "id": id,
                "projectID": project_id,
                "action": action,
                "resource": resource,
                "time": { "created": created, "updated": updated },
            })
        })
        .collect();
    Ok(json_data(Value::Array(data)))
}

async fn saved_remove(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    // v2: the location middleware resolves the request location (cwd
    // default) and the delete runs through the location's permission
    // service — the cold boot of the server's own cwd location is pinned
    // by the permission_saved golden.
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    let db = state.engine.db.clone();
    let id = id.clone();
    db.transaction(move |tx| {
        tx.execute("DELETE FROM permission WHERE id = ?1", (id,))?;
        Ok(())
    })
    .await
    .map_err(|error| {
        crate::errors::debug_cause("permission:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// Session-scoped family
// ---------------------------------------------------------------------------

async fn session_list(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let parsed = parse_session_id(&session_id);
    let data: Vec<Value> =
        services.permission.for_session(&parsed).iter().map(request_json).collect();
    Ok(json_data(Value::Array(data)))
}

async fn session_create(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let parsed_body = AskBody::parse(&body);
    let input = oc_core::permission::AssertInput {
        id: parsed_body.id.map(PermissionId),
        session_id: parse_session_id(&session_id),
        agent: None,
        action: parsed_body.action,
        resources: parsed_body.resources,
        save: parsed_body.save,
        metadata: None,
        source: None,
    };
    let result = services.permission.ask(input).await.map_err(|error| {
        crate::errors::debug_cause("permission:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    Ok(json_data(json!({
        "id": result.id.as_str(),
        "effect": serde_json::to_value(result.effect).unwrap_or_else(|_| json!("ask")),
    })))
}

async fn session_get(
    State(state): State<SharedState>,
    Path((session_id, request_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    let request = owned_request(&services, &request_id)
        .ok_or_else(|| ApiError::permission_not_found(&request_id))?;
    if request.session_id.as_str() != session_id {
        return Err(ApiError::permission_not_found(&request_id));
    }
    Ok(json_data(request_json(&request)))
}

async fn session_reply(
    State(state): State<SharedState>,
    Path((session_id, request_id)): Path<(String, String)>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let (services, _) = session_services(&state, &session_id).await?;
    // Ownership: a request belonging to another session is a 404 (§3).
    let request = owned_request(&services, &request_id)
        .ok_or_else(|| ApiError::permission_not_found(&request_id))?;
    if request.session_id.as_str() != session_id {
        return Err(ApiError::permission_not_found(&request_id));
    }
    let reply = match body["decision"].as_str() {
        Some("once") => oc_schema::permission::Reply::Once,
        Some("always") => oc_schema::permission::Reply::Always,
        Some("reject") => oc_schema::permission::Reply::Reject,
        _ => return Err(ApiError::invalid_request("invalid decision")),
    };
    let input = oc_core::permission::ReplyInput {
        request_id: PermissionId(request_id.clone()),
        reply,
        message: body["message"].as_str().map(String::from),
    };
    services.permission.reply(input).await.map_err(|error| {
        crate::errors::debug_cause("permission:permission_not_found", &error);
        ApiError::permission_not_found(&request_id)
    })?;
    Ok(no_content())
}

fn owned_request(
    services: &std::sync::Arc<oc_core::engine::LocationServices>,
    request_id: &str,
) -> Option<Request> {
    services.permission.get(&PermissionId(request_id.to_owned()))
}

/// Request-body projection for `permission ask`.
struct AskBody {
    id: Option<String>,
    action: String,
    resources: Vec<String>,
    save: Option<Vec<String>>,
}

impl AskBody {
    fn parse(body: &Value) -> Self {
        Self {
            id: body["id"].as_str().map(String::from),
            action: body["action"].as_str().unwrap_or_default().to_owned(),
            resources: body["resources"]
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect())
                .unwrap_or_default(),
            save: body["save"]
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect()),
        }
    }
}
