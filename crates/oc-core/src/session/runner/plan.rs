//! Port of `plugin/plan.ts` reminder halves (G-193.1).
//!
//! The ref plugin seeds a read-only `plan` agent (the seeding lives in
//! `engine::seed_builtin_agents`) and keeps the conversation reminded
//! of Plan mode: the `context` hook reconciles per request
//! (plan.ts:58-74) — a plan-agent request whose last reminder is not
//! the enter text gets one, a non-plan request still carrying the enter
//! reminder gets the leave text — inserted before the user's prompt.
//! The reminder texts are copied verbatim.

use oc_ai::schema::messages::{Message, MessageRole};

/// `~/.opencode/plan` — the only directory Plan mode may write
/// (plan.ts:31).
#[must_use]
pub fn plan_directory(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".opencode").join("plan")
}

/// `enter()` (plan.ts:14-21), verbatim.
#[must_use]
pub fn enter_reminder(directory: &std::path::Path) -> String {
    format!(
        "<system-reminder>
You are in Plan mode. Discuss the plan with the user directly in the conversation. Do not create or update plan files unless the user explicitly asks you to; when they do, write them only in:
{}

Do not modify any other files or ask a subagent to do so.

You remain in Plan mode until the user switches agents. If the user asks you to implement changes, do not do so. Tell them they need to switch agents.
</system-reminder>",
        directory.display()
    )
}

/// `leave` (plan.ts:23-25), verbatim.
pub const LEAVE_REMINDER: &str = "<system-reminder>
You are NO LONGER in Plan mode. The previous Plan restrictions no longer apply. Any Plan mode instructions from earlier in this conversation are no longer active.
</system-reminder>";

/// `lastReminder()` (plan.ts:115-121): the last single-text user message
/// equal to either reminder text, over the LOWERED request messages.
#[must_use]
pub fn last_reminder<'a>(messages: &'a [Message], enter: &str) -> Option<&'a str> {
    let mut found: Option<&str> = None;
    for message in messages {
        if message.role != MessageRole::User {
            continue;
        }
        if message.content.len() != 1 {
            continue;
        }
        let text = match message.content.first() {
            Some(oc_ai::schema::messages::ContentPart::Text { text, .. }) => text.as_str(),
            _ => continue,
        };
        if text == enter || text == LEAVE_REMINDER {
            found = Some(text);
        }
    }
    found
}

/// The per-request reconciliation (plan.ts:58-66): `Some(text)` when a
/// reminder must be spliced in.
#[must_use]
pub fn reminder_for(is_plan_agent: bool, last: Option<&str>, enter: &str) -> Option<String> {
    let missing = is_plan_agent && last != Some(enter);
    let stale = !is_plan_agent && last == Some(enter);
    if missing {
        Some(enter.to_string())
    } else if stale {
        Some(LEAVE_REMINDER.to_string())
    } else {
        None
    }
}

/// The context-hook splice (plan.ts:65-66) over the lowered messages:
/// before the user's prompt when the last message is a user message,
/// else at the tail.
pub fn inject(messages: &mut Vec<Message>, text: &str) {
    let at = if messages.last().is_some_and(|message| message.role == MessageRole::User) {
        messages.len() - 1
    } else {
        messages.len()
    };
    messages.insert(at, Message::user(text));
}

#[cfg(test)]
mod tests {
    use super::*;
    use oc_ai::schema::messages::ContentPart;

    fn user(text: &str) -> Message {
        Message::user(text)
    }

    #[test]
    fn enter_reminder_text_is_verbatim() {
        let reminder = enter_reminder(std::path::Path::new("/home/u/.opencode/plan"));
        assert_eq!(
            reminder,
            "<system-reminder>\nYou are in Plan mode. Discuss the plan with the user directly in the conversation. Do not create or update plan files unless the user explicitly asks you to; when they do, write them only in:\n/home/u/.opencode/plan\n\nDo not modify any other files or ask a subagent to do so.\n\nYou remain in Plan mode until the user switches agents. If the user asks you to implement changes, do not do so. Tell them they need to switch agents.\n</system-reminder>"
        );
        assert!(LEAVE_REMINDER.starts_with("<system-reminder>\nYou are NO LONGER in Plan mode."));
    }

    #[test]
    fn reconciliation_missing_and_stale() {
        let enter = enter_reminder(std::path::Path::new("/p"));
        // plan agent with no reminder → enter; already reminded → none
        assert_eq!(reminder_for(true, None, &enter).as_deref(), Some(enter.as_str()));
        assert_eq!(reminder_for(true, Some(enter.as_str()), &enter), None);
        // non-plan agent carrying the enter reminder → leave
        assert_eq!(
            reminder_for(false, Some(enter.as_str()), &enter).as_deref(),
            Some(LEAVE_REMINDER)
        );
        assert_eq!(reminder_for(false, None, &enter), None);
        assert_eq!(reminder_for(false, Some(LEAVE_REMINDER), &enter), None);
    }

    #[test]
    fn last_reminder_scans_single_text_user_messages() {
        let enter = enter_reminder(std::path::Path::new("/p"));
        let messages = vec![
            user("hello"),
            Message::assistant(vec![ContentPart::text("hi")]),
            user(&enter),
            user("after"),
        ];
        assert_eq!(last_reminder(&messages, &enter), Some(enter.as_str()));
        assert_eq!(last_reminder(&[user("hello")], &enter), None);
    }

    #[test]
    fn inject_lands_before_the_last_user_message() {
        let mut messages = vec![user("earlier"), user("latest prompt")];
        inject(&mut messages, "REMINDER");
        assert_eq!(messages.len(), 3);
        let injected = match &messages[1].content[0] {
            oc_ai::schema::messages::ContentPart::Text { text, .. } => text.as_str(),
            _ => panic!("expected text part"),
        };
        assert_eq!(injected, "REMINDER");

        let mut tail = vec![Message::assistant(vec![ContentPart::text("done")])];
        inject(&mut tail, "REMINDER");
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[1].role, MessageRole::User);
    }
}
