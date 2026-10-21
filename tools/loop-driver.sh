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
# Wiring (cron for the opencode-owning user — root on this box; flock prevents
# overlapping drivers):
#   */30 * * * * flock -n /tmp/opencode/loop-driver.lock \
#     /opt/opencode-rust/tools/loop-driver.sh >> /tmp/opencode/loop-driver.cron.log 2>&1
#
# Tunables via env:
#   OC_LOOP_RUN_FLAGS     extra flags for `opencode run` (default: --auto --format json)
#   OC_LOOP_QUOTA_SLEEP   seconds to wait on a suspected quota window (default 1800)
#   OC_LOOP_QUOTA_RETRIES max quota waits before giving up (default 6)
# Identity note: run this driver as the user that OWNS the opencode environment
# (root on this box — providers/config/session DB all live under /root; the
# eternal130 user has no opencode config). Build/test user isolation, CPU/memory
# caps, load backstop, and global serialization are enforced STRUCTURALLY by
# tools/dev-gate.sh (systemd-run --uid=eternal130 CPUQuota=400% MemoryMax=6G),
# which the orchestrator prompt mandates for every cargo invocation. The driver
# refuses to start only if that gate script is missing.
set -uo pipefail

REPO="/opt/opencode-rust"
PROMPT_FILE="$REPO/tools/loop-prompt-v2.md"
LOG_DIR="/tmp/opencode/loop-driver"
MAX_ITERS="${1:-25}"
RUN_FLAGS="${OC_LOOP_RUN_FLAGS:---auto --format json}"
QUOTA_SLEEP="${OC_LOOP_QUOTA_SLEEP:-1800}"
QUOTA_RETRIES_MAX="${OC_LOOP_QUOTA_RETRIES:-6}"

if [ ! -x "$REPO/tools/dev-gate.sh" ]; then
  echo "missing executable $REPO/tools/dev-gate.sh (the structural safety rail)" >&2
  exit 5
fi
command -v opencode >/dev/null 2>&1 || true
OC_BIN="${OC_LOOP_OPENCODE_BIN:-}"
if [ -z "$OC_BIN" ]; then
  for cand in "$(command -v opencode 2>/dev/null)" \
              "$HOME/.opencode/bin/opencode" \
              "/root/.opencode/bin/opencode" \
              "$HOME/.local/share/opencode/bin/opencode"; do
    if [ -n "$cand" ] && [ -x "$cand" ]; then OC_BIN="$cand"; break; fi
  done
fi
if [ -z "$OC_BIN" ] || [ ! -x "$OC_BIN" ]; then
  echo "opencode binary not found (cron PATH is minimal; set OC_LOOP_OPENCODE_BIN)" >&2
  exit 5
fi
[ -f "$PROMPT_FILE" ] || { echo "missing $PROMPT_FILE" >&2; exit 5; }
cd "$REPO" || exit 5
mkdir -p "$LOG_DIR"

# Terminal-state marker: once DONE/BLOCKED (or the breaker trips), cron must not
# resurrect the loop. Remove the file (or the cron line) to re-enable.
STOP_FILE="$LOG_DIR/STOPPED"
if [ -f "$STOP_FILE" ]; then
  echo "stop marker present ($(cat "$STOP_FILE")); remove $STOP_FILE to re-enable" >&2
  exit 0
fi

# Circuit breaker: 3 consecutive failed fires (no marker / hard rc!=0) stop the
# cron loop instead of burning tokens on a broken prompt forever.
FAIL_FILE="$LOG_DIR/consecutive-fails"
fail_count() { cat "$FAIL_FILE" 2>/dev/null || echo 0; }
bump_fail()    { echo $(( $(fail_count) + 1 )) > "$FAIL_FILE"; }
trip_breaker() { echo "$1" > "$STOP_FILE"; rm -f "$FAIL_FILE"; }

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
  run_scope "$OC_BIN" run $RUN_FLAGS "$PROMPT" >"$out" 2>&1
  rc=$?
  # Marker extraction: parse the WHOLE JSON event stream for the final assistant
  # text part (python3, no deps). Fixes two observed failure modes:
  #   (a) marker emitted mid-turn, buried under >200KB of later tool events —
  #       the old tail -c 200000 window missed it (session 2026-10-21 08:12);
  #   (b) prompt echo false-positives — user-role text and the prompt preamble
  #       ("你是 opencode-rust...") are excluded, only assistant text counts
  #       (session 2026-10-20 23:00 genuinely had no marker: the agent asked a
  #        question instead — that is a prompt-discipline failure, exit 3 is correct).
  final_text="$(python3 - "$out" 2>/dev/null <<'PYEOF' || true
import json, sys
last = None
try:
    with open(sys.argv[1], errors="ignore") as f:
        for line in f:
            try: e = json.loads(line)
            except Exception: continue
            if not isinstance(e, dict): continue
            p = e.get("part")
            if not isinstance(p, dict) or p.get("type") != "text": continue
            txt = p.get("text") or ""
            if not txt: continue
            info = e.get("info") if isinstance(e.get("info"), dict) else {}
            role = info.get("role") or e.get("role") or ""
            if role == "user": continue
            if txt.startswith("你是 opencode-rust"): continue  # prompt echo guard
            last = txt
except Exception:
    pass
sys.stdout.write(last or "")
PYEOF
)"
  marker_view="${final_text}${tail_view}"

  if [ "$rc" -ne 0 ]; then
    if printf '%s' "$marker_view" | grep -qiE 'usage limit|quota|rate.?limit|429'; then
      quota_retries=$((quota_retries + 1))
      if [ "$quota_retries" -gt "$QUOTA_RETRIES_MAX" ]; then
        log "quota retries exhausted ($QUOTA_RETRIES_MAX); giving up (next cron fire retries after window reset)"
        exit 4
      fi
      log "rc=$rc looks like a quota window; sleeping ${QUOTA_SLEEP}s (retry $quota_retries/$QUOTA_RETRIES_MAX)"
      sleep "$QUOTA_SLEEP"
      continue
    fi
    log "opencode run failed rc=$rc; stopping for manual inspection ($out)"
    bump_fail
    [ "$(fail_count)" -ge 3 ] && trip_breaker "breaker: 3 consecutive failed fires"
    exit 3
  fi

  if   printf '%s' "$marker_view" | grep -q 'PARITY_LOOP_DONE'; then
    echo "DONE $(date '+%F %T')" > "$STOP_FILE"
    rm -f "$FAIL_FILE"
    log "DONE"; exit 0
  elif printf '%s' "$marker_view" | grep -q 'PARITY_LOOP_BLOCKED'; then
    echo "BLOCKED $(date '+%F %T')" > "$STOP_FILE"
    rm -f "$FAIL_FILE"
    log "BLOCKED (see $out)"; exit 2
  elif printf '%s' "$marker_view" | grep -q 'BATCH_HANDOFF'; then
    rm -f "$FAIL_FILE"
    log "batch handoff; next session in 60s"
    sleep 60
  elif printf '%s' "$marker_view" | grep -q 'PARITY_LOOP_CONTINUE'; then
    rm -f "$FAIL_FILE"
    log "continue; fresh session in 10s"
    sleep 10
  else
    log "rc=0 but no marker found; stopping for manual inspection ($out)"
    bump_fail
    [ "$(fail_count)" -ge 3 ] && trip_breaker "breaker: 3 consecutive no-marker fires"
    exit 3
  fi
done

log "MAX_ITERS=$MAX_ITERS reached; stopping (rerun the driver to resume)"
exit 3
