#!/usr/bin/env python3
"""One-line rig summary from /tmp/opencode/tui-diff/report.md (m3-log style)."""
import re
import sys

flag = sys.argv[1] if len(sys.argv) > 1 else "?"
text = open("/tmp/opencode/tui-diff/report.md").read()
table = re.findall(
    r"\| ([\w-]+) \| (\d+x\d+) \| (\d) \| (\d+) \| (\d+) \| (\d+) \| (\d+) \|", text
)
for script, size, run, steps, ident, textonly, mismatch in table:
    status = "ok" if int(mismatch) == 0 else f"mm{mismatch}/{steps}"
    print(f"{flag}\t{script}\t{size}\trun{run}\t{status}")
detail = re.findall(r"### ([\w-]+) @ (\d+x\d+) run (\d+) step (\d+) \(([\w-]+)\) — MISMATCH", text)
for script, size, run, step, name in detail:
    print(f"{flag}\t{script}\t{size}\trun{run}\tstep{step}={name} MISMATCH")
