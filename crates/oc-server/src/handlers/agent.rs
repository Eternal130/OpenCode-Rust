//! Agent / skill / command / reference handlers — read-only catalog
//! surfaces over the location's assembled tables.

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use serde_json::{Value, json};

use super::common::{ApiError, SharedState, json_location_data, resolve_location};
use super::session::services;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        .route("/api/agent", axum::routing::get(list))
        .route("/api/agent/:agentID", axum::routing::get(get))
        .route("/api/command", axum::routing::get(command_list))
        .route("/api/skill", axum::routing::get(skill_list))
        .route("/api/reference", axum::routing::get(reference_list))
}

async fn list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    // G-270① — ref `Agent.list` (core agent.ts:123-129): EVERY agent
    // (hidden ones included) with the selected default FIRST — the
    // registry's `list()` already carries both; the old `!hidden` filter
    // here hid hidden agents (title/summary/config-defined) from the
    // wire. The default-first order rides `list()` unchanged.
    let agents = services.graph.agents.list();
    let data: Vec<Value> = agents
        .iter()
        .map(|agent| serde_json::to_value(agent).unwrap_or_else(|_| json!({})))
        .collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn get(
    State(state): State<SharedState>,
    Path(agent_id): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let agent = services
        .graph
        .agents
        .get(&oc_schema::id::AgentId(agent_id.clone()))
        .ok_or_else(|| ApiError::agent_not_found(&agent_id))?;
    let data = serde_json::to_value(&agent).unwrap_or_else(|_| json!({}));
    Ok(json_location_data(&location.as_path().to_string_lossy(), data))
}

async fn command_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    // CD-12/CD-34: the v2 `command.list` surface — config `commands`
    // maps, `{command,commands}/**/*.md` discovery, and plugin
    // `command.add` registrations, all merged in the location's command
    // table (`{name, description?}` per `Command.Info`).
    let data: Vec<Value> = services
        .commands
        .list()
        .into_iter()
        .map(|command| {
            let mut entry = json!({ "name": command.name });
            if let Some(description) = &command.description {
                entry["description"] = json!(description);
            }
            entry
        })
        .collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn skill_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let data: Vec<Value> = services
        .skills
        .list()
        .iter()
        .map(|skill| {
            json!({
                "id": skill.id.as_str(),
                "name": skill.name.0,
                "description": skill.description,
                "autoinvoke": skill.auto_invoke,
                "path": skill.path.as_str(),
                "content": skill.content,
            })
        })
        .collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn reference_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    // G-270② — the handler reads the location's assembled reference
    // service (ref handlers/reference.ts:7 pipes `Reference.Service.list`
    // straight out). The old hand-rolled config re-assembly here drifted
    // from the core semantics (ReferenceService::list — validation +
    // cache_path + invalid-entry dropping, pinned by oc-core
    // reference::tests): git sources kept their URL as `path`, invalid
    // entries survived, and description/hidden were always-null.
    let data: Vec<Value> = services
        .references
        .list()
        .iter()
        .map(|info| {
            // Wire shape = the protocol's PublicInfo (groups/reference.ts):
            // {name, path, description?, hidden?, source} where the PUBLIC
            // source omits description/hidden.
            let source = match &info.source {
                oc_core::reference::Source::Local(local) => json!({
                    "type": "local",
                    "path": local.path.to_string_lossy(),
                }),
                oc_core::reference::Source::Git(git) => {
                    let mut value = json!({
                        "type": "git",
                        "repository": git.repository,
                    });
                    if let Some(branch) = &git.branch {
                        value["branch"] = json!(branch);
                    }
                    value
                }
            };
            let mut value = json!({
                "name": info.name,
                "path": info.path.to_string_lossy(),
                "source": source,
            });
            if let Some(description) = &info.description {
                value["description"] = json!(description);
            }
            if let Some(hidden) = info.hidden {
                value["hidden"] = json!(hidden);
            }
            value
        })
        .collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}
