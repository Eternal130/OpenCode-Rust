//! Port of `packages/core/src/permission.ts` — rule evaluation plus the
//! ask/assert/reply coordination loop.
//!
//! Evaluation mirrors the v2 engine exactly:
//!
//! 1. `base = merge(agent.permissions ?? DENY_ALL, session.permissions ?? [])`
//!    — the agent ruleset first, session rules override.
//! 2. Any resource evaluating `deny` over `base` short-circuits the whole
//!    request to `deny`; saved grants are **not** consulted (a base deny can
//!    never be overridden by a saved grant).
//! 3. Saved allows (project-scoped) are appended **last**, so they outrank
//!    everything: `all = [...base, ...saved]`. Per-resource evaluation over
//!    `all`; any `ask` ⇒ ask, else allow.
//! 4. An optional async `HookOverride` (the `permission.evaluate` plugin hook)
//!    may replace the computed effect/message. It runs only on the
//!    non-short-circuited path, matching v2.
//!
//! Coordination semantics (`ask`, `assert`, `reply`, `close`) follow
//! `.slim/deepwork/rust-port/core-deep-map.md` §4b: pending requests are parked
//! in a map keyed by `per_` id, `permission.asked`/`permission.replied` are
//! ephemeral events, replies publish `replied` **first**, `reject`
//! batch-rejects same-session siblings, and `always` + `save` persists a grant
//! then batch-approves the remaining pending requests by re-evaluation.

#![allow(clippy::must_use_candidate)] // wire-shape helpers, no side effects

pub mod saved;
pub mod sql;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use oc_schema::common::Record;
use oc_schema::event::EphemeralEnvelope;
use oc_schema::id::{AgentId, EventId, PermissionId, ProjectId, SessionId};
use oc_schema::misc_events::{PermissionEvent, PermissionReplied};
use oc_schema::permission::{Effect, Reply, Request, Rule, Ruleset, Source};
use oc_util::determinism::{Clock, IdGen};
use oc_util::wildcard;
use parking_lot::Mutex;
use tokio::sync::{broadcast, oneshot};

pub use saved::{AddSavedGrants, MemorySavedGrantStore, SavedGrant, SavedGrantStore};

/// Ruleset substituted when an agent (or its permissions) is missing
/// (`missingAgentPermissions`): deny everything.
pub fn deny_all() -> Ruleset {
    vec![Rule { action: "*".into(), resource: "*".into(), effect: Effect::Deny }]
}

/// `Permission.evaluate` — last matching rule across the flattened rulesets,
/// defaulting to `ask` when nothing matches.
///
/// A rule matches when the **action** matches `rule.action` and the
/// **resource** matches `rule.resource`, both with wildcard semantics
/// (`oc_util::wildcard`).
pub fn evaluate(action: &str, resource: &str, rulesets: &[Ruleset]) -> Rule {
    let matched = rulesets.iter().flatten().rev().find(|rule| {
        wildcard::matches(action, &rule.action) && wildcard::matches(resource, &rule.resource)
    });
    matched.cloned().unwrap_or(Rule {
        action: action.into(),
        resource: "*".into(),
        effect: Effect::Ask,
    })
}

/// `Permission.merge` — concatenate rulesets in order.
pub fn merge(rulesets: &[Ruleset]) -> Ruleset {
    rulesets.iter().flatten().cloned().collect()
}

/// `configured` — base ruleset: agent first (missing ⇒ deny-all), session
/// permissions appended so they override the agent's.
pub fn configured(agent: Option<&Ruleset>, session: Option<&Ruleset>) -> Ruleset {
    let mut base = agent.cloned().unwrap_or_else(deny_all);
    base.extend(session.cloned().unwrap_or_default());
    base
}

/// Rules mentioning the action (used for `BlockedError.rules`) — kept in order.
pub fn relevant(action: &str, rules: &Ruleset) -> Ruleset {
    rules.iter().filter(|rule| wildcard::matches(action, &rule.action)).cloned().collect()
}

/// True when ANY resource evaluates `deny` over `rules` (step 1 short-circuit).
fn denied(action: &str, resources: &[String], rules: &Ruleset) -> bool {
    resources.iter().any(|resource| {
        evaluate(action, resource, std::slice::from_ref(rules)).effect == Effect::Deny
    })
}

/// User declined without feedback.
///
/// **Defect semantics**: in v2 this is raised via `Effect.die` and deliberately
/// tunnels through blanket error mapping so it can never surface as
/// model-facing tool output. The Rust runner must treat
/// [`PermissionError::Declined`] as an interrupt of the running step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("permission declined")]
pub struct DeclinedError;

/// User declined WITH feedback — a *typed* failure the leaf converts into a
/// tool failure so the model can continue with the correction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("corrected: {feedback}")]
pub struct CorrectedError {
    pub feedback: String,
}

/// Deny verdict from evaluation. Message is `reason` if present, else
/// `Permission denied: {permission}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedError {
    /// Action-relevant rules at denial time (base ruleset only — saved grants
    /// are never consulted on the deny path).
    pub rules: Ruleset,
    pub permission: String,
    pub resources: Vec<String>,
    pub reason: Option<String>,
}

impl BlockedError {
    /// v2 `BlockedError.message`.
    pub fn message(&self) -> &str {
        self.reason.as_deref().unwrap_or("Permission denied")
    }
}

impl std::fmt::Display for BlockedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.reason {
            Some(reason) => f.write_str(reason),
            None => write!(f, "Permission denied: {}", self.permission),
        }
    }
}

impl std::error::Error for BlockedError {}

/// `reply` targeted an unknown (already-answered) request id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("permission request not found: {request_id}")]
pub struct NotFoundError {
    pub request_id: PermissionId,
}

/// Error surface of `ask`/`assert`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionError {
    /// Defect: the user (or shutdown) declined. The runner interrupts the
    /// step; this must never become tool output. See [`DeclinedError`].
    #[error("permission declined")]
    Declined(DeclinedError),
    /// Typed failure with corrective feedback for the model.
    #[error(transparent)]
    Corrected(#[from] CorrectedError),
    /// Deny verdict.
    #[error(transparent)]
    Blocked(#[from] BlockedError),
    /// The session no longer exists (`SessionErrors.NotFoundError`).
    #[error("session not found: {0}")]
    SessionNotFound(SessionId),
}

/// `Permission.AssertInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct AssertInput {
    pub id: Option<PermissionId>,
    pub session_id: SessionId,
    pub agent: Option<AgentId>,
    pub action: String,
    pub resources: Vec<String>,
    pub save: Option<Vec<String>>,
    pub metadata: Option<Record>,
    pub source: Option<Source>,
}

impl AssertInput {
    /// Minimal input for `action` over `resources`.
    pub fn new(session_id: SessionId, action: impl Into<String>, resources: Vec<String>) -> Self {
        Self {
            id: None,
            session_id,
            agent: None,
            action: action.into(),
            resources,
            save: None,
            metadata: None,
            source: None,
        }
    }
}

/// `Permission.ReplyInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyInput {
    pub request_id: PermissionId,
    pub reply: Reply,
    pub message: Option<String>,
}

/// `Permission.AskResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskResult {
    pub id: PermissionId,
    pub effect: Effect,
}

/// Output of the internal evaluation pipeline (post hook).
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    pub effect: Effect,
    pub message: Option<String>,
    /// Base rules on the deny path, base + saved grants otherwise.
    pub rules: Ruleset,
}

/// Input handed to the `permission.evaluate` hook (step 4 override point).
#[derive(Debug, Clone, PartialEq)]
pub struct HookInput {
    pub session_id: SessionId,
    pub agent: Option<AgentId>,
    pub action: String,
    pub resources: Vec<String>,
    pub metadata: Option<Record>,
    pub source: Option<Source>,
    /// Effect computed by steps 1–3.
    pub effect: Effect,
}

/// Hook verdict replacing the computed effect/message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookVerdict {
    pub effect: Effect,
    pub message: Option<String>,
}

/// Async `permission.evaluate` override. Returning `None` keeps the computed
/// verdict. Default: none.
pub type HookOverride =
    Arc<dyn Fn(HookInput) -> BoxFuture<'static, Option<HookVerdict>> + Send + Sync>;

/// Seam for the (location-scoped) session + agent ruleset lookup backing
/// `configured`. Implemented by the session-store/agent services once those
/// lanes land; tests use a fixed table.
#[async_trait]
pub trait RuleResolver: Send + Sync {
    /// `Err(SessionNotFound)` when the session is unknown. An `agent` entry of
    /// `None` means "missing agent" ⇒ deny-all base; `session` `None` ⇒ `[]`.
    async fn resolve(
        &self,
        session_id: &SessionId,
        agent: Option<&AgentId>,
    ) -> Result<(Option<Ruleset>, Option<Ruleset>), PermissionError>;
}

/// Ephemeral event sink for `permission.asked` / `permission.replied`.
///
/// TODO(bus-lane): route through the real bus publish path (location-scoped
/// ephemeral routing). This trait only carries the events the permission
/// engine emits.
pub trait EventSink: Send + Sync {
    fn publish(&self, event: PermissionEvent);
}

/// In-memory broadcast [`EventSink`] (capacity-bounded, lossy when full).
#[derive(Debug, Clone)]
pub struct BroadcastEventSink {
    tx: broadcast::Sender<PermissionEvent>,
}

impl BroadcastEventSink {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// New subscriber seeing events published after this call.
    pub fn subscribe(&self) -> broadcast::Receiver<PermissionEvent> {
        self.tx.subscribe()
    }
}

impl EventSink for BroadcastEventSink {
    fn publish(&self, event: PermissionEvent) {
        // Losing a subscriber (lag) is fine for ephemeral events.
        let _ = self.tx.send(event);
    }
}

/// Outcome delivered to a parked `assert` when its request is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingReply {
    Approved,
    Declined,
    Corrected { feedback: String },
}

impl PendingReply {
    fn reject(message: Option<&str>) -> Self {
        match message {
            Some(feedback) => Self::Corrected { feedback: feedback.to_owned() },
            None => Self::Declined,
        }
    }
}

struct Pending {
    request: Request,
    agent: Option<AgentId>,
    tx: oneshot::Sender<PendingReply>,
}

/// Ticket returned when a request is parked; the deferred `assert` awaits it.
struct PendingTicket {
    id: PermissionId,
    rx: oneshot::Receiver<PendingReply>,
}

/// The permission engine service. Clone shares the pending map.
#[derive(Clone)]
pub struct Permission {
    inner: Arc<Inner>,
}

struct Inner {
    ids: Arc<dyn IdGen>,
    clock: Arc<dyn Clock>,
    events: Arc<dyn EventSink>,
    saved: Arc<dyn SavedGrantStore>,
    resolver: Arc<dyn RuleResolver>,
    hook: Option<HookOverride>,
    project_id: ProjectId,
    /// [`BTreeMap`] keyed by ascending `per_` ids ⇒ iteration order matches
    /// insertion order (v2 `Map` semantics) for the default id generator.
    pending: Mutex<BTreeMap<PermissionId, Pending>>,
    closed: AtomicBool,
}

impl Permission {
    /// Assembles the engine for one location (saved grants are keyed by
    /// `project_id` at grant time).
    pub fn new(
        ids: Arc<dyn IdGen>,
        clock: Arc<dyn Clock>,
        events: Arc<dyn EventSink>,
        saved: Arc<dyn SavedGrantStore>,
        resolver: Arc<dyn RuleResolver>,
        project_id: ProjectId,
        hook: Option<HookOverride>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                ids,
                clock,
                events,
                saved,
                resolver,
                hook,
                project_id,
                pending: Mutex::new(BTreeMap::new()),
                closed: AtomicBool::new(false),
            }),
        }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.inner.clock.unix_ms()).unwrap_or(0)
    }

    fn next_permission_id(&self) -> PermissionId {
        PermissionId(self.inner.ids.new_id("per"))
    }

    fn publish_asked(&self, request: &Request) {
        let event = PermissionEvent::Asked(EphemeralEnvelope {
            id: EventId(self.inner.ids.new_id("evt")),
            created: self.now_ms(),
            metadata: None,
            // TODO(bus-lane): stamp the location ref when the bus lane lands.
            location: None,
            data: request.clone(),
        });
        self.inner.events.publish(event);
    }

    fn publish_replied(&self, session_id: &SessionId, request_id: &PermissionId, reply: Reply) {
        let event = PermissionEvent::Replied(EphemeralEnvelope {
            id: EventId(self.inner.ids.new_id("evt")),
            created: self.now_ms(),
            metadata: None,
            location: None,
            data: PermissionReplied {
                session_id: session_id.clone(),
                request_id: request_id.clone(),
                reply,
            },
        });
        self.inner.events.publish(event);
    }

    /// Steps 1–4 of §4a. The hook is NOT triggered on the deny short-circuit.
    async fn evaluate_input(&self, input: &AssertInput) -> Result<Evaluated, PermissionError> {
        let inner = &self.inner;
        let (agent, session) =
            inner.resolver.resolve(&input.session_id, input.agent.as_ref()).await?;
        let base = configured(agent.as_ref(), session.as_ref());
        if denied(&input.action, &input.resources, &base) {
            return Ok(Evaluated { effect: Effect::Deny, message: None, rules: base });
        }
        let grants = inner.saved.list(Some(&inner.project_id)).await;
        let all = merge(&[base, saved::rules(&grants)]);
        let effects: Vec<Effect> = input
            .resources
            .iter()
            .map(|resource| evaluate(&input.action, resource, std::slice::from_ref(&all)).effect)
            .collect();
        let effect = if effects.contains(&Effect::Ask) { Effect::Ask } else { Effect::Allow };
        let (effect, message) = match &inner.hook {
            Some(hook) => {
                match hook(HookInput {
                    session_id: input.session_id.clone(),
                    agent: input.agent.clone(),
                    action: input.action.clone(),
                    resources: input.resources.clone(),
                    metadata: input.metadata.clone(),
                    source: input.source.clone(),
                    effect,
                })
                .await
                {
                    Some(verdict) => (verdict.effect, verdict.message),
                    None => (effect, None),
                }
            }
            None => (effect, None),
        };
        Ok(Evaluated { effect, message, rules: all })
    }

    /// Materializes the wire `Request` for an input (`id` defaults to a fresh
    /// `per_` id).
    fn build_request(&self, input: &AssertInput, message: Option<String>) -> Request {
        Request {
            id: input.id.clone().unwrap_or_else(|| self.next_permission_id()),
            session_id: input.session_id.clone(),
            action: input.action.clone(),
            resources: input.resources.clone(),
            save: input.save.clone(),
            metadata: input.metadata.clone(),
            source: input.source.clone(),
            message,
        }
    }

    /// Parks a request: registers it in the pending map and publishes
    /// `permission.asked`. When the engine is closed the ticket resolves to
    /// [`DeclinedError`] immediately without registering. Duplicate pending
    /// ids are a defect (v2 `Effect.die`).
    fn create(&self, request: Request, agent: Option<AgentId>) -> PendingTicket {
        let (tx, rx) = oneshot::channel();
        let id = request.id.clone();
        if self.inner.closed.load(Ordering::Acquire) {
            let _ = tx.send(PendingReply::Declined);
            return PendingTicket { id, rx };
        }
        {
            let mut pending = self.inner.pending.lock();
            assert!(!pending.contains_key(&id), "Duplicate pending permission ID: {id}");
            pending.insert(id.clone(), Pending { request, agent, tx });
        }
        let request = self.get(&id).expect("just inserted");
        self.publish_asked(&request);
        PendingTicket { id, rx }
    }

    /// `Permission.ask` — evaluate without parking a waiter; `ask` effects are
    /// parked (and published) for the UI, deny is returned as a value.
    pub async fn ask(&self, input: AssertInput) -> Result<AskResult, PermissionError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Ok(AskResult {
                id: input.id.unwrap_or_else(|| self.next_permission_id()),
                effect: Effect::Deny,
            });
        }
        let result = self.evaluate_input(&input).await?;
        let value = self.build_request(&input, result.message.clone());
        if result.effect == Effect::Ask {
            let _ = self.create(value.clone(), input.agent.clone());
        }
        Ok(AskResult { id: value.id, effect: result.effect })
    }

    /// `Permission.assert` — allow returns, deny raises [`BlockedError`], ask
    /// parks until `reply`/`close`.
    pub async fn assert(&self, input: AssertInput) -> Result<(), PermissionError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(PermissionError::Declined(DeclinedError));
        }
        let result = self.evaluate_input(&input).await?;
        match result.effect {
            Effect::Allow => Ok(()),
            Effect::Deny => Err(BlockedError {
                rules: relevant(&input.action, &result.rules),
                permission: input.action.clone(),
                resources: input.resources.clone(),
                reason: result.message.clone(),
            }
            .into()),
            Effect::Ask => {
                let ticket = self.create(
                    self.build_request(&input, result.message.clone()),
                    input.agent.clone(),
                );
                let outcome = match ticket.rx.await {
                    Ok(outcome) => outcome,
                    // Sender dropped without answering — treat as a decline.
                    Err(_) => PendingReply::Declined,
                };
                self.inner.pending.lock().remove(&ticket.id);
                match outcome {
                    PendingReply::Approved => Ok(()),
                    PendingReply::Declined => Err(PermissionError::Declined(DeclinedError)),
                    PendingReply::Corrected { feedback } => Err(CorrectedError { feedback }.into()),
                }
            }
        }
    }

    /// Removes and returns a pending item (taking its deferred sender).
    fn take_pending(&self, id: &PermissionId) -> Option<Pending> {
        self.inner.pending.lock().remove(id)
    }

    /// `Permission.reply` — exact v2 ordering:
    ///
    /// 1. publish `permission.replied` FIRST,
    /// 2. `reject`: fail this deferred (`Corrected` with feedback, else
    ///    `Declined`) and batch-reject every **same-session** pending sibling,
    /// 3. `always` + `save`: persist the grant (project-scoped), succeed this
    ///    deferred, then batch-approve **all** remaining pending requests that
    ///    re-evaluate to allow (publishing `replied always` for each).
    pub async fn reply(&self, input: ReplyInput) -> Result<(), NotFoundError> {
        let Some(existing) = self.take_pending(&input.request_id) else {
            return Err(NotFoundError { request_id: input.request_id });
        };
        self.publish_replied(&existing.request.session_id, &existing.request.id, input.reply);

        if input.reply == Reply::Reject {
            let _ = existing.tx.send(PendingReply::reject(input.message.as_deref()));
            let session_id = existing.request.session_id.clone();
            let siblings: Vec<PermissionId> = self
                .inner
                .pending
                .lock()
                .iter()
                .filter(|(_, item)| item.request.session_id == session_id)
                .map(|(id, _)| id.clone())
                .collect();
            for id in siblings {
                if let Some(item) = self.take_pending(&id) {
                    self.publish_replied(&item.request.session_id, &item.request.id, Reply::Reject);
                    let _ = item.tx.send(PendingReply::reject(input.message.as_deref()));
                }
            }
            return Ok(());
        }

        let save = existing.request.save.clone().unwrap_or_default();
        if input.reply == Reply::Always && !save.is_empty() {
            self.inner
                .saved
                .add(&AddSavedGrants {
                    project_id: self.inner.project_id.clone(),
                    action: existing.request.action.clone(),
                    resources: save,
                })
                .await;
        }
        let _ = existing.tx.send(PendingReply::Approved);
        if input.reply != Reply::Always || existing.request.save.as_ref().is_none_or(Vec::is_empty)
        {
            return Ok(());
        }

        // Batch approve: every still-pending request that now evaluates allow.
        let ids: Vec<PermissionId> = self.inner.pending.lock().keys().cloned().collect();
        for id in ids {
            let (request, agent) = {
                let pending = self.inner.pending.lock();
                let Some(item) = pending.get(&id) else { continue };
                (item.request.clone(), item.agent.clone())
            };
            let recheck = AssertInput {
                id: Some(request.id.clone()),
                session_id: request.session_id.clone(),
                agent,
                action: request.action.clone(),
                resources: request.resources.clone(),
                save: request.save.clone(),
                metadata: request.metadata.clone(),
                source: request.source.clone(),
            };
            // Session vanished mid-flight ⇒ leave the request parked.
            let Ok(result) = self.evaluate_input(&recheck).await else { continue };
            if result.effect != Effect::Allow {
                continue;
            }
            if let Some(item) = self.take_pending(&id) {
                self.publish_replied(&item.request.session_id, &item.request.id, Reply::Always);
                let _ = item.tx.send(PendingReply::Approved);
            }
        }
        Ok(())
    }

    /// `Permission.get`.
    pub fn get(&self, id: &PermissionId) -> Option<Request> {
        self.inner.pending.lock().get(id).map(|item| item.request.clone())
    }

    /// `Permission.forSession`.
    pub fn for_session(&self, session_id: &SessionId) -> Vec<Request> {
        self.inner
            .pending
            .lock()
            .values()
            .filter(|item| item.request.session_id == *session_id)
            .map(|item| item.request.clone())
            .collect()
    }

    /// `Permission.list` — pending requests in insertion (ascending id) order.
    pub fn list(&self) -> Vec<Request> {
        self.inner.pending.lock().values().map(|item| item.request.clone()).collect()
    }

    /// `Permission.close` (shutdown): mark closed, then reject every pending
    /// request (`replied reject` + `DeclinedError`). Later `assert`s decline
    /// immediately.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let items: Vec<Pending> =
            std::mem::take(&mut *self.inner.pending.lock()).into_values().collect();
        for item in items {
            self.publish_replied(&item.request.session_id, &item.request.id, Reply::Reject);
            let _ = item.tx.send(PendingReply::Declined);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::too_many_lines, clippy::items_after_statements, clippy::assert_is_empty)]

    use super::*;
    use oc_schema::id::PermissionSavedId;
    use oc_util::determinism::{FixedClock, SequentialIdGen};
    use oc_util::global_roots::Roots;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    // ---------------------------------------------------------------- helpers

    fn sid(value: &str) -> SessionId {
        SessionId(value.to_string())
    }

    fn rule(action: &str, resource: &str, effect: Effect) -> Rule {
        Rule { action: action.into(), resource: resource.into(), effect }
    }

    fn ruleset(entries: &[(&str, &str, Effect)]) -> Ruleset {
        entries.iter().map(|(a, r, e)| rule(a, r, *e)).collect()
    }

    /// Fixed ruleset table keyed by session id: `(agent rules, session rules)`.
    struct FixedResolver {
        entries: HashMap<String, (Option<Ruleset>, Option<Ruleset>)>,
    }

    #[async_trait]
    impl RuleResolver for FixedResolver {
        async fn resolve(
            &self,
            session_id: &SessionId,
            _agent: Option<&AgentId>,
        ) -> Result<(Option<Ruleset>, Option<Ruleset>), PermissionError> {
            self.entries
                .get(session_id.as_str())
                .cloned()
                .ok_or(PermissionError::SessionNotFound(session_id.clone()))
        }
    }

    /// [`SavedGrantStore`] wrapper counting `list` calls to prove the deny
    /// short-circuit never consults saved grants.
    struct CountingStore {
        inner: MemorySavedGrantStore,
        lists: AtomicUsize,
    }

    #[async_trait]
    impl SavedGrantStore for CountingStore {
        async fn list(&self, project_id: Option<&ProjectId>) -> Vec<SavedGrant> {
            self.lists.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.list(project_id).await
        }

        async fn add(&self, input: &AddSavedGrants) {
            self.inner.add(input).await;
        }

        async fn remove(&self, id: &PermissionSavedId) {
            self.inner.remove(id).await;
        }
    }

    struct Harness {
        perm: Permission,
        saved: Arc<MemorySavedGrantStore>,
        rx: broadcast::Receiver<PermissionEvent>,
    }

    fn engine(entries: Vec<(SessionId, Option<Ruleset>, Option<Ruleset>)>) -> Harness {
        let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
        let events = BroadcastEventSink::new(256);
        let rx = events.subscribe();
        let saved = Arc::new(MemorySavedGrantStore::new(ids.clone(), clock.clone()));
        let resolver = Arc::new(FixedResolver {
            entries: entries
                .into_iter()
                .map(|(id, agent, session)| (id.0, (agent, session)))
                .collect(),
        });
        let perm = Permission::new(
            ids,
            clock,
            Arc::new(events),
            saved.clone(),
            resolver,
            ProjectId("proj_1".to_string()),
            None,
        );
        Harness { perm, saved, rx }
    }

    fn plain_engine() -> Harness {
        engine(vec![(sid("ses_1"), Some(ruleset(&[("edit", "src/*", Effect::Ask)])), None)])
    }

    fn input(action: &str, resources: &[&str]) -> AssertInput {
        AssertInput::new(sid("ses_1"), action, resources.iter().map(ToString::to_string).collect())
    }

    /// Condensed event trace: `asked:<id>:<action>` / `replied:<id>:<reply>`.
    fn snapshot(rx: &mut broadcast::Receiver<PermissionEvent>) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(event) = rx.try_recv() {
            match event {
                PermissionEvent::Asked(env) => {
                    out.push(format!("asked:{}:{}", env.data.id.0, env.data.action));
                }
                PermissionEvent::Replied(env) => {
                    out.push(format!("replied:{}:{:?}", env.data.request_id.0, env.data.reply));
                }
            }
        }
        out
    }

    /// Waits for the next `asked` event and returns its request.
    async fn wait_asked(rx: &mut broadcast::Receiver<PermissionEvent>) -> Request {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Ok(PermissionEvent::Asked(env))) => env.data,
            other => panic!("expected asked event, got {other:?}"),
        }
    }

    fn reply(
        perm: &Permission,
        request_id: PermissionId,
        reply: Reply,
        message: Option<String>,
    ) -> impl std::future::Future<Output = Result<(), NotFoundError>> + '_ {
        perm.reply(ReplyInput { request_id, reply, message })
    }

    // ------------------------------------------------------------- evaluate()

    #[test]
    fn permission_evaluate_last_match_wins_across_flattened_rulesets() {
        let first = ruleset(&[("edit", "src/*", Effect::Ask), ("edit", "*", Effect::Allow)]);
        let second = ruleset(&[("edit", "src/a.rs", Effect::Ask)]);
        // Last match overall (flattened order): the ask in the second set.
        assert_eq!(
            evaluate("edit", "src/a.rs", &[first.clone(), second.clone()]).effect,
            Effect::Ask
        );
        // Only the first set matches here; its last matching rule is allow.
        assert_eq!(evaluate("edit", "src/b.rs", &[first, second]).effect, Effect::Allow);
    }

    #[test]
    fn permission_evaluate_defaults_to_ask_when_nothing_matches() {
        let result = evaluate("edit", "src/a.rs", &[ruleset(&[("read", "*", Effect::Allow)])]);
        assert_eq!(result.effect, Effect::Ask);
        assert_eq!(result.action, "edit");
        assert_eq!(result.resource, "*");
        assert_eq!(evaluate("edit", "x", &[]).effect, Effect::Ask);
    }

    #[test]
    fn permission_evaluate_matches_action_against_rule_pattern() {
        let rules =
            ruleset(&[("bash *", "dangerous", Effect::Deny), ("bash:git *", "*", Effect::Allow)]);
        assert_eq!(
            evaluate("bash -c ls", "dangerous", std::slice::from_ref(&rules)).effect,
            Effect::Deny
        );
        assert_eq!(
            evaluate("bash", "dangerous", std::slice::from_ref(&rules)).effect,
            Effect::Deny
        );
        assert_eq!(
            evaluate("bash:git status", "any", std::slice::from_ref(&rules)).effect,
            Effect::Allow
        );
        assert_eq!(evaluate("bash:hg status", "any", &[rules]).effect, Effect::Ask);
    }

    #[test]
    fn permission_evaluate_resource_wildcard_edge_cases() {
        let rules = ruleset(&[
            ("read", "*.env", Effect::Ask),
            ("read", "a?c", Effect::Deny),
            ("read", "deep/*/file", Effect::Allow),
            ("read", "dir\\win", Effect::Deny),
            ("read", "re.gex+", Effect::Deny),
            ("read", "日本*", Effect::Deny),
        ]);
        let ev = |resource: &str| evaluate("read", resource, std::slice::from_ref(&rules)).effect;
        assert_eq!(ev(".env"), Effect::Ask); // star matches empty run
        assert_eq!(ev("nested/x.env"), Effect::Ask); // star crosses /
        assert_eq!(ev("abc"), Effect::Deny);
        assert_ne!(ev("ac"), Effect::Deny); // ? needs exactly one char
        assert_ne!(ev("abbc"), Effect::Deny);
        assert_eq!(ev("deep/a/b/file"), Effect::Allow);
        assert_eq!(ev("dir/win"), Effect::Deny); // backslash normalized
        assert_eq!(ev("re.gex+"), Effect::Deny); // regex metas literal
        assert_ne!(ev("regexx"), Effect::Deny);
        assert_eq!(ev("日本語"), Effect::Deny);
    }

    #[test]
    fn permission_evaluate_resource_pattern_allows_bare_prefix() {
        let rules = ruleset(&[("bash", "git *", Effect::Allow)]);
        assert_eq!(evaluate("bash", "git", std::slice::from_ref(&rules)).effect, Effect::Allow);
        assert_eq!(
            evaluate("bash", "git commit", std::slice::from_ref(&rules)).effect,
            Effect::Allow
        );
        assert_eq!(evaluate("bash", "gitx", &[rules]).effect, Effect::Ask);
    }

    #[test]
    fn permission_merge_concatenates_in_order() {
        let merged =
            merge(&[ruleset(&[("a", "*", Effect::Allow)]), ruleset(&[("b", "*", Effect::Deny)])]);
        assert_eq!(merged, ruleset(&[("a", "*", Effect::Allow), ("b", "*", Effect::Deny)]));
    }

    #[test]
    fn permission_configured_missing_agent_is_deny_all() {
        assert_eq!(configured(None, None), deny_all());
        let deny_all = configured(None, Some(&ruleset(&[("read", "*", Effect::Allow)])));
        // Session rules append AFTER the deny-all default, so they override.
        assert_eq!(evaluate("read", "x", &[deny_all]).effect, Effect::Allow);
        assert_eq!(evaluate("edit", "x", &[configured(None, None)]).effect, Effect::Deny);
    }

    #[test]
    fn permission_configured_session_overrides_agent() {
        let base = configured(
            Some(&ruleset(&[("edit", "*", Effect::Allow), ("read", "*", Effect::Allow)])),
            Some(&ruleset(&[("edit", "*", Effect::Ask)])),
        );
        assert_eq!(evaluate("edit", "x", std::slice::from_ref(&base)).effect, Effect::Ask);
        assert_eq!(evaluate("read", "x", &[base]).effect, Effect::Allow);
    }

    #[test]
    fn permission_relevant_filters_by_action_pattern() {
        let rules = ruleset(&[("bash *", "x", Effect::Deny), ("edit", "y", Effect::Deny)]);
        assert_eq!(relevant("bash -c ls", &rules), ruleset(&[("bash *", "x", Effect::Deny)]));
        assert_eq!(relevant("edit", &rules), ruleset(&[("edit", "y", Effect::Deny)]));
        assert!(relevant("read", &rules).is_empty());
    }

    // ------------------------------------------------------- evaluate_input()

    #[tokio::test]
    async fn permission_evaluate_input_precedence_table() {
        // agent allow < session ask.
        let h = engine(vec![(
            sid("ses_1"),
            Some(ruleset(&[("edit", "src/*", Effect::Allow)])),
            Some(ruleset(&[("edit", "src/*", Effect::Ask)])),
        )]);
        assert_eq!(h.perm.ask(input("edit", &["src/a.rs"])).await.unwrap().effect, Effect::Ask);

        // agent ask < saved allow.
        let h = plain_engine();
        h.saved
            .add(&AddSavedGrants {
                project_id: ProjectId("proj_1".into()),
                action: "edit".into(),
                resources: vec!["src/*".into()],
            })
            .await;
        assert_eq!(h.perm.ask(input("edit", &["src/a.rs"])).await.unwrap().effect, Effect::Allow);

        // session deny > saved allow (deny short-circuit over base).
        let h = engine(vec![(
            sid("ses_1"),
            Some(ruleset(&[("edit", "*", Effect::Allow)])),
            Some(ruleset(&[("edit", "src/secret.rs", Effect::Deny)])),
        )]);
        h.saved
            .add(&AddSavedGrants {
                project_id: ProjectId("proj_1".into()),
                action: "edit".into(),
                resources: vec!["*".into()],
            })
            .await;
        let err = h.perm.assert(input("edit", &["src/secret.rs"])).await.unwrap_err();
        assert!(matches!(err, PermissionError::Blocked(_)));

        // ANY resource denying over base ⇒ whole request denies.
        let h = engine(vec![(
            sid("ses_1"),
            Some(ruleset(&[("edit", "*", Effect::Allow), ("edit", "src/lock.rs", Effect::Deny)])),
            None,
        )]);
        let err = h.perm.assert(input("edit", &["src/a.rs", "src/lock.rs"])).await.unwrap_err();
        assert!(matches!(err, PermissionError::Blocked(_)));

        // any ask among resources ⇒ ask.
        let h = engine(vec![(
            sid("ses_1"),
            Some(ruleset(&[("edit", "*", Effect::Allow), ("edit", "src/ask.rs", Effect::Ask)])),
            None,
        )]);
        assert_eq!(
            h.perm.ask(input("edit", &["src/a.rs", "src/ask.rs"])).await.unwrap().effect,
            Effect::Ask
        );

        // empty resources ⇒ allow (no ask among none).
        assert_eq!(h.perm.ask(input("edit", &[])).await.unwrap().effect, Effect::Allow);
    }

    #[tokio::test]
    async fn permission_evaluate_input_base_deny_skips_saved_lookup() {
        let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
        let events = Arc::new(BroadcastEventSink::new(16));
        let saved = Arc::new(CountingStore {
            inner: MemorySavedGrantStore::new(ids.clone(), clock.clone()),
            lists: AtomicUsize::new(0),
        });
        let resolver = Arc::new(FixedResolver {
            entries: [(sid("ses_1").0, (Some(ruleset(&[("*", "*", Effect::Deny)])), None))]
                .into_iter()
                .collect(),
        });
        let perm = Permission::new(
            ids,
            clock,
            events,
            saved.clone(),
            resolver,
            ProjectId("p".into()),
            None,
        );

        let err = perm.assert(input("edit", &["x"])).await.unwrap_err();
        assert!(matches!(err, PermissionError::Blocked(_)));
        assert_eq!(saved.lists.load(std::sync::atomic::Ordering::SeqCst), 0);

        perm.assert(input("nope", &["x"])).await.ok(); // still deny, still no lookup
        assert_eq!(saved.lists.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn permission_evaluate_input_hook_overrides_effect_and_message() {
        let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
        let events = Arc::new(BroadcastEventSink::new(16));
        let saved = Arc::new(MemorySavedGrantStore::new(ids.clone(), clock.clone()));
        let resolver = Arc::new(FixedResolver {
            entries: [(sid("ses_1").0, (Some(ruleset(&[("edit", "*", Effect::Ask)])), None))]
                .into_iter()
                .collect(),
        });
        let hook: HookOverride = Arc::new(|mut input: HookInput| {
            Box::pin(async move {
                assert_eq!(input.effect, Effect::Ask); // computed effect passed in
                input.effect = Effect::Deny;
                Some(HookVerdict { effect: input.effect, message: Some("classified".into()) })
            })
        });
        let perm =
            Permission::new(ids, clock, events, saved, resolver, ProjectId("p".into()), Some(hook));

        let err = perm.assert(input("edit", &["x"])).await.unwrap_err();
        match err {
            PermissionError::Blocked(blocked) => {
                assert_eq!(blocked.reason.as_deref(), Some("classified"));
                assert_eq!(blocked.to_string(), "classified");
            }
            other => panic!("expected blocked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn permission_evaluate_input_no_hook_on_deny_short_circuit() {
        use std::sync::atomic::AtomicUsize;
        let calls = Arc::new(AtomicUsize::new(0));
        let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
        let events = Arc::new(BroadcastEventSink::new(16));
        let saved = Arc::new(MemorySavedGrantStore::new(ids.clone(), clock.clone()));
        let resolver = Arc::new(FixedResolver {
            entries: [(sid("ses_1").0, (Some(ruleset(&[("*", "*", Effect::Deny)])), None))]
                .into_iter()
                .collect(),
        });
        let counter = calls.clone();
        let hook: HookOverride = Arc::new(move |_input: HookInput| {
            let counter = counter.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                None
            })
        });
        let perm =
            Permission::new(ids, clock, events, saved, resolver, ProjectId("p".into()), Some(hook));

        assert!(matches!(
            perm.assert(input("edit", &["x"])).await.unwrap_err(),
            PermissionError::Blocked(_)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn permission_evaluate_input_unknown_session_not_found() {
        let h = plain_engine();
        let err =
            h.perm.ask(AssertInput::new(sid("ses_missing"), "edit", vec![])).await.unwrap_err();
        assert!(matches!(err, PermissionError::SessionNotFound(ref id) if id.0 == "ses_missing"));
    }

    // ------------------------------------------------------ assert / ask flow

    #[tokio::test]
    async fn permission_assert_allow_returns() {
        let h = engine(vec![(sid("ses_1"), Some(ruleset(&[("*", "*", Effect::Allow)])), None)]);
        h.perm.assert(input("edit", &["a", "b"])).await.unwrap();
        assert!(h.perm.list().is_empty());
    }

    #[tokio::test]
    async fn permission_assert_deny_blocked_error_fields() {
        let h = engine(vec![(
            sid("ses_1"),
            Some(ruleset(&[("bash *", "/tmp/*", Effect::Deny), ("edit", "*", Effect::Allow)])),
            None,
        )]);
        let err = h
            .perm
            .assert(AssertInput {
                save: None,
                metadata: None,
                source: None,
                agent: None,
                id: None,
                ..input("bash rm -rf", &["/tmp/x"])
            })
            .await
            .unwrap_err();
        match err {
            PermissionError::Blocked(blocked) => {
                assert_eq!(blocked.permission, "bash rm -rf");
                assert_eq!(blocked.resources, vec!["/tmp/x".to_string()]);
                assert_eq!(blocked.reason, None);
                assert_eq!(blocked.to_string(), "Permission denied: bash rm -rf");
                // Only action-relevant rules are reported.
                assert_eq!(blocked.rules, ruleset(&[("bash *", "/tmp/*", Effect::Deny)]));
            }
            other => panic!("expected blocked, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn permission_assert_ask_parks_until_reply_once() {
        let mut h = plain_engine();
        let spawned = tokio::spawn({
            let perm = h.perm.clone();
            let input = input("edit", &["src/b.rs"]);
            async move { perm.assert(input).await }
        });

        // The spawned assert parks first (per_1)…
        let asked = wait_asked(&mut h.rx).await;
        assert_eq!(asked.id.0, "per_1");
        // …then an ask() parks a second request with no waiter (ids share the
        // generator with evt_ ids, so only assert distinctness).
        let second = h.perm.ask(input("edit", &["src/a.rs"])).await.unwrap();
        assert_eq!(second.effect, Effect::Ask);
        assert_ne!(second.id, asked.id);

        // Parked: no timeout, no auto-resolution.
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!spawned.is_finished());
        assert_eq!(h.perm.list().len(), 2);

        reply(&h.perm, asked.id.clone(), Reply::Once, None).await.unwrap();
        spawned.await.unwrap().unwrap();
        assert_eq!(h.perm.list().len(), 1); // only the ask() one remains
        assert_eq!(h.perm.list()[0].id, second.id);
    }

    #[tokio::test]
    async fn permission_assert_closed_declines_as_defect() {
        let h = plain_engine();
        h.perm.close();
        let err = h.perm.assert(input("edit", &["src/a.rs"])).await.unwrap_err();
        assert!(matches!(err, PermissionError::Declined(DeclinedError)));
    }

    #[tokio::test]
    async fn permission_ask_closed_returns_deny_without_parking() {
        let h = plain_engine();
        h.perm.close();
        let result = h
            .perm
            .ask(AssertInput {
                id: Some(PermissionId("per_fixed".into())),
                ..input("edit", &["src/a.rs"])
            })
            .await
            .unwrap();
        assert_eq!(result.id.0, "per_fixed");
        assert_eq!(result.effect, Effect::Deny);
        assert!(h.perm.list().is_empty());
    }

    #[tokio::test]
    async fn permission_ask_parks_and_publishes_asked_event() {
        let mut h = plain_engine();
        let input = AssertInput {
            save: Some(vec!["src/*".into()]),
            metadata: None,
            source: Some(Source::Tool { message_id: "msg_1".into(), id: "call_1".into() }),
            agent: None,
            id: None,
            ..input("edit", &["src/a.rs", "src/b.rs"])
        };
        let result = h.perm.ask(input).await.unwrap();
        assert_eq!(result.effect, Effect::Ask);

        let asked = wait_asked(&mut h.rx).await;
        assert_eq!(asked.id, result.id);
        assert_eq!(asked.session_id, sid("ses_1"));
        assert_eq!(asked.action, "edit");
        assert_eq!(asked.resources, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
        assert_eq!(asked.save, Some(vec!["src/*".to_string()]));
        assert_eq!(
            asked.source,
            Some(Source::Tool { message_id: "msg_1".into(), id: "call_1".into() })
        );
        assert_eq!(h.perm.get(&result.id).as_ref(), Some(&asked));
        assert_eq!(h.perm.for_session(&sid("ses_1")).len(), 1);
    }

    #[tokio::test]
    async fn permission_reply_unknown_request_not_found() {
        let h = plain_engine();
        let err =
            reply(&h.perm, PermissionId("per_nope".into()), Reply::Once, None).await.unwrap_err();
        assert_eq!(err.request_id.0, "per_nope");
    }

    #[tokio::test]
    async fn permission_reply_reject_batch_rejects_same_session_only() {
        let mut h = engine(vec![
            (sid("ses_1"), Some(ruleset(&[("*", "*", Effect::Ask)])), None),
            (sid("ses_2"), Some(ruleset(&[("*", "*", Effect::Ask)])), None),
        ]);
        // Deterministic target: an ask()-parked request in ses_1 (per_1).
        let target = h.perm.ask(input("edit", &["a"])).await.unwrap();
        let b = tokio::spawn({
            let perm = h.perm.clone();
            let input = AssertInput::new(sid("ses_1"), "edit", vec!["b".into()]);
            async move { perm.assert(input).await }
        });
        let c = tokio::spawn({
            let perm = h.perm.clone();
            let input = AssertInput::new(sid("ses_2"), "edit", vec!["c".into()]);
            async move { perm.assert(input).await }
        });

        wait_asked(&mut h.rx).await;
        wait_asked(&mut h.rx).await;
        wait_asked(&mut h.rx).await;

        reply(&h.perm, target.id.clone(), Reply::Reject, None).await.unwrap();

        // Same-session sibling rejected; other session stays parked.
        assert!(matches!(b.await.unwrap().unwrap_err(), PermissionError::Declined(DeclinedError)));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!c.is_finished());
        assert_eq!(h.perm.list().len(), 1);
        assert_eq!(h.perm.list()[0].session_id, sid("ses_2"));

        // replied published first, then the batch sibling — never ses_2's.
        let trace = snapshot(&mut h.rx);
        assert!(trace.contains(&format!("replied:{}:Reject", target.id.0)));
        assert_eq!(trace.iter().filter(|line| line.starts_with("replied:")).count(), 2);
        let other_session_id = h.perm.list()[0].id.0.clone();
        assert!(trace.iter().all(|line| !line.starts_with(&format!("replied:{other_session_id}"))));
    }

    #[tokio::test]
    async fn permission_reply_reject_event_ordering() {
        let mut h = engine(vec![(sid("ses_1"), Some(ruleset(&[("*", "*", Effect::Ask)])), None)]);
        let a = h.perm.ask(input("edit", &["a"])).await.unwrap();
        let b = h.perm.ask(input("edit", &["b"])).await.unwrap();

        reply(&h.perm, a.id.clone(), Reply::Reject, None).await.unwrap();

        let trace = snapshot(&mut h.rx);
        assert_eq!(
            trace,
            vec![
                format!("asked:{}:edit", a.id.0),
                format!("asked:{}:edit", b.id.0),
                format!("replied:{}:Reject", a.id.0),
                format!("replied:{}:Reject", b.id.0),
            ]
        );
        assert!(h.perm.list().is_empty());
    }

    #[tokio::test]
    async fn permission_reply_reject_with_message_corrects() {
        let mut h = plain_engine();
        let first = h.perm.ask(input("edit", &["src/a.rs"])).await.unwrap();
        let second = tokio::spawn({
            let perm = h.perm.clone();
            let input = input("edit", &["src/b.rs"]);
            async move { perm.assert(input).await }
        });
        wait_asked(&mut h.rx).await;
        wait_asked(&mut h.rx).await;

        reply(&h.perm, first.id, Reply::Reject, Some("use the formatter".into())).await.unwrap();

        match second.await.unwrap().unwrap_err() {
            PermissionError::Corrected(err) => assert_eq!(err.feedback, "use the formatter"),
            other => panic!("expected corrected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn permission_reply_always_saves_grant_and_batch_approves() {
        let mut h = plain_engine();
        let first = tokio::spawn({
            let perm = h.perm.clone();
            let input =
                AssertInput { save: Some(vec!["src/*".into()]), ..input("edit", &["src/a.rs"]) };
            async move { perm.assert(input).await }
        });
        let second = tokio::spawn({
            let perm = h.perm.clone();
            let input = input("edit", &["src/b.rs"]);
            async move { perm.assert(input).await }
        });
        let asked = wait_asked(&mut h.rx).await;
        wait_asked(&mut h.rx).await;

        reply(&h.perm, asked.id.clone(), Reply::Always, None).await.unwrap();

        first.await.unwrap().unwrap();
        // Sibling re-evaluated against the persisted grant ⇒ auto-approved.
        second.await.unwrap().unwrap();

        let grants = h.saved.list(Some(&ProjectId("proj_1".into()))).await;
        assert_eq!(saved::rules(&grants), ruleset(&[("edit", "src/*", Effect::Allow)]));
        // Both the answered request and the batch-approved sibling get replies.
        assert!(h.perm.list().is_empty()); // batch-approve drained the map
        let trace = snapshot(&mut h.rx);
        assert_eq!(trace.len(), 2);
        assert!(trace.iter().all(|line| line.ends_with(":Always")));

        // Subsequent asserts are allowed without asking.
        h.perm.assert(input("edit", &["src/c.rs"])).await.unwrap();
    }

    #[tokio::test]
    async fn permission_reply_always_without_save_skips_grant_and_batch() {
        let h = plain_engine();
        let first = h.perm.ask(input("edit", &["src/a.rs"])).await.unwrap();
        let _second = h.perm.ask(input("edit", &["src/b.rs"])).await.unwrap();

        reply(&h.perm, first.id, Reply::Always, None).await.unwrap();

        assert!(h.saved.list(Some(&ProjectId("proj_1".into()))).await.is_empty());
        // Sibling untouched: `always` without save has nothing to re-evaluate.
        assert_eq!(h.perm.list().len(), 1);
    }

    #[tokio::test]
    async fn permission_reply_once_leaves_siblings_pending() {
        let h = plain_engine();
        let first = h.perm.ask(input("edit", &["src/a.rs"])).await.unwrap();
        let second = h.perm.ask(input("edit", &["src/b.rs"])).await.unwrap();

        reply(&h.perm, first.id, Reply::Once, None).await.unwrap();

        assert_eq!(h.perm.list().len(), 1);
        assert_eq!(h.perm.list()[0].id, second.id);
    }

    #[tokio::test]
    async fn permission_close_rejects_all_pending() {
        let mut h = plain_engine();
        let a = tokio::spawn({
            let perm = h.perm.clone();
            let input = input("edit", &["src/a.rs"]);
            async move { perm.assert(input).await }
        });
        let b = tokio::spawn({
            let perm = h.perm.clone();
            let input = input("edit", &["src/b.rs"]);
            async move { perm.assert(input).await }
        });
        wait_asked(&mut h.rx).await;
        wait_asked(&mut h.rx).await;

        h.perm.close();

        assert!(matches!(a.await.unwrap().unwrap_err(), PermissionError::Declined(DeclinedError)));
        assert!(matches!(b.await.unwrap().unwrap_err(), PermissionError::Declined(DeclinedError)));
        assert!(h.perm.list().is_empty());
        let trace = snapshot(&mut h.rx);
        assert_eq!(trace.iter().filter(|line| line.starts_with("replied:")).count(), 2);
        assert!(trace.iter().all(|line| line.ends_with(":Reject")));
    }

    #[tokio::test]
    async fn permission_duplicate_pending_id_is_defect() {
        let h = plain_engine();
        let input = AssertInput {
            id: Some(PermissionId("per_dup".into())),
            ..input("edit", &["src/a.rs"])
        };
        h.perm.ask(input.clone()).await.unwrap();
        let result =
            futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(h.perm.ask(input))).await;
        assert!(result.is_err());
    }

    // ------------------------------------------------------------ saved store

    #[tokio::test]
    async fn permission_saved_store_roundtrip_and_uniqueness() {
        let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
        let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
        let store = MemorySavedGrantStore::new(ids, clock);

        store
            .add(&AddSavedGrants {
                project_id: ProjectId("p1".into()),
                action: "edit".into(),
                resources: vec!["src/*".into(), "doc/*".into()],
            })
            .await;
        // Conflict-do-nothing: same (project, action, resource) kept as-is.
        store
            .add(&AddSavedGrants {
                project_id: ProjectId("p1".into()),
                action: "edit".into(),
                resources: vec!["src/*".into()],
            })
            .await;
        // Empty resources is a no-op.
        store
            .add(&AddSavedGrants {
                project_id: ProjectId("p1".into()),
                action: "edit".into(),
                resources: vec![],
            })
            .await;
        store
            .add(&AddSavedGrants {
                project_id: ProjectId("p2".into()),
                action: "read".into(),
                resources: vec!["x".into()],
            })
            .await;

        let p1 = store.list(Some(&ProjectId("p1".into()))).await;
        assert_eq!(p1.len(), 2);
        let mut resources: Vec<&str> = p1.iter().map(|grant| grant.resource.as_str()).collect();
        resources.sort_unstable();
        assert_eq!(resources, vec!["doc/*", "src/*"]);
        assert!(p1.iter().all(|grant| grant.project_id.0 == "p1"));
        assert_eq!(store.list(None).await.len(), 3);

        let id = p1[0].id.clone();
        store.remove(&id).await;
        assert_eq!(store.list(Some(&ProjectId("p1".into()))).await.len(), 1);
    }

    #[test]
    fn permission_saved_rules_read_back_as_allow() {
        let grants = vec![SavedGrant {
            id: PermissionSavedId("psv_1".into()),
            project_id: ProjectId("p".into()),
            action: "edit".into(),
            resource: "src/*".into(),
            time_created: 0,
            time_updated: 0,
        }];
        assert_eq!(saved::rules(&grants), ruleset(&[("edit", "src/*", Effect::Allow)]));
    }

    #[test]
    fn permission_default_agent_external_allows_shape() {
        let roots = Roots {
            home: PathBuf::from("/home/u"),
            data: PathBuf::from("/data/opencode"),
            cache: PathBuf::from("/cache/opencode"),
            config: PathBuf::from("/cfg/opencode"),
            state: PathBuf::from("/state/opencode"),
            tmp: PathBuf::from("/tmp/opencode"),
        };
        let rules = crate::file_access::default_agent_external_allows(&roots);
        let resources: Vec<&str> = rules.iter().map(|rule| rule.resource.as_str()).collect();
        assert_eq!(
            resources,
            vec![
                "/data/opencode/shell/*/*",
                "/data/opencode/tool-output/*",
                "/tmp/opencode/*",
                "/cfg/opencode/*",
            ]
        );
        assert!(rules
            .iter()
            .all(|rule| { rule.action == "external_directory" && rule.effect == Effect::Allow }));
    }
}
