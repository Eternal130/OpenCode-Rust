# GATE5 — beta.13 release gate (5th independent scan) — verdict: **GO**

**Supersedes** the GATE3 copy of this document (lane/gate3 7b340a4; GATE4 never
produced a sweep — it STOPped at Phase A.3 on the m1 event-ordering flakes that
the DETERM merge subsequently fixed). This run gate-checked main at
**d52ba75** = the lane/final merge (9c22b02: provider.connect real
DialogIntegration flow, 7 stub dispatches + 6 dead session commands wired, zero
stub text). Full transcript: `docs/gate5.log`; sweep artifacts:
`/tmp/opencode/sweep3/` (per-script report snapshots under `reports/`).

**Protocol event mid-gate**: FINAL merged AFTER this gate's Phase A had run on
the pre-merge tip 1b7a186 (A.4 had just timed out at the 45-min ceiling under
FINAL's own concurrent sweep load). Per protocol the worktree was re-checked
out to main tip **before Phase B**, and — because a gate may only certify the
tip it scanned — **Phase A was rerun in full on d52ba75**. All Phase A numbers
below are from the FINAL tip; the superseded 1b7a186 results (also green)
remain in `docs/gate5.log` for provenance.

## Phase A — 6/6 green on d52ba75 (STOP protocol never fired)

| step | command | result |
|---|---|---|
| A.1 | `cargo fmt --all -- --check` | PASS (clean) |
| A.2 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS (no lints) |
| A.3 | `cargo nextest run --workspace --locked` | PASS — 3330/3330, 12 skipped; **3 full executions** (runs 2–3 with `--retries 0`), zero flaky marks in runs 2–3 |
| A.3b | m1 10x loop (`m1_scenario_prompt_fake_llm` + `m1_scenario_config_touch`, fresh process each) | PASS — **10/10 clean**, no retries, no flaky marks (DETERM holds on FINAL tip) |
| A.4 | `cargo nextest run --workspace --locked --profile pty` (serialized, test-threads=1) | PASS — 3330/3330 in 1155s (first gate ever to execute A.4; the one 45-min timeout was load interference from FINAL's concurrent sweep, not a failure — no orphans left) |
| A.5 | `cargo build --release -p oc-cli --locked` | PASS (3m23s, thin-LTO profile) |
| A.6 | `cargo nextest run -p oc-tui --release -E "test(frame_budget)"` | PASS — 12/12 (strict nominal budget is the local default; CI-scale env unset) |

### Flag (non-blocking, fully characterized)

Full-workspace run #1 marked `oc-client::transport
sse_decoding_multi_data_heartbeats_and_crlf` **FLAKY 2/2** (try-1 fail → try-2
pass, same second). Characterization: **unreproduced in 33 subsequent
executions** (30x isolated `--retries 0`, then 2 more full-workspace
`--retries 0` runs, all green). Mechanism, code-cited: `oc-client/src/sse.rs:96`
fires `on_activity` **per non-empty chunk read**, while the test's mock
separates its write groups only by 30ms `std::thread::sleep`s
(`tests/transport.rs:370,374`) on a current-thread `#[tokio::test]` runtime —
under 16-way workspace parallelism the executor can miss the 30ms window, the
chunks coalesce into one read, and the final `assert!(activity >= 2, "onActivity
per chunk")` fires. The decoder's frame-content assertions are unaffected.
Classification: **test-harness timing sensitivity, not a product defect; never a
hard failure** (exit 0 in every full run). Per GATE4 precedent (flaky-PASS =
recorded observation; only exhausted-retry failures stop gates) this did not
STOP the gate. Recommendation to the loop: a small hardening lane (read-gated
mock writes or larger separation).

## Phase B — sweep-3: full corpus, FULL live, cache OFF

26 scripts × both sizes (per-script size overrides honored: click-coverage-*
@80x24, sidebar-wide + subagent-sidebar @160x36, subagent-browser @80x24) ×
`--runs 2`, one script per invocation, `--no-ref-cache` (fresh bun PTY + ref
server every run), stale `target/debug/opencode-rust`+`oc-server` deleted
before every invocation. **92/92 run-rows booted both sides, zero errors, zero
skipped steps.** Adjudication baseline: sweep-2 (`docs/final-sweep-2.md` +
`docs/msweep2-rig.log`); per-step signatures compared against
`/tmp/opencode/final-sweep-2/reports/`.

### Per-script verdict table (totals over size×run; i/t/m = identical/text-only/mismatch)

| script | sweep-3 i/t/m | sweep-2 i/t/m | Δm | verdict |
|---|---|---|---|---|
| boot-home | 6/0/6 | 0/0/12 | −6 | better (coin) |
| click-coverage-dialog | 4/0/14 | 8/0/10 | +4 | coin-only: r0 step set + step-7 first-diff/cell-samples byte-identical to s2 r0; r1's 4 extra = placeholder coin |
| click-coverage-home | 6/0/4 | 10/0/0 | +4 | coin-only (all 4 = placeholder row-13; r1 fully identical) |
| click-coverage-session | 10/0/20 | 12/0/18 | +2 | substantively **better**: s2 steps 2–3 (297/107 mism, `Select↔Switch` title pair) now 1/4 style-only backdrop cells; +3 coin steps |
| composer-clear | 20/0/20 | 25/0/15 | +5 | coin-only (r0@80x24 set {0,1,7,8,9} ≡ s2 r0; Δ = r1 coin flip; 120x30 ≡ s2) |
| help-dialog | 18/0/14 | 15/0/17 | −3 | better (coin migration only) |
| hover-sweep | 12/0/56 | 8/0/60 | −4 | better; shifts within registered classes |
| long-reply-scroll | 11/0/53 | 14/0/50 | +3 | coin-only; step sets ≡ s2; pageup divergence same family (char 304 both sweeps) |
| mouse-hover-tabstrip | 14/0/54 | 17/0/51 | +3 | coin-only (steps {0,1,6} migrated r0↔r1) |
| multi-session | 17/0/83 | 11/0/89 | −6 | better |
| multi-turn | 19/4/73 | 22/4/70 | +3 | coin-only (stats digits); text-only 4 ≡ |
| palette | 4/0/20 | 0/0/24 | −4 | **better, lane-documented**: palette-open 64→1, palette-down 145→1 — exactly l1.log:76's L1 fix numbers |
| rename-dialog | 11/0/49 | 11/0/49 | 0 | exact |
| resize | 6/0/18 | 5/0/19 | −1 | better (120x30 r0 fully identical) |
| session-list-dialog | 17/0/39 | 8/0/48 | −9 | better |
| sidebar-wide | 7/0/25 | 4/0/28 | −3 | better |
| subagent-basic | 14/4/38 | 14/4/38 | 0 | exact |
| subagent-browser | 10/2/30 | 4/2/36 | −6 | better |
| subagent-sidebar | 7/2/29 | 10/2/26 | +3 | coin-only (C-hint position + coin; farewell ids text-only) |
| tabs-keyboard | 8/0/24 | 16/0/16 | +8 | coin-only (all-or-nothing placeholder; 120x30 r1 fully identical) |
| tabs-live-session | 14/0/82 | 17/0/79 | +3 | coin-only |
| themes-nord | 14/0/18 | 4/0/28 | −10 | better (nord-swatch coin forms) |
| thought-expand | 11/0/45 | 8/0/48 | −3 | better |
| thought-expand-structural | 14/0/54 | 11/0/57 | −3 | better |
| thought-render | 14/0/30 | 14/0/30 | 0 | exact |
| type-edit-submit | 25/0/27 | 16/0/36 | −9 | better |
| **TOTAL** | **313/12/925** | **284/12/954** | **−29** | net better, text-only flat |

### Class census — zero new divergence classes, zero unexplained signatures

Every mismatch step observed decomposes into exactly the registered set:

* **(b)-coin (per-run random)** — home-placeholder rotation (row 13@80x24 /
  16@120x30 / 19@160x36; pool chars `a/b/W/F`; compositions 21/20/10,
  39/38/13, 40/37/23 varying only with the drawn pair), stats-row digits
  `(26–28, 7–8)` incl. the ms-unit width shift (`'6'↔'m'`), `Thought · Nms`
  suffix (idx94 `(17,6–7)`), farewell session-id chars (`ses_…`, text-only).
* **Registered style families** — backdrop-fg idx15↔idx8 on bg232 (incl.
  bg idx234↔idx232 row-0/1 cells, the `'┃'` idx73 cell, and themes-nord's
  idx103/239 swatch forms); tab-title/mini-footer cluster idx251/246↔idx255
  [bold]; new-tab idx236↔idx255; tab ✕ idx7↔idx15; composer `C` hint
  idx255↔idx15; **L1 action-row `'+'` idx215↔idx94** (l1.log:16).
* **Registered deterministic text** — models-dialog title `Select model`↔
  `Switch model` (`'e'↔'w'` idx255/233 [bold]) — still present in several
  scripts (unchanged class), **eliminated** from click-coverage-session
  steps 2–3 by the L1 models-panel work.
* **Registered backlog** — long-reply-scroll pageup `'Jump to latest ↓'`
  viewport divergence (same family both sweeps; char-count 304 identical);
  markdown list-marker/inline-concealment `'·'→' '` idx94↔idx15 (in-step
  cells, upstream-faithful, goldened in M-retire).

**Lane-documented surface changes verified, none divergent**: FINAL's
session.export palette row matches the ref (v2 lists it — no palette-content
divergence); FINAL's wired dialogs/commands introduced zero new visible-class
diffs; TAIL's pulse/marquee/shimmer/scrim surfaces produced no cross-sweep
signature drift; SURF-1's location-footer/MCP cells are byte-stable vs the ref.

## Tag-readiness

**GO for v2.0.24.beta.13.** The bar — Phase A completely green on the exact
tag-candidate tip (d52ba75), the m1 DETERM fix holding 10/10-in-a-row, and a
full-live corpus sweep that is net-better than sweep-2 with every mismatch
decomposing into registered or lane-documented classes and zero unexplained
signatures — is met. One non-blocking flag recorded (oc-client SSE test
timing sensitivity, characterized and code-cited above) with a recommended
small hardening follow-up.
