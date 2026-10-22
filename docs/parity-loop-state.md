# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.
> **REBUILT from ledger+git 2026-10-21 09:30** (iteration 13 / wave 12 opened).
> Ledger + git are authoritative; state.md is a cross-session breadcrumb only.

## WAVE 14 (iteration 15) — CLOSED: §12 FULL GREEN

- **runtime-gates 39728809002 = SUCCESS, p10 battery 11/11 — the first full-green §12 run.**
  Final piece: cross_tui's reference TUI died under bun 1.4.2 because the harness's
  `--define OPENCODE_VERSION="2.0.24"` lost its quotes through sh -c (1.4.x parses define
  values as JSON; 1.3.10 had silently parsed the bare 2.0.24 as the number 2 — the old boot
  needle was matching the 'client is local' WARNING). lane/xui fixed the quoting + rebuilt
  the tmux rig (fail-fast dying-words diagnostics, fd hygiene).
- Terminal ledger state: **verified 149 · fixed 0 · S1 open 0 · accepted 17 · duplicate 3**;
  remaining opens are S2/S3 longtail + blocked env switches + user-scheduled SURF P1-P3.
- GATE-DB-FIXTURE-WAL + CI-RUNTIME-GATES-RED + CLI-N4 verified on the run.

## WAVE 14 (iteration 15) — CI forensics chain (bun runner generation breakage)

- **The breakage**: starting 2026-10-21 19:23Z, GitHub ubuntu-24.04 runners broke bun 1.3.10
  (install hangs post-extract AND reference runtime POST hangs) with ZERO repo changes. bun
  1.4.2 (= v2.0.24's own packageManager pin) works for servers but crashed the opentui
  reference TUI on the OLD shared install cache (key had no version).
- **Fixes landed (each proven by a CI round)**: gate5 harness drains child pipes (cold-cache
  TS recompile writes MBs of stderr; an unread 64KiB pipe blocked the child pre-bind) ·
  ReferenceSpawn.extra_args + readiness probe targeting the server's own `listening on` URL ·
  **minih IPv6 literal bug (THE gate5 root cause)**: brackets not stripped for
  ToSocketAddrs — our serve binds ::1 for --hostname localhost (v2-faithful) and every
  http://[::1]:port poll failed to connect; fixed + pinned against a real loopback listener ·
  BUN_PIN=1.4.2 single env driving setup-bun + a version-keyed install cache · harvest
  requires session_message>=1 (a message-less fixture panicked the §12.3 census with a
  misleading error) · zed_context CI wedge = exit_chord teardown race (stacked toast + dirty
  draft swallowed the chord) — systematically fixed in vt_harness (all PTY tests benefit).
- **Battery state after run 39724870814**: gate5 ✅ db_interop ✅ http_contract ✅ openapi ✅
  tui_frames ✅ tools ✅ cross_client ✅ db_import ✅ error_paths ✅ — **only cross_tui red**
  (reference TUI crash under bun 1.4.2; lane-xui reproducing locally).
- TUI S2 longtail: lane/tui-s2 merged (16 items) + lane/tui-s2 follow-up; ci 39714598221
  6/6 → 14 rows verified. SURF complete (mermaid state/gitgraph/gantt/timeline). Ledger:
  verified 146 · fixed 3 · S1 open 0.

## WAVE 13 (iteration 14) — RESULT (ceremony-dropped mode, in flight)

- **User ruling 2026-10-21**: parity-loop process dropped (no loop driver, no verifier ceremony,
  no serial-merge ritual). Direct flow: fix lane → orchestrator diff review → local gates → merge
  → push; **ci green run promotes `fixed` → `verified`**. Machine-safety disciplines kept.
  **Post-merge `chown -R 1000:1000` on merge-written files is MANDATORY** (root orchestrator
  merges write root-owned files that break the eternal130 gate — bit us once this wave).
- **Merged this wave** (all local-green + diff-reviewed): config6 `ccf16fd` · db3 `18d9c4d` ·
  k01 (RUNTIME-K01 + GATE-TUI-RENAME harness probe) `d60ff57` · cred (D6/D8/V3) `8ed6546` ·
  plug (D13/D30/M02) `9319dc0` + lint-hardening `9d5cb66` · tui7 probe fix (two-phase repaint
  wait) · ai (VERTEX-V2/V1/P06) `6250363` · misc (TOOL-D1 + RUNTIME-FDLEAK, spawn fix actually
  in oc-server/src/lifecycle.rs) `948c11d` · h0 (HTTP-H0 full validation rollout) `d2aebda`.
- **ci 39572597237 on b1d8754 = 6/6 SUCCESS** → **18 rows promoted to `verified`** (incl. the
  wave-12 six). Ledger: verified 118 · open 263 · fixed 2 (GATE-DB-FIXTURE-WAL +
  CI-RUNTIME-GATES-RED, both gated on green runtime-gates §12 legs) · fixing 1 (CLI-N4).
- **runtime-gates instability (NOT attributed yet)**: run 39554282298 §12.3 GREEN, run
  39572601969 §12.3 RED (`real_db_null_census` line 171 "at least one probed session directory
  exists" — fixture staging env, test code unchanged between the two runs) + p10 battery red on
  `p10_openapi_semantic_equivalence` (1.3s fast-fail, needs fixture-stage log forensics).
  Previous-run db legs (stage + §12.3) proved GATE-DB-FIXTURE-WAL's harvest fix once.
- **In flight**: lane-tui (TUI-12/14/15/16/17) · lane-h0 merged. Remaining S1 queue:
  SURF-LATEX-MERMAID (after lane-tui, oc-tui serialized) · CLI-N4 disposition ·
  CI-RUNTIME-AI-LIVE (needs live window — user action).

## WAVE 12 (iteration 13) — RESULT (PAUSED by user before verify/merge; no new subagents dispatched)

- **Recovery (0a) result**: `git worktree list` → only main; all 15 pre-existing `lane/*` branches are
  **0-ahead of main**; `git status --short` on main = clean; `.worktrees/` empty → **no orphan lane, nothing to
  adopt, nothing to remove**. Ledger rebuilt from `parity-ledger.py summary` (406 rows) — state.md is consistent
  with git, no discrepancy found.
- **CI verdicts observed at open**: `ci` **39525333640** (head 2bd2d3e) = **SUCCESS 6/6** (fmt+clippy, guards,
  build+archive, frame-budget, nextest, nextest-pty) → main is green; `runtime-gates` **39525125599** (head
  b72ac8b) still **in_progress** at open (macos leg ✅, stage v2-db ✅, needs-runtime AI ✅, needs-runtime TUI ❌,
  §12.3 ❌, p10 battery still running). Read its final verdict at the next iteration open.
- **Blocking S0/S1 = 27** (all S1, no S0 open). Wave 12 units:
  1. **FIX lane `lane-config6`** · branch `lane/config6` · worktree `/opt/opencode-rust/.worktrees/lane-config6`
     (from `main@2bd2d3e`, chown 1000:1000) — CONFIG cluster, **6 packaged rows**: `CLI-K1` (serve --hostname,
     on the §12.2 p10-battery critical path), `CLI-K8`, `CLI-N1`, `CLI-N2`, `CLI-N3`, `CLI-N4`.
     All six share the regression file `crates/oc-cli/tests/cli_config.rs` → they must land in ONE lane (1 writer
     per cluster). Ledger rows set `fixing`, owner=lane-config6 (ledger commit below).
  2. **FIX lane `lane-db3`** · branch `lane/db3` · worktree `/opt/opencode-rust/.worktrees/lane-db3`
     (from `main@2bd2d3e`, chown 1000:1000) — DB cluster, **1 row**: `GATE-DB-FIXTURE-WAL` (the §12.2 p10-battery
     + §12.3 real-v2-import blocker: the CI harvest copies only the main db file and drops the SQLite `-wal`/`-shm`
     sidecars → 4096-byte 0-table shell fixture). Write scope: `.github/workflows/runtime-gates.yml`,
     `tools/parity/tests/p10_db_import.rs`, `crates/oc-server/tests/real_db_null_census.rs`.
     **No path overlap with lane-config6** (different crates/files) → the two lanes may run in parallel.
  3. **ADJUDICATION batch #5** (oracle, read-only, zero code) — S1 rows from the clusters that are NOT being fixed
     this wave, each verdict against `/opt/opencode-ref` v2.0.24 only: accepted+ADR / duplicate / keep-open+fix scope.
- **Write-scope / hot-file check**: lane-config6 writes `crates/oc-cli/src/**`, `crates/oc-tui/src/storage.rs`,
  `crates/oc-server/src/**`; lane-db3 writes `.github/workflows/runtime-gates.yml`, `tools/parity/tests/**`,
  `crates/oc-server/tests/**`. No overlap. No hot file from §10.3 is touched by either lane
  (`crates/oc-core/src/hooks.rs`, `session/store.rs`, `jobs.rs`, `oc-server/src/handlers/session.rs`,
  `oc-server/src/plugins.rs`, `plugin-host/src/supervisor.rs`, `oc-tui/src/app.rs`, `oc-tui/src/store.rs`,
  `routes.gen.rs` (generated), `Cargo.toml`/`Cargo.lock` (integrator)) → **both lanes are legal to run in parallel**.
- **Ledger ownership**: the ORCHESTRATOR is the only writer of `docs/parity-ledger.jsonl` this wave; lanes must not
  touch it.

### Wave 12 result (iteration 13, paused by user request after the fix lanes returned)

- **Unit 1 — lane-config6 (fixer own/glm-5.3, ses_ea3e51d2dffe7lxPi7dvsjT1om)**: 6/6 rows committed, tree clean,
  one commit per row (CLI-K1 0477aca · CLI-N1 e65bdd7 · CLI-N2 f3a0273 · CLI-N3 fa3d5d9 · CLI-N4 32f15ca ·
  CLI-K8 a99a751), evidence complete per ID (/tmp/opencode/parity-evidence/<ID>/: repro.sh + red.log + green.log +
  green-test.log). Orchestrator §2F check PASSED: commits carry ledger IDs; production call sites spot-checked
  (resolve_hostname boot.rs:451→serve.rs:73,81 · server_process_env.rs declared lib.rs:23 · heal_prompt_state_files
  storage.rs:331 wired at :458 · OPENCODE_PRINT_LOGS standalone.rs:112 · report_error run/mod.rs:356 ·
  entrypoints plugin.rs:183); consolidated `nextest --test cli_config` 8/8; fmt/clippy(oc-cli,oc-server,oc-tui) rc=0.
  **CLI-N4 is PARTIAL**: ~5 switches wired, 4 blocked (no consumer subsystem: FILEWATCHER→CONFIG-C3, DISABLE_FFF,
  GIT_BASH_PATH, SIMULATE) + oc-core engine seam gap → row KEPT `fixing` (--keep-status evidence recorded).
  Ledger: CLI-K1/N1/N2/N3/K8 → **fixed** (awaiting independent verification).
- **Unit 2 — lane-db3 (fixer own/glm-5.3, ses_ea3e51d2bffePeV6jK7P8Vco5a)**: GATE-DB-FIXTURE-WAL fixed in 1 commit
  (6cecd1e): harvest ships sidecars + checkpoints the STAGED copy + validates migration≥1 & session_v2≥1 twice
  (fails loudly instead of uploading a shell); upload glob `v2-library.db*`; both consumers copy sidecars next to the
  sandbox main; 2 hermetic pins; real-fixture end-to-end green (48 migrations, journal 0 re-runs). Ledger → **fixed**.
  Residual (fixer-stated): the harvest step itself is only provable on a real runner → dispatch `runtime-gates`
  after merge.
- **Unit 3 — adjudication batch #5 (oracle own/glm-5.3, ses_ea3e51d29ffeAFW4BBjc2PBcSf, read-only)**:
  **0 accepted / 0 duplicate / 10 keep-open** — honest: every row's v2 surface is implemented upstream and missing
  in our port. Each of the 10 rows now carries a two-sided file:line fix scope in its ledger notes
  (CRED-D6, CRED-V3, PLUG-D30, PLUG-D13, PLUG-M02, RUNTIME-K01, HTTP-H0, TOOL-D1, SURF-LATEX-MERMAID,
  GATE-TUI-RENAME). Cheapest next S1s identified: **RUNTIME-K01** (one-flag fix) and **GATE-TUI-RENAME**
  (rescoped to a HARNESS PROBE repair: v2's rename dialog prefills currentTitle with no select-all —
  the artifact shows the REFERENCE TUI itself submitted the concatenation; product contract verified v2-faithful).
- **CI verdicts recorded**: `ci` **39525333640** (2bd2d3e) = **SUCCESS 6/6** (main green at open).
  `runtime-gates` **39525125599** (b72ac8b) = **failure** — macos ✅ · build+archive ✅ · stage v2-db ✅ ·
  needs-runtime AI ✅ · needs-runtime TUI ❌ (the 6 S2 rows needing the live window) · p10 battery ❌
  (GATE-TUI-RENAME + CLI-K1 + GATE-DB-FIXTURE-WAL — all three now have fixes on lanes awaiting verify/merge) ·
  §12.3 ❌ (fixture WAL) · summary ❌ (derived). CI-RUNTIME-GATES-RED stays `fixed` (contract = all §12 legs green).
- **New ledger rows (3)**: RUNTIME-FDLEAK (S1 — managed-service spawn leaks inherited fds; a killed gate command can
  wedge the global dev-gate lock via the surviving `serve --service` daemon; hit twice by the fixer),
  AI-CATALOG-ENVLESS (S2, probe-first: env-less provider marked unavailable, catalog.rs:159),
  CLI-JSON-KEYORDER (S3, cosmetic key-order diff; ADR candidate at exit).
- **Hygiene**: stale merged-lane target dirs removed (~307G freed; 352G free now); kept target-main + target-lane-db.
- **PAUSED HERE (user instruction, 2026-10-21)**: independent verification (§2G, glm-5.3-flash read-only sessions)
  and serial merges (§2H) for BOTH lanes are deferred. Both lanes are committed + clean → the next session's 0a
  recovery adopts them directly ("lane with commits, clean tree → dispatch verifier → merge").

## WAVE 11 (iteration 12) — RESULT

- **Recovery (0a) result**: `git worktree list` → only main; `.worktrees/` was empty at start;
  all 15 `lane/*` branches are **0-ahead of main** → **no orphan lane, nothing to adopt**.
  `git status --short` on main = clean. So: nothing to salvage, nothing to remove.
- **Units this iteration**: (1) DB gate lane `lane-db2` (fix → verify → merge);
  (2) adjudication batch #4 (oracle, zero code); (3) ledger promotions on the green CI runs that
  landed during iteration 11; (4) one new ledger row from the CI artifact itself.
- **Fix lane**: `lane-db2` · branch `lane/db2` · worktree `/opt/opencode-rust/.worktrees/lane-db2`
  (from `main@7036d96`, chown 1000:1000) · ledger rows `GATE-DB-NULLROWS`, `GATE-DB-STABLE`
  claimed `fixing`, owner=lane-db2 (ledger commit `5024009`).
  Root cause found: **harness-side, not product** — the reference resolves its data root as
  `XDG_DATA_HOME/opencode` (`/opt/opencode-ref/packages/util/src/global-roots.ts`) while the port
  resolves `XDG_DATA_HOME/opencode-rust` (`crates/oc-util/src/global_roots.rs:29`, `APP_NAME`),
  so the "shared" home silently held **two** databases and every port write landed in the
  one-way-migration copy → the reference never saw the port's model switch / session B.
  Fix: `p10_common::share_data_home` aliases the port's data root onto v2's with a symlink
  (both channel-scheme resolutions land on the same physical file) + revival of dead fixture
  directories (guarded to `/tmp/opencode/`) + removal of the two now-green known-red registrations.
- **Result**: fixer `f9d83b3` → independent verifier (`own/glm-5.3-flash ses_ea4134d3effe5DvPu5UoZ2gEqX`)
  **CLEAR TO MERGE, 0 blocking defects** (per-hunk no-weakening audit; the two recalibrated census
  assertions are exact-equality no-drop pins against an independent SQL count, verified live) →
  **merged `315ff89`** → ledger `GATE-DB-NULLROWS` + `GATE-DB-STABLE` = **verified** (merge commit recorded).
  Post-merge gate: fmt rc=0 · clippy(parity,oc-server) rc=0 (fresh `Checking` lines) ·
  `cargo check --workspace --locked` rc=0. Pushed `7036d96..b72ac8b`.
  Honest defect recorded: `GATE-DB-STABLE/junit/` is empty (the structured artifact exists only under
  GATE-DB-NULLROWS/junit/); both repro scripts run the identical nextest invocation and the verifier
  re-ran both independently (2/2 green).
- **CI dispatched for the merge head `b72ac8b`**: `ci` run **39525091316** (push) · `runtime-gates` run
  **39525125599** (workflow_dispatch). Async, non-blocking — record their verdicts next session.
- **Ledger ownership**: the ORCHESTRATOR is the only writer of `docs/parity-ledger.jsonl` this wave
  (the lane did not touch it — verified in the lane diff).
- **Adjudication batch #4 (oracle, read-only)**: 8 open S1 rows, surface-oriented candidates
  (not dead-config-key rows — wave-11 method note: 1/10 closure on that class).
  Candidates: SURF-BROWSER, SURF-LATEX-MERMAID, TUI-15, TUI-16, TUI-17, TUI-12, CLI-K2, CRED-D8.
  Deliverable: per-row verdict (accepted+ADR / duplicate / keep-open + fix scope) against
  `/opt/opencode-ref` v2.0.24 evidence only. No ADR without evidence; no "too hard" accepted.

- **updated**: 2026-10-21 10:10 — iteration 13 (wave 12) PAUSED by user after the fix lanes returned:
  lane-config6 (5 rows fixed + CLI-N4 partial) + lane-db3 (GATE-DB-FIXTURE-WAL fixed) committed & clean,
  adjudication #5 done (10 keep-open scopes, 0 closures), 3 new rows. Verification + merge deferred to the
  next session. **blocking S0/S1 = 22 open/fixing + 7 fixed-awaiting-verify (CI-RUNTIME-GATES-RED + 6 wave-12)**.
- **baseline**: main @ ledger/state commit on top of `2bd2d3e`; lanes: lane/config6 (6 commits), lane/db3 (1 commit)
- **batch**: 3 (wave 12) — DB, CONFIG + cross-cluster adjudication
- **iteration**: 13 (driver = tools/loop-driver.sh; session paused mid-wave by user instruction)

## Ledger snapshot (`parity-ledger.py summary`, end of iteration 13 / wave 12 — PAUSED)

- rows=409 — **verified 100 · accepted 16 · duplicate 3 · fixed 7** (CI-RUNTIME-GATES-RED + wave-12: CLI-K1/N1/N2/N3/K8, GATE-DB-FIXTURE-WAL) · fixing 1 (CLI-N4 partial) · needs-runtime 6 · open 276
- **blocking S0/S1 (not yet verified/accepted) = 27** (was 30 at the start of this iteration):
  CONFIG 6 (CLI-K1, CLI-K8, CLI-N1..N4) · TUI 5 (TUI-12, TUI-14, TUI-15, TUI-16, TUI-17) · AI 4 (AI-P06, AI-V1,
  AI-VERTEX-V2, CI-RUNTIME-AI-LIVE) · CRED 3 (D6, D8, V3) · PLUGIN 3 (PLUG-D13, PLUG-D30, PLUG-M02) ·
  WIRE 2 (HTTP-H0, GATE-TUI-RENAME) · DB 1 (GATE-DB-FIXTURE-WAL) · SURFACES 1 (SURF-LATEX-MERMAID) ·
  RUNTIME 1 (RUNTIME-K01) · TOOLS-EXEC 1 (TOOL-D1)
- **promoted to `verified` this iteration** (green CI): TUI-7, TUI-35 (CI 39519807488 = 6/6 jobs SUCCESS) and
  SURF-PLATFORM (runtime-gates 39519736380, macos-latest leg green).
- **1 row is still `fixed`** (needs a green run id): CI-RUNTIME-GATES-RED — its contract is "every runtime-gates §12 leg
  green" and the p10 battery / §12.3 / gates-summary legs are still red.
- **2 rows closed this iteration**: GATE-DB-NULLROWS + GATE-DB-STABLE → `verified` (merge `315ff89`, independent
  verifier CLEAR TO MERGE).
- **accepted this iteration**: SURF-BROWSER (ADR-0015), CLI-K2 (ADR-0016) — both with a written ADR, zero code.
- `validate` = 0 errors; `validate --paths` = 0 errors.

## What happened in iteration 11

1. **Recovery round (0a).** `git worktree list` → only the main worktree; `.worktrees/` empty; every `lane/*` branch
   0-ahead of main → **no orphan lane, nothing to adopt**. No second orchestrator session was active.
2. **CI-RUNTIME-GATES-RED fully attributed** (from `gh run view 39468671027 --json jobs` + `--log-failed` per job) —
   3 real root causes + 1 derived:
   | leg | verdict | root cause |
   |---|---|---|
   | macos-latest build target | **real product break** | `crates/oc-util/src/procutil.rs:56,70` use `libc::SYS_pidfd_send_signal`/`SYS_pidfd_open` (Linux-only) → E0425 on aarch64-apple-darwin; same class in `crates/oc-server/src/safekill.rs:126,140` and `tools/parity/src/tui_diff.rs:784` (`TIOCSCTTY` is u32 on darwin, ioctl wants c_ulong → E0308). |
   | p10 battery (§12.2) | **real tooling bug** | `actions/upload-artifact@v4` drops the exec bit; `tools/parity/src/rust_server.rs:147 build_binary()` only checked `is_file()` → handed back a 0644 `target/debug/oc-server`, skipped `cargo build` → `spawn ...: Permission denied (os error 13)`. |
   | §12.3 runtime gates | **same tooling bug** | `real_db_null_census` spawns oc-server the same way → EACCES, not the real DB gap. |
   | gates summary | **derived** | `fail=1` because legs 1-3 are red; the summary logic itself is correct (skip ≠ pass). |
3. **lane-cirt4** (fixer own/glm-5.3) → **merged `accab46`** (4 commits: 1d9f8ba procutil, 60738b2 exec-bit,
   3ea3142 safekill, b4c31f3 tui_diff ioctl). `runtime-gates.yml`: exec-bit restore after each of the 7 target-artifact
   downloads (bins enumerated via `cargo metadata` + jq, no blanket `chmod -R`, no job turned into a skip,
   OC_GATE_STRICT untouched, known-red.json untouched). New regression test `tools/parity/tests/server_binary_exec.rs`
   (6 hermetic tests). Post-merge gate: fmt rc=0 · clippy(oc-util,parity,oc-server) rc=0 (fresh `Checking`) ·
   `cargo check --workspace` rc=0. Verifier (own/glm-5.3-flash `ses_ea4a184d4ffegXCh9LC2bUWYRt`): **CLEAR TO MERGE, 0 defects**.
4. **lane-tui3** (TUI-35) → **merged `d07ae72`** (commit 3ab3eb2). The recorded symptom was **WRONG**: the TUI does
   NOT boot-hang on a quiet server (instrumentation: subscribe + `Ingest::Resync` reach the loop, the store sets
   Connected at `store.rs:8862-8871`, the wordmark paints). The real defect: `crates/oc-tui/src/components/dialog.rs:190-207`
   — with a dialog open and a dirty filter, ctrl+c only CLEARED the filter and returned `Consumed` (dialog stayed open),
   so the harness chord (ctrl+c, ctrl+c, `<leader>q`) spent its presses on clear/close and never reached `app.exit`
   (`<leader>q` is bound to `session.queued_prompts`, `oc-tui-core/src/keymap/table.rs:625`) → the app could not be
   torn down → **90 s harness timeout** (CI run 39501153874: `oc-tui::pty_daily::session_filter_semantics`, 2/2 tries
   @ 90.019 s, panic at `crates/oc-tui/tests/vt_harness.rs:1596`). Fix: one ctrl+c clears the text AND closes the modal
   (`ui/dialog.tsx:115-156`). Verifier (`ses_ea46c9bf0ffeKBPq4YrGw2TUeM`): CLEAR TO MERGE, 0 defects; repro 90.03 s → 0.99 s.
5. **lane/tui4** (TUI-7) → **merged `2404c83`** (commit 03dce7c). The six session-list filter semantics were **already
   implemented** in production (`dialog_select.rs:273-286` trim/case-fold/substring/whitespace-full-set +
   `dialog_open.rs:508` parent exclusion) — the ledger's "actual" column was **stale** and is now corrected. The entry's
   red came from three defects in its own regression test's probes: the F4 presence wait was satisfied instantly by the
   tab strip; the "zln must not match" assert scanned the tab-strip row (which v2 legitimately keeps painted under the
   dialog); the zln block sent 8 backspaces for the 9-character query "LOGIN RED". Repairs = sync/scoping/arithmetic only,
   every existing assertion kept verbatim, 2 new pins added, **no production change**. Verifier
   (`ses_ea43baeb4ffedrjxQCHERG1IAD`): CLEAR TO MERGE, 0 defects, with a per-hunk **no-weakening audit** (the new F4 wait
   propagates its timeout as Err → it cannot time out into a pass). Full CI PTY filter: **42/42**.
6. **Adjudication batch #3 (wave 10, zero code)** — oracle (own/glm-5.3): **1 closed, 0 accepted, 9 stay open**.
   - **CLI-N6 → duplicate of CLI-F11** (same root cause: tree-wide OTEL absence; CLI-F11 holds the upstream evidence).
   - **No ADR created** — every probed key turned out to be genuinely consumed by v2.0.24 production code
     (`packages/util/src/observability/otlp.ts:63-101`, `packages/tui/src/util/scroll.ts:20-29`,
     `packages/tui/src/component/devtools-bar.tsx:82-83`, `packages/tui/src/feature-plugins/system/diff-viewer.tsx:103-326`,
     `packages/tui/src/app.tsx:351,1148-1155`, `packages/cli/src/commands/handlers/pair.ts:33-38`), so the "upstream equally
     ignores it" hypothesis is refuted for all 5 dead-key rows. Fix scopes recorded in the ledger notes for CLI-F11,
     CONFIG-E3, CONFIG-E4, CONFIG-T5, CONFIG-T6, CONFIG-T7, CONFIG-T9, CONFIG-T10, CLI-F6.
   - **Method note**: a batch of "dead config key" rows is a poor candidate set (1/10 closures). Prefer rows whose ref
     evidence points at a *surface* (a CLI command, a plugin artifact, a hosted surface), not at a consumed config key.
7. **New row KNOWN-RED-MASK (S2, WIRE)**: the p10 known-red registry printed the spawn EACCES as
   `KNOWN-RED:GATE-TUI-RENAME` — it swallowed a *new* failure mode as a registered known-red (the real red-batch
   attribution hazard). Scope: only the registered failure signature may be downgraded.

## CI verdicts from iteration 11 (async, non-blocking)

| run id | head | workflow | verdict |
|---|---|---|---|
| 39501153874 | a549907 | ci | **failure** — only `nextest (pty, serialized)` (the TUI-35 90 s wedge). Now fixed (d07ae72). |
| 39507224067 | 7ea3ecc | ci | **failure** — only `nextest (pty, serialized)` again (session_filter_semantics = TUI-7). Now fixed (2404c83). fmt+clippy ✅ build+archive ✅ nextest ✅ frame-budget ✅ guards ✅ |
| 39507225187 | 7ea3ecc | runtime-gates (dispatch) | **failure** — `build target (macos-latest, aarch64-apple-darwin)` **GREEN** (the E0425/E0308 class is dead) · build+archive ✅ · stage v2-written db ✅ · needs-runtime (AI) ✅ · **red**: needs-runtime (TUI) [= the TUI-7 PTY battery, now fixed], p10 battery [= the DB fixture gap], §12.3 gates [= GATE-DB-NULLROWS], gates summary [derived] |
| 39519705990 | 5a58263 | ci (push) | **cancelled** (superseded by 39519807488) |
| 39519736380 | 5a58263 | runtime-gates (dispatch) | **failure** — macos leg ✅ · **stage v2-written library db ✅ (the fixture IS produced)** · needs-runtime (TUI) ✅ · needs-runtime (AI) ✅ · **red**: p10 battery (§12.2), §12.3 runtime gates, gates summary (derived) |
| **39519807488** | **7036d96** | **ci (push)** | **SUCCESS — 6/6 jobs** (build+archive, fmt+clippy, guards, frame-budget, nextest, nextest-pty) → **TUI-7 + TUI-35 promoted to `verified`** |

> The next session should check these two runs first: they decide whether SURF-PLATFORM / CI-RUNTIME-GATES-RED /
> TUI-35 / TUI-7 can be promoted from `fixed` to `verified`.

### The DB gap is now the honest, dominant §12.2 blocker

**CORRECTION (iteration 12): the earlier "the reference never wrote the db → db_staged=gap" story was WRONG.**
Re-reading the CI log of run 39507225187: the `::error::no v2-written opencode.db …` line is the **step script
echo**, not executed output — the job actually **uploaded the fixture** (`artifact v2-db-fixture`, and the upload
step is gated on `db_staged == 'ok'`). `gh run download 39519736380 -n v2-db-fixture` then showed the truth:
the fixture is a **4096-byte, 0-table SQLite shell**. Root cause (new row **GATE-DB-FIXTURE-WAL**, S1/DB):
the reference runs in WAL mode (`/opt/opencode-ref/packages/core/src/database/sqlite.bun.ts:93`
`PRAGMA journal_mode = WAL`); the CI harvest copies **only the main db file**, so when the reference is killed
before a checkpoint (which is what a red interop lane does) the schema + data stay in the `-wal` sidecar and
the main file is a single header page. The downstream reds follow from that shell fixture:
- p10 battery → `p10_db_import_real_library_copy` FAILS: `import fixture …/v2-library.db: SQLiteError: no such table: migration`.
- §12.3 → `oc-server::real_db_null_census::real_migrated_db_serves_create_list_and_messages_without_500s` panics
  (the shell has no sessions, and the fixture's session `location` dirs are pid-scoped and dead).
- gates summary → derived (`fail=1`).
- **also still red in the same battery**: `p10_cross_tui::p10_ts_tui_against_rust_server` (= GATE-TUI-RENAME, a real
  product gap) and `p10_cli::p10_cli_gate5` (= `serve --hostname localhost` — our side only accepts IP literals
  (`crates/oc-cli/src/handlers/serve.rs:66-70` → `boot::parse_hostname`), and the **reference** side also failed to
  answer `/api/info` on `127.0.0.1` within the 90×1 s poll → triage: IPv6 `::1` binding vs boot timeout).

## Active lanes

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| `lane-db2` | DB | `.worktrees/lane-db2` (merged) | `lane/db2` | lane-db2 | GATE-DB-NULLROWS, GATE-DB-STABLE | **merged `315ff89` → verified**; worktree removable, branch kept for history |
| `lane-config6` | CONFIG | `.worktrees/lane-config6` | `lane/config6` | lane-config6 | CLI-K1, CLI-K8, CLI-N1, CLI-N2, CLI-N3, CLI-N4 | **6 rows `fixed`, CLI-N4 partial `fixing`; PAUSED before verify/merge** (wave 12) |
| `lane-db3` | DB | `.worktrees/lane-db3` | `lane/db3` | lane-db3 | GATE-DB-FIXTURE-WAL | **`fixed` (6cecd1e); PAUSED before verify/merge** (wave 12) |
| (oracle adjudication #4 — read-only, no lane) | TUI/SURFACES/CONFIG/CRED | — | — | — | SURF-BROWSER, CLI-K2, SURF-LATEX-MERMAID, TUI-15/16/17/12, CRED-D8 | done: 2 accepted (ADR-0015/0016), 6 kept open with fix scopes |

- `.worktrees/lane-db2` — merged → removable after the merge (branch `lane/db2` kept for history).

## Adjudication batch #4 (oracle own/glm-5.3, session ses_ea410c4d9ffe7SZZdfYs2L33Hw) — result

- **accepted + ADR-0015**: `SURF-BROWSER` — v2's browser tool family is a desktop companion (host chain
  desktop → gui-extensions → plugin-browser, ADR-0004 out-of-scope); headless, v2 itself is deny-by-default and
  unattachable (`packages/plugin-browser/src/connection.ts:23-27`), so there is no missing behaviour contract.
- **accepted + ADR-0016**: `CLI-K2` — the ledger's "method set shrunk" claim was **stale** (ours is a superset:
  7 v2 names + `local-file`/`github`); the real difference is that v2's package-manager methods install the **TS
  artifact** and read the TS release feed (ADR-0012 channel-family ruling).
- **kept open (6)** with executable fix scopes + corrected (stale) ref paths recorded in the ledger:
  `SURF-LATEX-MERMAID` (real product surface, pure in-repo TS impl), `TUI-15` (Zed sqlite selection → context label),
  `TUI-16` (drag selection → clipboard + `terminal.copy` consumer), `TUI-17` (mini subagent inspector),
  `TUI-12` (composer footer live counts), `CRED-D8` (Poe client id + `api_key`/`api_key_expires_in`).
- **new row `TUI-EDITOR-MCP`** (S2): the upstream `packages/tui/src/context/editor.ts` also carries a WS MCP editor
  protocol half that TUI-15 does not cover — kept as its own row instead of widening TUI-15.

## Model policy for subagents (user ruling 2026-10-20 — keep for every future session)

- **fixer = `own/glm-5.3` · oracle = `own/glm-5.3` · verifier = `own/glm-5.3-flash`** (all via the own provider).
- There is **no `verifier` subagent** in this build — the verifier role is played by a `fixer` instance with
  `own/glm-5.3-flash` and a strict read-only mandate (report only, no writes in the repo). Worked well 3× in iteration 11
  (`ses_ea4a184d4ffegXCh9LC2bUWYRt`, `ses_ea46c9bf0ffeKBPq4YrGw2TUeM`, `ses_ea43baeb4ffedrjxQCHERG1IAD`).
- Session reuse pays: lane/tui4 reused the lane/tui3 session (it already knew the harness) — no re-orientation cost.
- Do NOT reuse free-gateway (`opencode/*`) models unless the user says so. Look models up with
  `tools.opencode.models({provider:"own"})` — confirmed ids: `own/glm-5.3`, `own/glm-5.3-flash`.

## needs-runtime (6) — LIVE WINDOW STILL OUTSTANDING (asked of the user; not a loop blocker)

Remaining: TUI-28, TUI-30, TUI-33, TUI-TD06, TUI-TD15, TUI-TD28 (all S2) plus the CI-RUNTIME-AI-LIVE gap row (S1).
**User action needed:** (a) endpoint/credential sandbox for the live provider paths (`OC_CI_VERTEX_PROJECT` /
`OC_CI_VERTEX_CREDENTIALS` / `OC_CI_GEMINI_API_KEY` / `OC_CI_COPILOT_TOKEN`), (b) a time slot, (c) whether
cassette-only (ADR-0006) still stands for release. Without a window these stay as-is; §12.1 accepts them if each
gets an explicit runtime conclusion (ADR).

## Discipline (carry-over, still binding)

- **CI non-blocking (user ruling 2026-10-20):** never wait for CI during development; lane local-green +
  independent verifier PASS ⇒ merge; CI red ⇒ patch lane. Only the §12 exit gate waits for CI.
- All cargo/fmt/clippy/nextest via `tools/dev-gate.sh` (eternal130=uid 1000, CPUQuota 400%, MemoryMax 6G,
  global flock, `OPENCODE_DEV_GATE_DIR=<worktree>` for lanes). Never run cargo as root.
  Root-owned files in a worktree break the eternal130 gate → `chown -R 1000:1000 <worktree>` after editing.
- `git` in a chowned worktree needs `safe.directory` → `git config --global --add safe.directory '*'` (done).
- **Never trust a cached lint pass**: require a fresh `Checking <crate>` line before believing a clippy/fmt result.
- Process safety: no `kill`/`pkill`/`killall`/`fuser`, no bare PIDs, only owned child handles; group signals via
  `procutil::signal_group_filtered`. Never touch port 49374; tests bind `127.0.0.1:0`; no real data dirs.
- Never mark `verified` from a fixer session. No skip/ignore/deleted-assertion greens. `accepted` requires an ADR.
- Upstream pinned **2.0.24** (`/opt/opencode-ref`, commit `e7a34f09bf`).
- Evidence per entry under `/tmp/opencode/parity-evidence/<ID>/` (repro.sh + own red.log + green.log).
  A shared/copied red.log is NOT acceptable evidence.
- Known-red registry `tools/parity/goldens/p10/known-red.json`; skip ≠ pass (`OC_GATE_STRICT=1`).
- **Background subagents die with the session** — foreground for anything that must not be lost.
- A "fix" that only repairs the test's probes is legitimate ONLY when the production semantics are proven to exist
  (see TUI-7) and the verifier audits every changed hunk for weakening.

## Blockers

- Live window for the 6 `needs-runtime` entries + CI-RUNTIME-AI-LIVE secrets (user) — **re-asked this iteration**.
- **CI fixture shell** (new row `GATE-DB-FIXTURE-WAL`, S1/DB): the harvest copies only the main db file and drops the
  SQLite `-wal`/`-shm` sidecars → when the reference is killed before a checkpoint the fixture is a 4096-byte,
  0-table shell → the §12.2 p10 battery leg (`no such table: migration`) and the §12.3 real-v2-import leg stay red.
- GATE-TUI-RENAME: genuinely red on product behaviour (the rename never reaches the server's session row) — S1, WIRE.
- `p10_cli_gate5` (S1, CONFIG): `serve --hostname localhost` — our side only accepts IP literals
  (`crates/oc-cli/src/handlers/serve.rs:66-70` → `boot::parse_hostname`), and in CI the **reference** side also failed
  to answer `/api/info` on 127.0.0.1 within the 90×1 s poll (triage: IPv6 `::1` bind vs boot timeout — not yet settled).
- CI-RUNTIME-GATES-RED is still `fixed` (not `verified`): its contract is "every runtime-gates §12 leg green" and the
  p10 battery / §12.3 / gates-summary legs are still red.

## Next (in priority order — resume from the PAUSED state)

1. **Adopt the two paused lanes (0a recovery → §2G verifier, then §2H merge)**:
   - verifier (own/glm-5.3-flash, NEW session, read-only) for `lane/config6` @ a99a751 — run each
     /tmp/opencode/parity-evidence/<ID>/repro.sh, read the landed artifacts, grep the production call sites,
     audit every changed hunk for weakening (esp. the CLI-N4 partial + CLI-K8 boot-time-only consumption);
   - verifier for `lane/db3` @ 6cecd1e — repro.sh, the two hermetic pins, the workflow YAML;
   - then SERIAL merges (one at a time), ledger `merge --id`, push, and dispatch `runtime-gates` to prove the
     new harvest step end-to-end on a runner (the fixer's stated residual risk).
2. **CLI-N4 disposition**: 4 switches have no consumer subsystem (FILEWATCHER→CONFIG-C3, DISABLE_FFF,
   GIT_BASH_PATH, SIMULATE) + the oc-core engine seam gap (engine/mod.rs:763, overlay.rs:162) — split into
   scoped rows or adjudicate accepted-with-ADR at the next batch.
3. **Cheapest S1s next** (scopes in ledger notes from adjudication #5): RUNTIME-K01 (one-flag fix),
   GATE-TUI-RENAME (harness probe repair: clear the dialog prefill or poll ends_with, then re-run the battery).
4. Then the remaining S1 lanes: WIRE HTTP-H0 (continue lane/h0b pattern), CRED-D6/V3/D8, PLUG-D13/M02/D30,
   TOOL-D1, SURF-LATEX-MERMAID, AI-V1/VERTEX-V2/P06, TUI-12/14/15/16/17, RUNTIME-FDLEAK (new).
5. **Live window (user action, re-asked every iteration)**: 6 needs-runtime S2 TUI rows + CI-RUNTIME-AI-LIVE (S1)
   need endpoint/credential sandbox + time slot; §12.1 accepts them only with an explicit runtime conclusion (ADR).
6. Record CI run ids as they land; promote `fixed` → `verified` only on a green run id (or independent verifier PASS
   per the 2026-10-20 CI-non-blocking ruling).
