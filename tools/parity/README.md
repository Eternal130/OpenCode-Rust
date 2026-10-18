# tools/parity — behavior-parity harness (Rust port vs reference v2.0.24)

Compares the Rust port against the runnable reference checkout
(`/tmp/opencode/ref`, executed with bun). Everything the reference server
touches is sandboxed under `/tmp/opencode/parity-sandbox/`; the harness never
binds the managed-service port 49374 and never touches the live service state
under `~/.local/state/opencode` (the child runs in a scrubbed environment
with sandboxed `HOME`/`XDG_*`/`TMPDIR`).

## Layout

| Module | Purpose |
| --- | --- |
| `src/normalize.rs` | The canonical rewrite algorithm. **Spec:** [`NORMALIZATION.md`](./NORMALIZATION.md). Both sides of every comparison go through this one function. |
| `src/reference.rs` | `ReferenceServer`: spawn/ready/stop of the reference server on a free port in a sandboxed HOME (`spawn_with_env` for extra child env, e.g. simulation-mode experiments); `skip_if_no_bun()` test helper. |
| `src/httpdiff.rs` | Same request against two bases (`httpdiff::Side`), normalize both, `deep_diff` reports the first divergence path (e.g. `$.urls[0]`). |
| `src/sse.rs` | `SseStream`: GET /api/event subscription, `data:`/heartbeat parsing, `collect(n, timeout)`, scenario collection (`collect_scenario`: N frames / quiescence), `frames_to_json` normalization, `compare_sequences` (canonical grouping policy — `NORMALIZATION.md` §8), and `scenario_capture` / `compare_scenario_captures` (the golden capture form of §8, NORMALIZATION.md §8.6). |
| `src/scenario.rs` | `ScenarioDriver`: scripted SSE event-sequence capture — boot the reference, subscribe from t0, run `Step`s (HTTP requests with `{capture}` templating, file writes, sleeps, sanity frame floors), collect to quiescence, golden the normalized group multiset. Includes `FakeLlm` (deterministic local OpenAI-compatible chat server; see below). |
| `src/golden.rs` | Golden compare with inline update (`PARITY_UPDATE=1`). Goldens live in `goldens/`. |
| `src/minih.rs` | Private minimal localhost HTTP/1.1 client (Basic auth, Content-Length/chunked, streaming). No heavyweight deps — the harness builds offline. |
| `tests/reference_info.rs` | The end-to-end sample test proving the whole chain. |
| `tests/scenarios.rs` | The scenario golden tests (below). |
| `goldens/` | Checked-in normalized expectations (`goldens/scenarios/` for SSE scenario goldens). |

## Running

```bash
source $HOME/.cargo/env
export CARGO_TARGET_DIR=/tmp/opencode/target-parity

cargo nextest run -p parity          # unit + integration (skips if bun/ref missing)
cargo clippy -p parity --all-targets -- -D warnings
cargo fmt -p parity -- --check
```

The sample end-to-end test (`parity::reference_info
reference_api_info_and_first_event_shape`) boots the reference server
(~1.5 s), fetches `/api/info` twice, asserts a stable normalized shape,
compares against the checked-in golden, exercises the two-sided `httpdiff`
self-compare, subscribes to `/api/event`, and goldens the first frame
(`server.connected`). It **skips with a pass** when bun or the reference
checkout is unavailable, so CI without the reference stays green.

### Reference invocation details

The harness runs (see `reference::spawn` module docs for the full story):

```
bun run --cwd /tmp/opencode/ref/packages/cli \
  --define OPENCODE_VERSION="2.0.24" --define OPENCODE_CHANNEL="stable" \
  --define OPENCODE_ARTIFACT="cli" \
  src/index.ts serve --port <free>
```

- `OPENCODE_VERSION` is a **build-time** define in the reference; passing it
  via `--define` (and invoking `src/index.ts` directly so the nested `dev`
  wrapper doesn't swallow it) makes `/api/info` report `2.0.24` instead of
  `local`. The runtime env var alone does not do this.
- **Auth/password acquisition:** the harness injects `OPENCODE_PASSWORD` into
  the child env (so the password is known a priori), with a fallback that
  parses the `server password <pw>` / `server listening on <url>` lines a
  foreground run prints (`packages/cli/src/server-process.ts` lines 174-175;
  client Basic-auth usage per `packages/client/src/service-probe.ts`).
- Readiness = poll `GET /api/info` until HTTP 200 (≤90 s).
- Shutdown = SIGTERM→SIGKILL to the child's **process group** (`bun run`
  re-execs a nested bun; killing only the wrapper would orphan the server).
- Sandbox: `<PARITY_SANDBOX_ROOT>/ref-<pid>-<port>/{home,tmp,logs}` with
  `HOME`/`OPENCODE_TEST_HOME`/`XDG_*`/`TMPDIR` pointed into it.
- Models fetch disabled (`OPENCODE_DISABLE_MODELS_FETCH=1`) for hermetic boot.

Environment overrides: `PARITY_BUN` (bun path), `PARITY_REF_ROOT` (reference
checkout), `PARITY_SANDBOX_ROOT` (sandbox parent), `PARITY_UPDATE` (golden
update mode).

## Updating goldens

```bash
PARITY_UPDATE=1 cargo nextest run -p parity   # (re)writes goldens/
```

Goldens store **normalized** output (see `NORMALIZATION.md`); a mismatch that
survives normalization is a real divergence. A missing golden is written on
first run (bootstrap) — ship it in-tree afterwards.

## Scenario goldens (SSE event sequences)

`tests/scenarios.rs` drives scripted API flows against a fresh reference
server, subscribes to `/api/event` from t0, collects data frames until
quiescence, and goldens the **normalized canonical grouping** of the frame
sequence (`NORMALIZATION.md` §8, golden form §8.6) under
`goldens/scenarios/<name>.json`:

| Scenario | Flow | Pinned frames |
| --- | --- | --- |
| `session_lifecycle` | POST /api/session (create, in the sandbox project dir) → PATCH rename → GET list → DELETE | `session.created`/`renamed`/`deleted` (seq order) + location boot frames |
| `config_touch` | boot project location → create `opencode.json` → sleep past the 100 ms watcher debounce → modify it | `config.updated` ×2 + the config-reload cascade (provider/model/agent/… `.updated`) |
| `permission_saved` | create session → warm-up ask → ask (`external_directory` outside the sandbox globs, with `save`) → reply `always` → GET /api/permission/saved → DELETE the row | `permission.asked`/`permission.replied` |
| `location_reload` | boot project location → POST /api/location/reload | `location.shutdown` + replacement-location boot frames |
| `prompt_fake_llm` | write a config-seeded `openai-compatible` provider (baseURL → `FakeLlm`) → create session with that model → POST prompt | full prompt→message flow: `session.inbox.*`, `step.started/streamed/ended`, `text.started/delta/ended`, `execution.*`, `usage.updated` — deterministic text/tokens/cost |

These goldens pin **reference v2.0.24 behavior**. In Phase 5 the same
scripts are replayed against the Rust port: boot both servers, run the
identical `Step` list against each, collect with the same budget, and
compare the two live sequences via `sse::compare_sequences` (or replay a
side's capture against the checked-in golden via
`sse::compare_scenario_captures`) — httpdiff-style side-by-side with the
same normalization on both sides.

### Adding a scenario

1. Add a test in `tests/scenarios.rs` using the `scenario_test` helper:
   build a `ScenarioSpec` (name, description; default budget is 512 frames
   / 3 s quiescence / 30 s overall) and a `Vec<Step>`.
2. Steps: `Step::request(method, path, body)` (`.expect(status)`,
   `.capture(name, "/json/pointer")`), `Step::write_file(path, contents)`,
   `Step::sleep_ms(n)`, `Step::expect_frames(type, min)` (a sanity floor
   that also guards update mode against writing a golden of nothing).
   Paths/bodies are `{capture}`-templated; `{project_dir}` /
   `{project_dir_q}` (percent-encoded) are pre-seeded with the sandbox
   project directory `<sandbox>/project`. Location-scoped GETs take
   `?location[directory]={project_dir_q}`; POST /api/session takes
   `location` in the body; session-scoped routes resolve the session's own
   location.
3. Run once to bootstrap the golden, ship it in-tree, then re-run in
   compare mode. If a frame is nondeterministic across runs, normalize it
   per `NORMALIZATION.md` (add the rule there first) — never delete frames
   from a golden by hand.

### Deterministic model responses without LLM keys

`scenario::FakeLlm` is a loopback HTTP server that answers every
`POST …/chat/completions` with the same scripted OpenAI chat SSE stream
(role delta → two content deltas → `stop` + fixed usage → `[DONE]`), so a
config-seeded provider drives the entire prompt→message pipeline
deterministically (see the module docs for the provider config snippet).

**Reference simulation support — finding** (`packages/simulation`):
`OPENCODE_SIMULATE=1` (see `packages/cli/src/server-process.ts`) replaces
all outbound HTTP with a simulated route table:

- Without `OPENCODE_DRIVE` it answers only
  `https://models.opencode.ai/api.json` (empty catalog) and denies every
  other destination — hermetic networking, **no** LLM answering. It would
  actually *break* the config-seeded fake provider (a localhost baseURL is
  not in the route table), which is why scenario servers do not enable it.
- With `OPENCODE_DRIVE=1` it additionally serves a driver-answered OpenAI
  endpoint at `https://api.openai.com/v1/chat/completions`: a control
  server listens on `ws://127.0.0.1:40950` and an external controller
  speaks a JSON-RPC websocket protocol (`simulation.handshake` →
  `llm.attach` → receive `llm.request` → answer `llm.chunk` /
  `llm.finish`; instance names resolve a manifest under
  `$XDG_STATE_HOME/opencode-drive/instances/<name>.json`). Fully
  deterministic in principle, but it needs a hand-rolled websocket client
  in the harness plus a config-seeded `openai` provider — more machinery
  than `FakeLlm` for the same guarantee. Documented, not used.
  (`reference::spawn_with_env(&[("OPENCODE_SIMULATE", "1")])` exists for
  experiments.)

## Stubbed for later phases

The harness ships skeleton-quality plumbing beyond the proven sample chain.
Not yet implemented (module seams exist):

- **db_state compare** — dump both servers' sqlite state after identical
  request scripts and diff normalized dumps. Needs a scripted-scenario runner
  on top of `httpdiff::compare_requests` plus a `db_state` normalizer
  (extra key-rewrites: autoincrement rowids, `updated_at` columns). The
  scenario runner half now exists (`src/scenario.rs`).
- **cli output compare** — run `opencode` CLI commands against both builds,
  normalize stdout/stderr (`normalize_str` applies directly; needs a
  terminal-width + ANSI-escape pass added to the spec first).
- **tui frame compare** — capture TUI frames from both builds at scripted
  checkpoints; requires an output-normalization layer for cursor addressing
  and timing-dependent spinners (extend `NORMALIZATION.md` §5 before use).
- **two-sided scenario replay** — DONE (Phase 5 / M1): `src/rust_server.rs`
  spawns the `oc-server` binary (built on demand, hermetic env, sandboxed
  HOME, cwd mirroring the reference spawn), `tests/rust_scenarios.rs`
  replays the SAME five scripts (`scenario::scripts`) against it and
  compares to the reference goldens through
  `sse::filter_capture_types` + `sse::compare_scenario_captures` with an
  explicit per-scenario skip-list. Current state: 4/4 no-model scenarios
  EXACT; `prompt_fake_llm` equal modulo `session.instructions.updated`
  (the port's instruction digest set differs — documented in the test).
  `tests/m1_tui_smoke.rs` additionally boots the official v2.0.24 TUI
  against the Rust server inside tmux (opt-in: `OC_M1_TUI=1` +
  `--run-ignored only`); its pane captures live in `goldens/m1/`.

## Phase-10 verification battery (`tests/p10_*.rs`)

Opt-in twice (`#[ignore]` + `OC_P10=1`), so the default suite never needs
bun/reference/tmux:

```bash
OC_P10=1 cargo nextest run -p parity --run-ignored only -E 'test(p10_)'
```

| Test | What it pins |
| --- | --- |
| `p10_openapi` | `GET /openapi.json` from the booted port vs the pinned `/opt/opencode-ref/packages/protocol/openapi.json`: semantic diff (`src/openapi_diff.rs` — path×method sets, per-op operationId/parameters/`$ref` sets, schema names) must be empty minus `goldens/p10/openapi-whitelist.json`; the summary golden pins counts + byte-identity. |
| `p10_error_paths` | 7 representative error routes probed on a booted reference AND port side-by-side; normalized bodies byte-compared; `goldens/p10/error-paths.json` pins each side + the divergence report (fails on new OR stale divergences). |
| `p10_cross_tui` | The real v2.0.24 TUI (bun + tmux) against the Rust server: prompt+reply (`FakeLlm`), rename (`ctrl+r`, verified server-side), permission-once (external read → footer → TUI replies → tool proceeds). Captures in `goldens/p10/tui/`. |
| `p10_cross_client` | The generated `oc-client` (dev-dependency) against a booted reference server: create → prompt → parked ask → permission reply → `message.list`, every response decoded. |
| `p10_db_interop` | One shared data home, three boots: reference writes NULL-heavy rows → port re-opens (migration journal byte-identical = 0 reruns) + continues → reference re-opens + continues. Journal manifest golden-pinned. Shared-file lane on `opencode.db` (v2 `OPENCODE_DISABLE_CHANNEL_DB=1`) + the stable-channel lane `p10_db_interop_stable_channel_same_file`: the reference build (baked `stable`) and an `OPENCODE_CHANNEL=stable` Rust boot interoperate on `opencode-stable.db` (P10-DB1 closed — the engine resolves DB paths through the channel scheme). |

Shared helpers: `tests/p10_common/mod.rs` (guards, `minih`-based API sugar,
the scripted fake LLM with tool calls, `bun:sqlite` DB dumps). Harness seams
added for the battery: `minih` is now `pub`, `openapi_diff` is a new module,
and `reference::spawn_reference` accepts seed files + env (mirroring
`RustSpawn`). The full battery ledger — table, whitelist, findings (all
closed by the final remediation sweep except note P10-N1) — is
`goldens/p10/VERIFICATION.md`.

## Phase-10 differential gates (§8.3 skeletons + known-red)

The five minimal dual-end gates of `docs/PARITY-LOOP.md` §8.3 (B0-2).
Each is `#[ignore]` + `OC_P10=1`; strict runs add `OC_GATE_STRICT=1`
(fail-not-skip, `parity::gate`):

```bash
OC_P10=1 OC_GATE_STRICT=1 PARITY_BUN=$HOME/.bun/bin/bun \
  cargo nextest run -p parity --locked --run-ignored only --no-fail-fast -E 'test(p10_)' \
  2>&1 | tee /tmp/opencode/p10-strict.log
python3 tools/parity/known-red-summary.py /tmp/opencode/p10-strict.log
```

| Gate | File | Live today / structured gaps |
| --- | --- | --- |
| 1 HTTP/SSE contract | `tests/p10_http_contract.rs` | pagination absent-vs-null + order + limit + cursor round-trips, error envelopes, 2-route validation matrix (full 141-op table in the artifact), first SSE event — live; `previous`-cursor presence and cursor-token payloads recorded as gap facts (unregistered divergences, pending ledger triage). |
| 2b real-v2 import | `tests/p10_db_import.rs` | copies a REAL v2 library DB into a lane sandbox (`PARITY_V2_DB_FIXTURE`), journal 0-rerun + project-id no-double-rows + continue smoke, then the reference re-opens. No fixture exists in-tree: strict mode fails named-missing until one is provided; `PARITY_V2_DB_FIXTURE_PROMPT=1` enables the write-path continue. |
| 3 tools | `tests/p10_tools.rs` | read/edit/glob/grep dual-end compares (normalized tool part + file side effects) — live; shell/timeout/permission/patch are ledger-linkable checkpoints. |
| 4 TUI five flows | `tests/p10_tui_frames.rs` | flow 1 (boot/home) structural smoke at 80x24 + 120x30 via `tui_diff::PtyChild`/`Grid` — live; flows 2–5 not-wired checkpoints (form/permission surfaces → ledger `TUI-TD16`/`TUI-TD18`). |
| 5 CLI | `tests/p10_cli.rs` | `--version`/`--help`/`serve --hostname localhost`/`auth list`/`models`/`session list`/`run --format json` + early error/`--print-logs` — live (both CLIs pinned to their own server; the managed-service port is never touched); help text, TTY branches and table time formats are recorded gaps. |

**Known-red workflow (§8.5):** expected mismatches are registered in
`goldens/p10/known-red.json` (`ledger_id`, `test`, `pattern`, `added`,
`expiry_batch`, `owner`). Gate tests wrap registered failures with
`p10_common::known_red(ledger_id, expiry_batch)` — the test still FAILS,
with the message prefixed `KNOWN-RED:<ledger-id>:` (a missing/expired/
drifted entry prefixes `NEW-RED:` instead). `known-red-summary.py`
classifies a strict-run log: exit 0 only when there are no `NEW-RED` and
no expired known-red; strict-mode prerequisite failures (e.g. the unset
Gate 2b fixture) are labeled `MISSING-PREREQ` — labeled, never counted as
passes. Known-red may only shrink: registering a new entry is a triage
decision (ledger + expiry + owner), not a lane-side shortcut.
