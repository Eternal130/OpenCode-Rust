# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.
> **REBUILT from ledger+git 2026-10-20 ~23:00, finalized ~00:0x** (session-rotated; previous copy was one
> DB merge + one CI red stale). Ledger + git are authoritative; state.md is a cross-session breadcrumb only.

- **updated**: 2026-10-20 ~00:0x — iteration 9 done: CI red root-caused & fixed (merge 063c9e1), 6 entries accepted+ADR, TUI WIP salvaged
- **baseline**: main @ `063c9e1` (merge: lane-dbfix) + the docs commit on top (ADRs + ledger)
- **batch**: 3 (wave 8) — Batch 3 clusters: DB, PLUGIN-RESIDUAL, EVENT/TOOL/CORE, PLATFORM, POLICY/RETENTION
- **iteration**: 9 (driver = tools/loop-driver.sh; previous run died rc=130 at 22:47, this one started 23:00)

## Ledger snapshot (`parity-ledger.py summary`, end of iteration 9)

- rows=398 — **verified 92 · accepted 7 · duplicate 2** · needs-runtime 9 · open 288 (fixing 0)
- open S0/S1 = **32** — unchanged queue: CONFIG 7 (CLI-K1,K2,K8,N1..N4) · TUI 9 (TUI-4..7,12,14..17) · CRED 3 (D6,D8,V3) ·
  PLUGIN 3 (D13,D30,M02) · SURFACES 3 (BROWSER,LATEX-MERMAID,PLATFORM) · DB 2 (GATE-DB-NULLROWS, GATE-DB-STABLE) ·
  AI 1 (AI-P06) · RUNTIME 1 (RUNTIME-K01) · TOOLS-EXEC 1 (TOOL-D1) · WIRE 2 (GATE-TUI-RENAME, HTTP-H0)
- `validate` = 0 errors; `dedupe` = 1 group (DB-R1 → PLUG-R1, correct)

## What happened in iteration 9 (read this before touching the DB or PLUGIN cluster)

1. **CI red 39425608284 root-caused and FIXED — and the recovery-round attribution was WRONG, corrected.**
   - Initial (wrong) hypothesis: the DB merge `bfcddfb` stalls boot via DB-D2/D3/D12.
   - Fixer reproduced locally (deterministic, both tests 20s-timeout) and DISPROVED it: boot settles <500ms,
     HTTP 200 throughout, plugin `active`, registrations present for the **boot project** the whole time.
   - True root cause: the tests poll `/api/command` + `/api/mcp` **location-less**; `resolve_location`
     (`crates/oc-server/src/handlers/common.rs:254`) falls back to the process cwd (`crates/oc-server`),
     which declares no plugins; **PLUG-D07** per-location activation (`670c840`, merged `b70f4b0`, wave 7)
     filters it to `Some([])`. `670c840` is NOT an ancestor of green head `77c3950` — it landed between green
     and red; the plugin-wave head's CI run (39420967951 @2340f0b) was CANCELLED, so the red surfaced at 482869a.
   - Fix `2fc1938` (lane/dbfix): TEST-ONLY — both polls now send `location[directory]=<boot project>`;
     one envelope-location assertion ADDED, none removed; **zero production delta**.
   - Independent verifier (`ses_ea610cb98ffecHr9NfcKzDEvX3`, own/glm-5.3-flash): **CLEAR TO MERGE** —
     claims 1–5 CONFIRMED, 2/2 + 6/6 + 1/1 green, artifacts checked (red.log.bak is the honest pre-fix red;
     repro.sh overwrites red.log by default — known hygiene nit).
   - Merged `063c9e1` (2-parent); fmt + clippy -D warnings clean. **DB cluster UNFROZEN** — the DB merge was innocent.
   - Ledger: canonical **PLUG-R1** (verified, merge_commit 063c9e1); **DB-R1** → duplicate of PLUG-R1 with the
     correction written down; DB-D1..D12 notes corrected (superseded wrong attribution).
2. **Adjudication batch #1 closed 6 of 10 with ADRs** (oracle `ses_ea62606a9ffe0omNO7zEgocC0`, zero code):
   - DB-D10 → `docs/adr/0008-kv-scan-consumer-equivalence.md`
   - DB-D15 → `docs/adr/0009-transaction-begin-immediate-default.md`
   - RUNTIME-ENVIRONMENT → `docs/adr/0010-environment-seam-not-ported.md`
   - RUNTIME-E01 → `docs/adr/0011-step-started-pending-delta-mutex.md` (closed by proof)
   - SURF-P1-SDK → `docs/adr/0012-no-npm-sdk-package.md`
   - SURF-P1-STORYBOOK → `docs/adr/0013-tui-storybook-not-ported.md`
   - Stays open with recorded fix scopes in their ledger notes: TUI-19 (sync-2026 wrap), CLI-F13 (stats tz+30s),
     CLI-F14 (channel build-time stamp; **same fingerprint as DB-D24 — merge rows when fixed**), SURF-P1-VSCODE.
3. **lane-tui2 salvaged**: dead-session WIP (13 files, +969) committed verbatim as `e74082b` on `lane/tui2`.
   TUI-4..7 evidence dirs still hold ONLY byte-identical red.log — TUI-4's "6/6 green" claim unproven.
4. **lane-config collapsed** (0-ahead, clean, zero evidence): CLI-K1/N1..N4 never started; re-dispatch fresh.

## Model policy for subagents (user ruling 2026-10-20 — keep for every future session)

- **fixer = `own/glm-5.3` · oracle = `own/glm-5.3` · verifier = `own/glm-5.3-flash`** (all via the own provider).
- Do NOT reuse free-gateway (`opencode/*`) models unless the user says so. The loop prompt's "verifier 用
  glm-5.3-flash" means `own/glm-5.3-flash`. Always look models up with `tools.opencode.models({provider:"own"})`
  instead of trusting an old filtered query.

## Active lanes

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| (none — all worktrees removed post-merge; branches kept) | | | | | | |

- `.worktrees/lane-dbfix` and `.worktrees/lane-db` removed after merge (clean trees). Branches `lane/dbfix`, `lane/db`, `lane/tui2`, `lane/config` kept.
- **Next dispatches**: (1) lane-tui2 resume — finish TUI-5/6/7 production paths on `lane/tui2` @`e74082b`, then TUI-4 green artifact; (2) fresh CONFIG lane for CLI-K1,N1,N2,N3,N4 (worktree re-create from main); (3) next adjudication batch (wave rule: one per wave — already done for wave 8).

## needs-runtime (9) — LIVE WINDOW STILL OUTSTANDING (ask the user, don't stall the loop)

AI-VERTEX-V2 (S1), AI-V1 (S1), AI-COPILOT-C6, TUI-28, TUI-30, TUI-33, TUI-TD06, TUI-TD15, TUI-TD28.
**User action needed:** (a) endpoint/credential sandbox for the live provider paths, (b) a time slot,
(c) whether cassette-only (ADR-0006) still stands for release. Without a window these stay `needs-runtime`
and §12.1 cannot close.

## Discipline (carry-over, still binding)

- **CI non-blocking (user ruling 2026-10-20):** never wait for CI during development; lane local-green +
  independent verifier PASS ⇒ merge; CI red ⇒ patch lane. Only the §12 exit gate waits for CI.
  Lesson from this iteration: when a red appears, attribute with `git merge-base --is-ancestor` per suspect
  commit + the CANCELLED runs list, NOT with "the only merge since green" (that produced the DB misattribution).
- All cargo/fmt/clippy/nextest via `tools/dev-gate.sh` (eternal130, CPUQuota 400%, MemoryMax 6G, global flock).
  Never run cargo as root. One gate scope on the box at a time.
- Process safety: no `kill`/`pkill`/`killall`/`fuser`, no bare PIDs, only owned child handles; group signals
  via `procutil::signal_group_filtered`. Never touch port 49374; tests bind `127.0.0.1:0`; no real data dirs.
- Never mark `verified` from a fixer session. No skip/ignore/deleted-assertion greens. `accepted` requires an ADR.
- Upstream pinned **2.0.24** (`/opt/opencode-ref`, commit `e7a34f09bf`).
- Evidence per entry under `/tmp/opencode/parity-evidence/<ID>/` (repro.sh + red + green). Fix repro.sh scripts
  that tee to red.log by default — write the post-fix log elsewhere (PLUG-R1 hygiene nit).
- Known-red registry `tools/parity/goldens/p10/known-red.json`; skip ≠ pass (`OC_GATE_STRICT=1`).
- **Background subagents die with the session** — foreground for anything that must not be lost.

## Blockers

- Live window for the 9 `needs-runtime` entries (user).
- Non-linux-x64 CI build target (SURF-PLATFORM) — workflow work only.
- CI on the just-pushed head should go green (the red's fix landed); if it is red again, run the §5 protocol
  with per-commit attribution (see the lesson above).

## Next

1. Resume lane-tui2 (TUI-5/6/7 then TUI-4 evidence); re-dispatch CONFIG (CLI-K1,N1..N4).
2. Then PLUGIN residual (D13, D30, M02), TOOL-D1, SURFACES (browser, latex/mermaid), 3 p10 gate reds, CLI-K2/K8.
3. Next wave's adjudication batch from the S2/S3 pool (288 open; 6 closed this way today).
4. Check CI on the new main head; record run id here.
