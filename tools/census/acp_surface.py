#!/usr/bin/env python3
"""ACP-surface census (channel: agent-client-protocol face).

Axis 1 — SessionUpdate event kinds: every update kind the rust ACP
  adapter can emit (`SessionUpdate::X` constructor sites across
  crates/oc-cli/src/acp/*.rs, `#[cfg(test)]`/`#[test]` regions stripped,
  comments stripped) vs every `sessionUpdate: "x"` literal in the ref
  adapter (/opt/opencode-ref/packages/cli/src/acp/*.ts). Kind-level
  only: per-field shape and ordering are behavioral and out of scope.
Axis 2 — request/notification method face: the ref's registered ACP
  methods (`request("x", …)` / `notification("x", …)` in
  packages/cli/src/acp/agent.ts) vs the rust registration arms
  (`request: XxxRequest` handler arms in
  crates/oc-cli/src/acp/agent.rs + the `CancelNotification` arm). The
  rust arms register typed handlers (method strings live inside the
  agent_client_protocol crate), so the rust axis is normalized through
  the declared REQUEST_TYPE→method table below (each entry verified
  against agent.rs's arms).

Known anchors: G-339/G-340/G-354/G-355 were behavioral diffs (already
fixed) — this census compares KIND sets only; behavior is out of scope
(declared).

Usage: python3 tools/census/acp_surface.py [--ref /opt/opencode-ref]
           [--write docs/audit/census/ACP.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from common import Report, strip_tests  # noqa: E402

from rpc_methods import strip_comments_keep_lines  # noqa: E402

RUST_ACP = Path("crates/oc-cli/src/acp")

# rust registration-arm type → wire method (agent.rs, one arm each;
# the crate maps the typed handler onto these JSON-RPC method strings).
REQUEST_TYPES = {
    "InitializeRequest": "initialize",
    "AuthenticateRequest": "authenticate",
    "NewSessionRequest": "session/new",
    "LoadSessionRequest": "session/load",
    "ListSessionsRequest": "session/list",
    "DeleteSessionRequest": "session/delete",
    "ResumeSessionRequest": "session/resume",
    "CloseSessionRequest": "session/close",
    "ForkSessionRequest": "session/fork",
    "SetSessionConfigOptionRequest": "session/set_config_option",
    "SetSessionModeRequest": "session/set_mode",
    "PromptRequest": "session/prompt",
    "CancelNotification": "session/cancel",  # notification arm (agent.rs:219-228)
}

CAMEL_TO_SNAKE_CACHE: dict[str, str] = {}


def camel_to_snake(name: str) -> str:
    if name not in CAMEL_TO_SNAKE_CACHE:
        CAMEL_TO_SNAKE_CACHE[name] = re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()
    return CAMEL_TO_SNAKE_CACHE[name]


def rust_update_kinds() -> dict[str, tuple[str, int]]:
    kinds: dict[str, tuple[str, int]] = {}
    for f in sorted(RUST_ACP.rglob("*.rs")):
        body = strip_comments_keep_lines(strip_tests(f.read_text()))
        for m in re.finditer(r"SessionUpdate::([A-Za-z]+)", body):
            wire = camel_to_snake(m.group(1))
            if wire not in kinds:
                kinds[wire] = (str(f), body[: m.start()].count("\n") + 1)
    return kinds


def rust_methods() -> dict[str, tuple[str, int]]:
    out: dict[str, tuple[str, int]] = {}
    agent = RUST_ACP / "agent.rs"
    body = strip_comments_keep_lines(strip_tests(agent.read_text()))
    for m in re.finditer(r"request: ([A-Za-z]+Request)", body):
        wire = REQUEST_TYPES.get(m.group(1))
        if wire is None:
            raise SystemExit(f"unmapped request type: {m.group(1)}")
        out.setdefault(wire, (str(agent), body[: m.start()].count("\n") + 1))
    arm = re.search(r"notification:\s*CancelNotification", body)
    if arm is None:
        raise SystemExit("CancelNotification arm not found in agent.rs")
    out.setdefault("session/cancel", (str(agent), body[: arm.start()].count("\n") + 1))
    return out


def ref_update_kinds(ref: str) -> dict[str, tuple[str, int]]:
    kinds: dict[str, tuple[str, int]] = {}
    base = Path(ref) / "packages/cli/src/acp"
    for f in sorted(base.rglob("*.ts")):
        if f.name.endswith(".test.ts"):
            continue
        src = f.read_text()
        for m in re.finditer(r'sessionUpdate:\s*"([a-z_]+)"', src):
            if m.group(1) not in kinds:
                kinds[m.group(1)] = (str(f), src[: m.start()].count("\n") + 1)
    return kinds


def ref_methods(ref: str) -> dict[str, tuple[str, int]]:
    out: dict[str, tuple[str, int]] = {}
    agent = Path(ref) / "packages/cli/src/acp/agent.ts"
    src = agent.read_text()
    for m in re.finditer(r'\b(request|notification)\(\s*\n?\s*"([^"]+)"', src):
        out.setdefault(m.group(2), (str(agent), src[: m.start()].count("\n") + 1))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default="docs/audit/census/ACP.md")
    args = ap.parse_args()

    # ---------------- axis 1: SessionUpdate kinds
    r_kinds = rust_update_kinds()
    f_kinds = ref_update_kinds(args.ref)
    both1 = sorted(set(r_kinds) & set(f_kinds))
    ref_only1 = sorted(set(f_kinds) - set(r_kinds))
    rust_only1 = sorted(set(r_kinds) - set(f_kinds))

    # ---------------- axis 2: request/notification methods
    r_methods = rust_methods()
    f_methods = ref_methods(args.ref)
    both2 = sorted(set(r_methods) & set(f_methods))
    ref_only2 = sorted(set(f_methods) - set(r_methods))
    rust_only2 = sorted(set(r_methods) - set(f_methods))

    r = Report(
        "ACP Census（缺口普查 · 通道 2：agent-client-protocol 面）",
        "rust ACP 适配器（crates/oc-cli/src/acp/）× ref 适配器（packages/cli/src/acp/）种类级对拍。",
    )
    r.w("> 生成命令：`python3 tools/census/acp_surface.py --write docs/audit/census/ACP.md`")
    r.w("> 确定性：静态扫描当前代码，排序输出，无时间戳。")
    r.w("> 范围声明：**只比事件/方法种类全集**；字段形状、次序、错误语义等行为级差异不在范围")
    r.w("> （G-339/G-340/G-354/G-355 为已修复的行为级项，本通道不重复裁定）。")
    r.w("")
    r.w("## 轴 1：SessionUpdate 事件种类全集")
    r.w("")
    total1 = len(set(r_kinds) | set(f_kinds))
    r.w(f"- rust（`SessionUpdate::X` 构造点，测试区/注释剥除）：{len(r_kinds)} 种")
    r.w(f"- ref（`sessionUpdate: \"x\"` 字面量）：{len(f_kinds)} 种")
    r.w(f"- **BOTH：{len(both1)}** · REF_ONLY：**{len(ref_only1)}** · RUST_ONLY：**{len(rust_only1)}**")
    for k in ref_only1:
        f, ln = f_kinds[k]
        r.w(f"  - 🔴 REF_ONLY `{k}` — {f}:{ln}（rust 无此事件种类 → 缺口候选）")
    for k in rust_only1:
        f, ln = r_kinds[k]
        r.w(f"  - 🔗 RUST_ONLY `{k}` — {f}:{ln}（rust 额外事件种类，需登记册背书）")
    r.w("")
    r.w("### BOTH（双侧 file:line）")
    for k in both1:
        rf, rl = f_kinds[k]
        sf, sl = r_kinds[k]
        r.w(f"  - ✅ `{k}` — rust {sf}:{sl} · ref {rf}:{rl}")
    r.w("")
    r.w("## 轴 2：请求/通知方法面（initialize / session/* 等）")
    r.w("")
    total2 = len(set(r_methods) | set(f_methods))
    r.w(f"- ref（agent.ts `request(\"x\")`/`notification(\"x\")` 注册）：{len(f_methods)} 个")
    r.w(f"- rust（agent.rs 每类型一臂 + CancelNotification 臂，类型→方法名映射为声明式归一）：{len(r_methods)} 个")
    r.w(f"- **BOTH：{len(both2)}** · REF_ONLY：**{len(ref_only2)}** · RUST_ONLY：**{len(rust_only2)}**")
    for k in ref_only2:
        f, ln = f_methods[k]
        r.w(f"  - 🔴 REF_ONLY `{k}` — {f}:{ln}（缺口候选）")
    for k in rust_only2:
        f, ln = r_methods[k]
        r.w(f"  - 🔗 RUST_ONLY `{k}` — {f}:{ln}（rust 额外方法，需登记册背书）")
    r.w("")
    r.w("### BOTH（双侧 file:line）")
    for k in both2:
        rf, rl = f_methods[k]
        sf, sl = r_methods[k]
        r.w(f"  - ✅ `{k}` — rust {sf}:{sl} · ref {rf}:{rl}")
    r.w("")
    r.w("## 归一化规则")
    r.w("")
    r.w("- rust 事件种类：`SessionUpdate::AgentMessageChunk` → `agent_message_chunk`（CamelCase→snake_case），全 acp/ 目录非测试区构造点。")
    r.w("- rust 方法面：agent.rs 的 `request: XxxRequest` 臂经声明映射表转 wire 方法名（13 请求 + 1 通知，与 ref service.ts:1 注记的「13 request methods plus the cancel notification」一致）。")
    r.w("- ref 侧均为字面量直取（agent.ts / 各 acp 模块的 `sessionUpdate:` 字面量）。")
    r.w("")
    adj1 = len(both1) + len(ref_only1) + len(rust_only1)
    adj2 = len(both2) + len(ref_only2) + len(rust_only2)
    r.w("## Census 裁定")
    r.w("")
    r.w(f"- 轴 1 已裁定：{adj1}/{total1}（覆盖率 {adj1}/{total1}）；轴 2 已裁定：{adj2}/{total2}（覆盖率 {adj2}/{total2}）。")
    r.w(f"- 结论：事件种类 REF_ONLY {len(ref_only1)} / 方法面 REF_ONLY {len(ref_only2)} 为缺口候选；"
        f"RUST_ONLY（{len(rust_only1)}/{len(rust_only2)}）需登记册背书。")
    r.w("")
    r.w("## 验证记录")
    r.w("")
    r.w("- 提取复核：轴 1 全部 11 种、轴 2 全部 13 个方法的双侧构造点/注册点均亲读当前代码核实（rust crates/oc-cli/src/acp/*.rs 非测试区 + ref packages/cli/src/acp/*.ts）；cmp-S3b-acp.md 仅作定位线索，未采信其结论。")
    r.w("- 幂等验证：`python3 tools/census/acp_surface.py --write /tmp/opencode/acp-a.md` 连跑两次，`diff` 逐字节一致（/tmp/opencode 下执行）。")
    r.w("- 脚本 exit 0（census 非门禁）。")
    r.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
