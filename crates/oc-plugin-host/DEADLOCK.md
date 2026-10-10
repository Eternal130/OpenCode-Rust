# The plugin-host deadlock protocol (binding)

Discovered for real in the spike (`spike-plugin-runtime.md`, risk
table): the supervisor held the stdin write lock while blocking on a
`load` reply that required answering the guest's nested `config.get`
request first → 30 s hang. This document is the binding spec; the
implementation (`src/supervisor.rs`, `src/host/host.js`) enforces it
structurally, and `tests/storm.rs` hammers it.

## Rules (supervisor side)

1. **The IO reader task never blocks on anything but the pipe.**
   Guest→host requests (`command.add`, `theme.register`,
   `event.subscribe`, `ping`) are answered *on the reader task*. The
   handler is fully synchronous — a registration insert or a log line —
   and the reply frame is *enqueued* onto the writer's channel. No lock
   on any write path is held across an await, ever.

2. **Nobody holds the write channel while awaiting a reply.**
   `Shared::request` builds the frame, moves it into the writer-owned
   unbounded channel (a `send`, not a blocking write), and only then
   awaits a oneshot keyed by the request id. A hundred concurrent
   requests therefore never serialize behind each other's pending
   replies.

3. **Every bridge call is timeout-bounded.** Defaults: 2 s
   registration (`initialize`/`load`/`unload`), 5 s event dispatch and
   command execution. On timeout the pending entry is removed; a late
   reply arrives to an unknown id and is dropped. A hung plugin costs
   exactly its own dispatch (logged, counted in `dropped_events`),
   never the stream: the dispatch pump continues to the next event.

4. **Generation discipline.** Each (re)spawn rotates the outbound
   channel, bumps a generation counter, and fails every still-pending
   request — a caller that raced a crash gets an immediate
   `Exited` (with a short retry window in the dispatch pump) instead
   of waiting out its timeout. Late replies from dead generations are
   discarded by id.

## Rules (guest side — the mirror)

5. **The guest read loop never awaits a handler before reading the
   next line.** `handleLine(...)` runs detached (`Promise` + catch):
   while a `load`/`dispatch` handler is busy — possibly blocked on a
   nested guest→host request — the loop keeps draining stdin, so the
   supervisor's replies to those nested requests are processed and the
   handlers make progress. Awaiting handlers inline would deadlock the
   pair the moment one side blocks on the other.

6. **Console containment.** Plugin `console.*` output is rerouted to
   `log` notifications; nothing but protocol frames is ever written to
   stdout (one JSON value per `\n`-terminated line, both directions).

## The reentrancy that must work (and is tested)

- `setup(ctx)` issues N nested `command.add` **while the supervisor's
  `load` reply is still outstanding** (registration bridge).
- An event handler issues a nested `ping` **while the `dispatch` reply
  is outstanding**.
- Command execution nests five more sequential `ping`s, concurrently
  with a 200-event tap storm from multiple publisher threads and a
  hot-reload digest scan.

`tests/storm.rs` asserts: all 200 events delivered in order, zero
dropped, every execution resolved, the whole storm bounded (< 30 s),
host responsive afterwards.
