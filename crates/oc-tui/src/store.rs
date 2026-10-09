//! The TUI store root: session-tabs state (the ported
//! `session-tabs-model` semantics), current route, dialog stack, theme
//! state and connection state — as reactive signals over
//! [`crate::reactive`], with **single-consumer event ingest** from the
//! SSE data layer and coalesced redraw scheduling.
//!
//! ## Ingest discipline (drain-before-reconcile)
//!
//! The `run` consumer-drain fix is the template: a resync snapshot may
//! never overtake live events still queued behind it. The store therefore
//! has exactly ONE ingest entry point ([`Store::ingest_all`]) that
//! applies a drained batch in arrival order; [`crate::data::Connection`]
//! serializes everything — live events, resync fetches, connection
//! transitions — through one ordered channel, and the store reconciles
//! only after the channel is drained. Mutations bump a redraw *request
//! counter* (never an immediate redraw); the render loop compares the
//! counter between ticks.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;

use crate::components::composer::{
    Composer, PromptDraft, PromptHistory, PromptSink, RecordingPromptSink, SubmittedPrompt,
};
use crate::components::dialog_select::SelectState;
use crate::components::footer::{FooterMachine, PromptRoute};
use crate::components::message::model::{Message, Part, PartTime};
use crate::components::toast::{ToastOptions, ToastQueue};
use crate::data::{BusEvent, ConnectionState, Ingest};
use crate::reactive::{Effect, Runtime, Signal};
use crate::theme::ThemeState;

/// Route (`context/route.tsx`): home or a session view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Session { session_id: String },
}

/// What currently receives key input: the route, or the top dialog
/// (the reference's keymap `modal` mode + the focused input renderable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Route,
    Dialog,
}

/// Dialog chrome size (`DialogSize` → `dialogWidth`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogSize {
    /// 60 columns.
    #[default]
    Medium,
    /// 88 columns.
    Large,
    /// 116 columns.
    Xlarge,
}

impl DialogSize {
    #[must_use]
    pub const fn width(self) -> usize {
        match self {
            Self::Medium => 60,
            Self::Large => 88,
            Self::Xlarge => 116,
        }
    }
}

/// One open dialog's identity (the `ui/dialog.tsx` stack). The content
/// state ([`SelectState`] for pickers, the prompt value for renames)
/// rides along so the keypress pump can drive it.
#[derive(Debug, Clone, PartialEq)]
pub enum DialogKind {
    /// The command palette (`ctrl+p`).
    CommandPalette,
    /// `DialogSessionRename`: rename the active session.
    SessionRename { session_id: String },
    /// `DialogThemeList`: the built-in theme registry.
    ThemeList,
    /// Server-backed listings (model/agent/session lists): the
    /// oc-client fetches land in wave 3 — a placeholder select until
    /// then (TODO(7.3): wire the `*.list` fetches).
    ServerList { command: &'static str, title: &'static str },
}

/// One stack entry: the dialog kind, chrome size and content state.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogEntry {
    pub kind: DialogKind,
    pub size: DialogSize,
    /// Picker state (filter/selection/scroll) for select-style kinds.
    pub select: SelectState,
    /// Prompt input value for prompt-style kinds (rename).
    pub value: String,
}

/// One open session tab (`SessionTab`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTab {
    pub session_id: String,
    pub title: Option<String>,
}

/// Unread marker on a tab (`SessionTabUnread`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    Activity,
    Error,
}

/// A closed tab eligible for reopening (`ClosedSessionTab`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedTab {
    pub tab: SessionTab,
    pub index: usize,
}

/// In-memory navigation history (`SessionTabHistory`), bounded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabHistory {
    pub entries: Vec<String>,
    pub index: isize,
}

pub const CLOSED_TAB_LIMIT: usize = 25;
pub const TAB_HISTORY_LIMIT: usize = 100;
pub const NEW_SESSION_TAB_TITLE: &str = "New session";

/// The tabs model (`session-tabs-model.ts`): pure functions, ported
/// one-to-one so the golden components share semantics with the store.
pub mod model {
    use super::{ClosedTab, SessionTab, TabHistory, CLOSED_TAB_LIMIT, TAB_HISTORY_LIMIT};

    /// `openSessionTab`: append or update the title in place.
    #[must_use]
    pub fn open_session_tab(mut tabs: Vec<SessionTab>, tab: SessionTab) -> Vec<SessionTab> {
        if let Some(existing) = tabs.iter_mut().find(|item| item.session_id == tab.session_id) {
            if tab.title.is_some() && existing.title != tab.title {
                existing.title = tab.title;
            }
            return tabs;
        }
        tabs.push(tab);
        tabs
    }

    /// `closeSessionTab`: removes the tab and proposes the next one
    /// (right neighbor, else left).
    #[must_use]
    pub fn close_session_tab(
        tabs: &[SessionTab],
        session_id: &str,
    ) -> (Vec<SessionTab>, Option<String>) {
        let Some(index) = tabs.iter().position(|tab| tab.session_id == session_id) else {
            return (tabs.to_vec(), None);
        };
        let next = tabs
            .get(index + 1)
            .or_else(|| index.checked_sub(1).and_then(|left| tabs.get(left)))
            .map(|tab| tab.session_id.clone());
        let mut remaining = tabs.to_vec();
        remaining.remove(index);
        (remaining, next)
    }

    /// `recordClosedSessionTab`: dedup + append, bounded to the limit.
    #[must_use]
    pub fn record_closed_tab(
        mut stack: Vec<ClosedTab>,
        tab: SessionTab,
        index: usize,
    ) -> Vec<ClosedTab> {
        stack.retain(|entry| entry.tab.session_id != tab.session_id);
        stack.push(ClosedTab { tab, index });
        if stack.len() > CLOSED_TAB_LIMIT {
            let excess = stack.len() - CLOSED_TAB_LIMIT;
            stack.drain(0..excess);
        }
        stack
    }

    /// `reopenSessionTab`: pop the most recently closed tab that is not
    /// already open (or the entry for `session_id`) and restore it at its
    /// original position.
    #[must_use]
    pub fn reopen_session_tab(
        mut stack: Vec<ClosedTab>,
        tabs: &[SessionTab],
        session_id: Option<&str>,
    ) -> (Vec<ClosedTab>, Option<Vec<SessionTab>>, Option<String>) {
        while !stack.is_empty() {
            let index = match session_id {
                Some(id) => stack.iter().position(|entry| entry.tab.session_id == id),
                None => Some(stack.len() - 1),
            };
            let Some(index) = index else { break };
            let entry = stack.remove(index);
            if tabs.iter().any(|tab| tab.session_id == entry.tab.session_id) {
                continue;
            }
            let mut restored = tabs.to_vec();
            let at = entry.index.min(restored.len());
            restored.insert(at, entry.tab.clone());
            let selected = entry.tab.session_id;
            return (stack, Some(restored), Some(selected));
        }
        (stack, None, None)
    }

    /// `moveSessionTab`: reorder, clamped.
    #[must_use]
    pub fn move_session_tab(
        mut tabs: Vec<SessionTab>,
        session_id: &str,
        index: usize,
    ) -> Vec<SessionTab> {
        let Some(from) = tabs.iter().position(|tab| tab.session_id == session_id) else {
            return tabs;
        };
        let to = index.min(tabs.len() - 1);
        if from == to {
            return tabs;
        }
        let tab = tabs.remove(from);
        tabs.insert(to, tab);
        tabs
    }

    /// `cycleSessionTab`: next/previous tab (optionally filtered by a
    /// predicate, e.g. unread-only cycling).
    #[must_use]
    pub fn cycle_session_tab(
        tabs: &[SessionTab],
        active: Option<&str>,
        direction: i8,
        matches: impl Fn(&SessionTab) -> bool,
    ) -> Option<String> {
        if tabs.is_empty() {
            return None;
        }
        let len = i64::try_from(tabs.len()).unwrap_or(i64::MAX);
        let start: i64 = match tabs.iter().position(|tab| Some(tab.session_id.as_str()) == active) {
            None if direction == 1 => -1,
            None => 0,
            Some(index) => i64::try_from(index).unwrap_or(i64::MAX),
        };
        for offset in 1..=tabs.len() {
            let raw = start + i64::from(direction) * i64::try_from(offset).unwrap_or(0) + len * 2;
            let index = usize::try_from(raw % len).expect("non-negative modulo");
            let candidate = &tabs[index];
            if matches(candidate) {
                return Some(candidate.session_id.clone());
            }
        }
        None
    }

    /// `recordSessionTabHistory`: truncate the forward tail, append,
    /// bound the length.
    #[must_use]
    pub fn record_tab_history(history: &TabHistory, session_id: &str) -> TabHistory {
        if history.index >= 0
            && history.entries.get(usize::try_from(history.index).unwrap_or(0)).map(String::as_str)
                == Some(session_id)
        {
            return history.clone();
        }
        let keep = usize::try_from((history.index + 1).max(0)).unwrap_or(0);
        let mut entries: Vec<String> = history.entries.iter().take(keep).cloned().collect();
        entries.push(session_id.to_owned());
        if entries.len() > TAB_HISTORY_LIMIT {
            let excess = entries.len() - TAB_HISTORY_LIMIT;
            entries.drain(0..excess);
        }
        TabHistory { index: isize::try_from(entries.len() - 1).unwrap_or(isize::MAX), entries }
    }
}

/// Reactive tabs state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabsState {
    pub tabs: Vec<SessionTab>,
    /// Quick slots 1-9 (+0 = 10th) address tabs by position.
    pub unread: HashMap<String, Unread>,
    pub pinned: Vec<String>,
    pub closed: Vec<ClosedTab>,
    pub history: TabHistory,
}

impl TabsState {
    #[must_use]
    pub fn active(&self, route: &Route) -> Option<String> {
        match route {
            Route::Home => None,
            Route::Session { session_id } => Some(session_id.clone()),
        }
    }
}

/// Per-session message trees (lane C): ordered messages with parts,
/// built by the SSE message reducers below (see
/// `routes/session/rows.ts`'s event subscriptions — the reference
/// appends parts live and re-reduces from the projected rows).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageState {
    pub by_session: HashMap<String, Vec<Message>>,
}

/// The store root.
/// The composer's store slice: per-session drafts (the draft stash),
/// the shared prompt history, and the engine prompt sink.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PromptState {
    /// Drafts keyed by session (the stash `PromptRef.set` restores).
    pub drafts: std::collections::HashMap<String, PromptDraft>,
    pub history: PromptHistory,
}

pub struct Store {
    /// Reactive runtime owning the graph (pump via `run_effects`).
    pub runtime: Runtime,
    tabs: Signal<TabsState>,
    route: Signal<Route>,
    dialogs: Signal<Vec<DialogEntry>>,
    theme: Signal<ThemeState>,
    connection: Signal<ConnectionState>,
    /// The mini footer state machine (view / route / blocker queues).
    footer: Signal<FooterMachine>,
    /// The toast overlay queue (current surface + pending stack).
    toasts: Signal<ToastQueue>,
    /// Session message trees (lane C message flow).
    messages: Signal<MessageState>,
    /// The composer state (drafts + history) for the session route.
    prompt: Signal<PromptState>,
    /// The engine prompt API stub submissions forward to
    /// (TODO(7.3): the real `session.prompt` oc-client call).
    prompt_sink: std::rc::Rc<dyn PromptSink>,
    /// The injected clock (ms since boot) — the ONLY time source
    /// animations may read (`ManualClock` semantics: a frozen clock pins
    /// spinner frame 0; render happens on tick, only when it advances).
    clock: Signal<u64>,
    /// Coalesced redraw request counter (bumped by the redraw effect;
    /// the render loop compares between ticks — never redraw inline).
    redraw_requests: Rc<Cell<u64>>,
    _redraw_effect: crate::reactive::EffectHandle,
}

impl Store {
    #[must_use]
    pub fn new(runtime: Runtime) -> Self {
        let tabs = Signal::new(&runtime, TabsState::default());
        let route = Signal::new(&runtime, Route::Home);
        let dialogs = Signal::new(&runtime, Vec::<DialogEntry>::new());
        let theme = Signal::new(&runtime, ThemeState::v2_dark());
        let connection = Signal::new(&runtime, ConnectionState::Connecting);
        let footer = Signal::new(&runtime, FooterMachine::new());
        let toasts = Signal::new(&runtime, ToastQueue::new());
        let messages = Signal::new(&runtime, MessageState::default());
        let prompt = Signal::new(&runtime, PromptState::default());
        let prompt_sink: std::rc::Rc<dyn PromptSink> =
            std::rc::Rc::new(RecordingPromptSink::default());
        let clock = Signal::new(&runtime, 0u64);
        let redraw_requests = Rc::new(Cell::new(0u64));
        // Any root-state change requests exactly one redraw per pump.
        let bump = Rc::clone(&redraw_requests);
        let watch_tabs = tabs.clone();
        let watch_route = route.clone();
        let watch_dialogs = dialogs.clone();
        let watch_theme = theme.clone();
        let watch_connection = connection.clone();
        let watch_footer = footer.clone();
        let watch_toasts = toasts.clone();
        let watch_messages = messages.clone();
        let watch_prompt = prompt.clone();
        let watch_clock = clock.clone();
        let redraw_effect = Effect::new(&runtime, move || {
            let _ = (
                watch_tabs.get(),
                watch_route.get(),
                watch_dialogs.get(),
                watch_theme.get(),
                watch_connection.get(),
                watch_footer.get(),
                watch_toasts.get(),
                watch_messages.get(),
                watch_prompt.get(),
                watch_clock.get(),
            );
            bump.set(bump.get() + 1);
        });
        Self {
            runtime,
            tabs,
            route,
            dialogs,
            theme,
            connection,
            footer,
            toasts,
            messages,
            prompt,
            prompt_sink,
            clock,
            redraw_requests,
            _redraw_effect: redraw_effect,
        }
    }

    // -- accessors --------------------------------------------------------

    #[must_use]
    pub fn tabs(&self) -> TabsState {
        self.tabs.get()
    }

    #[must_use]
    pub fn route(&self) -> Route {
        self.route.get()
    }

    #[must_use]
    pub fn dialogs(&self) -> Vec<DialogEntry> {
        self.dialogs.get()
    }

    /// Where key input goes: the top dialog when one is open, else the
    /// route (the reference pushes keymap mode `modal` while the stack
    /// is non-empty).
    #[must_use]
    pub fn focus(&self) -> Focus {
        if self.dialogs.get().is_empty() {
            Focus::Route
        } else {
            Focus::Dialog
        }
    }

    #[must_use]
    pub fn theme(&self) -> ThemeState {
        self.theme.get()
    }

    #[must_use]
    pub fn connection(&self) -> ConnectionState {
        self.connection.get()
    }

    /// The footer state machine (view / route / blocker queues).
    #[must_use]
    pub fn footer(&self) -> FooterMachine {
        self.footer.get()
    }

    /// The toast overlay queue.
    #[must_use]
    pub fn toasts(&self) -> ToastQueue {
        self.toasts.get()
    }

    // -- footer ------------------------------------------------------------------

    /// Open a footer picker route over the composer (prompt view only).
    pub fn open_footer_route(&self, route: PromptRoute) {
        self.footer.update(|footer| footer.open_route(route));
    }

    /// Collapse any open footer panel back to the composer.
    pub fn close_footer_panel(&self) {
        self.footer.update(super::components::footer::FooterMachine::close_panel);
    }

    // -- toasts ------------------------------------------------------------------

    /// Show a toast (immediately when idle, queued behind a live one).
    pub fn toast_show(&self, options: ToastOptions) {
        self.toasts.update(|queue| queue.show(options));
    }

    /// Dismiss the current toast (rotating the next in).
    pub fn toast_dismiss(&self) {
        self.toasts.update(super::components::toast::ToastQueue::dismiss);
    }

    /// Pause/resume the current toast's countdown (`now_ms` = clock).
    pub fn toast_set_paused(&self, paused: bool, now_ms: u64) {
        self.toasts.update(|queue| {
            if paused {
                queue.pause(now_ms);
            } else {
                queue.resume(now_ms);
            }
        });
    }

    /// Fire the current toast's expiry when its duration elapsed (the
    /// render loop's clock tick).
    pub fn toast_advance(&self, now_ms: u64) {
        self.toasts.update(|queue| {
            queue.advance(now_ms);
        });
    }

    // -- prompt (composer, wave 2) ---------------------------------------------

    /// The session's draft (empty when never edited — the composer's
    /// placeholder state).
    #[must_use]
    pub fn prompt_draft(&self, session_id: &str) -> PromptDraft {
        self.prompt.get().drafts.get(session_id).cloned().unwrap_or_default()
    }

    /// The composer editing state for a session (draft + history),
    /// ready for the renderers and the keypress pump.
    #[must_use]
    pub fn composer(&self, session_id: &str) -> Composer {
        let state = self.prompt.get();
        Composer {
            draft: state.drafts.get(session_id).cloned().unwrap_or_default(),
            history: state.history,
            ..Composer::default()
        }
    }

    /// Store a composer edit (draft + history advance together).
    pub fn set_composer(&self, session_id: &str, composer: &Composer) {
        self.prompt.update(|state| {
            if composer.draft.is_empty() {
                state.drafts.remove(session_id);
            } else {
                state.drafts.insert(session_id.to_owned(), composer.draft.clone());
            }
            state.history = composer.history.clone();
        });
    }

    /// `prompt.submit`: take the session's draft, record it in the
    /// history ring, and forward to the engine prompt stub.
    #[must_use = "the submitted prompt feeds the engine call"]
    pub fn submit_prompt(&self, session_id: &str) -> Option<SubmittedPrompt> {
        let draft = self.prompt_draft(session_id);
        if draft.is_empty() {
            return None;
        }
        self.prompt.update(|state| {
            state.drafts.remove(session_id);
            state.history = state.history.clone().push(&draft.text);
        });
        let prompt = SubmittedPrompt { text: draft.text, files: draft.files, agents: draft.agents };
        self.prompt_sink.send(session_id, &prompt);
        Some(prompt)
    }

    /// Swap the engine prompt sink (tests inject recorders; wave 3
    /// wires the oc-client call).
    pub fn set_prompt_sink(&mut self, sink: std::rc::Rc<dyn PromptSink>) {
        self.prompt_sink = sink;
    }

    // -- messages (lane C) -----------------------------------------------------

    /// The ordered messages of one session (the transcript's input).
    #[must_use]
    pub fn messages(&self, session_id: &str) -> Vec<Message> {
        self.messages.get().by_session.get(session_id).cloned().unwrap_or_default()
    }

    /// The whole per-session message state (route switches, resync).
    #[must_use]
    pub fn message_state(&self) -> MessageState {
        self.messages.get()
    }

    /// Replace one session's messages wholesale (history hydration /
    /// message.list resync — wave 3 wires the fetch).
    pub fn set_messages(&self, session_id: &str, messages: Vec<Message>) {
        self.messages.update(|state| {
            state.by_session.insert(session_id.to_owned(), messages);
        });
    }

    /// Mutate one session's message tree through a closure (the SSE
    /// reducers' shared entry).
    fn update_messages(&self, session_id: &str, update: impl FnOnce(&mut Vec<Message>)) {
        self.messages.update(|state| {
            update(state.by_session.entry(session_id.to_owned()).or_default());
        });
    }

    // -- clock (injected; ManualClock semantics) --------------------------------

    /// The injected clock (ms) — the only time source the render reads.
    #[must_use]
    pub fn clock(&self) -> u64 {
        self.clock.get()
    }

    /// Advance the clock to `now_ms`. Monotonic: a smaller/equal value
    /// is a no-op (no signal write ⇒ no redraw — render-on-tick only
    /// when the clock actually advances, frozen at 0 in tests).
    pub fn advance_clock(&self, now_ms: u64) {
        if now_ms > self.clock.get() {
            self.clock.set(now_ms);
        }
    }

    // -- redraw scheduling -------------------------------------------------

    /// Request a redraw (bumps the counter; the render loop drains it).
    pub fn request_redraw(&self) {
        self.redraw_requests.set(self.redraw_requests.get() + 1);
    }

    /// Monotonic request counter — compare between ticks to decide
    /// whether a redraw is needed.
    #[must_use]
    pub fn redraw_generation(&self) -> u64 {
        self.redraw_requests.get()
    }

    // -- tabs ---------------------------------------------------------------

    /// Open (or update) a tab and navigate to it.
    pub fn open_tab(&self, session_id: &str, title: Option<String>) {
        self.tabs.update(|state| {
            state.tabs = model::open_session_tab(
                std::mem::take(&mut state.tabs),
                SessionTab { session_id: session_id.to_owned(), title },
            );
            state.history = model::record_tab_history(&state.history, session_id);
        });
        self.navigate(Route::Session { session_id: session_id.to_owned() });
    }

    /// Close a tab (`session_id = None` closes the new-session pseudo
    /// tab — a no-op in 7.1, which has no new-tab state yet).
    pub fn close_tab(&self, session_id: Option<&str>) {
        let Some(session_id) = session_id else { return };
        let tabs = self.tabs.get();
        let Some(index) = tabs.tabs.iter().position(|tab| tab.session_id == session_id) else {
            return;
        };
        let (remaining, next) = model::close_session_tab(&tabs.tabs, session_id);
        self.tabs.update(|state| {
            let closed = state.tabs[index].clone();
            state.closed =
                model::record_closed_tab(std::mem::take(&mut state.closed), closed, index);
            state.tabs = remaining;
            state.unread.remove(session_id);
        });
        // Like the reference: focus the proposed neighbor, else home.
        match next {
            Some(neighbor) if self.tabs.get().tabs.iter().any(|tab| tab.session_id == neighbor) => {
                self.navigate(Route::Session { session_id: neighbor });
            }
            _ => self.navigate(Route::Home),
        }
    }

    /// Reopen the most recently closed tab (or a specific session).
    #[must_use]
    pub fn reopen_tab(&self, session_id: Option<&str>) -> Option<String> {
        let tabs = self.tabs.get();
        let (remaining, restored, selected) =
            model::reopen_session_tab(tabs.closed.clone(), &tabs.tabs, session_id);
        if let Some(restored) = restored {
            self.tabs.update(|state| {
                state.closed = remaining;
                state.tabs = restored;
            });
            if let Some(id) = selected.clone() {
                self.navigate(Route::Session { session_id: id });
            }
        }
        selected
    }

    /// Select a tab (navigates to its session).
    pub fn select_tab(&self, session_id: &str) {
        if self.tabs.get().tabs.iter().any(|tab| tab.session_id == session_id) {
            self.tabs.update(|state| {
                state.history = model::record_tab_history(&state.history, session_id);
            });
            self.navigate(Route::Session { session_id: session_id.to_owned() });
        }
    }

    /// Quick slot 1-9 (and 10 via 0): select the tab at that position.
    #[must_use]
    pub fn select_quick_slot(&self, slot: u8) -> Option<String> {
        let index = if slot == 0 { 9 } else { usize::from(slot.saturating_sub(1)) };
        let tabs = self.tabs.get();
        let tab = tabs.tabs.get(index)?.clone();
        self.select_tab(&tab.session_id);
        Some(tab.session_id)
    }

    /// Cycle to the next/previous tab (optionally unread-only).
    #[must_use]
    pub fn cycle_tab(&self, direction: i8, unread_only: bool) -> Option<String> {
        let tabs = self.tabs.get();
        let active = tabs.active(&self.route.get());
        let next = if unread_only {
            let unread = tabs.unread.clone();
            model::cycle_session_tab(&tabs.tabs, active.as_deref(), direction, |tab| {
                unread.contains_key(&tab.session_id)
            })
        } else {
            model::cycle_session_tab(&tabs.tabs, active.as_deref(), direction, |_| true)
        };
        if let Some(next) = &next {
            self.select_tab(next);
        }
        next
    }

    /// Toggle pinning a session in the session list.
    pub fn toggle_pin(&self, session_id: &str) {
        self.tabs.update(|state| {
            if let Some(position) = state.pinned.iter().position(|id| id == session_id) {
                state.pinned.remove(position);
            } else {
                state.pinned.push(session_id.to_owned());
            }
        });
    }

    /// Mark a tab's unread state (activity/error); selecting the tab
    /// clears it (the reference clears on view).
    pub fn set_unread(&self, session_id: &str, unread: Option<Unread>) {
        self.tabs.update(|state| match unread {
            Some(unread) => {
                state.unread.insert(session_id.to_owned(), unread);
            }
            None => {
                state.unread.remove(session_id);
            }
        });
    }

    // -- route / dialogs / theme / connection --------------------------------

    pub fn navigate(&self, route: Route) {
        if let Route::Session { session_id } = &route {
            self.tabs.update(|state| {
                state.history = model::record_tab_history(&state.history, session_id);
                // Viewing clears unread.
                state.unread.remove(session_id);
            });
        }
        self.route.set(route);
    }

    pub fn push_dialog(&self, kind: DialogKind) {
        let entry = DialogEntry {
            kind,
            size: DialogSize::default(),
            select: SelectState::default(),
            value: String::new(),
        };
        self.dialogs.update(|stack| stack.push(entry));
    }

    pub fn pop_dialog(&self) {
        self.dialogs.update(|stack| {
            stack.pop();
        });
    }

    // -- dialog framework (`ui/dialog.tsx` stack semantics) --------------------

    /// Push a dialog entry (kind + size + fresh content state).
    pub fn open_dialog(&self, kind: DialogKind, size: DialogSize) {
        let entry =
            DialogEntry { kind, size, select: SelectState::default(), value: String::new() };
        self.dialogs.update(|stack| stack.push(entry));
    }

    /// Replace the whole stack with one dialog (`dialog.replace`):
    /// every open dialog's close semantics run, then the new entry
    /// becomes the only one.
    pub fn replace_dialog(&self, kind: DialogKind, size: DialogSize) {
        let entry =
            DialogEntry { kind, size, select: SelectState::default(), value: String::new() };
        self.dialogs.update(|stack| {
            stack.clear();
            stack.push(entry);
        });
    }

    /// Close the top dialog (`escape` in the modal layer — the entry's
    /// close semantics, then the pop).
    pub fn close_dialog(&self) {
        self.dialogs.update(|stack| {
            stack.pop();
        });
    }

    /// Close every dialog (`dialog.clear`).
    pub fn clear_dialogs(&self) {
        self.dialogs.update(std::vec::Vec::clear);
    }

    /// Mutate the top dialog's entry (selection moves, filter typing,
    /// prompt input).
    pub fn update_dialog(&self, update: impl FnOnce(&mut DialogEntry)) {
        self.dialogs.update(|stack| {
            if let Some(top) = stack.last_mut() {
                update(top);
            }
        });
    }

    /// The top dialog entry, if any.
    #[must_use]
    pub fn top_dialog(&self) -> Option<DialogEntry> {
        self.dialogs.get().into_iter().last()
    }

    pub fn set_theme(&self, theme: ThemeState) {
        self.theme.set(theme);
    }

    pub fn switch_theme_mode(&self, mode: oc_theme::types::Mode) {
        let mut theme = self.theme.get();
        theme.switch_mode(mode);
        self.theme.set(theme);
    }

    pub fn set_connection(&self, state: ConnectionState) {
        self.connection.set(state);
    }

    // -- event ingest ----------------------------------------------------------

    /// Apply a drained batch of data-layer messages **in order**. This is
    /// the single consumer entry point; live events and resync snapshots
    /// are serialized through one channel (see [`crate::data`]) and must
    /// be fully drained before this call so a snapshot can never overtake
    /// queued live events (drain-before-reconcile).
    pub fn ingest_all(&self, batch: impl IntoIterator<Item = Ingest>) {
        for message in batch {
            self.ingest_one(message);
        }
    }

    fn ingest_one(&self, message: Ingest) {
        match message {
            Ingest::Connected => self.set_connection(ConnectionState::Connected),
            Ingest::Connection(state) => self.set_connection(state),
            Ingest::Event(event) => self.reduce_event(&event),
            Ingest::Resync { sessions, unread } => {
                // Reconcile AFTER the channel drained: refresh session
                // titles in place (tab order preserved), keep unread
                // marks the offline events re-established, and add the
                // ones the resync reports. `unread` is already filtered
                // by the data layer (`time.idle > time.viewed`, known
                // timestamps only — no false unread on reconnect; see
                // `crate::data::resync_unread`).
                self.tabs.update(|state| {
                    for session in &sessions {
                        state.tabs = model::open_session_tab(
                            std::mem::take(&mut state.tabs),
                            SessionTab {
                                session_id: session.id.clone(),
                                title: session.title.clone(),
                            },
                        );
                    }
                    for id in unread {
                        state.unread.entry(id).or_insert(Unread::Activity);
                    }
                });
            }
        }
    }

    #[allow(clippy::too_many_lines)] // one match over the event alphabet
    fn reduce_event(&self, event: &BusEvent) {
        match event.type_.as_str() {
            "session.updated" | "session.renamed" => {
                let Some(id) = event.string("sessionID") else { return };
                let title = event.string("title");
                self.tabs.update(|state| {
                    if state.tabs.iter().any(|tab| tab.session_id == id) {
                        state.tabs = model::open_session_tab(
                            std::mem::take(&mut state.tabs),
                            SessionTab { session_id: id.clone(), title },
                        );
                    }
                });
            }
            "session.deleted" => {
                let Some(id) = event.string("sessionID") else { return };
                if self.tabs.get().tabs.iter().any(|tab| tab.session_id == id) {
                    self.close_tab(Some(&id));
                }
            }
            "permission.asked" => {
                if let Ok(request) = serde_json::from_value::<
                    crate::components::footer::PermissionRequest,
                >(event.data.clone())
                {
                    self.footer.update(|footer| footer.permission_asked(request));
                }
            }
            "permission.replied" => {
                if let Some(id) = event.string("requestID") {
                    self.footer.update(|footer| footer.permission_replied(&id));
                }
            }
            "form.created" => {
                // The form rides `data.form` in the payload.
                let form = event.data.get("form").cloned().or_else(|| Some(event.data.clone()));
                if let Some(form) = form {
                    if let Ok(request) =
                        serde_json::from_value::<crate::components::footer::FormRequest>(form)
                    {
                        self.footer.update(|footer| footer.form_created(request));
                    }
                }
            }
            "form.replied" | "form.cancelled" => {
                if let Some(id) = event.string("id") {
                    self.footer.update(|footer| footer.form_removed(&id));
                }
            }
            "tui.toast.show" => {
                let title =
                    event.data.get("title").and_then(serde_json::Value::as_str).map(str::to_owned);
                let Some(message) = event
                    .data
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
                else {
                    return;
                };
                let variant =
                    event.data.get("variant").and_then(serde_json::Value::as_str).unwrap_or("info");
                let variant = match variant {
                    "success" => "success",
                    "warning" => "warning",
                    "error" => "error",
                    _ => "info",
                };
                let duration = event
                    .data
                    .get("duration")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(crate::components::toast::TOAST_DEFAULT_DURATION_MS);
                self.toast_show(ToastOptions {
                    title,
                    message,
                    variant,
                    duration_ms: duration,
                    action_label: None,
                });
            }
            "message.updated" | "message.part.updated" => {
                // A message arriving in a non-active session marks its
                // tab unread; the active session is watched live.
                let Some(id) = event.string("sessionID") else { return };
                let active = self.tabs.get().active(&self.route.get());
                if active.as_deref() != Some(id.as_str()) {
                    self.set_unread(&id, Some(Unread::Activity));
                }
                // Lane C: a full message payload projects into the tree
                // (deltas below patch it part-wise).
                if let Some(message) = message_from_event(event) {
                    self.project_message(&id, message);
                }
            }
            // -- lane C: SSE message-part reducers (`rows.ts` subscriptions) --
            "session.text.delta" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                if event.string("delta").is_none_or(|delta| delta.trim().is_empty()) {
                    return;
                }
                let ordinal = event.ordinal();
                let delta = event.string("delta").unwrap_or_default();
                self.update_messages(&session, |messages| {
                    with_part(messages, &assistant, "text", ordinal, |part| {
                        if let Part::Text { text, .. } = part {
                            text.push_str(&delta);
                        }
                    });
                });
            }
            "session.text.ended" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let Some(text) = event.string("text").filter(|text| !text.trim().is_empty()) else {
                    return;
                };
                let ordinal = event.ordinal();
                self.update_messages(&session, |messages| {
                    with_part(messages, &assistant, "text", ordinal, |part| {
                        if let Part::Text { text: slot, .. } = part {
                            *slot = text;
                        }
                    });
                });
            }
            "session.reasoning.delta" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                if event.string("delta").is_none_or(|delta| delta.trim().is_empty()) {
                    return;
                }
                let ordinal = event.ordinal();
                let delta = event.string("delta").unwrap_or_default();
                self.update_messages(&session, |messages| {
                    with_part(messages, &assistant, "reasoning", ordinal, |part| {
                        if let Part::Reasoning { text, .. } = part {
                            text.push_str(&delta);
                        }
                    });
                });
            }
            "session.reasoning.ended" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                if event.string("text").is_none_or(|text| text.trim().is_empty()) {
                    return;
                }
                let ordinal = event.ordinal();
                let text = event.string("text").unwrap_or_default();
                let completed = event.created;
                self.update_messages(&session, |messages| {
                    with_part(messages, &assistant, "reasoning", ordinal, |part| {
                        if let Part::Reasoning { text: slot, time } = part {
                            *slot = text;
                            time.completed = completed;
                        }
                    });
                });
            }
            "session.tool.input.started" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let Some(part_id) = event.string("id") else { return };
                let name = event.string("name").unwrap_or_default();
                let created = event.created;
                self.update_messages(&session, |messages| {
                    with_tool_part(messages, &assistant, &part_id, |part| {
                        if let Part::Tool { name: slot, time, .. } = part {
                            *slot = name;
                            *time = PartTime { created, completed: None };
                        }
                    });
                });
            }
            "session.step.started" => {
                // A new step reopens the assistant message (retry
                // footers are wave 3); completion state clears. A
                // message that does not exist yet is created OPEN with
                // the step's agent/model and the envelope's `created`
                // (the reference's session reducer — the
                // vscroll-stick-bottom golden streams exactly this:
                // step started → text ended → step ended).
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let agent = event.string("agent").unwrap_or_else(|| "build".to_owned());
                let model = event
                    .data
                    .pointer("/model")
                    .map(|model| match (model.get("providerID"), model.get("id")) {
                        (Some(provider), Some(id)) if provider.is_string() && id.is_string() => {
                            format!(
                                "{}/{}",
                                provider.as_str().unwrap_or_default(),
                                id.as_str().unwrap_or_default()
                            )
                        }
                        _ => String::new(),
                    })
                    .unwrap_or_default();
                let created = event.created;
                self.update_messages(&session, |messages| {
                    match messages.iter_mut().find(|m| m.id() == assistant.as_str()) {
                        Some(Message::Assistant { finish, time, .. }) => {
                            *finish = None;
                            time.completed = None;
                        }
                        Some(_) => {}
                        None => {
                            messages.push(Message::Assistant {
                                id: assistant.clone(),
                                agent,
                                model,
                                parts: Vec::new(),
                                finish: None,
                                time: PartTime { created, completed: None },
                            });
                        }
                    }
                });
            }
            "session.step.ended" | "session.step.failed" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let finish = match event.type_.as_str() {
                    "session.step.failed" => Some("error".to_owned()),
                    _ => event.string("finish").filter(|finish| !finish.is_empty()),
                };
                let completed = event.created;
                self.update_messages(&session, |messages| {
                    if let Some(Message::Assistant { finish: slot, time, .. }) =
                        messages.iter_mut().find(|m| m.id() == assistant.as_str())
                    {
                        *slot = finish;
                        time.completed = time.completed.or(completed);
                    }
                });
            }
            "session.idle" => {
                let Some(session) = event.string("sessionID") else { return };
                let id = event
                    .string("id")
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| format!("msg_{}", event.id.trim_start_matches("evt_")));
                let created = event.created.unwrap_or(0);
                self.update_messages(&session, |messages| {
                    if !messages.iter().any(|m| m.id() == id.as_str()) {
                        messages.push(Message::Idle { id, created });
                    }
                });
            }
            "session.compaction.started" => {
                let Some(session) = event.string("sessionID") else { return };
                let id = event
                    .string("inputID")
                    .or_else(|| event.string("id"))
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| format!("msg_{}", event.id.trim_start_matches("evt_")));
                let created = event.created.unwrap_or(0);
                self.update_messages(&session, |messages| {
                    if !messages.iter().any(|m| m.id() == id.as_str()) {
                        messages.push(Message::Compaction { id, running: true, created });
                    }
                });
            }
            _ => {}
        }
    }

    /// Upsert a fully-formed message from a `message.updated` payload
    /// (idempotent by id, order-preserving).
    fn project_message(&self, session_id: &str, message: Message) {
        self.update_messages(session_id, |messages| {
            match messages.iter().position(|existing| existing.id() == message.id()) {
                Some(index) => messages[index] = message,
                None => messages.push(message),
            }
        });
    }
}

// -- lane C message-tree helpers -----------------------------------------------

/// `(sessionID, assistantMessageID)` off a part event.
fn session_and_assistant(event: &BusEvent) -> Option<(String, String)> {
    Some((event.string("sessionID")?, event.string("assistantMessageID")?))
}

/// Find or create the `kind:ordinal` text/reasoning part on the
/// assistant message and mutate it. Parts keep **arrival order** (a
/// missing part is appended; an existing one merges in place — deltas
/// accumulate, `ended` replaces); a missing message is created open.
fn with_part(
    messages: &mut Vec<Message>,
    assistant: &str,
    kind: &str,
    ordinal: u64,
    mutate: impl FnOnce(&mut Part),
) {
    let index = if let Some(index) = messages.iter().position(|message| message.id() == assistant) {
        index
    } else {
        messages.push(Message::Assistant {
            id: assistant.to_owned(),
            agent: "build".to_owned(),
            model: String::new(),
            parts: Vec::new(),
            finish: None,
            time: PartTime::default(),
        });
        messages.len() - 1
    };
    let Some(Message::Assistant { parts, .. }) = messages.get_mut(index) else {
        return;
    };
    let mut seen = 0u64;
    let target = parts
        .iter()
        .position(|part| {
            if part.kind() == kind {
                if seen == ordinal {
                    return true;
                }
                seen += 1;
            }
            false
        })
        .or_else(|| {
            // Not present yet: append only when this is the next ordinal.
            (seen == ordinal).then(|| {
                parts.push(match kind {
                    "text" => Part::Text { text: String::new(), time: PartTime::default() },
                    _ => Part::Reasoning { text: String::new(), time: PartTime::default() },
                });
                parts.len() - 1
            })
        });
    if let Some(position) = target {
        mutate(&mut parts[position]);
    }
}

/// Find or create a tool part by id on the assistant message.
fn with_tool_part(
    messages: &mut Vec<Message>,
    assistant: &str,
    part_id: &str,
    mutate: impl FnOnce(&mut Part),
) {
    if !messages.iter().any(|message| message.id() == assistant) {
        messages.push(Message::Assistant {
            id: assistant.to_owned(),
            agent: "build".to_owned(),
            model: String::new(),
            parts: Vec::new(),
            finish: None,
            time: PartTime::default(),
        });
    }
    let Some(Message::Assistant { parts, .. }) =
        messages.iter_mut().find(|message| message.id() == assistant)
    else {
        return;
    };
    let target = parts
        .iter()
        .position(|part| matches!(part, Part::Tool { id, .. } if id == part_id))
        .unwrap_or_else(|| {
            parts.push(Part::Tool {
                id: part_id.to_owned(),
                name: String::new(),
                time: PartTime::default(),
            });
            parts.len() - 1
        });
    mutate(&mut parts[target]);
}

/// Parse a full message payload off `message.updated` (`SessionMessageInfo`
/// subset).
fn message_from_event(event: &BusEvent) -> Option<Message> {
    let id = event.string("id").or_else(|| event.string("messageID"))?;
    let time = PartTime {
        created: event.data.pointer("/time/created").and_then(Value::as_u64),
        completed: event.data.pointer("/time/completed").and_then(Value::as_u64),
    };
    match event.string("type").as_deref() {
        Some("user") => Some(Message::User {
            id,
            text: event.string("text").unwrap_or_default(),
            created: time.created.unwrap_or(0),
        }),
        Some("assistant") => {
            let mut parts = Vec::new();
            let mut ordinals: HashMap<&str, u64> = HashMap::new();
            for part in event.data.get("content").and_then(Value::as_array)? {
                let kind = part.get("type").and_then(Value::as_str)?;
                let part_time = PartTime {
                    created: part.pointer("/time/created").and_then(Value::as_u64),
                    completed: part.pointer("/time/completed").and_then(Value::as_u64),
                };
                match kind {
                    "text" | "reasoning" => {
                        let ordinal = ordinals.entry(kind).or_default();
                        let text = part.get("text").and_then(Value::as_str).unwrap_or_default();
                        if kind == "text" {
                            parts.push(Part::Text { text: text.to_owned(), time: part_time });
                        } else {
                            parts.push(Part::Reasoning { text: text.to_owned(), time: part_time });
                        }
                        *ordinal += 1;
                    }
                    "tool" => parts.push(Part::Tool {
                        id: part.get("id").and_then(Value::as_str)?.to_owned(),
                        name: part
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        time: part_time,
                    }),
                    _ => {}
                }
            }
            let model = event
                .data
                .pointer("/model")
                .map(|model| match (model.get("providerID"), model.get("id")) {
                    (Some(provider), Some(id)) if provider.is_string() && id.is_string() => {
                        format!(
                            "{}/{}",
                            provider.as_str().unwrap_or_default(),
                            id.as_str().unwrap_or_default()
                        )
                    }
                    _ => String::new(),
                })
                .unwrap_or_default();
            Some(Message::Assistant {
                id,
                agent: event.string("agent").unwrap_or_else(|| "build".to_owned()),
                model,
                parts,
                finish: event.string("finish"),
                time,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::SessionSnapshot;

    fn tab(id: &str, title: Option<&str>) -> SessionTab {
        SessionTab { session_id: id.to_owned(), title: title.map(str::to_owned) }
    }

    fn store_with_tabs(ids: &[&str]) -> Store {
        let store = Store::new(Runtime::new());
        for id in ids {
            store.open_tab(id, None);
        }
        store
    }

    #[test]
    fn open_tab_appends_and_updates_title() {
        let mut tabs = vec![tab("a", Some("A"))];
        tabs = model::open_session_tab(tabs, tab("a", Some("A2")));
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title.as_deref(), Some("A2"));
        tabs = model::open_session_tab(tabs, tab("b", Some("B")));
        assert_eq!(tabs.len(), 2);
    }

    #[test]
    fn close_proposes_neighbor_and_records_stack() {
        let store = store_with_tabs(&["a", "b", "c"]);
        store.select_tab("b");
        store.close_tab(Some("b"));
        let tabs = store.tabs();
        assert_eq!(
            tabs.tabs.iter().map(|t| t.session_id.clone()).collect::<Vec<_>>(),
            vec!["a", "c"]
        );
        assert_eq!(store.route(), Route::Session { session_id: "c".to_owned() }, "right neighbor");
        assert_eq!(tabs.closed.len(), 1);
        assert_eq!(tabs.closed[0].tab.session_id, "b");
        assert_eq!(tabs.closed[0].index, 1);
        store.close_tab(Some("c"));
        assert_eq!(store.route(), Route::Session { session_id: "a".to_owned() }, "left fallback");
        store.close_tab(Some("a"));
        assert_eq!(store.route(), Route::Home);
    }

    #[test]
    fn reopen_restores_position_and_walks_stack() {
        let store = store_with_tabs(&["a", "b", "c"]);
        store.close_tab(Some("b"));
        store.close_tab(Some("c"));
        assert_eq!(store.tabs().closed.len(), 2);
        assert_eq!(store.reopen_tab(None).as_deref(), Some("c"));
        let ids: Vec<String> = store.tabs().tabs.iter().map(|t| t.session_id.clone()).collect();
        assert_eq!(ids, vec!["a", "c"], "restored at its original position");
        // Reopening an already-open session with no stack entry is a
        // no-op; the plain reopen then walks to the next entry.
        assert_eq!(store.reopen_tab(Some("c")), None, "no c entry left on the stack");
        assert_eq!(store.reopen_tab(None).as_deref(), Some("b"));
        let ids: Vec<String> = store.tabs().tabs.iter().map(|t| t.session_id.clone()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "b restored at its recorded position");
    }

    #[test]
    fn cycle_wraps_and_reads_unread_filter() {
        let store = store_with_tabs(&["a", "b", "c"]);
        store.select_tab("a");
        assert_eq!(store.cycle_tab(1, false).as_deref(), Some("b"));
        assert_eq!(store.cycle_tab(1, false).as_deref(), Some("c"));
        assert_eq!(store.cycle_tab(1, false).as_deref(), Some("a"), "wraps");
        assert_eq!(store.cycle_tab(-1, false).as_deref(), Some("c"));
        store.set_unread("b", Some(Unread::Activity));
        store.select_tab("a");
        assert_eq!(store.cycle_tab(1, true).as_deref(), Some("b"));
        assert_eq!(store.cycle_tab(1, true), None, "no further unread tabs");
    }

    #[test]
    fn cycling_without_active_tab_starts_at_the_edges() {
        let store = store_with_tabs(&["a", "b"]);
        // Home route: no active tab; forward starts at the first tab.
        assert_eq!(store.cycle_tab(1, false).as_deref(), Some("a"));
        store.navigate(Route::Home);
        assert_eq!(store.cycle_tab(-1, false).as_deref(), Some("b"), "backward from the end");
    }

    #[test]
    fn quick_slots_address_positions() {
        let store = store_with_tabs(&["a", "b", "c"]);
        assert_eq!(store.select_quick_slot(1).as_deref(), Some("a"));
        assert_eq!(store.select_quick_slot(3).as_deref(), Some("c"));
        assert_eq!(store.select_quick_slot(9), None);
        assert_eq!(store.select_quick_slot(0), None, "slot 0 = 10th tab, absent");
    }

    #[test]
    fn model_pure_functions_match_reference_semantics() {
        let tabs = vec![tab("a", None), tab("b", None), tab("c", None)];
        let moved = model::move_session_tab(tabs.clone(), "c", 99);
        assert_eq!(moved.last().unwrap().session_id, "c");
        let moved = model::move_session_tab(tabs, "a", 1);
        assert_eq!(
            moved.iter().map(|t| t.session_id.clone()).collect::<Vec<_>>(),
            vec!["b", "a", "c"]
        );
        let history = TabHistory { entries: vec!["a".into(), "b".into()], index: 0 };
        let next = model::record_tab_history(&history, "c");
        assert_eq!(next.entries, vec!["a", "c"]);
        assert_eq!(next.index, 1);
        let same = model::record_tab_history(&next, "c");
        assert_eq!(same, next, "recording the current entry is a no-op");
        // The closed stack is bounded at 25 with the oldest dropped.
        let mut stack = Vec::new();
        for i in 0..30 {
            stack = model::record_closed_tab(stack, tab(&format!("s{i}"), None), 0);
        }
        assert_eq!(stack.len(), CLOSED_TAB_LIMIT);
        assert_eq!(stack.first().unwrap().tab.session_id, "s5");
    }

    #[test]
    fn unread_marks_clear_on_view() {
        let store = store_with_tabs(&["a", "b"]);
        store.set_unread("b", Some(Unread::Error));
        assert_eq!(store.tabs().unread.get("b"), Some(&Unread::Error));
        store.select_tab("b");
        assert!(!store.tabs().unread.contains_key("b"), "viewing clears unread");
    }

    #[test]
    fn pin_toggles() {
        let store = store_with_tabs(&["a"]);
        store.toggle_pin("a");
        assert_eq!(store.tabs().pinned, vec!["a".to_owned()]);
        store.toggle_pin("a");
        assert_eq!(store.tabs().pinned, Vec::<String>::new());
    }

    #[test]
    fn events_mark_unread_and_update_titles() {
        let store = store_with_tabs(&["a", "b"]);
        store.select_tab("a");
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({ "sessionID": "b", "messageID": "m1" }),
        ))]);
        assert_eq!(store.tabs().unread.get("b"), Some(&Unread::Activity));
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({ "sessionID": "a", "messageID": "m2" }),
        ))]);
        assert!(!store.tabs().unread.contains_key("a"), "active session stays clean");
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.renamed",
            serde_json::json!({ "sessionID": "b", "title": "Renamed" }),
        ))]);
        assert_eq!(
            store.tabs().tabs.iter().find(|t| t.session_id == "b").unwrap().title.as_deref(),
            Some("Renamed")
        );
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.deleted",
            serde_json::json!({ "sessionID": "b" }),
        ))]);
        assert!(!store.tabs().tabs.iter().any(|t| t.session_id == "b"));
    }

    #[test]
    fn resync_reconciles_after_drain() {
        let store = store_with_tabs(&["a"]);
        store.ingest_all([
            Ingest::Connected,
            Ingest::Event(BusEvent::new(
                "message.updated",
                serde_json::json!({ "sessionID": "a", "messageID": "m" }),
            )),
            Ingest::Resync {
                sessions: vec![SessionSnapshot {
                    id: "a".to_owned(),
                    title: Some("Fresh title".to_owned()),
                    time_idle: None,
                    viewed: None,
                }],
                unread: vec!["a".to_owned()],
            },
        ]);
        let tabs = store.tabs();
        assert_eq!(tabs.tabs[0].title.as_deref(), Some("Fresh title"));
        assert_eq!(
            tabs.unread.get("a"),
            Some(&Unread::Activity),
            "unread from the offline event survives the snapshot"
        );
        assert_eq!(store.connection(), ConnectionState::Connected);
    }

    #[test]
    fn mutations_coalesce_into_one_redraw_bump_per_pump() {
        let store = store_with_tabs(&["a"]);
        store.runtime.run_effects();
        let before = store.redraw_generation();
        store.set_unread("a", Some(Unread::Activity));
        store.set_unread("a", None);
        assert_eq!(store.redraw_generation(), before, "effects wait for the pump");
        store.runtime.run_effects();
        let after = store.redraw_generation();
        assert!(after > before, "coalesced redraw requested exactly once");
        store.runtime.run_effects();
        assert_eq!(store.redraw_generation(), after, "no change, no redraw");
    }

    #[test]
    fn dialogs_form_a_typed_stack_with_focus_routing() {
        let store = Store::new(Runtime::new());
        assert_eq!(store.focus(), Focus::Route);
        store.open_dialog(
            DialogKind::ServerList { command: "model.list", title: "Switch model" },
            DialogSize::Medium,
        );
        store.open_dialog(DialogKind::ThemeList, DialogSize::Large);
        let stack = store.dialogs();
        assert_eq!(stack.len(), 2);
        assert_eq!(stack[1].kind, DialogKind::ThemeList);
        assert_eq!(stack[1].size, DialogSize::Large, "chrome size rides the entry");
        assert_eq!(store.focus(), Focus::Dialog, "the top dialog takes focus");
        // Replace collapses the stack to one entry (dialog.replace).
        store.replace_dialog(DialogKind::CommandPalette, DialogSize::Medium);
        assert_eq!(store.dialogs().len(), 1);
        assert_eq!(store.top_dialog().unwrap().kind, DialogKind::CommandPalette);
        store.close_dialog();
        assert_eq!(store.dialogs().len(), 0);
        assert_eq!(store.focus(), Focus::Route, "focus returns to the route");
        // update_dialog mutates only the top entry.
        store.open_dialog(DialogKind::ThemeList, DialogSize::Medium);
        store.update_dialog(|entry| entry.select.set_filter("gru"));
        assert_eq!(store.top_dialog().unwrap().select.filter, "gru");
    }

    #[test]
    fn permission_events_drive_the_footer_view() {
        let store = Store::new(Runtime::new());
        store.ingest_all([Ingest::Event(BusEvent::new(
            "permission.asked",
            serde_json::json!({
                "id": "p1",
                "sessionID": "ses",
                "action": "read",
                "resources": ["src/index.ts"],
            }),
        ))]);
        let footer = store.footer();
        assert!(matches!(
            footer.view,
            crate::components::footer::FooterView::Permission { ref request } if request.id == "p1"
        ));
        store.ingest_all([Ingest::Event(BusEvent::new(
            "permission.replied",
            serde_json::json!({ "requestID": "p1", "decision": "once" }),
        ))]);
        assert_eq!(store.footer().view, crate::components::footer::FooterView::Prompt);
    }

    #[test]
    fn form_events_queue_behind_permission_asks() {
        let store = Store::new(Runtime::new());
        store.ingest_all([
            Ingest::Event(BusEvent::new(
                "permission.asked",
                serde_json::json!({ "id": "p1", "sessionID": "ses", "action": "edit", "resources": [] }),
            )),
            Ingest::Event(BusEvent::new(
                "form.created",
                serde_json::json!({ "form": {
                    "id": "f1",
                    "sessionID": "ses",
                    "title": "Deployment",
                    "fields": [{ "key": "k", "type": "string", "title": "K" }],
                }}),
            )),
        ]);
        assert!(
            matches!(store.footer().view, crate::components::footer::FooterView::Permission { .. }),
            "permission preempts the form"
        );
        store.ingest_all([Ingest::Event(BusEvent::new(
            "permission.replied",
            serde_json::json!({ "requestID": "p1" }),
        ))]);
        assert!(matches!(
            store.footer().view,
            crate::components::footer::FooterView::Form { ref request } if request.id == "f1"
        ));
        store.ingest_all([Ingest::Event(BusEvent::new(
            "form.cancelled",
            serde_json::json!({ "id": "f1" }),
        ))]);
        assert_eq!(store.footer().view, crate::components::footer::FooterView::Prompt);
    }

    #[test]
    fn toast_events_show_and_expire_through_the_clock() {
        let store = Store::new(Runtime::new());
        store.ingest_all([Ingest::Event(BusEvent::new(
            "tui.toast.show",
            serde_json::json!({
                "title": "Fixture notice",
                "message": "Toast rendered over session content",
                "variant": "info",
                "duration": 60000,
            }),
        ))]);
        let toasts = store.toasts();
        assert_eq!(
            toasts.current().map(|toast| toast.message.as_str()),
            Some("Toast rendered over session content")
        );
        assert_eq!(toasts.current().unwrap().duration_ms, 60000);
        // While live and unpaused a new show replaces the surface; a
        // paused one stacks the queue (`ToastContext.show`).
        store.toast_advance(0);
        store.toast_set_paused(true, 10);
        store.toast_show(crate::components::toast::ToastOptions::info("second"));
        assert_eq!(store.toasts().pending(), 1);
        // Paused toasts never expire; resuming arms the remaining time.
        store.toast_advance(1_000_000);
        assert_eq!(
            store.toasts().current().map(|toast| toast.message.as_str()),
            Some("Toast rendered over session content"),
            "paused toasts never expire"
        );
        store.toast_set_paused(false, 1_000_000);
        store.toast_advance(1_059_989);
        assert!(store.toasts().current().is_some(), "just under the remaining 59 990 ms");
        store.toast_advance(1_059_990);
        let toasts = store.toasts();
        assert_eq!(
            toasts.current().map(|toast| toast.message.as_str()),
            Some("second"),
            "rotated in"
        );
        // The rotated toast runs its own (default 5 s) countdown,
        // armed on the next tick after the rotation.
        store.toast_advance(1_059_991);
        store.toast_advance(1_064_990);
        assert!(store.toasts().current().is_some(), "inside the 5 s default duration");
        store.toast_advance(1_064_991);
        assert!(store.toasts().current().is_none(), "expired and the queue drained");
    }

    #[test]
    fn footer_routes_and_redraws_coalesce() {
        let store = Store::new(Runtime::new());
        store.runtime.run_effects();
        let before = store.redraw_generation();
        store.open_footer_route(crate::components::footer::PromptRoute::Model);
        store.close_footer_panel();
        assert_eq!(store.redraw_generation(), before);
        store.runtime.run_effects();
        assert!(store.redraw_generation() > before, "footer changes request a redraw");
    }

    // -- lane C: message-tree reducers + injected clock -----------------------

    #[test]
    fn text_deltas_merge_in_arrival_order() {
        let store = Store::new(Runtime::new());
        for delta in ["Three failure ", "modes dominate: recursion limits, ", "cycle detection."] {
            store.ingest_all([Ingest::Event(BusEvent::new(
                "session.text.delta",
                serde_json::json!({
                    "sessionID": "s", "assistantMessageID": "a", "delta": delta
                }),
            ))]);
        }
        let messages = store.messages("s");
        let Message::Assistant { parts, .. } = &messages[0] else {
            panic!("assistant message created by the delta");
        };
        assert_eq!(
            parts,
            &[Part::Text {
                text: "Three failure modes dominate: recursion limits, cycle detection.".into(),
                time: PartTime::default()
            }]
        );
        // text.ended replaces the accumulated text with the final one.
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.text.ended",
            serde_json::json!({
                "sessionID": "s", "assistantMessageID": "a", "ordinal": 0,
                "text": "Final text."
            }),
        ))]);
        assert_eq!(store.messages("s")[0].parts()[0].text(), "Final text.");
        // A second text ordinal appends a NEW part (steered answers).
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.text.ended",
            serde_json::json!({
                "sessionID": "s", "assistantMessageID": "a", "ordinal": 1,
                "text": "Second answer."
            }),
        ))]);
        assert_eq!(store.messages("s")[0].parts().len(), 2);
    }

    #[test]
    fn blank_deltas_do_not_create_parts() {
        let store = Store::new(Runtime::new());
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.text.delta",
            serde_json::json!({ "sessionID": "s", "assistantMessageID": "a", "delta": "  " }),
        ))]);
        assert!(store.messages("s").is_empty(), "no message from whitespace deltas");
    }

    #[test]
    fn reasoning_tools_and_steps_build_the_tree() {
        let store = Store::new(Runtime::new());
        let mut event = BusEvent::new(
            "session.reasoning.delta",
            serde_json::json!({ "sessionID": "s", "assistantMessageID": "a", "delta": "**Title**" }),
        );
        event.created = Some(11);
        store.ingest_all([Ingest::Event(event)]);
        let mut event = BusEvent::new(
            "session.tool.input.started",
            serde_json::json!({ "sessionID": "s", "assistantMessageID": "a", "id": "t1", "name": "read" }),
        );
        event.created = Some(12);
        store.ingest_all([Ingest::Event(event)]);
        // Parts stay in arrival order: reasoning then tool.
        let Message::Assistant { parts, finish, .. } = &store.messages("s")[0] else { panic!() };
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].kind(), "reasoning");
        assert_eq!(parts[1].kind(), "tool");
        assert_eq!(finish, &None);
        // reasoning.ended stamps completion; step.ended finalizes.
        let mut event = BusEvent::new(
            "session.reasoning.ended",
            serde_json::json!({
                "sessionID": "s", "assistantMessageID": "a",
                "text": "**Title**\n\nBody"
            }),
        );
        event.created = Some(13);
        store.ingest_all([Ingest::Event(event)]);
        let mut event = BusEvent::new(
            "session.step.ended",
            serde_json::json!({
                "sessionID": "s", "assistantMessageID": "a", "finish": "stop"
            }),
        );
        event.created = Some(14);
        store.ingest_all([Ingest::Event(event)]);
        let Message::Assistant { parts, finish, time, .. } = &store.messages("s")[0] else {
            panic!()
        };
        let Part::Reasoning { time: reasoning_time, .. } = &parts[0] else { panic!() };
        assert_eq!(reasoning_time.completed, Some(13));
        assert_eq!(finish.as_deref(), Some("stop"));
        assert_eq!(time.completed, Some(14));
        // A reopened step clears the terminal state.
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.step.started",
            serde_json::json!({ "sessionID": "s", "assistantMessageID": "a" }),
        ))]);
        let Message::Assistant { finish, time, .. } = &store.messages("s")[0] else { panic!() };
        assert_eq!(finish, &None);
        assert_eq!(time.completed, None);
    }

    #[test]
    fn idle_and_compaction_markers_append_once() {
        let store = Store::new(Runtime::new());
        for _ in 0..2 {
            let mut event = BusEvent::new("session.idle", serde_json::json!({ "sessionID": "s" }));
            event.id = "evt_i1".into();
            event.created = Some(20);
            store.ingest_all([Ingest::Event(event)]);
        }
        let mut event = BusEvent::new(
            "session.compaction.started",
            serde_json::json!({ "sessionID": "s", "inputID": "c1" }),
        );
        event.created = Some(21);
        store.ingest_all([Ingest::Event(event)]);
        let messages = store.messages("s");
        assert_eq!(messages.len(), 2, "idle deduped by id");
        assert!(matches!(&messages[0], Message::Idle { created: 20, .. }));
        assert!(matches!(&messages[1], Message::Compaction { running: true, .. }));
    }

    #[test]
    fn message_updated_projects_full_payloads() {
        let store = Store::new(Runtime::new());
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({
                "sessionID": "s",
                "id": "m1",
                "type": "user",
                "text": "What does the parser do?",
                "time": { "created": 0 }
            }),
        ))]);
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({
                "sessionID": "s",
                "id": "m2",
                "type": "assistant",
                "agent": "build",
                "model": { "providerID": "fixture", "id": "fixture" },
                "finish": "stop",
                "time": { "created": 1, "completed": 2 },
                "content": [
                    { "type": "tool", "id": "t1", "name": "read", "time": { "created": 1, "completed": 1 } },
                    { "type": "text", "text": "It reads the configuration." }
                ]
            }),
        ))]);
        let messages = store.messages("s");
        assert_eq!(messages.len(), 2);
        assert!(
            matches!(&messages[0], Message::User { text, .. } if text == "What does the parser do?")
        );
        let Message::Assistant { model, parts, finish, .. } = &messages[1] else { panic!() };
        assert_eq!(model, "fixture/fixture");
        assert_eq!(parts.len(), 2);
        assert_eq!(finish.as_deref(), Some("stop"));
        // Re-delivery is idempotent.
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({ "sessionID": "s", "id": "m2", "type": "assistant", "content": [] }),
        ))]);
        assert_eq!(store.messages("s").len(), 2);
    }

    #[test]
    fn clock_is_monotonic_and_renders_only_on_advance() {
        let store = Store::new(Runtime::new());
        assert_eq!(store.clock(), 0);
        store.runtime.run_effects();
        let before = store.redraw_generation();
        store.advance_clock(0);
        store.advance_clock(0);
        store.runtime.run_effects();
        assert_eq!(store.redraw_generation(), before, "a frozen clock never redraws");
        store.advance_clock(80);
        store.runtime.run_effects();
        assert!(store.redraw_generation() > before, "the tick redraws");
        store.advance_clock(40);
        assert_eq!(store.clock(), 80, "monotonic — never rewinds");
    }

    /// The stream fixture of the vscroll-stick-bottom golden end-to-end:
    /// step started → text started → text ended → step ended builds the
    /// appended assistant turn exactly as the fixture messages look
    /// (agent + model from the step envelope, completion from
    /// step.ended's envelope).
    #[test]
    fn streamed_turn_matches_the_fixture_shape() {
        let store = Store::new(Runtime::new());
        let mut started = BusEvent::new(
            "session.step.started",
            serde_json::json!({
                "sessionID": "s",
                "assistantMessageID": "vs-answer-appended",
                "agent": "build",
                "model": { "providerID": "fixture", "id": "fixture" },
            }),
        );
        started.created = Some(101);
        store.ingest_all([Ingest::Event(started)]);
        let events = [
            (
                "session.text.started",
                serde_json::json!({ "sessionID": "s", "assistantMessageID": "vs-answer-appended" }),
            ),
            (
                "session.text.ended",
                serde_json::json!({ "sessionID": "s", "assistantMessageID": "vs-answer-appended", "ordinal": 0, "text": "Appended while pinned to the bottom." }),
            ),
        ];
        for (kind, data) in events {
            store.ingest_all([Ingest::Event(BusEvent::new(kind, data))]);
        }
        let mut ended = BusEvent::new(
            "session.step.ended",
            serde_json::json!({ "sessionID": "s", "assistantMessageID": "vs-answer-appended", "finish": "stop" }),
        );
        ended.created = Some(103);
        store.ingest_all([Ingest::Event(ended)]);
        let messages = store.messages("s");
        let Message::Assistant { parts, finish, model, time, .. } = &messages[0] else { panic!() };
        assert_eq!(
            parts,
            &[Part::Text {
                text: "Appended while pinned to the bottom.".into(),
                time: PartTime::default()
            }]
        );
        assert_eq!(finish.as_deref(), Some("stop"));
        assert_eq!(model, "fixture/fixture");
        assert_eq!(time.created, Some(101));
        assert_eq!(time.completed, Some(103));
    }
}
