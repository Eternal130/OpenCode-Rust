# ADR-0006: AI provider 运行时验证采用 cassette-only（live fixture 窗口延后）

- 状态：accepted（2026-10-18 用户裁决：cassette-only for now；live window 可在 release 前再行批准）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：PARITY-LOOP §4.8（C7 AI 簇）的 exit 要求 Azure/Cohere/Mistral 等 provider 至少一组真实 fixture；规则是：没有用户预先批准的 live fixture 窗口（endpoint/credential sandbox + 运行时段），就必须 cassette + ADR 明确 "runtime reachability deferred"。B0-9 负责征求该窗口。
- 决定：**用户裁决（2026-10-18）：当前不设立 live fixture 窗口，AI provider 相关验证一律 cassette-only**。所有 provider 运行时可达性（真实网络行为、真实鉴权、真实流式）显式 deferred，直至用户后续批准具体窗口（窗口要素：批准的 endpoint 清单、credential sandbox、起止时段；窗口外一律 cassette；每次 live 运行记录 endpoint/时间/结果，不记录凭据）。
- 后果：C7 修复只能以 cassette + 本地 stall/fake server 验证协议与接线；"runtime reachable" 类断言不进入 verified 证据（不得口头声称）。release 前（§12 退出检查前）需重新征求一次窗口决定：若仍无窗口，release report 的 not-covered matrix 必须列出全部 runtime-deferred 的 provider 面。
- 证据（path:line / 测试）：`docs/PARITY-LOOP.md` §4.8 Live fixture window 段、§8.2 默认测试纪律（不依赖网络）；本裁决来自 2026-10-18 用户对 B0-9 问题的答复（cassette-only, recommended）。
- 复审日期：release 前（§12 退出检查前必问）。

## 附注（2026-10-21）：live canary 可选触发（CI-RUNTIME-AI-LIVE）

- 用户裁定（2026-10-21）：在 CI 侧设立 **live canary、可选触发** —— runtime-gates 的 `workflow_dispatch` 增加 `live` input（boolean，默认 false）。默认运行仍维持本 ADR 的 cassette-only 纪律；仅当显式 dispatch `live=true` 且四个 `OC_CI_*` secrets 在场时才执行真实请求（缺席 = 配置错误，workflow 醒目失败，不绿）。
- secrets 契约（以 `.github/workflows/runtime-gates.yml` 头注释为准）：`OC_CI_VERTEX_PROJECT`（GCP project id）、`OC_CI_VERTEX_CREDENTIALS`（**新鲜** OAuth2 access token，cloud-platform scope，约 1h 有效期，dispatch 前 `gcloud auth print-access-token --scopes=https://www.googleapis.com/auth/cloud-platform` 刷新；messages 与 chat/responses lane 均为 ADC-bearer，拒绝 API key）、`OC_CI_GEMINI_API_KEY`（AI-V1 Express 模式 `x-goog-api-key` 腿）、`OC_CI_COPILOT_TOKEN`（Copilot `/models` 同步）。
- 探针：`tools/parity/tests/live_ai_probes.rs`（三条：AI-VERTEX-V2 / AI-V1 / AI-COPILOT-C6），复用生产 `oc-ai` 调用面（route 组装 + `ReqwestTransport`），非独立 HTTP 栈；`#[ignore]` + `OC_CI_LIVE=1` 双重 opt-in；预算每探针 ≤60s、单请求 ≤25s；凭据值绝不打印/落盘（日志与 evidence 只记 endpoint/耗时/状态码/形状摘要，`/tmp/opencode/parity-evidence/CI-RUNTIME-AI-LIVE/live-runs.jsonl`）——满足本 ADR"每次 live 运行记录 endpoint/时间/结果，不记录凭据"的记录规则。
- 语义不变的部分：skip ≠ pass、LIVE-SKIP 醒目可见（strict 模式下直接失败）；live 探针失败使 job 红（gates-summary 如实记账）。
- live pass 本身待用户配置 secrets 后以 `live=true` 跑出（台账 CI-RUNTIME-AI-LIVE 状态 fixed 即指探针 + 触发链就绪，非已完成的 live 通过记录）。
