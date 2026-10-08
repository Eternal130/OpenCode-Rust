# Runner concurrency — fiber→tokio mapping (Wave B contract)

Status: **design note, written before any runner code** (oracle gate 4,
finding 3). Every section grounds in the v2 line references collected in
`core-runner-map.md` (§1.4 step finalization, §1.8 execution ownership, §8
Effect→Rust notes). This is the contract Wave B implements against; it is
descriptive of v2 semantics, not speculative.

v2 runs the whole session pipeline on Effect fibers. Fibers differ from
tokio tasks in three load-bearing ways:

1. **Structured interruption**: `Effect.interrupt` delivers an interrupt
   *at await points*, and `uninterruptibleMask` *disables* that delivery
   for a region — a fiber can decide "nothing may cut me between these two
   awaits". Tokio cancellation is *drop-the-future*: it strikes between
   awaits with no way to say "not here".
2. **Deferred/Deferred-safe resumption**: v2 resolves `Deferred`s from
   other fibers and treats that resolution as synchronous with the code
   around it. A oneshot `Sender::send` is the same *only if* the code that
   sends has already finished its awaited side effects (see §3 ordering).
3. **Single-threaded runtime**: v2's fibers run on one JS thread, so
   "publish before resolving" is trivially atomic. Tokio multithreaded
   execution makes publish-then-resolve a *ordering requirement on awaits*,
   not a byproduct.

The mapping below preserves v2's semantics under those differences.

---

## 1. Publisher: single-writer state machine (commit marks before first yield)

**v2 invariant** (`publish-llm-event.ts:66-77`, core-runner-map §8 note 1):
the `LLMEventPublisher` mutates its record (output started, tool call
settled, step started, …) **synchronously inside the stream callback** —
before any yield — so later fragments observe the mark. Two fragments
arriving "simultaneously" are serialized by the single-threaded runtime in
arrival order, and every mark is visible to the code handling the next
fragment.

**Rust mapping**: the publisher is NOT a shared struct with `Mutex`es
around individual fields — it is a **single-owner state machine** living in
the step task, driven by a stream loop:

```rust
enum Record { output_started: bool, tools: HashMap<CallId, ToolRecord>, finish: Option<Finish>, … }
let mut record = Record::default();
while let Some(fragment) = stream.next().await {   // the ONLY await in the loop body
    record.apply(&fragment);                       // sync — marks before any yield
    …                                              // publish side effects (§1.1)
}
```

- **Check-then-mark races are impossible by construction**: the only code
  touching `record` is the loop, which never awaits between reading and
  writing it.
- Tool fibers (§2) do NOT write the record; they send their outcomes over
  a channel the loop drains, so settlement marks happen in the loop's
  single-threaded order.
- If a future wave needs concurrent publishers on one session, the answer
  is an mpsc channel into one writer task — still single-writer — never a
  `Mutex<Record>` shared across tasks (check-then-mark on
  `tool.settled`/`stepStarted` under a mutex is exactly the race the v2
  design forbids: two tasks could both see "not settled" and both publish).
- **Effect.die / defects**: v2 defects (duplicate ids, invariant
  violations) map to `panic!`-free typed errors where callers can observe
  them and to task aborts where v2 would kill the fiber. A defecting writer
  task takes the step down via its `JoinHandle` result — never a shared
  poison flag.

### 1.1 Publish ordering inside the loop

Each durable publish (tool events, step boundaries) goes through
`Bus::publish`, which is await-based; the loop publishes **in record
order** and must not interleave two publishes for one aggregate
concurrently — the bus already serializes per-aggregate with its own lock
(`bus.rs` `lock_for`), but the *sequence numbers* must reflect loop order,
so the loop awaits each publish before applying the next fragment that
supersedes it. Streaming deltas (ephemeral, no aggregate) may be spawned
ahead — they carry no ordering contract with the record.

## 2. Tool fibers → spawned tasks + settlement channel (§1.4 "Fibers")

**v2**: per tool-call event (not per `providerExecuted`) v2 **forks**
`executeTool → truncate → publisher.toolExecution`; `Tool.Error ⇒
failTool`; the step joins every fiber at finalization. Fibers are
interruptible individually (`interruptTools`).

**Rust mapping**:

- one `tokio::spawn` per tool call, each returning its outcome on an
  `mpsc::channel::<ToolOutcome>` (bounded, one slot per fiber);
- the spawned task's contract: run decode → execute → encode pipeline,
  then push the outcome and **finish publishing** before pushing (§3);
- interruption: the step keeps a `JoinHandle` + a per-tool
  `CancellationToken`-equivalent (a `tokio::sync::watch` shutdown channel
  is enough; no new deps). `interrupt_tools()` fires the channel and
  **awaits the handles** — v2's "interrupt tool fibers; await" maps to
  fire-then-join, not fire-and-forget;
- exits classify exactly like §1.4 step 2: declines =
  `PermissionError::Declined` | question-tool cancellation; interrupted =
  the shutdown channel fired; failure = die-reasons-only. Interrupted
  fibers report `TOOLS_INTERRUPTED {type:"aborted", message:"Tool execution
  interrupted"}` through the same channel so the classification stays in
  one place.

**Uninterruptible final tool events**: the terminal `session.tool.success`
/ `session.tool.failed` publication for an *already-finished* tool must not
be cut by cancellation — see §4's cancellation guard; the guard wraps
"push outcome + publish", so a tool that finished its work always lands its
terminal event even when the step is being interrupted.

## 3. Awaited-ordering rule: publish BEFORE resolving (permission precedent)

The pattern established in Wave 0 for permissions (oracle gate 4, finding 2;
v2 `permission.ts:270-306`) is the general rule for Wave B:

> wherever v2 does `bus.publish(...)` and *then* resolves a `Deferred`, the
> Rust port **awaits the publish and only then sends the oneshot**.

Concretely, for tools and steps:

- a tool fiber publishes its terminal event (await) before signaling its
  outcome to the step loop;
- the step loop publishes `session.step.ended`/`session.step.failed`
  (await) before resolving whatever wakes the execution coordinator
  (§5 doorbell);
- `Effect.die`-style failures that v2 tunnels to the parent (declines)
  resolve the oneshot with the typed error — after the publish — so the
  interrupting party can never observe the step "done" before the event
  that explains it is on the bus. This is what keeps
  `session-runner-recorded`-style replays deterministic.

## 4. `uninterruptibleMask` → cancellation-guard regions

**v2**: finalization (§1.4 "Finalization (uninterruptibleMask)") and
settlement run uninterruptibly: once the stream exits, nothing — not
`interrupt`, not shutdown — may cut between "classify tool exits" and
"publish step.ended / fail assistant".

**Rust mapping**: a cancellation-guard region. Two mechanisms combined:

1. **`scopeguard`-style drop completion is NOT sufficient alone** (drop
   runs only what is undoable; publishes must *complete*, not roll back),
   so the region is expressed as a `tokio::select!` shutdown gate around
   the region's future:

```rust
// inside the step task:
async fn uninterruptible<F: Future>(region: F, shutdown: &mut ShutdownRx) -> F::Output {
    tokio::select! {
        biased;
        _ = shutdown.wait() => {
            // finish the region to completion, ignoring further interrupts
            region.await
        }
        output = region => output,
    }
}
```

   With `biased` + the shutdown arm *continuing into* `region.await`, the
   region always runs to completion once entered; interrupts received
   during the region are remembered (the gate stays latched) and take
   effect at the next interruptible await — exactly `uninterruptibleMask`
   semantics (v2 re-delivers the interrupt at the mask's exit).

2. **`scopeguard`/Drop is used for the *effect* side**: marking the record
   / releasing coordinator claims on early drop, so a panic or runtime
   shutdown still restores invariants.

What the guard protects, per §1.4 (finalization order, 1-12): exit stream →
`hasStarted ⇒ streamed()` → interrupt tool fibers + await + classify →
overflow check → unknownFinish mapping → RecoverFull decision → retry
decision → failAssistant / declines mapping (`location shut down` /
Cancelled reason / generic decline message) → `failUnsettledTools` /
`failAssistant(STEP_INTERRUPTED)` → missing hosted results → **durable
settlement** (snapshot, files diff, usage cost, `record.failure ⇒
publishStepFailure` else `Step.Ended{…}`) → Continue outcome → returns.
The whole block is one guard region; the interrupt that arrived mid-stream
is observed *before* the region (step 2 classification) and re-raised
*after* it (step "returns: interrupted ⇒ rethrow").

**Interruptors** (§2.3 interrupt chain): API interrupt → coordinator →
runner fiber → provider stream → tool fibers is a chain of await-points.
Each hop is a normal (interruptible) cancellation except the two
uninterruptible regions: finalization (above) and
`SessionExecution` terminal settlement (§5). `jobs.cancel(sessionID)`
follows the coordinator, not the fiber, so background jobs die with the
session claim.

## 5. Coordinator: doorbell + state enum (§1.8)

**v2** (`run-coordinator.ts`): per-key `Execution{done, owner, scope,
pendingWake, stopping, interruptionReason}`; `wake(id, reason)` coalesces
("input" subsumes "steer"); the drain loop re-drains while `pendingWake`
(yieldNow trampoline); settle starts a successor if the doorbell rang after
the loop; `run` joins the existing execution or starts one (force=true,
"input"); stopping executions refuse joiners; `interrupt` clears
`pendingWake`, marks stopping, fire-and-forget `interrupt(owner)`.

**Rust mapping** (per §1.8 note + §8 note 2):

```rust
struct ExecutionState {
    done: bool,
    stopping: bool,
    interruption_reason: Option<String>,
    pending_wake: Option<WakeReason>,          // None | Input | Steer …, coalescing in WakeReason::subsumes
    handle: Option<JoinHandle<()>>,
}
// per session: Mutex<HashMap<ExecutionId, ExecutionState>> + Notify (the doorbell)
```

- **doorbell** = `tokio::sync::Notify` + `pending_wake` under the mutex.
  `wake()` locks, coalesces (`Input` subsumes `Steer`; equal or lesser
  reasons don't downgrade), stores, `notify_one()`. The drain task holds
  the loop: drain until quiescent, then
  `notify_notified()` racing nothing — if `pending_wake` was set while the
  loop was draining (checked under the mutex after the last item — the v2
  "yieldNow trampoline"), loop again. This is the standard
  state+`Notify` pattern; a `watch` channel would drop the coalescing
  distinctions.
- **settle starts a successor**: when the drain task finishes, it checks
  `pending_wake` one last time under the mutex; if set, it *itself* starts
  the successor before marking `done` — no window where a wake lands on a
  finished execution.
- **`run` joins or starts**: lock, find non-`done` non-`stopping`
  execution for the key, clone its handle, unlock, await; else start a new
  drain task (force=true, reason "input"). `stopping` executions refuse
  joiners (fall through to the coordinator's interrupt path).
- **interrupt**: lock; clear `pending_wake`; set `stopping` +
  `interruption_reason`; take the owner handle; unlock; **fire-and-forget**
  the owner's cancellation (v2: `Effect.fork` — the interruptor does not
  wait here; the *settlement* path is where awaiting happens, §6).

## 6. `SessionExecution` settlement + write-ahead claim coupling

**v2** (`execution.ts`, §1.8): `Execution.Started` publishes with
`commit: store.claim(sessionID)` — the claim lands in the SAME transaction
as the started event. Terminals release the claim **except** shutdown
interruption (keeps the claim so restart resumes). Terminal selection:
success ⇒ `succeeded`; interrupts-only ⇒ `interrupted{reason ?? "shutdown"}`;
else `failed{toSessionError}`. Non-shutdown interrupt cancels session
jobs. After coordinator interrupt, promotable steer/control wakes with
"steer".

**Rust mapping**:

- claim = DB flag written via the bus commit hook —
  `PublishOptions { commit: Box::new(move |tx, _| tx.execute("UPDATE session_v2 SET …claim…")) }`
  on the `session.execution.started` publish. The commit hook runs inside
  the durable-commit transaction (bus lane contract), so event + claim are
  atomic — no separate `BEGIN`, no window.
- terminal publishes use the same coupling in reverse: `succeeded` /
  `failed` commit-hooks release the claim; `interrupted` with reason
  `"shutdown"` (or no reason) does **not**.
- settlement itself is a §4 guard region: classify (success |
  interrupts-only | failure), pick the terminal event, publish with the
  claim-coupled commit hook, *then* resolve the coordinator's doorbell
  state to `done`. Job cancellation (`jobs.cancel`) fires between the
  terminal publish and `done` for non-shutdown interrupts, mirroring
  execution.ts ordering.
- restart resume reads the claim the same way v2 does: a claimed session
  with no live execution is resumed by the next drain/activation.

## 7. Cancellation inventory (what interrupts what)

| v2 construct | site | Rust mapping |
|---|---|---|
| fiber fork (tool) | step.ts per tool-call event | `tokio::spawn` + outcome channel + shutdown watch |
| `interruptTools` | finalization step 2 | fire watch + `JoinHandle` await (join, not forget) |
| `uninterruptibleMask` | finalization, settlement | §4 guard region (biased select, latch, re-raise after) |
| `Effect.interrupt` delivery | anywhere | future drop at await points; guarded regions defer it |
| `awaitSettlement` | TTL sweeper / lifecycle | `SessionInterruptor::interrupt` awaits coordinator settle (Wave 0 seam: `location_services::spawn_activity_sweeper` already awaits it before invalidating) |
| coordinator `wake` doorbell | run-coordinator.ts | `Notify` + `pending_wake` state enum, coalescing (`input` ⊃ `steer`) |
| `yieldNow` trampoline | drain loop | mutex re-check after drain quiescence (successor start) |
| fire-and-forget interrupt | run-coordinator interrupt | `tokio::spawn(cancellation)` — settlement awaits later |
| `Deferred.succeed/fail` | everywhere | oneshot send, **after** the awaited publish that precedes it in v2 |

## 8. Non-goals / later waves

- PTY and subagent interruption chains (Wave D) reuse §2's
  fire-then-join and §4's guard; no new primitives.
- The projector's exactly-once interplay with these publishes (fork
  500-batching, replay guards) is Wave A scope; this note only fixes the
  *ordering* contract the projector can rely on: terminal events are
  published exactly once, from inside a guard region, before any waiter is
  released.
- Compaction/overflow recovery (§1.3) uses the same publisher loop; its
  marks (`recoverOverflow`, `recoverContinuation`) are loop-local state,
  not shared.
