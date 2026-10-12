//! opencode terminal UI (ratatui + taffy): reactive store, keymap,
//! theme wiring, layout layer, SSE data layer, and components — the
//! Phase 7.1 foundation.
//!
//! Module map:
//!
//! * [`reactive`] — signal/memo/effect micrograph (no deps; per-graph
//!   effect slots, never-dropped cross-pump notifications) powering the
//!   store's coalesced redraw scheduling.
//! * [`store`] — tabs (session-tabs-model semantics), route, dialogs,
//!   theme, connection state; single-consumer event ingest.
//! * [`keymap`] — @opentui/keymap semantics + the full v2 default table.
//! * [`layout`] — taffy flex tree with opentui box semantics, plain and
//!   styled-segment text leaves, and the substrate gotchas encoded as
//!   layer defaults.
//! * [`wrap`] — the flattened (cluster, style) text stream: wrapping +
//!   measurement over one break-opportunity seam, cluster-safe hard
//!   breaks, sidecar-pinned display widths.
//! * [`render`] — painted frames (per-run styles from segments) + the
//!   canonical ANSI serialization shared with `tools/parity` goldens.
//! * [`theme`] — `oc-theme` `ResolvedTheme` consumption → style defaults.
//! * [`data`] — SSE connection: reconnect, resync, ordered ingest.
//! * [`components`] — session-tabs (golden-matched), route skeletons,
//!   tool rows, palette skeleton.
//! * [`spike`] — the wave-7.0 renderer-substrate spike, now a thin shim
//!   over the production modules (kept as the absorption proof).

pub mod app;
pub mod attention;
pub mod clipboard;
pub mod components;
pub mod config;
pub mod data;
pub mod diff_viewer_golden_tests;
pub mod editor;
/// R-07: the fake-terminal image display tests (captured-output
/// chain — no PTY needed).
#[cfg(test)]
mod frame_budget_tests;
pub mod golden_tests;
pub mod home_golden_tests;
pub mod image;
pub mod image_blocks;
#[cfg(test)]
mod image_display_tests;
pub mod keymap;
pub mod layout;
pub mod message_golden_tests;
pub mod mini;
pub mod notifications_plugin;
pub mod os_notify;
pub mod plugin_boot;
pub mod plugin_ui;
pub mod reactive;
pub mod render;
pub mod replay;
pub mod screen_golden_tests;
pub mod sound;
pub mod spike;
pub mod stderr_redirect;
pub mod storage;
pub mod store;
pub mod theme;
pub mod wrap;
