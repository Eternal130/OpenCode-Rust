//! Permission body (`mini/footer.permission.tsx`,
//! `permission.shared.ts`, `util/permission.ts`): the three-stage ask
//! (permission, always, reject), the Allow once / Always allow /
//! Reject affordances, the free-text [`RejectField`], and the request
//! presentation (per-action icon/title/lines).
//!
//! The permission STATE MODEL (the request/reply types, the
//! presentation solver, the two-stage machine and its
//! [`permission_key`] keymap) lives in [`super::state`] — extracted
//! painter-agnostic in lane R2 step 1; this module re-exports it and
//! keeps the painter + the pointer-hit geometry.

#![allow(clippy::too_many_lines, clippy::match_same_arms)]

use super::canvas::{Canvas, Span, FILL_FG};
use super::theme::FooterTheme;
use crate::render::Frame;

pub use super::state::{
    create_permission_body_state, permission_always_lines, permission_cancel, permission_key,
    permission_offered_options, permission_option_label, permission_options,
    permission_presentation, permission_reject, permission_run, permission_shift,
    permission_type_message, PermissionBodyState, PermissionKey, PermissionOption,
    PermissionPresentation, PermissionReply, PermissionRequest, PermissionStage, PermissionStep,
};

// -- rendering ---------------------------------------------------------------

const PERMISSION_ICON: &str = "△";

fn permission_title(stage: PermissionStage, width: usize) -> &'static str {
    if stage == PermissionStage::Reject {
        return if width < 24 { "Reject" } else { "Reject permission" };
    }
    if width < 24 {
        "Permission"
    } else {
        "Permission required"
    }
}

/// Render the permission body (`footer-permission` golden): one spacer
/// row, the surface panel (header / scroll region with the █ scrollbar
/// track / pane controls row), one spacer row.
#[must_use]
pub fn render_permission_body(
    request: &PermissionRequest,
    state: &PermissionBodyState,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    let mut canvas = Canvas::new(width, height);
    let compact = width < 56 || height < 12;
    if height < 2 {
        return canvas.into_frame();
    }
    // The body fills everything below the one-row spacer above it:
    // header (3) · content (viewport rows + paddingBottom) · controls (3).
    let panel_end = height - 1; // inclusive
    for y in 1..=panel_end {
        canvas.fill_bg(y, 0, width, theme.surface);
    }
    // Header: pad(1) + title + pad(1).
    let title = permission_title(state.stage, width);
    canvas.spans(
        2,
        2,
        &[
            Span::new(format!("{PERMISSION_ICON} "), theme.permission, theme.surface),
            Span::new(title, theme.text, theme.surface),
        ],
    );
    // Content region: the scroll viewport rows (with the █ track).
    let content_start = 4;
    let content_end = panel_end.saturating_sub(4); // last viewport row
                                                   // Scrollbar track along the region's right edge (paddingRight 3).
    let track_x = width.saturating_sub(4);
    for y in content_start..=content_end {
        canvas.spans(y, track_x, &[Span::new("█", theme.line, theme.surface)]);
    }
    let text_x = 2;
    match state.stage {
        PermissionStage::Permission => {
            // The ask's body is a function of the SELECTION
            // (`body={(option) => ...}`, permission.tsx:223-231):
            // hovering "Always allow" swaps the presentation for the
            // always-lines preview (the two-stage replacement for the
            // old confirm stage).
            if state.selected == PermissionOption::Always {
                for (index, line) in
                    permission_always_lines(&request.action, &request.save).iter().enumerate()
                {
                    canvas.spans(
                        content_start + index,
                        text_x,
                        &[Span::new(line.clone(), theme.text, theme.surface)],
                    );
                }
            } else {
                let info = permission_presentation(request);
                canvas.spans(
                    content_start,
                    text_x + 1,
                    &[
                        Span::new(format!("{} ", info.icon), theme.muted, theme.surface),
                        Span::new(info.title.clone(), theme.text, theme.surface),
                    ],
                );
                for (index, line) in info.lines.iter().enumerate() {
                    canvas.spans(
                        content_start + 2 + index,
                        text_x,
                        &[Span::new(line.clone(), theme.text, theme.surface)],
                    );
                }
            }
        }
        PermissionStage::Reject => {
            canvas.spans(
                content_start,
                text_x,
                &[Span::new("Tell OpenCode what to do differently", theme.muted, theme.surface)],
            );
        }
    }
    // Controls rows on the pane background (pad(1) · actions · pad(1)).
    let controls_y = panel_end - 1;
    for y in (panel_end - 2)..=panel_end {
        canvas.fill_bg(y, 0, width, theme.pane);
    }
    render_permission_controls(&mut canvas, controls_y, request, state, width, theme, compact);
    canvas.into_frame()
}

/// The controls row: buttons left (padding 2), hints right-aligned
/// before paddingRight 3.
fn render_permission_controls(
    canvas: &mut Canvas,
    y: usize,
    request: &PermissionRequest,
    state: &PermissionBodyState,
    width: usize,
    theme: &FooterTheme,
    compact: bool,
) {
    let options = permission_offered_options(state.stage, request);
    let mut x = 2;
    for (index, option) in options.iter().enumerate() {
        let label = permission_option_label(*option);
        let selected = *option == state.selected;
        let bg = if selected { theme.action_focused_bg } else { theme.pane };
        if index > 0 {
            canvas.fill_bg(y, x, 1, theme.pane);
            x += 1;
        }
        canvas.fill_bg(y, x, label.chars().count() + 2, bg);
        let fg = if selected { theme.action_focused_text } else { theme.action_secondary_text };
        canvas.spans(y, x + 1, &[Span::new(label, fg, bg)]);
        x += label.chars().count() + 2;
    }
    if state.stage == PermissionStage::Reject {
        let spans = vec![
            Span::new("enter ", theme.text, theme.pane),
            Span::new("confirm", theme.muted, theme.pane),
            Span::new("  ", FILL_FG, theme.pane),
            Span::new("esc ", theme.text, theme.pane),
            Span::new("cancel", theme.muted, theme.pane),
        ];
        canvas.spans_right(y, width - 4, &spans);
        return;
    }
    let hint: Vec<Span> = if compact && width < 56 {
        vec![
            Span::new("pgup/pgdn", theme.text, theme.pane),
            Span::new(" scroll", theme.muted, theme.pane),
        ]
    } else {
        vec![
            Span::new("⇆", theme.text, theme.pane),
            Span::new(" select  ", theme.muted, theme.pane),
            Span::new("enter", theme.text, theme.pane),
            Span::new(" confirm  ", theme.muted, theme.pane),
            Span::new("esc", theme.text, theme.pane),
            Span::new(" reject", theme.muted, theme.pane),
        ]
    };
    canvas.spans_right(y, width - 4, &hint);
}

// -- the pointer surface (lane/if item 3, auditA #4) -------------------------

/// The permission body's pointer targets, at the SAME body-local
/// coordinates the canvas paints (`render_permission_controls`'s
/// layout): the offered option pills (stages Permission/Always) and,
/// in the reject stage, the `enter confirm` / `esc cancel` hint
/// BUTTONS (v2 renders both as `onMouseUp` boxes — permission.tsx:
/// 375-400). The caller translates by the mount origin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionPointerHits {
    /// The controls row (body-local y).
    pub row: usize,
    /// The offered pills left-to-right: the option + its rect.
    pub pills: Vec<(PermissionOption, (usize, usize))>,
    /// The reject stage's confirm button rect (`enter confirm`) —
    /// `(x, width)`, body-local.
    pub confirm: Option<(usize, usize)>,
    /// The reject stage's cancel button rect (`esc cancel`).
    pub cancel: Option<(usize, usize)>,
}

/// The controls row's pointer layout — mirrors
/// [`render_permission_controls`]: pills from x=2, each
/// `label.chars().count() + 2` wide with a 1-column gap (index > 0);
/// the reject-stage hints right-align as buttons ending at
/// `width - 4` (the row's paddingRight 3 + the canvas's 1-col rim).
#[must_use]
pub fn permission_pointer_hits(
    request: &PermissionRequest,
    state: &PermissionBodyState,
    width: usize,
    height: usize,
) -> PermissionPointerHits {
    let mut hits = PermissionPointerHits { row: height.saturating_sub(2), ..Default::default() };
    if height < 2 {
        return PermissionPointerHits::default();
    }
    let options = permission_offered_options(state.stage, request);
    let mut x = 2usize;
    for (index, option) in options.iter().enumerate() {
        let label = permission_option_label(*option);
        let pill_width = label.chars().count() + 2;
        if index > 0 {
            x += 1; // the inter-pill gap
        }
        hits.pills.push((*option, (x, pill_width)));
        x += pill_width;
    }
    if state.stage == PermissionStage::Reject {
        // The right-aligned hint block: `enter confirm  esc cancel`
        // (25 columns) ending at width - 4; each BUTTON is its own
        // label's extent (v2's two sibling boxes).
        let block = "enter confirm".chars().count() + 2 + "esc cancel".chars().count();
        let start = (width.saturating_sub(4)).saturating_sub(block);
        let confirm_width = "enter confirm".chars().count();
        let cancel_width = "esc cancel".chars().count();
        hits.confirm = Some((start, confirm_width));
        hits.cancel = Some((start + confirm_width + 2, cancel_width));
    }
    hits
}

/// Render the standalone [`RejectField`] (`permission-reject-field`
/// golden): a full-width focused textarea whose placeholder is the
/// muted prompt over the formfield focused background.
#[must_use]
pub fn render_reject_field(
    placeholder_visible: bool,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    let mut canvas = Canvas::new(width, height);
    if height == 0 {
        return canvas.into_frame();
    }
    canvas.fill_bg(0, 0, width, theme.formfield_focused_bg);
    if placeholder_visible {
        canvas.spans(
            0,
            0,
            &[Span::new(
                "Tell OpenCode what to do differently",
                theme.muted,
                theme.formfield_focused_bg,
            )],
        );
    }
    canvas.into_frame()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap::display_width;

    fn request(action: &str, resources: &[&str]) -> PermissionRequest {
        PermissionRequest {
            id: "per_dump".into(),
            session_id: "ses_dump".into(),
            action: action.into(),
            resources: resources.iter().map(|item| (*item).to_owned()).collect(),
            ..PermissionRequest::default()
        }
    }

    #[test]
    fn read_presentation_matches_the_golden_fixture() {
        let request = request("read", &["src/index.ts"]);
        let info = permission_presentation(&request);
        assert_eq!(info.icon, "→");
        assert_eq!(info.title, "Read src/index.ts");
        assert_eq!(info.lines, vec!["Path: src/index.ts".to_owned()]);
    }

    #[test]
    fn presentation_covers_the_action_family() {
        let grep = request("grep", &["pattern.*x"]);
        assert_eq!(permission_presentation(&grep).icon, "✱");
        assert_eq!(permission_presentation(&grep).title, "Grep \"pattern.*x\"");
        let shell = PermissionRequest {
            input: serde_json::json!({ "command": "ls" }),
            ..request("bash", &[])
        };
        assert_eq!(permission_presentation(&shell).lines, vec!["$ ls".to_owned()]);
        assert_eq!(
            permission_presentation(&request("unknown_tool", &[])).title,
            "Call tool unknown_tool"
        );
    }

    #[test]
    fn always_option_requires_save_patterns() {
        let mut request = request("edit", &["a.ts"]);
        assert_eq!(
            permission_offered_options(PermissionStage::Permission, &request),
            vec![PermissionOption::Once, PermissionOption::Reject]
        );
        request.save = vec!["*".into()];
        assert_eq!(
            permission_offered_options(PermissionStage::Permission, &request),
            vec![PermissionOption::Once, PermissionOption::Always, PermissionOption::Reject]
        );
    }

    #[test]
    fn shift_wraps_the_offered_options() {
        let request = request("read", &[]);
        let state = create_permission_body_state(&request);
        let offered = permission_offered_options(PermissionStage::Permission, &request);
        let right = permission_shift(&state, 1, &offered);
        assert_eq!(right.selected, PermissionOption::Reject);
        let wrap = permission_shift(&right, 1, &offered);
        assert_eq!(wrap.selected, PermissionOption::Once);
    }

    #[test]
    fn run_walks_the_two_stage_machine() {
        let mut request = request("read", &[]);
        request.save = vec!["src/**".into()];
        let state = create_permission_body_state(&request);
        // once → immediate reply.
        let once = permission_run(&state, PermissionOption::Once, false);
        assert_eq!(once.reply.as_ref().unwrap().decision, "once");
        // always → SINGLE-enter immediate reply (TD18: the old
        // three-stage machine staged a Confirm round first).
        let always = permission_run(&state, PermissionOption::Always, false);
        assert_eq!(always.reply.as_ref().unwrap().decision, "always");
        assert_eq!(always.state.stage, PermissionStage::Permission, "no confirm stage");
        // reject on a TOP-LEVEL session → immediate reply.
        let reject = permission_run(&state, PermissionOption::Reject, false);
        assert_eq!(reject.reply.as_ref().unwrap().decision, "reject");
        assert_eq!(reject.state.stage, PermissionStage::Permission);
        // reject on a CHILD session (parentID) → the free-text stage.
        let staged = permission_run(&state, PermissionOption::Reject, true);
        assert!(staged.reply.is_none(), "the child reject stages first");
        assert_eq!(staged.state.stage, PermissionStage::Reject);
        let with_message = PermissionBodyState { message: "  no way  ".into(), ..staged.state };
        let reply = permission_reject(&with_message).unwrap();
        assert_eq!(reply.decision, "reject");
        assert_eq!(reply.message, "no way", "trimmed");
        // The stage's esc returns to the ask, selection on Reject.
        let cancelled = permission_cancel(&with_message);
        assert_eq!(cancelled.stage, PermissionStage::Permission);
        assert_eq!(cancelled.selected, PermissionOption::Reject);
    }

    /// TD18: the keymap drives the two-stage flow — escape on a
    /// top-level session replies `reject` IMMEDIATELY; on a child it
    /// opens the free-text stage; `always` fires on one enter.
    #[test]
    fn key_handler_drives_the_two_stage_flow() {
        let mut request = request("read", &[]);
        request.save = vec!["*".into()];
        let mut state = create_permission_body_state(&request);
        // right → "Always allow" selected; enter replies IMMEDIATELY.
        state = match permission_key(&state, &request, false, "right") {
            PermissionKey::State(next) => next,
            other => panic!("right shifts: {other:?}"),
        };
        assert_eq!(state.selected, PermissionOption::Always);
        let reply = match permission_key(&state, &request, false, "enter") {
            PermissionKey::Reply(reply, _) => reply,
            other => panic!("always replies on one enter: {other:?}"),
        };
        assert_eq!(reply.decision, "always");
        // escape on a TOP-LEVEL session → immediate reject (the old
        // machine opened the free-text prompt and never replied).
        let state = create_permission_body_state(&request);
        let reply = match permission_key(&state, &request, false, "escape") {
            PermissionKey::Reply(reply, _) => reply,
            other => panic!("esc rejects immediately: {other:?}"),
        };
        assert_eq!(reply.decision, "reject");
        // escape on a CHILD session → the free-text stage; typing
        // builds the message; enter confirms.
        let state = create_permission_body_state(&request);
        let mut staged = match permission_key(&state, &request, true, "escape") {
            PermissionKey::State(next) => next,
            other => panic!("esc stages the child reject: {other:?}"),
        };
        assert_eq!(staged.stage, PermissionStage::Reject);
        for ch in ["c", "h", "o", "o", "s", "e", " "] {
            staged = match permission_key(&staged, &request, true, ch) {
                PermissionKey::Text(ch) => permission_type_message(&staged, ch),
                other => panic!("{ch} types: {other:?}"),
            };
        }
        assert_eq!(staged.message, "choose ");
        let reply = match permission_key(&staged, &request, true, "enter") {
            PermissionKey::Reply(reply, _) => reply,
            other => panic!("enter confirms the reject: {other:?}"),
        };
        assert_eq!(reply.decision, "reject");
        assert_eq!(reply.message, "choose", "the reply message is trimmed");
        // The stage's esc cancels back to the ask.
        let back = match permission_key(&staged, &request, true, "escape") {
            PermissionKey::State(next) => next,
            other => panic!("esc cancels: {other:?}"),
        };
        assert_eq!(back.stage, PermissionStage::Permission);
    }

    /// TD19: the edit presentation carries the diff preview — the
    /// first file's diff/patch, a metadata diff, or input.patchText.
    #[test]
    fn edit_presentation_renders_the_diff_preview() {
        let mut ask = PermissionRequest {
            input: serde_json::json!({ "path": "src/lib.rs" }),
            metadata: serde_json::json!({
                "files": [ { "diff": "@@ -1,2 +1,2 @@\n const old = 1\n-const gone = 2\n+const fresh = 3\n" } ]
            }),
            ..request("edit", &[])
        };
        let info = permission_presentation(&ask);
        assert_eq!(info.title, "Edit src/lib.rs");
        assert!(info.lines.contains(&"-const gone = 2".to_owned()), "lines: {:?}", info.lines);
        assert!(info.lines.contains(&"+const fresh = 3".to_owned()));
        // The patchText fallback (no files, no metadata diff).
        ask.metadata = serde_json::json!({});
        ask.input = serde_json::json!({ "path": "a.ts", "patchText": "+added" });
        let info = permission_presentation(&ask);
        assert_eq!(info.lines, vec!["+added".to_owned()]);
        // Neither → the muted fallback.
        ask.input = serde_json::json!({ "path": "a.ts" });
        let info = permission_presentation(&ask);
        assert_eq!(info.lines, vec!["No diff provided".to_owned()]);
    }

    #[test]
    fn always_lines_cover_wildcard_and_patterns() {
        assert_eq!(
            permission_always_lines("read", &["*".to_owned()]),
            vec!["This will always allow read for this project.".to_owned()]
        );
        assert_eq!(
            permission_always_lines("edit", &["a.ts".to_owned(), "b.ts".to_owned()]),
            vec![
                "This will always allow the following patterns for this project.".to_owned(),
                "- a.ts".to_owned(),
                "- b.ts".to_owned(),
            ]
        );
    }

    #[test]
    fn golden_frame_shape_at_80() {
        let theme = FooterTheme::fallback_dark();
        let request = request("read", &["src/index.ts"]);
        let state = create_permission_body_state(&request);
        let frame = render_permission_body(&request, &state, 80, 16, &theme);
        let grid = frame.text_grid();
        assert!(grid[2].trim_start().starts_with("△ Permission required"));
        assert!(grid[4].contains("→ Read src/index.ts"));
        assert!(grid[6].contains("Path: src/index.ts"));
        assert!(grid[14].contains("Allow once"));
        assert!(grid[14].contains("Reject"));
        assert!(grid[14].contains("⇆ select  enter confirm  esc reject"));
        assert_eq!(grid[15].chars().filter(|ch| *ch != ' ').count(), 0, "pane pad row");
        assert!(display_width(&grid[13]) <= 80);
        assert_eq!(frame.cell(13, 0).bg, theme.pane, "controls pad on the pane bg");
        assert_eq!(frame.cell(14, 0).bg, theme.pane, "actions row on the pane bg");
        assert_eq!(frame.cell(4, 76).ch, '█', "scrollbar track");
    }
}
