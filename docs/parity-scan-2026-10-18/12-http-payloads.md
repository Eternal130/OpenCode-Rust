# Parity Scan 12 — HTTP 载荷字段级补漏审计（handler 实际请求/响应 vs OpenAPI/schema vs ref handler）

- 日期：2026-10-18（第二轮「补漏」通道）
- 参考：`/opt/opencode-ref`（opencode v2.0.24, TS）— `packages/server/src/handlers/**`、`packages/protocol/src/groups/**`、`packages/schema/src/**`、`packages/protocol/openapi.json`
- 目标：`/opt/opencode-rust` — `crates/oc-server/src/handlers/**`、`errors.rs`、`common.rs`、`schema_check.rs`、`crates/oc-client/src/types.gen.rs`
- 方法：**逐 handler 双侧阅读**。第一轮已证明 OpenAPI 文档逐字节一致（05 §2），因此本轮 EXPECTED 取两重口径：(a) **OpenAPI/protocol group schema**（声明形状：字段名/类型/required/optional）；(b) **ref handler 实际输出**（含错误路径选择、文案、null-vs-absent、游标编码）。ACTUAL 为 rust handler 实际序列化。只读静态审计，未跑 build/test/live 差分。
- 边界：**不重复**已登记项 —— VCS base/branch/diff/status/init（11:#1-3、#9）、PTY WS F1-F4/F16-F20、pre-ready info F14/F15、F5-F13 桩族、F21/F22、worktree.remove 不跑 git（09:D5）、project.update 伪造响应（09:D4）、worktree.list 过滤/排序（09:D7）、project.list 排序（09:D8）、migration 恒 completed（09:D12）、session.update title="" 不再生成（F11）、compact 默认 Queue（01:K-05）、fs.find 模糊算法（02:D4）、plugin/rpc/stats 桶化桩本体（F5-F7/F9）、websearch 凭据/retry-after（11:#11）、图片直通（11:#5）、reference git path（11:#6）。

---

## 0. 审计范围与总结论

| 度量 | 值 |
|---|---|
| 审计操作数 | **141 / 141**（全部 handler 级走查；其中 ~120 个取得双侧行级证据，其余见 §5） |
| 系统性发现 | **1**（请求校验中间件缺失，波及 ~139 路由） |
| S1（强类型客户端解码失败 / 分页协议断裂） | **6**（全部 NEW） |
| S2（线上行为/内容分歧） | **20**（18 NEW + 2 为 KNOWN 项的形状细化） |
| S3（信封字段/文案细节） | **13**（全部 NEW） |

**一句话结论**：路由表、错误信封注册表与绝大多数 `{location,data}`/裸数组包装是对的，但**分页游标协议（null-vs-absent、游标内容、cursor-XOR-order）、mcp resource catalog 的 `uriTemplate` 字段名、stats 响应形状、WorktreeError 的 `name` 字面量**这五处会让 v2 生态的强类型客户端（Effect SDK / 官方 TUI）在对应端点上直接解码失败；另有**请求体/查询参数校验整体缺失**这一系统性差异：v2 对每条路由做 Effect schema 校验（400 `InvalidRequestError{message,kind}`），rust 只校验 `session.create` 载荷与 `session.list` 查询两处（`schema_check.rs:6-11` 自述），其余 ~139 条路由对错型/缺必填静默接受并按默认值降级。

---

## 1. 系统性发现（H0）

| ID | endpoint | 字段 | EXPECTED | ACTUAL | 严重度 | NEW |
|---|---|---|---|---|---|---|
| H0 | 全部 141 操作（除下述例外） | request payload + query 校验 | v2 `SchemaErrorMiddleware` 对**每条**路由的 payload/query 做 Effect 校验，失败 400 `InvalidRequestError{message(TreeFormatter), kind:"Payload"/"Query"}`（`schema_check.rs:6-9` 引述的 v2 行为） | rust 仅 `session.create` 载荷（`session.rs:343`）与 `session.list` 查询（`session.rs:215`）复刻该校验；其余路由由 handler 手工 `as_str()/as_bool()` 提取，错型→静默默认/忽略，缺必填→空串/默认值，均返回 2xx | **S1**（互操作语义） | NEW（05 §3 未登记该系统性缺口；个别实例散见各 handler） |

**H0 实例清单**（EXPECTED=400，ACTUAL=2xx 或不同信封）：

| endpoint | 输入 | v2 | rust |
|---|---|---|---|
| `GET /api/session/:id/message` | `limit=99999` / `limit=abc` | 400（`isLessThanOrEqualTo(200)`，groups/message.ts:9-12） | 200（无上限；`abc`→默认 50，message.rs:37-39） |
| `GET /api/session/:id/message` | `type=banana` | 400（枚举，groups/message.ts:23-39） | 200 空列表（透传 SQL，message.rs:51） |
| `POST /api/session/:id/model` | 缺 `model` | 400（`Model.Ref` 必填） | 204，发布 `model:null` 事件（session.rs:524-536） |
| `POST /api/session/:id/agent` | 缺 `agent` | 400（必填 string） | 404 `AgentNotFoundError("")`（session.rs:511-513，错误种类也不在声明集内，见 H10） |
| `POST /api/session/:id/view` | 缺 `idle` / 非数字 | 400（`DateTimeUtcFromMillis`） | 204，缺省=now（session.rs:1344） |
| `PUT /api/session/:id/environment` | `variables:{A:1}` | 400（Record<String,String>） | 204，非串值**静默丢弃**（session.rs:1392-1399） |
| `POST /api/session/:id/interrupt` | `resume=banana` | 400（Literals["true","false"]） | 200，按 false 处理（session.rs:631） |
| `PATCH /api/session/:id` | `title:123` | 400 kind=Payload | 204 静默跳过（session.rs:474） |
| `GET /api/fs/find` | `type=banana` | 400（枚举 file/directory） | 200，按"both"处理（fs.rs:265-269） |
| `POST /api/pty` | `size:{cols:0,rows:0}` | 400（PositiveInt） | 200，0 尺寸 PTY（pty.rs:92-94） |
| `PUT /api/experimental/persistent-pty/:id` | 缺 `size` | 400（必填，schema/persistent-pty.ts:66-69） | 200 不 resize 只回读（见 H24） |
| `POST /api/credential` | `value:"plaintext"`（非 oauth/key union） | 400 | 201 原样入库并在 list/create 回显（credential.rs:123） |
| `POST /api/websearch` | 载荷错型 | 400 schema | 400 但 `invalid_request("Invalid websearch payload: …")`，无 kind（misc.rs:202-205） |
| `PUT instructions/entries/:key` | key 含大写/斜杠 | 400（Key pattern `^[a-z0-9][a-z0-9._-]*$`） | 204 接受（session.rs:1566+） |
| `GET session/:id/diff` | `context=-1` | 400（NonNegativeInt） | 200（parse 失败→None→默认，session.rs:1098） |

---

## 2. S1 — 强类型客户端解码失败 / 分页协议断裂（NEW）

| ID | endpoint | 字段 | EXPECTED | ACTUAL | 严重度 | NEW |
|---|---|---|---|---|---|---|
| H1 | `GET /api/session/:id/message` | cursor×order 规则 | `message.ts:35-41`：**cursor 与 order 同给 → 400 `InvalidCursorError("Cursor cannot be combined with order")`**；仅 cursor → 用 cursor 内携带的 order | `message.rs:53-67`：**反转** —— cursor+order（且相等）→ 200；仅 cursor 且 cursor.order=asc → 400 `"Cursor order asc does not match requested order desc"`（`requested` 缺省取 "desc"）。v2 客户端按官方模式「首页 order=asc，后续只带 cursor」翻页时，rust 第 2 页必 400；反之 v2 拒绝的请求 rust 放行 | **S1** | NEW |
| H2 | `GET /api/session`、`GET /api/session/:id/message` | `cursor.previous/next` | v2 Effect 编码丢弃 undefined 键：无游标时响应为 `{"data":[…]}` 或 `{"data":[…],"cursor":{}}`（`session.ts:77-101`、`message.ts:56-62` + groups/session.ts:184-188 optional） | rust 显式输出 `"cursor":{"previous":null,"next":null}`（`session.rs:322-326`、`message.rs:117-121`）。Effect `Schema.optional` 不接受显式 null → **v2 SDK/官方 TUI 解码 SessionsResponse/SessionMessagesResponse 直接失败**（首页/末页必中） | **S1** | NEW |
| H3 | `GET /api/mcp/resource` | `templates[].uriTemplate` | `schema/mcp.ts:109-115`：ResourceTemplate 必填字段名 **`uriTemplate`**（handler 原样序列化 service 返回） | `mcp.rs:130-141`：输出 **`"uri": template.uri_template`** —— 字段名错。必填字段缺失 → 强类型客户端解码失败 | **S1** | NEW |
| H4 | `GET /api/experimental/session/stats` | 响应体 | `schema/session-stats.ts:51-67`：`{range{from,to}, sessions:number, subagents, prompts, steps, tokens(TokenUsage 含 cache{read,write}), cost, tools, activeDays, streak, activity[], models[]}` 全必填 | `session.rs:1801-1805`：`{sessions:{total}, tokens:{input,output,reasoning}, cost}` —— `sessions` 是对象而非数字、缺 range/subagents/prompts/steps/tools/activeDays/streak/activity/models、tokens 缺 `cache`。**任何强类型客户端都无法解码** | **S1** | KNOWN(F9 桶化) 的**形状细化 NEW**（05 只登记“无分桶”，未登记 sessions 类型错 + 必填缺失） |
| H5 | `POST /api/worktree`、`DELETE /api/worktree`、`POST /api/worktree/refresh` | 错误信封 `name` | `groups/worktree.ts:13-24`：`WorktreeError{name: Literal("WorktreeError"), data{message, forceRequired?}}`，handler 恒传 `"WorktreeError"`（worktree.ts:33-44） | rust `name` 字段传的是机器码：`"invalid_name"/"source_not_found"/"destination_exists"/"directory_unavailable"/"operation"`（`common.rs:183-192` + `project.rs:263,313,364,383,403,439,504,508`）—— 违反字面量声明，强类型客户端解码失败 | **S1** | NEW |
| H6 | `GET /api/session` | 游标内容与 order 优先级 | v2 cursor = base64url(JSON(除 limit 外的**全部查询**))：`{search, parentID, order, directory/project/subpath, anchor{id,time,direction}}`（groups/session.ts:83-96 `withCursor` 仅 omit `limit`）；handler 用 **cursor 内容整体替换查询**（session.ts:64-69），翻页保留过滤与排序 | rust `SessionCursor` 只含 `{scope, anchor}`（`common.rs:352-405`），**search/parentID/order/subpath 丢失**；且 order 取请求参数（缺省 desc），不取 cursor 内 order（session.rs:225-271）。v2 SDK「只带 cursor.next」翻页：搜索/父过滤失效 + asc 会话列表第二页变 desc | **S1** | NEW |

> H2/H6 同根：游标/可选字段的序列化语义（null vs absent、cursor 载荷）是第一轮 OpenAPI 字节级比对**不可能**发现的层。

---

## 3. S2 — 线上行为/内容分歧

| ID | endpoint | 字段 | EXPECTED | ACTUAL | 严重度 | NEW |
|---|---|---|---|---|---|---|
| H7 | `GET /api/session/active` | data 作用域 | 进程级：返回**本进程拥有的全部**前台 drain（groups/session.ts:274-283 描述，端点无 location 参数） | rust 按 `resolve_location`（query/header→cwd）只返回**单个 location** 的 active（session.rs:426-437）—— 多项目进程下丢其他 location 的活跃会话 | S2 | NEW |
| H8 | `GET /api/session/:id/context` | data 内容 | 最新 compaction 之后的消息（core store.ts:179 → history.ts:83-90 `seq >= latestCompaction.seq`） | rust 返回**全部**消息（`MessagesInput` 无 compaction 边界，session.rs:1065-1084）—— 压缩过的会话导出前后文混合 | S2 | NEW |
| H9 | `POST /api/session` | parentID×location | `session.ts:137-139`：给 parentID → **忽略 payload.location**，在父会话 location 创建 | rust：body location 优先（session.rs:348-356），无 location 时用 cwd —— parent 在其他目录时子会话落错项目 | S2 | NEW |
| H10 | `POST /api/session/:id/agent` | 错误集 | v2 不校验 agent 存在性，直接发布事件（core session.ts:89-96）；端点错误声明仅 `SessionNotFoundError`（groups/session.ts:328-342） | rust 校验存在性，未知 agent → **404 `AgentNotFoundError`**（session.rs:511-514）—— 未声明错误变体 + 与 v2 行为相反 | S2 | NEW |
| H11 | `POST /api/session/:id/generate`、`POST /api/experimental/generate` | 503 envelope `service` | session 生成：`service:"session generation"`（session.ts:601-605）；实验端点：`service: error.service`（generate.ts:47-49） | 两处均 `service:"opencode"`（session.rs:1178、misc.rs:37-39）；session 侧 NotFound 内部错误还映射 500 `UnknownError` | S2 | NEW |
| H12 | `GET /api/permission/saved` | projectID 缺省 | `permission.ts:88-96`：缺省 = `location.project.id`（**恒按当前项目过滤**） | rust 缺省 = **返回所有项目**的行（permission.rs:68-107 无 where） | S2 | NEW |
| H13 | `POST /api/credential` | `activate` 默认值 | `credential.ts:121`：`activate = input.activate !== false || !current`（**缺省=激活**，除非显式 false 且已有凭据） | rust `unwrap_or(false)`（credential.rs:124）—— **默认值反转**：不带 activate 的创建不激活、不发 `credential.switched`、响应 `active:false` | S2 | NEW |
| H14 | `DELETE /api/shell/:id` | 幂等性 | `shell.ts:63-69`：NotFound → `Effect.void`，**204 幂等** | rust：remove 失败 → **404 `ShellNotFoundError`**（shell.rs remove） | S2 | NEW |
| H15 | `GET /api/fs/list` | 非目录 path | v2 `info.type !== "Directory"` → **die 500 `UnknownError`**（filesystem.ts:172-176） | rust `!absolute.is_dir()` → **404 `FileNotFoundError`**（fs.rs:225-227） | S2 | NEW |
| H16 | `GET /api/fs/read/*` | Content-Type | v2 = `mime-types` 扩展名库（`FSUtil.mimeType` = `lookup(p)||octet-stream`）：`.md→text/markdown`、`.svg→image/svg+xml`、无扩展名→octet-stream | rust = 魔数嗅探（PNG/JPEG/GIF/WEBP/PDF）→ UTF-8 试探 text/plain → 17 项扩展表（fs.rs:32-69,168-177）：`.md→text/plain`、`.svg→text/plain`、**无扩展名 PNG→image/png**（v2 为 octet-stream） | S2 | NEW |
| H17 | `GET /api/experimental/session/:id/export` | 消息集 + sanitize | `transfer.ts:60-62`：只导出 settled 消息（未完成 assistant/运行中 shell/compaction 剔除，:165-169）；`sanitize=true` 做**密文替换**（title/metadata/directory/revert patch/消息文本 → `[redacted:kind:id]`，:180-230） | rust：导出**全部**消息；sanitize 仅 `map.remove("metadata")`（session.rs:1738-1769）—— 「脱敏导出」仍含标题/路径/正文 | S2 | NEW |
| H18 | `POST /api/experimental/session/import` | messages 回放 | v2：按 schema 逐条回放真实消息（transfer.ts:77-88） | rust：对每条消息发布一条 **text:"" 的空 synthetic**（session.rs:1715-1734，`let _ = message;`）—— 导入的消息内容全丢 | S2 | KNOWN(07:F-4 校验/directory) 的**消息回放细化 NEW** |
| H19 | `PUT /api/experimental/session/:id/instructions/entries/:key` | 413 阈值 | v2 `MaxValueBytes = 256*1024`（schema/instruction-entry.ts），消息 `Instruction entry value is N bytes; the limit is M bytes`（core :137-140） | rust `32*1024`（session.rs:1564）—— **上限小 8 倍**，v2 合法的 100KB 条目在 rust 413；消息文案也不同（"Instruction entry value too large"） | S2 | NEW |
| H20 | `POST /api/session/:id/form` | payload.metadata / 重复 id | v2：metadata 入库并在 Info 回显（session.ts:653-671）；`AlreadyExistsError` → **409 `ConflictError{resource:id}`** | rust：`CreateInput.metadata: None` 恒丢弃（form.rs:83）；AlreadyExists → **409 `FormAlreadySettledError`**（form.rs:38）—— 错误 tag 错 + 无 resource | S2 | NEW |
| H21 | `POST /api/session/:id/permission` | payload.metadata/source/agent | v2 原样传入 `permission.ask`，后续 GET/list 回显这些字段（permission.ts:40-57；Request schema 含 metadata?/source?） | rust `AskBody` 只取 id/action/resources/save，**metadata/source/agent 恒 None**（permission.rs:171-187,249-269）—— 建出的请求缺字段 | S2 | NEW |
| H22 | `GET/POST /api/credential` | 响应字段 | `Credential.Entry = {id, integrationID, label, active, value}`（schema/credential.ts，**无 time**）；create 冲突 → `ConflictError{resource, message}`（credential.ts:20-24） | rust list/create 均多输出 **`time:{created,updated}`**（credential.rs:109,182）；冲突信封**缺 `resource`**（credential.rs:161-163） | S2 | NEW |
| H23 | `POST /:id/agent`、`/:id/model`、`/:id/command` | SSE 事件 `previous` | v2 事件带 `previous`（旧 agent/model，core session.ts:89-107；schema session-event.ts:75,86 optional） | rust 发布 `session.agent.selected`/`session.model.selected` 恒不带 `previous`（session.rs:515-536,1017-1040）—— schema 合法但官方 TUI 的“从 X 切换到 Y”展示信息缺失 | S2 | NEW |
| H24 | `PUT /api/experimental/persistent-pty/:id` | `size` 必填 | `UpdateInput.size` 必填（schema/persistent-pty.ts:66-69）→ 缺失 400 | rust `if let Some(size)` —— 缺失时**跳过 resize 直接回读返回 200**（persistent_pty.rs:171-185）。（attachmentID 丢失=KNOWN F19，此处为另一字段） | S2 | NEW |
| H25 | `POST /api/experimental/persistent-pty/:id/connect-token` | ticket scope | v2 `tickets.issue({ptyID})` **不含 location scope**（persistent-pty.ts:101-108） | rust 签发时钉死 `directory = 服务器 cwd`（persistent_pty.rs:248-252），WS connect 按请求 location 消费 —— location≠cwd 的连接**凭有效票据也 403** | S2 | NEW（静态推断，需运行时复核） |
| H26 | `POST /api/session/:id/prompt` | 409 语义与文案 | v2 唯一 409 = ID 冲突：`"Prompt message ID conflicts with an existing durable record: <id>"` + resource（session.ts:325-332）；AttachmentError→400 带 `field:"files"`、SkillNotFound→`field:"skills"`（:333-338） | rust 409 = 生命周期冲突：`"Session is currently running"`（common.rs:563-571）；附件错误无 `field`、文案不同（session.rs:559-562）；ID 冲突路径未区分 | S2 | NEW |

---

## 4. S3 — 信封细节/文案/多余字段（NEW）

| ID | endpoint | 字段 | EXPECTED | ACTUAL | 严重度 | NEW |
|---|---|---|---|---|---|---|
| H27 | message.get、revert、diff、fork；revert 家族 409 | `message` 文案 | `"Message not found: <id>"`（session-error.ts:13-19）；`"Session is busy: <sessionID>"`（session.ts:45-49） | `"Message not found"`（common.rs:77）、`"Session is busy"`（common.rs:140-142）—— 无 ID 后缀 | S3 | NEW |
| H28 | 多组 404 信封 | `message` 文案 | v2 handler 全部带后缀：`"Agent not found: <id>"`、`"Provider not found: <id>"`、`"Skill not found: <id>"`、`"Command not found: <name>"`（command.ts:84）、`"Shell command not found: <id>"`（shell.ts:40）、`"PTY session not found: <id>"`（pty.ts:52-58）、`"Permission request not found: <id>"`、`"Form not found: <id>"`、`"Integration not found: <id>"`（integration.rs 侧已对齐 ✓） | rust 通用构造器全部无后缀：`"Agent not found"`（common.rs:84）、`"Provider not found"`（:86）、`"Skill not found"`（:95）、`"Command not found"`（:101）、`"Shell not found"`（:113）、`"Pty not found"`（:110）、`"Permission request not found"`（:104）、`"Form not found"`（:107）、`"Project not found"`（:89） | S3 | NEW |
| H29 | `POST /:id/fork` | 空会话文案 | `"Cannot fork empty session: <id>"`（core error.ts:19-27） | `"Session is empty"`（session.rs:776） | S3 | NEW |
| H30 | `POST /:id/move` | 400 文案 | `"Directory does not exist: <dir>"` 等（session.ts:294-302） | 无 `: <dir>` 后缀（session.rs:681-691） | S3 | NEW |
| H31 | 所有 session 响应 | `slug` | `Session.Info` **无 slug 字段**（schema/session.ts:31-59） | rust 每个新建会话都带 `slug`（store.rs:550 写入 + common.rs:487-489 回显）—— 全量响应多一个未知字段（Effect 客户端剥离，严格客户端可能拒绝） | S3 | NEW |
| H32 | `POST …/oauth/:attemptID/complete`、`GET …/command/:attemptID` | 400/404 文案 | `"Authorization code is required"`（integration.ts:154-163）；command attempt 404 = `"Command attempt not found: <id>"`（:223-231） | `"Authorization code required"`（integration.rs:426-429）；command 404 复用 `"OAuth attempt not found"`（integration.rs:33-39,490-495） | S3 | NEW |
| H33 | `GET /api/config/shell` | `acceptable` | 真实探测值 `compatible(item)`（shell/select.ts:160） | 恒 `true`（config.rs:49） | S3 | NEW |
| H34 | `GET /api/debug/location` | 数组元素 | RcMap 键 = `Location.Ref{directory, workspaceID?}`（debug.ts:14-20） | 只输出 `{directory}`，丢 workspaceID（location.rs:70-77） | S3 | NEW |
| H35 | `POST /:id/form/:formID/reply` | `FormInvalidAnswerError.id` | 真实 formID | 解析失败分支 id=`""`（form.rs:99-102） | S3 | NEW |
| H36 | `POST /api/pty/:id/connect-token`、persistent 同 | 403 文案 | `"Invalid PTY connect token request"` / `"Invalid persistent PTY connect token request"`（pty.ts:123-128、persistent-pty.ts:93-97） | `"missing x-opencode-ticket header"` / `"origin not allowed"`（pty.rs:163-168、persistent_pty.rs:240-245） | S3 | NEW |
| H37 | `GET /api/session`、message.list | cursor.previous 存在性 | v2：data 非空则 previous/next **恒存在**（锚点分别取首/末元素，session.ts:74-100、message.ts:54-61；末页也有 next，客户端跟到空页为止） | rust：next 仅 `has_more` 时、previous 仅请求带 cursor 时输出（session.rs:294-319、message.rs:95-114）—— 「按 cursor.previous 判断可回翻」的客户端行为不同（终止条件一致，多一次空请求差异） | S3 | NEW |
| H38 | `GET /api/session/:id/diff` 等未列入项 | — | — | （context 截断见 §5；diff 响应 `{data:[FileDiff.Info]}` 形状 ✓） | — | — |
| H39 | revert 响应（潜在） | `partID` 字段名 | `Session.Revert.partID`（schema/session-revert.ts:7-8） | rust `part_id` 无 rename —— 一旦 legacy 值被写入会以 `part_id` 上线（revert.rs:189-198 序列化结构体；当前生产路径恒 None，属潜伏项） | S3 | NEW（潜伏） |

---

## 5. 未能静态确认 / 覆盖声明

- **深度走查**（双侧行级证据）：session 全家族（40 op）、message(2)、permission(7)、form(6)、mcp(6)、integration(11)、credential(5)、fs(4)、agent(2)、command(1)、skill(1)、reference(1)、config(3)、model(2)、provider(2)、generate(2)、websearch(2)、shell(5)、pty HTTP(6)、persistent-pty HTTP(11)、project(2)、worktree(4)、location(2)、debug(2)、server(4)、migration(1)、plugin(3)、rpc(1)。
- **按第一轮结论采信、未重复走查**：`/api/event` SSE 帧细节（05 §5.4）、PTY/persistent-PTY WebSocket 帧协议（05 F1-F4/F16-F20）、`/openapi.json` 与 pre-ready 面、pairing/connect 主体（05 §5.1，仅复核 JSON 形状一致）。
- **未能静态确认**（列入运行时复核清单）：
  1. `POST /api/session/:id/revert/commit` 无 staged revert 时 v2 的实际行为（rust 造出 409 `"No revert started"` 类信封，session.rs:1449；v2 端点仅声明 SessionBusy/404）。
  2. `POST /api/experimental/session/:id/wait` 的 v2 503 触发条件（rust 恒 204，10 分钟上限轮询，session.rs:1354-1370）。
  3. v2 `fs.find` 的默认 limit 与索引范围（`node_modules/.git/target` 是否入结果；rust 明确跳过这些目录，fs.rs:307，且无默认 limit）。
  4. v2 `PermissionSaved.remove` 对不存在 id 的行为（两侧疑似均幂等）。
  5. H25 的跨 location 403 场景（需双端票据联动复现）。
  6. v2 `Worktree.OperationError.forceRequired` 的具体置位路径与 rust 对应（rust 有 force_required 通道但未见脏树检测，与 09:D5 同根）。
  7. `GET /api/config` 各层 Entry 的逐字段 serde（rust `snapshot.entries` 序列化 vs v2 `config.entries()`；粗对齐，未逐字段 diff）。
  8. integration `Info.methods/connections` 内层字段逐一比对（外层形状/attempt/time/mode 已核对一致）。

---

## 6. 核对一致（负面结论，抽样列举）

- 信封注册表：`_tag` + 字段名（Unauthorized/InvalidRequest{kind,field}/Conflict{resource}/各 NotFound 家族/ServiceUnavailable{service}/Worktree 外的 413 等）与 v2 errors 逐项对应（errors.rs 全文）。
- `{data}` vs `{location:{directory},data}` vs 裸数组的端点归属：session/message/permission(session 侧)/form(session 侧)/credential/persistent-pty 为 `{data}`；agent/command/skill/reference/model/provider/websearch/mcp/shell/pty/plugin 为 `{location,data}`；project.list、worktree.list、debug.location、config.get、config.shells 为裸数组 —— 全部与 schema 一致。
- `POST /:id/interrupt` 顶层 `{interrupted}`、`POST handoff` 顶层 `{handoff:null|obj}`、`session.create` 的 `{data:PublicSessionInfo}`、`permission.create` 的 `{data:{id,effect}}`、`pty.connect-token` 的 `{location,data:{ticket,expires_in}}`、`fs.write` 的 `{location,data:{path}}`、`shell.output` 的 `{output,cursor,size,truncated}`、`PersistentPty.ReadResult/Snapshot`（含 base64 checkpoint、screen.cursor）—— 形状均与 schema 一致。
- MCP：`Status` serde `tag="status"`（含 `needs_auth`/`error`）✓；`Server{name,status,integrationID?}` ✓（integrationID 为 skip_if_none，无 H2 类 null 问题）。
- websearch 错误映射：四个 kind/field/service 值与 v2 逐字一致（misc.rs:218-242 vs websearch.ts:29-49）。
- pty create 的 TERM/OPENCODE_TERMINAL 注入与 v2 core 一致（pty.rs:411-412 ≡ core pty.ts:174-175）；v2 `PtyEnvironment` 本身是空实现。
- session 时间字段为 epoch 毫秒数（`DateTimeUtcFromMillis`）✓；ID 前缀 `ses_/msg_/per_/psv_/frm_/cred_/con_` 生成侧一致 ✓。

## 7. 修复优先级建议

1. **P0（一处小改救一族）**：H2 —— 把 `cursor.previous/next` 的 `None` 改为省略键（serde `skip_serializing_if`）+ H1/H6 游标协议对齐（cursor 携带完整查询、以 cursor 为准、补 cursor+order 互斥）。这三个共同构成「v2 SDK 可用的分页」。
2. **P0**：H3（`uriTemplate` 一行改名）、H5（`name` 恒 `"WorktreeError"`，机器码移入 data 或日志）、H4（stats 至少补齐 required 字段或按 schema 重写）。
3. **P1**：H0 —— 在 axum 层引入按 OpenAPI 生成的请求校验中间件（141 路由一次性覆盖；`schema_check.rs` 的手工复刻仅保留错误文案格式器角色）。
4. **P1**：H13（credential activate 默认反转，登录链路用户可感）、H12（saved 权限跨项目泄漏内容）、H7/H8（active 作用域、context 不过滤 compaction）。
5. **P2**：其余 S2（H9-H11、H14-T24、H26）与 S3 文案族（H27-H36 可用一个「带 ID 后缀的构造器」批量修）。

---

*本报告为只读审计产物，未修改任何产品代码。*
