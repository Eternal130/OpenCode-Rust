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
//! port drives the same seams from one tick loop (the opentui input
//! pipeline → footer keys; SSE channel → transport folds; queue →
//! prompt turns),
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

/// The prompt-loop poll tick (`app.rs`'s 100 ms tick).
const TICK: Duration = Duration::from_millis(100);

/// G-181④ — the reconnect cadence (`wait(250, signal)`,
/// `stream-v2.transport.ts:1543`).
const RECONNECT_DELAY: Duration = Duration::from_millis(250);

/// G-181④ — the reconnect client factory (`Reconnect`,
/// runtime.ts:50 — `(signal) => Promise<Client>`): re-resolves the
/// endpoint and hands back a fresh client for the next subscription
/// attempt (`sdk = next; input.onClient?.(next)`,
/// stream-v2.transport.ts:1533-1539). `None` when the endpoint is
/// non-managed (ref leaves `reconnect` undefined there).
pub type ReconnectFactory = std::sync::Arc<
    dyn Fn() -> futures::future::BoxFuture<'static, Option<oc_client::Client>> + Send + Sync,
>;

/// G-181④ — the stream pump's private message vocabulary: a live SSE
/// event, or the internal reconnect notice (never an SSE event type —
/// the loop's own vocabulary).
#[derive(Debug)]
enum StreamMessage {
    Event(Value),
    /// A fresh subscription after a drop — carries the (possibly
    /// swapped) client instance for the runtime to adopt.
    Reconnected(Client),
}

/// G-181④ — the stream pump (the `connect` loop,
/// `stream-v2.transport.ts:1450-1545`): subscribe `/api/event`; on a
/// drop, resolve a FRESH client through the reconnect factory (the
/// ref swaps `sdk` before the retry), wait the 250 ms cadence, and
/// resubscribe — every RE-connect announces itself so the loop
/// replays the session's transcript through the same reconciler the
/// live stream folds through.
async fn stream_pump(
    mut client: Client,
    reconnect: Option<ReconnectFactory>,
    event_tx: tokio::sync::mpsc::Sender<StreamMessage>,
    ready: Option<tokio::sync::oneshot::Sender<()>>,
) {
    let mut ready = ready;
    let mut subscriptions = 0usize;
    loop {
        if let Ok(mut stream) = client.event().subscribe().await {
            subscriptions += 1;
            if subscriptions > 1 {
                // The reconnect notice (a fresh connection after a
                // drop — the SSE event vocabulary never carries this).
                if event_tx.send(StreamMessage::Reconnected(client.clone())).await.is_err() {
                    return;
                }
            }
            // Signal readiness on the FIRST live subscription (a dead
            // server falls through to the reconnect cadence; the
            // composer still runs after the wait below).
            if let Some(sender) = ready.take() {
                let _ = sender.send(());
            }
            while let Ok(Some(event)) = stream.next_json().await {
                if event_tx.send(StreamMessage::Event(event)).await.is_err() {
                    return;
                }
            }
        }
        // The ref resolves the fresh client BEFORE the retry wait
        // (stream-v2.transport.ts:1530-1542: `sdk = next` then
        // `wait(250, signal)`).
        if let Some(factory) = &reconnect {
            if let Some(next) = factory().await {
                client = next;
            }
        }
        // `wait(250, signal)` — the ref's reconnect cadence
        // (stream-v2.transport.ts:1543).
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

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
    /// `thinking=show`; tool commits need `tools=show` (the DIRECT-
    /// shell rows always flow — the ref's shell writes are not
    /// `showTools()`-gated, transport.ts:1125-1164); the skill rows
    /// follow the tools gate at the write site (transport.ts:612,
    /// :1083 — the `tool: "skill"` marker); everything else always
    /// flows.
    #[must_use]
    pub fn commit_visible(&self, commit: &super::scrollback::Commit) -> bool {
        use super::scrollback::CommitKind;
        match commit.kind {
            CommitKind::Reasoning => self.thinking,
            CommitKind::Tool => commit.shell.is_some() || self.tools,
            CommitKind::System if commit.tool.as_deref() == Some("skill") => self.tools,
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
    /// The resolved mini display settings (`resolveMiniSettings` over the
    /// cli.json `mini` section — CONFIG-T4). `None` resolves from the
    /// global config here, like v2's `resolveRunTuiConfig` default
    /// path (the CLI host may pre-resolve for tests).
    pub settings: Option<MiniSettings>,
    /// G-181④ — the reconnect client factory (`input.reconnect`,
    /// mini.ts:144-153 → stream-v2.transport.ts:1530-1542): resolves
    /// a FRESH client for the next subscription attempt when the
    /// stream drops. `None` = non-managed endpoint (no swap).
    pub reconnect: Option<ReconnectFactory>,
}

/// The runtime loop. Returns the process exit status.
///
/// Exit semantics (`runtime.ts:1081-1091`): teardown aborts the
/// runtime, closes the shell with `showExit = shown && sessionID` —
/// the exit splash only prints once a turn actually rendered.
pub async fn run(input: MiniInput) -> Result<i32, String> {
    // G-181① — the model-variant preference repository is REALLY
    // bound now (`createMiniHost`'s `preferences`,
    // `mini-host.ts:18-30`): the resolved target's `provider/model`
    // label resolves its persisted variant override and lands in the
    // recent list, exactly the seams `model-preference.ts` exposes to
    // mini's footer (the model picker panel itself arrives with the
    // picker family).
    let preferences = ModelPreferenceRepository::new(input.state_dir.join("model.json"));
    let boot_model = bind_model_preference(&preferences, input.target.model_label.as_deref());
    // G-181① — the variant-cycle state (`state.model ?? state.defaultModel`
    // + `state.variants`, runtime.ts:119-121/142-147): the boot target's
    // model and its catalog variants (best-effort — an unreachable
    // catalog means "no variants available", exactly the ref's empty-
    // `variantsFor` arm).
    let boot_target = input.target.model_label.as_deref().and_then(parse_model_label);
    let active_variant =
        boot_target.as_ref().and_then(|target| preferences.resolve_variant(target));
    // G-272 — the boot catalog fetch (`loadCatalog`, runtime.ts:601-
    // 611 over catalog.shared.ts): the agent list, the model catalog
    // (the picker's provider shape + the per-model context limits),
    // and the boot model's variants. Best-effort — an unreachable
    // catalog means "no variants available", exactly the ref's empty
    // `variantsFor` arm.
    let (variants, agents, providers, context_limits) = if input.demo {
        (Vec::new(), Vec::new(), Vec::new(), std::collections::HashMap::new())
    } else {
        match fetch_catalog(&input.client, &input.target.directory).await {
            Some(catalog) => (
                catalog.variants_for(boot_target.as_ref()),
                catalog.agents,
                catalog.providers,
                catalog.limits,
            ),
            None => (Vec::new(), Vec::new(), Vec::new(), std::collections::HashMap::new()),
        }
    };
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
        // G-181①: the variant-resolved label the preference seam
        // produced (the ref's `formatModelLabel(input.model,
        // input.variant)`, `runtime.lifecycle.ts:214`).
        model_label: boot_model.clone(),
        splash: settings.splash,
        mono: settings.mono,
        footer: settings.footer,
        work_spinner: settings.work_spinner.clone(),
        shell_output: settings.shell_output,
    })?;
    // G-272 — the catalog lands in the footer (`applyCatalog` →
    // `footer.event({ type: "catalog", agents })`, runtime.ts:575-
    // 590): the agent/model panels read it.
    lifecycle.pane.agents = agents.clone();
    lifecycle.pane.providers = providers.clone();
    lifecycle.pane.variants = variants.clone();
    lifecycle.pane.active_variant = active_variant.clone();
    // G-272 — the settings panel's entry state (CONFIG-T4's resolved
    // knobs; the runtime's gates stay authoritative).
    lifecycle.pane.settings_summary = crate::components::footer::state::MiniSettings {
        thinking: if settings.thinking { "show" } else { "hide" },
        tools: if settings.tools { "show" } else { "hide" },
        shell_output: if settings.shell_output { "show" } else { "hide" },
        turn_summary: if settings.turn_summary { "show" } else { "hide" },
        footer: if settings.footer { "show" } else { "hide" },
        splash: if settings.splash { "show" } else { "hide" },
        work_spinner: "block-soft-slide",
        mono: settings.mono,
    };
    // G-272 — `setTitle(input.sessionTitle)` at boot
    // (runtime.lifecycle.ts:180): the OSC 2 window title tracks the
    // session (fallback titles map to "OpenCode").
    lifecycle.set_title(input.target.session_title.as_deref());
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
    } else if input.target.resume {
        // G-272 — the hydrate's blocker sweep runs on EVERY connect
        // (`hydrate`, transport.ts:952-1004): `--no-replay` skips the
        // transcript render, never the pending permission/form
        // restoration.
        hydrate_blockers(&mut lifecycle, &client, &session_id, &mut transport).await;
    }

    // The stream transport (`ensureStream`, `runtime.ts:776-831`):
    // subscribe `/api/event` once; folds flow back over a channel and
    // the loop resubscribes on disconnect. G-181④ — the ref's
    // reconnect semantics (`stream-v2.transport.ts:1528-1543`): the
    // 250 ms cadence (`wait(250)`), and every RE-connect emits the
    // hydrate marker so the loop replays the session's transcript
    // through the same reconciler the live stream folds through
    // (`hydrate(attempt, { render: true, reconnect: true })`,
    // transport.ts:952-991 — missed deltas backfill, committed parts
    // never double-print). The queue never runs before the
    // subscription is live (v2 prewarms the transport after
    // firstPaint, `runtime.ts:1055-1069`).
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<StreamMessage>(256);
    // G-272 — the SIGINT subscription (`host.signals.sigint`
    // → `footer.requestExit()`, runtime.lifecycle.ts:275-295): a
    // real `kill -INT` (or the terminal's INT delivery when raw mode
    // missed the chord) routes through the SAME ctrl+c semantics —
    // first press clears a live draft, second press within 5 s exits.
    // The handler task only forwards presses; the loop applies them.
    let (sigint_tx, sigint_rx) = tokio::sync::mpsc::unbounded_channel();
    let sigint_task = tokio::spawn(async move {
        if let Ok(mut stream) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        {
            while stream.recv().await.is_some() {
                if sigint_tx.send(()).is_err() {
                    break;
                }
            }
        }
    });
    let (stream_ready_tx, stream_ready_rx) = tokio::sync::oneshot::channel::<()>();
    let stream_task = tokio::spawn(stream_pump(
        client.clone(),
        input.reconnect.clone(),
        event_tx,
        Some(stream_ready_tx),
    ));
    // Prewarm gate: wait for the subscription (or the timeout — an
    // unreachable server must not wedge the composer).
    let _ = tokio::time::timeout(Duration::from_secs(5), stream_ready_rx).await;

    // G-221 — the replay's inbox seed already synced the pane; the
    // dirty-check anchor starts there (`syncPending`).
    let queued_synced = transport.pending_prompts().len();
    let mut runtime = Runtime {
        lifecycle,
        keys: crate::input_term::KeyPipeline::new(),
        client,
        session_id: session_id.clone(),
        session_title: input.target.session_title.clone(),
        transport,
        queue: PromptQueue::new(true),
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
        turn_model: boot_model,
        subagents: super::transport::SubagentTracker::new(&session_id),
        preferences,
        directory: input.target.directory.clone(),
        replay_limit: input.replay_limit,
        needs_hydrate: false,
        queued_synced,
        boot_target,
        variants,
        active_variant,
        agents,
        providers,
        context_limits,
        needs_catalog: false,
        next_agent: None,
        next_model: None,
        sigint_rx,
        persist_settings: input.settings.is_none(),
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
        runtime.pump_sigint();
        runtime.pump_events();
        // G-181④ — a reconnect marker replays the session transcript
        // through the SAME reconciler the live folds consume (the
        // ref's `hydrate(attempt, { render: true, reconnect: true })`).
        while runtime.needs_hydrate {
            runtime.needs_hydrate = false;
            replay_history(
                &mut runtime.lifecycle,
                &runtime.client,
                &runtime.session_id.clone(),
                runtime.replay_limit,
                &mut runtime.transport,
                &runtime.settings,
            )
            .await;
            // G-272 — every (re)connect refreshes the catalogs
            // (`void refreshCatalog(attempt)` after hydrate,
            // transport.ts:1517).
            runtime.needs_catalog = true;
        }
        // G-272 — a catalog event (provider/model/agent/command/…
        // `.updated`, credential.switched) re-fetches the catalogs
        // (`refreshCatalog` → `requestCatalogRefresh`,
        // transport.ts:1009-1014 → runtime.ts:690-733).
        while runtime.needs_catalog {
            runtime.needs_catalog = false;
            if !runtime.demo {
                if let Some(catalog) =
                    fetch_catalog(&runtime.client, &runtime.directory.clone()).await
                {
                    runtime.variants = catalog.variants_for(runtime.boot_target.as_ref());
                    runtime.agents = catalog.agents;
                    runtime.providers = catalog.providers;
                    runtime.context_limits = catalog.limits;
                    runtime.lifecycle.pane.agents = runtime.agents.clone();
                    runtime.lifecycle.pane.providers = runtime.providers.clone();
                    runtime.lifecycle.pane.variants = runtime.variants.clone();
                }
            }
        }
        runtime.drain_queue().await;
        // G-181③ — the resize watch (ADR-0018's ≤50 ms TIOCGWINSZ
        // seam): a changed size re-renders through the engine at the
        // new geometry (the registered no-reflow deviation is closed).
        if runtime.lifecycle.poll_resize() {
            runtime.lifecycle.flush()?;
        }
        // CONFIG-T4 — `busy()` (`footer.view.tsx:980`): a running turn
        // anchors the footer's work-spinner cell.
        runtime.lifecycle.pane.set_busy(runtime.transport.is_running());
        runtime.lifecycle.flush()?;
        tokio::time::sleep(TICK).await;
    }
    stream_task.abort();
    sigint_task.abort();
    Ok(runtime.exit_code)
}

/// Everything the loop drives (the `RuntimeState`,
/// `runtime.ts:115-134`).
struct Runtime {
    lifecycle: Lifecycle,
    /// Lane B3a — the footer key pipeline (opentui parse; replaces the
    /// crossterm poll/read pair).
    keys: crate::input_term::KeyPipeline,
    client: Client,
    session_id: String,
    session_title: Option<String>,
    transport: TransportState,
    queue: PromptQueue,
    history: PromptHistory,
    pending: Vec<RunPrompt>,
    events: tokio::sync::mpsc::Receiver<StreamMessage>,
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
    /// G-181① — the bound preference repository
    /// (`<state>/model.json`, the host `preferences` seam).
    preferences: ModelPreferenceRepository,
    /// The session's directory (the `/new` factory's location).
    directory: String,
    replay_limit: Option<usize>,
    /// G-181④ — a reconnect notice arrived (the hydrate replay is
    /// due on the next loop step).
    needs_hydrate: bool,
    /// G-221 — the last footer-synced pending-prompt count
    /// (`syncPending`'s dirty check, transport.ts:582-590).
    queued_synced: usize,
    /// G-181① — the resolved target's model (`state.model ??
    /// state.defaultModel`, runtime.ts:294) the variant cycle edits.
    boot_target: Option<super::model_pref::ModelPreferenceModel>,
    /// G-181① — `state.variants` (variant ids for the boot model,
    /// `variantsFor(providers, model)`, runtime.ts:142-147 — fetched
    /// best-effort from the model catalog at boot).
    variants: Vec<String>,
    /// G-181① — `state.activeVariant`, resolved from the persisted
    /// preference at boot (`resolveVariant`, runtime.ts:204-207).
    active_variant: Option<String>,
    /// G-272 — the agent catalog (`state.agents` via
    /// `loadRunAgents`, runtime.ts:592-611): the footer's agent
    /// panel + `agentColor` source.
    agents: Vec<crate::components::footer::panels::RunAgent>,
    /// G-272 — the model catalog's picker shape (`state.providers`,
    /// `loadRunProviders`): the footer's model panel.
    providers: Vec<crate::components::footer::panels::RunProvider>,
    /// G-272 — `provider/model → limit.context` (the usage percent's
    /// denominator, `input.contextLimit?.(state.stepModel)`,
    /// transport.ts:1368).
    context_limits: std::collections::HashMap<(String, String), u64>,
    /// G-272 — a catalog event arrived (`catalogEvents` →
    /// `refreshCatalog`, transport.ts:383-392, 1009-1014): the loop
    /// re-fetches the catalogs.
    needs_catalog: bool,
    /// G-272 — the pending per-turn agent (`state.agent` +
    /// `session.switchAgent`, stream-v2.transport.ts:1799/1834):
    /// applied before the next prompt POST, then consumed.
    next_agent: Option<String>,
    /// G-272 — the pending per-turn model (`resolveSelectedModel` →
    /// `session.switchModel`, stream-v2.transport.ts:1804/1839).
    next_model: Option<(String, String)>,
    /// G-272 — the SIGINT presses (the `host.signals.sigint`
    /// subscription, runtime.lifecycle.ts:275-295): each press behaves
    /// exactly like the ctrl+c chord key (first press clears a draft,
    /// second within 5 s exits).
    sigint_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    /// G-272 — whether settings changes persist to the global cli.json
    /// (the production default path; injected test settings stay
    /// in-memory).
    persist_settings: bool,
}

impl Runtime {
    /// Drain the tty's pending bytes and route footer keys
    /// (`RunFooter`'s input layer; lane B3a — the opentui parse
    /// pipeline replaces the crossterm poll/read pair).
    async fn pump_keys(&mut self) {
        for event in self.keys.process(&read_footer_input()) {
            let crate::input_term::TermEvent::Key(spec) = event else { continue };
            let Some(footer_key) = footer_key_from_spec(&spec) else { continue };
            for effect in self.lifecycle.pane.press(footer_key) {
                self.apply_effect(effect).await;
            }
        }
    }

    /// G-272 — drain pending SIGINT presses (`attachSigint` →
    /// `footer.requestExit()`, runtime.lifecycle.ts:275-295): each
    /// press is the ctrl+c chord's semantics (clear the draft first,
    /// then the two-press exit window).
    fn pump_sigint(&mut self) {
        while let Ok(()) = self.sigint_rx.try_recv() {
            for effect in self.lifecycle.pane.press(FooterKey::CtrlC) {
                // The press arms/clears inside the pane; only a FULL
                // exit press-count emits the Close effect (the
                // futures-only runtime can't await inside `press`).
                if let FooterEffect::Close = effect {
                    self.lifecycle.close(
                        self.shown && !self.session_id.is_empty(),
                        self.session_title.as_deref(),
                        Some(&self.session_id),
                    );
                }
            }
        }
    }

    async fn apply_effect(&mut self, effect: FooterEffect) {
        match effect {
            FooterEffect::Submit(text) => {
                self.history.push(&text);
                // G-272 — the `!` shell passthrough (`setShellMode`'s
                // observable gesture: `!` at cursor 0 toggles shell
                // MODE, the submitted text minus the bang is the
                // command — `runShellTurn`,
                // stream-v2.transport.ts:1561-1592).
                let prompt = match super::prompt_shared::parse_shell_command(&text) {
                    Some(command) => {
                        RunPrompt { text: command.to_owned(), shell: true, ..RunPrompt::default() }
                    }
                    None => RunPrompt { text, ..RunPrompt::default() },
                };
                self.pending.push(prompt);
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
            // G-272 — the agent panel's pick (`state.agent` +
            // `switchAgent` before the next turn,
            // stream-v2.transport.ts:1799/1834): the status mirrors
            // the ref's `agent {id}` write (runtime.ts:337-352).
            FooterEffect::AgentSelect(agent) => {
                self.next_agent = Some(agent.clone());
                self.turn_agent = Some(agent.clone());
                self.lifecycle.append(Commit::system(&format!("agent {agent}")));
            }
            // G-272 — the model panel's pick (`switchModel` before
            // the next turn); the label feeds the footer + turn
            // summary (`formatModelLabel`).
            FooterEffect::ModelSelect { provider_id, model_id } => {
                let label = format!("{provider_id}/{model_id}");
                self.next_model = Some((provider_id, model_id));
                self.turn_model = Some(label.clone());
                self.lifecycle.append(Commit::system(&label));
            }
            // G-272 — the variant panel's pick (`onVariantSelect` →
            // `saveVariant`, runtime.ts:360-374): persist the choice.
            FooterEffect::VariantSelect(variant) => {
                self.active_variant = variant.clone();
                self.lifecycle.pane.active_variant = variant.clone();
                if let Some(target) = &self.boot_target {
                    self.preferences.save_variant(target, variant.as_deref());
                }
                let status = variant
                    .as_deref()
                    .map(|variant| format!("variant {variant}"))
                    .unwrap_or_else(|| "variant default".to_owned());
                self.lifecycle.append(Commit::system(&status));
            }
            // G-272 — a queued-prompt action (`onQueuedPromptAction`,
            // runtime.ts:394-402): steer/queue PATCH the delivery,
            // cancel DELETEs the row; failures surface as system rows
            // (`failed to … pending prompt: …`, footer.view.tsx:340).
            FooterEffect::QueuedPromptAction { action, inbox_id } => {
                if !self.demo {
                    if let Err(error) = transport::inbox_prompt_action(
                        &self.client,
                        &self.session_id,
                        &inbox_id,
                        action,
                    )
                    .await
                    {
                        self.lifecycle.append(Commit::system(&format!(
                            "failed to {action} pending prompt: {error}"
                        )));
                    }
                }
            }
            // G-272 — `session.background` (runtime.ts:386-392).
            FooterEffect::Background => {
                if !self.demo {
                    let _ = transport::session_background(&self.client, &self.session_id).await;
                }
            }
            // G-272 — a settings change (`onMiniSettingChange`,
            // runtime.ts:253-266): apply the preset/knob to the live
            // gates + the pane's summary and persist through the
            // cli.json write-through (`config.update`).
            FooterEffect::SettingChange { key, value } => {
                self.apply_setting_change(key, &value);
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
            // G-181① — `onCycleVariant` (runtime.ts:293-310): cycle
            // the active variant, persist it through the preference
            // repository (`saveVariant`), and surface the status.
            FooterEffect::CycleVariant => {
                match cycle_variant_decision(
                    self.boot_target.as_ref(),
                    self.active_variant.as_deref(),
                    &self.variants,
                ) {
                    CycleVariantDecision::NoVariants => {
                        self.lifecycle.append(Commit::system("no variants available"));
                    }
                    CycleVariantDecision::Cycle { next, status, label } => {
                        self.active_variant = next.clone();
                        // G-272 — the variant panel's `current` marker
                        // follows the cycle.
                        self.lifecycle.pane.active_variant = next.clone();
                        if let Some(target) = &self.boot_target {
                            self.preferences.save_variant(target, next.as_deref());
                        }
                        self.turn_model = label;
                        self.lifecycle.append(Commit::system(&status));
                    }
                }
            }
        }
    }

    /// G-272 — apply one settings change (`applyMiniSettingChange`,
    /// verbosity.ts:73-76 + `config.update`, runtime.ts:253-266): the
    /// live gates update immediately; the cli.json `mini` section
    /// persists write-through (production only — injected test
    /// settings never touch the file).
    fn apply_setting_change(&mut self, key: &str, value: &str) {
        let settings = &mut self.settings;
        let mut touched: Vec<(&'static str, String)> = Vec::new();
        match key {
            // The verbosity preset assigns ALL five knobs
            // (`Object.assign(draft.mini, verbosityPreset(value))`).
            "verbosity" => {
                let mut preset =
                    |thinking: bool, tools: bool, shell: bool, turn: bool, footer: bool| {
                        let on = |flag: bool| if flag { "show" } else { "hide" };
                        touched.extend([
                            ("thinking", on(thinking).to_string()),
                            ("tools", on(tools).to_string()),
                            ("shell_output", on(shell).to_string()),
                            ("turn_summary", on(turn).to_string()),
                            ("footer", on(footer).to_string()),
                        ]);
                        settings.thinking = thinking;
                        settings.tools = tools;
                        settings.shell_output = shell;
                        settings.turn_summary = turn;
                        settings.footer = footer;
                    };
                match value {
                    "quiet" => preset(false, false, false, false, false),
                    "everything" => preset(true, true, true, true, true),
                    _ => preset(true, false, false, true, true),
                }
            }
            "thinking" => {
                settings.thinking = value == "show";
                touched.push(("thinking", value.to_owned()));
            }
            "tools" => {
                settings.tools = value == "show";
                touched.push(("tools", value.to_owned()));
            }
            "shell_output" => {
                settings.shell_output = value == "show";
                touched.push(("shell_output", value.to_owned()));
            }
            "turn_summary" => {
                settings.turn_summary = value == "show";
                touched.push(("turn_summary", value.to_owned()));
            }
            "footer" => {
                settings.footer = value == "show";
                touched.push(("footer", value.to_owned()));
            }
            "splash" => {
                settings.splash = value == "show";
                touched.push(("splash", value.to_owned()));
            }
            "mono" => {
                settings.mono = value == "true";
                self.lifecycle.pane.mono = settings.mono;
                touched.push(("mono", value.to_owned()));
            }
            "work_spinner" => {
                settings.work_spinner = value.to_owned();
                self.lifecycle.pane.work_spinner = value.to_owned();
                touched.push(("work_spinner", value.to_owned()));
            }
            _ => {}
        }
        // The pane's summary mirror (`miniSettings()` reads).
        let show = |on: bool| if on { "show" } else { "hide" };
        let spinner = crate::components::footer::state::WORK_SPINNERS
            .iter()
            .find(|literal| **literal == self.settings.work_spinner)
            .copied()
            .unwrap_or("block-soft-slide");
        let summary = &mut self.lifecycle.pane.settings_summary;
        summary.thinking = show(self.settings.thinking);
        summary.tools = show(self.settings.tools);
        summary.shell_output = show(self.settings.shell_output);
        summary.turn_summary = show(self.settings.turn_summary);
        summary.footer = show(self.settings.footer);
        summary.splash = show(self.settings.splash);
        summary.mono = self.settings.mono;
        summary.work_spinner = spinner;
        self.lifecycle.pane.footer_details = self.settings.footer;
        // `config.update((draft) => { draft.mini[key] = value })`
        // (runtime.ts:255-262) — only on the production config path.
        if self.persist_settings {
            let service = oc_core::cli_config::CliConfig::open(&oc_util::global_roots::resolve());
            let result = service.update(|draft| {
                let mini = draft
                    .as_object_mut()
                    .map(|object| {
                        object
                            .entry("mini")
                            .or_insert(serde_json::Value::Object(Default::default()))
                    })
                    .map(serde_json::Value::as_object_mut)
                    .unwrap_or_default();
                if let Some(mini) = mini {
                    for (key, value) in &touched {
                        let json = if *key == "mono" {
                            serde_json::Value::Bool(value == "true")
                        } else {
                            serde_json::Value::String(value.clone())
                        };
                        mini.insert((*key).to_owned(), json);
                    }
                }
            });
            if let Err(error) = result {
                self.lifecycle.append(Commit::system(&format!("failed to save setting: {error}")));
            }
        }
    }

    /// G-272 — the per-turn switches (`runPromptTurn`'s head,
    /// stream-v2.transport.ts:1832-1840): the pending agent/model
    /// land BEFORE the prompt POST (`switchAgent` then
    /// `switchModel`), then consume.
    async fn apply_turn_switches(&mut self) {
        if self.demo {
            self.next_agent = None;
            self.next_model = None;
            return;
        }
        if let Some(agent) = self.next_agent.take() {
            if let Err(error) =
                transport::switch_agent(&self.client, &self.session_id, &agent).await
            {
                self.lifecycle.append(Commit::error(&error));
            }
        }
        if let Some((provider, model)) = self.next_model.take() {
            if let Err(error) =
                transport::switch_model(&self.client, &self.session_id, &provider, &model).await
            {
                self.lifecycle.append(Commit::error(&error));
            }
        }
    }

    /// Drain arrived SSE events through the transport folds.
    fn pump_events(&mut self) {
        while let Ok(message) = self.events.try_recv() {
            let event = match message {
                // G-181④ — the reconnect notice: adopt the fresh
                // client instance (the ref's `sdk = next;
                // input.onClient?.(next)`, stream-v2.transport.ts:
                // 1533-1539) and arm the hydrate replay; the ref
                // reconnect path rehydrates BEFORE live events resume
                // folding.
                StreamMessage::Reconnected(client) => {
                    self.client = client;
                    self.needs_hydrate = true;
                    continue;
                }
                StreamMessage::Event(event) => event,
            };
            // G-272 — `catalogEvents` (transport.ts:383-392, 1009-
            // 1014): a catalog mutation at THIS location re-fetches
            // the catalogs (`input.location && event.location &&
            // event.location.directory !== input.location.directory`
            // returns — a missing location passes).
            if let Some(kind) = event.get("type").and_then(Value::as_str) {
                if matches!(
                    kind,
                    "provider.updated"
                        | "model.updated"
                        | "integration.updated"
                        | "credential.switched"
                        | "agent.updated"
                        | "command.updated"
                        | "skill.updated"
                        | "reference.updated"
                ) {
                    let directory = event
                        .pointer("/location/directory")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if directory.is_empty() || directory == self.directory {
                        self.needs_catalog = true;
                    }
                }
            }
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
                // G-221 — a multi-row event renders row-for-row
                // through the same gates (`write(commits)`,
                // stream-v2.transport.ts).
                Fold::Commits(commits) => {
                    for commit in commits {
                        if !self.settings.commit_visible(&commit) {
                            continue;
                        }
                        self.shown = true;
                        self.lifecycle.append(commit);
                    }
                }
                Fold::Idle => {}
                // G-272 — `session.step.ended`'s usage patch
                // (transport.ts:1361-1381): the statusline's context/
                // cost groups (footer.view.tsx:213-218, 493-500).
                Fold::Usage { model, tokens, cost } => {
                    let percent = model.as_ref().and_then(|model| {
                        let limit = self.context_limits.get(model).copied().unwrap_or(0);
                        if limit == 0 {
                            return None;
                        }
                        // `Math.round((total / limit) * 100)`
                        // (transport.ts:1376).
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let ratio = (tokens as f64 / limit as f64) * 100.0;
                        Some((ratio + 0.5) as u32)
                    });
                    self.lifecycle.pane.usage =
                        Some(super::lifecycle::UsageInfo { tokens, percent, cost });
                }
                // G-272 — the ROOT session's rename
                // (`input.onSessionTitle`, transport.ts:1036-1039 →
                // `shell.setTitle(state.sessionTitle)`,
                // runtime.ts:809): the OSC 2 title follows.
                Fold::SessionTitle(title) => {
                    self.session_title = Some(title.clone());
                    self.lifecycle.set_title(Some(&title));
                }
            }
            // G-221 — `syncPending` (transport.ts:582-590): the
            // footer's `queued.prompts` view follows the transport's
            // pending list after every event (the dirty-check push —
            // the statusline's `N pending` group, footer.view.tsx:
            // 246). G-272 — the queued MENU rides the same push (the
            // action rows: inbox id + text + delivery).
            let queued = self.transport.pending_prompts().len();
            if queued != self.queued_synced {
                self.queued_synced = queued;
                self.lifecycle.pane.queued_pending = queued;
                self.lifecycle.pane.queued_prompts = self
                    .transport
                    .pending_prompts()
                    .iter()
                    .map(|prompt| super::lifecycle::QueuedPromptView {
                        message_id: prompt.message_id.clone(),
                        text: prompt.text.clone(),
                        delivery: prompt.delivery.clone(),
                    })
                    .collect();
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
                // G-181② — the session factory is wired
                // (`onNewSession: createSession`, `runtime.ts:933-1018`
                // → `runtime.queue.ts:98-107`): `/new` creates the
                // session server-side at the same location/agent and
                // swaps the runtime's targets (a fresh transport +
                // subagent tracker + history; the system row lands).
                // Demo mode keeps the offline note.
                if self.demo {
                    self.lifecycle.append(Commit::system("new sessions unavailable"));
                } else {
                    match create_session(
                        &self.client,
                        &self.directory,
                        self.lifecycle.pane.agent.as_deref(),
                    )
                    .await
                    {
                        Ok(new_id) => {
                            self.session_id = new_id.clone();
                            self.session_title = None;
                            // G-272 — a fresh session's title resets
                            // (`setTitle(undefined)` → "OpenCode").
                            self.lifecycle.set_title(None);
                            self.transport = TransportState::new(&new_id);
                            self.subagents = super::transport::SubagentTracker::new(&new_id);
                            self.queue = PromptQueue::new(true);
                            self.history = PromptHistory::new();
                            self.pending.clear();
                            self.shown = false;
                            self.turn_idle = false;
                            self.lifecycle.pane.subagent_children = Vec::new();
                            // G-221 — the fresh session's inbox view
                            // starts empty (the transport was reset).
                            self.queued_synced = 0;
                            self.lifecycle.pane.queued_pending = 0;
                            // `footer.append({ kind: "system", text:
                            // `new session ${sessionID}` })`.
                            self.lifecycle.append(Commit::system(&format!("new session {new_id}")));
                        }
                        Err(error) => {
                            // The ref's failure arm: the phase hint +
                            // an error commit (`runtime.ts:1011-1018`).
                            self.lifecycle.append(Commit::error(&error));
                        }
                    }
                }
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
                // G-272 — the admission runs the same per-turn
                // switches FIRST (`admitPromptTurn`'s head,
                // stream-v2.transport.ts:1795-1807), then the
                // durable-queue POST with the parts-aware body.
                self.apply_turn_switches().await;
                let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
                let _ = transport::send_prompt_parts(
                    &self.client,
                    &self.session_id,
                    &message_id,
                    &RunPrompt { delivery: Some(delivery), ..prompt },
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
        // G-272 — the `!` shell passthrough arm (`runShellTurn`,
        // stream-v2.transport.ts:1561-1592): NO user echo (the SSE
        // shell rows render `$ command` + output); the POST is
        // `session.shell` and the settle loop waits for the shell to
        // end through the same idle fence.
        if prompt.shell {
            let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
            if let Err(error) =
                transport::send_shell(&self.client, &self.session_id, &message_id, &prompt.text)
                    .await
            {
                self.lifecycle.append(Commit::error(&error));
                self.exit_code = 1;
                return;
            }
            self.settle_turn().await;
            return;
        }
        // The user echo (`onSend` → `rememberLocal`,
        // `runtime.ts:895-907`): pending rows can still be cancelled,
        // so the echo prints before the POST resolves.
        self.lifecycle.append(Commit::user(&prompt.text));
        // G-272 — the per-turn switches land BEFORE the POST
        // (`switchAgent`/`switchModel`, stream-v2.transport.ts:
        // 1832-1840).
        self.apply_turn_switches().await;
        let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        if let Err(error) =
            transport::send_prompt_parts(&self.client, &self.session_id, &message_id, prompt).await
        {
            self.lifecycle.append(Commit::error(&error));
            self.exit_code = 1;
            return;
        }
        self.settle_turn().await;
    }

    /// The turn settle loop (`runTurnWait` → `settleSession`): pump
    /// keys + folds until the execution terminates, then the turn
    /// summary (CONFIG-T4 `turn.duration`).
    async fn settle_turn(&mut self) {
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
        // G-181③ — the settle loop owns the footer through a turn;
        // the resize watch runs here too.
        if self.lifecycle.poll_resize() {
            let _ = self.lifecycle.flush();
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
    // G-221 — the hydrate's inbox seed (`client.session.inbox.list`,
    // transport.ts:955-977): the pending list lands in the transport
    // (a later `delivered` knows the row's text) and the footer's
    // `queued.prompts` view syncs (`syncPending`).
    if let Ok(items) = client.session().inbox().list(session_id).await {
        let values =
            items.iter().filter_map(|item| serde_json::to_value(item).ok()).collect::<Vec<_>>();
        transport.seed_pending(&values);
        lifecycle.pane.queued_pending = transport.pending_prompts().len();
        lifecycle.pane.queued_prompts = transport
            .pending_prompts()
            .iter()
            .map(|prompt| super::lifecycle::QueuedPromptView {
                message_id: prompt.message_id.clone(),
                text: prompt.text.clone(),
                delivery: prompt.delivery.clone(),
            })
            .collect();
    }
    // G-272 — the hydrate's blocker sweep (`hydrate`,
    // transport.ts:952-1004): the PARALLEL permission.list +
    // form.list + session.active the ref issues beside the message
    // walk — pending asks remount as footer blockers
    // (`syncBlockers`) and a still-running session keeps the running
    // phase (`state.rootActive`).
    hydrate_blockers(lifecycle, client, session_id, transport).await;
}

/// G-272 — the hydrate's parallel blocker fetch (`hydrate`,
/// transport.ts:952-1004): `permission.list` + `form.list` +
/// `session.active` → the footer machine's blocker queues and the
/// root-active phase. A blocker that re-mounts here behaves exactly
/// like its live event (the machine dedups by id).
async fn hydrate_blockers(
    lifecycle: &mut Lifecycle,
    client: &Client,
    session_id: &str,
    transport: &mut TransportState,
) {
    // Bind the API handles (the futures must outlive the builders).
    let session = client.session();
    let forms_api = session.form();
    let (permissions, forms, active) = tokio::join!(
        session.permission_list(session_id),
        forms_api.list(session_id),
        session.active(),
    );
    if let Ok(permissions) = permissions {
        for wire in &permissions {
            // The wire row IS the footer request shape (sessionID/
            // action/resources/save/source line up; `message` is
            // presentation-only and ignored).
            if let Ok(request) =
                serde_json::to_value(wire).and_then(|value| serde_json::from_value(value))
            {
                lifecycle.pane.permission_asked(request);
            }
        }
    }
    if let Ok(forms) = forms {
        for wire in &forms {
            if let Ok(value) = serde_json::to_value(wire) {
                lifecycle.pane.form_created(super::transport::decode_form(&value));
            }
        }
    }
    if let Ok(active) = active {
        // `state.rootActive = input.sessionID in active` (the
        // post-hydrate `write([], { phase: "running" })`).
        transport.set_running(active.contains_key(session_id));
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
    fn text_field_of(value: &Value, key: &str) -> String {
        value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
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
        // `renderUser` (transport.ts:600-618): the skill rows, the
        // text row (once per message — the `visible` guard), then the
        // image files' commits. G-221 — the skill rows ride the
        // queued-prompt seam (`skillCommits`, transport.ts:612).
        let joined = parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .map(|part| text(part).to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let skills = value
            .get("skills")
            .and_then(Value::as_array)
            .map(|skills| {
                skills
                    .iter()
                    .map(|skill| super::transport::QueuedSkill {
                        id: text_field_of(skill, "id"),
                        name: text_field_of(skill, "name"),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let prompt = super::transport::QueuedPrompt {
            message_id: message_id.clone(),
            text: joined,
            delivery: String::new(),
            skills,
        };
        commits.extend(transport.render_user_rows(&prompt));
        let files = value.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
        commits.extend(transport.commit_user_images(&message_id, &files));
        return commits;
    }

    // G-221 — the non-assistant transcript arms (`renderMessage`,
    // transport.ts:769-841): skill / shell / compaction messages.
    if matches!(kind, "skill" | "shell" | "compaction") {
        match kind {
            "skill" => {
                commits.extend(
                    transport.replay_skill_message(&message_id, &text_field_of(value, "name")),
                );
            }
            "shell" => {
                commits.extend(transport.replay_shell_message(value));
            }
            _ => {
                commits.extend(transport.replay_compaction_message(value));
            }
        }
        return commits.into_iter().filter(|commit| settings.commit_visible(commit)).collect();
    }

    // The assistant walk (transport.ts:846-881): per-part projections
    // with the showTools()/thinking visibility gates.
    let has_tools =
        parts.iter().any(|part| part.get("type").and_then(Value::as_str) == Some("tool"));
    let mut text_ordinal = 0usize;
    let mut reasoning_ordinal = 0usize;
    for part in &parts {
        let part_id_of =
            |update_key: &str| update_key.split('\u{0}').nth(1).unwrap_or_default().to_owned();
        match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                let fragment_text = text(part);
                let visible = settings.tools || !has_tools;
                let update = transport.project_part(
                    &message_id,
                    "text",
                    text_ordinal,
                    fragment_text,
                    visible,
                );
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
                let part_id =
                    format!("prt_{}", part.get("id").and_then(Value::as_str).unwrap_or_default());
                let state = part.get("state").cloned().unwrap_or(Value::Null);
                commits.extend(transport.commit_tool_images(&part_id, &state, &message_id));
            }
            _ => {}
        }
    }
    commits
}

/// Non-blocking read of whatever landed on fd 0 (the crossterm
/// `poll(0)` + `read` pair's lane B3a replacement).
#[allow(unsafe_code)] // FIONREAD/read on fd 0 are libc FFI
fn read_footer_input() -> Vec<u8> {
    let mut available: i32 = 0;
    // SAFETY: FIONREAD on stdin.
    unsafe { libc::ioctl(0, libc::FIONREAD, std::ptr::from_mut(&mut available)) };
    if available <= 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u8; usize::try_from(available).unwrap_or(0)];
    // SAFETY: read(2) of at most FIONREAD bytes.
    let read = unsafe { libc::read(0, buffer.as_mut_ptr().cast::<libc::c_void>(), buffer.len()) };
    if read <= 0 {
        return Vec::new();
    }
    #[allow(clippy::cast_sign_loss)]
    buffer.truncate(read as usize);
    buffer
}

/// Map a key spec (the shared `input_term` vocabulary) into the
/// footer's keypress vocabulary — the crossterm `map_key` arm-for-arm.
fn footer_key_from_spec(spec: &str) -> Option<FooterKey> {
    match spec {
        "ctrl+c" => Some(FooterKey::CtrlC),
        "ctrl+t" => Some(FooterKey::CtrlT),
        // G-272 — `session.background`'s ctrl+b (config/keybind.
        // ts:127) and the settings panel's left/right arrows
        // (footer.command.tsx:818-824).
        "ctrl+b" => Some(FooterKey::CtrlB),
        "left" => Some(FooterKey::Left),
        "right" => Some(FooterKey::Right),
        // G-272 — the panel-family keys (`command.palette.show`'s
        // ctrl+p and the `<leader>` = ctrl+x chord,
        // config/keybind.ts:41,57,131).
        "ctrl+p" => Some(FooterKey::CtrlP),
        "ctrl+x" => Some(FooterKey::CtrlX),
        "return" => Some(FooterKey::Enter),
        "escape" => Some(FooterKey::Escape),
        "backspace" => Some(FooterKey::Backspace),
        "up" => Some(FooterKey::Up),
        "down" => Some(FooterKey::Down),
        "tab" => Some(FooterKey::Tab),
        "pageup" => Some(FooterKey::PageUp),
        "pagedown" => Some(FooterKey::PageDown),
        _ => {
            let mut chars = spec.chars();
            let first = chars.next()?;
            chars.next().is_none().then_some(FooterKey::Printable(first))
        }
    }
}

/// G-181① — parse a `provider/model` label into the preference
/// model (`parseModel`'s target shape; empty halves stay unparsed).
#[must_use]
fn parse_model_label(label: &str) -> Option<super::model_pref::ModelPreferenceModel> {
    let (provider, model) = label.split_once('/')?;
    (!provider.is_empty() && !model.is_empty()).then(|| super::model_pref::ModelPreferenceModel {
        provider_id: provider.to_owned(),
        model_id: model.to_owned(),
    })
}

/// G-272 — the catalog snapshot (`fetchCatalog` + `loadCatalog`,
/// runtime.ts:592-611 over catalog.shared.ts): the agent list
/// (`loadRunAgents`), the model catalog as the picker's provider
/// shape (`loadRunProviders` → `runProviders`), and the per-model
/// context limits (the usage percent's denominator). Best-effort: an
/// unreachable catalog yields `None` (the caller drops to empty —
/// "no variants available" / no limits / no agents).
struct CatalogSnapshot {
    agents: Vec<crate::components::footer::panels::RunAgent>,
    providers: Vec<crate::components::footer::panels::RunProvider>,
    limits: std::collections::HashMap<(String, String), u64>,
    models: Vec<oc_client::types::ModelInfo>,
}

impl CatalogSnapshot {
    /// G-181① — `variantsFor(providers, model)` (runtime.ts:142-147):
    /// the target model's variant ids.
    fn variants_for(
        &self,
        target: Option<&super::model_pref::ModelPreferenceModel>,
    ) -> Vec<String> {
        let Some(target) = target else { return Vec::new() };
        self.models
            .iter()
            .find(|model| {
                model.provider_id == target.provider_id && model.model_id == target.model_id
            })
            .map(|model| model.variants.iter().map(|variant| variant.id.clone()).collect())
            .unwrap_or_default()
    }
}

/// `defaultCost` (catalog.shared.ts:24-28): the un-tiered cost (else
/// the first), zero when every tier is free.
fn default_cost(model: &oc_client::types::ModelInfo) -> f64 {
    let Some(picked) =
        model.cost.iter().find(|cost| cost.tier.is_none()).or_else(|| model.cost.first())
    else {
        return 0.0;
    };
    if model.cost.iter().all(|cost| cost.input == 0.0) {
        0.0
    } else {
        picked.input
    }
}

async fn fetch_catalog(client: &Client, directory: &str) -> Option<CatalogSnapshot> {
    let location =
        || Some(oc_client::types::QueryLocation { directory: Some(directory.to_owned()) });
    let model_query = oc_client::types::ModelListQuery { location: location() };
    let agent_query = oc_client::types::AgentListQuery { location: location() };
    let model_api = client.model();
    let agent_api = client.agent();
    let (models, agents) = tokio::join!(model_api.list(&model_query), agent_api.list(&agent_query));
    // `loadCatalog` catches each list independently (runtime.ts:601-
    // 611) — a failing list degrades to empty, never kills the rest.
    let models = models.map(|page| page.data).unwrap_or_default();
    let agents = agents
        .map(|page| {
            page.data
                .iter()
                .map(|agent| crate::components::footer::panels::RunAgent {
                    id: agent.id.clone(),
                    name: agent.name.clone(),
                    description: agent.description.clone(),
                    mode: agent.mode.as_str().to_owned(),
                    hidden: agent.hidden,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // `runProviders` (catalog.shared.ts:47-76): group the models per
    // provider (provider.list is not needed — an id-only group stands
    // in for a provider the list omits).
    let mut providers: Vec<crate::components::footer::panels::RunProvider> = Vec::new();
    let mut limits = std::collections::HashMap::new();
    for model in &models {
        let run_model = crate::components::footer::panels::RunModel {
            id: model.model_id.clone(),
            name: model.name.clone(),
            cost_input: default_cost(model),
            deprecated: model.status == oc_client::types::ModelInfoStatus::Deprecated,
        };
        match providers.iter_mut().find(|provider| provider.id == model.provider_id) {
            Some(entry) => entry.models.push(run_model),
            None => providers.push(crate::components::footer::panels::RunProvider {
                id: model.provider_id.clone(),
                name: model.provider_id.clone(),
                models: vec![run_model],
            }),
        }
        if model.limit.context > 0 {
            limits.insert(
                (model.provider_id.clone(), model.model_id.clone()),
                u64::try_from(model.limit.context).unwrap_or(0),
            );
        }
    }
    Some(CatalogSnapshot { agents, providers, limits, models })
}

/// G-181① — the `onCycleVariant` decision (`runtime.ts:293-310`):
/// no model or no variants → the "no variants available" notice;
/// otherwise the cycled variant, its status line, and the
/// `formatModelLabel(model, variant)` label the footer shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CycleVariantDecision {
    NoVariants,
    Cycle { next: Option<String>, status: String, label: Option<String> },
}

fn cycle_variant_decision(
    target: Option<&super::model_pref::ModelPreferenceModel>,
    active: Option<&str>,
    variants: &[String],
) -> CycleVariantDecision {
    let Some(target) = target else {
        return CycleVariantDecision::NoVariants;
    };
    if variants.is_empty() {
        return CycleVariantDecision::NoVariants;
    }
    let next = super::model_pref::cycle_model_variant(active, variants);
    let status = match &next {
        Some(variant) => format!("variant {variant}"),
        None => "variant default".to_owned(),
    };
    let base = format!("{}/{}", target.provider_id, target.model_id);
    // `formatModelLabel`'s variant suffix (`variant.shared.ts:21-29`).
    let label = match &next {
        Some(variant) => Some(format!("{base} · {variant}")),
        None => Some(base),
    };
    CycleVariantDecision::Cycle { next, status, label }
}

/// G-181① — the boot model binding (`resolveVariant` +
/// `addRecent`, `model-preference.ts:150-172`, consumed as
/// `formatModelLabel(input.model, input.variant)` at
/// `runtime.lifecycle.ts:214`): parse the resolved target's
/// `provider/model` label, resolve the persisted variant override
/// (the footer's model/variant switch reads the same document), and
/// record the model in the recent list. A label without a `/` or an
/// empty document keeps the label as-is.
#[must_use]
pub fn bind_model_preference(
    preferences: &ModelPreferenceRepository,
    model_label: Option<&str>,
) -> Option<String> {
    let label = model_label?;
    let Some((provider, model)) = label.split_once('/') else {
        return Some(label.to_owned());
    };
    if provider.is_empty() || model.is_empty() {
        return Some(label.to_owned());
    }
    let target = super::model_pref::ModelPreferenceModel {
        provider_id: provider.to_owned(),
        model_id: model.to_owned(),
    };
    preferences.add_recent(&target);
    // `formatModelLabel`'s variant suffix (`variant.shared.ts:21-29`).
    match preferences.resolve_variant(&target) {
        Some(variant) => Some(format!("{label} · {variant}")),
        None => Some(label.to_owned()),
    }
}

/// G-181② — the `/new` session factory (`createSession`,
/// `runtime.ts:933-1018`): create the session server-side at the
/// same location/agent (`session.create`), returning its id.
async fn create_session(
    client: &Client,
    directory: &str,
    agent: Option<&str>,
) -> Result<String, String> {
    let session = client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            agent: agent.map(str::to_owned),
            location: Some(oc_client::types::LocationPublicRef { directory: directory.to_owned() }),
            ..Default::default()
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(session.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G-181① — the boot model binding: a persisted variant override
    /// reaches the label (`resolveVariant`), the model lands in the
    /// recent list (`addRecent`), and a label-less / slash-less /
    /// override-less boot keeps the label unchanged.
    #[test]
    fn model_preference_binds_at_boot() {
        let dir = std::env::temp_dir().join(format!(
            "oc-mini-boot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        let preferences = ModelPreferenceRepository::new(dir.join("model.json"));
        let target = super::super::model_pref::ModelPreferenceModel {
            provider_id: "anthropic".into(),
            model_id: "claude".into(),
        };
        preferences.save_variant(&target, Some("think"));
        assert_eq!(
            bind_model_preference(&preferences, Some("anthropic/claude")).as_deref(),
            Some("anthropic/claude · think"),
            "the persisted variant override resolves into the label"
        );
        // addRecent recorded the resolved model.
        assert_eq!(preferences.read().recent.first(), Some(&target));
        // No override → the bare label; odd labels pass through.
        preferences.save_variant(&target, None);
        assert_eq!(
            bind_model_preference(&preferences, Some("anthropic/claude")).as_deref(),
            Some("anthropic/claude")
        );
        assert_eq!(
            bind_model_preference(&preferences, Some("label-only")).as_deref(),
            Some("label-only")
        );
        assert_eq!(bind_model_preference(&preferences, Some("/")).as_deref(), Some("/"));
        assert_eq!(bind_model_preference(&preferences, None), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// G-181④ — the reconnect vocabulary: the cadence is the ref's
    /// 250 ms and the notice is the pump's private message type (no
    /// SSE event carries it).
    #[test]
    fn reconnect_cadence_is_250ms() {
        assert_eq!(RECONNECT_DELAY, Duration::from_millis(250));
        // A live SSE event value is never mistaken for the private
        // notice — the vocabulary is typed, not a magic `type` string.
        let live = serde_json::json!({ "type": "session.text.delta", "data": { "delta": "x" } });
        assert!(matches!(StreamMessage::Event(live), StreamMessage::Event(_)));
    }

    /// G-181① — the variant-cycle decision (`onCycleVariant`,
    /// runtime.ts:293-310): cycles, persists through the preference
    /// repository (`saveVariant`), and a fresh boot reflects the
    /// saved override (`bindModelPreference`).
    #[test]
    fn variant_cycle_persists_and_boot_reflects_it() {
        let dir = std::env::temp_dir().join(format!(
            "oc-mini-cycle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos()
        ));
        let preferences = ModelPreferenceRepository::new(dir.join("model.json"));
        let target = super::super::model_pref::ModelPreferenceModel {
            provider_id: "anthropic".into(),
            model_id: "claude".into(),
        };
        let variants: Vec<String> = vec!["default".into(), "fast".into(), "think".into()];

        // No model → notice; model without variants → notice.
        assert_eq!(cycle_variant_decision(None, None, &variants), CycleVariantDecision::NoVariants);
        assert_eq!(
            cycle_variant_decision(Some(&target), None, &[]),
            CycleVariantDecision::NoVariants
        );

        // The cycle: none → fast (persisted), fast → think (persisted),
        // think → default.
        let mut active: Option<String> = None;
        for expected in ["fast", "think"] {
            match cycle_variant_decision(Some(&target), active.as_deref(), &variants) {
                CycleVariantDecision::Cycle { next, status, label } => {
                    assert_eq!(next.as_deref(), Some(expected));
                    assert_eq!(status, format!("variant {expected}"));
                    assert_eq!(
                        label.as_deref(),
                        Some(format!("anthropic/claude · {expected}").as_str()),
                        "formatModelLabel suffix"
                    );
                    preferences.save_variant(&target, next.as_deref());
                    active = next;
                }
                other => panic!("expected a cycle, got {other:?}"),
            }
        }
        match cycle_variant_decision(Some(&target), active.as_deref(), &variants) {
            CycleVariantDecision::Cycle { next, status, label } => {
                assert_eq!(next, None, "last named variant cycles back to default");
                assert_eq!(status, "variant default");
                assert_eq!(label.as_deref(), Some("anthropic/claude"));
                preferences.save_variant(&target, None);
            }
            other => panic!("expected the default arm, got {other:?}"),
        }

        // A fresh boot reflects the persisted override.
        assert_eq!(
            bind_model_preference(&preferences, Some("anthropic/claude")).as_deref(),
            Some("anthropic/claude"),
            "the default (cleared) override boot keeps the bare label"
        );
        preferences.save_variant(&target, Some("think"));
        assert_eq!(
            bind_model_preference(&preferences, Some("anthropic/claude")).as_deref(),
            Some("anthropic/claude · think"),
            "the saved override reaches the boot label"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// G-181① — the label parser: `provider/model` parses, empty
    /// halves and bare labels do not.
    #[test]
    fn model_labels_parse() {
        assert_eq!(
            parse_model_label("anthropic/claude").map(|model| model.model_id),
            Some("claude".into())
        );
        assert_eq!(parse_model_label("label-only"), None);
        assert_eq!(parse_model_label("/"), None);
        assert_eq!(parse_model_label("a/"), None);
        assert_eq!(parse_model_label("/b"), None);
    }

    /// G-181④ — the stream pump swaps the client on reconnect: the
    /// stub server serves one SSE frame then EOFs every connection;
    /// the reconnect factory hands back a client pointed at a SECOND
    /// stub. The pump must announce `Reconnected` carrying the fresh
    /// client's base URL (the ref's `sdk = next`,
    /// stream-v2.transport.ts:1533-1539) and keep folding live events
    /// from the NEW server.
    #[tokio::test]
    async fn reconnect_swaps_the_client_instance() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        async fn spawn_sse_server(
        ) -> (String, tokio::task::JoinHandle<()>, std::sync::Arc<std::sync::atomic::AtomicUsize>)
        {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let addr = listener.local_addr().expect("addr");
            let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let counter = std::sync::Arc::clone(&hits);
            let task = tokio::spawn(async move {
                loop {
                    let (mut socket, _) = listener.accept().await.expect("accept");
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let (mut reader, _) = socket.split();
                    let mut request = vec![0u8; 4096];
                    let _ = reader.read(&mut request).await;
                    // One live event, then EOF — forces the reconnect.
                    let body = "id: e1\nevent: message.updated\ndata: {\"sessionID\":\"s1\"}\n\n";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                }
            });
            (format!("http://{addr}"), task, hits)
        }

        let (first_url, _first_task, first_hits) = spawn_sse_server().await;
        let (second_url, _second_task, second_hits) = spawn_sse_server().await;
        let factory_url = second_url.clone();
        let reconnect: ReconnectFactory = std::sync::Arc::new(move || {
            let url = factory_url.clone();
            Box::pin(async move { Some(Client::new(&url).expect("client")) })
        });

        let client = Client::new(&first_url).expect("client");
        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamMessage>(16);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
        let pump = tokio::spawn(stream_pump(client, Some(reconnect), tx, Some(ready_tx)));
        ready_rx.await.expect("first subscription signals ready");

        // The first live event, then the reconnect notice carrying the
        // SECOND server's base URL, then the new server's live event.
        let mut saw_reconnect_with_fresh_client = false;
        let mut saw_event_from_second_server = false;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline
            && !(saw_reconnect_with_fresh_client && saw_event_from_second_server)
        {
            let message = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("message within timeout")
                .expect("pump alive");
            match message {
                StreamMessage::Reconnected(fresh) => {
                    assert_eq!(
                        fresh.base_url(),
                        second_url,
                        "the swapped client points at the reconnect factory's endpoint"
                    );
                    saw_reconnect_with_fresh_client = true;
                }
                StreamMessage::Event(_) => {
                    if saw_reconnect_with_fresh_client {
                        saw_event_from_second_server = true;
                    }
                }
            }
        }
        pump.abort();
        assert!(
            second_hits.load(std::sync::atomic::Ordering::SeqCst) > 0,
            "the second server actually received the resubscribe"
        );
        assert!(first_hits.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    }

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
        // to Ignored, the tail's CHUNK appends (G-241), the final
        // text closes the part.
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
            super::super::transport::Fold::Commit(commit) => {
                assert_eq!(commit.text, " four", "the delta commits its chunk")
            }
            other => panic!("tail folded: {other:?}"),
        }
        let ended = transport.fold(&serde_json::json!({
            "type": "session.text.ended",
            "data": { "sessionID": "s1", "messageID": "msg_a1", "partID": "text:0" },
        }));
        // G-241 — an ended equal to the committed text replays
        // nothing (the rows already printed).
        assert_eq!(ended, super::super::transport::Fold::Ignored);

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
