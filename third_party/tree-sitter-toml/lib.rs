//! tree-sitter-toml — the TOML grammar vendored at tag `v0.7.0`
//! (tree-sitter-grammars/tree-sitter-toml, the grammar v2's parser
//! catalog pins), vendored as a path crate because no crates.io
//! release of this grammar lineage carries Rust bindings. The C
//! sources under `src/` are byte-for-byte the tag's.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_toml() -> *const ();
}

/// The TOML grammar entry point.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_toml) };
