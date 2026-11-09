//! Plugin-scoped state-transform chain (G-5) — the Rust port of the
//! ref's `State.Transformable` runtime (packages/core/src/state.ts).
//!
//! Ref semantics being mirrored (state.ts `create().get`, lines
//! 172-194):
//!
//! * derived state rebuilds from a static base (`initial()`) and every
//!   registered transform runs against a mutable editor **in
//!   registration order**;
//! * a transform failure inside a plugin-scoped group **disables the
//!   whole group** (every registration of that plugin drops out) and
//!   the rebuild **restarts from a fresh candidate**;
//! * an ungrouped (host) transform failure propagates to the reader.
//!
//! Bounded surface: the config-derived **agent state** (the agent
//! list + the configured default) — the surface ref plugins actually
//! rewrite via `ctx.agent.transform` (`agent.ts` `Editor` with
//! `list`/`get`/`default`/`update`/`remove`). The additive halves
//! (`command.add`, `tool.add`, `mcp`, `skill`) already cross as plain
//! registration data through `plugin_apply`'s snapshot defs.
//!
//! Divergences from ref (deliberate, documented):
//!
//! * ref rebuilds lazily on read with revision tracking; Rust rebuilds
//!   eagerly at the one assembly point that reads the result
//!   (`apply_location_plugins`) — there is no lazy read seam to hook;
//! * ref groups span ALL domains of one plugin (a failure in a command
//!   transform also drops its agent registrations). Rust's bounded
//!   port isolates failure to the plugin's *state transforms* only —
//!   dropping already-applied tool/mcp rows mid-assembly has no ref
//!   replay guarantee here and would desync the snapshot seam;
//! * "throw" is a `Result::Err(String)` — the callback contract is
//!   otherwise identical (mutate the editor, return `Ok(())`).

use crate::location::AgentRegistry;
use oc_schema::agent::Info;
use oc_schema::id::AgentId;
use std::collections::HashSet;

/// One grouped-transform failure report (ref `State.Failure`:
/// `{ state, cause }` plus the owning plugin for logging).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformFailure {
    /// The state domain that failed to rebuild (e.g. `"agent"`).
    pub state: &'static str,
    /// The failing plugin's id (`None` = an ungrouped host transform).
    pub plugin_id: Option<String>,
    /// The transform's error description.
    pub cause: String,
}

/// The config-derived agent state transforms rewrite (ref agent.ts
/// `Data`: the agent table + the configured default).
#[derive(Debug, Clone, Default)]
pub struct AgentState {
    /// The agent table, list order.
    pub agents: Vec<Info>,
    /// The configured default agent (`config.agent.default` shape);
    /// `None` leaves the registry's fallback chain (`build` → first
    /// selectable) in charge.
    pub default: Option<AgentId>,
}

impl AgentState {
    /// Capture one registry's current state as a transform base.
    #[must_use]
    pub fn from_registry(registry: &AgentRegistry) -> Self {
        Self { agents: registry.list(), default: registry.configured_default() }
    }
}

/// The mutable editor one transform receives (ref `agent.ts` `Editor`:
/// `list` / `get` / `default` / `update` / `remove`).
pub struct AgentStateEditor<'a> {
    state: &'a mut AgentState,
}

impl AgentStateEditor<'_> {
    /// Every agent currently in the candidate (list order).
    #[must_use]
    pub fn list(&self) -> &[Info] {
        &self.state.agents
    }

    /// One agent by id.
    #[must_use]
    pub fn get(&self, id: &AgentId) -> Option<&Info> {
        self.state.agents.iter().find(|agent| &agent.id == id)
    }

    /// Set or clear the configured default (`editor.default(id)`).
    pub fn default(&mut self, id: Option<AgentId>) {
        self.state.default = id;
    }

    /// Edit one agent in place (`editor.update(id, fn)`); returns
    /// whether the id existed (ref creates schema defaults for
    /// unknown ids — the Rust editor is rewrite-only, creation goes
    /// through the snapshot lane).
    pub fn update(&mut self, id: &AgentId, editor: impl FnOnce(&mut Info)) -> bool {
        match self.state.agents.iter_mut().find(|agent| &agent.id == id) {
            Some(agent) => {
                editor(agent);
                true
            }
            None => false,
        }
    }

    /// Remove one agent (`editor.remove(id)`); returns whether it
    /// existed.
    pub fn remove(&mut self, id: &AgentId) -> bool {
        let before = self.state.agents.len();
        self.state.agents.retain(|agent| &agent.id != id);
        if self.state.default.as_ref() == Some(id) {
            self.state.default = None;
        }
        before != self.state.agents.len()
    }
}

/// One registered state transform, scoped per plugin (ref
/// `State.transform` registration: the callback + its group).
/// The transform body: mutate the editor, return `Ok(())` (an `Err`
/// is ref's "throw").
pub type AgentTransformFn =
    Box<dyn Fn(&mut AgentStateEditor<'_>) -> Result<(), String> + Send + Sync>;

pub struct AgentTransform {
    /// Owning plugin id — `None` is an ungrouped HOST transform
    /// (its failure propagates instead of disabling a group).
    pub plugin_id: Option<String>,
    /// The transform body: mutate the editor, return `Ok(())`
    /// (a `Err` is ref's "throw").
    pub run: AgentTransformFn,
}

impl AgentTransform {
    /// An ungrouped (host) transform — failures propagate.
    #[must_use]
    pub fn host(
        run: impl Fn(&mut AgentStateEditor<'_>) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self { plugin_id: None, run: Box::new(run) }
    }

    /// A plugin-scoped transform — a failure disables every transform
    /// of `plugin_id` and the rebuild restarts from the fresh base.
    #[must_use]
    pub fn plugin(
        plugin_id: impl Into<String>,
        run: impl Fn(&mut AgentStateEditor<'_>) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self { plugin_id: Some(plugin_id.into()), run: Box::new(run) }
    }
}

impl std::fmt::Debug for AgentTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentTransform").field("plugin_id", &self.plugin_id).finish_non_exhaustive()
    }
}

/// Replay every transform against a fresh copy of `base`, in
/// registration order (ref `state.ts:172-194` `get`):
///
/// * a plugin-scoped failure disables that plugin's whole transform
///   group (this pass aborts, the group's transforms drop out, and
///   the rebuild restarts from the fresh base);
/// * an ungrouped failure propagates as `Err` (ref rethrows);
/// * grouped failures never fail the read — they are reported in the
///   returned vec (ref hands them to the group's supervisor via
///   `report`; the Rust seam logs at the call site).
pub fn run_agent_transforms(
    base: &AgentState,
    transforms: &[AgentTransform],
) -> Result<(AgentState, Vec<TransformFailure>), TransformFailure> {
    let mut disabled: HashSet<String> = HashSet::new();
    let mut failures = Vec::new();
    // Termination: every restart disables at least one plugin group,
    // and groups are finite (ref's identical argument: `disable`
    // removes registrations before the retry).
    loop {
        let mut candidate = base.clone();
        let mut restart = false;
        for transform in transforms {
            let Some(plugin_id) = transform.plugin_id.as_ref() else {
                // Ungrouped: failure propagates to the reader (ref
                // `if (!transform.group) throw cause`).
                let mut editor = AgentStateEditor { state: &mut candidate };
                if let Err(cause) = (transform.run)(&mut editor) {
                    return Err(TransformFailure { state: "agent", plugin_id: None, cause });
                }
                continue;
            };
            if disabled.contains(plugin_id) {
                continue;
            }
            let mut editor = AgentStateEditor { state: &mut candidate };
            if let Err(cause) = (transform.run)(&mut editor) {
                // Grouped: disable the plugin's whole transform
                // group and restart from a fresh candidate (ref
                // `disable(transform.group, …)` + rebuild loop).
                disabled.insert(plugin_id.clone());
                failures.push(TransformFailure {
                    state: "agent",
                    plugin_id: Some(plugin_id.clone()),
                    cause,
                });
                restart = true;
                break;
            }
        }
        if !restart {
            return Ok((candidate, failures));
        }
    }
}

/// Run `transforms` over one registry's current state and write the
/// result back (`set` changed/new rows, `remove` vanished ids,
/// `set_default` the transformed default). Grouped failures are
/// logged and contained (the surviving state applies); an ungrouped
/// failure leaves the registry untouched and returns `Err`.
///
/// Returns the number of row-level changes applied.
pub fn apply_agent_state_transforms(
    registry: &AgentRegistry,
    transforms: &[AgentTransform],
) -> Result<usize, TransformFailure> {
    if transforms.is_empty() {
        return Ok(0);
    }
    let base = AgentState::from_registry(registry);
    let base_ids: HashSet<AgentId> = base.agents.iter().map(|agent| agent.id.clone()).collect();
    let (next, failures) = run_agent_transforms(&base, transforms)?;
    for failure in &failures {
        eprintln!(
            "opencode plugin-apply: disabled state transforms of plugin {:?} ({}): {}",
            failure.plugin_id.as_deref().unwrap_or("<ungrouped>"),
            failure.state,
            failure.cause
        );
    }
    let mut applied = 0usize;
    let next_ids: HashSet<&AgentId> = next.agents.iter().map(|agent| &agent.id).collect();
    for id in &base_ids {
        if !next_ids.contains(id) {
            registry.remove(id);
            applied += 1;
        }
    }
    for agent in &next.agents {
        let changed = base.agents.iter().find(|before| before.id == agent.id) != Some(agent);
        if changed {
            registry.set(agent.clone());
            applied += 1;
        }
    }
    if base.default != next.default {
        registry.set_default(next.default.clone());
        applied += 1;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oc_schema::agent::Info;

    fn agent(id: &str, prompt: &str) -> Info {
        let mut info = Info::default_for(oc_schema::id::AgentId::parse(id));
        info.system = Some(prompt.to_owned());
        info
    }

    fn base() -> AgentState {
        AgentState {
            agents: vec![agent("build", "base build"), agent("reviewer", "base review")],
            default: Some(AgentId::parse("build")),
        }
    }

    #[test]
    fn transforms_run_in_registration_order_and_chain() {
        let transforms = vec![
            AgentTransform::plugin("a", |editor| {
                editor.update(&AgentId::parse("build"), |info| {
                    info.system = Some(format!("{}+a", info.system.clone().unwrap_or_default()));
                });
                Ok(())
            }),
            AgentTransform::plugin("b", |editor| {
                // sees a's edit — chaining, not parallel bases
                let prompt = editor
                    .get(&AgentId::parse("build"))
                    .map(|a| a.system.clone().unwrap_or_default());
                match prompt {
                    Some(prompt) if prompt.ends_with("+a") => {
                        editor.update(&AgentId::parse("build"), |info| {
                            info.system =
                                Some(format!("{}+b", info.system.clone().unwrap_or_default()));
                        });
                        editor.default(Some(AgentId::parse("reviewer")));
                        Ok(())
                    }
                    other => Err(format!("chaining broken: {other:?}")),
                }
            }),
        ];
        let (state, failures) = run_agent_transforms(&base(), &transforms).expect("ok");
        assert_eq!(failures.len(), 0);
        assert_eq!(
            state.agents.first().map(|a| a.system.clone().unwrap_or_default()),
            Some("base build+a+b".to_owned())
        );
        assert_eq!(state.default, Some(AgentId::parse("reviewer")));
    }

    #[test]
    fn grouped_failure_disables_whole_plugin_group_and_restarts_from_base() {
        let probe = AgentId::parse("build");
        let probe2 = probe.clone();
        let transforms = vec![
            // a1 succeeds first pass (poisons the candidate), then a2
            // fails → plugin "a" drops entirely and the rebuild
            // restarts from the fresh base (a1's edit must vanish).
            AgentTransform::plugin("a", move |editor| {
                editor.update(&probe, |info| {
                    info.system = Some(format!("{}+a1", info.system.clone().unwrap_or_default()));
                });
                Ok(())
            }),
            AgentTransform::plugin("a", |_| Err("a2 exploded".to_owned())),
            AgentTransform::plugin("b", move |editor| {
                editor.update(&probe2, |info| {
                    info.system = Some(format!("{}+b", info.system.clone().unwrap_or_default()));
                });
                Ok(())
            }),
        ];
        let (state, failures) = run_agent_transforms(&base(), &transforms).expect("ok");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].plugin_id.as_deref(), Some("a"));
        assert_eq!(failures[0].cause, "a2 exploded");
        // b's edit survived on a FRESH candidate (no "+a1").
        assert_eq!(
            state.agents.first().map(|a| a.system.clone().unwrap_or_default()),
            Some("base build+b".to_owned())
        );
    }

    #[test]
    fn ungrouped_failure_propagates() {
        let transforms = vec![
            AgentTransform::plugin("a", |editor| {
                editor.update(&AgentId::parse("build"), |info| {
                    info.system = Some(format!("{}x", info.system.clone().unwrap_or_default()));
                });
                Ok(())
            }),
            AgentTransform::host(|_| Err("host bug".to_owned())),
        ];
        let failure = run_agent_transforms(&base(), &transforms).expect_err("propagates");
        assert_eq!(failure.plugin_id, None);
        assert_eq!(failure.cause, "host bug");
        assert_eq!(failure.state, "agent");
    }

    #[test]
    fn remove_and_default_editing_mirror_ref_editor() {
        let transforms = vec![AgentTransform::plugin("a", |editor| {
            editor.remove(&AgentId::parse("build"));
            editor.default(Some(AgentId::parse("reviewer")));
            Ok(())
        })];
        let (state, failures) = run_agent_transforms(&base(), &transforms).expect("ok");
        assert_eq!(failures.len(), 0);
        assert_eq!(state.agents.len(), 1);
        assert_eq!(state.agents[0].id, AgentId::parse("reviewer"));
        assert_eq!(state.default, Some(AgentId::parse("reviewer")));
    }

    #[test]
    fn registry_roundtrip_applies_additive_diff_only() {
        let registry = AgentRegistry::new();
        registry.set(agent("build", "base build"));
        registry.set(agent("reviewer", "base review"));
        registry.set_default(Some(AgentId::parse("build")));
        let transforms = vec![
            AgentTransform::plugin("a", |editor| {
                editor.update(&AgentId::parse("reviewer"), |info| {
                    info.system = Some("rewritten".to_owned());
                });
                editor.remove(&AgentId::parse("build"));
                Ok(())
            }),
            AgentTransform::plugin("b", |editor| {
                let rewritten = editor
                    .get(&AgentId::parse("reviewer"))
                    .map(|a| a.system.clone().unwrap_or_default());
                match rewritten {
                    Some(ref prompt) if prompt == "rewritten" => Ok(()),
                    other => Err(format!("second transform missed first's edit: {other:?}")),
                }
            }),
        ];
        let applied = apply_agent_state_transforms(&registry, &transforms).expect("ok");
        assert_eq!(applied, 3); // build removed + reviewer rewritten + default cleared (build gone)
        assert!(registry.get(&AgentId::parse("build")).is_none());
        assert_eq!(
            registry.get(&AgentId::parse("reviewer")).map(|a| a.system.clone().unwrap_or_default()),
            Some("rewritten".into())
        );
    }

    #[test]
    fn registry_roundtrip_isolated_ungrouped_failure_leaves_registry_untouched() {
        let registry = AgentRegistry::new();
        registry.set(agent("build", "base build"));
        let transforms = vec![
            AgentTransform::host(|_| Err("boom".to_owned())),
            AgentTransform::plugin("a", |editor| {
                editor.remove(&AgentId::parse("build"));
                Ok(())
            }),
        ];
        let failure = apply_agent_state_transforms(&registry, &transforms).expect_err("propagates");
        assert_eq!(failure.cause, "boom");
        assert!(registry.get(&AgentId::parse("build")).is_some());
    }
}
