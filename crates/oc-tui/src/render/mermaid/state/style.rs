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

//! `state/style.ts` — the default theme RGB values and the ramp
//! interpolation between note-connector/state-border/transition hues.

use crate::render::mermaid::core::color::{color_ramp, Rgba};
use crate::render::mermaid::state::types::StateCellStyle;

/// `StateStyleColors` — one resolved color per cell style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateStyleColors {
    pub state: Rgba,
    pub state_border: Rgba,
    pub composite: Rgba,
    pub composite_label: Rgba,
    pub transition: Rgba,
    pub label: Rgba,
    pub note_border: Rgba,
    pub note_text: Rgba,
    pub note_connector: Rgba,
    pub note_connector_ramp1: Rgba,
    pub note_connector_ramp2: Rgba,
    pub note_connector_ramp3: Rgba,
    pub state_departure_ramp1: Rgba,
    pub state_departure_ramp2: Rgba,
    pub state_departure_ramp3: Rgba,
    pub start: Rgba,
    pub end: Rgba,
    pub choice: Rgba,
}

impl StateStyleColors {
    /// `colors[style]` — the lookup `renderDiagramGridStyledTextByStyle` does.
    #[must_use]
    pub fn color(&self, style: Option<StateCellStyle>) -> Option<Rgba> {
        let style = style?;
        Some(match style {
            StateCellStyle::State => self.state,
            StateCellStyle::StateBorder => self.state_border,
            StateCellStyle::Composite => self.composite,
            StateCellStyle::CompositeLabel => self.composite_label,
            StateCellStyle::Transition => self.transition,
            StateCellStyle::Label => self.label,
            StateCellStyle::NoteBorder => self.note_border,
            StateCellStyle::NoteText => self.note_text,
            StateCellStyle::NoteConnector => self.note_connector,
            StateCellStyle::NoteConnectorRamp1 => self.note_connector_ramp1,
            StateCellStyle::NoteConnectorRamp2 => self.note_connector_ramp2,
            StateCellStyle::NoteConnectorRamp3 => self.note_connector_ramp3,
            StateCellStyle::StateDepartureRamp1 => self.state_departure_ramp1,
            StateCellStyle::StateDepartureRamp2 => self.state_departure_ramp2,
            StateCellStyle::StateDepartureRamp3 => self.state_departure_ramp3,
            StateCellStyle::Start => self.start,
            StateCellStyle::End => self.end,
            StateCellStyle::Choice => self.choice,
        })
    }
}

/// `StateColorInput` — the overrides the markdown seam passes
/// (`color(...)` results upstream are already-resolved RGBAs).
#[derive(Debug, Clone, Copy, Default)]
pub struct StateColorInput {
    pub state: Option<Rgba>,
    pub state_border: Option<Rgba>,
    pub composite: Option<Rgba>,
    pub composite_label: Option<Rgba>,
    pub transition: Option<Rgba>,
    pub label: Option<Rgba>,
    pub note_border: Option<Rgba>,
    pub note_text: Option<Rgba>,
    pub note_connector: Option<Rgba>,
    pub start: Option<Rgba>,
    pub end: Option<Rgba>,
    pub choice: Option<Rgba>,
}

/// `createColorRampTheme(keys, from, to)` — the three numbered keys
/// blend `from → to` at 1/4, 1/2, 3/4.
fn ramp3(from: Rgba, to: Rgba) -> [Rgba; 3] {
    let ramp = color_ramp(3, Some(from), Some(to));
    [ramp[0].unwrap_or(to), ramp[1].unwrap_or(to), ramp[2].unwrap_or(to)]
}

/// `resolveStateStyleColors`.
#[must_use]
pub fn resolve_state_style_colors(input: StateColorInput) -> StateStyleColors {
    let state = input.state.unwrap_or(Rgba::from_ints(228, 239, 232, 255));
    let state_border =
        input.state_border.or(input.state).unwrap_or(Rgba::from_ints(228, 239, 232, 255));
    let composite = input.composite.unwrap_or(Rgba::from_ints(111, 138, 126, 255));
    let composite_label =
        input.composite_label.or(input.composite).unwrap_or(Rgba::from_ints(111, 138, 126, 255));
    let transition = input.transition.unwrap_or(Rgba::from_ints(134, 225, 200, 255));
    let label = input.label.unwrap_or(Rgba::from_ints(134, 225, 200, 255));
    let note_border = input.note_border.unwrap_or(Rgba::from_ints(141, 169, 155, 255));
    let note_text = input.note_text.unwrap_or(Rgba::from_ints(215, 229, 221, 255));
    let note_connector = input.note_connector.unwrap_or(Rgba::from_ints(141, 169, 155, 255));
    let start = input.start.unwrap_or(Rgba::from_ints(134, 225, 200, 255));
    let end = input.end.unwrap_or(Rgba::from_ints(230, 177, 126, 255));
    let choice = input.choice.unwrap_or(Rgba::from_ints(134, 225, 200, 255));

    let note_ramp = ramp3(note_connector, note_border);
    let departure_ramp = ramp3(state_border, transition);
    StateStyleColors {
        state,
        state_border,
        composite,
        composite_label,
        transition,
        label,
        note_border,
        note_text,
        note_connector,
        note_connector_ramp1: note_ramp[0],
        note_connector_ramp2: note_ramp[1],
        note_connector_ramp3: note_ramp[2],
        state_departure_ramp1: departure_ramp[0],
        state_departure_ramp2: departure_ramp[1],
        state_departure_ramp3: departure_ramp[2],
        start,
        end,
        choice,
    }
}
