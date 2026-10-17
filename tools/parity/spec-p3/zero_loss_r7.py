#!/usr/bin/env python3
"""SPEC-R7 zero-loss re-check (R6-style, kept for re-runs).

A. Clause census v5 (git main) -> v6 (worktree): no drops; I2b added.
B. Appendix census: R1 52 / R2 30 / R3 20 / R5 12 / R7 15 = 129.
C. R7 gap-ID traceability: every R7-Gn in the appendix maps to a clause
   that carries its tag, and vice versa.
D. Verbatim containment: each R7 block in scan-spec.md v6 is present in
   docs/spec-round7.md character-exact modulo whitespace (squash-compare).
"""
import re, subprocess, sys, pathlib

WT = pathlib.Path(__file__).resolve().parents[3]  # worktree root
SPEC = WT / "docs/scan-spec.md"
ROUND7 = WT / "docs/spec-round7.md"
spec = SPEC.read_text(encoding="utf-8")
r7doc = ROUND7.read_text(encoding="utf-8")
v5 = subprocess.run(["git", "show", "main:docs/scan-spec.md"], cwd=WT,
                    capture_output=True, text=True, check=True).stdout

def squash(s):
    return re.sub(r"\s+", "", s)

def clause_census(text):
    start = text.find("## Standing instrument layer")
    body = text[start: text.find("## Round log")] if start >= 0 else text.split("## Round log")[0]
    ids = re.findall(r"(?m)^((?:I10b|I10|I\d+|[A-F])\d*b?)\s", body)
    # only clause headers: I-layer + letter+number at line start before Round log
    ids = [i for i in ids if re.fullmatch(r"I\d+b?|[A-F]\d+", i)]
    return ids

v5c, v6c = clause_census(v5), clause_census(spec)
from collections import Counter as Cnt
dupes = [k for k, v in Cnt(v6c).items() if v > 1]
dropped = sorted(set(v5c) - set(v6c))
added = sorted(set(v6c) - set(v5c))
print(f"A clause census v5={len(set(v5c))} v6={len(set(v6c))} dropped={dropped} added={added} dupes={dupes}")
okA = not dropped and not dupes and added == ["I2b"] and len(set(v6c)) == len(set(v5c)) + 1

# B appendix census
counts = {}
for sec, pat in [("R1", r"### R1 gaps \(52\)"), ("R2", r"### R2 gaps \(30\)"),
                 ("R3", r"### R3 register \(12 families \+ 8 folds\)"),
                 ("R5", r"### R5 gaps \(12\)"), ("R7", r"### R7 gaps \(15\)")]:
    m = re.search(pat, spec)
    assert m, f"section header missing: {sec}"
    nxt = spec.find("###", m.end())
    block = spec[m.end(): nxt if nxt > 0 else len(spec)]
    counts[sec] = len(re.findall(r"(?m)^\| [SUQRFM]", block))
# R3 rows use U/FOLD prefixes; count them too
m = re.search(r"### R3 register", spec); nxt = spec.find("###", m.end())
counts["R3"] = len(re.findall(r"(?m)^\| (U\d+|FOLD-)", spec[m.end():nxt]))
total = sum(counts.values())
print(f"B appendix rows: {counts} total={total}")
okB = (counts == {"R1": 52, "R2": 30, "R3": 20, "R5": 12, "R7": 15} and total == 129)

# C R7 traceability
app = spec[spec.find("### R7 gaps (15)"):]
app_ids = sorted(set(re.findall(r"R7-G\d+", app)), key=lambda x: int(x.split("G")[1]))
tags = re.findall(r"R7:\s*(?:G\d+[ ,]*)+", spec)
tagged = sorted(set(int(g) for t in tags for g in re.findall(r"G(\d+)", t)))
expect = [f"R7-G{i}" for i in range(1, 16)]
okC = app_ids == expect and tagged == list(range(1, 16))
print(f"C R7 appendix IDs={len(app_ids)} clause-tagged Gs={len(tagged)} (expect 15/15; app-covered={app_ids == expect})")

# D verbatim containment: (label, spec block start marker, block-end marker)
BLOCKS = [
    ("I2b", "I2b **Plugin-era ground-truth binary**:", "{R7: G1, G2;"),
    ("I5r", "**R7 extension (v6):**", "{R7: G8}"),
    ("I6r", "**R7 standing rule (v6) —", "{R7: G7}"),
    ("I8r", "**R7 canary-coverage matrix (v6),\n   maintained per round**", "{R7: G9}"),
    ("B6r", "per B6\n   cell the runner emits", "{R7: G3}"),
    ("B7r", "env-cell runner = boot+smoke under", "{R7: G4}"),
    ("B8r", "tool = committed corpus runner", "{R7: G5}"),
    ("B9r", "per B9 cell {tool, threshold,", "{R7: G6}"),
    ("E2r", "**R7 legs amendment (v6):**", "{R7: G15}"),
    ("A2-rewrite", "Geometry (padding/gap/centering/truncation/wrap).", "{R7: G10}"),
    ("B4-rewrite", "Input methods: [G] per paste form", "{R7: G11}"),
    ("C6-rewrite", "Plugin lifecycle x UI:", "R3: FOLD-5; R7: G12}"),
    ("D3-rewrite", "Plugin-host boundary:", "{R1: S3-4; R7: G13}"),
    ("D11-rewrite", "**SDK/client package**", "R7: G14}"),
]
r7sq = squash(r7doc)
fails = []
for label, start, end in BLOCKS:
    i = spec.find(start)
    if i < 0:
        fails.append(f"{label}: START-NOT-IN-SPEC"); continue
    j = spec.find(end, i)
    if j < 0:
        fails.append(f"{label}: END-NOT-IN-SPEC"); continue
    block = spec[i:j + len(end)]
    # body to compare: strip the spec-only label prefix up to the first ':'+' '
    k = block.find(":**")
    body = block[k + 3:] if k >= 0 else block
    if squash(body) not in r7sq:
        fails.append(f"{label}: BODY-NOT-IN-ROUND7")
print(f"D verbatim blocks checked: {len(BLOCKS)} fails={fails if fails else 'none'}")
okD = not fails

verdict = okA and okB and okC and okD
print("ZERO-LOSS RE-CHECK:", "PASS" if verdict else "FAIL")
sys.exit(0 if verdict else 1)
