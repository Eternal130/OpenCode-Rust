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
| `src/reference.rs` | `ReferenceServer`: spawn/ready/stop of the reference server on a free port in a sandboxed HOME; `skip_if_no_bun()` test helper. |
| `src/httpdiff.rs` | Same request against two bases (`httpdiff::Side`), normalize both, `deep_diff` reports the first divergence path (e.g. `$.urls[0]`). |
| `src/sse.rs` | `SseStream`: GET /api/event subscription, `data:`/heartbeat parsing, `collect(n, timeout)`, `frames_to_json` normalization. |
| `src/golden.rs` | Golden compare with inline update (`PARITY_UPDATE=1`). Goldens live in `goldens/`. |
| `src/minih.rs` | Private minimal localhost HTTP/1.1 client (Basic auth, Content-Length/chunked, streaming). No heavyweight deps — the harness builds offline. |
| `tests/reference_info.rs` | The end-to-end sample test proving the whole chain. |
| `goldens/` | Checked-in normalized expectations. |

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

## Stubbed for later phases

The harness ships skeleton-quality plumbing beyond the proven sample chain.
Not yet implemented (module seams exist):

- **db_state compare** — dump both servers' sqlite state after identical
  request scripts and diff normalized dumps. Needs a scripted-scenario runner
  on top of `httpdiff::compare_requests` plus a `db_state` normalizer
  (extra key-rewrites: autoincrement rowids, `updated_at` columns).
- **cli output compare** — run `opencode` CLI commands against both builds,
  normalize stdout/stderr (`normalize_str` applies directly; needs a
  terminal-width + ANSI-escape pass added to the spec first).
- **tui frame compare** — capture TUI frames from both builds at scripted
  checkpoints; requires an output-normalization layer for cursor addressing
  and timing-dependent spinners (extend `NORMALIZATION.md` §5 before use).
- **SSE sequence parity beyond frame 1** — `SseStream::collect` supports N
  frames; scenario scripts that trigger identical event sequences on both
  servers are the missing piece.
