# A2 结论: oc-ai 与 ref AI 管线/传输域逐行对齐度很高（cache-policy/effort/tool-history/provider-error/route 全链/WS 会话通道数值全对），唯一实质性缺口是 WS Interceptor seam（experimental.ws.* 钩子）构建后从未被消费，另有错误体 2KB 截断先于分类一处分类学风险。

## 对比表

| # | ref (file:line) | rust (file:line) | 对齐判定 |
|---|---|---|---|
| 1 | llm.ts:86-196 `generateObject`（合成 `generate_object` 工具 + ToolChoice.named + 首个匹配调用 + schema decode → InvalidProviderOutput） | llm.rs:61-108 | ✅ 逐行（仅 dynamic jsonSchema 模式，typed 模式用共享 validator 等价，llm.rs:9-15 自述） |
| 2 | llm.ts:39-84 `generate/stream/request` 入口 | 无独立入口函数；Route::generate/prepare_request（route/client.rs:331-375, 212-241）+ oc-core runner 组装 | ⚠️ 分散承载（见 D-8/D-10） |
| 3 | generation.ts:4-207 Status 机/5s 轮询/10min 超时/Queued/Progress/finished 事件 | media/model.rs:20-147（Status/Snapshot/事件/`DEFAULT_POLL_INTERVAL=5s`,`DEFAULT_POLL_TIMEOUT=600s`） | ⚠️ 类型与常量一致；Generation.await/events/resultEvents 的 deadline 语义未逐行 port（D-9） |
| 4 | ai-client.ts layer/layerWith(executor) | route/executor.rs + SharedTransport 注入 | ✅ 结构等价 |
| 5 | cache-policy.ts:17-195 AUTO/NONE/4-cap/RESPECTS_INLINE_HINTS(15 路由)/gateway/Qwen 策略/markLastTool/markSystem 首尾/markMessageAt/effort 跳过/manual 计入预算 | cache_policy.rs:13-345 + customization.rs:125-164 | ✅ 逐条含 `~` 前缀剥离、`anthropic/`/`qwen/`/`alibaba/qwen` 别名 |
| 6 | effort-updates.ts:5-24 | effort_updates.rs:11-72 | ✅ 逐行 |
| 7 | tool-history.ts:6-98 补缺 result/改名对齐/空输出替换/hold system 更新/providerExecuted 跳过 | tool_history.rs:39-223 | ✅ 主干一致；两处小差异 D-1/D-2 |
| 8 | provider-error.ts:16-339 34 条 overflow 正则/排除 4 条/代码集合/文本正则/分类优先级/providerCodes 15 抽取位/isRetryable | provider_error.rs:24-500 | ✅ 逐条件对照一致 |
| 9 | route/executor.ts retry-after(-ms/秒/HTTP-date)/OpenAI+Anthropic rate-limit 头/providerMessage(HTML 检测/2000 截断)/httpFailure | route/executor.rs:18-204 | ✅（HTTP-date 手写解析等价）；一处 R-2 截断差异在调用侧 |
| 10 | route/protocol.ts body/stream.initial/step/terminal/onHalt/supportsEffortUpdates/sanitizer | route/protocol.rs:30-74 | ✅（terminal 帧级 vs 事件级，由各协议等价实现） |
| 11 | route/client.ts resolveRequestOptions/prepareRequest(sanitize→dedupe→projection→effort→cache→headers)/streamPrepared/generate fold | route/client.rs:173-375, frames_to_events:482-558 | ✅ 管线顺序与语义一致；complete 时机差异 D-5 |
| 12 | route/framing.ts SSE(空/null/: keepalive/[DONE]/事件名过滤)/lines/document | route/framing.rs:19-360 | ✅ 过滤条件逐括号一致 |
| 13 | route/endpoint.ts path/baseURL/query/merge/trimBaseUrl | route/endpoint.rs:40-140 | ✅（补 percent-encode segment） |
| 14 | route/auth.ts credential 组合子/MissingCredential | route/auth.rs:62-300 | ✅ |
| 15 | route/transport/http.ts 超时预算(header/chunk 默认 300s、total 无默认) | route/transport.rs:216-409（DEFAULT_HTTP_TIMEOUT_MS=300_000） | ✅ |
| 16 | route/transport/websocket.ts MAX_FRAME_BYTES=16MiB/toWebSocketUrl/delivery 标注 | route/transport/websocket.rs:132-503 | ✅ |
| 17 | **core model-transport.ts(571L)** ROTATE=55min/CONNECT=15s/IDLE=30min/MAX_STREAM_FAILURES=5/1009 回退/delivery 三态/affinity/poison 仅 active 计数/rotation 丢 checkpoint | websocket.rs:519-1294 + oc-core session/runner/mod.rs:198-266 | ✅ C2a-R9 逐参数全对；缺 Interceptor seam 与 per-bind idleTimeout（R-1） |
| 18 | schema/events.ts 19 事件 + Usage 不变式 + LLMResponse reducer | schema/events.rs:148-814 | ✅ 全 19 变体字段级一致 |
| 19 | schema/messages.ts 全部 ContentPart/ToolOutput/Compaction XOR/ToolChoice 4 型 | schema/messages.rs:17-653 | ✅ |
| 20 | schema/options.ts HttpOptions 三超时/merge 族 12 字段/CachePolicy | schema/options.rs（含 :540-569 WebSocketSeams——Rust 新增承载） | ✅ |
| 21 | schema/errors.ts 12 reason + Transport 枚举 | schema/errors.rs:103-155+ | ✅ 枚举值逐字一致 |
| 22 | schema/ids.ts 7 个 ID | schema/ids.rs | ✅ |
| 23 | testing.ts TestLLM/push/always/serve/wait/gate | testing.rs:35-120+ | ✅（text 硬编码 `text_1` id，ref 要求显式 id） |
| 24 | tool.ts Tool.make 双模式/toDefinitions | Rust 无 Tool 对象（工具在 oc-core/tool，schema 走 ToolDefinition+json_schema.rs） | ⚠️ 分散承载（G-100 共享引擎） |
| 25 | tool-runtime.ts dispatch（No tool named/decode→execute→encode/事件对） | oc-core model_request.rs:1045 + tool/tests.rs:238 | ✅ 错误文案逐字一致 |
| 26 | provider-package.ts Settings/Definition | oc-core provider_catalog/facade.rs | ✅（A3 已证） |
| 27 | utils/sanitize.ts sanitizeSurrogates | sanitize.rs（构造性 no-op） | ✅ 论证成立 |

## 行为差异清单

- **D-1** pending 调用容器：ref `Map`（插入序）→ 补缺 missing tool results 按调用出现顺序排列；Rust `BTreeMap`（tool_history.rs:41）→ 按 id 字典序。多调用并发缺 result 时合成 part 顺序可能不同。
- **D-2** pending 追踪条件：ref `part.providerExecuted !== true`（含 `false`）；Rust 仅匹配 `provider_executed: None`（tool_history.rs:87）——`Some(false)` 的 tool-call 不进 pending、不补 result。现网生产者似只产 `Some(true)|None`，但 schema 允许 false。
- **D-4** Rust `SessionChannel::execute` 给 `send_text` 增加 30s 超时（websocket.rs:808-821，ref 没有）。
- **D-5** `complete()` 调用时机：ref 在 `streamPrepared` 用 `Stream.onEnd(execution.complete)` 无条件执行（client.ts:497）；Rust 仅在流 clean 结束时调用（session/runner/mod.rs:249-264）。多数情况等价，Rust 更严格。
- **D-6** ref `bind(sessionID, interceptor?, idleTimeout?)`（model-transport.ts:509-535）在 handshake/send/receive 三点应用 Interceptor、可按 bind 覆盖 idle 超时；Rust `SessionChannels::bind(&str)`（websocket.rs:1254-1265）无这两个参数——idleTimeout 仅剩全局 config。
- **D-7** `generateObject` ref 有 typed（Effect Schema）与 dynamic 双模式；Rust 仅 dynamic，typed 由共享 JSON-Schema 引擎近似（覆盖子集，llm.rs:112-119 列举）。
- **D-8** ref `request()` 自由函数在 oc-ai 无对应；Rust 由 `LLMRequest::with_*` 构建器 + oc-core model_request.rs 组装承载。
- **D-9** generation.ts `Generation.await/events/resultEvents` 的 deadline 零预算竞态防护（`within`）与 `Schedule.modifyDelay` 封顶；Rust 只 port 了 Status/Snapshot/事件/常量，轮询执行由各媒体协议自行实现，未见逐行等价物。
- **D-10** ref SessionModelTransport 的 Metric counter 无 Rust 对应（可观测性缺失）。
- **D-12** testing：ref `text(value, id)` 需显式 id；Rust 固定 `text_1`——仅测试工具面。
- **D-13** ref executor 的 `httpError` 链路在 Rust 由 `TransportError::into_ai_error` 简化承载；taxonomy 一致但 message 文案未逐字对齐。

## 正确性风险

- **R-1（高）** **WebSocketSeams（experimental.ws.* 插件钩子）编译后从未消费**。oc-core model_request.rs:974-988 在 hooks 注册时构建 `WebSocketSeams{handshake,send,receive}`（:1247-1318）存入 Prepared（:1065，类型 oc-ai schema/options.rs:566-569）；但 WS 执行路径 `stream_for_session → open_responses_channel::stream_channel`（session/runner/mod.rs:240-244 → open_responses_channel.rs:368-411）构造的 `WebSocketChannelExchange` 与 `SessionChannel::execute`（websocket.rs:714+）均不接收 seams——handshake/send/receive 三点重写在 Rust 不生效（ref model-transport.ts:309-314/375-377/430）。插件对 WS 会话通道的改写静默丢失。
- **R-2（中）** **错误体先截断后分类**。ref `statusError` 把完整 body 作为 `rawBody` 传入 `classifyProviderFailure`（executor.ts:103-111 → provider-error.ts:236-246）；Rust `status_failure_transport` 先 `read_body_limited(body, 2000)`（route/client.rs:457, transport.rs:171-186）再分类——>2KB 错误 JSON 中嵌套的 `error.code`/overflow 短语漏检，落到 `UnknownProvider`（retryable=true），改变重试行为；`http.body` 诊断也被截断。
- **R-3（低）** D-2：若上游产生 `providerExecuted: Some(false)` 的 tool-call，Rust 不补 missing result → 该调用悬空。
- **R-4（低）** D-1：合成 tool-result 顺序按 id 排序，对要求 call/result 顺序配对的 provider 极端场景可能拒绝。
- **R-5（低）** D-5：错误结束但 checkpoint 已 staged 且 ensuring 未清时 ref 仍提交 complete；Rust 不提交 → 下次全量重发（费用/缓存压力）。
- **R-6（信息）** D-4/D-13 为新增防呆或文案简化。

**C2a-R9 核验结论**：websocket.rs SessionChannels 与 ref model-transport.ts 逐参数一致——rotation 55min/connect 15s/idle 30min/失败 5 次回退/1009+rejected→httpFallback+当次重试/delivery 三态/poison 仅 active 计数/affinity/rotation 丢 checkpoint/connect 失败粘滞 fallback/terminal 错误帧丢 socket 全部一致；唯一缺项 = Interceptor seam（R-1）。

## 覆盖声明

- **逐行读完（双侧全文）**：llm.ts↔llm.rs、cache-policy↔cache_policy、effort-updates↔effort_updates、tool-history↔tool_history、provider-error↔provider_error（全部正则/代码集逐条）、route/{client,protocol,framing,endpoint,auth,auth-options,executor,executor-service}↔route/{client,protocol,framing,endpoint,auth,executor}、transport/http↔transport.rs（超时段）、transport/websocket+websocket-channel↔websocket.rs（1-1749 全文）、model-transport.ts(571L) 全文、schema/{events,messages,options,errors,ids}（events/messages 全文）、testing↔testing.rs、sanitize、tool-runtime、provider-package、ai-client、generation↔media/model.rs(1-147)。
- **结构+关键段核验（非逐行）**：route/media.ts/media-protocol.ts、schema/media.rs、customization.rs（gateway/alibaba 段全读，body-wrap 段略读）、json_schema.rs/test_support.rs（测试断言侧面验证）、route/transport/proxy.rs（隧道测试侧面验证）。
- **跨 crate 取证**：oc-core session/runner/{mod,model_request}.rs 的 WS 接线与 seams 断链（R-1）、tool/tests.rs dispatch 文案、transport.rs 超时断言。
