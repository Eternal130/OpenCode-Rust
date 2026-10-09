//! Port of `formatter.ts` — the post-write formatting hook point.
//!
//! v2 shells out to configured formatter commands (prettier et al.) matched
//! by extension. The Rust port pins the trait + registry surface and ships a
//! no-op implementation: tools call [`Formatter::file`] after every write,
//! and the real command execution plugs into the same seam later
//! (`TODO(formatter-lane)`).

//! Lint posture: ports of TS sources keep the mechanical-idiom allows the
//! runner established (numeric casts, exhaustive tables, ownership queries).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity,
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening,
    clippy::large_futures,
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::min_ident_chars,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl,
    clippy::unused_self,
    clippy::doc_lazy_continuation
)]

use std::collections::BTreeMap;
use std::path::Path;

/// One formatter registration (`formatter/builtins.ts Info`, trimmed to the
/// fields the runner consumes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatterInfo {
    pub name: String,
    /// File extensions (with the leading dot) this formatter handles.
    pub extensions: Vec<String>,
    /// The command template; `$FILE` substitutes the target path.
    pub command: Vec<String>,
}

/// The formatter service trait (`Formatter.Interface.file`): returns whether
/// a formatter ran successfully for the file.
pub trait Formatter: Send + Sync {
    /// Register or replace one formatter (`Editor.set`).
    fn set(&self, formatter: FormatterInfo);
    /// Remove one formatter by name (`Editor.remove`).
    fn remove(&self, name: &str);
    /// Run the matching formatter for `filepath`, if any. `true` when a
    /// formatter applied successfully.
    fn file(&self, filepath: &Path) -> bool;
}

/// The no-op implementation: formatters are recorded but never executed —
/// the observable behavior of v2 with no formatter commands available.
#[derive(Debug, Default)]
pub struct NoopFormatter {
    formatters: std::sync::Mutex<BTreeMap<String, FormatterInfo>>,
}

impl NoopFormatter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The registered formatter names (diagnostics/tests).
    pub fn names(&self) -> Vec<String> {
        self.formatters.lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect()
    }
}

impl Formatter for NoopFormatter {
    fn set(&self, formatter: FormatterInfo) {
        self.formatters
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(formatter.name.clone(), formatter);
    }

    fn remove(&self, name: &str) {
        self.formatters.lock().unwrap_or_else(|p| p.into_inner()).remove(name);
    }

    fn file(&self, _filepath: &Path) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatter_noop_passes_files_through() {
        let formatter = NoopFormatter::new();
        assert!(!formatter.file(Path::new("/tmp/opencode/x.rs")));
        assert!(formatter.names().is_empty());
        formatter.set(FormatterInfo {
            name: "test".into(),
            extensions: vec![".rs".into()],
            command: vec!["rustfmt".into(), "$FILE".into()],
        });
        assert_eq!(formatter.names(), vec!["test".to_string()]);
        // The no-op registry never executes commands.
        assert!(!formatter.file(Path::new("/tmp/opencode/x.rs")));
        formatter.remove("test");
        assert!(formatter.names().is_empty());
    }
}
