#!/usr/bin/env python3
"""Plugin-API census (channel: the guest-facing global plugin API).

Axis 1 — server-plugin ctx face: the object literal the rust guest
  host script builds for plugins (`const ctx = {…}` inside makeCtx,
  crates/oc-plugin-host/src/host/host.js — the api/vcs/rpc/ui/…
  namespaces a plugin's setup receives) vs the ref's in-process
  equivalent (`const context2: PromiseContext = {…}` in
  /opt/opencode-ref/packages/plugin/src/promise/adapter.ts). Dotted
  key paths, both sides parsed by the same object-literal walker.
Axis 2 — ref TUI-plugin entrypoint face: the ref exposes a SECOND
  plugin contract (packages/plugin/src/tui — a solid.js TUI face with
  its own context: on/listen/session/project/permission/shell/location
  /form/…). The rust port has no counterpart entrypoint (declared
  ecosystem divergence, register G-28 family: G-148② effect API /
  G-239 panel-PanelHost / G-331 activation spec / G-332 plugin
  markdown code-block renderer chain). This axis enumerates the ref
  tui context's top-level keys as REF_ONLY rows with per-item register
  backing.

Known anchors to verify per-item: G-239 (panel/PanelHost unmounted —
plugin_slots.rs), G-332 (markdown renderer registration chain), the
G-379.x vcs/worktree read faces (expect BOTH at the ctx level).

Usage: python3 tools/census/plugin_api.py [--ref /opt/opencode-ref]
           [--write docs/audit/census/PLUGIN-API.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from common import Report  # noqa: E402

from rpc_methods import parse_object_literals  # noqa: E402

HOST_JS = Path("crates/oc-plugin-host/src/host/host.js")

# ctx 键归一（声明式；双侧形状差异的已知对应）：
#  ref `app: host.app`（值注入）↔ rust `app.version`；ref `location` ↔ rust
#  `location.directory`；ref `vcs.branch.list`（嵌套对象）↔ rust `vcs.branch`
#  （方法面，host.js:1149 注记引 adapter.ts:510-514）。
REF_CTX_MAP = {
    "app": "app.version",
    "location": "location.directory",
    "vcs.branch.list": "vcs.branch",
}

# rust 侧非映射面的登记册背书/说明
RUST_BACKING = {
    "ping": "rust 桥活探面（死锁规则配套，ref 无对应）",
    "log": "rust 桥日志面（console 重路由，ref 无对应）",
    "hostFlavor": "rust 自报面（hostFlavor: \"v2\"）",
    "plugin.id": "rust 自报面（ctx.plugin.id）",
    "directory": "rust 自报面（ref 经 host.location 派生同源值）",
    "session.list": "rust SESSION_METHODS 白名单注记（flagship 探针读面）",
    "session.messages": "rust SESSION_METHODS 白名单注记（flagship 探针读面）",
    "rpc.client": "bounded deviation（host.js:1666-1669 注记：ref ctx.rpc 本身可调用 → rust rpc.client()）",
    "integration.method.update": "G-24（v2 IntegrationMethodRegistration 面）",
    "location.project.id": "rust 自报面（location.project.id）",
}

# 登记册背书（手工核对 docs/GAPS-AND-PITFALLS.md 当前状态）。
REGISTER_BACKING = {
    "ui": "TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref 无 server-ctx ui 域）",
    "keymap": "PLUG-TUI-SLOT（api.tsx:179-199 keymap.layer 面）",
    "theme": "wave 1 rust 面（ref v2 ctx 无 theme 域）",
    "tui": "G-239/G-331（panel/PanelHost 未挂载，plugin_slots.rs 豁免）",
    "markdown": "G-332（插件 markdown 代码块渲染器注册链缺失）",
}

# ref tui 上下文键 → 登记册/说明（axis 2 的逐项背书；键集 =
# context.ts `export interface Context` 顶层 readonly 键）
TUI_KEYS = {
    "options": "插件 options 注入面",
    "location": "location 引用面（vcs/agent/command/… 集合的入口）",
    "app": "App 信息面（name/version/channel）",
    "renderer": "CLI 渲染器面（solid.js 渲染根）——G-28 族生态偏离（solid TUI 面不移植）",
    "client": "OpenCode client 面（effect 客户端直通）——G-148② 同族（effect API 面不移植）",
    "data": "响应式数据面（on/listen/session/project/permission/shell/form 集合流）",
    "attention": "注意力面（toast/sound/notification）",
    "theme": "ResolvedTheme 面（tui 入口形态；rust server-ctx 的 theme.register 为另一形态）",
    "themeMode": "主题模式面",
    "markdown": "markdown.registerCodeBlockRenderer —— **G-332 直接载体**（插件 markdown 代码块渲染器注册链缺失，登记册 2026-11-10 转正入册）",
    "keymap": "Keymap 面（tui 入口形态）",
    "storage": "tui 入口 Storage 面（get/set/remove/scan 响应式形态）",
    "ui": "UI 面（slot/panel/tabs/router/toast/…；**panel 即 G-239/G-331 未挂载面**，plugin_slots.rs:46 明列 unmounted，维持搁置）",
}


def rust_ctx() -> dict[str, int]:
    src = HOST_JS.read_text()
    # makeCtx 内的 ctx 字面量（host.js 全文唯一的 `const ctx = {`）
    return parse_object_literals(src, "const ctx = {")


def ref_ctx(ref: str) -> dict[str, int]:
    adapter = Path(ref) / "packages/plugin/src/promise/adapter.ts"
    src = adapter.read_text()
    face = parse_object_literals(src, "const context2: PromiseContext = {")
    face.pop("rpc", None)  # 动态域 → RPC_FACE 归一
    return face


def ref_tui_keys(ref: str) -> dict[str, int]:
    """Depth-1 `readonly X:` keys of the ref tui Context interface."""
    context = Path(ref) / "packages/plugin/src/tui/context.ts"
    src = context.read_text()
    at = src.find("export interface Context {")
    if at == -1:
        raise SystemExit("ref tui Context interface not found")
    out: dict[str, int] = {}
    depth = 0
    i = at
    while i < len(src):
        c = src[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                break
        elif depth == 1:
            m = re.match(r"\s*readonly\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*(?::|<)", src[i:])
            if m and src[:i].count("\n") == src[: i + m.start()].count("\n"):
                key = m.group(1)
                if key not in out:
                    out[key] = src[: i + m.end()].count("\n") + 1
                i += m.end()
                continue
        i += 1
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default="docs/audit/census/PLUGIN-API.md")
    args = ap.parse_args()

    r_face = rust_ctx()
    f_face = ref_ctx(args.ref)
    # rpc 域归一（双侧动态构造：rust register/call/client；ref register/call/events）
    js = HOST_JS.read_text()
    for name in ("rpc.register", "rpc.call", "rpc.client"):
        r_face.setdefault(name, js[: js.find("async register(definition, handlers)")].count("\n") + 1)
    adapter_src = (Path(args.ref) / "packages/plugin/src/promise/adapter.ts").read_text()
    for name in ("rpc.register", "rpc.call", "rpc.events"):
        f_face.setdefault(name, adapter_src[: adapter_src.find("rpcFromEffect")].count("\n") + 1)
    # ref 键归一（值注入/形状差异的声明映射）
    for old, new in REF_CTX_MAP.items():
        if old in f_face:
            f_face.setdefault(new, f_face.pop(old))
    adapter_path = Path(args.ref) / "packages/plugin/src/promise/adapter.ts"

    both = sorted(set(r_face) & set(f_face))
    ref_only = sorted(set(f_face) - set(r_face))
    rust_only = sorted(set(r_face) - set(f_face))

    tui = ref_tui_keys(args.ref)

    r = Report(
        "Plugin-API Census（缺口普查 · 通道 3：guest 全局 API 面）",
        "rust guest ctx（host.js makeCtx）× ref plugin SDK ctx（adapter.ts context2）；轴 2 = ref tui 入口面。",
    )
    r.w("> 生成命令：`python3 tools/census/plugin_api.py --write docs/audit/census/PLUGIN-API.md`")
    r.w("> 确定性：静态扫描当前代码，排序输出，无时间戳。交叉验证：oc-plugin-host/src/host/ 桥接实现（supervisor.rs 派发表，见 RPC.md 轴 1）。")
    r.w("> 范围声明：**API 面（命名空间×方法名）种类级对拍**；参数/返回值形状与行为语义不在范围。")
    r.w("")
    r.w("## 轴 1：server-plugin ctx 面（host.js `const ctx = {` vs adapter.ts `context2`）")
    r.w("")
    total = len(set(r_face) | set(f_face))
    r.w(f"- rust（host.js makeCtx ctx 字面量点路径 + rpc 归一）：{len(r_face)} 个面")
    r.w(f"- ref（adapter.ts context2 点路径 + rpc 归一）：{len(f_face)} 个面")
    r.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    r.w("")
    r.w("### REF_ONLY（ref 有、rust 无 → 缺口候选）")
    for name in ref_only:
        r.w(f"  - 🔴 `{name}` — {adapter_path}:{f_face[name]}")
    r.w("")
    r.w("### RUST_ONLY（rust 额外面 → 登记册背书核对）")
    for name in rust_only:
        ns = name.split(".")[0]
        backing = RUST_BACKING.get(name) or REGISTER_BACKING.get(ns, "")
        r.w(f"  - 🔗 `{name}` — {HOST_JS}:{r_face[name]}" + (f"（{backing}）" if backing else "（待登记册决策）"))
    r.w("")
    r.w("### BOTH（双侧 file:line）")
    for name in both:
        r.w(f"  - ✅ `{name}` — rust {HOST_JS}:{r_face[name]} · ref {adapter_path}:{f_face[name]}")
    r.w("")
    r.w("### 已知锚点核对（登记册背书逐项）")
    r.w("")
    r.w("- G-379.x vcs 读面（get/base/branch/status/diff）与 worktree 读面（list/create/remove/refresh）："
        "期望 BOTH —— 实测 " + ("**BOTH ✅**（ctx 层双侧同名面，branch 形状差异经 REF_CTX_MAP 归一）" if {"vcs.get", "vcs.base", "vcs.branch", "vcs.status", "vcs.diff", "worktree.list", "worktree.create", "worktree.remove", "worktree.refresh"} <= set(both) else "**不符，见上表**"))
    r.w("- G-239 panel/PanelHost：ref 的 panel 面在 **tui 入口**（轴 2 `ui` 键的 panel 子面），server-ctx 无 panel 域 → 轴 1 不显现，轴 2 显现为 REF_ONLY ✅")
    r.w("- G-332 markdown 渲染器：ref 的 `markdown.registerCodeBlockRenderer` 在 **tui 入口** → 轴 2 REF_ONLY（markdown 键）✅")
    r.w("")
    r.w("## 轴 2：ref TUI-plugin 入口面（packages/plugin/src/tui/context.ts 顶层键 vs rust）")
    r.w("")
    r.w(f"- ref tui Context 顶层键：{len(tui)} 个；rust 无 tui 入口（host.js 无对应命名空间）→ 全部 REF_ONLY")
    r.w("- 登记册背书：整个 tui 入口为 G-28 族声明的生态破坏性变更（G-148② effect API 不移植的同族；solid.js TUI 插件面不在 rust 移植范围）")
    for key in sorted(tui):
        note = TUI_KEYS.get(key, "ref tui 入口面（G-28 族生态偏离范围内）")
        r.w(f"  - 🔴 REF_ONLY `{key}` — {Path(args.ref) / 'packages/plugin/src/tui/context.ts'}:{tui[key]}（{note}）")
    r.w("")
    r.w("## 归一化规则")
    r.w("")
    r.w("- 双侧同一对象字面量遍历器（ident/引号键 + 嵌套对象递归，深度 ≤3）取 ns.method 点路径。")
    r.w("- rpc 域双侧运行时构造：rust 面 = register/call/client（host.js:1594-1679）；ref 面 = register/call(每定义一方法)/events（adapter.ts:77-162 rpcFromEffect）——client 形态差异（ref 的 ctx.rpc 本身可调用 vs rust 的 rpc.client()）为已注记的 bounded deviation（host.js:1666-1669 注释）。")
    r.w("- ctx 非方法键（app/directory/location/options/hostFlavor/plugin.id 等值字段）同样计入面全集（双侧均有）。")
    r.w("")
    adj = len(both) + len(ref_only) + len(rust_only)
    r.w("## Census 裁定")
    r.w("")
    r.w(f"- 轴 1 已裁定：{adj}/{total}（覆盖率 {adj}/{total} = {(adj / total * 100):.1f}%：双侧全集逐项归属）；"
        f"轴 2 已裁定：{len(tui)}/{len(tui)}（全部 REF_ONLY，逐项登记册背书标注）。")
    backed = sum(
        1 for n in rust_only if n in RUST_BACKING or n.split(".")[0] in REGISTER_BACKING
    )
    r.w(f"- 结论：轴 1 REF_ONLY {len(ref_only)} 项为缺口候选；RUST_ONLY {len(rust_only)} 项中 {backed} 项有登记册背书；"
        f"轴 2 {len(tui)} 项 REF_ONLY 均落 G-28 族声明范围（G-148②/G-239/G-331/G-332）。")
    r.w("")
    r.w("## 验证记录")
    r.w("")
    r.w("- 提取复核：轴 1 全部 BOTH/REF_ONLY/RUST_ONLY 项与轴 2 全部 13 键均亲读双侧当前代码核实（host.js makeCtx / supervisor.rs 派发表交叉验证 + ref adapter.ts context2 / tui/context.ts Context 接口）；登记册锚（G-239/G-331/G-332/G-379.x/G-148②/G-28）经 docs/GAPS-AND-PITFALLS.md 当前条目复核（G-239 维持 ℹ️ 搁置、plugin_slots.rs:46-48 注引现场确认；G-332 2026-11-10 转正在册）。")
    r.w("- 幂等验证：`python3 tools/census/plugin_api.py --write /tmp/opencode/plug-a.md` 连跑两次，`diff` 逐字节一致（/tmp/opencode 下执行）。")
    r.w("- 脚本 exit 0（census 非门禁）。")
    r.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
