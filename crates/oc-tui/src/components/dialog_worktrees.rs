//! The Worktrees dialog (`component/dialog-workspaces.tsx` +
//! `component/dialog-worktree-name.tsx`): the session-move surface
//! behind `/worktrees` and the palette's "Manage workspaces".
//!
//! Shape (v2): an XLARGE `DialogSelect` titled "Worktrees" listing
//! the project's worktree directories — the CURRENT checkout's root
//! first, unmanaged roots before strategy-managed ones — as
//! "Current"/"Other" categories, with the footer action set
//! `move` (ctrl+m, session routes) / `new` (ctrl+a) / `delete`
//! (ctrl+d, two-press confirm) / `refresh` (ctrl+r). `new` opens the
//! name dialog (tab generates a slug, enter confirms a slugified
//! name); a delete that the server rejects with `forceRequired`
//! opens the file-changes confirm before the forced remove.

use crate::components::dialog_select::{SelectAction, SelectOption, SelectProps};

/// One `worktree.list` row (`Worktree.Directory` — `strategy` absent
/// on unmanaged/discovery roots).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub directory: String,
    /// The owning strategy id ("manual", "git", …); `None` = an
    /// unmanaged root the dialog can highlight but not delete.
    pub strategy: Option<String>,
}

/// The dialog's transient state (the solid signals in
/// dialog-workspaces.tsx): the fetched list, the armed two-press
/// delete, and the in-flight removal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorktreeDialogState {
    /// `projectID` the list belongs to (the fetch key).
    pub project_id: String,
    /// The current checkout's root directory (the "Current" row).
    pub current: Option<String>,
    /// The fetched directories (loading/loaded/failed).
    pub list: crate::store::ListState<WorktreeEntry>,
    /// First ctrl+d on a row: armed, the row renders the confirm
    /// prompt; any move re-arms nothing (`onMove` clears it).
    pub pending_delete: Option<String>,
    /// The remove in flight ("Deleting {directory}").
    pub removing: Option<String>,
}

/// v2's root sort (dialog-workspaces.tsx:277-286): the current
/// checkout's root first; unmanaged roots (`strategy == null`) next,
/// shortest path first; managed roots last. Stable otherwise.
pub fn sort_worktree_roots(roots: &mut [WorktreeEntry], current: Option<&str>) {
    roots.sort_by(|a, b| {
        let is_current = |root: &WorktreeEntry| Some(root.directory.as_str()) == current;
        is_current(b).cmp(&is_current(a)).then_with(|| {
            match (a.strategy.is_none(), b.strategy.is_none()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                // Unmanaged pair: shorter directory first.
                (true, true) => a.directory.len().cmp(&b.directory.len()),
                (false, false) => std::cmp::Ordering::Equal,
            }
        })
    });
}

/// `abbreviateHome` (runtime.ts): a leading home path becomes `~`.
#[must_use]
pub fn abbreviate_home(directory: &str, home: &str) -> String {
    if !home.is_empty() && directory.starts_with(home) {
        format!("~{}", &directory[home.len()..])
    } else {
        directory.to_owned()
    }
}

/// v2's `contains` (dialog-workspaces.tsx:381-386): `root` holds
/// `directory` (equal, or a proper non-`..` relative beneath it).
#[must_use]
pub fn contains(root: &str, directory: &str) -> bool {
    if root == directory {
        return true;
    }
    match directory.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')) {
        Some(rest) => !rest.is_empty() && !rest.starts_with(".."),
        None => false,
    }
}

/// The dialog's select props from the worktree state (the options
/// memo, dialog-workspaces.tsx:289-335): abbreviated titles,
/// "Current"/"Other" categories, the armed-delete row swapped for
/// the confirm prompt, the in-flight removal message, and the footer
/// action set. Loading/error/empty render v2's status rows.
#[must_use]
pub fn worktree_props(state: &WorktreeDialogState, session_route: bool, home: &str) -> SelectProps {
    let mut actions = Vec::new();
    if session_route {
        actions.push(SelectAction::new("dialog.move_session.move", "move", "ctrl+m"));
    }
    actions.push(SelectAction::new("dialog.move_session.new", "new", "ctrl+a"));
    actions.push(SelectAction::new("dialog.move_session.delete", "delete", "ctrl+d"));
    actions.push(SelectAction::new("dialog.move_session.refresh", "refresh", "ctrl+r"));

    let options = match &state.list {
        crate::store::ListState::Loading => {
            vec![status_row("Loading worktrees…")]
        }
        crate::store::ListState::Failed(error) => vec![
            status_row("Could not load worktrees"),
            SelectOption::titled(error.clone(), "error")
                .described("Close and reopen Worktrees to try again."),
        ],
        crate::store::ListState::Loaded(entries) if entries.is_empty() => Vec::new(),
        crate::store::ListState::Loaded(entries) => {
            let mut roots = entries.clone();
            sort_worktree_roots(&mut roots, state.current.as_deref());
            let current_root = state.current.as_ref().and_then(|directory| {
                roots
                    .iter()
                    .filter(|root| contains(&root.directory, directory))
                    .max_by_key(|root| root.directory.len())
                    .map(|root| root.directory.clone())
            });
            roots
                .into_iter()
                .map(|root| {
                    let deleting = state.pending_delete.as_deref() == Some(root.directory.as_str());
                    let removing = state.removing.as_deref() == Some(root.directory.as_str());
                    let title = if removing {
                        format!("Deleting {}", root.directory)
                    } else if deleting {
                        // The destructive confirm prompt (v2 swaps the
                        // row's title view; the port carries it as the
                        // title).
                        "Press ctrl+d again to confirm".to_owned()
                    } else {
                        abbreviate_home(&root.directory, home)
                    };
                    SelectOption::titled(title, root.directory.clone())
                        .categorized(if Some(&root.directory) == current_root.as_ref() {
                            "Current"
                        } else {
                            "Other"
                        })
                        .with_search(root.directory.clone())
                })
                .collect()
        }
    };

    SelectProps::new("Worktrees", options)
        .with_actions(actions)
        .flat(true)
        .with_empty_texts("No worktrees available", "No worktrees found")
        .with_current(state.current.clone())
}

/// One muted status row (loading) — the placeholder surface.
fn status_row(text: &str) -> SelectOption {
    SelectOption::titled(text, "status")
}

/// The name dialog's confirm: v2 `slugify` (dialog-worktree-name.tsx:
/// 76-82) — trim, lowercase, non-alphanumerics collapse to `-`, edge
/// dashes stripped. An empty result falls back to a generated slug
/// at the CALLER (the dialog's `confirm()`); the port's caller
/// passes the fallback explicitly.
#[must_use]
pub fn slugify(input: &str) -> String {
    let lower = input.trim().to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut dash = false;
    for ch in lower.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// v2 `Slug.create()` (util/slug.ts): a 10-char lowercase base36
/// slug. The port derives it from a seed counter (deterministic
/// tests); production randomness is a caller concern.
#[must_use]
pub fn generate_slug(seed: u64) -> String {
    let mut value = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut out = String::with_capacity(10);
    while out.len() < 10 {
        let digit = (value % 36) as u32;
        out.push(if digit < 10 {
            char::from(b'0' + digit as u8)
        } else {
            char::from(b'a' + digit as u8 - 10)
        });
        value = value / 36 + 0x1234_5678;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(directory: &str, strategy: Option<&str>) -> WorktreeEntry {
        WorktreeEntry { directory: directory.into(), strategy: strategy.map(String::from) }
    }

    #[test]
    fn roots_sort_current_first_then_unmanaged_by_length() {
        let mut roots = vec![
            entry("/src/.worktrees/feat", Some("manual")),
            entry("/home/u/project", None),
            entry("/home/u/other", None),
            entry("/src/.worktrees/fix", Some("git")),
        ];
        sort_worktree_roots(&mut roots, Some("/home/u/project"));
        let dirs: Vec<&str> = roots.iter().map(|root| root.directory.as_str()).collect();
        assert_eq!(
            dirs,
            vec!["/home/u/project", "/home/u/other", "/src/.worktrees/feat", "/src/.worktrees/fix"],
            "current first, unmanaged by path length, managed last (stable)"
        );
    }

    #[test]
    fn contains_matches_equal_and_proper_children_only() {
        assert!(contains("/a/b", "/a/b"));
        assert!(contains("/a/b", "/a/b/c"));
        assert!(!contains("/a/b", "/a/bc"));
        assert!(!contains("/a/b", "/a/b/../escape"));
    }

    #[test]
    fn abbreviate_home_swaps_the_home_prefix() {
        assert_eq!(abbreviate_home("/home/u/project", "/home/u"), "~/project");
        assert_eq!(abbreviate_home("/tmp/project", "/home/u"), "/tmp/project");
    }

    #[test]
    fn props_shape_matches_the_reference_dialog() {
        let state = WorktreeDialogState {
            project_id: "p1".into(),
            current: Some("/home/u/project".into()),
            list: crate::store::ListState::Loaded(vec![
                entry("/home/u/project", None),
                entry("/home/u/.worktrees/feat-a", Some("manual")),
            ]),
            pending_delete: None,
            removing: None,
        };
        let props = worktree_props(&state, true, "/home/u");
        assert_eq!(props.title, "Worktrees");
        assert_eq!(props.options[0].title, "~/project");
        assert_eq!(props.options[0].category.as_deref(), Some("Current"));
        assert_eq!(props.options[1].category.as_deref(), Some("Other"));
        assert_eq!(props.current.as_deref(), Some("/home/u/project"));
        let commands: Vec<&str> = props.actions.iter().map(|a| a.command).collect();
        assert_eq!(
            commands,
            vec![
                "dialog.move_session.move",
                "dialog.move_session.new",
                "dialog.move_session.delete",
                "dialog.move_session.refresh"
            ]
        );
        // Off a session route the move action stays hidden (v2 gates
        // it on `route.data.type === "session"`).
        let props = worktree_props(&state, false, "/home/u");
        assert_eq!(props.actions[0].command, "dialog.move_session.new");

        // The armed delete swaps the row for the confirm prompt.
        let armed = WorktreeDialogState {
            pending_delete: Some("/home/u/.worktrees/feat-a".into()),
            ..state
        };
        let props = worktree_props(&armed, false, "/home/u");
        assert_eq!(props.options[1].title, "Press ctrl+d again to confirm");
        // The in-flight removal renders the deleting message.
        let removing =
            WorktreeDialogState { removing: Some("/home/u/.worktrees/feat-a".into()), ..armed };
        let props = worktree_props(&removing, false, "/home/u");
        assert_eq!(props.options[1].title, "Deleting /home/u/.worktrees/feat-a");
    }

    #[test]
    fn props_status_rows_match_v2_copy() {
        let loading =
            WorktreeDialogState { list: crate::store::ListState::Loading, ..Default::default() };
        assert_eq!(worktree_props(&loading, false, "").options[0].title, "Loading worktrees…");

        let failed = WorktreeDialogState {
            list: crate::store::ListState::Failed("500".into()),
            ..Default::default()
        };
        let props = worktree_props(&failed, false, "");
        assert_eq!(props.options[0].title, "Could not load worktrees");
        assert_eq!(props.options[1].title, "500");
        assert_eq!(
            props.options[1].description.as_deref(),
            Some("Close and reopen Worktrees to try again.")
        );

        let empty = WorktreeDialogState {
            list: crate::store::ListState::Loaded(Vec::new()),
            ..Default::default()
        };
        let props = worktree_props(&empty, false, "");
        assert_eq!(props.options.len(), 0);
        assert_eq!(props.empty_text.as_deref(), Some("No worktrees available"));
        assert_eq!(props.no_match_text.as_deref(), Some("No worktrees found"));
    }

    #[test]
    fn slugify_matches_the_reference_pipeline() {
        assert_eq!(slugify("  My Fix--Two  "), "my-fix-two");
        assert_eq!(slugify("Feat/A_B"), "feat-a-b");
        assert_eq!(slugify("--trailing--"), "trailing");
        assert_eq!(slugify("///"), "");
    }

    #[test]
    fn generated_slugs_are_ten_char_base36() {
        let slug = generate_slug(7);
        assert_eq!(slug.len(), 10);
        assert!(slug.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit()));
        assert_ne!(generate_slug(7), generate_slug(8));
    }

    // -- store-level interaction (R-05: open → two-press delete →
    //    forced confirm → create → move) ------------------------------

    use crate::components::dialog::execute_submission;
    use crate::components::dialog::handle_dialog_key;
    use crate::keymap::{KeyStroke, Keymap, ManualClock};
    use crate::reactive::Runtime;
    use crate::store::{DeleteStep, DialogKind, RecordingWorktreeSink, Store, WorktreeCall};

    fn worktree_store() -> (Store, std::rc::Rc<RecordingWorktreeSink>) {
        let store = Store::new(Runtime::new());
        let sink = std::rc::Rc::new(RecordingWorktreeSink::default());
        store.set_worktree_sink(sink.clone());
        store.set_worktree_project("p1", Some("/home/u/project"));
        store.open_worktrees_dialog();
        // The open resets the list to loading + fires the fetch; the
        // result lands (production: the worker event; tests: direct).
        store.set_worktree_list(crate::store::ListState::Loaded(vec![
            entry("/home/u/project", None),
            entry("/home/u/.worktrees/feat-a", Some("manual")),
        ]));
        (store, sink)
    }

    fn dialog_keymap() -> Keymap {
        let mut keymap = Keymap::new();
        keymap.push_mode("dialog");
        keymap
    }

    fn press(
        store: &Store,
        keymap: &mut Keymap,
        spec: &str,
    ) -> crate::components::dialog::DialogKeyOutcome {
        handle_dialog_key(
            store,
            keymap,
            &ManualClock::default(),
            KeyStroke::parse(spec).expect("stroke parses"),
            (80, 24),
        )
    }

    #[test]
    fn open_fetches_and_renders_the_dialog_stack() {
        let (store, sink) = worktree_store();
        assert!(
            matches!(store.top_dialog().map(|entry| entry.kind), Some(DialogKind::Worktrees)),
            "the Worktrees dialog opens"
        );
        assert_eq!(
            store.top_dialog().map(|entry| entry.size.width()),
            Some(116),
            "v2 setSize(\"xlarge\")"
        );
        assert_eq!(
            sink.calls.borrow().as_slice(),
            &[WorktreeCall::List("p1".to_owned())],
            "open fires worktree.list"
        );
    }

    #[test]
    fn delete_is_two_press_and_fires_remove_then_force_confirm() {
        let (store, sink) = worktree_store();
        // Focus the managed row (the second option after the sort puts
        // the current root first).
        store.update_dialog(|top| top.select.selected = 1);
        let mut keymap = dialog_keymap();

        // First ctrl+d: armed, no remove yet (only the open's list
        // call is recorded).
        press(&store, &mut keymap, "ctrl+d");
        assert_eq!(
            store.worktree_dialog().pending_delete.as_deref(),
            Some("/home/u/.worktrees/feat-a")
        );
        assert!(
            sink.calls.borrow().iter().all(|call| matches!(call, WorktreeCall::List(_))),
            "armed only"
        );

        // Second ctrl+d: the remove fires with force:false.
        press(&store, &mut keymap, "ctrl+d");
        assert!(sink.calls.borrow().iter().any(|call| matches!(
            call,
            WorktreeCall::Remove {
                project_id, directory, force: false
            } if project_id == "p1" && directory == "/home/u/.worktrees/feat-a"
        )));
        assert_eq!(store.worktree_dialog().removing.as_deref(), Some("/home/u/.worktrees/feat-a"));

        // The server rejects with forceRequired → the confirm dialog.
        store.apply_worktree_remove_failed("/home/u/.worktrees/feat-a", "has changes", true);
        assert!(matches!(
            store.top_dialog().map(|entry| entry.kind),
            Some(DialogKind::WorktreeDeleteConfirm { .. })
        ));
        // "Yes" re-fires with force:true.
        store.update_dialog(|top| top.select.selected = 0);
        let outcome = press(&store, &mut keymap, "return");
        let crate::components::dialog::DialogKeyOutcome::Submitted(value) = outcome else {
            panic!("confirm submits a choice, got {outcome:?}");
        };
        let mut dispatcher = crate::components::command_palette::StoreDispatcher;
        let dispatch = execute_submission(&store, &mut dispatcher, &value);
        assert_eq!(dispatch, crate::components::command_palette::Dispatch::Handled);
        assert!(
            sink.calls.borrow().iter().any(|call| matches!(
                call,
                WorktreeCall::Remove { force: true, directory, .. }
                    if directory == "/home/u/.worktrees/feat-a"
            )),
            "forced remove fired"
        );
        // The success path drops the row.
        store.apply_worktree_removed("/home/u/.worktrees/feat-a");
        let state = store.worktree_dialog();
        let rows = state.list.rows().expect("loaded");
        assert!(rows.iter().all(|row| row.directory != "/home/u/.worktrees/feat-a"));
    }

    #[test]
    fn unmanaged_roots_are_not_deletable() {
        let (store, sink) = worktree_store();
        store.open_worktrees_dialog();
        store.update_dialog(|top| top.select.selected = 0);
        assert_eq!(
            store.worktree_delete_pressed("/home/u/project"),
            DeleteStep::Invalid,
            "the unmanaged current root stays"
        );
        let mut keymap = dialog_keymap();
        press(&store, &mut keymap, "ctrl+d");
        assert!(
            sink.calls.borrow().iter().all(|call| matches!(call, WorktreeCall::List(_))),
            "no remove fired for the unmanaged root"
        );
        assert_eq!(store.worktree_dialog().pending_delete, None);
    }

    #[test]
    fn new_opens_the_name_dialog_and_creates_a_slug() {
        let (store, sink) = worktree_store();
        let mut keymap = dialog_keymap();
        press(&store, &mut keymap, "ctrl+a");
        assert!(
            matches!(store.top_dialog().map(|entry| entry.kind), Some(DialogKind::WorktreeName)),
            "ctrl+a replaces with the name dialog"
        );
        // Type + tab-generate + enter submit.
        for key in ["f", "e", "a", "t"] {
            press(&store, &mut keymap, key);
        }
        let generated = press(&store, &mut keymap, "tab");
        assert_eq!(
            generated,
            crate::components::dialog::DialogKeyOutcome::Consumed,
            "tab generates a slug"
        );
        let value = store.top_dialog().map(|entry| entry.value.clone()).unwrap_or_default();
        assert_eq!(value.len(), 10, "the generated slug fills the input");
        let outcome = press(&store, &mut keymap, "return");
        let crate::components::dialog::DialogKeyOutcome::PromptSubmitted(value) = outcome else {
            panic!("enter submits the name, got {outcome:?}");
        };
        store.worktree_create_from_name(&value, 7);
        assert!(
            sink.calls.borrow().iter().any(|call| matches!(
                call,
                WorktreeCall::Create { name, project_id } if *name == value && *project_id == "p1"
            )),
            "create fired with the slug name"
        );
        // A typed name slugifies on the way through.
        store.worktree_create_from_name("My Fix--Two", 7);
        assert!(sink.calls.borrow().iter().any(|call| matches!(
            call,
            WorktreeCall::Create { name, .. } if *name == "my-fix-two"
        )));
        // The created directory joins the list + navigates home.
        store.apply_worktree_created("/home/u/.worktrees/my-fix-two");
        assert_eq!(store.dialogs().len(), 0, "dialogs cleared");
        assert!(matches!(store.route(), crate::store::Route::Home), "navigated home");
    }

    #[test]
    fn move_posts_session_move_on_session_routes_only() {
        let (store, sink) = worktree_store();
        store.open_tab("s1", None);
        store.navigate(crate::store::Route::Session { session_id: "s1".to_owned() });
        store.update_dialog(|top| top.select.selected = 1);
        let mut keymap = dialog_keymap();
        press(&store, &mut keymap, "ctrl+m");
        assert!(
            sink.calls.borrow().iter().any(|call| matches!(
                call,
                WorktreeCall::MoveSession { session_id, directory }
                    if session_id == "s1" && directory == "/home/u/.worktrees/feat-a"
            )),
            "ctrl+m fires session.move"
        );
        assert_eq!(store.dialogs().len(), 0);
    }

    #[test]
    fn refresh_fires_and_row_submit_navigates_home() {
        let (store, sink) = worktree_store();
        let mut keymap = dialog_keymap();
        press(&store, &mut keymap, "ctrl+r");
        assert!(sink
            .calls
            .borrow()
            .iter()
            .any(|call| matches!(call, WorktreeCall::Refresh(project) if project == "p1")));
        // Submitting a row closes + navigates home.
        store.update_dialog(|top| top.select.selected = 1);
        let outcome = press(&store, &mut keymap, "return");
        assert_eq!(
            outcome,
            crate::components::dialog::DialogKeyOutcome::Submitted(
                "/home/u/.worktrees/feat-a".to_owned()
            )
        );
        let mut dispatcher = crate::components::command_palette::StoreDispatcher;
        let dispatch = execute_submission(&store, &mut dispatcher, "/home/u/.worktrees/feat-a");
        assert_eq!(dispatch, crate::components::command_palette::Dispatch::Handled);
        assert!(matches!(store.route(), crate::store::Route::Home));
    }
}

/// Golden-frame scenarios (R-05): the resting Worktrees dialog and
/// the name dialog, recorded through the sanctioned
/// `PARITY_UPDATE=1` path (the same contract as
/// `screen_golden_tests.rs` — write-once fixtures, text-grid AND
/// canonical-ANSI byte compare afterwards).
#[cfg(test)]
mod golden_tests {
    use super::WorktreeDialogState;
    use crate::components::dialog::render_overlay_with;
    use crate::store::{DialogEntry, DialogKind, DialogSize};
    use crate::theme::ThemeState;

    fn goldens() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/parity/goldens/tui-frames")
    }

    fn update_mode() -> bool {
        std::env::var("PARITY_UPDATE").is_ok_and(|value| value == "1")
    }

    /// The dialog fixture: current root (unmanaged) + one managed
    /// worktree, session route (all four actions), home `/home/u`.
    fn fixture_state() -> WorktreeDialogState {
        WorktreeDialogState {
            project_id: "p1".into(),
            current: Some("/home/u/project".into()),
            list: crate::store::ListState::Loaded(vec![
                super::WorktreeEntry { directory: "/home/u/project".into(), strategy: None },
                super::WorktreeEntry {
                    directory: "/home/u/.worktrees/feat-a".into(),
                    strategy: Some("manual".into()),
                },
            ]),
            pending_delete: None,
            removing: None,
        }
    }

    fn empty_base(width: u16, height: u16, theme: &ThemeState) -> crate::render::Frame {
        let base_bg = theme.resolved.tokens.background.base;
        crate::render::Frame {
            cells: vec![
                vec![
                    crate::render::Cell {
                        ch: ' ',
                        attrs: 0,
                        fg: crate::render::CellColor::Rgb(255, 255, 255),
                        bg: crate::render::CellColor::Rgb(base_bg.r, base_bg.g, base_bg.b),
                    };
                    usize::from(width)
                ];
                usize::from(height)
            ],
            width: usize::from(width),
            height: usize::from(height),
        }
    }

    fn worktrees_frame(width: u16, height: u16) -> crate::render::Frame {
        let theme = ThemeState::v2_dark();
        let state = fixture_state();
        let props = super::worktree_props(&state, true, "/home/u");
        let mut entry = DialogEntry {
            kind: DialogKind::Worktrees,
            size: DialogSize::Xlarge,
            select: crate::components::dialog_select::SelectState::default(),
            value: String::new(),
            theme_initial: None,
        };
        entry.select.focus_current(&props);
        let mut base = empty_base(width, height, &theme);
        render_overlay_with(&mut base, &entry, &props, None, &theme, (width, height));
        base
    }

    fn name_frame(width: u16, height: u16) -> crate::render::Frame {
        let theme = ThemeState::v2_dark();
        let entry = DialogEntry {
            kind: DialogKind::WorktreeName,
            size: DialogSize::Medium,
            select: crate::components::dialog_select::SelectState::default(),
            value: "feat".to_owned(),
            theme_initial: None,
        };
        let props = crate::components::dialog_select::SelectProps::new("", Vec::new());
        let mut base = empty_base(width, height, &theme);
        render_overlay_with(&mut base, &entry, &props, None, &theme, (width, height));
        base
    }

    fn assert_text_grid_match(id: &str, width: usize, frame: &crate::render::Frame) {
        let path = goldens().join(format!("{id}.w{width}.txt"));
        if update_mode() || !path.is_file() {
            std::fs::write(&path, frame.text_grid().join("\n") + "\n").expect("write golden txt");
            return;
        }
        let text = std::fs::read_to_string(&path).expect("golden txt");
        let rows: Vec<String> = text.trim_end_matches('\n').split('\n').map(String::from).collect();
        let rendered = parity::tui_frame::TextGrid {
            rows: frame.text_grid(),
            height: frame.height,
            width: frame.width,
        };
        let diff = parity::tui_frame::compare_text_grids(
            &parity::tui_frame::TextGrid { rows, height: frame.height, width: frame.width },
            &rendered,
        );
        assert!(
            diff.first_divergence.is_none(),
            "{id}@{width} text grid diverged: {:?} ({:.2}% matching)\nfirst divergence row:\n  golden: {:?}\n  actual: {:?}",
            diff.first_divergence,
            diff.match_pct(),
            {
                let cell = diff.first_divergence.as_ref().map_or(0, |cell| cell.row);
                parity::tui_frame::TextGrid {
                    rows: std::fs::read_to_string(&path)
                        .expect("golden")
                        .trim_end_matches('\n')
                        .split('\n')
                        .map(String::from)
                        .collect::<Vec<_>>(),
                    height: frame.height,
                    width: frame.width,
                }
                .rows
                .get(cell)
                .cloned()
            },
            rendered.rows.get(diff.first_divergence.as_ref().map_or(0, |cell| cell.row)).cloned(),
        );
    }

    fn assert_ansi_match(id: &str, width: usize, frame: &crate::render::Frame) {
        let path = goldens().join(format!("{id}.w{width}.ansi"));
        if update_mode() || !path.is_file() {
            std::fs::write(&path, crate::render::serialize_canonical(frame))
                .expect("write golden ansi");
            return;
        }
        let golden = std::fs::read_to_string(path).expect("golden ansi");
        let diff = parity::tui_frame::compare_ansi_bytes_at(
            &golden,
            &crate::render::serialize_canonical(frame),
            parity::tui_frame::ComparisonDepth::TruecolorCanonical,
        )
        .expect("ansi parses");
        assert!(
            diff.equal,
            "{id}@{width} canonical ansi differs: first divergence {:?}",
            diff.first_divergent_offset
        );
    }

    #[test]
    fn worktrees_dialog_matches_golden_at_80() {
        let frame = worktrees_frame(80, 24);
        let grid = frame.text_grid();
        assert!(grid.iter().any(|row| row.contains("Worktrees")), "title present");
        assert!(grid.iter().any(|row| row.contains("~/project")), "current root abbreviated");
        assert!(
            grid.iter().any(|row| row.contains("~/.worktrees/feat-a")),
            "managed worktree listed"
        );
        assert!(
            grid.iter().any(|row| row.contains("move ctrl+m")),
            "footer actions (session route)"
        );
        assert_text_grid_match("worktrees-dialog", 80, &frame);
        assert_ansi_match("worktrees-dialog", 80, &frame);
    }

    #[test]
    fn worktrees_dialog_matches_golden_at_160() {
        let frame = worktrees_frame(160, 24);
        assert_text_grid_match("worktrees-dialog", 160, &frame);
        assert_ansi_match("worktrees-dialog", 160, &frame);
    }

    #[test]
    fn worktree_name_dialog_matches_golden_at_80() {
        let frame = name_frame(80, 24);
        let grid = frame.text_grid();
        assert!(grid.iter().any(|row| row.contains("Name worktree")), "title");
        assert!(grid.iter().any(|row| row.contains("feat")), "typed value");
        assert!(grid.iter().any(|row| row.contains("generate one")), "footer hint");
        assert_text_grid_match("worktree-name-dialog", 80, &frame);
        assert_ansi_match("worktree-name-dialog", 80, &frame);
    }
}
