//! Keypress replay harness — the wave-3 app-level keypress pump
//! ([`crate::app::App`]: store + keymap + dispatcher + composer +
//! footer blockers + render) driven by replay scripts
//! (`tools/parity/goldens/tui-frames/replay/*.json`).
//!
//! Pipeline per stroke: `keys → KeyStroke::parse → keymap (layer
//! order: dialog → permission → gated exit → textarea → input/base) →
//! store (dispatcher / composer / permission body) → run_effects →
//! render → expectations`. Frame checks run against the LIVE render
//! (route + blockers + dialog overlay).

use crate::app::App;

// -- replay scripts ------------------------------------------------------------

/// One replay script (`replay/*.json`).
#[derive(Debug, Clone)]
pub struct ReplayStep {
    pub keys: Vec<String>,
    pub expect_includes: Vec<String>,
    pub expect_excludes: Vec<String>,
    pub expect_dialogs: Option<usize>,
    pub expect_focus: Option<String>,
    pub expect_route: Option<(String, Option<String>)>,
    pub expect_exit: Option<bool>,
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
        let route = expect.get("route").and_then(|route| {
            let kind = route.get("type")?.as_str()?.to_owned();
            let session = route.get("sessionID").and_then(|id| id.as_str()).map(str::to_owned);
            Some((kind, session))
        });
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
            expect_route: route,
            expect_exit: expect.get("exit").and_then(serde_json::Value::as_bool),
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

/// The per-script scenario setup: the permission script's two asks are
/// armed by its recorded server-side flow (ask 1 before step 0, ask 2
/// once the once-reply resolves, after step 2) — the pump cannot
/// fabricate SSE events itself.
fn arm_scenario(script: &str, app: &mut App, before_step: usize) {
    if script != "permission-once-always-deny" {
        return;
    }
    let request = |id: &str| crate::components::footer::PermissionRequest {
        id: id.to_owned(),
        session_id: "ses_replay".to_owned(),
        action: "read".to_owned(),
        resources: vec!["src/index.ts".to_owned()],
        save: vec!["src/**".to_owned()],
        ..crate::components::footer::PermissionRequest::default()
    };
    match before_step {
        0 => crate::app::ask_permission(&app.store, &request("per_1")),
        3 => crate::app::ask_permission(&app.store, &request("per_2")),
        _ => {}
    }
}

/// Run every step of a script against a fresh pump; the first failure
/// short-circuits with a message. The pump mirrors the reference
/// app-lifecycle fixture (an active session — session-scoped commands
/// like rename need one) at its default 100×30 terminal.
pub fn run_script(script: &ReplayScript, size: (u16, u16)) -> Result<(), String> {
    let mut pump = App::new(size);
    arm_scenario(&script.id, &mut pump, 0);
    if script.id != "permission-once-always-deny" {
        pump.store.open_tab("ses_test", Some("Test session".to_owned()));
    }
    for (index, step) in script.steps.iter().enumerate() {
        if index > 0 {
            arm_scenario(&script.id, &mut pump, index);
        }
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
                return Err(format!("step {index}: frame must not include {needle:?}\n{frame}"));
            }
        }
        if let Some(want) = step.expect_dialogs {
            let got = pump.store.dialogs().len();
            if got != want {
                return Err(format!("step {index}: dialogs {got} != {want}"));
            }
        }
        if let Some(want) = &step.expect_focus {
            let got = pump.focus();
            if got != want {
                return Err(format!("step {index}: focus {got} != {want}"));
            }
        }
        if let Some((kind, session)) = &step.expect_route {
            let got = match pump.store.route() {
                crate::store::Route::Home => "home".to_owned(),
                crate::store::Route::Session { session_id } => {
                    if Some(&session_id) == session.as_ref() || session.is_none() {
                        "session".to_owned()
                    } else {
                        format!("session({session_id})")
                    }
                }
            };
            if &got != kind && got != format!("session({kind})") && got != *kind {
                return Err(format!("step {index}: route {got} != {kind}"));
            }
        }
        if let Some(want) = step.expect_exit {
            if pump.exit_requested != want {
                return Err(format!("step {index}: exit {} != {want}", pump.exit_requested));
            }
        }
    }
    Ok(())
}

/// Load and run one script by id from the goldens directory.
pub fn run_script_file(dir: &std::path::Path, id: &str, size: (u16, u16)) -> Result<(), String> {
    let json = std::fs::read_to_string(dir.join(format!("replay/{id}.json")))
        .map_err(|e| format!("replay script {id}: {e}"))?;
    let script = parse_script(&json).ok_or_else(|| format!("script {id} does not parse"))?;
    run_script(&script, size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Focus;
    use std::path::Path;

    fn goldens() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/parity/goldens/tui-frames")
    }

    /// All three wave-1/3 replay scripts pass on the unified pump.
    #[test]
    fn replay_exit_leader_q() {
        run_script_file(&goldens(), "exit-leader-q", (100, 30)).expect("replay passes");
    }

    #[test]
    fn replay_palette_ctrl_p() {
        run_script_file(&goldens(), "palette-ctrl-p", (100, 30)).expect("replay passes");
    }

    #[test]
    fn replay_permission_once_always_deny() {
        run_script_file(&goldens(), "permission-once-always-deny", (100, 30))
            .expect("replay passes");
    }

    /// The two wave-3 recorded scripts: the Sessions dialog flow and
    /// the theme-dialog select flow.
    #[test]
    fn replay_session_list_leader_l() {
        // The recorded scenario ran with two known sessions (the
        // fixture server's Alpha/Beta); the session-list dialog reads
        // the store's session HISTORY (the resync's session.list) —
        // stage the fixture's sessions through a resync batch.
        let script = load_script(&goldens(), "session-list-leader-l");
        let mut pump = App::new((100, 30));
        pump.store.ingest_all([crate::data::Ingest::Resync {
            sessions: vec![
                crate::data::SessionSnapshot {
                    model: None,
                    id: "ses_alpha".into(),
                    title: Some("Alpha".to_owned()),
                    parent_id: None,
                    time_idle: None,
                    viewed: None,
                    ..Default::default()
                },
                crate::data::SessionSnapshot {
                    model: None,
                    id: "ses_beta".into(),
                    title: Some("Beta".to_owned()),
                    parent_id: None,
                    time_idle: None,
                    viewed: None,
                    ..Default::default()
                },
            ],
            unread: Vec::new(),
            messages: Vec::new(),
            cursors: Vec::new(),
            permissions: Vec::new(),
            forms: Vec::new(),
        }]);
        run_steps(&mut pump, &script);
    }

    #[test]
    fn replay_theme_dialog_select() {
        run_script_file(&goldens(), "theme-dialog-select", (100, 30)).expect("replay passes");
    }

    fn load_script(dir: &std::path::Path, id: &str) -> ReplayScript {
        let json = std::fs::read_to_string(dir.join(format!("replay/{id}.json"))).expect("script");
        parse_script(&json).expect("parses")
    }

    fn run_steps(pump: &mut App, script: &ReplayScript) {
        for step in &script.steps {
            for key in &step.keys {
                pump.press(key);
            }
            pump.store.runtime.run_effects();
            let frame = pump.frame_text();
            for needle in &step.expect_includes {
                assert!(frame.contains(needle.as_str()), "missing {needle:?} in\n{frame}");
            }
            for needle in &step.expect_excludes {
                assert!(!frame.contains(needle.as_str()), "must exclude {needle:?}");
            }
            if let Some(want) = step.expect_dialogs {
                assert_eq!(pump.store.dialogs().len(), want);
            }
            if let Some(want) = &step.expect_focus {
                assert_eq!(pump.focus(), want);
            }
        }
    }

    /// The same flow asserted step by step (the pump's state machine,
    /// beyond the script's frame expectations). The recorded reference
    /// scenario ran inside an active session — the rename command is
    /// session-scoped.
    #[test]
    fn palette_open_filter_execute_flow() {
        let mut pump = App::new((100, 30));
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

    /// The exit gate beyond the script: ctrl+c exits on an empty
    /// prompt but CLEARS the draft when one exists (`input.clear`).
    #[test]
    fn ctrl_c_clears_then_exits() {
        let mut pump = App::new((100, 30));
        pump.store.open_tab("s1", Some("S".to_owned()));
        for key in ["a", "b"] {
            pump.press(key);
        }
        assert_eq!(pump.store.prompt_draft("s1").text, "ab");
        pump.press("ctrl+c");
        assert!(pump.store.prompt_draft("s1").is_empty(), "cleared, not exited");
        assert!(!pump.exit_requested);
        pump.press("ctrl+c");
        assert!(pump.exit_requested, "empty prompt: ctrl+c exits");
    }

    /// Leader chords work from the prompt (textarea-first only consumes
    /// keys the composer owns): ctrl+tab cycles, ctrl+1 switches with a
    /// draft present, leader+1/2 quick-switch fire on BOTH the empty and
    /// the non-empty prompt (D-02 — v2's keymap suspends the textarea
    /// for leader chords; the draft survives), leader+w closes the tab.
    #[test]
    fn leader_chords_and_tab_commands_dispatch_from_the_prompt() {
        let mut pump = App::new((100, 30));
        pump.store.open_tab("ses_a", Some("A".to_owned()));
        pump.store.open_tab("ses_b", Some("B".to_owned()));
        pump.store.select_tab("ses_a");
        // Draft text must not disable ctrl-modified chords.
        pump.press("x");
        assert_eq!(pump.store.prompt_draft("ses_a").text, "x");
        pump.press("ctrl+tab");
        assert_eq!(
            pump.store.route(),
            crate::store::Route::Session { session_id: "ses_b".to_owned() },
            "ctrl+tab cycles with a draft present"
        );
        pump.press("ctrl+1");
        assert_eq!(
            pump.store.route(),
            crate::store::Route::Session { session_id: "ses_a".to_owned() },
            "ctrl+1 quick slot with a draft present"
        );
        // D-02: leader continuations DISPATCH while text exists — the
        // chord suspends the textarea, and the draft survives intact.
        pump.press("ctrl+x");
        assert_eq!(pump.store.prompt_draft("ses_a").text, "x", "the leader never types");
        pump.press("2");
        assert_eq!(
            pump.store.route(),
            crate::store::Route::Session { session_id: "ses_b".to_owned() },
            "leader+2 quick slot with a draft present"
        );
        assert_eq!(
            pump.store.prompt_draft("ses_a").text,
            "x",
            "the executed chord leaves the draft intact"
        );
        // …and dispatch on the empty prompt too.
        pump.press("ctrl+x");
        pump.press("2");
        assert_eq!(
            pump.store.route(),
            crate::store::Route::Session { session_id: "ses_b".to_owned() },
            "leader+2 quick slot on the empty prompt"
        );
        pump.press("ctrl+x");
        pump.press("w");
        assert_eq!(pump.store.tabs().tabs.len(), 1, "leader+w closed the tab");
    }

    /// D-02 proper: the leader chord suspends a NON-empty composer —
    /// v2's keymap handles keys above the focused textarea
    /// (`terminal-pane.tsx:144` skips the leader/pending strokes for
    /// the focused surface; `prompt/index.tsx:192` mutes while pending).
    /// The chord executes, the draft is preserved, the textarea resumes;
    /// `app.exit` stays empty-prompt-conditional (`<leader>q` types `q`),
    /// and escape while pending clears the sequence without touching the
    /// draft.
    #[test]
    fn leader_chord_suspends_a_non_empty_draft_and_preserves_it() {
        let mut pump = App::new((100, 30));
        pump.store.open_tab("ses_a", Some("A".to_owned()));
        pump.store.open_tab("ses_b", Some("B".to_owned()));
        pump.store.select_tab("ses_a");
        for key in ["h", "i"] {
            pump.press(key);
        }
        assert_eq!(pump.store.prompt_draft("ses_a").text, "hi");
        // <leader> arms over the draft (consumed by the keymap, not typed).
        pump.press("ctrl+x");
        assert_eq!(pump.store.prompt_draft("ses_a").text, "hi");
        // The continuation executes the chord (session.new → Home); the
        // suspended draft survives on its session's composer.
        pump.press("n");
        assert_eq!(pump.store.route(), crate::store::Route::Home, "leader+n (session.new)");
        assert_eq!(
            pump.store.prompt_draft("ses_a").text,
            "hi",
            "the chord executed without touching the suspended draft"
        );
        // app.exit stays conditional: <leader>q over text resolves
        // through the base layer to session.queued_prompts (the OTHER
        // <leader>q binding — v2 dispatches it over text; our
        // dispatcher stub is a no-op), so the q never types and the
        // app never exits.
        for key in ["d", "r", "a", "f", "t"] {
            pump.press(key);
        }
        assert_eq!(pump.store.prompt_draft(crate::app::HOME_PROMPT_KEY).text, "draft");
        pump.press("ctrl+x");
        pump.press("q");
        assert_eq!(
            pump.store.prompt_draft(crate::app::HOME_PROMPT_KEY).text,
            "draft",
            "the chord consumed the q (dispatched, not typed)"
        );
        assert!(!pump.exit_requested, "app.exit requires the empty prompt");
        // Escape while pending clears the sequence — the keymap consumes
        // it before the composer's escape handling could.
        pump.press("ctrl+x");
        pump.press("escape");
        assert_eq!(
            pump.store.prompt_draft(crate::app::HOME_PROMPT_KEY).text,
            "draft",
            "escape cleared the sequence, the draft untouched"
        );
    }

    /// The composer: printable typing, submit through the sink, and
    /// history at the edges.
    #[test]
    fn composer_types_submits_and_walks_history() {
        let mut pump = App::new((100, 30));
        pump.store.open_tab("s1", Some("S".to_owned()));
        for key in ["h", "i"] {
            pump.press(key);
        }
        assert_eq!(pump.store.prompt_draft("s1").text, "hi");
        pump.press("enter");
        assert!(pump.store.prompt_draft("s1").is_empty(), "submitted");
        assert_eq!(pump.store.composer("s1").history.entries, vec!["hi".to_owned()]);
        pump.press("up");
        assert_eq!(pump.store.prompt_draft("s1").text, "hi", "history restored");
        pump.press("down");
        assert!(pump.store.prompt_draft("s1").is_empty(), "back to the live draft");
    }
}
