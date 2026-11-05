#!/usr/bin/env bash
# Lane TB — freeze the TS-oracle frames for the oc-tui tree scenarios.
#
# THE ORACLE SIDE OF THE DUAL-ENGINE TREE GATE. Sends every committed
# scenario under tools/parity/tree-scenarios/ through opentui-rust's
# own differential harness (tools/diff @ the pinned rev 2fbe00e, the
# same rev the workspace locks): the REAL @opentui 0.5.14 (npm
# tarballs under the harness sandbox) driven by bun through
# js/driver.ts, then the port replay + cell-for-cell compare. The
# oracle capture each run produces (out/oracle/<name>.json, carrying
# the harness's capture fingerprint) is FROZEN into
# tools/parity/tree-scenarios/oracle/ (committed) — that frozen frame
# is what the standing nextest gate
# (oc-tui::tree_scenarios::dual_engine_matches_frozen_oracle) compares
# against, so nextest needs no bun.
#
# Freeze point rationale (README mechanism): tools/diff caches oracle
# captures under its gitignored out/oracle/ for ITERATION; the
# established pattern for pinning oracle output in-repo is the frozen
# corpora (char/event/highlight/marked under the consumer's
# tests/data). The consumer here is oc-tui, so the frozen captures
# live under oc-tui's parity tree (tools/parity/tree-scenarios/oracle/)
# — committed, fingerprinted, never re-pinned casually.
#
# DOUBLE GATE (the PARITY_UPDATE stop-and-escalate discipline — never
# run casually, never from CI):
#   1. this script must be invoked EXPLICITLY, and
#   2. TREE_ORACLE_FREEZE=1 must be exported.
# Frozen oracle frames are evidence, not a scratchpad: a freeze is a
# reviewable diff of <scenario>.json only.
set -euo pipefail

if [[ "${TREE_ORACLE_FREEZE:-}" != "1" ]]; then
  echo "refusing: TREE_ORACLE_FREEZE=1 is not set (frozen oracles are re-pinned deliberately)" >&2
  exit 3
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCEN_DIR="${REPO_ROOT}/tools/parity/tree-scenarios"
ORACLE_DIR="${SCEN_DIR}/oracle"

# Locate the pinned opentui-rust checkout (the git dep's tools/diff
# harness — same rev the workspace Cargo.lock pins).
CHECKOUT="$(ls -d "${HOME}"/.cargo/git/checkouts/opentui-rust-*/2fbe00e 2>/dev/null | head -1)"
if [[ -z "${CHECKOUT}" ]]; then
  echo "opentui-rust @2fbe00e checkout not found under ~/.cargo/git/checkouts — build the workspace once first" >&2
  exit 3
fi
DIFF_DIR="${CHECKOUT}/tools/diff"
echo "oracle harness: ${DIFF_DIR}"

# The sandbox's node_modules (gitignored there) is the resolution pin
# (see tools/diff/README "the resolution incident").
if [[ ! -d "${DIFF_DIR}/sandbox/node_modules" ]]; then
  echo "installing the pinned sandbox (bun install)"
  (cd "${DIFF_DIR}/sandbox" && bun install)
fi

# Build the harness binary once in a scratch target dir (the git-dep
# checkout must stay build-artifact-free).
CARGO_TARGET_DIR=/tmp/opencode/target-opentui-diff cargo build -q --manifest-path "${DIFF_DIR}/Cargo.toml" --locked
BIN=/tmp/opencode/target-opentui-diff/debug/opentui-diff

mkdir -p "${ORACLE_DIR}"
pass=0
mismatch=0
for scenario in "${SCEN_DIR}"/tree-*.json; do
  name="$(basename "${scenario}" .json)"
  echo "--- ${name}"
  # --refresh-oracle forces a fresh TS capture (never trusts a cache);
  # the binary exits non-zero on any cell mismatch — the oracle frame
  # is still captured and frozen (the frozen ORACLE side is valid
  # evidence either way; a mismatch is a PORT-side finding the
  # standing gate pins).
  if "${BIN}" "${scenario}" --refresh-oracle; then
    echo "result: ${name} IDENTICAL"
    pass=$((pass + 1))
  else
    echo "result: ${name} MISMATCH (port-side finding — see the standing gate's pinned entry)"
    mismatch=$((mismatch + 1))
  fi
  cp "${DIFF_DIR}/out/oracle/${name}.json" "${ORACLE_DIR}/${name}.json"
  echo "frozen ${name}: ${ORACLE_DIR}/${name}.json"
done
echo "froze $((pass + mismatch)) oracle frame(s): ${pass} IDENTICAL, ${mismatch} MISMATCH (pinned)"
echo "standing gate: cargo nextest run -p oc-tui dual_engine_matches_frozen_oracle"
[[ ${mismatch} -eq 0 ]] || exit 4
