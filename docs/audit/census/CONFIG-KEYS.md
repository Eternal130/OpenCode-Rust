# CONFIG-KEYS Census（缺口普查 · 通道 2/∞）

> 双侧配置键全集机械提取 + 字面默认值对拍；census 非门禁，exit 0。

> 生成命令：`python3 tools/census/config_keys.py --write docs/audit/census/CONFIG-KEYS.md`
> 权威源：rust serde 结构体（oc-schema/config + oc-core/cli_config）vs ref effect Schema（packages/schema/src/config* + packages/tui/src/config/index.tsx）。

## 归一化与映射规则

- 键名映射规则：**IDENTITY + 显式容器规则** —— 两侧文档键名均为逐字面量（顶层 snake_case：`default_agent`/`tool_output`；嵌套 MCP/provider 形状 camelCase：`headerTimeout`，rust 经 `#[serde(rename)]`/`rename_all="camelCase"` 显式对齐）。大小写不做静默归一；case-only 不匹配将以 CASE_MISMATCH 呈现（本轮未出现）。
- 开放形状归一：`#[serde(flatten)]`/字符串键 map/`Schema.Record(Schema.String,…)`/`Schema.StructWithRest` → `<*>` 节点（serde flatten 语义为父级内联）；`Vec<T>`/`Schema.Array` → `<*>` 数组节点；集合比对时统一剥除 `.<*>`/`.<…>` 标记。
- union/tagged-union：双侧变体合并展开（rust enum 各 variant 的结构、`#[serde(tag="type")]` 判别键；ref `Schema.Union` 的各 struct/class 分支）；纯字面量 union 记为叶子。
- ref 侧 `//`、`/* */` 注释先行剥除（保偏移）；跨模块引用经 import 表解析（`../model.js` 等）。
- 默认值文本规范化：保留引号（`"lit"` ≠ `lit`）；`true/false`/数字/带引号字符串直接字面比对。

## 范围声明

- `crates/oc-core/src/config.rs` 为加载器（发现/合并/变量替换/watch），无键定义；其顶层结构体仅 `GlobalDirs, Options, WellknownDocument, NoWellknown, SourceDiagnostic, Loaded` 等服务类型 —— 键全集由 oc-schema（opencode.json）与 oc-core/cli_config（cli.json）承载。
- `crates/oc-tui/src/config.rs` 消费 `cli_config::Info`（另有手写 `parse` 直读 `value.get/pointer`，键集一致）；只取默认值与锚点证据。
- ref `packages/core/src/config/plugin/*.ts` 是行为消费者（不定义键），以锚点检查覆盖（mcp.ts/snapshot.ts）。
- `permissions`（Ruleset）两侧为递归规则结构，以 `<…>`/叶子表示，内部不展开（属 permission 通道）。
- `keybinds` 为开放键表（rust `Value` vs ref `TuiKeybind.KeybindOverrides`），记为 `<*>` 开放节点。
- 双方都读但语义分叉的键不做行为判定（超范围）。

## 轴 1a：opencode.json 键路径全集

- rust（crates/oc-schema/src/config/mod.rs `Info` 根展开）：149 个点路径
- ref（packages/schema/src/config.ts `Info` 根展开）：131 个点路径
- **BOTH：131** · REF_ONLY：**0** · RUST_ONLY：**2**
  - 🔗 RUST_ONLY `experimental.file_retention.cleanup_interval` — crates/oc-schema/src/config/experimental.rs:11
  - 🔗 RUST_ONLY `providers.models.compatibility.requireSignature` — crates/oc-schema/src/model.rs:69
    - 裁定【RUST_ONLY·加固旋钮】：rust `crates/oc-schema/src/config/experimental.rs` 的 `FileRetention`（默认 3_600_000ms 扫描）为 rust 新增的文件保留调优面；ref `packages/schema/src/config/experimental.ts` 仅有 portable_shell_scanner/subagent_depth/policies 三键。需登记册 rust-extra 家族背书。
    - 裁定【RUST_ONLY·有据分歧】：rust `crates/oc-schema/src/model.rs` Compatibility 第 7 键（rename_all camelCase → requireSignature）；ref schema `packages/schema/src/model.ts` Compatibility 仅 6 键不收此名，但 ref 内部 provider 数据确实写入（`packages/ai/src/providers/alibaba.ts:104` `compatibility:{requireSignature:false}`）——rust schema 比 ref 严格 schema 宽一档。

## 轴 1b：cli.json（TUI）键路径全集

- rust（crates/oc-core/src/cli_config/mod.rs `Info` 根展开）：59 个点路径
- ref（packages/tui/src/config/index.tsx `Info` 根展开）：57 个点路径
- **BOTH：57** · REF_ONLY：**0** · RUST_ONLY：**2**
  - 🔗 RUST_ONLY `$schema` — crates/oc-core/src/cli_config/mod.rs:292
  - 🔗 RUST_ONLY `attention.bell` — crates/oc-core/src/cli_config/mod.rs:106
    - 裁定【RUST_ONLY·宽容旋钮】：rust `crates/oc-core/src/cli_config/mod.rs` 的 cli.json `Info.$schema`；ref TUI `packages/tui/src/config/index.tsx` 的 Info 未声明（opencode.json 文档侧两侧都有）。
    - 裁定【RUST_ONLY·文档化取舍】：rust `crates/oc-core/src/cli_config/mod.rs` + `crates/oc-tui/src/config.rs:61`（默认 false，注释标 P11：v2 从不 emit BEL）；ref attention 节无 bell 键。

## 轴 2：字面默认值对拍（双侧均有字面默认值的键）

- ref 源：`packages/tui/src/config/index.tsx` `resolve()` 的 `?? <lit>` 链（机械提取 16 项）。
- rust 源：`crates/oc-tui/src/config.rs` `impl Default for TuiConfig` 字面（机械提取 19 项；可比子集经人工核对映射表 3 项）。
- RUST_TUI_FIELD_PATH（TuiConfig 字段→cli.json 点路径，逐项亲读双侧核对）：
  - `mouse` → `mouse`
  - `session_image_preview` → `session.image_preview`
  - `session_tps` → `session.tps`

- 可比键：2
  - ✅ `mouse` rust=`true` ref=`true`
  - ✅ `session.tps` rust=`true` ref=`true`
- 仅 ref 有机械可提字面默认（rust 默认在解析分支/子结构中，本轮不比对）：14
  - ℹ️ `attention.notifications` ref=`false`
  - ℹ️ `attention.sound` ref=`false`
  - ℹ️ `attention.sound_pack` ref=`"opencode.default"`
  - ℹ️ `attention.sounds` ref=`{}`
  - ℹ️ `attention.volume` ref=`0.4`
  - ℹ️ `cursor.blinking` ref=`true`
  - ℹ️ `cursor.style` ref=`"block"`
  - ℹ️ `leader.timeout` ref=`2000`
  - ℹ️ `session.new_location` ref=`"launch"`
  - ℹ️ `session.permissions` ref=`"prompt"`
  - ℹ️ `tabs.indicators` ref=`"status"`
  - ℹ️ `tabs.layout` ref=`"horizontal"`
  - ℹ️ `tabs.mode` ref=`(input.tabs?.enabled===undefined?"auto":input.tabs.enabled?"on":"off")`
  - ℹ️ `tabs.scope` ref=`"cwd"`
- 仅 rust 有机械可提字面默认（ref 以 falsy 检查/子结构承载，本轮不比对）：1
  - ℹ️ `session.image_preview` rust=`false`

## 锚点核对（提取器正确性验证；三锚来自上轮修复）

- **mcp.timeout 文档级合并（曾 G-362b）**：rust [crates/oc-core/src/engine/mod.rs:3443] vs ref [packages/core/src/config/plugin/mcp.ts:44] → ✅ 对齐
- **snapshots:false 开关（曾 G-362a）**：rust [crates/oc-core/src/engine/mod.rs:1920] vs ref [packages/core/src/config/plugin/snapshot.ts:16] → ✅ 对齐
- **snapshots 默认值 true**：rust [crates/oc-core/src/snapshot.rs:369] vs ref [packages/core/src/snapshot.ts:82] → ✅ 对齐
- **session.tps 默认值 true（曾 G-367）**：rust [crates/oc-tui/src/config.rs:237] vs ref [packages/tui/src/config/index.tsx:322] → ✅ 对齐

## Census 裁定与覆盖率

- 轴 1a：差异面已裁定 133 项（BOTH 131 + REF_ONLY 0 + RUST_ONLY 2；全集 143 中其余 10 个为仅作前缀的容器节点，已在归一化规则下剔除）
- 轴 1b：差异面已裁定 59 项（BOTH 57 + REF_ONLY 0 + RUST_ONLY 2；全集 59 中其余 0 个为容器节点）
- 轴 2：可比键 2，默认值不一致 0 项。
- 结论：REF_ONLY 为缺口候选（逐项亲读定性）；RUST_ONLY 为 rust 额外旋钮（需登记册 rust-extra 背书）；`<*>` 开放节点内部不判定。

## 提取不确定性

- 轴1a-ref: UNION-EXPANDED model -> Explicit (union branch)
- 轴1a-ref: UNION-EXPANDED agents.<*>.model -> Explicit (union branch)
- 轴1a-ref: UNION-EXPANDED formatter -> Entry (union branch)
- 轴1a-ref: UNION-EXPANDED lsp -> Disabled (union branch)
- 轴1a-ref: UNION-EXPANDED lsp -> Server (union branch)
- 轴1a-ref: UNION-EXPANDED lsp -> Entry (union branch)
- 轴1a-ref: UNION-EXPANDED mcp.servers.<*> -> LocalConfig (union branch)
- 轴1a-ref: UNION-EXPANDED mcp.servers.<*>.oauth -> OAuthConfig (union branch)
- 轴1a-ref: UNION-EXPANDED mcp.servers.<*> -> RemoteConfig (union branch)
- 轴1a-ref: UNION-EXPANDED commands.<*>.model -> Explicit (union branch)
- 轴1a-ref: UNION-EXPANDED websearch -> Info (union branch)
- 轴1a-ref: UNION-EXPANDED warming -> Info (union branch)
- 轴1a-ref: UNION-EXPANDED providers.<*>.settings.compaction (struct branch of union)
- 轴1a-ref: UNION-EXPANDED providers.<*>.settings.compaction (struct branch of union)
- 轴1a-ref: UNION-EXPANDED providers.<*>.models.<*>.settings.compaction (struct branch of union)
- 轴1a-ref: …另有 5 条同类
- 轴1b-ref: UNION-EXPANDED plugins.<*> (struct branch of union)
- 轴1b-ref: UNION-EXPANDED plugins.<*> -> Plugin (union branch)
- 键名提取只认 `#[serde(rename)]`/容器 `rename_all` 与 TS 字面键；rust 漏写 rename 的字段名≠键名情形提取器无法发现（人工抽查未见）。
- ref `mapFields` 动态改写按原 struct 字段展开（tools/input/output）；`Schema.withConstructorDefault` 等构造默认不影响键集。
- ref 相对 import（`../model.js`）经 import 表解析；包级 import（`@opencode/...`）不解析——本轴所需形状均在 schema 包内。
- rust 侧跨文件同名歧义经 `use crate::…` 段 + config/ 子树优先消歧（如 `Compaction` 的 provider/config 双定义）。
- union 展开为各分支键并集（variant merge），不表达互斥关系；`<*>` 开放节点内部键差不判定。

## 验证记录

- 每个差异项已亲读双侧当前代码核实（file:line 见上；旧文档仅作线索）。
- 幂等性：连跑两次输出经 `diff` 逐字节一致（/tmp/opencode/ 验证）。
- 确定性：文件排序遍历、集合输出前排序；无时间戳/随机源。
- exit 0（census 非门禁）。
