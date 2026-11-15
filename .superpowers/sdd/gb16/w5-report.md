# W5 — CI red hot-fix: FakeLlm real /prompt contract + walk timing margin

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
