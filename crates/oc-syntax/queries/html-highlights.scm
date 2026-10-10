; Vendored for oc-syntax (R-01b) — DO NOT EDIT BY HAND.
; Source: https://github.com/tree-sitter/tree-sitter-html/blob/v0.23.2/queries/highlights.scm
; Tag: v0.23.2 (matches the tree-sitter-html 0.23.2 crate pinned in Cargo.toml).
; v2 routing note: parsers-config.ts:149-151 routes html highlights from the
; parser repo (the nvim-treesitter query is marked broken there) — exactly
; this file's routing, at the pinned tag. Injections (script/style embedded
; languages) are a v2 TODO too; not wired.

(tag_name) @tag
(erroneous_end_tag_name) @tag.error
(doctype) @constant
(attribute_name) @attribute
(attribute_value) @string
(comment) @comment

[
  "<"
  ">"
  "</"
  "/>"
] @punctuation.bracket
