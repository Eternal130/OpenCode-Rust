//! The v1→v2 cli-config migration — port of
//! `packages/cli/src/config/migrate.ts` (`migrateV1`, `migrate.ts:160-258`)
//! plus the in-place legacy fixes for an existing v2-shape document
//! (`terminal.copy_on_select`, v1 keybind names — `migrate.ts:44-97`).
//!
//! Both halves operate at the VALUE level (parse, transform, write back):
//! v2 performs the in-place fixes on raw JSONC text to preserve comments
//! through the one-time rewrite; the port rewrites the document once,
//! pretty-printed (registered divergence — see the module docs in
//! [`super`]).
//!
//! Sources:
//! * `<config>/tui.json` — the v1 TUI config (`@opencode/tui/config/v1`);
//! * `<state>/kv.json` — the v1 key/value store (theme mode lock, diff
//!   viewer prefs, toggles...).

use serde_json::{json, Map, Value};

/// `TuiKeybind.CommandMap` (`config/v1/keybind.ts:251-414`): v1 keybind
/// name → v2 target name.
pub const COMMAND_MAP: &[(&str, &str)] = &[
    ("app_exit", "app.exit"),
    ("app_debug", "app.debug"),
    ("app_console", "app.console"),
    ("app_heap_snapshot", "app.heap_snapshot"),
    ("app_toggle_animations", "app.toggle.animations"),
    ("app_toggle_file_context", "app.toggle.file_context"),
    ("app_toggle_diffwrap", "app.toggle.diffwrap"),
    ("app_toggle_paste_summary", "app.toggle.paste_summary"),
    ("command_list", "command.palette.show"),
    ("help_show", "help.show"),
    ("docs_open", "docs.open"),
    ("diff_open", "diff.open"),
    ("diff_close", "diff.close"),
    ("diff_toggle", "diff.toggle"),
    ("diff_expand", "diff.expand"),
    ("diff_expand_all", "diff.expand_all"),
    ("diff_collapse", "diff.collapse"),
    ("diff_switch_focus", "diff.switch_focus"),
    ("diff_next_hunk", "diff.next_hunk"),
    ("diff_previous_hunk", "diff.previous_hunk"),
    ("diff_next_file", "diff.next_file"),
    ("diff_previous_file", "diff.previous_file"),
    ("diff_toggle_file_tree", "diff.toggle_file_tree"),
    ("diff_single_patch", "diff.single_patch"),
    ("diff_switch_source", "diff.switch_source"),
    ("diff_toggle_view", "diff.toggle_view"),
    ("diff_help", "diff.help"),
    ("editor_open", "prompt.editor"),
    ("theme_list", "theme.switch"),
    ("theme_switch_mode", "theme.switch_mode"),
    ("theme_mode_lock", "theme.mode.lock"),
    ("sidebar_toggle", "session.sidebar.toggle"),
    ("scrollbar_toggle", "session.toggle.scrollbar"),
    ("status_view", "opencode.status"),
    ("debug_view", "opencode.debug"),
    ("session_export", "session.export"),
    ("session_copy", "session.copy"),
    ("session_move", "session.move"),
    ("session_new", "session.new"),
    ("session_list", "session.list"),
    ("open_menu", "open.menu"),
    ("session_tab_next", "session.tab.next"),
    ("session_tab_previous", "session.tab.previous"),
    ("session_tab_next_unread", "session.tab.next_unread"),
    ("session_tab_previous_unread", "session.tab.previous_unread"),
    ("session_tab_close", "session.tab.close"),
    ("session_tab_reopen", "session.tab.reopen"),
    ("session_timeline", "session.timeline"),
    ("session_fork", "session.fork"),
    ("session_rename", "session.rename"),
    ("session_delete", "session.delete"),
    ("session_share", "session.share"),
    ("session_unshare", "session.unshare"),
    ("session_interrupt", "session.interrupt"),
    ("session_background", "session.background"),
    ("session_compact", "session.compact"),
    ("session_queued_prompts", "session.queued_prompts"),
    ("queued_prompt_delete", "queued_prompt.delete"),
    ("session_child_first", "session.child.first"),
    ("session_parent", "session.parent"),
    ("session_pin_toggle", "session.pin.toggle"),
    ("session_quick_switch_1", "session.quick_switch.1"),
    ("session_quick_switch_2", "session.quick_switch.2"),
    ("session_quick_switch_3", "session.quick_switch.3"),
    ("session_quick_switch_4", "session.quick_switch.4"),
    ("session_quick_switch_5", "session.quick_switch.5"),
    ("session_quick_switch_6", "session.quick_switch.6"),
    ("session_quick_switch_7", "session.quick_switch.7"),
    ("session_quick_switch_8", "session.quick_switch.8"),
    ("session_quick_switch_9", "session.quick_switch.9"),
    ("session_tab_select_1", "session.tab.select.1"),
    ("session_tab_select_2", "session.tab.select.2"),
    ("session_tab_select_3", "session.tab.select.3"),
    ("session_tab_select_4", "session.tab.select.4"),
    ("session_tab_select_5", "session.tab.select.5"),
    ("session_tab_select_6", "session.tab.select.6"),
    ("session_tab_select_7", "session.tab.select.7"),
    ("session_tab_select_8", "session.tab.select.8"),
    ("session_tab_select_9", "session.tab.select.9"),
    ("session_tab_select_10", "session.tab.select.10"),
    ("stash_delete", "stash.delete"),
    ("model_provider_list", "model.dialog.provider"),
    ("model_favorite_toggle", "model.dialog.favorite"),
    ("model_list", "model.list"),
    ("model_cycle_recent", "model.cycle_recent"),
    ("model_cycle_recent_reverse", "model.cycle_recent_reverse"),
    ("model_cycle_favorite", "model.cycle_favorite"),
    ("model_cycle_favorite_reverse", "model.cycle_favorite_reverse"),
    ("mcp_list", "mcp.list"),
    ("provider_connect", "provider.connect"),
    ("agent_list", "agent.list"),
    ("agent_cycle", "agent.cycle"),
    ("agent_cycle_reverse", "agent.cycle.reverse"),
    ("variant_cycle", "variant.cycle"),
    ("variant_list", "variant.list"),
    ("messages_page_up", "session.page.up"),
    ("messages_page_down", "session.page.down"),
    ("messages_line_up", "session.line.up"),
    ("messages_line_down", "session.line.down"),
    ("messages_half_page_up", "session.half.page.up"),
    ("messages_half_page_down", "session.half.page.down"),
    ("messages_first", "session.first"),
    ("messages_last", "session.last"),
    ("messages_next", "session.message.next"),
    ("messages_previous", "session.message.previous"),
    ("messages_next_user", "session.message.user.next"),
    ("messages_previous_user", "session.message.user.previous"),
    ("messages_last_user", "session.messages_last_user"),
    ("messages_copy", "messages.copy"),
    ("messages_undo", "session.undo"),
    ("messages_redo", "session.redo"),
    ("display_thinking", "session.toggle.thinking"),
    ("prompt_submit", "prompt.submit"),
    ("prompt_queue", "prompt.queue"),
    ("prompt_editor_context_clear", "prompt.editor_context.clear"),
    ("prompt_images_view", "prompt.images.view"),
    ("prompt_skills", "prompt.skills"),
    ("prompt_stash", "prompt.stash"),
    ("prompt_stash_pop", "prompt.stash.pop"),
    ("prompt_stash_list", "prompt.stash.list"),
    ("input_clear", "prompt.clear"),
    ("input_paste", "prompt.paste"),
    ("input_submit", "input.submit"),
    ("input_newline", "input.newline"),
    ("input_move_left", "input.move.left"),
    ("input_move_right", "input.move.right"),
    ("input_move_up", "input.move.up"),
    ("input_move_down", "input.move.down"),
    ("input_select_left", "input.select.left"),
    ("input_select_right", "input.select.right"),
    ("input_select_up", "input.select.up"),
    ("input_select_down", "input.select.down"),
    ("input_line_home", "input.line.home"),
    ("input_line_end", "input.line.end"),
    ("input_select_line_home", "input.select.line.home"),
    ("input_select_line_end", "input.select.line.end"),
    ("input_visual_line_home", "input.visual.line.home"),
    ("input_visual_line_end", "input.visual.line.end"),
    ("input_select_visual_line_home", "input.select.visual.line.home"),
    ("input_select_visual_line_end", "input.select.visual.line.end"),
    ("input_buffer_home", "input.buffer.home"),
    ("input_buffer_end", "input.buffer.end"),
    ("input_select_buffer_home", "input.select.buffer.home"),
    ("input_select_buffer_end", "input.select.buffer.end"),
    ("input_delete_line", "input.delete.line"),
    ("input_delete_to_line_end", "input.delete.to.line.end"),
    ("input_delete_to_line_start", "input.delete.to.line.start"),
    ("input_backspace", "input.backspace"),
    ("input_delete", "input.delete"),
    ("input_undo", "input.undo"),
    ("input_redo", "input.redo"),
    ("input_word_forward", "input.word.forward"),
    ("input_word_backward", "input.word.backward"),
    ("input_select_word_forward", "input.select.word.forward"),
    ("input_select_word_backward", "input.select.word.backward"),
    ("input_delete_word_forward", "input.delete.word.forward"),
    ("input_delete_word_backward", "input.delete.word.backward"),
    ("input_select_all", "input.select.all"),
    ("history_previous", "prompt.history.previous"),
    ("history_next", "prompt.history.next"),
    ("terminal_suspend", "terminal.suspend"),
    ("terminal_title_toggle", "terminal.title.toggle"),
];
/// `command_map(name)` — the v2 target for a v1 keybind name.
fn command_map(name: &str) -> Option<&'static str> {
    COMMAND_MAP.iter().find(|(from, _)| *from == name).map(|(_, to)| *to)
}

/// Is `name` a valid v2 `keybinds` target?
fn is_v2_keybind_name(name: &str) -> Option<()> {
    V2_KEYBIND_NAMES.contains(&name).then_some(())
}

/// The v2 keybind `Definitions` names (`config/keybind.ts:45+`) —
/// the valid `keybinds` targets after migration.
pub const V2_KEYBIND_NAMES: &[&str] = &[
    "leader",
    "app.exit",
    "app.clear",
    "app.debug",
    "app.console",
    "app.scrap",
    "app.toggle.animations",
    "app.toggle.file_context",
    "app.toggle.diffwrap",
    "app.toggle.paste_summary",
    "command.palette.show",
    "help.show",
    "docs.open",
    "opencode.settings",
    "server.pair",
    "service.restart",
    "location.reload",
    "diff.open",
    "diff.close",
    "diff.down",
    "diff.up",
    "diff.page.down",
    "diff.page.up",
    "diff.half_page.down",
    "diff.half_page.up",
    "diff.first",
    "diff.last",
    "diff.toggle",
    "diff.expand",
    "diff.expand_all",
    "diff.collapse",
    "diff.switch_focus",
    "diff.next_hunk",
    "diff.previous_hunk",
    "diff.next_file",
    "diff.previous_file",
    "diff.toggle_file_tree",
    "diff.single_patch",
    "diff.switch_source",
    "diff.toggle_view",
    "diff.mark_reviewed",
    "diff.help",
    "prompt.editor",
    "theme.switch",
    "theme.switch_mode",
    "theme.mode.lock",
    "session.sidebar.toggle",
    "pane.focus.left",
    "pane.focus.right",
    "terminal.select",
    "terminal.toggle",
    "terminal.close",
    "session.toggle.scrollbar",
    "opencode.status",
    "opencode.debug",
    "session.export",
    "session.copy",
    "session.copy.id",
    "session.move",
    "session.new",
    "session.list",
    "open.menu",
    "session.tab.next",
    "session.tab.previous",
    "session.tab.history.back",
    "session.tab.history.forward",
    "session.tab.next_unread",
    "session.tab.previous_unread",
    "session.tab.close",
    "session.tab.reopen",
    "session.timeline",
    "session.fork",
    "session.rename",
    "session.delete",
    "session.share",
    "session.unshare",
    "session.interrupt",
    "session.background",
    "session.compact",
    "session.aside",
    "session.cd",
    "session.queued_prompts",
    "queued_prompt.undo",
    "queued_prompt.delete",
    "session.toggle.exploration_grouping",
    "session.verbosity.cycle",
    "session.child.first",
    "session.child.next",
    "session.child.previous",
    "session.parent",
    "session.pin.toggle",
    "session.quick_switch.1",
    "session.quick_switch.2",
    "session.quick_switch.3",
    "session.quick_switch.4",
    "session.quick_switch.5",
    "session.quick_switch.6",
    "session.quick_switch.7",
    "session.quick_switch.8",
    "session.quick_switch.9",
    "session.tab.select.1",
    "session.tab.select.2",
    "session.tab.select.3",
    "session.tab.select.4",
    "session.tab.select.5",
    "session.tab.select.6",
    "session.tab.select.7",
    "session.tab.select.8",
    "session.tab.select.9",
    "session.tab.select.10",
    "stash.delete",
    "model.dialog.provider",
    "model.dialog.favorite",
    "model.list",
    "model.cycle_recent",
    "model.cycle_recent_reverse",
    "model.cycle_favorite",
    "model.cycle_favorite_reverse",
    "mcp.list",
    "provider.connect",
    "agent.list",
    "agent.cycle",
    "agent.cycle.reverse",
    "variant.cycle",
    "variant.list",
    "session.page.up",
    "session.page.down",
    "session.line.up",
    "session.line.down",
    "session.half.page.up",
    "session.half.page.down",
    "session.first",
    "session.last",
    "session.message.next",
    "session.message.previous",
    "session.message.user.next",
    "session.message.user.previous",
    "session.messages_last_user",
    "messages.copy",
    "session.undo",
    "session.redo",
    "session.toggle.thinking",
    "prompt.submit",
    "prompt.queue",
    "prompt.editor_context.clear",
    "prompt.images.view",
    "prompt.skills",
    "prompt.stash",
    "prompt.stash.pop",
    "prompt.stash.list",
    "prompt.clear",
    "prompt.paste",
    "input.submit",
    "input.newline",
    "input.move.left",
    "input.move.right",
    "input.move.up",
    "input.move.down",
    "input.select.left",
    "input.select.right",
    "input.select.up",
    "input.select.down",
    "input.line.home",
    "input.line.end",
    "input.select.line.home",
    "input.select.line.end",
    "input.visual.line.home",
    "input.visual.line.end",
    "input.select.visual.line.home",
    "input.select.visual.line.end",
    "input.buffer.home",
    "input.buffer.end",
    "input.select.buffer.home",
    "input.select.buffer.end",
    "input.delete.line",
    "input.delete.to.line.end",
    "input.delete.to.line.start",
    "input.backspace",
    "input.delete",
    "input.undo",
    "input.redo",
    "input.word.forward",
    "input.word.backward",
    "input.select.word.forward",
    "input.select.word.backward",
    "input.delete.word.forward",
    "input.delete.word.backward",
    "input.select.all",
    "prompt.history.previous",
    "prompt.history.next",
    "composer.subagent.up",
    "composer.subagent.down",
    "composer.subagent.select",
    "composer.subagent.interrupt",
    "composer.shell.up",
    "composer.shell.down",
    "composer.shell.select",
    "composer.shell.kill",
    "composer.terminal.up",
    "composer.terminal.down",
    "composer.terminal.select",
    "dialog.select.prev",
    "dialog.select.next",
    "dialog.select.page_up",
    "dialog.select.page_down",
    "dialog.select.home",
    "dialog.select.end",
    "dialog.select.submit",
    "dialog.prompt.submit",
    "dialog.integration.rename",
    "dialog.integration.delete",
    "dialog.worktree.generate",
    "dialog.move_session.new",
    "dialog.move_session.move",
    "dialog.move_session.delete",
    "dialog.move_session.refresh",
    "prompt.autocomplete.prev",
    "prompt.autocomplete.next",
    "prompt.autocomplete.hide",
    "prompt.autocomplete.select",
    "prompt.autocomplete.complete",
    "permission.prompt.fullscreen",
    "plugins.toggle",
    "dialog.mcp.toggle",
    "dialog.plugins.error",
    "dialog.plugins.install",
    "dialog.plugins.update",
    "dialog.plugins.check",
    "terminal.suspend",
    "terminal.title.toggle",
    "plugins.list",
    "plugins.install",
];
/// The in-place fixes for an existing cli.json (`migrate.ts:37-98`):
///
/// * `terminal.copy_on_select: bool` → `terminal.copy:
///   "select"|"manual"` when `copy` is absent, then the legacy key
///   drops (`migrate.ts:44-61`);
/// * v1 keybind names inside `keybinds` rename to their v2 targets;
///   entries whose target is not a v2 definition, or whose rename
///   collides with an existing target, drop (`migrate.ts:62-92`; the
///   parser's last-duplicate-wins maps to last-entry-wins here).
///
/// Returns the rewritten document when anything changed, `None` when
/// the document is already canonical (nothing is written).
#[must_use]
pub fn fix_legacy_document(object: &Map<String, Value>) -> Option<Value> {
    let mut document = Value::Object(object.clone());
    let mut changed = false;

    // terminal.copy_on_select → terminal.copy
    if let Some(terminal) = document.get("terminal").and_then(Value::as_object) {
        let legacy_copy = terminal.get("copy_on_select").and_then(Value::as_bool);
        if let Some(legacy_copy) = legacy_copy {
            let terminal =
                document.get_mut("terminal").and_then(Value::as_object_mut).expect("checked above");
            if !terminal.contains_key("copy") {
                terminal.insert(
                    "copy".to_owned(),
                    json!(if legacy_copy { "select" } else { "manual" }),
                );
            }
            terminal.remove("copy_on_select");
            changed = true;
        }
    }

    // keybind renames
    if let Some(keybinds) = document.get("keybinds").and_then(Value::as_object).cloned() {
        if !keybinds.is_empty() {
            let existing: Vec<String> = keybinds.keys().cloned().collect();
            let original = keybinds.clone();
            let mut renamed = Map::new();
            for (name, value) in keybinds {
                let target = command_map(&name).map(str::to_owned).or_else(|| {
                    // Already-v2 names (and v1 names that ARE their own
                    // target) pass through; anything else drops.
                    is_v2_keybind_name(&name).map(|()| name.clone())
                });
                let Some(target) = target else { continue };
                let remove = is_v2_keybind_name(&target).is_none()
                    || (target != name && existing.contains(&target));
                if remove {
                    continue;
                }
                renamed.insert(target, value);
            }
            if Value::Object(renamed.clone()) != Value::Object(original) {
                document
                    .as_object_mut()
                    .expect("document object")
                    .insert("keybinds".to_owned(), Value::Object(renamed));
                changed = true;
            }
        }
    }

    changed.then_some(document)
}

/// `migrateV1` (`migrate.ts:160-258`): fold the v1 `tui.json` config
/// and the v1 `kv.json` store into a v2 `cli.json` document (without
/// the `$schema`, which the caller prepends). One function per source
/// section would obscure the reference's flat field-by-field walk.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn migrate_v1(legacy: Option<&Value>, kv: Option<&Value>) -> Value {
    let legacy = legacy.and_then(Value::as_object);
    let kv = kv.and_then(Value::as_object).cloned().unwrap_or_default();
    let kv_bool = |key: &str| kv.get(key).and_then(Value::as_bool);
    let kv_value = |key: &str| kv.get(key).cloned().filter(|value| !value.is_null());

    let mut out = Map::new();

    // plugins: directives + enabled map (`migrate.ts:161-166`).
    let mut plugins: Vec<Value> = Vec::new();
    if let Some(plugin) = legacy.and_then(|l| l.get("plugin")).and_then(Value::as_array) {
        for entry in plugin {
            match entry {
                Value::String(name) => plugins.push(json!(name)),
                Value::Array(pair) if pair.len() == 2 => {
                    if let Some(package) = pair[0].as_str() {
                        plugins.push(json!({ "package": package, "options": pair[1] }));
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(enabled) = legacy.and_then(|l| l.get("plugin_enabled")).and_then(Value::as_object) {
        for (id, state) in enabled {
            match state.as_bool() {
                Some(true) => plugins.push(json!(id)),
                Some(false) => plugins.push(json!(format!("-{id}"))),
                None => {}
            }
        }
    }
    if !plugins.is_empty() {
        out.insert("plugins".to_owned(), Value::Array(plugins));
    }

    // theme (kv.theme is the v1 name; kv.theme_mode_lock the mode pin).
    let theme_name = legacy
        .and_then(|l| l.get("theme"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| kv_value("theme").and_then(|v| v.as_str().map(str::to_owned)));
    let theme_mode = kv_value("theme_mode_lock").and_then(|v| v.as_str().map(str::to_owned));
    if theme_name.is_some() || theme_mode.is_some() {
        let mut theme = Map::new();
        if let Some(name) = theme_name {
            theme.insert("name".to_owned(), json!(name));
        }
        if let Some(mode) = theme_mode {
            theme.insert("mode".to_owned(), json!(mode));
        }
        out.insert("theme".to_owned(), Value::Object(theme));
    }

    // keybinds through the CommandMap (`migrate.ts:173-182`).
    if let Some(keybinds) = legacy.and_then(|l| l.get("keybinds")).and_then(Value::as_object) {
        let mut renamed = Map::new();
        for (name, value) in keybinds {
            let target = command_map(name).map_or_else(|| name.clone(), str::to_owned);
            if is_v2_keybind_name(&target).is_some() {
                renamed.insert(target, value.clone());
            }
        }
        if !renamed.is_empty() {
            out.insert("keybinds".to_owned(), Value::Object(renamed));
        }
    }

    if let Some(timeout) = legacy.and_then(|l| l.get("leader_timeout")) {
        out.insert("leader".to_owned(), json!({ "timeout": timeout }));
    }

    // scroll (`migrate.ts:196-205`).
    let scroll_speed = legacy.and_then(|l| l.get("scroll_speed")).cloned();
    let scroll_acceleration = legacy
        .and_then(|l| l.get("scroll_acceleration"))
        .and_then(|a| a.get("enabled"))
        .and_then(Value::as_bool);
    if scroll_speed.is_some() || scroll_acceleration.is_some() {
        let mut scroll = Map::new();
        if let Some(speed) = scroll_speed {
            scroll.insert("speed".to_owned(), speed);
        }
        if let Some(acceleration) = scroll_acceleration {
            scroll.insert("acceleration".to_owned(), json!(acceleration));
        }
        out.insert("scroll".to_owned(), Value::Object(scroll));
    }

    // attention (`migrate.ts:206-213`).
    let attention_sound_pack =
        kv_value("attention_sound_pack").and_then(|v| v.as_str().map(str::to_owned));
    let legacy_attention = legacy.and_then(|l| l.get("attention")).cloned();
    if legacy_attention.is_some() || attention_sound_pack.is_some() {
        let mut attention: Map<String, Value> =
            legacy_attention.and_then(|v| v.as_object().cloned()).unwrap_or_default();
        if let Some(sound_pack) = attention_sound_pack {
            attention.insert("sound_pack".to_owned(), json!(sound_pack));
        }
        out.insert("attention".to_owned(), Value::Object(attention));
    }

    // diffs (`migrate.ts:214-227`).
    let diff_style = legacy.and_then(|l| l.get("diff_style")).and_then(Value::as_str);
    let diff_view = kv_value("diff_viewer_view").and_then(|v| v.as_str().map(str::to_owned));
    let view = diff_view.or(match diff_style {
        Some("stacked") => Some("unified".to_owned()),
        _ => None,
    });
    let wrap = kv_value("diff_wrap_mode").and_then(|v| v.as_str().map(str::to_owned));
    let tree = kv_bool("diff_viewer_show_file_tree");
    let single = kv_bool("diff_viewer_single_patch");
    if wrap.is_some() || tree.is_some() || single.is_some() || view.is_some() {
        let mut diffs = Map::new();
        if let Some(wrap) = wrap {
            diffs.insert("wrap".to_owned(), json!(wrap));
        }
        if let Some(tree) = tree {
            diffs.insert("tree".to_owned(), json!(tree));
        }
        if let Some(single) = single {
            diffs.insert("single".to_owned(), json!(single));
        }
        if let Some(view) = view {
            diffs.insert("view".to_owned(), json!(view));
        }
        out.insert("diffs".to_owned(), Value::Object(diffs));
    }

    // terminal.title (`migrate.ts:228`).
    if let Some(title) = kv_bool("terminal_title_enabled") {
        out.insert("terminal".to_owned(), json!({ "title": title }));
    }

    // prompt (`migrate.ts:229-238`).
    let editor = kv_bool("file_context_enabled");
    let paste =
        kv_bool("paste_summary_enabled").map(|enabled| if enabled { "compact" } else { "full" });
    if editor.is_some() || paste.is_some() {
        let mut prompt = Map::new();
        if let Some(editor) = editor {
            prompt.insert("editor".to_owned(), json!(editor));
        }
        if let Some(paste) = paste {
            prompt.insert("paste".to_owned(), json!(paste));
        }
        out.insert("prompt".to_owned(), Value::Object(prompt));
    }

    // session (`migrate.ts:239-253`).
    let sidebar = kv_value("sidebar").and_then(|v| v.as_str().map(str::to_owned));
    let scrollbar = kv_bool("scrollbar_visible");
    let thinking =
        kv_value("thinking_mode").and_then(|v| v.as_str().map(str::to_owned)).or_else(|| {
            kv_value("thinking_visibility").and_then(|v| {
                v.as_bool().map(|visible| if visible { "show" } else { "hide" }.to_owned())
            })
        });
    let grouping =
        kv_bool("exploration_grouping").map(|enabled| if enabled { "auto" } else { "none" });
    if sidebar.is_some() || scrollbar.is_some() || thinking.is_some() || grouping.is_some() {
        let mut session = Map::new();
        if let Some(sidebar) = sidebar {
            session.insert("sidebar".to_owned(), json!(sidebar));
        }
        if let Some(scrollbar) = scrollbar {
            session.insert("scrollbar".to_owned(), json!(scrollbar));
        }
        if let Some(thinking) = thinking {
            session.insert("thinking".to_owned(), json!(thinking));
        }
        if let Some(grouping) = grouping {
            session.insert("grouping".to_owned(), json!(grouping));
        }
        out.insert("session".to_owned(), Value::Object(session));
    }

    if let Some(animations) = kv_bool("animations_enabled") {
        out.insert("animations".to_owned(), json!(animations));
    }
    if let Some(mouse) = legacy.and_then(|l| l.get("mouse")).and_then(Value::as_bool) {
        out.insert("mouse".to_owned(), json!(mouse));
    }
    if let Some(cursor) = legacy.and_then(|l| l.get("cursor")).cloned() {
        if cursor.as_object().is_some_and(|object| !object.is_empty()) {
            out.insert("cursor".to_owned(), cursor);
        }
    }

    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_map_and_definitions_are_consistent() {
        // v2 dropped some v1 commands between generations
        // (e.g. `app.heap_snapshot` has no v2 definition); the migration
        // REMOVES those entries (migrate.ts:74 `remove = !(target in
        // Definitions)`), so values may legitimately be absent here.
        assert!(is_v2_keybind_name("app.heap_snapshot").is_none());
        assert_eq!(command_map("app_exit"), Some("app.exit"));
        assert_eq!(command_map("prompt_submit"), Some("prompt.submit"));
        assert!(command_map("app.exit").is_none(), "v2 names have no mapping");
        assert!(is_v2_keybind_name("app.exit").is_some());
        assert!(is_v2_keybind_name("nope.nope").is_none());
    }

    #[test]
    fn copy_on_select_renames_to_copy() {
        let mut object = Map::new();
        object.insert("terminal".to_owned(), json!({ "copy_on_select": true, "title": false }));
        let fixed = fix_legacy_document(&object).expect("changed");
        assert_eq!(fixed["terminal"]["copy"], json!("select"));
        assert!(fixed["terminal"].get("copy_on_select").is_none());
        assert_eq!(fixed["terminal"]["title"], json!(false));

        let mut object = Map::new();
        object.insert("terminal".to_owned(), json!({ "copy_on_select": false }));
        let fixed = fix_legacy_document(&object).expect("changed");
        assert_eq!(fixed["terminal"]["copy"], json!("manual"));

        // copy already set: only the legacy key drops.
        let mut object = Map::new();
        object.insert("terminal".to_owned(), json!({ "copy_on_select": true, "copy": "manual" }));
        let fixed = fix_legacy_document(&object).expect("changed");
        assert_eq!(fixed["terminal"]["copy"], json!("manual"));

        // Nothing legacy: no rewrite.
        let mut object = Map::new();
        object.insert("mouse".to_owned(), json!(false));
        assert!(fix_legacy_document(&object).is_none());
    }

    #[test]
    fn v1_keybind_names_rename_or_drop() {
        let mut object = Map::new();
        object.insert(
            "keybinds".to_owned(),
            json!({ "app_exit": "ctrl+q", "theme_list": "f2", "app.exit": "ctrl+c" }),
        );
        let fixed = fix_legacy_document(&object).expect("changed");
        let keybinds = fixed["keybinds"].as_object().expect("object");
        // app_exit → app.exit collides with the existing app.exit → drops
        // (the existing target wins, migrate.ts:74).
        assert_eq!(keybinds.get("app.exit"), Some(&json!("ctrl+c")));
        assert_eq!(keybinds.get("theme.switch"), Some(&json!("f2")));
        assert_eq!(keybinds.len(), 2);

        // Unknown targets drop.
        let mut object = Map::new();
        object.insert("keybinds".to_owned(), json!({ "bogus_key": "f9" }));
        let fixed = fix_legacy_document(&object).expect("changed");
        assert!(fixed["keybinds"].as_object().expect("object").is_empty());
    }

    #[test]
    fn migrate_v1_folds_tui_json_and_kv() {
        let legacy = json!({
            "theme": "catppuccin",
            "keybinds": { "app_exit": "ctrl+q", "bogus": "f9" },
            "plugin": ["oh-my-opencode", ["pkg", { "a": 1 }]],
            "plugin_enabled": { "builtin.x": true, "builtin.y": false },
            "leader_timeout": 750,
            "scroll_speed": 3,
            "scroll_acceleration": { "enabled": false },
            "attention": { "sound": true },
            "diff_style": "stacked",
            "mouse": false,
            "cursor": { "style": "block", "blinking": true }
        });
        let kv = json!({
            "theme_mode_lock": "dark",
            "attention_sound_pack": "custom",
            "diff_viewer_view": "split",
            "diff_wrap_mode": "word",
            "diff_viewer_show_file_tree": false,
            "diff_viewer_single_patch": true,
            "terminal_title_enabled": true,
            "file_context_enabled": true,
            "paste_summary_enabled": true,
            "sidebar": "hide",
            "scrollbar_visible": true,
            "thinking_mode": "hide",
            "exploration_grouping": true,
            "animations_enabled": false
        });
        let migrated = migrate_v1(Some(&legacy), Some(&kv));
        assert_eq!(migrated["theme"], json!({ "name": "catppuccin", "mode": "dark" }));
        assert_eq!(migrated["keybinds"], json!({ "app.exit": "ctrl+q" }));
        assert_eq!(
            migrated["plugins"],
            json!(["oh-my-opencode", { "package": "pkg", "options": { "a": 1 } },
                  "builtin.x", "-builtin.y"])
        );
        assert_eq!(migrated["leader"], json!({ "timeout": 750 }));
        assert_eq!(migrated["scroll"], json!({ "speed": 3, "acceleration": false }));
        assert_eq!(migrated["attention"]["sound_pack"], json!("custom"));
        assert_eq!(
            migrated["diffs"],
            json!({ "wrap": "word", "tree": false, "single": true, "view": "split" })
        );
        assert_eq!(migrated["terminal"], json!({ "title": true }));
        assert_eq!(migrated["prompt"], json!({ "editor": true, "paste": "compact" }));
        assert_eq!(
            migrated["session"],
            json!({ "sidebar": "hide", "scrollbar": true, "thinking": "hide", "grouping": "auto" })
        );
        assert_eq!(migrated["animations"], json!(false));
        assert_eq!(migrated["mouse"], json!(false));
        assert_eq!(migrated["cursor"], json!({ "style": "block", "blinking": true }));
    }

    #[test]
    fn migrate_v1_with_no_sources_is_empty() {
        assert_eq!(migrate_v1(None, None), json!({}));
        // thinking_visibility folds to show/hide when thinking_mode is
        // absent (migrate.ts:171-172).
        let kv = json!({ "thinking_visibility": true });
        assert_eq!(migrate_v1(None, Some(&kv))["session"]["thinking"], json!("show"));
        // kv alone with irrelevant keys produces nothing.
        assert_eq!(migrate_v1(None, Some(&json!({ "other": 1 }))), json!({}));
    }
}
