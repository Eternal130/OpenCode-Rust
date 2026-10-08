//! Shared support/model inputs for variant resolution (v2 `Variant.Support`
//! and the model slice `Variant.resolve` consults).

use crate::provider_catalog::source::SourceReasoningOption;

/// Reasoning support declaration (v2 `Variant.Support`):
/// `effort` (optional value list — `None` means the protocol's defaults),
/// `toggle`, or `budget_tokens` with optional min/max.
#[derive(Debug, Clone, PartialEq)]
pub enum Support {
    Effort { values: Option<Vec<String>> },
    Toggle,
    BudgetTokens { min: Option<i64>, max: Option<i64> },
}

/// The slice of `Model.Info` the variant protocols consult (v2 passes the
/// whole `Model.Info`; only these fields are read).
#[derive(Debug, Clone)]
pub struct VariantModel {
    pub package: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub limit_output: i64,
}

/// v2 `supports(model)`: source `reasoning_options` → [`Support`] list.
/// Effort values filter out `null` entries and the literal `"null"`.
#[must_use]
pub fn supports_from_reasoning_options(options: &[SourceReasoningOption]) -> Vec<Support> {
    options
        .iter()
        .map(|option| match option {
            SourceReasoningOption::Effort { values } => Support::Effort {
                values: values.as_ref().map(|list| {
                    list.iter()
                        .filter_map(|value| {
                            let value = value.as_deref()?;
                            (value != "null").then(|| value.to_string())
                        })
                        .collect()
                }),
            },
            SourceReasoningOption::Toggle => Support::Toggle,
            SourceReasoningOption::BudgetTokens { min, max } => {
                Support::BudgetTokens { min: min.map(f64_to_i64), max: max.map(f64_to_i64) }
            }
        })
        .collect()
}

/// JS-number → token count (truncating, non-negative clamped).
// JS `Number` parity: the bounds dance mirrors TS's implicit conversions.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "JS Number → token-count conversions"
)]
#[must_use]
pub fn f64_to_i64(value: f64) -> i64 {
    if !value.is_finite() {
        return 0;
    }
    let truncated = value.trunc();
    if truncated >= i64::MAX as f64 {
        i64::MAX
    } else if truncated <= i64::MIN as f64 {
        i64::MIN
    } else {
        truncated as i64
    }
}
