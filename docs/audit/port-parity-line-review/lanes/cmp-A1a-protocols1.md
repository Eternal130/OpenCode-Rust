# A1a 结论: openai 系+anthropic+gemini/interactions/vertex 协议族移植整体高度对齐（状态机/降级/usage 方向/常量逐项一致），但存在 4 处正确性风险（gemini 命名空间扁平丢字段、google-interactions 事件形状零校验、vertex Express+tuned 守卫缺失、/compact 响应宽容解析）与若干容错/元数据级差异。

## 对比表

| 协议功能 | ref | rust | 差异 | 状态 |
|---|---|---|---|---|
| openai-chat 请求体（messages/system/tools/temperature/maxTokens） | openai-chat.ts:804-854 | openai_chat.rs:2633-2765 | 结构一致；`max_tokens`/`max_completion_tokens` 探测表 rust 多 deepinfra 条目（D-3） | ✅(D-3) |
| openai-chat cache hint 注入（cache_control/4 断点/TTL） | utils/cache.ts:16-23；openai-chat.ts:333-341 | openai_chat.rs:1645-1668,1995-1998 | cap=4、`>=3600→"1h"` 一致 | ✅ |
| openai-chat 媒体映射（PDF file / image_url） | openai-chat.ts:365-380 | openai_chat.rs:2049-2096 | 一致（data_url/URL/inline 优先级同） | ✅ |
| openai-chat reasoning（scalar 字段优先级+OpenRouter/Kimi details 合并） | openai-chat.ts:942-1041,1097-1115 | openai_chat.rs:503-767,888-979 | merge/conflict/identity 逐条对齐；rust 额外 Copilot reasoning_opaque（ref 在 copilot lane） | ✅ |
| openai-chat 工具流（index 匹配/pending/finishAll） | openai-chat.ts:1126-1197 | openai_chat.rs:1005-1134 | fallback/matched/nextToolIndex 逻辑一致 | ✅ |
| openai-chat finish（requireFinishReason/stop→tool-calls 改写） | openai-chat.ts:1221-1264 | openai_chat.rs:1185-1260 | 一致（含 metadata 快照、reasoning 补 start） | ✅ |
| openai-chat 错误帧/finish_reason 映射 | openai-chat.ts:862-896,1045-1053 | openai_chat.rs:476-497,811-821 | 文案逐字一致；rust usage 校验多一个字段（D-4） | ✅(D-4) |
| openai-responses 请求体（hosted image tool/namespace/tool_choice） | openai-responses.ts:150-215 | openai_responses.rs:97-411 | 一致（含 `image_generation` tool_choice 特判、namespace 兜底描述） | ✅ |
| openai-responses hosted tools/checkpoint/compaction | openai-responses.ts:217-262,279-288; utils/responses-{checkpoint,compaction}.ts | openai_responses.rs:415-597; utils/responses_{checkpoint,compaction}.rs | hosted item 恢复校验轻量化（D-11）；checkpoint id 铸造格式不同（D-13）；/compact 响应解析宽容（D-12） | ✅(D-11/12/13) |
| openai-responses effort updates（GPT-6+/pro/contextManagement 门控） | openai-responses.ts:141-148 | openai_responses.rs:315-364 | 正则语义一致（`(?:^|\/)gpt-(\d+)`） | ✅ |
| openai-compatible chat/responses 路由 | openai-compatible-{chat,responses}.ts | openai_compatible.rs | id/metadataKey/defaults 一致 | ✅ |
| anthropic 请求体（system/messages/thinking/output_config/顶层透传） | anthropic-messages.ts:1035-1101 | anthropic_messages.rs:1763-1941 | DEFAULT_MAX_TOKENS=32000、MIN=1024、DEFAULT_EFFORT=high、可选 null 重发均一致 | ✅ |
| anthropic cache_control（4 断点 tools→system→messages、drop 警告） | anthropic-messages.ts:518-531,1042-1064 | anthropic_messages.rs:615-629,1799,1836-1838 | 一致；警告走 eprintln（G-309） | ✅ |
| anthropic thinking 降级（signature 缺失→text/空签名/redacted） | anthropic-messages.ts:933-959,799-816 | anthropic_messages.rs:1461-1518,1167-1192 | kimi/moonshot/xiaomi 白名单逐串一致 | ✅ |
| anthropic 系统更新（native/wrapped/hold/split 检查） | anthropic-messages.ts:818-855,871-999 | anthropic_messages.rs:1194-1233,1342-1586 | 一致 | ✅ |
| anthropic SSE 状态机（block start/delta/stop、compaction、server tool） | anthropic-messages.ts:1217-1582 | anthropic_messages.rs:2305-2832 | 一致；thinking delta 事件 metadata 丢失（D-5） | ✅(D-5) |
| anthropic usage（SUM 方向/merge 右偏/iterations） | anthropic-messages.ts:1120-1180 | anthropic_messages.rs:1960-2069 | 方向与 merge 语义一致 | ✅ |
| anthropic beta 头注入 + route（?beta=true/version 头） | anthropic-messages.ts:1613-1671 | anthropic_messages.rs:1676-1760,2922-2953 | transport 包装语义一致（读 body→并集） | ✅ |
| gemini 请求体（contents/thinkingConfig/functionDeclarations/toolConfig） | gemini.ts:272-491 | gemini.rs:992-1476 | MIN_THINKING_BUDGET=512、includeThoughts 默认 true、budget fit 一致 | ✅ |
| gemini thought signature 回放/skip 哨兵/omitCallIds/legacyToolMedia | gemini.ts:37-53,300-303,341-433 | gemini.rs:53-91,1113-1302 | 模型门控正则语义一致；命名空间重写丢字段（R-1） | ⚠️(R-1) |
| gemini 流式状态机（thought/text 切换/inlineData→media/functionCall id 去重） | gemini.ts:500-791 | gemini.rs:463-920 | 一致，含 reasoning-start 携带 signature 的 TS 不对称镜像 | ✅ |
| gemini usage（inclusive prompt、visible+thoughts 求和）+onHalt finish | gemini.ts:500-583 | gemini.rs:463-653 | 一致 | ✅ |
| google-interactions 请求体（steps/store/previous_interaction_id/topK 拒绝） | google-interactions.ts:185-338 | google_interactions.rs:120-453 | options 校验 ref=lenient 丢弃 / rust=硬失败（D-7）；事件形状零校验（R-2） | ⚠️(D-7,R-2) |
| google-interactions 状态机（step.start/delta/stop/completed 终态映射） | google-interactions.ts:367-521 | google_interactions.rs:524-794 | requires_action→tool-calls、incomplete→length、终态白名单一致 | ✅ |
| vertex gemini（labels/serviceTier 头/Express/ADC/tuned endpoints） | google-vertex.ts | vertex.rs:38-268 | host/project/env 序列一致；Express+tuned 守卫缺失（D-9/R-3）；apiKey+accessToken 互斥缺失（D-10） | ⚠️ |
| vertex chat/responses（endpoints/openapi 根） | google-vertex-{chat,responses}.ts | vertex.rs:271-423 | base URL 组装/global 回退/API key 拒绝一致；显式 baseURL 走模板展开（plugin 语义，注释声明） | ✅ |
| vertex messages（streamRawPredict/anthropic_version） | google-vertex-messages.ts | vertex_messages.rs | VERSION=vertex-2023-10-16、HEADER=2023-06-01、body 去 model 一致 | ✅ |
| tool-schema 投影（objectRoot/gemini/moonshot/选择顺序） | utils/tool-schema.ts | sanitizer.rs | anyOf 裸 object 分支丢弃、enum type 推导、family 匹配一致 | ✅ |
| partial-json（容错集 STR/NUM/ARR/OBJ/NULL/BOOL/NAN/INF） | utils/partial-json{,-options}.ts | utils/partial_json.rs | 位集一致；NaN/±Infinity → Malformed（ref 产生值，文件头声明 D-1） | ✅(D-1) |
| tool-stream（start/appendOrStart/append/finish/finishWithInput/finishAll） | utils/tool-stream.ts | utils/tool_stream.rs | 事件归并与空 delta 短路一致；providerExecuted(Some(false)) 映射差（D-2） | ✅(D-2) |
| lifecycle（start/delta/end/finish 关开块） | utils/lifecycle.ts | utils/lifecycle.rs | 一致；关块顺序 rust 排序（确定性，语义同） | ✅ |
| gemini-json-schema（required 过滤/04 bounds/tuple/cutLoops） | utils/gemini-json-schema.ts | utils/gemini_json_schema.rs | 含非元组 additionalItems 保真等 oracle 修正均一致 | ✅ |
| shared（token 方向助手/wrapSystemUpdate/parseToolInput/flattenTools/toolResultText） | shared.ts | shared.rs | sum/subtract/total/fit budget/64 字符 clamp 一致 | ✅ |
| speaker-turns | utils/speaker-turns.ts | （在对应消费 lane 内） | 未单列文件 | ➖ |
| openai-options（effort/verbosity/serviceTier 含 "scale"） | utils/openai-options.ts:12 | options 解析在 openai-options/options.rs（消费侧） | `scale` tier 为 openai 系独有扩展，两侧一致 | ✅ |

## 行为差异清单 D-n

- **D-1** partial-json：`Infinity`/`-Infinity`/`NaN` 字面量在 rust 解析为 `Malformed` 错误，ref 解析为对应 JS 数值（partial_json.rs:224-245 vs partial-json.ts:141-164）。文件头已声明；工具参数流不含这些字面量，实际影响≈0。
- **D-2** tool_stream：`provider_executed: Some(false)` 在 rust 事件中被映射为 `Some(true)`（tool_stream.rs:80,115），ref 将 falsy 映射为 `undefined`（tool-stream.ts:59,99）。现网协议只设 `Some(true)`/`None`，理论差异。
- **D-3** openai_chat 的 `detect_max_tokens_field`/`detect_supports_store` 比 ref 探测表多 deepinfra 条目（openai_chat.rs:1818-1822,1856-1857 vs openai-chat.ts:682-756）。ref 由 deepinfra provider 显式 compat 覆盖；rust 经 facade 注入同等 compat，仅"无 compat 的 openai-compatible 路由指向 deepinfra URL"这一路径下两者探测结果不同。
- **D-4** openai_chat `decode_usage_value` 额外校验 `prompt_tokens_details.cache_creation_input_tokens` 为数字（openai_chat.rs:226，为 Venice 重写服务）；ref schema 未声明该字段（rest 透传）。若上游发送非数字，rust 使整事件失败、ref 放行。极端边角。
- **D-5** anthropic `content_block_start(thinking)`：ref 的首个 thinking delta 事件携带 `{signature}` providerMetadata（anthropic-messages.ts:1278-1280），rust 的 reasoning_delta 传 `None`（anthropic_messages.rs:2380）。start 事件两侧都带；仅 delta 事件元数据缺失。
- **D-6** gemini 错误帧 `error.code`：rust `as_u64→u16` 截断（gemini.rs:675），>65535 的内部码被丢弃；ref 传任意 number（gemini.ts:597）。可能影响个别 google 内部码的重试/分类匹配。
- **D-7** google_interactions `decode_options`：ref 五个选项全部 `lenient`（坏值丢弃、请求照发，google-interactions.ts:31-40），rust 对非字符串/越界枚举硬失败 `invalid_request`（google_interactions.rs:56-110）。
- **D-8** google_interactions 事件解码仅做 JSON parse，无 ref `Event` union 的形状校验：`index` 缺失静默取 0（google_interactions.rs:710,715,720），`step` 缺失退化为 "Unsupported Interactions step" 错误而非 "invalid stream event"（R-2）。
- **D-9** vertex gemini：ref 在 apiKey+`endpoints/` tuned 模型组合抛 `ProviderConfigurationError("Google Vertex tuned models do not support Express Mode API keys")`；rust `vertex_route_for_model` 无此守卫（vertex.rs:212-268），静默拼无效 URL，用户得到上游 404 而非本地配置错误。
- **D-10** ref `model()` 的 apiKey+accessToken 互斥错误（"cannot be combined"）rust 未实现（VertexConfig 两字段并存时 api_key 静默胜出）。
- **D-11** openai_responses `restore_hosted_tool_item` 只校验 type+非空 id（openai_responses.rs:75-79），ref 用完整 schema 守卫（openai-responses.ts:54-86）。文件头已声明为"轻校验"有意分歧；畸形 hosted item 在 rust 会被当作可回放而 ref 丢弃。
- **D-12** /compact 响应解析宽容化：rust 对未知 output item 生成空 user 消息、非 `user/assistant` role 一律归 user、`compaction.encrypted_content` 缺失时填 `""`（responses_compaction.rs:254-267,311-319）；ref 严格 union，这些情况整体 `Invalid compaction response` 失败（responses-compaction.ts:60-81,140-144）。
- **D-13** responses_checkpoint：rust 为无 id 的 compaction item 铸造确定性 id `cmp_000…`（responses_checkpoint.rs:87-89），ref 依赖 OpenResponses.normalize 的 UUID 铸造。回放 id 字符串形态不同（仅诊断/metadata 可见）。
- **D-14** lifecycle 关块签名顺序：rust 按索引排序（lifecycle.rs:130-141），ref 按 JS 对象数字键插入序。语义等价，rust 更确定。
- **D-15** 错误 route 字段：openai_chat/anthropic 部分错误 rust 用 `provider_label` 或 ADAPTER，ref 恒用 ADAPTER 常量。诊断口径略异。
- **D-16** vertex 显式 baseURL 的 openapi lane：rust 做 `${GOOGLE_VERTEX_*}` 模板展开（vertex.rs:316-333，注明移植 provider plugin 语义），ref package `configuredRoute` 原样使用 baseURL。携带模板的 baseURL 直接传入 package API 时两者结果不同；oc-core catalog 路径下一致。

## 正确性风险 R-n

- **R-1（gemini 命名空间扁平丢字段）** gemini.rs:1345-1371 `rename_namespaced` 重写 namespaced tool-call/tool-result 时把 `provider_executed`/`cache`/`metadata` 置 None；ref `flattenToolRequest` 用 `{...part}` 展开保留全部字段（shared.ts:304-320）。同 crate 的 openai_chat.rs:1687-1714 与 anthropic_messages.rs:1300-1340 均保留。后果：经命名空间工具发出的 provider-executed 调用（server tool）或带 cache hint 的调用，仅在 gemini 路由上回放语义降级（server_tool_use 退化为普通 tool_use、缓存断点丢失）。建议对齐为 clone+改 name/namespace。
- **R-2（google-interactions 无事件形状校验）** ref `Event` union 要求 `step.start` 携带 `step` 与数字 `index`（google-interactions.ts:151-166），缺任一则整帧失败；rust 缺 `index` 静默取 0（google_interactions.rs:710-721），两个不同 step 可能被折叠到 index 0（steps map 键冲突、工具累积串扰），缺 `step` 则报语义错位的消息。中低概率、高混淆度；建议补最小形状校验。
- **R-3（vertex Express+tuned 组合未守卫）** ref 拒绝 apiKey+`endpoints/*`（D-9），rust 静默生成无效组合 URL——用户得到上游 404 而非本地配置错误。同时 apiKey+accessToken 并存时 rust 静默选 api-key。建议补两个 ProviderConfiguration 守卫。
- **R-4（/compact 响应宽容解析）** rust 将 ref 会整体拒绝的畸形 compaction 输出转成"成功但内容为空/残缺"的替换历史（D-12），错误被吞掉后压缩结果可能静默丢内容。建议至少对未知 item 类型与缺失 encrypted_content 报 `InvalidProviderOutput`。
- **R-5（D-7 的用户可见面）** google-interactions 选项硬失败违背 ref lenient 契约：调用方带非法 `thinkingLevel` 等选项的请求在 rust 直接失败而非"忽略该选项继续"。建议改为丢弃。

## 覆盖声明

- **已逐行读完（双侧非测试部分全部）**：ref openai-chat.ts、openai-responses.ts、openai-compatible-chat.ts、openai-compatible-responses.ts、anthropic-messages.ts、gemini.ts、google-interactions.ts、shared.ts；utils/ 下 cache.ts、claude-model.ts、tool-schema.ts、tool-stream.ts、partial-json.ts、partial-json-options.ts、lifecycle.ts、gemini-json-schema.ts、openai-options.ts、responses-checkpoint.ts、responses-compaction.ts、responses-hosted-tools.ts、speaker-turns.ts。rust openai_chat.rs（非测试 1-3622）、openai_responses.rs、openai_compatible.rs、anthropic_messages.rs（非测试 1-2969）、gemini.rs（非测试 1-1595）、google_interactions.rs、vertex.rs、vertex_messages.rs、shared.rs、sanitizer.rs；utils/ partial_json.rs、tool_stream.rs、lifecycle.rs、gemini_json_schema.rs、responses_checkpoint.rs、responses_compaction.rs、responses_hosted_tools.rs。
- **部分依赖、未逐行**：两侧 open-responses 主文件（属 A1b lane）仅经调用面交叉核对（签名级）；ref gemini-generate-content.ts / media-input.ts 与 rust gemini_generate_content.rs 仅经调用点核对；venice-chat.ts / copilot 扩展、mistral/cohere/zai 等 lane 外协议只在与 openai_chat.rs 交汇处比对；provider-error.ts / provider_error.rs 的分类规则本体未复核（A2 lane 域）。内嵌测试未逐行。
- **C2a D-9 复核结论**：openFragment/closeFragment（text/reasoning 块开合）语义在两侧都位于共享 lifecycle + 协议层显式开块：rust 在 openai_chat.rs:944-963 与 gemini.rs:803-822 以"先显式 reasoning_start(metadata) 再 delta"精确复刻了 TS 不对称——D-9 遗留问题在该两协议已正确落到协议层；anthropic 的 thinking delta 事件 metadata 差异见 D-5（start 侧已对齐，仅 delta 事件缺）。
