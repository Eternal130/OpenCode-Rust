#!/usr/bin/env bash
# Emit the canonical development standards (sections 2.1-2.5) VERBATIM from the registry.
#
# LANE DISPATCH RULE (user ruling 2026-11-17): every lane prompt MUST embed
# this script's output verbatim. Hand-compressed/rewritten summaries have
# repeatedly dropped rules and caused incidents (disk exhaustion -> production
# new-api outage, root-run cargo/merge ownership, false "clippy green" claims,
# cross-repo pushes bypassing the merge gate). Never retype the discipline —
# extract it:
#
#   DISCIPLINE="$(tools/lane-brief.sh)"
#   ... include "$DISCIPLINE" in the subagent prompt ...
#
# Usage:
#   tools/lane-brief.sh            # 开发规范 sections 2.1-2.5, verbatim
#   tools/lane-brief.sh --all      # + 现行制度速查 (quick-reference) table
set -euo pipefail
cd "$(dirname "$0")/.."

emit_all=0
[ "${1:-}" = "--all" ] && emit_all=1

awk -v all="$emit_all" '
  /^## 开发规范$/     { in_dev = 1 }
  /^## 现行制度速查/  { if (all) { in_dev = 1; in_table = 1 } else { exit } }
  in_table && /^---$/ { exit }
  in_dev              { print }
' docs/GAPS-AND-PITFALLS.md
