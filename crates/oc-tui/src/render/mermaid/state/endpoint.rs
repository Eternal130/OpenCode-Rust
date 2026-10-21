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

//! `state/endpoint.ts` + `state/options.ts` — `[*]` endpoint markers
//! and the render default constants.

use super::types::{StateBorderStyle, StateDirection};

pub const DEFAULT_STATE_DIAGRAM_MIN_STATE_GAP: i64 = 5;
pub const DEFAULT_STATE_BORDER_STYLE: StateBorderStyle = StateBorderStyle::Rounded;
pub const DEFAULT_STATE_ARROW_HEAD_STYLE: crate::render::mermaid::core::drawing::ArrowHeadStyle =
    crate::render::mermaid::core::drawing::ArrowHeadStyle::Filled;

/// `DEFAULT_DIRECTION` — the parser default when no `direction` line.
pub const DEFAULT_STATE_DIRECTION: StateDirection = StateDirection::Lr;

/// `normalizeStateMinStateGap`.
#[must_use]
pub fn normalize_state_min_state_gap(value: Option<i64>) -> i64 {
    value.map_or(DEFAULT_STATE_DIAGRAM_MIN_STATE_GAP, |value| 1.max(value))
}

/// The `"from" | "to"` endpoint position of `stateDiagramMarkerId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointPosition {
    From,
    To,
}

/// `stateDiagramStartMarkerId`.
#[must_use]
pub fn state_diagram_start_marker_id(scope: Option<&str>) -> String {
    scope.map_or("__start".to_string(), |scope| format!("{scope}.__start"))
}

/// `stateDiagramEndMarkerId`.
#[must_use]
pub fn state_diagram_end_marker_id(scope: Option<&str>) -> String {
    scope.map_or("__end".to_string(), |scope| format!("{scope}.__end"))
}

/// `stateDiagramMarkerId`.
#[must_use]
pub fn state_diagram_marker_id(position: EndpointPosition, scope: Option<&str>) -> String {
    match position {
        EndpointPosition::From => state_diagram_start_marker_id(scope),
        EndpointPosition::To => state_diagram_end_marker_id(scope),
    }
}

/// `normalizeStateDiagramEndpoint` — `[*]` resolves to the scoped
/// start/end marker.
#[must_use]
pub fn normalize_state_diagram_endpoint(
    value: &str,
    position: EndpointPosition,
    scope: Option<&str>,
) -> String {
    if value == "[*]" {
        state_diagram_marker_id(position, scope)
    } else {
        value.to_string()
    }
}
