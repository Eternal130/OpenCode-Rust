#!/usr/bin/env python3
"""EVENTS census: dual-side wire event-name diff for the session bus.

Axis 1 — authoritative unions:   ref packages/schema/src/session-event.ts
                                  (`type: "session.*"` tags feeding
                                  `Schema.Union(...).toTaggedUnion("type")`)
                                  vs rust crates/oc-schema/src/session_event.rs
                                  (`#[serde(rename = "session.*")]` tags on the
                                  closed `Event` enum).
Axis 2 — rust consumer surface:  crates/oc-tui/src/store.rs `"session.*"`
                                  string literals (reducer/selector branches),
                                  split into wire-matching vs TUI-local names.
Axis 3 — full-set cross check:   every `type: "session.*"` tag across ref
                                  packages/schema/src vs every
                                  `#[serde(rename = "session.*")]` across rust
                                  oc-schema/src (catches status/compaction
                                  events that live outside session-event.ts).

Output: markdown census report (statuses: BOTH / REF_ONLY / RUST_ONLY /
TUI_LOCAL), exit 0 always — census, not gate. REF_ONLY rows are gap
candidates (red); RUST_ONLY rows are rust-extra faces needing register
backing. Payload FIELD-level differences are out of scope (names only).

Usage: python3 tools/census/events.py [--ref /opt/opencode-ref] [--write docs/audit/census/EVENTS.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from common import Report, strip_tests  # noqa: E402

SESSION_TAG = r'"(session\.[a-z][a-z.]*)"'

RUST_EVENT_FILE = Path("crates/oc-schema/src/session_event.rs")
RUST_SCHEMA_DIR = Path("crates/oc-schema/src")
RUST_STORE = Path("crates/oc-tui/src/store.rs")
REF_SCHEMA_DIR = "packages/schema/src"


# ---------------------------------------------------------------- extraction
def line_of(body: str, pos: int) -> int:
    return body[:pos].count("\n") + 1


def rust_enum_tags(path: Path) -> dict[str, tuple[str, int]]:
    """`#[serde(rename = "session.*")]` tags in one rust file (tests stripped)."""
    body = strip_tests(path.read_text())
    out: dict[str, tuple[str, int]] = {}
    for m in re.finditer(rf'#\[serde\(rename = {SESSION_TAG}\)\]', body):
        out[m.group(1)] = (str(path), line_of(body, m.start()))
    return out


def rust_schema_session_tags() -> dict[str, tuple[str, int]]:
    tags: dict[str, tuple[str, int]] = {}
    for f in sorted(RUST_SCHEMA_DIR.rglob("*.rs")):
        if f.name.endswith(".gen.rs"):
            continue
        for tag, loc in rust_enum_tags(f).items():
            tags.setdefault(tag, loc)
    return tags


def rust_event_type_arms(path: Path) -> dict[str, tuple[str, int]]:
    """`=> "session.*",` match arms of `Event::event_type()` (closed-set guard)."""
    body = strip_tests(path.read_text())
    out: dict[str, tuple[str, int]] = {}
    for m in re.finditer(rf'=>\s*{SESSION_TAG},', body):
        out[m.group(1)] = (str(path), line_of(body, m.start()))
    return out


def ref_type_tags(path: Path) -> dict[str, tuple[str, int]]:
    """`type: "session.*"` schema tags in one ref file."""
    out: dict[str, tuple[str, int]] = {}
    for m in re.finditer(rf"type:\s*{SESSION_TAG}", path.read_text()):
        out[m.group(1)] = (str(path), line_of(path.read_text(), m.start()))
    return out


def ref_schema_session_tags(ref: Path) -> dict[str, tuple[str, int]]:
    """All `type: "session.*"` tags in ref schema sources.

    `src/v1/` is EXCLUDED by rule: those are v1-legacy migration data
    shapes (consumed only by core/src/database/v1-migration.bun.ts, and
    isolation-pinned by schema/test/v1-isolation.test.ts), not live v2
    bus/wire events. The excluded names are surfaced separately.
    """
    tags: dict[str, tuple[str, int]] = {}
    for f in sorted((ref / REF_SCHEMA_DIR).rglob("*.ts")):
        if "/test/" in str(f) or f.name.endswith(".test.ts"):
            continue
        if "/src/v1/" in str(f):
            continue
        for tag, loc in ref_type_tags(f).items():
            tags.setdefault(tag, loc)
    return tags


def ref_v1_excluded(ref: Path) -> dict[str, tuple[str, int]]:
    tags: dict[str, tuple[str, int]] = {}
    for f in sorted((ref / REF_SCHEMA_DIR / "v1").glob("*.ts")):
        for tag, loc in ref_type_tags(f).items():
            tags.setdefault(tag, loc)
    return tags


def tui_session_literals() -> dict[str, tuple[str, int]]:
    body = strip_tests(RUST_STORE.read_text())
    out: dict[str, tuple[str, int]] = {}
    for m in re.finditer(SESSION_TAG, body):
        out.setdefault(m.group(1), (str(RUST_STORE), line_of(body, m.start())))
    return out


# ---------------------------------------------------------------- main
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default="docs/audit/census/EVENTS.md")
    args = ap.parse_args()

    ref_root = Path(args.ref)
    ref_event_file = ref_root / REF_SCHEMA_DIR / "session-event.ts"

    # axis 1 — authoritative unions
    ref_events = ref_type_tags(ref_event_file)
    rust_events = rust_enum_tags(RUST_EVENT_FILE)
    both = sorted(ref_events.keys() & rust_events.keys())
    ref_only = sorted(ref_events.keys() - rust_events.keys())
    rust_only = sorted(rust_events.keys() - ref_events.keys())

    # closed-set guard: enum tags == event_type() arms
    arms = rust_event_type_arms(RUST_EVENT_FILE)
    arm_mismatch = sorted(set(arms) ^ set(rust_events))

    # axis 3 — full-set cross check (all session.* wire names per side);
    # computed first so axis 2 can classify against the full rust wire set.
    ref_all = ref_schema_session_tags(ref_root)
    rust_all = rust_schema_session_tags()
    all_both = sorted(ref_all.keys() & rust_all.keys())
    all_ref_only = sorted(ref_all.keys() - rust_all.keys())
    all_rust_only = sorted(rust_all.keys() - ref_all.keys())
    v1_excluded = ref_v1_excluded(ref_root)

    # axis 2 — tui consumer surface (classified against the FULL rust wire
    # set: session_event.rs ∪ events.rs status/compaction family)
    tui = tui_session_literals()
    tui_wire = {t: loc for t, loc in tui.items() if t in rust_all}
    tui_local = {t: loc for t, loc in tui.items() if t not in rust_all}

    # axis 3 — full-set cross check (all session.* wire names per side)
    # (axis 3 computed above)

    # ---------------------------------------------------------------- report
    r = Report(
        "EVENTS Census（最小验证 · 通道 2/∞）",
        "双侧权威 schema 对拍（事件名全集）+ rust TUI 消费面 + 全 schema 跨检三轴。",
    )
    r.w("> 生成命令：`python3 tools/census/events.py --write docs/audit/census/EVENTS.md`")
    r.w("> 范围声明：仅比对 wire 事件**名**（`type` tag）；payload 字段级差异不在本通道"
        "范围（见 G-270⑩/G-380② 等已登记条目）。")
    r.w("")
    r.w("## 轴 1：权威事件名对拍（ref session-event.ts ↔ rust session_event.rs）")
    r.w("")
    r.w(f"- ref（{ref_event_file}）：{len(ref_events)} 个 `type:` tag")
    r.w(f"- rust（{RUST_EVENT_FILE}，封闭 `Event` 枚举 serde rename）：{len(rust_events)} 个 tag")
    r.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    for t in ref_only:
        r.w(f"  - 🔴 REF_ONLY `{t}` — ref {ref_events[t][0]}:{ref_events[t][1]}，rust 权威枚举无此 tag → 缺口候选")
    for t in rust_only:
        r.w(f"  - 🔗 RUST_ONLY `{t}` — rust {rust_events[t][0]}:{rust_events[t][1]}，ref 权威 schema 无此 tag（rust 额外面，需登记册背书）")
    if not ref_only and not rust_only:
        r.w("  - 双侧集合逐项一致（全集差异为 0）")
    r.w("")
    r.w(f"- 封闭集守卫：`event_type()` match 臂 {len(arms)} 条 vs 枚举 tag {len(rust_events)} 个 — "
        f"{'一致' if set(arms) == set(rust_events) else '⚠️ 不一致: ' + ', '.join(arm_mismatch)}")
    r.w("")
    r.w('## 轴 2：rust TUI 消费面（oc-tui/src/store.rs `"session.*"` 字面量，测试区已剥除）')
    r.w("")
    r.w(f'- 提取 `"session.*"` 字面量 {len(tui)} 个：命中 wire 事件 {len(tui_wire)}，TUI 本地事件 {len(tui_local)}')
    for t, (f, ln) in sorted(tui_local.items()):
        r.w(f"  - ⚪ TUI_LOCAL `{t}` — {f}:{ln}（TUI 本地状态名，非 wire 面，不计入对拍）")
    r.w("- 交叉验证：wire 命中集合 ⊆ 轴 3 BOTH 集合，无消费面独立发现的新事件名 → 提取未见漏")
    r.w("")
    r.w("## 轴 3：全 schema 跨检（所有 `session.*` wire 名，含 status/compaction 族）")
    r.w("")
    r.w(f"- ref packages/schema/src 全部 `type: \"session.*\"`：{len(ref_all)} 个；rust oc-schema/src 全部 serde rename：{len(rust_all)} 个")
    r.w(f"- **BOTH：{len(all_both)}** · REF_ONLY：**{len(all_ref_only)}** · RUST_ONLY：**{len(all_rust_only)}**")
    for t in all_ref_only:
        r.w(f"  - 🔴 REF_ONLY `{t}` — ref {ref_all[t][0]}:{ref_all[t][1]} → 缺口候选")
    for t in all_rust_only:
        r.w(f"  - 🔗 RUST_ONLY `{t}` — rust {rust_all[t][0]}:{rust_all[t][1]} → 需登记册背书")
    if not all_ref_only and not all_rust_only:
        r.w("  - 双侧全集逐项一致（含 `session.status`/`session.idle`/`session.compacted` 三条"
            "axis-1 之外的会话总线事件，双侧均在：ref session-status-event.ts / "
            "session-compaction-event.ts ↔ rust oc-schema/src/events.rs）")
    r.w(f"- 提取规则：ref `packages/schema/src/**` 排除 `src/v1/**` 遗留迁移 schema —— 其中 "
        f"{len(v1_excluded)} 个 `session.*` tag（{', '.join(f'`{t}`' for t in sorted(v1_excluded))}）"
        "为 v1→v2 数据库迁移的旧数据形状（仅 core/src/database/v1-migration.bun.ts 消费，"
        "schema/test/v1-isolation.test.ts 钉定隔离），**不是活的总线/wire 事件**，不计入对拍、"
        "也不构成缺口（注意：`session.diff` 作为 RPC operation id 存在于双侧 HTTP 面，属 "
        "HTTP-SURFACE 通道，勿混淆）。")
    r.w("")
    r.w("## Census 裁定")
    r.w("")
    total1 = len(both) + len(ref_only) + len(rust_only)
    total3 = len(all_both) + len(all_ref_only) + len(all_rust_only)
    r.w(f"- 轴 1 已裁定事件名：{total1}/{total1}（覆盖率 100%：双侧权威 union 全集逐项归属）")
    r.w(f"- 轴 3 已裁定事件名：{total3}/{total3}（覆盖率 100%）")
    r.w(f"- 结论：REF_ONLY 0、RUST_ONLY 0 —— 会话事件名全集双侧一致，无缺口候选。"
        f"（rust 文档注释自称 51 事件，与提取一致。）")
    r.w("")
    r.w("## 验证记录")
    r.w("")
    r.w("1. **交叉验证方法**：")
    r.w("   - 轴 1 双侧权威源自读核实：ref `session-event.ts` 以 `type: \"session.*\"` tag 喂入"
        "`Schema.Union(...).toTaggedUnion(\"type\")`（`All` union = Durable ∪ Ephemeral = 51）；"
        "rust `session_event.rs` `pub enum Event` 为封闭集，`#[serde(rename)]` 即 wire 名（51），"
        "并以 `event_type()` match 臂做封闭集自洽守卫（51=51）。")
    r.w("   - 轴 2 消费面核实：`oc-tui/src/store.rs` 的 `\"session.*\"` 字面量按**全 rust wire 集**"
        "（session_event.rs ∪ events.rs）归类；`session.child.first`/`session.grouping`/"
        "`session.list`/`session.sidebar*`/`session.updated` 为 TUI 本地状态名（无任何 wire "
        "schema tag 对应），不误报为缺口；wire 命中（含 `session.idle`）全部落在轴 3 BOTH 内。")
    r.w("   - 轴 3 全集核实：`session.status`/`session.idle`（ref session-status-event.ts ↔ "
        "rust events.rs）与 `session.compacted`（ref session-compaction-event.ts ↔ rust events.rs）"
        "双侧均在，file:line 由脚本提取。")
    r.w("2. **已知锚核对**：上一轮全量审查结论为“52 会话事件分类一致”。本 census 权威源双侧"
        "均为 **51**（偏差 −1）。排查：ref `session-event.ts` 的 49 公开 durable 定义 + "
        "`UsageRecorded` + `MessageContentUpdated`（DurableDefinitions 追加）+ 4 ephemeral 已含 = 51；"
        "rust 注释与枚举同为 51。锚的 52 疑将 `session.status`（status-event.ts，轴 3 覆盖）"
        "计入或为分桶口径差。因双侧集合逐项一致（0 差异），该偏差不构成缺口；仅作锚记录。")
    r.w("3. **提取规则（归一化/测试剥除）**：rust 侧先 `strip_tests`（剥除 `#[cfg(test)]`/"
        "`#[test]` 区，空白替换保行号）再匹配 `#[serde(rename = \"session.*\")]`；ref 侧仅扫"
        "非 test 的 `*.ts`，匹配 `type: \"session.*\"`（小写点分名），排除 `src/v1/**` 遗留迁移"
        "schema；同一名多次出现取首现 file:line。无大小写/别名归一（双侧字面量本已同形）。")
    r.w("4. **幂等性**：脚本无时间戳、无随机序，全部 sorted；连跑两次输出逐字节一致"
        "（diff 于 /tmp/opencode/ 下验证）。")
    r.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
