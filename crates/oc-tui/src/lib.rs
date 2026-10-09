//! opencode terminal UI (ratatui + taffy): reactive store, keymap,
//! theme wiring, layout layer, SSE data layer, and components — the
//! Phase 7.1 foundation.
//!
//! Module map:
//!
//! * [`reactive`] — signal/memo/effect micrograph (no deps) powering the
//!   store's coalesced redraw scheduling.
//! * [`store`] — tabs (session-tabs-model semantics), route, dialogs,
//!   theme, connection state; single-consumer event ingest.
//! * [`keymap`] — @opentui/keymap semantics + the full v2 default table.
//! * [`layout`] — taffy flex tree with opentui box semantics and the
//!   substrate gotchas encoded as layer defaults.
//! * [`wrap`] — text wrapping + measurement sharing one break-opportunity
//!   seam; sidecar-pinned display widths.
//! * [`render`] — painted frames + the canonical ANSI serialization
//!   shared with `tools/parity` goldens.
//! * [`theme`] — `oc-theme` `ResolvedTheme` consumption → style defaults.
//! * [`data`] — SSE connection: reconnect, resync, ordered ingest.
//! * [`components`] — session-tabs (golden-matched), route skeletons,
//!   tool rows, palette skeleton.
//! * [`spike`] — the wave-7.0 renderer-substrate spike, now a thin shim
//!   over the production modules (kept as the absorption proof).

pub mod components;
pub mod data;
pub mod golden_tests;
pub mod keymap;
pub mod layout;
pub mod reactive;
pub mod render;
pub mod spike;
pub mod store;
pub mod theme;
pub mod wrap;
