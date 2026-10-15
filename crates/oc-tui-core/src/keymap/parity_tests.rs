//! Lane M-engine's keymap parity + K-1..K-9 dissolution suite: every
//! default-table binding resolves IDENTICALLY on the legacy engine and
//! the opentui-keymap engine, and each registered K-delta shows its
//! before/after — the delta disappears because the subsystem
//! semantics now match upstream 0.5.14 (v2's engine), never because a
//! case was special-cased.
//!
//! The engine path latches at `Keymap` construction; the in-process
//! override ([`crate::__set_test_engine_override`]) selects the
//! variant under the parity lock (the flag's OnceLock makes env-based
//! A/B impossible inside one libtest process).

use super::opentui::stroke_input_for_test;
use super::*;
use opentui_keymap::types::KeyStrokeInput;
use std::sync::{Mutex, MutexGuard, OnceLock};

static PARITY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn parity_lock() -> MutexGuard<'static, ()> {
    PARITY_LOCK.get_or_init(|| Mutex::new(())).lock().expect("parity lock")
}

/// Sets the engine-path override for the guard's scope (resets on
/// drop — panics must not leak the override into other tests).
struct OverrideGuard(bool);

impl OverrideGuard {
    fn new(opentui: bool) -> Self {
        crate::__set_test_engine_override(Some(opentui));
        Self(opentui)
    }

    fn flip(&mut self, opentui: bool) {
        self.0 = opentui;
        crate::__set_test_engine_override(Some(opentui));
    }
}

impl Drop for OverrideGuard {
    fn drop(&mut self) {
        crate::__set_test_engine_override(None);
    }
}

/// Build a keymap on the given engine path (call sites hold the
/// parity lock + an [`OverrideGuard`]).
fn keymap_on(opentui: bool) -> Keymap {
    crate::__set_test_engine_override(Some(opentui));
    Keymap::new()
}

fn stroke(spec: &str) -> KeyStroke {
    KeyStroke::parse(spec).expect("test stroke parses")
}

/// Feed a chord's strokes; the intermediate outcomes and the final
/// one (the parity comparison feeds stroke by stroke and compares
/// EVERY step).
fn feed(keymap: &mut Keymap, chord: &Chord, clock: &dyn Clock) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    for stroke in &chord.strokes {
        outcomes.push(keymap.lookup(stroke.clone(), clock));
    }
    keymap.clear_pending();
    outcomes
}

/// The parity key: kind + command, with Pending displays normalized —
/// the opentui engine shows the `<leader>` TOKEN form (upstream's
/// `getPendingSequence` display, the v2-faithful form), the legacy
/// engine the raw stroke; resolution parity is about WHICH command
/// fires, never the display string.
fn parity_key(outcomes: Vec<Outcome>) -> Vec<(&'static str, Option<String>)> {
    outcomes
        .into_iter()
        .map(|outcome| match outcome {
            Outcome::Command { command, .. } => ("command", Some(command)),
            Outcome::Pending { .. } => ("pending", None),
            Outcome::Cancelled => ("cancelled", None),
            Outcome::Unhandled => ("unhandled", None),
        })
        .collect()
}

/// THE resolution parity table — generated from the v2 default table
/// (`config/keybind.ts` `Definitions`, the data both engines consume):
/// every command's every effective spec, resolved in its own layer's
/// mode context, must produce the IDENTICAL outcome sequence on both
/// engines. Single strokes, leader chords, all four named modes.
#[test]
fn resolution_parity_table_both_engines() {
    let _guard = parity_lock();
    let leader = stroke(LEADER_DEFAULT);
    let mut legacy = keymap_on(false);
    let mut opentui = keymap_on(true);
    let clock = ManualClock::default();

    let mut cases = 0usize;
    for def in default_table() {
        // The `leader` entry defines the leader stroke itself (defaults
        // "ctrl+x") — not a resolvable binding (it arms forever).
        if def.bind_name == "leader" {
            continue;
        }
        for spec in def.defaults.split(',').map(str::trim) {
            if spec.is_empty() || spec == "none" {
                continue;
            }
            let Some(chord) = Chord::parse(spec, &leader) else { continue };
            let modes: Vec<&str> =
                if def.layer == "base" { Vec::new() } else { vec![def.layer] };
            let legacy_outcomes = {
                legacy.set_modes(&modes);
                let out = feed(&mut legacy, &chord, &clock);
                legacy.set_modes(&[]);
                out
            };
            let opentui_outcomes = {
                opentui.set_modes(&modes);
                let out = feed(&mut opentui, &chord, &clock);
                opentui.set_modes(&[]);
                out
            };
            // The v2 alias expander (enter→return, esc→escape, pgdown
            // →pagedown, pgup→pageup — context/keymap.tsx:89-99) is a
            // K-7-family registration the legacy engine never had:
            // an alias-keyed spec fires on its CANONICAL stroke on the
            // opentui engine (the live pump's stroke vocabulary) and
            // on the literal alias on legacy. Assert the opentui
            // side's canonical resolution, then continue.
            let alias_specs = ["enter", "esc", "pgdown", "pgup"];
            if alias_specs.contains(&spec) {
                let canonical = match spec {
                    "enter" => "return",
                    "esc" => "escape",
                    "pgdown" => "pagedown",
                    _ => "pageup",
                };
                opentui.set_modes(&modes);
                let outcome = opentui.lookup(stroke(canonical), &clock);
                opentui.set_modes(&[]);
                assert_eq!(
                    outcome,
                    Outcome::Command { command: def.command.to_owned(), args: None },
                    "{spec} resolves on the canonical {canonical} (the alias expander)"
                );
                cases += 1;
                continue;
            }
            assert_eq!(
                parity_key(legacy_outcomes.clone()),
                parity_key(opentui_outcomes.clone()),
                "{spec} ({}.{}) diverged: legacy {legacy_outcomes:?} vs opentui {opentui_outcomes:?}",
                def.layer, def.command
            );
            // The chord must RESOLVE (the last outcome is the command)
            // on both paths — a spec that cannot fire is table debt.
            assert!(
                matches!(legacy_outcomes.last(), Some(Outcome::Command { .. })),
                "{spec} ({}) did not resolve on the legacy engine",
                def.command
            );
            cases += 1;
        }
    }
    assert!(cases > 190, "the full table exercised, got {cases} specs");
}

/// Conflict parity: strokes claimed by MULTIPLE commands resolve to
/// the same command on both engines (the layer/walk order — K-9's
/// consistent semantics).
#[test]
fn conflicting_strokes_resolve_identically() {
    let _guard = parity_lock();
    let mut legacy = keymap_on(false);
    let mut opentui = keymap_on(true);
    let clock = ManualClock::default();
    let conflicts: &[(&str, &str)] = &[
        // ctrl+d: queued_prompt.delete vs dialog.move_session.delete.
        ("", "ctrl+d"),
        ("dialog", "ctrl+d"),
        // ctrl+p: palette (base) vs dialog.select.prev (dialog).
        ("", "ctrl+p"),
        ("dialog", "ctrl+p"),
        // down: picker toggle (base) vs input.move.down vs autocomplete.
        ("", "down"),
        ("input", "down"),
        ("autocomplete", "down"),
    ];
    for (mode, spec) in conflicts {
        let modes: Vec<&str> = if mode.is_empty() { vec![] } else { vec![mode] };
        for keymap in [&mut legacy, &mut opentui] {
            keymap.set_modes(&modes);
            let _ = keymap.lookup(stroke(spec), &clock);
            keymap.set_modes(&[]);
        }
        legacy.set_modes(&modes);
        let legacy_outcome = legacy.lookup(stroke(spec), &clock);
        legacy.set_modes(&[]);
        opentui.set_modes(&modes);
        let opentui_outcome = opentui.lookup(stroke(spec), &clock);
        opentui.set_modes(&[]);
        assert_eq!(
            legacy_outcome, opentui_outcome,
            "{spec} in mode {mode:?} diverged"
        );
    }
}

/// Leader timeout parity (K-4): v2 pins 2000 ms — BOTH engines keep a
/// leader sequence live at 1999 ms and expire it at 2000.
#[test]
fn k4_timed_leader_timeout_is_2000_on_both_engines() {
    let _guard = parity_lock();
    for opentui in [false, true] {
        let mut keymap = keymap_on(opentui);
        let clock = ManualClock::default();
        assert!(
            matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }),
            "leader arms (opentui={opentui})"
        );
        clock.advance(LEADER_TIMEOUT_MS - 1);
        assert!(!keymap.expire(&clock), "live one ms early (opentui={opentui})");
        clock.advance(1);
        assert!(keymap.expire(&clock), "expires at 2000 (opentui={opentui})");
    }
}

/// **K-1 — non-leader multi-press sequences arm.** Legacy: a `g>g`
/// binding never arms (`g` falls through, `g` again falls through) —
/// the class that made v2's gg/dd sequences untriggerable. Opentui:
/// `g` PENDS (the sequence holds), `g` completes.
#[test]
fn k1_nonleader_sequences_arm_on_the_opentui_engine_only() {
    let _guard = parity_lock();
    let overrides = parse_keybind_overrides(&serde_json::json!({ "session.rename": "g>g" }));
    let clock = ManualClock::default();

    let mut legacy = keymap_on(false);
    legacy.apply_overrides(&overrides);
    assert_eq!(legacy.lookup(stroke("g"), &clock), Outcome::Unhandled, "K-1 on legacy: g never arms");
    assert_eq!(legacy.lookup(stroke("g"), &clock), Outcome::Unhandled);

    let mut opentui = keymap_on(true);
    opentui.apply_overrides(&overrides);
    assert!(
        matches!(opentui.lookup(stroke("g"), &clock), Outcome::Pending { .. }),
        "K-1 dissolved: g arms the sequence"
    );
    assert_eq!(
        opentui.lookup(stroke("g"), &clock),
        Outcome::Command { command: "session.rename".to_owned(), args: None },
        "gg completes"
    );
}

/// **K-2 — a sequence miss clears without redispatch.** With a root
/// binding on `z`: legacy re-evaluates the missed stroke fresh from
/// the root (`ctrl+x z` FIRES z's command); upstream returns
/// sequence-miss UNCONSUMED — the stroke falls to the focused surface
/// (the composer), never re-resolving.
#[test]
fn k2_sequence_miss_does_not_redispatch_on_the_opentui_engine() {
    let _guard = parity_lock();
    let overrides =
        parse_keybind_overrides(&serde_json::json!({ "session.rename": "z", "app.exit": "ctrl+c" }));
    let clock = ManualClock::default();

    let mut legacy = keymap_on(false);
    legacy.apply_overrides(&overrides);
    assert!(matches!(legacy.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        legacy.lookup(stroke("z"), &clock),
        Outcome::Command { command: "session.rename".to_owned(), args: None },
        "K-2 on legacy: the miss re-evaluates z from the root"
    );

    let mut opentui = keymap_on(true);
    opentui.apply_overrides(&overrides);
    assert!(matches!(opentui.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        opentui.lookup(stroke("z"), &clock),
        Outcome::Unhandled,
        "K-2 dissolved: the miss clears unconsumed, z falls to the surface"
    );
    // The missed stroke is not sticky: the next root stroke resolves.
    assert_eq!(
        opentui.lookup(stroke("z"), &clock),
        Outcome::Command { command: "session.rename".to_owned(), args: None },
    );
}

/// **K-3 — a high layer's continuation shadows a low layer's exact.**
/// Input mode binds `a>b`; base binds exact `a`. Legacy: `a` resolves
/// the base exact immediately (continuations only count leader-first
/// prefixes). Upstream: the input layer's continuation HOLDS — `a`
/// pends, `b` completes the input chord; the base exact never fires.
#[test]
fn k3_continuation_shadows_exact_across_layers() {
    let _guard = parity_lock();
    let overrides = parse_keybind_overrides(&serde_json::json!({
        "input.move.left": "a>b",   // layer: input
        "session.rename": "a",      // layer: base
    }));
    let clock = ManualClock::default();

    let mut legacy = keymap_on(false);
    legacy.apply_overrides(&overrides);
    legacy.set_modes(&["input"]);
    assert_eq!(
        legacy.lookup(stroke("a"), &clock),
        Outcome::Command { command: "session.rename".to_owned(), args: None },
        "K-3 on legacy: the base exact wins under input mode"
    );

    let mut opentui = keymap_on(true);
    opentui.apply_overrides(&overrides);
    opentui.set_modes(&["input"]);
    assert!(
        matches!(opentui.lookup(stroke("a"), &clock), Outcome::Pending { .. }),
        "K-3 dissolved: the input continuation holds over the base exact"
    );
    assert_eq!(
        opentui.lookup(stroke("b"), &clock),
        Outcome::Command { command: "input.move.left".to_owned(), args: None },
        "ab completes the input chord"
    );
}

/// **K-5 — hyper stays distinct from super** (canonical modifier
/// order ctrl,shift,meta,super,hyper; no folding into super+meta).
/// The adapter's stroke conversion carries `hyper` separately; the
/// donor's normalizer orders and keeps it.
#[test]
fn k5_hyper_modifier_stays_distinct() {
    let _guard = parity_lock();
    let super_stroke = KeyStroke { key: "a".into(), ctrl: false, shift: false, alt: false, super_key: true };
    let input = opentui::stroke_input_for_test(&super_stroke);
    // The donor's normalizer: super and hyper produce DIFFERENT
    // normal forms (never folded into a super+meta alias).
    let super_only = opentui_keymap::stringify::normalize_key_stroke(&input).expect("normalizes");
    let hyper_input = KeyStrokeInput { hyper: Some(true), super_key: Some(false), ..input.clone() };
    let hyper_only = opentui_keymap::stringify::normalize_key_stroke(&hyper_input).expect("normalizes");
    assert_ne!(super_only, hyper_only, "K-5 dissolved: hyper stays a distinct modifier");
}

/// **K-6 — ANY escape clears a pending sequence.** Legacy: only a
/// BARE escape clears (ctrl+escape falls through). v2 registers the
/// escape-clears ADDON — every escape clears + consumes.
#[test]
fn k6_any_escape_clears_pending() {
    let _guard = parity_lock();
    let clock = ManualClock::default();

    let mut legacy = keymap_on(false);
    assert!(matches!(legacy.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        legacy.lookup(stroke("ctrl+escape"), &clock),
        Outcome::Unhandled,
        "K-6 on legacy: ctrl+escape does not clear"
    );

    let mut opentui = keymap_on(true);
    assert!(matches!(opentui.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        opentui.lookup(stroke("ctrl+escape"), &clock),
        Outcome::Cancelled,
        "K-6 dissolved: the addon clears on any escape"
    );
}

/// **K-7 — backspace pops a pending segment; comma specs expand.**
/// Legacy: backspace on a pending sequence re-evaluates fresh
/// (unbound → falls through, the sequence lost). Upstream: the addon
/// POPS the last typed segment (a one-part sequence clears), consumed.
/// Comma specs (`j,k`) bind both strokes on BOTH engines (the legacy
/// table splits at the spec level; upstream's expander splits inside
/// the parser) — parity, not delta.
#[test]
fn k7_backspace_pops_and_comma_expands() {
    let _guard = parity_lock();
    let clock = ManualClock::default();

    let mut legacy = keymap_on(false);
    assert!(matches!(legacy.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        legacy.lookup(stroke("backspace"), &clock),
        Outcome::Unhandled,
        "K-7 on legacy: backspace re-evaluates fresh"
    );

    let mut opentui = keymap_on(true);
    assert!(matches!(opentui.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        opentui.lookup(stroke("backspace"), &clock),
        Outcome::Cancelled,
        "K-7 dissolved: the addon pops the pending segment"
    );

    // Comma parity on both engines.
    let overrides =
        parse_keybind_overrides(&serde_json::json!({ "session.rename": "j,k" }));
    let mut legacy = keymap_on(false);
    legacy.apply_overrides(&overrides);
    let mut opentui = keymap_on(true);
    opentui.apply_overrides(&overrides);
    for keymap in [&mut legacy, &mut opentui] {
        assert_eq!(
            keymap.lookup(stroke("j"), &clock),
            Outcome::Command { command: "session.rename".to_owned(), args: None },
        );
        assert_eq!(
            keymap.lookup(stroke("k"), &clock),
            Outcome::Command { command: "session.rename".to_owned(), args: None },
        );
    }
}

/// **K-8 — `getActiveKeys` lists reachable keys at the root.**
/// Legacy: empty without a pending sequence. Opentui: the root's
/// reachable single keys (ctrl+p, escape, …).
#[test]
fn k8_active_keys_list_at_the_root() {
    let _guard = parity_lock();
    let mut legacy = keymap_on(false);
    assert!(legacy.active_keys().is_empty(), "K-8 on legacy: empty at the root");

    let mut opentui = keymap_on(true);
    let keys = opentui.active_keys();
    assert!(
        keys.iter().any(|key| key.display.contains("ctrl+p")),
        "K-8 dissolved: the root lists reachable keys ({:?})",
        keys.iter().map(|key| key.display.as_str()).take(8).collect::<Vec<_>>()
    );
}

/// **K-9 — the consistent semantics stay consistent** (layer order,
/// mode shadowing, rejection fall-through, the disable gate): the
/// legacy suite's behavior cases re-run on the opentui engine.
#[test]
fn k9_consistent_semantics_hold_on_both_engines() {
    let _guard = parity_lock();
    let clock = ManualClock::default();
    for opentui in [false, true] {
        let mut keymap = keymap_on(opentui);

        // Mode shadowing: input.down beats base down; autocomplete
        // beats input; back to base after pops.
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "session.child.first".to_owned(), args: None },
        );
        keymap.push_mode("input");
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "input.move.down".to_owned(), args: None },
        );
        keymap.push_mode("autocomplete");
        assert_eq!(
            keymap.lookup(stroke("down"), &clock),
            Outcome::Command { command: "prompt.autocomplete.next".to_owned(), args: None },
        );
        keymap.pop_mode();
        keymap.pop_mode();

        // The disable gate: a disabled command's stroke falls through
        // (may resolve a lower claim — app.exit's ctrl+c has no other
        // claimant → Unhandled).
        keymap.set_command_enabled("app.exit", false);
        assert_eq!(keymap.lookup(stroke("ctrl+c"), &clock), Outcome::Unhandled);
        keymap.set_command_enabled("app.exit", true);
        assert_eq!(
            keymap.lookup(stroke("ctrl+c"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None },
        );

        // Leader chords resolve + the pending indicator surfaces
        // (the opentui display is the `<leader>` token form).
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(
            keymap.lookup(stroke("n"), &clock),
            Outcome::Command { command: "session.new".to_owned(), args: None },
        );

        // Escape clears then interrupts.
        assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
        assert_eq!(keymap.lookup(stroke("escape"), &clock), Outcome::Cancelled);
        assert_eq!(
            keymap.lookup(stroke("escape"), &clock),
            Outcome::Command { command: "session.interrupt".to_owned(), args: None },
        );
    }
}

/// The full legacy replay suite's anchor cases on the opentui engine —
/// the always-green pre-M-engine tests, re-run per path (the mod.rs
// suite runs them on the latched default path; this pins the opentui
/// side to the same anchors).
#[test]
fn top_bindings_resolve_on_both_engines() {
    let _guard = parity_lock();
    let clock = ManualClock::default();
    for opentui in [false, true] {
        let mut keymap = keymap_on(opentui);
        for (spec, command) in [
            ("ctrl+p", "command.palette.show"),
            ("ctrl+o", "open.menu"),
            ("ctrl+tab", "session.tab.next"),
            ("ctrl+shift+tab", "session.tab.previous"),
            ("f2", "model.cycle_recent"),
            ("shift+f2", "model.cycle_recent_reverse"),
            ("escape", "session.interrupt"),
            ("ctrl+z", "terminal.suspend"),
            ("ctrl+1", "session.tab.select.1"),
            ("pageup", "session.page.up"),
            ("alt+shift+down", "session.tab.next_unread"),
        ] {
            assert_eq!(
                keymap.lookup(stroke(spec), &clock),
                Outcome::Command { command: command.to_owned(), args: None },
                "{spec} → {command} (opentui={opentui})"
            );
        }
        // Leader chord family.
        for (tail, command) in [
            ("n", "session.new"),
            ("e", "prompt.editor"),
            ("m", "model.list"),
            ("w", "session.tab.close"),
            ("0", "session.tab.select.10"),
        ] {
            assert!(
                matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }),
                "leader arms (opentui={opentui})"
            );
            assert_eq!(
                keymap.lookup(stroke(tail), &clock),
                Outcome::Command { command: command.to_owned(), args: None },
                "ctrl+x {tail} (opentui={opentui})"
            );
        }
    }
}

/// Overrides + leader rebinds survive the opentui rebuild path
/// (`apply_overrides` → `rebuild_engine` re-registers the donor
/// layers from the mutated rule sets).
#[test]
fn overrides_and_leader_rebinds_work_on_the_opentui_engine() {
    let _guard = parity_lock();
    let clock = ManualClock::default();
    let mut keymap = keymap_on(true);
    keymap.apply_overrides(&parse_keybind_overrides(&serde_json::json!({
        "app.exit": "ctrl+q,<leader>z",
        "command.palette.show": "none",
    })));
    assert_eq!(
        keymap.lookup(stroke("ctrl+q"), &clock),
        Outcome::Command { command: "app.exit".to_owned(), args: None }
    );
    assert!(matches!(keymap.lookup(stroke("ctrl+x"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        keymap.lookup(stroke("z"), &clock),
        Outcome::Command { command: "app.exit".to_owned(), args: None }
    );
    assert_eq!(keymap.lookup(stroke("ctrl+c"), &clock), Outcome::Unhandled, "old default replaced");
    assert_eq!(keymap.lookup(stroke("ctrl+p"), &clock), Outcome::Unhandled, "none unsets");

    // Leader rebind.
    keymap.apply_overrides(&parse_keybind_overrides(&serde_json::json!({ "leader": "ctrl+\\" })));
    assert!(matches!(keymap.lookup(stroke("ctrl+\\"), &clock), Outcome::Pending { .. }));
    assert_eq!(
        keymap.lookup(stroke("n"), &clock),
        Outcome::Command { command: "session.new".to_owned(), args: None },
    );

    // Owned (plugin) entries resolve after static — the rebuild path.
    // The owned spec uses the LEADER-RELATIVE form (register_owned's
    // plugin-facing "<leader>h" syntax, re-parsed under the CURRENT
    // leader): the chord arms with the static leader chords and
    // completes on h.
    keymap.register_owned(OwnedCommandDef {
        command: "hw.hello".to_owned(),
        description: "hello".to_owned(),
        defaults: vec!["<leader>h".to_owned()],
        layer: "base".to_owned(),
        palette: true,
        plugin: "helloworld".to_owned(),
    });
    assert!(
        matches!(keymap.lookup(stroke("ctrl+\\"), &clock), Outcome::Pending { .. }),
        "the leader arms with the owned chord among its continuations"
    );
    assert_eq!(
        keymap.lookup(stroke("h"), &clock),
        Outcome::Command { command: "hw.hello".to_owned(), args: None }
    );
    // The none-unset persists across the leader rebind rebuild.
    assert_eq!(keymap.lookup(stroke("ctrl+p"), &clock), Outcome::Unhandled);
}

/// `lookup_base` parity: the conditional app.exit rung resolves
/// against base with the pending sequence intact — both engines.
#[test]
fn lookup_base_resolves_identically() {
    let _guard = parity_lock();
    let clock = ManualClock::default();
    for opentui in [false, true] {
        let mut keymap = keymap_on(opentui);
        // A leader chord completes THROUGH the base-only dispatch.
        assert!(
            matches!(keymap.lookup_base(stroke("ctrl+x"), &clock), Outcome::Pending { .. }),
            "leader arms base-only (opentui={opentui})"
        );
        assert_eq!(
            keymap.lookup_base(stroke("n"), &clock),
            Outcome::Command { command: "session.new".to_owned(), args: None },
            "the chord completes base-only (opentui={opentui})"
        );
        // Mode shadowing is suspended: input's escape-closes binding
        // does not intercept base resolution.
        keymap.push_mode("input");
        assert_eq!(
            keymap.lookup_base(stroke("ctrl+c"), &clock),
            Outcome::Command { command: "app.exit".to_owned(), args: None },
            "app.exit resolves base-only under input mode (opentui={opentui})"
        );
        keymap.pop_mode();
    }
}
