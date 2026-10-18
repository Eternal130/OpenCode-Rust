# 16 · Copilot 深扫 + AI/provider 补漏（第二轮）—— Parity 扫描（2026-10-18）

- 参考实现：`/opt/opencode-ref`（opencode v2.0.24，`e7a34f09bf`）
- 目标实现：`/opt/opencode-rust`（基线 `bb364b6`）
- 定位：第二轮「补漏」通道，覆盖第一轮文件级扫描未深入的四个面：① `packages/core/src/github-copilot/**`（25 文件 / 4,335 行）逐能力核对；② 第一轮 basename 未被引用的 `packages/ai` 协议/provider 文件；③ 22 个已移植媒体协议的死代码/可达性判定；④ 9 个不可绑定 `aisdk:` provider 的量化影响 + `plugin/provider/{dynamic,factory,prompt-cache-key,sdk-factory}.ts`。
- 方法：只读静态审计；双侧 `path:line`；已登记项对照 `00-MASTER.md`、`03-ai.md`、`04-provider-auth.md` 去重。未跑 build/test。

---

## 1. GitHub Copilot：Rust 是「等价重写」还是「只覆盖 chat 子集」？

**结论：只覆盖 chat 子集。** ref 是一个 25 文件、三协议（chat / responses / `/v1/messages`-anthropic）、五会话 hook 的完整子系统；Rust 侧 = `oc-ai/src/protocols/copilot.rs`（138 行，openai-chat re-badge + 头策略）+ `oc-core/src/integration/copilot.rs`（147 行，device 凭据→route 绑定）+ `facade.rs:252-255` 的 `CopilotChat` family。responses lane、messages lane、模型目录动态同步、全部会话 hook（除凭据）缺失。catalog 34 个 copilot 模型在 v2 中约 **21 个（62%）按 id 规则走 `/responses`**（v2 注释明示 Grok/Gemini/MAI Code「仅在 /responses 提供」），Rust 一律打 `/chat/completions`。

### 1.1 文件映射总表

| ref 文件（行数） | rust 对应 | 状态 |
|---|---|---|
| `copilot-provider.ts` (97) | `oc-ai/src/protocols/copilot.rs:119-135` `copilot_chat_route` | **PARTIAL**：仅 chat 半（`createResponsesModel` 无对应） |
| `models.ts` (234) | 无 | **MISSING**（KNOWN `04:P-06`）：`/models` 远端同步、usable 过滤、derive 合并、`/v1/messages`→anthropic 包重写 + `${baseURL}/v1`、endpoint 设置、AIC→USD 计费换算、三族 variants（reasoningEffort+summary+include / adaptive+effort / budgetTokens max-1 与 max/2）、`model_picker_enabled`→enabled |
| `openai-compatible-error.ts` (27) | `oc-ai/src/provider_error.rs`（OpenAI error 结构） | OK |
| `README.md` (38) | — | N/A（文档；其「intentional Copilot adaptations」清单即本节差异清单的权威来源） |
| `chat/openai-compatible-chat-language-model.ts` (817) | `oc-ai/src/protocols/openai_chat.rs`（CopilotChat family 复用） | **PARTIAL**（见 C2/C6） |
| `chat/convert-to-openai-compatible-chat-messages.ts` (170) | `openai_chat.rs` 消息 lowering | **PARTIAL**：`reasoning_opaque` 回放缺（C2） |
| `chat/get-response-metadata.ts` (15) | 协议内 response-metadata 事件 | OK |
| `chat/map-openai-compatible-finish-reason.ts` (19) | `openai_chat.rs` finish 映射 | OK |
| `chat/openai-compatible-api-types.ts` (64) | wire 类型（内联） | OK |
| `chat/openai-compatible-chat-options.ts` (28) | `openai_chat.rs:1782-1792` `resolve_chat_options` | **PARTIAL**：只读 `store`/`reasoningEffort`；`user`、`textVerbosity`→`verbosity`、`thinking_budget` 不消费（C6） |
| `chat/openai-compatible-metadata-extractor.ts` (44) | — | N/A（v2 未给 copilot 挂 extractor，纯类型） |
| `chat/openai-compatible-prepare-tools.ts` (83) | `openai_chat.rs:1852-1873` `lower_tool` | OK（Rust 在嗅探通过时显式 `strict:false`，ref copilot 省略该字段——语义等价的 EXTRA） |
| `responses/openai-responses-language-model.ts` (1636) | 无 | **MISSING**（C1）：含 copilot 专属的「encrypted item ID 每事件轮换」状态机（`activeReasoning` 按 output_index 记 canonicalId、summaryParts active/can-conclude/concluded、`store:false` 下延迟 close，ref :736-748,1106-1151）、稳定 text id 归一（:750-752,1084-1101）、logprobs/serviceTier 元数据、hosted 工具回放（web_search action+`web_search_call.action.sources` include、code_interpreter partial code JSON 转义、image_generation partial_image、file_search、computer_call） |
| `responses/openai-responses-prepare-tools.ts` (236) | 无 | **MISSING**（随 C1） |
| `responses/convert-to-openai-responses-input.ts` (275) | 无 | **MISSING**（随 C1；`copilot.itemId` namespace + store 相关 item_reference/reasoning 回放语义） |
| `responses/map-openai-responses-finish-reason.ts` (22) | 无 | MISSING（随 C1） |
| `responses/openai-config.ts` (18) / `openai-error.ts` (22) / `openai-responses-api-types.ts` (189) / `openai-responses-settings.ts` (1) | 无 | MISSING（随 C1；类型/错误结构本体在 `open_responses.rs` 有等价物，但不在 copilot 命名空间下） |
| `responses/tool/{web-search(21), web-search-preview(40), file-search(127), code-interpreter(87), image-generation(25)}.ts` | 无（`oc-ai/protocols/utils/responses_hosted_tools.rs` 是 packages/ai open-responses 的对应，非本目录） | MISSING（随 C1） |

消费方 `plugin/provider/github-copilot.ts`（558 行）逐 hook：

| hook | ref | rust | 状态 |
|---|---|---|---|
| `aisdk.sdk` (252-265) | `copilotFetch` 头包装 + `createOpenaiCompatible` | `facade.rs:397-408` CopilotChat family + route headers seam | PARTIAL（chat 半等价） |
| `session.model.request` (266-280) | `X-Interaction-Type`（词汇表）+ `X-Interaction-Id`（`SessionAffinity.get`=parentID??fork??id）+ 非 agent-loop 时强制 `x-initiator:agent` | `oc-ai/copilot.rs:50-57` `interaction_type()` **存在但零生产调用方**（全仓 grep 仅测试）；`model_request.rs:624-666` 有 hook 缝但无 copilot 生产者 | **MISSING**（C3） |
| `session.title` (284-303) + `utilityTitle` (402-462) | 免费 `gpt-4o-mini` 标题（`X-Interaction-Type: agent-session-name-generation`、20s 超时、失败回退常规路径） | 无 | **MISSING**（C5） |
| `session.http.request` (304-316) | `/v1/messages` 请求（`x-api-key` 判定）头改写 + `anthropic-beta: interleaved-thinking-2025-05-14` | 无 | **MISSING**（C4，随 messages lane） |
| `aisdk.language` (317-344) | endpoint 选择：显式 `endpoint` 设置优先；否则 `gpt-(\d+)>=5 且非 gpt-5-mini`、`grok-`、`gemini-`、`mai-code-` → responses，其余 chat | 无（Rust 固定 chat） | **MISSING**（C1 的选择器半） |

### 1.2 差异明细

#### C1【S1 · KNOWN（03:§3.7/§7 已登记为 S2，本报告凭证据升 S1）】responses lane 整面缺失 + endpoint 选择缺，34 个 catalog 模型中 ~21 个路由错误
- EXPECTED：ref `github-copilot.ts:317-344`（`language` hook，无远端同步也生效）+ `models.ts:130-137,162-166`（远端同步后 per-model `endpoint`/anthropic 包重写，KNOWN `04:P-06`）。catalog 34 模型中按 regex 命中 responses 的 21 个：gpt-5.4/5.4-nano/5.4-mini/5.5/5.6-luna/terra/sol/5.3-codex/6-astra/6-luna/6-sol/6.1-sol、grok-4.5/4.6/4.7、gemini-3.5/3.6/3.7/3.8-flash、mai-code-1-flash-picker/1.1-flash。ref :334-341 注释：「Copilot serves Grok, Gemini, and MAI Code **only on /responses**」。
- ACTUAL：`facade.rs:252-255` 把 github-copilot 的全部 catalog 模型（snapshot 34 个均无 per-model package，`models-dev-snapshot.txt`）绑到 `ProtocolFamily::CopilotChat` → `copilot_chat_route` → `/chat/completions`（`facade.rs:854` 测试断言）。`oc-ai` 全仓无 copilot responses 适配。
- 影响：上述 21 个模型选中即 404/不可用——占 copilot 目录 62%。gpt-5-mini 与 12 个 claude/kimi 走 chat，可用（claude 在 v2 有远端同步时才走 `/v1/messages`）。
- 备注：03 报告将「copilot responses lane」记 S2（§7）；本报告以 regex 规则 + catalog 枚举实证上调。`04:P-06` 覆盖远端同步半（`github-copilot.ts:172-251`），`language` hook（:317-344）不在其引用范围内，属**未被登记的独立缺口**（NEW 半）。

#### C2【S1 · NEW】chat lane 的 `reasoning_opaque` 多轮推理续链全缺
- EXPECTED：`chat/openai-compatible-chat-language-model.ts:223-253,467-477,504,594,651,681` 捕获 delta/message 的 `reasoning_opaque`，放进 `providerMetadata.copilot.reasoningOpaque`；`convert-...-messages.ts:76-121` 下一轮回放为 assistant 消息的 `reasoning_opaque` + `reasoning_text`。
- ACTUAL：Rust 全仓 `reasoning_opaque|reasoningOpaque` **零命中**（grep 证实）；`openai_chat.rs:155/223/697` 只解析 `reasoning_text`。
- 影响：copilot chat 模型的 thinking 状态不跨轮携带——多轮工具循环中模型每轮丢失 opaque reasoning，推理链断裂/质量回退（Copilot 专属机制，README 明列为 intentional adaptation）。

#### C3【S2 · 半 KNOWN（`04:B-5`）/ 半 NEW】`X-Interaction-Type`/`X-Interaction-Id` 不发送；`interaction_type()` 是死代码；title/compaction 误报 `x-initiator:user`
- EXPECTED：`github-copilot.ts:266-280`（model.request hook）：`X-Interaction-Type`=conversation-agent/subagent/background/compaction；`X-Interaction-Id`=`SessionAffinity.get(session)`（`session/affinity.ts:7`，parentID??fork.sessionID??id）；`interaction !== "conversation-agent"` 时 `x-initiator=agent`。
- ACTUAL：`oc-ai/copilot.rs:39-57` 的 `SessionRequestKind`/`interaction_type()` 无任何生产调用方（仅 `copilot/tests.rs`）；route defaults 只带静态 `x-initiator:user`（`copilot.rs:33`），per-request 升级只看「最后消息角色」（`copilot.rs:71-86`）。title/compaction 请求（末消息是 user）在 Rust 发 `x-initiator:user`，ref 发 `agent`。`X-Interaction-Id` 无任何对应。
- 对照：`04:B-5` 曾登记「缺每请求 x-initiator 升级 / Vision-Request / X-Interaction-Type（KNOWN，be3b Ph.3 deferred）」——其中 x-initiator 升级与 `Copilot-Vision-Request` **现已落地**（`copilot.rs:92-102`，B-5 该半已过时）；残余缺口 = Interaction-Type/Id + kind 驱动的 initiator。
- 影响：Copilot 集成指南要求的交互分类头缺失（计费/限流分桶/遥测语义）；无硬失败证据，定 S2。

#### C4【S2 · NEW】`/v1/messages`（Claude-on-Copilot）lane 缺：包重写 + `anthropic-beta` 头
- EXPECTED：`models.ts:162-166`（`/v1/messages` 模型 → `@opencode/ai/providers/anthropic` + `baseURL=${baseURL}/v1`）+ `github-copilot.ts:304-316`（http.request hook 给 anthropic 协议请求补 copilot 头 + `anthropic-beta: interleaved-thinking-2025-05-14`，ref :495）。
- ACTUAL：无。该 lane 仅在远端模型同步成功时可达（P-06 下游），故 S2。

#### C5【S2 · NEW】utility title（免费 `gpt-4o-mini` 会话命名）缺失
- EXPECTED：`github-copilot.ts:284-303,400-462`：title agent 无显式模型且远端有 `gpt-4o-mini` 时，自带 `/chat/completions` 调用（`X-Interaction-Type: agent-session-name-generation`、`X-Interaction-Id`、`x-initiator:agent`、20s 超时、失败静默回退）。
- ACTUAL：无对应（Rust title 走常规计费模型路径）。
- 影响：Copilot 登录用户的会话标题消耗付费额度而非免费 utility 模型。

#### C6【S2 · NEW】copilot 命名空间 providerOptions 不消费 + User-Agent 头未设
- EXPECTED：`chat/openai-compatible-chat-options.ts:5-26` + `:105-116,175-186`：`providerOptions.copilot.{user, reasoningEffort, textVerbosity→verbosity, thinking_budget}`；`github-copilot.ts:487` 每请求 `User-Agent: App.useragent(app)`；chat 响应的 `accepted/rejected_prediction_tokens` 进 providerMetadata（:264-272,684-691）。
- ACTUAL：`openai_chat.rs:1782-1792` 只读 `store`/`reasoningEffort`（顶层扁平键）；`verbosity`/`user`/`thinking_budget`（copilot 语义）零处理（2878 行的 `thinking_budget` 是 alibaba overlay）；copilot 头集合无 User-Agent（`copilot.rs:29-35,92-102`）；prediction-token 元数据不产。
- 影响：边缘（`user` 滥用监控 id、verbosity 档位、Anthropic-on-copilot thinking 预算）。

#### 已对齐面（复核确认，不另立条目）
静态头策略（api-version/intent/initiator）、消息面 x-initiator 升级、vision 头、baseURL 解析（apiEndpoint > enterprise `copilot-api.{domain}` > 默认，`integration/copilot.rs:69-93`）、device OAuth/entitlement/`expires:0`、凭据绑定 refresh 双存（`integration/copilot.rs:128-147`）、GITHUB_TOKEN env 回退（`facade.rs:401-405`）、`reasoning_text` 解析（`openai_chat.rs:155,697`）、facade 线上断言（`facade.rs:829-857`）。

---

## 2. 第一轮 basename 未引用文件：逐个映射

| ref 文件 | rust 对应 | 状态 | 说明 |
|---|---|---|---|
| `protocols/assemblyai-transcription.ts` (209) | `media/transcription/assemblyai.rs` (418) | **WRONG（一处）** | 移植本体完整：upload-before-submit `prepare`（:94-101 ↔ rs:48-66）、queued 状态表、ms→s、usage seconds、`assemblyai.transcriptId/speechModel` 元数据。**但** rs:257-259 声明 `unsupported: ["prompt","speakers"]` 并在 `body_from` 前拒绝（rs:103-109,271）——ref **无**此声明，且 body 实际使用两字段（`prompt: request.prompt` :112、`speakers_expected: request.speakers` :118）。同目录 deepgram/google 才有该声明（`deepgram-transcription.ts:168`、`google-transcription.ts:186`）→ Rust 把别人的 unsupported 表错安到 assemblyai 头上。执行层缺失（M2）时无运行时影响。**S2 · NEW** |
| `protocols/openai-compatible-responses.ts` (23) | `openai_compatible.rs` `openai_compatible_responses_route` | OK | `store:false + include:["reasoning.encrypted_content"]` defaults、`.with` 合并语义逐字段一致（rs:104-148 测试锁定） |
| `protocols/utils/partial-json-options.ts` (64) | `protocols/utils/partial_json.rs:10-26` `allow` 模块 | OK | 位掩码逐一相同；NaN/Infinity 解析为错误是文档化良性分歧（rs:5-8） |
| `protocols/utils/responses-checkpoint.ts` (118) | `protocols/utils/responses_checkpoint.rs` (329) | OK（语义级复核通过） | 多槽复用/缺 encrypted/completion 后变更/恰好一个 checkpoint/responseID 漂移五不变式齐（rs:98-127,137-146 ↔ ref :26-46,67-92）；已接线：`openai_responses.rs:567-572` + `oc-core/session/runner/mod.rs:206` |
| `protocols/utils/responses-compaction.ts` (195) | `protocols/utils/responses_compaction.rs` (403) | OK（语义级复核通过） | `/compact` 端点拼接、strip effort updates、Body 字段表含 `prompt_cache_retention`/`prompt_cache_options`（rs:131-140）、单 compaction item 校验、replay media 三态（data-url/remote/base64）；已接线 `openai_responses.rs:562` |
| `protocols/utils/responses-hosted-tools.ts` (67) | `protocols/utils/responses_hosted_tools.rs` (168) | OK | onDone 双事件（providerExecuted tool-call+result）语义在，Rust 版为 open-responses 通用化的超集 |
| `providers/google-vertex-responses.ts` (93) | 无 | **MISSING** | OpenResponses over `https://aiplatform.googleapis.com/v1/projects/{project}/locations/{loc}/endpoints/openapi`（OAuth-only、显式拒绝 apiKey、location 默认 `global`、`store:false+include` defaults）。`facade.rs:169-224` family_of 无 `@opencode/ai/providers/google-vertex/responses` 键 → 用户 config 指定该包时 `UnsupportedProtocol`。当前 snapshot 无模型绑定该包（54 个 vertex 模型全部 `@ai-sdk/google-vertex`→gemini，双侧一致），故仅 config 可达。**S2 · NEW** |
| `providers/google-vertex-shared.ts` (88) | `oc-ai/src/protocols/vertex.rs:31-121` | OK | project 四别名/location 三别名/`host()`（global 根 + eu/us `rep.googleapis` + `{loc}-aiplatform`）ADC bearer/Express `x-goog-api-key` 互斥与链条齐。ADC 全链（service_account）缺 = 已登记 `03:V3` |
| `providers/openai-compatible-responses.ts` (60) | `openai_compatible.rs` + `facade.rs` OpenAICompatible family | OK（架构移位） | Settings/configure 门面形状差异属既有架构决定（facade vs per-provider factory），行为面（bearer optional + baseURL）等价 |
| `utils/media-type.ts` (70) | `media/utils.rs:233-330` | OK | 魔数嗅探全表（PNG/JPEG/GIF/RIFF-WEBP/WAVE/PDF/ftyp mp4/webm/ID3/AAC ADTS 11-bit sync/ogg/flac）、扩展名表 + 别名（audio/mp3、x-m4a、x-wav…）、`fileMediaType` = sniff ?? ext；测试 rs:879-894 |

**顺带实锤两个已登记项**（原标「未证实」）：
- `03:V1/V2`（vertex chat/messages URL 组合）：snapshot `google-vertex` **无 settings/baseURL/api 字段**（python 枚举证实）→ `facade.rs:455-460` chat 泛化臂直接 `ProviderError::Configuration "needs a baseURL"`（干净失败）；messages 臂 `anthropic_messages_route` 无 base 时**静默落 `https://api.anthropic.com/v1`**（`anthropic_messages.rs:57,2783` + `facade.rs:415-420` `with_base_url(route, None)` 不覆盖默认）→ vertex claude 模型请求打到 Anthropic 官方端点 + ADC bearer → 必 401。V2 建议从「未证实」改判 **WRONG S1 实锤**。

---

## 3. 22 个已移植媒体协议 vs ref：缺失 / 死代码 / 不可达

### 3.1 清单对账（ref 23 个媒体协议文件 → rust 22 个）

| 模态 | ref（23） | rust（22） | 缺失 |
|---|---|---|---|
| image (9) | openai/google/xai/zai/meta/bfl/replicate/fal/stability | `media/image/{同 9}` | 0 |
| video (4) | google/xai/fal/runway | `media/video/{同 4}` | 0 |
| speech (5) | openai/cartesia/deepgram/elevenlabs/**google** | `media/speech/{前 4}`（`speech/mod.rs:1-2` 自述 "the four"） | **google-speech**（KNOWN `03:M4` S1） |
| transcription (5) | openai/google/assemblyai/deepgram/elevenlabs | `media/transcription/{同 5}` | 0 |

### 3.2 运行时可达性判定

- **全部 22 个 = 死代码（结构性，双侧对等减一等）**：Rust 全仓 `oc_ai::media` 消费者**仅限 oc-ai 内部**（grep `oc_ai::media|media::protocol|media::model` 于 oc-core/oc-server/oc-cli/oc-tui 零命中）；执行层 `route/media.ts`(446)+`media-protocol.ts`(333)+4 个 `*-client.ts`+`ai-client.ts` 无对应（KNOWN `03:M2`，`media/mod.rs:20-22` 自述 open seam）。
- ref 侧对照：core/server 也无媒体 client 消费者（grep `ImageModel|TranscriptionModel|SpeechModel|VideoModel` 仅 `github-copilot/copilot-provider.ts` 的注释行命中）——v2 把它作为 packages/ai 的 SDK/插件交付面。因此该缺口是「交付面不完整」而非用户主链路断裂；但 23 组媒体录音在 Rust fixtures 已拷贝零回放（KNOWN `03:§1.7` S2 覆盖缺口）。
- **responses checkpoint/compaction 不是媒体**（任务表述澄清）：二者是 LLM 协议 utils，**已接线且可达**（§2 表）；openai-compatible responses 协议同样可达（mantle/vertex-经-config 等路径）。
- 死代码内部的移植错误：assemblyai `unsupported` 表 WRONG（§2，NEW）；其余 21 个本次抽验未发现同类错位。

### 3.3 逐项结论（重点后端）

| 后端 | 判定 |
|---|---|
| assemblyai-transcription | 协议在、死代码、**1 处 WRONG**（prompt/speakers 误拒） |
| google-speech | **MISSING**（S1 KNOWN M4；fixtures `google-speech` 2 条未用） |
| google-transcription / openai-transcription 等 | 在、死代码、抽验无误 |
| openai-compatible-responses（协议+provider） | 在且**可达**（mantle responses 路由 `facade.rs:369-377` 使用） |
| responses checkpoint/compaction/hosted-tools | 在且**可达**（`openai_responses.rs:562,567`、runner `mod.rs:206`） |

---

## 4. 9 个不可绑定 provider 的实际影响 + plugin/provider 四文件

### 4.1 `aisdk:` shim 不可绑定面（KNOWN `04:P-12`/§4.1，本报告量化）

对 snapshot 逐 provider 枚举（models-dev-snapshot.txt）：

| provider | npm | 模型数（text 输出） | 备注 |
|---|---|---:|---|
| merge-gateway | merge-gateway-ai-sdk-provider | 195 | 聚合器，最大单点 |
| aihubmix | @aihubmix/ai-sdk-provider | 137 (134 text) | 聚合器；同时在 ref prompt-cache-key 名单内 |
| sap-ai-core | @jerome-benoit/sap-ai-provider-v2 | 50 | 企业 |
| gitlab | gitlab-ai-provider | 30 | AI 网关 |
| qvac | @qvac/ai-sdk-provider | 9 | |
| watsonx | watsonx-ai-provider | 5 | |
| perplexity | @ai-sdk/perplexity | 4 | |
| v0 | @ai-sdk/vercel | 3 | |
| salad-cloud | @saladtechnologies-oss/ai-sdk-provider | 1 | |
| **合计** | | **434 / 8,390（5.2%）** | |

- 行为：选中即 `ProviderError::UnsupportedProtocol`（类型化 + 测试锁定 `facade.rs:697-732`）——失败干净、无静默错路。
- ref 侧这 9 个经 npm 运行时安装加载（`dynamic.ts` → `sdk-factory.ts` 的 `ai-v6` dist-tag 逻辑 + `factory.ts`）。Rust 无 JS 运行时（KNOWN/设计取舍）。**影响评估**：均为非默认、非 popular 档 provider（CLI/TUI 首页推荐序 `oc-tui/app.rs:3784` 不含它们）；主要受损人群是 merge-gateway/aihubmix 聚合器用户（合计 332 模型）。维持 S1→S2 边界的既有判定（`04` 归为设计取舍），无升级证据。

### 4.2 `plugin/provider/{dynamic,factory,sdk-factory}.ts`

| ref | rust | 状态 |
|---|---|---|
| `dynamic.ts` (19)：`aisdk.sdk` hook 兜底——未命中本地包时 npm 安装并加载 factory | 无 | MISSING（**KNOWN**，P-12 npm 运行时加载设计取舍；`aisdk.sdk` host 对象本身是 FINDINGS.md:84 永久分歧） |
| `factory.ts` (22)：`createProviderPlugin` 显式 load 函数注册 | 无 | MISSING（同上，P-12 家族） |
| `sdk-factory.ts` (24)：`npm.add` + `create*` 导出探测 + `@ai-sdk/*` → `@ai-v6` tag 规则 | 无 | MISSING（同上） |

### 4.3 `plugin/provider/prompt-cache-key.ts`【S2 · NEW】

- EXPECTED：ref `prompt-cache-key.ts:10-38` 对 12 个 catalog provider（abliteration-ai、aihubmix、anyapi、auriko、edenai、jalapeno、llama、morph、snowflake-cortex、tencent-tokenhub、upstage、venice）的**全部模型**设 `compatibility.supportsPromptCacheKey = true`——这些是无专属 AI 包的 openai-compatible 目录 provider，唯一让 `prompt_cache_key` 出现在 body 的途径（gate 在 `packages/ai/src/protocols/openai-chat.ts:792`）。
- ACTUAL：Rust catalog 的 compatibility **只从 `interleaved` 派生 reasoning field**（`catalog.rs:297,327-343`），`supports_prompt_cache_key` 对 catalog 模型恒为 None→false；无任何等价 overlay。gate 本体已移植（`openai_chat.rs:2557-2561` 与 ref :792 语义一致）；协议级默认只覆盖 openrouter/venice（`openai_chat.rs:3254-3270`，对应其各自协议文件）——venice 因此幸免，**其余 11 个 provider 的 prompt_cache_key 永不发送**。
- 影响：这些 provider 上的会话级前缀缓存命中率归零（成本/延迟回退，非正确性问题）。注意与 `01:M-04`（promptCacheKey 大写 hex 误判）同域不同根因。

---

## 5. 新增项计数

| 类别 | 数量 | 明细 |
|---|---:|---|
| **NEW（S1）** | 2 | C2 reasoning_opaque 续链缺；C1 的 `language` hook endpoint 选择缺（C1 主体为已登记 S2 的升级） |
| **NEW（S2）** | 6 | C3 残余半（X-Interaction-Id + kind-initiator）；C4 messages lane 头/包重写；C5 utility title；C6 copilot options/User-Agent；§2 assemblyai unsupported WRONG；§2 google-vertex-responses 包 MISSING；§4.3 prompt-cache-key overlay 缺 |
| **KNOWN 升级/实锤** | 3 | C1（03:§3.7 S2→S1，21/34 模型路由错误实证）；03:V2（vertex messages「未证实」→ WRONG S1 实锤：静默落 api.anthropic.com）；03:V1（vertex chat 缺 baseURL→干净 Configuration 错，实锤） |
| **KNOWN 复核过时** | 1 | 04:B-5 的「x-initiator 升级 / Vision-Request 缺」半已落地（`copilot.rs:92-102`），残余为 Interaction-Type/Id |
| **KNOWN 量化/确认** | 4 | P-06（models.ts 全文映射）；P-12（9 provider = 434/8390 模型，clean typed error）；M2（22 协议死代码 grep 再证）；M4（google-speech，speech/mod.rs 自述） |
| **OK 新证** | 8 | responses-checkpoint/compaction 语义级+接线确认；partial-json-options；openai-compatible-responses（协议）；google-vertex-shared；media-type；openai-compatible-responses provider 门面；hosted-tools |

**合计：8 个 NEW 差异条目（2×S1 + 6×S2），3 个已登记项升级/实锤，1 个已登记项过时更正。**

---

*本报告为只读审计产物，未修改任何产品代码；仅写入本文件。*
