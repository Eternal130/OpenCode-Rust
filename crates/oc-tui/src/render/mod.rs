//! Frame output: painted buffers plus the canonical serialization reused
//! by every frame test (the wave-7.0 spike's format, now the crate-wide
//! seam with `tools/parity::tui_frame`).

pub mod ansi;
pub mod frame;
pub mod paint;

pub use ansi::serialize_canonical;
pub use frame::{Cell, CellColor, Frame};
pub use paint::{paint_placed, paint_placed_with_fills, to_frame, Fill};
