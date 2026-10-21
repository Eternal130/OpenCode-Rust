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

//! `sequence/types.ts` + `options.ts`.

use crate::render::mermaid::core::color::Rgba;

/// `SequenceParticipant`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceParticipant {
    pub id: String,
    pub label: String,
}

/// `SequenceParticipantGroup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceParticipantGroup {
    pub label: String,
    pub participant_ids: Vec<String>,
}

/// `SequenceArrowHead`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceArrowHead {
    Open,
    Cross,
    Async,
}

/// `SequenceMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceMessage {
    pub from: String,
    pub to: String,
    pub label: String,
    pub dashed: bool,
    pub head: Option<SequenceArrowHead>,
    pub number: Option<i64>,
    pub activate: Option<String>,
    pub deactivate: Option<String>,
}

/// `SequenceNote`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceNote {
    pub over: Vec<String>,
    pub label: String,
    pub position: Option<NotePosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotePosition {
    Left,
    Right,
}

/// `SequenceActivation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceActivation {
    pub participant: String,
    pub active: bool,
}

/// `SequenceFragment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentKind {
    Alt,
    Else,
    Loop,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceFragment {
    pub kind: FragmentKind,
    pub label: String,
}

/// `SequenceStep`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceStep {
    Message(SequenceMessage),
    Note(SequenceNote),
    Activation(SequenceActivation),
    Fragment(SequenceFragment),
}

/// `SequenceDiagram`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SequenceDiagram {
    pub participants: Vec<SequenceParticipant>,
    pub messages: Vec<SequenceMessage>,
    pub steps: Vec<SequenceStep>,
    pub groups: Vec<SequenceParticipantGroup>,
}

/// `SequenceDiagramRenderOptions` (`options.ts`: defaults).
#[derive(Debug, Clone, Copy, Default)]
pub struct SequenceRenderOptions {
    pub compact: bool,
    pub min_participant_gap: Option<i64>,
}

pub const DEFAULT_MIN_PARTICIPANT_GAP: i64 = 18;

/// `SequenceCellStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SequenceStyle {
    Participant,
    Lifeline,
    Group,
    Request,
    Response,
    RequestFade(u8),
    ResponseFade(u8),
    LifelineRamp(u8),
    Fragment,
    FragmentLabel,
    Note,
    NoteBadge,
}

/// `SequenceStyleColors` (style.ts).
#[derive(Debug, Clone, Copy)]
pub struct SequenceColors {
    pub participant: Rgba,
    pub lifeline: Rgba,
    pub group: Rgba,
    pub request: Rgba,
    pub response: Rgba,
    pub fragment: Rgba,
    pub fragment_label_bg: Rgba,
    pub note: Rgba,
    pub note_bg: Rgba,
    pub request_fades: [Rgba; 5],
    pub response_fades: [Rgba; 5],
    pub lifeline_ramps: [Rgba; 3],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SequenceColorInput {
    pub participant: Option<Rgba>,
    pub lifeline: Option<Rgba>,
    pub group: Option<Rgba>,
    pub request: Option<Rgba>,
    pub response: Option<Rgba>,
    pub fragment: Option<Rgba>,
    pub fragment_label_bg: Option<Rgba>,
    pub note: Option<Rgba>,
    pub note_bg: Option<Rgba>,
}

const DEFAULT_SEQUENCE: [(u8, u8, u8); 10] = [
    (228, 239, 232), // participant
    (111, 138, 126), // lifeline
    (76, 99, 89),    // group
    (134, 225, 200), // request
    (230, 177, 126), // response
    (154, 184, 169), // fragment
    (28, 43, 36),    // fragmentLabelBg
    (215, 229, 221), // noteFg
    (36, 56, 47),    // noteBg
    (0, 0, 0),       // spare
];

fn rgb(index: usize) -> Rgba {
    let (r, g, b) = DEFAULT_SEQUENCE[index];
    Rgba::from_ints(r, g, b, 255)
}

impl SequenceColors {
    #[must_use]
    pub fn resolve(input: SequenceColorInput) -> Self {
        use crate::render::mermaid::core::color::{blend_color, color_ramp};
        let participant = input.participant.unwrap_or_else(|| rgb(0));
        let lifeline = input.lifeline.unwrap_or_else(|| rgb(1));
        let request = input.request.unwrap_or_else(|| rgb(3));
        let response = input.response.unwrap_or_else(|| rgb(4));
        let resolve_fades = |from: Rgba, to: Rgba| -> [Rgba; 5] {
            ramp5(
                blend_color(Some(from), Some(to), 0.0).unwrap(),
                color_ramp(5, Some(from), Some(to)),
            )
        };
        let ramp3 = color_ramp(3, Some(participant), Some(lifeline));
        let mut lifeline_ramps = [participant; 3];
        for (index, color) in ramp3.into_iter().enumerate() {
            lifeline_ramps[index] = color.unwrap_or(participant);
        }
        Self {
            participant,
            lifeline,
            group: input.group.unwrap_or_else(|| rgb(2)),
            request,
            response,
            fragment: input.fragment.unwrap_or_else(|| rgb(5)),
            fragment_label_bg: input.fragment_label_bg.unwrap_or_else(|| rgb(6)),
            note: input.note.unwrap_or_else(|| rgb(7)),
            note_bg: input.note_bg.unwrap_or_else(|| rgb(8)),
            request_fades: resolve_fades(lifeline, request),
            response_fades: resolve_fades(lifeline, response),
            lifeline_ramps,
        }
    }

    /// `sequenceStyleColor`.
    #[must_use]
    pub fn color(&self, style: Option<SequenceStyle>) -> Option<Rgba> {
        Some(match style? {
            SequenceStyle::Participant => self.participant,
            SequenceStyle::Lifeline => self.lifeline,
            SequenceStyle::Group => self.group,
            SequenceStyle::Request => self.request,
            SequenceStyle::Response => self.response,
            SequenceStyle::RequestFade(step) => self.request_fades[(step - 1) as usize],
            SequenceStyle::ResponseFade(step) => self.response_fades[(step - 1) as usize],
            SequenceStyle::LifelineRamp(step) => self.lifeline_ramps[(step - 1) as usize],
            SequenceStyle::Fragment => self.fragment,
            SequenceStyle::FragmentLabel => self.fragment,
            SequenceStyle::Note => self.note,
            SequenceStyle::NoteBadge => self.note,
        })
    }

    /// `sequenceStyleBackgroundColor`.
    #[must_use]
    pub fn background(&self, style: Option<SequenceStyle>) -> Option<Rgba> {
        Some(match style? {
            SequenceStyle::Note | SequenceStyle::NoteBadge => self.note_bg,
            SequenceStyle::FragmentLabel => self.fragment_label_bg,
            _ => return None,
        })
    }
}

fn ramp5(seed: Rgba, ramp: Vec<Option<Rgba>>) -> [Rgba; 5] {
    let mut fades = [seed; 5];
    for (index, color) in ramp.into_iter().enumerate() {
        fades[index] = color.unwrap_or(seed);
    }
    fades
}
