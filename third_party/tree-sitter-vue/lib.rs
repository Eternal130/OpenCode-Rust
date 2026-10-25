//! tree-sitter-vue — the Vue single-file-component grammar vendored at
//! tag `v0.1.2` of the anomalyco fork (the lineage v2's parser catalog
//! pins at parsers-config.ts:170-178; the fork never published a
//! crates.io release). The C sources under `src/` are byte-for-byte
//! the tag's.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_vue() -> *const ();
}

/// The Vue grammar entry point.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_vue) };
