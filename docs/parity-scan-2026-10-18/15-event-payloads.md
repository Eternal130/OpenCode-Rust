# Parity Scan 2026-10-18 · 15 事件 payload 字段级矩阵（补漏通道）

- 参考实现：`/opt/opencode-ref`（opencode v2.0.24，TypeScript / bun，commit `e7a34f09bf`）
- 目标实现：`/opt/opencode-rust`（workspace 2.0.24，审计基线 `bb364b6`）
- 范围（ref）：`packages/schema/src/{event,event-manifest,session-event,session-status-event,session-compaction-event,location-event,mcp-event,vcs-event,installation-event,tui-event,server-event,legacy-event,permission,form,shell,pty,persistent-pty,project,worktree,credential,plugin,command,filesystem,integration,models-dev,websearch,config,session-inbox,session-message,session-error,money}.ts`、`packages/core/src/{bus.ts,credential.ts,pty.ts,persistent-pty/index.ts,vcs.ts,filesystem/location-watcher.ts,session/{session,projector}.ts,session/runner/publish-llm-event.ts}`、`packages/server/src/event-feed.ts`、`packages/protocol/src/groups/event.ts`、消费者 `packages/client/src/solid/data.ts`
- 范围（rust）：`crates/oc-schema/src/{event,session_event,events,misc_events,shell,form,session_inbox,common}.rs`、`crates/oc-core/src/{bus.rs,bus/registry.rs,pty.rs,persistent_pty.rs,mcp/registry.rs,session/{projector,runner/compaction,runner/title}.rs,engine/mod.rs,provider_catalog/refresh.rs}`、`oc-server/src/{sse.rs,announce.rs,handlers/{credential,session,project,pty,persistent_pty}.rs}`、`oc-tui/src/{store.rs,data.rs,app.rs,plugin_ui.rs}`
- 方法：**字段级**（名称 / 类型 / required-optional / null-vs-absent / 时间单位 / 枚举值 / 嵌套形状）逐事件比对 schema 定义 → 追每个事件的生产者 publish 点核对实际 JSON → 核对消费者读取字段。只读静态审计，未跑 build/test，未改产品代码。
- 前轮边界：事件**类型集合**缺失已清零（BE-1），本报告不重复；`05:F12`（公共流多余 3 类型）在本通道被**细化纠正**为多余 4+1（见 EV-4/EV-8/EV-9）。

---

## 0. 矩阵统计

| 维度 | 数量 |
|---|---:|
| 逐字段比对的 session 事件定义（含 durable/ephemeral、版本号） | 52/52（51 enum + `session.diff`） |
| 逐字段比对的非 session 事件类型 | 38/38（registry NON_SESSION 30 + pty/persistent-pty 6 + server 2） |
| 追踪到生产者 publish 点并核对实际 JSON 的事件 | 41 |
| 消费者读取面核对（oc-tui store reduce 表 30 臂 + solid data.ts 对照） | 30 + ref 对照 12 处 |
| **NEW 差异** | **6**（S1×1，S2×5） |
| **KNOWN 差异（本通道给出字段级细化）** | **3** |
| 跨 lane 纠错 | 1（`command.executed`） |
| 正向确认（字段级 1:1） | 见 §6 正向清单 |
| 信封/聚合/顺序（durable version 表、seq、幂等、重放） | 1:1，无差异 |

严重度定义沿用总报告：S0 主流程断裂 / S1 特定子系统损坏 / S2 边缘或契约级偏差。

**一句话结论**：schema 层（`oc-schema` ↔ `packages/schema`）是高质量逐字段移植——51 个 session 事件定义、durable v1/v2 版本表、`state` vs `providerState` 键名、`StepFailFinish` 字符串化等细节全部对齐；真正的字段级缺口集中在**绕过 oc-schema、用裸结构体序列化的三个生产者**（`persistent-pty.added` 的 `terminal`、`pty.created/updated` 的 `info`、`announce` 的 `project.updated`），一个**公共流过滤器的清单口径差**（`session.usage.recorded` 泄漏），和**TUI 侧缺失的 idle 客户端合成**。

---

## 1. 信封与公共类型（全事件共享）

| 项 | ref | rust | 结论 |
|---|---|---|---|
| durable 信封 | `{aggregateID, seq≥0, version≥1}`（`event.ts:27`） | `DurableInfo`（`oc-schema/src/event.rs:26-31`） | ✅ |
| ephemeral 信封 | 无 `durable` 键（`event.ts:121-143`） | `EphemeralEnvelope`（`event.rs:48-58`） | ✅ |
| `created` | `Schema.Finite`，epoch **毫秒**（bus `Clock.currentTimeMillis`） | `u64` 毫秒（`bus.rs:457-459`） | ✅ 单位一致 |
| `metadata`/`location` | optional，absent-when-none | `Option` + `skip_serializing_if` | ✅ |
| durable 版本表 | v2：`deleted/forked/instructions.updated/tool.success/tool.failed`，其余 v1（`session-event.ts:42-47,177-201,261-277,530-566`） | `SESSION_V2` 恰为这 5 个，其余 `SESSION_V1`（`registry.rs:86-140`） | ✅ |
| `Money.USD` | 有限浮点（`money.ts`） | `Usd(Num)` transparent（`common.rs:102-106`） | ✅ |
| `TokenUsage.Info` | `{input,output,reasoning,cache{read,write}}` | 同名 camelCase（`common.rs:141-157`） | ✅ |
| `SessionError` | `{type,message,status?,response?{body}}` | `error_type→"type"` rename + optional（`common.rs:178-194`） | ✅ |
| bus 发布是否过 schema 编码 | **否**——live 通知直发原始 data；`Schema.encodeUnknownSync` 仅用于持久化行（`bus.ts:328,599`） | 同——`publish(definition, serde_json::Value)` 直通（`bus.rs:620-667`） | ✅ **因此线格式以生产者 JSON 为准**（本报告逐点核对） |
| SSE 编码 | `JSON.stringify(event)` 原样（`event-feed.ts:29-31`） | `serde_json::to_string(event)` 原样（`sse.rs:210-234`） | ✅ |

**过滤口径差（EV-4 根因）**：v2 公共门 = `isOpenCodeEvent` ≡ `server.connected \|\| EventManifest.isServer \|\| rpc.*`（`protocol/groups/event.ts:67-68`），`isServer` 基于 `ServerDefinitions`，**不含** `session.usage.recorded` / `session.message.content.updated`（二者只在 `DurableDefinitions`，`session-event.ts:704-709`）。Rust 公共门 = `by_type(type_).is_some()`（`sse.rs:163-169`），而 `by_type_latest` 覆盖**全部** durable session 清单（含这两个内部事件）。

---

## 2. session.* 域（52 类型）

### 2.1 字段级矩阵（关键事件；✅=双侧逐字段一致）

| 事件 | 字段核对结果 | ref / rust 证据 |
|---|---|---|
| `session.created` | `sessionID,projectID,location,subpath?,parentID?,slug,title?,agent?,model?,metadata?,permissions?,version` 全一致（optional 全部 absent-when-none） | `session-event.ts:48-66` ↔ `session_event.rs:34-58` |
| `session.viewed` | `idle: Finite`（epoch **ms** 水位）；Rust `Num`。生产者取 body 或 now_ms | `session-event.ts:131-140` ↔ `handlers/session.rs:1338-1351` |
| `session.inbox.enqueued` | `inboxID + item`（`Item` 四变体 `user/synthetic/compaction/move`，`payload+delivery` tagged union；`UserPayload`=`Prompt.fields+metadata?`）逐字段一致；`EnqueuedTime.created` 为 **ms**（`DateTimeUtcFromMillis` 编码回 millis，`schema.ts:25-30`） | `session-inbox.ts:14-50` ↔ `session_inbox.rs:20-71` |
| `session.execution.interrupted` | `reason ∈ {user,shutdown,superseded,inactivity}`；Rust 枚举同集合（`superseded` 超集差异已登记 `01:R-02`，不重复） | `session-event.ts:253-258` ↔ `session_event.rs:244-260` |
| `session.step.started/ended/failed` | `started: ms`；`ended{finish,rawFinish?,providerState?,cost,tokens,snapshot?,files?}`；`failed{error,finish?="content-filter",cost?,tokens?...}`；`StepFailFinish` 手写 serde 输出字符串字面量（Wave C1 修复有测试锚定） | `session-event.ts:328-389` ↔ `session_event.rs:312-411` |
| `session.text/reasoning.*` | wire 键 **`state`**（非 `providerState`）——Rust 显式 rename 并有回归测试（`session_event.rs:1109-1178`） | `session-event.ts:391-466` ↔ `session_event.rs:413-485` |
| `session.tool.called/success/failed/progress` | `input: Record`、`content: NonEmptyArray<Content>`、`metadata?`、`executed`、`resultState?`；`progress.metadata: Record<string,Json>`。生产者（engine `progress` 回调）发 `{sessionID,assistantMessageID,id,metadata}` 与 ref `publish-llm-event.ts:562-570` 一致 | `session-event.ts:468-567` ↔ `session_event.rs:520-579`、`engine/mod.rs:1715-1731` |
| `session.compaction.started/ended/failed` | `reason∈{auto,manual}`、`recent`、`inputID?`；`ended` 的 `model/providerState/providerContext/cost?/tokens?` 与 `CompactionCompleted` 字段一致（cost/tokens 双侧 optional） | `session-event.ts:582-636`、`session-message.ts:240-275` ↔ `session_event.rs:593-646` |
| `session.retry.scheduled` | `attempt: PositiveInt, at: ms, error` 一致（01 lane 已核对发布点） | `session-event.ts:569-580` ↔ `session_event.rs:581-591` |
| `session.usage.updated`（ephemeral） | `{sessionID,cost,tokens}`；Rust 读 session_v2 行合计、`cost` 过 `Num` 保证整美元序列化为 `0` 而非 `0.0`（与 v2 JS number 行为一致） | `projector.ts:75-104` ↔ `projector.rs:548-591` ✅ |
| `session.shell.started/ended`（durable） | `{sessionID, shell: Shell.Info}` / `{..., output: Shell.Output}`；`Shell.Info` 11 字段（`status∈{running,exited,timeout,killed}`、`pid?`、`exit?`、`signal?`、`metadata` 恒存在、`time{started,completed?}` ms）逐字段一致；生产者：started 事件 id 由调用方 `msg_→evt_` 派生、ended 带 ≤1MiB preview、 vanished 兜底 `unavailable` 文案——与 `session.ts:198-219` 逐语义一致 | `shell.ts:22-58` ↔ `oc-schema/src/shell.rs:8-80`、`handlers/session.rs:1188-1336` |
| `session.forked`（durable v2） | `parentID,boundary,instructions?,instructionEntries?` 一致（01 已核发布点） | `session-event.ts:187-201` ↔ `session_event.rs:170-182` |
| `session.usage.recorded`（internal, durable） | schema 双侧一致 `{sessionID,source∈{title,compaction},cost,tokens}`；**但公共流可见性不同 → EV-4** | `session-event.ts:155-165` ↔ `session_event.rs:136-152`、`compaction.rs:632-640`、`title.rs:151-159` |
| `session.message.content.updated`（replay-only） | 双侧均无生产发布方（仅投影消费/重放）；Rust 过滤器会放行（潜在） | `session-event.ts:142-153` ↔ `projector.rs:186`、`message_updater.rs:250` |
| `session.diff`（v1 遗留 ephemeral） | Rust 注册且发布（`registry.rs:149-152,217-220`）→ 公共流多余，**KNOWN `05:F12`** | `v1/session.ts:634-639` ↔ `registry.rs` |
| `session.status` / `session.idle` | schema 一致（`status` 三变体 tagged `type:idle|retry|busy`，retry 带 `attempt/message/action?{reason,provider,title,message,label,link?}/next:ms`）；**双侧服务器均不发布**（ref 无任何 publish 点；Rust 有测试断言不发，`oc-server/tests/be1_lane.rs:276-358`）。消费侧差异 → **EV-5** | `session-status-event.ts:9-53` ↔ `oc-schema/src/events.rs:37-93` |

### 2.2 该域差异

（无 NEW schema 差异；`session.usage.recorded` 的公共流泄漏见 §3.1 EV-4，`state` 合并/progress 进 record 等 runner 侧差异属 `01:T-01/T-02` 已登记，不重复。）

---

## 3. 非 session 域

### 3.1 差异清单（EXPECTED vs ACTUAL）

| ID | 事件/字段 | EXPECTED（ref） | ACTUAL（rust） | 严重度 | 状态 |
|---|---|---|---|---|---|
| **EV-1** | `persistent-pty.added` → `data.terminal`（嵌套 `PersistentPty.Info`） | `{...Pty.Info(camelCase: `exitCode` 仅 exited 时出现), sessionID, foregroundProcess: string\|null, size:{cols,rows}, output:{head,tail}}`——`persistent-pty.ts:9-15`；生产者 `toInfo`（`persistent-pty/index.ts:390-408`）、publish `{sessionID, terminal}`（`:178`） | `serde_json::to_value(&Info)` 裸序列化（`persistent_pty.rs:677-684`）：键名 **`exit_code`/`session_id`/`foreground_process`**（snake_case）；`exit_code` 恒出现（运行中为 `null`，v2 absent）；**`size`/`output` 序列化为二元数组 `[cols,rows]`/`[head,tail]`**（元组字段，`persistent_pty.rs:124-128`），v2 为对象 | **S1** | **NEW** |
| **EV-2** | `pty.created` / `pty.updated` → `data.info.exitCode` | `Pty.Info` camelCase，`exitCode: optional`（absent）（`pty.ts:22-31`）；publish `{info}`（`core/pty.ts:243,251`） | 裸 `oc_core::pty::Info` 无 serde rename：`exit_code` snake_case 且恒出现（null）（`oc-core/src/pty.rs:57-68`；publish `pty.rs:520-527,584-588`）。注：HTTP 面（`handlers/pty.rs:36-48 info_json`）已正确映射 `exitCode`——仅 bus 事件错 | **S2** | **NEW** |
| **EV-3** | `filesystem.changed` → `data.event` 枚举值 / `data.file` | watcher 映射 create→**`add`**、update→**`change`**、delete→**`unlink`**（`location-watcher.ts:28-32`）；`file` = realpath 后 gitdir 的 `HEAD`（`:33-38`），hg location 观测 `<store>/branch`（`:40-44`） | 恒发 `"change"`（`announce.rs:547`）——HEAD **删除**时也发 `change`（fingerprint 归零触发）；`file` 为 `.git/HEAD` 直拼，普通目录情形不做 realpath（`announce.rs:465-483` 仅 linked-worktree gitdir canonicalize）；无 hg 分支观测（`11:#9` KNOWN 的组成部分） | **S2** | **NEW**（hg 部分 KNOWN） |
| **EV-4** | 公共 SSE 流事件集合（字段级后果：内部计费事件上线） | `isOpenCodeEvent` 基于 `ServerDefinitions`：**丢弃** `session.usage.recorded`（`event.ts:67-68` + `session-event.ts:704-709`）——v2 客户端永远看不到该类型 | `is_public_event` 经 `by_type_latest` 放行（`sse.rs:163-169` → `registry.rs:215`）；compaction/title 生产发布（`compaction.rs:632-640`、`title.rs:151-159`）→ **`session.usage.recorded {sessionID,source,cost,tokens}` 实际出现在公共流**。`session.message.content.updated` 同样过门但无生产发布方（潜在） | **S2** | **NEW**（细化 `05:F12`：多余实为 **4** 个已发布 + 1 个潜在） |
| **EV-5** | `session.execution.{succeeded,failed,interrupted}` 的 TUI 消费（v2 客户端合成） | solid 客户端在终态事件上：①置会话 idle；②插入 idle 标记消息 `{id: msg(evt), type:"idle", outcome: succeeded\|failed\|interrupted, time.created}`，`interrupted&&reason==="shutdown"` 跳过；③有 streaming/running 工具时强制 `session.invalidate + message.sync`（`client/solid/data.ts:1025-1062`） | Rust TUI reduce 表**无 `session.execution.*` 臂**（`store.rs:6636-7560` 全表 30 臂）；`"session.idle"` 臂（`store.rs:7166-7187`）在生产中**死代码**（双侧服务器都不发该类型；仅测试手动注入）。turn 收口 idle 标记只能靠**每个会话首次打开时的一次性** `message.list` 拉取（`app.rs:5205-5213`，`fetched_transcripts` 去重）；`drain_queued_prompts` 退化为 tick 轮询（`app.rs:5052-5056`）、`interrupt_clear` 无 live 触发点 | **S2** | **NEW**（消费侧；06 未覆盖此合成路径） |
| **EV-6** | `credential.switched` → `data.credentialID` | `Schema.NullOr(ID)`：**必填、可空**（取消激活时 `credentialID: null` 上线，`credential.ts:23`；发布 `:293,:317`） | 生产者正确发 `null`（`handlers/credential.rs:338-347` raw json!）；但**类型层** `CredentialSwitched.credential_id: Option` + `skip_serializing_if`（`misc_events.rs:99-106`）——经类型结构 decode(null)→None→re-encode 会**丢键**（round-trip 不对称；当前 SSE 直通 JSON 不受影响，属潜伏） | **S2**（潜伏） | **NEW** |
| **EV-7** | `project.updated` → 载荷字段 | `Info.fields` 全量：`id,canonical,vcs?,name?,icon?,commands?,time,sandboxes`（`project.ts:39-48,60`）——v2 发布完整 project 行 | announce 生产者只发 `id,canonical,sandboxes,time` 四字段（`announce.rs:245-257`），漏 `vcs/name/icon/commands`（schema 可选故线格式合法，但 v2 恒全量）；且 project 行未命中时发 **`{}`**（违反 required 字段） | **S2** | **KNOWN**（`09:D14`；本条给出字段级明细） |
| **EV-8** | `mcp.tools.changed` / `mcp.prompts.changed` 上公共流 | ServerDefinitions 只含 `mcp.status.changed`/`mcp.resources.changed`（`event-manifest.ts:79-80`）；`tools/prompts` 是全量清单成员、非公共 | Rust 注册进 NON_SESSION（`registry.rs:310-316,380-383`）→ 公共流多发；payload 本身 `{server}` 与 `mcp/index.ts:24` 一致 | **S2** | **KNOWN**（`05:F12`） |
| **EV-9** | `session.diff` 上公共流 | v1 遗留，非 ServerDefinitions | Rust 注册 + 发布 → 公共流多发 | **S2** | **KNOWN**（`05:F12`） |
| **EV-10** | `command.executed`（纠错条目） | v1 legacy-only（`v1/legacy-event.ts:8-9`）：不在 ServerDefinitions、v2 全仓无 publish 点 | Rust 同样不发。**`01` 文件映射表中「command.executed 事件缺失属事件 lane（auditE 1.14 KNOWN）」的表述应更正为「双侧 v2.0.24 均不发布，非公共流缺口」** | — | 跨 lane 纠错 |

### 3.2 正向确认矩阵（字段级 1:1，抽查证据）

| 事件域 | 字段核对 | ref ↔ rust |
|---|---|---|
| `pty.exited` / `pty.deleted` | `{id, exitCode: NonNegativeInt}` / `{id}`；Rust 生产者 `json!` 手写键名正确（`pty.rs:505-510,544,598`） | `pty.ts:36-37` ↔ `pty.rs` ✅（`unwrap_or(0)` 与 v2 daemon 缺省等价，footnote） |
| `persistent-pty.removed` | `{sessionID, ptyID}`（`persistent_pty.rs:664-669,834`） | `persistent-pty.ts:69` ✅ |
| `shell.created/exited/deleted`（全局 Shell 服务） | `{info}` / `{id, exit?, status∈{running,exited,timeout,killed}}` / `{id}`；`Info` 嵌套 11 字段 camelCase 正确（oc-schema 有显式 serde） | `shell.ts:55-58` ↔ `misc_events.rs:186-220`、`oc-schema/src/shell.rs` ✅ |
| `form.created/replied/cancelled` | `{form: Form.Info}` / `{id, sessionID: String, answer}` / `{id, sessionID}`；Form.Info 字段树（Option/When/Field 五型 tagged union）逐字段一致 | `form.ts:133-174` ↔ `misc_events.rs:222-258`、`oc-schema/src/form.rs` ✅ |
| `permission.asked/replied` | asked=`Permission.Request.fields`（`id,sessionID,action,resources,save?,metadata?,source?{type:"tool",messageID,id},message?`）；replied=`{sessionID,requestID,reply∈{once,always,reject}}` | `permission.ts:25-52` ↔ `misc_events.rs:161-184`、`oc-schema/src/permission.rs` ✅；TUI typed decode（`store.rs:6695-6707`）字段全存在 |
| `mcp.{tools,resources,status}.changed` / `mcp.prompts.changed` | 全部 `{server: String}`；Rust 四处生产者均 `{server: name}` | `mcp-event.ts:6-27`、`mcp/index.ts:24` ↔ `events.rs:124-155`、`mcp/registry.rs:416-810` ✅（公共流集合差见 EV-8） |
| `vcs.branch.updated` | `branch?` absent-when-detached：v2 `{branch: undefined}` 经 JSON.stringify 丢键 → `{}`；Rust `Option` skip → `{}`；发布时机=分支实际移动（v2 refresh 循环 `vcs.ts:143-152` ↔ `announce.rs:552-572`） | `vcs-event.ts:7-12` ✅ |
| `worktree.updated` / `worktree.resolved` | `{projectID}` / durable v1 `{projectID,directory,previous,adopted?}` on projectID 聚合 | `worktree.ts:55-69` ↔ `misc_events.rs:260-289`、`handlers/project.rs:520` ✅ |
| `credential.updated` | `{}`（global 路由）；Rust `publish_global` 一致 | `credential.ts:17-20` ↔ `handlers/credential.rs:167` ✅ |
| `installation.updated` / `installation.update-available` | `{version}` 无 location（进程级）；Rust announce/autoupdate 双生产点一致 | `installation-event.ts:6-18`、`process.ts:115-117` ↔ `announce.rs:283-295`、`oc-cli/services/autoupdate.rs:322-334` ✅ |
| `tui.prompt.append` / `tui.command.execute` / `tui.toast.show` / `tui.session.select` | `{text}` / `{command: 17 字面量∪String}` / `{title?,message,variant∈4,duration: PositiveInt 默认 5000}` / `{sessionID}`；Rust typed 结构 serde-default-5000 且恒序列化 duration——与 v2 decoding-default 后必在场等价；TUI 本地合成 toast 显式带 duration（`plugin_ui.rs:63-77`） | `tui-event.ts:9-58` ↔ `events.rs:300-364` ✅ |
| `location.shutdown` / `config.updated` / 九个 `*.updated` 服务帧 | 全部 `{}`；类型名清单一致（`integration/provider/model/agent/command/skill/websearch/reference/plugin`）；Rust 额外把 SERVICE_FRAMES 并入公共门（v2 这些本就在 ServerDefinitions） | `location-event.ts:6`、各域 ts ↔ `announce.rs:71-113`、`registry.rs:299-357` ✅ |
| `models-dev.refreshed` | `{}`；Rust catalog refresh 发布（`provider_catalog/refresh.rs:362`） | `models-dev.ts:5-8` ✅ |
| `server.connected`（SSE 合成首帧） | `{id,type,data:{}}`，**无 `created`**——双侧一致 | `event-feed`/`handlers/event.ts:13-17` ↔ `sse.rs:362-365` ✅ |
| `global.disposed` / `lsp.updated` / `session.compacted` / v1 legacy 族（`message.*`/`part.*`/`session.updated`/`command.executed`） | 均不在 ServerDefinitions 且双侧无发布方 → 双侧公共流都不出现；Rust TUI reduce 表中的 `"message.updated"/"session.updated"` 臂是**本地 HTTP 同步合成事件**（`data.rs:815`），非线格式 | `event-manifest.ts:86-101` ↔ `sse.rs:163-169` ✅ |
| `log.synced`（session log 水位帧） | `{type:"log.synced", aggregateID, seq?}`（seq 仅 target≥0） | `event-log.ts` ↔ `events.rs:366-378`、`bus.rs:1143-1148` ✅（05 已覆盖） |

---

## 4. 消费者矩阵（TUI store / SSE client / SDK）

| 消费点 | 核对结果 |
|---|---|
| Rust TUI `store.rs` reduce 表（30 臂） | 读取的字段（`sessionID/title/version/id/form/item/payload/text/delta/ordinal/assistantMessageID/name/input/metadata/content[].{text,name,uri}/error.message/finish/tokens.{input,output,reasoning,cache.{read,write}}/agent/model.{providerID,id}/reason/inputID/revert/to/idle`）**全部存在于对应 Rust 生产者 payload**；`session.tool.success` 拼接 `content` 的 `text→name→uri` 回退链与 v2 `data.ts:954-985` 一致 |
| Rust TUI `session.idle` 臂 | 生产死代码（EV-5）；臂内读取 `sessionID/id/created` 本身与 schema 一致 |
| Rust TUI `installation.*` | 读 `version`（`store.rs:6671-6688`）↔ 生产 `{version}` ✅ |
| Rust TUI `tui.toast.show` | 读 `title?/message/variant/duration`（`store.rs:6724-6756`）↔ schema ✅；未知 variant 回退 `info` |
| Rust TUI `worktree.updated` | store 注释声明按 v2 SSE refetch 流（`store.rs:683,3100`）——消费面存在（06 域） |
| v2 solid `data.ts` 对照 | execution 终态合成 idle（EV-5 差异）；`session.viewed/revert.*/compaction.*` 消费字段与 Rust 生产者一致；`session.retry.scheduled` 消费 `{attempt,at,error}` 一致 |
| `oc-client` SSE 解码 | 16MiB cap/多行 data/注释跳过——05 已覆盖，字段级无新增 |
| SDK（`types.gen.rs`） | OpenAPI 逐字节一致（05:§2）；事件流类型为自由 JSON，无 schema 绑定——EV-1/EV-2 的错形载荷对强类型 SDK 消费者（web/desktop 终端 UI）是实际解码失败点 |

---

## 5. 顺序与聚合

| 项 | 结论 |
|---|---|
| durable 聚合 | session 事件全部 `aggregate=sessionID`；`worktree.resolved` → `projectID`——双侧一致（`registry.rs:24-27` ↔ durable-event-manifest） |
| seq 分配 | per-aggregate 锁内 `latest+1`、`publish_all` 批量连续段、`(aggregate_id,seq)` 唯一索引兜底——与 `bus.ts` 一致（01 lane 已核，本通道复核字段级无差） |
| 幂等/重放 | `replay` 身份比对：v2 `isDeepStrictEqual`（键序无关），Rust 存储行 JSON→Value 等值比较（键序无关）——等价 ✅；`log.synced` 水位语义一致 |
| 提交序=可见序 | 双侧 persist-then-publish（Rust 结构化保证，`bus.rs:1-44` 文档化的四条不变量）✅ |
| sessionID 作用域/路由 | location 路由（created 建表、moved 双投、deleted 清除、forked 用 parent）——`routing.rs` ↔ `bus.ts:212-261`（01 已核）✅ |
| `session.usage.recorded` 排除公共流 | **差**（EV-4）：v2 靠 ServerDefinitions 排除；Rust 清单口径漏排 |

---

## 6. 汇总与正向清单

### 差异计数

| 类别 | 数量 | 明细 |
|---|---:|---|
| S1 NEW | 1 | EV-1 `persistent-pty.added.terminal` 错形 |
| S2 NEW | 5 | EV-2 `pty.created/updated.exit_code`、EV-3 `filesystem.changed` 枚举/路径、EV-4 `session.usage.recorded` 公共流泄漏、EV-5 TUI idle 合成缺失、EV-6 `credential.switched` 类型层 null→absent |
| S2 KNOWN（细化） | 3 | EV-7 `project.updated` 载荷（09:D14）、EV-8 `mcp.tools/prompts.changed`（05:F12）、EV-9 `session.diff`（05:F12） |
| 纠错 | 1 | EV-10 `command.executed` 双侧均不发（更正 01 映射表注记） |
| Footnote | 4 | ① `session.usage.recorded.cost` 裸 f64 → `0.0` vs v2 `0`（JSON 语义等价，仅字节级 golden）；② `pty.exited` `unwrap_or(0)`；③ `store.rs:6808` 注释称 inbox 事件 `created` 为「epoch SECONDS」——实际信封恒为 ms（注释错误，无行为影响）；④ Rust `size/output` 元组问题的 HTTP 面无恙（`handlers/persistent_pty.rs:49` `info_json` 正确映射） |

### 修复建议（按性价比）

1. **EV-1+EV-2（同一改动簇）**：给 `oc_core::pty::Info` / `persistent_pty::Info` 补 `#[serde(rename_all = "camelCase")]` + `exit_code` 的 `skip_serializing_if`，并把 `size/output` 改为具名结构（或复用 `info_json` 的手工映射序列化 bus 事件）。一处修复覆盖 4 个事件类型。
2. **EV-4**：`is_public_event` 排除 `session.usage.recorded` / `session.message.content.updated`（对齐 v2 的「DurableDefinitions 不进公共清单」语义）；顺带把 F12 的多余计数从 3 修正为 4+1。
3. **EV-3**：HEAD 删除分支发 `unlink`（fingerprint 归零判据已在）；`file` 路径补 canonicalize。
4. **EV-5**：TUI 增加 `session.execution.{succeeded,failed,interrupted}` 臂，按 `data.ts:1025-1062` 合成 `Message::Idle{outcome}` + shutdown 跳过 + running-tool 触发 transcript 重拉。
5. **EV-6/EV-7**：`CredentialSwitched.credential_id` 去 skip（可空必填）；announce `publish_project` 补全 `vcs/name/icon/commands` 并消除 `{}` 兜底。

### 正向确认（字段级 1:1，无差异）

- **schema 层**：52 个 session 事件定义逐字段一致（含 durable v1/v2 版本表、`state` vs `providerState` 键名、`StepFailFinish` 字符串字面量、`CompactionEnded.cost/tokens` optional）；`SessionInbox.Item/UserPayload/SyntheticPayload/MovePayload`、`Shell.Info/Output`、`Form.*`、`Permission.Request/Reply`、`Pty.Info`（schema 本身）、`PersistentPty.Info`（schema 本身）、`Project.Info`、`Worktree.{Updated,Resolved}`、`Credential.*`、`McpEvent.*`、`VcsEvent.*`、`TuiEvent.*`、`InstallationEvent.*`、`SessionStatus.Info`、`ServerEvent`、`LogSynced` 全部一致。
- **生产者层抽查一致**：`session.usage.updated`（含整型化 cost 的细节）、`session.shell.started/ended`（evt_ id 派生、1MiB preview、unavailable 兜底）、`session.tool.progress`、`permission.*/form.*/shell.*/pty.exited/persistent-pty.removed/mcp.*/vcs.branch.updated/worktree.updated/credential.*/installation.*/models-dev.refreshed/config.updated/location.shutdown`。
- **消费者层**：Rust TUI reduce 表 30 臂读取字段全部存在于生产者 payload；v2 solid data.ts 的消费字段在 Rust 生产者 payload 中全部存在。

---

## 7. 覆盖清单

**逐字段读过的文件对**（ref ↔ rust）：`event.ts`↔`event.rs`；`session-event.ts`↔`session_event.rs`（含测试锚定的 wire 断言）；`session-status-event.ts`↔`events.rs`；`session-inbox.ts`↔`session_inbox.rs`；`shell.ts`↔`shell.rs`+`misc_events.rs`；`form.ts`↔`form.rs`+`misc_events.rs`；`permission.ts`↔`permission.rs`+`misc_events.rs`；`pty.ts`↔`oc-core/pty.rs`；`persistent-pty.ts`↔`oc-core/persistent_pty.rs`；`project.ts`/`worktree.ts`/`credential.ts`/`mcp-event.ts`/`vcs-event.ts`/`installation-event.ts`/`tui-event.ts`/`location-event.ts`/`server-event.ts`/`legacy-event.ts`/`v1/session.ts`↔`misc_events.rs`+`events.rs`+`registry.rs`；`event-manifest.ts`↔`registry.rs`（清单口径）；`bus.ts`↔`bus.rs`（发布/编码路径）；`event-feed.ts`+`protocol/groups/event.ts`↔`sse.rs`（公共门）。

**生产者核对**：`bus.rs`（publish/publish_all/commit/replay/log）、`projector.rs`（usage fan-out/idle 投影）、`engine/mod.rs:1707-1731`（tool.progress）、`compaction.rs:628-640`、`title.rs:148-160`、`mcp/registry.rs`（4 类 mcp 事件）、`pty.rs`（4 类）、`persistent_pty.rs`（2 类）、`provider_catalog/refresh.rs`（models-dev）、`announce.rs`（boot/config/head-watch 三族）、`handlers/{credential,session,project}.rs`（credential 三点、shell started/ended、viewed、worktree.updated）、`oc-cli/services/autoupdate.rs`（installation）。

**消费者核对**：`oc-tui/src/store.rs:6636-7560`（reduce 全表 30 臂）+ `data.rs`（BusEvent 解码/ingest/HTTP idle 行）+ `app.rs:5030-5234`（tick drain / transcript 一次性拉取）+ `plugin_ui.rs`；ref `client/solid/data.ts:985-1105`。

**未覆盖/浅覆盖**：`message_updater.rs` 487 行的每分支内部逻辑（01 已建议另开投影 lane；本通道仅确认其消费的事件字段与生产者一致）；`oc-tui/mini/transport.rs` 的 mini 消费面（06 域）；PTY WS 帧协议（05:F1-F4 已覆盖，非本通道）；运行时行为（未跑双端差分，EV-1/EV-2 的错形以静态序列化路径为据，置信度高——struct 无 serde 属性 + `to_value` 直发是决定性的）。

---

*本报告为只读审计产物，未修改任何产品代码。*
