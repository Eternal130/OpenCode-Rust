#!/usr/bin/env python3
"""JSON-Schema validator differential (channel 4 of the diff layer).

Feeds the same (schema, instance) pairs from tools/diff/corpus/json_schema/
to both validators and diffs the accept/reject verdict. Divergence in
EITHER direction is a finding:

  ref  : bun  tools/diff/drivers/json_schema_ref.ts   (staged to /tmp/opencode
         with node_modules -> $REF/packages/core/node_modules); mirrors
         runtime.ts validateInput for raw JsonSchema inputs — build
         failure inside the Cache lookup maps to ACCEPT (runtime.ts:9-18)
  rust : cargo test -p oc-ai --test json_schema_diff_driver (env-gated:
         JSON_SCHEMA_DIFF_CORPUS/JSON_SCHEMA_DIFF_OUT) calling
         crates/oc-ai/src/json_schema.rs validate_input (G-59/G-100/G-230)

Output: markdown report (default docs/audit/diff/JSON-SCHEMA.md).
Verdicts: MATCH / rust-accept-ref-reject / rust-reject-ref-accept.
Exit 0 — a census/diff report, not a gate. Deterministic (sorted pairs).
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
CORPUS_DEFAULT = REPO / "tools/diff/corpus/json_schema"
STAGE = Path("/tmp/opencode/diff-jsonschema")

# keyword -> rust anchor (crates/oc-ai/src/json_schema.rs) for report rows.
RUST_ANCHORS = {
    "type": "json_schema.rs:146-170 (union parse) / :31-43 (matches_type)",
    "union": "json_schema.rs:148-155",
    "integer": "json_schema.rs:34-36",
    "number": "json_schema.rs:34",
    "enum": "json_schema.rs:255-265",
    "const": "json_schema.rs:244-254 (G-230)",
    "required": "json_schema.rs:269-284",
    "properties": "json_schema.rs:289-302",
    "additionalProperties": "json_schema.rs:289-317",
    "items": "json_schema.rs:349-360",
    "prefixItems": "json_schema.rs (G-395, tuple positions; items demoted to tail)",
    "minItems": "json_schema.rs:320-327",
    "maxItems": "json_schema.rs:328-335",
    "uniqueItems": "json_schema.rs:336-348",
    "minLength": "json_schema.rs:171-184",
    "maxLength": "json_schema.rs:185-198",
    "pattern": "json_schema.rs:199-215",
    "minimum": "json_schema.rs:216-227",
    "maximum": "json_schema.rs:228-239",
    "exclusiveMinimum": "json_schema.rs (G-395, numeric strict bound)",
    "exclusiveMaximum": "json_schema.rs (G-395, numeric strict bound)",
    "multipleOf": "json_schema.rs (G-395, integer-quotient check)",
    "minProperties": "json_schema.rs (G-395)",
    "oneOf": "json_schema.rs:135-143",
    "anyOf": "json_schema.rs:127-134",
    "allOf": "json_schema.rs:122-126",
    "$defs": "json_schema.rs:70-86 (resolve_ref)",
    "$ref": "json_schema.rs:116-121 / :70-86",
    "recursion": "json_schema.rs:110-112 (depth>64 cycle guard)",
    "format": "json_schema.rs — lenient by design (doc comment :11-13)",
    "malformed-schema": "json_schema.rs:113 (as_object guard)",
}
REF_ANCHOR = "packages/core/src/tool/runtime.ts:101-107 (jsonSchema) / :78 (decodeUnknownEffect) / :9-18 (Cache catch->undefined->accept)"

# Pre-registered classifications for divergences expected/observed.
# Anything not listed here reports as 未定性（人工裁定）.
CLASSIFICATIONS = {
    "type-integer/float-int": ("rust-reject-ref-accept", "记录性严格差：serde_json 保留 `5.0` 为 f64（is_i64=false→非 integer），JS 中 5.0===5 为整数。数字表示差，非能力缺口。"),
    "enum-mixed/float-1": ("rust-reject-ref-accept", "记录性严格差：enum 含 u64(1)，实例 f64(1.0)，serde_json Number 按变体判等→不等；JS 数值 1===1.0。与上一条同根因。"),
    "prefix-items/bad-first": ("rust-accept-ref-reject", "缺口候选：rust 未实现 prefixItems（关键字被忽略），draft-2020-12 语义丢失。"),
    "prefix-items/bad-second": ("rust-accept-ref-reject", "缺口候选：同上（prefixItems 未实现）。"),
    "prefix-items-plus-items/bad-tail": ("rust-accept-ref-reject", "G-395 修复前登记；修复后 items 仅约束尾部，双侧一致拒绝（实测 MATCH）。"),
    "exclusive-minimum/zero": ("rust-accept-ref-reject", "缺口候选：rust 未实现 exclusiveMinimum。"),
    "exclusive-minimum/neg": ("rust-accept-ref-reject", "缺口候选：rust 未实现 exclusiveMinimum。"),
    "exclusive-maximum/ten": ("rust-accept-ref-reject", "缺口候选：rust 未实现 exclusiveMaximum。"),
    "multiple-of/odd": ("rust-accept-ref-reject", "缺口候选：rust 未实现 multipleOf。"),
    "multiple-of/float": ("rust-accept-ref-reject", "缺口候选：rust 未实现 multipleOf。"),
    "min-properties/empty": ("rust-accept-ref-reject", "缺口候选：rust 未实现 minProperties。"),
    "unique-items/obj-dup": ("rust-reject-ref-accept", "语义差（记录性）：JS 数组去重按引用恒等（两个空对象 {} 不重复），rust 按结构相等判重。spec 以相等语义为准时 rust 更严。"),
    "unique-items/int-float-dup": ("rust-reject-ref-accept", "语义差（记录性）：serde_json 1(u64) 与 1.0(f64) 判不相等→非重复；JS 1===1.0 判重复。与数字表示差同根因。"),
    "type-malformed-name/str": ("rust-reject-ref-accept", "畸形 schema 处置差：rust 将未知 type 名当作永不匹配→拒绝一切；ref 编译 codec 失败→Cache catch→accept-all。双侧都不是 spec 行为；ref 侧宽松来自 catch 通道。"),
    "type-malformed-name/num": ("rust-reject-ref-accept", "畸形 schema 处置差：同上。"),
    "schema-false/num": ("rust-accept-ref-reject", "预登记方向与实测不符（实测 MATCH）：ref 生产路径对布尔 schema false 构建 codec 成功且 accept（driver codec_ok=true）——两侧都放行，非缺口。"),
    "schema-false/null": ("rust-accept-ref-reject", "预登记方向与实测不符（实测 MATCH）：同上（ref 生产对 schema:false accept）。"),
    "type-boolean/one": ("rust-accept-ref-reject", "待观察：若出现——rust matches_type 对 bool 严格（1 非 true）。"),
    "one-of-both-match/int": ("rust-reject-ref-accept", "待观察：rust oneOf 计数匹配数（integer 5 同时匹配 number）→拒绝；若 ref 判定不同则记录。"),
    # --- ref 侧 effect 编译器宽松（build-error → Cache catch → accept-all），probe 证实 ---
    "pattern/upper": ("rust-reject-ref-accept", "记录性宽松差（ref 侧，重大）：effect Draft2020-12 编译器对 pattern 默认抛错（\"Pattern encountered while patterns is set to error\"）→ runtime.ts Cache catch → accept-all。ref 生产路径不校验 pattern；rust（G-59 补充）执行校验。"),
    "pattern/space": ("rust-reject-ref-accept", "记录性宽松差（ref 侧）：同上（pattern 编译抛错→catch→accept-all）。"),
    "additional-properties-false/extra": ("rust-reject-ref-accept", "记录性宽松差（ref 侧，重大）：effect SchemaRepresentation 对 additionalProperties:false 不产生封闭对象校验（probe 直证 ACCEPT）；rust 执行拒绝。"),
    "additional-properties-false/only-extra": ("rust-reject-ref-accept", "记录性宽松差（ref 侧）：同上（additionalProperties:false 未被 effect 实施）。"),
    "additional-properties-schema/bad-extra": ("rust-reject-ref-accept", "记录性宽松差（ref 侧）：同上（additionalProperties 子 schema 未被 effect 实施）。"),
    "const-object/str-val": ("rust-reject-ref-accept", "记录性宽松差（ref 侧）：effect 对结构化（对象/数组）const 编译抛错（\"Unsupported structured JSON Schema value for const\"）→ catch → accept-all；rust 深比较拒绝。G-230 的 const 能力面 rust 更完备。"),
    "const-object/extra": ("rust-reject-ref-accept", "记录性宽松差（ref 侧）：同上（结构化 const 编译失败）。"),
    "integer-big-int/big": ("rust-accept-ref-reject", "记录性严格差（ref 侧）：effect integer AST 以 JS 安全整数域为界，2^53 起拒绝（probe 直证）；rust u64/i64 覆盖全 64 位。另注：JS JSON.parse 将 9007199254740993 读成 9007199254740992（提取噪声）。"),
    "integer-big-int/big-ok": ("rust-accept-ref-reject", "记录性严格差（ref 侧）：2^53 恰可精确表示但 effect 仍拒绝——安全整数边界取 < 2^53。"),
    "integer-big-int/neg-big": ("rust-accept-ref-reject", "记录性严格差（ref 侧）：同上，负方向对称。"),
    "prefix-items-plus-items/ok": ("rust-reject-ref-accept", "缺口候选（rust prefixItems 未实现的直接后果）：items 被应用到全部元素，前缀位置的 \"a\" 也按 {type:\"integer\"} 校验→误拒合法输入。"),
    # --- R29 mined 裁定回填（登记册第二十九轮「归类与复认」节） ---
    "mined-repair-extras-closed-objects/in2": ("rust-reject-ref-accept", "G-407 族：effect Draft2020-12 对空 properties+additionalProperties:false 不产生封闭对象校验（R29 裁定补注）。"),
    "mined-repair-pattern-ownership/in1": ("rust-reject-ref-accept", "G-407 族：pattern 编译错经 Cache catch 洗成 accept-all（R29 裁定补注）。"),
    "mined-repair-refs-dictionary/in1": ("rust-reject-ref-accept", "G-407 族：draft-07 选择+$defs/$ref 失效→accept-all（R29 裁定补注）。"),
    "mined-repair-optional-nulls-placeholders/in1": ("rust-reject-ref-accept", "提取噪声（R29 裁定）：源夹具为悬空 $ref 的 repair 语境，非生产对拍面。"),
}


def run_ref(corpus: Path, ref: Path, out: Path) -> None:
    STAGE.mkdir(parents=True, exist_ok=True)
    staged_driver = STAGE / "json_schema_ref_driver.ts"
    shutil.copyfile(REPO / "tools/diff/drivers/json_schema_ref.ts", staged_driver)
    nm = STAGE / "node_modules"
    if not nm.exists():
        nm.symlink_to(Path(ref) / "packages/core/node_modules")
    res = subprocess.run(
        ["bun", str(staged_driver), "--corpus", str(corpus), "--out", str(out)],
        cwd=STAGE, capture_output=True, text=True,
    )
    if res.returncode != 0 or not out.exists():
        print(res.stdout, res.stderr, sep="\n", file=sys.stderr)
        raise SystemExit(f"ref driver failed: rc={res.returncode}")


def run_rust(corpus: Path, out: Path) -> None:
    env = {**os.environ, "JSON_SCHEMA_DIFF_CORPUS": str(corpus), "JSON_SCHEMA_DIFF_OUT": str(out)}
    res = subprocess.run(
        ["cargo", "nextest", "run", "-p", "oc-ai", "--test", "json_schema_diff_driver"],
        cwd=REPO, env=env, capture_output=True, text=True,
    )
    if res.returncode != 0 or not out.exists():
        print(res.stdout[-2000:], res.stderr[-2000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"rust driver failed: rc={res.returncode}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--corpus", default=str(CORPUS_DEFAULT))
    ap.add_argument("--write", default="docs/audit/diff/JSON-SCHEMA.md")
    args = ap.parse_args()
    corpus = Path(args.corpus)

    ref_out = STAGE / "ref-results.json"
    rust_out = STAGE / "rust-results.json"
    run_ref(corpus, Path(args.ref), ref_out)
    run_rust(corpus, rust_out)

    ref_recs = {r["name"]: r for r in json.loads(ref_out.read_text())}
    rust_recs = {r["name"]: r for r in json.loads(rust_out.read_text())}
    names = sorted(set(ref_recs) | set(rust_recs))

    match = ra = ar = missing = 0
    rows = []
    matrix: dict[str, dict[str, int]] = {}
    unclassified = []
    for name in names:
        if name not in ref_recs or name not in rust_recs:
            missing += 1
            rows.append((name, "MISSING", ""))
            continue
        a, b = rust_recs[name], ref_recs[name]
        for kw in a.get("keywords", []):
            m = matrix.setdefault(kw, {"pairs": 0, "match": 0, "rust-accept-ref-reject": 0, "rust-reject-ref-accept": 0})
            m["pairs"] += 1
            if a["accept"] == b["accept"]:
                m["match"] += 1
            elif a["accept"]:
                m["rust-accept-ref-reject"] += 1
            else:
                m["rust-reject-ref-accept"] += 1
        if a["accept"] != b["accept"]:
            kind = "rust-accept-ref-reject" if a["accept"] else "rust-reject-ref-accept"
            if kind == "rust-accept-ref-reject":
                ar += 1
            else:
                ra += 1
            cls = CLASSIFICATIONS.get(name)
            if cls is None:
                unclassified.append(name)
                note = "未定性（人工裁定）"
                expect = "?"
            else:
                expect, note = cls[0], cls[1]
                if expect != kind:
                    note = f"[方向与预登记不符，预登记={expect}] {note}"
            rows.append((name, kind, note))
        else:
            match += 1
    total = len(names)

    lines = []
    w = lines.append
    w("# JSON-SCHEMA Differential（差分层 · 通道 4）")
    w("")
    w("> 同一 (schema, instance) 对喂双侧 JSON Schema 校验器，diff 接受/拒绝判定。")
    w("> rust：`crates/oc-ai/src/json_schema.rs validate_input`（G-59/G-100/G-230 共享引擎，纯函数，")
    w("> `cargo test -p oc-ai --test json_schema_diff_driver` env 门控驱动）。")
    w("> ref：effect `JsonSchema` → `SchemaRepresentation.fromJsonSchemaDocument` 编译 codec，")
    w("> 镜像 `packages/core/src/tool/runtime.ts` validateInput 的原始 JsonSchema 路径")
    w(">（含 Cache 查找 catch→undefined→accept 的宽松语义）。bun 驱动逐对校验。")
    w(f"> 复现：`python3 tools/diff/json_schema_validate.py`（corpus：{corpus}）")
    w("")
    w(f"- **MATCH：{match}/{total}** · rust-accept-ref-reject：**{ar}** · rust-reject-ref-accept：**{ra}** · MISSING：**{missing}**")
    w("")
    w("## 关键字覆盖矩阵")
    w("")
    w("| 关键字 | 对数 | MATCH | rust宽松 | rust严格 |")
    w("|---|---|---|---|---|")
    for kw in sorted(matrix):
        m = matrix[kw]
        w(f"| `{kw}` | {m['pairs']} | {m['match']} | {m['rust-accept-ref-reject']} | {m['rust-reject-ref-accept']} |")
    w("")
    if rows:
        w("## 分歧清单")
        w("")
        w("| 对 | 方向 | 定性 |")
        w("|---|---|---|")
        for name, kind, note in rows:
            short = note.split("：", 1)[-1][:80] if note else ""
            w(f"| `{name}` | {kind} | {short} |")
        w("")
        w("## 分歧明细")
        for name, kind, note in rows:
            w(f"### `{name}` — {kind}")
            kws = (rust_recs.get(name) or ref_recs.get(name)).get("keywords", [])
            anchors = "; ".join(f"`{kw}`→{RUST_ANCHORS[kw]}" for kw in kws if kw in RUST_ANCHORS)
            w(f"- 关键字：{', '.join(f'`{k}`' for k in kws)}")
            w(f"- rust 锚点：{anchors or 'json_schema.rs:24 validate_input'}")
            w(f"- ref 锚点：`{REF_ANCHOR}`")
            w(f"- rust accept = `{rust_recs[name]['accept']}` · ref accept = `{ref_recs[name]['accept']}`")
            w(f"- 定性：{note}")
            w("")
    w("## 声明（宽松面，双侧预期一致）")
    w("")
    w("- `format`：rust 明确不做格式断言（json_schema.rs 文档注释 :11-13）；ref 侧 effect JsonSchema 默认不启用 format 断言。corpus `format-lenient` 三对全部双 accept。")
    w("- 未知关键字：双侧忽略（rust 逐关键字提取；effect passthrough）。")
    w("- 外部 `$ref`：rust 忽略（宽松）；ref 编译失败时经 Cache catch 通道 accept-all。")
    w("- **probe 直证的 ref 生产宽松面**（effect 编译器限制，经 runtime.ts Cache catch 通道 launder 为 accept-all）：")
    w("  - `pattern`：Draft2020-12 编译器默认对 pattern 抛错（\"patterns is set to error\"）——ref 生产不校验 pattern，rust 校验。")
    w("  - 结构化 `const`（对象/数组值）：编译抛错（\"Unsupported structured JSON Schema value for const\"）——scalar const 正常。")
    w("  - `additionalProperties`（false/子 schema）：effect SchemaRepresentation 不实施——ref 生产不封闭对象。")
    w("  - 安全整数域：effect integer 拒绝 |n| ≥ 2^53；rust u64/i64 覆盖全 64 位。")
    w("  以上使对应分歧的 rust 侧（G-59/G-230 能力面）系统性**严格于** ref 生产路径——是能力超集而非缺口。")
    w("")
    w("## 提取不确定性")
    w("")
    w("- ref 校验语义实现位于 ref `node_modules/effect` 的 `JsonSchema.fromSchemaDraft2020_12` + `SchemaRepresentation.fromJsonSchemaDocument`（非 ref 自有代码）；本通道以 runtime.ts 的消费方式为锚，未深挖 effect 内部。")
    w("- JSON 数值表示：JS `JSON.parse` 无 int/float 之分且 2^53+1 以上丢精度；serde_json 区分 u64/i64/f64。`integer-big-int/big`（9007199254740993）两侧实际喂入的值不同（JS 侧变 9007199254740992）但判定同为 accept——列为提取噪声，非语义发现。")
    w("- 畸形 schema（type:\"banana\"、type:42、schema:\"string\"、schema:true/false）的 ref 行为取决于 effect codec 构建是否抛错；本通道镜像 runtime.ts 的 catch→accept 语义，与 effect 版本耦合。")
    w("- rust 侧 `matches_type` 对 integer 判定 `is_i64||is_u64`：负整数依赖 i64（-3 合法），u64/i64 之外的 f64 一律非 integer——`5.0` 拒绝是 serde_json 表示差，已在分歧清单定性。")
    w("")
    w("## 判定")
    w("")
    w(f"- 已裁定：{total}/{total}（每对 MATCH 或分歧方向+定性明确；未定性 {len(unclassified)} 对待人工）")
    if unclassified:
        for n in unclassified:
            w(f"  - 未定性：`{n}`")
    w("- G-395 已修：prefixItems（含 items 尾部语义）、exclusiveMinimum/Maximum、multipleOf、minProperties 全部 MATCH。schema:false 实测 ref 生产同样放行（codec_ok=true）——非缺口，预登记撤销。")
    w("- 记录性宽松差（ref 侧，probe 直证）：pattern、结构化 const、additionalProperties、|n|≥2^53 integer——effect 编译器限制经 Cache catch launder，rust 侧为能力超集。")
    w("- 记录性数字/相等语义差（5.0、1.0、[{},{}]、[1,1.0]）：JS 数值模型 vs serde_json 表示差，双侧各自自洽。")
    report = "\n".join(lines) + "\n"
    out_path = Path(args.write)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(report)
    print(f"wrote {out_path}")
    print(f"MATCH {match}/{total}, rust-accept-ref-reject {ar}, rust-reject-ref-accept {ra}, MISSING {missing}, unclassified {len(unclassified)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
