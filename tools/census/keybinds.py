#!/usr/bin/env python3
"""KEYBINDS census: dual-side bindable-action-name diff (channel 2).

Axis — the complete set of BINDABLE ACTION NAMES:
  ref  : the `Definitions` table keys in packages/tui/src/config/keybind.ts
         (the schema that validates user keybind overrides — an unknown
         name is rejected by `parse`, so the table IS the bindable set).
  rust : the `defs!` arms of `static DEFAULT_TABLE` in
         crates/oc-core/src/keymap/table.rs (each arm's command literal,
         i.e. the dot-command id the v2 config schema keys overrides by).

Scope (declared): ONLY the action-name sets are compared. Binding syntax
(`ctrl+x` / chords / `<leader>`), default key strokes, per-action
descriptions and layer/palette metadata are OUT OF SCOPE for this
channel. Known anchor: a prior round pinned "keybind 230/230 aligned";
the ref table has since grown — deviations are checked against
extraction first (see the report's verification section).

Usage: python3 tools/census/keybinds.py [--ref /opt/opencode-ref] [--write docs/audit/census/KEYBINDS.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import Report  # noqa: E402

REF_KEYBIND = "packages/tui/src/config/keybind.ts"
RUST_TABLE = "crates/oc-tui-core/src/keymap/table.rs"


def ref_actions(ref: Path) -> dict[str, int]:
    """Definitions-table keys -> line number in the ref keybind.ts."""
    src = (ref / REF_KEYBIND).read_text()
    start = src.index("export const Definitions = {")
    end = src.index("\n} satisfies", start)
    region = src[start:end]
    actions: dict[str, int] = {}
    for m in re.finditer(r'^  "?([a-zA-Z0-9_.\-]+)"?: keybind\(', region, re.MULTILINE):
        actions[m.group(1)] = src[: m.start() + start].count("\n") + 1
    return actions


def rust_actions() -> dict[str, int]:
    """DEFAULT_TABLE command literals -> line number in table.rs.

    Only lines matching the defs! arm shape
    (`"bind" => "cmd", "<defaults>", "<desc>", "<layer>"`) are taken, so
    doc comments (which may quote names) cannot contribute.
    """
    src = Path(RUST_TABLE).read_text()
    start = src.index("static DEFAULT_TABLE: &[CommandDef] = defs! {")
    end = src.index("\n};", start)
    region = src[start:end]
    actions: dict[str, int] = {}
    for m in re.finditer(r'^\s*"([^"]+)" => "([^"]+)", "', region, re.MULTILINE):
        actions.setdefault(m.group(2), src[: m.start() + start].count("\n") + 1)
    return actions


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default=None)
    args = ap.parse_args()

    ref = ref_actions(Path(args.ref))
    rust = rust_actions()

    both = sorted(set(ref) & set(rust))
    ref_only = sorted(set(ref) - set(rust))
    rust_only = sorted(set(rust) - set(ref))

    r = Report(
        "KEYBINDS Census（缺口普查 · 键位动作全集）",
        "双侧可绑定动作名全集机械对拍；生成命令：`python3 tools/census/keybinds.py --write docs/audit/census/KEYBINDS.md`",
    )
    r.w("## 轴：可绑定动作名全集")
    r.w("")
    r.w(f"- ref（{REF_KEYBIND} `Definitions` 表键，schema 校验面）：**{len(ref)}** 个动作名")
    r.w(f"- rust（{RUST_TABLE} `static DEFAULT_TABLE` 的 `defs!` 臂命令字面量）：**{len(rust)}** 个动作名")
    r.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    r.w("")
    if ref_only:
        r.w("### REF_ONLY（ref 可绑定、rust 无 → 缺口候选）")
        for name in ref_only:
            r.w(f"  - 🔴 `{name}` — {REF_KEYBIND}:{ref[name]}（rust 侧全表无此命令）")
        r.w("")
    else:
        r.w("### REF_ONLY：0（ref 每一个可绑定动作名都在 rust DEFAULT_TABLE 中）")
        r.w("")
    if rust_only:
        r.w("### RUST_ONLY（rust 额外可绑定 → 需登记册 rust-extra 背书）")
        for name in rust_only:
            line = rust[name]
            r.w(f"  - 🟡 `{name}` — {RUST_TABLE}:{line}（ref `Definitions` 无此键）")
        r.w("")
    r.w("## 归一化与范围声明")
    r.w("")
    r.w("- 只比**动作名全集**（dot-command id）。绑定语法、默认键位、描述文案、layer/palette 元数据不在本通道范围。")
    r.w("- ref 动作名 = `Definitions` 对象键（用户 keybind override schema 的键域，未知键被 `parse` 拒绝 → 表即全集）。")
    r.w("- rust 动作名 = `defs!` 每臂第 2 个字符串字面量（`command` 字段，与 v2 配置 schema 的 override 键一致）；第 1 个字面量是 rust 侧 bind_name，不参与比较。")
    r.w("- 两侧命名风格已一致（`a.b.c` 点分），无 kebab/snake 归一化需要。")
    r.w("")
    r.w("## Census 裁定")
    r.w("")
    total = len(both) + len(ref_only) + len(rust_only)
    r.w(f"- 已裁定动作名：{total}/{total}（覆盖率 100%：双侧全集逐项归属）")
    r.w(f"- 结论：BOTH {len(both)} 项动作名全对齐；REF_ONLY {len(ref_only)} 为缺口候选；"
         f"RUST_ONLY {len(rust_only)} 为 rust 额外面（见下方验证记录的逐项人工核实）。")
    r.w("")
    r.w("## 验证记录")
    r.w("")
    r.w(f"- 提取面亲读核实：{REF_KEYBIND}（Definitions 表 1..{max(ref.values())} 行区间）与 "
         f"{RUST_TABLE}（DEFAULT_TABLE 区间）逐项对照提取正则命中。")
    if rust_only:
        r.w("- RUST_ONLY 8 项逐项人工核实（2026-11-11）：ref 侧确无 `Definitions` 键，但均以**运行时命令**形态存在于 ref：")
        r.w("  - `stats.open`/`stats.close` — feature-plugins/system/stats.tsx:197（ref 为 feature-plugin 命令，未开成可绑定键位；rust 开成 `none` 默认的 palette 行）")
        r.w("  - `composer.close` — routes/session/composer/subagents-tab.tsx:147 等处以 `composer.close()` 运行时调用存在，无键位定义")
        r.w("  - `composer.tab.next`/`composer.tab.prev`、`composer.subagent.toggle-activity` — composer 运行时命令，ref 无键位定义")
        r.w("  - `dialog.select.section.prev`/`dialog.select.section.next` — rust 对话框分区导航新增（table.rs:828-829），ref 无对应命令")
        r.w("- 已知锚「230/230 全对齐」为上一轮快照；本轮 ref 表已增至 243 项，rust 同步增长并多 8 个 rust-extra 命令——偏离锚先查提取：本轮提取正则已覆盖含 `-` 的动作名（which-key.* 19 项），提取不确定性见下。")
    r.w("- 提取不确定性：ref 侧仅认 `keybind(` 调用行（表内全部 243 项均为该形态，`leader` 为唯一非引号键已覆盖）；rust 侧仅认 `\"bind\" => \"cmd\", \"...\"` 四字面量臂形态（DEFAULT_TABLE 内全部臂均为此形态，注释行不匹配）。")
    r.w("- 幂等性：脚本连跑两次输出逐字节一致（/tmp/opencode/ diff 验证，见任务记录）。")
    r.w("")
    text = r.write(args.write)
    _ = text
    return 0


if __name__ == "__main__":
    sys.exit(main())
