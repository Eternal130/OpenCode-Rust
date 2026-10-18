# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.

- **updated**: 2026-10-18 (iteration 1, in flight)
- **batch**: 0 — NOT exited (B0-1..B0-9 pending/in progress; no product bug fixes allowed)
- **iteration**: 1
- **baseline**: main @ eb4023d (pre-bootstrap) → bootstrap commit adds PARITY-LOOP.md / ledger seed / ledger tool / this file

## Active lanes (Batch 0)

| lane | task | worktree | branch | owner | write scope | status |
|---|---|---|---|---|---|---|
| lane-b0-ledger | B0-1 ledger import | `.worktrees/lane-b0-ledger` | `lane/b0-ledger` | fixer (bg) | `docs/parity-ledger.jsonl`, `tools/parity-import.py`, `docs/parity-ledger-reconciliation.md`, (`tools/parity-ledger.py` ext. if needed) | dispatched |
| lane-b0-gate0 | B0-8 Gate 0 env | `.worktrees/lane-b0-gate0` | `lane/b0-gate0` | fixer (bg) | `tools/parity/src/*` skip/strict seam, `docs/parity-b0-gate0.md` | dispatched |

WIP: 2 active lanes (max 3). Disjoint write scopes; no hot-file overlap.

## Ledger snapshot (pre-B0-1)

- rows=114 (open=112, needs-runtime=2)
- severity: S0=6, S1=106, S2=2
- clusters: CRED 10, PLUGIN 14, RUNTIME 6, WIRE 16, CONFIG 13, TUI 24, AI 15, DB 6, TOOLS-EXEC 7, SURFACES 3
- B0-1 target: import 16 scan reports (386 raw records → ~340 unique), fingerprint dedupe, `validate --paths` = 0 errors, reconciliation doc explains 386→unique delta.

## Batch 0 checklist

- [ ] B0-1 台账导入 — in progress (lane-b0-ledger)
- [ ] B0-8 Gate 0 双端环境 — in progress (lane-b0-gate0)
- [ ] B0-2 基线（workspace + p10 → docs/parity-baseline.md；五组 gate 骨架）
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

## Next

1. Reconcile lane-b0-ledger + lane-b0-gate0 results → independent verifier per lane → serial merge (one at a time, affected checks after each).
2. Then B0-2 baseline: `tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'` + p10 battery → `docs/parity-baseline.md`; five gate skeletons with known-red→ledger IDs.
3. Then B0-4 guards, B0-5 ADRs, B0-7 test debt; ask user for B0-9 window; exit Batch 0.
