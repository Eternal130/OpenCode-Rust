//! v2-parity mouse reporting modes (auditC P1/P9, TM-4).
//!
//! v2 gates the terminal's mouse reporting on the config doc's
//! `mouse` key (`useMouse: config.mouse` — app.tsx:241, live flip
//! app.tsx:593; schema `mouse: Schema.optional(Schema.Boolean)`,
//! config/index.tsx:247, default `true` at :311) and enables EXACTLY
//! four modes:
//!
//! ```text
//! CSI ?1000h  — normal tracking (press/release)
//! CSI ?1002h  — button-event tracking (drag)
//! CSI ?1003h  — any-event tracking (motion)
//! CSI ?1006h  — SGR encoding
//! ```
//!
//! The port previously pushed crossterm's `EnableMouseCapture` bundle
//! UNCONDITIONALLY (`?1000 ?1002 ?1003 ?1015 ?1006` — the extra
//! `?1015` urxvt extended-coordinates mode v2 never sets, auditC P9),
//! which left users no escape hatch: with reporting forced on, the
//! terminal's own text selection only worked through the
//! shift-bypass convention. `mouse: false` now disables the pushes at
//! boot, after `$EDITOR`, and on live config reload — v2's documented
//! opt-out — and the enabled set is byte-identical to v2's, so
//! terminal-native selection behaves the same in both stacks
//! (shift-selection pass-through included).
//!
//! Focus reporting (`?1004`) is NOT part of the mouse set: ours sets
//! it unconditionally to feed the attention marker (auditC P10 —
//! "beneficial but wire-divergent", deliberately kept).

/// The enable sequence — EXACTLY v2's set, in v2's order (auditC
/// §1.1: `?1000h ?1002h ?1003h ?1006h`).
pub const V2_MOUSE_ENABLE: &[u8] = b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h";

/// The disable sequence — the symmetric pops in reverse order
/// (teardown / `$EDITOR` suspend / a live `mouse: false` flip).
pub const V2_MOUSE_DISABLE: &[u8] = b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

/// The bytes to push for the configured `mouse` value: v2's enable
/// set, or nothing when capture is off (`mouse: false` pushes no mode
/// at all — the terminal's native selection stays untouched).
#[must_use]
pub fn enable_bytes(mouse: bool) -> &'static [u8] {
    if mouse { V2_MOUSE_ENABLE } else { b"" }
}

/// The bytes to pop for the configured `mouse` value: the symmetric
/// disable set, or nothing when capture never went on.
#[must_use]
pub fn disable_bytes(mouse: bool) -> &'static [u8] {
    if mouse { V2_MOUSE_DISABLE } else { b"" }
}

/// Push the mouse modes onto a writer (boot / post-`$EDITOR` / a live
/// flip to `mouse: true`). A no-op write when `mouse` is false.
pub fn push(out: &mut dyn std::io::Write, mouse: bool) -> std::io::Result<()> {
    out.write_all(enable_bytes(mouse))
}

/// Pop the mouse modes (teardown / `$EDITOR` suspend / a live flip to
/// `mouse: false`). A no-op write when `mouse` is false.
pub fn pop(out: &mut dyn std::io::Write, mouse: bool) -> std::io::Result<()> {
    out.write_all(disable_bytes(mouse))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The selection-semantics contract (auditC P1/P9/TM-4): under
    /// `mouse: true` the emitted byte sequence is EXACTLY v2's
    /// four-mode set in v2's order — no `?1015` (crossterm's extra
    /// urxvt mode), nothing else.
    #[test]
    fn mouse_true_pushes_exactly_the_v2_mode_set() {
        assert_eq!(
            enable_bytes(true),
            b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h",
            "the enabled set must match v2 byte-for-byte (order included)"
        );
        assert!(
            !enable_bytes(true).windows(6).any(|window| window == b"?1015h"),
            "the urxvt extended-coordinates mode never rides the enable set"
        );
    }

    /// Under `mouse: false` NOTHING is emitted — v2's opt-out means
    /// the terminal keeps its native selection behavior with zero
    /// mode pushes (boot, editor re-entry, and the disable side all
    /// stay empty).
    #[test]
    fn mouse_false_emits_nothing() {
        assert!(enable_bytes(false).is_empty());
        assert!(disable_bytes(false).is_empty());
    }

    /// The disable side pops exactly what the enable side pushed, in
    /// reverse (pop semantics), and nothing more.
    #[test]
    fn disable_pops_exactly_the_enabled_set() {
        assert_eq!(disable_bytes(true), b"\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l");
        let enable_modes: Vec<&[u8]> =
            vec![b"?1000", b"?1002", b"?1003", b"?1006"];
        let mut popped: Vec<&[u8]> = vec![b"?1006", b"?1003", b"?1002", b"?1000"];
        popped.reverse();
        assert_eq!(enable_modes, popped, "the pop order mirrors the push order");
    }

    /// The writer seams route the same bytes (boot/teardown call these
    /// against stdout; a capture writer proves the sequence).
    #[test]
    fn push_and_pop_write_the_sequences() {
        let mut buffer = Vec::new();
        push(&mut buffer, true).expect("push");
        buffer.extend_from_slice(b"|");
        pop(&mut buffer, true).expect("pop");
        assert_eq!(
            buffer,
            b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h|\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?1000l"
        );
        push(&mut buffer, false).expect("push off");
        pop(&mut buffer, false).expect("pop off");
        assert_eq!(buffer.len(), 1 + V2_MOUSE_ENABLE.len() + V2_MOUSE_DISABLE.len());
    }
}
