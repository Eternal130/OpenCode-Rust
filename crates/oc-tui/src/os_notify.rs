//! OS notifications over terminal escape sequences — the port of
//! opentui's native `renderer.triggerNotification`
//! (`@opentui/core`, the libghostty-based renderer).
//!
//! The native renderer speaks three desktop-notification protocols
//! (protocol names + env overrides verified against the 0.5.14 native
//! binary's strings: `osc9`, `osc99`, `osc777`,
//! `OPENTUI_NOTIFICATION_PROTOCOL`, `OPENTUI_NOTIFICATIONS`):
//!
//! * **osc9** — `ESC ] 9 ; message BEL` (iTerm2 growl-style; ghostty,
//!   iTerm2). Title-less: the message is the payload.
//! * **osc777** — `ESC ] 777 ; notify ; title ; message BEL` (rxvt;
//!   foot, alacritty, wezterm, kitty). Carries both fields.
//! * **osc99** — kitty's notification protocol. The native binary's
//!   template is `]99;i=<id>:p=:e=1:d=` with the id
//!   `opentui-notifications`; our mirror carries the message in the
//!   `d=` field (title-less, like osc9). Auto never selects it — kitty
//!   also implements osc777 — but the env override accepts it.
//!
//! `auto` resolution: the native renderer negotiates in-process; this
//! port picks from `TERM`/`TERM_PROGRAM` (ghostty/iTerm → osc9,
//! everything else → osc777 — the broadest matrix). Override with
//! `OPENTUI_NOTIFICATION_PROTOCOL=osc9|osc99|osc777|auto`;
//! `OPENTUI_NOTIFICATIONS=0|false|off|no` disables the channel
//! entirely (both mirror the native env knobs).
//!
//! Payloads are already normalized by [`crate::attention::normalize_text`];
//! [`escape_osc_payload`] additionally drops `BEL`/`ESC`/C1 bytes so a
//! hostile message cannot terminate the sequence early.

use std::io::Write;

/// The notification protocols the native renderer implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotifyProtocol {
    /// Terminal-type detection (see [`NotifyProtocol::resolve`]).
    #[default]
    Auto,
    Osc9,
    Osc99,
    Osc777,
}

impl NotifyProtocol {
    /// Parse `OPENTUI_NOTIFICATION_PROTOCOL` (`auto` / unknown →
    /// `Auto`, mirroring the native fallback).
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value {
            Some("osc9") => Self::Osc9,
            Some("osc99") => Self::Osc99,
            Some("osc777") => Self::Osc777,
            _ => Self::Auto,
        }
    }

    /// Resolve `Auto` against the host terminal: ghostty and iTerm2
    /// speak osc9 natively; everything else (kitty, foot, alacritty,
    /// wezterm, rxvt, and unknown terminals incl. tmux) gets osc777 —
    /// the most widely implemented pair.
    #[must_use]
    pub fn resolve(self) -> Self {
        match self {
            Self::Auto => {
                let term = std::env::var("TERM").unwrap_or_default();
                let term_program = std::env::var("TERM_PROGRAM").unwrap_or_default();
                let osc9_host = term_program.contains("ghostty")
                    || term.contains("ghostty")
                    || term_program == "iTerm.app"
                    || term_program == "Apple_Terminal";
                if osc9_host {
                    Self::Osc9
                } else {
                    Self::Osc777
                }
            }
            resolved => resolved,
        }
    }
}

/// `OPENTUI_NOTIFICATIONS` kill-switch (`0|false|off|no` → disabled;
/// absent/unrecognized → enabled — mirrors the native env knob).
#[must_use]
pub fn notifications_env_enabled() -> bool {
    match std::env::var("OPENTUI_NOTIFICATIONS") {
        Ok(value) => !matches!(value.trim_ignorecase().as_str(), "0" | "false" | "off" | "no"),
        Err(_) => true,
    }
}

/// Case-insensitive `str::trim`+lowercase helper for env values.
trait TrimIgnorecase {
    fn trim_ignorecase(&self) -> String;
}

impl TrimIgnorecase for str {
    fn trim_ignorecase(&self) -> String {
        self.trim().to_ascii_lowercase()
    }
}

/// Drop `BEL`/`ESC`/ST/C1 controls from an OSC payload field — they
/// would terminate or corrupt the enclosing sequence.
#[must_use]
pub fn escape_osc_payload(value: &str) -> String {
    value
        .chars()
        .filter(|ch| {
            let code = *ch as u32;
            !matches!(code, 0x00..=0x1f | 0x7f..=0x9f)
        })
        .collect()
}

/// The OSC 9 sequence (message-only).
#[must_use]
pub fn osc9_sequence(message: &str) -> String {
    format!("\x1b]9;{}\x07", escape_osc_payload(message))
}

/// The OSC 777 sequence (`notify;title;message`).
#[must_use]
pub fn osc777_sequence(title: &str, message: &str) -> String {
    format!("\x1b]777;notify;{};{}\x07", escape_osc_payload(title), escape_osc_payload(message))
}

/// The OSC 99 (kitty) sequence — the native binary's template
/// (`i=opentui-notifications:p=:e=1:d=`) with the message in `d=`.
#[must_use]
pub fn osc99_sequence(message: &str) -> String {
    format!("\x1b]99;i=opentui-notifications:p=:e=1:d={}\x07", escape_osc_payload(message))
}

/// The resolved escape sequence for one notification.
#[must_use]
pub fn notification_sequence(protocol: NotifyProtocol, title: &str, message: &str) -> String {
    match protocol.resolve() {
        // Title-less protocols carry the message alone.
        NotifyProtocol::Osc9 => osc9_sequence(message),
        NotifyProtocol::Osc99 => osc99_sequence(message),
        // The title-carrying protocol (`auto` resolves into one of the
        // three above, never staying `Auto`).
        NotifyProtocol::Osc777 | NotifyProtocol::Auto => osc777_sequence(title, message),
    }
}

/// The production OS-notification half of [`AttentionEmitter`]:
/// resolves the protocol once, writes the sequence to the writer the
/// run loop drains (stdout), flushes, reports success.
pub struct OscNotifier<W: Write> {
    protocol: NotifyProtocol,
    out: W,
}

impl<W: Write> OscNotifier<W> {
    /// From the ambient environment (`OPENTUI_NOTIFICATION_PROTOCOL` +
    /// terminal detection) over `out`.
    pub fn from_env(out: W) -> Self {
        let protocol = NotifyProtocol::from_env_value(
            std::env::var("OPENTUI_NOTIFICATION_PROTOCOL").ok().as_deref(),
        );
        Self { protocol, out }
    }

    /// Write one notification; `false` on I/O failure or when
    /// `OPENTUI_NOTIFICATIONS` disables the channel.
    pub fn trigger(&mut self, title: &str, message: &str) -> bool {
        if !notifications_env_enabled() {
            return false;
        }
        let sequence = notification_sequence(self.protocol, title, message);
        self.out.write_all(sequence.as_bytes()).is_ok() && self.out.flush().is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_env_parsing() {
        assert_eq!(NotifyProtocol::from_env_value(Some("osc9")), NotifyProtocol::Osc9);
        assert_eq!(NotifyProtocol::from_env_value(Some("osc99")), NotifyProtocol::Osc99);
        assert_eq!(NotifyProtocol::from_env_value(Some("osc777")), NotifyProtocol::Osc777);
        assert_eq!(NotifyProtocol::from_env_value(Some("auto")), NotifyProtocol::Auto);
        assert_eq!(NotifyProtocol::from_env_value(Some("bogus")), NotifyProtocol::Auto);
        assert_eq!(NotifyProtocol::from_env_value(None), NotifyProtocol::Auto);
    }

    #[test]
    fn sequences_match_the_protocols() {
        assert_eq!(osc9_sequence("done"), "\x1b]9;done\x07");
        assert_eq!(
            osc777_sequence("OpenCode", "Session done"),
            "\x1b]777;notify;OpenCode;Session done\x07"
        );
        assert_eq!(
            osc99_sequence("Session done"),
            "\x1b]99;i=opentui-notifications:p=:e=1:d=Session done\x07"
        );
    }

    /// Payload sanitizing: BEL/ESC/C1 cannot break out of the sequence.
    #[test]
    fn payload_escapes_control_bytes() {
        assert_eq!(escape_osc_payload("a\x07b"), "ab");
        assert_eq!(escape_osc_payload("a\x1bb"), "ab");
        assert_eq!(escape_osc_payload("a\u{9c}b"), "ab");
        assert_eq!(escape_osc_payload("plain text: ok;"), "plain text: ok;");
    }

    /// The notifier writes the resolved sequence + flushes.
    #[test]
    fn notifier_writes_and_flushes() {
        let mut buffer: Vec<u8> = Vec::new();
        let mut notifier = OscNotifier { protocol: NotifyProtocol::Osc777, out: &mut buffer };
        assert!(notifier.trigger("T", "M"));
        assert_eq!(buffer, b"\x1b]777;notify;T;M\x07");
    }
}
