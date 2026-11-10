# C5a 结论: 事件总线与存储域移植高度忠实——52 会话事件 durable/ephemeral 分类与版本完全一致、48 个迁移 ID 全对齐、19 张终态表相同；差异集中在 rust 侧新增 `session.diff` 注册、非会话事件以 ad-hoc 定义发布、单 SQLite 后端及若干等价实现细节，无发现正确性级缺陷。

## 事件类型全量对照表

依据：ref `schema/src/session-event.ts:652-712`（Definitions/DurableDefinitions）+ `durable-event-manifest.ts:7` + `event-manifest.ts:41-101`；rust `oc-core/src/bus/registry.rs:86-238,386-447`。

### 会话聚合事件（ref 51 = 38 durable-v1 + 5 durable-v2 + 6 ephemeral + 2 内部 durable；rust 52 = 同集 + session.diff）

| 事件 | ref | rust | durable一致? | 负载一致? |
|---|---|---|---|---|
| session.created | durable v1 | durable v1 | ✅ | ✅ 12 字段逐一对应（session_event.rs:40-63 vs session-event.ts:48-66） |
| session.agent.selected | durable v1 | durable v1 | ✅ | ✅ |
| session.model.selected | durable v1 | durable v1 | ✅ | ✅ |
| session.moved | durable v1 | durable v1 | ✅ | ✅ MovePayload 字段 |
| session.renamed / metadata.updated / permissions | durable v1 | durable v1 | ✅ | ✅ |
| session.viewed | durable v1 | durable v1 | ✅ | ✅ idle 水位 |
| session.usage.recorded | durable v1（内部） | durable v1 | ✅ | ✅ source/cost/tokens（rs:143-149） |
| session.usage.updated | ephemeral | ephemeral | ✅ | ✅ |
| session.message.content.updated | durable v1（replay-only） | durable v1 | ✅ | ✅ |
| session.deleted | **durable v2** | durable v2 | ✅ | ✅ |
| session.forked | **durable v2** | durable v2 | ✅ | ✅ parentID/boundary/instructions/instructionEntries |
| session.inbox.delivered/enqueued/cancelled/delivery.changed | durable v1 ×4 | durable v1 ×4 | ✅ | ✅ enqueued 含 item（rs:200-206） |
| session.execution.started/succeeded/failed/interrupted | durable v1 ×4 | durable v1 ×4 | ✅ | ✅ interrupted reason 四值一致 |
| session.instructions.updated | **durable v2** | durable v2 | ✅ | ✅ |
| session.synthetic / skill.activated | durable v1 | durable v1 | ✅ | ✅ |
| session.shell.started / ended | durable v1 ×2 | durable v1 ×2 | ✅ | ✅ |
| session.step.started/streamed/ended/failed | durable v1 ×4 | durable v1 ×4 | ✅ | ✅ |
| session.text.started / ended | durable v1 | durable v1 | ✅ | ✅（wire 键 `state` 非 providerState，rs:450 有注释对齐） |
| session.text.delta | ephemeral | ephemeral | ✅ | ✅（rs:430-437） |
| session.reasoning.started/ended | durable v1 | durable v1 | ✅ | ✅ |
| session.reasoning.delta | ephemeral | ephemeral | ✅ | ✅ |
| session.tool.input.started/ended | durable v1 | durable v1 | ✅ | ✅ |
| session.tool.input.delta | ephemeral | ephemeral | ✅ | ✅ |
| session.tool.called | durable v1 | durable v1 | ✅ | ✅ input/executed/state |
| session.tool.progress | ephemeral | ephemeral | ✅ | ✅ |
| session.tool.success | **durable v2** | durable v2 | ✅ | ✅ content/metadata/executed/resultState（rs:554-566） |
| session.tool.failed | **durable v2** | durable v2 | ✅ | ✅ |
| session.retry.scheduled | durable v1 | durable v1 | ✅ | ✅ |
| session.compaction.started/delta/ended/failed | durable,ephemeral,durable,durable | 同 | ✅ | ✅ ended 8 字段一致（rs:619-636 vs ts:604-620） |
| session.revert.staged/cleared/committed | durable v1 ×3 | durable v1 ×3 | ✅ | ✅ |
| session.diff | ref 仅 v1 legacy schema，v2 core **无** bus 发布 | ➕ rust 注册为 ephemeral（registry.rs:150-153,220） | ➕ rust 独有注册 | ✅ {sessionID, diff} |
| worktree.resolved | durable v1 @projectID（worktree.ts） | durable v1 @projectID（registry.rs:234-237） | ✅ | ✅（misc_events.rs:269-276） |

### 非会话事件（全部 ephemeral；ref 静态 Definitions 清单 vs rust registry NON_SESSION + oc-server ad-hoc）

| 事件 | ref | rust | durable一致? | 负载一致? |
|---|---|---|---|---|
| location.shutdown | ✅ | ✅ registry[0] | ✅ ephemeral | ✅ {} |
| config.updated | ✅ | ✅ registry | ✅ | ✅ {}（misc_events.rs:72-76） |
| permission.asked / replied | ✅ | ✅ registry + sink.rs | ✅ | ✅ Request 字段/`{sessionID,requestID,reply}`（misc_events.rs:162-184 vs permission.ts:30-53） |
| project.updated | ✅ | ✅ registry | ✅ | ✅ 全 Project 字段 |
| mcp.tools/resources/status.changed | ✅ mcp-event.ts | ✅ registry ×3 | ✅ | ✅ {server} |
| mcp.prompts.changed | ✅（core mcp/index.ts:24 内联） | ✅ registry | ✅ | ✅ |
| pty.created/updated/exited/deleted | ✅ ×4 | ✅ ×4 | ✅ | ✅ |
| persistent-pty.added/removed | ✅ ×2 | ✅ ×2 | ✅ | ✅ |
| form.created/replied/cancelled | ✅ ×3 | ✅ ×3 | ✅ | ✅（misc_events.rs:222-258） |
| shell.created/exited/deleted | ✅ ×3（shell.ts:55-58） | ✅ ×3 | ✅ | ✅ |
| session.status / session.idle | ✅ | ✅ registry | ✅ | ✅（events.rs:69-93；ref 侧 status 含 Retry.action 结构一致） |
| filesystem.changed | ✅ | ✅ registry | ✅ | ✅ |
| worktree.updated | ✅ | ✅ registry | ✅ | ✅ {projectID} |
| vcs.branch.updated | ✅ | ✅ registry | ✅ | ✅ branch? |
| credential.updated/switched | ✅ ×2 | ✅ ×2 | ✅ | ✅ |
| models-dev.refreshed | ✅ | ✅ registry | ✅ | ✅ |
| tui.prompt.append/command.execute/toast.show/session.select | ✅ ×4 | ✅ ×4 | ✅ | ✅（events.rs:300-364） |
| installation.updated/update-available | ✅ ×2 | ✅ ×2 | ✅ | ✅ |
| integration.updated / reference.updated / plugin.updated / sdk.plugin.updated | ✅ ×4 | ✅ ×4 | ✅ | ✅ {} |
| agent.updated / model.updated / provider.updated / skill.updated / command.updated / websearch.updated | ✅ | ⚠️ 不在 bus registry，oc-server announce.rs:226-241 以 `EventDefinition::ephemeral` ad-hoc 发布 | ✅ | ✅ {} |
| lsp.updated / session.compacted / workspace.ready/failed/status / worktree.ready/failed / server.connected / global.disposed | ✅ schema 定义 | ❌ 无 oc-core 发布者（ref 同样基本无 core 发布者）；oc-schema 有 wire 定义、event_manifest.rs:209-216 标记 public | ✅ | ✅ schema 层 |
| log.synced | ✅ event-log.ts | ✅ events.rs:369-378 | n/a | ✅ `{type,aggregateID,seq?}` |

**小结**：会话域 52/52 分类（durable 版本 1/2 归属）与 ref 完全一致；唯一集合差是 rust 注册 `session.diff`（➕）。非会话域 6 个 registry 未列类型由 server 层 ad-hoc 发布，行为等价、结构不同。

## 表结构对照表

- **终态快照（bootstrap）**：ref `schema.gen.ts` 19 表 = rust `bootstrap.rs` 19 表，逐一名一致：account, account_state, control_account, credential, event, event_sequence, instruction_blob, instruction_entry, instruction_state, kv, permission, project, project_directory, session_inbox, session_message, session_pending, session_v2, workspace, worktree。索引亦对齐（event_aggregate_seq_idx 唯一 + event_aggregate_type_seq_idx 等，bootstrap.rs:250-265）。
- **历史全部 28 张**：48 个迁移文件 ID 逐一 diff **完全一致**（48 vs 48；清单 md 所称 "50" 不准确）；rust 以 `MigrationBody::Statements` 全量转写 + 4 个特化体（WorkspaceName/SessionMetadata/LoosePsylocke/ImportLegacyCredentials），loose_psylocke 双路径（marker rename vs 重建）与 ref 逐句对应（含 `20260730195856` marker 与 V1OnlyHistory 检查）。
- **journal 语义**：`migration(id,time_completed)` 无 hash 列；空库单事务 bootstrap + 全 ID 预写；legacy `__drizzle_migrations` 双变体（name 列 / created_at 前缀匹配）seed —— rust migrations.rs:552-614 与 ref migration.ts:22-146 一致，rust 额外在 `BEGIN IMMEDIATE` 内二次检查（跨进程竞态加固，ref 靠 Database 层文件锁）。
- **event/event_sequence**：列集一致。`event_sequence_upsert`（schema.rs:763-777）= ref bus.ts:404-415 的 `max()` + owner 保留。
- **kv**：Entry/ScanResult 形状、limit clamp(1,1000)+默认 100、`limit+1` 探测 next、`prefixEnd`（末位码点+1，`None`=无界）逐语义一致（schema.rs:663-750 vs kv.ts:11-90）。
- **后端**：ref sqlite.node/bun/workerd 三后端 + workerd `defer_foreign_keys` 回退；rust 单一 rusqlite（D-4）。
- **v1 迁移**：`transformSession`（warnings: invalid-message/orphan-part/invalid-part）、`status()` 三态、`run()`（global 兜底 project、分批清 event、sessions 游标降序逐条 + kv 检查点、next-database 导入、双源分母）阶段对齐（v1_migration.rs:308,1299,1327 vs v1-migration.bun.ts:222,493,534）。rust v2_migration.rs 将数据迁入隔离根（已知架构差异）。

## 行为差异清单 D-n

- **D-1** rust registry 将 `session.diff` 注册为 ephemeral 会话事件并纳入 `is_session_event` 路由集（registry.rs:152,220,269）；ref v2 仅有 legacy schema 定义、无 core bus 发布。影响：rust 发布该类型时会走 session_v2 路由 fallback——行为合理但为超集。
- **D-2** rust Bus 无 `Options.beforeAggregateRead` 钩子（ref bus.ts:174-176,769）。ref 侧实际无调用方传入，无行为损失。
- **D-3** 非会话事件注册方式不同：ref 全部进静态 `EventManifest.Definitions`；rust 只注册 oc-core 直接发布者（registry.rs:284-297 注释），agent/model/provider/skill/command/websearch.updated 由 oc-server `publish_empty` ad-hoc 构造 ephemeral 定义发布（announce.rs:226-241）。SSE 编码对未知类型 verbatim 直通，行为等价。
- **D-4** 数据库后端：ref 三 SQLite 后端 + workerd 外键切换降级；rust 仅 rusqlite，`foreign_keys:false` 迁移统一用 `PRAGMA foreign_keys=OFF`（事务外放松+恢复，与 ref node/bun 路径一致）。
- **D-5** `log()` 流通道：rust mpsc 容量 64（背压可阻塞发布侧 spawn 任务）；ref Effect Stream 无界。慢消费者极端场景下表现不同。
- **D-6** `Event.created`：ref `Schema.Finite`（允许浮点毫秒）；rust `u64`。wire 兼容。
- **D-7** `publish_all` 的 event_sequence 终写：ref 直接 `set seq=finalSeq`（bus.ts:643-646）；rust 用 `max()` upsert。聚合锁内 latest 同事务读取，二者恒等。
- **D-8** rust `PublishOptions.persist` 支持逐调用覆盖（bus.rs:196）；ref persist 仅 bus 级。rust 超集。
- **D-9** durable listener 失败：rust `eprintln!` 记录（bus.rs:513）；ref `Effect.logError`。等价。
- **D-10** rust 增加 lock-table 闲置逐出（bus.rs:466-468）与 `json_deep_equal` 数值归一（bus.rs:397-416，对齐 Node `isDeepStrictEqual` 的 `1==1.0` 语义，为正确性修正而非偏差）。

## 正确性风险 R-n

- **R-1（低）** `session.diff` 进入 SESSION_TYPES（registry.rs:266-272）后，若 rust 以不含 sessionID 的负载发布会静默路由为 `None`；发布路径需自查（server 侧有使用）。建议保持与 legacy `{sessionID, diff}` 形状。
- **R-2（低）** `Bus::latest_sequence`/`read_through` 每页都开事务（bus.rs:627-643,1266-1288）；高频 `log(follow)` 尾随写下放大写锁，仅性能层面。
- **R-3（低）** rust `replay` 不做 schema 解码（data 原样 JSON 透传，bus.rs:1333-1341）；ref `Schema.decodeUnknownSync` 会拒绝畸形历史数据。rust 对已损坏负载更宽容（ref die vs rust 通过）。
- **R-4（信息）** `route_ephemeral` 以空 envelope 占位构造 DurableEvent 走 routing（bus.rs:884-905）：与 ref prepareRoutes 对 ephemeral 会话事件的处理一致，无风险。
- **R-5（信息）** bootstrap 终态 19 表不含 v1 遗留 message/part/todo/session_share 等，而增量迁移路径两侧同样不删这些表——行为一致地保留遗留表（一致性成立，非移植缺陷）。

## 覆盖声明

**ref 侧全读**：bus.ts(908L 全)、kv.ts、kv/sql.ts、event/sql.ts、event-logger.ts、repository.ts(205L 全)、repository-cache.ts(249L 全)、migration.ts(147L 全)、schema/src/{event,event-log,event-manifest,durable-event-manifest,legacy-event,mcp-event,session-compaction-event,session-event(723L 全)}.ts、permission.ts(Request 段)、v1/session.ts(Diff 段)、shell.ts(事件段)、loose_psylocke.ts(头部+DROP 清单)。**清点+代表性细读**：migration/ 48 文件——全部 ID diff 逐一比对（48/48 一致），正文以 rust migrations.rs(947L 全读)转写为锚 + 抽样回对 ref 6 个；v1-migration.bun.ts(1135L) 读结构+关键 ~250L。**未读**：database.ts、sqlite.{node,bun,workerd}.ts 驱动内部、schema.gen.ts 仅表/索引结构 grep（19 表+10 索引）。
**rust 侧**：bus.rs(1421L 全)、bus/{registry.rs(536L 全),routing.rs(174L 全),sink.rs(头部)}、database/{migrations.rs(947L 全),bootstrap.rs(ID+表/索引段),schema.rs(row 宏+KV/event 段 ~250L),mod.rs(结构),v1_migration.rs(结构+run ~300L)}、oc-schema/{event.rs(104L 全),events.rs(444L 全),misc_events.rs(389L 全),session_event.rs(~700/1184L——52 个事件结构体与枚举主体已读，revert 尾段与测试略)}。
**结论可信度**：事件类型全集对照与 durable 分类为全量级证据；8 个抽样事件字段级对照全部一致；迁移为“ID 全量+正文转写侧全量+ref 抽样”级证据。
