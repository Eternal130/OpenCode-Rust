//! Session message data model + the grouping projection — the port of
//! `routes/session/grouping/{tree,session}.ts` and the
//! `reduceSessionRows` core of `routes/session/rows.ts` (7.2 lane C).
//!
//! The model is deliberately lean: the shapes the transcript renders
//! (user/assistant messages, text/reasoning/tool parts, idle and
//! compaction markers) plus the SSE append events that build them (see
//! [`crate::store`]). Everything here is pure — the store owns mutation,
//! [`crate::components::message::list`] owns painting.

use std::collections::HashMap;

/// A part reference (`PartRef`): identity of one part inside one
/// assistant message.
pub type PartRef = (String, String);

/// Timestamps a part/message carries (`time.created` / `time.completed`,
/// epoch milliseconds; `None` while open).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PartTime {
    pub created: Option<u64>,
    pub completed: Option<u64>,
}

/// One user-message attachment file (`Session.Message.User.files` —
/// the resolved form the server stores): the fields the transcript's
/// image strip consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFile {
    pub mime: String,
    /// Base64 payload (`data:<mime>;base64,<data>` reassembles for
    /// display and the preview dialog).
    pub data: String,
    pub name: Option<String>,
}

impl UserFile {
    #[must_use]
    pub fn is_image(&self) -> bool {
        self.mime.starts_with("image/")
    }

    /// The display data URI (`data:${mime};base64,${data}` — the v2
    /// `<image source>` shape, session/index.tsx:2193).
    #[must_use]
    pub fn data_uri(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.data)
    }
}

/// `deduplicateVisibleImages` (prompt/attachment.ts:65): drop repeated
/// inline image attachments that share [mime, name, description,
/// mention] metadata and payload — the echo-of-my-own-prompt case.
#[must_use]
pub fn deduplicate_visible_files(files: &[UserFile]) -> Vec<UserFile> {
    let mut seen: Vec<(String, String)> = Vec::new();
    files
        .iter()
        .filter(|file| {
            if !file.is_image() {
                return true;
            }
            let key = (format!("{}|{:?}", file.mime, file.name), file.data.clone());
            if seen.contains(&key) {
                return false;
            }
            seen.push(key);
            true
        })
        .cloned()
        .collect()
}

/// One assistant content part.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text {
        text: String,
        time: PartTime,
    },
    Reasoning {
        text: String,
        time: PartTime,
    },
    Tool {
        /// The tool part id (`part.id` on the wire — partIDs for
        /// text/reasoning are `text:N` / `reasoning:N` ordinals).
        id: String,
        name: String,
        time: PartTime,
        /// The tool part's live state (`part.state` on the wire —
        /// `{status, input, metadata}` per the v2 client reducers,
        /// data.ts:919-980). Drives the subagent card's running/
        /// completed shape (#40).
        state: ToolState,
    },
}

/// A tool part's state (the `SessionMessageAssistantTool.state`
/// union's display-relevant projection): status + the stringified
/// input/metadata maps the cards read (`stringValue` semantics —
/// non-string JSON coerces, `null` drops).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolState {
    pub status: ToolStatus,
    /// `state.input` (parsed args; empty while streaming).
    pub input: std::collections::BTreeMap<String, String>,
    /// `state.metadata` (the subagent tool's progress payload carries
    /// the child `sessionID`).
    pub metadata: std::collections::BTreeMap<String, String>,
    /// The RAW metadata JSON (`state.metadata` verbatim) — the detail
    /// cells read STRUCTURED fields the stringified map flattens away
    /// (`loaded` arrays, `files` objects, `toolCalls`).
    pub metadata_json: Option<serde_json::Value>,
    /// `state.content`'s joined text blocks (`toolDisplayContent`,
    /// util/tool-display.ts:59-62) — the shell output / read output
    /// the detail cells preview.
    pub output: Option<String>,
    /// `state.error.message` on `failed` — the error row + the
    /// `DialogExecute` error variant.
    pub error: Option<String>,
}

/// The tool part status (`state.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStatus {
    /// `streaming` — args still arriving.
    #[default]
    Streaming,
    /// `running` — executing.
    Running,
    /// `completed`.
    Completed,
    /// `error`.
    Error,
}

impl ToolStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Error => "error",
        }
    }

    /// Parse the wire discriminator.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "error" => Self::Error,
            _ => Self::Streaming,
        }
    }

    /// The part is settled (`completed`/`error`).
    #[must_use]
    pub const fn settled(self) -> bool {
        matches!(self, Self::Completed | Self::Error)
    }
}

impl Part {
    /// The part's kind discriminator (`part.type`).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Reasoning { .. } => "reasoning",
            Self::Tool { .. } => "tool",
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Text { text, .. } | Self::Reasoning { text, .. } => text,
            Self::Tool { .. } => "",
        }
    }

    #[must_use]
    pub fn time(&self) -> PartTime {
        match self {
            Self::Text { time, .. } | Self::Reasoning { time, .. } | Self::Tool { time, .. } => {
                *time
            }
        }
    }
}

/// One session message (`SessionMessageInfo` subset the transcript
/// renders).
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    User {
        id: String,
        text: String,
        created: u64,
        /// Attachment files (R-07): the transcript's image strip
        /// (`SessionImages`) renders the inline images among these.
        files: Vec<UserFile>,
    },
    Assistant {
        id: String,
        agent: String,
        /// Model label fallback (`providerID/id` when not in the model
        /// list — the fixture model never is).
        model: String,
        /// Arrival order of the parts (parts render in this order; SSE
        /// deltas merge into the part keyed by `Part::kind` + ordinal).
        parts: Vec<Part>,
        /// `finish` of the terminal step (`None` while streaming).
        finish: Option<String>,
        /// The step's structured error (`message.error`): renders the
        /// `Error: …` row above the assistant footer
        /// (`AssistantFooter`, ref session/index.tsx:1879).
        error: Option<String>,
        time: PartTime,
        /// `tokens.output + tokens.reasoning` of the terminal step
        /// (lane/chrome #13 — the footer's tok/s numerator; the ref's
        /// `turnTokensPerSecond` sums over the turn's steps).
        tokens: Option<u64>,
        /// The full context-usage total
        /// (`input+output+reasoning+cache.read+cache.write`,
        /// `contextUsage`, util/session.ts:58-73) — the footer usage
        /// row's numerator.
        usage_total: Option<u64>,
    },
    /// `session.idle` marker: closes a turn (drives turn duration and
    /// the wave-3 unread semantics).
    Idle { id: String, created: u64 },
    /// `compaction` message — the `──── Compaction ────` divider row
    /// (`CompactionMessage`, session/index.tsx:2002-2068). The wire
    /// union's three statuses project onto these fields (running →
    /// `running`, completed → summary/tokens, failed → error/cancel).
    Compaction {
        id: String,
        created: u64,
        /// `status === "running"`.
        running: bool,
        /// `status === "failed"`.
        failed: bool,
        /// failed AND `error.type === "aborted"` — the `· cancelled`
        /// suffix (index.tsx:2008); suppresses the error color.
        cancelled: bool,
        /// failed (not cancelled) → `error.message` — the divider
        /// paints it in the error color instead of a summary.
        error: Option<String>,
        /// running/completed → `summary` (the markdown body under the
        /// divider, paddingTop 1 paddingLeft 3).
        summary: Option<String>,
        /// `tokens.input + cache.read + cache.write` — the `N in`
        /// numerator (`CompactionMessage`'s usage, index.tsx:2021-2029).
        tokens_in: Option<u64>,
        /// `tokens.output + reasoning` — the `M out` numerator.
        tokens_out: Option<u64>,
        /// `providerContext` set on completion → "Provider compaction".
        provider_compaction: bool,
    },
    /// `skill` message — the `→ Skill {name}` inline row
    /// (`SessionSkillMessage`, session/index.tsx:1993-1999).
    Skill { id: String, name: String, created: u64 },
    /// `synthetic` message: an instruction load (`metadata.instruction`)
    /// — groups under one `◈ Instructions: N files` summary row
    /// (`instructionPaths`).
    Synthetic {
        id: String,
        /// `metadata.instruction.paths` — one load can carry several.
        instruction_paths: Vec<String>,
        created: u64,
    },
}

impl Message {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::User { id, .. }
            | Self::Assistant { id, .. }
            | Self::Idle { id, .. }
            | Self::Compaction { id, .. }
            | Self::Skill { id, .. }
            | Self::Synthetic { id, .. } => id,
        }
    }

    /// The assistant parts (empty for other message kinds).
    #[must_use]
    pub fn parts(&self) -> &[Part] {
        match self {
            Self::Assistant { parts, .. } => parts,
            _ => &[],
        }
    }

    #[must_use]
    pub fn created(&self) -> u64 {
        match self {
            Self::User { created, .. }
            | Self::Assistant { time: PartTime { created: Some(created), .. }, .. }
            | Self::Idle { created, .. }
            | Self::Compaction { created, .. }
            | Self::Skill { created, .. }
            | Self::Synthetic { created, .. } => *created,
            Self::Assistant { .. } => 0,
        }
    }

    /// `instructionPaths` (`grouping/session.ts`): the files a
    /// synthetic message loaded (empty for every other kind).
    #[must_use]
    pub fn instruction_paths(&self) -> &[String] {
        match self {
            Self::Synthetic { instruction_paths, .. } => instruction_paths,
            _ => &[],
        }
    }
}

/// Group kind (`GroupKind`). The four native kinds are the reference's
/// closed set; [`GroupKind::Plugin`] is wave 3's open rules table —
/// plugin-provided rules (match tool name/prefix → custom header)
/// extend grouping without touching the closed enum's semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKind {
    Activity,
    Reasoning,
    Exploration,
    Instructions,
    /// A plugin-defined group: the rule's header label.
    Plugin(String),
}

/// The plugin-provided grouping rules (the open rules table): checked
/// AFTER the native kinds (reasoning/exploration stay reference
/// behavior; a rule may claim any other tool).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupRules {
    pub rules: Vec<GroupRule>,
}

/// One rule: exact `tool` or `tool_prefix` match → `header` group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRule {
    pub tool: Option<String>,
    pub tool_prefix: Option<String>,
    pub header: String,
}

impl GroupRules {
    #[must_use]
    pub const fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// The matching plugin group for a tool name, if any rule claims
    /// it (first match wins; native kinds are checked before this).
    /// Matching is on the RAW tool name — the shape plugins register
    /// and see in `tool.transform` (canonicalization is a display
    /// concern).
    #[must_use]
    pub fn group_for(&self, tool: &str) -> Option<GroupKind> {
        self.rules
            .iter()
            .find(|rule| {
                rule.tool.as_deref() == Some(tool)
                    || rule.tool_prefix.as_deref().is_some_and(|prefix| tool.starts_with(prefix))
            })
            .map(|rule| GroupKind::Plugin(rule.header.clone()))
    }
}

/// The exploration tools that group under one "Explored" header
/// (`explorationTools`).
pub const EXPLORATION_TOOLS: [&str; 5] = ["read", "glob", "grep", "webfetch", "websearch"];

/// A leaf entry in the grouped transcript (`SessionEntry`).
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Message { message_id: String },
    Part { message_id: String, part_id: String },
    AssistantFooter { message_id: String },
}

/// A grouped transcript row (`SessionRow`): a leaf entry or a group.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Entry(Entry),
    Group {
        kind: GroupKind,
        children: Vec<Row>,
        /// Descendant leaf count (`size`).
        size: usize,
        completed: bool,
    },
}

impl Row {
    /// `groupRefs`: the part references under a row (groups recurse).
    #[must_use]
    pub fn part_refs(&self) -> Vec<PartRef> {
        match self {
            Self::Entry(Entry::Part { message_id, part_id }) => {
                vec![(message_id.clone(), part_id.clone())]
            }
            Self::Entry(_) => Vec::new(),
            Self::Group { children, .. } => children.iter().flat_map(Self::part_refs).collect(),
        }
    }

    /// The descendant message-entry ids (`descendants()` filtered to
    /// `entry.type === "message"` — the instructions group's input).
    #[must_use]
    pub fn message_refs(&self) -> Vec<String> {
        match self {
            Self::Entry(Entry::Message { message_id }) => vec![message_id.clone()],
            Self::Entry(_) => Vec::new(),
            Self::Group { children, .. } => children.iter().flat_map(Self::message_refs).collect(),
        }
    }
}

/// A projection input: one entry plus its grouping path and
/// close-previous flag (`ProjectionEntry`).
struct Projection {
    entry: Entry,
    part: Option<PartKind>,
    path: Vec<GroupKind>,
    /// `closesPrevious`: `None` mirrors the reference's `undefined`
    /// (part/footer entries — which still close, `!== false`), `Some(false)`
    /// marks pending messages that must not complete a previous group.
    closes_previous: Option<bool>,
    /// For reasoning parts: `part.time.completed` known (drives the
    /// all-children-completed group completion).
    reasoning_completed: bool,
}

/// The part-kind discriminator the path function needs.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PartKind {
    Text,
    Reasoning,
    Tool(String),
}

/// `partPath`: reasoning groups; exploration tools group; text parts
/// and non-exploration tools stand alone (verbosity medium — "low"'s
/// activity wrapping is out of scope). Plugin rules are checked AFTER
/// the native kinds (wave 3's open rules table).
fn part_path(part: &PartKind, rules: &GroupRules, group_exploration: bool) -> Vec<GroupKind> {
    match part {
        PartKind::Reasoning => vec![GroupKind::Reasoning],
        PartKind::Tool(name) if group_exploration && EXPLORATION_TOOLS.contains(&name.as_str()) => {
            vec![GroupKind::Exploration]
        }
        PartKind::Tool(name) => rules.group_for(name).map(|kind| vec![kind]).unwrap_or_default(),
        PartKind::Text => Vec::new(),
    }
}

/// `messagePath` for non-assistant messages: instruction loads
/// (synthetic messages) group; everything else stands alone
/// (`grouping/session.ts:94-97`, medium verbosity — "low" would wrap
/// them in an activity group too).
fn message_path(message: &Message) -> Vec<GroupKind> {
    if message.instruction_paths().is_empty() {
        Vec::new()
    } else {
        vec![GroupKind::Instructions]
    }
}

/// `reduceSessionRows` — messages → grouped rows. `inputs` are pending
/// user message ids (queued/steered); turn-token usage rows are out of
/// scope (debug flag).
#[must_use]
pub fn reduce_rows<S: std::hash::BuildHasher>(
    messages: &[Message],
    inputs: &std::collections::HashSet<String, S>,
) -> Vec<Row> {
    reduce_rows_with_rules(messages, inputs, &GroupRules::empty(), true)
}

/// [`reduce_rows`] with the plugin-provided grouping rules (wave 3's
/// open rules table — see [`GroupRules`]).
#[must_use]
pub fn reduce_rows_with_rules<S: std::hash::BuildHasher>(
    messages: &[Message],
    inputs: &std::collections::HashSet<String, S>,
    rules: &GroupRules,
    group_exploration: bool,
) -> Vec<Row> {
    let mut pending: std::collections::HashSet<String> = inputs
        .iter()
        .filter(|id| messages.iter().any(|m| m.id() == id.as_str()))
        .cloned()
        .collect::<std::collections::HashSet<String>>();
    for message in messages {
        if let Message::Compaction { id, running: true, .. } = message {
            pending.insert(id.clone());
        }
    }
    let mut projections: Vec<Projection> = Vec::new();
    // Without any idle marker, history predates markers and a turn ends
    // with its terminal step (`legacy`); with markers, steers keep the
    // turn open. Turn-usage rows themselves are a debug flag (out of
    // scope), so `legacy` only gates that debug projection.
    let legacy = !messages.iter().any(|m| matches!(m, Message::Idle { .. }));
    let _ = legacy;
    for message in messages {
        let is_pending = pending.contains(message.id());
        match message {
            Message::Idle { .. } => {}
            Message::Assistant { id, parts, finish, error, .. } => {
                let mut ordinals: HashMap<&str, usize> = HashMap::new();
                for part in parts {
                    let (kind, part_id) = match part {
                        Part::Text { .. } => {
                            let ordinal = ordinals.entry("text").or_default();
                            let id = format!("text:{ordinal}");
                            *ordinal += 1;
                            (PartKind::Text, id)
                        }
                        Part::Reasoning { .. } => {
                            let ordinal = ordinals.entry("reasoning").or_default();
                            let id = format!("reasoning:{ordinal}");
                            *ordinal += 1;
                            (PartKind::Reasoning, id)
                        }
                        Part::Tool { id, name, .. } => (PartKind::Tool(name.clone()), id.clone()),
                    };
                    if matches!(part, Part::Text { text, .. } | Part::Reasoning { text, .. }
                        if text.trim().is_empty())
                    {
                        continue;
                    }
                    projections.push(Projection {
                        entry: Entry::Part { message_id: id.clone(), part_id },
                        part: Some(kind.clone()),
                        path: part_path(&kind, rules, group_exploration),
                        closes_previous: None,
                        reasoning_completed: part.time().completed.is_some(),
                    });
                }
                // `terminal` (rows.ts:403): a finish outside
                // tool-calls/unknown OR a step error both close the
                // step's rendering with the footer (which paints the
                // error row).
                let terminal = finish
                    .as_deref()
                    .is_some_and(|finish| !["tool-calls", "unknown"].contains(&finish))
                    || error.is_some();
                if terminal {
                    projections.push(Projection {
                        entry: Entry::AssistantFooter { message_id: id.clone() },
                        part: None,
                        path: Vec::new(),
                        closes_previous: None,
                        reasoning_completed: false,
                    });
                }
            }
            Message::User { id, .. }
            | Message::Compaction { id, .. }
            | Message::Skill { id, .. }
            | Message::Synthetic { id, .. } => {
                projections.push(Projection {
                    entry: Entry::Message { message_id: id.clone() },
                    part: None,
                    path: message_path(message),
                    closes_previous: Some(!is_pending),
                    reasoning_completed: false,
                });
            }
        }
    }
    project_entries(projections)
}

/// `projectEntries` + `groupEntries` (`grouping/tree.ts`): adjacent
/// entries whose paths share a prefix nest into groups; a group is
/// `completed` when a following row exists and closes it (or every
/// reasoning child completed).
fn project_entries(projections: Vec<Projection>) -> Vec<Row> {
    /// `groupEntries`' building node (writable while constructing).
    struct Building {
        kind: Option<GroupKind>,
        children: Vec<Building>,
        entry: Option<Projection>,
        size: usize,
    }
    fn append(nodes: &mut Vec<Building>, projection: Projection, path: &[GroupKind], depth: usize) {
        let Some(kind) = path.get(depth).cloned() else {
            nodes.push(Building {
                kind: None,
                children: Vec::new(),
                entry: Some(projection),
                size: 1,
            });
            return;
        };
        if let Some(last) = nodes.last_mut() {
            if last.kind.as_ref() == Some(&kind) {
                last.size += 1;
                append(&mut last.children, projection, path, depth + 1);
                return;
            }
        }
        let mut children = Vec::new();
        append(&mut children, projection, path, depth + 1);
        nodes.push(Building { kind: Some(kind), children, entry: None, size: 1 });
    }
    fn to_rows(nodes: Vec<Building>) -> Vec<Row> {
        nodes
            .into_iter()
            .map(|node| match node.kind {
                None => Row::Entry(node.entry.expect("leaf entry").entry),
                Some(kind) => Row::Group {
                    kind,
                    children: to_rows(node.children),
                    size: node.size,
                    completed: false,
                },
            })
            .collect()
    }

    let mut nodes: Vec<Building> = Vec::new();
    for projection in projections {
        let path = projection.path.clone();
        append(&mut nodes, projection, &path, 0);
    }

    // `projectEntries` completion: a top-level group is completed when a
    // NEXT row exists and closes it — any following group, or an entry
    // with `closesPrevious !== false` — or (reasoning) when every child
    // part completed.
    let count = nodes.len();
    let mut rows: Vec<Row> = Vec::with_capacity(count);
    for index in 0..count {
        let node = std::mem::replace(
            &mut nodes[index],
            Building { kind: None, children: Vec::new(), entry: None, size: 0 },
        );
        let Some(kind) = node.kind else {
            rows.push(Row::Entry(node.entry.expect("leaf entry").entry));
            continue;
        };
        let next_closes = match nodes.get(index + 1) {
            None => false,
            Some(next) => {
                next.kind.is_some()
                    || next
                        .entry
                        .as_ref()
                        .is_none_or(|projection| projection.closes_previous != Some(false))
            }
        };
        let reasoning_all_done = kind == GroupKind::Reasoning
            && node.children.iter().all(|child| match &child.entry {
                Some(projection) => {
                    matches!(projection.part, Some(PartKind::Reasoning))
                        && projection.reasoning_completed
                }
                None => false,
            });
        rows.push(Row::Group {
            kind,
            children: to_rows(node.children),
            size: node.size,
            completed: next_closes || reasoning_all_done,
        });
    }
    rows
}

/// `reasoningSummary` (`context/thinking.ts`): a leading `**Title**`
/// block becomes disclosure metadata.
#[must_use]
pub fn reasoning_summary(text: &str) -> (Option<String>, String) {
    let content = text.trim();
    if let Some(after_open) = content.strip_prefix("**") {
        if let Some(close) = after_open.find("**") {
            let title = &after_open[..close];
            if !title.contains(['*', '\n']) {
                let rest = &after_open[close + 2..];
                let body = rest
                    .strip_prefix("\n\n")
                    .or_else(|| rest.strip_prefix("\r\n\r\n"))
                    .unwrap_or(rest);
                return (Some(title.trim().to_owned()), body.trim_end().to_owned());
            }
        }
    }
    (None, content.to_owned())
}

/// `Locale.duration`'s seconds arm: one decimal place.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn milliseconds_to_seconds(input: u64) -> f64 {
    f64::from(input as u32 % 10_000) / 1000.0
}

/// `Locale.duration` (`util/locale.ts`) — turn and group durations.
#[must_use]
pub fn locale_duration(input: u64) -> String {
    if input < 1000 {
        format!("{input}ms")
    } else if input < 60_000 {
        format!("{:.1}s", milliseconds_to_seconds(input))
    } else if input < 3_600_000 {
        let minutes = input / 60_000;
        let seconds = (input % 60_000) / 1000;
        format!("{minutes}m {seconds}s")
    } else if input < 86_400_000 {
        let hours = input / 3_600_000;
        let minutes = (input % 3_600_000) / 60_000;
        format!("{hours}h {minutes}m")
    } else {
        let days = input / 86_400_000;
        let hours = (input % 86_400_000) / 3_600_000;
        format!("{days}d {hours}h")
    }
}

/// `turnDuration`: the assistant step's completed time minus the
/// creating user message's time (nearest input walking back — legacy
/// sessions have no idle markers).
#[must_use]
pub fn turn_duration(message_id: &str, messages: &[Message]) -> u64 {
    let Some(index) = messages.iter().position(|m| m.id() == message_id) else {
        return 0;
    };
    let Message::Assistant { time, .. } = &messages[index] else { return 0 };
    let Some(completed) = time.completed else { return 0 };
    let mut input_created = None;
    for message in messages.iter().take(index + 1).rev() {
        match message {
            Message::Idle { .. } => break,
            Message::User { created, .. } => {
                input_created = Some(*created);
                break;
            }
            _ => {}
        }
    }
    completed.saturating_sub(input_created.unwrap_or_else(|| message_created(&messages[index])))
}

fn message_created(message: &Message) -> u64 {
    message.created()
}

/// `Locale.titlecase`: uppercase the first letter of every word.
#[must_use]
pub fn titlecase(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut boundary = true;
    for ch in text.chars() {
        if ch.is_whitespace() {
            boundary = true;
            out.push(ch);
        } else if boundary {
            out.extend(ch.to_uppercase());
            boundary = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// The exploration group's summary label (`GroupContent.label`):
/// `Explored: 2 searches, 1 read` — tool names deduped and counted,
/// grep/glob/websearch → "search", webfetch → "fetch".
#[must_use]
pub fn exploration_label(tools: &[&str], completed: bool) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for tool in tools {
        let canonical = canonical_tool_name(tool);
        let name = match canonical {
            "grep" | "glob" | "websearch" => "search",
            "webfetch" => "fetch",
            other => other,
        };
        match counts.iter_mut().find(|(existing, _)| existing == &name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    let names = counts
        .into_iter()
        .map(|(name, count)| {
            if count == 1 {
                format!("{count} {name}")
            } else if name == "search" || name == "fetch" {
                format!("{count} {name}es")
            } else {
                format!("{count} {name}s")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}: {names}", if completed { "Explored" } else { "Exploring" })
}

/// The instructions group's summary label (`InstructionsGroup`):
/// `Instructions: {files} {files === 1 ? "file" : "files"}` over the
/// count of DISTINCT paths its descendant messages loaded.
#[must_use]
pub fn instructions_label(files: usize) -> String {
    format!("Instructions: {files} {}", if files == 1 { "file" } else { "files" })
}

/// `canonicalToolName` (`util/tool-display`): strip provider prefixes.
#[must_use]
pub fn canonical_tool_name(tool: &str) -> &str {
    // The prefix strip (`foo_bash` → `bash`), then the display alias
    // map (`canonicalToolName`, util/tool-display.ts:1-6: bash→shell,
    // task→subagent, apply_patch→patch).
    let stripped = tool.rsplit_once('_').map_or(tool, |(_, last)| last);
    match stripped {
        "bash" => "shell",
        "task" => "subagent",
        "apply_patch" => "patch",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, text: &str, created: u64) -> Message {
        Message::User { id: id.into(), text: text.into(), created, files: Vec::new() }
    }

    fn assistant(id: &str, created: u64, completed: u64, parts: Vec<Part>) -> Message {
        Message::Assistant {
            id: id.into(),
            agent: "build".into(),
            model: "fixture/fixture".into(),
            parts,
            finish: Some("stop".into()),
            error: None,
            time: PartTime { created: Some(created), completed: Some(completed) },
            tokens: None,
            usage_total: None,
        }
    }

    /// The `message-groups` golden fixture shape: turn → tools
    /// (exploration group) → turn → reasoning + text.
    #[test]
    fn golden_fixture_reduces_to_grouped_rows() {
        let messages = vec![
            user("grp-user-0", "What does the parser do?", 0),
            assistant(
                "grp-answer-0",
                1,
                2,
                vec![
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
            ),
            user("grp-user-1", "Now summarize the failure modes", 10),
            assistant(
                "grp-answer-1",
                11,
                14,
                vec![
                    Part::Reasoning {
                        text: "**Failure modes**\n\nConsider recursion limits first".into(),
                        time: PartTime { created: Some(11), completed: Some(12) },
                    },
                    Part::Text {
                        text: "Three failure modes dominate: recursion limits, cycle detection and budget exhaustion.".into(),
                        time: PartTime::default(),
                    },
                ],
            ),
        ];
        let rows = reduce_rows(&messages, &std::collections::HashSet::<String>::new());
        let shape: Vec<String> = rows.iter().map(row_shape).collect();
        assert_eq!(
            shape,
            vec![
                "message:grp-user-0",
                "group:exploration(completed, 1)",
                "part:grp-answer-0:text:0",
                "footer:grp-answer-0",
                "message:grp-user-1",
                "group:reasoning(completed, 1)",
                "part:grp-answer-1:text:0",
                "footer:grp-answer-1",
            ]
        );
    }

    fn row_shape(row: &Row) -> String {
        match row {
            Row::Entry(Entry::Message { message_id }) => format!("message:{message_id}"),
            Row::Entry(Entry::Part { message_id, part_id }) => {
                format!("part:{message_id}:{part_id}")
            }
            Row::Entry(Entry::AssistantFooter { message_id }) => {
                format!("footer:{message_id}")
            }
            Row::Group { kind, size, completed, .. } => format!(
                "group:{}({}, {})",
                match kind {
                    GroupKind::Exploration => "exploration".to_owned(),
                    GroupKind::Reasoning => "reasoning".to_owned(),
                    GroupKind::Activity => "activity".to_owned(),
                    GroupKind::Instructions => "instructions".to_owned(),
                    GroupKind::Plugin(header) => format!("plugin:{header}"),
                },
                if *completed { "completed" } else { "open" },
                size
            ),
        }
    }

    #[test]
    fn exploration_groups_merge_adjacent_and_complete_on_next_row() {
        let messages = vec![
            user("u", "?", 0),
            Message::Assistant {
                id: "a".into(),
                agent: "build".into(),
                model: "fixture/fixture".into(),
                parts: vec![
                    Part::Tool {
                        id: "t1".into(),
                        name: "grep".into(),
                        time: PartTime { created: Some(1), completed: None },
                        state: ToolState::default(),
                    },
                    Part::Tool {
                        id: "t2".into(),
                        name: "read".into(),
                        time: PartTime { created: Some(2), completed: None },
                        state: ToolState::default(),
                    },
                ],
                finish: None,
                error: None,
                time: PartTime { created: Some(1), completed: None },
                tokens: None,
                usage_total: None,
            },
        ];
        let rows = reduce_rows(&messages, &std::collections::HashSet::<String>::new());
        let Row::Group { kind, size, completed, .. } = &rows[1] else {
            panic!("expected exploration group, got {:?}", rows[1]);
        };
        assert_eq!((kind.clone(), *size, *completed), (GroupKind::Exploration, 2, false));
        // "Explored: 1 search, 1 read" while open says "Exploring".
        assert_eq!(exploration_label(&["grep", "read"], false), "Exploring: 1 search, 1 read");
        assert_eq!(
            exploration_label(&["grep", "grep", "webfetch"], true),
            "Explored: 2 searches, 1 fetch"
        );
        assert_eq!(exploration_label(&["read"], true), "Explored: 1 read");
    }

    #[test]
    fn idle_marker_closes_turns_and_legacies_end_at_terminal_step() {
        let with_idle = vec![
            user("u", "?", 0),
            Message::Assistant {
                id: "a".into(),
                agent: "build".into(),
                model: "m".into(),
                parts: vec![Part::Text { text: "hi".into(), time: PartTime::default() }],
                finish: Some("stop".into()),
                error: None,
                time: PartTime { created: Some(1), completed: Some(5) },
                tokens: None,
                usage_total: None,
            },
            Message::Idle { id: "i".into(), created: 6 },
        ];
        let rows = reduce_rows(&with_idle, &std::collections::HashSet::<String>::new());
        assert_eq!(rows.len(), 3, "idle renders no row of its own");
        assert_eq!(row_shape(&rows[2]), "footer:a");
        // Duration walks to the nearest input.
        assert_eq!(turn_duration("a", &with_idle), 5);
        let steered = vec![
            user("u", "?", 0),
            Message::Assistant {
                id: "a".into(),
                agent: "build".into(),
                model: "m".into(),
                parts: vec![Part::Text { text: "hi".into(), time: PartTime::default() }],
                finish: Some("tool-calls".into()),
                error: None,
                time: PartTime { created: Some(1), completed: Some(5) },
                tokens: None,
                usage_total: None,
            },
            Message::Assistant {
                id: "b".into(),
                agent: "build".into(),
                model: "m".into(),
                parts: vec![Part::Text { text: "there".into(), time: PartTime::default() }],
                finish: Some("stop".into()),
                error: None,
                time: PartTime { created: Some(6), completed: Some(9) },
                tokens: None,
                usage_total: None,
            },
        ];
        let rows = reduce_rows(&steered, &std::collections::HashSet::<String>::new());
        assert!(
            !rows.iter().any(|row| matches!(row, Row::Entry(Entry::AssistantFooter { message_id })
                if message_id == "a")),
            "tool-calls finish is not terminal"
        );
        assert_eq!(turn_duration("b", &steered), 9, "same input spans steered steps");
    }

    #[test]
    fn blank_text_and_reasoning_parts_skip() {
        let messages = vec![Message::Assistant {
            id: "a".into(),
            agent: "build".into(),
            model: "m".into(),
            parts: vec![
                Part::Text { text: "  ".into(), time: PartTime::default() },
                Part::Text { text: "real".into(), time: PartTime::default() },
            ],
            finish: None,
            error: None,
            time: PartTime::default(),
            tokens: None,
            usage_total: None,
        }];
        let rows = reduce_rows(&messages, &std::collections::HashSet::<String>::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(
            row_shape(&rows[0]),
            "part:a:text:1",
            "blank parts consume ordinals (reference assigns partIDs before the skip)"
        );
    }

    #[test]
    fn reasoning_summary_extracts_leading_bold_title() {
        let (title, body) =
            reasoning_summary("**Failure modes**\n\nConsider recursion limits first");
        assert_eq!(title.as_deref(), Some("Failure modes"));
        assert_eq!(body, "Consider recursion limits first");
        let (title, body) = reasoning_summary("no title here");
        assert_eq!(title, None);
        assert_eq!(body, "no title here");
        // A title still awaiting its body.
        let (title, body) = reasoning_summary("**Pending**");
        assert_eq!(title.as_deref(), Some("Pending"));
        assert_eq!(body, "");
    }

    #[test]
    fn locale_duration_matches_reference_format() {
        assert_eq!(locale_duration(0), "0ms");
        assert_eq!(locale_duration(2), "2ms");
        assert_eq!(locale_duration(999), "999ms");
        assert_eq!(locale_duration(1500), "1.5s");
        assert_eq!(locale_duration(61_500), "1m 1s");
        assert_eq!(locale_duration(3_780_000), "1h 3m");
        assert_eq!(locale_duration(90_000_000), "1d 1h");
    }

    #[test]
    fn titlecase_uppercases_word_starts() {
        assert_eq!(titlecase("build"), "Build");
        assert_eq!(titlecase("plan b"), "Plan B");
    }

    #[test]
    fn canonical_tool_names_strip_prefixes() {
        assert_eq!(canonical_tool_name("fs_read"), "read");
        assert_eq!(canonical_tool_name("read"), "read");
    }

    /// Consecutive synthetic instruction loads reduce into ONE
    /// instructions group (`messagePath`), completed by a following
    /// closing row.
    #[test]
    fn synthetic_instruction_loads_group() {
        let messages = vec![
            Message::Synthetic {
                id: "a".into(),
                instruction_paths: vec!["AGENTS.md".into()],
                created: 1,
            },
            Message::Synthetic {
                id: "b".into(),
                instruction_paths: vec!["packages/tui/AGENTS.md".into()],
                created: 2,
            },
            user("u", "?", 3),
        ];
        let rows = reduce_rows(&messages, &std::collections::HashSet::<String>::new());
        assert_eq!(
            row_shape(&rows[0]),
            "group:instructions(completed, 2)",
            "the two loads merged; the user row closes it"
        );
        assert_eq!(rows[0].message_refs().len(), 2, "both descendant messages reachable");
        // The label counts DISTINCT files across the group.
        assert_eq!(instructions_label(1), "Instructions: 1 file");
        assert_eq!(instructions_label(2), "Instructions: 2 files");
        // A synthetic message with no paths stands alone (no group).
        let bare =
            vec![Message::Synthetic { id: "s".into(), instruction_paths: vec![], created: 0 }];
        let rows = reduce_rows(&bare, &std::collections::HashSet::<String>::new());
        assert_eq!(row_shape(&rows[0]), "message:s");
    }
}
