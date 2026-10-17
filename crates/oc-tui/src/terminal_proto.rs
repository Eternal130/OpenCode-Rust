//! Lane TAIL — auditC's protocol/robustness remainders, one module of
//! byte-exact sequences (each pinned by a test):
//!
//! * **Kitty keyboard negotiation** (TM-1/P2): boot writes the
//!   `CSI ? u` capability QUERY; the terminal's answer arrives through
//!   crossterm as `Event::KeyboardEnhancementFlags` (crossterm-0.28.1
//!   parses `CSI ? flags u` itself), and the run loop then PUSHES
//!   `CSI > 5 u` (disambiguate escape codes + report alternate keys —
//!   v2's set, app.tsx:238). Exit pops `CSI < u` (the ref never pops;
//!   the audit notes we should).
//! * **Live theme-flip subscriptions** (TM-2/P6): boot writes
//!   `CSI ?2027h` (color-scheme change reports) + `CSI ?2031h` (mode
//!   reports). The TUI-layer `?997;1n/2n` notifications cannot reach
//!   us through crossterm (its CSI parser dispatches only `?u`/`?c` —
//!   an unparsed `?997;1n` is swallowed), so the behavioral stand-in
//!   is a focus-triggered OSC 11 re-probe (`theme::reprobe_system_mode`)
//!   with a cooldown: a terminal light/dark flip lands on the next
//!   focus, no restart.
//! * **Cursor styling** (TM-3/P7): boot writes DECSCUSR (`CSI {n} SP
//!   q` — block/underline/line × blink/steady, the `cursor` config
//!   doc's shape, config/index.tsx:263-266) + `OSC 12 ; #rrggbb` (the
//!   theme's cursor color); teardown resets both.
//! * **Clipboard READ side** (P8): host tools first (`pbpaste` /
//!   `xclip -o` / `xsel` / `wl-paste` — v2's host-first layering),
//!   OSC 52 query (`OSC 52 ; c ; ?`) as the terminal arm, and the
//!   image-magic sniff for the paste-image path.
//! * **Panic/SIGHUP-safe restore** (P3): a static restore sequence +
//!   a `write(2)`-only signal handler (SIGHUP/SIGTERM — async-signal
//!   safe) and a chained panic hook, so a crash mid-render can never
//!   leave raw mode / alt screen / mouse / kitty latched.
//! * **Editor-suspend handoff** (P12) and the **kitty graphics
//!   teardown** (P13 — the promised `a=d,d=a` delete-all, actually
//!   emitted) are consumed from [`crate::app`].

// The signal/poll paths need raw FFI (write/poll/signal) — allowed at
// the module boundary, each block documented.
#![allow(unsafe_code)]

use std::io::Write;

/// The kitty keyboard capability query (boot; the answer surfaces as
/// `Event::KeyboardEnhancementFlags`).
pub const KITTY_QUERY: &[u8] = b"\x1b[?u";
/// The push (flags 5 = `DISAMBIGUATE_ESCAPE_CODES` |
/// `REPORT_ALTERNATE_KEYS` — v2's `CSI > 5u`, auditC §1.1).
pub const KITTY_PUSH: &[u8] = b"\x1b[>5u";
/// The pop (exit — one level).
pub const KITTY_POP: &[u8] = b"\x1b[<u";
/// The theme-flip subscriptions: `?2027h` color-scheme reports +
/// `?2031h` mode reports (v2 boot, auditC P6/TM-2).
pub const THEME_SUBSCRIPTIONS: &[u8] = b"\x1b[?2027h\x1b[?2031h";
/// DECSCUSR reset (teardown).
pub const CURSOR_RESET: &[u8] = b"\x1b[0 q";
/// The kitty graphics delete-all (teardown — auditC P13's promised
/// but never-emitted `a=d,d=a`).
pub const KITTY_GRAPHICS_DELETE_ALL: &[u8] = b"\x1b_Ga=d,d=a\x1b\\";
/// Bracketed paste + focus reporting — the pair the editor suspend
/// must pop before `$EDITOR` and re-push after (auditC P12).
pub const PASTE_FOCUS_OFF: &[u8] = b"\x1b[?2004l\x1b[?1004l";
pub const PASTE_FOCUS_ON: &[u8] = b"\x1b[?2004h\x1b[?1004h";

/// The `cursor.style` doc's shape (config/index.tsx:263-266).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorConfig {
    /// `block` | `underline` | `line` | `default`.
    pub style: CursorShape,
    /// `blinking` (default true — v2 emits `CSI 1 SP q`, blink block).
    pub blinking: bool,
}

/// The four `cursor.style` literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Line,
    Default,
}

impl CursorShape {
    /// Parse the config literal (unknown → the v2 default `block`).
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "underline" => Self::Underline,
            "line" => Self::Line,
            "default" => Self::Default,
            _ => Self::Block,
        }
    }

    /// The DECSCUSR selector: 1/2 block, 3/4 underline, 5/6 line
    /// (blink/steady), 0 the terminal default.
    #[must_use]
    pub fn decscusr(self, blinking: bool) -> u8 {
        match (self, blinking) {
            (Self::Block, true) => 1,
            (Self::Block, false) => 2,
            (Self::Underline, true) => 3,
            (Self::Underline, false) => 4,
            (Self::Line, true) => 5,
            (Self::Line, false) => 6,
            (Self::Default, _) => 0,
        }
    }
}

/// DECSCUSR — the cursor shape/blink selector (`CSI {n} SP q`).
#[must_use]
pub fn decscuru_sequence(config: CursorConfig) -> Vec<u8> {
    format!("\x1b[{} q", config.style.decscusr(config.blinking)).into_bytes()
}

/// OSC 12 — the cursor color (`OSC 12 ; #rrggbb ST`).
#[must_use]
pub fn osc12_sequence(color: (u8, u8, u8)) -> Vec<u8> {
    format!("\x1b]12;#{:02x}{:02x}{:02x}\x1b\\", color.0, color.1, color.2).into_bytes()
}

/// Write a byte sequence to stdout (best-effort; the caller flushes).
pub fn emit(bytes: &[u8]) {
    let mut out = std::io::stdout();
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

// ---------------------------------------------------------------------------
// panic / signal-safe restore (P3)
// ---------------------------------------------------------------------------

/// The mouse-capture state the restore sequences must respect (the
/// signal handler and panic hook read this — they cannot capture).
static MOUSE_APPLIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The thread that owns the terminal (the run loop's). The PANIC hook
/// restores only for THIS thread's panics: a render-path crash takes
/// the terminal down with it (the crash-restore contract, auditC P3),
/// while worker-thread panics are routinely CAUGHT (the plugin host's
/// `catch_unwind`, harness crashers) and must NOT tear the terminal
/// out from under the still-running loop.
static TERMINAL_THREAD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The current thread's id as a plain `u64`.
#[must_use]
fn current_thread_key() -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let id = std::thread::current().id();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

/// The full restore sequence for a crash mid-render: leave the alt
/// screen, pop focus/bracketed-paste, pop the mouse set if it went
/// on, pop kitty, reset the cursor. Mouse-off boots skip the mouse
/// pops (v2 pops what the renderer pushed).
#[must_use]
pub fn crash_restore_sequence() -> &'static [u8] {
    const WITH_MOUSE: &[u8] =
        b"\x1b[?1049l\x1b[?1004l\x1b[?2004l\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[<u\x1b[0 q";
    const WITHOUT_MOUSE: &[u8] = b"\x1b[?1049l\x1b[?1004l\x1b[?2004l\x1b[<u\x1b[0 q";
    if MOUSE_APPLIED.load(std::sync::atomic::Ordering::Relaxed) {
        WITH_MOUSE
    } else {
        WITHOUT_MOUSE
    }
}

/// The async-signal-safe crash exit: `write(2)` the restore bytes to
/// fd 1, then `_exit` (no allocations, no locks — SIGHUP/SIGTERM).
#[allow(unsafe_code)] // the signal path is write(2)+_exit(2) by design
fn crash_exit() -> ! {
    let bytes = crash_restore_sequence();
    unsafe {
        // Best-effort write loop (EINTR-safe): the terminal gets the
        // bytes or the process dies trying — either way nothing stays
        // latched that a `reset` wouldn't fix.
        let mut written = 0usize;
        while written < bytes.len() {
            let n = libc::write(1, bytes[written..].as_ptr().cast(), bytes.len() - written);
            if n <= 0 {
                break;
            }
            written += n.cast_unsigned();
        }
        libc::_exit(1);
    }
}

extern "C" fn signal_restore_handler(_signal: i32) {
    crash_exit();
}

/// Install the SIGHUP/SIGTERM restore handlers + the chained panic
/// hook (P3): a crash never leaves raw mode / alt screen / mouse /
/// kitty latched. `mouse` seeds [`MOUSE_APPLIED`]; the live config
/// flip keeps it current through [`set_mouse_applied`]. The panic
/// hook restores ONLY when the panicking thread owns the terminal
/// (the installer thread — the run loop): worker-thread panics are
/// routinely caught (the plugin host) and must not tear the terminal
/// from the running loop.
pub fn install_crash_restore(mouse: bool) {
    set_mouse_applied(mouse);
    TERMINAL_THREAD.store(current_thread_key(), std::sync::atomic::Ordering::Relaxed);
    // SAFETY: the handler is async-signal-safe (write+_exit only) and
    // installed once at boot before any thread reads the static.
    unsafe {
        let handler = signal_restore_handler as *const () as libc::sighandler_t;
        libc::signal(libc::SIGHUP, handler);
        libc::signal(libc::SIGTERM, handler);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore the terminal FIRST (a panicked renderer may hold a
        // broken frame; the shell must come back regardless), then
        // chain to the previous hook (the stderr guard's report) —
        // but only when THIS thread owns the terminal.
        if current_thread_key() == TERMINAL_THREAD.load(std::sync::atomic::Ordering::Relaxed) {
            emit(crash_restore_sequence());
        }
        previous(info);
    }));
}

/// Keep the restore sequences in step with the live mouse flip.
pub fn set_mouse_applied(mouse: bool) {
    MOUSE_APPLIED.store(mouse, std::sync::atomic::Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// clipboard read side (P8)
// ---------------------------------------------------------------------------

/// Read the system clipboard, HOST TOOLS FIRST (v2's layered adapter:
/// `pbpaste` / `xclip -o` / `xsel --clipboard --output` / `wl-paste`),
/// then the terminal arm (OSC 52 query with a short raw window).
/// Returns raw bytes — text or image (see [`sniff_image`]).
#[must_use]
pub fn clipboard_read() -> Option<Vec<u8>> {
    clipboard_read_host().or_else(clipboard_read_osc52)
}

/// The host-tool arm: run the first available paste tool.
#[must_use]
pub fn clipboard_read_host() -> Option<Vec<u8>> {
    const TOOLS: [&[&str]; 4] = [
        &["pbpaste"],
        &["xclip", "-o", "-selection", "clipboard"],
        &["xsel", "--clipboard", "--output"],
        &["wl-paste", "--no-newline"],
    ];
    for tool in TOOLS {
        if let Ok(output) = std::process::Command::new(tool[0]).args(&tool[1..]).output() {
            if output.status.success() {
                // A present tool that answered EMPTY is a valid empty
                // read — keep probing the next host tool (v2 prefers
                // the host layer when it exists; empty falls through
                // to the terminal arm in the caller).
                if !output.stdout.is_empty() {
                    return Some(output.stdout);
                }
            }
        }
    }
    None
}

/// The terminal arm: `OSC 52 ; c ; ?` + a 150 ms raw read window (the
/// same probe discipline as the boot OSC 11 background query — the
/// caller must own the terminal, i.e. run between frames).
#[must_use]
pub fn clipboard_read_osc52() -> Option<Vec<u8>> {
    use std::io::{IsTerminal, Read as _, Write as _};
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return None;
    }
    // Raw ON for the window (probe discipline: idempotent when the
    // loop already holds it; restored below).
    let _ = crossterm::terminal::enable_raw_mode();
    let answer = (|| {
        {
            let mut stdout = std::io::stdout().lock();
            if stdout.write_all(b"\x1b]52;c;?\x07").is_err() || stdout.flush().is_err() {
                return None;
            }
        }
        // Read until BEL/ST with a bounded window: terminal answers
        // arrive in one burst; anything longer is not an OSC 52 reply.
        let mut buffer = [0u8; 8192];
        let mut collected: Vec<u8> = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
        while collected.len() < 100 * 1024 {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let mut fds = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
            let timeout_ms =
                i32::try_from(remaining.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
            // SAFETY: poll(2) on fd 0 with a computed timeout — the
            // probe window's bounded wait.
            let ready = unsafe { libc::poll(std::ptr::from_mut(&mut fds), 1, timeout_ms) };
            if ready <= 0 {
                break;
            }
            let n = std::io::stdin().lock().read(&mut buffer).ok()?;
            if n == 0 {
                break;
            }
            collected.extend_from_slice(&buffer[..n]);
            if collected.windows(2).any(|w| w == b"\x07" || w == b"\x1b\\") {
                break;
            }
        }
        parse_osc52_reply(&collected)
    })();
    let _ = crossterm::terminal::disable_raw_mode();
    answer
}

/// Parse `OSC 52 ; c ; <base64|?> BEL|ST` out of the read window;
/// `?` (refused) and anything unparsable are `None`.
#[must_use]
pub fn parse_osc52_reply(bytes: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find("]52;")?;
    let rest = &text[start + 4..];
    // Skip the selection param (`c;`).
    let semi = rest.find(';')?;
    let payload = &rest[semi + 1..];
    let payload = payload.trim_end_matches(['\x07', '\x1b', '\\']);
    if payload == "?" || payload.is_empty() {
        return None;
    }
    // Base64 padding terminators are optional in our decoder; OSC 52
    // replies carry padded standard alphabet.
    crate::clipboard::base64_decode(payload.trim_end_matches('='))
}

/// The sniffed image's MIME (the attachment chip's data-URI type).
#[must_use]
pub fn image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else {
        "image/webp"
    }
}

/// The image sniff (the paste-image path's gate, P8): PNG / JPEG /
/// GIF / WEBP magic prefixes.
#[must_use]
pub fn sniff_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-exact sequences (the wire assertions).
    #[test]
    fn protocol_sequences_are_byte_exact() {
        assert_eq!(KITTY_QUERY, b"\x1b[?u");
        assert_eq!(KITTY_PUSH, b"\x1b[>5u");
        assert_eq!(KITTY_POP, b"\x1b[<u");
        assert_eq!(THEME_SUBSCRIPTIONS, b"\x1b[?2027h\x1b[?2031h");
        assert_eq!(CURSOR_RESET, b"\x1b[0 q");
        assert_eq!(KITTY_GRAPHICS_DELETE_ALL, b"\x1b_Ga=d,d=a\x1b\\");
        assert_eq!(PASTE_FOCUS_OFF, b"\x1b[?2004l\x1b[?1004l");
        assert_eq!(PASTE_FOCUS_ON, b"\x1b[?2004h\x1b[?1004h");
    }

    /// DECSCUSR selectors: the style × blink matrix (kitty/XTerm
    /// semantics), the `default` literal mapping to 0.
    #[test]
    fn decscusr_covers_the_style_blink_matrix() {
        let shape = |style: CursorShape, blink: bool| {
            let seq = decscuru_sequence(CursorConfig { style, blinking: blink });
            let text = std::str::from_utf8(&seq).unwrap();
            text.trim_start_matches("\x1b[").trim_end_matches(" q").to_owned()
        };
        assert_eq!(shape(CursorShape::Block, true), "1");
        assert_eq!(shape(CursorShape::Block, false), "2");
        assert_eq!(shape(CursorShape::Underline, true), "3");
        assert_eq!(shape(CursorShape::Underline, false), "4");
        assert_eq!(shape(CursorShape::Line, true), "5");
        assert_eq!(shape(CursorShape::Line, false), "6");
        assert_eq!(shape(CursorShape::Default, true), "0");
        assert_eq!(CursorShape::parse("underline"), CursorShape::Underline);
        assert_eq!(CursorShape::parse("nonsense"), CursorShape::Block);
    }

    /// OSC 12 — the theme cursor color as `#rrggbb`.
    #[test]
    fn osc12_formats_the_cursor_color() {
        assert_eq!(osc12_sequence((0x12, 0x34, 0xab)), b"\x1b]12;#1234ab\x1b\\".to_vec());
    }

    /// The crash-restore sequences respect the mouse gate.
    #[test]
    fn crash_restore_respects_the_mouse_gate() {
        set_mouse_applied(false);
        let without = crash_restore_sequence();
        assert!(without.starts_with(b"\x1b[?1049l"));
        assert!(!without.windows(4).any(|w| w == b"1000"));
        set_mouse_applied(true);
        let with = crash_restore_sequence();
        assert!(with.windows(6).any(|w| w == b"?1000l"));
        assert!(with.ends_with(b"\x1b[0 q"));
        set_mouse_applied(false);
    }

    /// The OSC 52 reply parser: payload, refusal (`?`), garbage.
    #[test]
    fn osc52_reply_parses_payload_and_refusal() {
        // "hi" → aGk=
        let reply = b"\x1b]52;c;aGk=\x07";
        assert_eq!(parse_osc52_reply(reply), Some(b"hi".to_vec()));
        // ST terminator.
        assert_eq!(parse_osc52_reply(b"\x1b]52;c;aGk=\x1b\\"), Some(b"hi".to_vec()));
        // Refused.
        assert_eq!(parse_osc52_reply(b"\x1b]52;c;?\x07"), None);
        // Not an OSC 52 line at all.
        assert_eq!(parse_osc52_reply(b"\x1b[?997;1n"), None);
    }

    /// The image magic sniff.
    #[test]
    fn image_sniff_matches_the_four_magics() {
        assert!(sniff_image(b"\x89PNG\r\n\x1a\nrest"));
        assert!(sniff_image(b"\xff\xd8\xff\xe0jpeg"));
        assert!(sniff_image(b"GIF89a..."));
        assert!(sniff_image(b"RIFF\x00\x00\x00\x00WEBPVP8"));
        assert!(!sniff_image(b"plain text"));
        assert!(!sniff_image(b""));
    }
}
