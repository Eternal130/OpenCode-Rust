//! tree-sitter-vim — the Vimscript grammar vendored at tag `v0.8.1`
//! (tree-sitter-grammars/tree-sitter-vim, the grammar v2's parser
//! catalog pins at parsers-config.ts:353-363), vendored as a path crate
//! because the crates.io release of this lineage (0.4.0) predates the
//! pin by four minor versions and still binds old-style tree-sitter.
//! The C sources under `src/` are byte-for-byte the tag's.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_vim() -> *const ();
}

/// The Vimscript grammar entry point.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_vim) };
