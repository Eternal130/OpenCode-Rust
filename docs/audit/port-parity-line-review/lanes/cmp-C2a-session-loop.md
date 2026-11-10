# C2a 结论: 会话/代理循环移植整体高度忠实（step/drain/retry/publisher/model-request 逐条语义对齐，含大量自证 G 编号偏差注释），发现 1 个 P1 事件序列偏差（中断后 hosted RESULT_MISSING 清扫）、若干 P2 wire/消息面偏差，无 P0。

## 对比表

| 功能 | ref 实现 | rust 实现 | 实现方式差异 | 状态 |
|---|---|---|---|---|
| drain 外层循环 | runner/llm.ts:54-196（入口门控/steer/queue、advanceToStep、step++ 仅 Completed 后） | runner/drain.rs:249-307 | rust 用 `RunFailure{surfaced}` 区分错误是否已投影到 assistant；ref 无此概念（由 session.ts 层消费） | ✅ |
| advanceToStep 边界 | llm.ts:71-186（inbox serialized、compaction/move 立即发布、cursor reset、promote、title fork、load） | drain.rs:311-563 | 逐条对齐（steer 保 step 计数 drain.rs:415-427 = llm.ts:106-109）；rust 在 ready 段用 `tokio::select!(shutdown)` 显式中断点（G-279⑥，ref 用 `restore`） | ✅ |
| plugins.awaitActivation | llm.ts:67（drain 前等插件激活） | 无对应（hooks 注册表在 engine boot 时同步就绪） | 缺失门控 | ⚠️ D-6 |
| 手动压缩失败发布 | llm.ts:137-157（interrupt/setup 失败才在此发布，避免双发） | drain.rs:436-488（`surfaced` 标志防双发，G-252①） | 语义等价实现 | ✅ |
| runStep 逻辑步策略 | llm.ts:206-297（initial 复用、auto compact、stepLimit、MAX_STEPS_PROMPT、toolChoice none、retry/recoverOverflow/recoverContinuation） | drain.rs:673-875 + model_request.rs:1353-1388 | RetryPolicy 提升到 runStep 作用域（正确：ref 也是每 runStep 一个 `SessionRunnerRetry.make` llm.ts:209） | ✅ |
| 单步 attempt 状态机 | step.ts:72-279（overflow 停靠、流读到尾、uninterruptible 终局 1-13） | step.rs:198-633（注释逐步标注 §1.4 1-13） | rust 单写者循环 + mpsc 工具通道替代 ref 的 fiber 并发发布 | ✅ |
| 事件序列 step.started→text/reasoning/tool→step.ended | publish-llm-event.ts 全文 | publisher.rs 全文 | startAssistant 幂等、ordinal、`publishPendingDeltas` 先于块 start、step-finish 先 flush：全部对齐 | ✅ |
| delta 100ms 批处理 | publish-llm-event.ts:79,145-205（forked timer 自旋 flush） | publisher.rs:96-233 + step.rs:259-289（deadline 加入 select 竞争） | rust 用循环内 deadline 事件替代 fork 定时器；flush 时机等价 | ✅ |
| 工具调度 | step.ts:100-133：Tool.Called 发布完成后 fork，fiber 内 truncate→toolExecution，decline 走类型化错误通道 | step.rs:311-347 spawn + run_tool_task（bell=watch），settle_tool_exit 在循环内 normalize→truncate→publish（G-308 顺序） | ref 工具 fiber 直接并发写 bus（per-source 序）；rust 全局单写者串行——事件交错拓扑不同但 per-tool 序保持 | ✅(D-3) |
| permission.ask / decline | model-request.ts:385-399 把 DeclinedError/CancelledError 从 defect 恢复为失败；step.ts:199-207 declines→failTool+Completed(locationClosed)/interrupt | model_request.rs:1035-1058 executor + ToolRunFailure::Declined；step.rs:525-537,620-625 | rust decline 消息由执行器带入（QuestionTool cancelled vs declined 文案在工具侧区分） | ✅ |
| Snapshot 捕获时机 | step.ts:73（attempt 起）+226-235（settle 时 capture，same→[]，diff→files，缺→undefined） | step.rs:201-217,568-601（G-275① 快照随 step.started） | 一致 | ✅ |
| Outcome 状态机 | step.ts:32-42/270-294 | step.rs:80-104 + drain.rs:845-873 | Compacted→recoverOverflow=false+新 msg id；Continue→synthetic 注入；RecoverFull→recoverContinuation=false：一致 | ✅ |
| 重试策略 | retry.ts:32-92（RETRY_AFTER_MAX 15min 钳制、2s 指数 cap10s、recurs10、jitter、timeout≤3、hook 覆写） | retry.rs:18-159 | 数值与流程一致；hook delay 归一化差异（D-4） | ✅ |
| retry.wait | retry.ts:97-113（RetryScheduled + 剩余 sleep） | retry.rs:163-190 + drain.rs:878-904 | 一致 | ✅ |
| 中断路径 | step.ts:136-144（stream 中断→interruptTools→awaitAll restore）；Effect.interrupt 语义 | step.rs:229-441（shutdown watch 锁存、bell、TOOL_JOIN_TIMEOUT 5s 有界 join 后 abort） | rust 增加 wedged fiber 5s 上限（ref 可无限等待）；中断后 re-raise 在终局之后：一致 | ✅(R-10) |
| hosted RESULT_MISSING 清扫 | step.ts:220-223：仅 `Exit.isSuccess(stream)` 时 | step.rs:557-563：`stream_error.is_none()` 未排除 stream_interrupted | 中断后 rust 多发 tool.failed(result-missing) | ❌ R-1 |
| unknown finish | step.ts:163-171：仅 `finish==="unknown"` | step.rs:482-489：`Unknown` **或 None**（G-105 自证偏差） | ref 协议层保证有 finish；rust 把无 finish 也判失败 | ⚠️ D-2 |
| 上下文溢出恢复 | step.ts:107-159（parked overflow、outputStarted 门、recoverOverflow→Compacted） | step.rs:224,295-309,461-470 | 一致（含 parked 发布时机在恢复判定后） | ✅ |
| is_retryable/isInterruptedStream/retry-full | step.ts:281-285 + @opencode/ai | runner/mod.rs:764-809（委托 oc_ai::provider_error） | 一致 | ✅ |
| max-steps 提示 | max-steps.ts 全文 | max_steps.rs 逐字 | 一致；toolChoice none + 保留工具定义（model_request.rs:1381-1383） | ✅ |
| model-request 构建 | model-request.ts:43-57 常量、87-199 outputLimit/媒体、219-401 hooks/headers/promptCacheKey/model.request/provenance guard/http+ws hooks | model_request.rs 同名常量与流程（204-233,382-433,619-1069,1087-1322） | identity/plan/optimize 从 ref plugin 移入 base_transcript（G-193）；openai omitOutputLimit 预省略（G-262①，与 ref 内建 hook 终态等价） | ✅ |
| 标题请求 kind | model-request.ts:411（title hook 无 agent/tools） | model_request.rs:683-704（title 走独立 dispatch、短路 result） | 一致 | ✅ |
| LLM 客户端边界 | aisdk.ts 1078L：SDKEvent/LanguageEvent hook、ai-sdk 路由桥、LanguageModelV3 适配 + aisdk-native.ts 包重写表 | runner/mod.rs:151-320 RouteLlmClient + oc-ai 原生协议状态机 + model_request.rs:792-828 aisdk sdk/language hook 分发（G-65） | 架构替换：JS SDK 桥→原生协议；aisdk hook 变换点保留；`aisdk:` 前缀自定义 SDK provider 无直接等价（依赖 oc-ai 已覆盖协议族） | ⚠️ D-9 |
| 无会话生成 generate.ts | generate.ts:39-94（ModelSelectionError/UnavailableError、x-opencode-session 头） | oc-server/handlers/misc.rs:42-118（直接流式，400/503 映射） | ref UnavailableError.service=providerID；rust 恒 "opencode"（R-3）；无 resolver 错误 tag 的 1:1 映射 | ⚠️ |
| 会话内生成 session/generate.ts | (session/generate.ts，ref) | runner/generate.rs（preview history + instruction update + prompt，无副作用） | 一致 | ✅ |
| model-transport（WS 会话通道） | model-transport.ts 571L（rotation 55min、idle 30min、失败 5 次回退、1009 回退、delivery 标注、interceptor） | runner/mod.rs:216-266 + oc-ai `route::transport::websocket::SessionChannels`（本 lane 外） | 生命周期参数未在本 lane 核对（未读 oc-ai 实现） | ⚠️ 覆盖声明 |
| to-llm-message | to-llm-message.ts 全文（附件/去重/providerMetadata 复用规则/model-switched effort/compaction checkpoint） | to_llm_message.rs 全文（含 G-279④ percent-decode、legacy media 升级） | 空 state record 被丢弃（ref 发 `{provider:{}}`）R-8；其余逐条对齐 | ✅ |
| title | session/title.ts | runner/title.rs（截断 100、MAX_CONTEXT 8000、first 2000、usage onInterrupt→Drop guard G-279③、乐观并发 rename） | FiberMap onlyIfMissing vs rust 完成后再跑（R-4） | ✅ |
| stale settlement | llm.ts:299-352 | drain.rs:934-1048（含 subagent sessionID 后缀、executed 透传） | 一致 | ✅ |
| agent 定义/默认 | agent.ts:18 defaultID "build"、Selection/Editor、selectable 过滤 | engine/mod.rs:3094-3218 seed_builtin_agents（"build"）+ AgentRegistry.select（drain/context 消费） | Editor transform 面在 oc-plugin-host registrations；本 lane 只核循环内 select/resolve 调用点 | ✅ |
| usage/tokens/cost 来源 | step-finish→SessionUsage.tokens→calculateCost→Step.Ended/Failed 附带；title→usage.recorded | publisher.rs:907-926,1104-1179 + mod.rs SessionUsage + title.rs | 一致（含 context-tier 定价、首匹配规则 mod.rs:668-698） | ✅ |
| temperature/effort/variant | generation options（hook 可塑）+ variant→resolver + Message.effort | model_request.rs GENERATION_KEYS/split_options + to_llm_message effort + engine/resolver variant | 一致（resolver 细节在 engine/resolver.rs，lane 外） | ✅ |

## 行为差异清单 D-n

- **D-1** 中断后的 hosted 清扫：ref `Exit.isSuccess(stream)` 排除中断流（step.ts:220）；rust `stream_error.is_none() && !provider_failed`（step.rs:557）不排除 `stream_interrupted` → 中断路径会额外发布 `tool.failed(tool.result-missing)`（hosted 工具）及可能的 `fail_assistant(result_missing)`。
- **D-2** unknown-finish 扩展：rust 把“流干净结束但无 finish 事件”也判 `unknown_finish_error()`（step.rs:482-489，自证 G-105）；ref 仅 `finish === "unknown"`（step.ts:163-171）。协议回归时 rust 失败该步，ref 静默完成（无 step.ended）。
- **D-3** 工具事件发布拓扑：ref 工具 fiber 与 provider fiber 并发各自顺序发布（publish-llm-event.ts:65-77 不变量 2）；rust 全部经单写者循环串行（publisher.rs 头注释）——per-tool/per-source 顺序保持，但 tool.success 与 text.delta 的全局交错形态不同（rust 的工具终态只能在流事件之间落地）。
- **D-4** retry hook delay 归一化：ref 对 hook 改写的非有限/负 delay 回退调度值（retry.ts:88-90）；rust 直接采用 u64（retry.rs:151-158）。
- **D-5** ModelUnavailableError 文案：ref 含 azure-cognitive-services / google-vertex-anthropic 弃用提示（model.ts:25-28）；rust `ModelError::Unavailable` 通用文案（mod.rs:445-446）。
- **D-6** drain 缺 `plugins.awaitActivation` 等价门（llm.ts:67 vs drain.rs:279 直接 settle）。
- **D-7** `experimental/generate` 错误映射：ref UnavailableError.service=resolved providerID（generate.ts:74-80）；rust 恒 `Some("opencode")`（misc.rs:38-40）；且 ModelSelectionError/UnavailableError 的 tag 体系折叠为 400/503 两个 ApiError。
- **D-9** aisdk 边界替换：ref `aisdk:` 前缀 + 插件提供 JS SDK/languageModel 的路径（aisdk.ts:314-359、providerOptionKey 439-453、callOptions 492-514、streamPartEvents 757-868）在 rust 中由 oc-ai 原生协议 + model_request.rs:792-828 的 sdk/language hook 分发替代；无 JS 插件 SDK 桥（plugin host 侧另论）。fragment 开合归一（openFragment/closeFragment 871-892）移入 oc-ai 协议 lifecycle（lane 外）。
- **D-10** TOOL_JOIN_TIMEOUT=5s 有界 join（step.rs:44,392-421）：ref 在不可中断终局内无限等待 fiber 自行退出；rust 超时 abort 并按 interrupted 分类（防呆增强，非 ref 行为）。
- **D-11** title 任务复用：ref `FiberMap.run(..., {onlyIfMissing})` 每会话仅一次（llm.ts:174-177）；rust `fork_title` 对已完成任务会再次 spawn（drain.rs:909-922，靠 generate 内 title 现值检查兜底）。
- **D-12** to_llm_message 空 provider state：ref 产出 `{[provider]:{}}`（to-llm-message.ts:106-109）；rust 空 record 返回 None（to_llm_message.rs:36-44）。对 reasoning meaningful 判定无影响（两边都按 keys>0），对 text 的 providerMetadata 回放有微小 wire 差异。
- **D-13** plan 提醒注入点：ref plugin 在 context hook 内且**另发持久 synthetic 消息**（model_request.rs:187-190 注释自认）；rust 仅请求组装期注入，不持久化。
- **D-14** maxTokens 对 provider "openai" 在 options 构造时即省略（model_request.rs:665-678, G-262①）——与 ref 内建 hook 的删除终态等价，但 hook 观察者（若有第三方 hook 读取 options）在 ref 能看到先插入后删除，rust 从未见过。

## 正确性风险 R-n

- **R-1 (P1)** 中断路径多发 `tool.failed("tool.result-missing")`（D-1，step.rs:557 vs step.ts:220）。用户中断一个含 provider-executed（hosted）工具且流未失败的场景：ref 只发 TOOLS_INTERRUPTED 清扫；rust 额外把 hosted 工具按“provider 未返回结果”落为持久失败事件，改变投影后的 tool 状态与下一轮请求上下文。建议：条件补 `&& !stream_interrupted`。
- **R-2 (P2)** G-105 无 finish 判失败（D-2）：对确实以非 finish 帧结束的协议实现（或网关截断后干净关流）行为从 ref 的“静默无终态”变为“步失败+step.failed 事件”。方向更安全，但与 ref 事件流不逐字节一致，golden/回放对比需知悉。
- **R-3 (P2)** `experimental/generate` 503 的 `service` 字段恒 "opencode"（misc.rs:39）而 ref 为 providerID（generate.ts:78）；客户端按 service 区分 provider 故障时误导。
- **R-4 (P2)** title 重新生成（D-11）在长会话多次晋升时可能重复计费一次标题请求（ref FiberMap 只跑一次）；有 is_untitled/current-title 守底，影响小。
- **R-5 (P2)** 无 `awaitActivation` 门（D-6）：慢加载插件的首个 drain 可能看不到插件注册的 hook/工具；engine 若保证 boot 完成后才挂 drain_fn 则无实际影响（engine/mod.rs:1875-1882 顺序上成立），但缺少显式屏障。
- **R-6 (P2)** retry hook delay 无归一化（D-4）：guest 桥送来的异常 delay 直接生效；数值域受限（u64）故无死循环风险。
- **R-7 (P2)** ModelUnavailable 弃用提示缺失（D-5）：azure-cognitive-services→azure、google-vertex-anthropic→google-vertex 迁移用户得不到指引文案。
- **R-8 (P2)** 空 provider state 的 `{provider:{}}` 回放省略（D-12）：个别 provider（依赖空对象占位回放签名）理论上可能拒绝；未证实。
- **R-9 (P2)** WS 会话通道（SessionChannels）的 rotation/idle/回退/1009/delivery 参数未在本 lane 内核对（ref model-transport.ts 全读，rust 实现在 oc-ai crate 外未读）——留待 oc-ai lane 交叉验证。
- **R-10 (P2)** TOOL_JOIN_TIMEOUT 5s abort（D-10）：被 abort 的工具副作用可能半途（无终态事件，仅 interrupted 分类）；ref 允许 fiber 在不可中断终局内跑完收尾。文档化偏差。

## 覆盖声明

**ref 侧全部读完（逐行）**：runner/{index.ts, llm.ts, step.ts, retry.ts, max-steps.ts, model.ts, publish-llm-event.ts, to-llm-message.ts, prompt/system.txt}；session.ts、agent.ts、generate.ts、aisdk.ts、aisdk-native.ts、session/{model-request.ts, model-transport.ts}。

**rust 侧全部读完**：runner/{mod.rs, step.rs, generate.rs, drain.rs, model_request.rs(1-1399, 尾部仅测试模块截断), retry.rs, max_steps.rs, plan.rs, optimize.rs(含 optimize/*.txt 以 include_str 引用、未展开 txt 内容), prompt_prepare.rs, system_prompt.rs, title.rs, publisher.rs, to_llm_message.rs, identity.rs, context.rs}；subagent.rs 全读；engine/mod.rs 仅读装配相关区段（1740-1890 + seed_builtin_agents 定位 grep，按要求“只读装配”）。

**测试文件**：按需原则未读（step.rs 内嵌 g308 测试随正文读到）。

**未读/部分依赖（不静默跳过，均已标注）**：oc-ai `route/transport/websocket`（SessionChannels 生命周期参数，R-9）；runner/compaction.rs 及 ref session/{context,compaction,title,history,prompt,instruction-state}.ts 原文（不在本 lane 范围，rust 侧对照依赖其源内 ref 行号注释，未独立核实 ref 行号准确性）；oc-server/handlers/misc.rs 仅读 generate 段（15-124）。
