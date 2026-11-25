#!/usr/bin/env python3
"""G-28 (2026-11-25): stale test-binary origin guard.

Background (the 2026-11-24 worktree-binary pitfall, 2026-11-25 recurrence):
a lane compiles test binaries inside its worktree; after the merge the content
is identical so cargo judges them fresh and never recompiles; the worktree is
then removed and every env!("CARGO_MANIFEST_DIR")-relative fixture read dies
with ENOENT (87-test cascade on the 2026-11-25 post-merge gate). The manual
protocol step ("confirm the compile origin before running realchain from
main") is forgettable — this guard automates it.

Rule: any executable under target/debug (and target/debug/deps) that embeds a
FUNCTIONAL `.worktrees/<name>/...` path (name followed by a slash — the
fixture/manifest path shapes that ENOENT at runtime) whose worktree no longer
exists is a violation. Trailing-slash discriminator (2026-11-25): main-compiled
binaries may carry `.worktrees/<name>` DWARF comp_dir relics WITHOUT a trailing
slash (string-table separators) — those pass; a green post-merge gate is the
functional proof. Binaries embedding worktree paths that still resolve (e.g. a
live lane) pass.

Usage: python3 tools/guards/test_binary_origins.py [--target-dir DIR]
Exit 0 = clean; 1 = violations; 2 = usage error. Stdlib only.
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

DEFAULT_TARGET = Path(__file__).resolve().parents[2] / "target"
PATTERN = re.compile(rb"\.worktrees/([A-Za-z0-9._-]+)/")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def main() -> int:
    target_dir = DEFAULT_TARGET
    if len(sys.argv) == 3 and sys.argv[1] == "--target-dir":
        target_dir = Path(sys.argv[2])
    elif len(sys.argv) != 1:
        print(__doc__)
        return 2

    debug = target_dir / "debug"
    if not debug.is_dir():
        print(f"SUMMARY test_binary_origins total=0 (no target/debug at {debug})")
        return 0

    root = repo_root()
    candidates: list[Path] = []
    deps = debug / "deps"
    if deps.is_dir():
        candidates.extend(deps.iterdir())
    candidates.extend(debug.iterdir())

    violations: list[tuple[Path, str]] = []
    scanned = 0
    for path in candidates:
        try:
            if not path.is_file() or not os.access(path, os.X_OK):
                continue
            size = path.stat().st_size
            if size < 1024 or size > 2_000_000_000:
                continue
            blob = path.read_bytes()
        except OSError:
            continue
        scanned += 1
        for match in set(PATTERN.findall(blob)):
            name = match.decode("utf-8", "replace")
            if not (root / ".worktrees" / name).is_dir():
                violations.append((path, name))

    print(f"SUMMARY test_binary_origins scanned={scanned} total={len(violations)}")
    for path, name in sorted(violations):
        print(f"  VIOLATION {path.name} embeds removed .worktrees/{name}")
    if violations:
        print("Fix: cargo clean -p <affected crates> then rerun the gate from main.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
