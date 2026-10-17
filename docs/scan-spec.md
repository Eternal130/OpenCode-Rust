# The Scan Specification (扫描细则) — LIVING DOCUMENT

Version: **v4** (SPEC-R4 verbatim-integration round). Protocol: docs/scan-program.md.
Lineage: v1 (R5 framework) -> SPEC-R1 (52 gaps; taxonomy-v2 [G][F][M]) -> v2 ->
SPEC-R2 (30 gaps; [G][F][M] keystone; I8-I11) -> v3 -> SPEC-R3 (12 unowned
families U1-U12 + 8 fold-amendments + clause rewrites) -> **THIS v4**.

**Integration rule (binding, per I11 — this round's correction):** clause text is
carried VERBATIM from the round documents — R1 taxonomy-v2 rows, R2 §c rewrites,
R3 §5 proposals. Where v3 compressed or dropped a round's text, the fuller round
text is RESTORED, not summarized. Every clause carries **[G]ranularity /
[F]idelity / [M]ethod** — an agent satisfying the letter must also satisfy the
spirit — plus its source gap IDs `{…}` (R2-M-4). The appendix maps ALL 102 gap
IDs (R1 52 · R2 30 · R3 12+8) to clauses/folds/justifications.

## Standing instrument layer (binds EVERY clause)
I1 REAL payloads end-to-end — no hand-set ids; chains from our own
   server/plugin-host outward (self-consistency). {R1: S4-2, S4-5}
I2 REAL plugin sandbox; **≥3 ecosystems + omo-slim first (restored)**, incl.
   marketplace fetch; ref+plugin = ground truth for plugin surfaces.
   {R1: S3-4, S4-3; R2: (a)S3-4, (a)S1-11}
I3 Virtual clock for **ALL timed behavior** (cadence = drive source), not
   animation only: toast pause-on-activity, reconnect backoff (ref 1s/5s),
   leader/chord timeouts, StartupLoading 500ms/3s gates, debounces,
   update-check interval, attention bell timing. {R1: S4-4; R2: (a)S4-4}
I4 Token-exact cells per element x state.
I5 Falsifiability: every clause names its evidence artifact. Standing in-repo
   instruments are BOUND as evidence artifacts (rounds must not rebuild ad-hoc):
   `golden_tests.rs`, `frame_budget_tests.rs`, `replay.rs`, `mouse_modes.rs`,
   `diff_viewer_golden_tests.rs`, ref `packages/simulation` tapes.
   {R1: S4-7; R2: S4-6, S4-7}
I6 Miss-attribution ledger per finding (inventory-grade / fixture-happy /
   adjudication-masking). {R1: S4-6}
I7 Bottom-up ref enumeration as feature completeness backstop, **quantified
   against the I10b package universe**. {R3: U12}
I8 **Falsifiability canary — TIERED** (v4, per R3 §3/§5): per round one
   random-axis canary + per-axis canary BEFORE that axis's first evidence is
   accepted + re-canary on instrument change; canary corpus committed as a
   standing artifact under I5. Concrete executable designs (R3 §3): A1
   token-cell swap, B8 width-table perturbation, C4 write-path value drop,
   D1 event reorder + optional-field drop, F7 extra-fs-write + extra-network-call
   injection. An axis whose method can't catch the canary is redesign on the
   spot. {R1: S4-7; R2: S4-2; R3: §3}
I9 **Provenance ledger**: every fixture/capture records its source (real-run
   capture vs synthetic); synthetic data is never sole evidence.
   {R1: S4-2; R2: S4-3}
I10 **Ground-truth pin**: ref commit pinned per round; re-baseline on ref
    release; ref+plugin pin recorded with the report. {R2: S4-4}
I10b **Enumeration-universe pin** (R3 U12): every round's report lists (a)
    in-scope packages and (b) out-of-scope packages with a one-line
    justification each; F2's "every export" is quantified against that list.
    Pinned universe (SPEC-R3 sweep; 35 packages under
    `/opt/opencode-ref/packages/`, verified 2026-10-17): exhaustive per-src-tree
    {core, tui, cli, session-ui, schema, plugin}; family-granular {ai app client
    codemode console containers desktop effect-drizzle-sqlite enterprise
    function gui-extensions httpapi-codegen http-recorder identity latex merman
    posts protocol script sdk server simulation stats storybook theme ui util
    web}. Standing out-of-scope-with-justification entries: **stats** (U8 — we
    ship no stats surface; absent-justified accounting entry under F2/I7 + one
    C-cell if/when shipped); **identity/enterprise** (U9 — not shipped by us;
    `core/src/managed-policy.ts` still binds C4 precedence). Standing
    accounting entries, no parity surface (R4 re-sweep): **console** (scaffold,
    src empty — F2 export accounting only), **posts** (static marketing/docs
    site — no runtime parity surface), **script** (single-file dev script — F2
    accounting), `tui/src/devtools` (dev-only route — owned generically by
    F1's route matrix). {R3: U8, U9, U12; R4: re-sweep}
I11 **Process + output contract**: ≤45-min chunks, no backgrounded scans,
    worktree isolation, per-round output format (gap register + adequacy table +
    convergence estimate), no summary-only dispatch — integrations quote clause
    text verbatim and carry gap-ID provenance; this v4 file is the corrected
    instance after three lossy integrations. {R2: S4-5}

## A — Visual / Rendering
A1 Theme tokens x element x STATE matrix — state set = union {base, hover,
   focus, busy, error, disabled, selected, expanded, interrupted, anim-phase};
   numeric edge sweep. [G] per token x element-cell x state [F] I4 token-exact
   cell capture; ref+plugin pair for plugin-painted cells [M] cell-diff harness
   vs ref frames incl. state injectors; ref token->element enumeration + vt
   capture diff. {R1: S5-1; R2: (a)S5-1}
A2 Geometry (padding/gap/centering/truncation/wrap). [G] per surface x size
   {40,60,80,120,160} + narrow/wide sweep [F] vt grid golden-frame capture
   [M] golden-frame diff at width sweep; rule-by-rule vs ref.
A3 Edge + huge data — **R2 §c rewrite, verbatim**: [G] widths {0,1,2,10,60,80,
   120,240} x msgs {0,1,2,10,1k,10k} x full part-kind enum (materialized from
   `oc-schema`, not prose) x wide-char set {pure CJK, mixed, combining, ZWJ
   family, VS16, ambiguous} x overflow mode {truncate, wrap, scroll} per
   surface. [F] I1+I4. [M] golden-frame corpus persisted in-repo (bind
   `golden_tests.rs`), cell table checked into the round artifact.
   {R1: S2-4, S5-2; R2: S5-1, (a)S5-2, (a)S3-5}
A4 Temporal at vt-clock: [G] per-frame content @ every animated element
   (spinners, pulse, shimmer, marquee, fade, scrim alpha) x phase [F] I3 clock +
   frame stepping + drive-source audit [M] clock-stepped frame series capture.
   {R1: S4-4}
A5 Plugin-rendered surfaces — incl. **internal feature-plugin SLOT
   architecture** (`feature-plugins/{home,prompt,sidebar,system}` — internal
   slots are surfaces; R3 fold 5). [G] every plugin API render slot x state
   [F] I2 ref+plugin pair (≥3 ecosystems + omo-slim) [M] side-by-side capture
   diff. {R1: S1-11, S3-4; R2: (a)S1-11; R3: FOLD-5}
A6 MINI frontend — **R2 §c rewrite, verbatim**: [G] panel enum from ref
   `mini/` (footer.{command,form,menu,permission,prompt,subagent,view},
   scrollback.{writer,surface}, splash, stream-v2.{fragment,image,subagent}) x
   state x width; fragment corpus = real LLM chunk boundaries. [F] I1+I3.
   [M] mini PTY capture diff vs mini spec (mini/theme.ts quantizer rules).
   {R1: S1-5; R2: (a)S1-5}
A7 Theme-system chain: [G] discovery -> select -> apply -> live-preview ->
   live-retheme -> error chain (ThemeErrorToast) x step; user-theme-dir
   discovery; system detect (OSC 11); oklab quantization; syntax themes.
   [F] I4 + real user theme dirs [M] apply-path trace + full-chain round-trip
   + capture. {R1: S1-13; R2: (a)S1-13}
A8 **Rich content** (v4 rewrite per R3 §2/§5 + R2 §c restored): [G] per
   content family — markdown, code-syntax, mermaid/merman (**sub-families:
   flowchart/gantt/gitgraph/sequence**), math/latex (**inline vs block**),
   diff, part-kind, `thinking-syntax.ts` (R3 fold 3) — x size x
   **syntax-theme (theme x renderer interaction)**; **absent-justified
   accounting per renderer WE lack** (mermaid/latex today: ref ships
   merman+latex packages; ours has neither). [F] I1 + **I9 corpus
   provenance** [M] renderer corpus diff — rendered frames, not parse trees;
   render parity per family. {R2: S1-4, (a)S1-4; R3: §2-A8, FOLD-3}
A9 **Images** (RESTORED v4 — R2's A9/R3's U1; v3 had zero "image" hits):
   [G] {paste, attach, tool-output, gen, preview-zoom, retained cache,
   diff-viewer-image, mini stream-v2.image} x format {png,jpg,webp,gif} x size
   class. [F] I1+I4 [M] capture diff per cell; ours-ships cells vs ref parity,
   absent-justified per missing. {R1: S1-9; R2: (a)S1-9, §c-A9; R3: U1}

## B — Interaction / Input / Environment
B1 Keyboard: [G] binding x route x mode x layer x config-variant (incl. user
   keybind overrides; chord/leader timeout values as observable outcomes) x
   state [F] I1 real chain + user keymap files [M] dispatched-action trace
   (end-behavior table), not table presence. {R1: S1-12, S5-3}
B2 Pointer: [G] target x state x shape x **lifetime-phase** (hit-rect validity
   across route/mode change, dialog open/close, busy transitions); drag/hold/
   middle/right; wheel in every scrollable; **selection copy — clipboard
   round-trip of RENDERED content (wide chars, emoji, box-drawing, masked
   text)** [F] I1 real metadata ids + plugin rows [M] hit-inject + observed
   effect + rect-lifetime trace; would catch copy that concatenates cells
   without wide-char reconstruction. {R1: S5-4; R2: S2-5}
B3 Focus/layering/modality stacks. [G] per legal stack [F] routed-input trace
   [M] stack enumeration + key/pointer routing per stack.
B4 Input methods: [G] per paste form (bracketed/image/large/WHATWG), IME
   composition, clipboard read/write (OSC 52) x size (incl. 100MB) [F] real
   buffers/paste payloads [M] inject + observe; byte-level paste/clip traces.
B5 Terminal events (resize/focus/suspend/$EDITOR round-trip/mouse-mode
   transitions) per surface. [G] per surface x event x mid-state [F] capture
   [M] PTY event scripts; inject + state assertion.
B6 Terminal matrix — **R2 §c rewrite, verbatim**: [G] rows {xterm, kitty,
   ghostty/wezterm-class, tmux x {1,2 nested}, screen, no-TERM, 16-color, ssh}
   x cols {kitty-keys, mouse modes, OSC 10/11/12/52/777, DECSCUSR, title,
   selection, color-count fallback, undercurl, synchronized-output}; TM-1..10
   cells absorbed with signed/unsigned status. [F] real emulators (tmux live)
   + I10 pin [M] protocol-signature probe script + behavior capture per cell.
   {R1: S3-1; R2: S5-1, (a)S3-1}
B7 Platform/env matrix — **R2 §c rewrite, verbatim + R3 folds**: [G] OS
   {linux, macOS, windows/ConPTY (mouse/resize/VT semantics, powershell
   profiles, CRLF in tool output, path separators, sound backend absence —
   each a distinct cell)} x env cells {unset TERM, no HOME, no PATH, no git/rg,
   read-only HOME, LANG x {C, en_US.UTF-8, non-UTF-8 (ru_RU.KOI8-R class)},
   TZ absent, OPENCODE_* incl. CLI_CONFIG_CONTENT, SSH_TTY, CI vars, docker} x
   per-OS {$EDITOR default, sound/notify backend, updater path} + named cells:
   `ssh-askpass.ts` (fold 4), `editor-zed*` (fold 7), `attention-sounds.{bun,
   node}` backend split (fold 8). [F] clean envs; real envs where available
   [M] boot+smoke matrix script, cell table artifact; behavior diff per class.
   {R1: S3-2, S3-3; R2: S3-4, S3-5, (a)S3-2, (a)S3-3; R3: FOLD-4, FOLD-7, FOLD-8}
B8 **Width implementation** (v4 rewrite per R3 §2): [G] per codepoint class
   {pure CJK, mixed, combining, ZWJ family, VS16, East-Asian Ambiguous} at OUR
   unicode-width version vs ref's width calculation; grapheme clustering;
   user-font cell-metric dependence. **Pass criterion: cursor-cell position vs
   ref on the same string, per class** (a version-string diff is a fail);
   corpus checked in; surface binding NAMED (wrap engine, input echo, diff
   viewer, mini). [F] I10 version-pinned tables [M] corpus sweep +
   cursor-desync diff vs ref; I8 canary = width-table perturbation must desync
   exactly the perturbed classes. {R2: S3-3; R3: §2-B8}
B9 **Remote/hosting matrix** (RESTORED v4 — R2's §c-B8 text; v3 reused the B8
   number for width and dropped this; deep cell pass owned by P2/P3): [G]
   {ssh, mosh, tmux-over-ssh, container, CI, dumb-tty} x {echo latency under
   100ms+ RTT, redraw, TERM negotiation, missing binaries, read-only HOME,
   no /dev/pts, CI env vars changing behavior}. [F] shaped link / real remote
   env where available + I9 [M] degraded-env boot + echo matrix; cell table
   artifact. Would catch echo-lag and redraw-storm classes invisible on a
   local pty; boot failure in devcontainer. {R2: S3-1, S3-2, §c-B8; R3: §5-nonP1}

## C — Behavior / State Machines
C1 Session lifecycle: [G] materialized from x to x trigger x UI-effect
   transition table (states enumerated from ref code, not prose) + named
   cells: session `fork` (`routes/session/dialog-fork.tsx`; ours: oc-server
   fork), `anchors`/`anchor-view`, `grouping/{session,tree}`,
   `message-navigation` (R3 fold 1) [F] I1 real child ids [M] table driven
   against both ends; transition-probe scripts. {R1: S5-5; R3: FOLD-1}
C2 Navigation graphs: [G] tabs/picker/family/session.child full graph incl.
   unreachable-node detection + grouping/tree navigation cells (R3 fold 1)
   [F] real child ids [M] graph walk asserts. {R3: FOLD-1}
C3 Permission+attention — **R2 §c rewrite, verbatim**: [G] decision {allow/
   deny/always} x tool x path-scope x persistence {session, forever, expiry} x
   queueing; attention: emission {bell OSC 777, OSC 9, in-process audio,
   system notify} x gating config. [F] I1+I3+real grant stores [M] flow
   scripts + emission capture trace, scripted flows end-to-end.
   {R1: S1-8, S5-6; R2: (a)S5-6}
C4 Config leaf-key round-trips: [G] PER LEAF KEY read x write x restart x
   cross-install visibility; **JSONC comment preservation**; env overlays
   (OPENCODE_* incl. CLI_CONFIG_CONTENT); `managed-policy.ts` precedence
   interaction (U9). [F] I9 real config dirs (v2 shape, jsonc, native
   providers) [M] boot -> toggle -> restart -> v2-read round-trip assert each
   leaf. {R1: S1-12, S5-7, S3-5; R3: U9}
C5 Error/recovery paths: [G] per failure {server loss, plugin crash, partial
   write, bad data} x mid-state + workspace-file-changes review flow cell
   (`dialog-workspace-file-changes` + file-retention/file-mutation, R3 fold 6)
   [F] injected failures [M] kill/fuzz scripts + recovery assertion.
   {R3: FOLD-6}
C6 Plugin lifecycle x UI: [G] install/enable/disable/update/fail x UI x
   storage/RPC (PluginStorage, NoopRpcPort) x phase; feature-plugin SLOT
   architecture slots are surfaces (R3 fold 5). [F] I2 real plugins, real
   payloads [M] lifecycle drive + capture. {R1: S1-11; R2: (a)S1-11; R3: FOLD-5}
C7 Background jobs surface. [G] per job state x surface [F] long-running real
   job [M] async replay + observe.
C8 Worktree family — **R2 §c rewrite, verbatim**: [G] {create, adopt, refresh,
   move, delete, rename} x repo state {clean, dirty, detached, bare,
   conflicted, no-git} x {git-side assertion, UI cell, session_move tool}.
   [F] I1 real repos [M] git-state assertion script per cell; op + UI + fs
   assert. {R1: S1-1; R2: (a)S1-1}
C9 MCP family — **R2 §c rewrite, verbatim**: [G] {add, connect, auth, fail,
   restart, remove} x {dialog cells, toasts, home row, CLI} x tool invocation
   + needs-auth + elicitation-blocked. [F] I1 real MCP servers [M] live
   round-trip trace. {R1: S1-2; R2: (a)S1-2}
C10 Provider auth family: [G] every OAuth/device/token flow x storage x
   refresh x UI (tokens sidebar, /connect, pair + QR) [F] real provider
   sandbox creds (mock IdP + I9) [M] flow capture + credential store assert.
   {R1: S1-3; R2: (a)S1-3}
C11 Update/upgrade family — **R2 §c rewrite, verbatim**: [G] check -> notify
   -> install -> restart x method {curl, npm, pnpm, bun, yarn, vp, brew} x
   {install prefix, PATH precedence, downgrade, cache migration} + uninstall;
   update-notice cadence under I3. [F] real updater paths (staged update)
   [M] upgrade round-trip script per method; UX + fs assert.
   {R1: S1-4, S2-9; R2: (a)S1-4}
C12 Onboarding/migration first-runs: [G] per first-run shape {fresh, migrated,
   mixed v2+rust install} — first-run model/provider seeding, StartupLoading
   500ms/3s gates (I3), MigrationOverlay, getting-started dismiss,
   location-missing recovery, downgrade-after-upgrade [F] synthetic v2/mixed
   homes + I9 [M] fresh-install + migrated-install scripts; observed boot +
   carried state. {R1: S1-14, S2-9, S3-7; R2: (a)S1-14}
C13 **Skills family** (v4 rewrite per R3 §2; R2 matrix restored): [G] sources
   {directory, url, embedded} x phase {discovery, permission, invocation} x UI
   (dialog-skill); embedded-vs-directory precedence; invocation-through-agent
   and URL-fetch failure paths. [F] real skill dir [M] enumeration + UI assert
   + failure-path probes. {R2: S1-1, (a)C13; R3: §2-C13}
C14 **Instructions/commands/agents/prompts discovery** (v4 rewrite; R2 proposed
   these as TWO clauses — both texts carried):
   (a) Instruction discovery: [G] AGENTS.md hierarchy (global -> project ->
   subdir) x precedence x dedup x injection order x builtins merge [F] real
   project shapes + I9 [M] **artifact = materialized final system-prompt
   diff**. {R2: S1-3, §c-C15}
   (b) Commands/agents/prompts: [G] source enum {command, agent, prompt} x
   {project, global, user, embedded} x **frontmatter-field validation
   (unknown field silently ignored = cell)** x collision x precedence x
   agent-mode binding x catalog/frecency UI [F] real `.opencode/{command,
   agent}/*.md` shapes [M] precedence assert + frontmatter validation.
   {R2: S1-2, §c-C14; R3: §2-C14}
C15 **Composer context family** (v4 rewrite per R3 §2): [G] per trigger
   {mention grammar, directory-completion, recents} x state + **payload-
   resolution cells (mention -> payload path; insert format)** + context chips
   + frecency/history/stash (IN [G], not prose) + composer tabs {shell,
   subagents, terminals} (R3 fold 2). [F] I1 real project tree [M] trigger +
   completion assert + payload-resolved effect; would catch @-mention
   resolving to the wrong path payload with a correct-looking chip.
   {R2: S1-5, (a)C15; R3: §2-C15, FOLD-2}
C16 **Long-tail dialogs** (v4 rewrite per R3 §2): [G] enumeration = filesystem
   union of THREE trees (`tui/src/component/dialog-*`, `tui/src/routes/
   session/dialog-*`, `session-ui`) — no single DialogKind symbol exists; per
   dialog x state with **ACTION-ASSERT (stash must actually stash, rename
   actually rename)**; named: dialog-stash, dialog-experiments, dialog-debug,
   dialog-status, dialog-shell-output, retry-provider, masked-text,
   command-palette, session-tabs-rail, pierre review-mode family
   (commented-lines, comment-hover, diff-selection, file-find); +
   workspace-file-changes review flow (R3 fold 6). [F] I1 [M] enumerate from
   ref trees + assert each dialog's action. {R2: S1-8, (a)C16; R3: §2-C16, FOLD-6}
C17 **Terminal / persistent-PTY family** (U3, R3 §5 verbatim): [G] {spawn,
   resize, scroll, exit-status, handoff, daemon-restart} x {terminals-tab,
   terminal-pane, shell-output dialog, pty handlers} x TTY state. [F] I1 real
   ptys [M] session drive + capture + exit-status assert. {R3: U3}
C18 **CLI/service/run family** (U4+U5, R3 §5 verbatim): [G] every
   `cli/src/commands/handlers/*` command x args-shape x output {tty, non-tty}
   x exit code; service lifecycle {start, stop, restart, status, get, set,
   unset, registration, standalone}; `run` {interactive, noninteractive, v1} x
   stream format (headless exec: stream format, exit codes, scriptability).
   [F] I1+I10 [M] executed-command matrix artifact (every cell dispositioned).
   {R3: U4, U5}
C19 **Integrations family** (U6, R3 §5 verbatim): [G] per integration
   provider x {connect, disconnect, auth-state} x {dialog, picker, handlers}.
   [F] I1 [M] flow assert + credential store check. {R3: U6}
C20 **Built-in tool family** (U7, R3 §5 verbatim): [G] per built-in tool x
   arg-shape x output-kind x renderer x permission class x side-effect (named:
   read-filesystem, mcp, html-markdown, http-body, runtime, plugin tools,
   websearch, file-mutation, file-access, file-retention, vcs/patch).
   [F] I1+I2 [M] schema+behavior+render parity vs ref per tool. {R3: U7}
C21 **Determinism/idempotency** (RESTORED v4 — R2's §c-C17 / R1 S2-13 / U11;
   v3 had zero "determin" hits): [G] rapid/duplicate input x double-submit
   guards x idempotent re-run x event-order stability under repetition.
   [F] I3 vt-clock driven [M] repetition scripts + order-stability assert;
   would catch queue double-fire on rapid Enter; duplicate session titles
   racing regeneration. {R1: S2-13; R2: S2-13, §c-C17; R3: U11}

## D — Data / Communication
D1 SSE: [G] type x field x consumer {TUI, webui} x flow x error-injection
   (publisher error mid-stream) + ordering guarantees per consumer, under
   plugin activity. [F] I1 real flows + I2 [M] subscription taps + publish->
   consume trace + field diff; I8 canary = reorder + optional-field drop.
   {R1: S5-8}
D2 REST: [G] endpoint x consumer x happy path x every documented error code x
   malformed input; **request-side field parity (what clients SEND)**.
   [F] actual client calls, live traces [M] proxy capture + shape/status
   assert. {R1: S5-9}
D3 Plugin-host boundary: [G] per API surface x real payload (omo-slim + ≥2
   other ecosystems) [F] I2 [M] host-adapter taps, boundary trace.
   {R1: S3-4}
D4 Persistence: [G] per table/column/row-kind writer-reader pair (**pairing
   bookkeeping explicit — unpaired writers/readers must be listable**); per
   state-file key; CELL-level migration diffs; **longitudinal shapes** (state
   written by N-1 read by N, months-old rows, v1-era remnants, long-use
   session trees). [F] I9 real DB snapshots, mixed installs [M] writer/reader
   pairing lint + migration A/B + write -> read -> migrate assert.
   {R1: S5-10, S3-5; R2: M-2, (a)S5-10}
D5 Self-consistency invariants: [G] every cross-layer id/metadata chain
   (sessionID class generalized) [F] I1 [M] invariant checkers, end-to-end id
   trace.
D6 WebUI serving — **R2 §c rewrite, verbatim + U10 amendment**: [G] {asset
   hosting, SPA fallback, browser auth/pair redirect, version gating
   (incompatible-server-panel), handoff, SDK/client package shape} x real
   browser session; PLUS desktop shell (`packages/desktop/{main,preload,
   renderer,shared}`: native chrome, protocol handlers, desktop-side update)
   and app package (`packages/app`: composer/home/new-session/desktop) either
   in the served-surface enumeration or **absent-justified** (U10).
   [F] I1 [M] served-app e2e + proxy capture of the app's actual calls;
   request matrix assert. {R1: S1-7, S3-7; R2: (a)S1-7; R3: U10}
D7 ACP surface: [G] session/tool/**elicitation translation** granularity x
   lifecycle (per method) [F] real ACP client [M] adapter interop, session
   assert. {R1: S1-6; R2: (a)S1-6}
D8 Share/export paths: [G] publish/unpublish lifecycle x export options/
   result dialogs x CLI export formats x fork-with-export x format x
   destination [F] real data/artifacts [M] round-trip diff + output assert.
   {R1: S1-10; R2: (a)S1-10}
D9 **Model/variant/provider matrix** (v3's number KEPT; v4 rewrite per R3
   §2/§5): [G] catalog resolution x provider shapes (multi-provider configs,
   custom/local providers) x variants {effort, toggle, budget_tokens} x
   preference persistence x models-dev drift x capability flags (incl.
   supportsText/image) x dialog-model/dialog-variant UI cells — IN [G], not
   prose; **folding rule: providers grouped into classes, ≥2 providers per
   class exercised** (the unbounded hundreds-cell matrix folds); models-dev
   pinned via I10. [F] catalog + config variants + I10 [M] chain assert
   (config -> catalog -> dialog -> request). {R1: S3-6; R2: S1-7, (a)D9; R3: §2-D9}
D10 **Code Mode + plugin-browser** (RESTORED v4 — R2's §c-D9 / U2; v3 reused
   the D9 number for model/variant and dropped the family): [G]
   interpreter/stdlib/openapi/tool-runtime/tool-schema surface x parity;
   browser tunnel/proxy/connection/rpc/files round-trips. [F] I2 [M] per-API
   trace (stdlib divergence is invisible to D1/D2 — not server-exposed).
   {R2: S1-6, §c-D9; R3: U2}
D11 **SDK/client package** (RESTORED v4 — R2's §c-D10, silently vanished in
   v3; R3 left the fold decision to R4 -> CLAUSE): [G] public SDK + client
   shapes the app/SDK consumers rely on x consumer {ref app, external} —
   versioned [F] I10 versioned pins [M] shape-compat assert per release
   (breaking-change diff). {R2: §c-D10; R3: §5-nonP1}

## E — Cross-cutting Quality
E1 Perf FEEL: [G] per interaction x load; **metric = frame-interval histogram
   at vt-clock; stall-burst = >2-frame deficit detector** [F] metric captured
   (ms), instrumented runs [M] stutter = finding; histogram artifact.
   {R1: S2-1; R2: S2-1}
E2 Startup TTI: [G] boot -> first-interactive per surface x **config variant
   (plugins on/off)** x data size; **threshold = ref StartupLoading 500ms/3s
   gates** [F] timed [M] threshold assert vs ref timing. {R1: S2-2; R2: (a)S2-2, S2-1}
E3 Memory growth (soak): [G] **RSS slope MB/h + span count over ≥4h soak** x
   session shape; periodic sampling [F] RSS/handles sampled [M] slope assert +
   sampling artifact. {R1: S2-3; R2: (a)S2-3, S2-1}
E4 Huge-data: [G] per family x magnitude; **p95 first-paint latency at 10k
   msgs** [F] synthetic-big + I9 [M] behavior + time. {R1: S2-4; R2: S2-1}
E5 Accessibility: [G] contrast per theme x state x element; keyboard-only path
   for every pointer target (~120 auditA targets); **color-independence of
   busy/error states** [F] computed contrast [M] ratio assert + key-only walk.
   {R1: S2-5; R2: (a)S2-5}
E6 i18n/locale: [G] per locale class (TZ absent, non-UTF-8 ru_RU.KOI8-R,
   LANG=C); width/truncation/ordering/formats (local-tz `toLocaleString` vs
   fixed UTC) [F] real locale env [M] behavior diff. {R1: S2-6, S3-5; R2: (a)S2-6, S3-5}
E7 Error-message quality: [G] per error path; exit-code semantics; toast
   action labels; dialog copy variants [F] real messages [M] **copy-diff
   method** + comprehensibility/actionable check; "Not wired in this build"
   placeholders reachable in normal flows = finding. {R1: S2-10; R2: (a)S2-10}
E8 Security posture — **R2 §c rewrite, verbatim**: [G] route x auth table
   incl. static assets; pair-code entropy/expiry; credential file perms;
   traversal corpus; redaction canaries (planted secrets in logs/share/
   export); SSRF targets (fetch/websearch). [F] adversarial payloads [M]
   pen-style probe script + findings file. {R1: S2-11; R2: S5-2, (a)S2-11}
E9 Teardown/cleanup: [G] per exit path x {children, PTYs, temp files, locks,
   terminal restore}; **kill -9 recovery + panic injection**; stale locks.
   [F] kill -9/panic injection fidelity [M] post-exit system-state audit.
   {R1: S2-12; R2: (a)S2-12}
E10 **Storage/IO growth** (RESTORED v4 — R2's §c-E10; deep metric pass owned
   by P2): [G] DB size, log growth, state-file churn, write amplification x
   hours over soak [F] E3-class periodic sampling [M] growth-slope artifact;
   would catch unbounded event-log growth — a disk-full field failure no other
   clause can express. {R2: S2-2, §c-E10}
E11 **Idle CPU/energy** (RESTORED v4 — R2's §c-E11; deep metric pass owned by
   P2): [G] event-poll loops, animations while idle, wakeups per minute at
   rest [F] idle-state sampling [M] wakeup-count artifact; would catch
   spin-loop battery drain that passes every functional axis.
   {R2: S2-3, §c-E11}
E12 **Search/filter family** (NEW v4 — R2's S2-4, silently dropped by v3 and
   missed by R3's register; caught in R4's zero-loss check): [G] per surface
   {session-list filter, transcript/message search} x query class {ascii,
   case variants, combining chars, CJK, fuzzy} [F] real corpus incl.
   combining chars + I9 [M] result-parity assert vs ref + normalization
   audit; would catch search that misses combining chars / case variants.
   {R2: S2-4}

## F — Adversarial / Meta
F1 Materialized command x route x state matrix. [G] every cell owned;
   exercised/unexercised bookkeeping persisted round-to-round; **per-axis
   coverage % (cells exercised / cells defined) reported per round** [F] I8
   canary on the matrix [M] cell-by-cell disposition; matrix runner + ledger
   artifact. {R1: S5-11; R2: M-4}
F2 Bottom-up ref enumeration. [G] every export in the **I10b-pinned package
   universe** accounted (ported/absorbed/absent-justified), incl. packages
   beyond tui/core; U8 stats = standing absent-justified entry [F] I10+I10b
   [M] export-diff tool; account per export. {R2: M-4; R3: U8, U12}
F3 Concurrency: [G] {TUI+TUI, TUI+webui} x same-session double-drive, settings
   race (flock under two clients), SSE per-client isolation, concurrent
   session create/delete, event cross-talk [F] two live clients [M]
   dual-client scripts, interleaving assert. {R1: S2-7}
F4 Network degradation: [G] per degradation {slow stream, timeout, drop,
   offline} x surface x flows; reconnect backoff (ref 1s/5s) under real drops
   (I3) [F] shaped link (tc/proxy) [M] degraded-net replay; semantics assert.
   {R1: S2-8}
F5 Soak with metrics: [G] per soak profile, hours-long real session; collects
   E1-E3, E10, E11 metrics [F] real plugins/data/clock [M] standing soak
   report.
F6 **Fresh-eyes + miss ledger + cross-axis interaction review** (v4 rewrite
   per R3 §2; cures the circularity): [G] **risk-ranked standing axis-pair
   corpus** incl. pairs with NO prior single-axis finding (precisely the class
   F6 exists for); seed pairs {theme x B6 color-depth, plugin x flow,
   concurrency x persistence, resize x streaming, mini x B6, auth x D6}; one
   designated interaction-of-the-round [F] I6 + **concrete interleaved-
   execution probe per tested pair** [M] pair-selection rule + probe artifact
   (not a prose walk). {R2: M-1; R3: §2-F6}
F7 **Negative-space parity** (v4 rewrite per R3 §2): [G] per surface x action;
   **trace scope DEFINED: {events emitted, files written, sockets opened, env
   reads, network calls}; diff granularity = name + count + args** [F]
   side-by-side traces vs ref [M] ours-minus-ref diff + **disposition rule
   (each extra = finding or justified)**; I8 canary = injected extra write +
   extra call must both trip (count+args, not name-only). {R2: M-3; R3: §2-F7}

## Round log
- v1: R5 framework.
- v2: SPEC-R1 integrated (52 gaps; taxonomy-v2 [G][F][M] columns).
- v3: SPEC-R2 integrated (30 gaps; keystone [G][F][M] everywhere; A8/B8/
  C13-C16/D9/F6-interaction/F7-negative-space new; I8-I11). Integration was
  LOSSY: A9 images, C17-determinism, D10-SDK, B8-remote, E10/E11, E12-search
  dropped; several §c matrices re-compressed.
- v4: SPEC-R3 integrated **VERBATIM** (corrected integration per I11):
  restored all v3-dropped clauses (A9, C21-determinism, D10-codemode with the
  D9 number-reuse resolved — model/variant keeps D9, codemode gets D10 — D11-
  SDK, B9-remote, E10, E11, E12-search); new U-families C17-C20 + A9; D6/U10
  desktop amendment; I10b enumeration-universe pin (U12); I8 canary tiering;
  R2 §c rewrites re-applied verbatim (A3/A6/B6/B7/C3/C8/C9/C11/D6/E8); R3
  §2-driven rewrites of A8/B8/C13/C14/C15/C16/D9/F6/F7; 8 fold-amendments
  landed as named cells; traceability appendix added (102 gap IDs: R1 52,
  R2 30, R3 12+8). NEXT: SPEC-R5 (P2 quality-dimensions round; directives in
  docs/spec-round4.md).

---

## Appendix — Gap traceability index (zero-loss ledger)

Every gap ID ever emitted by a round maps to exactly one destination below.
Status legend: **C** = owning clause (text in this file) · **A** = amendment/
named cell folded into an existing clause · **OOC** = out-of-scope with
justification (I10b). Counts: R1 52/52 · R2 30/30 · R3 20/20 = **102/102**.

### R1 gaps (52)

| Gap | Destination | Status |
|---|---|---|
| S1-1 worktree | C8 (R2 §c rewrite) | C |
| S1-2 MCP | C9 (R2 §c rewrite) | C |
| S1-3 provider auth | C10 | C |
| S1-4 update/upgrade | C11 (R2 §c rewrite) | C |
| S1-5 mini runtime | A6 (R2 §c rewrite) | C |
| S1-6 ACP | D7 | C |
| S1-7 WebUI app | D6 (R2 §c rewrite + U10) | C |
| S1-8 sound/notify | C3 (emission semantics) | C |
| S1-9 images | **A9 (restored v4)** | C |
| S1-10 export/share | D8 | C |
| S1-11 plugin ecosystem | A5 + C6 + D3 + I2 | C |
| S1-12 keymap+config write | B1 + C4 | C |
| S1-13 theme system | A7 | C |
| S1-14 onboarding | C12 | C |
| S2-1 perf feel | E1 (metric+detector) | C |
| S2-2 startup TTI | E2 (gates+variants) | C |
| S2-3 memory growth | E3 (slope+cadence) | C |
| S2-4 huge data | E4 + A3 | C |
| S2-5 accessibility | E5 (color-independence) | C |
| S2-6 CJK/i18n | E6 (+ E12 search) | C |
| S2-7 concurrency | F3 | C |
| S2-8 network degradation | F4 | C |
| S2-9 upgrade/migration UX | C11 + C12 + D4 | C |
| S2-10 error quality | E7 | C |
| S2-11 security posture | E8 (R2 §c rewrite) | C |
| S2-12 teardown | E9 | C |
| S2-13 determinism | **C21 (restored v4)** | C |
| S3-1 terminal matrix | B6 (R2 §c rewrite) | C |
| S3-2 platform matrix | B7 (R2 §c rewrite) | C |
| S3-3 shell/env matrix | B7 + C4 | C |
| S3-4 plugin ecosystem matrix | I2 (≥3 ecosystems + marketplace) + A5 + D3 | C |
| S3-5 data-shape matrices | A3 + C4 + D4 + E6 (+ I9) | C |
| S3-6 provider/model matrix | D9 | C |
| S3-7 cross-version interop | C12 + D4 + D6 | C |
| S4-1 standing instrument | I-layer header | C |
| S4-2 real-payload provenance | I1 + I9 | C |
| S4-3 ref+plugin pairing | I2 | C |
| S4-4 temporal beyond animation | I3 (amended v4) + A4 + B1 | C |
| S4-5 fidelity all axes | I-layer + per-clause [G][F][M] | C |
| S4-6 miss-attribution ledger | I6 | C |
| S4-7 process + falsifiability | I5 + I8 + I11 | C |
| S5-1 token x cell x state | A1 (state union restored) | C |
| S5-2 numeric sweeps | A3 (R2 §c rewrite) | C |
| S5-3 binding x route x mode x layer x config | B1 | C |
| S5-4 target x state x shape x lifetime | B2 | C |
| S5-5 materialized transition table | C1 | C |
| S5-6 permission granularity | C3 | C |
| S5-7 config leaf granularity | C4 (JSONC + restart + cross-install) | C |
| S5-8 SSE field x flow x error | D1 | C |
| S5-9 REST negative paths | D2 | C |
| S5-10 persistence pairing | D4 (bookkeeping explicit) | C |
| S5-11 materialized matrix | F1 | C |

### R2 gaps (30)

| Gap | Destination | Status |
|---|---|---|
| S1-1 skills | C13 (matrix restored v4) | C |
| S1-2 commands/agents/prompts | C14(b) | C |
| S1-3 instruction discovery | C14(a) (system-prompt-diff artifact) | C |
| S1-4 rich content | A8 (rewritten v4) | C |
| S1-5 composer context | C15 (rewritten v4) | C |
| S1-6 Code Mode/plugin-browser | **D10 (restored v4)** | C |
| S1-7 model/variant UI | D9 (rewritten v4) | C |
| S1-8 long-tail dialogs | C16 (rewritten v4) | C |
| S2-1 E1-E4 unfalsifiable | E1/E2/E3/E4 (metric+threshold+artifact) | C |
| S2-2 storage/IO growth | **E10 (restored v4)** | C |
| S2-3 idle CPU/energy | **E11 (restored v4)** | C |
| S2-4 search/filter quality | **E12 (NEW v4 — v3 drop, R4-caught)** | C |
| S2-5 selection/copy | B2 (rendered-content round-trip) | A |
| S3-1 remote SSH/mosh | **B9 (restored v4)** | C |
| S3-2 container/CI | B9 | C |
| S3-3 width implementation | B8 (rewritten v4) | C |
| S3-4 Windows/ConPTY | B7 | C |
| S3-5 locale matrix | B7 + E6 | C |
| S4-1 [G][F][M] keystone | whole-spec format + clause ID tags | C |
| S4-2 falsifiability canary | I8 (tiered v4) | C |
| S4-3 fixture provenance | I9 | C |
| S4-4 ground-truth pinning | I10 | C |
| S4-5 output contract/no-summary | I11 (this v4 = corrected instance) | C |
| S4-6 in-repo instruments | I5 (bound v4) | A |
| S5-1 10/10 inventory-grade | R2 §c rewrites applied verbatim (A3 A6 B6 B7 C3 C8 C9 C11 D6 E8) | C |
| S5-2 E8 probe set | E8 | C |
| M-1 cross-axis interactions | F6 (rewritten v4) | C |
| M-2 longitudinal data-shape | D4 | A |
| M-3 negative-space parity | F7 (rewritten v4) | C |
| M-4 change-management + coverage | clause gap-ID tags + appendix + F1 coverage % | C |

### R3 register (12 families + 8 folds)

| ID | Family / fold | Destination | Status |
|---|---|---|---|
| U1 | images | **A9** (restored) | C |
| U2 | Code Mode + plugin-browser | **D10** (D9 number-reuse resolved) | C |
| U3 | terminal/persistent-PTY | **C17** | C |
| U4 | CLI command surface | **C18** | C |
| U5 | noninteractive/run mode | C18 (named cell-family) | A |
| U6 | integrations | **C19** | C |
| U7 | built-in tool family | **C20** | C |
| U8 | stats/telemetry | F2/I7 absent-justified entry + I10b OOC list | OOC |
| U9 | account/identity/enterprise | I10b OOC list + C4 managed-policy note | OOC |
| U10 | desktop + app frontends | D6 amendment (named or absent-justified) | A |
| U11 | determinism/idempotency | **C21** (restored) | C |
| U12 | enumeration-universe pin | **I10b** | C |
| FOLD-1 | fork/anchors/grouping/message-navigation | C1 + C2 named cells | A |
| FOLD-2 | composer tabs shell/subagents/terminals | C15 | A |
| FOLD-3 | thinking-syntax.ts | A8 family list | A |
| FOLD-4 | ssh-askpass.ts | B7 | A |
| FOLD-5 | feature-plugin slot architecture | C6 + A5 | A |
| FOLD-6 | file-retention/mutation + workspace-file-changes | C16 + C5 | A |
| FOLD-7 | editor-zed* | B7 | A |
| FOLD-8 | attention-sounds.{bun,node} split | B7 | A |

R3 §2 clause-stress verdicts (9 rows) are cured by the v4 rewrites of
A8/B8/C13/C14/C15/C16/D9/F6/F7 — each rewritten clause carries its R3 §2
provenance tag. R3 §3's five canary designs are bound under I8.
