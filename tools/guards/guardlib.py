#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Shared harness for the §9.1 static guards (docs/PARITY-LOOP.md §9).

Common conventions for every guard in tools/guards/:

  * default mode        — exit 0 iff every current violation is covered by
                          the committed baseline or the allowlist; a NEW
                          violation (or an expired allowlist entry) exits 1.
  * --update-baseline   — rewrite tools/guards/baseline/<guard>.json with the
                          current violation set. Deterministic: sorted ids,
                          no timestamps. Requires a justification in the
                          committing message (see README.md).
  * --format {text,json}— text prints a human summary plus a final
                          ``SUMMARY <guard> total=.. baseline=.. allowlist=..
                          new=.. shrunk=..`` line; json prints one object.
  * exit 2              — guard error (malformed allowlist/baseline, missing
                          tool input). Distinct from "new violation" (1).

Baseline semantics (§9.2): the baseline is the recorded set of current
violations and may only shrink. Entries that no longer reproduce are
reported as ``shrunk`` (informational — never a failure). Allowlist
entries are explicit, ledger-backed exceptions (``pattern | reason |
ledger-id | owner | review-date``); an entry whose review-date is in the
past stops covering (expiry ⇒ CI red, §9.2).

Allowlist pattern matching (no wildcards allowed):
  1. exact match against the violation id;
  2. exact match against the path component of a ``path:line:rule`` id;
  3. prefix match when the pattern ends with ``/`` (directory scope).

Stdlib only. Nothing here signals processes, touches the network, or
requires bun / the reference checkout / cargo.
"""

import argparse
import datetime as _dt
import json
import re
import sys
from pathlib import Path

GUARDS_DIR = Path(__file__).resolve().parent
ROOT = GUARDS_DIR.parent.parent
BASELINE_DIR = GUARDS_DIR / "baseline"
ALLOWLIST_DIR = GUARDS_DIR / "allowlist"

TODAY = _dt.date.today()


class GuardError(Exception):
    """Fatal guard misconfiguration — exit 2."""


# --------------------------------------------------------------------------
# shared path helpers
# --------------------------------------------------------------------------

def is_test_path(rel_posix: str) -> bool:
    """File-level test exclusion used by every code-scanning guard.

    A reference inside a test *file* does not count as a production
    consumer/dispatcher/handler. cfg(test) modules inside production
    files are NOT stripped — that limitation is documented per guard
    (it can only under-report violations, never invent new ones).
    """
    return ("/tests/" in rel_posix) or rel_posix.endswith(("tests.rs", "_tests.rs"))


def rust_files(root: Path) -> list[tuple[str, Path]]:
    """All .rs files under root as (relative-posix, absolute), sorted."""
    out: list[tuple[str, Path]] = []
    for path in sorted(root.rglob("*.rs")):
        if not path.is_file():
            continue
        out.append((path.relative_to(ROOT).as_posix(), path))
    return out


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise GuardError(f"cannot read {path}: {error}") from error


# --------------------------------------------------------------------------
# baseline
# --------------------------------------------------------------------------

def baseline_path(guard: str) -> Path:
    return BASELINE_DIR / f"{guard}.json"


def load_baseline(guard: str) -> set[str]:
    path = baseline_path(guard)
    if not path.is_file():
        raise GuardError(f"baseline missing: {path} (run with --update-baseline)")
    try:
        data = json.loads(read_text(path))
    except json.JSONDecodeError as error:
        raise GuardError(f"baseline {path} is not valid JSON: {error}") from error
    if data.get("guard") != guard:
        raise GuardError(f"baseline {path} declares guard {data.get('guard')!r}")
    ids = data.get("violations")
    if not isinstance(ids, list) or not all(isinstance(v, str) for v in ids):
        raise GuardError(f"baseline {path}: 'violations' must be a list of strings")
    if ids != sorted(ids):
        raise GuardError(f"baseline {path}: violations not sorted (regenerate)")
    return set(ids)


def write_baseline(guard: str, ids: list[str]) -> Path:
    BASELINE_DIR.mkdir(parents=True, exist_ok=True)
    path = baseline_path(guard)
    payload = {
        "guard": guard,
        # No timestamp: baselines must be byte-deterministic.
        "count": len(ids),
        "violations": sorted(ids),
    }
    path.write_text(json.dumps(payload, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    return path


# --------------------------------------------------------------------------
# allowlist
# --------------------------------------------------------------------------

ALLOW_ENTRY_RE = re.compile(r"^(?P<pattern>[^|]+)\|(?P<reason>[^|]+)\|(?P<ledger>[^|]+)\|(?P<owner>[^|]+)\|(?P<date>[^|]+)$")


def load_allowlist(guard: str) -> list[dict]:
    path = ALLOWLIST_DIR / f"{guard}.txt"
    if not path.is_file():
        raise GuardError(f"allowlist missing: {path}")
    entries: list[dict] = []
    for lineno, raw in enumerate(read_text(path).splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        match = ALLOW_ENTRY_RE.match(line)
        if match is None:
            raise GuardError(
                f"{path}:{lineno}: not 'pattern | reason | ledger-id | owner | review-date': {line!r}"
            )
        entry = {key: value.strip() for key, value in match.groupdict().items()}
        if "*" in entry["pattern"]:
            raise GuardError(f"{path}:{lineno}: wildcard patterns are forbidden (§9.2): {entry['pattern']!r}")
        for field in ("reason", "ledger", "owner", "date"):
            if not entry[field]:
                raise GuardError(f"{path}:{lineno}: empty {field} (§9.2)")
        try:
            entry["date"] = str(_dt.date.fromisoformat(entry["date"]))
        except ValueError as error:
            raise GuardError(f"{path}:{lineno}: bad review-date: {error}") from error
        entry["line"] = lineno
        entries.append(entry)
    return entries


def _id_path_component(violation_id: str) -> str:
    return violation_id.split(":", 1)[0]


def allowlist_match(violation_id: str, entries: list[dict]) -> dict | None:
    for entry in entries:
        pattern = entry["pattern"]
        if violation_id == pattern:
            return entry
        if _id_path_component(violation_id) == pattern:
            return entry
        if pattern.endswith("/") and _id_path_component(violation_id).startswith(pattern):
            return entry
    return None


# --------------------------------------------------------------------------
# evaluation + output
# --------------------------------------------------------------------------

def evaluate(guard: str, violations: list[dict]) -> dict:
    """Compute new/shrunk/covered sets. `violations`: [{id, detail}, ...]."""
    for item in violations:
        if not item.get("id") or "\n" in item["id"]:
            raise GuardError(f"guard {guard}: violation without a stable id: {item!r}")
    baseline = load_baseline(guard)
    allow = load_allowlist(guard)

    expired = [e for e in allow if e["date"] < TODAY.isoformat()]
    expired_patterns = {e["pattern"] for e in expired}

    current_ids = sorted({item["id"] for item in violations})
    details = {item["id"]: item.get("detail", "") for item in violations}

    covered_allow: dict[str, dict] = {}
    for vid in current_ids:
        entry = allowlist_match(vid, [e for e in allow if e["pattern"] not in expired_patterns])
        if entry is not None:
            covered_allow[vid] = entry

    new_ids = [vid for vid in current_ids if vid not in baseline and vid not in covered_allow]
    shrunk = sorted(baseline - set(current_ids))
    # Allowlist entries that no longer match anything (fixed debt) — info only.
    matched_patterns = {e["pattern"] for e in covered_allow.values()}
    orphan = sorted(
        e["pattern"] for e in allow
        if e["pattern"] not in expired_patterns and e["pattern"] not in matched_patterns
        and not any(allowlist_match(b, [e]) for b in baseline)
    )

    return {
        "guard": guard,
        "total": len(current_ids),
        "baseline": sum(1 for v in current_ids if v in baseline),
        "allowlisted": len(covered_allow),
        "new": len(new_ids),
        "shrunk": len(shrunk),
        "new_violations": [{"id": v, "detail": details[v]} for v in new_ids],
        "shrunk_baseline": shrunk,
        "expired_allowlist": sorted(expired_patterns),
        "orphan_allowlist": orphan,
    }


def _emit_text(result: dict) -> None:
    guard = result["guard"]
    for item in result["new_violations"]:
        print(f"NEW {guard}: {item['id']} — {item['detail']}")
    for vid in result["shrunk_baseline"]:
        print(f"INFO {guard}: baseline entry no longer reproduces (shrink ok): {vid}")
    for pattern in result["orphan_allowlist"]:
        print(f"INFO {guard}: allowlist entry matches nothing anymore (remove when convenient): {pattern}")
    for pattern in result["expired_allowlist"]:
        print(f"EXPIRED {guard}: allowlist entry past review-date {pattern}")
    print(
        f"SUMMARY {guard} total={result['total']} baseline={result['baseline']} "
        f"allowlist={result['allowlisted']} new={result['new']} shrunk={result['shrunk']}"
    )


def _emit_json(result: dict, meta: dict | None) -> None:
    payload = dict(result)
    if meta is not None:
        payload["meta"] = meta
    print(json.dumps(payload, indent=2, sort_keys=False))


def run_guard(guard: str, collect, doc: str) -> int:
    """Wire the common CLI around a guard's `collect() -> (violations, meta)`."""
    parser = argparse.ArgumentParser(description=doc.splitlines()[0])
    parser.add_argument("--update-baseline", action="store_true",
                        help="rewrite tools/guards/baseline/%s.json with the current set" % guard)
    parser.add_argument("--format", choices=("text", "json"), default="text")
    args = parser.parse_args()

    try:
        violations, meta = collect()
    except GuardError as error:
        print(f"{guard}: GUARD-ERROR: {error}", file=sys.stderr)
        return 2

    if args.update_baseline:
        try:
            path = write_baseline(guard, sorted({v["id"] for v in violations}))
        except OSError as error:
            print(f"{guard}: GUARD-ERROR: cannot write baseline: {error}", file=sys.stderr)
            return 2
        print(f"{guard}: baseline updated: {path} ({len(violations)} violations)")
        return 0

    try:
        result = evaluate(guard, violations)
    except GuardError as error:
        print(f"{guard}: GUARD-ERROR: {error}", file=sys.stderr)
        return 2

    if args.format == "json":
        _emit_json(result, meta)
    else:
        _emit_text(result)

    if result["new_violations"] or result["expired_allowlist"]:
        return 1
    return 0
