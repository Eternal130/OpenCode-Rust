# Vendored `tree-sitter-kotlin` 0.3.8 (RA-07)

The Kotlin grammar pinned by v2's parser catalog
(`packages/tui/src/parsers-config.ts`: fwcd wasm 0.3.8), vendored as a
path crate: the registry's same-tag crates.io release (0.3.8) still
binds the old crate-style `tree_sitter` surface (`language()` fn)
instead of `tree-sitter-language` 0.1. Exposed only behind the
`lang-kotlin` cargo feature of `oc-syntax` (kotlin was one of the three
grammars dropped for release-binary size in R-01b; it stays off the
default feature set).

Contents:

- `src/parser.c`, `src/scanner.c`, `src/tree_sitter/parser.h` —
  byte-for-byte from
  https://github.com/fwcd/tree-sitter-kotlin/tree/0.3.8/src
- `lib.rs`, `build.rs`, `Cargo.toml` — standard tree-sitter-language
  0.1 bindings, following the `third_party/tree-sitter-vim` precedent.

License: MIT (upstream `LICENSE`, kept alongside).
