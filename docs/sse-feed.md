# SSE event feed — design note for Phase 5 wave 1

Status: **design note written before route code** (oracle gate 5, top-3
unwritten risk (a)). Every semantic below grounds in the v2 sources
`packages/server/src/event-feed.ts` + `packages/server/src/handlers/event.ts`
and the oc-core bus primitives (`bus.rs` `subscribe`/`subscribe_types`/
`listen`, `bus::registry::by_type_latest`); core-deep-map §2f collects the
bus invariants this rides on.

## 1. Shape (v2, verbatim)

One process-wide feed over the engine's bus. `GET /event` (the
`server.event` group) responds `text/event-stream` with:

- **the initial `server.connected` frame** — synthesized by the handler,
  NOT published on the bus: `{ id: <event id>, type: "server.connected",
  data: {} }`, `data: {json}\n\n` (event-feed.ts `frame`;
  handlers/event.ts prepends it via `Stream.make(frame(connected))`).
- **live frames** — every bus event that passes the visibility filter,
  each pre-rendered once as `data: {json}\n\n`.
- **heartbeats** — `: heartbeat\n\n` every **15 s**, comment frames that
  keep intermediaries from timing the connection out; merged with the
  live stream (`haltStrategy: "left"`: the live stream ending ends the
  response).
- **response headers** (exact set): `content-type: text/event-stream`,
  `cache-control: no-cache, no-transform`, `x-accel-buffering: no`
  (nginx: do not buffer), `x-content-type-options: nosniff`.

## 2. Per-subscriber queue semantics (the load-bearing part)

v2 (event-feed.ts:30-63): each subscriber owns a **dropping queue with
capacity 4096** (`SubscriberCapacity = 4_096`).

- The feed's publish is O(1) per subscriber: `Queue.offerUnsafe`. A full
  queue does NOT block, buffer, or backpressure — the subscriber is
  **removed from the set and failed with
  `EventFeed.SubscriberOverflowError { capacity }`** (drop-and-disconnect).
  Rationale: a slow SSE consumer must never stall the durable bus or
  punish fast consumers; the client's contract on overflow is
  **reconnect** (see §6).
- `subscribers.size === 0` short-circuits publish — zero overhead with no
  listeners.
- **Encode once per publish**: `render(event)` runs a single time per
  event for ALL subscribers (never per-subscriber JSON). An encoding
  failure is fatal for the feed (v2 `EncodingError`): every subscriber is
  failed and the set cleared — a wire-shape bug should not be papered
  over per-connection.

## 3. Visibility filtering (what reaches the wire)

`isOpenCodeEvent` (protocol groups/event.ts:67):

```
server.connected  ||  EventManifest.isServer(event)  ||  type.startsWith("rpc.")
```

- `EventManifest.isServer` = the event definitions the server exposes —
  in the Rust port this is **`bus::registry::by_type_latest(type_)`**: a
  registered definition exists at the type's LATEST version, and the
  server-visible set is the registry's server group (the parity lane's
  session/config/permission/location groups). Latest-version filtering
  matters because the registry carries versioned duplicates
  (`session.message.1` vs `.2`): the wire only ever carries the frame the
  CURRENT schema produces; `Bus::publish` already routes each publish to
  its definition's versioned type, so the filter is
  `is_server(type_) && !is_legacy_version(type_)`.
- **Per-session / per-location filtering happens at the route layer, not
  in the feed**: the feed broadcasts one stream; `handlers/event.ts` in
  v2 exposes no query filtering today, and the SSE handlers that DO
  filter (session-scoped tails) subscribe with
  `bus.subscribe_types([...])` + `Bus::log({ aggregate_id })` for the
  durable prefix. The oc-core mapping is already in place:
  `Engine::subscribe(&["session.step.ended", ...])` returns a
  type-filtered live subscription; per-session filtering composes it with
  `engine.bus.log(LogInput { aggregate_id: session_id, follow: true })`
  (replay-then-live, exactly-once at the watermark — bus lane contract).

## 4. tokio broadcast vs per-subscriber mpsc (decision)

**Decision: per-subscriber bounded `tokio::sync::mpsc::Receiver`
(capacity 4096, `try_send` semantics) behind the feed — NOT
`tokio::sync::broadcast`.**

Rationale:

1. **Overflow must terminate the subscriber, not lag it.** `broadcast`
   mutes a slow receiver with `RecvError::Lagged(n)` and keeps it alive —
   exactly the semantics v2 rejects. A dropping-mpsc + `try_send` failure
   maps 1:1 onto v2's "remove from set + fail with
   SubscriberOverflowError": the route translates the closed receiver into
   connection close, which is the observable v2 behavior
   (drop-and-disconnect).
2. **Encode-once-per-publish.** broadcast receivers decode a shared
   `Arc<Event>` per receiver; v2 shares the encoded STRING. The mpsc
   carries the pre-rendered `String` frame — one allocation path, one
   serialization per event.
3. **The bus already owns the fan-out primitive.** oc-core's
   `Bus::subscribe()` is an mpsc-per-subscriber over the bus's live lock;
   the feed is a thin adapter (filter → encode → `try_send`) over
   `bus.listen()` — the same shape as v2's `make(bus.listen)`. Introducing
   broadcast would add a second fan-out layer with different overflow
   semantics for no gain.
4. Cost: O(subscribers) `try_send` per publish under the publish path —
   identical to v2's `for (const subscriber of subscribers)`. With no
   subscribers the listen-guard filter short-circuits (mirror v2's
   `subscribers.size === 0`).

## 5. Rust mapping (wave 1 implementation sketch)

```
EventFeed::new(bus: &Bus, engine: &Engine)
  - bus.listen(move |event| feed.publish(event))            // one listener
  - publish: is_server(event.type_)? -> frame = render(event)
             for each subscriber: tx.try_send(frame) or remove + close
  - subscribe() -> (mpsc::Receiver<String>, JoinHandle)      // route task:
        initial server.connected frame, then frames, heartbeat select:
        loop { select! { frame = rx.recv() => write, _ = interval(15s) => ": heartbeat\n\n" } }
```

- The route task owns flushing; SSE framing is newline-terminated
  (`\n\n`), so writes are self-delimiting.
- Client disconnect closes the receiver's sender handle → the route task
  ends → the subscriber is removed from the feed set (the release half of
  v2's acquireRelease).

## 6. Reconnect expectations

- Overflow or transport death ends the stream; clients reconnect and
  **replay the durable prefix** they missed: `GET /event` is the live
  tail only — durable catch-up goes through the per-aggregate log
  endpoints (`Bus::log` semantics: `after: <last seq>` gives gapless
  replay in commit order; seq is the client's cursor).
- The initial `server.connected` frame carries no cursor — clients start
  cursors from the first observed durable event's `seq`.
- 15 s without frames is normal (heartbeat); a missed heartbeat window
  (2×) is the reconnect trigger for clients that also watch liveness.
