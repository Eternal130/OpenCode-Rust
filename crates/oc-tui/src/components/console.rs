//! TUI-29 — the renderer console overlay (app.tsx:242-244 + 1110-
//! 1118's `renderer.console`): the captured diagnostics tail painted
//! at the top of the screen while toggled on, with the console's own
//! `copy-selection` binding (ctrl+y → clipboard + toast + clear).

/// The ring's capacity (the console keeps the last 256 lines).
pub const CONSOLE_RING: usize = 256;

/// The overlay's row budget (opentui's console clamps to a top strip;
/// 12 rows — the border pair + 10 lines of tail).
pub const CONSOLE_ROWS: usize = 12;
