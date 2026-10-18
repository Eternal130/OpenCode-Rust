//! `SessionPrompt.prepare` (core/session/prompt.ts:38-84) — the
//! production `session.prompt` hook dispatch + `@skill` resolution
//! seam, ported to the Rust consume boundary.
//!
//! v2 fires the hook at prompt-submission time, before the inbox row
//! exists; the Rust HTTP handler admits the raw wire payload (the
//! handler is WIRE-owned), so the runner fires the seam at the Ready
//! boundary — AFTER admission, BEFORE the row is delivered/projected
//! and before the model turn. Once per pending user row (the drain
//! tracks prepared inbox ids), with the prepared payload persisted
//! back into the pending row so the `session.inbox.delivered`
//! projector materializes the PREPARED user message (hook-mutated
//! text/files/agents/metadata + resolved `skill.text`), matching the
//! v2 observable: the stored user message and the model turn both see
//! the prepared prompt.
//!
//! Ordering mirrors prompt.ts: the hook FIRST (its output feeds skill
//! resolution — a hook may attach skills), then `Skill.prepare` per
//! requested attachment, unknown skill ⇒ error (the prompt fails, the
//! poisoned row is cancelled — v2 never admits it).
//!
//! Mutation semantics per ADR-0005: copy-in (payload cloned into the
//! event), dispatch (`&mut` chain in-process, copy-in/copy-out across
//! the guest bridge), copy-out (the shaped event rewrites the row).

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use oc_schema::id::SessionId;
use oc_schema::prompt::Prompt;
use oc_schema::session_inbox::{Delivery, UserPayload};
use parking_lot::Mutex;

use crate::database::error::DbError;
use crate::database::Database;
use crate::hooks::{HookError, HookRegistry, SessionPromptEvent};
use crate::tool::skill::{prompt_text, SkillTable};

/// One pending `user` row as loaded from the inbox table.
#[derive(Debug)]
struct PendingUser {
    id: String,
    payload: UserPayload,
    delivery: Delivery,
}

/// The once-per-prompt id ring (bounded FIFO like the TUI's replay
/// window): an inbox id is prepared exactly once for as long as the
/// ring remembers it — a drain retry after a boundary bounce must not
/// re-fire the hook on an already-prepared row (double mutation).
#[derive(Default)]
struct PreparedRing {
    order: VecDeque<String>,
    set: HashSet<String>,
}

impl PreparedRing {
    const CAP: usize = 4096;

    fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }

    fn mark(&mut self, id: &str) {
        if self.set.insert(id.to_owned()) {
            self.order.push_back(id.to_owned());
        }
        while self.order.len() > Self::CAP {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
    }
}

/// `SkillNotFoundError` / hook failure surfaced by the prepare seam.
#[derive(Debug, thiserror::Error)]
pub enum PromptPrepareError {
    /// A `session.prompt` handler rejected the prompt.
    #[error("session.prompt hook failed: {0}")]
    Hook(#[from] HookError),
    /// The prompt attached an unknown skill (prompt.ts `SkillNotFoundError`).
    #[error("skill not found: {0}")]
    SkillNotFound(String),
    /// The pending-row read/write failed.
    #[error(transparent)]
    Db(#[from] DbError),
    /// The stored payload did not decode.
    #[error("inbox payload decode failed: {0}")]
    Decode(#[from] serde_json::Error),
    /// The skill body could not be read.
    #[error("skill prepare failed: {0}")]
    Io(#[from] std::io::Error),
}

/// The prepare service: the location's hook registry + skill table
/// over the shared database (the engine attaches it; the runner's
/// default is [`PromptPrepare::disabled`] — a no-op).
pub struct PromptPrepare {
    db: Database,
    hooks: Option<Arc<HookRegistry>>,
    skills: Option<Arc<dyn SkillTable>>,
    /// The once-per-prompt ring (see [`PreparedRing`]).
    prepared: Mutex<PreparedRing>,
}

impl PromptPrepare {
    /// The full seam (the engine's attachment).
    #[must_use]
    pub fn new(db: Database, hooks: Arc<HookRegistry>, skills: Arc<dyn SkillTable>) -> Self {
        Self { db, hooks: Some(hooks), skills: Some(skills), prepared: Mutex::default() }
    }

    /// The no-op seam (no hooks, no skills — the pre-attachment
    /// default and the deterministic-stack base).
    #[must_use]
    pub fn disabled(db: Database) -> Self {
        Self { db, hooks: None, skills: None, prepared: Mutex::default() }
    }

    /// Whether any stage is armed (the drain skips the read entirely
    /// when nothing can shape a prompt).
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.hooks.is_some() || self.skills.is_some()
    }

    /// Reads the pending `user` rows for `session_id` (steers and
    /// queued, enqueue order — the same rows the promote path walks).
    async fn pending_users(
        db: &Database,
        session_id: &str,
    ) -> Result<Vec<PendingUser>, PromptPrepareError> {
        let session_id = session_id.to_string();
        let rows: Vec<(String, String, String)> = db
            .transaction(move |tx| {
                let mut statement = tx.conn().prepare(
                    "SELECT id, payload, delivery FROM session_inbox \
                     WHERE session_id = ?1 AND type = 'user' ORDER BY enqueued_seq",
                )?;
                let rows = statement
                    .query_map(rusqlite::params![session_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter()
            .map(|(id, payload, delivery)| {
                let delivery = match delivery.as_str() {
                    "queue" => Delivery::Queue,
                    _ => Delivery::Steer,
                };
                Ok(PendingUser { id, payload: serde_json::from_str(&payload)?, delivery })
            })
            .collect()
    }

    /// Persists a prepared row back into the pending table: the
    /// `session.inbox.delivered` projector reads the row when the
    /// promote path delivers it, so the projected user message (and
    /// through it the model turn) carries the prepared payload.
    async fn persist(
        db: &Database,
        session_id: &str,
        id: &str,
        payload: &UserPayload,
        delivery: Delivery,
    ) -> Result<(), PromptPrepareError> {
        let encoded = serde_json::to_string(payload)?;
        let delivery = match delivery {
            Delivery::Steer => "steer",
            Delivery::Queue => "queue",
        };
        let (session_id, id) = (session_id.to_string(), id.to_string());
        db.transaction(move |tx| {
            tx.execute(
                "UPDATE session_inbox SET payload = ?1, delivery = ?2 \
                 WHERE id = ?3 AND session_id = ?4",
                rusqlite::params![encoded, delivery, id, session_id],
            )?;
            Ok(())
        })
        .await?;
        Ok(())
    }

    /// Prepares one pending row: hook dispatch (copy-in/copy-out),
    /// then skill resolution over the (possibly hook-shaped) request.
    async fn prepare_one(
        &self,
        session_id: &SessionId,
        row: PendingUser,
    ) -> Result<(UserPayload, Delivery), PromptPrepareError> {
        let mut payload = row.payload;
        let mut delivery = row.delivery;

        // 1. `hooks.trigger("session", "prompt", ...)` (prompt.ts:38-51).
        if let Some(hooks) = &self.hooks {
            let event = SessionPromptEvent {
                session_id: session_id.clone(),
                message_id: oc_schema::id::MessageId::parse(row.id.clone())
                    .unwrap_or_else(|error| oc_schema::id::MessageId(error.value)),
                prompt: Prompt {
                    text: payload.text.clone(),
                    files: payload.files.clone(),
                    agents: payload.agents.clone(),
                    skills: payload.skills.clone(),
                },
                metadata: payload.metadata.clone().map(|record| serde_json::Map::from_iter(record)),
                delivery,
            };
            let shaped = hooks.dispatch_session_prompt(event).await?;
            // Copy-out: the shaped prompt/metadata/delivery become the
            // admitted item (v2 admits `event.prompt`'s fields).
            payload.text = shaped.prompt.text;
            payload.files = shaped.prompt.files;
            payload.agents = shaped.prompt.agents;
            payload.skills = shaped.prompt.skills;
            payload.metadata = shaped
                .metadata
                .map(|record| record.into_iter().collect::<oc_schema::common::Record>());
            delivery = shaped.delivery;
        }

        // 2. Skill resolution (prompt.ts:56-76): every requested
        // attachment resolves against the engine table; the prepared
        // `Skill.prepare().output` becomes `skill.text`. Unknown id ⇒
        // error (the prompt fails, never a silent drop). A repeated id
        // reuses the first resolution (v2's `prepared` map).
        if let Some(skills) = &self.skills {
            let requested = payload.skills.take().unwrap_or_default();
            if !requested.is_empty() {
                let mut resolved: Vec<oc_schema::prompt::SkillAttachment> =
                    Vec::with_capacity(requested.len());
                let mut cache: Vec<(String, oc_schema::id::SkillName)> = Vec::new();
                for mut attachment in requested {
                    if let Some((_, name)) =
                        cache.iter().find(|(id, _)| *id == attachment.id.as_str())
                    {
                        attachment.name = name.clone();
                        attachment.text = None;
                        resolved.push(attachment);
                        continue;
                    }
                    let Some(skill) = skills.get(attachment.id.as_str()) else {
                        return Err(PromptPrepareError::SkillNotFound(
                            attachment.id.as_str().to_string(),
                        ));
                    };
                    attachment.name = skill.name.clone();
                    attachment.text = Some(prompt_text(&skill)?);
                    cache.push((skill.id.as_str().to_string(), skill.name.clone()));
                    resolved.push(attachment);
                }
                payload.skills = Some(resolved);
            }
        }
        Ok((payload, delivery))
    }

    /// Prepares every not-yet-prepared pending user row of
    /// `session_id` and persists the shaped payloads. Each row fires
    /// the seam exactly once (the [`PreparedRing`] guards drain
    /// retries). The first failing row aborts the batch and names the
    /// row — the drain cancels it (v2 never admits a prompt whose
    /// prepare failed) and surfaces the error.
    pub async fn prepare_pending(
        &self,
        session_id: &SessionId,
    ) -> Result<(), (String, PromptPrepareError)> {
        let pending = Self::pending_users(&self.db, session_id.as_str())
            .await
            .map_err(|error| (session_id.as_str().to_string(), error))?;
        for row in pending {
            if self.prepared.lock().contains(&row.id) {
                continue;
            }
            let id = row.id.clone();
            match self.prepare_one(session_id, row).await {
                Ok((payload, delivery)) => {
                    // The hook fired — the ring remembers before the
                    // persist (a persist failure must not re-fire it).
                    self.prepared.lock().mark(&id);
                    if let Err(error) =
                        Self::persist(&self.db, session_id.as_str(), &id, &payload, delivery).await
                    {
                        return Err((id, error));
                    }
                }
                Err(error) => {
                    // A dispatched hook that failed still fired: mark,
                    // then surface (the drain cancels the row).
                    self.prepared.lock().mark(&id);
                    return Err((id, error));
                }
            }
        }
        Ok(())
    }
}
