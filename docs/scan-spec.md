# The Scan Specification (扫描细则) — LIVING DOCUMENT

Version: v2 (integrated from SPEC-R1's 52-gap register + taxonomy v2; the
program protocol lives in docs/scan-program.md). Every future dev->scan->fix
loop's scan agent executes THIS spec. Refinement protocol: per-perspective
review rounds until convergence; each round's gaps integrate here with a
version bump.

## Standing instrument layer (binds EVERY axis — no axis is valid without it)
I1 REAL payloads end-to-end: no hand-set ids/metadata; chains verified from
   our own server/plugin-host outward (self-consistency, not ref-parity alone).
I2 REAL plugin sandbox: omo-slim (and any plugin with UI surface) installed
   in the test env; ref+plugin = the ground truth pair for plugin surfaces.
I3 Virtual clock for temporal claims (cadence = drive source, not redraws).
I4 Token-exact cell assertions: color/style verified per element x state.
I5 Falsifiability: every axis clause names its evidence artifact (capture,
   trace, metric) — a claim without an artifact is a finding of nothing.
I6 Miss-attribution: every finding records which prior instrument missed it
   and why (inventory-grade / fixture-happy-path / adjudication-masking).
I7 Bottom-up ref enumeration as the completeness backstop for feature axes.

## A — Visual / Rendering
A1 Theme tokens x element x STATE (busy/error/hover/selected/disabled/
   expanded/interrupted — the cell x state matrix, numeric sweep at edges)
A2 Geometry: padding/gap/centering/truncation/wrap rules per surface
A3 Edge + huge data: empty/long/narrow/overflow/CJK/emoji/10k-message
A4 Temporal at vt-clock: every animation's cadence, tick source, per-frame
A5 Plugin-rendered surfaces (ref+plugin ground truth; per registered surface)
A6 MINI frontend (full axis — no longer a spot-check): every panel/state
A7 Theme-system chain: dialog apply, live preview, system theme detection

## B — Interaction / Input / Environment
B1 Keyboard: every binding x route/mode/layer END EFFECT (not table
   presence); chords/leader timing/conflict resolution x config variants
B2 Pointer: every target under REAL shapes; rect lifetimes x transitions;
   drag/hold/middle/right; wheel in every scrollable
B3 Focus/layering/modality: every legal stack's routing
B4 Input methods: paste forms (bracketed/image/huge), clipboard, IME, kitty
B5 Terminal events: resize/focus/suspend per surface
B6 TERMINAL MATRIX: TERM x truecolor x kitty x tmux (TM-1..10 owned here)
B7 PLATFORM/ENV matrix: OS/paths/shell/locale env variations

## C — Behavior / State Machines
C1 Session lifecycle transition table (materialized, every cell's UI effect)
C2 Navigation graphs: tabs/picker/family/session.child
C3 Permission + attention flows end-to-end (incl. expiry/queueing)
C4 Config leaf-key round-trips: every cli.json/config leaf written+read back
C5 Error/recovery: server loss mid-X, plugin crash, partial writes
C6 Plugin lifecycle: install/enable/disable/update/fail x UI
C7 Background jobs surface behavior
C8 WORKTREE family (UI + git semantics + refresh)
C9 MCP family (servers/status/tools/needs-auth/elicitation-blocked)
C10 PROVIDER AUTH family (login flows x status x reconnect x credentials)
C11 UPDATE/UPGRADE family (notice/dialog/install/restart/migration UX)
C12 Onboarding/migration first-run states

## D — Data / Communication
D1 SSE: type x payload field x ordering, publisher->every consumer, error
   injection, under real flows + plugin activity
D2 REST: every endpoint field-level incl. negative paths, actual client calls
D3 Plugin-host boundary with real payloads (per API omo-slim exercises)
D4 Persistence: writer-reader pairing per artifact, migrations, cross-install
D5 Self-consistency invariants (id/metadata chains across our layers)
D6 WEBUI serving (asset hosting, SPA fallback, auth, PWA, the app's calls)
D7 ACP protocol surface
D8 Share/export data paths

## E — Cross-cutting Quality (all new)
E1 Performance FEEL: input latency, jank/stutter, frame pacing (marquee class)
E2 Startup time-to-interactive (TTI) per surface
E3 Memory growth over long sessions (soak-metric'd, falsifiable)
E4 Huge-data behavior (repositories, sessions, pastes, trees)
E5 Accessibility: contrast, colorblind themes, keyboard-only completeness
E6 i18n/locale (width, truncation, ordering, formats)
E7 Error-message quality (user-comprehensible, actionable)
E8 Security posture (prompts, secrets handling, path safety)
E9 Teardown/resource cleanup (processes, files, terminal state)

## F — Adversarial / Meta
F1 Materialized feature matrix: command x route x state — every cell owned
F2 Bottom-up ref enumeration: every export accounted for
F3 CONCURRENCY: two clients one server, racing operations, reconnect races
F4 NETWORK degradation: slow/timeout/offline/retry semantics
F5 Soak with metrics (memory/handles/event ordering over hours)
F6 Fresh-eyes + the miss ledger (what did the directions themselves miss)

## Round log
- v1: docs/scan-program.md R1-R5 (superseded as spec, remains as protocol)
- v2: SPEC-R1 integrated (52 gaps; axes E all-new, B6/B7/C8-C12/D6-D8 new,
  instrument layer binding). NEXT: SPEC-R2 fresh-eyes review of THIS doc.
