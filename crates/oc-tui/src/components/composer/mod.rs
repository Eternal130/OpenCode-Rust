//! The prompt composer (`component/prompt/index.tsx`, the 2011-LOC v2
//! surface) — 7.2 wave 2's editing core: draft state, multiline
//! editing per the v2 key table, prompt history, attachments (file
//! extmark chips), paste normalization, and submit → store action →
//! the engine prompt API stub.
//!
//! Division of labor (the lane contract): the **visible text grid is
//! C1** — byte-exact against the composer goldens via
//! [`render::render_prompt_frame`]; **caret/selection internals stay
//! C2** — the caret renders as v2's visible resting glyph only (i.e.
//! nothing in a golden frame; see `render` docs), and selection
//! ranges/IME composition are future work.
//!
//! Layout model (derived from the goldens): the textarea wraps with
//! word semantics where a break keeps the trailing space on the
//! previous line, CJK clusters break anywhere, and extmark chips
//! ([Image 1]-style file mentions) are atomic. The textarea's visible
//! height is `min(maxHeight, wrappedLines + 1)` — the +1 being the
//! cursor row under the last line.

pub mod attachment;
pub mod autocomplete;
pub mod golden_tests;
pub mod render;

pub use attachment::{
    parse_pasted_filepaths, prompt_attachment_label, resolve_pasted_attachments, PastedAttachment,
};
pub use autocomplete::{AutocompleteState, SlashEntry, SlashRegistry};
pub use oc_tui_core::text_edit::EditCommand as ComposerEditCommand;
pub use oc_tui_core::text_edit::{EditCore, EditOutcome, SnapshotKind};
pub use render::{prompt_text_width, AutocompleteView, FooterHints, PromptView};

use oc_tui_core::text_edit::EditCommand;

/// An inline attachment payload staged in the composer (the resolved
/// half of a `PromptInput.FileAttachment`: the data URI decoded into
/// base64 + mime, ready for `session.prompt`'s `files` wire shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentPayload {
    pub mime: String,
    /// Base64 bytes (`data:<mime>;base64,<data>` reassembles on send).
    pub data: String,
}

impl AttachmentPayload {
    /// Decode a `data:<mime>;base64,<payload>` URI (the attachment
    /// form `pasteAttachment` stages); `None` for foreign shapes.
    #[must_use]
    pub fn from_data_uri(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix("data:")?;
        let (header, payload) = rest.split_once(',')?;
        if !header.to_ascii_lowercase().contains("base64") {
            return None;
        }
        let mime = header.split(';').next().unwrap_or_default();
        Some(Self { mime: mime.to_owned(), data: payload.to_owned() })
    }

    /// Reassemble the data URI (the `session.prompt` wire form).
    #[must_use]
    pub fn to_data_uri(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.data)
    }
}

/// One file attachment rendered as an extmark chip inside the text
/// (`PromptInfo.files`): `name` + the char-range of the mention text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMention {
    pub name: String,
    /// Char index of the first mention character in `PromptDraft::text`.
    pub start: usize,
    /// Char index one past the last mention character.
    pub end: usize,
    /// The inline payload when this chip is a staged *attachment*
    /// (image/PDF); `None` for plain @-file mentions.
    pub payload: Option<AttachmentPayload>,
    /// The assigned chip label (`[Image 2]`, `promptAttachmentLabel`)
    /// for attachments; `None` for plain mentions (label = name).
    pub mention: Option<String>,
}

impl FileMention {
    /// The chip's mention text (the stored label for attachments,
    /// else the name).
    #[must_use]
    pub fn mention_text(&self) -> String {
        self.mention.clone().unwrap_or_else(|| self.name.clone())
    }
}

/// One agent mention (`@build`-style chips). RD-05: the chip's char
/// range inside `PromptDraft::text` (the extmark's virtual range —
/// autocomplete.tsx:230-238) rides along so the part survives edits
/// and submits as the wire `agents` attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMention {
    pub name: String,
    /// Char index of the `@` (the chip's first character).
    pub start: usize,
    /// Char index one past the chip label's last character.
    pub end: usize,
}

/// One skill mention (`@skill`-style chips, RD-05): the wire shape is
/// `PromptInput.SkillAttachment` `{id, mention}` — the name stays for
/// the stash/render labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMention {
    pub id: String,
    pub name: String,
    /// Char index of the `@` (the chip's first character).
    pub start: usize,
    /// Char index one past the chip label's last character.
    pub end: usize,
}

/// One folded paste (`store.prompt.pasted`, RD-06): the full pasted
/// text plus the char range of its `[Pasted ~N lines]` virtual chip
/// inside `PromptDraft::text` (the extmark's source range).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedPart {
    /// The TRIMMED pasted content (ref stores `pastedContent`).
    pub text: String,
    /// Char index of the chip label's first character.
    pub start: usize,
    /// Char index one past the chip label's last character.
    pub end: usize,
}

/// The paste-summary fold thresholds (`prompt/index.tsx:1452-1465`):
/// a paste folds when it spans ≥3 lines OR exceeds 150 characters.
pub const PASTE_SUMMARY_MIN_LINES: usize = 3;
pub const PASTE_SUMMARY_MAX_CHARS: usize = 150;

/// The composer's input mode (`store.mode`, prompt/index.tsx:91):
/// `!` typed at caret 0 flips to [`ComposerMode::Shell`] — the
/// footer swaps to `esc exit shell mode`, the metadata row labels
/// `Shell`, and Enter routes the draft through `session.shell`
/// instead of `session.prompt` (TUI-14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComposerMode {
    #[default]
    Normal,
    Shell,
}

/// A prompt draft (`PromptInfo`): the text plus its attachment parts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptDraft {
    pub text: String,
    pub files: Vec<FileMention>,
    pub agents: Vec<AgentMention>,
    /// RD-05 — the `@skill` chips (`store.prompt.skills`): each entry
    /// tracks the chip range in `text` + the skill id the wire
    /// `skills` attachment carries.
    pub skills: Vec<SkillMention>,
    /// RD-06 — the folded pastes (`store.prompt.pasted`): each entry
    /// tracks the chip range in `text` + the full content it expands
    /// back to.
    pub pasted: Vec<PastedPart>,
}

impl PromptDraft {
    /// One logical line of the draft.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.text.split('\n').count()
    }

    /// Empty drafts show the placeholder.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// `expandTrackedPastedText` (prompt/part.ts:10-15, RD-06): the
    /// draft's text with every paste chip's range replaced by its
    /// full content — back-to-front so the earlier ranges hold. The
    /// submit (:1123-1132) and $EDITOR (:556) chains both read this
    /// shape; a draft with no chips returns its text unchanged.
    #[must_use]
    pub fn expand_pasted_ranges(&self) -> String {
        let mut ranges: Vec<&PastedPart> = self.pasted.iter().collect();
        ranges.sort_by_key(|part| std::cmp::Reverse(part.start));
        let mut result = self.text.clone();
        for part in ranges {
            if part.start >= part.end {
                continue;
            }
            let start = char_to_byte(&result, part.start.min(result.chars().count()));
            let end = char_to_byte(&result, part.end.min(result.chars().count()));
            if start < end && end <= result.len() {
                result.replace_range(start..end, &part.text);
            }
        }
        result
    }
}

/// What a submit produced — the value handed to the engine stub.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmittedPrompt {
    pub text: String,
    pub files: Vec<FileMention>,
    pub agents: Vec<AgentMention>,
    /// RD-05 — the `@skill` chips (autocomplete.tsx:216-227): they
    /// POST as the wire `skills` array (index.tsx:1362).
    pub skills: Vec<SkillMention>,
    /// TUI-TD07 — the submit's inbox delivery ("queue" via
    /// `<leader>return`; the default "steer" sends None here and the
    /// wire body carries `delivery: "steer"`, prompt/index.tsx:1092).
    pub delivery: Option<String>,
    /// TUI-TD07 — the client-minted inbox id (the queue path's
    /// optimistic admit; the POST body carries it so the SSE echo
    /// re-admits under the same id, data.ts:769-787).
    pub inbox_id: Option<String>,
    /// TUI-15 — the pending editor context's `<system-reminder>` text
    /// (index.tsx:1339-1344): the sink admits it via
    /// `session.synthetic` BEFORE the prompt POST, then the label
    /// flips to sent.
    pub editor_context: Option<String>,
}

/// `src/prompt/history.ts` semantics (the golden-relevant subset): a
/// bounded ring with a cursor and a staging slot for the draft being
/// edited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptHistory {
    pub entries: Vec<String>,
    /// Cursor position: `None` = at the live draft (bottom).
    pub cursor: Option<usize>,
    pub staging: Option<String>,
}

pub const PROMPT_HISTORY_LIMIT: usize = 100;

impl PromptHistory {
    /// Record a submitted prompt (dedup vs the latest; bounded).
    #[must_use]
    pub fn push(mut self, text: &str) -> Self {
        if self.entries.last().is_none_or(|last| last != text) {
            self.entries.push(text.to_owned());
        }
        if self.entries.len() > PROMPT_HISTORY_LIMIT {
            let excess = self.entries.len() - PROMPT_HISTORY_LIMIT;
            self.entries.drain(0..excess);
        }
        self.cursor = None;
        self.staging = None;
        self
    }

    /// Previous (older) entry; the first call stashes the live draft.
    #[must_use]
    pub fn previous(self, draft: &str) -> (Self, Option<String>) {
        if self.entries.is_empty() {
            return (self, None);
        }
        let index = match self.cursor {
            // At the live draft: jump to the newest entry.
            None => self.entries.len() - 1,
            Some(0) => return (self, None),
            Some(index) => index - 1,
        };
        let mut state = self;
        if state.staging.is_none() {
            state.staging = Some(draft.to_owned());
        }
        state.cursor = Some(index);
        (state.clone(), state.entries.get(index).cloned())
    }

    /// Next (newer) entry; past the end restores the stashed draft.
    #[must_use]
    pub fn next(self, draft: &str) -> (Self, String) {
        let Some(index) = self.cursor else {
            return (self, draft.to_owned());
        };
        let mut state = self;
        if index + 1 >= state.entries.len() {
            state.cursor = None;
            let restored = state.staging.clone().unwrap_or_default();
            state.staging = None;
            return (state, restored);
        }
        state.cursor = Some(index + 1);
        (state.clone(), state.entries[index + 1].clone())
    }
}

/// The composer's full editing state for one session.
///
/// The editing core is oc-tui-core's [`EditCore`] (v2's
/// `EditBufferRenderable` over opentui-core's EditBuffer/EditorView —
/// selection, word motions, delete ops, undo/redo); the composer
/// keeps the file/agent mention bookkeeping and the prompt history
/// beside it. `draft.text` is the store-facing text source; the
/// engine's text mirrors it across each edit (see
/// [`Composer::run_edit`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Composer {
    pub draft: PromptDraft,
    /// The editing state (caret + selection anchor + undo/redo) — the
    /// opentui-core textarea line behind [`EditCore`].
    pub edit: EditCore,
    /// The mention snapshots parallel to the area's undo stack
    /// (an undo restores chips with their text).
    pub(crate) files_undo: Vec<Vec<FileMention>>,
    pub(crate) files_redo: Vec<Vec<FileMention>>,
    /// RD-06 — the folded pastes mirrored onto the area's undo stack
    /// (parallel to `files_undo`/`files_redo`, one-for-one).
    pub(crate) pasted_undo: Vec<Vec<PastedPart>>,
    pub(crate) pasted_redo: Vec<Vec<PastedPart>>,
    /// RD-05 — the `@agent`/`@skill` chips mirrored onto the area's
    /// undo stack (parallel to `files_undo`/`files_redo`).
    pub(crate) agents_undo: Vec<Vec<AgentMention>>,
    pub(crate) agents_redo: Vec<Vec<AgentMention>>,
    pub(crate) skills_undo: Vec<Vec<SkillMention>>,
    pub(crate) skills_redo: Vec<Vec<SkillMention>>,
    pub history: PromptHistory,
    pub placeholders: Vec<String>,
    pub placeholder_index: usize,
    pub autocomplete: AutocompleteState,
    /// TUI-28 — the destructive ctrl+d requests the second press
    /// armed (the absolute directories to drop from the store's
    /// recents; the app drains them — the removal itself touches the
    /// per-project store, which the pure composer cannot reach).
    pub destructive_requests: Vec<String>,
    /// TUI-14 — the input mode (`store.mode`): `Shell` while the
    /// `!` prefix holds (esc/backspace-at-0 exit; Enter sends through
    /// `session.shell`).
    pub mode: ComposerMode,
}

impl Default for Composer {
    fn default() -> Self {
        Self {
            draft: PromptDraft::default(),
            edit: Self::fresh_edit(),
            files_undo: Vec::new(),
            files_redo: Vec::new(),
            pasted_undo: Vec::new(),
            pasted_redo: Vec::new(),
            agents_undo: Vec::new(),
            agents_redo: Vec::new(),
            skills_undo: Vec::new(),
            skills_redo: Vec::new(),
            history: PromptHistory::default(),
            placeholders: Vec::new(),
            placeholder_index: 0,
            autocomplete: AutocompleteState::default(),
            destructive_requests: Vec::new(),
            mode: ComposerMode::Normal,
        }
    }
}

impl Composer {
    /// A fresh editing state (the opentui-core textarea engine, with
    /// its own grapheme model).
    #[must_use]
    pub fn fresh_edit() -> EditCore {
        EditCore::fresh()
    }
}

/// A key event in the pump's vocabulary (the subset the composer
/// consumes; see `keymap::KeyInput` for the full shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerKey {
    Char(char),
    Enter,
    ShiftEnter,
    CtrlEnter,
    AltEnter,
    CtrlJ,
    Up,
    Down,
    Left,
    Right,
    /// Wave A: the selection moves (v2's `{ select: true }` family,
    /// keybind.ts:212-215 — shift+arrows).
    ShiftLeft,
    ShiftRight,
    ShiftUp,
    ShiftDown,
    Home,
    End,
    Backspace,
    Delete,
    CtrlA,
    CtrlE,
    CtrlK,
    CtrlU,
    CtrlC,
    CtrlV,
    /// TUI-28 — the autocomplete layer's destructive confirm
    /// (`prompt.autocomplete.destructive`, bind ctrl+d —
    /// autocomplete.tsx:728-731).
    CtrlD,
    Escape,
    Tab,
}

/// Outcome of a key press: what the composer did (for assertions and
/// the pump's redraw scheduling).
#[derive(Debug, Clone, PartialEq)]
pub enum KeyOutcome {
    /// Nothing consumed (let the next layer handle it).
    Ignored,
    Edited,
    /// Submit produced a prompt.
    Submitted(SubmittedPrompt),
    /// Accepting a slash entry EXECUTED a built-in TUI command
    /// (v2 `onSelect: command.run`, `autocomplete.tsx:481`) — the
    /// app dispatches it through the command seam instead of
    /// submitting text to the model.
    Command(&'static str),
    /// TUI-14 — a SHELL-MODE submit (Enter in `!` mode): the text
    /// routes through `session.shell` (POST /api/session/:id/shell),
    /// never the model prompt; the mode already reset to Normal.
    ShellCommand(String),
    /// The `prompt.paste` command consumed the key (D-05): the paste
    /// handler's own `preventDefault`+`stopPropagation` parity — the
    /// stroke never reaches the draft or lower layers.
    Paste(PasteDisposition),
}

/// The `prompt.paste` command's event disposition. v2 binds ctrl+v as
/// a binding OBJECT with `preventDefault: false` (config/keybind.ts:
/// 205) — the keymap layer does NOT auto-suppress the matched event;
/// the handler decides, calling `event?.preventDefault()` +
/// `event?.stopPropagation()` synchronously, ONLY when the keymap
/// handed it a real key event (prompt/index.tsx:470-492). A
/// programmatic `keymap.dispatch("prompt.paste")` (the empty
/// bracketed-paste path, prompt/index.tsx:1787) has no event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteDisposition {
    /// Key-event driven: suppressed — the terminal's default ctrl+v
    /// (^V byte) never reaches the input, and the stroke stops here.
    Prevented,
    /// Dispatched without an event: nothing to prevent; the paste
    /// command runs identically.
    NoEvent,
}

impl Composer {
    /// The live popup state over the v2 slash registry (the store's
    /// composer constructor seeds fresh sessions with it — typing `/`
    /// opens the command palette with descriptions).
    #[must_use]
    pub fn live_autocomplete() -> AutocompleteState {
        AutocompleteState::with_registry(SlashRegistry::v2())
    }

    /// Caret offset in chars (the editing state's caret).
    #[must_use]
    pub fn caret(&self) -> usize {
        self.edit.caret()
    }

    /// Place the caret (tests + the history/paste paths that anchor
    /// it at an edge).
    pub fn set_caret(&mut self, caret: usize) {
        self.edit.set_caret(caret);
    }

    /// The caret/selection accessor core (tests + the render seam).
    #[must_use]
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        self.edit.selection()
    }

    /// Run one selection command (`{ select: true }` motions).
    fn select_command(&mut self, command: EditCommand) -> KeyOutcome {
        if self.apply_edit(command) {
            KeyOutcome::Edited
        } else {
            KeyOutcome::Ignored
        }
    }

    /// The selected text, if any.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.edit.selected_text()
    }

    /// `PromptRef.set` — the draft-restore path (stash/undo).
    pub fn set(&mut self, draft: PromptDraft) {
        self.draft = draft;
        self.edit.set_text(&self.draft.text);
        self.files_undo.clear();
        self.files_redo.clear();
        self.pasted_undo.clear();
        self.pasted_redo.clear();
        self.agents_undo.clear();
        self.agents_redo.clear();
        self.skills_undo.clear();
        self.skills_redo.clear();
        self.autocomplete.close();
    }

    /// `PromptRef.reset`.
    pub fn reset(&mut self) {
        self.draft = PromptDraft::default();
        self.edit.set_text("");
        self.files_undo.clear();
        self.files_redo.clear();
        self.pasted_undo.clear();
        self.pasted_redo.clear();
        self.agents_undo.clear();
        self.agents_redo.clear();
        self.skills_undo.clear();
        self.skills_redo.clear();
        self.autocomplete.close();
    }

    fn char_len(&self) -> usize {
        self.draft.text.chars().count()
    }

    /// Shift the mention ranges over one text delta (the deleted char
    /// range and the inserted length): mentions INSIDE the replaced
    /// range drop, mentions after it shift. RD-06: the paste chips'
    /// ranges shift under the same rule (the extmarks move with the
    /// buffer).
    fn apply_files_delta(&mut self, replaced: std::ops::Range<usize>, inserted: usize) {
        let removed = replaced.len();
        self.draft.files.retain(|file| file.end <= replaced.start || file.start >= replaced.end);
        for file in &mut self.draft.files {
            if file.start >= replaced.end {
                file.start = file.start - removed + inserted;
                file.end = file.end - removed + inserted;
            }
        }
        self.draft.pasted.retain(|part| part.end <= replaced.start || part.start >= replaced.end);
        for part in &mut self.draft.pasted {
            if part.start >= replaced.end {
                part.start = part.start - removed + inserted;
                part.end = part.end - removed + inserted;
            }
        }
        // RD-05: the `@agent`/`@skill` chips' ranges shift under the
        // same rule (their extmarks move with the buffer).
        self.draft.agents.retain(|part| part.end <= replaced.start || part.start >= replaced.end);
        for part in &mut self.draft.agents {
            if part.start >= replaced.end {
                part.start = part.start - removed + inserted;
                part.end = part.end - removed + inserted;
            }
        }
        self.draft.skills.retain(|part| part.end <= replaced.start || part.start >= replaced.end);
        for part in &mut self.draft.skills {
            if part.start >= replaced.end {
                part.start = part.start - removed + inserted;
                part.end = part.end - removed + inserted;
            }
        }
    }

    /// Mirror the mention + paste stacks onto the text area's history
    /// event (`files_undo`/`files_redo` track the area's snapshot
    /// stack one-for-one; RD-06: `pasted_undo`/`pasted_redo` mirror
    /// beside them).
    fn mirror_files_history(
        &mut self,
        files_before: Vec<FileMention>,
        pasted_before: Vec<PastedPart>,
        agents_before: Vec<AgentMention>,
        skills_before: Vec<SkillMention>,
        snapshot: SnapshotKind,
    ) {
        match snapshot {
            SnapshotKind::Pushed => {
                self.files_undo.push(files_before);
                self.files_redo.clear();
                self.pasted_undo.push(pasted_before);
                self.pasted_redo.clear();
                self.agents_undo.push(agents_before);
                self.agents_redo.clear();
                self.skills_undo.push(skills_before);
                self.skills_redo.clear();
            }
            SnapshotKind::Undone => {
                self.files_redo.push(self.draft.files.clone());
                if let Some(files) = self.files_undo.pop() {
                    self.draft.files = files;
                }
                self.pasted_redo.push(self.draft.pasted.clone());
                if let Some(pasted) = self.pasted_undo.pop() {
                    self.draft.pasted = pasted;
                }
                self.agents_redo.push(self.draft.agents.clone());
                if let Some(agents) = self.agents_undo.pop() {
                    self.draft.agents = agents;
                }
                self.skills_redo.push(self.draft.skills.clone());
                if let Some(skills) = self.skills_undo.pop() {
                    self.draft.skills = skills;
                }
            }
            SnapshotKind::Redone => {
                self.files_undo.push(self.draft.files.clone());
                if let Some(files) = self.files_redo.pop() {
                    self.draft.files = files;
                }
                self.pasted_undo.push(self.draft.pasted.clone());
                if let Some(pasted) = self.pasted_redo.pop() {
                    self.draft.pasted = pasted;
                }
                self.agents_undo.push(self.draft.agents.clone());
                if let Some(agents) = self.agents_redo.pop() {
                    self.draft.agents = agents;
                }
                self.skills_undo.push(self.draft.skills.clone());
                if let Some(skills) = self.skills_redo.pop() {
                    self.draft.skills = skills;
                }
            }
            SnapshotKind::None => {}
        }
    }

    /// The edit bridge: run one tracked text-area operation with the
    /// draft's text mirrored in/out, the mention ranges adjusted over
    /// the delta, the mention history mirrored, and the autocomplete
    /// refreshed on content changes (v2's `onContentChange`).
    fn run_tracked(
        &mut self,
        run: impl FnOnce(&mut EditCore) -> EditOutcome,
        refresh_autocomplete: bool,
    ) -> EditOutcome {
        self.edit.mirror_text(&self.draft.text);
        let files_before = self.draft.files.clone();
        let pasted_before = self.draft.pasted.clone();
        let agents_before = self.draft.agents.clone();
        let skills_before = self.draft.skills.clone();
        let outcome = run(&mut self.edit);
        self.draft.text = self.edit.text();
        let text_changed = match &outcome {
            EditOutcome::Rejected => false,
            EditOutcome::Applied { replaced, inserted, snapshot } => {
                if !matches!(snapshot, SnapshotKind::Undone | SnapshotKind::Redone) {
                    self.apply_files_delta(replaced.clone(), *inserted);
                }
                self.mirror_files_history(
                    files_before,
                    pasted_before,
                    agents_before,
                    skills_before,
                    *snapshot,
                );
                replaced.start != replaced.end || *inserted > 0
            }
        };
        if refresh_autocomplete && text_changed {
            self.autocomplete.on_input(&self.draft.text, self.edit.caret());
        }
        outcome
    }

    /// Apply one v2 `input.*` edit command (the keymap's composer
    /// slice — every binding in keybind.ts:206-243). The
    /// mention-aware pre-empts (an atomic chip backspace/delete) run
    /// first; `false` = the command rejected (the stroke falls
    /// through).
    pub fn apply_edit(&mut self, command: EditCommand) -> bool {
        // A chip deletes as one unit (the extmark's atomic range).
        match command {
            EditCommand::Backspace => {
                if let Some(range) = self.chip_before_caret() {
                    self.delete_range(range);
                    return true;
                }
            }
            EditCommand::Delete => {
                if let Some(range) = self.chip_after_caret() {
                    self.delete_range(range);
                    return true;
                }
            }
            EditCommand::MoveUp => return matches!(self.move_up(), KeyOutcome::Edited),
            EditCommand::MoveDown => return matches!(self.move_down(), KeyOutcome::Edited),
            _ => {}
        }
        !matches!(self.run_tracked(|area| area.apply_tracked(command), true), EditOutcome::Rejected)
    }

    /// The mention chip the caret sits at the END of (backspace
    /// deletes it atomically). RD-06: paste chips delete the same way
    /// (the extmark's atomic range).
    fn chip_before_caret(&self) -> Option<std::ops::Range<usize>> {
        let caret = self.edit.caret();
        if let Some(file) =
            self.draft.files.iter().find(|file| caret <= file.end && caret > file.start)
        {
            return Some(file.start..file.end);
        }
        if let Some(part) =
            self.draft.pasted.iter().find(|part| caret <= part.end && caret > part.start)
        {
            return Some(part.start..part.end);
        }
        // RD-05: `@agent`/`@skill` chips delete as one unit too.
        if let Some(part) =
            self.draft.agents.iter().find(|part| caret <= part.end && caret > part.start)
        {
            return Some(part.start..part.end);
        }
        self.draft
            .skills
            .iter()
            .find(|part| caret <= part.end && caret > part.start)
            .map(|part| part.start..part.end)
    }

    /// The mention chip the caret sits at the START of (delete
    /// forward). RD-06: paste chips included.
    fn chip_after_caret(&self) -> Option<std::ops::Range<usize>> {
        let caret = self.edit.caret();
        if let Some(file) =
            self.draft.files.iter().find(|file| caret >= file.start && caret < file.end)
        {
            return Some(file.start..file.end);
        }
        if let Some(part) =
            self.draft.pasted.iter().find(|part| caret >= part.start && caret < part.end)
        {
            return Some(part.start..part.end);
        }
        // RD-05: `@agent`/`@skill` chips delete as one unit too.
        if let Some(part) =
            self.draft.agents.iter().find(|part| caret >= part.start && caret < part.end)
        {
            return Some(part.start..part.end);
        }
        self.draft
            .skills
            .iter()
            .find(|part| caret >= part.start && caret < part.end)
            .map(|part| part.start..part.end)
    }

    /// Insert text at the caret, keeping file mentions' ranges valid.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let _ = self.run_tracked(|area| area.insert_tracked(text), false);
    }

    /// Replace the whole prompt (v2's `promptRef.set({ text })`: the
    /// edit state resets to a fresh area carrying the text — caret at
    /// the end, no undo history from the previous draft).
    pub fn set_text(&mut self, text: &str) {
        self.edit = Self::fresh_edit();
        self.draft = PromptDraft::default();
        self.files_undo.clear();
        self.files_redo.clear();
        self.pasted_undo.clear();
        self.pasted_redo.clear();
        self.agents_undo.clear();
        self.agents_redo.clear();
        self.skills_undo.clear();
        self.skills_redo.clear();
        if !text.is_empty() {
            self.insert_text(text);
        }
    }

    /// `input.newline` (shift/ctrl/alt+return, ctrl+j).
    pub fn newline(&mut self) {
        self.insert_text("\n");
    }

    /// `input.backspace` — deletes the cluster before the caret and
    /// any mention it sits inside.
    pub fn backspace(&mut self) {
        self.apply_edit(EditCommand::Backspace);
    }

    /// `input.delete` — the cluster after the caret.
    pub fn delete_forward(&mut self) {
        self.apply_edit(EditCommand::Delete);
    }

    /// Delete a char range directly (the chip paths).
    pub fn delete_range(&mut self, range: std::ops::Range<usize>) {
        let _ = self.run_tracked(|area| area.delete_range_tracked(range), false);
    }

    /// `input.line.home` / `input.line.end`.
    pub fn line_home(&mut self) {
        self.apply_edit(EditCommand::LineHome);
    }

    pub fn line_end(&mut self) {
        self.apply_edit(EditCommand::LineEnd);
    }

    fn line_start_offset(&self) -> usize {
        let text = &self.draft.text;
        let byte = char_to_byte(text, self.edit.caret().min(text.chars().count()));
        let start = text[..byte].rfind('\n').map_or(0, |pos| pos + 1);
        text[..start].chars().count()
    }

    /// `input.delete.to.line.end` (ctrl+k).
    pub fn delete_to_line_end(&mut self) {
        self.apply_edit(EditCommand::DeleteToLineEnd);
    }

    /// `input.delete.to.line.start` (ctrl+u).
    pub fn delete_to_line_start(&mut self) {
        self.apply_edit(EditCommand::DeleteToLineStart);
    }

    /// `input.undo` / `input.redo` (the editing core's stacks).
    pub fn undo(&mut self) -> bool {
        self.apply_edit(EditCommand::Undo)
    }

    pub fn redo(&mut self) -> bool {
        self.apply_edit(EditCommand::Redo)
    }

    /// RD-16 — `clearPrompt`'s retention gate (prompt/index.tsx:100,
    /// 1513-1526): a draft is worth keeping when the trimmed text
    /// reaches `DRAFT_RETENTION_MIN_CHARS` OR any pasted/file/agent
    /// part rides along — such drafts are appended to the input
    /// history before the clear so ↑ recalls them. (v2 checks
    /// `pasted`/`files`/`agents` only; skills don't gate.)
    pub const DRAFT_RETENTION_MIN_CHARS: usize = 20;

    #[must_use]
    fn clear_retains_draft(draft: &PromptDraft) -> bool {
        draft.text.trim().chars().count() >= Self::DRAFT_RETENTION_MIN_CHARS
            || !draft.pasted.is_empty()
            || !draft.files.is_empty()
            || !draft.agents.is_empty()
    }

    /// `prompt.clear` (ctrl+c in the composer) — clears the draft.
    /// RD-16 (prompt/index.tsx:1513-1526 `clearPrompt`): before the
    /// reset, a retaining draft (long text or any pasted/file/agent
    /// part) is appended to the input history so ↑ recalls it.
    pub fn clear(&mut self) {
        if Self::clear_retains_draft(&self.draft) {
            self.history = self.history.clone().push(&self.draft.text);
        }
        self.draft = PromptDraft::default();
        self.edit.set_text("");
        self.files_undo.clear();
        self.files_redo.clear();
        self.pasted_undo.clear();
        self.pasted_redo.clear();
        self.agents_undo.clear();
        self.agents_redo.clear();
        self.skills_undo.clear();
        self.skills_redo.clear();
        self.autocomplete.close();
    }

    /// Paste: normalize CRLF/CR to LF at the boundary (`onPaste`).
    pub fn paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_text(&normalized);
    }

    /// RD-06 — the fold decision (`prompt/index.tsx:1452-1465`): the
    /// TRIMMED paste folds into a chip when it spans ≥3 lines OR
    /// exceeds 150 characters (`Pasted ~N lines` counts LINES of the
    /// trimmed content — newline count + 1). The `prompt.paste ===
    /// "full"` config gate is the caller's (`compact` = fold armed).
    #[must_use]
    pub fn paste_should_summarize(text: &str) -> bool {
        let trimmed = text.trim();
        let line_count = trimmed.matches('\n').count() + 1;
        line_count >= PASTE_SUMMARY_MIN_LINES || trimmed.chars().count() > PASTE_SUMMARY_MAX_CHARS
    }

    /// The chip label a folding paste inserts (`[Pasted ~N lines]`).
    #[must_use]
    pub fn paste_summary_label(text: &str) -> String {
        let trimmed = text.trim();
        let line_count = trimmed.matches('\n').count() + 1;
        format!("[Pasted ~{line_count} lines]")
    }

    /// `pasteInputText`'s plain-text arm (prompt/index.tsx:1428-1469,
    /// RD-06): normalize, then either fold into the tracked
    /// `[Pasted ~N lines]` chip (thresholds + the `prompt.paste`
    /// config gate — `compact`) or insert the normalized text.
    pub fn paste_input_text(&mut self, text: &str, compact: bool) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if compact && Self::paste_should_summarize(&normalized) {
            let label = Self::paste_summary_label(&normalized);
            let content = normalized.trim().to_owned();
            self.paste_summary_chip(content, &label);
            return;
        }
        self.insert_text(&normalized);
    }

    /// `pasteText(content, virtualText)` (prompt/index.tsx:1394-1421,
    /// RD-06): the chip label inserts at the caret as a tracked
    /// virtual chip + trailing space; the full content never enters
    /// the buffer — it rides in `draft.pasted` until an expand chain
    /// restores it.
    fn paste_summary_chip(&mut self, content: String, label: &str) {
        let start = self.edit.caret();
        self.insert_text(&format!("{label} "));
        let end = start + label.chars().count();
        self.draft.pasted.push(PastedPart { text: content, start, end });
    }

    /// `expandPastedText(extmarkId)` (prompt/index.tsx:1424-1437,
    /// RD-06): one chip expands in place — the label range is
    /// replaced by the full content (the other chips' ranges shift
    /// with the delta via the tracked insert).
    pub fn expand_pasted(&mut self, index: usize) -> bool {
        let Some(part) = self.draft.pasted.get(index).cloned() else {
            return false;
        };
        if part.start >= part.end || part.end > self.char_len() {
            return false;
        }
        self.draft.pasted.remove(index);
        self.delete_range(part.start..part.end);
        self.set_caret(part.start);
        self.insert_text(&part.text);
        true
    }

    /// The click/caret chain (`onMouseDown`, prompt/index.tsx:
    /// 1813-1827, RD-06): expand the chip AT (or bordering) the
    /// offset — `getAtOffset(cursorOffset)` finds the pasted extmark.
    pub fn expand_pasted_at(&mut self, offset: usize) -> bool {
        let Some(index) =
            self.draft.pasted.iter().position(|part| offset >= part.start && offset <= part.end)
        else {
            return false;
        };
        self.expand_pasted(index)
    }

    /// `expandPastedTextPlaceholders` (prompt/part.ts:3-8, RD-06):
    /// every chip's label occurrence in `text` swaps for its full
    /// content — the clipboard read-back seam (`getClipboardText`,
    /// prompt/index.tsx:1799-1801).
    #[must_use]
    pub fn expand_pasted_placeholders(&self, text: &str) -> String {
        let chars: Vec<char> = self.draft.text.chars().collect();
        let mut result = text.to_owned();
        for part in &self.draft.pasted {
            if part.end <= part.start || part.end > chars.len() {
                continue;
            }
            let label: String = chars[part.start..part.end].iter().collect();
            if !label.is_empty() {
                result = result.replace(&label, &part.text);
            }
        }
        result
    }

    /// `prompt.paste` — the ctrl+v COMMAND (D-05 object-binding
    /// semantics). The keymap binds ctrl+v with `preventDefault:
    /// false` and leaves suppression to this handler: with a key
    /// event the stroke is consumed here (the terminal default and
    /// every lower layer skipped); without one (programmatic
    /// dispatch) nothing is prevented and the command still runs.
    /// The clipboard READ seam is [`crate::clipboard::ClipboardSource`]
    /// (G-03: opentui-core's clipboard service behind it) — app.rs's
    /// `prompt.paste` arm drives `paste_attachment`/`paste_resolved`;
    /// bracketed-paste text still lands through [`Self::paste`] (the
    /// `onPaste` path, prompt/index.tsx:1786-1802).
    pub fn prompt_paste(&mut self, from_event: bool) -> KeyOutcome {
        KeyOutcome::Paste(if from_event {
            PasteDisposition::Prevented
        } else {
            PasteDisposition::NoEvent
        })
    }

    /// `pasteAttachment` (prompt/index.tsx:1477): stage one data-URI
    /// attachment — the `[Image N]`/`[PDF N]` label (numbered past the
    /// staged mentions, reused for identical payloads) inserts at the
    /// caret as an atomic chip followed by a space.
    pub fn paste_attachment(&mut self, filename: Option<&str>, uri: &str) {
        let payload = AttachmentPayload::from_data_uri(uri);
        let label = attachment::prompt_attachment_label(&self.draft.files, &payload, filename);
        let start = self.edit.caret();
        self.insert_text(&format!("{label} "));
        let end = start + label.chars().count();
        self.draft.files.push(FileMention {
            name: filename.unwrap_or("file").to_owned(),
            start,
            end,
            payload,
            mention: Some(label),
        });
    }

    /// `pasteInputText`'s attachment arm (prompt/index.tsx:1437): a
    /// resolved paste stages its attachments (SVG text inserts as
    /// text) instead of the pasted path; plain text inserts verbatim.
    /// RD-06: the plain-text arm runs the fold decision (`compact` =
    /// `prompt.paste !== "full"`). Returns whether any attachment
    /// staged.
    pub fn paste_resolved(
        &mut self,
        text: &str,
        platform: &str,
        files: &dyn attachment::LocalFiles,
        compact: bool,
    ) -> bool {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let pasted = normalized.trim();
        if let Some(attachments) = attachment::resolve_pasted_attachments(pasted, platform, files) {
            for item in attachments {
                match item {
                    attachment::PastedAttachment::Text { filename, content } => {
                        self.paste(&content);
                        let _ = filename; // the `[SVG: name]` extmark stays C2
                    }
                    attachment::PastedAttachment::File { filename, uri } => {
                        self.paste_attachment(Some(&filename), &uri);
                    }
                }
            }
            return true;
        }
        self.paste_input_text(&normalized, compact);
        false
    }

    /// Left/right by one cluster (the editing core's cluster moves).
    pub fn move_left(&mut self) {
        self.apply_edit(EditCommand::MoveLeft);
    }

    pub fn move_right(&mut self) {
        self.apply_edit(EditCommand::MoveRight);
    }

    /// Up/down per v2: the autocomplete layer first (while open), then
    /// the textarea's row move, with the **history** commands winning
    /// at the first/last logical row (`up`/`down` at the edges).
    pub fn move_up(&mut self) -> KeyOutcome {
        if self.autocomplete.visible.is_some() {
            self.autocomplete.move_selection(-1);
            return KeyOutcome::Edited;
        }
        if self.line_start_offset() == 0 {
            return self.history_previous();
        }
        // Row move: to the previous line's end (logical rows; the
        // visual-row move is C2 caret internals). Direct to the area —
        // `apply_edit(MoveUp)` routes HERE (the composer owns the
        // history/autocomplete interplay).
        let _ = self.run_tracked(|area| area.apply_tracked(EditCommand::MoveUp), false);
        KeyOutcome::Edited
    }

    pub fn move_down(&mut self) -> KeyOutcome {
        if self.autocomplete.visible.is_some() {
            self.autocomplete.move_selection(1);
            return KeyOutcome::Edited;
        }
        let text = self.draft.text.clone();
        let byte = char_to_byte(&text, self.edit.caret());
        if text[byte..].find('\n').is_none() {
            return self.history_next();
        }
        let _ = self.run_tracked(|area| area.apply_tracked(EditCommand::MoveDown), false);
        KeyOutcome::Edited
    }

    /// `prompt.history.previous` — swap the draft for the older entry.
    pub fn history_previous(&mut self) -> KeyOutcome {
        let (history, entry) = self.history.clone().previous(&self.draft.text);
        if let Some(entry) = entry {
            self.history = history;
            self.draft.text = entry;
            self.set_caret(self.char_len());
            return KeyOutcome::Edited;
        }
        KeyOutcome::Ignored
    }

    /// `prompt.history.next`.
    pub fn history_next(&mut self) -> KeyOutcome {
        let (history, entry) = self.history.clone().next(&self.draft.text);
        self.history = history;
        self.draft.text = entry;
        self.set_caret(self.char_len());
        KeyOutcome::Edited
    }

    /// `input.submit` — returns the prompt (the store records history
    /// and forwards to the engine stub) and clears the draft.
    #[must_use]
    pub fn take_submit(&mut self) -> Option<SubmittedPrompt> {
        if self.draft.is_empty() {
            return None;
        }
        let prompt = SubmittedPrompt {
            // RD-06 — submit restores the full pasted text into the
            // prompt body (`expandTrackedPastedText`, prompt/index.
            // tsx:1123-1132): every chip's range swaps for its
            // content, back-to-front, so the submitted text reads as
            // if the paste never folded.
            text: self.draft.expand_pasted_ranges(),
            files: std::mem::take(&mut self.draft.files),
            agents: std::mem::take(&mut self.draft.agents),
            // RD-05 — the `@skill` chips ride the submit as the wire
            // `skills` array (index.tsx:1362).
            skills: std::mem::take(&mut self.draft.skills),
            ..SubmittedPrompt::default()
        };
        self.draft.text.clear();
        self.draft.pasted.clear();
        self.edit.set_text("");
        self.files_undo.clear();
        self.files_redo.clear();
        self.pasted_undo.clear();
        self.pasted_redo.clear();
        self.agents_undo.clear();
        self.agents_redo.clear();
        self.skills_undo.clear();
        self.skills_redo.clear();
        self.autocomplete.close();
        Some(prompt)
    }

    /// `Ask anything… "{entry}"` — the placeholder for the selected
    /// list entry (`placeholderText`). An EMPTY list renders no
    /// placeholder at all (the reference returns `undefined` when
    /// `list().length === 0` — the session route mounts `Prompt`
    /// without placeholders), so the empty-list form yields "".
    /// TUI-14: shell mode swaps the prefix (`Run a command…`,
    /// index.tsx:1595-1600).
    #[must_use]
    pub fn placeholder_text(&self) -> String {
        if self.placeholders.is_empty() {
            return String::new();
        }
        let entry = self
            .placeholders
            .get(self.placeholder_index % self.placeholders.len())
            .cloned()
            .unwrap_or_default();
        let prefix = match self.mode {
            ComposerMode::Normal => "Ask anything",
            ComposerMode::Shell => "Run a command",
        };
        format!("{prefix}… \"{entry}\"")
    }

    /// The pump's key dispatch: printable chars feed the textarea;
    /// the rest resolve through the v2 table's composer slice.
    #[allow(clippy::too_many_lines)] // one arm per key, the composer's dispatch table
    pub fn handle_key(&mut self, key: &ComposerKey) -> KeyOutcome {
        if self.autocomplete.visible.is_some() {
            match key {
                ComposerKey::Escape => {
                    self.autocomplete.dismiss();
                    return KeyOutcome::Edited;
                }
                // TUI-28 — `prompt.autocomplete.destructive`
                // (autocomplete.tsx:728-731, bind ctrl+d): the
                // two-press delete over the DIRECTORY rows. The
                // removal itself is the caller's (the recents live in
                // the store); the second press records the request
                // here.
                ComposerKey::CtrlD
                    if matches!(self.autocomplete.visible, Some(autocomplete::Mode::Directory)) =>
                {
                    if let Some(id) = self.autocomplete.trigger_destructive() {
                        self.destructive_requests.push(id);
                    }
                    return KeyOutcome::Edited;
                }
                ComposerKey::Enter | ComposerKey::Tab => {
                    // The autocomplete layer owns return/tab while
                    // visible (`prompt.autocomplete.select` — v2's
                    // layer shadows prompt.submit, and an EMPTY popup
                    // swallows the key: `select()` no-ops and
                    // `submitInner` bails on `auto()?.visible`).
                    let outcome = self.accept_autocomplete();
                    return if outcome == KeyOutcome::Ignored {
                        KeyOutcome::Edited
                    } else {
                        outcome
                    };
                }
                _ => {}
            }
        }
        // TUI-14 — the shell-mode layers (prompt/index.tsx:985-1018).
        if self.mode == ComposerMode::Shell {
            if let Some(outcome) = self.shell_mode_layer(key) {
                return outcome;
            }
        }
        match key {
            ComposerKey::Char(ch) => {
                // TUI-14 — the `!` entry layer (index.tsx:961-981,
                // `bind: "!"`): caret 0, normal mode, no popup — the
                // stroke NEVER inserts; the mode flips and the
                // placeholder slot re-rolls (`randomIndex`).
                if self.bang_enters_shell(*ch) {
                    return KeyOutcome::Edited;
                }
                self.insert_text(&ch.to_string());
                self.autocomplete.on_input(&self.draft.text, self.edit.caret());
                KeyOutcome::Edited
            }
            ComposerKey::Enter => match self.take_submit() {
                Some(prompt) => {
                    self.history = self.history.clone().push(&prompt.text);
                    KeyOutcome::Submitted(prompt)
                }
                None => KeyOutcome::Ignored,
            },
            ComposerKey::ShiftEnter
            | ComposerKey::CtrlEnter
            | ComposerKey::AltEnter
            | ComposerKey::CtrlJ => {
                self.newline();
                KeyOutcome::Edited
            }
            ComposerKey::Up => self.move_up(),
            ComposerKey::Down => self.move_down(),
            ComposerKey::Left => {
                self.move_left();
                KeyOutcome::Edited
            }
            ComposerKey::Right => {
                self.move_right();
                KeyOutcome::Edited
            }
            // Wave A: shift+arrows extend the selection
            // (`moveCursor*({ select: true })`).
            ComposerKey::ShiftLeft => self.select_command(EditCommand::SelectLeft),
            ComposerKey::ShiftRight => self.select_command(EditCommand::SelectRight),
            ComposerKey::ShiftUp => self.select_command(EditCommand::SelectUp),
            ComposerKey::ShiftDown => self.select_command(EditCommand::SelectDown),
            ComposerKey::Home | ComposerKey::CtrlA => {
                self.line_home();
                KeyOutcome::Edited
            }
            ComposerKey::End | ComposerKey::CtrlE => {
                self.line_end();
                KeyOutcome::Edited
            }
            ComposerKey::Backspace => {
                self.backspace();
                self.autocomplete.on_input(&self.draft.text, self.edit.caret());
                KeyOutcome::Edited
            }
            ComposerKey::Delete => {
                self.delete_forward();
                self.autocomplete.on_input(&self.draft.text, self.edit.caret());
                KeyOutcome::Edited
            }
            ComposerKey::CtrlK => {
                self.delete_to_line_end();
                KeyOutcome::Edited
            }
            ComposerKey::CtrlU => {
                self.delete_to_line_start();
                KeyOutcome::Edited
            }
            ComposerKey::CtrlC => {
                self.clear();
                KeyOutcome::Edited
            }
            // `prompt.paste`: the ctrl+v stroke resolves as the paste
            // COMMAND with an event — suppressed at the handler (the
            // ^V byte never inserts).
            ComposerKey::CtrlV => self.prompt_paste(true),
            ComposerKey::Escape => {
                self.autocomplete.close();
                KeyOutcome::Ignored
            }
            // TUI-28 — the destructive confirm only lives in the
            // autocomplete layer (handled above); bare ctrl+d (like
            // tab's fallthrough) is not a composer command — the base
            // layer's app.exit owns it.
            ComposerKey::Tab | ComposerKey::CtrlD => KeyOutcome::Ignored,
        }
    }

    /// TUI-14 — the `!` entry layer's gate (`bind: "!"`,
    /// index.tsx:961-981): caret 0 on an empty-headed normal-mode
    /// draft with no popup — the stroke flips to shell mode (never
    /// inserting) and re-rolls the placeholder slot.
    fn bang_enters_shell(&mut self, ch: char) -> bool {
        if ch != '!'
            || self.mode != ComposerMode::Normal
            || self.autocomplete.visible.is_some()
            || self.edit.caret() != 0
        {
            return false;
        }
        self.mode = ComposerMode::Shell;
        self.placeholder_index = self.placeholder_index.wrapping_add(1);
        true
    }

    /// TUI-14 — the shell-mode key layers (prompt/index.tsx:985-1018):
    /// `escape` and empty-draft `ctrl+c` exit the mode (priority 1),
    /// `backspace` at caret 0 exits (the third layer), and Enter
    /// submits through `session.shell` — the mode resets to normal
    /// after the send (`setStore("mode", "normal")`, index.tsx:1307).
    /// An EMPTY shell submit is a no-op (`!trimmed && store.mode ===
    /// "shell"` bails first, index.tsx:1120-1122). `None` = the key
    /// falls through to the normal table.
    fn shell_mode_layer(&mut self, key: &ComposerKey) -> Option<KeyOutcome> {
        match key {
            ComposerKey::Escape => {
                self.mode = ComposerMode::Normal;
                Some(KeyOutcome::Edited)
            }
            ComposerKey::CtrlC if self.draft.is_empty() => {
                self.mode = ComposerMode::Normal;
                Some(KeyOutcome::Edited)
            }
            ComposerKey::Backspace if self.edit.caret() == 0 => {
                self.mode = ComposerMode::Normal;
                Some(KeyOutcome::Edited)
            }
            ComposerKey::Enter => match self.take_submit() {
                Some(prompt) => {
                    self.history = self.history.clone().push(&prompt.text);
                    self.mode = ComposerMode::Normal;
                    Some(KeyOutcome::ShellCommand(prompt.text))
                }
                // An EMPTY shell submit is a consumed no-op (v2's
                // `!trimmed && store.mode === "shell"` bail — never
                // the empty-submit steer).
                None => Some(KeyOutcome::Edited),
            },
            _ => None,
        }
    }

    /// RD-05 — replace the `@query` trigger token with the virtual
    /// chip `@<name>` (autocomplete.tsx:160-190): the token range
    /// (trigger index through the caret) deletes first, the chip (+ a
    /// space unless the following char already is one) inserts, and
    /// the chip's char range is returned for the structured part's
    /// mention bookkeeping.
    fn insert_reference_chip(&mut self, name: &str) -> (usize, usize) {
        let caret = self.edit.caret();
        let index = self.autocomplete.trigger_index;
        let chars: Vec<char> = self.draft.text.chars().collect();
        let needs_space = chars.get(caret).is_none_or(|ch| *ch != ' ');
        if index < caret {
            self.delete_range(index..caret);
        }
        let chip = format!("@{name}");
        let end = index + chip.chars().count();
        let insert = if needs_space { format!("{chip} ") } else { chip };
        self.insert_text(&insert);
        (index, end)
    }

    /// Accept the selected autocomplete option (select/complete).
    ///
    /// Command mode follows v2's `select()` (`autocomplete.tsx:
    /// 615-626`): `hide(true)` removes the `/name` trigger token from
    /// the draft, then `onSelect` runs — `command.run` for built-ins
    /// without arguments (`autocomplete.tsx:481`), `insertSlash` only
    /// for argument-taking commands (`/cd`). A run reports
    /// [`KeyOutcome::Command`]; the rest of the draft survives both
    /// paths exactly like the reference.
    pub fn accept_autocomplete(&mut self) -> KeyOutcome {
        let Some(option) = self.autocomplete.selected_option().cloned() else {
            return KeyOutcome::Ignored;
        };
        match self.autocomplete.visible {
            Some(autocomplete::Mode::Command) => {
                // `hide(true)`: the trigger token is removed first.
                let index = self.autocomplete.trigger_index;
                self.autocomplete.close();
                let end = self.edit.caret();
                if index <= end {
                    self.delete_range(index..end);
                }
                // `onSelect: command.run` — the built-in executes
                // locally; nothing is submitted to the model.
                // `slash.arguments` (only /cd) inserts `/name `.
                if let Some(command) = autocomplete::SlashRegistry::command_for(&option.display) {
                    KeyOutcome::Command(command)
                } else {
                    let name = option.display.trim_start_matches('/');
                    self.insert_text(&format!("/{name} "));
                    KeyOutcome::Edited
                }
            }
            Some(autocomplete::Mode::Reference) => {
                self.autocomplete.close();
                // RD-05 — the `@agent`/`@skill` rows record their
                // structured parts (autocomplete.tsx:160-238): the
                // trigger token (`@query`) is replaced by the virtual
                // chip `@<id>`, and the part tracks the chip's char
                // range so edits/undo keep it consistent. Rows without
                // a structured identity (references/files — no live
                // source in the port) insert as plain text.
                match option.reference {
                    Some(autocomplete::Reference::Agent { name }) => {
                        let (start, end) = self.insert_reference_chip(&name);
                        // The ref pushes agents unconditionally (no
                        // dedupe — autocomplete.tsx:230-238).
                        self.draft.agents.push(AgentMention { name, start, end });
                        KeyOutcome::Edited
                    }
                    Some(autocomplete::Reference::Skill { id }) => {
                        // `hasSkill` (autocomplete.tsx:160): a skill
                        // already attached is a no-op.
                        if self.draft.skills.iter().any(|skill| skill.id == id) {
                            return KeyOutcome::Edited;
                        }
                        let (start, end) = self.insert_reference_chip(&id);
                        self.draft.skills.push(SkillMention { name: id.clone(), id, start, end });
                        KeyOutcome::Edited
                    }
                    None => {
                        // RE-03 — `insertPart` records the file part
                        // (autocomplete.tsx:241: `part.type === "file"
                        // && part.path → updateFrecency(path)`): the
                        // file/reference rows (no structured identity)
                        // bump the frecency store on accept.
                        if let Some(path) = option.path.clone() {
                            self.autocomplete.frecency.update(&path, autocomplete::now_ms());
                        }
                        let name = option.display.trim_start_matches('@');
                        self.insert_text(&format!("@{name} "));
                        KeyOutcome::Edited
                    }
                }
            }
            // TUI-28 — the directory rows' `select()` (autocomplete.
            // tsx:620-627): the recent rows carry NO `onSelect`, so
            // Enter only dismisses (the dismissed value suppresses
            // the reopen); the INSERT path is tab (`complete` →
            // `expandDirectory` → `insertDirectory(value + "/")`).
            Some(autocomplete::Mode::Directory) => {
                self.autocomplete.dismiss();
                KeyOutcome::Edited
            }
            None => KeyOutcome::Ignored,
        }
    }
}

/// Byte offset of a char index.
pub(crate) fn char_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(byte, _)| byte)
}

/// The engine prompt API stub: what `store::Store::submit_prompt`
/// forwards submissions to (the real `session.prompt` oc-client call
/// is wave 3's TODO(7.3)).
pub trait PromptSink {
    fn send(&self, session_id: &str, prompt: &SubmittedPrompt);

    /// Submit with the RESOLVED model selection (v2's `commitModel`,
    /// prompt/index.tsx:1283-1294: production awaits
    /// `session.switchModel` BEFORE the prompt POST so the turn runs
    /// on the committed model; sinks that don't model-gate delegate to
    /// [`Self::send`]).
    fn send_with_model(
        &self,
        session_id: &str,
        prompt: &SubmittedPrompt,
        _model: Option<&crate::store::ModelSelection>,
    ) {
        self.send(session_id, prompt);
    }

    /// TUI-14 — the shell-mode submit (`session.shell`, POST
    /// /api/session/:id/shell with `{id, command}`,
    /// prompt/index.tsx:1303-1307): the text runs as a shell command,
    /// never a model prompt. The default no-op keeps test sinks
    /// quiet; production posts through the client.
    fn send_shell(&self, session_id: &str, command: &str) {
        let _ = (session_id, command);
    }
}

/// The default sink: records submissions in order (tests + the replay
/// harness assert against it).
#[derive(Debug, Default)]
pub struct RecordingPromptSink {
    pub submissions: std::cell::RefCell<Vec<(String, SubmittedPrompt)>>,
    /// TUI-14 — the shell-command submits in order.
    pub shell_commands: std::cell::RefCell<Vec<(String, String)>>,
}

impl PromptSink for RecordingPromptSink {
    fn send(&self, session_id: &str, prompt: &SubmittedPrompt) {
        self.submissions.borrow_mut().push((session_id.to_owned(), prompt.clone()));
    }

    fn send_shell(&self, session_id: &str, command: &str) {
        self.shell_commands.borrow_mut().push((session_id.to_owned(), command.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composer(text: &str) -> Composer {
        let mut state = Composer::default();
        state.set(PromptDraft { text: text.to_owned(), ..PromptDraft::default() });
        state
    }

    #[test]
    fn multiline_editing_per_the_v2_table() {
        let mut state = Composer::default();
        for ch in "ab".chars() {
            state.handle_key(&ComposerKey::Char(ch));
        }
        // ctrl+j inserts a newline (input.newline).
        state.handle_key(&ComposerKey::CtrlJ);
        state.handle_key(&ComposerKey::Char('c'));
        assert_eq!(state.draft.text, "ab\nc");
        // shift/ctrl/alt+return all newline; return submits.
        state.handle_key(&ComposerKey::ShiftEnter);
        assert_eq!(state.draft.text, "ab\nc\n");
        state.handle_key(&ComposerKey::Enter);
        assert!(matches!(state.history.entries.last(), Some(text) if text == "ab\nc\n"));
        assert!(state.draft.text.is_empty(), "draft text cleared");
        // Empty submit is a no-op.
        assert_eq!(state.handle_key(&ComposerKey::Enter), KeyOutcome::Ignored);
    }

    #[test]
    fn up_down_at_boundaries_reach_history() {
        let mut state = Composer::default();
        state.history = state.history.clone().push("older");
        state.history = state.history.clone().push("oldest");
        state.set(PromptDraft { text: "draft".into(), ..PromptDraft::default() });
        // Up on the first line → history.previous.
        state.handle_key(&ComposerKey::Up);
        assert_eq!(state.draft.text, "oldest");
        state.handle_key(&ComposerKey::Up);
        assert_eq!(state.draft.text, "older");
        // Down walks forward and restores the stashed draft.
        state.handle_key(&ComposerKey::Down);
        assert_eq!(state.draft.text, "oldest");
        state.handle_key(&ComposerKey::Down);
        assert_eq!(state.draft.text, "draft");
        // Up/down INSIDE a multiline draft moves the caret, not history.
        let mut multi = composer("one\ntwo");
        multi.handle_key(&ComposerKey::Up);
        assert_eq!(multi.draft.text, "one\ntwo", "text untouched");
        assert!(multi.caret() <= 3, "caret moved to the first row");
    }

    #[test]
    fn paste_normalizes_line_endings() {
        let mut state = Composer::default();
        state.paste("a\r\nb\rc");
        assert_eq!(state.draft.text, "a\nb\nc");
    }

    /// RD-06 — the fold matrix (prompt/index.tsx:1452-1465): the
    /// TRIMMED paste folds at ≥3 lines OR >150 chars; `prompt.paste
    /// === "full"` (`compact: false`) inserts verbatim regardless.
    /// `~N` counts LINES of the trimmed content (newline count + 1).
    #[test]
    fn paste_summary_detection_matrix() {
        let fold = |text: &str| Composer::paste_should_summarize(text);
        // 2 lines, short: verbatim.
        assert!(!fold("one\ntwo"));
        // Exactly 3 lines (2 newlines): folds — "~3 lines".
        assert!(fold("one\ntwo\nthree"));
        // 150 chars, one line: verbatim (the ref is `> 150`, strict).
        assert!(!fold(&"x".repeat(150)));
        // 151 chars: folds — "~1 lines" (ref's raw line count).
        assert!(fold(&"x".repeat(151)));
        // Detection reads the TRIMMED content: the 3 newlines inside
        // count, the bounding whitespace does not.
        assert!(fold("\n  a\nb\nc  \n"));
        // CRLF normalizes BEFORE the thresholds (3 logical lines).
        let mut crlf = Composer::default();
        crlf.paste_input_text("a\r\nb\r\nc", true);
        assert!(crlf.draft.text.contains("[Pasted ~3 lines]"));

        // The label counts trimmed lines.
        assert_eq!(Composer::paste_summary_label("a\nb\nc"), "[Pasted ~3 lines]");
        assert_eq!(Composer::paste_summary_label(&"y".repeat(200)), "[Pasted ~1 lines]");

        // compact = false (`prompt.paste: "full"`): never folds.
        let mut full = Composer::default();
        full.paste_input_text("a\nb\nc\nd", false);
        assert_eq!(full.draft.text, "a\nb\nc\nd");
        assert_eq!(full.draft.pasted.len(), 0, "full paste never folds");

        // compact = true (the default gate): folds into the chip +
        // trailing space, tracking the trimmed content.
        let mut compact = Composer::default();
        compact.insert_text("head ");
        compact.paste_input_text("  a\nb\nc  ", true);
        assert_eq!(compact.draft.text, "head [Pasted ~3 lines] ");
        assert_eq!(compact.draft.pasted.len(), 1);
        let part = &compact.draft.pasted[0];
        assert_eq!(part.text, "a\nb\nc");
        assert_eq!(part.start, 5);
        assert_eq!(part.end, 5 + "[Pasted ~3 lines]".chars().count());
    }

    /// RD-06 — submit restores the folded pastes into the prompt body
    /// (`expandTrackedPastedText`, prompt/index.tsx:1123-1132): the
    /// chip ranges swap for their content back-to-front.
    #[test]
    fn submit_expands_folded_pastes() {
        let mut state = Composer::default();
        state.insert_text("before ");
        state.paste_input_text("l1\nl2\nl3", true);
        state.insert_text("mid ");
        state.paste_input_text("x".repeat(160).as_str(), true);
        let text = state.draft.text.clone();
        assert_eq!(text, "before [Pasted ~3 lines] mid [Pasted ~1 lines] ");
        let outcome = state.handle_key(&ComposerKey::Enter);
        let KeyOutcome::Submitted(prompt) = outcome else {
            panic!("expected a submit, got {outcome:?}");
        };
        assert_eq!(prompt.text, format!("before l1\nl2\nl3 mid {} ", "x".repeat(160)));
        // The draft clears wholesale after the submit.
        assert_eq!(state.draft.text, "");
        assert_eq!(state.draft.pasted.len(), 0);
    }

    /// RD-06 — the expand chains: `expand_pasted` (click/`expand-
    /// PastedText`) replaces the chip in place and shifts the other
    /// chip's range; `expand_pasted_placeholders` (the clipboard
    /// read-back) swaps label occurrences for the full content.
    #[test]
    fn paste_chip_expand_and_clipback_chains() {
        let mut state = Composer::default();
        state.insert_text("a ");
        state.paste_input_text("l1\nl2\nl3", true);
        state.insert_text("b ");
        state.paste_input_text("m1\nm2\nm3", true);
        assert_eq!(state.draft.text, "a [Pasted ~3 lines] b [Pasted ~3 lines] ");
        // Expand the FIRST chip: the second chip's range shifts by the
        // delta (5 chars label → 8 chars content).
        assert!(state.expand_pasted(0));
        assert_eq!(state.draft.text, "a l1\nl2\nl3 b [Pasted ~3 lines] ");
        assert_eq!(state.draft.pasted.len(), 1);
        assert_eq!(
            (state.draft.pasted[0].start, state.draft.pasted[0].end),
            (13, 13 + "[Pasted ~3 lines]".chars().count())
        );
        // The caret-offset hit (the mousedown chain's getAtOffset):
        // inside the surviving chip expands it, an outside offset
        // does not.
        assert!(state.expand_pasted_at(state.draft.pasted[0].start + 2));
        assert_eq!(state.draft.text, "a l1\nl2\nl3 b m1\nm2\nm3 ");
        assert_eq!(state.draft.pasted.len(), 0);
        assert!(!state.expand_pasted_at(0));

        // The clipboard read-back: labels swap for content anywhere
        // in the copied span.
        let mut copy = Composer::default();
        copy.paste_input_text("p1\np2\np3", true);
        assert_eq!(
            copy.expand_pasted_placeholders("see [Pasted ~3 lines] here"),
            "see p1\np2\np3 here"
        );

        // Backspace deletes the chip atomically (the extmark range)
        // and drops the tracked part.
        let mut del = Composer::default();
        del.paste_input_text("q1\nq2\nq3", true);
        del.handle_key(&ComposerKey::Backspace); // the trailing space
        del.handle_key(&ComposerKey::Backspace); // the whole chip
        assert_eq!(del.draft.text, "");
        assert_eq!(del.draft.pasted.len(), 0);
    }

    /// D-05 — `input_paste` preventDefault object semantics: the
    /// ctrl+v STROKE resolves as the paste command WITH an event —
    /// suppressed at the handler (nothing inserts; the ^V byte never
    /// reaches the draft), while a programmatic dispatch carries no
    /// event to prevent. Both run the same command.
    #[test]
    fn prompt_paste_prevents_the_key_event_but_not_a_dispatch() {
        let mut state = Composer::default();
        state.set(PromptDraft { text: "draft".into(), ..PromptDraft::default() });
        // The stroke (handle_key / the keymap layer handing the event
        // over): consumed + prevented.
        assert_eq!(
            state.handle_key(&ComposerKey::CtrlV),
            KeyOutcome::Paste(PasteDisposition::Prevented)
        );
        assert_eq!(state.draft.text, "draft", "suppressed — no ^V inserted");
        // The dispatch path (keymap.dispatch("prompt.paste") — the
        // empty-bracketed-paste arm): no event, no prevention.
        assert_eq!(state.prompt_paste(false), KeyOutcome::Paste(PasteDisposition::NoEvent));
        assert_eq!(state.draft.text, "draft");
        // Bracketed paste (the onPaste path) still inserts text.
        state.paste("pasted");
        assert_eq!(state.draft.text, "draftpasted");
    }

    #[test]
    fn paste_attachment_stages_a_labeled_chip_with_payload() {
        let mut state = Composer::default();
        state.paste_attachment(Some("shot.png"), "data:image/png;base64,AAECAw==");
        assert_eq!(state.draft.text, "[Image 1] ");
        assert_eq!(state.caret(), 10);
        assert_eq!(state.draft.files.len(), 1);
        let file = &state.draft.files[0];
        assert_eq!(file.mention_text(), "[Image 1]");
        assert_eq!(file.payload.as_ref().unwrap().mime, "image/png");
        assert_eq!(file.payload.as_ref().unwrap().data, "AAECAw==");
        assert_eq!(file.payload.as_ref().unwrap().to_data_uri(), "data:image/png;base64,AAECAw==");

        // A second, different image numbers past the first; the same
        // payload reuses its mention (promptAttachmentLabel).
        state.paste_attachment(Some("other.jpg"), "data:image/jpeg;base64,/9j/AA==");
        assert_eq!(state.draft.text, "[Image 1] [Image 2] ");
        state.paste_attachment(Some("shot.png"), "data:image/png;base64,AAECAw==");
        assert_eq!(state.draft.text, "[Image 1] [Image 2] [Image 1] ");
        // Backspace first removes the trailing space, then the chip
        // deletes atomically (the extmark range).
        state.handle_key(&ComposerKey::Backspace);
        state.handle_key(&ComposerKey::Backspace);
        assert_eq!(state.draft.text, "[Image 1] [Image 2] ");
        assert_eq!(state.draft.files.len(), 2);
    }

    #[test]
    fn paste_resolved_stages_attachments_and_falls_back_to_text() {
        use attachment::MapFiles;
        let files = MapFiles {
            files: [("/tmp/shot.png".to_owned(), b"\x89PNG".to_vec())].into_iter().collect(),
        };
        let mut state = Composer::default();
        assert!(state.paste_resolved("/tmp/shot.png", "linux", &files, true));
        assert_eq!(state.draft.text, "[Image 1] ");
        assert_eq!(state.draft.files.len(), 1);

        let plain =
            MapFiles { files: [("/tmp/a.txt".to_owned(), b"hi".to_vec())].into_iter().collect() };
        let mut text_state = Composer::default();
        assert!(!text_state.paste_resolved("just some words", "linux", &plain, true));
        assert_eq!(text_state.draft.text, "just some words");
    }

    #[test]
    fn chip_backspace_deletes_the_whole_mention() {
        let text = "Compare [Image 1] now";
        let start = text.find("[Image").expect("mention");
        let end = start + "[Image 1]".len();
        let mut state = Composer::default();
        state.set(PromptDraft {
            text: text.to_owned(),
            files: vec![FileMention {
                name: "screenshot.png".into(),
                start: text[..start].chars().count(),
                end: text[..end].chars().count(),
                payload: None,
                mention: Some("[Image 1]".into()),
            }],
            ..PromptDraft::default()
        });
        // Caret just after the chip.
        state.set_caret(state.draft.files[0].end);
        state.handle_key(&ComposerKey::Backspace);
        assert_eq!(state.draft.text, "Compare  now");
        assert!(state.draft.files.is_empty(), "chip removed with the mention");
    }

    #[test]
    fn clear_and_line_ops_follow_the_table() {
        let mut state = composer("hello world");
        state.set_caret(5);
        state.handle_key(&ComposerKey::CtrlK);
        assert_eq!(state.draft.text, "hello");
        state.handle_key(&ComposerKey::CtrlU);
        assert_eq!(state.draft.text, "");
        // ctrl+c clears.
        let mut state = composer("x");
        state.handle_key(&ComposerKey::CtrlC);
        assert!(state.draft.is_empty());
    }

    /// RD-16 — a LONG draft cleared by ctrl+c is appended to the
    /// input history (prompt/index.tsx:1513-1526): ↑ recalls it.
    #[test]
    fn clear_saves_long_draft_to_history() {
        let mut state = composer("this draft is definitely long enough to keep");
        state.handle_key(&ComposerKey::CtrlC);
        assert!(state.draft.is_empty());
        // ↑ recalls the cleared draft (caret 0 → history_previous).
        let outcome = state.handle_key(&ComposerKey::Up);
        assert!(matches!(outcome, KeyOutcome::Edited));
        assert_eq!(state.draft.text, "this draft is definitely long enough to keep");
    }

    /// RD-16 — a short bare draft clears WITHOUT a history append.
    #[test]
    fn clear_drops_short_bare_draft() {
        let mut state = composer("short one");
        state.handle_key(&ComposerKey::CtrlC);
        assert!(state.draft.is_empty());
        state.handle_key(&ComposerKey::Up);
        assert!(state.draft.is_empty(), "nothing to recall");
    }

    /// RD-16 — the part arms of the gate: a short text WITH a
    /// pasted/file/agent part still lands in the history (the ref's
    /// `pasted.length > 0 || files.length > 0 || agents.length > 0`).
    #[test]
    fn clear_saves_short_draft_carrying_parts() {
        let mut state = composer("see attached");
        state.draft.pasted.push(crate::components::composer::PastedPart {
            start: 4,
            end: 4,
            text: "the pasted body".into(),
        });
        state.handle_key(&ComposerKey::CtrlC);
        state.handle_key(&ComposerKey::Up);
        assert_eq!(state.draft.text, "see attached", "pasted part retains the draft");

        let mut state = composer("x");
        state.draft.files.push(FileMention {
            name: "a.png".into(),
            start: 1,
            end: 1,
            payload: None,
            mention: None,
        });
        state.handle_key(&ComposerKey::CtrlC);
        state.handle_key(&ComposerKey::Up);
        assert_eq!(state.draft.text, "x", "file part retains the draft");
    }

    #[test]
    fn placeholder_uses_the_selected_entry() {
        let mut state = Composer {
            placeholders: vec!["Fix a TODO".into(), "Other".into()],
            ..Composer::default()
        };
        assert_eq!(state.placeholder_text(), "Ask anything… \"Fix a TODO\"");
        state.placeholder_index = 1;
        assert_eq!(state.placeholder_text(), "Ask anything… \"Other\"");
    }

    /// RD-05 — accepting an `@agent` row records the structured part
    /// (autocomplete.tsx:230-238): the `@query` token is replaced by
    /// the virtual chip `@build `, and the part tracks the chip range.
    #[test]
    fn accepting_agent_completion_records_structured_part() {
        let mut state = Composer::default();
        state.set(PromptDraft { text: "hey @bu".into(), ..PromptDraft::default() });
        state.set_caret(state.char_len());
        // The popup opens on input; the app attaches the reference
        // rows (here: the agent row for `build`).
        state.autocomplete.on_input(&state.draft.text, state.edit.caret());
        assert!(state.autocomplete.visible.is_some());
        state.autocomplete.options = vec![autocomplete::AutocompleteOption {
            display: "@build".into(),
            reference: Some(autocomplete::Reference::Agent { name: "build".into() }),
            ..autocomplete::AutocompleteOption::default()
        }];
        state.autocomplete.filtered_len = 1;
        let outcome = state.accept_autocomplete();
        assert!(matches!(outcome, KeyOutcome::Edited));
        assert_eq!(state.draft.text, "hey @build ");
        let agent = state.draft.agents.first().expect("structured agent part");
        assert_eq!(agent.name, "build");
        // The chip range covers `@build` (not the trailing space).
        assert_eq!((agent.start, agent.end), (4, 10));
    }

    /// RD-05 — accepting an `@skill` row records the wire `skills`
    /// part; a skill already attached is a no-op (`hasSkill`,
    /// autocomplete.tsx:160).
    #[test]
    fn accepting_skill_completion_records_and_dedupes() {
        let mut state = Composer::default();
        state.set(PromptDraft { text: "@re".into(), ..PromptDraft::default() });
        state.set_caret(state.char_len());
        state.autocomplete.on_input(&state.draft.text, state.edit.caret());
        state.autocomplete.options = vec![autocomplete::AutocompleteOption {
            display: "@refactor".into(),
            reference: Some(autocomplete::Reference::Skill { id: "refactor".into() }),
            ..autocomplete::AutocompleteOption::default()
        }];
        state.autocomplete.filtered_len = 1;
        assert!(matches!(state.accept_autocomplete(), KeyOutcome::Edited));
        assert_eq!(state.draft.text, "@refactor ");
        let skill = state.draft.skills.first().expect("structured skill part");
        assert_eq!(skill.id, "refactor");
        assert_eq!((skill.start, skill.end), (0, 9));

        // Re-mentioning the same skill inserts the chip text but does
        // NOT double-attach the part (hasSkill skip).
        state.set(PromptDraft { text: "@re".into(), ..PromptDraft::default() });
        state.set_caret(state.char_len());
        state.autocomplete.on_input(&state.draft.text, state.edit.caret());
        state.autocomplete.options = vec![autocomplete::AutocompleteOption {
            display: "@refactor".into(),
            reference: Some(autocomplete::Reference::Skill { id: "refactor".into() }),
            ..autocomplete::AutocompleteOption::default()
        }];
        state.autocomplete.filtered_len = 1;
        assert!(matches!(state.accept_autocomplete(), KeyOutcome::Edited));
        assert_eq!(state.draft.skills.len(), 1, "skill parts dedupe");
    }

    /// RE-03 — accepting a FILE row (a path-carrying option, the
    /// ref's `insertPart` file-part record — autocomplete.tsx:241)
    /// bumps the frecency store: frequency+1 with a fresh lastOpen.
    #[test]
    fn accepting_file_completion_records_frecency() {
        let mut state = Composer::default();
        state.set(PromptDraft { text: "@sr".into(), ..PromptDraft::default() });
        state.set_caret(state.char_len());
        state.autocomplete.on_input(&state.draft.text, state.edit.caret());
        state.autocomplete.options = vec![autocomplete::AutocompleteOption {
            display: "@src/main.rs".into(),
            path: Some("src/main.rs".into()),
            ..autocomplete::AutocompleteOption::default()
        }];
        state.autocomplete.filtered_len = 1;
        assert!(matches!(state.accept_autocomplete(), KeyOutcome::Edited));
        // The unstructured row inserts as plain text after the token
        // (the port's insert path — no chip replacement).
        assert_eq!(state.draft.text, "@sr@src/main.rs ");
        let before = autocomplete::now_ms();
        assert_eq!(
            state.autocomplete.frecency.data.get("src/main.rs"),
            Some(&autocomplete::FrecencyEntry { frequency: 1, last_open: before }),
            "accept records frequency+1 at the wall clock"
        );

        // A second accept of the same file increments, not resets.
        state.set(PromptDraft { text: "@sr".into(), ..PromptDraft::default() });
        state.set_caret(state.char_len());
        state.autocomplete.on_input(&state.draft.text, state.edit.caret());
        state.autocomplete.options = vec![autocomplete::AutocompleteOption {
            display: "@src/main.rs".into(),
            path: Some("src/main.rs".into()),
            ..autocomplete::AutocompleteOption::default()
        }];
        state.autocomplete.filtered_len = 1;
        let _ = state.accept_autocomplete();
        assert_eq!(state.autocomplete.frecency.data["src/main.rs"].frequency, 2);
    }

    /// RD-05 — the chips delete atomically (backspace over the chip
    /// removes text AND part) and earlier edits shift the ranges.
    #[test]
    fn agent_skill_chips_stay_consistent_under_edits() {
        let mut state = Composer::default();
        state.set(PromptDraft { text: "hey @bu".into(), ..PromptDraft::default() });
        state.set_caret(state.char_len());
        state.autocomplete.on_input(&state.draft.text, state.edit.caret());
        state.autocomplete.options = vec![autocomplete::AutocompleteOption {
            display: "@build".into(),
            reference: Some(autocomplete::Reference::Agent { name: "build".into() }),
            ..autocomplete::AutocompleteOption::default()
        }];
        state.autocomplete.filtered_len = 1;
        let _ = state.accept_autocomplete();
        assert_eq!(state.draft.text, "hey @build ");

        // Inserting before the chip shifts its range.
        state.set_caret(0);
        state.insert_text(">> ");
        let agent = &state.draft.agents[0];
        assert_eq!((agent.start, agent.end), (7, 13));

        // Backspace at the chip's end deletes the whole chip + part.
        state.set_caret(state.draft.agents[0].end);
        state.handle_key(&ComposerKey::Backspace);
        assert_eq!(state.draft.text, ">> hey  ");
        assert!(state.draft.agents.is_empty(), "part removed with the chip");

        // Undo restores the part with the chip.
        state.undo();
        assert_eq!(state.draft.text, ">> hey @build ");
        assert_eq!(state.draft.agents.len(), 1, "undo restores the agent part");
        assert_eq!((state.draft.agents[0].start, state.draft.agents[0].end), (7, 13));
    }

    /// RD-05 — the submit carries the structured parts
    /// (index.tsx:1361-1362: `agents`/`skills` on the prompt POST).
    #[test]
    fn submit_carries_agent_and_skill_parts() {
        let mut state = Composer::default();
        state.set(PromptDraft {
            text: "go @build ".into(),
            agents: vec![AgentMention { name: "build".into(), start: 3, end: 9 }],
            skills: vec![SkillMention {
                id: "refactor".into(),
                name: "refactor".into(),
                start: 10,
                end: 10,
            }],
            ..PromptDraft::default()
        });
        let prompt = state.take_submit().expect("submit");
        assert_eq!(prompt.text, "go @build ");
        assert_eq!(prompt.agents.len(), 1);
        assert_eq!(prompt.skills.len(), 1);
        assert_eq!(prompt.skills[0].id, "refactor");
        assert!(state.draft.agents.is_empty() && state.draft.skills.is_empty());
    }
}
