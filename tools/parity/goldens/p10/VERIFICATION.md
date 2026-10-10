# Phase-10 parity verification battery — report ledger

Run as `eternal130`, 2026-10-10. Reference = pinned v2.0.24 worktree
(`/opt/opencode-ref`, executed via bun from `/tmp/opencode/ref`);
port = `oc-server`/`oc-client` of this repo (debug build,
`CARGO_TARGET_DIR=/tmp/opencode/target-parity`). The harness never binds
49374 and never touches live service state (scrubbed, sandboxed `HOME` per
child).

Invocation (all five tests are `#[ignore]`d **and** runtime-gated on
`OC_P10=1`, so the default suite is unaffected):

```bash
source $HOME/.cargo/env
export CARGO_TARGET_DIR=/tmp/opencode/target-parity
OC_P10=1 cargo nextest run -p parity --run-ignored only -E 'test(p10_)'
```

## 1. Battery table

| # | Test (binary) | Verifies | Evidence | Status |
|---|---------------|----------|----------|--------|
| 1 | `p10_openapi_semantic_equivalence` (`tests/p10_openapi.rs`) | Boot the Rust server, `GET /openapi.json`, semantic-diff vs the pinned `/opt/opencode-ref/packages/protocol/openapi.json`: path×method sets, per-op operationId/parameter shapes/`$ref` sets (params, requestBody, per-status responses), `components.schemas` names | `openapi-summary.json`; whitelist `openapi-whitelist.json` | **PASS — 0 differences** (117 paths / 141 operations / 256 schemas; served document **byte-identical** to the pinned file, sha256 `06124183…73ee`; whitelist EMPTY) |
| 2 | `p10_error_path_goldens_side_by_side` (`tests/p10_error_paths.rs`) | 7 representative error routes hit on a booted reference AND a booted port side-by-side; normalized bodies byte-compared (canonical serialization of the normalized shapes) | `error-paths.json` (pins both sides + divergence per route) | **PASS — 7/7 byte-equal** (P10-E1/E2/E3 closed by the remediation sweep; the test fails on any NEW divergence) |
| 3 | `p10_ts_tui_against_rust_server` (`tests/p10_cross_tui.rs`) | Cross-replacement (a): the REAL v2.0.24 TUI (bun in tmux, m1 pattern) drives the Rust server through 3 interactions: prompt+reply via `FakeLlm`, session rename (`ctrl+r` dialog → server row), permission-once (external read → "Allow once" footer → TUI POSTs the reply → tool proceeds) | pane captures + server rows in `tui/` | **PASS — 3/3 interactions** |
| 4 | `p10_rust_client_against_reference_server` (`tests/p10_cross_client.rs`) | Cross-replacement (b): the generated `oc-client` (in-process) drives a booted REFERENCE server: `session.create` → `session.prompt` → parked `external_directory` ask → `permission_list`/`permission_reply(once)` → `message.list` — every response decoded into the wire-faithful types | `evidence/client-messages.json` | **PASS — full flow decoded** |
| 5 | `p10_db_interop_v2_written_null_rows` (`tests/p10_db_interop.rs`) | One shared data home, three boots: v2 writes NULL-heavy rows (A: model/agent/parent all NULL; B: model switched; C: child of A) + a prompt exchange → port re-opens (journal byte-identical = **0 reruns**), reads all rows, continues (new prompt on B, second model switch on the v2 row, prompt on the NULL-model session A) → v2 re-opens, reads the port's writes, prompts again | `evidence/db-dump-phase-{a,b,c}.json`; manifest `db-migration-manifest.json` | **PASS — D3 column class exercised both directions on real v2 rows; journal stable at 48 across all three phases** |
| 6 | `p10_db_interop_stable_channel_same_file` (`tests/p10_db_interop.rs`) | **P10-DB1 stable-channel lane**: no `OPENCODE_DISABLE_CHANNEL_DB` on either side — the reference build (baked channel `stable`) and a Rust boot with `OPENCODE_CHANNEL=stable` interoperate on the SAME channel-suffixed `opencode-stable.db` (v2 writes A, port reads A + writes B, v2 re-reads both; journal 48, no unsuffixed file created) | run logs | **PASS** |

Battery result: **5/5 PASS**, re-run **3× total** (initial individual runs + 2
full battery runs) — no flakes observed.

## 2. OpenAPI delta status

**EMPTY.** The semantic difference list between the served document and the
pinned v2.0.24 protocol document is `[]`; the whitelist
(`openapi-whitelist.json`) is empty and no stale entries exist. The served
document is additionally byte-identical to the pinned file (the server
includes the pinned `tools/openapi-gen/openapi.json`, sha-guarded by
`oc_server::routes_gen::OPENAPI_SHA256`).

## 3. Findings — CLOSED by the final remediation sweep (2026-10-10)

- **P10-E1 [CLOSED]** — payload schema validation: the port now validates
  `session.create` payloads through `oc_server::handlers::schema_check`
  (the Effect `TreeFormatter` message shapes, verified field-by-field
  against a live reference boot: `Expected string | null\n  at
  ["title"]`, annotated-schema labels like `Model.Ref`, nested paths,
  first-field-wins, unknown fields ignored). `POST /api/session
  {"title":12345}` → 400 byte-equal.
- **P10-E2 [CLOSED]** — query schema validation: same module validates
  `session.list` queries (`limit`: JS-`Number()` semantics → Int →
  positivity; `order`: literal union). `?limit=notanumber` → 400
  byte-equal.
- **P10-E3 [CLOSED]** — method mismatch: the dispatch rewrites axum's
  405 to the empty-body 404 v2's Effect router answers. `DELETE
  /api/info` → 404 empty byte-equal. The web-UI asset 405 (GET/HEAD
  only) is a separate, intentional seam.
- **P10-DB1 [CLOSED]** — the engine's database path now goes through
  `oc_core::database::engine_location` (the port of
  `database-path.ts`): channel latest/dev/beta/next/prod →
  `opencode.db`, else `opencode-<sanitized>.db`, with
  `OPENCODE_DISABLE_CHANNEL_DB` and `OPENCODE_DB` honored. The channel
  comes from the ONE workspace source `oc_util::channel::current()`
  (`OPENCODE_CHANNEL`, default `latest`) — `oc-cli serve`, the engine,
  and channel-scoped v2 builds agree. The battery keeps the
  shared-file lane (`OPENCODE_DISABLE_CHANNEL_DB=1`, regression) and
  adds the stable-channel lane (`p10_db_interop_stable_channel_same_file`):
  the reference build (baked `stable`) and an
  `OPENCODE_CHANNEL=stable` Rust boot interoperate on
  `opencode-stable.db` — v2 writes, the port reads + writes, v2 re-reads.
- **P10-N1 [note, harness, still open]** — non-JSON-body 400s diverge in
  shape (v2: `InvalidRequestError` envelope "Expected a valid JSON
  body"; port: axum's plain-text "Failed to parse the request body as
  JSON") — observed during route scouting, same class as P10-E1, not
  separately pinned.

## 4. Route panel (test 2) — all seven byte-equal

| case | request | reference | port | equal? |
|------|---------|-----------|------|--------|
| unknown_session_404 | `GET /api/session/ses_UNKNOWN…` | 404 `SessionNotFoundError` | same | ✅ byte-equal (normalized) |
| invalid_cursor_400 | `GET /api/session?cursor=@@invalid…` | 400 `InvalidCursorError` | same | ✅ |
| unauthorized_401 | `GET /api/info` (no auth) | 401 `UnauthorizedError` | same | ✅ |
| unknown_route_404 | `GET /api/nonexistent-route` | 404 empty | same | ✅ |
| payload_schema_reject_400 | `POST /api/session {"title":12345}` | 400 `InvalidRequestError` kind=Payload | same | ✅ (P10-E1 closed) |
| query_schema_reject_400 | `GET /api/session?limit=notanumber` | 400 kind=Query | same | ✅ (P10-E2 closed) |
| method_mismatch_delete_info | `DELETE /api/info` | 404 empty | 404 empty | ✅ (P10-E3 closed) |

(Readiness gating is covered by the existing suite, not re-pinned here.)

## 5. Gate status (as eternal130)

- `cargo fmt -p parity -- --check` — **clean**
- `cargo clippy -p parity --all-targets -- -D warnings` — **clean**
- default suite `cargo nextest run -p parity` — **85/85 passed** (8 skips =
  the opt-in tests), i.e. the default suite is unaffected by the battery.

## 6. Final remediation sweep re-verification (2026-10-10, eternal130)

- Battery re-run after the P10-E1/E2/E3 + P10-DB1 fixes: **6/6 PASS**
  (the five original tests + the new stable-channel DB lane).
- The shared-file DB lane (OPENCODE_DISABLE_CHANNEL_DB=1) re-ran clean
  (0 reruns, journal 48 across all three phases) — no regression from the
  engine path change; the default-channel engine boot still opens
  `opencode.db` (channel `latest` is in the shared list).
- `oc-util`/`oc-core`/`oc-server`/`oc-cli`/`parity`: fmt + clippy
  `-D warnings` clean; workspace nextest green.
