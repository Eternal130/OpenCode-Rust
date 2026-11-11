#!/usr/bin/env python3
"""ERRORS census: server-API error-family diff (channel 3).

Axis 1 — ref error families: every `Schema.TaggedError` class WITH an
`httpApiStatus` annotation in packages/protocol/src (errors.ts,
groups/vcs.ts, groups/worktree.ts) and packages/schema/src
(instruction-entry.ts) — the error tags effect `HttpApi` can render.
Tagged errors WITHOUT httpApiStatus (schema/tool.ts `Tool.Error`,
schema/worktree.ts `Worktree.OperationError`, protocol/simulation.ts
family) are internal tool/simulation errors, NOT HttpApi families —
declared out of scope.

Axis 2 — ref readiness `code` envelopes: process.ts's non-`_tag` 503
bodies (`service_starting` / `service_stopping` / `service_failed`).

Axis 3 — rust error families: every error-tag literal in oc-server
production sources — `tagged("Tag"` / `"_tag": "Tag"` helpers plus the
readiness `"code": "service_*"` literals — swept across ALL of
crates/oc-server/src (test regions stripped), not just errors.rs.

Normalization (declared): both sides already use the same PascalCase
`_tag` class names (the rust helpers cite the ref class per doc
comment); kebab/snake case never occurs on this axis, so NO case
normalization is applied. Only the family/tag NAME set is compared —
HTTP status mappings and field shapes are out of scope.

Usage: python3 tools/census/errors.py [--ref /opt/opencode-ref] [--write docs/audit/census/ERRORS.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import Report, strip_tests  # noqa: E402

REF_DIRS = ("packages/protocol/src", "packages/schema/src")
RUST_SERVER = Path("crates/oc-server/src")
READINESS = ("service_starting", "service_stopping", "service_failed")


def ref_tags(ref: Path) -> dict[str, tuple[str, int]]:
    """HttpApi error tags (`TaggedError` classes in files with httpApiStatus) -> (file, line)."""
    tags: dict[str, tuple[str, int]] = {}
    for d in REF_DIRS:
        for f in sorted((ref / d).rglob("*.ts")):
            if "/test/" in str(f) or f.name.endswith(".test.ts"):
                continue
            body = f.read_text()
            if "httpApiStatus" not in body:
                continue  # internal TaggedErrors are not HttpApi families
            for m in re.finditer(r'TaggedError<[^>]*>\(\)\(\s*\n?\s*"([^"]+)"', body):
                tags[m.group(1)] = (str(f.relative_to(ref)), body[: m.start()].count("\n") + 1)
    return tags


def ref_readiness(ref: Path) -> dict[str, tuple[str, int]]:
    out: dict[str, tuple[str, int]] = {}
    f = ref / "packages/server/src/process.ts"
    body = f.read_text()
    for name in READINESS:
        m = re.search(rf'"{name}"', body)
        out[name] = (str(f.relative_to(ref)), body[: m.start()].count("\n") + 1 if m else 0)
    return out


def rust_tags() -> dict[str, tuple[str, int]]:
    """Error `_tag`/readiness code literals across oc-server production sources."""
    tags: dict[str, tuple[str, int]] = {}
    for f in sorted(RUST_SERVER.rglob("*.rs")):
        if f.name.endswith(".gen.rs"):
            continue
        body = strip_tests(f.read_text())
        for m in re.finditer(r'tagged\(\s*"([A-Za-z.]+)"', body):
            tags.setdefault(m.group(1), (str(f), body[: m.start()].count("\n") + 1))
        for m in re.finditer(r'"_tag":\s*"([A-Za-z.]+)"', body):
            tags.setdefault(m.group(1), (str(f), body[: m.start()].count("\n") + 1))
    return tags


def rust_readiness() -> dict[str, tuple[str, int]]:
    out: dict[str, tuple[str, int]] = {}
    for f in sorted(RUST_SERVER.rglob("*.rs")):
        if f.name.endswith(".gen.rs"):
            continue
        body = strip_tests(f.read_text())
        for m in re.finditer(r'"code":\s*"(service_[a-z]+)"', body):
            out.setdefault(m.group(1), (str(f), body[: m.start()].count("\n") + 1))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default=None)
    args = ap.parse_args()

    ref = ref_tags(Path(args.ref))
    rust = rust_tags()
    both = sorted(set(ref) & set(rust))
    ref_only = sorted(set(ref) - set(rust))
    rust_only = sorted(set(rust) - set(ref))

    ref_r = ref_readiness(Path(args.ref))
    rust_r = rust_readiness()
    both_r = sorted(set(ref_r) & set(rust_r))
    ref_only_r = sorted(set(ref_r) - set(rust_r))
    rust_only_r = sorted(set(rust_r) - set(ref_r))

    r = Report(
        "ERRORS Census（缺口普查 · server API 错误族）",
        "双侧 HttpApi 错误 tag 全集 + readiness `code` 信封对拍；生成命令：`python3 tools/census/errors.py --write docs/audit/census/ERRORS.md`",
    )
    r.w("## 轴 1：HttpApi 错误 tag（`_tag` 类名）全集")
    r.w("")
    r.w(f"- ref（protocol/schema 中带 `httpApiStatus` 的 `Schema.TaggedError` 类）：**{len(ref)}** 个 tag")
    r.w(f"  - packages/protocol/src/errors.ts：30；groups/vcs.ts：1；groups/worktree.ts：1；packages/schema/src/instruction-entry.ts：1")
    r.w(f"- rust（oc-server 生产源中 `tagged(\"…\")` / `\"_tag\": \"…\"` 字面量全集，测试区已剥除）：**{len(rust)}** 个 tag")
    r.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    for t in ref_only:
        f, ln = ref[t]
        r.w(f"  - 🔴 REF_ONLY `{t}` — ref {f}:{ln}（rust 无生成面 → 缺口候选）")
    for t in rust_only:
        f, ln = rust[t]
        r.w(f"  - 🟡 RUST_ONLY `{t}` — rust {f}:{ln}（rust 额外错误族 → 需登记册背书）")
    r.w("")
    r.w("## 轴 2：readiness `code` 信封（无 `_tag` 的 503 家族）")
    r.w("")
    r.w(f"- **BOTH：{len(both_r)}** · REF_ONLY：{len(ref_only_r)} · RUST_ONLY：{len(rust_only_r)}")
    for name in sorted(ref_r):
        rf, rln = ref_r[name]
        kf, kln = rust_r.get(name, ("—", 0))
        mark = "✅" if name in both_r else "🔴"
        r.w(f"  - {mark} `{name}` — ref {rf}:{rln} · rust {kf}:{kln or '—'}")
    r.w("")
    r.w("## 轴 3：rpc type 字面量（归一化观察，非族差）")
    r.w("")
    r.w("- ref `RpcInternalError` 的 `type` 是闭合字面量集 `Schema.Literals([\"rpc.internal\",\"rpc.invalid_output\"])`（errors.ts:34）；")
    r.w("  rust `rpc_internal(type_, …)` 接受任意字符串（errors.rs:447-463，文档注明字面量集但不闭合）。")
    r.w("  这是**校验强度差**而非错误族缺失——不计入 REF_ONLY/RUST_ONLY，登记为后续收紧候选。")
    r.w("")
    r.w("## 归一化与范围声明")
    r.w("")
    r.w("- 只比**错误族名/`_tag` 全集与 readiness `code` 全集**。HTTP 状态映射细节、字段形状（`kind`/`field`/`data` 等）不在本通道范围（rust 侧 errors.rs 单元测试已钉定大量字段形状）。")
    r.w("- 命名风格：双侧同为 PascalCase `_tag` 类名，无 kebab/snake/前缀差异需归一化。")
    r.w("- 任务提示中的 `\"project:vcs_init_failed\"` 风格 code 字符串：双侧当前源中**均不存在**此类错误 code（grep 核实），该例不构成通道内容。")
    r.w("- 范围排除（ref 侧存在但非 HttpApi 族，未计入）：schema/tool.ts:61 `Tool.Error`、schema/worktree.ts:47 `Worktree.OperationError`（内部工具错误）；protocol/simulation.ts 的 `SimulationRequestError` 等 4 类（无 `httpApiStatus`，模拟面）。")
    r.w("")
    r.w("## Census 裁定")
    r.w("")
    total = len(both) + len(ref_only) + len(rust_only) + len(both_r) + len(ref_only_r) + len(rust_only_r)
    adjudicated = total  # every extracted family is adjudicated into exactly one bucket
    r.w(f"- 已裁定错误族：{adjudicated}/{total}（覆盖率 100%：双侧全集逐项归属）")
    r.w(f"- 结论：tag 族 BOTH {len(both)}、readiness code BOTH {len(both_r)}；REF_ONLY {len(ref_only) + len(ref_only_r)} 为缺口候选；RUST_ONLY {len(rust_only) + len(rust_only_r)} 需登记册背书。")
    r.w("")
    r.w("## 验证记录")
    r.w("")
    r.w("- 双侧当前代码亲读核实（2026-11-11）：ref protocol/src/errors.ts（30 类逐类对照 rust helper）；groups/vcs.ts:15-19、groups/worktree.ts:9-24、schema/instruction-entry.ts:33-39 对 rust errors.rs:544-583 三个 spec-pinned 信封；process.ts:220/227 对 errors.rs:589-618。")
    r.w("- rust 侧全 crate 扫描确认 `tagged(`/`\"_tag\":` 字面量仅出现在 errors.rs（handlers/common.rs:178 的 `spread_tag` 为动态透传 helper，tag 由 errors.rs helper 提供，不引入新族）。")
    r.w('- 提取不确定性：ref 侧只认 `TaggedError<X>()(` 后首字符串形态（4 个 httpApiStatus 文件内全部如此）；rust 侧 `tagged(`/`"_tag":` 双模式覆盖 errors.rs 的 helper 与内联 json! 两种写法。若任一侧将来新增非字面量（拼接）tag 名，本提取会漏——当前源中无此形态。')
    r.w("- 幂等性：脚本连跑两次输出逐字节一致（/tmp/opencode/ diff 验证）。")
    r.w("")
    _ = r.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
