# openapi-gen

Generates the oc-server route/validation surface from the pinned
OpenAPI spec (`tools/openapi-gen/openapi.json`, exported from the
reference opencode v2 checkout).

```
cargo run -p openapi-gen                 # regenerate routes.gen.rs / validation.gen.rs / ...
cargo run -p openapi-gen -- --check      # fail if generated files are out of date (CI: RA-08)
cargo run -p openapi-gen -- --stdout routes|types|methods|validation
cargo run -p openapi-gen -- --spec PATH  # alternative spec input
```

## Baseline-bump checklist (when the reference openapi.json changes)

Run through this list in order when copying a new reference
`openapi.json` over the pinned one:

1. **Copy the spec**: replace `tools/openapi-gen/openapi.json` with the
   new export from the reference checkout.
2. **Regenerate**: `cargo run -p openapi-gen` (from the repo root), then
   `cargo fmt --all` — the generated files are fmt-checked in CI.
3. **Drift check**: `cargo run -p openapi-gen -- --check` must pass
   (proves the committed generated files match the new spec).
4. **Guards baseline**: run `tools/guards/run.sh`. If the OpenAPI
   surface GREW (new routes/responses), update the affected guard
   baselines with `tools/guards/run.sh --update-baseline` and justify
   the growth in the commit message. Baselines may only otherwise
   shrink. `error_envelope` (RA-09) has no baseline: a new error
   `_tag`/status in the spec must be backed by a constructor in
   `crates/oc-server/src/errors.rs` before the guard goes green.
5. **Re-audit deltas**: diff the old vs new spec and re-audit any change
   to `CAPPED_NODES` and the request-validation rules in the generator
   source (`tools/openapi-gen/src/`) — a new capped node or a changed
   validation shape is a parity decision, not mechanical regeneration.
6. **Regen p10 goldens**: regenerate the phase-10 golden files that pin
   wire bytes (see `crates/oc-server/tests/`) when response schemas
   changed.
7. **Re-probe validation messages**: for any endpoint whose payload or
   query schema changed, re-run the validation-message probes against
   the reference server and re-pin the observed bytes.

CI (RA-08): the `guards` job runs `cargo run -p openapi-gen --locked --
--check` after the static guards — an updated spec without regeneration
fails the build.

## Internal operationId divergence vs the reference (RB-05)

The served OpenAPI document is a byte-for-byte copy of the reference
export, so the wire contract (paths, methods, schemas) is unaffected.
The **internal route ids** emitted into `crates/oc-server/src/routes.gen.rs`,
however, diverge from the reference's internal endpoint names for 31 of
the 141 operations. The reference declares each endpoint in
`packages/protocol/src/groups/*.ts` as
`HttpApiEndpoint.<verb>("<name>", "<path>")`; effect's OpenAPI export
normally drops the redundant namespace segment of the group-qualified
name when building the spec operationId, but for the families below the
exported id differs from the endpoint name actually used in the
reference handler code.

The full list (derived by scripting `HttpApiEndpoint.*` declarations out
of the reference checkout against the pinned spec, keyed by path+method —
31 of 141; the other 110 operations match verbatim):

| # | Method | Path | Ref internal endpoint name | routes.gen.rs / spec operationId |
|---|--------|------|----------------------------|----------------------------------|
| 1 | PATCH | `/api/experimental/config` | `config.update` | `experimental.config.update` |
| 2 | GET | `/api/debug/location` | `debug.location` | `debug.location.list` |
| 3 | POST | `/api/experimental/fs/write` | `fs.write` | `experimental.fs.write` |
| 4 | POST | `/api/experimental/generate` | `generate.text` | `experimental.generate.text` |
| 5 | POST | `/api/experimental/integration/wellknown` | `integration.wellknown.add` | `experimental.integration.wellknown.add` |
| 6 | PUT | `/api/experimental/mcp/{server}` | `mcp.add` | `experimental.mcp.add` |
| 7 | DELETE | `/api/experimental/mcp/{server}` | `mcp.remove` | `experimental.mcp.remove` |
| 8 | POST | `/api/experimental/mcp/{server}/connect` | `mcp.connect` | `experimental.mcp.connect` |
| 9 | POST | `/api/experimental/mcp/{server}/disconnect` | `mcp.disconnect` | `experimental.mcp.disconnect` |
| 10 | GET | `/api/session/{sessionID}/message` | `session.messages` | `session.message.list` |
| 11 | GET | `/api/experimental/migration/v1` | `migration.v1.status` | `experimental.migration.v1.status` |
| 12 | GET | `/api/experimental/session/{sessionID}/terminal/read` | `persistentPty.read` | `server.experimental.persistentPty.read` |
| 13 | GET | `/api/experimental/session/{sessionID}/terminal` | `persistentPty.list` | `server.experimental.persistentPty.list` |
| 14 | POST | `/api/experimental/session/{sessionID}/terminal` | `persistentPty.create` | `server.experimental.persistentPty.create` |
| 15 | POST | `/api/experimental/persistent-pty/shutdown` | `persistentPty.shutdown` | `server.experimental.persistentPty.shutdown` |
| 16 | POST | `/api/experimental/persistent-pty/handoff` | `persistentPty.handoff` | `server.experimental.persistentPty.handoff` |
| 17 | GET | `/api/experimental/persistent-pty/{ptyID}` | `persistentPty.get` | `server.experimental.persistentPty.get` |
| 18 | PUT | `/api/experimental/persistent-pty/{ptyID}` | `persistentPty.update` | `server.experimental.persistentPty.update` |
| 19 | GET | `/api/experimental/persistent-pty/{ptyID}/snapshot` | `persistentPty.snapshot` | `server.experimental.persistentPty.snapshot` |
| 20 | DELETE | `/api/experimental/persistent-pty/{ptyID}` | `persistentPty.remove` | `server.experimental.persistentPty.remove` |
| 21 | POST | `/api/experimental/persistent-pty/{ptyID}/connect-token` | `persistentPty.connectToken` | `server.experimental.persistentPty.connectToken` |
| 22 | POST | `/api/pty/{ptyID}/connect-token` | `pty.connectToken` | `pty.connect.token` |
| 23 | GET | `/api/experimental/session/stats` | `session.stats` | `experimental.session.stats` |
| 24 | POST | `/api/experimental/session/import` | `session.import` | `experimental.session.import` |
| 25 | GET | `/api/experimental/session/{sessionID}/export` | `session.export` | `experimental.session.export` |
| 26 | POST | `/api/experimental/session/{sessionID}/skill` | `session.skill` | `experimental.session.skill` |
| 27 | POST | `/api/experimental/session/{sessionID}/wait` | `session.wait` | `experimental.session.wait` |
| 28 | GET | `/api/experimental/session/{sessionID}/instructions/entries` | `session.instructions.entry.list` | `experimental.session.instructions.entry.list` |
| 29 | PUT | `/api/experimental/session/{sessionID}/instructions/entries/{key}` | `session.instructions.entry.put` | `experimental.session.instructions.entry.put` |
| 30 | DELETE | `/api/experimental/session/{sessionID}/instructions/entries/{key}` | `session.instructions.entry.remove` | `experimental.session.instructions.entry.remove` |
| 31 | GET | `/api/session/{sessionID}/message/{messageID}` | `session.message` | `session.message.get` |

Notes on the families:

- **`experimental.*` prefix** (rows 1, 3-9, 11, 23-30): endpoints mounted
  under `/api/experimental/...` gain a prefix in the exported id that the
  reference's own endpoint name does not carry.
- **`server.experimental.persistentPty.*` namespace** (rows 12-21): the
  reference group is `server.experimental` with endpoint names
  `persistentPty.*`; because the endpoint name does not embed the group
  namespace, the export keeps the full group-qualified id. The three
  terminal routes (`persistentPty.create` / `.list` / `.read`,
  rows 12-14) reuse the namespace at
  `/api/experimental/session/{sessionID}/terminal[...]` — an
  operationId-keyed lookup gives no hint that the terminal routes live
  in the persistentPty namespace. The raw websocket upgrade
  (`persistentPty.connect`, annotated with an explicit
  `OpenApi.annotations({identifier})`) keeps its bare name in the spec
  and is therefore *not* a rename.
- **`session.message.get` / `session.message.list`** (rows 10, 31) vs ref
  `session.message` / `session.messages`, and **`debug.location.list`**
  (row 2) vs ref `debug.location`: method-suffix renames from the
  export.
- **`pty.connect.token`** (row 22): the only rename outside the
  experimental surface — camelCase `connectToken` became
  dot-separated `connect.token`.

Consequence: ecosystem tooling that keys on operationId (hooks,
dashboards, plugin route mapping) is **not interchangeable** between the
reference and this port — map by path+method, not by operationId.

For the schema-side reference (`@opencode/schema` TS files -> Rust
modules), see [docs/SCHEMA-MAP.md](../../docs/SCHEMA-MAP.md).
