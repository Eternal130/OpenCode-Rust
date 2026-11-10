# 第二十轮复核 W-B（AI/TLS/客户端）— 2026-11-10，HEAD=967bedba

> 对第十九轮立项的未闭合项在当前 HEAD 重验（c36d2556 全仓 fmt 后行号漂移已刷新）。

1. **G-298 Copilot X-Interaction-Type/-Id 未注入 → STILL_OPEN**
   - `oc-ai/src/protocols/copilot.rs:59-67` `interaction_type()` 仍零生产调用（仅 tests.rs:42-45）；`copilot_client_headers`（:38-46）与 `copilot_request_headers`（:100-113，仅 x-initiator 升格 + Copilot-Vision-Request）均无 Interaction 头；`X-Interaction-Id` 仍仅 utility-title（:831-832）；通用注入在 `oc-core/src/session/runner/model_request.rs:776`（路径已从 oc-ai 漂移至 oc-core）且非 Copilot 语义。
2. **G-299 NODE_EXTRA_CA_CERTS → STILL_OPEN**
   - 全仓 grep 零命中；`oc-http/src/lib.rs:34-54` `platform_tls_config()` 仍 platform-verifier→webpki 回退（:54），无 env 读取、无 RootCertStore 追加路径。
3. **G-300 客户端 SSE 空闲看门狗 → STILL_OPEN**
   - `oc-tui/src/data.rs:1093-1097` 订阅无 2s deadline；`:1158` `stream.next_json().await` 裸 await 无 45s 超时；首帧无 server.connected 断言（:1160 仅状态标记）。文件头注释（:1-54）自述无看门狗。
4. **G-301 run 附件 MIME 子集 → STILL_OPEN**
   - `oc-cli/src/run/mod.rs:118-152` detect_mime 仍 5 魔数+5 扩展，`_ => octet-stream`；svg/bmp/tiff/ico/avif/heic 零命中。
5. **G-306 Anthropic optionalNull → STILL_OPEN（container 半边微收窄）**
   - `anthropic_messages.rs:238-244` want_string 对 null 仍 Err；作用点刷新：service_tier :448-449、user_id :456-461（:458-460 重建仍丢键）、container.id/skills :464-483、inference_geo :485-486。注：container 现整体 `Option<Value>` 透传（:226/:1815-1816），内部 id/skills 显式 null 仍被拒——面略收窄，主根因未修。
6. **G-309 断点丢弃无告警 → STILL_OPEN**
   - `anthropic_messages.rs:523,544` dropped 仍仅计数；全文件 warn!/log:: 零命中。

汇总：6/6 STILL_OPEN（G-306 container 半边为 PARTIAL 性边角变化，主体未修）。
