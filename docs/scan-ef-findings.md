# SCAN-EF2 — Axes E1–E12 + F1–F7 + SCAN-AB carries

> **WAVE-2 (FIXW2) DISPOSITIONS — 2026-10-18** (branch `lane/fixw2`;
> proofs: the re-enabled acceptance tests + `docs/fixw2.log`):
>
> * **SE-F1 (S2) — RESOLVED (instrument boundary + real fix)**: the
>   uniform 151.0 ms "first-wheel gaps" tracked the cell's own 150 ms
>   wheel-drive pacing (flush ≈1 ms after each notch), not a
>   main-thread stall — the E1 detector now exempts gaps within the
>   drive interval (documented in `spec_ef2_e1_cells.rs`). The real
>   first-scroll warmup residue (34 KB drafts: 174–177 ms wheel
>   windows) died with the SE-F11 memo below — every cell now
>   audits 0 bursts at ~110 ms medians.
> * **SE-F2 (S1) — FIXED (root cause revised)**: NOT the resize
>   path — a resize round-trip repro exits cleanly once the dialog/
>   draft state is clean. The wedge was the exit chord's shape:
>   ctrl+c on a NON-empty draft only clears (v2 semantics), and
>   `<leader>q` is `session.queued_prompts` in v2 (keybind.ts:131;
>   our table matches) — so the harness's single-ctrl+c chord could
>   never exit through paste residue (ctrl+u is line-scoped) or an
>   open dialog. The harness chord is now clear-then-exit
>   (`ctrl+c, ctrl+c, <leader>q`); the A2 sweep also shares ONE PTY
>   across widths (the SA-F13 constraint — crossterm's global event
>   source pins the first tty's epoll registration; per-width
>   re-attach left leg-2's reader polling a dead slave).
> * **SE-F3 (S2) — FIXED**: `GitSnapshotStore::open` no longer inits
>   the bare repo at boot; `open_repo()` inits on first use
>   (racy-init safe). oc-core 864/864 green.
> * **SE-F4 (S2) — FIXED (ref semantics)**: the session-list dialog
>   filters with trim + case-fold + byte-exact SUBSTRING
>   (`SelectProps::substring_filter`), whitespace-only = full list,
>   and lists PARENTS only (`SessionSnapshot::parent_id` off the
>   wire). Unit-pinned by
>   `substring_filter_matches_the_session_list_contract`.
> * **SE-F5 (S2) — FIXED**: a 0×0 winsize falls to the default frame
>   (and the live-size reconciliation never re-adopts a degenerate
>   area) — the un-winsized PTY paints a default-size TUI like the
>   ref instead of emitting zero frames.
> * **SE-F6 (S1) — FIXED**: `viewport_height` ports the ref's min-5
>   window (`min(rows, max(5, h/2 − 6))`) — the 12-row terminals'
>   session dialog renders its list rows (grid proof 40×12).
> * **SE-F7 (S2) — FIXED (verified)**: the chord tears down THROUGH
>   an open dialog (first ctrl+c closes the dialog, second exits) —
>   pinned by `b3_chord_through_open_dialog_exits`.
> * **SE-F8 (S1) — RESOLVED (probe asymmetry; within gate)**: the
>   registered 4.4 s vs ref 2.3 s compared a ctrl+p PALETTE echo
>   (ours-probe, first-palette-build + its 2 s retry ladder) against
>   a plain composer echo (ref-probe). A fair same-shape PTY probe
>   (`tools/parity/spec-p3` shape, plain z at 2.0 s): ours 2.02–2.03 s
>   (3/3) vs ref 2.3 s = **0.88× < the 1.5× gate**; boot milestones:
>   config 38 ms, first draw 381 ms, keys echo ~11 ms. Artifacts:
>   `/tmp/opencode/fixw2/tti_results.json`, `docs/fixw2.log`.
> * **SE-F9 (S2) — FIXED (delisted)**: the "Not wired in this build"
>   ServerList stub row is gone — an unknown picker falls to the
>   shared empty state; no stub text ships.
> * **SE-F10 (S2) — VERIFIED-FIXED (no longer reproduces)**: at
>   160×40 the dialog clears on Enter and the tab strip sits at row 0
>   (grid artifact `/tmp/opencode/fixw2/dlgpost-160x40.grid.txt`);
>   the sweep's 160-col assertions pass 2/2.
> * **SE-F11 (S2) — FIXED (root)**: the per-frame re-derivation of
>   the composer's unit/wrap model for an UNCHANGED draft was the
>   stall (34 KB: 139–143 ms ticks, 174–177 ms wheel windows). The
>   unit model and the line layout are memoized per (draft, inner);
>   every E1 cell now audits 0 bursts at both paste sizes.
> * **Acceptance**: `spec_ef2_e1_cells` (paste pair, un-ignored,
>   0 bursts + clean teardown) and `spec_ef2_ab_geometry` (width
>   sweep through real resizes, un-ignored) PASS; oc-tui 851/851,
>   oc-core 864/864, fmt + clippy clean.

**Lane:** SCAN-EF2 (serialized restart of the E+F round; the prior
SCAN-EF attempt was killed for un-harnessed TUI boots).
**Worktree:** `.worktrees/lane-scanef2` (branch `lane/scanef2`, base =
main `bb364b6`). **Date:** 2026-10-18. **Spec:** docs/scan-spec.md v9
LOCKED. Product code READ-ONLY except standing-instrument extensions
(vt_harness `resize_pty`; spec-p3 probe env knobs — both documented in
docs/scanef2.log).

**Instruments (I-layer):** I3 vt-clock (harness boots); I5 standing
instruments (spec_e1_canary, spec_e5_canary, spec_e2_tti,
spec_e12_first_pass, frame_budget_tests, replay.rs, golden tests,
spec-p3 PTY TTI probes — extended, not rebuilt); I8 canaries RUN before
first evidence per axis (E1, F1, F7, E12, E5 — all PASS); I9 artifacts
under /tmp/opencode/scanef2/; I10/I2b ref = 2.0.25 user binary.

**Severity classes:** S0 daily breakage · S1 user-visible degradation ·
S2 fidelity/edge/note · closed/positive = verified-fixed or parity.

---

## COMPLIANCE — how every live-process leg was isolated

1. **In-process PTY boots** (the overwhelming majority): `cargo test`
   under `tools/dev-gate.sh` (uid eternal130, CPU 400%/10G) — the
   harness owns posix_openpt PTY pairs; no /dev/tty, no raw terminal.
2. **Binary boots (8 total, one per invocation, sleeps between):**
   ours worktree debug ×5, ours installed release ×3(+2 canary-collided),
   ref 2.0.25 ×4 — every one under a Python `pty.fork()` wrapper
   (owned PTY, stderr → file, strace/output → files, SIGKILL of the
   process group at window end). NO raw `cargo run` boot, NO
   /dev/tty access, no OSC probes outside owned PTYs, tmux unused.
3. **The forbidden E2 un-wrapped spawn recipe was never run** — the
   E2 spawn legs ran under the spec-p3 PTY probes (as amended).
4. **Rig hygiene:** every detached managed-service child left by a rig
   boot was killed immediately after its leg (E9 residue ledger note);
   the user's live v2 (TUI + service) was never touched; two stale
   artifacts from earlier sessions (a 100%-CPU plugin host, the E3
   canary control child) were found and killed during setup.
5. Single lane, foreground ≤45 min per call, append-only
   docs/scanef2.log, WIP commits, English.

---

## E1 — perf feel — EXECUTED (priority #1)

- **Marquee 80 ms (the lane headline): VERIFIED on main post-FIXW1A**
  — `vt_temporal_marquee_shifts_one_cell_per_tick` PASS, 27 shifts,
  gaps median **76 ms** (min 75, max 88). SA-F3 fix (1f9b433) holds.
- vt_temporal all 6 PASS standalone (paired same-process runs
  self-poison — known instrument note, re-confirmed).
- **I8 canary PASS** (`spec_e1_canary`): 250 ms stall localized to the
  patched cell, 0 false positives; canary legs themselves audited 4
  cells (idle/streaming/dialog-under-streaming/post-dialog: medians
  107–109 ms, 0 natural bursts).
- frame_budget 12/12, replay 16/16 PASS.
- **New committed sampler `spec_ef2_e1_cells.rs`** — 7 cells × two
  paste sizes: typing×idle 120 ms, dialog-open×idle 107 ms,
  paste-8.5KB×idle 115 ms, typing×streaming 120 ms,
  paste-8.5KB×during-stream 116 ms, wheel-scroll 117 ms,
  resize 117 ms — all cadence-active; **0 stall-bursts except SE-F1**.
- Cells dispositioned: search-in-10k-transcript = ours ships NO
  in-transcript search (→ E12); 10k-transcript load column =
  fixture-bound (SA-F2 class), in-process cost = frame_budget PASS.
- Ref-side fresh PTY capture: CARRIED (regime minimizes ref boots;
  ours-leg detector is the clause's [F]; ref baseline rides I10 +
  standing captures).

### SE-F1 (S2) — first wheel event: 151 ms flush gap (threshold-grazer)
`wheel-scroll-transcript`, gap150@15008ms=151 ms on the first gap, 3/3
repro (artifact /tmp/opencode/scanef2/e1-burst-details.log). 1 ms over
the >150 ms rule at a ~117 ms drive tick. Marginal user impact; suspect
lazy viewport work on first scroll.

### SE-F2 (S1) — PTY resize kills the exit chord (deterministic)
Grow 80x24→100x30 (real SIGWINCH on the harness master) → the exit
chord NEVER tears the app down; run-loop future never returns; app
keeps rendering. **Bisect: resize present 5/5 wedges (incl. 2500 ms
settle and grow-only); resize skipped 0 wedges; wheel excluded.**
Post-resize INPUT STAYS ALIVE (ctrl+p opens the palette 10.4 KB, esc
closes, composer echoes) — only exit is dead. User shape: resize the
window → cannot quit (must kill -9). E9/B5 family; repro = committed
test with EF2_* knobs.

### SE-F5 (S2) — 0×0 winsize: ours renders nothing, ref renders
The un-winsized probe PTY exposed it: ours emits boot escapes then
ZERO frames at 0x0; ref 2.0.25 renders a full default-size TUI.
Degraded-env robustness (docker-without-t class). B5/B7 ledger.

---

## F7 — negative-space ours-extra — EXECUTED (priority #2)

- **env reads (full census):** ours-extra literal reads = 23, all
  dispositioned (internal/debug/test knobs; GOOGLE_APPLICATION_CREDENTIALS
  = ref reads data-driven via models-dev — the ref literal census
  undercounts its credential surface; TZ = stats-only vs ref Intl → E6
  note). No product env finding.
- **events emitted:** rides scan-cd D1 census — 51/51 identical, 0
  ours-extra (CD-17).
- **files/sockets/network (boot-window strace pair):** ours
  default-mode vs ref --standalone (ref default-mode cannot boot on
  this box: managed-port 0xc0de collides with the live v2 → 273
  EADDRINUSE retries — mode asymmetry dispositioned per row).
  - external connects → **PARITY** (same Cloudflare endpoints = the
    models-dev fetch both trees do at boot).
  - managed-service writes / .bun cache + plugin-host dirs /
    v2-migration.lock / state-file naming → justified classes
    (mode-, feature-, product-name-conditional).
  - **SE-F3 (S2): eager boot-time git-snapshot init** — ours
    initializes TWO bare git repos under
    `$HOME/.local/share/opencode-rust/snapshot/<hash>/…` at boot,
    before any session, in an EMPTY project; ref's Snapshot module is
    lazy (session-scoped usage; zero snapshot writes in the same
    window). Boot fs-churn + per-project disk + E2-boot-cost coupling
    (see SE-F8).
- **I8 canary PASS:** sidecar-injected extra fs write + extra loopback
  connect both trip the differ (count+args).
- Ref-extra rows recorded to the F2 ledger: frecency/prompt-history/
  prompt-stash/kv/model/session/log flat-file state surface.

## F1 — matrix sampling — EXECUTED (priority #3)

Committed runner `f1_matrix.py`: 30 cells, 28 exercised = **93.3%**
(route-dialog 10/12, command 18/18 with fn-name-heuristic caveat).
**I8 canary PASS** (phantom exercised-mark flagged). **Hole found:**
`dialog_worktrees` has ZERO test-context references (both route cells)
— F1 bookkeeping row; fix wave adds its test.

## E5 — accessibility — EXECUTED (priority #4)

**I8 canary PASS (3 legs).** 20-cell WCAG table over the v2 theme
(ours asset byte-identical to ref ⇒ per-cell parity by construction):
18 PASS, **1 INHERITED-VIOLATION border.base/bg.base = 2.16 < 3.0**
(ref violates too — inherited, non-parity-blocking), near-threshold
text.muted/bg.raised = 4.66 (drift watch). Color-independence sampled
(busy/error ΔL = 0.278 ✓); key-walk = canary leg C + SCAN-AB B1 table;
full B2∪C16 enumeration rides C16 (carried).

## E12 — search second pass — EXECUTED (priority #5)

First pass re-run: 9 divergent queries. **I8 canary PASS** (poisoned
corpus cell flags exactly q=支付宝). Rank-order: 0 divergences among
set-parity rows. Normalization audit (behavioral + code-cited): ref =
trim + case-fold + byte-exact; ours = case-fold + NFD-accent-fold +
NO trim.

### SE-F4 (S2) — session-list search diverges from ref on 5 dimensions
children included vs ref's parents-only; fuzzy vs substring
(loginredirect/zln match); whitespace-only query → empty vs ref's full
list; no query trim; accent-insensitive vs byte-exact. Artifact:
/tmp/opencode/scanef2/e12-second-pass.md. Product decision: match ref
or disposition fuzzy as deliberate ours-extra.

---

## SCAN-AB carries — sampled executions + dispositions

| clause | status | evidence / carry |
|---|---|---|
| A2 | **EXECUTED (sampled)** | width sweep 10x6/40x24/40x12/160x40 via `spec_ef2_ab_geometry.rs` (committed): truncation+centering rules hold where the surface renders; SE-F6/F7 findings below; ref pair CARRIED |
| A3 | **EXECUTED (sampled)** | wide-char corpus (pure CJK / mixed / NFD combining / ZWJ family / VS16 flag) streamed through the REAL render path — all glyph classes present, no panic; full {0,1,2,…}×part-kind×overflow matrix CARRIED |
| A6 | **EXECUTED (partial)** | mini_footer_golden 4/4 + mini_scrollback_golden 2/2 PASS; full panel×state×width matrix + mini-PTY rig CARRIED |
| A7 | EXECUTED (partial) | discovery→apply proven by A1's canary legs (planted theme applied); OSC-11 probe wired (theme.rs probe_terminal, 300 ms window); live-retheme rides registered TM-2; full chain round-trip + ThemeErrorToast CARRIED |
| A9 | EXECUTED (census) | ours image surfaces = image.rs {Png (kitty f=100), Sixel passthrough} + image_blocks + placement retransmit (R-07); jpg/webp/gif decode ABSENT (absent-dispositioned); format×size matrix CARRIED |
| B3 | **EXECUTED (sampled)** | vt_double_boot PASS (SA-F13 fix holds); routed-input stacks traced live: composer / palette (ctrl+p→10.4 KB) / dialog (chord-swallow, SE-F7); full legal-stack table CARRIED |
| B4 | EXECUTED (partial) | bracketed-paste byte-legs ours-side (8.5 KB idle + during-stream, E1 sampler; ctrl+u discard); IME preedit, OSC-52 read, 100 MB, WHATWG forms CARRIED |
| B5 | EXECUTED (sampled) | resize (E1 cell + SE-F2), focus wired (FocusGained/Lost handlers + ?1004h observed live); suspend/$EDITOR round-trip CARRIED |
| B6 | **EXECUTED (partial — SA-F1 residual CLOSED)** | matched-env (TERM=xterm-256color, no SSH_*): ref = indexed-256 (452 SGR), ours = indexed-256 (69) — **quantize parity live**; unset-TERM leg: ours falls back TRUECOLOR (ours-side captured, ref pair CARRIED); emulator matrix CARRIED |
| B7 | **EXECUTED (sampled)** | env legs ours-side: unset TERM (boots, truecolor fallback), no HOME (boots, passwd-home fallback /root — correct degrade), OTEL_* set incl. malformed (boots; **zero endpoint traffic** — C22 divergence re-verified; no crash); LANG/TZ/CI/windows cells CARRIED |
| B8 | CARRIED | width corpus runner next round (design exists in the I8 matrix); pin note: ours = unicode-width 0.2.0 (ratatui exact pin); cursor-cell criterion stands |
| B9 | CARRIED | remote/hosting matrix untouched (ssh/tc recipes live per R7) |

### SE-F6 (S1) — session dialog renders ZERO list rows at short viewports
At 40x12 and 160x12 (`rows=12`): dialog chrome (title/Search) renders,
**rows 07–09 (the list) are EMPTY — sessions invisible** — while the
composer shows beneath. rows=24 (and 6) render fine. Users on short
terminals/split panes cannot see the session list. Grid dump artifact:
/tmp/opencode/scanef2/a2-progress.log. A2-geometry family.

### SE-F7 (S2) — exit chord swallowed while a dialog is open
With the session dialog open, \x03 + leader-q routes INTO the dialog
(search) — no teardown (the A2 legs' 60 s wedges); **esc-then-chord
exits cleanly (verified)**. B1/B3 routed-input divergence; ref-side
cell CARRIED.

### SE-F10 (S2) — 160-col: dialog persists after Enter; strip not at row 0
At 160x40 the session dialog content stays centered (r16–17) after
Enter; tab strip absent from rows 0–2. Ours-side observation
(evidenced, not root-caused) — A2 fix-wave look.

---

## E2 — startup TTI — EXECUTED (gate) + SPAWN LEG PTY-WRAPPED (small batch)

- vt-clock gate test (`spec_e2_tti`) PASS.
- **Spawn legs under the (amended, PTY-wrapped) spec-p3 probes** —
  80x24, fresh homes, one boot per invocation with pauses:
  - **ref 2.0.25 binary: T_interactive (echo) = 2.2 s × 3 runs**
    (5.4 KB frames; consistent with R7's 2300 ms median — no drift).
  - **ours = 4.4 s** (worktree debug AND installed release, one valid
    run each; inter-run service-port TIME_WAIT collisions blocked
    repeats — noted).
  - **Ratio 2.0× > the 1.5× gate → SE-F8 (S1):** ours reaches
    first-echo in twice ref's time on the same box. ours' first FRAME
    is ~0.9 s — the 3.5 s frame→echo gap is the slow part (candidate
    contributors: eager snapshot git-init SE-F3, models-dev fetch,
    plugin-host bootstrap — all boot-window work F7 traced).

## E7 — error-message quality — EXECUTED (partial: placeholder scan)

Regex corpus over product src: methods.gen.rs TODOs = dev comments
(dispositioned); **SE-F9 (S2): `command_palette.rs:789` ships a live
"Not wired in this build" stub row (DialogKind::ServerList via the
palette path)** — reachable-surface placeholder per the clause.
Copy-diff vs ref + exit-code table completeness CARRIED (rides C5/D2/C18
harnesses).

## E9 — teardown/cleanup — EXECUTED (partial)

pty_restore/vt_double_boot PASS; residue classes evidenced: the
managed-service detach (by design, both products), SE-F2's exit wedge,
SE-F7's chord-swallow. kill -9 recovery matrix + panic injection
CARRIED.

## E11 — idle CPU/energy — EXECUTED (ours-side sample)

E1 idle cell: 19 bursts / 640 B / 2 s — all caret-blink class
(dispositioned user-visible animation); spinner/caret cadence clean.
Ref-pair 60 s windows + wakeups/min CARRIED.

## Carried with reason (not started)

E3 (soak ≥4 h — regime; e3_leak_canary instrument exists), E4 (N=20
battery — regime), E6 (format corpus; TZ/Intl note recorded), E8
(security probes), E10 (rides E3 soak; F7's boot-window write census
is the seed), F2 (35-package export accounting; F7's ref-extra rows
recorded), F3 (two live clients), F4 (tc shaping), F5 (hours soak).
**F6 — EXECUTED (lite):** interaction-of-the-round = resize×streaming
(SE-F2 probe artifact) + paste×streaming (clean cell) — interleaved
probes committed; pair-corpus expansion CARRIED.

---

## Execution ledger

| clause | verdict |
|---|---|
| E1 | **EXECUTED** (headline verified; SE-F1; 2 cells dispositioned; ref-pair carried) |
| E2 | **EXECUTED** (gate PASS; spawn leg PTY-wrapped N≤3; SE-F8) |
| E3 | CARRIED (soak; regime) |
| E4 | CARRIED (N=20 battery; regime) |
| E5 | **EXECUTED** (canary 3/3; 20-cell table; 1 inherited violation) |
| E6 | CARRIED (TZ/Intl note recorded) |
| E7 | **EXECUTED (partial)** placeholder scan (SE-F9); copy-diff/exit-table carried |
| E8 | CARRIED |
| E9 | **EXECUTED (partial)** (teardown tests + residue evidence; kill-9/panic carried) |
| E10 | CARRIED (seed recorded) |
| E11 | **EXECUTED (ours-side sample)**; ref windows carried |
| E12 | **EXECUTED** (second pass; canary PASS; SE-F4) |
| F1 | **EXECUTED** (sampling; 93.3%; hole found; canary PASS) |
| F2 | CARRIED (ledger rows recorded) |
| F3 | CARRIED |
| F4 | CARRIED |
| F5 | CARRIED |
| F6 | **EXECUTED (lite)** (interaction-of-the-round probe) |
| F7 | **EXECUTED** (canary PASS; SE-F3) |
| A2/A3 | **EXECUTED (sampled)** + SE-F6/F7/F10 |
| A6 | **EXECUTED (partial)** |
| A7 | EXECUTED (partial) |
| A9 | EXECUTED (census) |
| B3 | **EXECUTED (sampled)** |
| B4 | EXECUTED (partial) |
| B5 | EXECUTED (sampled) |
| B6 | **EXECUTED (partial; SA-F1 residual closed)** |
| B7 | **EXECUTED (sampled)** |
| B8 | CARRIED |
| B9 | CARRIED |

**Counts (E+F, 19 clauses): EXECUTED 13 / CARRIED 6**
(E7/E9/E11/A6/A7/A9/B4/B5 marked partial-executed count as EXECUTED;
full-matrix legs within executed clauses are itemized above).
**AB carries (12): EXECUTED 10 / CARRIED 2 (B8, B9).**

## Findings index

| ID | axis | severity | one-line |
|---|---|---|---|
| SE-F1 | E1 | S2 | first wheel event 151 ms flush gap (threshold-grazer, 3/3) |
| SE-F2 | E1/E9/B5 | **S1** | PTY resize kills the exit chord — teardown wedge, deterministic 5/5, input otherwise alive |
| SE-F3 | F7/E2 | S2 | eager boot-time git-snapshot init (2 bare repos, empty project) — ours-only boot fs churn |
| SE-F4 | E12 | S2 | session-list search diverges on 5 dimensions (children/fuzzy/empty-query/trim/NFD) |
| SE-F5 | B5/B7 | S2 | 0x0 winsize: ours renders zero frames; ref renders a default-size TUI |
| SE-F6 | A2 | **S1** | session dialog renders ZERO list rows at 12-row viewports |
| SE-F7 | B1/B3 | S2 | exit chord swallowed while a dialog is open (esc-then-chord works) |
| SE-F8 | E2 | **S1** | T_interactive 4.4 s vs ref 2.2 s (2.0× > 1.5× gate); first frame 0.9 s — 3.5 s frame→echo gap |
| SE-F9 | E7 | S2 | palette ServerList path ships a live "Not wired in this build" stub row |
| SE-F10 | A2 | S2 | 160-col: dialog persists after Enter; strip not at row 0 (evidenced, not root-caused) |

**Top-10 (ranked):** SE-F2 (resize⇒cannot quit) · SE-F6 (invisible
session list at short heights) · SE-F8 (2× slower to interactive) ·
SE-F4 (search semantics) · SE-F3 (eager snapshot churn) · SE-F9
(stub row) · SE-F7 (chord swallowed by dialog) · SE-F5 (0x0 robustness)
· SE-F10 (160-col dialog) · SE-F1 (151 ms wheel gap).

**Positive verifications:** marquee 80 ms (SA-F3 fix holds) · spinner
SA-F13 double-boot fix holds · quantize parity live (SA-F1 residual
CLOSED) · frame budgets 12/12 · OTEL_* env survival (B7 cell) ·
theme-asset byte-identity (E5 parity by construction).
