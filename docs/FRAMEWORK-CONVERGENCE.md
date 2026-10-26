# G-11 framework convergence — disposition ledger

State: 2026-10-24, lane P3-DIFF (worktree `p3-diff`); rendering-stack
section superseded 2026-10-26 by lane R0 (see below). Source recon:
`.slim/deepwork/g-series/topic-tui-recon.md` § "G-11 framework
convergence".

Upstream pin rule: **opentui is consumed via the workspace git rev
(`bee57db`, crate path `opentui-core`) only.** No local `[patch]`
overrides, no vendored copies of upstream crates; an upstream change
means bumping the git rev in the workspace `Cargo.toml` and letting
`Cargo.lock` carry it.

## Converged

| Pair | Disposition |
| --- | --- |
| **clipboard** | Converged (P2-CLIP). `oc-tui`'s `clipboard.rs` delegates to `opentui_core::clipboard` (host + OSC52 read/write, the reference `createTuiClipboard` twin). |
| **paste** | Evaluated, **deliberately not adopted**. `opentui_core::paste` (`decode_paste_bytes`/`strip_ansi`) is a byte-decode helper; the port's paste path (`Event::Paste` → `App::paste_terminal_text` → `Composer::paste_resolved` → attachment resolution) already owns the same logic at a smaller footprint and is pinned by existing tests. Adopting it would be churn without a behavior win — revisit only if the two paste pipelines drift. |
| **diff logic** | Converged (this lane). Patch PARSING is single-sourced in `opentui_core::renderables::diff::parse_patch` (jsdiff 9.0.0, line-faithful). `oc-tui`'s `components/diff_viewer.rs::parse_patch` is now a thin adapter onto opentui's `StructuredPatch`; the ratatui renderer, keymap, tree behavior, and all goldens are unchanged. Details below. |
| **sound** | Converged (SOUND-CONV). `oc-tui`'s `sound/audio.rs` plays through `opentui_core::audio` (`device-audio` feature, opentui rev 18e2904) behind the existing `SoundHalf` seam; the duplicated local engine (`sound/inproc.rs` + `third_party/miniaudio-sys`) is deleted — nothing else used it. Headless degradation is a silent no-op (`err_device` → `false`), the system-player spawn fallback for non-Linux targets stays (documented port divergence). |

### Diff convergence details

* The duplicated unified-diff scanner in `oc-tui` (~50 LOC) is deleted;
  structural parsing, hunk-count validation, git/rename extended
  headers, and error semantics belong to opentui-core.
* The adapter (kept locally, by design — it is model mapping, not
  parsing): pre-assigned row numbers (`PatchRow::Context/Deleted/Added`
  with `u64` numbers), context-marker-space stripping, and the display
  header text + RAW starts re-derived from the source header lines
  (jsdiff bumps a `,0` count's start by one — parse.js:458-463 — while
  the viewer's header-hiding golden heuristic reads the raw start).
  Known edge (gate-3): when header/hunk counts misalign (e.g. an `@@`
  embedded in context text), starts fall back to the jsdiff-bumped values,
  shifting `,0` hunks by 1 vs the golden heuristic — rare, accepted.
* `\ No newline at end of file` markers: jsdiff keeps them in
  `hunk.lines`; upstream Diff.ts renders none of them; the adapter
  drops them (upstream disposition). The legacy scanner had rendered
  them as context rows — no golden covers that, and upstream behavior
  wins.
* Malformed patches: jsdiff `Err` (count mismatch, invalid body line,
  missing file header) → no hunks → the "No patch available for this
  file." fallback, matching upstream's error-view disposition. The
  legacy scanner tolerated anything; the vcs.diff endpoint emits valid
  unified diffs.
* **line_number** (checked, not converged): the viewer's gutter is
  painted inline in `paint_diff_row` (`␣` + number + sign + padding
  into `Placed`/`Fill` ratatui rows, golden-pinned bytes);
  opentui's `LineNumberRenderable` is a taffy-layout renderable with
  its own width/sign model. Not trivially lossless under the
  byte-identical bar — pending the R3 byte-parity spike
  (`LineNumberRenderable` vs the pinned gutter bytes); if the spike
  fails, line_number joins terminal_pane as a permanent local keep
  (see "Rendering-stack migration" below).

Adapter-contract test:
`diff_viewer::tests::diff_model_agrees_with_opentui_parse_patch`
(multi-hunk fixture with context, mixed add/del runs, a no-newline
marker, a git rename, and a malformed rejection case) pins the two
models together.

## Blocked upstream (with unblock conditions)

| Pair | Blocker | Unblock condition |
| --- | --- | --- |
| **syntax** | oc-syntax: 16 + 4 languages (vendored grammars + `.scm` queries) emitting ratatui `Style` spans; opentui-rust `TreeSitterClient`: 5 languages emitting `StyledText`. **Correction (2026-10-25, user)**: opentui (TS upstream) ALSO carries only ~5 languages — the ref TUI's full language experience comes from its own runtime `.wasm` download system (parsers-config.ts), not from opentui core. So this is not "opentui-rust lagging opentui"; the 20-language catalog is this port's own advantage. Convergence would still be a language-catalog AND output-type regression. | Converge when an upstream (either) carries the language catalog AND a ratatui-compatible span output. (The 2026-10-26 R0 correction removed "or oc-tui's rendering migrates to opentui buffers" — the hybrid rendering stack is now the declared target, not a precondition; see the section below.) |
| **terminal_pane** | **Correction (2026-10-25, user)**: terminal_pane is NOT an opentui feature at all — opentui (TS) has no terminal pane, so opentui-rust has nothing to port and never will. oc-tui's 468 LOC pane is protocol/plumbing unique to this port. | **Permanent local keep.** There is no upstream counterpart to converge to; embedded_terminal (opentui-rust's VT engine) is a different layer and stays irrelevant to pane rendering (the hybrid rendering stack is the declared target — R0, 2026-10-26). |

## Unblocked upstream, pending in-repo convergence

| Pair | Status |
| --- | --- |
| *(none — sound converged 2026-10-25, see the Converged table)* | |

## Rendering-stack migration (ratatui → opentui) — DECLARED TARGET (lane R0, 2026-10-26)

**Status correction (2026-10-26, lane R0)**: this section previously
recorded the migration as "not scheduled". That was stale — the
strangler migration was already executed via lanes **M1–M6 + M-theme +
M-engine** (see `docs/migration-m1-tabstrip.md`; every
`OC_*_OPENTUI` selection flag is retired and the `*_solid.rs` trees
are the only live renderers; the old ratatui painters were deleted
after cell-for-cell parity gates). Full analysis:
`.slim/deepwork/render-migration-assessment/oracle.md`.

The current **hybrid architecture is the TARGET, declared — not
transitional debt**:

```
store → opentui-solid tree per surface → SolidRenderer.flush()
  (taffy layout + raster) → OptimizedBuffer → frame_from_buffer
  cell copy → oc-tui (ratatui) Frame → render/ansi.rs serializer → PTY
```

* ratatui stays as the compositor cell type / final Frame / diff
  blitter; opentui is the per-surface raster; layout is taffy. The
  goldens pin this in-house `render::Frame` vocabulary and are NOT to
  be re-pinned for an upstream renderer.
* Perf is inside budget (solid worst case 1.1×–6.7× per surface,
  ≤ 5.0 ms absolute vs the 40 ms gate; re-check if a surface exceeds
  ~10 ms worst).
* Upstream changes are still consumed via git rev bumps only (pin
  rule above); the ratatui dependency itself is not removed by any
  planned work.

### Remaining tail (phased; per-surface rollback is the point of the
strangler)

| Phase | Scope | Size | Kill criterion |
| --- | --- | --- | --- |
| R1 | overlay chrome (toast, console overlay, devtools, plugin_page, location_missing, migration_overlay, tab rail+tooltip) | **done** (all seven migrated; see the R1 section in docs/migration-m1-tabstrip.md) | any NEW divergence class in the rig unions → keep that surface hand-rolled |
| R2 | footer forms family (`footer/*.rs`; extract the form/permission/menu STATE MODEL painter-agnostic first) | L | form-focus/keydown semantics can't stay store-side, or worst-frame budget > ~10 ms → a permanently hand-rolled footer family is an acceptable outcome |
| R3 | diff_viewer — gated on a `LineNumberRenderable` byte-parity spike first (gutter sign/width model, `,0` hunk-start bumps); adopt only if byte-identical or the re-pin ≤ the diff golden set (~tens of fixtures) | M, gated | any diff rig corpus loses flag-symmetry → keep local |

### Never migrate

* `terminal_pane` — no upstream counterpart exists (the G-11
  correction); permanent local keep.
* `mini/*` — separate embedded runtime with its own loop.
* markdown re-home to opentui-core's `MarkdownRenderable` — stays
  deferred behind its own byte-parity lane (the M4 recorded decision;
  two ports of the same upstream would fork the fallback contract),
  or forever.
* The final Frame / ANSI-serializer layer — ratatui stays as the
  compositor cell target; removing it buys nothing and re-opens every
  golden.

Consequence for the blocked pairs above: **syntax** stays blocked on
its own double precondition (language catalog AND span output) — the
rendering-stack decision is no longer part of its blocker;
**line_number** is no longer "blocked on the migration" but pending
the R3 byte-parity spike, with a permanent local keep (like
terminal_pane) as the recorded fallback.
