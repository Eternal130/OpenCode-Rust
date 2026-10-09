//! The mini footer family (`packages/tui/src/mini/footer*.tsx`):
//! prompt-idle composer + statusline, permission body (once/always/
//! reject with the reject free-text stage), multi-field forms, and the
//! five picker panels (command menu, model, agent, variant, settings)
//! sharing the searchable `PanelShell` — plus the session-route external
//! authorization form (`routes/session/form.tsx`).
//!
//! ## State machine
//!
//! [`FooterMachine`] ports the v2 footer semantics:
//!
//! * The active **view** ([`FooterView`]) is either the prompt (with a
//!   **route** stack: composer | command | agent | model | variant |
//!   settings) or a **blocker** (permission ask / form ask).
//! * Blockers win over the prompt — a permission ask preempts a form
//!   (`pickBlockerView`: permission before form) and any pending panel
//!   route collapses back to the composer when a blocker arrives.
//! * The queues are fed by SSE events (`permission.asked` /
//!   `permission.replied`, `form.created` / `form.replied` /
//!   `form.cancelled`) — [`crate::store`] wires them.

pub mod canvas;
pub mod form;
pub mod form_authorization;
pub mod panel;
pub mod panels;
pub mod permission;
pub mod prompt_idle;
#[cfg(test)]
pub mod replay;
pub mod theme;

#[cfg(test)]
mod golden_tests;

pub use canvas::{Canvas, Span, FILL_BG, FILL_FG};
pub use form::{FormField, FormRequest};
pub use permission::PermissionRequest;
pub use theme::FooterTheme;

/// A prompt-route panel (`FooterPromptRoute`, the picker subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptRoute {
    #[default]
    Composer,
    Command,
    Agent,
    Model,
    Variant,
    Settings,
}

impl PromptRoute {
    /// A panel route (anything the footer opens over the composer).
    #[must_use]
    pub fn is_panel(self) -> bool {
        !matches!(self, Self::Composer)
    }
}

/// The active footer view (`FooterView`): the prompt (route stack) or
/// one of the blockers that preempts it.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum FooterView {
    #[default]
    Prompt,
    Permission {
        request: PermissionRequest,
    },
    Form {
        request: FormRequest,
    },
}

impl FooterView {
    /// The view name the footer status uses (`blockerStatus`).
    #[must_use]
    pub fn status(&self) -> &'static str {
        match self {
            Self::Permission { .. } => "awaiting permission",
            Self::Form { .. } => "awaiting form",
            Self::Prompt => "",
        }
    }
}

/// `pickBlockerView` — permission asks take precedence over forms,
/// blockers take precedence over the prompt.
#[must_use]
pub fn pick_blocker_view(
    permission: Option<&PermissionRequest>,
    form: Option<&FormRequest>,
) -> FooterView {
    if let Some(request) = permission {
        return FooterView::Permission { request: request.clone() };
    }
    if let Some(request) = form {
        return FooterView::Form { request: request.clone() };
    }
    FooterView::Prompt
}

/// The footer controller state: blocker queues, the active view and
/// the prompt route stack (`RunFooter.present` + the view-sync effect
/// in `footer.view.tsx`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FooterMachine {
    pub view: FooterView,
    pub route: PromptRoute,
    pub permissions: Vec<PermissionRequest>,
    pub forms: Vec<FormRequest>,
}

impl FooterMachine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `permission.asked` — dedup by id, then re-derive the view.
    pub fn permission_asked(&mut self, request: PermissionRequest) {
        if !self.permissions.iter().any(|item| item.id == request.id) {
            self.permissions.push(request);
        }
        self.sync_view();
    }

    /// `permission.replied` — drop the request, re-derive.
    pub fn permission_replied(&mut self, request_id: &str) {
        self.permissions.retain(|item| item.id != request_id);
        self.sync_view();
    }

    /// `form.created` — dedup by id, then re-derive.
    pub fn form_created(&mut self, request: FormRequest) {
        if !self.forms.iter().any(|item| item.id == request.id) {
            self.forms.push(request);
        }
        self.sync_view();
    }

    /// `form.replied` / `form.cancelled` — drop the form, re-derive.
    pub fn form_removed(&mut self, form_id: &str) {
        self.forms.retain(|item| item.id != form_id);
        self.sync_view();
    }

    /// Open a picker route over the composer (no-op while a blocker
    /// owns the footer).
    pub fn open_route(&mut self, route: PromptRoute) {
        if matches!(self.view, FooterView::Prompt) {
            self.route = route;
        }
    }

    /// Collapse any open panel back to the composer (`closePanel`).
    pub fn close_panel(&mut self) {
        self.route = PromptRoute::Composer;
    }

    /// The view-sync effect: blockers preempt the prompt (permission
    /// first), and a blocker arriving over an open panel closes it.
    fn sync_view(&mut self) {
        self.view = pick_blocker_view(self.permissions.first(), self.forms.first());
        if !matches!(self.view, FooterView::Prompt) && self.route.is_panel() {
            self.route = PromptRoute::Composer;
        }
    }

    /// The mode the render loop dispatches on (footer state machine
    /// surface for callers).
    #[must_use]
    pub fn mode(&self) -> FooterMode {
        match &self.view {
            FooterView::Prompt => match self.route {
                PromptRoute::Composer => FooterMode::PromptIdle,
                PromptRoute::Command => FooterMode::CommandMenu,
                PromptRoute::Agent => FooterMode::AgentSelect,
                PromptRoute::Model => FooterMode::ModelSelect,
                PromptRoute::Variant => FooterMode::Variant,
                PromptRoute::Settings => FooterMode::Settings,
            },
            FooterView::Permission { .. } => FooterMode::Permission,
            FooterView::Form { .. } => FooterMode::Form,
        }
    }
}

/// The flattened footer mode (dispatch key for the render loop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterMode {
    PromptIdle,
    Form,
    Permission,
    CommandMenu,
    ModelSelect,
    AgentSelect,
    Variant,
    Settings,
}

#[cfg(test)]
mod tests {
    use super::*;
    use form::FormField;
    use permission::PermissionRequest as Request;

    fn permission(id: &str) -> PermissionRequest {
        Request { id: id.into(), session_id: "ses".into(), ..Request::default() }
    }

    fn form(id: &str) -> FormRequest {
        FormRequest {
            id: id.into(),
            session_id: "ses".into(),
            title: "T".into(),
            fields: vec![FormField::text("k", "Label")],
        }
    }

    #[test]
    fn permission_asks_preempt_forms_and_the_prompt() {
        let mut footer = FooterMachine::new();
        footer.open_route(PromptRoute::Model);
        footer.form_created(form("f1"));
        assert!(matches!(footer.view, FooterView::Form { .. }));
        assert_eq!(footer.route, PromptRoute::Composer, "blocker closes open panels");
        footer.permission_asked(permission("p1"));
        assert!(matches!(footer.view, FooterView::Permission { .. }));
        footer.permission_replied("p1");
        assert!(matches!(footer.view, FooterView::Form { .. }), "form still queued");
        footer.form_removed("f1");
        assert_eq!(footer.view, FooterView::Prompt);
        assert_eq!(footer.mode(), FooterMode::PromptIdle);
    }

    #[test]
    fn queues_drain_in_arrival_order_with_dedup() {
        let mut footer = FooterMachine::new();
        footer.permission_asked(permission("p1"));
        footer.permission_asked(permission("p1"));
        footer.permission_asked(permission("p2"));
        assert_eq!(footer.permissions.len(), 2);
        footer.permission_replied("p1");
        assert!(matches!(
            footer.view,
            FooterView::Permission { ref request } if request.id == "p2"
        ));
    }

    #[test]
    fn blocker_status_labels_match_the_reference() {
        assert_eq!(
            FooterView::Permission { request: permission("p") }.status(),
            "awaiting permission"
        );
        assert_eq!(FooterView::Form { request: form("f") }.status(), "awaiting form");
        assert_eq!(FooterView::Prompt.status(), "");
    }

    #[test]
    fn panel_routes_do_not_open_over_blockers() {
        let mut footer = FooterMachine::new();
        footer.permission_asked(permission("p1"));
        footer.open_route(PromptRoute::Settings);
        assert_eq!(footer.route, PromptRoute::Composer);
        footer.permission_replied("p1");
        footer.open_route(PromptRoute::Settings);
        assert_eq!(footer.route, PromptRoute::Settings);
        assert_eq!(footer.mode(), FooterMode::Settings);
        footer.close_panel();
        assert_eq!(footer.route, PromptRoute::Composer);
        assert_eq!(footer.mode(), FooterMode::PromptIdle);
    }
}
