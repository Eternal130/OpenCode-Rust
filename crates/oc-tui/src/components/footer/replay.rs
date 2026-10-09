//! The minimal keypress pump + the `permission-once-always-deny`
//! replay harness: kitty key tokens drive the footer state machine
//! ([`super::FooterMachine`]) and the live permission body through the
//! same semantics the reference input layer has (left/right/tab shift
//! · enter confirm · escape reject · printable keys edit the reject
//! reason), asserting frame contents, focus and reply decisions per
//! replay step.
//!
//! This pump is footer-scoped: the app-level pump (route → dialog →
//! composer dispatch over [`crate::keymap`]) lands with the dialog /
//! composer lanes; unify there when they land.

use super::permission::{
    create_permission_body_state, permission_key, permission_type_message, PermissionBodyState,
    PermissionKey, PermissionReply, PermissionRequest,
};
use super::prompt_idle::{render_prompt_idle, PromptIdleState};
use super::theme::FooterTheme;
use super::{FooterMachine, FooterView};
use crate::render::Frame;
use std::collections::HashMap;

/// What owns key handling right now (the replay scripts' `focus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Prompt,
    Dialog,
}

/// The permission harness: the footer machine's blocker queue plus the
/// body state of the live ask, collecting every reply sent.
pub struct PermissionHarness {
    pub machine: FooterMachine,
    pub states: HashMap<String, PermissionBodyState>,
    pub replies: Vec<PermissionReply>,
    theme: FooterTheme,
    width: usize,
    height: usize,
}

impl PermissionHarness {
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            machine: FooterMachine::new(),
            states: HashMap::new(),
            replies: Vec::new(),
            theme: FooterTheme::fallback_dark(),
            width,
            height,
        }
    }

    /// A permission ask arrives (the SSE event).
    pub fn ask(&mut self, request: PermissionRequest) {
        let state = create_permission_body_state(&request);
        self.states.insert(request.id.clone(), state);
        self.machine.permission_asked(request);
    }

    /// Feed one kitty key token ("right", "enter", "escape", "c", …)
    /// to the live permission body.
    pub fn press(&mut self, key: &str) {
        let FooterView::Permission { request } = &self.machine.view else {
            return;
        };
        let request = request.clone();
        let state = self
            .states
            .get(&request.id)
            .cloned()
            .unwrap_or_else(|| create_permission_body_state(&request));
        match permission_key(&state, &request, key) {
            PermissionKey::State(next) => {
                self.states.insert(request.id.clone(), next);
            }
            PermissionKey::Reply(reply, state) => {
                self.replies.push(reply.clone());
                self.states.insert(request.id.clone(), state);
                // The reply resolves the ask server-side
                // (`permission.replied`) — the view falls back.
                self.machine.permission_replied(&reply.request_id);
            }
            PermissionKey::Text(ch) => {
                let next = permission_type_message(&state, ch);
                self.states.insert(request.id.clone(), next);
            }
            PermissionKey::Ignored => {}
        }
    }

    /// The current footer frame: the permission body while an ask is
    /// live, the prompt-idle composer otherwise.
    #[must_use]
    pub fn frame(&self) -> Frame {
        if let FooterView::Permission { request } = &self.machine.view {
            let state = self
                .states
                .get(&request.id)
                .cloned()
                .unwrap_or_else(|| create_permission_body_state(request));
            return super::permission::render_permission_body(
                request,
                &state,
                self.width,
                self.height,
                &self.theme,
            );
        }
        render_prompt_idle(&PromptIdleState::default(), self.width, self.height, &self.theme)
    }

    #[must_use]
    pub fn focus(&self) -> Focus {
        match self.machine.view {
            FooterView::Permission { .. } | FooterView::Form { .. } => Focus::Dialog,
            FooterView::Prompt => Focus::Prompt,
        }
    }

    #[must_use]
    pub fn frame_includes(&self, needle: &str) -> bool {
        self.frame().text_grid().iter().any(|line| line.contains(needle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity::tui_frame as pf;

    fn request(id: &str, save: &[&str]) -> PermissionRequest {
        PermissionRequest {
            id: id.into(),
            session_id: "ses_replay".into(),
            action: "read".into(),
            resources: vec!["src/index.ts".into()],
            save: save.iter().map(|item| (*item).to_owned()).collect(),
            ..PermissionRequest::default()
        }
    }

    /// Drive `replay/permission-once-always-deny.json` end-to-end: the
    /// once/always/reject affordances, selection movement, the reject
    /// stage's free-text reason and its confirming reply — through the
    /// footer state machine (ask → reply → re-ask → reject).
    #[test]
    fn replay_permission_once_always_deny() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/parity/goldens/tui-frames");
        let script = pf::load_replay_script(&dir, "permission-once-always-deny").expect("script");
        let mut harness = PermissionHarness::new(80, 16);
        // Step 0: the ask carries save patterns ("Always allow" listed).
        harness.ask(request("per_1", &["src/**"]));
        let mut second_ask_armed = false;
        for (index, step) in script.steps.iter().enumerate() {
            // The script's second ask arrives between steps: after the
            // once-reply resolves (step 2) the server re-asks (step 3).
            if index == 3 && !second_ask_armed {
                harness.ask(request("per_2", &["src/**"]));
                second_ask_armed = true;
            }
            for key in &step.keys {
                harness.press(key);
            }
            let expect = &step.expect;
            if let Some(includes) = &expect.frame_includes {
                for needle in includes {
                    assert!(
                        harness.frame_includes(needle),
                        "step {index}: frame must include {needle:?} — frame:\n{}",
                        harness.frame().text_grid().join("\n")
                    );
                }
            }
            if let Some(excludes) = &expect.frame_excludes {
                for needle in excludes {
                    assert!(
                        !harness.frame_includes(needle),
                        "step {index}: frame must exclude {needle:?}"
                    );
                }
            }
            if let Some(focus) = &expect.focus {
                let expected = if focus == "dialog" { Focus::Dialog } else { Focus::Prompt };
                assert_eq!(harness.focus(), expected, "step {index}: focus {focus}");
            }
        }
        // The script's final state: one "once" reply then one "reject"
        // reply carrying the typed reason.
        assert_eq!(harness.replies.len(), 2);
        assert_eq!(harness.replies[0].decision, "once");
        assert_eq!(harness.replies[1].decision, "reject");
        assert_eq!(harness.replies[1].message, "choose another way");
    }

    #[test]
    fn pump_handles_enter_minimally() {
        let mut harness = PermissionHarness::new(80, 16);
        harness.ask(request("p", &[]));
        assert!(harness.frame_includes("Allow once"));
        harness.press("enter");
        assert_eq!(harness.replies.len(), 1);
        assert_eq!(harness.focus(), Focus::Prompt);
        assert!(harness.frame_includes("ctrl+p menu"), "back on the composer");
    }
}
