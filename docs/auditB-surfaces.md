# Lane audit-B — v2 TUI rendered-surface inventory vs the Rust port

**Scope**: exhaustive inventory of every rendered surface in `/opt/opencode-ref/packages/tui/src` (app shell, routes, sidebar/plugin sections, composer/footer chrome, transcript rows, tabs, dialog families, overlays, diff viewer, mini) compared against `crates/oc-tui/src` (+ `oc-tui-core`). Read-only audit; the corpus rig covers 26 scripted happy paths, so this report focuses on conditional gates, state variants (first-use, empty, loading, error, overflow, tiny widths), and theme-key bindings that the rig cannot see.

**Method**: structural walk of both trees; for each ref surface its `Show`/`Match` gates, state variants, and consumed `theme.*` keys were extracted and matched against the Rust component/store. Classification legend:

- **PORTED** — renders with equivalent gates/variants/bindings.
- **VARIANT-MISSING** — surface exists but a state/conditional variant does not.
- **STUB** — command opens a dialog titled correctly whose body is the `"Not wired in this build"` placeholder (`command_palette.rs:418-421`).
- **MISSING** — no Rust surface at all (or component exists but never mounted).
- **GATE-MISMATCH** — renders under different conditions than the ref.
- **STYLE-MISMATCH** — consumes different theme keys than the ref.

---

## 1. App shell (`app.tsx` → `app.rs` + `components/dialog.rs::render_app`)

| Ref surface | Gate / variants | Rust | Class |
|---|---|---|---|
| Route switch home/session/plugin | `Switch` over `route.data.type`; plugin route has `PluginRouteMissing` fallback | `render_app` matches `Route::Home \| Session` only (`dialog.rs:430-440`) | **MISSING** plugin routes + missing-route fallback |
| Horizontal session tabs | `tabsVisible() && !tabsVertical()` | `render_session` always paints strip when tabs exist (`session.rs:126-133`) | PORTED (windowing, unread `•`, permission `!`, hover-close, ` + `) |
| Vertical session tabs + `PaneResizeHandle` + persisted width | `tabs.layout === "vertical" && sessionTabsFitVertically()`; drag resize; storage `layout.verticalTabsWidth` | component math exists (`session_tabs.rs`, `SIDEBAR_WIDTH`) but never mounted; no `tabs.layout` config | **MISSING** (unmounted) |
| `StartupLoading` | shows after 500 ms when plugins not ready, holds 3 s; "Loading plugins…"/"Finishing startup…" | none | **MISSING** (boot-reachable) |
| `Reconnecting` | delayed 1 s (retry) / 5 s (initial); scrim `RGBA(0,0,0,150)`; managed vs unmanaged copy | none — no connection overlay at all | **MISSING** (daily-driver state) |
| `MigrationOverlay` | polls `migration.v1.status`; progress `n/d`; error → toast | none | **MISSING** |
| `DevToolsBar` | `config.debug.devtools \|\| channel === "local"` | none | **MISSING** (dev-only) |
| `ThemeErrorToast` | theme load errors → error toast | none | **MISSING** |
| `Toast` | variants info/success/warning/error, action label, pause-on-activity, pending count | `components/toast.rs` — all present incl. action + pending | PORTED |
| `ErrorBoundary → ErrorComponent` | render-crash fallback screen | none | **MISSING** |
| Terminal title (OSC 2) | per route/session, 40-char truncate, fallback "OpenCode", config gate | only OSC 777 notify (`os_notify.rs`) | **MISSING** |
| MCP needs_auth/failed toasts | once per server+state, action "Open MCP servers" → `mcp.list` | no such toast copy anywhere | **MISSING** |
| Mouse: selection copy, copy-on-select, right-click copy | config `terminal.copy` | partial (roadmap notes "mouse selection swallowed") | VARIANT-MISSING |

## 2. Home route (`routes/home.tsx`, `feature-plugins/home/footer.tsx` → `components/home.rs`)

| Ref surface | Gate / variants | Rust | Class |
|---|---|---|---|
| Logo block | hidden below 12 rows; centered; half-pixel rounding | `home.rs` `show_logo = height >= 12`, x pinned | PORTED |
| Prompt with rotating placeholders | `placeholder.normal`/`shell` pools; narrow inset `width < 44 ? 1 : 2` | rotation pools + inset ported | PORTED |
| `UpdateNotification` | update available / installed / remote-server variants; `/update`·`/exit` prefix; hover bg `action.primary.hovered`; FadeInText | none | **MISSING** (first-use surface) |
| Home footer | whole row hidden `h < 12 \|\| w < 44`; pad variant `h < 16`; MCP `⊙ N MCP` (success dot; `N MCP failed` error dot; `/mcps` hint at `w >= 64`); plugins-failed row (`/plugins` at `w >= 80`); version right-aligned (`w >= 64`) | footer geometry + version + plugin hint segments ported; **no MCP row, no plugins-failed row** (store carries no MCP/plugin status — `home.rs` header comment) | VARIANT-MISSING (2 of 4 cells) |
| Global FormPrompt overlay | global forms preempt the prompt; `disabled={forms.length > 0}` | no global-form handling on Home | **MISSING** (Home variant) |

## 3. Session route (`routes/session/index.tsx` + `session-frame.tsx` → `components/session.rs`, `session_screen.rs`)

| Ref surface | Gate / variants | Rust | Class |
|---|---|---|---|
| Transcript scrollbox + scrollbar | scrollbar `visible` gate, track colors `decrease(raised)/border.base`; sticky bottom; scroll acceleration | follow/jump logic + jump slot ported; scrollbar track uses different keys / minimal | PORTED (minor) |
| "Loading session history…" | `firstJump()` (anchor navigation warming up) | none | **MISSING** |
| "Jump to latest ↓" | `!firstJump && awayFromBottom`; fg `text.feedback.info.base`, hover `text.action.secondary.hovered`; click→bottom | ported (`message/list.rs:574-586`) but fg `text_muted` | **STYLE-MISMATCH** |
| `RevertMessage` banner | `session.revert.messageID`; "N messages reverted"; files list w/ +/− counts; hover lift; click clears | none (revert only exists as a Message-Actions entry) | **MISSING** |
| `BackgroundToolHint` | hint row under last message while background tools run | none | **MISSING** |
| Navigation slack spacer / anchors | `navigationSlack()`, `anchor-view` | not present | **MISSING** (navigation jump granularity) |
| Queued prompt dock | `!composer.open && !disabled && queued > 0`; `┃ N queued · next` | `paint_queued_dock` ported (`session.rs:466`) | PORTED |
| Bottom chrome switch | composer-open → null; permissions[0] → `PermissionPrompt`; forms[0] → `FormPrompt`; location-missing → `SessionLocationMissing`; else `Prompt` | FooterView machine: Permission/Form pre-empt Prompt (`footer/mod.rs`) | PORTED (see §5 for variant depth) |
| `SessionLocationMissing` | when current location errored; opens DialogWorkspaces to move | none | **MISSING** |
| Right pane: sidebar / terminal / panel + `PaneResizeHandle` + fullscreen panels | `rightPane()` 3-way; panel fullscreen; `session.sidebar.toggle` | sidebar auto/hide only; no terminal pane, no panels | VARIANT-MISSING (2 panes missing) |
| Sidebar auto gate | `wide = width - verticalTabsWidth > 120` | `SIDEBAR_AUTO_BREAKPOINT` (>120) — ignores vertical-tabs subtraction (moot while vertical tabs unmounted) | PORTED (degenerate) |

## 4. Sidebar (`routes/session/sidebar.tsx` + `feature-plugins/sidebar/*` → `components/sidebar*.rs`)

| Ref surface | Gate / variants | Rust | Class |
|---|---|---|---|
| Pane + bold title (title_shimmer, pending-rename dim when animations off) | `session()` gate; scrollbox w/ absolute scrollbar | pane + title ported; shimmer present | PORTED |
| Context section | `state() \|\| cost > 0`; tokens (locale), `% used` when defined, `$X.XX spent` | `SidebarView.sections.context` **always empty in live mount** (`session.rs:456-459`) | **MISSING** (live) |
| MCP section | `list.length > 0`; collapsed `▶ MCP (N active, M errors)` when >2 & closed; per-server dot (success/error/warning/muted); status text Connected/Connecting/Error/Disabled/Sign in; click → DialogMcp | `sections.mcp` **always empty in live mount** | **MISSING** (live) |
| Getting-started card | `height >= 22 && !dismissed && !hasConnectedProvider(integrations)`; ✕ dismiss persists; Connect provider row → `provider.connect` | `onboarding: height >= 22 && !dismissed` (`session.rs:461`) — **provider gate absent**; ✕ click target dead (dogfood3) | **GATE-MISMATCH** |
| Footer location line | `{dir}:{branch}` via FilePath maxWidth 38; hover `text.base`; click → working-directory actions | `location: None` in live mount — the row never renders | **MISSING** (live) |

## 5. Composer & footer chrome (`component/prompt/index.tsx`, `feature-plugins/prompt/*` → `components/composer*`, `footer/`)

| Ref surface | Gate / variants | Rust | Class |
|---|---|---|---|
| Prompt box, border rail, `╹▀` transition, caret, placeholder | plus rail color per agent | ported (`composer_solid`, `render.rs`) | PORTED |
| Metadata row (`Build · Model · variant · auto`) | agent color, provider muted, variant chip | `MetadataLine` ported | PORTED |
| Footer hints | location (+hover lift), agents, commands shortcuts, usage `N (P%)`, interrupt `esc interrupt/again` | `FooterHints` ported incl. interrupt + hover | PORTED |
| Live subagent/shell counts | `N subagents · M shells`; hover; click → `session.child.first`; drops agents hint when usage present | none | **MISSING** |
| Cost display | `$X.XX spent` when > 0 | none | **MISSING** |
| Shell mode (`!` prefix) | distinct footer `esc exit shell mode` / `esc shell` (<44); different submit semantics | no shell mode in composer | **MISSING** |
| Image preview attachments | `config.prompt.image_preview` gate; chips; "N more" overflow row | attachment parsing exists; no preview surface, no overflow row | VARIANT-MISSING |
| Autocomplete | `/command`, `@file` (+directory completion recents, skills, agents, variants entries) | Command + Reference(@file) modes only | VARIANT-MISSING |
| Prompt history | up/down recall | `PromptHistory` ported | PORTED |
| `/btw` side-question | spinner `/btw` in footer status slot + answer dialog | none | **MISSING** |
| Editor context label / move progress | `editorFileLabel`; `move.progress`/`pendingNew` rows | none | **MISSING** |
| Subagent picker (child-session composer: Subagents/Shell/Terminals tabs) | open on parent session route; tab labels, esc, ←/→ hints | `subagent_picker.rs` ports the panel + tabs (Terminals tab inert) | PORTED (terminals tab inert) |
| Permission ask (session route presentation) | `PermissionPrompt` w/ presentation body (diff preview, option labels, reject free-text stage) | mini-footer permission body mounted at bottom (`app.rs:1015-1030`); diff/patch subset in `footer/permission.rs` | VARIANT-MISSING (presentation-body depth) |
| Form ask (session route) | 1175-line form UI: field tabs, multiselect toggles/custom rows, textarea editing, external-URL buttons, validation | mini form body (`footer/form.rs` 705 lines) covers labels/options/textarea/hints; editing stage + external-ready + validation errors absent | VARIANT-MISSING |

## 6. Transcript rows (`routes/session/rows.ts`, `message-parts.tsx`, `index.tsx` rows → `message/*`, `tool_rows.rs`)

| Ref row/part | Variants | Rust | Class |
|---|---|---|---|
| User message | text, files strip w/ +N/−N, skills row, image strip; click → Message Actions | ported incl. image strip + actions dialog | PORTED |
| Assistant text (markdown) | streaming, syntax, tables, links (OSC 8) | `message/markdown.rs` + syntax map | PORTED |
| Reasoning group | collapsed title+duration; expand → thought rows | ported (expand + thought rows) | PORTED |
| Tool groups (Exploration/Activity) | collapsed label; **expanded per-tool cells** (bash cmd+output preview `collapseToolOutput` 4-line, edit diff, error row, hover expand → `DialogExecute`/`DialogShellOutput`) | collapsed label only; ungrouped non-subagent tool parts render **nothing** (`message/list.rs:288`) | **VARIANT-MISSING** (all detail surfaces) |
| Subagent tool card | running spinner / completed / error / "Continue subagent" | ported (`rows.rs:121-182`) | PORTED |
| SubagentHeading (child-session transcript header) | hover → navigate to child session | none | **MISSING** |
| Assistant footer | model display name, `· Nms · X.X tok/s`, error row gated `!interrupted && !retry` | ported (name/duration/tps/error); **interrupted variant absent** | VARIANT-MISSING |
| Compaction message | running spinner/`⋯` (animations off), failed `✗`, `· cancelled`, "Provider compaction", `N in · M out`, markdown summary | none ("wave 3" — `list.rs:251-253`) | **MISSING** |
| CompactionQueued row | `◇ Compaction queued` divider | none | **MISSING** |
| Skill message | `→ Skill {name}` inline row | none | **MISSING** |
| Turn-usage row | per-turn tokens (debug) | tps only | VARIANT-MISSING |
| Group headers (group-view) | Thinking tint `warning@0.6`, counts | ported | PORTED |

## 7. Dialog family matrix (~34 surfaces)

| Ref dialog | Trigger | Variants (empty/loading/error/narrow) | Rust | Class |
|---|---|---|---|---|
| Command palette | `ctrl+p` | fuzzy, categories, bindings column | full port | PORTED |
| DialogHelp | `/help` | keybinding table grouped by mode | `DialogKind::Help` | PORTED |
| DialogThemeList | `/themes` | live preview + revert on esc | ported incl. preview contract | PORTED |
| DialogSessionRename | tab rename | validation | ported (+server intent) | PORTED |
| DialogWorktreeName | new worktree | tab→slug | ported | PORTED |
| DialogWorkspaces | `/worktrees`, `session.move` | loading/working rows, move/new/delete/refresh, file-changes confirm | `dialog_worktrees.rs` + `WorktreeDeleteConfirm` | PORTED |
| DialogMessage (message actions) | click user msg | 4 fixed actions | ported | PORTED |
| Working-directory actions | click `{dir}:{branch}` | 3 actions | ported | PORTED |
| Queued prompts (`<leader>q`) | queue manager | steer/delete | ported | PORTED |
| DialogModel | `/models` | loading/error/empty, free flag, deprecated, current marker | `ServerList{model.list}` w/ loading+error rows | PORTED |
| DialogAgent | `/agents` | loading/error, current | `ServerList{agent.list}` | PORTED |
| DialogSessionList | `/sessions` | project group headers, per-row detail (time/model), current-project label | basic title-only rows | VARIANT-MISSING |
| DialogMcp | `/mcps`, sidebar click | per-server status, error focus, tools list | **stub** | **STUB** |
| DialogVariant | `/variants` | no-variants toast guard | **stub** | **STUB** |
| DialogOpen | `/open` | recent sessions + projects + worktrees; loading/`false` empty states; large size | **stub** (large size only) | **STUB** |
| DialogIntegration | `/connect` | 1045 lines: provider list, API-key forms, OAuth URL/copy, field errors | **stub** | **STUB** |
| DialogConfig | `/settings` | tabs of settings, editor launch | **stub** | **STUB** |
| DialogStatus | `/status` | "No MCP servers" fallback, per-status text/error | **stub** | **STUB** |
| DialogPair | `/pair` | localhost/loopback/links variants, "Loading server information…" | **stub** | **STUB** |
| DialogDebug | `/debug` | env/config dump | **stub** | **STUB** |
| DialogUpdate | `/update` | checking/available/installing/installed/current/unavailable/failed | none | **MISSING** |
| DialogExperiments | devtools | flags list | none | **MISSING** (dev-only) |
| DialogSkill | `/skills` | loading, error Match | none | **MISSING** |
| DialogStash | prompt stash | list/restore | none (queue ≠ stash) | **MISSING** |
| DialogShellOutput | tool output expand | omitted-lines notice, error variant | none | **MISSING** |
| DialogExecute | tool details | input/metadata views | none | **MISSING** |
| DialogErrorDetails | error rows | source + diagnostic ref | none | **MISSING** |
| DialogImagePreview | image click | zoom/controls | none | **MISSING** |
| DialogTimeline | `session.timeline` | turn list | **stub** | **STUB** |
| DialogFork | `session.fork` | message pick | **stub** | **STUB** |
| dialog-alert / dialog-confirm | plugin API, destructive asks | tone variants | none | **MISSING** |
| dialog-export-options/-result | diff export | option list / result path | none | **MISSING** |
| dialog-prompt (generic) | plugin API | masked input (pair codes) | only rename/worktree instances | VARIANT-MISSING |
| ui/dialog chrome | all | backdrop alpha, outside-click dismiss, sizes 60/88/116 clamp `width-2`, stack | ported + pinned | PORTED |
| ui/dialog-select | all selects | "No items available" / "No results found" / custom empty views, filter input (formfield keys), category headers `hue.accent[200]`, footer/gutter | empty/no-match/custom texts ported; **filter input paints without `formfield.focused/selected` bindings** | VARIANT-MISSING (style) |

## 8. Diff viewer (`feature-plugins/system/diff-viewer*` → `components/diff_viewer.rs`)

| Ref surface | Gate / variants | Rust | Class |
|---|---|---|---|
| Viewer mount | `diff.open` route/command | **no trigger anywhere** — component unreachable in the live app | **MISSING** (unmounted) |
| Body states | Loading "Loading diff…" / error (2 copy variants: needs-base vs retry) / unavailable / empty "No changes to show" | none of the four states | **VARIANT-MISSING** |
| File tree | `width >= 90 && files > 0`; expand `▾`; reviewed marks `N/M` | ported | PORTED |
| File menu (right-click) | stage/mode actions | none | **MISSING** |
| Image diffs | `DiffViewerImage` | none in viewer | **MISSING** |
| Source switch / split-unified / wrap | `diff.*` commands | ported (view override, wrap) | PORTED |
| Export | export-options/result dialogs | none | **MISSING** |

## 9. System feature-plugins

| Surface | Rust | Class |
|---|---|---|
| `notifications.ts` (sounds/OS notify, blurred gating, retry) | `notifications_plugin.rs` + `os_notify.rs` | PORTED |
| `stats.tsx` (session panel route) | none | **MISSING** |
| `plugins.tsx` (plugin manager route) | none | **MISSING** |
| `storybook/*` (dev) | none | **MISSING** (dev-only) |
| plugin slots (`app`, `sidebar.content/footer`, `home.footer`, `prompt.footer.status`, `session.composer.top`, `session.panel`) | sidebar sections + footer hints + plugin dialogs ported (`plugin_ui.rs`); `session.panel`/`session.composer.top`/`home.footer.status` slots absent | VARIANT-MISSING |

## 10. Mini TUI (`mini/*` → `crates/oc-tui/src/mini/*`)

Spot-checked (not exhaustively diffed): splash, runtime/lifecycle, queue, scrollback (incl. compaction divider), prompt_shared, model_pref, theme, transport, mono all have direct counterparts; footer family (form/permission/command/panels) is the port source for the session-route footer. No missing surface identified at spot-check depth.

## 11. Theme-key binding audit (`ThemeStyles` vs ref `theme.*` usage)

`ThemeStyles` (theme.rs:110-157) resolves: text/base+muted, background(base/raised/prompt), action selected/hovered(=raised.high)/focused bg+fg/secondary, accent(hue s200), error, warning, border, markdown.*, diff.*, formfield_base, agent categorical hues, syntax map. Ref surfaces additionally consume:

| Ref key (usage count) | Rust binding | Class |
|---|---|---|
| `text.feedback.success.base` (15) — MCP dot, home `⊙ N MCP`, turn success | none — no success token at all | **STYLE-MISMATCH** |
| `text.feedback.info.base` (7) — jump-to-latest, migration border, subagent heading | `text_muted` fallbacks | **STYLE-MISMATCH** |
| `text.action.secondary.hovered` (2) + `.base` (5) | only `.base` | VARIANT-MISSING (hover states) |
| `text/background.formfield.focused/selected` (20/17/15/5/4) — select filter, prompts, config | absent (formfield_base only) | **STYLE-MISMATCH** |
| `text/background.action.destructive.focused` (4+4) — delete/drop confirms | plain select rows | **STYLE-MISMATCH** |
| `scrollbar.base` (3) — sidebar/scrollbox tracks | not bound | STYLE-MISMATCH (minor) |
| `hue.interactive` (6) — btw spinner, interactive marks | only `accent` s200 | VARIANT-MISSING |
| `background.action.primary.hovered` (7) — update banner, buttons | absent (action_hovered aliases raised.high) | VARIANT-MISSING |
| diff palette incl. `diff.line`, line-number bgs | full DiffStyles | PORTED |

---

## Counts

- Ref rendered surfaces inventoried: **~85** (12 app chrome, 6 home, 12 session-route, 5 sidebar, 13 composer/footer, 12 transcript rows, 34 dialogs, 6 diff viewer, 4 system plugins, + mini spot-check).
- **PORTED**: 33 · **VARIANT-MISSING**: 14 · **STUB**: 12 (commands: `mcp.list`, `variant.list`, `open.menu`, `provider.connect`, `opencode.settings`, `opencode.status`, `server.pair`, `opencode.debug`, `docs.open`, `session.share`, `session.fork`, `session.timeline`) · **MISSING**: 24 · **GATE-MISMATCH**: 1 (+1 interaction-dead) · **STYLE-MISMATCH**: 6 clusters.

## Top-10 worst findings (severity = user-reachability)

1. **12 slash/palette commands land on a dead stub** — `/settings`, `/status`, `/mcps`, `/variants`, `/open`, `/connect`, `/pair`, `/debug`, `/share`, `/fork`, `/timeline`, docs — every one opens a titled dialog whose only row is "Not wired in this build" (`command_palette.rs:418-421`). Daily-reachable via slash autocomplete, which advertises them as real.
2. **No `Reconnecting` overlay and no `StartupLoading`** — on server loss the app keeps painting stale chrome (v2 dims the screen after 1 s with "Connection lost…"); on slow plugin boot v2 shows "Loading plugins…" after 500 ms. Both boot/disconnect states are invisible divergences.
3. **Sidebar live content is hollow** — `live_sidebar_view` hardcodes `context: []`, `mcp: []`, `location: None` (`session.rs:456-459`): no token/%/cost block, no MCP server list (incl. failed/needs-auth visibility), no `{dir}:{branch}` footer row. Wide-layout daily screen diverges wholesale.
4. **Getting-started card gate wrong + dead dismiss** — missing `!hasConnectedProvider` condition (`sidebar/footer.tsx:18-23` vs `session.rs:461`) so provisioned users keep seeing "Connect provider"; the ✕ dismiss click is dead (registered dogfood3), so it cannot even be dismissed by mouse.
5. **Transcript detail states missing** — expanded tool outputs (bash/edit cells, error rows, `DialogExecute`/`DialogShellOutput`), compaction message/queued rows, skill rows, revert banner, interrupted-message variant, "Loading session history…" — all absent; ungrouped non-subagent tool parts render nothing (`message/list.rs:288`).
6. **Diff viewer is unreachable** — 1,217-line ported component with no `diff.open` wiring (no route/dialog/command), and its Loading/Error/Unavailable/Empty variants + image diffs + file menu were never ported.
7. **Home first-use/update surfaces missing** — `UpdateNotification` banner (available/installed/remote variants), MCP status cell, plugins-failed cell: the footer's left half never renders.
8. **Theme-token binding gaps (nord-class)** — no `success`/`info` feedback tokens at all; jump-to-latest paints `text.muted` instead of `feedback.info`; formfield focused/selected and destructive-focused pairs unbound; `action.secondary.hovered` and `action.primary.hovered` absent. Correct on v2-dark defaults, wrong on every other theme.
9. **No updater surface** — `DialogUpdate` + home banner + `opencode.update` command all absent; update-state communication to the user doesn't exist.
10. **Error/edge surfaces absent** — `ErrorComponent` (crash fallback), `ThemeErrorToast`, `MigrationOverlay` (with error toast), `SessionLocationMissing` (deleted-directory recovery path), MCP needs-auth/failed toasts with "Open MCP servers" action.

### Notable near-misses (below top-10)
- Session-list dialog lacks project group headers + per-row detail; rename shimmer/dim pending state partial.
- Vertical tabs layout + pane resize handle unmounted; terminal title (OSC 2) never set.
- Composer: no shell mode (`!`), no subagent/shell counts, no cost, no `/btw`.
- Permission/Form asks render through the mini-footer body — main-route presentation depth (diff preview in ask, form editing/validation stages) partial.
