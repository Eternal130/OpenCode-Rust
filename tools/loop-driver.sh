#!/usr/bin/env bash
# loop-driver.sh — external session-rotation driver for the opencode-rust parity loop.
#
# opencode v2 has no built-in "auto new session" scheduler; this driver provides it
# externally. Every `opencode run` invocation is a FRESH orchestrator session
# (context economy); cross-session handoff rides on docs/parity-loop-state.md +
# docs/parity-ledger.jsonl + git worktrees (prompt §0a rebuild is idempotent).
#
# Marker protocol (emitted by the orchestrator per tools/loop-prompt-v2.md):
#   PARITY_LOOP_CONTINUE  -> same batch, next unit: start a new session
#   BATCH_HANDOFF         -> batch boundary: brief pause, then a new session
#   PARITY_LOOP_DONE      -> exit 0 (release report written)
#   PARITY_LOOP_BLOCKED   -> exit 2 (needs a human decision)
#
# Exit codes: 0 done | 2 blocked | 3 unknown/no marker or max iters | 4 quota
#             window exhausted | 5 safety refusal (root / missing binary / etc.)
#
# Wiring (cron as the dev user, NEVER root; flock prevents overlapping drivers):
#   */30 * * * * flock -n /tmp/opencode/loop-driver.lock \
#     /opt/opencode-rust/tools/loop-driver.sh >> /tmp/opencode/loop-driver.cron.log 2>&1
#
# Tunables via env:
#   OC_LOOP_RUN_FLAGS     extra flags for `opencode run` (default: --auto --format json)
#   OC_LOOP_QUOTA_SLEEP   seconds to wait on a suspected quota window (default 1800)
#   OC_LOOP_QUOTA_RETRIES max quota waits before giving up (default 6)
# Safety rails: refuses root; wraps each run in systemd-run --scope CPUQuota=400%
# when available; per-session logs are append-only under /tmp/opencode/loop-driver/.
set -uo pipefail

REPO="/opt/opencode-rust"
PROMPT_FILE="$REPO/tools/loop-prompt-v2.md"
LOG_DIR="/tmp/opencode/loop-driver"
MAX_ITERS="${1:-25}"
RUN_FLAGS="${OC_LOOP_RUN_FLAGS:---auto --format json}"
QUOTA_SLEEP="${OC_LOOP_QUOTA_SLEEP:-1800}"
QUOTA_RETRIES_MAX="${OC_LOOP_QUOTA_RETRIES:-6}"

if [ "$(id -u)" -eq 0 ]; then
  echo "refusing to run as root (project hard rule)" >&2
  exit 5
fi
command -v opencode >/dev/null 2>&1 || { echo "opencode not in PATH" >&2; exit 5; }
[ -f "$PROMPT_FILE" ] || { echo "missing $PROMPT_FILE" >&2; exit 5; }
cd "$REPO" || exit 5
mkdir -p "$LOG_DIR"

log() { printf '[%s] %s\n' "$(date '+%F %T')" "$*" >> "$LOG_DIR/driver.log"; }

# CPU cap (4 cores) when systemd is available; plain exec otherwise.
run_scope() {
  if command -v systemd-run >/dev/null 2>&1; then
    systemd-run --quiet --scope --property=CPUQuota=400% "$@"
  else
    "$@"
  fi
}

PROMPT="$(cat "$PROMPT_FILE")"
quota_retries=0

for i in $(seq 1 "$MAX_ITERS"); do
  out="$LOG_DIR/session-$(date '+%Y%m%d-%H%M%S').log"
  log "iteration $i: fresh orchestrator session (log: $out)"
  run_scope opencode run $RUN_FLAGS "$PROMPT" >"$out" 2>&1
  rc=$?
  # Strip NULs; search only the tail window — markers are emitted at the end.
  tail_view="$(tail -c 200000 "$out" | tr -d '\000')"

  if [ "$rc" -ne 0 ]; then
    if printf '%s' "$tail_view" | grep -qiE 'usage limit|quota|rate.?limit|429'; then
      quota_retries=$((quota_retries + 1))
      if [ "$quota_retries" -gt "$QUOTA_RETRIES_MAX" ]; then
        log "quota retries exhausted ($QUOTA_RETRIES_MAX); giving up"
        exit 4
      fi
      log "rc=$rc looks like a quota window; sleeping ${QUOTA_SLEEP}s (retry $quota_retries/$QUOTA_RETRIES_MAX)"
      sleep "$QUOTA_SLEEP"
      continue
    fi
    log "opencode run failed rc=$rc; stopping for manual inspection ($out)"
    exit 3
  fi

  if   printf '%s' "$tail_view" | grep -q 'PARITY_LOOP_DONE'; then
    log "DONE"; exit 0
  elif printf '%s' "$tail_view" | grep -q 'PARITY_LOOP_BLOCKED'; then
    log "BLOCKED (see $out)"; exit 2
  elif printf '%s' "$tail_view" | grep -q 'BATCH_HANDOFF'; then
    log "batch handoff; next session in 60s"
    sleep 60
  elif printf '%s' "$tail_view" | grep -q 'PARITY_LOOP_CONTINUE'; then
    log "continue; fresh session in 10s"
    sleep 10
  else
    log "rc=0 but no marker found; stopping for manual inspection ($out)"
    exit 3
  fi
done

log "MAX_ITERS=$MAX_ITERS reached; stopping (rerun the driver to resume)"
exit 3
