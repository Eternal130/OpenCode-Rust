# L11 AI 核心与媒体 (ai core) — 审查报告

> 审查方式：两侧逐文件逐行阅读，结论均以代码为准（不信任文档）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
已逐行阅读：Ref `packages/ai/src/` 全部根文件（llm/cache-policy/effort-updates/generation/image/speech/transcription/video/media/media-model/media-client/promise/provider-error/provider-package/provider/testing/tool-history/tool-runtime/tool 等）、`route/`（client/executor/endpoint/framing/auth/protocol/transport）、`schema/` 抽样核对、`providers/` 结构与入口；Rust `oc-ai/src/` 全部根文件（llm/cache_policy/effort_updates/customization/provider_error/tool_history/testing/sanitize/json_schema）、`route/`、`media/`（image/speech/transcription/video/route/model）、`protocols/mod.rs`，以及 `oc-core/src/engine/resolver.rs`、`provider_catalog/{catalog,facade,native}.rs`（aisdk 对应）。缺失结论均经全仓 grep 验证。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| llm.generate/stream 入口（request 归一化） | ai/src/llm.ts:44-81 | oc-ai/src/llm.rs:64-88 | 🟡 | Rust 只移植 `generateObject`；`generate/stream/request` 归一化直接走 `Route::generate/stream`（route/client.rs:346-380），`RequestInput`→`LLMRequest` 便捷层未移植（调用方自行构造） |
| generateObject（强制合成工具 + schema decode） | llm.ts:86-196 | llm.rs:64-135 | ✅ | 动态 jsonSchema 模式；typed 模式由共享 `json_schema::validate_input` 替代 Effect decode，错误路径/文案一致 |
| LLM 会话 stream 管线（compile→transport→frame→protocol 状态机） | route/client.ts:358-540 | route/client.rs:322-466 (`frames_to_events`) | ✅ | defaults 合并→sanitize/dedupe→cache/effort→body→transport→frame→step→terminal 强制，逐段对应 |
| 终止事件契约（terminal 后事件报错、无终止报 incomplete-stream） | route/client.ts:340-356,425 | route/client.rs:455-466,`incomplete_stream_error` | ✅ | |
| generate = stream 折叠 | route/client.ts:683-691 | route/client.rs:377-384 | ✅ | |
| abort | aisdk.ts:77-152（AbortSignal.any/header timer/chunk SSE watchdog）；promise.ts:55-70 | route/client.rs `stream` 为 async_stream（drop 即取消）；transport.rs:250 decode-or-drop | ⚠️ | Rust 取消= dropped future，语义可接受；但 ref 的 headerTimeout/chunkTimeout（SSE 读超时看门狗）在 oc-ai 层无直接对应（超时策略由上层/facade 承担），长挂流行为可能不同 |
| 重试/退避 | route/executor.ts（无自动重试，仅分类+retry-after 解析） | route/executor.rs:1-13 注释明确"无自动重试环"，决策在 `provider_error::is_retryable` + 上层 | ✅ | 两侧一致：库层不重试 |
| route 组合（Route.make/.with/.model 四轴） | route/client.ts:364-516 | route/client.rs:76-197 | ✅ | provider_metadata_key 随 provider 变更、defaults 深合并、baseURL 必填校验一致 |
| 请求默认值合并（route⊕model⊕request） | route/client.ts:299-316 | route/client.rs:199-248 `resolve_request_options` | ✅ | |
| 工具去重 + schema 投影 | route/client.ts:549-560 | route/client.rs:208-230 + protocols/sanitizer::project_tools | ✅ | sanitizeSurrogates 在 Rust 为天然 no-op（UTF-8 String） |
| WebSocket channel 传输 | route/transport/websocket-channel.ts（retry-full/rotate-and-retry） | route/transport/websocket.rs 存在 | 🟡 | client.rs:2 自述"minus websocket/media/compaction-endpoint lanes"；WS 存在于 transport 层，恢复策略（rejected/retry-full）覆盖度未逐一验证 |
| compaction（endpoint/trigger） | route/client.ts:214-280,636-660 | protocols/utils/responses_compaction.rs、responses_checkpoint.rs | 🟡 | Responses endpoint-compaction 有实现；`Route.compact` 通用 lane 与 trigger 机制未在 route 层暴露 |
| cache-policy（auto 断点：last tool/首尾 system/tail=1；4 断点预算；manual 计数；gateway/Qwen 特例） | cache-policy.ts:17-195 | cache_policy.rs:30-320 | ✅ | 逐条对应；gatewayPolicy/Qwen 移入 customization.rs（RC-06 seam），`RESPECTS_INLINE_HINTS` 15 条 route id 完全一致 |
| effort-updates（marker/strip/apply/resolve） | effort-updates.ts:5-25 | effort_updates.rs:11-63 | ✅ | 语义一致，含"last marker 与 current 不符则 strip" |
| provider-error 分类（context-overflow/payload/quota/auth/ratelimit/server/invalid/unknown + code/text 表） | provider-error.ts:8-339 | provider_error.rs:231-430（1401 行含测试） | ✅ | patterns/exclusions/QUOTA_CODES/AUTH_TEXT/GATEWAY_CODE_LABEL 等集合逐条移植 |
| isRetryable（x-should-retry 覆盖、Transport delivery 语义） | provider-error.ts:96-124 | provider_error.rs:263-300 | ✅ | |
| executor（retry-after-ms/秒/HTTP-date、OpenAI+Anthropic ratelimit 头、2000 字符截断） | route/executor.ts:23-110 | route/executor.rs:13-205 | ✅ | 含手工 HTTP-date 解析（Rust 无 Date.parse） |
| tool-history 归一化（pending map、held system、missing result 补齐、空结果占位） | tool-history.ts:6-98 | tool_history.rs:39-227 | ✅ | 常量 "(no tool output)"/"Tool result missing" 一致 |
| tool 定义（makeTool/动态 jsonSchema/toDefinitions） | tool.ts:44-253 | oc-ai schema ToolDefinition + json_schema.rs 验证器 | 🟡 | Rust 无 Effect Schema 双向 codec，decode 用共享 JSON-Schema 验证器；`toModelOutput/toStructuredOutput/_legacyResult` 投影链未见直接对应 |
| tool-runtime dispatch（namespace 拼接、无 handler/无工具错误、toolError+toolResult 事件对） | tool-runtime.ts:16-70 | grep 全仓无 `ToolRuntime::dispatch` 对应（oc-core engine/mod.rs、commands.rs 自行处理工具调用） | 🟡 | 核心执行循环在 oc-core session runner 中实现，事件对语义需该域确认；oc-ai 内无独立 runtime 模块 |
| media 统一层（Source bytes/base64/url/ref、Kind、Info、detectMediaType） | media.ts:13-323 | media/mod.rs、utils.rs、schema/media.rs | ✅ | |
| media-model / media route（inline/stream/queued、composeRoute、resume token） | media-model.ts:17-110、route/media.ts | media/model.rs、media/route.rs:802-1120 | ✅ | start/resume/refresh/timeout 均在 |
| generation 轮询（5s/10min 默认、deadline 边界、queued/progress/finished 事件、resultEvents） | generation.ts:28-207 | media/model.rs:139-147（常量一致）、media/route.rs:926-1000 | ✅ | |
| image 生成/编辑（openai edits multipart+JSON 双路、mask 校验；bfl/fal/google/meta/replicate/stability/xai/zai） | image.ts、providers/{black-forest-labs,fal,google,meta,replicate,stability,xai,zai} | media/image/{openai,bfl,fal,google,meta,replicate,stability,xai,zai}.rs | ✅ | openai.rs:22 `EDIT_PATH="/images/edits"`、mask+multipart 逻辑对齐 |
| speech（openai/cartesia/deepgram/elevenlabs/google） | speech.ts、providers 同名 | media/speech/ 同名 5 文件 | ✅ | |
| transcription（assemblyai/deepgram/elevenlabs/google/openai） | transcription.ts | media/transcription/ 同名 5 文件 | ✅ | |
| video（fal/runway/google/xai，含 fal 队列轮询） | video.ts、providers/{runway,fal} | media/video/{fal,google,runway,xai}.rs + fal_queue.rs | ✅ | |
| 内置 provider 注册（~50 个 TS provider 文件） | providers/*.ts（50 文件，~4.7K 行） | protocols/（~30 文件）+ openai_compatible 包装 + customization.rs seam | ✅ | Rust 以 openai_compatible+customization 折叠复刻，native.rs 表驱动验证"builtin_table_matches_ref" |
| models-dev 目录加载/刷新 | core/src/models-dev.ts:61-92 | oc-core/provider_catalog/{catalog,source,refresh}.rs（embedded+refreshed 事件） | ✅ | |
| 模型→provider 解析（canonical、custom apiKey/baseURL 覆盖、console 集成、keyless 降级） | core/src/model-resolver.ts | oc-core/engine/resolver.rs:246-460 | ✅ | 注释逐条引用 ref 行号 |
| npm provider 包加载（@ai-sdk/* via AISDK bridge、plugin hook） | core/aisdk.ts:254-560（SDK/language 缓存、hooks、providerOptionKey、callOptions、prompt 降级） | oc-core/provider_catalog/native.rs（静态映射表 native()/aisdk()）；无 JS 运行时 | ⚠️ | Rust 无法加载任意 npm AI-SDK 包：已知的 @ai-sdk/* 名映射到原生协议（native.rs:32-91、resolver.rs:877-894 匿名名单），但 ref 的"插件返回 SDK→languageModel 动态桥 + system 消息 wrap-user 降级 + tool-result media 附挂"管线无通用等价；未知 npm 包行为不同（ref 动态加载，Rust 归入兼容协议或失败） |
| aisdk-native（原生包优先） | core/aisdk-native.ts | native.rs `native()` 表 | ✅ | |
| provider-package.ts / provider.ts（结构化 Definition） | provider-package.ts:1-22、provider.ts:20-36 | provider_catalog/facade.rs（settings/baseURL/headers/body） | ✅ | |
| testing（TestLLM responders/gate） | testing.ts:23-259 | testing.rs:18-120 + test_support.rs（MockTransport 662 行） | ✅ | |
| AI Client（Effect Service 聚合） | ai-client.ts（24 行）、media/speech/…-client.ts | 无独立文件；能力分散在 oc-ai 各模块 | 🟡 | 服务聚合层（Layer 组装）在 Rust 中由 facade/构造函数替代 |
| promise.ts（Promise-first 门面、GenerationHandle、abort→reject） | promise.ts:1-214 | grep 全仓 crates 无对应 | 🔴 | 非核心语义，Rust 侧 async fn 天然 Promise 风格；但 GenerationHandle/abort-reason 语义无显式对应物 |
| 实验性 evaluation/system-one | experimental/*.ts | 无 | 🔴 | 明确未移植（实验特性） |

### 3. 关键差异与风险 Top 5
1. **npm AI-SDK 动态桥缺失（⚠️）**：ref 允许任意 npm provider 包经插件 hook 注入（aisdk.ts:334-343），Rust 只有静态映射表（native.rs:32-91）。自定义冷门 npm 包的请求体语义（providerOptions 键名、reasoning pro 降级 aisdk.ts:470-487）可能漂移。
2. **超时看门狗语义差异（⚠️）**：ref 的 headerTimeout 定时器 + chunkTimeout SSE 读超时（aisdk.ts:129-150）会主动 abort 连接；Rust route/transport 仅有 decode-or-drop（transport.rs:250），挂死流的故障表现不同（依赖上层超时）。
3. **tool-runtime 无独立模块（🟡）**：dispatch 的事件对（toolError+toolResult）与 legacy result 投影（tool-runtime.ts:52-62）由 oc-core session runner 承担，跨域需复核。
4. **Route.compact/trigger lane 未在 route 层暴露（🟡）**：仅 Responses 协议内部有 compaction（utils/responses_compaction.rs），其它协议的 endpoint compaction 扩展点不存在。
5. **WS 恢复策略覆盖度（🟡）**：websocket-channel.ts 的 rejected/retry-full/rotate-and-retry-full 状态机在 Rust transport/websocket.rs 的等价性未在本域范围内逐行验证。

### 4. 缺失文件/功能汇总
- `promise.ts`（Promise 门面/GenerationHandle）— 全仓 grep 无对应，标记缺失。
- `experimental/`（evaluation、system-one）— 未移植。
- `ai-client.ts` / `*-client.ts` 聚合层 — 无独立文件（能力已分散实现，非功能缺失）。
- `route/executor-service.ts`、`route/index.ts` barrel — Rust 模块组织替代。
- aisdk.ts 的插件动态 SDK 桥（语言模型缓存/hook/prompt 降级）— 无通用等价，仅静态映射。
