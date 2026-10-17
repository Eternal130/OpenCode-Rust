# SCAN-CD findings — first execution of scan-spec v9 (LOCKED), axes C1-C22 + D1-D11

- **Lane**: SCAN-CD (worktree `lane/scancd` @ `5bcf7f6`, main HEAD of v9-LOCKED spec commit)
- **Ref**: `/opt/opencode-ref` (I10b 35-package pin, verified) + I2b binary `/root/.opencode/bin/opencode` (2.0.25)
- **Instrument binds honored**: I1 (real oc-server + real child ids; only the upstream LLM is a scripted OpenAI-compatible mock — I9: instrument, declared, never sole evidence), I3 (not C/D-binding except timed behaviors), I5 (standing instruments cited where used), I6 (attribution per finding), I9 (provenance per artifact), I10/I10b (pins above).
- **Severity scale**: HIGH = behavioral divergence on a real flow; MED = partial/degraded parity or dead-wired surface; LOW = hygiene/dead-code/doc-drift; INFO = verified-parity evidence rows.

## Findings register

### CD-1 (HIGH) — C1/D2: `POST /api/session/:id/prompt` blocks until run settlement
- **Cell**: C1 lifecycle trigger→UI-effect; D2 request semantics.
- **Evidence (I1 live)**: with a held-open provider stream (SLOWSTREAM instrument), the prompt POST did not return for ≥90 s (`/tmp/opencode/scancd/c1/` runs; prompt-resp only arrives after stream end). Handler: `crates/oc-server/src/handlers/session.rs:586-592` — `services.execution.resume(&session_id).await` where `resume()` = "Starts execution while idle or **joins the active execution** and returns its exit" (`oc-core/src/session/execution.rs:216-217,646-649`).
- **Ref**: `server/src/handlers/session.ts:308-321` calls `session.prompt(...)` → `core/src/session/session.ts:174` `if (input.resume !== false) yield* execution.wake(sessionID)` — `wake` is fire-and-forget ("Registers newly recorded work. Repeated wakeups may coalesce", `execution.ts:24`); the HTTP response returns the Inbox info immediately.
- **Effect**: a slow/hung provider stalls the HTTP response; clients batching on this API serialize. (Ref TUI does not rely on the wait; any parity consumer would observe ours hanging.)
- **I6 attribution**: inventory-grade (found by real-flow probe, not fixture).

### CD-2 (HIGH) — C1/D5: fork children never persist `parent_id`
- **Evidence (I1 live + code)**: fork child session row returns `"parentID": null` while `fork.sessionID` is set (live probe home4; DB row `parent_id` NULL). Cause: `oc-core/src/session/projector.rs:702-704` — the fork INSERT's VALUES hardcodes `NULL` for the `parent_id` column (`VALUES (?1, ?2, ?3, NULL, ?4, …)`), so the bound `parent_id` param lands in `fork_session_id` (column shift is internally consistent, but the parent link column is dead).
- **Ref**: `core/src/session/projector.ts:446` inserts `parent_id: event.data.parentID`; ref children carry BOTH `parentID` and `fork{sessionID,boundary}` (`schema/src/session.ts:33-37`).
- **Effect**: every parent→child chain consumer (session.child navigation, grouping tree, D5 id-chain invariants) sees no parent link. UI-effect: ref TUI session.child/family navigation can't materialize on ours.
- **I6**: inventory-grade.

### CD-3 (MED) — C1/C2: message-navigation keybinds declared but dead
- **Evidence**: keymap table declares `messages_previous/next/_user` → commands `session.message.{previous,next,user.previous,user.next}` (`oc-tui-core/src/keymap/table.rs:681-683`, matching ref `tui/src/config/v1/keybind.ts:355-356`), but the command dispatcher (`oc-tui/src/components/command_palette.rs:1609`, 76 arms) has NO arm for any `session.message.*`; grep over `crates/oc-tui/src` finds no other handler. Ref implements them (`tui/src/routes/session/index.tsx:1108` + `message-navigation.ts`).
- **Effect**: pressing the bound key is silently swallowed (Dispatch::Unknown). Declared-binding-no-action class.

### CD-4 (LOW) — C1 hygiene: invented event name `session.execution.resumed`
- `oc-tui/src/mini/transport.rs:140` matches `"session.execution.started" | "session.execution.resumed"` — `execution.resumed` exists in NEITHER ref (grep `/opt/opencode-ref/packages/` = 0) nor our own 51-event schema union (`oc-schema/src/session_event.rs`). Dead branch; name invented during porting.

### CD-5 (INFO) — C1 verified-parity rows (live, I1)
- Event-type universe: 51/51 identical (`oc-schema/src/session_event.rs` vs ref `schema/src/session-event.ts`; mechanical diff, census files `/tmp/opencode/scancd/c1/{ref,ours}-events.txt`).
- Transition table (live-driven ours): created → inbox.enqueued → execution.started → step.started → text.started/delta/ended → step.ended → execution.succeeded (+`outcome=succeeded`, `time.idle` set) ✓; mid-run interrupt → active={running} → interrupt=true → step.failed → **execution.interrupted** + `outcome=interrupted` ✓; fork → session.forked + boundary{through,messageID} + title "… (fork #1)" + messages copied (8=8) ✓ (except CD-2); rename → session.renamed ✓; view → 204 (requires JSON content-type; bare POST = 415 — ref-shaped) ✓; delete → 204 + session.deleted ✓.
- Session list anchor cursor: `cursor.next` returned with base64 JSON `{all, anchor{direction,id,time}}` (live) — matches our handler port of ref `SessionsCursor` (`server/src/handlers/session.ts:83-93`); wire-encoding parity vs ref binary deferred to D2 leg.

### CD-6 (MED) — C4: `OPENCODE_CONFIG_CONTENT` overlay declared in the Loader but unwired in production
- **Evidence**: `oc-core/src/config.rs:487` implements the "inline OPENCODE_CONFIG content (highest priority)" entry via `options.content`, but no production call site ever sets it — `oc-server/src/boot.rs:121` and `oc-server/src/handlers/config.rs:24-30` (and `boot.rs:322` `location_config`) all construct `Loader::new(...)` with default options; only `engine/tests.rs:834` exercises `content`. Live probe: `OPENCODE_CONFIG_CONTENT='{"username":"content-user"}'` + boot → `GET /api/config` shows no content document (only the global Directory entry).
- **Ref**: `core/src/config.ts:210-214` parses `OPENCODE_CONFIG_CONTENT` as a source on every load.
- **Effect**: the env overlay silently no-ops for the real server.

### CD-7 (INFO) — C4 verified-parity rows
- Leaf-key census: all 28 ref `Config.Info` top-level keys recognized by our normalize/loader (mechanical census, `/tmp/opencode/scancd/c4/census.sh`).
- Valid-shape visibility (live): compaction {auto,keep{tokens},buffer}, lsp {server command}, permissions [action/resource/effect rules], worktree {directory}, experimental {portable_shell_scanner, subagent_depth} all round-trip through boot → `GET /api/config` (home2 run).
- Shell write-path (live): `PATCH /api/experimental/config {"shell":"/bin/zsh"}` → 204; write lands in global `~/.config/opencode/opencode.jsonc` (ref's target); **JSONC comment + unrelated keys in the project file preserved** (jsonc_edit); after restart project `shell=/bin/bash` still wins (project > global precedence, ref-shaped); `username` persists.
- `OPENCODE_CONFIG` explicit file: wired (`config.rs:385-392`, every load).
- `OPENCODE_CLI_CONFIG_CONTENT` (the F-1 cli.json class): implemented in `oc-core/src/cli_config/mod.rs:466-472`; unit test `env_content_overlays_the_file_everywhere` PASS (dev-gate run, this round).
- managed-policy (U9): absent on ours — matches the spec's standing U9 disposition (not shipped by us); ref `managed-policy.ts` remains the precedence authority.

### CD-8 (CARRY-NOTE, LOW) — C4/C3: invalid `permissions` rules parse to `[]` instead of dropping/erroring
- Probe: `permissions: [{"tool":"edit","mode":"ask"}]` (invalid Rule shape) → ours reports `permissions: []` (kept-but-empty), other invalid leaves (compaction garbage) are dropped silently with no API-visible diagnostic. Ref's normalize records diagnostics; surfacing parity (toast/event) unverified — carried to the C3/D2 ref-binary leg.

### CD-9 (MED) — C10: auth integration test suite fails under default parallelism at main HEAD (3/41)
- **Evidence (dev-gate, reproduced 4×/4 in BOTH the lane worktree and the main checkout)**: `cargo test -p oc-core --lib integration::tests` (default threads) → FAILED: `gitlab_pkce_flow_round_trip_against_mock` (panic: "GitLab login needs local port 8080, but it is already in use"), `gitlab_bundled_client_rejected_off_gitlab_com` (expected rejection, got a live Attempt), `vertex_adc_binding_refreshes_and_binds_the_native_route` ("ADC binding resolves"). `--test-threads=1` → 41/41 PASS; each passes in isolation. Port 8080 free before the run.
- **Root cause (pinned)**: the GitLab OAuth flow engine pins its loopback redirect port to 8080 (`tests.rs:926-929`; flow error string in-product); concurrent GitLab-flow tests in one process collide. The bundled-client test's expectation is order-dependent on the same collision.
- **I6 attribution**: test-infra interference in freshly landed auth code (BE-3/3b class); masks real regressions in CI (any parallel run flakes) — the guard tests exist but cannot stand guard.

### CD-10 (LOW/MED) — C10: pair dialog QR block deferred on ours
- `oc-tui/src/components/command_palette.rs:986` — "the QR block deferred". Ref renders the pairing URL as a QR in `tui/src/component/dialog-pair.tsx` (CLI pair: `cli/src/commands/handlers/pair.ts`). Pairing flow itself works (CD-11); the QR affordance cell is ours-absent.

### CD-11 (INFO) — C10 verified-parity rows (live, I1; mock-upstream only where declared)
- Auth host: passwordless boot → 200; `OPENCODE_PASSWORD` boot → 401 + `WWW-Authenticate: Basic realm="Secure Area"`; basic ok/bad 200/401 (`/tmp/opencode/scancd/c10/`).
- Pairing: `POST /api/pair` → base64url code (300s TTL, cap 1000 = ref `pairing.ts` Cache params); browser `GET /auth/connect/:code` → 200 + session token + CORS echo; **reuse → 401** (single-use ✓); pair token accepted as password-equivalent (ref auth.ts:32).
- Credential store: create key + oauth (`{type,methodID,refresh,access,expires}` — ref `credential.ts` OAuth shape exact) → Entry `{id,integrationID,label,active,value}` ✓; activate enforces one-active-per-integration (switch observed) ✓; update label ✓; remove 204 ✓; events exactly `credential.updated` ×4 + `credential.switched` ×4 ✓; DB row persists (`credential` table) ✓.
- Flow engines: device flow (`request_device_code`/`poll_device_token`, grant `urn:ietf:params:oauth:grant-type:device_code`), loopback+PKCE, refresh rotation (gitlab_refresh_rotates_via_mock, zen_refresh_round_trip_against_mock, zen_device_flow_round_trip_against_mock, gitlab_pkce_flow_round_trip_against_mock — mock-IdP class per [F], I9-declared) — all green serially.
- Copilot: no-refresh by design (`providers.rs:313`, ref github-copilot.ts:126-171) ✓.

*(execution continues — C14, D1, D5 next per lane priority)*


