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
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use serde_json::Value;

use crate::attention::AttentionState;
use crate::clipboard::ClipboardSink;
use crate::components::composer::{
    Composer, PromptDraft, PromptHistory, PromptSink, RecordingPromptSink, SubmittedPrompt,
};
use crate::components::dialog_select::SelectState;
use crate::components::footer::{FooterMachine, PromptRoute};
use crate::components::message::model::{Message, Part, PartTime};
use crate::components::toast::{ToastOptions, ToastQueue};
use crate::data::{BusEvent, ConnectionState, Ingest};
use crate::keymap::KeybindOverride;
use crate::reactive::{Effect, Runtime, Signal};
use crate::theme::{ThemeRegistry, ThemeState};

/// How many bus-event ids the replay-dedup window remembers (the F2
/// contract — see [`Store::ingest_all`] and `data.rs`).
const SEEN_EVENT_WINDOW: usize = 4096;

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
    /// `DialogHelp` (`ui/dialog-help.tsx`): the palette hint + the
    /// keybinding table grouped by mode.
    Help,
    /// Server-backed listings (model/agent/session lists): populated
    /// from the store's server-list state ([`ServerLists`]) — loading,
    /// error, and empty states included.
    ServerList { command: &'static str, title: &'static str },
    /// The Worktrees dialog (`component/dialog-workspaces.tsx`):
    /// `session.move` / `/worktrees` — the project's worktree
    /// directories with move/new/delete/refresh actions, driven by
    /// the store's worktree state ([`WorktreeDialogState`]).
    Worktrees,
    /// `DialogWorktreeName` (`component/dialog-worktree-name.tsx`):
    /// the new-worktree name input (tab generates a slug, enter
    /// confirms the slugified value, esc cancels).
    WorktreeName,
    /// `DialogWorkspaceFileChanges` in its delete-confirm role: the
    /// forced-remove question after a `forceRequired` rejection.
    WorktreeDeleteConfirm { directory: String },
    /// A plugin dialog (`DialogKind::Plugin`, wave 3's open dialog
    /// seam): the content was materialized at OPEN time through
    /// [`PluginDialogSource`] (or arrived fully-built on a
    /// `ui.dialog.open` request) and renders natively — the id keys
    /// the store's [`PluginDialogContent`].
    Plugin { id: String },
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
    /// The strip tab under the pointer (`marquee.hovered`,
    /// `session-tabs.tsx:338-369`) — drives the hover lift, the
    /// `text.base` foreground, and the revealed `✕` close mark.
    pub hover: Option<String>,
    /// The ` + ` affordance under the pointer (`addHovered`,
    /// `session-tabs.tsx:1301`).
    pub add_hovered: bool,
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

// -- server-backed pickers (F3.i) -----------------------------------------

/// One fetchable server list's state (loading → loaded/failed).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ListState<T> {
    /// The fetch is in flight (the picker shows its loading row).
    #[default]
    Loading,
    /// The fetch landed.
    Loaded(Vec<T>),
    /// The fetch failed (the picker shows the error row).
    Failed(String),
}

impl<T> ListState<T> {
    /// The loaded rows, if any.
    #[must_use]
    pub fn rows(&self) -> Option<&[T]> {
        match self {
            Self::Loaded(rows) => Some(rows),
            _ => None,
        }
    }
}

/// One `model.list` row (`ModelInfo` subset the pickers render).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerModel {
    pub provider_id: String,
    pub id: String,
    pub name: String,
    /// `cost[0].input` (USD per M tokens) — the footer panel's Free
    /// logic input.
    pub cost_input: f64,
    /// `cost` non-empty with every input zero (`free()` in
    /// dialog-model) — the dialog's "Free" footer.
    pub free: bool,
    /// `status == "deprecated"` filters the row out (dialog-model).
    pub deprecated: bool,
    /// `ModelInfo.variants[].id` — the variant picker's source.
    pub variants: Vec<String>,
    /// `ModelInfo.time.released` (ms epoch; 0 unknown) — the dialog's
    /// release-date ordering key (D-05).
    pub released: f64,
}

/// One `provider.list` row (`ProviderInfo` subset).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerProvider {
    pub id: String,
    pub name: String,
}

/// One `agent.list` row (`AgentInfo` subset).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerAgent {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// `all` | `primary` | `subagent`.
    pub mode: String,
    pub hidden: bool,
}

/// The model/agent/provider lists the pickers read (`data.location.*`
/// in the reference — fetched once per connect, refreshed on
/// connection loss).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ServerLists {
    pub models: ListState<ServerModel>,
    pub providers: Vec<ServerProvider>,
    pub agents: ListState<ServerAgent>,
}

/// A model selection (`providerID` + `modelID` + optional variant).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelSelection {
    pub provider_id: String,
    pub model_id: String,
    pub variant: Option<String>,
}

/// The per-session model/agent selection the composer commits (the
/// `local` context's draft state in the reference).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelectionState {
    pub by_session: HashMap<String, SessionSelection>,
}

/// One session's pending selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionSelection {
    pub model: Option<ModelSelection>,
    pub agent: Option<String>,
}

/// The server selection seam: applying a model/agent pick POSTs
/// `session.switchModel` / `session.switchAgent` (the reference's
/// commit path — `prompt/index.tsx` `prepareAgent`/`commitModel`, the
/// mini footer's `admitPromptTurn`). Tests record; production calls
/// oc-client.
pub trait SelectionSink {
    fn switch_model(&self, session_id: &str, model: &ModelSelection);
    fn switch_agent(&self, session_id: &str, agent: &str);
}

/// The default recording sink (tests assert against its log).
#[derive(Debug, Default)]
pub struct RecordingSelectionSink {
    pub calls: std::cell::RefCell<Vec<(String, SelectionCall)>>,
}

/// One recorded sink call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionCall {
    Model { provider_id: String, model_id: String, variant: Option<String> },
    Agent(String),
}

impl SelectionSink for RecordingSelectionSink {
    fn switch_model(&self, session_id: &str, model: &ModelSelection) {
        self.calls.borrow_mut().push((
            session_id.to_owned(),
            SelectionCall::Model {
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
                variant: model.variant.clone(),
            },
        ));
    }

    fn switch_agent(&self, session_id: &str, agent: &str) {
        self.calls
            .borrow_mut()
            .push((session_id.to_owned(), SelectionCall::Agent(agent.to_owned())));
    }
}

/// The worktree API seam (v2's `client.api.worktree.*` calls in
/// `dialog-workspaces.tsx` / `prompt/move.tsx`): the dialog's actions
/// run through it; tests record, production POSTs through oc-client
/// (spawned — results land back via the store's worktree-result
/// setters, mirroring the SSE `worktree.updated` refetch).
pub trait WorktreeSink {
    /// `worktree.list` (the dialog's open fetch).
    fn list(&self, project_id: &str);
    /// `worktree.refresh` (open-once + the ctrl+r action).
    fn refresh(&self, project_id: &str);
    /// `worktree.create` (the name dialog's confirm).
    fn create(&self, project_id: &str, name: &str);
    /// `worktree.remove` (two-press delete; `force` after the
    /// file-changes confirm).
    fn remove(&self, project_id: &str, directory: &str, force: bool);
    /// `session.move` (the ctrl+m action).
    fn move_session(&self, session_id: &str, directory: &str);
}

/// One recorded [`WorktreeSink`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeCall {
    List(String),
    Refresh(String),
    Create { project_id: String, name: String },
    Remove { project_id: String, directory: String, force: bool },
    MoveSession { session_id: String, directory: String },
}

/// The test sink: records every call.
#[derive(Debug, Default)]
pub struct RecordingWorktreeSink {
    pub calls: std::cell::RefCell<Vec<WorktreeCall>>,
}

impl WorktreeSink for RecordingWorktreeSink {
    fn list(&self, project_id: &str) {
        self.calls.borrow_mut().push(WorktreeCall::List(project_id.to_owned()));
    }

    fn refresh(&self, project_id: &str) {
        self.calls.borrow_mut().push(WorktreeCall::Refresh(project_id.to_owned()));
    }

    fn create(&self, project_id: &str, name: &str) {
        self.calls.borrow_mut().push(WorktreeCall::Create {
            project_id: project_id.to_owned(),
            name: name.to_owned(),
        });
    }

    fn remove(&self, project_id: &str, directory: &str, force: bool) {
        self.calls.borrow_mut().push(WorktreeCall::Remove {
            project_id: project_id.to_owned(),
            directory: directory.to_owned(),
            force,
        });
    }

    fn move_session(&self, session_id: &str, directory: &str) {
        self.calls.borrow_mut().push(WorktreeCall::MoveSession {
            session_id: session_id.to_owned(),
            directory: directory.to_owned(),
        });
    }
}

/// What one ctrl+d press did in the two-press delete machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteStep {
    /// First press: the row armed ("Press ctrl+d again to confirm").
    Armed,
    /// Second press on the armed row: fire the remove.
    Confirmed,
    /// The row is not deletable (unmanaged root / not the armed row).
    Invalid,
}

/// The store root.
/// The composer's store slice: per-session drafts (the draft stash),
/// the shared prompt history, the per-session autocomplete popup
/// state, and the engine prompt sink.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PromptState {
    /// Drafts keyed by session (the stash `PromptRef.set` restores).
    pub drafts: std::collections::HashMap<String, PromptDraft>,
    /// The live `/`,`@` popup state keyed by session (wave-6: survives
    /// the composer store round-trip between keypresses).
    pub autocomplete:
        std::collections::HashMap<String, crate::components::composer::AutocompleteState>,
    pub history: PromptHistory,
}

/// One plugin-registered palette command (the oc-plugin-host
/// registration snapshot's TUI projection — registration-time only;
/// execution routes through the [`PluginCommandSink`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommandEntry {
    /// Dot command id (`hw.hello`) — the dispatch key.
    pub command: String,
    /// Palette title (the description the plugin registered).
    pub title: String,
    /// Owning plugin id (provenance display).
    pub plugin_id: String,
}

/// One select option of a plugin dialog (the delegation payload's
/// option row — submit dispatches `value` through the palette
/// dispatcher).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDialogOption {
    pub title: String,
    pub value: String,
    pub description: String,
}

/// The content structure a plugin dialog renders natively from (the
/// delegation trait's payload): a title, informational rows, and
/// optional select semantics. Materialized ONCE at open time —
/// synchronously through the host bridge (HARD timeout there), never
/// per-frame.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginDialogContent {
    pub title: String,
    /// Static rows (rendered verbatim when no options).
    pub rows: Vec<String>,
    /// When non-empty the dialog is a select (the native pipeline:
    /// filter/move/submit; submit dispatches the option's value).
    pub options: Vec<PluginDialogOption>,
}

impl PluginDialogContent {
    /// Whether this content carries select semantics.
    #[must_use]
    pub fn is_select(&self) -> bool {
        !self.options.is_empty()
    }
}

/// Open-time dialog content source (the delegation seam behind
/// [`DialogKind::Plugin`]): the app wires this to the plugin host's
/// materialization call (the guest builds the structure
/// synchronously; the host bounds the whole call with a hard
/// timeout). Called once per open — never per frame.
pub trait PluginDialogSource {
    /// The dialog id (`plugin:local` qualified or raw local id).
    fn content(&self, id: &str) -> Option<PluginDialogContent>;
}

/// The no-op source (no plugin host attached).
#[derive(Debug, Default)]
pub struct NoPluginDialogs;

impl PluginDialogSource for NoPluginDialogs {
    fn content(&self, _id: &str) -> Option<PluginDialogContent> {
        None
    }
}

/// Execution sink for plugin commands: the app layer wires this to the
/// plugin host's `execute_command` (timeout-bounded there); the store
/// itself never talks IPC.
pub trait PluginCommandSink {
    /// `true` when the command was known and dispatched.
    fn execute(&self, command: &str, args: Option<&str>) -> bool;
}

/// The no-op sink (no plugin host attached).
#[derive(Debug, Default)]
pub struct NoPluginCommands;

impl PluginCommandSink for NoPluginCommands {
    fn execute(&self, _command: &str, _args: Option<&str>) -> bool {
        false
    }
}

/// The post-ingest event tap (the plugin-host broadcast seam,
/// oracle-gate11 §4 wave 1): after every event the store REDUCED, the
/// store publishes the same deduplicated, ordered stream to the tap —
/// plugins observe exactly what the store observed. Implementations
/// must be non-blocking (a slow plugin may never stall ingest).
pub trait EventTap: Send + Sync {
    fn publish(&self, event: &BusEvent);
}

pub struct Store {
    /// Reactive runtime owning the graph (pump via `run_effects`).
    pub runtime: Runtime,
    tabs: Signal<TabsState>,
    /// `config.tabs.enabled` — the open-tabs model's master gate
    /// (v2's `enabled()` in `context/session-tabs.tsx:68`): tab
    /// mutations become no-ops (navigation still works) and the
    /// strip hides. Defaults ON (mode "auto").
    tabs_enabled: Signal<bool>,
    /// The session HISTORY from the resync's `session.list` — the
    /// session-list dialog's data source (v2 lists history there,
    /// never the tab strip).
    session_history: Signal<Vec<crate::data::SessionSnapshot>>,
    route: Signal<Route>,
    dialogs: Signal<Vec<DialogEntry>>,
    theme: Signal<ThemeState>,
    /// The theme registry (builtins + discovered customs): the theme
    /// dialog's data source; theme selection resolves through it.
    registry: Signal<ThemeRegistry>,
    /// The user keybind overrides (`cli.json` → `keybinds`); a write
    /// here is the config-reload event the run loop re-merges into the
    /// keymap from.
    keybinds: Signal<Vec<KeybindOverride>>,
    connection: Signal<ConnectionState>,
    /// The mini footer state machine (view / route / blocker queues).
    footer: Signal<FooterMachine>,
    /// The toast overlay queue (current surface + pending stack).
    toasts: Signal<ToastQueue>,
    /// Session message trees (lane C message flow).
    messages: Signal<MessageState>,
    /// The composer state (drafts + history) for the session route.
    prompt: Signal<PromptState>,
    /// The server-backed picker lists (model/provider/agent).
    server_lists: Signal<ServerLists>,
    /// The per-session model/agent selection (composer commits).
    selection: Signal<SelectionState>,
    /// The Worktrees dialog state (list + two-press delete).
    worktrees: Signal<crate::components::dialog_worktrees::WorktreeDialogState>,
    /// The worktree API seam (the dialog's actions).
    worktree_sink: std::cell::RefCell<Rc<dyn WorktreeSink>>,
    /// The engine prompt API stub submissions forward to.
    prompt_sink: std::rc::Rc<dyn PromptSink>,
    /// The server selection API (model/agent switch POSTs).
    selection_sink: Rc<dyn SelectionSink>,
    /// The clipboard sink (`messages.copy` writes OSC 52 through it).
    clipboard_sink: Rc<dyn ClipboardSink>,
    /// The external `$EDITOR` request flag (the run loop drains it:
    /// suspend → run → resume → replace the focused draft).
    editor_requested: Cell<bool>,
    /// Attention (bell + pending marker) — see `attention.rs`.
    attention: Cell<AttentionState>,
    /// The attention engine (R-06): OS notifications + sounds behind
    /// the emitter seam — see `attention.rs`.
    attention_engine: std::cell::RefCell<crate::attention::Attention>,
    /// The notifications plugin (v2's `opencode.notifications`
    /// trigger map) — see `notifications_plugin.rs`.
    notifications: std::cell::RefCell<crate::notifications_plugin::NotificationsPlugin>,
    /// The plugin's session index (titles + parentIDs from the session
    /// lifecycle events).
    session_index: std::cell::RefCell<crate::notifications_plugin::SessionIndex>,
    /// Replay-dedup window: the recent bus-event ids (F2 contract).
    seen_events: std::cell::RefCell<(VecDeque<String>, HashSet<String>)>,
    /// Plugin-registered palette commands (registration-time only).
    plugin_commands: Signal<Vec<PluginCommandEntry>>,
    /// Plugin dialog contents keyed by dialog id (materialized at open
    /// or delivered built on a `ui.dialog.open` request).
    plugin_dialogs: Signal<HashMap<String, PluginDialogContent>>,
    /// The open-time dialog content source (delegation seam).
    plugin_dialog_source: std::cell::RefCell<Rc<dyn PluginDialogSource>>,
    /// Plugin sidebar sections (one per plugin, latest wins).
    plugin_sections: Signal<Vec<crate::components::sidebar::SidebarSection>>,
    /// Plugin footer hint segments (one per plugin, latest wins).
    plugin_hints: Signal<Vec<(String, String)>>,
    /// Plugin home quick actions (registration-time).
    plugin_home_actions: Signal<Vec<crate::components::home::PluginQuickAction>>,
    /// Plugin message-grouping rules (registration-time).
    plugin_group_rules: Signal<crate::components::message::model::GroupRules>,
    /// The post-ingest event tap (plugin-host broadcast seam).
    event_tap: std::cell::RefCell<Option<std::sync::Arc<dyn EventTap>>>,
    /// Plugin command execution (bridges the palette to the host).
    plugin_sink: std::cell::RefCell<Rc<dyn PluginCommandSink>>,
    /// The injected clock (ms since boot) — the ONLY time source
    /// animations may read (`ManualClock` semantics: a frozen clock pins
    /// spinner frame 0; render happens on tick, only when it advances).
    clock: Signal<u64>,
    /// Coalesced redraw request counter (bumped by the redraw effect;
    /// the render loop compares between ticks — never redraw inline).
    redraw_requests: Rc<Cell<u64>>,
    _redraw_effect: crate::reactive::EffectHandle,
    /// `session.image_preview` (R-07): the transcript image strip's
    /// gate — off unless the config turns it on (v2's falsy default,
    /// config/index.tsx:164 + session/index.tsx:2459).
    image_preview: Cell<bool>,
    /// The terminal's graphics protocol (R-07): detected once at boot
    /// from the environment (see `crate::image`).
    graphics_protocol: Cell<crate::image::GraphicsProtocol>,
    /// The last frame's image placements (R-07): `render_session`
    /// writes them; the run loop emits them after the cell grid draw
    /// and takes them for delta tracking.
    frame_images: std::cell::RefCell<Vec<crate::image::ImagePlacement>>,
    /// Disclosure state per session (v2 `groupExpanded` /
    /// `setGroupExpanded` on the session-tabs context,
    /// `session-tabs.tsx:387-391`): the set of EXPANDED group row
    /// keys (`group:Reasoning:…`) for each session.
    expanded_groups: std::cell::RefCell<HashMap<String, std::collections::HashSet<String>>>,
    /// The last frame's clickable transcript rows ([`render_session`]
    /// writes them like `frame_images`): the reasoning-group headers
    /// whose absolute `[y, y+height)` rect a mouse release can toggle.
    transcript_hit_rows: std::cell::RefCell<Vec<TranscriptHitRow>>,
}

/// One clickable transcript row from the last frame (absolute rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptHitRow {
    /// The session the row belongs to.
    pub session_id: String,
    /// The row's key (`row_key` — `group:Reasoning:…` for headers).
    pub key: String,
    /// Absolute screen row of the row's top (frame coordinates).
    pub y: usize,
    /// The row's height in rows.
    pub height: usize,
}

/// The store's root signals (one struct so [`Store::build`] stays
/// under clippy's argument budget as the plugin seams landed).
#[derive(Clone)]
struct StoreSignals {
    tabs: Signal<TabsState>,
    tabs_enabled: Signal<bool>,
    session_history: Signal<Vec<crate::data::SessionSnapshot>>,
    route: Signal<Route>,
    dialogs: Signal<Vec<DialogEntry>>,
    theme: Signal<ThemeState>,
    registry: Signal<ThemeRegistry>,
    keybinds: Signal<Vec<KeybindOverride>>,
    connection: Signal<ConnectionState>,
    footer: Signal<FooterMachine>,
    toasts: Signal<ToastQueue>,
    messages: Signal<MessageState>,
    prompt: Signal<PromptState>,
    server_lists: Signal<ServerLists>,
    selection: Signal<SelectionState>,
    worktrees: Signal<crate::components::dialog_worktrees::WorktreeDialogState>,
    plugin_commands: Signal<Vec<PluginCommandEntry>>,
    plugin_dialogs: Signal<HashMap<String, PluginDialogContent>>,
    plugin_sections: Signal<Vec<crate::components::sidebar::SidebarSection>>,
    plugin_hints: Signal<Vec<(String, String)>>,
    plugin_home_actions: Signal<Vec<crate::components::home::PluginQuickAction>>,
    plugin_group_rules: Signal<crate::components::message::model::GroupRules>,
}

impl Store {
    /// The coalesced-redraw effect: every root signal read once, one
    /// request per pump (`(clock, counter, effect)`).
    #[allow(clippy::type_complexity)]
    fn mount_redraw_effect(
        runtime: &Runtime,
        signals: &StoreSignals,
    ) -> (Signal<u64>, Rc<Cell<u64>>, crate::reactive::EffectHandle) {
        let clock = Signal::new(runtime, 0u64);
        let redraw_requests = Rc::new(Cell::new(0u64));
        // Any root-state change requests exactly one redraw per pump.
        let bump = Rc::clone(&redraw_requests);
        let watchers = signals.clone();
        let watch_clock = clock.clone();
        let redraw_effect = Effect::new(runtime, move || {
            let _ = (
                watchers.tabs.get(),
                watchers.route.get(),
                watchers.dialogs.get(),
                watchers.theme.get(),
                watchers.registry.get(),
                watchers.keybinds.get(),
                watchers.connection.get(),
                watchers.footer.get(),
                watchers.toasts.get(),
                watchers.messages.get(),
                watchers.prompt.get(),
                watchers.server_lists.get(),
                watchers.selection.get(),
                watchers.worktrees.get(),
                watchers.plugin_commands.get(),
                watchers.plugin_dialogs.get(),
                watchers.plugin_sections.get(),
                watchers.plugin_hints.get(),
                watchers.plugin_home_actions.get(),
                watchers.plugin_group_rules.get(),
                watch_clock.get(),
            );
            bump.set(bump.get() + 1);
        });
        (clock, redraw_requests, redraw_effect)
    }

    /// The store's constructor body (the signal graph + the redraw
    /// effect over every root signal).
    fn build(runtime: Runtime, signals: StoreSignals) -> Self {
        let prompt_sink: std::rc::Rc<dyn PromptSink> =
            std::rc::Rc::new(RecordingPromptSink::default());
        let (clock, redraw_requests, redraw_effect) = Self::mount_redraw_effect(&runtime, &signals);
        let StoreSignals {
            tabs,
            tabs_enabled,
            session_history,
            route,
            dialogs,
            theme,
            registry,
            keybinds,
            connection,
            footer,
            toasts,
            messages,
            prompt,
            server_lists,
            selection,
            worktrees,
            plugin_commands,
            plugin_dialogs,
            plugin_sections,
            plugin_hints,
            plugin_home_actions,
            plugin_group_rules,
        } = signals;
        Self {
            runtime,
            tabs,
            tabs_enabled,
            session_history,
            route,
            dialogs,
            theme,
            registry,
            keybinds,
            connection,
            footer,
            toasts,
            messages,
            prompt,
            server_lists,
            selection,
            worktrees,
            worktree_sink: std::cell::RefCell::new(Rc::new(RecordingWorktreeSink::default())),
            prompt_sink,
            plugin_commands,
            plugin_dialogs,
            plugin_dialog_source: std::cell::RefCell::new(Rc::new(NoPluginDialogs)),
            plugin_sections,
            plugin_hints,
            plugin_home_actions,
            plugin_group_rules,
            event_tap: std::cell::RefCell::new(None),
            plugin_sink: std::cell::RefCell::new(Rc::new(NoPluginCommands)),
            selection_sink: Rc::new(RecordingSelectionSink::default()),
            clipboard_sink: Rc::new(crate::clipboard::RecordingClipboardSink::default()),
            editor_requested: Cell::new(false),
            attention: Cell::new(AttentionState::default()),
            attention_engine: std::cell::RefCell::new(crate::attention::Attention::new()),
            notifications: std::cell::RefCell::new(
                crate::notifications_plugin::NotificationsPlugin::default(),
            ),
            session_index: std::cell::RefCell::new(
                crate::notifications_plugin::SessionIndex::default(),
            ),
            seen_events: std::cell::RefCell::new((VecDeque::new(), HashSet::new())),
            clock,
            redraw_requests,
            _redraw_effect: redraw_effect,
            image_preview: Cell::new(false),
            graphics_protocol: Cell::new(crate::image::GraphicsProtocol::None),
            frame_images: std::cell::RefCell::new(Vec::new()),
            expanded_groups: std::cell::RefCell::new(HashMap::new()),
            transcript_hit_rows: std::cell::RefCell::new(Vec::new()),
        }
    }

    #[must_use]
    pub fn new(runtime: Runtime) -> Self {
        let tabs = Signal::new(&runtime, TabsState::default());
        let tabs_enabled = Signal::new(&runtime, true);
        let session_history = Signal::new(&runtime, Vec::new());
        let route = Signal::new(&runtime, Route::Home);
        let dialogs = Signal::new(&runtime, Vec::<DialogEntry>::new());
        let theme = Signal::new(&runtime, ThemeState::v2_dark());
        let registry = Signal::new(&runtime, ThemeRegistry::builtins());
        let keybinds = Signal::new(&runtime, Vec::<KeybindOverride>::new());
        let connection = Signal::new(&runtime, ConnectionState::Connecting);
        let footer = Signal::new(&runtime, FooterMachine::new());
        let toasts = Signal::new(&runtime, ToastQueue::new());
        let messages = Signal::new(&runtime, MessageState::default());
        let prompt = Signal::new(&runtime, PromptState::default());
        let server_lists = Signal::new(&runtime, ServerLists::default());
        let selection = Signal::new(&runtime, SelectionState::default());
        let worktrees = Signal::new(
            &runtime,
            crate::components::dialog_worktrees::WorktreeDialogState::default(),
        );
        let plugin_commands = Signal::new(&runtime, Vec::<PluginCommandEntry>::new());
        let plugin_dialogs =
            Signal::new(&runtime, std::collections::HashMap::<String, PluginDialogContent>::new());
        let plugin_sections =
            Signal::new(&runtime, Vec::<crate::components::sidebar::SidebarSection>::new());
        let plugin_hints = Signal::new(&runtime, Vec::<(String, String)>::new());
        let plugin_home_actions =
            Signal::new(&runtime, Vec::<crate::components::home::PluginQuickAction>::new());
        let plugin_group_rules =
            Signal::new(&runtime, crate::components::message::model::GroupRules::empty());
        Self::build(
            runtime,
            StoreSignals {
                tabs,
                tabs_enabled,
                session_history,
                route,
                dialogs,
                theme,
                registry,
                keybinds,
                connection,
                footer,
                toasts,
                messages,
                prompt,
                server_lists,
                selection,
                worktrees,
                plugin_commands,
                plugin_dialogs,
                plugin_sections,
                plugin_hints,
                plugin_home_actions,
                plugin_group_rules,
            },
        )
    }

    // -- accessors --------------------------------------------------------

    #[must_use]
    pub fn tabs(&self) -> TabsState {
        self.tabs.get()
    }

    /// `config.tabs.enabled` (the open-tabs master gate; defaults ON).
    #[must_use]
    pub fn tabs_enabled(&self) -> bool {
        self.tabs_enabled.get()
    }

    /// Apply the resolved `tabs.enabled` config (boot / reload).
    pub fn set_tabs_enabled(&self, enabled: bool) {
        self.tabs_enabled.set(enabled);
    }

    /// The session history from the last resync — the session-list
    /// dialog's rows (v2's `data.session.list()`; the tab strip never
    /// reads this).
    #[must_use]
    pub fn session_history(&self) -> Vec<crate::data::SessionSnapshot> {
        self.session_history.get()
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

    /// `session.image_preview` (R-07): set from the loaded config
    /// (`App::apply_config`); default off (v2's falsy check).
    pub fn set_image_preview(&self, enabled: bool) {
        self.image_preview.set(enabled);
    }

    #[must_use]
    pub fn image_preview(&self) -> bool {
        self.image_preview.get()
    }

    /// The terminal's graphics protocol (R-07): detected once at boot
    /// (the run loop owns the env probe; tests inject).
    pub fn set_graphics_protocol(&self, protocol: crate::image::GraphicsProtocol) {
        self.graphics_protocol.set(protocol);
    }

    #[must_use]
    pub fn graphics_protocol(&self) -> crate::image::GraphicsProtocol {
        self.graphics_protocol.get()
    }

    /// The last frame's image placements (R-07): `render_session`
    /// writes the transcript's overlay set; the run loop reads it
    /// after drawing the cell grid (and replaces it next frame).
    pub fn set_frame_images(&self, images: Vec<crate::image::ImagePlacement>) {
        *self.frame_images.borrow_mut() = images;
    }

    #[must_use]
    pub fn frame_images(&self) -> Vec<crate::image::ImagePlacement> {
        self.frame_images.borrow().clone()
    }

    /// The composer editing state for a session (draft + history),
    /// ready for the renderers and the keypress pump. The autocomplete
    /// state (popup visibility/selection) persists per session so the
    /// popup survives the store round-trip between keypresses.
    #[must_use]
    pub fn composer(&self, session_id: &str) -> Composer {
        let state = self.prompt.get();
        let draft = state.drafts.get(session_id).cloned().unwrap_or_default();
        let caret = draft.text.chars().count();
        let autocomplete =
            state.autocomplete.get(session_id).cloned().unwrap_or_else(Composer::live_autocomplete);
        Composer { draft, caret, history: state.history, autocomplete, ..Composer::default() }
    }

    /// Store a composer edit (draft + history + autocomplete advance
    /// together — the popup state rides the same write).
    pub fn set_composer(&self, session_id: &str, composer: &Composer) {
        self.prompt.update(|state| {
            if composer.draft.is_empty() {
                state.drafts.remove(session_id);
            } else {
                state.drafts.insert(session_id.to_owned(), composer.draft.clone());
            }
            state.autocomplete.insert(session_id.to_owned(), composer.autocomplete.clone());
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
            state.autocomplete.remove(session_id);
            state.history = state.history.clone().push(&draft.text);
        });
        let prompt = SubmittedPrompt { text: draft.text, files: draft.files, agents: draft.agents };
        self.prompt_sink.send(session_id, &prompt);
        Some(prompt)
    }

    /// Forward an already-taken submission to the engine sink (the
    /// composer path: `Composer::handle_key` took the prompt and
    /// recorded history itself — only the sink call remains).
    pub fn forward_submission(&self, session_id: &str, prompt: &SubmittedPrompt) {
        self.prompt_sink.send(session_id, prompt);
    }

    /// Swap the engine prompt sink (tests inject recorders; wave 3
    /// wires the oc-client call).
    pub fn set_prompt_sink(&mut self, sink: std::rc::Rc<dyn PromptSink>) {
        self.prompt_sink = sink;
    }

    /// The composer rail's agent color (`local.agent.color(agent)`):
    /// the active agent's index among the non-hidden server agents,
    /// modulo the theme's categorical palette. `None` while the agent
    /// list has not loaded (v2's `promptDisplay` keeps its previous —
    /// initially undefined — agentColor, so the rail stays
    /// `border.base`) or when no agent is visible.
    #[must_use]
    pub fn composer_rail_color(&self, session_key: &str) -> Option<ratatui::style::Color> {
        let lists = self.server_lists();
        let rows = lists.agents.rows()?;
        let visible: Vec<&ServerAgent> = rows.iter().filter(|agent| !agent.hidden).collect();
        if visible.is_empty() {
            return None;
        }
        let selected = self.selection_for(session_key).agent;
        let index =
            selected.and_then(|id| visible.iter().position(|agent| agent.id == id)).unwrap_or(0);
        Some(self.theme().styles().agent_color(index))
    }

    // -- server-backed pickers + selection (F3.i) ------------------------

    /// The server-backed picker lists.
    #[must_use]
    pub fn server_lists(&self) -> ServerLists {
        self.server_lists.get()
    }

    /// Merge one list fetch result (loading/loaded/failed states per
    /// v2 — the picker rows re-derive from this).
    pub fn set_models(&self, models: ListState<ServerModel>, providers: Vec<ServerProvider>) {
        self.server_lists.update(|lists| {
            lists.models = models;
            lists.providers = providers;
        });
    }

    /// Merge the agent list fetch result.
    pub fn set_agents(&self, agents: ListState<ServerAgent>) {
        self.server_lists.update(|lists| lists.agents = agents);
    }

    // -- the Worktrees dialog (R-05) ---------------------------------------

    /// The current worktree dialog state.
    #[must_use]
    pub fn worktree_dialog(&self) -> crate::components::dialog_worktrees::WorktreeDialogState {
        self.worktrees.get()
    }

    /// Swap the worktree sink.
    pub fn set_worktree_sink(&self, sink: Rc<dyn WorktreeSink>) {
        *self.worktree_sink.borrow_mut() = sink;
    }

    /// Bind the dialog's project (`projectID` + the current checkout
    /// directory — v2 resolves these through the location service at
    /// open; the port receives them from the boot/project fetch).
    pub fn set_worktree_project(&self, project_id: &str, current: Option<&str>) {
        self.worktrees.update(|state| {
            project_id.clone_into(&mut state.project_id);
            state.current = current.map(String::from);
        });
    }

    /// Open the Worktrees dialog (`session.move` / `/worktrees`):
    /// XLARGE chrome (v2 `dialog.setSize("xlarge")`), the list reset
    /// to loading + the fetch fired (tests set the list directly).
    pub fn open_worktrees_dialog(&self) {
        let project = self.worktrees.get().project_id;
        self.worktrees.update(|state| {
            state.list = ListState::Loading;
            state.pending_delete = None;
            state.removing = None;
        });
        self.replace_dialog(DialogKind::Worktrees, DialogSize::Xlarge);
        if !project.is_empty() {
            self.worktree_sink.borrow().list(&project);
        }
    }

    /// A `worktree.list` result: replaces the rows and clears the
    /// armed delete / in-flight removal.
    pub fn set_worktree_list(
        &self,
        list: ListState<crate::components::dialog_worktrees::WorktreeEntry>,
    ) {
        self.worktrees.update(|state| {
            state.list = list;
            state.pending_delete = None;
            state.removing = None;
        });
    }

    /// One ctrl+d press on `directory` — v2's two-press delete
    /// (dialog-workspaces.tsx:240-246): the first press arms the row,
    /// the second on the SAME row confirms, anything else re-targets.
    /// Deletable rows are strategy-managed roots only.
    pub fn worktree_delete_pressed(&self, directory: &str) -> DeleteStep {
        let managed = self.worktrees.get().list.rows().is_some_and(|rows| {
            rows.iter().any(|row| row.directory == directory && row.strategy.is_some())
        });
        if !managed {
            return DeleteStep::Invalid;
        }
        let armed = self.worktrees.get().pending_delete;
        if armed.as_deref() == Some(directory) {
            self.worktrees.update(|state| {
                state.pending_delete = None;
                state.removing = Some(directory.to_owned());
            });
            DeleteStep::Confirmed
        } else {
            self.worktrees.update(|state| state.pending_delete = Some(directory.to_owned()));
            DeleteStep::Armed
        }
    }

    /// Any selection move clears the armed delete (`onMove`).
    pub fn clear_worktree_delete(&self) {
        self.worktrees.update(|state| state.pending_delete = None);
    }

    /// A `worktree.remove` succeeded: drop the row (and the in-flight
    /// marker). v2 refetches on `worktree.updated`; the port mutates
    /// the loaded list.
    pub fn apply_worktree_removed(&self, directory: &str) {
        self.worktrees.update(|state| {
            state.pending_delete = None;
            state.removing = None;
            if let ListState::Loaded(rows) = &mut state.list {
                rows.retain(|row| row.directory != directory);
            }
        });
    }

    /// A `worktree.remove` failed. `force_required` (v2's
    /// `error.data.forceRequired === true`) opens the file-changes
    /// confirm dialog; anything else surfaces the error toast.
    pub fn apply_worktree_remove_failed(&self, directory: &str, error: &str, force_required: bool) {
        self.worktrees.update(|state| {
            state.removing = None;
            state.pending_delete = None;
        });
        if force_required {
            self.replace_dialog(
                DialogKind::WorktreeDeleteConfirm { directory: directory.to_owned() },
                DialogSize::Medium,
            );
        } else {
            self.toast_show(crate::components::toast::ToastOptions {
                title: Some("Failed to delete worktree".to_owned()),
                message: error.to_owned(),
                variant: "error",
                duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
                action_label: None,
            });
        }
    }

    /// A `worktree.create` succeeded (`DialogWorktreeName` →
    /// `worktree.create` → navigate home at the new directory — the
    /// port's Home route carries no location, the row joins the
    /// list).
    pub fn apply_worktree_created(&self, directory: &str) {
        self.worktrees.update(|state| {
            if let ListState::Loaded(rows) = &mut state.list {
                if !rows.iter().any(|row| row.directory == directory) {
                    rows.push(crate::components::dialog_worktrees::WorktreeEntry {
                        directory: directory.to_owned(),
                        strategy: Some("manual".to_owned()),
                    });
                }
            }
        });
        self.clear_dialogs();
        self.navigate(Route::Home);
    }

    /// Submitting a row (v2's `onSelect` → `selectWorkspace`): close
    /// and navigate home at the directory.
    pub fn select_worktree(&self, _directory: &str) {
        self.clear_dialogs();
        self.navigate(Route::Home);
    }

    /// The ctrl+m action: `session.move` POST + close (v2 clears the
    /// dialog first, then moves).
    pub fn move_session_to_worktree(&self, session_id: &str, directory: &str) {
        self.clear_dialogs();
        self.worktree_sink.borrow().move_session(session_id, directory);
    }

    /// The ctrl+r action: `worktree.refresh` (fire-and-forget; the
    /// refetch lands through the list setter).
    pub fn worktree_sink_refresh(&self, project_id: &str) {
        self.worktree_sink.borrow().refresh(project_id);
    }

    /// A confirmed delete: `worktree.remove` (fire-and-forget; the
    /// result lands through `apply_worktree_removed` /
    /// `apply_worktree_remove_failed`).
    pub fn worktree_sink_remove(&self, project_id: &str, directory: &str, force: bool) {
        self.worktree_sink.borrow().remove(project_id, directory, force);
    }

    /// The name dialog's confirm: slugify (empty falls back to a
    /// generated slug — v2's `confirm()`), fire `worktree.create`.
    pub fn worktree_create_from_name(&self, name: &str, slug_seed: u64) {
        let name = crate::components::dialog_worktrees::slugify(name);
        let name = if name.is_empty() {
            crate::components::dialog_worktrees::generate_slug(slug_seed)
        } else {
            name
        };
        let project = self.worktrees.get().project_id;
        self.worktree_sink.borrow().create(&project, &name);
    }

    /// Swap the server selection sink.
    pub fn set_selection_sink(&mut self, sink: Rc<dyn SelectionSink>) {
        self.selection_sink = sink;
    }

    /// The selection state (per-session model/agent drafts).
    #[must_use]
    pub fn selection(&self) -> SelectionState {
        self.selection.get()
    }

    /// The active session's selection (home carries the draft for the
    /// next-created session).
    #[must_use]
    pub fn selection_for(&self, session_id: &str) -> SessionSelection {
        self.selection.get().by_session.get(session_id).cloned().unwrap_or_default()
    }

    /// Select a model: update the session's selection state and
    /// commit it through the server sink (`session.switchModel`).
    pub fn select_model(&self, session_id: &str, model: &ModelSelection) {
        self.selection.update(|state| {
            state.by_session.entry(session_id.to_owned()).or_default().model = Some(model.clone());
        });
        self.selection_sink.switch_model(session_id, model);
    }

    /// Select an agent: update the session's selection and commit it
    /// through the server sink (`session.switchAgent`).
    pub fn select_agent(&self, session_id: &str, agent: &str) {
        self.selection.update(|state| {
            state.by_session.entry(session_id.to_owned()).or_default().agent =
                Some(agent.to_owned());
        });
        self.selection_sink.switch_agent(session_id, agent);
    }

    /// Stage a model selection WITHOUT the server call (the home
    /// route: the pick drafts for the session the next submission
    /// creates — v2 keeps it local until the session exists).
    pub fn stage_model(&self, session_id: &str, model: ModelSelection) {
        self.selection.update(|state| {
            state.by_session.entry(session_id.to_owned()).or_default().model = Some(model);
        });
    }

    /// Stage an agent selection without the server call (home route).
    pub fn stage_agent(&self, session_id: &str, agent: &str) {
        self.selection.update(|state| {
            state.by_session.entry(session_id.to_owned()).or_default().agent =
                Some(agent.to_owned());
        });
    }

    // -- clipboard --------------------------------------------------------

    /// Swap the clipboard sink (production writes OSC 52).
    pub fn set_clipboard_sink(&mut self, sink: Rc<dyn ClipboardSink>) {
        self.clipboard_sink = sink;
    }

    /// Write text through the clipboard sink.
    pub fn clipboard_write(&self, text: &str) -> Result<(), crate::clipboard::ClipboardError> {
        self.clipboard_sink.write(text)
    }

    // -- external editor ---------------------------------------------------

    /// Request the external `$EDITOR` flow (<leader>e / `/editor`):
    /// the run loop suspends the terminal, runs the editor over the
    /// focused draft, and replaces it on success.
    pub fn request_editor(&self) {
        self.editor_requested.set(true);
        self.request_redraw();
    }

    /// Take the editor request (the run loop's side).
    #[must_use]
    pub fn take_editor_request(&self) -> bool {
        self.editor_requested.replace(false)
    }

    // -- attention (bell) ---------------------------------------------------

    /// The attention state (bell request + pending marker).
    #[must_use]
    pub fn attention(&self) -> AttentionState {
        self.attention.get()
    }

    /// Take the bell request (the run loop writes `\x07` once).
    #[must_use]
    pub fn take_bell(&self) -> bool {
        let mut attention = self.attention.get();
        let bell = attention.take_bell();
        self.attention.set(attention);
        bell
    }

    /// Terminal focus returned: consume the pending attention marker
    /// and un-blur the attention engine (the notification channel's
    /// focus gate).
    pub fn set_focused(&self) {
        let mut attention = self.attention.get();
        attention.focused();
        self.attention.set(attention);
        self.attention_engine.borrow().set_focus(crate::attention::FocusState::Focused);
    }

    /// Terminal focus lost: the attention engine blurs (OS
    /// notifications become eligible). The bell pending marker is
    /// unaffected (consumed only on focus return).
    pub fn set_blurred(&self) {
        self.attention_engine.borrow().set_focus(crate::attention::FocusState::Blurred);
    }

    // -- attention (engine: OS notifications + sounds, R-06) ------------------

    /// Install the emission seam (production: OSC notifier + sound
    /// player; tests: recorder). See `attention.rs`.
    pub fn set_attention_emitter(
        &self,
        emitter: std::rc::Rc<dyn crate::attention::AttentionEmitter>,
    ) {
        self.attention_engine.borrow().set_emitter(emitter);
    }

    /// Install the built-in sound resolver (production: the embedded
    /// pack extracted into the cache dir).
    pub fn set_attention_builtin_sounds(
        &self,
        resolve: std::rc::Rc<
            dyn Fn(crate::attention::AttentionSoundName) -> Option<std::path::PathBuf>,
        >,
    ) {
        self.attention_engine.borrow().set_builtin_sounds(resolve);
    }

    /// Apply the resolved `attention` config (boot + config reload).
    pub fn set_attention_config(&self, config: crate::attention::AttentionConfig) {
        self.attention_engine.borrow().set_config(config);
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

    /// Open (or update) a tab and navigate to it
    /// (`context/session-tabs.tsx` `open()` + the route effect).
    /// With tabs disabled the navigation still runs (v2's `open()` is
    /// a no-op but callers navigate separately; sessions stay
    /// reachable with no strip).
    pub fn open_tab(&self, session_id: &str, title: Option<String>) {
        if self.tabs_enabled.get() {
            self.tabs.update(|state| {
                state.tabs = model::open_session_tab(
                    std::mem::take(&mut state.tabs),
                    SessionTab { session_id: session_id.to_owned(), title },
                );
                state.history = model::record_tab_history(&state.history, session_id);
            });
        }
        self.navigate(Route::Session { session_id: session_id.to_owned() });
    }

    /// Restore the persisted tab list on boot (v2's
    /// `useStorage().store("tabs")` hydration): tabs land wholesale
    /// (order + titles preserved). The ROUTE does not move — v2 boots
    /// to the home route regardless of the persisted strip (the
    /// initial route is `args.continue ? session(dummy) : undefined`,
    /// never a restored tab; `app.tsx:364-371`).
    pub fn restore_tabs(&self, tabs: Vec<SessionTab>) {
        if tabs.is_empty() {
            return;
        }
        self.tabs.update(|state| {
            state.tabs = tabs;
        });
    }

    /// Close a tab (`session_id = None` closes the new-session pseudo
    /// tab — a no-op in 7.1, which has no new-tab state yet). A no-op
    /// while tabs are disabled (v2 gates `close()` on `enabled()`).
    pub fn close_tab(&self, session_id: Option<&str>) {
        if !self.tabs_enabled.get() {
            return;
        }
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

    /// Select a session (navigates; the route effect opens its tab if
    /// it is not open — v2's `select()` is a bare
    /// `route.navigate({type:"session"})`, `session-tabs.tsx:393-396`).
    pub fn select_tab(&self, session_id: &str) {
        self.navigate(Route::Session { session_id: session_id.to_owned() });
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

    // -- tab-strip pointer state (beta.8 bug 3) ------------------------------

    /// Set the hovered strip tab (`marquee.enter/leave`); returns
    /// whether the state CHANGED (mouse-move events fire constantly —
    /// only a real change may request a redraw).
    pub fn set_tab_hover(&self, session_id: Option<&str>) -> bool {
        let mut changed = false;
        self.tabs.update(|state| {
            let next = session_id.map(str::to_owned);
            if state.hover != next {
                state.hover = next;
                changed = true;
            }
        });
        changed
    }

    /// Set the ` + ` affordance's hover (`addHovered`); returns
    /// whether the state changed.
    pub fn set_tab_add_hovered(&self, hovered: bool) -> bool {
        let mut changed = false;
        self.tabs.update(|state| {
            if state.add_hovered != hovered {
                state.add_hovered = hovered;
                changed = true;
            }
        });
        changed
    }

    // -- transcript disclosure (beta.8 bug 4) --------------------------------

    /// Toggle a group's disclosure (`setGroupExpanded`,
    /// `session-tabs.tsx:390`) and report the new value.
    pub fn toggle_group_expanded(&self, session_id: &str, key: &str) -> bool {
        let mut groups = self.expanded_groups.borrow_mut();
        let set = groups.entry(session_id.to_owned()).or_default();
        if set.insert(key.to_owned()) {
            true
        } else {
            // Insert returned false: it was already present — toggle off.
            set.remove(key);
            false
        }
    }

    /// Whether a group row renders expanded (`groupExpanded`).
    #[must_use]
    pub fn group_expanded(&self, session_id: &str, key: &str) -> bool {
        self.expanded_groups.borrow().get(session_id).is_some_and(|set| set.contains(key))
    }

    /// The session's expanded-group keys (render options input).
    #[must_use]
    pub fn expanded_group_keys(&self, session_id: &str) -> std::collections::HashSet<String> {
        self.expanded_groups.borrow().get(session_id).cloned().unwrap_or_default()
    }

    /// The last frame's clickable transcript rows (reasoning headers).
    pub fn set_transcript_hit_rows(&self, rows: Vec<TranscriptHitRow>) {
        *self.transcript_hit_rows.borrow_mut() = rows;
    }

    #[must_use]
    pub fn transcript_hit_rows(&self) -> Vec<TranscriptHitRow> {
        self.transcript_hit_rows.borrow().clone()
    }

    // -- route / dialogs / theme / connection --------------------------------

    pub fn navigate(&self, route: Route) {
        if let Route::Session { session_id } = &route {
            self.tabs.update(|state| {
                state.history = model::record_tab_history(&state.history, session_id);
                // Viewing clears unread.
                state.unread.remove(session_id);
            });
            // The route effect (`session-tabs.tsx:180-203`): routing to
            // a session ADDS its tab when it is not open — this is the
            // only way a session enters the strip (a home submission,
            // the session-list dialog, `--continue`/`--session`, a
            // tab click — every path navigates first). The title
            // resolves at open time from the known session data (v2:
            // `data.session.get(id)?.title ?? persisted ?? fallback`);
            // `session.updated` refreshes open tabs in place later.
            if self.tabs_enabled.get() {
                let title = self
                    .session_history
                    .get()
                    .iter()
                    .find(|session| &session.id == session_id)
                    .and_then(|session| session.title.clone());
                self.tabs.update(|state| {
                    if !state.tabs.iter().any(|tab| &tab.session_id == session_id) {
                        state.tabs = model::open_session_tab(
                            std::mem::take(&mut state.tabs),
                            SessionTab { session_id: session_id.clone(), title },
                        );
                    }
                });
            }
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
        self.focus_dialog_current();
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
        self.focus_dialog_current();
    }

    /// A freshly-opened select dialog focuses its `current` option
    /// (`DialogSelect` mount: `focus(current)` + the edge-follow reveal
    /// — the theme list opens scrolled to the active theme).
    fn focus_dialog_current(&self) {
        let Some(entry) = self.top_dialog() else { return };
        if matches!(entry.kind, DialogKind::SessionRename { .. }) {
            return;
        }
        let props = crate::components::command_palette::select_props_for(&entry.kind, self);
        self.update_dialog(|top| {
            top.select.focus_current(&props);
            top.select.reveal(&props, 30);
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

    /// The theme registry (builtins + discovered customs).
    #[must_use]
    pub fn registry(&self) -> ThemeRegistry {
        self.registry.get()
    }

    /// Replace the registry (custom-theme discovery / hot reload).
    pub fn set_registry(&self, registry: ThemeRegistry) {
        self.registry.set(registry);
    }

    /// Merge plugin-registered theme documents into the registry
    /// (registration-time only — the oc-plugin-host snapshot's themes
    /// feed the same discovery path as `<config>/themes/*.json`
    /// files; invalid documents skip with a log, never brick the TUI).
    pub fn set_plugin_themes(&self, themes: &[(String, String)]) {
        let mut registry = self.registry.get();
        for (name, document) in themes {
            if let Err(reason) = registry.insert_custom(name, document) {
                crate::keymap::log_skip(&format!("theme {name}: {reason} (skipped)"));
            }
        }
        self.registry.set(registry);
    }

    /// Select a theme by registry name in the current mode (unknown
    /// names keep the current theme, like the reference's fallback to
    /// `opencode`).
    #[allow(clippy::must_use_candidate)] // fire-and-forget at the dispatch site
    pub fn set_theme_by_name(&self, name: &str) -> bool {
        let mode = self.theme.get().mode;
        match self.registry.get().resolve(name, mode) {
            Some(state) => {
                self.theme.set(state);
                true
            }
            None => false,
        }
    }

    /// The user keybind overrides (the config-reload event payload).
    #[must_use]
    pub fn keybinds(&self) -> Vec<KeybindOverride> {
        self.keybinds.get()
    }

    /// Set the user keybind overrides — a write here IS the config
    /// reload event: the run loop observes the change and re-merges
    /// into the live keymap (user wins, unset removes).
    pub fn set_keybinds(&self, overrides: Vec<KeybindOverride>) {
        self.keybinds.set(overrides);
    }

    // -- plugin seams (registration-time only) ---------------------------------

    /// Replace the plugin palette commands (the host's registration
    /// snapshot, applied once after load / hot reload — never
    /// per-frame).
    pub fn set_plugin_commands(&self, entries: Vec<PluginCommandEntry>) {
        self.plugin_commands.set(entries);
    }

    /// The plugin palette commands (palette data + dispatch lookup).
    #[must_use]
    pub fn plugin_commands(&self) -> Vec<PluginCommandEntry> {
        self.plugin_commands.get()
    }

    /// One plugin command by id.
    #[must_use]
    pub fn plugin_command(&self, command: &str) -> Option<PluginCommandEntry> {
        self.plugin_commands.get().into_iter().find(|entry| entry.command == command)
    }

    /// Attach the post-ingest event tap (plugin-host broadcast seam).
    /// The tap must be `Send + Sync`: it crosses into the host's
    /// dispatch thread while the store stays single-threaded.
    pub fn set_event_tap(&self, tap: std::sync::Arc<dyn EventTap>) {
        *self.event_tap.borrow_mut() = Some(tap);
    }

    /// Attach the plugin command execution sink (palette dispatch).
    pub fn set_plugin_command_sink(&self, sink: Rc<dyn PluginCommandSink>) {
        *self.plugin_sink.borrow_mut() = sink;
    }

    /// Dispatch one plugin command through the sink (the palette's
    /// plugin arm). `true` when a plugin claimed it.
    pub fn execute_plugin_command(&self, command: &str, args: Option<&str>) -> bool {
        if self.plugin_command(command).is_none() {
            return false;
        }
        self.plugin_sink.borrow().execute(command, args)
    }

    // -- plugin seams (wave 3: the TUI slot bridge) -----------------------------

    /// Attach the open-time plugin dialog content source (the
    /// delegation seam behind [`DialogKind::Plugin`]). The source
    /// materializes content ONCE per open — the app wires it to the
    /// host's hard-timeout-bounded call.
    pub fn set_plugin_dialog_source(&self, source: Rc<dyn PluginDialogSource>) {
        *self.plugin_dialog_source.borrow_mut() = source;
    }

    /// One dialog's content: the store's materialized copy if present
    /// (a `ui.dialog.open` delivery), else the source's answer at
    /// OPEN time (synchronous, hard-timeout-bounded by the host).
    #[must_use]
    pub fn plugin_dialog_content(&self, id: &str) -> Option<PluginDialogContent> {
        if let Some(content) = self.plugin_dialogs.get().get(id) {
            return Some(content.clone());
        }
        self.plugin_dialog_source.borrow().content(id)
    }

    /// Register one dialog's content (a fully-built `ui.dialog.open`
    /// delivery) and open it natively.
    pub fn open_plugin_dialog_with(&self, id: &str, content: PluginDialogContent) {
        self.plugin_dialogs.update(|dialogs| {
            dialogs.insert(id.to_owned(), content);
        });
        self.open_dialog(DialogKind::Plugin { id: id.to_owned() }, DialogSize::default());
    }

    /// Open a plugin dialog by id: materialize at open (the source),
    /// then push [`DialogKind::Plugin`]. Unknown ids are a no-op (a
    /// missing plugin never breaks the palette).
    pub fn open_plugin_dialog(&self, id: &str) -> bool {
        let Some(content) = self.plugin_dialog_content(id) else { return false };
        self.plugin_dialogs.update(|dialogs| {
            dialogs.insert(id.to_owned(), content);
        });
        self.open_dialog(DialogKind::Plugin { id: id.to_owned() }, DialogSize::default());
        true
    }

    /// Upsert one plugin's sidebar section (latest wins; an empty
    /// title + empty rows removes the section).
    pub fn set_plugin_sidebar_section(&self, section: &crate::components::sidebar::SidebarSection) {
        let remove = section.title.is_empty() && section.rows.is_empty();
        self.plugin_sections.update(|sections| {
            sections.retain(|existing| existing.plugin_id != section.plugin_id);
            if !remove {
                sections.push(section.clone());
            }
        });
    }

    /// The plugin sidebar sections (render order = registration order).
    #[must_use]
    pub fn plugin_sidebar_sections(&self) -> Vec<crate::components::sidebar::SidebarSection> {
        self.plugin_sections.get()
    }

    /// Upsert one plugin's footer hint segment (empty text removes).
    pub fn set_plugin_footer_hint(&self, plugin_id: &str, text: &str) {
        self.plugin_hints.update(|hints| {
            hints.retain(|(owner, _)| owner != plugin_id);
            if !text.is_empty() {
                hints.push((plugin_id.to_owned(), text.to_owned()));
            }
        });
    }

    /// The plugin footer hint segments (render order).
    #[must_use]
    pub fn plugin_footer_hints(&self) -> Vec<String> {
        self.plugin_hints.get().iter().map(|(_, text)| text.clone()).collect()
    }

    /// Replace the plugin home quick actions (registration-time).
    pub fn set_plugin_home_actions(
        &self,
        actions: Vec<crate::components::home::PluginQuickAction>,
    ) {
        self.plugin_home_actions.set(actions);
    }

    /// The plugin home quick actions.
    #[must_use]
    pub fn plugin_home_actions(&self) -> Vec<crate::components::home::PluginQuickAction> {
        self.plugin_home_actions.get()
    }

    /// Replace the plugin message-grouping rules (registration-time).
    pub fn set_plugin_group_rules(&self, rules: crate::components::message::model::GroupRules) {
        self.plugin_group_rules.set(rules);
    }

    /// The plugin message-grouping rules.
    #[must_use]
    pub fn plugin_group_rules(&self) -> crate::components::message::model::GroupRules {
        self.plugin_group_rules.get()
    }

    /// The open session IDs (tab order) — the resync's message.list
    /// re-fetch set.
    #[must_use]
    pub fn open_session_ids(&self) -> Vec<String> {
        self.tabs.get().tabs.iter().map(|tab| tab.session_id.clone()).collect()
    }

    pub fn switch_theme_mode(&self, mode: oc_theme::types::Mode) {
        let mut theme = self.theme.get();
        theme.switch_mode(mode);
        self.theme.set(theme);
    }

    pub fn set_connection(&self, state: ConnectionState) {
        self.connection.set(state);
    }

    /// A permission reply resolved the ask (the pump's reply path and
    /// the SSE `permission.replied` reducer share this).
    pub fn permission_replied(&self, request_id: &str) {
        self.footer.update(|footer| footer.permission_replied(request_id));
    }

    // -- event ingest ----------------------------------------------------------

    /// Apply a drained batch of data-layer messages **in order**. This is
    /// the single consumer entry point; live events and resync snapshots
    /// are serialized through one channel (see [`crate::data`]) and must
    /// be fully drained before this call so a snapshot can never overtake
    /// queued live events (drain-before-reconcile).
    ///
    /// ## Replay dedup (the F2 decision)
    ///
    /// Reconnects re-subscribe with the SSE `Last-Event-ID` header. A
    /// server that honors it REPLAYS events after the cursor — events
    /// the store already reduced before the disconnect. The store
    /// therefore remembers the last [`SEEN_EVENT_WINDOW`] bus-event ids
    /// and **drops exact re-deliveries** before any reducer runs, so a
    /// replaying server can never double-append `session.text.delta`
    /// onto a message tree (the gate-10 F2 hazard). The window spans
    /// resync snapshots deliberately: a replay crosses exactly that
    /// boundary. Events without ids pass untouched (fixture streams).
    /// See `data.rs` for the full contract.
    pub fn ingest_all(&self, batch: impl IntoIterator<Item = Ingest>) {
        for message in batch {
            self.ingest_one(message);
        }
    }

    /// F2 replay dedup: `true` when `event`'s id was already reduced
    /// (a replayed frame — drop it). Fresh ids are remembered.
    fn is_replayed(&self, event: &BusEvent) -> bool {
        if event.id.is_empty() {
            return false;
        }
        let mut window = self.seen_events.borrow_mut();
        let (order, set) = &mut *window;
        if set.contains(&event.id) {
            return true;
        }
        set.insert(event.id.clone());
        order.push_back(event.id.clone());
        while order.len() > SEEN_EVENT_WINDOW {
            if let Some(evicted) = order.pop_front() {
                set.remove(&evicted);
            }
        }
        false
    }

    fn ingest_one(&self, message: Ingest) {
        match message {
            Ingest::Connected => self.set_connection(ConnectionState::Connected),
            Ingest::Connection(state) => self.set_connection(state),
            Ingest::Event(event) => {
                if self.is_replayed(&event) {
                    return;
                }
                // R-06 — the notifications plugin (v2's
                // `opencode.notifications`): forms, permissions, and the
                // execution lifecycle drive the attention channels (OS
                // notification + sound), and execution failures show a
                // session-failed toast. A sound that actually played
                // suppresses the bell fallback (the bell is the
                // zero-config stand-in for the sound channel).
                self.session_index.borrow_mut().observe(&event);
                let (sound_played, failure_toast) = self.notifications.borrow_mut().handle(
                    &event,
                    &self.session_index.borrow(),
                    &*self.attention_engine.borrow(),
                );
                if let Some(toast) = failure_toast {
                    self.toast_show(ToastOptions {
                        title: Some("Session failed".to_owned()),
                        message: toast.message,
                        variant: "error",
                        duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
                        action_label: None,
                    });
                }
                // Attention (the notifications plugin's always-on
                // channel): permission asks and forms ring the bell
                // unless a real sound already played.
                if crate::attention::AttentionState::rings_on(&event) && !sound_played {
                    let mut attention = self.attention.get();
                    attention.ring();
                    self.attention.set(attention);
                }
                self.reduce_event(&event);
                // The broadcast tap: AFTER the reducer, in ingest
                // order — plugins see the same deduplicated, ordered
                // stream the store just reduced (never a replayed
                // frame: those returned above). Non-blocking by
                // contract.
                if let Some(tap) = self.event_tap.borrow().as_ref() {
                    tap.publish(&event);
                }
            }
            Ingest::Resync { sessions, unread, messages } => {
                // Reconcile AFTER the channel drained (v2's
                // `normalize()` effect, `session-tabs.tsx:241-250`):
                // the resync NEVER admits new tabs — only OPEN tabs
                // refresh in place (title updates from the wire);
                // routing is the only way a session enters the strip.
                // The full snapshot becomes the session HISTORY (the
                // session-list dialog's source, like v2's
                // `data.session.list()`).
                self.session_history.set(sessions.clone());
                self.tabs.update(|state| {
                    let open: Vec<&crate::data::SessionSnapshot> = sessions
                        .iter()
                        .filter(|session| state.tabs.iter().any(|tab| tab.session_id == session.id))
                        .collect();
                    for session in open {
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
                for (session_id, list) in messages {
                    self.set_messages(&session_id, list);
                }
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
                    // The unread TRANSITION is an attention event
                    // (background activity): ring the bell unless the
                    // tab was already unread (v2's dedup-by-session
                    // discipline in the notifications plugin).
                    let was_unread = self.tabs.get().unread.contains_key(&id);
                    if !was_unread {
                        let mut attention = self.attention.get();
                        attention.ring();
                        self.attention.set(attention);
                    }
                    self.set_unread(&id, Some(Unread::Activity));
                }
                // Lane C: a full message payload projects into the tree
                // (deltas below patch it part-wise).
                if let Some(message) = message_from_event(event) {
                    self.project_message(&id, message);
                }
            }
            // The user message arrives as an inbox item (`data.ts`:
            // session.inbox.enqueued with item.type "user" →
            // admitLocal inserts `{id: inboxID, ...item}` into the
            // message list).
            "session.inbox.enqueued" => {
                let Some(session) = event.string("sessionID") else { return };
                let active = self.tabs.get().active(&self.route.get());
                if active.as_deref() != Some(session.as_str()) {
                    // Same unread-transition attention as message.updated.
                    let was_unread = self.tabs.get().unread.contains_key(&session);
                    if !was_unread {
                        let mut attention = self.attention.get();
                        attention.ring();
                        self.attention.set(attention);
                    }
                    self.set_unread(&session, Some(Unread::Activity));
                }
                let Some(item) = event.data.get("item") else { return };
                if item.get("type").and_then(Value::as_str) == Some("user") {
                    let id = event.string("inboxID").unwrap_or_else(|| event.id.clone());
                    let text = item
                        .pointer("/payload/text")
                        .or_else(|| item.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let created = event.created.unwrap_or(0);
                    // `event.created` is epoch SECONDS on this event —
                    // match the transcript's ms convention loosely (the
                    // label renders relative time).
                    self.project_message(
                        &session,
                        crate::components::message::model::Message::User {
                            id,
                            text: text.to_owned(),
                            created,
                            files: Vec::new(),
                        },
                    );
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
                    // G11-F1: a delta for a message a snapshot already
                    // completed (the first-connect snapshot/stream
                    // overlap window double-appends text) is dropped —
                    // the snapshot's text is authoritative. A
                    // legitimate new step reopens the message
                    // (`session.step.started` clears completion)
                    // before its deltas arrive.
                    if message_already_complete(messages, &assistant) {
                        return;
                    }
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
                    // G11-F1 (same window, same rule as text deltas).
                    if message_already_complete(messages, &assistant) {
                        return;
                    }
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
                                error: None,
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

/// G11-F1: whether the assistant message exists and is already
/// complete (a snapshot's fully-formed tree — `time.completed` set).
fn message_already_complete(messages: &[Message], assistant: &str) -> bool {
    messages.iter().any(|message| {
        message.id() == assistant
            && matches!(message, Message::Assistant { time, .. } if time.completed.is_some())
    })
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
            error: None,
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
            error: None,
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

/// The resolved `files` array off a `message.updated` user payload →
/// the model's `UserFile` set (deduped inline images only).
fn files_from_event(data: &Value) -> Vec<crate::components::message::model::UserFile> {
    let Some(files) = data.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mapped: Vec<crate::components::message::model::UserFile> = files
        .iter()
        .filter_map(|file| {
            Some(crate::components::message::model::UserFile {
                mime: file.get("mime")?.as_str()?.to_owned(),
                data: file.get("data")?.as_str()?.to_owned(),
                name: file.get("name").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect();
    crate::components::message::model::deduplicate_visible_files(&mapped)
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
            // `message.updated` carries the resolved `files` array
            // (SSE echo of the stored user message).
            files: files_from_event(&event.data),
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
                // `message.error.message` (SessionStructuredError): the
                // `Error: …` transcript row + muted footer agent label.
                error: event
                    .data
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                time,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::command_palette::CommandDispatcher as _;
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
                sessions: vec![
                    SessionSnapshot {
                        id: "a".to_owned(),
                        title: Some("Fresh title".to_owned()),
                        time_idle: None,
                        viewed: None,
                    },
                    SessionSnapshot {
                        id: "not-open".to_owned(),
                        title: Some("History only".to_owned()),
                        time_idle: None,
                        viewed: None,
                    },
                ],
                unread: vec!["a".to_owned()],
                messages: Vec::new(),
            },
        ]);
        let tabs = store.tabs();
        assert_eq!(tabs.tabs.len(), 1, "the resync never admits new tabs");
        assert_eq!(tabs.tabs[0].title.as_deref(), Some("Fresh title"));
        assert_eq!(
            tabs.unread.get("a"),
            Some(&Unread::Activity),
            "unread from the offline event survives the snapshot"
        );
        assert_eq!(store.connection(), ConnectionState::Connected);
        // The full snapshot becomes the session history (the dialog's
        // source) — including sessions that are not open tabs.
        let history = store.session_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].id, "not-open");
        assert_eq!(history[1].title.as_deref(), Some("History only"));
    }

    /// The open-tabs model: the strip only ever holds OPEN tabs.
    /// Restore hydrates the strip WITHOUT moving the route (v2 boots
    /// home — `app.tsx:364-371`); routing to a session is what adds
    /// its tab (the route effect, `session-tabs.tsx:180-203`); with
    /// tabs disabled nothing opens but navigation still works.
    #[test]
    fn tabs_open_only_through_routing_and_restore_keeps_home() {
        let store = Store::new(Runtime::new());
        assert_eq!(store.route(), Route::Home, "boots home");

        // Restore hydrates the strip; the route stays home.
        store.restore_tabs(vec![
            SessionTab { session_id: "ses_1".into(), title: Some("One".into()) },
            SessionTab { session_id: "ses_2".into(), title: None },
        ]);
        assert_eq!(store.tabs().tabs.len(), 2);
        assert_eq!(store.route(), Route::Home, "restore never moves the route");

        // Routing to an open tab: no duplicate.
        store.select_tab("ses_1");
        assert_eq!(store.tabs().tabs.len(), 2);

        // Routing to a NOT-open session (the session-list dialog):
        // the route effect admits exactly one new tab.
        store.select_tab("ses_3");
        assert_eq!(store.tabs().tabs.len(), 3);
        assert!(store.tabs().tabs.iter().any(|tab| tab.session_id == "ses_3"));

        // Disabled (`tabs.enabled = false` / mode "off"): navigation
        // works, the strip stays as-is.
        store.set_tabs_enabled(false);
        store.select_tab("ses_4");
        assert_eq!(store.route(), Route::Session { session_id: "ses_4".to_owned() });
        assert_eq!(store.tabs().tabs.len(), 3, "no new tab while disabled");
        // Closing while disabled: the strip is inert (v2's close() is
        // gated on enabled()).
        store.close_tab(Some("ses_4"));
        assert_eq!(store.tabs().tabs.len(), 3);
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

    // -- Phase 8 lane B: attention / replay dedup / restore / selection --

    /// The bell fires once per permission ask (the run loop's `\x07`
    /// write drains it); the pending marker survives until focus.
    #[test]
    fn permission_asks_ring_the_bell_once() {
        let store = Store::new(Runtime::new());
        assert!(!store.take_bell(), "nothing rings at boot");
        store.ingest_all([Ingest::Event(BusEvent::new(
            "permission.asked",
            serde_json::json!({ "id": "p1", "sessionID": "ses", "action": "read" }),
        ))]);
        assert!(store.take_bell(), "permission.asked rings");
        assert!(!store.take_bell(), "exactly once");
        assert!(store.attention().pending, "the marker outlives the bell");
        store.set_focused();
        assert!(!store.attention().pending, "focus consumes the marker");
        // form.created rings too (the question channel).
        store.ingest_all([Ingest::Event(BusEvent::new(
            "form.created",
            serde_json::json!({ "form": { "id": "f1", "sessionID": "ses", "title": "T", "fields": [] } }),
        ))]);
        assert!(store.take_bell());
    }

    // -- R-06: the attention engine wiring (sounds + OS notifications) --

    /// A recorder over the store's emission seam.
    struct AttentionRecorder {
        notifications: std::cell::RefCell<Vec<(String, String)>>,
        sounds: std::cell::RefCell<Vec<std::path::PathBuf>>,
        play_ok: bool,
    }

    impl Default for AttentionRecorder {
        fn default() -> Self {
            Self {
                notifications: std::cell::RefCell::new(Vec::new()),
                sounds: std::cell::RefCell::new(Vec::new()),
                play_ok: true,
            }
        }
    }

    impl crate::attention::AttentionEmitter for AttentionRecorder {
        fn notify(&self, title: &str, message: &str) -> bool {
            self.notifications.borrow_mut().push((title.to_owned(), message.to_owned()));
            true
        }

        fn play(&self, candidates: &[crate::attention::SoundCandidate], _volume: f64) -> bool {
            let played = self.play_ok && !candidates.is_empty();
            if played {
                self.sounds.borrow_mut().push(candidates[0].path.clone());
            }
            played
        }
    }

    fn store_with_attention(
        config: crate::attention::AttentionConfig,
    ) -> (Store, std::rc::Rc<AttentionRecorder>) {
        let store = Store::new(Runtime::new());
        store.set_attention_config(config);
        let recorder = std::rc::Rc::new(AttentionRecorder::default());
        store.set_attention_emitter(recorder.clone());
        store.set_attention_builtin_sounds(std::rc::Rc::new(|name| {
            Some(std::path::PathBuf::from(format!("/builtin/{}.mp3", name.as_str())))
        }));
        (store, recorder)
    }

    /// A played sound suppresses the bell fallback on the same event
    /// (no double attention); with sounds disabled the bell stands.
    #[test]
    fn a_played_sound_suppresses_the_bell() {
        // Default config (sound off): the bell rings.
        let store = Store::new(Runtime::new());
        store.ingest_all([Ingest::Event(BusEvent::new(
            "permission.asked",
            serde_json::json!({ "id": "p1", "sessionID": "ses", "action": "read" }),
        ))]);
        assert!(store.take_bell(), "the zero-config bell fallback");

        // Sound enabled + emitter plays: the bell is suppressed.
        let (store, recorder) = store_with_attention(crate::attention::AttentionConfig {
            sound: true,
            ..crate::attention::AttentionConfig::default()
        });
        store.ingest_all([Ingest::Event(BusEvent::new(
            "permission.asked",
            serde_json::json!({ "id": "p1", "sessionID": "ses", "action": "read" }),
        ))]);
        assert!(!store.take_bell(), "a real sound replaced the bell");
        assert_eq!(recorder.sounds.borrow().len(), 1);
        assert_eq!(
            recorder.sounds.borrow()[0],
            std::path::PathBuf::from("/builtin/permission.mp3"),
            "the permission event maps to the permission sound"
        );
        assert!(!store.attention().pending, "the ring (bell + marker) was fully suppressed");
    }

    /// Execution failures show the session-failed toast and play the
    /// error sound; the following idle success is suppressed (v2's
    /// error-once discipline).
    #[test]
    fn execution_failure_toasts_and_plays_error_sound() {
        let (store, recorder) = store_with_attention(crate::attention::AttentionConfig {
            sound: true,
            ..crate::attention::AttentionConfig::default()
        });
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.execution.failed",
            serde_json::json!({ "sessionID": "ses", "error": { "type": "unknown", "message": "boom" } }),
        ))]);
        assert_eq!(
            recorder.sounds.borrow().last().cloned(),
            Some(std::path::PathBuf::from("/builtin/error.mp3"))
        );
        let toasts = store.toasts();
        let toast = toasts.current().expect("the failure toast is showing");
        assert_eq!(toast.title.as_deref(), Some("Session failed"));
        assert_eq!(toast.message, "boom");
        assert_eq!(toast.variant, "error");

        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.execution.succeeded",
            serde_json::json!({ "sessionID": "ses" }),
        ))]);
        assert_eq!(
            recorder.sounds.borrow().len(),
            1,
            "the idle done after the error is suppressed"
        );
        assert!(!store.take_bell(), "execution events never bell");
    }

    /// Focus wiring: FocusGained/Blur flip the engine's gate — an OS
    /// notification (blurred-only channel) fires blurred, not focused.
    #[test]
    fn focus_events_gate_the_notification_channel() {
        let (store, recorder) = store_with_attention(crate::attention::AttentionConfig {
            notifications: true,
            ..crate::attention::AttentionConfig::default()
        });
        let ask = |id: &str| {
            Ingest::Event(BusEvent::new(
                "permission.asked",
                serde_json::json!({ "id": id, "sessionID": "ses", "action": "read" }),
            ))
        };
        store.ingest_all([ask("p1")]);
        assert!(recorder.notifications.borrow().is_empty(), "focus unknown → no notification");

        store.set_blurred();
        store.ingest_all([ask("p2")]);
        assert_eq!(recorder.notifications.borrow().len(), 1, "blurred → notified");

        store.set_focused();
        store.ingest_all([ask("p3")]);
        assert_eq!(recorder.notifications.borrow().len(), 1, "focused → skipped");
    }

    /// Background activity rings only on the unread TRANSITION (a
    /// stream of updates into an already-unread tab bells once), and
    /// never for the active session.
    #[test]
    fn unread_transitions_ring_only_on_the_transition() {
        let store = store_with_tabs(&["a", "b"]);
        store.select_tab("a");
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({ "sessionID": "b" }),
        ))]);
        assert!(store.take_bell(), "the first background message rings");
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({ "sessionID": "b" }),
        ))]);
        assert!(!store.take_bell(), "already unread — no re-ring");
        store.ingest_all([Ingest::Event(BusEvent::new(
            "message.updated",
            serde_json::json!({ "sessionID": "a" }),
        ))]);
        assert!(!store.take_bell(), "the active session never rings");
    }

    /// F2 replay dedup at the store seam: the same event id reduces
    /// exactly once (a replaying server cannot double-append deltas);
    /// id-less events pass untouched.
    #[test]
    fn replayed_event_ids_reduce_once() {
        let store = Store::new(Runtime::new());
        let delta = |id: &str, text: &str| {
            let mut event = BusEvent::new(
                "session.text.delta",
                serde_json::json!({ "sessionID": "s", "assistantMessageID": "a", "delta": text }),
            );
            event.id = id.to_owned();
            Ingest::Event(event)
        };
        store.ingest_all([delta("evt_1", "Hello")]);
        // The replayed frame (same id, same payload) is dropped.
        store.ingest_all([delta("evt_1", "Hello")]);
        let messages = store.messages("s");
        assert_eq!(messages[0].parts()[0].text(), "Hello", "no double append");
        // A fresh id applies.
        store.ingest_all([delta("evt_2", " World")]);
        assert_eq!(store.messages("s")[0].parts()[0].text(), "Hello World");
        // Id-less events (fixture streams) pass through every time.
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.text.delta",
            serde_json::json!({ "sessionID": "s", "assistantMessageID": "a", "delta": "!" }),
        ))]);
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.text.delta",
            serde_json::json!({ "sessionID": "s", "assistantMessageID": "a", "delta": "!" }),
        ))]);
        assert_eq!(store.messages("s")[0].parts()[0].text(), "Hello World!!");
    }

    /// Restored tabs land wholesale (order + titles); the ROUTE stays
    /// home — the boot route is home unless `--continue`/`--session`
    /// says otherwise (v2 app.tsx:364-371, never a restored tab); an
    /// empty document stays home too.
    #[test]
    fn restore_tabs_hydrates_in_order() {
        let store = Store::new(Runtime::new());
        store.restore_tabs(Vec::new());
        assert_eq!(store.route(), Route::Home, "empty document stays home");
        store.restore_tabs(vec![
            SessionTab { session_id: "ses_1".into(), title: Some("First".into()) },
            SessionTab { session_id: "ses_2".into(), title: None },
        ]);
        let tabs = store.tabs();
        assert_eq!(tabs.tabs.len(), 2);
        assert_eq!(tabs.tabs[0].title.as_deref(), Some("First"));
        assert_eq!(store.route(), Route::Home, "restore never moves the route");
    }

    /// The selection state and the sink contract: select_* updates the
    /// state AND fires the sink; stage_* is state-only (the home
    /// route's next-session draft).
    #[test]
    fn selection_state_and_sink_contract() {
        use super::{RecordingSelectionSink, SelectionCall};
        let mut store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingSelectionSink::default());
        let sink_dyn: std::rc::Rc<dyn crate::store::SelectionSink> = sink.clone();
        store.set_selection_sink(sink_dyn);
        store.open_tab("s1", None);
        store.select_model(
            "s1",
            &ModelSelection {
                provider_id: "anthropic".into(),
                model_id: "claude".into(),
                variant: Some("high".into()),
            },
        );
        store.select_agent("s1", "plan");
        assert_eq!(
            sink.calls.borrow().as_slice(),
            &[
                (
                    "s1".into(),
                    SelectionCall::Model {
                        provider_id: "anthropic".into(),
                        model_id: "claude".into(),
                        variant: Some("high".into())
                    }
                ),
                ("s1".into(), SelectionCall::Agent("plan".into())),
            ]
        );
        assert_eq!(store.selection_for("s1").model.as_ref().unwrap().model_id, "claude");
        assert_eq!(store.selection_for("s1").agent.as_deref(), Some("plan"));

        store.stage_model(
            "home",
            ModelSelection {
                provider_id: "opencode".into(),
                model_id: "grok".into(),
                variant: None,
            },
        );
        assert_eq!(sink.calls.borrow().len(), 2, "staging fires no sink call");
        assert_eq!(store.selection_for("home").model.as_ref().unwrap().model_id, "grok");
    }

    // -- plugin seams (wave 9.1) ------------------------------------------------

    /// A recording tap: proves publish order, post-ingest timing, and
    /// dedup parity with the store.
    struct RecordingTap {
        seen: std::sync::Mutex<Vec<String>>,
    }

    impl crate::store::EventTap for RecordingTap {
        fn publish(&self, event: &BusEvent) {
            self.seen.lock().expect("tap").push(format!("{}:{}", event.id, event.type_));
        }
    }

    #[test]
    fn event_tap_sees_the_reduced_ordered_deduped_stream() {
        let store = Store::new(Runtime::new());
        let tap = std::sync::Arc::new(RecordingTap { seen: std::sync::Mutex::new(Vec::new()) });
        store.set_event_tap(tap.clone());
        let event = |id: &str, type_: &str| BusEvent {
            id: id.to_owned(),
            type_: type_.to_owned(),
            data: serde_json::json!({}),
            created: None,
        };
        // Two events + one REPLAY of the first: the tap sees exactly
        // the two the store reduced, in order, and never the replay.
        store.ingest_all([
            Ingest::Event(event("evt_1", "session.updated")),
            Ingest::Event(event("evt_2", "session.updated")),
        ]);
        store.ingest_all([Ingest::Event(event("evt_1", "session.updated"))]);
        let seen = tap.seen.lock().expect("tap").clone();
        assert_eq!(seen, vec!["evt_1:session.updated", "evt_2:session.updated"]);
        // Post-ingest placement means the tap fires synchronously
        // inside ingest_all — by the time it returns, every publish
        // already happened (the probe above would interleave otherwise).
        assert_eq!(tap.seen.lock().expect("tap").len(), 2);
    }

    #[test]
    fn g11f1_deltas_for_snapshot_complete_messages_are_dropped() {
        let store = store_with_tabs(&["ses_9"]);
        // A snapshot-built COMPLETE message (message.list re-fetch).
        store.set_messages(
            "ses_9",
            vec![Message::Assistant {
                id: "msg_a".into(),
                agent: "build".into(),
                model: String::new(),
                error: None,
                parts: vec![Part::Text {
                    text: "final answer".into(),
                    time: PartTime { created: Some(1), completed: Some(2) },
                }],
                finish: Some("stop".into()),
                time: PartTime { created: Some(1), completed: Some(2) },
            }],
        );
        // The G11-F1 window: a session.text.delta replayed after the
        // snapshot must NOT append onto the complete message.
        store.ingest_all([Ingest::Event(BusEvent {
            id: "evt_d1".into(),
            type_: "session.text.delta".into(),
            data: serde_json::json!({
                "sessionID": "ses_9", "assistantMessageID": "msg_a", "delta": "EXTRA"
            }),
            created: Some(3),
        })]);
        let messages = store.messages("ses_9");
        let Message::Assistant { parts, .. } = &messages[0] else { panic!("assistant") };
        let Part::Text { text, .. } = &parts[0] else { panic!("text part") };
        assert_eq!(text, "final answer", "delta dropped for the completed message");

        // A reopened message (session.step.started clears completion)
        // accepts deltas again.
        store.ingest_all([Ingest::Event(BusEvent {
            id: "evt_s1".into(),
            type_: "session.step.started".into(),
            data: serde_json::json!({
                "sessionID": "ses_9", "assistantMessageID": "msg_a"
            }),
            created: Some(4),
        })]);
        store.ingest_all([Ingest::Event(BusEvent {
            id: "evt_d2".into(),
            type_: "session.text.delta".into(),
            data: serde_json::json!({
                "sessionID": "ses_9", "assistantMessageID": "msg_a", "delta": "new turn"
            }),
            created: Some(5),
        })]);
        let messages = store.messages("ses_9");
        let Message::Assistant { parts, .. } = &messages[0] else { panic!("assistant") };
        let Part::Text { text, .. } = &parts[0] else { panic!("text part") };
        assert_eq!(text, "final answernew turn", "reopened message accumulates");
    }

    #[test]
    fn plugin_commands_and_themes_land() {
        // Dispatch routes through the sink (unknown to it → falls
        // through to Unknown).
        #[derive(Default)]
        struct Sink(std::cell::RefCell<Vec<String>>);
        impl PluginCommandSink for Sink {
            fn execute(&self, command: &str, _args: Option<&str>) -> bool {
                self.0.borrow_mut().push(command.to_owned());
                true
            }
        }
        let store = Store::new(Runtime::new());
        store.set_plugin_commands(vec![PluginCommandEntry {
            command: "hw.hello".into(),
            title: "Say hello".into(),
            plugin_id: "helloworld".into(),
        }]);
        assert_eq!(store.plugin_commands().len(), 1);
        assert_eq!(store.plugin_command("hw.hello").unwrap().plugin_id, "helloworld");

        // Theme documents feed the registry discovery path (a real v1
        // asset — valid by construction).
        let names_before = store.registry().names().len();
        store.set_plugin_themes(&[(
            "helloworld-theme".to_owned(),
            include_str!("../../oc-theme/assets/v1/aura.json").to_owned(),
        )]);
        let names_after = store.registry().names().len();
        assert_eq!(names_after, names_before + 1, "plugin theme merged into the registry");
        assert!(store.registry().get("helloworld-theme").is_some());

        // The palette carries the plugin row (group "Plugin").
        let props = crate::components::command_palette::palette_props(&store);
        let plugin_row = props
            .options
            .iter()
            .find(|option| option.value == "hw.hello")
            .expect("plugin row in the palette");
        assert_eq!(plugin_row.category.as_deref(), Some("Plugin"));

        let sink = Rc::new(Sink::default());
        store.set_plugin_command_sink(Rc::clone(&sink) as Rc<dyn PluginCommandSink>);
        let mut dispatcher = crate::components::command_palette::StoreDispatcher;
        assert_eq!(
            dispatcher.dispatch(&store, "hw.hello", None),
            crate::components::command_palette::Dispatch::Handled
        );
        assert_eq!(sink.0.borrow().clone(), vec!["hw.hello".to_owned()]);
        // Unregistered commands still Unknown.
        assert_eq!(
            dispatcher.dispatch(&store, "nope.nope", None),
            crate::components::command_palette::Dispatch::Unknown
        );
    }

    #[test]
    fn message_updated_user_files_project_into_the_model() {
        // R-07: the SSE echo of a stored user message carries the
        // resolved `files` array — inline images land on the model
        // (deduped: the identical payload twice shows once).
        let event = crate::data::BusEvent::new(
            "message.updated",
            serde_json::json!({
                "id": "msg_1",
                "type": "user",
                "text": "describe this",
                "time": {"created": 5},
                "files": [
                    {"mime": "image/png", "data": "AAECAw==", "name": "a.png"},
                    {"mime": "image/png", "data": "AAECAw==", "name": "a.png"},
                    {"mime": "application/pdf", "data": "JVBERi0=", "name": "r.pdf"}
                ]
            }),
        );
        let Some(crate::components::message::model::Message::User { files, .. }) =
            message_from_event(&event)
        else {
            panic!("user message");
        };
        assert_eq!(files.len(), 2, "duplicate inline image deduped, pdf kept");
        assert!(files[0].is_image());
        assert_eq!(files[0].data_uri(), "data:image/png;base64,AAECAw==");
        assert!(!files[1].is_image());

        // Absent files stay empty (pre-R-07 messages).
        let bare = crate::data::BusEvent::new(
            "message.updated",
            serde_json::json!({"id": "msg_2", "type": "user", "text": "hi"}),
        );
        let Some(crate::components::message::model::Message::User { files, .. }) =
            message_from_event(&bare)
        else {
            panic!("user message");
        };
        assert_eq!(files, Vec::new());
    }
}
