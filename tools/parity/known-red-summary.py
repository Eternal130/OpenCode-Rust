#!/usr/bin/env python3
"""Classify a nextest run log against the known-red registry (PARITY-LOOP.md
§8.5, B0-2).

Reads a nextest log (file argument or stdin) and classifies every FAILED
test into:

* KNOWN-RED      — the failure output carries a ``KNOWN-RED:<ledger-id>:``
                   marker whose ledger id is registered in known-red.json
                   and not yet expired;
* NEW-RED        — the failure carries a ``NEW-RED:<ledger-id>:`` marker, or
                   a registered-but-expired id, or NO marker at all (an
                   uncategorized failure is a new red by definition: gates
                   route every expected mismatch through the known_red()
                   helper);
* MISSING-PREREQ — the failure is a strict-mode gate-prerequisite panic
                   (``gate prerequisite missing``, e.g. Gate 2b's unset
                   ``PARITY_V2_DB_FIXTURE``). The test FAILED in nextest —
                   this classifier only labels it; it is never a pass.

Exit code 0 only when there is no NEW-RED and every KNOWN-RED marker
resolves to an unexpired registry entry (a broken/missing registry exits 2).

Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

BATCH_ORDER = ["Batch 0", "Batch 1A", "Batch 1B", "Batch 2", "Batch 3", "Batch 4"]

# Matches nextest event lines, including retry attempts (`TRY 1 FAIL`),
# final failures (`FAIL`/`TRY 2 FAIL`), and passes/skips.
EVENT_RE = re.compile(
    r"^\s*(?:TRY\s+\d+(?:/\d+)?\s+)?(PASS|FAIL|RETRY|ABORT|TIMEOUT|SKIP)"
    r"(?:\s+\d+/\d+)?\s+\[[^\]]*\]\s+(?:\([^)]*\)\s+)?(.+?)\s*(?:\s--\s.*)?$"
)
KNOWN_RE = re.compile(r"KNOWN-RED:([A-Za-z0-9-]+)")
NEW_RE = re.compile(r"NEW-RED:([A-Za-z0-9-]+)")
PREREQ_RE = re.compile(r"gate prerequisite missing")


def batch_rank(batch: str) -> int:
    try:
        return BATCH_ORDER.index(batch)
    except ValueError:
        return len(BATCH_ORDER)  # unknown labels never silently expire


def load_registry(path: Path) -> dict[str, dict]:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        print(f"known-red-summary: cannot read registry {path}: {error}", file=sys.stderr)
        sys.exit(2)
    try:
        rows = json.loads(text)
    except json.JSONDecodeError as error:
        print(f"known-red-summary: registry {path} is not valid JSON: {error}", file=sys.stderr)
        sys.exit(2)
    registry: dict[str, dict] = {}
    for row in rows if isinstance(rows, list) else []:
        ledger_id = row.get("ledger_id")
        if not isinstance(ledger_id, str):
            print(f"known-red-summary: registry entry without ledger_id: {row!r}", file=sys.stderr)
            sys.exit(2)
        if ledger_id in registry:
            print(f"known-red-summary: duplicate ledger id {ledger_id}", file=sys.stderr)
            sys.exit(2)
        registry[ledger_id] = row
    return registry


def classify(lines: list[str]) -> tuple[dict[str, dict], Counter, str]:
    """Parse the log into per-test state + a few run-level counters."""
    tests: dict[str, dict] = defaultdict(lambda: {"events": [], "known": [], "new": [], "prereq": False})
    counts: Counter = Counter()
    summary = ""
    current: str | None = None
    for line in lines:
        match = EVENT_RE.match(line)
        if match:
            event, test = match.group(1), match.group(2).strip()
            current = test
            tests[test]["events"].append(event)
            counts[event] += 1
            continue
        if line.lstrip().startswith("Summary"):
            summary = line.strip()
            current = None
            continue
        if current is None:
            continue
        state = tests[current]
        state["known"].extend(KNOWN_RE.findall(line))
        state["new"].extend(NEW_RE.findall(line))
        if PREREQ_RE.search(line):
            state["prereq"] = True
    return tests, counts, summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("log", nargs="?", help="nextest log file (default: stdin)")
    parser.add_argument(
        "--registry",
        type=Path,
        default=Path(__file__).resolve().parent / "goldens/p10/known-red.json",
        help="known-red registry (default: tools/parity/goldens/p10/known-red.json)",
    )
    parser.add_argument(
        "--current-batch",
        default="Batch 0",
        help="the batch evaluating expiry (default: Batch 0)",
    )
    args = parser.parse_args()

    registry = load_registry(args.registry)
    current_rank = batch_rank(args.current_batch)

    if args.log:
        try:
            lines = Path(args.log).read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError as error:
            print(f"known-red-summary: cannot read log {args.log}: {error}", file=sys.stderr)
            return 2
    else:
        lines = sys.stdin.read().splitlines()

    tests, counts, summary = classify(lines)

    # A test's outcome is its LAST event (retries: a later PASS redeems an
    # earlier FAIL attempt).
    failed = {
        name: state
        for name, state in tests.items()
        if state["events"] and state["events"][-1] in ("FAIL", "ABORT", "TIMEOUT")
    }

    known_counts: Counter = Counter()
    new_counts: Counter = Counter()
    prereq_tests: list[str] = []
    errors: list[str] = []

    for name, state in sorted(failed.items()):
        new_ids = sorted(set(state["new"]))
        known_ids = sorted(set(state["known"]))
        if new_ids:
            for ledger_id in new_ids:
                new_counts[ledger_id] += 1
            print(f"NEW-RED  {name}  ids={new_ids}")
        elif known_ids:
            for ledger_id in known_ids:
                entry = registry.get(ledger_id)
                if entry is None:
                    errors.append(
                        f"{name}: KNOWN-RED marker {ledger_id} has no registry entry"
                    )
                    new_counts[ledger_id] += 1
                    continue
                expiry = str(entry.get("expiry_batch", ""))
                if batch_rank(expiry) < current_rank:
                    errors.append(
                        f"{name}: known-red {ledger_id} expired at {expiry} "
                        f"(current {args.current_batch}) — §8.5: expired unfixed = gate failure"
                    )
                    new_counts[ledger_id] += 1
                    continue
                known_counts[ledger_id] += 1
            print(f"KNOWN-RED  {name}  ids={known_ids}")
        elif state["prereq"]:
            prereq_tests.append(name)
            print(f"MISSING-PREREQ  {name}  (strict-mode gate prerequisite; test FAILED — not a pass)")
        else:
            new_counts["<uncategorized>"] += 1
            print(f"NEW-RED  {name}  ids=[<no known-red marker routed>]")

    print()
    print(f"run summary line: {summary or '(not found)'}")
    print(f"events: {dict(sorted(counts.items()))}")
    print(f"failed tests: {len(failed)}")
    print(f"known-red per ledger id: {dict(sorted(known_counts.items())) or '{}'}")
    print(f"new-red per ledger id: {dict(sorted(new_counts.items())) or '{}'}")
    print(f"missing-prereq failures: {len(prereq_tests)}")
    for error in errors:
        print(f"REGISTRY-ERROR: {error}")

    if new_counts:
        print("verdict: NEW-RED present — gate fails (§8.5)")
        return 1
    if errors:
        print("verdict: registry verification failed — gate fails (§8.5)")
        return 1
    print("verdict: clean (0 NEW-RED, 0 expired known-red)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
