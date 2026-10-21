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

use std::cell::{Cell, RefCell};
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
use crate::components::message::model::{Message, Part, PartTime, ToolStatus};
use crate::components::toast::{ToastOptions, ToastQueue};
use crate::data::{BusEvent, ConnectionState, Ingest};
use crate::keymap::KeybindOverride;
use crate::mini::model_pref;
use crate::reactive::{Effect, Runtime, Signal};
use crate::theme::{ThemeRegistry, ThemeState};
use oc_tui_core::scroll::{FollowState, ScrollCommand, ScrollDriver};

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
    /// `DialogUpdate` (`/update` — component/dialog-update.tsx, lane
    /// TAIL auditB #10): the check/available/installed/current/failed
    /// states with Skip + Update/Restart actions.
    Update,
    /// `DialogSessionRename`: rename the active session.
    SessionRename { session_id: String },
    /// The plugin manager select (`plugins.list`, lane KM — v2's
    /// `DialogPlugins` approximation): one row per REGISTERED plugin
    /// id; enable/disable state is not modeled (submits close).
    Plugins,
    /// The tab context menu (lane KM, auditA #8b — v2's
    /// `TabContextMenu`, session-tabs.tsx:372-538): opened by a
    /// right-press at `(x, y)`; `session_id = None` renders the
    /// recently-closed variant. Rendered AT the pointer (never
    /// centered, no backdrop dim).
    TabMenu { session_id: Option<String>, x: usize, y: usize },
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
    /// `DialogMessage` (`routes/session/dialog-message.tsx`): the
    /// user-message "Message Actions" select (Jump to / Revert /
    /// Copy / Fork) opened by clicking a user-message block
    /// ([`UserMessage`]'s `onMouseUp`, session/index.tsx:2210-2250).
    MessageActions { session_id: String, message_id: String },
    /// The working-directory select (`ui/working-directory-actions.tsx`):
    /// Copy path / Open folder / Workspaces, opened by clicking the
    /// composer footer's `{dir}:{branch}` label
    /// (component/prompt/index.tsx:1925-1937).
    WorkingDirectory { directory: String },
    /// A plugin dialog (`DialogKind::Plugin`, wave 3's open dialog
    /// seam): the content was materialized at OPEN time through
    /// [`PluginDialogSource`] (or arrived fully-built on a
    /// `ui.dialog.open` request) and renders natively — the id keys
    /// the store's [`PluginDialogContent`].
    Plugin { id: String },
    /// The queued-prompts manager (`session.queued_prompts`,
    /// `<leader>q` — v2's `DialogSelect` "Queued prompts",
    /// index.tsx:627-675): enter steers the selected prompt, ctrl+d
    /// deletes it (`queued_prompt.delete`).
    QueuedPrompts,
    /// SURF-1: the tool detail viewer — `DialogExecute` for `execute`
    /// (dialog-execute.tsx; `dialog.replace` at index.tsx:3207) and
    /// `DialogShellOutput` for background shells
    /// (dialog-shell-output.tsx). The content materializes from the
    /// transcript's tool part state.
    ToolOutput { session_id: String, message_id: String, part_id: String },
    /// SURF-2: the diff source select (`diff.switch_source`,
    /// diff-viewer.tsx:735-767): All / Committed / Uncommitted
    /// (+ Last turn from a session route).
    DiffSource,
    /// SURF-2: the diff shortcuts dialog (`DiffViewerHelpDialog`,
    /// diff-viewer.tsx:1100-1192) — `diff.help`.
    DiffHelp,
    /// SURF-2: `DialogMcp` (`/mcps`, mcp.list — dialog-mcp.tsx): the
    /// per-server status rows with the toggle action.
    Mcp,
    /// SURF-2: the MCP error details (`MCP server: {name}` —
    /// dialog-mcp.tsx's `DialogErrorDetails` role).
    McpError { name: String },
    /// SURF-2: `DialogStatus` (`/status`, opencode.status —
    /// dialog-status.tsx): the MCP inventory summary.
    Status,
    /// SURF-2: `DialogVariant` (`/variants`, variant.list —
    /// dialog-variant.tsx): the current model's variants.
    Variant,
    /// SURF-2: `DialogPair` (`/pair`, server.pair — dialog-pair.tsx):
    /// the pairing links + code expiry.
    Pair,
    /// SURF-2: `DialogDebug` (`/debug`, opencode.debug —
    /// dialog-debug.tsx): the env/config dump with the copy action.
    Debug,
    /// SURF-2: `DialogTimeline` (`session.timeline` —
    /// routes/session/dialog-timeline.tsx): the user-message turn
    /// list; enter opens the message actions.
    Timeline { session_id: String },
    /// Lane FINAL: `DialogIntegration` (`provider.connect` —
    /// component/dialog-integration.tsx): the integration list over
    /// the BE-3 host (`integration.list`), submit routes to the
    /// accounts view (credentials present) or the method choice.
    Integration,
    /// The manage-connections view (`manageConnections`,
    /// dialog-integration.tsx:148-263): "Add account" + the
    /// connected credential rows (select activates).
    IntegrationAccounts { integration_id: String },
    /// `selectMethod` (dialog-integration.tsx:265-283): one row per
    /// non-env connect method.
    IntegrationMethod { integration_id: String },
    /// `KeyMethod` (dialog-integration.tsx:471-506): the API-key
    /// prompt (`integration.connect.key` on confirm).
    IntegrationKey { integration_id: String, method_id: String, title: String },
    /// `OAuthCode` (dialog-integration.tsx:676-728): the
    /// authorization-code prompt (`integration.oauth.complete`); the
    /// attempt's URL + instructions ride the store's attempt state.
    IntegrationCode { integration_id: String },
    /// `OAuthAuto`/`CommandPending`'s waiting view (the 500 ms
    /// status poll): URL + instructions rows while the worker polls.
    IntegrationPending { integration_id: String },
    /// Lane FINAL: `DialogConfig` (`opencode.settings` —
    /// component/dialog-config.tsx): the settings rows that this
    /// build both reads and persists (theme, color mode, sidebar,
    /// tool grouping); enter cycles the values through the config
    /// sink (cli.json write-through).
    Settings,
    /// Lane FINAL: `DialogFork` (`session.fork` —
    /// routes/session/dialog-fork.tsx): "Full session" + the user
    /// messages (newest first); enter forks at that boundary.
    ForkPicker { session_id: String },
    /// Lane FINAL: `session.export`'s destination choice (the
    /// `DialogExportOptions` rows collapsed to format × destination —
    /// dialog-export-options.tsx): markdown/json × clipboard/file.
    SessionExport { session_id: String },
    /// Lane FINAL: `DialogExportResult` (ui/dialog-export-result.
    /// tsx): the written filepath after a file export.
    ExportResult { path: String },
    /// TUI-8: `DialogSkill` (`/skills`, `prompt.skills` —
    /// `component/dialog-skill.tsx`): the large skill select over the
    /// `skill.list` state; submit inserts `@{skill}` into the focused
    /// composer.
    Skill,
    /// TUI-9: `DialogStash` (`prompt.stash.list` —
    /// `component/dialog-stash.tsx`): the stashed prompts most-recent
    /// first, ctrl+d the two-press delete, submit pops + restores.
    Stash,
    /// TUI-10: `DialogImagePreview` (`prompt.images.view` `<leader>i`
    /// — `component/dialog-image-preview.tsx`): xlarge centered, ←/→
    /// cycle the image list; the payload renders through the blocks
    /// / graphics-protocol pipeline.
    ImagePreview,
    /// TUI-24: `DialogErrorDetails` (`component/
    /// dialog-error-details.tsx`): the generic error drill-down — esc
    /// back, `c` copy details, `i` investigate, ↑/↓ scroll.
    ErrorDetails {
        title: String,
        source: String,
        error: String,
        context: String,
        diagnostic_ref: String,
        /// The plugins dialog's role: escape returns to the plugin
        /// list (v2 `onBack` re-seats the large mount).
        back_to_plugins: bool,
    },
    /// TUI-25: `ui/dialog-alert.tsx` — the generic alert tone:
    /// return/ok confirms; any close settles. `request_id` rides the
    /// plugin host's `ctx.ui.alert` promise; `confirm_command` is the
    /// native consumer's action.
    Alert {
        title: String,
        message: String,
        request_id: Option<u64>,
        confirm_command: Option<String>,
    },
    /// TUI-25: `ui/dialog-confirm.tsx` — the generic confirm tone:
    /// cancel/confirm pills with ←/→ focus, return runs the active
    /// one, esc closes unresolved.
    Confirm {
        title: String,
        message: String,
        confirm_label: String,
        cancel_label: String,
        /// The focused pill (`active` — ←/→ toggle).
        active_confirm: bool,
        request_id: Option<u64>,
        confirm_command: Option<String>,
        cancel_command: Option<String>,
    },
    /// TUI-TD22: `DialogExperiments` (`component/dialog-experiments.
    /// tsx`): the experiment select over the (v2.0.24-empty) table;
    /// ←/→/enter toggle `config.experimental.<id>`.
    Experiments,
    /// TUI-5 (lane-tui2): `DialogOpen` (`component/dialog-open.tsx`)
    /// — the Open menu (`open.menu`, ctrl+o): the Sessions recents +
    /// Projects sections (right-arrow on a project row opens the
    /// [`DialogKind::OpenWorktrees`] view).
    Open,
    /// TUI-5: the Open menu's worktrees view
    /// (`{Project} / Worktrees`, dialog-open.tsx:276-322): the
    /// canonical root + the project's worktree directories; left
    /// returns to [`DialogKind::Open`], ctrl+n seeds the
    /// new-worktree prompt.
    OpenWorktrees { project_id: String, project_name: String },
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
    /// The live-preview contract of `dialog-theme-list.tsx`: the theme
    /// name captured when the `ThemeList` dialog opened (`const initial =
    /// themes.selected`). `Some` while the dialog is open and
    /// UNCONFIRMED — moves/filters preview through it, an unconfirmed
    /// close reverts to it, a submit clears it (`confirmed = true`).
    pub theme_initial: Option<String>,
}

/// TUI-9: one stashed prompt (`prompt/stash.tsx`'s `StashEntry` —
/// the serialized shape is `{"prompt": {...}, "timestamp": ms}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashEntry {
    /// The stashed draft (text + files + agents — the port's
    /// `PromptDraft` is `PromptInfo`'s shape).
    pub prompt: crate::components::composer::PromptDraft,
    /// `Date.now()` at push time (ms since epoch).
    pub timestamp: u64,
}

/// `MAX_STASH_ENTRIES` (prompt/stash.ts:19) — the push-side cap.
pub const MAX_STASH_ENTRIES: usize = 50;

/// TUI-10: one image of the preview dialog (`ImagePreviewItem` —
/// `dialog-image-preview.tsx`'s readonly `{uri, mention?}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePreviewItem {
    /// The data URI (`data:image/png;base64,…`).
    pub uri: String,
    /// The caption (the attachment chip's label / the diff file).
    pub mention: Option<String>,
}

/// TUI-10: the preview dialog's live state (the images + the index).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImagePreviewState {
    pub images: Vec<ImagePreviewItem>,
    pub index: usize,
}

/// TUI-23: the settings snapshot — the boot config's display values
/// for the `DialogConfig` rows the store does not model live (the
/// live rows — theme, sidebar, grouping, … — read their own state).
pub type SettingsSnapshot = std::collections::BTreeMap<String, String>;

/// TUI-TD22: one experiment row (`dialog-experiments.tsx:8-13`).
/// v2.0.24 ships an EMPTY table (`experiments: Experiment[] = []`);
/// the port mirrors the shape so an upstream entry lands as a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Experiment {
    pub id: String,
    pub title: String,
    pub description: String,
}

/// The lane-dialogs dialog-family state (TUI-8/9/10/23/24/25/TD22):
/// the non-reactive halves — the stash (loaded once, mutated through
/// the store methods), the preview images, the plugin fault table,
/// the ctx.ui.alert/confirm resolution queue, the settings snapshot
/// and the error-details scroll offset.
#[derive(Debug, Default)]
pub struct DialogsLaneState {
    /// TUI-9: the prompt stash (oldest first, like the jsonl file).
    pub stash: Vec<StashEntry>,
    /// TUI-9: the armed two-press delete row (the stash INDEX).
    pub stash_delete_armed: Option<usize>,
    /// TUI-9: where the stash persists (`<state>/prompt-stash.jsonl`;
    /// `None` = memory-only — tests and the missing-state boot).
    pub stash_path: Option<std::path::PathBuf>,
    /// TUI-10: the preview dialog's images + index.
    pub image_preview: ImagePreviewState,
    /// TUI-24: the plugins dialog's fault table (plugin id → the
    /// load error string; fed from the plugin host's fault list).
    pub plugin_faults: std::collections::HashMap<String, String>,
    /// TUI-25: the ctx.ui.alert/confirm resolution queue — (guest
    /// request id, the settle value); the plugin pump drains it back
    /// into the host (`resolve_ui_request`).
    pub ui_dialog_resolutions: Vec<(u64, serde_json::Value)>,
    /// TUI-23: the boot config's settings display values.
    pub settings: SettingsSnapshot,
    /// TUI-24: the error-details body's scroll offset.
    pub error_details_scroll: usize,
    /// TUI-TD22: the experiments table (EMPTY in v2.0.24; tests
    /// inject rows to prove the toggle).
    pub experiments: Vec<Experiment>,
    /// TUI-TD22: the `config.experimental` map's local view.
    pub experimental_flags: std::collections::BTreeMap<String, bool>,
    /// TUI-10: the diff viewer's inline-image hit rects from the last
    /// render — `(x, y, width, height, uri)` absolute cells + the
    /// image's data URI (a left release opens the preview,
    /// diff-viewer-image.tsx:51-64).
    pub diff_image_hits: Vec<(usize, usize, usize, usize, String)>,
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

/// The open-url seam's callable (tests observe link clicks).
pub type OpenUrlSink = Rc<dyn Fn(String)>;

pub const CLOSED_TAB_LIMIT: usize = 25;
pub const TAB_HISTORY_LIMIT: usize = 100;
pub const NEW_SESSION_TAB_TITLE: &str = "New session";

/// The interrupt-arm window (prompt/index.tsx:517-519: the 5000 ms
/// reset timer after the first escape).
pub const INTERRUPT_ARM_WINDOW_MS: u64 = 5000;

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
    /// The hovered tab's ✕ cell under the pointer (`closeHovered`,
    /// `session-tabs.tsx:1091-1094` — the mark renders `text.base`
    /// instead of `closeColor` while the pointer sits on it).
    pub hover_close: bool,
    /// The ` + ` affordance under the pointer (`addHovered`,
    /// `session-tabs.tsx:1301`).
    pub add_hovered: bool,
    /// The drag-reorder preview (lane KM, auditA #8b): the dragged
    /// session + the slot index it would land at
    /// (`preview: { sessionID, index }`, session-tabs.tsx:1314-1321)
    /// — the strip renders the PREVIEW order while the drag is live;
    /// the move persists on release.
    pub drag_preview: Option<(String, usize)>,
    /// The ✕ close-hold (lane KM, auditA #8b — `MouseCloseHold`,
    /// session-tabs.tsx:83-91): after a ✕ close the layout FREEZES
    /// (visible ids + widths) so the next tab's ✕ stays under the
    /// pointer for rapid multi-close; expires after
    /// [`MOUSE_CLOSE_HOLD_MS`] or when the pointer leaves the strip.
    pub close_hold: Option<CloseHold>,
}

/// `MOUSE_CLOSE_HOLD_MS` (session-tabs.tsx:80) — 5 s.
pub const MOUSE_CLOSE_HOLD_MS: u64 = 5_000;

/// The frozen close-hold snapshot (`MouseCloseHold`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseHold {
    /// The items list at close time (ALL tabs, in order).
    pub items: Vec<String>,
    /// The VISIBLE window's ids at close time (`layout().tabs`).
    pub ids: Vec<String>,
    /// The visible window's widths at close time.
    pub widths: Vec<usize>,
    /// The closed tab's id.
    pub closed: String,
    /// The neighbor the pointer's ✕ column now addresses.
    pub target: String,
    /// The pointer's strip-local column at close time.
    pub x: usize,
    /// Wall-clock expiry (the store's clock + 5 s).
    pub expires_ms: u64,
}

/// CONFIG-C8: the retention recency window (session-tabs.tsx:144's
/// `limit: 3` — the navigated roots kept beside the open tabs).
const RETENTION_LIMIT: usize = 3;

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
    /// `ModelInfo.limit.context` — the footer usage row's denominator.
    pub context_limit: Option<i64>,
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
    /// TUI-8: the skill inventory (`skill.list` — `DialogSkill`'s
    /// rows; `skill.sync`'s refresh rides the same fetch worker).
    pub skills: ListState<ServerSkill>,
    /// `useConnected()` — any integration has connections
    /// (`hasConnectedProvider` over `integration.list`,
    /// use-connected.tsx). Gates the models dialog's categories +
    /// grouped sort and the palette's suggested
    /// "Connect an integration" row.
    pub connected: bool,
}

/// One skill row (`Skill.Info` — `DialogSkill`'s option: the padded
/// name title, the description beside it, the id as value).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerSkill {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
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

/// The session-list refresh seam (F13): the /sessions dialog's open
/// fires a `session.list` refetch through it (the boot resync snapshot
/// goes stale as sessions are created; v2's `DialogSessionList` syncs
/// on mount). Tests record; production round-trips the loop fetcher.
pub trait SessionListSink {
    fn refresh(&self);
}

/// The no-op sink (no loop fetcher attached).
#[derive(Debug, Default)]
pub struct NoSessionListRefresh;

impl SessionListSink for NoSessionListRefresh {
    fn refresh(&self) {}
}

// -- TUI-4: the session-terminals registry (context/session-terminals) --

/// One listed terminal (`PersistentPty.Info`, the picker row's label
/// source — `foregroundProcess ?? title`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRecord {
    pub id: String,
    pub title: String,
    pub foreground_process: Option<String>,
}

impl TerminalRecord {
    /// The picker row's label (terminals-tab.tsx:105:
    /// `terminal?.foregroundProcess ?? terminal?.title`).
    #[must_use]
    pub fn label(&self) -> &str {
        self.foreground_process.as_deref().unwrap_or(&self.title)
    }
}

/// One session's terminal list (the registry's per-session entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalListed {
    pub session_id: String,
    pub terminals: Vec<TerminalRecord>,
}

// -- TUI-12: the live shell registry (the client data cache's map) --

/// One in-flight shell (`shell.created`'s `info` row — the footer's
/// live segment reads the id + the owning session only, footer.tsx:
/// 27-33). The client cache's per-location map holds ONLY running
/// shells: `shell.exited`/`shell.deleted` remove the row
/// (client/src/solid/data.ts:100-102,1245-1261).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveShell {
    pub id: String,
    /// `metadata.sessionID` — the session that owns the shell.
    pub session_id: Option<String>,
}

/// The terminal pane's IO seam (the run loop's WS attach task): typed
/// input frames, resize controls, and lifecycle requests ride it.
/// Tests record; production pumps the attach socket.
pub trait TerminalSink {
    /// `refresh(sessionID)` — `persistentPty.list` (the registry
    /// reload, session-terminals.tsx:29).
    fn refresh(&self, session_id: &str);
    /// `newTerminal(sessionID)` — `persistentPty.create` + refresh +
    /// select (session-terminals.tsx:83-92).
    fn create(&self, session_id: &str);
    /// `selectTerminal(sessionID, ptyID|null)` — the persisted
    /// selection; `Some` mounts + attaches the pane.
    fn select(&self, session_id: &str, pty_id: Option<&str>);
    /// One encoded input frame for the attached pane.
    fn send_frame(&self, pty_id: &str, frame: &[u8]);
}

/// The no-op sink (no loop fetcher attached).
#[derive(Debug, Default)]
pub struct NoTerminals;

impl TerminalSink for NoTerminals {
    fn refresh(&self, _session_id: &str) {}
    fn create(&self, _session_id: &str) {}
    fn select(&self, _session_id: &str, _pty_id: Option<&str>) {}
    fn send_frame(&self, _pty_id: &str, _frame: &[u8]) {}
}

/// The recording sink (tests assert the dialog open fired a refresh).
#[derive(Debug, Default)]
pub struct RecordingSessionListSink {
    pub calls: std::cell::Cell<usize>,
}

impl SessionListSink for RecordingSessionListSink {
    fn refresh(&self) {
        self.calls.set(self.calls.get() + 1);
    }
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
    /// The editing state keyed by session (Wave A: the text-edit
    /// core's caret/selection/undo stacks survive the round-trip).
    pub edits: std::collections::HashMap<String, oc_tui_core::text_edit_opentui::EditCore>,
    pub history: PromptHistory,
    /// The rotated placeholder pick (lane/chrome #6 — the per-boot
    /// `randomIndex` roll; the pool itself is the route's constant).
    pub placeholder_index: usize,
    /// TUI-14 — the shell-mode flag keyed by composer session (v2's
    /// per-session prompt store `mode`; the composer engine
    /// round-trips it through [`Store::composer`]).
    pub modes: std::collections::HashMap<String, bool>,
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
    /// `config.tabs.layout` ("horizontal" | "vertical", default
    /// horizontal — config/index.tsx:330's `input.tabs?.layout`).
    /// TUI-3: the vertical rail consumes it through
    /// [`Store::vertical_rail_width`] (app.tsx:575's `tabsVertical`).
    tabs_layout: Signal<crate::components::session_tabs_rail::TabsLayout>,
    /// The vertical rail's persisted width (`useStorage().store
    /// ("layout").verticalTabsWidth`, app.tsx:503-517 — default
    /// `SESSION_SIDEBAR_WIDTH`, clamped per resize by
    /// [`crate::components::session_tabs_rail::clamp_session_tabs_
    /// width`]).
    vertical_tabs_width: Signal<usize>,
    /// Per-session cost totals (`session.usage.updated` events —
    /// client/solid/data.ts:646-652 writes `info[id].cost`). TUI-13:
    /// [`Store::session_cost`] is the sidebar/footer read (the
    /// family-summed `data.session.cost`, data.ts:1369-1379).
    session_costs: Signal<HashMap<String, f64>>,
    /// The session HISTORY from the resync's `session.list` — the
    /// session-list dialog's data source (v2 lists history there,
    /// never the tab strip).
    session_history: Signal<Vec<crate::data::SessionSnapshot>>,
    /// TUI-4 — `sessions.available()` (session-terminals.tsx:59-76):
    /// the server's `persistentPty` capability; the toggle trio is a
    /// no-op while false.
    terminal_available: Signal<bool>,
    /// TUI-4 — the per-session terminal registry
    /// (`terminals[sessionID]`, session-terminals.tsx:23).
    terminal_registry: Signal<Vec<TerminalListed>>,
    /// TUI-12 — the live shell registry (the client data cache's
    /// per-location `shell` map: only in-flight shells list). The
    /// composer footer's live segment counts the session's rows
    /// (footer.tsx:27-33).
    shells: Signal<Vec<LiveShell>>,
    /// TUI-4 — the selected terminal (`(sessionID, ptyID)`; the
    /// right pane mounts it, session-frame.tsx:118-122).
    terminal_selection: Signal<Option<(String, String)>>,
    /// TUI-4 — the ACTIVE pane (`activePane === "right"`): keys type
    /// into the embedded terminal while true
    /// (session-frame.tsx:72/171).
    terminal_focus_right: Signal<bool>,
    /// TUI-4 — the pane grids' change bump (the VT states live in
    /// [`Store::terminal_panes`], outside the signal graph).
    terminal_generation: Signal<u64>,
    /// TUI-4 — the live VT states by pty id (non-Clone emulators;
    /// interior-mutable, redraw-bumped through `terminal_generation`).
    terminal_panes: std::cell::RefCell<
        std::collections::HashMap<String, crate::terminal_pane::TerminalPaneState>,
    >,
    /// TUI-4 — the run loop's terminal IO seam.
    terminal_sink: std::cell::RefCell<Rc<dyn TerminalSink>>,
    /// TUI-4 — the attaches already requested (the self-healing
    /// mount's storm guard: one attach per selection until the
    /// snapshot lands).
    terminal_pending_attach: std::cell::RefCell<std::collections::HashSet<String>>,
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
    /// TUI-3: the compact rail's `⌕` search button hover lift
    /// (session-tabs-rail.tsx:24-36 — the pointer layer sets it).
    rail_search_hovered: Cell<bool>,
    /// TUI-3: the rail's resize-handle highlight (`hovered() ||
    /// resizing()`, pane-resize-handle.tsx:21-27).
    rail_handle_hot: Cell<bool>,
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
    /// The open plugin route page (PLUG-TUI-SLOT's router surface);
    /// `None` = no plugin page open.
    plugin_route: Signal<Option<PluginRoutePage>>,
    /// `session.sidebar` — the config mode (auto/hide) after the
    /// boot merge (cli.json, then the last toggle's persisted value).
    sidebar_mode: Signal<crate::components::sidebar::SidebarMode>,
    /// The manual open flag (v2's `sidebarOpen` signal,
    /// session-frame.tsx:68 — ephemeral per mount, never persisted).
    sidebar_open: Signal<bool>,
    /// The terminal size (the toggle's visibility input — v2's
    /// `rightPane()` reads the live dimensions; the render path takes
    /// its own `size` argument).
    terminal_size: Signal<(u16, u16)>,
    /// The onboarding card's dismissal (storage `getting-started`).
    sidebar_onboarding: Signal<bool>,
    permission_autoaccept: Signal<bool>,
    /// `config.session.grouping !== "none"` (index.tsx:233 — lane KM):
    /// the exploration-grouping toggle behind
    /// `session.toggle.exploration_grouping`.
    exploration_grouping: Signal<bool>,
    /// CONFIG-C8 (context/session-retention.ts): the recently navigated
    /// session ROOTS (newest first) — the recency window beside the
    /// open tabs that keeps `by_session` bounded.
    retention_recent: Signal<Vec<String>>,
    /// `session.thinking === "show"` (index.tsx:229,229-241 —
    /// CONFIG-T3): reasoning renders inline instead of the collapsed
    /// disclosure (default hide).
    session_thinking: Signal<bool>,
    /// `session.verbosity` (index.tsx:234): the grouping/expansion
    /// input (low wraps activity runs, high opens summaries).
    session_verbosity: Signal<String>,
    /// `session.tps` (index.tsx:1892): the assistant footer's tok/s
    /// span (default off).
    session_tps: Signal<bool>,
    /// `session.markdown === "source"` (index.tsx:231): raw text parts
    /// instead of rendered markdown (default rendered).
    session_markdown_source: Signal<bool>,
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
    /// The session-list refresh seam (F13 — the /sessions dialog's
    /// open-time refetch).
    session_list_sink: std::cell::RefCell<Rc<dyn SessionListSink>>,
    /// The configured default model (`config.model` — the selection
    /// chain's step 3; boot + reload set it, see [`Store::resolve_model`]).
    config_model: std::cell::RefCell<Option<ModelSelection>>,
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
    /// Per-session COLLAPSED thought rows (`"{message_id}:{part_id}"`
    /// keys — the minimal-mode thought disclosure, message-parts.tsx:
    /// 66-86; absent ⇒ expanded, the default render).
    collapsed_thoughts: std::cell::RefCell<HashMap<String, std::collections::HashSet<String>>>,
    /// The last frame's clickable transcript rows ([`render_session`]
    /// writes them like `frame_images`): the reasoning-group headers
    /// whose absolute `[y, y+height)` rect a mouse release can toggle.
    transcript_hit_rows: std::cell::RefCell<Vec<TranscriptHitRow>>,
    /// The top dialog's frame rect from the last render (dismissal).
    dialog_frame_rect: std::cell::RefCell<Option<(usize, usize, usize, usize)>>,
    /// The top dialog's visible option rows (flat index → screen row).
    dialog_option_rows: std::cell::RefCell<Vec<Option<usize>>>,
    /// The footer location label's hit rect + directory from the last
    /// render (None when the label didn't paint — empty location).
    footer_location_hit: std::cell::RefCell<Option<FooterLocationHit>>,
    /// TUI-12 — the footer live segment's hit rect from the last
    /// render (`None` when the segment didn't paint).
    footer_live_hit: std::cell::RefCell<Option<FooterLiveHit>>,
    /// The home footer's chip rects (lane KM — cleared when the
    /// footer hides or the route leaves home).
    home_footer_chip_hits: std::cell::RefCell<Option<FooterChipHits>>,
    /// The sidebar onboarding card's pointer targets (the render
    /// pass's absolute rects; `None` while the card is hidden).
    sidebar_onboarding_hits: std::cell::RefCell<Option<SidebarOnboardingHits>>,
    /// The MCP section's pointer targets (SURF-1).
    sidebar_mcp_hits: std::cell::RefCell<SidebarMcpHits>,
    picker_entry_hits: std::cell::RefCell<PickerEntryHits>,
    plugin_section_hits: std::cell::RefCell<PluginSectionHits>,
    plugin_click_sink: std::cell::RefCell<Option<PluginSectionClickSink>>,
    /// The session sidebar's wheel-scroll state (auditA #1): the
    /// content-slot offset plus the geometry the last render saw
    /// (content rows, viewport rows, absolute x extent). v2's
    /// `sidebar.content` is a scrollbox (`feature-plugins/sidebar`
    /// mounts it in one); the port's pane is one flat walk, so the
    /// wheel drives this offset and the render re-slices.
    sidebar_scroll: std::cell::RefCell<SidebarScrollState>,
    /// The permission body's pointer targets (auditA #4) at
    /// ABSOLUTE screen cells — recorded by the render (`App::frame`,
    /// the body's mount), cleared when the ask resolves.
    permission_pointer_hits:
        std::cell::RefCell<Option<crate::components::footer::permission::PermissionPointerHits>>,
    /// The top dialog's chrome buttons (auditA #6: the header close
    /// label + the Help `ok` pill) at absolute cells — recorded by
    /// the overlay render alongside `dialog_frame_rect`, cleared with
    /// it.
    dialog_button_hits: std::cell::RefCell<Vec<DialogButtonHit>>,
    /// The lane-dialogs dialog-family state (TUI-8/9/10/23/24/25/
    /// TD22): stash, image preview, plugin faults, ctx.ui
    /// resolutions, settings snapshot, error scroll, experiments.
    lane_dialogs: std::cell::RefCell<DialogsLaneState>,
    /// The location label's hover state (`hovered()`).
    location_hovered: std::cell::Cell<bool>,
    /// TUI-12 — the live segment's hover state (`liveHovered`).
    live_hovered: std::cell::Cell<bool>,
    /// TUI-15 — the Zed editor selection (context/editor.ts's store:
    /// the poll's latest decode; `None` while none resolved).
    editor_selection: std::cell::RefCell<Option<crate::editor_zed::ZedSelection>>,
    /// `selectionSent` — the submit that attached the context.
    editor_selection_sent: std::cell::Cell<bool>,
    /// `dismissedEditorSelectionKey` (index.tsx:224-328): the clear
    /// command's key — the SAME selection stays dismissed until Zed
    /// reports a different one.
    editor_dismissed_key: std::cell::RefCell<Option<String>>,
    /// Per-session scroll state (Wave A's scroll driver over
    /// [`FollowState`]); the render pass records the metrics.
    scroll: std::cell::RefCell<HashMap<String, ScrollDriver>>,
    transcript_metrics: std::cell::RefCell<HashMap<String, TranscriptMetrics>>,
    /// The interrupt-arm state + the model ring — reactive so the
    /// dock/hints re-render.
    interrupts: Signal<HashMap<String, InterruptArm>>,
    model_ring: Signal<ModelRing>,
    /// TUI-TD07 — the server inbox mirror (`store.session.pending`,
    /// client/solid/data.ts:117): user-type items keyed by inbox id;
    /// the queued prompts + pending-steer markers derive from it.
    pending_inbox: Signal<HashMap<String, Vec<InboxItem>>>,
    /// TUI-TD07 — the inbox mutation API seam
    /// (`session.inbox.update` PATCH / `session.inbox.cancel` DELETE).
    inbox_sink: std::cell::RefCell<Rc<dyn InboxSink>>,
    /// TUI-TD05 — the older-history load seam (`loadMore`).
    transcript_sink: std::cell::RefCell<Rc<dyn TranscriptSink>>,
    /// TUI-TD05 — per-session `messageCursor` (`cursor.next` after
    /// the tail page; `Some` ⇒ `more()`; absent ⇒ never paged).
    message_cursors: std::cell::RefCell<HashMap<String, Option<String>>>,
    /// TUI-TD05 — the per-session mount window pins
    /// (`hiddenRows()` / `visibleRowsEnd()`).
    mount_windows:
        std::cell::RefCell<HashMap<String, crate::components::message::list::MountWindow>>,
    /// TUI-TD02 — the transcript's per-message row geometry from the
    /// last render pass (`anchors.messagePositions()`' input).
    message_positions: std::cell::RefCell<HashMap<String, Vec<MessagePosition>>>,
    /// TUI-TD02 — the navigation target (`navigationMessage()`) per
    /// session: the message the nav commands last aligned to.
    navigation_message: Signal<HashMap<String, String>>,
    /// TUI-TD02 — the pending navigation intent (the two-phase
    /// `ensureAllRows` walk: full mount first, resolve after the next
    /// render records fresh positions).
    pending_navigation: std::cell::RefCell<HashMap<String, PendingNavigation>>,
    /// The composer's subagent picker (#44): v2's `composer` store
    /// (routes/session/index.tsx:217-220) + the Subagents tab's
    /// memory-scoped filter and selection (subagents-tab.tsx:35-37).
    picker: Signal<PickerState>,
    /// The interrupt API seam.
    interrupt_sink: std::cell::RefCell<Rc<dyn InterruptSink>>,
    /// The model-ring persistence flag (the run loop drains it).
    model_ring_dirty: Cell<bool>,
    /// #4: the footer's `{directory}:{branch}` location label — the
    /// launch cwd's vcs info (`prompt.footer.status`'s default,
    /// `prompt/index.tsx:1605-1619`); the app's location worker sets
    /// it once per boot (a `/cd` or session move would refresh it).
    footer_location: std::cell::RefCell<String>,
    /// TUI-30 — `currentLocation.error?.location.directory`
    /// (context/location.tsx): the directory that FAILED to resolve
    /// (the location worker's error state). A routed session whose
    /// `location.directory` matches mounts the
    /// `SessionLocationMissing` recovery screen (routes/session/
    /// index.tsx:1484-1496).
    location_error: std::cell::RefCell<Option<String>>,
    /// NEW-S1a: the live caret's screen cell for the TERMINAL cursor
    /// (`surface.set_cursor` at the draw seam — v2's caret is the
    /// hardware cursor, never a cell-painted block). Set during the
    /// route render; `None` hides it.
    caret_cell: Cell<Option<(u16, u16)>>,
    /// SURF-2: when the connection state last changed (the
    /// Reconnecting overlay's threshold anchor).
    connection_since: Cell<u64>,
    /// SURF-2: the `StartupLoading` pill's state machine.
    startup_state: RefCell<StartupLoadingState>,
    /// SURF-1 revert state (`session.revert.*` — the
    /// `RevertMessage` banner's source, index.tsx:1413-1418):
    /// per-session boundary + file list; `cleared`/`committed`
    /// remove it.
    reverts: Signal<HashMap<String, RevertState>>,
    /// The revert-clear API seam (`client.api.session.revert.clear`,
    /// index.tsx:2105-2111 — the banner's click). Tests record calls.
    revert_sink: std::cell::RefCell<Rc<dyn RevertSink>>,
    /// Lane FINAL: the integration connect state (`DialogIntegration`
    /// over `integration.list`).
    integration_list: Signal<IntegrationListState>,
    /// TUI-5 (lane-tui2): the Open menu's project registry
    /// (`project.list`) — the Projects section + the session footers'
    /// project labels.
    project_list: Signal<ListState<crate::components::dialog_open::ProjectRow>>,
    /// The project-list API seam.
    project_sink: std::cell::RefCell<Rc<dyn ProjectSink>>,
    /// The live oauth/command attempt (code entry + the pending poll
    /// view) — `None` when no attempt is open.
    integration_attempt: Signal<Option<IntegrationAttempt>>,
    /// The inline error line for the key/code prompts
    /// (`setError(errorMessage)`).
    integration_error: Signal<Option<String>>,
    /// The integration API seam.
    integration_sink: std::cell::RefCell<Rc<dyn IntegrationSink>>,
    /// The accounts view's two-press delete (`deleting` signal,
    /// dialog-integration.tsx:161).
    integration_deleting: Signal<Option<String>>,
    /// Lane FINAL: the sessions dialog's two-press delete (`toDelete`,
    /// dialog-session-list.tsx:161).
    session_delete_armed: Signal<Option<String>>,
    /// Lane FINAL: the session-action API seam (compact/export/
    /// remove/background/fork/revert-stage/reload).
    session_action_sink: std::cell::RefCell<Rc<dyn SessionActionSink>>,
    /// Pending compaction inbox ids (`pending.list` items of type
    /// `compaction` — the `◇ Compaction queued` divider rows,
    /// rows.ts:72-79). `session.compaction.started` with a matching
    /// `inputID` removes one (data.ts:1013).
    pending_compactions: Signal<HashMap<String, Vec<String>>>,
    /// SURF-1 history loading: the transcript fetch for a session is
    /// in flight — the jump row shows "Loading session history…"
    /// (index.tsx:1425-1427, v2's `firstJump()` hold).
    history_loading: Signal<HashMap<String, bool>>,
    /// SURF-1 tool detail disclosure (`InlineTool`/`ShellDisplay`'s
    /// inline expand): expanded `{mid}:{pid}` keys.
    expanded_tools: Signal<std::collections::HashSet<String>>,
    /// SURF-1 MCP server list (`location.mcp.server.list` — the
    /// sidebar's MCP section + `DialogStatus` source). Fetched by the
    /// server-lists worker; statuses drive the per-server dot.
    mcp_servers: Signal<Vec<McpServerRow>>,
    /// Lane KM (auditA #10): the plugins-failed count (the boot
    /// resolution's failures — the home footer chip's source).
    plugins_failed: Signal<usize>,
    /// app.tsx:521's `mcpAlerted` record (the toast-once tracker).
    mcp_alerted: std::cell::RefCell<Option<std::collections::HashMap<String, String>>>,
    /// The MCP section's collapse state (mcp.tsx's `view.open`).
    sidebar_mcp_open: Signal<bool>,
    /// SURF-2 the diff viewer's live state (the `diff` plugin route —
    /// feature-plugins/system/diff-viewer.tsx): mounted fullscreen
    /// while `open`, own key layer, `diff.close` returns to
    /// `return_route`.
    diff_viewer: Signal<DiffViewerLive>,
    /// The diff fetch seam (`vcs.base`/`vcs.diff` through the loop
    /// fetcher; tests record).
    diff_sink: std::cell::RefCell<Rc<dyn DiffSink>>,
    /// The open-url seam (lane KM — tests observe link clicks).
    open_url_sink: std::cell::RefCell<Option<OpenUrlSink>>,
    /// SURF-2 `DialogPair`'s resource state (server.info + pair).
    pair: Signal<PairState>,
    /// Rename-pending per session (session → armed-at clock ms) — the
    /// `title_shimmer` envelope's input (`StoreSignals::title_pending`).
    title_pending: Signal<HashMap<String, u64>>,
    /// The update-notification state (auditB #10 — v2's
    /// `update-notification.tsx`): the banner's notice + the dialog's
    /// live state.
    update_notice: Signal<Option<crate::components::update::UpdateNotice>>,
    update_dialog: Signal<crate::components::update::UpdateDialogState>,
    /// The dismissed `source:version` keys (`update-notifications` —
    /// loaded/saved by the run loop's storage handle).
    update_dismissed: RefCell<Vec<String>>,
    /// Per-tab pulse transition stamps (the strip's animated phases —
    /// `TabPulseTrack::sync` at status flips; the render derives every
    /// envelope level from these + the clock, stateless per frame).
    tab_pulses: RefCell<HashMap<String, crate::components::tab_pulse::TabPulseTrack>>,
    /// SURF-2 the MCP toggle seam (mcp.connect/disconnect).
    mcp_toggle_sink: std::cell::RefCell<Rc<dyn McpToggleSink>>,
    /// SURF-2 `DialogPair`'s fetch seam (server.info + server.pair).
    pair_refresh_sink: std::cell::RefCell<Option<Rc<dyn PairRefreshSink>>>,
    /// SURF-2 the config persistence sink (cli.json write-through).
    config_sink: std::cell::RefCell<Rc<dyn ConfigSink>>,
    /// The configured theme-mode pin (dark/light/system).
    theme_mode_pref: RefCell<Option<String>>,
    /// The terminal probe's system mode.
    system_mode: Cell<oc_theme::types::Mode>,
}

/// One staged revert (`session.revert.staged`'s `revert` payload).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RevertState {
    /// `revert.messageID` — messages at/after it are hidden (the
    /// transcript filter, rows.ts:63-66).
    pub message_id: String,
    /// `revert.files` — the banner's file rows (empty → no list).
    pub files: Vec<RevertFile>,
}

/// One reverted file row (`statusLabel` + path + `+A/−D`,
/// index.tsx:2131-2148).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RevertFile {
    pub file: String,
    /// `added` | `modified` | `deleted`.
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
}

/// The `session.revert.clear` API seam (the banner's dismiss click).
pub trait RevertSink {
    fn clear(&self, session_id: &str);
}

/// The recording sink (tests + the default no-op).
#[derive(Default)]
pub struct RecordingRevertSink {
    pub calls: std::cell::RefCell<Vec<String>>,
}

impl RevertSink for RecordingRevertSink {
    fn clear(&self, session_id: &str) {
        self.calls.borrow_mut().push(session_id.to_owned());
    }
}

// -- Lane FINAL: the integration connect surface (DialogIntegration,
// component/dialog-integration.tsx) over the BE-3 host ---------------

/// One non-env connect method (`connectMethods`,
/// dialog-integration.tsx:58-62 — env methods filtered, key first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationMethod {
    pub id: String,
    /// `key` | `oauth` | `command`.
    pub kind: &'static str,
    pub label: String,
}

/// One stored credential connection (`credentialConnections`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationAccount {
    pub id: String,
    pub label: String,
    /// `connections[0].status` — "Sign in required →".
    pub sign_in_required: bool,
}

/// One `integration.list` row, pre-shaped for the dialog
/// (`integrationOptions`'s sort + category derivation,
/// dialog-integration.tsx:30-56,102-128).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRow {
    pub id: String,
    pub name: String,
    /// `Popular` | `Services` | `MCP`.
    pub category: &'static str,
    pub methods: Vec<IntegrationMethod>,
    pub accounts: Vec<IntegrationAccount>,
    /// `connectionSummary` — the connected-credentials footer.
    pub summary: String,
    /// `connections[0]?.status` — the warning footer.
    pub sign_in_required: bool,
}

/// The integration list's fetch state (`createResource` over
/// `data.location.integration.list`).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum IntegrationListState {
    #[default]
    Loading,
    Loaded(Vec<IntegrationRow>),
    Failed(String),
}

/// One live OAuth/command attempt (`IntegrationAttempt` —
/// oauth/command connect's `data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationAttempt {
    pub integration_id: String,
    pub attempt_id: String,
    /// `oauth` | `command` (the cancel route + poll family).
    pub kind: &'static str,
    /// `mode === "code"` only for oauth: the code-entry dialog;
    /// auto-browser and command flows use the pending view.
    pub code_mode: bool,
    pub url: String,
    pub instructions: String,
    pub title: String,
}

/// The integration API seam (BE-3's routes — `integration.list`,
/// `integration.connect.key`, `integration.oauth.*`,
/// `integration.command.*`, `credential.activate/remove`): the dialog
/// actions run through it; tests record, production round-trips the
/// loop fetcher.
pub trait IntegrationSink {
    fn list(&self);
    fn connect_key(&self, integration_id: &str, key: &str);
    fn oauth_connect(&self, integration_id: &str, method_id: &str);
    fn oauth_complete(&self, integration_id: &str, attempt_id: &str, code: &str);
    fn command_connect(&self, integration_id: &str, method_id: &str);
    /// The esc path while an attempt is live (`onCleanup`'s cancel).
    fn cancel(&self, integration_id: &str, attempt_id: &str, kind: &'static str);
    fn credential_activate(&self, credential_id: &str);
    fn credential_remove(&self, credential_id: &str);
}

/// The no-op sink (no loop fetcher attached).
#[derive(Debug, Default)]
pub struct NoIntegrationSink;

impl IntegrationSink for NoIntegrationSink {
    fn list(&self) {}
    fn connect_key(&self, _integration_id: &str, _key: &str) {}
    fn oauth_connect(&self, _integration_id: &str, _method_id: &str) {}
    fn oauth_complete(&self, _integration_id: &str, _attempt_id: &str, _code: &str) {}
    fn command_connect(&self, _integration_id: &str, _method_id: &str) {}
    fn cancel(&self, _integration_id: &str, _attempt_id: &str, _kind: &'static str) {}
    fn credential_activate(&self, _credential_id: &str) {}
    fn credential_remove(&self, _credential_id: &str) {}
}

/// One recorded [`IntegrationSink`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationCall {
    List,
    ConnectKey { integration_id: String, key: String },
    OauthConnect { integration_id: String, method_id: String },
    OauthComplete { integration_id: String, attempt_id: String, code: String },
    CommandConnect { integration_id: String, method_id: String },
    Cancel { integration_id: String, attempt_id: String, kind: &'static str },
    Activate(String),
    Remove(String),
}

/// The recording sink (tests assert the dialog drove the API).
#[derive(Default)]
pub struct RecordingIntegrationSink {
    pub calls: std::cell::RefCell<Vec<IntegrationCall>>,
}

impl IntegrationSink for RecordingIntegrationSink {
    fn list(&self) {
        self.calls.borrow_mut().push(IntegrationCall::List);
    }
    fn connect_key(&self, integration_id: &str, key: &str) {
        self.calls.borrow_mut().push(IntegrationCall::ConnectKey {
            integration_id: integration_id.to_owned(),
            key: key.to_owned(),
        });
    }
    fn oauth_connect(&self, integration_id: &str, method_id: &str) {
        self.calls.borrow_mut().push(IntegrationCall::OauthConnect {
            integration_id: integration_id.to_owned(),
            method_id: method_id.to_owned(),
        });
    }
    fn oauth_complete(&self, integration_id: &str, attempt_id: &str, code: &str) {
        self.calls.borrow_mut().push(IntegrationCall::OauthComplete {
            integration_id: integration_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            code: code.to_owned(),
        });
    }
    fn command_connect(&self, integration_id: &str, method_id: &str) {
        self.calls.borrow_mut().push(IntegrationCall::CommandConnect {
            integration_id: integration_id.to_owned(),
            method_id: method_id.to_owned(),
        });
    }
    fn cancel(&self, integration_id: &str, attempt_id: &str, kind: &'static str) {
        self.calls.borrow_mut().push(IntegrationCall::Cancel {
            integration_id: integration_id.to_owned(),
            attempt_id: attempt_id.to_owned(),
            kind,
        });
    }
    fn credential_activate(&self, credential_id: &str) {
        self.calls.borrow_mut().push(IntegrationCall::Activate(credential_id.to_owned()));
    }
    fn credential_remove(&self, credential_id: &str) {
        self.calls.borrow_mut().push(IntegrationCall::Remove(credential_id.to_owned()));
    }
}

// -- TUI-5 (lane-tui2): the Open menu's project seam --------------------------

/// The project-list API seam (v2's `data.project.sync()` over
/// `project.list`, dialog-open.tsx:81-86): the Open menu's mount
/// fetch. Tests record; production round-trips the loop fetcher.
pub trait ProjectSink {
    fn list(&self);
}

/// The no-op sink (no loop fetcher attached).
#[derive(Debug, Default)]
pub struct NoProjectSink;

impl ProjectSink for NoProjectSink {
    fn list(&self) {}
}

/// One recorded [`ProjectSink`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectCall {
    List,
}

/// The recording sink (tests assert the dialog drove the API).
#[derive(Default)]
pub struct RecordingProjectSink {
    pub calls: std::cell::RefCell<Vec<ProjectCall>>,
}

impl ProjectSink for RecordingProjectSink {
    fn list(&self) {
        self.calls.borrow_mut().push(ProjectCall::List);
    }
}

// -- Lane FINAL: the dead session-command seam (compact / export /
// remove / background / fork / undo's revert.stage) + location.reload -

/// The session-action API seam (v2's `client.api.*` calls behind the
/// session command table, routes/session/index.tsx:945-1254 +
/// app.tsx:1019-1034): dispatch arms run through it; tests record,
/// production round-trips the loop fetcher.
pub trait SessionActionSink {
    /// `session.compact` (the current model rides the body).
    fn compact(&self, session_id: &str, model: Option<&ModelSelection>);
    /// `session.export` (sanitize only for the json copy path; the
    /// format/destination ride along for the result delivery).
    fn export(&self, session_id: &str, sanitize: bool, format: &str, destination: &str);
    /// `session.remove` (the sessions dialog's two-press delete).
    fn remove(&self, session_id: &str);
    /// `session.background`.
    fn background(&self, session_id: &str);
    /// `session.fork` (`before` = the picked user message).
    fn fork(&self, session_id: &str, before: Option<&str>);
    /// `session.undo`'s server chain — interrupt, wait, then
    /// `session.revert.stage` (index.tsx:985-992).
    fn revert_stage(&self, session_id: &str, message_id: &str);
    /// `location.reload` (app.tsx:1026-1031).
    fn location_reload(&self);
}

/// One recorded [`SessionActionSink`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionActionCall {
    Compact { session_id: String, model: Option<ModelSelection> },
    Export { session_id: String, sanitize: bool, format: String, destination: String },
    Remove(String),
    Background(String),
    Fork { session_id: String, before: Option<String> },
    RevertStage { session_id: String, message_id: String },
    LocationReload,
}

/// The recording sink (tests + the default).
#[derive(Default)]
pub struct RecordingSessionActionSink {
    pub calls: std::cell::RefCell<Vec<SessionActionCall>>,
}

impl SessionActionSink for RecordingSessionActionSink {
    fn compact(&self, session_id: &str, model: Option<&ModelSelection>) {
        self.calls.borrow_mut().push(SessionActionCall::Compact {
            session_id: session_id.to_owned(),
            model: model.cloned(),
        });
    }
    fn export(&self, session_id: &str, sanitize: bool, format: &str, destination: &str) {
        self.calls.borrow_mut().push(SessionActionCall::Export {
            session_id: session_id.to_owned(),
            sanitize,
            format: format.to_owned(),
            destination: destination.to_owned(),
        });
    }
    fn remove(&self, session_id: &str) {
        self.calls.borrow_mut().push(SessionActionCall::Remove(session_id.to_owned()));
    }
    fn background(&self, session_id: &str) {
        self.calls.borrow_mut().push(SessionActionCall::Background(session_id.to_owned()));
    }
    fn fork(&self, session_id: &str, before: Option<&str>) {
        self.calls.borrow_mut().push(SessionActionCall::Fork {
            session_id: session_id.to_owned(),
            before: before.map(str::to_owned),
        });
    }
    fn revert_stage(&self, session_id: &str, message_id: &str) {
        self.calls.borrow_mut().push(SessionActionCall::RevertStage {
            session_id: session_id.to_owned(),
            message_id: message_id.to_owned(),
        });
    }
    fn location_reload(&self) {
        self.calls.borrow_mut().push(SessionActionCall::LocationReload);
    }
}

/// One MCP server row for the sidebar's MCP section
/// (`location.mcp.server.list` — mcp.tsx:13-18).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpServerRow {
    pub name: String,
    /// `connected`, `pending`, `failed`, `disabled` or `needs_auth`.
    pub status: &'static str,
    /// The failed/`needs_auth` error text (`DialogMcp`'s details view).
    pub error: Option<String>,
}

/// The open plugin route page (PLUG-TUI-SLOT's router surface): the
/// plugin-registered route the router navigated to
/// (`ctx.ui.router.navigate({type:"plugin", name})`), with the
/// materialized content (the guest's render ran through the host's
/// bounded `route.render`; `rows` empty until it lands).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRoutePage {
    pub plugin_id: String,
    /// The registered route name (`stats`).
    pub name: String,
    /// The navigation data (`router.navigate({..., data})`).
    pub data: Option<serde_json::Value>,
    /// The materialized page title (the route name until content
    /// lands).
    pub title: String,
    /// The materialized rows.
    pub rows: Vec<String>,
    /// TUI-TD28 — the host has no registered route `{plugin_id}/
    /// {name}` (v2's `plugins.route(id, name)` undefined → the
    /// `PluginRouteMissing` fallback screen renders instead of a
    /// page).
    pub missing: bool,
}

/// The diff viewer's source (`DIFF_SOURCES`, diff-viewer.tsx:73-78):
/// branch (All), committed (Committed), working (Uncommitted), turn
/// (Last turn — only reachable from a session route).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffSource {
    /// "All" — branch + local changes.
    #[default]
    Branch,
    /// "Committed" — branch commits only (needs a base).
    Committed,
    /// "Uncommitted" — local changes only.
    Working,
    /// "Last turn" — the latest session turn.
    Turn,
}

impl DiffSource {
    /// `DIFF_SOURCES[mode].label` (the toolbar's bold word).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Branch => "All",
            Self::Committed => "Committed",
            Self::Working => "Uncommitted",
            Self::Turn => "Last turn",
        }
    }

    /// `DIFF_SOURCES[mode].description` (the source dialog's row copy).
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Branch => "Branch + local changes",
            Self::Committed => "Branch commits only",
            Self::Working => "Local changes only",
            Self::Turn => "Latest session turn",
        }
    }

    /// Parse the wire/config spelling (`diffs.source`, `vcs.diff`'s
    /// `mode`); unknown values degrade to branch.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "committed" => Self::Committed,
            "working" => Self::Working,
            "turn" => Self::Turn,
            _ => Self::Branch,
        }
    }

    /// The `vcs.diff` mode (`Vcs.Mode` — turn maps onto branch there).
    #[must_use]
    pub const fn vcs_mode(self) -> &'static str {
        match self {
            Self::Branch | Self::Turn => "branch",
            Self::Committed => "committed",
            Self::Working => "working",
        }
    }
}

/// The viewer's fetch phase — v2's `createResource` states plus the
/// `unavailable` arm (committed without base metadata,
/// diff-viewer.tsx:861-867).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DiffPhase {
    #[default]
    Loading,
    /// The resource errored (base or diff fetch).
    Failed(String),
    /// `committed && !base` — comparison unavailable.
    Unavailable,
    /// Loaded file list (empty = the "No changes to show" body).
    Loaded(Vec<crate::components::diff_viewer::DiffFile>),
}

/// The diff viewer's full live state (`DiffViewer`'s signals +
/// `DiffViewerContent`'s preferences, diff-viewer.tsx:84-200).
#[derive(Debug, Clone, PartialEq)]
pub struct DiffViewerLive {
    pub open: bool,
    /// The plugin route's `returnRoute` (diff-viewer.tsx:1206-1216) —
    /// `diff.close` navigates back here.
    pub return_route: Route,
    /// The route session (`sessionID` — enables the "turn" source).
    pub session_id: Option<String>,
    /// `memory.source` — the sticky source between opens
    /// (`props.context.storage.memory("review")`).
    pub mode: DiffSource,
    /// The fetch state machine.
    pub phase: DiffPhase,
    /// The resolved base's display name (`sourceDetail`'s `vs {name}`).
    pub base: Option<String>,
    /// The interactive preferences (tree/view/single/selection/reviewed).
    pub viewer: crate::components::diff_viewer::DiffViewerState,
    /// The patch column's scroll offset (`scroll.scrollTop`).
    pub scroll: usize,
    /// The fetch generation — results from an older request than the
    /// current are dropped (v2's `input !== diffInput()` re-check).
    pub generation: u64,
}

impl Default for DiffViewerLive {
    fn default() -> Self {
        Self {
            open: false,
            return_route: Route::Home,
            session_id: None,
            mode: DiffSource::default(),
            phase: DiffPhase::default(),
            base: None,
            viewer: crate::components::diff_viewer::DiffViewerState::default(),
            scroll: 0,
            generation: 0,
        }
    }
}

impl DiffViewerLive {
    /// `sourceDetail` (diff-viewer.tsx:182-190).
    #[must_use]
    pub fn source_detail(&self) -> String {
        if self.mode == DiffSource::Turn {
            return if matches!(self.phase, DiffPhase::Failed(_)) {
                "Diff unavailable".to_owned()
            } else {
                String::new()
            };
        }
        if self.mode == DiffSource::Working {
            return "vs HEAD".to_owned();
        }
        if matches!(self.phase, DiffPhase::Failed(_)) {
            return "Base or diff unavailable".to_owned();
        }
        if matches!(self.phase, DiffPhase::Loading) {
            return "Resolving diff…".to_owned();
        }
        match &self.base {
            Some(base) => format!("vs {base}"),
            None => "Base not reported".to_owned(),
        }
    }

    /// The visible files (`visiblePatchFiles`, diff-viewer.tsx:466-476):
    /// single-patch shows only the selected file.
    #[must_use]
    pub fn visible_files(&self) -> &[crate::components::diff_viewer::DiffFile] {
        let files: &[crate::components::diff_viewer::DiffFile] = match &self.phase {
            DiffPhase::Loaded(files) => files,
            _ => &[],
        };
        if self.viewer.single_patch {
            if let Some(index) = self.viewer.selected_file {
                return files.get(index).map_or(
                    &[] as &[crate::components::diff_viewer::DiffFile],
                    std::slice::from_ref,
                );
            }
            return &[];
        }
        files
    }
}

/// The diff fetch seam (`client.api.vcs.base/vcs.diff`,
/// diff-viewer.tsx:144-168): the run loop wires the HTTP worker;
/// tests record.
pub trait DiffSink {
    /// Fetch `mode` (with the viewer's session for `turn`).
    fn fetch(&self, mode: DiffSource, session_id: Option<String>, generation: u64);
}

/// The default no-op sink.
pub struct NoDiffSink;

impl DiffSink for NoDiffSink {
    fn fetch(&self, _mode: DiffSource, _session_id: Option<String>, _generation: u64) {}
}

/// The recording sink (tests).
#[derive(Default)]
pub struct RecordingDiffSink {
    pub calls: std::cell::RefCell<Vec<(DiffSource, Option<String>, u64)>>,
}

impl DiffSink for RecordingDiffSink {
    fn fetch(&self, mode: DiffSource, session_id: Option<String>, generation: u64) {
        self.calls.borrow_mut().push((mode, session_id, generation));
    }
}

/// `DialogPair`'s resource state (`createResource` over
/// `server.info` + `server.pair`, dialog-pair.tsx:21-38).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PairState {
    #[default]
    Loading,
    /// Both calls resolved: the auth links, the localhost link, the
    /// expiry minutes, whether any URL is loopback.
    Loaded { links: Vec<String>, localhost: Option<String>, minutes: u64, loopback: bool },
    /// The fetch errored — the error body copy.
    Failed(String),
}

/// The MCP connect/disconnect seam (`client.api.mcp.connect` /
/// `.disconnect`, dialog-mcp.tsx:113-132): the run loop wires the
/// HTTP worker; tests record.
pub trait McpToggleSink {
    fn toggle(&self, server: &str, connect: bool);
}

/// `DialogPair`'s fetch seam (`server.info` + `server.pair`,
/// dialog-pair.tsx:21-38): the run loop wires the HTTP worker.
pub trait PairRefreshSink {
    fn refresh(&self);
}

/// The config persistence seam (SURF-2, F-1's registered runtime
/// toggles): v2's `configState.update` writes — the theme
/// name/mode pins and `session.sidebar` land in cli.json through
/// oc-core's `CliConfig` service (comment-preserving write-through).
pub trait ConfigSink {
    fn update_theme_name(&self, name: &str);
    fn update_theme_mode(&self, mode: &str);
    fn update_session_sidebar(&self, hide: bool);
    /// Lane FINAL (should-fix #5's grouping half): the
    /// exploration-grouping toggle's write-through
    /// (`session.grouping` = "auto"/"none", config/index.tsx's
    /// `session.toggle.exploration_grouping` update,
    /// index.tsx:1050-1062).
    fn update_session_grouping(&self, grouped: bool);
    /// TUI-23/TUI-TD22: the GENERIC write-through — one `DialogConfig`
    /// row cycle (`config.update` setting `path` to `value`) or an
    /// experiment toggle (`experimental.<id>`). The default no-op
    /// keeps embedders compiling; the production sink writes the
    /// cli.json document.
    fn update_setting(&self, path: &str, value: &str) {
        let _ = (path, value);
    }
}

/// The default no-op sink.
pub struct NoConfigSink;

impl ConfigSink for NoConfigSink {
    fn update_theme_name(&self, _name: &str) {}
    fn update_theme_mode(&self, _mode: &str) {}
    fn update_session_sidebar(&self, _hide: bool) {}
    fn update_session_grouping(&self, _grouped: bool) {}
}

/// The recording sink (tests).
#[derive(Default)]
pub struct RecordingConfigSink {
    pub calls: std::cell::RefCell<Vec<(&'static str, String)>>,
}

impl ConfigSink for RecordingConfigSink {
    fn update_theme_name(&self, name: &str) {
        self.calls.borrow_mut().push(("theme.name", name.to_owned()));
    }
    fn update_theme_mode(&self, mode: &str) {
        self.calls.borrow_mut().push(("theme.mode", mode.to_owned()));
    }
    fn update_session_sidebar(&self, hide: bool) {
        self.calls
            .borrow_mut()
            .push(("session.sidebar", if hide { "hide" } else { "auto" }.to_owned()));
    }
    fn update_session_grouping(&self, grouped: bool) {
        self.calls
            .borrow_mut()
            .push(("session.grouping", if grouped { "auto" } else { "none" }.to_owned()));
    }
}

/// The default no-op sink.
pub struct NoMcpToggleSink;

impl McpToggleSink for NoMcpToggleSink {
    fn toggle(&self, _server: &str, _connect: bool) {}
}

/// The recording sink (tests).
#[derive(Default)]
pub struct RecordingMcpToggleSink {
    pub calls: std::cell::RefCell<Vec<(String, bool)>>,
}

impl McpToggleSink for RecordingMcpToggleSink {
    fn toggle(&self, server: &str, connect: bool) {
        self.calls.borrow_mut().push((server.to_owned(), connect));
    }
}

/// The `StartupLoading` pill's state machine fields
/// (startup-loading.tsx:7-11's `show`/`wait`/`hold`/`stamp`).
#[derive(Debug, Clone, Copy, Default)]
pub struct StartupLoadingState {
    /// `plugins.ready` (the host finished booting).
    pub ready: bool,
    /// When the 500 ms wait armed (`wait` fired at stamp).
    pub wait_started_at: Option<u64>,
    /// When the pill showed (`stamp` — the 3 s hold anchor).
    pub shown_at: Option<u64>,
}

/// The composer panel's tab (`ComposerTab` — registration order:
/// subagents, shell, terminals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PickerTab {
    #[default]
    Subagents,
    Shell,
    Terminals,
}

impl PickerTab {
    /// The tab's strip label (`register({ label })`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Subagents => "Subagents",
            Self::Shell => "Shell",
            Self::Terminals => "Terminals",
        }
    }

    /// The registration order's next/previous tab
    /// (`switchTab`, composer/index.tsx:65-70): the three-tab ring in
    /// mount order (subagents → shell → terminals → …).
    #[must_use]
    pub const fn cycle(self, forward: bool) -> Self {
        let ring = [Self::Subagents, Self::Shell, Self::Terminals];
        let index = match self {
            Self::Subagents => 0,
            Self::Shell => 1,
            Self::Terminals => 2,
        };
        let step = if forward { 1 } else { 2 };
        ring[(index + step) % ring.len()]
    }
}

/// The picker's open/tab/filter/selection state (v2: the Session
/// component's `composer` store survives navigation because the
/// session-frame mount keys on the FAMILY root's tab, and the
/// Subagents tab's filter lives in app-lifetime memory storage —
/// subagents-tab.tsx:36-37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerState {
    pub open: bool,
    pub tab: PickerTab,
    /// `subagents-filter` (memory): `active` shows RUNNING entries.
    pub filter_active: bool,
    /// The selected entry index (resets when the tab deactivates or
    /// the route changes — anchored to the route's current entry).
    pub selected: usize,
    /// The route session the selection was anchored against
    /// (`selectedSessionID`, subagents-tab.tsx:65-91).
    pub anchored_to: Option<String>,
}

impl Default for PickerState {
    fn default() -> Self {
        Self {
            open: false,
            tab: PickerTab::Subagents,
            filter_active: true,
            selected: 0,
            anchored_to: None,
        }
    }
}

/// One subagent picker entry (`SubagentEntry`, subagents-tab.tsx:16-23):
/// the family walk's session with its display label + tree prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntry {
    pub session_id: String,
    /// `Locale.titlecase(session.agent)` (or "Subagent").
    pub agent: String,
    /// `withTimestampedFallback(session)` — the title or the
    /// `Child session - <ISO>` fallback.
    pub title: String,
    pub running: bool,
    pub current: bool,
    /// The tree prefix (`├─ `/`└─ ` at depth ≥ 1; "" at depth 0).
    pub prefix: String,
}

/// What a transcript hit does when released (`onMouseUp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptHitAction {
    /// Toggle a group disclosure ([`group-view.tsx`]'s
    /// `onMouseUp={toggle}` — armed for EVERY group kind: Reasoning
    /// :183, Exploration :152, Activity :219, Instructions :256; the
    /// plugin rules table's kinds ride `GroupContent` too).
    ToggleGroup(String),
    /// Toggle one thought's disclosure (the minimal-mode
    /// `ReasoningHeader` box, message-parts.tsx:80-86: `toggleable`,
    /// the body behind `<Show when={expanded}>`). The port's
    /// `thought:{mid}:{pid}` rows are the equivalent surface —
    /// collapsed renders the one-line `+ Thought: {title}` header,
    /// expanded the bordered body.
    ToggleThought { message_id: String, part_id: String },
    /// Open the user message's "Message Actions" select
    /// ([`UserMessage`]'s `onMouseUp` → `dialog.replace(DialogMessage)`,
    /// session/index.tsx:2243-2249).
    MessageActions { message_id: String },
    /// Toggle a tool detail cell's INLINE disclosure
    /// (`ShellDisplay`'s expand / `GenericTool`'s expand — the
    /// `BlockTool`/`InlineTool` `onMouseUp`, index.tsx:2768-2786).
    ToggleToolDetail { message_id: String, part_id: String },
    /// Open the tool detail dialog (`Execute`'s `openDetails` →
    /// `dialog.replace(DialogExecute)`, index.tsx:3207; the background
    /// shell's `DialogShellOutput`).
    OpenToolDialog { message_id: String, part_id: String },
    /// Dismiss the staged-revert banner (`RevertMessage`'s `onMouseUp`
    /// → `session.revert.clear`, index.tsx:2105-2111).
    DismissRevert { session_id: String },
    /// Navigate to a subagent task's child session (the Subagent row's
    /// `onClick: () => navigate({type:"session", sessionID})`,
    /// index.tsx:3107-3112 — lane KM, auditA #9).
    NavigateToSession { session_id: String },
    /// Open a markdown link (`ui/link.tsx:27`'s `onMouseUp` →
    /// `openUrl(href)` — lane KM, auditA #9). The rect is the link
    /// label's run extent.
    OpenLink { url: String },
}

impl TranscriptHitAction {
    /// The disclosure key, when this is a toggle hit (tests).
    #[cfg(test)]
    #[must_use]
    pub fn toggle_key(&self) -> Option<&str> {
        match self {
            Self::ToggleGroup(key) => Some(key.as_str()),
            Self::ToggleThought { .. }
            | Self::MessageActions { .. }
            | Self::ToggleToolDetail { .. }
            | Self::OpenToolDialog { .. }
            | Self::DismissRevert { .. }
            | Self::NavigateToSession { .. }
            | Self::OpenLink { .. } => None,
        }
    }
}

/// One clickable transcript row from the last frame (absolute rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptHitRow {
    /// The session the row belongs to.
    pub session_id: String,
    /// What the release does.
    pub action: TranscriptHitAction,
    /// Absolute screen row of the row's top (frame coordinates).
    pub y: usize,
    /// The row's height in rows.
    pub height: usize,
    /// The hit rect's column extent (absolute screen columns): v2's
    /// disclosure toggle is the `InlineToolRow` BOX — `paddingLeft 3`
    /// plus the icon slot plus the label's own glyphs
    /// (`group-view.tsx:176-190`, the box hugs its content) — NOT the
    /// full row. A release inside `[x, x + width)` toggles. The
    /// user-message box is its full-width raised block.
    pub x: usize,
    /// The hit rect's width in columns.
    pub width: usize,
}

/// The footer location label's hit surface (#33): the painted label's
/// rect plus the RAW directory the working-directory actions use
/// (`ui/working-directory-actions.tsx`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterLocationHit {
    pub directory: String,
    /// Absolute screen row of the status line.
    pub row: usize,
    /// The label's first column.
    pub x: usize,
    /// The label's painted width.
    pub width: usize,
}

/// TUI-12 — the footer live segment's hit surface: the painted
/// `N subagents · N shells` rect (footer.tsx:65-94 — hover lifts the
/// counts muted → text.base, a release dispatches
/// `session.child.first`, the subagent picker's toggle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterLiveHit {
    /// Absolute screen row of the status line.
    pub row: usize,
    /// The segment's first column (the shortcut prefix).
    pub x: usize,
    /// The segment's painted width (shortcut + counts).
    pub width: usize,
}

/// One pointer target's absolute screen rect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// The home footer's status chips (lane KM, auditA #10 —
/// `feature-plugins/home/footer.tsx`): the MCP cell (success or
/// failure — both dispatch `mcp.list`, :24) and the plugins-failed
/// cell (`plugins.list`, :64). Absolute rects from the last home
/// render pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FooterChipHits {
    pub mcp: Option<HitRect>,
    pub plugins: Option<HitRect>,
}

impl HitRect {
    /// Whether the pointer cell sits inside the rect.
    #[must_use]
    pub fn contains(&self, col: usize, row: usize) -> bool {
        row >= self.y && row < self.y + self.height && col >= self.x && col < self.x + self.width
    }
}

/// The sidebar onboarding card's pointer targets (v2's two `onMouseUp`
/// boxes, footer.tsx:45-70): the `✕` dismiss cell
/// (`sidebar.footer.getting-started.dismiss` persists
/// `dismissed = true`) and the "Connect provider" row
/// (`sidebar.footer.getting-started.connect` dispatches
/// `provider.connect`). Recorded by the session render at the pane's
/// paste origin; `None` while the card is hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarOnboardingHits {
    pub dismiss: HitRect,
    pub connect: HitRect,
}

/// The sidebar MCP section's pointer targets (SURF-1, mcp.tsx:26-33 +
/// 76-84): the header row (the collapse toggle, only past two
/// servers) and one rect per visible server row (click → the MCP
/// dialog focused on that server).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidebarMcpHits {
    /// `None` while `list.length <= 2` (no toggle).
    pub header: Option<HitRect>,
    /// `(server name, rect)` in paint order.
    pub rows: Vec<(String, HitRect)>,
}

/// One subagent-picker entry row's pointer target (beta.13 bug 2 —
/// v2 subagents-tab.tsx:221-227: `onMouseMove` moves the selection to
/// the row, `onMouseUp` sets it and navigates into the child session).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntryHit {
    /// The entry's index in `picker_entries` (the selection's slot).
    pub index: usize,
    /// The child session the release navigates to.
    pub session_id: String,
    pub rect: HitRect,
}

/// One plugin sidebar section row's click target (beta.13 bug 2's
/// plugin-world path — the row carries the plugin's own `onMouseUp`;
/// the release runs it through the host's `slot.click`, and the
/// plugin's navigation flows back as a `NavigateToSession` UI
/// request).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRowHit {
    pub plugin_id: String,
    /// The row's index in the section (the `slot.click` address).
    pub row: usize,
    pub rect: HitRect,
}

/// The host-bridge click sink: `(plugin id, row index)` → the
/// plugin's own `onMouseUp` through `slot.click`.
pub type PluginSectionClickSink = Rc<dyn Fn(&str, usize)>;

/// The plugin sections' recorded clickable rows (paint-local until
/// the session render shifts them to absolute cells).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginSectionHits {
    /// The owning session route (empty while no session renders).
    pub session_id: String,
    pub rows: Vec<PluginRowHit>,
}

/// The picker panel's recorded entry rows: the owning session route +
/// one rect per visible entry (absolute cells; empty while the panel
/// is not live on that route).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickerEntryHits {
    pub session_id: String,
    pub rows: Vec<PickerEntryHit>,
}

/// The sidebar's wheel-scroll model (auditA #1): the content-slot
/// offset plus the last render's geometry — the max offset is
/// `content_height - viewport` (v2's scrollbar clamp), and the
/// absolute x extent gates which wheel events belong to the pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SidebarScrollState {
    /// The content slot's scrolled offset (rows hidden above).
    pub offset: usize,
    /// Rows the sections paint in total.
    pub content_height: usize,
    /// Rows the content slot can show (between title and footer).
    pub viewport: usize,
    /// The pane's absolute columns (`[x, x + width)`), when shown.
    pub x_extent: Option<(usize, usize)>,
}

impl SidebarScrollState {
    /// The maximum offset (0 when the content fits).
    #[must_use]
    pub fn max_offset(self) -> usize {
        self.content_height.saturating_sub(self.viewport)
    }

    /// Whether a wheel event at column `col` belongs to the pane.
    #[must_use]
    pub fn contains_col(self, col: usize) -> bool {
        self.x_extent.is_some_and(|(x, width)| col >= x && col < x + width)
    }
}

/// What a dialog chrome button does on release (auditA #6 — v2's
/// per-dialog close affordances).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogButtonAction {
    /// The title row's muted close label (`esc` / `esc/enter`) —
    /// every dialog header carries one (`dialog-help.tsx:25`,
    /// `dialog-confirm.tsx:64`, `dialog-update.tsx:98`, ~22 v2
    /// files): `onMouseUp={() => dialog.clear()}`. v2's label carries
    /// NO hover styling (plain muted text) — the hit rect alone is
    /// the affordance.
    Close,
    /// The Help dialog's right-justified `ok` button
    /// (`dialog-help.tsx:33-40`: the focused-action pill,
    /// `onMouseUp={() => dialog.clear()}`).
    HelpOk,
}

/// One dialog chrome button's pointer target (absolute cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogButtonHit {
    pub action: DialogButtonAction,
    pub rect: HitRect,
}

/// One queued prompt (`session.inbox` row with `delivery: "queue"` —
/// TUI-TD07: the server pending inbox mirror's queue items).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPrompt {
    pub id: String,
    pub text: String,
}

/// TUI-TD07 — one user-type item of the server's pending inbox
/// (`SessionInboxInfo`, client/solid/data.ts:117's
/// `store.session.pending`): the TUI's queue + pending-steer markers
/// derive from this registry (index.tsx:210-215).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItem {
    /// The inbox id (the prompt POST's minted id; the SSE echo keys
    /// every later mutation).
    pub id: String,
    /// `delivery` — "queue" | "steer".
    pub delivery: String,
    /// `payload.text`.
    pub text: String,
    /// `payload.files` (resolved attachments) — the undo refill's
    /// composer payload (data-URI strings).
    pub files: Vec<String>,
}

/// TUI-TD07 — the inbox mutation API seam (v2's [`mutatePending`],
/// index.tsx:605-623): [`update`] `PATCHes` the delivery (steer/queue),
/// `cancel` DELETEs the row. Tests record; production POSTs.
pub trait InboxSink {
    /// `session.inbox.update {delivery}` (PATCH).
    fn update(&self, session_id: &str, inbox_id: &str, delivery: &str);
    /// `session.inbox.cancel` (DELETE).
    fn cancel(&self, session_id: &str, inbox_id: &str);
}

/// The default recording sink (tests assert against its log).
#[derive(Debug, Default)]
pub struct RecordingInboxSink {
    pub calls: std::cell::RefCell<Vec<String>>,
}

impl InboxSink for RecordingInboxSink {
    fn update(&self, session_id: &str, inbox_id: &str, delivery: &str) {
        self.calls.borrow_mut().push(format!("update {session_id} {inbox_id} {delivery}"));
    }

    fn cancel(&self, session_id: &str, inbox_id: &str) {
        self.calls.borrow_mut().push(format!("cancel {session_id} {inbox_id}"));
    }
}

/// TUI-TD05 — the older-history load seam (`data.session.message.
/// loadMore`, client/solid/data.ts:1673+): one page, or `all` pages
/// (`session.first`'s jump-through-history).
pub trait TranscriptSink {
    fn load_older(&self, session_id: &str, cursor: &str, all: bool);
}

/// The default recording sink (tests assert the request log).
#[derive(Debug, Default)]
pub struct RecordingTranscriptSink {
    pub calls: std::cell::RefCell<Vec<String>>,
}

impl TranscriptSink for RecordingTranscriptSink {
    fn load_older(&self, session_id: &str, cursor: &str, all: bool) {
        self.calls
            .borrow_mut()
            .push(format!("load_older {session_id} {cursor} {}", if all { "all" } else { "page" }));
    }
}

/// TUI-TD02 — one message's row geometry in the last render pass
/// (`anchors.messagePositions()`' child: id + y).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagePosition {
    pub id: String,
    /// The row's content y (the nav walk's boundary coordinate).
    pub y: usize,
}

/// TUI-TD02 — the queued two-phase navigation intent (v2's
/// `ensureAllRows(continuation)` — mount everything, then walk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingNavigation {
    /// `scrollToMessage(direction, userOnly)` after the full mount.
    Step { forward: bool, user_only: bool },
    /// `jumpToMessage(id)` after the full mount
    /// (`session.messages_last_user`).
    Jump { id: String },
}

/// The direction of one message-navigation step (`findMessageBoundary`'s
/// `direction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDirection {
    Next,
    Previous,
}

/// The interrupt-arm state (v2's 2-press machine,
/// prompt/index.tsx:500-529): the first escape on a RUNNING turn arms
/// (`store.interrupt = 1` + a 5000 ms reset timer), the second fires
/// the server interrupt (`resume: true`). `count == 0` = disarmed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InterruptArm {
    pub count: u32,
    /// When the arm expires (store clock, ms).
    pub expires_at: u64,
}

/// The interrupt API seam (`client.api.session.interrupt`): tests
/// record; production POSTs with `resume: true` (the 2-press flow's
/// fire path, prompt/index.tsx:522-525).
pub trait InterruptSink {
    fn interrupt(&self, session_id: &str);
}

/// The default recording sink (tests assert against its log).
#[derive(Debug, Default)]
pub struct RecordingInterruptSink {
    pub calls: std::cell::RefCell<Vec<String>>,
}

impl InterruptSink for RecordingInterruptSink {
    fn interrupt(&self, session_id: &str) {
        self.calls.borrow_mut().push(session_id.to_owned());
    }
}

/// What one escape did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptStep {
    /// Not a running turn — the binding rejects (v2's
    /// `enabled: status() === "running"` gate) and the stroke falls
    /// through to the composer.
    Ignored,
    /// First press: armed ("esc again to interrupt").
    Armed,
    /// Second press within the window: the interrupt fired.
    Fired,
}

/// The cycle step (`local.agent.move` / `local.model.cycle`,
/// local.tsx:114-124 + 412-423): `index + direction` wrapping the
/// list length.
fn wrap_step(index: usize, direction: i8, len: usize) -> usize {
    let len = i64::try_from(len).unwrap_or(1).max(1);
    let stepped = i64::try_from(index).unwrap_or(0) + i64::from(direction);
    usize::try_from(stepped.rem_euclid(len)).unwrap_or(0)
}

/// The model-preference ring (`context/local.tsx`'s preferences:
/// `recent`/`favorite` lists + the variant map, `mini/model_pref.rs`'s
/// pure functions) — the f2/favorite cycle source.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelRing {
    pub document: model_pref::ModelPreferenceDocument,
}

/// The per-session transcript metrics the scroll driver needs (the
/// last render pass records them).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TranscriptMetrics {
    /// The transcript pane's row budget (the viewport).
    pub viewport: usize,
    /// The transcript's full content height.
    pub content_height: usize,
    /// TUI-TD05 — the FULL row count of the last render (pre-window;
    /// the mount budget + reveal walk's entry count).
    pub rows: usize,
}

/// The store's root signals (one struct so [`Store::build`] stays
/// under clippy's argument budget as the plugin seams landed).
#[derive(Clone)]
struct StoreSignals {
    tabs: Signal<TabsState>,
    tabs_enabled: Signal<bool>,
    tabs_layout: Signal<crate::components::session_tabs_rail::TabsLayout>,
    vertical_tabs_width: Signal<usize>,
    session_costs: Signal<HashMap<String, f64>>,
    session_history: Signal<Vec<crate::data::SessionSnapshot>>,
    terminal_available: Signal<bool>,
    terminal_registry: Signal<Vec<TerminalListed>>,
    shells: Signal<Vec<LiveShell>>,
    terminal_selection: Signal<Option<(String, String)>>,
    terminal_focus_right: Signal<bool>,
    terminal_generation: Signal<u64>,
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
    plugin_route: Signal<Option<PluginRoutePage>>,
    /// `session.sidebar` — the config mode (auto/hide) after the
    /// boot merge (cli.json, then the last toggle's persisted value).
    sidebar_mode: Signal<crate::components::sidebar::SidebarMode>,
    /// The manual open flag (v2's `sidebarOpen` signal,
    /// session-frame.tsx:68 — ephemeral per mount, never persisted).
    sidebar_open: Signal<bool>,
    /// The terminal size (the toggle's visibility input).
    terminal_size: Signal<(u16, u16)>,
    /// The onboarding card's dismissal (storage `getting-started`).
    sidebar_onboarding: Signal<bool>,
    /// `--auto`'s autoaccept arm (v2 `local.permission.mode ===
    /// "autoaccept"`, permission.tsx:14 — boot-immutable, from the
    /// CLI flag; the composer metadata's `auto` marker reads it).
    permission_autoaccept: Signal<bool>,
    exploration_grouping: Signal<bool>,
    retention_recent: Signal<Vec<String>>,
    session_thinking: Signal<bool>,
    session_verbosity: Signal<String>,
    session_tps: Signal<bool>,
    session_markdown_source: Signal<bool>,
    plugin_hints: Signal<Vec<(String, String)>>,
    plugin_home_actions: Signal<Vec<crate::components::home::PluginQuickAction>>,
    plugin_group_rules: Signal<crate::components::message::model::GroupRules>,
    interrupts: Signal<HashMap<String, InterruptArm>>,
    model_ring: Signal<ModelRing>,
    pending_inbox: Signal<HashMap<String, Vec<InboxItem>>>,
    navigation_message: Signal<HashMap<String, String>>,
    picker: Signal<PickerState>,
    reverts: Signal<HashMap<String, RevertState>>,
    integration_list: Signal<IntegrationListState>,
    integration_attempt: Signal<Option<IntegrationAttempt>>,
    integration_error: Signal<Option<String>>,
    integration_deleting: Signal<Option<String>>,
    session_delete_armed: Signal<Option<String>>,
    pending_compactions: Signal<HashMap<String, Vec<String>>>,
    history_loading: Signal<HashMap<String, bool>>,
    expanded_tools: Signal<std::collections::HashSet<String>>,
    mcp_servers: Signal<Vec<McpServerRow>>,
    plugins_failed: Signal<usize>,
    sidebar_mcp_open: Signal<bool>,
    diff_viewer: Signal<DiffViewerLive>,
    /// SURF-2 `DialogPair`'s resource state.
    pair: Signal<PairState>,
    /// Rename-pending per session (session → armed-at clock ms) — v2's
    /// `data.session.title.pending(sessionID)` (context/data.tsx:35-42:
    /// armed when the title POST fires, held until the new title
    /// projects locally through the `session.renamed` echo). The
    /// sidebar `title_shimmer`'s envelope input (lane TAIL item 1d).
    title_pending: Signal<HashMap<String, u64>>,
    update_notice: Signal<Option<crate::components::update::UpdateNotice>>,
    update_dialog: Signal<crate::components::update::UpdateDialogState>,
    /// TUI-5 (lane-tui2): the Open menu's project registry.
    project_list: Signal<ListState<crate::components::dialog_open::ProjectRow>>,
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
                watchers.tabs_layout.get(),
                watchers.vertical_tabs_width.get(),
                watchers.session_costs.get(),
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
                watchers.interrupts.get(),
                watchers.model_ring.get(),
                watchers.reverts.get(),
                watchers.pending_compactions.get(),
                watchers.history_loading.get(),
                watchers.expanded_tools.get(),
                watchers.mcp_servers.get(),
                watchers.plugins_failed.get(),
                watchers.sidebar_mcp_open.get(),
                watchers.diff_viewer.get(),
                watchers.pair.get(),
                watchers.project_list.get(),
                watch_clock.get(),
            );
            bump.set(bump.get() + 1);
        });
        (clock, redraw_requests, redraw_effect)
    }

    /// The store's constructor body (the signal graph + the redraw
    /// effect over every root signal).
    // The constructor body (one line per signal/seed field — the
    // pointer-surface seeds crossed the pedantic line budget).
    #[allow(clippy::too_many_lines)]
    #[allow(clippy::too_many_lines)] // the update signals are the lane-TAIL rows
    fn build(runtime: Runtime, signals: StoreSignals) -> Self {
        let prompt_sink: std::rc::Rc<dyn PromptSink> =
            std::rc::Rc::new(RecordingPromptSink::default());
        let (clock, redraw_requests, redraw_effect) = Self::mount_redraw_effect(&runtime, &signals);
        let StoreSignals {
            tabs,
            tabs_enabled,
            tabs_layout,
            vertical_tabs_width,
            session_costs,
            session_history,
            terminal_available,
            terminal_registry,
            shells,
            terminal_selection,
            terminal_focus_right,
            terminal_generation,
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
            plugin_route,
            sidebar_mode,
            sidebar_open,
            terminal_size,
            sidebar_onboarding,
            permission_autoaccept,
            exploration_grouping,
            retention_recent,
            session_thinking,
            session_verbosity,
            session_tps,
            session_markdown_source,
            plugin_hints,
            plugin_home_actions,
            plugin_group_rules,
            interrupts,
            model_ring,
            pending_inbox,
            navigation_message,
            picker,
            reverts,
            integration_list,
            project_list,
            integration_attempt,
            integration_error,
            integration_deleting,
            session_delete_armed,
            pending_compactions,
            history_loading,
            expanded_tools,
            mcp_servers,
            plugins_failed,
            sidebar_mcp_open,
            diff_viewer,
            pair,
            title_pending,
            update_notice,
            update_dialog,
        } = signals;
        Self {
            runtime,
            tabs,
            tabs_enabled,
            tabs_layout,
            vertical_tabs_width,
            session_costs,
            session_history,
            terminal_available,
            terminal_registry,
            shells,
            terminal_selection,
            terminal_focus_right,
            terminal_generation,
            terminal_panes: std::cell::RefCell::new(std::collections::HashMap::new()),
            terminal_sink: std::cell::RefCell::new(Rc::new(NoTerminals)),
            terminal_pending_attach: std::cell::RefCell::new(std::collections::HashSet::new()),
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
            plugin_route,
            sidebar_mode,
            sidebar_open,
            terminal_size,
            sidebar_onboarding,
            permission_autoaccept,
            exploration_grouping,
            retention_recent,
            session_thinking,
            session_verbosity,
            session_tps,
            session_markdown_source,
            plugin_hints,
            plugin_home_actions,
            plugin_group_rules,
            event_tap: std::cell::RefCell::new(None),
            plugin_sink: std::cell::RefCell::new(Rc::new(NoPluginCommands)),
            session_list_sink: std::cell::RefCell::new(Rc::new(NoSessionListRefresh)),
            config_model: std::cell::RefCell::new(None),
            selection_sink: Rc::new(RecordingSelectionSink::default()),
            clipboard_sink: Rc::new(crate::clipboard::RecordingClipboardSink::default()),
            editor_requested: Cell::new(false),
            rail_search_hovered: Cell::new(false),
            rail_handle_hot: Cell::new(false),
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
            collapsed_thoughts: std::cell::RefCell::new(HashMap::new()),
            transcript_hit_rows: std::cell::RefCell::new(Vec::new()),
            dialog_frame_rect: std::cell::RefCell::new(None),
            dialog_option_rows: std::cell::RefCell::new(Vec::new()),
            footer_location_hit: std::cell::RefCell::new(None),
            footer_live_hit: std::cell::RefCell::new(None),
            home_footer_chip_hits: std::cell::RefCell::new(None),
            sidebar_onboarding_hits: std::cell::RefCell::new(None),
            sidebar_mcp_hits: std::cell::RefCell::new(SidebarMcpHits::default()),
            picker_entry_hits: std::cell::RefCell::new(PickerEntryHits::default()),
            plugin_section_hits: std::cell::RefCell::new(PluginSectionHits::default()),
            plugin_click_sink: std::cell::RefCell::new(None),
            sidebar_scroll: std::cell::RefCell::new(SidebarScrollState::default()),
            permission_pointer_hits: std::cell::RefCell::new(None),
            dialog_button_hits: std::cell::RefCell::new(Vec::new()),
            lane_dialogs: std::cell::RefCell::new(DialogsLaneState::default()),
            location_hovered: std::cell::Cell::new(false),
            live_hovered: std::cell::Cell::new(false),
            editor_selection: std::cell::RefCell::new(None),
            editor_selection_sent: std::cell::Cell::new(false),
            editor_dismissed_key: std::cell::RefCell::new(None),
            scroll: std::cell::RefCell::new(HashMap::new()),
            transcript_metrics: std::cell::RefCell::new(HashMap::new()),
            interrupts,
            model_ring,
            pending_inbox,
            inbox_sink: std::cell::RefCell::new(Rc::new(RecordingInboxSink::default())),
            transcript_sink: std::cell::RefCell::new(Rc::new(RecordingTranscriptSink::default())),
            message_cursors: std::cell::RefCell::new(HashMap::new()),
            mount_windows: std::cell::RefCell::new(HashMap::new()),
            message_positions: std::cell::RefCell::new(HashMap::new()),
            navigation_message,
            pending_navigation: std::cell::RefCell::new(HashMap::new()),
            picker,
            reverts,
            integration_list,
            project_list,
            integration_attempt,
            integration_error,
            integration_deleting,
            session_delete_armed,
            pending_compactions,
            history_loading,
            expanded_tools,
            mcp_servers,
            plugins_failed,
            mcp_alerted: std::cell::RefCell::new(None),
            sidebar_mcp_open,
            diff_viewer,
            diff_sink: std::cell::RefCell::new(Rc::new(NoDiffSink)),
            open_url_sink: std::cell::RefCell::new(None),
            pair,
            title_pending,
            update_notice,
            update_dialog,
            update_dismissed: RefCell::new(Vec::new()),
            mcp_toggle_sink: std::cell::RefCell::new(Rc::new(NoMcpToggleSink)),
            pair_refresh_sink: std::cell::RefCell::new(None),
            config_sink: std::cell::RefCell::new(Rc::new(NoConfigSink)),
            tab_pulses: RefCell::new(HashMap::new()),
            theme_mode_pref: RefCell::new(None),
            system_mode: Cell::new(oc_theme::types::Mode::Dark),
            interrupt_sink: std::cell::RefCell::new(Rc::new(RecordingInterruptSink::default())),
            model_ring_dirty: Cell::new(false),
            revert_sink: std::cell::RefCell::new(Rc::new(RecordingRevertSink::default())),
            integration_sink: std::cell::RefCell::new(Rc::new(NoIntegrationSink)),
            project_sink: std::cell::RefCell::new(Rc::new(NoProjectSink)),
            session_action_sink: std::cell::RefCell::new(Rc::new(
                RecordingSessionActionSink::default(),
            )),
            footer_location: std::cell::RefCell::new(String::new()),
            location_error: std::cell::RefCell::new(None),
            caret_cell: Cell::new(None),
            connection_since: Cell::new(0),
            startup_state: RefCell::new(StartupLoadingState::default()),
        }
    }

    #[must_use]
    #[allow(clippy::too_many_lines)] // the update signal seeds are the lane-TAIL rows
    pub fn new(runtime: Runtime) -> Self {
        let tabs = Signal::new(&runtime, TabsState::default());
        let tabs_enabled = Signal::new(&runtime, true);
        let session_history = Signal::new(&runtime, Vec::new());
        let terminal_available = Signal::new(&runtime, false);
        let terminal_registry = Signal::new(&runtime, Vec::<TerminalListed>::new());
        let shells = Signal::new(&runtime, Vec::<LiveShell>::new());
        let terminal_selection = Signal::new(&runtime, Option::<(String, String)>::None);
        let terminal_focus_right = Signal::new(&runtime, false);
        let terminal_generation = Signal::new(&runtime, 0u64);
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
        let plugin_route = Signal::new(&runtime, Option::<PluginRoutePage>::None);
        let sidebar_mode =
            Signal::new(&runtime, crate::components::sidebar::SidebarMode::default());
        let sidebar_open = Signal::new(&runtime, false);
        let terminal_size = Signal::new(&runtime, (0u16, 0u16));
        let sidebar_onboarding = Signal::new(&runtime, false);
        let permission_autoaccept = Signal::new(&runtime, false);
        let exploration_grouping = Signal::new(&runtime, true);
        let retention_recent = Signal::new(&runtime, Vec::new());
        let session_thinking = Signal::new(&runtime, false);
        let session_verbosity = Signal::new(&runtime, "medium".to_owned());
        let session_tps = Signal::new(&runtime, false);
        let session_markdown_source = Signal::new(&runtime, false);
        let plugin_hints = Signal::new(&runtime, Vec::<(String, String)>::new());
        let plugin_home_actions =
            Signal::new(&runtime, Vec::<crate::components::home::PluginQuickAction>::new());
        let plugin_group_rules =
            Signal::new(&runtime, crate::components::message::model::GroupRules::empty());
        let interrupts = Signal::new(&runtime, HashMap::<String, InterruptArm>::new());
        let model_ring = Signal::new(&runtime, ModelRing::default());
        let pending_inbox = Signal::new(&runtime, HashMap::<String, Vec<InboxItem>>::new());
        let navigation_message = Signal::new(&runtime, HashMap::<String, String>::new());
        let picker = Signal::new(&runtime, PickerState::default());
        let reverts = Signal::new(&runtime, HashMap::<String, RevertState>::new());
        let integration_list = Signal::new(&runtime, IntegrationListState::Loading);
        let project_list =
            Signal::new(&runtime, ListState::<crate::components::dialog_open::ProjectRow>::Loading);
        let integration_attempt = Signal::new(&runtime, Option::<IntegrationAttempt>::None);
        let integration_error = Signal::new(&runtime, Option::<String>::None);
        let integration_deleting = Signal::new(&runtime, Option::<String>::None);
        let session_delete_armed = Signal::new(&runtime, Option::<String>::None);
        let pending_compactions = Signal::new(&runtime, HashMap::<String, Vec<String>>::new());
        let history_loading = Signal::new(&runtime, HashMap::<String, bool>::new());
        let expanded_tools = Signal::new(&runtime, std::collections::HashSet::<String>::new());
        let mcp_servers = Signal::new(&runtime, Vec::<McpServerRow>::new());
        let plugins_failed = Signal::new(&runtime, 0usize);
        let sidebar_mcp_open = Signal::new(&runtime, true);
        let diff_viewer = Signal::new(&runtime, DiffViewerLive::default());
        let pair = Signal::new(&runtime, PairState::default());
        let title_pending = Signal::new(&runtime, HashMap::<String, u64>::new());
        let update_notice = Signal::new(&runtime, None::<crate::components::update::UpdateNotice>);
        let update_dialog =
            Signal::new(&runtime, crate::components::update::UpdateDialogState::Current);
        // TUI-3/TUI-13: the vertical-rail layout/width and the
        // per-session cost totals (set at boot from config/storage,
        // fed by `session.usage.updated`).
        let tabs_layout =
            Signal::new(&runtime, crate::components::session_tabs_rail::TabsLayout::default());
        let vertical_tabs_width =
            Signal::new(&runtime, crate::components::session_tabs_rail::SESSION_SIDEBAR_WIDTH);
        let session_costs = Signal::new(&runtime, HashMap::<String, f64>::new());
        Self::build(
            runtime,
            StoreSignals {
                tabs,
                tabs_enabled,
                tabs_layout,
                vertical_tabs_width,
                session_costs,
                session_history,
                terminal_available,
                terminal_registry,
                shells,
                terminal_selection,
                terminal_focus_right,
                terminal_generation,
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
                plugin_route,
                sidebar_mode,
                sidebar_open,
                terminal_size,
                sidebar_onboarding,
                permission_autoaccept,
                exploration_grouping,
                retention_recent,
                session_thinking,
                session_verbosity,
                session_tps,
                session_markdown_source,
                plugin_hints,
                plugin_home_actions,
                plugin_group_rules,
                interrupts,
                model_ring,
                pending_inbox,
                navigation_message,
                picker,
                reverts,
                integration_list,
                integration_attempt,
                integration_error,
                integration_deleting,
                session_delete_armed,
                pending_compactions,
                history_loading,
                expanded_tools,
                mcp_servers,
                plugins_failed,
                sidebar_mcp_open,
                diff_viewer,
                pair,
                title_pending,
                update_notice,
                update_dialog,
                project_list,
            },
        )
    }

    // -- accessors --------------------------------------------------------

    /// The rename-pending arm's clock stamp (`data.session.title.
    /// pending(sessionID)` — context/data.tsx:35): `Some(since)` while
    /// the rename POST has not yet projected through the echo.
    #[must_use]
    pub fn title_pending_since(&self, session_id: &str) -> Option<u64> {
        self.title_pending.get().get(session_id).copied()
    }

    /// Arm/clear rename-pending for one session (the `title_shimmer`'s
    /// envelope input).
    pub fn set_title_pending(&self, session_id: &str, since: Option<u64>) {
        self.title_pending.update(|pending| match since {
            Some(ms) => {
                pending.insert(session_id.to_owned(), ms);
            }
            None => {
                pending.remove(session_id);
            }
        });
    }

    /// Sync one tab's pulse transition stamps against its live status
    /// (`TabPulseTrack::sync` — stamps only on flips, at the store
    /// clock) and return the track for the strip's envelope read.
    #[must_use]
    pub fn tab_pulse_track(
        &self,
        session_id: &str,
        runs: bool,
        glow: bool,
        complete: bool,
    ) -> crate::components::tab_pulse::TabPulseTrack {
        let now = self.clock();
        let mut pulses = self.tab_pulses.borrow_mut();
        let track = pulses.entry(session_id.to_owned()).or_default();
        track.sync(runs, glow, complete, now);
        *track
    }

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

    /// Apply the resolved `tabs.layout` config (TUI-3 — boot/reload;
    /// `config/index.tsx:330`'s `input.tabs?.layout ?? "horizontal"`).
    pub fn set_tabs_layout(&self, layout: crate::components::session_tabs_rail::TabsLayout) {
        self.tabs_layout.set(layout);
    }

    /// The persisted vertical-rail width (TUI-3 — the `layout`
    /// storage doc's `verticalTabsWidth`, seeded at boot).
    pub fn set_vertical_tabs_width(&self, width: usize) {
        self.vertical_tabs_width.set(width);
    }

    /// The live vertical-rail width (clamped against the CURRENT
    /// terminal width — `clampSessionTabsWidth`, ui/layout.ts:12-17).
    #[must_use]
    pub fn vertical_tabs_width(&self, total: usize) -> usize {
        crate::components::session_tabs_rail::clamp_session_tabs_width(
            self.vertical_tabs_width.get(),
            total,
        )
    }

    /// `verticalTabsVisible()` (app.tsx:575 + 582): the rail mounts
    /// when the layout is vertical AND the split fits
    /// (`sessionTabsFitVertically`, ui/layout.ts:8-10) AND tabs are
    /// enabled with at least one open tab. Returns the rail's width.
    #[must_use]
    pub fn vertical_rail_width(&self, total: usize) -> Option<usize> {
        if self.tabs_layout.get() != crate::components::session_tabs_rail::TabsLayout::Vertical {
            return None;
        }
        if !self.tabs_enabled.get() || self.tabs.get().tabs.is_empty() {
            return None;
        }
        let width = self.vertical_tabs_width(total);
        crate::components::session_tabs_rail::session_tabs_fit_vertically(total, width)
            .then_some(width)
    }

    /// The compact search button's hover state (TUI-3 — the pointer
    /// layer's `⌕` lift, session-tabs-rail.tsx:24-36).
    pub fn set_rail_search_hovered(&self, hovered: bool) {
        self.rail_search_hovered.set(hovered);
    }

    /// The rail resize handle's hover/drag highlight (TUI-3).
    pub fn set_rail_handle_hot(&self, hot: bool) {
        self.rail_handle_hot.set(hot);
    }

    #[must_use]
    pub fn rail_search_hovered(&self) -> bool {
        self.rail_search_hovered.get()
    }

    #[must_use]
    pub fn rail_handle_hot(&self) -> bool {
        self.rail_handle_hot.get()
    }

    /// `config.tabs.layout` — the resolved rail orientation (TUI-3).
    #[must_use]
    pub fn tabs_layout(&self) -> crate::components::session_tabs_rail::TabsLayout {
        self.tabs_layout.get()
    }

    /// `data.session.cost(sessionID)` (client/solid/data.ts:1369-1379,
    /// TUI-13): a CHILD returns its own total; a root sums over its
    /// family. Unknown-to-the-index sessions still report their own
    /// event-fed total (v2's info map holds every listed session; the
    /// port's index only tracks event-observed ones — the own-cost
    /// fallback keeps the listed-session read faithful).
    #[must_use]
    pub fn session_cost(&self, session_id: &str) -> f64 {
        let costs = self.session_costs.get();
        let own = costs.get(session_id).copied().unwrap_or(0.0);
        let index = self.session_index.borrow();
        let is_child = index.get(session_id).is_some_and(|info| {
            info.parent_id.as_deref().is_some_and(|parent| parent != session_id)
        });
        if is_child {
            return own;
        }
        let mut total = own;
        for (id, cost) in &costs {
            if id != session_id && index.root_of(id) == session_id {
                total += *cost;
            }
        }
        total
    }

    /// The session history from the last resync — the session-list
    /// dialog's rows (v2's `data.session.list()`; the tab strip never
    /// reads this).
    #[must_use]
    pub fn session_history(&self) -> Vec<crate::data::SessionSnapshot> {
        self.session_history.get()
    }

    /// The session's live title (v2 `data.session.get(id)?.title`):
    /// the notifications plugin's session index — fed UNCONDITIONALLY
    /// by `session.updated`/`renamed` events on the same ingest
    /// stream, with no open-tab guard. The tab label resolves
    /// session-title-first (`session()?.title ?? tab.title ??
    /// "Untitled session"`, component/session-tabs.tsx:754) so a title
    /// that lands BEFORE the submit flow opens the tab (the forked
    /// title task races the prompt response — drain.rs `fork_title`)
    /// still displays once the tab renders.
    #[must_use]
    pub fn session_title(&self, session_id: &str) -> Option<String> {
        self.session_index.borrow().title(session_id).map(str::to_owned)
    }

    /// The displayed tab label: the session row's title when known,
    /// else the tab's persisted title (v2
    /// `data?.session.get(tab.sessionID)?.title ?? tab.title ?? …` —
    /// session-tabs.tsx:754, 1585; the placeholder itself renders in
    /// the strip component).
    #[must_use]
    pub fn display_title(&self, tab: &SessionTab) -> Option<String> {
        self.session_title(&tab.session_id).or_else(|| tab.title.clone())
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

    /// `toast.activate()` (ui/toast.tsx:170-175 — lane KM): dismiss +
    /// hand the action's dispatch target back to the caller (the
    /// pump runs it through the palette dispatcher; `None` = no
    /// action, the dismiss stands).
    #[must_use]
    pub fn toast_activate(&self) -> Option<String> {
        let command = self.toasts.get().current().and_then(|toast| toast.action_command.clone());
        self.toasts.update(super::components::toast::ToastQueue::dismiss);
        command
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
        let autocomplete =
            state.autocomplete.get(session_id).cloned().unwrap_or_else(Composer::live_autocomplete);
        let edit = state.edits.get(session_id).cloned().unwrap_or_else(Composer::fresh_edit);
        let mut composer = Composer {
            draft,
            history: state.history,
            autocomplete,
            edit,
            placeholder_index: state.placeholder_index,
            // TUI-14 — the persisted shell-mode flag.
            mode: if state.modes.get(session_id).copied().unwrap_or(false) {
                crate::components::composer::ComposerMode::Shell
            } else {
                crate::components::composer::ComposerMode::Normal
            },
            ..Composer::default()
        };
        // The text mirror resyncs on the way in (the draft is the
        // store-facing source; the caret/selection persist in `edit`).
        if composer.edit.text() != composer.draft.text {
            let caret = composer.edit.caret().min(composer.draft.text.chars().count());
            composer.edit.mirror_text(&composer.draft.text);
            composer.edit.set_caret_keep_anchor(caret);
        }
        composer
    }

    /// TUI-14 — the composer's input mode (`store.mode`), without the
    /// full engine assembly (the render paths' read).
    #[must_use]
    pub fn composer_mode(&self, session_id: &str) -> crate::components::composer::ComposerMode {
        if self.prompt.get().modes.get(session_id).copied().unwrap_or(false) {
            crate::components::composer::ComposerMode::Shell
        } else {
            crate::components::composer::ComposerMode::Normal
        }
    }

    /// Update every composer engine's wrap width (the opentui editing
    /// engine's visual-line viewport — the display wrap's inner width,
    /// kept fresh on resize; a no-op on the legacy path).
    pub fn set_prompt_viewport_width(&self, width: usize) {
        self.prompt.update(|state| {
            for edit in state.edits.values_mut() {
                edit.set_viewport_width(u32::try_from(width.max(1)).unwrap_or(u32::MAX));
            }
        });
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
            state.edits.insert(session_id.to_owned(), composer.edit.clone());
            state.history = composer.history.clone();
            state.placeholder_index = composer.placeholder_index;
            // TUI-14 — the shell-mode flag persists per session.
            if composer.mode == crate::components::composer::ComposerMode::Shell {
                state.modes.insert(session_id.to_owned(), true);
            } else {
                state.modes.remove(session_id);
            }
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
        let prompt = SubmittedPrompt {
            text: draft.text,
            files: draft.files,
            agents: draft.agents,
            ..SubmittedPrompt::default()
        };
        let model = self.resolve_model(session_id);
        self.prompt_sink.send_with_model(session_id, &prompt, model.as_ref());
        Some(prompt)
    }

    /// Forward an already-taken submission to the engine sink (the
    /// composer path: `Composer::handle_key` took the prompt and
    /// recorded history itself — only the sink call remains). The
    /// resolved model rides along (the v2 submit's `commitModel`).
    pub fn forward_submission(&self, session_id: &str, prompt: &SubmittedPrompt) {
        let model = self.resolve_model(session_id);
        self.prompt_sink.send_with_model(session_id, prompt, model.as_ref());
    }

    /// TUI-14 — the shell-mode submit (`session.shell`,
    /// prompt/index.tsx:1303-1307): the text runs as a shell command
    /// through the engine sink (never `session.prompt`).
    pub fn send_shell_command(&self, session_id: &str, command: &str) {
        self.prompt_sink.send_shell(session_id, command);
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

    /// The user-message block's rail color — the SESSION's agent
    /// categorical hue (`local.agent.color(data.session.get(
    /// sessionID)?.agent ?? "build")`, session/index.tsx:2200): the
    /// registry's agent id resolved through the visible agent list,
    /// falling back to the first hue (the "build" default).
    #[must_use]
    pub fn session_agent_rail_color(&self, session_id: &str) -> Option<ratatui::style::Color> {
        let agent = self.session_info(session_id).and_then(|info| info.agent)?;
        let lists = self.server_lists();
        let rows = lists.agents.rows()?;
        let visible: Vec<&ServerAgent> = rows.iter().filter(|agent| !agent.hidden).collect();
        let index = visible.iter().position(|candidate| candidate.id == agent).unwrap_or(0);
        Some(self.theme().styles().agent_color(index))
    }

    // -- server-backed pickers + selection (F3.i) ------------------------

    /// The footer's location label (#4): `{abbreviated-cwd}:{branch}`
    /// ("" until the location worker resolves).
    #[must_use]
    pub fn footer_location(&self) -> String {
        self.footer_location.borrow().clone()
    }

    /// Set the footer location label (the boot location worker).
    pub fn set_footer_location(&self, label: String) {
        *self.footer_location.borrow_mut() = label;
    }

    /// TUI-30 — the location worker's error state
    /// (`currentLocation.error?.location.directory`,
    /// context/location.tsx): the directory that failed to resolve.
    /// The boot location worker sets it when `location.get` fails and
    /// clears it on a successful resolve.
    pub fn set_location_error(&self, directory: Option<String>) {
        *self.location_error.borrow_mut() = directory;
        self.request_redraw();
    }

    /// TUI-30 — the failed directory, when one is recorded.
    #[must_use]
    pub fn location_error(&self) -> Option<String> {
        self.location_error.borrow().clone()
    }

    /// TUI-30 — the SessionLocationMissing mount gate
    /// (index.tsx:1484-1496): `session() &&
    /// currentLocation.error?.location.directory ===
    /// session().location.directory`. The session's directory comes
    /// from the resync snapshot (`location.directory` on the wire);
    /// `Some(directory)` mounts the recovery screen for it.
    #[must_use]
    pub fn session_location_missing(&self, session_id: &str) -> Option<String> {
        let error = self.location_error.borrow().clone()?;
        let directory = self
            .session_history()
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| session.directory.clone())?;
        (directory == error).then_some(error)
    }

    /// The live caret's screen cell (the terminal cursor's position).
    #[must_use]
    pub fn caret_cell(&self) -> Option<(u16, u16)> {
        self.caret_cell.get()
    }

    /// Record/clear the caret cell (the route render sets it; the app
    /// applies it through `surface.set_cursor` at the draw seam).
    pub fn set_caret_cell(&self, cell: Option<(u16, u16)>) {
        self.caret_cell.set(cell);
    }

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

    /// Merge the integration-connected fold (`useConnected`).
    pub fn set_integration_connected(&self, connected: bool) {
        self.server_lists.update(|lists| lists.connected = connected);
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
                action_command: None,
                modal: false,
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

    /// `worktree.list` through the sink (the Open menu's worktrees
    /// view mount — dialog-open.tsx:121-126's `worktree.list`).
    pub fn worktree_sink_list(&self, project_id: &str) {
        self.worktree_sink.borrow().list(project_id);
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

    /// The session-list refresh seam (F13): install the production
    /// loop-fetcher bridge; the default no-ops.
    pub fn set_session_list_sink(&self, sink: Rc<dyn SessionListSink>) {
        *self.session_list_sink.borrow_mut() = sink;
    }

    /// F13: the /sessions dialog's open-time refetch request (the
    /// `DialogSessionList` mount sync).
    pub fn request_session_list_refresh(&self) {
        self.session_list_sink.borrow().refresh();
    }

    // -- TUI-4: the session-terminals registry + the pane states ------

    /// Install the terminal IO seam (the run loop's attach pump).
    pub fn set_terminal_sink(&self, sink: Rc<dyn TerminalSink>) {
        *self.terminal_sink.borrow_mut() = sink;
    }

    /// `sessions.available()` — the server's persistent-pty capability
    /// (server.info `capabilities.persistentPty !== false`,
    /// session-terminals.tsx:66-70).
    pub fn set_terminal_available(&self, available: bool) {
        self.terminal_available.set(available);
    }

    #[must_use]
    pub fn terminal_available(&self) -> bool {
        self.terminal_available.get()
    }

    /// The session's terminal list (the registry entry,
    /// session-terminals.tsx:79-82).
    #[must_use]
    pub fn terminals(&self, session_id: &str) -> Vec<TerminalRecord> {
        self.terminal_registry
            .get()
            .iter()
            .find(|entry| entry.session_id == session_id)
            .map(|entry| entry.terminals.clone())
            .unwrap_or_default()
    }

    /// Replace one session's registry entry (`refresh`,
    /// session-terminals.tsx:29-34 — an unknown selection clears).
    pub fn set_terminals(&self, session_id: &str, terminals: Vec<TerminalRecord>) {
        let selection_valid = terminals.iter().any(|terminal| {
            self.terminal_selection
                .get()
                .is_some_and(|(session, pty)| session == session_id && pty == terminal.id)
        });
        if !selection_valid {
            if let Some((session, _)) = self.terminal_selection.get() {
                if session == session_id {
                    self.terminal_selection.set(None);
                }
            }
        }
        self.terminal_registry.update(|registry| {
            match registry.iter_mut().find(|entry| entry.session_id == session_id) {
                Some(entry) => entry.terminals = terminals,
                None => {
                    registry.push(TerminalListed { session_id: session_id.to_owned(), terminals });
                }
            }
        });
        self.request_redraw();
    }

    /// `selectedTerminalID` (the persisted per-session pick).
    #[must_use]
    pub fn selected_terminal(&self, session_id: &str) -> Option<String> {
        self.terminal_selection
            .get()
            .filter(|(session, _)| session == session_id)
            .map(|(_, pty)| pty)
    }

    /// The MOUNTED pane's pty (session-frame's `rightPane() ===
    /// "terminal"` — the selection with the availability gate).
    #[must_use]
    pub fn terminal_pane_mounted(&self, session_id: &str) -> Option<String> {
        if !self.terminal_available.get() {
            return None;
        }
        self.selected_terminal(session_id)
    }

    /// `selectTerminal` (session-terminals.tsx:52-58): the persisted
    /// pick + the attach/detach ride on the sink; the pane takes the
    /// input focus on mount (`shouldFocus`).
    pub fn select_terminal(&self, session_id: &str, pty_id: Option<&str>) {
        if let Some(pty) = pty_id {
            self.terminal_selection.set(Some((session_id.to_owned(), pty.to_owned())));
            self.terminal_focus_right.set(true);
            // The attach-storm guard: the explicit selection IS the
            // attach request — the render self-heal must not fire a
            // second chain before the snapshot lands.
            self.terminal_pending_attach.borrow_mut().insert(pty.to_owned());
            if let Ok(storage) = crate::storage::ChannelStorage::open_default_migrated() {
                let mut doc = storage
                    .read("session-terminal-selection")
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                if let Some(sessions) =
                    doc.get_mut("sessions").and_then(|value| value.as_object_mut())
                {
                    sessions
                        .insert(session_id.to_owned(), serde_json::Value::String(pty.to_owned()));
                    let _ = storage.write("session-terminal-selection", &doc);
                }
            }
        } else {
            let was_selected =
                self.terminal_selection.get().is_some_and(|(session, _)| session == session_id);
            self.terminal_selection.set(None);
            self.terminal_focus_right.set(false);
            if was_selected {
                if let Ok(storage) = crate::storage::ChannelStorage::open_default_migrated() {
                    let mut doc = storage
                        .read("session-terminal-selection")
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    if let Some(sessions) =
                        doc.get_mut("sessions").and_then(|value| value.as_object_mut())
                    {
                        sessions.insert(session_id.to_owned(), serde_json::Value::Null);
                        let _ = storage.write("session-terminal-selection", &doc);
                    }
                }
            }
        }
        self.terminal_sink.borrow().select(session_id, pty_id);
        self.request_redraw();
    }

    /// Restore a persisted selection at boot (the storage doc's
    /// `sessions[sessionID]` — session-terminals.tsx:22-25).
    pub fn restore_terminal_selection(&self, session_id: &str, pty_id: Option<&str>) {
        if let Some(pty) = pty_id {
            self.terminal_selection.set(Some((session_id.to_owned(), pty.to_owned())));
        }
    }

    /// `terminal.toggle` (session-frame.tsx:207-226): open → the last
    /// listed terminal or a fresh one; open already → focus session +
    /// deselect.
    pub fn terminal_toggle(&self, session_id: &str) {
        if self.terminal_pane_mounted(session_id).is_some() {
            self.select_terminal(session_id, None);
            return;
        }
        if !self.terminal_available.get() {
            return;
        }
        // v2's toggle refreshes the registry first, then picks the
        // last terminal or creates (`refresh().then(...at(-1) ||
        // newTerminal)`); the port's synchronous store picks from the
        // local registry and always fires the refresh alongside.
        self.terminal_sink.borrow().refresh(session_id);
        let listed = self.terminals(session_id);
        if listed.is_empty() {
            self.terminal_sink.borrow().create(session_id);
            return;
        }
        let last = listed.last().map(|terminal| terminal.id.clone());
        match last {
            Some(pty) => self.select_terminal(session_id, Some(&pty)),
            None => self.terminal_sink.borrow().create(session_id),
        }
    }

    /// `terminal.close` (`<leader>up`) — deselect only.
    pub fn terminal_close(&self, session_id: &str) {
        if self.terminal_pane_mounted(session_id).is_some() {
            self.select_terminal(session_id, None);
        }
    }

    /// `terminal.select` (`<leader>down`) — the composer's Terminals
    /// tab + the registry refresh (session-frame.tsx:221-233).
    pub fn terminal_open_picker(&self) {
        if let Route::Session { session_id } = self.route.get() {
            let mut picker = self.picker.get();
            picker.open = true;
            picker.tab = PickerTab::Terminals;
            self.picker.set(picker);
            self.terminal_focus_right.set(false);
            self.terminal_sink.borrow().refresh(&session_id);
            self.request_redraw();
        }
    }

    /// The active-pane flag (`activePane`, session-frame.tsx:72).
    #[must_use]
    pub fn terminal_focus_right(&self) -> bool {
        self.terminal_focus_right.get()
    }

    /// `focusSession` / `focusRightPane` (the pane focus pair).
    pub fn set_terminal_focus_right(&self, focused: bool) {
        self.terminal_focus_right.set(focused);
        self.request_redraw();
    }

    /// The pane-grid redraw bump.
    pub fn terminal_bump(&self) {
        self.terminal_generation.update(|generation| generation.wrapping_add(1));
        self.request_redraw();
    }

    /// The mounted pane's rendered frame at `size` (the box's own
    /// width/height; the content grid is `width-2` —
    /// terminal-pane.tsx:289). A selection without a live state
    /// re-requests the attach (the self-healing mount).
    #[must_use]
    pub fn terminal_pane_frame(
        &self,
        session_id: &str,
        size: (u16, u16),
    ) -> Option<crate::render::Frame> {
        let pty = self.terminal_pane_mounted(session_id)?;
        let mut panes = self.terminal_panes.borrow_mut();
        if !panes.contains_key(&pty) {
            drop(panes);
            let requested = self.terminal_pending_attach.borrow_mut().insert(pty.clone());
            if requested {
                self.terminal_sink.borrow().select(session_id, Some(&pty));
            }
            return None;
        }
        let state = panes.get_mut(&pty)?;
        let content_cols = size.0.saturating_sub(2).max(1);
        if let Some(frame) = state.resize_pane(content_cols, size.1) {
            self.terminal_sink.borrow().send_frame(&pty, &frame);
        }
        let frame = crate::terminal_pane::render_terminal_pane(state, size.0, size.1);
        Some(frame)
    }

    /// The snapshot's fast-forward (`connect()`'s first hop,
    /// terminal-pane.tsx:175-182): seed the pane state.
    pub fn terminal_pane_snapshot(&self, pty_id: &str, size: (u16, u16), checkpoint: &[u8]) {
        self.terminal_pending_attach.borrow_mut().remove(pty_id);
        self.terminal_panes.borrow_mut().insert(
            pty_id.to_owned(),
            crate::terminal_pane::TerminalPaneState::from_snapshot(size, checkpoint),
        );
        self.terminal_bump();
    }

    /// One WS output chunk into the pane's grid.
    pub fn terminal_pane_output(&self, pty_id: &str, data: &[u8]) {
        if let Some(state) = self.terminal_panes.borrow_mut().get_mut(pty_id) {
            state.push_output(data);
        }
        self.terminal_bump();
    }

    /// A server resize (+ checkpoint) into the pane's queue.
    pub fn terminal_pane_resized(
        &self,
        pty_id: &str,
        cols: u16,
        rows: u16,
        checkpoint: Option<Vec<u8>>,
    ) {
        if let Some(state) = self.terminal_panes.borrow_mut().get_mut(pty_id) {
            state.push_resize(cols, rows, checkpoint);
        }
        self.terminal_bump();
    }

    /// `replay_complete` — the input gate opens; the backlog flushes
    /// and the controller's opening interact rides along.
    pub fn terminal_pane_ready(&self, pty_id: &str) {
        let mut to_send: Vec<Vec<u8>> = Vec::new();
        {
            let mut panes = self.terminal_panes.borrow_mut();
            if let Some(state) = panes.get_mut(pty_id) {
                to_send = state.mark_ready();
                if let Some(frame) = state.interact() {
                    to_send.push(frame);
                }
            }
        }
        for frame in &to_send {
            self.terminal_sink.borrow().send_frame(pty_id, frame);
        }
        self.terminal_bump();
    }

    /// `attached` — the info size + the controller role.
    pub fn terminal_pane_attached(&self, pty_id: &str, size: (u16, u16), controller: bool) {
        if let Some(state) = self.terminal_panes.borrow_mut().get_mut(pty_id) {
            state.attached(size, controller);
        }
        self.terminal_bump();
    }

    /// `controller_changed` (takeover).
    pub fn terminal_pane_controller(&self, pty_id: &str, controller: bool) {
        if let Some(state) = self.terminal_panes.borrow_mut().get_mut(pty_id) {
            state.set_controller(controller);
        }
        self.terminal_bump();
    }

    /// The failure surface (connection failed / disconnected).
    pub fn terminal_pane_failure(&self, pty_id: &str, message: &str) {
        if let Some(state) = self.terminal_panes.borrow_mut().get_mut(pty_id) {
            state.set_failure(message.to_owned());
        }
        self.terminal_bump();
    }

    /// Drop a detached pane's state.
    pub fn terminal_pane_drop(&self, pty_id: &str) {
        self.terminal_panes.borrow_mut().remove(pty_id);
        self.terminal_bump();
    }

    /// Route one stroke into the mounted pane (`handleKeyPress`):
    /// `true` when consumed (the encoder produced bytes for a pane
    /// that accepts input).
    pub fn terminal_pane_input(&self, session_id: &str, stroke: &crate::keymap::KeyStroke) -> bool {
        let Some(pty) = self.terminal_pane_mounted(session_id) else { return false };
        if !self.terminal_focus_right.get() {
            return false;
        }
        let Some(bytes) = crate::terminal_pane::encode_stroke(stroke) else { return false };
        let frames = {
            let mut panes = self.terminal_panes.borrow_mut();
            match panes.get_mut(&pty) {
                Some(state) => state.push_input(&bytes),
                None => Vec::new(),
            }
        };
        let consumed = !frames.is_empty();
        for frame in frames {
            self.terminal_sink.borrow().send_frame(&pty, &frame);
        }
        consumed
    }

    /// The Terminals tab's rows (terminals-tab.tsx:79-108): one entry
    /// per listed terminal (`foregroundProcess ?? title`, the visible
    /// pane `▪`-marked current) + the trailing `+ New terminal` slot.
    #[must_use]
    pub fn picker_terminals(&self, session_id: &str) -> Vec<PickerEntry> {
        let visible = self.terminal_pane_mounted(session_id);
        let mut entries: Vec<PickerEntry> = self
            .terminals(session_id)
            .into_iter()
            .map(|terminal| {
                let current = visible.as_deref() == Some(terminal.id.as_str());
                let title = terminal.label().to_owned();
                PickerEntry {
                    session_id: terminal.id,
                    agent: "Terminals".to_owned(),
                    title,
                    running: false,
                    current,
                    prefix: String::new(),
                }
            })
            .collect();
        entries.push(PickerEntry {
            session_id: String::new(),
            agent: "Terminals".to_owned(),
            title: "+ New terminal".to_owned(),
            running: false,
            current: false,
            prefix: String::new(),
        });
        entries
    }

    /// `composer.terminal.select` (terminals-tab.tsx:51-64): the
    /// selected row — a terminal mounts, the add slot creates; either
    /// way the composer closes.
    pub fn picker_terminal_select(&self) {
        if let Route::Session { session_id } = self.route.get() {
            let entries = self.picker_terminals(&session_id);
            let selected = self.picker_selection(&session_id, entries.len());
            let entry = entries.get(selected).cloned();
            self.picker_close();
            match entry {
                Some(entry) if !entry.session_id.is_empty() => {
                    self.select_terminal(&session_id, Some(&entry.session_id));
                }
                _ => {
                    self.terminal_sink.borrow().create(&session_id);
                }
            }
        }
    }

    // -- the model selection chain (F8/F10/F22) ------------------------------
    //
    // v2 `fallbackModel` (context/local.tsx:186-213) +
    // `durableSelection` (:258-266) + `sessionSelection` (:268-283):
    // per-session explicit pick → the session record's durable model →
    // the configured default → the ring's recent head → the catalog's
    // first model. Every non-catalog step validates against the loaded
    // model catalog (`isModelValid`); the catalog head needs no
    // validation. `None` while the catalog has not loaded (v2's
    // `models()` empty ⇒ every step invalid).

    /// The configured default model (`config.model`, local.tsx:172-184's
    /// `configuredModel` — the shared config's `model` field, string
    /// `provider/model` or `{providerID, model, variant}`).
    pub fn set_config_model(&self, model: Option<ModelSelection>) {
        *self.config_model.borrow_mut() = model;
    }

    /// Whether `(provider, id)` exists in the loaded model catalog
    /// (`isModelValid` — local.tsx's `models()` map lookup).
    fn model_in_catalog(&self, provider_id: &str, id: &str) -> bool {
        self.server_lists
            .get()
            .models
            .rows()
            .is_some_and(|models| models.iter().any(|m| m.provider_id == provider_id && m.id == id))
    }

    /// The durable model off the session record (`durableSelection`):
    /// the resync's `session.list` carries each session's committed
    /// model — restart restores the per-session selection from it.
    fn durable_model(&self, session_id: &str) -> Option<ModelSelection> {
        if session_id == crate::app::HOME_PROMPT_KEY {
            return None;
        }
        let history = self.session_history.get();
        let model = history
            .iter()
            .find(|session| session.id == session_id)
            .and_then(|session| session.model.clone())?;
        let selection = ModelSelection {
            provider_id: model.provider_id,
            model_id: model.id,
            variant: model.variant,
        };
        self.model_in_catalog(&selection.provider_id, &selection.model_id).then_some(selection)
    }

    /// Resolve the model a submission of `session_id` commits (the
    /// chain above; the home key resolves the NEXT session's model —
    /// the staged pick then the fallbacks).
    #[must_use]
    pub fn resolve_model(&self, session_id: &str) -> Option<ModelSelection> {
        // 1. The explicit per-session pick (a dialog/footer pick, a
        //    cycle, or the home route's staged selection).
        if let Some(model) = self.selection_for(session_id).model {
            return Some(model);
        }
        // 2. The session record's durable model (restart restore).
        if let Some(model) = self.durable_model(session_id) {
            return Some(model);
        }
        // 3. The configured default (`config.model`).
        if let Some(model) = self.config_model.borrow().clone() {
            if self.model_in_catalog(&model.provider_id, &model.model_id) {
                return Some(model);
            }
        }
        // 4. The ring's recent head (`preferences.recent[0]`).
        if let Some(recent) = self.model_ring.get().document.recent.first() {
            let selection = ModelSelection {
                provider_id: recent.provider_id.clone(),
                model_id: recent.model_id.clone(),
                variant: None,
            };
            if self.model_in_catalog(&selection.provider_id, &selection.model_id) {
                return Some(selection);
            }
        }
        // 5. The catalog's first model (`models()?.[0]` — the pristine
        //    home's first prompt resolves SOMETHING, so the provider
        //    call surfaces an actionable auth/provider error instead
        //    of the "No model" dead end).
        self.server_lists.get().models.rows().and_then(|models| {
            models.first().map(|model| ModelSelection {
                provider_id: model.provider_id.clone(),
                model_id: model.id.clone(),
                variant: None,
            })
        })
    }

    /// The model the UI DISPLAYS for `session_id` (the composer's
    /// statusline hint, the pickers's `current`): the explicit pick,
    /// else the resolved chain (`currentSelection` — local.tsx:225-236).
    #[must_use]
    pub fn effective_model(&self, session_id: &str) -> Option<ModelSelection> {
        self.selection_for(session_id).model.or_else(|| self.resolve_model(session_id))
    }

    /// TUI-TD15 — the `--prompt` auto-submit's readiness gate
    /// (home.tsx:69-80 / index.tsx:597-604): v2 waits for
    /// `local.model.ready && local.model.catalogReady` plus a current
    /// agent AND model before `r.submit()`. The port's translation:
    /// the model catalog is LOADED (the catalog/ready halves — the
    /// initial `Loading`/`Failed` states keep the startup prompt
    /// parked) and a current model RESOLVES against it (the agent
    /// half is the default `build` agent until a pick lands — always
    /// current in the port, v2's pristine state the same).
    #[must_use]
    pub fn startup_prompt_ready(&self, session_id: &str) -> bool {
        if !matches!(self.server_lists().models, ListState::Loaded(_)) {
            return false;
        }
        self.resolve_model(session_id).is_some()
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

    // -- update notification (auditB #10, lane TAIL) ---------------------

    /// The current banner notice (`update_notification()`).
    #[must_use]
    pub fn update_notice(&self) -> Option<crate::components::update::UpdateNotice> {
        self.update_notice.get()
    }

    /// The dismissed `source:version` keys (the loop loads/saves
    /// these through its storage handle).
    #[must_use]
    pub fn update_dismissed(&self) -> Vec<String> {
        self.update_dismissed.borrow().clone()
    }

    /// Replace the dismissal history (boot load).
    pub fn set_update_dismissed(&self, versions: Vec<String>) {
        *self.update_dismissed.borrow_mut() = versions;
    }

    /// Notify one update notice (v2's `notify`): suppressed when the
    /// `source:version` key is in the dismissed history; a CLIENT
    /// notice never displaces a live SERVER one.
    pub fn update_notify(&self, notice: crate::components::update::UpdateNotice) {
        let source = if notice.remote { "server" } else { "client" };
        let key = format!("{source}:{}", notice.version);
        if self.update_dismissed.borrow().contains(&key) {
            return;
        }
        let current = self.update_notice.get();
        if let Some(current) = &current {
            if !notice.remote && current.remote {
                return; // client never displaces server
            }
        }
        self.update_notice.set(Some(notice));
    }

    /// Dismiss the banner + record the dismissal (the loop persists
    /// through its storage handle; the last 100 keys).
    pub fn update_dismiss(&self) {
        if let Some(current) = self.update_notice.get() {
            let source = if current.remote { "server" } else { "client" };
            let mut history = self.update_dismissed.borrow_mut();
            history.push(format!("{source}:{}", current.version));
            history.truncate(100);
        }
        self.update_notice.set(None);
    }

    /// The update dialog's live state.
    #[must_use]
    pub fn update_dialog_state(&self) -> crate::components::update::UpdateDialogState {
        self.update_dialog.get()
    }

    /// Set the update dialog's live state (the check/install
    /// transitions).
    pub fn set_update_dialog(&self, state: crate::components::update::UpdateDialogState) {
        self.update_dialog.set(state);
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
    fn update_messages<R>(
        &self,
        session_id: &str,
        update: impl FnOnce(&mut Vec<Message>) -> R,
    ) -> R {
        self.messages
            .update(|state| update(state.by_session.entry(session_id.to_owned()).or_default()))
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

    /// `session.tab.history.back` (keybind.ts:114 — lane KM): walk the
    /// in-memory navigation history one entry back (the browser-shaped
    /// stack `recordSessionTabHistory` maintains). The walk must NOT
    /// re-record through `navigate` (that appends + truncates the
    /// forward tail) — the cursor moves inside the preserved stack.
    #[must_use]
    pub fn tab_history_back(&self) -> Option<String> {
        let tabs = self.tabs.get();
        let index = tabs.history.index;
        if index <= 0 {
            return None;
        }
        let target = tabs.history.entries.get(usize::try_from(index - 1).unwrap_or(0))?.clone();
        let mut history = tabs.history.clone();
        history.index = index - 1;
        self.navigate(Route::Session { session_id: target.clone() });
        self.tabs.update(|state| state.history = history);
        Some(target)
    }

    /// `session.tab.history.forward` (keybind.ts:115 — lane KM,
    /// ctrl+i): the symmetric forward walk.
    #[must_use]
    pub fn tab_history_forward(&self) -> Option<String> {
        let tabs = self.tabs.get();
        let next = tabs.history.index + 1;
        let target = usize::try_from(next)
            .ok()
            .and_then(|index| tabs.history.entries.get(index).cloned())?;
        let mut history = tabs.history.clone();
        history.index = next;
        self.navigate(Route::Session { session_id: target.clone() });
        self.tabs.update(|state| state.history = history);
        Some(target)
    }

    /// Reorder a tab (`moveSessionTab`, the drag-release persist).
    pub fn move_tab(&self, session_id: &str, index: usize) {
        self.tabs.update(|state| {
            state.tabs =
                model::move_session_tab(std::mem::take(&mut state.tabs), session_id, index);
        });
        self.request_redraw();
    }

    /// The drag-reorder preview (session-tabs.tsx:1314-1321 — lane KM):
    /// the slot the dragged tab would land at; `None` clears.
    pub fn set_tab_drag_preview(&self, preview: Option<(String, usize)>) {
        self.tabs.update(|state| state.drag_preview = preview);
        self.request_redraw();
    }

    /// The live preview state.
    #[must_use]
    pub fn tab_drag_preview(&self) -> Option<(String, usize)> {
        self.tabs.get().drag_preview
    }

    /// Arm the ✕ close-hold (session-tabs.tsx:1481-1503 — lane KM):
    /// snapshot the frozen layout + start the 5 s clock.
    pub fn arm_close_hold(&self, hold: CloseHold) {
        self.tabs.update(|state| state.close_hold = Some(hold));
        self.request_redraw();
    }

    /// Release the close-hold (`releaseCloseHold` — the expiry, the
    /// pointer leaving the strip, a resize, or any foreign mutation).
    pub fn clear_close_hold(&self) {
        if self.tabs.get().close_hold.is_some() {
            self.tabs.update(|state| state.close_hold = None);
            self.request_redraw();
        }
    }

    /// The close-hold, lazily expired against the store clock (the
    /// read IS the `setTimeout(releaseCloseHold, 5000)` arm).
    #[must_use]
    pub fn close_hold(&self) -> Option<CloseHold> {
        let hold = self.tabs.get().close_hold?;
        if self.clock() >= hold.expires_ms {
            self.clear_close_hold();
            return None;
        }
        Some(hold)
    }

    /// The strip's effective tab ORDER (lane KM): the drag preview
    /// applied (`ordered = moveSessionTab(tabs, pending)` — the strip
    /// renders the preview order while the drag is live).
    #[must_use]
    pub fn strip_tabs_ordered(&self) -> Vec<SessionTab> {
        let tabs = self.tabs.get();
        match tabs.drag_preview {
            Some((ref source, index)) => model::move_session_tab(tabs.tabs.clone(), source, index),
            None => tabs.tabs,
        }
    }

    /// `queued_prompt.undo` (TUI-TD07, index.tsx:653-673): cancel the
    /// LAST queued prompt server-side (`session.inbox.cancel`) and
    /// return its payload for the composer refill
    /// (`appendPrompt(target.current, {...payload, pasted: []})`).
    /// The v2 shell-mode guard has no port surface (the Rust composer
    /// has no `!` mode state), so every queue row is undoable.
    pub fn undo_queued_prompt(&self, session_id: &str) -> Option<InboxItem> {
        let last = self.queued_prompts(session_id).last().cloned()?;
        let payload = self.queued_prompt_payload(session_id, &last.id)?;
        self.inbox_sink.borrow().cancel(session_id, &last.id);
        self.retract_inbox_item(session_id, &last.id);
        Some(payload)
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
    /// only a real change may request a redraw). `close` marks the
    /// pointer as sitting on that tab's ✕ cell.
    pub fn set_tab_hover(&self, session_id: Option<&str>, close: bool) -> bool {
        let mut changed = false;
        self.tabs.update(|state| {
            let next = session_id.map(str::to_owned);
            let next_close = close && next.is_some();
            if state.hover != next {
                state.hover = next;
                changed = true;
            }
            if state.hover_close != next_close {
                state.hover_close = next_close;
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

    /// Toggle one thought row's disclosure (the minimal-mode
    /// `ReasoningHeader` toggle, message-parts.tsx:66-86) and report
    /// the new EXPANDED value (absent ⇒ expanded).
    pub fn toggle_thought_expanded(
        &self,
        session_id: &str,
        message_id: &str,
        part_id: &str,
    ) -> bool {
        let key = format!("{message_id}:{part_id}");
        let mut thoughts = self.collapsed_thoughts.borrow_mut();
        let set = thoughts.entry(session_id.to_owned()).or_default();
        if set.remove(&key) {
            true
        } else {
            set.insert(key);
            false
        }
    }

    /// Whether a thought row renders expanded (`thought:{mid}:{pid}`
    /// — the render option input; default true).
    #[must_use]
    pub fn thought_expanded(&self, session_id: &str, message_id: &str, part_id: &str) -> bool {
        let key = format!("{message_id}:{part_id}");
        !self.collapsed_thoughts.borrow().get(session_id).is_some_and(|set| set.contains(&key))
    }

    /// The session's collapsed thought keys (`"{mid}:{pid}"` — the
    /// transcript render options input).
    #[must_use]
    pub fn collapsed_thought_keys(&self, session_id: &str) -> std::collections::HashSet<String> {
        self.collapsed_thoughts.borrow().get(session_id).cloned().unwrap_or_default()
    }

    /// The last frame's clickable transcript rows (reasoning headers).
    pub fn set_transcript_hit_rows(&self, rows: Vec<TranscriptHitRow>) {
        *self.transcript_hit_rows.borrow_mut() = rows;
    }

    /// Disarm every route-scoped pointer registry (auditA §4.1): the
    /// transcript hit rows, the footer-location rect and the sidebar
    /// onboarding card all belong to the SESSION render that recorded
    /// them — v2's renderables unmount with the route, so the port's
    /// absolute rects must not outlive it. Called on every
    /// [`Store::navigate`] (the only `route.set` site); the next
    /// session render re-records what paints.
    pub fn clear_route_pointer_registries(&self) {
        self.transcript_hit_rows.borrow_mut().clear();
        *self.footer_location_hit.borrow_mut() = None;
        *self.footer_live_hit.borrow_mut() = None;
        *self.sidebar_onboarding_hits.borrow_mut() = None;
        // Lane KM: the home chips are route-scoped too (the footer
        // unmounts with the home route).
        *self.home_footer_chip_hits.borrow_mut() = None;
        // The sidebar's wheel geometry is route-scoped the same way
        // (the pane unmounts with the session route).
        *self.sidebar_scroll.borrow_mut() = SidebarScrollState::default();
        // The picker panel's entry rows unmount with the route the
        // same way (stale rects would zombie-navigate).
        *self.picker_entry_hits.borrow_mut() = PickerEntryHits::default();
        // The plugin sections' clickable rows ride the sidebar pane —
        // same route scoping.
        *self.plugin_section_hits.borrow_mut() = PluginSectionHits::default();
    }

    /// The last frame's top-dialog frame rect (absolute screen cells)
    /// — the outside-click dismissal surface (`ui/dialog.tsx:34-63`:
    /// the full-screen backdrop closes on mouseup; the inner box's
    /// own mouseup stops propagation).
    pub fn set_dialog_frame_rect(&self, rect: Option<(usize, usize, usize, usize)>) {
        *self.dialog_frame_rect.borrow_mut() = rect;
        if rect.is_none() {
            self.set_dialog_option_rows(Vec::new());
            // The chrome buttons die with the frame (auditA #6 —
            // no dialog, no close label).
            self.dialog_button_hits.borrow_mut().clear();
        }
    }

    /// The top dialog's frame rect, when a dialog rendered last frame.
    #[must_use]
    pub fn dialog_frame_rect(&self) -> Option<(usize, usize, usize, usize)> {
        *self.dialog_frame_rect.borrow()
    }

    /// The top select-dialog's option rows from the last render: for
    /// each FLAT option index, its absolute screen row (only visible
    /// options recorded). The pointer surface
    /// (`dialog-select.tsx:757-775`: motion moves the selection, a
    /// release submits).
    pub fn set_dialog_option_rows(&self, rows: Vec<Option<usize>>) {
        *self.dialog_option_rows.borrow_mut() = rows;
    }

    /// The visible options' absolute rows (flat-index order).
    #[must_use]
    pub fn dialog_option_rows(&self) -> Vec<Option<usize>> {
        self.dialog_option_rows.borrow().clone()
    }

    /// Record the top dialog's chrome buttons (the overlay render,
    /// each frame — auditA #6).
    pub fn set_dialog_button_hits(&self, hits: Vec<DialogButtonHit>) {
        *self.dialog_button_hits.borrow_mut() = hits;
    }

    /// The top dialog's chrome buttons from the last render.
    #[must_use]
    pub fn dialog_button_hits(&self) -> Vec<DialogButtonHit> {
        self.dialog_button_hits.borrow().clone()
    }

    /// #33 — the footer location label's hit rect from the last
    /// render (`useWorkingDirectoryActions`: hover lifts the label,
    /// a release opens the Working directory select).
    pub fn set_footer_location_hit(&self, hit: Option<FooterLocationHit>) {
        *self.footer_location_hit.borrow_mut() = hit;
    }

    /// The location label's hit rect, when one painted.
    #[must_use]
    pub fn footer_location_hit(&self) -> Option<FooterLocationHit> {
        self.footer_location_hit.borrow().clone()
    }

    /// TUI-12 — the footer live segment's hit rect from the last
    /// render (hover lifts the counts; a release dispatches
    /// `session.child.first`).
    pub fn set_footer_live_hit(&self, hit: Option<FooterLiveHit>) {
        *self.footer_live_hit.borrow_mut() = hit;
    }

    /// The live segment's hit rect, when one painted.
    #[must_use]
    pub fn footer_live_hit(&self) -> Option<FooterLiveHit> {
        self.footer_live_hit.borrow().clone()
    }

    /// TUI-12 — the composer footer's live counts (footer.tsx:20-33):
    /// the family's RUNNING sessions excluding self (the subagent
    /// count — `session.family(id).filter(id !== id && status ===
    /// "running")`) and the session's in-flight shells
    /// (`shell.metadata.sessionID === sessionID`).
    #[must_use]
    pub fn footer_live_counts(&self, session_id: &str) -> (usize, usize) {
        let index = self.session_index.borrow();
        let ids = index.ids();
        let mut children: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        for id in &ids {
            if let Some(parent) = index.get(id).and_then(|info| info.parent_id.as_deref()) {
                children.entry(parent).or_default().push(id);
            }
        }
        // The family walk from the ROOT (sessionFamily: the root's
        // descendant tree, self included).
        let mut subagents = 0usize;
        let mut stack = vec![index.root_of(session_id)];
        while let Some(id) = stack.pop() {
            if id != session_id && index.is_running(&id) {
                subagents += 1;
            }
            if let Some(group) = children.get(id.as_str()) {
                stack.extend(group.iter().map(|id| (*id).to_owned()));
            }
        }
        let shells = self
            .shells
            .get()
            .iter()
            .filter(|shell| shell.session_id.as_deref() == Some(session_id))
            .count();
        (subagents, shells)
    }

    /// TUI-12 — the live segment's hover lift (`liveHovered`).
    pub fn set_live_hovered(&self, hovered: bool) -> bool {
        if self.live_hovered.get() == hovered {
            return false;
        }
        self.live_hovered.set(hovered);
        true
    }

    /// The live segment's hover state.
    #[must_use]
    pub fn live_hovered(&self) -> bool {
        self.live_hovered.get()
    }

    // -- TUI-15: the Zed editor context (context/editor.ts) ---------

    /// `editorSelectionKey` (context/editor.ts:20-27's identity): the
    /// file + every range's extent — the poll's dedup + the dismiss
    /// key.
    fn editor_selection_key(selection: &crate::editor_zed::ZedSelection) -> String {
        let ranges = selection
            .ranges
            .iter()
            .map(|range| {
                format!(
                    "{}:{}-{}:{}:{}",
                    range.start_line,
                    range.start_character,
                    range.end_line,
                    range.end_character,
                    range.text.chars().count()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{}|{}", selection.file_path, ranges)
    }

    /// The poll's fold (editor.ts:198-206): `Empty` clears, a fresh
    /// selection lands (a NEW key re-arms `sent`), `Unavailable` KEEPS
    /// the last (transient failures never clear).
    pub fn apply_zed_selection(&self, result: crate::editor_zed::ZedSelectionResult) {
        use crate::editor_zed::ZedSelectionResult;
        match result {
            ZedSelectionResult::Selection(selection) => {
                let changed = self.editor_selection.borrow().as_ref().is_none_or(|current| {
                    Self::editor_selection_key(current) != Self::editor_selection_key(&selection)
                });
                if changed {
                    *self.editor_selection.borrow_mut() = Some(selection);
                    self.editor_selection_sent.set(false);
                    self.request_redraw();
                }
            }
            ZedSelectionResult::Empty => {
                if self.editor_selection.borrow().is_some() {
                    *self.editor_selection.borrow_mut() = None;
                    self.editor_selection_sent.set(false);
                    self.request_redraw();
                }
            }
            ZedSelectionResult::Unavailable => {}
        }
    }

    /// `editorContext()` (index.tsx:225-228): the selection unless the
    /// clear command dismissed exactly this key.
    #[must_use]
    pub fn editor_context(&self) -> Option<crate::editor_zed::ZedSelection> {
        let selection = self.editor_selection.borrow().clone()?;
        let key = Self::editor_selection_key(&selection);
        if self.editor_dismissed_key.borrow().as_deref() == Some(key.as_str()) {
            return None;
        }
        Some(selection)
    }

    /// `labelState()` (editor.ts:343-346): no label with no selection;
    /// `true` = pending (accent), `false` = sent (muted).
    #[must_use]
    pub fn editor_label_pending(&self) -> Option<bool> {
        self.editor_context().map(|_| !self.editor_selection_sent.get())
    }

    /// `dismissEditorContext` (index.tsx:327-330): pin the current
    /// key as dismissed and clear the selection.
    pub fn editor_context_clear(&self) {
        let key = self.editor_selection.borrow().as_ref().map(Self::editor_selection_key);
        if let Some(key) = key {
            *self.editor_dismissed_key.borrow_mut() = Some(key);
        }
        *self.editor_selection.borrow_mut() = None;
        self.request_redraw();
    }

    /// `markSelectionSent` (editor.ts:339-342): the submit that
    /// attached the context flips the label to `sent`.
    pub fn editor_mark_sent(&self) {
        if self.editor_selection.borrow().is_some() && !self.editor_selection_sent.get() {
            self.editor_selection_sent.set(true);
            self.request_redraw();
        }
    }

    /// The home footer chips' rects (lane KM, auditA #10) — recorded
    /// by the home render pass, cleared when the footer hides.
    pub fn set_home_footer_chip_hits(&self, hits: Option<FooterChipHits>) {
        *self.home_footer_chip_hits.borrow_mut() = hits;
    }

    /// The home footer chips' rects from the last render.
    #[must_use]
    pub fn home_footer_chip_hits(&self) -> Option<FooterChipHits> {
        self.home_footer_chip_hits.borrow().clone()
    }

    /// The sidebar onboarding card's pointer targets, when the card
    /// painted (the session render records them at the pane's paste
    /// origin).
    pub fn set_sidebar_onboarding_hits(&self, hits: Option<SidebarOnboardingHits>) {
        *self.sidebar_onboarding_hits.borrow_mut() = hits;
    }
    /// The recorded picker entry rows (absolute cells; default while
    /// the panel is not live).
    pub fn set_picker_entry_hits(&self, hits: PickerEntryHits) {
        *self.picker_entry_hits.borrow_mut() = hits;
    }
    /// The recorded picker entry rows from the last render.
    #[must_use]
    pub fn picker_entry_hits(&self) -> PickerEntryHits {
        self.picker_entry_hits.borrow().clone()
    }
    /// The recorded plugin-section clickable rows (absolute cells;
    /// cleared when the pane unmounts).
    pub fn set_plugin_section_hits(&self, hits: PluginSectionHits) {
        *self.plugin_section_hits.borrow_mut() = hits;
    }
    /// The recorded plugin-section rows from the last render.
    #[must_use]
    pub fn plugin_section_hits(&self) -> PluginSectionHits {
        self.plugin_section_hits.borrow().clone()
    }
    /// Run one plugin section row's activate handler through the
    /// installed sink (the plugin host's `slot.click` — installed by
    /// the run loop; a missing sink is a no-op).
    pub fn plugin_section_click(&self, plugin_id: &str, row: usize) {
        let sink = self.plugin_click_sink.borrow().clone();
        if let Some(sink) = sink {
            sink(plugin_id, row);
        }
    }
    /// Install the plugin-section click sink (the host bridge).
    pub fn set_plugin_section_click_sink(&self, sink: Option<PluginSectionClickSink>) {
        *self.plugin_click_sink.borrow_mut() = sink;
    }
    /// Hover-select a picker entry (`onMouseMove` →
    /// `setStore("selected", index())`, subagents-tab.tsx:222-224):
    /// the route's selection moves without navigating. Only the
    /// session route owns the panel.
    pub fn picker_hover_select(&self, index: usize) -> bool {
        if !matches!(self.route.get(), Route::Session { .. }) {
            return false;
        }
        let mut picker = self.picker.get();
        if picker.selected == index {
            return false;
        }
        picker.selected = index;
        self.picker.set(picker);
        true
    }
    /// The recorded MCP-section targets (absolute cells; empty while
    /// the section is hidden).
    pub fn set_sidebar_mcp_hits(&self, hits: SidebarMcpHits) {
        *self.sidebar_mcp_hits.borrow_mut() = hits;
    }
    /// The recorded MCP-section targets from the last render.
    #[must_use]
    pub fn sidebar_mcp_hits(&self) -> SidebarMcpHits {
        self.sidebar_mcp_hits.borrow().clone()
    }
    /// The recorded onboarding targets (`None` while the card is
    /// hidden — the render clears them every pass).
    #[must_use]
    pub fn sidebar_onboarding_hits(&self) -> Option<SidebarOnboardingHits> {
        *self.sidebar_onboarding_hits.borrow()
    }

    /// Record the sidebar's scroll geometry (the session render, each
    /// frame — same cadence as the onboarding rects above).
    pub fn set_sidebar_scroll_geometry(
        &self,
        content_height: usize,
        viewport: usize,
        x_extent: Option<(usize, usize)>,
    ) {
        let mut state = self.sidebar_scroll.borrow_mut();
        // A hidden pane unmounts v2's scrollbox — the offset resets.
        if x_extent.is_none() {
            state.offset = 0;
        }
        state.content_height = content_height;
        state.viewport = viewport;
        state.x_extent = x_extent;
        state.offset = state.offset.min(state.max_offset());
    }

    /// The sidebar's current scroll state (render input + the wheel
    /// gate).
    #[must_use]
    pub fn sidebar_scroll(&self) -> SidebarScrollState {
        *self.sidebar_scroll.borrow()
    }

    /// Wheel-scroll the sidebar's content slot by `delta` rows
    /// (negative up); `true` when the offset moved (the redraw key).
    /// v2's scrollbox wheel is a LINE scroll (opentui's terminal
    /// wheel delta: 3 rows a notch — `direction === "up" ? -3 : 3`
    /// in the core's own embedded-terminal translation); no
    /// acceleration model (the port's wheel stream carries no
    /// timestamps to feed v2's `MacOSScrollAccel`).
    pub fn sidebar_scroll_by(&self, delta: i32) -> bool {
        let mut state = self.sidebar_scroll.borrow_mut();
        let max = state.max_offset();
        let offset = i64::try_from(state.offset).unwrap_or(i64::MAX);
        let ceiling = i64::try_from(max).unwrap_or(i64::MAX);
        let next = offset.saturating_add(i64::from(delta)).clamp(0, ceiling);
        let next = usize::try_from(next).unwrap_or(0);
        if next == state.offset {
            return false;
        }
        state.offset = next;
        true
    }

    /// The location label's hover transition (returns whether it
    /// CHANGED — the redraw key).
    pub fn set_location_hovered(&self, hovered: bool) -> bool {
        if self.location_hovered.get() == hovered {
            return false;
        }
        self.location_hovered.set(hovered);
        true
    }

    /// The location label's hover state (the render input).
    #[must_use]
    pub fn location_hovered(&self) -> bool {
        self.location_hovered.get()
    }

    #[must_use]
    pub fn transcript_hit_rows(&self) -> Vec<TranscriptHitRow> {
        self.transcript_hit_rows.borrow().clone()
    }

    // -- scroll (Wave A: the session scroll driver) ---------------------------

    /// Record the transcript metrics of the last render pass (the
    /// scroll commands' viewport/content inputs).
    pub fn set_transcript_metrics(&self, session_id: &str, metrics: TranscriptMetrics) {
        self.transcript_metrics.borrow_mut().insert(session_id.to_owned(), metrics);
    }

    #[must_use]
    pub fn transcript_metrics(&self, session_id: &str) -> TranscriptMetrics {
        self.transcript_metrics.borrow().get(session_id).copied().unwrap_or_default()
    }

    /// The session's follow state (defaults sticky — the fresh-session
    /// vscroll-stick-bottom).
    #[must_use]
    pub fn follow_state(&self, session_id: &str) -> FollowState {
        self.scroll.borrow().get(session_id).map_or(FollowState::Sticky, ScrollDriver::follow)
    }

    /// The paint offset for the session's transcript (sticky sits at
    /// the maximum scrollTop).
    #[must_use]
    pub fn scroll_offset(&self, session_id: &str) -> usize {
        let metrics = self.transcript_metrics(session_id);
        ScrollDriver::offset(
            self.follow_state(session_id),
            metrics.viewport,
            metrics.content_height,
        )
    }

    /// Apply one of v2's scroll commands (`routes/session/index.tsx:
    /// 763-805`) against the recorded metrics; `true` when the state
    /// moved (the redraw rides the request bump).
    ///
    /// TUI-TD02: the six move commands clear the message-navigation
    /// anchor (`moveTranscript`'s `clearMessageNavigation()`);
    /// TUI-TD05: `First` is the backfill jump (`session.first`'s
    /// window + `loadMore({all})`), `Last` is `toBottom()` (both
    /// window pins reset), and an up-move near the mounted top
    /// reveals the older chunk first (`revealOlderRows`).
    pub fn scroll_command(&self, session_id: &str, command: ScrollCommand) -> bool {
        match command {
            ScrollCommand::First => {
                let was_sticky = self.follow_state(session_id) == FollowState::Sticky;
                self.session_first(session_id);
                !was_sticky
            }
            ScrollCommand::Last => {
                // `toBottom()` (index.tsx:732-746).
                self.clear_message_navigation(session_id);
                self.clear_mount_window(session_id);
                let was_sticky = self.follow_state(session_id) == FollowState::Sticky;
                self.scroll.borrow_mut().insert(session_id.to_owned(), ScrollDriver::new());
                !was_sticky
            }
            step => {
                self.clear_message_navigation(session_id);
                // `revealOlderRows`' gate: the reader sits within one
                // viewport of the mounted top (`scrollTop >
                // scroll.viewport.height` bails, index.tsx:435).
                if matches!(
                    step,
                    ScrollCommand::PageUp | ScrollCommand::HalfPageUp | ScrollCommand::LineUp
                ) {
                    let metrics = self.transcript_metrics(session_id);
                    let offset = ScrollDriver::offset(
                        self.follow_state(session_id),
                        metrics.viewport,
                        metrics.content_height,
                    );
                    if offset <= metrics.viewport && self.reveal_older_rows(session_id) {
                        return true;
                    }
                }
                let metrics = self.transcript_metrics(session_id);
                let mut scroll = self.scroll.borrow_mut();
                let driver = scroll.entry(session_id.to_owned()).or_default();
                let before = driver.follow();
                driver.scroll(command, metrics.viewport, metrics.content_height);
                driver.follow() != before
            }
        }
    }

    /// An append transition (a streaming part grew the transcript):
    /// sticky follows the tail, free keeps its offset.
    pub fn scroll_appended(&self, session_id: &str, new_content_height: usize) {
        let viewport = self.transcript_metrics(session_id).viewport;
        let maximum = new_content_height.saturating_sub(viewport);
        let mut scroll = self.scroll.borrow_mut();
        scroll.entry(session_id.to_owned()).or_default().appended(maximum);
    }

    // -- TUI-TD02: message navigation (message-navigation.ts) --------

    /// Record the transcript's per-message row geometry (the render
    /// pass calls this every frame — `anchors.messagePositions()`).
    pub fn set_message_positions(&self, session_id: &str, positions: Vec<MessagePosition>) {
        self.message_positions.borrow_mut().insert(session_id.to_owned(), positions);
    }

    /// The recorded positions of the last render pass.
    #[must_use]
    pub fn message_positions(&self, session_id: &str) -> Vec<MessagePosition> {
        self.message_positions.borrow().get(session_id).cloned().unwrap_or_default()
    }

    /// `navigationMessage()` — the message the nav walk last aligned
    /// to (drives sticky-off + the next step's anchor).
    #[must_use]
    pub fn navigation_message(&self, session_id: &str) -> Option<String> {
        self.navigation_message.get().get(session_id).cloned()
    }

    /// `clearMessageNavigation()` (index.tsx:733): the scroll commands
    /// and `toBottom` reset the walk's anchor.
    pub fn clear_message_navigation(&self, session_id: &str) {
        self.navigation_message.update(|state| {
            state.remove(session_id);
        });
    }

    /// TUI-TD05 — full mount for the navigation walk (`ensureAllRows`,
    /// index.tsx:469-486): pin the window to EVERYTHING (hidden 0, no
    /// end pin), then resolve the pending intent against the NEXT
    /// render's fresh positions.
    fn ensure_all_rows(&self, session_id: &str, intent: PendingNavigation) {
        self.set_mount_window(
            session_id,
            crate::components::message::list::MountWindow { hidden: Some(0), end: None },
        );
        self.pending_navigation.borrow_mut().insert(session_id.to_owned(), intent);
    }

    /// `session.message.next` / `.previous` (+ the user-only pair,
    /// index.tsx:1101-1128): stage the boundary walk. The first press
    /// with a windowed transcript mounts everything (v2's
    /// `ensureAllRows`); the render pass resolves the step.
    pub fn navigate_message(&self, session_id: &str, direction: NavDirection, user_only: bool) {
        self.ensure_all_rows(
            session_id,
            PendingNavigation::Step { forward: direction == NavDirection::Next, user_only },
        );
    }

    /// `session.messages_last_user` (alt+end, index.tsx:1084-1100):
    /// jump to the most recent user message with text.
    pub fn navigate_last_user(&self, session_id: &str) {
        let Some(target) = self
            .messages(session_id)
            .iter()
            .rev()
            .find(|message| {
                matches!(message, crate::components::message::model::Message::User { text, .. }
                    if !text.trim().is_empty())
            })
            .map(|message| message.id().to_owned())
        else {
            return;
        };
        self.ensure_all_rows(session_id, PendingNavigation::Jump { id: target });
    }

    /// The candidate walk (`findMessageBoundary`,
    /// message-navigation.ts:18-48): user messages with text are
    /// ALWAYS candidates; assistants join when `user_only` is false
    /// and they own a non-empty text part. Returns the target's id +
    /// align top (assistant rows align one row above, index.tsx:691).
    #[allow(clippy::type_complexity)]
    fn message_boundary(
        &self,
        session_id: &str,
        forward: bool,
        user_only: bool,
    ) -> Option<(String, usize)> {
        use crate::components::message::model::{Message, Part};
        let messages = self.messages(session_id);
        let positions = self.message_positions(session_id);
        let top_of = |id: &str, assistant: bool| -> Option<(String, usize)> {
            let y = positions.iter().find(|pos| pos.id == id).map(|pos| pos.y)?;
            Some((id.to_owned(), if assistant { y.saturating_sub(1) } else { y }))
        };
        let mut candidates: Vec<(String, usize)> = Vec::new();
        for message in &messages {
            match message {
                Message::User { id, text, .. } if !text.trim().is_empty() => {
                    if let Some(boundary) = top_of(id, false) {
                        candidates.push(boundary);
                    }
                }
                Message::Assistant { id, parts, .. } if !user_only => {
                    let has_text = parts.iter().any(
                        |part| matches!(part, Part::Text { text, .. } if !text.trim().is_empty()),
                    );
                    if has_text {
                        if let Some(boundary) = top_of(id, true) {
                            candidates.push(boundary);
                        }
                    }
                }
                _ => {}
            }
        }
        if candidates.is_empty() {
            return None;
        }
        let offset = self.scroll_offset(session_id);
        match self.navigation_message(session_id) {
            Some(current) => {
                let index =
                    candidates.iter().position(|(id, _)| id.as_str() == current.as_str())?;
                let next = if forward { index + 1 } else { index.saturating_sub(1) };
                candidates.get(next).cloned()
            }
            None => {
                if forward {
                    candidates.into_iter().find(|(_, y)| *y > offset)
                } else {
                    candidates.into_iter().rev().find(|(_, y)| *y < offset)
                }
            }
        }
    }

    /// Resolve the pending navigation intent after a render pass
    /// recorded fresh positions (`ensureAllRows`' continuation — the
    /// session renderer calls this every frame). `alignMessage`
    /// (index.tsx:673-688): sticky off, the anchor set, the offset
    /// pinned at the target's top.
    pub fn resolve_pending_navigation(&self, session_id: &str) -> bool {
        let Some(intent) = self.pending_navigation.borrow_mut().remove(session_id) else {
            return false;
        };
        let target = match intent {
            PendingNavigation::Step { forward, user_only } => {
                self.message_boundary(session_id, forward, user_only)
            }
            PendingNavigation::Jump { id } => {
                let assistant = matches!(
                    self.messages(session_id).iter().find(|message| message.id() == id.as_str()),
                    Some(crate::components::message::model::Message::Assistant { .. })
                );
                self.message_positions(session_id).iter().find(|pos| pos.id == id).map(|pos| {
                    (id.clone(), if assistant { pos.y.saturating_sub(1) } else { pos.y })
                })
            }
        };
        let Some((id, top)) = target else {
            return false;
        };
        let metrics = self.transcript_metrics(session_id);
        let maximum = metrics.content_height.saturating_sub(metrics.viewport);
        self.navigation_message.update(|state| {
            state.insert(session_id.to_owned(), id);
        });
        self.scroll.borrow_mut().insert(
            session_id.to_owned(),
            ScrollDriver::from_follow(FollowState::Free { offset: top.min(maximum), maximum }),
        );
        true
    }

    // -- TUI-TD05: the mount window + history pagination --------------

    /// The session's mount-window pins (`hiddenRows()` /
    /// `visibleRowsEnd()`).
    #[must_use]
    pub fn mount_window(&self, session_id: &str) -> crate::components::message::list::MountWindow {
        self.mount_windows.borrow().get(session_id).copied().unwrap_or_default()
    }

    /// Pin the mount window (`setHiddenRows` / `setVisibleRowsEnd`).
    pub fn set_mount_window(
        &self,
        session_id: &str,
        window: crate::components::message::list::MountWindow,
    ) {
        self.mount_windows.borrow_mut().insert(session_id.to_owned(), window);
    }

    /// `toBottom()`'s reset (index.tsx:734-737): both pins clear — the
    /// tail budget rules again.
    pub fn clear_mount_window(&self, session_id: &str) {
        self.mount_windows.borrow_mut().remove(session_id);
    }

    /// Record the older-history cursor after a page fetch
    /// (`messageCursor`, client/solid/data.ts:1652).
    pub fn set_message_cursor(&self, session_id: &str, cursor: Option<String>) {
        self.message_cursors.borrow_mut().insert(session_id.to_owned(), cursor);
    }

    /// `data.session.message.more()` — `true` while an older page
    /// remains (`cursor.next` present).
    #[must_use]
    pub fn has_more_history(&self, session_id: &str) -> bool {
        self.message_cursors.borrow().get(session_id).is_some_and(Option::is_some)
    }

    /// The older-page request ([`loadMore`]): the sink carries the
    /// current cursor; `all` walks every remaining page
    /// (`session.first`'s jump-through-history, index.tsx:830-836).
    pub fn load_older_history(&self, session_id: &str, all: bool) {
        if let Some(cursor) = self.message_cursors.borrow().get(session_id).cloned().flatten() {
            self.transcript_sink.borrow().load_older(session_id, &cursor, all);
        }
    }

    /// `revealOlderRows` (index.tsx:430-447): reveal one backfill
    /// chunk above the window; `true` when the mounted span grew (the
    /// scroll step then skips — v2 keeps the viewport via scrollBy).
    pub fn reveal_older_rows(&self, session_id: &str) -> bool {
        let rows = self.transcript_metrics(session_id).rows;
        if rows == 0 {
            return false;
        }
        let current = self.mount_window(session_id).hidden.unwrap_or_else(|| {
            crate::components::message::list::rows_before(
                rows,
                crate::components::message::list::TRANSCRIPT_TAIL_ROWS,
            )
        });
        if current == 0 {
            // The top is mounted: older history comes off the server.
            if self.has_more_history(session_id) {
                self.load_older_history(session_id, false);
                return true;
            }
            return false;
        }
        let hidden =
            current.saturating_sub(crate::components::message::list::TRANSCRIPT_BACKFILL_CHUNK);
        self.set_mount_window(
            session_id,
            crate::components::message::list::MountWindow { hidden: Some(hidden), end: None },
        );
        true
    }

    /// `session.first` (index.tsx:790-873): clear the navigation, mount
    /// from row 0 with the backfill budget, jump the offset to the
    /// top — and when older pages remain, fetch them ALL first (the
    /// `loadMore({all: true})` jump-through-history).
    pub fn session_first(&self, session_id: &str) {
        self.clear_message_navigation(session_id);
        if self.has_more_history(session_id) {
            self.load_older_history(session_id, true);
        }
        let end = Some(crate::components::message::list::TRANSCRIPT_BACKFILL_CHUNK);
        self.set_mount_window(
            session_id,
            crate::components::message::list::MountWindow { hidden: Some(0), end },
        );
        let metrics = self.transcript_metrics(session_id);
        let maximum = metrics.content_height.saturating_sub(metrics.viewport);
        self.scroll.borrow_mut().insert(
            session_id.to_owned(),
            ScrollDriver::from_follow(FollowState::Free { offset: 0, maximum }),
        );
    }

    // -- TUI-TD03: the parent route -----------------------------------

    /// `session.parent` (index.tsx:1272-1288): the routed session's
    /// parent id from the family registry (`session()?.parentID`);
    /// `None` off the session route or at a root.
    #[must_use]
    pub fn parent_session_id(&self, session_id: &str) -> Option<String> {
        self.session_index
            .borrow()
            .get(session_id)
            .and_then(|info| info.parent_id.clone())
            .filter(|parent| !parent.is_empty())
    }

    /// `session.parent`'s run body: route into the parent session.
    pub fn navigate_parent(&self, session_id: &str) -> bool {
        match self.parent_session_id(session_id) {
            Some(parent) => {
                self.navigate(Route::Session { session_id: parent });
                true
            }
            None => false,
        }
    }

    // -- interrupt (Wave A: the 2-press escape machine) ------------------------

    /// The interrupt sink (production POSTs `session.interrupt` with
    /// `resume: true`).
    pub fn set_interrupt_sink(&self, sink: Rc<dyn InterruptSink>) {
        *self.interrupt_sink.borrow_mut() = sink;
    }

    /// Whether the session's turn is RUNNING (the interrupt gate, v2's
    /// `enabled: status() === "running"`): the trailing message is
    /// either a submitted user prompt (no assistant yet) or an
    /// assistant message whose finish is absent / tool-calls /
    /// unknown with no `session.idle` marker after it.
    #[must_use]
    pub fn session_running(&self, session_id: &str) -> bool {
        use crate::components::message::model::Message;
        let messages = self.messages(session_id);
        // A trailing `session.idle` marker CLOSES the turn (the
        // marker is the boundary; earlier messages stay earlier).
        if matches!(messages.last(), Some(Message::Idle { .. })) {
            return false;
        }
        let Some(last) =
            messages.iter().rev().find(|message| !matches!(message, Message::Idle { .. }))
        else {
            return false;
        };
        match last {
            Message::User { .. } => true,
            Message::Assistant { finish, error, .. } => {
                error.is_none()
                    && finish
                        .as_deref()
                        .is_none_or(|finish| ["tool-calls", "unknown"].contains(&finish))
            }
            _ => false,
        }
    }

    /// The armed hint (v2 prompt/index.tsx:141: armed shows "again to
    /// interrupt"): `Some(true)` armed, `Some(false)` = running but
    /// not armed, `None` = not running.
    #[must_use]
    pub fn interrupt_hint_armed(&self, session_id: &str, now_ms: u64) -> Option<bool> {
        if !self.session_running(session_id) {
            return None;
        }
        let armed = self
            .interrupts
            .get()
            .get(session_id)
            .is_some_and(|arm| arm.count > 0 && now_ms < arm.expires_at);
        Some(armed)
    }

    /// One escape press on the session (prompt/index.tsx:513-527):
    /// arm on the first press (5000 ms window), fire on the second.
    pub fn interrupt_pressed(&self, session_id: &str, now_ms: u64) -> InterruptStep {
        if !self.session_running(session_id) {
            return InterruptStep::Ignored;
        }
        let mut state = self.interrupts.get();
        let arm = state.get(session_id).copied().unwrap_or_default();
        // An expired arm starts over.
        let count = if arm.count > 0 && now_ms < arm.expires_at { arm.count } else { 0 };
        if count == 0 {
            state.insert(
                session_id.to_owned(),
                InterruptArm { count: 1, expires_at: now_ms + INTERRUPT_ARM_WINDOW_MS },
            );
            self.interrupts.set(state);
            return InterruptStep::Armed;
        }
        state.insert(session_id.to_owned(), InterruptArm::default());
        self.interrupts.set(state);
        self.interrupt_sink.borrow().interrupt(session_id);
        InterruptStep::Fired
    }

    /// Clear the arm (the turn ended — the hint row collapses).
    pub fn interrupt_clear(&self, session_id: &str) {
        self.interrupts.update(|state| {
            state.remove(session_id);
        });
    }

    // -- SURF-1: revert / pending compactions / history loading / tool
    // disclosure / MCP --------------------------------------------------

    /// The session's staged revert (the `RevertMessage` banner's
    /// source, index.tsx:1413-1418).
    #[must_use]
    pub fn revert(&self, session_id: &str) -> Option<RevertState> {
        self.reverts.get().get(session_id).cloned()
    }

    /// Install the revert sink (the production POST
    /// `session.revert.clear`).
    pub fn set_revert_sink(&self, sink: Rc<dyn RevertSink>) {
        *self.revert_sink.borrow_mut() = sink;
    }

    /// The message-actions revert's server POST (the 7.3 seam, now
    /// wired: `session.revert.stage` through the session-action sink).
    pub fn session_action_sink_revert_stage(&self, session_id: &str, message_id: &str) {
        self.session_action_sink.borrow().revert_stage(session_id, message_id);
    }

    /// The banner's dismiss click (index.tsx:2105-2111): fire the
    /// clear API and drop the local state (the server's
    /// `session.revert.cleared` echo is the consistency net).
    pub fn revert_dismiss(&self, session_id: &str) {
        self.revert_sink.borrow().clear(session_id);
        self.reverts.update(|reverts| {
            reverts.remove(session_id);
        });
    }

    // -- Lane FINAL: the integration connect machine ---------------------

    /// Install the integration sink (the production loop-fetcher
    /// round-trips).
    pub fn set_integration_sink(&self, sink: Rc<dyn IntegrationSink>) {
        *self.integration_sink.borrow_mut() = sink;
    }

    /// Install the session-action sink.
    pub fn set_session_action_sink(&self, sink: Rc<dyn SessionActionSink>) {
        *self.session_action_sink.borrow_mut() = sink;
    }

    /// The integration list's current rows (`integrationOptions`'s
    /// sorted shape — set by the loop fetcher).
    #[must_use]
    pub fn integration_list(&self) -> IntegrationListState {
        self.integration_list.get()
    }

    /// The live attempt (code entry's URL/instructions + the pending
    /// view's poll status).
    #[must_use]
    pub fn integration_attempt(&self) -> Option<IntegrationAttempt> {
        self.integration_attempt.get()
    }

    /// The key/code prompt's inline error line.
    #[must_use]
    pub fn integration_error(&self) -> Option<String> {
        self.integration_error.get()
    }

    /// `provider.connect`'s run (app.tsx:950-955): refetch + mount the
    /// list dialog (`DialogIntegration`).
    pub fn integration_dialog_open(&self) {
        self.integration_list.set(IntegrationListState::Loading);
        self.integration_sink.borrow().list();
        self.replace_dialog(DialogKind::Integration, DialogSize::Medium);
    }

    /// Refetch the integration list through the sink (the credential
    /// events' resync).
    pub fn integration_sink_list(&self) {
        self.integration_sink.borrow().list();
    }

    /// The loop fetcher's `integration.list` result.
    pub fn integration_list_loaded(&self, rows: Vec<IntegrationRow>) {
        self.integration_list.set(IntegrationListState::Loaded(rows));
    }

    /// A failed `integration.list`.
    pub fn integration_list_failed(&self, error: String) {
        self.integration_list.set(IntegrationListState::Failed(error));
    }

    // -- TUI-5 (lane-tui2): the Open menu's project registry -------

    /// Swap the project-list sink.
    pub fn set_project_sink(&mut self, sink: Rc<dyn ProjectSink>) {
        *self.project_sink.borrow_mut() = sink;
    }

    /// The project registry (the Open menu's Projects section).
    #[must_use]
    pub fn project_list(&self) -> ListState<crate::components::dialog_open::ProjectRow> {
        self.project_list.get()
    }

    /// `open.menu`'s run (`app.tsx:763-774` + `dialog-open.tsx:56-86`):
    /// refetch the sessions recents AND `data.project.sync()`, then
    /// mount `DialogOpen` at the large size.
    pub fn open_menu_pressed(&self) {
        self.project_list.set(ListState::Loading);
        self.project_sink.borrow().list();
        self.request_session_list_refresh();
        self.replace_dialog(DialogKind::Open, DialogSize::Large);
    }

    /// Remount `DialogOpen` WITHOUT refetching (the worktrees view's
    /// `back()` — v2 restores the projects snapshot in place).
    pub fn open_menu_back(&self) {
        self.replace_dialog(DialogKind::Open, DialogSize::Large);
    }

    /// The right-arrow view swap (`Show project worktrees`,
    /// dialog-open.tsx:388-405): scope the worktree state + fetch the
    /// project's list, then mount the `{Project} / Worktrees` view.
    pub fn open_menu_worktrees(&self, project_id: &str, project_name: &str) {
        self.set_worktree_project(project_id, None);
        self.worktree_sink_list(project_id);
        self.replace_dialog(
            DialogKind::OpenWorktrees {
                project_id: project_id.to_owned(),
                project_name: project_name.to_owned(),
            },
            DialogSize::Large,
        );
    }

    /// The loop fetcher's `project.list` result.
    pub fn project_list_loaded(&self, rows: Vec<crate::components::dialog_open::ProjectRow>) {
        self.project_list.set(ListState::Loaded(rows));
    }

    /// A failed `project.list`.
    pub fn project_list_failed(&self, error: String) {
        self.project_list.set(ListState::Failed(error));
    }

    /// One list row (the downstream dialogs' source).
    #[must_use]
    pub fn integration_row(&self, id: &str) -> Option<IntegrationRow> {
        match self.integration_list.get() {
            IntegrationListState::Loaded(rows) => rows.into_iter().find(|row| row.id == id),
            _ => None,
        }
    }

    /// A list row's `onSelect` (dialog-integration.tsx:122-125):
    /// credentials present → the manage view; else the method choice.
    pub fn integration_select(&self, id: &str) {
        let Some(row) = self.integration_row(id) else { return };
        if row.accounts.is_empty() {
            self.integration_open_methods(id);
        } else {
            self.integration_deleting.set(None);
            self.replace_dialog(
                DialogKind::IntegrationAccounts { integration_id: id.to_owned() },
                DialogSize::Medium,
            );
        }
    }

    /// `selectMethod` (dialog-integration.tsx:265-283): a single
    /// method opens it directly; multiple mount the choice.
    pub fn integration_open_methods(&self, id: &str) {
        let Some(row) = self.integration_row(id) else { return };
        if row.methods.len() == 1 {
            self.integration_method_open(id, &row.methods[0]);
        } else {
            self.replace_dialog(
                DialogKind::IntegrationMethod { integration_id: id.to_owned() },
                DialogSize::Medium,
            );
        }
    }

    /// `openMethod` (dialog-integration.tsx:285-303): key → the API-key
    /// prompt; command → the pending view; oauth → `oauth.connect`.
    pub fn integration_method_open(&self, id: &str, method: &IntegrationMethod) {
        self.integration_error.set(None);
        match method.kind {
            "key" => {
                let title = if method.label.is_empty() {
                    let name = self.integration_row(id).map_or_else(String::new, |row| row.name);
                    format!("Connect {name}")
                } else {
                    method.label.clone()
                };
                self.replace_dialog(
                    DialogKind::IntegrationKey {
                        integration_id: id.to_owned(),
                        method_id: method.id.clone(),
                        title,
                    },
                    DialogSize::Medium,
                );
                self.update_dialog(|top| top.value.clear());
            }
            "oauth" => {
                self.integration_sink.borrow().oauth_connect(id, &method.id);
            }
            "command" => {
                self.integration_sink.borrow().command_connect(id, &method.id);
            }
            _ => {}
        }
    }

    /// `KeyMethod`'s confirm (dialog-integration.tsx:489-500): empty
    /// keys never submit (the `DialogPrompt` guard); the POST rides the
    /// sink, the result lands through the loop fetcher.
    pub fn integration_key_submit(&self, id: &str, key: &str) {
        if key.is_empty() {
            return;
        }
        self.integration_sink.borrow().connect_key(id, key);
    }

    /// `OAuthStarting`'s result (dialog-integration.tsx:548-568): a
    /// code attempt mounts the code prompt; auto mounts the pending
    /// view (the worker polls).
    pub fn integration_oauth_started(&self, attempt: IntegrationAttempt) {
        self.integration_error.set(None);
        let code_mode = attempt.code_mode;
        let id = attempt.integration_id.clone();
        self.integration_attempt.set(Some(attempt));
        if code_mode {
            self.replace_dialog(
                DialogKind::IntegrationCode { integration_id: id },
                DialogSize::Medium,
            );
            self.update_dialog(|top| top.value.clear());
        } else {
            self.replace_dialog(
                DialogKind::IntegrationPending { integration_id: id },
                DialogSize::Large,
            );
        }
    }

    /// The pending view's poll status message (`setOutput` /
    /// `CommandPending`'s status.message).
    pub fn integration_attempt_output(&self, message: &str) {
        self.integration_attempt.update(|attempt| {
            if let Some(attempt) = attempt.as_mut() {
                message.clone_into(&mut attempt.instructions);
            }
        });
    }

    /// `OAuthCode`'s confirm (`oauth.complete`, :704-718).
    pub fn integration_code_submit(&self, id: &str, attempt_id: &str, code: &str) {
        if code.is_empty() {
            return;
        }
        self.integration_sink.borrow().oauth_complete(id, attempt_id, code);
    }

    /// The accounts view's activate (`credential.activate`,
    /// dialog-integration.tsx:206).
    pub fn integration_account_activate(&self, credential_id: &str) {
        self.integration_sink.borrow().credential_activate(credential_id);
    }

    /// The accounts view's two-press delete (`dialog.integration.
    /// delete`, :237-258): the first press arms, the second fires
    /// `credential.remove`.
    pub fn integration_account_delete(&self, credential_id: &str) -> bool {
        if self.integration_deleting.get().as_deref() == Some(credential_id) {
            self.integration_deleting.set(None);
            self.integration_sink.borrow().credential_remove(credential_id);
            true
        } else {
            self.integration_deleting.set(Some(credential_id.to_owned()));
            false
        }
    }

    /// The connect flow's terminal event (`connected`,
    /// dialog-integration.tsx:1007-1029): success toast, drop the
    /// attempt, refetch the list (v2 also invalidates model/provider —
    /// the worker's Connected arm re-emits those).
    pub fn integration_connected(&self, name: &str) {
        self.integration_attempt.set(None);
        self.integration_error.set(None);
        self.integration_sink.borrow().list();
        self.clear_dialogs();
        self.toast_show(crate::components::toast::ToastOptions {
            message: format!("Connected {name}"),
            variant: "success",
            ..crate::components::toast::ToastOptions::info("")
        });
    }

    /// A failed connect step: the inline error keeps the prompt open
    /// (`setError(errorMessage)`); failures OUTSIDE a prompt (the
    /// start/poll arms) toast and clear (`toast.show` + `dialog.
    /// clear`).
    pub fn integration_failed(&self, error: &str) {
        let in_prompt = self.top_dialog().is_some_and(|entry| {
            matches!(
                entry.kind,
                DialogKind::IntegrationKey { .. } | DialogKind::IntegrationCode { .. }
            )
        });
        if in_prompt {
            self.integration_error.set(Some(error.to_owned()));
            return;
        }
        self.integration_attempt.set(None);
        self.clear_dialogs();
        self.toast_show(crate::components::toast::ToastOptions {
            message: error.to_owned(),
            variant: "error",
            ..crate::components::toast::ToastOptions::info("")
        });
    }

    /// Esc while an attempt is live: the `onCleanup` cancel
    /// (dialog-integration.tsx:654-661, 691-698).
    pub fn integration_cancel_active(&self) {
        if let Some(attempt) = self.integration_attempt.get() {
            self.integration_sink.borrow().cancel(
                &attempt.integration_id,
                &attempt.attempt_id,
                attempt.kind,
            );
            self.integration_attempt.set(None);
        }
    }

    // -- Lane FINAL: the six session commands + location.reload ---------

    /// `session.compact`'s run (index.tsx:952-967): fire the POST with
    /// the CURRENT model selection, clear the dialog; failures toast
    /// through the loop fetcher's result event.
    pub fn session_compact_pressed(&self) {
        let Route::Session { session_id } = self.route() else { return };
        let model = self.selection_for(&session_id).model;
        self.session_action_sink.borrow().compact(&session_id, model.as_ref());
        self.clear_dialogs();
    }

    /// `session.undo`'s run (index.tsx:974-999): the LAST user message
    /// with text before the revert boundary; a miss toasts "Nothing
    /// to undo"; a hit fires interrupt→wait→revert.stage and seeds
    /// the composer with the message text.
    pub fn session_undo_pressed(&self) {
        let Route::Session { session_id } = self.route() else { return };
        // `messagesBeforeRevert`: with a staged boundary the visible
        // transcript ENDS before `revert.messageID`.
        let boundary = self.revert(&session_id).map(|state| state.message_id);
        let messages = self.messages(&session_id);
        let visible = match &boundary {
            Some(id) => {
                let end = messages.iter().position(|message| message.id() == id).unwrap_or(0);
                &messages[..end]
            }
            None => messages.as_slice(),
        };
        let target = visible.iter().rev().find_map(|message| match message {
            crate::components::message::model::Message::User { id, text, .. }
                if !text.trim().is_empty() =>
            {
                Some((id.clone(), text.clone()))
            }
            _ => None,
        });
        let Some((message_id, text)) = target else {
            self.clear_dialogs();
            self.toast_show(crate::components::toast::ToastOptions {
                message: "Nothing to undo".to_owned(),
                variant: "error",
                duration_ms: 3000,
                ..crate::components::toast::ToastOptions::info("")
            });
            return;
        };
        self.session_action_sink.borrow().revert_stage(&session_id, &message_id);
        if !text.is_empty() {
            let mut composer = self.composer(&session_id);
            composer.set_text(&text);
            self.set_composer(&session_id, &composer);
        }
        self.clear_dialogs();
    }

    /// `session.redo`'s run (index.tsx:1007-1016): `revert.clear`
    /// through the existing revert sink; errors toast (the loop's
    /// result event carries them — the banner path shares the POST).
    pub fn session_redo_pressed(&self) {
        let Route::Session { session_id } = self.route() else { return };
        self.revert_dismiss(&session_id);
        self.clear_dialogs();
    }

    /// `session.background`'s run (index.tsx:1250-1253).
    pub fn session_background_pressed(&self) {
        let Route::Session { session_id } = self.route() else { return };
        self.session_action_sink.borrow().background(&session_id);
        self.clear_dialogs();
    }

    /// `DialogFork`'s submit (dialog-fork.tsx:29-47): fire `session.
    /// fork` (before = the picked user message); the loop fetcher's
    /// result opens the tab + seeds the prompt.
    pub fn session_fork_submit(&self, session_id: &str, before: Option<&str>) {
        self.session_action_sink.borrow().fork(session_id, before);
    }

    /// The fork result (`route.navigate` + the projected prompt +
    /// "Forked session" toast, dialog-fork.tsx:36-46).
    pub fn session_forked(&self, new_session_id: &str, prompt_text: Option<&str>) {
        self.open_tab(new_session_id, None);
        self.navigate(Route::Session { session_id: new_session_id.to_owned() });
        if let Some(text) = prompt_text.filter(|text| !text.is_empty()) {
            let mut composer = self.composer(new_session_id);
            composer.set_text(text);
            self.set_composer(new_session_id, &composer);
        }
        self.clear_dialogs();
        self.toast_show(crate::components::toast::ToastOptions {
            message: "Forked session".to_owned(),
            variant: "success",
            duration_ms: 4000,
            ..crate::components::toast::ToastOptions::info("")
        });
    }

    /// `session.export`'s confirm (index.tsx:1217-1238): fire the
    /// export GET; the loop fetcher formats + delivers (clipboard
    /// write + toast, or the tmp file + result dialog).
    pub fn session_export_submit(&self, session_id: &str, format: &str, destination: &str) {
        let sanitize = format == "json" && destination == "clipboard";
        self.session_action_sink.borrow().export(session_id, sanitize, format, destination);
        self.clear_dialogs();
    }

    /// The export clipboard arm's completion toast (index.tsx:1229).
    pub fn session_export_copied(&self) {
        self.clear_dialogs();
        self.toast_show(crate::components::toast::ToastOptions {
            message: "Copied to clipboard".to_owned(),
            variant: "success",
            ..crate::components::toast::ToastOptions::info("")
        });
    }

    /// The export file arm's completion (`DialogExportResult.show`).
    pub fn session_export_written(&self, path: &str) {
        self.replace_dialog(DialogKind::ExportResult { path: path.to_owned() }, DialogSize::Medium);
    }

    /// A failed export (index.tsx:1239-1241).
    pub fn session_export_failed(&self) {
        self.clear_dialogs();
        self.toast_show(crate::components::toast::ToastOptions {
            message: "Failed to export session".to_owned(),
            variant: "error",
            ..crate::components::toast::ToastOptions::info("")
        });
    }

    /// The sessions dialog's two-press delete (`toDelete`,
    /// dialog-session-list.tsx:161-166, 255-279): the first press
    /// arms the row, the second fires `session.remove`.
    pub fn session_delete_pressed(&self, session_id: &str) -> bool {
        if self.session_delete_armed.get().as_deref() == Some(session_id) {
            self.session_delete_armed.set(None);
            self.session_action_sink.borrow().remove(session_id);
            true
        } else {
            self.session_delete_armed.set(Some(session_id.to_owned()));
            false
        }
    }

    /// Any selection move clears the armed row (`onMove`,
    /// dialog-session-list.tsx:242).
    pub fn session_delete_disarm(&self) {
        if self.session_delete_armed.get().is_some() {
            self.session_delete_armed.set(None);
        }
    }

    /// The armed row id (the title/danger render's source).
    #[must_use]
    pub fn session_delete_armed(&self) -> Option<String> {
        self.session_delete_armed.get()
    }

    /// `location.reload`'s run (app.tsx:1023-1032): the info toast +
    /// the POST; the loop fetcher's result resolves the final toast.
    pub fn location_reload_pressed(&self) {
        self.clear_dialogs();
        self.toast_show(crate::components::toast::ToastOptions {
            message: "Reloading configuration…".to_owned(),
            variant: "info",
            duration_ms: 30_000,
            ..crate::components::toast::ToastOptions::info("")
        });
        self.session_action_sink.borrow().location_reload();
    }

    /// The reload result's success toast.
    pub fn location_reload_finished(&self) {
        self.toast_show(crate::components::toast::ToastOptions {
            message: "Configuration reloaded".to_owned(),
            variant: "success",
            ..crate::components::toast::ToastOptions::info("")
        });
    }

    /// A failed reload (the 30 s info toast collapses to the error).
    pub fn location_reload_failed(&self, error: &str) {
        self.toast_show(crate::components::toast::ToastOptions {
            message: error.to_owned(),
            variant: "error",
            ..crate::components::toast::ToastOptions::info("")
        });
    }

    /// `DialogConfig`'s `change` (dialog-config.tsx:365-386): cycle one
    /// setting's values through the config sink; the live state moves
    /// with it (the four settings this build both reads and persists).
    pub fn setting_cycle(&self, id: &str, direction: i64) {
        let cycle = |current: usize, values: &[&str]| -> String {
            let count = values.len();
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let next = (current as i64 + direction).rem_euclid(count as i64) as usize;
            values[next].to_owned()
        };
        match id {
            "theme.name" => {
                let mut names = self.registry().names();
                names.sort_by_key(|name| name.to_lowercase());
                let current = self.theme_name();
                let index = names.iter().position(|name| name == &current).unwrap_or(0);
                let next = cycle(index, &names.iter().map(String::as_str).collect::<Vec<_>>());
                self.set_theme_by_name(&next);
            }
            "theme.mode" => {
                let current = self.theme_mode_pref().unwrap_or_else(|| "system".to_owned());
                let next = cycle(
                    ["system", "dark", "light"]
                        .iter()
                        .position(|mode| *mode == current)
                        .unwrap_or(0),
                    &["system", "dark", "light"],
                );
                *self.theme_mode_pref.borrow_mut() = Some(next.clone());
                let resolved = if next == "system" {
                    self.system_mode.get()
                } else if next == "light" {
                    oc_theme::types::Mode::Light
                } else {
                    oc_theme::types::Mode::Dark
                };
                self.switch_theme_mode(resolved);
                self.config_sink.borrow().update_theme_mode(&next);
            }
            "session.sidebar" => {
                let hide =
                    matches!(self.sidebar_mode(), crate::components::sidebar::SidebarMode::Auto);
                self.set_sidebar_mode(if hide {
                    crate::components::sidebar::SidebarMode::Hide
                } else {
                    crate::components::sidebar::SidebarMode::Auto
                });
            }
            "session.grouping" => {
                let next = self.toggle_exploration_grouping();
                self.config_sink.borrow().update_session_grouping(next);
            }
            // TUI-23: every other row of the full table delegates to
            // the dialog-config cycle (value list / numeric step, the
            // live consumer, the snapshot + the generic config sink
            // write-through).
            other => {
                if let Some(setting) = crate::components::dialog_config::settings_table()
                    .into_iter()
                    .find(|setting| setting.path == other)
                {
                    let _ =
                        crate::components::dialog_config::change_setting(self, &setting, direction);
                }
            }
        }
        self.request_redraw();
    }

    /// The queued-compaction inbox ids (the `◇ Compaction queued`
    /// divider rows render one each, rows.ts:72-79).
    #[must_use]
    pub fn pending_compactions(&self, session_id: &str) -> Vec<String> {
        self.pending_compactions.get().get(session_id).cloned().unwrap_or_default()
    }

    /// Seed the pending-compaction queue (tests + the pending.sync
    /// snapshot seam).
    pub fn set_pending_compactions(&self, session_id: &str, ids: Vec<String>) {
        self.pending_compactions.update(|queues| {
            queues.insert(session_id.to_owned(), ids);
        });
    }

    /// The transcript fetch is in flight for this session — the jump
    /// row shows "Loading session history…" (index.tsx:1425-1427).
    #[must_use]
    pub fn history_loading(&self, session_id: &str) -> bool {
        self.history_loading.get().get(session_id).copied().unwrap_or(false)
    }

    pub fn set_history_loading(&self, session_id: &str, loading: bool) {
        self.history_loading.update(|map| {
            map.insert(session_id.to_owned(), loading);
        });
    }

    /// A tool detail cell's inline disclosure (`{mid}:{pid}` key).
    #[must_use]
    pub fn tool_expanded(&self, key: &str) -> bool {
        self.expanded_tools.get().contains(key)
    }

    pub fn toggle_tool_expanded(&self, key: &str) {
        self.expanded_tools.update(|set| {
            if !set.remove(key) {
                set.insert(key.to_owned());
            }
        });
    }

    /// The expanded tool keys (the transcript render's options feed).
    #[must_use]
    pub fn expanded_tool_keys(&self) -> std::collections::HashSet<String> {
        self.expanded_tools.get()
    }

    /// The MCP server rows (`location.mcp.server.list`).
    #[must_use]
    pub fn mcp_servers(&self) -> Vec<McpServerRow> {
        self.mcp_servers.get()
    }

    pub fn set_mcp_servers(&self, rows: Vec<McpServerRow>) {
        // Toast once when a server ENTERS a failed/needs-auth state
        // (app.tsx:522-545 — lane KM): per-server alerted-status
        // tracking avoids re-toasting the same problem on every
        // refresh while still re-alerting if the state changes. The
        // action dispatches `mcp.list` (`keymap.dispatch` in v2's
        // `action.run`, :534/:541).
        let mut alerts: std::collections::HashMap<String, String> =
            self.mcp_alerted.borrow_mut().take().unwrap_or_default();
        for server in &rows {
            if server.status != "failed" && server.status != "needs_auth" {
                alerts.remove(&server.name);
                continue;
            }
            if alerts.get(&server.name).map(String::as_str) == Some(server.status) {
                continue;
            }
            alerts.insert(server.name.clone(), server.status.to_owned());
            let toast = if server.status == "needs_auth" {
                crate::components::toast::ToastOptions {
                    title: Some("MCP server needs authentication".to_owned()),
                    message: format!("Connect \"{}\" to use its tools.", server.name),
                    variant: "warning",
                    ..crate::components::toast::ToastOptions::info("")
                }
            } else {
                crate::components::toast::ToastOptions {
                    title: Some(format!("MCP server failed: {}", server.name)),
                    message: "Run /mcps to view details.".to_owned(),
                    variant: "error",
                    ..crate::components::toast::ToastOptions::info("")
                }
            };
            self.toast_show(toast.with_action("Open MCP servers", "mcp.list"));
        }
        *self.mcp_alerted.borrow_mut() = Some(alerts);
        self.mcp_servers.set(rows);
    }

    /// The plugins-failed count (the home footer chip's source).
    #[must_use]
    pub fn plugins_failed(&self) -> usize {
        self.plugins_failed.get()
    }

    /// Set the plugins-failed count (the boot resolution's failures).
    pub fn set_plugins_failed(&self, count: usize) {
        self.plugins_failed.set(count);
    }

    /// The MCP section's collapse toggle (`storage.store("view")`,
    /// mcp.tsx:19 — default open; only meaningful past two servers).
    #[must_use]
    pub fn sidebar_mcp_open(&self) -> bool {
        self.sidebar_mcp_open.get()
    }

    pub fn set_sidebar_mcp_open(&self, open: bool) {
        self.sidebar_mcp_open.set(open);
    }

    // -- diff viewer (SURF-2: the `diff` plugin route) ----------------------

    /// The diff viewer's live state (render + dispatcher input).
    #[must_use]
    pub fn diff_viewer(&self) -> DiffViewerLive {
        self.diff_viewer.get()
    }

    /// Whether the diff viewer currently owns the screen + the `diff`
    /// key layer.
    #[must_use]
    pub fn diff_viewer_open(&self) -> bool {
        self.diff_viewer.get().open
    }

    /// `diff.open` (diff-viewer.tsx:1198-1226): capture the return
    /// route + session, mark loading, kick the fetch, clear dialogs.
    pub fn open_diff_viewer(&self, default_mode: Option<DiffSource>) {
        let (return_route, session_id) = match self.route() {
            Route::Session { session_id } => {
                (Route::Session { session_id: session_id.clone() }, Some(session_id))
            }
            Route::Home => (Route::Home, None),
        };
        let generation = self.diff_viewer.get().generation.wrapping_add(1);
        let mut mode = self.diff_viewer.get().mode;
        if !self.diff_viewer.get().open {
            // Fresh open: the sticky memory mode or the caller's
            // default (`initialMode`, diff-viewer.tsx:103-104); "turn"
            // is only valid with a session.
            mode = default_mode.unwrap_or(mode);
        }
        if mode == DiffSource::Turn && session_id.is_none() {
            mode = DiffSource::Branch;
        }
        self.diff_viewer.set(DiffViewerLive {
            open: true,
            return_route,
            session_id: session_id.clone(),
            mode,
            phase: DiffPhase::Loading,
            base: None,
            viewer: crate::components::diff_viewer::DiffViewerState::default(),
            scroll: 0,
            generation,
        });
        self.clear_dialogs();
        let sink = self.diff_sink.borrow().clone();
        sink.fetch(mode, session_id, generation);
    }

    /// `diff.close` (diff-viewer.tsx:554-557): unmount + navigate back
    /// to the captured return route.
    pub fn close_diff_viewer(&self) {
        let mut live = self.diff_viewer.get();
        if !live.open {
            return;
        }
        live.open = false;
        let back = live.return_route.clone();
        self.diff_viewer.set(live);
        self.navigate(back);
    }

    /// `diff.switch_source`'s selection (the source dialog's
    /// `onSelect` → `props.onSwitchSource`, diff-viewer.tsx:748-751):
    /// sticky mode change + refetch (scroll resets).
    pub fn switch_diff_source(&self, mode: DiffSource) {
        let mut live = self.diff_viewer.get();
        if !live.open || live.mode == mode {
            return;
        }
        if mode == DiffSource::Turn && live.session_id.is_none() {
            return;
        }
        let generation = live.generation.wrapping_add(1);
        live.mode = mode;
        live.phase = DiffPhase::Loading;
        live.base = None;
        live.viewer.selected_file = None;
        live.scroll = 0;
        live.generation = generation;
        let session_id = live.session_id.clone();
        self.diff_viewer.set(live);
        let sink = self.diff_sink.borrow().clone();
        sink.fetch(mode, session_id, generation);
    }

    /// A fetch result landing (the HTTP worker's reply): stale
    /// generations drop (v2's `input !== diffInput()` guard).
    pub fn diff_fetch_result(
        &self,
        generation: u64,
        result: Result<(Option<String>, Vec<crate::components::diff_viewer::DiffFile>), String>,
    ) {
        let mut live = self.diff_viewer.get();
        if !live.open || live.generation != generation {
            return;
        }
        match result {
            Ok((base, files)) => {
                // `committed && !base` — the unavailable body
                // (diff-viewer.tsx:158-160, 861-867).
                if live.mode == DiffSource::Committed && base.is_none() {
                    live.phase = DiffPhase::Unavailable;
                } else {
                    live.base = base;
                    live.phase = DiffPhase::Loaded(files);
                    live.scroll = 0;
                }
            }
            Err(error) => {
                live.phase = DiffPhase::Failed(error);
            }
        }
        self.diff_viewer.set(live);
    }

    /// Mutate the viewer state (the `diff.*` preference commands).
    pub fn update_diff_viewer(&self, update: impl FnOnce(&mut DiffViewerLive) -> bool) -> bool {
        let mut live = self.diff_viewer.get();
        let changed = update(&mut live);
        if changed {
            self.diff_viewer.set(live);
        }
        changed
    }

    /// The diff fetch seam (tests inject; the run loop installs the
    /// HTTP worker). The closure form accepts the loop-fetcher sender.
    pub fn set_diff_sink(&self, sink: Rc<dyn DiffSink>) {
        *self.diff_sink.borrow_mut() = sink;
    }

    // -- SURF-2: pair / mcp-toggle / variant ---------------------------------

    /// `DialogPair`'s fetch result (the run loop's HTTP worker).
    pub fn set_pair_state(&self, state: PairState) {
        self.pair.set(state);
    }

    /// `DialogPair`'s live state.
    #[must_use]
    pub fn pair_state(&self) -> PairState {
        self.pair.get()
    }

    /// The MCP toggle seam (`mcp.connect`/`mcp.disconnect`).
    pub fn set_mcp_toggle_sink(&self, sink: Rc<dyn McpToggleSink>) {
        *self.mcp_toggle_sink.borrow_mut() = sink;
    }

    /// `DialogPair`'s fetch seam (the run loop's HTTP worker).
    pub fn set_pair_refresh_sink(&self, sink: Rc<dyn PairRefreshSink>) {
        *self.pair_refresh_sink.borrow_mut() = Some(sink);
    }

    /// The installed pair fetch seam, if any.
    #[must_use]
    pub fn pair_refresh_sink(&self) -> Option<Rc<dyn PairRefreshSink>> {
        self.pair_refresh_sink.borrow().clone()
    }

    /// `DialogMcp`'s toggle (`dialog-mcp.tsx:113-132`): connected →
    /// disconnect; failed/pending-off → connect. Loading + pending
    /// rows no-op.
    pub fn mcp_toggle(&self, server: &str) {
        let Some(row) = self.mcp_servers().into_iter().find(|row| row.name == server) else {
            return;
        };
        if row.status == "pending" {
            return;
        }
        let connect = row.status != "connected";
        let sink = self.mcp_toggle_sink.borrow().clone();
        sink.toggle(server, connect);
    }

    /// The current model's variant ids (`local.model.variant.list`,
    /// local.tsx over the catalog row) — empty when the model has no
    /// variants or the list is unavailable.
    #[must_use]
    pub fn current_model_variants(&self) -> Vec<String> {
        let selection = match self.route() {
            Route::Session { session_id } => self.selection_for(&session_id).model,
            Route::Home => self.selection_for(crate::app::HOME_PROMPT_KEY).model,
        };
        let Some(selection) = selection else { return Vec::new() };
        self.server_lists()
            .models
            .rows()
            .map(|rows| {
                rows.iter()
                    .find(|row| {
                        row.provider_id == selection.provider_id && row.id == selection.model_id
                    })
                    .map(|row| {
                        row.variants.iter().filter(|v| v.as_str() != "default").cloned().collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// `local.model.variant.set` (dialog-variant.tsx:16/26): commit
    /// the variant onto the route's selection ("default" clears it).
    pub fn set_current_model_variant(&self, variant: Option<&str>) {
        let key = match self.route() {
            Route::Session { session_id } => session_id,
            Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
        };
        let Some(mut model) = self.selection_for(&key).model else { return };
        model.variant = variant.filter(|value| *value != "default").map(str::to_owned);
        match self.route() {
            Route::Session { session_id } => self.select_model(&session_id, &model),
            Route::Home => self.stage_model(&key, model),
        }
    }

    /// Closure-based sink install (the run loop's HTTP worker).
    pub fn set_diff_sink_fn(&self, fetch: impl Fn(DiffSource, Option<String>, u64) + 'static) {
        struct FnDiffSink<F: Fn(DiffSource, Option<String>, u64)>(F);
        impl<F: Fn(DiffSource, Option<String>, u64)> DiffSink for FnDiffSink<F> {
            fn fetch(&self, mode: DiffSource, session_id: Option<String>, generation: u64) {
                (self.0)(mode, session_id, generation);
            }
        }
        *self.diff_sink.borrow_mut() = Rc::new(FnDiffSink(fetch));
    }

    // -- queued prompts (Wave A: mini/queue.rs lifted into the main store) ----

    /// `openUrl` (`ui/link.tsx:27` → `util/open.ts`): hand a URL to
    /// the system opener — `xdg-open` detached, the spawn error
    /// swallowed (`openUrl(href).catch(() => {})`). A test seam
    /// records instead of spawning (`set_open_url_sink`).
    pub fn open_url(&self, url: &str) {
        if let Some(sink) = self.open_url_sink.borrow().as_ref() {
            sink(url.to_owned());
            return;
        }
        let _ = std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    /// Swap the open-url seam (tests observe without spawning).
    pub fn set_open_url_sink(&self, sink: Option<OpenUrlSink>) {
        *self.open_url_sink.borrow_mut() = sink;
    }

    /// The queued prompts of one session (the dock + manager rows) —
    /// TUI-TD07: the server pending inbox's `delivery: "queue"` items
    /// (`queuedPrompts()`, index.tsx:212-215).
    #[must_use]
    pub fn queued_prompts(&self, session_id: &str) -> Vec<QueuedPrompt> {
        self.pending_inbox.get().get(session_id).map_or_else(Vec::new, |items| {
            items
                .iter()
                .filter(|item| item.delivery == "queue")
                .map(|item| QueuedPrompt { id: item.id.clone(), text: item.text.clone() })
                .collect()
        })
    }

    /// TUI-TD07 — the pending inbox items of one session (the
    /// pending-steer markers + the queue derive from them).
    #[must_use]
    pub fn pending_inbox(&self, session_id: &str) -> Vec<InboxItem> {
        self.pending_inbox.get().get(session_id).cloned().unwrap_or_default()
    }

    /// `pendingDelivery(messageID)` (index.tsx:211): the pending
    /// marker on an admitted user row — `Some("steer"|"queue")`.
    #[must_use]
    pub fn pending_delivery(&self, session_id: &str, message_id: &str) -> Option<String> {
        self.pending_inbox
            .get()
            .get(session_id)
            .and_then(|items| items.iter().find(|item| item.id == message_id))
            .map(|item| item.delivery.clone())
    }

    /// Swap the inbox mutation seam (tests observe the `PATCH`/`DELETE`
    /// calls instead of `POST`ing).
    pub fn set_inbox_sink(&self, sink: Rc<dyn InboxSink>) {
        *self.inbox_sink.borrow_mut() = sink;
    }

    /// Swap the older-history load seam (`loadMore`).
    pub fn set_transcript_sink(&self, sink: Rc<dyn TranscriptSink>) {
        *self.transcript_sink.borrow_mut() = sink;
    }

    /// TUI-TD07 — `<leader>return` over an ACTIVE turn (v2's queue
    /// submit, prompt/index.tsx:446-458 → `session.prompt` with
    /// `delivery: "queue"` and the CLIENT-MINTED inbox id — the SSE
    /// echo re-admits under the same id, data.ts:769-787): the
    /// optimistic local admit renders the dock immediately; the send
    /// goes out through the prompt sink carrying the delivery.
    pub fn queue_prompt(&self, session_id: &str, text: &str, files: &[String]) {
        let id = uuid::Uuid::new_v4().to_string();
        self.admit_inbox_item(
            session_id,
            InboxItem {
                id: id.clone(),
                delivery: "queue".to_owned(),
                text: text.to_owned(),
                files: files.to_vec(),
            },
        );
        let prompt = crate::components::composer::SubmittedPrompt {
            text: text.to_owned(),
            delivery: Some("queue".to_owned()),
            inbox_id: Some(id),
            ..crate::components::composer::SubmittedPrompt::default()
        };
        let model = self.resolve_model(session_id);
        self.prompt_sink.send_with_model(session_id, &prompt, model.as_ref());
    }

    /// `admitLocal` (client/solid/data.ts:370-383): upsert one inbox
    /// item into the pending registry (the transcript row projection
    /// is the event path's job).
    fn admit_inbox_item(&self, session_id: &str, item: InboxItem) {
        self.pending_inbox.update(|state| {
            let items = state.entry(session_id.to_owned()).or_default();
            match items.iter_mut().find(|existing| existing.id == item.id) {
                Some(existing) => *existing = item,
                None => items.push(item),
            }
        });
    }

    /// TUI-TD07 — `mutatePending("cancel")` (the delete action +
    /// undo's first step, index.tsx:605-623): the server DELETE
    /// (`session.inbox.cancel`) plus the optimistic local retract —
    /// `retractLocal` (data.ts:403-415) also drops the admitted
    /// transcript row; `true` when the item existed.
    pub fn delete_queued_prompt(&self, session_id: &str, id: &str) -> bool {
        let existed = self
            .pending_inbox
            .get()
            .get(session_id)
            .is_some_and(|items| items.iter().any(|item| item.id == id));
        if existed {
            self.inbox_sink.borrow().cancel(session_id, id);
            self.retract_inbox_item(session_id, id);
        }
        existed
    }

    /// TUI-TD07 — `mutatePending("steer")` (the manager's enter, the
    /// empty-enter head steer and the pending-steer "Move to queue",
    /// index.tsx:612-614/1504-1508): the server PATCH
    /// (`session.inbox.update {delivery: "steer"}`) plus the
    /// optimistic local delivery flip; `true` when the item existed.
    pub fn steer_queued_prompt(&self, session_id: &str, id: &str) -> bool {
        let existed = self
            .pending_inbox
            .get()
            .get(session_id)
            .is_some_and(|items| items.iter().any(|item| item.id == id));
        if existed {
            self.inbox_sink.borrow().update(session_id, id, "steer");
            self.update_inbox_delivery(session_id, id, "steer");
        }
        existed
    }

    /// TUI-TD07 — the undo's refill payload: the queued item's
    /// text + files (`projectedPromptInput(queued.payload)`,
    /// index.tsx:662-668) for `appendPrompt`.
    #[must_use]
    pub fn queued_prompt_payload(&self, session_id: &str, id: &str) -> Option<InboxItem> {
        self.pending_inbox
            .get()
            .get(session_id)
            .and_then(|items| items.iter().find(|item| item.id == id))
            .cloned()
    }

    /// `updatePending` (client/solid/data.ts:311-317): flip one item's
    /// delivery (`session.inbox.delivery.changed` + the optimistic
    /// local mirror).
    fn update_inbox_delivery(&self, session_id: &str, id: &str, delivery: &str) {
        self.pending_inbox.update(|state| {
            if let Some(items) = state.get_mut(session_id) {
                if let Some(item) = items.iter_mut().find(|item| item.id == id) {
                    item.delivery.clone_from(&delivery.to_owned());
                }
            }
        });
    }

    /// `removePending` + `retractLocal` (data.ts:262-270/403-415):
    /// drop the pending item AND its admitted transcript row.
    fn retract_inbox_item(&self, session_id: &str, id: &str) {
        self.pending_inbox.update(|state| {
            if let Some(items) = state.get_mut(session_id) {
                items.retain(|item| item.id != id);
            }
        });
        self.update_messages(session_id, |messages| {
            messages.retain(|message| message.id() != id);
        });
    }

    /// The drain step — RETIRED by TUI-TD07: the server owns the
    /// queue's delivery (`session.prompt {delivery: "queue"}` waits
    /// server-side; `session.inbox.delivered` clears the pending row
    /// when the turn admits it). Kept as the no-op seam the run loop
    /// calls between ingests (the old local serial drain double-sent).
    pub fn drain_queued_prompts(&self) {}

    // -- model ring (Wave A: the f2/favorite cycle source) --------------------

    /// The model-preference ring (recent/favorite/variant —
    /// `context/local.tsx`'s preferences document).
    #[must_use]
    pub fn model_ring(&self) -> ModelRing {
        self.model_ring.get()
    }

    /// Seed the ring (boot: the persisted `<state>/model.json`, when
    /// available).
    pub fn set_model_ring(&self, ring: ModelRing) {
        self.model_ring.set(ring);
    }

    /// `local.model.set(model, { recent: true })` — `addRecent`
    /// (model-preference.ts:150-152): dedup, front-insert, cap 10.
    pub fn model_add_recent(&self, provider_id: &str, model_id: &str) {
        let model = model_pref::ModelPreferenceModel {
            provider_id: provider_id.to_owned(),
            model_id: model_id.to_owned(),
        };
        self.model_ring.update(|ring| {
            ring.document.recent = model_pref::recent_models(&model, &ring.document.recent);
        });
        self.model_ring_dirty.set(true);
    }

    /// `local.model.cycle(direction)` (local.tsx:412-423): step through
    /// the VALID recent ring (validity against the loaded model list),
    /// wrapping; returns the next selection.
    #[must_use]
    pub fn model_cycle_recent(&self, direction: i8) -> Option<ModelSelection> {
        let ring = self.model_ring.get();
        let valid = |model: &model_pref::ModelPreferenceModel| -> bool {
            self.server_lists().models.rows().is_none_or(|rows| {
                rows.iter()
                    .any(|row| row.provider_id == model.provider_id && row.id == model.model_id)
            })
        };
        let recent: Vec<&model_pref::ModelPreferenceModel> =
            ring.document.recent.iter().filter(|model| valid(model)).collect();
        if recent.is_empty() {
            return None;
        }
        let route = self.route();
        let current = match &route {
            Route::Session { session_id } => self.selection_for(session_id).model,
            Route::Home => self.selection_for(crate::app::HOME_PROMPT_KEY).model,
        };
        let index = current.as_ref().and_then(|selection| {
            recent.iter().position(|model| {
                model.provider_id == selection.provider_id && model.model_id == selection.model_id
            })
        });
        let next = match index {
            None if direction > 0 => 0,
            None => recent.len() - 1,
            Some(index) => wrap_step(index, direction, recent.len()),
        };
        let model = recent.get(next)?;
        Some(ModelSelection {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
            variant: None,
        })
    }

    /// `local.model.cycleFavorite(direction)` (local.tsx:424-451):
    /// step the favorites (info toast when none — the caller shows it),
    /// and push the pick onto recent.
    #[must_use]
    pub fn model_cycle_favorite(&self, direction: i8) -> Option<ModelSelection> {
        let ring = self.model_ring.get();
        let valid = |model: &model_pref::ModelPreferenceModel| -> bool {
            self.server_lists().models.rows().is_none_or(|rows| {
                rows.iter()
                    .any(|row| row.provider_id == model.provider_id && row.id == model.model_id)
            })
        };
        let favorites: Vec<&model_pref::ModelPreferenceModel> =
            ring.document.favorite.iter().filter(|model| valid(model)).collect();
        if favorites.is_empty() {
            return None;
        }
        let route = self.route();
        let current = match &route {
            Route::Session { session_id } => self.selection_for(session_id).model,
            Route::Home => self.selection_for(crate::app::HOME_PROMPT_KEY).model,
        };
        let index = current.as_ref().and_then(|selection| {
            favorites.iter().position(|model| {
                model.provider_id == selection.provider_id && model.model_id == selection.model_id
            })
        });
        let next = match index {
            None if direction > 0 => 0,
            None => favorites.len() - 1,
            Some(index) => wrap_step(index, direction, favorites.len()),
        };
        let model = favorites.get(next)?;
        self.model_add_recent(&model.provider_id, &model.model_id);
        Some(ModelSelection {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
            variant: None,
        })
    }

    /// The model-ring persistence flag (the run loop drains it: write
    /// `<state>/model.json` when set).
    pub fn take_model_ring_dirty(&self) -> Option<ModelRing> {
        if self.model_ring_dirty.replace(false) {
            Some(self.model_ring.get())
        } else {
            None
        }
    }

    /// The current selection's variant cycle
    /// (`local.model.variant.cycle`, local.tsx:494-498 over
    /// `cycleModelVariant`): `None` = the model has no variants.
    #[must_use]
    pub fn variant_cycle(&self) -> Option<Option<String>> {
        let route = self.route();
        let selection = match &route {
            Route::Session { session_id } => self.selection_for(session_id).model?,
            Route::Home => self.selection_for(crate::app::HOME_PROMPT_KEY).model?,
        };
        let variants = self
            .server_lists()
            .models
            .rows()?
            .iter()
            .find(|row| row.provider_id == selection.provider_id && row.id == selection.model_id)
            .map(|row| row.variants.clone())
            .unwrap_or_default();
        if variants.is_empty() {
            return None;
        }
        Some(model_pref::cycle_model_variant(selection.variant.as_deref(), &variants))
    }

    /// The visible agents for the agent cycle (`local.tsx:57-59`:
    /// `mode !== "subagent" && !hidden`).
    #[must_use]
    pub fn cycleable_agents(&self) -> Vec<ServerAgent> {
        match self.server_lists().agents.rows() {
            Some(rows) => rows
                .iter()
                .filter(|agent| agent.mode != "subagent" && !agent.hidden)
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    /// `local.agent.move(direction)` (local.tsx:114-124): step the
    /// visible agents, wrapping; `None` when the list is unavailable.
    #[must_use]
    pub fn agent_cycle(&self, direction: i8) -> Option<String> {
        let agents = self.cycleable_agents();
        if agents.is_empty() {
            return None;
        }
        let route = self.route();
        let current = match &route {
            Route::Session { session_id } => self.selection_for(session_id).agent,
            Route::Home => self.selection_for(crate::app::HOME_PROMPT_KEY).agent,
        }
        .or_else(|| agents.first().map(|agent| agent.id.clone()));
        let index = current.as_ref().and_then(|id| agents.iter().position(|agent| agent.id == *id));
        let next = match index {
            None if direction > 0 => 0,
            None => agents.len() - 1,
            Some(index) => wrap_step(index, direction, agents.len()),
        };
        Some(agents.get(next)?.id.clone())
    }

    // -- route / dialogs / theme / connection --------------------------------

    pub fn navigate(&self, route: Route) {
        // A session/home navigation closes the open plugin route page
        // (PLUG-TUI-SLOT: the plugin route is replaced exactly like
        // the ref's router replaces the visible route).
        self.close_plugin_route();
        // Pointer-registry lifetime (auditA §4.1, finding #2): v2's
        // hit targets are per-render-frame renderables — leaving a
        // route must disarm its recorded rects. `render_session`
        // re-records the transcript rows, the footer-location rect
        // and the sidebar onboarding card on its next pass; without
        // this clear, a release on the HOME screen could fire a
        // stale transcript toggle / open the WorkingDirectory dialog
        // with the previous session's directory (the phantom-click
        // hazard — the inverse of dogfood2's home-strip bug).
        self.clear_route_pointer_registries();
        if let Route::Session { session_id } = &route {
            // v2's session-tabs context roots every routed session
            // (context/session-tabs.tsx:138, 188): navigating into a
            // CHILD session routes the family ROOT for tab purposes —
            // no new tab opens, the root's pill stays active (#45).
            let root = self.session_root(session_id);
            self.tabs.update(|state| {
                state.history = model::record_tab_history(&state.history, &root);
                // Viewing clears unread.
                state.unread.remove(&root);
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
                let title = self.session_title(&root).or_else(|| {
                    self.session_history
                        .get()
                        .iter()
                        .find(|session| session.id == root)
                        .and_then(|session| session.title.clone())
                });
                self.tabs.update(|state| {
                    if !state.tabs.iter().any(|tab| tab.session_id == root) {
                        state.tabs = model::open_session_tab(
                            std::mem::take(&mut state.tabs),
                            SessionTab { session_id: root, title },
                        );
                    }
                });
            }
        }
        // CONFIG-C8 — the retention effect (context/session-retention.
        // ts): navigation advances the recency window (`viewed !==
        // previous`) and the pass evicts every listed root outside
        // tabs ∪ window ∪ current.
        let previous = match &self.route.get() {
            Route::Session { session_id } => Some(session_id.clone()),
            Route::Home => None,
        };
        self.route.set(route);
        let viewed = match &self.route.get() {
            Route::Session { session_id } => Some(session_id.clone()),
            Route::Home => None,
        };
        let navigated = viewed != previous;
        self.enforce_session_retention(viewed.as_deref(), navigated);
    }

    pub fn push_dialog(&self, kind: DialogKind) {
        let theme_initial = self.theme_dialog_initial(&kind);
        let entry = DialogEntry {
            kind,
            size: DialogSize::default(),
            select: SelectState::default(),
            value: String::new(),
            theme_initial,
        };
        self.dialogs.update(|stack| stack.push(entry));
    }

    pub fn pop_dialog(&self) {
        let removed = self.dialogs.update(std::vec::Vec::pop);
        if let Some(entry) = removed {
            self.revert_unconfirmed_theme_dialog(&entry);
        }
    }

    // -- dialog framework (`ui/dialog.tsx` stack semantics) --------------------

    /// Push a dialog entry (kind + size + fresh content state).
    pub fn open_dialog(&self, kind: DialogKind, size: DialogSize) {
        let theme_initial = self.theme_dialog_initial(&kind);
        let entry = DialogEntry {
            kind,
            size,
            select: SelectState::default(),
            value: String::new(),
            theme_initial,
        };
        self.dialogs.update(|stack| stack.push(entry));
        self.dialog_opened_strip_hover_reset();
        self.focus_dialog_current();
    }

    /// Replace the whole stack with one dialog (`dialog.replace`):
    /// every open dialog's close semantics run, then the new entry
    /// becomes the only one.
    pub fn replace_dialog(&self, kind: DialogKind, size: DialogSize) {
        let theme_initial = self.theme_dialog_initial(&kind);
        let entry = DialogEntry {
            kind,
            size,
            select: SelectState::default(),
            value: String::new(),
            theme_initial,
        };
        let removed = self.dialogs.update(|stack| std::mem::replace(stack, vec![entry]));
        for entry in &removed {
            self.revert_unconfirmed_theme_dialog(entry);
            // TUI-25: a replaced-away ctx.ui alert/confirm settles
            // unresolved (`settle` fires on close too).
            self.resolve_closing_ui_dialog(&entry.kind, serde_json::Value::Null);
        }
        self.dialog_opened_strip_hover_reset();
        self.focus_dialog_current();
    }

    /// The captured initial theme for a theme dialog (`const initial =
    /// themes.selected` at `DialogThemeList` mount).
    fn theme_dialog_initial(&self, kind: &DialogKind) -> Option<String> {
        match kind {
            DialogKind::ThemeList => Some(self.theme.get().name.clone()),
            _ => None,
        }
    }

    /// A PROMPT-input dialog (rename / worktree name) drops the
    /// strip's hover — v2's `DialogSessionRename` focuses its `<input>`
    /// and the marquee hover resets (the census: the `✕` vanishes
    /// while the rename dialog is open, returns after it closes).
    /// Select-family dialogs keep the hover (the session-list steps
    /// hold their `✕`).
    fn dialog_opened_strip_hover_reset(&self) {
        let prompt_dialog = self.top_dialog().is_some_and(|entry| {
            matches!(entry.kind, DialogKind::SessionRename { .. } | DialogKind::WorktreeName)
        });
        if prompt_dialog {
            self.tabs.update(|state| {
                state.hover = None;
                state.hover_close = false;
            });
        }
    }

    /// An unconfirmed theme dialog closing reverts the live preview to
    /// its captured initial (`onCleanup: if (!confirmed) themes.set(
    /// initial)` — dialog-theme-list.tsx:19-21).
    fn revert_unconfirmed_theme_dialog(&self, entry: &DialogEntry) {
        if let (DialogKind::ThemeList, Some(initial)) = (&entry.kind, entry.theme_initial.as_ref())
        {
            if self.theme.get().name != *initial {
                self.set_theme_by_name(initial);
            }
        }
    }

    /// Confirm the top theme dialog (a submit landed: `confirmed = true`
    /// before `dialog.clear()` — the close must NOT revert).
    pub fn confirm_theme_dialog(&self) {
        self.dialogs.update(|stack| {
            if let Some(top) = stack.last_mut() {
                top.theme_initial = None;
            }
        });
    }

    /// A freshly-opened select dialog focuses its `current` option
    /// (`DialogSelect` mount: `focus(current)` + `scrollAfterLayout(
    /// true)` — the CENTER reveal in the real viewport, not the
    /// edge-follow) — the theme list opens centered on the active theme.
    /// EXCEPTION: the models dialog mounts `focusCurrent={false}`
    /// (`dialog-model.tsx:171`) — the cursor rests on row 0 and the
    /// current model only carries its `●` marker.
    fn focus_dialog_current(&self) {
        let Some(entry) = self.top_dialog() else { return };
        if matches!(entry.kind, DialogKind::SessionRename { .. }) {
            return;
        }
        if matches!(entry.kind, DialogKind::ServerList { command: "model.list", .. }) {
            return;
        }
        let props = crate::components::command_palette::select_props_for_entry(&entry, self);
        let height = usize::from(self.terminal_size.get().1);
        self.update_dialog(|top| {
            top.select.focus_current(&props);
            top.select.center_on_selection(&props, height);
        });
    }

    /// Close the top dialog (`escape` in the modal layer — the entry's
    /// close semantics, then the pop).
    pub fn close_dialog(&self) {
        let removed = self.dialogs.update(std::vec::Vec::pop);
        if let Some(entry) = removed {
            self.revert_unconfirmed_theme_dialog(&entry);
            // TUI-25: an escaped ctx.ui alert/confirm settles
            // unresolved (`onClose: () => done(undefined)`).
            self.resolve_closing_ui_dialog(&entry.kind, serde_json::Value::Null);
        }
        self.set_dialog_frame_rect(None);
    }

    /// Close every dialog (`dialog.clear`).
    pub fn clear_dialogs(&self) {
        let removed = self.dialogs.update(std::mem::take);
        for entry in &removed {
            self.revert_unconfirmed_theme_dialog(entry);
            self.resolve_closing_ui_dialog(&entry.kind, serde_json::Value::Null);
        }
        self.set_dialog_frame_rect(None);
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

    // -- TUI-8/9/10/23/24/25/TD22: the lane-dialogs surfaces ---------

    /// TUI-8: replace the skill inventory (`skill.list`'s fetch
    /// result; the boot worker + the dialog's sync).
    pub fn set_skills(&self, skills: ListState<ServerSkill>) {
        self.server_lists.update(|lists| lists.skills = skills);
        self.request_redraw();
    }

    /// The skill inventory (`DialogSkill`'s rows).
    #[must_use]
    pub fn skills(&self) -> ListState<ServerSkill> {
        self.server_lists.get().skills
    }

    /// TUI-8: open `DialogSkill` at v2's LARGE size
    /// (`dialog.setSize("large")`, dialog-skill.tsx:19) — the rows
    /// read the `skill.list` state (the boot fetch's cache; v2's
    /// `sync` refetch only fires when the cache is absent, which the
    /// boot chain already satisfied).
    pub fn open_skills_dialog(&self) {
        self.replace_dialog(DialogKind::Skill, DialogSize::Large);
    }

    /// TUI-9: load the stash from `<state>/prompt-stash.jsonl`
    /// (`PromptStashProvider`'s onMount read — `parsePromptStash`'s
    /// per-line `{prompt, timestamp}` decode, corrupt lines skipped,
    /// the last MAX entries kept). Called at boot with the resolved
    /// state root; a missing file leaves the stash empty.
    pub fn stash_init(&self, path: Option<std::path::PathBuf>) {
        let Some(path) = path else { return };
        let mut entries = Vec::new();
        if let Ok(body) = std::fs::read_to_string(&path) {
            for line in body.lines().filter(|line| !line.is_empty()) {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                let Some(text) = value.pointer("/prompt/text").and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                let Some(timestamp) = value.get("timestamp").and_then(serde_json::Value::as_u64)
                else {
                    continue;
                };
                let mut draft = crate::components::composer::PromptDraft {
                    text: text.to_owned(),
                    ..crate::components::composer::PromptDraft::default()
                };
                if let Some(files) = value.pointer("/prompt/files").and_then(|v| v.as_array()) {
                    for file in files {
                        let Some(name) = file.get("name").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        let start = usize::try_from(
                            file.get("start").and_then(serde_json::Value::as_u64).unwrap_or(0),
                        )
                        .unwrap_or(0);
                        let end = usize::try_from(
                            file.get("end").and_then(serde_json::Value::as_u64).unwrap_or(0),
                        )
                        .unwrap_or(0);
                        let mention =
                            file.get("mention").and_then(|v| v.as_str()).map(str::to_owned);
                        let payload =
                            file.get("payload").and_then(|p| p.as_object()).and_then(|payload| {
                                let mime = payload.get("mime")?.as_str()?.to_owned();
                                let data = payload.get("data")?.as_str()?.to_owned();
                                Some(crate::components::composer::AttachmentPayload { mime, data })
                            });
                        draft.files.push(crate::components::composer::FileMention {
                            name: name.to_owned(),
                            start,
                            end,
                            payload,
                            mention,
                        });
                    }
                }
                if let Some(agents) = value.pointer("/prompt/agents").and_then(|v| v.as_array()) {
                    for agent in agents {
                        if let Some(name) = agent.get("name").and_then(|v| v.as_str()) {
                            draft.agents.push(crate::components::composer::AgentMention {
                                name: name.to_owned(),
                            });
                        }
                    }
                }
                entries.push(StashEntry { prompt: draft, timestamp });
            }
            entries.truncate(MAX_STASH_ENTRIES);
            // The onMount rewrite (v2 rewrites the parsed file so a
            // foreign tail normalizes) — only when it parsed entries.
            if !entries.is_empty() {
                let _ = Self::stash_persist(&entries, &path);
            }
        }
        let mut state = self.lane_dialogs.borrow_mut();
        state.stash = entries;
        state.stash_path = Some(path);
    }

    /// Serialize the stash (one `{prompt, timestamp}` per line).
    fn stash_persist(entries: &[StashEntry], path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = entries
            .iter()
            .map(|entry| {
                let files: Vec<serde_json::Value> = entry
                    .prompt
                    .files
                    .iter()
                    .map(|file| {
                        let mut value = serde_json::json!({
                            "name": file.name,
                            "start": file.start,
                            "end": file.end,
                        });
                        if let Some(mention) = &file.mention {
                            value["mention"] = serde_json::json!(mention);
                        }
                        if let Some(payload) = &file.payload {
                            value["payload"] = serde_json::json!({
                                "mime": payload.mime, "data": payload.data,
                            });
                        }
                        value
                    })
                    .collect();
                let agents: Vec<serde_json::Value> = entry
                    .prompt
                    .agents
                    .iter()
                    .map(|agent| serde_json::json!({ "name": agent.name }))
                    .collect();
                serde_json::json!({
                    "prompt": {
                        "text": entry.prompt.text,
                        "files": files,
                        "agents": agents,
                    },
                    "timestamp": entry.timestamp,
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut body = body;
        body.push('\n');
        // Atomic write like every other state file (temp + rename).
        let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
        let mut handle = std::fs::File::create(&temporary)?;
        handle.write_all(body.as_bytes())?;
        drop(handle);
        std::fs::rename(&temporary, path)
    }

    /// The stash (oldest first, like the file).
    #[must_use]
    pub fn stash_entries(&self) -> Vec<StashEntry> {
        self.lane_dialogs.borrow().stash.clone()
    }

    /// The route's prompt key (the focused composer — session or the
    /// home draft).
    fn focused_prompt_key(&self) -> String {
        match self.route() {
            Route::Session { session_id } => session_id,
            Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
        }
    }

    /// TUI-9: `prompt.stash` — stash the focused draft and reset the
    /// composer (`stash.push({prompt: store.prompt}); resetComposer()`,
    /// prompt/index.tsx:885-890). Returns false on the `enabled: !!
    /// text` gate.
    pub fn stash_push(&self) -> bool {
        let key = self.focused_prompt_key();
        let composer = self.composer(&key);
        if composer.draft.text.is_empty() {
            return false;
        }
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX));
        let mut state = self.lane_dialogs.borrow_mut();
        state.stash.push(StashEntry { prompt: composer.draft.clone(), timestamp });
        if state.stash.len() > MAX_STASH_ENTRIES {
            let overflow = state.stash.len() - MAX_STASH_ENTRIES;
            state.stash.drain(..overflow);
        }
        let entries = state.stash.clone();
        let path = state.stash_path.clone();
        drop(state);
        if let Some(path) = path {
            let _ = Self::stash_persist(&entries, &path);
        }
        let mut composer = self.composer(&key);
        composer.reset();
        self.set_composer(&key, &composer);
        self.clear_dialogs();
        self.request_redraw();
        true
    }

    /// TUI-9: `prompt.stash.pop` — remove the newest entry and
    /// restore it into the focused composer (`input.setText`,
    /// `setStore("prompt", entry.prompt)`, index.tsx:895-907).
    pub fn stash_pop(&self) -> Option<StashEntry> {
        let entry = {
            let mut state = self.lane_dialogs.borrow_mut();
            let entry = state.stash.pop()?;
            let entries = state.stash.clone();
            let path = state.stash_path.clone();
            drop(state);
            if let Some(path) = path {
                let _ = Self::stash_persist(&entries, &path);
            }
            entry
        };
        let key = self.focused_prompt_key();
        let mut composer = self.composer(&key);
        composer.set(entry.prompt.clone());
        self.set_composer(&key, &composer);
        self.clear_dialogs();
        self.request_redraw();
        Some(entry)
    }

    /// Remove stash slot `index` (the dialog's two-press delete + the
    /// select's pop-and-restore reads it first). Persists the rest.
    pub fn stash_remove(&self, index: usize) -> Option<StashEntry> {
        let (entry, path) = {
            let mut state = self.lane_dialogs.borrow_mut();
            if index >= state.stash.len() {
                return None;
            }
            let entry = state.stash.remove(index);
            state.stash_delete_armed = None;
            let remaining = state.stash.clone();
            let path = state.stash_path.clone();
            (Some((entry, remaining)), path)
        };
        let (entry, remaining) = entry?;
        if let Some(path) = path {
            let _ = Self::stash_persist(&remaining, &path);
        }
        self.request_redraw();
        Some(entry)
    }

    /// The armed two-press delete (stash dialog's `toDelete`).
    pub fn stash_delete_pressed(&self, index: usize) -> DeleteStep {
        let mut state = self.lane_dialogs.borrow_mut();
        if index >= state.stash.len() {
            return DeleteStep::Invalid;
        }
        if state.stash_delete_armed == Some(index) {
            state.stash_delete_armed = None;
            return DeleteStep::Confirmed;
        }
        state.stash_delete_armed = Some(index);
        DeleteStep::Armed
    }

    /// `onMove` clears the armed row (`setToDelete(undefined)`).
    pub fn stash_delete_disarm(&self) {
        self.lane_dialogs.borrow_mut().stash_delete_armed = None;
    }

    /// The armed row (the row's destructive title flip).
    #[must_use]
    pub fn stash_delete_armed(&self) -> Option<usize> {
        self.lane_dialogs.borrow().stash_delete_armed
    }

    /// TUI-10: open the image preview over `images` at `initial`
    /// (`dialog.replace(<DialogImagePreview …>)` — xlarge + centered).
    pub fn open_image_preview(&self, images: Vec<ImagePreviewItem>, initial: usize) {
        if images.is_empty() {
            return;
        }
        let index = initial.min(images.len() - 1);
        self.lane_dialogs.borrow_mut().image_preview = ImagePreviewState { images, index };
        self.replace_dialog(DialogKind::ImagePreview, DialogSize::Xlarge);
        self.request_redraw();
    }

    /// The preview dialog's state (the renderer's images + index).
    #[must_use]
    pub fn image_preview_dialog(&self) -> ImagePreviewState {
        self.lane_dialogs.borrow().image_preview.clone()
    }

    /// `move(direction)` (dialog-image-preview.tsx:31-35): the modulo
    /// cycle; a single image stays.
    pub fn image_preview_move(&self, direction: i64) {
        let mut state = self.lane_dialogs.borrow_mut();
        let count = state.image_preview.images.len();
        if count < 2 {
            return;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let next = (i64::try_from(state.image_preview.index).unwrap_or(0) + direction)
            .rem_euclid(i64::try_from(count).unwrap_or(1))
            .max(0) as usize;
        state.image_preview.index = next;
        drop(state);
        self.request_redraw();
    }

    /// TUI-24: open the generic error details (`dialog-error-details.
    /// tsx`'s props; `back_to_plugins` re-seats the plugins list on
    /// escape like v2's `onBack`).
    pub fn open_error_details(
        &self,
        title: &str,
        source: &str,
        error: &str,
        context: &str,
        diagnostic_ref: &str,
        back_to_plugins: bool,
    ) {
        self.lane_dialogs.borrow_mut().error_details_scroll = 0;
        self.replace_dialog(
            DialogKind::ErrorDetails {
                title: title.to_owned(),
                source: source.to_owned(),
                error: error.to_owned(),
                context: context.to_owned(),
                diagnostic_ref: diagnostic_ref.to_owned(),
                back_to_plugins,
            },
            DialogSize::Medium,
        );
        self.request_redraw();
    }

    /// The error-details body's scroll offset (`↑/↓ scrollBy`).
    #[must_use]
    pub fn error_details_scroll(&self) -> usize {
        self.lane_dialogs.borrow().error_details_scroll
    }

    /// Scroll the error body (`scrollBy(±1)` / `±20` the page keys;
    /// clamped by the caller's viewport).
    pub fn error_details_scroll_by(&self, delta: i64, max: usize) {
        let mut state = self.lane_dialogs.borrow_mut();
        let max = i64::try_from(max).unwrap_or(i64::MAX);
        let next = (i64::try_from(state.error_details_scroll).unwrap_or(0) + delta).clamp(0, max);
        state.error_details_scroll = usize::try_from(next).unwrap_or(0);
        drop(state);
        self.request_redraw();
    }

    /// TUI-25: open the generic alert (`ui/dialog-alert.tsx`).
    /// `request_id` rides the plugin `ctx.ui.alert` promise;
    /// `confirm_command` runs through the palette dispatcher on ok.
    pub fn open_alert(
        &self,
        title: &str,
        message: &str,
        request_id: Option<u64>,
        confirm_command: Option<String>,
    ) {
        self.replace_dialog(
            DialogKind::Alert {
                title: title.to_owned(),
                message: message.to_owned(),
                request_id,
                confirm_command,
            },
            DialogSize::Medium,
        );
        self.request_redraw();
    }

    /// TUI-25: open the generic confirm (`ui/dialog-confirm.tsx`).
    #[allow(clippy::too_many_arguments)] // the ref's props tuple
    pub fn open_confirm(
        &self,
        title: &str,
        message: &str,
        confirm_label: &str,
        cancel_label: &str,
        request_id: Option<u64>,
        confirm_command: Option<String>,
        cancel_command: Option<String>,
    ) {
        self.replace_dialog(
            DialogKind::Confirm {
                title: title.to_owned(),
                message: message.to_owned(),
                confirm_label: confirm_label.to_owned(),
                cancel_label: cancel_label.to_owned(),
                active_confirm: true,
                request_id,
                confirm_command,
                cancel_command,
            },
            DialogSize::Medium,
        );
        self.request_redraw();
    }

    /// Toggle the confirm dialog's focused pill (←/→).
    pub fn confirm_toggle_active(&self) {
        self.update_dialog(|top| {
            if let DialogKind::Confirm { active_confirm, .. } = &mut top.kind {
                *active_confirm = !*active_confirm;
            }
        });
        self.request_redraw();
    }

    /// The confirm dialog's focused pill (`active`).
    #[must_use]
    pub fn confirm_active_is_confirm(&self) -> bool {
        matches!(
            self.top_dialog().map(|entry| entry.kind),
            Some(DialogKind::Confirm { active_confirm: true, .. })
        )
    }

    /// Enqueue a ctx.ui dialog resolution (the plugin pump drains it
    /// into `PluginHost::resolve_ui_request`).
    pub fn enqueue_ui_dialog_resolution(&self, request_id: u64, value: serde_json::Value) {
        self.lane_dialogs.borrow_mut().ui_dialog_resolutions.push((request_id, value));
    }

    /// Drain the resolution queue (the plugin pump's per-frame step).
    pub fn take_ui_dialog_resolutions(&self) -> Vec<(u64, serde_json::Value)> {
        std::mem::take(&mut self.lane_dialogs.borrow_mut().ui_dialog_resolutions)
    }

    /// The settle-once semantics of a CLOSING alert/confirm (`settle`
    /// in api.tsx:318-325 — close and confirm both settle, the first
    /// wins; an unresolved close is `null` for confirm, `null` for
    /// alert too since `onConfirm`/`onClose` are the same `done`).
    fn resolve_closing_ui_dialog(&self, kind: &DialogKind, result: serde_json::Value) {
        let request_id = match kind {
            DialogKind::Alert { request_id, .. } | DialogKind::Confirm { request_id, .. } => {
                *request_id
            }
            _ => None,
        };
        if let Some(id) = request_id {
            self.enqueue_ui_dialog_resolution(id, result);
        }
    }

    /// TUI-24: the plugin fault table (the plugins dialog's error
    /// rows; fed from the host's `plugin_faults`).
    pub fn set_plugin_faults(&self, faults: std::collections::HashMap<String, String>) {
        self.lane_dialogs.borrow_mut().plugin_faults = faults;
        self.request_redraw();
    }

    /// A plugin's fault string (the error-details' error body).
    #[must_use]
    pub fn plugin_fault(&self, plugin_id: &str) -> Option<String> {
        self.lane_dialogs.borrow().plugin_faults.get(plugin_id).cloned()
    }

    /// TUI-23: seed the settings snapshot (the boot config's display
    /// values for the rows the store does not model live).
    pub fn set_settings_snapshot(&self, snapshot: SettingsSnapshot) {
        self.lane_dialogs.borrow_mut().settings = snapshot;
    }

    /// One settings snapshot value (`""` when unknown).
    #[must_use]
    pub fn settings_snapshot_value(&self, path: &str) -> String {
        self.lane_dialogs.borrow().settings.get(path).cloned().unwrap_or_default()
    }

    /// Record a cycled settings value into the snapshot.
    pub fn settings_snapshot_set(&self, path: &str, value: &str) {
        self.lane_dialogs.borrow_mut().settings.insert(path.to_owned(), value.to_owned());
    }

    /// TUI-TD22: the experiments table (EMPTY in v2.0.24 —
    /// `experiments: Experiment[] = []`, dialog-experiments.tsx:16).
    pub fn set_experiments(&self, experiments: Vec<Experiment>) {
        self.lane_dialogs.borrow_mut().experiments = experiments;
    }

    /// The experiments table.
    #[must_use]
    pub fn experiments(&self) -> Vec<Experiment> {
        self.lane_dialogs.borrow().experiments.clone()
    }

    /// Seed the `config.experimental` map's local view (read-only
    /// consumption of oc-core's parsed cli.json).
    pub fn set_experimental_flags(&self, flags: std::collections::BTreeMap<String, bool>) {
        self.lane_dialogs.borrow_mut().experimental_flags = flags;
    }

    /// `enabled(experiment)` — `config.data.experimental?.[id] ===
    /// true` (dialog-experiments.tsx:25).
    #[must_use]
    pub fn experiment_enabled(&self, id: &str) -> bool {
        self.lane_dialogs.borrow().experimental_flags.get(id).copied() == Some(true)
    }

    /// The toggle (`change`: `experimental.<id> = !enabled`) — flips
    /// the local view + writes through the config sink.
    pub fn experiment_toggle(&self, id: &str) {
        let next = !self.experiment_enabled(id);
        self.lane_dialogs.borrow_mut().experimental_flags.insert(id.to_owned(), next);
        self.config_sink
            .borrow()
            .update_setting(&format!("experimental.{id}"), if next { "true" } else { "false" });
        self.request_redraw();
    }

    /// TUI-10: the diff viewer's image hit rects (the inline
    /// previews' absolute cells + data URIs — a release opens the
    /// preview).
    pub fn set_diff_image_hits(&self, hits: Vec<(usize, usize, usize, usize, String)>) {
        self.lane_dialogs.borrow_mut().diff_image_hits = hits;
    }

    /// The diff viewer's image hit rects from the last render.
    #[must_use]
    pub fn diff_image_hits(&self) -> Vec<(usize, usize, usize, usize, String)> {
        self.lane_dialogs.borrow().diff_image_hits.clone()
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
                // v2's `themes.set` persists the name through
                // `configState.update` (context/theme.tsx:339-346) —
                // the SURF-2 config sink write-through.
                self.config_sink.borrow().update_theme_name(name);
                true
            }
            None => false,
        }
    }

    /// The theme dialog's LIVE PREVIEW after a selection move
    /// (`onMove: (opt) => themes.set(opt.value)` — dialog-theme-list.
    /// tsx:28-30). No-op unless the top dialog is the theme list.
    pub fn preview_theme_move(&self) {
        let Some(entry) = self.top_dialog() else { return };
        if !matches!(entry.kind, DialogKind::ThemeList) {
            return;
        }
        let props = crate::components::command_palette::select_props_for_entry(&entry, self);
        if let Some(option) = entry.select.selected_option(&props) {
            self.set_theme_by_name(&option.value.clone());
        }
    }

    /// The theme dialog's LIVE PREVIEW after a filter change
    /// (`onFilter`: empty query resets to the captured `initial`, else
    /// the FIRST filtered entry — dialog-theme-list.tsx:39-47).
    pub fn preview_theme_filter(&self) {
        let Some(entry) = self.top_dialog() else { return };
        if !matches!(entry.kind, DialogKind::ThemeList) {
            return;
        }
        let props = crate::components::command_palette::select_props_for_entry(&entry, self);
        if entry.select.filter.is_empty() {
            if let Some(initial) = entry.theme_initial.clone() {
                self.set_theme_by_name(&initial);
            }
            return;
        }
        let groups = crate::components::dialog_select::grouped(&props, &entry.select.filter);
        let first = crate::components::dialog_select::flat_indices(&groups)
            .first()
            .map(|index| props.options[*index].value.clone());
        if let Some(value) = first {
            self.set_theme_by_name(&value);
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

    /// Open (or replace) the plugin route page (PLUG-TUI-SLOT's
    /// router surface — `ctx.ui.router.navigate({type:"plugin"})`):
    /// the content materializes next pump through the host's bounded
    /// `route.render` (the title shows the route name until then).
    pub fn open_plugin_route(&self, plugin_id: &str, name: &str, data: Option<serde_json::Value>) {
        self.plugin_route.set(Some(PluginRoutePage {
            plugin_id: plugin_id.to_owned(),
            name: name.to_owned(),
            data,
            title: name.to_owned(),
            rows: Vec::new(),
            missing: false,
        }));
    }

    /// TUI-TD28 — latch the fallback state: the host's bounded
    /// `route.render` replied miss (no registered route
    /// `{plugin_id}/{name}`), so the page renders
    /// `PluginRouteMissing` (`Unknown plugin route: …` + go home,
    /// component/plugin-route-missing.tsx:3-21). No-op when no page
    /// is open.
    pub fn mark_plugin_route_missing(&self) {
        self.plugin_route.update(|page| {
            if let Some(page) = page.as_mut() {
                page.missing = true;
            }
        });
    }

    /// Fill the open page's materialized content (`route.render`'s
    /// bounded reply). A no-op when no page is open or the route
    /// changed mid-flight.
    pub fn set_plugin_route_content(&self, title: &str, rows: Vec<String>) {
        self.plugin_route.update(|page| {
            if let Some(page) = page.as_mut() {
                title.clone_into(&mut page.title);
                page.rows = rows;
            }
        });
    }

    /// Close the plugin route page (a session/home navigation or the
    /// page's own back bind).
    pub fn close_plugin_route(&self) {
        self.plugin_route.set(None);
    }

    /// The open plugin route page, when a plugin route is visible.
    #[must_use]
    pub fn plugin_route_page(&self) -> Option<PluginRoutePage> {
        self.plugin_route.get()
    }

    // ---------------------------------------------------------------------
    // The session sidebar's visibility state (`session-frame.tsx`)
    // ---------------------------------------------------------------------

    /// The terminal size (the toggle's visibility input; the render
    /// path passes its own `size`).
    pub fn set_terminal_size(&self, size: (u16, u16)) {
        self.terminal_size.set(size);
    }

    /// The last size the app reported (0×0 before the first frame).
    #[must_use]
    pub fn terminal_size(&self) -> (u16, u16) {
        self.terminal_size.get()
    }

    /// Apply the resolved `session.sidebar` config (boot / reload;
    /// cli.json first, then the persisted last toggle overrides).
    pub fn set_sidebar_mode(&self, mode: crate::components::sidebar::SidebarMode) {
        self.sidebar_mode.set(mode);
    }

    /// The current `session.sidebar` mode.
    #[must_use]
    pub fn sidebar_mode(&self) -> crate::components::sidebar::SidebarMode {
        self.sidebar_mode.get()
    }

    /// v2's `sidebarVisible()` (session-frame.tsx:107-111): a
    /// subagent session (`parentID`) never shows it; the manual open
    /// flag forces it on; otherwise `auto` shows above the wide
    /// breakpoint (width > 120).
    #[must_use]
    pub fn sidebar_shown(&self, session_id: &str, width: usize) -> bool {
        if self.session_index.borrow().is_subagent(session_id) {
            return false;
        }
        if self.sidebar_open.get() {
            return true;
        }
        crate::components::sidebar::sidebar_visible(self.sidebar_mode.get(), width)
    }

    /// `session.sidebar.toggle` (session-frame.tsx:118-130): when the
    /// sidebar is showing, `hide` persists and the manual flag clears;
    /// when hidden, `auto` persists and the manual flag forces it on
    /// (v2 writes `draft.session.sidebar` through `config.update` and
    /// flips the `sidebarOpen` signal in one batch). No-op off the
    /// session route (the command lives in the session frame's layer).
    pub fn toggle_sidebar(&self) {
        if !matches!(self.route.get(), Route::Session { .. }) {
            return;
        }
        let visible = matches!(self.route.get(), Route::Session { session_id } if self.sidebar_shown(&session_id, usize::from(self.terminal_size.get().0)));
        let mode = if visible {
            crate::components::sidebar::SidebarMode::Hide
        } else {
            crate::components::sidebar::SidebarMode::Auto
        };
        self.sidebar_mode.set(mode);
        self.sidebar_open.set(!visible);
        // v2's toggle writes `session.sidebar` through config.update
        // (the SURF-2 seam — replaces the channel-storage diversion
        // F-1 registered).
        self.config_sink
            .borrow()
            .update_session_sidebar(matches!(mode, crate::components::sidebar::SidebarMode::Hide));
    }

    /// The configured theme-mode pin (`theme.mode`: dark/light/system
    /// — `None` before the first `apply_config`).
    #[must_use]
    pub fn theme_mode_pref(&self) -> Option<String> {
        self.theme_mode_pref.borrow().clone()
    }

    /// The selected theme's registry name (the Settings row footer).
    #[must_use]
    pub fn theme_name(&self) -> String {
        self.theme.get().name
    }

    /// The accounts view's armed-delete credential id.
    #[must_use]
    pub fn integration_deleting(&self) -> Option<String> {
        self.integration_deleting.get()
    }

    /// Boot/reload: record the configured pin + the terminal probe.
    pub fn set_theme_mode_inputs(&self, pref: Option<String>, system: oc_theme::types::Mode) {
        *self.theme_mode_pref.borrow_mut() = pref;
        self.system_mode.set(system);
    }

    /// `theme.switch_mode` (app.tsx:1054-1062): toggle the resolved
    /// mode and PIN it (the write lands in cli.json's `theme.mode`).
    pub fn theme_switch_mode(&self) {
        let target = if self.theme.get().mode == oc_theme::types::Mode::Dark {
            oc_theme::types::Mode::Light
        } else {
            oc_theme::types::Mode::Dark
        };
        self.switch_theme_mode(target);
        let pref = if target == oc_theme::types::Mode::Dark { "dark" } else { "light" };
        *self.theme_mode_pref.borrow_mut() = Some(pref.to_owned());
        self.config_sink.borrow().update_theme_mode(pref);
    }

    /// `theme.mode.lock` (app.tsx:1064-1073): a system-following
    /// theme pins its CURRENT resolved mode; a pinned one unlocks
    /// back to system (re-resolving against the terminal probe).
    pub fn theme_mode_lock(&self) {
        let locked = self.theme_mode_pref.borrow().as_deref().is_some_and(|pref| pref != "system");
        if locked {
            *self.theme_mode_pref.borrow_mut() = Some("system".to_owned());
            self.switch_theme_mode(self.system_mode.get());
            self.config_sink.borrow().update_theme_mode("system");
        } else {
            let current =
                if self.theme.get().mode == oc_theme::types::Mode::Dark { "dark" } else { "light" };
            *self.theme_mode_pref.borrow_mut() = Some(current.to_owned());
            self.config_sink.borrow().update_theme_mode(current);
        }
    }

    /// Install the config persistence sink (the run loop's cli.json
    /// writer; tests record).
    pub fn set_config_sink(&self, sink: Rc<dyn ConfigSink>) {
        *self.config_sink.borrow_mut() = sink;
    }

    /// The installed config sink (the settings cycle's write-through —
    /// TUI-23's generic `update_setting` consumers).
    #[must_use]
    pub fn config_sink_handle(&self) -> Rc<dyn ConfigSink> {
        self.config_sink.borrow().clone()
    }

    // -- the composer's subagent picker (#44) ------------------------------

    /// The picker state (v2's Session `composer` store + the tab's
    /// memory filter).
    #[must_use]
    pub fn picker(&self) -> PickerState {
        self.picker.get()
    }

    /// Replace the picker state wholesale (tests; the commands go
    /// through the semantic methods above).
    pub fn set_picker(&self, state: PickerState) {
        self.picker.set(state);
    }

    /// The session registry's view of one session (`data.session.get`).
    #[must_use]
    pub fn session_info(
        &self,
        session_id: &str,
    ) -> Option<crate::notifications_plugin::SessionInfo> {
        self.session_index.borrow().get(session_id).cloned()
    }

    /// The live child-session statuses for the transcript's subagent
    /// cards (`data.session.status` — the `isRunning` second clause).
    #[must_use]
    pub fn subagent_running_map(&self) -> std::collections::HashMap<String, bool> {
        self.session_index.borrow().running_ids().into_iter().map(|id| (id, true)).collect()
    }

    /// `isSubagent` — the route's session has a known parent.
    #[must_use]
    pub fn session_is_subagent(&self, session_id: &str) -> bool {
        self.session_index.borrow().is_subagent(session_id)
    }

    /// The family root of the routed session (`session-tabs.tsx`'s
    /// `root()` — tab bookkeeping and the strip's active pill key on
    /// it, never the child id itself).
    #[must_use]
    pub fn session_root(&self, session_id: &str) -> String {
        self.session_index.borrow().root_of(session_id)
    }

    /// The picker panel's mount condition (v2
    /// routes/session/index.tsx:1451): explicitly open, OR the session
    /// is a subagent (`parentID`) with no blocking forms — children
    /// REPLACE the prompt with the panel.
    #[must_use]
    pub fn picker_live(&self, session_id: &str) -> bool {
        self.picker.get().open || self.session_is_subagent(session_id)
    }

    /// `session.child.first` (index.tsx:1255-1263): on the session
    /// route — open when closed (subagents tab), close when open; on
    /// a CHILD session the parentID clause keeps it mounted (the
    /// explicit `open` flag merely resets).
    pub fn toggle_picker(&self) {
        if !matches!(self.route.get(), Route::Session { .. }) {
            return;
        }
        let is_child = matches!(self.route.get(), Route::Session { ref session_id } if self.session_is_subagent(session_id));
        let mut picker = self.picker.get();
        if picker.open || is_child {
            picker.open = false;
        } else {
            picker.open = true;
            picker.tab = PickerTab::Subagents;
        }
        self.picker.set(picker);
    }

    /// The Composer's `close()` (index.tsx:1453-1460): on a child
    /// session closing NAVIGATES TO THE PARENT; otherwise the panel
    /// folds back to the prompt. Either way the explicit open flag
    /// clears — v2's session-frame is keyed by sessionID, so the
    /// parent's remount starts with a fresh (closed) composer store.
    pub fn picker_close(&self) {
        if let Route::Session { session_id } = self.route.get() {
            let parent = self
                .session_index
                .borrow()
                .get(&session_id)
                .and_then(|info| info.parent_id.clone());
            self.picker.update(|picker| picker.open = false);
            if let Some(parent) = parent {
                self.navigate(Route::Session { session_id: parent });
            }
        } else {
            self.picker.update(|picker| picker.open = false);
        }
    }

    /// The picker's tab cycle (`switchTab`, composer/index.tsx:65-70).
    pub fn picker_switch_tab(&self, forward: bool) {
        let mut picker = self.picker.get();
        picker.tab = picker.tab.cycle(forward);
        picker.selected = 0;
        picker.anchored_to = None;
        self.picker.set(picker);
    }

    /// `composer.subagent.toggle-activity` (subagents-tab.tsx:173-181):
    /// flip the running/inactive filter, reset the selection.
    pub fn picker_toggle_filter(&self) {
        let mut picker = self.picker.get();
        picker.filter_active = !picker.filter_active;
        picker.selected = 0;
        self.picker.set(picker);
    }

    /// The picker's family entries for the routed session
    /// (`sessionFamily`, util/session.ts:9-35: the ROOT's descendant
    /// tree in registry order with `├─ `/`└─ ` prefixes), filtered by
    /// the tab's activity filter (subagents-tab.tsx:39-63).
    #[must_use]
    pub fn picker_entries(&self, session_id: &str) -> Vec<PickerEntry> {
        // TUI-4 — the Terminals tab lists the registry rows + the
        // `+ New terminal` slot (terminals-tab.tsx:79-108), no
        // activity filter.
        if self.picker.get().tab == PickerTab::Terminals {
            return self.picker_terminals(session_id);
        }
        let index = self.session_index.borrow();
        // Registry order: insertion (first-seen) order like the client
        // cache — `ids()` is sorted, a stable deterministic stand-in.
        let ids = index.ids();
        let mut children: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        for id in &ids {
            if let Some(parent) = index.get(id).and_then(|info| info.parent_id.as_deref()) {
                children.entry(parent).or_default().push(id);
            }
        }
        let mut entries: Vec<PickerEntry> = Vec::new();
        picker_family_walk(
            &index,
            &children,
            &index.root_of(session_id),
            &[],
            session_id,
            &mut entries,
        );
        let filter_active = self.picker.get().filter_active;
        entries.retain(|entry| entry.running == filter_active);
        entries
    }

    /// The selection with the anchor/clamp discipline applied
    /// (subagents-tab.tsx:71-94): anchoring to the route's entry on
    /// tab activation or route change, clamping when the filtered
    /// list shrinks. Returns the effective selected index.
    #[must_use]
    pub fn picker_selection(&self, session_id: &str, entries_len: usize) -> usize {
        let mut picker = self.picker.get();
        if picker.anchored_to.as_deref() != Some(session_id) {
            picker.anchored_to = Some(session_id.to_owned());
            picker.selected = 0;
            let entries = self.picker_entries(session_id);
            if let Some(index) = entries.iter().position(|entry| entry.current) {
                picker.selected = index;
            }
            self.picker.set(picker.clone());
        }
        if entries_len == 0 {
            picker.selected = 0;
        } else {
            picker.selected = picker.selected.min(entries_len - 1);
        }
        picker.selected
    }

    /// `composer.subagent.up/down` (subagents-tab.tsx:140-162): the
    /// wrap-around move (`up` at 0 CLOSES the composer — even with
    /// an empty filtered list, the tsx's close check precedes the
    /// move; `down` on an empty list is inert).
    pub fn picker_move(&self, forward: bool) {
        let route = match self.route.get() {
            Route::Session { session_id } => session_id,
            Route::Home => return,
        };
        let len = self.picker_entries(&route).len();
        if forward && len == 0 {
            return;
        }
        let mut picker = self.picker.get();
        if forward {
            picker.selected = (picker.selected + 1) % len;
        } else if picker.selected == 0 {
            drop(picker);
            self.picker_close();
            return;
        } else {
            picker.selected -= 1;
        }
        self.picker.set(picker);
    }

    /// `composer.subagent.select` (subagents-tab.tsx:164-170):
    /// navigate into the selected child session (the route keeps the
    /// ROOT's tab — `navigate` roots the tab bookkeeping).
    pub fn picker_navigate(&self) {
        let route = match self.route.get() {
            Route::Session { session_id } => session_id,
            Route::Home => return,
        };
        let entries = self.picker_entries(&route);
        let selected = self.picker_selection(&route, entries.len());
        if let Some(entry) = entries.get(selected) {
            let target = entry.session_id.clone();
            self.navigate(Route::Session { session_id: target });
        }
    }

    /// `composer.subagent.interrupt` — the selected RUNNING entry's
    /// interrupt POST.
    pub fn picker_interrupt(&self) {
        let route = match self.route.get() {
            Route::Session { session_id } => session_id,
            Route::Home => return,
        };
        let entries = self.picker_entries(&route);
        let selected = self.picker_selection(&route, entries.len());
        if entries.get(selected).is_some_and(|entry| entry.running) {
            self.interrupt_sink.borrow().interrupt(&entries[selected].session_id);
        }
    }

    /// The exit epilogue (#46): v2's session route sets
    /// `sessionEpilogue({title, sessionID})` while mounted
    /// (index.tsx:185-189) and app.tsx:443-450 writes it to stdout
    /// after teardown. `None` off the session route.
    #[must_use]
    pub fn exit_epilogue(&self) -> Option<String> {
        let Route::Session { session_id } = self.route.get() else {
            return None;
        };
        let index = self.session_index.borrow();
        let title = index.title(&session_id).unwrap_or_default().to_owned();
        // Lane M6 (retired): the farewell renders through the
        // opentui-solid tree — the only renderer since the
        // retirement lane deleted the flag pair.
        Some(crate::farewell_solid::session_epilogue_dispatch(&title, &session_id))
    }

    /// The onboarding card's dismissal (v2 storage
    /// `getting-started.dismissed`, footer.tsx:11-23): the `✕`
    /// release path (`sidebar.footer.getting-started.dismiss`) —
    /// sets the signal; the run loop observes the flip and persists
    /// the document.
    pub fn dismiss_sidebar_onboarding(&self) {
        self.sidebar_onboarding.set(true);
    }

    /// `config.session.grouping !== "none"` (lane KM): the
    /// exploration-grouping flag the transcript walk reads.
    #[must_use]
    pub fn exploration_grouping(&self) -> bool {
        self.exploration_grouping.get()
    }

    /// `session.toggle.exploration_grouping` (index.tsx:1049-1061 —
    /// the "Group related tool calls" toggle): flip the flag; the
    /// next transcript pass re-groups (or explodes) the exploration
    /// rows.
    pub fn toggle_exploration_grouping(&self) -> bool {
        let next = !self.exploration_grouping.get();
        self.exploration_grouping.set(next);
        self.request_redraw();
        next
    }

    // -- CONFIG-C8: session retention (context/session-retention.ts) --

    /// `session.evict(root)` (client/solid/data.ts:1363-1369): clear
    /// the heavy cached data for the root and all known descendants —
    /// the message trees, loading flags, and pending-compaction
    /// dividers. Family membership resolves through the session index
    /// (every listed session whose root is `session_id`'s root).
    pub fn session_evict(&self, session_id: &str) {
        let family = {
            let index = self.session_index.borrow();
            let root = index.root_of(session_id);
            let mut members: Vec<String> = vec![root.clone()];
            for id in index.ids() {
                if index.root_of(id) == root {
                    members.push(id.to_owned());
                }
            }
            members
        };
        self.messages.update(|state| {
            for id in &family {
                state.by_session.remove(id);
            }
        });
        self.history_loading.update(|state| {
            for id in &family {
                state.remove(id);
            }
        });
        self.pending_compactions.update(|state| {
            for id in &family {
                state.remove(id);
            }
        });
    }

    /// `createSessionRetention`'s effect (context/session-retention.ts:
    /// 14-39, `limit: 3` at session-tabs.tsx:144): retain the open
    /// tabs' roots, the routed root, and the newest
    /// [`RETENTION_LIMIT`] navigated roots outside the keep set; every
    /// other listed session's root evicts. Only NAVIGATION advances
    /// the recency window (`viewed !== previous`).
    pub fn enforce_session_retention(&self, current: Option<&str>, navigated: bool) {
        let root_of = |id: &str| self.session_index.borrow().root_of(id);
        let current_root = current.map(root_of);
        // `recent` — newest first, roots re-resolved (metadata may
        // have arrived since the last pass).
        let mut recent: Vec<String> = Vec::new();
        if navigated {
            if let Some(root) = &current_root {
                recent.push(root.clone());
            }
        }
        for id in self.retention_recent.get() {
            let root = root_of(&id);
            if !recent.contains(&root) {
                recent.push(root.clone());
            }
        }
        let keep: std::collections::HashSet<String> =
            self.tabs.get().tabs.iter().map(|tab| root_of(&tab.session_id)).collect();
        // `retained = [...keep, ...recent.filter(!keep).slice(0, limit)]`
        // + the routed root.
        let mut retained: std::collections::HashSet<String> = keep.clone();
        for root in recent.iter().filter(|root| !keep.contains(*root)).take(RETENTION_LIMIT) {
            retained.insert(root.clone());
        }
        if let Some(root) = &current_root {
            retained.insert(root.clone());
        }
        // `recent = recent.filter(retained.has)` — evicted roots drop
        // out of the window.
        recent.retain(|root| retained.contains(root));
        self.retention_recent.set(recent);
        // Evict every listed session whose root fell outside the
        // retained set (`session.list()` drives the exclusion map).
        let listed: Vec<String> =
            self.session_history.get().iter().map(|session| session.id.clone()).collect();
        for id in listed {
            let root = root_of(&id);
            if !retained.contains(&root) {
                self.session_evict(&id);
            }
        }
    }

    /// The boot restore (the document's `dismissed` seeds the signal —
    /// the run loop reads `<state>/<ch>/tui/getting-started.json`
    /// before the first frame, mirroring v2's `storage.store(...)`
    /// initial value).
    pub fn restore_sidebar_onboarding(&self, dismissed: bool) {
        self.sidebar_onboarding.set(dismissed);
    }

    /// Whether the getting-started card was dismissed.
    #[must_use]
    pub fn sidebar_onboarding_dismissed(&self) -> bool {
        self.sidebar_onboarding.get()
    }

    /// Arm/disarm permission autoaccept (`--auto`; boot-only in
    /// practice — v2's args are boot-immutable).
    pub fn set_permission_autoaccept(&self, armed: bool) {
        self.permission_autoaccept.set(armed);
    }

    // -- CONFIG-T3: the session.* display keys' read side -----------

    /// `session.thinking === "show"`.
    #[must_use]
    pub fn session_thinking(&self) -> bool {
        self.session_thinking.get()
    }

    /// The `session.thinking` key (default hide — the collapsed
    /// reasoning disclosure; show renders thoughts inline).
    pub fn set_session_thinking(&self, show: bool) {
        self.session_thinking.set(show);
        self.request_redraw();
    }

    /// The `session.verbosity` level (`low` | `medium` | `high`).
    #[must_use]
    pub fn session_verbosity(&self) -> String {
        self.session_verbosity.get()
    }

    /// The `session.verbosity` key (low wraps activity runs; high
    /// opens exploration/instruction summaries).
    pub fn set_session_verbosity(&self, level: &str) {
        self.session_verbosity.set(level.to_owned());
        self.request_redraw();
    }

    /// `session.tps`.
    #[must_use]
    pub fn session_tps(&self) -> bool {
        self.session_tps.get()
    }

    /// The `session.tps` key (the assistant footer's tok/s span).
    pub fn set_session_tps(&self, enabled: bool) {
        self.session_tps.set(enabled);
        self.request_redraw();
    }

    /// `session.markdown === "source"`.
    #[must_use]
    pub fn session_markdown_source(&self) -> bool {
        self.session_markdown_source.get()
    }

    /// The `session.markdown` key (source renders raw text parts).
    pub fn set_session_markdown_source(&self, source: bool) {
        self.session_markdown_source.set(source);
        self.request_redraw();
    }

    /// The `session.grouping` key's boot restore (`grouping !== "none"`
    /// — index.tsx:233; the toggle flips it at runtime).
    pub fn restore_session_grouping(&self, grouped: bool) {
        self.exploration_grouping.set(grouped);
    }

    /// Whether permission autoaccept is armed (the composer
    /// metadata's `auto` marker — v2 `local.permission.mode ===
    /// "autoaccept"`).
    #[must_use]
    pub fn permission_autoaccept(&self) -> bool {
        self.permission_autoaccept.get()
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
        // SURF-2: the Reconnecting overlay's timer anchor — the clock
        // reading when the state LAST CHANGED (app.tsx:1298-1315's
        // effect re-arms the timeout on every transition).
        if self.connection.get() != state {
            self.connection_since.set(self.clock.get());
        }
        self.connection.set(state);
    }

    /// When the connection state last changed (ms; 0 before any
    /// transition) — the overlay's threshold anchor.
    #[must_use]
    pub fn connection_since(&self) -> u64 {
        self.connection_since.get()
    }

    /// Whether the Reconnecting overlay shows at `now`
    /// (app.tsx:1294-1315): every non-connected state arms a timer —
    /// `reconnecting` surfaces after 1 s, the initial/failed states
    /// after 5 s ("Initial connection gets a longer grace period;
    /// retries surface more quickly").
    #[must_use]
    pub fn reconnecting_overlay_visible(&self, now_ms: u64) -> bool {
        let state = self.connection.get();
        if state == ConnectionState::Connected {
            return false;
        }
        let threshold =
            if matches!(state, ConnectionState::Reconnecting { .. }) { 1000 } else { 5000 };
        now_ms.saturating_sub(self.connection_since.get()) >= threshold
    }

    /// The `StartupLoading` machine (startup-loading.tsx:13-47): the
    /// pill appears 500 ms after plugins go un-ready and holds 3 s
    /// from the moment it showed once ready ("Finishing startup…").
    /// Returns the pill's text when visible.
    #[must_use]
    pub fn startup_loading_text(&self, now_ms: u64) -> Option<&'static str> {
        let mut state = self.startup_state.borrow_mut();
        if state.ready {
            if let Some(shown_at) = state.shown_at {
                if now_ms.saturating_sub(shown_at) >= 3000 {
                    state.shown_at = None;
                }
            }
            return state.shown_at.map(|_| "Finishing startup…");
        }
        match (state.wait_started_at, state.shown_at) {
            (None, _) => {
                state.wait_started_at = Some(now_ms);
                None
            }
            (Some(started), None) => {
                if now_ms.saturating_sub(started) >= 500 {
                    state.shown_at = Some(now_ms);
                    return Some("Loading plugins…");
                }
                None
            }
            (Some(_), Some(_)) => Some("Loading plugins…"),
        }
    }

    /// The plugin host finished booting (or none was configured) —
    /// flips the `StartupLoading` copy to "Finishing startup…" and
    /// starts the 3 s hold.
    pub fn set_plugins_ready(&self) {
        self.startup_state.borrow_mut().ready = true;
    }

    /// A permission reply resolved the ask (the pump's reply path and
    /// the SSE `permission.replied` reducer share this).
    pub fn permission_replied(&self, request_id: &str) {
        self.footer.update(|footer| footer.permission_replied(request_id));
        // The ask's pointer targets die with it (auditA #4: the
        // pills/buttons belong to the live body — a resolved ask
        // must leave no zombie rects).
        *self.permission_pointer_hits.borrow_mut() = None;
    }

    /// `form.replied` / `form.cancelled` — the local settle arm the
    /// pump's reply/cancel queue also drives (the server echo arrives
    /// through the same reducer, deduplicated by the footer queue).
    pub fn form_removed(&self, form_id: &str) {
        self.footer.update(|footer| footer.form_removed(form_id));
    }

    /// Record the permission body's pointer targets at ABSOLUTE
    /// screen cells (the render pass — `App::frame`'s permission
    /// mount; the layout math lives with the body renderer).
    pub fn set_permission_pointer_hits(
        &self,
        hits: Option<crate::components::footer::permission::PermissionPointerHits>,
    ) {
        *self.permission_pointer_hits.borrow_mut() = hits;
    }

    /// The permission body's pointer targets from the last render.
    #[must_use]
    pub fn permission_pointer_hits(
        &self,
    ) -> Option<crate::components::footer::permission::PermissionPointerHits> {
        self.permission_pointer_hits.borrow().clone()
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

    #[allow(clippy::too_many_lines)] // one arm per wire event; splitting scatters the ingest contract
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
                        action_command: None,
                        // The F11 modal surface (keystrokes swallow
                        // until dismissed).
                        modal: true,
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
            Ingest::Resync { sessions, unread, messages, cursors, permissions, forms } => {
                // A resync only happens after a successful subscribe —
                // the stream is attached, the client is connected
                // (SURF-2: servers that never emit the `server.
                // connected` event must not leave the state Connecting
                // forever, or the Reconnecting scrim would cover a
                // healthy session).
                if self.connection.get() != ConnectionState::Connected {
                    self.set_connection(ConnectionState::Connected);
                }
                // TUI-TD05 — the resync's tail pages carry each open
                // session's older-history cursor (`messageCursor`,
                // client/solid/data.ts:1652: `more()` + `loadMore`
                // walk the pages the boot mount pinned away).
                for (session_id, cursor) in cursors {
                    self.set_message_cursor(&session_id, cursor);
                }
                // Reconcile AFTER the channel drained (v2's
                // `normalize()` effect, `session-tabs.tsx:241-250`):
                // the resync NEVER admits new tabs — only OPEN tabs
                // refresh in place (title updates from the wire);
                // routing is the only way a session enters the strip.
                // The full snapshot becomes the session HISTORY (the
                // session-list dialog's source, like v2's
                // `data.session.list()`).
                self.session_history.set(sessions.clone());
                // The session registry (#44/#46): the snapshot seeds
                // titles AND parent links like v2's
                // `session.sync(sessionID, { children: true })` cache
                // fill (RUNTIME-TD04: the family tree re-builds from
                // the fresh list; events refine agent/status later).
                {
                    let mut index = self.session_index.borrow_mut();
                    for session in &sessions {
                        let info = index.registry_entry_mut(&session.id);
                        if let Some(title) = &session.title {
                            info.title = Some(title.clone());
                        }
                        if let Some(parent) = &session.parent_id {
                            info.parent_id = Some(parent.clone());
                        }
                    }
                }
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
                // RUNTIME-TD04 — the blocker resync + the
                // Session-not-found wrap-up (helpers below).
                self.resync_reconcile_blockers(&permissions, &forms);
                self.resync_close_missing_sessions(&sessions);
                for (session_id, list) in messages {
                    self.set_messages(&session_id, list);
                }
                // CONFIG-C8 — the retention effect re-runs when the
                // session list changes (`input.session.list()` is a
                // dependency of createSessionRetention's effect): no
                // navigation advanced, but new/removed listings can
                // move roots in or out of the retained set.
                let current = match &self.route.get() {
                    Route::Session { session_id } => Some(session_id.clone()),
                    Route::Home => None,
                };
                self.enforce_session_retention(current.as_deref(), false);
            }
            // F12: the session-open transcript fetch (v2
            // `data.session.message.sync`, client/solid/data.ts:1630-1651)
            // — the fetched page REPLACES the bucket's known rows but
            // KEEPS local rows the page does not carry (a prompt
            // admitted between the fetch and its arrival must not be
            // wiped: `[...fetched, ...local]`).
            Ingest::MessagesSnapshot { session_id, messages } => {
                self.messages.update(|state| {
                    let local = state.by_session.remove(&session_id).unwrap_or_default();
                    let mut ids = std::collections::HashSet::new();
                    for message in &messages {
                        ids.insert(message.id().to_owned());
                    }
                    let mut merged = messages;
                    merged.extend(local.into_iter().filter(|message| !ids.contains(message.id())));
                    state.by_session.insert(session_id, merged);
                });
            }
            // TUI-TD05 — the older page PREPENDS (v2 `loadMore`:
            // `[...fetched.reverse().filter(not-known), ...existing]`,
            // client/solid/data.ts:1703-1704) and the cursor advances.
            Ingest::MessagesOlder { session_id, messages, cursor } => {
                self.set_message_cursor(&session_id, cursor);
                self.messages.update(|state| {
                    let known = state.by_session.remove(&session_id).unwrap_or_default();
                    let mut ids = std::collections::HashSet::new();
                    for message in &messages {
                        ids.insert(message.id().to_owned());
                    }
                    let mut merged = messages;
                    merged.extend(known.into_iter().filter(|message| !ids.contains(message.id())));
                    state.by_session.insert(session_id, merged);
                });
            }
        }
    }

    /// RUNTIME-TD04 — the blocker resync (v2
    /// `data.session.permission.sync` / `form.sync` after a reconnect,
    /// routes/session/index.tsx:313-320): asks that happened while
    /// offline re-deliver; asks replied while offline CLEAR. Per-type
    /// replace semantics over the fetched sessions only — an id we
    /// have no data for keeps its current blockers (no fetch, no
    /// decision).
    fn resync_reconcile_blockers(
        &self,
        permissions: &[(String, Vec<crate::components::footer::PermissionRequest>)],
        forms: &[(String, Vec<crate::components::footer::FormRequest>)],
    ) {
        self.footer.update(|footer| {
            let fetched_permissions: std::collections::HashSet<&str> =
                permissions.iter().map(|(id, _)| id.as_str()).collect();
            let live: std::collections::HashSet<&str> = permissions
                .iter()
                .flat_map(|(_, list)| list.iter().map(|request| request.id.as_str()))
                .collect();
            footer.permissions.retain(|request| {
                !fetched_permissions.contains(request.session_id.as_str())
                    || live.contains(request.id.as_str())
            });
            for (_, list) in permissions {
                for request in list {
                    footer.permission_asked(request.clone());
                }
            }
            let fetched_forms: std::collections::HashSet<&str> =
                forms.iter().map(|(id, _)| id.as_str()).collect();
            let live_forms: std::collections::HashSet<&str> = forms
                .iter()
                .flat_map(|(_, list)| list.iter().map(|form| form.id.as_str()))
                .collect();
            footer.forms.retain(|form| {
                !fetched_forms.contains(form.session_id.as_str())
                    || live_forms.contains(form.id.as_str())
            });
            for (_, list) in forms {
                for form in list {
                    footer.form_created(form.clone());
                }
            }
        });
    }

    /// RUNTIME-TD04 — the "Session not found" wrap-up (v2
    /// routes/session/index.tsx:340-353): a session deleted while
    /// offline is absent from the snapshot; its tab closes
    /// (`close_tab` routes to the neighbor, else home) and the ACTIVE
    /// route's miss surfaces the v2 toast.
    fn resync_close_missing_sessions(&self, sessions: &[crate::data::SessionSnapshot]) {
        let live_ids: std::collections::HashSet<&str> =
            sessions.iter().map(|session| session.id.as_str()).collect();
        let open_tabs: Vec<String> =
            self.tabs().tabs.iter().map(|tab| tab.session_id.clone()).collect();
        for id in open_tabs {
            if live_ids.contains(id.as_str()) {
                continue;
            }
            if let Route::Session { session_id } = self.route() {
                if session_id == id {
                    self.toast_show(ToastOptions {
                        title: None,
                        message: format!("Session not found: {id}"),
                        variant: "error",
                        duration_ms: 5000,
                        action_label: None,
                        action_command: None,
                        modal: false,
                    });
                }
            }
            if self.tabs_enabled() {
                self.close_tab(Some(&id));
            } else if let Route::Session { session_id } = self.route() {
                if session_id == id {
                    self.navigate(Route::Home);
                }
            }
        }
    }

    #[allow(clippy::too_many_lines)] // one match over the event alphabet
    #[allow(clippy::too_many_lines)] // the SSE reducer table (one arm per event)
    fn reduce_event(&self, event: &BusEvent) {
        match event.type_.as_str() {
            // TUI-4 — the registry refresh triggers
            // (session-terminals.tsx:48-58: refresh the session's
            // terminal list when the server announces one).
            "persistent-pty.added" | "persistent-pty.removed" => {
                let Some(id) = event.string("sessionID") else { return };
                let known = self.terminal_registry.get().iter().any(|entry| entry.session_id == id);
                if self.terminal_available.get() && known {
                    self.terminal_sink.borrow().refresh(&id);
                }
            }
            // TUI-12 — the live shell registry (the client data
            // cache's per-location `shell` map, solid/data.ts:
            // 1245-1261): `shell.created` inserts the in-flight row,
            // `shell.exited`/`shell.deleted` remove it — only running
            // shells ever list.
            "shell.created" => {
                let Some(info) = event.data.get("info") else { return };
                let shell = LiveShell {
                    id: info
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    session_id: info
                        .pointer("/metadata/sessionID")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                };
                if shell.id.is_empty() {
                    return;
                }
                let mut shells = self.shells.get();
                match shells.iter_mut().find(|row| row.id == shell.id) {
                    Some(row) => *row = shell,
                    None => shells.push(shell),
                }
                self.shells.set(shells);
            }
            "shell.exited" | "shell.deleted" => {
                let Some(id) = event.string("id") else { return };
                let mut shells = self.shells.get();
                shells.retain(|row| row.id != id);
                self.shells.set(shells);
            }
            "session.updated" | "session.renamed" => {
                let Some(id) = event.string("sessionID") else { return };
                let title = event.string("title");
                // The rename echo projects the new title locally —
                // rename-pending releases (`generatingTitles`'s
                // finally + the projection note, context/data.tsx:42).
                if self.title_pending.get().contains_key(&id) {
                    self.set_title_pending(&id, None);
                }
                // The session-list history refreshes in place (v2's
                // renamed handler writes the session store's
                // `info[sessionID].title` — client/solid/data.ts:687-699
                // — and `data.session.list()` re-derives from it).
                if let Some(title) = &title {
                    self.session_history.update(|history| {
                        if let Some(row) = history.iter_mut().find(|row| row.id == id) {
                            row.title = Some(title.clone());
                        }
                    });
                }
                self.tabs.update(|state| {
                    if state.tabs.iter().any(|tab| tab.session_id == id) {
                        state.tabs = model::open_session_tab(
                            std::mem::take(&mut state.tabs),
                            SessionTab { session_id: id.clone(), title },
                        );
                    }
                });
            }
            // The updater's installation notices (v2's
            // `installation.update-available`/`installation.updated`
            // handlers, update-notification.tsx:113-121 — the
            // server-hosting process's own updater).
            "installation.update-available" => {
                if let Some(version) = event.string("version") {
                    self.update_notify(crate::components::update::UpdateNotice {
                        kind: crate::components::update::UpdateNoticeKind::Available,
                        version,
                        remote: false,
                    });
                }
            }
            "installation.updated" => {
                if let Some(version) = event.string("version") {
                    self.update_notify(crate::components::update::UpdateNotice {
                        kind: crate::components::update::UpdateNoticeKind::Installed,
                        version,
                        remote: false,
                    });
                }
            }
            "session.deleted" => {
                let Some(id) = event.string("sessionID") else { return };
                // CONFIG-C8 — `session.deleted` clears the family's
                // cached data (removeSession → the evict surface,
                // client/solid/data.ts:642-644): the message trees,
                // loading flags, and pending dividers drop alongside
                // the tab close.
                self.session_evict(&id);
                if self.tabs.get().tabs.iter().any(|tab| tab.session_id == id) {
                    self.close_tab(Some(&id));
                }
            }
            // `session.usage.updated` (client/solid/data.ts:646-652,
            // TUI-13): the per-session cost total rides the public
            // event — `info[id].cost = data.cost`. The sidebar's
            // `$X.XX spent` row and the composer footer's money
            // segment read it through [`Store::session_cost`].
            "session.usage.updated" => {
                let Some(id) = event.string("sessionID") else { return };
                let Some(cost) = event.data.get("cost").and_then(serde_json::Value::as_f64) else {
                    return;
                };
                self.session_costs.update(|costs| {
                    costs.insert(id, cost);
                });
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
                    action_command: None,
                    modal: false,
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
                    // TUI-TD07 — the pending-inbox admit (`admitLocal`,
                    // data.ts:370-383): the item's delivery + resolved
                    // file payloads mirror into the registry the queue
                    // dock + pending-steer markers read.
                    let delivery =
                        item.get("delivery").and_then(Value::as_str).unwrap_or("steer").to_owned();
                    let files = item
                        .pointer("/payload/files")
                        .and_then(Value::as_array)
                        .map(|files| {
                            files
                                .iter()
                                .filter_map(|file| {
                                    let mime = file.get("mime")?.as_str()?;
                                    let data = file.get("data")?.as_str()?;
                                    Some(format!("data:{mime};base64,{data}"))
                                })
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default();
                    self.admit_inbox_item(
                        &session,
                        InboxItem { id: id.clone(), delivery, text: text.to_owned(), files },
                    );
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
                // A queued compaction rides the inbox the same way
                // (data.ts:783-788 → pending.list → the `◇ Compaction
                // queued` rows, rows.ts:72-79).
                if item.get("type").and_then(Value::as_str) == Some("compaction") {
                    let id = event.string("inboxID").unwrap_or_else(|| event.id.clone());
                    self.pending_compactions.update(|queues| {
                        let queue = queues.entry(session).or_default();
                        if !queue.iter().any(|pending| pending == &id) {
                            queue.push(id);
                        }
                    });
                }
            }
            // TUI-TD07 — `session.inbox.delivery.changed`
            // (`updatePending`, data.ts:765-767): the steer/queue flip
            // lands server-side (the PATCH's echo).
            "session.inbox.delivery.changed" => {
                let Some(session) = event.string("sessionID") else { return };
                let Some(id) = event.string("inboxID") else { return };
                if let Some(delivery) = event.string("delivery") {
                    self.update_inbox_delivery(&session, &id, &delivery);
                }
            }
            // TUI-TD07 — `session.inbox.cancelled` (`retractLocal`,
            // data.ts:775-780): the pending row AND its admitted
            // transcript row disappear.
            "session.inbox.cancelled" => {
                let Some(session) = event.string("sessionID") else { return };
                let Some(id) = event.string("inboxID") else { return };
                self.retract_inbox_item(&session, &id);
            }
            // TUI-TD07 — `session.inbox.delivered` (`removePending`,
            // data.ts:749-763): the server admitted the item into the
            // turn — the pending marker clears (the transcript row
            // stays as the admitted user message).
            "session.inbox.delivered" => {
                let Some(session) = event.string("sessionID") else { return };
                let Some(id) = event.string("inboxID") else { return };
                self.pending_inbox.update(|state| {
                    if let Some(items) = state.get_mut(&session) {
                        items.retain(|item| item.id != id);
                    }
                });
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
            "session.reasoning.started" => {
                // #14 (`+ Thought · 8ms`): the group duration sums
                // `time.completed - time.created` per thought — the
                // START event carries `created` (without it the
                // reducer left `time.created` None and the suffix
                // never rendered).
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let ordinal = event.ordinal();
                let created = event.created;
                self.update_messages(&session, |messages| {
                    if message_already_complete(messages, &assistant) {
                        return;
                    }
                    with_part(messages, &assistant, "reasoning", ordinal, |part| {
                        if let Part::Reasoning { time, .. } = part {
                            time.created = created;
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
                        if let Part::Tool { name: slot, time, state, .. } = part {
                            *slot = name;
                            *time = PartTime { created, completed: None };
                            // v2 pushes `{status: "streaming", input: ""}`
                            // (data.ts:919-929).
                            *state = crate::components::message::model::ToolState::default();
                        }
                    });
                });
            }
            // The live tool-state reducers (v2 client data.ts:930-980):
            // called → running with the parsed input; progress replaces
            // a running tool's metadata (the subagent tool's child
            // `sessionID` arrives this way); success/failed settle it.
            // (`session.tool.input.delta/ended` only advance the raw
            // arg STRING while streaming — display-inert: the card
            // reads the parsed map after `called`, so no reducer.)
            "session.tool.called" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let Some(part_id) = event.string("id") else { return };
                let input = event.data.get("input").cloned();
                let created = event.created;
                self.update_messages(&session, |messages| {
                    with_tool_part(messages, &assistant, &part_id, |part| {
                        if let Part::Tool { time, state, .. } = part {
                            *state = crate::components::message::model::ToolState {
                                status: ToolStatus::Running,
                                input: crate::data::stringify_map(input.as_ref()),
                                metadata: std::collections::BTreeMap::new(),
                                metadata_json: None,
                                output: None,
                                error: None,
                            };
                            if time.created.is_none() {
                                time.created = created;
                            }
                        }
                    });
                });
            }
            "session.tool.progress" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let Some(part_id) = event.string("id") else { return };
                let metadata = event.data.get("metadata").cloned();
                self.update_messages(&session, |messages| {
                    with_tool_part(messages, &assistant, &part_id, |part| {
                        if let Part::Tool { state, .. } = part {
                            if state.status == ToolStatus::Running {
                                state.metadata = crate::data::stringify_map(metadata.as_ref());
                            }
                        }
                    });
                });
            }
            "session.tool.success" | "session.tool.failed" => {
                let Some((session, assistant)) = session_and_assistant(event) else { return };
                let Some(part_id) = event.string("id") else { return };
                let metadata = event.data.get("metadata").cloned();
                // The settled state's detail payload (data.ts:954-985):
                // success joins `content`'s text blocks into the output
                // preview; failed also keeps `error.message` for the
                // error row + DialogExecute's error variant.
                let output = event
                    .data
                    .get("content")
                    .and_then(|content| content.as_array())
                    .map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|block| {
                                let text =
                                    block.get("text").and_then(Value::as_str).map(str::to_owned);
                                text.or_else(|| {
                                    block
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .map(str::to_owned)
                                        .or_else(|| {
                                            block
                                                .get("uri")
                                                .and_then(Value::as_str)
                                                .map(str::to_owned)
                                        })
                                })
                            })
                            .filter(|text| !text.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .filter(|text| !text.is_empty());
                let error = (event.type_ == "session.tool.failed")
                    .then(|| {
                        event
                            .data
                            .pointer("/error/message")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .flatten();
                let completed = event.created;
                self.update_messages(&session, |messages| {
                    with_tool_part(messages, &assistant, &part_id, |part| {
                        if let Part::Tool { time, state, .. } = part {
                            if !state.status.settled() {
                                state.status = if event.type_ == "session.tool.success" {
                                    ToolStatus::Completed
                                } else {
                                    ToolStatus::Error
                                };
                                if !metadata_is_empty(metadata.as_ref()) {
                                    state.metadata = crate::data::stringify_map(metadata.as_ref());
                                    state.metadata_json =
                                        metadata.clone().filter(|value| !value.is_null());
                                }
                                state.output.clone_from(&output);
                                state.error.clone_from(&error);
                                time.completed = time.completed.or(completed);
                            }
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
                                tokens: None,
                                usage_total: None,
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
                // #13: the footer's tok/s numerator + the context
                // usage total (`turnTokensPerSecond` rows.ts:442-463 /
                // `contextUsage` util/session.ts:58-73).
                let token_parts = event.data.get("tokens").map(|tokens| {
                    let part = |key: &str| tokens.get(key).and_then(Value::as_u64).unwrap_or(0);
                    let cache = tokens.get("cache");
                    let cache_read = cache
                        .and_then(|cache| cache.get("read"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let cache_write = cache
                        .and_then(|cache| cache.get("write"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    (
                        part("output") + part("reasoning"),
                        part("input")
                            + part("output")
                            + part("reasoning")
                            + cache_read
                            + cache_write,
                    )
                });
                let step_ended = event.type_ == "session.step.ended";
                let tokens = token_parts
                    .filter(|(output_reasoning, _)| *output_reasoning > 0)
                    .filter(|_| step_ended)
                    .map(|(output_reasoning, _)| output_reasoning);
                let usage_total = token_parts
                    .filter(|(_, total)| *total > 0)
                    .filter(|_| step_ended)
                    .map(|(_, total)| total);
                self.update_messages(&session, |messages| {
                    if let Some(Message::Assistant {
                        finish: slot,
                        time,
                        tokens: token_slot,
                        usage_total: usage_slot,
                        ..
                    }) = messages.iter_mut().find(|m| m.id() == assistant.as_str())
                    {
                        *slot = finish;
                        time.completed = time.completed.or(completed);
                        if let Some(tokens) = tokens {
                            *token_slot = Some(tokens);
                        }
                        if let Some(total) = usage_total {
                            *usage_slot = Some(total);
                        }
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
                let appended = self.update_messages(&session, |messages| {
                    let fresh = messages.iter().all(|m| m.id() != id.as_str());
                    if fresh {
                        messages.push(Message::Idle { id, created });
                    }
                    fresh
                });
                if appended {
                    // The turn ended: the interrupt arm collapses (the
                    // hint row goes). TUI-TD07: the queued prompts are
                    // the SERVER's inbox now — `session.inbox.delivered`
                    // clears each pending row as the turn admits it (no
                    // local re-POST drain).
                    self.interrupt_clear(&session);
                }
            }
            "session.compaction.started" => {
                let Some(session) = event.string("sessionID") else { return };
                let id = event
                    .string("inputID")
                    .or_else(|| event.string("id"))
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| format!("msg_{}", event.id.trim_start_matches("evt_")));
                let created = event.created.unwrap_or(0);
                // `removePending`: a queued compaction that started
                // leaves the inbox (data.ts:1013).
                self.pending_compactions.update(|queues| {
                    if let Some(queue) = queues.get_mut(&session) {
                        queue.retain(|pending| pending != &id);
                    }
                });
                self.update_messages(&session, |messages| {
                    if !messages.iter().any(|m| m.id() == id.as_str()) {
                        messages.push(Message::Compaction {
                            id,
                            created,
                            running: true,
                            failed: false,
                            cancelled: false,
                            error: None,
                            summary: Some(String::new()),
                            tokens_in: None,
                            tokens_out: None,
                            provider_compaction: false,
                        });
                    }
                });
            }
            // The running compaction's streaming summary body
            // (data.ts:1093-1098).
            "session.compaction.delta" => {
                let Some(session) = event.string("sessionID") else { return };
                let text = event.string("text").unwrap_or_default();
                self.update_messages(&session, |messages| {
                    if let Some(Message::Compaction { summary, running: true, .. }) = messages
                        .iter_mut()
                        .rev()
                        .find(|m| matches!(m, Message::Compaction { running: true, .. }))
                    {
                        *summary = Some(format!("{}{}", summary.clone().unwrap_or_default(), text));
                    }
                });
            }
            // Running → completed (or a fresh completed row when the
            // started event was missed — data.ts:1099-1127).
            "session.compaction.ended" => {
                let Some(session) = event.string("sessionID") else { return };
                let summary = event.string("text");
                let tokens = event.data.get("tokens").cloned();
                let provider_compaction =
                    event.data.get("providerContext").is_some_and(|value| !value.is_null());
                self.update_messages(&session, |messages| {
                    let position = messages
                        .iter()
                        .rposition(|m| matches!(m, Message::Compaction { running: true, .. }));
                    let fields = || {
                        let tokens = tokens.as_ref().map(|tokens| {
                            let number = |field: &str| {
                                tokens.pointer(&format!("/{field}")).and_then(Value::as_f64)
                            };
                            (
                                number("input"),
                                number("cache/read"),
                                number("cache/write"),
                                number("output"),
                                number("reasoning"),
                            )
                        });
                        let (input, output) = tokens.map_or((None, None), |usage| {
                            let (input, cr, cw, output, reasoning) = usage;
                            let input =
                                input.unwrap_or(0.0) + cr.unwrap_or(0.0) + cw.unwrap_or(0.0);
                            let output = output.unwrap_or(0.0) + reasoning.unwrap_or(0.0);
                            // Token counts: bounded by real usage.
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let input = Some(input.max(0.0) as u64);
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            let output = Some(output.max(0.0) as u64);
                            (input, output)
                        });
                        (summary.clone().or_else(|| Some(String::new())), input, output)
                    };
                    if let Some(index) = position {
                        if let Some(Message::Compaction {
                            running,
                            failed,
                            summary: slot,
                            tokens_in,
                            tokens_out,
                            provider_compaction: slot_provider,
                            ..
                        }) = messages.get_mut(index)
                        {
                            let (summary, tokens_in_new, tokens_out_new) = fields();
                            *running = false;
                            *failed = false;
                            *slot = summary;
                            *tokens_in = tokens_in_new;
                            *tokens_out = tokens_out_new;
                            *slot_provider = provider_compaction;
                        }
                    } else {
                        let id =
                            event.string("inputID").filter(|id| !id.is_empty()).unwrap_or_else(
                                || format!("msg_{}", event.id.trim_start_matches("evt_")),
                            );
                        let (summary, tokens_in, tokens_out) = fields();
                        messages.push(Message::Compaction {
                            id,
                            created: event.created.unwrap_or(0),
                            running: false,
                            failed: false,
                            cancelled: false,
                            error: None,
                            summary,
                            tokens_in,
                            tokens_out,
                            provider_compaction,
                        });
                    }
                });
            }
            // Running → failed (or a fresh failed row — data.ts:1128+).
            "session.compaction.failed" => {
                let Some(session) = event.string("sessionID") else { return };
                let id = event
                    .string("inputID")
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| format!("msg_{}", event.id.trim_start_matches("evt_")));
                self.pending_compactions.update(|queues| {
                    if let Some(queue) = queues.get_mut(&session) {
                        queue.retain(|pending| pending != &id);
                    }
                });
                let error_type = event
                    .data
                    .pointer("/error/type")
                    .and_then(Value::as_str)
                    .unwrap_or("compaction.failed")
                    .to_owned();
                let error_message = event
                    .data
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Compaction failed before recording an error")
                    .to_owned();
                let cancelled = error_type == "aborted";
                self.update_messages(&session, |messages| {
                    let failed = Message::Compaction {
                        id: id.clone(),
                        created: event.created.unwrap_or(0),
                        running: false,
                        failed: true,
                        cancelled,
                        error: (!cancelled).then(|| error_message.clone()),
                        summary: None,
                        tokens_in: None,
                        tokens_out: None,
                        provider_compaction: false,
                    };
                    let position = messages
                        .iter()
                        .rposition(|m| matches!(m, Message::Compaction { running: true, .. }));
                    if let Some(index) = position {
                        if let Some(Message::Compaction {
                            created,
                            failed: slot_failed,
                            cancelled: slot_cancelled,
                            error: slot_error,
                            ..
                        }) = messages.get_mut(index)
                        {
                            *created = event.created.unwrap_or(0);
                            *slot_failed = true;
                            *slot_cancelled = cancelled;
                            *slot_error = (!cancelled).then(|| error_message.clone());
                        }
                    } else if !messages.iter().any(|m| m.id() == id.as_str()) {
                        messages.push(failed);
                    }
                });
            }
            // The `◇ Compaction queued` divider rows' source
            // (pending.list items of type `compaction`,
            // rows.ts:72-79 — fed by `session.inbox.enqueued`,
            // data.ts:783-788).
            // The revert banner's lifecycle (data.ts:1068-1086):
            // staged carries `{messageID, files}`; cleared/committed
            // drop it (committed also re-filters the message list via
            // the server's next snapshot).
            "session.revert.staged" => {
                let Some(session) = event.string("sessionID") else { return };
                let Some(revert) = event.data.get("revert") else { return };
                let message_id =
                    revert.get("messageID").and_then(Value::as_str).unwrap_or_default().to_owned();
                let files = revert
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|files| {
                        files
                            .iter()
                            .filter_map(|file| {
                                Some(RevertFile {
                                    file: file.get("file")?.as_str()?.to_owned(),
                                    status: file
                                        .get("status")
                                        .and_then(Value::as_str)
                                        .unwrap_or("modified")
                                        .to_owned(),
                                    additions: file
                                        .get("additions")
                                        .and_then(Value::as_i64)
                                        .unwrap_or(0),
                                    deletions: file
                                        .get("deletions")
                                        .and_then(Value::as_i64)
                                        .unwrap_or(0),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                self.reverts.update(|reverts| {
                    reverts.insert(session, RevertState { message_id, files });
                });
            }
            "session.revert.cleared" | "session.revert.committed" => {
                let Some(session) = event.string("sessionID") else { return };
                self.reverts.update(|reverts| {
                    reverts.remove(&session);
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
            tokens: None,
            usage_total: None,
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

/// `metadata` absent/empty check (v2 keeps the previous metadata when
/// the terminal event carries none — `data.ts:953-965` only replaces
/// from `event.data.metadata` which may be undefined).
fn metadata_is_empty(metadata: Option<&Value>) -> bool {
    match metadata {
        None | Some(Value::Null) => true,
        Some(Value::Object(map)) => map.is_empty(),
        Some(_) => false,
    }
}

/// The family walk (`sessionFamily`, util/session.ts:25-45): the
/// root's descendant tree in registry order, each entry carrying its
/// `├─ `/`└─ ` tree prefix (empty at depth 0).
fn picker_family_walk(
    index: &crate::notifications_plugin::SessionIndex,
    children: &std::collections::HashMap<&str, Vec<&str>>,
    parent: &str,
    ancestors: &[bool],
    route: &str,
    out: &mut Vec<PickerEntry>,
) {
    let Some(group) = children.get(parent) else { return };
    for (position, id) in group.iter().enumerate() {
        let last = position + 1 == group.len();
        let prefix = if ancestors.is_empty() {
            String::new()
        } else {
            ancestors[1..]
                .iter()
                .map(|ancestor| if *ancestor { "   " } else { "│  " })
                .collect::<String>()
                + if last { "└─ " } else { "├─ " }
        };
        let info = index.get(id).cloned().unwrap_or_default();
        out.push(PickerEntry {
            session_id: (*id).to_owned(),
            agent: info.agent.as_deref().map_or_else(|| "Subagent".to_owned(), picker_titlecase),
            title: info.title.clone().unwrap_or_else(|| picker_fallback_title(&info)),
            running: index.is_running(id),
            current: *id == route,
            prefix,
        });
        let mut next = ancestors.to_vec();
        next.push(last);
        picker_family_walk(index, children, id, &next, route, out);
    }
}

/// `Locale.titlecase` for the entry's agent label.
fn picker_titlecase(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `withTimestampedFallback` (session-title-fallback.ts:18-21): the
/// `Child session - <ISO>` shape when the registry knows no title.
fn picker_fallback_title(info: &crate::notifications_plugin::SessionInfo) -> String {
    match info.created {
        Some(created) => format!("Child session - {}", iso_timestamp(created)),
        None => "Child session".to_owned(),
    }
}

/// `new Date(ms).toISOString()` — UTC `YYYY-MM-DDTHH:MM:SS.mmmZ`
/// (the session-title fallback's timestamp; days-from-civil inverse,
/// Howard Hinnant's `civil_from_days`).
fn iso_timestamp(epoch_ms: u64) -> String {
    #[allow(clippy::cast_possible_wrap)] // epoch days fit i64 by construction
    let days = (epoch_ms / 86_400_000) as i64;
    let millis_of_day = epoch_ms % 86_400_000;
    let time_of_day = epoch_ms % 1_000;
    let secs_of_day = millis_of_day / 1_000;
    let (hour, minute, second) = (secs_of_day / 3600, (secs_of_day / 60) % 60, secs_of_day % 60);
    // civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{time_of_day:03}Z")
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
            tokens: None,
            usage_total: None,
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
                state: crate::components::message::model::ToolState::default(),
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
// One arm per message kind (the SURF-1 skill arm pushed it over).
#[allow(clippy::too_many_lines)]
#[allow(clippy::cast_possible_truncation)] // wire numerics: integral values survive the f64 round-trip
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
                    "tool" => {
                        let mut state = crate::data::tool_state_from_json(
                            part.pointer("/state/status").and_then(Value::as_str).unwrap_or(""),
                            part.pointer("/state/input"),
                            part.pointer("/state/metadata"),
                        );
                        // The settled states carry `content` (+ `error`
                        // on failed) — the detail cells' output/error
                        // (data.ts:954-985).
                        state.output = part
                            .pointer("/state/content")
                            .and_then(Value::as_array)
                            .map(|blocks| {
                                blocks
                                    .iter()
                                    .filter_map(|block| {
                                        block.get("text").and_then(Value::as_str).map(str::to_owned)
                                    })
                                    .filter(|text| !text.is_empty())
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .filter(|text| !text.is_empty());
                        state.error = part
                            .pointer("/state/error/message")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        parts.push(Part::Tool {
                            id: part.get("id").and_then(Value::as_str)?.to_owned(),
                            name: part
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            time: part_time,
                            state,
                        });
                    }
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
                tokens: None,
                usage_total: None,
            })
        }
        // The `→ Skill {name}` row (message type `skill`,
        // index.tsx:1993-1999).
        Some("skill") => Some(Message::Skill {
            id,
            name: event.string("name").unwrap_or_default(),
            created: time.created.unwrap_or(0),
        }),
        // TUI-TD01 — the six wire classes the drop arm discarded
        // (`SessionMessageView`'s remaining arms, index.tsx:1781-1812).
        Some("shell") => Some(Message::Shell {
            id,
            shell_id: event.string("shellID").unwrap_or_default(),
            command: event.string("command").unwrap_or_default(),
            status: event.string("status").unwrap_or_else(|| "running".to_owned()),
            exit: event.data.get("exit").and_then(Value::as_f64).map(|code| code.round() as i64),
            output: event.data.pointer("/output/output").and_then(Value::as_str).map(str::to_owned),
            created: time.created.unwrap_or(0),
        }),
        Some("agent-switched") => Some(Message::AgentSwitched {
            id,
            agent: event.string("agent").unwrap_or_default(),
            previous: event.string("previous"),
            created: time.created.unwrap_or(0),
        }),
        Some("model-switched") => {
            let model_ref = |value: Option<&Value>| -> Option<(String, String, Option<String>)> {
                let model = value?;
                Some((
                    model.get("providerID").and_then(Value::as_str)?.to_owned(),
                    model.get("id").and_then(Value::as_str)?.to_owned(),
                    model.get("variant").and_then(Value::as_str).map(str::to_owned),
                ))
            };
            Some(Message::ModelSwitched {
                id,
                provider_id: event
                    .data
                    .pointer("/model/providerID")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                model_id: event
                    .data
                    .pointer("/model/id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                variant: event
                    .data
                    .pointer("/model/variant")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                previous: model_ref(event.data.get("previous")),
                created: time.created.unwrap_or(0),
            })
        }
        Some("location-switched") => Some(Message::LocationSwitched {
            id,
            directory: event
                .data
                .pointer("/location/directory")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            created: time.created.unwrap_or(0),
        }),
        Some("system") => Some(Message::System {
            id,
            description: event.string("description"),
            created: time.created.unwrap_or(0),
        }),
        Some("synthetic") => {
            let metadata = event.data.get("metadata");
            let instruction_paths = metadata
                .and_then(|map| map.get("instruction"))
                .and_then(|value| value.get("paths"))
                .and_then(Value::as_array)
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(|path| path.as_str().map(str::to_owned))
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
            let field = |key: &str| {
                metadata.and_then(|map| map.get(key)).and_then(crate::data::stringify_value)
            };
            Some(Message::Synthetic {
                id,
                instruction_paths,
                description: event.string("description"),
                source: field("source"),
                state: field("state"),
                agent: field("agent"),
                child_id: field("childID"),
                created: time.created.unwrap_or(0),
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

    /// Lane/title #12: a rename that lands with NO tab open still
    /// registers (the title task races the submit flow's tab-open),
    /// the session-list history row refreshes in place (v2's renamed
    /// handler writes the session store — data.ts:687-699 — and
    /// `session.list()` re-derives), and `display_title` resolves
    /// registry-first (`session()?.title ?? tab.title`,
    /// session-tabs.tsx:754).
    #[test]
    fn renamed_registers_without_a_tab_and_refreshes_history() {
        let store = store_with_tabs(&["a"]);
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.renamed",
            serde_json::json!({ "sessionID": "ghost", "title": "Parity fake says hello." }),
        ))]);
        // No tab opened — the registry still carries the title.
        assert_eq!(store.session_title("ghost").as_deref(), Some("Parity fake says hello."));
        assert!(
            !store.tabs().tabs.iter().any(|t| t.session_id == "ghost"),
            "the event never admits a tab (routing is the only way in)"
        );
        // The open tab's displayed label resolves registry-first: an
        // untitled tab shadows to the registry title, and the
        // registry shadows a stale persisted tab title.
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.renamed",
            serde_json::json!({ "sessionID": "a", "title": "Registry wins" }),
        ))]);
        let binding = store.tabs();
        let tab = binding.tabs.iter().find(|t| t.session_id == "a").unwrap();
        assert_eq!(store.display_title(tab).as_deref(), Some("Registry wins"));
        // The session-list history row refreshes in place.
        store.ingest_all([Ingest::Resync {
            sessions: vec![SessionSnapshot {
                model: None,
                id: "ghost".to_owned(),
                title: Some("Old wire title".to_owned()),
                parent_id: None,
                time_idle: None,
                viewed: None,
                ..Default::default()
            }],
            unread: Vec::new(),
            messages: Vec::new(),
            cursors: Vec::new(),
            permissions: Vec::new(),
            forms: Vec::new(),
        }]);
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.renamed",
            serde_json::json!({ "sessionID": "ghost", "title": "Parity fake says hello." }),
        ))]);
        let history = store.session_history();
        assert_eq!(
            history.iter().find(|s| s.id == "ghost").and_then(|s| s.title.as_deref()),
            Some("Parity fake says hello."),
            "history row carries the fresh title"
        );
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
                        model: None,
                        id: "a".to_owned(),
                        title: Some("Fresh title".to_owned()),
                        parent_id: None,
                        time_idle: None,
                        viewed: None,
                        ..Default::default()
                    },
                    SessionSnapshot {
                        model: None,
                        id: "not-open".to_owned(),
                        title: Some("History only".to_owned()),
                        parent_id: None,
                        time_idle: None,
                        viewed: None,
                        ..Default::default()
                    },
                ],
                unread: vec!["a".to_owned()],
                messages: Vec::new(),
                cursors: Vec::new(),
                permissions: Vec::new(),
                forms: Vec::new(),
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
        let props =
            crate::components::command_palette::select_props_for(&DialogKind::ThemeList, &store);
        store.update_dialog(|entry| entry.select.set_filter("gru", &props, 24));
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
                "text": "**Title**Body"
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
                tokens: None,
                usage_total: None,
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

    // -- dogfood wave 1 -------------------------------------------------------

    fn catalog() -> Vec<crate::store::ServerModel> {
        let model = |id: &str| crate::store::ServerModel {
            provider_id: "parityfake".into(),
            id: id.into(),
            name: id.into(),
            cost_input: 0.0,
            free: true,
            deprecated: false,
            variants: Vec::new(),
            released: 0.0,
            context_limit: None,
        };
        vec![model("fake-model"), model("picked-model"), model("recorded-model")]
    }

    /// F12 — the session-open transcript fetch MERGES over
    /// live-projected rows (a prompt admitted between the fetch and
    /// its arrival survives; v2 data.ts:1640-1647's
    /// `[...fetched, ...local]`).
    #[test]
    fn messages_snapshot_merge_keeps_local_rows() {
        use crate::components::message::model::Message;
        use crate::data::Ingest;
        let store = Store::new(Runtime::new());
        // The live stream admitted a user row before the fetch landed.
        store.ingest_all([Ingest::Event(crate::data::BusEvent::new(
            "session.inbox.enqueued",
            serde_json::json!({
                "sessionID": "s1",
                "inboxID": "msg_live",
                "item": { "type": "user", "payload": { "text": "just admitted" } }
            }),
        ))]);
        // The fetch returns the STORED page (without the live row).
        store.ingest_all([Ingest::MessagesSnapshot {
            session_id: "s1".to_owned(),
            messages: vec![
                Message::User {
                    id: "msg_u1".into(),
                    text: "stored ask".into(),
                    created: 0,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: "msg_a1".into(),
                    agent: "build".into(),
                    model: "p/m".into(),
                    parts: Vec::new(),
                    finish: None,
                    error: None,
                    time: crate::components::message::model::PartTime::default(),
                    tokens: None,
                    usage_total: None,
                },
            ],
        }]);
        let messages = store.messages("s1");
        let ids: Vec<&str> =
            messages.iter().map(crate::components::message::model::Message::id).collect();
        assert_eq!(
            ids,
            vec!["msg_u1", "msg_a1", "msg_live"],
            "fetched rows in page order, the local-only row appended (never wiped)"
        );
        // A re-delivery of the same snapshot is idempotent.
        store.ingest_all([Ingest::MessagesSnapshot {
            session_id: "s1".to_owned(),
            messages: messages.clone(),
        }]);
        assert_eq!(store.messages("s1").len(), 3);
    }

    /// F8/F10/F22 — the selection chain: explicit pick > durable
    /// (session record) > config default > recent head > catalog head;
    /// nothing resolves while the catalog has not loaded.
    #[test]
    fn model_chain_resolution_steps() {
        let store = Store::new(Runtime::new());
        assert!(
            store.resolve_model("s1").is_none(),
            "nothing resolves before the catalog loads (isModelValid on an empty models())"
        );

        store.set_models(crate::store::ListState::Loaded(catalog()), Vec::new());

        // Step 5: the catalog head.
        assert_eq!(
            store.resolve_model("s1").map(|model| model.model_id),
            Some("fake-model".to_owned()),
            "pristine session resolves models[0]"
        );
        assert_eq!(
            store.resolve_model(crate::app::HOME_PROMPT_KEY).map(|model| model.model_id),
            Some("fake-model".to_owned()),
            "pristine home resolves models[0] (the first prompt gets an actionable provider error)"
        );

        // Step 4: the ring's recent head.
        store.model_add_recent("parityfake", "picked-model");
        assert_eq!(
            store.resolve_model("s1").map(|model| model.model_id),
            Some("picked-model".to_owned())
        );

        // Step 3: the config default beats recent.
        store.set_config_model(Some(crate::store::ModelSelection {
            provider_id: "parityfake".into(),
            model_id: "recorded-model".into(),
            variant: None,
        }));
        assert_eq!(
            store.resolve_model("s1").map(|model| model.model_id),
            Some("recorded-model".to_owned())
        );
        // …but an INVALID config model (not in the catalog) falls through.
        store.set_config_model(Some(crate::store::ModelSelection {
            provider_id: "gone".into(),
            model_id: "ghost".into(),
            variant: None,
        }));
        assert_eq!(
            store.resolve_model("s1").map(|model| model.model_id),
            Some("picked-model".to_owned()),
            "an unknown config model is skipped (isModelValid)"
        );
        store.set_config_model(None);

        // Step 2: the durable session record beats recent.
        store.ingest_all([Ingest::Resync {
            sessions: vec![crate::data::SessionSnapshot {
                id: "s1".into(),
                title: None,
                parent_id: None,
                time_idle: None,
                viewed: None,
                model: Some(crate::data::SessionModelRef {
                    provider_id: "parityfake".into(),
                    id: "recorded-model".into(),
                    variant: None,
                }),
                ..Default::default()
            }],
            unread: Vec::new(),
            messages: Vec::new(),
            cursors: Vec::new(),
            permissions: Vec::new(),
            forms: Vec::new(),
        }]);
        assert_eq!(
            store.resolve_model("s1").map(|model| model.model_id),
            Some("recorded-model".to_owned()),
            "restart restores the per-session durable model"
        );

        // Step 1: the explicit pick beats everything.
        store.select_model(
            "s1",
            &crate::store::ModelSelection {
                provider_id: "parityfake".into(),
                model_id: "picked-model".into(),
                variant: None,
            },
        );
        assert_eq!(
            store.resolve_model("s1").map(|model| model.model_id),
            Some("picked-model".to_owned())
        );

        // The DISPLAY surface uses the resolved chain too.
        assert_eq!(
            store.effective_model("s2").map(|model| model.model_id),
            Some("picked-model".to_owned()),
            "a session with no pick displays the resolved model"
        );
    }

    /// F8 — the submit path carries the RESOLVED model into the sink
    /// (the v2 commitModel seam).
    #[test]
    fn submissions_carry_the_resolved_model() {
        use crate::components::composer::{PromptSink, SubmittedPrompt};
        use std::cell::RefCell;

        #[derive(Default)]
        struct ResolvedSink {
            seen: RefCell<Vec<(String, Option<String>)>>,
        }
        impl PromptSink for ResolvedSink {
            fn send(&self, _session_id: &str, _prompt: &SubmittedPrompt) {
                panic!("send_with_model must be the store's call");
            }
            fn send_with_model(
                &self,
                session_id: &str,
                _prompt: &SubmittedPrompt,
                model: Option<&crate::store::ModelSelection>,
            ) {
                self.seen
                    .borrow_mut()
                    .push((session_id.to_owned(), model.map(|model| model.model_id.clone())));
            }
        }

        let sink = std::rc::Rc::new(ResolvedSink::default());
        let mut store = Store::new(Runtime::new());
        store.set_prompt_sink(std::rc::Rc::clone(&sink) as std::rc::Rc<dyn PromptSink>);
        store.set_models(crate::store::ListState::Loaded(catalog()), Vec::new());

        let mut composer = store.composer("s1");
        composer.draft.text = "hi".into();
        store.set_composer("s1", &composer);
        let _ = store.submit_prompt("s1");
        assert_eq!(
            sink.seen.borrow().as_slice(),
            &[("s1".to_owned(), Some("fake-model".to_owned()))],
            "the submit committed the catalog-head model"
        );
    }

    // -- the session sidebar (session-frame.tsx) --------------------------

    /// v2's `sidebarVisible()` (session-frame.tsx:107-111): auto shows
    /// only above 120; the manual open forces it on at ANY width; hide
    /// never shows; a subagent session (parentID) never does.
    #[test]
    fn sidebar_visibility_matches_the_reference() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        assert!(!store.sidebar_shown("s1", 80), "auto below the breakpoint");
        assert!(!store.sidebar_shown("s1", 120), "strictly greater than 120");
        assert!(store.sidebar_shown("s1", 121), "auto above the breakpoint");
        store.set_sidebar_mode(crate::components::sidebar::SidebarMode::Hide);
        assert!(!store.sidebar_shown("s1", 160), "hide never shows");
        store.set_sidebar_mode(crate::components::sidebar::SidebarMode::Auto);
        // The manual toggle flips mode + open together; force open via
        // the toggle at a narrow width.
        store.set_terminal_size((80, 24));
        store.toggle_sidebar();
        assert!(store.sidebar_shown("s1", 80), "the manual open forces it on");
        // A subagent session never shows it (session-frame.tsx:108).
        store.ingest_all([crate::data::Ingest::Event(BusEvent::new(
            "session.created",
            serde_json::json!({ "sessionID": "sub", "parentID": "s1" }),
        ))]);
        assert!(!store.sidebar_shown("sub", 160), "subagent sessions suppress it");
        assert!(store.sidebar_shown("s1", 160), "the parent keeps it");
    }

    /// The toggle's write pair (session-frame.tsx:118-130): showing →
    /// mode `hide` + open false; hidden → mode `auto` + open true —
    /// and the mode is what persists (sidebarOpen is per-mount).
    #[test]
    fn toggle_writes_the_mode_and_manual_flag() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.set_terminal_size((160, 24));
        // Showing (auto + wide) → hide.
        store.toggle_sidebar();
        assert_eq!(store.sidebar_mode(), crate::components::sidebar::SidebarMode::Hide);
        assert!(!store.sidebar_shown("s1", 160));
        // Hidden → auto + the manual open (shows even narrow).
        store.toggle_sidebar();
        assert_eq!(store.sidebar_mode(), crate::components::sidebar::SidebarMode::Auto);
        assert!(store.sidebar_shown("s1", 80));
        // Off the session route the command is a no-op (the layer
        // lives in the session frame).
        store.navigate(Route::Home);
        store.toggle_sidebar();
        assert_eq!(store.sidebar_mode(), crate::components::sidebar::SidebarMode::Auto);
    }

    // -- fix-wave E2: the subagent surfaces (#40/#44/#45/#46) -------------

    /// The live tool-state reducers (v2 client data.ts:919-980): the
    /// part moves streaming → running (parsed input) → completed
    /// (metadata from the progress snapshot) off the `session.tool.*`
    /// events.
    #[test]
    fn tool_events_drive_the_part_state_machine() {
        let store = Store::new(Runtime::new());
        store.ingest_all([
            Ingest::Event(BusEvent::new(
                "session.step.started",
                serde_json::json!({
                    "sessionID": "s1", "assistantMessageID": "a1",
                    "agent": "build", "model": {"providerID": "p", "id": "m"}
                }),
            )),
            Ingest::Event(BusEvent::new(
                "session.tool.input.started",
                serde_json::json!({ "sessionID": "s1", "assistantMessageID": "a1", "id": "t1", "name": "subagent" }),
            )),
        ]);
        let part = |store: &Store| {
            let messages = store.messages("s1");
            let Some(Message::Assistant { parts, .. }) =
                messages.iter().find(|message| message.id() == "a1")
            else {
                panic!("assistant missing");
            };
            let Some(Part::Tool { state, .. }) =
                parts.iter().find(|p| matches!(p, Part::Tool { .. }))
            else {
                panic!("tool part missing");
            };
            state.clone()
        };
        assert_eq!(part(&store).status, ToolStatus::Streaming);
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.tool.called",
            serde_json::json!({
                "sessionID": "s1", "assistantMessageID": "a1", "id": "t1",
                "input": {"agent": "general", "description": "Check the fixture", "prompt": "do it"},
                "executed": false
            }),
        ))]);
        let state = part(&store);
        assert_eq!(state.status, ToolStatus::Running);
        assert_eq!(state.input.get("description").map(String::as_str), Some("Check the fixture"));
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.tool.progress",
            serde_json::json!({
                "sessionID": "s1", "assistantMessageID": "a1", "id": "t1",
                "metadata": {"sessionID": "ses_child", "status": "running"}
            }),
        ))]);
        assert_eq!(part(&store).metadata.get("sessionID").map(String::as_str), Some("ses_child"));
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.tool.success",
            serde_json::json!({
                "sessionID": "s1", "assistantMessageID": "a1", "id": "t1",
                "content": [{"type": "text", "text": "done"}],
                "metadata": {"sessionID": "ses_child"},
                "executed": false
            }),
        ))]);
        let state = part(&store);
        assert_eq!(state.status, ToolStatus::Completed);
        assert_eq!(
            state.metadata.get("sessionID").map(String::as_str),
            Some("ses_child"),
            "the success snapshot keeps the progress metadata"
        );
    }

    /// The registry's family/index state off the lifecycle events
    /// (#44/#45): the child session lands with its agent + title, the
    /// execution events drive the running filter, and `root_of`
    /// resolves the parent.
    #[test]
    fn session_registry_tracks_family_agent_and_status() {
        let store = Store::new(Runtime::new());
        store.ingest_all([
            Ingest::Event(BusEvent::new(
                "session.created",
                serde_json::json!({ "sessionID": "s1", "title": "Root flow" }),
            )),
            Ingest::Event(BusEvent::new(
                "session.created",
                serde_json::json!({
                    "sessionID": "ses_child", "parentID": "s1",
                    "agent": "general", "title": "Check the fixture"
                }),
            )),
        ]);
        assert!(store.session_is_subagent("ses_child"));
        assert!(!store.session_is_subagent("s1"));
        assert_eq!(store.session_root("ses_child"), "s1");
        assert_eq!(store.session_root("s1"), "s1");
        // Running set: started inserts, the terminal events remove.
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.execution.started",
            serde_json::json!({ "sessionID": "ses_child" }),
        ))]);
        assert!(store.session_info("ses_child").is_some());
        assert!(store.subagent_running_map().get("ses_child").copied().unwrap_or(false));
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.execution.succeeded",
            serde_json::json!({ "sessionID": "ses_child" }),
        ))]);
        assert!(!store.subagent_running_map().contains_key("ses_child"));
    }

    /// #44 — the picker walk: `session.child.first` opens the
    /// subagents tab, the filter flips with `toggle-activity`, the
    /// entry list follows, `select` navigates into the child WITHOUT
    /// opening a tab (the route roots the family), and the child route
    /// always mounts the panel — `close` navigates back to the parent.
    #[test]
    fn picker_opens_filters_navigates_and_closes_to_the_parent() {
        use crate::components::command_palette::CommandDispatcher as _;
        let mut dispatcher = crate::components::command_palette::StoreDispatcher;
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Root flow".to_owned()));
        store.navigate(Route::Session { session_id: "s1".to_owned() });
        store.ingest_all([
            Ingest::Event(BusEvent::new(
                "session.created",
                serde_json::json!({
                    "sessionID": "ses_child", "parentID": "s1",
                    "agent": "general", "title": "Check the fixture"
                }),
            )),
            Ingest::Event(BusEvent::new(
                "session.execution.succeeded",
                serde_json::json!({ "sessionID": "ses_child" }),
            )),
        ]);

        // Off the session route the toggle is inert.
        store.navigate(Route::Home);
        dispatcher.dispatch(&store, "session.child.first", None);
        assert!(!store.picker().open);
        store.navigate(Route::Session { session_id: "s1".to_owned() });

        // Open: the subagents tab, active filter (nothing running).
        dispatcher.dispatch(&store, "session.child.first", None);
        assert!(store.picker().open);
        assert_eq!(store.picker().tab, PickerTab::Subagents);
        assert_eq!(store.picker_entries("s1"), Vec::new(), "nothing running yet");
        assert!(store.picker_live("s1"));

        // toggle-activity: the completed run appears.
        dispatcher.dispatch(&store, "composer.subagent.toggle-activity", None);
        let entries = store.picker_entries("s1");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].session_id, "ses_child");
        assert_eq!(entries[0].agent, "General");
        assert_eq!(entries[0].title, "Check the fixture");
        assert!(!entries[0].running);
        assert!(!entries[0].current);

        // select: navigate into the child — one tab stays, the route
        // is the child, and the panel stays live (the parentID mount).
        dispatcher.dispatch(&store, "composer.subagent.select", None);
        assert!(
            matches!(store.route(), Route::Session { ref session_id } if session_id == "ses_child")
        );
        assert_eq!(store.tabs().tabs.len(), 1, "no tab opened for the child");
        assert!(store.picker_live("ses_child"), "the child route keeps the panel");
        let child_entries = store.picker_entries("ses_child");
        assert!(child_entries.iter().any(|entry| entry.current), "the child is `current`");

        // `session.child.first` on the child does not unmount it.
        dispatcher.dispatch(&store, "session.child.first", None);
        assert!(store.picker_live("ses_child"));

        // close: back to the parent, the explicit open flag folds.
        dispatcher.dispatch(&store, "composer.close", None);
        assert!(matches!(store.route(), Route::Session { ref session_id } if session_id == "s1"));
        assert!(!store.picker().open);
    }

    /// The picker's navigation keys (`composer.subagent.up/down`) wrap
    /// and `up` at the top closes (subagents-tab.tsx:140-151).
    #[test]
    fn picker_move_wraps_and_up_at_top_closes() {
        let store = Store::new(Runtime::new());
        store.open_tab("s1", Some("Root".to_owned()));
        store.navigate(Route::Session { session_id: "s1".to_owned() });
        store.ingest_all([
            Ingest::Event(BusEvent::new(
                "session.created",
                serde_json::json!({ "sessionID": "s1", "title": "Root" }),
            )),
            Ingest::Event(BusEvent::new(
                "session.created",
                serde_json::json!({
                    "sessionID": "c1", "parentID": "s1",
                    "agent": "general", "title": "First"
                }),
            )),
            Ingest::Event(BusEvent::new(
                "session.created",
                serde_json::json!({
                    "sessionID": "c2", "parentID": "s1",
                    "agent": "general", "title": "Second"
                }),
            )),
        ]);
        let mut picker = store.picker();
        picker.open = true;
        picker.filter_active = false;
        store.set_picker(picker);
        assert_eq!(store.picker_entries("s1").len(), 2);
        assert_eq!(store.picker_selection("s1", 2), 0);
        store.picker_move(true);
        assert_eq!(store.picker().selected, 1);
        store.picker_move(true);
        assert_eq!(store.picker().selected, 0, "down wraps");
        store.picker_move(false);
        assert!(!store.picker().open, "up at the top closes the composer");
    }

    /// #46 — the exit epilogue tracks the session route (title from
    /// the registry, truncated to 50) and is absent from home.
    #[test]
    fn exit_epilogue_tracks_the_session_route() {
        let store = Store::new(Runtime::new());
        assert!(store.exit_epilogue().is_none(), "home has no epilogue");
        store.open_tab("s1", Some("Root".to_owned()));
        store.navigate(Route::Session { session_id: "s1".to_owned() });
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.created",
            serde_json::json!({ "sessionID": "s1", "title": "Parity Subagent Flow" }),
        ))]);
        let epilogue = store.exit_epilogue().expect("the session stages an epilogue");
        assert!(
            epilogue
                .contains("\u{1b}[90mSession   \u{1b}[0m\u{1b}[1mParity Subagent Flow\u{1b}[0m"),
            "{epilogue:?}"
        );
        assert!(
            epilogue.contains("\u{1b}[90mContinue  \u{1b}[0m\u{1b}[1mopencode -s s1\u{1b}[0m"),
            "{epilogue:?}"
        );
        // Long titles truncate to 50 (Locale.truncate).
        store.ingest_all([Ingest::Event(BusEvent::new(
            "session.renamed",
            serde_json::json!({ "sessionID": "s1", "title": "x".repeat(80) }),
        ))]);
        let epilogue = store.exit_epilogue().expect("still staged");
        assert!(
            epilogue.contains(&format!(
                "\u{1b}[90mSession   \u{1b}[0m\u{1b}[1m{}…\u{1b}[0m",
                "x".repeat(49)
            )),
            "{epilogue:?}"
        );
        store.navigate(Route::Home);
        assert!(store.exit_epilogue().is_none(), "leaving the session clears it");
    }

    /// The `<leader>b` chord dispatches through the palette arm.
    #[test]
    fn dispatcher_toggle_sidebar_arm_runs_the_store_toggle() {
        use crate::components::command_palette::CommandDispatcher as _;
        let store = Store::new(Runtime::new());
        store.open_tab("s1", None);
        store.set_terminal_size((160, 24));
        let mut dispatcher = crate::components::command_palette::StoreDispatcher;
        assert_eq!(
            dispatcher.dispatch(&store, "session.sidebar.toggle", None),
            crate::components::command_palette::Dispatch::Handled
        );
        assert_eq!(store.sidebar_mode(), crate::components::sidebar::SidebarMode::Hide);
    }
}

// -- lane M-theme: the theme dialog's live-preview apply path ----------------

/// The captured initial, the centered open window, the
/// move/filter previews, and the unconfirmed-close revert — the
/// `dialog-theme-list.tsx` contract end to end at the store level.
#[test]
fn theme_dialog_live_preview_applies_and_reverts() {
    let store = Store::new(Runtime::new());
    store.set_terminal_size((80, 24));
    assert_eq!(store.theme().name, "opencode");

    store.open_dialog(DialogKind::ThemeList, DialogSize::Medium);
    let entry = store.top_dialog().expect("dialog open");
    assert_eq!(entry.theme_initial.as_deref(), Some("opencode"), "initial captured");

    // The open window centers the current theme (scrollAfterLayout
    // true): 34 sorted entries, opencode at flat index 22, viewport
    // min(34, 24/2-6) = 6 → scroll = 22 - 3 = 19 (nightowl first).
    let props = crate::components::command_palette::theme_list_props(&store);
    assert!(props.options.iter().all(|option| option.category.is_none()), "flat list");
    assert_eq!(props.options.len(), 34, "33 + the system slot");
    assert_eq!(entry.select.selected, 22, "selection = the active theme");
    assert_eq!(entry.select.scroll, 19, "centered on the active theme");

    // A selection move previews (`onMove`): opencode → orng.
    store.update_dialog(|top| top.select.move_selection(&props, 1));
    store.preview_theme_move();
    assert_eq!(store.theme().name, "orng", "onMove previews live");

    // A filter previews the FIRST match (`onFilter`): "nord" → nord.
    store.update_dialog(|top| top.select.set_filter("nord", &props, 24));
    store.preview_theme_filter();
    assert_eq!(store.theme().name, "nord", "onFilter previews the first match");

    // Clearing the filter resets to the captured initial.
    store.update_dialog(|top| top.select.set_filter("", &props, 24));
    store.preview_theme_filter();
    assert_eq!(store.theme().name, "opencode", "empty filter resets to initial");

    // An unconfirmed close REVERTS (`onCleanup`).
    store.update_dialog(|top| top.select.set_filter("matrix", &props, 24));
    store.preview_theme_filter();
    assert_eq!(store.theme().name, "matrix");
    store.close_dialog();
    assert_eq!(store.theme().name, "opencode", "unconfirmed close reverts");
    assert!(store.top_dialog().is_none());
}

/// A submit CONFIRMS: the preview becomes permanent and no later
/// close reverts (`confirmed = true` before `dialog.clear()`).
#[test]
fn theme_dialog_submit_confirms_and_keeps_the_theme() {
    let store = Store::new(Runtime::new());
    store.set_terminal_size((80, 24));
    store.open_dialog(DialogKind::ThemeList, DialogSize::Medium);
    let props = crate::components::command_palette::theme_list_props(&store);

    // Filter "nord" (preview), then submit it.
    store.update_dialog(|top| top.select.set_filter("nord", &props, 24));
    store.preview_theme_filter();
    assert_eq!(store.theme().name, "nord");
    let dispatch = crate::components::dialog::execute_submission(
        &store,
        &mut crate::components::command_palette::StoreDispatcher,
        "nord",
    );
    assert_eq!(dispatch, crate::components::command_palette::Dispatch::Handled);
    assert_eq!(store.theme().name, "nord", "submit applies");
    assert!(store.top_dialog().is_none(), "submit clears the dialog stack");
    // Nothing left to revert: later dialog churn must not rewind.
    store.open_dialog(DialogKind::CommandPalette, DialogSize::Medium);
    store.close_dialog();
    assert_eq!(store.theme().name, "nord", "confirmed theme survives closes");
}

/// The applied palette flows to the painters: every styles-derived
/// color the surfaces read changes with the theme signal (the
/// full-chain smoke — dialog apply → store theme → `ThemeStyles`).
#[test]
fn theme_apply_signal_reaches_every_surface_style() {
    let store = Store::new(Runtime::new());
    let before = store.theme().styles();
    store.set_theme_by_name("nord");
    let after = store.theme().styles();
    assert_ne!(before.background, after.background, "page fill restyles");
    assert_ne!(before.background_raised, after.background_raised, "dialog chrome restyles");
    assert_ne!(before.base, after.base, "whole-frame fill restyles");
    assert_ne!(before.text, after.text, "text restyles");
    // The frame renderers consume the store's theme each redraw
    // (render_app(store, theme, size)) — assert the seam directly.
    let frame = crate::components::dialog::render_app(&store, &store.theme(), (80, 24));
    let bg = after.background;
    let paints_bg = frame.cells.iter().flat_map(|row| row.iter()).any(|cell| cell.bg == bg.into());
    assert!(paints_bg, "the rendered frame carries the new palette");
}

// -- lane TAIL: the update-notification state machine (auditB #10) --

#[cfg(test)]
fn notice(
    kind: crate::components::update::UpdateNoticeKind,
    version: &str,
    remote: bool,
) -> crate::components::update::UpdateNotice {
    crate::components::update::UpdateNotice { kind, version: version.to_owned(), remote }
}

/// The banner lifecycle: notify → show; dismiss → gone + the key
/// recorded (a re-notify of the same version stays suppressed; a
/// NEW version shows again).
#[test]
fn update_notice_lifecycle_and_dismissal_history() {
    let store = Store::new(Runtime::new());
    assert!(store.update_notice().is_none());
    store.update_notify(notice(
        crate::components::update::UpdateNoticeKind::Available,
        "2.0.25",
        false,
    ));
    assert_eq!(store.update_notice().map(|n| n.version), Some("2.0.25".into()));
    store.update_dismiss();
    assert!(store.update_notice().is_none(), "dismissed");
    assert_eq!(store.update_dismissed(), vec!["client:2.0.25".to_owned()]);
    // The same version stays suppressed…
    store.update_notify(notice(
        crate::components::update::UpdateNoticeKind::Available,
        "2.0.25",
        false,
    ));
    assert!(store.update_notice().is_none(), "the dismissed version is suppressed");
    // …a NEW version shows.
    store.update_notify(notice(
        crate::components::update::UpdateNoticeKind::Available,
        "2.0.26",
        false,
    ));
    assert_eq!(store.update_notice().map(|n| n.version), Some("2.0.26".into()));
    // A CLIENT notice never displaces a live SERVER one.
    store.update_notify(notice(
        crate::components::update::UpdateNoticeKind::Installed,
        "9.9.9",
        true,
    ));
    assert_eq!(store.update_notice().map(|n| n.version), Some("9.9.9".into()));
    store.update_notify(notice(
        crate::components::update::UpdateNoticeKind::Available,
        "1.0.0",
        false,
    ));
    assert_eq!(
        store.update_notice().map(|n| n.version),
        Some("9.9.9".into()),
        "client never displaces server"
    );
}

/// The SSE installation events land the banner.
#[test]
fn installation_events_notify_the_banner() {
    let store = Store::new(Runtime::new());
    store.ingest_all([Ingest::Event(BusEvent::new(
        "installation.update-available",
        serde_json::json!({ "version": "2.0.25" }),
    ))]);
    assert_eq!(store.update_notice().map(|n| n.version), Some("2.0.25".into()));
}

/// `/update` opens the dialog and the actions route through the
/// submission seam (skip dismisses; confirm marks Installing).
#[test]
fn update_command_opens_the_dialog_with_actions() {
    use crate::components::command_palette::CommandDispatcher as _;
    let store = Store::new(Runtime::new());
    let mut dispatcher = crate::components::command_palette::StoreDispatcher;
    assert_eq!(
        dispatcher.dispatch(&store, "update.check", None),
        crate::components::command_palette::Dispatch::Handled
    );
    assert!(matches!(store.top_dialog().map(|entry| entry.kind), Some(DialogKind::Update)));
    // An available notice + confirm → Installing (the ServerPending
    // marker rides the submission).
    store.update_notify(notice(
        crate::components::update::UpdateNoticeKind::Available,
        "2.0.25",
        false,
    ));
    store.set_update_dialog(crate::components::update::UpdateDialogState::Available(
        "2.0.25".to_owned(),
    ));
    let dispatch =
        crate::components::dialog::execute_submission(&store, &mut dispatcher, "confirm");
    assert!(matches!(
        dispatch,
        crate::components::command_palette::Dispatch::ServerPending("update.install")
    ));
    assert!(matches!(
        store.update_dialog_state(),
        crate::components::update::UpdateDialogState::Installing(_)
    ));
    // Skip dismisses the banner too.
    store.clear_dialogs();
    store.open_dialog(DialogKind::Update, DialogSize::Medium);
    let _ = crate::components::dialog::execute_submission(&store, &mut dispatcher, "skip");
    assert!(store.update_notice().is_none(), "skip dismissed the banner");
}

// -- TUI-TD01: the six wire classes project from the SSE path ----

#[test]
fn six_wire_classes_project_from_events() {
    let store = Store::new(Runtime::new());
    store.open_tab("s1", None);
    let events = [
        BusEvent::new(
            "message.updated",
            serde_json::json!({
                "id": "m_shell", "sessionID": "s1", "type": "shell", "shellID": "sh",
                "command": "ls", "status": "exited", "exit": 2,
                "output": {"cursor": 1, "output": "nope", "size": 1, "truncated": false},
                "time": {"created": 1}
            }),
        ),
        BusEvent::new(
            "message.updated",
            serde_json::json!({
                "id": "m_agent", "sessionID": "s1", "type": "agent-switched",
                "agent": "plan", "previous": "build", "time": {"created": 2}
            }),
        ),
        BusEvent::new(
            "message.updated",
            serde_json::json!({
                "id": "m_model", "sessionID": "s1", "type": "model-switched",
                "model": {"providerID": "p", "id": "m"},
                "previous": {"providerID": "q", "id": "n"},
                "time": {"created": 3}
            }),
        ),
        BusEvent::new(
            "message.updated",
            serde_json::json!({
                "id": "m_loc", "sessionID": "s1", "type": "location-switched",
                "location": {"directory": "/elsewhere"}, "time": {"created": 4}
            }),
        ),
        BusEvent::new(
            "message.updated",
            serde_json::json!({
                "id": "m_sys", "sessionID": "s1", "type": "system", "text": "",
                "description": "notice", "time": {"created": 5}
            }),
        ),
        BusEvent::new(
            "message.updated",
            serde_json::json!({
                "id": "m_syn", "sessionID": "s1", "type": "synthetic", "text": "done",
                "description": "done",
                "metadata": {"source": "subagent", "state": "completed", "agent": "helper"},
                "time": {"created": 6}
            }),
        ),
    ];
    for event in events {
        store.ingest_all([Ingest::Event(event)]);
    }
    let messages = store.messages("s1");
    assert!(matches!(
        messages.iter().find(|m| m.id() == "m_shell"),
        Some(Message::Shell { command, status, exit, .. })
            if command == "ls" && status == "exited" && *exit == Some(2)
    ));
    assert!(matches!(
        messages.iter().find(|m| m.id() == "m_agent"),
        Some(Message::AgentSwitched { agent, previous, .. })
            if agent == "plan" && previous.as_deref() == Some("build")
    ));
    assert!(matches!(
        messages.iter().find(|m| m.id() == "m_model"),
        Some(Message::ModelSwitched { provider_id, model_id, .. })
            if provider_id == "p" && model_id == "m"
    ));
    assert!(matches!(
        messages.iter().find(|m| m.id() == "m_loc"),
        Some(Message::LocationSwitched { directory, .. }) if directory == "/elsewhere"
    ));
    assert!(matches!(
        messages.iter().find(|m| m.id() == "m_sys"),
        Some(Message::System { description, .. })
            if description.as_deref() == Some("notice")
    ));
    assert!(matches!(
        messages.iter().find(|m| m.id() == "m_syn"),
        Some(Message::Synthetic { source, state, agent, .. })
            if source.as_deref() == Some("subagent")
                && state.as_deref() == Some("completed")
                && agent.as_deref() == Some("helper")
    ));
}

// -- TUI-TD07: the inbox mirror's event lifecycle ------------------

#[cfg(test)]
fn queue_two(store: &Store) {
    store.queue_prompt("s1", "first", &[]);
    store.queue_prompt("s1", "second", &[]);
}

#[test]
fn inbox_mirror_tracks_delivery_changes_and_cancels() {
    let store = Store::new(Runtime::new());
    store.open_tab("s1", None);
    let sink = std::rc::Rc::new(RecordingInboxSink::default());
    store.set_inbox_sink(sink.clone());
    queue_two(&store);
    assert_eq!(store.queued_prompts("s1").len(), 2, "both queued");
    // The enqueued echo upserts (the same id admits once).
    store.ingest_all([Ingest::Event(BusEvent::new(
        "session.inbox.enqueued",
        serde_json::json!({
            "sessionID": "s1", "inboxID": store.queued_prompts("s1")[0].id,
            "item": {"type": "user", "delivery": "queue", "payload": {"text": "first"}}
        }),
    ))]);
    assert_eq!(store.queued_prompts("s1").len(), 2, "the echo upserted, not appended");
    // Steer flips the delivery server-side (PATCH).
    let head = store.queued_prompts("s1")[0].id.clone();
    assert!(store.steer_queued_prompt("s1", &head));
    assert_eq!(store.queued_prompts("s1").len(), 1, "steered out of the queue");
    assert_eq!(store.pending_delivery("s1", &head).as_deref(), Some("steer"));
    // delivery.changed refines (the PATCH's echo).
    store.ingest_all([Ingest::Event(BusEvent::new(
        "session.inbox.delivery.changed",
        serde_json::json!({"sessionID": "s1", "inboxID": head, "delivery": "steer"}),
    ))]);
    assert_eq!(store.pending_delivery("s1", &head).as_deref(), Some("steer"));
    // delivered clears the pending row.
    store.ingest_all([Ingest::Event(BusEvent::new(
        "session.inbox.delivered",
        serde_json::json!({"sessionID": "s1", "inboxID": head}),
    ))]);
    assert_eq!(store.pending_inbox("s1").len(), 1);
    // cancelled retracts the pending row AND the transcript row.
    let second = store.queued_prompts("s1")[0].id.clone();
    store.ingest_all([Ingest::Event(BusEvent::new(
        "session.inbox.cancelled",
        serde_json::json!({"sessionID": "s1", "inboxID": second}),
    ))]);
    assert_eq!(store.pending_inbox("s1"), Vec::<InboxItem>::new());
    assert!(
        sink.calls.borrow().iter().any(|call| call.contains("update") && call.contains("steer")),
        "the steer PATCHed: {:?}",
        sink.calls.borrow()
    );
}

#[test]
fn undo_returns_the_payload_and_cancels() {
    let store = Store::new(Runtime::new());
    store.open_tab("s1", None);
    let sink = std::rc::Rc::new(RecordingInboxSink::default());
    store.set_inbox_sink(sink.clone());
    store.queue_prompt("s1", "refill me", &["data:image/png;base64,AAA".to_owned()]);
    let payload = store.undo_queued_prompt("s1").expect("undo found the last row");
    assert_eq!(payload.text, "refill me");
    assert_eq!(payload.files, vec!["data:image/png;base64,AAA".to_owned()]);
    assert_eq!(store.queued_prompts("s1"), Vec::<QueuedPrompt>::new());
    assert!(
        sink.calls.borrow().iter().any(|call| call.starts_with("cancel")),
        "the cancel DELETEd: {:?}",
        sink.calls.borrow()
    );
    assert!(store.undo_queued_prompt("s1").is_none(), "an empty queue misses");
}

// -- TUI-TD02/TD05: the navigation + mount-window walk ------------

#[test]
fn navigation_pins_the_boundary_target() {
    let store = Store::new(Runtime::new());
    store.open_tab("s1", None);
    store.set_messages(
        "s1",
        vec![
            Message::User {
                id: "q1".into(),
                text: "ask one".into(),
                created: 1,
                files: Vec::new(),
            },
            Message::Assistant {
                id: "a1".into(),
                agent: "build".into(),
                model: "p/m".into(),
                parts: vec![Part::Text { text: "answer one".into(), time: PartTime::default() }],
                finish: Some("stop".into()),
                error: None,
                time: PartTime::default(),
                tokens: None,
                usage_total: None,
            },
            Message::User {
                id: "q2".into(),
                text: "ask two".into(),
                created: 2,
                files: Vec::new(),
            },
        ],
    );
    // The render pass's geometry: q1@10, a1@40, q2@80 (content y).
    store.set_message_positions(
        "s1",
        vec![
            MessagePosition { id: "q1".into(), y: 10 },
            MessagePosition { id: "a1".into(), y: 40 },
            MessagePosition { id: "q2".into(), y: 80 },
        ],
    );
    store.set_transcript_metrics(
        "s1",
        TranscriptMetrics { viewport: 10, content_height: 100, rows: 6 },
    );
    // From the sticky bottom (offset 90): previous targets the
    // LAST candidate above — q2 — and pins Free at its y.
    store.navigate_message("s1", NavDirection::Previous, false);
    assert!(store.resolve_pending_navigation("s1"), "the intent resolved");
    assert_eq!(store.navigation_message("s1").as_deref(), Some("q2"));
    assert_eq!(store.scroll_offset("s1"), 80, "aligned at the user block's head");
    // The next step from q2 has no later candidate: no move.
    store.navigate_message("s1", NavDirection::Next, false);
    assert!(!store.resolve_pending_navigation("s1"), "no later target");
    assert_eq!(store.navigation_message("s1").as_deref(), Some("q2"), "the anchor holds");
    // User-only previous from q2: q1 (assistants skipped).
    store.navigate_message("s1", NavDirection::Previous, true);
    assert!(store.resolve_pending_navigation("s1"));
    assert_eq!(store.navigation_message("s1").as_deref(), Some("q1"));
    assert_eq!(store.scroll_offset("s1"), 10);
    // The full mount pinned for the walk.
    assert_eq!(
        store.mount_window("s1"),
        crate::components::message::list::MountWindow { hidden: Some(0), end: None }
    );
}

#[test]
fn first_jumps_backfills_and_reveals_walk_up() {
    let store = Store::new(Runtime::new());
    store.open_tab("s1", None);
    let sink = std::rc::Rc::new(RecordingTranscriptSink::default());
    store.set_transcript_sink(sink.clone());
    store.set_transcript_metrics(
        "s1",
        TranscriptMetrics { viewport: 10, content_height: 60, rows: 100 },
    );
    store.set_message_cursor("s1", Some("cur1".to_owned()));
    // session.first: the all-pages backfill + the head window.
    store.session_first("s1");
    assert_eq!(
        store.mount_window("s1"),
        crate::components::message::list::MountWindow {
            hidden: Some(0),
            end: Some(crate::components::message::list::TRANSCRIPT_BACKFILL_CHUNK)
        }
    );
    assert_eq!(store.scroll_offset("s1"), 0, "pinned at the head");
    assert!(
        sink.calls.borrow().iter().any(|call| call.contains("cur1") && call.contains("all")),
        "the all-pages load fired: {:?}",
        sink.calls.borrow()
    );
    // The reveal walk: an up-move near the mounted top extends the
    // window by the chunk; at the top with more, another page.
    store.clear_mount_window("s1");
    assert!(store.reveal_older_rows("s1"), "the default tail window reveals");
    assert_eq!(
        store.mount_window("s1").hidden,
        Some(60 - crate::components::message::list::TRANSCRIPT_BACKFILL_CHUNK),
        "the default tail (rows-before 40 of 100) reveals one chunk"
    );
    // At the top with more history: the server page load fires.
    assert!(store.reveal_older_rows("s1"), "the top reached, the page load took over");
    assert!(
        sink.calls.borrow().iter().any(|call| call.contains("page")),
        "the page load fired: {:?}",
        sink.calls.borrow()
    );
    // Exhausted history: no more reveals past the top.
    store.set_message_cursor("s1", None);
    store.set_mount_window(
        "s1",
        crate::components::message::list::MountWindow { hidden: Some(0), end: None },
    );
    assert!(!store.reveal_older_rows("s1"), "nothing left to reveal");
}

// -- TUI-TD03: the parent lookup ---------------------------------

#[test]
fn parent_route_resolves_through_the_registry() {
    let store = Store::new(Runtime::new());
    store.open_tab("root", None);
    // The resync path seeds the family links (session.list rows).
    store.ingest_all([Ingest::Resync {
        sessions: vec![
            crate::data::SessionSnapshot {
                id: "root".to_owned(),
                title: None,
                parent_id: None,
                time_idle: None,
                viewed: None,
                model: None,
                ..Default::default()
            },
            crate::data::SessionSnapshot {
                id: "kid".to_owned(),
                title: None,
                parent_id: Some("root".to_owned()),
                time_idle: None,
                viewed: None,
                model: None,
                ..Default::default()
            },
        ],
        unread: Vec::new(),
        messages: Vec::new(),
        cursors: Vec::new(),
        permissions: Vec::new(),
        forms: Vec::new(),
    }]);
    store.navigate(Route::Session { session_id: "kid".to_owned() });
    assert_eq!(store.parent_session_id("kid").as_deref(), Some("root"));
    assert!(store.navigate_parent("kid"), "the up-route navigated");
    assert_eq!(store.route(), Route::Session { session_id: "root".to_owned() });
    assert!(!store.navigate_parent("root"), "a root has no parent");
}
