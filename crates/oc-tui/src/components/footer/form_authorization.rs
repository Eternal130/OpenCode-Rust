//! Session-route external authorization form
//! (`routes/session/form.tsx` `FormPrompt`, the `form-authorization`
//! golden): the bordered surface with the tab row (field tabs +
//! Submit), the external field (title / URL / status line) and the
//! `⇆ tab · enter open link · c copy · esc dismiss` hint row.

use super::canvas::{Canvas, Span, FILL_FG};
use super::theme::FooterTheme;
use crate::components::footer::form::FormRequest;
use crate::render::Frame;

/// The authorization form state (answers + external readiness).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormAuthorizationState {
    /// Which tab is active (field index; the Submit tab is
    /// `fields.len()`).
    pub tab: usize,
    pub external_ready: bool,
    pub acknowledged: bool,
}

impl FormAuthorizationState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `actionLabel` for the external field.
    #[must_use]
    pub fn action_label(&self) -> &'static str {
        if self.acknowledged {
            "continue"
        } else if self.external_ready {
            "I finished"
        } else {
            "open link"
        }
    }
}

/// Key outcomes for the authorization form (`FormPrompt` layer:
/// tab / enter / c / esc).
#[derive(Debug, Clone, PartialEq)]
pub enum FormAuthorizationKey {
    NextTab,
    OpenLink,
    Acknowledge,
    Copy,
    Dismiss,
    Ignored,
}

/// The form's key handler.
#[must_use]
pub fn form_authorization_key(
    state: &FormAuthorizationState,
    _tabs: usize,
    key: &str,
) -> FormAuthorizationKey {
    match key {
        "tab" => FormAuthorizationKey::NextTab,
        "enter" | "return" => {
            if state.acknowledged || state.external_ready {
                FormAuthorizationKey::Acknowledge
            } else {
                FormAuthorizationKey::OpenLink
            }
        }
        "c" => FormAuthorizationKey::Copy,
        "escape" => FormAuthorizationKey::Dismiss,
        _ => FormAuthorizationKey::Ignored,
    }
}

/// Advance the tab (wrapping across the Submit tab).
#[must_use]
pub fn form_authorization_next_tab(
    state: &FormAuthorizationState,
    tabs: usize,
) -> FormAuthorizationState {
    FormAuthorizationState { tab: (state.tab + 1) % tabs.max(1), ..state.clone() }
}

/// `truncate(formLabel(item), 24)`.
#[must_use]
fn truncate_label(label: &str, max: usize) -> String {
    if label.chars().count() > max {
        let mut text: String = label.chars().take(max - 1).collect();
        text.push('…');
        text
    } else {
        label.to_owned()
    }
}

/// Render the external authorization form (`form-authorization`
/// golden): `┃`-bordered raised surface, title, tab row, external
/// field block, hint row.
#[must_use]
pub fn render_form_authorization(
    request: &FormRequest,
    state: &FormAuthorizationState,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Frame {
    super::solid::finish(form_authorization_canvas(request, state, width, height, theme))
}

/// [`render_form_authorization`]'s canvas (the parity tests' op source).
pub(crate) fn form_authorization_canvas(
    request: &FormRequest,
    state: &FormAuthorizationState,
    width: usize,
    height: usize,
    theme: &FooterTheme,
) -> Canvas {
    let mut canvas = Canvas::new(width, height);
    let panel_rows = height.saturating_sub(2).min(13);
    // Below the panel the renderer's background shows through.
    for y in panel_rows..height {
        canvas.fill_bg(y, 0, width, theme.background);
    }
    for y in 0..panel_rows {
        canvas.fill_bg(y, 0, width, theme.surface);
        canvas.spans(y, 0, &[Span::new("┃", theme.action_focused_bg, theme.surface)]);
    }
    let x = 3;
    // Title.
    canvas.spans(1, x, &[Span::new(request.title.clone(), theme.muted, theme.surface)]);
    // Tab row: field tabs (BOLD active, paddingRight 2) + Submit.
    let submit_at = request.fields.len();
    let mut tabs: Vec<Span> = Vec::new();
    for (index, field) in request.fields.iter().enumerate() {
        let label = truncate_label(field.label(), 24);
        let active = index == state.tab;
        let span = Span::new(label, if active { theme.text } else { theme.muted }, theme.surface);
        tabs.push(if active { span.bold() } else { span });
        tabs.push(Span::new("  ", FILL_FG, theme.surface));
    }
    let submit_active = state.tab == submit_at;
    let submit =
        Span::new("Submit", if submit_active { theme.text } else { theme.muted }, theme.surface);
    tabs.push(if submit_active { submit.bold() } else { submit });
    canvas.spans(3, x, &tabs);
    // External field block (title / URL / status).
    if let Some(field) = request.fields.get(state.tab.min(request.fields.len().saturating_sub(1))) {
        let url = match &field.kind {
            super::form::FieldKind::External { url } => url.clone(),
            _ => String::new(),
        };
        canvas.spans(5, x, &[Span::new(field.label().to_owned(), theme.text, theme.surface)]);
        canvas.spans(7, x, &[Span::new(url, theme.text, theme.surface)]);
        let status = if state.acknowledged {
            ("✓ Acknowledged", theme.success)
        } else if state.external_ready {
            ("Complete the external action, then press enter to confirm.", theme.muted)
        } else {
            ("Open or copy the URL, complete the external action, then confirm.", theme.muted)
        };
        canvas.spans(9, x, &[Span::new(status.0, status.1, theme.surface)]);
    }
    // Hint row: `⇆ tab · enter {action} · c copy · esc dismiss`.
    let separator = Span::new("  ", FILL_FG, theme.surface);
    let hints = vec![
        Span::new("⇆ ", theme.text, theme.surface),
        Span::new("tab", theme.muted, theme.surface),
        separator.clone(),
        Span::new("enter ", theme.text, theme.surface),
        Span::new(state.action_label(), theme.muted, theme.surface),
        separator.clone(),
        Span::new("c ", theme.text, theme.surface),
        Span::new("copy", theme.muted, theme.surface),
        separator,
        Span::new("esc ", theme.text, theme.surface),
        Span::new("dismiss", theme.muted, theme.surface),
    ];
    canvas.spans(panel_rows.saturating_sub(2), x, &hints);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::footer::form::{FieldKind, FormField};
    use crate::wrap::display_width;

    fn authorization_form() -> FormRequest {
        FormRequest {
            id: "frm_test".into(),
            session_id: "ses_test".into(),
            title: "Authorization required".into(),
            fields: vec![FormField {
                key: "authorization".into(),
                title: Some("Authorize access".into()),
                description: None,
                required: false,
                kind: FieldKind::External { url: "https://example.com/authorize".into() },
            }],
        }
    }

    #[test]
    fn golden_frame_shape_at_80() {
        let theme = FooterTheme::fallback_dark();
        let frame = render_form_authorization(
            &authorization_form(),
            &FormAuthorizationState::new(),
            80,
            20,
            &theme,
        );
        let grid = frame.text_grid();
        assert!(grid[1].starts_with("┃  Authorization required"));
        assert!(grid[3].contains("Authorize access"));
        assert!(grid[3].contains("Submit"));
        assert!(grid[7].contains("https://example.com/authorize"));
        assert!(
            grid[9].contains("Open or copy the URL, complete the external action, then confirm.")
        );
        assert!(grid[11].contains("⇆ tab  enter open link  c copy  esc dismiss"));
        assert_eq!(grid[13].chars().filter(|ch| *ch != ' ').count(), 0, "panel ends at row 12");
        assert_eq!(frame.cell(0, 0).ch, '┃');
        assert_eq!(frame.cell(0, 0).fg, theme.action_focused_bg);
        assert!(display_width(&grid[9]) <= 80);
    }

    #[test]
    fn action_label_walks_the_acknowledge_flow() {
        let mut state = FormAuthorizationState::new();
        assert_eq!(state.action_label(), "open link");
        assert_eq!(form_authorization_key(&state, 2, "enter"), FormAuthorizationKey::OpenLink);
        state.external_ready = true;
        assert_eq!(state.action_label(), "I finished");
        assert_eq!(form_authorization_key(&state, 2, "enter"), FormAuthorizationKey::Acknowledge);
        state.acknowledged = true;
        assert_eq!(state.action_label(), "continue");
    }

    #[test]
    fn tab_cycles_the_field_and_submit_tabs() {
        let state = FormAuthorizationState::new();
        let next = form_authorization_next_tab(&state, 2);
        assert_eq!(next.tab, 1, "field → submit");
        let wrap = form_authorization_next_tab(&next, 2);
        assert_eq!(wrap.tab, 0, "submit wraps to the field");
    }
}
