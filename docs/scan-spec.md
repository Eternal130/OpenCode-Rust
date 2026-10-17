# The Scan Specification (扫描细则) — LIVING DOCUMENT

Version: v3. Protocol: docs/scan-program.md (per-perspective convergence).
Rounds: v1 (R5 framework) -> SPEC-R1 (52 gaps) -> v2 -> SPEC-R2 (30 gaps +
the [G][F][M] keystone + I8-I11) -> THIS v3. Every clause carries
**[G]ranularity / [F]idelity / [M]ethod** — an agent satisfying the letter
must also satisfy the spirit.

## Standing instrument layer (binds EVERY clause)
I1 REAL payloads end-to-end — no hand-set ids; chains from our own
   server/plugin-host outward (self-consistency).
I2 REAL plugin sandbox (omo-slim first); ref+plugin = ground truth for
   plugin surfaces.
I3 Virtual clock for temporal claims (cadence = drive source).
I4 Token-exact cells per element x state.
I5 Falsifiability: every clause names its evidence artifact.
I6 Miss-attribution ledger per finding (inventory-grade / fixture-happy /
   adjudication-masking).
I7 Bottom-up ref enumeration as feature completeness backstop.
I8 **Falsifiability canary**: before a round's evidence is accepted, inject
   a known defect into one axis (wrong token cell, dead hit target) and
   verify the method CATCHES it; an axis whose method can't catch the
   canary is redesign on the spot.
I9 **Provenance ledger**: every fixture/capture records its source
   (real-run capture vs synthetic); synthetic data is never sole evidence.
I10 **Ground-truth pin**: ref commit pinned per round; re-baseline on ref
    release; ref+plugin pin recorded with the report.
I11 **Process + output contract**: ≤45-min chunks, no backgrounded scans,
    worktree isolation, per-round output format (gap register + adequacy
    table + convergence estimate), no summary-only dispatch.

## A — Visual / Rendering
A1 Theme tokens x element x STATE matrix (busy/error/hover/selected/
   disabled/expanded/interrupted), numeric edge sweep.
   [G] per token x per state cell [F] I4 cell capture vs ref token map
   [M] ref token->element enumeration + vt capture diff.
A2 Geometry (padding/gap/centering/truncation/wrap). [G] per surface x
   size {40,60,80,120,160} [F] vt grid capture [M] rule-by-rule vs ref.
A3 Edge + huge data (empty/long/CJK/emoji/10k-msg). [G] per family x
   magnitude {0,1,big,huge} [F] real-shaped corpus [M] capture + perf note.
A4 Temporal at vt-clock: cadence, tick source, per-frame. [G] per animation
   x phase [F] I3 clock + drive-source audit [M] frame series capture.
A5 Plugin-rendered surfaces. [G] per registered surface x state [F] I2
   ref+plugin pair [M] side-by-side capture diff.
A6 MINI frontend full axis. [G] per panel x state x size [F] PTY + I1
   [M] capture vs mini spec (mini/theme.ts quantizer rules).
A7 Theme-system chain (apply/live-preview/system detect). [G] per theme x
   step [F] I4 [M] apply-path trace + capture.
A8 **Rich content** (NEW): mermaid/merman rendering, latex, code-fence
   families, any ref-shipped content renderer. [G] per content family x
   size [F] ref ships it -> we must (grep-verified: ref has merman+latex
   packages; ours has neither axis nor code) [M] capability inventory +
   render parity per family.

## B — Interaction / Input / Environment
B1 Keyboard end-effect per binding x route/mode/layer x config-variant.
   [G] per binding x state [F] I1 real chain [M] dispatched-action trace,
   not table presence.
B2 Pointer under real shapes; rect lifetimes x transitions; drag/hold/
   middle/right; wheel everywhere. [G] per target x phase [F] I1 ids
   [M] hit-inject + observed effect.
B3 Focus/layering/modality stacks. [G] per legal stack [F] routed-input
   trace [M] stack enumeration + key/pointer routing per stack.
B4 Input methods (paste forms/clipboard/IME/kitty). [G] per form x size
   [F] real buffers [M] inject + observe.
B5 Terminal events (resize/focus/suspend) per surface. [G] per surface x
   event x mid-state [F] capture [M] inject + state assertion.
B6 Terminal matrix (TERM x truecolor x kitty x tmux; owns TM-1..10).
   [G] per profile x surface [F] I10 pin [M] protocol-signature + capture.
B7 Platform/env matrix (OS paths/shell/locale/timezone). [G] per env var
   class [F] real envs where available [M] behavior diff per class.
B8 **Width implementation** (NEW): unicode-width version, ambiguous-width
   table, VS16/ZWJ at OUR version vs ref's — cursor-desync class.
   [G] per codepoint class [F] version-pinned tables [M] corpus sweep +
   version diff vs ref.

## C — Behavior / State Machines
C1 Session lifecycle transition table. [G] materialized cell per
   state x event [F] I1 [M] table driven against both ends.
C2 Navigation graphs (tabs/picker/family/session.child). [G] per edge
   [F] real child ids [M] graph walk asserts.
C3 Permission+attention (expiry/queue/multi-ask). [G] per flow x timing
   [F] I3 [M] scripted flows end-to-end.
C4 Config leaf-key round-trips (write->restart->read). [G] PER LEAF KEY
   [F] I9 [M] round-trip assert each.
C5 Error/recovery paths. [G] per failure x mid-state [F] injected failures
   [M] recovery assertion.
C6 Plugin lifecycle x UI. [G] per phase x surface [F] I2 [M] lifecycle
   drive + capture.
C7 Background jobs surface. [G] per job state x surface [F] real job
   [M] observe.
C8 Worktree family. [G] per op x git state [F] real repos [M] op + UI
   + fs assert.
C9 MCP family. [G] per server state x surface [F] real MCP server
   [M] flow assert.
C10 Provider auth family. [G] per flow x status x reconnect [F] mock
   IdP + I9 [M] flow + UI + credential store assert.
C11 Update/upgrade family. [G] per phase x channel [F] staged update
   [M] UX + fs assert.
C12 Onboarding/migration first-runs. [G] per first-run shape [F] synthetic
   v2/mixed homes [M] observed boot + carried state.
C13 **Skills family** (NEW): core skill registry + dialog-skill + slash
   skills. [G] per skill surface [F] real skill dir [M] enumeration +
   UI assert.
C14 **Instructions/commands/agents/prompts discovery** (NEW): AGENTS.md/
   commands/agents dirs, prompt templates. [G] per source x precedence
   [F] real project shapes [M] precedence assert.
C15 **Composer context family** (NEW): @-mentions, file/dir completion,
   frecency, prompt stash/history UI. [G] per trigger x state [F] real
   project tree [M] trigger + completion assert.
C16 **Long-tail dialogs** (NEW): stash/experiments/review-mode/every
   DialogKind in the ref not yet axis'd. [G] per dialog x state [F] I1
   [M] enumerate from ref + assert each.

## D — Data / Communication
D1 SSE type x field x ordering x consumer x error-injection.
   [G] per type x field [F] I1 real flows + I2 [M] publish->consume trace.
D2 REST field-level incl. negative paths. [G] per endpoint x path
   [F] actual client calls [M] shape + status assert.
D3 Plugin-host boundary. [G] per API x real payload [F] I2 [M] boundary
   trace.
D4 Persistence writer-reader pairing x migration x cross-install.
   [G] per artifact [F] I9 [M] write->read->migrate assert.
D5 Self-consistency invariants. [G] per chain [F] I1 [M] end-to-end id
   trace.
D6 WebUI serving (assets/SPA/auth/PWA/app calls). [G] per path x auth
   state [F] served app [M] request matrix assert.
D7 ACP surface. [G] per method [F] real ACP client [M] session assert.
D8 Share/export paths. [G] per format x destination [F] real data
   [M] output assert.
D9 **Model/variant/provider matrix** (NEW): dialog-model/variant shapes,
   models-dev drift, provider x model availability folding.
   [G] per provider x model x variant [F] catalog + config variants
   [M] chain assert.

## E — Cross-cutting Quality
E1 Perf FEEL (latency/jank/pacing). [G] per interaction x load [F] metric
   captured (ms) [M] instrumented runs; stutter = finding.
E2 Startup TTI per surface. [G] per surface x data size [F] timed [M]
   threshold assert vs ref timing.
E3 Memory growth (soak). [G] hours x session shape [F] RSS/handles
   sampled [M] slope assert.
E4 Huge-data. [G] per family x magnitude [F] synthetic-big + I9 [M]
   behavior + time.
E5 Accessibility (contrast/colorblind/keyboard-only). [G] per theme x
   element [F] computed contrast [M] ratio assert + key-only walk.
E6 i18n/locale (width/truncation/ordering/formats; LANG=C, ru_RU.KOI8-R
   classes). [G] per locale class [F] real locale env [M] behavior diff.
E7 Error-message quality. [G] per error path [F] real messages [M]
   comprehensibility + actionable check.
E8 Security posture (prompts/secrets/paths). [G] per surface [F] attack
   shapes [M] posture assert.
E9 Teardown/cleanup. [G] per exit path [F] process/file/terminal state
   [M] post-exit assert.

## F — Adversarial / Meta
F1 Materialized command x route x state matrix. [G] every cell owned
   [F] I8 canary on the matrix [M] cell-by-cell disposition.
F2 Bottom-up ref enumeration. [G] every export [F] I10 [M] account per
   export.
F3 Concurrency (two clients one server, races, reconnect). [G] per
   scenario [F] real second client [M] interleaving assert.
F4 Network degradation (slow/timeout/offline/retry). [G] per degradation
   x surface [F] shaped link [M] semantics assert.
F5 Soak with metrics. [G] per soak profile [F] E3 metrics [M] report.
F6 Fresh-eyes + miss ledger + **cross-axis interaction review** (NEW:
   pairs of axes whose interaction hides bugs — temporal x data, plugin x
   permission, resize x persistence...). [G] per axis-pair on the round's
   findings [F] I6 [M] interaction walk.
F7 **Negative-space parity** (NEW): enumerate where OURS does MORE than
   ref (extra writes/events/calls/defaults) — divergence is bidirectional.
   [G] per surface x action [F] side-by-side traces [M] ours-minus-ref
   diff.

## Round log
- v3: SPEC-R2 integrated (30 gaps; keystone [G][F][M] everywhere; A8/B8/
  C13-C16/D9/F6-interaction/F7-negative-space new; I8-I11). NEXT: SPEC-R3
  (P1 feature-space focus, fresh eyes on v3).
