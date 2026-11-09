//! The scrollback writer — port of `packages/tui/src/mini/entry.body.ts`
//! (commit → body/flags) and the writer semantics of
//! `mini/scrollback.writer.tsx` (entry groups + separators). Mini's
//! transcript is a MONO writer: every commit folds to plain text rows
//! that become immutable terminal history above the inline footer.

#![allow(clippy::match_same_arms)] // the phase arms mirror entry.body.ts 1:1

use super::mono::mono_tool_text;
use crate::mini::tool_snapshot::ToolSnapshot;
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
    /// G-68 — `StreamCommit["image"]`: the image commit's data URI
    /// (`data:{mime};base64,{data}`, stream-v2.image.ts). `None` on
    /// every text commit (the mono writer renders the row's `text`
    /// label; the URI is the dedup/display seam).
    pub image: Option<String>,
    /// G-134 — the COMPLETED tool's structured snapshot
    /// (`toolView(name).snap`, mini/tool.ts:920-967): Write/Edit/
    /// Patch/Task render their card (title + body rows) instead of
    /// the caption row; `None` keeps the caption subset.
    pub snapshot: crate::mini::tool_snapshot::ToolSnapshot,
    /// G-221 — `StreamCommit["shell"]` (`{ command }` on direct-shell
    /// commits, stream-v2.transport.ts:339-352): the `commit.shell`
    /// render branch keys on it (`$ command` on start, the merged
    /// output on progress — `toolEntryBody`'s first branch,
    /// tool.ts:1282-1294). `None` on every other commit.
    pub shell: Option<String>,
    /// G-272 — the part's START commit (`toolCommit(…, "start", …)`):
    /// the first caption row always renders (the view gate only
    /// suppresses PROGRESS refreshes and non-final finals,
    /// `TOOL_RULES[name].view`, tool.ts:890-1062).
    pub started: bool,
    /// G-272 — the completed tool's input dict (JSON), the
    /// `structuredFallback(ctx.meta)` source (tool.ts:1249-1262):
    /// rule-less tools with no structured final render the pretty
    /// JSON instead.
    pub meta: Option<String>,
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

/// G-272 — `TOOL_RULES[name].view` (tool.ts:890-1062): the per-tool
/// render gates — `output` lets PROGRESS rows stream, `final` lets
/// the completed (non-error) final row render. Tools NOT in the
/// table fall to `structuredFallback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolViewGate {
    pub output: bool,
    pub final_row: bool,
}

/// `toolView(name).view` — `None` for tools without a rule
/// (`rule(name) === undefined`, tool.ts:1070-1080).
#[must_use]
pub fn tool_view(name: &str) -> Option<ToolViewGate> {
    let gate = |output, final_row| Some(ToolViewGate { output, final_row });
    match canonical_tool_name(name) {
        "invalid" => gate(true, false),
        "shell" => gate(true, false),
        "write" => gate(false, true),
        "edit" => gate(false, true),
        "patch" => gate(false, true),
        "batch" => gate(true, false),
        "subagent" => gate(false, true),
        "question" => gate(false, true),
        "read" => gate(false, false),
        "glob" => gate(false, false),
        "grep" => gate(false, false),
        "list" => gate(false, false),
        "lsp" => gate(false, false),
        "webfetch" => gate(false, false),
        "websearch" => gate(false, false),
        "skill" => gate(false, false),
        _ => None,
    }
}

/// G-272 — `structuredFallback` (tool.ts:1249-1262): a rule-less
/// tool's meta dict pretty-printed, capped at 4096 chars with the
/// truncation marker.
#[must_use]
pub fn structured_fallback_rows(meta: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(meta) else {
        return Vec::new();
    };
    // `Object.keys(value).length === 0 → undefined` — an empty (or
    // non-object) meta renders nothing.
    if value.as_object().is_none_or(|object| object.is_empty()) {
        return Vec::new();
    }
    let content = serde_json::to_string_pretty(&value).unwrap_or_default();
    if content.is_empty() {
        return Vec::new();
    }
    const LIMIT: usize = 4096;
    const SUFFIX: &str = "\n… [truncated]";
    if content.len() <= LIMIT {
        return content.split('\n').map(str::to_owned).collect();
    }
    // Char-safe head: never split a UTF-8 boundary (the JSON body is
    // ASCII-dominated; the guard keeps non-ASCII strings panic-free).
    let head_end = content
        .char_indices()
        .map(|(at, _)| at)
        .take_while(|at| *at <= LIMIT - SUFFIX.len())
        .last()
        .unwrap_or(0);
    content[..head_end]
        .split('\n')
        .map(str::to_owned)
        .chain([SUFFIX.to_owned()])
        .collect()
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
    let raw = commit.text.replace("\r\n", "\n").replace('\r', "\n");
    // G-221 — `toolEntryBody`'s `commit.shell` branch (tool.ts:1282-
    // 1294): the DIRECT-shell rows — `$ command` on start, the merged
    // output body on progress, the error text on the error final. The
    // branch sits ABOVE the `mini.shell_output` check in the ref (the
    // knob gates only TOOL-CALL shell rows), so direct-shell output
    // always renders.
    if let Some(command) = commit.shell.as_deref() {
        if commit.phase == Phase::Start {
            return vec![format!("$ {command}")];
        }
        if commit.phase == Phase::Progress {
            return shell_output_rows(command, &raw);
        }
        if commit.tool_state == Some("error") {
            return split_rows(&raw);
        }
        return Vec::new();
    }
    if !shell_output
        && commit.kind == CommitKind::Tool
        && commit.phase == Phase::Progress
        && commit.shell.is_none()
        && !commit.started
        && commit.tool.as_deref().is_some_and(|tool| canonical_tool_name(tool) == "shell")
    {
        // G-272 — the knob gates the STREAMED OUTPUT only
        // (`options?.shellOutput === false`, tool.ts:1301); the
        // part's first caption (the ref's "start" commit) always
        // renders.
        return Vec::new();
    }
    if commit.summary {
        return Vec::new();
    }
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
/// through `monoToolText`. G-134 — a COMPLETED structured tool renders
/// its card instead (the title + body rows, `structuredBody`,
/// tool.ts:1290-1339; the caption row stays for the caption families
/// and for failures). G-272 — `TOOL_RULES[name].view` gates the rows
/// (`toolEntryBody`, tool.ts:1301-1337): the START caption always
/// renders; PROGRESS refreshes need `view.output`; the completed
/// FINAL needs `view.final` (error finals always render); rule-less
/// tools with no structured final fall to `structuredFallback`.
fn tool_rows(commit: &Commit, raw: &str) -> Vec<String> {
    // The part's FIRST caption (`started`) is the ref's "start"
    // commit — it always renders; later rows carry the gates.
    if !commit.started {
        let gate = tool_view(commit.tool.as_deref().unwrap_or_default());
        match commit.phase {
            Phase::Start => {}
            // PROGRESS refreshes need `view.output` (tool.ts:
            // 1301-1303).
            Phase::Progress => {
                if let Some(gate) = gate {
                    if !gate.output {
                        return Vec::new();
                    }
                }
            }
            Phase::Final => {
                // Errors always render (`ctx.status === "error"`
                // arm, tool.ts:1310-1312).
                if commit.tool_state != Some("error") {
                    if let Some(gate) = gate {
                        if !gate.final_row {
                            return Vec::new();
                        }
                    }
                    // G-272 — the structured fallback (tool.ts:
                    // 1333-1335): a rule-less completed tool with NO
                    // structured snapshot and a non-empty meta dict
                    // renders the pretty JSON instead of the caption.
                    if gate.is_none()
                        && commit.tool_state == Some("completed")
                        && commit.snapshot == ToolSnapshot::None
                    {
                        if let Some(meta) = commit.meta.as_deref() {
                            let rows = structured_fallback_rows(meta);
                            if !rows.is_empty() {
                                return rows;
                            }
                        }
                    }
                }
            }
        }
    }
    if commit.phase == Phase::Final
        && commit.tool_state == Some("completed")
        && commit.snapshot != ToolSnapshot::None
    {
        let mut rows = crate::mini::tool_snapshot::snapshot_rows(&commit.snapshot);
        if rows.is_empty() {
            rows = split_rows(&mono_tool_text(raw, true));
        }
        return rows;
    }
    split_rows(&mono_tool_text(raw, true))
}

fn split_rows(raw: &str) -> Vec<String> {
    raw.split('\n').map(str::to_owned).collect()
}

/// `shellOutput` (mini/tool.ts:1264-1275): the merged output body —
/// leading/trailing newlines trimmed; a known command starts the body
/// on a fresh row (`\n` + body, i.e. a leading blank row); an empty
/// body renders nothing.
fn shell_output_rows(command: &str, raw: &str) -> Vec<String> {
    let body = raw.trim_matches('\n');
    if body.is_empty() {
        return Vec::new();
    }
    if command.is_empty() {
        return split_rows(body);
    }
    let mut rows = vec![String::new()];
    rows.extend(split_rows(body));
    rows
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

    /// G-272 — `TOOL_RULES[name].view` (tool.ts:890-1062): the
    /// start caption always renders; write-family progress refreshes
    /// and read-family final rows vanish; error finals always render.
    #[test]
    fn tool_view_gates_suppress_rows_per_rule() {
        let start = |tool: &str| Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            tool: Some(tool.to_owned()),
            text: "running".to_owned(),
            started: true,
            ..Commit::default()
        };
        let progress = |tool: &str| Commit {
            kind: CommitKind::Tool,
            phase: Phase::Progress,
            tool: Some(tool.to_owned()),
            text: "refresh".to_owned(),
            ..Commit::default()
        };
        let final_row = |tool: &str, state: Option<&'static str>| Commit {
            kind: CommitKind::Tool,
            phase: Phase::Final,
            tool: Some(tool.to_owned()),
            text: "done".to_owned(),
            tool_state: state,
            ..Commit::default()
        };
        // The first caption (the ref's "start" commit) renders.
        assert!(!entry_body(&start("read")).is_empty());
        // Progress refreshes: shell streams (output: true), write
        // does not (output: false).
        assert!(!entry_body(&progress("bash")).is_empty());
        assert!(entry_body(&progress("write")).is_empty());
        // Finals: write renders (final: true), read does not
        // (final: false), errors always render.
        assert!(!entry_body(&final_row("write", Some("completed"))).is_empty());
        assert!(entry_body(&final_row("read", Some("completed"))).is_empty());
        assert!(!entry_body(&final_row("read", Some("error"))).is_empty());
    }

    /// G-272 — `structuredFallback` (tool.ts:1249-1262): a rule-less
    /// tool's meta pretty-prints (empty objects render nothing, the
    /// 4096 cap carries the truncation marker).
    #[test]
    fn structured_fallback_pretty_prints_meta() {
        assert_eq!(structured_fallback_rows("{}"), Vec::<String>::new());
        assert_eq!(structured_fallback_rows("not json"), Vec::<String>::new());
        let rows = structured_fallback_rows(r#"{"path":"src/lib.rs"}"#);
        assert_eq!(rows, vec!["{", "  \"path\": \"src/lib.rs\"", "}"]);
        let big = format!("{{\"blob\":\"{}\"}}", "x".repeat(8192));
        let rows = structured_fallback_rows(&big);
        assert_eq!(rows.last().unwrap(), "\n… [truncated]");
        let total: usize = rows.iter().map(String::len).sum::<usize>() + 1;
        assert!(total <= 4097, "cap holds: {total}");
    }

    /// G-272 — the structured fallback rides the final tool commit:
    /// a rule-less completed tool with meta renders the JSON rows.
    #[test]
    fn ruleless_final_tools_render_the_meta_json() {
        let commit = Commit {
            kind: CommitKind::Tool,
            phase: Phase::Final,
            tool: Some("customtool".to_owned()),
            text: "customtool done".to_owned(),
            tool_state: Some("completed"),
            meta: Some(r#"{"kind":"deploy"}"#.to_owned()),
            ..Commit::default()
        };
        let rows = entry_body(&commit);
        assert_eq!(rows, vec!["{", "  \"kind\": \"deploy\"", "}"]);
        assert_eq!(tool_view("customtool"), None, "no rule for the custom tool");
    }

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

    /// G-221 — the direct-shell rows (`toolEntryBody`'s
    /// `commit.shell` branch, tool.ts:1282-1294): start renders
    /// `$ command`, progress renders the merged output (leading blank
    /// row when the command is known, trimmed edges, empty →
    /// nothing), the error final renders the error text — and the
    /// `mini.shell_output` knob NEVER gates these (the ref's knob
    /// branch sits below the `commit.shell` one), while a TOOL-CALL
    /// shell progress row still hides.
    #[test]
    fn direct_shell_rows_render_the_command_and_output() {
        let shell = |text: &str, phase: Phase, state: Option<&'static str>| Commit {
            kind: CommitKind::Tool,
            tool: Some("shell".to_owned()),
            shell: Some("echo hi".to_owned()),
            part_id: Some("shell:shl_1".to_owned()),
            text: text.to_owned(),
            phase,
            tool_state: state,
            ..Commit::default()
        };
        assert_eq!(entry_body(&shell("running shell", Phase::Start, Some("running"))), vec!["$ echo hi"]);
        assert_eq!(
            entry_body(&shell("hi\n", Phase::Progress, Some("completed"))),
            vec![String::new(), "hi".to_owned()],
            "the output starts below the command row"
        );
        assert_eq!(entry_body(&shell("\n\nhi\n\n", Phase::Progress, None)), vec![String::new(), "hi".to_owned()]);
        assert_eq!(
            entry_body(&shell("", Phase::Progress, None)),
            Vec::<String>::new(),
            "an empty body renders nothing"
        );
        assert_eq!(
            entry_body(&shell("Shell exited with code 3", Phase::Final, Some("error"))),
            vec!["Shell exited with code 3"]
        );
        // The knob: direct-shell output always renders; the
        // TOOL-CALL (bash) progress row still hides.
        assert_eq!(
            entry_body_with(&shell("hi\n", Phase::Progress, None), false),
            vec![String::new(), "hi".to_owned()]
        );
        let bash = Commit {
            kind: CommitKind::Tool,
            tool: Some("bash".to_owned()),
            phase: Phase::Progress,
            text: "raw output".to_owned(),
            ..Commit::default()
        };
        assert!(entry_body_with(&bash, false).is_empty());
        assert_eq!(entry_body_with(&bash, true), vec!["raw output"]);
    }

    // -- G-134 ---------------------------------------------------------

    #[test]
    fn completed_structured_tools_render_the_card() {
        use crate::mini::tool_snapshot::ToolSnapshot;
        let commit = Commit {
            kind: CommitKind::Tool,
            phase: Phase::Final,
            tool: Some("write".to_owned()),
            tool_state: Some("completed"),
            text: "Write · src/main.rs".to_owned(),
            snapshot: ToolSnapshot::Code {
                title: "# Wrote src/main.rs".to_owned(),
                content: "fn main() {}".to_owned(),
                file: "src/main.rs".to_owned(),
            },
            ..Commit::default()
        };
        assert_eq!(
            entry_body(&commit),
            ["# Wrote src/main.rs", "fn main() {}"],
            "the card replaces the caption row"
        );
        // Failures + caption families keep the caption row.
        let failed = Commit {
            tool_state: Some("error"),
            snapshot: ToolSnapshot::Code {
                title: "# Wrote x".to_owned(),
                content: "x".to_owned(),
                file: "x".to_owned(),
            },
            ..commit.clone()
        };
        assert_eq!(entry_body(&failed).len(), 1, "the caption row stays on failure");
        let caption_only = Commit { snapshot: ToolSnapshot::None, ..commit };
        assert_eq!(entry_body(&caption_only).len(), 1);
        // Progress rows never render the card. G-272 — the view gate
        // (`TOOL_RULES.write.output === false`) suppresses the write
        // family's PROGRESS refresh entirely (tool.ts:1301-1303); a
        // streaming family (shell) keeps its progress rows.
        let progress = Commit {
            phase: Phase::Progress,
            snapshot: ToolSnapshot::Task {
                title: "# General Subagent".to_owned(),
                rows: vec!["d".to_owned()],
            },
            ..caption_only.clone()
        };
        assert_eq!(entry_body(&progress).len(), 0, "write refreshes suppress (output: false)");
        let streaming = Commit {
            tool: Some("bash".to_owned()),
            phase: Phase::Progress,
            snapshot: ToolSnapshot::Task {
                title: "# General Subagent".to_owned(),
                rows: vec!["d".to_owned()],
            },
            ..caption_only
        };
        assert_eq!(entry_body(&streaming).len(), 1, "shell streams (output: true)");
    }
