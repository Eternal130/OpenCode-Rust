//! Shell handlers (v2 `handlers/shell.ts`): list/create/get/output/remove
//! over the location's non-interactive shell service.

#![allow(clippy::must_use_candidate)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use oc_schema::shell::OutputInput;
use serde_json::{json, Value};

use super::common::{json_location_data, no_content, resolve_location, ApiError, SharedState};
use super::session::{query_field, services};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/shell", axum::routing::get(list))
        .route("/api/shell", axum::routing::post(create))
        .route("/api/shell/:id", axum::routing::delete(remove))
        .route("/api/shell/:id", axum::routing::get(get))
        .route("/api/shell/:id/output", axum::routing::get(output))
}

fn shell_json(info: &oc_schema::shell::Info) -> Value {
    serde_json::to_value(info).unwrap_or_else(|_| json!({}))
}

async fn list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let data: Vec<Value> = services.shell.list().iter().map(shell_json).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn create(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let input = oc_schema::shell::CreateInput {
        command: body["command"].as_str().unwrap_or_default().to_owned(),
        cwd: body["cwd"].as_str().map(String::from),
        timeout: body.get("timeout").and_then(Value::as_u64),
        metadata: body["metadata"].as_object().map(|map| map.clone().into_iter().collect()),
    };
    let info = services.shell.create(input, None, None, None).await.map_err(|error| {
        crate::errors::debug_cause("shell:unknown_ref", &error);
        ApiError::unknown_ref()
    })?;
    Ok(json_location_data(&location.as_path().to_string_lossy(), shell_json(&info)))
}

async fn get(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let info = services.shell.get(&oc_schema::id::ShellId(id.clone())).map_err(|error| {
        crate::errors::debug_cause("shell:shell_not_found", &error);
        ApiError::shell_not_found(&id)
    })?;
    let _ = query_field(&uri, "location");
    Ok(json_location_data(&location.as_path().to_string_lossy(), shell_json(&info)))
}

async fn output(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let input = OutputInput {
        cursor: query_field(&uri, "cursor").and_then(|value| value.parse::<u64>().ok()),
        limit: query_field(&uri, "limit").and_then(|value| value.parse::<u64>().ok()),
    };
    let output =
        services.shell.output(&oc_schema::id::ShellId(id.clone()), Some(input)).await.map_err(
            |error| {
                crate::errors::debug_cause("shell:shell_not_found", &error);
                ApiError::shell_not_found(&id)
            },
        )?;
    Ok(json_location_data(
        &location.as_path().to_string_lossy(),
        json!({
            "output": output.output,
            "cursor": output.cursor,
            "size": output.size,
            "truncated": output.truncated,
        }),
    ))
}

async fn remove(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    services.shell.remove(&oc_schema::id::ShellId(id.clone())).await.map_err(|error| {
        crate::errors::debug_cause("shell:shell_not_found", &error);
        ApiError::shell_not_found(&id)
    })?;
    Ok(no_content())
}
