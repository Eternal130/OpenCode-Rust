//! The opentui-keymap ENGINE: the oc-tui-core keymap facade's
//! dispatch layer over the REAL `@opentui/keymap` 0.5.14 port
//! (`opentui-keymap`, the proven zero-diff port). Since the
//! M-engine-retire lane this is the ONLY engine — the hand-rolled
//! predecessor is gone.
//!
//! This module also owns the engine's DATA contracts ([`Layer`],
//! [`EngineBinding`], [`DispatchOutcome`], [`ActiveKey`], …) — the
//! shapes the facade builds and the adapter converts into the donor's
//! registration grammar.
//!
//! ## The K-1..K-9 deltas dissolved by the port (historical register)
//!
//! * **K-1** non-leader multi-press sequences never armed — the donor's
//!   `dispatchFromRootAtIndex` holds a pending capture for ANY binding
//!   whose first part matched (no leader-only gate).
//! * **K-2** sequence miss re-evaluated from root — the donor returns
//!   `sequence-miss` UNCONSUMED without redispatch; the stroke falls
//!   to the focused surface instead of re-resolving.
//! * **K-3** exact-match vs continuation priority — the donor's
//!   `dispatchPendingSequence` holds continuations over exact matches
//!   across layers (a high-layer continuation shadows a low-layer
//!   exact).
//! * **K-4** timed leader default — v2 passes 2000 ms explicitly
//!   (`context/keymap.tsx:143`); the adapter passes the facade's
//!   timeout (default [`super::table::LEADER_TIMEOUT_MS`] = 2000).
//! * **K-5** hyper folding — the donor's `KeymapEvent` carries `hyper`
//!   separately in the canonical ctrl,shift,meta,super,hyper order; the
//!   adapter's stroke conversion keeps the modifier distinct.
//! * **K-6** escape-clears — v2 registers the ADDON
//!   (`registerEscapeClearsPendingSequence`), which clears on ANY
//!   escape (ctrl+escape included); the adapter registers the same.
//! * **K-7** backspace pop / comma expansion / base-layout fallback —
//!   all three addons registered, exactly v2's Provider set.
//! * **K-8** `getActiveKeys` at root — the donor lists the reachable
//!   keys from the (empty) prefix at the root too.
//! * **K-9** layer order / focus domains / rejection fall-through /
//!   preventDefault — already consistent (pinned by the table suite).
//!
//! ## The v2 Provider wiring this mirrors (context/keymap.tsx:80-150)
//!
//! `createDefaultOpenTuiKeymap`, `registerCommaBindings`, the
//! enter/esc/pgdown/pgup alias expander,
//! `registerBaseLayoutFallback`,
//! `registerEscapeClearsPendingSequence`,
//! `registerBackspacePopsPendingSequence`, and
//! `registerTimedLeader` (timeout
//! `config.leader?.timeout ?? leader_timeout ?? 2000`). The managed
//! textarea layer stays the oc-tui pump's own rung (the renderer
//! input lane has not landed); the table's `input` layer carries the
//! same `input.*` bindings v2 feeds it.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::table::{Chord, Clock, KeyStroke, LEADER_TIMEOUT_MS};
use opentui_keymap::addons::{
    register_backspace_pops_pending_sequence, register_base_layout_fallback,
    register_comma_bindings, register_escape_clears_pending_sequence, register_timed_leader,
    TimedLeaderOptions,
};
use opentui_keymap::opentui::OpenTuiHost;
use opentui_keymap::types::{
    Binding, BindingCommand, BindingEvent, Command, CommandResult, HostPlatform, KeyStrokeInput,
    Layer as DonorLayer,
};
use opentui_keymap::{KeyAfterReason, KeymapEvent, Registration};

// -- the engine's data contracts (the facade ↔ adapter vocabulary) -------

/// A focus-scope target id (v2: a `Renderable` reference). The engine
/// compares ids along the focus path; hosts map their surface tree to
/// a chain of ids (`"composer"`, `"session"`, …) ending at the root.
pub type TargetId = &'static str;

/// How a layer's target activates (`targetMode`).
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

/// A registered keymap layer (`registerLayer`) — the facade's
/// installation shape; the adapter converts it into the donor's
/// registration grammar.
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

/// One reachable next key from the pending sequence (`getActiveKeys`):
/// the which-key panel's option datum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveKey {
    pub display: String,
    /// The command an exact completion would run, when unambiguous.
    pub command: Option<String>,
    /// The key continues a longer sequence (a chord prefix).
    pub continues: bool,
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

// -- the engine -----------------------------------------------------------

/// The v2 mode data key (`MODE = { key: "opencode.mode", base:
/// "base" }`, context/keymap.tsx:49) + the base mode name.
const MODE_KEY: &str = "opencode.mode";
const BASE_MODE: &str = "base";

/// The mode data-key sentinel used by base-only resolution
/// ([`OpentuiKeymap::dispatch_base_only`]): no table layer binds this
/// mode name, so every named-mode layer goes inert — the global
/// (base + owned) layers resolve alone.
const BASE_ONLY_MODE: &str = "__oc_base_only__";

/// The shared clock cell the donor engine reads (synced from the
/// caller's [`Clock`] before every dispatch/expire — the facade's
/// injected clocks keep their meaning).
#[derive(Default)]
struct SharedNow(Mutex<u64>);

impl opentui_keymap::Clock for SharedNow {
    fn now_ms(&self) -> u64 {
        *self.0.lock().expect("shared clock")
    }
}

impl SharedNow {
    fn set(&self, ms: u64) {
        *self.0.lock().expect("shared clock") = ms;
    }
}

/// The command disable gate shared with every registered command
/// closure: a disabled command REJECTS (`run() → false`), so its
/// binding counts unhandled and dispatch continues — v2's rejected
/// binding semantics (index.js:2006-2029) 1:1.
#[derive(Default, Clone)]
struct DisableGate(Arc<Mutex<HashSet<String>>>);

impl DisableGate {
    fn enabled(&self, command: &str) -> bool {
        !self.0.lock().expect("disable gate").contains(command)
    }
}

/// The opentui-keymap-backed engine: the dispatch layer beneath
/// [`super::Keymap`].
pub struct OpentuiKeymap {
    keymap: opentui_keymap::Keymap,
    now: Arc<SharedNow>,
    gate: DisableGate,
    leader: KeyStroke,
    timeout_ms: u64,
    modes: Vec<String>,
    /// Release tokens for the installed layers + the timed leader (a
    /// rebuild re-registers; the donor Registrations unwind cleanly).
    layer_tokens: Vec<Registration>,
    leader_token: Option<Registration>,
}

impl Default for OpentuiKeymap {
    fn default() -> Self {
        Self::new()
    }
}

impl OpentuiKeymap {
    #[must_use]
    pub fn new() -> Self {
        // The headless host: root target, no focus tree (the
        // single-surface pump has no renderer focus yet), linux
        // metadata — the pieces createDefaultOpenTuiKeymap consumes.
        // The shared clock rides the with_clock constructor.
        let host = Box::new(OpenTuiHost::new(HostPlatform::Linux, false));
        let now = Arc::new(SharedNow::default());
        let mut keymap = opentui_keymap::Keymap::with_clock(host, Arc::clone(&now) as Arc<_>);
        // createDefaultOpenTuiKeymap's registration set (default
        // parser/matcher + enabled/metadata fields) — the with_clock
        // constructor skips it, so install here (idempotent resources).
        let _ = opentui_keymap::addons::register_default_keys(&mut keymap);
        let _ = opentui_keymap::addons::register_enabled_fields(&mut keymap);
        let _ = opentui_keymap::addons::register_metadata_fields(&mut keymap);
        // v2's Provider registration set (context/keymap.tsx:86-115),
        // in the same order: comma bindings, the alias expander,
        // base-layout fallback, escape-clears, backspace-pops.
        let _ = register_comma_bindings(&mut keymap);
        let _ = keymap.append_binding_expander(Arc::new(Self::alias_expander));
        let _ = register_base_layout_fallback(&mut keymap);
        let _ = register_escape_clears_pending_sequence(&mut keymap, None);
        let _ = register_backspace_pops_pending_sequence(&mut keymap, None);
        Self {
            keymap,
            now,
            gate: DisableGate::default(),
            leader: KeyStroke::parse("ctrl+x").expect("default leader parses"),
            timeout_ms: LEADER_TIMEOUT_MS,
            modes: Vec::new(),
            layer_tokens: Vec::new(),
            leader_token: None,
        }
    }

    /// The v2 alias expander (context/keymap.tsx:89-99): enter→return,
    /// esc→escape, pgdown→pagedown, pgup→pageup inside binding specs.
    fn alias_expander(
        input: &str,
        displays: Option<&[String]>,
    ) -> Option<Vec<opentui_keymap::parser::BindingExpansion>> {
        let key =
            [("enter", "return"), ("esc", "escape"), ("pgdown", "pagedown"), ("pgup", "pageup")]
                .into_iter()
                .fold(input.to_owned(), |result, (alias, value)| {
                    alias_replace(&result, alias, value)
                });
        if key == input {
            return None;
        }
        Some(vec![opentui_keymap::parser::BindingExpansion {
            key,
            displays: displays.map(<[String]>::to_vec),
        }])
    }
}

/// The alias substitution without a regex dependency: replace `alias`
/// with `value` wherever it appears as a whole `+`/`,`/whitespace/`>`
/// delimited token (v2's `(^|[+,\s>])alias(?=$|[+,\s<])` rule).
fn alias_replace(input: &str, alias: &str, value: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut at = 0usize;
    while let Some(offset) = input[at..].find(alias) {
        let start = at + offset;
        let before_ok =
            start == 0 || input[..start].chars().next_back().is_some_and(is_alias_delim);
        let end = start + alias.len();
        let after_ok = input[end..].chars().next().is_none_or(|ch| is_alias_delim(ch) || ch == '<');
        out.push_str(&input[at..start]);
        if before_ok && after_ok {
            out.push_str(value);
        } else {
            out.push_str(&input[start..end]);
        }
        at = end;
    }
    out.push_str(&input[at..]);
    out
}

fn is_alias_delim(ch: char) -> bool {
    ch == '+' || ch == ',' || ch.is_whitespace() || ch == '>'
}

/// The command handler behind every table binding: the always-accept
/// runner (every matched command "runs") behind the shared disable
/// gate; a disabled command REJECTS (`run() → false`) and dispatch
/// continues to the next binding.
fn gate_handler(gate: DisableGate, name: String) -> opentui_keymap::types::CommandHandler {
    Box::new(
        move |_context: &opentui_keymap::types::CommandContext| {
            if gate.enabled(&name) {
                CommandResult::Handled
            } else {
                CommandResult::Rejected
            }
        },
    )
}

/// Convert one table [`KeyStroke`] into the donor's input stroke
/// shape. Our `alt` is the donor's `meta` (the terminal ALT modifier —
/// opentui's `meta`); `hyper` stays distinct (K-5: no folding).
#[must_use]
fn stroke_input(stroke: &KeyStroke) -> KeyStrokeInput {
    KeyStrokeInput {
        name: stroke.key.clone(),
        ctrl: Some(stroke.ctrl),
        shift: Some(stroke.shift),
        meta: Some(stroke.alt),
        super_key: Some(stroke.super_key),
        hyper: Some(false),
    }
}

/// The binding key spec for a chord, in the DONOR parser's sequence
/// grammar: `<leader>` token when the chord starts with the leader,
/// else direct part concatenation ("gg", "actrl+b") — the upstream
/// default parser has NO separator between sequence parts (v2's specs
/// are `<leader>e`-style tokens). Modifiers within one stroke use
/// ctrl,alt,shift,super order (the parser scans any order; the
/// canonical display order is ctrl,shift,meta,super,hyper per K-5).
#[must_use]
fn chord_spec(chord: &super::table::Chord, leader: &KeyStroke) -> String {
    let stroke_str = |stroke: &KeyStroke| {
        let mut parts: Vec<String> = Vec::new();
        if stroke.ctrl {
            parts.push("ctrl".into());
        }
        if stroke.alt {
            parts.push("alt".into());
        }
        if stroke.shift {
            parts.push("shift".into());
        }
        if stroke.super_key {
            parts.push("super".into());
        }
        parts.push(stroke.key.clone());
        parts.join("+")
    };
    if chord.strokes.first() == Some(leader) && chord.strokes.len() > 1 {
        let mut spec = String::from("<leader>");
        for stroke in &chord.strokes[1..] {
            spec.push_str(&stroke_str(stroke));
        }
        spec
    } else {
        chord.strokes.iter().map(stroke_str).collect::<Vec<_>>().concat()
    }
}

impl OpentuiKeymap {
    // -- the facade's engine surface ---------------------------------------

    #[must_use]
    pub fn leader(&self) -> &KeyStroke {
        &self.leader
    }

    pub fn set_leader(&mut self, leader: KeyStroke) {
        self.leader = leader;
        self.install_timed_leader();
    }

    pub fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
        self.install_timed_leader();
    }

    #[must_use]
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// (Re)register the timed leader with the current trigger/timeout —
    /// v2's explicit 2000 ms default rides
    /// [`super::table::LEADER_TIMEOUT_MS`] (K-4).
    fn install_timed_leader(&mut self) {
        if let Some(token) = self.leader_token.take() {
            self.keymap.release(token);
        }
        let options = TimedLeaderOptions::new(stroke_input(&self.leader))
            .with_name("leader")
            .with_timeout_ms(self.timeout_ms);
        self.leader_token = Some(register_timed_leader(&mut self.keymap, options));
    }

    pub fn set_command_enabled(&mut self, command: &str, enabled: bool) {
        let mut gate = self.gate.0.lock().expect("disable gate");
        if enabled {
            gate.remove(command);
        } else {
            gate.insert(command.to_owned());
        }
    }

    #[must_use]
    pub fn command_enabled(&self, command: &str) -> bool {
        self.gate.enabled(command)
    }

    /// Install one layer (converted from the facade's [`Layer`]
    /// shape). Mode mapping: [`LayerMode::Global`] → donor `None`
    /// (global, every mode); a named mode → `Some(name)` gated by the
    /// `opencode.mode` data key — exactly the donor's
    /// `require("opencode.mode", value)`. Returns the layer name (the
    /// legacy register shape).
    pub fn register_layer(&mut self, layer: &Layer) -> String {
        let name = layer.name.clone();
        let mut bindings = Vec::new();
        let mut commands = Vec::new();
        let mut seen_commands = HashSet::new();
        for binding in &layer.bindings {
            bindings.push(Binding {
                key: chord_spec(&binding.chord, &self.leader).into(),
                cmd: Some(BindingCommand::Name(binding.command.clone())),
                event: BindingEvent::Press,
                prevent_default: Some(binding.prevent_default),
                fallthrough: binding.fallthrough,
                attrs: std::collections::BTreeMap::default(),
            });
            if seen_commands.insert(binding.command.clone()) {
                commands.push(Command::new(
                    &binding.command,
                    gate_handler(self.gate.clone(), binding.command.clone()),
                ));
            }
        }
        let mode = match layer.mode {
            LayerMode::Global => None,
            LayerMode::Named(ref mode) => Some(mode.clone()),
        };
        let token = self.keymap.register_layer(DonorLayer {
            target: None,
            priority: layer.priority,
            bindings,
            commands,
            target_mode: Some(opentui_keymap::types::TargetMode::FocusWithin),
            mode,
            enabled: Some(layer.enabled.into()),
        });
        self.layer_tokens.push(token);
        name
    }

    /// Drop every installed layer (a facade rebuild re-registers).
    pub fn clear_layers(&mut self) {
        for token in std::mem::take(&mut self.layer_tokens) {
            self.keymap.release(token);
        }
    }

    /// Replace the mode stack (the facade recomputes it per stroke);
    /// the data key carries the TOP mode (v2's single current mode —
    /// `createMode`, keymap.tsx:404-424).
    pub fn set_modes(&mut self, modes: &[&str]) {
        self.modes = modes.iter().map(|mode| (*mode).to_owned()).collect();
        self.sync_mode_key();
    }

    pub fn push_mode(&mut self, mode: &str) {
        self.modes.push(mode.to_owned());
        self.sync_mode_key();
    }

    pub fn pop_mode(&mut self) {
        self.modes.pop();
        self.sync_mode_key();
    }

    fn sync_mode_key(&mut self) {
        let current = self.modes.last().cloned().unwrap_or_else(|| BASE_MODE.to_owned());
        self.keymap.set_data(MODE_KEY, serde_json::json!(current));
    }

    /// The current mode in the facade's shape (`&str` off the
    /// adapter's own stack — the data key carries the same value).
    #[must_use]
    pub fn current_mode(&self) -> &str {
        self.modes.last().map_or(BASE_MODE, String::as_str)
    }

    /// The pending display ("ctrl+x" style — the join of the
    /// typed strokes' display forms; the leader shows upstream's
    /// `<leader>` token).
    #[must_use]
    pub fn pending_display(&mut self) -> Option<String> {
        let parts = self.keymap.get_pending_sequence();
        (!parts.is_empty())
            .then(|| parts.iter().map(|part| part.display.clone()).collect::<Vec<_>>().join(" "))
    }

    /// Expire a stale pending sequence (the timed leader's lazy
    /// `setTimeout`): sync the clock, force the donor's lazy expiry
    /// (the pending readers run `ensure_valid_pending`), report
    /// whether a pending sequence cleared.
    pub fn expire(&mut self, clock: &dyn Clock) -> bool {
        // `had` reads at the ENGINE's current (pre-sync) time — the
        // pending getters lazily expire at that stale instant, so a
        // pending that already expired engine-side reports false on
        // the next call; the freshly-advanced check below clears the
        // just-reached timeout.
        let had = self.keymap.has_pending_sequence();
        self.now.set(clock.now_ms());
        had && !self.keymap.has_pending_sequence()
    }

    pub fn clear_pending(&mut self) {
        self.keymap.clear_pending_sequence();
    }

    #[must_use]
    pub fn is_leader(&self, stroke: &KeyStroke) -> bool {
        *stroke == self.leader
    }

    /// Dispatch one stroke through the REAL engine.
    fn dispatch_event(&mut self, stroke: &KeyStroke, clock: &dyn Clock) -> DispatchOutcome {
        self.now.set(clock.now_ms());
        let mut event = KeymapEvent::new(&stroke.key).with_modifiers(
            stroke.ctrl,
            stroke.shift,
            stroke.alt,
            stroke.super_key,
            false,
        );
        let outcome = self.keymap.handle_key_event(&mut event);
        let pending =
            (outcome.reason == KeyAfterReason::SequencePending).then(|| self.pending_display());
        // The escape-clears/backspace-pop addons consume with no
        // command — the `cancelled` observable (Outcome::Cancelled).
        let cancelled =
            outcome.reason == KeyAfterReason::InterceptConsumed && outcome.command.is_none();
        // A REJECTED binding's name must not surface as a command
        // (the donor's rejected outcome carries it for diagnostics;
        // only accepted commands are reported).
        let command = if outcome.handled { outcome.command } else { None };
        DispatchOutcome {
            command: command.map(|command| (command, None)),
            consumed: outcome.consumed,
            pending: pending.flatten(),
            cancelled,
        }
    }

    /// Dispatch against the base (global) layers only: the mode key is
    /// set to a sentinel no layer names — every named-mode layer goes
    /// inert. The pending sequence survives (the sentinel swap is not
    /// a mode change; the leader chord completes through base).
    pub fn dispatch_base_only(
        &mut self,
        stroke: &KeyStroke,
        clock: &dyn Clock,
        _runner: &mut dyn FnMut(&str, Option<&str>) -> bool,
    ) -> DispatchOutcome {
        let restore = self.modes.last().cloned().unwrap_or_else(|| BASE_MODE.to_owned());
        self.keymap.set_data(MODE_KEY, serde_json::json!(BASE_ONLY_MODE));
        let outcome = self.dispatch_event(stroke, clock);
        self.keymap.set_data(MODE_KEY, serde_json::json!(restore));
        outcome
    }

    pub fn dispatch(&mut self, stroke: &KeyStroke, clock: &dyn Clock) -> DispatchOutcome {
        self.dispatch_event(stroke, clock)
    }

    /// The which-key surface (K-8: non-empty at the root too).
    #[must_use]
    pub fn active_keys(&mut self) -> Vec<ActiveKey> {
        self.keymap
            .active_keys()
            .into_iter()
            .map(|key| ActiveKey {
                display: key.display,
                command: key.command,
                continues: key.continues,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_replace_substitutes_whole_tokens_only() {
        assert_eq!(alias_replace("enter", "enter", "return"), "return");
        assert_eq!(alias_replace("ctrl+enter", "enter", "return"), "ctrl+return");
        assert_eq!(alias_replace("enter>x", "enter", "return"), "return>x");
        // A suffix/prefix inside another word stays.
        assert_eq!(alias_replace("center", "enter", "return"), "center");
        assert_eq!(alias_replace("esc", "esc", "escape"), "escape");
        assert_eq!(alias_replace("pgup,pgdown", "pgup", "pageup"), "pageup,pgdown");
        assert_eq!(alias_replace("pgup,pgdown", "pgdown", "pagedown"), "pgup,pagedown");
    }

    #[test]
    fn chord_specs_render_donor_syntax() {
        let leader = KeyStroke::parse("ctrl+x").unwrap();
        let chord = super::super::table::Chord::parse("<leader>q", &leader).unwrap();
        assert_eq!(chord_spec(&chord, &leader), "<leader>q");
        let multi = super::super::table::Chord::parse("g>g", &leader).unwrap();
        assert_eq!(chord_spec(&multi, &leader), "gg");
        let single = super::super::table::Chord::parse("ctrl+shift+tab", &leader).unwrap();
        assert_eq!(chord_spec(&single, &leader), "ctrl+shift+tab");
    }
}
