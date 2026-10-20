# Parity Loop State

> Orchestrator scratch state for the parity loop (docs/PARITY-LOOP.md).
> Updated at the end of every iteration. Not a task queue — the ledger is.
> **REBUILT from ledger+git 2026-10-21 06:25** (iteration 11 complete).
> Ledger + git are authoritative; state.md is a cross-session breadcrumb only.

- **updated**: 2026-10-21 06:25 — iteration 11 done: recovery round (nothing to adopt), **CI-RUNTIME-GATES-RED fully
  attributed and fixed** (lane-cirt4 → merged `da22466`), SURF-PLATFORM's macOS leg fixed in the same lane
  (both rows now `fixed`, pending a GREEN CI run id), wave-10 adjudication closed 1 duplicate + recorded 9 fix scopes.
- **baseline**: main @ `da22466` (merge: lane/cirt4)
- **batch**: 3 (wave 10) — Batch 3 clusters: DB, PLUGIN-RESIDUAL, EVENT/TOOL/CORE, PLATFORM, POLICY/RETENTION
- **iteration**: 11 (driver = tools/loop-driver.sh; session started 2026-10-21 05:33)

## Ledger snapshot (`parity-ledger.py summary`, end of iteration 11)

- rows=404 — **verified 95 · accepted 14 · duplicate 3 · fixed 2** · open 283 · needs-runtime 6 · blocked 1
- **blocking S0/S1 = 31** (was 33; −SURF-PLATFORM −CI-RUNTIME-GATES-RED, both now `fixed` pending a green CI run id):
  - AI 4 (AI-P06, AI-V1, AI-VERTEX-V2, CI-RUNTIME-AI-LIVE)
  - CONFIG 7 (CLI-K1, CLI-K2, CLI-K8, CLI-N1, CLI-N2, CLI-N3, CLI-N4)
  - CRED 3 (CRED-D6, CRED-D8, CRED-V3)
  - TUI 6 (TUI-12, TUI-14, TUI-15, TUI-16, TUI-17, TUI-35)
  - PLUGIN 3 (PLUG-D13, PLUG-D30, PLUG-M02)
  - SURFACES 2 (SURF-BROWSER, SURF-LATEX-MERMAID)
  - DB 2 (GATE-DB-STABLE, GATE-DB-NULLROWS)
  - WIRE 2 (GATE-TUI-RENAME, HTTP-H0)
  - RUNTIME 1 (RUNTIME-K01) · TOOLS-EXEC 1 (TOOL-D1)
  - (TUI-7 stays `blocked` — its root cause is TUI-35, counted above.)
- `validate` = 0 errors; `validate --paths` = 0 errors.

## What happened in iteration 11

1. **Recovery round (0a).** `git worktree list` → only the main worktree; `.worktrees/` empty; every `lane/*` branch is
   **0 ahead** of main → **no orphan lane, nothing to adopt, nothing to salvage**. No second orchestrator session was
   active (the iteration-10 "two orchestrators" hazard did not recur).
2. **CI-RUNTIME-GATES-RED fully attributed** (from `gh run view 39468671027 --json jobs` + `--log-failed` per job) —
   3 real root causes + 1 derived, not 4 independent bugs:
   | leg | verdict | root cause |
   |---|---|---|
   | macos-latest build target | **real product break** | `crates/oc-util/src/procutil.rs:56,70` use `libc::SYS_pidfd_send_signal`/`SYS_pidfd_open` (Linux-only numbers; module also scans `/proc`) → E0425 on aarch64-apple-darwin. Same class later found in `crates/oc-server/src/safekill.rs:126,140` and `tools/parity/src/tui_diff.rs:784` (`TIOCSCTTY` is u32 on darwin, ioctl wants c_ulong → E0308). |
   | p10 battery (§12.2) | **real tooling bug** | `actions/upload-artifact@v4` drops the exec bit; `tools/parity/src/rust_server.rs:147 build_binary()` only checked `is_file()` → handed back a 0644 `target/debug/oc-server` and skipped `cargo build` → `spawn ...: Permission denied (os error 13)`. |
   | §12.3 runtime gates | **same tooling bug** | `real_db_null_census` spawns oc-server the same way → EACCES, not the real DB gap. |
   | gates summary | **derived** | `fail=1` because legs 1-3 are red; the summary logic itself is correct (skip ≠ pass). |
3. **lane-cirt4 (fixer, own/glm-5.3)** — 4 commits on `lane/cirt4`, merged as **`da22466`**:
   - `682b580` procutil: Linux scan `#[cfg(target_os="linux")]`-gated (body unchanged) + documented non-Linux no-op.
   - `8361674` rust_server.rs: `build_binary()` rejects a non-executable file, **removes it** (cargo does not relink on a
     fresh fingerprint), rebuilds, asserts executability; new `is_executable_file()` / `ensure_binary()`;
     `tools/parity/tests/server_binary_exec.rs` = 6 hermetic regression tests;
     `.github/workflows/runtime-gates.yml`: exec-bit restore after each of the 7 target-artifact downloads
     (bins enumerated via `cargo metadata` + jq, no blanket `chmod -R`, no job turned into a skip, OC_GATE_STRICT
     untouched, known-red.json untouched).
   - `e110926` safekill.rs: pidfd helpers gated to Linux; non-Linux = single-pid POSIX `kill(2)`, **pid ≤ 0 refused**
     (never a group id, never a broadcast). Linux arm byte-identical.
   - `076625a` tui_diff.rs: `TIOCSCTTY`/`TIOCSWINSZ` cast to `libc::c_ulong` (the TIOCSWINSZ twin was hidden behind the first).
   - Post-merge gate on main: `cargo fmt --all -- --check` rc=0 · `cargo clippy -p oc-util -p parity -p oc-server
     --all-targets --locked -- -D warnings` rc=0 (fresh `Checking` lines) · `cargo check --workspace --locked` rc=0.
   - **Verifier** (own/glm-5.3-flash, independent session `ses_ea4a184d4ffegXCh9LC2bUWYRt`): **CLEAR TO MERGE, 0 defects** —
     both repro.sh green, red/green artifacts are real captured output, 6/6 + 6/6 counts reproduced, production call
     sites proven (`build_binary` callers rust_server.rs:133/143; `signal_group_filtered` callers rust_server.rs:103,
     reference.rs:123; `safe_signal` callers oc-server/src/lifecycle.rs:1085/1099), no group-kill on non-Linux,
     `git diff main..HEAD -- tools/parity/goldens tools/parity/tests/p10_db_interop.rs` empty, worktree clean.
     Could not verify the "119 passed" figure (no artifact carries it) — non-blocking.
   - **Status = `fixed`, not `verified`**: both rows' own contracts require a GREEN CI run id (SURF-PLATFORM: "close only
     after the first GREEN run"; CI-RUNTIME-GATES-RED: "every §12 exit leg green on CI"). Promote to `verified` once
     the runs are green. `merge_commit=da22466` recorded on both.
4. **Honest state of the two p10 known-reds** (was masked by the EACCES):
   - **GATE-TUI-RENAME is GENUINELY red on product behaviour** — the rename never reaches the server's session row
     (timed-out poll of `GET /api/session/...`). Local repro: `CI-RUNTIME-GATES-RED/t3-gate-tui-rename-local.log`.
   - **GATE-DB-NULLROWS unknown** — not reproducible on this box (no v2-written db fixture; `PARITY_V2_DB_FIXTURE`/
     `OC_REAL_DB` unset). With the exec bit restored the CI job will now run the real import instead of dying at spawn.
   - **New row KNOWN-RED-MASK (S2, WIRE)**: the p10 known-red registry printed the spawn EACCES as
     `KNOWN-RED:GATE-TUI-RENAME`, i.e. it swallowed a *new* failure mode as a registered known-red — the real red-batch
     attribution hazard. Scope: only the registered failure signature may be downgraded.
5. **Adjudication batch #3 (wave 10, zero code)** — oracle (own/glm-5.3): **1 closed, 0 accepted, 9 stay open**.
   - **CLI-N6 → duplicate of CLI-F11** (same root cause: tree-wide OTEL absence; CLI-F11 holds the upstream evidence).
   - **No ADR created** — every probed key turned out to be genuinely consumed by v2.0.24 production code
     (`packages/util/src/observability/otlp.ts:63-101`, `packages/tui/src/util/scroll.ts:20-29`,
     `packages/tui/src/component/devtools-bar.tsx:82-83`, `packages/tui/src/feature-plugins/system/diff-viewer.tsx:103-326`,
     `packages/tui/src/app.tsx:351,1148-1155`, `packages/cli/src/commands/handlers/pair.ts:33-38`), so the
     "upstream equally ignores it" hypothesis is refuted for all 5 dead-key rows. Fix scopes recorded in the ledger
     notes for CLI-F11, CONFIG-E3, CONFIG-E4, CONFIG-T5, CONFIG-T6, CONFIG-T7, CONFIG-T9, CONFIG-T10, CLI-F6.
   - **Method note for the next wave**: a batch of "dead config key" rows is a poor candidate set — this one produced
     1/10 closures. Prefer rows whose ref evidence points at a *surface* (a CLI command, a plugin artifact, a hosted
     surface) rather than at a consumed config key.

## Active lanes

| lane | cluster | worktree | branch | owner | entries | status |
|---|---|---|---|---|---|---|
| (none — lane-cirt4 merged as `da22466`; worktree removable, branch kept) | | | | | | |

- `.worktrees/lane-cirt4` — merged, tree clean → safe to `git worktree remove` (branch `lane/cirt4` kept for history).

## Model policy for subagents (user ruling 2026-10-20 — keep for every future session)

- **fixer = `own/glm-5.3` · oracle = `own/glm-5.3` · verifier = `own/glm-5.3-flash`** (all via the own provider).
- There is **no `verifier` subagent** in this build — the verifier role is played by a `fixer` instance with
  `own/glm-5.3-flash` and a strict read-only mandate (report only, no writes in the repo). This worked well in
  iteration 11 (own/glm-5.3-flash verifier session `ses_ea4a184d4ffegXCh9LC2bUWYRt`).
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

## CI run ids (async, non-blocking)

| run id | head | workflow | result |
|---|---|---|---|
| 39468661905 | 35e6343 (CI-RUNTIME) | ci | **failure** — only `static anti-regression guards` (3 × `sudo` NEW) → patched by lane/cirt3 (72b6837) |
| 39468671027 | 35e6343 (CI-RUNTIME) | runtime-gates (dispatch) | **failure** — 4 legs red → attributed + fixed in iteration 11 (see above) |
| 39499419280 | 00bc8e5 (TUI merge + ledger) | ci | `static anti-regression guards` **failure** = same pre-fix sudo violations, fmt+clippy ✅, build ✅ |
| 39500915503 | 0a0f0c3 | ci | **cancelled** (superseded by the next push) |
| 39501153874 | f1af96a | ci | `static anti-regression guards` ✅ (sudo patch confirmed), fmt+clippy ✅, build+archive ✅ — the rest of the run was superseded by the `da22466` push |
| (pending) | da22466 | ci + runtime-gates | to be dispatched at the end of iteration 11 |

## Blockers

- Live window for the 6 `needs-runtime` entries + CI-RUNTIME-AI-LIVE secrets (user).
- SURF-PLATFORM + CI-RUNTIME-GATES-RED are `fixed` but **not `verified`**: they need the GREEN CI run ids
  (macos platform-build leg + the runtime-gates legs) before promotion.
- GATE-TUI-RENAME is genuinely red on product behaviour (rename never reaches the server session row) — S1, WIRE.
- GATE-DB-NULLROWS condition still unknown (needs the CI-staged v2 db fixture).

## Next

1. **Dispatch the CI pair on `da22466`**: `ci` (push-triggered) + `gh workflow run runtime-gates.yml` (dispatch).
   Record the run ids here. If the macos platform-build leg is still red → it is a *new* macOS compile error → new row
   + patch lane. If the p10 battery is red only because of GATE-TUI-RENAME / GATE-DB-NULLROWS → those are product gaps,
   not workflow bugs: open a patch lane for GATE-TUI-RENAME (WIRE) and keep GATE-DB-NULLROWS (DB) separate.
2. **Promote SURF-PLATFORM + CI-RUNTIME-GATES-RED to `verified`** once their legs are green (`evidence --verified`
   + `merge --id <ID> --merge-commit da22466`).
3. **Fresh CONFIG lane** (7 open S1: CLI-K1, CLI-K2, CLI-K8, CLI-N1..N4 — `crates/oc-cli/**` + `crates/oc-server/**`;
   CLI-K8 also touches `crates/oc-tui/src/storage.rs`).
4. **TUI-35** (boot hang) — unblocks TUI-7; same TUI cluster.
5. **AI vertex lane** — AI-V1 + AI-VERTEX-V2 (+ CRED-V3 ADC) in one lane; `crates/oc-ai/src/protocols/**` +
   `crates/oc-core/src/provider_catalog/facade.rs`.
6. **GATE-TUI-RENAME** (S1, WIRE) — now a confirmed product gap; the rename never reaches the server's session row.
7. Record CI run ids here as they land.
