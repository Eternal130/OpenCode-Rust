; Vendored for oc-syntax (R-01b) — DO NOT EDIT BY HAND.
; Source: https://github.com/tree-sitter-grammars/tree-sitter-toml/blob/v0.7.0/queries/highlights.scm
; Tag: v0.7.0 (matches the vendored third_party/tree-sitter-toml crate,
; which carries the grammar at exactly this tag). v2 routing note:
; parsers-config.ts routes toml highlights from nvim-treesitter master;
; this vendors the parser repo's own query at the exact tag instead
; (grammar/query ABI coherence). Locals: none shipped; entry carries None.

; Properties
;-----------

(bare_key) @type

(quoted_key) @string

(pair
  (bare_key)) @property

(pair
  (dotted_key
    (bare_key) @property))

; Literals
;---------

(boolean) @boolean

(comment) @comment

(string) @string

[
  (integer)
  (float)
] @number

[
  (offset_date_time)
  (local_date_time)
  (local_date)
  (local_time)
] @string.special

; Punctuation
;------------

[
  "."
  ","
] @punctuation.delimiter

"=" @operator

[
  "["
  "]"
  "[["
  "]]"
  "{"
  "}"
] @punctuation.bracket
