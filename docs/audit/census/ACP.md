# ACP Census（缺口普查 · 通道 2：agent-client-protocol 面）

> rust ACP 适配器（crates/oc-cli/src/acp/）× ref 适配器（packages/cli/src/acp/）种类级对拍。

> 生成命令：`python3 tools/census/acp_surface.py --write docs/audit/census/ACP.md`
> 确定性：静态扫描当前代码，排序输出，无时间戳。
> 范围声明：**只比事件/方法种类全集**；字段形状、次序、错误语义等行为级差异不在范围
> （G-339/G-340/G-354/G-355 为已修复的行为级项，本通道不重复裁定）。

## 轴 1：SessionUpdate 事件种类全集

- rust（`SessionUpdate::X` 构造点，测试区/注释剥除）：11 种
- ref（`sessionUpdate: "x"` 字面量）：11 种
- **BOTH：11** · REF_ONLY：**0** · RUST_ONLY：**0**

### BOTH（双侧 file:line）
  - ✅ `agent_message_chunk` — rust crates/oc-cli/src/acp/replay.rs:130 · ref /opt/opencode-ref/packages/cli/src/acp/replay.ts:62
  - ✅ `agent_thought_chunk` — rust crates/oc-cli/src/acp/replay.rs:144 · ref /opt/opencode-ref/packages/cli/src/acp/replay.ts:67
  - ✅ `available_commands_update` — rust crates/oc-cli/src/acp/sessions.rs:425 · ref /opt/opencode-ref/packages/cli/src/acp/sessions.ts:78
  - ✅ `compaction_summary_chunk` — rust crates/oc-cli/src/acp/compaction.rs:90 · ref /opt/opencode-ref/packages/cli/src/acp/compaction.ts:79
  - ✅ `compaction_update` — rust crates/oc-cli/src/acp/compaction.rs:190 · ref /opt/opencode-ref/packages/cli/src/acp/compaction.ts:122
  - ✅ `config_option_update` — rust crates/oc-cli/src/acp/sessions.rs:456 · ref /opt/opencode-ref/packages/cli/src/acp/sessions.ts:87
  - ✅ `session_info_update` — rust crates/oc-cli/src/acp/compaction.rs:180 · ref /opt/opencode-ref/packages/cli/src/acp/compaction.ts:118
  - ✅ `tool_call` — rust crates/oc-cli/src/acp/child.rs:115 · ref /opt/opencode-ref/packages/cli/src/acp/replay.ts:73
  - ✅ `tool_call_update` — rust crates/oc-cli/src/acp/child.rs:121 · ref /opt/opencode-ref/packages/cli/src/acp/replay.ts:86
  - ✅ `usage_update` — rust crates/oc-cli/src/acp/turn.rs:655 · ref /opt/opencode-ref/packages/cli/src/acp/turn.ts:333
  - ✅ `user_message_chunk` — rust crates/oc-cli/src/acp/replay.rs:65 · ref /opt/opencode-ref/packages/cli/src/acp/replay.ts:42

## 轴 2：请求/通知方法面（initialize / session/* 等）

- ref（agent.ts `request("x")`/`notification("x")` 注册）：13 个
- rust（agent.rs 每类型一臂 + CancelNotification 臂，类型→方法名映射为声明式归一）：13 个
- **BOTH：13** · REF_ONLY：**0** · RUST_ONLY：**0**

### BOTH（双侧 file:line）
  - ✅ `authenticate` — rust crates/oc-cli/src/acp/agent.rs:51 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:72
  - ✅ `initialize` — rust crates/oc-cli/src/acp/agent.rs:41 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:68
  - ✅ `session/cancel` — rust crates/oc-cli/src/acp/agent.rs:223 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:116
  - ✅ `session/close` — rust crates/oc-cli/src/acp/agent.rs:133 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:96
  - ✅ `session/delete` — rust crates/oc-cli/src/acp/agent.rs:105 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:88
  - ✅ `session/fork` — rust crates/oc-cli/src/acp/agent.rs:144 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:100
  - ✅ `session/list` — rust crates/oc-cli/src/acp/agent.rs:94 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:84
  - ✅ `session/load` — rust crates/oc-cli/src/acp/agent.rs:78 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:80
  - ✅ `session/new` — rust crates/oc-cli/src/acp/agent.rs:62 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:76
  - ✅ `session/prompt` — rust crates/oc-cli/src/acp/agent.rs:182 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:112
  - ✅ `session/resume` — rust crates/oc-cli/src/acp/agent.rs:116 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:92
  - ✅ `session/set_config_option` — rust crates/oc-cli/src/acp/agent.rs:160 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:104
  - ✅ `session/set_mode` — rust crates/oc-cli/src/acp/agent.rs:171 · ref /opt/opencode-ref/packages/cli/src/acp/agent.ts:108

## 归一化规则

- rust 事件种类：`SessionUpdate::AgentMessageChunk` → `agent_message_chunk`（CamelCase→snake_case），全 acp/ 目录非测试区构造点。
- rust 方法面：agent.rs 的 `request: XxxRequest` 臂经声明映射表转 wire 方法名（13 请求 + 1 通知，与 ref service.ts:1 注记的「13 request methods plus the cancel notification」一致）。
- ref 侧均为字面量直取（agent.ts / 各 acp 模块的 `sessionUpdate:` 字面量）。

## Census 裁定

- 轴 1 已裁定：11/11（覆盖率 11/11）；轴 2 已裁定：13/13（覆盖率 13/13）。
- 结论：事件种类 REF_ONLY 0 / 方法面 REF_ONLY 0 为缺口候选；RUST_ONLY（0/0）需登记册背书。

## 验证记录

- 提取复核：轴 1 全部 11 种、轴 2 全部 13 个方法的双侧构造点/注册点均亲读当前代码核实（rust crates/oc-cli/src/acp/*.rs 非测试区 + ref packages/cli/src/acp/*.ts）；cmp-S3b-acp.md 仅作定位线索，未采信其结论。
- 幂等验证：`python3 tools/census/acp_surface.py --write /tmp/opencode/acp-a.md` 连跑两次，`diff` 逐字节一致（/tmp/opencode 下执行）。
- 脚本 exit 0（census 非门禁）。
