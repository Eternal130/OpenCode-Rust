//! tree-sitter-kotlin — the Kotlin grammar vendored at tag `0.3.8`
//! (fwcd/tree-sitter-kotlin, the grammar v2's parser catalog pins).
//! Bindings regenerated for `tree-sitter-language` 0.1; the C sources
//! under `src/` are byte-for-byte the tag's.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_kotlin() -> *const ();
}

/// The Kotlin grammar entry point.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_kotlin) };
