//! Typed resolved theme tokens (`ResolvedTheme` / `ResolvedThemeTokens`),
//! ported from `packages/theme/src/tui/types.ts`.
//!
//! `Serialize` mirrors the golden JSON shape used by the parity tests:
//! colors serialize as `[r, g, b, a]` integer arrays, hue scales as
//! `{"100": ..., ..., "900": ...}` maps, and stateful colors carry
//! `base` plus the five action states (renamed from `$hovered` etc.).

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::color::Rgba;
use crate::schema::{ACTION_STATES, HUE_STEPS};

/// Theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }

    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Mode::Light),
            "dark" => Some(Mode::Dark),
            _ => None,
        }
    }
}

/// Semantic hue names required in every hue definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticHue {
    Accent,
    Interactive,
    Neutral,
}

impl SemanticHue {
    pub const fn as_str(self) -> &'static str {
        match self {
            SemanticHue::Accent => "accent",
            SemanticHue::Interactive => "interactive",
            SemanticHue::Neutral => "neutral",
        }
    }
}

/// Action variants (`primary` / `secondary` / `destructive`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionVariant {
    Primary,
    Secondary,
    Destructive,
}

impl ActionVariant {
    pub const ALL: [ActionVariant; 3] = [Self::Primary, Self::Secondary, Self::Destructive];

    pub const fn as_str(self) -> &'static str {
        match self {
            ActionVariant::Primary => "primary",
            ActionVariant::Secondary => "secondary",
            ActionVariant::Destructive => "destructive",
        }
    }
}

/// Feedback kinds (`error` / `warning` / `success` / `info`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FeedbackKind {
    Error,
    Warning,
    Success,
    Info,
}

impl FeedbackKind {
    pub const ALL: [FeedbackKind; 4] = [Self::Error, Self::Warning, Self::Success, Self::Info];

    pub const fn as_str(self) -> &'static str {
        match self {
            FeedbackKind::Error => "error",
            FeedbackKind::Warning => "warning",
            FeedbackKind::Success => "success",
            FeedbackKind::Info => "info",
        }
    }
}

/// Inputs to [`StatefulColor::state`]: the first active state in the schema
/// order `disabled, pressed, focused, selected, hovered` wins, else `base`.
/// Mirrors the TS `Partial<Record<ActionState, boolean>>` argument.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ActionStates {
    pub disabled: bool,
    pub pressed: bool,
    pub focused: bool,
    pub selected: bool,
    pub hovered: bool,
}

/// A color plus its per-state variants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StatefulColor {
    pub base: Rgba,
    pub hovered: Rgba,
    pub focused: Rgba,
    pub pressed: Rgba,
    pub selected: Rgba,
    pub disabled: Rgba,
}

impl StatefulColor {
    /// Pick the color for the given states (TS `StatefulColor.state`).
    pub fn state(&self, states: ActionStates) -> Rgba {
        let flags =
            [states.disabled, states.pressed, states.focused, states.selected, states.hovered];
        for (state, active) in ACTION_STATES.iter().zip(flags) {
            if active {
                return match *state {
                    "disabled" => self.disabled,
                    "pressed" => self.pressed,
                    "focused" => self.focused,
                    "selected" => self.selected,
                    _ => self.hovered,
                };
            }
        }
        self.base
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ActionColors {
    pub primary: StatefulColor,
    pub secondary: StatefulColor,
    pub destructive: StatefulColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TextFeedbackColors {
    pub error: TextFeedback,
    pub warning: TextFeedback,
    pub success: TextFeedback,
    pub info: TextFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TextFeedback {
    pub base: Rgba,
    pub muted: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BackgroundFeedbackColors {
    pub error: BackgroundFeedback,
    pub warning: BackgroundFeedback,
    pub success: BackgroundFeedback,
    pub info: BackgroundFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BackgroundFeedback {
    pub base: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ResolvedText {
    pub base: Rgba,
    pub muted: Rgba,
    pub action: ActionColors,
    pub formfield: StatefulColor,
    pub feedback: TextFeedbackColors,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RaisedSurfaces {
    pub base: Rgba,
    pub high: Rgba,
    pub max: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ResolvedBackground {
    pub base: Rgba,
    pub raised: RaisedSurfaces,
    pub action: ActionColors,
    pub formfield: StatefulColor,
    pub feedback: BackgroundFeedbackColors,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ResolvedDiff {
    pub text: DiffText,
    pub background: DiffBackground,
    pub highlight: DiffHighlight,
    #[serde(rename = "lineNumber")]
    pub line_number: DiffLineNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DiffText {
    pub added: Rgba,
    pub removed: Rgba,
    pub context: Rgba,
    #[serde(rename = "hunkHeader")]
    pub hunk_header: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DiffBackground {
    pub added: Rgba,
    pub removed: Rgba,
    pub context: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DiffHighlight {
    pub added: Rgba,
    pub removed: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DiffLineNumber {
    pub text: Rgba,
    pub background: DiffLineNumberBackground,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DiffLineNumberBackground {
    pub added: Rgba,
    pub removed: Rgba,
}

/// The nine-step hue scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HueScale {
    #[serde(rename = "100")]
    pub s100: Rgba,
    #[serde(rename = "200")]
    pub s200: Rgba,
    #[serde(rename = "300")]
    pub s300: Rgba,
    #[serde(rename = "400")]
    pub s400: Rgba,
    #[serde(rename = "500")]
    pub s500: Rgba,
    #[serde(rename = "600")]
    pub s600: Rgba,
    #[serde(rename = "700")]
    pub s700: Rgba,
    #[serde(rename = "800")]
    pub s800: Rgba,
    #[serde(rename = "900")]
    pub s900: Rgba,
}

impl HueScale {
    /// All steps in order (`100 ..= 900`).
    pub fn steps(self) -> [Rgba; 9] {
        [
            self.s100, self.s200, self.s300, self.s400, self.s500, self.s600, self.s700, self.s800,
            self.s900,
        ]
    }

    pub fn step(&self, step: u16) -> Option<Rgba> {
        let index = HUE_STEPS.iter().position(|s| *s == step.to_string())?;
        Some(self.steps()[index])
    }
}

pub type HueMap = BTreeMap<String, HueScale>;

/// Where a color sits in a hue scale, and shifting along it
/// (TS `compileHueSteps`). The TS version keys a `WeakMap` on `RGBA` object
/// identity; here colors are matched by value, which is equivalent for every
/// color produced through `$hue.<name>.<step>` references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HueStepIndex {
    index: HashMap<[u8; 4], (String, u16, usize)>,
}

impl HueStepIndex {
    pub fn new(hue: &HueMap) -> Self {
        let mut index = HashMap::new();
        for (name, scale) in hue {
            for (position, color) in scale.steps().iter().enumerate() {
                let step: u16 = HUE_STEPS[position].parse().expect("step literal");
                index.insert(color.to_ints(), (name.clone(), step, position));
            }
        }
        Self { index }
    }

    pub fn source(&self, color: Rgba) -> Option<(&str, u16)> {
        self.index.get(&color.to_ints()).map(|(name, step, _)| (name.as_str(), *step))
    }

    pub fn increase(&self, hue: &HueMap, color: Rgba, amount: f64) -> Rgba {
        self.shift(hue, color, amount)
    }

    pub fn decrease(&self, hue: &HueMap, color: Rgba, amount: f64) -> Rgba {
        self.shift(hue, color, -amount)
    }

    fn shift(&self, hue: &HueMap, color: Rgba, amount: f64) -> Rgba {
        let Some((name, _, position)) = self.index.get(&color.to_ints()) else {
            return color;
        };
        let offset = if amount.is_finite() { amount.trunc() as i64 } else { 0 };
        let position = (*position as i64 + offset).clamp(0, 8) as usize;
        hue[name].steps()[position]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SyntaxColors {
    pub comment: Rgba,
    pub keyword: Rgba,
    pub function: Rgba,
    pub variable: Rgba,
    pub string: Rgba,
    pub number: Rgba,
    #[serde(rename = "type")]
    pub r#type: Rgba,
    pub operator: Rgba,
    pub punctuation: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct MarkdownColors {
    #[serde(rename = "text")]
    pub text: Rgba,
    pub heading: Rgba,
    pub link: Rgba,
    #[serde(rename = "linkText")]
    pub link_text: Rgba,
    pub code: Rgba,
    #[serde(rename = "blockQuote")]
    pub block_quote: Rgba,
    pub emphasis: Rgba,
    pub strong: Rgba,
    #[serde(rename = "horizontalRule")]
    pub horizontal_rule: Rgba,
    #[serde(rename = "listItem")]
    pub list_item: Rgba,
    #[serde(rename = "listEnumeration")]
    pub list_enumeration: Rgba,
    pub image: Rgba,
    #[serde(rename = "imageText")]
    pub image_text: Rgba,
    #[serde(rename = "codeBlock")]
    pub code_block: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct BorderColor {
    pub base: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScrollbarColor {
    pub base: Rgba,
}

/// All resolved theme tokens for one view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedThemeTokens {
    pub hue: HueMap,
    pub categorical: Vec<HueScale>,
    #[serde(skip)]
    pub hue_steps: HueStepIndex,
    pub text: ResolvedText,
    pub background: ResolvedBackground,
    pub border: BorderColor,
    pub scrollbar: ScrollbarColor,
    pub diff: ResolvedDiff,
    pub syntax: SyntaxColors,
    pub markdown: MarkdownColors,
}

impl ResolvedThemeTokens {
    /// The hue scale for a semantic hue.
    pub fn semantic(&self, hue: SemanticHue) -> &HueScale {
        self.hue
            .get(hue.as_str())
            .unwrap_or_else(|| panic!("semantic hue {} is required", hue.as_str()))
    }

    /// Report the hue/step a color came from, if any.
    pub fn source(&self, color: Rgba) -> Option<(&str, u16)> {
        self.hue_steps.source(color)
    }

    /// Shift a color up the hue scale (brighter in dark themes).
    pub fn increase(&self, color: Rgba, amount: f64) -> Rgba {
        self.hue_steps.increase(&self.hue, color, amount)
    }

    /// Shift a color down the hue scale.
    pub fn decrease(&self, color: Rgba, amount: f64) -> Rgba {
        self.hue_steps.decrease(&self.hue, color, amount)
    }
}

/// A fully resolved theme, plus its surface views (`surface("dialog")`).
///
/// Serializes as the flat token map with a nested `dialog` key, matching the
/// golden dumps.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedTheme {
    #[serde(flatten)]
    pub tokens: ResolvedThemeTokens,
    pub dialog: Box<ResolvedThemeTokens>,
}

/// Surface names (`SurfaceName` in schema.ts — currently only `dialog`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceName {
    Dialog,
}

impl ResolvedTheme {
    /// The theme re-resolved on a raised surface (TS `theme.surface(name)`).
    /// Themes without a `@dialog` override reuse the base view.
    pub fn surface(&self, _name: SurfaceName) -> &ResolvedThemeTokens {
        &self.dialog
    }
}
