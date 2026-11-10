# A1 结论: ref core/server/codemode/plugin 清单完成 — core/src 65.7k(另有 test 96.4k) + codemode 10.8k + server 4k + plugin 3.1k 非测试 LOC；17 个内建工具+execute、acorn AST 解释器+stdlib、141 endpoints、50 个 db 迁移、影子 git 快照

# opencode v2.0.24 参考侧功能清单（core/server/codemode/plugin，仅源码归纳）

## 1. core/src/tool — 内建工具全集（15 个插件文件 + codemode execute）

注册机制：`plugin/internal.ts:70-87` 注册全部工具插件；注册表/命名/权限过滤/镜像归一化在 `core/src/tool.ts`（Editor add/update/remove/namespace、`RegistrationError`、`snapshot()` 用 `whollyDisabled` 按 deny `*` 规则剔除工具、`normalizeImages`、hook before/after）。Schema→JSONSchema 转换与执行规范化在 `tool/runtime.ts`。

| 工具名 | id / options | 参数 | 要点 |
|---|---|---|---|
| `read` | opencode.tool.read, codemode:false (`plugin/read.ts:15,42`) | path, offset, limit(≤2000) | 文本分页/目录列表/图片 PDF 直出；读后向上发现并注入 AGENTS.md（read.ts:87-107）；非媒体 base64 报 BinaryFileError；sibling 备选路径回退（alternatePath） |
| `edit` | codemode:false, permission:"edit" (`plugin/edit.ts:22,123`) | path, oldString, newString, replaceAll | 唯一性校验、BOM/行尾保持（FileMutation.syncTextBom），失败重试宽容排版变体 |
| `write` | codemode:false, permission:"edit" (`plugin/write.ts:20,59`) | path, content | 覆盖写、自动建父目录 |
| `patch` | codemode:false, permission:"edit" (`plugin/patch.ts:19,81`) | patch（全文补丁文本） | 多文件 add/update/delete；输出 applied(A/M/D)+FileDiff；描述文本 `tool/patch.txt` |
| `glob` | codemode:false (`plugin/glob.ts:15`) | pattern, path, hidden, limit | 文件名 glob 发现 |
| `grep` | codemode:false (`plugin/grep.ts:15`) | pattern, path, include, literal, caseSensitive, limit | 内容搜索（ripgrep 驱动，见 §6） |
| `shell` | codemode:false (`plugin/shell.ts:22,192`) | command, workdir, timeout(默认 DEFAULT_TIMEOUT_MS, 0=无限), background | background→shellID+完成通知、禁轮询提示；shell/scan 解析与 OpaqueReason 风险标记 |
| `question` | codemode:false (`plugin/question.ts:10,58`) | questions[]（header/question/options/multiple） | 多选/单选交互（multiselect），经 permission 通道呈现 |
| `skill` | codemode:false (`plugin/skill.ts:10,42`) | id | 加载技能指令（SkillDiscovery） |
| `subagent` | codemode:false (`plugin/subagent.ts:17,104`) | agent, description, prompt, model?, sessionID?, background? | 子会话执行：Job.start/resume + SubagentJob 通知 + SubagentCompletion 回传（session/subagent-job.ts:30-55） |
| `webfetch` | codemode:false (`plugin/webfetch.ts:12,113`) | url, format(markdown), timeout(≤MAX_TIMEOUT_SECONDS), prompt/extract_main 等提取选项 | HTML→markdown（tool/html-markdown.ts，htmlparser2），llms.txt 探测 |
| `websearch` | codemode:false (`plugin/websearch.ts:12,43`) | query | WebSearch 服务多 provider + 信号量锁选择 provider（websearch.ts:14,58-68），结果以 progress 上报 |
| `list_mcp_resources` | namespace:opencode, codemode:true (`plugin/mcp-resource.ts:18`) | server? | 枚举 MCP 资源+uriTemplate |
| `read_mcp_resource` | 同上 (`plugin/mcp-resource.ts:50`) | server, uri | 读单个 MCP 资源 |
| `opencode.session_rename` | namespace:opencode (`plugin/opencode.ts:90`) | sessionID?, title | 重命名会话 |
| `opencode.session_move` | 同上, pinned:true (`plugin/opencode.ts:110`) | sessionID?, directory | 移动会话目录（steer 投递） |
| `opencode.models` | 同上 (`plugin/opencode.ts:135`) | query?, provider?, all?, limit, offset | 模型目录搜索（本 provider 优先排序） |
| `execute`（CodeMode） | 保留名（tool.ts:313-314 禁止他人占用） | code | 单一“写 JS 调 tools.*”聚合工具，catalog 渲染见 §7；非 codemode:false 的工具全部进入 catalog（tool.ts:234-254） |

辅助文件：`tool/file-diff.ts`（diff 输出结构）、`tool/read-filesystem.ts`（分页读取服务、MEDIA_MIMES、TextPage/ListPage）、`tool/http-body.ts`、`tool/runtime.ts`、`tool/html-markdown.ts`。

## 2. 会话与代理循环

- 会话服务：`session.ts`（Interface create/update/remove/get/list/messages/resume/move…，`session.ts:118-239`）；存储输入 `session/store.ts`；事件溯源投影 `session/projector.ts`(790L)；消息持久化 `session/history.ts`。
- 消息 parts 类型：`schema/src/session-message.ts` — user(text/files/agents/skills)、synthetic、system、skill、shell、assistant(content: text|reasoning|tool；snapshot/cost/tokens/finish/retry/error)、compaction(running|completed|failed, reason auto|manual)、idle(outcome succeeded|failed|interrupted)、agent-switched、model-switched、location-switched；ToolState=streaming|running|completed|error（:125-158）。
- Agent 循环：`session/runner/` — `index.ts`(编排)、`llm.ts`(LLM 客户端,375L)、`step.ts`(单步：流式 text/reasoning/tool 事件、工具执行、permission.ask、Snapshot capture、Outcome tagged enum, :32-66)、`retry.ts`(重试策略 + retry-after 钳制 RETRY_AFTER_MAX, :25-94)、`max-steps.ts`(MAX_STEPS_PROMPT)、`model.ts`、`prompt/`、`publish-llm-event.ts`、`to-llm-message.ts`。
- 生成原语：`generate.ts`（无会话文本生成；ModelSelectionError/UnavailableError）；`aisdk.ts`(1078L：SDKEvent/LanguageEvent 适配、provider 初始化)、`aisdk-native.ts`(native provider 重写、`aisdk:` 前缀)。
- 上下文压缩：`session/compaction.ts`(963L) — Trigger auto|overflow|manual（:53-57）、settings{auto, keep/recent 15000}（:197）、buildPrompt(update,legacy)（:145）、estimateContext/estimatePrompt（:868-874）、recentUserMessages（:854）；上下文加载 `session/context.ts`（SessionContext.Loaded, epoch）。
- Revert/snapshot：`session/revert.ts` stage/clear/commit（:21-76）；`snapshot.ts` — 影子 git 仓库于 `global.data/snapshot/<project>/<hash(worktree)>`，capture/files/diff/restore（snapshot.ts:92-98）。
- Fork/share/move/transfer：fork 事件 session.forked；session_share 表；`session/move.ts`(179L)；`session/transfer.ts`(336L，跨 location 迁移 + 文件描述 redact, :220-247)。
- Subagent/task：`session/subagent-job.ts`（Job type "subagent"，run=resume+取最后 assistant 文本）、`session/subagent-completion.ts`（deliver 回父会话）。
- 输入与附件：`session/prompt.ts` prepare()（materializeAttachment 并发 8，files→附件或 skill，:45-72）；注入/steer 队列 `session/inbox.ts`(550L，含 compaction 类型)。
- 其他：`session/title.ts`(自动标题)、`stats.ts`/`usage.ts`（token/成本）、`session/shell.ts`/`skill.ts`/`command.ts`（shell/skill/自定义命令消息）、`model-request.ts`+`model-transport.ts`（请求构建, 571+420L）、`provider-context.ts`、`system-prompt.ts`、`instructions.ts`+`instruction-entry.ts`+`instruction-state.ts`（系统提示组装）、`run-coordinator.ts`+`execution/restart.ts`（运行认领/重启）、`environment.ts`（会话环境变量）、`diff.ts`、`error.ts`/`to-session-error.ts`、`message-updater.ts`(487L)、`affinity.ts`。
- Agent 定义：`agent.ts`（ID/Name/Info/Color、defaultID "build"、Selection/Editor/State, :14-52）。

## 3. 配置与环境

- 根 schema：`schema/src/config.ts` 键全集 — shell, model, default_agent, update, share, enterprise, username, permissions(Ruleset), agents, snapshots, watcher, formatter, lsp, media, tool_output, mcp, compaction, skills, commands, instructions, references, websearch, plugins, worktree, warming, providers, experimental（config.ts:29-109）；Entry(document|directory)、config.updated 事件。
- 子 schema：`schema/src/config/*.ts` — agent/command/compaction/experimental/formatter/lsp/mcp/media/model/plugin/policy/provider/reference/shell/tool-output/warming/watcher/websearch/worktree。
- 发现与解析：`config/discovery.ts`（文件名 opencode.json/jsonc, :11, discover :23）；`config/normalize.ts` normalize()+Diagnostics（:57）；`config/watch.ts` plan()（:8）；`config/markdown.ts`（frontmatter 解析，agent/command md）；`config/variable.ts`（${VAR} 替换）。
- 配置→服务桥：`config/plugin/*` 21 个 ConfigXxxPlugin（agent, command, compaction, compatibility, entry-observer, formatter, image, instruction, location-watcher, mcp, policy, provider, reference, shell, skill, skill-file, snapshot, source, tool-output, websearch, worktree）。
- AGENTS.md 发现：`config/plugin/instruction.ts` — 全局 ~/.config AGENTS.md（:38）+ project directories AGENTS.md（:48）+ `fs.up` 向上遍历（:72）；read 工具读后二次注入（tool/plugin/read.ts:87-107）；注册表 `instruction-discovery.ts`；指令合并 `instructions/index.ts`（Key/Hash/Values/Delta）+ `instructions/builtins.ts`。
- 环境/工作区：`environment/`（driver 抽象 + local/memory/unavailable 实现 + files.ts + exec-defaults.ts）；`workspace.ts`+`workspace/`（workspace 域, ID/sql/driver）。
- 项目：`project.ts`(390L)+`project/`（schema.ts+sql.ts）。
- 集成与 wellknown：`integration.ts`(834L — OAuth/key/command 三种连接方式与 attempt 生命周期)；`wellknown.ts`（Auth/RemoteConfig/Manifest, :12-28）。
- 其他：`variant.ts`（reasoning variant 解析：effort/budget_tokens toggle, :16）；`state.ts`（通用 State 注册表：transform/reload/group/batch, :12-87）；`v1/`（v1 config 兼容）。

## 4. 事件与存储

- Bus：`bus.ts`(908L) — durable/ephemeral 事件 publish/subscribe、latestSequence/reserveSequence、log.synced（:21-115）；事件封装 aggregateID/seq/version（`schema/src/event.ts:33-42`）。
- 事件类型清单（`schema/src/*-event.ts`，grep 归纳）：session.created/agent.selected/model.selected/moved/renamed/metadata.updated/permissions/viewed/message.content.updated/usage.recorded/usage.updated/deleted/forked/inbox.{delivered,enqueued,cancelled,delivery.changed}/execution.{started,succeeded,failed,interrupted}/instructions.updated/synthetic/skill.activated/shell.{started,ended}/step.{started,streamed,ended,failed}/text.{started,delta,ended}/reasoning.{started,delta,ended}/tool.{called,input.started,input.delta,input.ended,progress,success,failed}/compaction.{started,delta,ended,failed}/retry.scheduled/revert.{staged,committed,cleared}；session.idle、session.status、session.compacted；mcp.status.changed/resources.changed/tools.changed、mcp.prompts.changed（core mcp/index.ts:24）；workspace.{ready,status,failed}；worktree.{ready,failed}；vcs.branch.updated；location.shutdown；installation.updated；server.connected、global.disposed；permission.asked/replied（schema permission.ts:39-53）；config.updated；instruction-discovery.updated；sdk.plugin.updated；wellknown.updated。Durable 清单：`schema/src/durable-event-manifest.ts`。
- 事件日志：`event-logger.ts`（effectDiscard layer, :9）。
- 数据库：`database/` — sqlite.node/bun/workerd 三后端、drizzle schema.gen.ts、50 个迁移文件（migration/）；表清单（migration grep 归纳）：account, account_state, control_account, credential, data_migration, event, event_sequence, instruction_blob, instruction_entry, instruction_state, kv, message, part, permission, project, project_directory, session, session_context_epoch, session_entry, session_inbox, session_input, session_message, session_pending, session_share, session_v2, todo, workspace, worktree；v1→v2 迁移 `v1-migration*.ts`。
- KV：`kv.ts`（Entry/ScanOptions/scan, :11-34）+ `kv/sql.ts`；事件存储 `event/sql.ts`。
- 权限持久化：`permission/saved.ts` + `permission/sql.ts`。
- 任务：`job.ts`(480L — start/wait/background/block、recovery kind 含 subagent, :32-115)。
- 快照/VCS：`snapshot.ts`（见 §2）；`vcs.ts`（Adapter: init/get/base/status/branch/diff, :38-56）+ `vcs/patch.ts`；`git.ts`(758L — Git.Repository, TreeID, Worktree, OperationError)。
- 文件安全：`file-access.ts`（authorizeRead/authorizeWrite、ExternalDirectoryAuthorization, :25-42）、`file-mutation.ts`（write/readText/syncTextBom, :45-49）、`file-retention.ts` cleanup()。

## 5. MCP / 权限 / 凭证 / 模型目录

- MCP：`mcp/index.ts`(846L — 服务聚合 tools/resources/prompts + 变更事件)；`mcp/client.ts`(418L — StreamableHTTP + stdio 传输、Mcp-Session-Id 处理、启动超时 :184-249)；`mcp/stdio.ts`（进程 spawn）；`mcp/oauth.ts`(532L — OAuthClientProvider：auth code + refresh token 轮转去重 :30-35、CLIENT_METADATA_URL)；`mcp/instructions.ts`；MCP→工具适配 `tool/mcp.ts`（PubSub/Semaphore/Stream）。
- 权限模型：`permission.ts` — Rule{action, resource, effect: allow|deny|ask}（schema permission.ts:58-65）、`evaluate()` 取最后匹配通配规则（core permission.ts:87）、`merge()`；ask() 产生 permission.asked/replied ephemeral 事件，Reply=once|always|reject；保存规则 `permission/saved.ts`；托管策略 `managed-policy.ts`（Console 编译的 statements，整体替换不合并）。
- 凭证：`credential.ts`(325L — Key/OAuth/Value、integrationID 关联 CRUD) + `credential/sql.ts`；OAuth 回调页 `oauth/page.ts`(276L)。
- 模型目录：`models-dev.ts`(450L — 内嵌 `models-dev/snapshot.txt` + 可选 fetch 刷新、成本/缓存价格映射 :143-184)；`provider.ts`（@opencode/ai 包 loader 表、`aisdk:` 自定义前缀 :24-49）；`model.ts`；`model-resolver.ts`(474L)；`github-copilot/`（copilot-provider.ts + models.ts 234L + openai-compatible-error.ts）；`account/`（仅 sql）；`modal/`（modal 远端模型目录映射 models.ts）。

## 6. 终端与文件基础设施（一句话+文件）

- `pty.ts` + `pty/`：伪终端 CRUD + connect-token，协议 `pty/protocol.ts`，node/bun/workerd 后端，票据 `pty/ticket.ts`。
- `persistent-pty.ts` + `persistent-pty/`：daemon 常驻终端、二进制流协议、handoff/snapshot。
- `shell/`：tree-sitter wasm 解析；`shell/scan.ts` 命令风险扫描（OpaqueReason：command-substitution/heredoc/…）；`shell/select.ts`/`result.ts`；入口 `shell.ts`(455L)。
- `formatter.ts` + `formatter/builtins.ts`：可插拔格式化器（配置 formatter 键驱动）。
- `ripgrep.ts` + `ripgrep/binary.ts`：内嵌 rg 二进制驱动 find/glob/grep。
- `filesystem.ts` + `filesystem/`：读写列举/watcher（`watcher.ts`,`location-watcher*.ts`）、fff.* 后端、`protected.ts`、`search.ts`。
- `image.ts` + `image/photon*`：photon-wasm 图片解码/缩放/限额。
- `form.ts`：交互表单 schema（ID/Field/Info，服务 :10-17）。
- `mime.ts`：magic-byte `detect()`。
- `location.ts`/`location-services.ts`/`location-service-map.ts`/`location-activity.ts`/`location-lifecycle.ts`：工作目录（Location）与服务图/活动/生命周期。
- `instance.ts` + `instance/service.ts`：每 Location 实例层图（InstancePlugins 等）。
- `app.ts`：App 元信息（name/version/useragent）。
- `rpc.ts`：插件/TUI RPC 通道（Definition via `schema/rpc.ts`）。
- `v1/`：v1 config 兼容层。
- `util/`：error.ts(NamedError)/wildcard/slug/token/which/git-executable/lazy/iife。
- `websearch.ts`：WebSearch 服务（多 provider、KV 状态）。
- `skill.ts` + `skill/`：SKILL.md 发现与 `.opencode-version` 暂存（skill/discovery.ts:112-205）。
- `worktree.ts` + `worktree/`：worktree 创建/删除/刷新，策略 `strategies.ts`。

## 7. codemode/

- `interpreter/`：acorn AST 解释器；支持的“op”=AST 节点：Array/ObjectExpression、Array/ObjectPattern、ArrowFunction/FunctionDeclaration/Expression、AssignmentPattern、AwaitExpression、Binary/Logical/Unary/UpdateExpression、Block/Expression/Empty/Labeled/If/Switch/Try/Throw/VariableDeclaration、Break/Continue/DoWhile/For/ForIn/ForOf/While、Call/New/Member/ChainExpression、Conditional、Identifier、Literal、RestElement、Return/Sequence、TaggedTemplate/TemplateLiteral、This、Yield（interpreter.ts 等 grep 归纳）；配套 scope/objects/references/promises/generators/limits(步数·深度·长度上限)/globals/intrinsics/native/callback/errors/extensions。
- `stdlib/` 全局：String, Array, Number, Boolean, Object, Math, JSON, RegExp, Date, Map/Set/WeakMap/WeakSet(+groupBy), console(log/info/warn/error/debug/dir/table), Iterator helpers, URL/URLSearchParams, Headers, TextEncoder/TextDecoder/Uint8Array(base64/hex/fromHex/toHex), crypto(getRandomValues/randomUUID), structuredClone, atob/btoa；方法面覆盖 ES 常用（charAt…toWellFormed、map/filter/reduce、sort/reverse/toSorted 等，见 string.ts/array.ts 等 methods() 注册）。
- `openapi/`：`spec.ts`（OpenAPI 文档解析：resolve/$ref、operationInput/Output、securitySchemes, :36-703）、`runtime.ts` invoke()（HTTP 调用）、`index.ts` fromSpec() → 工具集。
- `extension.ts`：宿主注入全局函数（Extension{name,globals}，值拷贝不可共享）。
- `tool-runtime.ts`：catalog prepare/searchSignature/toolExpression、Hooks 区分 ToolCall vs ExtensionInvocation、make()（:282-384）；`tool-schema.ts`：模型可见 JsonSchema 子集（不校验）；`tool.ts`/`namespace.ts`/`tools.ts`：`tools.*` 声明面。
- `codemode.ts`：run Options/ExecutionLimits；`tool-error.ts`；`data.ts`。
- core 侧：`core/src/codemode/tool.ts`（"execute" 工具 + 使用说明 catalog, :76-92）、`core/src/codemode/catalog.ts`（Inventory/Summary 渲染, summarize :54）、`core/src/codemode/instructions.ts`、`core/src/codemode/web.ts`（fetch 包装，30s 超时）。

## 8. plugin/ 包（宿主 API 面）

- 双 API：`plugin/src/effect/*`（进程内 Effect 风格：agent, aisdk, command, event, integration, mcp, model, permission, plugin, provider, reference, registration, rpc, session, shell, skill, storage, tool, vcs, websearch, worktree）与 `plugin/src/promise/*`（JS 插件 Promise 适配：同名列 + types）。
- Hook 域（`core/src/plugin/hooks.ts:11-17`）：aisdk / session / permission / shell / tool；事件名：tool execute.before（可 fail=拒绝调用）/execute.after、permission create.before、session 侧 model.request、http.request/response、experimental.ws.handshake/send/receive，及 ctx.session.hook(context|compaction|generate|title) 提供者（`plugin/effect/session.ts:139-169`）。
- 入口加载：`plugin/src/host.ts`（resolve Target→Entrypoints, load(), :17-46）、`source.ts`（bun/node/package 三源）。
- 其他宿主面：`app.ts`、`rpc.ts`、`storage.ts`、`worktree.ts`、`tui/`（TUI 插件 context/solid 绑定）。
- core 侧插件基建：`core/src/plugin/` — internal.ts(注册全部内建插件)、host/service/supervisor/instance/sdk/update/hooks、agent、command(+command/)、identity、mcp-codemode-defaults、models-dev、optimize、plan、promise、provider/、skill(+skill/)、source-directory、system-prompt/、tool-input-repair（id `opencode.tool.input.repair`, :22）、verbosity、warming、websearch/、vcs/。

## 9. server/ 包

- 路由由 effect HttpApi 定义于 `protocol/src/groups/*.ts`（共 141 个 endpoint），`server/src/routes.ts` 组装层图 + `/openapi.json`（:176），`handlers.ts` 合并 31 个 handler layer；中间件：authorization/schema-error/form-location/session-location/location。
- 认证 `server/src/auth.ts`：HTTP Basic（username 固定 "opencode" + 可选密码）；配对签发的会话令牌 `expires.HMAC-SHA256(payload)`（密钥派生自密码，改密全失效），TTL 30 天，按端口命名的 cookie（:20-60）。
- SSE `event-feed.ts`：`data: {json}\n\n` 帧，订阅队列容量 4096，溢出 SubscriberOverflowError（:11-89）。
- 配对 `pairing.ts`：16 字节 base64url code，TTL 5 分钟，一次性 consume。
- 其他：`workerd.ts`（workerd 运行时层替换）、`process.ts`（服务进程生命周期,239L）、`fetch.ts`、`options.ts`（ServerOptions：database/pty/events.persist/models/fs.filewatcher/fff/config/windows.gitbash/password/app/simulation）、`server-info.ts`、`service-status.ts`、`cors.ts`、`request-tracing.ts`、`pty-environment.ts`。

端点清单（method path — id）：
- server：GET /api/info；POST /api/pair；GET /auth/connect/:code
- debug：GET|DELETE /api/debug/location；migration：GET /api/experimental/migration/v1
- location：GET /api/location；POST /api/location/reload
- agent：GET /api/agent；GET /api/agent/:agentID
- plugin：GET /api/plugin；POST /api/plugin/check；POST /api/plugin/update
- model：GET /api/model；GET /api/model/default；provider：GET /api/provider[/:providerID]
- generate：POST /api/experimental/generate
- config：GET /api/config；GET /api/config/shell；PATCH /api/experimental/config
- fs：GET /api/fs/read/*；GET /api/fs/list；GET /api/fs/find；POST /api/experimental/fs/write
- form：GET /api/form；command：GET /api/command；skill：GET /api/skill
- rpc：POST /api/rpc/:rpcID/:method；event：GET /api/event（SSE）
- credential：GET|POST /api/credential；PATCH|DELETE /api/credential/:id；POST /api/credential/:id/activate
- project：GET /api/project；PATCH /api/project/:projectID
- mcp：GET /api/mcp；PUT|DELETE /api/experimental/mcp/:server；POST …/connect|/disconnect；GET /api/mcp/resource
- integration：GET /api/integration[/:id]；POST /api/experimental/integration/wellknown；POST …/connect/key；POST|GET|POST complete|DELETE …/connect/oauth(/:attemptID)；POST|GET|DELETE …/connect/command(/:attemptID)
- websearch：GET /api/websearch/provider；POST /api/websearch
- reference：GET /api/reference
- permission：GET /api/permission/request；GET|DELETE /api/permission/saved[/:id]；POST|GET /api/session/:sessionID/permission；GET …/permission/:requestID；POST …/permission/:requestID/reply
- session：GET /api/session[?list]；GET /api/experimental/session/stats；POST /api/session；POST /api/experimental/session/import；GET /api/experimental/session/:id/export；GET /api/session/active；GET|DELETE|PATCH /api/session/:id；POST /api/session/:id/fork；POST …/agent|/model|/move|/prompt|/command|/synthetic|/shell|/compact|/interrupt|/background|/generate；POST /api/experimental/session/:id/skill|/wait；GET|DELETE|POST|PATCH …/revert/stage|revert|revert/commit；GET …/context|/diff|/log(GET experimental)；GET|PUT|DELETE …/inbox(/:inboxID)；GET|PUT|DELETE /api/experimental/session/:id/instructions/entries(/:key)；GET|POST …/form[/:formID][/reply|cancel]；PUT …/environment；POST …/view；GET …/message(/:messageID)
- pty：GET|POST /api/pty；GET|PUT|DELETE /api/pty/:ptyID；POST …/connect-token；GET …/connect
- persistentPty：GET …/session/:id/terminal[/read]；POST …/terminal；POST /api/experimental/persistent-pty/shutdown|handoff；GET|PUT|DELETE …/persistent-pty/:ptyID；GET …/snapshot；POST …/connect-token；GET …/connect
- shell：GET|POST /api/shell；GET /api/shell/:id[/output]；DELETE /api/shell/:id
- worktree：GET|POST|DELETE /api/worktree(根路径变量)；POST …/refresh
- vcs：POST /api/vcs/init；GET /api/vcs[/base|/status|/branch|/diff]

## LOC 分布（非测试 .ts vs 测试 .ts，wc 统计）

| 包/目录 | 非测试 LOC | 测试 LOC |
|---|---|---|
| core/src（全部） | 65,749 | — |
| core/test（全部 *.test.ts） | — | 96,434 |
| — core/src/tool | 4,250 | 0 |
| — core/src/session | 10,477 | 0 |
| — core/src/config | 2,838 | 0（core/test/config=6,155） |
| — core/src/plugin | 9,348 | 0 |
| — core/src/database | 6,561 | 0 |
| — core/src/shell | 3,142 | 0 |
| — core/src/github-copilot | 4,297 | 0 |
| — core/src/mcp | 2,103 | 0 |
| — core/src/persistent-pty | 1,212 | 0 |
| — core/src/environment | 632 | 0 |
| — core/src/v1 | 602 | 0 |
| — core/src/codemode | 718 | 0 |
| — core/src/filesystem | 823 | 0 |
| — 其余 core/src 子目录 | 5,863 合计 | 0 |
| server/src | 3,961 | 0（server/test=3,877） |
| codemode/src | 10,764 | 0（codemode/test=20,075） |
| plugin/src | 3,121 | 0（plugin 包内测试 156） |
| protocol/src | 4,240 | 129 |
| schema/src | 5,693 | 1,154 |
