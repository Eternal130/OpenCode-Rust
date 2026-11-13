#!/usr/bin/env python3
"""TUI full-screen differential (channel 8 of the diff layer).

Corpus: `tui-diff-scenario/1` fixtures
(tools/diff/corpus/tui/scenarios/*.json, normative semantics in
tools/diff/corpus/tui/SCHEMA.md).  Each scenario is replayed through
BOTH TUI implementations and the resting frame compared as a
normalized dense cell grid (`tui-diff-grid/1`):

  ref  : bun  tools/diff/drivers/tui_fullscreen_ref.ts
         (staged to /tmp/opencode/diff-tuifull; boots the reference
          v2.0.24 TUI headless through its own createAppFixture at
          $DIFF_REF_ROOT, read-only; the solid transform preload is
          passed as an ABSOLUTE ref-tree path, so — unlike channel 7 —
          NO node_modules symlink is staged; bun 1.3.10, the tui-dump
          invocation convention).
  rust : cargo nextest -p oc-tui --test tui_fullscreen_diff_driver
         (env-gated: TUI_DIFF_MANIFEST/TUI_DIFF_OUT, driven through
          tools/dev-gate.sh; replays the scenario through the
          production ingest + render chain, no PTY).

Size tiers (SCHEMA 尺寸档位): every scenario renders at two tiers —
the primary tier (the file's own width/height, un-suffixed case id)
plus the mirrored 120x30 tier (case id `<scenario-id>@120x30`), same
fixture, only the grid size changes.

Manifest ("tui-diff-manifest/1", consumed by BOTH drivers, cases
sorted by id): {"schema","cases":[{id,scenario,width,height}]}.
Scenario paths in the manifest are absolute paths into the STAGE copy
(the rust driver runs nextest crate-local and cannot resolve a
ref-cwd-relative path; the ref driver's path.resolve handles absolute
paths identically).

Infra semantics (this task, T4): a run is an infra failure — exit 1 —
when either driver exits nonzero, when any case's `stub_gaps` is
non-empty (SCHEMA stub_gap rule: a requested-but-unstubbed endpoint is
an infra failure, never a render diff), or when a scenario is missing
its grid on either side.  `request_census` is NOT compared: the two
sides' boot request patterns are known to be asymmetric; the census is
recorded in the report for reference only.

Subprocess discipline: each driver is spawned as an ordinary child
(terminate/wait on our own child only); NO process-group signalling —
the http_full.py killpg pattern is path-specific and explicitly NOT
reusable here.  Both drivers are expected to exit on their own.

Out of scope (later tasks on this channel): known-cause CAUSES
annotations (T6) and divergence adjudication (T9).  Until the CAUSES
table exists, every non-exact/structural verdict is unadjudicated and
gates exit 2.

Output: markdown report (default docs/audit/diff/TUI-FULLSCREEN.md).
Exit codes (T5 gate): 0 — report produced and every case adjudicated
exact or structural(-tolerance) equal; 1 — infra failure (driver
crash/timeout, stub gap, missing/malformed grid, size mismatch vs
manifest); 2 — at least one case diverges beyond the structural
tolerance and is not covered by a known CAUSES annotation (T6; until
then every non-exact/structural verdict is unadjudicated => exit 2).
Deterministic: sorted cases, canonical JSON staging.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
REF_DEFAULT = Path("/opt/opencode-ref")
STAGE = Path("/tmp/opencode/diff-tuifull")
LOG_DIR = Path("/tmp/opencode/logs")

CORPUS = REPO / "tools/diff/corpus/tui/scenarios"
REF_DRIVER = REPO / "tools/diff/drivers/tui_fullscreen_ref.ts"
MIRRORED_TIER = (120, 30)

REF_PRELOAD = "packages/tui/node_modules/@opentui/solid/scripts/preload.js"

# driver timeouts (seconds): ref boots bun quickly; rust includes a
# cold `cargo nextest` compile of the oc-tui tree
REF_TIMEOUT = 600
RUST_TIMEOUT = 2400


# ---------------------------------------------------------------------------
# manifest + staging
# ---------------------------------------------------------------------------

def build_manifest(stage: Path) -> list[dict]:
    """Stage every scenario fixture into STAGE and emit the sorted
    `tui-diff-manifest/1` case list (primary tier + mirrored 120x30)."""
    if stage.exists():
        shutil.rmtree(stage)
    scenarios_dir = stage / "scenarios"
    scenarios_dir.mkdir(parents=True)
    # the rust leg runs through tools/dev-gate.sh as a different user
    # (eternal130, the gate's identity rule) — stage world-writable
    # with the sticky bit (1777, like /tmp)
    os.chmod(stage, 0o1777)
    os.chmod(scenarios_dir, 0o1777)

    cases: list[dict] = []
    for fname in sorted(os.listdir(CORPUS)):
        if not fname.endswith(".json"):
            continue
        doc = json.loads((CORPUS / fname).read_text())
        if doc.get("schema") != "tui-diff-scenario/1":
            raise SystemExit(f"bad scenario schema: {fname}")
        staged = scenarios_dir / fname
        shutil.copyfile(CORPUS / fname, staged)
        # primary tier: the fixture's own size, un-suffixed id
        cases.append({
            "id": doc["id"],
            "scenario": str(staged),
            "width": doc["width"],
            "height": doc["height"],
        })
        # mirrored tier (SCHEMA 尺寸档位): same fixture, 120x30 only
        cases.append({
            "id": f"{doc['id']}@{MIRRORED_TIER[0]}x{MIRRORED_TIER[1]}",
            "scenario": str(staged),
            "width": MIRRORED_TIER[0],
            "height": MIRRORED_TIER[1],
        })
    if not cases:
        raise SystemExit("empty corpus: no tui-diff-scenario/1 fixtures found "
                         f"under {CORPUS}")
    cases.sort(key=lambda c: c["id"])
    manifest = {"schema": "tui-diff-manifest/1", "cases": cases}
    (stage / "manifest.json").write_text(json.dumps(manifest, indent=1))
    return cases


# ---------------------------------------------------------------------------
# drivers
# ---------------------------------------------------------------------------

def _logged_run(cmd: list[str], log_name: str, timeout: int | None = None,
                side: str = "", **kw) -> subprocess.CompletedProcess:
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    try:
        res = subprocess.run(cmd, capture_output=True, text=True,
                             timeout=timeout, **kw)
    except subprocess.TimeoutExpired as exc:
        out, err = exc.stdout or "", exc.stderr or ""
        if isinstance(out, bytes):
            out = out.decode(errors="replace")
        if isinstance(err, bytes):
            err = err.decode(errors="replace")
        with open(LOG_DIR / log_name, "a") as log:
            log.write(f"$ {' '.join(cmd)}\nTIMEOUT after {timeout}s\n{out}{err}\n")
        print(out[-4000:], err[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(
            f"infra failure: {side or 'driver'} timed out after {timeout}s")
    with open(LOG_DIR / log_name, "a") as log:
        log.write(f"$ {' '.join(cmd)}\nrc={res.returncode}\n{res.stdout}{res.stderr}\n")
    return res


def run_ref(stage: Path, ref: Path, manifest: Path, out: Path) -> None:
    """Bun-side reference driver: staged copy under STAGE, DIFF_REF_ROOT
    env, absolute-path solid preload (no node_modules symlink needed)."""
    staged = stage / "tui_fullscreen_ref_driver.ts"
    shutil.copyfile(REF_DRIVER, staged)
    preload = ref / REF_PRELOAD
    env = {**os.environ, "DIFF_REF_ROOT": str(ref)}
    res = _logged_run(
        ["bun", "--preload", str(preload), str(staged),
         "--manifest", str(manifest), "--out", str(out)],
        "t4-ref-driver.log", cwd=stage, env=env,
        timeout=REF_TIMEOUT, side="ref driver",
    )
    if res.returncode != 0:
        print(res.stdout[-4000:], res.stderr[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"ref driver failed: rc={res.returncode}")


def run_rust(manifest: Path, out: Path) -> None:
    """Rust-side driver via the dev gate (single command string, env
    prefixes folded into the same string per dev-gate convention)."""
    cmd = (
        f"TUI_DIFF_MANIFEST={manifest} TUI_DIFF_OUT={out} "
        f"cargo nextest run -p oc-tui --test tui_fullscreen_diff_driver"
    )
    res = _logged_run(["tools/dev-gate.sh", cmd], "t4-rust-driver.log", cwd=REPO,
                      timeout=RUST_TIMEOUT, side="rust driver")
    if res.returncode != 0:
        print(res.stdout[-4000:], res.stderr[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"rust driver failed: rc={res.returncode}")


# ---------------------------------------------------------------------------
# result loading + infra semantics
# ---------------------------------------------------------------------------

def load_side(out: Path, cases: list[dict], side: str) -> dict[str, dict]:
    """Load one side's grids; enforce the infra rules (stub_gaps,
    missing grid) with SystemExit(1)."""
    grids: dict[str, dict] = {}
    infra: list[str] = []
    for case in cases:
        path = out / f"{case['id']}.json"
        if not path.exists():
            infra.append(f"{case['id']}: missing {side} grid")
            continue
        grid = json.loads(path.read_text())
        if grid.get("schema") != "tui-diff-grid/1":
            infra.append(f"{case['id']}: bad grid schema on {side}")
        if grid.get("error"):
            infra.append(f"{case['id']}: {side} driver error: {grid['error']}")
        stub_gaps = grid.get("stub_gaps") or []
        if stub_gaps:
            infra.append(f"{case['id']}: {side} stub_gaps {stub_gaps}")
        if grid.get("width") != case["width"] or grid.get("height") != case["height"]:
            infra.append(
                f"{case['id']}: {side} grid size {grid.get('width')}x{grid.get('height')} "
                f"!= manifest {case['width']}x{case['height']}")
        grids[case["id"]] = grid
    if infra:
        for line in infra:
            print(f"infra failure: {line}", file=sys.stderr)
        raise SystemExit(1)
    return grids


def run(stage: Path, ref: Path, cases: list[dict]) -> tuple[dict[str, dict], dict[str, dict]]:
    """Drive both sides over the staged manifest (cases from the
    already-run build_manifest) -> (ref grids, rust grids)."""
    manifest = stage / "manifest.json"
    ref_out = stage / "ref-out"
    rust_out = stage / "rust-out"
    for out in (ref_out, rust_out):
        out.mkdir(exist_ok=True)
        os.chmod(out, 0o1777)
    run_ref(stage, ref, manifest, ref_out)
    run_rust(manifest, rust_out)
    return load_side(ref_out, cases, "ref"), load_side(rust_out, cases, "rust")


# ---------------------------------------------------------------------------
# normalization + compare + gate (T5)
# ---------------------------------------------------------------------------

# keys stripped by normalize(): any time-ish field a driver might emit
# alongside the resting frame (none in the current drivers, but the hook
# exists so future instrumentation cannot perturb the diff)
TIME_FIELD_PREFIXES = ("captured_at", "driver_time", "timestamp", "elapsed")

# G-368 structural tolerance: RGB channels may differ by at most this much
RGB_TOLERANCE = 8


def normalize(grid: dict) -> dict:
    """Return a normalized copy of a `tui-diff-grid/1` payload: time
    fields stripped (hook — see TIME_FIELD_PREFIXES), cells made dense
    with canonical defaults for missing/None subfields."""
    out = {k: v for k, v in grid.items()
           if not k.startswith(TIME_FIELD_PREFIXES)}
    cells = []
    for row in grid.get("cells") or []:
        crow = []
        for cell in row:
            cell = cell or {}
            crow.append({
                "ch": cell.get("ch") or " ",
                "attrs": sorted(set(cell.get("attrs") or [])),
                "fg": list(cell.get("fg") or [-1, -1, -1]),
                "bg": list(cell.get("bg") or [-1, -1, -1]),
            })
        cells.append(crow)
    out["cells"] = cells
    return out


def _first_cell_diff(ref: dict, rust: dict) -> dict | None:
    """First differing field between two normalized cells, in a fixed
    field order (ch, attrs, fg, bg); None when equal."""
    for field in ("ch", "attrs", "fg", "bg"):
        if ref[field] != rust[field]:
            return {"field": field, "ref": ref[field], "rust": rust[field]}
    return None


def _within_tolerance(a: list, b: list) -> bool:
    """RGB closeness: every channel differs by <= RGB_TOLERANCE.
    Sentinels (negative values) never match a concrete RGB."""
    if len(a) != len(b):
        return False
    for x, y in zip(a, b):
        if x < 0 or y < 0:
            return False
        if abs(x - y) > RGB_TOLERANCE:
            return False
    return True


def compare(ref_grid: dict, rust_grid: dict) -> dict:
    """Compare two normalized grids -> verdict + first_diff + count.

    verdict:
      exact      — every cell identical
      structural — char/attrs identical everywhere, only RGB differs
                   and every differing channel is within RGB_TOLERANCE
                   (G-368; STILL a divergence, never an exemption)
      diverge    — anything else (char, attrs, sentinel-vs-RGB,
                   out-of-tolerance RGB, size mismatch)

    first_diff: {row, col, field, ref, rust} of the first differing
    cell in row-major order (None for exact).
    """
    result: dict = {"verdict": "exact", "first_diff": None, "cells_diff_count": 0}
    rw, rh = ref_grid.get("width"), ref_grid.get("height")
    uw, uh = rust_grid.get("width"), rust_grid.get("height")
    if (rw, rh) != (uw, uh):
        return {
            "verdict": "diverge",
            "first_diff": {"row": None, "col": None, "field": "size",
                           "ref": f"{rw}x{rh}", "rust": f"{uw}x{uh}"},
            "cells_diff_count": rw * rh if rw and rh else 0,
        }
    ref_cells = ref_grid.get("cells") or []
    rust_cells = rust_grid.get("cells") or []
    only_rgb = True
    diff_count = 0
    first: dict | None = None
    for r, (rrow, urow) in enumerate(zip(ref_cells, rust_cells)):
        for c, (rc, uc) in enumerate(zip(rrow, urow)):
            fd = _first_cell_diff(rc, uc)
            if fd is None:
                continue
            diff_count += 1
            if first is None:
                first = {"row": r, "col": c, **fd}
            if fd["field"] in ("ch", "attrs"):
                only_rgb = False
            elif fd["field"] in ("fg", "bg"):
                if not _within_tolerance(fd["ref"], fd["rust"]):
                    only_rgb = False
    if diff_count == 0:
        return result
    return {
        "verdict": "structural" if only_rgb else "diverge",
        "first_diff": first,
        "cells_diff_count": diff_count,
    }


def verdict(results: list[dict]) -> int:
    """Gate: 0 when every case is adjudicated (exact, or structural
    covered by the CAUSES table — which does not exist yet, T6, so
    structural counts as unadjudicated); 2 when any case is not
    adjudicated.  Infra failures never reach here (they exit 1 earlier).
    """
    for r in results:
        if r["verdict"] not in ("exact",):
            return 2
    return 0


# ---------------------------------------------------------------------------
# report
# ---------------------------------------------------------------------------

def write_report(path: Path, cases: list[dict], ref: dict[str, dict], rust: dict[str, dict],
                 results: dict[str, dict]) -> None:
    lines = []
    lines.append("# TUI-FULLSCREEN — TUI 整屏差分（通道 8）")
    lines.append("")
    lines.append("同一 `tui-diff-scenario/1` 场景（tools/diff/corpus/tui/SCHEMA.md）喂双侧 TUI 实现，")
    lines.append("静止帧导出为 `tui-diff-grid/1` 稠密网格后比对。T5 判门：exact / structural")
    lines.append("（G-368 RGB ≤8 容差，仍是分歧）/ diverge。CAUSES 已知因定性属 T6，")
    lines.append("adjudication 属 T9；当前一切非 exact 均未裁决 → exit 2。")
    lines.append("")
    n_exact = sum(1 for r in results.values() if r["verdict"] == "exact")
    n_struct = sum(1 for r in results.values() if r["verdict"] == "structural")
    n_div = sum(1 for r in results.values() if r["verdict"] == "diverge")
    lines.append(f"## 总量: {len(cases)} 个 case（主档 + 镜像 120x30 档）"
                 f"— exact {n_exact} / structural {n_struct} / diverge {n_div}")
    lines.append("")
    lines.append("| case | verdict | diff cells | first_diff | ref size | rust size | ref gaps | rust gaps | ref paths | rust paths |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|")
    for case in cases:
        cid = case["id"]
        r, u, res = ref[cid], rust[cid], results[cid]
        fd = res["first_diff"]
        fd_s = "—" if fd is None else (
            f"r{fd['row']}c{fd['col']} {fd['field']} ref={fd['ref']!r} rust={fd['rust']!r}"
            if fd.get("row") is not None else f"{fd['field']} {fd['ref']} vs {fd['rust']}")
        lines.append(
            f"| {cid} | {res['verdict']} | {res['cells_diff_count']} | {fd_s} "
            f"| {r.get('width')}x{r.get('height')} | {u.get('width')}x{u.get('height')} "
            f"| {len(r.get('stub_gaps') or [])} | {len(u.get('stub_gaps') or [])} "
            f"| {len(r.get('request_census') or {})} | {len(u.get('request_census') or {})} |"
        )
    lines.append("")
    if n_struct or n_div:
        lines.append("### first_diff 明细（未裁决分歧基线，裁决属 T9）")
        lines.append("")
        for case in cases:
            cid = case["id"]
            res = results[cid]
            if res["verdict"] == "exact":
                continue
            fd = res["first_diff"]
            if fd is None:
                continue
            if fd.get("row") is None:
                lines.append(f"- `{cid}` [{res['verdict']}] {fd['field']}: "
                             f"ref={fd['ref']} rust={fd['rust']} "
                             f"cells_diff_count={res['cells_diff_count']}")
            else:
                lines.append(
                    f"- `{cid}` [{res['verdict']}] row={fd['row']} col={fd['col']} "
                    f"field={fd['field']} ref={fd['ref']!r} rust={fd['rust']!r} "
                    f"cells_diff_count={res['cells_diff_count']}")
        lines.append("")
    lines.append("request_census 不参与比对（双侧 boot 请求模式已知不对称，仅供参考；")
    lines.append("stub_gap 规则见 SCHEMA.md，检测到即 infra 红，编排器已按此判门）。")
    lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n")
    print(f"wrote {path}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default=str(REF_DEFAULT))
    ap.add_argument("--write", default="docs/audit/diff/TUI-FULLSCREEN.md")
    ap.add_argument("--manifest-only", action="store_true")
    args = ap.parse_args()
    ref = Path(args.ref)

    cases = build_manifest(STAGE)
    manifest = STAGE / "manifest.json"
    print(f"manifest: {len(cases)} cases -> {manifest}")
    if args.manifest_only:
        return 0

    ref_grids, rust_grids = run(STAGE, ref, cases)
    results = {c["id"]: compare(normalize(ref_grids[c["id"]]),
                                normalize(rust_grids[c["id"]]))
               for c in cases}
    write_report(Path(args.write), cases, ref_grids, rust_grids, results)
    code = verdict(list(results.values()))
    for cid, res in results.items():
        fd = res["first_diff"]
        fd_s = "" if fd is None else f" first_diff={fd}"
        print(f"{cid}: {res['verdict']} cells_diff={res['cells_diff_count']}{fd_s}")
    print(f"gate exit {code} "
          f"(0=all adjudicated exact; 2=unadjudicated divergence; "
          f"infra failures exit 1 before this point)")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
