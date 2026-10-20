# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.
> **REBUILT from ledger+git 2026-10-21 05:2x** (recovery round + wave 9).
> Ledger + git are authoritative; state.md is a cross-session breadcrumb only.

- **updated**: 2026-10-21 05:20 — iteration 10 done: recovery round (lane-cirt collapsed, lane-tui2 adopted),
  TUI-4/5/6 verified + merged (`6a58662` → L0 fix `ad1ac69`), adjudication batch #2 closed 7 via ADR-0014,
  3 AI needs-runtime adjudicated to real gaps, TUI-7 blocked by new root cause **TUI-35**
- **baseline**: main @ `ad1ac69` (merge: lane/tui2 + post-merge clippy fix) on top of `35e6343` (CI-RUNTIME)
- **batch**: 3 (wave 9) — Batch 3 clusters: DB, PLUGIN-RESIDUAL, EVENT/TOOL/CORE, PLATFORM, POLICY/RETENTION
- **iteration**: 10 (driver = tools/loop-driver.sh; session started 2026-10-21 00:00, ended ~05:2x)

## Ledger snapshot (`parity-ledger.py summary`, end of iteration 10)

- rows=402 — **verified 95 · accepted 14 · duplicate 2** · open 284 · needs-runtime 6 · blocked 1
- **blocking S0/S1 = 33** (was 32; +AI-V1 +AI-VERTEX-V2 +CI-RUNTIME-AI-LIVE +TUI-35, −TUI-4/5/6):
  AI 4 (AI-P06, AI-V1, AI-VERTEX-V2, CI-RUNTIME-AI-LIVE) · CONFIG 7 (CLI-K1,K2,K8,N1..N4) ·
  CRED 3 (D6,D8,V3) · TUI 7 (TUI-12,14,15,16,17,35 + TUI-7 blocked) · PLUGIN 3 (D13,D30,M02) ·
  SURFACES 3 (BROWSER,LATEX-MERMAID,PLATFORM) · DB 2 (GATE-DB-NULLROWS, GATE-DB-STABLE) ·
  WIRE 2 (GATE-TUI-RENAME, HTTP-H0) · RUNTIME 1 (RUNTIME-K01) · TOOLS-EXEC 1 (TOOL-D1)
- `validate` = 0 errors; `validate --paths` = 0 errors (one path row corrected, see §CI-RUNTIME below)

## What happened in iteration 10

1. **Recovery round (0a).** `lane-cirt` (0-ahead, clean, 0 ledger rows, created 23:54 by a dead session) was
   removed — worktree + branch. All other lane branches are 0-ahead of main. `lane-tui2` was the only lane with
   unmerged work (3 commits ahead, quiet since 22:47 on 10-20 → not an active writer); its WIP was mid-edit with
   only a shared byte-identical red.log as evidence → **打回 fixing** and re-dispatched in place.
2. **TUI lane (lane-tui2) closed 3 of 4.**
   - Fixer (`own/glm-5.3`, ses_ea5ed006cffeX7QH3daofRNUsn): finished the salvaged production code —
     TUI-4 TerminalPane (`terminal_pane.rs` + registry/WS-attach/keys, `pty_terminal_pane.rs` E2E),
     TUI-5 `open.menu(ctrl+o)` → DialogOpen sessions+projects(+worktrees) (`dialog_open.rs`),
     TUI-6 session-list rows/grouping + **boot-time `project.list` sync** (`app.rs` `LoopRequest::ProjectList`).
     Commits: `06f1bcd`, `6337909`, `ad536ce`.
   - Verifier (`own/glm-5.3-flash`, ses_ea4ddef65ffehscWZOxfAS4sUJ, read-only, independent session):
     **CLEAR TO MERGE** — 3/3 repro.sh re-runs 1/1 green, per-ID red artifacts honest, production call sites
     proven, **no removed/weakened assertions**, `crates/oc-tui-core` untouched. Caveats recorded in the ledger:
     green.log = shared 4-test run; worktrees section (TUI-5) and per-row time/model + ● + pin/delete (TUI-6)
     are implemented in production but not asserted by the tests.
   - Merged `6a58662`; **post-merge L0 gate caught 2 clippy errors** (`semicolon_if_nothing_returned`,
     app.rs:6770 + store.rs:3981) that the lane's earlier clippy run had hidden behind cargo's lint cache.
     Fixer disclosed the false-positive; fixed in `e79b7a9`, re-linted fresh → `ad1ac69`. Main gate now:
     `cargo fmt --all -- --check` rc=0 · `cargo clippy -p oc-tui --tests --locked -- -D warnings` rc=0 (fresh
     `Checking oc-tui`) · `cargo check --workspace --locked` rc=0.
   - **TUI-7 could not be closed**: its contract IS implemented and wired, but its own regression test never
     reaches the dialog — the TUI **boot hangs** nondeterministically on a quiet fake server (connection runner's
     first cycle never completes → "Connection lost" scrim forever → exit chord ignored → 90s harness timeout).
     New ledger row **TUI-35** (S1, TUI) owns that root cause; TUI-7 = `blocked`.
3. **Adjudication batch #2 (wave 9, zero code)** — oracle ses_ea5edbb25ffe5wjSBsn037auEY:
   - **7 accepted via ADR-0014** (`docs/adr/0014-ts-monorepo-nonproduct-artifacts.md`): SURF-P2-SCRIPT,
     SURF-P2-SIMULATION, SURF-P2-DRIZZLE, SURF-P2-IDENTITY, SURF-P3-CONSOLE, SURF-P3-POSTS, SURF-P1-GHA —
     all non-product TS-monorepo artifacts (publishing scripts, dev harness, brand assets, hosted console/blog,
     GitHub Action = distribution channel family per ADR-0012).
   - **3 needs-runtime → open with recorded fix scope** (static adjudication, no live window needed):
     AI-V1 + AI-VERTEX-V2 (S1, vertex URL/auth arms missing in `provider_catalog/facade.rs`; ADC token sourcing
     stays CRED-V3 — same lane, different root cause) · AI-COPILOT-C6 (S2, copilot providerOptions namespace +
     User-Agent header).
4. **⚠ Concurrent orchestrator session detected.** While this session ran (00:00–05:2x) **another orchestrator
   session** committed to `main` and `origin/main`: `c95cf1e` (merge lane-cirt: `.github/workflows/runtime-gates.yml`,
   828 lines — p10 battery + §12.3 runtime gates + needs-runtime battery + macos build target + opt-in soak) and
   `35e6343` (merge lane-cirt2: secrets-in-if 422 fix). It left **uncommitted** ledger rows (CI-RUNTIME-TUI-TESTS,
   CI-RUNTIME-AI-LIVE, CI-RUNTIME-SOAK) and never wrote state.md. Those rows were preserved and committed here
   (`c07d73b`); CI-RUNTIME-AI-LIVE's wrong rust paths were corrected so `validate --paths` is clean.
   **Structural hazard for the next session: two orchestrators writing one ledger + one main.** The driver flock
   only serializes driver fires, not a hand-started session. Next session must re-read main + ledger before
   dispatching, and must treat the CI-RUNTIME workflow as landed (do not re-implement it).

## Active lanes

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| (none — lane-tui2 merged as ad1ac69; worktree removable, branch lane/tui2 kept) | | | | | | |

- `.worktrees/lane-tui2` — merged, tree clean; safe to remove (branch kept for the next TUI lane).
- Branch `lane/cirt2` still exists (0 ahead of main after merge) — can be deleted.

## Model policy for subagents (user ruling 2026-10-20 — keep for every future session)

- **fixer = `own/glm-5.3` · oracle = `own/glm-5.3` · verifier = `own/glm-5.3-flash`** (all via the own provider).
- There is **no `verifier` subagent** in this build — the verifier role was played by a `fixer` instance with
  `own/glm-5.3-flash` and a strict read-only mandate (report only, no writes in the repo).
- Do NOT reuse free-gateway (`opencode/*`) models unless the user says so. Look models up with
  `tools.opencode.models({provider:"own"})` instead of trusting an old filtered query.

## needs-runtime (6) — LIVE WINDOW STILL OUTSTANDING (asked of the user; not a loop blocker)

AI-COPILOT-C6 moved to open (adjudicated). Remaining: TUI-28, TUI-30, TUI-33, TUI-TD06, TUI-TD15, TUI-TD28 (all S2)
plus the CI-RUNTIME-AI-LIVE gap row (S1). **User action needed:** (a) endpoint/credential sandbox for the live
provider paths (the CI-RUNTIME workflow expects `OC_CI_VERTEX_PROJECT` / `OC_CI_VERTEX_CREDENTIALS` /
`OC_CI_GEMINI_API_KEY` / `OC_CI_COPILOT_TOKEN`), (b) a time slot, (c) whether cassette-only (ADR-0006) still
stands for release. Without a window these stay as-is; §12.1 accepts them if each gets an explicit runtime
conclusion (ADR).

## Discipline (carry-over, still binding)

- **CI non-blocking (user ruling 2026-10-20):** never wait for CI during development; lane local-green +
  independent verifier PASS ⇒ merge; CI red ⇒ patch lane. Only the §12 exit gate waits for CI.
- All cargo/fmt/clippy/nextest via `tools/dev-gate.sh` (eternal130=uid 1000, CPUQuota 400%, MemoryMax 6G,
  global flock, `OPENCODE_DEV_GATE_DIR=<worktree>` for lanes). Never run cargo as root.
  Root-owned files in a worktree break the eternal130 gate → `chown -R 1000:1000 <worktree>` after editing.
- **Never trust a cached lint pass**: the lane's clippy "clean" was a cache hit; require a fresh `Checking <crate>`
  line in the output before believing a clippy/fmt result.
- Process safety: no `kill`/`pkill`/`killall`/`fuser`, no bare PIDs, only owned child handles; group signals via
  `procutil::signal_group_filtered`. Never touch port 49374; tests bind `127.0.0.1:0`; no real data dirs.
- Never mark `verified` from a fixer session. No skip/ignore/deleted-assertion greens. `accepted` requires an ADR.
- Upstream pinned **2.0.24** (`/opt/opencode-ref`, commit `e7a34f09bf`).
- Evidence per entry under `/tmp/opencode/parity-evidence/<ID>/` (repro.sh + own red.log + green.log).
  A shared/copied red.log is NOT acceptable evidence.
- Known-red registry `tools/parity/goldens/p10/known-red.json`; skip ≠ pass (`OC_GATE_STRICT=1`).
- **Background subagents die with the session** — foreground for anything that must not be lost.

## Blockers

- Live window for the 6 `needs-runtime` entries + CI-RUNTIME-AI-LIVE secrets (user).
- Non-linux-x64 CI build target (SURF-PLATFORM) — the CI leg is now wired in runtime-gates.yml (platform-build
  job, macos-latest) but the row stays open until the first GREEN run id is recorded.

## Next

1. **Fresh CONFIG lane** (7 open S1: CLI-K1,K2,K8,N1,N2,N3,N4 — `crates/oc-cli/**` + `crates/oc-server/**`;
   CLI-K8 also touches `crates/oc-tui/src/storage.rs`, and the TUI cluster is now free since lane-tui2 merged).
2. **TUI-35** (boot hang) — unblocks TUI-7; same TUI cluster, reuse branch `lane/tui2` or a fresh worktree.
3. **AI vertex lane** — AI-V1 + AI-VERTEX-V2 (+ CRED-V3 ADC) in one lane; `crates/oc-ai/src/protocols/**` +
   `crates/oc-core/src/provider_catalog/facade.rs`.
4. Next adjudication batch (wave 10) from the S2/S3 pool (284 open; 7 closed this way in wave 9).
5. Check CI on the pushed head; record run ids here.
