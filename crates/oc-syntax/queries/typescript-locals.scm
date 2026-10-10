; Vendored for oc-syntax (R-01 spike) — DO NOT EDIT BY HAND.
; Source: https://github.com/tree-sitter/tree-sitter-typescript/blob/v0.23.2/queries/locals.scm
; Tag: v0.23.2 (matches the tree-sitter-typescript 0.23.2 crate).
; Registered as the registry's LOCALS_QUERY for typescript (built by the
; query-construction test; not executed for styling — locals captures feed
; editor navigation, not colors).

(required_parameter (identifier) @local.definition)
(optional_parameter (identifier) @local.definition)
