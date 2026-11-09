//! The remaining smaller namespaces: generate, migration, websearch,
//! plugin, rpc. The integration/auth handlers live in `integration.rs`
//! (BE-3 host).

#![allow(clippy::must_use_candidate, clippy::fn_params_excessive_bools)]
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use super::common::{json_location_data, no_content, resolve_location, ApiError, SharedState};
use super::session::services;

pub fn routes() -> axum::Router<SharedState> {
    axum::Router::new()
        // generate
        .route("/api/experimental/generate", axum::routing::post(generate_text))
        // integration handlers live in handlers/integration.rs (BE-3)
        // migration
        .route("/api/experimental/migration/v1", axum::routing::get(migration_status))
        // plugin
        .route("/api/plugin", axum::routing::get(plugin_list))
        .route("/api/plugin/check", axum::routing::post(plugin_check))
        .route("/api/plugin/update", axum::routing::post(plugin_update))
        .route("/api/plugin/install", axum::routing::post(plugin_install))
        // rpc
        .route("/api/rpc/:rpcID/:method", axum::routing::post(rpc_call))
        // websearch
        .route("/api/websearch", axum::routing::post(websearch_query))
        .route("/api/websearch/provider", axum::routing::get(websearch_providers))
}

// ---------------------------------------------------------------------------
// POST /api/experimental/generate
// ---------------------------------------------------------------------------

/// `Generate.UnavailableError` mapping (generate.ts:47-49).
fn generate_unavailable(error: &oc_ai::schema::AiError) -> ApiError {
    ApiError::service_unavailable(error.message().to_string(), Some("opencode"))
}

async fn generate_text(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    use futures::StreamExt as _;
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let Some(prompt) = body["prompt"].as_str() else {
        return Err(ApiError::invalid_request("expected string prompt"));
    };
    // One stateless generation over the base configuration's model
    // (generate.ts): resolve the optional reference or the configured
    // default, then stream one request with session-attribution headers
    // and no stored session.
    let synthetic = oc_core::database::schema::SessionRow::new(
        "generate",
        services.project.id.clone(),
        &services.project.directory.to_string_lossy(),
        "generate",
        "0",
    )
    .map_err(|error| {
        crate::errors::debug_cause("generate:text", &error);
        ApiError::service_unavailable("Text generation is not available", Some("opencode"))
    })?;
    let resolved = match &body["model"] {
        Value::Null => services.resolver.resolve(&synthetic).map_err(|error| match error {
            oc_core::session::runner::ModelError::NotSelected(_) => {
                ApiError::invalid_request("No model specified and no supported model is available")
            }
            other => ApiError::service_unavailable(other.to_string(), Some("opencode")),
        })?,
        reference => {
            let parsed: Result<oc_schema::model::Ref, _> =
                serde_json::from_value(reference.clone());
            match parsed {
                Ok(reference) => {
                    let provider = reference.provider_id.0.clone();
                    let model = reference.id.0.clone();
                    services.resolver.resolve_selected(&synthetic, &reference).map_err(|error| {
                        crate::errors::debug_cause("generate:text", &error);
                        ApiError::invalid_request(format!("Model unavailable: {provider}/{model}"))
                    })?
                }
                Err(_) => return Err(ApiError::invalid_request("expected string prompt")),
            }
        }
    };
    let mut request = oc_ai::schema::LLMRequest::new(resolved.model);
    request.messages = vec![oc_ai::schema::messages::Message::user(prompt)];
    // Gateways require session attribution even for a stateless call; no
    // session is stored (generate.ts:57-59).
    let mut http_headers = std::collections::BTreeMap::new();
    http_headers.insert(
        "x-opencode-session".to_string(),
        oc_schema::id::SessionId::new().as_str().to_string(),
    );
    request.http = Some(oc_ai::schema::HttpOptions {
        headers: Some(http_headers),
        ..oc_ai::schema::HttpOptions::default()
    });
    let stream_result = services.runner.llm().stream(request).await;
    let mut stream = stream_result.map_err(|error| generate_unavailable(&error))?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(|error| generate_unavailable(&error))? {
            oc_ai::schema::LLMEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
            oc_ai::schema::LLMEvent::ProviderError { message, .. } => {
                return Err(ApiError::service_unavailable(message, Some("opencode")));
            }
            _ => {}
        }
    }
    Ok(super::common::json_data(json!({ "text": text })))
}

// ---------------------------------------------------------------------------
// GET /api/experimental/migration/v1
// ---------------------------------------------------------------------------

async fn migration_status(State(state): State<SharedState>) -> Result<Response, ApiError> {
    // DB-D12 (server/handlers/migration.ts): the real V1Migration.status()
    // — required / running{progress} / error / completed, derived from the
    // legacy `session` table presence, the `migration.v1-v2` KV state and
    // the runner's in-process state (never a hardcoded "completed").
    let status = oc_core::database::v1_migration::status(&state.engine.db).await;
    Ok(axum::Json(status.to_json()).into_response())
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

async fn plugin_list(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // The booted plugin subsystem's inventory (wave-12): every resolved
    // plugin with its live status (active / failed / crash-disabled)
    // plus the resolution failures, in v2's `Plugin.Info` shape. The
    // subsystem is process-global (documented divergence from v2's
    // per-location activation); a plugin-less boot lists empty.
    let plugins = state.plugins.as_ref().map_or_else(Vec::new, |boot| boot.list_info());
    let data: Vec<Value> =
        plugins.iter().map(|info| serde_json::to_value(info).unwrap_or(Value::Null)).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn plugin_check(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // handlers/plugin.ts (plugin.check): the inventory's PACKAGE sources
    // are the check targets, optionally narrowed to one `target`; a named
    // target outside the inventory is a 400 naming it.
    let mut inventory: Vec<String> =
        state.plugins.iter().flat_map(|boot| boot.package_targets()).collect();
    inventory.sort();
    inventory.dedup();
    let targets: Vec<String> = match body["target"].as_str() {
        Some(target) if !target.is_empty() => {
            if !inventory.iter().any(|seen| seen == target) {
                return Err(crate::errors::invalid_request_message_field(
                    format!("Plugin package is not in the current server inventory: {target}"),
                    "target",
                ));
            }
            vec![target.to_owned()]
        }
        _ => inventory,
    };

    // The ref passes `{refresh: true}` (the 24 h throttle must not mask a
    // user-asked check); the probe runs the npm registry lookup, so it
    // parks on the blocking pool.
    let cache = oc_plugin_host::update_cache();
    let root = oc_plugin_host::install_root();
    let mut outdated: std::collections::HashSet<String> = std::collections::HashSet::new();
    for target in &targets {
        let result = tokio::task::spawn_blocking({
            let target = target.clone();
            let root = root.clone();
            move || cache.check(&target, &root, &oc_plugin_host::InstallOptions::default(), true)
        })
        .await;
        let flagged = matches!(result, Ok(true));
        if flagged {
            outdated.insert(target.clone());
        }
    }

    // The response is the inventory with fresh `outdated` flags applied
    // directly (the ref's `mutate(result.data)` — the caller must not
    // wait for a `plugin.updated` event).
    let mut plugins = state.plugins.as_ref().map_or_else(Vec::new, |boot| boot.list_info());
    for info in &mut plugins {
        if let oc_schema::misc::PluginSource::Package { target, outdated: flag, .. } =
            &mut info.source
        {
            if outdated.contains(target) {
                *flag = Some(true);
            }
        }
    }
    let data: Vec<Value> =
        plugins.iter().map(|info| serde_json::to_value(info).unwrap_or(Value::Null)).collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(data)))
}

async fn plugin_update(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // The generated request validation owns the `{targets: string[]}`
    // shape; a malformed body never reaches here.
    let targets: Vec<String> = body["targets"]
        .as_array()
        .map(|array| array.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();

    // handlers/plugin.ts (plugin.update): targets outside the current
    // inventory are a 400 naming every offender.
    let inventory: std::collections::HashSet<String> =
        state.plugins.iter().flat_map(|boot| boot.package_targets()).collect();
    let unknown: Vec<String> =
        targets.iter().filter(|target| !inventory.contains(*target)).cloned().collect();
    if !unknown.is_empty() {
        return Err(crate::errors::invalid_request_message_field(
            format!(
                "Plugin packages are not in the current server inventory: {}",
                unknown.join(", ")
            ),
            "targets",
        ));
    }

    // Every update runs to completion — one failure never interrupts
    // the rest; the failures roll up into one 503 (plugin.ts).
    let root = oc_plugin_host::install_root();
    let cache = oc_plugin_host::update_cache();
    let mut failures: Vec<String> = Vec::new();
    for target in &targets {
        let owned = target.clone();
        let result = tokio::task::spawn_blocking({
            let root = root.clone();
            move || {
                oc_plugin_host::update(
                    &owned,
                    &root,
                    cache,
                    &oc_plugin_host::InstallOptions::default(),
                )
            }
        })
        .await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => failures.push(format!("{target}: {error}")),
            Err(error) => failures.push(format!("{target}: update task failed: {error}")),
        }
    }
    if !failures.is_empty() {
        return Err(ApiError::service_unavailable(
            format!("Failed to update plugin packages: {}", failures.join("; ")),
            Some("plugin"),
        ));
    }

    // The ref notifies active locations to reload the updated plugins
    // and publishes `plugin.updated` (plugin.ts:121/175) — the reload
    // is contained (best-effort, logged; the host facade blocks on its
    // own runtime, so it runs off the async workers), the event is the
    // typed public `PluginEvent::Updated` registered on the bus (RF-02).
    if let Some(boot) = state.plugins.clone() {
        let strategies = std::sync::Arc::clone(&state.worktree_strategies);
        let _ = tokio::task::spawn_blocking(move || {
            let reloaded = crate::plugins::reload_after_update(&boot);
            // G-24 residual — re-run the worktree-strategy registry
            // seeding after the update (mirror of boot.rs's
            // `plugin_worktree_registry`): the updated plugin's guest
            // worktree strategies re-register instead of the registry
            // holding the dead generations' entries.
            if reloaded {
                *strategies.write() =
                    crate::plugins::plugin_worktree_registry(&boot.host, &boot.resolution);
            }
        })
        .await;
    }
    if let Some(definition) =
        oc_core::bus::registry::by_type(oc_core::bus::registry::PLUGIN_UPDATED)
    {
        let options = oc_core::bus::PublishOptions { global: true, ..Default::default() };
        let _ = state.engine.bus.publish(definition, serde_json::json!({}), options).await;
    }
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /api/plugin/install — G-29 (the TUI plugin manager's install flow)
// ---------------------------------------------------------------------------

/// The global config candidates (`opencode plugin add`'s resolution order
/// — the user-scope configuration file the installer appends to).
fn global_config_path() -> std::path::PathBuf {
    let directory = oc_util::global_roots::resolve().config;
    let candidates = [
        directory.join("opencode.json"),
        directory.join("opencode.jsonc"),
        directory.join(".opencode").join("opencode.json"),
        directory.join(".opencode").join("opencode.jsonc"),
    ];
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .unwrap_or_else(|| candidates[0].clone())
}

/// Append `target` to the global config's `plugins[]` (the CLI `plugin
/// add` server arm): comment-preserving edit via the jsonc editor, a
/// parse failure or an unwritable file rolls into the caller's error.
fn append_global_plugin(target: &str) -> Result<(), String> {
    let path = global_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let existing = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_owned());
    let parsed = oc_core::cli_config::jsonc::parse_jsonc(&existing)
        .map_err(|error| format!("{} is not parseable: {error}", path.display()))?;
    let mut plugins: Vec<Value> =
        parsed.get("plugins").and_then(Value::as_array).cloned().unwrap_or_default();
    if plugins.iter().any(|entry| entry.as_str() == Some(target)) {
        return Ok(()); // already configured — idempotent
    }
    plugins.push(Value::String(target.to_owned()));
    let merged = serde_json::to_value(plugins).expect("plugins array serializes");
    match oc_core::config::jsonc_edit::set_path(&existing, &["plugins"], Some(&merged)) {
        Ok(Some(mut updated)) => {
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            std::fs::write(&path, updated).map_err(|error| error.to_string())
        }
        Ok(None) => Ok(()),
        Err(_) => Err(format!(
            "{} does not contain a JSON object at the root, so it cannot be edited in place",
            path.display()
        )),
    }
}

/// The `plugin add` target guard (the CLI's `add`): directive-ish specs
/// (`*`, `name.*`, `-flag`, the reserved `opencode.*` namespace) are not
/// installable package targets.
fn installable_target(target: &str) -> bool {
    !(target.starts_with('-')
        || target == "*"
        || target.ends_with(".*")
        || target.starts_with("opencode."))
}

async fn plugin_install(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let _services = services(&state, &location).await?;
    // The generated request validation owns the `{target: string}` shape.
    let target = body["target"].as_str().unwrap_or_default().trim().to_owned();
    if !installable_target(&target) {
        return Err(crate::errors::invalid_request_message_field(
            "Plugin target must be an npm registry package or Git package specifier".to_owned(),
            "target",
        ));
    }

    // The pinned install (the CLI `plugin add` path — frozen when a
    // lockfile exists, reuse-first): npm/bun spawns park on the
    // blocking pool.
    let install_target = target.clone();
    let install = tokio::task::spawn_blocking(move || {
        oc_plugin_host::install(&install_target, &oc_plugin_host::InstallOptions::default())
    })
    .await
    .map_err(|error| {
        ApiError::service_unavailable(
            format!("Failed to install plugin package: {target}: {error}"),
            Some("plugin"),
        )
    })?;
    if let Err(error) = install {
        return Err(ApiError::service_unavailable(
            format!("Failed to install plugin package: {target}: {error}"),
            Some("plugin"),
        ));
    }

    // Persist into the user scope (`opencode plugin add`'s config leg);
    // a config write failure fails the request (the package is cached,
    // but the plugin would silently vanish on the next boot).
    if let Err(error) = append_global_plugin(&target) {
        return Err(ApiError::service_unavailable(
            format!("Failed to add plugin to the global configuration: {error}"),
            Some("plugin"),
        ));
    }

    // Activate in the running server: load the new entry's generation
    // into the live host (G-05's add leg) and swap the resolution cell
    // so `/api/plugin` lists it immediately; contained like every
    // reload — a failure logs and keeps the previous state.
    if let Some(boot) = state.plugins.clone() {
        let boot_target = target.clone();
        let _ = tokio::task::spawn_blocking(move || {
            crate::plugins::activate_installed_plugin(&boot, &boot_target);
        })
        .await;
    }
    if let Some(definition) =
        oc_core::bus::registry::by_type(oc_core::bus::registry::PLUGIN_UPDATED)
    {
        let options = oc_core::bus::PublishOptions { global: true, ..Default::default() };
        let _ = state.engine.bus.publish(definition, serde_json::json!({}), options).await;
    }
    Ok(no_content())
}

// ---------------------------------------------------------------------------
// POST /api/rpc/:rpcID/:method — {input} → {output}
// ---------------------------------------------------------------------------

/// `/api/rpc` dispatch (PLUG-D14): route the call into the booted
/// plugin host's guest (`Rpc.Service.call` parity — handlers/rpc.ts):
/// unknown ids/methods map to `RpcError` 400 with the v2 failure type;
/// handler defects and the invalid-output/internal types map to
/// `RpcInternalError` 500; a successful output returns `{output}`
/// (omitted when `undefined`, per the v2 handler).
async fn rpc_call(
    State(state): State<SharedState>,
    Path((rpc_id, method)): Path<(String, String)>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let input = body.get("input").cloned().unwrap_or(Value::Null);
    let Some(boot) = state.plugins.clone() else {
        // No plugin subsystem at all = an empty registry: v2's
        // `rpc.unavailable` (RpcError 400), never a 500 stub.
        return Err(ApiError::rpc(format!("RPC is unavailable: {rpc_id}"), "rpc.unavailable"));
    };
    match boot.rpc_call(&rpc_id, &method, input).await {
        // `output === undefined ? {} : { output }` (rpc.ts:10-11).
        Ok(output) => Ok(axum::Json(match output {
            Some(output) => json!({ "output": output }),
            None => json!({}),
        })
        .into_response()),
        Err(crate::plugins::RpcCallError::Failure { type_, message, .. }) => {
            Err(ApiError::rpc(message, type_))
        }
        Err(crate::plugins::RpcCallError::Internal(message)) => {
            Err(ApiError::rpc_internal(message, "rpc.internal"))
        }
    }
}

// ---------------------------------------------------------------------------
// Websearch
// ---------------------------------------------------------------------------

async fn websearch_query(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
    axum::Json(body): axum::Json<Value>,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    let payload: oc_schema::misc::WebSearchInput =
        serde_json::from_value(body).map_err(|error| {
            ApiError::invalid_request(format!("Invalid websearch payload: {error}"))
        })?;
    // v2 handlers/websearch.ts — the location's WebSearch service with the
    // tagged-error mapping (ProviderRequired/ProviderNotFound/Disabled →
    // 400 kinds; Request → 503 `service`).
    match services
        .websearch
        .query(&payload.query, None, payload.provider_id.as_ref().map(|id| id.0.as_str()))
        .await
    {
        Ok(response) => Ok(json_location_data(
            &location.as_path().to_string_lossy(),
            serde_json::to_value(&response).unwrap_or(Value::Null),
        )),
        Err(error) => Err(match error {
            oc_core::tool::websearch::WebSearchError::ProviderRequired => {
                crate::errors::invalid_request_field(
                    "Web search provider is required",
                    "websearch_provider_required",
                    "providerID",
                )
            }
            oc_core::tool::websearch::WebSearchError::ProviderNotFound(provider_id) => {
                crate::errors::invalid_request_field(
                    format!("Web search provider not found: {provider_id}"),
                    "websearch_provider_not_found",
                    "providerID",
                )
            }
            oc_core::tool::websearch::WebSearchError::Disabled => {
                crate::errors::invalid_request_kind("Web search is disabled", "websearch_disabled")
            }
            oc_core::tool::websearch::WebSearchError::Request { provider_id, .. } => {
                crate::errors::service_unavailable_named(
                    format!("Web search request failed: {provider_id}"),
                    provider_id,
                )
            }
        }),
    }
}

async fn websearch_providers(
    State(state): State<SharedState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let location = resolve_location(&state, uri.query().unwrap_or_default(), &headers)?;
    let services = services(&state, &location).await?;
    // v2 `websearch.providers`: id/name pairs sorted by display name.
    let providers: Vec<Value> = services
        .websearch
        .providers()
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    Ok(json_location_data(&location.as_path().to_string_lossy(), Value::Array(providers)))
}
