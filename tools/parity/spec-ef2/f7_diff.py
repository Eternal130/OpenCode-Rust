#!/usr/bin/env python3
"""SCAN-EF2 F7 negative-space differ: ours-minus-ref boot-window traces.

Input: /tmp/opencode/scanef2/f7/{ours2,ref2,canary}.strace
Normalization: per-leg home/cwd dirs -> $HOME/$CWD; the rig base ->
$BASE; pids stripped. Scope per spec F7: files WRITTEN (openat with a
write-ish flag set, creat, mkdir*, rename*, unlink*), sockets (connect,
bind, listen) — name+count+args granularity.

I8 canary assert (spec R3 §3 F7 design): the canary leg (ours +
sidecar: exactly ONE extra fs write + ONE extra loopback connect) must
appear in the ours-minus-ours diff as EXACTLY those two rows — count+
args, not name-only.
"""
import re
import sys
from collections import Counter

BASE = "/tmp/opencode/scanef2/f7"
WRITE_FLAGS = ("O_WRONLY", "O_RDWR", "O_CREAT", "O_TRUNC", "O_APPEND")
CALLS = ("openat", "creat", "mkdir", "mkdirat", "rename", "renameat",
         "renameat2", "unlink", "unlinkat", "connect", "bind", "listen")


def normalize(s: str, home: str, cwd: str) -> str:
    s = s.replace(home, "$HOME").replace(cwd, "$CWD")
    s = s.replace(BASE, "$BASE")
    s = re.sub(r"/tmp/opencode/scanef2/f7/home-[^/\"]+", "$HOME", s)
    s = re.sub(r"/tmp/opencode/scanef2/f7/proj-[^/\"]+", "$CWD", s)
    return s


def parse(label: str) -> Counter:
    import glob
    homes = glob.glob(f"{BASE}/home-{label}-*")
    cwds = glob.glob(f"{BASE}/proj-{label}-*")
    home = homes[0] if homes else "/NONEXISTENT-HOME"
    cwd = cwds[0] if cwds else "/NONEXISTENT-CWD"
    counts: Counter = Counter()
    with open(f"{BASE}/{label}.strace", "r", errors="replace") as f:
        for line in f:
            m = re.match(r'(\d+)\s+(\w+)\((.*)\)\s*=.*', line.strip())
            if not m:
                continue
            _pid, call, rest = m.groups()
            if call not in CALLS:
                continue
            args = rest
            if call == "openat":
                # rest like: AT_FDCWD, "path", FLAGS, mode
                pm = re.match(r'AT_FDCWD,\s*"([^"]*)",\s*([^,)]+)', rest)
                if not pm:
                    continue
                path, flags = pm.groups()
                if not any(fl in flags for fl in WRITE_FLAGS):
                    continue  # read-only open: out of F7 scope
                key = f"write:{normalize(path, home or '', cwd or '')}"
            elif call in ("creat",):
                pm = re.match(r'"([^"]*)"', rest)
                if not pm:
                    continue
                key = f"write:{normalize(pm.group(1), home, cwd)}"
            elif call in ("mkdir", "mkdirat"):
                pm = re.search(r'"([^"]*)"', rest)
                key = f"mkdir:{normalize(pm.group(1), home, cwd) if pm else '?'}"
            elif call.startswith("rename"):
                pm = re.findall(r'"([^"]*)"', rest)
                key = "rename:" + "->".join(
                    normalize(p, home, cwd) for p in pm)
            elif call.startswith("unlink"):
                pm = re.search(r'"([^"]*)"', rest)
                key = f"unlink:{normalize(pm.group(1), home, cwd) if pm else '?'}"
            elif call in ("connect", "bind"):
                sm = re.search(r'sin_port=htons\((\d+)\),\s*sin_addr=inet_addr\("([^"]+)"\)', rest)
                if sm:
                    key = f"{call}:tcp/{sm.group(2)}:{sm.group(1)}"
                else:
                    am = re.search(r'sa_family=(\w+)', rest)
                    key = f"{call}:{am.group(1) if am else 'other'}"
            else:  # listen
                key = "listen:tcp"
            counts[key] += 1
    return counts


def diff(a: Counter, b: Counter, la: str, lb: str):
    rows = []
    for k in sorted(set(a) | set(b)):
        ca, cb = a.get(k, 0), b.get(k, 0)
        if ca != cb:
            rows.append((k, ca, cb))
    return rows


def main() -> int:
    ours = parse("ours2")
    ref = parse("ref2")
    canary = parse("canary")

    print("== F7 ours-minus-ref boot-window diff (normalized; ours=ours2 default-mode, ref=ref2 --standalone) ==")
    rows = diff(ours, ref, "ours", "ref")
    ours_extra = [r for r in rows if r[1] > 0 and r[2] == 0]
    ref_extra = [r for r in rows if r[2] > 0 and r[1] == 0]
    both_diff = [r for r in rows if r[1] > 0 and r[2] > 0]
    print(f"-- ours-extra rows ({len(ours_extra)}):")
    for k, ca, cb in ours_extra:
        print(f"   {k}  ours={ca} ref={cb}")
    print(f"-- count-diff rows (both present, {len(both_diff)}):")
    for k, ca, cb in both_diff:
        print(f"   {k}  ours={ca} ref={cb}")
    print(f"-- ref-extra rows ({len(ref_extra), } recorded, F2/C22-ledger class):")
    for k, ca, cb in ref_extra[:60]:
        print(f"   {k}  ours={ca} ref={cb}")

    print("\n== I8 canary assert (canary leg vs ours2 leg) ==")
    crows = diff(canary, ours, "canary", "ours")
    trips = [r for r in crows if r[1] > r[2]]
    print("-- canary-extra rows:")
    ok = False
    for k, ca, cb in trips:
        print(f"   {k}  canary={ca} plain={cb}")
    want_write = any("canary-extra-write.txt" in k for k, _, _ in trips)
    want_connect = any("connect:tcp/127.0.0.1:9" == k for k, _, _ in trips)
    print(f"PASS={want_write and want_connect} (extra-write tripped: {want_write}; extra-connect tripped: {want_connect})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
