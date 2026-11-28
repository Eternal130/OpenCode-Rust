//! Toast surfaces + the overlay queue (`ui/toast.tsx`): one visible
//! toast with a duration, a pending queue, pause/resume, dismiss and
//! activate — and the bordered surface (variant-colored `┃` rails over
//! the raised-high background, title row + dismiss `x`, message,
//! `+N more`).
//!
//! The queue is wall-clock driven through [`ToastQueue::advance`] (no
//! timers): `show` starts the current toast, `advance(now_ms)` fires
//! the expiry, `dismiss` rotates the next entry in.

/// `ToastOptions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastOptions {
    pub title: Option<String>,
    pub message: String,
    /// `info` | `success` | `warning` | `error`.
    pub variant: &'static str,
    pub duration_ms: u64,
    pub action_label: Option<String>,
    /// Lane KM (auditA #10): v2's `action.run` closure materialized as
    /// the dot command it dispatches (`mcp.list`, `plugins.list` —
    /// app.tsx:534/541, plugin/context.tsx:484/559); `activate()`
    /// fires it through the palette dispatcher.
    pub action_command: Option<String>,
    /// G-475 — the sessionID arm's action target (api.tsx:108-131: a
    /// toast for a session whose family is NOT open offers `Open` —
    /// `action.run` navigates there). `activate()` fires it through
    /// [`crate::store::Store::toast_activate`]'s session arm.
    pub action_session: Option<String>,
    /// The F11 modal gate: `true` only for the "Session failed" /
    /// "Failed to send prompt" surfaces whose keystroke-swallowing
    /// the F11 lane pinned. Every other toast (v2's rule,
    /// ui/toast.tsx) NEVER blocks typing — the lane KM MCP-failure
    /// toasts ride the non-modal path.
    pub modal: bool,
}

/// G-475 — what the toast activation runs: a palette command (the
/// G-424-family toasts) or a session navigation (the sessionID arm's
/// `Open` action).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastActivation {
    /// The dot command (dispatched through the palette).
    Command(String),
    /// Navigate to the session (route.navigate `{type:"session"}`).
    Session(String),
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
            action_command: None,
            action_session: None,
            modal: false,
        }
    }

    /// An actionable toast (`action: { label, run: () => dispatch }`).
    #[must_use]
    pub fn with_action(mut self, label: &str, command: &str) -> Self {
        self.action_label = Some(label.to_owned());
        self.action_command = Some(command.to_owned());
        self
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

/// The affordance run's text width (`ui/toast.tsx:75-81`):
/// `› {label}` for an action toast, `x` otherwise.
#[must_use]
pub fn affordance_text_width(toast: &ToastOptions) -> usize {
    match &toast.action_label {
        Some(label) => 2 + crate::wrap::display_width(label), // "› {label}"
        None => 1,                                            // "x"
    }
}

/// The toast surfaces' word wrap — the ref title/message texts'
/// default (`wrapMode: "word"`, ui/toast.tsx:76/90): the seam is the
/// opentui text buffer's LAYOUT-WRAP break set (`text_buffer.rs`
/// `ascii_break_kind` — the port of utf8.zig:159-165): whitespace
/// PLUS the punctuation family `- / \ . , ; : ! ? ( ) [ ] { }`,
/// each closing its token (the break char stays with its token — a
/// fitting space rides the line's end, exactly
/// [`crate::wrap::wrap_atoms`]'s packing), a token wider than the
/// line hard-splits by chars. G-517 — the pre-fix port carried only
/// the global seam's space/dot/hyphen (+ the slash): a long
/// single-token JSON route-shape message (the v21b-router arms) was
/// ONE giant token to the port but MANY tokens to the ref (breaking
/// at every `:`/`,`), so the rows packed differently (mid-word
/// hard-splits vs the ref's punctuation boundaries — the wrap WIDTH
/// itself already agreed: rails 58/117, text area 54 on both ends).
fn toast_break_after(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t'
            | '-'
            | '/'
            | '\\'
            | '.'
            | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
    ) || crate::wrap::is_break_after(ch)
}

/// Word-wrap `text` to `width` display cells (greedy packing, the
/// [`ToastLayout`] lane): tokens close after a break char (the break
/// char stays with its token — a fitting space rides the line's end,
/// exactly [`crate::wrap::wrap_atoms`]'s packing), a token wider than
/// the line hard-splits by chars.
#[must_use]
pub fn toast_wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut tokens: Vec<(String, usize)> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    for ch in text.chars() {
        cur.push(ch);
        cur_w += crate::wrap::display_width(&ch.to_string());
        if toast_break_after(ch) {
            tokens.push((std::mem::take(&mut cur), cur_w));
            cur_w = 0;
        }
    }
    if !cur.is_empty() {
        tokens.push((cur, cur_w));
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut line_w = 0usize;
    let push_token =
        |token: &str, w: usize, line: &mut String, line_w: &mut usize, lines: &mut Vec<String>| {
            if w > width {
                for ch in token.chars() {
                    let cw = crate::wrap::display_width(&ch.to_string());
                    if *line_w > 0 && *line_w + cw > width {
                        lines.push(std::mem::take(line));
                        *line_w = 0;
                    }
                    line.push(ch);
                    *line_w += cw;
                }
            } else {
                line.push_str(token);
                *line_w += w;
            }
        };
    for (token, w) in &tokens {
        if line_w > 0 && line_w + w > width {
            lines.push(std::mem::take(&mut line));
            line_w = 0;
        }
        push_token(token, *w, &mut line, &mut line_w, &mut lines);
    }
    lines.push(line);
    lines
}

/// The toast surface geometry (`ToastSurface`): absolute at `top=1`,
/// `right=2`, intrinsic width = content (rails included) capped at
/// `min(60, width - 6)` — the maxWidth clamps the WHOLE box (borders
/// included, ui/toast.tsx:51), so a long title shrinks inside a
/// 60-wide panel instead of widening it past the clamp.
#[must_use]
pub fn toast_geometry(
    message_width: usize,
    title_width: usize,
    affordance_width: usize,
    term_width: usize,
) -> (usize, usize, usize) {
    // Content = max(message, title row) + 4 padding; the title row is
    // title + marginLeft 2 + the affordance run (flexShrink=0, it
    // counts toward the intrinsic width).
    let content = message_width.max(title_width + 2 + affordance_width) + 4;
    let max = 60.min(term_width.saturating_sub(6));
    let panel = (content + 2).min(max); // + the two rail columns, clamped
    let left = term_width.saturating_sub(2).saturating_sub(panel);
    (left, panel, 1)
}

/// The toast panel's full layout: the geometry plus the wrapped
/// content rows (G-424b — the ref title text shrinks and word-wraps
/// inside the clamped panel; the message wraps at the full text area
/// in the title case, or beside the affordance without one).
pub struct ToastLayout {
    pub left: usize,
    pub top: usize,
    pub panel_width: usize,
    pub rows: usize,
    /// The wrapped title lines (empty without a title).
    pub title_lines: Vec<String>,
    /// The wrapped message lines.
    pub message_lines: Vec<String>,
    /// The panel-local row of the `+N more` line (title case only,
    /// the current port's evidenced face).
    pub pending_row: Option<usize>,
}

/// Lay the toast out at `term_width` (ui/toast.tsx:46-99): paddingTop
/// 1, the title row (wrapped at the text area minus the affordance
/// run) + marginBottom 1, the message (wrapped at the text area),
/// `+N more` with marginTop 1, paddingBottom 1; without a title the
/// message shares the affordance's row.
#[must_use]
pub fn toast_layout(toast: &ToastOptions, pending: usize, term_width: usize) -> ToastLayout {
    let aff = affordance_text_width(toast);
    let message_width = crate::wrap::display_width(&toast.message);
    let title_width = toast.title.as_deref().map_or(0, crate::wrap::display_width);
    let (left, panel_width, top) = toast_geometry(message_width, title_width, aff, term_width);
    // The inner text area: panel minus the two rails and the 2+2 padding.
    let text_area = panel_width.saturating_sub(6).max(1);
    if let Some(title) = &toast.title {
        let title_lines = toast_wrap(title, text_area.saturating_sub(2 + aff).max(1));
        let message_lines = toast_wrap(&toast.message, text_area);
        let message_top = 1 + title_lines.len() + 1; // padding + title + margin
        let pending_row = (pending > 0).then_some(message_top + message_lines.len() + 1);
        // pad · title… · margin · message… · [margin · more] · pad
        let rows = message_top + message_lines.len() + 1 + 2 * usize::from(pending > 0);
        ToastLayout { left, top, panel_width, rows, title_lines, message_lines, pending_row }
    } else {
        // The untitled face (ui/toast.tsx:74-81's fallback): the
        // message and the affordance share the row — the message
        // `flexGrow={1}` pushes the affordance run (marginLeft 2,
        // flexShrink 0) to the row's right edge, the message wraps in
        // the remaining text area — and the row count is
        // content-driven like the title face (padding 1 top/bottom,
        // toast.tsx:64-71): the pre-va12 port pinned a 2-row,
        // affordance-less face (unported surface).
        let message_width = message_width + 2 + aff; // the row: message + marginLeft 2 + affordance
        let (left, panel_width, top) = toast_geometry(message_width, 0, 0, term_width);
        // The inner text area: panel minus the two rails and the 2+2 padding.
        let text_area = panel_width.saturating_sub(6).max(1);
        let message_lines = toast_wrap(&toast.message, text_area.saturating_sub(2 + aff).max(1));
        let message_top = 1; // paddingTop 1
        let pending_row = (pending > 0).then_some(message_top + message_lines.len() + 1);
        // pad · message… · [margin · more] · pad
        let rows = message_top + message_lines.len() + 1 + 2 * usize::from(pending > 0);
        ToastLayout {
            left,
            top,
            panel_width,
            rows,
            title_lines: Vec::new(),
            message_lines,
            pending_row,
        }
    }
}

/// [`toast_layout`]'s row count exposed for the app's pointer hit
/// rect (lane KM) and the solid painters.
#[must_use]
pub fn panel_rows_public(toast: &ToastOptions, pending: usize, term_width: usize) -> usize {
    toast_layout(toast, pending, term_width).rows
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
            action_command: None,
            action_session: None,
            modal: false,
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
        let (left, panel, top) = toast_geometry(35, 14, 1, 80);
        assert_eq!((left, panel, top), (37, 41, 1));
    }

    #[test]
    fn action_affordance_widens_the_panel() {
        // G-424: an action toast counts `› {label}` (marginLeft 2)
        // toward the intrinsic width — the dismiss `x` is the 3-wide
        // degenerate of the same run (title + 2 + 1).
        let (left, panel, _top) = toast_geometry(
            26,
            25,
            affordance_text_width(&ToastOptions {
                action_label: Some("Open MCP servers".into()),
                ..ToastOptions::info("")
            }),
            80,
        );
        // title 25 + 2 pads + "› Open MCP servers" (18) + 4 padding
        // + 2 rails = 51 — the ref panel (rails 27/77).
        assert_eq!((left, panel), (27, 51));
    }

    /// G-424b — the ref plugin-fallback grids' long-title shape: the
    /// panel clamps at 60 (rails included) and the title word-wraps
    /// three lines (break after '-' and '/'), the message below the
    /// marginBottom row, 7 rows total.
    #[test]
    fn long_title_shrinks_and_wraps_inside_the_clamped_panel() {
        let toast = ToastOptions {
            title: Some(
                "Plugin failed: /tmp/opencode/diff-tuifull/plugins-ref/\
                 node_modules/broken-plugin"
                    .to_owned(),
            ),
            message: "Run /plugins to view details.".to_owned(),
            variant: "error",
            action_label: Some("Open plugins".to_owned()),
            action_command: None,
            action_session: None,
            ..ToastOptions::info("")
        };
        let layout = toast_layout(&toast, 0, 80);
        assert_eq!(layout.panel_width, 60, "maxWidth clamps the whole box");
        assert_eq!((layout.left, layout.top), (18, 1));
        assert_eq!(
            layout.title_lines,
            vec![
                "Plugin failed: /tmp/opencode/diff-",
                "tuifull/plugins-ref/node_modules/",
                "broken-plugin",
            ],
            "the title wraps after the hyphen and the trailing slash"
        );
        assert_eq!(layout.message_lines, vec!["Run /plugins to view details."]);
        assert_eq!(layout.rows, 7, "pad + 3 title + margin + message + pad");
        assert_eq!(layout.pending_row, None);
        // The wrapped words themselves: '/' joins the break set.
        assert_eq!(toast_wrap("aaa/bb/ccc", 7), vec!["aaa/bb/", "ccc"]);
    }

    /// A pending queue behind a titled toast: the `+N more` line sits
    /// one marginTop row past the message.
    #[test]
    fn pending_more_rides_one_margin_row_past_the_message() {
        let layout = toast_layout(&notice(), 2, 80);
        assert_eq!(layout.rows, 7);
        assert_eq!(layout.pending_row, Some(5));
    }

    /// T2 (va12) — the untitled face is the ref's fallback shape
    /// (toast.tsx:64-81): the message and the affordance share the
    /// first content row, the panel width counts the affordance run
    /// (`message + marginLeft 2 + affordance` intrinsic row), and the
    /// row count is content-driven — pad + content + pad (3 rows for a
    /// one-line message; the pre-va12 port pinned 2 rows with no
    /// affordance cell). A pending queue rides the `+N more` margin
    /// row like the title face.
    #[test]
    fn untitled_face_shares_the_message_row_with_the_affordance() {
        let toast = ToastOptions::info("Copied to clipboard");
        let layout = toast_layout(&toast, 0, 80);
        // Geometry: 19 msg + 2 + 1 aff + 4 padding + 2 rails = 28.
        assert_eq!(layout.panel_width, 28, "the affordance run widens the panel");
        assert_eq!((layout.left, layout.top), (50, 1));
        assert_eq!(layout.message_lines, vec!["Copied to clipboard"]);
        assert_eq!(layout.rows, 3, "pad + message/affordance row + pad");
        assert_eq!(layout.pending_row, None);
        // A long message wraps inside the row's shrunk text area
        // (minus the affordance run), growing the row count.
        let long =
            ToastOptions::info("the quick brown fox jumps over the lazy dog again and again");
        let layout = toast_layout(&long, 0, 80);
        assert_eq!(layout.panel_width, 60, "maxWidth clamps the whole box");
        assert!(layout.rows > 3, "a clamped message wraps: {} rows", layout.rows);
        for line in &layout.message_lines {
            assert!(
                crate::wrap::display_width(line) < 60 - 6 - 2,
                "the message wraps inside the affordance-shrunk area"
            );
        }
        // The pending line rides one margin row past the message.
        let layout = toast_layout(&toast, 2, 80);
        assert_eq!(layout.rows, 5, "pad + message + margin + more + pad");
        assert_eq!(layout.pending_row, Some(3));
    }

    #[test]
    fn surface_matches_the_golden_region() {
        let theme_state = crate::theme::ThemeState::v2_dark();
        let footer =
            crate::components::footer::FooterTheme::from_resolved(&theme_state.resolved.tokens);
        let frame = crate::components::toast_solid::toast_over_blank_frame(
            &notice(),
            0,
            80,
            30,
            &footer,
            &theme_state.resolved.tokens,
        );
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

    /// T2 (va12) — the untitled face renders the affordance cell and
    /// the bottom pad row (ui/toast.tsx:64-81): the `x` shares the
    /// message's row (right-aligned), the rails span all three rows,
    /// and the row under the message is blank raised-high padding.
    #[test]
    fn untitled_surface_paints_the_affordance_and_bottom_pad() {
        let theme_state = crate::theme::ThemeState::v2_dark();
        let footer =
            crate::components::footer::FooterTheme::from_resolved(&theme_state.resolved.tokens);
        let toast = ToastOptions::info("Copied to clipboard");
        let frame = crate::components::toast_solid::toast_over_blank_frame(
            &toast,
            0,
            80,
            30,
            &footer,
            &theme_state.resolved.tokens,
        );
        let grid = frame.text_grid();
        // Rails on rows 1..3 (pad + content + pad).
        for (row, text) in grid.iter().enumerate().skip(1).take(3) {
            assert_eq!(text.chars().nth(50), Some('┃'), "left rail row {row}");
            assert_eq!(text.chars().nth(77), Some('┃'), "right rail row {row}");
        }
        // The message row (row 2) carries the message AND the affordance.
        assert!(grid[2].contains("Copied to clipboard"));
        assert!(grid[2].contains('x'), "the dismiss affordance shares the message row");
        // The affordance right-aligns: the `x` lands just inside the
        // right rail (pads + x = 3 cells ending at content_right).
        assert_eq!(grid[2].chars().nth(74), Some('x'), "affordance at content_right");
        // The bottom pad row is blank interior (no message leakage).
        assert!(!grid[3].contains("Copied"), "row 3 is padding");
    }
}
