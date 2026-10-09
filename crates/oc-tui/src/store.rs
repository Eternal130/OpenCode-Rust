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

use crate::data::{BusEvent, ConnectionState, Ingest};
use crate::reactive::{Effect, Runtime, Signal};
use crate::theme::ThemeState;

/// Route (`context/route.tsx`): home or a session view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Session { session_id: String },
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

/// The store root.
pub struct Store {
    /// Reactive runtime owning the graph (pump via `run_effects`).
    pub runtime: Runtime,
    tabs: Signal<TabsState>,
    route: Signal<Route>,
    dialogs: Signal<Vec<String>>,
    theme: Signal<ThemeState>,
    connection: Signal<ConnectionState>,
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
        let dialogs = Signal::new(&runtime, Vec::new());
        let theme = Signal::new(&runtime, ThemeState::v2_dark());
        let connection = Signal::new(&runtime, ConnectionState::Connecting);
        let redraw_requests = Rc::new(Cell::new(0u64));
        // Any root-state change requests exactly one redraw per pump.
        let bump = Rc::clone(&redraw_requests);
        let watch_tabs = tabs.clone();
        let watch_route = route.clone();
        let watch_dialogs = dialogs.clone();
        let watch_theme = theme.clone();
        let watch_connection = connection.clone();
        let redraw_effect = Effect::new(&runtime, move || {
            let _ = (
                watch_tabs.get(),
                watch_route.get(),
                watch_dialogs.get(),
                watch_theme.get(),
                watch_connection.get(),
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
    pub fn dialogs(&self) -> Vec<String> {
        self.dialogs.get()
    }

    #[must_use]
    pub fn theme(&self) -> ThemeState {
        self.theme.get()
    }

    #[must_use]
    pub fn connection(&self) -> ConnectionState {
        self.connection.get()
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

    pub fn push_dialog(&self, dialog: &str) {
        self.dialogs.update(|stack| stack.push(dialog.to_owned()));
    }

    pub fn pop_dialog(&self) {
        self.dialogs.update(|stack| {
            stack.pop();
        });
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
                // ones the resync reports.
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
            "message.updated" | "message.part.updated" => {
                // A message arriving in a non-active session marks its
                // tab unread; the active session is watched live.
                let Some(id) = event.string("sessionID") else { return };
                let active = self.tabs.get().active(&self.route.get());
                if active.as_deref() != Some(id.as_str()) {
                    self.set_unread(&id, Some(Unread::Activity));
                }
                // Known-but-unprojected types (usage, connected, …) pass
                // through; unknown types are future server surface.
            }
            _ => {}
        }
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
    fn dialogs_form_a_stack() {
        let store = Store::new(Runtime::new());
        store.push_dialog("model");
        store.push_dialog("theme");
        assert_eq!(store.dialogs(), vec!["model".to_owned(), "theme".to_owned()]);
        store.pop_dialog();
        assert_eq!(store.dialogs(), vec!["model".to_owned()]);
    }
}
