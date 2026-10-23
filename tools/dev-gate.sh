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
#   3. MEMORY CAP: MemoryMax=6G (box has 22G, shared with production
#      services; the 2026-10-19 oomd incident cut this from 10G).
#   4. LOAD BACKSTOP: refuses to start while 1-min load >= 6 (the per-lane
#      courtesy check remains "< 2 before gates"; this is the hard ceiling).
#   5. GLOBAL SERIALIZATION: exactly one gate scope on the machine at a
#      time (2026-10-19 incident: concurrent scopes exhausted memory and
#      systemd-oomd killed the user session + system services).
#
# Usage:   tools/dev-gate.sh '<command to run with cargo on PATH>'
# Example: tools/dev-gate.sh 'cargo nextest run --workspace --no-fail-fast'
set -euo pipefail

# --- global serialization (2026-10-19 incident) ---------------------------
LOCK_DIR=/tmp/opencode
LOCK="${LOCK_DIR}/dev-gate.lock"
mkdir -p "$LOCK_DIR"
exec 9>"$LOCK"
if ! flock -n 9; then
  echo "dev-gate: another gate scope is active; waiting on ${LOCK}" >&2
  flock 9
fi

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
# Footgun guard: with the env var unset, a lane-worktree invocation used
# to silently build the MAIN checkout (false greens). When the caller's
# $PWD is itself a workspace checkout (has a Cargo.toml) and is not the
# main checkout, target it instead — with one stderr notice. Otherwise
# keep the old default.
if [[ -n "${OPENCODE_DEV_GATE_DIR:-}" ]]; then
  GATE_DIR="${OPENCODE_DEV_GATE_DIR}"
elif [[ "${PWD}" != "/opt/opencode-rust" && -f "${PWD}/Cargo.toml" ]]; then
  GATE_DIR="${PWD}"
  echo "dev-gate: OPENCODE_DEV_GATE_DIR unset; using caller dir ${GATE_DIR}" >&2
else
  GATE_DIR="/opt/opencode-rust"
fi

exec systemd-run --scope --quiet \
  --unit="opencode-gate-$(date +%s)-$$" \
  --uid=eternal130 --gid=eternal130 \
  -p CPUQuota=400% \
  -p MemoryMax=6G \
  -p TasksMax=512 \
  --setenv=OPENCODE_DEV_GATE=1 \
  --setenv=HOME=/home/eternal130 \
  --setenv=PATH="/home/eternal130/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
  bash -c "cd ${GATE_DIR} && $1"
