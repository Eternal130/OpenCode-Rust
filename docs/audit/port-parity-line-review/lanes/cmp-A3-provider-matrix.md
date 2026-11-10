# A3 结论: 46 个 ref provider 中 31 个✅完整、13 个🟡部分、2 个❌缺失 — chat 协议面绑定密度很高（含 dialect/compat/baseURL/env 链/baked 默认的全套对齐），主要缺口集中在①媒体 provider 的部署接线（协议已移植但 oc-core/oc-server 无消费者）、②SystemOne evaluation 域（typesafe-ai/opencode-zen/openrouter/vercel 的 evaluation 车道）、③cloudflare-ai-gateway 网关语义与 alibaba region 派生。

## 支持矩阵（46+ 行全表）

ref 侧证据 = `/opt/opencode-ref/packages/ai/src/providers/*.ts`；rust 侧 = `oc-core/src/provider_catalog/{native,facade,catalog}.rs` + `oc-ai/src/protocols|media/`。

| provider | ref apis/特判 | rust 承载 | 状态 |
|---|---|---|---|
| alibaba | chat/messages/responses 三路由；region→dashscope host 表 + workspaceID maas 合成（alibaba.ts:47-52,77-100）；DASHSCOPE/ALIBABA 双 env；web_search/web_extractor/code_interpreter hosted tools | native.rs:93-98；facade.rs:1103 chat factory、:1039 AlibabaResponses（G-84 含 hosted tools）、:1345-1364 messages 的 compatible-mode→apps/anthropic 重写 + region 缺失报错 | 🟡 region/workspaceID host 派生缺失——catalog 固定 intl base，cn-beijing 等区域选择不可用 |
| amazon-bedrock | bedrock-converse + SigV4/bearer/默认凭证链 + region 链 + bearer/sigv4 互斥校验 | facade.rs:702-719 + G-262② region/profile/endpoint；bedrock/{sigv4,event_stream,credentials} | ✅ |
| amazon-bedrock-mantle | open-responses/openai-chat/anthropic-messages on mantle；Opus5.0 拒 mid-conversation output_config（≥5.1 才 supportsEffortUpdates，mantle.ts:71-78）；baseURL 默认 bedrock-mantle.{region}.api.aws | native.rs:71-79 mantle 按 gpt-oss 分 chat/responses；facade.rs:721-802 三车道 + messages claude 版本门（G-319） | ✅ |
| anthropic | anthropic-messages；x-api-key + ANTHROPIC_API_KEY；apiKey/authToken 互斥 | facade.rs:954-958 anthropic_route（x-api-key 链） | ✅ |
| anthropic-compatible | 自定义 provider 走 anthropic-messages，baseURL 必填，authToken bearer | facade.rs:273-298 CustomProvider「always bound through the openai-compatible facade」，package 字段 "Decoded but UNCONSUMED" | 🟡 自定义 anthropic-messages host 被降级为 openai-compatible chat；authToken bearer 车道缺失 |
| assemblyai | transcription；authorization 头 + ASSEMBLYAI_API_KEY | oc-ai media/transcription/assemblyai.rs（协议已移植） | 🟡 协议+media route 执行器在，但无 catalog/部署绑定（D-1） |
| azure | openai-responses(WS channel 55min 轮换+azure 域名门)+openai-chat；resourceName→URL、api-version=v1、deployment URL、api-key 头、useCompletionUrls 切 chat | facade.rs:989,1069→azure_catalog_route(:1211-1248)；native.rs azure shape/settings 分裂；azure_sync.rs | ✅ |
| baseten | openai-chat；baked inference.baseten.co/v1；BASETEN_API_KEY | native.rs:99-101 HOSTS；facade OpenAIChat generic + catalog api | ✅ |
| black-forest-labs | bfl-images；x-key 头 + BFL_API_KEY | media/image/bfl.rs | 🟡 同 D-1 未接线 |
| cartesia | speech；bearer CARTESIA_API_KEY | media/speech/cartesia.rs | 🟡 同 D-1 |
| cerebras | openai-chat；baked api.cerebras.ai/v1；compat(max_tokens/reasoning/supportsStore:false/promptCacheKey) | facade.rs:1504 baked(G-120)、:1557 compat；cerebras_chat_compatibility | ✅ |
| cloudflare-ai-gateway | 三协议 + accountId/gatewayId→gateway.ai.cloudflare.com URL 合成、native passthrough(cf-aig-authorization)、cf-aig-{metadata,cache-ttl,cache-key,skip-cache,collect-log,gateway-id} 头、模型前缀分派（cloudflare-ai-gateway.ts:105-160） | native.rs:102-107 Many→chat 入口；facade.rs:430 归 OpenAIChat generic；plugin_headers.rs 无 cf-aig | 🟡 仅当 plain openai-chat（需显式 baseURL）；网关 URL 合成/头/前缀分派/passthrough 鉴权全缺 |
| cloudflare-workers-ai | openai-chat；accountId→URL 合成或 baseURL；3 env 候选；models-dev 的 api 不注入 settings（models-dev.ts:106 排除） | catalog.rs:303-304 复刻排除；facade generic chat 需用户 baseURL | ✅（行为等价） |
| cohere | cohere-chat 原生 v2 + /chat/completions compatibility 线（COMPATIBILITY_BASE_URL）+ compat 预设 | facade.rs:445-447,1161-1172 双车道；cohere_chat.rs | ✅ |
| deepgram | speech+transcription；Token scheme + DEEPGRAM_API_KEY | media/speech+transcription/deepgram.rs | 🟡 同 D-1 |
| deepinfra | openai-chat；baked api.deepinfra.com/v1/openai；baseURL 归一化(去尾斜杠+补 /openai)；compat | facade.rs:1088-1092 归一化(G-119)、:1502 baked、:1555 compat | ✅ |
| deepseek | openai-chat；baked api.deepseek.com/v1；compat(max_tokens/store:false) | deepseek_chat_route（facade.rs:1095）+ baked + compat | ✅ |
| digitalocean | 自定义 protocol（openai-chat+cache_control 断点下沉）；3 env 链；compat(promptCacheKey) | facade.rs:1117-1119(G-118)；:1526 env 链(G-123)；digitalocean_sync.rs 动态 | ✅ |
| elevenlabs | speech+transcription；xi-api-key 头 | media/speech+transcription/elevenlabs.rs | 🟡 同 D-1 |
| fal | fal-images+video；Key scheme + FAL_KEY；共享队列轮询 | media/image/fal.rs + video/fal.rs + fal_queue.rs | 🟡 同 D-1 |
| fireworks | openai-chat；baked api.fireworks.ai/inference/v1；compat(promptCacheKey) | builtin providers/fireworks（native.rs:165-166）；catalog id=fireworks-ai；facade.rs:1558 compat | ✅ |
| google | gemini + interactions；x-goog-api-key + GOOGLE_GENERATIVE_AI_API_KEY；image/video/speech/transcription 媒体四线 | facade.rs:453 Gemini、:457 GoogleInteractions(AI-M3)；媒体在 media/{image,video,speech,transcription}/google.rs（未接线） | ✅（媒体子能力同 D-1） |
| google-vertex | gemini on aiplatform；express apiKey vs ADC；tuned endpoints 拒 apiKey；project/location env 链 + host 变体 | facade.rs:461-463 VertexGemini→vertex_route_for_model(AI-VERTEX-V2)；vertex.rs | ✅ |
| google-vertex-chat | openai-chat on endpoints/openapi；拒 apiKey | facade.rs:467 VertexChat(:845-866) | ✅ |
| google-vertex-messages | anthropic-messages on publishers/anthropic；anthropic_version=vertex-2023-10-16、去 model 字段 | facade.rs:382 VertexMessages；vertex_messages.rs | ✅ |
| google-vertex-responses | open-responses on endpoints/openapi；store:false+encrypted_content 默认 | facade.rs:468 VertexResponses(:867-886) | ✅ |
| google-vertex-shared | project/location/host 环境链 + ADC + requireProject | vertex.rs（integration/credential.rs ADC 绑定） | ✅（并入上四行） |
| groq | groq-chat 方言（reasoning_format=parsed、gpt-oss include_reasoning、service_tier/parallel_tool_calls/user）；baked api.groq.com/openai/v1；compat | groq_chat_route（facade.rs:1096）+ baked(:1477) + groq_chat_compatibility | ✅ |
| meta | meta-responses + chat + messages(thinking adaptive/omitted 默认)；web_search/image_generation hosted tools；META_API_KEY | facade.rs:401 MetaResponses(G-84)、:973-981 messages adaptive 默认(G-172)；meta_responses.rs | ✅（meta-images 未接线 D-1） |
| minimax | chat 方言(thinking 透传+reasoning_split=true)+messages(anthropic 根)+responses；MINIMAX_API_KEY | native.rs:130-132；facade.rs:1107(G-172)、messages 走 catalog api、:1568 compat | ✅ |
| mistral | mistral-chat 原生；MISTRAL_API_KEY | facade.rs:451,1173-1182；mistral_chat.rs（tool-id 9 位整形） | ✅ |
| moonshot | chat 方言(thinking 透传)+messages+responses；sanitizer:moonshot；双 env；api.moonshot.ai 根 | native.rs:128；facade.rs:1110、:1336-1341 messages 根重写(v1→anthropic/v1，含 .cn)、responses 走 OpenAIResponses | ✅ |
| openai | openai-responses+openai-chat；withOpenAIOptions(gpt-5 默认 effort/summary/encrypted_content、store:false)；org/project 头；image/speech/transcription | facade.rs:384-390、:602-621 G-171 gpt5 默认；媒体 media/*/openai.rs 未接线 | ✅（媒体同 D-1） |
| openai-compatible | 自定义 provider openai-compatible-chat；provider 名覆写 | facade.rs:469,1183-1192（CustomProvider 专用 seam） | ✅ |
| openai-compatible-responses | 自定义 provider open-responses 车道 | family_of 无此入口；CustomProvider 只绑 chat；openai_compatible_responses_route 仅用于 bedrock mantle | ❌ 自定义 provider 的 responses 协议车道缺失 |
| openrouter | openrouter-chat 方言(usage include、assistant reasoning 合并、reasoning.fitThinkingBudget、plugins/provider routing)；experimental.evaluation(SystemOne) | openrouter_chat_route + compatibility（facade.rs:1097,1562）；SystemOne 无对应 | ✅（evaluation 车道 ❌，D-2） |
| opencode-zen | SystemOne evaluation；OPENCODE_API_KEY；zen/v1；keyless 门（core plugin/provider/opencode.ts:317-342） | catalog.rs:177-204 keyless 门逐条复刻（含 cost>0 禁用）；chat 绑定经 openai-compatible@zen/v1；SystemOne/evaluation 无对应 | 🟡 keyless/chat ✅；evaluation 域缺失（D-2） |
| replicate | replicate-images；bearer REPLICATE_API_TOKEN；Prefer: wait 头 | media/image/replicate.rs | 🟡 同 D-1 |
| runway | runway-video；bearer RUNWAYML_API_SECRET | media/video/runway.rs | 🟡 同 D-1 |
| stability | stability-images + upscale；bearer STABILITY_API_KEY | media/image/stability.rs | 🟡 同 D-1 |
| togetherai | openai-chat；baked api.together.xyz/v1；双 env；compat | facade.rs:1503 baked、:1529 env 链(G-123)、:1556 compat | ✅ |
| typesafe-ai | SystemOne 结构化测试 provider（TYPESAFE_API_KEY@api.typesafe.ai/v1） | 无对应（oc-ai testing.rs 是 TestLLM 执行器，非本 provider） | ❌ |
| venice | venice-chat 原生 + openai-chat framing；VENICE_API_KEY；queryParams | venice_chat_route（facade.rs:1098）+ venice_chat_compatibility | ✅ |
| vercel-ai-gateway | gateway-protocol 包裹的三路由+per-model 分派+x-session-affinity；cacheTTL/cacheAnchorItems/thinking 适配；/evaluate 评测端点；AI_GATEWAY_API_KEY+VERCEL_OIDC_TOKEN | facade.rs:436-438→vercel_gateway.rs route_for_model(:167)+AUTH_ENV 链(:1019-1026)；/evaluate 无对应 | ✅（evaluation ❌ D-2；x-session-affinity 由 session 级头补：model_request.rs:776） |
| xai | xai-responses(WS channel 24h、continuation)+chat(x-grok-conv-id promptCacheKey 头)；image/video | facade.rs:407 XaiResponses、chat 走 generic；xai_responses.rs/open_responses_channel.rs | ✅（chat 的 x-grok-conv-id 头缺失 D-5；xai-images/video 未接线 D-1） |
| zai | zai-chat 原生 + zai-images；ZAI_API_KEY；api.z.ai/api/paas/v4 | facade.rs:1099-1101 zai_chat_route + compat；zai tool-stream 特判 openai_chat.rs:1889-1900 | ✅（zai-images 未接线 D-1） |
| zai-coding-plan | zai chat/messages/responses 三路由；计划模式 variant（variant.ts:275 coding→latest→effort low/high/max） | native.rs:136；facade messages 根重写(:1342-1344)；variant.rs:497-505 + :1098-1129 协议映射 | ✅ |
| provider.ts 工厂 | Definition.make + builtins 表（provider.ts:48-110） | native.rs BUILTIN_ENTRYPOINTS(:145-198，与 ref 逐条一致并有快照测试 :476-494）；resolve_specifier 移植 | ✅ |

➕ rust 独有/超集：github-copilot 双车道（G-167 + copilot_sync；ref 在 core/github-copilot 而非 providers/）、snowflake-cortex 400-"conversation complete" 重写 + 401 刷新重试（facade.rs:571-601,1283-1300）、zenmux/kilo/nvidia/llmgateway/cerebras 头插件、modal/digitalocean/azure 动态同步——均为 ref core 插件层的对应移植，非凭空新增。

## 统计

- **✅ 完整 31 / 🟡 部分 13 / ❌ 缺失 2**（共 46 行）
- 🟡 构成：媒体未接线 9（assemblyai、black-forest-labs、cartesia、deepgram、elevenlabs、fal、replicate、runway、stability）+ 网关/区域语义 3（cloudflare-ai-gateway、alibaba、anthropic-compatible）+ evaluation 域 1（opencode-zen）
- ❌：openai-compatible-responses（自定义 responses 车道）、typesafe-ai（SystemOne 测试 provider）
- 另有 4 个 ✅ 行带 evaluation/media 缺口注记（openrouter、vercel-ai-gateway、xai、google/meta/openai/zai 的媒体子能力）

## 行为差异清单

- **D-1 媒体 provider 无部署接线（影响 13+ 家）**：协议层与 route 执行器已完整移植（与 ref 协议文件一一对应），但 oc-core/oc-server 对 `oc_ai::media` 零引用（grep 证实），facade.rs 亦无媒体家族。model.rs:6-8 自认"deployment lives with a future media-route lane"。
- **D-2 SystemOne/evaluation 域缺失**：ref 的 experimental/system-one + evaluation-model（typesafe-ai.ts、opencode-zen.ts、openrouter.ts evaluation、vercel-ai-gateway /evaluate）在 rust 全缺（grep 无命中）。zen 的 keyless 门与 chat 绑定不受影响。
- **D-3 cloudflare-ai-gateway 语义降级**：ref 的 URL 合成、native passthrough（cf-aig-authorization）、5 个 cf-aig-* 头、模型前缀分派全部无对应；rust 只把它当需要显式 baseURL 的 plain openai-chat。
- **D-4 alibaba region/workspaceID 派生缺失**：ref alibaba.ts:47-52 的 4 区域 host 表 + `{workspaceID}.{region}.maas` 合成无对应；rust 固定 dashscope-intl 根，cn 用户需手写 baseURL。messages 的 compatible-mode→apps/anthropic 根重写已对齐。
- **D-5 小项**：①xai chat 的 `x-grok-conv-id`（promptCacheKey→头）无对应；②自定义 provider 的 `package` 字段解而不用于协议选择（facade.rs:291-297）。
- **D-6 对齐良好的易错点（正面对照）**：mantle 按 gpt-oss 分裂、azure useCompletionUrls/shape 双臂、cloudflare-workers-ai 的 api 不入 settings、deepinfra /openai 归一化、togetherai/digitalocean env 链（G-123）、gpt-5 默认 options（G-171）、meta messages adaptive 默认（G-172）——均有注释级 ref 行号锚定。

## 正确性风险

- **R-1（中，D-1）**：依赖媒体生成/语音/转写的上层功能在 rust 静默不可用——协议代码存在但不可达，容易被误判为"已支持"。
- **R-2（中，D-2）**：zen keyless 模式 chat 可用（逻辑对齐），但 evaluation 端点类功能无实现路径。
- **R-3（中，D-3）**：cloudflare-ai-gateway 用户按 ref 习惯只配 accountId/gatewayId 时，rust 报 "needs a baseURL"——错误信息未提示网关语义未支持。
- **R-4（低，D-4）**：alibaba cn 区域用户流量打到 intl 端点（而非报错），静默行为偏移——比报错更危险。
- **R-5（低）**：zai-coding-plan 的 zhipuai-coding-plan（bigmodel.cn 根）经 host_table 覆盖，但 messages 根重写表只列 zai 域（facade.rs:1342-1344 未含 open.bigmodel.cn 的 anthropic 根）——bigmodel coding-plan 的 messages 车道可能拿错根，需测试锁定。
- **R-6（低，D-5）**：xai chat prompt cache 降级，conv-id 亲和丢失，缓存命中率行为差异。

## 覆盖声明

- 已读：ref providers/ 全部 46 个文件正文（含 google-vertex*、mantle、vercel-ai-gateway 全文、openai-options.ts、provider.ts builtins 表、models-dev.ts normalize 段、variant.ts zaiChat 段、plugin/provider/opencode.ts keyless 段）；rust native.rs 全文、facade.rs 1-1600（build_route 全臂 + 三表）、catalog.rs available_models、variant.rs zai 段、openai_chat.rs detect_* 段、media 目录结构 + model.rs/route.rs 头部、models-dev-snapshot 226 provider 的 npm/api/env 抽样核对。
- 未逐行：各协议本体（A1a/A1b 已审，只看 provider 入口/特判段）；overlay.rs/refresh.rs/support.rs 全文（A2 已覆盖）；snapshot 中其余 ~180 个 catalog-only provider（走 openai-compatible seam，未逐一验证 api/env 字段）。
- "openai-compatible-error" 特判：ref packages/ai 中不存在该名；唯一近似物在 packages/core/src/github-copilot/chat/（v1 遗留），rust copilot 车道承载，未发现缺失证据。
