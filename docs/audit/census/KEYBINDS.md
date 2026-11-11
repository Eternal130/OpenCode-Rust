# KEYBINDS Census（缺口普查 · 键位动作全集）

> 双侧可绑定动作名全集机械对拍；生成命令：`python3 tools/census/keybinds.py --write docs/audit/census/KEYBINDS.md`

## 轴：可绑定动作名全集

- ref（packages/tui/src/config/keybind.ts `Definitions` 表键，schema 校验面）：**243** 个动作名
- rust（crates/oc-tui-core/src/keymap/table.rs `static DEFAULT_TABLE` 的 `defs!` 臂命令字面量）：**251** 个动作名
- **BOTH：243** · REF_ONLY：**0** · RUST_ONLY：**8**

### REF_ONLY：0（ref 每一个可绑定动作名都在 rust DEFAULT_TABLE 中）

### RUST_ONLY（rust 额外可绑定 → 需登记册 rust-extra 背书）
  - 🟡 `composer.close` — crates/oc-tui-core/src/keymap/table.rs:915（ref `Definitions` 无此键）
  - 🟡 `composer.subagent.toggle-activity` — crates/oc-tui-core/src/keymap/table.rs:902（ref `Definitions` 无此键）
  - 🟡 `composer.tab.next` — crates/oc-tui-core/src/keymap/table.rs:917（ref `Definitions` 无此键）
  - 🟡 `composer.tab.prev` — crates/oc-tui-core/src/keymap/table.rs:916（ref `Definitions` 无此键）
  - 🟡 `dialog.select.section.next` — crates/oc-tui-core/src/keymap/table.rs:829（ref `Definitions` 无此键）
  - 🟡 `dialog.select.section.prev` — crates/oc-tui-core/src/keymap/table.rs:828（ref `Definitions` 无此键）
  - 🟡 `stats.close` — crates/oc-tui-core/src/keymap/table.rs:642（ref `Definitions` 无此键）
  - 🟡 `stats.open` — crates/oc-tui-core/src/keymap/table.rs:641（ref `Definitions` 无此键）

## 归一化与范围声明

- 只比**动作名全集**（dot-command id）。绑定语法、默认键位、描述文案、layer/palette 元数据不在本通道范围。
- ref 动作名 = `Definitions` 对象键（用户 keybind override schema 的键域，未知键被 `parse` 拒绝 → 表即全集）。
- rust 动作名 = `defs!` 每臂第 2 个字符串字面量（`command` 字段，与 v2 配置 schema 的 override 键一致）；第 1 个字面量是 rust 侧 bind_name，不参与比较。
- 两侧命名风格已一致（`a.b.c` 点分），无 kebab/snake 归一化需要。

## Census 裁定

- 已裁定动作名：251/251（覆盖率 100%：双侧全集逐项归属）
- 结论：BOTH 243 项动作名全对齐；REF_ONLY 0 为缺口候选；RUST_ONLY 8 为 rust 额外面（见下方验证记录的逐项人工核实）。

## 验证记录

- 提取面亲读核实：packages/tui/src/config/keybind.ts（Definitions 表 1..300 行区间）与 crates/oc-tui-core/src/keymap/table.rs（DEFAULT_TABLE 区间）逐项对照提取正则命中。
- RUST_ONLY 8 项逐项人工核实（2026-11-11）：ref 侧确无 `Definitions` 键，但均以**运行时命令**形态存在于 ref：
  - `stats.open`/`stats.close` — feature-plugins/system/stats.tsx:197（ref 为 feature-plugin 命令，未开成可绑定键位；rust 开成 `none` 默认的 palette 行）
  - `composer.close` — routes/session/composer/subagents-tab.tsx:147 等处以 `composer.close()` 运行时调用存在，无键位定义
  - `composer.tab.next`/`composer.tab.prev`、`composer.subagent.toggle-activity` — composer 运行时命令，ref 无键位定义
  - `dialog.select.section.prev`/`dialog.select.section.next` — rust 对话框分区导航新增（table.rs:828-829），ref 无对应命令
- 已知锚「230/230 全对齐」为上一轮快照；本轮 ref 表已增至 243 项，rust 同步增长并多 8 个 rust-extra 命令——偏离锚先查提取：本轮提取正则已覆盖含 `-` 的动作名（which-key.* 19 项），提取不确定性见下。
- 提取不确定性：ref 侧仅认 `keybind(` 调用行（表内全部 243 项均为该形态，`leader` 为唯一非引号键已覆盖）；rust 侧仅认 `"bind" => "cmd", "..."` 四字面量臂形态（DEFAULT_TABLE 内全部臂均为此形态，注释行不匹配）。
- 幂等性：脚本连跑两次输出逐字节一致（/tmp/opencode/ diff 验证，见任务记录）。

