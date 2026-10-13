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
