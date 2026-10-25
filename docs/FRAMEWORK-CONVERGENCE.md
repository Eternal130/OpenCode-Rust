# G-11 framework convergence — disposition ledger

State: 2026-10-24, lane P3-DIFF (worktree `p3-diff`). Source recon:
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
  byte-identical bar — revisit only if oc-tui adopts the opentui
  rendering pipeline (see `terminal_pane` below).

Adapter-contract test:
`diff_viewer::tests::diff_model_agrees_with_opentui_parse_patch`
(multi-hunk fixture with context, mixed add/del runs, a no-newline
marker, a git rename, and a malformed rejection case) pins the two
models together.

## Blocked upstream (with unblock conditions)

| Pair | Blocker | Unblock condition |
| --- | --- | --- |
| **syntax** | oc-syntax: 16 + 4 languages (vendored grammars + `.scm` queries) emitting ratatui `Style` spans; opentui-rust `TreeSitterClient`: 5 languages emitting `StyledText`. **Correction (2026-10-25, user)**: opentui (TS upstream) ALSO carries only ~5 languages — the ref TUI's full language experience comes from its own runtime `.wasm` download system (parsers-config.ts), not from opentui core. So this is not "opentui-rust lagging opentui"; the 20-language catalog is this port's own advantage. Convergence would still be a language-catalog AND output-type regression. | Converge when an upstream (either) carries the language catalog AND a ratatui-compatible span output (or oc-tui's rendering migrates to opentui buffers — see "Rendering-stack migration" below). |
| **terminal_pane** | **Correction (2026-10-25, user)**: terminal_pane is NOT an opentui feature at all — opentui (TS) has no terminal pane, so opentui-rust has nothing to port and never will. oc-tui's 468 LOC pane is protocol/plumbing unique to this port. | **Permanent local keep.** There is no upstream counterpart to converge to; embedded_terminal (opentui-rust's VT engine) is a different layer and only becomes relevant under the rendering-stack migration below. |

## Unblocked upstream, pending in-repo convergence

| Pair | Status |
| --- | --- |
| **sound** | **Upstream completed real audio output (2026-10-25, user report)** — the original blocker (opentui-rust `audio.rs` was decode-only `HeadlessAudioLib` + NULL sink) is gone upstream-side. Convergence is now actionable in this repo: bump the workspace opentui git rev past the audio-output landing, swap `sound/inproc.rs` (miniaudio) for `opentui_core::audio` behind the existing `SoundHalf` seam, keep behavior parity pinned by the existing sound/attention tests (v2 trigger matrix + OSC 9/99/777 + 5 vendored assets). Until that lane runs, the inproc engine stays (it is audible and tested); treat this as a follow-up lane, not a gap. |

## Rendering-stack migration (ratatui → opentui) — recorded context

oc-tui today renders its main path onto **ratatui** `Frame` cells; the
opentui consumers in-tree are the experimental `*_solid.rs` path and
`message/markdown.rs` (markdown/code/scrollbar/text/editor renderables).
The remaining non-converged surfaces (syntax output type, line_number's
taffy layout model, any render-side of terminal_pane) are all blocked on
the same strategic decision: migrating oc-tui's rendering stack onto the
opentui pipeline. That migration is **not scheduled** — it is the
recorded direction for whenever convergence of those pairs is worth the
golden-frame churn:

* full stack migration makes line_number and opentui renderables
  first-class, unlocks embedded_terminal reuse for pane rendering;
* until then, convergence stays at the lossless logic layer
  (clipboard/paste-eval/diff-parse, done) and everything
  rendering-coupled keeps a local implementation with golden pins;
* upstream changes are consumed via git rev bumps only (pin rule above);
  the ratatui dependency itself is not removed by any currently planned
  work.
