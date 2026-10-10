//! Toast surfaces + the overlay queue (`ui/toast.tsx`): one visible
//! toast with a duration, a pending queue, pause/resume, dismiss and
//! activate — and the bordered surface (variant-colored `┃` rails over
//! the raised-high background, title row + dismiss `x`, message,
//! `+N more`).
//!
//! The queue is wall-clock driven through [`ToastQueue::advance`] (no
//! timers): `show` starts the current toast, `advance(now_ms)` fires
//! the expiry, `dismiss` rotates the next entry in.

use crate::components::footer::canvas::{Canvas, Span};
use crate::components::footer::theme::{feedback_color, FooterTheme};
use crate::render::{Cell, Frame};
use crate::theme::ThemeState;
use crate::wrap::display_width;
use oc_theme::types::ResolvedThemeTokens;

/// `ToastOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastOptions {
    pub title: Option<String>,
    pub message: String,
    /// `info` | `success` | `warning` | `error`.
    pub variant: &'static str,
    pub duration_ms: u64,
    pub action_label: Option<String>,
}

impl ToastOptions {
    #[must_use]
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            title: None,
            message: message.into(),
            variant: "info",
            duration_ms: 5000,
            action_label: None,
        }
    }
}

/// Default toast duration (`init`'s `duration ?? 5000`).
pub const TOAST_DEFAULT_DURATION_MS: u64 = 5000;

/// The toast overlay queue (`ToastContext`): the current surface plus
/// the pending stack.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToastQueue {
    current: Option<ToastOptions>,
    queue: Vec<ToastOptions>,
    /// Deadline of the current toast (`None` while paused); advanced
    /// through [`ToastQueue::advance`].
    deadline_ms: Option<u64>,
    remaining_ms: u64,
    paused: bool,
}

impl ToastQueue {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `show` — displays immediately when idle, else queues behind a
    /// live or paused current toast.
    pub fn show(&mut self, options: ToastOptions) {
        if self.current.is_some() && (self.paused || !self.queue.is_empty()) {
            self.queue.push(options);
            return;
        }
        self.start(options);
    }

    fn start(&mut self, options: ToastOptions) {
        self.remaining_ms = options.duration_ms.max(1);
        self.current = Some(options);
        // The deadline lands once the clock reaches `advance`.
        self.deadline_ms = None;
    }

    /// Fire the expiry for any elapsed current toast; returns the
    /// dismissed options (the store turns them into redraws).
    pub fn advance(&mut self, now_ms: u64) -> Option<ToastOptions> {
        let Some(current) = &self.current else { return None };
        match self.deadline_ms {
            None if !self.paused => {
                self.deadline_ms = Some(now_ms + self.remaining_ms);
                None
            }
            Some(deadline) if now_ms >= deadline => {
                let expired = current.clone();
                self.dismiss();
                Some(expired)
            }
            _ => None,
        }
    }

    /// `dismiss` — clear the current toast, rotating the queue's next
    /// entry in.
    pub fn dismiss(&mut self) {
        self.current = None;
        self.deadline_ms = None;
        if let Some(next) = if self.queue.is_empty() { None } else { Some(self.queue.remove(0)) } {
            self.start(next);
            return;
        }
        self.paused = false;
    }

    /// `activate` — dismiss + run the action (the caller runs it).
    #[must_use]
    pub fn activate(&mut self) -> Option<String> {
        let action = self.current.as_ref().and_then(|toast| toast.action_label.clone());
        self.dismiss();
        action
    }

    /// `pause` — freeze the countdown.
    pub fn pause(&mut self, now_ms: u64) {
        if self.current.is_none() || self.paused {
            return;
        }
        if let Some(deadline) = self.deadline_ms {
            self.remaining_ms = deadline.saturating_sub(now_ms);
        }
        self.deadline_ms = None;
        self.paused = true;
    }

    /// `resume` — restart the remaining countdown.
    pub fn resume(&mut self, now_ms: u64) {
        if self.current.is_none() || !self.paused {
            return;
        }
        self.paused = false;
        self.deadline_ms = Some(now_ms + self.remaining_ms.max(1));
    }

    #[must_use]
    pub fn current(&self) -> Option<&ToastOptions> {
        self.current.as_ref()
    }

    /// `pending` — queued toasts behind the current one.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused
    }
}

/// The toast surface geometry (`ToastSurface`): absolute at `top=1`,
/// `right=2`, intrinsic width = content (capped at
/// `min(60, width - 6)`).
#[must_use]
pub fn toast_geometry(
    message_width: usize,
    title_width: usize,
    term_width: usize,
) -> (usize, usize, usize) {
    // Content = max(message, title + 3 affordance) + 4 padding.
    let content = message_width.max(title_width + 3) + 4;
    let max = 60.min(term_width.saturating_sub(6));
    let panel = content.min(max) + 2; // + the two rail columns
    let left = term_width.saturating_sub(2).saturating_sub(panel);
    (left, panel, 1)
}

/// Render one toast over a blank terminal frame (the overlay region of
/// the `toast-over-session` golden).
#[must_use]
pub fn render_toast_over_blank(
    toast: &ToastOptions,
    pending: usize,
    term_width: usize,
    term_height: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
) -> Frame {
    let mut canvas = Canvas::new(term_width, term_height);
    paint_toast(
        &mut canvas,
        toast,
        pending,
        term_width,
        theme,
        feedback_color(feedback, toast.variant),
    );
    canvas.into_frame()
}

/// Paint the toast surface onto a canvas (absolute `top=1`, `right=2`).
pub fn paint_toast(
    canvas: &mut Canvas,
    toast: &ToastOptions,
    pending: usize,
    term_width: usize,
    theme: &FooterTheme,
    border: crate::render::CellColor,
) {
    let message_width = display_width(&toast.message);
    let title_width = toast.title.as_deref().map_or(0, display_width);
    let (left, panel_width, top) = toast_geometry(message_width, title_width, term_width);
    let bg = theme.pane;
    // The rails sit on the backdrop (the surface behind the overlay —
    // the sidebar in the session route).
    let rail = Span::new("┃", border, theme.surface);
    let content_x = left + 3; // rail + paddingLeft 2
    let content_right = left + panel_width - 4; // last content column (padding 2)
                                                // Every panel row carries the two rails over the backdrop with the
                                                // raised-high interior; the content rows fill in between.
    let rail_row = |canvas: &mut Canvas, row: usize| {
        canvas.spans(row, left, std::slice::from_ref(&rail));
        canvas.fill_bg(row, left + 1, panel_width - 2, bg);
        canvas.spans(row, left + panel_width - 1, std::slice::from_ref(&rail));
    };
    if let Some(title) = &toast.title {
        // paddingTop · title row (+ margin) · message · paddingBottom.
        for row in [top, top + 1, top + 2, top + 3, top + 4] {
            rail_row(canvas, row);
        }
        canvas.spans(top + 1, content_x, &[Span::new(title.clone(), theme.text, bg).bold()]);
        let affordance = vec![
            Span::new("  ", crate::components::footer::canvas::FILL_FG, bg),
            Span::new("x", theme.muted, bg),
        ];
        canvas.spans_right(top + 1, content_right, &affordance);
        canvas.spans(top + 3, content_x, &[Span::new(toast.message.clone(), theme.text, bg)]);
    } else {
        rail_row(canvas, top);
        rail_row(canvas, top + 1);
        canvas.spans(top + 1, content_x, &[Span::new(toast.message.clone(), theme.text, bg)]);
    }
    if pending > 0 {
        canvas.spans(top + 2, content_x, &[Span::new(format!("+{pending} more"), theme.muted, bg)]);
    }
}

/// Convenience: the toast surface rendered with the v2-dark fallback.
#[must_use]
pub fn render_toast_fallback_dark(
    toast: &ToastOptions,
    pending: usize,
    width: usize,
    height: usize,
) -> Frame {
    let theme_state = ThemeState::v2_dark();
    let footer = FooterTheme::from_resolved(&theme_state.resolved.tokens);
    render_toast_over_blank(toast, pending, width, height, &footer, &theme_state.resolved.tokens)
}

/// The toast's panel rows: 5 with a title (paddingTop · title · margin
/// · message · paddingBottom), 2 without.
fn panel_rows(toast: &ToastOptions) -> usize {
    if toast.title.is_some() {
        5
    } else {
        2
    }
}

/// Composite the toast over a LIVE frame (`toast-over-session`): the
/// panel interior replaces the content cells, while the two `┃` rails
/// keep the UNDERLYING cell's background — the reference surface is
/// transparent at its edges (the golden pins the rails over the
/// user-message fill AND over the blank base, per row).
pub fn overlay_toast_over_frame(
    frame: &mut Frame,
    toast: &ToastOptions,
    pending: usize,
    theme: &FooterTheme,
    feedback: &ResolvedThemeTokens,
) {
    let term_width = frame.width;
    let surface =
        render_toast_over_blank(toast, pending, term_width, frame.height, theme, feedback);
    let message_width = display_width(&toast.message);
    let title_width = toast.title.as_deref().map_or(0, display_width);
    let (left, panel_width, top) = toast_geometry(message_width, title_width, term_width);
    let right = left + panel_width - 1;
    for row in top..top + panel_rows(toast) {
        for col in left..=right {
            let Some(slot) = frame.cells.get_mut(row).and_then(|line| line.get_mut(col)) else {
                continue;
            };
            let overlay = surface.cell(row, col);
            if col == left || col == right {
                // Rail: the glyph + feedback foreground over whatever
                // background renders behind the toast.
                *slot = Cell { bg: slot.bg, ..overlay };
            } else {
                *slot = overlay;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice() -> ToastOptions {
        ToastOptions {
            title: Some("Fixture notice".into()),
            message: "Toast rendered over session content".into(),
            variant: "info",
            duration_ms: 60_000,
            action_label: None,
        }
    }

    #[test]
    fn queue_stacks_behind_a_paused_or_busy_toast() {
        let mut queue = ToastQueue::new();
        queue.show(notice());
        assert!(queue.current().is_some());
        assert_eq!(queue.pending(), 0);
        // While live and unpaused, a new show replaces the surface
        // (`show` only queues behind a paused or already-queued toast).
        queue.show(ToastOptions::info("replacement"));
        assert_eq!(queue.pending(), 0);
        assert_eq!(queue.current().unwrap().message, "replacement");
        // Paused: further shows stack.
        queue.advance(0);
        queue.pause(10);
        queue.show(ToastOptions::info("queued"));
        queue.show(ToastOptions::info("second"));
        assert_eq!(queue.pending(), 2, "a paused toast keeps the queue");
        // Advance far past the duration: paused toasts never expire.
        assert!(queue.advance(1_000_000).is_none());
        queue.resume(1_000_000);
        let expired = queue.advance(2_000_000).expect("expired");
        assert_eq!(expired.message, "replacement");
        assert_eq!(queue.pending(), 1, "the next rotated in");
        // The queued toast takes its full turn once resumed.
        assert_eq!(queue.current().unwrap().message, "queued");
        let _ = queue.advance(3_000_000); // arms the deadline
        let expired = queue.advance(3_005_000).expect("expired");
        assert_eq!(expired.message, "queued");
        assert_eq!(queue.current().unwrap().message, "second");
        queue.dismiss();
        assert!(queue.current().is_none(), "the queue drained");
    }

    #[test]
    fn pause_freezes_and_resume_restarts_the_countdown() {
        let mut queue = ToastQueue::new();
        queue.show(ToastOptions { duration_ms: 1000, ..ToastOptions::info("m") });
        queue.advance(0); // deadline = 1000
        queue.pause(400); // 600 remaining
        assert!(queue.is_paused());
        assert!(queue.advance(5000).is_none(), "paused toasts never expire");
        queue.resume(5000); // deadline = 5600
        assert!(queue.advance(5599).is_none());
        assert!(queue.advance(5600).is_some(), "resumed deadline fires");
    }

    #[test]
    fn activate_returns_the_action_and_dismisses() {
        let mut queue = ToastQueue::new();
        queue.show(ToastOptions { action_label: Some("Open".into()), ..ToastOptions::info("m") });
        assert_eq!(queue.activate().as_deref(), Some("Open"));
        assert!(queue.current().is_none());
    }

    #[test]
    fn geometry_pins_the_golden_region() {
        // Golden: panel x37..77 (41 wide), top row 1.
        let (left, panel, top) = toast_geometry(35, 14, 80);
        assert_eq!((left, panel, top), (37, 41, 1));
    }

    #[test]
    fn surface_matches_the_golden_region() {
        let frame = render_toast_fallback_dark(&notice(), 0, 80, 30);
        let grid = frame.text_grid();
        assert_eq!(grid[1].chars().nth(37), Some('┃'), "left rail at x37");
        assert_eq!(grid[1].chars().nth(77), Some('┃'), "right rail at x77");
        assert!(grid[2].contains("Fixture notice"));
        assert!(grid[2].contains('x'), "dismiss affordance");
        assert!(grid[4].contains("Toast rendered over session content"));
        // Title BOLD, rails the info (cyan) feedback color.
        assert!(frame.cell(2, 40).attrs & 1 == 1, "title bold");
        assert_eq!(frame.cell(1, 37).fg, crate::render::CellColor::Rgb(86, 182, 194));
        assert_eq!(frame.cell(2, 40).bg, crate::render::CellColor::Rgb(30, 30, 30), "raised high");
    }
}
