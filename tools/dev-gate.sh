#!/usr/bin/env bash
# Structural dev-discipline wrapper — EVERY local build/test command in this
# repo MUST run through this script (lane specs reference it; CI is exempt).
#
# Enforces the frozen rules structurally, not by memory:
#   1. IDENTITY: always executes as the configured non-root dev user
#      (OC_DEV_USER, else the uid-1000 account; root running cargo/fmt
#      dirtied file ownership three times in project history — never again).
#   2. CPU CAP: transient systemd scope with CPUQuota=400% (4 of 16 cores).
#      A forgotten -j4, a parallel lane, or a runaway linker physically cannot
#      saturate the box (newapi + the live service share it).
#   3. MEMORY CAP: MemoryMax=6G (box has 22G, shared with production
#      services; the 2026-10-19 oomd incident cut this from 10G).
#   4. LOAD BACKSTOP: refuses to start while 1-min load >= 8 (the per-lane
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

# --- disk backstop (2026-11-17 incident: a lane's load repro filled the
# disk to 0 B; postgres crashlooped, new-api/gateway chain died) ------------
free_kb=$(df -Pk / | awk 'NR==2 {print $4}')
if [ "$free_kb" -lt 52428800 ]; then
  echo "dev-gate: disk free $((free_kb/1024/1024))G < 50G — refusing to start." >&2
  echo "free space first: du -sh .worktrees/*/target target; git worktree remove merged lanes; cargo clean per stale worktree (register 2.1#6)." >&2
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

# Dev identity: OC_DEV_USER overrides; otherwise the uid-1000 account.
# All paths (HOME, group, cargo bin) are derived from that account — no
# hardcoded local usernames.
DEV_USER="${OC_DEV_USER:-$(getent passwd 1000 | cut -d: -f1)}"
if [[ -z "${DEV_USER}" ]]; then
  echo "dev-gate: cannot resolve the non-root dev user (no uid-1000 account; set OC_DEV_USER)" >&2
  exit 6
fi
DEV_HOME="$(getent passwd "${DEV_USER}" | cut -d: -f6)"
if [[ -z "${DEV_HOME}" ]]; then
  echo "dev-gate: cannot resolve HOME for user ${DEV_USER}" >&2
  exit 6
fi
DEV_GID="$(id -g "${DEV_USER}")"
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

# FD-inheritance fix (2026-11-18 incident): the gate's lock fd 9 was
# inherited by EVERY gated child (cargo/nextest/test binaries/TUI+ref
# children) — a single leaked test child kept the flock held forever
# after the gate exited, wedging all later gates ("another gate scope
# is active" while nothing legitimate ran). `9>&-` closes the lock fd
# at the gated bash boundary: the gate (this script, then systemd-run)
# keeps owning the flock for the run's duration, and nothing below
# inherits it.
exec systemd-run --scope --quiet \
  --unit="opencode-gate-$(date +%s)-$$" \
  --uid="${DEV_USER}" --gid="${DEV_GID}" \
  -p CPUQuota=400% \
  -p MemoryMax=6G \
  -p TasksMax=512 \
  --setenv=OPENCODE_DEV_GATE=1 \
  --setenv=HOME="${DEV_HOME}" \
  --setenv=PATH="${DEV_HOME}/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
  bash 9>&- -c "cd ${GATE_DIR} && $1"
