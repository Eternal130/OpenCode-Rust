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
use super::splash::{directory_label, entry_splash_rows, exit_splash_rows};
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
}

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
        }
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
            FooterKey::Up => effects.push(FooterEffect::HistoryUp),
            FooterKey::Down => effects.push(FooterEffect::HistoryDown),
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

    /// The footer pane height for the active view (`applyHeight`,
    /// `footer.ts:740-760`): blockers grow the dock over the composer
    /// base (`PERMISSION_ROWS`/`FORM_ROWS`, `footer.ts:117-118`).
    #[must_use]
    pub fn pane_height(&self) -> usize {
        match self.machine.view {
            FooterView::Permission { .. } => footer_base(FOOTER_HEIGHT) + PERMISSION_ROWS,
            FooterView::Form { .. } => footer_base(FOOTER_HEIGHT) + FORM_ROWS,
            FooterView::Prompt => FOOTER_HEIGHT,
        }
    }

    /// Compose the footer's [`crate::render::Frame`] (the dispatch the
    /// render loop performs on `FooterView`).
    #[must_use]
    pub fn render_frame(&self, width: usize, height: usize) -> crate::render::Frame {
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
                // The composer base: rail + transparent textarea with
                // the live draft, statusline below.
                let idle = PromptIdleState {
                    agent: self.agent.clone().unwrap_or_else(|| "Build".to_owned()),
                    model_label: self
                        .model_label
                        .clone()
                        .unwrap_or_else(|| "Default model".to_owned()),
                    command_shortcut: "ctrl+p".to_owned(),
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
                    paint_busy_cell(
                        &mut frame,
                        motion.frame(since.elapsed()),
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
/// narrowed width, so the cells shift into place.
fn paint_busy_cell(
    frame: &mut crate::render::Frame,
    glyph: char,
    color: crate::render::CellColor,
    width: usize,
) {
    let Some(row) = frame.cells.get_mut(1) else { return };
    let spacer = crate::render::Cell { ch: ' ', fg: color, ..crate::render::Cell::default() };
    let cell = crate::render::Cell { ch: glyph, fg: color, ..crate::render::Cell::default() };
    row.insert(0, cell);
    row.insert(1, spacer);
    row.truncate(width);
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
            splash_enabled: input.splash,
            mono: input.mono,
            shell_output: input.shell_output,
            version: input.version,
            home: input.home,
            directory: input.directory,
        };
        // `queueSplash(entry)` (`runtime.lifecycle.ts:191-203`): the
        // splash starts the transcript one row below prior output
        // (paddingTop 1 → blank row + label row).
        if lifecycle.splash_enabled {
            let detail = directory_label(&lifecycle.directory, &lifecycle.home);
            let rows =
                entry_splash_rows(&lifecycle.version, Some(&detail), lifecycle.mono, width.max(1));
            lifecycle.queue_rows(rows);
        }
        lifecycle.flush()?;
        Ok(lifecycle)
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
        let footer_frame = self.pane.render_frame(self.width, self.pane.pane_height());
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
