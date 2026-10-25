# Vendored `tree-sitter-vue` v0.1.2 (RA-07)

The Vue grammar pinned by v2's parser catalog
(`packages/tui/src/parsers-config.ts`: anomalyco fork wasm v0.1.2,
composed query `html_tags/highlights.scm` + `vue/highlights.scm`),
vendored as a path crate: the anomalyco fork never published to
crates.io (the registry's `tree-sitter-vue` 0.0.x releases are an
unrelated stale lineage).

Contents:

- `src/parser.c`, `src/scanner.c`, `src/tag.h` (included by the
  scanner), `src/tree_sitter/parser.h` — byte-for-byte from
  https://github.com/anomalyco/tree-sitter-vue/tree/v0.1.2/src
- `lib.rs`, `build.rs`, `Cargo.toml` — standard tree-sitter-language
  0.1 bindings, following the `third_party/tree-sitter-vim` precedent.

License: MIT (upstream `LICENSE`, kept alongside).
