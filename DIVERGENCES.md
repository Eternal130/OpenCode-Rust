# Known Divergences from the Reference Implementation — opencode-rust Register

> **Note (user ruling, 2026-11-22)**: this is the public register of known divergences (distilled from the original 43-round adjudication history). The full history — per-round rulings, G-1..G-464 fix forensics, incident records, cleanup notes — is deliberately not distributed with the public repo (the 2026-11-22 privacy cleanup rewrote history); full copies live in local backups and an out-of-repo archive (not committed). The only live register for active divergences is `tools/diff/realchain/adjudications.json` (the adjudication registry); the development standards live in the local private book (git-ignored, not in a clone).

## I. Known divergences from the reference (as of 2026-11-22)

### Current state

- Open unresolved gaps (🟡/❌): **zero** — G-1..G-464 are all closed (last batches 2026-11-21/22: G-428 reasoning positioning, G-430 animations six faces, G-93 $EDITOR attachment, G-464 skill attachment, G-94/95/96/400 server API alignment, G-439 managed copy, empty-stream remnants).
- realchain adjudication registry: `tools/diff/realchain/adjudications.json` (10 entries, all benign: framework-diff×6 / expected-shape×3 / env-artifact×1; exit semantics and the checker live in `tools/diff/realchain/adjudicate.py`).
- Pending live-machine observation (1): busy breathing-light symptom ⑦''② — not reproduced in-tree, feature present, frame sequence pinned; awaiting user reproduction material (screenshot + OC_TUI_DEBUG=1 trace).

### Documented-decision families (31 ℹ️ entries — intentional divergences from the reference, not gaps)

(Originally 34: G-211 split-footer was fully ported via ADR-0019 and flipped ✅ out of the family; G-328/G-329 were re-audited and flipped ✅ out — see "Closed register".)

**① Intentionally not ported (6)**

| G# | Divergence in one line | Ruling source |
|---|---|---|
| G-32 | v1/legacy schema family (legacy-event, v1 session events, rpc.ts, pty-ticket, durable-event-manifest, run/v1 compat layer) intentionally not ported — only affects old-data interop/replay, no impact on the pure v2 surface | Round-7 closure ruling (documented decision) |
| G-163 | workerd runtime target and ServerFetch.make not ported (no JS-runtime embedding target) — the server endgame is native boot_and_serve(axum) | Round-11 register closure (G-17 family) |
| G-212 | TUI storybook/test environment-variable surface not ported (~1400 lines of dev-time fixtures) | Gate D3 ruling 4 |
| G-238 | heap snapshot (SIGUSR1) / CPU profile (SIGPROF) not ported — V8 inspector platform specifics; native alternatives are pprof/tokio-console | Round-15 ℹ️ entry (G-17 family style) |
| G-313 | Plugin npm package startup auto-install not ported — installation authority belongs to `opencode plugin add`; the resolve-failure wire hint has landed | Round-17 entry; 2026-11-22 re-audit annotation |
| G-38 | OTel feature not on by default (ref defaults it on); release builds pull no telemetry dependencies | Round-3 ℹ️ (release decision) |

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
- Watchlist (live): `pty_composer_cycle_type_edit_submit_reply_stats_solid_path` (once), `pty_wheel_after_real_terminal_probe_replies` (once), the W38 settle-race fourth arm has landed (recurrence escalates), `tl_tool_loop_pair` (twice). Flake whitelist: §2.2#4.

## Pointers

- Development standards and the quick-reference table → the local private book `docs/GAPS-AND-PITFALLS.md` (git-ignored; the authoritative extraction is `tools/lane-brief.sh` verbatim — see `AGENTS.md`)
- Architecture comparison → `ARCHITECTURE-VS-REF.md`
- Full history (43 adjudication rounds, G-1..G-464 forensics) → local backups and the out-of-repo archive (not committed; the complete adjudication history is not distributed with the public repo — the 2026-11-22 privacy cleanup rewrote history)
