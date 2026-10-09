//! Keypress replay harness — the seed of the wave-3 key-replay
//! framework: a minimal pump that feeds key strings through the keymap
//! into the store (route commands through the command dispatcher,
//! dialog keys through the dialog framework's modal layer), pumps the
//! reactive runtime, renders the app frame and evaluates replay-script
//! expectations (`tools/parity/goldens/tui-frames/replay/*.json`).
//!
//! Pipeline per stroke: `keys → KeyStroke::parse → keymap (mode-synced)
//! → store (dispatcher / dialog layer) → run_effects → render →
//! expectations`.

use crate::components::command_palette::{CommandDispatcher as _, Dispatch, StoreDispatcher};
use crate::components::dialog::{
    execute_submission, handle_dialog_key, render_app, DialogKeyOutcome,
};
use crate::keymap::{KeyStroke, Keymap, ManualClock, Outcome};
use crate::render::Frame;
use crate::store::{Focus, Store};
use crate::theme::ThemeState;

/// The headless app: store + keymap + dispatcher + render, with the
/// keymap's `dialog` mode kept in sync with the dialog stack.
pub struct AppPump {
    pub store: Store,
    keymap: Keymap,
    clock: ManualClock,
    dispatcher: StoreDispatcher,
    dialog_mode: bool,
    theme: ThemeState,
    size: (u16, u16),
    /// Whether the exit command fired (the run-loop teardown itself is
    /// wave 3).
    pub exit_requested: bool,
}

impl AppPump {
    #[must_use]
    pub fn new(size: (u16, u16)) -> Self {
        Self {
            store: Store::new(crate::reactive::Runtime::new()),
            keymap: Keymap::new(),
            clock: ManualClock::default(),
            dispatcher: StoreDispatcher,
            dialog_mode: false,
            theme: ThemeState::v2_dark(),
            size,
            exit_requested: false,
        }
    }

    /// Feed one key spec ("ctrl+p", "r", "enter", "leader" chords come
    /// as separate strokes). Replay-script key tokens use the kitty
    /// vocabulary; the reference keymap expands the aliases
    /// (`enter → return`, `esc → escape`, `pgdown/pgup → page…`).
    pub fn press(&mut self, spec: &str) {
        self.sync_dialog_mode();
        let expanded = match spec {
            "enter" => "return",
            "esc" => "escape",
            "pgdown" => "pagedown",
            "pgup" => "pageup",
            other => other,
        };
        let Some(stroke) = KeyStroke::parse(expanded) else { return };
        if self.store.focus() == Focus::Dialog {
            self.stroke_in_dialog(stroke);
        } else {
            self.stroke_on_route(stroke);
        }
        self.sync_dialog_mode();
        self.store.runtime.run_effects();
    }

    fn stroke_in_dialog(&mut self, stroke: KeyStroke) {
        // The pump owns the keymap; the dialog layer borrows it for the
        // dialog.* commands — split borrows via a temporary take.
        let mut keymap = std::mem::take(&mut self.keymap);
        let outcome = handle_dialog_key(&self.store, &mut keymap, &self.clock, stroke, self.size);
        self.keymap = keymap;
        match outcome {
            DialogKeyOutcome::Submitted(value) => {
                let dispatch = execute_submission(&self.store, &mut self.dispatcher, &value);
                if let Dispatch::ServerPending(_) = dispatch {
                    // Server-backed submissions close the palette and
                    // surface nothing further in wave 1.
                }
            }
            DialogKeyOutcome::PromptSubmitted(value) => self.submit_rename(&value),
            _ => {}
        }
    }

    fn stroke_on_route(&mut self, stroke: KeyStroke) {
        if let Outcome::Command { command, args } = self.keymap.lookup(stroke, &self.clock) {
            let dispatch = self.dispatcher.dispatch(&self.store, &command, args.as_deref());
            if command == "app.exit" && dispatch != Dispatch::Unknown {
                self.exit_requested = true;
            }
        }
    }

    /// Rename submit: update the tab title locally; the server call is
    /// wave-3 oc-client wiring (TODO(7.3): `session.update`).
    fn submit_rename(&mut self, value: &str) {
        let dialogs = self.store.dialogs();
        if let Some(entry) = dialogs.last() {
            if let crate::store::DialogKind::SessionRename { session_id } = &entry.kind {
                let session_id = session_id.clone();
                if !value.is_empty() {
                    self.store.open_tab(&session_id, Some(value.to_owned()));
                }
            }
        }
        self.store.clear_dialogs();
    }

    /// Keep the keymap's `dialog` mode aligned with the dialog stack
    /// (the reference pushes keymap mode `modal` while dialogs are
    /// open — our dialog.* commands live in the `dialog` layer).
    fn sync_dialog_mode(&mut self) {
        let want = !self.store.dialogs().is_empty();
        if want != self.dialog_mode {
            if want {
                self.keymap.push_mode("dialog");
            } else {
                self.keymap.pop_mode();
            }
            self.dialog_mode = want;
        }
    }

    /// Render the current app frame (route + overlay).
    #[must_use]
    pub fn frame(&self) -> Frame {
        render_app(&self.store, &self.theme, self.size)
    }

    /// The frame as one text block (the `frameIncludes` surface).
    #[must_use]
    pub fn frame_text(&self) -> String {
        self.frame().text_grid().join("\n")
    }
}

// -- replay scripts ------------------------------------------------------------

/// One replay script (`replay/*.json`).
#[derive(Debug, Clone)]
pub struct ReplayStep {
    pub keys: Vec<String>,
    pub expect_includes: Vec<String>,
    pub expect_excludes: Vec<String>,
    pub expect_dialogs: Option<usize>,
    pub expect_focus: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReplayScript {
    pub id: String,
    pub steps: Vec<ReplayStep>,
}

/// Parse one replay script JSON.
#[must_use]
pub fn parse_script(json: &str) -> Option<ReplayScript> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let id = value.get("id")?.as_str()?.to_owned();
    let mut steps = Vec::new();
    for step in value.get("steps")?.as_array()? {
        let keys = step
            .get("keys")?
            .as_array()?
            .iter()
            .filter_map(|key| key.as_str().map(str::to_owned))
            .collect();
        let expect = step.get("expect").cloned().unwrap_or(serde_json::Value::Null);
        steps.push(ReplayStep {
            keys,
            expect_includes: string_array(expect.get("frameIncludes")),
            expect_excludes: string_array(expect.get("frameExcludes")),
            expect_dialogs: expect
                .get("dialogs")
                .and_then(serde_json::Value::as_u64)
                .map(|count| usize::try_from(count).unwrap_or(0)),
            expect_focus: expect
                .get("focus")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        });
    }
    Some(ReplayScript { id, steps })
}

fn string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// The outcome of running one step's expectations against a pump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepCheck {
    Pass,
    Fail(String),
}

/// Run every step of a script against a fresh pump; the first failure
/// short-circuits with a message. The pump mirrors the reference
/// app-lifecycle fixture (an active session — session-scoped commands
/// like rename need one) at its default 100×30 terminal.
pub fn run_script(script: &ReplayScript, size: (u16, u16)) -> Result<(), String> {
    let mut pump = AppPump::new(size);
    pump.store.open_tab("ses_test", Some("Test session".to_owned()));
    for (index, step) in script.steps.iter().enumerate() {
        for key in &step.keys {
            pump.press(key);
        }
        pump.store.runtime.run_effects();
        let frame = pump.frame_text();
        for needle in &step.expect_includes {
            if !frame.contains(needle.as_str()) {
                return Err(format!("step {index}: frame does not include {needle:?}\n{frame}"));
            }
        }
        for needle in &step.expect_excludes {
            if frame.contains(needle.as_str()) {
                return Err(format!("step {index}: frame must not include {needle:?}"));
            }
        }
        if let Some(want) = step.expect_dialogs {
            let got = pump.store.dialogs().len();
            if got != want {
                return Err(format!("step {index}: dialogs {got} != {want}"));
            }
        }
        if let Some(want) = &step.expect_focus {
            let got = match pump.store.focus() {
                Focus::Dialog => "dialog",
                Focus::Route => "route",
            };
            if got != want {
                return Err(format!("step {index}: focus {got} != {want}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn goldens() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
    }

    /// The `palette-ctrl-p` replay: open → filter → execute.
    #[test]
    fn replay_palette_ctrl_p() {
        let json = std::fs::read_to_string(goldens().join("replay/palette-ctrl-p.json"))
            .expect("replay script");
        let script = parse_script(&json).expect("script parses");
        assert_eq!(script.id, "palette-ctrl-p");
        run_script(&script, (100, 30)).expect("replay passes"); // the reference app-fixture terminal (test/fixture/app.ts defaults)
    }

    /// The same flow asserted step by step (the pump's state machine,
    /// beyond the script's frame expectations). The recorded reference
    /// scenario ran inside an active session — the rename command is
    /// session-scoped.
    #[test]
    fn palette_open_filter_execute_flow() {
        let mut pump = AppPump::new((100, 30));
        pump.store.open_tab("ses_test", Some("Test session".to_owned()));
        pump.press("ctrl+p");
        assert_eq!(pump.store.dialogs().len(), 1);
        assert_eq!(pump.store.focus(), Focus::Dialog);
        assert!(pump.frame_text().contains("New session"), "suggested/registry rows visible");
        for key in ["r", "e", "n", "a", "m", "e"] {
            pump.press(key);
        }
        let frame = pump.frame_text();
        assert!(frame.contains("Rename session"), "{frame}");
        assert!(!frame.contains("New session"), "{frame}");
        // Enter executes session.rename: the palette is REPLACED by the
        // rename dialog.
        pump.press("enter");
        assert_eq!(pump.store.dialogs().len(), 1, "replace, not push");
        assert!(matches!(
            pump.store.top_dialog().unwrap().kind,
            crate::store::DialogKind::SessionRename { .. }
        ));
        assert!(pump.frame_text().contains("Rename session"));
        // Typing feeds the rename prompt; enter renames the tab
        // (locally) and closes.
        for key in ["F", "i", "x"] {
            pump.press(key);
        }
        pump.press("enter");
        assert_eq!(pump.store.dialogs().len(), 0);
        assert_eq!(pump.store.focus(), Focus::Route);
    }

    /// Escape closes the top dialog; ctrl+c clears a typed filter
    /// first (the modal layer's semantics).
    #[test]
    fn modal_escape_and_ctrl_c_semantics() {
        let mut pump = AppPump::new((100, 30));
        pump.press("ctrl+p");
        for key in ["z", "z"] {
            pump.press(key);
        }
        assert_eq!(pump.store.top_dialog().unwrap().select.filter, "zz", "filter typed");
        pump.press("ctrl+c");
        assert_eq!(pump.store.top_dialog().unwrap().select.filter, "", "cleared, not closed");
        assert_eq!(pump.store.dialogs().len(), 1);
        pump.press("ctrl+c");
        assert!(pump.store.dialogs().is_empty(), "second ctrl+c closes");
        pump.press("ctrl+p");
        pump.press("escape");
        assert!(pump.store.dialogs().is_empty(), "escape closes");
    }

    /// Navigation moves the selection (down/ctrl+n wrap).
    #[test]
    fn palette_navigation_moves_selection() {
        let mut pump = AppPump::new((100, 30));
        pump.press("ctrl+p");
        let first = pump.store.top_dialog().unwrap().select.selected;
        pump.press("down");
        let second = pump.store.top_dialog().unwrap().select.selected;
        assert_ne!(first, second);
        pump.press("ctrl+n");
        assert_ne!(pump.store.top_dialog().unwrap().select.selected, second);
        pump.press("up");
        assert_eq!(pump.store.top_dialog().unwrap().select.selected, second);
    }
}
