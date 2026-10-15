//! The keymap crate segment — v2's `@opentui/keymap` contract in two
//! layers:
//!
//! * [`engine`]: the dispatch ENGINE port (layers, focus scopes,
//!   modes, interceptors, pending sequences / which-key data,
//!   preventDefault semantics, rejection fall-through).
//! * [`table`]: the DATA layer — the full v2 default binding table
//!   (`config/keybind.ts` `Definitions`), user overrides, clocks.
//!
//! [`Keymap`] is the oc-tui facade over both: it installs the table's
//! layers into the engine (base = a global layer like v2's app
//! commands; `input`/`dialog`/`autocomplete`/`diff` = named modes)
//! and exposes the port's historical lookup API on top of
//! [`engine::KeymapEngine::dispatch`]. Behavior is pinned by the
//! replay suite: every table binding resolves exactly as before the
//! Wave A migration.
//!
//! Wave A lane changes to the DATA table (both mandated by the
//! interaction audit §1b, both matching v2 `config/keybind.ts`):
//! * `theme.switch` default `none` (line 92) — was drifted to
//!   `<leader>t` (the terminal pane toggle's chord in v2, line 99);
//! * `app.exit` default `ctrl+c,ctrl+d` — `<leader>q` belongs to
//!   `session.queued_prompts` (line 131).

pub mod engine;
pub mod opentui;
pub mod table;

#[cfg(test)]
mod parity_tests;

pub use engine::{
    ActiveKey, DispatchOutcome, EngineBinding, Intercept, Interceptor, Layer, LayerMode,
    PendingPart, TargetId, TargetMode,
};
pub use table::{
    default_table, log_skip, parse_keybind_overrides, take_config_errors, Chord, Clock, CommandDef,
    CommandGroup, KeyStroke, KeybindOverride, ManualClock, OwnedCommandDef, SystemClock,
    LEADER_DEFAULT, LEADER_TIMEOUT_MS,
};

use std::collections::HashMap;

/// The dispatch engine beneath the facade (lane M-engine): the
/// hand-rolled interpretation (`Legacy`) or the REAL `@opentui/keymap`
/// 0.5.14 port (`Opentui`, [`opentui::OpentuiKeymap`]) — selected once
/// at facade construction by [`crate::opentui_engine_enabled`]
/// (`OC_ENGINE_OPENTUI`). Both variants expose the same internal
/// surface; the facade's code path is identical modulo the hop.
enum EngineSel {
    Legacy(engine::KeymapEngine),
    Opentui(opentui::OpentuiKeymap),
}

impl EngineSel {
    /// The active variant per the subsystem flag.
    #[must_use]
    fn active() -> Self {
        if crate::opentui_engine_enabled() {
            Self::Opentui(opentui::OpentuiKeymap::new())
        } else {
            Self::Legacy(engine::KeymapEngine::new())
        }
    }

    /// Reset to a fresh engine of the SAME variant (a facade rebuild):
    /// the legacy path constructs a new engine; the opentui path drops
    /// the installed layers but keeps the addon set, the disable gate,
    /// and the mode stack (the timed leader re-registers on
    /// set_leader/set_timeout).
    fn fresh(&mut self) {
        match self {
            Self::Legacy(_) => *self = Self::Legacy(engine::KeymapEngine::new()),
            Self::Opentui(engine) => engine.clear_layers(),
        }
    }

    fn leader(&self) -> &KeyStroke {
        match self {
            Self::Legacy(engine) => engine.leader(),
            Self::Opentui(engine) => engine.leader(),
        }
    }

    fn set_leader(&mut self, leader: KeyStroke) {
        match self {
            Self::Legacy(engine) => engine.set_leader(leader),
            Self::Opentui(engine) => engine.set_leader(leader),
        }
    }

    fn timeout_ms(&self) -> u64 {
        match self {
            Self::Legacy(engine) => engine.timeout_ms(),
            Self::Opentui(engine) => engine.timeout_ms(),
        }
    }

    fn set_timeout(&mut self, ms: u64) {
        match self {
            Self::Legacy(engine) => engine.set_timeout(ms),
            Self::Opentui(engine) => engine.set_timeout(ms),
        }
    }

    fn command_enabled(&self, command: &str) -> bool {
        match self {
            Self::Legacy(engine) => engine.command_enabled(command),
            Self::Opentui(engine) => engine.command_enabled(command),
        }
    }

    fn set_command_enabled(&mut self, command: &str, enabled: bool) {
        match self {
            Self::Legacy(engine) => engine.set_command_enabled(command, enabled),
            Self::Opentui(engine) => engine.set_command_enabled(command, enabled),
        }
    }

    fn register_layer(&mut self, layer: Layer) -> String {
        match self {
            Self::Legacy(engine) => engine.register_layer(layer),
            Self::Opentui(engine) => engine.register_layer(layer),
        }
    }

    fn set_modes(&mut self, modes: &[&str]) {
        match self {
            Self::Legacy(engine) => engine.set_modes(modes),
            Self::Opentui(engine) => engine.set_modes(modes),
        }
    }

    fn push_mode(&mut self, mode: &str) {
        match self {
            Self::Legacy(engine) => engine.push_mode(mode),
            Self::Opentui(engine) => engine.push_mode(mode),
        }
    }

    fn pop_mode(&mut self) {
        match self {
            Self::Legacy(engine) => engine.pop_mode(),
            Self::Opentui(engine) => engine.pop_mode(),
        }
    }

    fn current_mode(&self) -> &str {
        match self {
            Self::Legacy(engine) => engine.current_mode(),
            Self::Opentui(engine) => engine.current_mode(),
        }
    }

    fn pending_display(&mut self) -> Option<String> {
        match self {
            Self::Legacy(engine) => engine.pending_display(),
            Self::Opentui(engine) => engine.pending_display(),
        }
    }

    fn expire(&mut self, clock: &dyn Clock) -> bool {
        match self {
            Self::Legacy(engine) => engine.expire(clock),
            Self::Opentui(engine) => engine.expire(clock),
        }
    }

    fn clear_pending(&mut self) {
        match self {
            Self::Legacy(engine) => engine.clear_pending(),
            Self::Opentui(engine) => engine.clear_pending(),
        }
    }

    fn is_leader(&self, stroke: &KeyStroke) -> bool {
        match self {
            Self::Legacy(engine) => engine.is_leader(stroke),
            Self::Opentui(engine) => engine.is_leader(stroke),
        }
    }

    fn dispatch_base_only(
        &mut self,
        stroke: KeyStroke,
        clock: &dyn Clock,
        runner: &mut dyn FnMut(&str, Option<&str>) -> bool,
    ) -> DispatchOutcome {
        match self {
            Self::Legacy(engine) => engine.dispatch_base_only(stroke, clock, runner),
            Self::Opentui(engine) => engine.dispatch_base_only(stroke, clock, runner),
        }
    }

    fn dispatch(&mut self, stroke: KeyStroke, clock: &dyn Clock) -> DispatchOutcome {
        match self {
            Self::Legacy(engine) => engine.dispatch(stroke, clock),
            Self::Opentui(engine) => engine.dispatch(stroke, clock),
        }
    }
}


/// Assert a leader-arm outcome, accepting either display form: the
/// legacy engine shows the raw stroke ("ctrl+x"), the opentui engine
/// the `<leader>` token (upstream getPendingSequence's display —
/// v2's own form). The pinned semantic is the PENDING itself.
#[cfg(test)]
pub(crate) fn assert_pending_leader(outcome: &Outcome) {
    match outcome {
        Outcome::Pending { display } => assert!(
            display == "ctrl+x" || display == "<leader>",
            "leader display form ({display})"
        ),
        other => panic!("expected the leader to arm, got {other:?}"),
    }
}

/// Result of feeding one stroke to the keymap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A command resolved (with optional invocation args).
    Command { command: String, args: Option<String> },
    /// The stroke extended/started a pending sequence; `display` feeds
    /// the pending-indicator UI.
    Pending { display: String },
    /// A pending sequence was cleared (escape or timeout).
    Cancelled,
    /// No binding matched; the stroke passes through.
    Unhandled,
}

/// The keymap facade: the v2 default table + leader sequencing + mode
/// stack installed into the [`engine::KeymapEngine`], plus the
/// user-override merge (`cli.json` → `keybinds`) and the enable/
/// disable gate (the reference's `Keymap.createLayer({ enabled })`).
///
/// Layer mapping: table layer `"base"` registers as a GLOBAL engine
/// layer (v2's app command layer, `mode: "global"` — app.tsx:1218);
/// every other table layer (`input`, `dialog`, `autocomplete`,
/// `diff`) registers as a NAMED mode reachable only while current
/// (`registerLayerFields`' `require(opencode.mode, …)`). The
/// plugin-owned bindings ride their own lower-priority layer per mode
/// (resolved after the static table, exactly like the pre-Wave-A
/// behavior: a plugin chord never shadows a built-in).
pub struct Keymap {
    engine: EngineSel,
    /// layer name → (chord, command) rules (the static table's live
    /// state — overrides mutate, rebuilds re-install).
    layers: HashMap<&'static str, Vec<(Chord, String)>>,
    /// registry by dot command (for palette + binding display).
    registry: HashMap<&'static str, &'static CommandDef>,
    /// Effective binding specs per dot command (defaults overridden by
    /// the user table) — the palette's binding display reads these.
    effective: HashMap<String, Vec<String>>,
    /// The OWNED (plugin) layer rules, keyed by layer name.
    owned_layers: HashMap<String, Vec<(Chord, String)>>,
    /// Owned registry (insertion order preserved for the palette).
    owned_registry: Vec<OwnedCommandDef>,
    modes: Vec<String>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

impl Keymap {
    /// Keymap with the full v2 default table.
    #[must_use]
    pub fn new() -> Self {
        Self::with_table(default_table())
    }

    #[must_use]
    pub fn with_table(table: &'static [CommandDef]) -> Self {
        let leader = KeyStroke::parse(LEADER_DEFAULT).expect("leader parses");
        let mut layers: HashMap<&'static str, Vec<(Chord, String)>> = HashMap::new();
        let mut registry = HashMap::new();
        let mut effective = HashMap::new();
        for def in table {
            registry.insert(def.command, def);
            for spec in def.defaults.split(',') {
                let spec = spec.trim();
                if spec.is_empty() || spec == "none" {
                    continue;
                }
                if let Some(chord) = Chord::parse(spec, &leader) {
                    layers.entry(def.layer).or_default().push((chord, def.command.to_owned()));
                }
            }
            effective.insert(
                def.command.to_owned(),
                def.defaults
                    .split(',')
                    .map(str::trim)
                    .filter(|spec| !spec.is_empty() && *spec != "none")
                    .map(str::to_owned)
                    .collect(),
            );
        }
        let mut keymap = Self {
            engine: EngineSel::active(),
            layers,
            registry,
            effective,
            owned_layers: HashMap::new(),
            owned_registry: Vec::new(),
            modes: Vec::new(),
        };
        keymap.rebuild_engine();
        keymap
    }

    /// (Re)install the engine layers from the live rule sets: one
    /// layer per table layer name (static rules first, the owned
    /// plugin rules appended behind them at a lower priority), then
    /// restore the mode stack. Called after any table mutation
    /// (overrides, leader rebind, owned registration).
    fn rebuild_engine(&mut self) {
        let leader = self.engine.leader().clone();
        let timeout = self.engine.timeout_ms();
        let disabled = self.engine_disabled_snapshot();
        self.engine.fresh();
        self.engine.set_leader(leader);
        self.engine.set_timeout(timeout);
        for command in disabled {
            self.engine.set_command_enabled(&command, false);
        }
        let mut names: Vec<&'static str> = self.layers.keys().copied().collect();
        names.sort_unstable();
        // The base layer registers FIRST: the engine sorts
        // later-registered layers first (getSortedLayers' order-desc
        // tie-break), so base resolves LAST — named modes shadow
        // base (the pre-Wave-A resolution order the replay suite
        // pins: modes top-down, then base).
        names.sort_by_key(|name| u8::from(*name != "base"));
        for name in names {
            let mode = if name == "base" { "global" } else { name };
            let mut bindings = Vec::new();
            for (chord, command) in &self.layers[name] {
                bindings.push(engine::EngineBinding {
                    chord: chord.clone(),
                    command: command.clone(),
                    prevent_default: self
                        .registry
                        .get(command.as_str())
                        .is_some_and(|def| def.prevent_default),
                    fallthrough: false,
                });
            }
            self.engine.register_layer(engine::Layer {
                name: format!("table:{name}"),
                target: None,
                target_mode: engine::TargetMode::Path,
                priority: 0,
                order: 0,
                mode: engine::LayerMode::from_name(mode),
                enabled: true,
                bindings,
            });
        }
        // The owned (plugin) layer per mode: priority −1 — the engine
        // sorts priority DESC, so the static table always shadows.
        let mut owned_names: Vec<&String> = self.owned_layers.keys().collect();
        owned_names.sort();
        for name in owned_names {
            let mode = if name == "base" { "global" } else { name.as_str() };
            let bindings = self.owned_layers[name]
                .iter()
                .map(|(chord, command)| engine::EngineBinding {
                    chord: chord.clone(),
                    command: command.clone(),
                    prevent_default: true,
                    fallthrough: false,
                })
                .collect();
            self.engine.register_layer(engine::Layer {
                name: format!("owned:{name}"),
                target: None,
                target_mode: engine::TargetMode::Path,
                priority: -1,
                order: 0,
                mode: engine::LayerMode::from_name(mode),
                enabled: true,
                bindings,
            });
        }
        let modes: Vec<String> = self.modes.clone();
        let refs: Vec<&str> = modes.iter().map(String::as_str).collect();
        self.engine.set_modes(&refs);
    }

    /// The commands currently disabled in the engine (carried across
    /// rebuilds — an override re-merge must not re-enable the
    /// conditional `app.exit` layer's gate).
    fn engine_disabled_snapshot(&self) -> Vec<String> {
        default_table()
            .iter()
            .filter(|def| !self.engine.command_enabled(def.command))
            .map(|def| def.command.to_owned())
            .collect()
    }

    /// Register one OWNED (plugin) command entry. Re-registering the
    /// same command id replaces it (hot reload); chords land in the
    /// owned layer alongside the static ones — lookups consult the
    /// static table FIRST, the owned entries SECOND.
    pub fn register_owned(&mut self, def: OwnedCommandDef) {
        // Replace-in-place keeps palette ordering stable.
        if let Some(existing) =
            self.owned_registry.iter().position(|entry| entry.command == def.command)
        {
            self.owned_registry[existing] = def.clone();
        } else {
            self.owned_registry.push(def.clone());
        }
        let layer = if def.layer.is_empty() { "base".to_owned() } else { def.layer };
        if let Some(rules) = self.owned_layers.get_mut(&layer) {
            rules.retain(|(_, command)| *command != def.command);
        }
        for spec in &def.defaults {
            // Plugin-facing convenience: accept "ctrl+x h" (space
            // style, the display form) as well as the canonical
            // "ctrl+x>h" / "<leader>h" spec syntax.
            let normalized = if spec.contains(' ') { spec.replace(' ', ">") } else { spec.clone() };
            if let Some(chord) = Chord::parse(&normalized, self.engine.leader()) {
                self.owned_layers
                    .entry(layer.clone())
                    .or_default()
                    .push((chord, def.command.clone()));
            } else {
                log_skip(&format!("plugin keybind {spec:?}: unparseable stroke (skipped)"));
            }
        }
        self.rebuild_engine();
    }

    /// Drop every owned (plugin) entry — full plugin-registry refresh.
    pub fn clear_owned(&mut self) {
        self.owned_layers.clear();
        self.owned_registry.clear();
        self.rebuild_engine();
    }

    /// The owned (plugin) registry, insertion order.
    #[must_use]
    pub fn owned_commands(&self) -> &[OwnedCommandDef] {
        &self.owned_registry
    }

    /// Apply user keybind overrides (`parse_keybind_overrides` output):
    /// user bindings replace the defaults for that command (unset
    /// removes them); the `leader` override rebinds the leader stroke.
    /// Unknown keys are logged + skipped (defaults stand). Returns the
    /// number of overrides applied.
    pub fn apply_overrides(&mut self, overrides: &[KeybindOverride]) -> usize {
        let mut applied = 0usize;
        for entry in overrides {
            if entry.name == "leader" {
                if let Some(spec) = entry.specs.first() {
                    if let Some(stroke) = KeyStroke::parse(spec.trim()) {
                        self.rebind_leader(stroke);
                        applied += 1;
                    }
                }
                continue;
            }
            // v2 dot command first, v1 bind name second.
            let def = self
                .registry
                .values()
                .find(|def| def.command == entry.name || def.bind_name == entry.name)
                .copied();
            let Some(def) = def else {
                log_skip(&format!("keybind {:?}: unknown command (skipped)", entry.name));
                continue;
            };
            // Remove every existing chord for the command, then bind
            // the override specs in the command's layer.
            for rules in self.layers.values_mut() {
                rules.retain(|(_, command)| command != def.command);
            }
            for spec in &entry.specs {
                if let Some(chord) = Chord::parse(spec, self.engine.leader()) {
                    self.layers.entry(def.layer).or_default().push((chord, def.command.to_owned()));
                } else {
                    log_skip(&format!("keybind {spec:?}: unparseable stroke (skipped)"));
                }
            }
            self.effective.insert(def.command.to_owned(), entry.specs.clone());
            applied += 1;
        }
        self.rebuild_engine();
        applied
    }

    /// Rebind the leader stroke: chords mentioning `<leader>` are
    /// stored as SPECS in `effective` and re-parsed lazily against the
    /// current leader, so a leader override re-binds every affected
    /// chord here.
    fn rebind_leader(&mut self, leader: KeyStroke) {
        self.engine.set_leader(leader);
        let defs: Vec<&'static CommandDef> = self.registry.values().copied().collect();
        let effective = std::mem::take(&mut self.effective);
        for def in defs {
            let Some(specs) = effective.get(def.command) else { continue };
            if !specs.iter().any(|spec| spec.contains("leader")) {
                continue;
            }
            for rules in self.layers.values_mut() {
                rules.retain(|(_, command)| command != def.command);
            }
            for spec in specs {
                if let Some(chord) = Chord::parse(spec, self.engine.leader()) {
                    self.layers.entry(def.layer).or_default().push((chord, def.command.to_owned()));
                }
            }
        }
        self.effective = effective;
        self.rebuild_engine();
    }

    /// Disable/enable one command's bindings (the reference's
    /// conditional layers — e.g. `app.exit` gated on the empty
    /// prompt): a disabled command never matches.
    pub fn set_command_enabled(&mut self, command: &str, enabled: bool) {
        self.engine.set_command_enabled(command, enabled);
    }

    #[must_use]
    pub fn command_enabled(&self, command: &str) -> bool {
        self.engine.command_enabled(command)
    }

    /// Override the leader timeout (tests / user config `leader.timeout`).
    pub fn set_timeout(&mut self, ms: u64) {
        self.engine.set_timeout(ms);
    }

    #[must_use]
    pub fn is_leader(&self, stroke: &KeyStroke) -> bool {
        self.engine.is_leader(stroke)
    }

    /// Replace the whole mode stack (the pump recomputes it from
    /// scratch each stroke — dialog over input over base — avoiding
    /// LIFO ordering hazards when layers appear/disappear together).
    /// A change clears any half-typed sequence.
    pub fn set_modes(&mut self, modes: &[&str]) {
        let wanted: Vec<String> = modes.iter().map(|mode| (*mode).to_owned()).collect();
        if wanted != self.modes {
            self.modes = wanted;
            let refs: Vec<&str> = self.modes.iter().map(String::as_str).collect();
            self.engine.set_modes(&refs);
        }
    }

    /// Push an input mode (dialog, autocomplete, …); returns a guard id
    /// to pop with [`Keymap::pop_mode`].
    pub fn push_mode(&mut self, mode: &str) {
        self.modes.push(mode.to_owned());
        self.engine.push_mode(mode);
    }

    /// Pop the most recently pushed input mode.
    pub fn pop_mode(&mut self) {
        self.modes.pop();
        self.engine.pop_mode();
    }

    #[must_use]
    pub fn current_mode(&self) -> &str {
        self.engine.current_mode()
    }

    /// The engine handle (the which-key data surface + focus-scope
    /// configuration — Wave A's contract surface for later UI). The
    /// opentui engine path exposes its own active-keys surface on the
    /// facade; the legacy handle is `Some` on the legacy path only.
    #[must_use]
    pub fn legacy_engine(&self) -> Option<&engine::KeymapEngine> {
        match &self.engine {
            EngineSel::Legacy(engine) => Some(engine),
            EngineSel::Opentui(_) => None,
        }
    }

    /// Pending sequence display ("ctrl+x"), if any.
    #[must_use]
    pub fn pending_display(&mut self) -> Option<String> {
        self.engine.pending_display()
    }

    /// Expire a stale pending sequence; `true` when one expired.
    pub fn expire(&mut self, clock: &dyn Clock) -> bool {
        self.engine.expire(clock)
    }

    /// Resolve one stroke against the **base layer only** (the
    /// conditional `app.exit` layer sits above the managed textarea
    /// layer, so its chords must resolve before any input-mode
    /// shadowing) — same pending/leader machinery as [`Keymap::lookup`].
    pub fn lookup_base(&mut self, stroke: KeyStroke, clock: &dyn Clock) -> Outcome {
        let outcome = self.engine.dispatch_base_only(stroke, clock, &mut |_, _| true);
        if let Some((command, args)) = outcome.command {
            return Outcome::Command { command, args };
        }
        if let Some(display) = outcome.pending {
            return Outcome::Pending { display };
        }
        if outcome.cancelled {
            return Outcome::Cancelled;
        }
        Outcome::Unhandled
    }

    /// Drop any pending leader sequence (a composer-consumed stroke
    /// invalidates it — the reference's textarea layer swallows the
    /// continuation).
    pub fn clear_pending(&mut self) {
        self.engine.clear_pending();
    }

    /// The which-key surface (`getActiveKeys`): the reachable next
    /// keys from the pending sequence (K-8: the opentui engine lists
    /// them at the root too — the legacy engine returns empty there).
    #[must_use]
    pub fn active_keys(&mut self) -> Vec<ActiveKey> {
        match &mut self.engine {
            EngineSel::Legacy(engine) => engine.active_keys(),
            EngineSel::Opentui(engine) => engine.active_keys(),
        }
    }

    /// Resolve one stroke. See the module docs for the escape and leader
    /// special-cases.
    pub fn lookup(&mut self, stroke: KeyStroke, clock: &dyn Clock) -> Outcome {
        let outcome = self.engine.dispatch(stroke, clock);
        if let Some((command, args)) = outcome.command {
            return Outcome::Command { command, args };
        }
        if let Some(display) = outcome.pending {
            return Outcome::Pending { display };
        }
        if outcome.cancelled {
            return Outcome::Cancelled;
        }
        Outcome::Unhandled
    }

    /// Palette-visible commands (grouped order: the registry order).
    #[must_use]
    pub fn palette_commands(&self) -> Vec<&'static CommandDef> {
        default_table().iter().filter(|def| def.palette).collect()
    }

    /// Display bindings for a command ("ctrl+x n" style), in table
    /// order — the user overrides' effective specs (not the defaults);
    /// owned (plugin) entries resolve from their registered specs.
    #[must_use]
    pub fn bindings_for(&self, command: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(def) = self.registry.get(command) {
            let empty = Vec::new();
            let specs = self.effective.get(def.command).unwrap_or(&empty);
            for spec in specs {
                let spec = spec.trim();
                if spec.is_empty() || spec == "none" {
                    continue;
                }
                if let Some(chord) = Chord::parse(spec, self.engine.leader()) {
                    out.push(chord.display());
                }
            }
            return out;
        }
        if let Some(owned) = self.owned_registry.iter().find(|entry| entry.command == command) {
            for spec in &owned.defaults {
                let normalized =
                    if spec.contains(' ') { spec.replace(' ', ">") } else { spec.clone() };
                if let Some(chord) = Chord::parse(&normalized, self.engine.leader()) {
                    out.push(chord.display());
                }
            }
        }
        out
    }

    /// Whether `stroke` triggers `command` under the EFFECTIVE
    /// bindings (overrides honored): the stroke-level matcher behind
    /// dialog-scoped commands the generic layer lookup cannot
    /// disambiguate (e.g. `dialog.move_session.delete` vs the earlier
    /// `queued_prompt.delete` on the same `ctrl+d` — v2 binds the
    /// `DialogSelect` actions in their own higher-priority keymap layer).
    #[must_use]
    pub fn stroke_triggers(&self, stroke: &KeyStroke, command: &str) -> bool {
        let Some(def) = self.registry.get(command) else { return false };
        let empty = Vec::new();
        let specs = self.effective.get(def.command).unwrap_or(&empty);
        specs.iter().any(|spec| {
            let spec = spec.trim();
            !spec.is_empty()
                && spec != "none"
                && Chord::parse(spec, self.engine.leader())
                    .is_some_and(|chord| chord.strokes.len() == 1 && &chord.strokes[0] == stroke)
        })
    }

    #[must_use]
    pub fn command_def(&self, command: &str) -> Option<&'static CommandDef> {
        self.registry.get(command).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(spec: &str) -> KeyStroke {
        KeyStroke::parse(spec).expect("test stroke parses")
    }

    #[test]
    fn default_table_is_complete_and_parses() {
        let table = default_table();
        // Spot anchors from tui-map §4 + the full table shape.
        assert!(table.len() > 150, "full v2 table, got {}", table.len());
        let keymap = Keymap::new();
        for def in table {
            assert!(!def.bind_name.is_empty() && !def.command.is_empty());
            for spec in def.defaults.split(',') {
                let spec = spec.trim();
                if spec.is_empty() || spec == "none" {
                    continue;
                }
                assert!(
                    Chord::parse(spec, &KeyStroke::parse("ctrl+x").unwrap()).is_some(),
                    "{spec} parses"
                );
            }
            assert_eq!(
                keymap.command_def(def.command).map(|found| found.command),
                Some(def.command),
                "{} registered",
                def.command
            );
        }
        // Dot-command spot checks from CommandMap.
        assert_eq!(keymap.command_def("command.palette.show").unwrap().bind_name, "command_list");
        assert_eq!(
            keymap.command_def("session.tab.select.10").unwrap().defaults,
            "<leader>0,ctrl+0"
        );
    }

    #[test]
    fn top_bindings_resolve() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        let cases: &[(&str, &str)] = &[
            ("ctrl+p", "command.palette.show"),
            ("ctrl+o", "open.menu"),
            ("ctrl+tab", "session.tab.next"),
            ("alt+down", "session.tab.next"),
            ("ctrl+shift+tab", "session.tab.previous"),
            ("alt+up", "session.tab.previous"),
            ("ctrl+shift+t", "session.tab.reopen"),
            ("ctrl+r", "session.rename"),
            ("ctrl+b", "session.background"),
            ("ctrl+t", "variant.cycle"),
            ("f2", "model.cycle_recent"),
            ("shift+f2", "model.cycle_recent_reverse"),
            ("escape", "session.interrupt"),
            ("ctrl+z", "terminal.suspend"),
            ("ctrl+1", "session.tab.select.1"),
            ("ctrl+0", "session.tab.select.10"),
            ("pageup", "session.page.up"),
            ("alt+shift+down", "session.tab.next_unread"),
        ];
        for (spec, command) in cases {
            assert_eq!(
                keymap.lookup(stroke(spec), &clock),
                Outcome::Command { command: (*command).to_owned(), args: None },
                "{spec} → {command}"
            );
        }
    }

    #[test]
    fn leader_chords_resolve() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        // <leader>q decollided (Wave A): the chord belongs to
        // session.queued_prompts, never app.exit.
        assert_pending_leader(&keymap.lookup(stroke("ctrl+x"), &clock));
        assert_eq!(
            keymap.lookup(stroke("q"), &clock),
            Outcome::Command { command: "session.queued_prompts".to_owned(), args: None }
        );
        // n → session.new; e → prompt.editor; m → model.list; w → tab close.
        for (chord_tail, command) in [
            ("n", "session.new"),
            ("e", "prompt.editor"),
            ("m", "model.list"),
            ("w", "session.tab.close"),
        ] {
            assert_pending_leader(&keymap.lookup(stroke("ctrl+x"), &clock));
            assert_eq!(
                keymap.lookup(stroke(chord_tail), &clock),
                Outcome::Command { command: command.to_owned(), args: None },
                "ctrl+x {chord_tail}"
            );
        }
        // The pending display extends for multi-stroke prefixes.
        assert_pending_leader(&keymap.lookup(stroke("ctrl+x"), &clock));
    }

    #[test]
    fn leader_timeout_expires_pending() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        clock.advance(LEADER_TIMEOUT_MS);
        assert!(keymap.expire(&clock), "pending expired at the timeout");
        assert_eq!(keymap.pending_display(), None);
        // After expiry a plain stroke resolves normally (no stale prefix).
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "session.child.first".to_owned(), args: None }
        );
        // One ms before the timeout it is still live.
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        clock.advance(LEADER_TIMEOUT_MS - 1);
        assert!(!keymap.expire(&clock));
    }

    #[test]
    fn escape_clears_pending_then_interrupts_when_idle() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(keymap.lookup(stroke("escape"), &clock), Outcome::Cancelled);
        // With nothing pending, escape interrupts the session.
        assert_eq!(
            keymap.lookup(stroke("escape"), &clock),
            Outcome::Command { command: "session.interrupt".to_owned(), args: None }
        );
    }

    #[test]
    fn unmatched_continuation_falls_through() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        // ctrl+x z has no chord; the z is then evaluated fresh — nothing.
        assert_eq!(keymap.lookup(stroke("z"), &clock), Outcome::Unhandled);
        assert_eq!(keymap.pending_display(), None);
    }

    #[test]
    fn mode_stack_overrides_base() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        // Base: down → subagent picker toggle.
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "session.child.first".to_owned(), args: None }
        );
        keymap.push_mode("input");
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "input.move.down".to_owned(), args: None }
        );
        assert_eq!(
            keymap.lookup(stroke("return"), &clock),
            Outcome::Command { command: "input.submit".to_owned(), args: None }
        );
        keymap.push_mode("autocomplete");
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "prompt.autocomplete.next".to_owned(), args: None }
        );
        keymap.pop_mode();
        keymap.pop_mode();
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "session.child.first".to_owned(), args: None }
        );
    }

    #[test]
    fn dialog_layer_shadows_base_conflicts() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.push_mode("dialog");
        // ctrl+p inside a dialog moves the selection, not the palette.
        assert_eq!(
            keymap.lookup(stroke("ctrl+p"), &clock),
            Outcome::Command { command: "dialog.select.prev".to_owned(), args: None }
        );
    }

    #[test]
    fn binding_displays_use_leaders() {
        let keymap = Keymap::new();
        assert_eq!(keymap.bindings_for("session.new"), vec!["ctrl+x n".to_owned()]);
        assert_eq!(
            keymap.bindings_for("session.tab.select.10"),
            vec!["ctrl+x 0".to_owned(), "ctrl+0".to_owned()]
        );
        // Wave A decollision: app.exit keeps ctrl+c/ctrl+d only.
        assert_eq!(keymap.bindings_for("app.exit"), vec!["ctrl+c".to_owned(), "ctrl+d".to_owned()]);
    }

    #[test]
    fn palette_lists_grouped_commands() {
        let keymap = Keymap::new();
        let palette = keymap.palette_commands();
        assert!(palette
            .iter()
            .any(|def| def.command == "session.new" && def.group == Some(CommandGroup::Session)));
        assert!(palette
            .iter()
            .any(|def| def.command == "model.list" && def.group == Some(CommandGroup::Agent)));
        assert!(palette.iter().all(|def| def.palette));
    }
}

#[cfg(test)]
mod dialog_layer_tests {
    use super::*;

    fn stroke(spec: &str) -> KeyStroke {
        KeyStroke::parse(spec).expect("test stroke parses")
    }

    /// The `dialog.*` navigation table resolves inside the dialog mode
    /// (the modal layer's mover bindings).
    #[test]
    fn dialog_navigation_table_resolves_in_dialog_mode() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.push_mode("dialog");
        let cases: &[(&str, &str)] = &[
            ("up", "dialog.select.prev"),
            ("ctrl+p", "dialog.select.prev"),
            ("down", "dialog.select.next"),
            ("ctrl+n", "dialog.select.next"),
            ("pageup", "dialog.select.page_up"),
            ("pagedown", "dialog.select.page_down"),
            ("home", "dialog.select.home"),
            ("end", "dialog.select.end"),
            ("return", "dialog.select.submit"),
            ("ctrl+d", "queued_prompt.delete"), // first ctrl+d rule in table order
        ];
        for (spec, command) in cases {
            assert_eq!(
                keymap.lookup(stroke(spec), &clock),
                Outcome::Command { command: (*command).to_owned(), args: None },
                "{spec} → {command}"
            );
        }
    }

    /// The `move_session` defaults follow the operative v2 table (the
    /// dialog-select golden footer shows "new ctrl+a").
    #[test]
    fn move_session_defaults_follow_the_v2_table() {
        let keymap = Keymap::new();
        assert_eq!(keymap.bindings_for("dialog.move_session.new"), vec!["ctrl+a".to_owned()]);
        assert_eq!(keymap.bindings_for("dialog.move_session.delete"), vec!["ctrl+d".to_owned()]);
    }

    /// D-05 — `input_paste` ships the binding-OBJECT form
    /// `{ key: "ctrl+v", preventDefault: false }` (config/keybind.ts:
    /// 205): the only default whose keymap layer does not
    /// auto-suppress the matched event — the paste handler prevents
    /// it itself (prompt/index.tsx:470-492). Every other default is a
    /// plain string (preventDefault true).
    #[test]
    fn input_paste_is_the_only_no_prevent_default_binding() {
        let mut no_prevent: Vec<&str> = default_table()
            .iter()
            .filter(|def| !def.prevent_default)
            .map(|def| def.bind_name)
            .collect();
        no_prevent.sort_unstable();
        assert_eq!(no_prevent, vec!["input_paste"]);
        let paste = default_table().iter().find(|def| def.bind_name == "input_paste").unwrap();
        assert_eq!(paste.defaults, "ctrl+v");
        assert_eq!(paste.layer, "input");
    }

    /// The Wave A default drifts: `theme.switch` is UNBOUND in v2
    /// (config/keybind.ts:92) — `<leader>t` is the terminal pane
    /// toggle's chord there (line 99), never the theme list's.
    #[test]
    fn theme_switch_default_is_none() {
        let theme = default_table().iter().find(|def| def.command == "theme.switch").unwrap();
        assert_eq!(theme.defaults, "none");
        let keymap = Keymap::new();
        assert_eq!(keymap.bindings_for("theme.switch"), Vec::<String>::new());
    }

    /// R-05 — the Worktrees dialog resolves its action commands by
    /// their own effective bindings (`stroke_triggers`): the
    /// same-layer table conflict (`queued_prompt.delete` also claims
    /// ctrl+d, earlier in the table) must not shadow the dialog's
    /// actions.
    #[test]
    fn stroke_triggers_resolves_move_session_commands() {
        let keymap = Keymap::new();
        let ctrl = |spec: &str| KeyStroke::parse(spec).expect("stroke");
        assert!(keymap.stroke_triggers(&ctrl("ctrl+d"), "dialog.move_session.delete"));
        assert!(keymap.stroke_triggers(&ctrl("ctrl+a"), "dialog.move_session.new"));
        assert!(keymap.stroke_triggers(&ctrl("ctrl+m"), "dialog.move_session.move"));
        assert!(keymap.stroke_triggers(&ctrl("ctrl+r"), "dialog.move_session.refresh"));
        assert!(!keymap.stroke_triggers(&ctrl("ctrl+d"), "dialog.move_session.new"));
        assert!(!keymap.stroke_triggers(&ctrl("ctrl+x"), "dialog.move_session.delete"));
    }
}

#[cfg(test)]
mod override_tests {
    use super::*;

    fn stroke(spec: &str) -> KeyStroke {
        KeyStroke::parse(spec).expect("test stroke parses")
    }

    fn overrides(json: &serde_json::Value) -> Vec<KeybindOverride> {
        parse_keybind_overrides(json)
    }

    /// User wins over defaults, unset removes, comma alternatives all
    /// bind, and the palette's binding display reflects the override.
    #[test]
    fn user_overrides_replace_defaults() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.apply_overrides(&overrides(&serde_json::json!({
            "app.exit": "ctrl+q,<leader>z",
            "command.palette.show": "none",
            "session.rename": ["ctrl+shift+r", "f9"],
        })));
        // ctrl+q exits now.
        assert_eq!(
            keymap.lookup(stroke("ctrl+q"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None }
        );
        // <leader>z too (multi-stroke alternative).
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(
            keymap.lookup(stroke("z"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None }
        );
        // The old ctrl+c default is REPLACED (user wins, not merged).
        assert_eq!(keymap.lookup(stroke("ctrl+c"), &clock), Outcome::Unhandled);
        // "none" unsets: ctrl+p no longer opens the palette.
        assert_eq!(keymap.lookup(stroke("ctrl+p"), &clock), Outcome::Unhandled);
        // Array form binds every alternative.
        assert_eq!(
            keymap.lookup(stroke("ctrl+shift+r"), &clock),
            Outcome::Command { command: "session.rename".to_owned(), args: None }
        );
        assert_eq!(
            keymap.lookup(stroke("f9"), &clock),
            Outcome::Command { command: "session.rename".to_owned(), args: None }
        );
        // Binding displays reflect the overrides.
        assert_eq!(
            keymap.bindings_for("app.exit"),
            vec!["ctrl+q".to_owned(), "ctrl+x z".to_owned()]
        );
        assert_eq!(keymap.bindings_for("command.palette.show"), Vec::<String>::new());
    }

    /// The v1 bind-name keys are accepted as aliases of the dot ids.
    #[test]
    fn v1_bind_names_are_accepted() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.apply_overrides(&overrides(&serde_json::json!({ "command_list": "ctrl+shift+p" })));
        assert_eq!(
            keymap.lookup(stroke("ctrl+shift+p"), &clock),
            Outcome::Command { command: "command.palette.show".to_owned(), args: None }
        );
    }

    /// Unknown keys and unparseable values are skipped (logged), never
    /// fatal; the default bindings keep working.
    #[test]
    fn unknown_and_invalid_entries_are_skipped() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.apply_overrides(&overrides(&serde_json::json!({
            "no.such.command": "ctrl+q",
            "app.exit": 42,
            "app.console": { "event": "press" },
        })));
        assert_eq!(
            keymap.lookup(stroke("ctrl+c"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None },
            "app.exit default intact"
        );
        // The `{key}`-less BindingObject shape is skipped but a proper
        // one binds.
        keymap
            .apply_overrides(&overrides(&serde_json::json!({ "app.exit": [{ "key": "ctrl+q" }] })));
        assert_eq!(
            keymap.lookup(stroke("ctrl+q"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None }
        );
    }

    /// F5: bad keybinds land in the sink (the oc-tui layer shapes them
    /// into toasts).
    #[test]
    fn config_errors_land_in_the_sink() {
        let _ = take_config_errors(); // isolate the sink
        let _ = parse_keybind_overrides(&serde_json::json!({ "app.exit": 42 }));
        let errors = take_config_errors();
        assert!(
            errors.iter().any(|reason| reason.starts_with("keybind \"app.exit\"")),
            "the unparseable value recorded: {errors:?}"
        );
        assert!(take_config_errors().is_empty(), "the drain is total");
    }

    /// The leader override rebinds every `<leader>` chord.
    #[test]
    fn leader_override_rebinds_chords() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.apply_overrides(&overrides(&serde_json::json!({ "leader": "ctrl+\\" })));
        assert!(matches!(keymap.lookup(stroke("ctrl+\\"), &clock), Outcome::Pending { .. }));
        assert_eq!(
            keymap.lookup(stroke("n"), &clock),
            Outcome::Command { command: "session.new".to_owned(), args: None },
            "leader+n resolves under the new leader"
        );
        assert!(keymap.bindings_for("session.new").contains(&"ctrl+\\ n".to_owned()));
    }

    /// Disabled commands never match — the reference's conditional
    /// layer (`app.exit` off while the prompt has text). After the
    /// Wave A decollision, disabling app.exit leaves `<leader>q` on
    /// the queue manager either way.
    #[test]
    fn disabled_commands_do_not_match() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.set_command_enabled("app.exit", false);
        assert_eq!(keymap.lookup(stroke("ctrl+c"), &clock), Outcome::Unhandled);
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(
            keymap.lookup(stroke("q"), &clock),
            Outcome::Command { command: "session.queued_prompts".to_owned(), args: None }
        );
        keymap.set_command_enabled("app.exit", true);
        assert_eq!(
            keymap.lookup(stroke("ctrl+c"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None }
        );
    }

    /// A disable gate SURVIVES an override re-merge (the rebuild
    /// carries the conditional state across).
    #[test]
    fn disable_gate_survives_override_remerge() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.set_command_enabled("app.exit", false);
        keymap.apply_overrides(&overrides(&serde_json::json!({ "session.rename": "f9" })));
        assert_eq!(keymap.lookup(stroke("ctrl+c"), &clock), Outcome::Unhandled);
    }

    fn owned(command: &str, defaults: &[&str]) -> OwnedCommandDef {
        OwnedCommandDef {
            command: command.to_owned(),
            description: format!("{command} description"),
            defaults: defaults.iter().map(|spec| (*spec).to_owned()).collect(),
            layer: "base".to_owned(),
            palette: true,
            plugin: "helloworld".to_owned(),
        }
    }

    #[test]
    fn owned_entries_resolve_after_static() {
        let mut keymap = Keymap::new();
        let clock = ManualClock::default();
        keymap.register_owned(owned("hw.hello", &["ctrl+x h"]));

        // The plugin chord resolves (leader sequence).
        assert_pending_leader(&keymap.lookup(stroke("ctrl+x"), &clock));
        assert_eq!(
            keymap.lookup(stroke("h"), &clock),
            Outcome::Command { command: "hw.hello".to_owned(), args: None }
        );
        assert_eq!(keymap.bindings_for("hw.hello"), vec!["ctrl+x h".to_owned()]);
        assert_eq!(keymap.owned_commands().len(), 1);

        // Static wins on chord collision: a plugin binding for ctrl+p
        // never shadows the palette.
        keymap.register_owned(owned("hw.grab", &["ctrl+p"]));
        assert_eq!(
            keymap.lookup(stroke("ctrl+p"), &clock),
            Outcome::Command { command: "command.palette.show".to_owned(), args: None }
        );

        // Re-registering replaces (hot reload), clear_owned drops all.
        keymap.register_owned(owned("hw.hello", &["ctrl+x j"]));
        assert_eq!(keymap.owned_commands().len(), 2);
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(
            keymap.lookup(stroke("j"), &clock),
            Outcome::Command { command: "hw.hello".to_owned(), args: None }
        );
        // The old chord is gone (replaced, not duplicated).
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(keymap.lookup(stroke("h"), &clock), Outcome::Unhandled);
        keymap.clear_owned();
        assert!(matches!(keymap.owned_commands(), []));
        assert_eq!(
            keymap.lookup(stroke("ctrl+p"), &clock),
            Outcome::Command { command: "command.palette.show".to_owned(), args: None }
        );
    }
}
