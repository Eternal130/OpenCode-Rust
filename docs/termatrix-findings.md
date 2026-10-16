# Terminal-matrix findings — lane/termatrix (2026-10-16)

The terminal-environment differential ROADMAP §4.3 planned but never landed:
the pinned **v2.0.24 reference** and **our Rust TUI** run by the tui-diff rig
under **eight different terminal environments**, identical on both ends
(ENV *and* terminal query answers). The rig only ever ran one TERM profile
before — TERM-conditional behavior (SGR downgrade, mouse protocols, kitty
keyboard negotiation, clipboard OSC, cursor shape, theme-mode detection) was
an untested surface.

* Rig extension: `tools/parity/src/termatrix.rs` (+ wiring in
  `tools/parity/src/tui_diff.rs`, `--term-profile` in the runner). Rig code
  only — **no oc-tui product code was touched** (report-only lane).
* Environments: TERM/COLORTERM overrides **plus a PTY query responder** that
  answers the TUIs' capability queries (DA1/DA2/XTVERSION/kitty `CSI ? u`/
  OSC 11/OSC 10/OSC 4/DECRQM-2026) with canned terminal replies — both TUIs
  see byte-identical answers. One profile wraps both TUIs in a **real
  tmux 3.4 session**. One answers OSC 11 with a **white** background
  (theme-mode probe).
* Grid comparison is the rig's stock EXACT/STRUCTURAL machinery; protocol
  negotiation is compared by a new **protocol signature** channel (the
  multiset of protocol-class escape sequences each side emits — grids cannot
  see negotiation).
* Matrix: 8 profiles × 5 scripts (boot-home, type-edit-submit, themes-nord,
  mouse-hover-tabstrip, palette) × 2 runs × 80×24, live ref (no cache
  replay; the cache fingerprint does not cover environments and profiles
  force it off). Full run log: `docs/termatrix.log`. Artifacts:
  `/tmp/opencode/termatrix/<profile>/` (per-run grids, raw streams,
  `protocol-signature.txt`). Analyzer:
  `tools/parity/tui-diff/termatrix-analyze.py`.

## Headline verdict

**No TERM-conditional GRID divergence exists in this corpus.** Every
non-baseline environment reproduces the baseline's step-class map exactly;
the only step-class flips anywhere are the registered placeholder coin
(P-1, below). The SGR downgrade matrix and the OSC 11 theme-mode decision
are at parity under every environment tested. **All divergences found are
protocol-level** (invisible to the vt grid but visible to real terminals)
— registered below with severity for the fix queue.

## Environments

| profile | TERM | COLORTERM | query answers | wrap |
|---|---|---|---|---|
| `xterm256-silent` (baseline = old rig) | xterm-256color | — | none (silent PTY) | — |
| `xterm256` | xterm-256color | — | xterm(371): DA1/DA2/XTVERSION, OSC 11 dark, OSC 10/4, DECRQM 2026=supported | — |
| `xterm256-light` | xterm-256color | — | same but OSC 11 **white** (theme-mode probe) | — |
| `truecolor` | xterm-256color | truecolor | same as `xterm256` | — |
| `screen256` | screen-256color | — | screen(5.0): DA1 `?83;0c`, sync-2026 **unsupported** | — |
| `tmux` | screen-256color (inner) | — | tmux 3.4 answers itself (outer responder silent) | real `tmux new-session` |
| `kitty-yes` | xterm-kitty | truecolor | kitty(0.42.2) identity, **kitty query answered flags=511** | — |
| `kitty-no` (control) | xterm-256color | — | same as `xterm256` but `CSI ? u` **unanswered** | — |

## Grid evidence table (per profile × script, runs r0/r1)

Mismatch counts (steps not byte-identical). The counts are dominated by
pre-existing, profile-independent classes (below); no environment adds or
removes any class. `boot`, `dismiss`, `palette-open/down`, `submit`,
`hover-*` labels refer to step labels in the per-profile reports.

| profile | boot-home (3) | type-edit-submit (13) | themes-nord (8) | mouse-hover-tabstrip (17) | palette (6) |
|---|---|---|---|---|---|
| xterm256-silent | 3 / 0 | 9 / 9 | 7 / 7 | 15 / 15 | 6 / 2 |
| xterm256 | 0 / 3 | 9 / 9 | 7 / 7 | 12 / 15 | 6 / 6 |
| xterm256-light | 3 / 0 | 9 / 9 | 7 / 7 | 15 / 15 | 2 / 6 |
| truecolor | 0 / 3 | 6 / 9 | 7 / 2 | 15 / 15 | 6 / 2 |
| screen256 | 0 / 3 | 9 / 9 | 2 / 7 | 12 / 12 | 2 / 2 |
| tmux | 0 / 3 | 6 / 9 | 7 / 7 | 15 / 12 | 2 / 2 |
| kitty-yes | 3 / 3 | 9 / 9 | 2 / 7 | 15 / 15 | 6 / 6 |
| kitty-no | 3 / 3 | 6 / 6 | 7 / 2 | 12 / 15 | 2 / 6 |

Run-to-run wobble in a column is the placeholder coin (P-1) plus the
reply-stats duration row (K-3): every flip is one of those two rows —
verified per-step by `termatrix-analyze.py` (step-class deltas vs baseline
contain **only** P-1 steps; see `docs/termatrix.log` run lines).

## Registered pre-existing classes (profile-independent, reproduce at
baseline on **unmodified main** — checked via `/tmp/opencode/termatrix/maincheck`)

These predate this lane (REPORT.md's clean rows are a stale snapshot) and
are NOT TERM findings; registered so the matrix rows above are readable:

* **K-1 P-1 placeholder coin** — the home placeholder is picked at random
  per boot by BOTH stacks (ref `randomIndex`,
  `packages/tui/src/component/prompt/index.tsx:103-106` + `:364`; ours
  `crates/oc-tui/src/components/home.rs:43-55`). ~2/3 of runs mismatch on
  that single row (33/73–37/77 mismatch steps across profiles are
  placeholder-only — analyzer census).
* **K-2 palette command-set drift** — ref palette shows
  `Connect an integration` (an integration row) ours does not
  (first diff (9,13), `palette s2/s3` + all scripts' palette steps).
* **K-3 reply-stats real durations** — the settled reply row renders real
  elapsed times ('1'→'2' at (26,8)); inherently non-deterministic.
* **K-4 models-dialog filter row drift** — `models-open/filter/select`
  steps diverge at (15,7) text ('e'→'w'); content drift, identical under
  every environment.
* **K-5 hover-✕ color + dialog-bg cell** — hover s12 ✕ cell fg idx255 vs
  idx7, hover/type s5 bg idx0 vs idx232; registered style classes.

## TERM-conditional findings (the matrix's purpose) — severity-ranked

### TM-1 — kitty keyboard protocol negotiation gap (protocol-negotiation, env-conditional) — **severity HIGH**

Under `kitty-yes` (query answered with progressive-enhancement flags):

* ref: `CSI ? u` query → on answer: `CSI u` (reset) then **`CSI > 5u`
  (push disambiguate+alternate-keys)** — enters the kitty protocol
  (`/tmp/opencode/termatrix/kitty-yes/boot-home-80x24-r0/ref.raw.ansi`,
  sequences `\x1b[?u`, `\x1b[u`, `\x1b[>5u`). Under every other profile the
  ref queries and stays silent-legacy — advertisement-gated, exactly per
  protocol spec. Gated by `useKittyKeyboard: {}`
  (`/opt/opencode-ref/packages/tui/src/app.tsx:236-246`); the mini host
  enables/disables explicitly
  (`/opt/opencode-ref/packages/tui/src/mini/runtime.lifecycle.ts:235-238`,
  `buildKittyKeyboardFlags`); the push itself comes from the native raster.
* ours: **never queries, never pushes** (0 kitty sequences in any
  environment; setup is unconditional crossterm modes,
  `crates/oc-tui/src/app.rs:2161-2179`).
* Grid impact in-corpus: none (legacy input remains parseable with flags
  pushed; the rig drives legacy encodings). Real-terminal impact: no key
  disambiguation/alternate-key reporting for us on kitty/foot/ghostty/wezterm.
* Ref quirk registered while here: the ref **never pops** (`CSI < u`
  absent) — kitty flags stay pushed after exit; we cannot match this
  "accidentally" and should pop on teardown when we implement push.

### TM-2 — live color-scheme / mode-report subscriptions missing (protocol, universal) — **severity MEDIUM-HIGH**

* ref enables `?2027h` (color-scheme change notifications) and `?2031h`
  (mode-report notifications) after DECRQM-probing them
  (`[?2027$p/[?2027h`, `[?2031$p/[?2031h/[?2031l` in
  `@opentui/core-linux-x64@0.5.14/libopentui.so` rodata): a terminal that
  flips light/dark at runtime re-themes the ref live.
* ours: probes OSC 11 once at boot
  (`crates/oc-tui/src/theme.rs:489-540`), no subscription — a runtime
  terminal theme flip never reaches us until restart.

### TM-3 — cursor shape + cursor color never set (protocol, universal) — **severity MEDIUM**

* ref emits `CSI 1 SP q` (DECSCUSR blink-block) + `OSC 12 ; #eeeeee`
  (cursor color) at boot (`[1 q`, `]12;#` rodata; observed in every
  non-tmux profile's ref signature).
* ours: no DECSCUSR / OSC 12 anywhere in `crates/oc-tui/src` (grep
  verified) — on real terminals our caret keeps the terminal default
  shape/color over the TUI background.

### TM-4 — mouse mode set difference (protocol, universal) — **severity LOW-MEDIUM**

* ref enables exactly `?1000h ?1002h ?1003h ?1006h`.
* ours enables those **plus `?1015h`** (urxvt extended coords) and
  **`?1004h`** (focus reporting; the ref only DECRQMs focus, never sets
  it) — `crates/oc-tui/src/app.rs:2178` (crossterm `EnableMouseCapture` +
  `EnableFocusChange`). Extra modes are harmless-to-beneficial but are a
  wire-visible divergence; SGR-mouse event *encoding* is identical on both
  sides (hover/click sweep steps parse identically everywhere).

### TM-5 — window title never set (protocol, universal) — **severity LOW**

* ref sets `OSC 0 ; OpenCode` (`]0;` rodata, observed every non-tmux
  profile); ours never writes a title.

### TM-6 — synchronized-output (2026) wrapping unused (protocol, universal) — **severity LOW**

* ref wraps every frame burst in `?2026h`/`?2026l` (12–14 pairs per run)
  — **even when the terminal answered the DECRQM with "unsupported"**
  (`screen256` still shows 12 pairs): usage is unconditional outside tmux.
* ours never wraps. No grid impact (parsers ignore it); pure
  flicker/tearing polish on slow terminals. Matching the ref means
  emitting unconditionally like it does.

### TM-7 — per-frame cursor-show chatter (protocol, universal) — **severity LOW**

* ours emits `?25h` (show cursor) ~300×/run — every draw
  (`mode:?25h our=293..302` vs `ref=11..12`); ref toggles rarely and
  leaves the cursor hidden (`?25l` our=0 vs ref=13-15). Wire-volume (and
  potential visible caret flicker) difference only.

### TM-8 — capability-probe surface + answer-driven escalation (protocol, informational) — **severity INFO**

* ref probes DA1, XTVERSION, XTGETTCAP, DECRQM{1004,1016,2004,2026,2027,
  2031}, OSC 10/11/4, OSC 99 (kitty notify probe
  `]99;i=opentui-notifications:p=?`), OSC 1337 (iTerm2 capabilities), and
  its private OSC 66 — all in `libopentui.so` rodata + bundled JS
  (`chunk-bun-j2z63cdy.js:6661/6664` for OSC 99/1337).
* ours probes **only OSC 11**. When the terminal ANSWERS OSC 11, the ref
  escalates: re-queries OSC 11 (×2) and sweeps **17 OSC 4 palette-slot
  queries** (xterm256/screen256/kitty-no rows); silent terminals get one
  pass. Our probe traffic is constant. No behavioral divergence observed
  from escalation (theme resolves the same) — registered as the documented
  negotiation-profile difference.

### TM-9 — clipboard OSC 52 (evidence gap, not a divergence) — **severity INFO**

Both stacks implement OSC 52 writes (ref
`/opt/opencode-ref/packages/tui/src/clipboard.ts` via opentui
`writeText({destination:"all-available"})`; ours
`crates/oc-tui/src/clipboard.rs:1-40`, 100 KB cap) but **no corpus action
triggers a copy on either side** — the matrix cannot rule this surface
in or out. Needs a corpus script (e.g. mouse-selection + `ctrl+y`
copy-selection) before the fix queue can act.

### TM-10 — inside real tmux (protocol, env-conditional) — **severity INFO**

* Under the tmux wrap the OUTER stream is tmux's: both stacks' outer
  signatures converge (mouse toggles 3/8 vs 2/6, `?25h` 11 vs 9) and both
  degrade to 256-index — grid parity holds through tmux 3.4 for every
  script (table above).
* ref ADAPTS to tmux: its DA1/DECRQM/OSC-10/11/4/kitty/1337/66/12/cursor-
  shape/title traffic disappears entirely inside tmux (TMUX env detection
  in the raster); ours emits its identical unconditional setup (consumed
  by tmux). No user-visible consequence found.

## Clean bills (equally valuable evidence)

1. **SGR downgrade matrix parity** — both stacks flip emission modes
   identically per environment: 256-index under xterm-256color (silent or
   answered), screen-256color, and inside tmux; truecolor under
   COLORTERM=truecolor and TERM=xterm-kitty. Our
   `crates/oc-tui/src/render/color_mode.rs:69-85` decision table matches
   the raster's under all eight environments (`[sgr split]` rows in
   `docs/termatrix.log`'s analysis).
2. **Theme-mode negotiation parity** — with OSC 11 answered **white**,
   BOTH stacks flip to the light theme and stay byte-comparable
   (`xterm256-light` boot grids: `bg=idx255` both sides, same P-1-only
   divergence; mismatch counts equal to dark profiles).
3. **Grid parity through a real tmux session** — no new divergence class
   under the tmux wrap (smcup/rmcup, margins, escape-time 0).
4. **Query answers are harmless to our input path** — injecting terminal
   replies (DA1/DA2/XTVERSION/OSC 10/4/11/DECRQM/kitty) into both PTYs
   never produced a phantom keystroke or lost input on either side
   across 80 runs.
5. **Kitty-unadvertised control** — with answers present but `CSI ? u`
   unanswered (`kitty-no`), the ref stays legacy: exactly the baseline
   protocol set. The advertisement is the sole gate.

## Rig/environment notes (append-only log has the full trail)

* The ref TUI **crashes** (`OpenCode crashed — Unknown Error, TODO`,
  `FSWatcher` at `packages/tui/src/config/index.tsx:300`) when
  `fs.inotify.max_user_instances` is exhausted — stale ref-servers from
  older lanes held ~100/128 instances. Raised to 512 (`sysctl
  fs.inotify.max_user_instances`). Rig hygiene, not a product finding
  (any v2 TUI boot fails there, ours does not use fs.watch).
* A root-owned out-dir made five profiles lose their first report
  (runs fine, report write failed) — those profiles were re-run; the log
  marks the RERUN lines.
* Ref-cache is force-disabled under profiles (baseline captures must
  never replay under another environment).

## Fix-queue ordering (termatrix lane's recommendation)

1. TM-1 kitty keyboard: query → conditional push (flags 5 to match ref),
   pop on teardown (improve on the ref's missing pop).
2. TM-2 `?2027h`/`?2031h` subscriptions + runtime re-theme.
3. TM-3 DECSCUSR + OSC 12 cursor color.
4. TM-9 clipboard corpus script (evidence first), then any OSC 52 deltas.
5. TM-4 mouse mode set alignment (drop `?1015h`, decide `?1004h` policy).
6. TM-5 window title; TM-6 sync-2026; TM-7 cursor chatter (polish wave).
