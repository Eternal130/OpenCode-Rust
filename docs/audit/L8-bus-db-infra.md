# L8 基础设施 (Bus/Event/DB/KV/State/RPC/Job/Location) — 审查报告

> 审查方式：两侧逐行阅读 + 迁移 id 逐条 diff（48/48 一致）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
已逐行阅读：Ref 侧 bus.ts(908)、event-logger.ts、kv.ts+kv/sql.ts、event/sql.ts、state.ts(252)、rpc.ts(289)、job.ts(480)、location.ts/-activity/-lifecycle/-service-map/-services、repository.ts/repository-cache.ts、database/{database,schema.gen,migration.gen,migration,path}.ts，并核对 migration/ 目录 48 个文件名。Rust 侧 bus.rs(1412)+bus/{registry,routing,sink}.rs、database/{mod,schema,migrations,bootstrap,path,v1_migration}.rs、jobs.rs(1277)、location.rs(1227)、location_services.rs(953)、state_transform.rs(433)、reference/{repository,repository_cache}.rs、oc-server/boot.rs(event-logger)、oc-plugin-host/{lib,supervisor,registrations}.rs、oc-client/rpc_typed.rs。全仓 grep 验证后才标缺失。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| Bus.publish durable（锁/事务/seq 分配/projector/commit 钩子/event_sequence max-upsert/event 行插入） | bus.ts:284-484 | bus.rs:905-1102, 660-710 | ✅ | BEGIN IMMEDIATE 内同序执行；owner_id coalesce 语义一致 |
| Bus.publish 通知时序 | bus.ts:469-477（notify 在 aggregate 锁内） | bus.rs:702-708（先 drop guard 再 notify） | ⚠️ | 跨发布观察者可见 seq 乱序（见风险 #1） |
| publishAll（单聚合、单锁、单事务、latest+1..n、批内查重） | bus.ts:531-669 | bus.rs:716-872 | ✅ | 每事件独立采样时钟；通知同样在锁外（同上 ⚠️） |
| 事件 envelope/序列化 | bus.ts:50-108, 67-85 | bus.rs:104-161, schema.rs EventRow | ✅ | type 存 "{type}.{version}" |
| ephemeral 发布（无 DB、路由即时） | bus.ts:450-484, 494-505 | bus.rs:683-692, 877-899 | ✅ | |
| subscribe（live/typed/多类型过滤） | bus.ts:748-762 | bus.rs:1106-1129, 326-362 | ✅ | unbounded channel；`.local()` 链式等价 |
| location 路由（session.created/moved/forked/deleted、事务后应用） | bus.ts:212-261 | bus/routing.rs:94-174 | ✅ | moved 双路由/forked 继承 owner |
| log（分页 512、跳过未知 versioned type、唯一 log.synced 标记、follow 无缝衔接） | bus.ts:764-871 | bus.rs:1154-1313 | ✅ | 唤醒先注册、读前 re-arm，无间隙保证更明确 |
| replay（幂等回放、deep-equal 身份、diverged 报错） | bus.ts:671-701, 337-367 | bus.rs:1317-1355, 952-1010, 394-413 | ✅ | json_deep_equal 数值归一 (1==1.0) 为修偏（G-276②） |
| remove/claim/reserveSequence/latestSequence | bus.ts:21-48, 703-724 | bus.rs:630-656, 1359-1398 | ✅ | |
| beforeAggregateRead 钩子 | bus.ts:176-179, 769 | grep 全仓无 | 🟡 | ref 侧也无调用方，仅 option；影响为零 |
| event-logger（5 类 .updated 事件写日志） | event-logger.ts:7-17 | oc-server/boot.rs:161-190 | ✅ | 移至 server 层，类型集逐字一致 |
| Database 打开（pragmas/0600 权限/锁/WAL） | database.ts:33-98 | database/mod.rs:121-162, 321-383 | ✅ | Rust 加 WAL busy 重试与失败回滚 |
| schema 快照（19 表+16 索引） | schema.gen.ts:4-280 | bootstrap.rs:16-265 | ✅ | 表/索引清单逐项一致 |
| 48 条增量迁移 + drizzle journal 播种 | migration.gen.ts, migration.ts:22-147 | migrations.rs（id diff 48/48 一致）, :540-560 | ✅ | foreign_keys:false 包裹、legacy 前缀匹配均一致 |
| v1→v2 迁移 | v1-migration.bun.ts(1135) | v1_migration.rs(1993) | ✅ | 含 localeCompare 等保真细节；结构/关键函数一一对应 |
| KV get/set/remove/scan（prefix/after/limit 1..1000、prefixEnd、limit+1 探测） | kv.ts:36-90 | schema.rs:663-749, 845-869 | ✅ | |
| KV TTL | kv.ts 无 TTL | 无 | ✅ | 两侧均无 TTL，语义一致 |
| State（transform 链/分组失败禁用整组并从基重建/未分组失败上抛） | state.ts:160-252 | state_transform.rs:174-218 | 🟡 | 仅 agent 域；急切重建而非惰性+revision；无 batch/shutdown/reload/notify |
| State batch/shutdown/inherit/reconcile | state.ts:87-132 | 无对应 | 🔴 | ref 中被 plugin/config 域使用；Rust 无该运行时 |
| RPC register（多注册、at(-1) 最后注册生效） | rpc.ts:69-106 | plugin-host/registrations.rs:419-494 | ✅ | |
| RPC call 分发 + 错误 taxonomy | rpc.ts:108-142 | plugin-host/lib.rs:856-921, server/handlers/misc.rs:446-477 | 🟡 | 输入/输出 schema 校验与声明式 error 编解码下沉到 guest JS |
| RPC events.emit（发布 `rpc.{id}.{name}` ephemeral 事件） | rpc.ts:91-103, 195-201 | supervisor.rs:399-592 无 emit；全仓无生产者 | 🔴 | 客户端订阅端存在（oc-client/rpc_typed.rs:216-234）但无发布方 |
| RPC close（Deferred 关闭竞速挂起调用） | rpc.ts:41,108-137 | 依赖 guest 存活状态 | 🟡 | |
| Job start/wait/block/background/backgroundAll/cancel（25 条消费历史、recovery、timeout、blockingSessions） | job.ts:178-476 | jobs.rs:170-622 | ✅ | watch channel 代替 Deferred；cancel 语义等价 |
| Job pendingBackground/completeBackground | job.ts:441-463 | jobs.rs:561-595 | ✅ | LIKE 前缀扫描等价 kv.scan |
| 重启恢复 sweep | session/execution/restart.ts | jobs.rs:631-1037 | 🔵 | 移植版并入 jobs.rs，语义逐条对齐 |
| Location 服务（canonical key/构建失败 typed error/旧图存活/失效即 shutdown） | location-services.ts:12-87 | location.rs:797-1003, location_services.rs:219-342 | ✅ | `Arc` 旧图存活=ref "borrower 保留旧图" |
| Location activity TTL（60min/1min、re-arm、中断后重查） | location-activity.ts:16-90 | location_services.rs:344-443 | ✅ | sweep_once 逐条对应 |
| location.shutdown 事件 | location-lifecycle.ts:48-55 | location.rs:968-987 | ✅ | |
| project.activate（boot + 会话事件 60s 限频） | location-lifecycle.ts:32-39 | engine/mod.rs:916-944 | ✅ | |
| repository parse/validateBranch/cachePath/same | repository.ts:47-187 | reference/repository.rs:74-215 | ✅ | |
| repository-cache ensure（flock/reuse/daily 刷新/clone/fetch/checkout/reset） | repository-cache.ts:115-231 | reference/repository_cache.rs:171-320 | 🟡 | 刷新时间戳存 FileRefreshStore 而非 KV 表（持久化位置不同） |

### 3. 关键差异与风险 Top 5

1. **⚠️ durable 发布的通知在聚合锁外执行**（bus.rs:702-708、858-870 先 `drop(guard)` 再 `notify`；模块头注释自称"锁内通知"，与代码矛盾）。Ref 在锁内 notify（bus.ts:469-477），保证同一聚合的观察者严格按 seq 序可见。Rust 中 publish A 放锁后、notify 前，publish B 可完成全流程并先 notify → SSE/订阅者可能先见 seq2 后见 seq1。修复：notify 移回 guard 作用域内。
2. **🔴 插件 RPC 事件发布缺失**：ref `ctx.rpc.events.emit` 经 Bus 发布 ephemeral `rpc.{id}.{name}`；Rust 客户端订阅已实现但 plugin-host 无任何 emit 通路，且 `event.subscribe` 是 no-op——插件事件链路单向断开。
3. **⚠️ RPC 输入/输出校验下沉 guest**：ref 宿主做 method.input parse / output encode / 声明式 error 编解码；Rust 宿主只门禁 id/method，校验依赖 guest 内 JS 桥。恶意/异常 guest 可绕过 schema 契约。
4. **🟡 State 运行时大幅收窄**：ref State 是通用惰性重建 + batch/shutdown/reload/notify/revision，驱动 config/agent/command 全域热更新；Rust 仅 agent 域、急切重建。配置热更新粒度受限。
5. **⚠️ repository-cache 刷新时间戳持久化位置不同**：ref 写 KV 表（跨进程共享）；Rust 用独立文件——同时运行 ref/Rust 二进制时 daily 节流互不可见，可能导致重复 clone/refresh。

### 4. 缺失文件/功能汇总
- **缺失**：RPC events.emit 宿主通路（`rpc.{id}.{name}` 生产者）；State.batch/shutdown/inherit/reconcile/notify/revision 通用运行时；Bus `beforeAggregateRead` option（ref 亦无调用方，低危）。
- **位置/形态偏移（功能在但宿主不同）**：event-logger → oc-server/boot.rs；KV → Tx/Database 方法；RPC 校验 → plugin-host guest 桥；repository-cache 刷新戳 → 文件而非 KV；restart sweep → 并入 jobs.rs。
- **移植版新增（🔵）**：json_deep_equal 数值归一、WAL busy 重试、锁表/wake 表空闲驱逐、log follow re-arm 无间隙读、`Bus::scoped`、jobs `cancel_session`/RestartDeps。
