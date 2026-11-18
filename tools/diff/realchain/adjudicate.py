#!/usr/bin/env python3
"""W12 diff-adjudication checker (user ruling 2026-11-18, GAPS 2.2#12).

Reads the realchain battery's exported diff reports
(`.test-logs/diff-reports/*.json`) and matches every divergence against
the committed adjudication registry (`adjudications.json`, sibling of
this script by default). The sandbox lane judges NOTHING — this checker
is the controller-side merge gate.

Exit codes:
  0  every divergence adjudicated (EXACT reports included) and NO
     expected-shape violation
  2  UNADJUDICATED divergences OR an expected-shape entry matched (a
     registered expectation was BROKEN — the violation form) — the
     merge-gate red
  3  infrastructure error (missing/invalid reports dir or registry)

Expected-shape semantics (C-1, review fix): those entries describe the
VIOLATION form of a registered expectation — a match is a RED, not a
pass. Their green channel is implicit: the scenario/state report
existing without a matching divergence means the expectation held
(counted in the "expected-shape satisfied" line; stale hints skip the
class outright).

Usage:
  python3 tools/diff/realchain/adjudicate.py .test-logs/diff-reports
  python3 tools/diff/realchain/adjudicate.py .test-logs/diff-reports \
      --update-last-observed        # rewrite registry last_observed
  python3 tools/diff/realchain/adjudicate.py DIR --stale-days 14

Stdlib only; run directly (never wrapped in dev-gate — rule 2.1#2).
"""

from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import json
import re
import sys
from pathlib import Path

DEFAULT_REGISTRY = Path(__file__).with_name("adjudications.json")
DEFAULT_STALE_DAYS = 30


def load_registry(path: Path) -> list[dict]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise SystemExit(f"adjudicate: registry unreadable ({path}): {exc}") from exc
    entries = data.get("entries")
    if not isinstance(entries, list):
        raise SystemExit(f"adjudicate: registry {path} carries no 'entries' list")
    # Most-specific-first: longer scenario_pattern wins, then registry order.
    entries.sort(key=lambda e: -len(e.get("scenario_pattern", "")))
    return entries


def shape_matches(shape: dict, ref: str, rust: str) -> bool:
    kind = shape.get("kind", "substring")

    def contains_all(haystacks: list[str], needles: list[str]) -> bool:
        return all(
            any(re.search(needle, haystack) for haystack in haystacks)
            for needle in needles
        )

    if kind == "regex":
        contains = lambda needles, haystacks: contains_all(haystacks, [f"(?:{n})" for n in needles])  # noqa: E731
    else:
        contains = lambda needles, haystacks: all(  # noqa: E731
            any(needle in haystack for haystack in haystacks) for needle in needles
        )

    if shape.get("ref_contains") and not contains(shape["ref_contains"], [ref]):
        return False
    if shape.get("rust_contains") and not contains(shape["rust_contains"], [rust]):
        return False
    if shape.get("any_contains") and not contains(shape["any_contains"], [ref, rust]):
        return False
    if shape.get("rust_missing") and contains(shape["rust_missing"], [rust]):
        return False
    if shape.get("ref_missing") and contains(shape["ref_missing"], [ref]):
        return False
    # A shape with no positive/negative clause matches nothing (guard
    # against accidentally-blank registry entries matching the world).
    return any(
        key in shape
        for key in ("ref_contains", "rust_contains", "any_contains", "rust_missing", "ref_missing")
    )


def entry_matches(entry: dict, scenario: str, state: str, ref: str, rust: str) -> bool:
    if not fnmatch.fnmatchcase(scenario, entry.get("scenario_pattern", "*")):
        return False
    if not fnmatch.fnmatchcase(state, entry.get("state_pattern", "*")):
        return False
    return shape_matches(entry.get("shape", {}), ref, rust)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("reports_dir", type=Path)
    parser.add_argument("--registry", type=Path, default=DEFAULT_REGISTRY)
    parser.add_argument("--stale-days", type=int, default=DEFAULT_STALE_DAYS)
    parser.add_argument(
        "--update-last-observed",
        action="store_true",
        help="rewrite each matched entry's last_observed to today",
    )
    args = parser.parse_args()

    if not args.reports_dir.is_dir():
        print(f"adjudicate: reports dir missing: {args.reports_dir}", file=sys.stderr)
        return 3
    if not args.registry.is_file():
        print(f"adjudicate: registry missing: {args.registry}", file=sys.stderr)
        return 3

    entries = load_registry(args.registry)
    report_paths = sorted(args.reports_dir.glob("*.json"))
    if not report_paths:
        print(f"adjudicate: no *.json reports under {args.reports_dir}", file=sys.stderr)
        return 3

    today = dt.date.today().isoformat()
    total = 0
    adjudicated = 0
    unmatched: list[str] = []
    violations: list[str] = []
    matched_ids: set[str] = set()
    adjudicated_ids: set[str] = set()
    # (scenario, state) of every report on disk — feeds the
    # expected-shape "expectation held" channel (C-1: an expected-shape
    # entry whose scenario/state report exists WITHOUT a matching
    # divergence means the expectation was satisfied this round).
    seen_scenarios: list[tuple[str, str]] = []

    for path in report_paths:
        try:
            report = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            print(f"adjudicate: invalid report {path}: {exc}", file=sys.stderr)
            return 3
        scenario = report.get("scenario", path.stem)
        state = report.get("state", "")
        tree_hash = str(report.get("tree_hash", "?"))
        seen_scenarios.append((scenario, state))
        where = f"scenario={scenario} state={state} tree_hash={tree_hash}"
        divergences = report.get("divergences", [])
        if not divergences:
            continue  # EXACT — nothing to adjudicate
        for div in divergences:
            total += 1
            ref = str(div.get("ref", ""))
            rust = str(div.get("rust", ""))
            line_no = div.get("line_no", "?")
            hit = next(
                (
                    entry
                    for entry in entries
                    if entry_matches(entry, scenario, state, ref, rust)
                ),
                None,
            )
            if hit is None:
                unmatched.append(
                    f"  {path.name} [{where}] line {line_no}: ref={ref!r} rust={rust!r}"
                )
            elif hit.get("classification") == "expected-shape":
                # C-1: expected-shape entries describe the VIOLATION
                # form — a hit means the registered expectation was
                # BROKEN this round. That is a red (exit 2), never a
                # pass. (In an unadjudicated-by-default architecture,
                # expectation entries are inherently inverted: the
                # semantic fork must live here, in the checker.)
                violations.append(
                    f"  {path.name} [{where}] line {line_no}: "
                    f"expected-shape VIOLATED ({hit['id']}): "
                    f"ref={ref!r} rust={rust!r}"
                )
                matched_ids.add(hit["id"])
            else:
                adjudicated += 1
                adjudicated_ids.add(hit["id"])
                matched_ids.add(hit["id"])

    # Expected-shape entries whose scenario/state report exists but
    # produced NO matching divergence: the expectation HELD (green
    # observation channel — also keeps these entries out of the stale
    # hints, which additionally skip the class outright, see below).
    expected_ok: set[str] = set()
    for entry in entries:
        if entry.get("classification") != "expected-shape":
            continue
        for scenario, state in seen_scenarios:
            if fnmatch.fnmatchcase(scenario, entry.get("scenario_pattern", "*")) and fnmatch.fnmatchcase(
                state, entry.get("state_pattern", "*")
            ):
                expected_ok.add(entry["id"])
                matched_ids.add(entry["id"])
                break

    print(f"adjudicate: {len(report_paths)} reports, {total} divergences")
    print(f"  adjudicated: {adjudicated}/{total} ({', '.join(sorted(adjudicated_ids)) or '-'})")
    if expected_ok:
        print(f"  expected-shape satisfied: {len(expected_ok)} ({', '.join(sorted(expected_ok))})")
    if unmatched:
        print("  UNADJUDICATED:")
        print("\n".join(unmatched))
    if violations:
        print("  EXPECTED-SHAPE VIOLATED (registered expectation broken — red):")
        print("\n".join(violations))

    # Stale entries: a registered shape nobody observed in ANY current
    # report AND whose last_observed is older than the cutoff. A hint
    # to retire the entry — never a red. Expected-shape entries are
    # skipped: their green rounds produce no divergence, so absence of
    # a hit is the HEALTHY signal (I-2) — the satisfied channel above
    # already refreshes their last_observed.
    stale: list[str] = []
    cutoff = (dt.date.today() - dt.timedelta(days=args.stale_days)).isoformat()
    for entry in entries:
        if entry.get("classification") == "expected-shape":
            continue
        if entry["id"] in matched_ids:
            continue
        last = str(entry.get("last_observed", ""))
        if last and last < cutoff:
            stale.append(f"  {entry['id']} (last_observed {last})")
    if stale:
        print(f"stale registry shapes (not observed for >{args.stale_days} days; consider retiring):")
        print("\n".join(stale))

    if args.update_last_observed and matched_ids:
        # NOTE: only the MATCHED subset is rewritten (violated
        # expected-shape and satisfied expectations included; unmatched
        # divergences' hypothetical entries are not). Atomic: tmp +
        # rename so a crash never leaves a half-written registry.
        raw = json.loads(args.registry.read_text(encoding="utf-8"))
        for entry in raw["entries"]:
            if entry["id"] in matched_ids:
                entry["last_observed"] = today
        tmp = args.registry.with_name(args.registry.name + ".tmp")
        tmp.write_text(
            json.dumps(raw, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
        tmp.replace(args.registry)
        print(f"updated last_observed -> {today} for {len(matched_ids)} entries")

    return 2 if (unmatched or violations) else 0


if __name__ == "__main__":
    sys.exit(main())
