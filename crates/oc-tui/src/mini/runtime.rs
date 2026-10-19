//! The mini runtime orchestrator — port of
//! `packages/tui/src/mini/runtime.ts`.
//!
//! Boots the lifecycle (renderer + footer pane), starts the stream
//! transport (the SSE subscription), and feeds prompts through the
//! queue until the footer closes. The frontend paints before
//! resolving follow-up state, then (`runtime.ts:1-10`):
//!
//! 1. resolves model info + session history (replay),
//! 2. creates the split-footer lifecycle,
//! 3. starts the stream transport,
//! 4. runs the prompt queue until the footer closes.
//!
//! The v2 runtime is an effect graph over abortable promises; the Rust
//! port drives the same seams from one tick loop (crossterm poll →
//! footer keys; SSE channel → transport folds; queue → prompt turns),
//! mirroring [`crate::app`]'s live loop. Turns are serial: `run`
//! waits for the active execution to settle (`settle` →
//! `waitForIdle`, `runtime.ts:929-932`) before the queue drains the
//! next prompt; prompts submitted mid-turn admit to the server's
//! durable queue instead (`runtime.queue.ts:256-276`).

#![allow(clippy::match_same_arms)] // fold arms mirror the transport switch

use std::time::{Duration, Instant};

use oc_client::Client;
use serde_json::Value;

use super::demo::{self, DemoOutcome};
use super::lifecycle::{FooterEffect, FooterKey, Lifecycle, LifecycleInput};
use super::model_pref::ModelPreferenceRepository;
use super::prompt_shared::PromptHistory;
use super::queue::{PromptQueue, QueueAction, RunPrompt};
use super::scrollback::{Commit, CommitKind, Phase};
use super::transport::{self, Fold, TransportState};

/// The prompt-loop poll tick (`app.rs`'s 100 ms crossterm poll).
const TICK: Duration = Duration::from_millis(100);

/// How long a turn may run before the queue drains anyway (the
/// reference keeps no explicit cap; the guard only stops pathological
/// stalls).
const TURN_TIMEOUT: Duration = Duration::from_secs(600);

/// A session resolved by the caller (`ResolvedSession`,
/// `runtime.ts:104-113` — the runMini seam).
pub struct ResolvedSession {
    pub session_id: String,
    pub session_title: Option<String>,
    pub directory: String,
    pub model_label: Option<String>,
    pub agent: Option<String>,
    /// `resume`: the session existed (replay applies).
    pub resume: bool,
}

/// `MiniSettings` (`resolveMiniSettings`, runtime.boot.ts:87-98 —
/// CONFIG-T4): the cli.json `mini` section's display keys, resolved
/// with v2's per-key defaults.
#[allow(clippy::struct_excessive_bools)] // the knob set mirrors resolveMiniSettings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiniSettings {
    /// `mini.thinking === "show"` (default show): reasoning commits
    /// reach the scrollback.
    pub thinking: bool,
    /// `mini.tools === "show"` (default HIDE): tool commits reach the
    /// scrollback (stream.ts:802-803's `tools` flag).
    pub tools: bool,
    /// `mini.shell_output === "show"` (default hide): raw shell tool
    /// output in the scrollback stream.
    pub shell_output: bool,
    /// `mini.turn_summary === "show"` (default show): the agent/model/
    /// duration summary row after each turn (footer.ts:457).
    pub turn_summary: bool,
    /// `mini.footer === "show"` (default show): the footer's
    /// persistent details strip.
    pub footer: bool,
    /// `mini.splash === "show"` (default show): the entry/exit splash.
    pub splash: bool,
    /// `mini.work_spinner`: the footer spinner animation id.
    pub work_spinner: String,
    /// `mini.mono` (default false): monochrome ASCII output.
    pub mono: bool,
}

impl Default for MiniSettings {
    fn default() -> Self {
        Self {
            thinking: true,
            tools: false,
            shell_output: false,
            turn_summary: true,
            footer: true,
            splash: true,
            work_spinner: "block-soft-slide".to_owned(),
            mono: false,
        }
    }
}

impl MiniSettings {
    /// `resolveMiniSettings(config)` (runtime.boot.ts:87-98): each key
    /// falls back to its per-key default when absent.
    #[must_use]
    pub fn resolve(info: &oc_core::cli_config::Info) -> Self {
        let mini = info.mini.as_ref();
        let flag = |value: Option<&String>| value.map(|text| text == "show");
        Self {
            thinking: flag(mini.and_then(|m| m.thinking.as_ref())).unwrap_or(true),
            tools: flag(mini.and_then(|m| m.tools.as_ref())).unwrap_or(false),
            shell_output: flag(mini.and_then(|m| m.shell_output.as_ref())).unwrap_or(false),
            turn_summary: flag(mini.and_then(|m| m.turn_summary.as_ref())).unwrap_or(true),
            footer: flag(mini.and_then(|m| m.footer.as_ref())).unwrap_or(true),
            splash: flag(mini.and_then(|m| m.splash.as_ref())).unwrap_or(true),
            work_spinner: mini
                .and_then(|m| m.work_spinner.clone())
                .unwrap_or_else(|| "block-soft-slide".to_owned()),
            mono: mini.and_then(|m| m.mono).unwrap_or(false),
        }
    }

    /// `resolveRunTuiConfig`'s default path (runtime.boot.ts:76-85):
    /// no injected config — read the global cli.json (missing/invalid
    /// file degrades to the defaults).
    #[must_use]
    pub fn from_global_config() -> Self {
        let roots = oc_util::global_roots::resolve();
        let service = oc_core::cli_config::CliConfig::open(&roots);
        Self::resolve(&service.get())
    }

    /// The transport-fold gate (stream-v2.transport.ts:870,1219 —
    /// `input.thinking`; :682 `showTools()`): whether one folded
    /// scrollback commit renders. Reasoning commits need
    /// `thinking=show`; tool commits need `tools=show`; everything
    /// else always flows.
    #[must_use]
    pub fn commit_visible(&self, commit: &super::scrollback::Commit) -> bool {
        use super::scrollback::CommitKind;
        match commit.kind {
            CommitKind::Reasoning => self.thinking,
            CommitKind::Tool => self.tools,
            _ => true,
        }
    }
}

/// The frontend inputs (`RunDeferredInput`, `runtime.ts:68-85`).
pub struct MiniInput {
    pub client: Client,
    pub target: ResolvedSession,
    /// `--prompt` merged with piped stdin (`initialInput`).
    pub initial_input: Option<String>,
    /// `replay` default true (`mini.ts:123`).
    pub replay: bool,
    pub replay_limit: Option<usize>,
    pub demo: bool,
    pub version: String,
    pub home: String,
    pub state_dir: std::path::PathBuf,
    /// The mini display settings (`resolveMiniSettings` over the
    /// cli.json `mini` section — CONFIG-T4). `None` resolves from the
    /// global config here, like v2's `resolveRunTuiConfig` default
    /// path (the CLI host may pre-resolve for tests).
    pub settings: Option<MiniSettings>,
}

/// The runtime loop. Returns the process exit status.
///
/// Exit semantics (`runtime.ts:1081-1091`): teardown aborts the
/// runtime, closes the shell with `showExit = shown && sessionID` —
/// the exit splash only prints once a turn actually rendered.
pub async fn run(input: MiniInput) -> Result<i32, String> {
    // The model-variant preference repository rides along for parity
    // with `createMiniHost` (`mini-host.ts:18-30`); variant switching
    // itself arrives with the model picker panel.
    let _preferences = ModelPreferenceRepository::new(input.state_dir.join("model.json"));
    // CONFIG-T4 — `resolveMiniSettings(tuiConfig)` (runtime.ts:197-199
    // via runtime.boot.ts:76-98): the cli.json `mini` section drives
    // the display (splash/mono into the lifecycle boot, the fold gates
    // and the turn summary below); absent config = v2's per-key
    // defaults.
    let settings = input.settings.clone().unwrap_or_else(MiniSettings::from_global_config);
    let mut lifecycle = Lifecycle::boot(LifecycleInput {
        version: input.version.clone(),
        home: input.home.clone(),
        directory: input.target.directory.clone(),
        agent: input.target.agent.clone(),
        model_label: input.target.model_label.clone(),
        splash: settings.splash,
        mono: settings.mono,
        footer: settings.footer,
        work_spinner: settings.work_spinner.clone(),
        shell_output: settings.shell_output,
    })?;
    let client = input.client.clone();
    let session_id = input.target.session_id.clone();

    // Replay: the session's prior transcript into scrollback
    // (`resolveSessionInfo` + `footer.event({ type: "history" })`,
    // `runtime.ts:474-491` — the newest `replayLimit` messages).
    let mut shown = false;
    if input.target.resume && input.replay {
        replay_history(&mut lifecycle, &client, &session_id, input.replay_limit).await;
        shown = true;
    }

    // The stream transport (`ensureStream`, `runtime.ts:776-831`):
    // subscribe `/api/event` once; folds flow back over a channel and
    // the loop resubscribes on disconnect (the SSE retry). The queue
    // never runs before the subscription is live (v2 prewarms the
    // transport after firstPaint, `runtime.ts:1055-1069`).
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<Value>(256);
    let (stream_ready_tx, stream_ready_rx) = tokio::sync::oneshot::channel::<()>();
    let subscribe_client = client.clone();
    let stream_task = tokio::spawn(async move {
        let mut ready = Some(stream_ready_tx);
        loop {
            if let Ok(mut stream) = subscribe_client.event().subscribe().await {
                // Signal readiness on the FIRST live subscription (a
                // dead server falls through to the reconnect cadence;
                // the composer still runs after the wait below).
                if let Some(sender) = ready.take() {
                    let _ = sender.send(());
                }
                while let Ok(Some(event)) = stream.next_json().await {
                    if event_tx.send(event).await.is_err() {
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });
    // Prewarm gate: wait for the subscription (or the timeout — an
    // unreachable server must not wedge the composer).
    let _ = tokio::time::timeout(Duration::from_secs(5), stream_ready_rx).await;

    let mut runtime = Runtime {
        lifecycle,
        client,
        session_id: session_id.clone(),
        session_title: input.target.session_title.clone(),
        transport: TransportState::new(&session_id),
        queue: PromptQueue::new(false),
        history: PromptHistory::new(),
        pending: Vec::new(),
        events: event_rx,
        demo: input.demo,
        demo_counter: 0,
        shown,
        exit_code: 0,
        turn_idle: false,
        settings,
        turn_agent: input.target.agent.clone(),
        turn_model: input.target.model_label.clone(),
    };

    // The initial input submits first (`runPromptQueue`'s
    // `submit(initialInput)`, `runtime.queue.ts:307-310`).
    if let Some(initial) = input.initial_input.clone().filter(|text| !text.trim().is_empty()) {
        runtime.pending.push(RunPrompt { text: initial, ..RunPrompt::default() });
    }

    // Demo mode greets with the slash-command intro (`state.demo.start()`
    // → `intro()`, `demo.ts:938-940` + `:903-921`).
    if input.demo {
        runtime.lifecycle.append(Commit::system(&super::demo::intro_note()));
    }

    while !runtime.lifecycle.is_closed() {
        runtime.pump_keys().await;
        runtime.pump_events();
        runtime.drain_queue().await;
        // CONFIG-T4 — `busy()` (`footer.view.tsx:980`): a running turn
        // anchors the footer's work-spinner cell.
        runtime.lifecycle.pane.set_busy(runtime.transport.is_running());
        runtime.lifecycle.flush()?;
        tokio::time::sleep(TICK).await;
    }
    stream_task.abort();
    Ok(runtime.exit_code)
}

/// Everything the loop drives (the `RuntimeState`,
/// `runtime.ts:115-134`).
struct Runtime {
    lifecycle: Lifecycle,
    client: Client,
    session_id: String,
    session_title: Option<String>,
    transport: TransportState,
    queue: PromptQueue,
    history: PromptHistory,
    pending: Vec<RunPrompt>,
    events: tokio::sync::mpsc::Receiver<Value>,
    demo: bool,
    demo_counter: usize,
    shown: bool,
    exit_code: i32,
    /// A terminal execution fold arrived this turn (`waitForIdle`).
    turn_idle: bool,
    /// CONFIG-T4: the resolved mini display settings (the fold gates
    /// + the turn summary).
    settings: MiniSettings,
    /// The turn summary's labels (v2's `turnAgent ?? currentAgent()`
    /// / `currentModel()` — the resolved target's labels stand in for
    /// the per-turn tracking the footer owns).
    turn_agent: Option<String>,
    turn_model: Option<String>,
}

impl Runtime {
    /// Poll crossterm and route footer keys (`RunFooter`'s input
    /// layer).
    async fn pump_keys(&mut self) {
        while crossterm::event::poll(Duration::from_millis(0)).unwrap_or(false) {
            let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() else { continue };
            if key.kind == crossterm::event::KeyEventKind::Release {
                continue;
            }
            let Some(footer_key) = map_key(&key) else { continue };
            for effect in self.lifecycle.pane.press(footer_key) {
                self.apply_effect(effect).await;
            }
        }
    }

    async fn apply_effect(&mut self, effect: FooterEffect) {
        match effect {
            FooterEffect::Submit(text) => {
                self.history.push(&text);
                self.pending.push(RunPrompt { text, ..RunPrompt::default() });
            }
            FooterEffect::Close => {
                self.lifecycle.close(
                    self.shown && !self.session_id.is_empty(),
                    self.session_title.as_deref(),
                    Some(&self.session_id),
                );
            }
            FooterEffect::Interrupt => {
                if self.demo {
                    return;
                }
                let _ = transport::interrupt(&self.client, &self.session_id, true).await;
            }
            FooterEffect::PermissionReply(reply) => {
                if self.demo {
                    self.demo_counter += 1;
                    if let Some(commit) =
                        demo::demo_permission_reply(&reply.decision, self.demo_counter)
                    {
                        self.lifecycle.append(commit);
                    }
                } else {
                    let _ = transport::reply_permission(
                        &self.client,
                        &reply.session_id,
                        &reply.request_id,
                        &reply.decision,
                        (!reply.message.is_empty()).then(|| reply.message.clone()),
                    )
                    .await;
                }
            }
            FooterEffect::FormReply(reply) => {
                if self.demo {
                    self.demo_counter += 1;
                    let answer = reply
                        .answer
                        .values()
                        .find_map(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    self.lifecycle.append(demo::demo_form_reply(&answer, self.demo_counter));
                }
                // Live form replies ride the session.form route once
                // forms flow over SSE (the transport lane).
            }
            FooterEffect::FormCancel(form_id) => {
                if self.demo {
                    self.demo_counter += 1;
                    self.lifecycle.append(demo::demo_form_cancel(self.demo_counter));
                }
                let _ = form_id;
            }
            FooterEffect::HistoryUp => {
                if let Some(text) = self.history.up(&self.lifecycle.pane.draft) {
                    self.lifecycle.pane.draft = text;
                }
            }
            FooterEffect::HistoryDown => {
                if let Some(text) = self.history.down() {
                    self.lifecycle.pane.draft = text;
                }
            }
        }
    }

    /// Drain arrived SSE events through the transport folds.
    fn pump_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match self.transport.fold(&event) {
                Fold::Ignored => {}
                Fold::Permission(request) => self.lifecycle.pane.permission_asked(request),
                Fold::PermissionReplied(id) => {
                    self.lifecycle.pane.machine.permission_replied(&id);
                }
                Fold::Form(request) => self.lifecycle.pane.form_created(request),
                Fold::FormRemoved(id) => {
                    self.lifecycle.pane.machine.form_removed(&id);
                }
                Fold::Commit(commit) => {
                    // CONFIG-T4 — the display gates (stream-v2.
                    // transport.ts:870,1219 `input.thinking`, :682
                    // `showTools()`): hidden kinds never reach the
                    // scrollback (the fold still tracks them).
                    if !self.settings.commit_visible(&commit) {
                        continue;
                    }
                    self.shown = true;
                    self.lifecycle.append(commit);
                }
                Fold::Idle => {}
            }
            if self.transport.is_terminated() {
                self.turn_idle = true;
            }
        }
    }

    /// The queue drain: one local turn at a time (`drain`,
    /// `runtime.queue.ts:85-241`); ordinary prompts over an active
    /// turn admit as durable queued work.
    async fn drain_queue(&mut self) {
        if self.transport.is_running() {
            return;
        }
        let Some(prompt) = self.pending.first().cloned() else { return };
        let action = self.queue.submit(prompt);
        match action {
            QueueAction::Rejected => {
                self.pending.remove(0);
            }
            QueueAction::Exit => {
                self.pending.clear();
                self.lifecycle.close(
                    self.shown && !self.session_id.is_empty(),
                    self.session_title.as_deref(),
                    Some(&self.session_id),
                );
            }
            QueueAction::NewSession { unavailable: _ } => {
                self.pending.remove(0);
                // The session factory stays with the caller's
                // `createSession` seam (`runtime.ts:933-1018`) — mini's
                // Rust port keeps the boot session.
                self.lifecycle.append(Commit::system("new sessions unavailable"));
            }
            QueueAction::Compact => {
                self.pending.remove(0);
                if !self.demo {
                    let _ = transport::compact(&self.client, &self.session_id).await;
                }
                self.lifecycle.append(super::scrollback::compaction_note("compacting session"));
            }
            QueueAction::Run { prompt } => {
                self.pending.remove(0);
                self.shown = true;
                self.queue.turn_started(prompt.clone());
                self.run_turn(&prompt).await;
                self.queue.turn_finished();
            }
            QueueAction::Admit { prompt, delivery } => {
                self.pending.remove(0);
                self.shown = true;
                let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
                let _ = transport::send_prompt(
                    &self.client,
                    &self.session_id,
                    &message_id,
                    &prompt.text,
                    Some(delivery),
                )
                .await;
            }
        }
    }

    /// One prompt turn: demo scripts intercept (`state.demo.prompt`,
    /// `runtime.ts:1019-1021`), everything else posts `session.prompt`
    /// and the SSE folds render the reply while the loop keeps
    /// pumping (`settle` → `waitForIdle`, `runtime.ts:929-932`).
    async fn run_turn(&mut self, prompt: &RunPrompt) {
        if self.demo {
            self.demo_counter += 1;
            match demo::run_demo_prompt(&prompt.text, self.demo_counter) {
                DemoOutcome::Passthrough => {}
                DemoOutcome::Script { user_echo, rows, permission, form, note } => {
                    if let Some(echo) = user_echo {
                        self.lifecycle.append(echo);
                    }
                    for row in rows {
                        self.lifecycle.append(row);
                    }
                    if let Some(request) = permission {
                        self.lifecycle.pane.permission_asked(request);
                    }
                    if let Some(request) = form {
                        self.lifecycle.pane.form_created(request);
                    }
                    if let Some(note) = note {
                        self.lifecycle.append(note);
                    }
                    return;
                }
            }
        }
        // The user echo (`onSend` → `rememberLocal`,
        // `runtime.ts:895-907`): pending rows can still be cancelled,
        // so the echo prints before the POST resolves.
        self.lifecycle.append(Commit::user(&prompt.text));
        let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        if let Err(error) =
            transport::send_prompt(&self.client, &self.session_id, &message_id, &prompt.text, None)
                .await
        {
            self.lifecycle.append(Commit::error(&error));
            self.exit_code = 1;
            return;
        }
        // Settle: pump keys + folds until the execution terminates (the
        // running → idle transition, or a terminal fold that arrived in
        // the same batch), then drain once more.
        let deadline = Instant::now() + TURN_TIMEOUT;
        let turn_started = Instant::now();
        let mut observed_running = self.transport.is_running();
        self.turn_idle = false;
        self.transport.begin_turn();
        while Instant::now() < deadline && !self.lifecycle.is_closed() {
            self.pump_keys().await;
            self.pump_events();
            // CONFIG-T4 — the busy cell stays live through the turn
            // (the settle loop owns the footer while it waits).
            self.lifecycle.pane.set_busy(self.transport.is_running());
            let running = self.transport.is_running();
            if self.turn_idle || (observed_running && !running) {
                break;
            }
            observed_running = running;
            let _ = self.lifecycle.flush();
            tokio::time::sleep(TICK).await;
        }
        // CONFIG-T4 — `turn.duration` → `writeTurnSummary`
        // (footer.ts:455-470): the agent/model/duration summary row
        // lands in the scrollback after every settled turn unless
        // `mini.turn_summary` is "hide".
        if self.settings.turn_summary {
            let agent = self.turn_agent.clone().unwrap_or_else(|| "Build".to_owned());
            let model = self.turn_model.clone().unwrap_or_else(|| "default".to_owned());
            #[allow(clippy::cast_possible_truncation)]
            let duration = crate::components::message::model::locale_duration(
                u64::try_from(turn_started.elapsed().as_millis()).unwrap_or(0),
            );
            self.lifecycle.append(super::scrollback::turn_summary(
                &agent,
                &model,
                &duration,
                self.settings.mono,
            ));
        }
        if !self.lifecycle.is_closed() {
            let _ = self.lifecycle.flush();
        }
    }
}

/// Replay the session's prior transcript (`resolveSessionInfo`,
/// `runtime.boot.ts` — the newest messages, user + assistant text).
async fn replay_history(
    lifecycle: &mut Lifecycle,
    client: &Client,
    session_id: &str,
    limit: Option<usize>,
) {
    let query = oc_client::types::SessionMessageListQuery {
        limit: limit.map(|limit| limit.to_string()).or(Some("200".to_owned())),
        order: Some(oc_client::types::SessionMessageListQueryOrder::Desc),
        ..Default::default()
    };
    let Ok(page) = client.message().list(session_id, &query).await else { return };
    let mut messages = page.data;
    messages.reverse();
    for message in &messages {
        let Ok(value) = serde_json::to_value(message) else { continue };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
        let role = value.get("role").and_then(Value::as_str).unwrap_or_default();
        let text = value
            .get("parts")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| {
                        let part_type = part.get("type").and_then(Value::as_str)?;
                        if part_type != "text" {
                            return None;
                        }
                        part.get("text").and_then(Value::as_str).map(str::to_owned)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let commit = if kind == "user" || role == "user" {
            Commit::user(&text)
        } else {
            Commit { kind: CommitKind::Assistant, phase: Phase::Final, text, ..Commit::default() }
        };
        lifecycle.append(commit);
    }
}

/// Map a crossterm key into the footer vocabulary.
fn map_key(key: &crossterm::event::KeyEvent) -> Option<FooterKey> {
    use crossterm::event::{KeyCode, KeyModifiers};
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') => Some(FooterKey::CtrlC),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Enter => Some(FooterKey::Enter),
        KeyCode::Esc => Some(FooterKey::Escape),
        KeyCode::Backspace => Some(FooterKey::Backspace),
        KeyCode::Up => Some(FooterKey::Up),
        KeyCode::Down => Some(FooterKey::Down),
        KeyCode::Char(char) => Some(FooterKey::Printable(char)),
        _ => None,
    }
}
