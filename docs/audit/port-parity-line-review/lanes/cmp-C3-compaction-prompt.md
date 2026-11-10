# C3 结论: 压缩与指令/系统提示域移植高度忠实（阈值数学、split/deliver/shrink、事件序列、Key/Hash/Delta 语义、agent 基线全部逐行对齐，文案逐句 verbatim），发现 7 处行为差异（最大的是 Plan 提醒不落库且渗入压缩请求）与 2 处低风险点，无致命缺陷。

## 对比表

| 域 | ref（file:line） | rust（file:line） | 判定 |
|---|---|---|---|
| 触发枚举 auto/overflow/manual | compaction.ts:53-57 | compaction.rs:56-97（Reason + wire() 折叠 overflow→auto，防 G-252②） | ✅ |
| Settings{auto,buffer,keep:15000} | compaction.ts:41-47,197 | compaction.rs:39-53（Default keep 15_000） | ✅ |
| auto 关闭时仅 manual 可压（含 overflow） | compaction.ts:210 | compaction.rs:290-292 | ✅ |
| due()：末尾 completed 压缩 / measured<checkpoint / estimate≥ceiling | compaction.ts:242-251 | compaction.rs:948-974（-1 哨兵语义显式保留） | ✅ |
| calculateCeiling（10% vs RESERVE_MIN 16000，buffer 覆盖，input‖context，∞） | compaction.ts:905-912 | compaction.rs:978-994 | ✅ |
| UNKNOWN_WINDOW 200k / SHRINK_STEPS .7/.5/.35 | compaction.ts:95-99 | compaction.rs:134-142 | ✅ |
| overflow 首预算 min(cap, ⌊est×0.7⌋) | compaction.ts:227-228 | compaction.rs:325-329 | ✅ |
| buildPrompt(update,legacy) 全部文案 | compaction.ts:104-185 | compaction.rs:149-189 | ✅ 逐句一致（含 SUMMARY_TEMPLATE/RULES/NUDGE/LEGACY_HEADING） |
| nudge 一次重试 + headings 匹配 | compaction.ts:283-306 | compaction.rs:378-421 | ✅ |
| deliver 循环：fits/flatten/413-as-text-不计次/shrink/重试策略 | compaction.ts:384-455 | compaction.rs:536-654（is_original 布尔替代引用相等，分支逐一标注） | ✅ |
| flattenAndDropOldest（lead=prev summary 去 recent / 交换组整丢 / note 预留） | compaction.ts:465-504 | compaction.rs:1388-1474 | ✅ |
| splitConversation/recentStart/oldestToDrop（user 边界、latest exchange、previous.recent） | compaction.ts:702-750 | compaction.rs:1174-1261 | ✅ |
| messageToText 全类型格式（user/skill/file/location/assistant/tool/shell background） | compaction.ts:753-805 | compaction.rs:1266-1356 | ✅（一处空 name 微差，见 D-7） |
| truncateToolOutput 1250 码点 | compaction.ts:807-820 | compaction.rs:1360-1366（chars() 计数=码点） | ✅ |
| estimateContext/estimatePrompt（anchor 测量+未测量估算、unmeasured 过滤 anchor 自身） | compaction.ts:868-898 | compaction.rs:1009-1055 | ✅ |
| estimateRequest/Tool/Message/Part/Media（image 1500/pdf 2000/effort 0/compaction 按 text） | compaction.ts:925-963 | compaction.rs:1070-1154 | ✅（G-279⑤ 有效名、G-256 UTF-16/4 修正有注释） |
| Token.estimate = round(len/4)，UTF-16 单位 | util/token.ts:5 | compaction.rs:1003-1005（encode_utf16） | ✅ |
| hasMeasuredPrompt（同 provider、无 error、input+cache>0） | compaction.ts:918-923 | context.rs:1692-1703 | ✅ |
| native 路径：provenance 稳定端点校验、trigger/endpoint 机制、recentUserMessages+checkpoint、endpoint replacement | compaction.ts:324-372 | compaction.rs:436-530 | ✅（D-8 checkpoint None 边界） |
| recentUserMessages（strip skills、unsupportedParts、boundImages、keep 内最新整条） | compaction.ts:854-866 | compaction.rs:1577-1609 | ✅ |
| 压缩事件序列：started(非 manual=reason auto+recent)/delta/ended(+usage)/failed(+inputID+usage)/usage.recorded(source=compaction)/interrupted | compaction.ts:394,623-672 | compaction.rs:551-560,752-815 + drain.rs:340-370,703-729（中断时 clear_spent+interrupted） | ✅ |
| fromHook（hook 供摘要、tokens 计费、metadata 进 publish options） | compaction.ts:594-608 | compaction.rs:704-729 | ✅ |
| prepare（baseTranscript、inputTokens measured=budget、native webSocket="session"） | compaction.ts:575-592 | compaction.rs:664-699 | ✅ |
| spent 按会话累积、ensuring 清理、每调用即时计费 | compaction.ts:614-621,671 | compaction.rs:202,279-281,732-750 | ✅ |
| StepFinish 计费+providerState 按 metadataKey；finish unknown/error/length/content-filter 分类 | compaction.ts:506-565 | compaction.rs:819-905（error finish→Provider 错误走重试，与 ref UnknownProviderError 一致） | ✅ |
| runStep 每次尝试前 auto 压缩；completed→新消息 id 重来；overflow 恢复一次性（Compacted 后关闭） | llm.ts:207-290 | drain.rs:690-872（AtomicBool + Outcome::Compacted store(false)） | ✅ |
| 入口门（!force&&!continuing 需 promotable Input；steer 作用域遇 queued 非 control 即 Complete） | llm.ts:61-66 | drain.rs:264-277 | ✅ |
| inbox：compaction 控制语义（admit 去重、steer 优先不跨 move、splice 到队首、cancel 冲突） | inbox.ts:164,204-224,468,493-547 | inbox.rs:145,318,368-392,531-539,590-595 | ✅ |
| 手动 /compact：eager inbox.delivered+started(reason manual,inputID)、preview、取消→"Compaction cancelled" | llm.ts:113-157 | drain.rs:339-370,435-475,597-660 | ✅ |
| 系统提示模板+tool guidance（shell×2/write/edit，`${OPENCODE_TOOL_GUIDANCE}`） | system-prompt.ts:5-27 + prompt/system.txt | system_prompt.rs:3-35 | ✅ 逐句一致 |
| baseTranscript：agent.system 优先否则 make(tools)+initial；identity 插 index1；optimize 家族覆写/append | model-request.ts:100-114, identity.ts, optimize.ts | model_request.rs:152-183 | ✅（D-4/D-6 微差） |
| optimize 家族表 gpt/astra/claude(append)/kimi/trinity/muse({{MODEL_NAME}})，自定义 system 跳过 | optimize.ts:16-62,81 | optimize.rs:37-100（禁用的 tools-curation 半边两侧均未启用） | ✅ |
| instructions Key/Hash/Values/Delta：canonical JSON+sha256、Unavailable/Removed 单例、InitializationBlocked 仅首次、removed 哨兵、blobs、renderInitial/Update join("\n\n") | instructions/index.ts | context.rs:40-200,300-470（render/render_update/instruction_diff/apply_hash_delta） | ✅ |
| 指令源顺序 codemode→mcp→references→skills→discovery→builtins(date,env)→api entries（缓存友好序） | context.ts:145-159 | context.rs:1594-1609 | ✅ |
| builtins date（toDateString）/environment（env 块文案） | builtins.ts:24-54 | context.rs:887-969 | ✅ 文案一致（Platform 值见 D-3） |
| AGENTS.md discovery 源：空=observed absence、update 三态+3 上下文 diff 短者胜、removed 文案 | instruction-discovery.ts:80-149 | context.rs:763-885（two_files_patch_ctx 替代 node diff） | ✅ |
| 读后注入 AGENTS.md（in-flight claim+synthetic ledger 恢复） | session/instructions.ts | engine/instructions.rs（claim 同步、settle spawn、metadata.instruction.paths 台账） | ✅ |
| api/<key> 会话条目（context key 块/changed/removed 文案） | session/instruction-entry.ts | context.rs:971-1032 | ✅ |
| mcp/references/skill 指导源（可见性过滤、Removed 语义、文案） | mcp/instructions.ts 等 | context.rs:560-760,1388-1420 | ✅（skill 排序 localeCompare 近似，见 D-9） |
| Agent.Info 字段面 + default 权限基线（allow-all、external ask、env ask×2/env.example allow） | schema/agent.ts:39-55 | oc-schema/agent.rs:44-72 | ✅（含 JSON golden） |
| defaultID "build"、selectable 排除 subagent/hidden、默认回退链 | agent.ts:18,92-104 | location::AgentRegistry（engine/mod.rs:3094+ seed_builtin_agents） | ✅ |
| 内建 agent 组（build/general/explore/plan/compaction/title/summary，权限与文案） | plugin/agent.ts, plan.ts, session/title | engine/mod.rs:3094-3207 | ✅ |
| 会话权限收窄工具快照与指导 | context.ts:130-141 | context.rs:1585-1642（merged_permissions→snapshot+shape_request_tools） | ✅ |
| title 选择（agent.model→small 家族→primary；MINIMAL_REASONING_VARIANTS none/minimal/low） | context.ts:95-119,186 | context.rs:1539-1583,1497-1516 | ✅ |
| variant resolve（toggle 仅贡献 none、effort 优先于 budget、按 id 去重）+ PROTOCOLS 表 47 项 | variant.ts:16-25,566-621 | provider_catalog/variant.rs:1085-1183 | ✅（aisdk: sap/gitlab 见 D-5） |
| anthropic 家族（claudeInfo/manual 3.7与4<6/opus4.5/always fable·mythos、adaptive、4.6 defaults+max） | variant.ts:55-75,294-324 | variant.rs:135-175,544-600 | ✅（抽查全读） |
| withVariant 折叠（settings 深合并/headers/body、未知 variant 报错） | model-resolver.ts:137-161 | engine/resolver.rs:796-805+ | ✅ |
| MAX_STEPS_PROMPT 文案 + steps 限制（附 assistant 消息、toolChoice none） | max-steps.ts, llm.ts:222-234 | max_steps.rs:3-4 + drain.rs:762-790（prepare_primary step_limit） | ✅ |
| prompt prepare（hook 先于 skill 解析、未知 skill 报错、一次语义） | session/prompt.ts | prompt_prepare.rs（admission 路径+drain 兜底+PreparedRing） | ✅ |

## 行为差异清单 D-n

- **D-1 Plan 提醒不持久化**：ref plugin/plan.ts:59-74 在 context hook 注入提醒的同时 `ctx.session.synthetic(...)` 落一条 durable synthetic，且 :100-121 订阅 `session.created`/`session.agent.selected` 发布切换提醒 synthetic；rust 仅在 model_request.rs:187-199 请求组装时插入（注释自认），无 synthetic 落库、无切换提醒事件。影响：TUI 转录不显示提醒；重放/rehydrate 后提醒仅存在于下次请求的内存注入（语义近似但可观测面不同）。
- **D-2 Plan 提醒渗入压缩请求**：ref plan 插件只挂 `ctx.session.hook("context", ...)`，压缩请求（`requests.compaction`）不带 plan 提醒；rust 的 `base_transcript` 同时被 compaction prepare 使用（compaction.rs:671），压缩请求也做 plan 校正并可能插入 enter/leave 提醒。压缩摘要提示词上下文多了无关 reminder。
- **D-3 环境块 Platform 值**：ref `process.platform`（"darwin"/"win32"）vs rust `std::env::consts::OS`（"macos"/"windows"）（context.rs:964）。非 Linux 平台提示词文本不同。
- **D-4 optimize 家族匹配无 Info.default 回退**：ref optimize.ts:71-73 目录找不到模型时 `?? Model.Info.default(providerID,id)` 仍按 id 匹配家族；rust optimize.rs:91 `model: None` 直接返回原文。动态/目录外模型（如自定义 openai-compatible 端点的 gpt-* id）在 rust 不再套用家族模板。identity 无此问题（identity_for None 时回退 id 为名）。
- **D-5 aisdk: sap-ai/gitlab 变体派发为空（有意）**：ref variant.ts:619-620 两条 aisdk: 协议正常出 variants；rust variant.rs:1108-1116 注释声明无 npm-install bridge，返回 None（G-17②）。文档化分歧。
- **D-6 空 system part 过滤差异**：ref baseTranscript 恒产出 `[system, initial]`（initial 可为空串 part）后 identity 插 index 1；rust model_request.rs:173-183 先滤空 part、identity 插 `min(len,1)`——initial 为空时 identity 落 index 0、part 数少一。wire 层差异，主流 provider 无语义影响。
- **D-7 serialize_tool_content 空 name**：ref compaction.ts:849 `name===undefined?"":"…: name"`（空串仍带 `": "`）；rust compaction.rs:1374-1377 空串名走无后缀分支。极小文本差异。
- **D-8 native checkpoint 缺失分支**：rust compaction.rs:491-493 `outcome.checkpoint` 为 None 时窗口仅含 retained 用户消息；ref :360 直接 `Message.assistant(response.checkpoint)`（trigger 机制假定必有）。防御性分支，无 ref 对应物。
- **D-9 skill 指导排序 localeCompare 近似**：rust context.rs:712-720 注释自认用 ASCII 折叠近似 ICU tertiary；非 ASCII skill id 排序可能不同（文档化分歧）。

## 正确性风险 R-n

- **R-1 canonical_json 数值序列化**（context.rs:186-200）：serde_json 与 JS `JSON.stringify` 对浮点/大整数的文本形式可能不同（如 1.0 vs 1、指数格式）。hash 仅在单实现 DB 内闭环使用，无跨端一致性需求，风险低；仅当将来做 TS↔Rust 数据目录互通时需对齐。
- **R-2 deliver 的 `is_original` 布尔近似**（compaction.rs:576-597）：ref 以引用相等判定“未改动的原始请求”（413 免计次 as-text 只认原始请求）。rust 在 `fits` 分支与 `rejections==0 && !as_text` 兜底分支标 true，flatten 产物恒 false——逐分支核对后与 ref 语义等价（ref 的 Infinity 兜底同样是新对象）。确认无风险，留档以防后续改动破坏该不变量。
- **R-3 `due()` 的浮点 ceiling 比较**（compaction.rs:973）：ref 与 rust 均为 `number/f64 >=`，等价；rust `calculate_ceiling` 返回 f64 而内部预算用 i64，`cap as i64` 在 buffer 为负等病态配置下可能截断，ref 同为 number 语义，等价。

## 覆盖声明

**逐行读完（双侧）**：ref compaction.ts(963L)、context.ts、instructions/index.ts、instructions/builtins.ts、instruction-discovery.ts、session/system-prompt.ts、runner/prompt/system.txt、agent.ts、schema/agent.ts、plugin/plan.ts、plugin/optimize.ts、plugin/identity.ts、session/instructions.ts、util/token.ts、runner/max-steps.ts、runner/index.ts；rust compaction.rs(1609L)、system_prompt.rs、prompt_prepare.rs、plan.rs、optimize.rs、identity.rs、engine/instructions.rs、oc-schema/agent.rs、context.rs(1-200,560-1299,1300-1703 段)、drain.rs(compaction 相关 260-900 段)、inbox.rs（compaction 语义段）、provider_catalog/variant.rs(1084-1250 表+resolve+anthropic 段)、model_request.rs(base_transcript 段)、engine/mod.rs(3094-3218 seed_builtin_agents)。

**部分/抽查（未逐行，非静默跳过）**：variant.ts 其余 ~40 个家族 handler（deepseek/nvidia/zai/gemini/bedrock/vercel 等）与 rust variant.rs 对应实现仅核对常量（EFFORTS/32000/64000）与 anthropic_messages 全文，未逐行互校——建议另开抽查；variant.ts:621 全表与 rust 表逐项核对一致。model-request.ts 与 model_request.rs 只读 baseTranscript/系统组装段（请求 hook 管线属 C2a 已审域）。to_llm_message 两侧未逐行（C2a 覆盖）。ref inbox.ts/llm.ts 仅读 compaction 相关行号段。测试文件（tests_compaction.rs 等）未逐行，仅作交叉印证。optimize/*.txt 六个模板按 include_str 疑似 verbatim，仅以 rust 内嵌测试断言佐证，未与 ref system-prompt/*.txt 做字节比对——低风险，建议机械 diff 一次。
