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
pub mod solid;
pub mod state;
pub mod theme;

#[cfg(test)]
mod golden_tests;

pub use canvas::{Canvas, Span, FILL_BG, FILL_FG};
pub use form::{FormField, FormRequest};
pub use permission::PermissionRequest;
pub use state::{pick_blocker_view, FooterMachine, FooterMode, FooterView, PromptRoute};
pub use theme::FooterTheme;

// The controller state machine (PromptRoute / FooterView /
// FooterMachine / FooterMode) lives in `state` since lane R2
// step 1; the re-exports above keep the historical paths live.

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
