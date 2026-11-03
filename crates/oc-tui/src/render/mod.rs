//! Frame output: painted buffers plus the canonical serialization reused
//! by every frame test (the wave-7.0 spike's format, now the crate-wide
//! seam with `tools/parity::tui_frame`).

pub mod ansi;
pub mod color_mode;
pub mod frame;
pub mod latex;
pub mod mermaid;
pub mod paint;

pub use ansi::serialize_canonical;
pub use frame::{CONTINUATION, Cell, CellColor, ClusterMap, Frame};
pub use paint::{Fill, paint_placed, paint_placed_with_fills, to_frame};
