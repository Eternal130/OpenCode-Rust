//! Demo mode — port of `packages/tui/src/mini/demo.ts` (the subset
//! the Rust port exercises): intercepts prompt submissions and drives
//! the same presentation commits and footer actions as the live
//! transport without a real model call. Slash commands
//! (`demo.ts:6-17`):
//!
//! * `/permission [kind]` — fire a permission-ask variant into the
//!   footer (kinds `edit, shell, read, subagent, external, doom`);
//! * `/form [kind]` — fire a canonical form variant (`question`,
//!   `external`);
//! * `/fmt <kind>` — emit a specific presentation sample
//!   (`text, reasoning, error, markdown, table, mix`).
//!
//! Replies resolve locally (`createRunDemo`'s `permission` /
//! `formReply` / `formCancel`, `demo.ts:1007-1059`): allow completes
//! the synthetic tool, reject fails it.

use super::scrollback::{Commit, CommitKind, Phase};
use crate::components::footer::permission::PermissionRequest;
use crate::components::footer::{FormField, FormRequest};

/// `PERMISSIONS` (`demo.ts:60`).
pub const PERMISSIONS: &[&str] = &["edit", "shell", "read", "subagent", "external", "doom"];

/// `FORMS` (`demo.ts:61`).
pub const FORMS: &[&str] = &["question", "external"];

/// `KINDS` (`demo.ts:37-51`) — the `/fmt` set the Rust demo emits.
pub const KINDS: &[&str] = &["text", "reasoning", "error", "markdown", "table", "mix"];

/// The intro note (`intro`, `demo.ts:903-921`) — byte-for-byte.
#[must_use]
pub fn intro_note() -> String {
    [
        "Demo slash commands enabled for interactive mode.".to_owned(),
        format!("- /permission [kind] ({})", PERMISSIONS.join(", ")),
        format!("- /form [kind] ({})", FORMS.join(", ")),
        format!("- /fmt <kind> ({})", KINDS.join(", ")),
        "Examples:".to_owned(),
        "- /permission shell".to_owned(),
        "- /form question".to_owned(),
        "- /form external".to_owned(),
        "- /fmt markdown".to_owned(),
        "- /fmt motion".to_owned(),
        "- /fmt table".to_owned(),
        "- /fmt text your custom text".to_owned(),
    ]
    .join("\n")
}

/// `parseSlashHead` (the `/<name> <args…>` split).
fn slash_head(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    match trimmed.split_once(' ') {
        Some((head, rest)) => Some((head.to_owned(), rest.trim().to_owned())),
        None => Some((trimmed.to_owned(), String::new())),
    }
}

/// What a demo prompt produced.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // one fat script variant, built per keystroke
pub enum DemoOutcome {
    /// Not a demo command — run through the live transport.
    Passthrough,
    /// Transcript rows + footer interactions to perform.
    Script {
        user_echo: Option<Commit>,
        rows: Vec<Commit>,
        permission: Option<PermissionRequest>,
        form: Option<FormRequest>,
        note: Option<Commit>,
    },
}

/// `createRunDemo().prompt` (`demo.ts:942-1005`): `/help` re-prints
/// the intro; `/permission` `/form` `/fmt` run their scripts.
#[must_use]
pub fn run_demo_prompt(text: &str, counter: usize) -> DemoOutcome {
    let Some((command, arguments)) = slash_head(text) else { return DemoOutcome::Passthrough };
    let list: Vec<&str> = arguments.split_whitespace().collect();
    let script = |user_echo: Option<Commit>, rows: Vec<Commit>, permission, form, note| {
        DemoOutcome::Script { user_echo, rows, permission, form, note }
    };
    match command.as_str() {
        "/help" => script(None, Vec::new(), None, None, Some(Commit::system(&intro_note()))),
        "/permission" => {
            let kind = list.first().copied().unwrap_or("edit").to_lowercase();
            if !PERMISSIONS.contains(&kind.as_str()) {
                return script(
                    None,
                    Vec::new(),
                    None,
                    None,
                    Some(Commit::system(&format!(
                        "Pick a permission kind: {}",
                        PERMISSIONS.join(", ")
                    ))),
                );
            }
            let request = demo_permission(&kind, counter);
            script(Some(Commit::user(text)), Vec::new(), Some(request), None, None)
        }
        "/form" => {
            let kind = list.first().copied().unwrap_or("question").to_lowercase();
            if !FORMS.contains(&kind.as_str()) {
                return script(
                    None,
                    Vec::new(),
                    None,
                    None,
                    Some(Commit::system(&format!("Pick a form kind: {}", FORMS.join(", ")))),
                );
            }
            let request = demo_form(&kind, counter);
            script(Some(Commit::user(text)), Vec::new(), None, Some(request), None)
        }
        "/fmt" => {
            let kind = list.first().copied().unwrap_or_default().to_lowercase();
            if kind.is_empty() {
                return script(
                    None,
                    Vec::new(),
                    None,
                    None,
                    Some(Commit::system(&format!("Pick a kind: {}", KINDS.join(", ")))),
                );
            }
            let body = list[1..].join(" ");
            match demo_fmt(&kind, &body, counter) {
                Some(rows) => script(Some(Commit::user(text)), rows, None, None, None),
                None => script(
                    None,
                    Vec::new(),
                    None,
                    None,
                    Some(Commit::system(&format!(
                        "Unknown kind \"{kind}\". Use: {}",
                        KINDS.join(", ")
                    ))),
                ),
            }
        }
        _ => DemoOutcome::Passthrough,
    }
}

/// `emitPermission` (`demo.ts` permission variants): the request the
/// footer renders per kind.
fn demo_permission(kind: &str, counter: usize) -> PermissionRequest {
    let (action, resources) = match kind {
        "shell" => ("bash", vec!["bash(*)".to_owned()]),
        "read" => ("read", vec!["read(*)".to_owned()]),
        "subagent" => ("task", vec!["task(*)".to_owned()]),
        "external" => ("webfetch", vec!["https://opencode.ai".to_owned()]),
        "doom" => ("rm", vec!["rm(-rf /)".to_owned()]),
        _ => ("edit", vec!["edit(*)".to_owned()]),
    };
    PermissionRequest {
        id: format!("demo_prm_{counter}"),
        session_id: "demo".to_owned(),
        action: action.to_owned(),
        resources,
        ..PermissionRequest::default()
    }
}

/// `emitForm` (`demo.ts` form variants).
fn demo_form(kind: &str, counter: usize) -> FormRequest {
    match kind {
        "external" => FormRequest {
            id: format!("demo_frm_{counter}"),
            session_id: "demo".to_owned(),
            title: "External authorization".to_owned(),
            fields: vec![FormField::text("token", "Access token")],
        },
        _ => FormRequest {
            id: format!("demo_frm_{counter}"),
            session_id: "demo".to_owned(),
            title: "Question".to_owned(),
            fields: vec![FormField::text("answer", "What should the agent do next?")],
        },
    }
}

/// `SAMPLE_MARKDOWN` rows (`demo.ts:74-99`, the table-bearing subset).
const SAMPLE_MARKDOWN: &str = "# Direct Mode Demo\n\nThis is a realistic assistant response for direct-mode formatting checks.\n\n## Status\n\n| Area | Before | After |\n| --- | --- | --- |\n| scrollback | streamed | committed |\n| footer | fixed | live |\n";

fn demo_fmt(kind: &str, body: &str, counter: usize) -> Option<Vec<Commit>> {
    let commit = |kind: CommitKind, phase: Phase, text: &str| Commit {
        kind,
        phase,
        text: text.to_owned(),
        message_id: Some(format!("demo_msg_{counter}")),
        part_id: Some(format!("demo_part_{counter}")),
        ..Commit::default()
    };
    match kind {
        "text" if !body.is_empty() => {
            Some(vec![commit(CommitKind::Assistant, Phase::Progress, body)])
        }
        "text" => Some(vec![commit(CommitKind::Assistant, Phase::Progress, "Plain demo text.")]),
        "reasoning" => {
            Some(vec![commit(CommitKind::Reasoning, Phase::Progress, "Thinking: demo reasoning")])
        }
        "error" => Some(vec![Commit::error("demo error")]),
        "markdown" => Some(vec![commit(CommitKind::Assistant, Phase::Progress, SAMPLE_MARKDOWN)]),
        "table" => Some(vec![commit(
            CommitKind::Assistant,
            Phase::Progress,
            "| A | B |\n| --- | --- |\n| 1 | 2 |",
        )]),
        "mix" => Some(vec![
            commit(CommitKind::Assistant, Phase::Progress, "Mixed scenario start"),
            Commit {
                kind: CommitKind::Tool,
                phase: Phase::Progress,
                text: "Bash · echo hi".to_owned(),
                tool: Some("bash".to_owned()),
                ..commit(CommitKind::Assistant, Phase::Progress, "")
            },
            Commit::error("demo mixed scenario error"),
        ]),
        _ => None,
    }
}

/// `createRunDemo().permission` (`demo.ts:1007-1023`): allowed asks
/// complete their tool, rejected ones fail it.
#[must_use]
pub fn demo_permission_reply(decision: &str, counter: usize) -> Option<Commit> {
    let commit = |tool_state: &'static str, text: &str| Commit {
        kind: CommitKind::Tool,
        phase: Phase::Final,
        text: text.to_owned(),
        tool: Some("demo".to_owned()),
        tool_state: Some(tool_state),
        message_id: Some(format!("demo_msg_{counter}")),
        part_id: Some(format!("demo_part_{counter}")),
        ..Commit::default()
    };
    match decision {
        "once" | "always" => Some(commit("completed", "Demo · completed")),
        "reject" => Some(commit("error", "Demo · permission rejected")),
        _ => None,
    }
}

/// `createRunDemo().formReply` (`demo.ts:1025-1050`).
#[must_use]
pub fn demo_form_reply(answer: &str, counter: usize) -> Commit {
    Commit {
        kind: CommitKind::Tool,
        phase: Phase::Final,
        text: format!("Demo · Form submitted: answer={answer}"),
        tool: Some("demo".to_owned()),
        tool_state: Some("completed"),
        message_id: Some(format!("demo_msg_{counter}")),
        part_id: Some(format!("demo_part_{counter}")),
        ..Commit::default()
    }
}

/// `createRunDemo().formCancel` (`demo.ts:1052-1059`).
#[must_use]
pub fn demo_form_cancel(counter: usize) -> Commit {
    Commit {
        kind: CommitKind::Tool,
        phase: Phase::Final,
        text: "Demo · form cancelled".to_owned(),
        tool: Some("demo".to_owned()),
        tool_state: Some("error"),
        message_id: Some(format!("demo_msg_{counter}")),
        part_id: Some(format!("demo_part_{counter}")),
        ..Commit::default()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::match_single_binding)] // outcome destructures stay 1:1 with the arms
    #![allow(clippy::needless_return)] // early returns mirror the reference script

    use super::*;

    #[test]
    fn intro_note_matches_the_reference_bytes() {
        let note = intro_note();
        assert!(note.starts_with("Demo slash commands enabled for interactive mode.\n- /permission [kind] (edit, shell, read, subagent, external, doom)"));
        assert!(note.contains("- /fmt text your custom text"));
        assert!(note.ends_with("- /fmt text your custom text"), "no trailing newline");
    }

    #[test]
    fn plain_prompts_pass_through() {
        assert_eq!(run_demo_prompt("hello", 0), DemoOutcome::Passthrough);
        assert_eq!(run_demo_prompt("  ", 0), DemoOutcome::Passthrough);
    }

    #[test]
    fn permission_command_produces_a_footer_ask() {
        let outcome = run_demo_prompt("/permission shell", 3);
        match outcome {
            DemoOutcome::Script { user_echo, permission, .. } => {
                assert!(user_echo.is_some(), "the command echoes like a user row");
                let request = permission.expect("ask");
                assert_eq!(request.id, "demo_prm_3");
                assert_eq!(request.action, "bash");
            }
            DemoOutcome::Passthrough => panic!("expected the demo script"),
        }
        // Unknown kinds note the valid set (`demo.ts:968`).
        match run_demo_prompt("/permission nope", 0) {
            DemoOutcome::Script { note: Some(note), .. } => {
                assert_eq!(
                    note.text,
                    format!("Pick a permission kind: {}", PERMISSIONS.join(", "))
                );
            }
            _ => panic!("expected the demo script"),
        }
        // Default kind is edit (`demo.ts:50`).
        match run_demo_prompt("/permission", 0) {
            DemoOutcome::Script { permission: Some(request), .. } => {
                assert_eq!(request.action, "edit");
            }
            _ => panic!("expected the demo script"),
        }
    }

    #[test]
    fn form_command_produces_a_form_ask() {
        match run_demo_prompt("/form question", 1) {
            DemoOutcome::Script { form: Some(request), .. } => {
                assert_eq!(request.id, "demo_frm_1");
                assert_eq!(request.title, "Question");
                assert_eq!(request.fields.len(), 1);
            }
            _ => panic!("expected the demo script"),
        }
        match run_demo_prompt("/form external", 1) {
            DemoOutcome::Script { form: Some(request), .. } => {
                assert_eq!(request.title, "External authorization");
            }
            _ => panic!("expected the demo script"),
        }
    }

    #[test]
    fn fmt_kinds_emit_rows_and_unknown_kinds_note() {
        match run_demo_prompt("/fmt text custom words", 2) {
            DemoOutcome::Script { rows, .. } => assert_eq!(rows[0].text, "custom words"),
            DemoOutcome::Passthrough => panic!("expected the demo script"),
        }
        match run_demo_prompt("/fmt markdown", 2) {
            DemoOutcome::Script { rows, .. } => {
                assert!(rows[0].text.starts_with("# Direct Mode Demo"));
            }
            DemoOutcome::Passthrough => panic!("expected the demo script"),
        }
        match run_demo_prompt("/fmt bogus", 2) {
            DemoOutcome::Script { note: Some(note), .. } => {
                assert_eq!(
                    note.text,
                    "Unknown kind \"bogus\". Use: text, reasoning, error, markdown, table, mix"
                );
            }
            _ => panic!("expected the demo script"),
        }
        match run_demo_prompt("/fmt", 2) {
            DemoOutcome::Script { note: Some(note), .. } => {
                assert_eq!(note.text, format!("Pick a kind: {}", KINDS.join(", ")));
            }
            _ => panic!("expected the demo script"),
        }
    }

    #[test]
    fn demo_replies_complete_or_fail_the_synthetic_tool() {
        assert_eq!(demo_permission_reply("once", 1).unwrap().tool_state, Some("completed"));
        assert_eq!(demo_permission_reply("reject", 1).unwrap().tool_state, Some("error"));
        assert_eq!(demo_permission_reply("always", 1).unwrap().text, "Demo · completed");
        assert!(demo_permission_reply("other", 1).is_none());
        assert_eq!(demo_form_reply("yes", 1).text, "Demo · Form submitted: answer=yes");
        assert_eq!(demo_form_cancel(1).text, "Demo · form cancelled");
    }
}
