//! Port of `packages/core/src/tool/plugin/opencode.ts` — the `opencode`
//! namespace: `session_rename`, `session_move`, `models` (§4.12).

use std::sync::Arc;

use crate::tool::{BuiltinTool, ToolExecuteWithContext, ToolFailure, ToolOutcome, ToolServices};

/// The worktree guidance system part pushed on every context build.
pub const WORKTREE_GUIDANCE: &str = "When you create a worktree outside the current working directory and intend to use it as your primary working directory, consider using `execute` to call `tools.opencode.session_move` and make the worktree the session's working directory.";

/// G-266③ — ref tool/plugin/opencode.ts:71-81: the guidance rides one
/// hook registered on `session.context`, `session.generate` AND
/// `session.compaction` — every request surface sees the
/// `session_move` migration hint.
pub fn register_worktree_guidance(hooks: &crate::hooks::HookRegistry) {
    let guidance = crate::hooks::fn_hook(move |event: &mut crate::hooks::SessionContextEvent| {
        event.system.push(oc_ai::schema::messages::SystemPart::new(WORKTREE_GUIDANCE));
        Ok(())
    });
    let guidance_compaction =
        crate::hooks::fn_hook(move |event: &mut crate::hooks::SessionCompactionEvent| {
            event
                .context
                .system
                .push(oc_ai::schema::messages::SystemPart::new(WORKTREE_GUIDANCE));
            Ok(())
        });
    // Registrations live for the location lifetime (dropped with the
    // registry when the bundle dies) — the shell description-refresh
    // hooks use the same pattern.
    std::mem::forget(futures::executor::block_on(hooks.register_session_context(
        guidance.clone(),
        crate::hooks::HookOptions::default(),
    )));
    std::mem::forget(futures::executor::block_on(
        hooks.register_session_generate(guidance, crate::hooks::HookOptions::default()),
    ));
    std::mem::forget(futures::executor::block_on(hooks.register_session_compaction(
        guidance_compaction,
        crate::hooks::HookOptions::default(),
    )));
}

/// `session_rename` — rename via the session store + `session.renamed`.
pub struct SessionRenameExecute {
    pub store: crate::session::store::SessionStore,
    pub bus: crate::bus::Bus,
}

#[derive(serde::Deserialize)]
struct RenameInput {
    #[serde(rename = "sessionID")]
    session_id: Option<String>,
    title: String,
}

impl ToolExecuteWithContext for SessionRenameExecute {
    fn execute(
        &self,
        _services: &ToolServices,
        input: &serde_json::Value,
        context: &crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + '_>,
    > {
        let input = input.clone();
        let context = context.clone();
        Box::pin(async move {
            let input: RenameInput = serde_json::from_value(input)
                .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            let session_id = input.session_id.unwrap_or_else(|| context.session_id.to_string());
            let title = input.title.trim().to_string();
            if title.is_empty() {
                return Err(ToolFailure::message("Session title must not be empty"));
            }
            let Some(session) = self.store.get(&session_id).await.map_err(|error| ToolFailure {
                message: format!("Unable to rename session {session_id}"),
                cause: Some(super::FailureCause::Nested(error.to_string())),
            })?
            else {
                return Err(ToolFailure {
                    message: format!("Unable to rename session {session_id}"),
                    cause: Some(super::FailureCause::Nested(format!(
                        "session not found: {session_id}"
                    ))),
                });
            };
            let options = crate::bus::PublishOptions {
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath(session.directory.clone()),
                    workspace_id: session.workspace_id.clone().map(|value| {
                        oc_schema::id::WorkspaceId::parse(value)
                            .unwrap_or_else(|error| oc_schema::id::WorkspaceId(error.value))
                    }),
                }),
                ..Default::default()
            };
            let def = crate::bus::registry::by_type_latest("session.renamed")
                .unwrap_or_else(|| panic!("no session.renamed definition"));
            self.bus
                .publish(
                    def,
                    serde_json::json!({ "sessionID": session_id, "title": title }),
                    options,
                )
                .await
                .map_err(|error| ToolFailure {
                    message: format!("Unable to rename session {session_id}"),
                    cause: Some(super::FailureCause::Nested(error.to_string())),
                })?;
            Ok(ToolOutcome {
                content: vec![oc_schema::tool::Content::Text {
                    text: format!("Renamed session {session_id} to {title}."),
                }],
                metadata: None,
                output: Some(serde_json::json!({ "sessionID": session_id, "title": title })),
            })
        })
    }
}

/// `session_move` dependencies (`ctx.session.move` — session/move.ts):
/// the session store + inbox, the execution wake seam, project
/// resolution for the destination, and the home dir for `~` expansion.
pub struct SessionMoveServices {
    pub store: crate::session::store::SessionStore,
    pub inbox: crate::session::inbox::SessionInbox,
    pub execution: std::sync::Arc<dyn crate::subagent::ChildExecution>,
    pub projects: crate::project::Resolver,
    pub home: std::path::PathBuf,
    pub ids: std::sync::Arc<dyn oc_util::determinism::IdGen>,
}

/// `session_move` — admit a `move` inbox item for the target session and
/// wake it (session/move.ts: the runner delivers the move at the next
/// safe boundary — `session.inbox.delivered` + `session.moved`).
pub struct SessionMoveExecute {
    pub deps: SessionMoveServices,
}

#[derive(serde::Deserialize)]
struct MoveInput {
    #[serde(rename = "sessionID")]
    session_id: Option<String>,
    directory: String,
}

/// `resolveDestination`'s `~` expansion + lexical resolve against the
/// session's directory (move.ts:59-71 — `path.resolve`, no symlink
/// resolution).
fn resolve_destination(
    home: &std::path::Path,
    session_dir: &str,
    directory: &str,
) -> std::path::PathBuf {
    let value = directory.trim();
    let expanded = if value == "~" {
        home.to_path_buf()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        std::path::PathBuf::from(value)
    };
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        std::path::Path::new(session_dir).join(expanded)
    };
    // Lexical `..`/`.` cleanup (path.resolve semantics).
    let mut normalized = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

impl ToolExecuteWithContext for SessionMoveExecute {
    fn execute(
        &self,
        _services: &ToolServices,
        input: &serde_json::Value,
        context: &crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + '_>,
    > {
        let input = input.clone();
        let context = context.clone();
        let deps = &self.deps;
        Box::pin(async move {
            let input: MoveInput = serde_json::from_value(input)
                .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            let blanket = |cause: String| ToolFailure {
                message: format!("Unable to move session to {}", input.directory),
                cause: Some(super::FailureCause::Nested(cause)),
            };
            let session_id =
                input.session_id.clone().unwrap_or_else(|| context.session_id.to_string());
            let Some(session) =
                deps.store.get(&session_id).await.map_err(|error| blanket(error.to_string()))?
            else {
                return Err(blanket(format!("session not found: {session_id}")));
            };

            // Destination: expand, resolve against the session's
            // directory, and require an existing directory.
            let destination = resolve_destination(&deps.home, &session.directory, &input.directory);
            let directory = destination.to_string_lossy().to_string();
            match std::fs::metadata(&destination) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(blanky(&input.directory, "Not a directory")),
                Err(_) => {
                    return Err(blanky(&input.directory, "Directory does not exist"));
                }
            }

            // `projects.resolve(directory)` → MovePayload
            // (location/projectID/subpath — `path.relative(project.directory,
            // directory)`).
            let resolved = deps.projects.resolve(&destination).await;
            let subpath = destination
                .strip_prefix(&resolved.directory)
                .ok()
                .map(|rest| rest.to_string_lossy().to_string())
                .filter(|rest| !rest.is_empty());
            let payload = oc_schema::session_inbox::MovePayload {
                location: oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath(directory.clone()),
                    workspace_id: None,
                },
                project_id: oc_schema::id::ProjectId::parse(resolved.id),
                subpath: subpath.map(oc_schema::common::RelativePath),
            };

            // Admit the queued move (delivery "steer" — the tool always
            // steers, opencode.ts:120) and wake the session.
            let message_id = deps.ids.new_id("msg");
            deps.inbox
                .admit(
                    &message_id,
                    &session_id,
                    oc_schema::session_inbox::Item::Move_ {
                        payload,
                        delivery: oc_schema::session_inbox::Delivery::Steer,
                    },
                )
                .await
                .map_err(|error| blanket(error.to_string()))?;
            deps.execution.wake(&session_id);

            Ok(ToolOutcome {
                content: vec![oc_schema::tool::Content::Text {
                    text: format!("Moved session {session_id} to {}.", input.directory),
                }],
                metadata: None,
                output: Some(
                    serde_json::json!({ "sessionID": session_id, "directory": input.directory }),
                ),
            })
        })
    }
}

/// Blanket failure helper with the exact nested cause text.
fn blanky(directory: &str, cause: &str) -> ToolFailure {
    ToolFailure {
        message: format!("Unable to move session to {directory}"),
        cause: Some(super::FailureCause::Nested(cause.to_string())),
    }
}

/// `models` — catalog query with own-provider-first sort, family dedup and
/// paging (page 20 / limit 100).
pub struct ModelsExecute {
    pub catalog: crate::provider_catalog::catalog::Catalog,
    /// `session.model.providerID` of the current session, when pinned
    /// (tests/embedders). G-86: production wiring resolves this at
    /// EXECUTION time from the invoking session's row via `store`
    /// (ref opencode.ts:134-142 `ctx.session.get({sessionID}).model?.
    /// providerID`).
    pub own_provider: parking_lot::Mutex<Option<String>>,
    /// The session store backing the G-86 own-provider resolution;
    /// `None` (tests) leaves `own_provider` as-is.
    pub store: Option<crate::session::store::SessionStore>,
}

fn status_str(status: &oc_schema::model::Status) -> String {
    match status {
        oc_schema::model::Status::Alpha => "alpha".into(),
        oc_schema::model::Status::Beta => "beta".into(),
        oc_schema::model::Status::Deprecated => "deprecated".into(),
        oc_schema::model::Status::Active => "active".into(),
    }
}

/// One page entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelEntry {
    /// `providerID/modelID`.
    pub id: String,
    pub name: String,
    /// Release date in milliseconds, or 0 when unknown.
    pub released: u64,
    pub variants: Vec<String>,
    /// Pricing in USD per million tokens.
    pub cost: serde_json::Value,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderPage {
    pub id: String,
    pub name: String,
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelsOutput {
    /// Matching models grouped by provider; the caller's own provider first.
    pub providers: Vec<ProviderPage>,
    pub total: usize,
    pub next: Option<usize>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ModelsQuery {
    pub query: Option<String>,
    pub provider: Option<String>,
    pub all: Option<bool>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl ModelsExecute {
    /// G-86 — `own` from the invoking session's row: the stored
    /// `session.model` reference's `providerID` (opencode.ts:134-142).
    /// A missing session or model — or a store read failure — degrades
    /// to `None` (no own-first boost), matching ref's optional chain.
    async fn session_own_provider(&self, session_id: &str) -> Option<String> {
        let store = self.store.as_ref()?;
        let session = store.get(session_id).await.ok()??;
        let raw = session.model.as_deref()?;
        let reference: oc_schema::model::Ref = serde_json::from_str(raw).ok()?;
        Some(reference.provider_id.0.clone())
    }

    /// The model listing over the catalog (models tool core).
    pub fn list(&self, input: &ModelsQuery, own: Option<&str>) -> ModelsOutput {
        let offset = input.offset.unwrap_or(0);
        let limit = input.limit.unwrap_or(20).clamp(1, 100);
        let terms: Vec<String> = input
            .query
            .as_ref()
            .map(|query| query.to_lowercase().split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let provider_filter = input.provider.as_ref().map(|value| value.to_lowercase());

        let mut providers: Vec<(String, String)> = self
            .catalog
            .providers()
            .iter()
            .map(|snapshot| (snapshot.info.id.0.clone(), snapshot.info.name.clone()))
            .collect();
        providers.sort_by(|a, b| a.1.cmp(&b.1));

        #[derive(Clone)]
        struct Row {
            provider_id: String,
            provider_name: String,
            model_id: String,
            model_name: String,
            released: u64,
            family: Option<String>,
            variants: Vec<String>,
            cost: serde_json::Value,
            status: String,
        }
        let mut rows: Vec<Row> = Vec::new();
        for snapshot in self.catalog.providers() {
            let provider_id = snapshot.info.id.0.clone();
            let provider_name = snapshot.info.name.clone();
            if let Some(filter) = &provider_filter {
                let id_match = provider_id.to_lowercase() == *filter;
                let name_match = provider_name.to_lowercase() == *filter;
                if !id_match && !name_match {
                    continue;
                }
            }
            for model in &snapshot.models {
                let text = format!("{}/{} {}", provider_id, model.id.0, model.name).to_lowercase();
                if !terms.iter().all(|term| text.contains(term)) {
                    continue;
                }
                rows.push(Row {
                    provider_id: provider_id.clone(),
                    provider_name: provider_name.clone(),
                    model_id: model.id.0.clone(),
                    model_name: model.name.clone(),
                    released: model.time.released.0 as u64,
                    family: model.family.clone(),
                    variants: model.variants.iter().map(|variant| variant.id.0.clone()).collect(),
                    cost: serde_json::json!(model.cost),
                    status: status_str(&model.status),
                });
            }
        }
        rows.sort_by(|left, right| {
            let own = |provider: &str| usize::from(own == Some(provider));
            own(&right.provider_id)
                .cmp(&own(&left.provider_id))
                // v2 compares `left.providerID.localeCompare(right.providerID)`
                // (tool/plugin/opencode.ts:159-165) — ICU primary is
                // case-insensitive with lowercase-first tertiary ties; the
                // repo's ASCII approximation (session/runner/context.rs)
                // is case-fold then lowercase-first tie-break.
                .then_with(|| {
                    left.provider_id
                        .to_lowercase()
                        .cmp(&right.provider_id.to_lowercase())
                        .then_with(|| right.provider_id.cmp(&left.provider_id))
                })
                .then_with(|| right.released.cmp(&left.released))
        });
        // Family dedup: newest per (provider, family) unless `all`.
        if !input.all.unwrap_or(false) {
            let mut seen: Vec<(String, String)> = Vec::new();
            rows.retain(|row| match &row.family {
                None => true,
                Some(family) => {
                    let key = (row.provider_id.clone(), family.clone());
                    if seen.contains(&key) {
                        false
                    } else {
                        seen.push(key);
                        true
                    }
                }
            });
        }
        let total = rows.len();
        let page: Vec<Row> = rows.into_iter().skip(offset).take(limit).collect();
        let mut order: Vec<String> = Vec::new();
        for row in &page {
            if !order.contains(&row.provider_id) {
                order.push(row.provider_id.clone());
            }
        }
        let providers: Vec<ProviderPage> = order
            .into_iter()
            .map(|id| {
                let models: Vec<ModelEntry> = page
                    .iter()
                    .filter(|row| row.provider_id == id)
                    .map(|row| ModelEntry {
                        id: format!("{}/{}", row.provider_id, row.model_id),
                        name: row.model_name.clone(),
                        released: row.released,
                        variants: row.variants.clone(),
                        cost: row.cost.clone(),
                        status: row.status.clone(),
                    })
                    .collect();
                let name = page
                    .iter()
                    .find(|row| row.provider_id == id)
                    .map(|row| row.provider_name.clone())
                    .unwrap_or_else(|| id.clone());
                ProviderPage { id, name, models }
            })
            .collect();
        let next = if offset + limit < total { Some(offset + limit) } else { None };
        ModelsOutput { providers, total, next }
    }
}

impl ToolExecuteWithContext for ModelsExecute {
    fn execute<'a>(
        &'a self,
        _services: &'a ToolServices,
        input: &'a serde_json::Value,
        context: &'a crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        let input = input.clone();
        Box::pin(async move {
            let query: ModelsQuery = serde_json::from_value(input)
                .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            // G-86 — ref opencode.ts:134-142: `own` is the invoking
            // session's selected model provider, read at execution time.
            let pinned = self.own_provider.lock().clone();
            let own = match pinned {
                Some(pinned) => Some(pinned),
                None => self.session_own_provider(context.session_id.as_str()).await,
            };
            let output = self.list(&query, own.as_deref());
            Ok(ToolOutcome {
                content: vec![oc_schema::tool::Content::Text {
                    // `normalizeContent` stringifies output-only results with
                    // compact `JSON.stringify` (runtime.ts:265-269).
                    text: serde_json::to_string(&output).unwrap_or_default(),
                }],
                metadata: None,
                output: Some(serde_json::to_value(&output).unwrap_or_default()),
            })
        })
    }
}

/// `session_rename` registration (namespace `opencode`).
pub fn session_rename_tool(
    store: crate::session::store::SessionStore,
    bus: crate::bus::Bus,
) -> BuiltinTool {
    BuiltinTool {
        name: "session_rename",
        description: "Rename a session, or omit sessionID to rename the current session. Use a short, specific title that summarizes the work being done.".into(),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "sessionID": { "type": "string", "description": "Omit to rename the current session." },
                "title": { "type": "string", "minLength": 1, "description": "New session title." }
            },
            "required": ["title"]
        }),
        permission: None,
        codemode: true,
        namespace: Some("opencode"),
        execute: Arc::new(SessionRenameExecute { store, bus }),
    }
}

/// `session_move` registration (namespace `opencode`).
pub fn session_move_tool(deps: SessionMoveServices) -> BuiltinTool {
    BuiltinTool {
        name: "session_move",
        description: "Move a session to another directory, or omit sessionID to move the current session. The current session moves at the next safe boundary; do not run destination-dependent tools in the same execute call.".into(),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "sessionID": { "type": "string", "description": "Omit to move the current session." },
                "directory": { "type": "string", "minLength": 1, "description": "Destination directory, relative to the target session's directory or absolute. Supports ~." }
            },
            "required": ["directory"]
        }),
        permission: None,
        codemode: true,
        namespace: Some("opencode"),
        execute: Arc::new(SessionMoveExecute { deps }),
    }
}

/// `models` registration.
pub fn models_tool(
    catalog: crate::provider_catalog::catalog::Catalog,
    store: crate::session::store::SessionStore,
) -> BuiltinTool {
    BuiltinTool {
        name: "models",
        description: "Search the models available to use. Use this to turn a model name the user mentions into an exact reference before running a subagent on it. Check your own provider first.".into(),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Text to search for in model names and IDs." },
                "provider": { "type": "string", "description": "Provider ID or name to filter by. Try your own provider first." },
                "all": { "type": "boolean", "description": "Include older versions of each model family. By default only the newest version is listed." },
                "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Maximum number of models to return. Defaults to 20." },
                "offset": { "type": "integer", "minimum": 0, "description": "Number of models to skip, for paging through results." }
            }
        }),
        permission: None,
        codemode: true,
        namespace: Some("opencode"),
        execute: Arc::new(ModelsExecute {
            catalog,
            own_provider: parking_lot::Mutex::new(None),
            store: Some(store),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G-266③ — ref opencode.ts:71-81: the worktree guidance is
    /// appended to the system parts on EVERY context/generate/
    /// compaction event (the previously dead `WORKTREE_GUIDANCE`
    /// constant's runtime).
    #[tokio::test]
    async fn tool_opencode_worktree_guidance_rides_all_three_hooks() {
        let hooks = crate::hooks::HookRegistry::new();
        super::register_worktree_guidance(&hooks);
        let base_event = || crate::hooks::SessionContextEvent {
            session_id: oc_schema::id::SessionId::parse("ses_guidance").expect("id"),
            agent: oc_schema::id::AgentId::parse("build"),
            model: oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("p"),
                id: oc_schema::id::ModelId::parse("m"),
                variant: None,
            },
            system: Vec::new(),
            messages: Vec::new(),
            options: Default::default(),
            tools: Default::default(),
        };
        for shaped in [
            hooks.dispatch_session_context(base_event()).await.expect("context"),
            hooks.dispatch_session_generate(base_event()).await.expect("generate"),
        ] {
            assert!(
                shaped.system.iter().any(|part| part.text == WORKTREE_GUIDANCE),
                "context/generate carry the worktree guidance"
            );
        }
        let shaped = hooks
            .dispatch_session_compaction(crate::hooks::SessionCompactionEvent {
                context: base_event(),
                result: None,
            })
            .await
            .expect("compaction");
        assert!(
            shaped
                .context
                .system
                .iter()
                .any(|part| part.text == WORKTREE_GUIDANCE),
            "compaction carries the worktree guidance"
        );
    }

    fn catalog() -> crate::provider_catalog::catalog::Catalog {
        crate::provider_catalog::catalog::Catalog::parse(
            r#"{
                "mine": {
                    "name": "Mine",
                    "id": "mine",
                    "models": {
                        "new": {
                            "id": "new",
                            "name": "Mine New",
                            "family": "mine",
                            "release_date": "2024-06-01",
                            "attachment": false,
                            "reasoning": true,
                            "tool_call": true,
                            "cost": { "input": 1, "output": 2, "cache_read": 0.5, "cache_write": 0 },
                            "limit": { "context": 1000, "output": 10 }
                        },
                        "old": {
                            "id": "old",
                            "name": "Mine Old",
                            "family": "mine",
                            "release_date": "2023-01-01",
                            "attachment": false,
                            "reasoning": false,
                            "tool_call": true,
                            "limit": { "context": 1000, "output": 10 }
                        }
                    }
                },
                "other": {
                    "name": "Other",
                    "id": "other",
                    "models": {
                        "solo": {
                            "id": "solo",
                            "name": "Other Solo",
                            "release_date": "2024-01-01",
                            "attachment": false,
                            "reasoning": false,
                            "tool_call": false,
                            "limit": { "context": 1000, "output": 10 }
                        }
                    }
                }
            }"#,
        )
        .expect("catalog parses")
    }

    /// G-86 — `own` resolves from the invoking session's row at
    /// execution time (`session.model.providerID`), sorting that
    /// provider's models first (opencode.ts:134-142,157-163).
    #[tokio::test]
    async fn tool_opencode_models_own_provider_resolves_from_session() {
        let db = crate::database::Database::open_in_memory().await.expect("db");
        {
            let project =
                crate::database::schema::ProjectRow::new("proj_g86", "/tmp/opencode", None)
                    .expect("project");
            db.transaction(move |tx| crate::database::schema::ProjectRow::insert(tx, &project))
                .await
                .expect("project insert");
        }
        let mut row = crate::database::schema::SessionRow::new(
            "ses_g86",
            "proj_g86",
            "/tmp/opencode",
            "slug",
            "1",
        )
        .expect("row");
        row.model = Some(r#"{"id":"new","providerID":"mine"}"#.to_string());
        db.transaction(move |tx| crate::database::schema::SessionRow::insert(tx, &row))
            .await
            .expect("insert");

        let execute = ModelsExecute {
            catalog: catalog(),
            own_provider: parking_lot::Mutex::new(None),
            store: Some(crate::session::store::SessionStore::new(db)),
        };
        // Resolved straight off the session row.
        assert_eq!(execute.session_own_provider("ses_g86").await, Some("mine".to_string()));
        // Unknown session / missing model degrade to None.
        assert_eq!(execute.session_own_provider("ses_none").await, None);

        // The resolved own provider orders the listing.
        let own = execute.session_own_provider("ses_g86").await;
        let output = execute.list(&ModelsQuery::default(), own.as_deref());
        assert_eq!(output.providers[0].id, "mine");
        assert_eq!(output.providers[0].models[0].id, "mine/new");
    }

    #[test]
    fn tool_opencode_models_sorts_own_provider_first_with_family_dedup() {
        let execute = ModelsExecute {
            catalog: catalog(),
            own_provider: parking_lot::Mutex::new(None),
            store: None,
        };
        let output = execute.list(&ModelsQuery::default(), Some("mine"));
        assert_eq!(output.total, 2, "family dedup drops Mine Old");
        assert_eq!(output.providers.len(), 2, "mine first, then other");
        assert_eq!(output.providers[0].id, "mine");
        assert_eq!(output.providers[0].name, "Mine");
        assert_eq!(output.providers[0].models[0].id, "mine/new");
        assert_eq!(output.providers[0].models[0].name, "Mine New");
        assert_eq!(output.next, None);

        let all =
            execute.list(&ModelsQuery { all: Some(true), ..Default::default() }, Some("mine"));
        assert_eq!(all.total, 3);
        assert_eq!(all.providers[0].models.len(), 2);
    }

    #[test]
    fn tool_opencode_models_pages() {
        let execute = ModelsExecute {
            catalog: catalog(),
            own_provider: parking_lot::Mutex::new(None),
            store: None,
        };
        let page = execute.list(
            &ModelsQuery { limit: Some(1), offset: Some(0), all: Some(true), ..Default::default() },
            Some("other"),
        );
        assert_eq!(page.total, 3);
        assert_eq!(page.providers.len(), 1);
        assert_eq!(page.providers[0].id, "other");
        assert_eq!(page.next, Some(1));
        let next = execute.list(
            &ModelsQuery { limit: Some(1), offset: Some(1), all: Some(true), ..Default::default() },
            Some("other"),
        );
        assert_eq!(next.providers[0].id, "mine");
    }

    #[test]
    fn tool_opencode_models_filters_by_query_and_provider() {
        let execute = ModelsExecute {
            catalog: catalog(),
            own_provider: parking_lot::Mutex::new(None),
            store: None,
        };
        let output =
            execute.list(&ModelsQuery { query: Some("solo".into()), ..Default::default() }, None);
        assert_eq!(output.total, 1);
        assert_eq!(output.providers[0].id, "other");
        let output = execute.list(
            &ModelsQuery { provider: Some("Mine".into()), all: Some(true), ..Default::default() },
            None,
        );
        assert_eq!(output.total, 2);
        assert_eq!(output.providers[0].id, "mine");
    }

    /// D-05: the provider key is `localeCompare` (ICU case-insensitive
    /// primary), not a byte compare — `"anthropic"` sorts before
    /// `"Z.ai"` even though `'Z' < 'a'` in ASCII.
    #[test]
    fn tool_opencode_models_provider_order_is_locale_collated() {
        let mixed = crate::provider_catalog::catalog::Catalog::parse(
            r#"{
                "Z.ai": {
                    "name": "Z.ai", "id": "Z.ai",
                    "models": { "glm": { "id": "glm", "name": "GLM", "release_date": "2024-06-01",
                        "limit": { "context": 1000, "output": 10 } } }
                },
                "anthropic": {
                    "name": "Anthropic", "id": "anthropic",
                    "models": { "claude": { "id": "claude", "name": "Claude", "release_date": "2024-06-01",
                        "limit": { "context": 1000, "output": 10 } } }
                }
            }"#,
        )
        .expect("catalog parses");
        let execute = ModelsExecute {
            catalog: mixed,
            own_provider: parking_lot::Mutex::new(None),
            store: None,
        };
        let output = execute.list(&ModelsQuery::default(), None);
        let order: Vec<&str> = output.providers.iter().map(|page| page.id.as_str()).collect();
        assert_eq!(order, vec!["anthropic", "Z.ai"], "localeCompare, not byte order");
    }
}
