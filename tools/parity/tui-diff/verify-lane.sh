#!/usr/bin/env bash
# Lane verification tiering (lane/accel) — THE lane verification entrypoint
# referenced by docs/migration-m1-tabstrip.md ("accelerated protocol").
#
# Two modes:
#
#   verify-lane.sh iterate [script …]   — the inner loop (minutes):
#         * cargo fmt check + nextest for the AFFECTED crates only
#           (oc-tui, oc-tui-core, parity),
#         * the tui-diff rig in --quick mode (1 run × 80x24) on the
#           lane's scripts (arguments; default the representative trio
#           boot-home / type-edit-submit / long-reply-scroll), with the
#           ref-capture cache ON — replays beat bun boots.
#
#   verify-lane.sh final                 — the merge gate (the full
#         matrix, ~1-2 h): fmt --all, clippy --workspace --all-targets
#         -D warnings (byte-equivalent to CI), full-workspace nextest,
#         the serialized pty profile, the rig at BOTH sizes × --runs 2
#         with the cache DISABLED (final verdicts are live verdicts),
#         and the release build budget gate.
#
# Every cargo invocation goes through tools/dev-gate.sh (identity, CPU
# cap, memory cap). PARITY_BUN must point at a world-readable bun
# (default: /tmp/opencode/bun/bin/bun — see tui-diff/README.md).
#
# Exit status: 0 = all tiers green, non-zero = first failure wins.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # …/tools/parity/tui-diff
ROOT="$(cd "$HERE/../../.." && pwd)"                   # the checkout root
GATE="$ROOT/tools/dev-gate.sh"

export PARITY_BUN="${PARITY_BUN:-/tmp/opencode/bun/bin/bun}"
if [[ ! -x "$PARITY_BUN" ]]; then
  echo "verify-lane: PARITY_BUN not executable: $PARITY_BUN (see tui-diff/README.md)" >&2
  exit 2
fi

gate() {  # gate '<cargo command>'
  OPENCODE_DEV_GATE_DIR="$ROOT" "$GATE" "$1"
}

run_rig() {  # run_rig <outdir-tag> <flags…>
  local tag="$1"; shift
  gate "PARITY_BUN='$PARITY_BUN' cargo run -p parity --bin tui-diff -- --out /tmp/opencode/tui-diff/$tag $*"
}

mode="${1:-}"; shift || true
case "$mode" in
  iterate)
    scripts=("$@")
    if [[ ${#scripts[@]} -eq 0 ]]; then
      scripts=(boot-home type-edit-submit long-reply-scroll)
    fi
    echo "== [iterate] fmt check (affected surface) =="
    gate 'cargo fmt --all --check'
    echo "== [iterate] nextest: oc-tui + oc-tui-core + parity =="
    gate 'cargo nextest run -p oc-tui -p oc-tui-core -p parity'
    echo "== [iterate] rig --quick on: ${scripts[*]} (ref-cache ON) =="
    for script in "${scripts[@]}"; do
      run_rig "iterate-$$" --quick --filter "$script"
    done
    echo "== [iterate] GREEN — remember: FINAL still owes the full matrix =="
    ;;

  final)
    [[ $# -eq 0 ]] || { echo "verify-lane: final takes no arguments" >&2; exit 2; }
    echo "== [final] fmt --all --check =="
    gate 'cargo fmt --all --check'
    echo "== [final] clippy --workspace --all-targets -D warnings (CI-equivalent) =="
    gate 'cargo clippy --workspace --all-targets --locked -- -D warnings'
    echo "== [final] nextest: full workspace =="
    gate 'cargo nextest run --workspace'
    echo "== [final] nextest: serialized pty profile =="
    gate 'cargo nextest run --workspace --profile pty'
    echo "== [final] release budget gate (build + frame_budget) =="
    gate 'cargo build --release -p oc-cli --locked'
    gate 'cargo nextest run -p oc-tui --release -E "test(frame_budget)"'
    echo "== [final] rig FULL matrix: both sizes x --runs 2, cache DISABLED (live verdicts) =="
    run_rig "final-$$" --runs 2 --no-ref-cache
    echo "== [final] ALL GREEN =="
    ;;

  *)
    cat >&2 <<EOF
usage: verify-lane.sh iterate [script …]   # fast inner loop (oc-tui+oc-tui-core+parity, --quick rig)
       verify-lane.sh final                 # full merge gate (workspace + pty + full-matrix rig + budgets)
EOF
    exit 2
    ;;
esac
