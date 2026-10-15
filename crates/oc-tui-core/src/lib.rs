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
pub mod text_edit_opentui;

/// Lane M-engine's subsystem flag (`OC_ENGINE_OPENTUI`): selects the
/// opentui-rust ports for BOTH the keymap engine (opentui-keymap
/// dispatch, dissolving the K-1..K-9 deltas) and the composer editing
/// engine (opentui-core's EditBuffer/EditorView textarea line). The
/// hand-rolled oc-tui-core implementations stay compiled as the `=0`
/// fallback for one release.
///
/// The M-lane OnceLock pattern: read once at first use ("at boot") —
/// a running session never switches engines mid-flight. DEFAULT ON
/// after the lane's proof (the unit parity table over the whole
/// binding table, 2883 workspace tests green on both paths, the rig
/// A/B flag-symmetric across the composer/leader corpus, both PTY
/// profiles, the keystroke budget pair); `OC_ENGINE_OPENTUI=0`
/// keeps the hand-rolled pair for one release (the retirement lane
/// removes them).
#[must_use]
pub fn opentui_engine_enabled() -> bool {
    cfg_if_test_override().unwrap_or_else(|| {
        static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FLAG.get_or_init(|| {
            !std::env::var("OC_ENGINE_OPENTUI")
                .is_ok_and(|value| value == "0" || value == "false" || value == "off")
        })
    })
}

/// The in-process override for the engine-path parity tests: the flag
/// latches once per process, so libtest's shared process cannot A/B
/// through the env — the override wins until cleared. `None` in
/// non-test builds.
#[cfg(test)]
static TEST_ENGINE_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(test)]
fn cfg_if_test_override() -> Option<bool> {
    match TEST_ENGINE_OVERRIDE.load(std::sync::atomic::Ordering::SeqCst) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
#[doc(hidden)]
pub fn __set_test_engine_override(enabled: Option<bool>) {
    let value = match enabled {
        Some(false) => 1,
        Some(true) => 2,
        None => 0,
    };
    TEST_ENGINE_OVERRIDE.store(value, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(not(test))]
const fn cfg_if_test_override() -> Option<bool> {
    None
}
