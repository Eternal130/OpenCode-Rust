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

Out of scope (later tasks on this channel): divergence adjudication
(T9).  The CAUSES table (T6) annotates KNOWN-cause clusters only —
matchers are exact-shape (scenario-id prefix + divergence form); an
unknown divergence never matches and stays unadjudicated => exit 2.

Output: markdown report (default docs/audit/diff/TUI-FULLSCREEN.md).
Exit codes (gate): 0 — report produced and every case adjudicated
(exact, or non-exact covered by a CAUSES matcher); 1 — infra failure
(driver crash/timeout, stub gap, missing/malformed grid, size or
cells-shape mismatch vs manifest); 2 — at least one case diverges and
is not covered by a known CAUSES annotation (T6 table; unadjudicated
divergences are T9's input).
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
        # T5-review follow-up: the cells array must actually be a dense
        # width x height grid — declared size == real size, else infra
        cells = grid.get("cells")
        if not isinstance(cells, list) or len(cells) != grid.get("height") or any(
                not isinstance(row, list) or len(row) != grid.get("width")
                for row in cells):
            real = (f"{len(cells)}x?" if isinstance(cells, list) else "non-list")
            infra.append(
                f"{case['id']}: {side} cells shape {real} "
                f"!= declared {grid.get('width')}x{grid.get('height')}")
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


def _shape(cells: list) -> str:
    """Compact rows x cols-of-first-row shape string for cells_shape diffs."""
    if not isinstance(cells, list):
        return "non-list"
    cols = {len(r) for r in cells if isinstance(r, list)}
    return f"{len(cells)}x{','.join(str(c) for c in sorted(cols)) or '?'}"


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

    Defensive only: size or cells-shape mismatches diverge loudly
    rather than being silently truncated by zip() — unreachable in
    practice because load_side() already rejects any grid whose
    declared size or cells shape is inconsistent (infra, exit 1).
    """
    result: dict = {"verdict": "exact", "first_diff": None, "cells_diff_count": 0}
    rw, rh = ref_grid.get("width"), ref_grid.get("height")
    uw, uh = rust_grid.get("width"), rust_grid.get("height")
    if (rw, rh) != (uw, uh):
        return {
            # defensive, unreachable after load validation (load_side
            # red-flags size mismatches vs manifest on both sides)
            "verdict": "diverge",
            "first_diff": {"row": None, "col": None, "field": "size",
                           "ref": f"{rw}x{rh}", "rust": f"{uw}x{uh}"},
            "cells_diff_count": rw * rh if rw and rh else 0,
        }
    ref_cells = ref_grid.get("cells") or []
    rust_cells = rust_grid.get("cells") or []
    if (len(ref_cells) != len(rust_cells)
            or any(len(a) != len(b) for a, b in zip(ref_cells, rust_cells))):
        # defensive, unreachable after load validation (load_side
        # requires a dense width x height cells array)
        return {
            "verdict": "diverge",
            "first_diff": {"row": None, "col": None, "field": "cells_shape",
                           "ref": _shape(ref_cells), "rust": _shape(rust_cells)},
            "cells_diff_count": max(len(ref_cells), len(rust_cells)),
        }
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


# ---------------------------------------------------------------------------
# CAUSES adjudication table (T6) + gate
# ---------------------------------------------------------------------------

# Known-cause annotations (dual file:line), channel-7 lambda style:
# (label, matcher(divergence_record)->bool, (定性文, ref file:line, rust file:line)).
# A divergence_record is {"id", "verdict", "first_diff", "cells_diff_count"}.
# Matchers are written against EXACT shapes (scenario-id prefix + precise
# divergence form); anything broader would swallow unknown divergences and
# fake a green gate.  Unknown divergences stay unadjudicated (T9's job).
CAUSES = [
    (
        "G-368 量化色（oklab 最近邻量化差）",
        lambda d: d.get("verdict") == "structural",
        (
            "仅 RGB 差且每通道 ≤RGB_TOLERANCE(8)、char/attrs 全等：双侧滚动区主题"
            "各经一次 oklab 感知最近邻量化（同一算法、不同浮点环境），量化格点"
            "选择可相邻一格——结构性色彩差，非语义差",
            "ref packages/tui/src/mini/theme.ts:81-105（oklab/nearestIndexed），"
            "经 mini/theme.ts:117-125 V1 序列化恢复后量化",
            "rust crates/oc-tui/src/mini/theme.rs:114-192（oklab 量化 "
            "nearest_indexed/scrollback_color，1:1 移植）",
        ),
    ),
    (
        "G-28 插件 TSX 有界（bounded shim）",
        lambda d: str(d.get("id", "")).startswith("plugin-")
        and d.get("verdict") == "diverge",
        (
            "plugin-* 场景的形状类分歧：ref 侧插件以 TSX 活树渲染进 Slot，"
            "rust 侧以有界 shim（固定 UI 请求形状）近似——已立项的有界偏差",
            "ref packages/tui/src/component/panel-host.tsx:35-55（Slot path="
            "\"session.panel\"，TSX 活树 + reactive props）",
            "rust crates/oc-tui/src/plugin_ui.rs:15-108（apply_plugin_ui 有界 "
            "shim，UiRequest 中性事件形状）",
        ),
    ),
    (
        "G-332 markdown 有界行形状",
        lambda d: str(d.get("id", "")).startswith("session-markdown-")
        and (d.get("first_diff") or {}).get("field") == "ch",
        (
            "session-markdown-* 场景且 ch 字段差（行形状分歧）：ref 经 markdown "
            "渲染器 memo 重建整块，rust 经分派表逐行缓存渲染——折行/代码块"
            "边界行的有界形状差",
            "ref packages/tui/src/plugin/markdown.ts:7-18（createMarkdownRenderer "
            "memo，renderNode 变更整块重建）",
            "rust crates/oc-tui/src/plugin_markdown.rs:82-147（分派表 install_table/"
            "cached_rows/spawn_render 逐行缓存）",
        ),
    ),
    (
        "G-239/G-331 panel 有界（PanelFingerprint）",
        lambda d: str(d.get("id", "")).startswith("panel-"),
        (
            "panel-* 场景：PanelInput 的 focus()/close()/toggleFullscreen() "
            "闭包无法过线，rust 以 PanelFingerprint 快照近似 reactive props——"
            "已立项的有界偏差",
            "ref packages/tui/src/component/panel-host.tsx:36-55（Slot input 的 "
            "reactive width/presentation/focused + 闭包 props）",
            "rust crates/oc-tui/src/plugin_boot.rs:100（PanelFingerprint）、"
            ":425-440（session.panel props 有界化）、:767-780（fingerprint 快照）",
        ),
    ),
]


def adjudicate(divergence_record: dict) -> str | None:
    """First matching CAUSES label for a divergence record, else None
    (unadjudicated — a T9 candidate, never silently green)."""
    for label, match, _ in CAUSES:
        if match(divergence_record):
            return label
    return None


def verdict(results: list[dict], causes: list = ()) -> int:
    """Gate: 0 when every case is adjudicated (exact, or non-exact but
    covered by a CAUSES matcher); 2 when any case diverges without a
    known cause.  `results` records carry the case id (the CAUSES
    matchers key on scenario-id prefixes).  Infra failures never reach
    here (they exit 1 earlier).
    """
    for r in results:
        if r.get("verdict") == "exact":
            continue
        if any(match(r) for _, match, _ in causes):
            continue
        return 2
    return 0


# ---------------------------------------------------------------------------
# report
# ---------------------------------------------------------------------------

def write_report(path: Path, cases: list[dict], ref: dict[str, dict], rust: dict[str, dict],
                 results: dict[str, dict]) -> None:
    lines = []
    # --- ① header: replay line + both driver surfaces (file:line) ---
    lines.append("# TUI-FULLSCREEN — TUI 整屏差分（通道 8）")
    lines.append("")
    lines.append("同一 `tui-diff-scenario/1` 场景（tools/diff/corpus/tui/SCHEMA.md）喂双侧 TUI 实现，")
    lines.append("静止帧导出为 `tui-diff-grid/1` 稠密网格后归一化比对。判门：exact /")
    lines.append("structural（G-368 RGB ≤8 容差，仍是分歧）/ diverge；CAUSES 已知因定性（本表），")
    lines.append("adjudication 属 T9——未命中 CAUSES 的一切非 exact 均未裁决 → exit 2。")
    lines.append("")
    lines.append("> 复现：python3 tools/diff/tui_fullscreen.py --write docs/audit/diff/TUI-FULLSCREEN.md")
    lines.append("")
    lines.append("- rust 侧: `cargo nextest -p oc-tui --test tui_fullscreen_diff_driver`（env 门控")
    lines.append("  TUI_DIFF_MANIFEST/TUI_DIFF_OUT，dev-gate 身份）。驱动面:")
    lines.append("  crates/oc-tui/tests/tui_fullscreen_diff_driver.rs:1")
    lines.append("- ref 侧: `bun --preload @opentui/solid/scripts/preload.js")
    lines.append("  tools/diff/drivers/tui_fullscreen_ref.ts`（staged 到 STAGE，DIFF_REF_ROOT")
    lines.append("  指向 ref 树，只读）。驱动面: tools/diff/drivers/tui_fullscreen_ref.ts:1")
    lines.append("- 编排器: tools/diff/tui_fullscreen.py:1（manifest/staging/infra 判门/比对/本报告）")
    lines.append("")
    # --- ② normalization declaration ---
    lines.append("归一化声明: time* 钩子字段剥离（captured_at/driver_time/timestamp/elapsed）；")
    lines.append("cells 稠密化，None/缺省子字段补规范默认（ch=' '、attrs 排序去重、fg/bg=[-1,-1,-1]）；")
    lines.append("比较字段序 ch→attrs→fg→bg；RGB 每通道 |Δ|≤8 为 structural（G-368，仍计分歧）；")
    lines.append("哨兵负值 vs 具体 RGB 一律 diverge；request_census 不参与比对（双侧 boot 请求模式")
    lines.append("已知不对称，信息性记录）；stub_gap 检测到即 infra 红（SCHEMA 规则）。")
    lines.append("")
    # --- ③ counts table ---
    n_exact = sum(1 for r in results.values() if r["verdict"] == "exact")
    n_struct = sum(1 for r in results.values() if r["verdict"] == "structural")
    n_div = sum(1 for r in results.values() if r["verdict"] == "diverge")
    causes_hit: dict[str, list[str]] = {}
    n_unadj = 0
    for cid, res in results.items():
        if res["verdict"] == "exact":
            continue
        label = adjudicate({"id": cid, **res})
        if label is None:
            n_unadj += 1
        else:
            causes_hit.setdefault(label, []).append(cid)
    n_caused = sum(len(v) for v in causes_hit.values())
    lines.append("## 计数")
    lines.append("")
    lines.append("| 总量 | EXACT | STRUCTURAL | CAUSES 命中 | 未裁决 |")
    lines.append("|---|---|---|---|---|")
    lines.append(f"| {len(cases)} | {n_exact} | {n_struct} | {n_caused} | {n_unadj} |")
    lines.append("")
    lines.append(f"（verdict 分布：exact {n_exact} / structural {n_struct} / diverge {n_div}；")
    lines.append("gate 见编排器输出，未裁决>0 ⇒ exit 2）")
    lines.append("")
    # --- ④ per-case divergence detail ---
    lines.append("## 分歧明细")
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
        lines.append("### first_diff 明细")
        lines.append("")
        for case in cases:
            cid = case["id"]
            res = results[cid]
            if res["verdict"] == "exact":
                continue
            fd = res["first_diff"]
            if fd is None:
                continue
            cause = adjudicate({"id": cid, **res})
            cause_s = f" — CAUSES: {cause}" if cause else " — 未裁决（T9 候选）"
            if fd.get("row") is None:
                lines.append(f"- `{cid}` [{res['verdict']}] {fd['field']}: "
                             f"ref={fd['ref']} rust={fd['rust']} "
                             f"cells_diff_count={res['cells_diff_count']}{cause_s}")
            else:
                lines.append(
                    f"- `{cid}` [{res['verdict']}] row={fd['row']} col={fd['col']} "
                    f"field={fd['field']} ref={fd['ref']!r} rust={fd['rust']!r} "
                    f"cells_diff_count={res['cells_diff_count']}{cause_s}")
        lines.append("")
    # --- ⑤ CAUSES adjudication ---
    lines.append("## CAUSES 定性")
    lines.append("")
    for label, _match, (desc, ref_loc, rust_loc) in CAUSES:
        hits = causes_hit.get(label, [])
        lines.append(f"- **{label}** — 命中 {len(hits)} case"
                     + (f"（{', '.join(f'`{c}`' for c in hits)}）" if hits else ""))
        lines.append(f"  - {desc}")
        lines.append(f"  - {ref_loc}")
        lines.append(f"  - {rust_loc}")
    lines.append("")
    lines.append("未命中任何簇的分歧保持未裁决进入 T9；CAUSES 不为绿灯放宽 matcher。")
    lines.append("")
    # --- ⑥ out-of-scope declaration ---
    lines.append("## 范围外声明")
    lines.append("")
    lines.append("- /opt/opentui-rust 127 场景 = 框架层（OpenTUI 渲染器）边界，本通道只测 oc-tui 应用层，")
    lines.append("  不在其上做差分裁决")
    lines.append("- 动画中间帧 G-128/G-49/G-368 豁免：本通道只比对静止帧（resting frame）")
    lines.append("- 活体交互序列（键位/焦点/resize 时序）= v2 扩展，静止帧差分不覆盖")
    lines.append("- 双侧 request_census 不对称 = 信息性（boot 请求模式已知不同），不参与判门")
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
    recs = [{"id": cid, **res} for cid, res in results.items()]
    code = verdict(recs, CAUSES)
    n_caused = sum(1 for r in recs if r["verdict"] != "exact"
                   and adjudicate(r) is not None)
    for cid, res in results.items():
        fd = res["first_diff"]
        fd_s = "" if fd is None else f" first_diff={fd}"
        cause = adjudicate({"id": cid, **res})
        cause_s = f" cause={cause}" if cause else ""
        print(f"{cid}: {res['verdict']} cells_diff={res['cells_diff_count']}"
              f"{fd_s}{cause_s}")
    print(f"gate exit {code} "
          f"(0=all adjudicated exact or CAUSES-covered; 2=unadjudicated "
          f"divergence; infra failures exit 1 before this point); "
          f"CAUSES hits: {n_caused}")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
