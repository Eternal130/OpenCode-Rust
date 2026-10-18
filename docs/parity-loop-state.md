# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.

- **updated**: 2026-10-18 (iteration 1 — B0-1 + B0-8 merged; B0-2 baseline in progress, p10 reds under triage)
- **batch**: 0 — NOT exited (B0-1..B0-9 pending/in progress; no product bug fixes allowed)
- **iteration**: 1
- **baseline**: main @ eb4023d (pre-bootstrap) → bootstrap commit adds PARITY-LOOP.md / ledger seed / ledger tool / this file

## Active lanes (Batch 0)

| lane | task | worktree | branch | owner | write scope | status |
|---|---|---|---|---|---|---|
| lane-b0-ledger | B0-1 ledger import | removed (merged) | `lane/b0-ledger` | fixer→verifier | `docs/parity-ledger.jsonl`, `tools/parity-import.py`, `docs/parity-ledger-reconciliation.md`, `tools/parity-ledger.py` (owner-empty relaxation) | **MERGED @e2b5b73** (lane commit f04f986, verifier PASS) |
| ~~lane-b0-gate0~~ | B0-8 Gate 0 env | removed (merged) | `lane/b0-gate0` | fixer→verifier | `tools/parity/src/*` skip/strict seam, `docs/parity-b0-gate0.md` | **MERGED @729725d** (lane commit 0d95bbf, verifier PASS) |

WIP: 0 active lanes. Full p10 baseline re-run (no-fail-fast) in flight as orchestrator background shell.

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

## Ledger snapshot (pre-B0-1)

- rows=114 (open=112, needs-runtime=2)
- severity: S0=6, S1=106, S2=2
- clusters: CRED 10, PLUGIN 14, RUNTIME 6, WIRE 16, CONFIG 13, TUI 24, AI 15, DB 6, TOOLS-EXEC 7, SURFACES 3
- B0-1 target: import 16 scan reports (386 raw records → ~340 unique), fingerprint dedupe, `validate --paths` = 0 errors, reconciliation doc explains 386→unique delta.

## Batch 0 checklist

- [x] B0-1 台账导入 — **DONE** (merged @e2b5b73; 367 rows, validate --paths errors=0, 407→367 reconciled)
- [x] B0-8 Gate 0 双端环境 — **DONE** (merged @729725d; OC_GATE_STRICT fail-not-skip live; lane bun discovery works; ref/rust boot evidence recorded)
- [~] B0-2 基线 — in progress: workspace `nextest --workspace --locked --no-fail-fast` → **3391/3391 passed, 10 skipped, 0 failed** (1 leaky flag); `fmt --all --check` OK; clippy `-p parity` → 15 errors ALL in pre-existing `tools/parity/src/bin/rescan-tui.rs` (byte-identical to main, known-red); **p10 strict battery RED on main@729725d**: `p10_ts_tui_against_rust_server` (rename never reached v2 session row; ref-side poll timeout) + `p10_db_interop_stable_channel_same_file` (v2 did not read model switch) — full no-fail-fast re-run in flight for complete picture + flake-vs-deterministic classification. Remaining: docs/parity-baseline.md + 5 gate skeletons + known-red ledger entries.
- [ ] B0-4 静态守卫 tools/guards/*（目录尚不存在，需从零建）
- [ ] B0-5 ADR 0001–0005
- [ ] B0-7 测试隔离债（legacy pkill 脚本登记、root-owned auth_e2e.rs、8080 skip 语义）
- [ ] B0-9 live fixture 窗口 — **需要用户决策**（endpoint/credential sandbox + 时段）；其余 Batch 0 完成后集中询问

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

- B0-9 requires user decision (live fixture window). Deferred until rest of Batch 0 lands; will ask once.
- p10 strict battery red on main (2+ failures) — must be classified (flake vs deterministic vs pre-existing before eb4023d) and ledger-ized as known-red with owner/expiry in B0-2; blocks nothing else in Batch 0.

## Known-red register (to ledger-ize in B0-2)

1. `tools/parity/src/bin/rescan-tui.rs` clippy debt — 15 errors, pre-existing, byte-identical to main@2693ae8.
2. `parity::scenarios scenario_permission_saved` — 1 flaky failure in 3 repetitions (B0-8 verifier observation; passed in all orchestrator runs since).
3. p10 strict failures (pending full run): `p10_ts_tui_against_rust_server`, `p10_db_interop_stable_channel_same_file` (+ any revealed by no-fail-fast run).
4. tmux-availability skips in opt-in TUI tests remain skips under OC_GATE_STRICT (documented B0-8 follow-up; decision needed whether p10 battery requires tmux strictness).

## Next

1. Reconcile lane-b0-ledger + lane-b0-gate0 results → independent verifier per lane → serial merge (one at a time, affected checks after each).
2. Then B0-2 baseline: `tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'` + p10 battery → `docs/parity-baseline.md`; five gate skeletons with known-red→ledger IDs.
3. Then B0-4 guards, B0-5 ADRs, B0-7 test debt; ask user for B0-9 window; exit Batch 0.
