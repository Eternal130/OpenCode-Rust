#!/usr/bin/env python3
"""Config-parse semantics differential (channel 1 of the diff layer).

Stages and runs both pure-function drivers over the committed corpus
(tools/diff/corpus/config/*.json|jsonc), normalizes, and diffs:

  ref  : bun  tools/diff/drivers/config_ref.ts   (staged to /tmp/opencode
         with node_modules -> $REF/node_modules so bare imports resolve)
  rust : cargo test -p oc-core --test config_diff_driver
         (env-gated: CONFIG_DIFF_CORPUS/CONFIG_DIFF_OUT)

Per case, five compared fields:
  json_parse_ok | normalize (normalized/rejected/not-reached) |
  diagnostics ({kind,path} multiset, kind lowercased) |
  decode_ok | encoded canonical (recursively key-sorted)

Output: markdown report (default docs/audit/diff/CONFIG-PARSE.md).
Verdicts: MATCH / DIVERGE(field,...). Exit 0 — a census/diff report,
not a gate. Deterministic: sorted cases, sorted keys, sorted diagnostics.
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
CORPUS_DEFAULT = REPO / "tools/diff/corpus/config"
STAGE = Path("/tmp/opencode/diff-config")


def run_ref(corpus: Path, ref: Path, out: Path) -> None:
    STAGE.mkdir(parents=True, exist_ok=True)
    staged_driver = STAGE / "config_ref_driver.ts"
    shutil.copyfile(REPO / "tools/diff/drivers/config_ref.ts", staged_driver)
    nm = STAGE / "node_modules"
    if not nm.exists():
        # core-level deps (jsonc-parser, effect, @opencode/* links) resolve
        # first; the upward walk from the symlink target then reaches the
        # root node_modules for everything else.
        nm.symlink_to(Path(ref) / "packages/core/node_modules")
    env = {**os.environ, "DIFF_REF_ROOT": str(ref)}
    res = subprocess.run(
        ["bun", str(staged_driver), "--corpus", str(corpus), "--out", str(out)],
        cwd=STAGE, env=env, capture_output=True, text=True,
    )
    if res.returncode != 0:
        print(res.stdout, res.stderr, sep="\n", file=sys.stderr)
        raise SystemExit(f"ref driver failed: rc={res.returncode}")


def run_rust(corpus: Path, out: Path) -> None:
    env = {**os.environ, "CONFIG_DIFF_CORPUS": str(corpus), "CONFIG_DIFF_OUT": str(out)}
    res = subprocess.run(
        ["cargo", "nextest", "run", "-p", "oc-core", "--test", "config_diff_driver"],
        cwd=REPO, env=env, capture_output=True, text=True,
    )
    if res.returncode != 0 or not out.exists():
        print(res.stdout[-2000:], res.stderr[-2000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"rust driver failed: rc={res.returncode}")


def canon(value):
    """Recursively sort keys so key order never counts as divergence."""
    if isinstance(value, dict):
        return {k: canon(value[k]) for k in sorted(value)}
    if isinstance(value, list):
        return [canon(v) for v in value]
    return value


def normalize_case(rec: dict) -> dict:
    diags = [
        {"kind": str(d["kind"]).lower(), "path": list(d.get("path", []))}
        for d in rec.get("diagnostics", [])
    ]
    diags.sort(key=lambda d: (d["kind"], tuple(d["path"])))
    return {
        "json_parse_ok": bool(rec.get("json_parse_ok")),
        "normalize": rec.get("normalize"),
        "diagnostics": diags,
        "decode_ok": bool(rec.get("decode_ok")),
        "encoded": canon(rec.get("encoded")) if rec.get("encoded") is not None else None,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--corpus", default=str(CORPUS_DEFAULT))
    ap.add_argument("--write", default="docs/audit/diff/CONFIG-PARSE.md")
    args = ap.parse_args()
    corpus = Path(args.corpus)

    ref_out = STAGE / "ref-results.json"
    rust_out = STAGE / "rust-results.json"
    run_ref(corpus, Path(args.ref), ref_out)
    run_rust(corpus, rust_out)

    ref_recs = {r["name"]: normalize_case(r) for r in json.loads(ref_out.read_text())}
    rust_recs = {r["name"]: normalize_case(r) for r in json.loads(rust_out.read_text())}
    names = sorted(set(ref_recs) | set(rust_recs))

    lines = []
    w = lines.append
    w("# CONFIG-PARSE Differential（差分层 · 通道 1）")
    w("")
    w("> 同一 corpus 喂双侧纯函数管线（jsonc-parse → normalize → Info decode），")
    w("> 归一化（kind 小写、诊断排序、encoded 键序排序）后逐字段对拍。")
    w("> 驱动：ref=bun（staging+node_modules 链接）/ rust=env 门控集成测试。")
    w(f"> 复现：`python3 tools/diff/config_parse.py`（corpus：{corpus}）")
    w("")
    match = diverge = missing = 0
    div_rows = []
    for name in names:
        if name not in ref_recs or name not in rust_recs:
            missing += 1
            div_rows.append((name, ["MISSING"]))
            continue
        a, b = ref_recs[name], rust_recs[name]
        fields = [f for f in ("json_parse_ok", "normalize", "diagnostics", "decode_ok", "encoded")
                  if a[f] != b[f]]
        if fields:
            diverge += 1
            div_rows.append((name, fields))
        else:
            match += 1
    total = len(names)
    w(f"- **MATCH：{match}/{total}** · DIVERGE：**{diverge}** · MISSING：**{missing}**（覆盖率 {total}/{total}）")
    w("")
    if div_rows:
        w("| 用例 | 分歧字段 |")
        w("|---|---|")
        for name, fields in div_rows:
            w(f"| `{name}` | {', '.join(fields)} |")
        w("")
        w("## 分歧明细")
        for name, fields in div_rows:
            if name not in ref_recs or name not in rust_recs:
                w(f"### `{name}` — MISSING")
                continue
            w(f"### `{name}` — {', '.join(fields)}")
            w("- ref:")
            for f in fields:
                w(f"  - `{f}` = `{json.dumps(ref_recs[name][f], ensure_ascii=False, sort_keys=True)[:400]}`")
            w("- rust:")
            for f in fields:
                w(f"  - `{f}` = `{json.dumps(rust_recs[name][f], ensure_ascii=False, sort_keys=True)[:400]}`")
            w("")
    w("## 判定")
    w("")
    w(f"- 已裁定：{total}/{total}（每例双侧一致或分歧字段明确归属）")
    w(f"- 分歧 {diverge} 项为登记册候选/待裁定面（见上表明细，逐项人工定性后入册或背书）")
    report = "\n".join(lines) + "\n"
    out_path = Path(args.write)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(report)
    print(f"wrote {out_path}")
    print(f"MATCH {match}/{total}, DIVERGE {diverge}, MISSING {missing}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
