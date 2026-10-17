# SPEC-R5 — Round-5: P2 quality-dimension track (E-axis exhaustion + quality-machinery sweep)

**Lane:** SPEC-R5 (worktree `.worktrees/lane-spec5`, branch `lane/spec5`). **Date:** 2026-10-17.
**Inputs:** `docs/scan-spec.md` v4 (593 lines), `docs/spec-round4.md` §P2-directives, ref tree
`/opt/opencode-ref/packages/` (re-consulted this round; anchors cited inline), ours `crates/oc-*`.
**Product-adjacent edit:** NONE — this is a FOCUS round. The v5 diff is PROPOSED verbatim in §3;
application deferred to R6 integration per I11 protocol (thresholds below involve judgment calls
that R6 should ratify against measured baselines, so "unambiguous" does not hold).

## §1 Per-clause executability table (the P2 exhaustion pass)

Test applied to every clause: **can an agent run it and produce artifact X with number Y?**
Columns: M = method concrete (tool/metric/threshold) · F = fidelity real (where the metric
comes from) · G = granularity falsifiable. Verdict: **OK** = executable as written ·
**RW** = needs rewrite (gap ID in §2).

| Clause | M | F | G | Verdict | Failure mode as-written |
|---|---|---|---|---|---|
| E1 perf-feel | ✗ | ✗ | ✗ | **RW → R5-Q1** | "per interaction x load" unenumerated; "2-frame deficit" undefined — a TUI has no fixed fps; frames are drive-source ticks. No capture harness named for ref side. |
| E2 startup TTI | half | ✓ | ✗ | **RW → R5-Q2** | T0 / "first-interactive" markers undefined; wall-clock parity is hardware-dependent — assert must be gate BEHAVIOR under I3 vt-clock + wall-clock recorded as baseline, not as pass/fail. |
| E3 memory soak | ✗ | half | ✗ | **RW → R5-Q3** | "slope assert" has NO threshold value; "span count ≥4h" is garbled (spans? runs?); sampling method unnamed (/proc? ps? cadence?). |
| E4 huge-data | ✗ | half | ✗ | **RW → R5-Q4** | "first-paint" marker undefined; no N-runs for "p95"; no threshold for the time; corpus generator unnamed. |
| E5 accessibility | ✗ | ✓ | ✗ | **RW → R5-Q5** | Contrast ratio threshold absent (no WCAG number); **`auditA` NOT FOUND in ref** (verified: zero hits under `tui/src`) — target list unanchored; "color-independence" has no method. |
| E6 i18n/locale | ✓ | ✓ | half | **RW (minor) → R5-Q6** | Overlaps B7 env cells without a stated boundary; format corpus (timestamps, relative times) unnamed. |
| E7 error quality | half | ✓ | ✗ | **RW → R5-Q7** | "comprehensibility/actionable check" is not agent-executable; error-path enumeration source unstated. |
| E8 security | ✓ | ✓ | ✓ | **OK** | Probe set concrete (routes, entropy, perms, traversal, redaction, SSRF). Minor note: entropy = sampled min-entropy estimate, N≥1000 codes. |
| E9 teardown | ✓ | ✓ | ✓ | **OK** | Exit paths enumerable; kill -9/panic injection named; audit target set concrete. |
| E10 storage/IO | ✗ | ✗ | ✗ | **RW → R5-Q8** | P1-residual NOMINAL #1: metric/cadence/artifact all deferred. Directive 2 requires closure. |
| E11 idle CPU | ✗ | ✗ | ✗ | **RW → R5-Q9** | P1-residual NOMINAL #2: "wakeups/min at rest" has no sampling method, no threshold. |
| E12 search/filter | half | ✓ | ✓ | **RW (minor) → R5-Q10** | Corpus not committed; "fuzzy" parity undefined (result-set vs rank-order). Directive 3 requires first-evidence readiness. |
| A4 temporal | ✓ | ✓ | ✓ | OK | Drive-source audit grounds it; add pinned cadences to I3 text (spinner 80ms — see §3 note). |
| B4 paste forms | ✓ | ✓ | ✓ | OK | Forms × size incl. 100MB; latency dimension owned by E1 (named cell, R5-Q1). |
| I3 virtual clock | ✓ | ✓ | ✓ | OK | Cadence list could append "spinner 80ms, cursor-blink emission" — note only, not a gap. |
| C3 attention/bell | ✓ | ✓ | ✓ | OK | Emission set + I3 timing covers bell behavior incl. cadence gating. |
| C11 update cadence | ✓ | ✓ | ✓ | OK | No "channel" concept exists in ref (verified `dialog-update.tsx`) — nothing missed. |
| F5 soak collector | ✓ | ✓ | ✓ | OK | Collects E1–E3, E10, E11 — consistent with rewritten E-clauses (shared harness stated in R5-Q8/Q9). |
| F7 negative-space | ✓ | ✓ | ✓ | OK + amendment (R5-Q12) | Needs pre-registered dispositions for ref-side emissions ours lacks (stats POST n/a; openTelemetry config key; event-logger writes). |

**Count:** 12 E-clauses → 1 OK-as-is pair (E8, E9), 10 rewrites (8 full, 2 minor).
Quality-bearing clauses elsewhere: 7 checked, all OK (1 amendment).

## §2 New-gap register

| ID | Clause | Gap (one line) | Cure |
|---|---|---|---|
| R5-Q1 | E1 | interaction×load matrix unenumerated; "frame" undefined (no fps in a TUI); ref-side capture harness unnamed | §3 rewrite: drive-source tick model + pinned cadences + named interaction cells (incl. paste-during-stream) |
| R5-Q2 | E2 | T0/T_interactive undefined; wall-clock threshold hardware-coupled | §3 rewrite: gate-behavior assert under I3 + baseline artifact |
| R5-Q3 | E3 | no threshold value; "span count" garbled; sampling method unnamed | §3 rewrite: VmRSS+FD @60s, ≥2×4h, threshold = ref baseline ×2 (I10), leak-canary gated |
| R5-Q4 | E4 | "first-paint" marker undefined; no N, no threshold | §3 rewrite: marker = first frame containing target content; p95 over N=20; ≤ ref ×1.5 on same corpus |
| R5-Q5 | E5 | WCAG number absent; auditA unanchored (ref has NO auditA); color-independence methodless | §3 rewrite: 4.5:1/3:1 thresholds, target list = B2 hit-rects ∪ C16 dialogs, grayscale+luminance method |
| R5-Q6 | E6 | B7 boundary unstated; format corpus unnamed | §3 minor rewrite: scope statement + toLocaleString corpus |
| R5-Q7 | E7 | "comprehensibility check" unexecutable | §3 rewrite: copy-diff parity + placeholder scan + exit-code table, subjective term dropped |
| R5-Q8 | E10 | NOMINAL — metric/cadence/artifact deferred | §3 rewrite: closes nominal #1 (named metrics @5min on F5 soak, slope artifact) |
| R5-Q9 | E11 | NOMINAL — sampling method + threshold absent | §3 rewrite: closes nominal #2 (schedstat/pidstat @60s windows, byte-rate-at-rest=0) |
| R5-Q10 | E12 | corpus uncommitted; fuzzy parity ambiguous | §3 minor rewrite: committed corpus JSON + rank-order parity |
| R5-Q11 | devtools | ref `tui/src/devtools` DevTools.register observability groups/entries = quality machinery with only generic route ownership | F2 standing accounting entry (export-level); no parity clause — ours absent-justified (dev-only debug counters) |
| R5-Q12 | F7 | ref-side emissions ours lacks will false-positive the ours-minus-ref diff | pre-registered dispositions: openTelemetry = config leaf (C4); event-logger writes = F7 files/events scope; stats server = U8 OOC |

## §3 Proposed v5 diff (VERBATIM clause text for R6 to carry)

> Replacement text for each clause below, in full. Provenance tags appended. All other v4
> text unchanged. Appendix gains 12 rows (R5-Q1…Q12 → destinations E1,E2,E3,E4,E5,E6,E7,E10,
> E11,E12,F2,F7).

**E1** (replaces v4 E1):
> E1 Perf FEEL: [G] interaction × load matrix, cells = {typing, scroll, dialog open, resize,
> paste-large, paste-during-stream, search-in-10k-transcript} × {idle, streaming-active,
> 10k-msg transcript open}; frame model = **drive-source ticks** (a TUI has no fixed fps —
> "frame" = one render flush driven by a cadence source; pinned cadences: spinner 80ms
> `spinner.tsx`, toast animation, reconnect backoff 1s/5s — same list as I3). [F] ours:
> `frame_budget_tests.rs` + `replay.rs` (I5); ref: timestamped PTY capture (write-stamp
> deltas) under I10 pin; vt-clock per I3 for all cadence sources. [M] per cell: inter-flush
> interval histogram (ms) + stall-burst detector = ≥2 consecutive missed drive ticks OR any
> gap >150ms while a cadence source is active; **any stall-burst = finding**; artifact =
> per-cell histogram JSON + burst list. Canary per I8: injected 250ms stall must flag exactly
> the patched cell. {R1: S2-1; R2: S2-1; R5: Q1}

**E2** (replaces v4 E2):
> E2 Startup TTI: [G] boot → first-interactive per surface {tui, mini, webui, headless run}
> × config variant {plugins on, plugins off} × data size {fresh home, 100 sessions, 10k-msg
> session}; **T0 = process spawn (first exec), T_interactive = first frame where the
> composer accepts+echoes a key** (marker = observed echo in capture). [F] I3 vt-clock for
> gate behavior + wall-clock N=10 runs recorded as baseline artifact (median, p95 — same
> host for ours and ref, I10 pin). [M] TWO asserts: (a) gate BEHAVIOR — stretch boot phases
> under vt-clock, StartupLoading appears exactly when 500ms elapses and escalates at 3s
> (thresholds pinned from ref `startup-loading.tsx`: 500ms show, 3000ms gate); (b) baseline —
> ours T_interactive median ≤ ref ×1.5. {R1: S2-2; R2: S2-1; R5: Q2}

**E3** (replaces v4 E3):
> E3 Memory growth (soak): [G] per session shape {idle session open, streaming session
> completed, 10k-msg transcript open, plugin-active} × ≥2 runs × ≥4h (or 1×24h); metrics =
> VmRSS + RssAnon + FD count sampled from /proc/PID every 60s (real clock — RSS is not
> virtualizable). [F] sampling harness = the F5 soak; ref baseline measured identically
> under I10 pin. [M] slope (MB/h, least-squares over full run) reported per run; threshold:
> ours slope ≤ **max(2 × ref slope, ref slope + 20 MB/h)**; artifact = CSV + slope table.
> Leak-canary gated per I8 before first evidence counts (see §5). {R1: S2-3; R2: S2-1; R5: Q3}

**E4** (replaces v4 E4):
> E4 Huge-data: [G] per family {transcript render, session-list, search, diff view} ×
> magnitude {1k, 10k, 100k msgs}; corpus = committed generator script (synthetic, I9-labeled;
> generation is part of the artifact). [F] I5 replay harness, same corpus for ours and ref
> (I10). [M] marker = **first frame containing the target content after
> load/navigate/scroll-to-message**; p95 over N=20 runs each side; threshold: ours p95 ≤
> ref p95 ×1.5 on the same host; content correctness is A3's (E4 owns time only — boundary
> per §4). Artifact = per-cell p95 table. {R1: S2-4; R2: S2-1; R5: Q4}

**E5** (replaces v4 E5):
> E5 Accessibility: [G] (a) contrast per theme × state × element — computed WCAG ratio from
> theme-token fg/bg pairs (post-quantizer, A7 chain); **threshold 4.5:1 text, 3:1 UI/large**;
> cells where REF itself violates are inherited-findings (listed, dispositioned, not parity-
> blocking) — ours must be ≥ ref ratio per cell. (b) keyboard-only path for every pointer
> target — **target enumeration = B2 hit-rect inventory ∪ C16 dialog enumeration** (v4's
> "~120 auditA targets" is DELETED: `auditA` does not exist in ref, verified zero hits);
> method = scripted key walk reaching each target with visible focus. (c) color-independence
> of busy/error/disabled states: under grayscale transform + luminance-equality check
> (deuteranopia-class pairs share luminance), state pairs must remain distinguishable by
> non-hue channel or glyph/shape. [F] computed from token values (I4) [M] ratio assert +
> walk artifact + independence table; canary per I8 (§5). {R1: S2-5; R2: S2-5; R5: Q5}

**E6** (replaces v4 E6):
> E6 i18n/locale: [G] locale classes {LANG=C, en_US.UTF-8, ru_RU.KOI8-R class, TZ absent} ×
> format corpus {transcript timestamps, session-list relative times, date pickers} —
> local-tz `toLocaleString` vs fixed-UTC behavior, width/truncation under non-UTF-8.
> **Boundary: B7 owns boot/survival in these envs; E6 owns format/rendering correctness
> parity in them** (no duplicated boot cells). [F] real locale env (same env matrix as B7,
> reused) [M] format-diff table ours vs ref per corpus item. {R1: S2-6, S3-5; R2: S2-6; R5: Q6}

**E7** (replaces v4 E7):
> E7 Error-message quality: [G] error-path enumeration = union of {C5 injected failures,
> D2 documented error codes, C18 CLI exit codes}; per path: message copy, toast action
> labels, dialog copy, exit code. [F] real messages from the C5/D2/C18 harnesses [M]
> (a) **copy-diff vs ref** per path (objective parity); (b) placeholder scan — regex corpus
> {"Not wired in this build", TODO, FIXME, lorem} across all reachable surfaces, any hit in a
> normal flow = finding; (c) exit-code table completeness — every emitted code documented or
> dispositioned. (v4's "comprehensibility/actionable check" is DELETED — not agent-executable;
> comprehensibility is proxied by parity + placeholder freedom.) {R1: S2-10; R2: S2-10; R5: Q7}

**E10** (replaces v4 E10 — closes nominal #1):
> E10 Storage/IO growth: [G] metrics = {DB main+WAL file bytes, log-dir bytes, state-file
> bytes + writes/hour (mtime count), process write_bytes delta /proc/PID/io} sampled every
> 5min during the F5/E3 soak, per session shape; write amplification = disk-bytes-written ÷
> new-data-bytes per soak segment. [F] same soak harness as E3 (shared sampling loop; real
> clock) [M] growth-slope per metric (bytes/h); threshold: ours ≤ max(2 × ref baseline,
> ref + 50 MB/h) on identical session shape (I10); **unbounded monotonic growth in any
> metric = finding regardless of threshold**; artifact = CSV + slope table + amplification
> ratios. {R2: S2-2; R4 restore; R5: Q8}

**E11** (replaces v4 E11 — closes nominal #2):
> E11 Idle CPU/energy: [G] idle states {no session, idle session open, post-stream session}
> × 60s windows; metrics = utime+stime delta /proc/PID/stat, voluntary+involuntary ctxt
> switches /proc/PID/schedstat (fallback `pidstat -w 1 60`), and **PTY output byte-rate at
> rest** (E1 capture harness reused) — animations-while-idle manifests as nonzero byte-rate
> with no cadence source active. [F] real clock, quiescent machine (no other load; harness
> asserts loadavg < 1.5), ref measured identically (I10) [M] per state+window: CPU% and
> wakeups/min; threshold: ours CPU% ≤ ref + 0.5%, wakeups/min ≤ ref ×1.5, byte-rate = 0
> unless a user-visible animation is active (each nonzero dispositioned); artifact = window
> table. {R2: S2-3; R4 restore; R5: Q9}

**E12** (replaces v4 E12):
> E12 Search/filter family: [G] per surface {session-list filter, transcript/message search}
> × query class {ascii, case variants, combining chars (NFC/NFD pairs), CJK, fuzzy} —
> **corpus committed in-repo as JSON** (queries × expected-matching/non-matching strings,
> I9-labeled real-session-derived). [F] corpus + I10 pin [M] result-parity assert vs ref:
> ordered result-list equality per query; fuzzy = **rank-order parity** (ties dispositioned
> as set-equality); normalization audit = which of NFC/NFD/case-fold each side applies.
> Artifact = per-query diff table. {R2: S2-4; R4 new; R5: Q10}

**F2 amendment** (append one sentence):
> Standing accounting entries (R5): `tui/src/devtools` DevTools.register export — dev-only
> debug-counter groups/entries; ours absent-justified (no parity surface; R5-Q11).

**F7 amendment** (append one sentence):
> Pre-registered dispositions (R5): ref-side `openTelemetry` config key (recognized in
> `config/normalize.ts`, no implementation in ref core — leaf round-trip owned by C4);
> event-logger writes (in F7 files/events scope, parity expected); stats-server traffic =
> U8 OOC. These rows prevent false positives in the ours-minus-ref network/env diff.

**I3 amendment** (append cadences): "pinned cadence sources: spinner 80ms (`spinner.tsx`),
StartupLoading 500ms/3s, reconnect 1s/5s, update-check interval, attention bell timing" —
(I3 already lists all but spinner 80ms; add it.)

## §4 E vs C/F overlap resolution

| Pair | Relationship | Resolution |
|---|---|---|
| E6 ↔ B7 | same locale env cells | boundary written into both: B7 = boot/survival, E6 = format correctness (R5-Q6) |
| E1 ↔ A4 | both touch frames | A4 = per-frame CONTENT at clock steps; E1 = interval DISTRIBUTION; E1's frame = A4's drive source (stated in E1 rewrite) |
| E4 ↔ A3 | both touch huge data | A3 = golden-frame content parity; E4 = first-paint time only (stated in E4 rewrite) |
| E9 ↔ C5 | both touch failures | C5 = recovery behavior from injected failure; E9 = post-exit SYSTEM residue (children/PTYs/temp/locks/termios). No cell duplicated. |
| E10/E11 ↔ E3/F5 | soak sharing | by design: F5 names the collectors; E10/E11 rewrites explicitly reuse the E3/F5 sampling loop. Not duplication. |
| E8 ↔ F7/I8 | canaries | complementary: E8 redaction canaries (payload-visible) vs F7 extra-write/call injection (count/args). Keep both. |
| E12 ↔ C15/C2/C20 | search surfaces | C15 = completion triggers, C2 = nav graph, C20 = ripgrep tool-side search; E12 = user-facing filter/search behavior. Distinct. |
| E1 paste ↔ B4 | paste | B4 = forms/bytes correctness; E1 = paste-during-stream timing (named cell, evidence: ref `prompt/index.tsx` enqueuePaste serialization queue). |

No contradictions found; seven boundaries stated, one new named cell (paste-during-stream).

## §5 Injectable canaries for E1/E3/E5 (I8 per-axis tier — run BEFORE first evidence)

**E1 stall canary.** Break: patch our render flush with a one-shot 250ms sleep, injected in
exactly one matrix cell (dialog-open under streaming load). Must catch: detector flags ≥1
stall-burst in that cell AND 0 bursts in the 3 control cells. False-positive leg: synthetic
evenly-spaced flush series → 0 findings. Both legs recorded in the I5 canary corpus.

**E3 leak canary.** Break: spawn a background task retaining 10MB/min in a never-drained
buffer (never eligible for GC — true leak, not GC lag). Must catch: 30-min validation run
reports slope ≈ 600 MB/h ±20% and flags; control (task disabled) slope ≈ 0 within noise.
Validates sampling cadence + slope math before any 4h soak counts as evidence.

**E5 accessibility canaries.** (a) contrast: set one theme token pair fg==bg (ratio 1:1) →
ratio assert fails exactly that cell. (b) color-independence: set busy/error colors to a
luminance-equal red/green pair → grayscale/luminance check must fail. (c) key-only walk:
unbind one target's key path → walk reports exactly that target unreachable. Three legs, one
canary run, all failures must localize to the injected defect.

## §6 Missing-quality-dimension sweep (adversarial, ref-grounded — this round's fresh-eyes)

Checked in ref tree this round (paths cited, not remembered):

| Candidate | Ref evidence | Disposition |
|---|---|---|
| telemetry/stats phone-home from client | `stats/` = server-side site; NO client POST found; `openTelemetry` = recognized config key only, zero OTel implementation in core | U8 OOC stands; F7 pre-disposition (R5-Q12); C4 owns the leaf |
| update channels (beta/stable) | `dialog-update.tsx` has no channel concept | none needed (C11 adequate) |
| benchmark scripts | zero `bench`/`perf`/`jank` matches in tui/src | ours' `frame_budget_tests.rs` already bound (I5) |
| perf/logging layer | `event-logger.ts` logs 5 event types via Effect | F7 events/files scope owns; E10 owns growth |
| devtools observability | `tui/src/devtools/index.ts` DevTools.register groups API | R5-Q11 → F2 accounting (absent-justified) |
| font fallback | terminal-side; app-side = cell metrics | B8 "user-font cell-metric dependence" owns |
| cursor blink cadence | config `cursor.blinking` (default true, no-op when style=default); blink itself is terminal-side after DECSCUSR emission | B6 DECSCUSR cell + C4 leaf own; I3 pinned-cadence note; NO new clause |
| bell behavior | C3 emission set + I3 timing | owned |
| paste latency under load | `prompt/index.tsx` pasteQueue serialization (pastes queue behind streaming) | NEW named cell in E1 (R5-Q1) — the one genuinely missing quality dim |
| spinner/animation cadence | `spinner.tsx` setInterval 80ms | A4/I3 pin (I3 amendment) |

**Net new quality dimensions found: 1 substantive (paste-during-stream timing → E1 cell) +
2 accounting dispositions (devtools, F7 pre-dispositions).**

## §7 Verdict + P3 directives handoff

### P2 verdict: **ONE more round needed (R6), then closed.**
- All 10 rewrites are now specified verbatim (§3) with metric+threshold+artifact — the
  "agent ran it, produced artifact X with number Y" bar is met on paper.
- What R6 must DO (not just integrate): the thresholds marked ×ref-baseline (E3, E4, E10,
  E11) require measured ref baselines under I10 — they are RATIFICATION targets, not yet
  numbers. E1/E3/E5 canaries must run before first evidence (I8 per-axis tier). E12 corpus
  must be committed and its first parity pass run (R4 directive 3, still unstarted).
- Exit criterion for P2 at R6: v5 integrated verbatim, appendix diff zero (now 114 IDs:
  102 + 12), 5 canary legs green, ref baseline table measured, E12 corpus + first-pass
  artifact exists, zero NOMINAL clauses remain.

### Directives for SPEC-R6 (integration + first-evidence round)
1. **Integrate §3 verbatim** per I11 protocol; +12 appendix rows (R5-Q1…Q12); table-diff the
   appendix (the zero-loss script from R4).
2. **Run E-baselines**: ref-side measurements under I10 for E2 (TTI N=10), E3 (one 4h
   shape), E4 (p95 per family), E10/E11 (soak-window metrics) → threshold-ratification
   table; adjust any ×1.5/×2 factor that baseline data contradicts, recording the delta.
3. **E1/E3/E5 canaries first** (I8): 5 legs from §5; any leg that fails to localize =
   instrument redesign on the spot (I8 rule).
4. **E12 first evidence**: commit the corpus JSON, run result-parity vs ref, file findings.
5. **Carry-forward from R4 (still unstarted, P3-owned after this round):** F6 pair-corpus
   materialization + interaction-of-the-round; F7 tracer prototype on one surface. These are
   F-axis work — schedule as P3's opening move, not R6's.
6. **P3 preview** (adversarial deepening track): E8 probe-set expansion (entropy sampling,
   SSRF target list), F4 real-drop matrix on B9-shaped links, F6 no-prior-finding pairs,
   plus first field-round trigger review (I10b universe vs any new ref packages).

## Round log (this lane)
- E1–E12 exhausted: 2 OK (E8, E9), 10 rewrites proposed verbatim (§3) — including both
  P1-residual nominals E10/E11 closed on paper.
- Fresh-eye anchors verified in ref: StartupLoading 500/3000 (`startup-loading.tsx`),
  spinner 80ms (`spinner.tsx`), pasteQueue (`prompt/index.tsx`), **auditA NOT FOUND**,
  openTelemetry key-only, no client telemetry, no update channels, no bench scripts.
- Overlaps: 7 boundaries resolved, 0 contradictions; 1 new quality dim (paste-during-stream)
  + 2 accounting dispositions.
- Canaries designed for E1/E3/E5 (5 injectable legs, localization required).
- scan-spec.md NOT touched (focus round; v5 diff proposed in §3 for R6).
- Verdict: P2 = one more round (R6: integrate + baselines + canaries + E12 first pass).
