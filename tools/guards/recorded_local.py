#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `recorded_local` (§9.1, diff-ext C2): the local real-session
corpus must never enter git.

WHAT IT CHECKS (the privacy iron law, user ruling 2026-11-15):
  1. `.gitignore` carries the recorded-local ignore rules (the dir
     contents) with the README re-included — a removed rule would let
     future recordings show up as untracked-addable files.
  2. `git ls-files` tracks NOTHING under
     `tools/diff/corpus/tui/recorded-local/` except the README itself.
  3. The index/staging area holds no recorded-local blob (defense
     against a `git add -f` that was committed yet).

Exit codes: 0 clean / 1 violation / 2 guard error.
Stdlib only (the §9.1 guard convention).
"""

import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
DIR = "tools/diff/corpus/tui/recorded-local"

REQUIRED_RULES = (
    "tools/diff/corpus/tui/recorded-local/*",
    "!tools/diff/corpus/tui/recorded-local/README.md",
)


def main() -> int:
    problems: list[str] = []

    gitignore = (REPO / ".gitignore").read_text()
    lines = {line.strip() for line in gitignore.splitlines()}
    for rule in REQUIRED_RULES:
        if rule not in lines:
            problems.append(f".gitignore missing rule: {rule}")

    tracked = subprocess.run(
        ["git", "-c", "core.quotepath=false", "ls-files", DIR],
        cwd=REPO, capture_output=True, text=True, check=False)
    if tracked.returncode != 0:
        print(f"guard error: git ls-files failed: {tracked.stderr.strip()}")
        return 2
    for path in tracked.stdout.split():
        if path != f"{DIR}/README.md":
            problems.append(f"tracked recorded-local content: {path}")

    if problems:
        for line in problems:
            print(f"recorded_local: VIOLATION: {line}")
        return 1
    print("recorded_local: ok (no recorded content tracked; ignore rules present)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
