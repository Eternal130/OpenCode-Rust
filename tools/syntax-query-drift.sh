#!/usr/bin/env bash
#
# syntax-query-drift — RB-06 audit: vendored .scm vs upstream pinned query
#
# For every language whose highlight/locals query is vendored under
# crates/oc-syntax/queries/ (see crates/oc-syntax/src/queries.rs for the pin
# rationale), fetch the upstream query file at the EXACT pinned tag and diff
# it against the vendored copy. Prints a per-language PASS/DRIFT/SKIP summary;
# exits nonzero iff at least one language DRIFTs (SKIP does not fail — it
# means the upstream source is not fetchable as a pinned URL, e.g. the clojure
# query that ships inside the crates.io crate's grammar-src submodule, or a
# deliberately unpinned moving target like nvim-treesitter master for hcl).
#
# Usage:   tools/syntax-query-drift.sh [-v]   (from the repo root or anywhere)
#          -v  also print unified diffs for drifting files
# Network: required (curl to raw.githubusercontent.com). This is an
#          offline-optional audit tool — NOT wired into the default PR CI;
#          it runs weekly via .github/workflows/runtime-gates.yml
#          (job syntax-query-drift, network available on the GH runner).
#
# Pin sources: crates/oc-syntax/Cargo.toml crate versions for crates.io
# languages; third_party/*/README.md (vendored path crates: toml, vim, vue,
# kotlin) carry the same tags as the vendored C sources.

set -uo pipefail

verbose=0
[ "${1:-}" = "-v" ] && verbose=1

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
qdir="$here/../crates/oc-syntax/queries"

raw=https://raw.githubusercontent.com

# tag variants helper: emit both vX.Y.Z and X.Y.Z spellings
tags() { printf '%s\nv%s\n' "$1" "${1#v}"; }

# entry format (| separated):
#   name | vendored-file | owner/repo | version | repo-query-path [| alt-path ...]
# For each entry every (tag-spelling × path) candidate is tried until one
# returns HTTP 200 — handles repos that tag without the leading v, and repos
# whose query moved between subdirectories across tags.
ENTRIES=(
  "rust|rust-highlights.scm|tree-sitter/tree-sitter-rust|0.24.2|queries/highlights.scm"
  "python|python-highlights.scm|tree-sitter/tree-sitter-python|0.25.0|queries/highlights.scm"
  "typescript|typescript-highlights.scm|tree-sitter/tree-sitter-typescript|0.23.2|queries/highlights.scm"
  "typescript-locals|typescript-locals.scm|tree-sitter/tree-sitter-typescript|0.23.2|queries/locals.scm"
  "javascript|javascript-highlights.scm|tree-sitter/tree-sitter-javascript|0.23.1|queries/highlights.scm"
  "go|go-highlights.scm|tree-sitter/tree-sitter-go|0.25.0|queries/highlights.scm"
  "c|c-highlights.scm|tree-sitter/tree-sitter-c|0.24.2|queries/highlights.scm"
  "cpp|cpp-highlights.scm|tree-sitter/tree-sitter-cpp|0.23.4|queries/highlights.scm"
  "java|java-highlights.scm|tree-sitter/tree-sitter-java|0.23.5|queries/highlights.scm"
  "json|json-highlights.scm|tree-sitter/tree-sitter-json|0.24.8|queries/highlights.scm"
  "yaml|yaml-highlights.scm|tree-sitter-grammars/tree-sitter-yaml|0.7.2|queries/highlights.scm"
  "toml|toml-highlights.scm|tree-sitter-grammars/tree-sitter-toml|0.7.0|queries/highlights.scm"
  "html|html-highlights.scm|tree-sitter/tree-sitter-html|0.23.2|queries/highlights.scm"
  "css|css-highlights.scm|tree-sitter/tree-sitter-css|0.25.0|queries/highlights.scm"
  "diff|diff-highlights.scm|tree-sitter-grammars/tree-sitter-diff|0.1.0|queries/highlights.scm"
  "ruby|ruby-highlights.scm|tree-sitter/tree-sitter-ruby|0.23.1|queries/highlights.scm"
  "ruby-locals|ruby-locals.scm|tree-sitter/tree-sitter-ruby|0.23.1|queries/locals.scm"
  "php|php-highlights.scm|tree-sitter/tree-sitter-php|0.25.1|queries/highlights.scm"
  "lua|lua-highlights.scm|tree-sitter-grammars/tree-sitter-lua|0.5.0|queries/highlights.scm"
  "lua-locals|lua-locals.scm|tree-sitter-grammars/tree-sitter-lua|0.5.0|queries/locals.scm"
  "scala|scala-highlights.scm|tree-sitter/tree-sitter-scala|0.24.0|queries/highlights.scm"
  "make|make-highlights.scm|tree-sitter-grammars/tree-sitter-make|1.1.1|queries/highlights.scm"
  # vendored path-crate grammars (third_party/), same tags as the C sources
  "vim|vim-highlights.scm|tree-sitter-grammars/tree-sitter-vim|0.8.1|queries/vim/highlights.scm|queries/highlights.scm"
  "kotlin|kotlin-highlights.scm|fwcd/tree-sitter-kotlin|0.3.8|queries/highlights.scm"
  "bash|bash-highlights.scm|tree-sitter/tree-sitter-bash|0.25.0|queries/highlights.scm"
  "julia|julia-highlights.scm|tree-sitter/tree-sitter-julia|0.23.1|queries/highlights.scm"
  "elixir|elixir-highlights.scm|elixir-lang/tree-sitter-elixir|0.3.5|queries/highlights.scm"
  "xml|xml-highlights.scm|tree-sitter-grammars/tree-sitter-xml|0.7.0|queries/xml/highlights.scm|queries/highlights.scm"
  "agda|agda-highlights.scm|tree-sitter/tree-sitter-agda|1.3.3|queries/highlights.scm"
  "r|r-highlights.scm|r-lib/tree-sitter-r|1.2.0|queries/highlights.scm"
  "r-locals|r-locals.scm|r-lib/tree-sitter-r|1.2.0|queries/locals.scm"
  "haskell|haskell-highlights.scm|tree-sitter/tree-sitter-haskell|0.23.1|queries/highlights.scm"
  "ocaml|ocaml-highlights.scm|tree-sitter/tree-sitter-ocaml|0.24.2|queries/ocaml/highlights.scm|queries/highlights.scm"
  # fsharp: published from the ionide fork (no tree-sitter org repo)
  "fsharp|fsharp-highlights.scm|ionide/tree-sitter-fsharp|0.3.0|queries/highlights.scm"
  "clojure|clojure-highlights.scm|sogaiu/tree-sitter-clojure|0.1.0|grammar-src/queries/highlights.scm|queries/highlights.scm"
  "nix|nix-highlights.scm|nix-community/tree-sitter-nix|0.3.0|queries/highlights.scm"
  # the anomalyco vue fork (vendored grammar; may not exist as a fetchable tag)
  "vue|vue-highlights.scm|anomalyco/tree-sitter-vue|0.1.2|queries/vue/highlights.scm|queries/highlights.scm"
  "vue-html-tags|vue-html-tags-highlights.scm|anomalyco/tree-sitter-vue|0.1.2|queries/html_tags/highlights.scm"
  "swift|swift-highlights.scm|alex-pinkus/tree-sitter-swift|0.7.1|queries/highlights.scm"
  "csharp|csharp-highlights.scm|tree-sitter/tree-sitter-c-sharp|0.23.1|queries/highlights.scm"
)

# Deliberately NOT checked (always SKIP, recorded for the summary):
#   hcl        — vendored from nvim-treesitter master (queries.rs pin
#                rationale: hcl v1.1.0 ships no queries; the ref itself
#                routes nvim master). Moving target, no pinned URL by design.
#   markdown / markdown-inline — the vendored sources ARE the
#                nvim-treesitter queries (header says so; the ref routes
#                opentui's built-in with injections, not vendored here), not
#                a grammar-repo tag; no pinned URL by design.

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

pass=0; drift=0; skip=0
declare -a summary

fetch() { # fetch url out — 0 on HTTP 200
  curl -fsSL --max-time 30 --retry 1 -o "$2" "$1" 2>/dev/null
}

# strip_header file-out file-in — copy file-in to file-out, dropping the
# leading provenance header the vendoring lane stamped on most files (the
# ";; Vendored for oc-syntax … DO NOT EDIT" block through its blank line).
# Files without that stamp are copied verbatim.
strip_header() {
  awk 'NR==1 && /Vendored for oc-syntax/ {skip=1}
       skip && /^$/{skip=0; next}
       skip{next}
       {print}' "$2" >"$1"
}

for entry in "${ENTRIES[@]}"; do
  IFS='|' read -r name file repo ver paths <<<"$entry"
  local_file="$qdir/$file"
  if [ ! -f "$local_file" ]; then
    summary+=("SKIP|$name|vendored file missing: crates/oc-syntax/queries/$file")
    skip=$((skip+1)); continue
  fi
  IFS='|' read -ra path_list <<<"$paths"
  upstream=""; used_url=""
  for tag in $(tags "$ver"); do
    for p in "${path_list[@]}"; do
      url="$raw/$repo/$tag/$p"
      if fetch "$url" "$tmp/up.scm"; then upstream="$tmp/up.scm"; used_url="$url"; break 2; fi
    done
  done
  if [ -z "$upstream" ]; then
    summary+=("SKIP|$name|upstream not fetchable at pin ($repo @$ver — no candidate tag/path returned 200)")
    skip=$((skip+1)); continue
  fi
  strip_header "$tmp/vendored.scm" "$local_file"
  if diff -u "$tmp/vendored.scm" "$upstream" >"$tmp/d.txt"; then
    summary+=("PASS|$name|identical to $used_url")
    pass=$((pass+1))
  else
    summary+=("DRIFT|$name|differs from $used_url")
    drift=$((drift+1))
    if [ "$verbose" = 1 ]; then cat "$tmp/d.txt"; fi
  fi
done

# hcl / markdown / markdown-inline: unconditional SKIP (see note above)
summary+=("SKIP|hcl|nvim-treesitter master by design (hcl crate ships no queries; ref routes nvim master) — no pinned URL")
summary+=("SKIP|markdown|vendored source is the nvim-treesitter query (unpinned master), not a grammar-repo tag")
summary+=("SKIP|markdown-inline|vendored source is the nvim-treesitter query (unpinned master), not a grammar-repo tag")
skip=$((skip+3))

echo
echo "=== syntax-query-drift summary ==="
for row in "${summary[@]}"; do
  IFS='|' read -r st nm info <<<"$row"
  case "$st" in
    PASS)  printf 'PASS   %-18s %s\n'  "$nm" "$info" ;;
    DRIFT) printf 'DRIFT  %-18s %s\n'  "$nm" "$info" ;;
    SKIP)  printf 'SKIP   %-18s %s\n'  "$nm" "$info" ;;
  esac
done
echo "--------------------------------"
echo "PASS=$pass  DRIFT=$drift  SKIP=$skip  (SKIP = upstream not pinned/fetchable, not a failure)"

if [ "$drift" -gt 0 ]; then
  echo "RESULT: DRIFT ($drift file(s) differ from their pinned upstream)"
  exit 1
fi
echo "RESULT: PASS (no drift)"
