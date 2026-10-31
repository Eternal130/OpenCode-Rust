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
    // G-68 — the transport exists BEFORE the replay so the replay's
    // message projections land in the SAME fragment reconciler the
    // live stream folds through (a replayed part's text becomes the
    // committed prefix the live deltas consume from —
    // `hydrate(attempt, {render: true})` before `apply`, transport.ts:
    // 955-991).
    let mut transport = TransportState::new(&session_id);
    let mut shown = false;
    if input.target.resume && input.replay {
        replay_history(
            &mut lifecycle,
            &client,
            &session_id,
            input.replay_limit,
            &mut transport,
            &settings,
        )
        .await;
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
        transport,
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
        subagents: super::transport::SubagentTracker::new(&session_id),
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
    /// TUI-17 — the subagent tracker (`createSubagentTracker`,
    /// stream-v2.subagent.ts): every event folds through it beside
    /// the parent transport.
    subagents: super::transport::SubagentTracker,
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
            FooterEffect::InterruptChild(child) => {
                // TUI-17 — per-child interrupt
                // (`v2.session.interrupt(childID)`, resume false).
                if self.demo {
                    return;
                }
                let _ = transport::interrupt(&self.client, &child, false).await;
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
            // TUI-17 — the tracker folds EVERY event (child sessions
            // and the parent's subagent tool calls included); a
            // changed tab/detail refreshes the pane's snapshot.
            let subagent_fold = self.subagents.fold(&event);
            if subagent_fold.changed {
                self.lifecycle.pane.subagent_children = self.subagents.children();
            }
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
/// `runtime.boot.ts`) — the ref's `renderMessage(message, render =
/// true)` walk (transport.ts:802-899), one message at a time through
/// [`replay_message_commits`].
async fn replay_history(
    lifecycle: &mut Lifecycle,
    client: &Client,
    session_id: &str,
    limit: Option<usize>,
    transport: &mut TransportState,
    settings: &MiniSettings,
) {
    let query = oc_client::types::SessionMessageListQuery {
        limit: limit.map(|limit| limit.to_string()).or(Some("200".to_owned())),
        order: Some(oc_client::types::SessionMessageListQueryOrder::Desc),
        ..Default::default()
    };
    let Ok(page) = client.message().list(session_id, &query).await else { return };
    let mut messages = page.data;
    messages.reverse();
    for (index, message) in messages.iter().enumerate() {
        let Ok(value) = serde_json::to_value(message) else { continue };
        for commit in replay_message_commits(&value, index, transport, settings) {
            lifecycle.append(commit);
        }
    }
}

/// `renderMessage` (transport.ts:802-899): one projected message →
/// its scrollback commits. Every text/reasoning part re-projects
/// through the fragment reconciler (the suffix past the committed
/// prefix appends — a replayed part never double-prints), the user
/// message's image files commit (deduped), and the tool parts'
/// `data:image/*` display content commits (deduped). The projections
/// land in the SAME reconciler the live stream folds through, so
/// post-replay deltas consume the projected prefix instead of
/// double-appending (G-68's production wiring).
fn replay_message_commits(
    value: &Value,
    fallback_index: usize,
    transport: &mut TransportState,
    settings: &MiniSettings,
) -> Vec<Commit> {
    fn text(part: &Value) -> &str {
        part.get("text").and_then(Value::as_str).unwrap_or_default()
    }
    let mut commits = Vec::new();
    let parts = value.get("parts").and_then(Value::as_array).cloned().unwrap_or_default();
    let kind = value.get("type").and_then(Value::as_str).unwrap_or_default();
    let role = value.get("role").and_then(Value::as_str).unwrap_or_default();
    let message_id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("replay:{fallback_index}"));

    if kind == "user" || role == "user" {
        // `renderUser` (transport.ts:600-616): the text row, then the
        // image files' commits.
        let joined = parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .map(|part| text(part).to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        if !joined.trim().is_empty() {
            commits.push(Commit::user(&joined));
        }
        let files = value.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
        commits.extend(transport.commit_user_images(&message_id, &files));
        return commits;
    }

    // The assistant walk (transport.ts:846-881): per-part projections
    // with the showTools()/thinking visibility gates.
    let has_tools = parts.iter().any(|part| part.get("type").and_then(Value::as_str) == Some("tool"));
    let mut text_ordinal = 0usize;
    let mut reasoning_ordinal = 0usize;
    for part in &parts {
        let part_id_of = |update_key: &str| {
            update_key.split('\u{0}').nth(1).unwrap_or_default().to_owned()
        };
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                let fragment_text = text(part);
                let visible = settings.tools || !has_tools;
                let update =
                    transport.project_part(&message_id, "text", text_ordinal, fragment_text, visible);
                text_ordinal += 1;
                if visible && fragment_text.len() > update.previous.len() {
                    let suffix = &fragment_text[update.previous.len()..];
                    commits.push(Commit {
                        kind: CommitKind::Assistant,
                        phase: Phase::Progress,
                        text: suffix.to_owned(),
                        message_id: Some(message_id.clone()),
                        part_id: Some(part_id_of(&update.key)),
                        ..Commit::default()
                    });
                }
            }
            Some("reasoning") => {
                let fragment_text = text(part);
                let visible = settings.thinking;
                let update = transport.project_part(
                    &message_id,
                    "reasoning",
                    reasoning_ordinal,
                    fragment_text,
                    visible,
                );
                reasoning_ordinal += 1;
                if visible && fragment_text.len() > update.previous.len() {
                    // The ref prefixes the FIRST run
                    // (`update.previous.length === 0 ?
                    // `Thinking: ${item.text}` : suffix`,
                    // transport.ts:873-876).
                    let suffix = &fragment_text[update.previous.len()..];
                    let row = if update.previous.is_empty() {
                        format!("Thinking: {fragment_text}")
                    } else {
                        suffix.to_owned()
                    };
                    commits.push(Commit {
                        kind: CommitKind::Reasoning,
                        phase: Phase::Progress,
                        text: row,
                        message_id: Some(message_id.clone()),
                        part_id: Some(part_id_of(&update.key)),
                        ..Commit::default()
                    });
                }
            }
            // `renderTool`'s image arm (transport.ts:758): the part's
            // `data:image/*` display content commits.
            Some("tool") => {
                let part_id = format!("prt_{}", part.get("id").and_then(Value::as_str).unwrap_or_default());
                let state = part.get("state").cloned().unwrap_or(Value::Null);
                commits.extend(transport.commit_tool_images(&part_id, &state, &message_id));
            }
            _ => {}
        }
    }
    commits
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
        KeyCode::Tab => Some(FooterKey::Tab),
        KeyCode::PageUp => Some(FooterKey::PageUp),
        KeyCode::PageDown => Some(FooterKey::PageDown),
        KeyCode::Char(char) => Some(FooterKey::Printable(char)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G-68 (fix #1) — the replay's production wiring: projections
    /// land in the SAME reconciler the live stream folds through.
    /// A replayed assistant part's text becomes the committed prefix
    /// (a re-replay never double-prints, a live covered delta is
    /// consumed, an `ended` without text keeps the full projection),
    /// and the user/tool image arms fire on the replay walk.
    #[test]
    fn replay_projects_fragments_and_commits_images() {
        let settings = MiniSettings { tools: false, thinking: true, ..MiniSettings::default() };
        let mut transport = TransportState::new("s1");

        // A user message with an image file: text row + image commit.
        let user = serde_json::json!({
            "id": "msg_u1", "type": "user",
            "parts": [{ "type": "text", "text": "look" }],
            "files": [{ "mime": "image/png", "data": "AAEC", "name": "shot.png" }],
        });
        let commits = replay_message_commits(&user, 0, &mut transport, &settings);
        assert_eq!(commits.len(), 2, "the text row + the image commit");
        assert_eq!(commits[0].kind, CommitKind::User);
        assert_eq!(commits[1].image.as_deref(), Some("data:image/png;base64,AAEC"));

        // An assistant message: the text part projects visibly (no
        // tools in the message) with the `text:0` part id.
        let assistant = serde_json::json!({
            "id": "msg_a1", "type": "assistant",
            "parts": [
                { "type": "reasoning", "text": "hmm" },
                { "type": "text", "text": "one two three" },
            ],
        });
        let commits = replay_message_commits(&assistant, 1, &mut transport, &settings);
        assert_eq!(commits.len(), 2, "one reasoning + one text row");
        assert_eq!(commits[0].kind, CommitKind::Reasoning);
        assert_eq!(commits[0].text, "Thinking: hmm");
        assert_eq!(commits[1].kind, CommitKind::Assistant);
        assert_eq!(commits[1].text, "one two three");
        assert_eq!(commits[1].part_id.as_deref(), Some("text:0"));

        // The projection covers the live stream: covered deltas fold
        // to Ignored, the tail appends, the final commit keeps all.
        let covered = transport.fold(&serde_json::json!({
            "type": "session.text.delta",
            "data": { "sessionID": "s1", "messageID": "msg_a1", "partID": "text:0", "delta": "one two " },
        }));
        assert_eq!(covered, super::super::transport::Fold::Ignored, "covered by the replay");
        let tail = transport.fold(&serde_json::json!({
            "type": "session.text.delta",
            "data": { "sessionID": "s1", "messageID": "msg_a1", "partID": "text:0", "delta": " four" },
        }));
        match tail {
            super::super::transport::Fold::Commit(commit) => assert_eq!(commit.text, "one two three four"),
            other => panic!("tail folded: {other:?}"),
        }
        let ended = transport.fold(&serde_json::json!({
            "type": "session.text.ended",
            "data": { "sessionID": "s1", "messageID": "msg_a1", "partID": "text:0" },
        }));
        match ended {
            super::super::transport::Fold::Commit(commit) => {
                assert_eq!(commit.text, "one two three four", "the final keeps the projection");
            }
            other => panic!("ended folded: {other:?}"),
        }

        // A re-replay of the SAME message never double-prints: the
        // committed prefix swallows the projection.
        let re = replay_message_commits(&assistant, 1, &mut transport, &settings);
        assert!(re.is_empty(), "a fully-committed part replays nothing: {re:?}");

        // A tool part's data:image/* display content commits (kind
        // Tool), and replays dedup.
        let tool = serde_json::json!({
            "id": "msg_a2", "type": "assistant",
            "parts": [
                { "type": "text", "text": "here" },
                { "type": "tool", "id": "9", "state": { "content": [
                    { "type": "file", "mime": "image/png", "uri": "data:image/png;base64,AAEC", "name": "out.png" },
                ] } },
            ],
        });
        let commits = replay_message_commits(&tool, 2, &mut transport, &settings);
        // showTools() false + the message HAS a tool part → the text
        // row hides (transport.ts:854's visible gate).
        assert_eq!(commits.len(), 1, "only the tool image commit");
        assert_eq!(commits[0].kind, CommitKind::Tool);
        assert_eq!(commits[0].part_id.as_deref(), Some("prt_9:image:0"));
        assert!(replay_message_commits(&tool, 2, &mut transport, &settings).is_empty());
    }
}
