//! Terminal-environment matrix (lane/termatrix) — the TERM/protocol
//! negotiation dimension the base rig never varied.
//!
//! The base rig always ran ONE terminal environment:
//! `TERM=xterm-256color`, no `COLORTERM`, and a silent PTY (no DA1 /
//! kitty / OSC-11 answers — a terminal that never replies to
//! capability queries). Every TERM-conditional surface (SGR color
//! downgrade, mouse protocol choice, kitty keyboard negotiation,
//! clipboard OSC, cursor shape) was therefore untested against the
//! pinned v2.0.24 reference.
//!
//! This module adds:
//!
//! * [`TermProfile`] — a named terminal environment: env overrides
//!   (`TERM`/`COLORTERM`) plus a canned reply table for the terminal
//!   capability queries a TUI sends at boot (DA1, DA2, XTVERSION,
//!   kitty keyboard `CSI ? u`, OSC 11 background, DECRQM sync-2026).
//! * [`PtyResponder`] — a background thread per PTY that scans the
//!   CHILD→TERMINAL byte stream for those queries and writes the
//!   profile's replies back into the master (the child reads them as
//!   terminal answers). Attached to BOTH sides — the ref and our TUI
//!   must SEE the same terminal answers.
//! * tmux wrapping ([`wrap_argv_for_tmux`]) — run a TUI command
//!   inside a REAL `tmux 3.4` session on the PTY (tmux answers the
//!   inner queries itself and applies its own screen-256color
//!   semantics: that IS the environment under test).
//! * [`protocol_signature`] — classify the PROTOCOL sequences in a
//!   raw child stream (private modes h/l, kitty push/pop/query,
//!   cursor shape, clipboard OSC 52, notifications, DA queries,
//!   SGR truecolor-vs-index emission counts) into a comparable
//!   multiset. Grid identity cannot see protocol negotiation — this
//!   is the evidence channel for it.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

/// One terminal environment for the matrix.
#[derive(Debug, Clone)]
pub struct TermProfile {
    /// Registry id (`--term-profile NAME`).
    pub id: &'static str,
    /// What the environment models.
    pub description: &'static str,
    /// `TERM` for both TUIs.
    pub term: &'static str,
    /// `COLORTERM` for both TUIs (absent when `None`).
    pub colorterm: Option<&'static str>,
    /// Reply to DA1 (`CSI c`) — full escape sequence, `None` = silent.
    pub da1: Option<&'static str>,
    /// Reply to DA2 (`CSI > c`) — full escape sequence.
    pub da2: Option<&'static str>,
    /// Reply to XTVERSION (`CSI > q`) — DCS form.
    pub xtversion: Option<&'static str>,
    /// Reply to the kitty keyboard query (`CSI ? u`) — `CSI ? flags u`
    /// form; `None` = the terminal does NOT advertise the protocol.
    pub kitty: Option<&'static str>,
    /// OSC 11 background reply (full sequence, ST-terminated);
    /// `None` = silent (theme probe falls back to dark on both sides).
    pub osc11: Option<&'static str>,
    /// OSC 10 foreground reply (full sequence, ST-terminated);
    /// `None` = silent.
    pub osc10: Option<&'static str>,
    /// OSC 4 palette-slot-0 query reply (`OSC 4;0;?`); `None` = silent.
    pub osc4: Option<&'static str>,
    /// Reply to DECRQM `CSI ? 2026 $ p` — full sequence (`;2$y` =
    /// supported, `;0$y` = unsupported).
    pub sync: Option<&'static str>,
    /// Wrap the TUI inside a real tmux session (profile `tmux`).
    pub tmux: bool,
}

/// The matrix. Reply bytes model the real terminals:
/// * xterm(371): DA1 `?64;…c`, DA2 `>41;371;0c`, no kitty.
/// * screen 5.0: DA1 `?83;0c` (the classic screen reply), no kitty,
///   sync-2026 unsupported.
/// * kitty 0.42: DA1 `?62;…c` (VT520 class + the kitty attribute set),
///   the full progressive-enhancement flag set in the `CSI ? u`
///   answer (`1+2+4+8+16+32+64+128+256` → `511`), sync-2026 supported.
const PROFILES: &[TermProfile] = &[
    TermProfile {
        id: "xterm256-silent",
        description:
            "BASELINE — the rig as it always ran: TERM=xterm-256color, a PTY that never answers any capability query",
        term: "xterm-256color",
        colorterm: None,
        da1: None,
        da2: None,
        xtversion: None,
        kitty: None,
        osc11: None,
        osc10: None,
        osc4: None,
        sync: None,
        tmux: false,
    },
    TermProfile {
        id: "xterm256",
        description:
            "a real xterm-256color: xterm(371) DA1/DA2/XTVERSION answers, OSC 11 dark background, sync-2026 supported, kitty keyboard NOT advertised",
        term: "xterm-256color",
        colorterm: None,
        da1: Some("\x1b[?64;1;2;6;9;15;16;17;18;21;22c"),
        da2: Some("\x1b[>41;371;0c"),
        xtversion: Some("\x1bP>|xterm(371)\x1b\\"),
        kitty: None,
        osc11: Some("\x1b]11;rgb:0000/0000/0000\x1b\\"),
        osc10: Some("\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
        osc4: Some("\x1b]4;0;rgb:0000/0000/0000\x1b\\"),
        sync: Some("\x1b[?2026;2$y"),
        tmux: false,
    },
    TermProfile {
        id: "truecolor",
        description:
            "xterm-256color + COLORTERM=truecolor (the modern default): same xterm answers, OSC 11 dark, kitty NOT advertised",
        term: "xterm-256color",
        colorterm: Some("truecolor"),
        da1: Some("\x1b[?64;1;2;6;9;15;16;17;18;21;22c"),
        da2: Some("\x1b[>41;371;0c"),
        xtversion: Some("\x1bP>|xterm(371)\x1b\\"),
        kitty: None,
        osc11: Some("\x1b]11;rgb:0000/0000/0000\x1b\\"),
        osc10: Some("\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
        osc4: Some("\x1b]4;0;rgb:0000/0000/0000\x1b\\"),
        sync: Some("\x1b[?2026;2$y"),
        tmux: false,
    },
    TermProfile {
        id: "screen256",
        description:
            "TERM=screen-256color (smcup/rmcup alternate screen, margins): GNU screen 5.0 DA replies, OSC 11 dark, no kitty, sync-2026 unsupported",
        term: "screen-256color",
        colorterm: None,
        da1: Some("\x1b[?83;0c"),
        da2: Some("\x1b[>83;0;0c"),
        xtversion: Some("\x1bP>|screen(5.0.0)\x1b\\"),
        kitty: None,
        osc11: Some("\x1b]11;rgb:0000/0000/0000\x1b\\"),
        osc10: Some("\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
        osc4: Some("\x1b]4;0;rgb:0000/0000/0000\x1b\\"),
        sync: Some("\x1b[?2026;0$y"),
        tmux: false,
    },
    TermProfile {
        id: "tmux",
        description:
            "inside a REAL tmux 3.4 session (status off, escape-time 0): inner TERM=screen-256color; tmux itself answers the TUI's capability queries — the outer responder stays silent",
        term: "screen-256color",
        colorterm: None,
        da1: None,
        da2: None,
        xtversion: None,
        kitty: None,
        osc11: None,
        osc10: None,
        osc4: None,
        sync: None,
        tmux: true,
    },
    TermProfile {
        id: "xterm256-light",
        description:
            "PROBE — a LIGHT-background xterm (OSC 11 answers white): exercises the theme-mode negotiation seam (dark is the default everywhere else)",
        term: "xterm-256color",
        colorterm: None,
        da1: Some("\x1b[?64;1;2;6;9;15;16;17;18;21;22c"),
        da2: Some("\x1b[>41;371;0c"),
        xtversion: Some("\x1bP>|xterm(371)\x1b\\"),
        kitty: None,
        osc11: Some("\x1b]11;rgb:ffff/ffff/ffff\x1b\\"),
        osc10: Some("\x1b]10;rgb:0000/0000/0000\x1b\\"),
        osc4: Some("\x1b]4;0;rgb:ffff/ffff/ffff\x1b\\"),
        sync: Some("\x1b[?2026;2$y"),
        tmux: false,
    },
    TermProfile {
        id: "kitty-yes",
        description:
            "kitty keyboard protocol ADVERTISED: kitty(0.42.2) identity, full progressive-enhancement flag set in the CSI ? u answer, truecolor, OSC 11 dark, sync-2026",
        term: "xterm-kitty",
        colorterm: Some("truecolor"),
        da1: Some("\x1b[?62;6;7;13;14;15;16;17;18;19;21;22;23;24;26;27;28;29;30c"),
        da2: Some("\x1b[>1;4000;13c"),
        xtversion: Some("\x1bP>|kitty(0.42.2)\x1b\\"),
        kitty: Some("\x1b[?511u"),
        osc11: Some("\x1b]11;rgb:0000/0000/0000\x1b\\"),
        osc10: Some("\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
        osc4: Some("\x1b]4;0;rgb:0000/0000/0000\x1b\\"),
        sync: Some("\x1b[?2026;2$y"),
        tmux: false,
    },
    TermProfile {
        id: "kitty-no",
        description:
            "kitty protocol NOT advertised in an otherwise identical answering terminal: xterm answers + OSC 11 dark + sync-2026, CSI ? u stays UNANSWERED (the advertised-vs-not control)",
        term: "xterm-256color",
        colorterm: None,
        da1: Some("\x1b[?64;1;2;6;9;15;16;17;18;21;22c"),
        da2: Some("\x1b[>41;371;0c"),
        xtversion: Some("\x1bP>|xterm(371)\x1b\\"),
        kitty: None,
        osc11: Some("\x1b]11;rgb:0000/0000/0000\x1b\\"),
        osc10: Some("\x1b]10;rgb:ffff/ffff/ffff\x1b\\"),
        osc4: Some("\x1b]4;0;rgb:0000/0000/0000\x1b\\"),
        sync: Some("\x1b[?2026;2$y"),
        tmux: false,
    },
];

/// Look up a profile by id.
#[must_use]
pub fn find_term_profile(id: &str) -> Option<&'static TermProfile> {
    PROFILES.iter().find(|profile| profile.id == id)
}

/// Every registered profile id (for `--list-term-profiles`).
#[must_use]
pub fn term_profile_ids() -> Vec<&'static str> {
    PROFILES.iter().map(|profile| profile.id).collect()
}

impl TermProfile {
    /// Env overlay applied to BOTH TUIs' environments (replaces the
    /// rig's baseline `TERM=xterm-256color`).
    #[must_use]
    pub fn env_overlay(&self) -> Vec<(String, String)> {
        let mut overlay = vec![("TERM".to_owned(), self.term.to_owned())];
        if let Some(colorterm) = self.colorterm {
            overlay.push(("COLORTERM".to_owned(), colorterm.to_owned()));
        }
        overlay
    }

    /// Does this profile answer ANYTHING? (The silent profiles skip
    /// the responder thread entirely.)
    #[must_use]
    pub fn answers_queries(&self) -> bool {
        self.da1.is_some()
            || self.da2.is_some()
            || self.xtversion.is_some()
            || self.kitty.is_some()
            || self.osc11.is_some()
            || self.osc10.is_some()
            || self.osc4.is_some()
            || self.sync.is_some()
    }
}

// ---------------------------------------------------------------------------
// Query responder (terminal answers on the PTY master)
// ---------------------------------------------------------------------------

/// One (needle, reply) pair the responder serves.
type QueryRule = (&'static [u8], &'static [u8]);

/// The query needles + replies derived from a profile. Both OSC 11
/// query terminators (BEL and ST) share the reply.
#[must_use]
fn query_rules(profile: &'static TermProfile) -> Vec<QueryRule> {
    let mut rules = Vec::new();
    let mut push = |needles: &[&'static [u8]], reply: Option<&'static str>| {
        if let Some(reply) = reply {
            for needle in needles {
                rules.push((*needle, reply.as_bytes()));
            }
        }
    };
    push(&[b"\x1b[c", b"\x1b[0c"], profile.da1);
    push(&[b"\x1b[>c", b"\x1b[>0c"], profile.da2);
    push(&[b"\x1b[>q"], profile.xtversion);
    push(&[b"\x1b[?u"], profile.kitty);
    push(&[b"\x1b]11;?\x07", b"\x1b]11;?\x1b\\"], profile.osc11);
    push(&[b"\x1b]10;?\x07", b"\x1b]10;?\x1b\\"], profile.osc10);
    push(&[b"\x1b]4;0;?\x07", b"\x1b]4;0;?\x1b\\"], profile.osc4);
    push(&[b"\x1b[?2026$p"], profile.sync);
    rules
}

/// Count non-overlapping occurrences of `needle` in `hay`.
fn count_occurrences(hay: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || hay.len() < needle.len() {
        return 0;
    }
    let mut count = 0;
    let mut offset = 0;
    while let Some(found) = hay[offset..].windows(needle.len()).position(|w| w == needle) {
        count += 1;
        offset += found + needle.len();
    }
    count
}

/// The handle the rig keeps per PTY. The background thread polls the
/// shared raw mirror (child OUTPUT — where queries appear) and writes
/// replies to the shared PTY writer (child INPUT — what a real
/// terminal's answer would be). Occurrence-count dedup keeps each
/// query answered exactly once; a hard reply cap guards against a
/// pathological query loop. Stop is join-based so the PTY teardown
/// (master close → SIGHUP) keeps working.
pub struct PtyResponder {
    stop: Arc<AtomicBool>,
    replies_sent: Arc<AtomicUsize>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl PtyResponder {
    /// Attach a responder for `profile` to a PTY (raw mirror + writer).
    /// Silent profiles attach nothing.
    pub fn attach(
        profile: &'static TermProfile,
        raw: Arc<Mutex<Vec<u8>>>,
        writer: Arc<Mutex<std::fs::File>>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let replies_sent = Arc::new(AtomicUsize::new(0));
        if !profile.answers_queries() {
            return Self { stop, replies_sent, handle: None };
        }
        let rules = query_rules(profile);
        let thread_stop = Arc::clone(&stop);
        let thread_count = Arc::clone(&replies_sent);
        let handle = std::thread::spawn(move || {
            // Queries answered so far, per needle (dedup across polls).
            let mut answered: Vec<usize> = rules.iter().map(|_| 0).collect();
            // XTGETTCAP (`DCS +q hexcaps ST`) always answers "not
            // found" (`DCS 0+r ST`) — counted separately.
            let mut xtgettcap_answered = 0_usize;
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(3));
                let data = raw.lock().expect("termatrix raw").clone();
                let mut replies: Vec<&'static [u8]> = Vec::new();
                for (index, (needle, reply)) in rules.iter().enumerate() {
                    let seen = count_occurrences(&data, needle);
                    let delta = seen.saturating_sub(answered[index]);
                    answered[index] = seen;
                    for _ in 0..delta {
                        replies.push(reply);
                    }
                }
                let xtgettcap_seen = count_occurrences(&data, b"\x1bP+q");
                let delta = xtgettcap_seen.saturating_sub(xtgettcap_answered);
                xtgettcap_answered = xtgettcap_seen;
                // Same-item pushes are the point here: one reply per query.
                replies.extend(std::iter::repeat_n(b"\x1bP0+r\x1b\\".as_slice(), delta));
                for reply in replies {
                    if thread_count.load(Ordering::Relaxed) > 512 {
                        break;
                    }
                    if let Ok(mut writer) = writer.lock() {
                        let _ = writer.write_all(reply);
                        let _ = writer.flush();
                    }
                    thread_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        Self { stop, replies_sent, handle: Some(handle) }
    }

    /// Number of replies actually written (diagnostics).
    #[must_use]
    pub fn replies_sent(&self) -> usize {
        self.replies_sent.load(Ordering::Relaxed)
    }

    /// Stop the responder and join the thread (before killing the PTY
    /// child, so the master close still hangs up the session).
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for PtyResponder {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// tmux wrapping
// ---------------------------------------------------------------------------

/// Wrap one TUI command line for execution inside a real tmux session.
/// Returns `(tmux argv, socket path)` — the wrapper script + conf are
/// materialized in `work_dir`.
///
/// The wrapper script `exec`s the real argv (no shell quoting in the
/// tmux command string); the conf pins the inner `TERM` to the
/// profile's value, disables the status bar (the TUI grid must fill
/// the pane 1:1), sets escape-time 0 (keystroke pacing) and enables
/// passthrough + focus events — the closest thing to a stock
/// `tmux new-session` a human runs.
pub fn wrap_argv_for_tmux(
    profile: &TermProfile,
    program: &Path,
    argv: &[String],
    work_dir: &Path,
    side: &str,
    cols: u16,
    rows: u16,
) -> std::io::Result<(Vec<String>, PathBuf)> {
    let socket = work_dir.join(format!("tmux-{side}.sock"));
    let script = work_dir.join(format!("run-{side}.sh"));
    let conf = work_dir.join(format!("tmux-{side}.conf"));
    let mut script_text = String::from("#!/bin/sh\nexec");
    for token in std::iter::once(program.to_string_lossy().into_owned()).chain(argv.iter().cloned())
    {
        script_text.push(' ');
        script_text.push_str(&shell_word(&token));
    }
    script_text.push('\n');
    std::fs::write(&script, script_text)?;
    make_executable(&script)?;
    std::fs::write(
        &conf,
        format!(
            "set -g default-terminal \"{term}\"\n\
             set -g status off\n\
             set -g mouse off\n\
             set -g escape-time 0\n\
             set -g focus-events on\n\
             set -g allow-passthrough on\n\
             set -g remain-on-exit off\n\
             set -g destroy-unattached on\n\
             set -g exit-empty on\n",
            term = profile.term
        ),
    )?;
    let tmux_argv = vec![
        "-S".to_owned(),
        socket.display().to_string(),
        "-f".to_owned(),
        conf.display().to_string(),
        "new-session".to_owned(),
        "-x".to_owned(),
        cols.to_string(),
        "-y".to_owned(),
        rows.to_string(),
        "-s".to_owned(),
        "tui".to_owned(),
        script.display().to_string(),
    ];
    Ok((tmux_argv, socket))
}

/// Quote one word for `sh -c`-free exec lines (safe charset passthru,
/// single-quote otherwise).
fn shell_word(token: &str) -> String {
    if !token.is_empty()
        && token.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '/' | '.' | '-' | '_' | ':' | '=' | '@' | '%' | '+')
        })
    {
        return token.to_owned();
    }
    format!("'{}'", token.replace('\'', "'\\''"))
}

fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions)
}

/// Kill a tmux server started by [`wrap_argv_for_tmux`] (the pane's
/// processes live in the SERVER's session, not the client's process
/// group — killing the client alone would orphan them).
pub fn kill_tmux_server(socket: &Path) {
    let _ = std::process::Command::new("tmux")
        .arg("-S")
        .arg(socket)
        .arg("kill-server")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

// ---------------------------------------------------------------------------
// Protocol signature (the negotiation evidence channel)
// ---------------------------------------------------------------------------

/// Classify the PROTOCOL sequences in a raw child→terminal stream into
/// a `name=count` multiset: private modes set/reset, kitty keyboard
/// push/pop/query/set, cursor shape, clipboard OSC 52, OSC 11 query,
/// notifications, DA1/DA2/XTVERSION/XTGETTCAP queries, DECRQM, window
/// title, and the SGR color encoding split (truecolor vs 256-index —
/// the downgrade matrix evidence). Text and ordinary styling are NOT
/// captured — only protocol-class sequences.
#[must_use]
pub fn protocol_signature(raw: &[u8]) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] != 0x1b {
            index += 1;
            continue;
        }
        let Some((&next, rest)) = raw[index + 1..].split_first() else { break };
        match next {
            b'[' => {
                let Some((params, intermediates, final_byte, consumed)) = parse_csi(rest) else {
                    index += 2;
                    continue;
                };
                let params = String::from_utf8_lossy(&params).into_owned();
                classify_csi(&params, &intermediates, final_byte, &mut counts);
                index += 2 + consumed;
            }
            b']' => {
                let Some((osc, consumed)) = parse_string_terminated(rest, true) else {
                    index += 2;
                    continue;
                };
                classify_osc(&osc, &mut counts);
                index += 2 + consumed;
            }
            b'P' => {
                let Some((dcs, consumed)) = parse_string_terminated(rest, false) else {
                    index += 2;
                    continue;
                };
                if dcs.starts_with(b"+q") {
                    *counts.entry("query:xtgettcap".to_owned()).or_default() += 1;
                }
                index += 2 + consumed;
            }
            _ => {
                index += 2;
            }
        }
    }
    let mut signature: Vec<String> =
        counts.into_iter().map(|(name, count)| format!("{name}={count}")).collect();
    signature.sort();
    signature
}

/// Diff two signatures → (ref-side-only, our-side-only) human lines
/// `name: ref=N our=M`.
#[must_use]
pub fn signature_diff(reference: &[String], our: &[String]) -> (Vec<String>, Vec<String>) {
    let parse = |entries: &[String]| -> BTreeMap<String, usize> {
        entries
            .iter()
            .filter_map(|entry| {
                let (name, count) = entry.split_once('=')?;
                Some((name.to_owned(), count.parse::<usize>().ok()?))
            })
            .collect()
    };
    let reference = parse(reference);
    let our = parse(our);
    let mut ref_only = Vec::new();
    let mut our_only = Vec::new();
    for (name, count) in &reference {
        let other = our.get(name).copied().unwrap_or(0);
        if other != *count {
            ref_only.push(format!("{name}: ref={count} our={other}"));
        }
    }
    for (name, count) in &our {
        if !reference.contains_key(name) {
            our_only.push(format!("{name}: our={count} ref=0"));
        }
    }
    (ref_only, our_only)
}

/// Parse one CSI sequence body (after `ESC [`): params, intermediates,
/// final byte, bytes consumed.
fn parse_csi(bytes: &[u8]) -> Option<(Vec<u8>, Vec<u8>, u8, usize)> {
    let mut params = Vec::new();
    let mut intermediates = Vec::new();
    let mut position = 0;
    let mut in_params = true;
    while position < bytes.len() {
        let byte = bytes[position];
        if (0x30..=0x3f).contains(&byte) {
            if !in_params {
                return None;
            }
            params.push(byte);
        } else if (0x20..=0x2f).contains(&byte) {
            in_params = false;
            intermediates.push(byte);
        } else if (0x40..=0x7e).contains(&byte) {
            return Some((params, intermediates, byte, position + 1));
        } else {
            return None;
        }
        position += 1;
    }
    None
}

/// Parse an OSC (BEL or ST terminated; `allow_bel`) or DCS/PM/APC
/// (ST terminated) string body — bytes after the introducer.
fn parse_string_terminated(bytes: &[u8], allow_bel: bool) -> Option<(Vec<u8>, usize)> {
    let mut position = 0;
    while position < bytes.len() {
        if allow_bel && bytes[position] == 0x07 {
            return Some((bytes[..position].to_vec(), position + 1));
        }
        if bytes[position] == 0x1b {
            if position + 1 < bytes.len() && bytes[position + 1] == b'\\' {
                return Some((bytes[..position].to_vec(), position + 2));
            }
            return None;
        }
        position += 1;
    }
    None
}

fn classify_csi(
    params: &str,
    intermediates: &[u8],
    final_byte: u8,
    counts: &mut BTreeMap<String, usize>,
) {
    let bump = |name: String, counts: &mut BTreeMap<String, usize>| {
        *counts.entry(name).or_default() += 1;
    };
    // Kitty keyboard protocol.
    if final_byte == b'u' {
        if let Some(flags) = params.strip_prefix('>') {
            bump(format!("kitty-push(>{flags}u)"), counts);
        } else if let Some(count) = params.strip_prefix('<') {
            bump(format!("kitty-pop(<{count}u)"), counts);
        } else if params == "?" {
            bump("kitty-query(?u)".to_owned(), counts);
        } else if let Some(rest) = params.strip_prefix('=') {
            bump(format!("kitty-set(={rest}u)"), counts);
        }
        return;
    }
    // Cursor style: `CSI Ps SP q`.
    if final_byte == b'q' && intermediates == [0x20] {
        bump(format!("cursor-shape({params} SP q)"), counts);
        return;
    }
    // XTVERSION query: `CSI > q`.
    if final_byte == b'q' && params == ">" {
        bump("query:xtversion".to_owned(), counts);
        return;
    }
    // DECRQM: `CSI ? mode $ p`.
    if final_byte == b'p' && intermediates == b"$" {
        bump(format!("query:decrqm({params})"), counts);
        return;
    }
    // Private modes: `CSI ? Pm h/l`.
    if (final_byte == b'h' || final_byte == b'l') && params.starts_with('?') {
        bump(format!("mode:{}{}", params, final_byte as char), counts);
        return;
    }
    // Device attributes.
    if final_byte == b'c' && (params.is_empty() || params == "0") {
        bump("query:da1".to_owned(), counts);
        return;
    }
    if final_byte == b'c' && params.starts_with('>') {
        bump("query:da2".to_owned(), counts);
        return;
    }
    // SGR color encoding split (the downgrade matrix evidence).
    if final_byte == b'm' {
        if params.contains("38;2;") || params.contains("48;2;") {
            bump("sgr-truecolor".to_owned(), counts);
        }
        if params.contains("38;5;") || params.contains("48;5;") {
            bump("sgr-256idx".to_owned(), counts);
        }
    }
}

fn classify_osc(osc: &[u8], counts: &mut BTreeMap<String, usize>) {
    let text = String::from_utf8_lossy(osc).into_owned();
    let bump = |name: &str, counts: &mut BTreeMap<String, usize>| {
        *counts.entry(name.to_owned()).or_default() += 1;
    };
    let number = text.split(';').next().unwrap_or_default();
    match number {
        "52" => {
            if text.ends_with('?') {
                bump("query:osc52", counts);
            } else {
                bump("osc52-clipboard-write", counts);
            }
        }
        "11" if text.ends_with('?') => bump("query:osc11", counts),
        "10" if text.ends_with('?') => bump("query:osc10-fg", counts),
        "4" if text.ends_with('?') => bump("query:osc4-palette", counts),
        "99" => {
            if text.contains("p=?") {
                bump("query:osc99-notify-probe", counts);
            } else {
                bump("osc99-notify", counts);
            }
        }
        "1337" => bump("probe:osc1337-iterm2-caps", counts),
        "66" => bump("osc66-opentui", counts),
        "12" => bump("osc12-cursor-color", counts),
        "0" | "2" => bump("osc-title", counts),
        "9" | "777" => bump("osc-notify", counts),
        "8" => bump("osc-hyperlink", counts),
        "133" => bump("osc-shell-integration", counts),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responder_stops_cleanly() {
        let profile = find_term_profile("xterm256").expect("profile");
        let mut responder = PtyResponder::attach(
            profile,
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(std::fs::File::create("/dev/null").expect("devnull"))),
        );
        responder.stop();
    }

    #[test]
    fn signature_finds_modes_and_sgr_split() {
        let raw = b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[38;2;1;2;3m\x1b[48;5;17m\x1b[>1u\x1b[<u\x1b[?u\x1b]52;c;aaa\x1b\\\x1b[2 q\x1b[?25l";
        let signature = protocol_signature(raw);
        assert!(signature.iter().any(|e| e.starts_with("mode:?1049h")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("sgr-truecolor=")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("sgr-256idx=")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("kitty-push(>1u)")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("kitty-pop(<u)")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("kitty-query")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("osc52-clipboard-write")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("cursor-shape(2 SP q)")), "{signature:?}");
        assert!(signature.iter().any(|e| e.starts_with("mode:?25l")), "{signature:?}");
    }

    #[test]
    fn signature_diff_reports_both_sides() {
        let reference = vec!["mode:?1006h=4".to_owned(), "sgr-256idx=10".to_owned()];
        let our = vec![
            "mode:?1006h=4".to_owned(),
            "sgr-256idx=10".to_owned(),
            "kitty-push(>1u)=0".to_owned(),
        ];
        let (ref_only, our_only) = signature_diff(&reference, &our);
        assert!(our_only.iter().any(|entry| entry.contains("kitty-push")));
        assert_eq!(ref_only, Vec::<String>::new());
    }

    #[test]
    fn query_rules_cover_both_osc11_terminators() {
        let profile = find_term_profile("xterm256").expect("profile");
        let rules = query_rules(profile);
        assert!(rules.iter().any(|(needle, _)| needle == b"\x1b]11;?\x07"));
        assert!(rules.iter().any(|(needle, _)| needle == b"\x1b]11;?\x1b\\"));
        // kitty NOT advertised under xterm256: no CSI ? u rule.
        assert!(!rules.iter().any(|(needle, _)| needle == b"\x1b[?u"));
    }

    #[test]
    fn count_occurrences_is_non_overlapping() {
        assert_eq!(count_occurrences(b"ababab", b"abab"), 1);
        assert_eq!(count_occurrences(b"aaaa", b"a"), 4);
        assert_eq!(count_occurrences(b"abc", b"abcd"), 0);
    }
}
