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
pub use render::{render_prompt_frame, AutocompleteView, FooterHints, PromptView};

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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Composer {
    pub draft: PromptDraft,
    /// Caret offset in chars (C2: not rendered).
    pub caret: usize,
    pub history: PromptHistory,
    pub placeholders: Vec<String>,
    pub placeholder_index: usize,
    pub autocomplete: AutocompleteState,
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

    /// `PromptRef.set` — the draft-restore path (stash/undo).
    pub fn set(&mut self, draft: PromptDraft) {
        self.draft = draft;
        self.caret = self.draft.text.chars().count();
        self.autocomplete.close();
    }

    /// `PromptRef.reset`.
    pub fn reset(&mut self) {
        self.draft = PromptDraft::default();
        self.caret = 0;
        self.autocomplete.close();
    }

    fn char_len(&self) -> usize {
        self.draft.text.chars().count()
    }

    /// Insert text at the caret, keeping file mentions' ranges valid.
    pub fn insert_text(&mut self, text: &str) {
        let insert_len = text.chars().count();
        let byte = char_to_byte(&self.draft.text, self.caret);
        for file in &mut self.draft.files {
            if file.start >= self.caret {
                file.start += insert_len;
                file.end += insert_len;
            }
        }
        self.draft.text.insert_str(byte, text);
        self.caret += insert_len;
    }

    /// `input.newline` (shift/ctrl/alt+return, ctrl+j).
    pub fn newline(&mut self) {
        self.insert_text("\n");
    }

    /// `input.backspace` — deletes the cluster before the caret and
    /// any mention it sits inside.
    pub fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        // A chip deletes as one unit (the extmark's atomic range).
        if let Some(index) = self
            .draft
            .files
            .iter()
            .position(|file| self.caret <= file.end && self.caret > file.start)
        {
            let file = self.draft.files.remove(index);
            self.delete_range(file.start, file.end);
            return;
        }
        let start = self.caret - prev_cluster_len(&self.draft.text, self.caret);
        self.delete_range(start, self.caret);
    }

    /// `input.delete` — the cluster after the caret.
    pub fn delete_forward(&mut self) {
        if self.caret >= self.char_len() {
            return;
        }
        if let Some(index) = self
            .draft
            .files
            .iter()
            .position(|file| self.caret >= file.start && self.caret < file.end)
        {
            let file = self.draft.files.remove(index);
            self.delete_range(file.start, file.end);
            return;
        }
        let end = self.caret + next_cluster_len(&self.draft.text, self.caret);
        self.delete_range(self.caret, end);
    }

    fn delete_range(&mut self, start: usize, end: usize) {
        let start_b = char_to_byte(&self.draft.text, start);
        let end_b = char_to_byte(&self.draft.text, end);
        self.draft.text.replace_range(start_b..end_b, "");
        let removed = end - start;
        self.draft.files.retain(|file| file.end <= start || file.start >= end);
        for file in &mut self.draft.files {
            if file.start >= end {
                file.start -= removed;
                file.end -= removed;
            }
        }
        self.caret = self.caret.min(start).max(start);
    }

    /// `input.line.home` / `input.line.end`.
    pub fn line_home(&mut self) {
        self.caret = self.line_start_offset();
    }

    pub fn line_end(&mut self) {
        let text = &self.draft.text;
        let start_b = char_to_byte(text, self.line_start_offset());
        let rest = &text[start_b..];
        let nl = rest.find('\n').unwrap_or(rest.len());
        self.caret = text[..start_b + nl].chars().count();
    }

    fn line_start_offset(&self) -> usize {
        let text = &self.draft.text;
        let byte = char_to_byte(text, self.caret.min(text.chars().count()));
        let start = text[..byte].rfind('\n').map_or(0, |pos| pos + 1);
        text[..start].chars().count()
    }

    /// `input.delete.to.line.end` (ctrl+k).
    pub fn delete_to_line_end(&mut self) {
        let start = self.caret;
        let before = self.caret;
        self.line_end();
        let end = self.caret;
        self.caret = before;
        if end > start {
            self.delete_range(start, end);
        }
    }

    /// `input.delete.to.line.start` (ctrl+u).
    pub fn delete_to_line_start(&mut self) {
        let end = self.caret;
        let start = self.line_start_offset();
        if end > start {
            self.delete_range(start, end);
        }
    }

    /// `prompt.clear` (ctrl+c in the composer) — clears the draft.
    pub fn clear(&mut self) {
        self.draft = PromptDraft::default();
        self.caret = 0;
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
        let start = self.caret;
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

    /// Left/right by one cluster.
    pub fn move_left(&mut self) {
        if self.caret > 0 {
            self.caret -= prev_cluster_len(&self.draft.text, self.caret);
        }
    }

    pub fn move_right(&mut self) {
        if self.caret < self.char_len() {
            self.caret += next_cluster_len(&self.draft.text, self.caret);
        }
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
        // visual-row move is C2 caret internals).
        self.caret = self.line_start_offset().saturating_sub(1);
        self.line_end();
        KeyOutcome::Edited
    }

    pub fn move_down(&mut self) -> KeyOutcome {
        if self.autocomplete.visible.is_some() {
            self.autocomplete.move_selection(1);
            return KeyOutcome::Edited;
        }
        let text = &self.draft.text;
        let byte = char_to_byte(text, self.caret);
        if text[byte..].find('\n').is_none() {
            return self.history_next();
        }
        self.caret = text[..=byte].chars().count();
        self.line_end();
        KeyOutcome::Edited
    }

    /// `prompt.history.previous` — swap the draft for the older entry.
    pub fn history_previous(&mut self) -> KeyOutcome {
        let (history, entry) = self.history.clone().previous(&self.draft.text);
        if let Some(entry) = entry {
            self.history = history;
            self.draft.text = entry;
            self.caret = self.char_len();
            return KeyOutcome::Edited;
        }
        KeyOutcome::Ignored
    }

    /// `prompt.history.next`.
    pub fn history_next(&mut self) -> KeyOutcome {
        let (history, entry) = self.history.clone().next(&self.draft.text);
        self.history = history;
        self.draft.text = entry;
        self.caret = self.char_len();
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
        };
        self.caret = 0;
        self.autocomplete.close();
        Some(prompt)
    }

    /// `Ask anything… "{entry}"` — the placeholder for the selected
    /// list entry (`placeholderText`). An EMPTY list renders no
    /// placeholder at all (the reference returns `undefined` when
    /// `list().length === 0` — the session route mounts `Prompt`
    /// without placeholders), so the empty-list form yields "".
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
        format!("Ask anything… \"{entry}\"")
    }

    /// The pump's key dispatch: printable chars feed the textarea;
    /// the rest resolve through the v2 table's composer slice.
    pub fn handle_key(&mut self, key: &ComposerKey) -> KeyOutcome {
        if self.autocomplete.visible.is_some() {
            match key {
                ComposerKey::Escape => {
                    self.autocomplete.dismiss();
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
        match key {
            ComposerKey::Char(ch) => {
                self.insert_text(&ch.to_string());
                self.autocomplete.on_input(&self.draft.text, self.caret);
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
                self.autocomplete.on_input(&self.draft.text, self.caret);
                KeyOutcome::Edited
            }
            ComposerKey::Delete => {
                self.delete_forward();
                self.autocomplete.on_input(&self.draft.text, self.caret);
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
            ComposerKey::Tab => KeyOutcome::Ignored,
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
                let end = self.caret;
                if index <= end {
                    let byte_start = char_to_byte(&self.draft.text, index);
                    let byte_end = char_to_byte(&self.draft.text, end);
                    self.draft.text.replace_range(byte_start..byte_end, "");
                    self.caret = index;
                    let removed = end - index;
                    for file in &mut self.draft.files {
                        if file.start >= end {
                            file.start -= removed;
                            file.end -= removed;
                        }
                    }
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

/// The cluster length (in chars) ending at `caret`.
fn prev_cluster_len(text: &str, caret: usize) -> usize {
    let starts = cluster_starts(text);
    let mut prev = 0;
    for start in starts {
        if start >= caret {
            break;
        }
        prev = start;
    }
    caret - prev
}

/// The cluster length (in chars) starting at `caret`.
fn next_cluster_len(text: &str, caret: usize) -> usize {
    let starts = cluster_starts(text);
    match starts.iter().find(|start| **start > caret) {
        Some(next) => next - caret,
        None => 1,
    }
}

/// The engine prompt API stub: what `store::Store::submit_prompt`
/// forwards submissions to (the real `session.prompt` oc-client call
/// is wave 3's TODO(7.3)).
pub trait PromptSink {
    fn send(&self, session_id: &str, prompt: &SubmittedPrompt);
}

/// The default sink: records submissions in order (tests + the replay
/// harness assert against it).
#[derive(Debug, Default)]
pub struct RecordingPromptSink {
    pub submissions: std::cell::RefCell<Vec<(String, SubmittedPrompt)>>,
}

impl PromptSink for RecordingPromptSink {
    fn send(&self, session_id: &str, prompt: &SubmittedPrompt) {
        self.submissions.borrow_mut().push((session_id.to_owned(), prompt.clone()));
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
        assert!(multi.caret <= 3, "caret moved to the first row");
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
        assert_eq!(state.caret, 10);
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
        state.caret = state.draft.files[0].end;
        state.handle_key(&ComposerKey::Backspace);
        assert_eq!(state.draft.text, "Compare  now");
        assert!(state.draft.files.is_empty(), "chip removed with the mention");
    }

    #[test]
    fn clear_and_line_ops_follow_the_table() {
        let mut state = composer("hello world");
        state.caret = 5;
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
