# v2.0.24 Gap Burn-down — Master Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Each lane = one bounded dispatch (foreground, ≤45-min chunks). Steps use checkbox syntax.

**Goal:** Close every open functional gap in docs/GAPS-AND-PITFALLS.md §1 so the port reaches v2.0.24 parity (except items excluded by standing ruling: R-04 frontend, R1–R5 hook registrations, (b)-class coin).

**Architecture:** Serial specialist lanes (production-respect regime: ONE lane at a time), each in its own worktree, merged back with affected-crate build+test gates. Bounded feature fills first (JWKS / skill-URL / console), then divergence+verification lanes (E12/E2/SSE/F15/DBMIG), then the PTY daemon process boundary (design→oracle→implement), then CI/platform (R-11), then the consolidated gate.

**Tech Stack:** Rust workspace (`crates/oc-*`), ring 0.17 (already in tree — NO new crypto deps), reqwest 0.12, tokio, existing vt/PTY test harnesses, GitHub Actions.

**Spec:** docs/GAPS-AND-PITFALLS.md (gap register) + v2 reference at /opt/opencode-ref (read-only, commit e7a34f09bf).

## Global Constraints

- ONE lane at a time; no parallel build lanes (production-respect, 2026-10-18).
- Every local build/test through `tools/dev-gate.sh` (never raw cargo). Worktree lanes set `OPENCODE_DEV_GATE_DIR=<worktree>`; worktrees are created AS eternal130 (e.g. `tools/dev-gate.sh 'git -C /opt/opencode-rust worktree add .worktrees/<name> -b <branch> main'`).
- `CARGO_INCREMENTAL=0` for lane builds (disk regime).
- English commits; WIP commit every ~6 tool rounds; single tool call ≤45 min.
- Subagents NEVER run background tasks (host H-1). Foreground chunks only.
- TUI binaries run only inside the vt/PTY harness or stdout/stderr-redirected no-tty mode. OSC/escape probes only to owned PTYs.
- safe_signal only; tests use own child handles; never bare-PID kills.
- Merge to main only after affected-crate build+test green in the lane worktree; final report must paste `git log -1 <branch>` as proof-of-landing; orchestrator verifies deletion lists on merge.
- Affected-crate tests + targeted suites only per lane; ONE consolidated full gate (workspace nextest + pty profile + release + budgets + CI) at the END.
- Every fix on an interaction surface carries PTY/field-level verification (implementation exists ≠ chain connected).

## Review Focus

1. **Forged/expired/wrong-issuer tokens accepted** → JWKS lane pins the full v2 failure matrix (forged sig, wrong iss/aud, expired, missing exp/sub, nonce mismatch, unknown kid w/ cooldown).
2. **Path traversal / cross-origin via skill URL index** → skill-URL lane pins traversal/absolute/`..`/cross-origin rejection before any file fetch.
3. **Cost/limit silent merge inversion** (merge vs replace) → console lane pins cost REPLACE, limit per-field, capability per-field, baseURL precedence model>provider>base + provider-deletes-inherited rule.
4. **Daemon restart killing live terminal children** → PTYD lane must prove (process-level test) children survive server death and replay restores after adopt.
5. **Golden self-attestation on composition models** → F15/PTYD verified against v2 source file:line + PTY behavior, not goldens alone.

---

## Phase 1 — Bounded feature fills

### Task 1: Lane JWKS — RS256/JWKS ID-token verification

**Files:** Modify `crates/oc-core/src/integration/providers_enterprise.rs` (verify_id_token_claims → async verify_id_token); Create `crates/oc-core/src/integration/jwks.rs`; Test `crates/oc-core/tests/` (or unit mod) — mirror v2 matrix.

**Interfaces:** Produces `pub(crate) async fn verify_id_token(id_token:&str, client_id:&str, nonce:&str, jwks_url:&str) -> Result<(),String>` + `struct RemoteJwks` (reqwest-based, kid-indexed, refetch on kid miss with ≥30s cooldown, injectable base URL for tests). ring-based RS256: JWK n/e (base64url) → DER SEQUENCE(INTEGER n, INTEGER e) → `ring::signature::RsaPublicKey::der` → `UnparsedKey<BS256>`. Verify order per v2 `chatgpt.ts:439-459` / jose: signature(RS256 only, header alg enforced) → iss `https://auth.openai.com` (exact) → aud == client_id (exact) → exp required, tolerance 0 → required claims exp/nonce/sub present → sub non-blank → nonce exact. ALL failures surface as `"ChatGPT sign-in returned an invalid ID token."` (specific cause logged server-side); nonce/sub specifics live in the log, mirroring v2's cause-attachment. Call site `providers_enterprise.rs:1226` becomes async (already inside async block).

- [ ] Step 1: failing tests: generate committed PKCS#8 RSA-2048 test key fixture; local hyper/reqwest test server serves JWKS + lets tests forge each failure case (valid / wrong iss / wrong aud / expired / missing exp / missing sub / blank sub / wrong nonce / forged signature / unknown kid then key-rotation after cooldown / alg=none+HS256 rejected). Expected: FAIL (no signature verification yet).
- [ ] Step 2: implement jwks.rs + verification; run via dev-gate; tests PASS.
- [ ] Step 3: update module docs + the deferral comment at :1252-1255; run affected crate `cargo nextest -p oc-core` + clippy + fmt; commit.

### Task 2: Lane SKILL-URL — skill URL source download channel

**Files:** Create `crates/oc-core/src/engine/skills_url.rs`; Modify `crates/oc-core/src/engine/skills.rs` (`:292-296` URL skip → proper URL parse), engine config-load path (async pull before `ConfigSkillTable::discover`), `skills.rs` module docs.

**Interfaces:** `pub async fn pull_url_sources(entries:&[String], cache_root:&Path) -> Vec<PathBuf>` porting v2 `discovery.ts`: only `URL::canParse` + `http(s)://` are URL sources (everything else falls to the existing directory path); GET `{base}/index.json` every pass → `{skills:[{name,version?,files}]}`; per-file GET; cache `<cache_root>/skills/<sha1(base)>/<name>/`; eligibility SKILL.md|`{name}.md`; guards `isSafeSegment`/`isSafeRelativePath` + same-origin + containment; 2 retries ~200ms jittered; concurrency ≤4 skills/≤8 files; version stamp `.opencode-version` (same version ⇒ no file downloads; changed ⇒ tmp-dir staging + atomic swap + rollback); index failure ⇒ source yields nothing (log, no offline fallback). sha1 via `ring::digest::SHA1_FOR_LEGACY_USE_ONLY`.

- [ ] Step 1: failing tests w/ local test server: manifest download; version-gated skip (assert request counts); version-change atomic swap; traversal/absolute/cross-origin/`..`-segment rejected before fetch; index failure → empty; non-http scheme → directory semantics unchanged.
- [ ] Step 2: implement; wire async pull into the engine config-load/boot seam (locate `ConfigSkillTable::discover` callers; pull BEFORE discover, pass returned dirs as extra sources); dev-gate affected tests; PASS.
- [ ] Step 3: docs + comment updates; affected-crate nextest + clippy + fmt; commit.

### Task 3: Lane CONSOLE — capability/cost/limit + baseURL overlay merge

**Files:** Modify `crates/oc-core/src/integration/console.rs` (decode `ConsoleModel`/`ConsoleProvider` :45-97, `to_custom_providers` :208-250), `crates/oc-core/src/provider_catalog/facade.rs` (`CustomModel`/`CustomProvider` :131-154), `crates/oc-core/src/engine/resolver.rs` (`custom_model_info` :713-735).

**Interfaces:** Port v2 `opencode.ts:245-315` merge rules exactly: console **cost REPLACES** model cost (single-or-array normalized to array; missing cache read/write → 0.0); **limit per-field override** (context/input/output); **capabilities per-field** `{tools,input,output}` over base = catalog model caps else defaults `{tools:true,input:[text,image],output:[text]}`; per-model `settings.baseURL` overlay with precedence console-model > console-provider > base; provider-level baseURL **deletes** inherited base-model baseURL (opencode.ts:273); decode key fix `"package"` (currently reads `"type"` — keep `"type"` as accepted fallback for old payloads); decode `family`,`compatibility`,`variants`,`disabled`. Catalog-base lookup in `custom_model_info` by modelID/family when the console provider has a canonical/catalog match; unavailable base ⇒ defaults (current behavior) with fields now populated from console config alone.

- [ ] Step 1: failing tests over decode+projection+`custom_model_info`: cost replace (single/array/cache-zero-default), limit partial, caps partial+default fallback, baseURL precedence incl. provider-deletes-inherited, `package` key, disabled filter unchanged.
- [ ] Step 2: implement; dev-gate affected tests; PASS.
- [ ] Step 3: update deferral comments (console.rs:208-211); affected-crate nextest + clippy + fmt; commit.

**Phase 1 gate:** @oracle review of the three lanes' semantics vs v2 citations before Phase 2.

## Phase 2 — Divergence + verification lanes

### Task 4: Lane E12 — live-ref recheck + dispositions
Stage a ref sandbox (COPY /opt/opencode-ref → /tmp/opencode/<pid>-refsbx; bun install with pinned bun; NEVER mutate /opt/opencode-ref), boot ref TUI + our TUI each in owned PTY harness, run `tools/parity/spec-canaries/e12_search_corpus.json` queries, record F1–F6 empirical outcomes (accent café/cafe, NFD, whitespace collapse, empty/whitespace query, child rows, fuzzy zln). Our side already pins source-read semantics (`dialog_select.rs:273-290`, `dialog_open.rs:504-511`, `session_list_filter_six_semantics` app.rs:7946) — the live run settles F2/F3/F5 semantics + F4 decision (substring, per ref source). Deliverable: disposition table written into docs/GAPS-AND-PITFALLS.md §1.2 + ROADMAP §3 ledger + any bug fix if the live ref contradicts our pin. One-shot semantic confirmation, NOT a reinstated differential loop.

### Task 5: Lane E2 — boot-stretch temporal gate
`crates/oc-tui/tests/vt_temporal.rs`: new test stretching boot (delayed first server response ≥500ms) asserting the StartupLoading overlay renders after the 500ms display threshold and escalates at the 3s pin (thresholds `store.rs:9303-9335`, overlay `overlays.rs:131-142`; unit pin `overlays.rs:194-212` stays). Real-time vt harness, serialized profile already in ci.yml:140.

### Task 6: Lane SSE — deterministic chunk-split test
`crates/oc-client/tests/transport.rs:362` `sse_decoding_multi-data_heartbeats_and_crlf`: replace fixed 30ms sleeps with deterministic synchronization (flush half-event, await decoder-idle/activity assertion with timeout instead of sleep; or channel-injected mock stream). Preserve characterization: cross-chunk buffering, heartbeat skip, id/event fields, per-chunk onActivity.

### Task 7: Lane F15 — option-row height model (narrow-width wrap)
Rework `dialog_select.rs` `render_select` (:664-700) one-row-per-option model to wrapped option rows at narrow widths, 1:1 with ref `dialog-select.tsx` (spec the ref height/wrap model from source first — file:line citations; opentui TextBuffer wrap semantics). Selection movement, viewport centering, hit-testing account for multi-row options. Verification = unit + PTY at 80/40 cols with long titles + golden additions; NOT golden-only (composition-model class).

**Phase 2 gate:** @oracle review.

## Phase 3 — Structural lanes

### Task 8: Lane DBMIG — pre-2.0.24 upgrade battery
Locate the 48 translated migrations; add a non-OC_P10 test battery: (a) stepwise 1→head on empty DB ≡ direct-latest schema (schema hash compare); (b) canary-row seeding at key schema boundaries (pick migration steps that add/rename tables-columns), upgrade to head, assert canary data readable + shapes preserved; (c) document the battery in MIGRATION.md. Runs in CI (no external artifacts).

### Task 9: Lane PTYD — PTY daemon process boundary (Phase 9)
Design doc `docs/superpowers/specs/2026-10-23-pty-daemon-design.md` first (bin layout, socket+framed `{token,request}` protocol mirroring `daemon.ts` ProtocolVersion 7, service.json/lock lifecycle, ownership transfer making the `Owned` adoption state reachable, child keepalive, real checkpoint serializer format, `OPENCODE_PTY_BIN` spawn/fallback policy, in-process one-release fallback) → **@oracle design gate** → implement D1 daemon bin, D2 client conversion, D3 ownership-transfer + process-level integration tests (spawn daemon; child survives server kill; adopt restores replay; checkpoint round-trip). TUI/PTY tests only under harness; own child handles only.

**Phase 3 gate:** @oracle review (PTYD gets its design gate + phase gate).

## Phase 4 — CI + platform

### Task 10: Lane R10 — rmcp elicitation (pending librarian scoping)
rmcp 0.5.0 → current (3.6.0) upgrade with client-side elicitation wired (pending Task 0 librarian report on API + breaking changes; if the jump is too large for one lane, split 0.5→2→3 with per-step workspace gates).

### Task 11: Lane CI — automation + platform matrix (R-11)
`.github/workflows/runtime-gates.yml`: add weekly cron trigger (self-staging jobs already exist: v2-db-fixture :217, p10-battery :436, runtime-gates :607, omo staging :705-706); keep known-reds pinned. `release.yml` + new/extended platform-build: aarch64-unknown-linux-gnu, aarch64-unknown-linux-musl, x86_64-pc-windows (build-only where tests infeasible; macos job already exists :1123). Update RELEASE.md matrix table. CI is dev-gate-exempt (off-machine).

**Phase 4 gate:** @oracle review.

## Phase 5 — Consolidated close

- Full gate via dev-gate: fmt, clippy `-D warnings`, workspace nextest (expect 3951+new green), pty profile, release build + budgets; runtime-gates workflow dispatched on GitHub.
- Docs sync: ROADMAP ledger (R-09 Phase-9 done, R-10 disposition, R-11 done, JWKS/skill/console done, E12/E2/SSE/F15/DBMIG closed), GAPS-AND-PITFALLS §1 rewrite, PORT_REPORT/README as needed.
- Final @oracle review → summary to user.
