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

//! `state/types.ts` — the state-diagram model and render options.

use crate::render::mermaid::core::canvas::CanvasSizeError;
use crate::render::mermaid::core::drawing::ArrowHeadStyle;
use crate::render::mermaid::diagnostics::MermaidSyntaxError;

/// `StateDiagramDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StateDirection {
    Tb,
    Td,
    Bt,
    #[default]
    Lr,
    Rl,
}

impl StateDirection {
    /// `normalizeDirection` — case-insensitive parse with the `LR`
    /// default applied by the parser.
    pub fn parse(value: &str) -> Option<Self> {
        let upper = value.to_uppercase();
        Some(match upper.as_str() {
            "TB" => Self::Tb,
            "TD" => Self::Td,
            "BT" => Self::Bt,
            "LR" => Self::Lr,
            "RL" => Self::Rl,
            _ => return None,
        })
    }
}

/// `StateDiagramState["kind"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    State,
    Start,
    End,
    Choice,
}

/// `StateDiagramState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDiagramState {
    pub id: String,
    pub label: String,
    pub kind: StateKind,
    pub parent_id: Option<String>,
}

/// `StateDiagramTransition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDiagramTransition {
    pub from: String,
    pub to: String,
    pub label: String,
}

/// `StateDiagramCompositeState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDiagramCompositeState {
    pub id: String,
    pub label: String,
    pub parent_id: Option<String>,
}

/// `StateDiagramNote["position"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotePosition {
    Left,
    Right,
}

/// `StateDiagramNote`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDiagramNote {
    pub target: String,
    pub position: NotePosition,
    pub lines: Vec<String>,
}

/// `StateDiagram` (also serves as `StateVisibleDiagram` — the visible
/// model has the same shape, upstream only re-types the transitions).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateDiagram {
    pub direction: StateDirection,
    pub states: Vec<StateDiagramState>,
    pub transitions: Vec<StateDiagramTransition>,
    pub composites: Vec<StateDiagramCompositeState>,
    pub notes: Vec<StateDiagramNote>,
}

/// `BorderStyle` (the two tables the diagram core ships).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateBorderStyle {
    Rounded,
    Double,
}

/// `StateDiagramRenderOptions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct StateDiagramRenderOptions {
    pub direction: Option<StateDirection>,
    pub border_style: Option<StateBorderStyle>,
    pub arrow_head_style: Option<ArrowHeadStyle>,
    pub min_state_gap: Option<i64>,
    /// Target rendered width. Oversized horizontal layouts fold vertically.
    pub layout_max_width: Option<i64>,
}

/// `BaseStateCellStyle | NoteConnectorRampStyle | StateDepartureRampStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateCellStyle {
    State,
    StateBorder,
    Composite,
    CompositeLabel,
    Transition,
    Label,
    NoteBorder,
    NoteText,
    NoteConnector,
    NoteConnectorRamp1,
    NoteConnectorRamp2,
    NoteConnectorRamp3,
    StateDepartureRamp1,
    StateDepartureRamp2,
    StateDepartureRamp3,
    Start,
    End,
    Choice,
}

/// The state renderer's failure modes (upstream throws; the markdown
/// seam catches everything and falls back to the plain code block).
#[derive(Debug)]
pub enum StateRenderError {
    Syntax(MermaidSyntaxError),
    CanvasSize(CanvasSizeError),
    /// `"State X has no exterior note corridor"` / convergence guard.
    Layout(String),
    /// `"Transition X has no clear label position"`.
    Routing(String),
}

impl std::fmt::Display for StateRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(error) => write!(f, "{error}"),
            Self::CanvasSize(_) => write!(f, "diagram canvas size error"),
            Self::Layout(message) => write!(f, "{message}"),
            Self::Routing(message) => write!(f, "{message}"),
        }
    }
}

impl From<MermaidSyntaxError> for StateRenderError {
    fn from(error: MermaidSyntaxError) -> Self {
        Self::Syntax(error)
    }
}

impl From<CanvasSizeError> for StateRenderError {
    fn from(error: CanvasSizeError) -> Self {
        Self::CanvasSize(error)
    }
}
