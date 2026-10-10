//! Prompt turns (port of `acp/turn.ts`): the SSE subscription, the
//! fold loop, permission/elicitation asks, cancel/wind-down and the
//! prompt response.

use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::{PromptResponse, SessionNotification};
use serde_json::Value;
use tokio::sync::{mpsc, watch, Mutex};

use crate::acp::child;
use crate::acp::error::{classify, AcpError};
use crate::acp::prompt::Prepared;
use crate::acp::sessions::Attached;
use crate::acp::translate::{self, Event, Folded, Output, Terminal, TurnContext, TurnState};
use crate::acp::{App, Conn};

/// `CancelDrainTimeout` (turn.ts:39-42): core acknowledges an interrupt
/// before its cleanup settles, and its shell tool waits 3s before
/// SIGKILL.
const CANCEL_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// One registered turn.
pub struct Turn {
    /// Signals `session/cancel` / `$/cancel_request` (turn.ts:389-391).
    cancelled: watch::Sender<bool>,
}

impl Turn {
    fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }
}

/// A future that resolves once the turn is cancelled.
type Cancelled<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

fn cancelled_future(turn: &Arc<Turn>) -> Cancelled<'_> {
    let mut receiver = turn.cancelled.subscribe();
    let cancelled = *receiver.borrow();
    Box::pin(async move {
        if cancelled {
            return;
        }
        let _ = receiver.changed().await;
    })
}

/// The per-connection turn registry (`FiberMap`, turn.ts:68).
#[derive(Default)]
pub struct Turns {
    turns: Mutex<std::collections::HashMap<String, Arc<Turn>>>,
}

impl Turns {
    /// Register or reject (turn.ts:372-383): one active prompt per
    /// session.
    pub async fn register(&self, session_id: &str) -> Result<Arc<Turn>, AcpError> {
        let mut turns = self.turns.lock().await;
        if turns.contains_key(session_id) {
            return Err(AcpError::ServiceFailure {
                safe_message: format!("Session already has an active ACP prompt: {session_id}"),
                service: Some("session".to_owned()),
                error_name: None,
            });
        }
        let (cancelled, _) = watch::channel(false);
        let turn = Arc::new(Turn { cancelled });
        turns.insert(session_id.to_owned(), Arc::clone(&turn));
        Ok(turn)
    }

    /// `cancel` (turn.ts:389-391): signal and forget.
    pub async fn cancel(&self, session_id: &str) {
        if let Some(turn) = self.turns.lock().await.remove(session_id) {
            if std::env::var("OPENCODE_ACP_DEBUG").is_ok() {
                eprintln!("[acp] cancelling active turn {session_id}");
            }
            let _ = turn.cancelled.send(true);
        }
    }

    async fn remove(&self, session_id: &str) {
        self.turns.lock().await.remove(session_id);
    }

    pub async fn active(&self, session_id: &str) -> bool {
        self.turns.lock().await.contains_key(session_id)
    }
}

/// `prompt` (turn.ts:366-388).
pub async fn prompt(
    app: &Arc<App>,
    conn: &Conn,
    attached: &Arc<Attached>,
    prepared: &Prepared,
) -> Result<PromptResponse, AcpError> {
    let turn = app.turns.register(&attached.id).await?;
    let result = run(app, conn, attached, prepared, &turn).await;
    app.turns.remove(&attached.id).await;
    result
}

/// `run` (turn.ts:346-364): open the subscription, execute, settle.
async fn run(
    app: &Arc<App>,
    conn: &Conn,
    attached: &Arc<Attached>,
    prepared: &Prepared,
    turn: &Arc<Turn>,
) -> Result<PromptResponse, AcpError> {
    let capabilities = *app.capabilities.read().await;
    let ctx = TurnContext {
        session_id: attached.id.clone(),
        cwd: attached.cwd.clone(),
        start: prepared.start.clone(),
        child_updates: capabilities.child_session_updates,
        compaction: capabilities.compaction,
    };
    let state = Arc::new(Mutex::new(TurnState::default()));
    let terminal = execute(app, conn, attached, prepared, &ctx, &state, turn).await?;
    settle(app, conn, attached, &ctx, &state, terminal).await
}

/// The consumer side of the turn's event subscription.
struct Consumer {
    events: mpsc::Receiver<Result<Event, AcpError>>,
}

/// `execute` (turn.ts:276-288): the feed opens with `server.connected`,
/// then the submission's events follow.
#[allow(clippy::too_many_lines)]
async fn execute(
    app: &Arc<App>,
    conn: &Conn,
    attached: &Arc<Attached>,
    prepared: &Prepared,
    ctx: &TurnContext,
    state: &Arc<Mutex<TurnState>>,
    turn: &Arc<Turn>,
) -> Result<Terminal, AcpError> {
    let mut stream =
        app.client.event().subscribe().await.map_err(|_| AcpError::ServerUnavailable)?;
    let (events_tx, events_rx) = mpsc::channel(64);
    let debug = std::env::var("OPENCODE_ACP_DEBUG").is_ok();
    let reader = tokio::spawn(async move {
        loop {
            match stream.next_json().await {
                Ok(Some(value)) => {
                    let Some(event) = parse_event(&value) else { continue };
                    if debug {
                        eprintln!("[acp] event {}", event.type_);
                    }
                    if events_tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                Ok(None) => {
                    let _ = events_tx.send(Err(AcpError::ServerUnavailable)).await;
                    return;
                }
                Err(error) => {
                    if debug {
                        eprintln!("[acp] event stream error: {error}");
                    }
                    let _ = events_tx.send(Err(classify(&error))).await;
                    return;
                }
            }
        }
    });
    let mut consumer = Consumer { events: events_rx };

    // The feed opens with server.connected (turn.ts:279-282).
    match consumer.events.recv().await {
        Some(Ok(event)) if event.type_ == "server.connected" => {}
        Some(Ok(event)) => {
            reader.abort();
            return Err(AcpError::ServiceFailure {
                safe_message: format!("expected server.connected, got {}", event.type_),
                service: None,
                error_name: Some("Protocol".to_owned()),
            });
        }
        Some(Err(error)) => {
            reader.abort();
            return Err(error);
        }
        None => return Err(AcpError::ServerUnavailable),
    }

    // `submit` (turn.ts:221-254) — after the subscription is live. The
    // Rust server's POST /prompt awaits the turn's execution (its
    // `resume` resolves with the run's `Exit`), so the HTTP call is
    // fired in the background; errors surface through the consume race
    // below.
    let (submit_tx, mut submit_rx) = mpsc::channel::<Result<(), AcpError>>(1);
    {
        let app = Arc::clone(app);
        let attached = Arc::clone(attached);
        let prepared = prepared.clone();
        let submit_tx = submit_tx.clone();
        tokio::spawn(async move {
            let result = submit(&app, &attached, &prepared).await;
            let _ = submit_tx.send(result).await;
        });
    }
    if std::env::var("OPENCODE_ACP_DEBUG").is_ok() {
        eprintln!("[acp] turn submitting start={}", prepared.start.id);
    }
    if prepared.command.is_some() {
        // Commands settle server-side; the spawned submit carries them.
        return Ok(Terminal::Succeeded);
    }

    // `consume` (turn.ts:204-212): fold until a terminal event, racing
    // the submission's HTTP result (a fast 4xx fails the turn promptly).
    let terminal =
        match consume(app, conn, ctx, state, turn, &mut consumer, false, &mut submit_rx).await {
            Outcome::Terminal(terminal) => terminal,
            Outcome::Failed(error) => {
                reader.abort();
                return Err(error);
            }
            Outcome::Cancelled => {
                // `windDown` (turn.ts:256-274).
                let _ = app
                    .client
                    .session()
                    .interrupt(
                        &ctx.session_id,
                        &oc_client::types::SessionInterruptQuery { resume: None },
                    )
                    .await;
                let started = state.lock().await.started;
                let drained = if started {
                    let (_spent_tx, mut spent_rx) = mpsc::channel(1);
                    tokio::time::timeout(
                        CANCEL_DRAIN_TIMEOUT,
                        consume(app, conn, ctx, state, turn, &mut consumer, false, &mut spent_rx),
                    )
                    .await
                    .ok()
                } else {
                    None
                };
                // Whatever settles during the drain, the turn was
                // cancelled: `session/cancel` MUST report stopReason
                // `cancelled` (StopReason docs; turn.ts:269 keeps the
                // interrupted exit over any drained terminal).
                #[allow(clippy::single_match_else)]
                let drained_terminal = match drained {
                    Some(Outcome::Terminal(_) | Outcome::Cancelled) => Terminal::Interrupted,
                    _ => {
                        reader.abort();
                        // `abandon` (translate.ts:163-187): fail the tracked
                        // tools and the root compaction.
                        let (next, outputs) = {
                            let guard = state.lock().await;
                            translate::abandon(&guard, ctx)
                        };
                        *state.lock().await = next;
                        for output in outputs {
                            interpret(app, conn, ctx, state, turn, output, false).await;
                        }
                        Terminal::Interrupted
                    }
                };
                drained_terminal
            }
        };
    // `handoff` (turn.ts:290-307): children outlive a cancelled turn, so
    // their asks still reach the client.
    follow_children_if_any(app, conn, ctx, state, turn, &mut consumer, terminal).await;
    Ok(terminal)
}

enum Outcome {
    Terminal(Terminal),
    Cancelled,
    Failed(AcpError),
}

/// `consume`/`advance` (turn.ts:191-212): fold each event and interpret
/// its outputs. In `background` mode only tracked-child events advance
/// the fold and root session updates are suppressed.
#[allow(clippy::too_many_arguments)]
async fn consume(
    app: &Arc<App>,
    conn: &Conn,
    ctx: &TurnContext,
    state: &Arc<Mutex<TurnState>>,
    turn: &Arc<Turn>,
    consumer: &mut Consumer,
    background: bool,
    submit_rx: &mut mpsc::Receiver<Result<(), AcpError>>,
) -> Outcome {
    loop {
        let event = tokio::select! {
            event = consumer.events.recv() => match event {
                Some(Ok(event)) => event,
                Some(Err(error)) => return Outcome::Failed(error),
                None => return Outcome::Failed(AcpError::ServerUnavailable),
            },
            submitted = submit_rx.recv() => {
                if let Some(Err(error)) = submitted {
                    return Outcome::Failed(error);
                }
                continue;
            }
        };
        if turn.is_cancelled() {
            return Outcome::Cancelled;
        }
        let folded = {
            let mut guard = state.lock().await;
            if background && !translate::from_tracked_child(&guard, &event) {
                Folded::default().with_state(guard.clone())
            } else {
                let folded = translate::fold(&guard, &event, ctx);
                *guard = folded.state();
                folded
            }
        };
        for output in folded.outputs {
            interpret(app, conn, ctx, state, turn, output, background).await;
        }
        if let Some(terminal) = folded.terminal {
            return Outcome::Terminal(terminal);
        }
    }
}

/// `interpret` (turn.ts:134-189).
async fn interpret(
    app: &Arc<App>,
    conn: &Conn,
    ctx: &TurnContext,
    state: &Arc<Mutex<TurnState>>,
    turn: &Arc<Turn>,
    output: Output,
    background: bool,
) {
    let debug = std::env::var("OPENCODE_ACP_DEBUG").is_ok();
    if debug {
        let kind = match &output {
            Output::SessionUpdate(_) => "session-update",
            Output::ChildUpdate(_) => "child-update",
            Output::PermissionAsk { .. } => "permission-ask",
            Output::FormAsk { .. } => "form-ask",
            Output::AskSettled { .. } => "ask-settled",
        };
        eprintln!("[acp] interpret {kind}");
    }
    match output {
        Output::SessionUpdate(update) => {
            if background {
                return;
            }
            let _ =
                conn.send_notification(SessionNotification::new(ctx.session_id.clone(), update));
        }
        Output::ChildUpdate(update) => {
            send_ext(conn, child::UPDATE_METHOD, &update);
        }
        Output::PermissionAsk { data, tool, child } => {
            // queueAsk (turn.ts:105-132): race the ask against cancel;
            // the fallback is reject.
            let input = crate::acp::permission::AskInput {
                client: &app.client,
                conn,
                event: &data,
                session_id: data
                    .get("sessionID")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                client_session_id: ctx.session_id.clone(),
                cwd: ctx.cwd.clone(),
                tracked_tool: tool,
                child,
            };
            let mut cancelled = cancelled_future(turn);
            let decision = crate::acp::permission::ask(&input, &mut cancelled).await;
            let decision = decision.unwrap_or(crate::acp::permission::Decision::Reject);
            let _ = crate::acp::permission::respond(&input, decision).await;
            let id = data.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            state.lock().await.asks.remove(&id);
        }
        Output::FormAsk { data, child } => {
            // FormAsk (turn.ts:164-181): capability-gated.
            let capabilities = *app.capabilities.read().await;
            let form = data.get("form").cloned().unwrap_or(Value::Null);
            let Some(schema) = crate::acp::elicitation::requested_schema(&form, capabilities)
            else {
                crate::acp::elicitation::cancel_unshown(&app.client, &form).await;
                let id = form.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
                state.lock().await.asks.remove(&id);
                return;
            };
            let tool_call_sent = !background && (child.is_none() || !ctx.child_updates);
            let input = crate::acp::elicitation::ElicitationInput {
                client: &app.client,
                conn,
                form: &form,
                requested_schema: schema,
                client_session_id: ctx.session_id.clone(),
                child,
                tool_call_sent,
            };
            let mut cancelled = cancelled_future(turn);
            let outcome = crate::acp::elicitation::ask(&input, &mut cancelled).await;
            crate::acp::elicitation::respond(&input, outcome).await;
            let id = form.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
            state.lock().await.asks.remove(&id);
        }
        Output::AskSettled { id } => {
            state.lock().await.asks.remove(&id);
        }
    }
}

fn send_ext(conn: &Conn, method: &str, params: &Value) {
    if let Ok(raw) = serde_json::value::RawValue::from_string(params.to_string()) {
        let _ = conn.send_notification(
            agent_client_protocol::schema::v1::AgentNotification::ExtNotification(
                agent_client_protocol::schema::v1::ExtNotification::new(
                    method,
                    std::sync::Arc::from(raw),
                ),
            ),
        );
    }
}

/// `submit` (turn.ts:221-254).
async fn submit(
    app: &Arc<App>,
    attached: &Arc<Attached>,
    prepared: &Prepared,
) -> Result<(), AcpError> {
    if !prepared.synthetic.is_empty() {
        app.client
            .session()
            .synthetic(
                &attached.id,
                &oc_client::types::SessionSyntheticBody {
                    text: prepared.synthetic.join("\n\n"),
                    description: Some("ACP embedded context".to_owned()),
                    delivery: Some(oc_client::types::SessionInboxDelivery::Steer),
                    resume: Some(false),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| classify(&error))?;
    }
    if prepared.start.kind == translate::TurnStartKind::Compaction {
        app.client
            .session()
            .compact(
                &attached.id,
                &oc_client::types::SessionCompactBody {
                    id: Some(prepared.start.id.clone()),
                    delivery: None,
                },
            )
            .await
            .map_err(|error| classify(&error))?;
        return Ok(());
    }
    let files: Vec<oc_client::types::PromptInputFileAttachment> = prepared
        .files
        .iter()
        .map(|file| oc_client::types::PromptInputFileAttachment {
            uri: file.get("uri").and_then(Value::as_str).unwrap_or_default().to_owned(),
            name: file.get("name").and_then(Value::as_str).map(str::to_owned),
            description: None,
            mention: None,
        })
        .collect();
    if prepared.command.is_some() {
        let Some(command) = &prepared.command else { unreachable!("checked above") };
        let args = prepared.slash.as_ref().map_or_else(String::new, |(_, args)| args.clone());
        app.client
            .session()
            .command(
                &attached.id,
                &oc_client::types::SessionCommandBody {
                    name: command.name.clone(),
                    text: args,
                    files: Some(files),
                    delivery: Some(oc_client::types::SessionInboxDelivery::Steer),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| classify(&error))?;
        return Ok(());
    }
    app.client
        .session()
        .prompt(
            &attached.id,
            &oc_client::types::SessionPromptBody {
                id: Some(prepared.start.id.clone()),
                text: prepared.text.clone(),
                files: Some(files),
                delivery: Some(oc_client::types::SessionInboxDelivery::Steer),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| classify(&error))?;
    Ok(())
}

/// `handoff`/`followChildren` (turn.ts:214-219, 290-307): a cancelled
/// turn's children keep streaming to the client until they close.
async fn follow_children_if_any(
    app: &Arc<App>,
    conn: &Conn,
    ctx: &TurnContext,
    state: &Arc<Mutex<TurnState>>,
    turn: &Arc<Turn>,
    consumer: &mut Consumer,
    terminal: Terminal,
) {
    if terminal != Terminal::Interrupted {
        return;
    }
    let open = state.lock().await.open_children.clone();
    if open.is_empty() {
        return;
    }
    let (_spent_tx, mut spent_rx) = mpsc::channel(1);
    let _ = consume(app, conn, ctx, state, turn, consumer, true, &mut spent_rx).await;
}

/// `settle` (turn.ts:309-319) + `sendUsageUpdate` (turn.ts:321-343).
async fn settle(
    app: &Arc<App>,
    conn: &Conn,
    attached: &Arc<Attached>,
    _ctx: &TurnContext,
    state: &Arc<Mutex<TurnState>>,
    terminal: Terminal,
) -> Result<PromptResponse, AcpError> {
    let guard = state.lock().await.clone();
    if let Some(failure) = translate::failure(&guard) {
        return Err(failure);
    }
    send_usage_update(app, conn, attached, &guard).await;
    Ok(translate::response(&guard, &attached.id, terminal))
}

/// `sendUsageUpdate` (turn.ts:321-343): when the turn used tokens and
/// the model's context limit is known.
async fn send_usage_update(
    app: &Arc<App>,
    conn: &Conn,
    attached: &Arc<Attached>,
    state: &TurnState,
) {
    let Some((turn_usage, _last)) = state.usage else { return };
    let used = turn_usage.total();
    if used == 0 {
        return;
    }
    let Ok(catalog) = app.catalogs.get(&attached.cwd).await else { return };
    let selection = attached.selection().await;
    let current = crate::acp::config_option::current_model(&catalog, &selection);
    let Some(model) = catalog.find_model(&current) else { return };
    if model.context_limit == 0 {
        return;
    }
    let Ok(info) = app.client.session().get(&attached.id).await else { return };
    let update = agent_client_protocol::schema::v1::SessionUpdate::UsageUpdate(
        agent_client_protocol::schema::v1::UsageUpdate::new(used, model.context_limit)
            .cost(agent_client_protocol::schema::v1::Cost::new(info.cost, "USD")),
    );
    let _ = conn.send_notification(SessionNotification::new(attached.id.clone(), update));
}

/// Parse one SSE frame into an [`Event`].
#[must_use]
pub fn parse_event(value: &Value) -> Option<Event> {
    let type_ = value.get("type").and_then(Value::as_str)?;
    Some(Event::new(type_, value.get("data").cloned().unwrap_or(Value::Null)))
}

/// `close` (turn.ts:392-399): unlike cancel, interrupts an idle session
/// too.
pub async fn close(app: &Arc<App>, session_id: &str) -> Result<(), AcpError> {
    if app.turns.active(session_id).await {
        app.turns.cancel(session_id).await;
        return Ok(());
    }
    let id = crate::acp::error::decode_session_id(session_id)?;
    let result = app
        .client
        .session()
        .interrupt(&id, &oc_client::types::SessionInterruptQuery { resume: None })
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(error) if matches!(error.api_name(), Some("SessionNotFoundError")) => Ok(()),
        Err(error) => Err(classify(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_parse_from_the_wire_envelope() {
        let event = parse_event(&serde_json::json!({
            "id": "evt_1", "type": "session.text.delta",
            "data": { "sessionID": "ses_1", "delta": "hi" },
        }))
        .expect("event");
        assert_eq!(event.type_, "session.text.delta");
        assert_eq!(event.data["delta"], "hi");
        assert!(parse_event(&serde_json::json!({ "id": "evt_2" })).is_none());
    }
}
