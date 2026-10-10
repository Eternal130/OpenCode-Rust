# Vendored `tree-sitter-kotlin` 0.3.8 (R-01b)

The Kotlin grammar pinned by v2's parser catalog
(`packages/tui/src/parsers-config.ts`: fwcd wasm 0.3.8 + fwcd highlights
query at the same tag), vendored as a path crate because the crates.io
`tree-sitter-kotlin` 0.3.8 release predates `tree-sitter-language` (its
bindings target the incompatible tree-sitter 0.22 line) and fwcd has
published nothing newer.

Contents:

- `src/parser.c`, `src/scanner.c`, `src/tree_sitter/*.h` — byte-for-byte
  from https://github.com/fwcd/tree-sitter-kotlin/tree/0.3.8/src
- `lib.rs`, `build.rs`, `Cargo.toml` — regenerated bindings in the
  standard tree-sitter-language 0.1 shape (same as every crates.io
  grammar the workspace pulls: `pub const LANGUAGE: LanguageFn`).

License: MIT (upstream `LICENSE`, kept alongside).
