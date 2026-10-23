# Lane PTYD — the PTY daemon process boundary (Phase 9) — design

Task 9 of the 2026-10-23 v2 gap burndown. Status: **design for gate review** (no code).

Binding inputs: the controller rulings R1–R7 (reproduced in §2.1), the v2 reference
(`daemon.ts`, `persistent-pty/index.ts`, `pty-handoff.ts`, `handlers/persistent-pty.ts`),
and our port (`oc-core/src/persistent_pty.rs` + `oc-server` wiring). Citations use
`ref:` = `/opt/opencode-ref/packages/...` and `ours:` = `/opt/opencode-rust/crates/...`.

---

## 1. Goals / non-goals

### Goals

1. **Move PTY masters + children out of the server process** into a `oc-pty` daemon so
   server death leaves terminals alive and daemon death kills children — the v2
   keepalive contract already documented in our module doc
   (ours: `oc-core/src/persistent_pty.rs:44-56`; ref: `daemon.ts:401-403` "A replacement
   must own its inherited daemon before the server becomes ready").
2. **One state machine** (R1): the existing `PersistentPty` machine is refactored into a
   library the daemon hosts; the daemon is a thin process wrapper (Unix-socket server +
   framing + auth + registration). No second implementation of terminals/buffers/
   roles/generations/replay.
3. **Real ownership transfer** (R4): with a live previous daemon, the adoption ladder's
   `Owned` state becomes the socket-mediated `{op:"own"}` claim
   (ref: `daemon.ts:446-481`); the `state.json` snapshot path remains for
   crash/dead-daemon cases only.
4. **A real checkpoint serializer** (R6): the "documented-format stub"
   (ours: `persistent_pty.rs:166-168`, `:792`, `:1218`) becomes a versioned byte format
   shared by the wire (`snapshot` / `resized` responses) and future emulator state.
5. **Socket security** (R7): filesystem socket in a 0700 runtime dir, per-frame token
   auth, stale-socket detection via connect-probe + `/proc` pid liveness with the
   `boot_id`+`starttime` generation guard (ours: `oc-server/src/safekill.rs:13-36`).
   No bare-PID kills anywhere.
6. **Default ON with a one-release in-process escape hatch** (R3): `OC_PTY_DAEMON=0`
   forces the existing in-process path for exactly one release window (the repo's
   M-retire pattern, ROADMAP.md:219 precedent); existing hermetic tests keep running
   unchanged against the machine.

### Non-goals

- No change to the HTTP/WS surface (`/api/experimental/persistent-pty/*`, the WS attach
  protocol) — handlers keep their exact request/response shapes
  (ours: `oc-server/src/handlers/persistent_pty.rs:27-43`, `:260-310`).
- No control-sequence-aware terminal emulator (the plain-text screen tracker stays;
  the checkpoint format is versioned so it can carry richer state later —
  ours: `persistent_pty.rs:1160-1185`).
- No abstract-namespace sockets (the ref uses a filesystem path in the registration:
  ref: `daemon.ts:447` `net.createConnection({ path: registration.socket })`,
  `:33-39`).
- No cross-user security model: the 0700 dir + 0600 files mean same-user-only, same
  as v2 (ref: `daemon.ts:438` `mode: 0o700`; ours: `persistent_pty.rs:1394-1403`).
- No Windows support (Unix sockets, `flock`, `/proc` — same platform scope as
  safekill, ours: `safekill.rs:37-47`).

---

## 2. Architecture

### 2.1 Controller rulings (binding)

| # | Ruling |
|---|--------|
| R1 | Single implementation: the in-process `PersistentPty` state machine is THE implementation, refactored into a library the daemon hosts. The daemon binary is a thin wrapper: socket + framing + auth + registration. |
| R2 | Binary `oc-pty`, published alongside the main binary. Discovery: `OPENCODE_PTY_BIN` → binary next to `current_exe` → PATH. Protocol version mirrors `daemon.ts` (7). |
| R3 | Default ON: persistent-terminal ops route to the daemon (spawn-if-absent). `OC_PTY_DAEMON=0` forces in-process for ONE release window, then retirement. Hermetic tests unchanged. |
| R4 | Ownership transfer via socket (`own` op, token-authenticated); `state.json` stays for crash/dead-daemon. |
| R5 | Child keepalive is the point: masters + children live in the daemon; server death leaves terminals alive; daemon death kills children. |
| R6 | Checkpoint stub becomes the real daemon↔client wire format (versioned, specified here). |
| R7 | Filesystem socket (follow ref), per-frame token auth, socket permissions, stale-socket detection (connect-probe + pid liveness with boot_id+starttime guard). No bare-PID kills. |

### 2.2 Process / crate layout

```
┌─────────────────────────────┐         ┌──────────────────────────────────┐
│ oc-server (per server boot) │         │ oc-pty daemon (bin crate)        │
│                             │         │                                  │
│  handlers/persistent_pty.rs │  UDS    │  main: daemon --runtime-dir ROOT │
│   │ (unchanged HTTP/WS)     │  frames │        --name ID                 │
│   ▼                         │ ──────▶ │   ┌────────────────────────────┐ │
│  PersistentPtyFrontend      │         │   │ PersistentPtyMachine       │ │
│   enum Backend {            │         │   │  (oc-core library, THE     │ │
│     InProcess(Machine)   ← R3 escape  │   │   implementation, ours:    │ │
│     Daemon(Transport),  ← default     │   │   persistent_pty.rs)       │ │
│   }                         │         │  └────────────────────────────┘ │
│  boot.rs: adopt_inherited_  │         │  socket server + framing + auth │
│         pty runs the ladder │         │  registration/lock lifetime     │
└─────────────────────────────┘         └──────────────────────────────────┘
```

- **`oc-core::persistent_pty`** stays the machine; the refactor (a) makes the runtime
  scope/registration usable by a host process, (b) extracts the restore loop of
  `try_adopt` (ours: `persistent_pty.rs:1096-1133`) into a public
  `Machine::restore(state: HandoffState) -> usize` reused by both the in-process
  adoption arm and a new daemon op (see §6 arm B). No behavior change; the hermetic
  tests (ours: `persistent_pty.rs:1475-1853`) run unchanged (R3).
- **New bin crate `crates/oc-pty`** (workspace member, `[[bin]] oc-pty`), a thin host:
  parse args (mirror ref: `daemon.ts:235` `["daemon", "--runtime-dir", root,
  "--name", id]`), claim the runtime dir (ours: `persistent_pty.rs:1340-1361`),
  bind `<dir>/daemon.sock`, write `service.json` + `service.boot`, serve frames.
- **New transport module** in `oc-core` (`persistent_pty::transport`), a port of
  `makeDaemonTransport` (ref: `daemon.ts:158-431`): discover/ping/claim/oneShot/
  subscribe/spawn/retry ladder.
- **Frontend** in `oc-server` (replaces direct machine calls at
  ours: `handlers/pty_state.rs:16-24`): same method surface the handlers use today
  (`list/get/create/write/resize/control/input/snapshot/read/remove/shutdown/
  handoff/attach`), backed by `InProcess` or `Daemon`.

### 2.3 What lives where (R1/R5 consequences)

| State | Lives in | Today | Citation |
|---|---|---|---|
| PTY masters, children, killers | daemon process | server process | ours: `persistent_pty.rs:378-394`; ref keepalive: ours module doc `:44-56` |
| Replay buffers, roles, generations, `current[sessionID]` | the machine (hosted by daemon) | the machine (in server) | ours: `:378-452` |
| `service.lock` (flock) + `service.json` | daemon process | server process | ours: `:421-436`, `:1340-1382`; ref: `daemon.ts:165-166` |
| `state.json` handoff snapshot | runtime dir; written by daemon's `prepare_handoff`, consumed by client ladder | same (machine writes it) | ours: `:195-206`, `:891-929`; ref counterpart is the live daemon itself |
| Handoff ticket | minted by daemon (`prepare_handoff`), validated on `own` | minted in `handoff()` | ref: `daemon.ts:356-380`, `:471-473` |

---

## 3. Message set — every op mapped to the machine API + wire types

All requests ride the `{token, request}` envelope (ref: `daemon.ts:455`, `:485`,
`:564-575`). JSON field names are v2's snake_case wire names. `WireTerminal` is
ref: `daemon.ts:17-30`; our `Info` (ours: `persistent_pty.rs:114-129`) maps through
the inverse of `toInfo` (ref: `index.ts:390-408`): `id` = the numeric suffix of
`pty_persistent_<n>` (ref: `index.ts:410-418`; ours id scheme `:553-557`),
`group_id` = `session_id`, `command` = `[command, args...]`, `output_head/output_tail`
= `output.0/1`, `lifecycle` = `{status:"running"}` | `{status:"exited", exit_code}`
(ref: `daemon.ts:11-15`; `failed` is reserved — spawn failures return `error`).

| Wire request (op) | Machine API (ours) | Wire response (ref type) | Spawn-if-absent |
|---|---|---|---|
| `{op:"ping"}` | (host identity: registration fields) | `pong{instance_id,pid,protocol}` (`daemon.ts:44-48`) | no |
| `{op:"own", instance_id, ticket?}` | ownership claim: validate ticket (single-use), reset `current[]` (new selection epoch), mark owner connection | `owned` (`:52`) | no |
| `{op:"prepare_handoff"}` | `Machine::handoff()` — mints ticket, writes `state.json` (ours: `:891-929`); ticket registered for one `own` | `handoff{ticket,expires_at}` (`:53`) | no (owner conn only, ref: `:356-360`) |
| `{op:"shutdown"}` | `Machine::shutdown()` (ours: `:875-883`, incl. `state.json` invalidation `:1039-1045`) then daemon exits | `ok`, then socket closes; client waits for discover to fail ≤5s (ref: `:340-354`) | no (requestIfRunning, ref: `:341`) |
| `{op:"list"}` | `Machine::list(None)` (ours: `:567-576`) | `terminals{terminals[]}` (`:50`) | no — absent daemon ⇒ `[]` (ref: `index.ts:143-145`) |
| `{op:"create", program, args, cwd, title, group_id, env, cols, rows}` | `Machine::create(group_id, CreateInput)` (ours: `:591-717`; defaults `program`←shell seam `:593-596`, `cwd:"/"` `:597`, `80x24` `:598`) | `created{terminal}` (`:49`) | **yes** — the only start=true op (ref: `index.ts:161-175`) |
| `{op:"write", id, attachment_id, data_base64}` | `Machine::write` (ours: `:721-730`) | `ok` (`:51`) | no |
| `{op:"resize", id, attachment_id, cols, rows}` | `Machine::resize` (ours: `:735-746`) | `ok` | no |
| `{op:"control", id, attachment_id, cols, rows}` | `Machine::control` (ours: `:750-761`) | `ok` | no |
| `{op:"input", id, attachment_id, cols, rows, data_base64}` | `Machine::input` (ours: `:765-778`) | `ok` | no |
| `{op:"snapshot", id}` | `Machine::snapshot` (ours: `:783-795`; checkpoint = §7 format) | `snapshot{terminal,text,checkpoint_base64,cursor_x,cursor_y}` (`:54-61`) | no |
| `{op:"read_rows", id, rows?}` | resolve `session = get(id).session_id` then `Machine::read(session, rows)` (ours: `:797-854`; the machine owns `current[]` selection + 1..=65535 validation `:806-812`) | `rows{terminal,lines[],cursor_x,cursor_y}` (`:62-68`; daemon picks live height when `rows` absent — ref: `index.ts:275-276`) | no — absent ⇒ `undefined`→`null` result (ref: `index.ts:143-145` analogue) |
| `{op:"read", session_id, rows?}` **(extension)** | `Machine::read(session_id, rows)` directly | same `rows` shape | no |
| `{op:"terminate", id}` | `Machine::remove` (ours: `:857-869`) | `ok` | no |
| `{op:"subscribe", id, offset, attachment_id, role, takeover}` | `Machine::attach` (ours: `:956-1014`) — long-lived connection; first frame `attached`, then stream frames below | `attached{terminal,role,generation,requested_offset,available_offset,end_offset,truncated,replay…}` (`:69-79`) | no (ref: `:387` `connect(false)`) |

`read` (session-keyed) is a deliberate extension over v2's `read_rows` (id-keyed):
v2 keeps the `current[sessionID]` selection map client-side
(ref: `index.ts:139-140`, set at `:209`, `:228`, `:249`, `:340`); our machine owns it
(ours: `:440-441`, `:1001-1003`) and R1 forbids a second copy. Keeping the map
daemon-side preserves one selection implementation for both backends; `own` resets it
(§6 arm A). `read_rows` stays for wire parity, routed through the same machine call.

**Stream frames on a subscribe connection** (after `attached`):

| Frame | Encoding | Source |
|---|---|---|
| output | binary `[0x00][start u64BE][end u64BE][data…]` (≥17 bytes; ref: `daemon.ts:589-597`) | `StreamEvent::Output` (ours: `:351-353`); start/end derived from the machine's absolute output tail (`info.output.1`, ours: `:1159`) |
| resized | JSON `resized{cols,rows,generation,checkpoint_base64}` (ref: `:80-86`) | `StreamEvent::Resized` (ours: `:357`, emitted at `:1215-1226`) |
| exited | JSON `exited{exit_code,final_offset}` (ref: `:87-91`) — ends the stream (ref: `:617-624`) | `StreamEvent::Exited` (ours: `:359`, `:1242-1269`); `final_offset` = output tail read at exit |
| controller_changed | JSON `controller_changed{attachment_id,generation}` (ref: `:93-96`) | machine signal on controller takeover/generation bump (ours: `:968-973`, `:1210-1214`) — needs the small `StreamEvent` extension in §12/Q3 |
| title_changed / foreground_process_changed | JSON (ref: `:97-98`) | **reserved, no producer today** (our machine never sets title/foreground: ours: `:625-638`) |

---

## 4. Framing, versioning, auth

### 4.1 Framing (mirror ref exactly)

- Frame = `u32 BE length` + payload (ref: `daemon.ts:659-666` encode, `:668-682`
  decoder). `MaxFrameBytes = 8 MiB` (ref: `daemon.ts:9`); oversized or truncated
  frames are protocol errors (ref: `:675`, `:681`).
- Request payload = JSON `{"token": "…", "request": {…}}` — the token rides **every**
  frame (ref: `daemon.ts:455`, `:485`, `:563-575`; R7).
- Response payload = JSON `WireResponse` (ref: `:42-100`).
- Subscribe connections multiplex: binary frames (first byte `0x00`) are output;
  everything else is JSON (ref: `:589-599`).
- Timeouts: 5s per one-shot request / claim / handoff-prepare (ref: `:195-199`,
  `:365-369`, `:538-548`); connect failures classify as `connect` before dispatch,
  `response` after (ref: `:542-546`).

### 4.2 Versioning

`ProtocolVersion = 7`, mirroring ref: `daemon.ts:8` (the ref states no other number;
R2 ruling). Our `PTY_PROTOCOL_VERSION` (ours: `persistent_pty.rs:407-409`, currently
`1`) is bumped to `7`. Consequences:

- Discover compares `registration.protocol !== ProtocolVersion` → hard
  `protocol` error, message `opencode-pty protocol mismatch: daemon=N, client=7`
  (ref: `daemon.ts:209-216`).
- Registrations written by earlier betas (protocol `1`) are discarded → fresh start.
  Terminals are best-effort across upgrades (ref: `daemon.ts:419-427` "persistent
  terminals will start fresh"), so this is an acceptable one-time loss, called out in
  the rollout (§11).
- `ping` must echo `instance_id`, `pid`, `protocol` (ref: `:217-226`) — guards
  against a recycled runtime dir holding a stale registration.

### 4.3 Auth

- **Token**: 32-hex random (ours: `new_random_token`, `persistent_pty.rs:1436-1446`),
  generated by the daemon at registration, written in `service.json` (0600,
  atomic — ours: `write_json_atomic`, `:1414-1434`) inside the 0700 runtime dir
  (ours: `:1394-1403`; ref: `daemon.ts:438`). Same-user-only by construction.
- **Per-frame check** (R7): every request frame's `token` must equal the registration
  token; failure → `error{message:"authentication failed"}` and the client maps that
  exact message to error kind `registration` (drop cached registration, re-discover)
  (ref: `daemon.ts:527-533`, `:579-583`).
- **Ownership**: `own` additionally requires the unexpired single-use ticket minted by
  `prepare_handoff` (ref: `daemon.ts:471-473`; ours' single-use consumption today is
  `state.json` removal, `persistent_pty.rs:1135-1138` — in the daemon arm the ticket
  is consumed in-daemon memory AND `state.json` removed).
- **Socket**: `<runtime-dir>/daemon.sock`, filesystem path (follows ref — the socket
  path is a file path in the registration, ref: `daemon.ts:38`, `:447`), `chmod 0600`
  at bind; the 0700 dir is the outer wall.

### 4.4 Stale-socket / dead-daemon detection (R7)

1. **Connect-probe**: discover reads `service.json`, then one-shot `ping`; ping
   failure (connect error / timeout / mismatched echo) = daemon unreachable
   (ref: `daemon.ts:201-226`).
2. **Pid liveness with generation guard**: the daemon writes a `service.boot` sidecar
   `{boot_id, start_time}` next to `service.json` — exactly the safekill
   `Expectation::Generation` record (ours: `safekill.rs:64-80`, written by
   `write_expectation`, `:280-287`; `boot_id` from
   `/proc/sys/kernel/random/boot_id` `:88-93`, `start_time` from `/proc/<pid>/stat`
   field 22 `:96-116`). A client (or the daemon itself, before reclaiming a
   predecessor dir) treats the registered pid as live only if the current `boot_id`
   matches and `/proc/<pid>/stat` still reports the recorded `start_time`
   (`process_alive`, `:248`). Mismatch ⇒ stale record ⇒ never signal, just reclaim
   (ours: `safekill.rs:16-22` "delete the record, log, never signal").
3. **No bare-PID kills**: the only signal paths are (a) the daemon's own children via
   their `ChildKiller` handles (ours: `persistent_pty.rs:1271-1292`), (b) the client
   SIGTERMming the daemon *it just spawned* through the owned child handle on
   failed startup (ref: `daemon.ts:267-271`), and (c) `shutdown` op = the daemon
   exits itself. Nothing ever signals a pid read from disk without passing
   `safe_signal`'s identity checks (ours: `safekill.rs:176`).

---

## 5. Registration / lock / lifecycle state diagrams (text)

### 5.1 Daemon process lifecycle

```
                spawn: oc-pty daemon --runtime-dir ROOT --name ID
                (detached, stdio ignore — ref daemon.ts:229-244)
                              │
                              ▼
              ┌───────────────────────────────┐
              │ Claim: mkdir ROOT/ID (0700);  │   lock busy & LIVE predecessor
              │ flock(service.lock, EX|NB);   │──────────────────────────┐
              │ write service.json{instance_  │   (probe per §4.4)       │
              │  id,pid,protocol,socket,      │                          ▼
              │  token} + service.boot;       │                 exit(code≠0) —
              │ bind daemon.sock (0600)       │                 client retries in
              └──────────────┬────────────────┘                 a FRESH dir (§8)
                             ▼
                    ┌────────────────┐  own{instance_id,ticket} ok
                    │  OWNERLESS     │─────────────────────────┐
                    │ (serving; no   │                         ▼
                    │  owner conn)   │                 ┌────────────────┐
                    └───────┬────────┘                 │ OWNED          │
                            │                          │ owner conn +  │
                            │ no terminals && no       │ terminals      │
                            │ owner for grace period   │ (children      │
                            ▼                          │ ALIVE — R5)    │
                    ┌────────────────┐  owner socket close, terminals   └───────┬────────┘
                    │ SELF-EXIT      │  live ⇒ back to OWNERLESS (adoptable)   │ shutdown op
                    │ (lock fd       │                                         ▼
                    │  closes; children                       ┌──────────────────────┐
                    │  get HUP as masters                    │ DRAIN: Machine::     │
                    │  close — daemon-death  │               │ shutdown kills kids  │
                    │  semantics, ours:      │               │ (ours :875-883),     │
                    │  persistent_pty.rs:44-56)              │ unlink sock, release  │
                    └────────────────┘                       │ lock, exit           │
                                                             └──────────────────────┘
```

Key invariants: the lock is held for process lifetime — kernel-released on death
(ours: `persistent_pty.rs:421-424`, `take_lock` `:1363-1382`); the registration's
`instance_id` never changes for a live daemon (the identity that handoff tickets
pin — ref: `daemon.ts:411-412`).

### 5.2 Client (server-side frontend) connection lifecycle

```
 boot ──▶ set root <data>/pty (ours boot.rs:275; ref index.ts:28-29)
        │
        ├─ OPENCODE_PTY_HANDOFF present? ──▶ adoption ladder (§6)  [boot.rs:483-504]
        │
        ▼
   no registration ── discover() ── ok ──▶ open owner conn: own{} ──▶ OWNED
        │                                   (no ticket ⇒ plain own, first server)
        │ ping fail / no service.json
        ▼
   op needs daemon? ── create ──▶ spawn-if-absent (§8) ──▶ claim ──▶ OWNED
        │
        └─ anything else ──▶ requestIfRunning: absent ⇒ soft-empty
                            (list ⇒ [], read ⇒ null — ref index.ts:143-145)

   steady state: owner conn (unref'd, ref daemon.ts:473) + per-request one-shots
                 + one conn per subscribe
   failure ladder per request (ref daemon.ts:313-333):
     error{authentication failed} ⇒ drop registration ⇒ rediscover ⇒ retry once
     connect error               ⇒ drop registration ⇒ (create only) respawn+retry
```

### 5.3 Registration/lock files

```
<root>/<id>/                 0700  (ours :1394-1403)
  service.lock               flock EX|NB, held for life (ours :1363-1382)
  service.json               {instance_id, pid, protocol:7, socket, token}
                             0600 atomic (ours :1414-1434; ref daemon.ts:33-39)
  service.boot               {boot_id, start_time}  (NEW; safekill record, §4.4)
  daemon.sock                0600 Unix socket
  state.json                 handoff snapshot; exists only between
                             prepare_handoff and own/consume (ours :195-206)
```

---

## 6. Adoption ladder (incl. the new socket-transfer arm)

Ports ref: `daemon.ts:403-428` onto the real daemon; the in-process ladder stays at
ours: `persistent_pty.rs:1047-1154` for the `OC_PTY_DAEMON=0` path unchanged.

```
adopt(inherited Handoff{directory, instanceID, ticket, expiresAt})
 │
 ├─1─ expires_at <= now ─────────────────────────▶ Discard(expired)
 │      (ref daemon.ts:406-407; ours :1049-1053)
 ├─2─ read service.json in directory ── fail ────▶ Discard(unreachable)
 │      (ref :203-205; ours :1054-1056)
 ├─3─ protocol ≠ 7 ──────────────────────────────▶ Discard(protocol)
 │      (ref :209-216; ours :1057-1060)
 ├─4─ instance_id ≠ inherited.instanceID ────────▶ Discard(changed)
 │      (ref :411-412; ours :1061-1064)
 │
 ├─5─ ARM A — LIVE DAEMON (new, R4): ping ok (§4.4), then
 │    owner claim {op:"own", instance_id, ticket}:
 │      ├─ "owned" ──────────────────────────────▶ ADOPTED-ALIVE
 │      │   • terminals + CHILDREN LIVE (R5) — no state restore at all
 │      │   • daemon consumes the ticket (single-use) + deletes state.json
 │      │   • daemon resets current[] (fresh selection epoch — v2's
 │      │     replacement has an empty map, ref index.ts:140)
 │      │   • if the handoff directory moved vs. our root: daemon-side
 │      │     inode-preserving migrate BEFORE own (ref :413-414, :435-444)
 │      │   (ref claim+ticket: daemon.ts:446-481, 415)
 │      └─ ticket rejected/expired ──────────────▶ Discard(invalid_ticket)
 │
 └─6─ ARM B — DEAD DAEMON (ping fails; lock free by kernel):
       ├─ state.json missing ────────────────────▶ Discard(no_state)
       │    (died without handoff = v2 daemon-died path,
       │     ref pty-handoff.ts:35-37; ours :1065-1073)
       ├─ state ticket ≠ inherited.ticket ───────▶ Discard(invalid_ticket)
       │    (ours :1071-1073)
       ├─ lock still held (daemon alive but socket-dead =
       │   unreachable-owner) ───────────────────▶ Discard(owned), then the
       │    client abandons: fresh dir + respawn (ref :275-292; ours :503-522)
       └─ valid snapshot ────────────────────────▶ ADOPTED-FROM-SNAPSHOT
            • client spawns oc-pty in the inherited dir (lock is free)
            • op restore{state} → Machine::restore(state) — the extracted
              loop of ours :1096-1133: replay windows intact, children gone
              (terminals marked exited — v2 daemon-death outcome, ours :50-56)
            • consume state.json (single-use, ours :1135-1138); registration
              rewritten with the new daemon's pid/socket (ours :1139-1149)

 ANY Discard: warn "Discarding PTY restart handoff; persistent terminals
 will start fresh", best-effort {op:"shutdown"} of a found-but-unclaimable
 daemon, abandon to a fresh identity — boot stays green
 (ref daemon.ts:417-427; ours adopt() :940-952; boot.rs:496-503).
```

Arm A is the flagship behavior change: today `Owned` is a dead end
(ours: `AdoptionError::owned`, `persistent_pty.rs:255-261`, test
`adopt_while_owner_alive_discards` `:1713-1726`); with the daemon it is the happy
path — the stopping server's sidecar/env chain (`OPENCODE_PTY_HANDOFF`, produced by
`pty_handoff_prepare` ours: `lifecycle.rs:1232-1289`, injected by
`pty_handoff_environment` `:1333-1353`, decoded at `boot.rs:483-504`) hands the
replacement a ticket that claims the SAME daemon with its LIVE children.

The `own` ticket vs `state.json`: both exist at prepare time (the daemon writes
`state.json` as crash insurance — R4). Successful `own` consumes both; Arm B uses
`state.json` only when the daemon itself died.

---

## 7. Checkpoint format spec (R6)

Today the checkpoint is "documented-format stub": raw UTF-8 of the screen rows
(`snapshot`: ours `persistent_pty.rs:788-793`; `resized` event: `:1215-1219`). The
daemon↔client wire makes it real and versioned. **Version 1** serializes exactly what
the machine tracks (rows + cursor + size + generation) — self-describing so later
versions can carry control-sequence-aware emulator state without silent misreads.

```
byte layout (all integers big-endian)

offset  size  field
0       8     magic        b"OCPTYCHK"
8       2     version      u16 (=1)
10      2     flags        u16 (bit0 = scrollback present; rest 0)
12      2     cols         u16
14      2     rows         u16
16      4     cursor_x     u32
20      4     cursor_y     u32
24      8     generation   u64  (controller generation at capture, ours :390)
32      8     scroll_len   u64  (byte length of the scrollback blob)
40      var   scrollback   UTF-8; screen rows joined by '\n', top→bottom,
                            exactly `rows` rows (the machine's `Live::rows`,
                            ours :388, :1185)
40+len  4     crc32        IEEE crc32 of bytes [0, 40+len)
```

Rules:

- Producers: the daemon's `snapshot` op response (`checkpoint_base64`) and every
  `resized` stream event (ref: `daemon.ts:54-61`, `:80-86`); consumers fast-forward
  on resize instead of replaying from scratch (ref: `:107-113`; our WS handler already
  forwards it base64 — ours: `handlers/persistent_pty.rs:333-347`).
- Decoders reject: wrong magic, unknown version (≠1), crc mismatch, `rows`/`cols` = 0,
  scrollback row-count mismatch → `protocol` error kind (§9).
- The in-process backend produces the identical bytes (same machine code path), so
  hermetic round-trip tests pin the format regardless of backend.
- `exited` terminals keep their last checkpoint ( Arm B restore stores rows/cursor in
  `TerminalSnapshot`, ours: `:211-224` — the restored machine re-emits a v1
  checkpoint from those fields).

---

## 8. Spawn-if-absent policy + binary discovery order (R2)

### 8.1 Discovery order

1. `OPENCODE_PTY_BIN` env override (ref: `daemon.ts:161`
   `process.env.OPENCODE_PTY_BIN || "opencode-pty"`).
2. Binary next to the running executable: `<parent(current_exe())>/oc-pty`
   (replaces v2's npm-package-adjacent `resolveBinary(global.bin)` /
   `binaryPath` binding — ref: `index.ts:130-135`, `pty-binding.ts:2-5`; our
   published layout puts `oc-pty` beside the main binary).
3. `PATH` lookup of `oc-pty` (the ref's bare-name fallback, `daemon.ts:161`).

Resolution happens once and is cached with reset-on-failure
(ref: `index.ts:131-135`). Missing binary ⇒ `spawn` error kind ⇒ `create` surfaces
`UnavailableError(service:"opencode-pty")` (ours: `handlers/persistent_pty.rs:74`).

### 8.2 Spawn + readiness (mirror ref `daemon.ts:228-273`)

- Spawn detached, `stdio: ignore`, unref'd:
  `oc-pty daemon --runtime-dir <root> --name <id>` (ref: `:235-237`; `id` is a fresh
  random token, ours: `new_random_token` `:1436-1446` — same shape as v2's
  `crypto.randomUUID`, ref: `:291`).
- Poll `discover()` every 50 ms up to a 5 s deadline; on success, `claim`
  (`own` without ticket) (ref: `:246-262`).
- Deadline/failure ⇒ SIGTERM the child *we spawned* (handle-based, §4.4) and
  surface the last error (ref: `:263-272`).
- **Fresh-dir restart**: if startup fails because the old dir's lock is held by an
  unreachable daemon, `abandon()` — new random id — and retry spawn once in a fresh
  directory (ref: `:275-292`; ours' two-attempt `set_runtime_dir` `:503-522` is the
  same policy in-process).
- Only `create` starts daemons; `list`/`read` treat absence as soft-empty and
  `write`/`resize`/`control`/`input`/`snapshot`/`terminate`/`subscribe` fail with
  `connect` → `UnavailableError` (§3 table; ref: `index.ts:378-385`).

---

## 9. Error taxonomy

**Client transport kinds** (port of ref `DaemonError`, `daemon.ts:134-138`):

| kind | meaning | trigger examples |
|---|---|---|
| `connect` | socket unreachable / claim timeout | ECONNREFUSED, 5s claim timeout (ref `:195-199`) |
| `response` | connected but no/bad reply | closed-before-response, post-dispatch timeout (ref `:542-546`) |
| `registration` | auth/identity invalid → drop + rediscover | `authentication failed` (ref `:527-533`), own rejected |
| `protocol` | version/shape mismatch | protocol≠7 (ref `:209-216`), ping echo mismatch (ref `:217-224`), checkpoint decode (§7) |
| `spawn` | binary missing / child failed | discovery exhausted (§8.1), spawn error (ref `:229`) |

**Wire errors**: `error{message}` responses (ref: `:99`); `authentication failed` is
the only message with transport semantics (→ `registration`), all others →
`protocol`/`response` by call site (ref: `:527-533`).

**Adoption reasons** (unchanged set, ours: `persistent_pty.rs:229-273`):
`expired` / `unreachable` / `protocol` / `changed` / `owned` / `invalid_ticket` /
`no_state`. With the daemon, `owned` demotes from "dead end" to "unreachable-owner,
retry fresh dir" (§6 step 6), and `invalid_ticket` gains the live-arm producer.
Every discard logs and leaves a fresh identity — boot never fails
(ours: `adopt()` `:940-952`).

**HTTP mapping** (unchanged): `NotFoundError` → 404 `PtyNotFoundError`;
`UnavailableError` → 503 `ServiceUnavailableError(service:"opencode-pty")`
(ours: `handlers/persistent_pty.rs:69-75`; ref: `handlers/persistent-pty.ts:236-249`).

---

## 10. Test matrix

### 10.1 Unit (no processes)

- Frame codec: encode/decode round-trip, 8 MiB cap enforcement, truncated-frame and
  oversize rejection (ref parity: `daemon.ts:659-682`).
- Binary output-frame parse: `[0][start u64BE][end u64BE][data]`, <17-byte rejection
  (ref: `:589-597`).
- Checkpoint v1: round-trip, crc/magic/version/row-count rejection (§7).
- `WireTerminal` ↔ `Info` mapping incl. `pty_persistent_<n>` id parse and
  lifecycle variants (ref: `index.ts:390-418`).
- Discovery-order resolution with fixture dirs (`OPENCODE_PTY_BIN`, exe-sibling,
  PATH) (§8.1).
- `service.boot` record write/read; stale-boot_id refusal path (ours:
  `safekill.rs:318-367` precedent).

### 10.2 Hermetic in-process (existing suite unchanged — R3)

- The whole `persistent_pty.rs` test module keeps passing as-is
  (ours: `:1475-1853`), including `restarted_server_reattaches_to_terminal_state`
  (`:1594-1672`), the ladder discards (`:1674-1726`), the daemon-died path
  (`:1732-1762`), single-use ticket (`:1785-1820`), inode-preserving migration
  (`:1825-1852`).
- New: `Machine::restore` extraction is covered by the same tests (pure refactor).
- Boot tests: `OPENCODE_PTY_HANDOFF` consumption/garbage/env-scrubbing
  (ours: `boot.rs:699-781`).

### 10.3 Process-level integration (real `oc-pty` binary, temp runtime roots)

| # | Scenario | Asserts |
|---|---|---|
| I1 | **kill-server → restart → adopt → child alive** (flagship, R5/Arm A): server A creates a terminal running a marker child (`sleep`-like with a pidfile), `prepare_handoff`, kill -9 server A; server B boots with the sidecar env | terminal listed `running`, SAME child pid alive (pidfile + `/proc` generation check §4.4), attach replays pre-kill output, new input reaches the child |
| I2 | **daemon-death kills children** (R5): SIGKILL the daemon | children gone (masters closed), client ops → `connect` error → 503; next `create` respawns (fresh-dir policy if lock raced) |
| I3 | **replay restore** (Arm B): daemon killed AFTER a handoff snapshot exists; replacement adopts via `state.json` | terminals restored `exited` with pre-death replay windows; writes no-op; ids continue (`pty_persistent_N+1`) — mirrors ours `:1594-1672` at process level |
| I4 | **checkpoint round-trip** (R6): create → write → resize → `snapshot` op → decode v1 checkpoint; `resized` stream event carries a decodable checkpoint | §7 fields (cols/rows/cursor/generation/scrollback) match machine state; crc rejects a corrupted byte |
| I5 | **stale socket** (R7): fabricate `service.json` pointing at a dead socket + a `service.boot` naming a recycled pid | discover fails via connect-probe; generation guard refuses to treat the recycled pid as live; spawn-if-absent proceeds in a fresh dir; nothing signalled |
| I6 | **token rejection** (R7): connect with wrong token | daemon replies `error{"authentication failed"}`; client classifies `registration`, drops cache, recovers on next discover with the true token |
| I7 | **protocol mismatch**: hand-write `service.json` with `protocol:1` (or any ≠7) | `protocol` error with daemon=N client=7 message (ref: `:209-216`); discard-fresh |
| I8 | **shutdown semantics**: `shutdown` op | `ok` then discover fails ≤5s; children dead; `state.json` invalidated (ours: `:1039-1045`) |
| I9 | **migration**: handoff directory outside the new root | lock+registration move inode-preservingly; the moved lock still excludes a competing claim (ours `:1825-1852` semantics over the socket path) |
| I10 | **two contenders**: both spawn daemons / both try `own` | exactly one `owned`; loser discard-fresh; single-use ticket consumed once (ref: `pty-handoff.ts:32-34`) |
| I11 | **=0 escape hatch**: `OC_PTY_DAEMON=0` | frontend binds InProcess; no `oc-pty` process ever spawned; HTTP surface identical |

---

## 11. Migration / rollout plan

1. **This lane (default ON + escape hatch).** Land the library refactor (machine
   semantics unchanged), `oc-pty` bin, transport, frontend. Default backend = Daemon
   (spawn-if-absent per §8). `OC_PTY_DAEMON=0` selects the InProcess backend —
   exactly one release window, the repo's established M-retire flag pattern
   (precedent: ROADMAP.md:219 — `OC_*` gates + `=0` variants retired in the next
   lane). Hermetic tests unchanged (R3). Release archives ship `oc-pty` beside the
   main binary so discovery step 2 (§8.1) always hits.
2. **One-time registration reset.** Bumping `PTY_PROTOCOL_VERSION` 1→7 (§4.2)
   invalidates runtime dirs written by earlier betas → one "persistent terminals
   will start fresh" event per upgrading machine. Terminals are best-effort across
   restarts by design (ref: `daemon.ts:419-427`); documented in release notes.
3. **Retirement lane (next release).** Delete `OC_PTY_DAEMON` and the InProcess
   backend glue in `oc-server` (frontend always Daemon; absent binary ⇒ clean 503).
   The machine library + hermetic tests REMAIN — they test the implementation the
   daemon hosts (this is the R3 carve-out: retire the path selection, never the
   machine or its tests). Precedent for keeping hermetic coverage while retiring a
   path: the M-engine window (ROADMAP.md:263).
4. **Compat guards.** The `read` extension op (§3) is additive; unknown-op replies
   follow the wire `error` shape so mixed client/daemon versions degrade to clear
   503s, never hangs. The `service.json` schema is unchanged from v2's
   (ref: `daemon.ts:33-39`) — a v2-written registration on the same machine simply
   fails the protocol check and starts fresh (§4.2).

---

## 12. Risks + open questions (for the design gate)

**Risks**

- **R-1 Child-process leakage on daemon death is kernel-arbitrated**: children die
  because the masters close (HUP to foreground process groups) — we do not (and must
  not) kill them ourselves (R7). If a child ignores HUP and detaches, it can outlive
  the daemon; same exposure as v2, accepted.
- **R-2 Replay over the wire vs the frame cap**: v2's `attached` carries the whole
  replay inline as base64 (ref: `daemon.ts:78`); our `OUTPUT_LIMIT` is 8 MiB
  (ours: `persistent_pty.rs:405`) and base64 inflates ×4/3 against the same 8 MiB
  `MaxFrameBytes` (ref: `daemon.ts:9`) — a full buffer would not fit. Proposed:
  deliver replay as chunked binary output frames after `attached` (offsets make
  reassembly exact), keep the inline field for small replays only. See Q4.
- **R-3 Orphan daemons linger by design**: a daemon with live terminals and no owner
  persists (that IS persistence, R5); a daemon with neither self-exits after a grace
  period (§5.1). No v2 citation exists for the grace period — it is our addition to
  bound clutter; conservative default 60 s.
- **R-4 `stdio: ignore` hurts debuggability** (ref: `daemon.ts:236`). Proposal: the
  daemon appends a small JSONL log inside its runtime dir (0600), best-effort.
- **R-5 Same-user trust boundary**: token + 0700 dir keep other users out, but any
  same-user process can read `service.json` and drive the daemon — identical to v2;
  documented, not solved.
- **R-6 Platform scope**: Unix sockets + `flock` + `/proc` make this Linux-first;
  non-Linux unix degrades the way safekill does (refuses rather than misbehaves,
  ours: `safekill.rs:37-47`).

**Open questions**

1. **Protocol version = 7 (binding R2)** vs starting a local lineage at 1: adopting 7
   signals "wire-compatible number-space with v2" while our JSON adds the `read`
   extension op and checkpoint bytes. The mismatch path already handles any skew
   (§4.2). Confirm 7 at the gate.
2. **`current[]` daemon-side + `reset-on-own`** vs v2's client-side map
   (ref: `index.ts:139-140`): daemon-side preserves R1's single implementation but
   makes selection state shared across a daemon's lifetime; with exclusive ownership
   this is unobservable. Confirm.
3. **`controller_changed` needs a machine producer**: extend `StreamEvent` with
   `ControllerChanged{attachment_id, generation}` (a delta to the machine, not a
   second machine) so observers learn of takeovers (ref: `daemon.ts:93-96`, `:115`);
   `title_changed`/`foreground_process_changed` stay reserved no-producers. Size:
   small; needed for wire parity. Approve the delta.
4. **Replay transport** (R-2): inline-base64-in-`attached` (v2-faithful, capped) vs
   chunked binary frames after `attached` (robust at buffer limit). Recommend
   chunked; needs a gate decision because it deviates from ref bytes-on-wire.
5. **`restore` op privilege**: restrict `restore{state}` to owner connections only
   (post-`own` or the spawning client) so a bare token holder cannot inject fake
   terminal history. Confirm restriction list (`own`, `restore`, `prepare_handoff`,
   `shutdown` = owner-only; everything else = token).
6. **Grace period value** for ownerless-childless daemon self-exit (R-3, proposed
   60 s) and whether `list` from a non-owner client should keep such a daemon alive
   (proposed: no — only owner connections and live terminals pin it).
