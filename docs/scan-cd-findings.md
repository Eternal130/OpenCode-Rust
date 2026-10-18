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

### CD-12 (HIGH) — C14(b): markdown + config command discovery absent; `/api/command` hardcoded empty
- **Evidence (I1 live)**: project with `.opencode/command/md-cmd.md` AND config `commands.cfg-cmd` → `GET /api/command` = `[]`. Handler: `oc-server/src/handlers/agent.rs:55-64` — "Commands are plugin-registered (TODO(phase-9)…)"; no `command_md.rs` exists in `oc-core/src/config/` (agent_md.rs and instruction.rs do exist).
- **Ref**: `{command,commands}/**/*.md` discovery (`core/src/config/plugin/command.ts:159`, v9 census verified this round) + config `commands` map → `command.list` returns both.
- **Effect**: every slash-command surface (catalog/frecency UI cell of C14(b), composer autocomplete) has zero data on ours. Config-declared commands are normalized (normalize.rs:320-339) but never surfaced — half-wired.

### CD-13 (HIGH) — C14(a): global AGENTS.md read from the wrong directory (`.config/opencode-rust/` not `.config/opencode/`)
- **Evidence (I1 live, positive control)**: AGENTS.md in BOTH `home/.config/opencode/` (canonical, what every other config consumer reads — handlers/config.rs:28, boot.rs:122) and `home/.config/opencode-rust/` → materialized system prompt contains ONLY the block from the **suffixed** dir. Source: `engine/mod.rs:890-894` builds the global-file path with `oc_util::global_roots::APP_NAME` ("opencode-rust", global_roots.rs:29).
- **Ref**: `config/plugin/instruction.ts:38` = `{global.config}/AGENTS.md` — the same dir as opencode.json (XDG opencode).
- **Effect**: a v2-shaped install (global AGENTS.md at the documented location) silently loses ALL global instructions. Cross-install visibility class.

### CD-14 (INFO) — C14 verified-parity rows
- **(a) hierarchy (live)**: git-rooted project, AGENTS.md at root/deep/sub + global(suffixed) → materialized system prompt order = global → subdir → deep → project (nearest-to-farthest after global) — matches ref render order (instruction.ts walk + our port's cited parity); dedup via resolved-path `seen` ✓; per-session `session.synthetic` exactly-once ledger ✓; non-git project root correctly scopes the walk to the location dir.
- **(b) agents (live)**: `.opencode/agent/md-agent.md` + `.opencode/mode/md-mode.md` both discovered, ids from relative path ✓; `mode` default "primary" both sides (ref fills `Info.default` mode:"primary" via `update()`; the legacySources primary:false only forces mode/ files — verified ref agent.ts:177-210 + core/src/agent.ts:69-82 — NOT a divergence); unknown frontmatter key triggers legacy-v1 migration into `request.body` (ref `legacy = some(!agentKeys.has(key))` → decodeLegacyAgent+migrateAgent — same chain) ✓; collision (markdown vs config `agents.md-agent`) → markdown wins, ref-equivalent by document ordering (synthetic .opencode-dir docs load after the project's direct opencode.json; findLast precedence).
- Prompt-copy note: `prompt` has no file-discovery source in ref (v9 census re-verified: `grep -n 'Prompt' …/{command,agent}.ts` = 0) — MCP prompts are C9/C20 cells (carried).

### CD-15 (MED) — D1: `session.forked` published without the `location` envelope field
- **Evidence (I1 live both legs, same flow)**: ours `{"id","created","type","durable","data"}` vs ref-binary 2.0.25 `{...,"location":{directory}}` — the ONLY envelope diff on the only diverging shared type. Every other shared event type matched envelope+payload key sets exactly (`/tmp/opencode/scancd/d1/fielddiff.py` over ourssse.log vs refsse2.log; 30 vs 31 types; run-asymmetry rows adjudicated below).
- **Effect**: location-keyed SSE consumers (per-directory routing in webui/session-ui class) misroute fork events. Narrow fix: publish with location like every other session event.

### CD-16 (LOW) — D1: reasoning emission is lazy on ours, eager on ref (openai-compat)
- **Evidence**: content-only stream → ref emits `session.reasoning.started{state:{reasoningField}}`+deltas+ended even with NO reasoning content; ours emits nothing. Positive control (mock sends `reasoning_content`): ours emits all 3 events AND stores the reasoning part `{state:{reasoningField:"reasoning_content"},text,type:reasoning}` — payload-shape exact vs ref. Divergence = eager-open vs lazy-open of the reasoning fragment (timing/visibility cell; end-state identical).

### CD-17 (INFO) — D1 verified-parity rows
- **Field-level universe (live, both legs, same scripted flow)**: envelope `{id, created?, type, location, data, durable?{aggregateID,seq,version}}` + `: heartbeat` keepalives on both; payload key sets identical on every shared type (session.created/inbox.enqueued/inbox.delivered/execution.started/step.started/step.streamed/text.started/text.delta/text.ended/step.ended/step.failed/usage.updated/instructions.updated/forked); boot registry snapshot events identical on both (server.connected + agent/command/integration/model/plugin/project/provider/reference/skill/websearch .updated — ours under OPENCODE_CATALOG_FETCH=0 suppressed only `models-dev.refreshed`, which ours implements: provider_catalog/refresh.rs).
- **Error-injection**: mid-stream provider abort → ours `session.step.failed` + retry path + `session.execution.interrupted{reason}` on interrupt (live); ref leg produced `session.execution.failed{error,sessionID}` — our schema `ExecutionFailed{sessionID, error}` field-exact (`oc-schema/src/session_event.rs:236-242`). Provider-unreachable leg (C1 first run): outcome=failed + assistant error part `{type,message}` ✓.
- **Ordering guarantee**: within-aggregate durable `seq` strictly monotonic on ours across the full capture (2 aggregates, 0 violations).
- **TUI consumer**: store.rs ingest arms exist for every streaming type (text/reasoning/tool/step/inbox — enumerated during C1); webui consumer rides D6 (carried).

### CD-18 (I8) — D1 canary executed: v1 FAIL → oracle redesigned → PASS (recorded per the on-the-spot rule)
- Canary A (adjacent same-type event reorder) was INVISIBLE to a type-sequence ordering check — the method's blindspot; redesigned to instance fingerprints (type + payload digest): 2 mismatched positions detected. Canary B (optional `location` drop) flagged by the field-diff. Final: **I8-D1 PASS** with the fingerprint oracle; the v1 oracle is recorded as insufficient (an agent re-running D1 must use instance fingerprints, not type sequences).

### CD-19 (INFO) — D5 verified-parity rows + CD-2 scope refinement
- **Invariant checker** (`/tmp/opencode/scancd/d5/invariants.py`) over a live 242-session stress corpus (1 parent + 241 subagent children, 964 messages, full SSE): ZERO violations on message→session, session→project, per-directory project consistency, SSE durable-aggregate→session, inbox→session, instruction_state→session, per-session messageID uniqueness, fork_session_id orphans.
- **CD-2 refined**: subagent children carry `parent_id` 241/241 ✓ (the subagent creation path is correct); ONLY the fork projector hardcodes NULL — the bug is fork-scoped, not chain-wide.
- **Subagent id chain (live, FIX5 area)**: parent tool part embeds `<subagent sessionID="ses_…" state="completed">` with the child's real id ✓; `session.tool.progress` emitted live (240×) ✓; child job completion delivered into the parent (subagent_job notify keyed `{child}:{startedAt}`, exactly-once set) ✓ code-verified.
- **Step-cap semantics = ref**: cap applies only when `agent.steps` is set (ours drain.rs:541 ↔ ref llm.ts:223 identical); a tool-looping model with steps unset loops unbounded on BOTH (my instrument's 241-loop reproduces ref semantics — I6/I9: instrument artifact, not a divergence; recorded because quota-burn is a real shared property, dispositioned ref-equal).
- LOW-note: subagent child sessions get empty titles (`title: ''` in DB); ref derives child titles from the prompt (title.ts) — carried to a UI lane if visible.

### CD-20 (MED-HIGH) — C2: 21 ref commands declared in the keymap table with NO dispatcher arm (silent dead keybinds)
- **Evidence (mechanical census)**: `oc-tui-core/src/keymap/table.rs` declares each with its binding; `command_palette.rs` dispatcher (76 arms) + full-tree grep find NO handler: `session.parent, session.pin.toggle, session.copy, session.unshare, session.toggle.scrollbar, session.toggle.thinking, model.dialog.favorite, app.toggle.animations, app.toggle.diffwrap, permission.prompt.fullscreen, plugins.toggle, prompt.images.view, terminal.suspend, terminal.title.toggle, prompt.stash, prompt.stash.list, prompt.stash.pop, stash.delete` (+ CD-3's 4 `session.message.*`). Census table: `keymap-table=1 dispatcher=0` ×17 rows + message-nav rows (`/tmp/opencode/scancd/c2/` outputs inline in log).
- **Adjudication**: `prompt.history.next/previous` also dispatcher-dead BUT the feature exists composer-internally (PromptHistory with draft-stash, composer/mod.rs:132-181) — cosmetic only. The rest have NO feature substitute (grep-verified): family-up navigation, tab pinning, scrollbar/thinking toggles, animations/diffwrap toggles, permission fullscreen, plugins toggle, image viewer, terminal suspend/title, **the whole persistent prompt-stash family** (ref `prompt/stash.tsx` + `dialog-stash.tsx`; ours has only the in-composer draft stash — also a C16 cell), model-dialog favorite.
- **I6**: the keymap parity test guards TABLE parity only — it masks the missing actions (fixture-happy guard class).

### CD-21 (INFO) — C2 verified-parity rows
- Dialog graph: 29 DialogKind variants, ZERO unreachable (all referenced; mechanical comm of defined vs used).
- Tabs: session.tab.{close,next,previous,reopen,history.back,history.forward,previous_unread} all dispatched ✓; picker family (session.list/model.list/agent.list/command.palette) ✓; session.child.first handled in app.rs ✓ (session.parent dead per CD-20).
- Grouping/tree: `session.grouping` state persisted (store.rs:1723), `session.toggle.exploration_grouping` dispatched ✓, group-view row rendering ported (components/session.rs citing group-view.tsx lines).

*(next: C3)*
### CD-22 (INFO) — C3 verified-parity rows
- **Literals (census, v8 pin re-run)**: Effect {allow,deny,ask} (permission.ts:55) and Reply {once,always,reject} (:41) — ours field-exact (`oc-schema/src/permission.rs:11-19,63-69`). HTTP reply field = `decision` on BOTH (protocol/groups/permission.ts:120 ↔ handlers/permission.rs:222).
- **Live round-trip (I1)**: config rule `{action:"shell",resource:"*",effect:"ask"}` → tool call → `permission.asked{action,id,resources,save:["echo *"],sessionID,source{messageID,call id}}` → `POST …/reply {"decision":"always"}` → `permission.replied` + tool executed (shell exit 0) + **grant persisted** (`/api/permission/saved` row {action,resource:"echo *",projectID} + `permission` DB table) + subsequent identical calls auto-allowed without re-ask (single asked event, N successful executions).
- **Unknown-tool guard**: a model call to a nonexistent tool fails cleanly ("No tool named … is currently available") — ref-guard present.
- **Attention census**: notification protocols osc9/osc99/osc777 + system-notify matrix + in-process sounds (volume/sound_pack defaults 0.4/"opencode.default" = ref) + terminal-bell fallback with sound-suppress rule (attention.rs/os_notify.rs/sound.rs); gating `notifications/sound` config keys match ref defaults (false/false). Timing cells (bell cadence) ride I3 and are unit-tested in attention.rs (declared in-module).
- Partial: reply=once/reject legs and multi-ask queueing not separately driven (grant-persistence contrast covers once-semantics; queue surface = the permission list API, exercised for the single ask). Carried as C3-residual if a fix lands in the family.

*(next: C5)*
### CD-23 (MED-LOW) — C5/C16: `DialogWorkspaceFileChanges` exists only in its delete-confirm role
- Ours models the dialog solely as `WorktreeDeleteConfirm` (store.rs:124) after a `forceRequired` rejection. Ref's `dialog-workspace-file-changes.tsx` is the full changed-files REVIEW flow (list + review before adopt/move). Ours = partial (confirm-only); the review-list role is absent.

### CD-24 (LOW, UNCERTAIN — instrument anomaly, recorded for follow-up) — C5/E-class: data-root fallback to `/.local/share/opencode-rust` under HOME-unresolvable conditions
- Two MANUAL server invocations (identical env exports to the scripted drives: `OPENCODE_TEST_HOME` + full XDG_* set) wrote their data to `/.local/share/opencode-rust/opencode.db` (the XDG error-path root) and loaded the user-global plugin config, while ALL scripted drives isolated correctly per-home (per-probe DBs verified distinct). Exact trigger unisolated (shell env scoping vs root-resolution precedence — boot.rs:359's redirect seam is the suspect surface). No user data touched (the /.local DB contains only this lane's sessions). Flagged as a precedence-audit TODO, not adjudicated a product bug from this evidence alone.

### CD-25 (INFO) — C5 verified-parity rows
- **Server loss (live, kill -9 mid-SLOWSTREAM + restart)**: session row intact (no false terminal: `outcome=None` until a real settle — ref semantics: outcome only at terminal transitions); messages readable including the partial assistant; `active` set EMPTY after restart (no stuck execution ownership); new prompts admitted post-recovery.
- **Bad data (live)**: bogus session id → 404 `SessionNotFoundError` shape; invalid prompt body handled without crash; corrupt config JSON (`{'shell': /bin/broken}`) → server BOOTS and serves (invalid document dropped with loader diagnostics, boot survives — ref-consistent).
- **file-mutation/file-retention**: ports present with keyed lock tables (`file_mutation.rs:149`), tests inline.
- Carried: partial-write mid-state (needs fs-level fault injection — no standing instrument); plugin-crash recovery folded into C6 (same supervision surface).

*(next: C9, C13)*
### CD-26 (MED) — C9: cold-start MCP race — zero rows for seconds after boot
- **Evidence (I1 live, repeated)**: fresh home + config-mcp {c9echo(ok), c9bad(dead)} → `GET /api/mcp` returns `[]` at t+2-4s, rows appear ~t+5s (c9bad failed with os-error detail ✓, c9echo connected ✓). Ref registers the config-derived ROW synchronously (status transitions pending→connected async); ours defers even row registration to the async connect, so listing consumers (sidebar MCP section, DialogStatus) see nothing then a jump.
- Same window affects first-request tool visibility (snapshot before bridge sync).

### CD-27 (LOW) — C9: connect/remove/disconnect map EVERY error to 404 `McpServerNotFoundError`
- `handlers/mcp.rs:73-74, 84-86, 89` — `map_err` collapses spawn failures/timeouts into "not found" (a connect failure of an existing server reads as a missing server). Ref surfaces connect errors distinctly (status row error detail — which ours DOES show in rows once registered).

### CD-28 (LOW) — C9: ours accepts a flat `mcp: {<server>: cfg}` map ref rejects
- Ref `Config.MCP.Info = {timeout?, servers: Record<id, Server>}` (schema/config/mcp.ts:17-20) — nested only. Ours normalized+registered my flat probe map (both servers live). Lenient-shape acceptance (ours-extra; silent divergence for shared configs).

### CD-29 (INFO) — C9 verified-parity rows
- **Live round-trip**: config mcp local server (stdio JSON-RPC) → boot auto-connect → status events (`mcp.status.changed`/`tools.changed`/`resources.changed`/`prompts.changed`, ref type set) → `GET /api/mcp` rows {name,status{status,error},integrationID} → `POST …/connect` 204 (post-warmup) → `DELETE` remove 204 → disconnect 204. Dead-server fail path carries the spawn error string in the row ✓.
- **MCP tool exposure = Code Mode design, NOT a gap** (instrument correction recorded): with `execute` registered, `shape_request_tools` (tool/codemode.rs:91-107) filters flat MCP tools from the request and exposes them via `execute` under `tools[<server>]` (the CODEMODE catalog instruction) — matches ref's Code Mode architecture; the bridge registers namespaced `<server>_<tool>` + executors into the live registry (mcp/registry.rs:1404-1432), debounced refresh on `mcp.tools.changed`.
- Carried: needs-auth (OAuth MCP) + elicitation-blocked live cells (elicitation.rs port present; needs a real authenticated MCP server — no standing instrument; code-level verified only).

*(next: C13)*
*(execution continues — second tier: C13/C15/C16/C20/D2/D3/D4)*





