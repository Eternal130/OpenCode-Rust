# Parity Scan 2026-10-18 · 01 会话生命周期与 Runner

- 参考实现：`/opt/opencode-ref`（TypeScript，opencode v2.0.24，bun）
- 目标实现：`/opt/opencode-rust`（Rust 移植，workspace version 2.0.24）
- 审计范围：ref `packages/core/src/session/**`（含 `runner/`、`execution/`）+ 顶层 `state.ts / agent.ts / command.ts / generate.ts / instruction-discovery.ts / instructions/ / reference/（reference.ts 走 instructions.ts）/ skill.ts / skill/`；rust `crates/oc-core/src/session/**`、`engine/**`、`subagent.rs`、`subagent_jobs_adapter.rs`、`jobs.rs`、`bus.rs`、`bus/`，以及承接 session 生命周期的 `oc-server/src/handlers/session.rs`（调用语义）。
- 方法：逐文件映射 + 关键语义逐段比对（drain/step/publisher/compaction/retry/title/inbox/execution 逐行读，projector/message_updater 做事件覆盖面比对），并用 grep 验证调用点是否真实接线。**未跑 build/test，未改任何产品代码。**
- 数据库 schema 本身不在范围（另有 lane），但投影/commit-hook 的**调用语义**在范围内。

## 0. 总览统计

| 维度 | 数量 |
|---|---|
| ref 侧范围文件（含 runner/system.txt、execution/restart.ts、顶层 9 项） | 62 |
| 已映射到 Rust 对应文件/模块 | 58 |
| 无对应实现（整文件级） | 2（`session/model-transport.ts`、`session/execution/restart.ts` 的运行时接线）+ 1 个部分（`session/prompt.ts` 的 prepare 语义） |
| 本报告列出的开放差异 | 34 |
| — MISSING | 11 |
| — STUB | 2 |
| — PARTIAL | 2 |
| — WRONG | 15 |
| — EXTRA | 4 |
| 严重度 S0（日常主流程断裂） | **0**（无全新 S0；主 drain/step/compaction 主干可用且高度对齐） |
| 严重度 S1（常见路径不一致） | 7 |
| 严重度 S2（边缘/低频/UX 级） | 27 |
| KNOWN（已登记于 ROADMAP/FINDINGS/docs） | 9 |
| NEW（本次新发现） | 25 |

结论摘要：**runner 主干（drain 边界机、step 终局顺序 1–13、publisher 单写者、retry 退避、inbox 提升/控制重排、execution claim/release、to-llm-message 降级、system/summary 模板）是高质量逐行移植，模板逐字节一致、常量一致。** 差异集中在四类：① 生命周期外围没接线的子系统（崩溃恢复 sweep、model WebSocket transport、http/ws hook 的实际分发、prompt 插件链）；② 若干移植笔误（compaction deliver 的 too-large 免费重发死代码、手动 native compaction 重复 Started、后台 subagent 完成不 wake 父会话）；③ 与 v2 有意为之的偏离（title 竞争重试、失败回合合成 error part、Superseded 中断原因）——均已登记；④ 边缘语义（fragment state 合并、delta 批处理定时器、URL 技能源、reference guidance stub）。

---

## 1. 文件映射总表（必查项 1）

| ref 文件 | Rust 对应 | 状态 |
|---|---|---|
| session/affinity.ts | `session/runner/model_request.rs::session_affinity` | ✅ |
| session/command.ts | `engine/commands.rs` + oc-server `command` handler | ✅（command.executed 事件缺失属事件 lane，见 auditE 1.14 KNOWN） |
| session/compaction.ts | `session/runner/compaction.rs`（1501 行） | ✅（见 §6 差异 K-01/02/03/04/05） |
| session/context.ts | `session/runner/context.rs`（select/load/prepareContext） | ✅（见 §3） |
| session/diff.ts | `session/diff.rs` + server diff handler | ✅（auditE 时期的 `[]` stub 已修复；reposcan 登记的"file-level 解析"仍 TODO） |
| session/environment.ts | `session/runner/context.rs::environment_source/environment_block` + `env.rs` | ✅ |
| session/error.ts | 无独立层（字符串错误 + `ApiError`） | 结构性差异，无用户可见缺口（错误码在 to_session_error/handler 层对齐） |
| session/event.ts | `oc-schema` + `bus/registry.rs` | ✅ |
| session/execution.ts | `session/execution.rs`（SessionExecution + ExecutionHooks） | ✅（见 §8） |
| session/execution/restart.ts | **无运行时对应**：`store.rs` 有 `list_suspended/release_child_claims/count_resume`、`jobs.rs` 有 `pending_background`，但全仓库无生产调用方（仅测试） | ❌ **L-01** |
| session/generate.ts | `session/runner/generate.rs`（SessionGenerate）+ server `generate` handler | ✅ |
| session/history.ts | `session/runner/context.rs`（history_load / entries_for_runner / history_preview / first_user_message / latest_compaction） | ✅ |
| session/inbox.ts | `session/inbox.rs`（834 行） | ✅ |
| session/info.ts | `database/schema.rs::SessionRow` + handlers `session_row_to_info` | ✅ |
| session/instruction-entry.ts | `session/runner/context.rs::entry_sources`（api/<key>） | ✅ |
| session/instruction-state.ts | `session/runner/context.rs::InstructionState` | ✅ |
| session/instructions.ts | `engine/instructions.rs`（双重去重 claim+ledger） | ✅ 逐语义对齐 |
| session/message.ts | `oc-schema/session_message` | ✅ |
| session/message-updater.ts | `session/message_updater.rs`（886 行） | ✅ |
| session/model-request.ts | `session/runner/model_request.rs` | ✅（见 §4 M-01..04） |
| session/model-transport.ts（571 行） | **无对应**（`RequestInput.web_socket` 字段存在但无人消费；drain.rs:239 `modelTransport.close` 为 TODO(wave-D)） | ❌ **M-01** |
| session/move.ts | drain 的 Move 边界处理 + `engine::move_session` + server move handler | 🟡 **L-05**（Destination* 校验缺失，KNOWN auditE 1.1） |
| session/projector.ts | `session/projector.rs`（1210 行，45 个 session.* 事件全覆盖，与 ref 事件清单 1:1） | ✅ |
| session/prompt.ts | **无核心对应**：附件物化内联在 oc-server `handlers/session.rs:559-562`（materialize_files），`session.prompt` hook 与 skills 解析缺失 | ❌ **P-01/P-02** |
| session/provider-context.ts | `session/runner/context.rs`（provenance/encode/decode/upgrade_legacy_media） | ✅ |
| session/revert.ts | `session/revert.rs` + engine stage/clear/commit | ✅ |
| session/run-coordinator.ts | `session/execution.rs::SessionRunCoordinator` | ✅ |
| session/runner/index.ts | `session/execution.rs`（DrainOutcome/Continuation/DrainFn） | ✅ |
| session/runner/llm.ts | `session/runner/drain.rs`（947 行） | ✅（见 §5 T-05、P-04） |
| session/runner/max-steps.ts | `session/runner/max_steps.rs` | ✅ 逐字一致 |
| session/runner/model.ts | `session/runner/mod.rs`（ModelResolver/ResolvedModel/ModelError） | ✅（azure/google-vertex 弃用提示文案在 ModelError 中简化为通用 "Model unavailable"，S2 文案差异） |
| session/runner/prompt/system.txt | `session/runner/system_prompt.rs::SYSTEM_TEMPLATE` | ✅ **逐字节一致**（已脚本比对） |
| session/runner/publish-llm-event.ts | `session/runner/publisher.rs`（1152 行） | ✅（见 §5 T-01/02/03、E-01） |
| session/runner/retry.ts | `session/runner/retry.rs` | ✅（schedule 常量、jitter、retry-after 上限、timeout 上限、hook 覆写一致） |
| session/runner/step.ts | `session/runner/step.rs`（686 行） | ✅（终局顺序 1–13 对齐；并发映射见 §5） |
| session/runner/to-llm-message.ts | `session/runner/to_llm_message.rs`（694 行） | ✅（providerResultState 回退、effort 变体、附件 dedup、legacy media 升级均对齐） |
| session/schema.ts | `oc-schema` | ✅ |
| session/session.ts（21 个操作） | `engine/mod.rs` server-shaped ops + oc-server `handlers/session.rs` | ✅（shell/fork/transfer/generate 已修复为真实现；残留差异见 §2） |
| session/shell.ts | oc-server shell handler（真执行 Shell 服务、started/ended、用户通知 synthetic） | ✅（通知 delivery=Queue vs ref 默认 steer，S2，见 L-06） |
| session/skill.ts（SessionSkill.get） | engine skills 表 + server skill 路由（`session.skill.activated`） | ✅ |
| session/sql.ts | database schema（不审计） | — |
| session/stats.ts（436 行） | server stats handler（最小化：count + token/cost 合计） | 🟡 **X-01** KNOWN |
| session/store.ts | `session/store.rs`（get/list/messages/context/claim/release/listSuspended/countResume） | ✅ |
| session/subagent-completion.ts | `session/subagent_completion.rs` | ✅（S-01/S-02） |
| session/subagent-job.ts | `session/subagent_job.rs` + `subagent_jobs_adapter.rs` | ✅ |
| session/system-prompt.ts | `session/runner/system_prompt.rs::render/make` | ✅ |
| session/title.ts | `session/runner/title.rs` | ✅（R-01/R-05） |
| session/to-session-error.ts | `session/runner/mod.rs::to_session_error/content_filter_error` | ✅（TokenSharing 文案表一致；Tool.Error 嵌套 cause 解包未移植，S2） |
| session/transfer.ts | oc-server import/export handlers | ✅ |
| session/usage.ts | `session/runner/mod.rs::SessionUsage` | ✅ |
| state.ts（顶层） | 无统一 State 服务；各处 RwLock/settings（compaction Settings、AgentRegistry 等） | 结构性等价 |
| agent.ts（顶层） | `location.rs::AgentRegistry`（select/get/list + 默认 agent 权限种子） | ✅ |
| command.ts（顶层） | `engine/commands.rs`（CommandTable + transform 注册面） | ✅ |
| generate.ts（顶层，无会话一次性生成） | oc-server misc.rs `generate_text`（/api/experimental/generate） | ✅（auditE 的 503 判定已过时） |
| instruction-discovery.ts | `ContextDeps.discovery`（engine/mod.rs:886-920 装配，全局→项目 nearest→farthest） | ✅ |
| instructions/index.ts | `session/runner/context.rs`（read/diff/renderInitial/renderUpdate/hash） | ✅ |
| instructions/builtins.ts | `session/runner/context.rs`（date 源 + environment 源） | ✅（文案一致） |
| reference/instructions.ts（含 reference.ts 消费） | `session/runner/context.rs::reference_guidance_source` | 🟡 **C-01** STUB |
| skill.ts（顶层，Skill.Service/prepare/available） | `engine/skills.rs`（ConfigSkillTable）+ `tool/skill.rs`（Skill.prepare） | ✅（C-03 URL 源缺） |
| skill/discovery.ts | URL 源"识别但不下载"（engine/skills.rs 注释自认） | 🟡 **C-03** |

**完全没有对应实现的文件**：`session/model-transport.ts`；`session/execution/restart.ts`（文件级逻辑存在于 store/jobs 但无 sweep 接线）；`session/prompt.ts` 的 prepare/hook/skills 语义。

---

## 2. 会话创建 / 恢复 / 分叉（含崩溃恢复）

流程基准（ref）：创建走 store/projector（`session.created`）；执行认领（claim）在 `execution.started` 的 commit-hook 里落库；终态 release；崩溃后由 `execution/restart.ts::resumeSuspendedSessions` 扫描孤儿 claim → `countResume` 预算（默认 10 次）→ 注入 `CONTINUE_AFTER_SERVER_RESTART` synthetic → resume；后台 shell/subagent 通知按 `pendingBackground` 补发（shell 先于 subagent）；`releaseChildClaims` 清理不可恢复子会话认领。分叉走 projector 的 `session.forked`（settled-copy + fork 标题 + 指令基线种子）。

| ID | 标题 | 类型 | 严重度 | ref | rust | 差异（EXPECTED vs ACTUAL） | 用户可见影响 |
|---|---|---|---|---|---|---|---|
| L-01 | 崩溃/重启恢复 sweep 未接线 | MISSING | **S1** | session/execution/restart.ts:192-231（`resumeSuspendedSessions`：listSuspended + releaseChildClaims + pendingBackground 补发 + prepareResume 预算 + CONTINUE_AFTER_SERVER_RESTART） | `store.rs:350-433`、`jobs.rs:561` 具备全部原语，但 grep 全仓库（oc-core engine + oc-server boot）**无任何生产调用方**（仅测试） | EXPECTED：受管服务器 boot 时恢复孤儿回合并补发后台通知；ACTUAL：claim 永不释放、孤儿 turn 不恢复、`resume_attempts` 永不计数、后台 shell/subagent 完成通知在重启后丢失 | kill -9/重启后：会话看起来"卡住"（time_suspended 残留影响列表/恢复语义），正在运行的回合静默消失，后台任务结果永不送达。注：docs/auditE-core.md §1.3 将此项标 ✅（依据是 drain 的 stale-settle），属审计误标 → **NEW** |
| L-02 | `engine.prompt`（插件桥 `ctx.session.prompt` 使用）绕过 prepare 且阻塞 | WRONG | S2 | session/session.ts:145-178（prompt= prepare(hook+文件+技能)→admit→**wake** 立即返回）；prompt.ts:29-91 | engine/mod.rs:1134-1161：裸 `inbox.admit(UserPayload{text})` + `execution.resume()`（阻塞 join 整个回合） | 插件发起的 prompt：无 session.prompt hook、无附件/技能、无 staged-revert 提交，且调用方被整个 drain 阻塞 | 插件驱动的对话注入走"降级路径"；长回合期间插件 host 调用长时间不返回 |
| L-03 | 新输入不提交 staged revert | MISSING | S2 | session/session.ts:165（`if (session.revert) yield* SessionRevert.commit(...)`，prompt 前）；:251（compact 前） | oc-server handlers/session.rs:543-586（prompt）、646-668（compact）均无 revert 提交；synthetic 仅在 wake 条件里读 `row.revert.is_none()`（:616） | EXPECTED：用户在 staged revert 状态下发新 prompt/compact 时先落 revert 再 admit；ACTUAL：staged revert 悬置，历史仍含被回退范围的消息，直到手动 commit/clear | revert 工作流（TUI/undo）中继续对话时模型看到"将被回退"的内容，revert 语义被绕过 |
| L-05 | move 的目的地校验/错误面缺失 | PARTIAL | S2 | session/move.ts（Destination* 错误族、project 解析失败面） | engine/mod.rs:1227-1254 只 admit `Move_` 项 | KNOWN（auditE 1.1 🟡）：drain 边界执行 move 本身对齐（drain.rs:288-321 InboxDelivered+Moved 顺序一致） | 非法 move 目的地错误码/文案不一致 |
| L-06 | shell 用户通知 synthetic 的 delivery | WRONG | S2 | session/session.ts:213-219（`synthetic(..., resume:false)` → delivery 默认 **steer**） | handlers/session.rs:1310 前后 `Item::Synthetic{delivery: Queue}` | steer：运行中回合在下一边界即看到命令结果；Queue：要等 idle 边界 | 用户 `!cmd` 的结果注入时机在"回合运行中"场景滞后一个回合 |
| — | fork 服务器边界 bug | （已修复） | — | projector.ts:144-230 | handlers/session.rs:825-876 + projector.rs:571 | KNOWN（auditE core #3 / CD-15）：当前代码已按 `{type,messageID}` 边界、不预建行、走 location envelope 发布 | 无（列出以闭合 KNOWN 项） |

恢复/claim 正向确认：`execution.started` 的 claim commit-hook（null-guard 幂等 + `time_updated` 钉住）、终态 release（`resume_attempts=0`）、shutdown 中断保 claim、`settled` 每忙碌期一次终态观测——与 execution.ts:81-148 一致（execution.rs:786-1018）。

---

## 3. Prompt admission / 指令与技能注入

流程基准（ref）：`Session.prompt` → `SessionPrompt.prepare`：① `Plugin.awaitActivation` ② `session.prompt` hook（可改写 text/files/agents/skills/metadata/delivery）③ 并发 8 物化附件（20MiB 上限、data-URL 校验、目录列举、行切片、图片 normalize）④ skills 附件逐个 `Skill.get`+`Skill.prepare` 填 `{id,name,text,mention}`（未知技能→SkillNotFoundError）→ `admission.admit` → `execution.wake`。

Rust：物化在 oc-server handler 内联完成（`materialize_files`，AttachmentError 语义保留）；**其余步骤缺失**。

| ID | 标题 | 类型 | 严重度 | ref | rust | 差异 | 用户可见影响 |
|---|---|---|---|---|---|---|---|
| P-01 | `session.prompt` hook 在真实路径从不触发 | MISSING | **S1** | prompt.ts:38-51（每次 admit 前 trigger，可改写 prompt 全字段） | hooks.rs:350/860 已实现注册与 dispatch，但 `dispatch_session_prompt` 全仓库唯一调用点是 location_services.rs:643 的**测试**；handlers/session.rs:543-586 直接 admit | 插件（如自审查/脱敏/路由类）对 prompt 的改写完全失效 | 安装了 prompt hook 的用户插件行为静默不同 |
| P-02 | prompt 附带 skills 不解析（无 text） | MISSING | **S1** | prompt.ts:56-76（`Skill.prepare` 填 text；SkillNotFoundError） | handlers/session.rs:567 原样存 `typed_value(&body,"skills")`；projector.rs:208 仅 clone；to_llm_message.rs:348-353 只注入 `skill.text`（永远为 None） | 模型永远收不到 prompt 附带技能的指令正文；未知技能也不报错 | `@skill` 附带调用（TUI/客户端技能提及）静默无效——技能正文不进上下文 |
| P-03 | 附件图片 normalize/resize（photon）缺失 | MISSING | S2 | prompt.ts:135-149（image.normalize，ResizerUnavailable 降级） | 附件原样 base64 透传 | KNOWN（auditE 1.17 ❌） | 超大图片直接进请求（配合 boundImages 只在超 25MiB 才裁） |
| P-04 | `plugins.awaitActivation` 门槛缺失 | MISSING | S2 | llm.ts:67（drain 入口 awaitActivation）；prompt.ts:38 | drain.rs:179 `// plugins.awaitActivation — TODO(plugin-lane)` | 插件注册的 agents/tools 可能在首个回合尚未生效 | 首回合插件工具缺失的竞态窗口 |
| C-01 | reference-guidance 指令源是 stub | STUB | S2 | reference/instructions.ts:62-91（可用列表渲染 `<available_references>`，增删 delta 文案，removed 文案） | context.rs:620-636：read 恒 `Removed`、initial 固定 "No reference files are currently loaded..."、removed=None | KNOWN（reposcan-findings TODO(wave-C)） | 配置了 references 的项目：模型完全不知道参考目录存在 |
| C-02 | skill-guidance 的 changed 文案不走增量 diff | WRONG | S2 | skill/instructions.ts:35-58（added/removed 小 delta；仅同项变更才全量重述，且带 "The available skills have changed...supersedes" 引导句） | context.rs:709-712：changed 恒渲染全量 render(current)，无引导句 | 技能列表变化时注入的 synthetic 文案与 v2 不同（更啰嗦、缺 supersede 语义句） | 长会话中技能变更的上下文更新文案漂移；对模型语义有轻微影响 |
| C-03 | 远程技能源（URL index.json 下载）缺失 | MISSING | S2 | skill/discovery.ts:66-120（pull：index.json、安全路径校验、缓存目录、重试） | engine/skills.rs 头注 "URL sources are recognized but not fetched" | KNOWN-ish（代码内自认；docs 未单列）→ 计 NEW | 配置 URL 技能源的用户拿不到技能 |
| C-04 | select 不做 `mcpTools.flush` | MISSING | S2 | context.ts:127（select 先 flush MCP 工具注册） | context.rs:1445-1476 无对应 | MCP 工具异步注册的 flush 屏障缺失 | 极窄竞态：新连 MCP 工具未进本回合工具快照 |

对齐确认：指令源组装顺序 codemode→mcp→references→skills→discovery→date/env→api-entries 与 context.ts:150-158 一致；`Instructions` hash/diff/render/Unavailable/Removed 哨兵、blob 500 批读、epoch 基线与 advance 语义一致；AGENTS.md 注入（engine/instructions.rs）双重去重与 ledger 语义逐条对齐。

---

## 4. Model request / transport / hook 面

流程基准（ref）：`SessionModelRequest.prepare` 四种 kind 各自跑 hook（context/compaction/generate/title）→ 工具按身份匹配回改名 → 输出限额（fallback 32k、cap 256k、summary 32k、estimate+15%、min 1024）→ affinity/UA/头集 → promptCacheKey（`ses_[0-9a-f]{64}` 剥前缀）→ `boundImages(unsupportedParts(...))` → `session.model.request` hook（baseURL/headers）→ ProviderContext 兼容 die 检查 → http hooks 包装（`session.http.request/response`）→ 可选 per-session WebSocket transport（含 `ws.handshake/send/receive` hooks、chunkTimeout）。

Rust `model_request.rs::prepare`：kind hook、身份匹配、限额、头集、boundImages/unsupportedParts、model.request hook、promptCacheKey 均对齐（含 GENERATION_KEYS 拆分）。差异：

| ID | 标题 | 类型 | 严重度 | ref | rust | 差异 | 用户可见影响 |
|---|---|---|---|---|---|---|---|
| M-01 | per-session WebSocket model transport 整体缺失 | MISSING | **S1** | session/model-transport.ts（571 行：bind/close、握手与帧 hook、chunkTimeout 心跳）；model-request.ts:351-375；llm.ts:76/96（drain 关闭点） | `RequestInput.web_socket` 字段无消费者；drain.rs:239 TODO(wave-D) | 支持 WS 传输的 provider/gateway：请求永远走 HTTP；drain Reloaded/Moved 边界不关传输 | 仅影响配置了 websocket transport 的 provider；但 `modelTransport.close` 缺失也意味着 Move/Reload 边界的连接清理语义缺失 |
| M-02 | `session.http.request/response`、`ws.*` hook 注册但从不分发 | STUB | **S1** | model-request.ts:320-375（StreamOptions.http 包装所有 HTTP 请求，含 WS 回退路径） | hooks.rs:872-876 有完整注册面；`dispatch_session_http_request/response/ws_*` 无任何请求路径调用（`LlmClient::stream(request)` 无 options 参数） | auditE 1.6 "18/18 ✅" 只验证了注册面；实际分发面 4 个 hook 家族死置 | 计费代理/遥测/重写类插件（依赖 http hook）完全无效 |
| M-03 | model.request hook 改写路由后的 ProviderContext 兼容检查缺失 | MISSING | S2 | model-request.ts:307-318（hook 后 provenance 不兼容且消息含 compaction part → die） | model_request.rs:645-659 只做 baseURL/headers 应用 | 路由 hook 把带原生压缩窗口的会话指向异构端点时不会拒绝 | 罕见：原生 compaction + 路由 hook 组合下静默发出不可回放的窗口 |
| M-04 | promptCacheKey 大小写 hex 差异 | WRONG | S2 | model-request.ts:278（`/^ses_[0-9a-f]{64}$/`） | model_request.rs:612-621（`is_ascii_hexdigit` 接受 A-F） | 大写 hex 会话 id 会错误走剥前缀分支 | 边缘（id 生成器产小写）；仅极端手工数据 |
| M-05 | 无工具时的 executeTool 错误文案 | PARTIAL | S2 | model-request.ts:225-228（"Tools are not available for this request"） | model_request.rs:716-723（"No tool named ... use a tool from the available tool list"） | title/generate 请求被模型幻觉出工具调用时的纠偏文案不同 | 极低频；文案差异 |

---

## 5. 工具调用循环（step / publisher / 并发取消）

流程基准（ref）：step.attempt —— 起始快照 → publisher（单写者不变量：先标记后 await）→ 流消费（overflow 预首 token 停车；Tool.Called 发布完成后才 fork 本地工具 fiber；toolChoice none → Tool.Error）→ 不可中断终局区：streamed→interruptTools→join(restore)→classify→overflow 恢复→publish(overflowFailure)→unknownFinish→RecoverFull→retry 决策（未开播则 Retry+startAssistant）→failAssistant→declines（location closed 文案区分）→TOOLS/STEP_INTERRUPTED→RESULT_MISSING(hosted)→快照/费用→Step.Ended/Step.Failed→Continue 出口→failCause 重抛。

Rust step.rs 逐条对应该顺序（含 `TOOL_JOIN_TIMEOUT=5s` 的 wedge 防护、bell-latch 中断、`biased` select）。差异：

| ID | 标题 | 类型 | 严重度 | ref | rust | 差异 | 用户可见影响 |
|---|---|---|---|---|---|---|---|
| T-01 | step 循环不接 publisher.progress（工具运行中进度不进 record） | WRONG | S2 | step.ts:96（`progress: (update) => publisher.progress(call.id, update)`）；publish-llm-event.ts:343-354（failTool 用 `tool.progress` 给 subagent 错误加 `(sessionID: …)` 后缀） | step.rs 无 progress 通道（`ExecuteTool` 签名无回调）；subagent 工具的 progress 由 engine 直发**ephemeral** `session.tool.progress`（engine/mod.rs:1715-1731），不进 `ToolRecord.progress` | 中断扫尾 `failUnsettledTools(TOOLS_INTERRUPTED)` 时，运行中 subagent 的失败消息缺 `(sessionID: …)` 后缀；durable 元数据路径不受影响 | 中断时 TUI 的 subagent 卡片少 child 会话指针；与 v2 错误文案不一致 |
| T-02 | fragment state 合并语义（end 覆盖 vs 合并） | WRONG | S2 | publish-llm-event.ts:161/180-190（append 时 `state={...acc,...new}`；end 时 `{...acc,...end}`） | publisher.rs:122-124/183（append/end 均**整体替换**） | Gemini thought-signature 等逐 delta 累积 state 的 provider：end 事件只带部分键时，之前累积键被清掉 | 特定 provider 的签名回放丢失 → 可能引发 provider 拒绝重放 |
| T-03 | delta 批处理无定时器（事件驱动 flush） | WRONG | S2 | publish-llm-event.ts:162-177（fork timer：pending 100ms 后即发布，即使无新 chunk） | publisher.rs:131-170（仅下一事件到达/fragment start/flush 时检查到期） | provider 流中途停顿 >100ms 时，最后一批 delta 被扣住到下一事件 | 流式打字机观感在慢模型上延迟一批；终态一致 |
| T-05 | drain 步进游标对 steer 型 compaction 的重置 | WRONG | S2 | llm.ts:106-109（`!continuing && pending?.delivery !== "steer"` —— 对**任意类型**的 steer 交付不重置 entering/step） | drain.rs:328-339（steer_delivered 只匹配 User/Synthetic；Compaction{delivery:Steer} 会触发 `entering=true, step=1`） | 手动 compaction（默认 steer）边界的 step 计数被重置 | 仅影响 agent steps 上限的计数位置；S2 边缘 |
| E-01 | Step.Started 与 pending delta 的先后 | WRONG | S2 | publish-llm-event.ts:407-415（`assistantMessageID: yield* startAssistant()` 惰性求值——pendingDeltas 之后才发 Step.Started） | publisher.rs:570-577（先 `start_assistant` 再 publish_pending_delta_batches 再 Text.Started） | 首个 text block 前的事件顺序：Rust `Step.Started→Text.Started`，ref `Text.Started 参数求值时机` 下同为 Step.Started 先行——实际差异仅当 step 尚未开始且有未 flush delta（不可能同时成立） | 理论性；列出备查 |

并发/取消语义正向确认：interrupt=shutdown watch（信号而非 future drop）+ 不可中断终局区 + settle 后重抛；decline（含 question 取消文案透传 `failure.message`）、`is_location_closed` 的 "location shut down" 文案、`RESULT_MISSING` hosted 范围、`record.needsContinuation`（非 providerExecuted 且 called/settled）均一致。UNWIND_TIMEOUT=60s 与 TOOL_JOIN_TIMEOUT=5s 是 Rust 侧新增的 wedge 防护（合理增强，非差异语义）。

---

## 6. Compaction（自动/手动/overflow/native、hook 短路）

流程基准（ref）：trigger(reason auto/overflow/manual) → auto 关闭则跳过 → due 估算（锚点 usage+估算；刚完成 compaction 不 due；未测量原生窗口不 due）→ native or summarize（compaction agent 模型覆盖）→ ceiling/UNKNOWN_WINDOW/overflow 0.7 首缩 → deliver：manual 不发 Started（runner 已发）；hook result 短路；重试策略；too-large(未改写请求)→免费 as-text 重发；too-large/overflow→70/50/35 阶梯；其他错误走 retry 策略；流式 delta/usage 即时计费；publish Ended/Failed；onInterrupt 发 compaction.interrupted；ensuring 清 spent。

Rust compaction.rs 是完整移植（模板/规则/NUDGE/LEGACY_HEADING 逐字，split/recentStart/oldestToDrop/flatten/drop-oldest/estimate 族逐语义）。差异：

| ID | 标题 | 类型 | 严重度 | ref | rust | 差异 | 用户可见影响 |
|---|---|---|---|---|---|---|---|
| K-01 | deliver 的 payload-too-large 免费文本重发永不触发（死代码） | WRONG | **S1** | compaction.ts:430-432（`tooLarge && request === prepared.request` → `asText=true` 不计次重发） | compaction.rs:525/541：`sends += 1` 在 send **之前**执行，`too_large && sends == 0` 恒为假 | 首个未改写请求被 413 时：v2 免费转文本重发；Rust 直接消耗一个 70% 收缩阶（甚至最终失败） | 大附件会话的手动/溢出压缩成功率下降、多花 1-2 次模型调用 |
| K-02 | 手动 native compaction 重复发布 Compaction.Started(reason=auto) | WRONG | S2 | compaction.ts:393-395（deliver 收 trigger，manual 分支跳过 Started）；compactNatively 传原 trigger | compaction.rs:463：`self.deliver(Reason::Auto, ...)` 硬编码 Auto | manual+native：runner 已发 InboxDelivered+Started，deliver 又发一条 auto Started → 双 running compaction 消息投影 | 手动原生压缩的事件流多一条假 auto 开始（消息流出现两条 compaction 行为异常） |
| K-03 | `session.compaction` hook 短路结果（fromHook）缺失 | MISSING | S2 | compaction.ts:396/594-608（`prepared.event.result` 直接产出 summary+usage） | compaction.rs:495-496 `TODO(wave-C)` | KNOWN（reposcan-findings 已登记） | 提供压缩代理的插件无效（退回真模型压缩） |
| K-04 | 取消的压缩不发 compaction.interrupted、spent 泄漏 | WRONG | S2 | compaction.ts:664-672（onInterrupt→非 manual 发 `{type:"compaction.interrupted"}`；ensuring 清 spent） | compaction.rs:220-224（只在正常完成路径 `spent.lock().clear()`；drop/取消路径无 onInterrupt 等价） | 中断的 auto 压缩：不立即发 Failed(interrupted)（靠下次 drain 的 stale-settle 兜底）；已计费 usage 残留 map，**下一次**同会话压缩的 Ended/Failed 会把旧账重复计入 | 中断后立即的下一次压缩 usage 虚高；事件时序上 interrupted 延后 |
| K-05 | `/compact` 默认 delivery：queue vs steer | WRONG | S2 | session/session.ts:246-258（`input.delivery ?? "steer"`；server 原样透传可选字段） | oc-server handlers/session.rs:653-656（默认 Queue）；engine/mod.rs:1383（显式 Queue） | 回合运行中发 /compact：v2 在下一边界即压缩（steer）；Rust 排队等回合结束 | TUI/API 手动压缩在忙时会话上的生效时机推迟一整个回合 |
| K-06 | native 路径不传 webSocket="session" | MISSING | S2 | compaction.ts:333（`prepare(..., "session")`） | compaction.rs:391/608（web_socket:false） | 与 M-01 同根 | 同 M-01 |

正向确认：due/calculateCeiling（10% 或 RESERVE_MIN=16k、buffer 覆盖、未知窗口 200k）、SHRINK_STEPS、summary 模板填充校验+NUDGE 一次、estimatePrompt 锚点算法（含 boundImages/unsupportedParts 复跑）、native trigger 的 recentUserMessages+checkpoint 组窗、spend 即时计费+UsageRecorded(source=compaction)、publish 字段集（model/providerState/providerContext/metadata）一致。

---

## 7. Retry / 错误分类 / Title

| ID | 标题 | 类型 | 严重度 | ref | rust | 差异 | 用户可见影响 |
|---|---|---|---|---|---|---|---|
| R-01 | title 乐观并发败者重试 ≤20 次 | EXTRA | S2 | title.ts:135-147（序列竞争失败 → 吞掉缺陷直接放弃） | title.rs:258-305（≤20 次、50ms 退避、行守卫重检） | KNOWN（ROADMAP 2026-10-11 wave F #27，修 "never titled" 缺陷的有意增强） | 正面差异：标题不再因并发事件丢命名 |
| R-02 | `InterruptReason::Superseded` | EXTRA | S2 | execution.ts:48（reason 仅 user/shutdown/inactivity） | execution.rs:838（多一个 "superseded"） | 枚举多一员；SSE 消费端可能见到 v2 不存在的 reason 值 | 极低频；wire 枚举超集 |
| R-03 | 失败回合合成 step.started+step.failed + stderr 行 | EXTRA | S2 | execution.ts:118-147（pre-step 失败只发 Execution.Failed，不造 assistant 消息） | execution.rs:849-942（未 surfaced 失败 → 伪造 v2 形状 error part + 必打日志行） | KNOWN（ROADMAP B4-H2，beta.4 有意落地"surfacing turn failures"） | 正面差异：模型解析失败等 pre-step 错误在 transcript 可见 |
| R-04 | drain 完成后才发现 stopping 的分类 | WRONG | S2 | run-coordinator.ts:74-85（drain 成功返回后 stopping → 以 success 出口收束） | execution.rs:446-469（同样情形强制 `Err(Interrupted)`） | TS 在该竞态下可能记 succeeded；Rust 恒记 interrupted | 极窄竞态；终端事件 reason 分布不同 |
| R-05 | title 置空再生（`""` → LLM 重生成） | MISSING | S2 | server PATCH "" → title regen | handlers/session.rs:468-476（TODO(phase-9)，仅改名） | KNOWN（reposcan/auditE） | 清空标题不触发重命名 |
| R-06 | `wait_usage_fanout` 终态前屏障 | EXTRA | S2 | （无——依赖 Effect 语义天然顺序） | execution.rs:820（settled 前 await usage 扇出） | KNOWN（ROADMAP GATE4/DETERM：保证 usage.updated 严格先于 terminal） | 正面差异：SSE 顺序确定化 |
| R-07 | to-session-error 的 Tool.Error 嵌套 cause 解包 | PARTIAL | S2 | to-session-error.ts:63-69（cause.error 存在时递归解包保留其 type） | runner/mod.rs::to_session_error 只映射 AIError；工具错误在 step.rs 固定 "tool.execution"/"permission.rejected" | 工具以结构化 SessionError 失败时，v2 保留子错误类型（如 provider.auth） | 工具内 provider 错误被降格为 tool.execution，模型看到的错误类别变粗 |

Retry 正向确认：2s 指数、10s 封顶、10 次、jitter ±20%、retry-after 取 max 且 15min 封顶、timeout 类 3 次上限、hook 可覆写 decision（含强制 retry）、`RetryScheduled` 事件字段、per-runStep 策略生命周期（Rust 注释明确修复了 per-attempt 重建 bug）——全对齐。

---

## 8. 执行协调 / abort / interrupt / 事件发布顺序

正向确认（无差异或已列入上表）：
- doorbell/`pendingWake` 最宽 scope 合并、settle 时幸存门铃先起后继再放 waiter（execution.rs:487-507 ↔ run-coordinator.ts:116-120）；
- interrupt 的 settlement-window 语义（`loop_done` ↔ `owner===undefined`：只认领 wake 不挂 reason）；
- `interrupt(resume=true)` 只对 steer/compaction/move 唤醒（execution.rs:658-687 ↔ execution.ts:153-170）；
- wrap_drain 的 Moved/Reloaded 递归再 drain（↔ execution.ts:105-110）；
- `advanceToStep` 内 `SessionInbox.serialized` 包住 peek+eager publish（compaction 的 InboxDelivered+Started、move 的 InboxDelivered+Moved 顺序一致）；
- `settleStaleCompactions`（running→compaction.interrupted，最新优先）与 `settleStaleToolCalls`（streaming/running→aborted，subagent 元数据带 sessionID，executed 透传）逐字段对齐；
- bus：durable 事件提交序=可见序、projector/commit-hook 同事务、ephemeral/durable 双通道——与 bus.ts 结构等价（含 `InvalidDurableEvent`→conflict 隧道）；
- projector 45 个 `session.*` 事件与 ref 投影清单 1:1（脚本比对）。

---

## 9. Subagent / task / todo

| ID | 标题 | 类型 | 严重度 | ref | rust | 差异 | 用户可见影响 |
|---|---|---|---|---|---|---|---|
| S-01 | 后台 subagent 完成投递后不 wake 父会话 | MISSING | **S1** | subagent-completion.ts:31-43 经 `Session.synthetic`（session.ts:310 `execution.wake`） | subagent_completion.rs:42-66 只 `inbox.admit`（steer）；全链路（deliver→notify→jobs）无 wake 调用；jobs.rs 亦无 | 父会话空闲时后台结果滞留 inbox，直到用户下一条输入才被消费 | `run_in_background` 的任务"完成了但父会话永远不反应"——后台 subagent 主流程断裂面 |
| S-02 | completion 文本过滤更严格 | EXTRA | S2 | subagent-completion.ts:10-19（任意 assistant 的 text parts） | subagent_completion.rs:93-115 / subagent.rs:480-514（要求 completed 且无 error，跳过未完成继续找） | 与 ref job body 的 findLast(completed&&!error) 行为一致、与 deliver 侧 text() 不一致——Rust 两处统一加严 | 极端时返回 "Subagent completed without a text response." 而 v2 会取未完成文本；更安全 |
| — | 深度守卫/agent 解析/模型解析四类错误文案/`Available subagents` 描述注入/block+interrupt 链/{child}:{startedAt} 代际去重/notification 幂等 admit | ✅ | — | subagent.ts/subagent-job.ts | subagent.rs/subagent_job.rs | 逐条比对一致（四条模型错误文案逐字、depth "Parent session not found"、primary 拒绝文案） | — |

todo：ref 侧会话范围内无 todo 专门文件（属工具 lane）；runner 侧仅涉及工具运行元数据（已覆盖，见 T-01）。

---

## 10. KNOWN 清单（本次复核确认仍开放）

| 编号 | 内容 | 登记处 |
|---|---|---|
| L-05 | move 目的地校验/错误面 | docs/auditE-core.md §1.1 🟡 |
| P-03 | 附件图片 normalize（photon） | docs/auditE-core.md §1.17 ❌ |
| C-01 | reference guidance stub（TODO(wave-C)） | docs/reposcan-findings.md:56 |
| K-03 | compaction hook 短路（TODO(wave-C)） | docs/reposcan-findings.md:56 |
| R-01 | title 竞争重试（有意） | ROADMAP 2026-10-11 wave F（#27） |
| R-03 | 失败回合 error part + 日志行（有意） | ROADMAP B4-H2 |
| R-05 | title "" 再生 TODO(phase-9) | docs/reposcan-findings.md:50 |
| X-01 | stats 最小化（binned TODO(phase-9)） | docs/reposcan-findings.md:50、auditE §1.1 |
| R-06 | usage fan-out 屏障（有意） | ROADMAP GATE4 / DETERM |

审计过时项（本次确认已修复，无需再列开放项）：fork 服务器边界 bug（auditE core #3，现为 `{type,messageID}`+不预建行+envelope 发布）；shell 走 LLM 的 ⚠️（现真执行 Shell 服务）；generate 503（现双路实现）；diff `[]`（现 turn-range 实现）。

---

## 11. 覆盖清单

**已逐行/逐段比对的文件对**（ref ↔ rust）：
session/runner/llm.ts ↔ runner/drain.rs；step.ts ↔ runner/step.rs；publish-llm-event.ts ↔ runner/publisher.rs；retry.ts ↔ runner/retry.rs；model.ts ↔ runner/mod.rs（ModelResolver/usage/to-session-error 部分）；to-llm-message.ts ↔ runner/to_llm_message.rs；max-steps.ts+system.txt ↔ max_steps.rs/system_prompt.rs（模板字节级比对）；model-request.ts ↔ runner/model_request.rs；context.ts ↔ runner/context.rs（含 instruction-state/history/provider-context 合并体）；compaction.ts ↔ runner/compaction.rs；title.ts ↔ runner/title.rs；generate.ts ↔ runner/generate.rs；session.ts ↔ engine/mod.rs+oc-server handlers；prompt.ts ↔ oc-server prompt handler（差异见 §3）；inbox.ts ↔ session/inbox.rs；execution.ts+run-coordinator.ts ↔ session/execution.rs；store.ts ↔ session/store.rs；instructions.ts ↔ engine/instructions.rs；subagent-job.ts/subagent-completion.ts ↔ subagent_job.rs/subagent_completion.rs（+subagent.rs、subagent_jobs_adapter.rs、jobs.rs 的 seam 面）；skill/instructions.ts、reference/instructions.ts ↔ context.rs 指令源；顶层 agent/command/generate/instruction-discovery 装配点。

**事件覆盖面比对**：projector.ts ↔ projector.rs（45 事件清单 diff）；message-updater 仅做事件↰处理面核对（未逐行，双方均有大量单测锚定）。

**未覆盖/浅覆盖及原因**：
- `session/sql.ts`、数据库 schema/migrations——按任务书排除（另有 lane）。
- `message-updater.ts`（487 行）与 `projector.ts`（790 行）的**每个事件分支内部逻辑**未逐行比对（只做了事件清单+关键分支抽查：inbox delivered、step.failed、fork、instructions delta）；建议另开 lane 做"投影字段级"比对。
- `session/diff.ts` ↔ `diff.rs`、`revert.ts` ↔ `revert.rs`：只验证了对外签名/调用点接线（diff 的 turn 边界选择、revert 的 stage/clear/commit 调用语义），内部 diff 算法未逐行（reposcan 已登记 file-level 解析 TODO）。
- `bus.ts` ↔ `bus.rs`：结构语义（提交序、事务、路由）比对，未逐行核对 publishAll 批量插入路径的每处细节。
- `state.ts`（顶层）为 Effect 结构性设施，Rust 无对应统一层，仅确认各消费者（compaction settings/agents/commands）具备等价 transform 能力。
- todo/task 工具内部（非会话范围）；`engine/tests_*`、`session/tests_*` 测试文件不参与比对。

**建议优先修复顺序**（按用户可见度）：S-01（后台 subagent 不 wake）→ K-01（too-large 免费重发死代码）→ L-01（restart sweep 接线）→ P-01/P-02（prompt hook+skills 解析）→ M-01/M-02（WS transport + http/ws hook 分发）→ 其余 S2。
