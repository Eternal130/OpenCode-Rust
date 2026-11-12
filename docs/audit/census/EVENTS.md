# EVENTS Census（最小验证 · 通道 2/∞）

> 双侧权威 schema 对拍（事件名全集）+ rust TUI 消费面 + 全 schema 跨检三轴。

> 生成命令：`python3 tools/census/events.py --write docs/audit/census/EVENTS.md`
> 范围声明：仅比对 wire 事件**名**（`type` tag）；payload 字段级差异不在本通道范围（见 G-270⑩/G-380② 等已登记条目）。

## 轴 1：权威事件名对拍（ref session-event.ts ↔ rust session_event.rs）

- ref（/opt/opencode-ref/packages/schema/src/session-event.ts）：51 个 `type:` tag
- rust（crates/oc-schema/src/session_event.rs，封闭 `Event` 枚举 serde rename）：51 个 tag
- **BOTH：51** · REF_ONLY：**0** · RUST_ONLY：**0**
  - 双侧集合逐项一致（全集差异为 0）

- 封闭集守卫：`event_type()` match 臂 51 条 vs 枚举 tag 51 个 — 一致

## 轴 2：rust TUI 消费面（oc-tui/src/store.rs `"session.*"` 字面量，测试区已剥除）

- 提取 `"session.*"` 字面量 38 个：命中 wire 事件 35，TUI 本地事件 3
  - ⚪ TUI_LOCAL `session.grouping` — crates/oc-tui/src/store.rs:3071（TUI 本地状态名，非 wire 面，不计入对拍）
  - ⚪ TUI_LOCAL `session.sidebar` — crates/oc-tui/src/store.rs:3066（TUI 本地状态名，非 wire 面，不计入对拍）
  - ⚪ TUI_LOCAL `session.updated` — crates/oc-tui/src/store.rs:13098（TUI 本地状态名，非 wire 面，不计入对拍）
- 交叉验证：wire 命中集合 ⊆ 轴 3 BOTH 集合，无消费面独立发现的新事件名 → 提取未见漏

## 轴 3：全 schema 跨检（所有 `session.*` wire 名，含 status/compaction 族）

- ref packages/schema/src 全部 `type: "session.*"`：54 个；rust oc-schema/src 全部 serde rename：54 个
- **BOTH：54** · REF_ONLY：**0** · RUST_ONLY：**0**
  - 双侧全集逐项一致（含 `session.status`/`session.idle`/`session.compacted` 三条axis-1 之外的会话总线事件，双侧均在：ref session-status-event.ts / session-compaction-event.ts ↔ rust oc-schema/src/events.rs）
- 提取规则：ref `packages/schema/src/**` 排除 `src/v1/**` 遗留迁移 schema —— 其中 5 个 `session.*` tag（`session.created`, `session.deleted`, `session.diff`, `session.error`, `session.updated`）为 v1→v2 数据库迁移的旧数据形状（仅 core/src/database/v1-migration.bun.ts 消费，schema/test/v1-isolation.test.ts 钉定隔离），**不是活的总线/wire 事件**，不计入对拍、也不构成缺口（注意：`session.diff` 作为 RPC operation id 存在于双侧 HTTP 面，属 HTTP-SURFACE 通道，勿混淆）。

## Census 裁定

- 轴 1 已裁定事件名：51/51（覆盖率 100%：双侧权威 union 全集逐项归属）
- 轴 3 已裁定事件名：54/54（覆盖率 100%）
- 结论：REF_ONLY 0、RUST_ONLY 0 —— 会话事件名全集双侧一致，无缺口候选。（rust 文档注释自称 51 事件，与提取一致。）

## 验证记录

1. **交叉验证方法**：
   - 轴 1 双侧权威源自读核实：ref `session-event.ts` 以 `type: "session.*"` tag 喂入`Schema.Union(...).toTaggedUnion("type")`（`All` union = Durable ∪ Ephemeral = 51）；rust `session_event.rs` `pub enum Event` 为封闭集，`#[serde(rename)]` 即 wire 名（51），并以 `event_type()` match 臂做封闭集自洽守卫（51=51）。
   - 轴 2 消费面核实：`oc-tui/src/store.rs` 的 `"session.*"` 字面量按**全 rust wire 集**（session_event.rs ∪ events.rs）归类；`session.child.first`/`session.grouping`/`session.list`/`session.sidebar*`/`session.updated` 为 TUI 本地状态名（无任何 wire schema tag 对应），不误报为缺口；wire 命中（含 `session.idle`）全部落在轴 3 BOTH 内。
   - 轴 3 全集核实：`session.status`/`session.idle`（ref session-status-event.ts ↔ rust events.rs）与 `session.compacted`（ref session-compaction-event.ts ↔ rust events.rs）双侧均在，file:line 由脚本提取。
2. **已知锚核对**：上一轮全量审查结论为“52 会话事件分类一致”。本 census 权威源双侧均为 **51**（偏差 −1）。排查：ref `session-event.ts` 的 49 公开 durable 定义 + `UsageRecorded` + `MessageContentUpdated`（DurableDefinitions 追加）+ 4 ephemeral 已含 = 51；rust 注释与枚举同为 51。锚的 52 疑将 `session.status`（status-event.ts，轴 3 覆盖）计入或为分桶口径差。因双侧集合逐项一致（0 差异），该偏差不构成缺口；仅作锚记录。
3. **提取规则（归一化/测试剥除）**：rust 侧先 `strip_tests`（剥除 `#[cfg(test)]`/`#[test]` 区，空白替换保行号）再匹配 `#[serde(rename = "session.*")]`；ref 侧仅扫非 test 的 `*.ts`，匹配 `type: "session.*"`（小写点分名），排除 `src/v1/**` 遗留迁移schema；同一名多次出现取首现 file:line。无大小写/别名归一（双侧字面量本已同形）。
4. **幂等性**：脚本无时间戳、无随机序，全部 sorted；连跑两次输出逐字节一致（diff 于 /tmp/opencode/ 下验证）。
