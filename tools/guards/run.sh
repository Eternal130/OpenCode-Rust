#!/usr/bin/env bash
# §9.1 static anti-regression guards — run all eight (docs/PARITY-LOOP.md §9).
#
#   tools/guards/run.sh                    — CI mode: fail on NEW violations
#   tools/guards/run.sh --update-baseline  — record current sets (see README)
#
# Guards are stdlib-only Python 3 and need no bun / reference checkout /
# cargo build. Exit code is nonzero if any guard reports new violations
# (1), an expired allowlist entry (1), or a guard error (2).
set -u

# Keep the tree clean: guards import a shared module (guardlib.py).
export PYTHONDONTWRITEBYTECODE=1

here="$(cd "$(dirname "$0")" && pwd)"

GUARDS=(
  config_consumers
  hook_dispatch
  route_validation
  event_schema
  env_inventory
  command_keymap
  ledger_integrity
  process_safety
)

extra=""
if [[ "${1:-}" == "--update-baseline" ]]; then
  extra="--update-baseline"
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--update-baseline]" >&2
  exit 2
fi

declare -A summary
overall=0
for guard in "${GUARDS[@]}"; do
  echo "=== guard: ${guard} ==="
  python3 "${here}/${guard}.py" ${extra:+"${extra}"}
  rc=$?
  if [[ ${rc} -ne 0 ]]; then
    overall=1
    summary[${guard}]="exit=${rc}"
  fi
done

echo
echo "=== guards summary ==="
failed=0
for guard in "${GUARDS[@]}"; do
  if [[ -n "${summary[${guard}]:-}" ]]; then
    echo "FAIL ${guard} (${summary[${guard}]})"
    failed=1
  else
    echo "ok   ${guard}"
  fi
done
if [[ ${failed} -eq 0 ]]; then
  echo "guards: all green (baseline may only shrink; new violations are red)"
fi
exit "${overall}"
