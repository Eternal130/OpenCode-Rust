# Parity Baseline — 2026-10-18 (Batch 0 / B0-2)

> Baseline of the opencode-rust workspace at the parity-loop bootstrap.
> Recorded by the orchestrator from actual gate runs; every red has a ledger ID.
> Reference: `/opt/opencode-ref` @ `e7a34f09bf` (read-only, v2.0.24). Ledger: 372 rows.

## 1. Environment

| item | value |
|---|---|
| gate identity | `eternal130` via `tools/dev-gate.sh` (systemd scope: CPUQuota=400%, MemoryMax=10G, TasksMax=512) |
| bun (lane) | `/home/eternal130/.bun/bin/bun` v1.3.10 (staged copy of root's; `/root` is 700 so root's copy is unreachable for the gate user) |
| ref checkout (runnable) | `/tmp/opencode/ref` (eternal130-owned, node_modules installed, self-heals from source) |
| strict mode | `OC_GATE_STRICT=1` fail-not-skip live since B0-8 merge `557675d` (`tools/parity/src/gate.rs`) |
| baseline commits | main `29f5e0d` (results) — p10 additionally bisected at `277a375` (pre-B0-8) with identical outcome |

## 2. Results

| gate | command (all via dev-gate) | result |
|---|---|---|
| workspace | `cargo nextest run --workspace --locked --no-fail-fast` | **3391 run: 3391 passed, 10 skipped, 0 failed** (319.9s; 1 leaky-test flag — see §4.6) |
| fmt | `cargo fmt --all -- --check` | **OK** |
| clippy (parity) | `cargo clippy -p parity --lib --tests -- -D warnings` | **RED: 15 errors, all in `tools/parity/src/bin/rescan-tui.rs`** → `HYGIENE-CLIPPY-RESCAN-TUI` |
| p10 battery (strict) | `OC_P10=1 OC_GATE_STRICT=1 PARITY_BUN=… cargo nextest run -p parity --locked --run-ignored only --no-fail-fast -E "test(p10_)"` | **RED: 3/6 failed** (each failed both nextest retries → deterministic) → `GATE-DB-STABLE`, `GATE-DB-NULLROWS`, `GATE-TUI-RENAME` |
| Gate 0 | `gate0_environment` (strict) | **PASS** — `gate_ran:true`, ref_commit `e7a34f09…`, reference_boot ok, rust_boot ok (B0-8 evidence: `docs/parity-b0-gate0.md`) |

### 2.1 p10 failure detail (deterministic at `29f5e0d` AND `277a375`)

| test | failure | ledger |
|---|---|---|
| `p10_db_interop_stable_channel_same_file` | v2 re-open doesn't reflect Rust-side model switch on `opencode-stable.db` (`p10_db_interop.rs:433`; model.id stays `fake-model-a`) | `GATE-DB-STABLE` |
| `p10_db_interop_v2_written_null_rows` | three-boot NULL-heavy interop flow fails (~15s) | `GATE-DB-NULLROWS` |
| `p10_ts_tui_against_rust_server` | rename exact-title poll times out; last row title `The outside file says p10-secret.p10 renamed from tui` (auto-title merge suspected; `p10_cross_tui.rs:244`) | `GATE-TUI-RENAME` |

**Bisect note**: the identical 3-failure set reproduces at `277a375` (pre-B0-8, same eternal130-bun environment) — **not a B0-8 regression**. The historical "6/6" (`tools/parity/goldens/p10/VERIFICATION.md`) predates the bootstrap and was recorded under an unknown execution context. First triage step for the owning clusters: separate harness/environment causes from product causes (C8 DB / WIRE session semantics). Do not mark these `accepted` without an ADR.

## 3. Five minimal differential gates (§8.3) — build state

| gate | file (planned) | state after B0-2 (2026-10-18) |
|---|---|---|
| Gate 0 env precheck | `tools/parity/tests/gate0_environment.rs` | **live** (B0-8) |
| Gate 1 HTTP/SSE contract | `tools/parity/tests/p10_http_contract.rs` | **skeleton built** (B0-2): pagination null/order/limit/round-trip, error envelope, 2-route validation matrix (141 ops tabled), SSE first-event live; `previous`-cursor + cursor-token payload divergences recorded as gap facts (unregistered, pending triage) |
| Gate 2 DB | `tools/parity/tests/p10_db_interop.rs` + `p10_db_import.rs` | interop partially live (3 red lanes ledger-ized in §2.1/§4, failures now `KNOWN-RED:`-prefixed); **import skeleton built** (B0-2) — fixture-gated behind `PARITY_V2_DB_FIXTURE` (strict mode fails named-missing until a real v2 library copy exists) |
| Gate 3 Tools | `tools/parity/tests/p10_tools.rs` | **skeleton built** (B0-2): read/edit/glob/grep dual-end content+metadata+side-effect compares live; shell/timeout/permission/patch recorded as ledger-linkable checkpoints |
| Gate 4 TUI five flows | `tools/parity/tests/p10_tui_frames.rs` | **skeleton built** (B0-2): flow 1 (boot/home) live as structural smoke at 80x24+120x30 (chrome-EXACT machinery with empty certified-row list); flows 2–5 not-wired checkpoints (form/permission → `TUI-TD16`/`TUI-TD18`) |
| Gate 5 CLI | `tools/parity/tests/p10_cli.rs` | **skeleton built** (B0-2): version/help/serve/auth/models/session-list/run-json/early-error/--print-logs live; help text, TTY branches, session-list full-JSON + table time, run-json event coverage recorded as gaps |

Known-red registry: `tools/parity/goldens/p10/known-red.json` (created B0-2;
entries below). Classify a strict-run log with
`tools/parity/known-red-summary.py <log>` (exit 0 only when 0 NEW-RED and
0 expired known-red).

## 4. Known-red register (all carry ledger IDs)

| # | red | ledger id | owner | expiry |
|---|---|---|---|---|
| 1 | p10 stable-channel model switch | `GATE-DB-STABLE` | unassigned | Batch 1B |
| 2 | p10 null-rows interop | `GATE-DB-NULLROWS` | unassigned | Batch 1B |
| 3 | p10 cross-TUI rename title | `GATE-TUI-RENAME` | unassigned | Batch 1B |
| 4 | `scenario_permission_saved` flaky | `GATE-FLAKE-PERM-SAVED` | unassigned | Batch 1B |
| 5 | clippy rescan-tui (15 errors) | `HYGIENE-CLIPPY-RESCAN-TUI` | unassigned | Batch 0 exit (B0-4/B0-5 disposition) |

### 4.6 Non-red observations (tracked, no ledger entry)

- nextest flagged **1 leaky test** in the workspace run (not identified in tail output; identify on next full run if it recurs).
- tmux-availability skips in opt-in TUI tests remain **skips** under `OC_GATE_STRICT=1` (documented B0-8 follow-up, `docs/parity-b0-gate0.md` §7). Decision needed by the p10 battery owner whether strict mode should require tmux.
- `docs/host-issues.md` H-1 (child background-task reports may not reach parent) — operational constraint on lanes, already encoded in lane rules.

## 5. Batch 0 progress against this baseline

- B0-1 ledger import: **done** (`9fafd41`, 367 imported rows + 5 known-red = 372; `validate --paths` errors=0)
- B0-8 Gate 0 environment: **done** (`557675d`)
- B0-2 this document + gate skeletons: in progress
- B0-4 guards / B0-5 ADRs / B0-7 test debt / B0-9 live window: pending (B0-9 needs user decision)
