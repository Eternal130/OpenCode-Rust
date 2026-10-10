; Vendored for oc-syntax (R-01b) — DO NOT EDIT BY HAND.
; Source: https://github.com/tree-sitter-grammars/tree-sitter-diff/blob/v0.1.0/queries/highlights.scm
; Tag: v0.1.0 — the tag's commit IS the exact commit v2 pins
; (parsers-config.ts points at 2520c3f934b3179bb540d23e0ef45f75304b5fed,
; which is v0.1.0). Aliases per the v2 catalog: udiff, patch. The
; diff.plus/diff.minus captures have no theme scopes (they fall back to
; base); the diff viewer paints add/remove semantics itself.

(comment) @comment @spell

[
  (addition)
  (new_file)
] @diff.plus

[
  (deletion)
  (old_file)
] @diff.minus

(commit) @constant

(location) @attribute

(command
  "diff" @function
  (argument) @variable.parameter)

(filename) @string.special.path

(mode) @number

([
  ".."
  "+"
  "++"
  "+++"
  "++++"
  "-"
  "--"
  "---"
  "----"
] @punctuation.special
  (#set! priority 95))

[
  (binary_change)
  (similarity)
  (file_change)
] @label

(index
  "index" @keyword)

(similarity
  (score) @number
  "%" @number)
