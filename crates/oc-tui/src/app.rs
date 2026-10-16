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
//! 3. **The keymap's leader interception** (D-02): v2's keymap
//!    prepends its key listener above every focused renderable
//!    (`@opentui/keymap/opentui.ts` `onKeyPress` →
//!    `renderer.keyInput.prependListener`), and the focused surfaces
//!    skip the leader stroke and any stroke while a leader sequence is
//!    pending (`terminal-pane.tsx:144` — `if (keymap.isLeader(event)
//!    || leader()) return`; `prompt/index.tsx:192-193` mutes the
//!    prompt while pending). Leader chords therefore fire with a
//!    NON-EMPTY draft: the chord suspends the textarea, executes, and
//!    the draft survives intact. `app.exit` stays conditional on the
//!    empty prompt (`<leader>q` merely types `q` into text).
//! 4. **The conditional `app.exit` layer**: enabled only while the
//!    focused prompt is EMPTY (`Keymap.createLayer({ enabled: () =>
//!    prompt.text === "" })`).
//! 5. **The managed textarea layer** (`registerManagedTextareaLayer`):
//!    with text present, printable/editing keys feed the composer.
//! 6. **The base layer** (`ctrl+p`, leader chords, tab commands …).
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
use crate::components::composer::render::prompt_text_width;
use crate::components::composer::{ComposerEditCommand, ComposerKey, KeyOutcome};
use crate::components::dialog::{execute_submission, handle_dialog_key, render_app};
use crate::components::footer::permission::{
    create_permission_body_state, permission_key, PermissionBodyState, PermissionKey,
    PermissionReply, PermissionRequest,
};
use crate::components::footer::{FooterTheme, FooterView};
use crate::data::{Connection, HttpResync, Ingest};
use crate::keymap::{KeyStroke, Keymap, ManualClock, Outcome};
use crate::render::color_mode::{self, ColorMode};
use crate::render::Frame;
use crate::store::{Focus, Route, Store};
use crate::theme::{ThemeRegistry, ThemeState};
use oc_theme::types::Mode;

/// The sentinel session key for the home route's composer draft (the
/// reference's Home mounts the same Prompt component).
pub const HOME_PROMPT_KEY: &str = "home";

/// The docked height of the permission body while a blocker is live.
const PERMISSION_MOUNT_HEIGHT: usize = 16;

/// The docked height of a footer picker panel (the mini footer family's
/// `RUN_COMMAND_PANEL_ROWS + 1` mount).
const FOOTER_PANEL_MOUNT_HEIGHT: usize = 17;

/// The headless/live application: the full keymap dispatch over the
/// store, with the keymap's `dialog`/`input` modes kept in sync with
/// the dialog stack and the focused composer.
#[allow(clippy::struct_excessive_bools)] // the pump's mode flags grew one per lane
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
    /// Rename submits pending their `session.update` POST (v2
    /// routes/session/index.tsx:908 — the dialog closes immediately;
    /// the `session.renamed` echo lands the new title through the
    /// event stream).
    pub rename_requests: Vec<(String, String)>,
    /// `--auto` (v2 permission.tsx:14's `args.auto ? "autoaccept"`):
    /// armed at boot, never flipped at runtime (v2's args are
    /// boot-immutable too).
    pub autoaccept: bool,
    /// The asks autoaccept already answered (v2's `autoApproved` set,
    /// session/index.tsx:252-254 — one `once` per request id).
    auto_approved: std::collections::HashSet<String>,
    size: (u16, u16),
    /// The last pointer cell (motion/up events) — the strip's hover
    /// re-evaluation source after a re-layout moves controls under a
    /// stationary pointer (opentui re-dispatches hover on render).
    last_pointer: Option<(usize, usize)>,
    /// Whether the exit command fired (the run loop's teardown gate).
    pub exit_requested: bool,
    /// The applied keybind override set (re-merged when the store's
    /// config-reload signal changes).
    applied_keybinds: Vec<crate::keymap::KeybindOverride>,
    /// The plugin-owned keymap entries (the host's registration
    /// snapshot's keybind chords) — re-applied after every keybind
    /// re-merge (a fresh `Keymap` drops owned entries).
    plugin_owned: Vec<crate::keymap::OwnedCommandDef>,
    /// The plugin-registered theme documents — re-applied after every
    /// config reload's registry rebuild.
    plugin_themes: Vec<(String, String)>,
}

impl App {
    #[must_use]
    pub fn new(size: (u16, u16)) -> Self {
        Self::with_store(Store::new(crate::reactive::Runtime::new()), size)
    }

    #[must_use]
    pub fn with_store(store: Store, size: (u16, u16)) -> Self {
        // The sidebar toggle's visibility input (session-frame.tsx's
        // `rightPane()` reads the live dimensions).
        store.set_terminal_size(size);
        // The editing engines' wrap viewport matches the composer's
        // display wrap from boot (lane M-engine's opentui path).
        store.set_prompt_viewport_width(prompt_text_width(usize::from(size.0)));
        Self {
            store,
            keymap: Keymap::new(),
            clock: ManualClock::default(),
            dispatcher: StoreDispatcher,
            dialog_mode: false,
            input_mode: false,
            permission_states: HashMap::new(),
            permission_replies: Vec::new(),
            rename_requests: Vec::new(),
            autoaccept: false,
            auto_approved: std::collections::HashSet::new(),
            size,
            last_pointer: None,
            exit_requested: false,
            applied_keybinds: Vec::new(),
            plugin_owned: Vec::new(),
            plugin_themes: Vec::new(),
        }
    }

    /// Install the plugin-owned keymap entries (the registration
    /// snapshot's keybind chords — `App::remerge_keybinds` re-applies
    /// them after every override re-merge, since a fresh `Keymap`
    /// drops owned entries).
    pub fn set_plugin_commands(&mut self, owned: Vec<crate::keymap::OwnedCommandDef>) {
        self.plugin_owned = owned;
        for definition in &self.plugin_owned {
            self.keymap.register_owned(definition.clone());
        }
    }

    /// Install the plugin-registered theme documents: applied into the
    /// live theme registry immediately AND re-applied by
    /// [`App::apply_config`] on every config reload (which rebuilds the
    /// registry from disk discovery).
    pub fn set_plugin_themes(&mut self, themes: Vec<(String, String)>) {
        self.plugin_themes = themes;
        self.apply_plugin_themes();
    }

    /// Rebuild the live registry's plugin-theme segment.
    fn apply_plugin_themes(&self) {
        if self.plugin_themes.is_empty() {
            return;
        }
        let mut registry = self.store.registry();
        crate::theme::register_custom_themes(&mut registry, &self.plugin_themes);
        self.store.set_registry(registry);
    }

    /// Apply the theme registry + user keybinds from a loaded config
    /// (boot + reload): custom themes register into the registry, the
    /// theme name/mode applies, and the keybind overrides merge into
    /// the live keymap.
    pub fn apply_config(&mut self, config: &crate::config::TuiConfig, mode_hint: Mode) {
        // Attention (R-06): the resolved sounds/notifications config
        // reaches the engine on boot AND every reload.
        self.store.set_attention_config(config.attention.clone());
        // Registry: builtins + discovered customs (discovery ran in the
        // caller — the config only carries overrides here).
        let mut registry = ThemeRegistry::builtins();
        crate::theme::register_custom_themes(
            &mut registry,
            &crate::theme::discover_custom_themes(&Self::config_directories()),
        );
        self.store.set_registry(registry);
        // Plugin themes ride every registry rebuild (config reload).
        self.apply_plugin_themes();
        // Theme name + mode. v2's default follows the terminal: the
        // mode is `lock ?? renderer.themeMode ?? props.mode` — only an
        // explicit dark/light pin overrides the probe
        // (context/theme.tsx:134). The probe's dark fallback covers
        // headless/timeout boots.
        let mode = match config.theme.mode.as_deref() {
            Some("light") => Mode::Light,
            _ => mode_hint,
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
        // R-07: the transcript image strip's gate (`session.image_preview`).
        self.store.set_image_preview(config.session_image_preview);
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
        // The plugin-owned chords re-register onto the fresh table
        // (a config reload must not drop plugin keybinds).
        for definition in &self.plugin_owned {
            self.keymap.register_owned(definition.clone());
        }
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
        self.store.set_terminal_size(size);
        self.store.set_prompt_viewport_width(prompt_text_width(usize::from(size.0)));
        self.store.request_redraw();
    }

    /// Feed one key spec ("ctrl+p", "r", "enter", "BACKSPACE", …).
    /// Replay-script key tokens use the kitty vocabulary; the reference
    /// keymap expands the aliases (`enter → return`, `esc → escape`,
    /// `pgdown/pgup → page…`; `KeyCodes` names like BACKSPACE lowercase —
    /// single characters keep their case for the shift form).
    /// A terminal paste (bracketed paste — R-07): the focused
    /// composer's `pasteInputText` — pasted text that names local
    /// image/PDF file(s) stages `[Image N]` attachment chips (the
    /// resolution is bounded filesystem work, ≤20 MiB per file; v2
    /// enqueues it async — the sync pump here trades that for the
    /// single-read latency, pastes being user-rare).
    pub fn paste_terminal_text(&mut self, text: &str) {
        let key = self.prompt_key();
        let mut composer = self.store.composer(&key);
        composer.paste_resolved(
            text,
            "unix",
            &crate::components::composer::attachment::FsLocalFiles,
        );
        self.store.set_composer(&key, &composer);
        self.keymap.clear_pending();
    }

    /// One terminal mouse event (beta.8 bugs 3 + 4). v2's tab strip
    /// is a POINTER surface — hover lifts the tab and reveals the `✕`
    /// (`session-tabs.tsx:1652-1757`), a left release selects
    /// (`release()` → `tabs.select`, `session-tabs.tsx:1505-1514`),
    /// the `✕` closes (`tabs.close`, `session-tabs.tsx:1745-1754`),
    /// the ` + ` adds (`tabs.add?.()` → home route,
    /// `session-tabs.tsx:1786-1792` + `406-416`) — and the reasoning
    /// group's `+ Thought` header toggles its disclosure on release
    /// (`onMouseUp={toggle}`, `group-view.tsx:152-190`). This routes
    /// crossterm's mouse stream to exactly those surfaces.
    // The pointer routing table (modal → footer label → strip →
    // transcript hits) — one screen-sized match, kept whole like the
    // sibling key layers.
    #[allow(clippy::too_many_lines)]
    pub fn mouse_event(&mut self, mouse: &crossterm::event::MouseEvent) {
        use crate::components::session_tabs::{strip_hit, StripHit};
        use crossterm::event::{MouseButton, MouseEventKind};
        let width = usize::from(self.size.0);
        let col = usize::from(mouse.column);
        let row = usize::from(mouse.row);
        // The strip renders on row 0 while tabs are enabled and nothing
        // modal owns the screen — on EVERY non-plugin route, exactly
        // like v2's app-wide mount (`app.tsx:1353-1355` seats
        // `<SessionTabs/>` above the route `<Switch>`; `tabsAvailable()`
        // gates on `route.data.type !== "plugin"` only, `app.tsx:576-579`
        // — home included, no active pill). The dogfood2 repro: restored
        // tabs on the home route painted but took no clicks because this
        // gate was session-only.
        let strip_live = self.store.tabs_enabled()
            && self.store.dialogs().is_empty()
            && !self.permission_live()
            && !self.store.tabs().tabs.is_empty();
        // Track the pointer for the strip's post-mutation hover
        // re-evaluation (`refresh_strip_hover`).
        if matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Up(MouseButton::Left)) {
            self.last_pointer = Some((col, row));
        }
        match mouse.kind {
            MouseEventKind::Moved => {
                // A modal select's option rows track the pointer
                // (`dialog-select.tsx:762-767`: `onMouseMove` moves
                // the selection).
                if !self.store.dialogs().is_empty() {
                    if let Some(index) = self.dialog_option_at(col, row) {
                        self.move_dialog_selection_to(index);
                    }
                    return;
                }
                // The permission pills track the pointer (auditA #4 —
                // permission.tsx:583: `onMouseMove` moves the
                // selection; the ask's own surface is the screen's
                // only interactive one while it holds the footer).
                if self.permission_live() {
                    if self.permission_hover_select(col, row) {
                        self.store.request_redraw();
                    }
                    return;
                }
                // #33 — the footer location label's hover lift
                // (`useWorkingDirectoryActions`: muted → text.base).
                // Route-gated (auditA §4.1): a stale session rect
                // must not flip the hover state on Home.
                let location_hover = matches!(self.store.route(), Route::Session { .. })
                    && self.store.footer_location_hit().is_some_and(|hit| {
                        row == hit.row && col >= hit.x && col < hit.x + hit.width
                    })
                    && !self.permission_live();
                let changed_location = self.store.set_location_hovered(location_hover);
                let (hover, close, add) = if strip_live && row == 0 {
                    self.strip_hover_at(col, width)
                } else {
                    (None, false, false)
                };
                let changed = self.store.set_tab_hover(hover.as_deref(), close);
                let changed_add = self.store.set_tab_add_hovered(add);
                if changed || changed_add || changed_location {
                    self.store.request_redraw();
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // A modal dialog's option rows submit on release
                // (`dialog-select.tsx:768-773`: mousedown moved the
                // selection, mouseup fires `onSelect`); the backdrop
                // dismisses on an OUTSIDE release (`ui/dialog.tsx:
                // 34-63`: the full-screen backdrop box closes on
                // mouseup; the inner chrome box's own mouseup stops
                // propagation).
                if !self.store.dialogs().is_empty() {
                    if let Some(index) = self.dialog_option_at(col, row) {
                        self.submit_dialog_option(index);
                    } else if let Some((x, y, width, height)) = self.store.dialog_frame_rect() {
                        if col < x || row < y || col >= x + width || row >= y + height {
                            self.store.close_dialog();
                        }
                    }
                    self.store.request_redraw();
                    return;
                }
                // The permission ask's own pointer surface (auditA
                // #4): v2 renders the pills and the reject stage's
                // `enter confirm` / `esc cancel` hints as the ONLY
                // interactive boxes while the ask holds the footer
                // (permission.tsx:375-400, 583-590) — the early
                // return that protects the transcript must not kill
                // them. Release on a pill: select + run (the keyboard
                // enter path with the selection set); release on the
                // reject buttons: confirm sends the reject reply /
                // cancel returns to the ask.
                if self.permission_live() {
                    if self.permission_release(col, row) {
                        self.store.request_redraw();
                    }
                    return;
                }
                if strip_live && row == 0 {
                    let hit = {
                        let (tabs, active, show_plus) = self.strip_layout_tabs();
                        let hover = self.store.tabs().hover.clone();
                        strip_hit(&tabs, active.as_deref(), show_plus, width, col, hover.as_deref())
                    };
                    match hit {
                        StripHit::Tab(id) => {
                            // The home pseudo-tab IS the home route —
                            // selecting it is a stay-put (`NEW_SESSION_TAB`
                            // has no session to route to).
                            if id != crate::components::session_tabs::NEW_SESSION_TAB_ID {
                                self.store.select_tab(&id);
                            }
                        }
                        StripHit::Close(id) => {
                            if id == crate::components::session_tabs::NEW_SESSION_TAB_ID {
                                // `tabs.close()` on the promoted slot
                                // (session-tabs.tsx:1745-1753 +
                                // context `close(undefined)` :420-433):
                                // no target → navigate to the previous
                                // history session, else the last tab.
                                let tabs = self.store.tabs();
                                let fallback = tabs.tabs.last().map(|tab| tab.session_id.clone());
                                if let Some(session_id) = fallback {
                                    self.store.select_tab(&session_id);
                                }
                            } else {
                                self.store.close_tab(Some(&id));
                            }
                        }
                        StripHit::Add => self.store.navigate(Route::Home),
                        StripHit::Outside => {}
                    }
                    // opentui re-dispatches hover from the LAST
                    // pointer position whenever a re-layout moves
                    // controls under it (the multi-session ✕-persists
                    // census cells): a release that mutates the strip
                    // re-hits the stationary pointer.
                    self.refresh_strip_hover(width);
                    return;
                }
                // The sidebar onboarding card's two pointer targets
                // (footer.tsx:45-70): the `✕` release persists
                // `dismissed = true` (the run loop writes the
                // `getting-started` document), the "Connect provider"
                // row dispatches `provider.connect` — the palette
                // row's own handler (the server-list dialog). Only the
                // session route paints the card; the rects clear with
                // it (a hidden card must not zombie-click).
                if !self.permission_live() && matches!(self.store.route(), Route::Session { .. }) {
                    if let Some(hits) = self.store.sidebar_onboarding_hits() {
                        if hits.dismiss.contains(col, row) {
                            self.store.dismiss_sidebar_onboarding();
                            self.store.request_redraw();
                            return;
                        }
                        if hits.connect.contains(col, row) {
                            let dispatch =
                                self.dispatcher.dispatch(&self.store, "provider.connect", None);
                            if let Dispatch::ServerPending(reason) = dispatch {
                                let _ = reason;
                            }
                            self.store.request_redraw();
                            return;
                        }
                    }
                }
                // #33 — the footer location label opens the Working
                // directory select on release
                // (`useWorkingDirectoryActions.openMenu`,
                // prompt/index.tsx:1932-1934). Route-gated (auditA
                // §4.1): the rect belongs to the session render —
                // v2's label unmounts with the route, so a stale
                // rect must never fire on Home.
                if !self.permission_live() && matches!(self.store.route(), Route::Session { .. }) {
                    if let Some(hit) = self.store.footer_location_hit() {
                        if row == hit.row && col >= hit.x && col < hit.x + hit.width {
                            self.store.open_dialog(
                                crate::store::DialogKind::WorkingDirectory {
                                    directory: hit.directory.clone(),
                                },
                                crate::store::DialogSize::Medium,
                            );
                            self.store.request_redraw();
                            return;
                        }
                    }
                }
                // The transcript's click surface — the hit rows were
                // recorded by the last `render_session` pass with
                // absolute positions:
                // * reasoning headers toggle on release (beta.8 bug
                //   4) with a TIGHT rect: v2's toggle box is the
                //   InlineToolRow content (icon + label glyphs), not
                //   the whole row (`group-view.tsx:176-190`);
                // * user-message blocks open the Message Actions
                //   select (`UserMessage`'s `onMouseUp`,
                //   session/index.tsx:2210-2250).
                // Nothing pointer-driven happens under a modal.
                // Route-gated (auditA §4.1): each row carries its
                // session id — the release only fires while THAT
                // session is the live route (v2's rows unmount with
                // the route; the belt to `navigate`'s registry
                // clear, covering the window between a route flip
                // and the next render pass).
                if self.permission_live() {
                    return;
                }
                let route_session = match self.store.route() {
                    Route::Session { session_id } => Some(session_id),
                    Route::Home => None,
                };
                for hit in self.store.transcript_hit_rows() {
                    if route_session.as_deref() != Some(hit.session_id.as_str()) {
                        continue;
                    }
                    if row >= hit.y
                        && row < hit.y + hit.height
                        && col >= hit.x
                        && col < hit.x + hit.width
                    {
                        match &hit.action {
                            crate::store::TranscriptHitAction::ToggleGroup(key) => {
                                self.store.toggle_group_expanded(&hit.session_id, key);
                            }
                            crate::store::TranscriptHitAction::ToggleThought { message_id, part_id } => {
                                self.store
                                    .toggle_thought_expanded(&hit.session_id, message_id, part_id);
                            }
                            crate::store::TranscriptHitAction::MessageActions { message_id } => {
                                self.store.open_dialog(
                                    crate::store::DialogKind::MessageActions {
                                        session_id: hit.session_id.clone(),
                                        message_id: message_id.clone(),
                                    },
                                    crate::store::DialogSize::Medium,
                                );
                            }
                        }
                        self.store.request_redraw();
                        return;
                    }
                }
            }
            // The wheel (auditA #1 — `MouseEventKind::Scroll*` was
            // unhandled anywhere): v2 forwards wheel events into the
            // focused scrollbox (`session-frame.tsx:310-317` hands
            // the transcript's scroll renderable
            // `processMouseEvent`; every dialog list is a scrollbox;
            // the sidebar's content slot is one). Routing order
            // mirrors the modal-first pointer table: dialog →
            // permission → sidebar columns → the transcript.
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let down = matches!(mouse.kind, MouseEventKind::ScrollDown);
                // A modal select's option window scrolls WITHOUT
                // moving the selection (v2's scrollbox; the rows
                // keep their own pointer tracking). Terminal-wheel
                // semantics: 3 rows a notch (opentui's own
                // `direction === "up" ? -3 : 3` delta).
                if !self.store.dialogs().is_empty() {
                    if self.dialog_scroll_window_by(if down { 3 } else { -3 }) {
                        self.store.request_redraw();
                    }
                    return;
                }
                // The permission body's content region doesn't
                // window in the port (it renders every line) — the
                // wheel is consumed, never leaked to the transcript
                // underneath (v2's overlay owns the pointer).
                if self.permission_live() {
                    return;
                }
                if let Route::Session { session_id } = self.store.route() {
                    // The sidebar's content slot: wheel over the
                    // pane's columns scrolls the sections (v2's
                    // `sidebar.content` scrollbox).
                    if self.store.sidebar_scroll().contains_col(col)
                        && self.store.sidebar_scroll_by(if down { 3 } else { -3 })
                    {
                        self.store.request_redraw();
                        return;
                    }
                    // The transcript: v2's wheel drives the scroll
                    // renderable; the port routes it through the
                    // SAME driver the page keys use — one notch =
                    // `session.page.up/down` (the E1 half-page math:
                    // ±viewport/2 with js_round, sticky re-arming at
                    // the tail — `scroll_driver_matches_the_ported_
                    // scrollbar_math` pins the exact landing).
                    let command = if down {
                        oc_tui_core::scroll::ScrollCommand::PageDown
                    } else {
                        oc_tui_core::scroll::ScrollCommand::PageUp
                    };
                    self.store.scroll_command(&session_id, command);
                    self.store.request_redraw();
                }
                // Home has no scroll surface. DIFF-VIEWER HOOK POINT:
                // when `components/diff_viewer.rs` mounts (auditA
                // §2.12 — currently dead code: no route, no
                // dispatcher arm), its file tree + hunk panes own the
                // wheel here (v2 `diff-viewer.tsx`: two scrollboxes),
                // gated on the viewer's route/layer ABOVE the
                // transcript arm — wire `diff.down/up` line steps,
                // not the page math.
            }
            _ => {}
        }
    }

    /// The flat option index under (`col`, `row`), when the top dialog
    /// is a select showing that row (`dialog-select.tsx`'s option box
    /// spans the dialog width).
    fn dialog_option_at(&self, col: usize, row: usize) -> Option<usize> {
        let (x, _y, width, _) = self.store.dialog_frame_rect()?;
        if col < x || col >= x + width {
            return None;
        }
        self.store.dialog_option_rows().into_iter().position(|option_row| option_row == Some(row))
    }

    /// Wheel-scroll the top dialog's option window (auditA #1): v2's
    /// dialog lists are scrollboxes — the wheel moves the WINDOW,
    /// never the selection (the rows' own pointer tracking stays
    /// independent, `dialog-select.tsx`'s scrollbox wheel). `false`
    /// when the window cannot move (short list / already at an end).
    fn dialog_scroll_window_by(&self, delta: i32) -> bool {
        let Some(entry) = self.store.top_dialog() else { return false };
        let props = crate::components::command_palette::select_props_for_entry(&entry, &self.store);
        let height = usize::from(self.size.1);
        let mut moved = false;
        self.store.update_dialog(|top| {
            moved = top.select.scroll_window_by(&props, height, delta);
        });
        moved
    }

    /// The strip's layout inputs RIGHT NOW: the store tabs plus the
    /// home-route `New session` pseudo-tab (selected; the idle ` + `
    /// hides — session-tabs.tsx:1339-1344).
    fn strip_layout_tabs(&self) -> (Vec<crate::store::SessionTab>, Option<String>, bool) {
        let tabs = self.store.tabs();
        let mut items: Vec<crate::store::SessionTab> = tabs.tabs.clone();
        if matches!(self.store.route(), Route::Home) && !items.is_empty() {
            items.push(crate::store::SessionTab {
                session_id: crate::components::session_tabs::NEW_SESSION_TAB_ID.to_owned(),
                title: Some(crate::components::session_tabs::NEW_SESSION_TAB_TITLE.to_owned()),
            });
            (items, Some(crate::components::session_tabs::NEW_SESSION_TAB_ID.to_owned()), false)
        } else {
            let active = match self.store.route() {
                Route::Session { session_id } => Some(session_id),
                Route::Home => None,
            };
            (items, active, true)
        }
    }

    /// The strip hover state under pointer column `col`:
    /// (hovered tab, pointer-on-✕, ` + ` hovered).
    fn strip_hover_at(&self, col: usize, width: usize) -> (Option<String>, bool, bool) {
        use crate::components::session_tabs::{strip_hit, StripHit};
        let (tabs, active, show_plus) = self.strip_layout_tabs();
        let hover = self.store.tabs().hover.clone();
        match strip_hit(&tabs, active.as_deref(), show_plus, width, col, hover.as_deref()) {
            StripHit::Tab(id) => (Some(id), false, false),
            StripHit::Close(id) => (Some(id), true, false),
            StripHit::Add => (None, false, true),
            StripHit::Outside => (None, false, false),
        }
    }

    /// Re-evaluate the strip hover from the LAST pointer position —
    /// opentui's renderer re-dispatches `mouseOver/mouseOut` whenever
    /// a re-render moves elements under a stationary pointer (the
    /// multi-session census: the ✕ stays revealed on the tab that
    /// grew under the pointer after an add-release).
    fn refresh_strip_hover(&mut self, width: usize) {
        let Some((col, row)) = self.last_pointer else { return };
        if row != 0 {
            return;
        }
        let (hover, close, add) = self.strip_hover_at(col, width);
        let changed = self.store.set_tab_hover(hover.as_deref(), close);
        let changed_add = self.store.set_tab_add_hovered(add);
        if changed || changed_add {
            self.store.request_redraw();
        }
    }

    /// The paint-path twin of [`Self::refresh_strip_hover`] — the
    /// render-driven re-hover (v2's reconciler redoes hit-testing on
    /// every tree change; ours recomputes at each frame paint).
    fn rehover_strip_from_last_pointer(&self) {
        let Some((col, row)) = self.last_pointer else { return };
        if row != 0 || !self.store.dialogs().is_empty() || self.permission_live() {
            return;
        }
        if !self.store.tabs_enabled() || self.store.tabs().tabs.is_empty() {
            return;
        }
        let width = usize::from(self.size.0);
        let (hover, close, add) = self.strip_hover_at(col, width);
        let changed = self.store.set_tab_hover(hover.as_deref(), close);
        let changed_add = self.store.set_tab_add_hovered(add);
        if changed || changed_add {
            self.store.request_redraw();
        }
    }

    /// Move the top dialog's selection to a flat index (pointer
    /// motion over an option row — `moveTo`).
    fn move_dialog_selection_to(&mut self, index: usize) {
        let before = self.store.dialogs().last().map_or(usize::MAX, |top| top.select.selected);
        self.store.update_dialog(|top| top.select.selected = index);
        if before != index {
            // Pointer motion runs the same `moveTo` → `onMove` path as
            // the keyboard — the theme list previews on it too.
            if self
                .store
                .dialogs()
                .last()
                .is_some_and(|top| matches!(top.kind, crate::store::DialogKind::ThemeList))
            {
                self.store.preview_theme_move();
            }
            self.store.request_redraw();
        }
    }

    /// Release on an option row: select it and submit — through the
    /// same stroke path as a keyboard enter (the keymap's modal layer
    /// is armed by `press`'s mode sync).
    fn submit_dialog_option(&mut self, index: usize) {
        self.move_dialog_selection_to(index);
        self.press("enter");
    }

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
        } else if self.error_toast_live() {
            // F11 — the error panel consumes input until closed: while
            // an ERROR-variant toast is up (the "Session failed"
            // surface), keystrokes must not fall through into the
            // composer (a typed key or Enter re-submitting into the
            // failed turn is the trap the dogfood hit). escape /
            // ctrl+c dismiss the panel; every other stroke is
            // consumed. Info/success/warning toasts stay transparent
            // (v2's toast never blocks typing — only the error variant
            // rides this modal gate).
            let plain = !stroke.ctrl && !stroke.alt;
            if (plain && stroke.key == "escape") || (stroke.ctrl && plain && stroke.key == "c") {
                self.store.toast_dismiss();
            }
        } else if self.picker_live() {
            // The composer's subagent picker (#44): the panel REPLACES
            // the prompt (v2's Switch mounts no prompt while
            // `composer.open || parentID` — index.tsx:1464-1465), so
            // the composer-mode layers own the keys: escape/ctrl+c
            // close (navigating to the parent on a child session),
            // arrows/enter/ctrl+a drive the list, tabs switch with
            // left/right; leader chords still resolve through base.
            self.stroke_in_picker(stroke);
        } else {
            self.stroke_on_route(&stroke);
        }
        self.sync_modes();
        self.store.runtime.run_effects();
    }

    /// The composer's subagent picker holds the session route (#44):
    /// explicitly open or the route is a child session (which always
    /// mounts the panel — v2 index.tsx:1451).
    fn picker_live(&self) -> bool {
        matches!(self.store.route(), crate::store::Route::Session { session_id }
            if self.store.picker_live(&session_id))
    }

    /// One stroke while the picker panel is mounted: the composer
    /// mode's bindings (`composer.*` — the table's `composer` layer,
    /// pushed while live) resolve ABOVE base, so ctrl+c/escape close
    /// the panel instead of exiting and the arrows/list keys drive
    /// the entries. Unbound strokes are consumed — the prompt is
    /// unmounted, nothing types.
    fn stroke_in_picker(&mut self, stroke: KeyStroke) {
        if let Outcome::Command { command, args } = self.keymap.lookup(stroke, &self.clock) {
            let dispatch = self.dispatcher.dispatch(&self.store, &command, args.as_deref());
            if let Dispatch::ServerPending(reason) = dispatch {
                let _ = reason;
            }
            self.keymap.clear_pending();
        }
        // A pending leader sequence, its cancellation, or an unbound
        // stroke: consumed — the prompt is unmounted while the panel
        // shows, nothing types.
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

    /// The pill under (`col`, `row`), when the pointer sits on the
    /// controls row inside one of the offered pills' extents.
    fn permission_pill_at(
        &self,
        col: usize,
        row: usize,
    ) -> Option<crate::components::footer::permission::PermissionOption> {
        let hits = self.store.permission_pointer_hits()?;
        if row != hits.row {
            return None;
        }
        hits.pills
            .iter()
            .find(|(_, (x, width))| col >= *x && col < *x + *width)
            .map(|(option, _)| *option)
    }

    /// Pointer motion over a pill moves the selection (v2
    /// `onMouseMove={() => setStore("selected", option)}`,
    /// permission.tsx:583); `true` when the state changed.
    fn permission_hover_select(&mut self, col: usize, row: usize) -> bool {
        let Some(option) = self.permission_pill_at(col, row) else { return false };
        let FooterView::Permission { request } = self.store.footer().view.clone() else {
            return false;
        };
        let state = self
            .permission_states
            .get(&request.id)
            .cloned()
            .unwrap_or_else(|| create_permission_body_state(&request));
        if state.selected == option {
            return false;
        }
        self.permission_states.insert(
            request.id.clone(),
            crate::components::footer::permission::PermissionBodyState {
                selected: option,
                ..state
            },
        );
        true
    }

    /// A release on the permission body's pointer targets (auditA
    /// #4): a pill selects + RUNS (the keyboard enter path with the
    /// selection set — `onMouseUp` sets the selection then fires
    /// `onSelect`, permission.tsx:584-587); the reject stage's
    /// confirm/cancel buttons map to the reject-stage enter/escape
    /// (`onConfirm`/`onCancel`, permission.tsx:381/395). `true` when
    /// a target consumed the release.
    fn permission_release(&mut self, col: usize, row: usize) -> bool {
        use crate::components::footer::permission::{permission_key, PermissionKey};
        let hits = self.store.permission_pointer_hits();
        let FooterView::Permission { request } = self.store.footer().view.clone() else {
            return false;
        };
        let state = self
            .permission_states
            .get(&request.id)
            .cloned()
            .unwrap_or_else(|| create_permission_body_state(&request));
        // The reject stage's hint buttons: `enter confirm` sends the
        // reject reply (with the typed message), `esc cancel` returns
        // to the ask with the selection on Reject.
        let key = if row == hits.as_ref().map_or(usize::MAX, |hits| hits.row) {
            if let (Some(hits), true) =
                (&hits, state.stage == crate::components::footer::permission::PermissionStage::Reject)
            {
                let inside = |rect: Option<&(usize, usize)>| {
                    rect.is_some_and(|(x, width)| col >= *x && col < *x + *width)
                };
                if inside(hits.confirm.as_ref()) {
                    Some("enter")
                } else if inside(hits.cancel.as_ref()) {
                    Some("escape")
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let Some(key) = key else {
            // Not on the hint buttons — the pills (Permission/Always
            // stages): set the selection, then run the enter path.
            return match self.permission_pill_at(col, row) {
                Some(option) => {
                    let state = crate::components::footer::permission::PermissionBodyState {
                        selected: option,
                        ..state
                    };
                    match permission_key(&state, &request, "enter") {
                        PermissionKey::State(next) => {
                            self.permission_states.insert(request.id.clone(), next);
                        }
                        PermissionKey::Reply(reply, next) => {
                            self.permission_replies.push(reply.clone());
                            self.permission_states.insert(request.id.clone(), next);
                            // The reply resolves the ask server-side
                            // (`permission.replied`) — the view falls
                            // back.
                            self.store.permission_replied(&reply.request_id);
                        }
                        _ => {}
                    }
                    true
                }
                None => false,
            };
        };
        match permission_key(&state, &request, key) {
            PermissionKey::State(next) => {
                self.permission_states.insert(request.id.clone(), next);
            }
            PermissionKey::Reply(reply, next) => {
                self.permission_replies.push(reply.clone());
                self.permission_states.insert(request.id.clone(), next);
                self.store.permission_replied(&reply.request_id);
            }
            _ => {}
        }
        true
    }

    /// v2's overlay-sidebar keymap gate (routes/session/index.tsx:244):
    /// the close layer is armed only while the session sidebar is
    /// visible on a NARROW frame — `dimensions().width -
    /// props.verticalTabsWidth <= 120` (the floating scrim overlay;
    /// the wide dock keeps escape for the prompt/dialog chain, and
    /// the sidebar never mounts on Home).
    fn overlay_sidebar_live(&self) -> bool {
        let width = usize::from(self.size.0);
        if width > crate::components::sidebar::SIDEBAR_AUTO_BREAKPOINT {
            return false;
        }
        match self.store.route() {
            Route::Session { session_id } => self.store.sidebar_shown(&session_id, width),
            Route::Home => false,
        }
    }

    /// F11: whether an ERROR panel currently holds the screen — a
    /// TITLED error toast (the "Session failed" / "Failed to send
    /// prompt" panel class). Untitled error notices (the log-skip
    /// surface: transient transport skips like a failed switch POST)
    /// stay transparent, like every non-error toast.
    fn error_toast_live(&self) -> bool {
        self.store
            .toasts()
            .current()
            .is_some_and(|toast| toast.variant == "error" && toast.title.is_some())
    }

    /// `--auto`'s autoaccept arm (v2 session/index.tsx:252-262): while
    /// armed, a live permission ask replies `once` THROUGH the same
    /// queue a keyboard allow uses (`permission_replies` + the local
    /// `permission_replied` resolution — the ask footer falls back),
    /// once per request id (`autoApproved`). Called after every SSE
    /// ingest drain.
    pub fn autoanswer_permissions(&mut self) {
        if !self.autoaccept {
            return;
        }
        let FooterView::Permission { request } = self.store.footer().view.clone() else {
            return;
        };
        if !self.auto_approved.insert(request.id.clone()) {
            return;
        }
        self.permission_replies.push(crate::components::footer::permission::PermissionReply {
            session_id: request.session_id.clone(),
            request_id: request.id.clone(),
            decision: "once".to_owned(),
            message: String::new(),
        });
        // The reply resolves the ask locally (the server echo lands
        // through `permission.replied` — same as the keyboard path).
        self.store.permission_replied(&request.id);
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

    /// Rename submit: optimistically update the tab title (v2's tabs
    /// state re-resolves on its next mutation) and POST
    /// `session.update` — the `session.renamed` echo then lands the
    /// title across every surface (tab label, sidebar, session list)
    /// through the live session registry
    /// (routes/session/index.tsx:903-908).
    fn submit_rename(&mut self, value: &str) {
        let dialogs = self.store.dialogs();
        if let Some(entry) = dialogs.last() {
            match &entry.kind {
                crate::store::DialogKind::SessionRename { session_id } => {
                    let session_id = session_id.clone();
                    if !value.is_empty() {
                        self.store.open_tab(&session_id, Some(value.to_owned()));
                        self.rename_requests.push((session_id, value.to_owned()));
                    }
                }
                // `DialogWorktreeName`'s enter (`confirm()`): slugify
                // (empty → generated), fire `worktree.create` — the
                // created directory lands through the event loop
                // (`apply_worktree_created`).
                crate::store::DialogKind::WorktreeName => {
                    let seed =
                        value.chars().map(|ch| u64::from(ch as u32)).fold(0u64, u64::wrapping_add);
                    self.store.worktree_create_from_name(value, seed);
                }
                _ => {}
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

    /// Whether `stroke` matches one of `command`'s effective bindings
    /// (user overrides respected — the engine's stroke matcher).
    fn stroke_matches_command(&self, stroke: &KeyStroke, command: &str) -> bool {
        self.keymap.stroke_triggers(stroke, command)
    }

    /// Route-layer stroke: conditional exit layer → managed textarea →
    /// input/base keymap (see the module docs).
    #[allow(clippy::too_many_lines)] // the layered dispatch ladder, one commented rung each
    fn stroke_on_route(&mut self, stroke: &KeyStroke) {
        // 0. The overlay-sidebar close layer (v2 routes/session/index.tsx:
        //    243-245 — `Keymap.createLayer({ priority: 10, enabled: () =>
        //    props.sidebarVisible && dimensions().width -
        //    props.verticalTabsWidth <= 120 && !disabled(), commands: [{
        //    bind: "escape,ctrl+c", run: props.onToggleSidebar }] })`):
        //    while the session sidebar floats over a narrow frame (the
        //    scrim overlay — visible and width ≤ 120), escape and
        //    ctrl+c close it before any other route layer sees the
        //    key. Without this the overlay's opaque scrim hides the
        //    composer entirely and NO key recovers the frame (the
        //    pty_fuzz dead end); v2's `disabled()` guard (permissions/
        //    forms open) is the pump's earlier Dialog/permission
        //    branches, which keep their precedence here.
        if self.overlay_sidebar_live() {
            let plain = !stroke.ctrl && !stroke.alt;
            let close_chord = (plain && stroke.key == "escape")
                || (stroke.ctrl && !stroke.alt && stroke.key == "c");
            if close_chord {
                self.store.toggle_sidebar();
                self.keymap.clear_pending();
                return;
            }
        }
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
                Outcome::Command { command, .. } if had_pending && command == "prompt.queue" => {
                    self.queue_prompt_command(&key);
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
        // 1b. D-02 — leader chords with a NON-empty draft: v2's keymap
        //     sees keys above the focused textarea (the prepended
        //     keypress listener), and the focused surface skips the
        //     leader stroke and any continuation while a sequence is
        //     pending (`terminal-pane.tsx:144` — `if (keymap.isLeader-
        //     (event) || leader()) return`). The leader therefore arms
        //     over text, the continuation resolves through the base
        //     layer, the chord executes, and the draft stays intact.
        //     `app.exit` is disabled above while text exists, so
        //     <leader>q falls through fresh and types `q`; an
        //     unmatched/escape continuation is consumed by the keymap
        //     (escape clears the pending sequence), never the textarea.
        else if self.keymap.pending_display().is_some() || self.keymap.is_leader(stroke) {
            let had_pending = self.keymap.pending_display().is_some();
            match self.keymap.lookup_base(stroke.clone(), &self.clock) {
                Outcome::Command { command, .. } if command == "app.exit" => {
                    self.request_exit();
                    return;
                }
                Outcome::Command { command, .. } if had_pending && command == "prompt.queue" => {
                    self.queue_prompt_command(&key);
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

        // 2b. `session.child.first` over an EMPTY draft (#44): the
        //     subagent picker's toggle resolves ABOVE the textarea's
        //     `input.move.down`/`history.next` when nothing is typed
        //     (the corpus3 evidence: `down` opens the composer panel
        //     on a settled session — keybind.ts:136). The picker-live
        //     case never reaches here (press routes it to the
        //     composer layers).
        if draft_empty
            && matches!(self.store.route(), Route::Session { .. })
            && self.stroke_matches_command(stroke, "session.child.first")
        {
            self.store.toggle_picker();
            self.keymap.clear_pending();
            return;
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
                    self.handle_composer_outcome(&key, outcome);
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
            } else if command == "prompt.queue" {
                self.queue_prompt_command(&key);
            } else if composer_command(&command).is_some()
                || composer_special(&command).is_some()
                || command == "input.submit"
            {
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
                self.handle_composer_outcome(&key, outcome);
            }
        }
    }

    /// `prompt.queue` (`<leader>return`, v2 prompt/index.tsx:444-457 +
    /// submitInner's queue arm): an EMPTY draft does nothing; an
    /// unqueueable draft (an exit word / a slash command) warns
    /// ("This prompt cannot be queued", index.tsx:1134-1136); an
    /// ACTIVE turn admits the draft to the local serial queue
    /// (mini/queue.rs's Admit — the dock row appears); an idle session
    /// sends it like a normal submit.
    fn queue_prompt_command(&mut self, key: &str) {
        let draft = self.store.prompt_draft(key);
        if draft.is_empty() {
            return;
        }
        let trimmed = draft.text.trim();
        let exit_word = matches!(trimmed, "exit" | "quit" | ":q");
        let slash = trimmed.starts_with('/');
        let Route::Session { session_id } = self.store.route() else {
            // Home has no live turn to queue behind — send instead.
            let _ = self.store.submit_prompt(key);
            return;
        };
        if exit_word || slash {
            self.store.toast_show(crate::components::toast::ToastOptions {
                message: "This prompt cannot be queued".to_owned(),
                variant: "warning",
                ..crate::components::toast::ToastOptions::info("")
            });
            return;
        }
        if self.store.session_running(&session_id) {
            // The Admit path (mini/queue.rs): the queued copy IS the
            // record; the draft clears (submit semantics) but the
            // sink send waits for the drain.
            let text = draft.text.clone();
            self.store.queue_prompt(&session_id, &text);
            let mut composer = self.store.composer(key);
            if let Some(prompt) = composer.take_submit() {
                composer.history = composer.history.clone().push(&prompt.text);
                self.store.set_composer(key, &composer);
            }
            return;
        }
        // Idle: an ordinary submit (`submit_prompt` records history
        // AND sends — no second forward).
        let _ = self.store.submit_prompt(key);
    }

    /// The empty-enter steering (v2 `submitInner`, prompt/index.tsx:
    /// 1120-1121 + the session route's `onEmptySubmit`,
    /// index.tsx:1504-1508): an empty submit with queued prompts
    /// promotes the head — `mutatePending("steer")` locally.
    fn steer_queued_head(&mut self) -> bool {
        let Route::Session { session_id } = self.store.route() else { return false };
        let head = self.store.queued_prompts(&session_id).first().cloned();
        let Some(head) = head else { return false };
        self.store.delete_queued_prompt(&session_id, &head.id);
        let prompt = crate::components::composer::SubmittedPrompt {
            text: head.text,
            ..crate::components::composer::SubmittedPrompt::default()
        };
        self.store.forward_submission(&session_id, &prompt);
        true
    }

    /// Post-`handle_key` routing of a consumed composer outcome: a
    /// submit forwards to the engine sink; a slash-entry COMMAND run
    /// (v2 `onSelect: command.run`) dispatches through the command
    /// seam — `/help` opens the help dialog, `/exit` exits, `/editor`
    /// opens $EDITOR — never a prompt submission.
    fn handle_composer_outcome(&mut self, key: &str, outcome: KeyOutcome) {
        match outcome {
            KeyOutcome::Submitted(prompt) => {
                self.store.forward_submission(key, &prompt);
            }
            KeyOutcome::Command(command) => {
                let dispatch = self.dispatcher.dispatch(&self.store, command, None);
                if command == "app.exit" && dispatch != Dispatch::Unknown {
                    self.exit_requested = true;
                }
                if let Dispatch::ServerPending(reason) = dispatch {
                    let _ = reason;
                }
            }
            KeyOutcome::Edited | KeyOutcome::Ignored | KeyOutcome::Paste(_) => {}
        }
    }

    fn run_composer_command(&mut self, key: &str, command: &str) {
        // The composer-level specials (submit/clear/paste).
        if let Some(key_event) = composer_special(command) {
            let mut composer = self.store.composer(key);
            let outcome = composer.handle_key(&key_event);
            if outcome != KeyOutcome::Ignored {
                self.store.set_composer(key, &composer);
                self.handle_composer_outcome(key, outcome);
                return;
            }
            // An empty submit steers the queued head (v2
            // `onEmptySubmit`).
            if command == "input.submit" && self.steer_queued_head() {
                return;
            }
            return;
        }
        // Every `input.*` binding: the text-edit core's arm
        // (selection, word motions, delete ops, undo/redo).
        if let Some(edit) = composer_command(command) {
            let mut composer = self.store.composer(key);
            if composer.apply_edit(edit) {
                self.store.set_composer(key, &composer);
            }
        }
    }

    fn request_exit(&mut self) {
        self.exit_requested = true;
        self.store.clear_dialogs();
    }

    /// Keep the keymap's mode stack aligned: `dialog` while dialogs are
    /// open (the reference's `modal` mode), `input` below it while a
    /// composer holds the route focus (the managed textarea layer),
    /// `composer` while the subagent picker replaces the prompt (v2's
    /// `keymap.mode.push("composer")` — composer/index.tsx:59-63).
    /// The stack is recomputed whole each stroke (LIFO push/pop ordering
    /// hazards when a dialog opens over the input mode).
    fn sync_modes(&mut self) {
        let dialog = !self.store.dialogs().is_empty();
        let input = !dialog && !self.permission_live();
        let picker = input && self.picker_live();
        let mut modes: Vec<&str> = Vec::new();
        if input {
            modes.push("input");
        }
        if picker {
            modes.push("composer");
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
        // opentui re-dispatches strip hover from the LAST pointer
        // position on every re-render that moves controls under it
        // (the multi-session census: the ✕ stays revealed on the tab
        // that replaced the promoted slot after a submit opened it).
        // The store setters are signal-backed (`&self`) — the paint
        // path can self-heal the hover exactly like the render tree.
        self.rehover_strip_from_last_pointer();
        let theme = self.store.theme();
        let mut frame = render_app(&self.store, &theme, self.size);
        let footer_theme = FooterTheme::from_resolved(&theme.resolved.tokens);
        if let FooterView::Permission { request } = self.store.footer().view.clone() {
            let state = self
                .permission_states
                .get(&request.id)
                .cloned()
                .unwrap_or_else(|| create_permission_body_state(&request));
            let mount_height = PERMISSION_MOUNT_HEIGHT.min(usize::from(self.size.1));
            let width = usize::from(self.size.0);
            let body = crate::components::footer::permission::render_permission_body(
                &request,
                &state,
                width,
                mount_height,
                &footer_theme,
            );
            let y = usize::from(self.size.1) - mount_height;
            paste_frame(&mut frame, &body, y);
            // The pointer surface (auditA #4): the same layout math
            // the controls row paints with, translated by the mount
            // origin — the body mounts full-width (x is already
            // absolute), the controls row sits at screen row
            // `size.1 − 2`.
            let mut hits =
                crate::components::footer::permission::permission_pointer_hits(
                    &request,
                    &state,
                    width,
                    mount_height,
                );
            hits.row = usize::from(self.size.1).saturating_sub(2);
            self.store.set_permission_pointer_hits(Some(hits));
        } else {
            // No live body — no pointer targets (the else branch also
            // owns the picker mount below; the ask's rects must not
            // outlive the view).
            self.store.set_permission_pointer_hits(None);
            // A footer picker panel mounts over the composer from LIVE
            // store data (the mini footer family's docked pickers).
            let footer = self.store.footer();
            if matches!(footer.view, FooterView::Prompt) && footer.route.is_panel() {
                let mount_height = FOOTER_PANEL_MOUNT_HEIGHT.min(usize::from(self.size.1));
                if let Some(panel) = footer_panel_frame(
                    &self.store,
                    footer.route,
                    usize::from(self.size.0),
                    mount_height,
                    &footer_theme,
                ) {
                    let y = usize::from(self.size.1) - mount_height;
                    paste_frame(&mut frame, &panel, y);
                }
            }
        }
        // The toast layer (`ui/toast.tsx`): the queue's current toast
        // composites over whatever rendered underneath — rails keep the
        // underlying cell background (overlay_toast_over_frame).
        if let Some(toast) = self.store.toasts().current().cloned() {
            let pending = self.store.toasts().pending();
            crate::components::toast::overlay_toast_over_frame(
                &mut frame,
                &toast,
                pending,
                &footer_theme,
                &theme.resolved.tokens,
            );
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

/// Paste `region`'s rows at absolute y (right-clamped to the frame).
fn paste_frame(target: &mut Frame, region: &Frame, y: usize) {
    for (row, cells) in region.cells.iter().enumerate() {
        let Some(slot) = target.cells.get_mut(y + row) else { break };
        let count = cells.len().min(slot.len());
        slot[..count].clone_from_slice(&cells[..count]);
    }
}

/// The live footer panel for one picker route — the mini footer
/// family's docked pickers (`footer.command.tsx`) fed from the
/// store's server lists (model/agent/variant) and settings state:
///
/// * **model**: provider-grouped `model.list` rows, the active
///   session's selection as `current`, "Models loading" /
///   "No results found" empty states (`footer-model-select`);
/// * **agent**: non-hidden non-subagent `agent.list` rows;
/// * **variant**: "Default" + the active model's variants;
/// * **command/settings**: the static entry lists (the golden
///   fixtures' data source).
fn footer_panel_frame(
    store: &Store,
    route: crate::components::footer::PromptRoute,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Option<Frame> {
    use crate::components::footer::panels as footer_panels;
    use crate::components::footer::PromptRoute;

    // The selection the panel marks `current`: the active session's
    // RESOLVED selection (the chain's fallbacks included), or the
    // home route's staged next-session selection.
    let active = match store.route() {
        crate::store::Route::Session { session_id } => session_id,
        crate::store::Route::Home => crate::app::HOME_PROMPT_KEY.to_owned(),
    };
    let session_selection = store.effective_model(&active);
    let session_agent = store.selection_for(&active).agent;

    match route {
        PromptRoute::Composer => None,
        PromptRoute::Model => Some(model_panel_frame(
            &store.server_lists(),
            session_selection
                .as_ref()
                .map(|model| (model.provider_id.as_str(), model.model_id.as_str())),
            width,
            height,
            theme,
        )),
        PromptRoute::Agent => {
            let agents: Vec<footer_panels::RunAgent> = store
                .server_lists()
                .agents
                .rows()
                .map(|rows| {
                    rows.iter()
                        .map(|agent| footer_panels::RunAgent {
                            id: agent.id.clone(),
                            name: agent.name.clone(),
                            description: agent.description.clone(),
                            mode: agent.mode.clone(),
                            hidden: agent.hidden,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let current = session_agent;
            Some(footer_panels::render_agent_select(
                &agents,
                current.as_deref(),
                width,
                height,
                theme,
            ))
        }
        PromptRoute::Variant => {
            // The active model's variant list (Default + variants).
            let variants = store
                .server_lists()
                .models
                .rows()
                .and_then(|models| {
                    session_selection.as_ref().and_then(|selection| {
                        models
                            .iter()
                            .find(|model| {
                                model.provider_id == selection.provider_id
                                    && model.id == selection.model_id
                            })
                            .map(|model| model.variants.clone())
                    })
                })
                .unwrap_or_default();
            let current = session_selection.as_ref().and_then(|m| m.variant.clone());
            Some(footer_panels::render_variant_select(
                &variants,
                current.as_deref(),
                width,
                height,
                theme,
            ))
        }
        PromptRoute::Command => Some(footer_panels::render_command_menu(width, height, theme)),
        PromptRoute::Settings => Some(footer_panels::render_settings(
            &footer_panels::resolve_mini_settings(),
            width,
            height,
            theme,
        )),
    }
}

/// The model picker panel from the server-list state: provider-
/// grouped rows (provider.list names; orphan models keep a synthetic
/// provider row), the current selection revealed; loading/failed
/// render the shell with its loading label ("Models loading" — the
/// mini footer's empty state).
fn model_panel_frame(
    lists: &crate::store::ServerLists,
    current: Option<(&str, &str)>,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    use crate::components::footer::panels as footer_panels;
    let Some(models) = lists.models.rows() else {
        // Loading / failed — the empty-list mount still renders the
        // shell with its loading label.
        return footer_panels::render_model_select(&[], None, width, height, theme);
    };
    let mut providers: Vec<crate::store::ServerProvider> = lists.providers.clone();
    providers.sort_by(|a, b| a.name.cmp(&b.name));
    let mut by_provider: HashMap<String, Vec<footer_panels::RunModel>> = HashMap::new();
    for model in models {
        by_provider.entry(model.provider_id.clone()).or_default().push(footer_panels::RunModel {
            id: model.id.clone(),
            name: model.name.clone(),
            cost_input: model.cost_input,
            deprecated: model.deprecated,
        });
    }
    // Orphan models (no provider row) get a synthetic provider entry.
    for id in by_provider.keys().cloned().collect::<Vec<_>>() {
        if !providers.iter().any(|provider| provider.id == id) {
            providers.push(crate::store::ServerProvider { id: id.clone(), name: id });
        }
    }
    providers.retain(|provider| by_provider.contains_key(&provider.id));
    let run_providers: Vec<footer_panels::RunProvider> = providers
        .into_iter()
        .map(|provider| footer_panels::RunProvider {
            id: provider.id.clone(),
            name: provider.name,
            models: by_provider.remove(&provider.id).unwrap_or_default(),
        })
        .collect();
    footer_panels::render_model_select(&run_providers, current, width, height, theme)
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
        "up" if shift => Some(ComposerKey::ShiftUp),
        "up" => Some(ComposerKey::Up),
        "down" if shift => Some(ComposerKey::ShiftDown),
        "down" => Some(ComposerKey::Down),
        "left" if shift => Some(ComposerKey::ShiftLeft),
        "left" => Some(ComposerKey::Left),
        "right" if shift => Some(ComposerKey::ShiftRight),
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

/// Map an `input.*` command onto the text-edit core's vocabulary
/// (the v2 table's input layer — keybind.ts:206-243; every `input.*`
/// binding has a real arm through [`ComposerEditCommand::parse`]).
fn composer_command(command: &str) -> Option<ComposerEditCommand> {
    ComposerEditCommand::parse(command)
}

/// The composer commands handled OUTSIDE the text-edit core (their
/// effects are composer-level: submit/clear/paste — see
/// [`run_composer_command`]).
fn composer_special(command: &str) -> Option<ComposerKey> {
    match command {
        "input.submit" | "prompt.submit" => Some(ComposerKey::Enter),
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
    /// A send FAILED at the transport layer (server 5xx, connection
    /// refused): v2 surfaces the error as an error toast and restores
    /// the composer draft (`fail()` in prompt/index.tsx:1194-1198 —
    /// the toast carries a title like "Failed to send prompt" /
    /// "Creating a session failed" and `restoreEntry()` puts the text
    /// back when the user has not started typing something new).
    SendFailed {
        /// The composer key the failed send came from.
        session_key: String,
        title: String,
        message: String,
        /// The submitted text (the restore candidate).
        draft: String,
    },
}

/// Server-list fetch results (the pickers' data side).
#[derive(Debug, Clone)]
pub enum ServerListEvent {
    /// The config `model` default from the server's config API — v2's
    /// `configuredModel` (`context/local.tsx:170-179`: the LAST config
    /// document entry carrying a `model` field; string `provider/model`
    /// or the `{providerID, model, variant}` object). `None` = no
    /// configured default (the chain falls through to recent/catalog).
    Config(Option<crate::store::ModelSelection>),
    Models(Result<(Vec<crate::store::ServerModel>, Vec<crate::store::ServerProvider>), String>),
    Agents(Result<Vec<crate::store::ServerAgent>, String>),
    /// `useConnected()` — any integration has connections.
    Connected(bool),
}

/// One `worktree.list` call mapped into the dialog's row type.
async fn fetch_worktree_list(
    client: &oc_client::Client,
    project_id: &str,
) -> Result<Vec<crate::components::dialog_worktrees::WorktreeEntry>, String> {
    client
        .worktree()
        .list(&oc_client::types::WorktreeListQuery { project_id: project_id.to_owned() })
        .await
        .map(|directories| {
            directories
                .iter()
                .map(|entry| crate::components::dialog_worktrees::WorktreeEntry {
                    directory: entry.directory.clone(),
                    strategy: entry.strategy.clone(),
                })
                .collect()
        })
        .map_err(|error| error.to_string())
}

/// The worktree worker's requests (the sink → the spawned task).
#[derive(Debug)]
enum WorktreeRequest {
    List(String),
    Refresh(String),
    Create { project_id: String, name: String },
    Remove { project_id: String, directory: String, force: bool },
    MoveSession { session_id: String, directory: String },
}

/// The worktree worker's events (the spawned task → the pump → the
/// store's worktree setters).
#[derive(Debug)]
pub enum WorktreeEvent {
    /// Boot project resolution (`projectID` + the current checkout).
    Resolved { project_id: String, current: Option<String> },
    /// A `worktree.list` result.
    Listed(Result<Vec<crate::components::dialog_worktrees::WorktreeEntry>, String>),
    /// A `worktree.create` result (the new directory).
    Created(Result<String, String>),
    /// A `worktree.remove` result; the error pair carries
    /// (message, forceRequired).
    RemoveResult { directory: String, result: Result<(), (String, bool)> },
}

/// The Worktrees dialog's production seam: a spawned worker owning
/// the oc-client calls (the dialog actions send requests; results
/// arrive as [`WorktreeEvent`]s the pump applies to the store —
/// mirroring v2's SSE `worktree.updated` refetch flow).
fn spawn_worktree_worker(
    client: oc_client::Client,
    cwd: String,
) -> (tokio::sync::mpsc::Sender<WorktreeRequest>, tokio::sync::mpsc::Receiver<WorktreeEvent>) {
    let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<WorktreeRequest>(16);
    let (tx, rx) = tokio::sync::mpsc::channel::<WorktreeEvent>(16);
    tokio::spawn(async move {
        // Boot: resolve the project (v2 goes through the location
        // service; the port matches the cwd beneath a project
        // `canonical`, else the first project).
        let project = client.project().list().await.ok().and_then(|projects| {
            projects
                .iter()
                .find(|project| {
                    crate::components::dialog_worktrees::contains(&project.canonical, &cwd)
                })
                .or_else(|| projects.first())
                .cloned()
        });
        let Some(project) = project else { return };
        let _ = tx
            .send(WorktreeEvent::Resolved {
                project_id: project.id.clone(),
                current: Some(project.canonical.clone()),
            })
            .await;
        let list = fetch_worktree_list(&client, &project.id).await;
        let _ = tx.send(WorktreeEvent::Listed(list)).await;

        while let Some(request) = req_rx.recv().await {
            match request {
                WorktreeRequest::List(project_id) => {
                    let list = fetch_worktree_list(&client, &project_id).await;
                    let _ = tx.send(WorktreeEvent::Listed(list)).await;
                }
                WorktreeRequest::Refresh(project_id) => {
                    let _ = client
                        .worktree()
                        .refresh(&oc_client::types::WorktreeRefreshBody {
                            project_id: project_id.clone(),
                        })
                        .await;
                    let list = fetch_worktree_list(&client, &project_id).await;
                    let _ = tx.send(WorktreeEvent::Listed(list)).await;
                }
                WorktreeRequest::Create { project_id, name } => {
                    let result = client
                        .worktree()
                        .create(&oc_client::types::WorktreeCreateInput {
                            project_id,
                            name: Some(name),
                            ..Default::default()
                        })
                        .await
                        .map(|info| info.directory)
                        .map_err(|error| error.to_string());
                    let _ = tx.send(WorktreeEvent::Created(result)).await;
                }
                WorktreeRequest::Remove { project_id, directory, force } => {
                    let result = client
                        .worktree()
                        .remove(&oc_client::types::WorktreeRemoveInput {
                            project_id,
                            directory: directory.clone(),
                            force,
                        })
                        .await
                        .map_err(|error| {
                            let force_required = match &error {
                                oc_client::ClientError::Api { fields, .. } => {
                                    fields.get("forceRequired").and_then(serde_json::Value::as_bool)
                                        == Some(true)
                                        || fields
                                            .get("data")
                                            .and_then(|data| data.get("forceRequired"))
                                            .and_then(serde_json::Value::as_bool)
                                            == Some(true)
                                }
                                _ => false,
                            };
                            (error.to_string(), force_required)
                        });
                    let _ = tx.send(WorktreeEvent::RemoveResult { directory, result }).await;
                }
                WorktreeRequest::MoveSession { session_id, directory } => {
                    // `session.move` POST — failures log (v2 toasts
                    // through `catch(toast.error)`; the port's move
                    // result needs no dialog state).
                    if let Err(error) = client
                        .session()
                        .move_(
                            &session_id,
                            &oc_client::types::SessionMoveBody {
                                directory: directory.clone(),
                                ..Default::default()
                            },
                        )
                        .await
                    {
                        crate::keymap::log_skip(&format!("session move: {error}"));
                    }
                }
            }
        }
    });
    (req_tx, rx)
}

/// The production worktree sink: dialog actions become worker
/// requests (fire-and-forget; results arrive as events).
struct HttpWorktreeSink {
    tx: tokio::sync::mpsc::Sender<WorktreeRequest>,
}

impl crate::store::WorktreeSink for HttpWorktreeSink {
    fn list(&self, project_id: &str) {
        let _ = self.tx.blocking_send(WorktreeRequest::List(project_id.to_owned()));
    }

    fn refresh(&self, project_id: &str) {
        let _ = self.tx.blocking_send(WorktreeRequest::Refresh(project_id.to_owned()));
    }

    fn create(&self, project_id: &str, name: &str) {
        let _ = self.tx.blocking_send(WorktreeRequest::Create {
            project_id: project_id.to_owned(),
            name: name.to_owned(),
        });
    }

    fn remove(&self, project_id: &str, directory: &str, force: bool) {
        let _ = self.tx.blocking_send(WorktreeRequest::Remove {
            project_id: project_id.to_owned(),
            directory: directory.to_owned(),
            force,
        });
    }

    fn move_session(&self, session_id: &str, directory: &str) {
        let _ = self.tx.blocking_send(WorktreeRequest::MoveSession {
            session_id: session_id.to_owned(),
            directory: directory.to_owned(),
        });
    }
}

/// Fetch the picker lists once against the resolved server
/// (`model.list` + `provider.list` + `agent.list`; failures land as
/// the picker's error state, never a crash).
fn configured_model_from_entries(
    entries: &[oc_client::types::ConfigEntry],
) -> Option<crate::store::ModelSelection> {
    let model = entries.iter().rev().find_map(|entry| match entry {
        oc_client::types::ConfigEntry::DocumentEncoded(document) => document.info.model.as_ref(),
        oc_client::types::ConfigEntry::DirectoryEncoded(_) => None,
    })?;
    match model {
        oc_client::types::ConfigInfoEncodedModel::String(raw) => {
            let (provider_id, model_id) = raw.split_once('/')?;
            let (provider_id, model_id) = (provider_id.trim(), model_id.trim());
            (!provider_id.is_empty() && !model_id.is_empty()).then(|| {
                crate::store::ModelSelection {
                    provider_id: provider_id.to_owned(),
                    model_id: model_id.to_owned(),
                    variant: None,
                }
            })
        }
        oc_client::types::ConfigInfoEncodedModel::ConfigInfoEncodedModelVariant2(object) => {
            let provider_id = object.provider_id.trim();
            let model_id = object.model.trim();
            (!provider_id.is_empty() && !model_id.is_empty()).then(|| {
                crate::store::ModelSelection {
                    provider_id: provider_id.to_owned(),
                    model_id: model_id.to_owned(),
                    variant: object.variant.clone(),
                }
            })
        }
    }
}

#[must_use]
pub fn spawn_server_lists(
    client: oc_client::Client,
) -> (tokio::sync::mpsc::Receiver<ServerListEvent>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let task = tokio::spawn(async move {
        let to_string = |error: oc_client::ClientError| error.to_string();
        let config = client
            .config()
            .get(&oc_client::types::ConfigGetQuery { location: None })
            .await
            .ok()
            .and_then(|entries| configured_model_from_entries(&entries));
        let _ = tx.send(ServerListEvent::Config(config)).await;
        let models = client
            .model()
            .list(&oc_client::types::ModelListQuery { location: None })
            .await
            .map_err(to_string);
        let providers = client
            .provider()
            .list(&oc_client::types::ProviderListQuery { location: None })
            .await
            .map_err(|_| ());
        let agents = client
            .agent()
            .list(&oc_client::types::AgentListQuery { location: None })
            .await
            .map_err(|error| error.to_string());
        let providers = providers.unwrap_or_default().data;
        // `useConnected()` — `hasConnectedProvider(integration.list())`
        // (use-connected.tsx): any integration carrying a connection
        // flips the models dialog to its connected rendering and drops
        // the palette's suggested "Connect an integration" row.
        let connected = client
            .integration()
            .list(&oc_client::types::IntegrationListQuery { location: None })
            .await
            .is_ok_and(|response| {
                response.data.iter().any(|integration| !integration.connections.is_empty())
            });
        let _ = tx.send(ServerListEvent::Connected(connected)).await;
        let _ = tx
            .send(ServerListEvent::Models(models.map(|response| {
                let models = response
                    .data
                    .iter()
                    .map(|model| crate::store::ServerModel {
                        provider_id: model.provider_id.clone(),
                        id: model.id.clone(),
                        name: model.name.clone(),
                        // `free` = a non-empty cost table whose inputs
                        // are all zero (dialog-model's `free()`).
                        cost_input: model.cost.first().map_or(0.0, |cost| cost.input),
                        free: !model.cost.is_empty() && model.cost.iter().all(|c| c.input == 0.0),
                        deprecated: model.status == oc_client::types::ModelInfoStatus::Deprecated,
                        variants: model.variants.iter().map(|variant| variant.id.clone()).collect(),
                        released: model.time.released,
                        context_limit: (model.limit.context > 0).then_some(model.limit.context),
                    })
                    .collect();
                let providers = providers
                    .iter()
                    .map(|provider| crate::store::ServerProvider {
                        id: provider.id.clone(),
                        name: provider.name.clone(),
                    })
                    .collect::<Vec<_>>();
                (models, providers)
            })))
            .await;
        let _ = tx
            .send(ServerListEvent::Agents(agents.map(|response| {
                response
                    .data
                    .iter()
                    .map(|agent| crate::store::ServerAgent {
                        id: agent.id.clone(),
                        name: agent.name.clone(),
                        description: agent.description.clone(),
                        mode: agent.mode.as_str().to_owned(),
                        hidden: agent.hidden,
                    })
                    .collect()
            })))
            .await;
    });
    (rx, task)
}

/// The production selection sink: model/agent picks POST
/// `session.switchModel` / `session.switchAgent` fire-and-forget (the
/// reference commits selections the same way — failures log, the
/// store keeps the optimistic selection).
pub struct HttpSelectionSink {
    client: oc_client::Client,
}

impl HttpSelectionSink {
    #[must_use]
    pub fn new(client: oc_client::Client) -> Self {
        Self { client }
    }
}

impl crate::store::SelectionSink for HttpSelectionSink {
    fn switch_model(&self, session_id: &str, model: &crate::store::ModelSelection) {
        let client = self.client.clone();
        let session_id = session_id.to_owned();
        let body = oc_client::types::SessionSwitchModelBody {
            model: oc_client::types::ModelRef {
                id: model.model_id.clone(),
                provider_id: model.provider_id.clone(),
                variant: model.variant.clone(),
            },
        };
        tokio::spawn(async move {
            if let Err(error) = client.session().switch_model(&session_id, &body).await {
                crate::keymap::log_skip(&format!("switch model: {error}"));
            }
        });
    }

    fn switch_agent(&self, session_id: &str, agent: &str) {
        let client = self.client.clone();
        let session_id = session_id.to_owned();
        let body = oc_client::types::SessionSwitchAgentBody { agent: agent.to_owned() };
        tokio::spawn(async move {
            if let Err(error) = client.session().switch_agent(&session_id, &body).await {
                crate::keymap::log_skip(&format!("switch agent: {error}"));
            }
        });
    }
}

/// Map the pump's [`PermissionReply`] onto the generated client body
/// (the `permission.reply` POST — D1's transport).
#[must_use]
pub fn permission_reply_body(
    reply: &PermissionReply,
) -> oc_client::types::SessionPermissionReplyBody {
    let decision = match reply.decision.as_str() {
        "always" => oc_client::types::PermissionReply::Always,
        "reject" => oc_client::types::PermissionReply::Reject,
        _ => oc_client::types::PermissionReply::Once,
    };
    oc_client::types::SessionPermissionReplyBody {
        decision,
        message: (!reply.message.is_empty()).then(|| reply.message.clone()),
    }
}

/// POST one recorded permission reply through the generated client
/// (`session.permission.reply`); failures log (the local ask already
/// resolved — the server parks the tool until the reply or timeout).
pub fn post_permission_reply(client: oc_client::Client, reply: PermissionReply) {
    tokio::spawn(async move {
        let body = permission_reply_body(&reply);
        if let Err(error) =
            client.session().permission_reply(&reply.session_id, &reply.request_id, &body).await
        {
            crate::keymap::log_skip(&format!(
                "permission reply {}/{}: {error}",
                reply.session_id, reply.request_id
            ));
        }
    });
}

/// The rename submit's `session.update` POST (v2
/// routes/session/index.tsx:908: `client.api.session.update({
/// sessionID, title })`). The `session.renamed` echo — not this
/// response — lands the new title across the TUI (the live session
/// registry refreshes the tab label, sidebar, and session-list rows).
pub fn post_session_rename(client: oc_client::Client, session_id: String, title: String) {
    tokio::spawn(async move {
        let body = oc_client::types::SessionUpdateBody {
            title: Some(title),
            ..oc_client::types::SessionUpdateBody::default()
        };
        if let Err(error) = client.session().update(&session_id, &body).await {
            crate::keymap::log_skip(&format!("session rename {session_id}: {error}"));
        }
    });
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
        self.send_with_model(session_id, prompt, None);
    }

    /// The v2 submit order (`commitSelection` → prompt,
    /// prompt/index.tsx:1290-1294): the resolved model — when one
    /// exists — commits through `session.switchModel` BEFORE the
    /// prompt POST, inside the same task, so the turn runs on the
    /// committed model (the F8/F10/F22 dead-end fix: a session with no
    /// stored model gets the chain's resolution committed instead of
    /// failing with "No model is available").
    fn send_with_model(
        &self,
        session_id: &str,
        prompt: &crate::components::composer::SubmittedPrompt,
        model: Option<&crate::store::ModelSelection>,
    ) {
        let client = self.client.clone();
        let events = self.events.clone();
        let directory = self.directory.clone();
        let text = prompt.text.clone();
        let model = model.cloned();
        // R-07: staged attachment chips ride as `PromptInput.FileAttachment`
        // data URIs (`files`), the wire shape the server materializes.
        let files: Vec<oc_client::types::PromptInputFileAttachment> = prompt
            .files
            .iter()
            .filter_map(|file| {
                let payload = file.payload.as_ref()?;
                Some(oc_client::types::PromptInputFileAttachment {
                    uri: payload.to_data_uri(),
                    name: Some(file.name.clone()),
                    description: None,
                    mention: Some(oc_client::types::PromptMention {
                        start: u32::try_from(file.start).map_or(f64::MAX, f64::from),
                        end: u32::try_from(file.end).map_or(f64::MAX, f64::from),
                        text: file.mention_text(),
                    }),
                })
            })
            .collect();
        let session_key = session_id.to_owned();
        tokio::spawn(async move {
            let target = if session_key == HOME_PROMPT_KEY {
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
                        // v2's `newSession.recover`: "Creating a session
                        // failed" + the draft back in the composer.
                        let _ = events.send(PromptSinkEvent::SendFailed {
                            session_key: session_key.clone(),
                            title: "Creating a session failed".to_owned(),
                            message: client_error_message(&error),
                            draft: text,
                        });
                        return;
                    }
                }
            } else {
                session_key.clone()
            };
            // `commitModel` (prompt/index.tsx:1283-1294): the resolved
            // selection commits before the prompt fires; a failed
            // switch surfaces as the send failure (v2's
            // "Failed to switch model" wrap keeps going — the prompt
            // still attempts, matching the reference's catch-less path
            // only when no model resolved).
            if let Some(model) = model {
                let body = oc_client::types::SessionSwitchModelBody {
                    model: oc_client::types::ModelRef {
                        id: model.model_id.clone(),
                        provider_id: model.provider_id.clone(),
                        variant: model.variant.clone(),
                    },
                };
                if let Err(error) = client.session().switch_model(&target, &body).await {
                    let _ = events.send(PromptSinkEvent::SendFailed {
                        session_key,
                        title: "Failed to switch model".to_owned(),
                        message: client_error_message(&error),
                        draft: text,
                    });
                    return;
                }
            }
            let body = oc_client::types::SessionPromptBody {
                text,
                files: (!files.is_empty()).then_some(files),
                ..oc_client::types::SessionPromptBody::default()
            };
            if let Err(error) = client.session().prompt(&target, &body).await {
                // v2's `fail("Failed to send prompt", error)`: the
                // error toast + `restoreEntry()`.
                let _ = events.send(PromptSinkEvent::SendFailed {
                    session_key,
                    title: "Failed to send prompt".to_owned(),
                    message: client_error_message(&error),
                    draft: body.text,
                });
            }
        });
    }
}

/// `errorMessage` (util/error.ts): the client error's message text.
fn client_error_message(error: &oc_client::ClientError) -> String {
    error.to_string()
}

/// Apply one [`PromptSinkEvent::SendFailed`] to the store: the error
/// toast (`fail()`'s `toast.show({ title, message, variant: "error" })`)
/// plus `restoreEntry()` — the submitted text returns to the composer
/// ONLY when the user has not started typing something new.
pub fn apply_send_failure(store: &Store, event: PromptSinkEvent) {
    let PromptSinkEvent::SendFailed { session_key, title, message, draft } = event else {
        return;
    };
    store.toast_show(crate::components::toast::ToastOptions {
        title: Some(title),
        message,
        variant: "error",
        duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
        action_label: None,
    });
    if store.prompt_draft(&session_key).text.is_empty() && !draft.is_empty() {
        let mut composer = store.composer(&session_key);
        composer
            .set(crate::components::composer::PromptDraft { text: draft, ..Default::default() });
        store.set_composer(&session_key, &composer);
    }
}

/// The production interrupt sink: the 2-press machine's fire path
/// POSTs `session.interrupt` with `resume: true` (prompt/index.tsx:
/// 522-525).
pub struct HttpInterruptSink {
    client: oc_client::Client,
}

impl HttpInterruptSink {
    #[must_use]
    pub fn new(client: oc_client::Client) -> Self {
        Self { client }
    }
}

impl crate::store::InterruptSink for HttpInterruptSink {
    fn interrupt(&self, session_id: &str) {
        let client = self.client.clone();
        let session_id = session_id.to_owned();
        tokio::spawn(async move {
            let query = oc_client::types::SessionInterruptQuery {
                resume: Some(oc_client::types::SessionInterruptQueryResume::True),
            };
            if let Err(error) = client.session().interrupt(&session_id, &query).await {
                // v2 surfaces the failure as an error toast.
                eprintln!("opencode tui: interrupt failed: {error}");
            }
        });
    }
}

/// The model-preference repository path (`<state>/tui/model.json` —
/// the mini host's `preferences` seam, mini-host.ts:18-30; the main
/// TUI shares the document).
#[must_use]
pub fn model_preference_repository() -> Option<crate::mini::model_pref::ModelPreferenceRepository> {
    let roots = oc_util::global_roots::resolve();
    let state = roots.data.join("tui");
    Some(crate::mini::model_pref::host_preferences(&state))
}

/// The run loop's on-demand fetch requests (the dogfood wave-1 data
/// side): transcript hydration for newly opened sessions (F12) and the
/// session-list refresh the /sessions dialog fires on open (F13).
#[derive(Debug)]
pub enum LoopRequest {
    /// `message.list` for one session (chronological — see
    /// [`crate::data::fetch_session_messages`]).
    Transcript(String),
    /// `session.list` (the dialog-open refetch).
    SessionList,
    /// `vcs.get` for one directory (the footer's branch lookup).
    LocationVcs(String),
}

/// One resolved [`LoopRequest`].
#[derive(Debug)]
pub enum LoopFetch {
    Transcript {
        session_id: String,
        result: Result<Vec<crate::components::message::model::Message>, String>,
    },
    SessionList(Result<Vec<crate::data::SessionSnapshot>, String>),
    LocationVcs {
        directory: String,
        branch: Result<Option<String>, String>,
    },
}

/// The fetch worker: requests spawn per-fetch tasks (no head-of-line
/// blocking); results arrive on one ordered channel the run loop
/// drains into [`Store::ingest_all`].
#[must_use]
pub fn spawn_loop_fetcher(
    client: oc_client::Client,
) -> (
    tokio::sync::mpsc::UnboundedSender<LoopRequest>,
    tokio::sync::mpsc::UnboundedReceiver<LoopFetch>,
) {
    let (req_tx, mut req_rx) = tokio::sync::mpsc::unbounded_channel::<LoopRequest>();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<LoopFetch>();
    tokio::spawn(async move {
        while let Some(request) = req_rx.recv().await {
            match request {
                LoopRequest::Transcript(session_id) => {
                    let client = client.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let result =
                            crate::data::fetch_session_messages(&client, &session_id).await;
                        let _ = tx.send(LoopFetch::Transcript { session_id, result });
                    });
                }
                LoopRequest::SessionList => {
                    let client = client.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let fetcher = HttpResync::new(client);
                        let result = crate::data::ResyncSource::fetch_sessions(&fetcher).await;
                        let _ = tx.send(LoopFetch::SessionList(result));
                    });
                }
                LoopRequest::LocationVcs(directory) => {
                    let client = client.clone();
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let branch = client
                            .vcs()
                            .get(&oc_client::types::VcsGetQuery {
                                location: Some(oc_client::types::QueryLocation {
                                    directory: Some(directory.clone()),
                                }),
                            })
                            .await
                            .map(|response| {
                                response.data.branch.current.filter(|branch| !branch.is_empty())
                            })
                            .map_err(|error| error.to_string());
                        let _ = tx.send(LoopFetch::LocationVcs { directory, branch });
                    });
                }
            }
        }
    });
    (req_tx, rx)
}

/// The F13 production seam: the session-list dialog's open fires a
/// `session.list` refetch through the loop fetcher (v2's
/// `DialogSessionList` syncs on mount — the boot resync snapshot alone
/// goes stale as sessions are created).
struct HttpSessionListSink {
    tx: tokio::sync::mpsc::UnboundedSender<LoopRequest>,
}

impl crate::store::SessionListSink for HttpSessionListSink {
    fn refresh(&self) {
        let _ = self.tx.send(LoopRequest::SessionList);
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

/// The TUI's boot flags (v2 `input.args`): the route rules of
/// `app.tsx:364-371` + `639-674` — the default boot is the HOME
/// route; `--session <id>` routes that session; `--continue` resolves
/// the directory's latest root session and routes it. A restored tab
/// strip never moves the boot route.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootArgs {
    /// `--continue` / `-c`: open the most recent session for the
    /// current directory (v2 resolves its `sessionID: "dummy"`
    /// placeholder through `session.list` limit 1, order desc,
    /// `parentID: null`, `directory: cwd`).
    pub continue_last: bool,
    /// `--session <id>` / `-s`: open this session.
    pub session: Option<String>,
    /// `--prompt <text>` (v2 `input.args.prompt`): the startup prompt
    /// — seeds the booted route's composer and auto-submits once on
    /// boot (app.tsx:626's `startupPrompt`; home.tsx:52-80 and
    /// routes/session/index.tsx:377-381+601-607 both submit it as the
    /// first turn).
    pub prompt: Option<String>,
    /// `--auto` (with `--yolo`/`--dangerously-skip-permissions`, v2's
    /// PermissionParams): arm autoaccept — every permission ask
    /// auto-replies `once` (session/index.tsx:255-262), the ask UI
    /// never mounts (permission.tsx:14's `args.auto ?
    /// "autoaccept"`).
    pub auto: bool,
}

/// Resolve the session `--continue` opens: the newest ROOT session in
/// the current directory (v2 `app.tsx:653-662` — `session.list`
/// `{limit: 1, order: "desc", parentID: null, directory}`).
async fn latest_session(client: &oc_client::Client, directory: &str) -> Option<String> {
    let query = oc_client::types::SessionListQuery {
        limit: Some("1".to_owned()),
        order: Some(oc_client::types::SessionListQueryOrder::Desc),
        // The wire's `parentID=null` filters to root sessions (the
        // generated query type carries the raw string).
        parent_id: Some("null".to_owned()),
        directory: Some(directory.to_owned()),
        ..oc_client::types::SessionListQuery::default()
    };
    let response = client.session().list(&query).await.ok()?;
    response.data.first().map(|session| session.id.clone())
}

/// Apply the boot route rules to a booted store (pure seam — the PTY
/// tests and the run loop share it).
fn apply_boot_route(store: &crate::store::Store, boot: &BootArgs, continue_target: Option<&str>) {
    if let Some(session_id) = &boot.session {
        // `--session`: route it (the route effect opens its tab).
        store.select_tab(session_id);
    } else if boot.continue_last {
        // `--continue`: route the resolved latest session; none found
        // stays home (v2: `if (!match) return`).
        if let Some(latest) = continue_target {
            store.select_tab(latest);
        }
    }
    // Default: home — the store's initial route. Never a restored tab.
}

/// The live run loop: terminal acquire/release, event drain, render.
/// Blocks until `app.exit`; returns the terminal to its prior state.
///
/// Errors clearly when stdin is not a TTY (the TUI needs a terminal).
pub async fn run(client: oc_client::Client, boot: BootArgs) -> Result<(), String> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(
            "the TUI requires an interactive terminal (stdin/stdout are not TTYs)".to_owned()
        );
    }
    let size = crossterm::terminal::size().ok();
    run_with_size(client, size, boot).await
}

/// The live mouse flip's writer (v2 app.tsx:593 —
/// `createEffect(() => renderer.useMouse = config.data.mouse)`):
/// push or pop the v2 mode set the moment a config reload toggles
/// `mouse`.
fn write_mouse_flip(mouse: bool) {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    if mouse {
        let _ = crate::mouse_modes::push(&mut out, true);
    } else {
        let _ = crate::mouse_modes::pop(&mut out, true);
    }
    let _ = out.flush();
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
    boot: BootArgs,
) -> Result<(), String> {
    use crossterm::event::Event as CrosstermEvent;
    use crossterm::event::KeyEventKind;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::backend::CrosstermBackend;

    // Terminal acquire (app.tsx's renderer acquire/release): raw mode +
    // alternate screen, restored on every exit path. Focus events feed
    // the attention module (the pending marker consumes on FocusGained).
    enable_raw_mode().map_err(|e| e.to_string())?;
    let _ = crossterm::execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        crossterm::event::EnableFocusChange,
        // Bracketed paste (R-07): pasted text (an image file's path,
        // typically) arrives as `Event::Paste` instead of raw keys —
        // the seam `pasteInputText`'s attachment resolution hangs off.
        crossterm::event::EnableBracketedPaste,
        // Mouse tracking (beta.8 bug 3/4): v2's renderer enables the
        // terminal's mouse reporting CONFIG-GATED (the strip's
        // hover/click and the reasoning rows' disclosure toggle are
        // mouse-driven, `session-tabs.tsx:1652-1754` +
        // `group-view.tsx:152-190`). The v2-parity mode set
        // (`?1000 ?1002 ?1003 ?1006` — no crossterm `?1015`) pushes
        // right after the config loads below (`crate::mouse_modes`).
    );
    // The alternate screen owns the display from here: route EVERY
    // stderr write to `<state>/tui.log` until teardown (subsystem
    // errors, plugin-host lines, the bun child's forwarded stderr —
    // none of it may splat over the frame; v2 never does). Panics
    // still reach the real stderr through the guard's saved fd.
    let mut stderr_guard = crate::stderr_redirect::install(&crate::stderr_redirect::log_path());
    // S0: one-shot terminal color-capability probe (TERM/COLORTERM/
    // SSH_*/WT_SESSION — v2 raster semantics; see render::color_mode).
    let color_mode = color_mode::detect_from_env();
    let mut color_cache = color_mode::ColorCache::default();
    let mut terminal = ratatui::Terminal::new(CrosstermBackend::new(std::io::stdout()))
        .map_err(|e| e.to_string())?;
    let size = size.unwrap_or((100, 30));
    let _ = terminal.resize(ratatui::layout::Rect::new(0, 0, size.0, size.1));

    // App + config boot.
    let store = Store::new(crate::reactive::Runtime::new());
    let mut app = App::with_store(store, size);
    // R-07: the terminal's graphics protocol (env probe — see
    // `crate::image` for why not a DA1 round-trip).
    app.store.set_graphics_protocol(crate::image::detect_protocol());
    let config = crate::config::TuiConfig::load(&oc_util::global_roots::resolve().config);

    // The mouse-capture gate (v2 `useMouse: config.mouse`,
    // app.tsx:241): push EXACTLY v2's four-mode set when on, nothing
    // when off — `mouse: false` leaves the terminal's native text
    // selection alone (auditC P1/P9). `applied_mouse` tracks the live
    // state for the editor suspend/re-enter pair, the reload flip,
    // and the teardown pop.
    let mut applied_mouse = config.mouse;
    {
        use std::io::Write as _;
        let mut out = std::io::stdout();
        let _ = crate::mouse_modes::push(&mut out, applied_mouse);
        let _ = out.flush();
    }

    // R-06 — the attention channels (v2's sounds + OS notifications):
    // the production emitter writes OSC notification sequences to
    // stdout and plays the embedded sound pack through the first
    // available system player; the built-in resolver extracts the
    // pack into the cache dir. The resolved config gates both
    // channels (both default OFF, like v2).
    {
        let roots = oc_util::global_roots::resolve();
        app.store.set_attention_emitter(std::rc::Rc::new(crate::sound::TerminalAttention {
            notifier: std::cell::RefCell::new(crate::os_notify::OscNotifier::from_env(
                std::io::stdout(),
            )),
            sounds: crate::sound::SoundHalf::default(),
        }));
        app.store.set_attention_builtin_sounds(crate::sound::builtin_sound_resolver(roots.cache));
        app.store.set_attention_config(config.attention.clone());
    }
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
    app.store.set_prompt_sink(std::rc::Rc::new(HttpPromptSink::new(
        client.clone(),
        directory.clone(),
        sink_tx,
    )));
    // Wave A: the interrupt sink (the 2-press escape machine's fire
    // path) + the model-preference ring boot (`<state>/tui/model.json`
    // — shared with the mini host's document).
    app.store.set_interrupt_sink(std::rc::Rc::new(HttpInterruptSink::new(client.clone())));
    if let Some(repository) = model_preference_repository() {
        let document = repository.read();
        app.store.set_model_ring(crate::store::ModelRing { document });
    }
    let model_preference_path = model_preference_repository();

    // Production sinks: clipboard writes OSC 52; model/agent picks POST
    // session.switchModel / session.switchAgent.
    app.store
        .set_clipboard_sink(std::rc::Rc::new(crate::clipboard::Osc52Sink::new(std::io::stdout())));
    app.store.set_selection_sink(std::rc::Rc::new(HttpSelectionSink::new(client.clone())));

    // Server-backed picker lists (model/provider/agent).
    let (mut list_rx, lists_task) = spawn_server_lists(client.clone());

    // The Worktrees dialog's worker (R-05): boot resolves the project
    // + list; dialog actions flow through the sink.
    let (worktree_tx, mut worktree_rx) = spawn_worktree_worker(
        client.clone(),
        std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default(),
    );
    app.store.set_worktree_sink(std::rc::Rc::new(HttpWorktreeSink { tx: worktree_tx }));

    // Tabs persistence (v2's useStorage contract — see storage.rs):
    // restore on boot, save on change. The open-tabs model restores
    // ONLY the configured scope (`config.tabs.scope`, default cwd —
    // `session-tabs.tsx:104-110`), a legacy all-history file resets
    // (read_tabs), and the restore NEVER moves the route: the boot
    // route is home unless `--continue`/`--session` says otherwise
    // (v2 app.tsx:364-371). The boot load runs the one-time
    // local→stable migration first (D-06).
    app.store.set_tabs_enabled(config.tabs.enabled);
    // `session.sidebar` (config/index.tsx:151-152): cli.json first,
    // then the persisted last toggle overrides (v2's toggle writes the
    // config doc; our ChannelStorage "sidebar" key stands in — the TUI
    // never rewrites the user's cli.json).
    app.store.set_sidebar_mode(config.session_sidebar);
    let tabs_storage = crate::storage::ChannelStorage::open_default_migrated().ok();
    if let Some(storage) = &tabs_storage {
        let doc: crate::storage::SidebarDoc = storage.read_or_default(crate::storage::KEY_SIDEBAR);
        if doc.mode == crate::components::sidebar::SidebarMode::Hide {
            app.store.set_sidebar_mode(doc.mode);
        }
        // Onboarding persistence (auditC S2): v2's
        // `storage.store("getting-started", { initial: { dismissed:
        // false } })` seeds the card's dismissal from
        // `<state>/<ch>/tui/getting-started.json` — the `✕` release
        // writes it back (the flip check below).
        let doc: crate::storage::GettingStartedDoc =
            storage.read_or_default(crate::storage::KEY_GETTING_STARTED);
        app.store.restore_sidebar_onboarding(doc.dismissed);
    }
    let mut persisted_onboarding = app.store.sidebar_onboarding_dismissed();
    let mut persisted_sidebar = app.store.sidebar_mode();
    let tabs_scope_cwd = directory.clone();
    if let Some(storage) = &tabs_storage {
        if config.tabs.enabled {
            let document = storage.read_tabs();
            // v2 `state()` (`session-tabs.tsx:104-107`): the cwd scope
            // reads its own entry and falls back to EMPTY (never the
            // global scope's tabs).
            let empty = crate::storage::TabsScope::default();
            let scoped: Vec<crate::store::SessionTab> = match config.tabs.scope {
                crate::config::TabsScope::Global => &document.global,
                crate::config::TabsScope::Cwd => {
                    document.cwd.get(&tabs_scope_cwd).unwrap_or(&empty)
                }
            }
            .tabs
            .iter()
            .map(|entry| crate::store::SessionTab {
                session_id: entry.session_id.clone(),
                title: entry.title.clone(),
            })
            .collect();
            app.store.restore_tabs(scoped);
        }
    }
    // The boot route (v2 app.tsx:639-674): `--session` routes that
    // session, `--continue` routes the directory's latest root
    // session (resolved before the first frame — v2 boots a "dummy"
    // session placeholder, then swaps; resolving first avoids the
    // flash entirely), and the default stays home.
    let continue_target = if boot.continue_last && boot.session.is_none() {
        // Bounded: an unroutable server must not hang the boot (the
        // TCP connect timeout alone can stretch past two minutes).
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            latest_session(&client, &directory),
        )
        .await
        .unwrap_or_default()
    } else {
        None
    };
    apply_boot_route(&app.store, &boot, continue_target.as_deref());
    // The restored/boot tabs must be visible to the connection's
    // FIRST-connect resync (D4: boot hydration fetches `message.list`
    // per open tab right after subscribe) — sync the registry here,
    // in the same non-yielding boot section, so the spawned task
    // cannot observe it empty.
    if let Ok(mut ids) = open_sessions.lock() {
        *ids = app.store.open_session_ids();
    }
    // F12/F13 — the loop fetcher: transcript hydration for tabs opened
    // AFTER boot (the first-connect resync covers the boot set — those
    // ids seed `fetched_transcripts`) + the /sessions dialog's
    // open-time session-list refetch.
    let (loop_fetch_tx, mut loop_fetch_rx) = spawn_loop_fetcher(client.clone());
    let mut fetched_transcripts: std::collections::HashSet<String> =
        app.store.open_session_ids().into_iter().collect();
    app.store
        .set_session_list_sink(std::rc::Rc::new(HttpSessionListSink { tx: loop_fetch_tx.clone() }));
    let mut persisted_tabs = app.store.tabs().tabs.clone();

    // Lane/chrome #4: the footer location's vcs lookup — one request
    // for the launch cwd at boot (the label paints once it resolves).
    if !directory.is_empty() {
        let _ = loop_fetch_tx.send(LoopRequest::LocationVcs(directory.clone()));
    }
    // Lane/chrome #6: v2 picks the home placeholder at RANDOM per boot
    // (`randomIndex` on the Prompt mount, prompt/index.tsx:103-106 +
    // :364 — the exporter pinned `Math.random = () => 0`; the live
    // process rolls). The per-boot pick is inherently unsyncable
    // between two live stacks — the rig's structural glyph mask
    // absorbs the difference.
    {
        let mut composer = app.store.composer(crate::app::HOME_PROMPT_KEY);
        composer.placeholder_index = crate::components::home::random_placeholder_index(
            crate::components::home::PLACEHOLDERS.len(),
        );
        app.store.set_composer(crate::app::HOME_PROMPT_KEY, &composer);
    }

    // `--prompt` (v2 app.tsx:626's `startupPrompt` + home.tsx:52-80 /
    // routes/session/index.tsx:377-381, 601-607): seed the BOOTED
    // route's composer with the flag's text and submit it once — the
    // first turn goes out on boot (home submissions create their
    // session through the prompt sink; a `--session`/`--continue`
    // boot submits into the routed session).
    if let Some(prompt) = &boot.prompt {
        let key = app.prompt_key();
        let mut composer = app.store.composer(&key);
        composer.draft.text.clone_from(prompt);
        app.store.set_composer(&key, &composer);
        let _ = app.store.submit_prompt(&key);
    }
    // `--auto` (v2 permission.tsx:14 `args.auto ? "autoaccept"`):
    // arm the permission auto-answer (every ask replies `once`,
    // session/index.tsx:255-262) + the composer metadata's `auto`
    // marker.
    app.autoaccept = boot.auto;
    app.store.set_permission_autoaccept(boot.auto);

    // Config reload: mtime check per tick (the reference watches the dir).
    let mut config_mtime = config
        .path
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .and_then(|meta| meta.modified().ok());
    let mut last_tick = std::time::Instant::now();
    let mut redraw_at = 0u64;
    // The previous frame's image placements (R-07) — the emission
    // delta (vanished ids delete; changed rects retransmit).
    let mut last_frame_images: Vec<crate::image::ImagePlacement> = Vec::new();

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

    // The production plugin boot (wave-12): the host loads the config's
    // `plugins[]` entries and every wave-3 seam attaches — the event
    // tap (post-ingest broadcast), the palette sink
    // (`execute_command_in_session` with the ACTIVE session), the
    // dialog source (materialize-at-open), the registration projection
    // (palette rows / group rules / home actions + the owned keymap
    // chords). `None` = no plugins configured (or no bun — the
    // graceful-disable logged once); the loop then skips the per-pump
    // drain.
    let config_root = oc_util::global_roots::resolve().config;
    let boot_cwd = std::path::PathBuf::from(&directory);
    let plugins_configured = {
        let files = crate::plugin_boot::discover_config_files(&config_root, &boot_cwd);
        crate::plugin_boot::collect_plugin_ops(&files)
            .iter()
            .any(|op| matches!(op, oc_plugin_host::PluginOp::Add { .. }))
    };
    let plugin_runtime = if plugins_configured {
        let agents = crate::plugin_boot::fetch_agent_seed(&client).await;
        let boot_client = client.clone();
        let boot_version = env!("CARGO_PKG_VERSION").to_owned();
        let boot_cwd = boot_cwd.clone();
        tokio::task::spawn_blocking(move || {
            crate::plugin_boot::TuiPluginRuntime::boot(
                &boot_client,
                agents,
                &config_root,
                &boot_cwd,
                &boot_version,
            )
        })
        .await
        .ok()
        .flatten()
        .map(crate::plugin_boot::TuiPluginRuntime::new)
    } else {
        None
    };
    if let Some(runtime) = &plugin_runtime {
        let projection = runtime.attach(&app.store);
        app.set_plugin_commands(projection.owned);
        app.set_plugin_themes(projection.themes);
    }

    let result: Result<(), String> = loop {
        // The plugin pump (wave-12): refresh the palette sink's active
        // session (marker commands submit into it), then drain the
        // plugin UI-request queue into the store (toasts, sidebar
        // sections, footer hints, dialog opens — the between-frames
        // poll the wave-3 contract set).
        if let Some(runtime) = &plugin_runtime {
            let active = match app.store.route() {
                crate::store::Route::Session { session_id } => Some(session_id),
                crate::store::Route::Home => None,
            };
            runtime.set_current_session(active);
            runtime.drain_ui_requests(&app.store);
        }
        // Drain ingest.
        while let Ok(message) = rx.try_recv() {
            if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("[tui] ingest: {message:?}");
            }
            app.store.ingest_all([message]);
        }
        // `--auto`: the freshly-ingested ask (if any) auto-answers
        // while autoaccept is armed — before the render, so the ask
        // footer never flashes.
        app.autoanswer_permissions();
        // Permission replies (D1): every once/always/reject the pump
        // recorded POSTs to the server — without this the parked tool
        // waits forever while the footer already looks resolved.
        for reply in std::mem::take(&mut app.permission_replies) {
            if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
                eprintln!(
                    "[tui] permission reply {} on {}: {}",
                    reply.request_id, reply.session_id, reply.decision
                );
            }
            post_permission_reply(client.clone(), reply);
        }
        // Rename submits (v2 index.tsx:908): the POST's
        // `session.renamed` echo lands the title in the store.
        for (session_id, title) in std::mem::take(&mut app.rename_requests) {
            if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("[tui] session rename {session_id}: {title}");
            }
            post_session_rename(client.clone(), session_id, title);
        }
        // Attention: the bell request set by permission asks / unread
        // transitions — write `\x07` once (raw mode passes it through
        // to the terminal, audible/visible over ssh).
        if app.store.take_bell() {
            use std::io::Write as _;
            let mut out = std::io::stdout();
            let _ = out.write_all(b"\x07");
            let _ = out.flush();
        }
        // Drain the server-list fetches into the picker state.
        while let Ok(event) = list_rx.try_recv() {
            match event {
                ServerListEvent::Config(model) => {
                    // The selection chain's step 3 (`configuredModel`,
                    // local.tsx:170-179) — the server's merged config
                    // view, not any TUI-local file.
                    app.store.set_config_model(model);
                }
                ServerListEvent::Models(Ok((models, providers))) => {
                    app.store.set_models(crate::store::ListState::Loaded(models), providers);
                }
                ServerListEvent::Models(Err(error)) => {
                    app.store.set_models(crate::store::ListState::Failed(error), Vec::new());
                }
                ServerListEvent::Agents(Ok(agents)) => {
                    app.store.set_agents(crate::store::ListState::Loaded(agents));
                }
                ServerListEvent::Agents(Err(error)) => {
                    if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("[tui] agent list failed: {error}");
                    }
                    app.store.set_agents(crate::store::ListState::Failed(error));
                }
                ServerListEvent::Connected(connected) => {
                    app.store.set_integration_connected(connected);
                }
            }
        }
        // Drain the worktree worker (R-05): project resolution, list
        // refreshes, create/remove results — v2's `worktree.updated`
        // SSE refetch flow, port-side.
        while let Ok(event) = worktree_rx.try_recv() {
            match event {
                WorktreeEvent::Resolved { project_id, current } => {
                    app.store.set_worktree_project(&project_id, current.as_deref());
                }
                WorktreeEvent::Listed(Ok(rows)) => {
                    app.store.set_worktree_list(crate::store::ListState::Loaded(rows));
                }
                WorktreeEvent::Listed(Err(error)) => {
                    app.store.set_worktree_list(crate::store::ListState::Failed(error));
                }
                WorktreeEvent::Created(Ok(directory)) => {
                    app.store.apply_worktree_created(&directory);
                }
                WorktreeEvent::Created(Err(error)) => {
                    app.store.toast_show(crate::components::toast::ToastOptions {
                        title: Some("Creating workspace failed".to_owned()),
                        message: error,
                        variant: "error",
                        duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
                        action_label: None,
                    });
                }
                WorktreeEvent::RemoveResult { directory, result } => match result {
                    Ok(()) => app.store.apply_worktree_removed(&directory),
                    Err((message, force_required)) => {
                        app.store.apply_worktree_remove_failed(
                            &directory,
                            &message,
                            force_required,
                        );
                    }
                },
            }
        }
        // Drain the prompt-sink events (home submissions creating
        // their session open a tab, commit the staged selection, and
        // navigate; FAILED sends surface as the error toast + the
        // restored draft — `fail()`/`recover()` in the reference's
        // submit flow).
        while let Ok(event) = sink_rx.try_recv() {
            if let PromptSinkEvent::SessionCreated { session_id, title } = event {
                app.store.open_tab(&session_id, title);
                let staged = app.store.selection_for(HOME_PROMPT_KEY);
                if let Some(model) = &staged.model {
                    app.store.select_model(&session_id, model);
                }
                if let Some(agent) = staged.agent {
                    app.store.select_agent(&session_id, &agent);
                }
            } else {
                apply_send_failure(&app.store, event);
            }
        }
        // Config-error toasts (the F5 seam): every skipped config entry
        // the boot/reload logged now surfaces in the app, never on the
        // frame (the stderr guard carries the log copy).
        for toast in crate::keymap::drain_config_error_toasts() {
            app.store.toast_show(toast);
        }
        // Wave A: the queued-prompt serial drain (a session that went
        // idle with queued prompts admits the next head — the idle
        // event hook also fires this; the tick covers restored tabs)
        // + the model-ring persistence.
        app.store.drain_queued_prompts();
        if let (Some(repository), Some(ring)) =
            (&model_preference_path, app.store.take_model_ring_dirty())
        {
            repository.write(&ring.document);
        }
        // The external editor (<leader>e / /editor): suspend → $EDITOR
        // over the focused draft → resume → replace on success (a
        // non-zero exit keeps the old draft per v2). The stderr guard
        // steps aside for the editor window — the child inherits fd 2
        // and its messages belong on the real terminal (v2 parity).
        if app.store.take_editor_request() {
            let editor_guard =
                stderr_guard.take().map(crate::stderr_redirect::StderrRedirect::restore);
            let key = app.prompt_key();
            let draft = app.store.prompt_draft(&key).text.clone();
            let cwd = std::env::current_dir().ok();
            let _ = disable_raw_mode();
            {
                use std::io::Write as _;
                let mut out = std::io::stdout();
                let _ = crossterm::queue!(out, crossterm::terminal::LeaveAlternateScreen);
                // Pop the mouse modes ONLY when they went on (the
                // config gate — a `mouse: false` boot never pushed
                // them, and popping unpushed modes is wire noise).
                let _ = crate::mouse_modes::pop(&mut out, applied_mouse);
                let _ = out.flush();
            }
            let result = crate::editor::open_editor(&draft, cwd.as_deref(), || {}, || {});
            let _ = enable_raw_mode();
            {
                use std::io::Write as _;
                let mut out = std::io::stdout();
                let _ = crossterm::queue!(out, crossterm::terminal::EnterAlternateScreen);
                // Re-arm the mouse modes under the SAME config gate
                // (auditC P1 called out the old unconditional
                // re-enable after $EDITOR).
                let _ = crate::mouse_modes::push(&mut out, applied_mouse);
                let _ = out.flush();
            }
            let _ = terminal.clear();
            if editor_guard.is_some() {
                stderr_guard = crate::stderr_redirect::install(&crate::stderr_redirect::log_path());
            }
            match result {
                Ok(Some(text)) => {
                    let normalized = crate::editor::normalize_prompt_content(&text);
                    let mut composer = app.store.composer(&key);
                    composer.draft.text = normalized;
                    app.store.set_composer(&key, &composer);
                }
                Err(crate::editor::EditorError::Exit(reason)) => {
                    eprintln!("opencode tui: {reason} (draft unchanged)");
                }
                Err(error) => eprintln!("opencode tui: {error:?}"),
                Ok(None) => {}
            }
            app.store.request_redraw();
        }
        // Tabs persistence: save when the tab list changed — into the
        // CONFIGURED scope only (v2 `session-tabs.tsx:109-115` writes
        // `draft.cwd[cwd]` or `draft.global`), preserving the other
        // scope's tabs, stamped with the open-tabs document version.
        let tabs_now = app.store.tabs().tabs.clone();
        if tabs_now != persisted_tabs {
            persisted_tabs = tabs_now.clone();
            if let Some(storage) = &tabs_storage {
                if config.tabs.enabled {
                    let mut document = storage.read_tabs();
                    let scope_tabs = crate::storage::TabsScope {
                        tabs: tabs_now
                            .iter()
                            .map(|tab| crate::storage::TabEntry {
                                session_id: tab.session_id.clone(),
                                title: tab.title.clone(),
                            })
                            .collect(),
                        unread: std::collections::BTreeMap::new(),
                    };
                    match config.tabs.scope {
                        crate::config::TabsScope::Global => document.global = scope_tabs,
                        crate::config::TabsScope::Cwd => {
                            document.cwd.insert(tabs_scope_cwd.clone(), scope_tabs);
                        }
                    }
                    if let Err(error) = storage.write_tabs(&document) {
                        // Failed writes lose only tab layout — but silence
                        // would hide tabs resetting every launch (v2 logs).
                        eprintln!("opencode tui: failed to persist session tabs: {error}");
                    }
                }
            }
        }
        // Sidebar persistence (v2's toggle writes `session.sidebar`
        // through config.update; ours lands in the channel's
        // "sidebar" key — same last-toggle-wins contract).
        let sidebar_now = app.store.sidebar_mode();
        if sidebar_now != persisted_sidebar {
            persisted_sidebar = sidebar_now;
            if let Some(storage) = &tabs_storage {
                if let Err(error) = storage.write_typed(
                    crate::storage::KEY_SIDEBAR,
                    &crate::storage::SidebarDoc { mode: sidebar_now },
                ) {
                    eprintln!("opencode tui: failed to persist sidebar mode: {error}");
                }
            }
        }
        // Onboarding persistence (footer.tsx:48-52 — the `✕` release
        // persists `dismissed = true`): save on the flip, v2's storage
        // draft shape.
        let onboarding_now = app.store.sidebar_onboarding_dismissed();
        if onboarding_now != persisted_onboarding {
            persisted_onboarding = onboarding_now;
            if let Some(storage) = &tabs_storage {
                if let Err(error) = storage.write_typed(
                    crate::storage::KEY_GETTING_STARTED,
                    &crate::storage::GettingStartedDoc { dismissed: onboarding_now },
                ) {
                    eprintln!("opencode tui: failed to persist onboarding dismissal: {error}");
                }
            }
        }
        // Keep the resync's open-session set current + F12 — transcript
        // hydration for newly opened sessions: every id the boot resync
        // did NOT cover (any tab opened after the first connect — the
        // /sessions dialog, a tab click) fetches `message.list` once
        // through the loop fetcher; the result merges over
        // live-projected rows (v2's route-mount
        // `data.session.message.sync`).
        let open_now = app.store.open_session_ids();
        if let Ok(mut ids) = open_sessions.lock() {
            ids.clone_from(&open_now);
        }
        for id in open_now {
            if fetched_transcripts.insert(id.clone()) {
                let _ = loop_fetch_tx.send(LoopRequest::Transcript(id));
            }
        }
        // Drain the loop fetcher (F12 transcripts + the F13 dialog-open
        // session-list refetch) through the store's single ingest path.
        while let Ok(fetch) = loop_fetch_rx.try_recv() {
            match fetch {
                LoopFetch::Transcript { session_id, result } => match result {
                    Ok(messages) => {
                        app.store.ingest_all([Ingest::MessagesSnapshot { session_id, messages }]);
                    }
                    Err(error) => {
                        crate::keymap::log_skip(&format!("transcript fetch {error}"));
                        fetched_transcripts.remove(&session_id);
                    }
                },
                LoopFetch::SessionList(Ok(sessions)) => {
                    let unread = crate::data::resync_unread(&sessions);
                    app.store.ingest_all([Ingest::Resync {
                        sessions,
                        unread,
                        messages: Vec::new(),
                    }]);
                }
                LoopFetch::SessionList(Err(error)) => {
                    crate::keymap::log_skip(&format!("session list refresh: {error}"));
                }
                // #4: the footer location label — `abbreviateHome(cwd)`
                // plus `:{branch}` when vcs resolves one
                // (prompt/index.tsx:1613-1619).
                LoopFetch::LocationVcs { directory, branch: Ok(branch) } => {
                    let home = std::env::var("HOME").ok().filter(|home| !home.is_empty());
                    let mut label = match home {
                        Some(home) => {
                            crate::components::dialog_worktrees::abbreviate_home(&directory, &home)
                        }
                        None => directory.clone(),
                    };
                    if let Some(branch) = branch {
                        label.push(':');
                        label.push_str(&branch);
                    }
                    app.store.set_footer_location(label);
                }
                // A failed/absent vcs lookup leaves the label as the
                // bare directory (v2's `vcs.sync` failures are silent
                // — `location.vcs.info()` just stays undefined; never
                // a user-facing error surface).
                LoopFetch::LocationVcs { branch: Err(error), .. } => {
                    eprintln!("opencode tui: vcs lookup skipped: {error}");
                }
            }
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
                // The live mouse flip (v2 app.tsx:593 —
                // `createEffect(() => renderer.useMouse =
                // config.data.mouse)`): a config change that toggles
                // `mouse` pushes/pops the v2 mode set immediately, no
                // restart needed.
                if reloaded.mouse != applied_mouse {
                    applied_mouse = reloaded.mouse;
                    write_mouse_flip(applied_mouse);
                }
            }
        }
        // Render (coalesced; the clock tick keeps spinners/toasts live).
        app.store.runtime.run_effects();
        let generation = app.store.redraw_generation();
        if generation != redraw_at || app.store.clock() > 0 {
            redraw_at = generation;
            // Live-size reconciliation (found by the parity rig's
            // resize corpus): ratatui's `autoresize` inside `draw()`
            // snaps the internal buffer to the PTY's CURRENT size the
            // moment the kernel delivers SIGWINCH — potentially one
            // tick BEFORE crossterm's Resize event reaches this loop
            // and updates `app.size`. Drawing a frame built at the
            // stale size then indexed a 100-col row into a 60-col
            // buffer (`index outside of buffer … index is (60, 0)` —
            // resize@60x20, our side panicked mid-script). v2 renders
            // at the live terminal dimensions; mirror that by adopting
            // the backend's size before building the frame.
            if let Ok(area) = terminal.size() {
                if (area.width, area.height) != app.size {
                    app.resize((area.width, area.height));
                }
            }
            let mut frame = app.frame();
            // S0 parity: degrade RGB → xterm-256 indices at the
            // terminal-draw seam when the raster-level capability
            // probe says the terminal is 256-color (v2's raster does
            // this at emission; `App::frame()` and the goldens stay
            // truecolor — see render::color_mode).
            if color_mode == ColorMode::Indexed256 {
                color_mode::degrade_frame(&mut frame, &mut color_cache);
            }
            let draw = terminal.draw(|surface| {
                // Resize race guard: the frame may have been built at
                // the PREVIOUS size in the same tick the terminal
                // buffer auto-resized (lane/chrome — the 60-col rig
                // resize caught a 100-wide frame against a 60-wide
                // buffer). Cells outside the surface are skipped for
                // one frame.
                let area = surface.area();
                let buffer = surface.buffer_mut();
                for (y, row) in frame.cells.iter().enumerate() {
                    for (x, cell) in row.iter().enumerate() {
                        let Ok(x) = u16::try_from(x) else { continue };
                        let Ok(y) = u16::try_from(y) else { continue };
                        // Bounds-safe write: the frame is built at the
                        // reconciled size (above), but never panic if a
                        // resize slips between frame build and draw.
                        let Some(slot) = buffer.cell_mut((x, y)) else { continue };
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
                // NEW-S1a (lane/chrome): the live caret is the TERMINAL
                // cursor — v2's textarea positions the hardware cursor
                // (never a painted block; the parity corpus never caught
                // a painted caret cell on the ref). The route render
                // stashed the cell; a dialog over the composer hides it
                // (v2 blurs the prompt while any dialog is open).
                if app.store.top_dialog().is_none() {
                    if let Some((x, y)) = app.store.caret_cell() {
                        if x < area.width && y < area.height {
                            surface.set_cursor_position(ratatui::layout::Position { x, y });
                        }
                    }
                }
            });
            if draw.is_err() {
                break Err("terminal draw failed".to_owned());
            }
            // R-07: the frame's image overlays — emit AFTER the cell
            // grid flushed (the placements sit over their placeholder
            // rects); ids that left the frame delete first.
            let frame_images = app.store.frame_images();
            if frame_images != last_frame_images {
                let vanished: Vec<u32> = last_frame_images
                    .iter()
                    .filter(|old| !frame_images.iter().any(|new| new.id == old.id))
                    .map(|old| old.id)
                    .collect();
                let mut out = std::io::stdout().lock();
                let _ = crate::image::emit_image_deletes(&mut out, &vanished);
                let _ = crate::image::emit_frame_images(
                    &mut out,
                    app.store.graphics_protocol(),
                    &frame_images,
                );
                last_frame_images = frame_images;
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
            // Focus: the attention marker consumes on FocusGained (the
            // bell itself fired regardless — the sound channel is
            // "always" in v2's notifications plugin). FocusLost blurs
            // the attention engine (OS notifications become eligible).
            Some(CrosstermEvent::FocusGained) => app.store.set_focused(),
            Some(CrosstermEvent::FocusLost) => app.store.set_blurred(),
            // Bracketed paste (R-07): resolve attachment file paths →
            // chips, else insert the text (`pasteInputText`).
            Some(CrosstermEvent::Paste(text)) => app.paste_terminal_text(&text),
            // Mouse (beta.8 bugs 3/4): the tab strip's hover/select/
            // close/add surface and the transcript's reasoning-row
            // disclosure toggle.
            Some(CrosstermEvent::Mouse(mouse)) => {
                if std::env::var("OC_TUI_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("[tui] mouse: {mouse:?}");
                }
                app.mouse_event(&mouse);
            }
            _ => {}
        }
        if app.exit_requested {
            break Ok(());
        }
    };

    // Teardown.
    connection_task.abort();
    lists_task.abort();
    if let Some(runtime) = &plugin_runtime {
        runtime.shutdown();
    }
    // Give stderr back to the terminal BEFORE leaving the alternate
    // screen: whatever the exit path prints lands on the real display.
    if let Some(guard) = stderr_guard {
        guard.restore();
    }
    let _ = disable_raw_mode();
    {
        use std::io::Write as _;
        let mut out = std::io::stdout();
        let _ = crossterm::queue!(
            out,
            LeaveAlternateScreen,
            crossterm::event::DisableFocusChange,
            crossterm::event::DisableBracketedPaste,
            // Clear any lingering graphics placements (R-07): the
            // alternate screen would discard them anyway, but an explicit
            // `a=d,d=a` keeps a same-screen exit clean.
        );
        // Pop the mouse modes under the same gate they went on with
        // (v2's teardown pops what the renderer pushed — a
        // `mouse: false` run pushed nothing).
        let _ = crate::mouse_modes::pop(&mut out, applied_mouse);
        let _ = out.flush();
    }
    // #46 — the exit epilogue (v2 app.tsx:446-450: the renderer stops,
    // then `process.stdout.write(epilogue + "\n")`): the wordmark +
    // resume hint the session route staged while mounted
    // (session/index.tsx:185-189). Absent from the home route — the
    // corpus's identical post-exit blanks.
    if result.is_ok() {
        if let Some(epilogue) = app.store.exit_epilogue() {
            use std::io::Write as _;
            let mut out = std::io::stdout();
            let _ = out.write_all(epilogue.as_bytes());
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        }
    }
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
            // Tab and crossterm's BackTab (the legacy `ESC [ Z`
            // sequence; it arrives with the SHIFT modifier set) are
            // both shift+tab — v2's `agent.cycle` chord.
            KeyCode::Tab | KeyCode::BackTab => Some("shift+tab".to_owned()),
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
        KeyCode::BackTab => Some("shift+tab".to_owned()),
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
    use crate::store::DialogSize;

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

    // -- Phase 8 lane B: live pickers / editor / copy / persistence ---

    /// The footer model panel renders from LIVE store data: loading
    /// ("Models loading") until the fetch lands, then the provider-
    /// grouped rows with the active session's selection as `current`.
    #[test]
    fn footer_model_panel_renders_live_data() {
        let app = App::new((100, 30));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.open_footer_route(crate::components::footer::PromptRoute::Model);
        let loading = app.frame_text();
        assert!(loading.contains("Models loading"), "loading state: {loading}");
        assert!(loading.contains("Select model"));

        app.store.set_models(
            crate::store::ListState::Loaded(vec![
                crate::store::ServerModel {
                    provider_id: "opencode".into(),
                    id: "grok".into(),
                    name: "Grok".into(),
                    cost_input: 0.0,
                    free: true,
                    deprecated: false,
                    variants: Vec::new(),
                    released: 0.0,
                    context_limit: None,
                },
                crate::store::ServerModel {
                    provider_id: "opencode".into(),
                    id: "stage".into(),
                    name: "Stage".into(),
                    cost_input: 0.0,
                    free: true,
                    deprecated: false,
                    variants: Vec::new(),
                    released: 0.0,
                    context_limit: None,
                },
            ]),
            vec![crate::store::ServerProvider { id: "opencode".into(), name: "OpenCode".into() }],
        );
        app.store.select_model(
            "s1",
            &crate::store::ModelSelection {
                provider_id: "opencode".into(),
                model_id: "grok".into(),
                variant: None,
            },
        );
        let loaded = app.frame_text();
        assert!(loaded.contains("Grok"), "the model row: {loaded}");
        assert!(loaded.contains("OpenCode"), "the provider group");
        assert!(loaded.contains("current"), "the current marker");
        // The mini footer's Free rule: zero-cost opencode models that
        // are not the current one carry the "Free" footer.
        assert!(loaded.contains("Free"), "zero-cost opencode models mark Free");

        // Collapsing the panel removes it from the frame.
        app.store.close_footer_panel();
        let collapsed = app.frame_text();
        assert!(!collapsed.contains("Select model"), "panel gone: {collapsed}");
    }

    /// `<leader>y` runs the copy flow through the pump: the clipboard
    /// sink receives the last assistant text and the success toast
    /// surfaces **in the frame** (the live toast layer — V1).
    #[test]
    fn leader_y_copies_the_last_assistant_message() {
        use crate::components::message::model::{Message, Part, PartTime};
        let clipboard = std::rc::Rc::new(crate::clipboard::RecordingClipboardSink::default());
        let clipboard_dyn: std::rc::Rc<dyn crate::clipboard::ClipboardSink> = clipboard.clone();
        let mut store = Store::new(crate::reactive::Runtime::new());
        store.set_clipboard_sink(clipboard_dyn);
        let mut app = App::with_store(store, (100, 30));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.set_messages(
            "s1",
            vec![Message::Assistant {
                id: "a1".into(),
                agent: "build".into(),
                model: String::new(),
                parts: vec![Part::Text { text: "Copy me.".into(), time: PartTime::default() }],
                finish: None,
                error: None,
                time: PartTime::default(),
                tokens: None,
                usage_total: None,
            }],
        );
        app.press("ctrl+x");
        app.press("y");
        assert_eq!(
            clipboard.writes.lock().unwrap().as_slice(),
            &["Copy me.".to_owned()],
            "OSC-52 sink received the text"
        );
        assert_eq!(
            app.store.toasts().current().map(|toast| toast.message.as_str()),
            Some("Message copied to clipboard!")
        );
        // V1: the toast is composited over the live frame, not just
        // tracked in the store.
        let frame = app.frame_text();
        assert!(frame.contains("Message copied to clipboard!"), "toast visible: {frame}");
    }

    /// D1: the once/always/reject decisions map onto the
    /// `session.permission.reply` wire body, and the pump's Enter
    /// leaves exactly one recorded reply for the drain loop to POST.
    #[test]
    fn permission_replies_map_onto_the_wire_body() {
        let reply = |decision: &str, message: &str| PermissionReply {
            session_id: "ses_1".into(),
            request_id: "per_1".into(),
            decision: decision.into(),
            message: message.into(),
        };
        let body = permission_reply_body(&reply("once", ""));
        assert_eq!(body.decision, oc_client::types::PermissionReply::Once);
        assert_eq!(body.message, None, "empty message omitted");
        assert_eq!(
            permission_reply_body(&reply("always", "")).decision,
            oc_client::types::PermissionReply::Always
        );
        let reject = permission_reply_body(&reply("reject", "do less"));
        assert_eq!(reject.decision, oc_client::types::PermissionReply::Reject);
        assert_eq!(reject.message.as_deref(), Some("do less"));

        // The pump records the reply (the run loop drains + POSTs it).
        let request = PermissionRequest {
            id: "per_1".into(),
            session_id: "ses_1".into(),
            action: "read".into(),
            ..PermissionRequest::default()
        };
        let mut app = App::new((100, 30));
        app.store.open_tab("ses_1", None);
        ask_permission(&app.store, &request);
        app.press("enter");
        assert_eq!(app.permission_replies.len(), 1, "one recorded reply");
        assert_eq!(app.permission_replies[0].decision, "once");
        assert!(!app.permission_live(), "the ask resolved locally");
    }

    /// `<leader>e` sets the editor request the run loop drains (the
    /// suspend/resume itself is a terminal-only flow — the invocation
    /// is unit-tested in `editor.rs`).
    #[test]
    fn leader_e_requests_the_external_editor() {
        let mut app = App::new((100, 30));
        app.store.open_tab("s1", Some("S".to_owned()));
        assert!(!app.store.take_editor_request());
        app.press("ctrl+x");
        app.press("e");
        assert!(app.store.take_editor_request(), "the pump set the request");
        assert!(!app.store.take_editor_request());
    }

    /// Lane/title #12: the rename dialog's enter records the
    /// `session.update` intent for the run loop's drain (v2
    /// index.tsx:908 — the server's `session.renamed` echo lands the
    /// title across every surface) alongside the optimistic tab
    /// patch.
    #[test]
    fn rename_dialog_records_the_server_intent() {
        use crate::store::DialogKind;
        let mut app = App::new((100, 30));
        app.store.open_tab("s1", None);
        app.store.navigate(Route::Session { session_id: "s1".to_owned() });
        app.store.replace_dialog(
            DialogKind::SessionRename { session_id: "s1".to_owned() },
            DialogSize::Medium,
        );
        for key in ["F", "i", "x", " ", "i", "t"] {
            app.press(key);
        }
        app.press("enter");
        assert_eq!(app.rename_requests, vec![("s1".to_owned(), "Fix it".to_owned())]);
        assert_eq!(
            app.store.tabs().tabs.iter().find(|t| t.session_id == "s1").unwrap().title.as_deref(),
            Some("Fix it"),
            "optimistic tab patch holds until the renamed echo"
        );
        // The rename body maps onto the wire shape.
        let body = oc_client::types::SessionUpdateBody {
            title: Some("Fix it".to_owned()),
            ..oc_client::types::SessionUpdateBody::default()
        };
        assert_eq!(body.title.as_deref(), Some("Fix it"));
    }

    /// L1 — the home route renders the strip with the `New session`
    /// pseudo-tab selected and NO idle ` + ` affordance
    /// (`items = [...ordered, NEW_SESSION_TAB]`, `showPlus`,
    /// session-tabs.tsx:1339-1344); the session route keeps the ` + `.
    #[test]
    fn home_strip_promotes_the_new_session_slot() {
        use crate::store::Route;
        let app = App::new((100, 30));
        app.store.open_tab("s1", Some("One".to_owned()));
        app.store.navigate(Route::Session { session_id: "s1".to_owned() });
        let session_row = app.frame_text().lines().next().unwrap_or_default().to_owned();
        assert!(session_row.contains("One"), "session route shows the tab: {session_row}");
        assert!(session_row.contains('+'), "session route keeps the add affordance: {session_row}");
        assert!(
            !session_row.contains("New session"),
            "no pseudo-tab on the session route: {session_row}"
        );

        app.store.navigate(Route::Home);
        let home_row = app.frame_text().lines().next().unwrap_or_default().to_owned();
        assert!(
            home_row.contains("+ New session"),
            "home promotes the new-session slot: {home_row}"
        );
        assert!(!home_row.ends_with('+'), "no idle add affordance at home: {home_row}");
    }

    /// L1 — opentui re-hovers from the stationary pointer after a
    /// re-layout: clicking ` + ` (home route, pseudo-tab grows under
    /// the pointer) keeps the ✕ revealed on the pseudo-tab — the
    /// multi-session census cells (`✕` at the hovered tab's edge).
    #[test]
    fn add_release_keeps_hover_on_the_tab_under_the_pointer() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mut app = App::new((100, 30));
        app.store.open_tab("s1", Some("Parity fake says hello.".to_owned()));
        app.store.navigate(crate::store::Route::Session { session_id: "s1".to_owned() });
        let motion = |col: u16, kind: MouseEventKind| MouseEvent {
            kind,
            column: col,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        // Motion onto the ` + ` glyph (one tab at 100: the solver caps
        // the tab at 32 cols, the plus zone is 32..35 — the glyph
        // sits at col 33).
        app.mouse_event(&motion(33, MouseEventKind::Moved));
        assert!(app.store.tabs().add_hovered, "the affordance is hovered");
        // Press + release → home route; the pointer now sits INSIDE
        // the grown pseudo-tab (no motion event fires).
        app.mouse_event(&motion(33, MouseEventKind::Down(MouseButton::Left)));
        app.mouse_event(&motion(33, MouseEventKind::Up(MouseButton::Left)));
        assert_eq!(app.store.route(), crate::store::Route::Home, "the add routed home");
        let row = app.frame_text().lines().next().unwrap_or_default().to_owned();
        assert!(row.contains('✕'), "the hovered pseudo-tab reveals its ✕: {row}");
    }

    /// A model pick through the DIALOG applies server-side: the sink
    /// receives switchModel for the active session and the dialog
    /// stack clears.
    #[test]
    fn model_dialog_pick_applies_through_the_sink() {
        use crate::store::{RecordingSelectionSink, SelectionCall};
        let sink = std::rc::Rc::new(RecordingSelectionSink::default());
        let sink_dyn: std::rc::Rc<dyn crate::store::SelectionSink> = sink.clone();
        let mut store = Store::new(crate::reactive::Runtime::new());
        store.set_selection_sink(sink_dyn);
        let mut app = App::with_store(store, (100, 30));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.set_models(
            crate::store::ListState::Loaded(vec![crate::store::ServerModel {
                provider_id: "anthropic".into(),
                id: "claude".into(),
                name: "Claude".into(),
                cost_input: 3.0,
                free: false,
                deprecated: false,
                variants: Vec::new(),
                released: 0.0,
                context_limit: None,
            }]),
            Vec::new(),
        );
        app.press("ctrl+x");
        app.press("m");
        assert!(matches!(
            app.store.top_dialog().map(|entry| entry.kind),
            Some(crate::store::DialogKind::ServerList { command: "model.list", .. })
        ));
        // Submit the (only) row.
        app.press("return");
        assert_eq!(app.store.dialogs().len(), 0, "the picker closed");
        assert_eq!(
            sink.calls.borrow().as_slice(),
            &[(
                "s1".into(),
                SelectionCall::Model {
                    provider_id: "anthropic".into(),
                    model_id: "claude".into(),
                    variant: None
                }
            )],
            "switchModel posted for the active session"
        );
    }

    /// The boot frame at 80×24 under the DEFAULT boot args: the home
    /// route (wordmark + centered composer), not a restored session —
    /// the beta.7 "Untitled session + blank void" regression guard.
    /// (The byte-exact home view is pinned by `home_golden_tests`
    /// against v2's `home.w80` golden; this pins the ROUTE.)
    #[test]
    fn boot_frame_renders_home_at_80x24() {
        let app = App::new((80, 24));
        apply_boot_route(&app.store, &BootArgs::default(), None);
        app.store.runtime.run_effects();
        let frame = app.frame_text();
        // The wordmark paints as block glyphs (█ rows), not the
        // literal string "OPENCODE".
        assert!(frame.contains("█▀▀█"), "the home wordmark: {frame}");
        assert!(frame.contains("Ask anything"), "the centered composer: {frame}");
        assert_eq!(app.store.route(), crate::store::Route::Home);
    }

    /// The tabs persistence round-trip at the seam the run loop uses:
    /// store tabs → the versioned `tabs` document shape (cwd scope,
    /// like the default config) → a fresh store. The restore hydrates
    /// the strip WITHOUT moving the route — the boot route stays home
    /// (v2 app.tsx:364-371).
    #[test]
    fn tabs_persist_and_restore_through_the_v2_document() {
        let store = Store::new(crate::reactive::Runtime::new());
        store.open_tab("ses_1", Some("First".to_owned()));
        store.open_tab("ses_2", None);
        store.navigate(crate::store::Route::Home);

        // Persist exactly like the run loop does (cwd scope).
        let document = crate::storage::TabsDocument {
            global: crate::storage::TabsScope::default(),
            cwd: std::collections::BTreeMap::from([(
                "/work".to_owned(),
                crate::storage::TabsScope {
                    tabs: store
                        .tabs()
                        .tabs
                        .iter()
                        .map(|tab| crate::storage::TabEntry {
                            session_id: tab.session_id.clone(),
                            title: tab.title.clone(),
                        })
                        .collect(),
                    unread: std::collections::BTreeMap::new(),
                },
            )]),
            ..crate::storage::TabsDocument::default()
        };
        let root = std::env::temp_dir().join(format!(
            "oc-tui-tabs-rt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        let storage = crate::storage::ChannelStorage::open(&root, "local").expect("open");
        storage.write_tabs(&document).expect("write");

        // Boot a fresh store from the same file.
        let restored = storage.read_tabs();
        let fresh = Store::new(crate::reactive::Runtime::new());
        // The cwd scope reads ONLY its own entry: a global-scope list
        // (even a populated one) never leaks into a cwd restore (v2
        // `state()` falls back to EMPTY, not global).
        let mut with_global = restored.clone();
        with_global.global = crate::storage::TabsScope {
            tabs: vec![crate::storage::TabEntry { session_id: "ses_global".into(), title: None }],
            unread: std::collections::BTreeMap::new(),
        };
        fresh.restore_tabs(
            with_global
                .cwd
                .get("/work")
                .unwrap()
                .tabs
                .iter()
                .map(|entry| crate::store::SessionTab {
                    session_id: entry.session_id.clone(),
                    title: entry.title.clone(),
                })
                .collect(),
        );
        assert_eq!(fresh.tabs().tabs.len(), 2);
        assert_eq!(fresh.tabs().tabs[0].title.as_deref(), Some("First"));
        assert!(
            !fresh.tabs().tabs.iter().any(|tab| tab.session_id == "ses_global"),
            "the global scope never leaks into a cwd restore"
        );
        assert_eq!(fresh.route(), crate::store::Route::Home, "restore never moves the boot route");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The boot route rules (v2 app.tsx:364-371 + 639-674): default
    /// boot = home; `--session <id>` routes that session (opening its
    /// tab through the route effect); `--continue` routes the resolved
    /// latest session — and stays home when none resolves.
    #[test]
    fn boot_route_rules_default_home_continue_and_session() {
        // Default: home, no tabs.
        let store = Store::new(crate::reactive::Runtime::new());
        apply_boot_route(&store, &BootArgs::default(), None);
        assert_eq!(store.route(), crate::store::Route::Home);

        // --session: that session, its tab open.
        let store = Store::new(crate::reactive::Runtime::new());
        apply_boot_route(
            &store,
            &BootArgs {
                continue_last: false,
                session: Some("ses_direct".into()),
                ..BootArgs::default()
            },
            None,
        );
        assert_eq!(
            store.route(),
            crate::store::Route::Session { session_id: "ses_direct".to_owned() }
        );
        assert!(store.tabs().tabs.iter().any(|tab| tab.session_id == "ses_direct"));

        // --continue: the resolved latest root session.
        let store = Store::new(crate::reactive::Runtime::new());
        apply_boot_route(
            &store,
            &BootArgs { continue_last: true, session: None, ..BootArgs::default() },
            Some("ses_new"),
        );
        assert_eq!(
            store.route(),
            crate::store::Route::Session { session_id: "ses_new".to_owned() }
        );

        // --continue with nothing to resume: stays home (v2 `if
        // (!match) return`).
        let store = Store::new(crate::reactive::Runtime::new());
        apply_boot_route(
            &store,
            &BootArgs { continue_last: true, session: None, ..BootArgs::default() },
            None,
        );
        assert_eq!(store.route(), crate::store::Route::Home);
    }
}

/// Wave-6 live palette: typing `/` through the PUMP opens the popup
/// (with descriptions), arrows move, Enter ACCEPTS into the draft —
/// the popup state survives the composer store round-trip between
/// keypresses.
#[test]
fn slash_palette_opens_accepts_and_restores_the_popup_state() {
    let mut app = App::new((80, 24));
    app.store.open_tab("s1", Some("S".to_owned()));
    app.press("/");
    let composer = app.store.composer("s1");
    let visible = composer.autocomplete.visible;
    assert_eq!(visible, Some(crate::components::composer::autocomplete::Mode::Command));
    assert!(
        composer.autocomplete.options.iter().take(3).any(|option| option.display == "/agents"),
        "the v2 table loaded: {:?}",
        composer.autocomplete.options.iter().map(|o| o.display.clone()).take(5).collect::<Vec<_>>()
    );
    assert!(
        composer
            .autocomplete
            .options
            .iter()
            .any(|option| option.description.as_deref().is_some_and(|d| d.contains("agent"))),
        "descriptions attached (the preview column)"
    );

    // Filter to /ex: typing narrows the list — the display prefix
    // doubles the score, so /exit leads; /quit and /q ride the
    // DESCRIPTION match ("Exit the app" — v2 matches descriptions for
    // slash commands).
    app.press("e");
    app.press("x");
    let composer = app.store.composer("s1");
    assert_eq!(
        composer.autocomplete.options.first().map(|o| o.display.clone()),
        Some("/exit".to_owned())
    );
    assert!(
        composer.autocomplete.options.iter().all(|option| option.display.starts_with("/e")
            || option.description.as_deref().is_some_and(|d| d.to_lowercase().contains("ex"))),
        "filtered: {:?}",
        composer.autocomplete.options.iter().map(|o| o.display.clone()).collect::<Vec<_>>()
    );

    // Enter accepts the selected command — v2 RUNS it (`onSelect:
    // command.run`, autocomplete.tsx:481): /exit requests the app
    // exit; the trigger token is REMOVED (hide(true)) and nothing is
    // submitted to the model.
    app.press("enter");
    assert_eq!(app.store.prompt_draft("s1").text, "", "the run removes the trigger token");
    assert!(app.store.composer("s1").autocomplete.visible.is_none(), "accept closes the popup");
    assert!(app.exit_requested, "/exit executed app.exit, not a prompt send");
}

/// beta.8 bug 1 — the composer's slash dispatch, per v2 semantics:
/// * known built-ins (`onSelect: command.run`) EXECUTE locally —
///   `/help` opens `DialogHelp`, `/editor` requests $EDITOR;
/// * the argument-taking `/cd` INSERTS (`insertSlash`);
/// * an UNKNOWN `/word` (popup dismissed or no matches) submits as
///   prompt text exactly like v2's `submitInner` fall-through (only
///   `slash.arguments` commands and SERVER custom commands intercept
///   the submit path, prompt/index.tsx:1133-1150).
#[test]
fn slash_builtins_execute_and_unknown_words_submit() {
    let mut app = App::new((80, 24));
    app.store.open_tab("s1", Some("S".to_owned()));

    // /help → the DialogHelp dialog (help.show → dialog.replace).
    for spec in ["/", "h", "e", "l", "p"] {
        app.press(spec);
    }
    app.press("enter");
    assert_eq!(
        app.store.top_dialog().map(|dialog| dialog.kind),
        Some(crate::store::DialogKind::Help),
        "/help opened the help dialog"
    );
    assert!(app.store.prompt_draft("s1").is_empty(), "no prompt was sent");
    // Escape closes the dialog.
    app.press("escape");
    assert!(app.store.dialogs().is_empty(), "escape closes the help dialog");

    // /editor → prompt.editor: the run loop's editor request flag.
    for spec in ["/", "e", "d", "i", "t", "o", "r"] {
        app.press(spec);
    }
    app.press("enter");
    assert!(
        app.store.take_editor_request(),
        "/editor executed prompt.editor (the $EDITOR suspend seam)"
    );

    // /cd — the one argument-taking command — still INSERTS.
    for spec in ["/", "c", "d"] {
        app.press(spec);
    }
    app.press("enter");
    assert_eq!(app.store.prompt_draft("s1").text, "/cd ", "the argument command inserts");

    // An unknown /word: v2's popup stays visible EMPTY for a
    // non-matching query and swallows Enter (`select()` no-ops;
    // `submitInner` bails on `auto()?.visible`) — only after ESCAPE
    // dismisses it does Enter submit the word as prompt TEXT (the
    // fall-through — nothing rejects it, prompt/index.tsx:1147-1163).
    app.press("ctrl+c"); // prompt.clear: drop the /cd draft
    for spec in ["/", "z", "z", "z"] {
        app.press(spec);
    }
    let composer = app.store.composer("s1");
    assert!(composer.autocomplete.visible.is_some(), "the popup stays up with zero matches");
    assert!(
        composer.autocomplete.options.is_empty(),
        "a non-matching query filters to nothing: {:?}",
        composer.autocomplete.options
    );
    app.press("enter");
    assert!(
        app.store.composer("s1").history.entries.is_empty(),
        "enter with the empty popup visible does nothing"
    );
    app.press("escape");
    app.press("enter");
    assert_eq!(
        app.store.composer("s1").history.entries.last(),
        Some(&"/zzz".to_owned()),
        "after dismissal the unknown word submits as text"
    );
}

/// A crossterm mouse event at (`col`, `row`).
#[cfg(test)]
fn mouse(
    kind: crossterm::event::MouseEventKind,
    col: u16,
    row: u16,
) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind,
        column: col,
        row,
        modifiers: crossterm::event::KeyModifiers::empty(),
    }
}

/// beta.8 bug 3 — the tab strip's pointer surface through the pump:
/// hover arms the lift, `✕` closes, a body click selects, ` + `
/// navigates home (v2 `session-tabs.tsx:1505-1514`, `1745-1754`,
/// `1786-1792`).
#[test]
fn mouse_strip_hover_close_select_and_add() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new((80, 24));
    app.store.open_tab("s1", Some("Alpha".to_owned()));
    app.store.open_tab("s2", Some("Beta".to_owned()));
    app.store.navigate(Route::Session { session_id: "s1".to_owned() });

    // Hover the second tab (x=40 on the 32+32 split): the lift arms.
    app.mouse_event(&mouse(MouseEventKind::Moved, 40, 0));
    assert_eq!(app.store.tabs().hover.as_deref(), Some("s2"));
    assert!(!app.store.tabs().add_hovered);
    // The ✕ cell at 62 closes s2 (the mark renders one cell in from
    // the hovered tab's right edge).
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 62, 0));
    let tabs = app.store.tabs().tabs.clone();
    assert_eq!(
        tabs.iter().map(|tab| tab.session_id.as_str()).collect::<Vec<_>>(),
        vec!["s1"],
        "the hovered tab's ✕ closed it"
    );
    assert_eq!(app.store.route(), Route::Session { session_id: "s1".to_owned() });

    // A body click selects: s1 is the only tab; open another and
    // click its body mid-slot.
    app.store.open_tab("s3", Some("Gamma".to_owned()));
    app.mouse_event(&mouse(MouseEventKind::Moved, 40, 0));
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 40, 0));
    assert_eq!(app.store.route(), Route::Session { session_id: "s3".to_owned() });

    // The ` + ` affordance navigates home (v2 `add()` → home route).
    app.mouse_event(&mouse(MouseEventKind::Moved, 65, 0));
    assert!(app.store.tabs().add_hovered, "the + highlights under the pointer");
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 65, 0));
    assert_eq!(app.store.route(), Route::Home);

    // Hover off the strip clears the pointer state.
    app.store.navigate(Route::Session { session_id: "s3".to_owned() });
    app.mouse_event(&mouse(MouseEventKind::Moved, 40, 5));
    assert_eq!(app.store.tabs().hover, None);
    assert!(!app.store.tabs().add_hovered);
}

/// dogfood2 bug 1 — the strip is a POINTER surface on the HOME route
/// too: v2 mounts `<SessionTabs/>` app-wide (app.tsx:1353-1355, above
/// the route `<Switch>`; `tabsAvailable()` = `enabled() &&
/// tabs().length > 0 && route.data.type !== "plugin"`,
/// app.tsx:576-579 — never session-route-gated), and `select()` is a
/// bare route navigate (context/session-tabs.tsx:393-396). Restored
/// tabs painted at home but took no clicks when this gate was
/// session-only.
#[test]
fn mouse_home_strip_click_selects_the_restored_tab() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let mut app = App::new((80, 24));
    app.store.restore_tabs(vec![
        crate::store::SessionTab { session_id: "r1".into(), title: Some("Restored One".into()) },
        crate::store::SessionTab { session_id: "r2".into(), title: Some("Restored Two".into()) },
    ]);
    assert_eq!(app.store.route(), Route::Home, "restore never moves the boot route");
    // Hover arms on home exactly like the session route.
    app.mouse_event(&mouse(MouseEventKind::Moved, 5, 0));
    assert_eq!(app.store.tabs().hover.as_deref(), Some("r1"));
    // The body release selects: home → the restored session's route.
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 5, 0));
    assert_eq!(app.store.route(), Route::Session { session_id: "r1".to_owned() });
    // The ` + ` affordance still navigates home from the session...
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 65, 0));
    assert_eq!(app.store.route(), Route::Home);
    // ...and the second restored tab's body selects from home (tab 2
    // spans x=32..64 on the 32+32 layout).
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 40, 0));
    assert_eq!(app.store.route(), Route::Session { session_id: "r2".to_owned() });
}

/// beta.8 bug 4 — the `+ Thought` disclosure through the pump: a
/// completed reasoning part renders the collapsed header, a mouse
/// release over the header row expands it (the bordered muted body
/// renders), and a second release collapses it again
/// (`onMouseUp={toggle}`, group-view.tsx:183).
#[test]
fn mouse_release_toggles_the_thought_disclosure() {
    use crate::components::message::model::{Message, Part, PartTime};
    use crossterm::event::{MouseButton, MouseEventKind};

    let mut app = App::new((80, 24));
    app.store.set_messages(
        "s1",
        vec![
            Message::User {
                id: "u1".into(),
                text: "think hard".into(),
                created: 0,
                files: Vec::new(),
            },
            Message::Assistant {
                id: "a1".into(),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: vec![Part::Reasoning {
                    text: "**Inspecting**\n\nThe parser's edges need care.".into(),
                    time: PartTime { created: Some(1), completed: Some(2) },
                }],
                finish: Some("success".into()),
                error: None,
                time: PartTime { created: Some(1), completed: Some(2) },
                tokens: None,
                usage_total: None,
            },
        ],
    );
    app.store.open_tab("s1", Some("Thoughts".to_owned()));

    // Collapsed: the header row, no body.
    let collapsed = app.frame_text();
    assert!(collapsed.contains("+ Thought"), "the collapsed header: {collapsed}");
    assert!(!collapsed.contains("parser's edges"), "the body stays hidden: {collapsed}");

    // The header's absolute rect comes from the last render's hit
    // rows — release over it to expand.
    let hit = app
        .store
        .transcript_hit_rows()
        .iter()
        .find(|row| matches!(
            &row.action,
            crate::store::TranscriptHitAction::ToggleGroup(key) if key.starts_with("group:Reasoning:")
        ))
        .expect("the reasoning header recorded a hit row")
        .clone();
    app.mouse_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        10,
        u16::try_from(hit.y).unwrap_or(u16::MAX),
    ));
    assert!(
        app.store.group_expanded(
            &hit.session_id,
            hit.action.toggle_key().expect("the reasoning header hit toggles"),
        ),
        "the release toggled the disclosure open"
    );
    let expanded = app.frame_text();
    assert!(expanded.contains("- Thought"), "the header flips to -: {expanded}");
    assert!(expanded.contains("parser's edges"), "the bordered muted body renders: {expanded}");
    assert!(expanded.contains('┃'), "the SplitBorder left border: {expanded}");

    // Collapse again.
    let hit_again = app
        .store
        .transcript_hit_rows()
        .iter()
        .find(|row| matches!(
            &row.action,
            crate::store::TranscriptHitAction::ToggleGroup(key) if key.starts_with("group:Reasoning:")
        ))
        .expect("the header re-recorded its expanded-rect")
        .clone();
    app.mouse_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        10,
        u16::try_from(hit_again.y).unwrap_or(u16::MAX),
    ));
    let key = hit.action.toggle_key().expect("a toggle hit").to_owned();
    assert!(!app.store.group_expanded(&hit.session_id, key.as_str()));
    let collapsed_again = app.frame_text();
    assert!(collapsed_again.contains("+ Thought"), "back to the collapsed header");
    assert!(!collapsed_again.contains("parser's edges"), "the body hides again");
}

/// #29 — the toggle hit zone is TIGHT: v2's disclosure toggle is the
/// `InlineToolRow` content box (icon + label glyphs,
/// `group-view.tsx:176-190`), not the full header row — a release on
/// the empty columns to the right of `+ Thought…` is a no-op there.
#[test]
fn thought_toggle_hit_zone_is_tight_to_the_glyphs() {
    use crate::components::message::model::{Message, Part, PartTime};
    use crossterm::event::{MouseButton, MouseEventKind};

    let mut app = App::new((80, 24));
    app.store.set_messages(
        "s1",
        vec![
            Message::User {
                id: "u1".into(),
                text: "think hard".into(),
                created: 0,
                files: Vec::new(),
            },
            Message::Assistant {
                id: "a1".into(),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: vec![Part::Reasoning {
                    text: "The parser's edges need care.".into(),
                    time: PartTime { created: Some(1), completed: Some(2) },
                }],
                finish: Some("success".into()),
                error: None,
                tokens: None,
                usage_total: None,
                time: PartTime { created: Some(1), completed: Some(2) },
            },
        ],
    );
    app.store.open_tab("s1", Some("Thoughts".to_owned()));
    let _ = app.frame(); // the render pass records the hit rows
    let hit = app
        .store
        .transcript_hit_rows()
        .iter()
        .find(|row| matches!(row.action, crate::store::TranscriptHitAction::ToggleGroup(_)))
        .expect("the reasoning header recorded a hit row")
        .clone();
    // The recorded rect hugs the glyphs: it starts at the icon and
    // ends after the label text (well short of the 80-col row).
    assert_eq!(hit.x, 5, "paddingLeft 3 + the transcript's 2-col inset");
    assert!(hit.width < 40, "the zone is the label's glyphs, not the row: {hit:?}");
    assert_eq!(hit.height, 1);
    let row = u16::try_from(hit.y).unwrap_or(u16::MAX);
    // A release past the glyphs (col 79): nothing toggles.
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 79, row));
    assert!(
        !app.store.group_expanded(&hit.session_id, hit.action.toggle_key().expect("a toggle hit")),
        "the empty columns beside the header are inert"
    );
    // Inside the glyphs: the disclosure opens.
    app.mouse_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        u16::try_from(hit.x + 2).unwrap_or(u16::MAX),
        row,
    ));
    assert!(app
        .store
        .group_expanded(&hit.session_id, hit.action.toggle_key().expect("a toggle hit")));
}

/// #31 — the dialog backdrop dismisses on an OUTSIDE release
/// (`ui/dialog.tsx:34-63`); a release inside the chrome box stays.
#[test]
fn dialog_outside_release_dismisses() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let mut app = App::new((80, 24));
    app.store.open_dialog(crate::store::DialogKind::Help, crate::store::DialogSize::Medium);
    let _ = app.frame(); // the render pass records the chrome rect
    let (x, y, width, height) =
        app.store.dialog_frame_rect().expect("the render pass recorded the frame rect");
    assert!(x > 0 && y > 0 && width < 80, "centered with margins: {x},{y},{width},{height}");
    // Inside the frame: stays open.
    app.mouse_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        u16::try_from(x + width / 2).unwrap_or(u16::MAX),
        u16::try_from(y + height / 2).unwrap_or(u16::MAX),
    ));
    assert!(!app.store.dialogs().is_empty(), "an inside release does not dismiss");
    // Outside (top margin): dismissed.
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 40, 0));
    assert!(app.store.dialogs().is_empty(), "the backdrop's outside release closes the dialog");
    assert_eq!(app.store.dialog_frame_rect(), None, "the rect cleared with the dialog");
}

/// #32 — ctrl+c routing with a modal open (`ui/dialog.tsx:115-156`):
/// the modal layer CLOSES the dialog (clearing any filter text
/// first); it does not exit. Once the dialog is gone, the home
/// route's empty-composer ctrl+c exits — the rig's
/// click-coverage-dialog teardown sequence.
#[test]
fn ctrl_c_closes_the_dialog_then_exits_from_home() {
    let mut app = App::new((80, 24));
    app.store.open_dialog(crate::store::DialogKind::Help, crate::store::DialogSize::Medium);
    assert_eq!(app.store.focus(), crate::store::Focus::Dialog);
    app.press("ctrl+c");
    assert!(app.store.dialogs().is_empty(), "the modal layer consumed ctrl+c to close");
    assert!(!app.exit_requested, "the close is not an exit");
    // The empty home composer: the same chord now exits.
    app.press("ctrl+c");
    assert!(app.exit_requested, "post-dismissal ctrl+c exits at home");
}

/// #34 — a user-message block release opens the Message Actions
/// select (`UserMessage`'s `onMouseUp` → `dialog.replace(DialogMessage)`,
/// session/index.tsx:2243-2249); its options carry the four v2 actions.
#[test]
fn user_message_release_opens_message_actions() {
    use crate::components::message::model::{Message, Part, PartTime};
    use crossterm::event::{MouseButton, MouseEventKind};

    let mut app = App::new((80, 24));
    app.store.set_messages(
        "s1",
        vec![
            Message::User {
                id: "u1".into(),
                text: "edit this turn".into(),
                created: 0,
                files: Vec::new(),
            },
            Message::Assistant {
                id: "a1".into(),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: vec![Part::Text { text: "the reply".into(), time: PartTime::default() }],
                finish: Some("success".into()),
                error: None,
                tokens: None,
                usage_total: None,
                time: PartTime { created: Some(1), completed: Some(2) },
            },
        ],
    );
    app.store.open_tab("s1", Some("Edit me".to_owned()));
    let _ = app.frame(); // the render pass records the hit rows
    let hits = app.store.transcript_hit_rows();
    let user_hit = hits
        .iter()
        .find(|hit| matches!(hit.action, crate::store::TranscriptHitAction::MessageActions { .. }))
        .expect("the user-message block recorded a hit row")
        .clone();
    assert!(user_hit.height >= 3, "the box's padding rows are clickable too");
    assert!(user_hit.x >= 2 && user_hit.width >= 60, "the inner box spans the block");
    // A release inside the block opens the dialog.
    app.mouse_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        u16::try_from(user_hit.x + 2).unwrap_or(u16::MAX),
        u16::try_from(user_hit.y + 1).unwrap_or(u16::MAX),
    ));
    let dialogs = app.store.dialogs();
    assert!(
        matches!(
            dialogs.last().map(|entry| &entry.kind),
            Some(crate::store::DialogKind::MessageActions { message_id, .. }) if message_id == "u1"
        ),
        "the Message Actions select opened: {dialogs:?}"
    );
    // The assistant reply text is NOT clickable (no handlers on
    // TextPart rows).
    assert!(hits.iter().all(|hit| !matches!(
        hit.action,
        crate::store::TranscriptHitAction::MessageActions { ref message_id } if message_id == "a1"
    )));
}

/// The sidebar onboarding card's pointer surface (footer.tsx:45-70):
/// the `✕` release dismisses the card — the store flip the run loop's
/// persistence check writes the `getting-started` doc from — and the
/// "Connect provider" row dispatches `provider.connect` (the palette
/// row's own handler opens the server-list dialog). The rects clear
/// with the card (no zombie clicks on a dismissed card).
#[test]
fn onboarding_card_pointer_routes_dismiss_and_connect() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let mut app = App::new((160, 30));
    app.store.open_tab("s1", Some("Bug hunt".to_owned()));
    app.store.navigate(Route::Session { session_id: "s1".to_owned() });
    let _ = app.frame(); // the render pass records the hit rects
    let hits = app.store.sidebar_onboarding_hits().expect("the card painted");
    // The dismiss rect IS the painted ✕ cell; the connect rect spans
    // the action row (label + /connect hint).
    let grid = app.frame().text_grid();
    assert_eq!(
        grid[hits.dismiss.y].chars().nth(hits.dismiss.x),
        Some('✕'),
        "the dismiss target is the ✕ glyph"
    );
    let connect_row: String =
        grid[hits.connect.y].chars().skip(hits.connect.x).take(hits.connect.width).collect();
    assert!(connect_row.contains("Connect provider"), "row: {connect_row}");
    assert!(connect_row.contains("/connect"), "row: {connect_row}");
    // Release on the ✕ dismisses; the re-render drops the rects.
    app.mouse_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        u16::try_from(hits.dismiss.x).unwrap_or(u16::MAX),
        u16::try_from(hits.dismiss.y).unwrap_or(u16::MAX),
    ));
    assert!(app.store.sidebar_onboarding_dismissed(), "the ✕ flipped the dismissal");
    let _ = app.frame();
    assert!(app.store.sidebar_onboarding_hits().is_none(), "the rects cleared");

    // The connect row on a fresh card dispatches `provider.connect`.
    let mut app = App::new((160, 30));
    app.store.open_tab("s2", Some("Second".to_owned()));
    app.store.navigate(Route::Session { session_id: "s2".to_owned() });
    let _ = app.frame();
    let hits = app.store.sidebar_onboarding_hits().expect("the card painted again");
    app.mouse_event(&mouse(
        MouseEventKind::Up(MouseButton::Left),
        u16::try_from(hits.connect.x).unwrap_or(u16::MAX),
        u16::try_from(hits.connect.y).unwrap_or(u16::MAX),
    ));
    assert!(
        matches!(
            app.store.dialogs().last().map(|entry| &entry.kind),
            Some(crate::store::DialogKind::ServerList { command: "provider.connect", .. })
        ),
        "provider.connect opened the server-list dialog: {:?}",
        app.store.dialogs()
    );
}

/// `--auto`'s autoaccept (v2 session/index.tsx:252-262): while armed,
/// a live permission ask auto-replies `once` through the same queue
/// a keyboard allow uses, resolves locally (the ask footer falls
/// back), and never double-answers the same request id.
#[test]
fn autoaccept_auto_answers_permission_asks_once() {
    let mut app = App::new((80, 24));
    let ask = crate::data::Ingest::Event(crate::data::BusEvent::new(
        "permission.asked",
        serde_json::json!({
            "id": "p1",
            "sessionID": "ses",
            "action": "read",
            "resources": ["src/index.ts"],
        }),
    ));
    // Unarmed: the ask surfaces (the keyboard path owns the reply).
    app.store.ingest_all([ask.clone()]);
    app.autoanswer_permissions();
    assert!(app.permission_live(), "the ask footer holds the screen");
    assert!(app.permission_replies.is_empty(), "no reply while disarmed");
    // Armed (--auto): the SAME ask answers once — decision `once`,
    // the footer falls back, and a second drain does not duplicate.
    app.autoaccept = true;
    app.autoanswer_permissions();
    assert!(!app.permission_live(), "the ask resolved locally");
    assert_eq!(app.permission_replies.len(), 1, "one reply");
    assert_eq!(app.permission_replies[0].request_id, "p1");
    assert_eq!(app.permission_replies[0].session_id, "ses");
    assert_eq!(app.permission_replies[0].decision, "once");
    app.autoanswer_permissions();
    assert_eq!(app.permission_replies.len(), 1, "the autoApproved set dedupes");
    // A NEW ask under the arm answers immediately (the pump's
    // post-ingest call site).
    app.store.ingest_all([crate::data::Ingest::Event(crate::data::BusEvent::new(
        "permission.asked",
        serde_json::json!({
            "id": "p2",
            "sessionID": "ses",
            "action": "bash",
            "resources": ["rm -rf /tmp/x"],
        }),
    ))]);
    app.autoanswer_permissions();
    assert_eq!(app.permission_replies.len(), 2, "each ask answered");
    assert_eq!(app.permission_replies[1].request_id, "p2");
}

/// The select dialog's pointer surface (`dialog-select.tsx:757-775`):/// pointer motion over an option row moves the selection, a release
/// submits it (the same path as enter), a release outside the frame
/// dismisses.
#[test]
fn dialog_select_pointer_moves_and_submits() {
    use crate::components::command_palette::select_props_for;
    use crossterm::event::{MouseButton, MouseEventKind};

    let mut app = App::new((80, 24));
    let clipboard = std::rc::Rc::new(crate::clipboard::RecordingClipboardSink::default());
    app.store.set_clipboard_sink(std::rc::Rc::clone(&clipboard) as std::rc::Rc<_>);
    app.store.open_dialog(
        crate::store::DialogKind::MessageActions {
            session_id: "s1".into(),
            message_id: "u1".into(),
        },
        crate::store::DialogSize::Medium,
    );
    let _ = app.frame(); // the render pass records the rects
    let rows = app.store.dialog_option_rows();
    assert_eq!(rows.len(), 4, "the four Message Actions");
    assert!(rows.iter().all(Option::is_some), "all four visible at 80x24");
    // Motion over the third row (Copy) moves the selection.
    let copy_row = u16::try_from(rows[2].unwrap_or_default()).unwrap_or(u16::MAX);
    let (_, box_y, _, _) = app.store.dialog_frame_rect().unwrap_or_default();
    assert!(usize::from(copy_row) > box_y, "the option rows sit inside the frame");
    app.mouse_event(&mouse(MouseEventKind::Moved, 40, copy_row));
    assert_eq!(
        app.store.dialogs().last().map(|top| top.select.selected),
        Some(2),
        "pointer motion moved the selection to Copy"
    );
    // Release there submits: Copy writes the clipboard and closes.
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 40, copy_row));
    assert!(app.store.dialogs().is_empty(), "the release submitted and closed");
    assert!(
        clipboard.writes.lock().is_ok_and(|writes| writes.len() == 1),
        "the message text hit the clipboard sink"
    );
    // A release far outside any dialog dismisses without submitting.
    app.store.open_dialog(
        crate::store::DialogKind::MessageActions {
            session_id: "s1".into(),
            message_id: "u1".into(),
        },
        crate::store::DialogSize::Medium,
    );
    let _ = app.frame();
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 79, 0));
    assert!(app.store.dialogs().is_empty(), "the outside release dismissed");
    // Props parity: the four v2 actions with descriptions.
    let props = select_props_for(
        &crate::store::DialogKind::MessageActions {
            session_id: "s1".into(),
            message_id: "u1".into(),
        },
        &app.store,
    );
    assert_eq!(props.title, "Message Actions");
    assert_eq!(
        props.options.iter().map(|o| o.value.as_str()).collect::<Vec<_>>(),
        vec!["message.jump", "session.revert", "message.copy", "session.fork"]
    );
}

/// #33 — the footer location label: hover lifts the label and a
/// release opens the Working directory select
/// (`useWorkingDirectoryActions`, prompt/index.tsx:1925-1937).
#[test]
fn footer_location_hover_and_click() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let mut app = App::new((80, 24));
    let clipboard = std::rc::Rc::new(crate::clipboard::RecordingClipboardSink::default());
    app.store.set_clipboard_sink(std::rc::Rc::clone(&clipboard) as std::rc::Rc<_>);
    // The hit rect is recorded by the SESSION render whenever the
    // label paints (route-gated since lane/if — a stale rect must
    // never fire on Home); the seam is exercised directly here on
    // the session route it belongs to.
    app.store.open_tab("s1", Some("S".to_owned()));
    app.store.navigate(Route::Session { session_id: "s1".to_owned() });
    app.store.set_footer_location_hit(Some(crate::store::FooterLocationHit {
        directory: "/tmp/opencode/project".into(),
        row: 22,
        x: 0,
        width: 26,
    }));
    // Hover: the lift arms.
    app.mouse_event(&mouse(MouseEventKind::Moved, 10, 22));
    assert!(app.store.location_hovered(), "the label lifts under the pointer");
    app.mouse_event(&mouse(MouseEventKind::Moved, 10, 21));
    assert!(!app.store.location_hovered(), "off the label the lift clears");
    // Release on the label: the Working directory select opens.
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 10, 22));
    let dialogs = app.store.dialogs();
    assert!(
        matches!(
            dialogs.last().map(|entry| &entry.kind),
            Some(crate::store::DialogKind::WorkingDirectory { directory }) if directory == "/tmp/opencode/project"
        ),
        "the working-directory select opened: {dialogs:?}"
    );
    // Copy path writes the clipboard + the info toast.
    let _ = app.frame(); // the render pass records the rects
    let rows = app.store.dialog_option_rows();
    let copy_row = u16::try_from(rows[0].unwrap_or_default()).unwrap_or(u16::MAX);
    app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 40, copy_row));
    assert!(app.store.dialogs().is_empty(), "Copy path closed the dialog");
    let last_write = clipboard.writes.lock().ok().and_then(|writes| writes.last().cloned());
    assert_eq!(last_write.as_deref(), Some("/tmp/opencode/project"));
}

// -- Wave A lane: the interaction-contract wiring ---------------------------

#[cfg(test)]
mod core_interaction_tests {
    use super::*;
    use crate::components::composer::{ComposerEditCommand, PromptDraft};
    use crate::components::message::model::{Message, PartTime};
    use crate::store::{InterruptStep, RecordingInterruptSink};

    /// A streaming assistant message (no finish, no idle marker) —
    /// the `session_running` derivation's running case.
    fn streaming_session(store: &crate::store::Store, session_id: &str) {
        store.set_messages(
            session_id,
            vec![
                Message::User {
                    id: "u1".into(),
                    text: "please answer".into(),
                    created: 0,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: "a1".into(),
                    agent: "build".into(),
                    model: "fixture/fixture".into(),
                    parts: vec![crate::components::message::model::Part::Text {
                        text: "par".into(),
                        time: PartTime::default(),
                    }],
                    finish: None,
                    error: None,
                    time: PartTime::default(),
                    tokens: None,
                    usage_total: None,
                },
            ],
        );
    }

    /// The 2-press escape interrupt (prompt/index.tsx:500-529): idle
    /// ignores; the first escape on a RUNNING turn arms (the footer
    /// hint flips to "again"); the second within the window FIRES the
    /// sink; after 5 s the arm expires and the cycle restarts.
    #[test]
    fn escape_interrupts_a_running_turn_after_two_presses() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        let sink = std::rc::Rc::new(RecordingInterruptSink::default());
        app.store.set_interrupt_sink(sink.clone());

        // Idle: the binding rejects (v2's `enabled: status() ===
        // "running"` gate) and the escape falls through untouched.
        assert_eq!(app.store.interrupt_pressed("s1", 0), InterruptStep::Ignored);
        app.press("escape");
        assert!(sink.calls.borrow().is_empty(), "idle never interrupts");

        // Running: arm → hint → fire.
        streaming_session(&app.store, "s1");
        assert!(app.store.session_running("s1"));
        assert_eq!(app.store.interrupt_pressed("s1", 100), InterruptStep::Armed);
        assert_eq!(
            app.store.interrupt_hint_armed("s1", 100),
            Some(true),
            "the footer hint shows the armed state"
        );
        assert_eq!(app.store.interrupt_pressed("s1", 200), InterruptStep::Fired);
        assert_eq!(*sink.calls.borrow(), vec!["s1".to_owned()], "the sink fired once");

        // The window expires: a late second press re-arms instead.
        assert_eq!(app.store.interrupt_pressed("s1", 300), InterruptStep::Armed);
        assert_eq!(
            app.store.interrupt_pressed("s1", 300 + crate::store::INTERRUPT_ARM_WINDOW_MS),
            InterruptStep::Armed,
            "the expired arm restarts the cycle"
        );
    }

    /// The interrupt hint renders in the composer's footer row while
    /// a turn runs (`PromptInterruptStatus`, prompt/index.tsx:137-144).
    #[test]
    fn running_turn_shows_the_interrupt_hint_in_the_footer() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        let idle = app.frame_text();
        assert!(!idle.contains("esc interrupt"), "idle hides the hint: {idle}");
        streaming_session(&app.store, "s1");
        app.press("escape"); // arm
        let armed = app.frame_text();
        assert!(armed.contains("esc again to interrupt"), "armed hint: {armed}");
    }

    /// The scroll commands move the transcript (v2's six + first/last,
    /// routes/session/index.tsx:763-887): sticky starts at the bottom;
    /// page-up frees (older rows surface); page-down re-sticks.
    #[test]
    fn scroll_keys_move_the_transcript_and_restick_at_bottom() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("Long".to_owned()));
        // A transcript much taller than the viewport: 20 turns.
        let mut messages = Vec::new();
        for index in 0..20 {
            messages.push(Message::User {
                id: format!("u{index}"),
                text: format!("turn {index} of the long transcript"),
                created: index,
                files: Vec::new(),
            });
            messages.push(Message::Assistant {
                id: format!("a{index}"),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: vec![crate::components::message::model::Part::Text {
                    text: format!("answer {index}"),
                    time: PartTime::default(),
                }],
                finish: Some("stop".into()),
                error: None,
                time: PartTime::default(),
                tokens: None,
                usage_total: None,
            });
        }
        app.store.set_messages("s1", messages);
        // Sticky: the frame records the metrics; the tail shows.
        let tail = app.frame_text();
        assert!(tail.contains("answer 19"), "sticky at the bottom: {tail}");
        assert!(!tail.contains("turn 0 of"), "the head is out of view: {tail}");
        // pageup frees the follow — the tail leaves the view (a page
        // is HALF the viewport, index.tsx:769: `-scroll.height / 2`).
        app.press("pageup");
        let up = app.frame_text();
        assert!(!up.contains("answer 19"), "the tail left the view: {up}");
        // Enough page-ups reach the head (clamped at offset 0).
        for _ in 0..40 {
            app.press("pageup");
        }
        let head = app.frame_text();
        assert!(head.contains("turn 0 of"), "repeated page-ups reveal the head: {head}");
        // end (session.last) re-sticks: the tail returns.
        app.press("end");
        let stuck = app.frame_text();
        assert!(stuck.contains("answer 19"), "end re-sticks to the tail: {stuck}");
        // home (session.first) jumps to the top.
        app.press("home");
        let top = app.frame_text();
        assert!(top.contains("turn 0 of"), "home jumps to the top: {top}");
        // pagedown returns one page (still free, off the very bottom).
        app.press("pagedown");
        let middle = app.frame_text();
        assert!(!middle.contains("turn 0 of"), "a page down leaves the head: {middle}");
    }

    /// The queued prompts (mini/queue.rs lifted): `<leader>return`
    /// over an ACTIVE turn queues behind it (the dock row appears);
    /// the turn going idle delivers the head; empty-enter steers.
    #[test]
    fn leader_return_queues_and_the_queue_drains_serially() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        let sink = std::rc::Rc::new(crate::components::composer::RecordingPromptSink::default());
        app.store.set_prompt_sink(sink.clone());

        // An idle session: <leader>return sends immediately.
        for key in ["h", "i"] {
            app.press(key);
        }
        app.press("ctrl+x");
        app.press("return");
        assert_eq!(sink.submissions.borrow().len(), 1, "idle sends");
        assert_eq!(app.store.queued_prompts("s1"), Vec::<crate::store::QueuedPrompt>::new());
        assert_eq!(app.store.prompt_draft("s1").text, "", "the draft cleared");

        // A running turn: the same chord queues (the Admit path).
        streaming_session(&app.store, "s1");
        for key in ["n", "e", "x", "t"] {
            app.press(key);
        }
        let before = app.frame_text();
        assert!(!before.contains("queued"), "no dock yet: {before}");
        app.press("ctrl+x");
        app.press("return");
        assert_eq!(sink.submissions.borrow().len(), 1, "nothing sent while running");
        assert_eq!(app.store.queued_prompts("s1").len(), 1);
        assert_eq!(app.store.prompt_draft("s1").text, "", "the draft cleared into the queue");
        let dock = app.frame_text();
        assert!(dock.contains("1 queued · next"), "the dock row: {dock}");

        // The turn goes idle: the drain delivers the head.
        let mut event =
            crate::data::BusEvent::new("session.idle", serde_json::json!({ "sessionID": "s1" }));
        event.id = "evt_idle1".into();
        event.created = Some(10);
        app.store.ingest_all([crate::data::Ingest::Event(event)]);
        assert!(app.store.queued_prompts("s1").is_empty(), "the idle drained the queue");
        assert_eq!(sink.submissions.borrow().len(), 2, "the head delivered");
        assert_eq!(sink.submissions.borrow()[1].1.text, "next");
    }

    /// Empty-enter steering (v2 `onEmptySubmit` → steer): an empty
    /// submit with queued prompts promotes the head immediately.
    #[test]
    fn empty_enter_steers_the_queued_head() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        let sink = std::rc::Rc::new(crate::components::composer::RecordingPromptSink::default());
        app.store.set_prompt_sink(sink.clone());
        streaming_session(&app.store, "s1");
        for key in ["q", "u", "e", "u", "e"] {
            app.press(key);
        }
        app.press("ctrl+x");
        app.press("return");
        assert_eq!(app.store.queued_prompts("s1").len(), 1);
        // Empty enter steers.
        app.press("enter");
        assert!(app.store.queued_prompts("s1").is_empty(), "steered");
        assert_eq!(sink.submissions.borrow().len(), 1);
        assert_eq!(sink.submissions.borrow()[0].1.text, "queue");
    }

    /// The queued-prompts manager (`<leader>q`, v2 index.tsx:627-675):
    /// rows with the `N of M` footer, ctrl+d deletes (the last
    /// removal closes), enter steers; an EMPTY queue never opens.
    #[test]
    fn leader_q_opens_the_queue_manager_and_ctrl_d_deletes() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.queue_prompt("s1", "first queued");
        app.store.queue_prompt("s1", "second queued");
        app.press("ctrl+x");
        app.press("q");
        let dialog = app.frame_text();
        assert!(dialog.contains("Queued prompts"), "the manager title: {dialog}");
        assert!(dialog.contains("2 of 2"), "the index footer: {dialog}");
        // ctrl+d deletes the selected row (the first).
        app.press("ctrl+d");
        assert_eq!(app.store.queued_prompts("s1").len(), 1);
        assert!(app.store.dialogs().len() == 1, "still open with rows left");
        // enter steers the remaining row.
        app.press("enter");
        assert_eq!(app.store.queued_prompts("s1"), Vec::<crate::store::QueuedPrompt>::new());
        assert!(app.store.dialogs().is_empty(), "the last action closed the manager");
        // An empty queue is a no-op (v2's enabled gate).
        app.press("ctrl+x");
        app.press("q");
        assert!(app.store.dialogs().is_empty(), "empty queue never opens");
    }

    /// The unqueueable drafts warn (v2 index.tsx:1134-1136).
    #[test]
    fn unqueueable_drafts_warn() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        streaming_session(&app.store, "s1");
        for key in ["/", "h", "e", "l", "p"] {
            app.press(key);
        }
        app.press("ctrl+x");
        app.press("return");
        let frame = app.frame_text();
        assert!(frame.contains("This prompt cannot be queued"), "the warning toast: {frame}");
        assert_eq!(app.store.queued_prompts("s1"), Vec::<crate::store::QueuedPrompt>::new());
    }

    /// shift+tab cycles the visible agents (v2 local.tsx:114-124) and
    /// the footer hint flips (the composer statusline's agent label).
    #[test]
    fn shift_tab_cycles_agents_visibly() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.set_agents(crate::store::ListState::Loaded(vec![
            crate::store::ServerAgent {
                id: "build".into(),
                name: "Build".into(),
                description: None,
                mode: "all".into(),
                hidden: false,
            },
            crate::store::ServerAgent {
                id: "plan".into(),
                name: "Plan".into(),
                description: None,
                mode: "all".into(),
                hidden: false,
            },
            crate::store::ServerAgent {
                id: "secret".into(),
                name: "Secret".into(),
                description: None,
                mode: "subagent".into(),
                hidden: false,
            },
        ]));
        app.store.select_agent("s1", "build");
        let before = app.frame_text();
        assert!(before.contains("Build"), "the initial hint: {before}");
        app.press("shift+tab");
        let after = app.frame_text();
        assert!(after.contains("Plan"), "cycled to Plan: {after}");
        assert!(!after.contains("Build ·"), "Build left the hint: {after}");
        assert!(!after.contains("Secret"), "subagents never cycle: {after}");
        // Wraps: build → plan → build.
        app.press("shift+tab");
        let wrapped = app.frame_text();
        assert!(wrapped.contains("Build"), "wrapped back to Build: {wrapped}");
        assert!(!wrapped.contains("Plan"), "Plan left the hint: {wrapped}");
    }

    /// f2 cycles the recent model ring (v2 local.tsx:412-423) and the
    /// footer hint flips; the pick lands on the session's selection.
    #[test]
    fn f2_cycles_recent_models_visibly() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.set_models(
            crate::store::ListState::Loaded(vec![
                crate::store::ServerModel {
                    provider_id: "anthropic".into(),
                    id: "claude".into(),
                    name: "Claude".into(),
                    cost_input: 0.0,
                    free: false,
                    deprecated: false,
                    variants: Vec::new(),
                    released: 0.0,
                    context_limit: None,
                },
                crate::store::ServerModel {
                    provider_id: "openai".into(),
                    id: "gpt".into(),
                    name: "GPT".into(),
                    cost_input: 0.0,
                    free: false,
                    deprecated: false,
                    variants: Vec::new(),
                    released: 0.0,
                    context_limit: None,
                },
            ]),
            Vec::new(),
        );
        app.store.select_model(
            "s1",
            &crate::store::ModelSelection {
                provider_id: "anthropic".into(),
                model_id: "claude".into(),
                variant: None,
            },
        );
        app.store.model_add_recent("anthropic", "claude");
        app.store.model_add_recent("openai", "gpt");
        let before = app.frame_text();
        // Lane/chrome #5: the model now rides the composer's METADATA
        // row under its DISPLAY NAME (metadata.tsx — the ref shows
        // `Build · Claude anthropic`, never the bare model id).
        assert!(before.contains("Claude"), "the initial model hint: {before}");
        app.press("f2");
        let after = app.frame_text();
        assert!(after.contains("GPT"), "f2 stepped to GPT: {after}");
        assert!(!after.contains("Claude"), "Claude left the hint: {after}");
        // shift+f2 steps back (model.cycle_recent_reverse).
        app.press("shift+f2");
        assert!(app.frame_text().contains("Claude"), "shift+f2 stepped back");
    }

    /// The text-edit core's full command surface through the pump:
    /// selection, word motions, delete ops, undo/redo — every
    /// `input.*` binding in keybind.ts:206-243 has a real arm.
    #[test]
    fn input_commands_edit_the_draft_with_selection_and_undo() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        for key in ["h", "e", "l", "l", "o", " ", "w", "o", "r", "l", "d"] {
            app.press(key);
        }
        assert_eq!(app.store.prompt_draft("s1").text, "hello world");
        // alt+b (word back) + alt+d (delete word forward).
        app.press("alt+b");
        app.press("alt+d");
        assert_eq!(app.store.prompt_draft("s1").text, "hello ");
        // ctrl+w (delete word backward) removes "hello ".
        app.press("ctrl+w");
        assert_eq!(app.store.prompt_draft("s1").text, "");
        // ctrl+- (input.undo) restores twice.
        app.press("ctrl+-");
        assert_eq!(app.store.prompt_draft("s1").text, "hello ");
        app.press("ctrl+-");
        assert_eq!(app.store.prompt_draft("s1").text, "hello world");
        // ctrl+. (input.redo) re-deletes.
        app.press("ctrl+.");
        assert_eq!(app.store.prompt_draft("s1").text, "hello ");
        // Selection: shift+left selects; typing replaces the selection.
        app.press("shift+left");
        app.press("shift+left");
        let composer = app.store.composer("s1");
        assert_eq!(composer.selection(), Some(4..6), "the trailing space+o selected");
        drop(composer);
        app.press("!");
        assert_eq!(app.store.prompt_draft("s1").text, "hell!", "the selection replaced");
    }

    /// The composer applies every [`ComposerEditCommand`] arm (the
    /// parity check: `apply_edit` accepts the whole v2 `input.*`
    /// surface).
    #[test]
    fn composer_applies_the_full_edit_command_surface() {
        let mut composer = crate::components::composer::Composer::default();
        composer.set(PromptDraft { text: "one two".into(), ..PromptDraft::default() });
        for command in [
            ComposerEditCommand::MoveLeft,
            ComposerEditCommand::LineHome,
            ComposerEditCommand::SelectRight,
            ComposerEditCommand::DeleteToLineEnd,
            ComposerEditCommand::SelectAll,
            ComposerEditCommand::BufferHome,
            ComposerEditCommand::BufferEnd,
        ] {
            assert!(composer.apply_edit(command), "{command:?} applies");
        }
        // "one two" → SelectRight (0..1) → DeleteToLineEnd from 1
        // leaves the first char.
        assert_eq!(composer.draft.text, "o");
        assert!(composer.undo(), "undo restores");
        assert_eq!(composer.draft.text, "one two");
    }

    /// F11 — the error panel consumes input until closed: while an
    /// ERROR-variant toast is live, typed keys and Enter must NOT
    /// reach the composer (no draft changes, no submit); escape
    /// dismisses the panel and typing resumes. Info toasts stay
    /// transparent (v2's toast never blocks typing).
    #[test]
    fn error_toast_consumes_keys_until_dismissed() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));

        // The failure surface: an error toast (the "Session failed"
        // panel class).
        app.store.toast_show(crate::components::toast::ToastOptions {
            title: Some("Session failed".to_owned()),
            message: "No model is available".to_owned(),
            variant: "error",
            duration_ms: crate::components::toast::TOAST_DEFAULT_DURATION_MS,
            action_label: None,
        });
        assert!(app.error_toast_live(), "the error panel is up");

        // Type + Enter: everything consumed, nothing submitted.
        for spec in ["h", "i"] {
            app.press(spec);
        }
        app.press("enter");
        assert!(app.store.prompt_draft("s1").is_empty(), "keystrokes never reached the composer");
        assert!(
            app.store.composer("s1").history.entries.is_empty(),
            "enter did not submit under the error panel"
        );
        assert!(app.error_toast_live(), "the panel is still up (consumed keys do not dismiss)");

        // Escape dismisses; typing reaches the composer again.
        app.press("escape");
        assert!(!app.error_toast_live(), "escape closed the panel");
        app.press("o");
        app.press("k");
        assert_eq!(app.store.prompt_draft("s1").text, "ok", "typing resumed after dismissal");

        // An INFO toast never blocks (the copy-success class).
        app.store.toast_show(crate::components::toast::ToastOptions::info("copied"));
        app.press("backspace");
        app.press("backspace");
        assert!(app.store.prompt_draft("s1").is_empty(), "info toasts stay transparent");
    }

    /// v2's overlay-sidebar close layer (routes/session/index.tsx:
    /// 243-245): at a narrow width the manually-opened sidebar floats
    /// over a scrim that hides the composer — escape and ctrl+c must
    /// close it (priority 10, above every route layer), restoring the
    /// composer rail. The `pty_fuzz` recoverability invariant depends on
    /// this (the pre-fix dead end: no `┃`, no visible echo).
    #[test]
    fn escape_closes_the_overlay_sidebar_back_to_the_composer() {
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("Bug hunt".to_owned()));

        // <leader>b at 80 cols: the manual overlay — the sidebar shows
        // and the opaque scrim covers the composer (no `┃` anywhere).
        app.press("ctrl+x");
        app.press("b");
        assert!(app.store.sidebar_shown("s1", 80), "the manual overlay is up");
        assert!(!app.frame_text().contains('┃'), "the scrim hides the composer");

        // Escape closes it (v2 binds "escape,ctrl+c" → onToggleSidebar)
        // and the composer rail returns.
        app.press("escape");
        assert!(!app.store.sidebar_shown("s1", 80), "escape closed the overlay");
        assert!(app.frame_text().contains('┃'), "the composer is back");

        // Re-open: ctrl+c closes it too — and unlike escape it must
        // still be able to reach `app.exit` once the overlay is down
        // (the chord order the exit protocol relies on).
        app.press("ctrl+x");
        app.press("b");
        assert!(app.store.sidebar_shown("s1", 80));
        app.press("ctrl+c");
        assert!(!app.store.sidebar_shown("s1", 80), "ctrl+c closed the overlay");
        assert!(!app.exit_requested, "the first ctrl+c closes, not exits");
        assert!(app.frame_text().contains('┃'), "the composer is back again");
    }

    /// The close layer stays armed ONLY for the floating overlay: at a
    /// wide width the docked sidebar leaves escape to the normal
    /// prompt/dialog chain (v2's `<= 120` width guard).
    #[test]
    fn escape_does_not_close_the_wide_docked_sidebar() {
        let mut app = App::new((160, 24));
        app.store.open_tab("s1", Some("Bug hunt".to_owned()));
        // 160 cols: auto mode already docks the pane.
        assert!(app.store.sidebar_shown("s1", 160), "the wide pane docks");
        app.press("escape");
        assert!(app.store.sidebar_shown("s1", 160), "escape left the docked pane alone");
    }

    // -- lane/if item 2: hit-registry lifetime + route gating (auditA #2) --

    /// A staged session with one reasoning group (the transcript hit
    /// rows' source) — shared by the phantom-click proofs.
    fn staged_reasoning_session(app: &mut App) {
        use crate::components::message::model::{Message, Part, PartTime};
        app.store.set_messages(
            "s1",
            vec![
                Message::User {
                    id: "u1".into(),
                    text: "think".into(),
                    created: 0,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: "a1".into(),
                    agent: "build".into(),
                    model: "fixture/fixture".into(),
                    parts: vec![Part::Reasoning {
                        text: "Pondering the parser edges".into(),
                        time: PartTime { created: Some(1), completed: Some(2) },
                    }],
                    finish: Some("success".into()),
                    error: None,
                    time: PartTime { created: Some(1), completed: Some(2) },
                    tokens: None,
                    usage_total: None,
                },
            ],
        );
        app.store.open_tab("s1", Some("Thoughts".to_owned()));
        // The footer location label (its hit rect's source): v2's
        // `{cwd}:{branch}` — empty label ⇒ no rect.
        app.store.set_footer_location("/w:main:feature".to_owned());
        // The session render records the hit rows + the footer rect.
        let _ = app.frame_text();
    }

    /// Route change disarms the session's pointer registries (auditA
    /// §4.1): `navigate` is the only `route.set` site — leaving the
    /// session clears the transcript rows, the footer-location rect
    /// and the onboarding card, exactly like v2's renderables
    /// unmounting with the route.
    #[test]
    fn route_change_clears_the_pointer_registries() {
        let mut app = App::new((80, 24));
        staged_reasoning_session(&mut app);
        assert!(!app.store.transcript_hit_rows().is_empty(), "the session render armed rows");
        assert!(
            app.store.footer_location_hit().is_some(),
            "the session render armed the footer rect"
        );
        app.store.navigate(Route::Home);
        assert!(
            app.store.transcript_hit_rows().is_empty(),
            "navigate cleared the transcript rows"
        );
        assert!(app.store.footer_location_hit().is_none(), "navigate cleared the footer rect");
        assert!(app.store.sidebar_onboarding_hits().is_none(), "the onboarding card disarmed");
    }

    /// The phantom-click negative: after Session → Home, a release on
    /// the coordinates the stale rows used to occupy toggles nothing
    /// and opens no dialog — twice over (the registry clear AND the
    /// route gate in `mouse_event`, so either regressing alone stays
    /// pinned).
    #[test]
    fn home_release_fires_no_stale_transcript_or_location_hits() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new((80, 24));
        staged_reasoning_session(&mut app);
        let stale = app
            .store
            .transcript_hit_rows()
            .first()
            .cloned()
            .expect("the session armed a hit row");
        let stale_footer = app.store.footer_location_hit().expect("the footer rect armed");
        app.store.navigate(Route::Home);

        // The cleared registry: nothing under the old rects.
        let row = u16::try_from(stale.y).unwrap_or(u16::MAX);
        app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 10, row));
        assert!(
            !app.store.group_expanded(&stale.session_id, "group:Reasoning:a1:reasoning:0"),
            "no stale toggle fired on home"
        );
        assert!(app.store.dialogs().is_empty(), "no dialog opened on home");

        // The route gate, exercised directly: a re-injected stale row
        // (the registry clear's failure mode) still cannot fire — the
        // release only lands while the row's session is the route.
        app.store.set_transcript_hit_rows(vec![stale]);
        app.store.set_footer_location_hit(Some(stale_footer.clone()));
        app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 10, row));
        app.mouse_event(
            &mouse(
                MouseEventKind::Up(MouseButton::Left),
                u16::try_from(stale_footer.x).unwrap_or(u16::MAX),
                u16::try_from(stale_footer.row).unwrap_or(u16::MAX),
            ),
        );
        assert!(
            !app.store.group_expanded(&"s1".to_owned(), "group:Reasoning:a1:reasoning:0"),
            "the route gate blocked the re-injected row"
        );
        assert!(
            app.store.dialogs().is_empty(),
            "the route gate blocked the stale footer rect (no WorkingDirectory dialog)"
        );
        // A hover over the stale footer rect cannot flip the home
        // label's hover state either.
        app.mouse_event(
            &mouse(
                MouseEventKind::Moved,
                u16::try_from(stale_footer.x).unwrap_or(u16::MAX),
                u16::try_from(stale_footer.row).unwrap_or(u16::MAX),
            ),
        );
        assert!(!app.store.location_hovered(), "the stale rect cannot arm hover on home");
    }

    // -- lane/if item 1: the wheel (auditA #1) ------------------------------

    /// `Math.round` with halves toward +∞ (opentui's js_round — the
    /// E1 scroll math's rounding rule).
    fn js_round(value: f64) -> usize {
        value.floor() as usize + usize::from(value.fract() >= 0.5)
    }

    /// The transcript wheel runs the E1 half-page math (auditA #1 +
    /// ROADMAP wave E1): one notch = `session.page.up/down` —
    /// ±viewport/2 with js_round (15-row viewport: −7/+8), sticky
    /// re-arming at the tail — and lands EXACTLY where the page keys
    /// land.
    #[test]
    fn wheel_scrolls_the_transcript_half_a_page_per_notch() {
        use crate::components::message::model::{Message, Part, PartTime};
        use crossterm::event::MouseEventKind;
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("Long".to_owned()));
        let mut messages = Vec::new();
        for index in 0..20 {
            messages.push(Message::User {
                id: format!("u{index}"),
                text: format!("turn {index} of the long transcript"),
                created: index,
                files: Vec::new(),
            });
            messages.push(Message::Assistant {
                id: format!("a{index}"),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: vec![Part::Text { text: format!("answer {index}"), time: PartTime::default() }],
                finish: Some("stop".into()),
                error: None,
                time: PartTime::default(),
                tokens: None,
                usage_total: None,
            });
        }
        app.store.set_messages("s1", messages);
        let sticky = app.frame_text();
        assert!(sticky.contains("answer 19"), "sticky at the tail: {sticky}");
        let metrics = app.store.transcript_metrics("s1");
        let viewport = metrics.viewport;
        let max = metrics.content_height.saturating_sub(viewport);
        assert_eq!(app.store.scroll_offset("s1"), max, "sticky sits at the maximum");

        // One wheel notch up: -(viewport/2) with js_round (half toward
        // +∞) — the E1 numbers (15-row viewport ⇒ −7).
        app.mouse_event(&mouse(MouseEventKind::ScrollUp, 40, 10));
        let expected = js_round(max as f64 - viewport as f64 / 2.0);
        assert_eq!(
            app.store.scroll_offset("s1"),
            expected,
            "wheel-up = pageup's half-page math (viewport {viewport}, max {max})"
        );
        let freed = app.frame_text();
        assert!(!freed.contains("answer 19"), "the tail left the view: {freed}");

        // The wheel and the KEY land on the same offset: re-stick,
        // then pageup — identical math, identical result.
        app.press("end");
        assert_eq!(app.store.scroll_offset("s1"), max, "end re-sticks");
        app.press("pageup");
        assert_eq!(
            app.store.scroll_offset("s1"),
            expected,
            "the page key and the wheel agree"
        );

        // Wheel down from the head: +(viewport/2) js_round (⇒ +8) —
        // then repeated notches re-reach the tail (sticky re-arms).
        for _ in 0..8 {
            app.mouse_event(&mouse(MouseEventKind::ScrollDown, 40, 10));
        }
        let bottom = app.frame_text();
        assert!(
            bottom.contains("answer 19"),
            "enough wheel notches re-stick to the tail: {bottom}"
        );
        assert_eq!(app.store.scroll_offset("s1"), max, "sticky at the maximum again");
    }

    /// The dialog wheel scrolls the option WINDOW, never the
    /// selection (v2's dialog lists are scrollboxes; the rows keep
    /// their own pointer tracking) — 3 rows a notch (opentui's
    /// terminal wheel delta), clamped at both ends.
    #[test]
    fn wheel_scrolls_the_dialog_window_without_moving_the_selection() {
        use crate::store::DialogKind;
        let mut app = App::new((80, 24));
        // The command palette: the registry is long enough to window
        // at 24 rows (viewport = 24/2 − 6 = 6); the frame records the
        // rows, the selection starts at 0.
        app.store.open_dialog(DialogKind::CommandPalette, crate::store::DialogSize::Medium);
        let _ = app.frame();
        let before = app
            .store
            .top_dialog()
            .map(|entry| (entry.select.selected, entry.select.scroll))
            .expect("the dialog is open");
        assert_eq!(before.0, 0, "the selection starts at the first row");
        assert!(
            app.store.dialog_option_rows().iter().any(Option::is_some),
            "the palette windows (rows recorded)"
        );

        app.mouse_event(&mouse(crossterm::event::MouseEventKind::ScrollDown, 40, 12));
        let after = app
            .store
            .top_dialog()
            .map(|entry| (entry.select.selected, entry.select.scroll))
            .expect("the dialog stays open");
        assert_eq!(after.0, 0, "the selection did NOT move");
        assert_eq!(after.1, before.1 + 3, "the window moved 3 rows (one notch)");
        // Up returns (clamped at 0, never negative).
        app.mouse_event(&mouse(crossterm::event::MouseEventKind::ScrollUp, 40, 12));
        app.mouse_event(&mouse(crossterm::event::MouseEventKind::ScrollUp, 40, 12));
        let back = app
            .store
            .top_dialog()
            .map(|entry| (entry.select.selected, entry.select.scroll))
            .expect("the dialog stays open");
        assert_eq!(back.1, 0, "the window clamped at the top");
        assert_eq!(back.0, 0, "the selection still did not move");
    }

    /// The sidebar wheel: over the pane's columns the content slot
    /// scrolls (3 a notch, clamped by content − viewport); outside
    /// them the same event falls through to the transcript.
    #[test]
    fn wheel_over_the_sidebar_columns_scrolls_the_content_slot() {
        use crossterm::event::MouseEventKind;
        let mut app = App::new((160, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.navigate(Route::Session { session_id: "s1".to_owned() });
        // A rendered docked pane records the geometry; force an
        // overflowing content slot (content 40 rows, viewport 10).
        app.store.set_sidebar_scroll_geometry(40, 10, Some((118, 42)));

        app.mouse_event(&mouse(MouseEventKind::ScrollDown, 130, 10));
        assert_eq!(app.store.sidebar_scroll().offset, 3, "one notch = 3 rows down");
        for _ in 0..9 {
            app.mouse_event(&mouse(MouseEventKind::ScrollDown, 130, 10));
        }
        assert_eq!(
            app.store.sidebar_scroll().offset,
            30,
            "clamped at content − viewport (40 − 10)"
        );
        app.mouse_event(&mouse(MouseEventKind::ScrollUp, 130, 10));
        assert_eq!(app.store.sidebar_scroll().offset, 27, "up steps back 3");
        // Outside the pane's columns the wheel belongs to the
        // transcript — the sidebar offset must not move.
        app.mouse_event(&mouse(MouseEventKind::ScrollDown, 10, 10));
        assert_eq!(app.store.sidebar_scroll().offset, 27, "the transcript wheel left the pane alone");
    }

    // -- lane/if item 3: the permission pointer arms (auditA #4) ----------

    /// Stage one permission ask (SSE ingest shape) with `save`
    /// patterns (the "Always allow" pill's gate).
    fn stage_permission_ask(app: &mut App, id: &str, save: serde_json::Value) {
        app.store.ingest_all([crate::data::Ingest::Event(crate::data::BusEvent::new(
            "permission.asked",
            serde_json::json!({
                "id": id,
                "sessionID": "ses",
                "action": "read",
                "resources": ["src/index.ts"],
                "save": save,
            }),
        ))]);
    }

    /// The pills move the selection on hover and RUN on release (v2
    /// permission.tsx:583-587: `onMouseMove` selects, `onMouseUp`
    /// selects + `onSelect`): Allow-once replies `once` through the
    /// same queue the keyboard enter uses and resolves the ask.
    #[test]
    fn permission_allow_once_via_pointer() {
        use crate::components::footer::permission::PermissionOption;
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.navigate(Route::Session { session_id: "s1".to_owned() });
        stage_permission_ask(&mut app, "p1", serde_json::json!([]));
        assert!(app.permission_live(), "the ask holds the screen");
        let _ = app.frame();
        let hits = app.store.permission_pointer_hits().expect("the render armed the pills");
        assert_eq!(hits.row, 22, "the controls row at size.1 − 2");
        let pills: Vec<(PermissionOption, (usize, usize))> = hits.pills.clone();
        assert_eq!(pills.len(), 2, "no save patterns ⇒ no Always pill: {pills:?}");
        assert_eq!(pills[0].0, PermissionOption::Once, "Allow once leads");
        assert_eq!(pills[1].0, PermissionOption::Reject);

        // Hover the Reject pill: the selection tracks the pointer.
        let (_, (reject_x, reject_width)) = pills[1];
        let mid = reject_x + reject_width / 2;
        app.mouse_event(&mouse(MouseEventKind::Moved, u16::try_from(mid).unwrap_or(u16::MAX), 22));
        assert_eq!(
            app.permission_states.get("p1").map(|state| state.selected),
            Some(PermissionOption::Reject),
            "pointer motion selected the Reject pill"
        );

        // Release on the ALLOW-ONCE pill: the run replies `once` and
        // resolves the ask (the keyboard enter path with the
        // selection set by the release itself).
        let (once_x, once_width) = pills[0].1;
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            u16::try_from(once_x + once_width / 2).unwrap_or(u16::MAX),
            22,
        ));
        assert_eq!(app.permission_replies.len(), 1, "one reply");
        assert_eq!(app.permission_replies[0].decision, "once");
        assert_eq!(app.permission_replies[0].request_id, "p1");
        assert!(!app.permission_live(), "the ask resolved locally");
        assert!(
            app.store.permission_pointer_hits().is_none(),
            "the resolved ask left no zombie rects"
        );
    }

    /// The reject flow through the pointer: the Reject pill opens the
    /// reject stage (no reply), `enter confirm` sends the reject
    /// reply, `esc cancel` returns to the ask — v2's two hint
    /// BUTTONS (permission.tsx:375-400), not dead labels.
    #[test]
    fn permission_reject_confirm_and_cancel_via_pointer() {
        use crate::components::footer::permission::{PermissionOption, PermissionStage};
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new((80, 24));
        app.store.open_tab("s1", Some("S".to_owned()));
        app.store.navigate(Route::Session { session_id: "s1".to_owned() });
        stage_permission_ask(&mut app, "p2", serde_json::json!(["src/**"]));
        let _ = app.frame();
        let hits = app.store.permission_pointer_hits().expect("armed");
        let pills: Vec<(PermissionOption, (usize, usize))> = hits.pills.clone();
        assert_eq!(pills.len(), 3, "save patterns arm the Always pill: {pills:?}");
        // Reject is the third pill: release opens the reject stage.
        let (reject_x, reject_width) = pills[2].1;
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            u16::try_from(reject_x + reject_width / 2).unwrap_or(u16::MAX),
            22,
        ));
        assert!(app.permission_replies.is_empty(), "the reject stage sends nothing yet");
        let staged = app.permission_states.get("p2").cloned().expect("staged");
        assert_eq!(staged.stage, PermissionStage::Reject, "the pill run opened the stage");
        let _ = app.frame();
        let hits = app.store.permission_pointer_hits().expect("re-armed for the stage");
        let (confirm_x, confirm_width) = hits.confirm.expect("the confirm button armed");
        let (cancel_x, cancel_width) = hits.cancel.expect("the cancel button armed");
        assert!(confirm_x + confirm_width < cancel_x, "the buttons sit in the hint block");

        // `esc cancel` first: back to the ask, selection on Reject.
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            u16::try_from(cancel_x + cancel_width / 2).unwrap_or(u16::MAX),
            22,
        ));
        let cancelled = app.permission_states.get("p2").cloned().expect("staged");
        assert_eq!(cancelled.stage, PermissionStage::Permission, "cancel returned to the ask");
        assert_eq!(cancelled.selected, PermissionOption::Reject);

        // Re-open and `enter confirm`: the reject reply fires.
        let _ = app.frame();
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            u16::try_from(reject_x + reject_width / 2).unwrap_or(u16::MAX),
            22,
        ));
        let _ = app.frame();
        let hits = app.store.permission_pointer_hits().expect("re-armed");
        let (confirm_x, confirm_width) = hits.confirm.expect("the confirm button armed");
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            u16::try_from(confirm_x + confirm_width / 2).unwrap_or(u16::MAX),
            22,
        ));
        assert_eq!(app.permission_replies.len(), 1, "the confirm button sent the reply");
        assert_eq!(app.permission_replies[0].decision, "reject");
        assert!(!app.permission_live(), "the ask resolved");

        // A release on the transcript rows under a live ask stays
        // dead (the early return that protects the transcript must
        // not fire anything).
        stage_permission_ask(&mut app, "p3", serde_json::json!([]));
        let _ = app.frame();
        app.mouse_event(&mouse(MouseEventKind::Up(MouseButton::Left), 10, 5));
        assert!(app.permission_live(), "a stray transcript release toggled nothing");
        assert_eq!(app.permission_replies.len(), 1, "no extra reply");
    }

    // -- lane/if item 4: all five disclosure surfaces (auditA #5) --------

    /// The Exploration group's header arms and toggles (v2
    /// group-view.tsx:152 — `onMouseUp={toggle}` on the InlineToolRow;
    /// previously only `group:Reasoning:` rows had hit rows, so the
    /// affordance rendered but swallowed clicks).
    #[test]
    fn exploration_group_header_toggles() {
        use crate::components::message::model::{Message, Part, PartTime, ToolState};
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new((80, 24));
        app.store.set_messages(
            "s1",
            vec![
                Message::User {
                    id: "u1".into(),
                    text: "look".into(),
                    created: 0,
                    files: Vec::new(),
                },
                Message::Assistant {
                    id: "a1".into(),
                    agent: "build".into(),
                    model: "fixture/fixture".into(),
                    parts: vec![
                        Part::Tool {
                            id: "tool-read".into(),
                            name: "read".into(),
                            time: PartTime { created: Some(1), completed: Some(2) },
                            state: ToolState::default(),
                        },
                        Part::Text {
                            text: "done".into(),
                            time: PartTime::default(),
                        },
                    ],
                    finish: Some("success".into()),
                    error: None,
                    time: PartTime { created: Some(1), completed: Some(3) },
                    tokens: None,
                    usage_total: None,
                },
            ],
        );
        app.store.open_tab("s1", Some("Explore".to_owned()));
        let frame = app.frame_text();
        assert!(frame.contains("Explored"), "the exploration header renders: {frame}");
        let hit = app
            .store
            .transcript_hit_rows()
            .iter()
            .find(|row| matches!(
                &row.action,
                crate::store::TranscriptHitAction::ToggleGroup(key) if key.starts_with("group:Exploration:")
            ))
            .expect("the exploration header recorded a hit row")
            .clone();
        let key = hit.action.toggle_key().expect("a group toggle").to_owned();
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            10,
            u16::try_from(hit.y).unwrap_or(u16::MAX),
        ));
        assert!(app.store.group_expanded(&hit.session_id, &key), "the release armed the disclosure");
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            10,
            u16::try_from(hit.y).unwrap_or(u16::MAX),
        ));
        assert!(
            !app.store.group_expanded(&hit.session_id, &key),
            "the second release collapsed it (v2 setGroupExpanded semantics)"
        );
    }

    /// The thought rows toggle their own disclosure (v2's minimal-mode
    /// ReasoningHeader, message-parts.tsx:66-86): the `thought:` row's
    /// first line is the hit target; collapsed renders the one-line
    /// `+ Thought: {title}` header, expanded the bordered body.
    #[test]
    fn thought_rows_toggle_their_disclosure() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mut app = App::new((80, 24));
        staged_reasoning_session(&mut app);
        // Expand the reasoning group (its header is the wired toggle).
        let group_hit = app
            .store
            .transcript_hit_rows()
            .iter()
            .find(|row| matches!(
                &row.action,
                crate::store::TranscriptHitAction::ToggleGroup(key) if key.starts_with("group:Reasoning:")
            ))
            .cloned()
            .expect("the group header hit");
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            10,
            u16::try_from(group_hit.y).unwrap_or(u16::MAX),
        ));
        let expanded = app.frame_text();
        assert!(expanded.contains("Pondering the parser edges"), "the body shows: {expanded}");

        // The thought row is armed as its own toggle (the first
        // body line's glyphs).
        let thought_hit = app
            .store
            .transcript_hit_rows()
            .iter()
            .find(|row| matches!(
                &row.action,
                crate::store::TranscriptHitAction::ToggleThought { message_id, part_id }
                    if message_id == "a1" && part_id == "reasoning:0"
            ))
            .cloned()
            .expect("the thought row recorded a hit");
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            10,
            u16::try_from(thought_hit.y).unwrap_or(u16::MAX),
        ));
        assert!(
            !app.store.thought_expanded(&hit_session(&thought_hit), "a1", "reasoning:0"),
            "the release collapsed the thought"
        );
        let collapsed = app.frame_text();
        assert!(collapsed.contains("+ Thought"), "the collapsed header renders: {collapsed}");
        assert!(
            !collapsed.contains("Pondering the parser edges"),
            "the body is behind the toggle: {collapsed}"
        );
        // Release again: the body returns.
        let thought_hit = app
            .store
            .transcript_hit_rows()
            .iter()
            .find(|row| matches!(
                &row.action,
                crate::store::TranscriptHitAction::ToggleThought { .. }
            ))
            .cloned()
            .expect("the collapsed header re-recorded its hit");
        app.mouse_event(&mouse(
            MouseEventKind::Up(MouseButton::Left),
            10,
            u16::try_from(thought_hit.y).unwrap_or(u16::MAX),
        ));
        let reopened = app.frame_text();
        assert!(reopened.contains("Pondering the parser edges"), "the body returned: {reopened}");
    }

    /// The hit row's owning session (test helper).
    fn hit_session(hit: &crate::store::TranscriptHitRow) -> String {
        hit.session_id.clone()
    }
}
