# Known Divergences from the Reference Implementation — opencode-rust Register

> **Note (user ruling, 2026-11-22)**: this is the public register of known divergences (distilled from the original 43-round adjudication history). The full history — per-round rulings, G-1..G-464 fix forensics, incident records, cleanup notes — is deliberately not distributed with the public repo (the 2026-11-22 privacy cleanup rewrote history); full copies live in local backups and an out-of-repo archive (not committed). The only live register for active divergences is `tools/diff/realchain/adjudications.json` (the adjudication registry); the development standards live in the local private book (git-ignored, not in a clone).

## I. Known divergences from the reference (as of 2026-11-22)

### Current state

- Open unresolved gaps (🟡/❌): **one** — G-466, narrowed by lane va5 (2026-11-23) to the 10x6 structural residue family + item 7. G-465 closed and G-467.1 closed the same day (lane va5); G-467.2 accepted by controller ruling (framework-diff family, no fix scheduled). G-1..G-464 are all closed (last batches 2026-11-21/22: G-428 reasoning positioning, G-430 animations six faces, G-93 $EDITOR attachment, G-464 skill attachment, G-94/95/96/400 server API alignment, G-439 managed copy, empty-stream remnants).
- realchain adjudication registry: `tools/diff/realchain/adjudications.json` (10 entries, all benign: framework-diff×6 / expected-shape×3 / env-artifact×1; exit semantics and the checker live in `tools/diff/realchain/adjudicate.py`).
- Pending live-machine observation (1): busy breathing-light symptom ⑦''② — not reproduced in-tree, feature present, frame sequence pinned; awaiting user reproduction material (screenshot + OC_TUI_DEBUG=1 trace).

#### Open gaps (🟡) — narrowed by lane va5, 2026-11-23

Found by a paired ref↔rust probe at three non-default geometries (10x6, 40x12, 160x40; two runs per end per size) — the probe existed because `a2_geometry_width_sweep_10_40_160` had been passing over a failed driver (a null driver report read as PASS). It was graduated (lane va4) into permanent paired scenarios (`geom10_narrow_extreme_pair` / `geom40_narrow_pair` / `geom160_wide_pair` in `crates/oc-tui/tests/realchain_pair.rs`, states home0/home/dialog/enter, export-only). Lane va5 then fixed every sub-item reachable with a contained change and re-pinned `a2`'s expectations; verified EXACT: geom40-home/home0/dialog/enter, geom160-enter, geom160-dialog, geom40-enter. `a2` is un-ignored and green (10 consecutive runs + one pty-profile run).

- **G-465 — CLOSED 2026-11-23** (va5 `41c339f1`+`fbd1d5a9`): the muted ` for {project}` suffix renders via `SelectProps::with_title_suffix` (ref `dialog-session-list.tsx:198-207`); adjudication entries cleared.
- **G-466 — narrowed; fixed 2/3/4/6(home rows)/9 and the 40-col faces of 1/8** (va5 `1c871584`, `a774892a`, `1bbcf04a`, `3a1f56c0`; ref anchors logo.tsx:57-75, dialog-select.tsx:838, sidebar.tsx:33-48, index.tsx:1370, footer.tsx:87-97). Remaining open, all at 10x6:
  - 1+8 residue — session-route composer vertical seating: ref `[strip r0][blank r1][┃pad r2..meta r5]` vs port `[┃pad r0..footer r5]` (the tab-title row is suppressed). Structural: replicating ref taffy's per-child flexShrink collapse under column overflow vs the port's fixed-height mounts. (`va4-g466-enter-10-tabtitle`, `va4-nc-enter-10-composer-rows`, `va4-nc-composer-box-offset-10-home` stay adjudicated pending-fix.)
  - 6 residue — narrow truncation glyph shapes at 10x6 home (`Ask a`/`Ask`, `c...…`/`claude-`), same structural family.
  - 7 — 10x6 dialog first truncated title (`S` vs `e` + line 6); the ref truncation strategy at a 2-column budget is unresolved (`va4-nc-dialog-10-truncated-titles`).
  - 5 — ACCEPTED (ref-side artifact): ref's zero-row viewport still leaks one overprinted row (`AlphaSession Title That Must T`); the port clips correctly. Kept as the narrowed benign entry `va4-g466-3-5-dialog-40-overprint`.

  The bottom-right pill (`local` vs `2.0.24`) is the known build-artifact delta the harness normalizes (G-315 family), not a gap.
- **G-467 — default-geometry divergences surfaced by the no-alnum normalizer fix (`85029a33`)**:
  - 1 — CLOSED 2026-11-23 (va5 `2c829595`): kinsoku pull-back (UAX-14 cl/ex subset) — ref never starts a wrapped line with CJK closing punctuation; `w13-g-cjk-settled` EXACT.
  - 2 — ACCEPTED by controller ruling 2026-11-23 (framework-diff family, no fix scheduled): the `█` column in mid-capture frames is the port's permission-body scrollbar track (16-row mini-footer dock) vs ref's composer-slot SessionQuestion (`permission.tsx:514-523`, border-left, maxHeight 15). Settled frames are EXACT (`w13-perm-once-settled`); the residual is the same docked-placement/mid-capture family already adjudicated as `w13-perm-once-dialog-midstream` (va5 re-registered `va4-nc-wordmark-bleed-w13-permonce` with this root cause). Changing it would be a permission-body renderer refactor.

  Historical caveat: every paired EXACT verdict recorded before `85029a33` verified only alnum-bearing rows; citing one as full-frame proof requires re-export on the fixed harness.

### Documented-decision families (32 ℹ️ entries — intentional divergences from the reference, not gaps)

(Originally 34: G-211 split-footer was fully ported via ADR-0019 and flipped ✅ out of the family; G-328/G-329 were re-audited and flipped ✅ out — see "Closed register".)

**① Intentionally not ported (7)**

| G# | Divergence in one line | Ruling source |
|---|---|---|
| G-32 | v1/legacy schema family (legacy-event, v1 session events, rpc.ts, pty-ticket, durable-event-manifest, run/v1 compat layer) intentionally not ported — only affects old-data interop/replay, no impact on the pure v2 surface | Round-7 closure ruling (documented decision) |
| G-163 | workerd runtime target and ServerFetch.make not ported (no JS-runtime embedding target) — the server endgame is native boot_and_serve(axum) | Round-11 register closure (G-17 family) |
| G-212 | TUI storybook/test environment-variable surface not ported (~1400 lines of dev-time fixtures) | Gate D3 ruling 4 |
| G-238 | heap snapshot (SIGUSR1) / CPU profile (SIGPROF) not ported — V8 inspector platform specifics; native alternatives are pprof/tokio-console | Round-15 ℹ️ entry (G-17 family style) |
| G-313 | Plugin npm package startup auto-install not ported — installation authority belongs to `opencode plugin add`; the resolve-failure wire hint has landed | Round-17 entry; 2026-11-22 re-audit annotation |
| G-38 | OTel feature not on by default (ref defaults it on); release builds pull no telemetry dependencies | Round-3 ℹ️ (release decision) |
| COV-tabs-marquee | Tab-title hover marquee is not ported — ref shifts a hovered truncated title's content window ~1 cell per tick (`marquee.ts`, `session-tabs.tsx:1652`); the port renders the tab-pulse envelopes (ignition/release/sweep/flash/marquee) as the static C2 class (`components/tab_pulse.rs`), so a truncated title stays truncated. `vt_temporal_marquee_shifts_one_cell_per_tick` is `#[ignore]`d as the pin: it turns green the day the feature lands | Retired coverage-ledger row (tabs, GAP※), re-verified unported 2026-11-22 |

**② Intentional corrections / supersets (9)**

| G# | Divergence in one line | Ruling source |
|---|---|---|
| G-403 | HTTP session create with a bad location: rust rejects with 404 vs ref 200 + dangling session (ref-side suspect behavior); clients handle 404 | Round-28 ruling = intentional correction |
| G-407 | ref's production validator compiles pattern/structured-const errors into accept-all (production validates nothing); rust faithfully implements and is intentionally stricter — schemas relying on ref laxness are rejected by rust | Round-27 JSON-SCHEMA ruling |
| G-314 | custom/local/console provider names get a case-insensitive fallback (exact-first + eq_ignore_ascii_case) — pure leniency delta; extra accepted configs add no failure surface | Round-17 entry |
| G-316 | ssh-askpass prompt takes argv[1] per the OpenSSH convention — ref's single-file compiled binary slicing (2) yields an empty string, a genuine ref-side defect | Round-17 intentional correction |
| G-317 | Bounded-timeout regime: session.remove 10s / TOOL_JOIN 5s / UNWIND 60s — ref's unbounded waits are bounded in rust for wedge scenarios | Round-17 batched entry |
| G-389 | keybind rust-extra superset of 8 actions (composer.close/tab.next/prev, subagent.toggle-activity, dialog.select.section.next/prev, stats.open/close) — ref has no bindable keys for these | Round-24 census KEYBINDS entry |
| G-390 | `experimental.file_retention.cleanup_interval` — a rust-only hardening knob (superset key) | Round-24 census CONFIG-KEYS entry |
| G-318 | repository-cache refresh timestamp persisted to `<repos>/.repository-cache.json` instead of KV — observable only when ref/rust binaries share the same data directory | Round-17 bounded-divergence entry |
| G-240 | opencode-pty binary exempt from sha256 pinning (rust has no bundled-install path; nothing comparable); OPENCODE_PTY_BIN override owner/symlink-refusal hardening has landed | RW-T2 intentional trade-off; gb9 lane5 hardening |

**③ Bounded shims / architectural substitutions (6)**

| G# | Divergence in one line | Ruling source |
|---|---|---|
| G-28 | Plugin TSX rendering is a bounded shim — the render tree is flattened over IPC into a `{title,rows,clickable}` row shape; the markdown renderer registration chain and panel slot have landed; the ecosystem declaration surface is narrowed to the bounded row shape + node-module loading restrictions | Round 3 onward, multiple rounds; 2026-11-12/22 narrowing annotations |
| G-289 | One bun guest for the whole process vs ref's per-Location plugin instances and scopes — plugin module-level state is shared when multiple locations coexist; per-Location isolation is future engineering, not a gap | Round-18 ruling = register, don't change code |
| G-386 | Plugin RPC/ctx bridge shape delta: 13 read faces implemented guest-locally (agent/command/mcp/skill/tool list/reload etc.) + ctx.command.add / flattened integration direct-call superset (RPC RUST_ONLY 25 keys) — no behavioral gap | Round-24 census; Round-25 re-scan annotation |
| G-164 | announce.rs is the server-side stand-in for location event cascades (project.updated 60s rate limit, empty-frame reproduction, vcs.branch.updated subscription cascade) — new location services/events must keep the cascade list and parity goldens in sync | Round-11 register closure; premise revised after G-218 |
| G-326 | session.diff registered as public-ephemeral but never published anywhere in-repo (SSE does not forward it, matching ref's exposure) — watch condition: if it ever starts being published, it must join PRIVATE_TYPES first to avoid workspace-diff leaks | Round-12 classification; conditional watch slot |
| G-39 | Clipboard write is OSC52-only (the read arm is solid: pbpaste/xclip/xsel/wl-paste) — copy silently fails on terminals without OSC52 support | Round-3 ℹ️ intentional |

**④ Purely documentary (10)**

| G# | Divergence in one line | Ruling source |
|---|---|---|
| G-12 | `context_management.edits[].type` hardcodes `"compact_20260112"` (inbound is also fail-fast validated, not silently dropped); new edit types need manual edits in two places | Round-3 ℹ️ |
| G-16 | Hand-written serde maintained in parallel with ref's Effect Schema, drift-guarded by goldens (the largest long-term drift surface); provider folding needs two-place edits; openapi.json sha256 pin needs a manual baseline-bump — **the vendor-list auto-comparison test is not built (the family's only real TODO)** | Round-3 ℹ️ maintenance pit |
| G-17 | Intentional omissions: ref dead code (mcp/resolve.ts, plugin/inventory.ts), web/desktop/enterprise/cloud packages not ported, rust-extra defensive supersets (upgrade -m, completion, --force etc.) — all commented in code | Round-3 ℹ️ correct decision |
| G-40 | Documentary wire deltas: `x-opencode-build` response header (absent in ref), empty pagination cursor explicitly null instead of key omission — normalized away in golden comparisons | Round-3 ℹ️ |
| G-290 | Dialog ctrl+c clears and closes on first press (ref's first press only clears, staying open) — intentional decision preventing the ctrl+c×N exit-chain swallowing that caused a 90s wedge incident | Round-18 entry (G-226 style) |
| G-315 | devtools default-on lacks the `channel==="local"` branch (rust has no channel concept) — a developer-default delta only, no impact on release users | Round-17 entry |
| G-408 | Numeric/equality semantics record family: integer ≥2^53 ×3 (Effect safe-integer domain vs u64/i64 full domain), `5.0` as integer, enum `1.0`, uniqueItems structural equality vs reference identity ×2 — inherent JS number-model differences, no action surface | Round-27 JSON-SCHEMA |
| G-409 | SSE projection architectural deltas, 4 families: anchor-less delta creation + out-of-order backfill (rust superset), text.started lazy creation (transient delta converging on the same end state), replay dedup layer position (inside Store vs the app SSE layer), session.moved pure event-stream gating | Round-27 SSE-PROJECTION cause family |
| G-410 | HTTP wire documentary items, 5: cost `0.0` vs `0`, session multi-slug/subpath superset keys, project/skill default-key null vs omission, error message missing offender suffix, integration catalog 224 vs 229 (member freshness delta) | Round-27 HTTP-FULL |
| G-411 | ACP evidenced shape deltas, 3: pending tool_call always sets name + default status/locations (vendored patch domain), abnormal ordinal `unwrap_or(0)` vs ref template string, missing delta field as `""` on the wire vs ref dropping the key | Round-27 ACP-TRANSLATE B1-B3 |

### Closed register (summary)

- Full fix/forensics text for G-1..G-464: the complete adjudication history is not distributed with the public repo (2026-11-22 privacy cleanup rewrote history); full copies live in local backups and the out-of-repo archive (not committed).
- Recent flips out of the families: G-211 split-footer fully ported and consumed via ADR-0019; G-328 TAB_SPINNERS all four styles landed; G-329 mini demo scripts 13/13 (2026-11-22 re-audit flips — the fifth stale-ledger instance).
- The coverage ledger (formerly docs/audit/diff/COVERAGE.md, 12 families, four audit rounds into maintenance mode) and the diff-ledger family (JSON-SCHEMA/HTTP-FULL/TUI-FULLSCREEN/ACP-TRANSLATE etc., 11 books): left the tree in that batch and are not distributed with the public repo; **the coverage-completeness ruling (§2.2#12) remains in force** — when new testable surfaces appear (user reports, code reading, upstream diffs), revive the ledger from local backups or rebuild it.
- Watchlist (live): `pty_wheel_after_real_terminal_probe_replies` (once), `tl_tool_loop_pair` (twice), `geom160_wide_pair` (once — battery1 TRY1 red 276s / TRY2 187s pass, shape text unrecorded, re-runs clean), `pty_fuzz_smoke` (once — merge-gate TRY1 red, retry passed, re-runs clean), `w13_g_longline_pair` (once — va5 battery TRY1 red retried pass), `pty_reconnecting_overlay_on_loss_and_recovery` (once — va5 merge gate TRY1 red retried pass), the W38 settle-race fourth arm has landed (recurrence escalates). (`pty_composer_cycle_type_edit_submit_reply_stats_solid_path` left the list 2026-11-23: instrumented down to a harness mark race, fixed.) Flake whitelist and its entry threshold: §2.2#4.

## Pointers

- Development standards and the quick-reference table → the local private book `docs/GAPS-AND-PITFALLS.md` (git-ignored; the authoritative extraction is `tools/lane-brief.sh` verbatim — see `AGENTS.md`)
- Architecture comparison → `ARCHITECTURE-VS-REF.md`
- Full history (43 adjudication rounds, G-1..G-464 forensics) → local backups and the out-of-repo archive (not committed; the complete adjudication history is not distributed with the public repo — the 2026-11-22 privacy cleanup rewrote history)
