//! Clipboard via `opentui-core`'s clipboard service
//! (`packages/tui/src/clipboard.ts` / `context/clipboard.tsx`).
//!
//! G-03/G-11: the engine is [`opentui_core::clipboard`] —
//! `create_clipboard({host, terminal})` with a host arm (the pluggable
//! read backend) and a terminal arm (an **OSC 52** escape sequence,
//! `ESC ] 52 ; c ; <base64 payload> ESC \`, which reaches the system
//! clipboard over ssh, written to stdout through crossterm's raw
//! writer). [`Osc52Sink`] keeps the [`ClipboardSink`] seam the store's
//! `messages.copy` path calls: a thin adapter that delegates to the
//! service's `writeText({ destination: "all-available" })` — the OSC 52
//! terminal write is still emitted for terminals that need it.
//!
//! The READ arm ([`ClipboardSource`]) mirrors [`crate::components::
//! composer::PromptSink`]: `prompt.paste` (ctrl+v) reads
//! `{preferredTypes: ["image/png", "text/plain"]}` through the same
//! service — production's host backend layers the platform paste tools
//! (`pbpaste`/`xclip`/`xsel`/`wl-paste`, v2's host-first layering) over
//! the OSC 52 query window ([`crate::terminal_proto`]); tests inject
//! [`FixedClipboardSource`] fakes.

use std::io::Write;

use opentui_core::clipboard::{
    create_clipboard, create_host_clipboard_with_backend, ClipboardOptions, ClipboardReadOptions,
    ClipboardReadResult, ClipboardRepresentation, ClipboardService, ClipboardWriteDestination,
    ClipboardWriteOptions, HostClipboardBackend, HostClipboardClearResult, HostClipboardOptions,
    HostClipboardReadOptions, HostClipboardWriteOptions, HostClipboardWriteResult, Osc52Support,
    TerminalClipboardAdapter, TerminalClipboardOperationResult,
};

/// The OSC 52 write cap. Terminals reject huge sequences (kitty
/// truncates around 100 KB); refuse larger copies explicitly instead
/// of shipping a truncation.
pub const OSC52_MAX_BYTES: usize = 100 * 1024;

/// Terminal clipboard destination: `c` = the system clipboard (v2
/// `selection: "clipboard"`).
const OSC52_DESTINATION: char = 'c';

/// Encode the OSC 52 escape sequence for `text` (base64 payload,
/// `ESC \` terminator).
#[must_use]
pub fn osc52_sequence(text: &str) -> Vec<u8> {
    use base64_encode::encode as b64;
    let mut out = Vec::with_capacity(text.len() + 32);
    out.extend_from_slice(b"\x1b]52;");
    out.push(OSC52_DESTINATION as u8);
    out.push(b';');
    out.extend_from_slice(b64(text.as_bytes()).as_bytes());
    out.extend_from_slice(b"\x1b\\");
    out
}

/// Decode standard-alphabet base64 (padded or unpadded); `None` on
/// any non-alphabet byte — the display path's data-URI payload
/// decoder (bad payloads render the placeholder only).
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    /// Alphabet value of one base64 byte.
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let stripped: Vec<u8> = text.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
    if stripped.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(stripped.len() / 4 * 3);
    for chunk in stripped.chunks(4) {
        let mut triple = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            triple |= value(*byte)? << (18 - 6 * index);
        }
        match chunk.len() {
            4 => out.extend_from_slice(&[(triple >> 16) as u8, (triple >> 8) as u8, triple as u8]),
            3 => out.extend_from_slice(&[(triple >> 16) as u8, (triple >> 8) as u8]),
            2 => out.push((triple >> 16) as u8),
            _ => return None,
        }
    }
    Some(out)
}

/// Minimal base64 (standard alphabet, padded) — shared by the OSC 52
/// writer and the composer's data-URI attachments (no new dependency
/// for one encoder).
pub(crate) mod base64_encode {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b0 = u32::from(chunk[0]);
            let b1 = chunk.get(1).map_or(0, |b| u32::from(*b));
            let b2 = chunk.get(2).map_or(0, |b| u32::from(*b));
            let triple = (b0 << 16) | (b1 << 8) | b2;
            out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
            out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(triple >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 { ALPHABET[triple as usize & 63] as char } else { '=' });
        }
        out
    }
}

/// Why a clipboard write failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    /// The payload exceeds [`OSC52_MAX_BYTES`] (refused, not truncated).
    TooLarge { bytes: usize },
    /// The underlying write failed.
    Io(String),
}

/// The clipboard write seam: the store's `messages.copy` path calls it.
pub trait ClipboardSink {
    /// Write `text` to the clipboard; `Err` surfaces the v2 "Failed to
    /// copy" toast.
    fn write(&self, text: &str) -> Result<(), ClipboardError>;
}

/// A write handle shared between the sink and its terminal adapter
/// (the adapter writes the OSC 52 bytes the service composes).
pub struct SharedHandle<W: Write>(pub std::sync::Mutex<W>);

/// The service's terminal arm: OSC 52 to the shared `Write` handle
/// (stdout live, a `Vec<u8>` in tests — the written-bytes assertion).
struct Osc52TerminalAdapter<W: Write> {
    handle: std::sync::Arc<SharedHandle<W>>,
}

impl<W: Write> TerminalClipboardAdapter for Osc52TerminalAdapter<W> {
    fn remote(&self) -> bool {
        // The live sink always reaches for the terminal arm (the OSC
        // 52 sequence is the whole point over ssh).
        false
    }

    fn write_text(
        &mut self,
        text: &str,
        _selection: opentui_core::clipboard::ClipboardSelection,
    ) -> TerminalClipboardOperationResult {
        let sequence = osc52_sequence(text);
        let attempted = match self.handle.0.lock() {
            Ok(mut handle) => handle.write_all(&sequence).and_then(|()| handle.flush()).is_ok(),
            Err(_) => false,
        };
        TerminalClipboardOperationResult {
            attempted: Some(attempted),
            capability: Osc52Support::Supported,
        }
    }

    fn clear(
        &mut self,
        _selection: opentui_core::clipboard::ClipboardSelection,
    ) -> TerminalClipboardOperationResult {
        TerminalClipboardOperationResult { attempted: None, capability: Osc52Support::Unknown }
    }
}

/// A read-only terminal arm: the read source never touches the
/// terminal (the OSC 52 query window lives in the HOST backend,
/// [`HostToolsClipboardBackend`], which owns its probe discipline).
struct NoopTerminalAdapter;

impl TerminalClipboardAdapter for NoopTerminalAdapter {
    fn remote(&self) -> bool {
        false
    }
    fn write_text(
        &mut self,
        _text: &str,
        _selection: opentui_core::clipboard::ClipboardSelection,
    ) -> TerminalClipboardOperationResult {
        TerminalClipboardOperationResult { attempted: None, capability: Osc52Support::Unknown }
    }
    fn clear(
        &mut self,
        _selection: opentui_core::clipboard::ClipboardSelection,
    ) -> TerminalClipboardOperationResult {
        TerminalClipboardOperationResult { attempted: None, capability: Osc52Support::Unknown }
    }
}

/// The production sink: a thin [`ClipboardSink`] adapter over
/// `opentui_core::clipboard::ClipboardService::write_text` with
/// `destination: "all-available"` — the host arm reports
/// `Unsupported` (host WRITES stay unwired, v2 parity is the OSC 52
/// terminal write), so the composed result is exactly the OSC 52
/// sequence on the handle.
pub struct Osc52Sink<W: Write> {
    /// The shared output handle (tests assert the written bytes).
    pub handle: std::sync::Arc<SharedHandle<W>>,
    service: std::sync::Mutex<ClipboardService>,
}

/// The host backend of the write sink: no platform write arm — every
/// write resolves through the terminal (OSC 52) arm.
struct UnsupportedHostBackend;

impl HostClipboardBackend for UnsupportedHostBackend {
    fn read(&mut self, _options: &HostClipboardReadOptions) -> ClipboardReadResult {
        ClipboardReadResult::Unsupported
    }
    fn write_text(
        &mut self,
        _text: &str,
        _options: &HostClipboardWriteOptions,
    ) -> HostClipboardWriteResult {
        HostClipboardWriteResult::Unsupported
    }
    fn clear(&mut self, _options: &HostClipboardWriteOptions) -> HostClipboardClearResult {
        HostClipboardClearResult::Unsupported
    }
    fn dispose(&mut self) {}
}

impl<W: Write + 'static> Osc52Sink<W> {
    #[must_use]
    pub fn new(handle: W) -> Self {
        let handle = std::sync::Arc::new(SharedHandle(std::sync::Mutex::new(handle)));
        let host = create_host_clipboard_with_backend(
            HostClipboardOptions::default(),
            Box::new(UnsupportedHostBackend),
        )
        .expect("default host options normalize");
        let service = create_clipboard(ClipboardOptions {
            host,
            terminal: Box::new(Osc52TerminalAdapter { handle: std::sync::Arc::clone(&handle) }),
        });
        Self { handle, service: std::sync::Mutex::new(service) }
    }
}

impl<W: Write> ClipboardSink for Osc52Sink<W> {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        if text.len() > OSC52_MAX_BYTES {
            return Err(ClipboardError::TooLarge { bytes: text.len() });
        }
        // v2 strips NUL before any destination (OpenTUI rejects it).
        let cleaned = if text.contains('\0') { text.replace('\0', "") } else { text.to_owned() };
        // The service rejects empty text (its `writeText requires
        // non-empty text` TypeError); v2's empty copy still clears the
        // terminal clipboard — emit the empty OSC 52 set directly.
        if cleaned.is_empty() {
            return self.write_osc52_direct(&cleaned);
        }
        let options = ClipboardWriteOptions {
            destination: Some(ClipboardWriteDestination::AllAvailable),
            ..ClipboardWriteOptions::default()
        };
        let result = self
            .service
            .lock()
            .map_err(|_| ClipboardError::Io("locked".into()))?
            .write_text(&cleaned, &options)
            .map_err(|error| ClipboardError::Io(error.kind_str().to_owned()))?;
        // `all-available` always attempts the terminal arm; a
        // `local-failure` there is the write failure.
        match result.terminal.attempted {
            Some(false) | None => Err(ClipboardError::Io("terminal write failed".into())),
            Some(true) => Ok(()),
        }
    }
}

impl<W: Write> Osc52Sink<W> {
    /// The empty-payload shortcut: the exact OSC 52 bytes, bypassing
    /// the service's non-empty validation.
    fn write_osc52_direct(&self, text: &str) -> Result<(), ClipboardError> {
        let sequence = osc52_sequence(text);
        let mut handle = self.handle.0.lock().map_err(|_| ClipboardError::Io("locked".into()))?;
        handle.write_all(&sequence).map_err(|error| ClipboardError::Io(error.to_string()))?;
        handle.flush().map_err(|error| ClipboardError::Io(error.to_string()))?;
        Ok(())
    }
}

/// One host-clipboard read: `{ data: base64, mime }` — the ref
/// `read()` representation (`clipboard.ts:32-35`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardContent {
    /// Canonical lowercase MIME essence (e.g. `image/png`).
    pub mime: String,
    /// The payload, standard-alphabet base64.
    pub data: String,
}

/// The clipboard READ seam (G-03): `prompt.paste` (ctrl+v) reads
/// `{preferredTypes: ["image/png", "text/plain"]}` through it;
/// production layers the host paste tools over the OSC 52 query
/// window, tests inject fakes (host-clipboard behavior is
/// untestable in CI).
pub trait ClipboardSource {
    /// `None` = no usable content (headless, unsupported, empty).
    fn read(&self) -> Option<ClipboardContent>;
}

/// The production host backend: the platform paste tools first
/// (`pbpaste` / `xclip` / `xsel` / `wl-paste` — v2's host-first
/// layering), then the OSC 52 query window ([`crate::terminal_proto`]
/// owns both arms and the probe discipline). Raw bytes get their
/// MIME from the image-magic sniff (`image/png` minimum; text is
/// `text/plain`).
pub struct HostToolsClipboardBackend;

impl HostClipboardBackend for HostToolsClipboardBackend {
    fn read(&mut self, _options: &HostClipboardReadOptions) -> ClipboardReadResult {
        let Some(bytes) = crate::terminal_proto::clipboard_read() else {
            return ClipboardReadResult::Empty;
        };
        let mime = if crate::terminal_proto::sniff_image(&bytes) {
            crate::terminal_proto::image_mime(&bytes)
        } else {
            "text/plain"
        };
        ClipboardReadResult::Read(ClipboardRepresentation { mime_type: mime.to_owned(), bytes })
    }
    fn write_text(
        &mut self,
        _text: &str,
        _options: &HostClipboardWriteOptions,
    ) -> HostClipboardWriteResult {
        HostClipboardWriteResult::Unsupported
    }
    fn clear(&mut self, _options: &HostClipboardWriteOptions) -> HostClipboardClearResult {
        HostClipboardClearResult::Unsupported
    }
    fn dispose(&mut self) {}
}

/// The production [`ClipboardSource`]: the `opentui_core` clipboard
/// service with the host-tools backend.
pub struct CoreClipboardSource {
    service: std::sync::Mutex<ClipboardService>,
}

impl CoreClipboardSource {
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(Box::new(HostToolsClipboardBackend))
    }

    /// Build around an injected host backend (the test seam —
    /// CI cannot reach a real host clipboard).
    #[must_use]
    pub fn with_backend(backend: Box<dyn HostClipboardBackend>) -> Self {
        let host = create_host_clipboard_with_backend(HostClipboardOptions::default(), backend)
            .expect("default host options normalize");
        let service =
            create_clipboard(ClipboardOptions { host, terminal: Box::new(NoopTerminalAdapter) });
        Self { service: std::sync::Mutex::new(service) }
    }
}

impl Default for CoreClipboardSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardSource for CoreClipboardSource {
    fn read(&self) -> Option<ClipboardContent> {
        // ref `preferredTypes` (prompt/index.tsx:474-483): image/png
        // before text/plain.
        let options = ClipboardReadOptions {
            preferred_types: vec!["image/png".to_owned(), "text/plain".to_owned()],
            ..ClipboardReadOptions::default()
        };
        let mut service = self.service.lock().ok()?;
        match service.read(&options) {
            Ok(ClipboardReadResult::Read(representation)) => Some(ClipboardContent {
                mime: representation.mime_type,
                data: base64_encode::encode(&representation.bytes),
            }),
            _ => None,
        }
    }
}

/// The test source: a fixed read result (injected into the store).
#[derive(Debug, Default)]
pub struct FixedClipboardSource {
    pub content: Option<ClipboardContent>,
}

impl ClipboardSource for FixedClipboardSource {
    fn read(&self) -> Option<ClipboardContent> {
        self.content.clone()
    }
}

/// The test sink: records every write (value assertions).
#[derive(Debug, Default)]
pub struct RecordingClipboardSink {
    pub writes: std::sync::Mutex<Vec<String>>,
}

impl ClipboardSink for RecordingClipboardSink {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        self.writes.lock().expect("clipboard recorder").push(text.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact OSC 52 bytes (base64 payload, ESC \ terminator) —
    /// the written-bytes assertion for `messages.copy`.
    #[test]
    fn osc52_bytes_match_the_protocol() {
        // "hi" → base64 "aGk=".
        assert_eq!(osc52_sequence("hi"), b"\x1b]52;c;aGk=\x1b\\");
        // Empty payload round-trips as an empty clipboard set.
        assert_eq!(osc52_sequence(""), b"\x1b]52;c;\x1b\\");
        // Multi-line text stays verbatim inside the base64 blob.
        let sequence = osc52_sequence("line one\nline two");
        let payload = std::str::from_utf8(&sequence[7..sequence.len() - 2]).unwrap();
        assert_eq!(payload, base64_encode::encode(b"line one\nline two"));
    }

    /// The base64 encoder matches the standard alphabet (spot values).
    #[test]
    fn base64_matches_reference_values() {
        assert_eq!(base64_encode::encode(b""), "");
        assert_eq!(base64_encode::encode(b"f"), "Zg==");
        assert_eq!(base64_encode::encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode::encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode::encode(b"foobar"), "Zm9vYmFy");
    }

    /// The sink delegates to the opentui-core service (terminal arm =
    /// OSC 52 on the handle, host arm `Unsupported` under
    /// `all-available`) — same bytes as before the convergence, plus
    /// the oversize refusal and NUL strip.
    #[test]
    fn sink_writes_bytes_and_refuses_oversized() {
        let sink = Osc52Sink::new(Vec::new());
        sink.write("hello").expect("writes");
        let expected = [osc52_sequence("hello")].concat();
        assert_eq!(sink.handle.0.lock().unwrap().clone(), expected);

        let huge = "x".repeat(OSC52_MAX_BYTES + 1);
        assert_eq!(sink.write(&huge), Err(ClipboardError::TooLarge { bytes: OSC52_MAX_BYTES + 1 }));
        // The refusal wrote nothing.
        assert_eq!(sink.handle.0.lock().unwrap().clone(), expected);

        // NUL is stripped before encoding (v2 parity); the second
        // write appends after the first (stdout-like).
        sink.write("a\0b").expect("writes");
        let expected = [osc52_sequence("hello"), osc52_sequence("ab")].concat();
        assert_eq!(sink.handle.0.lock().unwrap().clone(), expected);

        // The empty write still clears the terminal clipboard (the
        // service itself rejects empty text — the direct shortcut).
        sink.write("").expect("writes");
        let expected = [osc52_sequence("hello"), osc52_sequence("ab"), osc52_sequence("")].concat();
        assert_eq!(sink.handle.0.lock().unwrap().clone(), expected);
    }

    /// A scriptable host backend — CI cannot reach real hardware.
    struct FakeBackend {
        representation: Option<ClipboardRepresentation>,
    }

    impl HostClipboardBackend for FakeBackend {
        fn read(&mut self, _options: &HostClipboardReadOptions) -> ClipboardReadResult {
            self.representation
                .clone()
                .map_or(ClipboardReadResult::Empty, ClipboardReadResult::Read)
        }
        fn write_text(
            &mut self,
            _text: &str,
            _options: &HostClipboardWriteOptions,
        ) -> HostClipboardWriteResult {
            HostClipboardWriteResult::Unsupported
        }
        fn clear(&mut self, _options: &HostClipboardWriteOptions) -> HostClipboardClearResult {
            HostClipboardClearResult::Unsupported
        }
        fn dispose(&mut self) {}
    }

    /// The read source: the service routes the host representation to
    /// `{mime, data: base64}`; empty/unsupported reads are `None`.
    #[test]
    fn source_routes_representations_to_content() {
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let source = CoreClipboardSource::with_backend(Box::new(FakeBackend {
            representation: Some(ClipboardRepresentation {
                mime_type: "image/png".to_owned(),
                bytes: png.clone(),
            }),
        }));
        assert_eq!(
            source.read(),
            Some(ClipboardContent {
                mime: "image/png".to_owned(),
                data: base64_encode::encode(&png)
            })
        );

        let empty =
            CoreClipboardSource::with_backend(Box::new(FakeBackend { representation: None }));
        assert_eq!(empty.read(), None);
    }

    /// The fixed source hands back exactly what was injected (the
    /// app-level ctrl+v tests' seam).
    #[test]
    fn fixed_source_returns_the_injected_content() {
        let source = FixedClipboardSource {
            content: Some(ClipboardContent {
                mime: "text/plain".to_owned(),
                data: "aGk=".to_owned(),
            }),
        };
        assert_eq!(
            source.read(),
            Some(ClipboardContent { mime: "text/plain".to_owned(), data: "aGk=".to_owned() })
        );
        assert_eq!(FixedClipboardSource::default().read(), None);
    }
}
