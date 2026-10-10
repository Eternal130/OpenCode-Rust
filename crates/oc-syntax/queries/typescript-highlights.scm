; Vendored for oc-syntax (R-01 spike) — DO NOT EDIT BY HAND.
; Source: https://github.com/tree-sitter/tree-sitter-typescript/blob/v0.23.2/queries/highlights.scm
; Tag: v0.23.2 (matches the tree-sitter-typescript 0.23.2 crate pinned in
; Cargo.toml). Composed AFTER javascript-highlights.scm into the typescript
; highlight query at compile time (see that file's header).

; Types

(type_identifier) @type
(predefined_type) @type.builtin

((identifier) @type
 (#match? @type "^[A-Z]"))

(type_arguments
  "<" @punctuation.bracket
  ">" @punctuation.bracket)

; Variables

(required_parameter (identifier) @variable.parameter)
(optional_parameter (identifier) @variable.parameter)

; Keywords

[ "abstract"
  "declare"
  "enum"
  "export"
  "implements"
  "interface"
  "keyof"
  "namespace"
  "private"
  "protected"
  "public"
  "type"
  "readonly"
  "override"
  "satisfies"
] @keyword
