# ADR-0006: AI provider 运行时验证采用 cassette-only（live fixture 窗口延后）

- 状态：accepted（2026-10-18 用户裁决：cassette-only for now；live window 可在 release 前再行批准）
- 日期：2026-10-18
- 上游版本：2.0.24 (ref e7a34f09bf)
- 背景：PARITY-LOOP §4.8（C7 AI 簇）的 exit 要求 Azure/Cohere/Mistral 等 provider 至少一组真实 fixture；规则是：没有用户预先批准的 live fixture 窗口（endpoint/credential sandbox + 运行时段），就必须 cassette + ADR 明确 "runtime reachability deferred"。B0-9 负责征求该窗口。
- 决定：**用户裁决（2026-10-18）：当前不设立 live fixture 窗口，AI provider 相关验证一律 cassette-only**。所有 provider 运行时可达性（真实网络行为、真实鉴权、真实流式）显式 deferred，直至用户后续批准具体窗口（窗口要素：批准的 endpoint 清单、credential sandbox、起止时段；窗口外一律 cassette；每次 live 运行记录 endpoint/时间/结果，不记录凭据）。
- 后果：C7 修复只能以 cassette + 本地 stall/fake server 验证协议与接线；"runtime reachable" 类断言不进入 verified 证据（不得口头声称）。release 前（§12 退出检查前）需重新征求一次窗口决定：若仍无窗口，release report 的 not-covered matrix 必须列出全部 runtime-deferred 的 provider 面。
- 证据（path:line / 测试）：`docs/PARITY-LOOP.md` §4.8 Live fixture window 段、§8.2 默认测试纪律（不依赖网络）；本裁决来自 2026-10-18 用户对 B0-9 问题的答复（cassette-only, recommended）。
- 复审日期：release 前（§12 退出检查前必问）。
