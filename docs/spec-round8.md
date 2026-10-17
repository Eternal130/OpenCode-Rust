# SPEC-R8 — Round-8: RATIFY-1 CURE (two material gaps + residual riders; v7 integration)

**Lane:** SPEC-R8 (worktree `.worktrees/lane-spec8`, branch `lane/spec8`).
**Date:** 2026-10-17. **Inputs:** `docs/scan-spec.md` **v6** (appendix
129/129, FINAL candidate 1), `docs/ratify-1.md` (RATIFY-1 **FAIL**: 2
material gaps MG-1/MG-2 + 8 non-blocking residuals; proposed cures — none
applied by that lane). **Output:** `docs/scan-spec.md` **v7** (appendix
140/140), this report. Per `docs/scan-program.md`: a failed ratification
routes to an integration round, the convergence/ratification counter
resets, and a new FINAL candidate precedes RATIFY-1′/RATIFY-2′.

## §0 Evidence re-verified this round (fresh, against the trees)

Every RATIFY-1 fact underpinning the cures was re-verified before editing:

1. **`packages/cli/src/index.ts:8`** imports `Observability` from
   `@opencode/util/observability`; **`cli/src/index.ts:126`** provides
   `Observability.layer({endpoint: process.env.OTEL_EXPORTER_OTLP_ENDPOINT,
   headers: process.env.OTEL_EXPORTER_OTLP_HEADERS, client/version/channel…})`
   into the main effect — unconditional at CLI boot.
2. `packages/util/src/observability.ts` (layer, `Layer.catchCause(() =>
   local)` fallback, `OtlpSerialization.layerJson` + `FetchHttpClient.layer`
   + `OtlpExporter.layerFlusher`) and `observability/otlp.ts` (env reads
   `OTEL_EXPORTER_OTLP_ENDPOINT` / `OTEL_EXPORTER_OTLP_HEADERS` /
   `OTEL_RESOURCE_ATTRIBUTES`; `runID()`; BatchSpanProcessor;
   AsyncLocalStorageContextManager set as GLOBAL context manager) exist as
   described. `observability/logging.ts`: `formatStructured` `key=value`
   lines; shared append-only log file compacted in place —
   `LOG_MAX_BYTES` 50 MB, `LOG_KEEP_BYTES` 25 MB, `LOG_TRIM_INTERVAL`
   "1 hour", `LOG_TRIM_LOCK_STALE_MS` 5 min.
3. `openTelemetry` is a recognized config key at
   `core/src/config/normalize.ts:49` (leaf round-trip stays C4's).
4. **Ours:** `grep -rn "OTEL_" crates/` = **0**; `grep -rIn "otlp"
   crates/ --include="*.rs"` = **0** (one base64-blob false positive in a
   fixture excluded) — zero OTel implementation, i.e. the live divergence
   is real, not archival.
5. **`ls -1 /opt/opencode-ref/packages/ | wc -l` = 35**; the v6 I10b pin
   enumerated 34; sorted diff = **`plugin-browser`** missing (see §3).

## §1 MG-1 cure — the telemetry/observability family is now OWNED (new C22)

Applied all four cure legs from `docs/ratify-1.md` §MG-1:

- **(a) New clause C22** (full [G][F][M] — what ref emits/spans/envs, what
  ours must do, how a scan verifies), quoted verbatim as integrated:

```
C22 **Telemetry/observability family** (v7, NEW — R8 MG-1(a); the family
   RATIFY-1 found unowned: ref wires OTLP observability in PRODUCTION,
   unconditionally, at CLI boot — `packages/cli/src/index.ts:8` imports
   `Observability` from `@opencode/util/observability`, and
   `cli/src/index.ts:126` provides `Observability.layer({endpoint:
   process.env.OTEL_EXPORTER_OTLP_ENDPOINT, headers:
   process.env.OTEL_EXPORTER_OTLP_HEADERS, client/version/channel…})`
   into the main effect; R5's adjudication examined `packages/core` only
   and concluded "no OTel implementation" — a POST grep cannot find
   env-gated boot-layer wiring; ours has zero OTEL_*/otlp code (grep
   `crates/` = 0 code hits, 2026-10-17) — a live ref-only family
   divergence invisible to F7's ours-minus-ref letter and to any
   core-only scan; exactly the U-family class R3's sweep existed to
   catch, plus an I6-class masking premise). [G] cells =
   {env-read set, boot-layer wiring, emission gating, config-key
   behavior, always-on file-log layer} x {ours, ref}: (1) env reads —
   the ref set is exactly {OTEL_EXPORTER_OTLP_ENDPOINT,
   OTEL_EXPORTER_OTLP_HEADERS, OTEL_RESOURCE_ATTRIBUTES} (read at boot
   in `cli/src/index.ts` and in `util/src/observability{.ts,/otlp.ts}`),
   enumerated name-by-name per F7's env-read scope; malformed values
   (bad headers k=v list, bad OTEL_RESOURCE_ATTRIBUTES W3C list) are
   cells, not errors (ref parses defensively, drops bad attrs). (2) boot
   layer — provided UNCONDITIONALLY: the layer, the env reads, and
   structured logging are always on; only emission is endpoint-gated;
   OTLP-layer failure falls back to the local-only logger
   (`Layer.catchCause(() => local)`). (3) emission — OTLP JSON over HTTP
   (`OtlpSerialization.layerJson` + `FetchHttpClient.layer` +
   `OtlpExporter.layerFlusher`): logs → `{endpoint}/v1/logs`, traces →
   `{endpoint}/v1/traces` (BatchSpanProcessor; AsyncLocalStorage-
   ContextManager installed as the GLOBAL context manager), headers from
   OTEL_EXPORTER_OTLP_HEADERS parsed `k=v,k=v`; resource = serviceName
   `opencode` + serviceVersion + `deployment.environment.name` = channel
   + `opencode.client` + `opencode.run`/`service.instance.id` = runID
   (8-char UUID per process). (4) config key `openTelemetry`
   (recognized in `core/src/config/normalize.ts:49`; leaf round-trip =
   C4's; behavior disposition on/off/endpoint/headers vs ref = here).
   (5) always-on file-log layer (endpoint-independent): structured
   `key=value` lines (timestamp/level/run-id/spans/annotations) to a
   shared machine-wide append-only log file, compacted in place
   (50 MB cap → 25 MB keep, 1 h trim interval, 5 min stale-trim-lock),
   local-vs-channel naming per `Logging.loggers(local, channel)`.
   [F] I2b binary leg (ref 2.0.25 user binary — telemetry lives in the
   CLI boot path, NOT in the 2.0.24-source `packages/core`, which is
   why core-only scans kept missing it); B7's OTEL_* env cells reused;
   I9 provenance for the mock collector (instrument, never sole
   evidence). [M] mock OTLP collector (loopback HTTP recorder) as the
   standing assert target: (i) endpoint SET ⇒ after one logged action +
   one traced span, the collector holds POSTs to /v1/logs and /v1/traces
   with parsed headers + resource attrs (channel/version/client) + JSON
   body shape — ours vs ref-binary per cell; (ii) endpoint UNSET ⇒ zero
   collector traffic while the file-log layer still writes (file exists,
   structured lines carry run-id + channel); (iii) env-read trace parity
   (F7 scope: name + count + args); (iv) ours disposition AS EVIDENCED:
   zero OTel implementation = a standing divergence finding until
   dispositioned (absent-justified requires a product decision, not scan
   silence). I8 canary per the matrix's R8 design addendum (binding
   before first evidence). Artifact = per-cell parity table {env x cell x
   ours/ref-binary} + collector capture corpus. **Boundary: U8's OOC
   entry stays SERVER-SIDE `stats/` only — client observability is IN
   scope here.** {R8: MG-1}
```

- **(b) B7 gains OTEL_* env cells** (inline in the env-cell enumeration,
  tagged R8-MG1(b), plus a rider block naming the cells — endpoint /
  headers (valid + malformed) / resource attributes (valid + malformed) —
  LIVE under plain `env(1)`), with an explicit **E6↔B7-style boundary:
  B7 owns boot/survival/smoke under OTEL_* envs; C22 owns the
  observability behavior in them** (env-read trace, emission gating,
  log-layer).
- **(c) F7's R5-Q12 pre-disposition REWRITTEN** (§2 below).
- **(d) F2 standing accounting entry** for `util/src/observability{.ts,/}`
  exports — parity-owned by C22, explicitly NOT absent-justified (zero
  OTel code in ours = live divergence held in C22's ledger until
  dispositioned); **I8 canary matrix gains a C22 design** (endpoint
  set/unset traffic-gating pair, binding before C22's first evidence, per
  the defer-or-design rule).

## §2 MG-1(c) — the F7 R5-Q12 re-disposition (false premise removed)

RATIFY-1 established that R5's adjudication examined `packages/core` only
(a POST grep cannot find env-gated boot-layer wiring), so F7's standing
pre-disposition — "no implementation in ref core" — was letter-true for
`packages/core` and **false for the ref product binary the scans actually
run**. The v6 sentence is replaced (ratify-1's proposed text, applied
character-exact):

> implementation lives in `packages/util/observability`, wired
> unconditionally in `cli/src/index.ts` (boot layer; emission gated on
> OTEL_EXPORTER_OTLP_ENDPOINT); core has none

…with the config-key leaf round-trip kept C4's and the telemetry behavior
family pointed at C22. The event-logger and stats-server pre-disposition
rows are untouched. Appendix row **R8-MG1-F7** carries the correction;
the R5-Q12 row now notes the re-disposition (bidirectional).

## §3 MG-2 cure — I10b enumeration pin corrected + census made mechanical

Mechanical census (this round, commands and outputs pinned):

```
$ ls -1 /opt/opencode-ref/packages/ | wc -l
35
$ ls -1 /opt/opencode-ref/packages/ | sort | tr '\n' ' '
ai app cli client codemode console containers core desktop effect-drizzle-sqlite enterprise function gui-extensions httpapi-codegen http-recorder identity latex merman plugin plugin-browser posts protocol schema script sdk server session-ui simulation stats storybook theme tui ui util web
```

Sorted diff v6-pin vs filesystem: **`plugin-browser`** was the one missing
name (v6: 6 exhaustive + 28 family-granular = 34 ≠ 35 claimed). The v7
pin restores it between merman and posts (D10-owned — browser
tunnel/proxy/connection/rpc/files — so no family was ever behaviorally
unowned, but the F2/I7 completeness backstop now materializes the full
universe). The census discipline block as integrated:

```
**Census discipline (v7): the pin is
    mechanical, not prose** — `ls -1 /opt/opencode-ref/packages/ | sort`
    must yield exactly the 35 names above (6 exhaustive + 29
    family-granular), and `ls -1 /opt/opencode-ref/packages/ | wc -l` =
    35 (verified 2026-10-17, RATIFY-1 and this round); round reports' (a)/(b)
    lists reconcile against that command's output, never against recall.
    {R8: MG-2}
```

(Reflowed for this quote; the spec carries the indent. The zero-loss
script compares squash-normalized.)

## §4 Residual riders (ratify R-A..R-D → R8-N1..N4) + registered N-1..N-4

The four unambiguous tightenings were applied verbatim from ratify-1's
proposed edits (each tagged in the spec; squash-compared by the script):

| Tag | Ratify ID | Clause | Applied text (verbatim core) |
|---|---|---|---|
| R8-N1 | R-A | E6 | assert = format-string equality per corpus item; any diff dispositioned. |
| R8-N2 | R-B | E9 | zero un-dispositioned residue = pass (artifact = per-exit-path residue audit table) |
| R8-N3 | R-C | C7 | artifact = per-job-state surface table + progress/lifetime assert |
| R8-N4 | R-D | B3 | artifact = per-stack routed-input trace table, assert = routing matches ref per legal stack |

The remaining four register entries are NOT ambiguous enough to tighten
without inventing policy, so they are **registered (REG)** in the appendix
rather than silently dropped: R8-N5 (=N-1, B7 linux-LIVE naming + B9
column thresholds — cosmetic, S3 CLEAN), R8-N6 (=N-2, F7 direction
blindness — structurally cured by C22's bottom-up ownership; F7's
ours-minus-ref direction unchanged, symmetric diff rejected as
ref-dev-noise-drowning), R8-N7 (=N-3, F1 aggregate closure — round
structure is the program doc's to govern), R8-N8 (=N-4, gui-extensions
transitivity — D6(U10)'s letter already forces the app disposition; the
v7 census makes the family accounting explicit).

## §5 v7 integration + zero-loss re-check

v6 → v7 changes: 1 new clause (**C22**); 1 pin correction (I10b); 8
amendment/rider sites (B3, B7 OTEL cells, C7, E6, E9, F2 accounting,
I8 C22-design, F7 re-disposition); header/lineage/round-log
updated; appendix **+11 rows → 140/140** (every RATIFY-1 register ID
mapped). Zero-loss re-check scripted (`tools/parity/spec-p3/
zero_loss_r8.py`, R7-style):

- **A clause census:** v6 82 → v7 83 — added C22, dropped 0, dupes 0.
- **B appendix census:** R1 52 · R2 30 · R3 20 · R5 12 · R7 15 · R8 11 =
  **140**.
- **C R8 traceability:** 11/11 appendix rows (R8-MG1, R8-MG1-F7, R8-MG2,
  R8-N1..N8); clause-side tags {R8: MG-1} (C22 + B7 + F2 + I8 + F7),
  {R8: MG-2} (I10b), {R8: R-A..R-D} (the four riders); no orphan tags.
- **D verbatim containment:** D1 — the F7 re-disposition + 4 residual
  riders are character-exact (squash-normalized) vs `docs/ratify-1.md`;
  D2 — the R8-authored blocks (C22 clause, census discipline) are
  character-exact vs this report.
- **E MG-2 census:** filesystem 35 == pin 35 (6 exhaustive + 29
  family-granular); set equality exact; the v6 pin's missing name was
  plugin-browser.
- **Cross-run:** `zero_loss_r7.py` against v7 — its D check (all **14**
  v6-verbatim blocks vs `docs/spec-round7.md`) reports **fails=none**;
  its C check still 15/15; its A/B checks "fail" only on expected growth
  (C22 added, appendix 129→140) — i.e. no v6 text was damaged by the
  v7 edits.

## §6 Declaration — v7 = FINAL candidate 2; ratification counter reset

Both material gaps are cured at their root — MG-1 by ownership (C22 +
re-disposition + env cells + accounting + canary design: the family can
no longer be invisible to any direction of scan), MG-2 by correction plus
a mechanical census the spec itself pins (the completeness backstop's
universe is now command-verifiable, not prose-recall). All eight
non-blocking residuals are either tightened verbatim (R-A..R-D) or
registered with reasons (N-1..N-4) — none dropped.

**CALL: `docs/scan-spec.md` v7 is declared the FINAL candidate (2nd).**
Per `docs/scan-program.md` (RATIFY-1 FAIL → R8 → reset): the ratification
counter RESETS — **RATIFY-1′ and RATIFY-2′ follow** (two clean
ratification rounds → LOCK). Fresh-agent stances S1-S5 + META on v7,
per the same protocol as ratify-1.
