# Static anti-regression guards (§9.1)

Eight stdlib-only Python 3 guards that pin the **current** state of the
port's known gaps and fail the moment a **new** violation of the same
class appears. They are not scanners: they check fixed patterns, they do
not fix anything, they do not need bun / the reference checkout / cargo,
and they never signal processes.

Semantics (PARITY-LOOP.md §9.2):

* `tools/guards/baseline/<guard>.json` records the violation set that
  exists **today**. The baseline may only shrink. Shrunk entries are
  reported as `INFO` and never fail.
* `tools/guards/allowlist/<guard>.txt` holds explicit, ledger-backed
  exceptions (`pattern | reason | ledger-id | owner | review-date`).
  An entry whose `review-date` has passed stops covering and turns the
  guarded hits red (expiry ⇒ CI failure, §9.2). Wildcards are forbidden;
  empty reasons are forbidden. An allowlist entry matching nothing
  anymore is reported as `INFO` (remove it when convenient).
* Any violation not covered by baseline ∪ allowlist ⇒ exit 1 (red).
* Guard errors (malformed baseline/allowlist, missing tool) ⇒ exit 2.

## Running

```bash
tools/guards/run.sh                  # all guards; nonzero on any new violation
tools/guards/<guard>.py              # one guard, text summary
tools/guards/<guard>.py --format json
tools/guards/run.sh --update-baseline  # record current sets (see below)
```

CI runs the `guards` job (`.github/workflows/ci.yml`) — python3 only.

## Updating baselines / allowlists

`--update-baseline` rewrites the baseline with the current set. It is a
commit-worthy act, not a chore:

* **Shrink** (a violation stopped reproducing): run `--update-baseline`
  in the same commit that fixed the gap, or let it ride — shrunk entries
  never fail. Cleanup commits are welcome.
* **Grow** (accepting NEW debt): forbidden without justification. The
  commit message must say (a) which ledger entry covers each new
  violation, (b) why baselining is right instead of fixing. For
  `process_safety` a growing baseline is a red flag, not a routine act.
* **Allowlist**: only "explicitly accepted, ledger-backed, expiring"
  exceptions (§9.2). Never to bypass a red guard that you could fix.
  Every entry needs a real ledger id pointing at an `open` /
  `needs-runtime` row (§9.4).

New gaps the guards *discover* go to the ledger as new entries — guards
do not fix and do not self-register ledger rows.

## The guards

### config_consumers
Schema key with no read point. Parses `crates/oc-schema/src/config/**`
struct fields (serde renames honored; `skip`-ped fields ignored) and
greps production `crates/` code for `.<field>` accesses or `"<key>"`
literals. Violation = key referenced nowhere outside the schema
definitions. Brittle-parser honesty: line-regex struct parser, reported
section inventory in `meta`; common-word fields (`model`, `agent`) can
never read as dead — the heuristic only under-reports.

### hook_dispatch
Registered hook kind with no production dispatcher. Hook kinds come from
the `oc_hook_points!` table in `crates/oc-core/src/hooks.rs`;
`dispatch_<kind>(` must appear in a non-test file other than `hooks.rs`.
The plugin host's generic `Bridge::dispatch_hook`/`dispatch_pump` path
is engine-driven RPC and is not a per-kind dispatch.

### route_validation
Route without request validation. Routes come from the generated
`crates/oc-server/src/routes.gen.rs` (read-only); handler mounts
(`.route("<path>", <method>(<fn>))`) from `crates/oc-server/src/handlers/`
(mounts inside a `#[cfg(test)]` region are test fixtures, not surface);
the handler body (paren/brace-matched) must invoke a validator
(`schema_check::<fn>(..)` or any `*validat*(..)` call). Validation is
ALSO credited from the HTTP-H0 middleware seam
(`crates/oc-server/src/request_validation.rs`, wired in
`handlers/mod.rs::mount`): every op in `ENFORCED_OPS` (the COMPLETE
28-op validate-first body rollout — 46 JSON bodies minus 18
location-first) and every op with a generated query row
(`QueryValidation::Params` in `validation.gen.rs`, 71 of them) that is
NOT in the seam's `LOCATION_FIRST_OPS` (29 ops whose v2 endpoint
resolves the session/form before decode — 404 precedes any schema 400,
probe evidence `rollout-probe*.json`). A second rule catches **mount
drift**: handler mounts absent from the generated table (surface outside
the OpenAPI contract). After the HTTP-H0 rollout (2026-10-21) 84 of 141
ops are covered; the 57-op baseline is exactly the two documented
classes: 29 location-first (pending the post-location seam, ledger
HTTP-H0) and 28 readonly ops with neither a request body nor query
parameters (nothing to validate; path params are a separate slice — v2
`kind: "Params"` 400s are a known port gap, probe
`rollout-probe-2.json`). Limitation: axum `Json<T>` extraction counts as
no validation (deliberately strict).

### event_schema
Published event type with no schema/round-trip test. Event types are the
dotted literals in `crates/oc-core/src/bus/registry.rs` plus literals
passed to `EventDefinition::{ephemeral,durable}` in production code;
coverage = the quoted type appearing in a test file
(`**/tests/**`, `tests.rs`, `*_tests.rs`). Dynamic (variable) type
publishers are out of scope — the registry lists are the canonical
surface.

### env_inventory
Reference `OPENCODE_*` env read with no Rust claim point. Canonical list
= READ-ONLY scan of `/opt/opencode-ref/packages` for
`process.env.OPENCODE_*` (fast, ~50 ms); CI (no ref checkout) uses the
committed snapshot `tools/guards/snapshots/env_ref.txt`, regenerated
with `--refresh` (the only other writing mode). A claim point is the
variable name appearing in any `crates/**/*.rs`. Refreshing after a ref
upgrade surfaces new vars as NEW violations until baselined with ledger
CONFIG entries. Limitation: indirect ref env access that doesn't
literally spell `process.env.OPENCODE_X` is invisible.

### command_keymap
Registered TUI command with a dead handler. Commands come from the
`defs!` entries of `DEFAULT_TABLE` in
`crates/oc-tui-core/src/keymap/table.rs`; a command is "handled" when
its quoted literal appears in production oc-tui / oc-tui-core / oc-cli
code outside the table itself and outside `oc-tui/src/config.rs` (the
keybind-override parser mentions every command name — that is not a
handler). Palette-only and deprecated rows read as dead today →
baseline. Limitation: a no-op handler body still passes; handler
references must be string literals.

### ledger_integrity
`docs/parity-ledger.jsonl` must validate clean. Wraps
`python3 tools/parity-ledger.py validate --paths` (same interpreter);
every reported error line is a violation; a nonzero tool exit without
parseable errors is itself one. `--exit-criteria` adds the ledger's
`--exit` (batch-exit gates) — off by default. The baseline is empty by
design: there is no acceptable standing violation here. Do not baseline
this guard; fix the ledger row.

### process_safety
§2.1 red lines as static patterns: `pkill`, `killall`, `fuser`,
`os.killpg`, bare-PGID kills (`kill -9 -<PGID>`, `kill -- -N` — signal
flag followed by a negative target; plain `kill -9 <pid>` and liveness
`kill -0 <pid>` do NOT match), `kill $(pgrep ...)`, name-targeted
pkill/killall (`bun|node|cargo|nextest|python|opencode|oc-server`), any
`kill` in `tools/**/*.sh` (raw `$PID`/`$!` shell kills — §2.1 wants
child handles or `signal_group_filtered`), and `sudo` / `systemd-run`
outside `tools/dev-gate.sh` (the sanctioned scope user). Scans repo text
files, skipping `.git/`, `.worktrees/`, `target/`, `node_modules/`,
`docs/` (spec text quotes the forbidden list), `tools/guards/**` (this
guard's own artifacts), binaries, and >8 MiB files. The §2.1 legacy debt
(the four `spec-p3`/`spec-ef2` probes and `tools/release-smoke.sh`) sits
in the allowlist with ledger IDs `HYGIENE-PKILL-LEGACY-PROBES` and
`HYGIENE-RELEASE-SMOKE-KILL`, review-date 2026-12-18 (Batch 1+ rewrite).

## Layout

```
tools/guards/
  run.sh                 runner (all guards, per-guard summary)
  guardlib.py            shared baseline/allowlist/reporting harness
  <guard>.py             the nine guards
  baseline/<guard>.json  current violation sets (only shrink)
  allowlist/<guard>.txt  ledger-backed exceptions (§9.2 format)
  snapshots/env_ref.txt  env_inventory's committed ref snapshot (CI mode)
```

### error_envelope

RA-09 reconciliation, no baseline/allowlist: every (operation, status,
`_tag`) the pinned `tools/openapi-gen/openapi.json` declares as an error
response (431 triples today) plus a 3-entry hand-maintained supplement
for `LocationNotFoundError` (spec-invisible, produced by the
request-location middleware) must be constructible by
`crates/oc-server/src/errors.rs`. One-directional — registry richness
beyond the spec (e.g. the readiness `code` envelopes) is fine. A spec
regeneration that adds an error tag goes red until the registry gains
the constructor; a tag change in both places stays green only if the
status matches too.

