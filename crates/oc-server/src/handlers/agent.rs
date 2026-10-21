//! Agent / skill / command / reference handlers — read-only catalog
//! surfaces over the location's assembled tables.

#![allow(clippy::must_use_candidate, clippy::similar_names)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use serde_json::{json, Value};

use super::common::{json_location_data, resolve_location, ApiError, SharedState};
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
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let agents = services.graph.agents.list();
    let data: Vec<Value> = agents
        .iter()
        .filter(|agent| !agent.hidden)
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
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
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
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
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
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
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
    let location = resolve_location(uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // References are config-sourced (`Config.reference`); load the same
    // snapshot the engine assembled from.
    let mut loader = oc_core::config::Loader::new(
        location.as_path().to_path_buf(),
        oc_core::config::GlobalDirs {
            config: state.engine.home.join(".config").join("opencode"),
            home: state.engine.home.clone(),
        },
    );
    // CLI-N4 — the project-config disable switch (v2 Config.configured).
    crate::server_process_env::configure_loader(&mut loader);
    let snapshot = loader.load();
    let mut references: Vec<Value> = Vec::new();
    for entry in &snapshot.entries {
        let info = match entry {
            oc_schema::config::Entry::Document(document) => &document.info,
            oc_schema::config::Entry::Directory(_) => continue,
        };
        let Some(map) = info.references.as_ref() else { continue };
        for (name, entry) in map {
            let (source, path, description, hidden) = match entry {
                oc_schema::config::reference::Entry::Path(path) => {
                    (json!({ "type": "local", "path": path }), path.clone(), None, None)
                }
                oc_schema::config::reference::Entry::Git(git) => (
                    json!({
                        "type": "git",
                        "repository": git.repository,
                        "branch": git.branch,
                    }),
                    git.repository.clone(),
                    git.description.clone(),
                    git.hidden,
                ),
                oc_schema::config::reference::Entry::Local(local) => (
                    json!({ "type": "local", "path": local.path }),
                    local.path.clone(),
                    local.description.clone(),
                    local.hidden,
                ),
            };
            references.push(json!({
                "name": name,
                "path": path,
                "description": description,
                "hidden": hidden,
                "source": source,
            }));
        }
    }
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(references)))
}
