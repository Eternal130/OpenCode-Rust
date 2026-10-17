# RESCAN — comprehensive divergence re-scan (report + instrument-gap audit)

**Lane:** rescan (worktree `.worktrees/lane-rescan`, branch `lane/rescan`, base `a4431d2` = beta.13).
**Date:** 2026-10-17. **Product code untouched** — artifacts: this file, `docs/rescan.log`,
`tools/parity/src/bin/rescan-tui.rs` (the reusable plugin-armed live-capture driver, test scaffolding).

**Method (what changed vs every prior instrument):** both stacks run LIVE, in one shared
project, with **oh-my-opencode-slim 3.0.3 ACTUALLY ARMED** (the user's real install copied
into each side's plugin cache + the config `plugin` entry). Ref side = the **user's real
binary** `/root/.opencode/bin/opencode2` (v2.0.25) after proving the pinned 2.0.24 checkout
**cannot load the plugin at all** (F10). Our side = freshly built `opencode-rust` (beta.13 tip).
Grids captured per side via PTY + vt100 mirror (`PtyChild`), incl. a 120 ms-cadence temporal
series through the busy window. Evidence roots: `/tmp/opencode/rescan-live/{session,subagent}`,
`/tmp/opencode/rescan-time`, `/tmp/opencode/rescan-click`, `/tmp/opencode/rescan-diag`.

---

## 0. Headline counts

| Axis | Findings | S0 (daily, user-visible) | S1 | S2/note |
|---|---|---|---|---|
| A. Rendering (plugin + tokens + temporal) | 6 | 2 | 2 | 2 |
| B. Interaction (real shapes) | 3 | 1 | 1 | 1 |
| C. C/S communication (plugin-era) | 3 | 1 | 1 | 1 |
| **Total** | **12** | **4** | **4** | **4** |

Plus the **masking audit** (§4): 41 corpus steps across 2 scripts whose busy-frame cells were
absorbed by umbrella (b)-classes — reported loudly per the addendum.

---

## 1. Axis A — Rendering

### F1 (S0) — the omo-slim sidebar is structurally absent in our TUI
- **EXPECTED (ref, live capture** `rescan-live/session/captures/ref-session-06.txt` rows 18–24):
  ```
  ▼ OMO-Slim                    v3.0.3
  • orchestrator               default
  • explorer                   default
  • librarian                  default
  • oracle                     default
  • designer                   default
  • fixer                      default
  ```
  Rendered by the plugin's `tui2.js` through the v2 host's slot-claim API:
  `ctx.ui.slot({append:"sidebar.content", render})` — ref `packages/tui/src/plugin/api.tsx:298-312`,
  mount at `routes/session/sidebar.tsx:70` (`<Slot path="sidebar.content">`). The section
  further carries (per the plugin's `dist/tui.d.ts`, all live on v2): preset row with
  `onActivate`, clickable subagent session targets (`ui.router.navigate`), reusable-task
  targets, busy/status dots (`•`/`⚰`), the animated attention indicator
  (`getSidebarActivityIndicator(now)`), hydrated provider/model display names.
- **OURS (same capture, our-session-06.txt):** the sidebar region below `Context / 18 tokens`
  is blank — nothing of the plugin renders, ever.
- **Root cause (code-cited):** our host cannot execute the plugin's TUI entry at all —
  `tui2.js` statically imports `@opentui/core`, which is host-provided in v2
  (`ensureRuntimePluginSupport`, ref `plugin/runtime-plugin-support.bun.ts`) and absent in
  ours: the load fails containedly, **asserted as the intended behavior by our own test**
  `crates/oc-plugin-host/tests/flagship_tui.rs:113-133`. Our `ctx.ui` bridge offers only
  toast/text-rows-sidebar/footer-hint/dialog (`crates/oc-plugin-host/src/host/host.js:698-784`)
  — no `ui.slot`, no `ui.router`, no `keymap.layer` — and omo-slim (built for the v2 contract)
  never calls our native seams.
- **Why every prior instrument missed it (attribution):**
  - *ref-parity (`tools/parity`)*: server-level HTTP/SSE only; TUI rendering out of scope by design.
  - *tui-diff rig*: `seed_project` (`tools/parity/src/tui_diff.rs:2181-2214`) writes a fixed
    `opencode.json` with **no `plugin` key**; all 26 corpus scripts are plugin-less → the
    comparison could never contain plugin-rendered content. This is instrument-construction,
    not adjudication failure.
  - *auditB "Sidebar live content" closure*: adjudicated against the NATIVE sidebar
    (context/mcp/location sections) with a vanilla stub server — plugin-content was outside
    the audit's inventory.
  - *repo-scan*: registered the phase-9 plugin-host boundary as "acceptable-registered" from
    markers, without a live-plugin render probe.

### F2 (S0) — plugin-registered MCP servers are invisible in our TUI
- **EXPECTED (ref, same capture rows 8–11):** sidebar `MCP` section: `• context7  Connected`,
  `• gh_grep  Connected` — servers registered by the plugin server-side (omo-slim bundles MCP
  servers; connected at plugin boot — v2.0.25 sandbox log: `mcp connected server=gh_grep`).
- **OURS:** no MCP section (sidebar ends after Context). Our plugin host **did accept the
  registrations** — the plugin's own log in OUR sandbox:
  `[v2] mcp servers registered {"count":2}` (`rescan-live/session/our-home/.local/share/opencode/log/oh-my-opencode-slim.*.log`).
- **Root cause:** the engine's plugin seam carries agents+tools only —
  `LocationPluginSource` (`crates/oc-core/src/plugin_apply.rs:77-100`) has no MCP surface;
  `oc-plugin-host` `Registrations.mcps` (`registrations.rs:242,309-310`) dead-ends: nothing
  forwards it into the engine MCP registry or any API the TUI reads.
- **Instrument attribution:** auditD's API/event parity ran a vanilla stub server; the
  MCP-list API with a plugin registering servers was never differenced. auditB's MCP sidebar
  closure used config-sourced MCPs only.

### F3 (S1, temporal) — busy-tab indicator: ref renders within 240 ms, ours never does in the window
- **EXPECTED (ref, `rescan-time/temporal/captures/ref-busy-00..01`):** 120 ms after submit,
  tab strip cell (1,0) = `⠙` style `0x0760_0001_eb0`; by 240 ms = `•` style `0x0584_0001_eb1`
  (two distinct token sets: spinner frame vs pulse dot, bold-bit differs `eb1` vs `eb0`).
- **OURS:** ZERO braille glyphs and no indicator cell across all 40 busy frames
  (`grep -P "[\x{2800}-\x{28ff}]" our-busy-*.txt` → empty); the tab itself still read
  "Untitled session" at 120 ms — the tab opens only as the reply completes.
- **Caveat (honest):** the stub replies in ~100–200 ms, so ours may simply never enter the
  renderable busy state under this cadence; with a real LLM the window is seconds. The
  divergence class (indicator latency + absence under fast replies) stands; per-frame color
  comparison of OUR spinner was impossible in this run because ours never rendered one.
- **Instrument attribution:** see §4 — this is exactly the class the rig cannot see
  (settle-then-capture) and the tail lane pinned only at frozen clocks
  (`docs/tail.log:5-10` — SPINNER_FRAMES unit tests + "vt_temporal_marquee" at frozen clock).

### F4 (S1, tokens) — footer/sidebar usage percent missing
- **EXPECTED (ref):** session footer `18 (0%)` + sidebar Context row `0% used`
  (ref-session-06.txt rows 26,5). **OURS:** footer `18`, no percent row
  (our-session-06.txt rows 26,5).
- **Instrument attribution:** corpus `type-edit-submit` steps were adjudicated with "footer
  usage percent only" inside the (b)-digit-drift umbrella (REPORT.md:786) — the MISSING
  `(0%)`/`0% used` text is a geometry/text difference, not digit drift; the umbrella
  adjudication masked it (same mechanism as §4).

### F5 (S2) — ref update banner `/update to install v2.0.32` — version-skew artifact of using
the user's real 2.0.25 binary (phones home); ours (2.0.24 fleet) has no 2.0.32 knowledge.
Expected binary divergence; recorded so nobody diff-chases it.

### F6 (parity positives, live-confirmed) — the prior color-miss families ARE fixed at rest:
`┃` gutter style digests identical (subagent captures, (2,2)=`0x052c_0001_e90` both sides);
`✕` close glyph style identical (`0x0420_0001_ea0` both sides); boot layout, subagent
transcript card (`✓ General Subagent — Check the fixture`), tab geometry, title fallback all
match. The residual color/temporal risk concentrates in F3's busy-window indicators.

---

## 2. Axis B — Interaction (real shapes)

### F7 (S0) — every plugin interaction affordance is dead by construction
The plugin's clickable surfaces — sidebar session targets (`navigate(sessionID)`),
reusable-task targets, preset row `onActivate`, `/preset` + its keymap commands
(`keymap.layer`, ref `api.tsx:179-199`), status items — all live inside `tui2.js`'s UI, which
never runs in our host (F1). Our alternative seams (plugin commands with `keybinds`,
`registrations.rs:16-26`) are never invoked by this plugin. **The user's "subagent click
dead" in the omo-slim world is this finding** — in v2 the clickable subagent rows are the
plugin's sidebar targets.

### F8 (S1→positive) — native subagent card click: PARITY + server metadata self-consistent (addendum item)
- **End-to-end server self-consistency (our stack, real subagent flow, DB dump of
  `rescan-live/subagent/our-home/.../opencode-stable.db`):**
  - `session_v2`: child `ses_eb6152d3cf…` has `parent_id = ses_eb6152f45f…` ✓, title
    "Check the fixture" ✓.
  - the assistant tool part carries `state.metadata = {sessionID: "ses_eb6152d3cf…",
    status: "completed", truncated: false}` ✓ — exactly the key the TUI click router reads
    (`crates/oc-tui/src/components/session.rs:763-766`, `sessionID`/`sessionId`).
  → **our server's real part-metadata is self-consistent end-to-end; the dead click is not a
  server-metadata bug.**
- **Live click test** (`rescan-click/clicksub`): clicking the transcript card row at (6,5) is
  a no-op on BOTH sides (neither navigates from that row/coords); enter-navigation into the
  child was already corpus3-verified. Parity holds for the native card.
- **In the plugin world** the equivalent affordances are F7 (absent surface), and omo-slim's
  `task_*` tool cards render as generic tools in our transcript (click → ToggleToolDetail at
  best, never NavigateToSession).

### F9 (S2) — plugin commands: our TUI's own host loads the plugin (the orchestrator agent
rail + agent rewrite DID flow: our transcript shows `Orchestrator · Fake Model`), and 4
commands registered host-side; but see F11 — the server never announces them, and palette
visibility for plugin commands was not live-asserted this run (flagged for the fix lane).

---

## 3. Axis C — C/S communication (plugin-era)

### F10 (S0) — the pinned ref (2.0.24) cannot load the current plugin: every plugin-era parity claim from the standing rig is vacuous
- Live proof: with the plugin armed, the 2.0.24 ref SERVER fails the plugin with
  `Cannot find module 'oh-my-opencode-slim/rpc'` (ref-home log, refs err_7b5efd85…d5244f41),
  the TUI toasts `⊙ 1 plugin failed /plugins`, the /plugins dialog shows
  `x oh-my-opencode-slim@latest — failed, update available`.
- The user's real binary (v2.0.25, sandboxed HOME, same cache) loads it cleanly:
  `loading plugin id=oh-my-opencode-slim entrypoint=…/dist/server/index.js`, then
  `mcp connected server=gh_grep`, `command.updated` ×3 — zero errors.
- **Consequence:** tools/parity + tui-diff, both pinned to `/tmp/opencode/ref` @2.0.24, are
  structurally incapable of grounding plugin-era parity (they would compare "both sides
  broken"). My driver therefore used `--ref-bin /root/.opencode/bin/opencode2`.
- **Instrument attribution:** the ref pin predates the plugin era; no gate checks that the
  ref can load the current plugin generation.

### F11 (S1) — plugin registration/MCP events are not emitted with payloads
- **EXPECTED (ref live):** at plugin boot + MCP connect the ref broadcasts `command.updated`×3,
  `agent.updated`, `provider.updated`, `model.updated` (v2.0.25 log) — clients (e.g. the
  plugins dialog, `feature-plugins/system/plugins.tsx:60`) refetch on them.
- **OURS:** `SERVICE_FRAMES` (`crates/oc-server/src/announce.rs:70-80`) publishes these as
  **empty-payload boot/config frames only**; nothing fires when the plugin host registers
  commands/MCPs (our DB `event` table for the same session: session lifecycle types only).
- Divergence class: external v2 clients (and our own future TUI) cannot reactively discover
  plugin-registered tables.

### F12 (S2) — cache-root duality: ours reads `.cache/opencode-rust/npm`, v2 reads
`.cache/opencode/npm`; bridged on the live machine by the one-way migration. A fresh our-side
home with only the v2 cache fails with the correct actionable error
(`not installed; run opencode plugin add` — captured in our tui.log). By design; recorded so
the fix lane's sandbox uses the right root.

---

## 4. MASKING AUDIT (addendum-critical — reported loudly)

**Yes — the rig rendered busy-frame cells inside mismatch steps, and (b)-umbrella
adjudication absorbed them without per-cell adjudication.**

1. `tabs-live-session` (the ONLY corpus script that creates live/busy sessions via the `+`
   affordance): **19 mismatch steps** adjudicated in `docs/km.log:248` as
   *"coin + digit drift + idx15: registered classes"* — i.e., the placeholder (b)-coin, the
   `Nms` (b)-digit-drift, and the backdrop-fg idx15 indexed-color artifact.
   `docs/tail.log:46` then states verbatim: *"NO new spinner/sweep-cell divergence class
   from the animation family (**the busy-tab frames are part of the registered composition**)"*
   — with the criterion being **identical mismatch counts vs the baseline (19 == 19)**, not a
   per-cell adjudication of the busy frames.
2. `multi-session`: **22 mismatch steps**, same umbrella (`km.log:246`, `tail.log:45`,
   `if.log:171-175`).
3. Today's live capture (F3) proves a busy-indicator divergence class EXISTS in reality
   (ref `⠙`/`•` with distinct styles within 240 ms; ours nothing) — so the "no new class"
   conclusion was **false at the element-family level**, and the umbrella classes
   (coin/digits/idx15 — the last of which is itself a *color* class) were structurally
   capable of swallowing busy-frame cell diffs at (0..2,0)-class coordinates.
4. Mitigating fact (recorded for fairness): the surviving rig artifacts
   (`/tmp/opencode/tui-diff-m5b`, `-fallback`) contain **zero braille/spinner glyphs** — the
   settle-then-capture discipline means the rig usually never OBSERVED a busy frame at all;
   the masking applies to the steps where busy frames did appear inside settled compositions
   (tabs-live-session/multi-session above). Both failure modes — never-observed AND
   umbrella-absorbed — are instrument gaps, listed in §6.
5. Adjacent recorded non-observations (same family, in-log): `tail.log:47` — themes-nord
   jump-row fg "did not surface (the script's viewport stays stuck-bottom — the indicator
   never mounts)".

---

## 5. Top-10 (severity-ranked: user-visible daily first)

| # | Finding | Sev | One-line fix direction |
|---|---|---|---|
| 1 | F1 plugin sidebar absent (omo-slim UI cannot run) | S0 | Decide product stance: native first-party sidebar for the flagship plugin (server-state-driven: agents/board/attention from tui-state.json + APIs) or a JS-sidecar TUI-plugin runtime; today's answer ("not mappable") leaves daily users with nothing |
| 2 | F10 ref pin 2.0.24 can't load the plugin | S0 | Bump the parity ref pin to ≥2.0.25 (or drive the user-track binary) + gate that the ref loads the current plugin generation |
| 3 | F2 plugin MCPs invisible | S0 | Forward `Registrations.mcps` into the engine MCP registry + emit mcp events |
| 4 | F7 plugin interaction affordances dead | S0 | Follows F1; at minimum map plugin-registered commands + status items to native surfaces |
| 5 | F3 busy-tab indicator absent in fast window | S1 | Open the tab + render the busy indicator at submit-ack time; add live busy-window assertions |
| 6 | F4 usage percent missing (footer + sidebar) | S1 | Add `(N%)` / `N% used` where ref renders them |
| 7 | F11 registration events payload-less | S1 | Emit plugin/mcp/command/agent updated with payloads at registration time |
| 8 | F9 plugin-command palette visibility unasserted | S2 | Live-assert palette entries for plugin commands in the fix lane |
| 9 | F12 cache-root duality | S2 | Keep, but document in plugin add error hint |
| 10 | §4 umbrella-class masking policy | S0(process) | Forbid count-equality closures on steps containing animated/busy cells; require per-cell family classification |

---

## 6. Instrument gaps — what standing infrastructure MUST permanently cover

1. **Real-plugin sandbox in the rig (both stacks).** `seed_project` gains a plugin-armed
   variant: seed `<home>/.cache/opencode{-rust,}/npm/<pkg>@<spec>/<stamp>/` + config
   `plugin` entry; assert BOTH servers report the plugin active before any capture
   (this alone would have caught F1/F2/F10 instantly).
2. **Ref-version policy for the plugin era.** The rig must pin a ref that loads the current
   plugin generation (≥2.0.25), record the ref version in every report, and FAIL when the
   ref server's plugin state ≠ active while ours is (and vice versa).
3. **Temporal stepping.** Add sub-150 ms cadence captures through submit→reply busy windows
   to the standing corpus (the `temporal` scenario in `tools/parity/src/bin/rescan-tui.rs`
   is the working prototype: per-frame animated-cell extraction + glyph/style sequence
   comparison). Settle-only captures must never be cited for animated-element parity.
4. **Token sweep table.** A standing per-element token assertion list (busy dot, pulse,
   spinners, `┃` gutters, scrollbar track/thumb, attention marks, unread dots, `✕`/`＋`,
   placeholders) asserted against live ref cells each release — token drift survives
   goldens because goldens are self-pinned.
5. **(b)-class hygiene.** Umbrella classes (coin/digit-drift/idx15) may not absorb steps
   containing busy/animated/indicator cells; lane closures need per-cell family evidence,
   not identical mismatch counts (§4).
6. **Plugin-era server-event parity.** Scenario goldens must include a plugin-armed boot:
   assert `plugin/command/agent/mcp updated` events with payloads (F11), and `/api/plugin`
   + MCP-list APIs reflecting plugin registrations (F2).
