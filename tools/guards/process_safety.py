#!/usr/bin/env python3
r"""Guard `process_safety` (§9.1/§2.1): kill-by-name / bare-PGID / privilege patterns.

WHAT IT CHECKS
  A static scan of repo text files for the §2.1 red-line process-safety
  patterns — the mechanisms that once killed host services by PID reuse
  or name matching:

    rule               pattern
    -----------------  ----------------------------------------------------
    pkill              any `pkill` invocation (kill-by-name/class, ever)
    killall            any `killall` invocation
    fuser              any `fuser` invocation (e.g. `fuser -k 49374/tcp`)
    killpg             `os.killpg(...)` / `killpg(` (python/C PGID kills)
    kill-bare-pgid     `kill -9 -<PGID>` / `kill -TERM -<PGID>` / `kill -- -N`
                       (signal flag followed by a NEGATIVE pid — plain
                       `kill -9 <pid>` of a concrete pid does NOT match)
    kill-pgrep-subst   `kill ... $(pgrep ...)` / `` kill `pgrep ...` ``
    kill-by-name       pkill/killall lines targeting bun|node|cargo|nextest|
                       python|opencode|oc-server
    tools-sh-kill      any `kill` in shell scripts under `tools/**` (raw
                       `$PID`/`$!` shell kills — §2.1 wants Child handles
                       or signal_group_filtered instead)
    sudo               `sudo` anywhere outside tools/dev-gate.sh
    systemd-run        `systemd-run` anywhere outside tools/dev-gate.sh
                       (dev-gate is the sanctioned scope user, §2.1)

  Sanctioned stops (safekill.rs, procutil signal_group_filtered, Child
  handles in Rust, tools/parity/src/reference.rs) are production code in
  `crates/` and contain none of these textual patterns; they are not
  special-cased.

SCAN SCOPE
  Text files by extension (see EXTS) across the repo, skipping `.git/`,
  `.worktrees/`, `target/`, `node_modules/`, `docs/` (spec text quotes
  the forbidden list verbatim), `.slim/` (untracked, git-ignored session
  work-logs; prose may name TUI commands like the killall palette), and
  `tools/guards/**` (this guard's own baseline/allowlist/README mention
  the pattern words). Binary files (NUL byte) and files >8 MiB are
  skipped.

BASELINE vs ALLOWLIST (§2.1 legacy debt)
  The five §2.1-mandated legacy files sit in the ALLOWLIST with ledger
  IDs (HYGIENE-PKILL-LEGACY-PROBES for the four probes,
  HYGIENE-RELEASE-SMOKE-KILL for release-smoke.sh) — see
  tools/guards/allowlist/process_safety.txt. Everything else that exists
  today (false-positive prose like the `killall` shell-word list in
  oc-core's parser tests data, the `/killall` TUI label) lives in the
  baseline. A NEW occurrence anywhere fails the guard.

UPDATE BASELINE (`--update-baseline`) only with an orchestrator-approved
justification — for this guard a growing baseline is a red flag, not a
routine act (README § Updating).
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

ROOT = guardlib.ROOT
SANCTIONED_PRIVILEGE_PATH = "tools/dev-gate.sh"  # §2.1: the sanctioned sudo/systemd-run user
SKIP_DIR_NAMES = {".git", ".worktrees", "target", "node_modules"}
SKIP_REL_PREFIXES = ("docs/", "tools/guards/", ".slim/")
EXTS = {
    ".py", ".sh", ".bash", ".zsh",
    ".rs", ".ts", ".tsx", ".js", ".jsx", ".mts", ".cts",
    ".json", ".jsonl", ".toml", ".yml", ".yaml", ".service",
    ".md", ".txt", ".cfg", ".conf", ".ini", ".html", ".css", ".nix",
}
NO_EXT_NAMES = {"Makefile", "Dockerfile", "LICENSE"}
MAX_FILE_BYTES = 8 * 1024 * 1024

RULES: list[tuple[str, re.Pattern]] = [
    ("pkill", re.compile(r"\bpkill\b")),
    ("killall", re.compile(r"\bkillall\b")),
    ("fuser", re.compile(r"\bfuser\b")),
    ("killpg", re.compile(r"\bos\.killpg\b|\bkillpg\s*\(")),
    # signal flag(s) then a NEGATIVE numeric target, or `kill -- -N`:
    # `kill -0 "$PID"` (liveness probe) and `kill -9 517` must NOT match.
    ("kill-bare-pgid", re.compile(r"\bkill\s+(?:-[A-Za-z0-9]+\s+)+-\d+\b|\bkill\s+--\s+-\d+\b")),
    ("kill-pgrep-subst", re.compile(r"\bkill\b[^#\n]*(?:\$\(\s*pgrep\b|`\s*pgrep\b)")),
    ("kill-by-name", re.compile(
        r"\b(?:pkill|killall)\b[^#\n]*\b(?:bun|node|cargo|nextest|python|opencode|oc-server)\b"
    )),
]


def iter_scan_files() -> list[tuple[str, Path]]:
    out: list[tuple[str, Path]] = []
    stack = [ROOT]
    while stack:
        directory = stack.pop()
        try:
            entries = sorted(directory.iterdir(), reverse=True)
        except OSError:
            continue
        for path in entries:
            name = path.name
            if path.is_dir():
                if name in SKIP_DIR_NAMES:
                    continue
                stack.append(path)
                continue
            if path.suffix not in EXTS and name not in NO_EXT_NAMES:
                continue
            if not path.is_file():
                continue
            rel = path.relative_to(ROOT).as_posix()
            if rel.startswith(SKIP_REL_PREFIXES):
                continue
            try:
                if path.stat().st_size > MAX_FILE_BYTES:
                    continue
            except OSError:
                continue
            out.append((rel, path))
    out.sort()
    return out


def collect() -> tuple[list[dict], dict]:
    violations: list[dict] = []
    files_scanned = 0
    for rel, path in iter_scan_files():
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue  # binary or unreadable — out of scope for a text scan
        if "\x00" in text:
            continue
        files_scanned += 1
        is_tools_sh = rel.startswith("tools/") and rel.endswith(".sh")
        for lineno, line in enumerate(text.splitlines(), 1):
            for rule, pattern in RULES:
                if pattern.search(line):
                    violations.append({
                        "id": f"{rel}:{lineno}:{rule}",
                        "detail": f"{rule}: {line.strip()[:120]}",
                    })
            if is_tools_sh and re.search(r"(?<![\w./-])kill(?![\w])", line):
                violations.append({
                    "id": f"{rel}:{lineno}:tools-sh-kill",
                    "detail": f"tools-sh-kill: {line.strip()[:120]}",
                })
            if rel != SANCTIONED_PRIVILEGE_PATH:
                if re.search(r"\bsudo\b", line):
                    violations.append({"id": f"{rel}:{lineno}:sudo",
                                       "detail": f"sudo outside {SANCTIONED_PRIVILEGE_PATH}: {line.strip()[:120]}"})
                if re.search(r"\bsystemd-run\b", line):
                    violations.append({"id": f"{rel}:{lineno}:systemd-run",
                                       "detail": f"systemd-run outside {SANCTIONED_PRIVILEGE_PATH}: {line.strip()[:120]}"})

    if files_scanned == 0:
        raise guardlib.GuardError("process_safety scanned zero files — scanner broken?")

    meta = {"files_scanned": files_scanned, "rules": [r for r, _ in RULES] + [
        "tools-sh-kill", "sudo", "systemd-run"]}
    return violations, meta


if __name__ == "__main__":
    sys.exit(guardlib.run_guard("process_safety", collect, __doc__))
