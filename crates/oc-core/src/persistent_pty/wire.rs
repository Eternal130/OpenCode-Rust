//! The `oc-pty` daemon wire format — framing, envelopes, responses,
//! checkpoints and runtime-record helpers shared by the daemon binary
//! (D1) and the in-server client transport (D2).
//!
//! Port of `daemon.ts` (`/opt/opencode-ref/packages/core/src/persistent-pty/daemon.ts`):
//!
//! - **Framing** (`daemon.ts:659-682`): `u32 BE length` + payload,
//!   `MaxFrameBytes = 8 MiB`; oversized/truncated frames are protocol
//!   errors.
//! - **Envelope** (`daemon.ts:455`, `:485`, `:563-575`): every request
//!   payload is JSON `{"token": "…", "request": {…}}` — the token rides
//!   every frame (R7).
//! - **Responses** (`daemon.ts:42-100`): `WireResponse` tagged by
//!   `type`; field names are v2's snake_case wire names.
//! - **Binary output frames** (`daemon.ts:589-597`):
//!   `[0x00][start u64BE][end u64BE][data…]`, ≥ 17 bytes.
//! - **Checkpoint v1** (design §7, R6): `OCPTYCHK` magic + version +
//!   flags + size/cursor/generation + scrollback + CRC32.
//! - **Runtime records** (design §4.4/§5.3): `service.boot` generation
//!   sidecar (`{boot_id, start_time}`) and the stale-registration
//!   liveness predicate (connect-probe is the caller's job; the
//!   `/proc` generation guard lives here).
//!
//! Replay transport (design §3.2): `attached` carries the replay inline
//! (`replay_base64`) iff the replay is ≤ [`INLINE_REPLAY_LIMIT`] raw
//! bytes; above that `replay_chunked: true` + `replay_length` with no
//! inline field, the bytes arrive as ordinary binary output frames and
//! a `replay_complete{end_offset}` JSON frame ends the transfer.

use std::path::Path;

use super::{HandoffState, Info, Registration, PTY_PROTOCOL_VERSION};
use crate::pty::PtyId;

/// `MaxFrameBytes` (`daemon.ts:9`).
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Replay rides inline in `attached` iff the raw replay is at most this
/// many bytes (design §3.2 — pinned at 1 MiB so both arms are testable).
pub const INLINE_REPLAY_LIMIT: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Framing (`daemon.ts:659-682`)
// ---------------------------------------------------------------------------

/// A framing/protocol error (mirrors the ref's thrown messages).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// `daemon.ts:661`, `:675` — frame exceeds [`MAX_FRAME_BYTES`].
    #[error("opencode-pty frame too large")]
    TooLarge,
    /// `daemon.ts:681` — the stream ended mid-frame.
    #[error("opencode-pty truncated frame")]
    Truncated,
    /// `daemon.ts:590` — a binary output frame shorter than 17 bytes.
    #[error("invalid opencode-pty output frame")]
    InvalidOutputFrame,
}

/// Encodes one frame: `u32 BE length` + payload (`daemon.ts:659-666`).
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge);
    }
    let length = u32::try_from(payload.len()).map_err(|_| FrameError::TooLarge)?;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Incremental frame decoder (`daemon.ts:668-682`): feed socket chunks,
/// collect complete payloads; [`FrameDecoder::finish`] rejects a stream
/// that ended mid-frame.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    pending: Vec<u8>,
}

impl FrameDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one chunk, returning every complete frame in order.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        self.pending.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while self.pending.len() >= 4 {
            let length = u32::from_be_bytes([
                self.pending[0],
                self.pending[1],
                self.pending[2],
                self.pending[3],
            ]) as usize;
            if length > MAX_FRAME_BYTES {
                return Err(FrameError::TooLarge);
            }
            if self.pending.len() < length + 4 {
                break;
            }
            let frame = self.pending[4..4 + length].to_vec();
            self.pending.drain(..4 + length);
            frames.push(frame);
        }
        Ok(frames)
    }

    /// End of stream: pending bytes mean a truncated frame.
    pub fn finish(&self) -> Result<(), FrameError> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(FrameError::Truncated)
        }
    }
}

// ---------------------------------------------------------------------------
// Binary output frames (`daemon.ts:589-597`)
// ---------------------------------------------------------------------------

/// A parsed binary output frame `[0x00][start u64BE][end u64BE][data]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFrame {
    pub start: u64,
    pub end: u64,
    pub data: Vec<u8>,
}

/// Encodes a binary output frame payload (first byte `0x00`). The
/// result is a frame **payload**: wrap it with [`encode_frame`] before
/// writing to the socket — binary frames ride the same `u32 BE length`
/// framing as JSON frames (`daemon.ts:589-599`).
pub fn encode_output_frame(start: u64, end: u64, data: &[u8]) -> Result<Vec<u8>, FrameError> {
    let mut out = Vec::with_capacity(17 + data.len());
    out.push(0x00);
    out.extend_from_slice(&start.to_be_bytes());
    out.extend_from_slice(&end.to_be_bytes());
    out.extend_from_slice(data);
    if out.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge);
    }
    Ok(out)
}

/// Parses a binary output frame; the caller routes on `frame[0] == 0`
/// first (`daemon.ts:589`), so this rejects only shape errors.
pub fn parse_output_frame(frame: &[u8]) -> Result<OutputFrame, FrameError> {
    if frame.len() < 17 || frame[0] != 0x00 {
        return Err(FrameError::InvalidOutputFrame);
    }
    let start = u64::from_be_bytes(frame[1..9].try_into().expect("8 bytes"));
    let end = u64::from_be_bytes(frame[9..17].try_into().expect("8 bytes"));
    Ok(OutputFrame { start, end, data: frame[17..].to_vec() })
}

// ---------------------------------------------------------------------------
// Wire terminal (`daemon.ts:17-30`; mapping via `index.ts:390-418`)
// ---------------------------------------------------------------------------

/// `daemon.ts:11-15` — `failed` is reserved (spawn failures return
/// `error` responses instead).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WireLifecycle {
    Running,
    Exited {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
}

/// `WireTerminal` (`daemon.ts:17-30`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WireTerminal {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground_process: Option<String>,
    pub group_id: String,
    pub command: Vec<String>,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub lifecycle: WireLifecycle,
    pub output_head: u64,
    pub output_tail: u64,
}

/// `pty_persistent_<n>` → `n` (`index.ts:410-418`); `None` on any other
/// shape.
#[must_use]
pub fn parse_terminal_number(id: &PtyId) -> Option<u64> {
    let suffix = id.0.strip_prefix("pty_persistent_")?;
    let number: u64 = suffix.parse().ok()?;
    (number >= 1).then_some(number)
}

/// `n` → `pty_persistent_<n>` (the inverse of [`parse_terminal_number`]).
#[must_use]
pub fn terminal_id(number: u64) -> PtyId {
    PtyId(format!("pty_persistent_{number}"))
}

/// Maps the machine's [`Info`] onto the wire shape (the inverse of the
/// ref's `toInfo`, `index.ts:390-408`): `group_id` = `session_id`,
/// `command` = `[command, args...]`, `output_head/output_tail` =
/// `output.0/1`. Errors when the id is not a `pty_persistent_<n>`.
pub fn wire_terminal(info: &Info) -> Result<WireTerminal, String> {
    let number = parse_terminal_number(&info.id)
        .ok_or_else(|| format!("invalid persistent PTY ID: {}", info.id.0))?;
    let lifecycle = match info.status {
        crate::pty::Status::Running => WireLifecycle::Running,
        crate::pty::Status::Exited => WireLifecycle::Exited { exit_code: info.exit_code },
    };
    let mut command = Vec::with_capacity(1 + info.args.len());
    command.push(info.command.clone());
    command.extend(info.args.iter().cloned());
    Ok(WireTerminal {
        id: number,
        pid: (info.pid != 0).then_some(info.pid),
        title: info.title.clone(),
        foreground_process: info.foreground_process.clone(),
        group_id: info.session_id.as_str().to_owned(),
        command,
        cwd: info.cwd.clone(),
        cols: info.size.0,
        rows: info.size.1,
        lifecycle,
        output_head: u64::try_from(info.output.0).unwrap_or(u64::MAX),
        output_tail: u64::try_from(info.output.1).unwrap_or(u64::MAX),
    })
}

// ---------------------------------------------------------------------------
// Requests (envelope `daemon.ts:455/485/563-575`; ops per design §3)
// ---------------------------------------------------------------------------

/// `controller` | `observer` (`daemon.ts:103`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireRole {
    Controller,
    Observer,
}

impl From<WireRole> for super::Role {
    fn from(role: WireRole) -> Self {
        match role {
            WireRole::Controller => super::Role::Controller,
            WireRole::Observer => super::Role::Observer,
        }
    }
}

/// Every daemon op (design §3 table). `Unknown` keeps mixed-version
/// degradation on the wire `error` shape (design §11.4) instead of a
/// deserialize failure.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WireRequest {
    Ping,
    /// Ownership claim (§6 arm A).
    Own {
        instance_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ticket: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directory: Option<String>,
    },
    PrepareHandoff,
    /// TOKEN-level by design (§4.3): the discard path calls it from a
    /// non-owner one-shot connection.
    Shutdown,
    List,
    Create {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        program: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        group_id: String,
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cols: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<u16>,
    },
    Write {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachment_id: Option<String>,
        data_base64: String,
    },
    Resize {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachment_id: Option<String>,
        cols: u16,
        rows: u16,
    },
    Control {
        id: u64,
        attachment_id: String,
        cols: u16,
        rows: u16,
    },
    Input {
        id: u64,
        attachment_id: String,
        cols: u16,
        rows: u16,
        data_base64: String,
    },
    Snapshot {
        id: u64,
    },
    ReadRows {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<u32>,
    },
    /// Extension (§3): session-keyed read against the daemon-side
    /// `current[]` map.
    Read {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rows: Option<u32>,
    },
    Terminate {
        id: u64,
    },
    Subscribe {
        id: u64,
        #[serde(default)]
        offset: u64,
        attachment_id: String,
        role: WireRole,
        #[serde(default)]
        takeover: bool,
    },
    /// Extension (§6 arm B): snapshot restore — owner-only (§4.3).
    Restore {
        state: HandoffState,
    },
    /// Any op this daemon does not know (§11.4 compat guard).
    #[serde(other)]
    Unknown,
}

/// The per-frame authenticated envelope (`daemon.ts:455`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Envelope {
    pub token: String,
    pub request: WireRequest,
}

// ---------------------------------------------------------------------------
// Responses (`daemon.ts:42-100`) + stream/owner frames
// ---------------------------------------------------------------------------

/// `WireResponse` (`daemon.ts:42-100`) plus the stream frames that ride
/// subscribe connections (`resized`/`exited`/`controller_changed`,
/// §3 stream table) and the §3.2 `replay_complete` marker.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireResponse {
    /// `daemon.ts:44-48` — the §4.2 ping guard echo.
    Pong {
        instance_id: String,
        pid: u32,
        protocol: u64,
    },
    Created {
        terminal: WireTerminal,
    },
    Terminals {
        terminals: Vec<WireTerminal>,
    },
    Ok,
    Owned,
    Handoff {
        ticket: String,
        expires_at: u64,
    },
    Snapshot {
        terminal: WireTerminal,
        text: String,
        checkpoint_base64: String,
        cursor_x: u64,
        cursor_y: u64,
    },
    /// `terminal` is `None` when nothing is selected for the session
    /// (our daemon owns `current[]`, §3) — v2 never hits this because
    /// it keeps the map client-side.
    Rows {
        terminal: Option<WireTerminal>,
        lines: Vec<String>,
        cursor_x: u64,
        cursor_y: u64,
    },
    Attached {
        terminal: WireTerminal,
        role: WireRole,
        generation: u64,
        requested_offset: u64,
        available_offset: u64,
        end_offset: u64,
        truncated: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_base64: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_chunked: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replay_length: Option<u64>,
    },
    Resized {
        cols: u16,
        rows: u16,
        generation: u64,
        checkpoint_base64: String,
    },
    Exited {
        exit_code: Option<i32>,
        final_offset: u64,
    },
    ControllerChanged {
        attachment_id: Option<String>,
        generation: u64,
    },
    /// §3.2: end of a chunked replay transfer.
    ReplayComplete {
        end_offset: u64,
    },
    /// `daemon.ts:99`. `authentication failed` is the only message with
    /// transport semantics (§9).
    Error {
        message: String,
    },
}

impl WireResponse {
    /// Serializes + frames the response for the socket.
    pub fn to_frame(&self) -> Result<Vec<u8>, FrameError> {
        let payload = serde_json::to_vec(self).map_err(|_| FrameError::TooLarge)?;
        encode_frame(&payload)
    }
}

/// Daemon→owner asynchronous frames (§3.1, §6) — interleaved with
/// exchange responses on the owner connection, self-describing by
/// `type`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OwnerFrame {
    /// `{"type":"bus_event","event":…,"data":…}` — the machine's bus
    /// publications, serialized verbatim for the client layer to
    /// republish onto the server bus (§3.1).
    BusEvent { event: String, data: serde_json::Value },
    /// §6 supersession: a valid-ticket `own` demoted this connection;
    /// it is closed right after the frame flushes.
    OwnerSuperseded,
}

// ---------------------------------------------------------------------------
// Errors + versioning (§4.2, §9)
// ---------------------------------------------------------------------------

/// `daemon.ts:209-216` — the exact mismatch message.
#[must_use]
pub fn protocol_mismatch_message(daemon_protocol: u64) -> String {
    format!(
        "opencode-pty protocol mismatch: daemon={daemon_protocol}, client={PTY_PROTOCOL_VERSION}"
    )
}

// ---------------------------------------------------------------------------
// Checkpoint v1 (design §7, R6)
// ---------------------------------------------------------------------------

/// `b"OCPTYCHK"` (§7).
pub const CHECKPOINT_MAGIC: [u8; 8] = *b"OCPTYCHK";
/// Checkpoint format version 1 (§7).
pub const CHECKPOINT_VERSION: u16 = 1;
/// Header size: magic(8) + version(2) + flags(2) + cols(2) + rows(2) +
/// `cursor_x`(4) + `cursor_y`(4) + generation(8) + `scroll_len`(8).
const CHECKPOINT_HEADER: usize = 40;

/// Checkpoint decode failures (§7 rules) — the caller maps these onto
/// the `protocol` error kind (§9).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckpointError {
    #[error("opencode-pty checkpoint magic mismatch")]
    Magic,
    #[error("opencode-pty checkpoint version unsupported")]
    Version,
    #[error("opencode-pty checkpoint crc mismatch")]
    Crc,
    #[error("opencode-pty checkpoint shape invalid")]
    Shape,
    #[error("opencode-pty checkpoint scrollback row count mismatch")]
    Rows,
}

/// What the machine tracks at capture time (§7 version 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointV1 {
    pub cols: u16,
    pub rows: u16,
    pub cursor_x: u32,
    pub cursor_y: u32,
    pub generation: u64,
    /// Screen rows joined by `\n`, top→bottom, exactly `rows` rows.
    pub scrollback: String,
}

/// Serializes a v1 checkpoint (all integers big-endian, CRC32 IEEE
/// trailer over bytes `[0, 40+len)`).
#[must_use]
pub fn encode_checkpoint(checkpoint: &CheckpointV1) -> Vec<u8> {
    let scroll = checkpoint.scrollback.as_bytes();
    let mut out = Vec::with_capacity(CHECKPOINT_HEADER + scroll.len() + 4);
    out.extend_from_slice(&CHECKPOINT_MAGIC);
    out.extend_from_slice(&CHECKPOINT_VERSION.to_be_bytes());
    let flags: u16 = u16::from(!scroll.is_empty());
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&checkpoint.cols.to_be_bytes());
    out.extend_from_slice(&checkpoint.rows.to_be_bytes());
    out.extend_from_slice(&checkpoint.cursor_x.to_be_bytes());
    out.extend_from_slice(&checkpoint.cursor_y.to_be_bytes());
    out.extend_from_slice(&checkpoint.generation.to_be_bytes());
    out.extend_from_slice(&u64::try_from(scroll.len()).unwrap_or(u64::MAX).to_be_bytes());
    out.extend_from_slice(scroll);
    let crc = crc32_ieee(&out);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

/// Decodes + validates a v1 checkpoint (§7 rejection rules: wrong
/// magic, unknown version, CRC mismatch, zero rows/cols, row-count
/// mismatch).
pub fn decode_checkpoint(bytes: &[u8]) -> Result<CheckpointV1, CheckpointError> {
    if bytes.len() < CHECKPOINT_HEADER + 4 {
        return Err(CheckpointError::Shape);
    }
    if bytes[0..8] != CHECKPOINT_MAGIC {
        return Err(CheckpointError::Magic);
    }
    let version = u16::from_be_bytes(bytes[8..10].try_into().expect("2 bytes"));
    if version != CHECKPOINT_VERSION {
        return Err(CheckpointError::Version);
    }
    let scroll_len =
        usize::try_from(u64::from_be_bytes(bytes[32..40].try_into().expect("8 bytes")))
            .map_err(|_| CheckpointError::Shape)?;
    let body_end = CHECKPOINT_HEADER + scroll_len;
    if bytes.len() != body_end + 4 {
        return Err(CheckpointError::Shape);
    }
    let crc = u32::from_be_bytes(bytes[body_end..body_end + 4].try_into().expect("4 bytes"));
    if crc32_ieee(&bytes[..body_end]) != crc {
        return Err(CheckpointError::Crc);
    }
    let cols = u16::from_be_bytes(bytes[12..14].try_into().expect("2 bytes"));
    let rows = u16::from_be_bytes(bytes[14..16].try_into().expect("2 bytes"));
    if cols == 0 || rows == 0 {
        return Err(CheckpointError::Shape);
    }
    let scrollback = std::str::from_utf8(&bytes[CHECKPOINT_HEADER..body_end])
        .map_err(|_| CheckpointError::Shape)?
        .to_owned();
    if scrollback.split('\n').count() != usize::from(rows) {
        return Err(CheckpointError::Rows);
    }
    Ok(CheckpointV1 {
        cols,
        rows,
        cursor_x: u32::from_be_bytes(bytes[16..20].try_into().expect("4 bytes")),
        cursor_y: u32::from_be_bytes(bytes[20..24].try_into().expect("4 bytes")),
        generation: u64::from_be_bytes(bytes[24..32].try_into().expect("8 bytes")),
        scrollback,
    })
}

/// CRC32 (IEEE 802.3, reflected, init/xorout `0xFFFF_FFFF`) — no new
/// dependency; pinned by the standard test vector below.
#[must_use]
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// Runtime records: `service.boot` + stale detection (§4.4, §5.3)
// ---------------------------------------------------------------------------

/// The `service.boot` sidecar (§5.3) — the safekill generation record
/// shape (`{boot_id, start_time}`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BootRecord {
    pub boot_id: String,
    pub start_time: u64,
}

/// The machine's current boot id (`/proc/sys/kernel/random/boot_id`).
#[must_use]
pub fn boot_id() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
}

/// Process start time (field 22 of `/proc/<pid>/stat`, clock ticks).
#[must_use]
pub fn proc_start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit(')').next()?;
    let mut fields = rest.split_whitespace();
    for _ in 3..=21 {
        fields.next()?;
    }
    fields.next()?.parse().ok()
}

/// Writes `<directory>/service.boot` (atomic, 0600) for the daemon
/// process `pid` (§4.4 — the record a stale-socket probe checks).
pub fn write_boot_record(directory: &Path, pid: u32) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let (Some(boot), Some(start_time)) = (boot_id(), proc_start_time(pid)) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "boot identity unavailable on this platform",
        ));
    };
    let record = BootRecord { boot_id: boot, start_time };
    let payload = serde_json::to_string(&record)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let target = directory.join("service.boot");
    let temp = directory.join(format!("service.boot.{}.tmp", super::new_random_token()));
    let mut file =
        std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&temp)?;
    file.write_all(payload.as_bytes())?;
    drop(file);
    match std::fs::rename(&temp, &target) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Reads `<directory>/service.boot`, if present.
#[must_use]
pub fn read_boot_record(directory: &Path) -> Option<BootRecord> {
    let text = std::fs::read_to_string(directory.join("service.boot")).ok()?;
    serde_json::from_str(&text).ok()
}

/// The §4.4 stale predicate for a runtime directory's registration:
/// `Some(true)` only when `service.json` + `service.boot` are readable
/// AND the current `boot_id` matches AND `/proc/<pid>/stat` still
/// reports the recorded `start_time` (a recycled pid reads as dead).
/// `None` when either record is unreadable — the caller falls back to
/// its connect-probe; either way nothing is ever signalled.
#[must_use]
pub fn registration_alive(directory: &Path) -> Option<bool> {
    let registration_text = std::fs::read_to_string(directory.join("service.json")).ok()?;
    let registration: Registration = serde_json::from_str(&registration_text).ok()?;
    let record = read_boot_record(directory)?;
    if registration.pid == 0 {
        return Some(false);
    }
    let live = boot_id().is_some_and(|current| current == record.boot_id)
        && proc_start_time(registration.pid).is_some_and(|start| start == record.start_time);
    Some(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_and_split_chunks() {
        let payload = b"{\"op\":\"ping\"}".to_vec();
        let bytes = encode_frame(&payload).expect("encode");
        assert_eq!(&bytes[..4], &u32::try_from(payload.len()).expect("length").to_be_bytes());

        let mut decoder = FrameDecoder::new();
        // Split the encoded frame across two chunks mid-header/mid-body.
        assert_eq!(decoder.feed(&bytes[..2]).expect("feed"), Vec::<Vec<u8>>::new());
        assert_eq!(decoder.feed(&bytes[2..6]).expect("feed"), Vec::<Vec<u8>>::new());
        let frames = decoder.feed(&bytes[6..]).expect("feed");
        assert_eq!(frames, vec![payload]);
        assert_eq!(decoder.finish(), Ok(()));
    }

    #[test]
    fn frame_multiple_in_one_chunk() {
        let first = encode_frame(b"one").expect("encode");
        let second = encode_frame(b"two").expect("encode");
        let mut combined = first.clone();
        combined.extend_from_slice(&second);
        let mut decoder = FrameDecoder::new();
        let frames = decoder.feed(&combined).expect("feed");
        assert_eq!(frames, vec![b"one".to_vec(), b"two".to_vec()]);
    }

    #[test]
    fn frame_oversize_rejected() {
        let huge = vec![0u8; MAX_FRAME_BYTES + 1];
        assert_eq!(encode_frame(&huge), Err(FrameError::TooLarge));

        // Decoder sees a length header beyond the cap.
        let mut decoder = FrameDecoder::new();
        let mut bytes =
            u32::try_from(MAX_FRAME_BYTES + 1).expect("cap fits u32").to_be_bytes().to_vec();
        bytes.extend_from_slice(&[0u8; 8]);
        assert_eq!(decoder.feed(&bytes), Err(FrameError::TooLarge));
    }

    #[test]
    fn frame_at_cap_accepted() {
        let payload = vec![7u8; MAX_FRAME_BYTES];
        let bytes = encode_frame(&payload).expect("8 MiB frame is in-cap");
        let mut decoder = FrameDecoder::new();
        let frames = decoder.feed(&bytes).expect("feed");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), MAX_FRAME_BYTES);
    }

    #[test]
    fn frame_truncated_at_eof_rejected() {
        let mut decoder = FrameDecoder::new();
        let bytes = encode_frame(b"payload").expect("encode");
        decoder.feed(&bytes[..bytes.len() - 1]).expect("feed");
        assert_eq!(decoder.finish(), Err(FrameError::Truncated));
    }

    #[test]
    fn output_frame_round_trip_and_rejections() {
        let frame = encode_output_frame(17, 42, b"data").expect("encode");
        assert_eq!(frame[0], 0x00);
        assert_eq!(frame.len(), 17 + 4);
        let parsed = parse_output_frame(&frame).expect("parse");
        assert_eq!(parsed, OutputFrame { start: 17, end: 42, data: b"data".to_vec() });
        // <17 bytes and non-zero first byte are rejected.
        assert_eq!(parse_output_frame(&frame[..16]), Err(FrameError::InvalidOutputFrame));
        let mut bad = frame.clone();
        bad[0] = 0x01;
        assert_eq!(parse_output_frame(&bad), Err(FrameError::InvalidOutputFrame));
    }

    fn sample_info() -> Info {
        Info {
            id: PtyId("pty_persistent_3".to_owned()),
            title: "workbench".to_owned(),
            command: "/bin/sh".to_owned(),
            args: vec!["-c".to_owned(), "ls".to_owned()],
            cwd: "/tmp".to_owned(),
            status: crate::pty::Status::Running,
            pid: 4242,
            exit_code: None,
            session_id: oc_schema::id::SessionId::parse("ses_wire").expect("session"),
            foreground_process: None,
            size: (80, 24),
            output: (10, 20),
        }
    }

    #[test]
    fn wire_terminal_mapping_round_trip() {
        let info = sample_info();
        let wire = wire_terminal(&info).expect("map");
        assert_eq!(wire.id, 3);
        assert_eq!(wire.group_id, "ses_wire");
        assert_eq!(wire.command, vec!["/bin/sh".to_owned(), "-c".to_owned(), "ls".to_owned()]);
        assert_eq!(wire.output_head, 10);
        assert_eq!(wire.output_tail, 20);
        assert_eq!(wire.lifecycle, WireLifecycle::Running);
        assert_eq!(parse_terminal_number(&terminal_id(3)), Some(3));
    }

    #[test]
    fn wire_terminal_lifecycle_and_pid_variants() {
        let mut info = sample_info();
        info.status = crate::pty::Status::Exited;
        info.exit_code = Some(1);
        info.pid = 0;
        let wire = wire_terminal(&info).expect("map");
        assert_eq!(wire.lifecycle, WireLifecycle::Exited { exit_code: Some(1) });
        assert_eq!(wire.pid, None);

        // The wire shape is v2's: camel-free snake_case JSON.
        let json = serde_json::to_value(&wire).expect("json");
        assert_eq!(json["lifecycle"]["status"], "exited");
        assert_eq!(json["lifecycle"]["exit_code"], 1);
        assert!(json.get("pid").is_none(), "null pid omitted");
    }

    #[test]
    fn wire_terminal_rejects_foreign_ids() {
        let mut info = sample_info();
        info.id = PtyId("pty_other_1".to_owned());
        assert!(wire_terminal(&info).is_err());
        assert_eq!(parse_terminal_number(&PtyId("pty_persistent_0".to_owned())), None);
        assert_eq!(parse_terminal_number(&PtyId("pty_persistent_x".to_owned())), None);
    }

    #[test]
    fn envelope_round_trips_create_defaults() {
        let json = r#"{"token":"abc","request":{"op":"create","group_id":"ses_1"}}"#;
        let envelope: Envelope = serde_json::from_str(json).expect("parse");
        match &envelope.request {
            WireRequest::Create { program, args, cwd, title, group_id, env, cols, rows } => {
                assert_eq!(*program, None);
                assert_eq!(args.len(), 0, "args default empty");
                assert_eq!(*cwd, None);
                assert_eq!(*title, None);
                assert_eq!(group_id, "ses_1");
                assert_eq!(env.len(), 0, "env defaults empty");
                assert_eq!(*cols, None);
                assert_eq!(*rows, None);
            }
            other => panic!("wrong op: {other:?}"),
        }
    }

    #[test]
    fn envelope_own_optional_fields_absent() {
        let json = r#"{"token":"t","request":{"op":"own","instance_id":"i1"}}"#;
        let envelope: Envelope = serde_json::from_str(json).expect("parse");
        assert_eq!(
            envelope.request,
            WireRequest::Own { instance_id: "i1".to_owned(), ticket: None, directory: None }
        );
        // Serialize does not emit the absent fields.
        let back = serde_json::to_value(&envelope).expect("json");
        let request = back["request"].as_object().expect("object");
        assert!(request.contains_key("op"));
        assert!(!request.contains_key("ticket"));
        assert!(!request.contains_key("directory"));
    }

    #[test]
    fn envelope_unknown_op_degrades_to_unknown() {
        let json = r#"{"token":"t","request":{"op":"future_op","x":1}}"#;
        let envelope: Envelope = serde_json::from_str(json).expect("parse");
        assert_eq!(envelope.request, WireRequest::Unknown);
    }

    #[test]
    fn responses_round_trip_through_json() {
        let response = WireResponse::Pong {
            instance_id: "inst".to_owned(),
            pid: 17,
            protocol: PTY_PROTOCOL_VERSION,
        };
        let json = serde_json::to_string(&response).expect("json");
        assert!(json.contains(r#""type":"pong""#));
        assert!(json.contains(r#""instance_id":"inst""#));
        let back: WireResponse = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, response);
    }

    #[test]
    fn attached_inline_vs_chunked_field_presence() {
        let inline = WireResponse::Attached {
            terminal: wire_terminal(&sample_info()).expect("wire"),
            role: WireRole::Controller,
            generation: 2,
            requested_offset: 0,
            available_offset: 0,
            end_offset: 5,
            truncated: false,
            replay_base64: Some("aGVsbG8=".to_owned()),
            replay_chunked: None,
            replay_length: None,
        };
        let json = serde_json::to_value(&inline).expect("json");
        assert!(json.get("replay_base64").is_some());
        assert!(json.get("replay_chunked").is_none());
        assert!(json.get("replay_length").is_none());

        let chunked = WireResponse::Attached {
            terminal: wire_terminal(&sample_info()).expect("wire"),
            role: WireRole::Observer,
            generation: 0,
            requested_offset: 0,
            available_offset: 0,
            end_offset: 2_000_000,
            truncated: false,
            replay_base64: None,
            replay_chunked: Some(true),
            replay_length: Some(2_000_000),
        };
        let json = serde_json::to_value(&chunked).expect("json");
        assert!(json.get("replay_base64").is_none());
        assert_eq!(json["replay_chunked"], true);
        assert_eq!(json["replay_length"], 2_000_000);
        let back: WireResponse = serde_json::from_value(json).expect("parse");
        assert_eq!(back, chunked);
    }

    #[test]
    fn owner_frames_json_shapes() {
        let bus = OwnerFrame::BusEvent {
            event: "persistent-pty.added".to_owned(),
            data: serde_json::json!({"sessionID": "ses_1"}),
        };
        let json = serde_json::to_value(&bus).expect("json");
        assert_eq!(json["type"], "bus_event");
        assert_eq!(json["event"], "persistent-pty.added");
        assert_eq!(json["data"]["sessionID"], "ses_1");

        let superseded = serde_json::to_value(OwnerFrame::OwnerSuperseded).expect("json");
        assert_eq!(superseded["type"], "owner_superseded");
    }

    #[test]
    fn protocol_mismatch_message_is_exact() {
        assert_eq!(
            protocol_mismatch_message(1),
            format!("opencode-pty protocol mismatch: daemon=1, client={PTY_PROTOCOL_VERSION}")
        );
    }

    fn sample_checkpoint() -> CheckpointV1 {
        CheckpointV1 {
            cols: 80,
            rows: 3,
            cursor_x: 4,
            cursor_y: 2,
            generation: 7,
            scrollback: "alpha\nbeta\ngamma".to_owned(),
        }
    }

    #[test]
    fn checkpoint_round_trip() {
        let bytes = encode_checkpoint(&sample_checkpoint());
        assert_eq!(&bytes[0..8], b"OCPTYCHK");
        assert_eq!(decode_checkpoint(&bytes).expect("decode"), sample_checkpoint());
    }

    #[test]
    fn checkpoint_rejections() {
        let bytes = encode_checkpoint(&sample_checkpoint());
        // Wrong magic.
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert_eq!(decode_checkpoint(&bad), Err(CheckpointError::Magic));
        // Unknown version.
        let mut bad = bytes.clone();
        bad[8..10].copy_from_slice(&2u16.to_be_bytes());
        assert_eq!(decode_checkpoint(&bad), Err(CheckpointError::Version));
        // CRC mismatch (corrupt one byte).
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0xFF;
        assert_eq!(decode_checkpoint(&bad), Err(CheckpointError::Crc));
        // Row-count mismatch: claim 4 rows over 3 lines.
        let mut lying = sample_checkpoint();
        lying.rows = 4;
        assert_eq!(decode_checkpoint(&encode_checkpoint(&lying)), Err(CheckpointError::Rows));
        // Zero cols rejected.
        let mut zeroed = sample_checkpoint();
        zeroed.cols = 0;
        assert_eq!(decode_checkpoint(&encode_checkpoint(&zeroed)), Err(CheckpointError::Shape));
        // Too short.
        assert_eq!(decode_checkpoint(&bytes[..10]), Err(CheckpointError::Shape));
    }

    #[test]
    fn crc32_ieee_known_vector() {
        // Standard CRC-32/ISO-HDLC test vector.
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn boot_record_round_trip_and_liveness() {
        let dir = tempfile::Builder::new()
            .prefix("oc-pty-wire-boot-")
            .tempdir_in("/tmp/opencode")
            .expect("temp dir");
        let directory = dir.path();
        // No records yet.
        assert_eq!(registration_alive(directory), None);

        write_boot_record(directory, std::process::id()).expect("write");
        let record = read_boot_record(directory).expect("read");
        assert_eq!(record.boot_id, boot_id().expect("boot id"));
        assert_eq!(record.start_time, proc_start_time(std::process::id()).expect("start"));

        // A registration for THIS process with a correct sidecar is live.
        std::fs::write(
            directory.join("service.json"),
            serde_json::to_string(&Registration {
                instance_id: "i".to_owned(),
                pid: std::process::id(),
                protocol: PTY_PROTOCOL_VERSION,
                socket: String::new(),
                token: "t".to_owned(),
            })
            .expect("json"),
        )
        .expect("write registration");
        assert_eq!(registration_alive(directory), Some(true));

        // A stale boot id (machine rebooted) is refused: never live.
        let stale = BootRecord {
            boot_id: "00000000-0000-0000-0000-000000000000".to_owned(),
            start_time: record.start_time,
        };
        std::fs::write(
            directory.join("service.boot"),
            serde_json::to_string(&stale).expect("json"),
        )
        .expect("write stale");
        assert_eq!(registration_alive(directory), Some(false));

        // A recycled pid (start_time mismatch) is refused too.
        let recycled = BootRecord { boot_id: record.boot_id, start_time: record.start_time + 1 };
        std::fs::write(
            directory.join("service.boot"),
            serde_json::to_string(&recycled).expect("json"),
        )
        .expect("write recycled");
        assert_eq!(registration_alive(directory), Some(false));
    }
}
