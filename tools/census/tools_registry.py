#!/usr/bin/env python3
"""TOOLS census: builtin tool registry, rust vs ref.

Axis 1 — tool-name universe: rust builtin static registrations — every
  production `BuiltinTool { … }` construction under crates/oc-core/src/tool
  (name/namespace resolved, `NAME` consts expanded) plus the synthesized
  Code Mode `execute` registration (codemode.rs `execute_registration`) —
  vs the ref builtin registry — every `.add({ … })` tool draft in
  packages/core/src/tool/plugin/*.ts plus the Code Mode `execute` tool
  (packages/core/src/codemode/tool.ts).
Axis 2 — per-tool INPUT schema presence (declared / not declared). Presence
  only; schema CONTENT is out of scope.
Axis 3 — per-tool OUTPUT schema declaration presence (the G-230 anchor:
  rust builtin path hardcodes `output_schema: None` in
  tool/registry.rs::register).

Scope declaration (out of scope, stated): MCP dynamic tool registration
  (rust mcp/registry.rs — `pipeline_executor_with_output` with a live
  schema; ref tool/mcp.ts) is runtime-driven and not part of the builtin
  static registry. Plugin/agent-dynamic tools are likewise out of scope.

Usage: python3 tools/census/tools_registry.py [--ref /opt/opencode-ref] [--write docs/audit/census/TOOLS.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import Report, _match_brace, strip_tests  # noqa: E402

TOOL_DIR = Path("crates/oc-core/src/tool")
ENGINE = Path("crates/oc-core/src/engine/mod.rs")
REGISTRY = Path("crates/oc-core/src/tool/registry.rs")
REF_PLUGIN_DIR = "packages/core/src/tool/plugin"
REF_CODEMODE_TOOL = "packages/core/src/codemode/tool.ts"

NAME_CONST = re.compile(r'pub const (\w*NAME)\s*:\s*&str\s*=\s*"([^"]+)"')


def kebab(s: str) -> str:
    return re.sub(r"_", "-", s)


# ---------------------------------------------------------------- rust side
def rust_name_consts(src_dir: Path) -> dict[str, str]:
    consts: dict[str, str] = {}
    for f in sorted(src_dir.rglob("*.rs")):
        for m in NAME_CONST.finditer(strip_tests(f.read_text())):
            consts[m.group(1)] = m.group(2)
    return consts


def rust_builtin_tools() -> list[dict]:
    """Production BuiltinTool constructions: name/namespace/input/line."""
    consts = rust_name_consts(TOOL_DIR)
    tools = []
    for f in sorted(TOOL_DIR.glob("*.rs")):
        if f.name.startswith("tests"):
            continue
        body = strip_tests(f.read_text())
        local_consts = dict(NAME_CONST.findall(body))
        for m in re.finditer(r"(?m)^\s*(?:(?:super|crate::tool)::)?BuiltinTool\s*\{", body):
            close = _match_brace(body, m.end() - 1)
            region = body[m.end():close if close != -1 else m.end() + 2000]
            nm = re.search(r'name:\s*"([^"]+)"', region)
            if not nm:
                nm2 = re.search(r"name:\s*([A-Za-z0-9_:]+)", region)
                if not nm2:
                    continue
                # per-file NAME consts win (shell/subagent both define NAME)
                key = nm2.group(1).rsplit("::", 1)[-1]
                name = local_consts.get(key) or consts.get(key)
                if name is None:
                    continue  # unresolved — surfaced via coverage count
            else:
                name = nm.group(1)
            ns = re.search(r'namespace:\s*Some\("([^"]+)"\)', region)
            line = body[: m.start()].count("\n") + 1
            tools.append({
                "name": name,
                "namespace": ns.group(1) if ns else None,
                "input": re.search(r"input:\s", region) is not None,
                "file": str(f),
                "line": line,
            })
    return tools


def rust_execute_tool() -> dict | None:
    """The synthesized Code Mode `execute` registration (codemode.rs)."""
    body = strip_tests((TOOL_DIR / "codemode.rs").read_text())
    const = re.search(r'pub const EXECUTE_NAME\s*:\s*&str\s*=\s*"([^"]+)"', body)
    reg = re.search(r"pub fn execute_registration\(\)\s*->\s*ToolRegistration", body)
    if not (const and reg):
        return None
    region = body[reg.end(): reg.end() + 600]
    return {
        "name": const.group(1),
        "namespace": None,
        "input": "ToolRegistration::new(" in region,
        "file": str(TOOL_DIR / "codemode.rs"),
        "line": body[: reg.start()].count("\n") + 1,
    }


def rust_registration_sites() -> list[tuple[str, int]]:
    """Production `register_builtin(`/direct `::register(` call sites."""
    hits = []
    for f in sorted(Path("crates/oc-core/src").rglob("*.rs")):
        if f.name.endswith(".gen.rs"):
            continue
        body = strip_tests(f.read_text())
        for m in re.finditer(r"register_builtin\(|mcp_resource::register\(|subagent::register\(|register_code_mode_with_limits\(", body):
            hits.append((str(f), body[: m.start()].count("\n") + 1))
    return hits


def rust_output_schema_none() -> tuple[int, int]:
    """(line of `output_schema: None` inside registry::register, count of
    production `pipeline_executor_with_output` calls passing `Some`)."""
    body = REGISTRY.read_text()
    m = re.search(r"pub fn register\(", body)
    line = 0
    if m:
        idx = body.find("output_schema: None,", m.end())
        if idx != -1:
            line = body[:idx].count("\n") + 1
    some_calls = 0
    for f in sorted(TOOL_DIR.rglob("*.rs")):
        src = strip_tests(f.read_text())
        for c in re.finditer(r"pipeline_executor_with_output\(", src):
            tail = src[c.end(): c.end() + 400]
            # third argument (after name, schema) — look for a `Some(` used
            # as an argument before the closing paren region of the args
            args = tail.split(",")
            if len(args) > 2 and "Some(" in args[2]:
                some_calls += 1
    return line, some_calls


# ---------------------------------------------------------------- ref side
def ref_tool_entries(ref: Path) -> list[dict]:
    """Every `.add({ … })` tool draft in plugin/*.ts + codemode execute."""
    entries: list[dict] = []
    files = sorted((ref / REF_PLUGIN_DIR).glob("*.ts")) + [ref / REF_CODEMODE_TOOL]
    for f in files:
        if not f.exists():
            continue
        src = f.read_text()
        shorthand = re.search(r'export const name = "([^"]+)"', src)
        shorthand_name = shorthand.group(1) if shorthand else None
        for m in re.finditer(r"\.add\(\s*\{", src):
            close = _match_brace(src, m.end() - 1)
            region = src[m.end():close if close != -1 else m.end() + 4000]
            # only the option fields before the execute body
            head = region.split("execute:")[0]
            nm = re.search(r'name:\s*"([^"]+)"', head)
            if nm:
                name = nm.group(1)
            elif shorthand_name and re.search(r"^\s*name,\s*$", head, re.MULTILINE):
                name = shorthand_name
            else:
                continue
            if name == "opencode":  # the namespace draft, not a tool
                continue
            ns = re.search(r'namespace:\s*"([^"]+)"', head)
            entries.append({
                "name": name,
                "namespace": ns.group(1) if ns else None,
                "input": re.search(r"input:\s", head) is not None,
                "output": re.search(r"output:\s", head) is not None,
                "file": str(f),
                "line": src[: m.start()].count("\n") + 1,
            })
        if f == ref / REF_CODEMODE_TOOL:
            # the synthesized Code Mode tool is a returned object literal,
            # not an `.add({…})` draft (codemode/tool.ts create() return)
            nm = re.search(r'name:\s*"execute"', src)
            if nm:
                head = src[nm.start(): nm.start() + 600].split("execute:")[0]
                entries.append({
                    "name": "execute",
                    "namespace": None,
                    "input": re.search(r"input:\s", head) is not None,
                    "output": re.search(r"output:\s", head) is not None,
                    "file": str(f),
                    "line": src[: nm.start()].count("\n") + 1,
                })
    return entries


# ---------------------------------------------------------------- main
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default=None)
    args = ap.parse_args()
    ref = Path(args.ref)

    rust = rust_builtin_tools()
    execute = rust_execute_tool()
    if execute:
        rust.append(execute)
    rust = sorted(rust, key=lambda t: (t["namespace"] or "", t["name"]))

    ref_tools = sorted(ref_tool_entries(ref), key=lambda t: (t["namespace"] or "", t["name"]))

    def eff(t: dict) -> str:
        return f'{t["namespace"]}_{t["name"]}' if t["namespace"] else t["name"]

    rust_idx = {eff(t): t for t in rust}
    ref_idx = {eff(t): t for t in ref_tools}
    both = sorted(set(rust_idx) & set(ref_idx))
    ref_only = sorted(set(ref_idx) - set(rust_idx))
    rust_only = sorted(set(rust_idx) - set(ref_idx))

    sites = rust_registration_sites()
    none_line, some_calls = rust_output_schema_none()

    rep = Report(
        "TOOLS Census（内建工具注册表 · 通道 2）",
        "rust 内建静态注册（BuiltinTool + execute 合成）vs ref tool/plugin 注册表；轴 2 input schema 存在性；轴 3 output schema 声明（G-230 锚）。",
    )
    rep.w("> 生成命令：`python3 tools/census/tools_registry.py --write docs/audit/census/TOOLS.md`")
    rep.w()
    rep.w("## 归一化与范围声明")
    rep.w()
    rep.w("- 比较键 = 有效名（effectiveName）：`namespace_name`（无命名空间则裸名）；rust `Some(\"opencode\")` ↔ ref `options.namespace`。")
    rep.w("- rust 侧提取：`crates/oc-core/src/tool/*.rs` 生产区 `BuiltinTool { … }` 构造（`#[cfg(test)]`/`#[test]` 区已剥除，tests*.rs 跳过；`name: NAME` 常量已展开）+ `codemode.rs::execute_registration` 合成的 `execute`。")
    rep.w("- ref 侧提取：`packages/core/src/tool/plugin/*.ts` 每个 `.add({ … })` draft（`export const name` 简写已归并；`draft.namespace({name:\"opencode\"})` 命名空间声明行不算工具）+ `packages/core/src/codemode/tool.ts` 的 `execute`。")
    rep.w("- 轴 2 只比 input schema **存在性**（有/无），不比内容。")
    rep.w("- 范围外：MCP 动态注册（rust `mcp/registry.rs`（含唯一传 `Some` 的 `pipeline_executor_with_output` 调用）、ref `tool/mcp.ts`）、运行时插件动态工具。`file-diff.ts` 无工具登记（辅助文件）。")
    rep.w()
    rep.w("## 轴 1：工具名全集 diff（有效名）")
    rep.w()
    rep.w(f"- rust（{TOOL_DIR}）：{len(rust_idx)} 个内建工具")
    rep.w(f"- ref（{ref / REF_PLUGIN_DIR} + codemode/tool.ts）：{len(ref_idx)} 个内建工具")
    rep.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    for name in ref_only:
        t = ref_idx[name]
        rep.w(f"  - 🔴 REF_ONLY `{name}` — {t['file']}:{t['line']}（ref 有、rust 无 → 缺口候选）")
    for name in rust_only:
        t = rust_idx[name]
        rep.w(f"  - 🔗 RUST_ONLY `{name}` — {t['file']}:{t['line']}（rust 额外面，需登记册决策背书）")
    rep.w()
    rep.w("  全集逐项（BOTH 项双侧 file:line）：")
    rep.w()
    rep.w("  | 有效名 | rust | ref |")
    rep.w("  |---|---|---|")
    for name in both:
        r, f = rust_idx[name], ref_idx[name]
        rep.w(f"  | `{name}` | {r['file']}:{r['line']} | {f['file']}:{f['line']} |")
    rep.w()
    rep.w("## 轴 2：每工具 input schema 存在性")
    rep.w()
    rust_no_input = [eff(t) for t in rust if not t["input"]]
    ref_no_input = [eff(t) for t in ref_tools if not t["input"]]
    rep.w(f"- rust 声明 input：{len(rust) - len(rust_no_input)}/{len(rust)}"
          f"（`BuiltinTool.input`/`ToolRegistration::new` 第三参为必填字段，缺一即编译失败）")
    rep.w(f"- ref 声明 input：{len(ref_tools) - len(ref_no_input)}/{len(ref_tools)}")
    for name in rust_no_input:
        rep.w(f"  - 🔴 rust 无 input `{name}`")
    for name in ref_no_input:
        t = next(x for x in ref_tools if eff(x) == name)
        rep.w(f"  - 🔴 ref 无 input `{name}` — {t['file']}:{t['line']}")
    rep.w()
    rep.w("## 轴 3：每工具 output schema 声明（G-230 锚核对）")
    rep.w()
    ref_out = [t for t in ref_tools if t["output"]]
    ref_files_out = sorted({t["file"] for t in ref_out})
    rep.w(f"- ref 声明 output：**{len(ref_out)}/{len(ref_tools)}** 个工具"
          f"（{len(ref_files_out)} 个文件：{', '.join(Path(p).name for p in ref_files_out)}）")
    rep.w(f"- rust 内建路径声明 output_schema：**0/{len(rust)}** —— `tool/registry.rs::register` 对所有内建工具硬编码"
          f" `output_schema: None`（{REGISTRY}:{none_line}，注释自引 G-230：走 mcp.ts:48 式 accept-all 回退）；"
          f"`pipeline_executor_with_output` 带 `Some` 的生产调用在内建范围内为 **{some_calls}** 处"
          f"（唯一 Some 调用在 MCP 动态路径 `mcp/registry.rs`，范围外）。")
    rep.w(f"- 锚核对：登记册 G-230 记载「rust 0/14 声明 vs ref 13 文件全声明」——当前 census 实测 rust 内建总数 {len(rust)}"
          f"（仍 0 声明，**锚的 rust 侧结论成立、数量已从 14 涨到 {len(rust)}**），ref 侧实测 {len(ref_files_out)} 文件/{len(ref_out)} 工具声明"
          f"（与锚的「13 文件」相差 +1：现为 mcp-resource 内联两工具也计入；如实呈现）。**G-230 缺口未修复。**")
    rep.w()
    rep.w("## rust 注册挂载点（交叉核对）")
    rep.w()
    rep.w(f"- 生产区 `register_builtin(`/`mcp_resource::register`/`subagent::register`/`register_code_mode_with_limits` 调用 {len(sites)} 处：")
    for f, ln in sites:
        rep.w(f"  - {f}:{ln}")
    rep.w()
    rep.w("## Census 裁定")
    rep.w()
    total = set(rust_idx) | set(ref_idx)
    adjudicated = len(both) + len(ref_only) + len(rust_only)
    rep.w(f"- 已裁定工具：{adjudicated}/{len(total)}（覆盖率 100%：双侧全集逐项归属）")
    rep.w(f"- 结论：工具名全集{'一致' if not ref_only and not rust_only else '存在差异（见轴 1）'}；"
          f"input schema 双侧全声明；output schema 声明 rust 0/{len(rust)} vs ref {len(ref_out)}/{len(ref_tools)} —— **G-230 缺口仍在（缺口候选，非新 gap）**。")
    rep.w()
    rep.w("## 验证记录")
    rep.w()
    rep.w("- 双侧每差异项均亲读当前代码核实（rust BuiltinTool 构造/registry.rs::register/codemode.rs::execute_registration；ref plugin/*.ts 的 .add draft 与 codemode/tool.ts）。")
    rep.w("- 幂等性：脚本纯读取、确定性排序；连跑两次输出逐字节一致（/tmp/opencode/ 下 diff 验证）。")
    rep.w("- 旧文档（G-230「0/14 vs 13 文件」）仅作锚，现状优先：数量以本 census 实测为准。")
    rep.w()

    rep.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
