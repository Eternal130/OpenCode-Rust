//! Math node types + the source/nesting limits (upstream
//! `packages/latex/src/types.ts` + `limits.ts`).

use std::fmt;

/// `MathVariant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathVariant {
    Normal,
    Bold,
    Italic,
    Sans,
    Monospace,
    DoubleStruck,
    Script,
    Fraktur,
}

/// `SymbolRole`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolRole {
    Ordinary,
    Binary,
    Relation,
    Operator,
    Punctuation,
    Opening,
    Closing,
}

/// `MatrixEnvironment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixEnvironment {
    Matrix,
    Pmatrix,
    Bmatrix,
    BigBmatrix,
    Vmatrix,
    DoubleVmatrix,
    Cases,
    Aligned,
    Align,
    Gathered,
    Gather,
    SmallMatrix,
    Array,
}

impl MatrixEnvironment {
    /// The parser's environment-name table (matrixEnvironments).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "matrix" => Self::Matrix,
            "pmatrix" => Self::Pmatrix,
            "bmatrix" => Self::Bmatrix,
            "Bmatrix" => Self::BigBmatrix,
            "vmatrix" => Self::Vmatrix,
            "Vmatrix" => Self::DoubleVmatrix,
            "cases" => Self::Cases,
            "aligned" => Self::Aligned,
            "align" => Self::Align,
            "gathered" => Self::Gathered,
            "gather" => Self::Gather,
            "smallmatrix" => Self::SmallMatrix,
            "array" => Self::Array,
            _ => return None,
        })
    }
}

/// `AccentKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccentKind {
    Hat,
    WideHat,
    Bar,
    Overline,
    Underline,
    Vec,
    Tilde,
    Dot,
    DoubleDot,
}

/// Continued-fraction numerator alignment (`numeratorAlign`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumeratorAlign {
    Left,
    Right,
}

/// Over/under brace position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BracePosition {
    Over,
    Under,
}

/// `MathNode` (the discriminated union upstream spells as a TS union).
#[derive(Debug, Clone, PartialEq)]
pub enum MathNode {
    Row(Vec<MathNode>),
    Symbol {
        value: String,
        role: Option<SymbolRole>,
    },
    Text(String),
    Space(u32),
    Fraction {
        numerator: Box<MathNode>,
        denominator: Box<MathNode>,
        bar: bool,
        numerator_align: Option<NumeratorAlign>,
    },
    Root {
        body: Box<MathNode>,
        index: Option<Box<MathNode>>,
    },
    Scripts {
        base: Box<MathNode>,
        superscript: Option<Box<MathNode>>,
        subscript: Option<Box<MathNode>>,
    },
    Delimited {
        left: String,
        body: Box<MathNode>,
        right: String,
    },
    Matrix {
        rows: Vec<Vec<MathNode>>,
        environment: MatrixEnvironment,
        columns: Option<String>,
    },
    Brace {
        body: Box<MathNode>,
        position: BracePosition,
    },
    Accent {
        accent: AccentKind,
        body: Box<MathNode>,
    },
    Variant {
        variant: MathVariant,
        body: Box<MathNode>,
    },
    Operator {
        value: String,
        limits: bool,
    },
    OverUnder {
        base: Box<MathNode>,
        over: Option<Box<MathNode>>,
        under: Option<Box<MathNode>>,
    },
    Color {
        color: String,
        body: Box<MathNode>,
    },
}

/// `ParseOptions`.
#[derive(Debug, Clone, Default)]
pub struct ParseOptions {
    pub macros: Option<std::collections::HashMap<String, String>>,
    pub max_expand: Option<u32>,
    /// Maximum accepted input length (guards interactive/AI input).
    pub max_source_length: Option<u32>,
    /// Maximum length after user-macro expansion (default: `max_source_length`).
    pub max_expanded_length: Option<u32>,
    /// Maximum structural nesting depth.
    pub max_depth: Option<u32>,
    pub strict: bool,
}

/// `LatexParseError` + `RangeError`, unified: parse failures and
/// option-bound failures both fall back to the source fence in
/// upstream's markdown renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatexError {
    pub message: String,
    pub position: usize,
}

impl fmt::Display for LatexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at offset {}", self.message, self.position)
    }
}

impl std::error::Error for LatexError {}

pub(crate) fn latex_error(message: impl Into<String>, position: usize) -> LatexError {
    LatexError { message: message.into(), position }
}

pub(crate) const DEFAULT_MAX_SOURCE_LENGTH: u32 = 100_000;
pub(crate) const DEFAULT_MAX_NESTING_DEPTH: u32 = 256;

/// `resolvePositiveInteger` — non-positive/non-finite values become
/// the upstream `RangeError`.
pub(crate) fn resolve_positive_integer(
    value: Option<u32>,
    fallback: u32,
    option_name: &str,
) -> Result<u32, LatexError> {
    match value {
        None => Ok(fallback),
        Some(value) if value >= 1 => Ok(value),
        Some(_) => Err(latex_error(format!("{option_name} must be a positive safe integer"), 0)),
    }
}

/// `assertSourceLength`.
pub(crate) fn assert_source_length(
    source: &str,
    maximum: u32,
    label: &str,
) -> Result<(), LatexError> {
    if source.chars().count() > maximum as usize {
        return Err(latex_error(
            format!("{label} exceeds the {maximum}-character limit"),
            maximum as usize,
        ));
    }
    Ok(())
}

/// `assertNestingDepth` — brace-depth pre-scan (escaped braces skip).
pub(crate) fn assert_nesting_depth(source: &[char], maximum: u32) -> Result<(), LatexError> {
    let mut depth: i64 = 0;
    let mut slash_run = 0u32;
    for (index, &char) in source.iter().enumerate() {
        if char == '\\' {
            slash_run += 1;
            continue;
        }
        let escaped = slash_run % 2 == 1;
        slash_run = 0;
        if char == '{' && !escaped {
            depth += 1;
            if depth > i64::from(maximum) {
                return Err(latex_error(
                    format!("LaTeX nesting exceeds the {maximum}-level limit"),
                    index,
                ));
            }
        } else if char == '}' && !escaped {
            depth = (depth - 1).max(0);
        }
    }
    Ok(())
}
