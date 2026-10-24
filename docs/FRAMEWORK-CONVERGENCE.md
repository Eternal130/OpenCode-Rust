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
| **sound** | opentui-core `audio.rs` is `HeadlessAudioLib` + NULL sink — decode only, NOT audible. The port's inproc miniaudio (412 + 630 LOC) is audible. | Converge only when opentui-core gains a real playback sink (device output); until then the inproc engine stays (regression otherwise). |
| **syntax** | oc-syntax: 16 + 4 languages (vendored grammars + `.scm` queries) emitting ratatui `Style` spans; opentui `TreeSitterClient`: 5 languages emitting `StyledText`. Convergence today = language-catalog and output-type regression. | Converge when upstream carries the language catalog and a ratatui-compatible span output. |
| **terminal_pane** | oc-tui 468 LOC of protocol/plumbing vs opentui `embedded_terminal` VT engine — least overlap, different layers. | Revisit if oc-tui adopts the opentui rendering pipeline (same trigger as the `line_number` revisit above). |
