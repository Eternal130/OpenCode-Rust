//! `opencode mini` — the split-footer interactive frontend
//! (`packages/tui/src/mini/`, R-02).
//!
//! Mini is an INLINE interactive chat loop — NOT an alternate-screen
//! TUI. Conversation output goes to the terminal's native scrollback
//! (a mono writer, one entry per block, per
//! `mini/scrollback.writer.tsx`), while a live footer stays glued to
//! the bottom of the screen (ratatui's inline viewport, never
//! `EnterAlternateScreen`) hosting the composer, permission asks,
//! forms and the picker panels.
//!
//! ## Module map (each mirrors one v2 file)
//!
//! * [`mono`] — the ASCII-folding tables (`mini/mono.ts`)
//! * [`splash`] — entry/exit scrollback banners (`mini/splash.ts`)
//! * [`prompt_shared`] — the `/exit` `/new` `/compact` commands and the
//!   history ring (`mini/prompt.shared.ts`)
//! * [`scrollback`] — transcript commits → mono lines
//!   (`mini/entry.body.ts` + `mini/scrollback.writer.tsx`)
//! * [`model_pref`] — `state/model.json` variant preferences
//!   (`tui/model-preference.ts`)
//! * [`queue`] — the serial prompt queue (`mini/runtime.queue.ts`)
//! * [`lifecycle`] — renderer boot + splash + teardown order
//!   (`mini/runtime.lifecycle.ts`)
//! * [`transport`] — SSE events → footer state + scrollback commits
//!   (`mini/stream-v2.transport.ts`)
//! * [`demo`] — `--demo` offline mode with `/permission` `/form`
//!   `/fmt` (`mini/demo.ts`)
//! * [`spinner`] — the work-spinner frame tables
//!   (`ui/one-cell-motion.ts`, `mini.work_spinner`)
//! * [`runtime`] — the orchestrator: boot → lifecycle → transport →
//!   queue (`mini/runtime.ts`)
//!
//! The footer family itself (composer, permission body, forms,
//! pickers) is NOT duplicated here — it lives in
//! [`crate::components::footer`], which IS the port of
//! `mini/footer*.tsx`.

pub mod demo;
pub mod lifecycle;
pub mod model_pref;
pub mod mono;
pub mod prompt_shared;
pub mod queue;
pub mod runtime;
pub mod scrollback;
pub mod spinner;
pub mod splash;
pub mod theme;
pub mod transport;

/// The docked footer height (`FOOTER_HEIGHT`,
/// `mini/runtime.lifecycle.ts:41`) — the inline viewport the renderer
/// keeps glued to the bottom of the terminal.
pub const FOOTER_HEIGHT: usize = 4;

/// `PERMISSION_ROWS` (`mini/footer.ts:117`): the permission body adds
/// these rows over the composer base while a blocker is live.
pub const PERMISSION_ROWS: usize = 12;

/// `FORM_ROWS` (`mini/footer.ts:118`).
pub const FORM_ROWS: usize = 14;

/// The composer base height (`footerHeight - TEXTAREA_MIN_ROWS`,
/// `mini/footer.ts:314` with `TEXTAREA_MIN_ROWS = 1`).
#[must_use]
pub fn footer_base(footer_height: usize) -> usize {
    footer_height.saturating_sub(1).max(1)
}
