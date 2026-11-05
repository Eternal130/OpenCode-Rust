//! The split-footer renderer lifecycle — port of
//! `packages/tui/src/mini/runtime.lifecycle.ts`.
//!
//! Boots the renderer in **split-footer mode**: raw mode, NO alternate
//! screen — a [`FOOTER_HEIGHT`] dock stays glued to the bottom of the
//! terminal while transcript rows are written into the terminal's
//! native scrollback above it. Writes the entry splash on boot, paints
//! the footer from the footer family
//! ([`crate::components::footer`]), and tears down in the reference's
//! order on close.
//!
//! ## Lane D2.3 — the true split-footer engine
//!
//! The retired lane-B4 thin seam (a hand-drawn dock + a CUP/scroll
//! `insert_before` simulation) is gone. The lifecycle now consumes the
//! opentui D1b split-footer family exactly the way the reference
//! boots it (`runtime.lifecycle.ts:152-205`):
//!
//! * the terminal session is an opentui [`Terminal`] in the non-alt
//!   arm — `use_alt_screen: false` + `reserve_surface_height:
//!   FOOTER_HEIGHT` (the `makeRoomForRendererOutput` boot push, so
//!   the shell's prior output scrolls up and the dock's rows free up
//!   at the bottom);
//! * the renderer is a real [`CliRenderer`] with `screen_mode:
//!   SplitFooter`, `footer_height: FOOTER_HEIGHT`,
//!   `external_output_mode: CaptureStdout` (the ref's
//!   `createCliRenderer` config, `runtime.lifecycle.ts:157-174`) —
//!   the capture gate every scrollback write/flush checks;
//! * transcript rows flow through `writeToScrollback` → the
//!   `EXTERNAL_OUTPUT` commit FIFO → `flushPendingSplitCommits`,
//!   whose newline-ownership bytes (the tail-column arithmetic,
//!   `renderer.ts:2497-2566`) write through the attached session's
//!   output — the engine's scrollback path, not a hand-rolled
//!   CUP-write;
//! * the dock rides the renderer's ROOT tree (lane D4.1c — the
//!   ref's `render(() => createComponent(RunFooterView, …),
//!   renderer)`, `footer.ts:326`): the pane's composed footer frame
//!   mounts through [`super::footer_tree`] as one signal-driven row
//!   list, the engine's band layout + `renderOffset` emission
//!   addresses the dock's rows (opentui rev `e52adf6`,
//!   `calculate_render_geometry` + `set_footer_height`), and the
//!   frame bytes leave through the terminal pump exit — the
//!   `?2026`-wrapped SGR stream (`Terminal::pump_output`). The lane-B
//!   "GridEmitter is the one frame writer" ruling retires here: mini
//!   emits NO grid-diffed bytes of its own, every frame byte flows
//!   through the renderer pump (the D2.1b "never pump" seam is dead).
//!
//! ADR-0018 §5's registered deviations flip with this: the
//! no-scrolling-regions `insert_before` semantics are replaced by the
//! engine's newline-ownership stream, and resize now re-renders (the
//! registered "mini resize does not reflow" (M3) deviation is
//! closed — see [`Lifecycle::poll_resize`]).
//!
//! ## Exit semantics (`runtime.lifecycle.ts:97-111` + `close`,
//! `:300-343`)
//!
//! 1. exit splash (when `showExit` and splash is shown) flushed to
//!    scrollback;
//! 2. footer `close()` → `destroy()`;
//! 3. `shutdown(renderer)`: leave split-footer mode, destroy;
//! 4. when no exit splash was written: `stdout.write("\n")`
//!    (`runtime.lifecycle.ts:339-341`).//!
//! The footer's key handling lives in [`FooterPane`] so the state
//! machine is unit-testable without a terminal.

#![allow(clippy::cast_possible_truncation)] // terminal dims sit far below u16
#![allow(clippy::match_same_arms)] // the key/vocabulary arms mirror the reference 1:1

use std::io::Write as _;
use std::time::{Duration, Instant};

use opentui_core::renderer::{
    create_cli_renderer, CliRendererConfig, ExternalOutputMode,
    RendererTerminalSession, ScreenMode,
};
use opentui_core::scrollback::ScrollbackSnapshot;

use super::footer_tree::FooterTree;
use super::scrollback::{format_commit_with, Commit};
use super::splash::{
    directory_label, entry_splash_layout, entry_splash_rows, exit_splash_rows, EntrySplashInput,
};
use super::{footer_base, FOOTER_HEIGHT, FORM_ROWS, PERMISSION_ROWS};
use crate::components::footer::form::{
    form_commit_input, form_move, form_pick, render_form_body, FormBodyState, FormReply,
};
use crate::components::footer::permission::{
    create_permission_body_state, permission_key, permission_type_message, render_permission_body,
    PermissionBodyState, PermissionKey, PermissionReply, PermissionRequest,
};
use crate::components::footer::prompt_idle::render_prompt_idle;
use crate::components::footer::prompt_idle::PromptIdleState;
use crate::components::footer::{FooterMachine, FooterView};

/// The footer pane's keypress vocabulary (the key-event mapping the
/// renderer performs on the parsed key spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterKey {
    Printable(char),
    Enter,
    Escape,
    Backspace,
    Up,
    Down,
    /// TUI-17 — the inspector's cycle key (`tab next`).
    Tab,
    PageUp,
    PageDown,
    CtrlC,
}

/// The side effects a keypress produced — the runtime seams
/// (`onPermissionReply` / `onFormReply` / `onFormCancel` /
/// prompt-submission / exit, `runtime.lifecycle.ts:70-82`).
#[derive(Debug, Clone, PartialEq)]
pub enum FooterEffect {
    Submit(String),
    PermissionReply(PermissionReply),
    FormReply(FormReply),
    FormCancel(String),
    Close,
    Interrupt,
    /// TUI-17 — interrupt the inspected CHILD session
    /// (`v2.session.interrupt(childID)`, footer.subagent's
    /// `composer.subagent.interrupt`).
    InterruptChild(String),
    HistoryUp,
    HistoryDown,
}

/// The footer state machine + per-view key handling (the pure half of
/// `RunFooter`, `mini/footer.ts`).
pub struct FooterPane {
    pub machine: FooterMachine,
    theme: crate::components::footer::FooterTheme,
    permission_states: std::collections::HashMap<String, PermissionBodyState>,
    form_states: std::collections::HashMap<String, FormBodyState>,
    form_drafts: std::collections::HashMap<String, String>,
    /// The composer draft.
    pub draft: String,
    pub agent: Option<String>,
    pub model_label: Option<String>,
    /// Two-press exit/interrupt (`footer.ts:1017-1041` / `:990-1015`):
    /// press counts + 5 s reset deadlines.
    exit_presses: u8,
    exit_deadline: Option<Instant>,
    interrupt_presses: u8,
    interrupt_deadline: Option<Instant>,
    /// Replies this session produced (assertions / the transport seam).
    pub permission_replies: Vec<PermissionReply>,
    pub form_replies: Vec<FormReply>,
    pub form_cancels: Vec<String>,
    /// CONFIG-T4 `mini.footer`: the statusline's informational groups
    /// render while set (v2's `footerDetails` memo,
    /// `footer.view.tsx:220`).
    pub footer_details: bool,
    /// CONFIG-T4 `mini.work_spinner`: the busy cell's animation id.
    pub work_spinner: String,
    /// CONFIG-T4 `mini.mono` (the render-side copy): the ASCII
    /// spinner swap (`animation={mono ? SEED_MONO : …}`,
    /// `footer.view.tsx:983`).
    pub mono: bool,
    /// CONFIG-T4: the busy-cell clock anchor (`started()`,
    /// `footer.view.tsx:988`'s `age` input) — `None` while idle.
    busy_since: Option<Instant>,
    /// RD-19 — the startup row (`footer.view.tsx:709-725`): while set,
    /// the pane grows by ONE launch row whose spinner cell runs the
    /// `SEED_LAUNCH` one-shot (`footer.ts:389-390`'s 800 ms window
    /// — `(21 - 1) * 40`). The lifecycle clears it and flushes the
    /// entry splash to scrollback when the animation completes.
    pub startup: Option<StartupRow>,
    /// TUI-17 — the tracked child sessions (the runtime's
    /// `SubagentTracker` snapshot; `footer.view.tsx:139-149`).
    pub subagent_children: Vec<super::transport::SubagentChild>,
    /// The inspected child (`route.sessionID`).
    subagent_selected: Option<String>,
    /// The detail scroll (rows from the bottom, sticky-bottom).
    subagent_scroll: usize,
}

/// RD-19 — the launch row's state (`footer.view.tsx:709-725`): the
/// clock anchor plus the `entrySplashLayout` label/metadata this row
/// paints (the spinner cell replaces the label's leading ▪ mark).
#[derive(Debug)]
pub struct StartupRow {
    pub since: Instant,
    /// `layout().label` — `▪ oc mini` when the mark fits.
    pub label: String,
    /// `layout().metadata` — ` v{version} · {detail}` when it fits.
    pub metadata: String,
}

/// `SEED_LAUNCH`'s one-shot window (`footer.ts:390`):
/// `(frames.length - 1) * interval` = 20 × 40 ms.
pub const STARTUP_DURATION_MS: u64 = (21 - 1) * 40;

impl FooterPane {
    #[must_use]
    pub fn new(theme: crate::components::footer::FooterTheme) -> Self {
        Self {
            machine: FooterMachine::new(),
            theme,
            permission_states: std::collections::HashMap::new(),
            form_states: std::collections::HashMap::new(),
            form_drafts: std::collections::HashMap::new(),
            draft: String::new(),
            agent: None,
            model_label: None,
            exit_presses: 0,
            exit_deadline: None,
            interrupt_presses: 0,
            interrupt_deadline: None,
            permission_replies: Vec::new(),
            form_replies: Vec::new(),
            form_cancels: Vec::new(),
            footer_details: true,
            work_spinner: "block-soft-slide".to_owned(),
            mono: false,
            busy_since: None,
            startup: None,
            subagent_children: Vec::new(),
            subagent_selected: None,
            subagent_scroll: 0,
        }
    }

    /// RD-19 — the launch one-shot's expiry (`footer.ts:390`'s
    /// setTimeout window): true once `STARTUP_DURATION_MS` have
    /// elapsed since the row mounted.
    #[must_use]
    pub fn startup_expired(&self) -> bool {
        self.startup
            .as_ref()
            .is_some_and(|row| row.since.elapsed().as_millis() >= u128::from(STARTUP_DURATION_MS))
    }

    /// The busy-cell state (`busy()` in `footer.view.tsx:980`): a
    /// running turn anchors the spinner clock; idle clears it. The
    /// anchor only moves on transitions so the animation never
    /// restarts mid-turn.
    pub fn set_busy(&mut self, busy: bool) {
        self.busy_since =
            if busy { self.busy_since.or_else(|| Some(Instant::now())) } else { None };
    }

    /// Expire armed two-press counters (`armExitTimer` /
    /// `armInterruptTimer`, `footer.ts:976-989`).
    fn expire_armed(&mut self) {
        if self.exit_deadline.is_some_and(|deadline| Instant::now() > deadline) {
            self.exit_presses = 0;
            self.exit_deadline = None;
        }
        if self.interrupt_deadline.is_some_and(|deadline| Instant::now() > deadline) {
            self.interrupt_presses = 0;
            self.interrupt_deadline = None;
        }
    }

    /// `permission.asked` fed through the machine (the store wiring).
    pub fn permission_asked(&mut self, request: PermissionRequest) {
        self.machine.permission_asked(request);
    }

    /// `form.created` fed through the machine.
    pub fn form_created(&mut self, request: crate::components::footer::FormRequest) {
        self.machine.form_created(request);
    }

    /// Route one key through the active footer view. Blocker views own
    /// every key (the `app.rs` layer order: permission over form over
    /// prompt).
    #[must_use]
    pub fn press(&mut self, key: FooterKey) -> Vec<FooterEffect> {
        self.expire_armed();
        match self.machine.view.clone() {
            FooterView::Permission { request } => self.press_permission(&request, key),
            FooterView::Form { request } => self.press_form(&request, key),
            FooterView::Prompt => self.press_prompt(key),
        }
    }

    fn press_permission(
        &mut self,
        request: &PermissionRequest,
        key: FooterKey,
    ) -> Vec<FooterEffect> {
        let state = self
            .permission_states
            .remove(&request.id)
            .unwrap_or_else(|| create_permission_body_state(request));
        let name = match key {
            FooterKey::Printable(char) => char.to_string(),
            FooterKey::Enter => "enter".to_owned(),
            FooterKey::Escape => "escape".to_owned(),
            FooterKey::Backspace => "backspace".to_owned(),
            FooterKey::Up => "up".to_owned(),
            FooterKey::Down => "down".to_owned(),
            FooterKey::CtrlC => "ctrl+c".to_owned(),
            FooterKey::Tab => "tab".to_owned(),
            FooterKey::PageUp => "pageup".to_owned(),
            FooterKey::PageDown => "pagedown".to_owned(),
        };
        let mut effects = Vec::new();
        // The mini surface drives the same session-route two-stage
        // machine; the free-text reject stage stays a child-session
        // affordance (parentID), which the mini transport does not
        // track — top-level semantics.
        match permission_key(&state, request, false, &name) {
            PermissionKey::State(next) => {
                self.permission_states.insert(request.id.clone(), next);
            }
            PermissionKey::Reply(reply, next) => {
                self.permission_replies.push(reply.clone());
                self.permission_states.insert(request.id.clone(), next);
                let id = reply.request_id.clone();
                self.machine.permission_replied(&id);
                effects.push(FooterEffect::PermissionReply(reply));
            }
            PermissionKey::Text(char) => {
                let next = permission_type_message(&state, char);
                self.permission_states.insert(request.id.clone(), next);
            }
            PermissionKey::Ignored => {
                self.permission_states.insert(request.id.clone(), state);
            }
        }
        effects
    }

    fn press_form(
        &mut self,
        request: &crate::components::footer::FormRequest,
        key: FooterKey,
    ) -> Vec<FooterEffect> {
        let mut state =
            self.form_states.remove(&request.id).unwrap_or_else(|| FormBodyState::new(request));
        let mut draft = self.form_drafts.remove(&request.id).unwrap_or_default();
        let mut effects = Vec::new();
        let textual =
            state.current(request).is_some_and(crate::components::footer::FormField::textual);
        match key {
            FooterKey::Up => state = form_move(&state, request, -1),
            FooterKey::Down => state = form_move(&state, request, 1),
            FooterKey::Enter => {
                if textual || state.editing {
                    let (next, reply) = form_commit_input(&state, request, &draft);
                    state = next;
                    draft.clear();
                    if let Some(reply) = reply {
                        let id = reply.form_id.clone();
                        self.form_replies.push(reply.clone());
                        self.machine.form_removed(&id);
                        effects.push(FooterEffect::FormReply(reply));
                    }
                } else {
                    let (next, reply) = form_pick(&state, request, state.selected);
                    state = next;
                    if let Some(reply) = reply {
                        let id = reply.form_id.clone();
                        self.form_replies.push(reply.clone());
                        self.machine.form_removed(&id);
                        effects.push(FooterEffect::FormReply(reply));
                    }
                }
            }
            FooterKey::Escape => {
                self.form_cancels.push(request.id.clone());
                self.machine.form_removed(&request.id);
                effects.push(FooterEffect::FormCancel(request.id.clone()));
            }
            FooterKey::Backspace => {
                draft.pop();
            }
            FooterKey::Printable(char) => {
                if textual || state.editing {
                    draft.push(char);
                } else if let Some(digit) = char.to_digit(10) {
                    let index = digit as usize;
                    if state.current(request).is_some_and(|field| {
                        index < field.rows().len() + usize::from(field.custom())
                    }) {
                        let (next, reply) = form_pick(&state, request, index);
                        state = next;
                        if let Some(reply) = reply {
                            let id = reply.form_id.clone();
                            self.form_replies.push(reply.clone());
                            self.machine.form_removed(&id);
                            effects.push(FooterEffect::FormReply(reply));
                        }
                    }
                }
            }
            FooterKey::CtrlC => {}
            FooterKey::Tab | FooterKey::PageUp | FooterKey::PageDown => {}
        }
        // Only keep the per-request state while the blocker is live.
        if let FooterView::Form { request: live } = &self.machine.view {
            if live.id == request.id {
                self.form_states.insert(request.id.clone(), state);
                self.form_drafts.insert(request.id.clone(), draft);
            }
        }
        effects
    }

    fn press_prompt(&mut self, key: FooterKey) -> Vec<FooterEffect> {
        // TUI-17 — the inspector owns keys while mounted
        // (`useKeyboard`, footer.subagent.tsx:115-146): esc closes,
        // tab cycles, up/k + down/j scroll, pgup/pgdn page, and
        // ctrl+c interrupts the inspected child
        // (`composer.subagent.interrupt`).
        if self.machine.route == crate::components::footer::PromptRoute::Subagent {
            return self.press_subagent(key);
        }
        let mut effects = Vec::new();
        match key {
            FooterKey::Printable(char) => {
                self.draft.push(char);
                // Typing clears armed counters (`handleInputClear`,
                // `footer.ts:861-868`).
                self.exit_presses = 0;
                self.exit_deadline = None;
                self.interrupt_presses = 0;
                self.interrupt_deadline = None;
            }
            FooterKey::Backspace => {
                self.draft.pop();
                self.exit_presses = 0;
                self.exit_deadline = None;
            }
            FooterKey::Enter => {
                if !self.draft.trim().is_empty() {
                    effects.push(FooterEffect::Submit(std::mem::take(&mut self.draft)));
                }
            }
            FooterKey::CtrlC => {
                // Two-press exit (`handleExit`, `footer.ts:1017-1041`):
                // the first press over a draft clears it.
                if !self.draft.is_empty() {
                    self.draft.clear();
                    return effects;
                }
                self.interrupt_presses = 0;
                self.interrupt_deadline = None;
                self.exit_presses += 1;
                self.exit_deadline = Some(Instant::now() + Duration::from_secs(5));
                if self.exit_presses >= 2 {
                    self.exit_presses = 0;
                    self.exit_deadline = None;
                    effects.push(FooterEffect::Close);
                }
            }
            FooterKey::Escape => {
                // Two-press interrupt (`handleInterrupt`,
                // `footer.ts:990-1015`).
                self.exit_presses = 0;
                self.exit_deadline = None;
                self.interrupt_presses += 1;
                self.interrupt_deadline = Some(Instant::now() + Duration::from_secs(5));
                if self.interrupt_presses >= 2 {
                    self.interrupt_presses = 0;
                    self.interrupt_deadline = None;
                    effects.push(FooterEffect::Interrupt);
                }
            }
            FooterKey::Tab | FooterKey::PageUp | FooterKey::PageDown => {}
            FooterKey::Up => effects.push(FooterEffect::HistoryUp),
            FooterKey::Down => {
                // TUI-17 — `session.child.first` over tracked
                // children: the inspector opens on the newest running
                // child (footer.view.tsx:312's subagent route via the
                // statusline detail).
                if self.subagent_children.is_empty() {
                    effects.push(FooterEffect::HistoryDown);
                } else {
                    self.machine.route = crate::components::footer::PromptRoute::Subagent;
                    self.subagent_selected = self
                        .subagent_children
                        .iter()
                        .rev()
                        .find(|child| child.status == super::transport::SubagentStatus::Running)
                        .or_else(|| self.subagent_children.last())
                        .map(|child| child.session_id.clone());
                    self.subagent_scroll = 0;
                }
            }
        }
        effects
    }

    /// The statusline hint while an exit/interrupt is armed
    /// (`footer.view.tsx:402-408`: `Press ${key} again to ${action}`).
    #[must_use]
    pub fn armed_hint(&self) -> Option<String> {
        if self.exit_presses > 0 {
            return Some("ctrl+c again to exit".to_owned());
        }
        if self.interrupt_presses > 0 {
            return Some("esc again to interrupt".to_owned());
        }
        None
    }

    /// The inspector's key handling (footer.subagent.tsx:115-146).
    fn press_subagent(&mut self, key: FooterKey) -> Vec<FooterEffect> {
        let mut effects = Vec::new();
        match key {
            FooterKey::Escape | FooterKey::Enter => {
                // esc back (onClose → the composer route).
                self.machine.close_panel();
                self.subagent_selected = None;
                self.subagent_scroll = 0;
            }
            FooterKey::Printable('k') => {
                self.subagent_scroll = self.subagent_scroll.saturating_add(1);
            }
            FooterKey::Printable('j') => {
                self.subagent_scroll = self.subagent_scroll.saturating_sub(1);
            }
            FooterKey::Printable(' ') | FooterKey::Backspace => {}
            FooterKey::Up => self.subagent_scroll = self.subagent_scroll.saturating_add(1),
            FooterKey::Down => self.subagent_scroll = self.subagent_scroll.saturating_sub(1),
            FooterKey::PageUp => self.subagent_scroll = self.subagent_scroll.saturating_add(10),
            FooterKey::PageDown => self.subagent_scroll = self.subagent_scroll.saturating_sub(10),
            FooterKey::Tab => {
                // tab next: the ring over the tracked children (the
                // selected index is 1-based — the next 0-based slot is
                // `index % len`).
                if self.subagent_children.len() > 1 {
                    let index = self.selected_subagent_index().unwrap_or(0);
                    let next = index % self.subagent_children.len();
                    self.subagent_selected = Some(self.subagent_children[next].session_id.clone());
                    self.subagent_scroll = 0;
                }
            }
            FooterKey::CtrlC => {
                // `{hint} interrupt` — per-child (v2.session.interrupt
                // over the inspected session, resume false).
                if let Some(selected) = self.subagent_selected.clone() {
                    effects.push(FooterEffect::InterruptChild(selected));
                }
            }
            FooterKey::Printable(_) => {}
        }
        effects
    }

    /// The selected child's 1-based position (0 = none).
    #[must_use]
    pub fn selected_subagent_index(&self) -> Option<usize> {
        let id = self.subagent_selected.as_deref()?;
        self.subagent_children.iter().position(|child| child.session_id == id).map(|at| at + 1)
    }

    /// The footer pane height for the active view (`applyHeight`,
    /// `footer.ts:740-760`): blockers grow the dock over the composer
    /// base (`PERMISSION_ROWS`/`FORM_ROWS`, `footer.ts:117-118`);
    /// TUI-17 — the inspector adds its 14 rows.
    #[must_use]
    pub fn pane_height(&self) -> usize {
        // RD-19 — the launch row grows the dock by one
        // (`footer.ts:762`'s `desired + (this.startup() ? 1 : 0)`).
        let startup = usize::from(self.startup.is_some());
        startup
            + match self.machine.view {
                FooterView::Permission { .. } => footer_base(FOOTER_HEIGHT) + PERMISSION_ROWS,
                FooterView::Form { .. } => footer_base(FOOTER_HEIGHT) + FORM_ROWS,
                FooterView::Prompt => {
                    if self.machine.route == crate::components::footer::PromptRoute::Subagent {
                        footer_base(FOOTER_HEIGHT) + super::footer_subagent::SUBAGENT_INSPECTOR_ROWS
                    } else {
                        FOOTER_HEIGHT
                    }
                }
            }
    }

    /// Compose the footer's [`crate::render::Frame`] (the dispatch the
    /// render loop performs on `FooterView`). RD-19: the launch row
    /// sits ABOVE the view body (`footer.view.tsx:709-725`'s first
    /// box in the column) while the startup animation runs.
    #[must_use]
    pub fn render_frame(&self, width: usize, height: usize) -> crate::render::Frame {
        let startup = usize::from(self.startup.is_some());
        let mut frame = self.render_body(width, height - startup);
        if let Some(row) = self.startup.as_ref().map(|row| self.render_startup_row(row, width)) {
            frame.cells.insert(0, row);
            frame.height += 1;
        }
        frame
    }

    /// The launch row (`footer.view.tsx:711-724`): the `SEED_LAUNCH`
    /// one-shot spinner in cell 0 (the label's leading ▪ mark replaced
    /// by the animated cell, colored `splash.right` with its glow
    /// levels), then the label tail and the metadata (`splash.left`).
    #[must_use]
    fn render_startup_row(&self, startup: &StartupRow, width: usize) -> Vec<crate::render::Cell> {
        use crate::render::{Cell, CellColor};
        let splash = super::splash::SplashColors::from_footer_theme(&self.theme);
        let right = CellColor::from(splash.right);
        let left = CellColor::from(splash.left);
        let animation = super::spinner::seed_launch();
        let cell = animation.one_cell_frame(startup.since.elapsed());
        let mut row: Vec<Cell> = Vec::with_capacity(width);
        let mark = '\u{25aa}';
        let (tail, metadata) = if startup.label.starts_with(mark) {
            // The animated spinner replaces the label's first cell.
            row.push(Cell { ch: cell.glyph, fg: apply_glow(right, cell.level), ..Cell::default() });
            (startup.label[mark.len_utf8()..].to_owned(), startup.metadata.clone())
        } else {
            // A label without the mark (too narrow) renders as plain
            // text — the ref's `Show when={label.startsWith("▪")}`.
            (startup.label.clone(), startup.metadata.clone())
        };
        for ch in tail.chars().chain(metadata.chars()) {
            if row.len() >= width {
                break;
            }
            // `wrapMode="none"`: the tail clips at the terminal edge.
            let color = if row.len() < tail.chars().count() { right } else { left };
            row.push(Cell { ch, fg: color, ..Cell::default() });
        }
        row
    }

    /// The view body beneath the launch row (the previous
    /// `render_frame` dispatch).
    #[must_use]
    fn render_body(&self, width: usize, height: usize) -> crate::render::Frame {
        match &self.machine.view {
            FooterView::Permission { request } => {
                let state = self
                    .permission_states
                    .get(&request.id)
                    .cloned()
                    .unwrap_or_else(|| create_permission_body_state(request));
                render_permission_body(request, &state, width, height, &self.theme)
            }
            FooterView::Form { request } => {
                let state = self
                    .form_states
                    .get(&request.id)
                    .cloned()
                    .unwrap_or_else(|| FormBodyState::new(request));
                render_form_body(request, &state, width, height, &self.theme)
            }
            FooterView::Prompt => {
                // TUI-17 — the subagent inspector body over the
                // composer (`footer.view.tsx:156-196`'s inspecting
                // mount).
                if self.machine.route == crate::components::footer::PromptRoute::Subagent {
                    let selected = self
                        .subagent_selected
                        .as_deref()
                        .and_then(|id| {
                            self.subagent_children.iter().find(|child| child.session_id == id)
                        })
                        .or_else(|| self.subagent_children.last());
                    if let Some(child) = selected {
                        let view = super::footer_subagent::SubagentView {
                            child,
                            index: self.selected_subagent_index().unwrap_or(0),
                            total: self.subagent_children.len(),
                            scroll: self.subagent_scroll,
                            mono: self.mono,
                        };
                        return super::footer_subagent::render_subagent_body(
                            &view,
                            &self.theme,
                            width,
                            height,
                        );
                    }
                }
                // The composer base: rail + transparent textarea with
                // the live draft, statusline below.
                let running = self
                    .subagent_children
                    .iter()
                    .filter(|child| child.status == super::transport::SubagentStatus::Running)
                    .count();
                let idle = PromptIdleState {
                    agent: self.agent.clone().unwrap_or_else(|| "Build".to_owned()),
                    model_label: self
                        .model_label
                        .clone()
                        .unwrap_or_else(|| "Default model".to_owned()),
                    command_shortcut: "ctrl+p".to_owned(),
                    // TUI-17 — the statusline's `N subagent(s)`
                    // detail (footer.view.tsx:247).
                    status_detail: if running > 0 {
                        format!("{running} subagent{}", if running == 1 { "" } else { "s" })
                    } else {
                        String::new()
                    },
                };
                // CONFIG-T4 — the busy statusline (`footer.view.tsx:210`
                // + `:978-988`): while a turn runs, the status text
                // narrows by the one-cell spinner box + its gap; the
                // `mono` flag swaps in the ASCII animation.
                let busy = self.busy_since.is_some();
                let status_width = if busy { width.saturating_sub(2).max(1) } else { width };
                let mut frame = render_prompt_idle(&idle, status_width, height, &self.theme);
                self.paint_draft(&mut frame);
                if !self.footer_details {
                    blank_statusline_details(&mut frame);
                }
                if let Some(since) = self.busy_since {
                    let motion = if self.mono {
                        super::spinner::seed_mono()
                    } else {
                        super::spinner::work_spinner(&self.work_spinner)
                    };
                    // RD-19 — `oneCellFrame`'s glyph + glow level; the
                    // ref mounts the spinner with `glow={!props.mono}`
                    // (footer.view.tsx:985) so the per-frame alpha
                    // applies except in mono.
                    let cell = motion.one_cell_frame(since.elapsed());
                    let level = if self.mono { 1.0 } else { cell.level };
                    paint_busy_cell(
                        &mut frame,
                        cell.glyph,
                        level,
                        self.theme.agent_color(0),
                        width,
                    );
                    frame.width = width;
                }
                frame
            }
        }
    }

    /// Paint the composer draft into the transparent textarea region
    /// (the managed textarea the reference mounts in composer row 0;
    /// paddingLeft 2 / paddingRight 2, `footer.prompt.tsx`).
    fn paint_draft(&self, frame: &mut crate::render::Frame) {
        use crate::render::CellColor;
        let text = self.draft.replace('\n', " ");
        let region = frame.width.saturating_sub(4);
        let visible = crate::wrap::take_width(&text, region);
        for (index, atom) in
            crate::wrap::flatten(&visible, crate::style::Style::new()).into_iter().enumerate()
        {
            let x = 2 + index;
            if x + atom.width > 2 + region {
                break;
            }
            if let Some(cell) = frame.cells.get_mut(0).and_then(|row| row.get_mut(x)) {
                cell.ch = atom.text.chars().next().unwrap_or(' ');
                cell.fg = CellColor::Default;
            }
        }
    }
}

/// CONFIG-T4 `mini.footer === "hide"`: the statusline's informational
/// groups vanish (v2's `footerDetails` memo returning empty status
/// sections — `footer.view.tsx:220,416-500`); the composer row and
/// the row itself stay (the height never changes).
fn blank_statusline_details(frame: &mut crate::render::Frame) {
    if let Some(row) = frame.cells.get_mut(1) {
        for cell in row.iter_mut() {
            *cell = crate::render::Cell::default();
        }
    }
}

/// CONFIG-T4 busy cell (`footer.view.tsx:978-988`): the one-cell
/// spinner box (glyph at col 0, its `gap={1}` at col 1) prepended to
/// the statusline row; the status text was already rendered into the
/// narrowed width, so the cells shift into place. RD-19 — `level`
/// carries `oneCellFrame`'s glow multiplier: the ref scales the
/// spinner RGBA's alpha per frame (`color.a *= level`); the port's
/// cell model composites over the (dark) theme background, so the
/// level dims the RGB channels.
fn paint_busy_cell(
    frame: &mut crate::render::Frame,
    glyph: char,
    level: f64,
    color: crate::render::CellColor,
    width: usize,
) {
    let color = apply_glow(color, level);
    let Some(row) = frame.cells.get_mut(1) else { return };
    let spacer = crate::render::Cell { ch: ' ', fg: color, ..crate::render::Cell::default() };
    let cell = crate::render::Cell { ch: glyph, fg: color, ..crate::render::Cell::default() };
    row.insert(0, cell);
    row.insert(1, spacer);
    row.truncate(width);
}

/// `color.a *= level` (`one-cell-spinner.tsx:29-34`) in the port's
/// alpha-less cell model: scale the channels (alpha-over-black).
fn apply_glow(color: crate::render::CellColor, level: f64) -> crate::render::CellColor {
    let level = level.clamp(0.0, 1.0);
    // level ∈ [0, 1] scales each channel back INTO u8 range — the
    // float→u8 cast below is sign- and range-safe.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let dim = |channel: u8| (f64::from(channel) * level).round() as u8;
    match color {
        crate::render::CellColor::Rgb(r, g, b) => {
            crate::render::CellColor::Rgb(dim(r), dim(g), dim(b))
        }
        other => other,
    }
}

/// [`Lifecycle::boot`]'s inputs (`LifecycleInput`,
/// `runtime.lifecycle.ts:55-83`, the host-provided subset).
#[allow(clippy::struct_excessive_bools)] // the display-flag set mirrors the reference
pub struct LifecycleInput {
    pub version: String,
    pub home: String,
    pub directory: String,
    pub agent: Option<String>,
    pub model_label: Option<String>,
    pub splash: bool,
    pub mono: bool,
    /// `mini.footer` (CONFIG-T4): the statusline's informational
    /// groups render while "show" (`footerDetails`,
    /// `footer.view.tsx:220`).
    pub footer: bool,
    /// `mini.work_spinner` (CONFIG-T4): the busy cell's animation id
    /// (`WORK_SPINNERS[miniSettings().work_spinner]`,
    /// `footer.view.tsx:983`).
    pub work_spinner: String,
    /// `mini.shell_output` (CONFIG-T4): shell-family tools stream
    /// their live rows while "show" (`footer.ts:234`).
    pub shell_output: bool,
}

/// The split-footer renderer handle (`createRuntimeLifecycle`'s
/// Pending scrollback rows: plain text (mono/default styling) or
/// styled spans (the exit splash's per-cell half-block mark).
#[derive(Debug, Clone)]
enum PendingRows {
    Plain(Vec<String>),
    Styled(Vec<Vec<crate::layout::StyledSpan>>),
}

impl PendingRows {
    fn len(&self) -> usize {
        match self {
            PendingRows::Plain(rows) => rows.len(),
            PendingRows::Styled(rows) => rows.len(),
        }
    }

    /// Row `index` flattened into cluster atoms — plain rows under the
    /// default style, styled rows through their spans (the same
    /// [`crate::wrap::flatten`] seam the retired cell-buffer writer
    /// used).
    fn atoms(&self, index: usize) -> Vec<crate::wrap::Atom> {
        match self {
            PendingRows::Plain(rows) => {
                crate::wrap::flatten(&rows[index], crate::style::Style::new())
            }
            PendingRows::Styled(rows) => crate::wrap::flatten_spans(&rows[index]),
        }
    }

    /// The whole batch as styled ANSI rows (D2.3 — the payload the
    /// split-footer engine's commit FIFO carries: one String per row,
    /// plain rows as text, styled rows as SGR-run text using the same
    /// serialization the retired CUP writer stamped). Each row ends
    /// with a CR: the session is in raw mode (OPOST off), so the
    /// engine's LF row separator needs the CR half to return the
    /// cursor — the retired writer's `\r\n` pairs, split across the
    /// row payload and the engine's newline intent.
    fn ansi_rows(&self, width: usize) -> Vec<String> {
        (0..self.len())
            .map(|index| {
                let mut row = serialize_atoms_row(&self.atoms(index), width);
                row.push('\r');
                row
            })
            .collect()
    }
}

/// One row's cluster atoms → styled ANSI text: a style boundary
/// re-opens SGR from reset (one burst per run), the row closes with
/// a final reset (the retired cell-buffer writer's merge shape).
fn serialize_atoms_row(atoms: &[crate::wrap::Atom], width: usize) -> String {
    let mut sequence = String::new();
    let mut x = 0usize;
    let mut last_style: Option<crate::style::Style> = None;
    for atom in atoms {
        if x >= width {
            break;
        }
        if last_style != Some(atom.style) {
            sequence.push_str("\x1b[0m");
            if let Some(color) = atom.style.fg {
                crate::grid_emitter::emit_fg(
                    &mut sequence,
                    crate::render::CellColor::from(color),
                );
            }
            if let Some(color) = atom.style.bg {
                crate::grid_emitter::emit_bg(
                    &mut sequence,
                    crate::render::CellColor::from(color),
                );
            }
            crate::grid_emitter::apply_attributes(&mut sequence, atom.style.add_modifier.bits());
            last_style = Some(atom.style);
        }
        sequence.push_str(&atom.text);
        x += atom.width.max(1);
    }
    if last_style.is_some() {
        sequence.push_str("\x1b[0m");
    }
    sequence
}

/// The mini terminal session — opentui's `Terminal` over the real
/// tty + stdout (the main app's [`crate::app::SessionTerminal`]
/// shape).
pub type MiniTerminal = opentui_core::terminal::Terminal<
    opentui_core::terminal::UnixTty,
    std::io::Stdout,
>;

/// D2.3 — the renderer's terminal-session handle (the same adapter
/// the main app installs, `app.rs`'s `RendererSessionHandle`): the
/// split-commit flush and teardown reach the terminal through the
/// shared slot. The mini loop is single-threaded and the only writer,
/// so the lock is uncontended by construction.
struct MiniSessionHandle {
    session: std::sync::Arc<std::sync::Mutex<MiniTerminal>>,
}

impl RendererTerminalSession for MiniSessionHandle {
    fn write_bytes(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write as _;
        if let Ok(mut session) = self.session.lock() {
            let out = session.output();
            out.write_all(bytes)?;
            out.flush()?;
            Ok(())
        } else {
            Err(std::io::Error::other("mini session terminal lock poisoned"))
        }
    }

    fn pump_output(
        &mut self,
        renderer: &mut opentui_core::renderer::CliRenderer,
    ) -> std::io::Result<()> {
        // D4.1c — the retired "never pump" ruling: the mini footer's
        // frame bytes flow through the renderer pump exit (the
        // `?2026`-wrapped SGR stream). The lifecycle pumps directly on
        // the locked session; this seam stays the API-complete twin.
        if let Ok(mut session) = self.session.lock() {
            opentui_core::terminal::Terminal::pump_output(&mut session, renderer)
        } else {
            Err(std::io::Error::other("mini session terminal lock poisoned"))
        }
    }

    fn teardown(&mut self) -> std::io::Result<()> {
        if let Ok(mut session) = self.session.lock() {
            session.teardown()
        } else {
            Err(std::io::Error::other("mini session terminal lock poisoned"))
        }
    }

    fn is_torn_down(&self) -> bool {
        self.session
            .lock()
            .map(|session| session.is_torn_down())
            .unwrap_or(true)
    }
}

/// `Lifecycle`, `runtime.lifecycle.ts:85-92`).
#[allow(clippy::struct_excessive_bools)] // the renderer flag set mirrors the reference
pub struct Lifecycle {
    /// D4.1c — the mini footer's root-tree mount state (the rows +
    /// geometry signals behind the mounted [`FooterTree`] element).
    footer: FooterTree,
    /// D4.1c — the split-footer renderer, owned by the solid mount
    /// (the ref's `render(() => …, renderer)` root mount,
    /// `footer.ts:326`): the footer content rides the renderer's
    /// ROOT tree; every frame byte leaves through the pump exit (the
    /// `?2026`-wrapped SGR stream), never a grid-diffed writer.
    solid: opentui_solid::SolidRenderer,
    /// The region/park control-write channel (one `Stdout`;
    /// single-threaded mini keeps the write seam uncontended).
    out: std::io::Stdout,
    /// D2.3 — the opentui terminal session: raw mode + the reserved
    /// footer surface (the non-alt `makeRoomForRendererOutput` boot
    /// push). Transcript bytes from the split-commit flush and the
    /// teardown epilogue write through it.
    /// D2.3 — the split-footer renderer's terminal session ... (the
    /// renderer itself lives inside `solid`, see above).
    session: std::sync::Arc<std::sync::Mutex<MiniTerminal>>,
    pub pane: FooterPane,
    pending: Vec<PendingRows>,
    /// The last committed transcript row (separator decisions).
    last_commit: Option<Commit>,
    width: usize,
    #[allow(dead_code)] // kept for resize parity with the reference
    height: usize,
    closed: bool,
    /// D2.3 — the transcript band's cursor row (1-based, inside the
    /// scroll region). The engine's stream writes from WHEREVER the
    /// cursor sits, and the dock paint parks it mid-dock — so every
    /// scrollback flush re-CUPs here first and the row advances with
    /// the written lines (clamped at the region bottom; commits
    /// always end on a fresh row, so the column is always 1).
    transcript_row: usize,
    /// The live region bottom (1-based); `0` = unset. Tracks
    /// `height - max(FOOTER_HEIGHT, pane height)` — the pane's
    /// blocker bodies grow the dock upward, shrinking the band.
    region_bottom: usize,
    /// The last painted footer signature (the dirty check) + a force
    /// flag for post-scroll invalidation (a scrollback write scrolls
    /// the screen, so the next footer draw must repaint fully).
    last_footer_signature: String,
    force_draw: bool,
    /// The dock rows currently on screen (0 after an erase — a
    /// scrollback write wiped them; the next draw repaints).
    drawn_footer_rows: usize,
    splash_enabled: bool,
    mono: bool,
    /// CONFIG-T4 `mini.shell_output`: shell-family tools stream their
    /// live rows while set (`entryBody(commit, { shellOutput })`,
    /// `scrollback.surface.ts:457`).
    shell_output: bool,
    version: String,
    home: String,
    directory: String,
}

impl Lifecycle {
    /// Boot the renderer (`createRuntimeLifecycle`,
    /// `runtime.lifecycle.ts:152-205`): the split-footer terminal
    /// session + renderer, entry splash, first footer paint.
    pub fn boot(input: LifecycleInput) -> Result<Self, String> {
        // D2.3 — the split-footer session base: raw mode + the
        // reserved main-screen surface (`makeRoomForRendererOutput`,
        // `renderer.zig:566-569`) — the boot push that scrolls the
        // shell's prior output up and frees the dock's rows at the
        // bottom. Mouse/kitty/paste/focus stay off (the ref's
        // `useMouse: false`, `autoFocus: false`, mono-independent —
        // mini keys ride the KeyPipeline fd read, `exitOnCtrlC` is
        // the footer's two-press chord, not the renderer's).
        let session = {
            let tty = opentui_core::terminal::UnixTty::new().map_err(|e| e.to_string())?;
            let setup = opentui_core::terminal::SetupOptions {
                use_mouse: false,
                mouse_movement: false,
                kitty_keyboard_flags: 0,
                bracketed_paste: false,
                auto_focus: false,
                use_alt_screen: false,
                reserve_surface_height: u32::try_from(FOOTER_HEIGHT).unwrap_or(4),
            };
            let terminal =
                MiniTerminal::new(tty, std::io::stdout(), &setup).map_err(|e| e.to_string())?;
            std::sync::Arc::new(std::sync::Mutex::new(terminal))
        };
        let (width, height) = {
            use opentui_core::terminal::TtyDevice as _;
            let Ok(mut session) = session.lock() else {
                return Err("mini session terminal lock poisoned".to_owned());
            };
            let (columns, rows) =
                session.tty().window_size().map_err(|e| e.to_string())?;
            (usize::from(columns).max(1), usize::from(rows).max(1))
        };
        // Split-footer mode never enters the alternate screen — the
        // conversation owns the native scrollback; only the footer
        // pane is live-rendered. D2.3: the real CliRenderer in the
        // ref's configuration (`runtime.lifecycle.ts:157-174`) — the
        // capture gate arms at construction (`resolveModes`).
        let mut renderer = create_cli_renderer(CliRendererConfig {
            width: u32::try_from(width).unwrap_or(u32::MAX),
            height: u32::try_from(height).unwrap_or(u32::MAX),
            use_mouse: false,
            auto_focus: false,
            exit_on_ctrl_c: false,
            clear_on_shutdown: false,
            screen_mode: ScreenMode::SplitFooter,
            footer_height: Some(u32::try_from(FOOTER_HEIGHT).unwrap_or(4)),
            external_output_mode: Some(ExternalOutputMode::CaptureStdout),
            ..CliRendererConfig::default()
        });
        renderer.attach_terminal_session(Box::new(MiniSessionHandle {
            session: std::sync::Arc::clone(&session),
        }));
        debug_assert!(renderer.is_split_footer_capture_active(), "capture arms at boot");
        // D4.1c — the footer's root-tree mount (`render(() => …,
        // renderer)`, footer.ts:326): the solid mount owns the
        // renderer, the dock content rides its root tree, and the
        // frame bytes leave through the pump exit below. The engine's
        // band geometry (renderWidth × effectiveFooterHeight +
        // renderOffset) came with the split config.
        let initial_height = FOOTER_HEIGHT.min(height).max(1);
        let (footer, element) = FooterTree::new(width, initial_height);
        let solid = opentui_solid::render_into(renderer, element);

        let mut pane = FooterPane::new(crate::components::footer::FooterTheme::fallback_dark());
        if let Some(agent) = input.agent {
            pane.agent = Some(agent);
        }
        if let Some(model_label) = input.model_label {
            pane.model_label = Some(model_label);
        }
        // CONFIG-T4: the display knobs ride the pane (`miniSettings()`
        // reads in `footer.view.tsx`).
        pane.footer_details = input.footer;
        pane.work_spinner = input.work_spinner;
        pane.mono = input.mono;
        let mut lifecycle = Self {
            out: std::io::stdout(),
            footer,
            solid,
            session,
            pane,
            pending: Vec::new(),
            last_commit: None,
            width,
            height,
            closed: false,
            last_footer_signature: String::new(),
            force_draw: true,
            drawn_footer_rows: 0,
            transcript_row: 1,
            region_bottom: 0,
            splash_enabled: input.splash,
            mono: input.mono,
            shell_output: input.shell_output,
            version: input.version,
            home: input.home,
            directory: input.directory,
        };
        // D2.3 — the boot push is the Terminal's reserved-surface arm
        // (`makeRoomForRendererOutput` ran inside `Terminal::new`,
        // scrolling the shell's prior output until the dock's rows
        // free at the bottom); no separate newline push remains.
        // `queueSplash(entry)` (`runtime.lifecycle.ts:191-203`) with
        // RD-19's startup split (`runtime.lifecycle.ts:187-196`):
        // splash=show AND NOT mono (the ref also requires
        // `tuiConfig.animations !== false`; the port carries no
        // animations knob into mini) mounts the SEED_LAUNCH launch row
        // and DEFERS the entry splash to its completion — any other
        // combination queues the splash at boot as before.
        if lifecycle.splash_enabled {
            let detail = directory_label(&lifecycle.directory, &lifecycle.home);
            if lifecycle.mono {
                let rows = entry_splash_rows(
                    &lifecycle.version,
                    Some(&detail),
                    lifecycle.mono,
                    width.max(1),
                );
                lifecycle.queue_rows(rows);
            } else {
                let layout = entry_splash_layout(&EntrySplashInput {
                    width: width.max(1),
                    version: &lifecycle.version,
                    detail: Some(&detail),
                    mono: false,
                });
                lifecycle.pane.startup = Some(StartupRow {
                    since: Instant::now(),
                    label: layout.label,
                    metadata: layout.metadata,
                });
            }
        }
        // D2.3 — the transcript scroll region (the split-footer
        // geometry: transcript band scrolls, dock band stays).
        lifecycle.reconcile_scrollback_region()?;
        lifecycle.flush()?;
        Ok(lifecycle)
    }

    /// RD-19 — `finishStartup` (`footer.ts:393-409`): when the
    /// launch one-shot completes (`(21-1) × 40 ms`), drop the launch
    /// row and queue the entry splash for scrollback (the ref's
    /// setTimeout fires once; the flush tick is the port's driver).
    fn finish_startup(&mut self) {
        if !self.pane.startup_expired() {
            return;
        }
        self.pane.startup = None;
        if self.splash_enabled {
            let detail = directory_label(&self.directory, &self.home);
            let rows =
                entry_splash_rows(&self.version, Some(&detail), self.mono, self.width.max(1));
            self.queue_rows(rows);
        }
        self.force_draw = true;
    }

    fn queue_rows(&mut self, rows: Vec<String>) {
        if !rows.is_empty() {
            self.pending.push(PendingRows::Plain(rows));
        }
    }

    /// Queue styled scrollback rows (the exit splash's half-block
    /// mark — one style per cell, like v2's per-cell
    /// `TextRenderable` pushes).
    fn queue_styled_rows(&mut self, rows: Vec<Vec<crate::layout::StyledSpan>>) {
        if !rows.is_empty() {
            self.pending.push(PendingRows::Styled(rows));
        }
    }

    /// `footer.append` → scrollback rows (`entryWriter`,
    /// `scrollback.writer.tsx:278-295`) — the `mini.shell_output`
    /// knob rides the entry-body call (`scrollback.surface.ts:457`).
    pub fn append(&mut self, commit: Commit) {
        let rows = format_commit_with(
            self.last_commit.as_ref(),
            &commit,
            self.width.max(1),
            self.shell_output,
        );
        self.last_commit = Some(commit);
        self.queue_rows(rows);
    }

    /// Flush pending scrollback rows above the dock, then repaint the
    /// footer pane (`renderer.writeToScrollback` + the frame's
    /// `flushPendingSplitCommits` + `requestRender`) — see
    /// [`Self::write_scrollback`].
    pub fn flush(&mut self) -> Result<(), String> {
        self.reconcile_scrollback_region()?;
        self.finish_startup();
        if !self.pending.is_empty() {
            if self.closed || !self.solid.renderer.is_split_footer_capture_active() {
                // Post-close rows (SSE folds that landed while the
                // exit chord ran) drain through the passthrough arm —
                // the engine's capture gate is down after `shutdown`.
                self.write_scrollback_passthrough()?;
            } else {
                self.write_scrollback()?;
            }
        }
        self.draw_footer()
    }

    /// The post-close drain (`externalOutputMode: "passthrough"`,
    /// `runtime.lifecycle.ts:97-111`'s shutdown order): late rows
    /// write directly through the same newline-ownership shape (a
    /// newline per row, trailing blanks trimmed) — the engine's FIFO
    /// no longer accepts commits once the capture is disarmed.
    fn write_scrollback_passthrough(&mut self) -> Result<(), String> {
        let mut bytes = String::new();
        let mut lines_written = 0usize;
        self.park_transcript_cursor()?;
        while let Some(batch) = self.pending.first().cloned() {
            for line in batch.ansi_rows(self.width.max(1)) {
                // Each row ends with its CR; the LF separator rides
                // here (the raw-mode `\r\n` pair).
                bytes.push_str(line.trim_end_matches(' '));
                bytes.push('\n');
                lines_written += 1;
            }
            self.pending.remove(0);
        }
        if !bytes.is_empty() {
            self.write_raw(bytes.as_bytes())?;
            let bottom = self.region_bottom.max(1);
            self.transcript_row = (self.transcript_row + lines_written).min(bottom);
            self.force_draw = true;
        }
        Ok(())
    }

    /// The scrollback write (D2.3 — `writeToScrollback` +
    /// `flushPendingSplitCommits`, `renderer.ts:2363-2440` +
    /// `:2729-2826`): each pending batch becomes ONE
    /// `EXTERNAL_OUTPUT` commit in the renderer's FIFO (styled rows
    /// serialized per the retired writer's SGR-run shape), then the
    /// flush drains the queue — the engine's newline-ownership bytes
    /// (tail-column arithmetic, `renderer.ts:2497-2566`) write
    /// through the attached session's output, scrolling the screen
    /// until the rows land in the native scrollback above the dock.
    ///
    /// The dock's rows erase FIRST (the engine's stream scrolls the
    /// whole screen — a painted dock would scroll up as a ghost into
    /// the scrollback); the following [`Self::draw_footer`] repaints
    /// the dock at the bottom. `startOnNewLine`/`trailingNewline` own
    /// the newline intent (every mini commit starts and ends on a
    /// fresh row, so the tail column resets per commit).
    fn write_scrollback(&mut self) -> Result<(), String> {
        let row_columns = u32::try_from(self.width.max(1)).unwrap_or(u32::MAX);
        let mut lines_written = 0usize;
        while let Some(batch) = self.pending.first().cloned() {
            let lines = batch.ansi_rows(self.width.max(1));
            if !lines.is_empty() {
                lines_written += lines.len();
                self.solid
                    .renderer
                    .write_to_scrollback(Box::new(move |_ctx| ScrollbackSnapshot {
                        lines,
                        row_columns: Some(row_columns),
                        start_on_new_line: Some(true),
                        trailing_newline: Some(true),
                    }))
                    .map_err(|error| match error {
                        opentui_core::renderer::ScrollbackWriteError::CaptureNotActive => {
                            "writeToScrollback: the split-footer capture is not active".to_owned()
                        }
                        opentui_core::renderer::ScrollbackWriteError::EmptySnapshot => {
                            "writeToScrollback: the writer returned no rows".to_owned()
                        }
                    })?;
            }
            self.pending.remove(0);
        }
        // Park the cursor at the transcript position (the dock paint
        // left it mid-dock; the engine's stream writes from the
        // cursor), flush, then advance the tracked row (clamped at
        // the region bottom — the trailing newline scrolls in place
        // there).
        self.park_transcript_cursor()?;
        let _ = self.solid.renderer.flush_pending_split_commits(true, true);
        let bottom = self.region_bottom.max(1);
        self.transcript_row = (self.transcript_row + lines_written).min(bottom);
        self.force_draw = true;
        Ok(())
    }

    /// CUP to the transcript band's tracked row (column 1 — every
    /// commit ends on a fresh row).
    fn park_transcript_cursor(&mut self) -> Result<(), String> {
        let row = self.transcript_row.clamp(1, self.height.max(1));
        self.write_raw(format!("\x1b[{row};1H").as_bytes())
    }

    /// The transcript scroll region (`syncSplitScrollback`'s DECSTBM
    /// arm, `renderer.ts:2921-2972`): rows `1..=region_bottom` scroll
    /// with the engine's newline stream while the reserved footer band
    /// below stays put — the true split-footer geometry. The band
    /// reserves `max(FOOTER_HEIGHT, pane height)` rows: the blocker
    /// bodies (permission/form/subagent) grow the dock upward, so the
    /// region re-arms whenever the pane's height changes. DECSTBM
    /// homes the cursor (the next park re-positions anyway).
    fn reconcile_scrollback_region(&mut self) -> Result<(), String> {
        let pane = self.pane.pane_height().clamp(1, self.height);
        let bottom = self
            .height
            .saturating_sub(pane.max(FOOTER_HEIGHT))
            .max(1);
        if bottom != self.region_bottom {
            self.region_bottom = bottom;
            self.transcript_row = self.transcript_row.min(bottom);
            self.write_raw(format!("\x1b[1;{bottom}r").as_bytes())?;
            self.force_draw = true;
        }
        Ok(())
    }

    /// One raw write through the lifecycle's single writer.
    fn write_raw(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.out
            .write_all(bytes)
            .and_then(|()| self.out.flush())
            .map_err(|error| error.to_string())
    }

    /// Paint the footer pane into the screen's bottom rows — D4.1c:
    /// the pane's composed frame rides the renderer's ROOT tree
    /// (the rows + geometry signals), the engine's band layout +
    /// `renderOffset` emission addresses the dock's rows, and the
    /// frame bytes leave through the terminal pump exit wrapped in
    /// the `?2026` synchronized-update envelope. TUI-17/applyHeight:
    /// a pane whose height CHANGED (the blocker bodies + the
    /// subagent inspector grow it) re-geometries the band through
    /// `set_footer_height` (renderer.ts:1768-1772) — the dock grows
    /// upward, the buffers rebuild, and the frame repaints fully.
    pub fn draw_footer(&mut self) -> Result<(), String> {
        let pane_height = self.pane.pane_height().clamp(1, self.height);
        let footer_frame = self.pane.render_frame(self.width, pane_height);
        // Skip identical repaints (the renderer's dirty check — a
        // no-change draw only emits cursor noise), unless a
        // scrollback write invalidated the dock.
        let signature = crate::render::serialize_canonical(&footer_frame);
        if !self.force_draw
            && signature == self.last_footer_signature
            && self.drawn_footer_rows == pane_height
        {
            return Ok(());
        }
        self.last_footer_signature = signature;
        let forced = self.force_draw;
        self.force_draw = false;
        // The band re-geometry (`set footerHeight`, renderer.ts
        // :1768-1772 → applyScreenMode): a changed pane height moves
        // renderOffset and rebuilds the buffers — the next frame is a
        // full repaint by construction. The pane clamps to the
        // terminal height (a taller pane clips to the band, the
        // retired viewport's `max_height` clamp).
        let band_height = pane_height.min(self.height).max(1);
        let height_changed = u32::try_from(band_height)
            .ok()
            .map_or(true, |height| height != self.solid.renderer.footer_height());
        if height_changed {
            self.solid
                .renderer
                .set_footer_height(u32::try_from(band_height).unwrap_or(4))
                .map_err(|error| error.to_string())?;
        }
        if forced && !height_changed {
            // A scrollback write scrolled the screen under an intact
            // band geometry: drop the diff base so the next render
            // repaints every band row (the retired force flag's
            // full-paint semantics; set_footer_height already rebuilt
            // the buffers on a height change).
            let (width, height) =
                (self.solid.renderer.width(), self.solid.renderer.height());
            self.solid.renderer.current_render_buffer =
                opentui_core::buffer::OptimizedBuffer::new(width, height);
        }
        // The tree update patches content in place (signal dyn-props;
        // no remount), and the flush renders one frame into the
        // renderer's buffers + SGR stream.
        self.footer.update(self.width, band_height, &footer_frame);
        self.solid.flush();
        // The pump exit: the frame's SGR bytes drain through the
        // terminal session wrapped in `?2026` (the D2.1b "never pump"
        // ruling retired — the renderer stream IS the dock's writer).
        if let Ok(mut session) = self.session.lock() {
            opentui_core::terminal::Terminal::pump_output(&mut session, &mut self.solid.renderer)
                .map_err(|error| error.to_string())?;
        }
        self.drawn_footer_rows = pane_height;
        Ok(())
    }

    /// `close()` (`runtime.lifecycle.ts:300-343`): exit splash (when
    /// `showExit` + splash shown), then the teardown order — footer
    /// close/destroy, `shutdown` (leave split-footer mode), trailing
    /// newline when no splash was written.
    pub fn close(&mut self, show_exit: bool, title: Option<&str>, session_id: Option<&str>) {
        if self.closed {
            return;
        }
        self.closed = true;
        let mut wrote_exit = false;
        if show_exit && self.splash_enabled {
            if self.mono {
                let rows = exit_splash_rows(
                    title,
                    session_id.unwrap_or_default(),
                    true,
                    self.width.max(1),
                );
                self.queue_rows(rows);
            } else {
                // The styled banner (R-08): the `go` mark's `_`
                // cells paint as full shadow cells and `^`/`~` as
                // half-block mixes — v2's `cells()`/`draw()`
                // (`splash.ts:46-68, 142-170`), colors from the
                // mini theme's splash triple (`theme.ts:173-175`).
                let rows = super::splash::exit_splash_styled_rows(
                    title,
                    session_id.unwrap_or_default(),
                    false,
                    self.width.max(1),
                    &super::splash::SplashColors::from_footer_theme(&self.pane.theme),
                );
                self.queue_styled_rows(rows);
            }
            wrote_exit = true;
        }
        let _ = self.flush();
        // `shutdown(renderer)` (`runtime.lifecycle.ts:97-111`): switch
        // external output back to passthrough BEFORE leaving
        // split-footer mode, so pending stdout doesn't get captured
        // into the now-dead scrollback pipeline; `destroy()` then
        // tears the attached terminal session down (the port's
        // `screenMode → main-screen` transition rides the same
        // capture-off + destroy pair — there is no public mode
        // setter, bounded divergence). The transcript scroll region
        // resets first (`resetScrollRegion`, renderer.ts:2968) so the
        // host shell scrolls normally after exit;
        // `clearOnShutdown: false` (`runtime.lifecycle.ts:172`) — the
        // last footer frame stays (the config is carried on the
        // renderer).
        let _ = self.write_raw(b"\x1b[r");
        // The footer mount's dispose half (`footer.destroy()` → the
        // solid root unmount, index.ts:12-46's runDispose) — no bytes
        // leave (the pump never runs again after this).
        self.solid.dispose_root();
        self.solid.renderer.set_split_footer_capture(false);
        self.solid.renderer.destroy();
        if !wrote_exit {
            // `host.stdout.write("\n")` (`runtime.lifecycle.ts:339-341`).
            let _ = std::io::stdout().write_all(b"\n");
        }
        let _ = std::io::stdout().flush();
    }

    /// Terminal resize (SIGWINCH / TIOCGWINSZ): re-render at the new
    /// size. D2.3 — the registered "mini resize does not reflow"
    /// deviation (ADR-0018, Gate B M3) is CLOSED: the engine's
    /// geometry tracks the new size (`renderer.resize` re-derives the
    /// band extents + renderOffset and rebuilds the buffers), so the
    /// next draw fully repaints the dock at the new width/height.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width.max(1);
        self.height = height.max(1);
        // The solid mount's resize routes core resize + the
        // dimensions signal; the engine re-derives the band geometry
        // (renderOffset/renderHeight) from the new terminal size, so
        // the next draw fully repaints the dock at the new
        // width/height.
        self.solid
            .resize(u32::try_from(self.width).unwrap_or(u32::MAX), u32::try_from(self.height).unwrap_or(u32::MAX));
        // The transcript band re-arms at the new height (the flush's
        // region reconcile; the dock repaint follows).
        self.region_bottom = 0;
        self.force_draw = true;
    }

    /// Poll the terminal size (TIOCGWINSZ on fd 0, the ADR-0018
    /// ≤50 ms watch's seam) and apply [`Self::resize`] when it
    /// changed. Returns whether a re-render is due.
    pub fn poll_resize(&mut self) -> bool {
        let Some((width, height)) = resize_target((self.width, self.height), observe_window_size())
        else {
            return false;
        };
        self.resize(width, height);
        true
    }

    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

/// G-181③ — the resize decision: a real, non-degenerate observation
/// that DIFFERS from the current geometry re-renders; a missing ioctl
/// (`None`), a 0×0 report, or an unchanged size does not. Extracted so
/// the decision table is testable without a tty.
fn resize_target(current: (usize, usize), observed: Option<(u16, u16)>) -> Option<(usize, usize)> {
    let (columns, rows) = observed?;
    if columns == 0 || rows == 0 {
        return None;
    }
    let target = (usize::from(columns), usize::from(rows));
    (target != current).then_some(target)
}

/// G-181③ — the TIOCGWINSZ observation (fd 0, the same device the
/// KeyPipeline reads): `None` when the ioctl fails or reports a
/// degenerate 0×0 (the resize poll ignores it).
#[allow(unsafe_code)] // TIOCGWINSZ on stdin is libc FFI
fn observe_window_size() -> Option<(u16, u16)> {
    let mut winsize = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: TIOCGWINSZ on stdin.
    if unsafe { libc::ioctl(0, libc::TIOCGWINSZ, std::ptr::from_mut(&mut winsize)) } != 0 {
        return None;
    }
    (winsize.ws_col > 0 && winsize.ws_row > 0).then_some((winsize.ws_col, winsize.ws_row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::footer::FooterTheme;

    /// D2.3 — the styled-row serialization the split-footer commit
    /// payload carries: one SGR run per style boundary, a trailing
    /// reset, plain rows stay plain text, and the width clamps the
    /// atom walk.
    #[test]
    fn ansi_rows_carry_style_runs_and_reset() {
        use super::PendingRows;
        let plain = PendingRows::Plain(vec!["hello".to_owned(), String::new()]);
        // A plain row carries only the reset bookends (no color/attr
        // runs) plus the trailing CR — the retired writer's exact
        // shape split across the row payload and the engine's LF.
        let rows = plain.ansi_rows(80);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].trim_matches('\x1b').contains("hello"), "{rows:?}");
        assert!(rows[0].ends_with('\r'), "the CR half of the row pair: {rows:?}");
        assert_eq!(rows[1], "\r");
        let styled = PendingRows::Styled(vec![vec![
            crate::layout::StyledSpan {
                text: "go".to_owned(),
                style: crate::style::Style {
                    fg: Some(crate::style::Color::Indexed(231)),
                    ..crate::style::Style::new()
                },
                link: None,
            },
            crate::layout::StyledSpan {
                text: " off".to_owned(),
                style: crate::style::Style::new(),
                link: None,
            },
        ]]);
        let rows = styled.ansi_rows(80);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].starts_with("\x1b[0m"), "a boundary opens from reset");
        assert!(rows[0].contains("38;5;231"), "the indexed fg opens: {}", rows[0]);
        assert!(rows[0].ends_with("\x1b[0m\r"), "reset then the CR half: {}", rows[0]);
        // The styled run's text survives the SGR boundaries.
        assert!(rows[0].contains("go") && rows[0].contains(" off"), "{}", rows[0]);
        // The width clamp drops atoms past the budget.
        let wide = PendingRows::Plain(vec!["abcdefgh".to_owned()]);
        assert!(wide.ansi_rows(3)[0].contains("abc"), "clamped: {:?}", wide.ansi_rows(3));
    }

    /// G-181③ — the poll's size-decision table: only a real 0×0-free
    /// observation that DIFFERS from the current size re-renders.
    #[test]
    fn poll_resize_decision_table() {
        use super::{observe_window_size, resize_target};
        assert_eq!(resize_target((80, 24), None), None, "denied ioctl: no re-render");
        assert_eq!(resize_target((80, 24), Some((0, 24))), None, "0 columns: ignored");
        assert_eq!(resize_target((80, 24), Some((80, 0))), None, "0 rows: ignored");
        assert_eq!(resize_target((80, 24), Some((80, 24))), None, "unchanged: no re-render");
        assert_eq!(
            resize_target((80, 24), Some((100, 30))),
            Some((100, 30)),
            "a changed size re-renders at the new geometry"
        );
        // Under `cargo test` stdin may or may not be a tty; the
        // observation itself must never panic and never report 0×0.
        let observed = observe_window_size();
        assert!(observed.is_none_or(|(columns, rows)| columns > 0 && rows > 0));
    }

    fn pane() -> FooterPane {
        FooterPane::new(FooterTheme::fallback_dark())
    }

    // -- RD-19: the SEED_LAUNCH launch row -------------------------------

    /// The launch row grows the pane by one (`footer.ts:762`'s
    /// `desired + (this.startup() ? 1 : 0)`) and renders ABOVE the
    /// composer: the animated spinner cell first, then the label tail
    /// and metadata.
    #[test]
    fn startup_row_grows_the_pane_and_renders_the_one_shot() {
        let mut footer = pane();
        assert_eq!(footer.pane_height(), super::super::FOOTER_HEIGHT);
        footer.startup = Some(StartupRow {
            since: std::time::Instant::now(),
            label: "▪ oc mini".to_owned(),
            metadata: " v2.0.24 · ~".to_owned(),
        });
        assert_eq!(footer.pane_height(), super::super::FOOTER_HEIGHT + 1);
        let frame = footer.render_frame(80, footer.pane_height());
        assert_eq!(frame.cells.len(), super::super::FOOTER_HEIGHT + 1);
        // Cell 0: the SEED_LAUNCH spinner (a young age is an outline).
        assert_eq!(frame.cells[0][0].ch, '\u{25ab}');
        // The label tail follows (" oc mini") and the metadata after.
        let text: String = frame.cells[0].iter().map(|cell| cell.ch).collect();
        assert!(text.starts_with('\u{25ab}'), "{text}");
        assert!(text.contains("oc mini"), "{text}");
        assert!(text.contains("v2.0.24"), "{text}");
        // The spinner's glow: the fg is the splash right color scaled
        // by the frame's level (full strength at level 1.0 would be
        // the unscaled color — the level here is the seed head's dim).
        let splash = crate::mini::splash::SplashColors::from_footer_theme(&footer.theme);
        let level = super::super::spinner::seed_launch()
            .one_cell_frame(std::time::Duration::from_millis(0))
            .level;
        assert_eq!(
            frame.cells[0][0].fg,
            apply_glow(crate::render::CellColor::from(splash.right), level)
        );
    }

    /// A label without the ▪ mark (a narrow terminal) renders as
    /// plain text — the ref's `Show when={label.startsWith("▪")}`
    /// never mounts the spinner (`footer.view.tsx:712-717`).
    #[test]
    fn startup_row_without_the_mark_renders_plain_text() {
        let mut footer = pane();
        footer.startup = Some(StartupRow {
            since: std::time::Instant::now(),
            label: "oc m".to_owned(),
            metadata: String::new(),
        });
        let frame = footer.render_frame(80, footer.pane_height());
        let text: String = frame.cells[0].iter().map(|cell| cell.ch).collect();
        assert!(text.starts_with("oc m"), "{text}");
    }

    /// The one-shot's expiry matches the ref's 800 ms setTimeout
    /// (`footer.ts:390`).
    #[test]
    #[allow(clippy::unchecked_time_subtraction)] // the backdated anchors are ~1 s old
    fn startup_row_expires_after_the_window() {
        let mut footer = pane();
        footer.startup = Some(StartupRow {
            since: std::time::Instant::now() - std::time::Duration::from_millis(799),
            label: "▪ oc mini".to_owned(),
            metadata: String::new(),
        });
        assert!(!footer.startup_expired());
        footer.startup = Some(StartupRow {
            since: std::time::Instant::now() - std::time::Duration::from_millis(800),
            label: "▪ oc mini".to_owned(),
            metadata: String::new(),
        });
        assert!(footer.startup_expired());
    }

    fn permission(id: &str) -> PermissionRequest {
        PermissionRequest {
            id: id.into(),
            session_id: "ses".into(),
            action: "bash".into(),
            ..PermissionRequest::default()
        }
    }

    #[test]
    fn pane_heights_match_the_reference_mounts() {
        let mut footer = pane();
        assert_eq!(footer.pane_height(), super::super::FOOTER_HEIGHT);
        footer.permission_asked(permission("p1"));
        assert_eq!(
            footer.pane_height(),
            footer_base(super::super::FOOTER_HEIGHT) + PERMISSION_ROWS,
            "3 + PERMISSION_ROWS(12)"
        );
        footer.machine.permission_replied("p1");
        footer.form_created(crate::components::footer::FormRequest {
            id: "f1".into(),
            session_id: "ses".into(),
            title: "T".into(),
            fields: vec![crate::components::footer::FormField::text("k", "Label")],
        });
        assert_eq!(
            footer.pane_height(),
            footer_base(super::super::FOOTER_HEIGHT) + FORM_ROWS,
            "3 + FORM_ROWS(14)"
        );
    }

    #[test]
    fn two_press_exit_and_draft_clear() {
        let mut footer = pane();
        let _ = footer.press(FooterKey::Printable('h'));
        let effects = footer.press(FooterKey::CtrlC);
        assert!(effects.is_empty(), "no effect on a draft clear");
        assert!(footer.draft.is_empty(), "first ctrl+c clears the draft");
        let _ = footer.press(FooterKey::CtrlC);
        assert_eq!(footer.armed_hint().as_deref(), Some("ctrl+c again to exit"));
        let effects = footer.press(FooterKey::CtrlC);
        assert_eq!(effects, vec![FooterEffect::Close]);
        // Typing resets the arming.
        let mut again = pane();
        let _ = again.press(FooterKey::CtrlC);
        let _ = again.press(FooterKey::Printable('x'));
        assert_eq!(again.armed_hint(), None);
    }

    #[test]
    fn permission_keys_reply_and_release_the_view() {
        let mut footer = pane();
        footer.permission_asked(permission("p1"));
        // "y" = allow once in the permission body vocabulary
        // (`permission_options`: yes/y accepts).
        let effects: Vec<FooterEffect> = footer.press(FooterKey::Enter);
        assert!(
            effects.iter().any(|effect| matches!(effect, FooterEffect::PermissionReply(_))),
            "enter accepts: {effects:?}"
        );
        assert_eq!(footer.machine.view, FooterView::Prompt, "view falls back after reply");
    }

    #[test]
    fn composer_draft_paints_in_the_textarea_region() {
        let mut footer = pane();
        footer.agent = Some("Build".into());
        footer.model_label = Some("gpt-5".into());
        let _ = footer.press(FooterKey::Printable('h'));
        let _ = footer.press(FooterKey::Printable('i'));
        let frame = footer.render_frame(40, super::super::FOOTER_HEIGHT);
        let grid = frame.text_grid();
        assert!(grid[0].starts_with("┃ hi"), "draft in composer row: {:?}", grid[0]);
        assert!(grid[1].starts_with("Build · gpt-5"), "statusline below");
    }
}
