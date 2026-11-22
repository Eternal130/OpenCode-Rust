# recorded-local — local real-session corpus (git-ignored)

> **Daily-recording enablement (three lines)**
> 1. `export TUI_RECORD_DIR=~/.local/share/opencode/tui-recordings` (set only when you want to record; unset = fully bypassed, zero overhead)
> 2. Work normally with opencode-rust (TUI) — the session event stream / key sequences / fetches / size history are automatically written to `<dir>/rec-*.raw.json` (redacted by default)
> 3. `python3 tools/diff/tui_record_compile.py <dir>` compiles them into this directory; afterwards running the channel (`tools/diff/tui_fullscreen.py`) automatically includes them in replay
>
> The iron rule stands: recorded content is real user data, **never committed, never uploaded** (items 1-4 below).

**Privacy iron rule (user ruling 2026-11-15, top priority)**:

1. This directory holds differential scenarios compiled from real TUI
   sessions (`tui-record-compile` compiles the `.raw.json` raw files
   recorded under `TUI_RECORD_DIR`). **The real corpus runs locally
   only; the data is never uploaded.**
2. This directory is git-ignored as a whole (the
   `tools/diff/corpus/tui/recorded-local/` entry in the root
   `.gitignore`); no commit may contain recorded content — no
   exceptions for raw files (`.raw.json`) or compiled artifacts.
   Defenses: `tools/guards/recorded_local.py` (`git ls-files`
   assertion) + the startup check of the orchestrator
   `tools/diff/tui_fullscreen.py`.
3. Reports may carry **aggregate counts only** (case counts, verdict
   distributions) and must never quote recorded content verbatim.
4. Message content is redacted by default (recorder + compiler double
   layer, shape-preserving placeholders: length/line
   breaks/unicode class preserved); `TUI_RECORD_REDACT=0` /
   `--no-redact` is for local debugging only — the artifacts still
   must not leave this machine.

Orchestrator behavior: when this directory exists it is automatically
included in channel 8 (case id prefix `rec-`, the report carries a
separate recorded-local section with aggregate counts only); an absent
directory = zero impact.
**Gate semantics (controller ruling 2026-11-15)**: unadjudicated
divergences on `rec-*` = advisory — they do not participate in the
gate exit (daily recordings carry standing unadjudicated items; the
gate is green only when the committed corpus is green); an infra
failure on `rec-*` (stub_gap / missing grid, etc.) still = exit 1
(that means the replay machinery is broken, regardless of
adjudication).
