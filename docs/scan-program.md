# The Five-Round Repository Scan Program (user-directed, 2026-10-17)

Process per round: the orchestrator sets directions + classifications -> a
scan agent hunts EXHAUSTIVELY within (and beyond) them -> findings feed a
fix wave -> the NEXT round's directions are refined from what the previous
round revealed (missed angles become first-class). Five rounds minimum.
The scan is the acceptance instrument (differential testing retired).

## Round framework (living — refined after each round)

### Round 1 — VISUAL / RENDERING (assigned: lane RESCAN, in flight)
1.1 Theme-token->element binding map: every token the ref binds, the exact
    element/cell it paints, vs ours (esp. indicators: busy dot, pulse sweep,
    gutters, scrollbars, attention marks, unread dots, placeholders).
1.2 Real-plugin rendered surfaces (ref + plugin installed = ground truth).
1.3 Temporal: animation cadence, tick sources, per-frame content.
1.4 Edge states: empty/long/narrow/overflow/wide-char/emoji rendering.
1.5 Geometry: paddings, gaps, centering, truncation, wrap rules.

### Round 2 — INTERACTION / INPUT (dispatched this round)
2.1 Keyboard: every binding's REAL end-behavior (not dispatch-table
    presence): what each key actually does in each route/mode/layer, chord
    arming, leader timeouts, conflict resolution.
2.2 Pointer: every hit target under REAL data shapes (real metadata ids,
    real plugin rows); hit-rect lifetimes across route/mode changes;
    drag/hold/middle/right; wheel in every scrollable.
2.3 Focus/layering/modality: dialog-in-dialog, permission-over-composer,
    overlay-over-strip; input routing at every layer stack.
2.4 Input methods: paste forms (bracketed/image/large), IME composition,
    clipboard read/write, kitty keyboard modes.
2.5 Terminal events: resize at every surface, focus loss/regain, suspend
    ($EDITOR) round-trip, mouse-mode transitions.

### Round 3 — BEHAVIOR / STATE MACHINES (directions refined after R1+R2)
3.1 Session lifecycle states x UI (new/active/busy/idle/errored/interrupted/
    compacted/forked/shared/exported/backgrounded/deleted) — every
    transition's UI consequence vs ref.
3.2 Tabs/picker/family navigation full graph.
3.3 Permission/attention flows end-to-end.
3.4 Onboarding/config persistence round-trips (boot->toggle->restart).
3.5 Error/recovery paths (server loss mid-X, plugin crash, partial writes).
3.6 Plugin lifecycle states (install/enable/disable/update/fail) x UI.

### Round 4 — DATA / COMMUNICATION / SELF-CONSISTENCY
4.1 SSE: every event type x payload field, publisher->consumer end-to-end
    (server->TUI, server->webui app), under real flows incl. plugin activity.
4.2 REST: every endpoint's request/response the clients ACTUALLY make vs
    our shapes (field-level, error paths).
4.3 Plugin-host boundary with REAL plugin payloads (omo-slim first).
4.4 Persistence: every state file/DB row writer vs reader, migration paths.
4.5 Self-consistency invariants: ids/metadata chains our own layers must
    uphold (the subagent sessionID class).

### Round 5 — ADVERSARIAL GAP HUNT (directions from R1-R4's misses)
5.1 Feature matrix: every command x every route x every state — cells
    never exercised by any prior round.
5.2 Bottom-up ref enumeration: every exported component/hook/service in
    the ref tree accounted for (ported/absorbed/absent-justified).
5.3 Fresh-eyes: what NO round looked at (the meta-question each round
    answers: what did this round's directions themselves miss?).
5.4 Real-environment soak findings (long session, real plugins, real data).

## Round log
- R1: RESCAN lane (axes 1.1-1.5 + interaction-real-shapes + C/S real-flow
  + instrument-miss attribution). IN FLIGHT.
- R2: dispatched after framework finalized — see docs/scan-round2-findings.md.
- R3-R5: directions refined from R1/R2 results before dispatch.
