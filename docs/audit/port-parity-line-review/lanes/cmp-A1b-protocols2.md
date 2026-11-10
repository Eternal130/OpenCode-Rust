# A1b 结论: 本 lane 协议族 Rust 移植整体高度忠实（sigv4 有 AWS 官方向量钉死、事件流帧/CRC 自实现正确、copilot oauth.refresh 与 ref 一致、venice/zai 以 overlay 并入 openai_chat 承载），仅存在诊断 body 丢失、providerMetadataKey 硬编码、少量错误文案措辞等低危差异，无阻断性缺失。

## 协议覆盖矩阵

| ref 协议文件 | rust 对应 | 状态 |
|---|---|---|
| bedrock-converse.ts | protocols/bedrock/converse.rs（含内联 bedrock-cache/mistral-tool-id 移植） | ✅ 对齐 |
| bedrock-event-stream.ts | protocols/bedrock/event_stream.rs + route/framing.rs `AwsEventStream` | ✅ 对齐（诊断 body 降级，D-2） |
| utils/bedrock-auth.ts | protocols/bedrock/sigv4.rs + credentials.rs（默认链 env/profile/SSO/IMDS） | ✅ 对齐（链为子集，D-9） |
| （mantle: providers/amazon-bedrock-mantle.ts） | protocols/bedrock/mantle_messages.rs | ✅ 对齐 |
| （rust 独有 invoke 路由） | bedrock/converse.rs `bedrock_invoke_route` (converse.rs:1188) | ⚠️ ref 无对应（见 D-8） |
| mistral-chat.ts | protocols/mistral_chat.rs | ✅ 对齐 |
| cohere-chat.ts | protocols/cohere_chat.rs | ✅ 对齐 |
| venice-chat.ts | openai_chat.rs `VeniceChatProtocol`+`venice_{assistant,body}_overlay`（1344-1650, 3110-3234） | ✅ 并入承载，非缺失 |
| zai-chat.ts | openai_chat.rs `zai_body_overlay`/`zai_chat_route`（3006-3035, 3356） | ✅ 并入承载 |
| zai-messages.ts | customization.rs `zai_wrap`(196) + anthropic_messages.rs `zai_coding_plan_messages_compatibility`(1150) | ✅ 并入承载 |
| alibaba-chat.ts | openai_chat.rs `alibaba_body_overlay`/`alibaba_chat_route`（3039-3086, 3379） | ✅ 并入承载 |
| alibaba-messages.ts | customization.rs（alibaba wrap，同 zai 机制） | ✅ 并入承载 |
| alibaba-responses.ts | protocols/alibaba_responses.rs | ✅ 对齐 |
| meta-messages.ts | customization.rs meta web_search remap + anthropic_messages.rs（tests 2553 注） | ✅ 并入承载 |
| meta-responses.ts | protocols/meta_responses.rs | ✅ 对齐（含 completedItems 终端恢复） |
| xai-responses.ts | protocols/xai_responses.rs | ✅ 对齐 |
| open-responses.ts（本 lane 侧重 hosted tools） | protocols/open_responses.rs + utils/responses_hosted_tools.rs（经 alibaba/meta/xai 消费验证） | ✅ 对齐 |
| open-responses-channel.ts | protocols/open_responses_channel.rs | ✅ 对齐 |
| open-responses-continuation.ts | protocols/open_responses_continuation.rs | ✅ 对齐 |
| utils/gateway-protocol.ts + providers/vercel-ai-gateway.ts | utils/gateway_protocol.rs + protocols/vercel_gateway.rs | ✅ 对齐（ref 实际消费者是 vercel-ai-gateway.ts，非 cloudflare-ai-gateway.ts） |
| copilot（ref 在 packages/core/src/plugin/provider/github-copilot.ts + github-copilot/，非 packages/ai） | protocols/copilot.rs + copilot_responses.rs + oc-core/integration/copilot.rs | ✅ 对齐（C6b R-4 已核，见下） |
| utils/bedrock-cache.ts / bedrock-media.ts | converse.rs 内联（44-222） | ✅ 对齐 |

## 对比表（关键逐点）

| 关注点 | ref | rust | 结论 |
|---|---|---|---|
| SigV4 canonical request | aws4fetch（bedrock-auth.ts:57-84） | sigv4.rs:132-186 自实现，tests:441 钉 AWS 文档向量（GET+POST 双签名）、RFC4231 HMAC、query 排序编码 | ✅ 等价且更强 |
| SigV4 签名头集合 | aws4fetch 签所给头+host/date | sigv4.rs:140-153 签全部输入头（排除 user-agent/authorization）+host/x-amz-date/security-token | ✅ 实际输入头仅 content-type，等价 |
| bearer/sigv4 优先级 | bearer(apiKey??AWS_BEARER_TOKEN_BEDROCK) > 静态 > 默认链（bedrock-auth.ts:139-144） | sigv4.rs:193-262 同序 | ✅（空串差异见 D-6） |
| region 解析 | region→credentials.region→env→us-east-1 | sigv4.rs:86-98 | ✅ |
| 事件流帧解码 | @smithy/eventstream-codec（CRC 校验） | event_stream.rs:67-162 手写（prelude CRC + message CRC、头类型 7/2-5、`:message-type`/`:exception-type` 重包装、`p` padding 剔除） | ✅ 结构等价 |
| 不完整流 | InvalidProviderOutput，classification="incomplete-stream"，带 base64 body（bedrock-event-stream.ts:42-53） | framing.rs:204-216 同 classification，无 body | ✅ 分类对齐（body 缺失 D-2） |
| converse 断点预算 | tools→system→messages 顺序，cap 4，短 TTL 模型表 | converse.rs:44-96 逐模型 ID 一致 | ✅ |
| converse Nova2/maxTokens、thinking binding、anthropic_beta | bedrock-converse.ts:460-492,556-564 | converse.rs:656-701, 774-791 | ✅ |
| mistral tool id | FNV-1a×2 + base36(7+7) 取尾 9 | mistral_chat.rs:35-113 / converse.rs:228-309 | ✅ 同算法（非 ASCII 微差 D-10） |
| mistral pending/generated 工具键规则 | mistral-chat.ts:599-654（index→id:→position:→latest/missing + pending 累积 + generatedTools 计数） | mistral_chat.rs:939-1033 | ✅ 逐分支对齐 |
| cohere 消息映射/finish 映射/usage | cohere-chat.ts:123-324 | cohere_chat.rs:143-735 | ✅（`fitThinkingBudget` 默认 min=1 一致） |
| venice MARKER 流过滤 + 签名重放 + cache_write 回填 | venice-chat.ts:249-348 | openai_chat.rs:1435-1639 | ✅（含 pending 前缀挂起、`reasoning_details:[]` 注入、onHalt 补 delta） |
| zai tool_stream 判定 | glm-(\d+)(\.\d+)? 边界正则（zai-chat.ts:45-48） | openai_chat.rs:2968-3004 手写边界解析 | ✅ 等价 |
| alibaba allowed_tools 包装 | alibaba-responses.ts:58-62 | alibaba_responses.rs:149-157 | ✅ |
| meta 签名图柄重放/注解/终端恢复 | meta-responses.ts:61-199 | meta_responses.rs:150-345 | ✅ |
| xai contextManagement 拒绝 + hosted 表 + 24h 轮换 + store:false 续传剪 instructions | xai-responses.ts:40-65 / providers/xai.ts:43 | xai_responses.rs:43-56, 145-160 | ✅ |
| gateway 元数据合并→finish | gateway-protocol.ts:71-88 | gateway_protocol.rs:246-281 | ✅（merge_json_records 同语义） |
| gateway prepare（messages effort/thinking、responses cache_ttl、chat reasoning） | vercel-ai-gateway.ts:142-183 | gateway_protocol.rs:105-165 | ✅ |
| gateway 三路由 defaults/`x-session-affinity`/模型族分派 | vercel-ai-gateway.ts:200-227,272-277 | vercel_gateway.rs:46-175 | ✅（requireSignature/reasoningField compat 在 oc-core facade.rs:1607 补齐） |
| channel create 消息（剥 stream/stream_options/background + type 标记） | open-responses-channel.ts:42-49 | open_responses_channel.rs:115-127 | ✅ |
| continuation 增量判定（invariant/canonical/comparable、拒绝恢复码、checkpoint 重加密合并） | open-responses-continuation.ts:32-206 | open_responses_continuation.rs:53-429 | ✅ 逐函数对齐 |
| copilot bearer | `credential.refresh`（core github-copilot.ts:187,218 `Authorization: Bearer ${credential.refresh}`） | oc-core integration/copilot.rs:224 `api_key: oauth.refresh.clone()` | ✅ **C6b-R4 遗留已核实：一致**（ref 将 GitHub token 同时存于 access+refresh，发送取 refresh 字段；rust 注释亦如实） |
| copilot 头策略 | applyHeaders（useragent、Openai-Intent、api-version 2026-08-01、x-initiator 升级、Vision、Interaction 三级 + agent-session-name-generation 免费标题） | copilot.rs:38-196, 883-973 | ✅（kind 推导机制不同：ref 由 session hook 事件 kind，rust 由 compaction part/parent 头推导——Title 可记账路径归为 chat，注释已声明） |
| copilot lane 分派 | aisdk hook 端点判定 gpt≥5(除 gpt-5-mini)/grok/gemini/mai-code→responses（github-copilot.ts:317-344） | copilot_responses.rs:76-107 动态端点+dispatch protocol | ✅（`endpoint` 选项优先一致） |
| 重试分类 | stream exception/error → classifyProviderFailure（bedrock-converse.ts:799-809、mistral-chat.ts:666-675） | converse.rs:1036-1061、mistral_chat.rs:1069-1087 走同 classify | ✅ |

## 行为差异清单 D-n

- **D-1（低）** converse 请求侧 reasoning 元数据键硬编码 `"bedrock"`（converse.rs:529-530）及 `initial` 回退（converse.rs:1128-1134）；ref 取 `route.providerMetadataKey ?? String(provider)`（bedrock-converse.ts:333,392,861）。默认路由相同；仅自定义 metadata key 的路由会取不到 signature/redactedData。
- **D-2（低）** ref 事件流帧携带 `rawBody` 并在 decode/payload 失败/incomplete-stream 上附 base64/JSON body（bedrock-event-stream.ts:49,63-74,102-113）；rust 帧 JSON 化后错误不带 body（event_stream.rs:68-70,197-201、framing.rs:209-215）。纯诊断信息降级。
- **D-3（低）** `messageStop` malformed_* 错误：ref 附 encodeJson(event) body；rust `AiError::event_error` 无 body（converse.rs:1014-1018）。
- **D-4（微）** 不支持内容错误文案：ref `"Bedrock Converse user messages only support text and media content for now"`；rust `"Bedrock Converse only supports text and media user content"`（converse.rs:513）。cohere/mistral 侧文案逐字对齐。
- **D-5（微）** mistral `hasLateContent`：ref `Boolean(tool.id)` 空串 falsy；rust `is_some_and(Value::is_string)` 空串计为内容（mistral_chat.rs:1045）。仅 `id:""` 病态帧触发。
- **D-6（微）** bedrock bearer：ref 空串仍发 `Bearer `；rust 过滤空 api_key/env（sigv4.rs:203-204）。rust 更严格，安全侧偏离。
- **D-7（微）** base64 解码错误文案：rust 追加 `": {error}"`；ref 无后缀。
- **D-8（信息）** `bedrock_invoke_route`（converse.rs:1188，anthropic-messages over `/model/{id}/invoke-stream`）在 ref v2.0.24 无对应——Rust 独有扩展，需产品确认是否有意保留。
- **D-9（信息）** rust AWS 默认凭证链 env/profile/SSO/IMDS，不实现 credential_process/assume-role/web-identity/ECS 并"响亮失败"（credentials.rs:25-30）；ref 经 `fromNodeProviderChain` 全量支持。企业 assume-role 场景 rust 覆盖窄于 ref。
- **D-10（微）** Mistral id 哈希对非 ASCII 输入：converse.rs:237 `char as u32 & 0xFF` 截断、mistral_chat.rs:58 不截断（同 `charCode`）。converse 侧与 ref 在非 ASCII tool id 时产生不同（各自内部一致的 9 字符 id）。

## 正确性风险 R-n

- **R-1（低）** D-1 的 metadata key 硬编码：若未来 converse 路由被复用为非 `bedrock` key 的部署，reasoning 签名回放静默丢失 → 模型拒答 unsigned reasoning 退化为 text（有兜底，不致错，语义弱化）。
- **R-2（低）** D-2/D-3 诊断 body 缺失：现场排障（AWS CRC 坏帧、ThrottlingException 原文）信息少于 ref，影响可调试性不影响正确性。
- **R-3（低）** copilot `InteractionContext` 的 kind 推导（copilot.rs:131-157）依赖 oc-core session seam 注入的头存在；直接 route 使用（无 session）时交互头省略——与 ref hook 不在 session 外运行一致；title 记账路径在 oc-ai 层不可分辨（注释已声明）。
- 未发现签名算法、帧解码、重试分类、usage 映射、增量续传判定层面的功能性错误。

## 覆盖声明

- 已逐行读完：ref 侧 bedrock-converse/event-stream/mistral/cohere/venice/zai-chat/zai-messages/alibaba-{chat,messages,responses}/meta-{messages,responses}/xai-responses/open-responses-{channel,continuation} + utils/{bedrock-auth,gateway-protocol} + providers/{vercel-ai-gateway,cloudflare-ai-gateway}；rust 侧 bedrock/ 全部（converse/event_stream/sigv4/mantle_messages/credentials 首部）、mistral_chat、cohere_chat、openai_chat 的 venice/zai/alibaba overlay 与流包装段、alibaba_responses、meta_responses、xai_responses、vercel_gateway、utils/gateway_protocol、open_responses_channel、open_responses_continuation、copilot(+responses)、oc-core integration/copilot.rs 关键段；shared.rs wrap_system_update 与 ref shared.ts 抽查对齐。
- 未逐行：open-responses.ts 主体（归 A1a 基线 lane，本 lane 仅经 hosted-tools 消费面验证）、utils/responses-hosted-tools.ts 与 rust 对应文件未做逐行双读（经 alibaba/meta/xai 三消费端测试交叉验证）、bedrock/credentials.rs 200 行以后的 SSO/IMDS 细节、ref packages/core github-copilot/{chat,responses} 转换器全量（经 rust copilot.rs 注释+关键行核对）。
