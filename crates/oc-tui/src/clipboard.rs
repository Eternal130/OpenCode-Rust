//! Clipboard write via OSC 52 (`packages/tui/src/clipboard.ts` /
//! `context/clipboard.tsx`).
//!
//! v2's clipboard service writes text through `OpenTUI`'s
//! `writeText({ destination: "all-available" })` — the terminal
//! adapter arm of which is an **OSC 52** escape sequence
//! (`ESC ] 52 ; c ; <base64 payload> ESC \`), which reaches the
//! system clipboard over ssh. The Rust port writes that sequence to
//! stdout through crossterm's raw writer; host-clipboard access (the
//! other arm) needs a platform crate and stays out of scope.
//!
//! The [`ClipboardSink`] seam mirrors [`crate::components::composer::
//! PromptSink`]: the store calls it for `messages.copy`, tests inject
//! a recorder, production writes OSC 52.

use std::io::Write;

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

/// Minimal base64 (standard alphabet, padded) — no new dependency for
/// one encoder.
mod base64_encode {
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

/// The clipboard seam: the store's `messages.copy` path calls it.
pub trait ClipboardSink {
    /// Write `text` to the clipboard; `Err` surfaces the v2 "Failed to
    /// copy" toast.
    fn write(&self, text: &str) -> Result<(), ClipboardError>;
}

/// The production sink: OSC 52 to any `Write` handle (stdout live, a
/// `Vec<u8>` in tests — the written-bytes assertion).
pub struct Osc52Sink<W: Write> {
    handle: std::sync::Mutex<W>,
}

impl<W: Write> Osc52Sink<W> {
    #[must_use]
    pub fn new(handle: W) -> Self {
        Self { handle: std::sync::Mutex::new(handle) }
    }
}

impl<W: Write> ClipboardSink for Osc52Sink<W> {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        if text.len() > OSC52_MAX_BYTES {
            return Err(ClipboardError::TooLarge { bytes: text.len() });
        }
        // v2 strips NUL before any destination (OpenTUI rejects it).
        let cleaned = if text.contains('\0') { text.replace('\0', "") } else { text.to_owned() };
        let sequence = osc52_sequence(&cleaned);
        let mut handle = self.handle.lock().map_err(|_| ClipboardError::Io("locked".into()))?;
        handle.write_all(&sequence).map_err(|error| ClipboardError::Io(error.to_string()))?;
        handle.flush().map_err(|error| ClipboardError::Io(error.to_string()))?;
        Ok(())
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

    /// The sink writes the sequence to the handle (accumulating, like
    /// stdout) and refuses oversized payloads (v2's limit-exceeded
    /// path → error toast).
    #[test]
    fn sink_writes_bytes_and_refuses_oversized() {
        let sink = Osc52Sink::new(Vec::new());
        sink.write("hello").expect("writes");
        let expected = [osc52_sequence("hello")].concat();
        assert_eq!(sink.handle.lock().unwrap().clone(), expected);

        let huge = "x".repeat(OSC52_MAX_BYTES + 1);
        assert_eq!(sink.write(&huge), Err(ClipboardError::TooLarge { bytes: OSC52_MAX_BYTES + 1 }));
        // The refusal wrote nothing.
        assert_eq!(sink.handle.lock().unwrap().clone(), expected);

        // NUL is stripped before encoding (v2 parity); the second
        // write appends after the first (stdout-like).
        sink.write("a\0b").expect("writes");
        let expected = [osc52_sequence("hello"), osc52_sequence("ab")].concat();
        assert_eq!(sink.handle.lock().unwrap().clone(), expected);
    }
}
