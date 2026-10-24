# Vendored `tree-sitter-vim` v0.8.1 (G-08)

The Vimscript grammar pinned by v2's parser catalog
(`packages/tui/src/parsers-config.ts`: tree-sitter-grammars wasm
v0.8.1), vendored as a path crate: the registry's lone crates.io
release (`tree-sitter-vim` 0.4.0, Aug 2024) predates the pin and still
depends on the crate-style `tree-sitter` binding surface instead of
`tree-sitter-language`.

Contents:

- `src/parser.c`, `src/scanner.c`, `src/keywords.h` (included by the
  scanner), `src/tree_sitter/parser.h` — byte-for-byte from
  https://github.com/tree-sitter-grammars/tree-sitter-vim/tree/v0.8.1/src
  (generated parser ABI 15, compatible with the workspace's
  tree-sitter 0.25 runtime).
- `lib.rs`, `build.rs`, `Cargo.toml` — standard tree-sitter-language
  0.1 bindings (`pub const LANGUAGE: LanguageFn`), following the
  `third_party/tree-sitter-toml` precedent.

License: MIT (upstream `LICENSE`, kept alongside).
