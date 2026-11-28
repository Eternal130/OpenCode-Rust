#!/usr/bin/env bash
# dev-gate-echo — the bounded console echo for a COMPLETED gated run.
#
# Called by tools/dev-gate.sh (not by users) after the evidence tee has
# written the complete log:   dev-gate-echo.sh <log> <run-rc>
# The log on disk is the evidence and is always complete; this script only
# decides what the CONSOLE (a subagent's context, a shell capture) sees.
#
# Why it exists (2026-11-28, the va22a fragment-log incident): a full merge
# gate prints ~2500 lines / ~280 KB (~70k tokens). Lanes coped by filtering
# INSIDE the gated command — which shrank the run AND the log. This keeps
# the gate command whole and bounds the echo instead.
#
#   * <= WHOLE_MAX lines                  -> echoed whole
#   * rc != 0 and <= FAIL_WHOLE_MAX lines -> echoed whole (diagnostics win)
#   * otherwise                           -> the digest: the Summary / FAIL /
#     TRY (retry verdicts) / SLOW / rustc error + location / Finished /
#     test-result / panic lines (the last DIGEST_MAX of them, each with its
#     log line number); on rc != 0 also the last TAIL_ON_FAIL lines.
#
# Output goes to stdout — the stream the tee itself used for short outputs,
# so programmatic callers see the same bytes as before.
#
# Always exits 0: it must never alter the gate's exit status.
set -u

LOG="${1:?usage: dev-gate-echo.sh <log> <run-rc>}"
RC="${2:-0}"
[[ "$RC" =~ ^[0-9]+$ ]] || RC=1

WHOLE_MAX=200
FAIL_WHOLE_MAX=600
DIGEST_MAX=200
TAIL_ON_FAIL=40

if [[ ! -r "$LOG" ]]; then
  echo "dev-gate: echo: log ${LOG} is not readable" >&2
  exit 0
fi
lines=$(wc -l < "$LOG" | tr -d '[:space:]')

# Short output: exactly what the tee used to pass through.
if [ "$lines" -le "$WHOLE_MAX" ]; then
  cat "$LOG"
  exit 0
fi
# A failure of moderate size: the diagnostics are worth the tokens.
if [ "$RC" -ne 0 ] && [ "$lines" -le "$FAIL_WHOLE_MAX" ]; then
  cat "$LOG"
  exit 0
fi

echo "dev-gate: quiet echo — the run printed ${lines} lines (rc=${RC}); digest below (line:no = log line), full log: ${LOG}"
digest=$(grep -nE '^[[:space:]]*(Summary \[|SLOW \[|TRY [0-9]+ (PASS|FAIL)|FAIL \[|error(\[E[0-9]+\])?:|-->[[:space:]]|Finished|test result:|thread .* panicked at|could not compile)' "$LOG" | tail -n "$DIGEST_MAX")
if [ -n "$digest" ]; then
  printf '%s\n' "$digest"
else
  echo "dev-gate: no digest-shaped line matched — head and tail of the log:"
  head -n 10 "$LOG"
  echo "   ..."
  tail -n 10 "$LOG"
fi
if [ "$RC" -ne 0 ]; then
  echo "── last ${TAIL_ON_FAIL} lines of the log ──"
  tail -n "$TAIL_ON_FAIL" "$LOG"
fi
exit 0
