#!/usr/bin/env python3
''''exec python3 -- "$0" "$@" # '''
r"""Guard `orphan_daemon` (va6, 2026-11-23 incident): PPID=1 leaked daemons.

WHAT IT CHECKS (runtime scan, not static text)
  Walks /proc and flags processes that reparented to init while still
  running a workload that belongs to THIS repo's build/test lanes:

    rule              pattern
    ----------------  ------------------------------------------------------
    rust-serve        PPID=1 and cmdline starts with
                      /opt/opencode-rust/target/<profile>/opencode-rust
                      followed by `serve --service` — the managed-daemon
                      path a realchain test leaked (2026-11-23: 64 such
                      daemons, ~340MB each, ~22GB total).
    ref-serve         PPID=1 and cmdline contains /tmp/opencode/ref/packages
                      — the reference checkout's bun-launched server.

  EXPLICIT EXCLUSION (by design, the legitimate 2-day service):
    /root/.opencode/bin/opencode serve --service — the user's own long-
    lived service process, NOT a lane orphan. Never flagged.

  Everything else at PPID=1 (systemd children, tini-style supervisors,
  sshd sessions) is out of scope — this guard only adjudicates the two
  leaked-workload shapes above, it is not a generic reaper scan.

BASELINE vs ALLOWLIST
  Standard guardlib semantics: an orphan seen at recording time sits in
  the committed baseline; anything NEW is red. Normal state is an EMPTY
  baseline — every orphan is new by definition.

EXIT CODES (guardlib convention)
  0 — all current violations covered by baseline/allowlist (normally:
      zero orphans at all)
  1 — new violation(s) found
  2 — guard error (missing baseline)

Stdlib only. Reads /proc; signals nothing.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import guardlib  # noqa: E402

GUARD = "orphan_daemon"

TARGET_PREFIX = "/opt/opencode-rust/target/"
REF_NEEDLE = "/tmp/opencode/ref/packages"
# The legitimate user service — excluded BEFORE any rule matches.
EXCLUDED_PREFIX = "/root/.opencode/bin/opencode"


def proc_cmdline(pid_dir: Path) -> list[str]:
    try:
        raw = (pid_dir / "cmdline").read_bytes()
    except OSError:
        return []
    parts = [chunk.decode("utf-8", "replace") for chunk in raw.split(b"\0") if chunk]
    return parts


def proc_ppid(pid_dir: Path) -> int:
    try:
        for line in (pid_dir / "status").read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("PPid:"):
                return int(line.split(":", 1)[1].strip())
    except (OSError, ValueError):
        return -1
    return -1


def collect() -> tuple[list[dict], dict]:
    violations: list[dict] = []
    scanned = 0
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        scanned += 1
        if proc_ppid(entry) != 1:
            continue
        cmdline = proc_cmdline(entry)
        if not cmdline:
            continue
        joined = "\0".join(cmdline)
        if cmdline[0].startswith(EXCLUDED_PREFIX) and "serve" in cmdline:
            continue
        vid: str | None = None
        detail = ""
        if cmdline[0].startswith(TARGET_PREFIX) and cmdline[0].endswith("/opencode-rust") \
                and cmdline[1:3] == ["serve", "--service"]:
            vid = f"rust-serve:{cmdline[0]}"
            detail = f"pid={entry.name} cmdline={joined!r}"
        elif REF_NEEDLE in joined:
            vid = f"ref-serve:{cmdline[0]}"
            detail = f"pid={entry.name} cmdline={joined[:200]!r}"
        if vid is not None:
            violations.append({"id": vid, "detail": detail})
    meta = {"scanned": scanned, "excluded": EXCLUDED_PREFIX}
    return violations, meta


if __name__ == "__main__":
    sys.exit(guardlib.run_guard(GUARD, collect, __doc__))
