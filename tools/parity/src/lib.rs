//! Parity harness comparing the Rust port of opencode v2.0.24 against the
//! reference implementation (`/tmp/opencode/ref`, runnable with bun).
//!
//! Module map:
//!
//! - [`normalize`] — the canonical rewrite algorithm specified by
//!   `NORMALIZATION.md` (repo-relative: `tools/parity/NORMALIZATION.md`).
//!   Every value compared by this crate goes through this one function on
//!   both sides.
//! - [`reference`] — spawns the reference server (`opencode serve`) on a free
//!   port inside a sandboxed HOME, waits for readiness, exposes url/password,
//!   and shuts the whole process group down.
//! - [`httpdiff`] — runs the same request against two base URLs, normalizes
//!   both bodies, and reports the first divergence path.
//! - [`sse`] — subscribes to `GET /api/event` (text/event-stream), parses
//!   `data:` frames and heartbeat comments, collects frames (count-based or
//!   the scenario policy: N frames / quiescence), and compares two frame
//!   sequences per the canonical grouping policy.
//! - [`golden`] — golden-file compare with inline update (`PARITY_UPDATE=1`).
//! - [`scenario`] — scripted SSE scenario runner: boot the reference,
//!   subscribe from t0, drive a scripted API flow, collect to quiescence,
//!   and golden the normalized frame-sequence groups (Phase 5 replays the
//!   same scripts against the port).
//! - [`tui_frame`] — TUI golden-frame primitives (wave 7.0): load the
//!   headless-exported reference frames (`goldens/tui-frames`), parse the
//!   canonical text-grid / ANSI-styled formats, compare frames with
//!   first-divergent-cell classification, and degrade colors through the
//!   TERM matrix (truecolor → 256 → 16).
//!
//! (2026-10-17 paradigm ruling: the side-by-side differential rig —
//! `tui_diff`/`termatrix`, the tui-diff binary, corpus scripts and
//! ref-capture cache — was REMOVED. Golden-frame regression tests stay.)
//!
//! HTTP is performed by a minimal localhost HTTP/1.1 client
//! ([`minih`]): the harness only ever talks to `127.0.0.1`, and keeping it
//! dependency-free keeps the harness buildable offline. The Phase-10 p10
//! battery reuses it directly for raw error-path probing.

pub mod golden;
pub mod httpdiff;
pub mod normalize;
pub mod openapi_diff;
pub mod reference;
pub mod rust_server;
pub mod scenario;
pub mod sse;
pub mod termatrix;
pub mod tui_diff;
pub mod tui_frame;

pub mod minih;
