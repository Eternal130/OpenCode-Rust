//! # oc-tui-core — the opencode TUI interaction-contract crate
//!
//! Wave A lane (ROADMAP §架构决策 B): the TUI's INTERACTION half as a
//! proper crate, ported from the v2 reference (v2.0.24) with
//! `file:line` citations on every ported semantic:
//!
//! * [`keymap`]: the `@opentui/keymap` engine contract (layer
//!   precedence, focus-scope resolution, mode gating, pending
//!   sequences / which-key data, `preventDefault` object semantics,
//!   rejection fall-through, interceptor precedence) + the full v2
//!   default binding table and the oc-tui [`keymap::Keymap`] facade.
//! * [`text_edit`]: the textarea editing engine — v2's
//!   `input.*` command surface (`config/keybind.ts:206-243`) over the
//!   REAL `@opentui/core` textarea line (`EditBuffer`/`EditorView`:
//!   selection, word motions, delete-to-line ops, undo/redo), plus
//!   the composer-facing [`text_edit::EditCore`] seam.
//! * [`scroll`]: the session transcript's scroll driver — v2's six
//!   scroll commands (`routes/session/index.tsx:763-805`) over the
//!   follow/sticky state machine (`FollowState`), with the
//!   at-bottom/sticky semantics of the vscrollbox
//!   (index.tsx:1392-1393, `stickyScroll`/`stickyStart="bottom"`).
//!
//! The crate is render-free and store-free: pure state machines and
//! data contracts that oc-tui mounts.
//!
//! Lane M-engine-retire: the opentui-rust ports are the ONLY engine
//! path — the hand-rolled keymap dispatch engine and text-edit
//! composer engine are gone (the lane that replaced them ran default-
//! ON for one release, per the M-engine plan).

pub mod keymap;
pub mod scroll;
pub mod text_edit;
