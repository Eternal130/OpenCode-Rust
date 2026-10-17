# Lane GATE2 — beta.13 acceptance evidence: **NO-GO (Phase A.3 blocker ×2)**

**Supersession.** This run supersedes the prior GATE record
(`lane/gate` f4ffc48: NO-GO at Phase A.2). That blocker — the F-1
test-code clippy debt — is **fixed on main** by `fd78b23`, and this
lane **re-verified A.1 and A.2 green** on the fixed tree before
proceeding. The NO-GO verdict below is a **new, distinct** blocker
surface, one phase later.

**Scope intended.** The consolidated verify-lane final (Phase A) +
the full 26-script live differential sweep (Phase B, sweep-3) as the
v2.0.24.beta.13 tagging evidence, run on `lane/gate2` (worktree
`.worktrees/lane-gate2`, base `fd78b23` = main with the A.2 fix).

**Outcome.** Phase A.3 — the full-workspace nextest — **FAILED**
with two deterministic, tree-wide failures, both of the same
meta-class the consolidated gate exists to catch: **a post-beta.12
lane's crate-scoped gate missed a cross-crate golden consistency
break**. Per the lane protocol (any Phase A failure = STOP, log
evidence, report, do not fix), the gate halted at A.3: phases A.4–A.6
and the entire Phase B sweep were **not run**. beta.13 is **not
tag-ready**.

## Phase A results

| # | phase | result |
|---|---|---|
| A1 | `cargo fmt --all -- --check` | **PASS** (0 diffs) |
| A2 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | **PASS** (1m11s — the 6 F-1 lints cleared by `fd78b23`, verified) |
| A3 | `cargo nextest run --workspace --locked --no-fail-fast` | **FAIL** — 3322 tests: 3320 passed (2 slow, 1 flaky, 1 leaky), **2 failed**, 12 skipped |
| A4 | nextest `--profile pty` (serialized) | NOT RUN (protocol STOP at A3) |
| A5 | `cargo build --release -p oc-cli --locked` | NOT RUN |
| A6 | release frame budgets (strict, no scale env) | NOT RUN |
| B | 26-script live sweep (`--runs 2 --no-ref-cache`, both sizes) | NOT RUN |

## The blockers (complete fix list + provenance)

### F1 — tui-frames golden corpus inconsistent with its own manifest (tail(4) residue)

`parity tui_frame::tests::golden_corpus_on_disk_verifies_against_manifest`
fails deterministically (identical on both nextest tries):

```
vscroll-top.w80.ansi:    length 5026 != manifest 5027  (sha mismatch)
vscroll-middle.w80.ansi: length 5026 != manifest 5027  (sha mismatch)
```

* **Provenance**: `1a43069` (tail(4) theme tokens, jump-to-latest
  info fg) re-pinned exactly these two `.ansi` goldens
  (`PARITY_UPDATE=1` — "one fg cell each", per `docs/tail.log`) but
  did **not** regenerate `goldens/tui-frames/manifest.json`. The
  other 42+ blobs (incl. the matching `.txt` twins and
  `vscroll-bottom`/`vscroll-stick-bottom`) verify clean — the
  inconsistency is precisely the two re-pinned files.
* **Why the lane missed it**: tail(4)'s gate was
  `cargo nextest -p oc-tui 805/805` — the manifest-verification test
  lives in the `parity` crate (`tools/parity/src/tui_frame.rs:1461`),
  outside that filter.
* **Fix** (mechanical, zero production surface): regenerate the
  manifest entries for the two re-pinned blobs (the
  `PARITY_UPDATE=1` export flow / manifest writer), then the test
  passes as it did pre-`1a43069`.

### F2 — m1 `prompt_fake_llm` golden predates BE-1's live event publications

`parity::rust_scenarios m1_scenario_prompt_fake_llm` fails
deterministically — 4/4 attempts today (2 in the workspace run, 2 in
an isolated standalone rerun), byte-identical signature each time:

```
sequence divergence in group golden group #2 at frame 1:
  reference = session.usage.updated
  port      = session.status {status:{type:busy}}
```

The live capture (`/tmp/opencode/m1-artifacts/pid-1562970`, preserved)
vs the pinned golden (`goldens/scenarios/prompt_fake_llm.json`,
phase-3 vintage `0cf3779`) differs by **four inserted frames**:

| group | golden | live capture |
|---|---|---|
| #2 (prompt turn) | `execution.started, usage.updated, execution.succeeded` | `execution.started, **session.status(busy)**, usage.updated, execution.succeeded, **session.status(idle), session.idle**` |
| #3 | `server.connected` | `server.connected, **models-dev.refreshed**` |

* **Provenance**: `239a990` (BE-1 lane, 2026-10-16 20:19 — **after**
  the beta.12 tag `fe1ebb7`, 2026-10-16 04:44) "publish the 14
  missing public event types at v2 emission points" added the live
  emissions: `session.status` busy/idle + `session.idle`
  (`oc-core/src/session/execution.rs:819-833`) and
  `models-dev.refreshed` (`provider_catalog/refresh.rs`). The pinned
  m1 golden predates all of them, and the scenario's skip-list has
  been **empty** since gate-6 (`b2478bd`, "prompt_fake_llm now ZERO
  skips") — the comparison is exact and fails structurally.
* **NOT the dogfood2 flake**: `docs/dogfood2.log` 9C registered this
  test as a timing flake that passes on retry — that framing predates
  `239a990`. Today it is a structural insertion with zero variance;
  the "flaky" tag nextest showed applies to the *other* m1 test
  (`m1_scenario_location_reload`, TRY1-FAIL/TRY2-PASS, known class,
  not a blocker).
* **Why the lane missed it**: BE-1's gate was crate-scoped
  (oc-server 193/193, oc-core 764/764+279/279, oc-cli 184/184 —
  `docs/be1.log` Phase 4); the `parity` m1 replay is in none of them.
* **Fix** (needs one adjudication, not mechanical):
  1. **If the v2.0.24 reference server emits these frames at prompt
     time** → re-capture the golden live (the `scenarios.rs` reference
     twin with `PARITY_UPDATE=1`) — the golden then carries them and
     exact comparison resumes.
  2. **If the reference does not** (plausible: `execution.rs:823`
     notes the *ref client* derives the busy/idle union from
     execution events — `client/src/solid/data.ts:1009-1037` — so the
     reference *server* may never publish `session.status`) → either
     re-introduce a documented skip-list entry for the port's
     derived-union publications, or gate the emission at the seam.
     The BE-1 log (`docs/be1.log:10`) cites schema
     `session-status-event.ts:35-53` for the *event*, which speaks to
     the wire type existing, not to the reference publishing it —
     hence the adjudication.

## Path to beta.13

1. One small lane: (a) regenerate the tui-frames manifest (F1);
   (b) adjudicate + apply the F2 fix (re-capture vs skip-list/gate).
   Both are goldens/test-code only; zero production surface.
2. Re-run lane GATE **in full**: Phase A (fmt → clippy → nextest →
   pty profile → release build → strict frame budgets) then Phase B
   (the 26-script sweep-3, both sizes × `--runs 2`, `--no-ref-cache`,
   adjudicated against the (b)-coin / style-family / sweep-2-census
   bars).

No sweep verdicts are claimed this run: the 1–2 h Phase B sweep on a
tree that already failed acceptance would produce evidence for a tree
that cannot ship (same ruling as lane/gate f4ffc48). Evidence log:
`docs/gate2.log`; full transcripts `/tmp/opencode/gate2-clippy.log`,
`/tmp/opencode/gate2-nextest.log`, m1 capture
`/tmp/opencode/m1-artifacts/pid-1562970/`.
