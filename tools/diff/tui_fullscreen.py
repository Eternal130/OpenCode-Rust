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

Out of scope (later tasks on this channel): the cell-grid MATCH /
DIVERGE verdict gate and exit 2 semantics (T5), known-cause CAUSES
annotations (T6).

Output: markdown report (default docs/audit/diff/TUI-FULLSCREEN.md).
Exit 0 — a report was produced (T4 scope: infra-clean run of both
sides; diff verdicts arrive with T5).  Deterministic: sorted cases,
canonical JSON staging.
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
    os.chmod(stage, 0o777)
    os.chmod(scenarios_dir, 0o777)

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
    cases.sort(key=lambda c: c["id"])
    manifest = {"schema": "tui-diff-manifest/1", "cases": cases}
    (stage / "manifest.json").write_text(json.dumps(manifest, indent=1))
    return cases


# ---------------------------------------------------------------------------
# drivers
# ---------------------------------------------------------------------------

def _logged_run(cmd: list[str], log_name: str, **kw) -> subprocess.CompletedProcess:
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    res = subprocess.run(cmd, capture_output=True, text=True, **kw)
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
    res = _logged_run(["tools/dev-gate.sh", cmd], "t4-rust-driver.log", cwd=REPO)
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
        if grid.get("stub_gaps"):
            infra.append(f"{case['id']}: {side} stub_gaps {grid['stub_gaps']}")
        grids[case["id"]] = grid
    if infra:
        for line in infra:
            print(f"infra failure: {line}", file=sys.stderr)
        raise SystemExit(1)
    return grids


def run(stage: Path, ref: Path) -> tuple[list[dict], dict[str, dict], dict[str, dict]]:
    """Drive both sides over the staged manifest -> (cases, ref, rust)."""
    cases = build_manifest(stage)
    manifest = stage / "manifest.json"
    ref_out = stage / "ref-out"
    rust_out = stage / "rust-out"
    for out in (ref_out, rust_out):
        out.mkdir(exist_ok=True)
        os.chmod(out, 0o777)
    run_ref(stage, ref, manifest, ref_out)
    run_rust(manifest, rust_out)
    return cases, load_side(ref_out, cases, "ref"), load_side(rust_out, cases, "rust")


# ---------------------------------------------------------------------------
# report (T4 scope: infra-clean census; verdicts arrive with T5/T6)
# ---------------------------------------------------------------------------

def write_report(path: Path, cases: list[dict], ref: dict[str, dict], rust: dict[str, dict]) -> None:
    lines = []
    lines.append("# TUI-FULLSCREEN — TUI 整屏差分（通道 8）")
    lines.append("")
    lines.append("同一 `tui-diff-scenario/1` 场景（tools/diff/corpus/tui/SCHEMA.md）喂双侧 TUI 实现，")
    lines.append("静止帧导出为 `tui-diff-grid/1` 稠密网格后比对。本报告为 T4 编排器产出：")
    lines.append("双侧驱动 census 与 infra 状态；逐格 MATCH/DIVERGE 判门与 CAUSES 定性由 T5/T6 补齐。")
    lines.append("")
    lines.append(f"## 总量: {len(cases)} 个 case（主档 + 镜像 120x30 档）")
    lines.append("")
    lines.append("| case | ref cells | rust cells | ref gaps | rust gaps | ref paths | rust paths |")
    lines.append("|---|---|---|---|---|---|---|")
    for case in cases:
        cid = case["id"]
        r, u = ref[cid], rust[cid]
        lines.append(
            f"| {cid} | {r['width']}x{r['height']} | {u['width']}x{u['height']} "
            f"| {len(r['stub_gaps'])} | {len(u['stub_gaps'])} "
            f"| {len(r['request_census'])} | {len(u['request_census'])} |"
        )
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

    cases, ref_grids, rust_grids = run(STAGE, ref)
    write_report(Path(args.write), cases, ref_grids, rust_grids)
    print(f"infra-clean: {len(cases)} cases on both sides (exit 0 = report produced)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
