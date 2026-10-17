# REPO-SCAN — full-tree pre-release gap sweep (report-only)

**Lane:** reposcan (worktree `.worktrees/lane-reposcan`, branch `lane/reposcan`, base `fd78b23`,
post-F-1-clippy main). **Date:** 2026-10-17. **Product code untouched** — this file is the only
artifact. Scope: marker sweep over `crates/`, deferral reconciliation against every lane log +
audit register, dead-surface check (dispatch/dialog/keymap/SSE), ROADMAP backlog cross-check.

---

## 0. Headline numbers

| Metric | Count |
|---|---|
| `todo!()` / `unimplemented!()` / `panic!("stub…")` macros in product code | **0** |
| TODO/FIXME/XXX/HACK comment markers in non-test product code | **46** (+8 lines of UI placeholder strings / stale doc refs, not gaps) |
| → markers that are **stale** (lane landed, comment never removed) | **15** (lane-integration ×8, protocol-lane ×3, server-lane ×2, database-lane ×1, plugin-host wave-2 ×1) |
| → markers that are **real + already registered** divergences | **20** (plugin-lane ×9, phase-9 ×5, wave-integration ×3 [session_move], pty-ws ×2, schema/port ×2, formatter-lane ×1, policy-spelling ×1) |
| → markers exposing **unregistered** functional gaps | **~2 clusters** (see §4: dead session commands; the 7 stub dialogs carry no TODO at all — found via dead-surface check, not markers) |
| Deferral/registered-gap lists reconciled | **19 lane logs + 6 audits + termatrix TM-1..10 + final-sweep-2 census + ROADMAP ledger** |
| auditB top-10 | **9/10 closed** (one — stub commands — partially: 6 of 12 fixed, 7 stub dispatches remain incl. `location.reload`) |
| auditE top-10 | **9/10 closed** (session_move tool remainder), auditD top-10 **10/10 closed**, auditC P-family **11/13 closed**, S-family **7/10 closed** |
| Release-blockers (user-visible functional break) | **2** |
| Should-fix (parity gap reachable in normal use) | **14** (3 clusters + 8 singles) |
| Acceptable-registered (documented divergence / rare path) | **~28** |

---

## 1. Marker sweep — classification

Sweep regex: `todo!()|unimplemented!()|panic!("stub|TODO|FIXME|XXX|HACK|stub|not wired|phase-9|deferred|placeholder|for now`
(case-insensitive, `/tests/` + golden fixtures excluded). Raw hits ≈ 200 lines; the bulk are
false positives (SQL `BEGIN DEFERRED`, permission-parking "deferred" senders, `BashEvaluation::Deferred`,
UI placeholder pools). After classification:

### 1.1 Stale markers (lane landed; comment debt only — zero functional weight)
| Marker | Site | Reality |
|---|---|---|
| `TODO(lane-integration)` ×8 | `oc-core/src/location.rs:129,139,150,160`, `location_services.rs:61,139`, `hooks.rs:826,1008` | all ports have live impls in `engine/mod.rs:1402-1495` (EngineInterruptor / Permission / Forms ports) |
| `TODO(protocol-lane)` ×3 | `oc-ai/src/protocols/mod.rs:10,16,35` | the modules are fully implemented (anthropic_messages.rs 5,420 LOC; openai_chat.rs 5,612; openai_compatible.rs 149) — the comments are the lane spec left in place |
| `TODO(server-lane)` ×2 | `oc-core/src/provider_catalog/refresh.rs:8,32` | boot.rs:162 runs the production refresh loop; reqwest impl wired |
| `TODO(database-lane)` ×1 | `oc-core/src/permission/saved.rs:38` | `SqliteSavedGrantStore` is wired in `engine/mod.rs:~705` |
| "engine prompt API stub" ×3 | `oc-tui/src/store.rs:872,2504`, `composer/mod.rs:944` | real `PromptSink` (server POST) wired at `store.rs:8216`; comment predates wave 3 |
| wave-2 composer stub docs | `oc-tui/src/components/session_screen.rs:57,115`, `session.rs:4` | historical narration of the retired stub |
| `wave 4's wave-2 TODO` | `oc-plugin-host/src/lib.rs:487` | `execute_tool` is implemented directly below the comment |

### 1.2 Real, already-registered divergences (documented; disposition = accept or schedule)
| Marker | Site | Registration |
|---|---|---|
| `TODO(plugin-lane)` ×9 | `oc-core/src/hooks.rs:49,1258,1302,1352,1421,1507`, `location.rs:31`, `provider_catalog/refresh.rs:116` | plugin KV storage (`PluginStorage` trait has **zero impls**) + `RpcPort` = `NoopRpcPort` (engine/mod.rs:515) + host-plugin slot (`host_slot: ()`) — the phase-9 plugin-host boundary; JS plugins get commands/tools/home-actions but no storage/rpc |
| `TODO(phase-9)` ×5 | persistent_pty.rs daemon boundary ×3, `session.rs:473` (empty-title regeneration), `project.rs:559` (worktree.refresh rescan) + `session.rs:1639`/`project.rs:689` (binned stats / file-level diff parse) | registered phase-9 remainders |
| `session_move` not wired | `oc-core/src/tool/opencode_tools.rs:89-121,361` | the LLM-facing `session_move` tool always errors ("not wired until the session-move lane lands"); the user-facing `session.move` API + Worktrees dialog (ctrl+m POST) **are** wired |
| `TODO(pty-ws)` ×2 | `oc-client/src/methods.gen.rs:1929,2504` | schema models the pty connect WS upgrade as a boolean GET — WS clients live outside the crate (registered, server-deep-map §3) |
| `TODO(port)` ×2 | `oc-schema/src/lib.rs:73`, `misc.rs:121` | v1 legacy schemas, rpc.ts, event-manifest index, `Worktree.adopt()` projection helper — schema-surface deferrals (server routes incl. `/api/rpc` still exist) |
| policy action literal | `oc-core/src/config/normalize.rs:58-62` | oc-schema serializes `provider-use` (kebab) where v2 wire is `provider.use` — registered schema divergence, encode-side only |
| formatter no-op | `oc-core/src/formatter.rs:7` | `Formatter::file` is a no-op seam (`TODO(formatter-lane)`); v2 default ships no formatters → default-parity, diverges only when user configures one |
| `TODO(wave-C)` ×2 | `session/runner/compaction.rs:495` (compaction-hook short-circuit result), `context.rs:620-624` (reference-file guidance is a static "No reference files…" string) | registered runner-lane remainders |
| skills URL sources | `oc-core/src/engine/skills.rs:11,289` | URL skill entries recognized, never fetched |
| pair QR | `oc-cli/src/handlers/misc.rs:225` | links print without the uqr QR block (REGISTERED) |
| `session.{other}` plugin seam | `oc-server/src/plugins.rs:569` | plugin-host session RPC supports prompt/get/list/messages/interrupt; the wider v2 surface errors with an explicit message |

### 1.3 Test scaffolding (excluded)
`golden_tests.rs` placeholder pools ("Fix a TODO in the codebase"), `stub-editor.sh`, websearch
`StubProvider`, ACP stub HTTP server — fixtures by design.

---

## 2. Deferral reconciliation — unified register

Every deferral/remainder list from the lane logs (be1/be2a/be2b/be3/be3b/be4/f1/l1/if/surf1/surf2/
km/tail/dogfood2/dogfood3/termatrix/oklab/sound/d03) and audit registers (auditA–F, termatrix TM,
final-sweep-2 census), one row each. **fixed-since** = verified in code on this branch.

### 2.1 Fixed since registration (verified)
| Item | Origin | Status |
|---|---|---|
| SSE 20 missing event types (form.*, shell.*, session.status/idle, filesystem.changed, worktree.updated, vcs.branch.updated, credential.*, …) | auditD #1–#8 | **fixed** — `sse.rs:705-722` public set + tests |
| session.shell sent command to LLM | auditE #2 | **fixed** — handler executes via Shell service + started/ended events (`session.rs:1075+`) |
| session.fork copied nothing (mis-decoded `before`, pre-created row) | auditE #3 | **fixed** — `fork_boundary` + object boundary, no pre-create, publish errors surfaced (`session.rs:745-920`) |
| websearch zero providers | auditE #4 | **fixed** — `providers_from_env()` wired (`engine/mod.rs:695`) |
| models-dev refresh never wired | auditE #5 | **fixed** — production refresh loop at `boot.rs:162` |
| worktree.create never ran git | auditE #10 | **fixed** — `git worktree add --detach` strategy (`handlers/project.rs:303-305`) |
| experimental.config.update 500 | auditE #8 | **fixed** — `update_shell` JSONC-preserving write (`handlers/config.rs`) |
| Integration/auth host + 10 OAuth providers | auditE #1, be3/be3b | **fixed** — `oc-core/src/integration/*` + credential events; remainders registered (§2.3) |
| Bedrock / Vertex / Copilot chat | be2b/be4 | **fixed** incl. copilot device OAuth; ADC service-account + web-identity registered |
| oc-ai media protocols + generate + session.diff payload | be2a | **fixed**; media ROUTE layer registered |
| `/api/info` capabilities.persistentPty | auditD #9 | **fixed** — `info.rs:37` |
| auditB stub commands ×6 (`mcp.list`, `variant.list`, `server.pair`, `opencode.debug`, `opencode.status`, `session.timeline`) | auditB #1, SURF-2 | **fixed** — dedicated DialogKind arms (`command_palette.rs:1660-1719`) |
| Reconnecting overlay + StartupLoading | auditB #2, SURF-2 | **fixed** — `app.rs:2073` |
| Sidebar live content (context/mcp/location) | auditB #3, SURF-1 | **fixed** |
| Transcript detail states (tool cells, compaction/skill rows, revert banner) | auditB #5, SURF-1 | **fixed** |
| Diff viewer unreachable | auditB #6, SURF-2 | **fixed** (mounted, wired, layer keys) |
| Update surfaces (notice/banner/dialog/install) | auditB #9, tail | **fixed** — closed SURF-2's "DEFERRED updater" |
| Theme tokens (formfield/destructive/hovered/scrollbar/info) | auditB #8, tail(4) | **fixed** |
| ErrorComponent crash fallback + ThemeErrorToast | auditB #10, tail | **fixed** |
| Onboarding ✕ dead + provider gate + persistence | auditB #4, auditC S2, dogfood3 | **fixed** (storage key + hit routing + `!connected` gate) |
| Mouse capture unconditional / `?1015h` / `mouse:false` ignored | auditC P1/P9/P10, dogfood3 | **fixed** |
| Kitty keyboard negotiation | TM-1, tail | **fixed** (PTY-asserted) |
| Theme subs ?2027/?2031 (+?997 stand-in) | TM-2, tail | **fixed** (?997 via OSC-11 re-probe stand-in, registered) |
| DECSCUSR + OSC 12 | TM-3, tail | **fixed** |
| Mouse mode set / text selection | TM-4, dogfood3 | **fixed** |
| Clipboard read (host arm + image paste) | TM-9/P8, tail | **fixed** (OSC-52 read arm implemented+tested, unwired — registered) |
| Panic/SIGHUP restore; BEL gate; editor ?2004/?1004; kitty teardown | auditC P3/P11/P12/P13, tail | **fixed** |
| Stale hit registries / phantom clicks | auditA §4.1, if lane | **fixed** (route gating) |
| Keymap 53 bindings + divergent ids/defaults | auditA #8a, km | **fixed** — 243/243 v2 table coverage, dispatchers for every boot-reachable binding |
| Tab advanced interactions, tool-row affordances, toast action/chips | auditA #8b/#9/#10, km | **fixed** (rail variant + plugins-failed chip registered) |
| `--prompt` / `--auto` forwarding | auditF #1/#2, BE-1→dogfood3 | **fixed** — `BootArgs` (`misc.rs:47`, `app.rs:3430`) |
| cli.json read/write/JSONC/v1-migration/env overlay + mini replay fallback | auditF #3/#5/#6/S7/S8, f1 | **fixed** (`oc-core/cli_config`; TUI delegation landed in SURF-2) |
| v2→rust state-root migration (tabs adoption, pinned, history jsonl copies, model.json both-ends bridge) | auditF #4, auditC S1, f1 | **fixed** |
| Server log layout (opencode[-ch].log + 50MiB trim) | auditF #10, f1 | **fixed** (server sink) |
| read_tabs wiping v2 tabs.json | auditC S1 | **fixed** (foreign docs untouched + adoption path) |
| Sidebar toggle persistence | auditC S4, SURF-2 | **fixed** (session.sidebar write-through) |
| Mini-footer surfaces (Select model/agent/variant titles, palette C row, tab fade/✕/pseudo-tab) | l1 | **fixed** (ctrl+a row rendered-not-dispatched — registered note) |
| Busy-tab spinner + pulse sweep + hover marquee + scrim alpha | user findings, tail(1)/(2) | **fixed** |
| Home strip dead clicks / caret offset / boot model pick | dogfood2 | **fixed** |
| F-1 test-clippy debt | f1 | **fixed** (HEAD commit) |

### 2.2 Still open — user-reachable (not covered by any registration)
See §4 severity lists. Summary: 7 stub palette/slash dispatches; dead `session.compact/undo/
redo/delete/export/background` commands; `serve --hostname` non-IP silent fallback; prompt
history/frecency/stash never persisted by the TUI; session-toggle persistence (thinking/verbosity/
scrollbar/grouping); tui.log outside the data/log layout; P16 epilogue-on-error; worktree.refresh
no-op rescan.

### 2.3 Still open — registered/acceptable (documented divergence or rare path)
| Item | Registered by |
|---|---|
| Persistent-PTY daemon process boundary (socket/keepalive) | ROADMAP R-09 note, phase-9 |
| Plugin KV storage + rpc port + host-plugin slot (sdk/package plugins) | hooks.rs TODO(plugin-lane), phase-9 |
| Plugin `session.*` RPC surface subset | plugins.rs:569 |
| session command registry → documented 404 | agent.rs:62, session.rs:908 |
| session_move tool errors (LLM path) | opencode_tools.rs:121 |
| Pair QR block (CLI + TUI dialog) | misc.rs:225, SURF-2, tail |
| DiffViewerImage working-tree halfblock preview | SURF-2/tail |
| MigrationOverlay + SessionLocationMissing | tail (no v1-migration API / no location-error source in this stack) |
| plugins-failed home cell | SURF-2 (no plugin-status source) |
| OSC-52 read arm unwired (crossterm owns stdin) | tail |
| ?997 notifications undeliverable → OSC-11 re-probe stand-in | tail |
| Media ROUTE layer (executor/auth/polling), BFL sample materialization | be2a |
| Bedrock: web-identity/container/process creds; Vertex: RS256 service-account keys; `X-Interaction-Type` headers; copilot live-sync | be4 |
| OAuth flows: chatgpt browser/headless, azure Entra CLI, gitlab, snowflake external-browser, openai codex variants | be3 |
| Integration console websearch/mcp/policies sections; RS256 WellKnown validation; per-request x-initiator escalation | be3b |
| v2-side never-ported: v1 schemas, rpc.ts schema, Worktree.adopt helper | oc-schema TODO(port) |
| pty connect WS upgrade outside oc-client | methods.gen.rs TODO(pty-ws) |
| Formatter no-op seam | formatter.rs (v2 default empty) |
| Compaction hook short-circuit result; reference-file guidance static | runner TODO(wave-C) |
| Skills URL sources not fetched | skills.rs |
| Policy action literal `provider-use` vs `provider.use` | normalize.rs TODO(schema) |
| Binned session-stats breakdown; file-level diff parse (raw string array returned); worktree.refresh rescan no-op | phase-9 TODOs |
| Global AGENTS.md non-suffixed dir; models-dev snapshot vintage | dogfood2 |
| Surfaceless consumed no-ops: terminal/pane family, which-key panel, scrap, service.restart, aside, verbosity cycle, plugins.install, integration rename/delete, plugin-dialog actions, composer.terminal | km (v2 keymap also eats these without handlers in the panel-less case — but note v2 HAS terminal panes + which-key via addons) |
| Mini-footer `ctrl+a` "View all integrations" row rendered-not-dispatched | l1 note |
| Home-placeholder (b)-coin, backdrop-fg idx15, bg idx0↔232, stats-digit drift, ✕ fg item | final-sweep-2 census (registered classes) |
| K-1..K-9 editing/keymap deltas | dissolved by M-engine (ROADMAP) — verified landed |
| R-10 upstream-blocked; R-11 platform matrix awaiting scheduling | ROADMAP |
| WS channel/continuation for open-responses | oc-ai mod header |

---

## 3. Dead-surface check

* **Dispatcher registry** (command_palette.rs `StoreDispatcher::dispatch`, lines 1378–1860):
  every palette entry and default-bound key was cross-checked. Arms exist for all scroll/diff/
  tab/dialog/theme/update families. **Dead entries found** (advertised, no arm → silent no-op):
  `session.compact` (ctrl+x c, palette), `session.undo` (ctrl+x u, palette), `session.redo`
  (ctrl+x r, palette), `session.delete` (ctrl+d, palette Session), `session.export` (<leader>x,
  palette), `session.background` (ctrl+b). (`prompt.queue`, `app.clear`, `queued_prompt.undo`,
  `session.tab.history.forward`, `terminal.toggle` all have handlers — auditA's older dead list is stale.)
* **Dialog mounts vs registry**: `DialogKind::ServerList` catch-all at `command_palette.rs:556`
  renders the muted `"Not wired in this build"` stub row. Reached by 7 dispatches:
  `open.menu`, `provider.connect`, `opencode.settings`, `session.share`, `session.fork`,
  `location.reload`, `docs.open` (`command_palette.rs:1547-1550`). All 7 are registered palette
  entries (visible in slash autocomplete + palette) — v2 renders real surfaces for each
  (DialogOpen/DialogIntegration/external settings/clipboard-share/fork picker/server reload/browser).
  The onboarding card's **Connect provider** row (pointer-wired in dogfood3) lands here too.
* **Keymap table vs dispatcher**: oc-tui-core table 249 command ids; all either dispatched or in
  km's registered no-op families (§2.3) — no NEW dead bindings beyond §4 cluster.
* **Routes/SSE**: auditD is the baseline (141/141 routes, enforced by `handlers_route_coverage`);
  SSE now publishes the full public set (`sse.rs:705-734`); 1 registered extra (`mcp.prompts.changed`),
  `x-opencode-build` header is the B4-H3 feature. `/api/rpc/:rpcID/:method` route exists but the
  engine's `RpcPort` is `NoopRpcPort` → RPC calls cannot resolve (registered, §2.3).
* **`Dispatch::ServerPending`** sites: only `update.install` / `update.restart` (intercepted by
  the run loop, app.rs:1533+) and `app.exit` (intercepted pre-dispatch via `request_exit`) — the
  `let _ = reason` arms in app.rs are defensive dead letters, not user-visible gaps.

---

## 4. Severity-ranked findings

### Release-blockers (user-visible functional break)
1. **`provider.connect` dead-ends in a "Not wired in this build" stub dialog** — including from
   the onboarding card's Connect-provider CTA (first-run path, pointer-wired in dogfood3 → stub).
   The integration host itself landed (be3/be3b); the TUI entry surface is the stub
   (`command_palette.rs:556`, dispatch arm 1548). First-run users cannot connect a provider from
   the TUI.
2. **Palette/slash commands advertised but stubbed**: `session.fork` (message-context Fork +
   palette row also route here — note the *server API* is correct and fixed; only the picker is
   stub), `open.menu`, `opencode.settings`, `session.share`, `location.reload`, `docs.open`.
   Grouped with #1 as one family (7 dispatches → one catch-all). Individually should-fix;
   collectively they are the largest user-visible stub surface left.

### Should-fix (parity gap reachable in normal use)
1. Dead session commands (bound + palette-advertised, no dispatcher): `session.compact`,
   `session.undo`, `session.redo`, `session.delete`, `session.export`, `session.background`.
2. `session_move` LLM tool always errors — subagent worktree workflow cannot round-trip
   (user-facing `session.move` + Worktrees dialog work; the model-invoked path fails mid-task).
3. Prompt history / frecency / stash never persisted by the TUI (f1 migrated the files, the
   read/write halves registered "for the TUI lane" never landed) — input history + crash-safe
   draft recovery evaporate on exit.
4. `serve --hostname` (and service.json hostname) non-IP literals silently fall back to
   127.0.0.1 (`boot.rs:256`, `serve.rs:66-71`) — no error, wrong bind.
5. Session display toggles (thinking / verbosity / scrollbar / grouping) not persisted through
   the config doc (v2 persists all four; only theme pin + sidebar landed).
6. `model.json` reader still on `<data>/tui/model.json` (`app.rs:3032`) — f1's both-ends bridge
   covers migration, but new writes remain invisible to v2 (split-brain half-open).
7. Pinned quick-switch slots in-memory only (tabs.json side) — v2's `session.json` pins copied at
   migration but never read/written live.
8. Window title never set (OSC 0, TM-5) + `?2026` sync-output wrap (TM-6) + per-frame cursor
   chatter (TM-7) — the registered "polish wave" never ran after tail.
9. Epilogue skipped on error exit (`app.rs:4662` `if result.is_ok()` — auditC P16, INFO).
10. `worktree.refresh` POST is a 204 no-op (external worktrees don't appear until restart).
11. P16-adjacent: `dialog.integration.rename/delete`, plugins install/update/check routes —
    no-op family (registered, but reachable from the plugins dialog).
12. Session PATCH with empty title clears instead of LLM-regenerating (`session.rs:473` — the
    auto-title lane landed, the manual regenerate path didn't).
13. TUI stderr still writes `<state>/tui.log` instead of the shared `data/log` layout (f1
    registered for the TUI lane; not landed).
14. Stale-marker cleanup (15 stale TODOs, §1.1) — misleads future sweeps; zero runtime weight.

### Acceptable-registered (documented divergence / rare path)
The full §2.3 table (≈28 items). Notable: phase-9 plugin KV/RPC + persistent-PTY daemon boundary,
OAuth remainder families (be3/be4), media ROUTE layer, pair QR, DiffViewerImage preview,
MigrationOverlay/SessionLocationMissing, OSC-52 read arm, formatter seam, skills URL fetch,
km's surfaceless no-op families, final-sweep registered classes, R-10/R-11.

---

## 5. ROADMAP backlog cross-check

Burn-down table: R-01..R-09 done (R-09 with the registered phase-9 daemon remainder); R-10
upstream-blocked, R-11 awaiting user scheduling — both properly registered. The 2026-10-16
"SIX-AUDIT MASTER REPLAN" fix program is fully executed through its queue: waves 1–3 all landed
(be1..be4, f1, if, surf1/2, km, tail, l1, dogfood2/3, d03, oklab, sound, termatrix). The plan's
one requeue (--prompt/--auto → oc-tui chain) landed in dogfood3. No ROADMAP "done" claim failed
verification; no open ROADMAP item is unregistered.

---

## 6. Release-readiness call

**GO for beta.13 with two named blockers to fix first, or an explicit accept.**

The core is release-grade: zero stub macros, 141/141 routes, full SSE public set, all six audits'
top-10s closed or registered except the items below, full-corpus parity sweeps green with only
registered classes. The remaining exposure is concentrated in **one TUI dispatch cluster** —
7 commands whose palette/slash rows and (for provider.connect) the first-run onboarding CTA land
on a "Not wired in this build" stub — plus **6 dead session commands** and the **session_move
tool error**. Everything else open is either a small persistence/protocol polish item or a
documented, justified registration.

Recommended pre-tag order: (1) wire or hide the 7 stub dispatches (provider.connect first —
DialogIntegration over the landed integration host; hide-not-wire is acceptable for
docs.open/settings if ruled); (2) dispatch or delist the 6 dead session commands (compact is the
most-used); (3) decide session_move (wire the inbox admit or pull the tool from the registry);
(4) trivial: serve hostname error, epilogue-on-error, worktree.refresh rescan. The persistence
family (history/toggles/model.json/pins) and the polish wave (title/2026/chatter) can ship as
registered should-fixes for beta.13 without blocking acceptance.

*Verification for this report: read-only greps/reads on the lane/reposcan worktree at fd78b23;
no builds, no tests, no product-code edits.*
