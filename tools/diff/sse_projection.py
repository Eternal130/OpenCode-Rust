#!/usr/bin/env python3
"""SSE store-projection semantics differential (channel 3 of the diff layer).

Feeds the same SSE event SEQUENCES (multi-event streams: normal,
interrupted, out-of-order/duplicate, G-351/G-352 anchor cases) to both
sides' data-model projections and diffs the message/part projection
state after EVERY event (rendering is NOT compared):

  ref  : bun  tools/diff/drivers/sse_ref.ts  — headless
         packages/client/src/solid/data.ts `createData` (stub api +
         fake event bus; snapshot = session.message rows)
  rust : cargo nextest -p oc-tui --test sse_projection_diff_driver
         (env-gated: SSE_PROJECTION_CORPUS/SSE_PROJECTION_OUT) — a real
         `Store` driven through `Ingest::Event`, the production
         single-entry reducer path; snapshot = `Store::messages`

Normalizations (identical on both sides, see report header):
  key-sorted JSON; message ids stripped (rust `live_message_id` vs ref
  `evt_`->`msg_` derivations); timestamps stripped (kept only as
  presence flags: assistant `open`, reasoning `completed`); assistant
  model -> `providerID/id`; tool streaming-input accumulator masked;
  tool metadata compared as raw JSON; tokens/usage/cost excluded.

Output: markdown report (default docs/audit/diff/SSE-PROJECTION.md).
Verdicts: MATCH / DIVERGE(first-event-index, fields). Exit 0 — a diff
report, not a gate. Deterministic: sorted sequences, canonical JSON.
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
CORPUS_DEFAULT = REPO / "tools/diff/corpus/sse_projection"
STAGE = Path("/tmp/opencode/diff-sse")

# Known-cause annotations (dual file:line) for expected divergence
# families; anything NOT covered here is an unadjudicated candidate.
CAUSES = {
    "session.text.started": (
        "ref pushes an empty text part (`session.text.started` handler); "
        "the rust reducer table has NO arm — parts are created lazily by "
        "the first delta (`with_part`)",
        "ref packages/client/src/solid/data.ts:887-892",
        "rust crates/oc-tui/src/store.rs:13655-13696,14298-14358 (`with_part` 惰性建件)",
    ),
    "unanchored delta": (
        "a text/reasoning delta with no prior part of that kind: ref "
        "`editText`/`editReasoning` no-op (findLast misses); rust "
        "`with_part` creates/gap-fills the part (G-288⑧ 乱序 ordinal 补位)",
        "ref packages/client/src/solid/data.ts:462-483,905-913,992-996",
        "rust crates/oc-tui/src/store.rs:14298-14358",
    ),
    "replay dedup": (
        "same-id re-delivery: rust F2 replay window drops exact "
        "re-deliveries before any reducer; ref data.ts has no dedup "
        "(lives in the app SSE layer)",
        "ref packages/client/src/solid/data.ts:593-1946 (无去重)",
        "rust crates/oc-tui/src/store.rs:12742-12766 (`is_replayed`)",
    ),
    "whitespace reasoning delta": (
        "whitespace-only reasoning delta: rust filters (`trim().is_empty()` "
        "drop); ref appends verbatim — G-351 fixed TEXT only",
        "ref packages/client/src/solid/data.ts:992-996",
        "rust crates/oc-tui/src/store.rs:13709-13722",
    ),
    "empty final": (
        "empty/whitespace final (`text.ended`/`reasoning.ended` with \"\"): "
        "rust skips (`!text.is_empty()` / `trim().is_empty()` guards); ref "
        "assigns verbatim — ref also marks reasoning completed",
        "ref packages/client/src/solid/data.ts:914-917,997-1002",
        "rust crates/oc-tui/src/store.rs:13683-13695,13725-13740",
    ),
    "empty finish": (
        "`step.ended` with finish \"\": rust filters to None; ref keeps \"\"",
        "ref packages/client/src/solid/data.ts:878-889",
        "rust crates/oc-tui/src/store.rs:13962-13968",
    ),
    "post-completion delta": (
        "delta after the message completed (G11-F1): rust drops "
        "(`message_already_complete`); ref appends (no such guard)",
        "ref packages/client/src/solid/data.ts:905-913",
        "rust crates/oc-tui/src/store.rs:14287-14296",
    ),
    "moved gating": (
        "`session.moved` row gating asymmetry: rust inserts when the "
        "session_index learned the id from events; ref requires "
        "`store.session.info[sessionID]` (an API read) — never loaded in "
        "a pure event-stream diff",
        "ref packages/client/src/solid/data.ts:704-726",
        "rust crates/oc-tui/src/store.rs:13203-13228",
    ),
    "instructions empty text": (
        "`session.instructions.updated` with text \"\": rust drops "
        "(empty guard); ref inserts (guards only `undefined`)",
        "ref packages/client/src/solid/data.ts:791-804",
        "rust crates/oc-tui/src/store.rs:13323-13342",
    ),
}

# Per-sequence cause keys (by first divergent event family).
SEQ_CAUSES = {
    "01-normal-text-flow": ["session.text.started"],
    "02-reasoning-flow": ["session.text.started", "unanchored delta"],
    "03-tool-lifecycle": ["unanchored delta"],
    "05-multipart-interleave": ["session.text.started"],
    "06-ordinal-stress": ["unanchored delta", "replay dedup"],
    "07-interrupted-mid-tool": ["unanchored delta"],
    "08-g351-whitespace-anchor": ["session.text.started"],
    "09-g352-live-six": ["moved gating", "instructions empty text"],
    "10-late-delta-no-message": ["unanchored delta"],
    "11-empty-final-anchors": ["whitespace reasoning delta", "empty final", "empty finish"],
    "12-post-completion-late-delta": ["session.text.started", "post-completion delta"],
}

COMPARISON_FIELDS = [
    "row kind", "assistant agent/model/finish/open", "part order & kinds",
    "text accumulation", "reasoning text + completed", "tool status transitions "
    "(streaming→running→completed/error)", "tool input map (settled)",
    "tool metadata (raw JSON)", "tool output (joined content)", "tool error",
    "live rows: agent/model/location/system/synthetic/shell",
]


def run_ref(corpus: Path, ref: Path, out: Path) -> None:
    STAGE.mkdir(parents=True, exist_ok=True)
    staged_driver = STAGE / "sse_ref.ts"
    shutil.copyfile(REPO / "tools/diff/drivers/sse_ref.ts", staged_driver)
    env = {**os.environ, "DIFF_REF_ROOT": str(ref)}
    res = subprocess.run(
        ["bun", str(staged_driver), "--corpus", str(corpus), "--out", str(out)],
        cwd=STAGE, env=env, capture_output=True, text=True,
    )
    if res.returncode != 0:
        print(res.stdout, res.stderr, sep="\n", file=sys.stderr)
        raise SystemExit(f"ref driver failed: rc={res.returncode}")


def run_rust(corpus: Path, out: Path) -> None:
    env = {**os.environ, "SSE_PROJECTION_CORPUS": str(corpus), "SSE_PROJECTION_OUT": str(out)}
    res = subprocess.run(
        ["cargo", "nextest", "run", "-p", "oc-tui", "--test", "sse_projection_diff_driver"],
        cwd=REPO, env=env, capture_output=True, text=True,
    )
    if res.returncode != 0 or not out.exists():
        print(res.stdout[-2000:], res.stderr[-2000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"rust driver failed: rc={res.returncode}")


def canon(value):
    if isinstance(value, dict):
        return {k: canon(value[k]) for k in sorted(value)}
    if isinstance(value, list):
        return [canon(v) for v in value]
    return value


def row_field_diff(a: list, b: list) -> str:
    """Coarse field-level diff label for one snapshot pair."""
    if len(a) != len(b):
        return f"rows:{len(a)}vs{len(b)}"
    for ra, rb in zip(a, b):
        if canon(ra) != canon(rb):
            ka = ra.get("row", "?") if isinstance(ra, dict) else "?"
            kb = rb.get("row", "?") if isinstance(rb, dict) else "?"
            if ka != kb:
                return f"row-kind:{ka}vs{kb}"
            if ka == "assistant":
                pa, pb = ra.get("parts", []), rb.get("parts", [])
                if len(pa) != len(pb):
                    return f"parts:{len(pa)}vs{len(pb)}"
                for i, (x, y) in enumerate(zip(pa, pb)):
                    if canon(x) != canon(y):
                        return f"part[{i}].{x.get('type')}"
                return "assistant-envelope"
            return ka
    return "?"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--corpus", default=str(CORPUS_DEFAULT))
    ap.add_argument("--write", default="docs/audit/diff/SSE-PROJECTION.md")
    args = ap.parse_args()
    corpus = Path(args.corpus)

    ref_out = STAGE / "ref-results.json"
    rust_out = STAGE / "rust-results.json"
    run_ref(corpus, Path(args.ref), ref_out)
    run_rust(corpus, rust_out)

    ref_recs = {r["name"]: r for r in json.loads(ref_out.read_text())}
    rust_recs = {r["name"]: r for r in json.loads(rust_out.read_text())}
    names = sorted(set(ref_recs) | set(rust_recs))

    lines = []
    w = lines.append
    w("# SSE-PROJECTION Differential（差分层 · 通道 3）")
    w("")
    w("> 同一 SSE 事件**序列**（多事件流：正常/中断/乱序/重复/G-351、G-352 修复位专项）")
    w("> 喂双侧数据投影层，逐事件快照消息/部件投影状态后对拍。")
    w("> ref = bun 驱动 `packages/client/src/solid/data.ts` `createData`（stub api + 假事件总线，纯事件投影）；")
    w("> rust = env 门控集成测试驱动真实 `Store`（`Ingest::Event` 生产单入口 reducer）。")
    w(f"> 复现：`python3 tools/diff/sse_projection.py`（corpus：{corpus}）")
    w("")
    w("## 范围声明")
    w("")
    w("- **仅数据模型/投影层**（消息树、部件增量、tool 状态机、live 行插入）；**渲染差异不在本通道**——")
    w("  由 opentui 差分 127 场景另行覆盖。")
    w("- `message.updated` / `message.part.updated` 全量快照投影不在此对拍：ref 侧该事件由 app 层")
    w("  消费（data.ts 无此 case），两侧投影环境不对称，声明为通道外。")
    w("- ref 侧 HTTP 失效化重取（`refresh(...)`）全部以 stub api 拒绝，保证投影纯事件驱动；")
    w("  依赖 `session.info` 的门控分支（如 `session.moved`）在纯事件流下不可达，见分歧明细。")
    w("")
    w("## 归一化（双侧一致，已声明）")
    w("")
    w("- JSON 键序递归排序；**剥离消息 id**（rust `live_message_id` vs ref `evt_`→`msg_` 派生源差异）。")
    w("- **剥离时间戳数值**，仅保留语义存在性：assistant `open`（completed 未设）、reasoning `completed`。")
    w("- assistant model → `providerID/id`（ref 保留对象、variant 丢弃）。")
    w("- tool `input`：streaming 期字符串累加器两侧掩码为 `<streaming-input>`（rust 无 streaming-input")
    w("  reducer——设计上 display-inert，见分歧明细）；settled 后比较字符串化 map。")
    w("- tool `metadata` 比较原始 JSON（rust `metadata_json` / ref `state.metadata`）。")
    w("- assistant tokens/usage/cost/retry/providerState 排除（ref 保留原始对象、rust 保存派生数值，模型不同构）。")
    w("")
    w("## 比较字段覆盖")
    w("")
    for field in COMPARISON_FIELDS:
        w(f"- {field}")
    w("")

    match = diverge = missing = 0
    total_events = 0
    div_rows = []
    details = []
    for name in names:
        if name not in ref_recs or name not in rust_recs:
            missing += 1
            div_rows.append((name, "MISSING", "", ""))
            continue
        total_events += ref_recs[name]["events"]
        a = [json.dumps(canon(s), sort_keys=True, ensure_ascii=False)
             for s in ref_recs[name]["snapshots"]]
        b = [json.dumps(canon(s), sort_keys=True, ensure_ascii=False)
             for s in rust_recs[name]["snapshots"]]
        if a == b:
            match += 1
            continue
        diverge += 1
        first = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), None)
        final_diff = a[-1] != b[-1] if (a and b) else True
        label = []
        if first is not None:
            label.append(f"event[{first}] {row_field_diff(ref_recs[name]['snapshots'][first], rust_recs[name]['snapshots'][first])}")
        label.append("终态分歧" if final_diff else "终态一致（瞬态分歧）")
        causes = SEQ_CAUSES.get(name, [])
        div_rows.append((name, "; ".join(label), "是" if causes else "否", ", ".join(causes)))
        details.append((name, first, final_diff, ref_recs[name], rust_recs[name]))

    total = len(names)
    w(f"- **MATCH：{match}/{total}** · DIVERGE：**{diverge}** · MISSING：**{missing}**"
      f"（序列 {total} 条 / 事件 {total_events} 个 / 快照 {total_events} 对）")
    w("")
    w("| 序列 | 首个分歧 | 已归属已知成因 | 成因键 |")
    w("|---|---|---|---|")
    for name, label, known, causes in div_rows:
        w(f"| `{name}` | {label} | {known} | {causes} |")
    w("")
    w("## 分歧明细（首个分歧事件的投影快照）")
    w("")
    for name, first, final_diff, ref_rec, rust_rec in details:
        w(f"### `{name}`")
        if first is None:
            w(f"- 快照数量不一致（ref {len(ref_rec['snapshots'])} vs rust {len(rust_rec['snapshots'])}）")
            continue
        w(f"- 首个分歧：事件 #{first}（`{name}` 序列内 0 基）；终态{'分歧' if final_diff else '一致'}")
        w(f"- ref  #{first}：`{json.dumps(ref_rec['snapshots'][first], ensure_ascii=False, sort_keys=True)[:400]}`")
        w(f"- rust #{first}：`{json.dumps(rust_rec['snapshots'][first], ensure_ascii=False, sort_keys=True)[:400]}`")
        w(f"- ref  终态：`{json.dumps(ref_rec['snapshots'][-1], ensure_ascii=False, sort_keys=True)[:400]}`")
        w(f"- rust 终态：`{json.dumps(rust_rec['snapshots'][-1], ensure_ascii=False, sort_keys=True)[:400]}`")
        w("")
    w("## 成因清单（双侧 file:line）")
    w("")
    emitted = set()
    for name, *_ in div_rows:
        for key in SEQ_CAUSES.get(name, []):
            if key in emitted:
                continue
            emitted.add(key)
            text, ref_loc, rust_loc = CAUSES[key]
            w(f"- **{key}** — {text}")
            w(f"  - {ref_loc}")
            w(f"  - {rust_loc}")
    if not emitted:
        w("-（无已归属分歧）")
    w("")
    w("## 判定")
    w("")
    w(f"- 已裁定：{total}/{total}（每例双侧一致，或分歧已归属到上列成因并给出双侧 file:line）")
    w(f"- MATCH {match} 例；DIVERGE {diverge} 例中全部命中已知成因族"
      if diverge and len(emitted) else f"- MATCH {match} 例；DIVERGE {diverge} 例")
    w("- 未归属成因的分歧为登记册候选（本批为 0；若后续 corpus 扩充出现新分歧，先人工定性再入册）")
    report = "\n".join(lines) + "\n"
    out_path = Path(args.write)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(report)
    print(f"wrote {out_path}")
    print(f"MATCH {match}/{total}, DIVERGE {diverge}, MISSING {missing}, events {total_events}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
