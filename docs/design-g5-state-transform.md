# G-5: Plugin-scoped state-transform chain (ref `state.ts` port)

Ref: `packages/core/src/state.ts` (`State.create` / `Transformable`) — plugin-scoped
`Transformable`s can dynamically rewrite configuration-derived state; the state
pipeline applies every registered transform, in registration order, after static
assembly.

## Ref surface actually used

`Transformable` is implemented by ~20 ref domains, but the **rewrite** surface
plugins touch from JS (`ctx.*.transform`) splits into:

- **additive** registrations (`command.add`, `tool.add`, `mcp`, `skill`,
  provider/model) — these already cross into Rust as plain registration data
  (`PluginCommandDef`, `PluginToolDef`, `PluginMcpDef`, `PluginSkillDef` in
  `plugin_apply.rs`, materialized by the host bridge's whole-table snapshots);
- **rewrites of config-derived state** — `ctx.agent.transform`
  (`agent.ts` `Editor`: `list`/`get`/`default`/`update`/`remove`), the one
  surface where plugins mutate previously assembled config state.

The Rust port is therefore bounded to the **agent state** (agent table +
configured default), the minimal surface that was missing.

## Rust design

New module `oc-core/src/state_transform.rs`:

- `AgentState { agents, default }` — the config-derived agent state; captured
  from an `AgentRegistry` via `AgentState::from_registry`.
- `AgentStateEditor<'a>` — the ref editor API (`list`/`get`/`default`/`update`/
  `remove`) over a mutable candidate.
- `AgentTransform { plugin_id: Option<String>, run }` — one registration,
  scoped per plugin (`AgentTransform::plugin`) or ungrouped/host
  (`AgentTransform::host`).
- `run_agent_transforms(base, transforms)` — the ref `state.ts:172-194` `get`
  rebuild loop: replay transforms in registration order over a fresh candidate;
  a **plugin-scoped failure disables that plugin's whole transform group** and
  restarts from the fresh base (terminating because each restart disables one
  group); an **ungrouped (host) failure propagates** to the caller. Grouped
  failures are reported (ref `State.Failure` shape) instead of failing the read.
- `apply_agent_state_transforms(registry, transforms)` — captures the registry
  base, runs the chain, writes the result back as a diff (`set` changed rows,
  `remove` vanished ids, `set_default`). An ungrouped failure leaves the
  registry untouched.

Wiring (`plugin_apply.rs`): `LocationPluginSource::agent_state_transforms()` is
a new default (empty) trait method; `apply_location_plugins` runs the chain
AFTER the additive snapshot application (agents + default), filtered to this
location's active plugins (PLUG-D07). An ungrouped failure is logged and the
pre-transform agent state is kept (assembly continues).

Guest exposure: **host-side only**. Ref plugins *can* register transforms from
JS, and the existing bridge already materializes `ctx.agent.transform` results
as whole-table snapshots per plugin (additive at the core side). The rewrite
stage is exposed to embedders as host closures over the snapshot-backed plugin
source; a synchronous JS-callback-during-assembly bridge would need a blocking
IPC round-trip inside the engine's assembly path, which the bridge explicitly
avoids ("per-frame IPC round-trips are a latency disaster").

## Divergences from ref (and why)

1. **Eager vs lazy rebuild** — ref rebuilds lazily on read with revision
   tracking and Scope-based disposal. Rust rebuilds once at the assembly point
   that consumes the result (`apply_location_plugins`); there is no lazy read
   seam to hook, and the location graph rebuild is the natural reload boundary.
2. **Group scope** — ref groups span ALL domains of a plugin (a failing command
   transform also drops that plugin's agent registrations). The bounded port
   disables only the plugin's *state transforms*; retro-dropping already-applied
   tool/MCP rows mid-assembly would desync the host's registration snapshot.
3. **Error channel** — "throw" is `Result::Err(String)`; ungrouped failures are
   contained at the assembly seam (logged, pre-transform state kept) instead of
   failing the location build, since oc-core assembly has no ref-equivalent
   supervisor to hand the defect to at this layer.
4. **`editor.update` on unknown id** — ref fabricates schema defaults; the Rust
   editor is rewrite-only (creation stays on the snapshot lane).

## Files

- `crates/oc-core/src/state_transform.rs` (new) — chain + editor + registry
  round-trip + unit tests.
- `crates/oc-core/src/plugin_apply.rs` — `agent_state_transforms()` trait
  method + transform stage in `apply_location_plugins`.
- `crates/oc-core/src/location.rs` — `AgentRegistry::configured_default()`
  getter (transform base capture).
- `crates/oc-core/src/lib.rs` — module export.
