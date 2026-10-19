#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `ledger_integrity` (§9.1): parity-ledger JSONL schema/state/evidence.

WHAT IT CHECKS
  Wraps `python3 tools/parity-ledger.py validate --paths` (the ledger
  tool shipped with the repo — stdlib-only, no build required) and turns
  every reported validation error into a violation. The ledger must
  validate clean: no schema violations, no `fixed` without test/evidence,
  no `verified` without an independent verifier, no dangling local paths.

HEURISTIC
  * Runs the ledger tool as a subprocess with the SAME python
    interpreter; a non-zero tool exit without parseable `  - <error>`
  * lines is itself a violation (`ledger:validate-exit=<rc>`).
  * `--exit-criteria` adds the ledger tool's `--exit` flag (the Batch-exit
    gates: S0/S1 not open, verified rows carrying merge commits). It is
    OFF by default because exit criteria are evaluated at batch
    boundaries, not on every CI push.
  * The row count from the tool's summary line is reported in `meta`
    for trend visibility.

KNOWN LIMITATIONS
  * The guard is only as strong as the wrapped validator — semantic
    ledger quality (is the repro real?) is out of scope.
  * Baseline for this guard is empty by design: ledger integrity must
    hold at all times; there is no acceptable standing violation.

UPDATE BASELINE: do not. A violation here is fixed by fixing the ledger
row (or the tool), never by baselining.
"""

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

LEDGER_TOOL = guardlib.ROOT / "tools" / "parity-ledger.py"
SUMMARY_RE = re.compile(r"rows=(\d+)\s+unique_ids=(\d+)\s+errors=(\d+)")

# Set from __main__ when the guard is invoked with --exit-criteria.
EXIT_CRITERIA = False


def collect() -> tuple[list[dict], dict]:  # noqa: C901 - flat is clearer here
    if not LEDGER_TOOL.is_file():
        raise guardlib.GuardError(f"ledger tool missing: {LEDGER_TOOL}")

    cmd = [sys.executable, str(LEDGER_TOOL), "validate", "--paths"]
    if EXIT_CRITERIA:
        cmd.append("--exit")

    try:
        proc = subprocess.run(  # noqa: S603 - fixed argv, no shell
            cmd, cwd=str(guardlib.ROOT), capture_output=True, text=True, timeout=120
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise guardlib.GuardError(f"ledger tool failed to run: {error}") from error

    output = proc.stdout + proc.stderr
    violations: list[dict] = []
    for line in output.splitlines():
        stripped = line.strip()
        if stripped.startswith("- "):
            violations.append({
                "id": f"ledger:{stripped[2:][:160]}",
                "detail": "parity-ledger validate error",
            })
    if proc.returncode != 0 and not violations:
        violations.append({
            "id": f"ledger:validate-exit={proc.returncode}",
            "detail": f"ledger tool exited {proc.returncode} without parseable errors: {output.strip()[:200]}",
        })

    summary = SUMMARY_RE.search(output)
    meta = {
        "tool_command": " ".join(cmd[2:]),
        "tool_exit": proc.returncode,
        "rows": int(summary.group(1)) if summary else None,
        "unique_ids": int(summary.group(2)) if summary else None,
    }
    return violations, meta


if __name__ == "__main__":
    # `--exit-criteria` is this guard's own flag; capture it, then strip
    # it before the common harness re-parses argv.
    if "--exit-criteria" in sys.argv:
        EXIT_CRITERIA = True
        sys.argv = [a for a in sys.argv if a != "--exit-criteria"]
    sys.exit(guardlib.run_guard("ledger_integrity", collect, __doc__))
