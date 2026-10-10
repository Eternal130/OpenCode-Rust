; Vendored for oc-syntax (R-01b) — DO NOT EDIT BY HAND.
; Source: https://github.com/tree-sitter/tree-sitter-cpp/blob/v0.23.4/queries/highlights.scm
; Tag: v0.23.4 (matches the tree-sitter-cpp 0.23.4 crate pinned in Cargo.toml).
; The parser repo's query is self-contained (no `; inherits:` — that is the
; nvim-treesitter variant); C-family base patterns are inlined upstream.
; Locals: no locals.scm at this tag; the entry carries None.

; Functions

(call_expression
  function: (qualified_identifier
    name: (identifier) @function))

(template_function
  name: (identifier) @function)

(template_method
  name: (field_identifier) @function)

(template_function
  name: (identifier) @function)

(function_declarator
  declarator: (qualified_identifier
    name: (identifier) @function))

(function_declarator
  declarator: (field_identifier) @function)

; Types

((namespace_identifier) @type
 (#match? @type "^[A-Z]"))

(auto) @type

; Constants

(this) @variable.builtin
(null "nullptr" @constant)

; Keywords

[
 "catch"
 "class"
 "co_await"
 "co_return"
 "co_yield"
 "constexpr"
 "constinit"
 "consteval"
 "delete"
 "explicit"
 "final"
 "friend"
 "mutable"
 "namespace"
 "noexcept"
 "new"
 "override"
 "private"
 "protected"
 "public"
 "template"
 "throw"
 "try"
 "typename"
 "using"
 "concept"
 "requires"
 "virtual"
] @keyword

; Strings

(raw_string_literal) @string
