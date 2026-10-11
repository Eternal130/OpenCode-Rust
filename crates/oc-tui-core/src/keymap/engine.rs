//! The keymap engine — a Rust port of the `@opentui/keymap` v0.5.14
//! dispatch CONTRACT (studied from the shipped package source:
//! `src/index.js`, `chunks/index-7g381cth.js`, `addons/opentui/index.js`
//! in the bun cache, and v2's usage in `packages/tui/src/context/
//! keymap.tsx`).
//!
//! ## Ported contract points (with the reference anchors)
//!
//! * **Layer ordering** (`getSortedLayers`, chunks/index-7g381cth.js:
//!   154-159): layers sort by priority DESC, then registration order
//!   DESC — a later-registered layer shadows an earlier one at equal
//!   priority (v2 relies on this: the conditional `app.exit` layer at
//!   app.tsx:1244-1251 binds over the session route's layers).
//! * **Focus-scope resolution** (`getActiveLayersForFocused` /
//!   `getActivationPath`, chunks:118-172): the active layer set is the
//!   FOCUSED target's ancestor chain — a layer bound to a target is
//!   active while that target is on the focus path (bindings of the
//!   focused renderable win, bubbling to the root); `target_mode:
//!   Focus` restricts a layer to the exactly-focused target; a
//!   target-less layer is global (always active).
//! * **Mode gating** (`createMode` + `registerLayerFields`,
//!   keymap.tsx:420-453): named-mode layers are reachable only while
//!   their mode is current (the mode stack's last ENABLED entry);
//!   `mode: "global"` layers stay reachable in every mode
//!   (keymap.test.tsx:103-141).
//! * **Sequence captures** (`buildSequenceTree`,
//!   `collectRootSequenceCaptures`, `advanceSequenceCapture`,
//!   chunks:219-404): a multi-stroke binding (a leader chord) holds a
//!   PENDING sequence while continuations remain
//!   (`captureHasContinuations`); an exact match runs the bindings in
//!   order — the first handled non-fallthrough binding stops the walk
//!   (`runCaptureBindings`, index.js:3410-3431).
//! * **preventDefault object semantics** (`applyBindingEventEffects`,
//!   index.js:2035-2041 + config/keybind.ts:14-29): a handled binding
//!   whose `prevent_default` is true consumes the stroke (the keymap
//!   suppressed it); `prevent_default: false` (v2's `prompt.paste`)
//!   leaves suppression to the command handler — the engine reports
//!   the command WITHOUT consuming, and the handler decides.
//! * **Command rejection fall-through** (`runBinding`, index.js:
//!   2006-2029): a command whose run returns `false` is REJECTED — the
//!   binding counts as unhandled and dispatch continues to the next
//!   binding/layer (`dispatch_with_runner`). The always-true runner is
//!   the legacy compat path ([`super::Keymap::lookup`]).
//! * **Interceptor precedence** (`createRuntimeOrderedRegistry` +
//!   `keyHooks`, index.js:25-110 + 2811-2856): key interceptors run
//!   BEFORE layer dispatch, ordered by priority then registration;
//!   `prepend`/`append` choose the registration end. A consuming
//!   interceptor stops dispatch (`reason: "intercept-consumed"`).
//! * **Which-key data model** (`getPendingSequence` /
//!   `getActiveKeys`, index.js:442-533): the pending sequence's typed
//!   strokes (display + token name) and the set of NEXT keys reachable
//!   from it (with `continues` flags) — the pending-chord display
//!   surface, data only; the which-key panel UI is a later wave.
//! * **Timed leader + escape-clears** (keymap.tsx:137-146
//!   `registerTimedLeader`, addons `registerEscapeClearsPending-
//!   Sequence`): the pending sequence expires after the leader timeout
//!   (2000 ms default) and a plain escape clears it before anything
//!   else dispatches.
//!
//! ## Divergences (documented, pinned by the port's replay suite)
//!
//! * A pending-sequence MISS re-evaluates the stroke fresh from the
//!   root (v2 returns a `sequence-miss` outcome without redispatch —
//!   the un-consumed event then reaches the focused textarea, which
//!   for our single-surface pump is behaviorally the fresh walk).
//! * The event-match resolver hook (multiple match keys per event) is
//!   elided: the port's [`KeyStroke`] is already canonical.

use super::table::{Chord, Clock, KeyStroke, LEADER_TIMEOUT_MS};

/// A focus-scope target id (v2: a `Renderable` reference). The engine
/// compares ids along the focus path; hosts map their surface tree to
/// a chain of ids (`"composer"`, `"session"`, …) ending at the root.
pub type TargetId = &'static str;

/// How a layer's target activates (`targetMode`, chunks:160-172).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetMode {
    /// Active while the target is anywhere on the focus path (the
    /// default — bindings bubble from the focused renderable up).
    #[default]
    Path,
    /// Active only while the target IS the focused node.
    Focus,
}

/// A mode qualifier. v2's layer `mode` field: named modes require the
/// mode to be current; `global` layers are reachable in every mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerMode {
    Global,
    Named(String),
}

impl LayerMode {
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        if name == "global" {
            Self::Global
        } else {
            Self::Named(name.to_owned())
        }
    }
}

/// One binding inside a layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineBinding {
    /// The binding's sequence (`key` after leader expansion).
    pub chord: Chord,
    /// The command id dispatched on an exact match (the `cmd`).
    pub command: String,
    /// `preventDefault` (config/keybind.ts:14-29): `true` — the engine
    /// consumes the matched stroke; `false` — suppression is the
    /// handler's job (v2's `prompt.paste`).
    pub prevent_default: bool,
    /// `fallthrough`: the binding runs but does NOT stop the walk.
    pub fallthrough: bool,
}

/// A registered keymap layer (`registerLayer`).
#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    /// Focus-scope target; `None` = a global layer.
    pub target: Option<TargetId>,
    pub target_mode: TargetMode,
    /// Higher sorts first (`getSortedLayers`).
    pub priority: i32,
    /// Registration sequence (later wins ties).
    pub order: u64,
    /// Mode gating (`mode: "global"` stays reachable).
    pub mode: LayerMode,
    /// The layer's enable flag (v2's `enabled: () => …` — the host
    /// updates it between strokes; the conditional `app.exit` layer).
    pub enabled: bool,
    pub bindings: Vec<EngineBinding>,
}

/// One pending-sequence part (the which-key display datum,
/// `collectSequencePartsFromPending`, index.js:341-385).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPart {
    /// The stroke's display form (`stringifyKeyStroke`).
    pub display: String,
    /// The token the stroke came from (`"leader"`), when known.
    pub token: Option<String>,
}

/// One reachable next key from the pending sequence (`getActiveKeys`,
/// index.js:479-532): the which-key panel's option datum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveKey {
    pub display: String,
    /// The command an exact completion would run, when unambiguous.
    pub command: Option<String>,
    /// The key continues a longer sequence (a chord prefix).
    pub continues: bool,
}

/// What one interceptor did.
pub enum Intercept {
    /// Keep dispatching (the interceptor observed only).
    Pass,
    /// Consume the stroke (`intercept-consumed`): no layer dispatch.
    Consume,
}

/// A key interceptor hook (v2's `keymap.intercept("key", fn)`).
pub struct Interceptor {
    /// Higher runs first; ties keep registration order
    /// (`createRuntimeOrderedRegistry`, index.js:94-110).
    pub priority: i32,
    pub order: u64,
    pub hook: Box<dyn Fn(&KeyStroke) -> Intercept>,
}

/// The result of dispatching one stroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOutcome {
    /// The resolved command, when one matched and was accepted.
    pub command: Option<(String, Option<String>)>,
    /// The stroke was consumed by the keymap (`preventDefault`: the
    /// terminal default and lower surfaces never see it).
    pub consumed: bool,
    /// The stroke extended/started a pending sequence (the leader
    /// display surface).
    pub pending: Option<String>,
    /// A pending sequence was cleared (escape/timeout).
    pub cancelled: bool,
}

impl DispatchOutcome {
    /// No binding matched; the stroke passes through.
    #[must_use]
    pub fn unhandled() -> Self {
        Self { command: None, consumed: false, pending: None, cancelled: false }
    }
}

struct PendingSequence {
    /// The typed strokes (display surface).
    strokes: Vec<KeyStroke>,
    /// Which token the sequence came from (`"leader"`).
    token: Option<String>,
    expires_at: u64,
}

/// The engine: layers + focus path + mode stack + interceptors + the
/// pending sequence, dispatching one stroke at a time.
pub struct KeymapEngine {
    layers: Vec<Layer>,
    next_order: u64,
    interceptors: Vec<Interceptor>,
    interceptor_order: u64,
    focus_path: Vec<TargetId>,
    /// The mode stack (the last entry names the current mode; empty =
    /// `"base"`), keymap.tsx:420-453.
    modes: Vec<String>,
    disabled_commands: Vec<String>,
    leader: KeyStroke,
    timeout_ms: u64,
    pending: Option<PendingSequence>,
    /// While set, named-mode layers are suspended (the port's
    /// base-only resolution: the conditional `app.exit` layer sits
    /// above the managed textarea layer, so its chords resolve before
    /// any input-mode shadowing — oc-tui's `lookup_base`). Unlike a
    /// mode-stack change this NEVER clears the pending sequence: the
    /// leader chord completes through the base layer mid-sequence.
    base_only: bool,
}

impl Default for KeymapEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl KeymapEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
            next_order: 0,
            interceptors: Vec::new(),
            interceptor_order: 0,
            focus_path: Vec::new(),
            modes: Vec::new(),
            disabled_commands: Vec::new(),
            leader: KeyStroke::parse(super::table::LEADER_DEFAULT)
                .unwrap_or_else(|| KeyStroke::ctrl("x")),
            timeout_ms: LEADER_TIMEOUT_MS,
            pending: None,
            base_only: false,
        }
    }

    /// The configured leader stroke.
    #[must_use]
    pub fn leader(&self) -> &KeyStroke {
        &self.leader
    }

    pub fn set_leader(&mut self, leader: KeyStroke) {
        self.leader = leader;
    }

    pub fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }

    #[must_use]
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Register a layer; returns its handle (name) for
    /// [`Self::set_layer_enabled`] / removal.
    pub fn register_layer(&mut self, mut layer: Layer) -> String {
        layer.order = self.next_order;
        self.next_order += 1;
        let name = layer.name.clone();
        self.layers.push(layer);
        name
    }

    /// Remove every layer with this name (`useBindings` cleanup).
    pub fn remove_layers(&mut self, name: &str) {
        self.layers.retain(|layer| layer.name != name);
    }

    /// The conditional-layer gate (v2's `enabled` accessor — the host
    /// re-evaluates it between strokes).
    pub fn set_layer_enabled(&mut self, name: &str, enabled: bool) {
        for layer in &mut self.layers {
            if layer.name == name {
                layer.enabled = enabled;
            }
        }
    }

    /// Disable one command's bindings everywhere (the port's granular
    /// conditional-layer equivalent — a disabled command never matches,
    /// exactly v2's rejected binding).
    pub fn set_command_enabled(&mut self, command: &str, enabled: bool) {
        if enabled {
            self.disabled_commands.retain(|entry| entry != command);
        } else if !self.disabled_commands.iter().any(|entry| entry == command) {
            self.disabled_commands.push(command.to_owned());
        }
    }

    #[must_use]
    pub fn command_enabled(&self, command: &str) -> bool {
        !self.disabled_commands.iter().any(|entry| entry == command)
    }

    /// Set the focus path (focused target first, root last —
    /// `getActivationPath`, chunks:124-142).
    pub fn set_focus_path(&mut self, path: &[TargetId]) {
        self.focus_path = path.to_vec();
        // v2 revalidates the pending sequence when focus changes
        // (ensureValidPendingSequence, index.js:414-432).
        self.pending = None;
    }

    #[must_use]
    pub fn focus_path(&self) -> &[TargetId] {
        &self.focus_path
    }

    /// Replace the mode stack wholesale (the port's pump recomputes
    /// it each stroke; a change clears any half-typed sequence — the
    /// reference clears on mode transitions the same way).
    pub fn set_modes(&mut self, modes: &[&str]) {
        let wanted: Vec<String> = modes.iter().map(|mode| (*mode).to_owned()).collect();
        if wanted != self.modes {
            self.modes = wanted;
            self.pending = None;
        }
    }

    pub fn push_mode(&mut self, mode: &str) {
        self.modes.push(mode.to_owned());
        self.pending = None;
    }

    pub fn pop_mode(&mut self) {
        self.modes.pop();
        self.pending = None;
    }

    /// The current mode (`createMode.current`: the stack's last entry,
    /// `"base"` when empty).
    #[must_use]
    pub fn current_mode(&self) -> &str {
        self.modes.last().map_or("base", String::as_str)
    }

    /// Prepend a key interceptor (runs before the appended ones at
    /// equal priority — the `prependListener` family).
    pub fn prepend_interceptor(&mut self, hook: Box<dyn Fn(&KeyStroke) -> Intercept>) {
        self.interceptors.push(Interceptor { priority: 1, order: self.interceptor_order, hook });
        self.interceptor_order += 1;
    }

    /// Append a key interceptor (lowest precedence).
    pub fn append_interceptor(&mut self, hook: Box<dyn Fn(&KeyStroke) -> Intercept>) {
        self.interceptors.push(Interceptor { priority: 0, order: self.interceptor_order, hook });
        self.interceptor_order += 1;
    }

    pub fn clear_interceptors(&mut self) {
        self.interceptors.clear();
    }

    /// The pending sequence's typed strokes (the display surface).
    #[must_use]
    pub fn pending_parts(&self) -> Vec<PendingPart> {
        self.pending.as_ref().map_or(Vec::new(), |pending| {
            pending
                .strokes
                .iter()
                .map(|stroke| PendingPart {
                    display: stroke.display(),
                    token: pending.token.clone(),
                })
                .collect()
        })
    }

    /// The pending display ("ctrl+x n" style), if a sequence is live.
    #[must_use]
    pub fn pending_display(&self) -> Option<String> {
        self.pending.as_ref().map(|pending| {
            pending.strokes.iter().map(KeyStroke::display).collect::<Vec<_>>().join(" ")
        })
    }

    /// The reachable next keys from the pending sequence — the
    /// which-key option set (`getActiveKeys` data model).
    #[must_use]
    pub fn active_keys(&self) -> Vec<ActiveKey> {
        let mut out: Vec<ActiveKey> = Vec::new();
        let Some(pending) = &self.pending else { return out };
        let typed = &pending.strokes;
        for layer in self.active_layers_sorted() {
            if !layer.enabled {
                continue;
            }
            for binding in &layer.bindings {
                if !self.command_enabled(&binding.command) {
                    continue;
                }
                let strokes = &binding.chord.strokes;
                if strokes.len() <= typed.len() || !strokes.starts_with(typed) {
                    continue;
                }
                let next = &strokes[typed.len()];
                let continues = strokes.len() > typed.len() + 1;
                if let Some(existing) = out.iter_mut().find(|entry| entry.display == next.display())
                {
                    existing.continues |= continues;
                } else {
                    out.push(ActiveKey {
                        display: next.display(),
                        command: continues.then(|| binding.command.clone()),
                        continues,
                    });
                }
            }
        }
        out
    }

    /// Expire a stale pending sequence (`registerTimedLeader`);
    /// `true` when one expired.
    pub fn expire(&mut self, clock: &dyn Clock) -> bool {
        if let Some(pending) = &self.pending {
            if clock.now_ms() >= pending.expires_at {
                self.pending = None;
                return true;
            }
        }
        false
    }

    /// Drop any pending sequence (a surface consumed the continuation).
    pub fn clear_pending(&mut self) {
        self.pending = None;
    }

    #[must_use]
    pub fn is_leader(&self, stroke: &KeyStroke) -> bool {
        *stroke == self.leader
    }

    /// The active layers in dispatch order: sorted by priority DESC
    /// then order DESC (`getSortedLayers`), filtered by focus scope
    /// and mode.
    fn active_layers_sorted(&self) -> Vec<&Layer> {
        let mut layers: Vec<&Layer> =
            self.layers.iter().filter(|layer| self.layer_active(layer)).collect();
        layers.sort_by(|left, right| {
            right.priority.cmp(&left.priority).then_with(|| right.order.cmp(&left.order))
        });
        layers
    }

    /// `isLayerActiveForFocused` (chunks:160-172) + the mode gate
    /// (`registerLayerFields`' `require(opencode.mode, value)`).
    fn layer_active(&self, layer: &Layer) -> bool {
        if let Some(target) = layer.target {
            let on_path = match layer.target_mode {
                TargetMode::Focus => self.focus_path.first() == Some(&target),
                TargetMode::Path => self.focus_path.contains(&target),
            };
            if !on_path {
                return false;
            }
        }
        match &layer.mode {
            LayerMode::Global => true,
            LayerMode::Named(mode) => !self.base_only && mode == self.current_mode(),
        }
    }

    /// Dispatch one stroke with a command runner. The runner executes
    /// a resolved command and returns whether it was ACCEPTED (`true`)
    /// or REJECTED (`false` — v2's `run()` returning `false`: the
    /// binding counts as unhandled and dispatch continues,
    /// index.js:2006-2029).
    pub fn dispatch_with_runner(
        &mut self,
        stroke: KeyStroke,
        clock: &dyn Clock,
        runner: &mut dyn FnMut(&str, Option<&str>) -> bool,
    ) -> DispatchOutcome {
        self.expire(clock);
        // Interceptors first (`handleKeyEvent`, index.js:2811-2856):
        // priority DESC, registration order; a consumer stops dispatch.
        let mut ordered: Vec<&Interceptor> = self.interceptors.iter().collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            right.priority.cmp(&left.priority).then_with(|| left.order.cmp(&right.order))
        });
        for interceptor in ordered {
            match (interceptor.hook)(&stroke) {
                Intercept::Pass => {}
                Intercept::Consume => return DispatchOutcome::unhandled(),
            }
        }
        // Escape clears a pending sequence first
        // (`registerEscapeClearsPendingSequence`).
        if self.pending.is_some() && stroke.key == "escape" && !stroke.ctrl && !stroke.alt {
            self.pending = None;
            return DispatchOutcome {
                command: None,
                consumed: true,
                pending: None,
                cancelled: true,
            };
        }
        // Advance a pending sequence (`dispatchPendingSequence`,
        // index.js:2935-2953).
        if let Some(pending) = self.pending.take() {
            let mut candidate = pending.strokes.clone();
            candidate.push(stroke.clone());
            match self.match_layers(&candidate, runner) {
                LayerMatch::Owned((command, args), consumed) => {
                    return DispatchOutcome {
                        command: Some((command, args)),
                        consumed,
                        pending: None,
                        cancelled: false,
                    };
                }
                LayerMatch::Prefix => {
                    self.pending = Some(PendingSequence {
                        strokes: candidate,
                        token: pending.token,
                        expires_at: clock.now_ms() + self.timeout_ms,
                    });
                    let display = self.pending_display().unwrap_or_default();
                    return DispatchOutcome {
                        command: None,
                        consumed: true,
                        pending: Some(display),
                        cancelled: false,
                    };
                }
                LayerMatch::None => {
                    // Miss: clear and evaluate the stroke fresh (the
                    // documented divergence — see the module docs).
                }
            }
        }
        // Root capture (`dispatchFromRoot`, index.js:2998-3037): the
        // leader arms only when a chord starts with it.
        let single = std::slice::from_ref(&stroke);
        if self.is_leader(&stroke) && self.leader_starts_sequence() {
            self.pending = Some(PendingSequence {
                strokes: vec![stroke],
                token: Some("leader".to_owned()),
                expires_at: clock.now_ms() + self.timeout_ms,
            });
            return DispatchOutcome {
                command: None,
                consumed: true,
                pending: self.pending_display(),
                cancelled: false,
            };
        }
        match self.match_layers(single, runner) {
            LayerMatch::Owned((command, args), consumed) => DispatchOutcome {
                command: Some((command, args)),
                consumed,
                pending: None,
                cancelled: false,
            },
            _ => DispatchOutcome::unhandled(),
        }
    }

    /// Dispatch one stroke against the BASE layer only (named-mode
    /// layers suspended — [`super::Keymap::lookup_base`]'s engine
    /// form). The pending sequence survives: a leader chord started
    /// on the route completes through the base layer.
    pub fn dispatch_base_only(
        &mut self,
        stroke: KeyStroke,
        clock: &dyn Clock,
        runner: &mut dyn FnMut(&str, Option<&str>) -> bool,
    ) -> DispatchOutcome {
        self.base_only = true;
        let outcome = self.dispatch_with_runner(stroke, clock, runner);
        self.base_only = false;
        outcome
    }

    /// Dispatch with an always-accepting runner (the legacy compat
    /// path — every matched command "runs").
    pub fn dispatch(&mut self, stroke: KeyStroke, clock: &dyn Clock) -> DispatchOutcome {
        self.dispatch_with_runner(stroke, clock, &mut |_, _| true)
    }

    /// Whether any active binding begins with the leader stroke.
    fn leader_starts_sequence(&self) -> bool {
        self.layers.iter().filter(|layer| self.layer_active(layer) && layer.enabled).any(|layer| {
            layer.bindings.iter().any(|binding| binding.chord.strokes.first() == Some(&self.leader))
        })
    }

    /// The layered matcher: walk the active layers in dispatch order;
    /// within a layer, the first accepted exact match wins
    /// (`runCaptureBindings`); a longer chord with this stroke list as
    /// prefix keeps the sequence (`captureHasContinuations`).
    fn match_layers(
        &self,
        strokes: &[KeyStroke],
        runner: &mut dyn FnMut(&str, Option<&str>) -> bool,
    ) -> LayerMatch {
        let mut prefix = false;
        for layer in self.active_layers_sorted() {
            if !layer.enabled {
                continue;
            }
            for binding in &layer.bindings {
                if binding.chord.strokes == strokes {
                    if !self.command_enabled(&binding.command) {
                        continue;
                    }
                    if runner(&binding.command, None) {
                        return LayerMatch::Owned(
                            (binding.command.clone(), None),
                            binding.prevent_default,
                        );
                    }
                    // Rejected: keep walking (index.js:2010-2014).
                    continue;
                }
                if binding.chord.strokes.len() > strokes.len()
                    && binding.chord.strokes.starts_with(strokes)
                    && strokes.first() == Some(&self.leader)
                {
                    prefix = true;
                }
            }
        }
        if prefix {
            LayerMatch::Prefix
        } else {
            LayerMatch::None
        }
    }
}

enum LayerMatch {
    Owned((String, Option<String>), bool),
    Prefix,
    None,
}

/// Convenience: build an [`EngineBinding`] from a spec + command.
#[must_use]
pub fn binding(spec: &str, command: &str, leader: &KeyStroke) -> Option<EngineBinding> {
    Chord::parse(spec, leader).map(|chord| EngineBinding {
        chord,
        command: command.to_owned(),
        prevent_default: true,
        fallthrough: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::table::{Chord, ManualClock};

    fn leader() -> KeyStroke {
        KeyStroke::parse("ctrl+x").expect("leader parses")
    }

    fn stroke(spec: &str) -> KeyStroke {
        KeyStroke::parse(spec).expect("stroke parses")
    }

    fn engine_with(layers: Vec<Layer>) -> KeymapEngine {
        let mut engine = KeymapEngine::new();
        for layer in layers {
            engine.register_layer(layer);
        }
        engine
    }

    fn layer(name: &str, mode: &str, specs: &[(&str, &str)]) -> Layer {
        let leader = leader();
        Layer {
            name: name.to_owned(),
            target: None,
            target_mode: TargetMode::Path,
            priority: 0,
            order: 0,
            mode: LayerMode::from_name(mode),
            enabled: true,
            bindings: specs
                .iter()
                .filter_map(|(spec, command)| binding(spec, command, &leader))
                .collect(),
        }
    }

    /// `getSortedLayers`: later registration shadows earlier at equal
    /// priority (v2's conditional app.exit layer pattern).
    #[test]
    fn later_layers_shadow_earlier_ones() {
        let mut engine = engine_with(vec![
            layer("route", "global", &[("return", "session.submit")]),
            layer("exit", "global", &[("return", "app.exit")]),
        ]);
        let clock = ManualClock::default();
        let outcome = engine.dispatch(stroke("return"), &clock);
        assert_eq!(outcome.command.map(|(command, _)| command), Some("app.exit".to_owned()));
        // Priority trumps registration order.
        let mut engine = engine_with(vec![
            Layer { priority: 10, ..layer("high", "global", &[("return", "session.submit")]) },
            layer("low", "global", &[("return", "app.exit")]),
        ]);
        let outcome = engine.dispatch(stroke("return"), &clock);
        assert_eq!(outcome.command.map(|(command, _)| command), Some("session.submit".to_owned()));
    }

    /// Focus-scope resolution (`getActiveLayersForFocused`): a
    /// targeted layer activates on the focus path; `target_mode:
    /// Focus` restricts to the exactly-focused node; global layers
    /// always run.
    #[test]
    fn focus_scoped_layers_bubble() {
        let mut engine = engine_with(vec![
            layer("global", "global", &[("return", "app.submit")]),
            Layer {
                target: Some("composer"),
                target_mode: TargetMode::Path,
                ..layer("composer", "global", &[("return", "composer.enter")])
            },
            Layer {
                target: Some("composer"),
                target_mode: TargetMode::Focus,
                ..layer("composer-focus", "global", &[("tab", "composer.focus.tab")])
            },
        ]);
        let clock = ManualClock::default();
        // Focus on a child of the composer: path layers active, focus
        // layers not.
        engine.set_focus_path(&["caret", "composer", "root"]);
        let outcome = engine.dispatch(stroke("return"), &clock);
        assert_eq!(outcome.command.map(|(command, _)| command), Some("composer.enter".to_owned()));
        assert!(
            engine.dispatch(stroke("tab"), &clock).command.is_none(),
            "Focus-mode layer inactive on a child"
        );
        // Exactly focused: both active.
        engine.set_focus_path(&["composer", "root"]);
        let outcome = engine.dispatch(stroke("tab"), &clock);
        assert_eq!(
            outcome.command.map(|(command, _)| command),
            Some("composer.focus.tab".to_owned())
        );
        // Off the path: only the global layer.
        engine.set_focus_path(&["sidebar", "root"]);
        let outcome = engine.dispatch(stroke("return"), &clock);
        assert_eq!(outcome.command.map(|(command, _)| command), Some("app.submit".to_owned()));
    }

    /// Mode gating (keymap.test.tsx:103-141): named-mode layers are
    /// unreachable while another mode is current; global layers stay.
    #[test]
    fn named_modes_gate_and_globals_stay_reachable() {
        let mut engine = engine_with(vec![
            layer("app", "global", &[("g", "app.global")]),
            layer("base", "base", &[("b", "app.base")]),
            layer("form", "form", &[("f", "app.form")]),
        ]);
        let clock = ManualClock::default();
        assert_eq!(engine.current_mode(), "base");
        assert_eq!(engine.dispatch(stroke("b"), &clock).command.unwrap().0, "app.base");
        engine.push_mode("form");
        assert_eq!(engine.current_mode(), "form");
        // base layer unreachable in form mode; form + global are.
        assert!(engine.dispatch(stroke("b"), &clock).command.is_none());
        assert_eq!(engine.dispatch(stroke("f"), &clock).command.unwrap().0, "app.form");
        assert_eq!(engine.dispatch(stroke("g"), &clock).command.unwrap().0, "app.global");
        engine.pop_mode();
        assert_eq!(engine.dispatch(stroke("b"), &clock).command.unwrap().0, "app.base");
    }

    /// Rejected commands fall through to the next binding/layer
    /// (`runBinding`, index.js:2006-2029).
    #[test]
    fn rejected_commands_fall_through() {
        let mut engine =
            engine_with(vec![layer("l", "global", &[("return", "first"), ("return", "second")])]);
        let clock = ManualClock::default();
        let outcome = engine
            .dispatch_with_runner(stroke("return"), &clock, &mut |command, _| command != "first");
        assert_eq!(outcome.command.unwrap().0, "second");
    }

    /// `preventDefault` object semantics: the consumed flag follows
    /// the binding's `prevent_default`, not the match.
    #[test]
    fn prevent_default_object_semantics() {
        let leader = leader();
        let mut engine = engine_with(vec![Layer {
            bindings: vec![
                EngineBinding {
                    chord: Chord::parse("ctrl+v", &leader).unwrap(),
                    command: "prompt.paste".into(),
                    prevent_default: false,
                    fallthrough: false,
                },
                EngineBinding {
                    chord: Chord::parse("ctrl+p", &leader).unwrap(),
                    command: "palette".into(),
                    prevent_default: true,
                    fallthrough: false,
                },
            ],
            ..layer("l", "global", &[])
        }]);
        let clock = ManualClock::default();
        let outcome = engine.dispatch(stroke("ctrl+v"), &clock);
        assert_eq!(outcome.command.as_ref().unwrap().0, "prompt.paste");
        assert!(!outcome.consumed, "preventDefault:false leaves suppression to the handler");
        let outcome = engine.dispatch(stroke("ctrl+p"), &clock);
        assert_eq!(outcome.command.as_ref().unwrap().0, "palette");
        assert!(outcome.consumed);
    }

    /// Interceptor precedence: priority first, then registration
    /// order; a consumer stops dispatch.
    #[test]
    fn interceptors_run_before_layers_by_precedence() {
        let mut engine = engine_with(vec![layer("l", "global", &[("x", "bound")])]);
        let clock = ManualClock::default();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let a = seen.clone();
        let b = seen.clone();
        engine.append_interceptor(Box::new(move |_| {
            a.borrow_mut().push("appended");
            Intercept::Pass
        }));
        engine.prepend_interceptor(Box::new(move |_| {
            b.borrow_mut().push("prepended");
            Intercept::Pass
        }));
        let outcome = engine.dispatch(stroke("x"), &clock);
        assert_eq!(*seen.borrow(), vec!["prepended", "appended"]);
        assert_eq!(outcome.command.unwrap().0, "bound");
        // A consumer stops the layer dispatch.
        engine.prepend_interceptor(Box::new(|_| Intercept::Consume));
        let outcome = engine.dispatch(stroke("x"), &clock);
        assert!(outcome.command.is_none());
    }

    /// The which-key data model: pending parts + the reachable next
    /// keys with `continues` flags.
    #[test]
    fn which_key_data_model_tracks_pending_and_options() {
        let mut engine = engine_with(vec![layer(
            "base",
            "global",
            &[("<leader>n", "session.new"), ("<leader>1", "slot1"), ("<leader>0", "slot10")],
        )]);
        let clock = ManualClock::default();
        let outcome = engine.dispatch(stroke("ctrl+x"), &clock);
        assert_eq!(outcome.pending.as_deref(), Some("ctrl+x"));
        let parts = engine.pending_parts();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].display, "ctrl+x");
        assert_eq!(parts[0].token.as_deref(), Some("leader"));
        let mut keys = engine.active_keys();
        keys.sort_by(|left, right| left.display.cmp(&right.display));
        assert_eq!(keys.len(), 3, "n, 1, 0 reachable");
        assert!(keys.iter().all(|key| !key.continues), "all one-stroke continuations");
        // Advance to an exact match; the pending set clears.
        let outcome = engine.dispatch(stroke("n"), &clock);
        assert_eq!(outcome.command.unwrap().0, "session.new");
        assert_eq!(engine.pending_parts(), Vec::<PendingPart>::new());
        assert_eq!(engine.active_keys(), Vec::<ActiveKey>::new());
    }

    /// The conditional-layer gate + command disable.
    #[test]
    fn conditional_layers_and_disabled_commands() {
        let mut engine =
            engine_with(vec![Layer { ..layer("exit", "global", &[("<leader>q", "app.exit")]) }]);
        let clock = ManualClock::default();
        engine.set_layer_enabled("exit", false);
        let outcome = engine.dispatch(stroke("ctrl+x"), &clock);
        assert_eq!(outcome.pending, None, "a disabled layer's leader does not arm");
        engine.set_layer_enabled("exit", true);
        engine.set_command_enabled("app.exit", false);
        engine.dispatch(stroke("ctrl+x"), &clock);
        let outcome = engine.dispatch(stroke("q"), &clock);
        assert!(outcome.command.is_none(), "disabled commands never match");
        engine.set_command_enabled("app.exit", true);
        engine.dispatch(stroke("ctrl+x"), &clock);
        let outcome = engine.dispatch(stroke("q"), &clock);
        assert_eq!(outcome.command.unwrap().0, "app.exit");
    }

    /// Escape clears a pending sequence before anything dispatches;
    /// timeout expires it.
    #[test]
    fn escape_and_timeout_clear_pending() {
        let mut engine =
            engine_with(vec![layer("base", "global", &[("<leader>n", "session.new")])]);
        let clock = ManualClock::default();
        engine.dispatch(stroke("ctrl+x"), &clock);
        let outcome = engine.dispatch(stroke("escape"), &clock);
        assert!(outcome.cancelled && outcome.consumed);
        assert!(engine.pending_display().is_none());
        engine.dispatch(stroke("ctrl+x"), &clock);
        clock.advance(LEADER_TIMEOUT_MS);
        assert!(engine.expire(&clock));
        engine.dispatch(stroke("ctrl+x"), &clock);
        clock.advance(LEADER_TIMEOUT_MS - 1);
        assert!(!engine.expire(&clock));
    }

    /// Base-only dispatch suspends named modes WITHOUT clearing the
    /// pending sequence (the leader chord completes through base —
    /// oc-tui's D-02 flow).
    #[test]
    fn base_only_dispatch_suspends_modes_but_keeps_pending() {
        let mut engine = engine_with(vec![
            layer("base", "global", &[("<leader>n", "session.new")]),
            layer("input", "input", &[("n", "input.move.none")]),
        ]);
        let clock = ManualClock::default();
        engine.push_mode("input");
        engine.dispatch(stroke("ctrl+x"), &clock);
        assert!(engine.pending_display().is_some());
        let outcome = engine.dispatch_base_only(stroke("n"), &clock, &mut |_, _| true);
        assert_eq!(outcome.command.unwrap().0, "session.new", "the chord completes through base");
        assert!(engine.pending_display().is_none());
    }
}
