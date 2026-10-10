# Vendored `tree-sitter-toml` v0.7.0 (R-01b)

The TOML grammar pinned by v2's parser catalog
(`packages/tui/src/parsers-config.ts`: tree-sitter-grammars wasm
v0.7.0), vendored as a path crate because the grammar lineage was never
published to crates.io with Rust bindings (the registry's lone
`tree-sitter-toml` 0.20.0 is a 2022-era release of a different line
that still binds tree-sitter 0.20).

Contents:

- `src/parser.c`, `src/scanner.c`, `src/tree_sitter/parser.h` —
  byte-for-byte from
  https://github.com/tree-sitter-grammars/tree-sitter-toml/tree/v0.7.0/src
- `lib.rs`, `build.rs`, `Cargo.toml` — standard tree-sitter-language
  0.1 bindings (`pub const LANGUAGE: LanguageFn`).

License: MIT (upstream `LICENSE`, kept alongside).
