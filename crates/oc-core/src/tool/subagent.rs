//! `tool/plugin/subagent.ts` port (core-runner-map §4.13) — spawns an
//! agent in a child session. All model-facing strings are byte-exact
//! against v2; the tests pin them.

use std::sync::Arc;

use oc_schema::id::{AgentId, MessageId, SessionId};
use oc_schema::location;
use oc_schema::session_inbox::{Delivery, Item, UserPayload};
use oc_schema::tool::Content;
use serde_json::json;

use crate::location::{
    ToolCallContext, ToolExecute, ToolRegistration, ToolRegistryState, ToolRunFailure,
    ToolRunResult,
};
use crate::permission::{AssertInput, Permission, PermissionError};
use crate::session::inbox::SessionInbox;
use crate::session::store::{CreateInput, SessionStore};
use crate::session::subagent_job::SubagentJob;
use crate::subagent::{
    append_available_subagents, lenient_session_id, resolve_agent, resolve_model, session_depth,
    ChildExecution, JobControl, JobStatus, ModelSource, SubagentRecovery, FIRST_PROMPT_PREFIX,
};
use oc_schema::permission::Source;

/// `SubagentTool.name`.
pub const NAME: &str = "subagent";

/// `backgroundResult` (subagent.ts:19-27) — EXACT.
pub fn background_result(session_id: &str) -> String {
    [
        format!("The subagent is working in the background (sessionID: {session_id}). You will be notified automatically when it finishes."),
        "DO NOT sleep, poll for progress, ask the subagent for status, or duplicate this subagent's work; avoid working with the same files or topics it is using.".to_string(),
        "Work on non-overlapping tasks, or briefly tell the user what you launched and end your response.".to_string(),
    ]
    .join("\n")
}

/// The tool description (subagent.ts:55-62).
pub const DESCRIPTION: &str = "Spawns an agent in a child session to work on the specified task.\nThe output includes a sessionID you can pass back later to continue that specific conversation with the subagent.\nNew child sessions start with fresh context, so include all relevant context and instructions when you don't pass a sessionID.\nForeground (default) runs the subagent to completion and returns its final response.\nBackground mode (background=true) launches it asynchronously and returns immediately; you are notified when it finishes.\nUse background only for independent work that can run while you continue elsewhere.";

/// The input schema (JSON-Schema form of `SubagentTool.Input`).
#[must_use]
pub fn input_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "agent": {
                "type": "string",
                "description": "The type of specialized agent to use for this task. If the user asks for a subagent by a name that is not one of the available subagents, they most likely mean a model: pick a suitable agent and pass the name through the model parameter instead."
            },
            "description": {
                "type": "string",
                "description": "A short 3-5 word label for the task, displayed to the user"
            },
            "prompt": {
                "type": "string",
                "description": "The task for the subagent to perform"
            },
            "model": {
                "type": "string",
                "description": "NEVER set this unless the user explicitly asks for a particular model or variant. The value is written as \"providerID/modelID\", or \"providerID/modelID#variant\" to include a variant. Do not guess the ID: look the model up with the models tool, filtering to your own provider first."
            },
            "sessionID": {
                "type": "string",
                "description": "Continue a specific previous subagent conversation by passing its sessionID. Calls without a sessionID start a new conversation."
            },
            "background": {
                "type": "boolean",
                "description": "Run the subagent in the background and return immediately. You will be notified when it completes. DO NOT sleep, poll, or proactively check on its progress."
            }
        },
        "required": ["agent", "description", "prompt"]
    })
}

/// Dependencies the executor needs (the plugin's service context).
pub struct SubagentDeps {
    pub store: SessionStore,
    pub inbox: SessionInbox,
    pub execution: Arc<dyn ChildExecution>,
    pub jobs: Arc<dyn JobControl>,
    pub subagents: Arc<SubagentJob>,
    pub models: Arc<dyn ModelSource>,
    pub permission: Permission,
    /// `experimental.subagent_depth` (defaults to 1).
    pub depth: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// Agents visible to the caller's location.
    pub agents: Arc<dyn Fn() -> Vec<oc_schema::agent::Info> + Send + Sync>,
    /// Per-call progress channel (v2 `context.progress` — the runner
    /// publishes `session.tool.progress` with the tool CALL id,
    /// step.ts:96). The invocation context carries the ids; the
    /// payload is the metadata record.
    pub progress: Option<Arc<dyn Fn(&ToolCallContext, serde_json::Value) + Send + Sync>>,
    /// The interrupt bell raced against the foreground block (the runner's
    /// tool-fiber cancel).
    pub interrupt: Option<Arc<tokio::sync::Notify>>,
}

/// Decoded tool input.
#[derive(Debug, Clone, Default)]
pub struct SubagentInput {
    pub agent: String,
    pub description: String,
    pub prompt: String,
    pub model: Option<String>,
    pub session_id: Option<SessionId>,
    pub background: bool,
}

impl SubagentInput {
    /// Decodes the raw tool arguments; empty-string `model`/`sessionID`
    /// (the `execute.before` hook in v2 strips them) are dropped.
    pub fn decode(value: &serde_json::Value) -> Result<Self, ToolRunFailure> {
        let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
        let Some(agent) = text("agent") else {
            return Err(ToolRunFailure::Error {
                message: "Invalid arguments for tool \"subagent\": missing agent".to_string(),
                metadata: None,
            });
        };
        let session_id = match text("sessionID") {
            Some("") | None => None,
            Some(raw) => {
                Some(SessionId::parse(raw.to_string()).map_err(|_| ToolRunFailure::Error {
                    message: format!(
                        "sessionID: Expected a string starting with \"ses\", got: \"{raw}\""
                    ),
                    metadata: None,
                })?)
            }
        };
        Ok(Self {
            agent: agent.to_string(),
            description: text("description").unwrap_or_default().to_string(),
            prompt: text("prompt").unwrap_or_default().to_string(),
            model: text("model").filter(|model| !model.is_empty()).map(str::to_string),
            session_id,
            background: value
                .get("background")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// One settled subagent execution (the tool `Output`).
#[derive(Debug, Clone)]
pub struct SubagentOutput {
    pub session_id: SessionId,
    pub status: &'static str,
    pub output: String,
}

/// The executor (subagent.ts:108-266).
pub struct SubagentTool {
    deps: SubagentDeps,
}

impl SubagentTool {
    #[must_use]
    pub fn new(deps: SubagentDeps) -> Self {
        Self { deps }
    }

    /// `ctx.tool.transform(editor => editor.add(…))` — the registration.
    #[must_use]
    pub fn registration() -> ToolRegistration {
        let mut registration = ToolRegistration::new(NAME, DESCRIPTION, input_schema());
        registration.permission = Some(NAME.to_string());
        registration.codemode = false;
        registration
    }

    /// The description hook (subagent.ts:282-306) applied per-context.
    #[must_use]
    pub fn description_for(
        agents: &[oc_schema::agent::Info],
        permissions: &oc_schema::permission::Ruleset,
    ) -> String {
        append_available_subagents(DESCRIPTION, agents, permissions)
    }

    /// The execute path. `context` mirrors v2's `Tool.Context`: session /
    /// message / call identity for the permission assert.
    pub async fn execute(
        &self,
        input: &SubagentInput,
        context: &ToolCallContext,
    ) -> Result<SubagentOutput, ToolRunFailure> {
        let deps = &self.deps;
        let parent = deps
            .store
            .get(context.session_id.as_str())
            .await
            .map_err(|error| parent_not_found(&context.session_id, error))?
            .ok_or_else(|| ToolRunFailure::Error {
                message: format!("Parent session not found: {}", context.session_id),
                metadata: None,
            })?;

        // Depth guard: walk the parentID chain.
        let depth = session_depth(&deps.store, &parent)
            .await
            .map_err(|message| ToolRunFailure::Error { message, metadata: None })?;
        let limit = (deps.depth)();
        if depth >= limit {
            return Err(ToolRunFailure::Error {
                message: format!(
                    "Subagent depth limit reached ({limit}). Increase \"experimental.subagent_depth\" to allow nested subagents."
                ),
                metadata: None,
            });
        }

        let agents = (deps.agents)();
        let agent = resolve_agent(&agents, &input.agent)
            .map_err(|message| ToolRunFailure::Error { message, metadata: None })?;
        let agent_resource = agent.id.0.clone();

        // Permission assert: action subagent, resource/save agent.id.
        deps.permission
            .assert(AssertInput {
                id: None,
                session_id: context.session_id.clone(),
                agent: Some(context.agent.clone()),
                action: NAME.to_string(),
                resources: vec![agent_resource.clone()],
                save: Some(vec![agent_resource.clone()]),
                metadata: None,
                source: Some(Source::Tool {
                    message_id: context.message_id.clone(),
                    id: context.call_id.clone(),
                }),
            })
            .await
            .map_err(|error| ToolRunFailure::Error {
                message: format!(
                    "Subagent denied: {agent_resource}: {}",
                    permission_message(&error)
                ),
                metadata: None,
            })?;

        // Continue path: input.sessionID must be a child of the current session.
        let existing = match &input.session_id {
            Some(session_id) => {
                let row = deps
                    .store
                    .get(session_id.as_str())
                    .await
                    .map_err(|error| ToolRunFailure::Error {
                        message: format!(
                            "Subagent session not found: {}: {error}",
                            session_id.as_str()
                        ),
                        metadata: None,
                    })?
                    .ok_or_else(|| ToolRunFailure::Error {
                        message: format!("Subagent session not found: {}", session_id.as_str()),
                        metadata: None,
                    })?;
                if row.parent_id.as_deref() != Some(context.session_id.as_str()) {
                    return Err(ToolRunFailure::Error {
                        message: format!(
                            "Session {} is not a child of the current session",
                            row.id
                        ),
                        metadata: None,
                    });
                }
                Some(row)
            }
            None => None,
        };

        let override_model = input
            .model
            .as_deref()
            .map(|model| resolve_model(&*deps.models, model))
            .transpose()
            .map_err(|message| ToolRunFailure::Error { message, metadata: None })?;

        // Continuing with a different agent switches the child, mirroring
        // create semantics where an explicit model wins over the agent's
        // configured model, which wins over the inherited one.
        if let Some(existing) = &existing {
            let switched = existing.agent.as_deref() != Some(agent.id.0.as_str());
            let model =
                override_model.clone().or_else(|| switched.then(|| agent.model.clone()).flatten());
            if switched {
                self.switch_agent(context, existing, &agent.id).await?;
            }
            if let Some(model) = model {
                self.switch_model(context, existing, model).await?;
            }
        }

        let model = override_model
            .or_else(|| agent.model.clone())
            .or_else(|| crate::subagent::decode_row_model(&parent));

        let child = match existing {
            Some(existing) => existing,
            None => match deps
                .store
                .create(
                    &deps.inbox.bus(),
                    CreateInput {
                        id: None,
                        parent_id: Some(context.session_id.clone()),
                        location: Some(location::Ref {
                            directory: oc_schema::common::AbsolutePath(parent.directory.clone()),
                            workspace_id: parent.workspace_id.clone().map(|value| {
                                oc_schema::id::WorkspaceId::parse(value)
                                    .unwrap_or_else(|err| oc_schema::id::WorkspaceId(err.value))
                            }),
                        }),
                        title: Some(input.description.clone()),
                        agent: Some(agent.id.0.clone()),
                        model,
                        metadata: None,
                        permissions: None,
                        project: None,
                        version: "2".to_string(),
                    },
                )
                .await
            {
                Ok(created) => match created {
                    crate::session::store::CreateOutcome::Created(row)
                    | crate::session::store::CreateOutcome::Existing(row) => row,
                },
                Err(_) => {
                    return Err(ToolRunFailure::Error {
                        message: format!("Parent session not found: {}", context.session_id),
                        metadata: None,
                    });
                }
            },
        };

        let background = input.background;
        if let Some(progress) = &deps.progress {
            // v2 `yield* context.progress({ sessionID: child.id, status:
            // "running" })` (subagent.ts:201) — the runner injects the
            // tool CALL id (step.ts:96 `publisher.progress(call.id,
            // update)`); the child id is PAYLOAD. The beta.13 real-env
            // bug passed the child id as the tool id (and the engine
            // never wired the channel at all), so the RUNNING subagent
            // card carried no `metadata.sessionID` — not clickable
            // mid-run.
            progress(context, json!({ "sessionID": child.id.as_str(), "status": "running" }));
        }

        // Standard prompt admission outside the job: Job.start joining a
        // running child skips its run effect, and the wake below starts an
        // idle child or steers a running one. The wake does NOT await the
        // drain (v2 session.ts:174 `execution.wake` — never the awaiting
        // `sessions.resume`): the only awaiting resume is the job body's,
        // which joins this wake-started execution.
        let text = if existing_is_none(&input.session_id) {
            format!("{FIRST_PROMPT_PREFIX}\n{}", input.prompt)
        } else {
            input.prompt.clone()
        };
        let inbox_id = MessageId::new();
        deps.inbox
            .admit(
                inbox_id.as_str(),
                child.id.as_str(),
                Item::User {
                    payload: UserPayload { text, ..UserPayload::default() },
                    delivery: Delivery::Steer,
                },
            )
            .await
            .map_err(|_| ToolRunFailure::Error {
                message: format!("Failed to prompt subagent: {}", child.id.as_str()),
                metadata: None,
            })?;
        // `resume: false` only applies to a brand-new background child.
        let wake = !(background && existing_is_none(&input.session_id));
        if wake {
            deps.execution.wake(child.id.as_str());
        }

        let recovery = SubagentRecovery {
            parent_session_id: context.session_id.clone(),
            child_session_id: lenient_session_id(child.id.clone()),
            agent: agent.name.0.clone(),
            description: input.description.clone(),
        };
        deps.subagents.start(&recovery).await;

        if background {
            deps.subagents.background(&recovery).await;
            return Ok(SubagentOutput {
                session_id: lenient_session_id(child.id.clone()),
                status: "running",
                output: background_result(&child.id),
            });
        }

        // Foreground: block on the child job; interrupt ⇒ child interrupt
        // + cancel (subagent.ts:234-239). The runner's per-attempt bell
        // drops this future mid-block; the guard fires the same child
        // interrupt + job cancel on that drop, and the direct bell (when
        // wired) resolves first.
        let result = {
            let block = deps.subagents.block(&child.id, context.session_id.as_str());
            let interrupt = deps.interrupt.clone();
            let mut guard = SubagentCancelGuard {
                subagents: deps.subagents.clone(),
                child: child.id.clone(),
                armed: true,
            };
            let result = match interrupt {
                Some(bell) => tokio::select! {
                    biased;
                    () = bell.notified() => {
                        guard.disarm();
                        deps.subagents.interrupt(&child.id).await;
                        return Err(ToolRunFailure::Declined {
                            message: format!("Subagent cancelled (sessionID: {})", child.id),
                        });
                    }
                    outcome = block => outcome,
                },
                None => block.await,
            };
            guard.disarm();
            result
        };

        match result {
            Some(crate::subagent::BlockOutcome::Backgrounded(info)) => {
                deps.subagents.notify(&recovery, info.started_at).await;
                Ok(SubagentOutput {
                    session_id: lenient_session_id(child.id.clone()),
                    status: "running",
                    output: background_result(&child.id),
                })
            }
            Some(crate::subagent::BlockOutcome::Finished(info)) => match info.status {
                JobStatus::Error => Err(ToolRunFailure::Error {
                    message: format!(
                        "Subagent failed (sessionID: {}): {}",
                        child.id,
                        info.error.as_deref().unwrap_or("unknown error")
                    ),
                    metadata: None,
                }),
                JobStatus::Cancelled => Err(ToolRunFailure::Error {
                    message: format!("Subagent cancelled (sessionID: {})", child.id),
                    metadata: None,
                }),
                JobStatus::Completed => Ok(SubagentOutput {
                    session_id: lenient_session_id(child.id.clone()),
                    status: "completed",
                    output: info.output.unwrap_or_else(|| crate::subagent::NO_TEXT.to_string()),
                }),
                JobStatus::Running => Err(ToolRunFailure::Error {
                    message: format!("Subagent cancelled (sessionID: {})", child.id),
                    metadata: None,
                }),
            },
            // Missing job (e.g. removed): treat as cancelled — mirrors the
            // interrupt path where cancel settles before block registers.
            None => Err(ToolRunFailure::Error {
                message: format!("Subagent cancelled (sessionID: {})", child.id),
                metadata: None,
            }),
        }
    }
}

/// The `Effect.onInterrupt` translation for the foreground block: future
/// drop (the runner's tool-fiber cancel) fires the child-session interrupt
/// + job cancel v2 performs on fiber interrupt. Fire-and-forget (Drop
/// cannot await); both operations are idempotent.
struct SubagentCancelGuard {
    subagents: Arc<crate::session::subagent_job::SubagentJob>,
    child: String,
    armed: bool,
}

impl SubagentCancelGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SubagentCancelGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let subagents = Arc::clone(&self.subagents);
        let child = self.child.clone();
        tokio::spawn(async move {
            subagents.interrupt(&child).await;
        });
    }
}

fn existing_is_none(session_id: &Option<SessionId>) -> bool {
    session_id.is_none()
}

impl SubagentTool {
    /// `sessions.switchAgent` — publish `session.agent.selected`; failure
    /// maps to the exact switch message.
    async fn switch_agent(
        &self,
        context: &ToolCallContext,
        existing: &crate::database::schema::SessionRow,
        agent: &AgentId,
    ) -> Result<(), ToolRunFailure> {
        let data = json!({
            "sessionID": existing.id,
            "agent": agent.0,
            "previous": existing.agent,
        });
        self.publish_switch(context, "session.agent.selected", data, &existing.id).await
    }

    /// `sessions.switchModel` — publish `session.model.selected`. v2
    /// normalizes an omitted variant to `"default"` on read
    /// (`session/info.ts:34`); the projector stores verbatim, so the
    /// default is baked in here for parity.
    async fn switch_model(
        &self,
        context: &ToolCallContext,
        existing: &crate::database::schema::SessionRow,
        mut model: oc_schema::model::Ref,
    ) -> Result<(), ToolRunFailure> {
        model.variant = Some(
            model
                .variant
                .take()
                .unwrap_or_else(|| oc_schema::id::ModelVariantId("default".to_string())),
        );
        let data = json!({
            "sessionID": existing.id,
            "model": serde_json::to_value(&model).unwrap_or_default(),
            "previous": existing.model.as_deref().and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok()),
        });
        self.publish_switch(context, "session.model.selected", data, &existing.id).await
    }

    async fn publish_switch(
        &self,
        _context: &ToolCallContext,
        type_: &str,
        data: serde_json::Value,
        session: &str,
    ) -> Result<(), ToolRunFailure> {
        use crate::bus::PublishOptions;
        let failed = || ToolRunFailure::Error {
            message: format!("Failed to switch subagent session: {session}"),
            metadata: None,
        };
        let definition = crate::bus::registry::by_type_latest(type_).cloned().ok_or_else(failed)?;
        self.deps
            .inbox
            .bus()
            .publish(&definition, data, PublishOptions::default())
            .await
            .map_err(|_| failed())?;
        Ok(())
    }
}

fn parent_not_found(
    session_id: &SessionId,
    error: crate::database::error::DbError,
) -> ToolRunFailure {
    ToolRunFailure::Error {
        message: format!("Parent session not found: {}: {error}", session_id.as_str()),
        metadata: None,
    }
}

fn permission_message(error: &PermissionError) -> String {
    match error {
        PermissionError::Declined(_) => "The user declined this tool call".to_string(),
        PermissionError::Corrected(corrected) => corrected.feedback.clone(),
        PermissionError::Blocked(blocked) => blocked.to_string(),
        PermissionError::SessionNotFound(id) => format!("session not found: {id}"),
    }
}

#[async_trait::async_trait]
impl ToolExecute for SubagentTool {
    async fn execute(&self, _input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        // The subagent path needs the invocation identity (session, agent,
        // message, call) for its permission assert; route through
        // `execute_with_context` — the C1 pipeline always provides one.
        Err(ToolRunFailure::Error {
            message: "subagent tool requires an invocation context".to_string(),
            metadata: None,
        })
    }

    async fn execute_with_context(
        &self,
        input: &serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        let decoded = SubagentInput::decode(input)?;
        let output = self.execute(&decoded, context).await?;
        Ok(settle(output))
    }
}

/// The result mapping (subagent.ts:257-265): completed runs wrap in the
/// EXACT `<subagent …>` envelope; running (background) runs pass the
/// DO-NOT-POLL text through; metadata carries `{sessionID, status}`.
pub fn settle(output: SubagentOutput) -> ToolRunResult {
    let content = if output.status == "completed" {
        vec![Content::Text {
            text: format!(
                "<subagent sessionID=\"{}\" state=\"completed\">\n{}\n</subagent>",
                output.session_id, output.output
            ),
        }]
    } else {
        vec![Content::Text { text: output.output.clone() }]
    };
    let mut metadata = oc_schema::common::Record::default();
    metadata.insert("sessionID".into(), json!(output.session_id.as_str()));
    metadata.insert("status".into(), json!(output.status));
    // TOOL-D1: v2's declared output is the whole settled subagent value
    // — `{sessionID, status, output}` (subagent.ts:253-266) — while the
    // envelope above is only the model-facing content.
    ToolRunResult {
        content,
        metadata: Some(metadata),
        output: Some(json!({
            "sessionID": output.session_id.as_str(),
            "status": output.status,
            "output": output.output,
        })),
    }
}

/// Installs the tool into a tool registry (the `editor.add` line).
pub fn register(registry: &ToolRegistryState, tool: Arc<SubagentTool>) {
    registry.register(SubagentTool::registration());
    registry.register_executor(NAME, tool);
}
