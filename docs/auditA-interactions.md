# Audit A — Interactive-Surface Inventory: v2 TUI vs Rust Port

**Lane:** audit-A (read-only). **Date:** 2026-10-16. **Refs compared:**
`/opt/opencode-ref/packages/tui/src` (v2, operative keybind table `config/keybind.ts`, 243 defs)
vs `/opt/opencode-rust/crates/oc-tui` + `oc-tui-core` (rust port).

**Method.** Every `onMouseUp/Down/Move/Over/Out/Drag/DragEnd/Scroll` prop, `onClick`, `hover()`
usage, `id="…"` hit-target declaration, `keymap.dispatch(...)` call site, and every entry of the
v2 default keybind table was inventoried with file:line, then matched against the port's pointer
routing (`app.rs::mouse_event`, the store's hit registries) and keymap table
(`oc-tui-core/src/keymap/table.rs`). Classification per target:

* **(a) ported+wired** — renders and routes to the same action
* **(b) hit-dead** — the element/state renders in the port but no pointer routing reaches it
* **(c) missing** — neither rendered nor routed
* **(d) divergent** — wired but with different event semantics, gating, or arming

---

## 0. Headline counts

| Metric | Count |
|---|---|
| v2 handler props (excl. tests) | **285** (`Up` 127, `Down` 27, `Move` 10, `Over` 38, `Out` 39, `Drag/DragEnd` 15, `onClick` 26, `Scroll` 3) across 62 files |
| v2 unique semantic targets (dedup by action) | **≈120** |
| Rust pointer surfaces wired | **9** (strip select/✕/+/hover, footer-location hover+click, reasoning-group toggle, user-message actions, dialog option move+submit, backdrop dismiss) |
| **(a) ported+wired** | 9 |
| **(b) rendered but hit-dead** | 21 |
| **(c) missing entirely** | ≈78 (incl. whole diff viewer, wheel scroll, drag, middle/right-click, selection copy) |
| **(d) divergent** | 8 (incl. 1 *inverse* bug: rust arms hits v2 would not — stale-registry phantom clicks) |
| v2 keybind defs with **no rust table entry** | **53** (incl. all 12 `which-key.*`, the diff scroll family, terminal pane trio, plugins/dialog-integration/dialog-plugins families) |
| Rust table entries not in v2 table | 6 (`leader`, `app.heap_snapshot`, `composer.close/tab.prev/tab.next` [layer-registered in v2 — legit], `dialog.project_copy.generate` [v2 id is `dialog.worktree.generate`]) |
| Rust table entries with **divergent defaults** | 5 (`diff.toggle/expand/expand_all/collapse/switch_focus` — v2 ships `"none"` "deprecated: file tree is mouse-controlled"; rust binds `enter,space/right/E/left/tab`) |

The port's *keyboard* surface is close to parity (207/243 table entries, most dispatchers wired);
the *pointer* surface is ~8% parity. Every v2 surface that is not one of the 9 wired ones is dead
or absent.

---

## 1. Rust pointer routing — the complete wired surface

`crates/oc-tui/src/app.rs:292 mouse_event` is the port's *entire* pointer routing:

| # | Surface | Event | Action | v2 source |
|---|---|---|---|---|
| 1 | Tab strip: tab | Moved/Up | hover lift / select | `session-tabs.tsx:1745` |
| 2 | Tab strip: `✕` | Moved/Up | reveal / close | `session-tabs.tsx:1654-1754` |
| 3 | Tab strip: `+` | Moved/Up | hover / add (home route) | `session-tabs.tsx:1786` |
| 4 | Footer `{dir}:{branch}` | Moved | hover lift | `prompt/index.tsx:1927-1934` |
| 5 | Footer `{dir}:{branch}` | Up | open WorkingDirectory select | `working-directory-actions.tsx:64` |
| 6 | Transcript reasoning header | Up | toggle group | `group-view.tsx:183` |
| 7 | Transcript user-message block | Up | open MessageActions | `session/index.tsx:2226` |
| 8 | Dialog option rows | Moved/Up | moveTo / select+submit | `dialog-select.tsx:762-774` |
| 9 | Dialog backdrop (outside release) | Up | close | `ui/dialog.tsx:34-63` |

Event support: **only** `Moved` and `Up(Left)`. No `Down`, no drag, no wheel
(`MouseEventKind::ScrollUp/Down` unhandled anywhere in the crate), no middle/right button.

---

## 2. v2 target inventory → classification

### 2.1 App chrome

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Root right-click selection copy (manual mode) | `app.tsx:1326` | **(c)** | no selection model in port |
| Root copy-on-select release | `app.tsx:1334` | **(c)** | idem |
| Tabs-strip resize drag (vertical strip width) | `app.tsx:1338-1345` | **(c)** | no drag events |
| Session pane resize handle (down/hover/finish) | `pane-resize-handle.tsx:21`, `pane-resize.ts:49,74`, `session-frame.tsx:66,270` | **(c)** | sidebar width is fixed/key-toggled only |
| **Wheel scroll → transcript scrollbox** | `session-frame.tsx:310`, `session/index.tsx`, `diff-viewer.tsx` | **(c)** | mouse wheel does nothing anywhere in the port |
| Overlay release → focus session pane | `session-frame.tsx:318` | **(c)** | |
| Terminal pane mousedown → interact/focus | `terminal-pane.tsx:293` | **(c)** | terminal pane feature absent (`terminal.toggle/select/close` also unbound) |
| DevTools panel focus | `panel-host.tsx:34` | **(c)** | devtools bar absent |

### 2.2 Session tabs strip (`component/session-tabs.tsx`, `session-tabs-rail.tsx`)

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Tab release → select | `1745` | **(a)** | wired (`strip_hit`) |
| Hovered tab `✕` → close | `1654-1754` | **(a)** | one-cell zone, golden-pinned |
| `+` → add tab (home) | `1786` | **(a)** | fixed for home in dogfood2 (gate is route-complete now, `app.rs:306-309`) |
| Hover marquee + `✕` reveal | `893`, `1096` | **(a)** | hover only; marquee animation n/a |
| Middle-click tab → close | `893`, `1164` | **(c)** | no middle button support |
| Right-click tab / `+` → context menu | `1164`, `1654`; menu rows `487`; backdrop `455` | **(c)** | no context menu model |
| Drag to reorder tabs | `717`, `1547` (`onMouseDrag`) | **(c)** | no drag events |
| Close-hold on tab (press-and-hold) | `releaseCloseHold` paths `1164/1654` | **(c)** | |
| New-tab `×` → close strip/add | `1225` | **(c)** | |
| Overflow chevrons `‹n`/`n›` | `1547` area | **(c)** | unverified clickability in v2; strip overflow renders in port without hit |
| Vertical rail rows | `session-tabs-rail.tsx:38-42` | **(c)** | vertical orientation absent |
| `+` right-press → rail context menu | `session-tabs-rail.tsx:38` | **(c)** | |

### 2.3 Home route

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Update banner → dismiss/open-update/exit | `routes/home.tsx:150-152` | **(c)** | update-notification surface absent in port |
| Footer `⊙ N MCP` chip → `mcp.list` | `feature-plugins/home/footer.tsx:24` | **(b)** | renders via `PluginQuickAction` label (`home.rs:96-100`, `plugin_boot.rs:380`) on the footer row — **no hit rect**; also no failure-color state |
| Footer `⊙ N plugins failed` chip → `plugins.list` | `footer.tsx:64` | **(b)** | idem; `plugins.list` also has no keymap entry (§3) |
| Home status-row `{dir}:{branch}` | (Prompt footer, shared) | **(d)** | label + hover state render (`home.rs:114-115,326-328`) but `footer_location_hit` is **only set by the session render** (`session.rs:613`) — never armed on home; see stale-hit hazard §4.1 |

### 2.4 Session transcript (`routes/session/index.tsx`, `message-parts.tsx`, `group-view.tsx`)

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| "Jump to latest ↓" → scroll bottom | `index.tsx:1429-1437` | **(c)** | no button rendered; keyboard `session.last` exists |
| Tokens debug disclosure (`+ Tokens: …`) | `index.tsx:1650-1658` | **(c)** | `config.debug.turn_tokens` surface absent |
| Child-session completion row → navigate | `index.tsx:1976-1983` | **(c)** | row renders in port? no hit either way |
| Reverted banner → `revert.clear` | `index.tsx:2101-2135` | **(c)** | |
| User message block → MessageActions | `index.tsx:2222-2250` | **(a)** | wired, tight rect golden-pinned |
| User message (pending steer) → steer select | `index.tsx:2229-2241` | **(d)** | port opens MessageActions for *all* user blocks — the pending-steer branch (Queue/Delete) is not modeled |
| Queued dock → open manager | `index.tsx:2318-2326` | **(b)** | `paint_queued_dock` renders (`session.rs:470`) — no hit routing |
| Assistant image previews → preview dialog | `index.tsx:2485` | **(c)** | images render; click + `DialogImagePreview` absent |
| Generic tool row → expand | `index.tsx:2548`, `2650-2670` | **(c)** | InlineToolRow click model absent |
| Failed tool row → error expand / `onErrorClick` | `index.tsx:2656-2668` | **(c)** | |
| BlockTool (file/patch cards) → expand | `index.tsx:2717`, `2905` | **(c)** | |
| Task row → navigate to child | `index.tsx:3107` | **(c)** | |
| Execute call row → expand | `index.tsx:3155-3170` | **(c)** | |
| Execute row → details dialog | `index.tsx:3218` | **(c)** | `DialogExecute` absent |
| Reasoning header (minimal mode) toggle | `message-parts.tsx:80` | **(b)** | `thought:{mid}:{pid}` rows are keyed (`message/list.rs:216`) but `session.rs:288` arms **only `group:Reasoning:`** keys |
| Tool group "Exploring:" disclosure | `group-view.tsx:146-190` | **(b)** | `group:Exploration:…` keys emitted (`message/list.rs:526`) — never armed |
| Reasoning group disclosure | `group-view.tsx:183` | **(a)** | the one wired group kind |
| Activity group (low verbosity) | `group-view.tsx:219` | **(c)** | verbosity model absent (`session.verbosity.cycle` unbound) |
| Instructions group disclosure | `group-view.tsx:256` | **(b)** | `group:Instructions:…` keys emitted — never armed |

### 2.5 Composer / prompt (`component/prompt/*`, feature-plugins/prompt/*)

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Attachment image previews → preview dialog | `prompt/index.tsx:1695` | **(c)** | attachment thumbnails render (`composer/attachment.rs`) — no click |
| "+N more" images → preview | `prompt/index.tsx:1731` | **(c)** | |
| Pasted-text extmark → expand (mousedown) | `prompt/index.tsx:1814` | **(c)** | |
| Footer location label | `prompt/index.tsx:1927-1934` | **(a)** | session route only (see §4.1 for home/sidebar gaps) |
| Live subagents/shells chip → `session.child.first` | `feature-plugins/prompt/footer.tsx:66-88` | **(c)** | chip not rendered in port's statusline (`composer/render.rs` = agent · model · menu) |
| btw card `✕` / copy | `feature-plugins/prompt/btw.tsx:136,162` | **(c)** | btw card absent |
| Autocomplete rows (move/down/up → select) | `prompt/autocomplete.tsx:907-909` | **(b)** | popup renders (`AutocompleteView`, `home.rs:313`) — no pointer routing |

### 2.6 Composer picker panel (`routes/session/composer/*`)

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| `esc` close chip | `composer/index.tsx:123` | **(b)** | hint row renders (`subagent_picker.rs:134-146`) — no hit |
| Subagent rows (move+up → navigate) | `subagents-tab.tsx:222-223` | **(b)** | rows render; keyboard-only in port |
| Shell rows | `shell-tab.tsx:128-129` | **(b)** | |
| Terminal rows | `terminals-tab.tsx:87-88` | **(b)** | terminals data layer absent, rows unreachable |
| Picker tab labels | — | n/a | not clickable in v2 either (keyboard `←/→`) — parity |

### 2.7 Dialogs

Rust dialog kinds: `CommandPalette, SessionRename, ThemeList, Help, ServerList, Worktrees,
WorktreeName, WorktreeDeleteConfirm, MessageActions, WorkingDirectory, Plugin, QueuedPrompts`
(`store.rs:87-134`). All select-style bodies share the wired option-row routing (§1 #8).

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Backdrop outside-release dismiss | `ui/dialog.tsx:34-63` | **(a)** | |
| Inner chrome stop-propagation | `ui/dialog.tsx:57` | **(a)** (by construction) | inside-frame release is a no-op |
| Per-dialog muted `✕` close label | every dialog header (22 files, e.g. `dialog-help.tsx:25`, `dialog-confirm.tsx:64`, `dialog-update.tsx:98`) | **(c)** | **the `✕` glyph is not rendered at all** in the port's dialog chrome — ~22 instances |
| Select rows move/select | `dialog-select.tsx:762-774` | **(a)** | |
| Select action rows (`trigger(item)`) | `dialog-select.tsx:643` | **(a)** | same option-row routing |
| Select footer cancel | `dialog-select.tsx:681` | **(c)** | not rendered |
| Prompt-dialog submit | `dialog-prompt.tsx:106` | **(c)** | enter-only in port |
| Confirm OK / cancel | `dialog-confirm.tsx:64,78` | **(c)** | generic confirm dialog absent |
| Alert OK | `dialog-alert.tsx:36,48` | **(c)** | |
| Help close button | `dialog-help.tsx:39` | **(c)** | |
| Export-options format rows + 4 toggles + copy + export | `dialog-export-options.tsx:110-263` | **(c)** | export dialog absent; `session.export` also unhandled by dispatcher |
| Export-result open/close | `dialog-export-result.tsx:33,45` | **(c)** | |
| Error-details back/investigate/copy | `dialog-error-details.tsx:87-124` | **(c)** | |
| Debug-dialog copy | `dialog-debug.tsx:61,83` | **(c)** | |
| Image preview `✕`/prev/next | `dialog-image-preview.tsx:45-65` | **(c)** | |
| Shell-output copy code/output | `dialog-shell-output.tsx:109` (+ copy rows) | **(c)** | |
| Execute-dialog copy code/output | `dialog-execute.tsx:109,164,170` | **(c)** | |
| Status `✕` | `dialog-status.tsx:25` | **(c)** | `opencode.status` opens a generic ServerList instead — approximated |
| Pair `✕` | `dialog-pair.tsx:95` | **(c)** | `server.pair` unbound in table |
| Integration `✕` ×2 | `dialog-integration.tsx:451,746` | **(c)** | |
| Update dialog buttons (Update/Dismiss/Restart) | `dialog-update.tsx:98,149` | **(c)** | |
| Worktree-name `✕` | `dialog-worktree-name.tsx:55` | **(c)** | dialog exists, `✕` not rendered; tab-generate keybind id diverges (§3) |
| Workspace file-changes rows | `dialog-workspace-file-changes.tsx:122` | **(c)** | only the delete-confirm role ported |

### 2.8 Permission prompt (`routes/session/permission.tsx`)

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| `enter confirm` button | `permission.tsx:381` | **(b)** | body renders (`footer/permission.rs`) — pointer routing **returns early** when `permission_live()` (`app.rs:420-422`); keyboard enter works |
| `esc cancel` button | `permission.tsx:395` | **(b)** | |
| Option pills (move+up → select) | `permission.tsx:583-584` | **(b)** | |
| Mini permission pills | `mini/footer.permission.tsx:57-60` | **(b)** | renders (`footer/permission.rs`) — no routing |

### 2.9 Session form (`routes/session/form.tsx`)

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Field tabs (hover+up → switch) | `form.tsx:810-830` | **(c)** | session-route form UI absent (mini footer form exists, §2.10) |
| Option rows / custom row | `form.tsx:923-979` | **(c)** | |
| External URL open / copy / cancel | `form.tsx:860,1161,1165` | **(c)** | |

### 2.10 Mini footers

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Form option rows (move+down+up → choose) | `mini/footer.form.tsx:441-443` | **(b)** | renders (`footer/form.rs`) — no routing |
| Form custom-answer row | `mini/footer.form.tsx:491` | **(b)** | |
| Subagent footer close / cycle | `mini/footer.subagent.tsx:239,259` | **(b)** | renders — no routing |

### 2.11 Sidebar + feature plugins (`feature-plugins/sidebar/*`)

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| **Onboarding card `✕` → dismiss (persisted)** | `sidebar/footer.tsx:45-55` | **(b)** | card + `✕` render (`sidebar.rs:277-317`), store API exists (`store.rs:3620 dismiss_sidebar_onboarding`) — **no hit routing** (dogfood3's known bug; still open) |
| Onboarding "Connect provider" row → `provider.connect` | `sidebar/footer.tsx:61-70` | **(b)** | renders — no routing |
| Sidebar footer location → working-directory menu | `sidebar/footer.tsx:96-110` | **(b)** | location line renders (`sidebar.rs:136-137`) — the *single* `footer_location_hit` rect belongs to the composer footer, not the sidebar's copy |
| MCP section header → collapse toggle | `sidebar/mcp.tsx:29` | **(c)** | sections render as static title+rows |
| MCP server rows → server dialog | `sidebar/mcp.tsx:56` | **(c)** | no `DialogMcp` |

### 2.12 Diff viewer (`feature-plugins/system/diff-viewer*.tsx`)

The entire surface is **(c) missing**: the port's `components/diff_viewer.rs` is compiled and
golden-tested but **mounted nowhere** — no route, no dispatcher arm for `diff.open`, no palette
entry (`command_palette.rs` has zero `"diff…"` arms). 15 v2 targets dead by unreachability:
file-tree rows (`diff-viewer-file-tree.tsx:77,182-185`), file-menu items
(`diff-viewer-file-menu.tsx:51-74`), image open (`diff-viewer-image.tsx:66,82`), hunk/file header
menus (`diff-viewer.tsx:783,819,951`), close `✕` (`:1149`). The 24 `diff.*` keymap entries ship in
the rust table but cannot fire (no surface, no dispatcher).

### 2.13 Misc components

| v2 target | v2 source | Class | Notes |
|---|---|---|---|
| Toast hover (pause) + click → activate action | `ui/toast.tsx:57-59` | **(b)** | toast renders; `activate()` exists (`toast.rs:111-116`) but nothing calls it from input |
| Link clicks in markdown → open URL | `ui/link.tsx:27` | **(c)** | no link hit model |
| Error-component action rows | `error-component.tsx:157-158` | **(c)** | |
| Plugin-route-missing → home | `plugin-route-missing.tsx:12` | **(c)** | |
| DevTools bar (~18 targets) | `devtools-bar.tsx:242-532` | **(c)** | dev-only (`app.debug`), lower severity |
| Storybook tabs resize | `storybook/session-tabs.tsx:504` | **(c)** | dev-only |

---

## 3. Keymap diff (v2 default table → rust table)

**53 v2 bindings have no rust table entry** (`oc-tui-core/src/keymap/table.rs`). Boot-reachable
ones first:

### Daily-driver gaps (bound by default in v2 → dead key in port)

| v2 binding | v2 default | Impact |
|---|---|---|
| `diff.down` / `diff.up` | `j,down` / `k,up` | diff scroll — moot until viewer mounts, but table parity |
| `diff.page.up/down`, `diff.half_page.up/down`, `diff.first`, `diff.last` | pgup/pgdn/ctrl+f/b, ctrl+d/u, `gg,home`, `shift+g,end` | idem |
| `diff.mark_reviewed` | `m` | idem |
| `terminal.toggle` / `terminal.select` / `terminal.close` | `<leader>t` / `<leader>down` / `<leader>up` | terminal pane family — unbound **and** feature absent |
| `composer.terminal.up/down/select` | `up,k`/`down,j`/`return` | picker's Terminals tab |
| `session.tab.history.forward` | `ctrl+i` | tab history forward |
| `queued_prompt.undo` | `ctrl+u` | queued-prompt undo |
| `app.clear` | `ctrl+l` | clear screen (mini) |
| `which-key.toggle` + 11 siblings | `ctrl+alt+k` family | no which-key panel in port (v2 tui src has no component consumer either — likely keymap-addon provided; flag for confirmation) |

### Palette-reachable but unbound (v2 default `"none"`, command exists there, absent here)

`app.debug`, `app.scrap`, `location.reload`, `opencode.settings` (settings has a ServerList stub in the dispatcher but no table entry — **a palette/`ctrl+…` override on it is silently ignored**), `server.pair`, `service.restart`, `session.aside`, `session.cd`, `session.copy.id`, `session.tab.history.back`, `session.toggle.exploration_grouping`, `session.verbosity.cycle`, `plugins.list`, `plugins.install`, `dialog.integration.rename/delete`, `dialog.plugins.error/install/update/check`, `dialog.worktree.generate` (see divergence below).

**Rust-only entries:** `leader` (token, fine), `app.heap_snapshot` (port addition), `composer.close/tab.prev/tab.next` (v2 registers these as layer commands with `bind:` props — `composer/index.tsx:77-80` — legit parity), and `dialog.project_copy.generate` — **divergent id**: v2's table key is `dialog.worktree.generate` (`config/keybind.ts:267`, default `tab`); a user override keyed `dialog.worktree.generate` will not reach the port's `dialog.project_copy.generate`.

**Divergent defaults (rust ≠ v2):**

| Command | v2 | Rust |
|---|---|---|
| `diff.toggle` | `none` (deprecated) | `enter,space` |
| `diff.expand` | `none` (deprecated) | `right` |
| `diff.expand_all` | `none` (deprecated) | `E` |
| `diff.collapse` | `none` (deprecated) | `left` |
| `diff.switch_focus` | `none` (deprecated) | `tab` |
| `diff.next_file` / `diff.previous_file` | `n,alt+down` / `p,alt+up` | `n` / `p` (missing alt+arrow) |
| `diff.help` | `?,shift+?,shift+/` | `?` |
| `session.sidebar.toggle` layer | session-frame layer (`index.tsx:245` escape/ctrl+c close) | base layer only |

The five deprecated diff tree bindings are v1-era residue: v2 removed tree keyboard control
("file tree is mouse-controlled", `config/keybind.ts:74-79`) but the rust table still binds
`enter/space/right/left/E/tab` — those keys will do tree things the day the viewer is mounted.

**Dispatcher gaps (table entry exists, no handler):** `session.export`, `session.delete`,
`session.timeline`, `session.compact`, `session.undo/redo`, `prompt.queue`, `prompt.images.view`,
`prompt.stash*`, `session.background`, `terminal.suspend/title.toggle`, `docs.open` (opens list
stub), the whole `diff.*` layer — all fall to `Dispatch::Unknown` (`command_palette.rs:1110`).

---

## 4. Hazard classes (systematic, not just per-target)

### 4.1 Stale hit registries — phantom clicks after route change *(inverse of dogfood2; NEW finding)*

`set_transcript_hit_rows` (`session.rs:279`) and `set_footer_location_hit` (`session.rs:613`) are
called **only** by the session render. `render_home` (home.rs) and `navigate()` (`store.rs:2940`)
never clear them, and `mouse_event`'s `Up(Left)` arm loops `transcript_hit_rows()` and checks
`footer_location_hit()` **without any route gate** (`app.rs:395-446`). After Session → Home:

* a release landing on a stale transcript row fires `toggle_group_expanded` / opens
  `MessageActions` for the *previous* session while the home screen is showing;
* a release on the stale footer-location rect opens the WorkingDirectory dialog with the previous
  session's directory; `Moved` events can flip the home label's hover state via stale coords.

v2 has no such hazard — hit targets are per-render-frame renderables. The port needs
registry clearing on route change (or route gating in `mouse_event`).

### 4.2 Conditional arming that gates on the wrong predicate

* **Transcript groups:** v2 arms 4 disclosure kinds + minimal-mode thought headers; the port arms
  only `group:Reasoning:` (`session.rs:288`). Exploration/Instructions/`thought:` rows render with
  the same visual affordance and silently swallow clicks.
* **Permission:** pointer routing returns early under `permission_live()` (`app.rs:420`) — correct
  for the transcript, but it also kills the *permission's own* buttons/pills (v2 renders them as
  the screen's only interactive surface).
* **Dialogs:** option-row routing is correctly modal-first; but any *non-row* control inside a
  dialog (per-dialog `✕`, copy labels, export toggles) has no routing at all — the port's dialog
  interiors are keyboard-only except for option rows.

### 4.3 First-use / conditional-Show states

* **Sidebar onboarding** (`Show when={showOnboarding()}` — height ≥ 22, no connected provider):
  ported visually + storage-backed (`sidebar_onboarding` signal), dismissal is **mouse-only in
  v2** — so in the port it is *undismissable by pointer* and there is no keyboard fallback. Every
  first-run user hits this. (Connect-provider row same.)
* **Home strip arming** (dogfood2): now fixed — `strip_live` gates on tabs-enabled + no dialogs +
  no permission + non-empty tabs, route-agnostic (`app.rs:306-309`), regression-pinned.
* **Update notification / reconnect banner** (`Show when` gates in `home.tsx:1439-1476`):
  absent — first-install and reconnect UX has no interactive surface in the port.

### 4.4 Event-semantics divergences on wired targets

* **Backdrop:** v2 closes on any *inside-backdrop* release unless a selection was dragged
  (`dialog.tsx:30-63`'s `dismiss` flag); port closes on outside-frame release only — close enough,
  but the selection-guard behavior is unmodelled (no selection).
* **Dialog rows:** v2 `mousedown` moves selection *and* `mouseup` submits; port `Moved` moves +
  `Up` submits — releases without prior motion over the row still work (parity OK).
* **Strip `✕`:** v2 requires hover-armed `✕` (`hovered() === tab.sessionID`); port's `strip_hit`
  models the same one-cell zone — parity, golden-pinned.

---

## 5. Top-10 worst findings (user-impact order)

1. **No mouse-wheel support anywhere** — `MouseEventKind::Scroll*` unhandled; v2 scrolls the
   transcript, diff viewer, and frame via wheel (`session-frame.tsx:310`). Daily-driver.
2. **Stale hit registries → phantom clicks on Home** (§4.1) — the same *class* as the dogfood2
   home-strip bug, inverted: hits stay armed after leaving the session route.
3. **Sidebar onboarding is undismissable** — `✕` and "Connect provider" render with storage + store
   API but no pointer route (`sidebar/footer.tsx:48,66` vs `app.rs:292-450`); no keyboard fallback
   exists in v2 either. Every first-run user.
4. **Permission prompt is pointer-dead** — confirm/cancel buttons + option pills render
   (`footer/permission.rs`) but `mouse_event` early-returns under `permission_live()`
   (`app.rs:420-422`). Permission flows are the highest-stakes clicks in the app.
5. **Transcript disclosure arming is 1-of-5** — only `group:Reasoning:` rows toggle; Exploration
   groups, Instructions groups, activity groups, and minimal-mode thought headers render as
   clickable and do nothing (`session.rs:288` vs `message/list.rs:216,526`).
6. **Every dialog `✕` missing** (~22 instances) — v2 closes every dialog via a header `✕`; the
   port renders none of them (backdrop/escape only). Combined with per-dialog button rows
   (confirm OK, export options, update actions, error-details) being entirely absent.
7. **Diff viewer is dead code** — `diff_viewer.rs` mounts nowhere; `diff.open` has no dispatcher
   arm or palette entry; 15 pointer targets + the entire `diff.*` key layer unreachable; and the
   table's 5 deprecated v1 bindings will misfire once it mounts.
8. **Tab strip advanced interactions absent** — middle-click close, right-click context menus,
   drag reorder, close-hold, vertical rail (`session-tabs.tsx:455-1786`); power users lose all of
   them, and 53 v2 keybind defs (terminal trio, plugins/integration dialog families, which-key,
   queued_prompt.undo, session.tab.history.forward) have no table entry.
9. **Tool-row interaction model absent** — generic tool expand, failed-tool error expand, BlockTool
   cards, task-child navigation, execute details: 10 visible transcript affordances
   (`session/index.tsx:2548-3218`) are non-interactive; also no clickable links (`ui/link.tsx`).
10. **Toast action is dead** + home footer chips hit-dead — toast click-to-activate renders an
    action label nothing can trigger (`toast.rs:111`); MCP/plugins failure chips draw on the home
    footer row without hit rects, so "N MCP failed" cannot be clicked open.

### Suggested fix order (severity × reach)

1. Clear/gate hit registries on route change (§4.1) — correctness, small diff.
2. Wire permission buttons/pills (bypass the early return with a permission branch).
3. Wire sidebar onboarding `✕` + connect row (registry + 2 rects).
4. Extend `ToggleGroup` arming to `group:Exploration:`, `group:Instructions:`, `thought:` keys.
5. Add `ScrollUp/Down` → transcript scroll in `mouse_event`.
6. Render + wire the shared dialog `✕` (one chrome change covers ~22 dialogs).
7. Home footer chips → `mcp.list`/`plugins.list` hit rects; toast click → activate.
8. Then the long tail (tab middle/right/drag, picker rows, autocomplete rows, queued dock).

---

## Appendix A — v2 handler-prop census (file → counts)

`onMouseUp` 127, `onMouseDown` 27, `onMouseMove` 10, `onMouseOver` 38, `onMouseOut` 39,
`onMouseDrag/DragEnd` 15, `onClick` 26 (devtools), `onMouseScroll` 3. Full grep output preserved
at `/tmp/opencode/auditA/ref-mouse.txt` (193 Up/Down/Move/Over/Out/onClick/hover lines with
file:line). Key files: `session/index.tsx` (17), `session-tabs.tsx` (16), `form.tsx` (12),
`dialog-select.tsx` (6), `dialog-export-options.tsx` (7), `devtools-bar.tsx` (18), `diff-viewer*.tsx` (12),
`mini/footer.*` (8), `permission.tsx` (5), `prompt/index.tsx` (5), `sidebar/*` (6), `composer/*` (7).

## Appendix B — keymap diff lists

**Ref-only (53):** `app.clear, app.debug, app.scrap, composer.terminal.down, composer.terminal.select,
composer.terminal.up, dialog.integration.delete, dialog.integration.rename, dialog.plugins.check,
dialog.plugins.error, dialog.plugins.install, dialog.plugins.update, dialog.worktree.generate,
diff.down, diff.first, diff.half_page.down, diff.half_page.up, diff.last, diff.mark_reviewed,
diff.page.down, diff.page.up, diff.up, location.reload, opencode.settings, pane.focus.left,
pane.focus.right, plugins.install, plugins.list, queued_prompt.undo, server.pair, service.restart,
session.aside, session.cd, session.copy.id, session.tab.history.back, session.tab.history.forward,
session.toggle.exploration_grouping, session.verbosity.cycle, terminal.close, terminal.select,
terminal.toggle, which-key.end, which-key.group.next, which-key.group.previous, which-key.home,
which-key.layout.toggle, which-key.page.down, which-key.page.up, which-key.pending.toggle,
which-key.scroll.down, which-key.scroll.up, which-key.toggle`

**Rust-only (6):** `leader, app.heap_snapshot, composer.close, composer.tab.next, composer.tab.prev,
dialog.project_copy.generate`

**Ref `keymap.dispatch` call sites from UI (7):** `mcp.list` ×3, `plugins.list` ×3, `session.list`,
`session.child.first`, `provider.connect`, `prompt.paste` — all in home footer chips, prompt footer
chip, sidebar footer rows (see §2.3/§2.5/§2.11 for their wiring state).

**parity_tests.rs coverage note:** the existing spec asserts ~59 behaviors dominated by
input-layer strokes and a handful of base commands (`session.new/rename/interrupt/page.up`,
`model.cycle_recent`, `prompt.editor`, `open.menu`, `session.child.first`, `command.palette.show`,
`prompt.autocomplete.next`, leader chords). None of the 53 missing bindings, the 5 divergent diff
defaults, or the `dialog.worktree.generate` id divergence is asserted.
