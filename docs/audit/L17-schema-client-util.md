# L17 Schema / Protocol / Client / Util / v1 兼容 — 审查报告

> 审查方式：逐文件清点两侧目录 + 重点全文精读 + 类型级/operationId 级全集 diff（穷举枚举，非抽查）。事件类型 104↔104、方法面 141↔142 全集比对。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
- 逐文件清点：Ref schema 101 文件（5693 行）、protocol 21 文件（4240 行）、client 28 文件（16766 行，含 generated）、util 24 文件（4159 行）、core/v1 8 文件（602 行）；Rust oc-schema 49 文件（8468 行）、oc-client 10 文件（20452 行）、oc-util 17 文件（3948 行）、oc-http 147 行、v2_migration.rs 1084 行。
- 重点全文精读：event.ts↔event.rs、event-manifest.ts↔events.rs/bus/registry.rs、shared-events.ts↔shared_events.rs(787行)、sse 解码↔sse.rs(321行)、service*.ts↔oc-server/lifecycle.rs、rpc-runtime/promise-rpc↔rpc_typed.rs、migrate.ts↔config/normalize.rs、v2_migration.rs、oc-http/lib.rs、retry/encode/hash/glob/pathx/bunman 等。
- 事件类型全集 diff：Ref `type: "..."` 字面量 104 个 vs Rust `#[serde(rename="...")]` 104 个，双向差集为空。
- 方法面全集 diff：Ref 141 operationId vs Rust 142；REF-only 为空，RUST-only 仅 `plugin.install`。

### 2. 逐项对比表

**A. Schema**
| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| 事件信封 durable/ephemeral | schema/src/event.ts:83-143 | oc-schema/src/event.rs:34-58 | ✅ | `aggregateID` camel rename 正确 |
| 事件类型全集（104 类） | event-manifest.ts:41-101 聚合 | oc-schema 各模块；聚合于 bus/registry.rs:284,415 与 oc-server/event_manifest.rs | ✅ | 类型字符串逐一相等 |
| 小事件模块（server/status/compaction/location/mcp/vcs/worktree/workspace/installation/lsp/tui/log.synced） | 对应 *-event.ts | oc-schema/src/events.rs:17-378 | ✅ | 逐 variant 核对（含 tui.toast duration 默认 5000） |
| session-event.ts (19K) | session-event.ts | session_event.rs + session_inbox.rs | ✅ | 类型 diff 全集通过 |
| TokenUsage | token-usage.ts:5-16 | common.rs:144-165 | ✅ | Finite→Num(f64)，total() 一致 |
| 标识符/ID | identifier.ts, session-id.ts 等 | identifier.rs, id.rs (宏生成 brand) | ✅ | 前缀校验保留 |
| config 全域（20 文件） | config/*.ts | oc-schema/src/config/ 20 文件 | ✅ | 一一对应 |
| misc 汇总 | vcs.ts, worktree.ts 等 | misc.rs:11-359 | ✅ | Rust 合并文件 |
| instruction/file-diff/session-metadata/revert | instruction*.ts 等 | session_support.rs:1-206 | ✅ | 含 RevertV1 legacy 形状 |
| rpc.ts（PortableDefinition） | rpc.ts:1-40 | 无 oc-schema 模块；运行时 oc-client/rpc_typed.rs:32-45 | 🟡 | 类型层未移植，运行时语义等价 |
| pty-ticket.ts ConnectToken | pty-ticket.ts | oc-client/methods.gen.rs | 🟡 | 移位到 client |
| durable-event-manifest.ts | durable-event-manifest.ts | oc-server/event_manifest.rs | 🟡 | 移位 |
| v1/ filesystem+permission v1 schema | v1/filesystem.ts, v1/permission.ts | 未找到独立对应 | 🔴 | 仅 DB 迁移引用 |
| v1/session.ts (20K) | v1/session.ts | database/v1_migration.rs:39-60 | 🟡 | 仅迁移消费面 |
| legacy-event.ts (`command.executed`) | v1/legacy-event.ts | grep 全 crates 无 | 🔴 | Ref 侧也仅 schema+test 引用，影响低 |
| oc-schema 自认 TODO | — | oc-schema/src/lib.rs:80-81 | ⚠️ | 注释承认 v1/、rpc.ts、pty-ticket 未在 schema 层移植（实际多已移位） |

**B. Protocol / Client**
| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| 方法表（141 op） | protocol/openapi.json + generated/client.ts (2286行) | oc-client/methods.gen.rs (6548行) | ✅ | REF-only 仅 `/api/fs/read/{P}`，Rust 用 `{*path}` 通配等价 |
| Rust 新增方法 | — | plugin.install、pty connect 2 个显式暴露 (G-17) | 🔵 | 3 个 rust-extra |
| 错误模型 | protocol/src/errors.ts (267行) | oc-client/src/error.rs:11-21 | ✅ | `_tag`→name、UnexpectedStatus 等对齐 |
| 共享事件多路复用 | client/src/shared-events.ts:22-148 | oc-client/src/shared_events.rs:130-442 | ✅ | 懒连接、4096 容量、server.connected 只重放 marker；G-125 对齐 |
| SSE 解码 | generated client `sse` | oc-client/src/sse.rs:16-212 | ✅ | 16MiB 上限按 UTF-16 码元（G-126）、CRLF 归一、多 data: join |
| 断线重连+空闲看门狗 | solid/connection.ts:29-120（2s/1s/45s、首帧强校验） | oc-tui/store.rs:12326 + event_ext.rs:20-28（Last-Event-ID） | 🟡 | 重连语义存在（快照式），2s/45s 看门狗常量与首帧强校验未见逐一对应 |
| solid/ data.ts 响应式绑定 | solid/data.ts | oc-tui/store.rs | 🟡 | 架构性替代（TUI 域） |
| service 发现/probe | client/service-probe.ts:30-77 | oc-server/lifecycle.rs:291-375 | ✅ | 404=协议不兼容、pid/version 校验一致 |
| 版本协商 matchesVersion | client/service-version.ts:3-8（谓词函数） | lifecycle.rs:590-595（仅精确匹配） | 🟡 | 谓词形式未移植；另增 matches_build（🔵） |
| contender 竞争池 | client/service-contender.ts:39-135 | lifecycle.rs:549-830 | ✅ | 时序常量与 ref service-timing.ts 一致 |
| pty-handoff sidecar | client/pty-handoff.ts:27-120 | oc-server/lifecycle.rs + boot.rs:802-820 | ✅ | 移位到 server lane |
| RPC runtime | rpc-runtime.ts:11-59, promise/rpc.ts:53-145 | oc-client/src/rpc_typed.rs | ✅ | G-122 有界偏离：方法表运行时校验 |
| effect/solid 双风味 SDK | client/src/effect/, solid/ | oc-client 单一 SDK | 🔵/N/A | Rust 无需双运行时 |
| protocol/simulation.ts (691行 JSON-RPC 仿真) | simulation.ts | grep 无对应 | 🔴 | 测试基建，运行时无依赖 |

**C. Util**
| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| encode/hash/glob/path/flock | util/src 对应 .ts | oc-util 对应 .rs | ✅ | 含测试矩阵 |
| global-roots/global | global-roots.ts, global.ts | oc-util/global_roots.rs (APP_NAME=opencode-rust) | ✅ | 根目录后缀刻意分叉 |
| retry | retry.ts:1-50 | oc-util/src/retry.rs:30-149 | ✅ | transient 消息表移植 |
| npm-config / npm | npm-config.ts / npm.ts | oc-util/npm_config.rs + plugin-host/install.rs | ✅/🟡 | 移位+去 Effect 化 |
| bom | bom.ts:11-35 | oc-core/file_mutation | ✅ | 移位 |
| artifact (mime/kind 表) | artifact.ts:12-83 | grep 全 crates 无 | 🔴 | 唯一消费方 packages/app（Web TUI），Rust TUI 未需要 |
| binary (search/insert) | binary.ts:3-30 | 仅局部 partition_point | 🔴 | 琐碎工具，低影响 |
| fs-util | fs-util.ts (284行) | 散布于 engine/resolver.rs 等 | 🟡 | 无单一模块 |
| open | open.ts | oc-tui/oc-cli 内联 xdg-open | 🟡 | |
| patch.ts（unified diff） | patch.ts (381行) | oc-core/tool/patch*.rs, file_diff.rs | 🟡 | 工具侧重实现 |
| process.ts / cross-spawn-spawner | process.ts, cross-spawn-spawner.ts | procutil.rs（组信号+pidfd）、bunman.rs（托管 bun） | 🟡/🔵 | 托管 bun 是刻意新设计（优雅降级、kill switch） |
| session-title-fallback | session-title-fallback.ts | session/runner/title.rs:68-73 | ✅ | MAX_LENGTH 100 |
| activity-calendar | activity-calendar.ts | oc-cli/stats.rs, oc-tui stats_page | ✅ | |
| observability(+otlp) | observability.ts | oc-server/observability.rs | ✅ | 移位 |
| effect/, runtime/, effect-flock | 1753 行 Effect 基建 | N/A | 🔵 | Rust 原生替代 |

**D. v1→v2 迁移 & oc-http**
| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| v1 config 迁移 | core/src/v1/config/migrate.ts:24-225 | config/normalize.rs:78-2150 | ✅ | write→edit/task→subagent/bash→shell、azure-cognitive→azure 等规则均在 |
| v1 DB session 内容迁移 | v1-migration.bun.ts | database/v1_migration.rs:306+ | ✅ | 1993 行逐段对应（RENAMED_TOOLS、legacy_tool_notice） |
| legacy auth.json 导入 | core/src/database | migrations.rs:68-75,749-820 | ✅ | 迁移链 id 与上游一致 |
| v1 permission 事件 schema | schema/v1/permission.ts | 仅 DB 清理迁移 | 🟡 | |
| v2(TS)→rust 数据目录迁移 | 无（上游就地） | v2_migration.rs:47-108 | 🔵 | 移植版新增（目录分叉必需） |
| oc-http G-26 TLS 统一 | Node 默认信任 + NODE_EXTRA_CA_CERTS 增量 | oc-http/lib.rs:33-73（rustls-platform-verifier） | 🔵/⚠️ | 未识别 `NODE_EXTRA_CA_CERTS`，ref 环境的企业 CA env 注入场景不等价 |

### 3. 关键差异与风险 Top 5
1. **⚠️ NODE_EXTRA_CA_CERTS 不被 oc-http 识别**（oc-http/lib.rs:33-61 vs ref websocket-constructor.ts:66）：依赖该 env 的企业部署在 rust 侧只有 OS 信任 Store 路径，行为不等价。
2. **🟡 v1 legacy schema 家族在 schema 层缺位**（oc-schema/lib.rs:80 自认）：要暴露/校验 v1 线上形状的调用点只能用 DB 迁移内部结构体，契约面缺失。
3. **🟡 matchesVersion 不支持谓词**（lifecycle.rs:590）：调用方传函数做兼容过滤的能力丢失；目前无 rust 调用点受损。
4. **🔵 数据根目录分叉 + 单向迁移**（v2_migration.rs、global_roots APP_NAME）：与 ref 就地迁移语义刻意不同；marker/锁策略健全，风险受控。
5. **🟡 solid/connection.ts 的空闲看门狗（45s/2s、首帧必须 server.connected）未在 client crate 找到逐常数对应**，重连语义分散到 oc-tui；半开连接检测是否等价需在 TUI 域复核。

### 4. 缺失文件/功能汇总
真缺失（grep 全 crates 确认）：
- `schema/v1/permission.ts`、`v1/filesystem.ts`、`legacy-event.ts`、`v1/session.ts`（作为 schema crate 公开类型；DB 内部形状已有）
- `util/artifact.ts`（mime/kind 映射表）、`util/binary.ts`
- `protocol/src/simulation.ts`（测试基建，或为有意不移植）

移位实现（非缺失）：rpc.ts→rpc_typed.rs；pty-ticket→methods.gen；durable-event-manifest→oc-server；npm→plugin-host；bom→file_mutation；service 探测/竞争/handoff→oc-server；observability→oc-server；title-fallback→title.rs。

结论：本域完成度很高——事件类型 104/104、API 方法 141/141 全集对齐，client 传输层为高保真移植且带偏离登记；风险集中在 TLS env 语义、v1 schema 契约面与 solid 重连看门狗三处。
