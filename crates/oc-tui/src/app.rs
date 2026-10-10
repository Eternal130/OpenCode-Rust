//! The application core: the unified keypress pump (store + keymap +
//! dispatcher + composer + footer blockers + render) and the live run
//! loop that mounts it on a real terminal.
//!
//! ## Key routing (the reference's layer order)
//!
//! `app.tsx` layers, topmost first — the pump reproduces the order:
//!
//! 1. **Dialogs** (`ui/dialog.tsx` modal layer): escape/ctrl+c/printables/
//!    `dialog.*` navigation.
//! 2. **Permission blockers** (`routes/session/permission.tsx`): while a
//!    permission ask is live, every key routes to the question body
//!    (`app.exit` shadowed while open).
//! 3. **The conditional `app.exit` layer**: enabled only while the
//!    focused prompt is EMPTY (`Keymap.createLayer({ enabled: () =>
//!    prompt.text === "" })`) — `<leader>q` exits on an empty prompt and
//!    merely types `q` into a non-empty one.
//! 4. **The managed textarea layer** (`registerManagedTextareaLayer`):
//!    with text present, printable/editing keys feed the composer.
//! 5. **The base layer** (`ctrl+p`, leader chords, tab commands …).
//!
//! ## The run loop
//!
//! [`run`] acquires the terminal (raw mode + alternate screen),
//! spawns the SSE [`crate::data::Connection`], and drains: key input →
//! [`App::press`], ingest → [`Store::ingest_all`], clock ticks →
//! spinner/toast advance, config-file changes → keybind/theme re-merge.
//! SIGWINCH (resize) re-lays-out at the new size. Exit (`app.exit`)
//! tears the terminal down and returns.

use std::collections::HashMap;

use crate::components::command_palette::{CommandDispatcher as _, Dispatch, StoreDispatcher};
use crate::components::composer::{ComposerKey, KeyOutcome};
use crate::components::dialog::{execute_submission, handle_dialog_key, render_app};
use crate::components::footer::permission::{
    create_permission_body_state, permission_key, PermissionBodyState, PermissionKey,
    PermissionReply, PermissionRequest,
};
use crate::components::footer::{FooterTheme, FooterView};
use crate::data::{Connection, HttpResync, Ingest};
use crate::keymap::{KeyStroke, Keymap, ManualClock, Outcome};
use crate::render::Frame;
use crate::store::{Focus, Route, Store};
use crate::theme::{ThemeRegistry, ThemeState};
use oc_theme::types::Mode;

/// The sentinel session key for the home route's composer draft (the
/// reference's Home mounts the same Prompt component).
pub const HOME_PROMPT_KEY: &str = "home";

/// The docked height of the permission body while a blocker is live.
const PERMISSION_MOUNT_HEIGHT: usize = 16;

/// The headless/live application: the full keymap dispatch over the
/// store, with the keymap's `dialog`/`input` modes kept in sync with
/// the dialog stack and the focused composer.
pub struct App {
    pub store: Store,
    keymap: Keymap,
    clock: ManualClock,
    dispatcher: StoreDispatcher,
    dialog_mode: bool,
    input_mode: bool,
    /// Per-request permission body state (selection / reject stage).
    permission_states: HashMap<String, PermissionBodyState>,
    /// Every permission reply the pump sent (assertions / the server
    /// call seam).
    pub permission_replies: Vec<PermissionReply>,
    size: (u16, u16),
    /// Whether the exit command fired (the run loop's teardown gate).
    pub exit_requested: bool,
    /// The applied keybind override set (re-merged when the store's
    /// config-reload signal changes).
    applied_keybinds: Vec<crate::keymap::KeybindOverride>,
}

impl App {
    #[must_use]
    pub fn new(size: (u16, u16)) -> Self {
        Self::with_store(Store::new(crate::reactive::Runtime::new()), size)
    }

    #[must_use]
    pub fn with_store(store: Store, size: (u16, u16)) -> Self {
        Self {
            store,
            keymap: Keymap::new(),
            clock: ManualClock::default(),
            dispatcher: StoreDispatcher,
            dialog_mode: false,
            input_mode: false,
            permission_states: HashMap::new(),
            permission_replies: Vec::new(),
            size,
            exit_requested: false,
            applied_keybinds: Vec::new(),
        }
    }

    /// Apply the theme registry + user keybinds from a loaded config
    /// (boot + reload): custom themes register into the registry, the
    /// theme name/mode applies, and the keybind overrides merge into
    /// the live keymap.
    pub fn apply_config(&mut self, config: &crate::config::TuiConfig, mode_hint: Mode) {
        // Registry: builtins + discovered customs (discovery ran in the
        // caller — the config only carries overrides here).
        let mut registry = ThemeRegistry::builtins();
        crate::theme::register_custom_themes(
            &mut registry,
            &crate::theme::discover_custom_themes(&Self::config_directories()),
        );
        self.store.set_registry(registry);
        // Theme name + mode.
        let mode = match config.theme.mode.as_deref() {
            Some("light") => Mode::Light,
            Some("dark") => Mode::Dark,
            Some("system") => mode_hint,
            _ => self.store.theme().mode,
        };
        let name = config.theme.name.clone().unwrap_or_else(|| "opencode".to_owned());
        let registry = self.store.registry();
        let state = registry
            .resolve(&name, mode)
            .or_else(|| registry.resolve("opencode", mode))
            .unwrap_or_else(ThemeState::v2_dark);
        self.store.set_theme(state);
        // Keybinds.
        if let Some(timeout) = config.leader_timeout_ms {
            self.keymap.set_timeout(timeout);
        }
        let overrides = crate::keymap::parse_keybind_overrides(&config.keybinds);
        self.store.set_keybinds(overrides);
        self.remerge_keybinds();
    }

    fn config_directories() -> Vec<std::path::PathBuf> {
        let roots = oc_util::global_roots::resolve();
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        crate::theme::config_directories(&roots.config, &cwd)
    }

    /// Re-merge the store's keybind overrides into the live keymap
    /// (idempotent: the table resets to defaults first).
    fn remerge_keybinds(&mut self) {
        let overrides = self.store.keybinds();
        if overrides == self.applied_keybinds {
            return;
        }
        self.keymap = Keymap::new();
        self.keymap.apply_overrides(&overrides);
        self.applied_keybinds = overrides;
        // Restore the mode stack the fresh keymap dropped.
        self.dialog_mode = false;
        self.input_mode = false;
        self.sync_modes();
    }

    /// The pump's clock (tests advance it to expire leader sequences).
    pub fn advance_clock(&mut self, ms: u64) {
        self.clock.advance(ms);
        self.store.advance_clock(self.clock.now_ms_value());
    }

    /// Resize the terminal (SIGWINCH): the next frame lays out at the
    /// new size.
    pub fn resize(&mut self, size: (u16, u16)) {
        self.size = size;
        self.store.request_redraw();
    }

    /// Feed one key spec ("ctrl+p", "r", "enter", "BACKSPACE", …).
    /// Replay-script key tokens use the kitty vocabulary; the reference
    /// keymap expands the aliases (`enter → return`, `esc → escape`,
    /// `pgdown/pgup → page…`; `KeyCodes` names like BACKSPACE lowercase —
    /// single characters keep their case for the shift form).
    pub fn press(&mut self, spec: &str) {
        self.maybe_remerge_keybinds();
        let lowered = spec.to_ascii_lowercase();
        let expanded = match spec {
            "enter" => "return",
            "esc" => "escape",
            "pgdown" => "pagedown",
            "pgup" => "pageup",
            _ => {
                if spec.chars().count() == 1 {
                    spec
                } else {
                    lowered.as_str()
                }
            }
        };
        let Some(stroke) = KeyStroke::parse(expanded) else { return };
        self.sync_modes();
        if self.store.focus() == Focus::Dialog {
            self.stroke_in_dialog(stroke);
        } else if self.permission_live() {
            self.stroke_in_permission(expanded);
        } else {
            self.stroke_on_route(&stroke);
        }
        self.sync_modes();
        self.store.runtime.run_effects();
    }

    fn maybe_remerge_keybinds(&mut self) {
        if self.store.keybinds() != self.applied_keybinds {
            self.remerge_keybinds();
        }
    }

    /// A permission ask is the live footer view.
    fn permission_live(&self) -> bool {
        matches!(self.store.footer().view, FooterView::Permission { .. })
    }

    fn stroke_in_permission(&mut self, key: &str) {
        let FooterView::Permission { request } = self.store.footer().view.clone() else {
            return;
        };
        let state = self
            .permission_states
            .get(&request.id)
            .cloned()
            .unwrap_or_else(|| create_permission_body_state(&request));
        match permission_key(&state, &request, key) {
            PermissionKey::State(next) => {
                self.permission_states.insert(request.id.clone(), next);
            }
            PermissionKey::Reply(reply, state) => {
                self.permission_replies.push(reply.clone());
                self.permission_states.insert(request.id.clone(), state);
                // The reply resolves the ask server-side
                // (`permission.replied`) — the view falls back.
                self.store.permission_replied(&reply.request_id);
            }
            PermissionKey::Text(ch) => {
                let next =
                    crate::components::footer::permission::permission_type_message(&state, ch);
                self.permission_states.insert(request.id.clone(), next);
            }
            PermissionKey::Ignored => {}
        }
    }

    fn stroke_in_dialog(&mut self, stroke: KeyStroke) {
        // The pump owns the keymap; the dialog layer borrows it for the
        // dialog.* commands — split borrows via a temporary take.
        let mut keymap = std::mem::take(&mut self.keymap);
        let outcome = handle_dialog_key(&self.store, &mut keymap, &self.clock, stroke, self.size);
        self.keymap = keymap;
        match outcome {
            crate::components::dialog::DialogKeyOutcome::Submitted(value) => {
                let dispatch = execute_submission(&self.store, &mut self.dispatcher, &value);
                if let Dispatch::ServerPending(_) = dispatch {
                    // Server-backed submissions close the palette and
                    // surface nothing further in the pump.
                }
            }
            crate::components::dialog::DialogKeyOutcome::PromptSubmitted(value) => {
                self.submit_rename(&value);
            }
            _ => {}
        }
    }

    /// Rename submit: update the tab title locally (the `session.update`
    /// server call is TODO(7.3)).
    fn submit_rename(&mut self, value: &str) {
        let dialogs = self.store.dialogs();
        if let Some(entry) = dialogs.last() {
            if let crate::store::DialogKind::SessionRename { session_id } = &entry.kind {
                let session_id = session_id.clone();
                if !value.is_empty() {
                    self.store.open_tab(&session_id, Some(value.to_owned()));
                }
            }
        }
        self.store.clear_dialogs();
    }

    /// The session key whose composer holds the prompt focus (home has
    /// its own Prompt mount).
    fn prompt_key(&self) -> String {
        match self.store.route() {
            Route::Session { session_id } => session_id,
            Route::Home => HOME_PROMPT_KEY.to_owned(),
        }
    }

    /// Route-layer stroke: conditional exit layer → managed textarea →
    /// input/base keymap (see the module docs).
    fn stroke_on_route(&mut self, stroke: &KeyStroke) {
        let key = self.prompt_key();
        let draft_empty = self.store.prompt_draft(&key).is_empty();
        self.keymap.set_command_enabled("app.exit", draft_empty);

        // 1. The conditional app.exit layer (empty prompt only): base
        //    chords resolve ABOVE the textarea — <leader>q / ctrl+c /
        //    ctrl+d exit, and a PENDING leader sequence completes
        //    through the base layer (leader+2 switches tabs). A single
        //    stroke that only the base layer binds (up, escape…) keeps
        //    falling through — the textarea/input layers sit above
        //    base for those.
        if draft_empty {
            let had_pending = self.keymap.pending_display().is_some();
            match self.keymap.lookup_base(stroke.clone(), &self.clock) {
                Outcome::Command { command, .. } if command == "app.exit" => {
                    self.request_exit();
                    return;
                }
                Outcome::Command { command, args } if had_pending => {
                    let dispatch = self.dispatcher.dispatch(&self.store, &command, args.as_deref());
                    if let Dispatch::ServerPending(reason) = dispatch {
                        let _ = reason;
                    }
                    return;
                }
                Outcome::Pending { .. } | Outcome::Cancelled => return,
                Outcome::Command { .. } | Outcome::Unhandled => {}
            }
        }

        // 2. The managed textarea layer: with text present, keys the
        //    composer owns feed it before the base keymap sees them
        //    (printables never leak into leader continuations).
        if !draft_empty {
            if let Some(key_event) = composer_key(stroke) {
                let mut composer = self.store.composer(&key);
                let outcome = composer.handle_key(&key_event);
                if outcome != KeyOutcome::Ignored {
                    self.store.set_composer(&key, &composer);
                    if let KeyOutcome::Submitted(prompt) = outcome {
                        self.store.forward_submission(&key, &prompt);
                    }
                    self.keymap.clear_pending();
                    return;
                }
            }
        }

        // 3. The input/base keymap (input mode pushed while a composer
        //    is focused): editing chords resolve as input.* commands,
        //    everything else falls to base.
        if let Outcome::Command { command, args } = self.keymap.lookup(stroke.clone(), &self.clock)
        {
            if command == "app.exit" {
                self.request_exit();
            } else if composer_command(&command).is_some() || command.starts_with("input.") {
                self.run_composer_command(&key, &command);
            } else {
                let dispatch = self.dispatcher.dispatch(&self.store, &command, args.as_deref());
                if command == "app.exit" && dispatch != Dispatch::Unknown {
                    self.exit_requested = true;
                }
            }
        } else if let Some(key_event) = composer_key(stroke) {
            // 4. Unbound printable keys land in the composer.
            let mut composer = self.store.composer(&key);
            let outcome = composer.handle_key(&key_event);
            if outcome != KeyOutcome::Ignored {
                self.store.set_composer(&key, &composer);
                if let KeyOutcome::Submitted(prompt) = outcome {
                    self.store.forward_submission(&key, &prompt);
                }
            }
        }
    }

    fn run_composer_command(&mut self, key: &str, command: &str) {
        let mut composer = self.store.composer(key);
        if let Some(key_event) = composer_command(command) {
            let outcome = composer.handle_key(&key_event);
            if outcome != KeyOutcome::Ignored {
                self.store.set_composer(key, &composer);
                if let KeyOutcome::Submitted(prompt) = outcome {
                    self.store.forward_submission(key, &prompt);
                }
            }
        }
    }

    fn request_exit(&mut self) {
        self.exit_requested = true;
        self.store.clear_dialogs();
    }

    /// Keep the keymap's mode stack aligned: `dialog` while dialogs are
    /// open (the reference's `modal` mode), `input` below it while a
    /// composer holds the route focus (the managed textarea layer).
    /// The stack is recomputed whole each stroke (LIFO push/pop ordering
    /// hazards when a dialog opens over the input mode).
    fn sync_modes(&mut self) {
        let dialog = !self.store.dialogs().is_empty();
        let input = !dialog && !self.permission_live();
        let mut modes: Vec<&str> = Vec::new();
        if input {
            modes.push("input");
        }
        if dialog {
            modes.push("dialog");
        }
        self.keymap.set_modes(&modes);
        self.dialog_mode = dialog;
        self.input_mode = input;
    }

    /// The current frame (route + blockers + dialog overlay).
    #[must_use]
    pub fn frame(&self) -> Frame {
        let theme = self.store.theme();
        let mut frame = render_app(&self.store, &theme, self.size);
        if let FooterView::Permission { request } = self.store.footer().view.clone() {
            let state = self
                .permission_states
                .get(&request.id)
                .cloned()
                .unwrap_or_else(|| create_permission_body_state(&request));
            let footer_theme = FooterTheme::from_resolved(&theme.resolved.tokens);
            let mount_height = PERMISSION_MOUNT_HEIGHT.min(usize::from(self.size.1));
            let body = crate::components::footer::permission::render_permission_body(
                &request,
                &state,
                usize::from(self.size.0),
                mount_height,
                &footer_theme,
            );
            let y = usize::from(self.size.1) - mount_height;
            for (row, cells) in body.cells.iter().enumerate() {
                let Some(slot) = frame.cells.get_mut(y + row) else { break };
                let count = cells.len().min(slot.len());
                slot[..count].clone_from_slice(&cells[..count]);
            }
        }
        frame
    }

    /// The frame as one text block (the `frameIncludes` surface).
    #[must_use]
    pub fn frame_text(&self) -> String {
        self.frame().text_grid().join("\n")
    }

    /// Where key input goes right now (the replay scripts' `focus`).
    #[must_use]
    pub fn focus(&self) -> &'static str {
        if self.store.focus() == Focus::Dialog || self.permission_live() {
            "dialog"
        } else {
            "prompt"
        }
    }
}

/// Map a stroke onto the composer's key vocabulary (printable unmodified
/// chars + the editing set); `None` = not a composer key.
fn composer_key(stroke: &KeyStroke) -> Option<ComposerKey> {
    if !stroke.ctrl && !stroke.alt && stroke.key.chars().count() == 1 {
        let ch = stroke.key.chars().next().expect("one char");
        let ch = if stroke.shift { ch.to_ascii_uppercase() } else { ch };
        return Some(ComposerKey::Char(ch));
    }
    let key = if stroke.ctrl && !stroke.alt {
        match stroke.key.as_str() {
            "a" => return Some(ComposerKey::CtrlA),
            "e" => return Some(ComposerKey::CtrlE),
            "k" => return Some(ComposerKey::CtrlK),
            "u" => return Some(ComposerKey::CtrlU),
            "c" => return Some(ComposerKey::CtrlC),
            "v" => return Some(ComposerKey::CtrlV),
            "j" => return Some(ComposerKey::CtrlJ),
            _ => return None,
        }
    } else if stroke.alt {
        return None;
    } else {
        stroke.key.as_str()
    };
    let shift = stroke.shift;
    match key {
        "return" if shift => Some(ComposerKey::ShiftEnter),
        "return" => Some(ComposerKey::Enter),
        "up" => Some(ComposerKey::Up),
        "down" => Some(ComposerKey::Down),
        "left" => Some(ComposerKey::Left),
        "right" => Some(ComposerKey::Right),
        "home" => Some(ComposerKey::Home),
        "end" => Some(ComposerKey::End),
        "backspace" => Some(ComposerKey::Backspace),
        "delete" => Some(ComposerKey::Delete),
        "escape" => Some(ComposerKey::Escape),
        "tab" => Some(ComposerKey::Tab),
        _ => None,
    }
}

/// Map an `input.*` / `prompt.*` command onto the composer vocabulary
/// (the v2 table's input layer).
fn composer_command(command: &str) -> Option<ComposerKey> {
    match command {
        "input.submit" | "prompt.submit" => Some(ComposerKey::Enter),
        "input.newline" | "prompt.queue" => Some(ComposerKey::ShiftEnter),
        "input.move.left" => Some(ComposerKey::Left),
        "input.move.right" => Some(ComposerKey::Right),
        "input.move.up" | "prompt.history.previous" => Some(ComposerKey::Up),
        "input.move.down" | "prompt.history.next" => Some(ComposerKey::Down),
        "input.line.home" => Some(ComposerKey::CtrlA),
        "input.line.end" => Some(ComposerKey::CtrlE),
        "input.delete.to.line.end" => Some(ComposerKey::CtrlK),
        "input.delete.to.line.start" => Some(ComposerKey::CtrlU),
        "input.backspace" => Some(ComposerKey::Backspace),
        "input.delete" => Some(ComposerKey::Delete),
        "prompt.clear" => Some(ComposerKey::CtrlC),
        "prompt.paste" => Some(ComposerKey::CtrlV),
        _ => None,
    }
}

/// A permission ask injected into the store (SSE ingest in production,
/// replay scripts arm it between steps).
pub fn ask_permission(store: &Store, request: &PermissionRequest) {
    store.ingest_all([Ingest::Event(crate::data::BusEvent::new(
        "permission.asked",
        serde_json::to_value(request).unwrap_or_default(),
    ))]);
}

/// Store-side effects of the engine prompt sink (the run loop applies
/// them on its next drain — the sink itself runs on a background task).
#[derive(Debug, Clone)]
pub enum PromptSinkEvent {
    /// A home submission created its session (open + navigate).
    SessionCreated { session_id: String, title: Option<String> },
}

/// The production engine prompt sink: submitting from an open session
/// POSTs `session.prompt`; a home submission first creates the session
/// (`POST /api/session`) and reports it back through the event channel.
pub struct HttpPromptSink {
    client: oc_client::Client,
    directory: String,
    events: tokio::sync::mpsc::UnboundedSender<PromptSinkEvent>,
}

impl HttpPromptSink {
    #[must_use]
    pub fn new(
        client: oc_client::Client,
        directory: String,
        events: tokio::sync::mpsc::UnboundedSender<PromptSinkEvent>,
    ) -> Self {
        Self { client, directory, events }
    }
}

impl crate::components::composer::PromptSink for HttpPromptSink {
    fn send(&self, session_id: &str, prompt: &crate::components::composer::SubmittedPrompt) {
        let client = self.client.clone();
        let events = self.events.clone();
        let directory = self.directory.clone();
        let text = prompt.text.clone();
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let target = if session_id == HOME_PROMPT_KEY {
                // Home submission: create the session first.
                let body = oc_client::types::SessionCreateBody {
                    location: Some(oc_client::types::LocationPublicRef { directory }),
                    ..oc_client::types::SessionCreateBody::default()
                };
                match client.session().create(&body).await {
                    Ok(session) => {
                        let _ = events.send(PromptSinkEvent::SessionCreated {
                            session_id: session.id.clone(),
                            title: session.title.clone(),
                        });
                        session.id
                    }
                    Err(error) => {
                        crate::keymap::log_skip(&format!("session create: {error}"));
                        return;
                    }
                }
            } else {
                session_id
            };
            let body = oc_client::types::SessionPromptBody { text, ..Default::default() };
            if let Err(error) = client.session().prompt(&target, &body).await {
                crate::keymap::log_skip(&format!("prompt: {error}"));
            }
        });
    }
}

/// Spawn the SSE connection against a resolved server (the run loop's
/// data side). Returns the ingest receiver + the open-sessions handle
/// the render loop keeps current.
#[must_use]
pub fn spawn_connection(
    client: oc_client::Client,
    config: crate::data::ConnectionConfig,
) -> (
    tokio::sync::mpsc::Receiver<Ingest>,
    tokio::task::JoinHandle<()>,
    crate::data::SharedOpenSessions,
) {
    let resync = HttpResync::new(client.clone());
    let open_sessions = resync.open_sessions_handle();
    let connection = Connection::with_config(client, config);
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let task = tokio::spawn(connection.run(tx, resync));
    (rx, task, open_sessions)
}

/// The live run loop: terminal acquire/release, event drain, render.
/// Blocks until `app.exit`; returns the terminal to its prior state.
///
/// Errors clearly when stdin is not a TTY (the TUI needs a terminal).
pub async fn run(client: oc_client::Client) -> Result<(), String> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(
            "the TUI requires an interactive terminal (stdin/stdout are not TTYs)".to_owned()
        );
    }
    let size = crossterm::terminal::size().ok();
    run_with_size(client, size).await
}

/// [`run`] with an explicit terminal size (tests); `None` = the
/// terminal query. The loop: SSE ingest drain → open-session sync →
/// clock ticks (spinner/toast) → config-reload check → coalesced
/// render → crossterm key wait (100 ms poll tick). `app.exit` (and any
/// `Ctrl+c` on an empty prompt) breaks the loop.
#[allow(clippy::too_many_lines)]
pub async fn run_with_size(
    client: oc_client::Client,
    size: Option<(u16, u16)>,
) -> Result<(), String> {
    use crossterm::event::Event as CrosstermEvent;
    use crossterm::event::KeyEventKind;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::backend::CrosstermBackend;

    // Terminal acquire (app.tsx's renderer acquire/release): raw mode +
    // alternate screen, restored on every exit path.
    enable_raw_mode().map_err(|e| e.to_string())?;
    let _ = crossterm::execute!(std::io::stdout(), EnterAlternateScreen);
    let mut terminal = ratatui::Terminal::new(CrosstermBackend::new(std::io::stdout()))
        .map_err(|e| e.to_string())?;
    let size = size.unwrap_or((100, 30));
    let _ = terminal.resize(ratatui::layout::Rect::new(0, 0, size.0, size.1));

    // App + config boot.
    let store = Store::new(crate::reactive::Runtime::new());
    let mut app = App::with_store(store, size);
    let config = crate::config::TuiConfig::load(&oc_util::global_roots::resolve().config);
    // The system-mode hint comes from the terminal probe — run inside
    // the input thread below; dark until it reports.
    let mut system_mode = Mode::Dark;

    // SSE ingest + the engine prompt sink (home submissions create
    // their session and report back through the sink events).
    let (mut rx, connection_task, open_sessions) =
        spawn_connection(client.clone(), crate::data::ConnectionConfig::default());
    let (sink_tx, mut sink_rx) = tokio::sync::mpsc::unbounded_channel::<PromptSinkEvent>();
    let directory =
        std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
    app.store.set_prompt_sink(std::rc::Rc::new(HttpPromptSink::new(client, directory, sink_tx)));

    // Config reload: mtime check per tick (the reference watches the dir).
    let mut config_mtime = config
        .path
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok());
    let mut last_tick = std::time::Instant::now();
    let mut redraw_at = 0u64;

    // Input thread: crossterm's blocking read (keys + resize) bridged
    // into the async loop (the detached thread exits with the process;
    // terminal teardown below runs first). The thread FIRST probes the
    // terminal background (OSC 11 — same-thread input ownership, no
    // keystroke stealing) and reports the system mode before reading.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<CrosstermEvent>(64);
    let (probe_tx, probe_rx) = std::sync::mpsc::channel::<Mode>();
    std::thread::spawn(move || {
        let background = crate::theme::query_terminal_background();
        let mode =
            background.map_or(Mode::Dark, |(r, g, b)| crate::theme::mode_for_background(r, g, b));
        let _ = probe_tx.send(mode);
        while let Ok(event) = crossterm::event::read() {
            if event_tx.blocking_send(event).is_err() {
                break;
            }
        }
    });
    if let Ok(mode) = probe_rx.recv_timeout(std::time::Duration::from_millis(600)) {
        system_mode = mode;
    }
    // The probe toggles raw mode itself; the loop owns it from here.
    let _ = crossterm::terminal::enable_raw_mode();
    app.apply_config(&config, system_mode);

    let result: Result<(), String> = loop {
        // Drain ingest.
        while let Ok(message) = rx.try_recv() {
            if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("[tui] ingest: {message:?}");
            }
            app.store.ingest_all([message]);
        }
        // Drain the prompt-sink events (home submissions creating
        // their session open a tab and navigate).
        while let Ok(event) = sink_rx.try_recv() {
            match event {
                PromptSinkEvent::SessionCreated { session_id, title } => {
                    app.store.open_tab(&session_id, title);
                }
            }
        }
        // Keep the resync's open-session set current.
        if let Ok(mut ids) = open_sessions.lock() {
            *ids = app.store.open_session_ids();
        }
        // Toast/spinner clock.
        let now_ms = system_now_ms();
        if last_tick.elapsed() >= std::time::Duration::from_millis(100) {
            app.store.advance_clock(now_ms);
            app.store.toast_advance(now_ms);
            last_tick = std::time::Instant::now();
        }
        // Config reload: keybinds + theme re-merge through the store.
        let mtime = config
            .path
            .as_ref()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|meta| meta.modified().ok());
        if mtime != config_mtime {
            config_mtime = mtime;
            if let Some(reloaded) = config.reload() {
                app.apply_config(&reloaded, system_mode);
            }
        }
        // Render (coalesced; the clock tick keeps spinners/toasts live).
        app.store.runtime.run_effects();
        let generation = app.store.redraw_generation();
        if generation != redraw_at || app.store.clock() > 0 {
            redraw_at = generation;
            let frame = app.frame();
            let draw = terminal.draw(|surface| {
                let buffer = surface.buffer_mut();
                for (y, row) in frame.cells.iter().enumerate() {
                    for (x, cell) in row.iter().enumerate() {
                        let Ok(x) = u16::try_from(x) else { continue };
                        let Ok(y) = u16::try_from(y) else { continue };
                        let slot = &mut buffer[(x, y)];
                        slot.set_char(cell.ch);
                        slot.set_fg(to_ratatui_color(cell.fg));
                        slot.set_bg(to_ratatui_color(cell.bg));
                        if cell.attrs & 1 != 0 {
                            slot.set_style(
                                ratatui::style::Style::new()
                                    .add_modifier(ratatui::style::Modifier::BOLD),
                            );
                        }
                    }
                }
            });
            if draw.is_err() {
                break Err("terminal draw failed".to_owned());
            }
        }
        // Input / tick wait.
        let event = tokio::select! {
            maybe = event_rx.recv() => maybe,
            () = tokio::time::sleep(std::time::Duration::from_millis(100)) => None,
        };
        match event {
            Some(CrosstermEvent::Key(key)) => {
                if key.kind == KeyEventKind::Press {
                    if let Some(spec) = crossterm_key_to_spec(&key) {
                        if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
                            eprintln!("[tui] key: {spec}");
                        }
                        app.press(&spec);
                    }
                }
            }
            // SIGWINCH: relayout at the new size.
            Some(CrosstermEvent::Resize(width, height)) => {
                let _ = terminal.resize(ratatui::layout::Rect::new(0, 0, width, height));
                app.resize((width, height));
                redraw_at = 0; // force one redraw
            }
            _ => {}
        }
        if app.exit_requested {
            break Ok(());
        }
    };

    // Teardown.
    connection_task.abort();
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(std::io::stdout(), LeaveAlternateScreen);
    result
}

fn system_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// crossterm key event → the pump's key spec vocabulary.
fn crossterm_key_to_spec(key: &crossterm::event::KeyEvent) -> Option<String> {
    use crossterm::event::{KeyCode, KeyModifiers};
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char(ch) => Some(format!("ctrl+{}", ch.to_ascii_lowercase())),
            _ => None,
        };
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        return match key.code {
            KeyCode::Char(ch) => Some(format!("alt+{}", ch.to_ascii_lowercase())),
            KeyCode::Down => Some("alt+down".to_owned()),
            KeyCode::Up => Some("alt+up".to_owned()),
            _ => None,
        };
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        return match key.code {
            KeyCode::Char(ch) => Some(ch.to_ascii_uppercase().to_string()),
            KeyCode::Tab => Some("shift+tab".to_owned()),
            KeyCode::BackTab => Some("ctrl+shift+tab".to_owned()),
            KeyCode::F(f) => Some(format!("shift+f{f}")),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Char(ch) => Some(ch.to_string()),
        KeyCode::Enter => Some("return".to_owned()),
        KeyCode::Esc => Some("escape".to_owned()),
        KeyCode::Backspace => Some("backspace".to_owned()),
        KeyCode::Delete => Some("delete".to_owned()),
        KeyCode::Tab => Some("tab".to_owned()),
        KeyCode::BackTab => Some("ctrl+shift+tab".to_owned()),
        KeyCode::Up => Some("up".to_owned()),
        KeyCode::Down => Some("down".to_owned()),
        KeyCode::Left => Some("left".to_owned()),
        KeyCode::Right => Some("right".to_owned()),
        KeyCode::Home => Some("home".to_owned()),
        KeyCode::End => Some("end".to_owned()),
        KeyCode::PageUp => Some("pageup".to_owned()),
        KeyCode::PageDown => Some("pagedown".to_owned()),
        KeyCode::F(f) => Some(format!("f{f}")),
        _ => None,
    }
}

fn to_ratatui_color(color: crate::render::CellColor) -> ratatui::style::Color {
    match color {
        crate::render::CellColor::Rgb(r, g, b) => ratatui::style::Color::Rgb(r, g, b),
        crate::render::CellColor::Indexed(index) => ratatui::style::Color::Indexed(index),
        crate::render::CellColor::Default => ratatui::style::Color::Reset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `apply_config` resolves the theme through the registry and
    /// merges keybind overrides into the live keymap; a config reload
    /// (mtime change → reload → apply) re-merges atomically.
    #[test]
    fn config_applies_theme_and_keybinds_and_reloads() {
        let mut app = App::new((100, 30));
        app.store.open_tab("s1", Some("S".to_owned()));

        // Boot config: a custom-ish theme pick + a ctrl+q exit override.
        let boot = crate::config::TuiConfig::parse(
            r#"{ "theme": { "name": "catppuccin" }, "keybinds": { "app.exit": "ctrl+q" } }"#,
            None,
        );
        app.apply_config(&boot, Mode::Dark);
        assert_eq!(app.store.theme().name, "catppuccin", "theme resolved through the registry");
        assert_ne!(
            app.store.theme().resolved.tokens.background.base,
            ThemeState::v2_dark().resolved.tokens.background.base,
            "a different theme document is active"
        );
        // The override merged: ctrl+q exits, the ctrl+c default replaced.
        app.press("ctrl+q");
        assert!(app.exit_requested, "user override wins");

        // Reload: a new binding table replaces the old one wholesale.
        let mut app = App::new((100, 30));
        app.store.open_tab("s1", Some("S".to_owned()));
        let reloaded = crate::config::TuiConfig::parse(
            r#"{ "theme": { "name": "bogus", "mode": "light" }, "keybinds": { "app.exit": "ctrl+q" } }"#,
            None,
        );
        app.apply_config(&reloaded, Mode::Dark);
        // Unknown theme names fall back to `opencode`; mode applies.
        assert_eq!(app.store.theme().name, "opencode");
        assert_eq!(app.store.theme().mode, Mode::Light);
        app.press("q"); // types, not exits
        assert!(!app.exit_requested);
        app.press("backspace");
        app.press("ctrl+q");
        assert!(app.exit_requested);

        // A later store-side set_keybinds (the reload event) re-merges
        // into the live keymap without a pump restart.
        let mut app = App::new((100, 30));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.press("ctrl+x");
        app.press("p");
        assert_eq!(app.store.dialogs().len(), 0, "leader+p unbound (pending, then p typed)");
        assert_eq!(app.store.prompt_draft("s1").text, "p");
        app.press("backspace");
        app.store.set_keybinds(crate::keymap::parse_keybind_overrides(
            &serde_json::json!({ "command.palette.show": "<leader>p" }),
        ));
        app.press("ctrl+x");
        app.press("p");
        assert_eq!(app.store.dialogs().len(), 1, "the reload re-merge re-bound leader+p");
    }

    /// The system-mode hint flows from the terminal detection into the
    /// applied theme (`theme.mode: "system"`).
    #[test]
    fn system_mode_hint_applies() {
        let mut app = App::new((100, 30));
        let config = crate::config::TuiConfig::parse(r#"{ "theme": { "mode": "system" } }"#, None);
        app.apply_config(&config, Mode::Light);
        assert_eq!(app.store.theme().mode, Mode::Light, "light terminal → light mode");
        let mut app = App::new((100, 30));
        app.apply_config(&config, Mode::Dark);
        assert_eq!(app.store.theme().mode, Mode::Dark, "dark terminal → dark mode");
    }
}
