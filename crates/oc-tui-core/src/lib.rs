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
//! * [`text_edit`]: the textarea editing primitives — v2's
//!   `input.*` command surface (`config/keybind.ts:206-243`) against
//!   the `EditBufferRenderable` operation set (`@opentui/core`
//!   renderables/EditBufferRenderable.d.ts:140-205): selection,
//!   word motions, delete-to-line ops, undo/redo.
//! * [`scroll`]: the session transcript's scroll driver — v2's six
//!   scroll commands (`routes/session/index.tsx:763-805`) over the
//!   follow/sticky state machine (`FollowState`), with the
//!   at-bottom/sticky semantics of the vscrollbox
//!   (index.tsx:1392-1393, `stickyScroll`/`stickyStart="bottom"`).
//!
//! The crate is render-free and store-free: pure state machines and
//! data contracts that oc-tui mounts.

pub mod keymap;
pub mod scroll;
pub mod text_edit;
