# Task 9 D2 — oc-core CLIENT conversion (PersistentPty → daemon, spawn-if-absent + one-release in-process fallback)

**Status: DONE** — lane `lane/ptyd-d2`, all gates green.

## Scope delivered (binding spec §3, §5.2, §8, §9, §11)

1. **Backend selection** (`frontend.rs`): `OC_PTY_DAEMON=0` → in-process machine
   (one-release window, R3); otherwise daemon backend — DEFAULT ON. Read once at
   startup via the lane OnceLock pattern (M1 precedent: `OC_ENGINE_OPENTUI` in
   oc-tui-core 6a8cd9d^, incl. the `#[cfg(test)]` atomic override so libtest's
   shared process can't A/B through the env).
2. **Discovery + spawn-if-absent** (`transport.rs`): §8.1 order
   (`OPENCODE_PTY_BIN` → `<exe-dir>/oc-pty` → PATH, resolve-once with
   reset-on-failure); §8.2 spawn (detached, stdio-null, own process group) +
   50 ms/5 s poll + claim; SIGTERM-on-failure through the owned child handle only
   (§4.4(b) — single positive pid we spawned; no pid from disk, no group);
   abandon → fresh-dir single retry (daemon.ts:275-292).
3. **Owner connection lifecycle**: single owner conn (own → epoch'd reader task);
   demux of exchange responses vs `bus_event` (republished onto the REAL Bus
   through the same registry definitions — TUI/EV-1 surface byte-identical) vs
   `owner_superseded` (drop to non-owner + drop cached registration; the next op
   re-runs discover+claim — the re-own ladder); per-request one-shots; the §5.2
   failure ladder (auth → drop registration → rediscover → retry once; connect →
   drop → create-only respawn+retry); subscribe streams with §3.2 chunked-replay
   reassembly by absolute offset + client-deferred `replay_complete` (attach
   returns only once contiguous); binary output frames ride standard framing.
4. **`Rows{terminal: null}` → `None`** (D1 report concern 2) — plus absent-daemon
   `read` → `None`, `list` → `[]` (soft-empty), §8.2 op errors → connect-kind
   503s.
5. **Error taxonomy** (§9): TransportError kinds
   connect/response/registration/protocol/spawn mapped onto the existing
   `UnavailableError` message shapes (the `PersistentPty.NotFoundError:` prefix
   the handlers 404-match rides verbatim); the protocol-mismatch EMITTER lives
   client-side in the §6 ladder (pinned I7 message
   `opencode-pty protocol mismatch: daemon=N, client=7`), which also prints the
   discard line "Discarding PTY restart handoff; persistent terminals will start
   fresh"; the machine's `adopt()` ladder stays as the in-process fallback arm.
6. **§6 adoption ladder** (daemon backend): arm A live-claim with the single-use
   ticket + client-side inode-preserving migration + `directory` re-anchor; arm B
   dead-daemon `state.json` restore (flock probe for the unreachable-owner
   `owned` case, migrate → spawn → owner-conn `restore` → consume `state.json`);
   every discard warns, best-effort TOKEN-level `shutdown`s a FOUND daemon,
   abandons to a fresh identity — boot stays green.
7. **Consumers keep compiling/behaving identically**: `oc-server` `AppState`
   constructs `Frontend::new(bus)`; handlers/boot surface unchanged in shape —
   `list/get/snapshot/handoff/attach/adopt` gained `.await` (the daemon path is
   inherently async; minimal oc-server touch only where signatures adapted).

## Hermetic test seam (the D2 requirement)

`transport::Launcher`/`Spawned` abstract "spawn oc-pty". The hermetic suite
(`crates/oc-core/tests/pty_client.rs`, 22 tests) starts a FAKE daemon — a
std-thread Unix-socket server speaking the real `wire` codec, writing real §5.3
records (`service.json`, `service.lock` flock, `daemon.sock`) — via the fake
launcher. No binary is built or run; the real-binary matrix is D3 (via
`OPENCODE_PTY_BIN`/BinaryLauncher, already implemented).

Covered: escape-hatch flag; discovery order (fixture dirs); soft-empty + no
spawn for non-create ops; spawn→poll→claim; abandon→fresh-dir retry; deadline →
child SIGTERM'd; protocol-mismatch message verbatim; bus_event republish onto a
real Bus (asserts `persistent-pty.added` name + payload — the TUI-visible
surface); owner_superseded → second `own`; Rows{null}→None + Rows{Some} mapping;
snapshot wire mapping; inline + chunked replay (1.5 MiB, delayed marker, exact
reassembly, deferred completion); live output after `activate` and nothing
before; arm A claim-with-ticket (+ directory in claim), bad ticket discard +
best-effort shutdown, arm B snapshot restore (migration, restore op, state.json
consumed), discard reasons (expired/no_state/lock-held); auth-failure
drop-and-rediscover recovery; handoff None-without-daemon + owner-conn handoff;
in-process fallback parity (lines validation, NotFound message shape).

Two real bugs the TDD pass caught in the client itself: (1) a frame decoder
dropped extra frames decoded within one chunk (replay + live output lost);
(2) dropping the subscribe connection's write half EOFs the daemon, and AF_UNIX
close purges the peer's pending receive queue — the client now holds the write
half for the stream's lifetime.

## Consumer/test-harness notes (R3)

- The machine's whole hermetic suite runs unchanged (1033/1033 oc-core).
- oc-server REST harnesses that create terminals pin `OC_PTY_DAEMON=0` in their
  `boot()` helpers (`parity_event_payloads.rs`, `parity_pty_frames.rs` — each is
  its own test binary, so the OnceLock latch is race-free); the boot adoption
  lib test pins the in-process frontend explicitly (no env race under
  `cargo test`). 263/263 oc-server tests green (incl. EV-1, PTY-F3, boot ladder).
- oc-tui consumes bus events only — untouched, unchanged reducer.

## Gates

- `cargo nextest run -p oc-core --locked` — 1033/1033 PASS.
- `cargo nextest run -p oc-server --locked` — 263/263 PASS (check also clean).
- `cargo clippy --workspace --locked --all-targets` — 0 warnings.
- `cargo fmt --all --check` — clean.

## Concerns / handoffs

- **D3**: real-binary process tests (I1-I15) via `OPENCODE_PTY_BIN`; the
  BinaryLauncher + spawn deadline are in place. The 5 s poll/claim deadline is
  overridable (`Transport::with_spawn_deadline`) for slow CI.
- **Release packaging**: discovery step 2 requires `oc-pty` shipped beside the
  main binary (§11.1) — packaging change belongs to the release lane.
- **Default ON** means production `create` spawns the daemon on first use; if
  the binary is missing, create surfaces a clean 503 (`spawn` kind), never a
  hang (§11.4 compat guard honored: unknown ops degrade over the wire error
  shape).
- The daemon-side D1 latent hazard (its `exited`-then-close could race a slow
  subscriber, same AF_UNIX queue-purge class the client hit) is noted for D3 —
  the client-side fix (holding the write half) covers our subscribe conns.
