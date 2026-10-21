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
pub use oc_tui_core::text_edit::{EditOutcome, SnapshotKind};
pub use oc_tui_core::text_edit_opentui::EditCore;
pub use render::{prompt_text_width, AutocompleteView, FooterHints, PromptView};

use std::rc::Rc;

use oc_tui_core::text_edit::{ClusterBounds, EditCommand, TextArea};

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

/// One agent mention (`@build`-style chips).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMention {
    pub name: String,
}

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
}

/// What a submit produced — the value handed to the engine stub.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmittedPrompt {
    pub text: String,
    pub files: Vec<FileMention>,
    pub agents: Vec<AgentMention>,
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

/// The composer's grapheme-cluster model (the editing state's motion
/// granularity): the same `wrap::flatten` seam the renderer uses, so
/// caret moves land on rendered cluster boundaries exactly like the
/// pre-Wave-A composer.
#[derive(Debug, Clone, Copy, Default)]
struct WrapClusters;

impl ClusterBounds for WrapClusters {
    fn cluster_starts(&self, text: &str) -> Vec<usize> {
        cluster_starts(text)
    }
}

/// The composer's full editing state for one session.
///
/// Wave A: the editing core is oc-tui-core's [`TextArea`] (v2's
/// `EditBufferRenderable` operation set — selection, word motions,
/// delete ops, undo/redo); the composer keeps the file/agent mention
/// bookkeeping and the prompt history beside it. `draft.text` is the
/// store-facing text source; the area's text mirrors it across each
/// edit (see [`Composer::run_edit`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Composer {
    pub draft: PromptDraft,
    /// The editing state (caret + selection anchor + undo/redo) — the
    /// flag-selected engine (legacy `TextArea` or the opentui-core
    /// textarea line behind [`EditCore`]).
    pub edit: EditCore,
    /// The mention snapshots parallel to the area's undo stack
    /// (an undo restores chips with their text).
    pub(crate) files_undo: Vec<Vec<FileMention>>,
    pub(crate) files_redo: Vec<Vec<FileMention>>,
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
    /// A fresh editing state with the composer's cluster model (the
    /// legacy engine's wrap-derived clusters; the opentui engine has
    /// its own grapheme model). Flag-selected once per process.
    #[must_use]
    pub fn fresh_edit() -> EditCore {
        if oc_tui_core::opentui_engine_enabled() {
            EditCore::fresh()
        } else {
            EditCore::Legacy(TextArea::with_clusters(Rc::new(WrapClusters)))
        }
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
        self.autocomplete.close();
    }

    /// `PromptRef.reset`.
    pub fn reset(&mut self) {
        self.draft = PromptDraft::default();
        self.edit.set_text("");
        self.files_undo.clear();
        self.files_redo.clear();
        self.autocomplete.close();
    }

    fn char_len(&self) -> usize {
        self.draft.text.chars().count()
    }

    /// Shift the mention ranges over one text delta (the deleted char
    /// range and the inserted length): mentions INSIDE the replaced
    /// range drop, mentions after it shift.
    fn apply_files_delta(&mut self, replaced: std::ops::Range<usize>, inserted: usize) {
        let removed = replaced.len();
        self.draft.files.retain(|file| file.end <= replaced.start || file.start >= replaced.end);
        for file in &mut self.draft.files {
            if file.start >= replaced.end {
                file.start = file.start - removed + inserted;
                file.end = file.end - removed + inserted;
            }
        }
    }

    /// Mirror the mention stacks onto the text area's history event
    /// (`files_undo`/`files_redo` track the area's snapshot stack
    /// one-for-one).
    fn mirror_files_history(&mut self, files_before: Vec<FileMention>, snapshot: SnapshotKind) {
        match snapshot {
            SnapshotKind::Pushed => {
                self.files_undo.push(files_before);
                self.files_redo.clear();
            }
            SnapshotKind::Undone => {
                self.files_redo.push(self.draft.files.clone());
                if let Some(files) = self.files_undo.pop() {
                    self.draft.files = files;
                }
            }
            SnapshotKind::Redone => {
                self.files_undo.push(self.draft.files.clone());
                if let Some(files) = self.files_redo.pop() {
                    self.draft.files = files;
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
        let outcome = run(&mut self.edit);
        self.draft.text = self.edit.text();
        let text_changed = match &outcome {
            EditOutcome::Rejected => false,
            EditOutcome::Applied { replaced, inserted, snapshot } => {
                if !matches!(snapshot, SnapshotKind::Undone | SnapshotKind::Redone) {
                    self.apply_files_delta(replaced.clone(), *inserted);
                }
                self.mirror_files_history(files_before, *snapshot);
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
    /// deletes it atomically).
    fn chip_before_caret(&self) -> Option<std::ops::Range<usize>> {
        let caret = self.edit.caret();
        self.draft
            .files
            .iter()
            .find(|file| caret <= file.end && caret > file.start)
            .map(|file| file.start..file.end)
    }

    /// The mention chip the caret sits at the START of (delete
    /// forward).
    fn chip_after_caret(&self) -> Option<std::ops::Range<usize>> {
        let caret = self.edit.caret();
        self.draft
            .files
            .iter()
            .find(|file| caret >= file.start && caret < file.end)
            .map(|file| file.start..file.end)
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

    /// `prompt.clear` (ctrl+c in the composer) — clears the draft.
    pub fn clear(&mut self) {
        self.draft = PromptDraft::default();
        self.edit.set_text("");
        self.files_undo.clear();
        self.files_redo.clear();
        self.autocomplete.close();
    }

    /// Paste: normalize CRLF/CR to LF at the boundary (`onPaste`).
    pub fn paste(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_text(&normalized);
    }

    /// `prompt.paste` — the ctrl+v COMMAND (D-05 object-binding
    /// semantics). The keymap binds ctrl+v with `preventDefault:
    /// false` and leaves suppression to this handler: with a key
    /// event the stroke is consumed here (the terminal default and
    /// every lower layer skipped); without one (programmatic
    /// dispatch) nothing is prevented and the command still runs.
    /// The port's clipboard READ seam is out of scope (clipboard.rs
    /// writes only), so the enqueue arm of v2's `enqueuePaste`
    /// inserts nothing by itself — bracketed-paste text lands through
    /// [`Self::paste`] (the `onPaste` path, prompt/index.tsx:1786-
    /// 1802).
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
    /// Returns whether any attachment staged.
    pub fn paste_resolved(
        &mut self,
        text: &str,
        platform: &str,
        files: &dyn attachment::LocalFiles,
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
        self.paste(&normalized);
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
            text: std::mem::take(&mut self.draft.text),
            files: std::mem::take(&mut self.draft.files),
            agents: std::mem::take(&mut self.draft.agents),
            ..SubmittedPrompt::default()
        };
        self.edit.set_text("");
        self.files_undo.clear();
        self.files_redo.clear();
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
                let name = option.display.trim_start_matches('@');
                self.insert_text(&format!("@{name} "));
                KeyOutcome::Edited
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

/// Cluster boundaries (char offsets where a grapheme cluster starts),
/// derived from the same `wrap::flatten` seam the renderer uses.
fn cluster_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::with_capacity(text.chars().count() + 1);
    let mut offset = 0usize;
    for atom in crate::wrap::flatten(text, ratatui::style::Style::new()) {
        starts.push(offset);
        offset += atom.text.chars().count();
    }
    starts.push(offset);
    starts
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
        assert!(state.paste_resolved("/tmp/shot.png", "linux", &files));
        assert_eq!(state.draft.text, "[Image 1] ");
        assert_eq!(state.draft.files.len(), 1);

        let plain =
            MapFiles { files: [("/tmp/a.txt".to_owned(), b"hi".to_vec())].into_iter().collect() };
        let mut text_state = Composer::default();
        assert!(!text_state.paste_resolved("just some words", "linux", &plain));
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
}
