//! MCP handlers: list / add / remove / connect / disconnect + the resource
//! catalog (deep-map §3 "mcp list/add/remove/connect/disconnect").

#![allow(clippy::must_use_candidate)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use serde_json::{json, Value};

use super::common::{json_location_data, no_content, resolve_location, ApiError, SharedState};
use super::session::{query_field, services};

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/experimental/mcp/:server", axum::routing::delete(remove))
        .route("/api/experimental/mcp/:server", axum::routing::put(add))
        .route("/api/experimental/mcp/:server/connect", axum::routing::post(connect))
        .route("/api/experimental/mcp/:server/disconnect", axum::routing::post(disconnect))
        .route("/api/mcp", axum::routing::get(list))
        .route("/api/mcp/resource", axum::routing::get(resource_catalog))
}

fn status_json(status: &oc_schema::mcp::Status) -> Value {
    serde_json::to_value(status).unwrap_or_else(|_| json!({ "status": "pending" }))
}

async fn list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let data: Vec<Value> = services
        .mcp
        .servers()
        .iter()
        .map(|server| {
            json!({
                "name": server.name,
                "status": status_json(&server.status),
                "integrationID": server.integration_id.as_ref().map(|id| id.0.clone()),
            })
        })
        .collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn add(
    State(state): State<SharedState>,
    Path(server): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let config: oc_schema::mcp::ServerConfig = serde_json::from_value(body["config"].clone())
        .map_err(|error| ApiError::invalid_request(format!("invalid mcp config: {error}")))?;
    services.mcp.add(&server, config).await;
    Ok(no_content())
}

async fn remove(
    State(state): State<SharedState>,
    Path(server): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    services.mcp.remove(&server).await.map_err(|error| {
        crate::errors::debug_cause("mcp:mcp_server_not_found", &error);
        ApiError::mcp_server_not_found(&server)
    })?;
    Ok(no_content())
}

async fn connect(
    State(state): State<SharedState>,
    Path(server): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    services.mcp.connect(&server).await.map_err(|error| {
        crate::errors::debug_cause("mcp:mcp_server_not_found", &error);
        ApiError::mcp_server_not_found(&server)
    })?;
    Ok(no_content())
}

async fn disconnect(
    State(state): State<SharedState>,
    Path(server): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    services.mcp.disconnect(&server).await.map_err(|error| {
        crate::errors::debug_cause("mcp:mcp_server_not_found", &error);
        ApiError::mcp_server_not_found(&server)
    })?;
    Ok(no_content())
}

async fn resource_catalog(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let catalog = services.mcp.resource_catalog().await;
    let resources: Vec<Value> = catalog
        .resources
        .iter()
        .map(|resource| {
            json!({
                "server": resource.server,
                "name": resource.name,
                "uri": resource.uri,
                "description": resource.description,
                "mimeType": resource.mime_type,
            })
        })
        .collect();
    let templates: Vec<Value> = catalog
        .templates
        .iter()
        .map(|template| {
            json!({
                "server": template.server,
                "name": template.name,
                "uri": template.uri_template,
                "description": template.description,
                "mimeType": template.mime_type,
            })
        })
        .collect();
    let _ = query_field(&uri, "location");
    Ok(json_location_data(
        &location.as_path().to_string_lossy(),
        json!({ "resources": resources, "templates": templates }),
    ))
}
