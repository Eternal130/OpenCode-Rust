# GATE3 — beta.13 release gate (3rd consolidated run) — verdict: **NO-GO at Phase A.1**

**Supersedes** the GATE2 copy of this document (lane/gate2 657e825) and the GATE1
record (lane/gate f4ffc48). This run gate-checked main at **f02b1bc** — the commit
that cleared GATE2's two golden blockers (vscroll manifest re-pin 5027→5026,
session.status/session.idle publication removed per the v2-truth adjudication,
m1 golden re-pinned, BE-1 test rewritten to pin the v2 truth).

**Result: `cargo fmt --all -- --check` FAILS on f02b1bc's own test rewrite.**
Gate stopped at the first Phase A step per the STOP protocol; phases A.2–A.6
(clippy, workspace nextest, pty profile, release build, release frame budgets)
and Phase B (the 26-script sweep-3) were **not run**. Full transcript:
`docs/gate3.log`; raw fmt output `/tmp/opencode/gate3-fmt.log`.

## The blocker (deterministic, 3 hunks, one file)

`crates/oc-server/tests/be1_lane.rs` — lines 337, 345, 352 — the file f02b1bc
itself rewrote ("rewrite the BE-1 test to pin the v2 truth"). All three are pure
rustfmt line-wrapping violations, zero semantic content:

| line | violation |
|---|---|
| 337 | `expect_frame(...)` call exceeds line width (needs break after `=`) |
| 345 | `if frame.contains(..) \|\| frame.contains(..)` needs break-before-`\|\|` + brace |
| 352 | `assert!(!saw_status, "the server must not publish session.status/session.idle (v2 derives client-side)")` needs multi-line `assert!` wrap |

**Provenance**: worktree verified pristine at f02b1bc (`git status` clean except
this lane's docs); `git log -1 -- be1_lane.rs` → f02b1bc. The BE-1 fix lane's
own gate was nextest-scoped (oc-server) and did not run `cargo fmt --check`, so
the unformatted hunks landed on main unexamined.

## Fix (one command) + path to beta.13

1. On main: `cargo fmt --all` (only be1_lane.rs changes — confirmed no other
   file has diffs), commit, and **add `cargo fmt --all -- --check` to the fix
   lane's own gate** so a fourth gate is never burned on this class again.
2. Re-run the consolidated gate (GATE4) from that commit: A.1→A.6 + the
   26-script × 2-size × 2-run full-live sweep-3 (protocol unchanged, baseline
   sweep-2, adjudication whitelist per GATE2 spec).
3. FINAL lane (command blockers) had **not** merged as of this STOP (main tip
   bf7aa83 is docs-only, ROADMAP.md +2). GATE4 must re-checkout main's tip and
   will pick it up naturally.

## Phase scoreboard (3 gate runs)

| gate | stopped at | blocker | fixed on main by |
|---|---|---|---|
| GATE1 (lane/gate f4ffc48) | A.2 clippy | 6 F-1 test-code lints | fd78b23 |
| GATE2 (lane/gate2 657e825) | A.3 nextest | vscroll manifest 5027≠5026 + BE-1 event publications vs m1 golden | f02b1bc |
| GATE3 (this, lane/gate3) | A.1 fmt | 3 rustfmt hunks in be1_lane.rs (f02b1bc's own rewrite) | **pending — one `cargo fmt --all`** |

## Tag-readiness verdict

**NO-GO for v2.0.24.beta.13 at f02b1bc** — mechanical-only blocker (fmt), no
evidence gathered beyond A.1; the golden blockers GATE2 found are claimed fixed
but unverified by test run in this gate. With the one-command fix + GATE4, no
known substantive blocker remains between main and the tag.

## Verdict table

| phase | status | notes |
|---|---|---|
| A.1 fmt --all --check | **FAIL** | 3 diffs, be1_lane.rs only, deterministic, from f02b1bc |
| A.2 clippy -D warnings | NOT RUN | STOP protocol |
| A.3 workspace nextest | NOT RUN | STOP protocol (GATE2's two blockers claimed fixed, unverified here) |
| A.4 pty profile (serialized) | NOT RUN | STOP protocol |
| A.5 release build oc-cli | NOT RUN | STOP protocol |
| A.6 release frame budgets (strict) | NOT RUN | STOP protocol |
| B sweep-3 (26 scripts) | NOT RUN | STOP protocol |
