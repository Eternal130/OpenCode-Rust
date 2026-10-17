# SPEC-R2 — Round-2 fresh-eyes review of the scan spec (v2)

**Lane:** SPEC-R2 (worktree `.worktrees/lane-spec2`, branch `lane/spec2`). **Date:** 2026-10-17.
**Input under audit:** `docs/scan-spec.md` **v2**, cross-referenced against `docs/spec-round1.md`
(the 52-gap register), `docs/scan-program.md` (protocol), and a fresh bottom-up enumeration of the
ref tree (`/opt/opencode-ref/packages/*`) vs ours (`crates/oc-*`). **Spec audit; product code untouched.**

**Method.** The five R1 stances re-applied as fresh eyes + a new meta-stance on the frame itself.
S1 re-enumerated the ref tree bottom-up (core modules, tui routes/components/dialogs, mini/,
prompt/, session-ui, codemode, plugin-browser, merman, latex — verified by direct listing, not
memory). S4/S5 verdicts were reached by asking, per clause: *what is the cheapest evidence that
satisfies this clause's letter?* — if that evidence is an inventory or a fixture happy path, the
clause fails process-fidelity.

## Headline

- **Integration adequacy of the 52 R1 gaps: 14 ADEQUATE · 35 NOMINAL · 3 MISSING.** The dominant
  failure mode is *nominal integration*: v2 names every family but dropped the taxonomy-v2 tables'
  **[G]ranularity / [F]idelity / [M]ethod columns** during compression. v2 is a family index, not
  an executable spec.
- **30 NEW gaps** (S1 8 · S2 5 · S3 5 · S4 6 · S5 2 systemic · meta 4), including **8 whole feature
  families with no axis clause** that R1's enumeration also missed (skills, commands/agents/prompts,
  instruction discovery, rich-content rendering, composer-context/mentions, Code Mode/plugin-browser,
  model/variant UI, long-tail dialogs incl. stash/experiments).
- **S5 stress: 10/10 sampled clauses passable at inventory grade** — a competent agent can satisfy
  v2 verbatim and still miss every field-finding-class bug R1 was written to prevent.
- **Root cause is one systemic gap (R2-S4-1):** no clause names its granularity × fidelity ×
  evidence artifact. Fix that and ~30 of the 35 nominals collapse; fix nothing and SPEC-R3 will
  re-report this entire register.

---

## (a) Adequacy verdict per integrated R1 gap (all 52)

Verdict scale: **ADEQUATE** = family owned at executable granularity; **NOMINAL** = family named but
granularity, fidelity, or method lost in compression (an agent can pass at inventory grade);
**MISSING** = still no owning clause.

| # | R1 gap | v2 clause | Verdict | What was lost in compression |
|---|---|---|---|---|
| S1-1 | Worktree | C8 | NOMINAL | adopt/move/delete ops, `session_move` tool, dirty-repo fidelity, git-state assertions |
| S1-2 | MCP | C9 | NOMINAL | restart path, CLI surface, real-server fidelity, live round-trip method |
| S1-3 | Provider auth | C10 | NOMINAL | named surfaces (tokens sidebar, /connect, pair QR), storage/refresh, sandbox-cred fidelity |
| S1-4 | Update/upgrade | C11 | NOMINAL | **method set {curl,npm,pnpm,bun,yarn,vp,brew}** — the auditF core finding — uninstall, update cache |
| S1-5 | Mini runtime | A6 | NOMINAL | panel enum (38 modules exist), stream-v2 fragments, width sweep, real-LLM-chunk fidelity |
| S1-6 | ACP | D7 | NOMINAL | session/tool/elicitation translation granularity, real-client interop |
| S1-7 | WebUI app | D6 | NOMINAL | **version gating** (incompatible-server-panel) and **SDK shape** dropped; "PWA" added |
| S1-8 | Sound/notify | C3 (partial) | NOMINAL | emission semantics: OSC 777 vs 9 vs in-process audio, backends (bun/node), gating config |
| S1-9 | Images | B4 only | **MISSING** | DialogImagePreview, gen/attach, retained-image cache, DiffViewerImage still unowned (ours: `image.rs`/`image_blocks.rs` exist) |
| S1-10 | Export/share | D8 | NOMINAL | publish/unpublish lifecycle, export options/result dialogs, CLI formats, round-trip diff |
| S1-11 | Plugin ecosystem | A5/C6/D3/I2 | NOMINAL | "≥3 ecosystems" weakened to "any with UI surface"; plugins.tsx manager route, PluginStorage/NoopRpcPort, plugin-browser unowned |
| S1-12 | Keymap+config write | B1+C4 | ADEQUATE | (C4's JSONC-preservation sub-point lost — see S5-7) |
| S1-13 | Theme system | A7 | NOMINAL | error chain (ThemeErrorToast), oklab, syntax themes, user-theme-dir fidelity |
| S1-14 | Onboarding | C12 | NOMINAL | StartupLoading 500ms/3s gates, MigrationOverlay, getting-started dismiss, location-missing recovery |
| S2-1 | Perf feel | E1 | NOMINAL | no metric/threshold/percentile; "marquee class" named but nothing measurable |
| S2-2 | Startup TTI | E2 | NOMINAL | per-config-variant dimension (plugins on/off) dropped |
| S2-3 | Memory growth | E3 | NOMINAL | RSS/span metrics + sampling cadence dropped; "falsifiable" asserted, not specified |
| S2-4 | Huge data | E4+A3 | ADEQUATE | — |
| S2-5 | Accessibility | E5 | NOMINAL | **color-independence of busy/error states** (the busy-dot lesson's a11y shade) dropped |
| S2-6 | CJK/i18n | E6 | NOMINAL | filter normalization, non-en locale-env fidelity dropped |
| S2-7 | Concurrency | F3 | ADEQUATE | — |
| S2-8 | Network degradation | F4 | ADEQUATE | — |
| S2-9 | Upgrade/migration UX | C11/C12/D4 | NOMINAL | downgrade-after-upgrade, update-notice cadence, mixed-install round-trip |
| S2-10 | Error quality | E7 | NOMINAL | exit codes, toast action labels, copy-diff method dropped |
| S2-11 | Security posture | E8 | NOMINAL | **serious weakening**: auth-per-route incl. statics, pair entropy/expiry, SSRF, adversarial-payload fidelity all dropped |
| S2-12 | Teardown | E9 | NOMINAL | kill -9 / panic-injection fidelity, post-exit system-state audit dropped |
| S2-13 | Determinism/idempotency | (none) | **MISSING** | no clause owns double-submit guards, idempotent re-run, event order under rapid input |
| S3-1 | Terminal matrix | B6 | NOMINAL | profile×capability cell matrix (screen, no-TERM, 16-color; keys/mouse/color/clipboard/cursor/title/selection cols) collapsed to 4 words; TM ledger pointer w/o cell format |
| S3-2 | Platform matrix | B7 | NOMINAL | per-OS sound/notification backends, $EDITOR defaults, per-platform emulators/updater dropped |
| S3-3 | Shell/env matrix | B7 | NOMINAL | OPENCODE_* family, OPENCODE_CLI_CONFIG_CONTENT overlay (auditF finding) dropped |
| S3-4 | Plugin ecosystem matrix | I2/A5/D3 | NOMINAL | minimum ecosystem count + marketplace fetch dropped |
| S3-5 | Data-shape matrices | I1+A3+C4+D4 | NOMINAL | edge-row enumeration (null titles, huge parts, orphan ids, deep chains), fixture provenance dropped |
| S3-6 | Provider/model matrix | (none) | **MISSING** | multi-provider configs, custom/local providers, models-dev drift, capability flags — still no clause anywhere |
| S3-7 | Cross-version interop | C12/D4 | NOMINAL | rust-TUI-vs-v2-server, webui version drift dropped from D6 |
| S4-1 | Standing instrument | I-layer header | ADEQUATE | binding stated ("no axis is valid without it") — but see R2-S4-1 mechanism gap |
| S4-2 | Real-payload provenance | I1 | NOMINAL | I1 rewritten to self-consistency; **provenance recording + "stubs never sole evidence" dropped** |
| S4-3 | Ref+plugin pairing | I2 | ADEQUATE | — |
| S4-4 | Temporal beyond animation | I3+A4+B1 | NOMINAL | toast pause-on-activity, reconnect backoff, debounces, update-check interval, bell timing have no vt-clock owner |
| S4-5 | Fidelity on all axes | header clause | ADEQUATE | letter present; per-clause actionability missing (R2-S4-1) |
| S4-6 | Miss-attribution ledger | I6 | ADEQUATE | (improved: adjudication-masking class added) |
| S4-7 | Process + falsifiability | I5 partial | NOMINAL | **can-fail demonstration (injected-defect detection) dropped**; 45-min/no-background/worktree/English process clause dropped from the spec |
| S5-1 | token×cell×state | A1 | ADEQUATE | state set reshuffled (focus/anim-phase out; A4 owns anim) |
| S5-2 | Numeric sweeps | A3 | NOMINAL | **regression to adjectives** — widths 0..240 incl 0/1/2, msg counts, part-kind enum, ZWJ sequences all dropped |
| S5-3 | binding×route×mode×layer×config | B1 | ADEQUATE | — |
| S5-4 | target×state×shape×lifetime | B2 | ADEQUATE | (selection-copy sub-point dropped → R2-S2-5) |
| S5-5 | Materialized transition table | C1 | ADEQUATE | — |
| S5-6 | Permission granularity | C3 | NOMINAL | per-tool/per-path saved grants, "always allow" scope leakage dropped |
| S5-7 | Config leaf granularity | C4 | NOMINAL | restart, cross-install visibility, JSONC preservation, env overlays dropped |
| S5-8 | SSE field×flow×error | D1 | ADEQUATE | — |
| S5-9 | REST negative paths | D2 | ADEQUATE | — |
| S5-10 | Persistence pairing | D4 | NOMINAL | cell-level migration diffs, pairing-bookkeeping artifact dropped |
| S5-11 | Materialized matrix | F1 | ADEQUATE | round-to-round persistence implied by "materialized" + F6 |

**Tally: 14 ADEQUATE / 35 NOMINAL / 3 MISSING.** Pattern: wherever R1's taxonomy carried a table
row with [G][F][M] columns, v2 kept the row label and dropped the columns. The 3 MISSING are all
cases where R1 itself only sketched the fix (images beyond paste, determinism, provider matrix)
and v2 integrated nothing rather than something thin.

---

## (b) New gap register (30, stance-attributed)

### S1 — Feature-space (8) — from a fresh bottom-up ref enumeration

**R2-S1-1. Skills family — NO AXIS.** Ref: `core/src/skill.ts` + `skill/{discovery,instructions}.ts`
(sources: directory/url/embedded), `tui/.../dialog-skill.tsx`; ours references skill routes
(`oc-server/src/routes.gen.rs`, `handlers/agent.rs`). No clause owns skill discovery, permission
interaction, invocation UI. *Would catch:* skill list/render parity gaps; embedded-vs-directory
precedence bugs.

**R2-S1-2. Custom command / agent / prompt-library family — NO AXIS.** Ref: `core/src/command.ts`,
`agent.ts`, `tui/src/prompt/catalog*` (verified: `prompt/` holds attachment, codec, directory-
completion, directory-recents, frecency, history, mention, parse, display). Filesystem discovery
(`.opencode/{command,agent}/*.md`), frontmatter parsing, name collisions, project-vs-global
precedence, agent-mode binding. B1 scans the *default* binding table only; nobody owns the
user-extensible families. *Would catch:* custom command shadowing a default; agent frontmatter
field silently ignored.

**R2-S1-3. Instruction/rules discovery — NO AXIS.** Ref `core/src/instruction-discovery.ts` +
`instructions/{builtins,index}.ts`; ours `oc-core/src/engine/instructions.rs` (ported, so parity
matters). AGENTS.md hierarchy (global→project→subdir), precedence, dedup, injection order into the
system prompt. Affects *every session's* core behavior. *Would catch:* instruction-order divergence
that changes model behavior with zero visual trace.

**R2-S1-4. Rich-content rendering family — NO AXIS.** Ref ships whole packages ours lacks
entirely: `merman` (TUI-rendered mermaid: flowchart/gantt/gitgraph/sequence + layout audits),
`latex` (math layout), `parsers-config.ts`, `session-ui` timeline renderers, `patch-diff.tsx`;
ours has **zero** mermaid/latex (grep-verified) + `oc-syntax`. No clause owns markdown/code-block/
diagram/math/diff/part-kind renderer parity — including *absent-justified* accounting (F2 covers
exports, not render behaviors). *Would catch:* every "model returns mermaid, TUI prints fence text"
class bug; syntax-theme token drift.

**R2-S1-5. Composer context family — NO AXIS.** Ref `tui/src/prompt/`: `mention.ts` (@-mention
grammar), `directory-completion/recents`, `frecency`, `attachment`, `codec`, `history`. B1/B2 are
generic input clauses; no axis owns mention parsing, autocomplete popup behavior, mention→payload
resolution, context chips. This is the highest-traffic surface in the product. *Would catch:*
@-mention that resolves to the wrong path payload with a correct-looking chip.

**R2-S1-6. Code Mode / plugin-browser / tunnel — NO AXIS.** Ref `packages/codemode` (interpreter,
stdlib, openapi, tool-runtime, tool-schema) + `packages/plugin-browser` (proxy, tunnel, connection,
rpc, files). Referenced only glancingly (I2 pairing note). Code Mode is a complete alternate
execution runtime with its own tool surface. *Would catch:* stdlib function divergence invisible to
D1/D2 (not server-exposed).

**R2-S1-7. Model/variant/preference UI family — NO AXIS (absorbs still-MISSING S3-6).** Ref
`model-preference.ts`, `dialog-model.tsx`, `dialog-variant.tsx`, `core/src/variant.ts` (effort/
toggle/budget_tokens variants); ours `oc-server/src/handlers/model.rs`. No clause owns model
catalog resolution, variants, preference persistence, models-dev drift, capability flags. *Would
catch:* dogfood2-class boot-model seeding across provider shapes.

**R2-S1-8. Long-tail dialog/route surfaces unowned.** Verified in ref tui: `dialog-stash.tsx`,
`dialog-experiments.tsx`, `dialog-debug`, `dialog-status`, `dialog-shell-output`, `retry-provider`,
`masked-text`, `command-palette.tsx`, `session-tabs-rail`, and the `session-ui/pierre/` review-mode
family (commented-lines, comment-hover, diff-selection, file-find). Several are user-visible
features (stash, experiments, review mode), none named in any clause; F1's matrix is the only net
and it is not yet materialized. *Would catch:* stash/experiments shipping as stubs (the auditB STUB
class recurring).

### S2 — Quality-dimensions (5)

**R2-S2-1. E1–E4 are unfalsifiable as written.** None names metric, threshold, percentile, or
artifact. The ref itself supplies thresholds v2 ignores: StartupLoading 500ms/3s gates, reconnect
backoff 1s/5s. Required: every E clause = metric + threshold source + capture artifact (e.g., E1:
frame-interval histogram at vt-clock, stall-burst = >2-frame deficit detector; E2: boot→first-
interactive against the 500ms/3s gates per config variant; E3: RSS slope MB/h + span count over
≥4h soak with periodic sampling artifact; E4: p95 first-paint latency at 10k msgs).

**R2-S2-2. Storage/IO growth dimension missing.** E3 covers memory only. DB size, log growth,
state-file churn, write amplification over soak. *Would catch:* unbounded event-log growth — a
disk-full field failure no current clause can express.

**R2-S2-3. Idle CPU/energy dimension missing.** Event-poll loops, animations while idle, wakeups
per minute at rest. *Would catch:* spin-loop battery drain that passes every functional axis.

**R2-S2-4. Search/filter family quality unowned.** Session-list filter, transcript/message search,
fuzzy matching, normalization (dropped from S2-6's integration and never picked up by a C axis).
*Would catch:* search that misses combining chars / case variants.

**R2-S2-5. Selection/copy fidelity dropped from B2.** R1's B2 explicitly had "selection copy"; v2
dropped it. Clipboard round-trip of *rendered* content (wide chars, emoji, box-drawing, masked
text) unowned. *Would catch:* copy that concatenates cells without wide-char reconstruction.

### S3 — Environment/matrix (5)

**R2-S3-1. Remote-SSH / mosh / slow-link cells missing.** Input-echo latency under 100ms+ RTT,
nested tmux-over-ssh, ssh-forced TERM quirks, mosh prediction conflicts. B6/B7 are local-matrix
only. *Would catch:* echo-lag and redraw-storm classes invisible on a local pty.

**R2-S3-2. Container/CI/minimal-OS cells missing.** No git/rg binaries, read-only HOME, no
/dev/pts, dumb/none TTY, CI env vars (GITHUB_ACTIONS etc. changing behavior). *Would catch:* boot
failure in devcontainer — the fastest-growing install environment.

**R2-S3-3. Width-implementation divergence axis missing.** unicode-width table version vs ref's
width calculation, East-Asian *Ambiguous* width policy, VS16 emoji presentation, grapheme
clustering, user-font cell-metric dependence. *Would catch:* cursor desync / layout corruption
that only manifests with specific fonts+locales — a class A3's "CJK/emoji" adjectives cannot
discriminate.

**R2-S3-4. Windows/ConPTY granularity missing.** B7 says "OS". ConPTY mouse/resize/VT semantics,
powershell profiles, CRLF in tool output, path separators, sound backend absence — each a distinct
cell. *Would catch:* mouse完全壊れる-class Windows-only breakage.

**R2-S3-5. Locale matrix needs cell enumeration.** Absent TZ database, non-UTF-8 locales
(LANG=ru_RU.KOI8-R class), LANG=C fallback, per-locale time formats. "locale env variations" is
not a matrix. *Would catch:* the auditF fixed-UTC timezone bug class on exotic locales.

### S4 — Process-fidelity (6)

**R2-S4-1. Per-clause [G][F][M] columns absent — THE systemic gap.** The I-layer binds "every
axis" in prose, but no individual clause states its granularity, fidelity requirement, or evidence
artifact, so the cheapest legal reading of nearly every clause is inventory-grade. This single
omission is the mechanism behind 35 of the 52 nominal verdicts and 10/10 of the S5 stress
failures. Fix: every clause gets explicit [G] cell enumeration, [F] instrument binding, [M] named
artifact.

**R2-S4-2. Falsifiability canary dropped.** I5 ("a claim without an artifact is a finding of
nothing") is artifact-naming, not S4-7's requirement that the instrument *demonstrate it can
fail* by detecting an injected defect of its class. Reinstate: per axis, an injected-defect canary
run before the axis's evidence is accepted.

**R2-S4-3. Fixture provenance ledger dropped.** v2's I1 was rewritten to self-consistency (good,
but different); R1's provenance rule (every fixture records source: live-ref capture / user dump /
synthetic; synthetic never sole evidence) vanished.

**R2-S4-4. Ground-truth pinning absent.** The spec never pins which ref commit/tree is ground
truth. Ref drifts (beta.13→…); every parity claim becomes non-reproducible within weeks. Add: pin
+ re-baseline protocol on ref release.

**R2-S4-5. Round output contract + no-summary dispatch rule absent.** v2's own compression is the
existence proof: integration lost fidelity because nothing requires dispatches/integrations to
carry clause text verbatim, and no contract defines a round's emitted artifacts (findings file,
matrix delta, ledger entries, spec-delta with gap-ID continuity). Add: rounds emit named
artifacts; integrations quote clauses verbatim and carry gap provenance IDs.

**R2-S4-6. In-repo instruments unbound.** Our tree already contains falsifiable instruments the
spec never mentions: `golden_tests.rs`, `frame_budget_tests.rs`, `replay.rs`, `mouse_modes.rs`,
`diff_viewer_golden_tests.rs`, and ref's `packages/simulation` (tape/replay harness). The I-layer
should bind these as standing evidence artifacts (and adoption candidates), else rounds rebuild
ad-hoc instruments per axis.

### S5 — Granularity (2 systemic; stress detail in §c rewrites)

**R2-S5-1. 10/10 sampled clauses passable at inventory grade.** Sampled: A3, A6, B6, B7, C3, C8,
C9, C11, D6, E8. Each can be satisfied by listing names without exercising a single cell under
real data (rewrites in §c). This generalizes R1's S5 finding from "some axes" to "the v2 text as
a whole".

**R2-S5-2. E8's probe set is gone.** "prompts, secrets handling, path safety" vs R1's route×auth
table *including statics*, pair-code entropy/expiry, credential file perms, traversal corpus,
redaction canaries, SSRF targets. As written, E8 is passable by reviewing our own prompt text.

### Meta — what the five-stance frame structurally misses (4)

**R2-M-1. Cross-axis interactions.** Five single-dimension stances cannot see intersection bugs:
theme×color-depth (a theme correct in truecolor, wrong at 256), plugin×any-flow (plugin mutates
state mid-operation), concurrency×persistence (flock under two clients), resize×streaming (relayout
mid-fragment). Propose: generated pairwise interaction clauses for high-risk axis pairs + one
"interaction of the round" per round.

**R2-M-2. Longitudinal data-shape evolution.** Static shape matrices (S3-5) miss the time axis:
state written by N−1 read by N, months-old rows, v1-era remnants, session trees that only exist
after long real use. Ties to R2-S4-4 pinning.

**R2-M-3. Negative-space parity (superset detection).** Parity asks "does ours do what ref does",
never "does ours do MORE": extra events, extra disk writes, extra network calls — privacy/battery/
perf regressions invisible to parity. Propose: diff the *observable behavior set* (events emitted,
files touched, sockets opened) ours vs ref per flow; extras are findings.

**R2-M-4. Spec change-management + coverage accounting.** v2 clauses carry no gap-provenance IDs,
so integration adequacy had to be reverse-engineered this round; and no per-axis coverage metric
(cells exercised / cells defined) exists, so "convergence" is currently vibes. Fix: every clause
carries source-gap IDs; F1/F6 track per-axis coverage percentage per round.

---

## (c) Proposed v3 diff (concrete clause edits/additions)

Format change first: **every clause becomes a table row `[G] granularity · [F] fidelity (I-layer
refs) · [M] method/artifact`**, per R2-S4-1. Sample rewrites of the ten stressed clauses (S5;
these also cure the corresponding nominals in (a)):

- **A3 →** [G] widths {0,1,2,10,60,80,120,240} × msgs {0,1,2,10,1k,10k} × full part-kind enum
  (materialized from `oc-schema`, not prose) × wide-char set {pure CJK, mixed, combining, ZWJ
  family, VS16, ambiguous} × overflow mode {truncate, wrap, scroll} per surface. [F] I1+I4.
  [M] golden-frame corpus persisted in-repo (bind `golden_tests.rs`), cell table checked into the
  round artifact.
- **A6 →** [G] panel enum from ref `mini/` (footer.{command,form,menu,permission,prompt,subagent,
  view}, scrollback.{writer,surface}, splash, stream-v2.{fragment,image,subagent}) × state ×
  width; fragment corpus = real LLM chunk boundaries. [F] I1+I3. [M] mini PTY capture diff.
- **B6 →** [G] rows {xterm, kitty, ghostty/wezterm-class, tmux×{1,2 nested}, screen, no-TERM,
  16-color, ssh} × cols {kitty-keys, mouse modes, OSC 10/11/12/52/777, DECSCUSR, title,
  selection, color-count fallback, undercurl, synchronized-output}; TM-1..10 cells absorbed with
  signed/unsigned status. [F] real emulators (tmux live). [M] protocol-signature probe script +
  behavior capture per cell.
- **B7 →** [G] OS {linux, macOS, windows/ConPTY} × env cells {unset TERM, no HOME, no PATH, no
  git/rg, read-only HOME, LANG×{C, en_US.UTF-8, non-UTF-8}, TZ absent, OPENCODE_* incl.
  CLI_CONFIG_CONTENT, SSH_TTY, CI vars, docker} × per-OS {$EDITOR default, sound/notify backend,
  updater path}. [F] clean envs. [M] boot+smoke matrix script, cell table artifact.
- **C3 →** [G] decision {allow/deny/always} × tool × path-scope × persistence {session, forever,
  expiry} × queueing; attention: emission {bell OSC 777, OSC 9, in-process audio, system notify}
  × gating config. [F] I1+I3+real grant stores. [M] flow scripts + emission capture trace.
- **C8 →** [G] {create, adopt, refresh, move, delete, rename} × repo state {clean, dirty,
  detached, bare, conflicted, no-git} × {git-side assertion, UI cell, session_move tool}.
  [F] I1 real repos. [M] git-state assertion script per cell.
- **C9 →** [G] {add, connect, auth, fail, restart, remove} × {dialog cells, toasts, home row,
  CLI} × tool invocation + needs-auth + elicitation-blocked. [F] I1 real MCP servers.
  [M] live round-trip trace.
- **C11 →** [G] check→notify→install→restart × method {curl, npm, pnpm, bun, yarn, vp, brew} ×
  {install prefix, PATH precedence, downgrade, cache migration} + uninstall. [F] real updater
  paths. [M] upgrade round-trip script per method.
- **D6 →** [G] {asset hosting, SPA fallback, browser auth/pair redirect, version gating
  (incompatible-server-panel), handoff, SDK/client package shape} × real browser session.
  [F] I1. [M] served-app e2e + proxy capture of the app's actual calls.
- **E8 →** [G] route×auth table incl. static assets; pair-code entropy/expiry; credential file
  perms; traversal corpus; redaction canaries (planted secrets in logs/share/export); SSRF
  targets (fetch/websearch). [F] adversarial payloads. [M] pen-style probe script + findings file.

New clauses:

- **A8 Rich content:** markdown/code-syntax/mermaid(merman-class)/math(latex-class)/diff/
  part-kind renderer matrix vs ref, incl. absent-justified accounting per renderer.
  [F] I1+I7. [M] renderer corpus diff (rendered frames, not parse trees).
- **A9 Images (cures S1-9 MISSING):** {paste, attach, tool-output image, gen, preview dialog
  zoom/controls, retained cache, DiffViewerImage} × format {png,jpg,webp,gif} × size class.
- **B8 Remote/hosting matrix (R2-S3-1/2):** {ssh, mosh, tmux-over-ssh, container, CI, dumb-tty}
  × {echo latency, redraw, TERM negotiation, missing binaries}.
- **C13 Skills:** {directory, url, embedded} sources × discovery/permission/invocation × UI.
- **C14 Commands/agents/prompts:** filesystem discovery × frontmatter fields × collision ×
  project/global precedence × agent-mode binding × catalog/frecency UI.
- **C15 Instruction discovery:** AGENTS.md hierarchy × precedence × dedup × injection order ×
  builtins merge; artifact = materialized final system-prompt diff.
- **C16 Model/variant/preference (cures S3-6 MISSING):** catalog resolution × provider shapes ×
  variants {effort, toggle, budget_tokens} × preference persistence × models-dev drift ×
  capability flags.
- **C17 Determinism/idempotency (cures S2-13 MISSING):** rapid/duplicate input × double-submit
  guards × idempotent re-run × event-order stability under repetition; vt-clock driven.
- **D9 Code Mode + plugin-browser:** interpreter/stdlib/openapi/tool-runtime surface × browser
  tunnel/proxy/connection round-trips.
- **D10 SDK/client package:** public SDK + client shapes the app/SDK consumers rely on, versioned.
- **E10 Storage/IO growth; E11 idle CPU/energy** — each with named metric+artifact per R2-S2-1
  discipline; retrofit E1–E4 with metric/threshold/artifact (use ref constants: 500ms/3s gates,
  1s/5s backoff).
- **X1 Interaction clauses (R2-M-1):** generated pairwise clauses for high-risk pairs {theme×
  B6 color-depth, plugin×flow, concurrency×persistence, resize×streaming, mini×B6, auth×D6};
  one designated "interaction of the round".
- **I-layer additions:** **I8 falsifiability canary** (per-axis injected-defect detection before
  evidence accepted); **I9 provenance ledger** (fixture source recorded; synthetic never sole
  evidence — restores S4-2); **I10 ground-truth pin** (ref commit pinned; re-baseline on ref
  release); **I11 process + output contract** (≤45-min chunks, no backgrounded scans, worktree,
  English; rounds emit {findings, matrix delta, ledger, spec-delta w/ gap-IDs}; integrations quote
  clauses verbatim — no-summary rule). Bind in-repo instruments (goldens, frame-budget, replay,
  mouse_modes, ref `simulation` tapes) as standing artifacts under I5.
- **F1/F6 amendment (R2-M-4):** every clause carries source-gap IDs; per-axis coverage %
  (cells exercised / cells defined) reported per round.

---

## (d) Convergence estimate per perspective track

| Track | State after R2 | Est. further rounds | Basis |
|---|---|---|---|
| P1 feature-space | 8 new families found; long-tail (dialogs/pierre) not yet exhausted | **2** | one full family-sweep round (incl. session-ui/pierre, feature-plugins slots), one verification round |
| P2 quality-dimensions | axes exist; unfalsifiable (no metric/threshold/artifact); 3 new dims (storage, idle-CPU, search) | **1–2** | one metric-pass round writes E1–E11 concretely; verify in next field round |
| P3 environment/matrix | B6/B7 nominal; 5 new matrix families (remote, container, width-impl, windows, locale cells) | **1–2** | cell enumeration is mechanical once [G] restored; width-impl axis may spawn its own round |
| P4 process-fidelity | structural fixes fully specifiable now (I8–I11, pinning, output contract) | **1** (+field validation) | one round to ratify; real validation happens inside subsequent field rounds |
| P5 granularity | systemic (10/10 sampled fail); cure is mechanical [G][F][M] restoration, 10 rewrites already drafted | **1** | v3 applies the rewrites; SPEC-R3 samples 10 new clauses to confirm |
| **Total** | | **~6±1** | within the 10–15 round budget (2 used) |

Sequencing note for the orchestrator: R2-S4-1 ([G][F][M] restoration) is the keystone — applying
it in v3 collapses most nominal verdicts and mechanically carries the S5 rewrites; the new-family
clauses (A8/A9, C13–C17, D9/D10) can land in the same v3 edit. P4's I8–I11 should land before the
next field round dispatch so its evidence is contract-bound.

## Round log (this lane)
- v2 audited: 52/52 gap verdicts recorded; 30 new gaps; v3 diff proposed; convergence ~6±1 rounds.
- Next: integrate v3 (keystone R2-S4-1 + new clauses), then SPEC-R3 fresh-eyes with 10-clause
  random stress sampling per P5.
