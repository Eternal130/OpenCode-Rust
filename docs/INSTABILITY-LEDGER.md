# Instability Ledger (va8, 2026-11-23)

Consolidated register of every recently-sighted flaky/unstable test:
failure shape (quoted from the on-disk log) + cause attribution or a
concrete plan. This is the public companion to the book's §2.2#4 flake
watchlist (the book stays authoritative; entries here carry the full
evidence chain). Sightings count only NEW first-attempt reds (retry
passes are recorded but do not block merges per §2.2#4).

Evidence base: `.test-logs/va7-lane/oc-tui,oc-cli,parity-20261123-211653-va7.log`
(va7 merge gate, 2432 tests, 1341.837s wall), the va5-era battery
probes, and the va8 audit.

---

## Resource-ceiling class (infra, not test logic)

### RL-1 — daemon spawn EAGAIN under battery concurrency (systemic)

Shape (`.test-logs/oc-tui-20261123-165259-exp-t4-r1.log`, repeated ×8+ in
one round; same shape in `-r2`):

```
the managed server never registered; service start exited (exit status: 1):
Failed to start server: exited with exit status: 101
opencode plugin-host: spawn failed (/usr/local/bin/bun: Resource temporarily unavailable (os error 11))
thread 'main' panicked at crates/oc-core/src/config/watch.rs:336:14:
spawn notify dispatcher: Os { code: 11, kind: WouldBlock }
```
and
```
thread 'main' panicked at .../tokio-1.53.2/src/runtime/scheduler/multi_thread/worker.rs:514:13:
OS can't spawn worker thread: Resource temporarily unavailable (os error 11)
```

Attribution: `os error 11 (EAGAIN)` on thread/fork = the dev-gate
cgroup's `TasksMax=512` (`tools/dev-gate.sh:139`) is exhausted while 3-4
realchain legs coexist (each leg = gateway python + rust daemon + rust
TUI + bun TUI + bun daemon + plugin-host buns). This single mechanism
explains a large share of the realchain first-attempt reds
(geom160/w13/pair family "never registered" failures across rounds).

Plan: raise the battery-run TasksMax (or run the battery outside the
512-task scope). Controller decision required — `tools/dev-gate.sh` is
the shared single-entry gate; not changed unilaterally in va8.

### RL-3 — oc-plugin-host flagship_sidebar ×2: /tmp/code-* IDE-socket interference (environmental, deterministic)

Shape (`.test-logs/workspace-20261123-230321-va8ws.log`, oc-plugin-host::flagship_sidebar,
`flagship_sidebar_renders_plugin_version_and_padded_columns` + `flagship_sidebar_section_materializes_from_the_real_plugin`,
failed TRY1+TRY2 AND standalone rerun — retry does NOT pass):

```
opencode plugin-host: tui entry loaded for oh-my-opencode-slim (oh-my-opencode-slim:tui, sidebar slot: true)
opencode plugin-host [host] error: uncaughtException: ENXIO: no such device or address, open '/tmp/code-19560cfb-5f7e-4d77-9e55-ac85c122d43c'
thread 'flagship_sidebar_renders_plugin_version_and_padded_columns' panicked at crates/oc-plugin-host/tests/flagship_sidebar.rs:226:5:
the badge shows the plugin's own version 3.0.3: ""
```

Attribution: the uncaughtException path is the host's VS Code remote
server socket (`/root/.vscode-server/.../code-server
--socket-path=/tmp/code-19560cfb-...`, alive since 09:58). Empirical:
`fs.openSync` on ANY unix socket inode returns ENXIO on Linux
(reproduced with both the pinned and /usr/local/bin bun on a probe
script). Some code in the plugin-host boot chain enumerates /tmp/code-*
and hard-fails on the foreign IDE socket, killing the plugin's tui
entry → the sidebar section never materializes → empty title. The same
ENXIO line appears in the RL-1 logs inside daemon boot failures — it is
environment noise of this box, not a product regression of va8 (va8's
diff touches no oc-plugin-host code; the tests previously only SKIPPED
in old workspace gates at 0.004s because the flagship source resolved
from an npm stamp that did not exist then).

Plan: (a) product-side — the /tmp/code-* discovery must tolerate
socket ENXIO (skip non-openable sockets) instead of an uncaught
exception; (b) test-side — if (a) is not taken, gate
flagship_sidebar on the absence of foreign /tmp/code-* sockets. Not
whitelisted (retry fails). Controller decision required for both.

### RL-2 — stale-binary guard vs shared CARGO_TARGET_DIR

Shape: `realchain_harness.rs:161` "stale binary ... rebuild via
tools/dev-gate.sh" first-attempt red when another lane's rebuild races
the battery on the shared target dir. Not a test bug; a scheduling
discipline item (one gate at a time is already the rule). Plan: none
beyond the existing single-gate rule.

---

## Realchain family

### g461_ref_port_walks_past_incumbent_listener — 1 sighting — FIXED (va8)

Shape: `AddrInUse` panic on the dummy-incumbent bind
("band free by construction"). Cause: parallel legs' live ref services
land in the same 50000-60000 band. Fix: adaptive incumbent (bind
success = dummy incumbent; AddrInUse = the real occupant IS the
incumbent). Verified ×20 gate rounds zero red
(`.test-logs/*va8g*.log`, `.test-logs/oc-tui-20261123-215*.log`).

### pty_reconnecting_overlay_on_loss_and_recovery — 2 sightings (book watchlist)

Shape (va7 merge gate, `TRY 1 FAIL [25.031s]`, `.test-logs/va7-lane/...va7.log:1996-2015`):

```
opencode tui panicked: panicked at crates/oc-tui/tests/pty_core_interactions.rs:1183:9:
the Reconnecting overlay appeared after the retry threshold
opencode tui panicked: panicked at crates/oc-tui/tests/pty_core_interactions.rs:664:13:
the TUI did not exit in time: deadline has elapsed
```

Attribution (va8): HARNESS timing budget, not product.
- The `drive()` outer cap is 25s (`pty_core_interactions.rs:649`); the
  test's inner budget sums to 25.5s (wordmark ≤5s + 0.5s sleep +
  overlay ≤20s) — under load the outer cap fires before the inner
  waits complete (TRY1 died at exactly the 25s cap).
- The overlay paints only when the scripted loss (open stream closes
  on its first idle tick, ~20ms after open) crosses the 1s retry
  threshold before reconnect — the paint instant is load-sensitive;
  under 4-way merge-gate load the TUI's event-loop backlog can push it
  past the 20s inner window.
Plan: subdivide the outer cap across the inner waits (e.g.
`drive_with_budget(Duration::from_secs(60))` for this test, or make the
scripted loss fire after an explicit handshake rather than the first
idle tick). Not whitelisted — stays on the watchlist until the budget
fix lands.

### geom160_wide_pair / w13_g_longline_pair / w13_perm_always_pair — booked (three-element entries in the book §2.2#4)

Shapes on disk: `TRY 1 FAIL [3.657s]/[4.617s] geom160_wide_pair`,
`TRY 1 FAIL [6.021s] w13_g_longline_pair`,
`TRY 1 FAIL [127.262s] w13_perm_always_pair` (retry-passed rounds in
`oc-tui-20261122-195833-gate.log` family). Dominant underlying mechanism
per RL-1: daemon spawn EAGAIN / never-registered under battery
concurrency. Plan: re-triage after RL-1's TasksMax lever — several of
these may retire as RL-1 duplicates rather than genuine timing flakes.

---

## PTY / VT family

### pty_lock audit (va8, T4)

`pty_lock()` (`crates/oc-tui/tests/vt_harness.rs:532` pre-va8) is a
process-static `std::sync::Mutex<()>`: it serializes tests ONLY under
plain `cargo test` (one process per binary). Cross-process (nextest,
one process per test) it is INERT. Its real protected resource is
`sandbox_env()` mutating process-global `HOME`/`XDG_*` env
(`vt_harness.rs:512-517`). The PTY family's cross-process
serialization is a runner property (`--profile pty`, test-threads=1).
va8 adds an opt-in cross-process shard flock (`PTY_LANE_SHARDS=N`,
default unset = historical behavior) — the T9_LANE_SHARDS pattern;
same-shard legs merely serialize.

### pty_fuzz_smoke — booked watchlist; also the merge gate's single slowest test

`oc-cli::pty_fuzz pty_fuzz_smoke` = **325.18s** in the va7 merge gate —
29% of the entire oc-cli family's summed time, one test. Booked as a
flake; also the top T4 speedup target (cap/tune fuzz iterations).
Plan: split out of this ledger once tuned.

---

## Merge-gate speed audit (va7 log, per-test sums; wall 1341.837s @ t4)

SUM(per-test) = 5326.7s / wall 1341.8s = 3.97 ≈ 4.0 → the gate is fully
saturated at t4; there is no idle-lock headroom left. Composition:

| bucket | summed | n |
|---|---|---|
| realchain battery | 3917.1s | 107 |
| pty family | 642.8s | 206 |
| other (2117 tests, avg 0.36s) | 766.9s | 2117 |

Top single tests: pty_fuzz_smoke 325.2s; realchain_pair geom40 263.8s /
geom160 257.6s; realchain_g454 writable_both 223.4s / superpowers
216.9s; vt_hover_sweep ×13 ≈ 470s summed. The realchain legs dominate
(74% of summed time) — meaningful gate speedup requires either the
battery shard lever (T9_LANE_SHARDS / PTY_LANE_SHARDS, both default
off) or the RL-1 resource fix so legs stop inflating each other's
in-test lock waits (nextest per-test durations for realchain legs
include lane_lock flock wait).
