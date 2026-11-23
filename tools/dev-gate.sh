#!/usr/bin/env bash
# Structural dev-discipline wrapper — EVERY local build/test command in this
# repo MUST run through this script (lane specs reference it; CI is exempt).
#
# Enforces the frozen rules structurally, not by memory:
#   1. CPU CAP: transient systemd scope with CPUQuota=400% (4 of 16 cores).
#      A forgotten -j4, a parallel lane, or a runaway linker physically cannot
#      saturate the box (newapi + the live service share it).
#   2. MEMORY CAP: MemoryMax=12G (box has 22G, shared with production
#      services; 6G per the 2026-10-19 oomd incident ruling, raised to 12G
#      2026-11-23 by owner ruling for the nextest-concurrency experiment —
#      execution-phase tests are wait-bound, memory was the tight ceiling).
#   3. LOAD BACKSTOP: refuses to start while 1-min load >= 8 (the per-lane
#      courtesy check remains "< 2 before gates"; this is the hard ceiling).
#   4. GLOBAL SERIALIZATION: exactly one gate scope on the machine at a
#      time (2026-10-19 incident: concurrent scopes exhausted memory and
#      systemd-oomd killed the user session + system services).
#
# (2026-11-22 user ruling: the former IDENTITY item — always execute as a
# configured non-root dev user — was retired along with the "no root cargo"
# discipline; gates now run as the invoking user under the same scope caps.)
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

# --- disk backstop (2026-11-17 incident: a lane's load repro filled the
# disk to 0 B; postgres crashlooped, new-api/gateway chain died) ------------
free_kb=$(df -Pk / | awk 'NR==2 {print $4}')
if [ "$free_kb" -lt 52428800 ]; then
  echo "dev-gate: disk free $((free_kb/1024/1024))G < 50G — refusing to start." >&2
  echo "free space first: du -sh .worktrees/*/target target; git worktree remove merged lanes; cargo clean per stale worktree (register 2.1#5)." >&2
  exit 5
fi

# --- harness discipline (register 2.2#1) ------------------------------------
# Canonical harness = nextest. Plain `cargo test` is forbidden as evidence or
# iteration harness; the ONLY sanctioned libtest form is `cargo test --exact
# <name>` single-test repro. Enforced structurally after a lane drifted back
# to libtest by habit (2026-11-11, gap-burndown-12 lane 14). CI is exempt.
deny_plain_cargo_test() {
  local segment
  # cargo as a word, boundary incl. inside quotes (\x27=\x27-quote, \x22=dquote)
  local cargo_word=$'(^|[[:space:]\x27\x22])cargo([[:space:]]|$)'
  # Split the command on shell separators; inspect each segment's shape.
  while IFS= read -r segment; do
    # Only segments that invoke cargo…
    [[ "$segment" =~ $cargo_word ]] || continue
    # …nextest invocations are the sanctioned harness.
    [[ "$segment" =~ [[:space:]]nextest([[:space:]]|$) ]] && continue
    # …a bare `test` subcommand (word-boundary; --tests flags do not match).
    if [[ "$segment" =~ [[:space:]]test([[:space:]]|$) ]]; then
      if [[ "$segment" =~ --exact ]]; then
        continue # allowed: --exact single-test repro (2.2#1 carve-out)
      fi
      echo "dev-gate: plain 'cargo test' is forbidden (register 2.2#1) — use 'cargo nextest run' (or 'cargo nextest run --no-run' to compile only)." >&2
      echo "dev-gate: offending segment: ${segment}" >&2
      exit 4
    fi
  done < <(printf '%s\n' "$1" | sed -e 's/&&/\n/g' -e 's/||/\n/g' -e 's/;/\n/g' -e 's/|/\n/g')
}
deny_plain_cargo_test "$1"

# --- load backstop ---------------------------------------------------------
LOAD1=$(cut -d' ' -f1 /proc/loadavg)
if awk -v l="$LOAD1" 'BEGIN{exit !(l>=8)}'; then
  echo "dev-gate: 1-min load ${LOAD1} >= 8 — refusing to start; retry when the box calms down" >&2
  exit 3
fi

# Already inside a gated scope? Run directly (no nested scopes).
if [[ -n "${OPENCODE_DEV_GATE:-}" ]]; then
  exec bash -c "$1"
fi

# Repo root = this script's checkout (works from any worktree invocation).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Target checkout: the main checkout by default; lanes may set
# OPENCODE_DEV_GATE_DIR to run against an isolated git worktree.
# Footgun guard: with the env var unset, a lane-worktree invocation used
# to silently build the MAIN checkout (false greens). When the caller's
# $PWD is itself a workspace checkout (has a Cargo.toml) and is not the
# main checkout, target it instead — with one stderr notice. Otherwise
# keep the old default.
if [[ -n "${OPENCODE_DEV_GATE_DIR:-}" ]]; then
  GATE_DIR="${OPENCODE_DEV_GATE_DIR}"
elif [[ "${PWD}" != "${REPO_ROOT}" && -f "${PWD}/Cargo.toml" ]]; then
  GATE_DIR="${PWD}"
  echo "dev-gate: OPENCODE_DEV_GATE_DIR unset; using caller dir ${GATE_DIR}" >&2
else
  GATE_DIR="${REPO_ROOT}"
fi

# --- evidence tee (register 2.1#3) ------------------------------------------
# Every gated run is logged IN FULL to the target checkout's .test-logs/
# (the log is the only test evidence — log lost = evidence lost). The tee
# captures the complete, untruncated stream; display filtering (grep/tail)
# belongs AFTER the run, on the file. Lane label via OC_GATE_LANE.
LOG_CRATES=$(printf '%s' "$1" | grep -oE -- '-p [A-Za-z0-9_-]+' | sed 's/^-p //' | paste -sd, - || true)
if printf '%s' "$1" | grep -q -- '--workspace'; then LOG_CRATES="workspace"; fi
[ -n "$LOG_CRATES" ] || LOG_CRATES="gate"
LOG_DIR="${GATE_DIR}/.test-logs"
mkdir -p "$LOG_DIR"
LOG="${LOG_DIR}/${LOG_CRATES}-$(date +%Y%m%d-%H%M%S)-${OC_GATE_LANE:-gate}.log"
echo "dev-gate: logging full output to ${LOG}" >&2

# FD-inheritance fix (2026-11-18 incident, va6 hardening): the gate's lock
# fd 9 was inherited by EVERY gated child (cargo/nextest/test binaries/TUI+
# ref children) — a single leaked test child kept the flock held forever
# after the gate exited, wedging all later gates ("another gate scope
# is active" while nothing legitimate ran). Closures at every layer:
#   * `9>&-` on systemd-run: if the gate SCRIPT is killed, nothing in the
#     scope chain still holds the lock — it releases immediately.
#   * `9>&-` on the gated bash: nothing below (cargo/nextest/tests/TUI/
#     daemons) ever sees the lock fd.
#   * `9>&-` on tee: the evidence tee is also a lock-fd holder otherwise.
# The gate script itself (this shell) keeps owning the flock for the run's
# duration; a killed gate therefore frees the lock at once.
systemd-run --scope --quiet 9>&- \
  --unit="opencode-gate-$(date +%s)-$$" \
  -p CPUQuota=400% \
  -p MemoryMax=12G \
  -p TasksMax=512 \
  --setenv=OPENCODE_DEV_GATE=1 \
  bash 9>&- -c "cd ${GATE_DIR} && $1" 2>&1 | tee "$LOG" 9>&-
ps=("${PIPESTATUS[@]}")
run_rc=${ps[0]}; tee_rc=${ps[1]}
if [ "$tee_rc" -ne 0 ]; then
  echo "dev-gate: tee FAILED (rc=$tee_rc) — evidence-loss risk for ${LOG}" >&2
  run_rc=1
fi
echo "dev-gate: full log at ${LOG}" >&2
exit "$run_rc"
