//! Subagent support — port of the shared pieces of
//! `tool/plugin/subagent.ts` + `session/{subagent-job,subagent-completion}.ts`
//! (core-runner-map §4.13).
//!
//! The tool leaf itself lives in [`crate::tool::subagent`]; this module
//! owns:
//!
//! - the **depth guard** (parentID chain walk, `experimental.subagent_depth`
//!   default 1, exact message);
//! - **agent resolution** (subagent-eligible agents, primary rejection);
//! - **model resolution** (`resolveModel` with the four exact failure
//!   messages and the `default`-variant normalization `session.fromRow`
//!   applies);
//! - the **jobs adapter seam** ([`JobControl`]) — the C2 lane owns the full
//!   `jobs.ts` port (durable KV markers, background recovery); until it
//!   lands this module defines the minimal interface the subagent path
//!   needs behind [`MemoryJobs`], to be swapped at the wiring site;
//! - the **child execution seam** ([`ChildExecution`]) — resume/interrupt
//!   against the session execution coordinator.

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_schema::agent::{Info as AgentInfo, Mode as AgentMode};
use oc_schema::config::Entry;
use oc_schema::id::{MessageId, SessionId};
use oc_schema::model;
use oc_schema::session_message::Info as MessageInfo;
use oc_schema::session_support::SessionMetadata;

/// Rebrands an id, tolerating legacy values (DB rows store raw strings).
pub fn lenient_session_id(value: impl Into<String>) -> SessionId {
    SessionId::parse(value.into()).unwrap_or_else(|err| SessionId(err.value))
}

use crate::database::schema::{SessionMessageRow, SessionRow};
use crate::session::store::{ListOrder, MessagesInput, SessionStore};

/// `Config.latest(entries, "experimental")?.subagent_depth ?? 1`.
#[must_use]
pub fn subagent_depth(entries: &[Entry]) -> u64 {
    let mut depth = 1u64;
    for entry in entries {
        if let Entry::Document(document) = entry {
            if let Some(experimental) = &document.info.experimental {
                if let Some(value) = experimental.subagent_depth {
                    depth = value;
                }
            }
        }
    }
    depth
}

/// `SubagentCompletion.NO_TEXT`.
pub const NO_TEXT: &str = "Subagent completed without a text response.";

/// First-prompt prefix for a freshly spawned child (subagent.ts:209-212).
pub const FIRST_PROMPT_PREFIX: &str = "You are a subagent spawned by another session.";

/// `SubagentJob.Recovery` — the durable recovery record for a subagent job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRecovery {
    pub parent_session_id: SessionId,
    pub child_session_id: SessionId,
    /// Agent **name** (v2 stores `agent.name`, not the id).
    pub agent: String,
    pub description: String,
}

/// Job status (`job.ts Status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Completed,
    Error,
    Cancelled,
}

impl JobStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Error => "error",
            JobStatus::Cancelled => "cancelled",
        }
    }
}

/// `Job.Info`.
#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: String,
    pub job_type: String,
    pub title: Option<String>,
    pub status: JobStatus,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub notification_id: Option<MessageId>,
}

/// `Job.block` result: the job finished, or the user backgrounded it.
#[derive(Debug, Clone)]
pub enum BlockOutcome {
    Finished(JobInfo),
    Backgrounded(JobInfo),
}

/// The body of a job (`StartInput.run`): drives the child session and
/// yields its final text.
pub type JobRun = futures::future::BoxFuture<'static, JobRunOutcome>;

/// Terminal classification of a job body (`Exit.isSuccess` → completed,
/// interrupts-only → cancelled, otherwise error).
#[derive(Debug, Clone)]
pub enum JobRunOutcome {
    Completed(String),
    Failed(String),
    Cancelled,
}

/// `Job.start` input (the subagent-relevant subset).
pub struct JobStart {
    pub id: String,
    pub job_type: String,
    pub title: Option<String>,
    pub recovery: Option<SubagentRecovery>,
    pub run: JobRun,
}

/// The jobs adapter seam. **C2 owns the production implementation**
/// (`src/jobs.rs`: process-local map + durable `job.background/<id>` KV
/// markers with the 25-entry consumed history and restart re-notify);
/// this interface is the minimal surface the subagent path consumes, so
/// Wave D could land before the jobs lane. Swap at the wiring site.
#[async_trait::async_trait]
pub trait JobControl: Send + Sync {
    /// Registers (or joins) a job and returns its info. The body settles
    /// the job even when no one blocks on it.
    async fn start(&self, input: JobStart) -> JobInfo;
    /// `block` — waits for the job; `None` when the id is unknown.
    async fn block(&self, id: &str, session_id: &str) -> Option<BlockOutcome>;
    /// `background` — mints the durable notification marker for
    /// recoverable jobs and wakes blocked callers.
    async fn background(&self, id: &str) -> Option<JobInfo>;
    /// `cancel`.
    async fn cancel(&self, id: &str) -> Option<JobInfo>;
    /// `wait` — settles without consuming recoverable results.
    async fn wait(&self, id: &str) -> Option<JobInfo>;
    /// `get`.
    async fn get(&self, id: &str) -> Option<JobInfo>;
    /// `completeBackground` — acknowledges a delivered notification.
    async fn complete_background(&self, notification_id: &str);
}

/// In-memory [`JobControl`] (tests + interim adapter; the production
/// adapter over the C2 jobs lane is
/// [`crate::subagent_jobs_adapter::JobsAdapter`]). Job bodies settle in
/// their own task (`jobs.ts` forks the run into a scope); waiters watch
/// versioned channels so no settle/observe race can drop a wakeup.
#[derive(Default)]
pub struct MemoryJobs {
    shared: Arc<SharedJobs>,
}

#[derive(Default)]
struct SharedJobs {
    inner: parking_lot::Mutex<BTreeMap<String, MemoryJob>>,
}

struct MemoryJob {
    info: JobInfo,
    done: tokio::sync::watch::Sender<Option<JobInfo>>,
    backgrounded: tokio::sync::watch::Sender<Option<JobInfo>>,
    is_backgrounded: bool,
    recovery: Option<SubagentRecovery>,
}

impl MemoryJobs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn settle(&self, id: &str, outcome: JobRunOutcome) {
        let mut jobs = self.shared.inner.lock();
        let Some(job) = jobs.get_mut(id) else { return };
        if job.info.status != JobStatus::Running {
            return;
        }
        match outcome {
            JobRunOutcome::Completed(output) => {
                job.info.status = JobStatus::Completed;
                job.info.output = Some(output);
            }
            JobRunOutcome::Failed(error) => {
                job.info.status = JobStatus::Error;
                job.info.error = Some(error);
            }
            JobRunOutcome::Cancelled => {
                job.info.status = JobStatus::Cancelled;
            }
        }
        job.info.completed_at = Some(now_ms());
        let info = job.info.clone();
        let _ = job.done.send(Some(info));
    }
}

#[async_trait::async_trait]
impl JobControl for MemoryJobs {
    async fn start(&self, input: JobStart) -> JobInfo {
        let (info, settle_state) = {
            let mut jobs = self.shared.inner.lock();
            if let Some(existing) = jobs.get(&input.id) {
                if existing.info.status == JobStatus::Running {
                    return existing.info.clone();
                }
            }
            let (done_tx, _done_rx) = tokio::sync::watch::channel(None);
            let (backgrounded_tx, _backgrounded_rx) = tokio::sync::watch::channel(None);
            let info = JobInfo {
                id: input.id.clone(),
                job_type: input.job_type,
                title: input.title,
                status: JobStatus::Running,
                started_at: now_ms(),
                completed_at: None,
                output: None,
                error: None,
                // `markBackground` mints this lazily for recoverable jobs.
                notification_id: None,
            };
            let job = MemoryJob {
                info: info.clone(),
                done: done_tx,
                backgrounded: backgrounded_tx,
                is_backgrounded: false,
                recovery: input.recovery,
            };
            jobs.insert(input.id.clone(), job);
            (info, Arc::clone(&self.shared))
        };
        // Fork the body: `Job.start` returns after registration; the run
        // settles the job even when nobody blocks on it.
        let id = input.id;
        let run = input.run;
        let jobs = settle_state;
        tokio::spawn(async move {
            let outcome = run.await;
            if let Some(this) = Arc::downgrade(&jobs).upgrade() {
                let this = MemoryJobs { shared: this };
                this.settle(&id, outcome);
            }
        });
        info
    }

    async fn block(&self, id: &str, _session_id: &str) -> Option<BlockOutcome> {
        let (mut done_rx, mut backgrounded_rx) = {
            let jobs = self.shared.inner.lock();
            let job = jobs.get(id)?;
            if job.info.status != JobStatus::Running {
                return Some(BlockOutcome::Finished(job.info.clone()));
            }
            if job.is_backgrounded {
                return Some(BlockOutcome::Backgrounded(job.info.clone()));
            }
            (job.done.subscribe(), job.backgrounded.subscribe())
        };
        loop {
            tokio::select! {
                changed = done_rx.changed() => {
                    if changed.is_err() {
                        return None;
                    }
                    if let Some(info) = done_rx.borrow().clone() {
                        return Some(BlockOutcome::Finished(info));
                    }
                }
                changed = backgrounded_rx.changed() => {
                    if changed.is_err() {
                        continue;
                    }
                    if let Some(info) = backgrounded_rx.borrow().clone() {
                        return Some(BlockOutcome::Backgrounded(info));
                    }
                }
            }
        }
    }

    async fn background(&self, id: &str) -> Option<JobInfo> {
        let mut jobs = self.shared.inner.lock();
        let job = jobs.get_mut(id)?;
        if job.info.status != JobStatus::Running && job.recovery.is_none() {
            return None;
        }
        if job.is_backgrounded {
            return Some(job.info.clone());
        }
        if job.recovery.is_some() && job.info.notification_id.is_none() {
            job.info.notification_id = Some(MessageId::new());
        }
        job.is_backgrounded = true;
        let info = job.info.clone();
        let _ = job.backgrounded.send(Some(info.clone()));
        Some(info)
    }

    async fn cancel(&self, id: &str) -> Option<JobInfo> {
        let mut jobs = self.shared.inner.lock();
        let job = jobs.get_mut(id)?;
        if job.info.status != JobStatus::Running {
            return Some(job.info.clone());
        }
        job.info.status = JobStatus::Cancelled;
        job.info.completed_at = Some(now_ms());
        let info = job.info.clone();
        let _ = job.done.send(Some(info.clone()));
        Some(info)
    }

    async fn wait(&self, id: &str) -> Option<JobInfo> {
        let mut done_rx = {
            let jobs = self.shared.inner.lock();
            let job = jobs.get(id)?;
            if job.info.status != JobStatus::Running {
                return Some(job.info.clone());
            }
            job.done.subscribe()
        };
        loop {
            if let Some(info) = done_rx.borrow().clone() {
                return Some(info);
            }
            if done_rx.changed().await.is_err() {
                return None;
            }
        }
    }

    async fn get(&self, id: &str) -> Option<JobInfo> {
        self.shared.inner.lock().get(id).map(|job| job.info.clone())
    }

    async fn complete_background(&self, notification_id: &str) {
        let mut jobs = self.shared.inner.lock();
        for job in jobs.values_mut() {
            if job.info.notification_id.as_ref().is_some_and(|id| id.as_str() == notification_id) {
                job.recovery = None;
                return;
            }
        }
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default(),
    )
    .unwrap_or_default()
}

/// The child-session execution seam (`Session.Service`'s resume/interrupt,
/// driven through the Wave-A coordinator). `resume` failing mirrors the
/// child runner failing (the subagent job settles as `error`).
#[async_trait::async_trait]
pub trait ChildExecution: Send + Sync {
    /// `sessions.resume` — start or join the drain.
    async fn resume(&self, session_id: &str) -> Result<(), String>;
    /// `execution.wake` (session.ts:174 — `sessions.prompt`'s
    /// `resume !== false` arm): register work WITHOUT awaiting the drain.
    /// The subagent tool's prompt admission wakes the child; the only
    /// awaiting resume is the job body's, which joins the wake-started
    /// execution (awaiting a resume in the tool instead lets the child
    /// settle idle before the job body runs, whose forced resume then
    /// drained one extra empty step — corpus3 #41).
    fn wake(&self, session_id: &str);
    /// `sessions.interrupt`.
    async fn interrupt(&self, session_id: &str);
}

/// Model-catalog seam (`Model.Service.available`).
pub trait ModelSource: Send + Sync {
    /// `(providerID, modelID)` → the catalog entry (with variants).
    fn model(&self, provider_id: &str, model_id: &str) -> Option<ModelEntry>;
}

/// The model fields `resolveModel` needs.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub provider_id: String,
    pub id: String,
    pub variants: Vec<String>,
}

impl ModelSource for crate::provider_catalog::Catalog {
    fn model(&self, provider_id: &str, model_id: &str) -> Option<ModelEntry> {
        let info = self.model(provider_id, model_id)?;
        Some(ModelEntry {
            provider_id: provider_id.to_string(),
            id: model_id.to_string(),
            variants: info.variants.iter().map(|variant| variant.id.0.clone()).collect(),
        })
    }
}

/// `resolveModel` (subagent.ts:75-98) — the four EXACT failure messages.
pub fn resolve_model(models: &dyn ModelSource, input: &str) -> Result<model::Ref, String> {
    let reference = model::Ref::parse(input).map_err(|_| {
        format!("Invalid model \"{input}\". Use \"providerID/modelID\" or \"providerID/modelID#variant\".")
    })?;
    let Some(entry) = models.model(&reference.provider_id.0, &reference.id.0) else {
        return Err(format!(
            "Model \"{}/{}\" is not available. Use the models tool to see what is available.",
            reference.provider_id, reference.id
        ));
    };
    if let Some(variant) = &reference.variant {
        if !entry.variants.iter().any(|available| available == &variant.0) {
            return Err(if entry.variants.is_empty() {
                format!(
                    "Model \"{}/{}\" has no variants. Omit the variant.",
                    reference.provider_id, reference.id
                )
            } else {
                format!(
                    "Variant \"{}\" is not available for \"{}/{}\". Available: {}.",
                    variant,
                    reference.provider_id,
                    reference.id,
                    entry.variants.join(", ")
                )
            });
        }
    }
    Ok(reference)
}

/// `session.fromRow` model decode: `variant ?? "default"` (info.ts:34).
#[must_use]
pub fn decode_row_model(row: &SessionRow) -> Option<model::Ref> {
    let raw = row.model.as_deref()?;
    let mut parsed: model::Ref = serde_json::from_str(raw).ok()?;
    parsed.variant = Some(
        parsed.variant.take().unwrap_or_else(|| oc_schema::id::ModelVariantId::parse("default")),
    );
    Some(parsed)
}

/// Decodes a `session_v2` row's metadata column.
#[must_use]
pub fn decode_row_metadata(row: &SessionRow) -> Option<SessionMetadata> {
    row.metadata.as_deref().and_then(|raw| serde_json::from_str(raw).ok())
}

/// Decodes a `session_message` row's payload. Rows store the encoded
/// message **minus** `id`/`type` (the projector strips them), so the row
/// type is re-injected before decoding.
#[must_use]
pub fn decode_message(row: &SessionMessageRow) -> Option<MessageInfo> {
    let mut value: serde_json::Value = serde_json::from_str(&row.data).ok()?;
    let object = value.as_object_mut()?;
    object.insert("type".to_string(), serde_json::json!(row.type_));
    object.insert("id".to_string(), serde_json::json!(row.id));
    serde_json::from_value(value).ok()
}

/// `SubagentCompletion.text` (subagent-completion.ts:10-19): the newest
/// completed assistant message's non-empty text parts joined by a blank
/// line, or [`NO_TEXT`].
pub async fn completion_text(store: &SessionStore, session_id: &str) -> String {
    let rows = store
        .messages(MessagesInput {
            session_id: session_id.to_string(),
            order: Some(ListOrder::Desc),
            limit: Some(20),
            ..MessagesInput::default()
        })
        .await
        .unwrap_or_default();
    for row in rows {
        let Some(MessageInfo::Assistant { time, content, error, .. }) = decode_message(&row) else {
            continue;
        };
        if time.completed.is_none() || error.is_some() {
            continue;
        }
        let text = content
            .iter()
            .filter_map(|part| match part {
                oc_schema::session_message::AssistantContent::Text { text, .. } => {
                    Some(text.clone())
                }
                _ => None,
            })
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !text.is_empty() {
            return text;
        }
        return NO_TEXT.to_string();
    }
    NO_TEXT.to_string()
}

/// `Available subagents:` description hook (subagent.ts:282-306): the
/// selected agent's context hook appends the non-primary, non-hidden,
/// not-denied subagents sorted by id.
#[must_use]
pub fn append_available_subagents(
    description: &str,
    agents: &[AgentInfo],
    permissions: &oc_schema::permission::Ruleset,
) -> String {
    let mut available: Vec<&AgentInfo> = agents
        .iter()
        .filter(|agent| {
            agent.mode != AgentMode::Primary
                && !agent.hidden
                && crate::permission::evaluate(
                    "subagent",
                    &agent.id.0,
                    std::slice::from_ref(permissions),
                )
                .effect
                    != oc_schema::permission::Effect::Deny
        })
        .collect();
    available.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    if available.is_empty() {
        return description.to_string();
    }
    let mut out = String::with_capacity(description.len() + 64);
    out.push_str(description);
    out.push_str("\n\nAvailable subagents:");
    for agent in available {
        out.push_str("\n- ");
        out.push_str(&agent.id.0);
        out.push_str(": ");
        out.push_str(&agent.description.clone().unwrap_or_else(|| {
            "This subagent should only be called when explicitly requested.".to_string()
        }));
    }
    out
}

/// Depth-guard walk (subagent.ts:117-133). Returns the depth of the
/// current session (number of ancestors) or the exact failure message when
/// an ancestor vanished.
pub async fn session_depth(store: &SessionStore, session: &SessionRow) -> Result<u64, String> {
    let mut current = session.clone();
    let mut depth = 0u64;
    while let Some(parent_id) = current.parent_id.clone() {
        depth += 1;
        let parent = store
            .get(&parent_id)
            .await
            .ok()
            .flatten()
            .ok_or_else(|| format!("Parent session not found: {parent_id}"))?;
        current = parent;
    }
    Ok(depth)
}

/// Agent resolution (subagent.ts:134-137).
pub fn resolve_agent<'a>(
    agents: &'a [AgentInfo],
    name_or_id: &str,
) -> Result<&'a AgentInfo, String> {
    let found = agents
        .iter()
        .find(|agent| agent.id.0 == name_or_id || agent.name.0 == name_or_id)
        .ok_or_else(|| format!("Unknown agent: {name_or_id}"))?;
    if found.mode == AgentMode::Primary {
        return Err(format!("Agent {name_or_id} cannot run as a subagent"));
    }
    Ok(found)
}

/// A resolved agent's id for permission resources/save.
#[must_use]
pub fn agent_resource(agent: &AgentInfo) -> String {
    agent.id.0.clone()
}
