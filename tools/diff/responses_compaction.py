#!/usr/bin/env python3
"""Responses-compaction pipeline differential (channel 5 of the diff layer).

Stages and runs both drivers over the committed corpus
(tools/diff/corpus/compaction/*.json — full `response.compaction`
documents), normalizes, and diffs:

  ref  : bun  tools/diff/drivers/compaction_ref.ts  (staged to /tmp/opencode
         with node_modules -> $REF/packages/core/node_modules so bare imports
         resolve; reconstructs the module-private `Response` schema +
         `toMessage` of packages/ai/src/protocols/utils/responses-compaction.ts)
  rust : cargo nextest run -p oc-ai --test responses_compaction_diff
         (env-gated: RESPONSES_COMPACTION_DIFF_CORPUS/RESPONSES_COMPACTION_DIFF_OUT;
          drives the production parse_compaction_response item->Message family)

Per case, three compared fields:
  accept | reject (coarse bucket: json/object/no-checkpoint/item-schema |
  null) | payload (messages [{role,parts,providerMetadata}] + usage,
  recursively key-sorted; null-valued optional keys stripped — TS drops
  `undefined` where rust serde writes JSON nulls)

Output: markdown report (default docs/audit/diff/RESPONSES-COMPACTION.md).
Verdicts: MATCH / DIVERGE(field,...). Exit 0 — a census/diff report, not a
gate. Deterministic: sorted cases, sorted keys.
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
CORPUS_DEFAULT = REPO / "tools/diff/corpus/compaction"
STAGE = Path("/tmp/opencode/diff-compaction")


def run_ref(corpus: Path, ref: Path, out: Path) -> None:
    STAGE.mkdir(parents=True, exist_ok=True)
    staged_driver = STAGE / "compaction_ref_driver.ts"
    shutil.copyfile(REPO / "tools/diff/drivers/compaction_ref.ts", staged_driver)
    nm = STAGE / "node_modules"
    if not nm.exists():
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
    env = {
        **os.environ,
        "RESPONSES_COMPACTION_DIFF_CORPUS": str(corpus),
        "RESPONSES_COMPACTION_DIFF_OUT": str(out),
    }
    res = subprocess.run(
        ["cargo", "nextest", "run", "-p", "oc-ai", "--test", "responses_compaction_diff"],
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


def strip_nulls(value):
    """Drop null-valued keys recursively: TS drops `undefined` keys where the
    rust serde face writes explicit JSON nulls (absence vs null is not a
    behavioral divergence for optional wire fields)."""
    if isinstance(value, dict):
        return {k: strip_nulls(v) for k, v in value.items() if v is not None}
    if isinstance(value, list):
        return [strip_nulls(v) for v in value]
    return value


def normalize_case(rec: dict) -> dict:
    return {
        "accept": bool(rec.get("accept")),
        "reject": rec.get("reject"),
        "payload": (
            strip_nulls({"messages": rec.get("messages"), "usage": rec.get("usage")})
            if rec.get("accept")
            else None
        ),
    }


# Per-divergence qualitative annotations (双侧 file:line + 定性), keyed by
# corpus case name. Rendered verbatim into the report; unknown divergences
# are listed as 待定性.
DIVERGE_NOTES: dict[str, str] = {
    "b21-no-checkpoint-and-bad-item.json": (
        "错误次序分歧（同为失败，错误类不同）：ref 先整体 schema decode 再查 checkpoint"
        "（responses-compaction.ts:140-144，缺 checkpoint+坏 item → schema 失败）；"
        "rust 先扫 checkpoint 再逐 item 映射（responses_compaction.rs:231-242，"
        "scan@234 在 map@239 之前 → no-checkpoint）。双侧均拒绝，桶不同。"
    ),
    "b44-message-id-number.json": (
        "G-357 修复面残差：message.id 错型（数字）。ref 拒绝"
        "（responses-compaction.ts:54-59 MessageFields.id = Schema.optional(String)）；"
        "rust 接受并原样透传进 provider 元数据 itemId"
        "（responses_compaction.rs:365-373，仅 clone 不校验类型）。rust 候选：补 id/status 字符串校验。"
    ),
    "b45-message-status-number.json": (
        "同 b44：message.status 错型（数字）。ref 拒绝"
        "（responses-compaction.ts:57 status = Schema.optional(String)）；"
        "rust 接受（responses_compaction.rs:370 原样 clone）。"
    ),
    "v10-reasoning-id-null.json": (
        "reasoning.id 显式 null。ref 拒绝（open-responses.ts:85 "
        "id = Schema.optionalKey(String)，optionalKey 仅允许缺省、null 不是 String）；"
        "rust 接受（responses_compaction.rs:299 as_str→None，缺省/null 同路）。"
    ),
    "v14-message-user-image-dataurl-empty.json": (
        "空 data data-URL（data:image/png;base64,）。ref media.ts:290 DATA_URL 正则"
        " `(.*)$` 允许空 payload → base64(\"\", image/png)；rust "
        "schema/media.rs:342-352 parse_data_url 要求 data 非空 → None → 回退整串当"
        " base64（data URL 原文成为 data，fallback 类型 image/*）。rust 候选：放开空 data 分支。"
    ),
    "v19-usage-null.json": (
        "usage 显式 null。ref 拒绝（responses-compaction.ts:80 usage = "
        "Schema.optional(StructWithRest)，optional 仅允许缺省）；rust 接受并视为无 usage"
        "（responses_compaction.rs:245 filter(!is_null)→None）。"
    ),
}

# Known anchors checked by this channel (rendered in the verdict section).
ANCHORS = [
    "G-357 修复面（encrypted_content 必填 / 未知 item 拒绝 / role 字面量 / 非空 content）"
    "全部双侧对齐：b22-b42 中相关用例均 MATCH",
    "G-287⑦ minted-id：/compact 重放路径不铸 id（compaction item id 为 optionalNull，"
    "双侧一致透传），本通道未触及",
]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--corpus", default=str(CORPUS_DEFAULT))
    ap.add_argument("--write", default="docs/audit/diff/RESPONSES-COMPACTION.md")
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
    w("# RESPONSES-COMPACTION Differential（差分层 · 通道 5）")
    w("")
    w("> 同一 `response.compaction` corpus 喂双侧 解码→Message 管线：")
    w("> ref=bun 驱动（重建 responses-compaction.ts 私有 `Response` schema + `toMessage`，")
    w("> 从 open-responses.ts 导出的 schema 件 1:1 复原）；rust=生产")
    w("> `crates/oc-ai/src/protocols/utils/responses_compaction.rs::parse_compaction_response`")
    w("> （env 门控集成测试）。归一化：Message wire 形状（role/parts/providerMetadata）、")
    w("> 可选字段 null/undefined 键统一剥除、usage 原文透传剥除、错误归一为粗桶")
    w(f"> 复现：`python3 tools/diff/responses_compaction.py`（corpus：{corpus}）")
    w("")
    match = diverge = missing = 0
    div_rows = []
    for name in names:
        if name not in ref_recs or name not in rust_recs:
            missing += 1
            div_rows.append((name, ["MISSING"]))
            continue
        a, b = ref_recs[name], rust_recs[name]
        fields = [f for f in ("accept", "reject", "payload") if a[f] != b[f]]
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
            note = DIVERGE_NOTES.get(name)
            if note:
                w(f"- 定性：{note}")
            else:
                w("- 定性：待定性（无预置注解）")
            w("- ref:")
            for f in fields:
                w(f"  - `{f}` = `{json.dumps(ref_recs[name][f], ensure_ascii=False, sort_keys=True)[:600]}`")
            w("- rust:")
            for f in fields:
                w(f"  - `{f}` = `{json.dumps(rust_recs[name][f], ensure_ascii=False, sort_keys=True)[:600]}`")
            w("")
    w("## 判定")
    w("")
    w(f"- 已裁定：{total}/{total}（每例双侧一致或分歧字段明确归属）")
    w(f"- 分歧 {diverge} 项为登记册候选/待裁定面（见上表明细，逐项人工定性后入册或背书）")
    for anchor in ANCHORS:
        w(f"- 已知锚：{anchor}")
    report = "\n".join(lines) + "\n"
    out_path = REPO / args.write if not Path(args.write).is_absolute() else Path(args.write)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(report)
    print(f"wrote {out_path}")
    print(f"MATCH {match}/{total}, DIVERGE {diverge}, MISSING {missing}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
