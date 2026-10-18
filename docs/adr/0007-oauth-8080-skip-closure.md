# ADR-0007: HYGIENE-OAUTH-8080-SKIP 以"验证通过、无需改动"结案

- 状态：accepted（2026-10-18 用户裁决：关闭该 ledger 条目）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：PARITY-LOOP §2.1 要求 Batch 0 确认 OAuth 回调测试（GitLab 固定 `127.0.0.1:8080`）的 skip-on-busy 语义真实存在且不因并行而失败，并写入测试说明。ledger 条目 HYGIENE-OAUTH-8080-SKIP（S3）登记该验证义务。
- 决定：**经验证行为已正确且已有文档，无需任何代码改动**：`crates/oc-core/src/integration/tests.rs:945-950` 在绑定 8080 失败时走 documented-skip 分支（注释注明 v2 同样失败、属环境冲突）。2026-10-18 实证：占用 127.0.0.1:8080 期间运行 `gitlab_pkce_flow_round_trip_against_mock` → PASS（0.005s，即 skip 分支；完整流程远慢于此）。ledger 条目据此以 accepted 结案（本 ADR 即其依据）。
- 后果：该条目退出活跃队列；若未来该测试出现并行冲突/失败，按新证据重开新条目。8080 为该测试固有固定端口（非 49374），不违反端口纪律。
- 证据（path:line / 测试）：`crates/oc-core/src/integration/tests.rs:945-950`；验证运行记录于 `docs/parity-loop-state.md` B0-7 段（2026-10-18，orchestrator，占用端口的 holder 自行退出、无信号）。
- 复审日期：无需复审（行为静态可见 + 已实证）。
