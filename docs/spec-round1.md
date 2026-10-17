# SPEC-R1 — Round-1 adversarial review of the scan direction set

**Lane:** SPEC-R1 (worktree `.worktrees/lane-spec1`, branch `lane/spec1`). **Date:** 2026-10-17.
**Input under audit:** `docs/scan-program.md` (R1–R5 framework) as the standing direction set,
cross-referenced against the six audit reports' coverage (`docs/audit{A..F}-*.md`), the field
findings (ROADMAP 2026-10-16/17), `docs/reposcan-findings.md`, `docs/termatrix-findings.md`, and a
feature-surface enumeration of the ref tree (`/opt/opencode-ref/packages/*`) vs ours
(`crates/oc-*`). **This is a spec audit, not a product scan.** Product code untouched.

**Method.** Five stances applied in order: (S1) feature-space coverage — full surface taxonomy from
both trees, family by family, checked for a owning axis; (S2) quality-dimension coverage —
non-functional dimensions the framework cannot express; (S3) environment/matrix coverage; (S4)
process-fidelity lessons — field misses classified as direction-gap vs instrument-gap, both
promoted to spec clauses; (S5) granularity audit — axes a scanning agent can satisfy at
inventory grade. Each gap records WHY it matters + a concrete finding it would catch.

**Headline counts: 52 gaps — S1 feature-space 14 · S2 quality-dimension 13 · S3
environment/matrix 7 · S4 process-fidelity 7 · S5 granularity 11.** Top-10 most dangerous in §3.

---

## (a) Gap register

Legend: **NO AXIS** = no R1–R5 direction owns the family; **PARTIAL** = an axis touches it but
leaves named sub-families uncovered. "Audit-only" means a one-shot audit inventoried it once but
the standing scan program has no direction for it — audits retire; the program is the instrument.

### S1 — Feature-space coverage (14 gaps)

**S1-1. Worktree family — NO AXIS.** Ref: `core/src/worktree*`, `dialog-worktree-name.tsx`,
`dialog-workspaces.tsx`, `session_move`, `worktree.create/adopt/refresh`, branch naming, file
changes. The scan program mentions worktrees nowhere (R5.1's matrix would only catch dead
commands, not wrong git semantics). *Why:* worktrees mutate the user's filesystem; wrong behavior
destroys trust. *Would catch:* auditE's worktree.create git-side gap; the `session_move` LLM tool
that always errors (reposcan §1.2); worktree.refresh rescan TODO (phase-9).

**S1-2. MCP family — NO AXIS.** Ref: `core/src/mcp`, `dialog-mcp.tsx`, MCP home-footer row,
needs-auth/failed toasts, `mcp add/remove` CLI, server restart, tool namespacing. R3.6 covers
*plugins*, not MCP servers. *Would catch:* `/mcps` stub dialog (auditB STUB list); missing MCP
status row + needs-auth toasts (auditB §1/§2); MCP tool permission interactions.

**S1-3. Provider auth / integration host — NO AXIS.** AuditE verdict: integration/auth host
**missing entirely**; Wave-2 fix lane exists in the roadmap but no scan direction will ever verify
it. OAuth flows, device codes, token sidebar, `/connect`, pair + QR, credential storage/refresh.
*Why:* first-run onboarding is exactly where new users land; the connect flow was a beta.13
release blocker. *Would catch:* pair without QR block (auditF); tokens-sidebar dead after
auth-lane fixes regress.

**S1-4. Update / upgrade family — NO AXIS.** Ref: `UpdateNotification` (available/installed/
remote variants), update-check cadence, `upgrade --method {curl,npm,pnpm,bun,yarn,vp,brew}`,
uninstall plan, update cache migration. *Would catch:* UpdateNotification MISSING (auditB §2 —
first-use surface); upgrade reduced to curl with a different install prefix (`~/.opencode/
bin/opencode-rust` vs method-dependent paths, auditF 1.2 #10) — npm/brew users silently diverge.

**S1-5. Mini runtime — NO AXIS (audit B "spot-check" only).** `packages/tui/src/mini/*` (footer
command/form/menu/permission/prompt/subagent/view, stream-v2 transport/fragment/image/subagent,
scrollback writer/surface, splash, queue, verbosity, variants, demo, theme) vs
`crates/oc-tui/src/mini/*`. The roadmap called mini-footer "the sweep's largest visible family"
yet the scan program never says "mini". *Would catch:* mini footer state machine gaps; stream-v2
fragment rendering under real LLM chunk boundaries; oklab quantizer deltas (P1 lane).

**S1-6. ACP adapter — NO AXIS.** Ref `packages/cli/src/acp/*` (connection, translate, elicitation)
vs `crates/oc-cli/src/acp/*`. AuditF checked the command name and exit code only — protocol
semantics (session adapt, tool translate, elicitation round-trips, ACP client interop) never
scanned. *Would catch:* an ACP adapter that exits correctly but translates tool calls wrongly —
invisible to every current axis.

**S1-7. WebUI app family — PARTIAL (R4 consumer only).** The served app (`packages/app`) is
ref-built, so its expectations bind our server — R4.1/4.2 cover event/response *fields*, but no
axis owns: static hosting correctness (asset paths, SPA fallback), browser auth/pairing redirect,
`incompatible-server-panel` version gating, handoff flows, the SDK surface the app uses.
*Would catch:* a version-string or capability-shape change that makes the app refuse our server
(auditD's `info` capability item was exactly this class).

**S1-8. Sound / audio / system notifications — NO AXIS.** Ref `attention-sounds.*`, `audio.ts`,
`feature-plugins/system/notifications.ts`, bell config; ours `oc-tui/src/sound*`,
`notifications_plugin.rs`, `attention.rs`. R3.3 covers attention *flows*, not the audio/notify
emission semantics. *Would catch:* P2 in-process sound playback (v2 miniaudio semantics) — a
roadmap lane with no verification direction; OSC 777 vs OSC 9/notify variants; notification
gating config.

**S1-9. Images family — PARTIAL.** R2.4 covers image *paste* only. Missing: `DialogImagePreview`
(zoom/controls — auditB MISSING), image gen/attach (media protocols — auditE missing domain),
retained-image cache, image diff viewer (`DiffViewerImage`). *Would catch:* pasted image renders
in transcript but is unopenable/unviewable; image tool outputs dropped.

**S1-10. Export / import / share — PARTIAL (R3.1 lists "shared/exported" as session *states*).**
No axis for the flows: export options/result dialogs (auditB MISSING), share publish/unpublish
lifecycle, `session export` CLI formats, fork-with-export. *Would catch:* `/share` stub (auditB);
export producing wrong file content while the dialog looks right.

**S1-11. Plugin manager UX + ecosystem breadth — PARTIAL.** R1.2/3.6/4.3 cover plugin rendering,
lifecycle, payloads — for **omo-slim only**. Missing: `plugins.tsx` manager route (auditB
MISSING), `plugin list/add/check/update/remove` UX, plugin storage/RPC (`PluginStorage` zero
impls, `NoopRpcPort` — reposcan), and the ecosystem matrix (other popular plugins,
plugin-browser/Code Mode pairing). *Why:* the omo-slim lesson generalizes — every plugin that
uses an unexercised API slot is a future field finding.

**S1-12. Keymap customization + config write-path — PARTIAL.** R2.1 scans the *default* binding
table. Ref also has: user keybind overrides, sequence/leader config, v1 keybind-target migration
(`config/migrate.ts`), `Config.update` write path (flock + JSONC-preserving — auditF: **cli.json
is never written by us**; the sidebar toggle is diverted to a rust-private key v2 can't see).
*Would catch:* every "settings toggle invisible across installs" divergence — a data-loss-class
bug for dual-boot users.

**S1-13. Theme *system* family — PARTIAL (R1.1 binds tokens of the active theme only).**
Missing: user-theme discovery dir, theme list/select dialog behavior, **live retheme**
(TM-2 `?2027/?2031` — status unverified post-rig-retirement), system-theme detection (OSC 11),
oklab quantization, syntax themes, `ThemeErrorToast`. *Would catch:* custom user themes silently
falling back to default; theme-dialog listing but not applying.

**S1-14. Onboarding / first-use / recovery surfaces — PARTIAL (scattered).** StartupLoading
(500ms/3s gates), MigrationOverlay, getting-started card (gate + dismiss — dogfood3),
`SessionLocationMissing` recovery, first-run model/provider seeding (dogfood2's wrong-model bug:
read config from the wrong dir, missed the shared v2 `opencode.jsonc` native-providers shape).
These are the surfaces every NEW user hits first, yet they are spread across R1.4/R3.x with no
owner. *Would catch:* exactly the dogfood2/dogfood3 class — both were user-reported, none were
audit-predicted.

### S2 — Quality-dimension coverage (13 gaps)

**S2-1. Performance FEEL — NO AXIS.** Input latency, render jank, scroll stutter, animation
frame pacing. The marquee **stutter** was a field finding (a *performance* bug inside a visual
feature); R1.3 checks cadence *existence*, not delivered smoothness. *Would catch:* marquee
stutter class; transcript re-layout stalls on every keystroke; spinner that freezes while a tool
streams.

**S2-2. Startup time / TTI — NO AXIS.** Boot→interactive latency, first-paint ordering, plugin
load blocking. Ref has explicit 500ms/3s StartupLoading gates; we have no direction that even
asks what OUR boot latency is. *Would catch:* boot that works but takes 3× ref under real
plugin loads.

**S2-3. Memory growth over long sessions — NO AXIS (R5.4 "long session" has no metric).*
Scrollback, event buffers, memoization caches, image retention growing unbounded over hours.
*Would catch:* a leak that only manifests at hour 2 of a real session — the soak ran once
(L4-era) with no memory instrumentation specified.

**S2-4. Huge-data behavior — PARTIAL (R1.4 "long" for rendering only).** 10k-message sessions,
100MB pastes, huge files in diff, huge tool outputs, hundreds of tabs, deep subagent trees.
*Would catch:* truncation/OOM/unbounded-render blowups; the 800-message 24ms transcript figure
was a one-off measurement, not a standing direction.

**S2-5. Accessibility — NO AXIS.** Color contrast per theme, colorblind-safe palettes (are any
ref themes high-contrast?), keyboard-only reachability of every mouse affordance (auditA found
~120 pointer targets; which have key paths?), screen-reader/notification semantics.
*Would catch:* busy/error states distinguishable by color alone (the busy-dot color field finding
is one shade of this class).

**S2-6. CJK / i18n / locale beyond render — PARTIAL (R1.4 wide-char, R2.4 IME).** Truncation
mid-wide-char, wrap at width (not cell count), search/filter with combining chars and
normalization, locale-dependent time formats (auditF: we print fixed UTC where ref prints
local-tz `toLocaleString`). *Would catch:* the session-list timezone bug class; CJK title
marquee width math.

**S2-7. Concurrency — NO AXIS.** Two clients (TUI+TUI, TUI+webui) on one server: same-session
double-drive, SSE per-client isolation, concurrent state-file writes (flock semantics),
concurrent session create/delete, event cross-talk. *Would catch:* client B's events leaking
client A's partial state; settings races on cli.json.

**S2-8. Network degradation — PARTIAL (R3.5 "server loss mid-X").** Slow/streaming LLM
responses, provider timeouts/retry storms, SSE reconnect backoff (1s/5s ref values) under real
drops, marketplace/catalog fetch failure, offline boot. *Would catch:* retry storms, reconnect
overlay that never appears (Reconnecting MISSING — auditB) precisely when the network is bad.

**S2-9. Upgrade/migration UX as a dimension — PARTIAL (R4.4 migrations = schema only).**
v1→v2 migration overlay, state-dir migration, **mixed v2+rust installs sharing config/state**,
downgrade-after-upgrade, update-notice cadence. *Would catch:* auditF's state-root migration gap;
dogfood2's shared-config-dir shape class generalized.

**S2-10. Error-message quality — PARTIAL (R4.2 error envelopes = API shape).** User-facing error
COPY parity and actionability (toast action labels, dialog copy variants, exit-code semantics —
auditF verified codes once). *Would catch:* "Not wired in this build" placeholders reachable in
normal flows (reposcan: 7 stub dispatches remain).

**S2-11. Security posture — NO AXIS.** Per-route auth enforcement incl. static assets, pair-code
entropy/expiry, credential-file permissions, path traversal in static hosting/file
tools/export, secret redaction in logs/share/export, SSRF in fetch/websearch.
*Would catch:* a parity-correct API that serves state files without auth to LAN clients.

**S2-12. Teardown / resource cleanup — NO AXIS.** Child processes, PTYs, temp files, lock files,
terminal restore (panic-safe restore is a wave-3 item), socket cleanup on exit/kill -9 recovery
(stale locks). *Would catch:* leaked ref-bun processes (GATE3 killed 12 — rig hygiene today,
user machine tomorrow); PTY daemons surviving exit.

**S2-13. Determinism / idempotency — PARTIAL (GATE4 was a one-shot).** Event ordering under
repeated/rapid commands, idempotent re-run of commands, double-submit guards. *Would catch:*
queue double-fire on rapid Enter; duplicate session titles racing regeneration.

### S3 — Environment/matrix coverage (7 gaps)

**S3-1. Terminal matrix — NO AXIS (orphaned by rig retirement).** The 2026-10-17 ruling retired
termatrix profiles WITH the differential rig; TM-1..TM-10 (kitty keyboard negotiation, live
retheme subs, DECSCUSR/OSC 12 cursor styling, OSC 52 clipboard, mouse-set selection) currently
have no standing owner. R2.4 mentions "kitty keyboard modes" once. *Would catch:* TM-class
regressions; dogfood3's no-text-selection (mouse-mode sequence) — a field finding from exactly
this matrix.

**S3-2. Platform matrix — NO AXIS.** Linux/macOS/Windows: path handling, `$EDITOR` defaults,
sound/notification backends, terminal emulators per platform, curl-updater per platform.
*Would catch:* platform-conditional divergence no Linux-only scan can see.

**S3-3. Shell/env matrix — NO AXIS.** `$SHELL` variants for tool exec, PATH-less/no-HOME envs,
`LANG`/`LC_ALL`, `TZ`, unset `TERM`, `OPENCODE_*` envs, `OPENCODE_CLI_CONFIG_CONTENT` overlay
(auditF: we don't implement it). *Would catch:* boot failures in minimal/CI environments; the
config-env-overlay miss.

**S3-4. Plugin ecosystem matrix — NO AXIS (omo-slim monoculture).** Direction should require:
top-N real-world plugins exercised (markdown renderers, themes, statusline, command plugins),
plugin-browser/Code Mode pairing, malicious/broken plugin tolerance (S1-11's breadth + S3's
matrix). *Would catch:* any plugin API slot with zero real consumers tested — the omo-slim blind
spot, again.

**S3-5. Data-shape matrices — NO AXIS.** Real-world config variants (jsonc comments, native
providers shape, package-adapter/settings/cost-array — the dogfood2 shape), v2/mixed install
state dirs, real DB snapshots with edge rows (null titles, huge parts, orphan ids, deep subagent
chains). *Would catch:* every "works on fixture, dies on user data" bug — the fixture-fidelity
field class.

**S3-6. Provider/model matrix — NO AXIS.** Multi-provider configs, custom/local providers,
models-dev catalog drift, image/model capability flags (`supportsText` find — dogfood2's
miss-path). *Would catch:* boot-model seeding bugs across provider shapes.

**S3-7. Cross-version interop matrix — NO AXIS.** Rust TUI against v2 server and vice versa
(shared state dirs, protocol tolerance), webui pinned to ref build vs our server version drift.
*Would catch:* dual-install corruption (ties to S2-9).

### S4 — Process-fidelity lessons (7 gaps)

Field findings (2026-10-17): omo-slim sections, dead subagent switch, marquee stutter,
busy-dot color, thought-gutter color. Root causes split into direction-gaps vs
instrument-gaps; BOTH must become spec clauses.

**S4-1. Standing instrument clauses are NOT in the spec.** ROADMAP mandates "real-plugin
sandbox + temporal + token coverage become permanent rig members" — but `scan-program.md` carries
no fidelity section at all. The spec must state: every axis's evidence is invalid unless
produced under the fidelity requirements listed for that axis.

**S4-2. Real-payload provenance undefined.** "REAL plugin payloads" (R4.3), "REAL data shapes"
(R2.2) — "real" is undefined. Clause: real = captured from a live ref (or ref+plugin) sandbox run
or a real user data dump; vanilla stubs may supplement but never substitute; each fixture
records provenance.

**S4-3. Ground-truth pairing rule.** Direction-gap from the omo-slim class: ref-parity cannot
see user-plugin content. Clause: any axis involving plugin content requires the **ref+plugin
pair** as ground truth (R1.2 has this; R2.2/R3.6/R4.3 must say it explicitly).

**S4-4. Temporal class beyond animation.** R1.3 covers animation cadence only; the temporal
field class (stutter, cadence drift) applies equally to: toast pause-on-activity, reconnect
backoff timers, leader/chord timeouts, StartupLoading 500ms/3s gates, debounces, update-check
interval, attention bell timing. Clause: all timed behavior stepped under a **virtual/known
clock** (vt clock), per-frame content recorded.

**S4-5. Fidelity clauses absent from R3/R4.1/R4.2/R4.4/R4.5/R5.1/R5.2.** R1.2/R2.2/R4.3/R5.4
have partial fidelity wording; the rest have none. E.g., R4.1 must require real flows *under
plugin activity and error injection*; R3.1 must require real child sessions (the subagent
sessionID class — dead click was found only with real metadata).

**S4-6. Miss-attribution ledger per round.** R1's log line includes "instrument-miss
attribution" — make it standing: every round closes by classifying each miss as direction-gap /
instrument-gap / coverage-gap and emitting new spec clauses. Without it, rounds repeat blind
spots instead of retiring them.

**S4-7. Process clauses from incidents.** (a) 45-min chunking; (b) long scans never backgrounded
(TTL-reaper killed the orphaned matrix once — 2026-10-16 incident); (c) falsifiability check: a
direction is not accepted until its instrument has demonstrated it can FAIL (detect an injected
defect of the class it claims to cover) — the retired rig's lesson: five gates that couldn't see
the real bug class.

### S5 — Granularity audit (11 gaps)

An axis a scanning agent can satisfy at inventory grade is a dead axis. Required granularity:

**S5-1. R1.1 theme tokens: token × element-cell × STATE.** Current text omits interaction/
animation state. Both color field-findings (busy-dot, thought gutter) were cell×state-level.
Required: for every token, the cell it paints in each of {base, hover, focus, busy, error,
disabled, anim-phase}.

**S5-2. R1.4 edge states: numeric sweeps, not adjectives.** "long/narrow/overflow" → widths
1..240 sweep (incl. 0/1/2), message counts {0,1,10,1k,10k}, full part-kind enum, wide-char
boundaries (incl. emoji ZWJ sequences).

**S5-3. R2.1 keyboard: binding × route × mode × layer × CONFIG-VARIANT, with timings.** Add
user-keybind overrides as a dimension and leader/chord timeout values as observable outcomes.

**S5-4. R2.2 pointer: target × state × shape × LIFETIME-PHASE.** Hit-rect validity across
route/mode change, dialog open/close, busy transitions — not just existence under data shapes.

**S5-5. R3.1 session lifecycle: materialized transition tables.** from×to×trigger×UI-effect,
states enumerated from ref code (not prose lists); R3's current state enumeration is ad hoc
("compacted/forked/shared/..." invites misses).

**S5-6. R3.3 permission/attention: decision × tool × persistence × EXPIRY granularity.** Saved
grants per-tool/per-path, expiry semantics, "always allow" scope leakage.

**S5-7. R3.4 config: per-LEAF-KEY × read-path × write-path × cross-install visibility.** AuditF
found 13/20 cli.json sections ignored — section-level granularity would have missed leaf-level
ignores inside ported sections too. Must also cover write-back format fidelity (JSONC comments
preserved).

**S5-8. R4.1 SSE: event × field × consumer × FLOW × ERROR-INJECTION, plus ordering guarantees
per consumer.** The 20-never-published-events finding (auditD) is what field-level means; add
"under plugin activity" (already noted) and "under failure" (publisher error mid-stream).

**S5-9. R4.2 REST: endpoint × consumer × happy path × every documented error code × malformed
input.** AuditD verified envelopes once; standing axis must include negative paths and what
clients SEND (request-side field parity).

**S5-10. R4.4 persistence: per-table/column/row-kind writer-reader pairs + per state-file key +
migration CELL-level diffs.** "Every state file/DB row writer vs reader" is close but needs the
pairing bookkeeping made explicit so unpaired writers/readers are listable.

**S5-11. R5.1 feature matrix must be a MATERIALIZED ARTIFACT.** command×route×state cells
persisted between rounds with exercised/unexercied bookkeeping — otherwise "cells never
exercised by any prior round" is unanswerable and the axis collapses to whatever the agent
remembers.

---

## (b) Proposed taxonomy v2 — complete top-level direction set

Six scan axes + one standing instrument layer. Per sub-axis: **[G]ranularity · [F]idelity ·
[M]ethod**. Fidelity clauses marked ✱ are the standing instrument (S4-1) applied everywhere.

### Axis A — VISUAL / RENDERING
| Sub-axis | [G] | [F] | [M] |
|---|---|---|---|
| A1 Theme-token bindings | token×cell×state {base,hover,focus,busy,error,disabled,anim-phase} | ✱ token-exact cell capture; ref+plugin pair for plugin-painted cells | cell-diff harness vs ref frames incl. state injectors |
| A2 Geometry/layout | per-surface paddings/gaps/centering/truncate/wrap rules | narrow+wide sweep frames | golden-frame diff at width sweep |
| A3 Edge & huge-data states | widths 0..240; msgs {0,1,10,1k,10k}; full part-kind enum; CJK/emoji/ZWJ | real DB snapshot w/ edge rows ✱ | synthetic corpus + real-session replay |
| A4 Temporal rendering | per-frame content @ every animated element (spinners, pulse, shimmer, marquee, fade, scrim alpha) | ✱ vt/virtual clock; frame stepping | clock-stepped frame capture |
| A5 Plugin-rendered surfaces | every plugin API render slot × real plugin | ✱ real plugins installed in sandbox (≥3 ecosystems + omo-slim) | ref+plugin vs ours+plugin |
| A6 Mini rendering | every footer family state × width; stream-v2 fragments | real LLM chunk streams ✱ | mini PTY capture |
| A7 Theme system | discovery→select→apply→live-retheme→error chain; user themes; OSC 11; oklab; syntax themes | real user theme dirs ✱ | full-chain round-trip |

### Axis B — INTERACTION / INPUT / ENVIRONMENT
| Sub-axis | [G] | [F] | [M] |
|---|---|---|---|
| B1 Keyboard | binding×route×mode×layer×config-variant; chord/leader timing values; custom keybinds | user keymap files ✱ | end-behavior table, dispatch-trace + observable outcome |
| B2 Pointer | target×state×shape×lifetime-phase; wheel in every scrollable; drag/hold/middle/right; selection copy | real metadata ids + plugin rows ✱ | hit-inject + rect-lifetime trace |
| B3 Focus/layering/modality | every reachable layer stack × input routing | — | stack enumeration + routing probe |
| B4 Input methods | paste forms (bracketed/image/large/WHATWG), IME composition, clipboard read/write (OSC 52) | real paste payloads incl. 100MB ✱ | byte-level paste/clip traces |
| B5 Terminal events | resize per surface, focus loss/regain, $EDITOR round-trip, mouse-mode transitions, suspend | — | PTY event scripts |
| B6 Terminal matrix ✱ | {xterm, kitty, tmux (real), screen, no-TERM, 16-color} × {keys, mouse, color, clipboard, cursor, title, selection} | real emulators (tmux live) | protocol-signature + behavior probe (inherits TM-1..10 ledger) |
| B7 Platform/env matrix | {linux, macOS, windows} × {$SHELL, PATH-less, no-HOME, LANG/LC_ALL, TZ, TERM unset, OPENCODE_*} | clean envs ✱ | boot+smoke per cell |

### Axis C — BEHAVIOR / STATE MACHINES
| Sub-axis | [G] | [F] | [M] |
|---|---|---|---|
| C1 Session lifecycle | materialized from×to×trigger×UI-effect table (states from ref code) | real child sessions ✱ | transition-probe scripts |
| C2 Navigation graphs | tabs/picker/family full graph incl. unreachable-node detection | — | graph walk |
| C3 Permission/attention | decision×tool×persistence×expiry; attention incl. sound + system notify + bell | real grant stores ✱ | flow scripts + emission capture |
| C4 Config persistence | per-leaf-key read×write×restart×cross-install visibility; JSONC preservation; env overlays | real config dirs (v2 shape, jsonc, native providers) ✱ | boot→toggle→restart→v2-read |
| C5 Error/recovery | every failure injection {server loss, plugin crash, partial write, bad data} × recovery path | ✱ fault injection | kill/fuzz scripts |
| C6 Plugin lifecycle | install/enable/disable/update/fail × UI × storage/RPC | ✱ real plugins, real payloads | lifecycle scripts |
| C7 Jobs/background tools | job create→progress→finish/fail × UI (hints, dock, panels) | long-running real jobs ✱ | async replay |
| C8 Worktree family | create/adopt/refresh/move/delete × git semantics × UI × session_move tool | real repos w/ dirty states ✱ | git-state assertions |
| C9 MCP family | server add/auth/fail/restart × tools × UI (dialog, toasts, home row) × CLI | real MCP servers ✱ | live MCP round-trips |
| C10 Provider auth | every OAuth/device/token flow × storage × refresh × UI (tokens sidebar, /connect, pair QR) | real provider sandbox creds ✱ | flow capture |
| C11 Update/upgrade | check→notify→install→restart × method set × uninstall × update cache | ✱ real updater paths (curl/npm/brew) | upgrade round-trip |
| C12 Onboarding/migration | first-run seeding, StartupLoading gates, MigrationOverlay, getting-started, location-missing recovery, mixed installs | real v2 user dirs ✱ | fresh-install + migrated-install scripts |

### Axis D — DATA / COMMUNICATION
| Sub-axis | [G] | [F] | [M] |
|---|---|---|---|
| D1 SSE | event×field×consumer(TUI,webui)×flow×error-injection + ordering per consumer | real flows incl. plugin activity ✱ | subscription taps + field diff |
| D2 REST | endpoint×consumer×error-code×malformed-input; request-side parity | live client traces ✱ | proxy capture |
| D3 Plugin-host boundary | every API surface × real payload (omo-slim + ≥2 others) | ✱ real payloads | host-adapter taps |
| D4 Persistence | table/column/row-kind writer-reader pairs; state-file keys; cell-level migration diffs | real DB snapshots, mixed installs ✱ | writer/reader pairing lint + migration A/B |
| D5 Self-consistency invariants | every cross-layer id/metadata chain (sessionID class generalized) | — | invariant checkers |
| D6 WebUI serving | static hosting, SPA fallback, browser auth/pair, version gating, SDK shape | real browser session ✱ | served-app e2e |
| D7 ACP adapter | session/tool/elicitation translation × lifecycle | real ACP client ✱ | adapter interop |
| D8 Share/export/import | publish/unpublish, export options/result, CLI export formats, fork-export | real artifacts ✱ | round-trip diff |

### Axis E — CROSS-CUTTING QUALITY (all new)
| Sub-axis | [G] | [F] | [M] |
|---|---|---|---|
| E1 Performance feel | input latency, scroll/animation frame pacing per surface | ✱ real plugins + real data | instrumented PTY timing |
| E2 Startup TTI | boot→interactive per config variant (plugins on/off) | ✱ real plugin load | timed boots |
| E3 Memory growth | RSS/spans over hours, per-buffer growth curves | ✱ real long session + soak | periodic heap/RSS sampling |
| E4 Huge data | 10k msgs, 100MB paste, huge diff/tool outputs, 100+ tabs | ✱ real oversized payloads | stress corpus |
| E5 Accessibility | contrast per theme×state; keyboard-only path for every pointer target; color-independence of busy/error states | — | automated contrast + key-path reachability |
| E6 i18n/locale | tz/time formats, wide-char truncate/wrap, filter normalization, CJK titles/marquee | ✱ non-en locale env | locale-matrix scripts |
| E7 Error quality | user-facing copy parity + actionability + exit codes | — | copy diff + code audit |
| E8 Security posture | auth per route incl. statics, pair entropy/expiry, file perms, traversal/redaction/SSRF | adversarial payloads ✱ | pen-style probes |
| E9 Teardown/cleanup | exit/kill paths × {children, PTYs, temp, locks, terminal restore} | ✱ kill -9, panic injection | post-exit system-state audit |

### Axis F — ADVERSARIAL / META
| Sub-axis | [G] | [F] | [M] |
|---|---|---|---|
| F1 Feature matrix | command×route×state materialized artifact, exercised/unexercied bookkeeping persisted round-to-round | — | matrix runner + ledger |
| F2 Bottom-up ref enumeration | every export in ref tree accounted (ported/absorbed/absent-justified) incl. packages beyond tui/core | — | export-diff tool |
| F3 Concurrency | {TUI+TUI, TUI+webui} × same-session, settings race, SSE isolation | ✱ two live clients | dual-client scripts |
| F4 Network degradation | slow stream, timeout, drop, offline × flows | ✱ fault-shaped network (tc/proxy) | degraded-net replay |
| F5 Soak | hours-long real session; collects E1–E3 metrics | ✱ real plugins/data/clock | standing soak |
| F6 Fresh eyes | per-round meta-question + ✱ miss-attribution ledger (S4-6) + falsifiability check (S4-7) | — | round close-out ritual |

### Standing instrument layer (applies to every axis; ✱ above)
1. Real-plugin sandbox (omo-slim + ≥2 ecosystem plugins) installed for any plugin-adjacent evidence.
2. Real payloads/data shapes: provenance-recorded fixtures from live ref sandbox or real user dumps; stubs never sole evidence.
3. Virtual/known clock for all temporal evidence.
4. Token-exact cell capture for all color evidence.
5. Terminal matrix cells (B6) available to any axis claiming "renders correctly".
6. Process: ≤45-min chunks, no backgrounded long scans, English reports, worktree isolation.

---

## (c) Top-10 most dangerous current gaps

1. **S3-1 Terminal matrix orphaned post-retirement.** TM-1..TM-10 have no owner; dogfood3's
   selection bug proves the class reaches users. Every "R1 visual" claim is currently scoped to
   one terminal profile by default.
2. **S1-3 No provider-auth axis.** The entire integration/auth host was auditE's missing domain;
   nothing verifies the Wave-2 lane that fixes it. First-run blockers recur.
3. **S2-1 No performance-feel axis.** Marquee stutter was a user-reported field miss; no
   direction can express "too slow/janky" — the spec is blind to an entire bug class it already
   hit once.
4. **S5-1 R1.1 lacks the state dimension.** Both color field-findings were cell×state; without
   mandating state enumeration, the token axis re-scans base states and re-misses.
5. **S4-1/S4-5 Fidelity clauses absent from most axes.** The fixture-fidelity field class
   (vanilla stubs) recurs wherever "real" is unspecified — R3/R4.1/4.2/4.4/5.1 today.
6. **S1-5 Mini has no axis.** A complete alternate runtime with audit coverage of "spot-check
   depth" only; its state machine, transport, and theming are unowned.
7. **S1-12/S5-7 Config write-path at leaf granularity.** cli.json never written; 13/20 sections
   ignored; cross-install invisibility is data loss for dual installs. R3.4's current wording
   cannot surface leaf-level misses.
8. **S1-4 + C12 No update/upgrade/migration UX axis.** Update banner, upgrade methods, migration
   overlay, mixed installs — the surfaces every release forces users through, unowned.
9. **S2-7/F3 No concurrency axis.** Two clients one server is a supported posture (server +
   webui + TUI); SSE isolation and settings races are unauditable under the current set.
10. **S2-3/S2-4 No memory/huge-data axis with metrics.** The product's core loop is long
    sessions; R5.4's "long session" has no measurement definition, so soak evidence is
    un falsifiable.

**Runners-up:** sound/notify (S1-8), security posture (S2-11), ACP (S1-6), WebUI serving
(S1-7), teardown (S2-12).

## Round-2 directive suggestions (for the orchestrator)

1. Fold S5 granularity fixes into R2's dispatch text NOW (binding×route×mode×layer×config;
   target×state×shape×lifetime) — they cost nothing and prevent inventory-grade passes.
2. Add the standing-instrument section (✱ clauses) to `scan-program.md` before R3 dispatch.
3. Promote C8/C9/C10 (worktree, MCP, auth) and B6 (terminal matrix) into R3/R4 directions — they
   are the largest NO-AXIS feature families with known open items.
4. Create F1's materialized matrix artifact early; every later round consumes it.
5. Schedule taxonomy-v2 review as SPEC-R2's input; this register's S2/S3 axes (quality, env)
   become first-class rounds or standing sub-directions attached to every round.
