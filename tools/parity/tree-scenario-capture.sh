#!/usr/bin/env bash
# Lane TB — regenerate the oc-tui tree-scenario captures (the JSON
# under tools/parity/tree-scenarios/).
#
# DOUBLE GATE (the PARITY_UPDATE stop-and-escalate discipline):
#   1. this script must be invoked EXPLICITLY, and
#   2. OC_TUI_TREE_CAPTURE_UPDATE=1 must be exported (the rewrite
#      test itself refuses to run without it).
#
# A scenario edit WITHOUT re-freezing the oracle fails the standing
# gate (the frozen oracle's scenario fingerprint no longer matches) —
# after regenerating scenarios ALWAYS run tools/parity/tree-oracle-freeze.sh.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${REPO_ROOT}"

export OC_TUI_TREE_CAPTURE_UPDATE=1
/opt/opencode-rust/tools/dev-gate.sh \
  'cargo nextest run -p oc-tui --locked --run-ignored only scenario_capture_rewrite'
echo "scenarios regenerated — re-freeze oracles next: TREE_ORACLE_FREEZE=1 tools/parity/tree-oracle-freeze.sh"
