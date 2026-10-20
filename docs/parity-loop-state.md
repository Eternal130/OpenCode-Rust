# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.
> **This file was REBUILT from ledger+git on 2026-10-20** (the previous copy was
> stale: it claimed "batch 2 / iteration 6 / baseline cc694e0" while main was
> already at 736dc02 with waves 5–7 merged). Ledger + git are authoritative;
> state.md is a cross-session breadcrumb only.

- **updated**: 2026-10-20 — recovery round done; 3 lanes dispatched (DB verify/adopt, TUI-4..7 fix, CONFIG CLI-* fix) + 1 adjudication batch
- **baseline**: main @ `736dc02` (fix(tools): loop-driver runs as the opencode-owning user)
- **batch**: 3 (wave 8) — Batch 3 clusters: DB, PLUGIN-RESIDUAL, EVENT/TOOL/CORE, PLATFORM, POLICY/RETENTION
- **iteration**: 8 (session-rotated; driver = tools/loop-driver.sh, `opencode run` per iteration)

## Ledger snapshot (rebuilt 2026-10-20, `python3 tools/parity-ledger.py summary`)

- rows=396 — verified 91 · accepted 1 · duplicate 1 · needs-runtime 9 · open 294 · fixing 0
- severity: S0 6 · S1 115 · S2 244 · S3 31
- open S0/S1 = **32** (AI 1, CONFIG 7, CRED 3, DB 2, PLUGIN 3, RUNTIME 1, SURFACES 3, TOOLS-EXEC 1, TUI 9, WIRE 2)
- clusters (all rows): WIRE 78 · TUI 60 · CONFIG 46 · TOOLS-EXEC 42 · RUNTIME 34 · AI 31 · PLUGIN 27 · SURFACES 27 · CRED 26 · DB 25

## Recovery findings (2026-10-20, §0a) — READ BEFORE DISPATCHING

1. **The wave-6 "merge: lane-db" never landed.** `77c3950` ("merge: lane-db", DB-D1/D2/D3/D4/D6/D12) has
   **one parent**, touched only `docs/parity-ledger.jsonl`; the DB code (`5cb0010`/`fc35b6d`/`12b2d79`/`4074fed`)
   exists only on `lane/db` (`7657230`). Main still hashes project ids with **SHA-256** (`crates/oc-core/src/project.rs:20,26`);
   `lane/db` has **SHA-1** (`Sha1::digest`). `crates/oc-core/src/database/v1_migration.rs`,
   `crates/oc-core/tests/database_identity.rs` are **missing from main**.
   The ledger's `merge_commit=73c4f53` on DB-D1/D2/D3/D4/D6/D12 is a **false merge record** (§10.7 phantom).
   → Fix: independent verifier re-checks the lane, then a real `git merge --no-ff lane/db`, then re-record the true merge sha.
2. **lane/tui2 is an orphan WIP**: `570306f` (TUI-4 TerminalPane port, claimed 6/6 + oc-tui 961/964; TUI-5/6/7
   reds captured, production code not started). Not in main. Evidence dirs for TUI-4/5/6/7 hold **only `red.log`**
   (all four byte-identical, 12845 B) — no green artifact.
   → Fix: continue the lane (do NOT restart it): finish TUI-4, implement TUI-5/6/7 production paths.
3. **6 stale worktrees removed** (all 0-ahead + clean): lane-ai-media, lane-dialogs, lane-plugeng, lane-plugin,
   lane-plugin2, lane-h0b. Branches kept.
4. **HTTP-H0 zombie `fixing` → `open`.** Its lane (`lane/h0b`, merged @5f7df02) had its enforcement **rolled back**
   2026-10-19 (ENFORCED_OPS dropped `pty.create` + `server.experimental.persistentPty.create`; route_validation
   baseline 122→124). 26/141 routes validated, 115 remain. Ledger row reopened, owner/branch/worktree cleared.
5. **State.md was 2 waves stale** — rebuilt from ledger+git (this file).

## Active lanes (Batch 3 wave 8)

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| lane-db | DB (adoption) | `.worktrees/lane-db` | `lane/db` @7657230 | verifier→integrator | DB-D1,D2,D3,D4,D6,D12 | dispatched — verify → merge → re-record sha |
| lane-tui2 | TUI (continuation) | `.worktrees/lane-tui2` | `lane/tui2` @570306f | fixer | TUI-4 (WIP), TUI-5, TUI-6, TUI-7 | dispatched |
| lane-config | CONFIG | `.worktrees/lane-config` | `lane/config` @736dc02 | fixer | CLI-K1, CLI-N1, CLI-N2, CLI-N3, CLI-N4 | dispatched |
| (read-only) | adjudication | — | — | explorer | CRED-D6,D8,V3 · PLUG-D13,M02 · TOOL-D1 · AI-P06 · RUNTIME-K01 · SURF-PLATFORM | dispatched |

Active clusters (writers) = 3 (DB verify-phase, TUI, CONFIG). Adjudication batch is read-only.
Hot-file owners: `crates/oc-tui/src/app.rs` + `crates/oc-tui/src/store.rs` = TUI (lane-tui2 only);
`crates/oc-core/src/hooks.rs` + `crates/oc-server/src/plugins.rs` = PLUGIN (idle this wave);
`crates/oc-server/src/handlers/project.rs` = DB (integrator, this round).
`docs/parity-ledger.jsonl` = **orchestrator-owned** — fixers must NOT edit it (report wiring instead).

## Open S0/S1 queue (32) — next-up order after this wave

CONFIG 7 (CLI-K1,K2,K8,N1..N4) · TUI 9 (TUI-4..7,12,14..17) · CRED 3 (D6,D8,V3) · PLUGIN 3 (D13,D30,M02) ·
SURFACES 3 (BROWSER,LATEX-MERMAID,PLATFORM) · DB 2 (GATE-DB-NULLROWS, GATE-DB-STABLE) · AI 1 (AI-P06) ·
RUNTIME 1 (RUNTIME-K01) · TOOLS-EXEC 1 (TOOL-D1) · WIRE 2 (GATE-TUI-RENAME, HTTP-H0)
S2/S3 (269) stay in the adjudication/ADR queue until S0/S1 is clear.

## needs-runtime (9) — LIVE WINDOW REQUESTED, still outstanding

AI-VERTEX-V2 (S1), AI-V1 (S1), AI-COPILOT-C6, TUI-28, TUI-30, TUI-33, TUI-TD06, TUI-TD15, TUI-TD28.
**User action needed (§0a step 4):** provide (a) an endpoint/credential sandbox able to exercise the live provider
paths, (b) a time slot, (c) whether cassette-only (ADR-0006) still stands for release. Without a window these 9
stay `needs-runtime` and §12.1 ("needs-runtime 队列清空，或全部有明确运行时结论") cannot close.

## Discipline (carry-over, still binding)

- **CI non-blocking (user ruling 2026-10-20):** never wait for CI during development. Lane local-green + independent
  verifier PASS ⇒ merge now; CI red ⇒ patch lane. The only CI wait is the §12 exit gate.
- All cargo/fmt/clippy/nextest via `tools/dev-gate.sh` (eternal130, CPUQuota 400%, MemoryMax 6G, global flock).
  Never run cargo as root. The dev-gate flock serializes gates — one gate scope on the box at a time.
- Process safety: no `kill`/`pkill`/`killall`/`fuser`, no bare PIDs, no killing non-owned processes; only owned
  child handles; group signals via `procutil::signal_group_filtered`. Never touch port 49374; tests bind
  `127.0.0.1:0`; never write real data dirs.
- Never mark `verified` from a fixer session. No skip/ignore/deleted-assertion greens. `accepted` requires an ADR.
- Upstream pinned to **2.0.24** (ref source `/opt/opencode-ref`, commit `e7a34f09bf`). No v2.0.25 mixing.
- Evidence: repro.sh + `red.log` + green artifact per entry under `/tmp/opencode/parity-evidence/<ID>/`.
- Known-red registry `tools/parity/goldens/p10/known-red.json`; skip ≠ pass (`OC_GATE_STRICT=1`).

## Blockers

- Live window for the 9 `needs-runtime` entries (user).
- Non-linux-x64 CI build target (SURF-PLATFORM) — §12.4 requires one; no code blocker, only workflow work.

## Next

1. Reconcile the 3 dispatched lanes + the adjudication batch when they return.
2. Merge `lane/db` for real once the verifier says CLEAR TO MERGE (resolve the one `docs/parity-ledger.jsonl`
   conflict — union of rows — then `tools/dev-gate.sh 'cargo fmt --all -- --check'` + a compile smoke).
   Re-record DB-D1/D2/D3/D4/D6/D12 `merge_commit` to the **real** merge sha (fix the phantom).
3. Close the adjudication batch as accepted+ADR / duplicate / (still-open → fix queue).
4. Next wave: PLUGIN residual (D13, D30, M02), TOOLS-EXEC TOOL-D1, SURFACES (browser, latex/mermaid, platform),
   the 3 p10 gate reds (GATE-DB-NULLROWS, GATE-DB-STABLE, GATE-TUI-RENAME), then CLI-K2/CLI-K8.
