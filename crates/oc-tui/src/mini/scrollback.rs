//! The scrollback writer — port of `packages/tui/src/mini/entry.body.ts`
//! (commit → body/flags) and the writer semantics of
//! `mini/scrollback.writer.tsx` (entry groups + separators). Mini's
//! transcript is a MONO writer: every commit folds to plain text rows
//! that become immutable terminal history above the inline footer.

#![allow(clippy::match_same_arms)] // the phase arms mirror entry.body.ts 1:1

use super::mono::mono_tool_text;
use crate::wrap::wrap_label;

/// A transcript commit (`StreamCommit`, `mini/types.ts` — the text
/// subset the mono writer renders).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Commit {
    /// `kind`.
    pub kind: CommitKind,
    /// `text`.
    pub text: String,
    /// `phase`.
    pub phase: Phase,
    /// `messageID`.
    pub message_id: Option<String>,
    /// `partID`.
    pub part_id: Option<String>,
    /// `tool` (the tool name for tool commits).
    pub tool: Option<String>,
    /// `toolState`.
    pub tool_state: Option<&'static str>,
    /// `interrupted` (assistant/reasoning final).
    pub interrupted: bool,
    /// `compaction` (the `—— compacted ——` divider row).
    pub compaction: bool,
    /// `summary` (the turn-summary row: rendered by the caller).
    pub summary: bool,
}

/// `StreamCommit["kind"]` (the text subset).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommitKind {
    User,
    Assistant,
    Reasoning,
    Tool,
    Error,
    #[default]
    System,
}

/// `StreamCommit["phase"]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    Start,
    Progress,
    #[default]
    Final,
}

impl Commit {
    /// A user echo commit (`rememberLocal` in `runtime.ts:899-907`).
    #[must_use]
    pub fn user(text: &str) -> Self {
        Self {
            kind: CommitKind::User,
            text: text.to_owned(),
            phase: Phase::Start,
            ..Self::default()
        }
    }

    /// A system note (demo `note()` / `new session` rows).
    #[must_use]
    pub fn system(text: &str) -> Self {
        Self {
            kind: CommitKind::System,
            text: text.to_owned(),
            phase: Phase::Final,
            ..Self::default()
        }
    }

    /// An error row (`renderPromptError`, `runtime.ts:866-872`).
    #[must_use]
    pub fn error(text: &str) -> Self {
        Self {
            kind: CommitKind::Error,
            text: text.to_owned(),
            phase: Phase::Start,
            ..Self::default()
        }
    }

    fn part_key(&self) -> Option<String> {
        self.part_id
            .as_ref()
            .map(|part| format!("{}\0{part}", self.message_id.clone().unwrap_or_default()))
    }
}

/// `entryGroupKey` (`scrollback.writer.tsx:18-29`): same part ⇒ same
/// group (streaming deltas never open a new entry).
fn entry_group_key(commit: &Commit) -> Option<String> {
    let part = commit.part_key()?;
    Some(match commit.kind {
        CommitKind::Tool => format!("tool:{part}:final"),
        _ => format!("{}:{part}", kind_name(commit.kind)),
    })
}

fn kind_name(kind: CommitKind) -> &'static str {
    match kind {
        CommitKind::User => "user",
        CommitKind::Assistant => "assistant",
        CommitKind::Reasoning => "reasoning",
        CommitKind::Tool => "tool",
        CommitKind::Error => "error",
        CommitKind::System => "system",
    }
}

fn same_entry_group(left: &Commit, right: &Commit) -> bool {
    match (entry_group_key(left), entry_group_key(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

/// `entryLayout` (`scrollback.writer.tsx:41-61`): user/assistant/
/// reasoning/error/system are block rows; tools are inline unless
/// structured (the mono tool caption is a single text row ⇒ inline).
fn entry_inline(commit: &Commit) -> bool {
    commit.kind == CommitKind::Tool && commit.phase != Phase::Final
}

/// `separatorRows` (`scrollback.writer.tsx:63-77`): one blank row
/// between different groups unless both sides are inline.
#[must_use]
pub fn separator_rows(prev: Option<&Commit>, next: &Commit) -> usize {
    let Some(prev) = prev else { return 0 };
    if same_entry_group(prev, next) {
        return 0;
    }
    if entry_inline(prev) && entry_inline(next) {
        return 0;
    }
    1
}

/// `EntryFlags` (`entry.body.ts:105-159`): does the entry start on a
/// fresh line / leave a trailing blank row?
#[must_use]
pub fn entry_flags(commit: &Commit) -> (bool, bool) {
    if commit.summary {
        return (true, false);
    }
    match commit.kind {
        CommitKind::User => (true, false),
        CommitKind::Tool => {
            if commit.phase == Phase::Progress {
                (false, false)
            } else {
                (true, true)
            }
        }
        CommitKind::Assistant | CommitKind::Reasoning => {
            if commit.phase == Phase::Progress {
                (false, false)
            } else {
                (true, true)
            }
        }
        CommitKind::Error => (true, false),
        CommitKind::System => (true, true),
    }
}

/// `canonicalToolName` (`util/tool-display.ts:1-5`): the display-name
/// aliases (the `shell_output` gate keys on the canonical name).
#[must_use]
pub fn canonical_tool_name(name: &str) -> &str {
    match name {
        "bash" => "shell",
        "task" => "subagent",
        "apply_patch" => "patch",
        other => other,
    }
}

/// `entryBody` for the mono writer (`entry.body.ts:189-239` with
/// `mono: true`): the text rows the entry prints, in order (empty ⇒
/// nothing).
#[must_use]
pub fn entry_body(commit: &Commit) -> Vec<String> {
    entry_body_with(commit, true)
}

/// `entryBody(commit, { shellOutput })` (`tool.ts:1278-1301` —
/// CONFIG-T4's `mini.shell_output`): a shell-family tool's streaming
/// rows vanish when the knob hides them (the start caption and the
/// final row always render — v2 gates only the live output body).
#[must_use]
pub fn entry_body_with(commit: &Commit, shell_output: bool) -> Vec<String> {
    if !shell_output
        && commit.kind == CommitKind::Tool
        && commit.phase == Phase::Progress
        && commit.tool.as_deref().is_some_and(|tool| canonical_tool_name(tool) == "shell")
    {
        return Vec::new();
    }
    if commit.summary {
        return Vec::new();
    }
    let raw = commit.text.replace("\r\n", "\n").replace('\r', "\n");
    let rows: Vec<String> = if commit.compaction {
        vec![raw.trim().to_owned()]
    } else {
        match commit.kind {
            CommitKind::User => user_rows(&raw),
            CommitKind::Assistant => match commit.phase {
                Phase::Start => Vec::new(),
                Phase::Final if commit.interrupted => vec!["assistant interrupted".to_owned()],
                Phase::Final => Vec::new(),
                Phase::Progress => split_rows(&raw),
            },
            CommitKind::Reasoning => match commit.phase {
                Phase::Start => Vec::new(),
                Phase::Final if commit.interrupted => vec!["reasoning interrupted".to_owned()],
                Phase::Final => Vec::new(),
                Phase::Progress => reasoning_rows(&raw),
            },
            CommitKind::Tool => tool_rows(commit, &raw),
            CommitKind::Error => split_rows(&raw),
            CommitKind::System => vec![if commit.phase == Phase::Progress {
                raw.clone()
            } else {
                raw.trim().to_owned()
            }],
        }
    };
    rows.into_iter().filter(|row| !row.is_empty() || commit.kind == CommitKind::User).collect()
}

/// `userBody` (`entry.body.ts:52-60`): leading newlines kept, then the
/// `>` mono prefix on the first row.
fn user_rows(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let lead: String = raw.chars().take_while(|c| *c == '\n').collect();
    let body = &raw[lead.len()..];
    let first = format!("{lead}> {body}");
    vec![first]
}

/// `reasoningBody` mono branch (`entry.body.ts:62-77`).
fn reasoning_rows(raw: &str) -> Vec<String> {
    let clean = raw.replace("[REDACTED]", "");
    if clean.is_empty() {
        return Vec::new();
    }
    let lead: String = clean.chars().take_while(|c| *c == '\n').collect();
    let body = &clean[lead.len()..];
    let mark = "Thinking:";
    if let Some(rest) = body.strip_prefix(mark) {
        return vec![format!("{lead}{mark} {}", rest.trim_start())];
    }
    vec![clean]
}

/// Mono tool caption (`entry.body.ts:206-211`): the tool title row run
/// through `monoToolText`.
fn tool_rows(commit: &Commit, raw: &str) -> Vec<String> {
    let _ = commit;
    split_rows(&mono_tool_text(raw, true))
}

fn split_rows(raw: &str) -> Vec<String> {
    raw.split('\n').map(str::to_owned).collect()
}

/// `format_commit`: the full scrollback rows for one commit — the
/// separator (when flagged), the word-wrapped body rows at the
/// terminal width, and the trailing blank row (`trailingNewline`).
#[must_use]
pub fn format_commit(prev: Option<&Commit>, commit: &Commit, width: usize) -> Vec<String> {
    format_commit_with(prev, commit, width, true)
}

/// `format_commit` with the `mini.shell_output` knob
/// (`scrollback.surface.ts:457`'s `entryBody(commit, { shellOutput })`
/// pass-through — CONFIG-T4).
#[must_use]
pub fn format_commit_with(
    prev: Option<&Commit>,
    commit: &Commit,
    width: usize,
    shell_output: bool,
) -> Vec<String> {
    let (start_on_new_line, trailing_newline) = entry_flags(commit);
    let _ = start_on_new_line; // the mono writer always starts entries on new rows
    let mut rows = Vec::new();
    if prev.is_some() && separator_rows(prev, commit) > 0 {
        rows.push(String::new());
    }
    if commit.compaction {
        // The `— text —` divider (`scrollback.writer.tsx:128-135`).
        rows.push(format!("─── {} ───", commit.text.trim()));
        return rows;
    }
    if commit.summary {
        rows.push(commit.text.clone());
        return rows;
    }
    let body = entry_body_with(commit, shell_output);
    for row in body {
        if row.is_empty() {
            rows.push(String::new());
            continue;
        }
        rows.extend(wrap_label(&row, width.max(1)));
    }
    if trailing_newline && !rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

/// The turn-summary row (`turnSummaryWriter`,
/// `scrollback.writer.tsx:311-326`, mono: `agent - model - duration`).
#[must_use]
pub fn turn_summary(agent: &str, model: &str, duration: &str, mono: bool) -> Commit {
    let separator = if mono { " - " } else { " · " };
    Commit {
        kind: CommitKind::System,
        text: format!("{agent}{separator}{model}{separator}{duration}"),
        phase: Phase::Final,
        summary: true,
        ..Commit::default()
    }
}

/// The compaction divider (`footer.append({ compaction: true, text })`
/// — `runtime.ts:926-928` callers).
#[must_use]
pub fn compaction_note(text: &str) -> Commit {
    Commit {
        kind: CommitKind::System,
        text: text.to_owned(),
        phase: Phase::Final,
        compaction: true,
        ..Commit::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_entries_get_the_mono_prefix() {
        let rows = entry_body(&Commit::user("hello world"));
        assert_eq!(rows, vec!["> hello world"]);
        // Leading newlines preserved (`entry.body.ts:55-58`).
        let rows = entry_body(&Commit::user("\n\nlate start"));
        assert_eq!(rows, vec!["\n\n> late start"]);
        assert_eq!(
            entry_body(&Commit::user("   ")),
            Vec::<String>::new(),
            "blank user rows vanish"
        );
    }

    #[test]
    fn assistant_phases() {
        let start =
            Commit { kind: CommitKind::Assistant, phase: Phase::Start, ..Commit::default() };
        assert_eq!(entry_body(&start), Vec::<String>::new());
        let progress = Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Progress,
            text: "hello".into(),
            ..Commit::default()
        };
        assert_eq!(entry_body(&progress), vec!["hello"]);
        let interrupted = Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Final,
            interrupted: true,
            ..Commit::default()
        };
        assert_eq!(entry_body(&interrupted), vec!["assistant interrupted"]);
    }

    #[test]
    fn reasoning_mark_prefix() {
        let commit = Commit {
            kind: CommitKind::Reasoning,
            phase: Phase::Progress,
            text: "Thinking: deep thought".into(),
            ..Commit::default()
        };
        assert_eq!(entry_body(&commit), vec!["Thinking: deep thought"]);
    }

    #[test]
    fn separators_split_blocks_but_not_inline_tools() {
        let user = Commit::user("hi");
        let assistant = Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Progress,
            message_id: Some("m1".into()),
            part_id: Some("p1".into()),
            text: "answer".into(),
            ..Commit::default()
        };
        assert_eq!(separator_rows(None, &user), 0);
        assert_eq!(separator_rows(Some(&user), &assistant), 1);
        // Streaming deltas of the same part stay in one group.
        let delta = assistant.clone();
        assert_eq!(separator_rows(Some(&assistant), &delta), 0);
        // Two inline tool captions stay on the same row group.
        let tool_a = Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            tool: Some("bash".into()),
            message_id: Some("m1".into()),
            part_id: Some("t1".into()),
            text: "$ ls".into(),
            ..Commit::default()
        };
        let tool_b = Commit { part_id: Some("t2".into()), text: "$ pwd".into(), ..tool_a.clone() };
        assert_eq!(separator_rows(Some(&tool_a), &tool_b), 0);
    }

    #[test]
    fn flags_follow_the_reference() {
        assert_eq!(entry_flags(&Commit::user("x")), (true, false));
        assert_eq!(entry_flags(&Commit::error("x")), (true, false));
        let tool = Commit { kind: CommitKind::Tool, phase: Phase::Final, ..Commit::default() };
        assert_eq!(entry_flags(&tool), (true, true));
        let tool_progress =
            Commit { kind: CommitKind::Tool, phase: Phase::Progress, ..Commit::default() };
        assert_eq!(entry_flags(&tool_progress), (false, false));
        let note = Commit::system("note");
        assert_eq!(entry_flags(&note), (true, true));
    }

    #[test]
    fn format_commit_wraps_and_pads() {
        let commit = Commit {
            kind: CommitKind::Assistant,
            phase: Phase::Progress,
            text: "word ".repeat(30).trim_end().to_owned(),
            ..Commit::default()
        };
        let rows = format_commit(None, &commit, 20);
        assert!(rows.iter().all(|row| crate::wrap::display_width(row) <= 20), "{rows:?}");
        let user = Commit::user("hi");
        let rows = format_commit(Some(&user), &Commit::user("again"), 20);
        assert_eq!(rows.first().map(String::as_str), Some(""), "separator row first");
    }

    #[test]
    fn compaction_and_summary_rows() {
        let rows = format_commit(None, &compaction_note("context compacted"), 40);
        assert_eq!(rows, vec!["─── context compacted ───"]);
        let summary = turn_summary("Build", "gpt-5", "3s", true);
        assert_eq!(summary.text, "Build - gpt-5 - 3s");
        assert_eq!(format_commit(None, &summary, 40), vec!["Build - gpt-5 - 3s"]);
        let styled = turn_summary("Build", "gpt-5", "3s", false);
        assert_eq!(styled.text, "Build · gpt-5 · 3s");
    }
}
