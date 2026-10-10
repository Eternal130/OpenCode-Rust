; Vendored for oc-syntax (R-01b) — DO NOT EDIT BY HAND.
; Source: https://github.com/tree-sitter/tree-sitter-json/blob/v0.24.8/queries/highlights.scm
; Tag: v0.24.8 (matches the tree-sitter-json 0.24.8 crate pinned in Cargo.toml).
; Locals: json has no locals query upstream; the entry carries None.

(pair
  key: (_) @string.special.key)

(string) @string

(number) @number

[
  (null)
  (true)
  (false)
] @constant.builtin

(escape_sequence) @escape

(comment) @comment
