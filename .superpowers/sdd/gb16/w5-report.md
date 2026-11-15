# W5 — CI red hot-fix: FakeLlm real /prompt contract + walk timing margin

> **W5 follow-up (same day) — STOPPED, production change required.** The
> Concern-#1 gap-closer (adding `session.inbox.delivered` for
> `tabs_script`'s `msg_user_1` so the walk's session truly goes idle and
> the spinner/pump rests) was implemented and then REVERTED: with the
> pump calm, `composer_tabs_walk` fails DETERMINISTICALLY (~8.2 s,
> 3/3 runs locally) at the same "← never returned to Shell" leg — but
> from a latent PRODUCT render bug that the busy spinner's 100 ms
> repaints always papered over. Details and evidence below, after the
> original W5 sections. Final state = commit `ecc73cf1` unchanged.

Branch `gb16-w5`, base = origin/main 677b96a0. Test-only change (3 files under
`crates/oc-tui/tests/`); no production code touched.

## Root cause (verified before implementing)

W3's optimistic admit (client mints uuid, `admit_inbox_item` upserts by id,
`session_busy = pending ∨ running`) met a FakeLlm whose `/prompt` response was a
static `msg_user_1` echo and whose feed never fired `session.inbox.delivered`.
Result: the client-uuid row AND the scripted `msg_user_1` row both sat pending
forever ⇒ `session_busy` armed forever ⇒ tab spinner pegging the 100 ms render
pump; on a ~2.2x-slower CI runner (run 43708446481: local 2.7 s vs CI ~6 s for
the walk) `composer_tabs_walk`'s 4 s step windows saturated — the 3/3 red at
"← never returned to Shell".

## Changes

### 1. `vt_harness.rs` — FakeLlm real-contract prompt arm
* New dedicated POST arm for `/api/session/{id}/prompt`: reads the request
  body, answers an `Envelope<SessionInboxUser>` echoing the REQUEST body's
  client-minted `id` + `text` + the path's `sessionID` (old static
  `msg_user_1` body removed; `payload.text` stays present so the client
  decode never fails).
* New `delivered_feed: Arc<Mutex<Vec<String>>>` broadcast queue: the prompt
  arm pushes a `session.inbox.delivered` event body
  (`{"sessionID":…,"inboxID":<same id>}`) at the same moment it answers.
* The SSE feed drains the queue right after the first prompt POST is
  observed (500 ms grace; anchor unchanged so the script schedule does not
  shift) and keeps draining every 200 ms in the post-script park loop (later
  prompts get delivered too).
* `prompt_posts` increment moved into the new arm (no double count).

### 2. `pty_composer_tabs.rs` — walk timing margin
The two ← steps' wait windows 4 s → 6 s with a comment citing CI run
43708446481 (local 2.7 s vs CI ~6 s, runner ~2.2x). All other windows
untouched.

### 3. New contract-pinning test
`fake_llm_contract_tests::prompt_echoes_request_id_and_feed_delivers_clearing_pending`
(raw-TCP against a spawned FakeLlm + a store-level leg):
* the /prompt response echoes the request id / text / sessionID;
* the SSE feed fires `session.inbox.delivered` with the SAME inboxID;
* feeding that delivered event into a `Store` clears the pending row and
  flips `session_busy` false (busy = pending ∨ running).

### 4. Comment refresh in `pty_lane_if.rs`
The wheel test's "both stay pending — the ref-faithful resting shape"
comment updated: the CLIENT row is delivered now (W5); the script's
`msg_user_1` row still models no delivered event, so it stays pending at the
tail. The assertion (`"list the lines"` visible below L59) is unaffected —
verified by the full-suite green run; no assertion was relaxed.

## Consumer audit (who depends on the old pending-forever shape)

| Consumer | Script | Impact | Verified by |
|---|---|---|---|
| `streaming_thinking_script` (vt_temporal spinner, spec_scanab_a1 busy series, spec_e1_canary cadence) | holds the stream: `step.started` + deltas, NO `step.ended` | none — execution stays RUNNING, so busy stays armed through every spinner/busy observation window; the loading-indicator intent is preserved | full suite green |
| `busy_window_script` (vt_temporal busy strip/sweep) | same hold | none | full suite green |
| `tabs_script` (composer_tabs_walk) | enqueues `msg_user_1`, step ends | the client row now clears; the scripted `msg_user_1` row still pends (script sends no delivered for it) ⇒ busy stays armed there — same resting shape the test has always tolerated; no walk assertion touches spinner/busy | full suite green |
| pty_lane_if wheel | asserts the scripted `msg_user_1` pending row visible at tail | unchanged (that row still pends); comment refreshed only | full suite green |
| pty_subagent / pty_surf1 / pty_lane_km / pty_permission | enqueued `msg_user_1` rows | no pending-forever assertions; green | full suite green |
| `pty_core_interactions` | own `spawn_scripted_server`, not FakeLlm | out of scope, untouched | full suite green |
| `send_failure.rs` | store-level unit tests | untouched; already pins retract→busy-false | full suite green |

No test case required relaxing an assertion.

## Verification (all via `tools/dev-gate.sh`, full logs teed to `.test-logs/`)

* `cargo nextest run -p oc-tui --locked --no-fail-fast`:
  `Summary [ 228.495s] 1703 tests run: 1702 passed, 1 failed, 7 skipped` — the
  one failure was the whitelisted flake `e1_cells_paste_8k`
  (`test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 19.68s`);
  same-round `--exact` retry:
  `Summary [ 19.672s] 1 test run: 1 passed, 1709 skipped`.
* `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
* `cargo fmt --all -- --check`: clean (after one `cargo fmt --all`).
* `bash tools/guards/run.sh`: `guards: all green`.

## Concerns / residuals

* The scripted `session.inbox.enqueued` rows (inboxID `msg_user_1` etc.) still
  model no `session.inbox.delivered` BY DESIGN (they are how the fixtures paint
  the transcript's user row), so scripted sessions keep ONE pending row and
  `session_busy` stays armed in those tests. If a future CI saturation points
  at the render pump again, the scripts could add a delivered event for their
  own inbox ids — deferred here because no current assertion needs it and the
  walk's CI margin is covered by the widened windows.
* The delivered frame is drained per SSE connection; on feed reconnects the
  queue is already empty so the replayed script runs without a re-delivered
  frame (matches the real server: delivered fires once).

---

## Follow-up investigation (Concern #1): STOPPED — production render bug found

**Change attempted** (then reverted): `tabs_script` gains
`evt_deliv` (`session.inbox.delivered`, delay 200, inboxID `msg_user_1`)
after `evt_e` (`step.ended`, delay 180) — the real contract's close-out;
pending fully clears, session goes idle, spinner/pump rests.

**Result**: `composer_tabs_walk` now fails DETERMINISTICALLY (8.2 s, 3/3
local runs, `--profile pty` and default) at the very leg CI was red on —
`"← never returned to Shell"`. Grid dump at failure:

```
19:   ┃  Subagents  Shell  Terminals                                                                       esc
21:   ┃  cargo build
27:   ┃  tabsut return  kill ctrl+d  tabs ←/→
```

The hints row is CORRUPTED: `output return` lost its first four cells to a
stale `tabs` (left over from the Terminals tab, whose only hint group is
`tabs ←/→` painted at the same column).

**Root cause evidence (raw PTY byte capture, `/tmp/opencode/w5fu-raw.bin`)**:

* First Shell visit (correct): the app emits
  `ESC[28;6H … "output"` then `ESC[28;12H … " return"` — full hint run.
* Back-to-Shell visit (corrupted): the app itself emits
  `ESC[28;10H … "ut"` then `ESC[28;12H … " return"` — the emitter's diff
  believed cells 28;6-9 (`outp`) were already on screen and SKIPPED them,
  even though the Terminals frame had overwritten them with `tabs`.

So the app's frame/diff cache disagrees with what it actually emitted
(sparse-repaint correctness bug somewhere in the picker-hints render →
`render/opentui_emit.rs` / opentui-core `OptimizedBuffer` word-diff /
opentui-solid re-attach chain). The `picker_panel_paint` walk itself is
stateless (fresh `Vec<Placed>` per paint) — the staleness lives in the
frame diff or the solid tree's element reuse.

**Why it never showed before**: with a pending inbox row, `session_busy`
stayed armed, the tab spinner re-rendered every 100 ms, and each pump tick
re-issued a broad repaint that repaired the corrupted row within ~100 ms —
`wait_row`'s 25 ms polling always caught a good frame. The corruption was
TRANSIENT; delivering `msg_user_1` makes it PERMANENT (pump rests, no
repair tick ever comes).

**Decision**: per the standing rule (production change required → stop and
report), the script change and its debug instrumentation were reverted;
branch state = `ecc73cf1` (full suite green, verified in this session).
Any of the walk's other steps could equally hit a stale-sparse frame once
the pump calms — widening windows cannot fix a permanent corruption.

**Options for the orchestrator**:

1. Fix the render/diff staleness (production: `render/opentui_emit.rs`
   cache, or opentui-core/upstream `OptimizedBuffer`, or opentui-solid
   element diff), then re-land the `evt_deliv` script change (it is
   ready: 5-line ScriptEvent, evidence in `.test-logs/*-w5fu-*`).
2. Keep W5 as-is: the client-uuid row delivers (the W3 regression's
   double-pending is gone — the pump load is halved vs the worst case)
   and CI margin rides on the 6 s windows. Residual risk: the scripted
   `msg_user_1` row keeps one pending row armed for the whole walk.
3. Investigate whether the stale cells come from the solid tree's
   per-key re-attach (comments in the repo say "the port re-attaches
   per key") — a targeted prod fix might be small.

**Post-revert re-measurement sharpens the picture**: at the reverted
baseline (`ecc73cf1`) `composer_tabs_walk` is FLAKY locally at the SAME
leg — 4/4 runs pass only via nextest retries (3× `1 passed (1 flaky)`,
TRY 1 FAIL at "← never returned to Shell"; 1× clean 2.7 s pass; box load
~0.2). So the corruption is a race that exists WITHOUT any delivered
event (the pump is hot the whole time at baseline — the spinner repaints
re-emit the frame, meaning the corrupted frame itself can persist across
pump ticks; repair comes only from a lucky re-render, not from the pump).
This matches the whitelisted `composer_tabs_walk` flake and the CI 3/3
red (all tries): CI's slower box shifts the race to near-certain
corruption, and the `evt_deliv` variant (calm pump) makes it
deterministic locally — 3/3 runs, every try red.

**Implication for W5's hot-fix premise**: the 4 s → 6 s widening treats
a saturation problem, but the measured failure mode is a persistent
corrupted frame, which no window length fixes. The durable fix is the
product-side staleness repair (options above); the windows merely buy
retry odds.

**Evidence files**: `.test-logs/oc-tui-*-w5fu-tabs-*.log`,
`-w5fu-walk.log`, `-w5fu-dbg.log`, `-w5fu-dbg2.log` (grid + raw-byte
dumps), `-w5fu-base-*.log` (baseline flake measurement),
`-w5fu-baseline.log`.
