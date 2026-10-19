//! The message list (`routes/session/index.tsx` transcript column) —
//! non-virtualized in wave 1: every row renders, the full paint list
//! flows through the same [`Placed`] pipeline, and the **window seam**
//! for wave-2 virtualization is exposed now ([`Transcript::slice_at`]).
//!
//! ## Geometry (read off the `message-groups` golden + source)
//!
//! * Transcript container: `paddingLeft/Right = 2` (1 below 44 cols),
//!   `paddingBottom = 1`; every row `marginTop = 1` (a blank line
//!   between rows — the golden's separators).
//! * The transcript column is followed by a 1-row jump-to-latest
//!   overlay row (`session-jump-to-latest`, wave-2) and the composer
//!   column (wave 2 — `composer_bottom_chrome` in `golden_tests` pins
//!   its golden bytes as the seam fixture).
//!
//! ## Wave-2 seams designed here (not implemented)
//!
//! * **Windowed rendering**: rows measure their heights up front
//!   ([`Transcript::rows`] carries `(y, height)` per row), so
//!   [`Transcript::slice_at`] can pick the row range intersecting
//!   `[offset, offset + viewport)` in O(log n); wave 1 always paints
//!   the full range with `offset = 0`.
//! * **Append-follow / jump-to-latest** ([`FollowState`]): the
//!   reference's `stickyScroll` + `awayFromBottom` state machine —
//!   sticky-at-bottom follows appends (the vscroll-stick-bottom
//!   golden), scrolling up arms the away indicator, returning to the
//!   maximum scrollTop clears it and re-arms stickiness. The state
//!   sketch below is the contract wave 2 will drive from scroll events.
//! * **Sidebar slot**: [`render_transcript`] takes the full frame
//!   width; the session-route layout (wave 2 sidebar goldens) will
//!   hand it `width - sidebar_width` and paint the sidebar beside it.

#![allow(clippy::cast_possible_truncation)]

use ratatui::style::Style;

use crate::components::message::markdown::{render_markdown, MarkdownStyles};
use crate::components::message::model::{
    canonical_tool_name, exploration_label, reasoning_summary, reduce_rows_with_rules, Entry,
    Message, Part, Row,
};
use crate::components::message::rows::{
    completed_tool_row, reasoning_header_row, running_tool_row, user_message_row, RowRender,
};
use crate::layout::{Placed, PlacedLine};
use crate::render::Fill;
use crate::theme::ThemeStyles;

/// Thinking-mode disclosure (`ctx.thinkingMode`; default `hide` — the
/// collapsed `+ Thought` header).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThinkingMode {
    #[default]
    Hide,
    Show,
}

/// Exploration grouping toggle (`ctx.groupExploration`, default on).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GroupExploration(pub bool);

/// `session.verbosity` (config/index.tsx:160-163, default medium —
/// CONFIG-T3): `low` wraps every run of tools and thoughts in one
/// activity summary (grouping/session.ts:89-96); `high` opens
/// exploration and instruction groups by default (index.tsx:237-238).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Verbosity {
    Low,
    #[default]
    Medium,
    High,
}

/// Render options for the transcript.
#[derive(Debug, Clone)]
pub struct TranscriptOptions {
    pub thinking_mode: ThinkingMode,
    pub group_exploration: bool,
    pub verbosity: Verbosity,
    /// `session.tps` (index.tsx:1892 — `config.data.session.tps &&
    /// tokensPerSecond()`): the assistant footer's `· X.X tok/s` span.
    pub tps_enabled: bool,
    /// `session.markdown === "source"` (message-parts.tsx:184-190):
    /// assistant text renders as plain wrapped source instead of
    /// rendered markdown.
    pub markdown_source: bool,
    /// The injected clock (spinner frames; frozen ⇒ `⠋`).
    pub clock_ms: u64,
    /// Expanded group keys (disclosure state; collapsed by default).
    pub expanded_groups: Vec<String>,
    /// Collapsed thought keys (`"{mid}:{pid}"` — the minimal-mode
    /// thought disclosure, message-parts.tsx:66-86; a collapsed row
    /// renders its one-line `+ Thought: {title}` header instead of
    /// the bordered body).
    pub collapsed_thoughts: std::collections::HashSet<String>,
    /// Plugin-provided grouping rules (wave 3's open rules table);
    /// empty = the native closed set only.
    pub group_rules: crate::components::message::model::GroupRules,
    /// SURF-1: inline-expanded tool cells (`{mid}:{pid}` keys — the
    /// shell block's full output / the execute calls' input rows /
    /// the generic tool's error row).
    pub expanded_tools: std::collections::HashSet<String>,
    /// The user-message image strip (R-07): disabled by default — the
    /// goldens predate it; `render_session` enables it per config.
    pub image_strip: crate::components::message::rows::ImageStripOptions,
    /// Lane/chrome #13: the model display-name table
    /// (`providerID/modelID` → catalog `name`) for the assistant
    /// footer row — empty leaves the fallback labels.
    pub model_names: Vec<(String, String)>,
    /// Live child-session statuses (#40): `metadata.sessionID` →
    /// running — the subagent card's `isRunning` second clause
    /// (`data.session.status(id) === "running"`). Empty in fixtures.
    pub subagent_running: std::collections::HashMap<String, bool>,
    /// The user-message block's rail color — the SESSION's agent
    /// categorical hue (`local.agent.color(session.agent)`,
    /// session/index.tsx:2200; `None` → the walk's first-hue default
    /// for fixtures). Set by `render_session` from the registry.
    pub user_rail: Option<ratatui::style::Color>,
    /// SURF-1: the staged revert (`session.revert`) — messages at/after
    /// `message_id` filter out (rows.ts:63-66) and the `RevertMessage`
    /// banner renders after the last row (index.tsx:1413-1418).
    pub revert: Option<crate::store::RevertState>,
    /// SURF-1: queued-compaction inbox ids — one `◇ Compaction queued`
    /// divider each, spliced after the reduced rows (rows.ts:72-79;
    /// `queuedStart` degenerates to the tail with no pending inputs).
    pub pending_compactions: Vec<String>,
    /// SURF-1: the redo shortcut label for the banner's footer
    /// (`Keymap.useShortcut("session.redo")`, index.tsx:2153).
    pub redo_key: String,
}

impl Default for TranscriptOptions {
    fn default() -> Self {
        Self {
            thinking_mode: ThinkingMode::Hide,
            group_exploration: true,
            verbosity: Verbosity::default(),
            tps_enabled: false,
            markdown_source: false,
            clock_ms: 0,
            expanded_groups: Vec::new(),
            collapsed_thoughts: std::collections::HashSet::new(),
            group_rules: crate::components::message::model::GroupRules::empty(),
            expanded_tools: std::collections::HashSet::new(),
            image_strip: crate::components::message::rows::ImageStripOptions::default(),
            model_names: Vec::new(),
            subagent_running: std::collections::HashMap::new(),
            user_rail: None,
            revert: None,
            pending_compactions: Vec::new(),
            redo_key: "R".to_owned(),
        }
    }
}

/// One measured transcript row: its y offset, height and paint list.
#[derive(Debug, Clone)]
pub struct MeasuredRow {
    pub y: usize,
    pub height: usize,
    pub placed: Vec<Placed>,
    pub fills: Vec<Fill>,
    /// Stable row key (group id / entry id) — the disclosure + window
    /// seam's identity.
    pub key: String,
}

/// A rendered transcript: measured rows plus the total content height.
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    pub rows: Vec<MeasuredRow>,
    pub content_height: usize,
    /// The container's horizontal inset (paddingLeft) — the width the
    /// rows were rendered at is `width - inset*2`.
    pub inset: usize,
    /// The transcript's image placements at absolute (transcript)
    /// coordinates (R-07) — the graphics-protocol overlay set.
    pub images: Vec<crate::image::ImagePlacement>,
}

/// The follow/sticky state machine — MOVED to oc-tui-core's
/// [`oc_tui_core::scroll`] module in the Wave A lane (the scroll
/// driver's state); re-exported so the goldens' imports stay put.
pub use oc_tui_core::scroll::FollowState;

/// Render the session transcript (rows from [`reduce_rows`]) at
/// `width` terminal columns into measured rows.
///
/// Disclosure (beta.8 bug 4): a reasoning group whose row key is in
/// `options.expanded_groups` renders the header PLUS its thought
/// children (`<Show when={expanded()}>`, `group-view.tsx:190-193`) —
/// one extra measured unit per child, each carrying its own
/// `marginTop 1` (`ThoughtEntry`'s anchor margin).
// The disclosure loop grew past the line budget with the SURF-1 tool
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn render_transcript(
    messages: &[Message],
    options: &TranscriptOptions,
    theme: &ThemeStyles,
    width: usize,
) -> Transcript {
    let inset = if width < 44 { 1 } else { 2 };
    let content_width = width.saturating_sub(inset * 2);
    // The staged-revert boundary (rows.ts:63-66): messages at/after
    // the boundary id hide until the revert clears or commits
    // (lexicographic id compare — the ref's `message.id < boundary`).
    // `revert_user_count` = the USER messages FROM the boundary (the
    // banner's "N messages reverted" numerator, index.tsx:1415).
    let (bounded, revert_user_count) = match &options.revert {
        Some(revert) if !revert.message_id.is_empty() => (
            messages
                .iter()
                .filter(|message| message.id() < revert.message_id.as_str())
                .cloned()
                .collect::<Vec<Message>>(),
            messages
                .iter()
                .filter(|message| {
                    matches!(message, Message::User { .. })
                        && message.id() >= revert.message_id.as_str()
                })
                .count(),
        ),
        _ => (messages.to_vec(), 0),
    };
    let messages = bounded.as_slice();
    let rows: Vec<Row> = reduce_rows_with_rules(
        messages,
        &std::collections::HashSet::<String>::new(),
        &options.group_rules,
        options.group_exploration,
        options.verbosity == Verbosity::Low,
    );
    let markdown = MarkdownStyles::from_theme(theme);
    let mut measured: Vec<MeasuredRow> = Vec::new();
    let mut images: Vec<crate::image::ImagePlacement> = Vec::new();
    let mut y = 0usize;
    let mut place =
        |render: RowRender, key: String, measured: &mut Vec<MeasuredRow>, y: &mut usize| {
            // Every row carries marginTop=1 (the golden's blank separators,
            // including above the FIRST row).
            let row_y = *y + 1;
            let mut placed = render.placed;
            for item in &mut placed {
                item.y += row_y;
                item.x += inset;
            }
            let mut fills = render.fills;
            for fill in &mut fills {
                fill.y += row_y as u16;
                fill.x += inset as u16;
            }
            for mut image in render.images {
                image.y += row_y as u16;
                image.x += inset as u16;
                images.push(image);
            }
            *y = row_y + render.height;
            measured.push(MeasuredRow { y: row_y, height: render.height, placed, fills, key });
        };
    for row in &rows {
        // The disclosure expansion: an expanded reasoning group emits
        // the header unit plus one unit per thought child; an expanded
        // exploration/activity group emits the header plus one detail
        // CELL per tool child (`Children mode="tool"`,
        // group-view.tsx:186-188 — `props.entry(entry, false)`).
        if let Row::Group { kind, .. } = row {
            let expanded = group_default_expanded(kind, row, options);
            if matches!(kind, crate::components::message::model::GroupKind::Reasoning) && expanded {
                let header = render_row(row, messages, options, theme, &markdown, content_width);
                let height = header.height;
                place(header, row_key(row), &mut measured, &mut y);
                let _ = height;
                for (message_id, part_id) in row.part_refs() {
                    let Some(Message::Assistant { parts, .. }) =
                        messages.iter().find(|m| m.id() == message_id.as_str())
                    else {
                        continue;
                    };
                    let Some(Part::Reasoning { text, time }) = resolve_part(parts, &part_id) else {
                        continue;
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    // The per-thought disclosure (message-parts.tsx:
                    // 66-86): expanded renders the bordered body; a
                    // collapsed key renders the one-line `+ Thought:
                    // {title}` header (the minimal-mode
                    // ReasoningHeader) — the port's stand-in for v2's
                    // toggleable inline header, which the reducer's
                    // always-grouped reasoning never produces here.
                    let entry = if options
                        .collapsed_thoughts
                        .contains(&format!("{message_id}:{part_id}"))
                    {
                        let (title, _) = reasoning_summary(text);
                        let duration = match (time.created, time.completed) {
                            (Some(created), Some(completed)) => {
                                Some(completed.saturating_sub(created))
                            }
                            _ => Some(0),
                        };
                        reasoning_header_row(
                            title.as_deref(),
                            duration,
                            time.completed.is_some(),
                            false,
                            options.clock_ms,
                            theme,
                            content_width,
                        )
                    } else {
                        crate::components::message::rows::thought_entry_row(
                            text,
                            theme,
                            content_width,
                        )
                    };
                    place(entry, format!("thought:{message_id}:{part_id}"), &mut measured, &mut y);
                }
                continue;
            }
            if matches!(
                kind,
                crate::components::message::model::GroupKind::Exploration
                    | crate::components::message::model::GroupKind::Activity
            ) && expanded
            {
                let header = render_row(row, messages, options, theme, &markdown, content_width);
                place(header, row_key(row), &mut measured, &mut y);
                for (message_id, part_id) in row.part_refs() {
                    let Some(Message::Assistant { parts, .. }) =
                        messages.iter().find(|m| m.id() == message_id.as_str())
                    else {
                        continue;
                    };
                    let Some(Part::Tool { name, state, .. }) = resolve_part(parts, &part_id) else {
                        continue;
                    };
                    let expanded_key = format!("{message_id}:{part_id}");
                    let cell = crate::components::message::tool_detail::tool_detail_row(
                        canonical_tool_name(name),
                        state,
                        options.expanded_tools.contains(&expanded_key),
                        options.clock_ms,
                        theme,
                        content_width,
                    );
                    place(cell, format!("tool:{message_id}:{part_id}"), &mut measured, &mut y);
                }
                continue;
            }
        }
        let render = render_row(row, messages, options, theme, &markdown, content_width);
        let key = row_key(row);
        place(render, key, &mut measured, &mut y);
    }
    // The queued-compaction dividers (`CompactionQueued`,
    // index.tsx:2070-2081) — one per pending inbox id, spliced at the
    // pending boundary (the tail when no input rows are pending).
    for inbox_id in &options.pending_compactions {
        let render = crate::components::message::rows::compaction_queued_row(theme, content_width);
        place(render, format!("compaction-queued:{inbox_id}"), &mut measured, &mut y);
    }
    // The staged-revert banner (`RevertMessage`, index.tsx:1413-1418 +
    // 2084-2158): after every row, with its own marginTop 1 (the
    // `place` closure adds the row margin).
    if options.revert.is_some() {
        let files = options.revert.as_ref().map_or(&[][..], |revert| revert.files.as_slice());
        let render = crate::components::message::rows::revert_banner_row(
            revert_user_count,
            files,
            &options.redo_key,
            theme,
            content_width,
        );
        place(render, "revert:banner".to_owned(), &mut measured, &mut y);
    }
    Transcript { rows: measured, content_height: y, inset, images }
}

/// TUI-TD05 — the tail-first mount budgets (session/index.tsx:132-135):
/// a collapsed group costs one entry however many parts it holds, so
/// the measured-row count IS the rendered-entry count.
pub const TRANSCRIPT_TAIL_ROWS: usize = 40;
pub const TRANSCRIPT_BACKFILL_CHUNK: usize = 60;

/// TUI-TD05 — the session's mount-window signals
/// (`hiddenRows()` / `visibleRowsEnd()`, index.tsx:397-410): `None`
/// mirrors the reference's `undefined` (no pin; the tail budget rules
/// the top, the full list the bottom).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MountWindow {
    /// The pinned count of hidden rows at the top (`setHiddenRows`).
    pub hidden: Option<usize>,
    /// The pinned end row index (`setVisibleRowsEnd`).
    pub end: Option<usize>,
}

/// `rowsBefore` (mount-budget.ts:28-34) with flat weights: the first
/// row index such that rows `[index, end)` spend at least `budget`.
#[must_use]
pub fn rows_before(end: usize, budget: usize) -> usize {
    end.saturating_sub(budget)
}

/// `rowsAfter` (mount-budget.ts:36-42) with flat weights: the end row
/// index such that rows `[start, index)` spend at least `budget`.
#[must_use]
pub fn rows_after(start: usize, budget: usize, len: usize) -> usize {
    (start + budget).min(len)
}

/// TUI-TD05 — apply the mount window to a rendered transcript
/// (`hidden()` / `visibleEnd()` / `visibleRows`, index.tsx:407-411):
/// the rows OUTSIDE the window drop from the render set, but every
/// coordinate stays ABSOLUTE — v2's scrollbox keeps the hidden region
/// as a spacer (the mount budget saves render work, never reshapes
/// the scroll math: sticky offsets, pageup walks and reveal growth
/// all address the SAME document space, so growing the window above
/// never moves the viewport).
pub fn apply_mount_window(transcript: &mut Transcript, window: MountWindow) {
    let len = transcript.rows.len();
    if len == 0 {
        return;
    }
    let hidden = window.hidden.unwrap_or(usize::MAX).min(rows_before(len, TRANSCRIPT_TAIL_ROWS));
    let end = window.end.unwrap_or(len).min(len).max(hidden);
    if hidden == 0 && end >= len {
        return; // the full list is mounted
    }
    // Slice the render set; keep the rows' absolute y (the spacer
    // above is simply "rows that don't paint" — content_height keeps
    // measuring the FULL transcript).
    transcript.rows.drain(..hidden);
    transcript.rows.truncate(end - hidden);
}

/// Render one reduced row.
// The per-kind match grew with the SURF-1 detail arms.
#[allow(clippy::too_many_lines)]
fn render_row(
    row: &Row,
    messages: &[Message],
    options: &TranscriptOptions,
    theme: &ThemeStyles,
    markdown: &MarkdownStyles,
    width: usize,
) -> RowRender {
    match row {
        Row::Entry(Entry::Message { message_id }) => {
            let Some(message) = messages.iter().find(|m| m.id() == message_id.as_str()) else {
                return RowRender::default();
            };
            match message {
                Message::User { text, files, .. } => user_message_row(
                    text,
                    files,
                    options.user_rail.unwrap_or_else(|| theme.agent_color(0)),
                    theme,
                    width,
                    &options.image_strip,
                ),
                // The compaction divider (`CompactionMessage`,
                // index.tsx:2002-2068): running spinner / `✗` failed
                // / `· cancelled` / `Provider compaction` / usage +
                // the markdown summary body.
                Message::Compaction {
                    running,
                    failed,
                    cancelled,
                    error,
                    summary,
                    tokens_in,
                    tokens_out,
                    provider_compaction,
                    ..
                } => {
                    // `text()` (index.tsx:2008-2011): failed → the
                    // error message (cancelled → none), else the
                    // summary — the markdown body under the divider.
                    let body = if *failed && !*cancelled { error.clone() } else { summary.clone() }
                        .filter(|text| !text.trim().is_empty())
                        .map(|text| render_markdown(text.trim(), width.saturating_sub(3), markdown))
                        .unwrap_or_default();
                    crate::components::message::rows::compaction_message_row(
                        *running,
                        *failed,
                        *cancelled,
                        *provider_compaction,
                        *tokens_in,
                        *tokens_out,
                        &body,
                        options.clock_ms,
                        theme,
                        width,
                    )
                }
                // The `→ Skill {name}` row (`SessionSkillMessage`,
                // index.tsx:1993-1999).
                Message::Skill { name, .. } => {
                    crate::components::message::rows::skill_message_row(name, theme, width)
                }
                // TUI-TD01 — the synthetic message: instruction loads
                // render nothing here (the Instructions group owns
                // them); the notice forms render
                // `SessionNoticeMessageV2` (index.tsx:1934-1986) —
                // the subagent/shell completion heading, else the
                // `◈ Notice` row over the description.
                Message::Synthetic {
                    instruction_paths, description, source, state, agent, ..
                } => {
                    // An instruction load renders nothing here (the
                    // Instructions group owns it); otherwise the
                    // `SessionNoticeMessageV2` row.
                    if instruction_paths.is_empty() {
                        let text = description.as_deref().unwrap_or("");
                        match source.as_deref() {
                            Some("subagent" | "shell") => {
                                crate::components::message::rows::completion_notice_row(
                                    state.as_deref(),
                                    source.as_deref(),
                                    agent.as_deref(),
                                    text,
                                    theme,
                                    width,
                                )
                            }
                            _ => crate::components::message::rows::notice_row(text, theme, width),
                        }
                    } else {
                        RowRender::default()
                    }
                }
                // TUI-TD01 — the user `!` shell row (`ShellMessage` →
                // `ShellDisplay`, index.tsx:2166-2186).
                Message::Shell { command, status, exit, output, .. } => {
                    crate::components::message::tool_detail::shell_message_row(
                        command,
                        status,
                        *exit,
                        output.as_deref(),
                        options.clock_ms,
                        theme,
                        width,
                    )
                }
                // TUI-TD01 — the three switch rows
                // (`SessionSwitchMessageV2`, index.tsx:1904-1932).
                Message::AgentSwitched { agent, previous, .. } => {
                    let label = match previous {
                        Some(previous) if previous != agent => format!(
                            "Switched agent from {} to {}",
                            crate::components::message::rows::switch_actor(previous),
                            crate::components::message::rows::switch_actor(agent)
                        ),
                        _ => format!(
                            "Switched agent to {}",
                            crate::components::message::rows::switch_actor(agent)
                        ),
                    };
                    crate::components::message::rows::switch_message_row(&label, theme, width)
                }
                Message::ModelSwitched { provider_id, model_id, variant, previous, .. } => {
                    let model = (provider_id.clone(), model_id.clone(), variant.clone());
                    let label = crate::components::message::rows::model_switch_label(
                        &model,
                        previous,
                        &options.model_names,
                    );
                    crate::components::message::rows::switch_message_row(&label, theme, width)
                }
                Message::LocationSwitched { directory, .. } => {
                    crate::components::message::rows::location_switch_row(directory, theme, width)
                }
                // TUI-TD01 — the system notice (`◈ Notice` over the
                // description; "Instructions updated" when absent).
                Message::System { description, .. } => {
                    crate::components::message::rows::notice_row(
                        description.as_deref().unwrap_or("Instructions updated"),
                        theme,
                        width,
                    )
                }
                // Idle markers render nothing.
                Message::Idle { .. } | Message::Assistant { .. } => RowRender::default(),
            }
        }
        Row::Entry(Entry::Part { message_id, part_id }) => {
            let Some(Message::Assistant { parts, .. }) =
                messages.iter().find(|m| m.id() == message_id.as_str())
            else {
                return RowRender::default();
            };
            let Some(part) = resolve_part(parts, part_id) else {
                return RowRender::default();
            };
            match part {
                Part::Text { text, .. } => {
                    // `session.markdown === "source"` (CONFIG-T3,
                    // message-parts.tsx:184-190): the raw wrapped text
                    // instead of rendered markdown.
                    if options.markdown_source {
                        source_text_part_row(text, width)
                    } else {
                        text_part_row(text, markdown, width)
                    }
                }
                // The subagent tool renders its InlineTool card (#40 —
                // `Subagent`, session/index.tsx:3087-3120).
                Part::Tool { name, state, .. } if canonical_tool_name(name) == "subagent" => {
                    let session_running = state
                        .metadata
                        .get("sessionID")
                        .and_then(|id| options.subagent_running.get(id).copied());
                    crate::components::message::rows::subagent_tool_row(
                        state,
                        session_running,
                        options.clock_ms,
                        theme,
                        width,
                    )
                }
                // SURF-1: ungrouped tool parts render their detail cell
                // (`ToolPart`, index.tsx:2373-2446) — the shell block,
                // read/glob/grep rows, write/edit blocks, the execute
                // card, the generic row (exploration tools reach this
                // through their group header when grouped).
                Part::Tool { name, state, .. } => {
                    let expanded_key = format!("{message_id}:{part_id}");
                    crate::components::message::tool_detail::tool_detail_row(
                        canonical_tool_name(name),
                        state,
                        options.expanded_tools.contains(&expanded_key),
                        options.clock_ms,
                        theme,
                        width,
                    )
                }
                Part::Reasoning { .. } => RowRender::default(),
            }
        }
        Row::Entry(Entry::AssistantFooter { message_id }) => {
            let Some(Message::Assistant { agent, model, error, tokens, time, .. }) =
                messages.iter().find(|m| m.id() == message_id.as_str())
            else {
                return RowRender::default();
            };
            let duration = crate::components::message::model::turn_duration(message_id, messages);
            // The model's DISPLAY NAME (`ctx.models().find(...)
            // ?.name ?? providerID/modelID`, index.tsx:1858-1864).
            let model_display = options
                .model_names
                .iter()
                .find(|(key, _)| key == model)
                .map_or_else(|| model.clone(), |(_, name)| name.clone());
            // tok/s (`turnTokensPerSecond`, rows.ts:442-463): the
            // step's output+reasoning tokens over its streamed span —
            // the VALUE is per-run (b); the row + `X.X tok/s` format
            // is the (a) surface this pins. `session.tps` gates the
            // span (index.tsx:1892 `config.data.session.tps &&
            // tokensPerSecond()` — default off).
            let tps = match (tokens, time.created, time.completed) {
                (Some(tokens), Some(created), Some(created_end))
                    if created_end > created && options.tps_enabled =>
                {
                    // Display-rate rounding, not accounting: u64→f64
                    // loss is irrelevant at one decimal.
                    #[allow(clippy::cast_precision_loss)]
                    let tps = *tokens as f64 * 1000.0 / (created_end - created) as f64;
                    Some(tps)
                }
                _ => None,
            };
            // `interrupted` (index.tsx:1872): the step error IS the
            // interrupt marker — no `Error:` row, a ` · interrupted`
            // suffix instead.
            let interrupted = error.as_deref() == Some("Step interrupted");
            crate::components::message::rows::assistant_footer_row_ext(
                agent,
                &model_display,
                duration,
                tps,
                error.as_deref(),
                interrupted,
                theme,
                width,
            )
        }
        Row::Group { kind, children, completed, .. } => {
            let expanded = group_default_expanded(kind, row, options);
            group_row(kind.clone(), children, *completed, expanded, messages, options, theme, width)
        }
    }
}

/// The disclosure's default state (CONFIG-T3, v2's
/// `groupedKind`/`groupExpanded`, index.tsx:237-241): explicit user
/// expansion wins; otherwise `high` verbosity opens
/// exploration/instruction summaries, and a reasoning group under
/// thinking=show renders its children inline (group-view.tsx:110's
/// `grouped()` false → normal).
fn group_default_expanded(
    kind: &crate::components::message::model::GroupKind,
    row: &Row,
    options: &TranscriptOptions,
) -> bool {
    options.expanded_groups.iter().any(|key| key == &row_key(row))
        || match kind {
            crate::components::message::model::GroupKind::Reasoning => {
                options.thinking_mode == ThinkingMode::Show
            }
            crate::components::message::model::GroupKind::Exploration
            | crate::components::message::model::GroupKind::Instructions => {
                options.verbosity == Verbosity::High
            }
            _ => false,
        }
}

/// The assistant text part (`TextPart`): markdown inside
/// `paddingLeft 3`.
fn text_part_row(text: &str, markdown: &MarkdownStyles, width: usize) -> RowRender {
    let body = width.saturating_sub(3);
    let lines = render_markdown(text.trim(), body, markdown);
    let placed = lines
        .iter()
        .enumerate()
        .map(|(index, line)| Placed {
            x: 3,
            y: index,
            width: body,
            lines: vec![PlacedLine { runs: line.0.clone() }],
            tag: None,
        })
        .collect();
    RowRender { placed, fills: Vec::new(), height: lines.len(), images: Vec::new() }
}

/// The assistant text part under `session.markdown === "source"`: the
/// raw text, wrapped inside the same `paddingLeft 3` (no markdown
/// rendering — message-parts.tsx:184-190's plain text node).
fn source_text_part_row(text: &str, width: usize) -> RowRender {
    let body = width.saturating_sub(3);
    let lines: Vec<String> = crate::wrap::wrap_label(text.trim(), body);
    let placed: Vec<Placed> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| Placed {
            x: 3,
            y: index,
            width: body,
            lines: vec![PlacedLine::plain(line, Style::new())],
            tag: None,
        })
        .collect();
    let height = placed.len();
    RowRender { placed, fills: Vec::new(), height, images: Vec::new() }
}

/// A group row: the collapsed header (exploration/reasoning) or, when
/// expanded (disclosure), the header plus child rows.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn group_row(
    kind: crate::components::message::model::GroupKind,
    children: &[Row],
    completed: bool,
    expanded: bool,
    messages: &[Message],
    options: &TranscriptOptions,
    theme: &ThemeStyles,
    width: usize,
) -> RowRender {
    use crate::components::message::model::GroupKind;
    match kind {
        GroupKind::Exploration | GroupKind::Activity => {
            let tools: Vec<String> = children
                .iter()
                .flat_map(super::model::Row::part_refs)
                .filter_map(|(message_id, part_id)| {
                    let message = messages.iter().find(|m| m.id() == message_id.as_str())?;
                    let Message::Assistant { parts, .. } = message else { return None };
                    let Part::Tool { name, .. } = resolve_part(parts, &part_id)? else {
                        return None;
                    };
                    Some(canonical_tool_name(name).to_owned())
                })
                .collect::<Vec<_>>();
            let tool_names: Vec<&str> = tools.iter().map(String::as_str).collect();
            if tool_names.is_empty() {
                return RowRender::default();
            }
            let label = exploration_label(&tool_names, completed);
            if completed {
                completed_tool_row('→', &label, theme, width)
            } else {
                running_tool_row(&label, options.clock_ms, theme, width)
            }
        }
        GroupKind::Reasoning => {
            // The latest reasoning child with a title drives the header;
            // the duration sums every thought's completed span.
            let mut title: Option<String> = None;
            let mut total_duration = 0u64;
            for (message_id, part_id) in children.iter().flat_map(Row::part_refs) {
                let Some(message) = messages.iter().find(|m| m.id() == message_id.as_str()) else {
                    continue;
                };
                let Message::Assistant { parts, .. } = message else { continue };
                let Some(Part::Reasoning { text, time }) = resolve_part(parts, &part_id) else {
                    continue;
                };
                if text.trim().is_empty() {
                    continue;
                }
                let (part_title, _) = reasoning_summary(text);
                if let (Some(created), Some(complete)) = (time.created, time.completed) {
                    total_duration += complete.saturating_sub(created);
                }
                if part_title.is_some() {
                    title = part_title;
                }
            }
            if title.is_none() && children.is_empty() {
                return RowRender::default();
            }
            reasoning_header_row(
                title.as_deref(),
                Some(total_duration),
                completed,
                expanded,
                options.clock_ms,
                theme,
                width,
            )
        }
        GroupKind::Plugin(header) => {
            // Wave 3's open rules table: a plugin-defined group header
            // — one muted `◈` row with the rule's label and the child
            // count (completed shape; the group closes like a tool
            // run when the next row closes it).
            let label = if children.len() > 1 {
                format!("{header} ({})", children.len())
            } else {
                header.clone()
            };
            completed_tool_row('◈', &label, theme, width)
        }
        GroupKind::Instructions => {
            // `InstructionsGroup` (group-view.tsx:235): the number of
            // DISTINCT files the descendant synthetic messages loaded,
            // summarized as one `◈` row — complete, muted, no spinner
            // (`complete={true}`, `color={text.muted}`, `pending=""`).
            let mut files = std::collections::HashSet::<String>::new();
            for message_id in children.iter().flat_map(Row::message_refs) {
                if let Some(message) =
                    messages.iter().find(|message| message.id() == message_id.as_str())
                {
                    files.extend(message.instruction_paths().iter().cloned());
                }
            }
            let label = super::model::instructions_label(files.len());
            completed_tool_row('◈', &label, theme, width)
        }
    }
}

/// The `group-view` golden mount (`SessionGroupView` alone): one
/// rendered row painted at `top` over a blank frame — the reference
/// harness shape (`<box paddingTop={N}><SessionGroupView …/></box>`).
#[must_use]
pub fn group_view_frame(
    row: &Row,
    messages: &[Message],
    options: &TranscriptOptions,
    theme: &ThemeStyles,
    width: u16,
    height: u16,
    top: usize,
) -> crate::render::Frame {
    let markdown = MarkdownStyles::from_theme(theme);
    let render = render_row(row, messages, options, theme, &markdown, usize::from(width));
    let mut placed = render.placed;
    for item in &mut placed {
        item.y += top;
    }
    let mut fills = render.fills;
    for fill in &mut fills {
        fill.y += top as u16;
    }
    crate::render::paint_placed_with_fills(
        &placed,
        &fills,
        (width, height),
        crate::components::tool_rows::fill_style(theme),
    )
}

/// `resolvePart` over the parts: tool parts by their id, text/reasoning
/// by `text:N` / `reasoning:N` ordinals.
#[must_use]
pub fn resolve_part<'a>(parts: &'a [Part], part_id: &str) -> Option<&'a Part> {
    if let Some(tool) =
        parts.iter().find(|part| matches!(part, Part::Tool { id, .. } if id == part_id))
    {
        return Some(tool);
    }
    let (kind, ordinal) = part_id.split_once(':')?;
    let ordinal: usize = ordinal.parse().ok()?;
    parts
        .iter()
        .filter(|part| match part {
            Part::Text { .. } => kind == "text",
            Part::Reasoning { .. } => kind == "reasoning",
            Part::Tool { .. } => false,
        })
        .nth(ordinal)
}

fn row_key(row: &Row) -> String {
    match row {
        Row::Entry(Entry::Message { message_id }) => format!("message:{message_id}"),
        Row::Entry(Entry::Part { message_id, part_id }) => format!("part:{message_id}:{part_id}"),
        Row::Entry(Entry::AssistantFooter { message_id }) => format!("footer:{message_id}"),
        Row::Group { kind, children, .. } => {
            let first = children
                .iter()
                .flat_map(Row::part_refs)
                .next()
                .map(|(message_id, part_id)| format!("{message_id}:{part_id}"))
                .unwrap_or_default();
            format!("group:{kind:?}:{first}")
        }
    }
}

impl Transcript {
    /// The **wave-2 window seam**: the rows intersecting
    /// `[offset, offset + viewport)` — binary search over the measured
    /// y positions. Wave 1 paints `slice_at(0, content_height)`
    /// (everything); the goldens exercise the full range.
    #[must_use]
    pub fn slice_at(&self, offset: usize, viewport: usize) -> &[MeasuredRow] {
        let end = offset.saturating_add(viewport);
        let start = self.rows.partition_point(|row| row.y + row.height <= offset);
        let stop = self.rows.partition_point(|row| row.y < end);
        &self.rows[start..stop]
    }

    #[must_use]
    pub fn paint(
        &self,
        follow: FollowState,
        theme: &ThemeStyles,
        size: (u16, u16),
        window: Option<(usize, usize)>,
    ) -> crate::render::Frame {
        let frame_height = usize::from(size.1);
        let viewport = frame_height.saturating_sub(1);
        let (offset, viewport) = window.unwrap_or_else(|| {
            let offset = match follow {
                FollowState::Sticky => self.content_height.saturating_sub(viewport),
                FollowState::Free { offset, .. } => offset.min(self.content_height),
            };
            (offset, viewport)
        });
        let mut placed: Vec<Placed> = Vec::new();
        let mut fills: Vec<Fill> = Vec::new();
        for row in self.slice_at(offset, viewport) {
            for item in &row.placed {
                if let Some(shifted) = shift_placed(item, offset) {
                    placed.push(shifted);
                }
            }
            fills.extend(row.fills.iter().filter_map(|fill| shift_fill(fill, offset)));
        }
        // The jump-to-latest indicator (right-aligned in the slot row
        // under the viewport, `inset` from the frame's right edge —
        // pinned by the vscroll-top/middle goldens at cols 62..77 of
        // 80).
        if follow.jump_to_latest_visible() {
            let indicator = "Jump to latest ↓";
            let width = usize::from(size.0);
            let x = width.saturating_sub(self.inset + crate::wrap::display_width(indicator));
            placed.push(Placed {
                x,
                y: frame_height.saturating_sub(1),
                width: crate::wrap::display_width(indicator),
                // auditB #8: `text.feedback.info.base` (the jump row's
                // ref fg — index.tsx's `text.feedback.info` class, NOT
                // text.muted).
                lines: vec![PlacedLine::plain(
                    indicator,
                    Style::new().fg(theme.info).bg(theme.background),
                )],
                tag: Some("session-jump-to-latest".into()),
            });
        }
        crate::render::paint_placed_with_fills(
            &placed,
            &fills,
            size,
            crate::components::tool_rows::fill_style(theme),
        )
    }
}

/// Shift one placed run up by `offset` rows, dropping lines that scroll
/// out the top (a window may cut a row mid-way).
fn shift_placed(item: &Placed, offset: usize) -> Option<Placed> {
    if item.y >= offset {
        return Some(Placed { y: item.y - offset, ..item.clone() });
    }
    let cut = offset - item.y;
    if cut >= item.lines.len() {
        return None;
    }
    Some(Placed {
        x: item.x,
        y: 0,
        width: item.width,
        lines: item.lines[cut..].to_vec(),
        tag: item.tag.clone(),
    })
}

fn shift_fill(fill: &Fill, offset: usize) -> Option<Fill> {
    if usize::from(fill.y) >= offset {
        return Some(Fill { y: fill.y - offset as u16, ..*fill });
    }
    let cut = offset - usize::from(fill.y);
    let height = usize::from(fill.height).saturating_sub(cut);
    if height == 0 {
        return None;
    }
    Some(Fill { y: 0, height: height as u16, ..*fill })
}

/// The `message-groups` fixture messages (`session_app.tsx`
/// `GROUP_MESSAGES`) — byte-for-byte.
#[must_use]
pub fn message_groups_fixture() -> Vec<Message> {
    use crate::components::message::model::{PartTime, ToolState};
    vec![
        Message::User {
            id: "grp-user-0".into(),
            text: "What does the parser do?".into(),
            created: 0,
            files: Vec::new(),
        },
        Message::Assistant {
            id: "grp-answer-0".into(),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            parts: vec![
                Part::Tool {
                    id: "grp-tool-read".into(),
                    name: "read".into(),
                    time: PartTime { created: Some(1), completed: Some(1) },
                    state: ToolState::default(),
                },
                Part::Text {
                    text: "It reads the configuration and produces a build plan.".into(),
                    time: PartTime::default(),
                },
            ],
            finish: Some("stop".into()),
            error: None,
            time: PartTime { created: Some(1), completed: Some(2) },
        tokens: None,
        usage_total: None,},
        Message::User {
            id: "grp-user-1".into(),
            text: "Now summarize the failure modes".into(),
            created: 10,
            files: Vec::new(),
        },
        Message::Assistant {
            id: "grp-answer-1".into(),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            parts: vec![
                Part::Reasoning {
                    text: "**Failure modes**\n\nConsider recursion limits first".into(),
                    time: PartTime { created: Some(11), completed: Some(12) },
                },
                Part::Text {
                    text: "Three failure modes dominate: recursion limits, cycle detection and budget exhaustion.".into(),
                    time: PartTime::default(),
                },
            ],
            finish: Some("stop".into()),
            error: None,
            time: PartTime { created: Some(11), completed: Some(14) },
        tokens: None,
        usage_total: None,},
    ]
}

/// The `vscroll-*` fixture messages (`session_app.tsx`
/// `VSCROLL_MESSAGES`) — ten short turns, ~8 rows each, so the
/// transcript (80 rows) overflows a 16-row viewport 5×.
#[must_use]
pub fn vscroll_messages_fixture() -> Vec<Message> {
    use crate::components::message::model::PartTime;
    let mut messages = Vec::new();
    for index in 0..10u64 {
        messages.push(Message::User {
            id: format!("vs-user-{index}"),
            text: format!("Turn {index} asks about section {index} of the document"),
            created: index * 10,
            files: Vec::new(),
        });
        messages.push(Message::Assistant {
            id: format!("vs-answer-{index}"),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            parts: vec![Part::Text {
                text: format!(
                    "Answer {index}: the section describes step {index} of the pipeline in detail."
                ),
                time: PartTime::default(),
            }],
            finish: Some("stop".into()),
            error: None,
            time: PartTime { created: Some(index * 10 + 1), completed: Some(index * 10 + 2) },
            tokens: None,
            usage_total: None,
        });
    }
    messages
}

/// The appended assistant turn the `vscroll-stick-bottom` golden
/// streams in while the viewport stays pinned to the bottom
/// (`evt_vs_*` in `session_app.tsx`): step started at 101, text ended
/// at 102, step ended (stop) at 103 — so the footer duration reads
/// 103 − 90 (the turn-9 user message) = 13ms.
#[must_use]
pub fn vscroll_appended_turn() -> Message {
    use crate::components::message::model::PartTime;
    Message::Assistant {
        id: "vs-answer-appended".into(),
        agent: "build".into(),
        model: "fixture/fixture".into(),
        parts: vec![Part::Text {
            text: "Appended while pinned to the bottom.".into(),
            time: PartTime::default(),
        }],
        finish: Some("stop".into()),
        error: None,
        time: PartTime { created: Some(101), completed: Some(103) },
        tokens: None,
        usage_total: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    /// The transcript's geometry over the golden fixture at 80 cols:
    /// 21 content rows (marginTop-1 separators, including above the
    /// first row), exactly filling the golden's transcript area.
    #[test]
    fn golden_fixture_transcript_geometry_at_80() {
        let theme = theme();
        let messages = message_groups_fixture();
        let transcript = render_transcript(&messages, &TranscriptOptions::default(), &theme, 80);
        let heights: Vec<(usize, usize)> =
            transcript.rows.iter().map(|row| (row.y, row.height)).collect();
        assert_eq!(
            heights,
            vec![
                (1, 3),  // user message (marginTop 1 + padding/text/padding)
                (5, 1),  // → Explored: 1 read
                (7, 1),  // text part
                (9, 1),  // Build · fixture/fixture · 2ms
                (11, 3), // user message 2
                (15, 1), // + Thought: Failure modes · 1ms
                (17, 2), // text part (wrapped)
                (20, 1), // Build · fixture/fixture · 4ms
            ]
        );
        assert_eq!(transcript.content_height, 21);
    }

    /// The rows render the golden's text (spot-check the exact lines).
    #[test]
    fn golden_fixture_text_lines() {
        let theme = theme();
        let messages = message_groups_fixture();
        let transcript = render_transcript(&messages, &TranscriptOptions::default(), &theme, 80);
        let frame = transcript.paint(FollowState::Sticky, &theme, (80, 30), None);
        let grid = frame.text_grid();
        let padded = |text: &str| format!("{text}{}", " ".repeat(80 - text.chars().count()));
        assert_eq!(grid[2], padded("  ┃  What does the parser do?"));
        assert_eq!(grid[5], padded("     → Explored: 1 read"));
        assert_eq!(grid[7], padded("     It reads the configuration and produces a build plan."));
        assert_eq!(grid[9], padded("     Build · fixture/fixture · 2ms"));
        assert_eq!(grid[15], padded("     + Thought: Failure modes · 1ms"));
        assert_eq!(grid[20], padded("     Build · fixture/fixture · 4ms"));
    }

    /// The window seam: `slice_at` picks exactly the intersecting rows
    /// and the full range reproduces the unpainted transcript.
    #[test]
    fn slice_at_windows_the_measured_rows() {
        let theme = theme();
        let messages = message_groups_fixture();
        let transcript = render_transcript(&messages, &TranscriptOptions::default(), &theme, 80);
        // Full range = every row.
        assert_eq!(transcript.slice_at(0, transcript.content_height).len(), 8);
        // A viewport over rows [5, 10): header, text part and footer.
        let slice = transcript.slice_at(5, 5);
        let keys: Vec<&str> = slice.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "group:Exploration:grp-answer-0:grp-tool-read",
                "part:grp-answer-0:text:0",
                "footer:grp-answer-0"
            ]
        );
        // A viewport falling in the blank gap between rows intersects
        // nothing; extending it one row picks the next row.
        assert!(transcript.slice_at(6, 1).is_empty());
        let slice = transcript.slice_at(6, 2);
        assert_eq!(slice.len(), 1);
        assert_eq!(slice[0].y, 7);
        // Past the end: empty.
        assert!(transcript.slice_at(100, 5).is_empty());
    }

    #[test]
    fn follow_state_machine_sketch() {
        // Sticky follows appends; scrolling away arms the indicator.
        let state = FollowState::Sticky.appended(120);
        assert_eq!(state, FollowState::Sticky);
        assert!(!state.jump_to_latest_visible());
        let away = state.scrolled_to(40, 120);
        assert!(away.jump_to_latest_visible());
        let stayed = away.appended(140);
        assert!(stayed.jump_to_latest_visible(), "free does not follow appends");
        let back = stayed.scrolled_to(140, 140);
        assert_eq!(back, FollowState::Sticky);
        assert!(!back.jump_to_latest_visible());
    }

    /// The jump indicator paints in its slot when armed (wave-2 golden
    /// bytes; geometry pinned here — one row under the content).
    #[test]
    fn jump_indicator_renders_when_away_from_bottom() {
        let theme = theme();
        let messages = message_groups_fixture();
        let transcript = render_transcript(&messages, &TranscriptOptions::default(), &theme, 80);
        let frame =
            transcript.paint(FollowState::Free { offset: 0, maximum: 40 }, &theme, (80, 23), None);
        let line = &frame.text_grid()[22];
        assert!(line.trim_end().ends_with("Jump to latest ↓"), "{line}");
    }

    #[test]
    fn resolve_part_ords_and_tool_ids() {
        use crate::components::message::model::{PartTime, ToolState};
        let parts = vec![
            Part::Text { text: "a".into(), time: PartTime::default() },
            Part::Reasoning { text: "r".into(), time: PartTime::default() },
            Part::Text { text: "b".into(), time: PartTime::default() },
            Part::Tool {
                id: "t9".into(),
                name: "read".into(),
                time: PartTime::default(),
                state: ToolState::default(),
            },
        ];
        assert!(matches!(resolve_part(&parts, "text:0"), Some(Part::Text { .. })));
        assert!(
            matches!(resolve_part(&parts, "text:1"), Some(Part::Text { text, .. }) if text == "b")
        );
        assert!(matches!(resolve_part(&parts, "reasoning:0"), Some(Part::Reasoning { .. })));
        assert!(matches!(resolve_part(&parts, "t9"), Some(Part::Tool { .. })));
        assert!(resolve_part(&parts, "text:2").is_none());
    }
}

#[cfg(test)]
mod surf1_tests {
    use super::*;
    use crate::components::message::model::PartTime;
    use crate::theme::ThemeState;

    fn theme() -> ThemeStyles {
        ThemeState::v2_dark().styles()
    }

    fn frame_of(transcript: &Transcript) -> crate::render::Frame {
        transcript.paint(FollowState::Sticky, &theme(), (80, 30), None)
    }

    #[test]
    fn compaction_and_skill_rows_render_through_the_transcript() {
        let messages = vec![
            Message::User { id: "u1".into(), text: "hello".into(), created: 1, files: Vec::new() },
            Message::Compaction {
                id: "c1".into(),
                created: 2,
                running: false,
                failed: false,
                cancelled: false,
                error: None,
                summary: Some("The early turns.".into()),
                tokens_in: Some(1_200),
                tokens_out: Some(200),
                provider_compaction: false,
            },
            Message::Skill { id: "sk1".into(), name: "commitlint".into(), created: 3 },
        ];
        let options = TranscriptOptions {
            pending_compactions: vec!["inbox_cq".into()],
            ..TranscriptOptions::default()
        };
        let transcript = render_transcript(&messages, &options, &theme(), 80);
        let text = frame_of(&transcript).text_grid().join("\n");
        assert!(text.contains("Compaction · 1,200 in · 200 out"), "{text}");
        assert!(text.contains("The early turns."), "the summary body: {text}");
        assert!(text.contains("◇ Compaction queued"), "{text}");
        assert!(text.contains("→ Skill commitlint"), "{text}");
    }

    #[test]
    fn revert_boundary_filters_rows_and_paints_the_banner() {
        let messages = vec![
            Message::User {
                id: "msg_a".into(),
                text: "kept".into(),
                created: 1,
                files: Vec::new(),
            },
            Message::User {
                id: "msg_z".into(),
                text: "hidden".into(),
                created: 2,
                files: Vec::new(),
            },
        ];
        let options = TranscriptOptions {
            revert: Some(crate::store::RevertState {
                message_id: "msg_z".into(),
                files: vec![crate::store::RevertFile {
                    file: "a.py".into(),
                    status: "modified".into(),
                    additions: 2,
                    deletions: 0,
                }],
            }),
            ..TranscriptOptions::default()
        };
        let transcript = render_transcript(&messages, &options, &theme(), 80);
        let text = frame_of(&transcript).text_grid().join("\n");
        assert!(text.contains("kept"), "the pre-boundary row stays: {text}");
        assert!(!text.contains("hidden"), "the boundary row hides: {text}");
        assert!(text.contains("1 message reverted"), "{text}");
        assert!(text.contains("M a.py +2"), "{text}");
    }

    #[test]
    fn ungrouped_tool_parts_render_their_detail_cell() {
        let messages = vec![Message::Assistant {
            id: "a1".into(),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            parts: vec![Part::Tool {
                id: "t1".into(),
                name: "bash".into(),
                time: PartTime::default(),
                state: crate::components::message::model::ToolState {
                    status: crate::components::message::model::ToolStatus::Completed,
                    input: [("command".to_owned(), "echo hi".to_owned())].into_iter().collect(),
                    metadata: std::collections::BTreeMap::new(),
                    metadata_json: None,
                    output: Some("hi".to_owned()),
                    error: None,
                },
            }],
            finish: Some("stop".into()),
            error: None,
            time: PartTime::default(),
            tokens: None,
            usage_total: None,
        }];
        let transcript = render_transcript(&messages, &TranscriptOptions::default(), &theme(), 80);
        let text = frame_of(&transcript).text_grid().join("\n");
        assert!(text.contains("$ echo hi"), "the shell cell: {text}");
        assert!(text.contains("hi"), "the output preview: {text}");
    }
}
