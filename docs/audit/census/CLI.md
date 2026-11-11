# CLI Census（命令×旗标面 · 通道 1）

> rust oc-cli clap 定义（cli.rs）vs ref packages/cli effect-cli Spec 定义（commands.ts）。

> 生成命令：`python3 tools/census/cli_surface.py --write docs/audit/census/CLI.md`

## 归一化与范围声明

- N1 旗标拼写：rust 字段名 snake_case → kebab-case（尾部单个 `_` 去除，`continue_`→`continue`）；显式 `long = "x"` 优先。ref 名取自 `Flag.*("name")` 原文（ref 本已是 kebab-case），不做变换。
- N2 全局层：ref 除 `print-logs`（GlobalFlag，runtime.ts 注册）外无真全局，而是把 `ServerParams`（standalone、server）与 `PermissionParams`（auto、yolo、dangerously-skip-permissions）以 `...spread` 摊入 root 及多个子命令；rust 将六个全部声明为 clap 根级 `global = true`。**spread 组与 GlobalFlag 只进全局轴（轴 2），并从各命令本地旗标表中剔除**。
- N3 位置参数（ref `Argument.*`、rust 无 `#[arg(long)]` 的裸字段）只盘点不裁定（轴 4 信息轴）。
- N4 框架内建 `--help`/`--version` 双侧皆有，范围外。隐藏旗标（`hide`/`withHidden`）作为标注不作为差异。
- rust 侧来源：crates/oc-cli/src/cli.rs（`#[cfg(test)]` 区已剥除）；ref 侧来源：/opt/opencode-ref/packages/cli/src/commands/commands.ts。

## 轴 1：命令树 diff

- rust：47 个命令 · ref：46 个命令
- **BOTH：46** · REF_ONLY：**0** · RUST_ONLY：**1**
  - 🔗 RUST_ONLY `completion` — crates/oc-cli/src/cli.rs:442（rust 额外面，需登记册决策背书）

  别名核对（BOTH 命令）：

  - 其余命令别名双侧一致（rust `upgrade` alias `update` ↔ ref `aliases: ["update"]`）。

## 轴 2：全局旗标 diff

- rust（clap `global = true`，根级）：6 个
  - rust `auto` — crates/oc-cli/src/cli.rs:71
  - rust `dangerously-skip-permissions`（hidden） — crates/oc-cli/src/cli.rs:77
  - rust `print-logs` — crates/oc-cli/src/cli.rs:34
  - rust `server` — crates/oc-cli/src/cli.rs:42
  - rust `standalone` — crates/oc-cli/src/cli.rs:38
  - rust `yolo`（hidden） — crates/oc-cli/src/cli.rs:74
- ref（GlobalFlag + ServerParams/PermissionParams spread 组，N2）：6 个
  - ref `--auto` — /opt/opencode-ref/packages/cli/src/commands/commands.ts（PermissionParams spread）:27
  - ref `--dangerously-skip-permissions` — /opt/opencode-ref/packages/cli/src/commands/commands.ts（PermissionParams spread）:32
  - ref `--print-logs` — /opt/opencode-ref/packages/cli/src/commands/commands.ts:6
  - ref `--server` — /opt/opencode-ref/packages/cli/src/commands/commands.ts（ServerParams spread）:20
  - ref `--standalone` — /opt/opencode-ref/packages/cli/src/commands/commands.ts（ServerParams spread）:16
  - ref `--yolo` — /opt/opencode-ref/packages/cli/src/commands/commands.ts（PermissionParams spread）:31
- **BOTH：6** · REF_ONLY：**0** · RUST_ONLY：**0**
- ref GlobalFlag 注册点：/opt/opencode-ref/packages/cli/src/framework/runtime.ts:85（`withGlobalFlags([PrintLogs])`）。

## 轴 3：命令×本地旗标 diff（spread/全局已剔除）

- rust 命令×本地旗标对：62 · ref：57
- **BOTH：57** · REF_ONLY：**0** · RUST_ONLY：**5**
  - 🔗 RUST_ONLY `(root) --version-rust`（hidden） — crates/oc-cli/src/cli.rs:30
  - 🔗 RUST_ONLY `mcp add --force` — crates/oc-cli/src/cli.rs:608
  - 🔗 RUST_ONLY `mini --no-replay`（hidden） — crates/oc-cli/src/cli.rs:373
  - 🔗 RUST_ONLY `plugin add --force` — crates/oc-cli/src/cli.rs:636
  - 🔗 RUST_ONLY `plugin remove --force` — crates/oc-cli/src/cli.rs:654

## 轴 4（信息轴）：位置参数盘点（不裁定）

  说明：位置参数只按名盘点；`api` / `service set` / `session delete` 的「不一致」均为**展示名差异**（rust `value_name`/字段名 vs ref 参数名：`request`↔`operation | method path`、`nested_value`↔`env-value`、`session_id`↔`sessionID`），语义一一对应，不构成缺口；`completion shell` 为 rust-extra 命令的位置参数。

  - `(root)`：rust ['directory'] vs ref ['directory']
  - `api`：rust ['request'] vs ref ['operation | method path']（⚠️ 不一致，信息轴提示）
  - `auth export`：rust ['target'] vs ref ['target']
  - `auth import`：rust ['file'] vs ref ['file']
  - `auth login`：rust ['target'] vs ref ['target']
  - `auth logout`：rust ['target', 'credential'] vs ref ['target', 'credential']
  - `auth switch`：rust ['target', 'credential'] vs ref ['target', 'credential']
  - `completion`：rust ['shell'] vs ref ∅（⚠️ 不一致，信息轴提示）
  - `debug paths`：rust ['name'] vs ref ['name']
  - `mcp add`：rust ['name', 'command'] vs ref ['name', 'command']
  - `mcp auth`：rust ['name'] vs ref ['name']
  - `mcp logout`：rust ['name'] vs ref ['name']
  - `plugin add`：rust ['package'] vs ref ['package']
  - `plugin check`：rust ['target'] vs ref ['target']
  - `plugin remove`：rust ['package'] vs ref ['package']
  - `plugin update`：rust ['target'] vs ref ['target']
  - `run`：rust ['message'] vs ref ['message']
  - `service get`：rust ['key', 'name'] vs ref ['key', 'name']
  - `service set`：rust ['key', 'value', 'nested_value'] vs ref ['key', 'value', 'env-value']（⚠️ 不一致，信息轴提示）
  - `service unset`：rust ['key', 'name'] vs ref ['key', 'name']
  - `session delete`：rust ['session_id'] vs ref ['sessionID']（⚠️ 不一致，信息轴提示）
  - `session export`：rust ['session'] vs ref ['session']
  - `session import`：rust ['file'] vs ref ['file']
  - `upgrade`：rust ['target'] vs ref ['target']

## Census 裁定

- 命令：47/47（覆盖率 100%）
- 全局旗标：6/6（覆盖率 100%）
- 命令×本地旗标：62/62（覆盖率 100%：双侧全集逐项归属）
- 结论：REF_ONLY 0 项（缺口候选）；RUST_ONLY 6 项（rust 额外面，需登记册决策背书）。

## 验证记录

- 双侧每差异项均亲读当前代码核实（rust crates/oc-cli/src/cli.rs 的 clap 定义；ref packages/cli/src/commands/commands.ts 的 Spec/Flag 定义与 framework/runtime.ts 的 GlobalFlag 注册）。
- 幂等性：脚本纯读取、确定性排序；连跑两次输出逐字节一致（/tmp/opencode/ 下 diff 验证）。
- 提取不确定性：ref 命令树按 `Spec.make` 花括号区域嵌套推断（文件内无模板字面量花括号干扰）；rust 旗标名按 N1 规则推导，显式 `long=` 优先。

