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
however, diverge systematically from the reference's internal handler
names: an `experimental.*` prefix family (`experimental.config.update`,
`experimental.fs.write`, `experimental.generate.text`, `experimental.mcp.*`,
`experimental.integration.wellknown.add`, …), `session.message.get`/`
`session.message.list` vs ref `session.message`/`session.messages`, and
`debug.location.list` vs ref `debug.location`.

Consequence: ecosystem tooling that keys on operationId (hooks,
dashboards, plugin route mapping) is **not interchangeable** between the
reference and this port — map by path+method, not by operationId.
