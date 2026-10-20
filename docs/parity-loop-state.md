# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.
> **REBUILT from ledger+git on 2026-10-20 23:0x** (session-rotated; the previous
> copy was stale by one DB merge + one CI red). Ledger + git are authoritative;
> state.md is a cross-session breadcrumb only.

- **updated**: 2026-10-20 23:0x — recovery round: DB CI red attributed (bfcddfb), DB-R1 patch lane created, TUI WIP salvaged, CONFIG lane collapsed
- **baseline**: main @ `77efe8a` (fix(tools): cron-path binary probing + active-writer guard in 0a)
- **batch**: 3 (wave 8) — Batch 3 clusters: DB, PLUGIN-RESIDUAL, EVENT/TOOL/CORE, PLATFORM, POLICY/RETENTION
- **iteration**: 9 (driver = tools/loop-driver.sh, `opencode run` per iteration; previous run died rc=130 at 22:47)

## Ledger snapshot (`python3 tools/parity-ledger.py summary`, 2026-10-20 23:0x)

- rows=397 — verified 91 · accepted 1 · duplicate 1 · fixing 1 · needs-runtime 9 · open 294
- severity: S0 6 · S1 116 · S2 244 · S3 31 — **open S0/S1 = 32** (AI 1, CONFIG 7, CRED 3, DB 2, PLUGIN 3, RUNTIME 1, SURFACES 3, TOOLS-EXEC 1, TUI 9, WIRE 2) + new DB-R1 (fixing)
- `validate` (schema/state/evidence) = **0 errors**
- `validate --paths --exit` still blocked (32 open S1 + 1 fixing S1 + 9 needs-runtime)

## Recovery findings (§0a, 2026-10-20 23:0x) — READ BEFORE DISPATCHING

1. **The previous session was SIGINT-killed, not finished.** `tools/loop-driver/driver.log`:
   `20:33:24 iteration 1` → `22:47:12 opencode run failed rc=130; stopping for manual inspection`.
   Its child fixer session `ses_ea6a11064ffek4TtBlPRcp5CjQ` ("Fix TUI-4/5/6/7 in lane-tui2")
   has `time_updated = 22:47:12` — terminal, and no worktree write since 22:45 ⇒ **writer confirmed dead**
   (no `opencode run` process, no cargo/rustc, no process cwd in the worktree). → lane-tui2 was salvaged.
2. **lane-tui2 salvaged, not restarted.** Dirty TUI-5/6/7 production code (13 files, +969) committed
   verbatim as `e74082b WIP(tui2): salvage …`. Branch `lane/tui2` @ `e74082b`; worktree `.worktrees/lane-tui2`.
   Evidence dirs for TUI-4/5/6/7 still hold **only `red.log`** (all four byte-identical, 12845 B) — no green
   artifact, so TUI-4's "6/6 green" claim is still unproven.
3. **lane-config produced NOTHING.** Worktree was 0-ahead + clean, branch `lane/config` == main, zero
   evidence dirs for CLI-K1/N1/N2/N3/N4 ⇒ the previous CONFIG fixer never ran. Worktree removed; branch kept.
   The 5 CONFIG S1 entries are untouched and must be re-dispatched.
4. **CI IS RED — attributed.** Run `39425608284` (head `482869a`) nextest job failed:
   `oc-server::fixw1b_lane::cd34_plugin_commands_surface_and_execute_through_the_real_boot` and
   `oc-server::plugin_engine_mcp::headless_boot_lists_the_plugin_declared_mcp_server` (both 20s poll
   deadline, empty registrations; plugin itself loads "active"). Both **PASSED** in green run
   `39387166800` (head `77c3950`). `.github/` unchanged between the two heads ⇒ not a workflow/bun delta.
   ⇒ **First red commit = `bfcddfb` (merge: lane-db, wave-8 re-land)** — the only crates/ change.
   Ledger row **DB-R1** created (S1, cluster DB, status `fixing`) with the CI evidence attached.
5. **DB cluster is FROZEN** for new merges until green (prompt §5). The DB code was **not** reverted:
   its 6 entries keep their verified evidence and a reachable `merge_commit`; the regression gets its own
   ledger row and its own patch lane. If the fix changes DB-D2/D3/D12 semantics, those entries must be
   re-verified before exit (noted on all six rows).

## Active lanes (Batch 3, wave 8)

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| lane-dbfix | DB (patch) | `.worktrees/lane-dbfix` | `lane/dbfix` @77efe8a | fixer | DB-R1 | dispatched (foreground) |
| lane-tui2 | TUI (resume) | `.worktrees/lane-tui2` | `lane/tui2` @e74082b | fixer | TUI-4,5,6,7 | salvaged — resume next round |
| lane-db | DB (frozen) | `.worktrees/lane-db` | `lane/db` @7657230 | — | DB-D1..D12 | merged already (bfcddfb); branch kept for re-land |
| (read-only) | adjudication | — | — | oracle | 10 S2/S3 entries (see below) | dispatched (background) |

Active clusters (writers) = 1 (DB). Adjudication batch is read-only.
Hot-file owners: `crates/oc-tui/src/app.rs` + `crates/oc-tui/src/store.rs` = TUI (lane-tui2 only);
`crates/oc-core/src/{engine/mod.rs,project.rs,database/v1_migration.rs}` = DB (lane-dbfix only).
`docs/parity-ledger.jsonl` = **orchestrator-owned** — fixers must NOT edit it (report wiring instead).

## Adjudication batch (read-only, dispatched this round)

DB-D10, DB-D15, RUNTIME-ENVIRONMENT, RUNTIME-E01, TUI-19, CLI-F13, CLI-F14, SURF-P1-SDK,
SURF-P1-VSCODE, SURF-P1-STORYBOOK — zero-code closure candidates (already-equivalent / out-of-scope /
same-root-cause-as-verified). Each needs ref evidence from `/opt/opencode-ref` (pinned 2.0.24) and an ADR
(`docs/adr/0008+`) for every `accepted`. No ADR ⇒ no `accepted`.

## Open S0/S1 queue (32) — next-up order

CONFIG 7 (CLI-K1,K2,K8,N1..N4) · TUI 9 (TUI-4..7,12,14..17) · CRED 3 (D6,D8,V3) · PLUGIN 3 (D13,D30,M02) ·
SURFACES 3 (BROWSER,LATEX-MERMAID,PLATFORM) · DB 2 (GATE-DB-NULLROWS, GATE-DB-STABLE) · AI 1 (AI-P06) ·
RUNTIME 1 (RUNTIME-K01) · TOOLS-EXEC 1 (TOOL-D1) · WIRE 2 (GATE-TUI-RENAME, HTTP-H0)
S2/S3 (269) stay in the adjudication/ADR queue until S0/S1 is clear.

## needs-runtime (9) — LIVE WINDOW STILL OUTSTANDING (ask the user)

AI-VERTEX-V2 (S1), AI-V1 (S1), AI-COPILOT-C6, TUI-28, TUI-30, TUI-33, TUI-TD06, TUI-TD15, TUI-TD28.
**User action needed:** (a) an endpoint/credential sandbox able to exercise the live provider paths,
(b) a time slot, (c) whether cassette-only (ADR-0006) still stands for release. Without a window these 9
stay `needs-runtime` and §12.1 cannot close.

## Discipline (carry-over, still binding)

- **CI non-blocking (user ruling 2026-10-20):** never wait for CI during development. Lane local-green +
  independent verifier PASS ⇒ merge now; CI red ⇒ patch lane. The only CI wait is the §12 exit gate.
  (This round deviates deliberately on the DB red: the patch lane is the same thing, minus the revert churn.)
- All cargo/fmt/clippy/nextest via `tools/dev-gate.sh` (eternal130, CPUQuota 400%, MemoryMax 6G, global flock).
  Never run cargo as root. The dev-gate flock serializes gates — one gate scope on the box at a time.
- Process safety: no `kill`/`pkill`/`killall`/`fuser`, no bare PIDs, no killing non-owned processes; only owned
  child handles; group signals via `procutil::signal_group_filtered`. Never touch port 49374; tests bind
  `127.0.0.1:0`; never write real data dirs.
- Never mark `verified` from a fixer session. No skip/ignore/deleted-assertion greens. `accepted` requires an ADR.
- Upstream pinned to **2.0.24** (ref source `/opt/opencode-ref`, commit `e7a34f09bf`). No v2.0.25 mixing.
- Evidence: repro.sh + `red.log` + green artifact per entry under `/tmp/opencode/parity-evidence/<ID>/`.
- Known-red registry `tools/parity/goldens/p10/known-red.json`; skip ≠ pass (`OC_GATE_STRICT=1`).
- **Background subagents die with the session** (the driver ends the session at the final message). Use
  foreground for anything that must not be lost; background only for read-only work that can be redone.

## Blockers

- Live window for the 9 `needs-runtime` entries (user).
- Non-linux-x64 CI build target (SURF-PLATFORM) — §12.4 requires one; no code blocker, only workflow work.
- main CI red (DB-R1) — patch lane in flight; DB cluster frozen.

## Next

1. Collect DB-R1 fix → independent verifier (fresh session, read-only + repro only) → merge `lane/dbfix`
   when locally green → re-record merge_commit. If the fix changes DB-D2/D3/D12 semantics, re-verify those 6.
2. Collect the adjudication batch → write ADRs (`docs/adr/0008+`) → record `accepted`/`duplicate` rows.
3. Resume lane-tui2: finish TUI-5/6/7 production paths, then TUI-4 green artifact (repro + red + green).
4. Re-dispatch CONFIG (CLI-K1, N1, N2, N3, N4) from a fresh `.worktrees/lane-config`.
5. Then PLUGIN residual (D13, D30, M02), TOOL-D1, SURFACES, the 3 p10 gate reds, CLI-K2/CLI-K8.
