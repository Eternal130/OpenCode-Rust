# L5 权限与认证 (Permission/Auth/Copilot) — 审查报告

> 审查方式：两侧逐文件逐行阅读，结论均以代码为准（不信任文档）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明

**Ref 侧逐文件阅读**（全部实际读取）：
- `permission.ts` (347行)、`permission/saved.ts` (88)、`permission/sql.ts` (20)
- `credential.ts` (325)、`credential/sql.ts` (14)、`account/sql.ts` (36)
- `managed-policy.ts` (34)、`oauth/page.ts` (276)
- `github-copilot/`：`copilot-provider.ts`、`models.ts` (234)、`openai-compatible-error.ts`、`chat/` 7文件（含 817 行 language-model）、`responses/` 9文件（含 1636 行 responses-language-model）——chat/responses 的 aisdk 适配器在 Rust 侧由 oc-ai 的 openai_chat/openai_responses 协议机替代（架构差异，非逐行移植对象），本次对比其数据面语义（reasoning_opaque 回放、finish-reason 映射、lane 选择）。
- 相关上游：`plugin/provider/github-copilot.ts` (558)、`mcp/oauth.ts` 索引检查、`integration/connection.ts`。

**Rust 侧逐文件阅读**：
- `oc-core/src/permission.rs` (1617，含测试)、`permission/saved.rs`、`permission/sql.rs`
- `oc-core/src/integration/credential.rs` (407)、`copilot.rs` (403)、`flows.rs` (704)、`providers.rs` (copilot/xai 段)、`providers_enterprise.rs`（安装清单）
- `oc-core/src/config/managed_policy.rs` (250)
- `oc-core/src/mcp/oauth.rs` (1408，索引级)、`mcp/oauth_page.rs`
- `oc-core/src/provider_catalog/copilot_sync.rs` (345)
- `oc-ai/src/protocols/copilot.rs` (899)、`copilot_responses.rs` (240)
- `database/schema.rs`（account/credential/permission 表）、`migrations.rs`（相关段落）、`engine/mod.rs`（policy 接线段）

grep 全仓确认无 keychain（两侧均无——v2 ref 也无 keychain，token 以 JSON 明文存 sqlite，语义对等）。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| evaluate：flatten 后**最后匹配**胜、默认 ask | permission.ts:87-97 | permission.rs:60-69 | ✅语义等价 | `.rev().find()` 对应 `findLast` |
| merge/configured：agent 缺失⇒deny-all，session 追加覆盖 | permission.ts:19,158-163 | permission.rs:50-52,78-82 | ✅ | |
| 决策链：base 任一 resource deny 短路（不查 saved）；saved allow 追加最后；含 ask⇒ask | permission.ts:165-189 | permission.rs:90-94,431-449 | ✅ | 测试证明 deny 路径不触发 saved.list (permission.rs:1008-1037) |
| 通配符匹配（action 对 pattern、resource 对 pattern，`*`跨`/`、`?`单字符） | permission.ts:91 + util/wildcard | permission.rs:61-63 + oc_util::wildcard | ✅ | 边缘用例齐全 (permission.rs:875-906) |
| permission.evaluate 插件 hook（仅非短路路径，可改 effect/message） | permission.ts:179-188 | permission.rs:450-468 (HookOverride) | ✅ | deny 短路不触发 hook 有测试 (1071-1099) |
| ask：ask 效果也 park+发布事件；closed⇒返回 deny | permission.ts:223-229 | permission.rs:510-523 | ✅ | |
| assert：deny⇒BlockedError(rules=relevant, message)；ask park 至 reply | permission.ts:231-263 | permission.rs:527-558 | ✅ | Declined 作为 defect 语义有文档化约定 |
| reply：先 await 发布 replied；reject 批量拒同 session（feedback⇒Corrected） | permission.ts:265-297 | permission.rs:575-600 | ✅ | 事件顺序有 golden 测试 (1284-1302) |
| reply always+save：持久化 grant 后批量重评放行 | permission.ts:299-322 | permission.rs:602-652 | ✅ | session 消失⇒留 pending (641-642)，对应 catchTag |
| close：批量 reject | permission.ts:133-145 | permission.rs:680-688 | ✅ | |
| 重复 pending id = defect | permission.ts:213-214 (Effect.die) | permission.rs:500 (assert!) | ✅ | |
| saved grants：唯一索引 (project,action,resource)、conflict-do-nothing、空 resources no-op、读回为 allow | permission/saved.ts:63-78, sql.ts:19 | permission/sql.rs (索引/SQL 同名同列), saved.rs:92-110 | ✅ | G-114：Rust 把 ref 的 orDie 显式化为 Store 错误（行为更严谨）🔵 |
| managed policy：state 整体替换、文档逆序合并、Console 最后、`{action}:{resource}` 判定 | managed-policy.ts:23-32 + config/plugin/policy.ts | config/managed_policy.rs:61-158 | ✅ | 接入 engine (engine/mod.rs:1164-1178) |
| provider.use 剪枝（last-match + wildcard） | policy.ts:44-52 | managed_policy.rs:128-137 | ✅ | |
| credential 表（active 选中、时间排序、active_pick/replacement_pick 顺序） | credential.ts:84-100,186,287 | integration/credential.rs:17-22,55-110 | ✅ | SQL 与排序完全一致 |
| credential create：activate!==false||无现有 ⇒ 激活；legacy NULL active 钉住 | credential.ts:112-141 | credential.rs:128-154 | ⚠️行为差异 | Ref `!current.active` 对 active=false 也钉住；Rust 只在 active IS NULL 时钉 (credential.rs:136-144)。极端 legacy 边缘场景 |
| credential activate/update/remove + 事件 (updated/switched) 语义 | credential.ts:169-320 | credential.rs:176-331 | ✅ | update 仅 label 变化发 updated；time_updated 不 bump（G-117）均对齐 |
| 坏 value 解码：整读失败（不静默降级） | credential.ts:63-67 (decodeUnknownSync throw) | credential.rs:341-348 | ✅ | G-225 显式化 |
| token 存储 | sqlite credential.value 明文 JSON | 同 (schema/migrations.rs:415) | ✅ | 两侧均无 keychain/加密，风险对等 |
| OAuth 回调页（branded success/error/bootstrap） | oauth/page.ts:21-133 | mcp/oauth_page.rs + flows.rs:264-305 | ✅ | 错误回调 400+error 卡、deferred 应答 (flows.rs:139-158,398-421) |
| PKCE S256（32B verifier，snowflake 48B）+ state nonce + JWT claims | mcp/oauth.ts, openai.ts 等 | flows.rs:24-74 | ✅ | |
| 环回监听：pinned 端口 /cancel 探测重试、bootstrap POST-back、隐式流 | openai.ts:131-138, digitalocean.ts | flows.rs:80-129,307-481 | ✅ | 手写 HTTP/1.1 解析，读上限 64KB |
| 设备码流（RFC 8628）：+3s margin、slow_down 退避 | github-copilot.ts:20,87-148, xai.ts | flows.rs:638-666 | ✅ | zen 变体（先睡、无 margin、JSON body、累积+5s）flows.rs:672-704 🔵 |
| Copilot OAuth：clientID/scope、github.com↔enterprise URL、表单字段、key 方法移除 | github-copilot.ts:17,47-157,213 | providers.rs:348-421,482-579 | ✅ | 常量逐字一致；Rust 额外接受显式 http(s) GHE origin 🔵 |
| Copilot entitlement：仅 chat_enabled===false 拒绝，双消息；user 查询失败不拒 | github-copilot.ts:365-371,114-119 | providers.rs:461-480,527-546 | ✅ | |
| Copilot token：access=refresh=GitHub token、expires=0、无 refresh | github-copilot.ts:123-136 | providers.rs:557-563,415, copilot.rs:222-224 | ✅ | 无二次 token 交换（两侧一致——直接以 GitHub token 为 Bearer） |
| base URL：apiEndpoint > enterpriseUrl(copilot-api.{域}) > api.githubcopilot.com | github-copilot.ts:359-377 | copilot.rs(integration):119-143 | ✅ | |
| /models 拉取：5s 超时、usable 过滤、AIC→USD(10000/batch)、cache_read??cache、Date.parse(version)、variants 三分支 | models.ts:81-234 | oc-ai copilot.rs:239-754 + copilot_sync.rs:128-160 | ✅ | Rust 手写 civil-days Date.parse |
| derive：别名保留条件（API model 存在）、远端新增、gpt-5-chat-latest 隐藏 | models.ts:97-118, ts:236-244 | copilot_sync.rs:128-160, oc-ai:568-588 | ✅ | |
| 请求头策略：Bearer 重写、UA、Openai-Intent、api-version、x-initiator 只升不降、Copilot-Vision-Request、anthropic-beta | github-copilot.ts:464-496 | oc-ai copilot.rs:34-111 (client+request headers) | 🟡 | `anthropic-beta: interleaved-thinking`（anthropic lane, ref :495）未见对应注入——Claude-on-Copilot /v1/messages 路径走 anthropic 协议机，需确认其默认头；requestMetadata 的 messages-lane `tool_result`-only agent 判定被简化为统一 `!ToolResult` 检查（oc-ai:81-88），语义近似但不逐字 |
| X-Interaction-Type 词汇表 + session affinity + x-initiator=agent(非会话步) | github-copilot.ts:266-280,500-505 | oc-ai copilot.rs:59-66 (interaction_type)；engine 侧注入 | 🟡 | 词汇表一致；X-Interaction-Id(session affinity) 的注入点在 engine，未在本域文件中逐字核对 |
| lane 选择：endpoint 选项优先，gpt≥5(除 gpt-5-mini*)/grok/gemini/mai-code → /responses | github-copilot.ts:325-342 | copilot_responses.rs:64-107 | ✅ | 正则逐字移植 |
| 免费 utility 标题（gpt-4o-mini、20s 超时、专用头、失败回退） | github-copilot.ts:402-462 | copilot_sync.rs:169-267 + oc-ai:763-897 | ✅ | |
| chat 适配：reasoning_opaque 捕获/回放（仅在有 handle 时带 reasoning_text） | chat/convert-*.ts:69-124 | oc-ai copilot.rs:113-159 | ✅ | AI-COPILOT-C2 |
| responses 适配（store:false、include encrypted_content、finish-reason 映射） | responses/*.ts | copilot_responses.rs:222-240 (借 OpenAI Responses 协议机) | 🟡 | 数据面等价，但 ref 1636 行专用实现 vs 复用 openai_responses 机器——细节差异未逐行核对（超出本域 oc-ai 深度） |
| MCP OAuth（发现/DCR/PKCE/resource 校验/刷新） | mcp/oauth.ts | mcp/oauth.rs:263-695 | ✅ | www-authenticate 挑战、resource 允许清单校验均在 |
| 账户表 account/account_state/control_account（含 org 迁移终态 active_org_id） | account/sql.ts:5-36 | schema.rs:471-508, migrations.rs:140-181 | ✅ | 迁移链含 20260309 move_org_to_state 对等操作 |
| API key 环境变量回退（ANTHROPIC_API_KEY、OPENCODE_API_KEY 级联、GITHUB_TOKEN） | config/provider 定义（schema/env 链） | provider_catalog/catalog.rs:885-959, facade.rs:1078,1517 (package_env_chain), oc-ai copilot.rs:201 (Secret::Env("GITHUB_TOKEN")) | ✅ | env 链组合有测试 |
| OpenAI/ChatGPT OAuth（browser/headless→codex 后端、token-sharing→resource URL、模型整形/限额） | openai.ts, chatgpt.ts | providers_enterprise.rs:973-1261+, integration/copilot.rs:235-324 | ✅ | codex 资格谓词、fallback 模型表、cost 清零、400k/272k 限额齐 |
| xAI/DigitalOcean/Poe/GitLab/Snowflake/zen OAuth | plugin/provider/*.ts | providers.rs:589+,749+,828+; providers_enterprise.rs:88+,725+ | ✅ | 安装清单覆盖 ref 全部 OAuth provider |

### 3. 关键差异与风险 Top 5

1. **⚠️ credential create 的 legacy 钉住分支**：Ref（credential.ts:129 `!current.active`）对 `active=false` 的现有行也重新钉为 active；Rust（credential.rs:136-144）仅在 `active IS NULL` 时钉。多账号 + 全部 inactive 的 legacy 库里，Ref 会强选一行 active，Rust 不会 ⇒ credential.switched 事件与激活语义可能分叉。低概率、真实存在。
2. **🟡 anthropic-beta 头缺失风险**：Ref 在 anthropic（/v1/messages）lane 强制 `anthropic-beta: interleaved-thinking-2025-05-14`（github-copilot.ts:495）；Rust 的 copilot 头策略（oc-ai copilot.rs:101-111、copilot.rs(integration):107-113）均无此头。若 Claude-on-Copilot 走 Rust 的 anthropic 协议机且该机不注入，交错思考可能被服务端拒绝/降级。建议核实 anthropic 协议机默认头。
3. **🟡 requestMetadata 的 agent 判定简化**：Ref 对 messages/completions/responses 三种 body 形状分别判定 last-turn（responses: 非末位 user 即 agent；chat: tool-only content 也算 agent；messages: tool_result-only）。Rust（oc-ai copilot.rs:80-95）统一为「非 user 或含非 ToolResult part」，未区分 image/tool_result 嵌套等细节——`x-initiator` 错标为 user/agent 会影响 Copilot 端计费归类与限流，建议补逐形状对齐测试。
4. **🟡 X-Interaction-Id（session affinity）注入点未验证**：Ref 用 `SessionAffinity.get(session)` 稳定子会话 id（github-copilot.ts:276）；Rust 侧 interaction_type 已移植但 affinity id 的实际注入在 engine 层，本域内未见证据。若缺失，Copilot 端会话亲和（缓存/限流）退化。
5. **架构性差异（非缺陷，需持续看守）**：Ref 每个 provider 是 aisdk 插件 + 817/1636 行专用 language model；Rust 以共享 openai_chat/openai_responses 协议机 + Copilot 扩展点替代。当前数据面（reasoning_opaque、lane、finish reason、SSE 终止条件 `[DONE]`+response.completed 双判）已对齐并有测试，但 ref 专用实现中的长尾错误分类（openai-compatible-error.ts、openai-error.ts）未逐行核对，流式错误重试语义可能存在偏差。

### 4. 缺失文件/功能汇总

**无整块缺失。** 本域全部功能在 Rust 侧有对应实现且大体逐语义移植。遗留核对项：
- `anthropic-beta` interleaved-thinking 头（风险2）——grep `anthropic-beta` 于 oc-ai 确认；
- `SessionAffinity`/`X-Interaction-Id` 注入点（风险4）；
- copilot chat/responses 的错误分类与重试（openai-compatible-error.ts 对应物）；
- credential legacy 钉住分支的 `active=false` 案例（风险1）。

总体评价：该域移植质量异常高——权限决策链（短路顺序、事件先于唤醒、批量 reject/always 语义）、credential 选中/替换 SQL 排序、Copilot 设备流（clientID/scope/URL/entitlement 文案逐字一致）、models derive（AIC 换算、variants 三分支、别名保留）均有代码+测试双重对齐，并多处以 G-编号标注与 ref 行号的映射。上述 5 项为边角风险，非结构性缺口。
