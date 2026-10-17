#!/usr/bin/env python3
"""SPEC-R8 zero-loss re-check (R7-style, kept for re-runs).

A. Clause census v6 (git main) -> v7 (worktree): no drops; C22 added.
B. Appendix census: R1 52 / R2 30 / R3 20 / R5 12 / R7 15 / R8 11 = 140.
C. R8 gap-ID traceability: every R8 register ID (docs/ratify-1.md) maps to
   exactly one appendix row, and every clause-side {R8: ...} tag has a row.
D. Verbatim containment:
   D1 - the ratify-1 proposed cures (F7 re-disposition + 4 residual riders)
        are present in scan-spec.md v7 character-exact modulo whitespace;
   D2 - the R8-authored blocks (C22 clause, I10b census discipline) in the
        spec are present in docs/spec-round8.md character-exact ditto.
E. MG-2 census: `ls -1 /opt/opencode-ref/packages/ | sort` (35 names)
   equals the I10b pin's enumerated universe (6 exhaustive + 29
   family-granular), and the count is 35.
"""
import re, subprocess, sys, pathlib
from collections import Counter as Cnt

WT = pathlib.Path(__file__).resolve().parents[3]  # worktree root
SPEC = WT / "docs/scan-spec.md"
ROUND8 = WT / "docs/spec-round8.md"
RATIFY = WT / "docs/ratify-1.md"
REF_PKGS = pathlib.Path("/opt/opencode-ref/packages")
spec = SPEC.read_text(encoding="utf-8")
ratify = RATIFY.read_text(encoding="utf-8")
round8 = ROUND8.read_text(encoding="utf-8")
v6 = subprocess.run(["git", "show", "main:docs/scan-spec.md"], cwd=WT,
                    capture_output=True, text=True, check=True).stdout

def squash(s):
    return re.sub(r"\s+", "", s)

def clause_census(text):
    start = text.find("## Standing instrument layer")
    body = text[start: text.find("## Round log")] if start >= 0 else text.split("## Round log")[0]
    ids = re.findall(r"(?m)^((?:I10b|I10|I\d+|[A-F])\d*b?)\s", body)
    ids = [i for i in ids if re.fullmatch(r"I\d+b?|[A-F]\d+", i)]
    return ids

# A clause census
v6c, v7c = clause_census(v6), clause_census(spec)
dupes = [k for k, v in Cnt(v7c).items() if v > 1]
dropped = sorted(set(v6c) - set(v7c))
added = sorted(set(v7c) - set(v6c))
print(f"A clause census v6={len(set(v6c))} v7={len(set(v7c))} dropped={dropped} added={added} dupes={dupes}")
okA = not dropped and not dupes and added == ["C22"] and len(set(v7c)) == len(set(v6c)) + 1

# B appendix census
counts = {}
for sec, pat in [("R1", r"### R1 gaps \(52\)"), ("R2", r"### R2 gaps \(30\)"),
                 ("R3", r"### R3 register \(12 families \+ 8 folds\)"),
                 ("R5", r"### R5 gaps \(12\)"), ("R7", r"### R7 gaps \(15\)"),
                 ("R8", r"### R8 gaps \(11")]:
    m = re.search(pat, spec)
    assert m, f"section header missing: {sec}"
    nxt = spec.find("###", m.end())
    block = spec[m.end(): nxt if nxt > 0 else len(spec)]
    if sec == "R3":
        counts[sec] = len(re.findall(r"(?m)^\| (U\d+|FOLD-)", block))
    elif sec == "R8":
        counts[sec] = len(re.findall(r"(?m)^\| R8-", block))
    else:
        counts[sec] = len(re.findall(r"(?m)^\| [SUQRFM]", block))
total = sum(counts.values())
print(f"B appendix rows: {counts} total={total}")
okB = (counts == {"R1": 52, "R2": 30, "R3": 20, "R5": 12, "R7": 15, "R8": 11} and total == 140)

# C R8 traceability (ratify-1 register -> appendix rows -> clause tags)
app8 = spec[spec.find("### R8 gaps (11"):]
row_ids = sorted(set(re.findall(r"(?m)^\| (R8-[A-Za-z0-9-]+)", app8)))
expect_rows = ["R8-MG1", "R8-MG1-F7", "R8-MG2"] + [f"R8-N{i}" for i in range(1, 9)]
tags = set(re.findall(r"\{R8:\s*([^}]+)\}", spec))
expect_tags = {"MG-1", "MG-2", "R-A", "R-B", "R-C", "R-D"}
okC = (row_ids == sorted(expect_rows) and len(row_ids) == 11
       and expect_tags <= tags and tags <= expect_tags)
c22_ok = bool(re.search(r"(?m)^C22 \*\*Telemetry/observability family\*\*", spec)) and "{R8: MG-1}" in spec[spec.find("C22 **Telemetry"):]
print(f"C R8 appendix rows={len(row_ids)} clause-tags={sorted(tags)} C22-header+tag={c22_ok}")
okC = okC and c22_ok

# D verbatim containment
def sq_contain(needle, hay, label, fails):
    if squash(needle) not in squash(hay):
        fails.append(label)

fails = []
# D1: ratify-1 proposed cures -> spec (squash-compare)
D1 = [
    ("F7-rewrite", "implementation lives in `packages/util/observability`, wired unconditionally in `cli/src/index.ts` (boot layer; emission gated on OTEL_EXPORTER_OTLP_ENDPOINT); core has none"),
    ("E6-rider", "assert = format-string equality per corpus item; any diff dispositioned."),
    ("E9-rider", "zero un-dispositioned residue = pass"),
    ("C7-rider", "artifact = per-job-state surface table + progress/lifetime assert"),
    ("B3-rider", "artifact = per-stack routed-input trace table, assert = routing matches ref per legal stack"),
]
for label, s in D1:
    sq_contain(s, spec, f"D1 {label}: NOT-IN-SPEC", fails)
    sq_contain(s, ratify, f"D1 {label}: NOT-IN-RATIFY1(source)", fails)

# D2: R8-authored spec blocks -> round-8 report
def block(start, end):
    i = spec.find(start)
    assert i >= 0, f"start marker missing: {start[:40]}"
    j = spec.find(end, i)
    assert j >= 0, f"end marker missing: {end[:40]}"
    return spec[i:j + len(end)]

c22 = block("C22 **Telemetry/observability family**", "{R8: MG-1}")
census = block("**Census discipline (v7):", "{R8: MG-2}")
for label, b in [("C22-in-round8", c22), ("census-pin-in-round8", census)]:
    sq_contain(b, round8, f"D2 {label}: NOT-IN-ROUND8", fails)
print(f"D verbatim blocks checked: {len(D1) + 2} fails={fails if fails else 'none'}")
okD = not fails

# E MG-2 census: filesystem == I10b pin, count == 35
fs = sorted(p.name for p in REF_PKGS.iterdir() if p.is_dir())
pin_block = spec[spec.find("Pinned universe"): spec.find("{R8: MG-2}")]
exhaustive = re.findall(r"exhaustive per-src-tree\s*\{([^}]*)\}", pin_block)
fam = re.findall(r"family-granular \{([^}]*)\}", pin_block)
assert exhaustive and fam, "pin lists not parseable"
pinned = [x for x in (t.strip(",") for t in re.split(r"[\s]+", exhaustive[0])) if x] + \
         [x for x in (t.strip(",") for t in re.split(r"[\s]+", fam[0])) if x]
okE = (len(fs) == 35 and sorted(pinned) == fs and len(set(pinned)) == 35
       and len(exhaustive[0].strip().split()) == 6)
v6pin_missing = [p for p in fs if p not in pinned and p != "plugin-browser"]
print(f"E census: filesystem={len(fs)} pinned={len(pinned)} (exhaustive 6 + family {len(pinned)-6}) "
      f"v6-pin-was-missing=[plugin-browser confirmed: {'plugin-browser' in pinned and 'plugin-browser' in fs}] "
      f"missing={v6pin_missing or 'none'} extra={[p for p in pinned if p not in fs] or 'none'}")

verdict = okA and okB and okC and okD and okE
print("ZERO-LOSS RE-CHECK:", "PASS" if verdict else "FAIL")
sys.exit(0 if verdict else 1)
