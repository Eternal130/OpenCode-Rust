//! Port of `session/projector.ts` (projector.ts:434-788) — every
//! `bus.project` registration plus the `session.usage.updated` fan-out
//! subscription.
//!
//! Handlers run inside the durable-commit transaction with `&mut Tx`
//! (persist-then-publish contract, `bus.rs`). Defects (duplicate
//! projections, inbox lifecycle conflicts) tunnel through the typed
//! [`DbError::SessionConflict`] variants so publishers can catch them:
//! [`crate::database::error::SessionConflict::AlreadyProjected`] for
//! `session.created`/`session.forked` races (→ store re-read),
//! [`crate::database::error::SessionConflict::InboxConflict`] for inbox
//! lifecycle conflicts.

#![allow(
    clippy::too_many_lines, // install_projectors is one registration table
    clippy::must_use_candidate,
    clippy::redundant_closure,
    clippy::cast_possible_truncation, // epoch-ms columns are i64
    clippy::let_and_return,
)]

use oc_schema::id::{AgentId, MessageId, ProjectId};
use oc_schema::session_message;
use oc_schema::session_support::{InstructionDeltaValue, InstructionValues};
use std::sync::Arc;

use rusqlite::OptionalExtension;

use crate::bus::{Bus, DurableEvent, EventDefinition, Listener};
use crate::database::error::{DbError, Result};
use crate::database::schema::{
    encode_json, InstructionEntryRow, InstructionStateRow, SessionMessageRow, SessionRow,
};
use crate::database::Tx;
use crate::session::inbox::{
    project_admitted, project_cancelled, project_delivered, project_delivery_changed,
};
use crate::session::message_updater::{
    decode_row, Adapter, PreviousLocation, SessionMessageUpdater,
};
use futures::FutureExt as _;

/// The typed defect (`Effect.die(new SessionAlreadyProjected())`).
#[derive(Debug, thiserror::Error)]
#[error("session already projected")]
pub struct SessionAlreadyProjected;

/// Raises the already-projected conflict from inside a projector
/// transaction (tunneled out as [`DbError::SessionConflict`]).
fn tx_defect(detail: &str) -> DbError {
    DbError::SessionConflict(crate::database::error::SessionConflict::AlreadyProjected(
        detail.to_string(),
    ))
}

/// Detects the already-projected defect on a failed publish.
pub fn is_already_projected(err: &crate::bus::BusError) -> bool {
    matches!(
        err,
        crate::bus::BusError::Db(DbError::SessionConflict(
            crate::database::error::SessionConflict::AlreadyProjected(_)
        ))
    )
}

/// The fork copy batch size (projector.ts:49).
const FORK_BATCH_SIZE: i64 = 500;

/// `forkTitle` — `X (fork #N)` pattern (projector.ts:51-56).
#[must_use]
pub fn fork_title(value: Option<&str>) -> Option<String> {
    let value = value?;
    if let Some(index) = value.rfind(" (fork #") {
        if let Some(close) = value[index + 8..].strip_suffix(')') {
            if let Ok(number) = close.parse::<u64>() {
                return Some(format!("{} (fork #{})", &value[..index], number + 1));
            }
        }
    }
    Some(format!("{value} (fork #1)"))
}

fn definition(type_: &str) -> &'static EventDefinition {
    crate::bus::registry::by_type_latest(type_)
        .unwrap_or_else(|| panic!("no durable definition for {type_}"))
}

/// Registers every session projector on the bus
/// (`SessionProjector.layer`).
pub fn install_projectors(bus: &Bus) {
    install(bus, "session.created", |tx, event| {
        let data = event.data.clone();
        let session_id = data["sessionID"].as_str().unwrap_or_default().to_string();
        let workspace_id = data["location"]["workspaceID"].as_str().map(str::to_string);
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO session_v2 \
             (id, project_id, workspace_id, parent_id, fork_session_id, fork_boundary, slug, \
              directory, path, title, version, metadata, permission, agent, model, \
              time_created, time_updated) \
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
            rusqlite::params![
                session_id,
                data["projectID"].as_str().unwrap_or_default(),
                workspace_id,
                data["parentID"].as_str(),
                data["slug"].as_str().unwrap_or_default(),
                data["location"]["directory"].as_str().unwrap_or_default(),
                data["subpath"].as_str(),
                data["title"].as_str(),
                data["version"].as_str().unwrap_or_default(),
                (!data["metadata"].is_null()).then(|| encode_json(&data["metadata"])),
                (!data["permissions"].is_null()).then(|| encode_json(&data["permissions"])),
                data["agent"].as_str(),
                (!data["model"].is_null()).then(|| encode_json(&data["model"])),
                i64::try_from(event.created).unwrap_or(0),
            ],
        )?;
        if inserted == 0 {
            return Err(tx_defect(&session_id));
        }
        Ok(())
    });

    install(bus, "session.moved", |tx, event| {
        run_updater(tx, event)?;
        let session_id = session_field(event);
        let project_id = event.data["projectID"].as_str().map(str::to_string);
        let workspace_id = event.data["location"]["workspaceID"].as_str().map(str::to_string);
        let directory = event.data["location"]["directory"].as_str().map(str::to_string);
        let subpath = event.data["subpath"].as_str().map(str::to_string);
        let created = i64::try_from(event.created).unwrap_or(0);
        let updated = tx.execute(
            "UPDATE session_v2 SET directory = ?1, path = ?2, workspace_id = ?3, time_updated = ?4 \
             WHERE id = ?5",
            rusqlite::params![directory, subpath, workspace_id, created, session_id],
        )?;
        if let (Some(project_id), 1..) = (&project_id, updated) {
            tx.execute(
                "UPDATE session_v2 SET project_id = ?1 WHERE id = ?2",
                rusqlite::params![project_id, session_id],
            )?;
        }
        Ok(())
    });

    install(bus, "worktree.resolved", |tx, event| project_worktree(tx, event));

    install(bus, "session.deleted", |tx, event| {
        let session_id = session_field(event);
        tx.execute("DELETE FROM session_v2 WHERE id = ?1", rusqlite::params![session_id])?;
        Ok(())
    });

    install(bus, "session.agent.selected", |tx, event| {
        run_updater(tx, event)?;
        let agent = event.data["agent"].as_str().map(str::to_string);
        update_session_column(tx, event, "agent = ?1", vec![Box::new(agent)])
    });

    install(bus, "session.model.selected", |tx, event| {
        run_updater(tx, event)?;
        let model = encode_json(&event.data["model"]);
        update_session_column(tx, event, "model = ?1", vec![Box::new(Some(model))])
    });

    install(bus, "session.renamed", |tx, event| {
        let title = event.data["title"].as_str().map(str::to_string);
        update_session_column(tx, event, "title = ?1", vec![Box::new(title)])
    });

    install(bus, "session.metadata.updated", |tx, event| {
        let metadata = encode_json(&event.data["metadata"]);
        update_session_column(tx, event, "metadata = ?1", vec![Box::new(Some(metadata))])
    });

    install(bus, "session.permissions", |tx, event| {
        let permission = encode_json(&event.data["permissions"]);
        update_session_column(tx, event, "permission = ?1", vec![Box::new(Some(permission))])
    });

    install(bus, "session.viewed", |tx, event| {
        let session_id = session_field(event);
        let idle = event.data["idle"].as_f64().unwrap_or(0.0) as i64;
        tx.execute(
            // Monotone watermark: a duplicate or stale view never regresses.
            "UPDATE session_v2 SET time_viewed = max(?1, coalesce(time_viewed, ?1)), \
             time_updated = time_updated WHERE id = ?2",
            rusqlite::params![idle, session_id],
        )?;
        Ok(())
    });

    install(bus, "session.message.content.updated", |tx, event| run_updater(tx, event));

    install(bus, "session.usage.recorded", |tx, event| {
        apply_usage(tx, event)?;
        Ok(())
    });

    install(bus, "session.forked", |tx, event| project_fork(tx, event));

    install(bus, "session.inbox.delivered", |tx, event| {
        let session_id = session_field(event);
        let inbox_id = event.data["inboxID"].as_str().unwrap_or_default().to_string();
        let input = project_delivered(tx, &session_id, &inbox_id)?;
        let message = match &input {
            oc_schema::session_inbox::Info::User { id, payload, .. } => {
                session_message::Info::User {
                    id: id.clone(),
                    metadata: payload.metadata.clone(),
                    time: session_message::Time { created: event.created },
                    text: payload.text.clone(),
                    files: payload.files.clone(),
                    agents: payload.agents.clone(),
                    skills: payload.skills.clone(),
                }
            }
            oc_schema::session_inbox::Info::Synthetic { id, payload, .. } => {
                session_message::Info::Synthetic {
                    id: id.clone(),
                    metadata: payload.metadata.clone(),
                    time: session_message::Time { created: event.created },
                    text: payload.text.clone(),
                    description: payload.description.clone(),
                }
            }
            // Controls are loop-exiting boundaries; no visible message.
            _ => return Ok(()),
        };
        insert_message(tx, event, &message)
    });

    install(bus, "session.inbox.enqueued", |tx, event| {
        let session_id = session_field(event);
        let inbox_id = event.data["inboxID"].as_str().unwrap_or_default().to_string();
        let item: oc_schema::session_inbox::Item =
            serde_json::from_value(event.data["item"].clone())?;
        project_admitted(
            tx,
            i64::try_from(event.durable.seq).unwrap_or(0),
            &inbox_id,
            &session_id,
            &item,
            event.created,
        )?;
        tx.execute(
            "UPDATE session_v2 SET time_updated = ?1 WHERE id = ?2",
            rusqlite::params![i64::try_from(event.created).unwrap_or(0), session_id],
        )?;
        Ok(())
    });

    install(bus, "session.inbox.cancelled", |tx, event| {
        let session_id = session_field(event);
        let inbox_id = event.data["inboxID"].as_str().unwrap_or_default().to_string();
        project_cancelled(tx, &session_id, &inbox_id)
    });

    install(bus, "session.inbox.delivery.changed", |tx, event| {
        let session_id = session_field(event);
        let inbox_id = event.data["inboxID"].as_str().unwrap_or_default().to_string();
        let delivery = event.data["delivery"].as_str().unwrap_or_default();
        project_delivery_changed(tx, &session_id, &inbox_id, delivery)
    });

    install(bus, "session.execution.succeeded", |tx, event| project_idle(tx, event, "succeeded"));
    install(bus, "session.execution.failed", |tx, event| project_idle(tx, event, "failed"));
    install(bus, "session.execution.interrupted", |tx, event| {
        project_idle(tx, event, "interrupted")
    });

    install(bus, "session.instructions.updated", |tx, event| {
        run_updater(tx, event)?;
        let session_id = session_field(event);
        let delta: oc_schema::session_support::InstructionDelta =
            serde_json::from_value(event.data["delta"].clone())?;
        instruction_apply(tx, &session_id, i64::try_from(event.durable.seq).unwrap_or(0), &delta)
    });

    for type_ in [
        "session.synthetic",
        "session.skill.activated",
        "session.shell.started",
        "session.shell.ended",
        "session.step.started",
        "session.step.streamed",
        "session.text.started",
        "session.text.ended",
        "session.reasoning.started",
        "session.reasoning.ended",
        "session.tool.input.started",
        "session.tool.input.ended",
        "session.tool.called",
        "session.tool.success",
        "session.tool.failed",
        "session.retry.scheduled",
        "session.compaction.started",
        "session.compaction.failed",
    ] {
        install(bus, type_, |tx, event| run_updater(tx, event));
    }

    install(bus, "session.step.ended", |tx, event| {
        run_updater(tx, event)?;
        apply_usage(tx, event)
    });

    install(bus, "session.step.failed", |tx, event| {
        run_updater(tx, event)?;
        if !event.data["cost"].is_null() && !event.data["tokens"].is_null() {
            apply_usage(tx, event)?;
        }
        Ok(())
    });

    install(bus, "session.compaction.ended", |tx, event| {
        run_updater(tx, event)?;
        let session_id = session_field(event);
        instruction_advance_epoch(tx, &session_id, i64::try_from(event.durable.seq).unwrap_or(0))
    });

    install(bus, "session.revert.staged", |tx, event| {
        let session_id = session_field(event);
        let revert = encode_json(&event.data["revert"]);
        tx.execute(
            "UPDATE session_v2 SET revert = ?1, time_updated = ?2 WHERE id = ?3",
            rusqlite::params![revert, i64::try_from(event.created).unwrap_or(0), session_id],
        )?;
        Ok(())
    });

    install(bus, "session.revert.cleared", |tx, event| {
        let session_id = session_field(event);
        tx.execute(
            "UPDATE session_v2 SET revert = NULL, time_updated = ?1 WHERE id = ?2",
            rusqlite::params![i64::try_from(event.created).unwrap_or(0), session_id],
        )?;
        Ok(())
    });

    install(bus, "session.revert.committed", |tx, event| {
        let session_id = session_field(event);
        let to = event.data["to"].as_str().unwrap_or_default().to_string();
        let boundary: Option<i64> = tx
            .conn()
            .query_row(
                "SELECT seq FROM session_message WHERE session_id = ?1 AND id = ?2",
                rusqlite::params![session_id, to],
                |row| row.get(0),
            )
            .optional()?;
        let Some(boundary) = boundary else {
            return Err(DbError::Pragma(format!("Revert boundary message not found: {to}")));
        };
        tx.execute(
            "DELETE FROM session_message WHERE session_id = ?1 AND seq >= ?2",
            rusqlite::params![session_id, boundary],
        )?;
        tx.execute(
            "DELETE FROM session_inbox WHERE session_id = ?1 AND enqueued_seq >= ?2",
            rusqlite::params![session_id, boundary],
        )?;
        tx.execute(
            "UPDATE session_v2 SET revert = NULL, time_updated = ?1 WHERE id = ?2",
            rusqlite::params![i64::try_from(event.created).unwrap_or(0), session_id],
        )?;
        instruction_reset(tx, &session_id)
    });

    // UsageUpdated fan-out (projector.ts:776-787): after step terminals and
    // usage records commit, publish the row totals.
    //
    // Ordering contract (GATE4 A.3 / S2): v2's subscription is one serial
    // `Stream.runForEach` fiber, and its cooperative runtime completes
    // `publishSessionUsage` (projector.ts:94 `bus.publish(UsageUpdated)`)
    // inside the drain's yield window — strictly BEFORE the coordinator's
    // `settled` publishes the terminal (`session.execution.succeeded` /
    // `.failed` / `.interrupted`, execution.ts:118-147). The port restores
    // that causal edge on a multithreaded runtime: each fan-out is chained
    // onto the session's previous fan-out (serial, like runForEach) and
    // tracked as in-flight; `ExecutionHooks::settled` awaits
    // [`wait_usage_fanout`] before publishing any terminal.
    let fanout_bus = bus.clone();
    let usage_listener: Listener = Arc::new(move |event: &crate::bus::Event| {
        let carries_usage = match event.type_.as_str() {
            "session.step.failed" => {
                !event.data["cost"].is_null() && !event.data["tokens"].is_null()
            }
            "session.step.ended" | "session.usage.recorded" => true,
            _ => false,
        };
        if !carries_usage {
            return Ok(());
        }
        let bus = fanout_bus.clone();
        let session_id = event.data["sessionID"].as_str().unwrap_or_default().to_string();
        let event_id = event.id.clone();
        let fanout_session = session_id.clone();
        enqueue_usage_fanout(&session_id, move || {
            async move {
                if let Err(err) = publish_usage_updated(&bus, &fanout_session).await {
                    eprintln!(
                        "[session] usage fan-out failed for {fanout_session} (event {event_id}): {err}"
                    );
                }
            }
            .boxed()
        });
        Ok(())
    });
    bus.listen(usage_listener).detach();
}

// ---------------------------------------------------------------------------
// Usage fan-out serialization + in-flight tracking
// ---------------------------------------------------------------------------

/// A session's fan-out chain segment: `prev.await → publish → cleanup`.
type UsageFanoutTail = futures::future::Shared<futures::future::BoxFuture<'static, ()>>;

/// The session's current chain tail plus its generation (the cleanup
/// contract: only the newest segment may remove the entry, and only when
/// it has completed).
struct UsageFanoutSlot {
    generation: u64,
    tail: UsageFanoutTail,
}

/// Per-session chain of pending usage fan-outs (global: ids are engine-wide
/// UUIDs; deterministic test ids are unique per bus in practice, and a
/// cross-bus await only ever waits on a driven, completing chain).
fn usage_fanout_chains(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, UsageFanoutSlot>> {
    static CHAINS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, UsageFanoutSlot>>,
    > = std::sync::OnceLock::new();
    CHAINS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Chain one fan-out job onto the session's previous fan-out (serial per
/// session, v2's single `runForEach` stream) and drive it on its own task.
fn enqueue_usage_fanout(
    session_id: &str,
    make: impl FnOnce() -> futures::future::BoxFuture<'static, ()> + Send + 'static,
) {
    let mut chains =
        usage_fanout_chains().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let prev = chains.get(session_id).map(|slot| slot.tail.clone());
    let generation = chains.get(session_id).map_or(0, |slot| slot.generation + 1);
    let sid = session_id.to_string();
    let tail = async move {
        // Serial per session: the previous fan-out's publish (SSE order)
        // completes before this one starts.
        if let Some(prev) = prev {
            prev.await;
        }
        make().await;
        // Self-cleanup: a completed NEWEST segment removes the entry so
        // idle sessions hold no state; if a newer segment replaced the
        // slot meanwhile, that chain cleans up after itself instead.
        let mut chains =
            usage_fanout_chains().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if chains.get(&sid).is_some_and(|slot| slot.generation == generation) {
            chains.remove(&sid);
        }
    }
    .boxed()
    .shared();
    chains.insert(session_id.to_string(), UsageFanoutSlot { generation, tail: tail.clone() });
    // The chain is driven by its own task (a waiter-only chain would never
    // progress when no terminal settles — e.g. title-generation usage
    // records publish outside any execution).
    tokio::spawn(tail);
}

/// Await the completion of every usage fan-out enqueued so far for
/// `session_id` (v2: the drain's subscriber has finished
/// `publishSessionUsage` before `settled` publishes the terminal). No-op
/// when none are in flight.
pub async fn wait_usage_fanout(session_id: &str) {
    let tail = usage_fanout_chains()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(session_id)
        .map(|slot| slot.tail.clone());
    if let Some(tail) = tail {
        tail.await;
    }
}

fn install(
    bus: &Bus,
    type_: &str,
    handler: impl Fn(&mut Tx<'_>, &DurableEvent) -> Result<()> + Send + Sync + 'static,
) {
    let def = definition(type_);
    bus.project(def, Arc::new(move |tx, event| handler(tx, event)));
}

fn session_field(event: &DurableEvent) -> String {
    event.data["sessionID"].as_str().unwrap_or_default().to_string()
}

fn update_session_column(
    tx: &mut Tx<'_>,
    event: &DurableEvent,
    assignment: &str,
    values: Vec<Box<dyn rusqlite::ToSql + Send>>,
) -> Result<()> {
    let session_id = session_field(event);
    let created = i64::try_from(event.created).unwrap_or(0);
    let sql = format!("UPDATE session_v2 SET {assignment}, time_updated = ? WHERE id = ?");
    let mut params = values;
    params.push(Box::new(created));
    params.push(Box::new(session_id));
    let references: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|value| value.as_ref() as &dyn rusqlite::ToSql).collect();
    tx.execute(&sql, references.as_slice())?;
    Ok(())
}

/// `applyUsage` (projector.ts:58-73): cost + 5 token counters;
/// `time_updated` pinned (usage is not user activity).
fn apply_usage(tx: &mut Tx<'_>, event: &DurableEvent) -> Result<()> {
    let session_id = session_field(event);
    let cost = event.data["cost"].as_f64().unwrap_or(0.0);
    let tokens = &event.data["tokens"];
    let cache = &tokens["cache"];
    let value = |input: &serde_json::Value| input.as_f64().unwrap_or(0.0) as i64;
    tx.execute(
        "UPDATE session_v2 SET \
           cost = cost + ?1, \
           tokens_input = tokens_input + ?2, \
           tokens_output = tokens_output + ?3, \
           tokens_reasoning = tokens_reasoning + ?4, \
           tokens_cache_read = tokens_cache_read + ?5, \
           tokens_cache_write = tokens_cache_write + ?6, \
           time_updated = time_updated \
         WHERE id = ?7",
        rusqlite::params![
            cost,
            value(&tokens["input"]),
            value(&tokens["output"]),
            value(&tokens["reasoning"]),
            value(&cache["read"]),
            value(&cache["write"]),
            session_id
        ],
    )?;
    Ok(())
}

/// `publishSessionUsage` (projector.ts:75-104): read row totals, publish
/// the ephemeral `session.usage.updated`.
async fn publish_usage_updated(bus: &Bus, session_id: &str) -> Result<()> {
    let lookup = session_id.to_string();
    let row = bus
        .database()
        .transaction(move |tx| {
            Ok(tx.conn().query_row(
                "SELECT cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, \
                 tokens_cache_write FROM session_v2 WHERE id = ?1",
                rusqlite::params![lookup],
                |row| {
                    Ok((
                        row.get::<_, f64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?)
        })
        .await?;
    let Some((cost, input, output, reasoning, cache_read, cache_write)) = row else {
        return Ok(());
    };
    // `cost` through `Num` so whole dollars serialize as integers (the
    // v2 wire emits `0`, not `0.0`).
    let cost = serde_json::to_value(oc_schema::common::Num::from(cost)).unwrap_or_default();
    let data = serde_json::json!({
        "sessionID": session_id,
        "cost": cost,
        "tokens": {
            "input": input,
            "output": output,
            "reasoning": reasoning,
            "cache": { "read": cache_read, "write": cache_write },
        },
    });
    bus.publish(definition("session.usage.updated"), data, crate::bus::PublishOptions::default())
        .await
        .map(|_| ())
        .map_err(|err| DbError::Pragma(err.to_string()))
}

/// `projectIdle` (projector.ts:403-432).
fn project_idle(tx: &mut Tx<'_>, event: &DurableEvent, outcome: &str) -> Result<()> {
    run_updater(tx, event)?;
    let shutdown_interrupt = event.type_ == "session.execution.interrupted"
        && event.data["reason"].as_str() == Some("shutdown");
    if shutdown_interrupt {
        return Ok(());
    }
    let session_id = session_field(event);
    let time = i64::try_from(event.created).unwrap_or(0);
    tx.execute(
        // Unread uses strict timestamp comparison: every terminal advances
        // even within one millisecond.
        "UPDATE session_v2 SET \
           time_idle = max(?1, coalesce(time_idle + 1, ?1)), \
           idle_outcome = ?2, \
           time_updated = time_updated \
         WHERE id = ?3",
        rusqlite::params![time, outcome, session_id],
    )?;
    Ok(())
}

/// `insertMessage` (projector.ts:386-401).
fn insert_message(
    tx: &mut Tx<'_>,
    event: &DurableEvent,
    message: &session_message::Info,
) -> Result<()> {
    let mut value = serde_json::to_value(message)?;
    let object =
        value.as_object_mut().ok_or_else(|| DbError::Pragma("message not an object".into()))?;
    let id = object.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let type_ =
        object.get("type").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    object.remove("id");
    object.remove("type");
    let row = SessionMessageRow {
        id,
        session_id: session_field(event),
        type_,
        seq: i64::try_from(event.durable.seq).unwrap_or(0),
        time_created: message_time_created(message),
        time_updated: i64::try_from(event.created).unwrap_or(0),
        data: value.to_string(),
    };
    SessionMessageRow::insert(tx, &row)
}

fn message_time_created(message: &session_message::Info) -> i64 {
    let created = match message {
        session_message::Info::User { time, .. }
        | session_message::Info::Synthetic { time, .. }
        | session_message::Info::System { time, .. }
        | session_message::Info::Skill { time, .. }
        | session_message::Info::Idle { time, .. }
        | session_message::Info::AgentSwitched { time, .. }
        | session_message::Info::ModelSwitched { time, .. }
        | session_message::Info::LocationSwitched { time, .. } => time.created,
        session_message::Info::Shell { time, .. } => time.created,
        session_message::Info::Assistant { time, .. } => time.created,
        session_message::Info::Compaction(compaction) => match compaction {
            session_message::Compaction::Running { time, .. }
            | session_message::Compaction::Completed { time, .. }
            | session_message::Compaction::Failed { time, .. } => time.created,
        },
    };
    i64::try_from(created).unwrap_or(0)
}

/// `projectFork` (projector.ts:106-226).
fn project_fork(tx: &mut Tx<'_>, event: &DurableEvent) -> Result<()> {
    let fork_session = event.data["sessionID"].as_str().unwrap_or_default().to_string();
    let parent_id = event.data["parentID"].as_str().unwrap_or_default().to_string();
    let boundary_message =
        event.data["boundary"]["messageID"].as_str().unwrap_or_default().to_string();
    let boundary_type = event.data["boundary"]["type"].as_str().unwrap_or_default();

    let parent: Option<SessionRow> = SessionRow::get(tx, parent_id.clone())?;
    let Some(parent) = parent else {
        return Err(DbError::Pragma(format!("Fork parent session not found: {parent_id}")));
    };
    let boundary: Option<i64> = tx
        .conn()
        .query_row(
            "SELECT seq FROM session_message WHERE session_id = ?1 AND id = ?2",
            rusqlite::params![parent_id, boundary_message],
            |row| row.get(0),
        )
        .optional()?;
    let Some(boundary_seq) = boundary else {
        return Err(DbError::Pragma(format!(
            "Fork boundary message not found: {boundary_message}"
        )));
    };
    let comparison = if boundary_type == "before" { "<" } else { "<=" };
    let copied: Option<i64> = tx
        .conn()
        .query_row(
            &format!(
                "SELECT seq FROM session_message WHERE session_id = ?1 AND seq {comparison} ?2 \
                 ORDER BY seq DESC LIMIT 1"
            ),
            rusqlite::params![parent_id, boundary_seq],
            |row| row.get(0),
        )
        .optional()?;
    let copied_seq = copied;

    let fork_boundary = encode_json(&event.data["boundary"]);
    // Ref-verified (projector.ts:149-153): the fork child row carries
    // `parent_id = NULL` and `fork_session_id = <parent id>` — a fork
    // child is a top-level session for parent-chain consumers (subagent
    // permission inheritance, parent listing); only the fork source is
    // recorded. (The prior "CD-2" claim that ref carries both linkages
    // was false.)
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO session_v2 \
         (id, project_id, workspace_id, parent_id, fork_session_id, fork_boundary, slug, directory, \
          path, title, version, metadata, permission, agent, model, cost, tokens_input, \
          tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, time_created, \
          time_updated) \
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, 0, 0, 0, \
          0, 0, ?15, ?15)",
        rusqlite::params![
            fork_session,
            parent.project_id,
            parent.workspace_id,
            parent_id,
            fork_boundary,
            crate::session::store::slug_create(),
            parent.directory,
            parent.path,
            fork_title(parent.title.as_deref()),
            parent.version,
            parent.metadata,
            parent.permission,
            parent.agent,
            parent.model,
            i64::try_from(event.created).unwrap_or(0),
        ],
    )?;
    if inserted == 0 {
        return Err(tx_defect(&fork_session));
    }

    if !event.data["instructionEntries"].is_null() {
        let entries: oc_schema::session_support::InstructionEntrySnapshot =
            serde_json::from_value(event.data["instructionEntries"].clone())?;
        instruction_entry_initialize(tx, &fork_session, &entries, event.created)?;
    }

    if let Some(copied_seq) = copied_seq {
        let prefix = format!("{}_{}", MessageId::from_event(&event.id).as_str(), "{}");
        let mut cursor: i64 = -1;
        loop {
            let rows: Vec<SessionMessageRow> = {
                let mut statement = tx.conn().prepare(
                    "SELECT * FROM session_message WHERE session_id = ?1 AND seq > ?2 \
                     AND seq < ?3 \
                     AND (type != 'assistant' OR json_extract(data, '$.time.completed') IS NOT NULL) \
                     AND (type != 'shell' OR json_extract(data, '$.status') != 'running') \
                     AND (type != 'compaction' OR json_extract(data, '$.status') != 'running') \
                     ORDER BY seq LIMIT ?4",
                )?;
                let rows = statement
                    .query_map(
                        rusqlite::params![parent_id, cursor, copied_seq + 1, FORK_BATCH_SIZE],
                        SessionMessageRow::decode_row,
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            if rows.is_empty() {
                break;
            }
            for row in &rows {
                tx.execute(
                    "INSERT INTO session_message (id, session_id, type, seq, time_created, \
                     time_updated, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        prefix.replace("{}", &row.seq.to_string()),
                        fork_session,
                        row.type_,
                        row.seq,
                        row.time_created,
                        row.time_updated,
                        row.data
                    ],
                )?;
            }
            cursor = rows.last().expect("non-empty").seq;
        }
        tx.event_sequence_upsert(&fork_session, copied_seq, None)?;
    }

    if !event.data["instructions"].is_null() {
        let values: InstructionValues = serde_json::from_value(event.data["instructions"].clone())?;
        instruction_initialize(
            tx,
            &fork_session,
            i64::try_from(event.durable.seq).unwrap_or(0),
            &values,
        );
    }
    Ok(())
}

/// `Worktree.Event.Resolved` re-own (projector.ts:486-542): sessions whose
/// ownership came from the directory's previous resolution follow its new
/// identity; location, transcript, instructions and recency are untouched.
fn project_worktree(tx: &mut Tx<'_>, event: &DurableEvent) -> Result<()> {
    let project_id = event.data["projectID"].as_str().unwrap_or_default().to_string();
    let directory = event.data["directory"].as_str().unwrap_or_default().to_string();
    let previous = event.data["previous"].as_str().unwrap_or_default().to_string();
    let adopted: Vec<String> = event.data["adopted"]
        .as_array()
        .map(|values| {
            values.iter().filter_map(|value| value.as_str().map(str::to_string)).collect()
        })
        .unwrap_or_default();

    let mut candidates: Vec<String> = Vec::new();
    for id in std::iter::once(previous)
        .chain(std::iter::once("global".to_string()))
        .chain(adopted.iter().cloned())
    {
        if id != project_id && !candidates.contains(&id) {
            candidates.push(id);
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }

    let placeholders = candidates.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let adopted_clause = if adopted.is_empty() {
        format!("s.directory >= ?{} AND s.directory <= ?", candidates.len() + 1)
    } else {
        let adopted_marks = adopted.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        format!(
            "(s.project_id IN ({adopted_marks}) OR (s.directory >= ?{} AND s.directory <= ?{}))",
            candidates.len() + adopted.len() + 1,
            candidates.len() + adopted.len() + 2
        )
    };
    let sql = format!(
        "SELECT s.id, s.directory, s.project_id, p.worktree FROM session_v2 s \
         JOIN project p ON s.project_id = p.id \
         WHERE s.project_id IN ({placeholders}) AND s.workspace_id IS NULL AND ({adopted_clause})"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::new();
    for id in &candidates {
        params.push(id);
    }
    if !adopted.is_empty() {
        for id in &adopted {
            params.push(id);
        }
    }
    let upper_bound = format!("{directory}\u{ffff}");
    params.push(&directory);
    params.push(&upper_bound);

    let rows: Vec<(String, String, String, String)> = {
        let mut statement = tx.conn().prepare(&sql)?;
        let rows = statement
            .query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    for (id, session_directory, session_project, canonical) in rows {
        let effective =
            if adopted.contains(&session_project) { canonical } else { session_directory };
        if !contains(&directory, &effective) {
            continue;
        }
        let relative = pathdiff(&directory, &effective);
        tx.execute(
            "UPDATE session_v2 SET project_id = ?1, path = ?2, time_updated = time_updated \
             WHERE id = ?3",
            rusqlite::params![project_id, relative, id],
        )?;
    }
    Ok(())
}

/// `FSUtil.contains(base, target)`.
fn contains(base: &str, target: &str) -> bool {
    target == base || target.starts_with(&format!("{base}/"))
}

/// POSIX-style `path.relative(from, to)` with forward slashes.
fn pathdiff(from: &str, to: &str) -> String {
    let from_parts: Vec<&str> = from.split('/').filter(|part| !part.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|part| !part.is_empty()).collect();
    let mut common = 0;
    while common < from_parts.len()
        && common < to_parts.len()
        && from_parts[common] == to_parts[common]
    {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..".to_string());
    }
    for part in &to_parts[common..] {
        parts.push((*part).to_string());
    }
    if parts.is_empty() {
        // `path.relative(x, x)` is the empty string — projector.ts:531 stores
        // the raw `path.relative(...)` result with no `|| "."` fallback.
        String::new()
    } else {
        parts.join("/")
    }
}

// ---------------------------------------------------------------------------
// Message updater adapter over the event transaction
// ---------------------------------------------------------------------------

struct TxAdapter<'a, 'c> {
    tx: &'a mut Tx<'c>,
    session_id: String,
    /// The driving event (append rows take `id`/`seq` from it).
    event: DurableEvent,
}

impl TxAdapter<'_, '_> {
    fn select_message(
        &mut self,
        sql_tail: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Option<session_message::Info>> {
        let sql = format!("SELECT id, type, data FROM session_message {sql_tail}");
        let row = {
            let mut statement = self.tx.conn().prepare(&sql)?;
            let row = statement
                .query_row(params, |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .optional()?;
            row
        };
        match row {
            Some((id, type_, data)) => Ok(Some(decode_row(&id, &type_, &data)?)),
            None => Ok(None),
        }
    }
}

impl Adapter for TxAdapter<'_, '_> {
    fn get_agent(&mut self) -> Result<Option<AgentId>> {
        // The `agent` column is NULL for sessions created without one —
        // read it as nullable or the whole durable commit rolls back
        // (the M2 D3 defect: every switch 500'd on a fresh session).
        let agent: Option<Option<String>> = self
            .tx
            .conn()
            .query_row(
                "SELECT agent FROM session_v2 WHERE id = ?1",
                rusqlite::params![self.session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(agent.flatten().map(|agent| AgentId::parse(agent)))
    }

    fn get_model(&mut self) -> Result<Option<oc_schema::model::Ref>> {
        // Same NULL handling as `get_agent` (the `model` column).
        let model: Option<Option<String>> = self
            .tx
            .conn()
            .query_row(
                "SELECT model FROM session_v2 WHERE id = ?1",
                rusqlite::params![self.session_id],
                |row| row.get(0),
            )
            .optional()?;
        match model.flatten() {
            Some(model) => Ok(Some(serde_json::from_str(&model)?)),
            None => Ok(None),
        }
    }

    fn get_location(&mut self) -> Result<Option<PreviousLocation>> {
        let row: Option<(String, Option<String>, String, Option<String>)> = self
            .tx
            .conn()
            .query_row(
                "SELECT directory, workspace_id, project_id, path FROM session_v2 WHERE id = ?1",
                rusqlite::params![self.session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        Ok(row.map(|(directory, workspace_id, project_id, path)| PreviousLocation {
            location: oc_schema::location::Ref {
                directory: oc_schema::common::AbsolutePath(directory),
                workspace_id: workspace_id.map(|value| {
                    oc_schema::id::WorkspaceId::parse(value)
                        .unwrap_or_else(|err| oc_schema::id::WorkspaceId(err.value))
                }),
            },
            project_id: Some(ProjectId::parse(project_id)),
            subpath: path,
        }))
    }

    fn get_current_assistant(&mut self) -> Result<Option<session_message::Info>> {
        // A newer step supersedes stale incomplete rows; never resume an
        // older assistant projection.
        let session = self.session_id.clone();
        let message = self.select_message(
            "WHERE session_id = ?1 AND type = 'assistant' ORDER BY seq DESC LIMIT 1",
            &[&session],
        )?;
        Ok(match message {
            Some(message @ session_message::Info::Assistant { time, .. }) => {
                (time.completed.is_none()).then_some(message)
            }
            _ => None,
        })
    }

    fn get_assistant(&mut self, message_id: &str) -> Result<Option<session_message::Info>> {
        let session = self.session_id.clone();
        let message = self.select_message(
            "WHERE id = ?1 AND session_id = ?2 AND type = 'assistant'",
            &[&message_id, &session],
        )?;
        Ok(message.filter(|message| matches!(message, session_message::Info::Assistant { .. })))
    }

    fn get_shell(&mut self, shell_id: &str) -> Result<Option<session_message::Info>> {
        let session = self.session_id.clone();
        let message = self.select_message(
            "WHERE session_id = ?1 AND type = 'shell' AND json_extract(data, '$.shellID') = ?2 \
             ORDER BY seq DESC LIMIT 1",
            &[&session, &shell_id],
        )?;
        Ok(message.filter(|message| matches!(message, session_message::Info::Shell { .. })))
    }

    fn get_compaction(&mut self) -> Result<Option<session_message::Info>> {
        let session = self.session_id.clone();
        let message = self.select_message(
            "WHERE session_id = ?1 AND type = 'compaction' \
             AND json_extract(data, '$.status') = 'running' ORDER BY seq DESC LIMIT 1",
            &[&session],
        )?;
        Ok(message.filter(|message| matches!(message, session_message::Info::Compaction(_))))
    }

    fn update_assistant(&mut self, message: &session_message::Info) -> Result<()> {
        update_message_row(self.tx, &self.session_id, message)
    }

    fn update_shell(&mut self, message: &session_message::Info) -> Result<()> {
        update_message_row(self.tx, &self.session_id, message)
    }

    fn update_compaction(&mut self, message: &session_message::Info) -> Result<()> {
        update_message_row(self.tx, &self.session_id, message)
    }

    fn append_message(&mut self, message: &session_message::Info) -> Result<()> {
        insert_message(self.tx, &self.event, message)
    }
}

fn run_updater(tx: &mut Tx<'_>, event: &DurableEvent) -> Result<()> {
    let session_id = session_field(event);
    let mut adapter = TxAdapter { tx, session_id, event: event.clone() };
    SessionMessageUpdater::update(&mut adapter, event)
}

fn update_message_row(
    tx: &mut Tx<'_>,
    session_id: &str,
    message: &session_message::Info,
) -> Result<()> {
    let mut value = serde_json::to_value(message)?;
    let object =
        value.as_object_mut().ok_or_else(|| DbError::Pragma("message not an object".into()))?;
    let id = object.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let type_ =
        object.get("type").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    object.remove("id");
    object.remove("type");
    tx.execute(
        "UPDATE session_message SET type = ?1, time_created = ?2, data = ?3 \
         WHERE id = ?4 AND session_id = ?5",
        rusqlite::params![type_, message_time_created(message), value.to_string(), id, session_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Instruction state / entries (instruction-state.ts, instruction-entry.ts)
// ---------------------------------------------------------------------------

fn values_json(values: &InstructionValues) -> String {
    serde_json::to_string(&values).unwrap_or_else(|_| "{}".to_string())
}

/// `InstructionState.apply` (instruction-state.ts:89-117).
fn instruction_apply(
    tx: &mut Tx<'_>,
    session_id: &str,
    seq: i64,
    delta: &oc_schema::session_support::InstructionDelta,
) -> Result<()> {
    let stored: Option<InstructionStateRow> = InstructionStateRow::get(tx, session_id.to_string())?;
    let mut current: InstructionValues = stored
        .as_ref()
        .and_then(|row| serde_json::from_str::<InstructionValues>(&row.current_values).ok())
        .unwrap_or_default();
    for (key, value) in delta {
        match value {
            InstructionDeltaValue::Hash(hash) => {
                current.insert(key.clone(), hash.clone());
            }
            InstructionDeltaValue::Removed => {
                current.remove(key);
            }
        }
    }
    let encoded = values_json(&current);
    match stored {
        None => {
            InstructionStateRow::insert(
                tx,
                &InstructionStateRow {
                    session_id: session_id.to_string(),
                    epoch_start: seq,
                    through_seq: seq,
                    initial_values: encoded.clone(),
                    current_values: encoded,
                },
            )?;
        }
        Some(_) => {
            tx.execute(
                "UPDATE instruction_state SET through_seq = ?1, current_values = ?2 \
                 WHERE session_id = ?3",
                rusqlite::params![seq, encoded, session_id],
            )?;
        }
    }
    Ok(())
}

/// `InstructionState.initialize` (onConflictDoNothing).
fn instruction_initialize(tx: &mut Tx<'_>, session_id: &str, seq: i64, values: &InstructionValues) {
    let encoded = values_json(values);
    let _ = InstructionStateRow::insert_or_ignore(
        tx,
        &InstructionStateRow {
            session_id: session_id.to_string(),
            epoch_start: seq,
            through_seq: seq,
            initial_values: encoded.clone(),
            current_values: encoded,
        },
    );
}

/// `InstructionState.advanceEpoch`.
fn instruction_advance_epoch(tx: &mut Tx<'_>, session_id: &str, epoch_start: i64) -> Result<()> {
    tx.execute(
        "UPDATE instruction_state SET epoch_start = ?1, through_seq = ?1, \
         initial_values = current_values WHERE session_id = ?2",
        rusqlite::params![epoch_start, session_id],
    )?;
    Ok(())
}

/// `InstructionState.reset`.
fn instruction_reset(tx: &mut Tx<'_>, session_id: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM instruction_state WHERE session_id = ?1",
        rusqlite::params![session_id],
    )?;
    Ok(())
}

/// `InstructionEntry.initialize` (10-row batches, instruction-entry.ts:41-67).
///
/// Insert errors propagate (ref `orDie`): the whole projection
/// transaction rolls back — bus.rs documents that a projector `Err`
/// aborts the publish.
fn instruction_entry_initialize(
    tx: &mut Tx<'_>,
    session_id: &str,
    entries: &oc_schema::session_support::InstructionEntrySnapshot,
    created: u64,
) -> Result<()> {
    const INSERT_BATCH_SIZE: usize = 10;
    let created = i64::try_from(created).unwrap_or(0);
    for batch in entries.chunks(INSERT_BATCH_SIZE) {
        for entry in batch {
            let row = InstructionEntryRow {
                session_id: session_id.to_string(),
                key: entry.key.clone(),
                value: serde_json::to_string(&entry.value).ok(),
                removed: entry.removed,
                time_created: created,
                time_updated: created,
            };
            InstructionEntryRow::insert(tx, &row)?;
        }
    }
    Ok(())
}
