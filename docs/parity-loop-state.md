# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.
> **REBUILT from ledger+git 2026-10-21 07:10** (iteration 11 complete).
> Ledger + git are authoritative; state.md is a cross-session breadcrumb only.

- **updated**: 2026-10-21 07:10 — iteration 11 done. Three lanes closed, all merged and CI-attributed:
  `lane-cirt4` → `da22466` (CI-RUNTIME-GATES-RED + SURF-PLATFORM), `lane-tui3` → `bba65d3` (TUI-35),
  `lane/tui4` → `e4c3bd4` (TUI-7). Wave-10 adjudication closed 1 duplicate. **blocking S0/S1 = 30** (was 33).
- **baseline**: main @ `e4c3bd4` (merge: lane/tui4) on top of `bba65d3` (merge: lane/tui3) on top of `da22466`
  (merge: lane/cirt4)
- **batch**: 3 (wave 10) — Batch 3 clusters: DB, PLUGIN-RESIDUAL, EVENT/TOOL/CORE, PLATFORM, POLICY/RETENTION
- **iteration**: 11 (driver = tools/loop-driver.sh; session started 2026-10-21 05:33, ended ~07:10)

## Ledger snapshot (`parity-ledger.py summary`, end of iteration 11)

- rows=404 — **verified 95 · accepted 14 · duplicate 3 · fixed 4** · open 282 · needs-runtime 6
- **blocking S0/S1 = 30** (open): CONFIG 7 (CLI-K1, CLI-K2, CLI-K8, CLI-N1..N4) · AI 4 (AI-P06, AI-V1,
  AI-VERTEX-V2, CI-RUNTIME-AI-LIVE) · TUI 5 (TUI-12, TUI-14, TUI-15, TUI-16, TUI-17) · CRED 3 (D6, D8, V3) ·
  PLUGIN 3 (PLUG-D13, PLUG-D30, PLUG-M02) · DB 2 (GATE-DB-STABLE, GATE-DB-NULLROWS) · WIRE 2 (HTTP-H0,
  GATE-TUI-RENAME) · SURFACES 2 (SURF-BROWSER, SURF-LATEX-MERMAID) · RUNTIME 1 (RUNTIME-K01) ·
  TOOLS-EXEC 1 (TOOL-D1)
- **4 rows are `fixed` (awaiting a GREEN CI run id to promote to `verified`)**: SURF-PLATFORM,
  CI-RUNTIME-GATES-RED, TUI-35, TUI-7. All four have merge commits on main.
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
3. **lane-cirt4** (fixer own/glm-5.3) → **merged `da22466`** (4 commits: 682b580 procutil, 8361674 exec-bit,
   e110926 safekill, 076625a tui_diff ioctl). `runtime-gates.yml`: exec-bit restore after each of the 7 target-artifact
   downloads (bins enumerated via `cargo metadata` + jq, no blanket `chmod -R`, no job turned into a skip,
   OC_GATE_STRICT untouched, known-red.json untouched). New regression test `tools/parity/tests/server_binary_exec.rs`
   (6 hermetic tests). Post-merge gate: fmt rc=0 · clippy(oc-util,parity,oc-server) rc=0 (fresh `Checking`) ·
   `cargo check --workspace` rc=0. Verifier (own/glm-5.3-flash `ses_ea4a184d4ffegXCh9LC2bUWYRt`): **CLEAR TO MERGE, 0 defects**.
4. **lane-tui3** (TUI-35) → **merged `bba65d3`** (commit ccc11b8). The recorded symptom was **WRONG**: the TUI does
   NOT boot-hang on a quiet server (instrumentation: subscribe + `Ingest::Resync` reach the loop, the store sets
   Connected at `store.rs:8862-8871`, the wordmark paints). The real defect: `crates/oc-tui/src/components/dialog.rs:190-207`
   — with a dialog open and a dirty filter, ctrl+c only CLEARED the filter and returned `Consumed` (dialog stayed open),
   so the harness chord (ctrl+c, ctrl+c, `<leader>q`) spent its presses on clear/close and never reached `app.exit`
   (`<leader>q` is bound to `session.queued_prompts`, `oc-tui-core/src/keymap/table.rs:625`) → the app could not be
   torn down → **90 s harness timeout** (CI run 39501153874: `oc-tui::pty_daily::session_filter_semantics`, 2/2 tries
   @ 90.019 s, panic at `crates/oc-tui/tests/vt_harness.rs:1596`). Fix: one ctrl+c clears the text AND closes the modal
   (`ui/dialog.tsx:115-156`). Verifier (`ses_ea46c9bf0ffeKBPq4YrGw2TUeM`): CLEAR TO MERGE, 0 defects; repro 90.03 s → 0.99 s.
5. **lane/tui4** (TUI-7) → **merged `e4c3bd4`** (commit 3c5b464). The six session-list filter semantics were **already
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
| 39501153874 | f1af96a | ci | **failure** — only `nextest (pty, serialized)` (the TUI-35 90 s wedge). Now fixed (bba65d3). |
| 39507224067 | b054456 | ci | **failure** — only `nextest (pty, serialized)` again (session_filter_semantics = TUI-7). Now fixed (e4c3bd4). fmt+clippy ✅ build+archive ✅ nextest ✅ frame-budget ✅ guards ✅ |
| 39507225187 | b054456 | runtime-gates (dispatch) | **failure** — `build target (macos-latest, aarch64-apple-darwin)` **GREEN** (the E0425/E0308 class is dead) · build+archive ✅ · stage v2-written db ✅ · needs-runtime (AI) ✅ · **red**: needs-runtime (TUI) [= the TUI-7 PTY battery, now fixed], p10 battery [= the DB fixture gap], §12.3 gates [= GATE-DB-NULLROWS], gates summary [derived] |
| (pending) | e4c3bd4 | ci + runtime-gates | to be dispatched at the end of iteration 11 |

### The DB gap is now the honest, dominant §12.2 blocker

With the exec-bit defect fixed, the p10 battery and §12.3 gates finally RUN the binaries, and what they show is real:
- `stage v2-written library db` (job green, but **no fixture**): the interop lane ran
  (`p10_db_interop_v2_written_null_rows`, 2 tries @ ~17.4 s) and then the harvest
  `find /tmp/opencode/p10-db-*/home/.local/share/opencode/opencode.db` found **nothing** → `db_staged=gap`:
  the reference v2.0.24 never wrote the db, so the fixture is a GAP and every downstream import gate reports it
  honestly (never a pass).
- p10 battery → `p10_db_import_real_library_copy` FAILS (404 `LocationNotFoundError` on `/tmp/opencode/p10-db-*/project`).
- §12.3 → `oc-server::real_db_null_census::real_migrated_db_serves_create_list_and_messages_without_500s` panics at
  `crates/oc-server/tests/real_db_null_census.rs:97`.
- gates summary → derived (`fail=1`).

## Active lanes

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| (none — lane-cirt4 `da22466`, lane-tui3 `bba65d3`, lane/tui4 `e4c3bd4` all merged) | | | | | | |

- `.worktrees/lane-cirt4`, `.worktrees/lane-tui3`, `.worktrees/lane-tui4` — merged, trees clean → removable
  (branches `lane/cirt4`, `lane/tui3`, `lane/tui4` kept for history).

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

- Live window for the 6 `needs-runtime` entries + CI-RUNTIME-AI-LIVE secrets (user).
- 4 rows are `fixed` but not `verified` (SURF-PLATFORM, CI-RUNTIME-GATES-RED, TUI-35, TUI-7): they need GREEN CI run ids.
- **DB fixture gap** (GATE-DB-NULLROWS + GATE-DB-STABLE): the reference v2.0.24 never writes the library db in the
  interop lane, so the §12.2 p10 battery and the §12.3 v2-import gate cannot pass. This is now the dominant §12.2 blocker.
- GATE-TUI-RENAME: genuinely red on product behaviour (the rename never reaches the server's session row) — S1, WIRE.

## Next (in priority order)

1. **DB lane (next unit, hot files: `crates/oc-core/src/session/store.rs`, `crates/oc-server/**`)**:
   - make the v2.0.24 reference actually write `opencode.db` in the interop lane (so the fixture exists and
     `db_staged=ok`), and close `GATE-DB-NULLROWS` (three-boot NULL-heavy flow) + `GATE-DB-STABLE`.
   - this unblocks the p10 battery leg, the §12.3 v2-import leg and the gates-summary leg — i.e. most of §12.2.
   - the workflow's fixture job is honest already (`db_staged=gap`, never a silent pass) — do NOT make it skip.
2. **Re-dispatch CI** on the current head (`e4c3bd4` + the ledger/state commit): `ci` (push) +
   `gh workflow run runtime-gates.yml`. Expect the PTY legs to go green (TUI-35 + TUI-7) and only the DB legs to stay
   red. Record the run ids here; then promote SURF-PLATFORM / CI-RUNTIME-GATES-RED / TUI-35 / TUI-7 to `verified`
   (`evidence --verified` + `merge --id <ID> --merge-commit <sha>`).
3. **CONFIG lane** (7 open S1: CLI-K1, CLI-K2, CLI-K8, CLI-N1..N4 — `crates/oc-cli/**` + `crates/oc-server/**`;
   CLI-K8 also touches `crates/oc-tui/src/storage.rs`).
4. **AI vertex lane** — AI-V1 + AI-VERTEX-V2 (+ CRED-V3 ADC) in one lane; `crates/oc-ai/src/protocols/**` +
   `crates/oc-core/src/provider_catalog/facade.rs`.
5. **GATE-TUI-RENAME** (S1, WIRE) — confirmed product gap: the rename never reaches the server's session row.
6. Remaining S1s: TUI-12/14/15/16/17, PLUG-D13/D30/M02, CRED-D6/D8/V3, HTTP-H0, RUNTIME-K01, TOOL-D1,
   SURF-BROWSER, SURF-LATEX-MERMAID, AI-P06, CI-RUNTIME-AI-LIVE.
7. Record CI run ids here as they land.
