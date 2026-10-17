# Lane GATE — beta.13 acceptance evidence: **NO-GO (Phase A blocker)**

**Scope intended.** The consolidated verify-lane final + the full
26-script live differential sweep (sweep-3) as the v2.0.24.beta.13
tagging evidence, run on `lane/gate` (worktree `.worktrees/lane-gate`,
base `a6d03b0` = every post-beta.12 fix lane merged).

**Outcome.** Phase A.2 — the CI-parity clippy gate — **FAILED**.
Per the lane protocol (any Phase A failure = STOP, log evidence,
report, do not fix) the gate halted at A.2: phases A.3–A.6 and the
entire Phase B sweep were **not run**. beta.13 is **not tag-ready**.

## Phase A results

| # | phase | result |
|---|---|---|
| A1 | `cargo fmt --all --check` | **PASS** (0 diffs) |
| A2 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | **FAIL** — exit 101, 6 lint errors (oc-core test targets) |
| A3 | nextest full workspace | NOT RUN (protocol STOP at A2) |
| A4 | nextest `--profile pty` (serialized) | NOT RUN |
| A5 | `cargo build --release -p oc-cli --locked` | NOT RUN |
| A6 | release frame budgets (strict 40 ms) | NOT RUN |
| B | 26-script live sweep (`--runs 2 --no-ref-cache`, both sizes) | NOT RUN |

## The blocker (complete fix list)

All 6 errors are in `oc-core` **lib-test** code, workspace pedantic
lint set, toolchain pinned 1.99.0 (identical command in
`.github/workflows/ci.yml` → CI on a tag would fail the same way;
verified not a local-version artifact):

| # | location | lint | fix |
|---|---|---|---|
| 1 | `crates/oc-core/src/cli_config/jsonc.rs:797:42` | `redundant_closure_for_method_calls` | `.map(|o| o.len())` → `.map(serde_json::Map::len)` |
| 2 | `crates/oc-core/src/cli_config/mod.rs:769:9` | `doc_markdown` | backtick `OPENCODE_CLI_CONFIG_CONTENT` |
| 3 | `crates/oc-core/src/cli_config/mod.rs:934:9` | `items_after_statements` | hoist `struct EnvGuard` above the statements |
| 4 | `crates/oc-core/src/cli_config/mod.rs:935:9` | `items_after_statements` | hoist `impl EnvGuard` |
| 5 | `crates/oc-core/src/cli_config/mod.rs:941:9` | `items_after_statements` | hoist `impl Drop for EnvGuard` |
| 6 | `crates/oc-core/src/cli_config/mod.rs:965:14` | `used_underscore_binding` | `drop(_env)` — rename binding or restructure the guard |

Full clippy transcript: `/tmp/opencode/gate-clippy.log`.

## Provenance (why this is new since beta.12)

- Tag `v2.0.24.beta.12` = `fe1ebb7` (2026-10-16 04:44 +0800).
- The failing code landed **after** the tag: `f3eb7f5` (config: F-1
  consumption halves, 2026-10-17 05:54 +0800) and `af55e25`
  (tail(2) protocol/robustness, 2026-10-17 10:28 +0800) are the last
  two commits touching `crates/oc-core/src/cli_config/`; neither is an
  ancestor of the beta.12 tag.
- The tail lane knew: `docs/tail.log` records "the 7 oc-core
  TEST-target lints are pre-existing on main, untouched files" —
  pre-existing relative to that lane's start (i.e. landed by an
  earlier post-beta.12 lane), and its own gates covered only the
  surfaces it touched. The full-workspace verification that surfaces
  the debt tree-wide was exactly what the tier ruling deferred to
  this lane. It fired.

## Path to beta.13

1. A mechanical clippy-debt lane (~10 lines, list above; all in test
   code, zero production surface) → clippy workspace -D warnings green.
2. Re-run lane GATE in full: Phase A (fmt → clippy → nextest → pty →
   release build → strict frame budgets) then Phase B (the 26-script
   sweep-3, both sizes × 2 runs, `--no-ref-cache`, adjudicated against
   the (b)-coin / style-family / sweep-2-census bars).

No sweep verdicts are claimed this run: running the 1–2 h sweep on a
tree that already failed acceptance would produce evidence for a tree
that cannot ship. Evidence log: `docs/gate.log`.
