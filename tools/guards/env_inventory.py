#!/usr/bin/env python3
r"""Guard `env_inventory` (§9.1): reference `OPENCODE_*` env read no Rust claim.

WHAT IT CHECKS
  Every `OPENCODE_*` environment variable the v2 reference READS
  (`process.env.OPENCODE_*` under `/opt/opencode-ref/packages/**`) must
  have a claim point in the Rust port (the variable name appearing in
  `crates/**/*.rs`). A violation is a ref env read with no Rust claim —
  the env-parity gap the ledger's CONFIG entries track.

CANONICAL LIST + SNAPSHOT
  * The canonical list is produced by a READ-ONLY scan of
    `/opt/opencode-ref/packages` (only `process.env.OPENCODE_*` reads;
    bare `OPENCODE_*` string constants such as icon/favicon names are
    NOT env reads and are ignored).
  * CI has no reference checkout, so the guard falls back to the
    committed snapshot `tools/guards/snapshots/env_ref.txt`. When the
    reference IS present, the live scan is used (it is fast, ~50 ms) —
    so local runs surface new ref env reads immediately while CI pins
    the last refreshed snapshot.
  * `--refresh` re-scans the reference and rewrites the snapshot (this
    is the only mode that writes, and it requires the ref checkout).
    Refreshing pulls new ref vars into the violation set — those show up
    as NEW until baselined with justification.

KNOWN LIMITATIONS
  * Ref-side effect-style / indirect env access that does not literally
    spell `process.env.OPENCODE_X` is invisible to the scan.
  * A Rust "claim point" is any appearance of the name in a .rs file —
    including tests or unrelated string literals. Conservative: can
    under-report the gap, never invent one.

UPDATE BASELINE (`--update-baseline`) when env vars are ported (shrink)
or after a deliberate `--refresh` accepting new ref vars as debt (grow —
one ledger CONFIG entry per var, or one covering entry; README § Updating).
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

REF_ROOT = Path("/opt/opencode-ref")
PACKAGES_DIR = REF_ROOT / "packages"
SNAPSHOT = Path(__file__).resolve().parent / "snapshots" / "env_ref.txt"

REF_ENV_RE = re.compile(r"process\.env\.(OPENCODE_[A-Z_0-9]+)")
RUST_ENV_RE = re.compile(r"\b(OPENCODE_[A-Z_0-9]+)\b")
REF_EXTS = {".ts", ".tsx", ".mts", ".cts", ".js"}


def scan_ref() -> list[str]:
    if not PACKAGES_DIR.is_dir():
        raise guardlib.GuardError(
            f"reference checkout not readable: {PACKAGES_DIR} (env_inventory needs it for --refresh)"
        )
    found: set[str] = set()
    for path in sorted(PACKAGES_DIR.rglob("*")):
        if not path.is_file() or path.suffix not in REF_EXTS:
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        found.update(REF_ENV_RE.findall(text))
    if not found:
        raise guardlib.GuardError("reference scan produced zero env reads — scanner broken?")
    return sorted(found)


def load_snapshot() -> list[str]:
    if not SNAPSHOT.is_file():
        raise guardlib.GuardError(f"env snapshot missing: {SNAPSHOT} (run --refresh on a host with the ref)")
    lines = [line.strip() for line in guardlib.read_text(SNAPSHOT).splitlines()]
    vars_ = [line for line in lines if line and not line.startswith("#")]
    if not vars_:
        raise guardlib.GuardError(f"env snapshot empty: {SNAPSHOT}")
    return vars_


def canonical_list() -> tuple[list[str], str]:
    if PACKAGES_DIR.is_dir():
        return scan_ref(), "reference-scan"
    return load_snapshot(), "committed-snapshot"


def collect() -> tuple[list[dict], dict]:
    ref_vars, source = canonical_list()

    claims: set[str] = set()
    rust_files = 0
    for _, path in guardlib.rust_files(guardlib.ROOT / "crates"):
        rust_files += 1
        claims.update(RUST_ENV_RE.findall(guardlib.read_text(path)))

    violations: list[dict] = []
    for var in ref_vars:
        if var not in claims:
            violations.append({
                "id": f"env:{var}",
                "detail": f"read by the reference ({source}) but no OPENCODE_* claim point in crates/**.rs",
            })

    meta = {
        "list_source": source,
        "ref_vars": len(ref_vars),
        "rust_claimed_vars": len(claims & set(ref_vars)),
        "rust_files_scanned": rust_files,
    }
    return violations, meta


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--update-baseline", action="store_true",
                        help="rewrite tools/guards/baseline/env_inventory.json with the current set")
    parser.add_argument("--refresh", action="store_true",
                        help="re-scan /opt/opencode-ref (READ-ONLY) and rewrite snapshots/env_ref.txt")
    parser.add_argument("--format", choices=("text", "json"), default="text")
    args = parser.parse_args()

    if args.refresh:
        try:
            vars_ = scan_ref()
        except guardlib.GuardError as error:
            print(f"env_inventory: GUARD-ERROR: {error}", file=sys.stderr)
            return 2
        SNAPSHOT.parent.mkdir(parents=True, exist_ok=True)
        SNAPSHOT.write_text(
            "# OPENCODE_* env reads in /opt/opencode-ref/packages (process.env.* only).\n"
            "# Regenerate: python3 tools/guards/env_inventory.py --refresh\n"
            + "\n".join(vars_) + "\n",
            encoding="utf-8",
        )
        print(f"env_inventory: snapshot refreshed: {SNAPSHOT} ({len(vars_)} vars)")
        # guardlib.run_guard re-parses argv with the common flags only.
        sys.argv = [argv for argv in sys.argv if argv != "--refresh"]

    return guardlib.run_guard("env_inventory", collect, __doc__)


if __name__ == "__main__":
    sys.exit(main())
