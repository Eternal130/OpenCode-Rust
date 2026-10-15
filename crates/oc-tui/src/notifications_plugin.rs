//! The internal notifications plugin — the port of v2's
//! `packages/tui/src/feature-plugins/system/notifications.ts`
//! (`opencode.notifications`): which bus events ask for attention,
//! with which message, sound, and title.
//!
//! Trigger map (v2 parity):
//!
//! | event | message | sound | notification |
//! | --- | --- | --- | --- |
//! | `form.created` | "Input needs response" | `question` | blurred (root only) |
//! | `permission.asked` | "Permission needs input" | `permission` | blurred (root only) |
//! | `session.execution.succeeded`/`.interrupted` | "Session done" | `done` / `subagent_done` for children | blurred (root only) |
//! | `session.execution.failed` | the error message | `error` | blurred (root only) + a "Session failed" toast |
//!
//! Sounds are `when: "always"`; OS notifications `when: "blurred"`;
//! subagent sessions are sound-only (`notification: false`). Forms and
//! permissions dedup by id until replied/cancelled; an execution error
//! notifies once and suppresses the following idle `Session done`;
//! `execution.started` re-arms both.

use std::collections::{HashMap, HashSet};

use crate::attention::{
    Attention, AttentionNotifyOptions, AttentionNotifyResult, AttentionSoundName,
};
use crate::data::BusEvent;

/// The engine boundary the plugin talks to (v2's `context.attention`).
/// Implemented by the real [`Attention`] engine and by test recorders.
pub trait AttentionSink {
    fn notify(&self, request: &AttentionNotifyOptions) -> AttentionNotifyResult;
}

impl AttentionSink for Attention {
    fn notify(&self, request: &AttentionNotifyOptions) -> AttentionNotifyResult {
        Attention::notify(self, request)
    }
}

/// The slice of session info the plugin needs (v2's client session
/// cache: `session.get(id)` → `{ title, parentID }`) — plus the
/// subagent picker's fields (#44): `agent` (the entry label's
/// titlecased prefix) and `created` (the timestamped fallback title).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionInfo {
    pub title: Option<String>,
    pub parent_id: Option<String>,
    /// The session's agent id (`session.created` / `session.agent.selected`).
    pub agent: Option<String>,
    /// `time.created` (epoch ms) — the fallback title's timestamp.
    pub created: Option<u64>,
}

/// The plugin's session index — filled from the session lifecycle
/// events on the same ingest stream (`session.created` carries
/// `parentID`; `session.updated`/`renamed` refresh titles).
#[derive(Debug, Default)]
pub struct SessionIndex {
    sessions: HashMap<String, SessionInfo>,
    /// The RUNNING set (`data.session.status`: `session.execution.started`
    /// inserts; the terminal execution events remove — subagents-tab.tsx's
    /// active/inactive filter reads it).
    running: std::collections::HashSet<String>,
}

impl SessionIndex {
    /// Fold one event into the index (called before the trigger
    /// mapping so the same event's session data is visible).
    pub fn observe(&mut self, event: &BusEvent) {
        match event.type_.as_str() {
            "session.created" => {
                if let Some(id) = event.string("sessionID") {
                    let info = self.sessions.entry(id).or_default();
                    info.title = event.string("title").or_else(|| info.title.take());
                    info.parent_id = event.string("parentID").or_else(|| info.parent_id.take());
                    info.agent = event.string("agent").or_else(|| info.agent.take());
                    info.created = info.created.or(event.created);
                }
            }
            "session.updated" | "session.renamed" => {
                if let Some(id) = event.string("sessionID") {
                    let info = self.sessions.entry(id).or_default();
                    if let Some(title) = event.string("title") {
                        info.title = Some(title);
                    }
                }
            }
            "session.agent.selected" => {
                if let Some(id) = event.string("sessionID") {
                    let info = self.sessions.entry(id).or_default();
                    if let Some(agent) = event.string("agent") {
                        info.agent = Some(agent);
                    }
                }
            }
            "session.execution.started" => {
                if let Some(id) = event.string("sessionID") {
                    self.running.insert(id);
                }
            }
            "session.execution.succeeded"
            | "session.execution.failed"
            | "session.execution.interrupted" => {
                if let Some(id) = event.string("sessionID") {
                    self.running.remove(&id);
                }
            }
            _ => {}
        }
    }

    /// The registry entry for `session_id`, inserted when absent
    /// (the resync's title seed — see the store's Resync arm).
    pub fn registry_entry_mut(&mut self, session_id: &str) -> &mut SessionInfo {
        self.sessions.entry(session_id.to_owned()).or_default()
    }

    /// The plugin's `session.get` (absent sessions are root sessions
    /// with unknown titles).
    #[must_use]
    pub fn get(&self, session_id: &str) -> Option<&SessionInfo> {
        self.sessions.get(session_id)
    }

    /// `isSubagent` — a known parent.
    #[must_use]
    pub fn is_subagent(&self, session_id: &str) -> bool {
        self.get(session_id).is_some_and(|info| info.parent_id.is_some())
    }

    /// The session title (for notification titles).
    #[must_use]
    pub fn title(&self, session_id: &str) -> Option<&str> {
        self.get(session_id).and_then(|info| info.title.as_deref())
    }

    /// `data.session.status(id)` — the running set's verdict (unknown
    /// sessions idle, matching the client cache's absent entry).
    #[must_use]
    pub fn is_running(&self, session_id: &str) -> bool {
        self.running.contains(session_id)
    }

    /// The running set (the subagent card's live-status oracle).
    #[must_use]
    pub fn running_ids(&self) -> Vec<String> {
        self.running.iter().cloned().collect()
    }

    /// Every known session id (the picker's `session.list()` input).
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.sessions.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    /// The family ROOT of a session (`session-tabs.tsx`'s `root`):
    /// walks `parentID` links to the topmost known ancestor.
    #[must_use]
    pub fn root_of(&self, session_id: &str) -> String {
        let mut current = session_id.to_owned();
        let mut depth = 0;
        while let Some(parent) = self
            .get(&current)
            .and_then(|info| info.parent_id.clone())
            .filter(|parent| parent != &current)
        {
            if depth >= 8 {
                break; // cycle guard
            }
            current = parent;
            depth += 1;
        }
        current
    }
}

/// The toast the failure path shows (v2's
/// `context.ui.toast.show({ sessionID, title: "Session failed", … })`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureToast {
    pub session_id: String,
    pub message: String,
}

/// The plugin state machine (v2's module-level `errored`/`terminal`/
/// `forms`/`permissions` sets + handlers).
#[derive(Debug, Default)]
pub struct NotificationsPlugin {
    errored: HashSet<String>,
    terminal: HashSet<String>,
    forms: HashSet<String>,
    permissions: HashSet<String>,
}

impl NotificationsPlugin {
    /// Handle one bus event: dedup bookkeeping + the attention
    /// requests. Returns the played-sound flag of the (last) request —
    /// the store's bell fallback suppresses itself when a real sound
    /// played — plus a toast request on execution failures.
    pub fn handle(
        &mut self,
        event: &BusEvent,
        sessions: &SessionIndex,
        attention: &dyn AttentionSink,
    ) -> (bool, Option<FailureToast>) {
        match event.type_.as_str() {
            "form.created" => {
                let Some(form) = event.data.get("form") else { return (false, None) };
                let Some(id) = form.get("id").and_then(|v| v.as_str()) else {
                    return (false, None);
                };
                if !self.forms.insert(id.to_owned()) {
                    return (false, None);
                }
                let session_id = form.get("sessionID").and_then(|v| v.as_str()).map(str::to_owned);
                let title =
                    form.get("title").and_then(|v| v.as_str()).map(str::to_owned).or_else(|| {
                        session_id.as_deref().and_then(|id| sessions.title(id).map(str::to_owned))
                    });
                (
                    notify_attention(
                        title,
                        "Input needs response",
                        AttentionSoundName::Question,
                        session_id.as_deref(),
                        sessions,
                        attention,
                    ),
                    None,
                )
            }
            "form.replied" | "form.cancelled" => {
                if let Some(id) = event.string("id") {
                    self.forms.remove(&id);
                }
                (false, None)
            }
            "permission.asked" => {
                let Some(id) = event.string("id") else { return (false, None) };
                if !self.permissions.insert(id) {
                    return (false, None);
                }
                let session_id = event.string("sessionID");
                let title =
                    session_id.as_deref().and_then(|id| sessions.title(id).map(str::to_owned));
                (
                    notify_attention(
                        title,
                        "Permission needs input",
                        AttentionSoundName::Permission,
                        session_id.as_deref(),
                        sessions,
                        attention,
                    ),
                    None,
                )
            }
            "permission.replied" => {
                if let Some(id) = event.string("requestID") {
                    self.permissions.remove(&id);
                }
                (false, None)
            }
            "session.execution.started" => {
                if let Some(session_id) = event.string("sessionID") {
                    self.errored.remove(&session_id);
                    self.terminal.remove(&session_id);
                }
                (false, None)
            }
            "session.execution.succeeded" | "session.execution.interrupted" => {
                let Some(session_id) = event.string("sessionID") else { return (false, None) };
                let sound = if sessions.is_subagent(&session_id) {
                    AttentionSoundName::SubagentDone
                } else {
                    AttentionSoundName::Done
                };
                (self.ended(&session_id, sound, sessions, attention), None)
            }
            "session.execution.failed" => self.execution_failed(event, sessions, attention),
            _ => (false, None),
        }
    }

    /// The `session.execution.failed` arm (split out of [`Self::handle`]
    /// for size): notify-once + the failure toast + the immediate
    /// terminal transition that suppresses the follow-up idle done.
    fn execution_failed(
        &mut self,
        event: &BusEvent,
        sessions: &SessionIndex,
        attention: &dyn AttentionSink,
    ) -> (bool, Option<FailureToast>) {
        let Some(session_id) = event.string("sessionID") else {
            return (false, None);
        };
        if self.terminal.contains(&session_id) {
            return (false, None);
        }
        let message = event
            .data
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("Session failed")
            .to_owned();
        if self.errored.contains(&session_id) {
            // The failure already notified; drain the terminal
            // transition without a second one.
            return (self.ended(&session_id, AttentionSoundName::Done, sessions, attention), None);
        }
        self.errored.insert(session_id.clone());
        let sound_played = notify_attention(
            None,
            &message,
            AttentionSoundName::Error,
            Some(&session_id),
            sessions,
            attention,
        );
        let toast = FailureToast { session_id: session_id.clone(), message };
        // ended() immediately: the error notification IS this
        // execution's attention (and suppresses a follow-up idle
        // `Session done`).
        self.ended(&session_id, AttentionSoundName::Done, sessions, attention);
        (sound_played, Some(toast))
    }

    /// v2's `ended()`: one attention per execution — terminal already
    /// seen → nothing; a prior error → just clear it (the error
    /// notified); else the `Session done` request.
    fn ended(
        &mut self,
        session_id: &str,
        sound: AttentionSoundName,
        sessions: &SessionIndex,
        attention: &dyn AttentionSink,
    ) -> bool {
        if self.terminal.contains(session_id) {
            return false;
        }
        self.terminal.insert(session_id.to_owned());
        if self.errored.remove(session_id) {
            return false;
        }
        notify_attention(None, "Session done", sound, Some(session_id), sessions, attention)
    }
}

/// v2's `notify()` helper: title defaults to the session title,
/// subagents are sound-only, sounds always-on.
fn notify_attention(
    title: Option<String>,
    message: &str,
    sound: AttentionSoundName,
    session_id: Option<&str>,
    sessions: &SessionIndex,
    attention: &dyn AttentionSink,
) -> bool {
    let subagent = session_id.is_some_and(|id| sessions.is_subagent(id));
    let title = title.or_else(|| session_id.and_then(|id| sessions.title(id).map(str::to_owned)));
    let request =
        AttentionNotifyOptions::plugin_request(title, message.to_owned(), sound, subagent);
    attention.notify(&request).sound
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::{AttentionWhen, NotificationOpt, SoundOpt};
    use serde_json::json;
    use std::cell::RefCell;

    /// A request-level recorder (the engine boundary seam): records
    /// every request and reports the configured outcome.
    struct RecordingSink {
        requests: RefCell<Vec<AttentionNotifyOptions>>,
        sound_result: bool,
    }

    impl RecordingSink {
        fn new() -> Self {
            Self { requests: RefCell::new(Vec::new()), sound_result: true }
        }
    }

    impl AttentionSink for RecordingSink {
        fn notify(&self, request: &AttentionNotifyOptions) -> AttentionNotifyResult {
            self.requests.borrow_mut().push(request.clone());
            AttentionNotifyResult {
                ok: true,
                notification: true,
                sound: self.sound_result,
                skipped: None,
            }
        }
    }

    /// Harness mirroring v2's notifications.test.ts setup.
    struct Harness {
        sink: RecordingSink,
        sessions: SessionIndex,
        plugin: NotificationsPlugin,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                sink: RecordingSink::new(),
                sessions: SessionIndex::default(),
                plugin: NotificationsPlugin::default(),
            }
        }

        fn emit(&mut self, event: &BusEvent) -> Option<FailureToast> {
            self.sessions.observe(event);
            let (_, toast) = self.plugin.handle(event, &self.sessions, &self.sink);
            toast
        }

        fn requests(&self) -> Vec<AttentionNotifyOptions> {
            self.sink.requests.borrow().clone()
        }
    }

    fn event(type_: &str, data: serde_json::Value) -> BusEvent {
        BusEvent::new(type_, data)
    }

    fn form(id: &str, session_id: &str, title: Option<&str>) -> BusEvent {
        let mut form = json!({ "id": id, "sessionID": session_id, "fields": [] });
        if let Some(title) = title {
            form["title"] = json!(title);
        }
        event("form.created", json!({ "form": form }))
    }

    fn permission(id: &str, session_id: &str) -> BusEvent {
        event("permission.asked", json!({ "id": id, "sessionID": session_id, "action": "edit" }))
    }

    fn execution(type_: &str, session_id: &str) -> BusEvent {
        let mut data = json!({ "sessionID": session_id });
        if type_ == "session.execution.failed" {
            data["error"] = json!({ "type": "unknown", "message": "boom" });
        }
        event(type_, data)
    }

    /// A `session.created` payload (the session-index seed).
    fn created(session_id: &str, title: &str, parent: Option<&str>) -> BusEvent {
        let mut data = json!({ "sessionID": session_id, "title": title });
        if let Some(parent) = parent {
            data["parentID"] = json!(parent);
        }
        event("session.created", data)
    }

    /// v2 test: "notifies for form and permission requests with
    /// blurred notifications and always-on sounds".
    #[test]
    fn notifies_for_forms_and_permissions() {
        let mut harness = Harness::new();
        harness.emit(&created("session", "Demo session", None));
        harness.emit(&form("form-1", "session", Some("Confirm deployment")));
        harness.emit(&permission("permission-1", "session"));

        let requests = harness.requests();
        assert_eq!(requests.len(), 2);
        // Form: title from the form, question sound, blurred
        // notification, always-on sound.
        assert_eq!(requests[0].title.as_deref(), Some("Confirm deployment"));
        assert_eq!(requests[0].message, "Input needs response");
        assert_eq!(
            requests[0].sound,
            SoundOpt::On {
                name: Some(AttentionSoundName::Question),
                volume: None,
                when: Some(AttentionWhen::Always)
            }
        );
        assert_eq!(requests[0].notification, NotificationOpt::When(AttentionWhen::Blurred));
        // Permission: title falls back to the session title.
        assert_eq!(requests[1].title.as_deref(), Some("Demo session"));
        assert_eq!(requests[1].message, "Permission needs input");
        assert!(matches!(
            requests[1].sound,
            SoundOpt::On { name: Some(AttentionSoundName::Permission), .. }
        ));
    }

    /// v2 test: "dedupes pending forms and permissions until they are
    /// resolved".
    #[test]
    fn dedupes_until_resolved() {
        let mut harness = Harness::new();
        harness.emit(&created("session", "Demo session", None));
        harness.emit(&form("form-1", "session", Some("Input requested")));
        harness.emit(&form("form-1", "session", Some("Input requested")));
        harness.emit(&event("form.cancelled", json!({ "sessionID": "session", "id": "form-1" })));
        harness.emit(&form("form-1", "session", Some("Input requested")));

        harness.emit(&permission("permission-1", "session"));
        harness.emit(&permission("permission-1", "session"));
        harness.emit(&event(
            "permission.replied",
            json!({ "sessionID": "session", "requestID": "permission-1", "reply": "once" }),
        ));
        harness.emit(&permission("permission-1", "session"));

        assert_eq!(
            harness.requests().len(),
            4,
            "2 forms + 2 permissions across the resolve cycles"
        );
    }

    /// v2 test: "notifies for terminal lifecycle events even when
    /// attached after execution started".
    #[test]
    fn terminal_lifecycle_notifies_session_done() {
        let mut harness = Harness::new();
        harness.emit(&created("session", "Demo session", None));
        harness.emit(&execution("session.execution.succeeded", "session"));
        harness.emit(&execution("session.execution.started", "session"));
        harness.emit(&execution("session.execution.succeeded", "session"));

        let requests = harness.requests();
        assert_eq!(requests.len(), 2);
        assert!(
            requests.iter().all(|request| {
                request.message == "Session done"
                    && matches!(
                        request.sound,
                        SoundOpt::On { name: Some(AttentionSoundName::Done), .. }
                    )
            }),
            "both lifecycle ends notify, the started between them re-arms"
        );
    }

    /// v2 test: "uses sound-only notifications and `subagent_done` sound
    /// for subagent sessions".
    #[test]
    fn subagents_are_sound_only_with_subagent_done() {
        let mut harness = Harness::new();
        harness.emit(&created("session", "Demo", None));
        harness.emit(&created("subagent", "Subagent session", Some("session")));
        harness.emit(&form("form-1", "subagent", Some("Questions")));
        harness.emit(&execution("session.execution.succeeded", "subagent"));

        let requests = harness.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].notification, NotificationOpt::Off);
        assert!(matches!(
            requests[0].sound,
            SoundOpt::On { name: Some(AttentionSoundName::Question), .. }
        ));
        assert_eq!(requests[0].title.as_deref(), Some("Questions"));
        assert_eq!(requests[1].notification, NotificationOpt::Off);
        assert!(matches!(
            requests[1].sound,
            SoundOpt::On { name: Some(AttentionSoundName::SubagentDone), .. }
        ));
        assert_eq!(requests[1].message, "Session done");
    }

    /// v2 test: "notifies session errors once and suppresses the
    /// following idle done notification" (+ the failure toast).
    #[test]
    fn errors_notify_once_and_suppress_done() {
        let mut harness = Harness::new();
        harness.emit(&created("session", "Demo session", None));
        harness.emit(&execution("session.execution.started", "session"));
        let toast = harness.emit(&execution("session.execution.failed", "session"));
        harness.emit(&execution("session.execution.succeeded", "session"));

        let requests = harness.requests();
        assert_eq!(requests.len(), 1, "the done after the error is suppressed");
        assert_eq!(requests[0].message, "boom");
        assert!(matches!(
            requests[0].sound,
            SoundOpt::On { name: Some(AttentionSoundName::Error), .. }
        ));
        assert_eq!(
            toast,
            Some(FailureToast { session_id: "session".to_owned(), message: "boom".to_owned() })
        );
    }

    /// A second failure after a retry notifies again (v2's
    /// execution.started clears `errored`/`terminal`).
    #[test]
    fn retries_rearm_error_notifications() {
        let mut harness = Harness::new();
        harness.emit(&created("session", "Demo session", None));
        harness.emit(&execution("session.execution.started", "session"));
        harness.emit(&execution("session.execution.failed", "session"));
        harness.emit(&execution("session.execution.started", "session"));
        harness.emit(&execution("session.execution.failed", "session"));

        let requests = harness.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request.message == "boom"));
    }

    /// Unknown/irrelevant events never notify.
    #[test]
    fn ignores_other_events() {
        let mut harness = Harness::new();
        harness.emit(&created("session", "Demo", None));
        harness.emit(&event("message.updated", json!({ "sessionID": "session" })));
        harness.emit(&event("server.connected", json!({})));
        harness.emit(&event("session.updated", json!({ "sessionID": "session", "title": "t" })));
        assert_eq!(harness.requests().len(), 0);
    }

    /// The session index: created carries parentID; updated refreshes
    /// titles without clobbering the parent.
    #[test]
    fn session_index_tracks_titles_and_parents() {
        let mut index = SessionIndex::default();
        index.observe(&created("root", "Root", None));
        index.observe(&created("child", "Child", Some("root")));
        assert!(index.is_subagent("child"));
        assert!(!index.is_subagent("root"));
        assert!(!index.is_subagent("unknown"), "unknown sessions are roots");
        index.observe(&event(
            "session.renamed",
            json!({ "sessionID": "child", "title": "Renamed" }),
        ));
        assert_eq!(index.title("child"), Some("Renamed"));
        assert!(index.is_subagent("child"), "rename keeps the parent");
    }
}
