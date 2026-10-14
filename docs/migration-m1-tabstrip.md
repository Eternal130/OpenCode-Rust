# Lane M1 — the tab strip 换血 (transfusion): pattern doc

> Status: **done** (lane/m1). First surface migrated from the
> hand-rolled ratatui renderer onto opentui-solid. This doc is the
> TEMPLATE for M2..Mn — what to copy, what to watch for, what the
> instruments said.

The subject: v2's `HorizontalSessionTabs`
(`packages/tui/src/component/session-tabs.tsx:1288-1803`) — the
1-row tab strip (titles, `✕` close affordance, ` + ` add, hover
lifts, unread-glow, adaptive windowing). Ported structure-1:1 as
`crates/oc-tui/src/components/session_tabs_solid.rs` using the
opentui-solid builder API (`box_`/`text`/`show`/`for_list`/
`children_expr`/`dyn_prop` — the JSX-equivalent surface, see
`/opt/opentui-rust/crates/opentui-solid`).

## The migration pattern (copy this shape)

```
store (unchanged)                       oc-tui app loop (unchanged)
   │ snapshot per redraw                        │
   ▼                                            ▼
StripInputs ──set()──► Signal ──map() chains──► dyn props / For / Show
                                                │  (opentui-solid reconciler)
                                                ▼
                                       SolidRenderer.flush()  (patches + render)
                                                │
                                                ▼
                                    OptimizedBuffer ──cell copy──► oc-tui Frame
```

1. **One inputs snapshot signal per surface.** `Signal<StripInputs>`
   carries everything the tree reads (tabs+statuses, active, hover,
   width, theme colors). The app loop sets it at its EXISTING redraw
   cadence — exactly the moments the old renderer re-rendered. The
   store's own signal graph is untouched; the reconciler's per-prop
   equality skip makes unchanged inputs a no-op.
2. **Derive, don't rebuild.** Every reactive value is a `map` chain
   over the signal (the tsx's `createMemo`s): the layout window, each
   tab's paint state, each affordance. Elements mount ONCE; a hover
   change patches `backgroundColor`/`content`/`width` in place.
3. **`For` keyed by identity** (sessionID) over the SAME solver the
   old path uses (`adaptive_session_tab_layout`) — reuse the ported
   model code verbatim; the view layer is what changed.
4. **Frame out = cell copy.** The surface paints through a real
   `CliRenderer` (taffy + raster) into an `OptimizedBuffer`; convert
   cells (char/attrs/fg/bg) into the oc-tui `Frame` the compositor
   already consumes. The route compositor, ANSI serializer, PTY —
   all unchanged.
5. **Dual-path + flag.** One env flag selects the renderer, read
   ONCE at the surface's first paint ("at boot") via `OnceLock` — a
   running session never switches renderers. M1 shipped with the
   flag DEFAULT ON after the rig proof; `OC_TABS_OPENTUI=0` keeps the
   hand-rolled fallback for one release, then it gets deleted.
6. **Interactions stay bridged.** The tsx's `onMouseOver/onMouseUp`
   bodies all land in three store methods (`select_tab`/`close_tab`/
   `Route::Home`). Our `app.rs` mouse routing already drives exactly
   those through `strip_hit` geometry; the solid tree carries NO
   event wiring, and the bridge held byte-for-byte (rig-verified).
   When a later lane moves the PTY mouse stream onto the renderer's
   hit grid, that routing is the seam to replace.

## Gotchas (each one cost real debugging time)

* **`flexDirection` defaults to COLUMN.** opentui pins yoga's null ⇒
  Column (taffy defaults Row; the port re-pins it). The tsx's
  `<Show>`/`<For>` constructs have no Rust element form — each needs
  a bg-transparent CARRIER box, and a bare carrier is a COLUMN: the
  tabs stacked vertically out of the 1-row strip and the second tab
  vanished. **Every carrier needs `.flex_direction("row")` when it
  sits in a row.**
* **One dynamic-children source per element.** An element's children
  are `Static(Vec)` OR one `Dyn` source — no mixed arrays like JSX.
  Multiple `Show`/`For` siblings ⇒ one carrier box each.
* **`Mapped` is not `Clone`.** One `map(...)` chain per dynamic prop
  (each subscribes independently) — matches the tsx's one render
  effect per expression, but you can't share a single chain.
* **Opaque fills hard-write fg=white.** The old path's gradient
  `Fill`s carry `fg(Color::Reset)`; opentui's box fill writes
  fg=white unconditionally. The frame copy restores `Default` on
  glow-range space cells whose fg is exactly white (fill signature).
  A glyph-painted space (a glowing title's own spaces carry per-char
  tints) keeps its fg. If a future surface needs fg-transparent
  fills, that's an opentui-core change.
* **Transparent-bg text blends, it doesn't overwrite.** Chunks with
  `bg: None` keep the destination cell bg (the upstream
  TabPulse-under-text stacking) — this is why the per-cell glow boxes
  work: the title/indicator glyphs float over them. Explicit chunk
  `bg` overwrites (used for the `✕`, whose flat bg must mask the
  gradient under it, and for the `" + "` pads).
* **A closing item's mapped views fire once more** after it left the
  inputs, before its `For` unmount lands at the next flush — every
  per-item view must tolerate "item gone" (degrade to empty), never
  `.expect()`. The default flip surfaced this as a live panic
  (`pty_interactions` caught it); the flag-off pilot runs masked it.
* **Graphemes are pool-packed in the buffer.** `cell.char` is an
  ASCII codepoint, a packed grapheme id, or a continuation marker —
  decode via `is_grapheme_char`/`grapheme_bytes`/`is_continuation_char`
  (see `frame_from_buffer`).
* **Attribute bits are ECMA-ordered** (bold=1<<0 … strikethrough at
  bit 8 = SGR 9) — map 1:1 except strikethrough.
* **Resize/theme change = rebuild the mount** (`SolidTabsStrip::new`)
  — the reconciler has no in-place background-color seam, and percent
  sizing against a changed root needs the fresh taffy tree. Rare, so
  cheap.

## Instruments & numbers (the lane's acceptance record)

* **Unit gate** (`solid_strip_frame_equals_the_old_renderer`): the
  solid frame must equal the old renderer CELL FOR CELL over a
  27-entry matrix (resting / unread glow / error glow / attention /
  busy / hover lift + ✕ / add-hover / untitled / truncation /
  overflow windowing × w80/w160/w40). This is the cheapest instrument
  — keep it green for every future tweak before touching the rig.
* **Rig** (tui-diff, 5 scripts × 2 sizes × 2 runs): the solid default
  vs the `=0` fallback produce IDENTICAL mismatch-step sets; the
  `mouse-hover-tabstrip` corpus table is identical; the
  hover-induced cell deltas are `{0,0,+1,0,0,0}` on BOTH paths (the
  +1 is the known `✕` fg indexed-color finding — pre-existing).
  Run-to-run cell-count swings (±20) are the home placeholder
  rotation (random per boot), not strip cells. Rig flag: the runner
  forwards `OC_TABS_OPENTUI` to our side only.
* **PTY**: `pty_tabs.rs` runs the boot/open/close choreography under
  BOTH paths, plus `pty_solid_strip_hover_add_close_cycle` (SGR
  hover → `✕` reveal → ` + ` click → home → `✕` click → close).
  nextest's per-test process isolation is what makes the once-per-
  process flag testable.
* **Frame budget (release, 200×50, 12 tabs, hover-churn)**: old
  307 µs worst / 281 µs avg; solid 863 µs worst / 832 µs avg —
  ~2.8× the old path, ~0.9 ms absolute, vs the 40 ms TEMP budget.
  The transcript budget test (worst 20 ms) is unaffected.
* **Release size**: 64,659,904 → 65,886,112 B
  (**+1,226,208 B ≈ +1.17 MiB, +1.90%**) — the first surface whose
  release graph links opentui (core's renderer/buffer/text +
  solid's reconciler). Every later surface rides the same cost.

## What M2..Mn should copy verbatim

* `StripInputs`-shaped snapshot signal + `map` chains + dyn props.
* The `frame_from_buffer` cell-copy bridge (glow-range fg handling
  generalizes: any cell the old path `Fill`s with `fg(Reset)`).
* The flag pattern: `OC_<SURFACE>_OPENTUI` once-at-boot `OnceLock`,
  default OFF until the rig proof, then flip, one-release fallback.
* The unit parity gate against the old renderer BEFORE any rig run.
* The PTY parametrization (both paths) + one interaction-cycle test
  on the new path.

---

# Lane M2 — the dialog family: the pattern at scale

> Status: **done** (lane/m2). The first at-scale replication of the
> M1 shape: the WHOLE dialog family (`ui/dialog-select.tsx`,
> `ui/dialog-prompt.tsx` + the worktree-name twin, `ui/dialog-help.
> tsx`, the plugin rows payload, the chrome box of `ui/dialog.tsx`)
> ported as one builder tree in
> `crates/oc-tui/src/components/dialog_solid.rs`, selected by
> `OC_DIALOGS_OPENTUI` (default off until this lane's rig proof).
> The store's dialog machinery, the modal key layer, the mouse
> routing (`option_dialog_rows`), and the dispatcher are untouched.

The M1 shape held: one inputs-snapshot `Signal`, `map` chains over
the same solver code (`grouped`/`flat_indices`/`viewport_height` —
the old renderer's own walk), `For` keyed by option identity, the
`frame_from_buffer` cell copy, OnceLock flag. The unit gate
(`solid_dialogs_equal_the_old_renderer_*`) runs a
dialog-kind × {80,120,40} × state matrix (open/browse/filter/
selected/footers/empty/no-match/scroll-ends/prompts/help/plugin) —
cell-for-cell AND rect-for-rect identical.

## What this family ADDED to the template

* **`builder.child()` REPLACES, `children_dyn` CLOBBERS.** The
  builder's `child()` sets `Children::Static(vec![kid])` — a second
  `.child()` call on the same chain SILENTLY drops the first child,
  and `children_dyn` on a chain with static children drops them all.
  Every multi-child box must be one `.children(Children::Static(
  vec![…]))`; every `Show`/`For` needs its own bg-transparent
  CARRIER (M1 knew the carrier rule; M2 found the clobbering
  mechanism behind it). Symptom class: "the second child of every
  space-between row is missing."
* **Percent width on the MOUNT ROOT breaks the tree.** A
  `width("100%")` root mis-sizes: space-between rows lose their
  trailing children entirely. Explicit points for the root (the
  buffer size), percents fine deeper in. The mount rebuilds on size
  change anyway.
* **Height-varying surfaces key the mount on CONTENT HEIGHT.** The
  strip was 1 row forever; a dialog's height changes (a filter
  shrinks the window, help re-wraps). The mount key is `(family,
  width, colors, 1 + content_height)` where the height comes from
  the SAME walk that sizes the old path's row vector. Height-stable
  frames — the hot ones (selection moves, scroll) — patch in place;
  height-changing frames rebuild (rare, small trees).
* **The empty state rides IN the `For`.** The tsx's
  `Show(fallback=Show(…))` nesting can't be expressed (one dynamic
  children source per element) and a sibling `Show` carrier would
  add a gap row the old renderer doesn't paint. The no-match row is
  a For ITEM (`e:{message}`) that replaces the window — matching the
  old walk's "push the message into the window" exactly.
* **The scrollbox's paddings become ROW MARGINS.** The selected-row
  background must span `[1, width-1)` — a box fills inside its
  margins, so the row boxes carry `marginX 1` instead of the
  scrollbox carrying padding; the empty row (paddingLeft 4, NO
  margins) then lands at the same x=4 the old renderer pins, in the
  same For.
* **The old renderer paints PAST its boxes; the tree must too.**
  The old `put` walks to the ROW edge — a title runs through the
  row's paddingRight into the last column, and the right-aligned
  footer then OVERWRITES its span (glyphs right of the footer
  survive). The faithful tree: the title text is `flexShrink 0` +
  `wrapMode none` (natural width, pre-truncated by the same atom
  walk), the footer is `position:absolute; right:3` — absolutely
  positioned so it overlaps the title, and LATER in DOM order so it
  overwrites it. Every option-row chunk also carries the ROW bg
  explicitly (the old `put` writes bg on every glyph cell, past the
  row box's own fill span).
* **Text must not shrink: the default flexShrink wraps.** At 40
  columns the four footer actions overflowed; the texts shrank and
  WRAPPED onto extra rows, which compressed the flexGrow scroll area
  and swallowed a group header. Every fixed-content text in a row
  (action labels, option runs) needs `wrapMode none` +
  `flexShrink 0` — overflow past the box is then clipped by the
  buffer, exactly like the old renderer's row edge.
* **The For key is an API: parse it deliberately.** Row keys carry
  payloads (`o:{index}`, `h:{category}`, `e:{message}`); the render
  closure branches on them. A prefix/parse mismatch doesn't fail —
  it falls through to an empty fallback element (symptom: rows mount
  per the ops log but paint nothing). Strip the FULL prefix
  (`"o:"`, not `'o'`).
* **The backdrop stays at the bridge (NEW-S2a).** The scrim cannot
  be a tree node: the opentui raster alpha-composites over its OWN
  buffer, which does not contain the route content. The bridge
  applies v2's observable semantics per cell over the base frame
  (bg always dimmed; fg dimmed only on glyph cells — `blend_backdrop`,
  shared by both paths).
* **Diff-serializer gotcha for PTY asserts.** A dialog REPLACING
  another dialog (palette → theme list) emits only changed cells:
  the new title "Themes" over "Commands" arrives as sparse runs
  ("The" … "es"), so byte-searching the title NEVER matches. Assert
  on a full changed ROW ("lucent-orng"), not on chrome that overlaps
  the previous frame.
* **Leaf-text `paddingLeft` is inert in the v2 binary but LIVE in
  the port.** dialog-select.tsx's title text carries `paddingLeft=3`
  which the JS core does not apply to leaf texts (rig-pinned: titles
  at col 4); the Rust port routes it through taffy. The tree mirrors
  the BINARY (no pad on the text), and header rows use a padded BOX
  around the text — never leaf-text padding.

## Instruments (this lane's record)

* **Unit gate**: `solid_dialogs_equal_the_old_renderer_store_driven`
  + `_structural` — ~30 scenarios × 3 widths, cell-for-cell + rect.
* **Rig A/B**: OC_DIALOGS_OPENTUI forwarded to our side (the M1
  seam); scripts help-dialog, palette, rename-dialog,
  session-list-dialog, type-edit-submit, click-coverage-dialog at
  80x24+120x30 ×2 runs — identical mismatch-step sets both paths.
* **PTY**: `pty_dialog_palette_cycle_{old,solid}_path` (open →
  browse → filter → APPLY → unwind) + the help-dialog test
  parametrized over both paths.
* **Budget**: `frame_budget_dialog_open_{old,solid}_path` (release,
  200×50, palette open, alternating selection): old 328 µs worst /
  301 µs avg; solid 1450 µs worst / 1142 µs avg — ~4.4× the old
  path, ~1.5 ms absolute, vs the 40 ms gate.
* **Flag protocol**: default flipped ON after the proof (the unit
  matrix, the identical rig sets, both PTY paths, the 1.4 ms worst
  frame); `OC_DIALOGS_OPENTUI=0` keeps the hand-rolled renderer for
  one release.
* **Release size**: 64,659,904 → 66,048,256 B
  (**+1,388,352 B ≈ +1.32 MiB, +2.15%**) — the dialog family's trees
  + the shared bridge ride the graph opentui already occupies.

## Lane M3 — composer + footer (the largest single surface)

Scope: prompt mount chrome (rail/insets), metadata row ladder, footer row
(location budget/truncateFilePath, hints block, usage arm, interrupt hint,
hover lift), placeholder presentation, hardware-cursor bridge
(`CursorOutputState`). Editor ENGINE deliberately NOT migrated (subsystem
lane later). Numbers: rig 24 invocations flag-symmetric (only the (b)-class
home-placeholder coin differs — run-to-run within the SAME flag too);
release budget old 340µs / solid 375µs worst (1.2×, bridge reuses the old
solver walk); unit parity matrix + PTY both paths in the WIP series.

**Operational lessons (the real M3 cost — three session deaths):**
1. Long background verification runs inside a lane session die with the
   session (host restarts / quota windows). Countermeasure: run heavy
   verification as FOREGROUND CHUNKS (one invocation per command), append
   results to a log file, and WIP-commit every ~6 invocations — resume =
   read the log tail. A dead lane then loses minutes, not hours.
2. Watchdogs must detect liveness by DISK ACTIVITY (file mtimes under the
   worktree), not process names (`pgrep` self-matches; a watcher counting
   itself keeps a dead lane "alive"). Done-criterion must be a structural
   marker (final commit subject not `wip(`), not a commit count (progress
   WIPs trip counts).
3. When a child session proves flaky, ORCHESTRATOR TAKEOVER of the
   mechanical tail is cheaper than revive roulette: the remaining steps
   (rig invocations, gates, flag flip, squash) are deterministic and the
   orchestrator can run them chunked with the same durability rules.
   Beware takeover overlap: a revived child may wake mid-takeover —
   append-only logs make interleaving non-corrupting; check the board and
   prefer ONE worker per worktree.

---

# Lane M3 — the composer + footer: the pattern on the hot path

> Status: **done** (lane/m3). The largest single v2 surface — the
> prompt mount (`component/prompt/index.tsx`, 2011 LOC: the `┃` rail
> box, placeholder presentation, the metadata row, the `╹▀` transition,
> the footer status row, the autocomplete popup above) — ported as one
> builder tree in `crates/oc-tui/src/components/composer_solid.rs`,
> selected by `OC_COMPOSER_OPENTUI` (default ON after this lane's
> proof). The text EDITING stays on the store's `oc_tui_core`
> `text_edit` engine (the editor-engine swap is a later subsystem
> lane): this lane renders the composer AROUND it.

The M1/M2 shape held — one inputs-snapshot signal, the SAME solver
walk (`layout_draft_lines`, `MetadataLine::layout`, the popup rows)
collected as pre-shaped chunks, the `frame_from_buffer` cell copy, the
OnceLock flag. The mount keys on GEOMETRY (width, top, popup height,
content height, frame height, colors): height-stable frames (typing
within the last row, caret moves, hover) patch content dyn-props in
place; a wrap landing a new row or the popup opening rebuilds the
small tree (the M2 content-height rule).

## What this family ADDED to the template

* **Extract the geometry, don't mirror it.** The old renderer's
  footer math (`#4`/`#20`'s hints-vs-location budget, `#33`'s hover
  extent) existed TWICE already (`paint_footer_row` +
  `footer_location_extent` "keep in sync"). The lane extracted
  `render::footer_plan` — ONE resolution shared by the old painter,
  the hit-rect helper, AND the solid bridge. A third copy would have
  been drift-on-arrival; when the old path encodes fixed findings,
  factor the fix, then consume it from both paths.
* **Separator/gap cells must stay fill-painted.** The metadata row's
  inter-segment spaces and the footer's 2-gaps between arms carry the
  FILL's white fg in the old bytes. A space inside a styled chunk
  paints the chunk's fg on that cell — a cell-for-cell divergence the
  matrix catches. The shape: SEPARATE absolutely positioned text
  elements per segment (x from the same walk), never separator spaces
  inside chunks. (M2's footer-overwrites-title was the same class from
  the other side.)
* **A fixed-count segment farm beats a shape-varying For.** The
  metadata ladder renders at most 7 segments (agent auto · model
  provider · variant) — the tree mounts seven fixed absolute texts,
  each reading `segments.get(i)` (absent ⇒ empty chunks). No For, no
  keyed churn; a dropped field just empties its slot in place.
* **The software caret is a BRIDGE recolor.** The golden-only block
  (`PromptView::caret` + the blink phase) inverts one cell's colors
  over the copied buffer — char preserved — exactly the old painter's
  write. The LIVE caret never touches the tree: it is the TERMINAL
  cursor (NEW-S1a), still emitted by the untouched
  `prompt_caret_screen_cell` → `Store::set_caret_cell` →
  `set_cursor_position` seam. opentui-core's `CursorOutputState`
  (renderer.rs:105) is driven by a focused textarea renderable — the
  editor-engine lane moves the emission there (`set_position` on the
  buffer-relative cell) and retires the store seam.
* **Degenerate frames guard at the bridge.** width/height 0 returns
  the old renderer's empty-row shape directly; a frame shorter than
  the mount clips by buffer bounds (children laid out past the edge
  raster nothing) — same visible bytes as the old painter's
  row-bounds skips, no special cases in the tree.
* **Stub-harness traps for composer-cycle PTY tests.** (1) The
  scripted server's inbox echo had a FIXED id — a second submission
  dedups invisibly; give each POST its own id. (2) Its mid-session
  SSE pushes are unreliable (the event-stream drain dies on read
  hiccups) — drive assertions off the inbox echo and the
  message-list fetch, never off pushed frames. (3) `wait_for_after`
  marks: two paints landing in one output burst make the second
  assert miss — take ONE mark before the burst and share it. (4) A
  keystroke per diff frame paints one changed cell — never grep for a
  contiguous word the composer is still echoing.

## Instruments (this lane's record)

* **Unit gate**: `solid_composer_frame_equals_the_old_renderer` —
  ~25 scenarios (idle, typing, multiline 3+ rows incl. the CJK and
  extmark-chip wrap fixtures, interrupt armed/unarmed, usage arm +
  the narrow gate, plugin hints, hover lift, bound shortcuts, the
  metadata drop ladder ×4 budgets, agent rail, caret block
  empty/mid/end, popup open + top-clamped, the golden top-2 shape,
  frame-shorter-than-mount) × widths 40/60/80/120/160 — cell for
  cell. Plus typing-churn patch-in-place and resize-rebuild gates.
* **Rig A/B** (`docs/m3-rig-ab.log`, 24 invocations = 6 scripts × 2
  sizes × 2 runs × both flags, run under the generalized `OC_*_OPENTUI`
  forwarding seam): 10/12 pairs have IDENTICAL mismatch-step unions;
  the other 2 differ only in placeholder-carrying steps — the
  documented (b)-class home-placeholder coin (row 13 alone; the old
  path flips the same steps run-to-run within itself).
* **PTY**: `pty_escape_interrupt_*` + `pty_leader_return_queue_*`
  parametrized over both paths; the new
  `pty_composer_cycle_type_edit_submit_reply_stats_solid_path`
  (type → edit → submit → reply admission → `18 (1%)` stats row).
* **Budget** (release, 200×50 session route): old idle 209/199 µs,
  churn 217/206 µs worst/avg; solid idle 225/219 µs, churn 256/248 µs
  — ≈1.1–1.2× the old path (the bridge SHARES the old path's solver
  walk, so the delta is only taffy + raster), vs the 40 ms gate. All
  other budget gates re-checked green post-flip.
* **Flag protocol**: default flipped ON after the proof; the
  `OC_COMPOSER_OPENTUI=0` fallback keeps the hand-rolled renderer for
  one release (638 oc-tui tests + 2801 workspace tests green through
  the solid default — the goldens now pin the SOLID path's bytes).
* **Release size**: 66,048,256 → 66,120,608 B
  (**+72,352 B ≈ +70.7 KiB, +0.11%**) — the composer tree + the
  footer-plan extraction; the opentui graph was already linked, so
  this lane's marginal cost is the smallest of the three.
