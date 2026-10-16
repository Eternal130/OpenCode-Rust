# Audit C — Terminal-protocol emission & persisted state (v2 ref vs Rust port)

Lane `lane/auditC` (2026-10-16). READ-ONLY audit of the two layers the grid
differ cannot see: **what escape sequences each TUI puts on the wire** (and in
which phase — boot / mount / during-run / teardown / epilogue) and **what state
each TUI persists across restarts** (files, keys, formats, migration).

Method notes:
* v2 = `/opt/opencode-ref` (pinned v2.0.24). Its TUI delegates terminal setup
  to the native `@opentui/core` 0.5.14 (`libopentui.so`); emissions were
  verified against the `.so` rodata (`strings` on the bun-cache artifact) plus
  the TUI-layer call sites (`createCliRenderer` options, `destroyRenderer`,
  `setTerminalTitle`, clipboard adapter, theme providers).
* ours = `/opt/opencode-rust` (`crates/oc-tui`, crossterm 0.28.1 + ratatui);
  crossterm's exact emission strings were verified against the vendored
  registry source (`EnableMouseCapture` = `?1000h ?1002h ?1003h ?1015h ?1006h`,
  crossterm-0.28.1/src/event.rs:319-331).
* Cross-checked against `docs/termatrix-findings.md` TM-1..TM-10 (per-finding
  verdicts in §4).

---

## 1. Protocol-emission inventory

### 1.1 v2 (ref) emissions

| Sequence | Phase | Source (v2) |
|---|---|---|
| `CSI ?1049h` alt screen enter | boot | opentui rodata (`[?1049h`); `createCliRenderer` app.tsx:246-263 |
| kitty keyboard: `CSI ? u` query → `CSI > 5u` push (answer-gated; never pops) | boot | rodata + TM-1 wire evidence; gated by `useKittyKeyboard: {}` app.tsx:238 |
| mouse `?1000h ?1002h ?1003h ?1006h` — **config-gated** (`useMouse: config.mouse`, default true) | boot / live re-config | rodata; app.tsx:241, live flip app.tsx:593; schema config/index.tsx:247, default :311 |
| `CSI ?1004$p` DECRQM focus probe (set `?1004h` not observed on wire) | boot | rodata `[?1004$p`; TM-4 |
| `CSI ?2004$p` probe → `?2004h` bracketed paste | boot / teardown pop | rodata |
| `CSI ?2026$p` probe → `?2026h/l` per-frame sync wrap (unconditional in use) | every frame | rodata; TM-6 |
| `?2027h` color-scheme + `?2031h` mode-report subscriptions (DECRQM-gated) | boot | rodata; TM-2 |
| custom `?997;1n` / `?997;2n` theme-change notifications consumed → `refreshPalette()` | during-run (input handler) | theme.tsx:261-266, mini/footer.ts:1069 |
| OSC 11 bg query (+ OSC 10/4, escalation sweep), `waitForThemeMode(1000)` | boot | theme.tsx:290; TM-8 |
| DA1/DA2/XTVERSION/XTGETTCAP/OSC 99/OSC 1337/private OSC 66 probes | boot | rodata; TM-8 |
| `CSI 1 SP q` DECSCUSR blink-block + `OSC 12 ; #…` cursor color | boot | rodata; TM-3; configurable `cursor.style` config/index.tsx:263-266 |
| `OSC 0 ; OpenCode` / `OC | {session title}` / `""` clear | mount / route change / teardown | app.tsx:604, 611, 615, 620, 1138; destroy clear util/renderer.ts:4 |
| OSC 52 clipboard write (terminal arm of layered adapter; **host-first**: pbcopy/xclip etc.; `destination: "all-available"`), NUL-stripped | during-run (copy) | clipboard.ts:53-61 |
| clipboard **read** (image/png + text/plain, host or terminal) | during-run (paste-image) | clipboard.ts:24-51 |
| `onCopySelection` → clipboard write + `clearSelection()` | during-run (selection copy) | app.tsx:561-570 |
| kitty graphics APC (negotiated via DA1/rodata `kitty_gfx`; quiet/control flags) | during-run | rodata; image placements |
| OSC 8 hyperlinks (`]8;id=`) | during-run | rodata |
| OSC 9 / 99 / 777 notifications (`triggerNotification`) | during-run (attention) | attention.ts:142-148; rodata `]99;i=opentui-notifications` |
| suspend (`renderer.suspend()` → teardown set) + `SIGCONT` resume + SIGTSTP | during-run (palette cmd `terminal.suspend`) | app.tsx:1120-1128 |
| SIGHUP → `destroyRenderer` | teardown (signal) | app.tsx:283-287 |
| mode pops + `?1049l` + cursor restore on `renderer.destroy()` | teardown | rodata; acquireRelease app.tsx:257-263, 249-251 |
| epilogue `stdout.write(epilogue + "\n")` (always) + error → stderr | exit | app.tsx:447-450 |
| embedded terminal pane: `ESC c` RIS checkpoints, OSC 4/10/11 palette sets | during-run (terminal pane) | terminal-pane.tsx:114, 362-363 |

### 1.2 ours (Rust port) emissions

| Sequence | Phase | Source (ours) |
|---|---|---|
| raw mode + `CSI ?1049h` alt screen | boot | app.rs:2167-2170 |
| `CSI ?1004h` focus (**unconditional**) | boot | app.rs:2171 |
| `CSI ?2004h` bracketed paste | boot | app.rs:2175 |
| `CSI ?1000h ?1002h ?1003h ?1015h ?1006h` mouse (**unconditional — no config gate**) | boot; re-emitted after editor | app.rs:2168-2185, 2613-2624; crossterm-0.28.1 event.rs:319-331 |
| OSC 11 query `\x1b]11;?\x07` + 150 ms raw poll read (keystrokes in window dropped) | boot (input thread) | theme.rs:494-534; loop blocks ≤600 ms app.rs:2401-2405 |
| per-frame ratatui draw: `?25h` show-cursor chatter (~300/run), CUP caret positioning | every frame | app.rs:2808-2848; TM-7 |
| OSC 52 clipboard write, 100 KB cap, `\x1b]52;c;<b64>\x1b\\` (**write-only**, no host fallback, no read) | during-run (copy) | clipboard.rs:18-40, 119-140 |
| kitty graphics APC (env-probe only: KITTY_WINDOW_ID/TERM names) + per-frame vanished-id `a=d,d=i` deletes | during-run | image.rs:33-94, 124-158, 223-227 |
| OSC 9 / 99 / 777 notifications (careful port of opentui's three protocols + env knobs) | during-run (attention) | os_notify.rs:1-70 |
| BEL `\x07` on attention transitions (**ungated** by attention config) | during-run | app.rs:2493-2500, attention.rs:44-65 |
| editor suspend: `?1000l ?1002l ?1003l ?1015l ?1006l` + `?1049l` only — **?2004/?1004 stay latched through `$EDITOR`**; raw mode off/on | during-run (editor) | app.rs:2612-2625 |
| teardown: `?1049l ?1004l ?2004l ?1000l ?1002l ?1003l ?1015l ?1006l` | teardown | app.rs:2931-2941 |
| (comment claims kitty `a=d,d=a` delete-all at exit — **no command is actually emitted**) | teardown (missing) | app.rs:2938-2941 vs image.rs:155-158 |
| epilogue stdout write (**success path only**) | exit | app.rs:2947-2955 |

### 1.3 Protocol diff, severity-ranked

**We emit, v2 doesn't:**

| # | Finding | Severity |
|---|---|---|
| P1 | Mouse capture **unconditional** — `TuiConfig` has no `mouse` field at all (config.rs:131-152; the JSON key is parsed away in tests without assertion, config.rs:261), so `"tui": {"mouse": false}` is silently ignored; v2 gates boot AND live (`useMouse`) and defaults true (app.tsx:241, 593). Combined with P9 this is the "mouse emission swallows native selection" family: the v2 escape hatch does not exist in the port. Re-enabled unconditionally after $EDITOR (app.rs:2620-2624). | **HIGH** |
| P9 | `?1015h` (urxvt extended coords) extra mode v2 never sets (crossterm bundle) | MED-LOW |
| P10 | `?1004h` set unconditionally (v2 only DECRQMs `?1004$p`; feeds our attention marker — beneficial but wire-divergent) | LOW |
| P11 | BEL `\x07` on attention transitions, not gated by `attention.*` config — v2 never emits BEL | MED-LOW |
| P12 | Editor suspend leaves `?2004h`/`?1004h` latched into `$EDITOR` (only mouse+alt disabled, app.rs:2613-2617) | LOW |
| P13 | Teardown kitty delete-all promised by comment, never emitted (app.rs:2938-2941) — alt-screen exit mostly covers it; dead comment | LOW |
| P3 | **Panic/SIGHUP-unsafe teardown**: no SIGHUP handler; the panic hook restores stderr only (stderr_redirect.rs:111-125), so a panic mid-loop leaves raw mode + alt screen + all four modes latched (user's shell is broken until `reset`). v2: SIGHUP → `destroyRenderer` (app.tsx:283-287) + Effect finalizers on every exit path | MED-HIGH |

**v2 emits, we don't:**

| # | Finding | Severity |
|---|---|---|
| P2 | Kitty keyboard protocol entirely absent: no `CSI ? u` query, no `CSI > 5u` push, no pop (= TM-1). No disambiguate/alternate-keys on kitty/foot/ghostty/wezterm | **HIGH** |
| P4 | Window title lifecycle: no `OSC 0` set of "OpenCode"/"OC | {title}", no clear at exit (= TM-5, extended with route titles app.tsx:604-620 + destroy-clear util/renderer.ts:4) | MED |
| P5 | `?2026h/l` per-frame synchronized output (= TM-6) — flicker/tearing polish on slow links | MED |
| P6 | Live re-theme subscriptions: `?2027h`/`?2031h` (= TM-2) **and** the TUI-layer `?997;1n/2n` handler (theme.tsx:261-266, mini/footer.ts:1069) — a runtime terminal light/dark flip never reaches us until restart | MED |
| P7 | `CSI 1 SP q` DECSCUSR + `OSC 12` cursor color (= TM-3); v2 even exposes `cursor.style` in config (config/index.tsx:263-266) — unimplemented here | MED |
| P8 | Clipboard read side (image/png + text) and host-clipboard fallback (pbcopy/xclip) — ours is OSC-52-write-only with a hard 100 KB cap (extends TM-9) | MED |
| P15 | Capability negotiation surface: DA1/DA2/XTVERSION/XTGETTCAP/DECRQM{1004,1016,2004,2026,2027,2031}/OSC 10/4/99/1337/private 66 — ours probes only OSC 11 (= TM-8) | INFO |
| P16 | Epilogue always written in v2 (also error text → stderr, app.tsx:447-450); ours only on `result.is_ok()` (app.rs:2947) | INFO |
| P17 | v2-only surfaces with no port analog: OSC 8 hyperlinks, kitty text-sizing negotiation, embedded-terminal palette writes (terminal-pane.tsx:362-363), smcup-style `renderer.suspend()` + SIGTSTP palette command (app.tsx:1120-1128 — ours has no suspend command) | INFO/LOW |
| P14 | Ours probes OSC 11 with a self-managed raw-mode toggle inside the input thread + ≤600 ms boot block before first `apply_config` (app.rs:2388-2406); v2 races a live renderer (`waitForThemeMode(1000)`) — different boot-latency/ownership profile | INFO |

Parity confirmed: alt screen `?1049`, bracketed paste `?2004`, kitty graphics APC shape (`q=2` quiet), OSC 9/99/777 notification protocol selection incl. env overrides, OSC 52 write encoding, exit-epilogue-after-teardown ordering, color-mode decision table (TM clean bill #1).

---

## 2. Persisted-state inventory

### 2.1 v2 (ref) — files and readers/writers

State root `paths.state` = `<XDG_STATE_HOME>/opencode` (util/global-roots.ts:8,16);
channel storage = `<state>/<channel>/tui/<key>.json`, channel = `OPENCODE_TUI_CHANNEL ?? OPENCODE_CHANNEL` (baked `stable`), flock + atomic JSON + fs.watch live reload + `flush()` (context/storage.tsx:47-141).

| Key / file | What | Writer / reader (v2) |
|---|---|---|
| `<state>/<ch>/tui/tabs.json` | open tabs, cwd/global scope | session-tabs.tsx:72-113 |
| `<state>/<ch>/tui/session-list.json` | dialog prefs (`all_projects`) | dialog-session-list.tsx:42 |
| `<state>/<ch>/tui/layout.json` | `verticalTabsWidth` + `paneWidth`/`terminalWidth` | app.tsx:500; session-frame.tsx:42 |
| `<state>/<ch>/tui/getting-started.json` | sidebar onboarding dismissal | feature-plugins/sidebar/footer.tsx:11-51 |
| `<state>/<ch>/tui/update-notifications.json` | shown version numbers | update-notification.tsx:37 |
| `<state>/<ch>/tui/directory-recents.json` | recent project dirs | prompt/directory-recents.ts:13 |
| `<state>/<ch>/tui/session-terminal-selection.json` | selected embedded terminal | session-terminals.tsx:21-22 |
| `<state>/<ch>/tui/view.json` (mcp sidebar open) + `plugin.<id>.*` | plugin storage namespace | mcp.tsx:6; plugin/api.tsx:201 |
| `<state>/model.json` | model preference doc: recent (cap 10)/favorites/variants | local.tsx:154; model-preference.ts:110-122; mini/variant.shared.ts:6 |
| `<state>/session.json` | pinned session slots (1-9 quick switch) | local.tsx:514-544 |
| `<state>/prompt-history.jsonl` | prompt history (cap 50) | prompt/history.tsx:93 |
| `<state>/prompt-stash.jsonl` | prompt stash (cap 50) — crash-safe draft recovery | prompt/stash.tsx:40-90 |
| `<state>/frecency.jsonl` | mention frecency | prompt/frecency.tsx:42 |
| `<config>/cli.json` (JSONC, `$schema`, flock, atomic) | TUI config doc: mouse, attention, tabs, theme, keybinds, leader, session toggles, cursor | cli config service packages/cli/src/config/config.ts:31-60; TUI writes: theme pin/free theme.tsx:233-253, session toggles routes/session/index.tsx:1029-1073; v1 migration migrate.ts (keybind renames, `copy_on_select`) |

### 2.2 ours (Rust port)

| Key / file | What | Writer / reader (ours) |
|---|---|---|
| `<state>/<ch>/tui/tabs.json` | open tabs (cwd/global scope, **`version:2` stamp**) | storage.rs:110-142; read gate storage.rs:336-350; save loop app.rs:2644-2665 |
| `<state>/<ch>/tui/session-list.json` | dialog prefs | storage.rs:144-159 |
| `<state>/<ch>/tui/sidebar.json` | sidebar mode stand-in (`{"mode":"auto"|"hide"}`) | storage.rs:161-174; app.rs:2277-2288 |
| `<XDG_DATA>/opencode/tui/model.json` | model preference ring (recent/variant) | app.rs:1952-1959 (`roots.data.join("tui")`), write app.rs:2596-2600, mini/model_pref.rs:197 |
| `<config>/cli.json` (strict JSON, read-only) | TUI config: theme, keybinds, leader, attention, tabs, session.sidebar, image_preview — **no `mouse` field** | config.rs:131-165 |
| D-06 one-time `local`→`stable` channel migration (tabs, session-list) | storage.rs:191-241 | ✓ idempotent, source kept |
| in-memory only | sidebar onboarding dismissal (store.rs:3617-3627), pinned sessions (store.rs:345), scroll positions, drafts | — |

### 2.3 Persisted-state diff, severity-ranked

| # | Finding | Severity |
|---|---|---|
| S1 | **`read_tabs` wipes v2-written tabs.** v2's `tabs.json` has **no `version` field** (session-tabs.tsx:72-76; our own doc admits it, storage.rs:108-109), but `read_tabs` resets *any* doc without `version: 2` (storage.rs:336-350). First boot against a state dir used by v2 → user's tab list is destroyed and a version-stamped empty doc replaces it. One-way destructive; contradicts storage.rs:44-49's "v2 TUI and Rust TUI interoperate on the same tab list" claim. | **HIGH** |
| S2 | **Onboarding `getting-started` never persisted and never read** — `dismiss_sidebar_onboarding` (store.rs:3617-3627) has zero callers, is in-memory only, and no storage key exists. v2 persists the dismissal (footer.tsx:11-51). Confirms the registered roadmap bug ("onboarding persistence unread"). | **HIGH** |
| S3 | **model.json split-brain**: ours uses `<XDG_DATA>/opencode/tui/model.json` (app.rs:1958 `roots.data.join("tui")`) while v2 uses `<XDG_STATE>/opencode/model.json` (local.tsx:154; mini/variant.shared.ts:6 "persists in ~/.local/state/opencode/model.json"). Wrong XDG class *and* extra subdir *and* channel-visible to neither side's other binary; recent/favorite/variant preferences silently diverge between the two TUIs. Comments claim `<state>/model.json` (store.rs:2767, app.rs:1952) — code disagrees with the comment. No migration either direction. | **HIGH** |
| S4 | **Sidebar toggle persisted to the wrong place**: ours writes channel-storage `sidebar.json` (app.rs:2278-2288, storage.rs:164-174) and never writes cli.json; v2 persists the toggle through the config doc (`session.sidebar`). Neither binary reads the other's copy. | MED |
| S5 | **No config-doc writes at all**: v2 persists theme-mode pin/free (theme.tsx:233-253) and session toggles — thinking/scrollbar/grouping/verbosity (session/index.tsx:1029-1073) — through the flocked JSONC-preserving config service; every such toggle in the port is either absent or session-only. | MED |
| S6 | **Unpersisted v2 keys (no reader or writer exists)**: `layout` (two docs: verticalTabsWidth app.tsx:500, paneWidths session-frame.tsx:42), `prompt-history.jsonl` (history.tsx:93), `prompt-stash.jsonl` (stash.tsx:40 — v2's crash-safe draft recovery), `frecency.jsonl` (frecency.tsx:42), `session.json` pinned quick-switch slots (local.tsx:514-544), `directory-recents` (directory-recents.ts:13), `update-notifications` (update-notification.tsx:37), `session-terminal-selection` (session-terminals.tsx:21-22), `view`/`plugin.<id>.*` (mcp.tsx:6, plugin/api.tsx:201). | MED |
| S7 | **cli.json parse strictness**: v2 parses JSONC (`jsonc-parser`, `allowTrailingComma` — config.ts:144-146); ours is `serde_json::from_str` strict (config.rs:169-172) → a comment or trailing comma silently drops the *entire* TUI config to defaults (only a logged skip). | MED-LOW |
| S8 | No v1→v2 cli.json migration (keybind renames, `terminal.copy_on_select` → `copy` — migrate.ts:42-70) | LOW |
| S9 | Concurrency contract: v2 flock per key + fs.watch live-reload + exit `flush()` (storage.tsx:71-110, 113-133); ours atomic-rename only (storage.rs:302-315, documented divergence) — SIGKILL between tab change and save iteration loses the last write; no cross-process locking. | INFO |
| S10 | D-06 local→stable migration exists and is idempotent (tabs + session-list only) — matches v2's stable bake; correct. | OK |

---

## 3. Counts

* Protocol findings: **17** (we-only: P1, P3, P9, P10, P11, P12, P13; v2-only: P2, P4, P5, P6, P7, P8, P15, P16, P17; profile: P14). Severity: 2 HIGH, 5 MED-HIGH/MED, rest LOW/INFO.
* Persisted-state findings: **10** (3 HIGH, 3 MED, 2 LOW/MED-LOW, 2 INFO/OK).
* Independent confirmations of previously registered items: the two roadmap bugs (mouse emission / onboarding persistence) both reproduce from source; TM-1, TM-2, TM-3, TM-4, TM-5, TM-6, TM-7, TM-8 fully confirmed; TM-9 extended (read side + host fallback); TM-10 noted.

## 4. TM-1..TM-10 cross-check

| TM | Verdict from this audit |
|---|---|
| TM-1 kitty keyboard | **Confirmed + extended** (P2): ours never queries/pushes/pops; also noting the ref never pops (we should, when implementing). |
| TM-2 live mode subscriptions | **Confirmed + extended** (P6): beyond `?2027/?2031`, v2's TUI layer itself consumes `?997;1n/2n` (theme.tsx:261-266) to trigger `refreshPalette()` — an application-level dependency the rodata-only view misses. |
| TM-3 cursor shape/color | **Confirmed** (P7); add: v2 exposes `cursor.style` in the config doc (config/index.tsx:263-266) — the fix should read it. |
| TM-4 mouse mode set | **Confirmed + extended** (P1, P9, P10): the set difference is real, but the bigger miss is the **missing `mouse` config gate** — v2's documented opt-out (`"mouse": false`, live-updated) is unimplemented, which is what makes the extra/forced emission user-hostile rather than merely divergent. |
| TM-5 window title | **Confirmed + extended** (P4): route-dependent titles and the destroy-time clear. |
| TM-6 sync 2026 | **Confirmed** (P5). |
| TM-7 cursor chatter | **Confirmed** (ratatui draw path app.rs:2808-2848). |
| TM-8 probe surface | **Confirmed** (P15). |
| TM-9 clipboard | **Extended** (P8): write side at parity (encoding + destination); **read side and host-first fallback are missing** in the port — paste-image-from-clipboard and pbcopy/xclip environments are unreachable. |
| TM-10 tmux | Informational; consistent with our unconditional-setup reading of app.rs:2168-2185. |

## 5. Top-10 worst findings

1. **S1** — `read_tabs` version-gate wipes v2-written `tabs.json` on first boot (data loss, one-way interop break).
2. **P1** — mouse capture unconditional; `tui.mouse:false` not even parsed (the user's only escape hatch is gone).
3. **S3** — model preferences at `<data>/tui/model.json` instead of v2's `<state>/model.json` (split-brain; comment/code disagree).
4. **S2** — onboarding `getting-started` never read/persisted; dismissal setter is dead code (confirms registered bug).
5. **P3** — no panic/SIGHUP-safe terminal restore: crash leaves raw mode + alt screen + mouse/focus/paste latched.
6. **P2** — kitty keyboard protocol absent (no query/push/pop) on the terminals that matter most.
7. **S5/S4** — zero config-doc writes: theme-mode pin, session toggles (thinking/verbosity/scrollbar/grouping) and the sidebar toggle never persist where v2 persists them.
8. **P8** — clipboard read side + host fallback missing (OSC-52-write-only, 100 KB cap).
9. **S6** — ten v2 persisted keys with no counterpart: prompt stash (draft recovery), prompt history, frecency, layout widths, pinned sessions, directory recents, update notifications, terminal selection, plugin/mcp keys.
10. **P6** — no live theme-flip path: `?2027/?2031` subscriptions and the `?997` handler are both absent; a terminal light/dark switch requires restart.

(Runners-up: P11 ungated BEL, P12 editor-suspend leaves `?2004/?1004` latched into `$EDITOR`, S7 strict-JSON cli.json parse silently defaults everything, P13 dead teardown comment promising kitty `a=d,d=a`.)
