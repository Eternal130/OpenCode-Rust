//! The split-footer renderer lifecycle — port of
//! `packages/tui/src/mini/runtime.lifecycle.ts`.
//!
//! Boots the renderer in **split-footer mode**: raw mode, NO alternate
//! screen — a [`FOOTER_HEIGHT`] inline viewport (ratatui
//! `Viewport::Inline`) stays glued to the bottom of the terminal while
//! transcript rows are written into the terminal's native scrollback
//! above it (`Terminal::insert_before`). Writes the entry splash on
//! boot, paints the footer from the footer family
//! ([`crate::components::footer`]), and tears down in the reference's
//! order on close.
//!
//! ## Exit semantics (`runtime.lifecycle.ts:97-111` + `close`,
//! `:300-343`)
//!
//! 1. exit splash (when `showExit` and splash is shown) flushed to
//!    scrollback;
//! 2. footer `close()` → `destroy()`;
//! 3. `shutdown(renderer)`: leave split-footer mode, destroy;
//! 4. when no exit splash was written: `stdout.write("\n")`
//!    (`runtime.lifecycle.ts:339-341`).
//!
//! The footer's key handling lives in [`FooterPane`] so the state
//! machine is unit-testable without a terminal.

#![allow(clippy::cast_possible_truncation)] // terminal dims sit far below u16
#![allow(clippy::match_same_arms)] // the key/vocabulary arms mirror the reference 1:1

use std::io::Write as _;
use std::time::{Duration, Instant};

use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

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

/// The footer pane's keypress vocabulary (the crossterm event mapping
/// the renderer performs on `KeyEvent`).
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
                        footer_base(FOOTER_HEIGHT)
                            + super::footer_subagent::SUBAGENT_INSPECTOR_ROWS
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
            crate::wrap::flatten(&visible, ratatui::style::Style::new()).into_iter().enumerate()
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

/// `Lifecycle`, `runtime.lifecycle.ts:85-92`).
#[allow(clippy::struct_excessive_bools)] // the renderer flag set mirrors the reference
pub struct Lifecycle {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
    pub pane: FooterPane,
    pending: Vec<PendingRows>,
    /// The last committed transcript row (separator decisions).
    last_commit: Option<Commit>,
    width: usize,
    #[allow(dead_code)] // kept for resize parity with the reference
    height: usize,
    closed: bool,
    /// The last painted footer signature (the dirty check) + a force
    /// flag for post-scroll invalidation (`insert_before` scrolls the
    /// viewport, so the next footer draw must repaint fully).
    last_footer_signature: String,
    force_draw: bool,
    /// TUI-17 — the inline viewport's current height (`applyHeight`:
    /// a taller pane rebuilds the dock at the new row count).
    viewport_rows: u16,
    /// TUI-17/REGRESS — the dock-rebuild cursor probe failed once
    /// (`crossterm::cursor::position` timed out: a headless pty that
    /// answers the BOOT probe but not later ones — CI harnesses).
    /// Once set, height changes stop rebuilding the inline dock
    /// (taller panes render clipped into the boot dock, the
    /// pre-inspector behavior) instead of killing the run loop.
    dock_probe_failed: bool,
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
    /// `runtime.lifecycle.ts:152-205`): raw mode + the inline footer
    /// viewport, entry splash, first footer paint.
    pub fn boot(input: LifecycleInput) -> Result<Self, String> {
        crossterm::terminal::enable_raw_mode().map_err(|error| error.to_string())?;
        // Split-footer mode never enters the alternate screen — the
        // conversation owns the native scrollback; only the footer
        // pane is live-rendered.
        let terminal = Terminal::with_options(
            CrosstermBackend::new(std::io::stdout()),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Inline(super::FOOTER_HEIGHT as u16),
            },
        )
        .map_err(|error| error.to_string())?;
        let area = terminal.size().map_err(|error| error.to_string())?;
        let width = area.width as usize;
        let height = area.height as usize;

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
            terminal,
            pane,
            pending: Vec::new(),
            last_commit: None,
            width,
            height,
            closed: false,
            last_footer_signature: String::new(),
            force_draw: true,
            viewport_rows: super::FOOTER_HEIGHT as u16,
            dock_probe_failed: false,
            splash_enabled: input.splash,
            mono: input.mono,
            shell_output: input.shell_output,
            version: input.version,
            home: input.home,
            directory: input.directory,
        };
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
                let layout =
                    entry_splash_layout(&EntrySplashInput { width: width.max(1), version: &lifecycle.version, detail: Some(&detail), mono: false });
                lifecycle.pane.startup = Some(StartupRow {
                    since: Instant::now(),
                    label: layout.label,
                    metadata: layout.metadata,
                });
            }
        }
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

    /// Flush pending scrollback rows above the inline footer, then
    /// repaint the footer pane (`renderer.writeToScrollback` +
    /// `requestRender`).
    /// Flush pending scrollback rows above the inline footer, then
    /// repaint the footer pane (`renderer.writeToScrollback` +
    /// `requestRender`). `insert_before` hands the writer the raw
    /// buffer — mono rows stamp cluster-by-cluster (the same
    /// [`crate::wrap::flatten`] seam the canvas uses).
    pub fn flush(&mut self) -> Result<(), String> {
        self.finish_startup();
        while let Some(rows) = self.pending.first().cloned() {
            let height = match &rows {
                PendingRows::Plain(rows) => rows.len(),
                PendingRows::Styled(rows) => rows.len(),
            } as u16;
            self.terminal
                .insert_before(height, |buffer| {
                    let origin = {
                        let area = buffer.area();
                        (area.x, area.y)
                    };
                    match &rows {
                        PendingRows::Plain(rows) => {
                            for (index, row) in rows.iter().enumerate() {
                                let mut x: u16 = 0;
                                for atom in crate::wrap::flatten(row, ratatui::style::Style::new())
                                {
                                    if let Some(cell) =
                                        buffer.cell_mut((origin.0 + x, origin.1 + index as u16))
                                    {
                                        cell.set_symbol(&atom.text);
                                    }
                                    x += atom.width.max(1) as u16;
                                }
                            }
                        }
                        PendingRows::Styled(rows) => {
                            for (index, row) in rows.iter().enumerate() {
                                let mut x: u16 = 0;
                                for atom in crate::wrap::flatten_spans(row) {
                                    if let Some(cell) =
                                        buffer.cell_mut((origin.0 + x, origin.1 + index as u16))
                                    {
                                        cell.set_symbol(&atom.text).set_style(atom.style);
                                    }
                                    x += atom.width.max(1) as u16;
                                }
                            }
                        }
                    }
                })
                .map_err(|error| error.to_string())?;
            self.pending.remove(0);
            self.force_draw = true;
        }
        self.draw_footer()
    }

    /// Paint the footer pane into the inline viewport. The viewport
    /// occupies the terminal's BOTTOM rows — paint the footer frame at
    /// the draw area's origin, not at (0, 0).
    pub fn draw_footer(&mut self) -> Result<(), String> {
        let pane_height = self.pane.pane_height();
        // TUI-17/applyHeight: a pane whose height CHANGED rebuilds the
        // inline dock at the new row count (the blocker bodies + the
        // subagent inspector grow it; v2's footer resizes its box).
        // REGRESS (mini permission regression): the Inline
        // reconstruction probes the cursor position (`ESC[6n`) — an
        // environment that leaves the probe unanswered (a headless
        // pty that only emulates the BOOT reply) makes the probe time
        // out; treating that as fatal killed the run loop the moment
        // a permission ask grew the dock. Degrade instead: keep the
        // boot dock (the taller pane renders clipped into it — the
        // pre-inspector behavior) and stop re-probing this session.
        let rows = u16::try_from(pane_height).unwrap_or(u16::MAX);
        if rows != self.viewport_rows {
            let rebuilt = (!self.dock_probe_failed)
                .then(|| {
                    Terminal::with_options(
                        CrosstermBackend::new(std::io::stdout()),
                        ratatui::TerminalOptions { viewport: ratatui::Viewport::Inline(rows) },
                    )
                    .ok()
                })
                .flatten();
            if let Some(terminal) = rebuilt {
                self.terminal = terminal;
                self.force_draw = true;
                self.last_footer_signature = String::new();
            } else {
                // The probe failed (or had failed before): keep the
                // boot dock — the taller pane renders clipped into it.
                self.dock_probe_failed = true;
            }
            self.viewport_rows = rows;
        }
        let footer_frame = self.pane.render_frame(self.width, pane_height);
        // Skip identical repaints (the renderer's dirty check — a
        // no-change draw only emits cursor noise), unless a
        // scrollback write invalidated the viewport.
        let signature = crate::render::serialize_canonical(&footer_frame);
        if !self.force_draw && signature == self.last_footer_signature {
            return Ok(());
        }
        self.last_footer_signature = signature;
        self.force_draw = false;
        self.terminal
            .draw(|frame| {
                let area = frame.area();
                let buffer = frame.buffer_mut();
                let mut offset = BufferOverlay { buffer, origin: (area.x, area.y) };
                paint_frame(&mut offset, &footer_frame);
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
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
        // shutdown(renderer) (`runtime.lifecycle.ts:97-111`): leave
        // split-footer mode before final output so pending writes pass
        // through to the real scrollback. `clearOnShutdown: false`
        // (`runtime.lifecycle.ts:172`) — the last footer frame stays.
        let _ = crossterm::terminal::disable_raw_mode();
        if !wrote_exit {
            // `host.stdout.write("\n")` (`runtime.lifecycle.ts:339-341`).
            let _ = std::io::stdout().write_all(b"\n");
        }
        let _ = std::io::stdout().flush();
    }

    /// Terminal resize (SIGWINCH): re-render at the new size.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        let _ = self.terminal.resize(Rect::new(0, 0, width as u16, height as u16));
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

/// Paint an oc-tui [`crate::render::Frame`] into a ratatui frame
/// buffer (cell-for-cell: chars, colors, attrs). `BufferOverlay` maps
/// frame coordinates onto a positioned buffer region — inline
/// viewports live at the bottom of the terminal, not at (0, 0).
pub struct BufferOverlay<'a> {
    buffer: &'a mut Buffer,
    origin: (u16, u16),
}

impl BufferOverlay<'_> {
    fn cell_mut(&mut self, x: u16, y: u16) -> Option<&mut ratatui::buffer::Cell> {
        self.buffer.cell_mut((self.origin.0 + x, self.origin.1 + y))
    }
}

/// Paint `frame` into `target` at the overlay's origin.
pub fn paint_frame(target: &mut BufferOverlay<'_>, frame: &crate::render::Frame) {
    use ratatui::style::Modifier;
    for (y, row) in frame.cells.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let Some(target_cell) = target.cell_mut(x as u16, y as u16) else {
                continue;
            };
            let symbol = if cell.ch == '\0' { " ".to_owned() } else { cell.ch.to_string() };
            target_cell.set_symbol(&symbol);
            let mut modifier = Modifier::empty();
            if cell.attrs & (1 << 0) != 0 {
                modifier |= Modifier::BOLD;
            }
            if cell.attrs & (1 << 1) != 0 {
                modifier |= Modifier::DIM;
            }
            if cell.attrs & (1 << 2) != 0 {
                modifier |= Modifier::ITALIC;
            }
            if cell.attrs & (1 << 3) != 0 {
                modifier |= Modifier::UNDERLINED;
            }
            if cell.attrs & (1 << 5) != 0 {
                modifier |= Modifier::REVERSED;
            }
            target_cell.set_style(
                ratatui::style::Style::new()
                    .fg(color_of(cell.fg))
                    .bg(color_of(cell.bg))
                    .add_modifier(modifier),
            );
        }
    }
}

fn color_of(color: crate::render::CellColor) -> ratatui::style::Color {
    use crate::render::CellColor;
    use ratatui::style::Color as RColor;
    match color {
        CellColor::Default => RColor::Reset,
        CellColor::Indexed(index) => RColor::Indexed(index),
        CellColor::Rgb(r, g, b) => RColor::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::footer::FooterTheme;

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
        let splash =
            crate::mini::splash::SplashColors::from_footer_theme(&footer.theme);
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
