# 第十九轮复核 V-B（AI/provider）— 2026-11-09

> 对 17 通道审查（docs/audit/L1-L17）产出候选逐项现行源码双侧复核，与登记册 G-1..G-290 去重。

## 裁定总览
1 RETRACT / 2 NEW-CONFIRM 🟡 / 3 RETRACT / 4 NEW-CONFIRM 🟢 / 5 NEW-CONFIRM 🟢（含 6 合并）/ 6 并入 5 / 7 RETRACT / 8 RETRACT / 9 RETRACT / 10 RETRACT / 11 ℹ️ 登记 / 12 RETRACT / 13 RETRACT

---

**1. Copilot anthropic 车道 anthropic-beta 头 → RETRACT**
- rust：`facade.rs:908-935` CopilotMessages 绑定 anthropic_messages_route；`anthropic_messages.rs:2861` BetaHeadersTransport::wrap，:1657-1680 send 时对 body 计算 required_beta_headers_of_body（:1603 恒含 `interleaved-thinking-2025-05-14`）。测试 tests.rs:1482 钉定。G-167 车道 beta 头注入完整，指控不成立。

**2. X-Interaction-Type / X-Interaction-Id 主车道未注入 → NEW-CONFIRM 🟡**
- ref：`github-copilot.ts:272-280` model.request hook 注入 `X-Interaction-Type = interactionType(kind, child)` 与 `X-Interaction-Id = SessionAffinity.get(session) ?? sessionID`，且 `interaction !== "conversation-agent"` 时强制 `x-initiator: agent`。
- rust：`copilot.rs:59-66` `interaction_type()` 已移植但**零生产调用**（仅 tests.rs:42-45）；`copilot_client_headers`(:38-44) 与 `copilot_request_headers`(:101-111) 均无 Interaction 头；`X-Interaction-Id` 仅 utility-title（:840）。model_request.rs:767-778 只注入通用 `x-session-affinity`/`X-Session-Id`，非 Copilot 语义。
- 影响：主 chat/responses/messages 三车道均缺 session affinity 与 interaction 词汇，可能影响 Copilot 侧限流归类/免费 utility 配额判定。
- 修法：copilot route headers seam（或 model_request provider 分支）补 `X-Interaction-Type/-Id`（复用 `session_affinity()` + `interaction_type()`）并在非 agent 交互时升格 x-initiator。

**3. requestMetadata 判定简化 → RETRACT**
- ref 三分支（responses/completions `last.role !== "user"`；messages 额外 user+全 tool_result 判 agent）；rust `copilot.rs:80-95` 统一判定在 messages 分支逐字等价，chat/responses 的 user-only-tool_result 形状在各自 lowering 下不会以 user 消息出现，实际不发散。vision Media 判定覆盖三种 wire 形状。不立项。

**4. credential legacy pin 的 active=false 边角 → NEW-CONFIRM 🟢（G-268 残边）**
- ref `credential.ts:129` `!current.active` 对 **false 和 null 都为真**（顶行 active=false 也钉回 true）；rust 两路均只钉 NULL：`oc-core/integration/credential.rs:139-144`（`pinned.is_none()`）与 `oc-server/handlers/credential.rs:198-207`（同构）。G-268 落地文案即 "legacy NULL pin"。
- 发生条件：integration 全部行显式 active=false。边角低频。
- 修法：两路钉住条件放宽为「active 非真即钉」（`!pinned.unwrap_or(false)` 语义）。

**5. Anthropic options optionalNull 字段拒绝显式 null（含 6 user_id:null 丢弃合并）→ NEW-CONFIRM 🟢**
- ref `anthropic-messages.ts:303/:309-310/:352-353` optionalNull（metadata.user_id、container.id/.skills、inference_geo/inferenceGeo），显式 null 合法且保留（:1086 原样回传 wire）。
- rust `anthropic_messages.rs:238-244` `want_string` 对 `Value::Null` 抛 InvalidRequest；作用于 :449（service_tier）、:456（user_id）、:480-486（container.id/skills、inference_geo）。:458-460 重建丢 user_id 键是同根因下游表现，并入本条。
- 修法：这组字段改 nullable 解码（null→Ok(None)），body 重建保留显式 null 键。

**7. openai_chat ext.body overlay 与 retain(null) 顺序 → RETRACT**
- rust :2759-2763 确为 overlay 先 retain 后；但全部 overlay（groq/minimax/moonshot/openrouter/venice/zai/alibaba）对 null 先过滤再 insert，从不写顶层 `Value::Null`；ref overlay 用 undefined 表缺省同样不产出显式 null。无「overlay 写 null」路径，不构成分歧。

**8. SSE headerTimeout/chunkTimeout 看门狗 → RETRACT**
- rust `oc-ai/route/transport.rs:250-270`（TimeoutBudget）、:310-336（header 预算）、:356-400（chunk idle + total deadline，typed chunk/total_timeout_error）；主发送路径已接 route/client.rs:419,432 与 media/route.rs:528,539,644,656，有行为测试（transport.rs:426-448）。等价看门狗完整在位。

**9. Route.compact 通用 lane → RETRACT**
- ref 的 CompactOperation/TriggerCompactOperation 全仓仅 responses-compaction/checkpoint 两处实现；rust 对应在 responses_compaction.rs（:83/:167/:395）与 responses_checkpoint.rs（:217/:306），经 openai_responses.rs:586-596 暴露、runner/mod.rs:305 触发。唯一实现族已全覆盖，仅架构摆放差异。

**10. Model.small 家族偏好 → RETRACT**
- rust `context.rs:1497-1516` `small_model()` 与 ref model.ts:266-278 逐字对齐（同过滤+同偏好序 ["gpt-luna","gemini-flash-lite","gemini-flash","claude-haiku"]）；调用点 :1544-1553 对齐。

**11. custom/local provider 大小写不敏感兜底 → ℹ️ rust-extra 登记（NEW，登记级）**
- rust `engine/resolver.rs:245-254/:394-397/:364/:406/:445` exact-first + eq_ignore_ascii_case 兜底（注释自证有意宽容）；ref model.ts:255-257 精确 Map。纯宽容度差：ref 报 provider.no-route 的输入 rust 静默成功。建议 ℹ️ 入册。

**12. LocationModelResolver 双层 fallback → RETRACT**
- rust `resolver.rs:695-760` 双层与 ref 合并视图（server/routes.ts:130-135 Config.configured 层叠）产出完全同构，是对合并语义的忠实实现。

**13. models.dev options.file 无接线 → RETRACT**
- ref 源头即 env（server-process.ts:109 OPENCODE_MODELS_PATH）；rust 等价接线在位：server_process_env.rs:74-77,100-124（读文件→Catalog::parse→options.catalog，坏文件降级 embedded）+ boot.rs:337-341，CLI-N4 注释钉定。

## 需登记动作汇总
- NEW ①（🟡）：Copilot 主车道 X-Interaction-Type/-Id 未注入（interaction_type 已移植零接线）。
- NEW ②（🟢）：credential legacy pin 仅 NULL、漏 active=false（双路；G-268 残边）。
- NEW ③（🟢）：anthropic options optionalNull 族显式 null 被拒 + null 键丢弃。
- ℹ️ ④：resolver 大小写不敏感兜底 rust-extra。
- 其余（1/3/7/8/9/10/12/13）RETRACT。
