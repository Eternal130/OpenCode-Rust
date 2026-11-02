//! Lane B3a — the opentui-backed terminal session I/O layer.
//!
//! The main TUI session's input/mode plumbing, migrated off crossterm
//! onto opentui-core's terminal module (`Terminal<UnixTty, Stdout>` owns
//! raw mode / alt screen / the setup+teardown bursts; the capability
//! probe rides `probe_capabilities` + `CliRenderEvent`). The RENDER path
//! is untouched (ratatui `CrosstermBackend` stays the frame writer until
//! B3b).
//!
//! # Input routing (see the lane inventory, commit 1)
//!
//! The `CliRenderer` feed lane is used for what it owns — capability
//! probe routing, the late-OSC theme watch, terminal focus — while
//! [`KeyPipeline`] parses keys/mouse/paste from its own pending buffer
//! over the same read chunks. The two gaps that forced this split:
//!
//! * a lone `ESC` never resolves inside `CliRenderer::feed_bytes`
//!   (`complete_unit_len`: "lone ESC(s): timeout resolution"), while
//!   crossterm flushed it at read-chunk end — the pipeline flushes
//!   all-ESC pending at chunk end for parity;
//! * a headless renderer drains mouse sequences into its (empty)
//!   renderable tree with no consumer surface — the pipeline
//!   intercepts them with opentui's public `MouseParser` first.
//!
//! The double parse of one stream is deterministic and side-effect
//! free: the renderer side dispatches keys/paste to no listeners and
//! `CliRendererConfig::use_mouse` is false so its mouse arm only
//! swallows the bytes the pipeline already reported.

use oc_theme::types::Mode;
use opentui_core::parse_keypress::{parse_keypress, ParseKeypressOptions, ParsedKey};

/// The oc-tui-owned mouse event mirror (crossterm's shape, so
/// [`crate::app::App::mouse_event`] and its tests keep their form).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: KeyModifiers,
}

/// crossterm's `MouseEventKind` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Moved,
    Drag(MouseButton),
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

/// crossterm's `MouseButton` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// crossterm's `KeyModifiers` shape (mouse chords carry the three
/// classic bits; `NONE` kept for test-source parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    #[must_use]
    pub fn empty() -> Self {
        Self(0)
    }
    pub const SHIFT: Self = Self(0b0001);
    pub const ALT: Self = Self(0b0010);
    pub const CTRL: Self = Self(0b0100);
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The app event vocabulary bridged into the run loop's tokio mpsc
/// (replaces the crossterm `Event` enum 1:1).
#[derive(Debug, Clone, PartialEq)]
pub enum TermEvent {
    Key(String),
    Mouse(MouseEvent),
    Paste(String),
    FocusGained,
    FocusLost,
    Resize(u16, u16),
    /// The probe's theme reduction reported a system mode.
    ThemeMode(Mode),
    /// A capability snapshot landed (`true` = DA1 attr 4 / sixel).
    Sixel(bool),
}

// ---------------------------------------------------------------------------
// Key conversion — opentui ParsedKey → the pump's key spec vocabulary
// ---------------------------------------------------------------------------

/// opentui's parsed key → the pump's key spec vocabulary (the
/// replacement for the retired `crossterm_key_to_spec`; behavior
/// aligned — the PTY key families are the gate).
#[must_use]
pub fn opentui_key_to_spec(key: &ParsedKey) -> Option<String> {
    let name = key.name.as_str();
    if name.is_empty() {
        return None; // unknown/unmapped sequence
    }
    if key.ctrl {
        // Combined chords keep the modifier set (ctrl+alt family).
        let alt = key.meta;
        if name == "tab" {
            // G-139: ctrl+tab / ctrl+shift+tab (BackTab arrives with
            // SHIFT set; the CONTROL branch wins the modifier order).
            if alt {
                return None;
            }
            return Some(
                if key.shift { "ctrl+shift+tab" } else { "ctrl+tab" }.to_owned(),
            );
        }
        // Only the Char family carries through the ctrl branch
        // (crossterm's CONTROL arm drops ctrl+<non-char> bindings).
        let ch = key_char(name)?;
        return Some(if alt {
            format!("ctrl+alt+{}", ch.to_ascii_lowercase())
        } else {
            format!("ctrl+{}", ch.to_ascii_lowercase())
        });
    }
    if key.meta {
        return match key_char(name) {
            Some(ch) => Some(format!("alt+{}", ch.to_ascii_lowercase())),
            None => match name {
                "up" | "down" | "left" | "right" | "home" | "end" | "pageup" | "pagedown" => {
                    Some(format!("alt+{name}"))
                }
                _ => None,
            },
        };
    }
    if key.shift {
        if let Some(ch) = key_char(name) {
            return Some(ch.to_ascii_uppercase().to_string());
        }
        return match name {
            // Tab and the legacy `ESC [ Z` backtab are both shift+tab.
            "tab" => Some("shift+tab".to_owned()),
            _ if is_function_key(name) => Some(format!("shift+{name}")),
            "up" | "down" | "left" | "right" | "home" | "end" => {
                Some(format!("shift+{name}"))
            }
            _ => None,
        };
    }
    plain_key_spec(name)
}

/// The named-key → char vocabulary (single characters; "space" is the
/// one multi-char name that maps to a Char in crossterm).
fn key_char(name: &str) -> Option<char> {
    if name == "space" {
        return Some(' ');
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

/// `f1`..`f35` (crossterm's `KeyCode::F(u8)` family).
fn is_function_key(name: &str) -> bool {
    match name.strip_prefix('f') {
        Some(digits) => !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

fn plain_key_spec(name: &str) -> Option<String> {
    if let Some(ch) = key_char(name) {
        return Some(ch.to_string());
    }
    let spec = match name {
        "return" => "return",
        // crossterm maps a raw `\n` in raw mode to `Char('\n')` — the
        // (unbound) literal spec, not `return`.
        "linefeed" => "\n",
        "escape" => "escape",
        "backspace" => "backspace",
        "delete" => "delete",
        "tab" => "tab",
        "up" => "up",
        "down" => "down",
        "left" => "left",
        "right" => "right",
        "home" => "home",
        "end" => "end",
        "pageup" => "pageup",
        "pagedown" => "pagedown",
        _ if is_function_key(name) => return Some(name.to_owned()),
        _ => return None,
    };
    Some(spec.to_owned())
}
// ---------------------------------------------------------------------------
// The key/mouse/paste pipeline
// ---------------------------------------------------------------------------

/// The adapter's own feed state machine over raw read chunks: mouse
/// interception, bracketed-paste collection, focus sequences, and key
/// units (with the lone-ESC chunk-end flush crossterm had).
#[derive(Default)]
pub struct KeyPipeline {
    pending: Vec<u8>,
    mouse: opentui_core::parse_mouse::MouseParser,
    paste: Option<opentui_core::paste::BracketedPasteCollector>,
    kitty: bool,
}

impl KeyPipeline {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The kitty parse flag flips on with the `CSI >5u` push (the
    /// terminal then encodes chords in kitty form).
    pub fn set_kitty(&mut self, on: bool) {
        self.kitty = on;
    }

    /// Process one read chunk into app events (in order).
    pub fn process(&mut self, bytes: &[u8]) -> Vec<TermEvent> {
        let mut events = Vec::new();
        if bytes.is_empty() {
            return events;
        }
        self.pending.extend_from_slice(bytes);
        self.drain(&mut events);
        self.flush_lone_escape(&mut events);
        events
    }

    fn drain(&mut self, events: &mut Vec<TermEvent>) {
        loop {
            // Active paste: everything flows through the collector.
            if let Some(mut collector) = self.paste.take() {
                match collector.consume(&self.pending) {
                    Some((paste, remainder)) => {
                        events.push(TermEvent::Paste(
                            opentui_core::paste::decode_paste_bytes(&paste),
                        ));
                        self.pending = remainder;
                    }
                    None => {
                        // The collector consumed the chunk; the bytes
                        // are gone from the wire stream.
                        self.pending.clear();
                        self.paste = Some(collector);
                        return; // wait for the end marker
                    }
                }
                continue;
            }
            // Paste start at a unit boundary.
            if self
                .pending
                .starts_with(opentui_core::paste::BRACKETED_PASTE_START)
            {
                let start_len = opentui_core::paste::BRACKETED_PASTE_START.len();
                self.pending.drain(..start_len);
                self.paste = Some(opentui_core::paste::BracketedPasteCollector::new());
                continue;
            }
            // Mouse sequences route before keys (crossterm's order).
            if let Some(len) = self.mouse.parse_mouse_sequence_len(&self.pending, 0) {
                for raw in self.mouse.parse_all_mouse_events(&self.pending[..len]) {
                    if let Some(event) = convert_mouse(&raw) {
                        events.push(TermEvent::Mouse(event));
                    }
                }
                self.pending.drain(..len);
                continue;
            }
            // Terminal focus sequences.
            if self.pending.starts_with(b"\x1b[I") {
                self.pending.drain(..3);
                events.push(TermEvent::FocusGained);
                continue;
            }
            if self.pending.starts_with(b"\x1b[O") {
                self.pending.drain(..3);
                events.push(TermEvent::FocusLost);
                continue;
            }
            // Complete key units only; incompletes stay pending.
            let Some(unit_len) = complete_unit_len(&self.pending) else {
                return;
            };
            let unit: Vec<u8> = self.pending.drain(..unit_len).collect();
            if let Some(event) = self.key_event(&unit) {
                events.push(event);
            }
        }
    }

    /// The crossterm lone-ESC parity flush: a read chunk that ENDS on
    /// bare `ESC` byte(s) resolves to the escape key immediately
    /// (crossterm's `input_available=false` arm), never held pending.
    fn flush_lone_escape(&mut self, events: &mut Vec<TermEvent>) {
        if !self.pending.is_empty() && self.pending.iter().all(|b| *b == 0x1b) {
            let unit = std::mem::take(&mut self.pending);
            if let Some(event) = self.key_event(&unit) {
                events.push(event);
            }
        }
    }

    fn key_event(&mut self, unit: &[u8]) -> Option<TermEvent> {
        let raw = match std::str::from_utf8(unit) {
            Ok(text) => text.to_owned(),
            // Non-UTF-8 bytes decode latin1 like the renderer's feed.
            Err(_) => unit
                .iter()
                .map(|b| char::from_u32(u32::from(*b)).unwrap_or('\u{fffd}'))
                .collect(),
        };
        let parsed = parse_keypress(
            &raw,
            ParseKeypressOptions {
                use_kitty_keyboard: self.kitty,
            },
        )?;
        if parsed.event_type != opentui_core::parse_keypress::KeyEventType::Press {
            return None; // the Press-only filter (no release flags pushed)
        }
        opentui_key_to_spec(&parsed).map(TermEvent::Key)
    }
}

/// opentui `RawMouseEvent` → the oc-tui mirror shape.
fn convert_mouse(
    raw: &opentui_core::parse_mouse::RawMouseEvent,
) -> Option<MouseEvent> {
    use opentui_core::parse_mouse::{MouseEventType, ScrollDirection};
    let button =
        |code: i32| -> Option<MouseButton> {
            match code {
                0 => Some(MouseButton::Left),
                1 => Some(MouseButton::Middle),
                2 => Some(MouseButton::Right),
                _ => None,
            }
        };
    let kind = match raw.event_type {
        MouseEventType::Down => MouseEventKind::Down(button(raw.button)?),
        MouseEventType::Up => MouseEventKind::Up(button(raw.button)?),
        MouseEventType::Move => MouseEventKind::Moved,
        MouseEventType::Drag => MouseEventKind::Drag(button(raw.button)?),
        MouseEventType::Scroll => match raw.scroll.map(|info| info.direction) {
            Some(ScrollDirection::Up) => MouseEventKind::ScrollUp,
            Some(ScrollDirection::Down) => MouseEventKind::ScrollDown,
            Some(ScrollDirection::Left) => MouseEventKind::ScrollLeft,
            Some(ScrollDirection::Right) => MouseEventKind::ScrollRight,
            None => return None,
        },
        // Synthetic tree-routing kinds never come off the wire parser.
        MouseEventType::DragEnd | MouseEventType::Drop | MouseEventType::Over
        | MouseEventType::Out => return None,
    };
    let mut modifiers = KeyModifiers::empty();
    if raw.modifiers.shift {
        modifiers = modifiers.union(KeyModifiers::SHIFT);
    }
    if raw.modifiers.alt {
        modifiers = modifiers.union(KeyModifiers::ALT);
    }
    if raw.modifiers.ctrl {
        modifiers = modifiers.union(KeyModifiers::CTRL);
    }
    #[allow(clippy::cast_sign_loss)] // parser coords are terminal-nonnegative
    let column = u16::try_from(raw.x.max(0)).unwrap_or(0);
    #[allow(clippy::cast_sign_loss)]
    let row = u16::try_from(raw.y.max(0)).unwrap_or(0);
    Some(MouseEvent { kind, column, row, modifiers })
}

/// The length of the first complete protocol unit in `data`, `None`
/// while incomplete — the semantics of opentui's (private)
/// `complete_unit_len`, ported so the adapter can pre-split units:
/// single ASCII bytes, `ESC`-introduced sequences, complete UTF-8
/// codepoints. Lone `ESC`s report incomplete (the caller's chunk-end
/// flush resolves them).
fn complete_unit_len(data: &[u8]) -> Option<usize> {
    let first = *data.first()?;
    if first != 0x1b {
        let len = match first {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return None, // stray continuation / invalid lead
        };
        if data.len() < len {
            return None;
        }
        if len > 1 && !data[1..len].iter().all(|b| (0x80..=0xbf).contains(b)) {
            return Some(1); // broken sequence: the maximal subpart is a unit
        }
        return Some(len);
    }
    let mut i = 0;
    while i < data.len() && data[i] == 0x1b {
        i += 1;
    }
    if i == data.len() {
        return None; // lone ESC(s): the chunk-end flush owns these
    }
    match data[i] {
        b'[' => {
            let mut j = i + 1;
            if j == 2 && data.get(2) == Some(&b'[') {
                j = 3; // the cygwin `ESC [[ A` payload bracket
            }
            while j < data.len() {
                if (0x40..=0x7e).contains(&data[j]) {
                    return Some(j + 1);
                }
                j += 1;
            }
            None
        }
        b'O' | b'N' => {
            if data.len() >= i + 2 {
                Some(i + 2)
            } else {
                None
            }
        }
        b']' | b'P' | b'X' | b'^' | b'_' => {
            let is_osc = data[i] == b']';
            let mut j = i + 1;
            while j < data.len() {
                if is_osc && data[j] == 0x07 {
                    return Some(j + 1);
                }
                if data[j] == 0x1b && data.get(j + 1) == Some(&b'\\') {
                    return Some(j + 2);
                }
                j += 1;
            }
            None
        }
        _ => Some(i + 1), // ESC + byte: a complete two-byte escape
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_spec(bytes: &[u8]) -> Option<String> {
        let mut pipeline = KeyPipeline::new();
        let events = pipeline.process(bytes);
        events
            .into_iter()
            .find_map(|event| match event {
                TermEvent::Key(spec) => Some(spec),
                _ => None,
            })
            .or_else(|| {
                // multi-unit feeds: collect the first key only
                None
            })
    }

    fn first_key(bytes: &[u8]) -> Option<String> {
        let mut pipeline = KeyPipeline::new();
        pipeline
            .process(bytes)
            .into_iter()
            .find_map(|event| match event {
                TermEvent::Key(spec) => Some(spec),
                _ => None,
            })
    }

    // -- the crossterm_key_to_spec parity table (PTY key families) --

    #[test]
    fn plain_keys_map() {
        assert_eq!(first_key(b"a"), Some("a".to_owned()));
        assert_eq!(first_key(b"A"), Some("A".to_owned()));
        assert_eq!(first_key(b"1"), Some("1".to_owned()));
        assert_eq!(first_key(b" "), Some(" ".to_owned()));
        assert_eq!(first_key(b"("), Some("(".to_owned()));
        assert_eq!(first_key(b"\r"), Some("return".to_owned()));
        assert_eq!(first_key(b"\n"), Some("\n".to_owned()));
        assert_eq!(first_key(b"\x1b"), Some("escape".to_owned()));
        assert_eq!(first_key(b"\x7f"), Some("backspace".to_owned()));
        assert_eq!(first_key(b"\t"), Some("tab".to_owned()));
        assert_eq!(first_key(b"\x1b[Z"), Some("shift+tab".to_owned()));
        assert_eq!(first_key(b"\x1b[A"), Some("up".to_owned()));
        assert_eq!(first_key(b"\x1b[B"), Some("down".to_owned()));
        assert_eq!(first_key(b"\x1b[C"), Some("right".to_owned()));
        assert_eq!(first_key(b"\x1b[D"), Some("left".to_owned()));
        assert_eq!(first_key(b"\x1b[H"), Some("home".to_owned()));
        assert_eq!(first_key(b"\x1b[F"), Some("end".to_owned()));
        assert_eq!(first_key(b"\x1b[5~"), Some("pageup".to_owned()));
        assert_eq!(first_key(b"\x1b[6~"), Some("pagedown".to_owned()));
        assert_eq!(first_key(b"\x1b[3~"), Some("delete".to_owned()));
        assert_eq!(first_key(b"\x1bOP"), Some("f1".to_owned()));
        assert_eq!(first_key(b"\x1b[11~"), Some("f1".to_owned()));
        assert_eq!(first_key(b"\x1b[24~"), Some("f12".to_owned()));
        assert_eq!(first_key(b"\x1b[200~nope"), None, "paste start eats the stream");
    }

    #[test]
    fn modifier_chords_map() {
        // ctrl family (control-byte encodings)
        assert_eq!(first_key(b"\x03"), Some("ctrl+c".to_owned()));
        assert_eq!(first_key(b"\x01"), Some("ctrl+a".to_owned()));
        assert_eq!(first_key(b"\x00"), Some("ctrl+ ".to_owned()));
        // CSI modifier form: 2=shift 3=alt 5=ctrl. The ctrl+arrow
        // family has no keymap binding (crossterm's CONTROL arm drops
        // non-Char chords) - parity is dropping it.
        assert_eq!(key_spec(b"\x1b[1;5A"), None);
        assert_eq!(key_spec(b"\x1b[1;2A"), Some("shift+up".to_owned()));
        assert_eq!(key_spec(b"\x1b[1;3A"), Some("alt+up".to_owned()));
        assert_eq!(key_spec(b"\x1b[1;3F"), Some("alt+end".to_owned()));
        assert_eq!(key_spec(b"\x1b[1;3H"), Some("alt+home".to_owned()));
        assert_eq!(key_spec(b"\x1b[1;3B"), Some("alt+down".to_owned()));
        // alt+letter is the ESC-prefix form
        assert_eq!(key_spec(b"\x1ba"), Some("alt+a".to_owned()));
        assert_eq!(key_spec(b"\x1bA"), Some("alt+a".to_owned()));
        // ctrl+alt
        assert_eq!(key_spec(b"\x1b\x01"), Some("ctrl+alt+a".to_owned()));
        // CSI R forms are cursor-position reports in crossterm
        // (swallowed, never a key) - parity is dropping them.
        assert_eq!(key_spec(b"\x1b[1;2R"), None);
        // shift+f1 via the SS3-style modifier form
        assert_eq!(key_spec(b"\x1b[1;2P"), Some("shift+f1".to_owned()));
    }

    #[test]
    fn mouse_and_focus_and_paste_map() {
        let mut pipeline = KeyPipeline::new();
        let events = pipeline.process(
            b"\x1b[<0;10;5M\x1b[<0;10;5m\x1b[<64;3;3M\x1b[<66;3;3M\x1b[I\x1b[O",
        );
        let kinds: Vec<&MouseEventKind> = events
            .iter()
            .filter_map(|event| match event {
                TermEvent::Mouse(mouse) => Some(&mouse.kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                &MouseEventKind::Down(MouseButton::Left),
                &MouseEventKind::Up(MouseButton::Left),
                &MouseEventKind::ScrollUp,
                &MouseEventKind::ScrollLeft,
            ]
        );
        assert_eq!(pipeline.process(b"\x1b[<0;10;5M")[0], TermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 9,
            row: 4,
            modifiers: KeyModifiers::empty(),
        }));
        // modifiers: 4=shift 8=alt 16=ctrl bits in the SGR button code
        assert!(matches!(
            pipeline.process(b"\x1b[<4;1;1M")[0],
            TermEvent::Mouse(MouseEvent { modifiers, .. }) if modifiers.contains(KeyModifiers::SHIFT)
        ));
        assert!(matches!(
            pipeline.process(b"\x1b[<16;1;1M")[0],
            TermEvent::Mouse(MouseEvent { modifiers, .. }) if modifiers.contains(KeyModifiers::CTRL)
        ));
        // focus
        assert_eq!(events[events.len() - 2], TermEvent::FocusGained);
        assert_eq!(events[events.len() - 1], TermEvent::FocusLost);
        // paste
        let mut paste_pipe = KeyPipeline::new();
        let events = paste_pipe.process(b"\x1b[200~hello");
        assert!(events.is_empty(), "paste waits for the end marker");
        let events = paste_pipe.process(b" world\x1b[201~");
        assert_eq!(
            events,
            vec![TermEvent::Paste("hello world".to_owned())]
        );
    }

    /// The lone-ESC flush: an ESC-final chunk resolves immediately, a
    /// split sequence across chunks resolves once complete.
    #[test]
    fn lone_escape_flushes_at_chunk_end() {
        let mut pipeline = KeyPipeline::new();
        assert_eq!(
            pipeline.process(b"\x1b"),
            vec![TermEvent::Key("escape".to_owned())]
        );
        let mut pipeline = KeyPipeline::new();
        assert!(pipeline.process(b"\x1b[").is_empty(), "incomplete CSI holds");
        assert_eq!(pipeline.process(b"A"), vec![TermEvent::Key("up".to_owned())]);
        // ESC + key in ONE chunk is the alt+key chord (two-byte unit)
        let mut pipeline = KeyPipeline::new();
        assert_eq!(pipeline.process(b"\x1bx"), vec![TermEvent::Key("alt+x".to_owned())]);
        // ESC alone then the key in the NEXT chunk: escape fired first
        let mut pipeline = KeyPipeline::new();
        assert_eq!(pipeline.process(b"\x1b"), vec![TermEvent::Key("escape".to_owned())]);
        assert_eq!(pipeline.process(b"x"), vec![TermEvent::Key("x".to_owned())]);
    }
}
