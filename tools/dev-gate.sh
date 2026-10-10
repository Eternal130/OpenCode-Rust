#!/usr/bin/env bash
# Structural dev-discipline wrapper — EVERY local build/test command in this
# repo MUST run through this script (lane specs reference it; CI is exempt).
#
# Enforces the frozen rules structurally, not by memory:
#   1. IDENTITY: always executes as eternal130 (root running cargo/fmt dirtied
#      file ownership three times in project history — never again).
#   2. CPU CAP: transient systemd scope with CPUQuota=400% (4 of 16 cores).
#      A forgotten -j4, a parallel lane, or a runaway linker physically cannot
#      saturate the box (newapi + the live service share it).
#   3. MEMORY CAP: MemoryMax=10G (box has 22G; gates need well under 10).
#   4. LOAD BACKSTOP: refuses to start while 1-min load >= 6 (the per-lane
#      courtesy check remains "< 2 before gates"; this is the hard ceiling).
#
# Usage:   tools/dev-gate.sh '<command to run with cargo on PATH>'
# Example: tools/dev-gate.sh 'cargo nextest run --workspace --no-fail-fast'
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 '<command>'" >&2
  exit 2
fi

# --- load backstop ---------------------------------------------------------
LOAD1=$(cut -d' ' -f1 /proc/loadavg)
if awk -v l="$LOAD1" 'BEGIN{exit !(l>=6)}'; then
  echo "dev-gate: 1-min load ${LOAD1} >= 6 — refusing to start; retry when the box calms down" >&2
  exit 3
fi

# Already inside a gated scope? Run directly (no nested scopes).
if [[ -n "${OPENCODE_DEV_GATE:-}" ]]; then
  exec bash -c "$1"
fi

# Target checkout: /opt/opencode-rust by default; lanes may set
# OPENCODE_DEV_GATE_DIR to run against an isolated git worktree.
GATE_DIR="${OPENCODE_DEV_GATE_DIR:-/opt/opencode-rust}"

exec systemd-run --scope --quiet \
  --unit="opencode-gate-$(date +%s)-$$" \
  --uid=eternal130 --gid=eternal130 \
  -p CPUQuota=400% \
  -p MemoryMax=10G \
  -p TasksMax=512 \
  --setenv=OPENCODE_DEV_GATE=1 \
  --setenv=HOME=/home/eternal130 \
  --setenv=PATH="/home/eternal130/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
  bash -c "cd ${GATE_DIR} && $1"
