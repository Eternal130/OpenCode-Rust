// Upstream-mirror lint posture: the merman port keeps the TypeScript
// arithmetic shape (JS Number semantics on an i64 grid, the same cast
// sites and long single-responsibility functions), so the pedantic
// numeric/style lints that would rewrite that shape are quieted here.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::explicit_iter_loop,
    clippy::for_kv_map,
    clippy::map_unwrap_or,
    clippy::manual_strip,
    clippy::range_plus_one,
    clippy::option_option,
    clippy::implicit_hasher,
    clippy::match_same_arms
)]

//! `state/note.ts` — the default note-to-target connector.

use crate::render::mermaid::core::geometry::Point;
use crate::render::mermaid::state::types::NotePosition;

/// `StateDiagramNoteConnector`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDiagramNoteConnector {
    pub points: Vec<Point>,
    pub connector_y: i64,
}

/// The note-side geometry `stateDiagramNoteConnector` consumes.
#[derive(Debug, Clone, Copy)]
pub struct NoteConnectorGeometry {
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
    pub center_y: i64,
    pub position: NotePosition,
}

/// The target-side geometry.
#[derive(Debug, Clone, Copy)]
pub struct TargetConnectorGeometry {
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
    pub center_y: i64,
}

/// `stateDiagramNoteConnector` — recomputed from the current bounds
/// (upstream memoizes on the note bound; shifts update both, so the
/// recomputation is identical).
#[must_use]
pub fn state_diagram_note_connector(
    bounds: &NoteConnectorGeometry,
    target: &TargetConnectorGeometry,
) -> StateDiagramNoteConnector {
    let note_x = match bounds.position {
        NotePosition::Right => bounds.left - 1,
        NotePosition::Left => bounds.left + bounds.width,
    };
    let target_x = match bounds.position {
        NotePosition::Right => target.left + target.width,
        NotePosition::Left => target.left - 1,
    };
    let target_bottom = target.top + target.height - 1;
    let note_bottom = bounds.top + bounds.height - 1;
    let note_above = note_bottom < target.top;
    let note_below = bounds.top > target_bottom;

    if note_above || note_below {
        let connector_y = bounds.center_y;
        return StateDiagramNoteConnector {
            connector_y,
            points: vec![
                Point {
                    x: target_x,
                    y: if note_above { target.top - 1 } else { target_bottom + 1 },
                },
                Point { x: target_x, y: connector_y },
                Point { x: note_x, y: connector_y },
            ],
        };
    }

    let connector_y = (bounds.top + 1).max(target.center_y.min(bounds.top + bounds.height - 2));
    StateDiagramNoteConnector {
        connector_y,
        points: vec![Point { x: target_x, y: connector_y }, Point { x: note_x, y: connector_y }],
    }
}
