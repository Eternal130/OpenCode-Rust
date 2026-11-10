# S2 结论: oc-client 是高保真移植——142/142 端点、SSE/共享事件/RPC/服务生命周期语义对齐（少数已文档化的有意偏差），主要缺口在 ref solid/data.ts 的增量投影面：rust 以"全量 resync 快照"替代"串行 revalidate + 逐事件失效"模型，且数类 transcript 级事件无 live reducer。

## 对比表

| 面 | ref (packages/client) | rust | 判定 |
|---|---|---|---|
| 方法清点 | promise 生成客户端 **139** 个端点方法；effect 面由 `ClientApi` 契约声明式派生（同 139 端点） | `OPERATION_IDS` **142** = ref 139 + 3 rust-extra（`plugin.install`/`pty.connect`/`persistentPty.connect`）；`326 pub fn` = 142×(`x`+`x_with_options`) + ~42 namespace 访问器 | ✅ 与 S1 142 ops 一致；3 个 rust-extra 已声明 |
| 双 API 面 | effect + promise 同构双面 | 仅 promise 风格单面；serde 承担解码（默认容忍未知字段） | ⚠️ D-1 |
| fetch transport | 精确 successStatus 匹配、declared 错误 JSON 解码、UnexpectedStatus 弃 body、query 嵌套 `location[directory]`、headers 合并顺序 | 同精确 status、declared→`_tag`→name + 全 envelope 保留、UnexpectedStatus 读 ≤64KiB 诊断（G-288⑦ 有意偏差）、header 顺序一致 | ✅ 等价 |
| SSE 解码 | 16MiB cap（UTF-16 units）、CRLF 归一、`\n\n` 分帧、多 `data:` join、心跳跳过、onActivity | sse.rs 全套同构；G-126 cap 按 UTF-16 code units 精确计数（含代理对测试）、末块补 `\n\n` | ✅ 严格等价 |
| 事件双解码 | `/api/event` JSON-in-string 双解码 | `execute_sse(double_decode=true)` | ✅ |
| shared-events | `make<A>`：共享连接、引用计数、4096 容量、晚订阅者只补 `server.connected`、错误保持 identity、无自动重试 | 同契约 + G-125（失败后每次 `next()` 重复 Err）+ `terminal_error()` 保 identity | ✅（D-4 边缘差异） |
| 事件类型面 | OpenCodeEvent union | bus_event.rs `EventKind` 116 known types + `Rpc`/`Unknown` 前向兼容 + 漂移守卫测试 + oc-server manifest 锁步 | ✅ 增强（RE-06） |
| 重连游标 | 连接层无 Last-Event-ID（server 支持但客户端未带） | `subscribe_from(Last-Event-ID)` + store F2 replay 去重窗口 | ➕ rust-extra 增强 |
| rpc-runtime | read/readError/event（Schema 驱动）+ makeRpc（方法面生成 + events.subscribe 共享事件流） | rpc_typed.rs `call_into`（loud 解码）+ `RpcDefinition/RpcClient`（G-122：方法表校验、`rpc.<id>.<name>` 过滤、drop-guard unsubscribe） | ✅（D-5/D-6） |
| service 生命周期 | service-probe（404=不兼容、pid/version 校验）；service-contender（≤2 存活、8KiB stderr 环、指数退避）；service-timing（25ms/2s/5s/30s/120s）；service-version（string‖谓词） | **位于 oc-server/src/lifecycle.rs**：probe 分支逐条对齐；EnsureTiming 默认值逐一相同；ContenderPool 语义对齐；ensure 循环分支一一对应 | ✅ 结构迁移 + D-2/D-3 |
| pty-handoff | prepare（并发自查→handoff→失败 shutdown→0600 wx+rename 原子发布）、complete/clear、expiresAt 30s 兜底 | lifecycle.rs 同构（sidecar、same 判定、boot 收养） | ✅ |
| solid/connection | 2s connect 超时、首帧必须 `server.connected`、45s idle watchdog、1s 重连延迟、resync forced、flush 10ms batch | oc-tui data.rs `Connection::run`：connect window 覆盖订阅+首帧（G-300）、首帧类型校验、idle watchdog、指数退避、每次成功订阅后全量 resync | ✅ 策略等价（D-8） |
| solid/pty | token→WS URL（ticket/cursor/attachment_id/takeover/input_protocol 查询参数） | oc-core persistent_pty/frontend + oc-tui terminal_proto | 本 lane 只核挂接面 |
| solid/data 投影 | data.ts（1952L）：createSync 串行 revalidate、settle 150ms 防抖、per-location 资源缓存+事件失效、optimistic outbox 链、~60 事件 case | store.rs `reduce_event` + data.rs resync：核心 lane（text/reasoning/tool 状态机/step/inbox/compaction/revert/permission/form）对齐；**增量失效-再验证模型整体未移植**，由 resync 快照替代 | ⚠️ D-9/D-10/D-11 |

## 行为差异清单

- **D-1 双 API 面**：ref effect 面无 rust 对应；rust 单面 + serde。未知字段静默通过（Effect Schema 会拒）；`deny_unknown_fields` 未全局强制。
- **D-2 service 生命周期迁移+加固**：位置移到 oc-server/lifecycle.rs；新增 build stamp 校验（"build-mismatch"）、safekill 期望、fork 前 fd close-on-exec、`OPENCODE_PASSWORD*` env 过滤；terminate 最终失败 eprintln 而非 throw。
- **D-3 版本谓词缺失**：ref `version?: string | (v)=>boolean`；rust 仅 `Option<&str>` 精确匹配。
- **D-4 共享流错误完成后的缓冲语义**：ref `next()` 先排空 queued 再 reject；rust `Terminal::Failed` 立即弃 rx 并反复 Err。另：mpsc(4096) 可缓冲 4096 条 vs ref 最多 4095 入队——off-by-one，无实质影响。
- **D-5 RPC 事件连接复用**：ref makeRpc 的 events.subscribe 走 SharedEvents 共享一条 `/api/event`；rust `RpcClient::subscribe_events` 每订阅独占一条 SSE——多订阅时连接数放大。
- **D-6 rpc 输出严格性**：ref 透传 `result.output`（可 null/undefined）；rust 对缺失/null output 报 MalformedResponse（RE-06 有意）。
- **D-7 UnexpectedStatus body**：ref 弃 body；rust 保留 ≤64KiB 诊断体（G-288⑦ 文档化偏差）。
- **D-8 重连/恢复模型**：ref = 固定 1s 重试 + data 层串行 revalidate；rust = 指数退避 + `Last-Event-ID` 游标 + F2 去重窗口 + 每次订阅后全量 resync（含首次连接）。
- **D-9 transcript 级事件 live reducer 缺失**：ref 逐事件插入的 `agent-switched`/`model-switched`/`location-switched`/`system`/`synthetic`/`shell` transcript 行在 rust store 无 live case；相应 Message 变体仅由快照填充。idle 标记驱动事件不同：ref `session.execution.{succeeded,failed,interrupted}`（含 outcome）；rust `session.idle`（无 outcome 分型）。
- **D-10 增量失效-再验证未移植**：ref 的 per-location 缓存 + 九类事件→invalidate+refetch、settle 150ms 防抖、credential.switched 连接重排、worktree.resolved 采纳重写在 rust 无逐项对应——依赖下一次 resync 快照覆盖。
- **D-11 optimistic admission 未移植**：ref `outbox/sessionOutbox/sending` 链在 rust 无对应（发送路径归 T3 pump）；rust 的 `MessagesSnapshot/MessagesOlder` 保留"本地未确认行不被快照 wiping"合并语义 ✅。
- **D-12 事件批量**：ref flush 10ms + batch()；rust 单 mpsc 按序 ingest——顺序语义等价。

## 正确性风险

- **R-1（中）空白 delta 丢弃**：store.rs:13193/13252 对 `delta.trim().is_empty()` 直接 return；ref 原样追加（data.ts:909-913）。纯空白/换行 delta（段落间空行、代码块缩进换行）静默丢失，流式文本与 `text.ended` 终值可能不一致（终值又要求 text 非空才写回——ref 无此过滤）。建议按字节追加不过滤。
- **R-2（中）live 状态投影缺口**（D-9）：agent/model 切换、synthetic、instructions.updated、shell 回合行延迟到下一次 resync；`session.usage.updated` rust 只取 cost，ref 同时写 tokens——sidebar tokens 口径滞后。
- **R-3（低-中）错误完成丢弃已缓冲事件**（D-4）：TUI 有 resync 兜底，独立使用 oc-client 的调用方需知。
- **R-4（低）RPC 订阅连接放大**（D-5）：每 `on()` 一条 SSE + 服务器每连接一个 bus tap。
- **R-5（低）probe URL 拼接**：ref `new URL("/api/info", url)` 丢弃 basePath；rust 保留 path 前缀——反代前缀场景行为相反（rust 更合理但与 ref 不一致）。
- **R-6（低）serde 宽松解码**（D-1）：wire 漂移静默通过；仅 bus envelope 层 loud。
- 已知并已防护：G-11 snapshot/stream 双追加窗口、G-125 终止错误重复投递、G-300 connect window。

## 覆盖声明

- **ref 全读**：client/src 顶层 12 文件、promise/{client,rpc,service}.ts、effect/{api,client}.ts、solid 全 4 文件（data.ts 1952L 逐行）、promise/generated/client.ts（transport 核心 240-460 逐行 + 代表方法抽样 + 全部 139 path 计数）；effect/generated 确认为契约声明式派生（按清单+计数核验）。
- **rust 全读**：oc-client 8 个手写文件全文（含测试）；methods.gen.rs 读结构+OPERATION_IDS 全表+代表方法；types.gen.rs 仅结构核。
- **跨界核验**：oc-server/lifecycle.rs 1-1544（probe/ensure/stop/contender/timing/handoff 区段精读，1544-2675 serve/bind 区段未逐行——非 S2 面）；oc-tui data.rs Connection/resync 区段精读；oc-tui store.rs ingest/reduce_event 12440-13800 精读 + 缺失事件类 grep 全文核验。
- **未覆盖**：effect api/ 目录逐文件、solid/index.ts（3 行再导出）、测试文件除引证行外未读。
