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

## The accelerated protocol (added by lane/accel — copy this too)

The rig inner loop is tiered now; every later lane verifies like this:

* **Iterate**: `tools/parity/tui-diff/verify-lane.sh iterate <your
  scripts>` — fmt + nextest for oc-tui/oc-tui-core/parity + the rig in
  `--quick` mode (`--runs 1`, single size 80x24) with the ref-capture
  cache ON (the bun side's settled grids replay from
  `tui-diff/cache/ref/`; our side always boots fresh). Minutes, not
  tens of minutes.
* **Final**: `verify-lane.sh final` — CI-equivalent fmt+clippy, full
  workspace nextest + the serialized pty profile, the release budget
  gate, and the rig at BOTH sizes × `--runs 2` **with the cache
  disabled** — final mismatch verdicts are live verdicts.
* The A/B discipline is unchanged: flag-on vs flag-off must produce
  IDENTICAL mismatch-step unions; the cache/`--quick` only accelerate
  reaching those unions, they must never alter them (the cache
  replays only settled grids confirmed byte-identical across N
  independent runs — `--measure-stability` warms AND confirms — with
  unconfirmed or flippy steps staying live, fingerprint-keyed so any
  ref/bun/script change invalidates).

## What M2..Mn should copy verbatim

* `StripInputs`-shaped snapshot signal + `map` chains + dyn props.
* The `frame_from_buffer` cell-copy bridge (glow-range fg handling
  generalizes: any cell the old path `Fill`s with `fg(Reset)`).
* The flag pattern: `OC_<SURFACE>_OPENTUI` once-at-boot `OnceLock`,
  default OFF until the rig proof, then flip, one-release fallback.
* The unit parity gate against the old renderer BEFORE any rig run.
* The PTY parametrization (both paths) + one interaction-cycle test
  on the new path.
* The accelerated protocol above: iterate via `verify-lane.sh
  iterate`, final via `verify-lane.sh final` (full matrix, cache off).

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

## Lane M4 — the transcript: the biggest tree, the flattest one

> Status: **done** (lane/m4). The session route's transcript region —
> the message list the store's snapshots reduce to (user boxes,
> exploration/tool rows, thought blocks, markdown text parts, stats
> rows, error rows, the image strip) — ported as one builder tree in
> `crates/oc-tui/src/components/transcript_solid.rs`, selected by
> `OC_TRANSCRIPT_OPENTUI` (default ON after this lane's proof; the
> hand-rolled region walk stays one release as the `=0` fallback).
> The old walk was EXTRACTED to `Transcript::region_frame` (the
> session route's paint semantics, drop-above-the-window units) so
> both painters consume the ONE shared `render_transcript` solver
> output; the hit rows (E1 #29/#34), the scroll metrics and the
> image overlays are computed upstream of the flag and are
> painter-agnostic.

## What this surface ADDED to the template

* **A flat keyed `For` of ABSOLUTE units beats a nested row tree.**
  The transcript's rows are coordinate frames, not layout
  participants: every placed line and fill rect is an
  absolutely-positioned element (`left`/`top` points) keyed
  `{row.key}#{unit}` — one `For`, no row carriers, no nesting. The
  scroll offset is applied INSIDE the reactive `top` maps (never
  baked, never a mount key — sticky streaming moves it every chunk),
  and the `For`'s source is WINDOWED to the viewport (units enter/
  leave as the offset moves — the Wave-B visible-rows shape arrived
  free: the tree cost is bounded by the viewport, not the
  transcript).
* **The For's DOM order is the PAINT order — fills first.** Sibling
  rects overwrite: a row's fill boxes must precede its text units in
  the items list or the opaque fills cover the glyphs (the first
  matrix run failed with exactly-blank user boxes over a painted
  raised background — the raster z-orders siblings, last wins). The
  old painter stamps all fills before any text; per-row fills-first
  mirrors it because rows never share screen rows.
* **The strikethrough bit is the one attribute asymmetry.** opentui
  packs SGR 9 at attribute bit 7, the cell grid at bit 8 — the
  bridge remaps after the copy (`~~del~~` markdown). Every other
  modifier maps 1:1 (`ecma_attributes` inverts
  `paint::modifier_bits` exactly).
* **The old painter's wide-char trailing cells were REF-divergent.**
  `paint_run` left a multi-cell grapheme's continuation cells at the
  base fill; the ref's opentui raster plants `'\0'` continuations
  carrying the run's style (the composer's `paint_text` already
  matched). Fixed at the shared painter (continuation planting) —
  no golden or rig corpus carries wide clusters, and the solid path
  plants them natively. The same class: `thought_entry_row` ran
  `\n` through as a GLYPH (the F6 bug user messages already fixed)
  — now hard-breaks per line, matching v2's ThoughtEntry and the
  solid path's native breaks.
* **SSE-broadcast replies are the FLAKY test seam; the open-time
  message fetch is the reliable one.** The PTY thought-cycle test
  serves its turn through the session-open `message.list` fetch
  (DESC wire order! `fetch_session_messages` reverses the page) —
  and the stub's fixture ids must be DISTINCT from the live-streamed
  pair (the snapshot merge drops local rows whose ids it carries —
  shared ids silently un-runs the session and eats the interrupt
  hint). State asserts read FORCED FULL REPAINTS (the SIGWINCH
  toggle): the diff writer splits a row's styled runs and skips
  unchanged gap cells, so even full-repaint strip_ansi text is the
  only contiguous surface.
* **The scroll driver IS the ported ScrollBar math — keep it at the
  store.** oc-tui-core's `ScrollDriver` implements
  `round(clamp(v, 0, scrollSize - viewportSize))` (the js_round
  halves-toward-+∞ behavior the long-reply-scroll corpus pins);
  `scroll_driver_matches_the_ported_scrollbar_math` proves the
  equivalence against `opentui-core::renderables::scrollbar::
  clamp_scroll_position` over a viewport × content × start × command
  matrix. Porting the viewport onto the ScrollBox renderable would
  move identical integers INTO the tree and force the hit rows,
  metrics and jump indicator to be re-derived from it — decision:
  the store driver stays; the solid tree is a pure paint of its
  window (the rig's long-reply-scroll steps are flag-symmetric).
* **Markdown: ONE solver, deferred re-homing.** The old path's
  markdown bytes come from `message/markdown.rs` (the C1 port of
  `@opentui/core MarkdownRenderable`'s text behavior) INSIDE the
  shared `render_transcript` walk; the solid bridge consumes its
  runs as chunks. opentui-core's own `MarkdownRenderable` block
  pipeline is a SEPARATE port of the same upstream — wiring the
  transcript to it now would fork the fallback contract's bytes.
  Re-home when the old path retires (then the C1 module's goldens
  move to the core pipeline's output in one lane).

## Instruments (this lane's record)

* **Unit gate**: `solid_transcript_frame_equals_the_old_renderer` —
  11 scenarios (the groups fixture, running tools × 2 clock phases,
  the disclosure expansion, a running Thinking header, markdown-rich
  CJK + model display names, the error part, multiline user text +
  the image strip, the 10-turn vscroll fixture) × widths 80/120/40 ×
  heights 16/23 × 4 scroll positions (top / a row-cutting mid /
  bottom−1 / sticky) — cell for cell. Plus streaming+scroll+expand
  patch-in-place churn and resize-rebuild gates.
* **Rig A/B** (`docs/m4-rig-ab.log`, 28 invocations = 7 scripts × 2
  sizes × 2 runs × both flags): 11/14 (script,size) pairs IDENTICAL
  mismatch-step unions; the 3 others differ only in the
  placeholder-carrying steps (boot/dismiss/variant-dismiss) — the
  documented (b)-class home-placeholder coin, proven by the coin
  flipping run-to-run within the SAME flag (type-edit-submit 120x30
  solid: run0 clean, run1 carries them).
* **PTY**: `pty_scroll_keys_move_the_transcript` parametrized over
  both paths; the new
  `pty_transcript_cycle_thought_expand_solid_path` (open → the
  thought turn renders → SGR click the disclosure header → the
  guttered body expands → second click collapses — the hit-row
  bridge through the solid geometry).
* **Budget** (release, 200×50 session route, 300-message transcript,
  both paths pinned): old idle 7423 µs / churn 7538 µs worst; solid
  idle 8845 µs / churn 9781 µs worst — ≈1.2–1.3× the old path
  (both measured under the gate's CPU cap; the shared solver walk
  dominates), vs the 40 ms gate. All other budget gates re-checked
  green post-flip.
* **Flag protocol**: default flipped ON after the proof; 645 oc-tui
  tests + 2808 workspace tests + the 121-test pty-profile run green
  through the solid default.
* **Release size**: 66,120,736 → 66,115,648 B (**−5088 B, −0.008%**
  — the transcript tree + bridge ride the already-linked opentui
  graph; the delta is linker noise, effectively zero).

---

# Lane M5 — the sidebar + home: two surfaces, one lane

> Status: **done** (lane/m5). The session route's right-hand pane
> (`routes/session/sidebar.tsx` + the `sidebar.content`/
> `sidebar.footer` slots) and the home route's chrome
> (`routes/home.tsx` + `feature-plugins/home/footer.tsx`) ported as
> `crates/oc-tui/src/components/sidebar_solid.rs` +
> `home_solid.rs`, selected by `OC_SIDEBAR_OPENTUI` /
> `OC_HOME_OPENTUI` (both default ON after this lane's proof; the
> hand-rolled renderers stay one release as the `=0` fallbacks — two
> flags because the surfaces differ in the rig matrix). The store's
> sidebar state (`SidebarView`/`SidebarSections`), the visibility
> and overlay-close keymaps, and the route assembly are untouched.

The sidebar is the M4 flat-units shape verbatim: `render_sidebar`
(the old walk) stays the ONE solver, and the tree is a keyed `For`
of ABSOLUTE units — the card's fill rect first, then every placed
line (`wrapMode none` + `flexShrink 0`, explicit-bg chunks). The
home surface took the M3 route instead: the geometry was EXTRACTED
(`home::plan_home` — the footer ladder, the div_ceil-2 centering,
the paste origin) so the old painter and the solid chrome tree
consume the same integers; the wordmark is eight STATIC text rows
(two per art row — the halves never touch, the `gap={1}` cell keeps
the page fill), and the sessionless composer stays lane M3's own
mount PASTED at the plan's origin through the same
`render_prompt_frame_dispatch` chain (the flags compose: either
home path paints either composer path's frame).

## What these surfaces ADDED to the template

* **ratatui's `set_style` MERGES; the raster overwrites.** The old
  pane painter writes through `Cell::set_style`, which patches
  fg/bg and ORs modifiers into the EXISTING cell — a unit painted
  over an earlier unit's bold cells inherits the bold bit (a short
  pane stacks the card/footer over the section text; the golden
  corpus never overlapped so this never showed). The bridge bakes
  the merge at shape time: one attrs-accumulation grid (every text
  unit ORs its ECMA word onto its cells, in paint order, through
  the same atom walk `paint_run` uses), then chunks split per cell
  on (fg, bg, merged-attrs). Symptom class: "attrs 1 vs 0 on the
  glyph that overlaps an earlier bold row."
* **For keys must be POSITION-STABLE, not index-stable.** Keying
  placed units by their vector INDEX churns every key when content
  inserts above (a plugin section landing shifted every card key),
  and the solid keyed diff mishandled the full-churn frame — an
  inserted-then-removed element left a hole (the location line
  vanished). The fix keys by CONTENT POSITION (`t:{tag}@{y}` /
  `u@{y}`, index only as a collision suffix): the pane's chrome is
  bottom-anchored, so a section landing inserts two clean keys and
  everything else patches in place.
* **The mount key must cover every BAKED prop.** The home tree
  bakes the version's `left` at build time; a version-length change
  moved the label without tripping the (width, height, bg, row)
  key, and the label overflowed the right edge. Fix: key on the
  WHOLE plan (it's `PartialEq`) — any geometry field moving rebuilds
  the small tree.
* **The wordmark's per-cell shadow art is run-split chunks.** Each
  art row's cells alternate fg/bg by glyph class (`_` space-over-
  shadow, `^` `▀`-over-shadow, `~` `▀`-IN-shadow); adjacent cells
  with the same (fg, bg, attrs) merge into one explicit-bg chunk,
  two text elements per row (the gap cell stays OUTSIDE both).
* **The resting shimmer IS the base renderable.** The tsx's
  `title_shimmer` subclasses `TextRenderable`; its animated phases
  (rename-pending pulse, arrival wipe — scratch buffer +
  colorMatrix) only run while `animating`, which needs rename
  state the store doesn't carry. At rest it renders as plain
  BOLD `text.base` over `raised.base` — exactly what both painters
  paint. The animated phases are the registered seam: O2's
  `tab_pulse` covers the sustain math; the envelopes would land in
  a core renderable when a later lane carries rename-pending into
  the store.
* **The scrim stays at the route compositor** (M2's backdrop rule
  again): the ≤120 manual-open overlay pastes an opaque-black
  stand-in under the floating pane — painter-agnostic; the
  alpha-blend seam stays registered in `session.rs`.

## Instruments (this lane's record)

* **Unit gates**: `solid_sidebar_pane_equals_the_old_renderer`
  (fixture/live/plugin-sections/dismissed/long-title/many-sections
  × heights 30/24/22/16/8, cell for cell) +
  `solid_home_frame_equals_the_old_renderer` (idle/typing/
  multiline/popup/plugin-actions/version-gate/carets × widths
  40/60/80/120/160 × heights 20/24/30) + churn/resize gates on
  both surfaces.
* **Rig A/B** (`docs/m5-rig-ab.log`, 12 invocations = 6 scripts ×
  2 flags × 2 runs): FLAG-SYMMETRIC — identical mismatch-step
  unions on every (script,size) pair; the only cell deltas are the
  ref-side LIVE "Context / N tokens" meter (the documented
  `SidebarSections.context` seam — the ref's sidebar context widget
  was never modeled on our side; pre-existing on BOTH paths).
  boot-home/resize/click-coverage-home flip ok↔mm on the
  (b)-class home-placeholder coin WITHIN each flag; help-dialog
  (regression) carries its deterministic mm2 (submit/help-open
  style backlog) on both.
* **PTY**: `pty_sidebar_toggles_*` + `pty_sidebar_plugin_section_*`
  parametrized over both paths; the new
  `pty_sidebar_overlay_cycle_solid_path` (hidden at 80 →
  `<leader>b` overlay over the scrim → ESC dismiss through the
  already-ported close-keymap layer).
* **Budget** (release, per-process): home old 43/55 µs worst →
  solid 112/111 µs (≈2–2.5×, ~0.11 ms); sidebar pane old 66/69 µs
  → solid 141/227 µs (≈2.1–3.3×, ~0.23 ms) — vs the 40 ms gate;
  all 13 release budget tests green.
* **Flag protocol**: defaults flipped ON after the proof; 661
  oc-tui tests + 2824 workspace tests + the serialized pty profile
  green through the solid defaults (the home goldens now pin the
  SOLID path's bytes — `render_home_view` dispatches).
* **Release size**: 66,120,736 → 66,231,424 B
  (**+110,688 B ≈ +108 KiB, +0.17%**) — the pane's unit For + the
  home chrome tree + the shared `plan_home` extraction ride the
  graph opentui already occupies.

---

# Lane M6 — the subagent surfaces: the restore + the solid port

> Status: **done** (lane/m6). The composer's subagent picker panel
> (v2's `Composer` + `subagents-tab.tsx` at the corpus3 geometry)
> and the exit farewell (`sessionEpilogue`) ported as
> `crates/oc-tui/src/components/picker_solid.rs` +
> `farewell_solid.rs`, selected by `OC_SUBAGENT_OPENTUI` (default
> ON after this lane's proof; the hand-rolled painters stay one
> release as the `=0` fallback). The store's picker state
> (`PickerState`/`PickerTab`/`PickerEntry`), the `session.child.*` /
> `composer.*` keymap family, the family walk, the child-view
> navigation rooting, and the dispatcher are painter-agnostic.

## The lane's precondition was a REVERT (read this before trusting any lane's "current state")

The E2 subagent surfaces (2b7ae6d — picker, farewell, tool card,
child view, pty suite) had been ACCIDENTALLY REVERTED by lane/
chrome's merge (967a216 deleted `subagent_picker.rs`,
`presentation.rs`, `pty_subagent.rs` and rewound the store/keymap/
transcript seams); M0-M5 built on the wounded tree. The lane began
by RESTORING E2's surfaces onto the current architecture (the
`=0` fallback path), then ported. Lesson: a lane spec that says
"the current implementation lives at X" must be verified against
the TREE, not the roadmap — `git log --all -S "<symbol>"` first.

## What this family ADDED to the template

* **A stdout surface ports as tree + raster→ANSI serializer.** The
  farewell is not a frame — v2 writes it to stdout after teardown,
  and the rig compares the BYTES. The solid path mounts the
  epilogue grid as STATIC tree rows (the M5 wordmark shape over
  presentation.rs's own art tables), renders through a real
  `SolidRenderer`, then walks the raster back to presentation.ts's
  byte stream: wordmark rows emit PER GLYPH (the right half's
  default-fg is the literal `${reset}`-as-fg prefix — a cell class,
  not a color), label rows merge into RUNS (`weak()` + bold), and
  an EMPTY bold value still emits `\x1b[1m\x1b[0m`. The unit gate
  pins the bytes equal to the string builder AND the raster cells
  equal to the string builder's own parse.
* **The mount budget vs the paste row.** The picker mount is 12
  rows but the transcript's height BUDGET is the panel's 11 — the
  12th row is a pad BELOW the panel (the prompt mount analog pastes
  its pad differently). Symptom class: "the whole panel is one row
  too high and the transcript one row too short." Rig-pinned.
* **Per-session chrome inputs ride TranscriptOptions.** The
  user-block rail keys on the SESSION's agent
  (`session_agent_rail_color` — the registry's agent through the
  visible-agent list), and the subagent card's running oracle rides
  `subagent_running` — both painter-agnostic, both defaulted for
  fixtures (`None`/empty).
* **`up` at the top closes even when the filtered list is EMPTY**
  (the tsx's close check precedes the move guard; `down` stays
  inert on empty). Found by the new PTY keymap cycle, not the
  corpus.
* **The rig reuses any existing binary.** `rust_server::
  build_binary` returns early when `target/debug/opencode-rust`
  exists — after source edits the rig keeps spawing the STALE
  binary and the A/B chases ghosts. `rm` it before every rig run.

## Instruments (this lane's record)

* **Unit gates**: `solid_picker_panel_equals_the_old_renderer`
  (open/browse/show-inactive/child-mounted/shell-tab × widths
  40/80/120/160, cell for cell — including the `Running`-over-bold-
  label attrs merge, the M5 gotcha again) +
  `selection_churn_patches_in_place` +
  `solid_farewell_bytes_equal_presentation_ts` (6 titles × 3 ids,
  byte-for-byte) + `farewell_tree_grid_matches_the_string_builder_parse`
  + the restored E2 store/picker/presentation suites (the state
  machine, the family registry, the dispatcher walk, the epilogue
  truncation).
* **Rig A/B** (`docs/m6-rig-ab.log`): subagent-basic/browser/
  sidebar × `--quick` × both flags — IDENTICAL mismatch-step unions
  (browser/sidebar exactly; basic modulo the documented (b)-class
  home-placeholder coin, proven flipping within each flag). After
  the lane's geometry fixes the picker/card/child/farewell steps'
  remaining deltas live ONLY inside the per-run stats rows and the
  exit step's session-id characters.
* **PTY**: `pty_subagent_surfaces_card_picker_child_farewell`
  parametrized over both paths (the full walk: card → picker →
  ctrl+a → child → esc → farewell stdout) + the new
  `pty_picker_keymap_cycle_solid_path` (down opens → up-at-top
  closes → reopens → ctrl+a → arrows → tab cycle → esc) + the 160
  col sidebar suppression + the home-exit negative.
* **Budget** (release, 200×11 panel, 5 entries): old 75/67 µs,
  solid 505/489 µs worst/avg (selection churn 74/505 µs) — ~6.7×
  the old path, 0.5 ms absolute, vs the 40 ms gate; all 15 release
  budget tests green post-flip.
* **Flag protocol**: default flipped ON after the proof; 733
  oc-tui tests + 2850 workspace tests + the serialized pty profile
  + all 15 release budget gates green through the solid default.
* **Closing gate**: `verify-lane.sh final` — fmt/clippy/pty/budget
  green; the rig full matrix (every script × sizes × `--runs 2`,
  cache OFF) completed as chunked per-script runs after a session
  glitch killed the monolithic invocation mid-corpus
  (`docs/m6-rig-ab.log`). Two script fixes landed with it: the
  clippy line's missing `--` before `-D warnings`, and the release
  budget line's bogus `--run-ignored only` (0 tests matched).

---

# Lane M-theme — the theme system: a DATA+chain port, not a surface

> Status: **done** (lane/theme). v2's theme system — the builtin
> table, the registry's `system` slot, and the themes dialog's
> LIVE-PREVIEW apply path — ported onto the solid stack. No new solid
> tree: the palette already flows into every mount key (M2 colors /
> M5 whole-plan), so a palette switch repaints everything through the
> EXISTING reconciler rebuilds. The final sweep's largest (a)-class
> residual — themes-nord's step-4 FULL-GRID divergence (1920 cells) —
> dissolved here.

## What the "nord debt" actually was (the table was already parity)

`crates/oc-theme/assets/` is byte-identical to the ref's
`packages/tui/src/theme/assets/` — all 33 v1 assets + the v2 default
(sha256-verified by a GENERATED digest test,
`builtins.rs::embedded_assets_match_the_reference_source_bytes`). The
nord divergence was NEVER the values; it was the APPLY PATH:

* **`dialog-theme-list.tsx` is a live-preview dialog.** `onMove` and
  `onFilter` call `themes.set()` IMMEDIATELY (typing "nord" previews
  nord across the whole app — that is the full-grid restyle the rig
  saw at step 4); an empty filter resets to the captured initial; a
  close without submit REVERTS (`onCleanup`); a submit confirms.
  Ported as `DialogEntry.theme_initial` + `preview_theme_move` /
  `preview_theme_filter` + the revert-on-close sweep + the submit's
  confirm-before-clear.
* **`current` is the MOUNT-TIME capture, not the live theme.** The `●`
  marker and the filter-clear re-focus key on the entry's captured
  initial — under `filter: "nord"` the initial (`opencode`) is
  filtered out and the marker disappears. A live-theme `current`
  re-marks the previewed theme (the rig caught exactly this).
* **The registry always exposes a `system` slot**
  (`listThemes`'s final spread: `systemTheme ?? themes.system ??
  themes.opencode`). Headless (no cached palette, no OSC probe) it
  resolves to the opencode document — 34 dialog entries, `system`
  sorted between synthwave84 and tokyonight. The generated-from-
  palette `systemTheme` (OSC 11 → `generateSystem`) is the registered
  seam for a later system-theme lane.
* **The open window CENTERS on the current option** (the mount
  effect's `scrollAfterLayout(true)`), in the REAL viewport
  (`min(rows, H/2-6)`) — not an edge-reveal at a hardcoded height.
  Clearing a filter to empty re-focuses + re-centers `current` (the
  filter effect's empty branch).

## Rig verdicts (docs/mtheme-rig-ab.log has the full record)

* themes-nord FULL (both sizes × 2 runs, cache OFF): step 4's
  1920-cell full-grid collapse → 5 style-only cells at 80x24 (29 at
  120x30 incl. the coin text), ALL in the two PRE-EXISTING families
  (below); the run pattern (6/2, 1/7, 1/7, 6/2) matches the final
  sweep's own coin distribution exactly.
* Every other script ran FULL-LIVE (the dialog/touched set) or
  --quick (the rest): same-or-better mismatch distributions vs the
  final sweep everywhere; NO new divergence class.

## The two pre-existing style families this lane DID NOT create

1. **backdrop-fg**: ref paints fg=idx15 (default white) on
   dialog-covered cells; ours keeps the underlying chunk fg. Same
   signatures in the final sweep on the DEFAULT theme
   (click-coverage-dialog#3/#4, palette#2).
2. **input-caret cell**: after typing, our dialog search paints the
   caret cell with the focused-formfield pair; ref paints it plain
   (the terminal cursor is the caret). Same family at
   hover-sweep#4 (default theme).

Both need bridge/seam work in the dialog surface, not theme values.

## The idx216↔233 family: NOT theme debt (measured)

click-coverage-session/hover-sweep's `ref[bg=idx216] our[bg=idx233]`
models-dialog rows: idx216 IS `background.action.primary.focused`
(the tan) — BOTH stacks paint that exact token. The ref surface is
the mini footer panel (`mini/footer.command.tsx` "Select model",
`RunFooterTheme.actionFocusedBg`); ours is the main ServerList dialog
("Switch model"). The family dissolves with the MINI-FOOTER surface
port (its own lane), not with any theme-table change.

## Instruments (this lane's record)

* **Unit**: the generated ref-asset digest gate (34 sha256s); 3
  store-level apply-path tests (preview/revert/confirm, submit-keeps,
  the style signal reaches `render_app`); registry 34 with the system
  slot resolving headless-identical to opencode. oc-theme 25/25,
  oc-tui 687/687, workspace 2856/2856, pty profile 2856/2856.
* **Budget** (release, 200×50, theme dialog open, palette flipped
  EVERY frame — the worst-case preview churn): old 2728/2153 µs,
  solid 5043/3665 µs worst/avg (~1.85×, ~5 ms absolute) vs the 40 ms
  gate. All 17 release budget tests green.
* **Operational**: the click-coverage scripts PIN their size in the
  script JSON (`--sizes` cannot override — a mid-chunk discovery);
  sweep-class scripts cost ~13 min/run live — chunk per script, the
  40-min tool-call cap bites mid-corpus otherwise.

---

# Lane M-engine — the subsystem swap: the editing ENGINE + the keymap ENGINE

> Status: **done** (lane/engine). The first NON-surface lane: not a
> painter but the two state machines beneath the composer — the
> editing engine (`oc-tui-core::text_edit::TextArea`, hand-rolled)
> and the keymap dispatch engine (`oc-tui-core::keymap::engine`,
> hand-rolled interpretation) — replaced by the opentui-rust ports
> AS LIBRARIES (`opentui-core`'s `EditBuffer`/`EditorView` textarea
> line; `opentui-keymap`'s engine), behind `OC_ENGINE_OPENTUI`
> (default ON after this lane's proof; `=0` keeps the hand-rolled
> pair one release). The K-1..K-9 registered keymap deltas dissolved
> here — the dispatch semantics now ARE upstream 0.5.14's (v2's
> engine), no case special-cased.

## The subsystem pattern (what differs from the surface lanes)

* **The facade stays; the engine beneath swaps.** The `Keymap`
  facade's public surface (mod.rs) and the DATA table (table.rs — v2's
  `config/keybind.ts`) are untouched; the facade's `engine` field
  became an `EngineSel::{Legacy, Opentui}` enum both variants answer.
  Same for the composer: `Composer.edit` became `EditCore` with the
  legacy `TextArea` and `TextAreaOpentui` behind one internal
  surface. oc-tui's pump code changed ZERO dispatch calls — only the
  ~25 direct field reads (`edit.caret`, `edit.text = …`) became
  method calls.
* **Translate at the boundary, never inside the port.** The donor
  keymap's binding grammar is `<leader>e` TOKENS + CONCATENATED
  sequence parts ("gg") — there is NO `>` separator (our table's
  internal form). The adapter's `chord_spec` renders donor grammar;
  strokes map `alt`↔`meta`, `hyper` stays distinct (K-5). The editor
  adapter converts char offsets (the composer's model) ↔ the buffer's
  display-width offsets through the buffer's own cluster table —
  wide/CJK clusters move exactly like upstream's editor.
* **Register EXACTLY v2's Provider set** (context/keymap.tsx:86-150):
  comma bindings, the enter/esc/pgdown/pgup alias expander,
  base-layout fallback, escape-clears (any escape), backspace-pops,
  timed leader at v2's explicit 2000 ms. `lookup_base` = the mode
  data key set to a sentinel no layer names (named-mode layers go
  inert); the pending sequence survives.
* **Commands as sink closures**: every dot command registers a
  `Command` whose run closure checks the shared disable gate and
  returns Handled — the legacy always-accept runner with rejection
  fall-through intact, and `set_command_enabled` needs no rebuild.
* **The engine owns wrap**: `EditorView` runs WrapMode::Word
  (TextareaRenderable's default) at the composer's INNER width —
  `prompt_text_width` was extracted from `mount_height` so the
  display solver, the height walk, and the engine's visual motions
  share ONE integer (the app feeds it at boot + resize).
* **The observable-only bridge**: the opentui engine's per-op bridge
  (`run_tracked`) mirrors text in/out but `mirror_text` is a NO-OP on
  matching text — the first draft reset the caret+history on EVERY
  keystroke (the per-op bridge must be inert when already in sync).

## Engine-behavior deltas the swap MADE (upstream-faithful, each pinned)

1. Plain moves over a selection COLLAPSE to the edge without
   stepping (`collapseSelectionToEdge`); legacy cleared + moved.
2. Word boundaries: script-transition rules ("fn foo()" alt+f hops
   3,7,8 vs legacy 3,6,8).
3. Line home/end readline jumps at the row edges (home at col 0 →
   previous line's end; end at EOL → next line's start).
4. Visual motions follow the WRAP model (vlines + boundary affinity
   — the cursor stays on the last grapheme, not past the break's
   trailing space).
5. `deleteForward`'s phantom undo snapshot (the ported upstream
   double-snapshot — one extra undo press walks a same-state restore).
6. Undo never restores across an external `setText` (rope history
   clears — no undo-after-submit).
7. The pending display is the `<leader>` token form; the alias
   expander revived diff.toggle's "enter" spec (dead on legacy — the
   pump speaks "return").

## Instruments (this lane's record)

* **Unit**: the resolution parity table — EVERY default-table spec
  (~202) resolves identically on both engines in its layer's mode
  context (kind+command; displays normalized); the K-1..K-9 suite
  (before/after per delta); conflict parity; lookup_base parity;
  overrides/leader-rebind/owned rebuilds; the editor parity +
  delta suites. oc-tui-core 77/77 both paths; oc-tui 688/688 both
  paths; workspace 2887/2887 both paths.
* **Rig A/B** (`docs/mengine-rig-ab.log`): type-edit-submit,
  composer-clear, palette, tabs-keyboard (LEADER-heavy), rename-
  dialog, multi-session × both sizes × 2 runs × both flags —
  IDENTICAL mismatch-step unions; the only count deltas are the
  per-run stats-row digit widths and the (b)-class home-placeholder
  coin (proven flipping WITHIN one flag). FULL live re-runs (cache
  OFF) of the same corpus through the default engine: same
  distributions, NO new divergence class.
* **PTY**: the serialized profile 2885/2885 on BOTH paths + the new
  `pty_engine_cycle_type_worddelete_undo_leader_escape_{legacy,
  opentui}_path` (the shared observable cycle: type → ctrl+w → undo
  → leader-arm + escape-clear → prompt.clear).
* **Budget** (release, 200×50, the full pump round-trip on a 2 KB
  CJK-mixed draft): legacy worst 225 µs / avg 67 µs; opentui worst
  1270 µs / avg 399 µs — ~5.6×, 1.27 ms absolute vs the 40 ms gate;
  all 19 release budget tests green.
* **Release size**: 66,231,424 → 66,727,072 B (**+495,648 B ≈
  +484 KiB, +0.75%**).
* **Flag protocol**: default flipped ON after the proof;
  `OC_ENGINE_OPENTUI=0` keeps the hand-rolled pair one release (the
  retirement lane removes text_edit.rs + keymap/engine.rs and the
  enum hops).
