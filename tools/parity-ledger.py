#!/usr/bin/env python3
"""Operate the parity ledger (docs/parity-ledger.jsonl).

Commands:
  validate   schema/state/evidence checks (and optional local-path checks)
  summary    counts by status/severity/cluster + ready queue
  next       print the next ready entries (S0 first)
  dedupe     group by fingerprint and report duplicates
  evidence   append an evidence record; set fixed/verified
  merge      record merge_commit/merged_at after integrator merge

No third-party dependencies. This tool never edits product code and never
signals processes.
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "parity-ledger.jsonl"

STATUSES = {
    "open", "reproduced", "fixing", "fixed", "verified",
    "needs-runtime", "accepted", "duplicate", "blocked",
}
SEVERITIES = {"S0", "S1", "S2", "S3", "needs-runtime"}
CLUSTERS = {
    "CRED", "PLUGIN", "RUNTIME", "WIRE", "CONFIG",
    "TUI", "AI", "DB", "SURFACES", "TOOLS-EXEC",
}
REQUIRED = [
    "id", "source_id", "fingerprint", "domain", "cluster", "severity",
    "status", "title", "symptom", "expected", "actual", "ref", "rust",
    "repro", "test", "owner", "upstream", "wiring", "evidence",
]
# Fields that must be present but are allowed to be empty strings:
# owner is only set once a lane claims the item (B0-1 imports unclaimed rows).
MAY_BE_EMPTY = {"owner"}
SEV_ORDER = {"S0": 0, "S1": 1, "S2": 2, "S3": 3, "needs-runtime": 4}


def load() -> list[dict]:
    if not LEDGER.is_file():
        sys.exit(f"ledger not found: {LEDGER}")
    rows = []
    for lineno, line in enumerate(LEDGER.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError as exc:
            sys.exit(f"{LEDGER}:{lineno}: invalid JSON: {exc}")
    return rows


def save(rows: list[dict]) -> None:
    LEDGER.write_text(
        "\n".join(json.dumps(r, ensure_ascii=False) for r in rows) + "\n",
        encoding="utf-8",
    )


def now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def cmd_validate(args: argparse.Namespace) -> int:
    rows = load()
    errors: list[str] = []
    ids: set[str] = set()
    for row in rows:
        rid = row.get("id", "<missing-id>")
        for key in REQUIRED:
            value = row.get(key)
            if value is None or (value == "" and key not in MAY_BE_EMPTY):
                errors.append(f"{rid}: missing/empty {key}")
        if rid in ids:
            errors.append(f"{rid}: duplicate id")
        ids.add(rid)
        if row.get("status") not in STATUSES:
            errors.append(f"{rid}: bad status {row.get('status')!r}")
        if row.get("severity") not in SEVERITIES:
            errors.append(f"{rid}: bad severity {row.get('severity')!r}")
        if row.get("cluster") not in CLUSTERS:
            errors.append(f"{rid}: bad cluster {row.get('cluster')!r}")
        if row.get("status") in {"fixed", "verified"}:
            if not row.get("test"):
                errors.append(f"{rid}: {row['status']} without test")
            evidence = row.get("evidence") or []
            if not evidence:
                errors.append(f"{rid}: {row['status']} without evidence")
            if not any(e.get("red_artifact") for e in evidence):
                errors.append(f"{rid}: {row['status']} without red_artifact (red-before-green)")
            if row["status"] == "verified":
                ok = any(
                    e.get("verified_by") and e.get("verified_by") != row.get("owner")
                    for e in evidence
                )
                if not ok:
                    errors.append(
                        f"{rid}: verified without an independent verified_by"
                    )
        if row.get("status") == "duplicate" and not row.get("duplicate_of"):
            errors.append(f"{rid}: duplicate without duplicate_of")
        if args.exit:
            if row.get("severity") in {"S0", "S1"} and row.get("status") in {
                "open", "reproduced", "fixing", "blocked", "needs-runtime"
            }:
                errors.append(f"{rid}: exit blocked by {row['status']} {row['severity']}")
            if row.get("status") == "verified":
                if not row.get("merge_commit"):
                    errors.append(f"{rid}: verified without merge_commit")
                if not row.get("merged_at"):
                    errors.append(f"{rid}: verified without merged_at")
        if args.paths:
            for field in ("ref", "rust"):
                for path in row.get(field) or []:
                    if not isinstance(path, str):
                        continue
                    if path.startswith(("crates/", "tools/", ".github/")):
                        base = path.split(":", 1)[0]
                        if not (ROOT / base).exists():
                            errors.append(f"{rid}: {field} path not found: {base}")
    for row in rows:
        if row.get("status") == "duplicate" and row.get("duplicate_of") not in ids:
            errors.append(f"{row.get('id')}: duplicate_of does not exist: {row.get('duplicate_of')}")
    print(f"rows={len(rows)} unique_ids={len(ids)} errors={len(errors)}")
    for err in errors:
        print(f"  - {err}")
    return 1 if errors else 0


def cmd_summary(_: argparse.Namespace) -> int:
    rows = load()
    print(f"ledger={LEDGER}")
    print(f"rows={len(rows)}")
    print("\nby status:")
    for status, count in sorted(Counter(r["status"] for r in rows).items()):
        print(f"  {status:14} {count}")
    print("\nby severity:")
    for sev, count in sorted(Counter(r["severity"] for r in rows).items()):
        print(f"  {sev:14} {count}")
    print("\nby cluster:")
    for cluster, count in sorted(Counter(r["cluster"] for r in rows).items()):
        print(f"  {cluster:14} {count}")
    ready = [r for r in rows if r["status"] in {"open", "reproduced"}]
    print(f"\nready={len(ready)} (open/reproduced)")
    for row in sorted(ready, key=lambda r: (SEV_ORDER.get(r["severity"], 9), r["id"]))[:10]:
        print(f"  {row['severity']:3} {row['id']:20} {row['title']}")
    return 0


def cmd_next(args: argparse.Namespace) -> int:
    rows = load()
    ready = [
        r for r in rows
        if r["status"] in {"open", "reproduced"}
        and (not args.cluster or r["cluster"] == args.cluster)
    ]
    ready.sort(key=lambda r: (SEV_ORDER.get(r["severity"], 9), r["id"]))
    for row in ready[: args.limit]:
        print(
            f"{row['severity']}\t{row['id']}\t{row['cluster']}\t"
            f"{row['owner']}\t{row['title']}"
        )
    return 0


def cmd_dedupe(_: argparse.Namespace) -> int:
    rows = load()
    groups: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        groups[row["fingerprint"]].append(row)
    duplicates = {fp: rs for fp, rs in groups.items() if len(rs) > 1}
    print(f"fingerprints={len(groups)} duplicate_groups={len(duplicates)}")
    for fp, rs in sorted(duplicates.items()):
        print(f"  {fp}")
        for row in rs:
            print(f"    {row['id']} ({row['status']}, {row['severity']})")
    return 0


def cmd_evidence(args: argparse.Namespace) -> int:
    rows = load()
    target = next((r for r in rows if r["id"] == args.id), None)
    if target is None:
        sys.exit(f"unknown ledger id: {args.id}")
    if not target.get("test"):
        sys.exit(f"{args.id}: refusing evidence without a test")
    if not args.command:
        sys.exit("evidence requires at least one --command")
    if not args.red_artifact:
        sys.exit("evidence requires --red-artifact (failing repro output)")
    if args.verified and not args.verified_by:
        sys.exit("--verified requires --verified-by")
    if args.verified and args.verified_by == target.get("owner"):
        sys.exit("--verified-by must differ from owner (no self-verification)")
    record = {
        "commit": args.commit,
        "branch": args.branch,
        "commands": args.command,
        "result": args.result,
        "counts": args.counts,
        "red_artifact": args.red_artifact,
        "artifacts": args.artifact,
        "verified_by": args.verified_by,
        "verified_at": now(),
    }
    target.setdefault("evidence", []).append(record)
    target["status"] = "verified" if args.verified else "fixed"
    target["updated"] = now()[:10]
    save(rows)
    print(f"{args.id}: status={target['status']} evidence={len(target['evidence'])}")
    return 0


def cmd_merge(args: argparse.Namespace) -> int:
    rows = load()
    target = next((r for r in rows if r["id"] == args.id), None)
    if target is None:
        sys.exit(f"unknown ledger id: {args.id}")
    if target.get("status") != "verified":
        sys.exit(f"{args.id}: merge requires status=verified (current {target.get('status')})")
    target["merge_commit"] = args.merge_commit
    target["merged_at"] = args.merged_at or now()
    target["updated"] = now()[:10]
    save(rows)
    print(f"{args.id}: merged_at={target['merged_at']} merge_commit={args.merge_commit}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("validate")
    p.add_argument("--paths", action="store_true", help="also check local path existence")
    p.add_argument("--exit", action="store_true", help="also enforce the exit criteria")
    p.set_defaults(func=cmd_validate)

    p = sub.add_parser("summary")
    p.set_defaults(func=cmd_summary)

    p = sub.add_parser("next")
    p.add_argument("--cluster", choices=sorted(CLUSTERS))
    p.add_argument("--limit", type=int, default=20)
    p.set_defaults(func=cmd_next)

    p = sub.add_parser("dedupe")
    p.set_defaults(func=cmd_dedupe)

    p = sub.add_parser("merge")
    p.add_argument("--id", required=True)
    p.add_argument("--merge-commit", required=True)
    p.add_argument("--merged-at", default="")
    p.set_defaults(func=cmd_merge)

    p = sub.add_parser("evidence")
    p.add_argument("--id", required=True)
    p.add_argument("--commit", required=True)
    p.add_argument("--branch", default="")
    p.add_argument("--command", action="append", default=[])
    p.add_argument("--result", default="pass")
    p.add_argument("--counts", default="")
    p.add_argument("--red-artifact", default="")
    p.add_argument("--artifact", action="append", default=[])
    p.add_argument("--verified", action="store_true")
    p.add_argument("--verified-by", default="")
    p.set_defaults(func=cmd_evidence)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
