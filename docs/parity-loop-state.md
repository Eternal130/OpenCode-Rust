# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.

- **updated**: 2026-10-19 17:4x — LOOP PAUSED (3rd host incident: hard halt ~17:32, no log evidence; sessions C6b/CRED-2-verifier dead; resized_checkpoint NEW-RED post-H0-merge pending triage)
- PREVIOUS: wave 2 MERGED (CONFIG 6 @5b69cd5, H0 spike @9c9c9d4 oracle-approved, AI-PROV-2 @50f2ed0); wave 3 dispatched
- **batch**: 2 — wave 2 in flight (3 lanes); wave-1 remains: CONFIG continuation
- **iteration**: 6
- **baseline**: main @ cfac11e

## Active lanes (Batch 2 wave 2)

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| lane-config | C5 CONFIG (continuation) | `.worktrees/lane-config` | `lane/config` | fixer (bg) | C7,C8,C1,T1,T3 (5 committed by dead session) + T4 WIP | finishing |
| lane-ai-prov2 | C7b slice 2 | `.worktrees/lane-ai-prov2` | `lane/ai-providers-2` | fixer (bg) | AI-COPILOT-C1, AI-COPILOT-C2, AI-COHERE-MISTRAL | dispatched |
| lane-h0spike | C4b H0 spike+slice | `.worktrees/lane-h0spike` | `lane/h0-spike` | fixer (bg) | HTTP-H0 (spike + 10 routes; oracle review after) | dispatched |

Queued wave 3: C6b session-route (TD-01/02/03/05/07), C9a SURFACES, C7b remainder (M2-M5), C5a keys-env remainder.

## INCIDENT LOG — ROOT CAUSE CORRECTED (2026-10-19 11:0x)

**真凶不是内存，是我（orchestrator）的命令错误**：两次主机软重启（09:12:35、10:43:59）都由 `bash tools/guards/process_safety.py --update-baseline` 引起 —— 用 bash 执行 Python 文件时，bash 把源码当 shell 解析，docstring 文档表格里的反引号示例被当作真实命令替换执行：
- 第 18 行的 `kill ... $(pgrep ...)` 示例 → root 身份对**全机进程**（含 PID 1）发 SIGTERM → systemd 软重启（journal 铁证：`systemd[1]: Received SIGTERM from PID xxxxx (bash)` 两次均为该命令运行时刻）
- 第 13 行的 `fuser -k 49374/tcp` 示例 → 可能实际执行过（用户 49374 服务中断的另一嫌疑）
- 连带损害：用户会话/caddy/docker/lobehub-redis 全部来自软重启，非资源耗尽
**修复（@18cfee2）**：全部 9 个守卫脚本加 bash/python 双解释器哨兵（bash 误执行时第一行即重定向给 python3），文档字符串去武装化；实测 `bash process_safety.py` 现在安全。之前的 oomd/内存归因是错误结论，已废弃（flock/6G 加固保留作为一般性卫生，但非本次根因）。4 个孤儿 bun 服务器是真实但独立的旧泄漏（已修 + 已清）。

历史现场保留：用户手动恢复了 caddy/docker/lobehub；机器 up 7 天未重启（软重启不重置 uptime）。

## Uncommitted/interrupted items (all safe to leave)

- lane-config worktree: partial fixer work (inspect `git log main..lane/config` + status before re-dispatch; fixer session dead)
- process_safety baseline: needs `--update-baseline` (pure line-drift, verifier-confirmed) — currently reads 3 false NEW
- loop otherwise fully consistent: 391 ledger rows validate clean; all merges committed; no orphan processes remaining (verified)

## Batch 1B — EXITED (2026-10-19)

4 clusters merged & verified (28 entries + 1 regression): WIRE-PROTOCOL @ef1d201 (15) · TOOLS-EXEC @fda1daf (7) · AI-TIMEOUT @1ab94f0 (1) · RUNTIME @86a37c7+9fcd97a (5) · **RUNTIME-SWEEP-RACE regression** @d73218f (found by batch-exit gate, bisected to runtime-a, root-caused as claim-sweep without owner signature, fixed with pre-boot-claim bound; verifier PASS).
Exit: workspace **3470/3470**, p10 strict **clean** (3 KNOWN-RED, 0 NEW-RED), fmt --all green, guards 8/8.
Notable: Gate 1 caught WIRE's static-reading error (empty-page cursor = explicit nulls per live-ref probe); merge conflicts resolved by integrator (ledger rows + add/add test file split into prompt_prepare_hooks.rs).

## Active lanes (Batch 2 wave 1)

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| lane-config | C5 CONFIG | `.worktrees/lane-config` | `lane/config` | fix-29 (bg) | CONFIG-C7,C8,C1,T1,T3,T4 | dispatched |
| lane-ai-prov | C7b AI-PROVIDERS | `.worktrees/lane-ai-prov` | `lane/ai-prov` | fix-30 (bg) | AI-M1, M10, AZURE | dispatched |
| lane-tui-daily | C6c TUI-DAILY | `.worktrees/lane-tui-daily` | `lane/tui-daily` | fix-31 (bg) | TUI-3, TUI-11, TUI-13 | dispatched |

Queued wave 2: C4b (HTTP-H0 validator generator, oracle-reviewed spike first per §4.5 设计门), C9a SURFACES (browser tool + LaTeX/Mermaid), rest of C7b (Cohere/Mistral/Copilot/Bedrock-remainder), C6b session-route, C5a keys-env remainder.

## Batch 1B — core clusters EXITED (2026-10-19); C3 in flight

- AI-TIMEOUT @1ab94f0 (verified; ref-constants line-exact; ref[0] citation corrected)
- TOOLS-EXEC @fda1daf (7 verified; process-safety audited; hook_dispatch baseline shrank 10→9)
- WIRE-PROTOCOL @ef1d201 (15 verified + fold GATE-GAP-CURSOR-PAGE→HTTP-H2; two exit fixes on main: oc-cli acp cursor test → v2 pass-through @ff1817a; empty-page cursor = EXPLICIT NULLS per live-ref probe @27405eb + evidence correction @59ebc04 — Gate 1 caught the lane verifier's static-reading error; runtime probes beat static reads)
- Batch exit (pre-C3): workspace **3448/3448**, p10 strict verdict **clean** (3 KNOWN-RED, 0 NEW-RED), oc-server 231/231 post-fix, guards 8/8, fmt clean. Ledger: 387 rows.

## Active lanes (C3 RUNTIME — cluster merge serial after both sub-lanes verify)

| lane | sub-lane | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| lane-runtime-a | C3a wake-recovery | `.worktrees/lane-runtime-a` | `lane/runtime-a` | fix-23 (bg) | RUNTIME-S01, RUNTIME-L01 | dispatched |
| lane-runtime-b | C3b reconnect-hooks | `.worktrees/lane-runtime-b` | `lane/runtime-b` | fix-24 (bg) | RUNTIME-P01, RUNTIME-P02, RUNTIME-TD04 | dispatched |

K-01 (compaction payload retry, S1) intentionally unclaimed — outside §5's 1B row; queue for Batch 2.

## Batch 1A — EXITED (2026-10-18)

All 3 clusters verified+merged serially, each with an independent verifier (red reconstruction + golden/manifest audits):
- CRED @bc7fdd3 (B1/B2/B3 verified; +2 follow-ups CRED-CONSOLE-KEYAUTH, CRED-AUTHNONE)
- PLUGIN-core @24b107a (D21/D18/D14 verified; +2 follow-ups PLUG-KVSCAN-CURSOR, PLUG-RPC-DATA-FIELD)
- TUI-FORM @bc44de1 (TD16-19 verified, golden re-auth v2-grounded; tui-frames manifest re-pin @e180f01; +1 follow-up TUI-EMIT-GAP)
- Batch exit: workspace **3413/3413**, p10 strict verdict **clean** (3 KNOWN-RED, 0 NEW-RED), guards 8/8, fmt clean. Ledger: 385 rows.

## Active lanes (Batch 1B — 3 clusters; serial-heavy discipline)

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| lane-wire | C4a WIRE-PROTOCOL | `.worktrees/lane-wire` | `lane/wire` | fix-17 (bg) | HTTP-H1..H6, PTY-F1..F4, VCS-1..4, EV-1 (15) | dispatched |
| lane-tools | C10 TOOLS-EXEC | `.worktrees/lane-tools` | `lane/tools` | fix-18 (bg) | TOOL-C1..C5, A1, A2 (7) | dispatched |
| lane-ai-timeout | C7a AI-TIMEOUT | `.worktrees/lane-ai-timeout` | `lane/ai-timeout` | fix-19 (bg) | AI-TIMEOUT | dispatched |

Queued: C3 RUNTIME (C3a wake-recovery first — after C10's hooks.rs round + C4a's session.rs seam). C4b (HTTP-H0 validator generator) slides to Batch 2 per §5.

Discipline update (user lag report): all heavy runs serialized — orchestrator never runs gates while a child is active; fixers instructed to serialize their own gate commands.

## Host-issue H-1 mitigation (active)

Children with background-task history: parent re-checks with `task_result` on completion instead of trusting push notifications (TUI verifier's final report was stored-but-not-pushed; recovered manually).

| lane | task | worktree | branch | owner | write scope | status |
|---|---|---|---|---|---|---|
| lane-b0-ledger | B0-1 ledger import | removed (merged) | `lane/b0-ledger` | fixer→verifier | `docs/parity-ledger.jsonl`, `tools/parity-import.py`, `docs/parity-ledger-reconciliation.md`, `tools/parity-ledger.py` (owner-empty relaxation) | **MERGED @e2b5b73** (lane commit f04f986, verifier PASS) |
| lane-b0-gates | B0-2 gate skeletons | removed (merged) | `lane/b0-gates` | fixer→verifier | `tools/parity/tests/**` (5 new gates + p10_common), `goldens/p10/known-red.json`, `known-red-summary.py`, README, baseline §3 | **MERGED @3eea5fa** (lane commit f7b44f4, verifier PASS) |
| ~~lane-b0-gate0~~ | B0-8 Gate 0 env | removed (merged) | `lane/b0-gate0` | fixer→verifier | `tools/parity/src/*` skip/strict seam, `docs/parity-b0-gate0.md` | **MERGED @729725d** (lane commit 0d95bbf, verifier PASS) |

WIP: 0 active lanes.

## B0-1 record (complete)

- Lane commit `f04f986` → merge commit `e2b5b73` (main, --no-ff). Verifier: PASS on all 9 checks (validate errors=0; determinism double-run byte-identical sha256 9cbcb6c3…; 114 seeds immutable except 13 documented alias-only additions; 30-ID orphan sampling 0 orphans; 12-row field-fidelity sampling clean; reconciliation 407=113+16+253+25 independently re-summed; tool change only relaxes owner-emptiness).
- Ledger now: **367 rows** (open=357, needs-runtime=10; S0=6, S1=108, S2=233, S3=20; clusters: WIRE 71, TUI 55, CONFIG 44, TOOLS-EXEC 42, RUNTIME 33, AI 31, CRED 24, DB 23, PLUGIN 23, SURFACES 21).
- Verifier observations (documentation-level, non-blocking): report-01 §10 KNOWN cross-ref `01:X-01` outside the 407 enumeration (legacy registration, consistent with detail-rows-authoritative rule); report-10 P→S ladder (P0→S1, P1/P2→S2, P3→S3) defensible but undocumented in reconciliation §4.

## B0-8 record (complete)

- Lane commit `0d95bbf` → merge commit `729725d` (main, --no-ff). Verifier: PASS on all 9 checks (strict-red exit 100 with named-missing-bun panic; green 1/0; lane discovery 1/0; gate_ran record reproduced with ref_commit e7a34f09…, both boots ok; scope clean; fmt OK; clippy errors confined to pre-existing `tools/parity/src/bin/rescan-tui.rs`, byte-identical to main).
- Post-merge gates on main: `nextest -p parity` 103 passed / 0 failed / 9 skipped; `fmt -p parity` OK; clippy 15 errors all in rescan-tui (pre-existing).
- Evidence: `docs/parity-b0-gate0.md`, `/tmp/opencode/parity-evidence/B0-8/{red-strict.log,gate-ran.json,verify-gate-ran.json,...}`.
- Follow-ups (to ledger-ize after B0-1 merge):
  1. `parity::scenarios scenario_permission_saved` showed 1 flaky failure under repetition (verifier run) — §8.2 requires flaky ledger entry.
  2. tmux-availability skips in opt-in TUI tests remain skips under strict mode (documented in evidence doc §7) — p10 battery owner decision.
  3. `rescan-tui.rs` pre-existing clippy debt — B0-2 baseline known-red.
  4. `env-identity.log` artifact is root-owned (content correct) — capture-process slip only.

## Ledger snapshot (post-B0-2)

- rows=376 (open=366, needs-runtime=10)
- severity: S0=6, S1=112, S2=236, S3=22
- imported 367 (B0-1) + 5 known-red (B0-2 baseline) + 4 gate-gap divergences (B0-2 skeletons)
- known-red registry: `tools/parity/goldens/p10/known-red.json` (5 ids; expiry Batch 1B / Batch 0); summary tool exit-0 semantics verified adversarially by B0-2 verifier.

## Batch 0 checklist

- [x] B0-1 台账导入 — **DONE** (merged @e2b5b73; 367 rows, validate --paths errors=0, 407→367 reconciled)
- [x] B0-8 Gate 0 双端环境 — **DONE** (merged @729725d; OC_GATE_STRICT fail-not-skip live; lane bun discovery works; ref/rust boot evidence recorded)
- [x] B0-2 基线 — **DONE**: `docs/parity-baseline.md` @fe2a97d; five gate skeletons + known-red registry + known-red-summary.py merged @3eea5fa (lane f7b44f4, verifier PASS incl. adversarial script tests); post-merge: default suite 103/14 skip, strict battery verdict **clean** (3 KNOWN-RED registered, 1 MISSING-PREREQ by design, 0 NEW-RED, exit 0); +4 gate-gap divergence entries (GATE-GAP-CURSOR-PAGE/TUI-FOOTER/CLI-JSON-EPOCH/RUN-JSON-EVENTS) → ledger 376 rows. Polish nits (non-blocking, recorded): stale comment p10_db_interop.rs:~637 "panics"→Err; README lacks OPENCODE_RG_PATH note; baseline §3 Gate-4 row understates certified chrome rows.
- [ ] B0-4 静态守卫 tools/guards/*（目录尚不存在，需从零建）
- [x] B0-5 ADR 0001–0005 — **DONE** (merged @cc092d9; verifier PASS, 44/44 citations verified; 3 OPEN questions recorded: 0002 miniaudio-vs-system-player ruling, 0004 README scope staleness at release, 0001 KV persistence → C2)
- [~] B0-7 测试隔离债 — mostly done: all 4 items ledger-ized (HYGIENE-PKILL-LEGACY-PROBES / RELEASE-SMOKE-KILL / AUTH-E2E-ROOT-OWNED / OAUTH-8080-SKIP); 8080 skip semantics **empirically verified** (port held → test PASSes via documented-skip branch, 5ms); guards lane (B0-4, in flight) baselines the legacy pkill set. Remaining: owner assignment (Batch 1+) + user nod on auth_e2e.rs ownership fix (bundled into B0-9 decision batch).
- [x] B0-9 live fixture 窗口 — **DONE** (2026-10-18 用户裁决：cassette-only for now；ADR-0006；release 前必复审)

## User-decision batch (2026-10-18, all recorded)

1. **B0-9 live window** → cassette-only；ADR-0006 `docs/adr/0006-cassette-only-live-fixtures.md`；release 前重问。
2. **ADR-0002 miniaudio ruling** → 接受 Linux inproc vendored miniaudio 为既成决定；ADR-0002 已修订（OPEN 关闭）；README "sounds" 滞后随 §12.4 清理。
3. **tmux strict policy** → 保持 documented skip（即使 OC_GATE_STRICT=1）；tmux 属环境项非双端前提；B0-8 证据文档 §7 follow-up 就此了结，无需代码改动。
4. **root-owned files chown** → 用户批准并已执行：auth_e2e.rs、spec-p3/*、spec-ef2/* 全部 eternal130 化（剩余 root-owned=0）。
5. **HYGIENE-OAUTH-8080-SKIP** → 关闭（accepted，ADR-0007）。

## Environment facts (verified this iteration)

- Orchestrator host runs as **root**; all repo mutations go through eternal130 via
  `systemd-run --scope --quiet --uid=eternal130 --gid=eternal130 --setenv=HOME=/home/eternal130 ...`
  (same mechanism as `tools/dev-gate.sh`). All cargo/test strictly via `tools/dev-gate.sh`.
- bun: root's `/root/.bun/bin/bun` (755 but `/root` is 700 → eternal130 cannot traverse).
  Staged copy at `/home/eternal130/.bun/bin/bun` (755, world-exec). eternal130 cache at
  `~/.bun/install/cache` is eternal130-owned. Verified `bun --version` runs as eternal130.
- ref checkout: `/tmp/opencode/ref` (eternal130-owned, node_modules installed, NOT a git checkout);
  read-only source `/opt/opencode-ref` exists. `PARITY_REF_ROOT`/`PARITY_BUN`/`PARITY_REF_SOURCE` supported in
  `tools/parity/src/reference.rs`; self-heal path exists (tar copy + `bun install --frozen-lockfile`).
- **OC_GATE_STRICT is not implemented anywhere yet** — `reference::skip_if_no_bun()` self-skips
  (e.g. `tools/parity/tests/reference_info.rs:18`). B0-8 lane implements fail-not-skip.
- `tools/guards/` does not exist (B0-4 builds it).
- Stale worktree `.worktrees/lane-fixw2` (branch `lane/fixw2`, fully merged at 0b24906, clean) — removed; branch kept.

## Blockers

- None. ADR-0002/0006/0007 + tmux ruling recorded; ownership fixed. (ADR-0004 README scope alignment stays scheduled at release per its 复审日期.)

## Known-red register (to ledger-ize in B0-2)

1. `tools/parity/src/bin/rescan-tui.rs` clippy debt — 15 errors, pre-existing, byte-identical to main@2693ae8.
2. `parity::scenarios scenario_permission_saved` — 1 flaky failure in 3 repetitions (B0-8 verifier observation; passed in all orchestrator runs since).
3. p10 strict failures (pending full run): `p10_ts_tui_against_rust_server`, `p10_db_interop_stable_channel_same_file` (+ any revealed by no-fail-fast run).
4. tmux-availability skips in opt-in TUI tests remain skips under OC_GATE_STRICT (documented B0-8 follow-up; decision needed whether p10 battery requires tmux strictness).

## Next

1. Reconcile lane-b0-ledger + lane-b0-gate0 results → independent verifier per lane → serial merge (one at a time, affected checks after each).
2. Then B0-2 baseline: `tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'` + p10 battery → `docs/parity-baseline.md`; five gate skeletons with known-red→ledger IDs.
3. Then B0-4 guards, B0-5 ADRs, B0-7 test debt; ask user for B0-9 window; exit Batch 0.
