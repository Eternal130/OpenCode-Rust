# TOOLS Census（内建工具注册表 · 通道 2）

> rust 内建静态注册（BuiltinTool + execute 合成）vs ref tool/plugin 注册表；轴 2 input schema 存在性；轴 3 output schema 声明（G-230 锚）。

> 生成命令：`python3 tools/census/tools_registry.py --write docs/audit/census/TOOLS.md`

## 归一化与范围声明

- 比较键 = 有效名（effectiveName）：`namespace_name`（无命名空间则裸名）；rust `Some("opencode")` ↔ ref `options.namespace`。
- rust 侧提取：`crates/oc-core/src/tool/*.rs` 生产区 `BuiltinTool { … }` 构造（`#[cfg(test)]`/`#[test]` 区已剥除，tests*.rs 跳过；`name: NAME` 常量已展开）+ `codemode.rs::execute_registration` 合成的 `execute`。
- ref 侧提取：`packages/core/src/tool/plugin/*.ts` 每个 `.add({ … })` draft（`export const name` 简写已归并；`draft.namespace({name:"opencode"})` 命名空间声明行不算工具）+ `packages/core/src/codemode/tool.ts` 的 `execute`。
- 轴 2 只比 input schema **存在性**（有/无），不比内容。
- 范围外：MCP 动态注册（rust `mcp/registry.rs`（动态 outputSchema 经 `pipeline_executor_with_output` 传入）、ref `tool/mcp.ts`）、运行时插件动态工具。`file-diff.ts` 无工具登记（辅助文件）。

## 轴 1：工具名全集 diff（有效名）

- rust（crates/oc-core/src/tool）：18 个内建工具
- ref（/opt/opencode-ref/packages/core/src/tool/plugin + codemode/tool.ts）：18 个内建工具
- **BOTH：18** · REF_ONLY：**0** · RUST_ONLY：**0**

  全集逐项（BOTH 项双侧 file:line）：

  | 有效名 | rust | ref |
  |---|---|---|
  | `edit` | crates/oc-core/src/tool/edit.rs:212 | /opt/opencode-ref/packages/core/src/tool/plugin/edit.ts:121 |
  | `execute` | crates/oc-core/src/tool/codemode.rs:95 | /opt/opencode-ref/packages/core/src/codemode/tool.ts:76 |
  | `glob` | crates/oc-core/src/tool/glob.rs:50 | /opt/opencode-ref/packages/core/src/tool/plugin/glob.ts:56 |
  | `grep` | crates/oc-core/src/tool/grep.rs:366 | /opt/opencode-ref/packages/core/src/tool/plugin/grep.ts:75 |
  | `opencode_list_mcp_resources` | crates/oc-core/src/tool/mcp_resource.rs:378 | /opt/opencode-ref/packages/core/src/tool/plugin/mcp-resource.ts:17 |
  | `opencode_models` | crates/oc-core/src/tool/opencode_tools.rs:662 | /opt/opencode-ref/packages/core/src/tool/plugin/opencode.ts:134 |
  | `opencode_read_mcp_resource` | crates/oc-core/src/tool/mcp_resource.rs:396 | /opt/opencode-ref/packages/core/src/tool/plugin/mcp-resource.ts:49 |
  | `opencode_session_move` | crates/oc-core/src/tool/opencode_tools.rs:565 | /opt/opencode-ref/packages/core/src/tool/plugin/opencode.ts:109 |
  | `opencode_session_rename` | crates/oc-core/src/tool/opencode_tools.rs:530 | /opt/opencode-ref/packages/core/src/tool/plugin/opencode.ts:89 |
  | `patch` | crates/oc-core/src/tool/patch.rs:67 | /opt/opencode-ref/packages/core/src/tool/plugin/patch.ts:79 |
  | `question` | crates/oc-core/src/tool/question.rs:228 | /opt/opencode-ref/packages/core/src/tool/plugin/question.ts:56 |
  | `read` | crates/oc-core/src/tool/read.rs:104 | /opt/opencode-ref/packages/core/src/tool/plugin/read.ts:40 |
  | `shell` | crates/oc-core/src/tool/shell.rs:658 | /opt/opencode-ref/packages/core/src/tool/plugin/shell.ts:190 |
  | `skill` | crates/oc-core/src/tool/skill.rs:204 | /opt/opencode-ref/packages/core/src/tool/plugin/skill.ts:40 |
  | `subagent` | crates/oc-core/src/tool/subagent.rs:718 | /opt/opencode-ref/packages/core/src/tool/plugin/subagent.ts:102 |
  | `webfetch` | crates/oc-core/src/tool/webfetch.rs:304 | /opt/opencode-ref/packages/core/src/tool/plugin/webfetch.ts:111 |
  | `websearch` | crates/oc-core/src/tool/websearch.rs:908 | /opt/opencode-ref/packages/core/src/tool/plugin/websearch.ts:41 |
  | `write` | crates/oc-core/src/tool/write.rs:46 | /opt/opencode-ref/packages/core/src/tool/plugin/write.ts:57 |

## 轴 2：每工具 input schema 存在性

- rust 声明 input：18/18（`BuiltinTool.input`/`ToolRegistration::new` 第三参为必填字段，缺一即编译失败）
- ref 声明 input：18/18

## 轴 3：每工具 output schema 声明（G-230 锚核对）

- ref 声明 output：**18/18** 个工具（15 个文件：tool.ts, edit.ts, glob.ts, grep.ts, mcp-resource.ts, opencode.ts, patch.ts, question.ts, read.ts, shell.ts, skill.ts, subagent.ts, webfetch.ts, websearch.ts, write.ts）
- rust 内建声明 output：**18/18** 个工具（每模块 `output_schema()` json! 字面 / codemode `registration.output = Some(...)`；`registry.rs::register` 经 `pipeline_executor_with_output` 线入（crates/oc-core/src/tool/registry.rs:188），`Value::Null` 仍为未声明 scope）
- die 分支（未声明却返回 output → 失败，ref runtime.ts:46 原文）：**在位**（crates/oc-core/src/tool/registry.rs:422）
- 锚核对：登记册 G-230 记载「rust 0/14 声明 vs ref 13 文件全声明」（round-24 census 扩为 0/18）；当前 census 实测 rust 18/18 声明、die 分支在位。**G-230 前置与 die 分支已修复（round 25）。**

## rust 注册挂载点（交叉核对）

- 生产区 `register_builtin(`/`mcp_resource::register`/`subagent::register`/`register_code_mode_with_limits` 调用 20 处：
  - crates/oc-core/src/engine/mod.rs:1784
  - crates/oc-core/src/engine/mod.rs:2688
  - crates/oc-core/src/engine/mod.rs:2694
  - crates/oc-core/src/engine/mod.rs:2710
  - crates/oc-core/src/engine/mod.rs:2739
  - crates/oc-core/src/engine/mod.rs:2752
  - crates/oc-core/src/engine/mod.rs:2782
  - crates/oc-core/src/engine/mod.rs:2829
  - crates/oc-core/src/engine/mod.rs:2923
  - crates/oc-core/src/engine/mod.rs:2989
  - crates/oc-core/src/engine/mod.rs:3002
  - crates/oc-core/src/mcp/tests.rs:1614
  - crates/oc-core/src/tool/codemode.rs:2057
  - crates/oc-core/src/tool/codemode.rs:2073
  - crates/oc-core/src/tool/mcp_resource.rs:374
  - crates/oc-core/src/tool/mcp_resource.rs:392
  - crates/oc-core/src/tool/mod.rs:283
  - crates/oc-core/src/tool/mod.rs:307
  - crates/oc-core/src/tool/mod.rs:344
  - crates/oc-core/src/tool/subagent.rs:739

## Census 裁定

- 已裁定工具：18/18（覆盖率 100%：双侧全集逐项归属）
- 结论：工具名全集一致；input schema 双侧全声明；output schema 声明 rust 0/18 vs ref 18/18 —— **G-230 缺口仍在（缺口候选，非新 gap）**。

## 验证记录

- 双侧每差异项均亲读当前代码核实（rust BuiltinTool 构造/registry.rs::register/codemode.rs::execute_registration；ref plugin/*.ts 的 .add draft 与 codemode/tool.ts）。
- 幂等性：脚本纯读取、确定性排序；连跑两次输出逐字节一致（/tmp/opencode/ 下 diff 验证）。
- 旧文档（G-230「0/14 vs 13 文件」）仅作锚，现状优先：数量以本 census 实测为准。

