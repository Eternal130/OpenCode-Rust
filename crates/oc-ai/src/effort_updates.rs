//! Effort-update markers (port of `src/effort-updates.ts`): changing the
//! top-level reasoning effort invalidates the provider prompt cache, so
//! protocols with a native per-message update keep markers frozen and lower
//! them instead; other routes strip them.

use crate::route::Route;
use crate::schema::{ContentPart, LLMRequest, LLMRequestPatch, Message, MessageRole};

/// The effort-change marker of a message, when the message is exactly one.
#[must_use]
pub fn effort_update(message: &Message) -> Option<EffortMarker> {
    if message.role != MessageRole::System || message.content.len() != 1 {
        return None;
    }
    match &message.content[0] {
        ContentPart::Effort { effort, previous } => {
            Some(EffortMarker { effort: effort.clone(), previous: previous.clone() })
        }
        _ => None,
    }
}

/// The marker payload (effort → previous).
#[derive(Debug, Clone, PartialEq)]
pub struct EffortMarker {
    pub effort: Option<String>,
    pub previous: Option<String>,
}

/// Drop every effort-update marker from the message list.
#[must_use]
pub fn strip_effort_updates(request: &LLMRequest) -> LLMRequest {
    let messages: Vec<Message> = request
        .messages
        .iter()
        .filter(|message| effort_update(message).is_none())
        .cloned()
        .collect();
    if messages.len() == request.messages.len() {
        request.clone()
    } else {
        request.update(LLMRequestPatch { messages: Some(messages), ..Default::default() })
    }
}

/// Keep markers only when the route lowers them natively (port of
/// `applyEffortUpdates`).
#[must_use]
pub fn apply_effort_updates(route: &Route, request: &LLMRequest) -> LLMRequest {
    if route.supports_effort_updates(request) {
        request.clone()
    } else {
        strip_effort_updates(request)
    }
}

/// Reconcile markers with the requested effort (port of
/// `resolveEffortUpdates`): reverted or forked history can leave the last
/// marker disagreeing with the request.
pub fn resolve_effort_updates(
    request: &LLMRequest,
    current: Option<&str>,
) -> (LLMRequest, Option<String>) {
    let updates: Vec<EffortMarker> = request.messages.iter().filter_map(effort_update).collect();
    if updates.is_empty() {
        return (request.clone(), current.map(str::to_string));
    }
    if updates.last().and_then(|marker| marker.effort.as_deref()) != current {
        return (strip_effort_updates(request), current.map(str::to_string));
    }
    (request.clone(), updates.first().and_then(|marker| marker.previous.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_update_detects_single_marker_messages() {
        let marker = Message::effort(Some("low".into()), Some("high".into()));
        assert_eq!(
            effort_update(&marker),
            Some(EffortMarker { effort: Some("low".into()), previous: Some("high".into()) })
        );
        assert_eq!(effort_update(&Message::system("plain")), None);
        assert_eq!(effort_update(&Message::user("hi")), None);
        let two = Message::new(
            MessageRole::System,
            vec![ContentPart::text("a"), ContentPart::Effort { effort: None, previous: None }],
        );
        assert_eq!(effort_update(&two), None);
    }

    #[test]
    fn strip_removes_only_marker_messages() {
        let request = crate::test_support::request_for_tests(vec![
            Message::user("hi"),
            Message::effort(Some("low".into()), Some("high".into())),
            Message::user("again"),
        ]);
        let stripped = strip_effort_updates(&request);
        assert_eq!(stripped.messages.len(), 2);
        assert_eq!(stripped.messages[0].content[0].kind(), "text");
        // no markers → identical clone
        let plain = crate::test_support::request_for_tests(vec![Message::user("hi")]);
        assert_eq!(strip_effort_updates(&plain).messages.len(), 1);
    }

    #[test]
    fn resolve_follows_the_last_marker_or_strips_on_disagreement() {
        let base = |messages: Vec<Message>| crate::test_support::request_for_tests(messages);
        let marker = Message::effort(Some("low".into()), Some("high".into()));
        // no updates: effort passes through
        let (request, effort) =
            resolve_effort_updates(&base(vec![Message::user("hi")]), Some("high"));
        assert_eq!(effort.as_deref(), Some("high"));
        assert_eq!(request.messages.len(), 1);
        // last marker agrees: previous effort comes from the first marker
        let (request, effort) =
            resolve_effort_updates(&base(vec![marker.clone(), Message::user("hi")]), Some("low"));
        assert_eq!(effort.as_deref(), Some("high"));
        assert_eq!(request.messages.len(), 2);
        // last marker disagrees: markers are stripped, requested effort kept
        let (request, effort) =
            resolve_effort_updates(&base(vec![marker, Message::user("hi")]), Some("medium"));
        assert_eq!(effort.as_deref(), Some("medium"));
        assert_eq!(request.messages.len(), 1);
    }
}
