# Runner concurrency contract

> Contract doc for the concurrency comments in crates/oc-core/src/session/runner/ (written 2026-10-23 from the implementation; the citations predate this doc).

This document records the concurrency contract the session runner implements: how the
LLM event stream, local tool executions, interrupts, and durable settlement compose
without locks on the hot path. Sections match the numbered citations in the code:

| § | Topic | Cited from |
|---|-------|-----------|
| 1 | Single-writer `LLMEventPublisher` + event mapping | `runner/mod.rs:15`, `runner/publisher.rs:2` |
| 2 | Tool fibers + per-tool interrupt bell | `runner/step.rs:3`, `runner/step.rs:176`, `runner/step.rs:576` |
| 3 | The single-writer step loop | (bridges §2 → §4) |
| 4 | Shutdown watch, uninterruptible guard regions, settling | `runner/step.rs:3`, `runner/step.rs:163`, `session/execution.rs:52` |
| 5 | Execution mapping contract (coordinator) | `session/execution.rs:12` |

Line citations below point into the working tree as of 2026-10-23; paths are relative to
the repo root (`crates/oc-core/src/...`). Upstream v2 citations
(`/opt/opencode-ref/packages/core/src/...`) are informational; the Rust citations are
authoritative.

---

## §1 — The single-writer `LLMEventPublisher`

Port of `runner/publish-llm-event.ts`
(`crates/oc-core/src/session/runner/publisher.rs:1-2`;
upstream: `packages/core/src/session/runner/publish-llm-event.ts`).

### Ownership model

The publisher is **not** a shared struct with mutexes. It is a single-owner state
machine driven exclusively by the step loop, which holds `&mut LlmEventPublisher`
(`publisher.rs:4-10`, `publisher.rs:230`). One instance is constructed per attempt
(`step.rs:191-199`). Tool fibers never touch the record: they send settled exits over
an mpsc channel and the loop performs every record mark and publish
(`publisher.rs:8-10`, `step.rs:202`, `step.rs:388-395`). Settlement marks therefore
happen in the loop's single-threaded order.

### Mark-before-await discipline

Every method mutates the record **synchronously before its first `await`**, so
check-then-mark races are impossible by construction (`publisher.rs:5-7`,
`publisher.rs:559-563`). Concrete sites:

- `ToolResult` marks `settled = true` before the terminal
  `session.tool.success`/`session.tool.failed` publish (`publisher.rs:710-715`);
- `ToolError` marks settled before publishing (`publisher.rs:777-780`);
- malformed tool input marks settled before the terminal publish (`publisher.rs:521-522`);
- `tool_execution` (local exits) marks settled before publishing (`publisher.rs:889-892`);
- `fail_tool` marks settled before publishing (`publisher.rs:939-943`);
- `ToolCall` marks `called = true` synchronously inside the record block
  (`publisher.rs:660-665`).

This is the Rust rendering of v2 wrapping publisher writes in
`Effect.uninterruptible` (upstream `publish-llm-event.ts:558`; see also the
"uninterruptible through its writes" note at upstream `:73`): a publish that has begun
cannot strand a mark without the frame.

### LLM event → durable event mapping

`publish()` first flushes due delta batches (`publisher.rs:564-565`), then switches on
the provider event:

| `LLMEvent` | Record marks (synchronous) | Durable publishes |
|---|---|---|
| `StepStart` | — | `session.step.started`, once per attempt (idempotent guard `step_started`, `publisher.rs:315-327`) |
| `TextStart` | `output_started = true`; fragment opens (`publisher.rs:570-577`) | pending batches → `session.text.delta`, then `session.text.started` |
| `TextDelta` | appends to the fragment's pending batch (`publisher.rs:578-581`) | batched — see below |
| `TextEnd` | fragment closes (`publisher.rs:582-590`) | trailing `session.text.delta` (if any), then `session.text.ended` |
| `ReasoningStart/Delta/End` | same shape over the reasoning family (`publisher.rs:591-611`) | `session.reasoning.started/delta/ended` |
| `ToolInputStart` | `output_started = true`; `ToolRecord` inserted with `called = false` (`publisher.rs:612-615`, `publisher.rs:445-470`) | `session.tool.input.started` |
| `ToolInputDelta` | appends; name must match the record (`publisher.rs:616-632`) | none in-band — the text accumulates and publishes joined at end (`publisher.rs:472-493`) |
| `ToolInputEnd` | fragment closes (`publisher.rs:633-635`) | `session.tool.input.ended` with the joined text |
| `ToolInputError` | settled marked before the terminal publish (`publisher.rs:521-522`) | `session.tool.failed` with `tool.input-json` (`publisher.rs:636-639`, `publisher.rs:495-537`) |
| `ToolCall` | `called = true`; duplicate call is a defect (`publisher.rs:640-665`) | `session.tool.called` (`publisher.rs:666-674`); starts an implicit input fragment when the provider skipped one (`publisher.rs:644-649`) |
| `ToolResult` (hosted only) | settled marked before the terminal publish; a late *error* result on a settled tool is a benign straggler, a late success is a defect (`publisher.rs:676-715`) | `session.tool.success` or `session.tool.failed` (`tool.execution`) (`publisher.rs:716-758`) |
| `ToolError` | settled marked before the terminal publish (`publisher.rs:759-780`) | `session.tool.failed` — `tool.unknown` for `Unknown tool: …`, else `tool.execution` (`publisher.rs:781-807`) |
| `StepFinish` | `StepSettlement` recorded; duplicate finish is a defect; content-filter also sets `provider_failed` and runs `fail_assistant` (`publisher.rs:808-827`) | flush, then (for content filter) the failure path; terminal frame deferred to settlement (§4) |
| `Finish` | — | ignored (`publisher.rs:828`) |
| `ProviderError` | `provider_failed = true` (`publisher.rs:829-838`) | `fail_assistant` with `provider.unknown` |
| `Compaction`, `Media` | — | ignored (`publisher.rs:839`) |

Loop-side (not `LLMEvent`-driven) publishing: `session.step.streamed` when the response
body ended (`publisher.rs:337-343`), `session.tool.progress` for live metadata
(`publisher.rs:845-859`), `session.tool.success` for locally executed tools via
`tool_execution` (`publisher.rs:861-901`), `session.tool.failed` via `fail_tool`
(`publisher.rs:928-966`), and the terminal `session.step.failed` / `session.step.ended`
carrying settlement details (cost, tokens, snapshot, files) (`publisher.rs:1004-1039`,
`publisher.rs:1050-1080`).

### Delta batching

Text and reasoning deltas coalesce per v2's `deltaBatchInterval` of 100 ms
(`publisher.rs:96-99`; upstream `publish-llm-event.ts:79`). Appends accumulate into a
pending batch (`publisher.rs:115-126`); a batch publishes when:

- the interval elapsed — checked as the next stream event arrives (v2's forked timer is
  approximated on arrival; publish *order* is preserved — every batch precedes the next
  frame) (`publisher.rs:128-144`, `publisher.rs:159-170`, `publisher.rs:361-372`, `publisher.rs:564-565`);
- a fragment starts — all pending batches flush first (`publisher.rs:146-157`,
  `publisher.rs:349-359`); or
- the fragment ends — an unpublished trailing batch flushes immediately before the
  `ended` frame (`publisher.rs:183-188`, `publisher.rs:582-611`).

`flush()` force-ends every open text/reasoning/tool-input fragment (`publisher.rs:903-926`)
and runs at the head of `fail_assistant` and `StepFinish` processing
(`publisher.rs:808-809`, `publisher.rs:993-1002`).

### Defects

Publisher invariant violations (`Effect.die` in v2) surface as `PublisherError`:
duplicate starts, delta/end before start, start-before-end on the single-fragment
families (text/reasoning allow one open fragment; tool input allows many —
`publisher.rs:101-113`, `publisher.rs:276-278`), tool name changes, tool events before
assistant start (`publisher.rs:329-335`), duplicate call/result/error/execution, tool
result/progress outside a running call, executions with no content, and duplicate step
finish (`publisher.rs:35-45` and the `defect(...)` sites in the table above).

### Invariants

1. **Single writer**: only the step loop's `&mut` publisher mutates the record; tool
   fibers communicate only via the outcome channel (`publisher.rs:4-10`).
2. **Mark-before-await**: every record mutation completes before the first `await` of
   its publish; a started publish cannot strand a mark (`publisher.rs:5-7`).
3. **Once-only frames**: `session.step.started` publishes at most once per attempt;
   terminal tool events and the step finish are single-shot (duplicates are defects).
4. **Ordering**: delta batches always precede the frame that flushed them; the
   terminal `session.step.ended`/`session.step.failed` publishes only after `flush()`
   has closed every open fragment and every unsettled tool has a terminal
   (`publisher.rs:903-926`, `publisher.rs:993-1002`, `publisher.rs:1004-1080`).
5. **Hosted vs local split**: provider-hosted results settle inside `publish()`
   (`ToolResult`); locally executed tools settle only through `tool_execution` driven
   by the outcome channel (`publisher.rs:684-687`, `publisher.rs:861-901`).

---

## §2 — Tool fibers and the per-tool interrupt bell

### Fibers

A local tool execution starts only after its `session.tool.called` publication has
completed (`step.rs:277-278`, `step.rs:269-306`); provider-executed calls are skipped
(`step.rs:274-276`), and a `tool_choice: none` request fails the call durably instead
of executing (`step.rs:279-295`). Each execution is a `tokio::spawn`ed task
(`step.rs:305`, handles collected at `step.rs:204`) reporting one
`ToolExit` — `Success`, `Error` (execution/permission), `Declined`, or `Interrupted` —
over an **unbounded** mpsc channel (`step.rs:112-131`, `step.rs:202`), so fibers never
block on send and exits queued after the loop moved on are still drained
(`step.rs:388-392`). This is the counterpart of v2's per-tool Effect fibers
(upstream `step.ts:85`, spawned under `uninterruptibleMask` at upstream `:120`).

Fibers never touch the publisher record; settlement marks happen when the loop drains
the channel (`publisher.rs:8-10`, `step.rs:249-251`, `step.rs:393-395`).

### The bell

`ToolInterrupt = Arc<watch::Sender<bool>>` — one shared bell per attempt
(`step.rs:176-181`, created at `step.rs:203`). Semantics:

- **Latching**: a `watch` holds its value; a fiber reaching its await point after the
  ring still observes `true`. Fibers therefore check the **current** value before
  selecting — a receiver created after the ring marks the change as seen, so
  `changed()` would never fire; the fresh-borrow check closes the spawn→first-poll
  window and cancels the execution before it is ever polled (`step.rs:576-598`).
- **Ringing**: the finalization phase sends `true` once for all fibers — when the
  stream loop latched the execution interrupt (`step.rs:329-334`), when the join phase
  observes the shutdown watch mid-join (`step.rs:335-341`, `step.rs:357-369`).
- **Report**: a fiber interrupted by the bell sends `ToolExit::Interrupted` over the
  outcome channel (`step.rs:599-601`), so classification stays in one place — the loop
  folds those exits into the `TOOLS_INTERRUPTED` step failure at classification
  (`step.rs:409-410`, `step.rs:479-495`; error factory `step.rs:46-55`).

The fiber body is a biased select — bell first, execution second — mapping
`ToolRunFailure::{Error, PermissionRejected, Declined}` onto the exit
(`step.rs:585-641`).

### Bounded joins

`TOOL_JOIN_TIMEOUT = 5 s` (`step.rs:41-44`): once interrupting, the join of each fiber
is bounded; a wedged fiber is aborted and classified **interrupted, never a die**
(`step.rs:343-352`, `step.rs:374`). A panicked fiber is a defect (`ToolDie`,
`step.rs:376-385`); a fiber aborted while interrupting reads as interrupted
(`step.rs:379-381`). Tool-output truncation (the 2000-line/50 KiB budget) runs during
exit settlement, before the terminal `session.tool.success` publish
(`step.rs:645-686`).

### Invariants

1. **Publish-called-then-execute**: a local execution spawns only after its
   `session.tool.called` publish completes (`step.rs:277-278`).
2. **Bell latches**: once rung, every fiber — parked or not yet polling — observes it
   (current-value checks at `step.rs:592-598`; the coordinator loop applies the same
   guard to its shutdown watch at `step.rs:242-247`).
3. **One exit per fiber**: each fiber sends exactly one `ToolExit`; the loop settles
   each exit at most once (duplicate settlement is a publisher defect, §1).
4. **No fiber-side record access**: fibers communicate only over the unbounded
   outcome channel (`step.rs:202`, `publisher.rs:8-10`).
5. **Wedged fibers cannot stall settlement**: bounded joins abort stragglers and
   classify them interrupted (`step.rs:346-352`).

---

## §3 — The single-writer step loop

One loop owns the attempt (`step.rs:228-316`): a **biased** `tokio::select!` over
three sources, in priority order (`step.rs:229-234`):

1. the execution's shutdown watch — `shutdown.changed()` (`step.rs:231`);
2. tool exits — `outcome_rx.recv()` (`step.rs:232`);
3. the provider stream — `polled.next()` (`step.rs:233`).

Shutdown latches into `stream_interrupted` and leaves the stream loop
(`step.rs:235-241`); the watch's **current** value is re-checked every iteration
because a cloned receiver marks pre-clone changes as seen (`step.rs:242-247`).
Everything from classification onward is the uninterruptible region (§4).

Stream handling:

- A connect-phase `stream()` failure enters the *same* settlement as a first-poll
  stream error — the failure surfaces inside stream consumption so
  `fail_assistant` + `publish_step_failure` still run (`step.rs:213-226`).
- A stream error event is recorded and the stream is still consumed to its end
  (`step.rs:308-312`); events arriving after a parked overflow or a recorded provider
  failure are dropped (`step.rs:254-256`).
- A pre-first-token context-overflow `ProviderError` is **parked** — not published —
  until after the overflow-recovery decision (`step.rs:257-267`, resolved at
  `step.rs:412-421`).
- Tool exits settling mid-stream are published inline in arrival order
  (`step.rs:249-251`); exits arriving during finalization drain after the join and
  settle in channel order (`step.rs:388-395`).

### Invariants

1. **One writer**: the loop is the only code that marks the publisher record and
   publishes (§1), so stream events, tool exits, and interrupt latching serialize
   through a single order.
2. **Shutdown beats stream beats nothing**: the biased order means an interrupt is
   observed before any event that became ready in the same poll, and tool exits beat
   stream events (`step.rs:229-234`).
3. **Interrupt latches exactly once**: whichever await observed it (stream loop or
   join phase), the latch `stream_interrupted` is monotonic and re-raises after
   settlement (`step.rs:208-211`, `step.rs:555-557`).

---

## §4 — Shutdown watch, guard regions, and settling

### The signal path

The coordinator owns a per-execution `watch::Sender<bool>`
(`execution.rs:118-121`); `interrupt()` signals it **first**, then rings the stop bell
(`execution.rs:296-302`). The receiver travels in-band through the drain seam:
`DrainFn`'s last argument (`execution.rs:133-141`) → `drain()` (`drain.rs:180-189`,
`drain.rs:198`) → `run_step` (`drain.rs:234`, `drain.rs:552`) → `StepDeps.shutdown`
(`drain.rs:677`, `step.rs:163-167`). The attempt observes it only at await points
(`step.rs:227-247`, join phase `step.rs:335-372`).

### The interrupt is a signal, not a drop

The coordinator **never drops the drain future** on interrupt. The execution loop
polls the pinned drain under a biased select (stop bell vs drain completion,
`execution.rs:428-436`); when the bell rings, it keeps polling — bounded by
`UNWIND_TIMEOUT = 60 s` (`execution.rs:51-55`) — so the drain observes the watch,
unwinds through its guard regions, and settles on its own. Only a drain that never
returns within the timeout is abandoned (the only way out of a wedge) and classified
`RunError::Interrupted` (`execution.rs:414-427`).

### Step-side guard regions

Finalization (`step.ts`'s `uninterruptibleMask`, upstream `step.ts:135-144`) spans
`step.rs:318-573` in numbered order:

1. `streamed()` once the stream exited (`step.rs:319-322`);
2. join the tool fibers (this sub-phase **still observes the interrupt** — v2 wraps
   `Fiber.awaitAll` in `restore`: ring the bells for live fibers, bounded-join, drain
   and settle all outcomes) (`step.rs:324-395`);
2b. classify tool exits / interruption flags (`step.rs:401-410`);
3. overflow recovery → `Outcome::Compacted` (`step.rs:412-417`);
4. `unknownFinish` → incomplete-stream error (`step.rs:422-431`);
5. `RecoverFull` (`step.rs:435-441`);
6. retry decision (`step.rs:443-459`);
7. `fail_assistant` (`step.rs:460-463`);
8. declines (`step.rs:465-478`);
9. tool failure / step interrupt → `fail_unsettled_tools` + `fail_assistant`
   (`step.rs:479-495`);
10. missing hosted results (`step.rs:497-504`);
11. durable settlement: end snapshot, file diff, cost/tokens, then
    `session.step.failed` / `session.step.ended` (`step.rs:506-542`);
12. `Continue` outcome (`step.rs:544-550`);
13. returns — **the latched interrupt re-raises here, after durable settlement**
    (`step.rs:552-557`).

From 2b onward no code awaits the shutdown watch: classification through durable
settlement runs to completion (`step.rs:397-399`).

### Coordinator-side guard regions

- The `started` hook runs uninterruptibly before the first drain
  (`execution.rs:403-408`).
- The settlement window opens when the drain loop exits: `loop_done` set, reason
  taken (`execution.rs:472-477`). An interrupt arriving in the window only *claims
  wakes* — the terminal outcome is already decided (`execution.rs:282-288`).
- The `settled` hook runs uninterruptibly; its defects **replace** the exit (waiters
  observe them) but never skip settle (`execution.rs:479-485`).
- Settle: a surviving doorbell starts the successor **before** waiters resolve
  (`execution.rs:487-507`; upstream `run-coordinator.ts:116-120`); waiters release via
  the done watch (`execution.rs:507`).

### Invariants

1. **Signal, not drop**: interrupt = shutdown-watch send + stop bell; the drain future
   is abandoned only when `UNWIND_TIMEOUT` (60 s) elapses (`execution.rs:414-427`,
   `execution.rs:51-55`).
2. **Guard regions complete**: classification through `session.step.ended/failed`
   (`step.rs:397-542`) and the coordinator's settled/settle sequence
   (`execution.rs:472-507`) contain no interrupt observation points.
3. **Interrupt re-raises after settlement**: `AttemptFailure::Interrupted` returns
   only at step 13 (`step.rs:555-557`), and the coordinator classifies the exit
   `interrupted{reason ?? "shutdown"}` (`execution.rs:544-557`).
4. **Wedge bounds**: 60 s for the drain (`execution.rs:55`), 5 s per fiber join
   (`step.rs:44`) — neither timeout bypasses durable settlement; both classify as
   interrupted.
5. **Settlement-window interrupts are wake-claims only**: they cannot attach a reason
   or change the terminal outcome (`execution.rs:282-288`).

---

## §5 — Execution mapping contract (coordinator)

`session/execution.rs` ports `run-coordinator.ts` + `execution.ts`
(`execution.rs:1`). One execution is a busy period for one key: one task drains from
the first wake until the key would stay idle (`execution.rs:5-9`). The concurrency
mapping the header cites (`execution.rs:12-19`), as implemented:

### interrupt = `Notify::notify_one` on a per-execution stop bell

The bell (`execution.rs:116-117`) is fire-and-forget: `notify_one` stores a permit,
and nobody waits out cleanup at the interrupt site (`execution.rs:301-302`). Its role
is to unblock the execution loop's biased select (stop bell vs drain completion,
`execution.rs:428-436`) so the loop stops waiting on the drain *normally* and switches
to the bounded-unwind poll of §4. Note the drain future itself is not dropped at its
next await — §4's signal-and-bounded-unwind semantics apply
(`execution.rs:414-427`); the future is abandoned only if the 60 s unwind timeout
elapses. The bell also covers wakeup paths parked outside the drain's observation
surface (`execution.rs:296-299`). `interrupt()` is idempotent-ish: an
already-stopping execution returns its done receiver without re-signalling
(`execution.rs:279-281`); the recorded wakes belong to the interrupted intent and are
claimed by the interrupt, while wakes arriving during cleanup are new admissions that
restart normally at settle (`execution.rs:290-294`).

### settlement runs uninterruptibly after the drain loop exits

Sequence per execution: leading yield (`execution.rs:395-397`;
upstream `run-coordinator.ts:94-96`) → `started` hook → drain loop with the doorbell
trampoline (a recorded wake re-drains with its widest scope, stopping ends as
interrupted; `execution.rs:410-470`) → settlement window: `settled` hook, then settle
(surviving doorbell starts the successor before waiters resolve, else the key is
removed) → waiter release (`execution.rs:472-507`).

### waiters observe the whole exit through a `watch` channel

Each execution owns `done_tx: watch::Sender<Option<Exit>>` (`execution.rs:122-123`,
created in `start_locked` at `execution.rs:348-360`). `run()` joins the active
execution — or starts one; a stopping execution refuses joiners, which wait out its
exit and loop to run fresh (`execution.rs:216-240`). `await_idle` never starts work
(`execution.rs:313-321`); `await_receiver` clones the receiver and resolves on the
first `Some(exit)` (`execution.rs:366-382`). The exit waiters see is the drain exit
**composed with settlement defects** (`execution.rs:479-485`), sent exactly once at
`execution.rs:507`.

### Doorbell

`wake()` records work for an idle key (starting an execution) or an active one (which
drains again before settling); coalesced wakes keep the widest scope — "input"
subsumes "steer" (`execution.rs:242-260`, `execution.rs:6-9`). A doorbell surviving
the loop starts a fresh execution at settle (`execution.rs:487-506`, matching
upstream `run-coordinator.ts:116-120`).

### Drain re-entry and the execution service

- `wrap_drain` maps `DrainOutcome`: `Moved` re-drains without force carrying the
  continuation; `Reloaded` re-drains with its own force flag (`execution.rs:701-746`).
- Write-ahead claim: `session.execution.started` publishes with a commit hook that
  flips the DB claim in the same transaction (`execution.rs:22-29`,
  `execution.rs:783-804`). Terminals release the claim — except shutdown
  interruption, which keeps it for restart resume (`execution.rs:980-1011`,
  `execution.rs:826-847`).
- Terminal classification: success ⇒ `succeeded`; interrupts-only ⇒
  `interrupted{reason ?? "shutdown"}`; else `failed{toSessionError}` with `surfaced`
  marking whether the failure already reached an assistant message
  (`execution.rs:544-573`). The settled hook awaits the usage fan-out chain before any
  terminal publish so SSE order is deterministic (`execution.rs:806-821`).
- `interrupt_all` is the graceful twin of an unclean death: interrupt everything,
  terminals read as shutdown (`execution.rs:323-338`, `execution.rs:694-698`).

### Invariants

1. **Per-key serialization**: at most one execution per key in the map; different
   keys run concurrently (`execution.rs:174-179`).
2. **Watch before bell**: the shutdown watch is signalled before the stop bell so the
   in-band signal is never missed by a drain already polling (`execution.rs:296-302`).
3. **Successor before waiters**: a surviving doorbell starts the next execution
   before `done_tx` resolves, so `run()`/`await_idle` loops observe continuity
   (`execution.rs:487-507`).
4. **One terminal per busy period**: exactly one `done_tx.send(Some(exit))`
   (`execution.rs:507`), covering every coalesced drain.
5. **Widest-scope coalescing**: `pending_wake` only ever widens
   (`Input ⊃ Steer`) (`execution.rs:250-254`).

---

## Appendix — v2 Effect → Rust primitive map

| v2 (Effect / upstream) | Rust counterpart | Where |
|---|---|---|
| per-tool Effect fiber (upstream `step.ts:85`, `:120`) | `tokio::spawn` task + `JoinHandle`, exits over unbounded mpsc | `step.rs:202-204`, `step.rs:305` |
| `interruptTools` = `Fiber.interruptAll` (upstream `step.ts:87`) | latching bell `watch::Sender<bool>` send + bounded join/abort | `step.rs:176-181`, `step.rs:329-372` |
| `Effect.interrupt` of the drain | shutdown watch (in-band) + stop bell + `UNWIND_TIMEOUT` bounded unwind | `execution.rs:118-123`, `execution.rs:296-302`, `execution.rs:414-427` |
| publisher writes under `Effect.uninterruptible` (upstream `publish-llm-event.ts:558`) | single-writer `&mut` + mark-before-await | `publisher.rs:4-10`, `publisher.rs:559-563` |
| finalization `uninterruptibleMask(restore)` (upstream `step.ts:135-144`) | guard region without shutdown awaits; join sub-phase keeps observing | `step.rs:318-399` |
| `deltaBatchInterval` timer fiber (upstream `publish-llm-event.ts:79`, `:166`) | due-batch check on event arrival (order-preserving) | `publisher.rs:96-99`, `publisher.rs:159-170`, `publisher.rs:361-372` |
| run-coordinator `Deferred` done (upstream `run-coordinator.ts:89`, `:116-120`) | `watch::Sender<Option<Exit>>` fan-out | `execution.rs:122-123`, `execution.rs:366-382`, `execution.rs:507` |
| `Effect.yieldNow` trampoline (upstream `run-coordinator.ts:94-96`) | `tokio::task::yield_now` (leading + before each re-drain) | `execution.rs:396-397`, `execution.rs:467` |
| `Effect.die` (publisher/defect invariants) | `PublisherError` / `RunError::Defect` | `publisher.rs:35-45`, `execution.rs:84-86` |
| stream consumption (`Stream` in v2 step) | single-writer biased select over watch/outcome/stream | `step.rs:229-234` |
