#!/usr/bin/env python3
"""termatrix analyzer — verdict matrices, baseline deltas, protocol tables.

Walks /tmp/opencode/termatrix/<profile>/<script>.log (the rig's full
stdout, one invocation per script) + the per-run artifact dirs
(<profile>/<script>-80x24-rN/) and emits the analysis blocks used by
docs/termatrix-findings.md:

  1. per-step verdict matrix (profile x script x step, both runs)
  2. class-CHANGE deltas vs the baseline profile (xterm256-silent)
     — a step that flips IDENTICAL->MISMATCH under a non-baseline
     environment is the TERM-conditional signal
  3. placeholder-class tagging (registered-known): a MISMATCH whose
     only text divergence is the composer placeholder row
  4. protocol signature tables per profile (ref/our), incl. the SGR
     truecolor/256-index downgrade split
"""

import os
import re
import sys
from collections import defaultdict

ROOT = "/tmp/opencode/termatrix"
PROFILES = ["xterm256-silent", "xterm256", "xterm256-light", "truecolor", "screen256", "tmux", "kitty-yes", "kitty-no"]
BASELINE = "xterm256-silent"
SCRIPTS = ["boot-home", "type-edit-submit", "themes-nord", "mouse-hover-tabstrip", "palette"]
STEP_RE = re.compile(r"^### (\S+) @ (\d+)x(\d+) run (\d+) step (\d+) — (\S+)(?: \((.*)\))?$")

def parse_report(text):
    """report.md body → {(script, run, step): (class, label, firstdiff)}"""
    steps = {}
    lines = text.splitlines()
    for i, line in enumerate(lines):
        m = STEP_RE.match(line)
        if not m:
            continue
        script, _c, _r, run, step, klass, label = m.groups()
        first = ""
        for j in range(i, min(i + 8, len(lines))):
            fm = re.match(r"^first diff: (.*)$", lines[j])
            if fm:
                first = fm.group(1)
                break
        steps[(script, int(run), int(step))] = (klass, label or "", first)
    return steps

def sig_file(profile, script, run):
    path = os.path.join(ROOT, profile, f"{script}-80x24-r{run}", "protocol-signature.txt")
    if not os.path.exists(path):
        return None
    entries = {}
    section = None
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if line == "--- ref ---":
            section = "ref"
            continue
        if line == "--- our ---":
            section = "our"
            continue
        if "=" in line and section:
            name, _, count = line.rpartition("=")
            try:
                entries.setdefault(section, {})[name] = int(count)
            except ValueError:
                pass
    return entries

def placeholder_only(profile, script, run, step):
    """True when the step's dumps differ ONLY in rows that are the
    composer placeholder row (Ask anything… \"…\") on one side."""
    d = os.path.join(ROOT, profile, f"{script}-80x24-r{run}")
    ref = os.path.join(d, f"s{step:02d}.ref.txt")
    our = os.path.join(d, f"s{step:02d}.our.txt")
    if not (os.path.exists(ref) and os.path.exists(our)):
        return False
    r = open(ref, encoding="utf-8").read().splitlines()
    o = open(our, encoding="utf-8").read().splitlines()
    if len(r) != len(o):
        return False
    for a, b in zip(r, o):
        if a == b:
            continue
        ask = 'Ask anything… "' in a or a.strip() in ("", "┃")
        return ask and ('Ask anything… "' in b or b.strip() in ("", "┃"))
    return True

def main():
    # 1. verdicts per profile
    verdicts = {}  # profile -> {(script, run, step): (class,label,first)}
    for profile in PROFILES:
        verdicts[profile] = {}
        for script in SCRIPTS:
            path = os.path.join(ROOT, profile, f"{script}.log")
            if not os.path.exists(path):
                continue
            text = open(path, encoding="utf-8", errors="replace").read()
            verdicts[profile].update(parse_report(text))

    # 2. matrix + deltas vs baseline
    # (the report's detail section lists only MISMATCH/TEXT-ONLY steps;
    #  a step absent from the detail of its (script, run) is IDENTICAL)
    STEP_COUNTS = {"boot-home": 3, "type-edit-submit": 13, "themes-nord": 8,
                   "mouse-hover-tabstrip": 17, "palette": 6}
    print("== Step-class deltas vs baseline (non-baseline profile steps whose class CHANGED) ==")
    base = verdicts[BASELINE]
    for profile in PROFILES:
        if profile == BASELINE:
            continue
        vp = verdicts[profile]
        changes = []
        for script in SCRIPTS:
            for run in (0, 1):
                for step in range(STEP_COUNTS[script]):
                    key = (script, run, step)
                    kb = base.get(key, ("IDENTICAL",))[0]
                    kp = vp.get(key, ("IDENTICAL",))[0]
                    if kb != kp:
                        label = vp.get(key, base.get(key, ("", "")))[1]
                        changes.append(f"{script} r{run} s{step} ({label}): {kb} -> {kp}")
        if changes:
            print(f"\n-- {profile} --")
            for c in changes:
                print("  " + c)
        else:
            print(f"-- {profile}: NO step-class changes vs baseline")

    # 3. placeholder-class tagging
    print("\n== MISMATCH steps whose ONLY divergence is the placeholder row (registered-known P-1) ==")
    counts = defaultdict(int)
    total_mismatch = defaultdict(int)
    for profile in PROFILES:
        for (script, run, step), (klass, _label, _first) in verdicts[profile].items():
            if klass != "MISMATCH":
                continue
            total_mismatch[profile] += 1
            if placeholder_only(profile, script, run, step):
                counts[profile] += 1
    for profile in PROFILES:
        print(f"  {profile}: {counts[profile]}/{total_mismatch[profile]} mismatch steps are placeholder-only")

    # 4. first-diff signature census (non-baseline only, excluding placeholder-only)
    print("\n== First-diff signature census (non-baseline profiles, placeholder-only steps excluded) ==")
    sigs = defaultdict(list)
    for profile in PROFILES:
        if profile == BASELINE:
            continue
        for (script, run, step), (klass, _label, first) in sorted(verdicts[profile].items()):
            if klass != "MISMATCH":
                continue
            if placeholder_only(profile, script, run, step):
                continue
            key = (script, step)
            sigs[key].append((profile, first[:80]))
    for (script, step), hits in sorted(sigs.items()):
        profiles = sorted({p for p, _ in hits})
        first = hits[0][1]
        baseline_class = base.get((script, 0, step), ("?",))[0]
        print(f"  {script} s{step} [baseline class {baseline_class}] profiles={len(profiles)}: {first}")

    # 5. protocol signature tables
    print("\n== Protocol signatures per profile (run 0; presence-level ref vs our) ==")
    for profile in PROFILES:
        for script in ["boot-home"]:
            entries = sig_file(profile, script, 0)
            if not entries:
                continue
            ref = entries.get("ref", {})
            our = entries.get("our", {})
            names = sorted(set(ref) | set(our))
            diffs = [f"{n}: ref={ref.get(n,0)} our={our.get(n,0)}" for n in names if ref.get(n, 0) != our.get(n, 0)]
            print(f"\n-- {profile} ({script}) --")
            for d in diffs:
                print("  " + d)
            print(f"  [sgr split] ref: truecolor={ref.get('sgr-truecolor',0)} idx={ref.get('sgr-256idx',0)}"
                  f" | our: truecolor={our.get('sgr-truecolor',0)} idx={our.get('sgr-256idx',0)}")

if __name__ == "__main__":
    sys.exit(main())
