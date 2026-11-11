# ERRORS Census（缺口普查 · server API 错误族）

> 双侧 HttpApi 错误 tag 全集 + readiness `code` 信封对拍；生成命令：`python3 tools/census/errors.py --write docs/audit/census/ERRORS.md`

## 轴 1：HttpApi 错误 tag（`_tag` 类名）全集

- ref（protocol/schema 中带 `httpApiStatus` 的 `Schema.TaggedError` 类）：**33** 个 tag
  - packages/protocol/src/errors.ts：30；groups/vcs.ts：1；groups/worktree.ts：1；packages/schema/src/instruction-entry.ts：1
- rust（oc-server 生产源中 `tagged("…")` / `"_tag": "…"` 字面量全集，测试区已剥除）：**33** 个 tag
- **BOTH：33** · REF_ONLY：**0** · RUST_ONLY：**0**

## 轴 2：readiness `code` 信封（无 `_tag` 的 503 家族）

- **BOTH：3** · REF_ONLY：0 · RUST_ONLY：0
  - ✅ `service_failed` — ref packages/server/src/process.ts:220 · rust crates/oc-server/src/errors.rs:589
  - ✅ `service_starting` — ref packages/server/src/process.ts:227 · rust crates/oc-server/src/errors.rs:602
  - ✅ `service_stopping` — ref packages/server/src/process.ts:227 · rust crates/oc-server/src/errors.rs:611

## 轴 3：rpc type 字面量（归一化观察，非族差）

- ref `RpcInternalError` 的 `type` 是闭合字面量集 `Schema.Literals(["rpc.internal","rpc.invalid_output"])`（errors.ts:34）；
  rust `rpc_internal(type_, …)` 接受任意字符串（errors.rs:447-463，文档注明字面量集但不闭合）。
  这是**校验强度差**而非错误族缺失——不计入 REF_ONLY/RUST_ONLY，登记为后续收紧候选。

## 归一化与范围声明

- 只比**错误族名/`_tag` 全集与 readiness `code` 全集**。HTTP 状态映射细节、字段形状（`kind`/`field`/`data` 等）不在本通道范围（rust 侧 errors.rs 单元测试已钉定大量字段形状）。
- 命名风格：双侧同为 PascalCase `_tag` 类名，无 kebab/snake/前缀差异需归一化。
- 任务提示中的 `"project:vcs_init_failed"` 风格 code 字符串：双侧当前源中**均不存在**此类错误 code（grep 核实），该例不构成通道内容。
- 范围排除（ref 侧存在但非 HttpApi 族，未计入）：schema/tool.ts:61 `Tool.Error`、schema/worktree.ts:47 `Worktree.OperationError`（内部工具错误）；protocol/simulation.ts 的 `SimulationRequestError` 等 4 类（无 `httpApiStatus`，模拟面）。

## Census 裁定

- 已裁定错误族：36/36（覆盖率 100%：双侧全集逐项归属）
- 结论：tag 族 BOTH 33、readiness code BOTH 3；REF_ONLY 0 为缺口候选；RUST_ONLY 0 需登记册背书。

## 验证记录

- 双侧当前代码亲读核实（2026-11-11）：ref protocol/src/errors.ts（30 类逐类对照 rust helper）；groups/vcs.ts:15-19、groups/worktree.ts:9-24、schema/instruction-entry.ts:33-39 对 rust errors.rs:544-583 三个 spec-pinned 信封；process.ts:220/227 对 errors.rs:589-618。
- rust 侧全 crate 扫描确认 `tagged(`/`"_tag":` 字面量仅出现在 errors.rs（handlers/common.rs:178 的 `spread_tag` 为动态透传 helper，tag 由 errors.rs helper 提供，不引入新族）。
- 提取不确定性：ref 侧只认 `TaggedError<X>()(` 后首字符串形态（4 个 httpApiStatus 文件内全部如此）；rust 侧 `tagged(`/`"_tag":` 双模式覆盖 errors.rs 的 helper 与内联 json! 两种写法。若任一侧将来新增非字面量（拼接）tag 名，本提取会漏——当前源中无此形态。
- 幂等性：脚本连跑两次输出逐字节一致（/tmp/opencode/ diff 验证）。

