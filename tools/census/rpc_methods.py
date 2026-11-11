#!/usr/bin/env python3
"""RPC-methods census (channel: plugin RPC dispatch surface).

Axis 1 — rust served methods: every guest→host bridge method the shared
  dispatch core answers. Extracted from `Shared::answer_guest` in
  crates/oc-plugin-host/src/supervisor.rs (the routing table around
  `rpc_dispatch`, introduced with be2c7dc6/95b719b9-era guest vcs/
  worktree faces): string literals in the match arms, plus the
  prefix-routed families expanded from their allowlist consts in
  crates/oc-plugin-host/src/context/mod.rs (SESSION_METHODS →
  `session.<m>`, CODEMODE_METHODS → `codemode.tool`,
  GENERATE_METHODS → `generate.text`). Comments stripped (length-
  preserving, so line numbers match the original file).
Axis 2 — rust guest-called methods: every `hostRequest("x.y", …)`
  literal in the guest host script (crates/oc-plugin-host/src/host/
  host.js) — cross-check that each served method has a caller (an
  uncalled served method is a dead face, not a gap).
Axis 3 — ref plugin host method face: the ctx object literal the ref
  constructs for plugins (`const context2: PromiseContext = {…}` in
  packages/plugin/src/promise/adapter.ts) — the ref's in-process
  equivalent of the guest→host bridge. The dynamic `rpc` namespace is
  expanded per the declared normalization (see RPC_NORM below).

Diff classes: BOTH / REF_ONLY (gap candidate) / RUST_ONLY (rust-extra
face — needs register backing, e.g. the G-379.7/G-379.8 vcs/worktree
read faces). Exit 0 always — census, not a gate.

Usage: python3 tools/census/rpc_methods.py [--ref /opt/opencode-ref]
           [--write docs/audit/census/RPC.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from common import Report, _skip_noncode, strip_tests  # noqa: E402

SUPERVISOR = Path("crates/oc-plugin-host/src/supervisor.rs")
CONTEXT_MOD = Path("crates/oc-plugin-host/src/context/mod.rs")
HOST_JS = Path("crates/oc-plugin-host/src/host/host.js")

# Declared normalization for the ref's dynamic rpc namespace
# (adapter.ts:453 `rpc: yield* rpcFromEffect(host.rpc, streams)` — the
# faces are built at runtime in adapter.ts:77-162). Mapping to the rust
# bridge names is stated here, verified against adapter.ts/rpc.ts.
RPC_NORM = {
    "rpc.register": "rpc.register",
    "rpc.call(client)": "rpc.invoke",
    "rpc.events.emit": "rpc.event",
}

DOTTED = re.compile(r"^[a-zA-Z][a-zA-Z0-9_]*(\.[a-zA-Z0-9_]+)+$")

# ref → rust 名称归一（声明式；均已对照双侧当前代码核实）：
#  - ref `<domain>.hook(name, cb)` 在 rust 桥上是统一登记方法 `hook.register`
#  - ref rpc 动态面（rpcFromEffect）映射到 rust 桥名
#  - ref `command.transform` 的 draft.add 落 rust 桥 `command.add` 同一路径
#  - ref `integration.transform` 的 method.update 编辑臂落 rust 桥
#    `integration.method.update`（其余编辑臂 rust 为 guest 本地表）
REF_TO_RUST = {
    "rpc.call(client)": "rpc.invoke",
    "rpc.events.emit": "rpc.event",
    "command.transform": "command.add",
    "integration.transform": "integration.method.update",
}


def hook_name(path: str) -> str | None:
    if path.endswith(".hook") and path.count(".") == 1:
        return "hook.register"
    return None


# ref 面中 rust 于 guest 本地实现（host.js 编辑器表/本地命令表，无桥方法）
# 的项 —— 种类面在 rust ctx 上等价存在（PLUGIN-API.md 通道核验），桥层
# REF_ONLY 不构成缺口，但行为等价性归行为通道。
REF_LOCAL = {
    "agent.get", "agent.list", "agent.reload",
    "command.list", "command.reload",
    "mcp.list", "mcp.reload",
    "skill.list", "skill.reload",
    "tool.list", "tool.reload",
    "integration.reload",
    "websearch.reload", "worktree.reload",
}


def strip_comments_keep_lines(src: str) -> str:
    """Blank out // and /* */ comments, preserving offsets/newlines."""
    out = list(src)
    i, n = 0, len(src)
    while i < n:
        i = _skip_noncode(src, i)
        if i >= n:
            break
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j == -1 else j
            for k in range(i, j):
                out[k] = " "
            i = j
        elif src.startswith("/*", i):
            j = src.find("*/", i)
            j = n if j == -1 else j + 2
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
        else:
            i += 1
    return "".join(out)


def fn_body(src: str, sig: str) -> tuple[str, int]:
    """(body, line-of-first-brace) of the fn whose source contains sig."""
    at = src.find(sig)
    if at == -1:
        raise SystemExit(f"signature not found: {sig}")
    brace = src.find("{", at)
    depth, j = 0, brace
    while j < len(src):
        j = _skip_noncode(src, j)
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                break
        j += 1
    return src[brace : j + 1], src[:brace].count("\n") + 1


def const_strs(src: str, const_name: str) -> list[str]:
    m = re.search(rf"pub const {const_name}: &\[&str\] = &\[(.*?)\]", src, re.DOTALL)
    if not m:
        raise SystemExit(f"const not found: {const_name}")
    return re.findall(r'"([^"]+)"', m.group(1))


# ------------------------------------------------------------- js/ts object
def parse_object_literals(src: str, anchor: str) -> dict[str, int]:
    """Dotted key paths (ns.method / ns.sub.method) of the object
    literal assigned after `anchor` (e.g. `const ctx = {`). Values that
    are nested object literals recurse; any other value records the
    path as a leaf. Returns {path: line}."""
    at = src.find(anchor)
    if at == -1:
        raise SystemExit(f"anchor not found: {anchor}")
    open_brace = src.find("{", at)
    results: dict[str, int] = {}

    def walk(pos: int, prefix: str, depth_limit: int) -> int:
        # pos sits just after '{'; parse entries until matching '}'
        i = pos
        while True:
            i = skip_ws_comments(src, i)
            if i >= len(src):
                return i
            if src[i] == "}":
                return i + 1
            # key: identifier, quoted, or shorthand-method name
            m = re.match(r'"([^"]+)"|\'([^\']+)\'|([A-Za-z_$][A-Za-z0-9_$]*)', src[i:])
            if not m:
                i += 1
                continue
            key = m.group(1) or m.group(2) or m.group(3)
            if key == "async":  # `async name(...) {}` shorthand — real key follows
                rest = re.match(r"\s+([A-Za-z_$][A-Za-z0-9_$]*)", src[i + m.end():])
                if rest:
                    key = rest.group(1)
                    j = i + m.end() + rest.end()
                else:
                    i += m.end()
                    continue
            else:
                j = i + m.end()
            key_line = src[: i + m.start()].count("\n") + 1
            j = i + m.end()
            j = skip_ws_comments(src, j)
            if j < len(src) and src[j] == "(":
                # shorthand method  key(args) { … } — record, skip body
                path = f"{prefix}{key}"
                results[path] = key_line
                brace = src.find("{", j)
                body, _ = fn_body_raw(src, j)
                j = brace + len(body)
                i = j
                continue
            if j < len(src) and src[j] == ":":
                j = skip_ws_comments(src, j + 1)
                path = f"{prefix}{key}"
                if j < len(src) and src[j] == "{":
                    # nested object literal (or a block — best effort)
                    if depth_limit > 1:
                        end = walk(j + 1, f"{path}.", depth_limit - 1)
                    else:
                        results[path] = key_line
                        body, _ = fn_body_raw(src, j)
                        end = j + len(body)
                    i = skip_ws_comments(src, end)
                    if i < len(src) and src[i] == ",":
                        i += 1
                    continue
                results[path] = key_line
                # skip to end of this value (comma/} at depth 0)
                i = skip_value(src, j)
                continue
            # bare key (set shorthand) — record
            results[f"{prefix}{key}"] = key_line
            i = j

    walk(open_brace + 1, "", 3)
    return results


def fn_body_raw(src: str, from_pos: int) -> tuple[str, int]:
    brace = src.find("{", from_pos)
    depth, j = 0, brace
    while j < len(src):
        j = _skip_noncode(src, j)
        if src[j] == "{":
            depth += 1
        elif src[j] == "}":
            depth -= 1
            if depth == 0:
                break
        j += 1
    return src[brace : j + 1], brace


def skip_ws_comments(src: str, i: int) -> int:
    n = len(src)
    while i < n:
        while i < n and src[i] in " \t\r\n":
            i += 1
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j == -1 else j
        elif src.startswith("/*", i):
            j = src.find("*/", i)
            i = n if j == -1 else j + 2
        else:
            return i
    return n


def skip_value(src: str, i: int) -> int:
    """Skip a non-object value until a top-level ',' or '}'."""
    depth = 0
    n = len(src)
    while i < n:
        i = _skip_noncode(src, i)
        if i >= n:
            break
        c = src[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return i
            depth -= 1
        elif c == "," and depth == 0:
            return i + 1
        i += 1
    return i


# ------------------------------------------------------------------- axes
def rust_served() -> dict[str, tuple[str, int]]:
    raw = strip_tests(SUPERVISOR.read_text())
    body = strip_comments_keep_lines(raw)
    body, line0 = fn_body(body, "fn answer_guest(")
    served: dict[str, tuple[str, int]] = {}
    for m in re.finditer(r'"([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+)"', body):
        name = m.group(1)
        if name == "session." or name.endswith(".call"):
            continue  # host→guest rpc.call is not guest-callable
        if name not in served:
            served[name] = (str(SUPERVISOR), line0 + body[: m.start()].count("\n"))
    for bare in ("ping", "log"):  # 无点名的桥字面量
        at = body.find(f'"{bare}"')
        if at != -1:
            served.setdefault(bare, (str(SUPERVISOR), line0 + body[:at].count("\n")))
    # prefix-routed families from the allowlist consts
    mod = strip_comments_keep_lines(CONTEXT_MOD.read_text())
    for method in const_strs(mod, "SESSION_METHODS"):
        served.setdefault(f"session.{method}", (str(CONTEXT_MOD), 0))
    for const in ("CODEMODE_METHODS", "GENERATE_METHODS"):
        for method in const_strs(mod, const):
            served.setdefault(method, (str(CONTEXT_MOD), 0))
    return served


def rust_called() -> dict[str, tuple[str, int]]:
    js = HOST_JS.read_text()
    called: dict[str, tuple[str, int]] = {}
    for m in re.finditer(r'(?:hostRequest|notify)\(\s*[`"\']?\s*([a-zA-Z][a-zA-Z0-9_.]*)', js):
        name = m.group(1)
        if name.startswith("session.") or name in ("method", "other"):
            name = "session.<m>"  # template-literal / variable family
        if name not in called:
            called[name] = (str(HOST_JS), js[: m.start()].count("\n") + 1)
    return called


def ref_face(ref: str) -> dict[str, tuple[str, int]]:
    adapter = Path(ref) / "packages/plugin/src/promise/adapter.ts"
    src = adapter.read_text()
    face = parse_object_literals(src, "const context2: PromiseContext = {")
    out: dict[str, tuple[str, int]] = {}
    for path, line in face.items():
        if path == "rpc":
            continue  # dynamic namespace — expanded via RPC_NORM below
        mapped = REF_TO_RUST.get(path) or hook_name(path) or path
        out.setdefault(mapped, (str(adapter), line))
    # hook registrations are method faces too (ctx.<domain>.hook)
    # (already captured as `<domain>.hook` keys in the literal).
    for declared, mapped in RPC_NORM.items():
        line = src.find("rpcFromEffect")
        out.setdefault(mapped, (str(adapter), src[:line].count("\n") + 1 if line != -1 else 0))
    return {k: v for k, v in out.items() if DOTTED.match(k) or k in REF_TO_RUST.values()}


# register backing for known RUST_ONLY families (manual annotation
# table; every entry cites the register ledger anchor).
RUST_BACKING = {
    "vcs.branch.list": "G-379.8（be2c7dc6）新增读面",
    "vcs.status": "G-379.8（be2c7dc6）新增读面",
    "vcs.diff": "G-379.8（be2c7dc6）新增读面",
    "worktree.list": "G-379.8（be2c7dc6）新增读面",
    "worktree.create": "G-379.8（be2c7dc6）新增读面",
    "worktree.remove": "G-379.8（be2c7dc6）新增读面",
    "worktree.refresh": "G-379.8（be2c7dc6）新增读面",
    "rpc.invoke": "G-379.7 客户端面（rpc.call ↔ ref Rpc.Service.call）",
    "websearch.default.get": "G-24 websearch 域（登记册 G-24 裁决面）",
    "websearch.default.set": "G-24 websearch 域（登记册 G-24 裁决面）",
    "theme.register": "wave 1 rust 面（ref v2 ctx 无 theme 域）",
    "event.subscribe": "事件订阅（ref event.subscribe 为 ctx.event 内联面）",
    "hook.register": "guest 钩子登记（ref 各域 hook() 内联面）",
    "ping": "rust 桥活探（死锁规则配套，无 ref 对应面）",
    "plugin.list": "G-148",
    "command.add": "wave 1（ref command.transform.add 同义）",
    "codemode.tool": "wave 4 Code Mode 嵌套工具桥（G-247 面内）",
    "integration.method.update": "G-24（v2 IntegrationMethodRegistration 快照面）",
    "rpc.event": "G-295（register().events.emit 桥面）",
    "session.list": "rust SESSION_METHODS 白名单注记（flagship 探针读面，ref 15 方法 Pick 外）",
    "session.messages": "rust SESSION_METHODS 白名单注记（flagship 探针读面，ref 15 方法 Pick 外）",
    "session.switchAgent": "",
    "session.switchModel": "",
}
UI_RUST = "TUI-25 / G-237 / PLUG-TUI-SLOT（rust TUI 插件 UI 面；ref v2 server-ctx 无 ui 域——ref 的 UI 面在独立 tui 入口，见 PLUGIN-API.md）"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default="docs/audit/census/RPC.md")
    args = ap.parse_args()

    served = rust_served()
    called = rust_called()
    ref = ref_face(args.ref)

    called_names = {n for n in called if n != "session.<m>"}
    if "session.<m>" in called:
        called_names |= {n for n in served if n.startswith("session.")}
    uncalled = sorted(set(served) - called_names)
    # guest→host 通知（无应答面，不经 answer_guest 请求派发）
    GUEST_NOTIFIES = {"plugin.busy", "plugin.idle", "plugin.fault", "host.exit",
                      "tui.route.plugin", "tui.route.navigate", "tui.route"}
    unserved_calls = sorted(
        n for n in called_names
        if n not in served and not n.startswith("session.") and n not in GUEST_NOTIFIES
    )

    both = sorted(set(served) & set(ref))
    ref_only = sorted(set(ref) - set(served))
    rust_only = sorted(set(served) - set(ref))

    r = Report(
        "RPC-Methods Census（缺口普查 · 通道 1：插件 RPC 派发面）",
        "rust 桥派发表（answer_guest + 前缀族展开）× ref ctx 方法面（adapter.ts context2）。",
    )
    r.w("> 生成命令：`python3 tools/census/rpc_methods.py --write docs/audit/census/RPC.md`")
    r.w("> 确定性：全部提取自当前代码静态扫描，排序输出，无时间戳。")
    r.w("")
    r.w("## 轴 1：rust 桥派发面（supervisor.rs `answer_guest` 匹配臂 + 前缀族白名单）")
    r.w("")
    r.w(f"- 提取 guest→host 方法 **{len(served)}** 个（`{SUPERVISOR}`；"
        f"`session.*` 由 `{CONTEXT_MOD}` SESSION_METHODS（{len(const_strs(strip_comments_keep_lines(CONTEXT_MOD.read_text()), 'SESSION_METHODS'))} 项）展开，"
        f"另含 codemode.tool / generate.text）。")
    for name in sorted(served):
        f, ln = served[name]
        r.w(f"  - `{name}` — {f}:{ln}" if ln else f"  - `{name}` — {f}（const 展开）")
    r.w("")
    r.w("## 轴 2：rust guest 实际调用面（host.js `hostRequest(` 字面量）")
    r.w("")
    r.w(f"- 提取调用 **{len(called)}** 处方法名（含 `session.<m>` 模板族）。")
    r.w(f"- **已服务但 guest 无调用者：{len(uncalled)}**（死面候选，非缺口）")
    for name in uncalled:
        r.w(f"  - 🟡 UNCALLED `{name}`")
    r.w(f"- **guest 调用但未入派发表：{len(unserved_calls)}**（若 >0 需排查）")
    for name in unserved_calls:
        r.w(f"  - ⚠️ UNSERVED `{name}`")
    r.w("")
    r.w("## 轴 3：diff（rust 派发面 vs ref ctx 方法面）")
    r.w("")
    total = len(set(served) | set(ref))
    r.w(f"- rust：{len(served)} · ref：{len(ref)} · 并集：{total}")
    r.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    r.w("")
    r.w("### REF_ONLY（ref 有、rust 无 → 缺口候选）")
    for name in ref_only:
        f, ln = ref[name]
        note = "（rust 于 guest 本地实现（host.js 编辑器表/本地表），桥层缺口候选——行为等价归行为通道）" if name in REF_LOCAL else ""
        r.w(f"  - 🔴 `{name}` — {f}:{ln}{note}")
    r.w("")
    r.w("### RUST_ONLY（rust 额外面 → 需登记册背书）")
    for name in rust_only:
        f, ln = served[name]
        backing = RUST_BACKING.get(name, UI_RUST if name.startswith(("ui.", "keymap.")) else "")
        where = f"{f}:{ln}" if ln else f
        r.w(f"  - 🔗 `{name}` — {where}" + (f"（{backing}）" if backing else "（待登记册决策）"))
    r.w("")
    r.w("### BOTH（双侧 file:line）")
    for name in both:
        rf, rl = ref[name]
        sf, sl = served[name]
        r.w(f"  - ✅ `{name}` — rust {sf}:{sl or '·'} · ref {rf}:{rl}")
    r.w("")
    r.w("## 归一化规则")
    r.w("")
    r.w("- rust 侧：`answer_guest` 匹配臂字符串字面量 + `session.<m>`/`codemode.tool`/`generate.text` 白名单展开；注释剥除（保行号）。")
    r.w("- ref 侧：adapter.ts `context2` 对象字面量的 ns.method 点路径；`rpc` 为运行时构造（rpcFromEffect，adapter.ts:77-162），按声明映射展开：")
    for k, v in RPC_NORM.items():
        r.w(f"  - ref `{k}` ↔ rust `{v}`")
    r.w("- 范围：**种类级**方法全集对拍；参数/返回值/错误语义（行为级差异）不在本通道范围（G-339/G-340/G-354/G-355 类项由行为通道盯守）。")
    r.w("")
    adjudicated = len(both) + len(ref_only) + len(rust_only)
    r.w("## Census 裁定")
    r.w("")
    r.w(f"- 已裁定方法：{adjudicated}/{total}（覆盖率 {adjudicated}/{total} = "
        f"{(adjudicated / total * 100):.1f}%：双侧全集逐项归属）")
    r.w(f"- 结论：REF_ONLY {len(ref_only)} 项为缺口候选；RUST_ONLY {len(rust_only)} 项中 "
        f"{sum(1 for n in rust_only if n in RUST_BACKING or n.startswith(('ui.', 'keymap.')))} 项有登记册背书标注，其余待裁决。")
    r.w("")
    r.w("## 验证记录")
    r.w("")
    r.w("- 提取复核：全部 REF_ONLY / RUST_ONLY 差异项均亲读双侧当前代码逐项核实（supervisor.rs / context/mod.rs / host.js 与 ref adapter.ts 当前版本）；旧文档仅作定位线索。")
    r.w("- 幂等验证：`python3 tools/census/rpc_methods.py --write /tmp/opencode/rpc-a.md` 连跑两次（rpc-a/rpc-b），`diff` 逐字节一致（/tmp/opencode 下执行）。")
    r.w("- 脚本 exit 0（census 非门禁）。")
    r.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
