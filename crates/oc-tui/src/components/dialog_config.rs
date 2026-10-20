//! TUI-23: the full `DialogConfig` settings table
//! (`component/dialog-config.tsx:11-330`) — every v2.0.24 row with
//! its category, value list (or numeric step), display labels and
//! search keywords; the CURRENT-value resolution against the store's
//! live state (falling back to the boot snapshot seeded from
//! `TuiConfig`), and the `change(direction)` cycle every row runs on
//! ←/→/enter.
//!
//! Rows whose key has a LIVE consumer (theme, sidebar, thinking,
//! verbosity, tps, markdown, grouping, tabs mode, image previews,
//! mouse) update the store directly; every row writes through the
//! config sink's generic `update_setting` (v2's `config.update`
//! draft walk) and records the new display value into the snapshot.

// The settings-table literal and the cycle math mirror the ref's
// numeric coercion shapes (HYGIENE-CLIPPY-TUI).
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::too_many_lines)]

/// One settings row (`Setting`, dialog-config.tsx:11-24).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingDef {
    pub title: &'static str,
    pub category: &'static str,
    /// The dot path (`path.join(".")` — the row's id).
    pub path: &'static str,
    pub default: &'static str,
    /// The value cycle (`values`); `None` = a numeric step row.
    pub values: Option<&'static [&'static str]>,
    /// Display labels per value (`labels[i]`).
    pub labels: Option<&'static [&'static str]>,
    /// Numeric rows: (step, min, max) in the config's unit.
    pub step: Option<(f64, f64, f64)>,
    /// The search text (`keywords.join(" ")`).
    pub keywords: &'static str,
}

impl SettingDef {
    /// `settingID` — the dot path.
    #[must_use]
    pub fn id(&self) -> &'static str {
        self.path
    }

    /// The display value for `value` (`display`: the label at the
    /// value's index, else the raw value; the numeric rows' `format`
    /// applies — `scroll.speed` two decimals, `leader.timeout` the
    /// ` ms` suffix, `attention.volume` the percent).
    #[must_use]
    pub fn display(&self, value: &str) -> String {
        if let Some(values) = self.values {
            if let Some(index) = values.iter().position(|candidate| *candidate == value) {
                return self
                    .labels
                    .and_then(|labels| labels.get(index).copied())
                    .unwrap_or(value)
                    .to_owned();
            }
        }
        match self.path {
            "scroll.speed" => format!("{value:.2}"),
            "leader.timeout" => format!("{value} ms"),
            "attention.volume" => match value.parse::<f64>() {
                Ok(ratio) => format!("{}%", (ratio * 100.0).round() as i64),
                Err(_) => value.to_owned(),
            },
            _ => value.to_owned(),
        }
    }
}

/// The full v2.0.24 table (`settings: Setting[]`,
/// dialog-config.tsx:26-330) — order verbatim.
#[must_use]
pub fn settings_table() -> Vec<SettingDef> {
    use SettingDef as S;
    vec![
        S {
            title: "Theme",
            category: "Appearance",
            path: "theme.name",
            default: "opencode",
            values: None,
            labels: None,
            step: None,
            keywords: "color scheme, colors",
        },
        S {
            title: "Color mode",
            category: "Appearance",
            path: "theme.mode",
            default: "system",
            values: Some(&["system", "dark", "light"]),
            labels: None,
            step: None,
            keywords: "",
        },
        S {
            title: "Animations",
            category: "Appearance",
            path: "animations",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "motion, effects",
        },
        S {
            title: "Sidebar",
            category: "Session",
            path: "session.sidebar",
            default: "auto",
            values: Some(&["hide", "auto"]),
            labels: None,
            step: None,
            keywords: "side panel",
        },
        S {
            title: "Scrollbar",
            category: "Session",
            path: "session.scrollbar",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "scroll bar",
        },
        S {
            title: "Thinking",
            category: "Session",
            path: "session.thinking",
            default: "hide",
            values: Some(&["hide", "show"]),
            labels: None,
            step: None,
            keywords: "reasoning, chain of thought",
        },
        S {
            title: "Markdown",
            category: "Session",
            path: "session.markdown",
            default: "rendered",
            values: Some(&["source", "rendered"]),
            labels: None,
            step: None,
            keywords: "syntax, concealment, rendering",
        },
        S {
            title: "Tool grouping",
            category: "Session",
            path: "session.grouping",
            default: "auto",
            values: Some(&["none", "auto"]),
            labels: None,
            step: None,
            keywords: "transcript, messages, reads, searches",
        },
        S {
            title: "Verbosity",
            category: "Session",
            path: "session.verbosity",
            default: "medium",
            values: Some(&["low", "medium", "high"]),
            labels: None,
            step: None,
            keywords: "detail, activity, summary, transcript",
        },
        S {
            title: "Transcript images",
            category: "Session",
            path: "session.image_preview",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "attachments, images, tool output",
        },
        S {
            title: "TPS",
            category: "Session",
            path: "session.tps",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "tokens per second, throughput",
        },
        S {
            title: "New session location",
            category: "Session",
            path: "session.new_location",
            default: "launch",
            values: Some(&["launch", "inherit"]),
            labels: Some(&["launch directory", "active session"]),
            step: None,
            keywords: "directory, cwd, inherit",
        },
        S {
            title: "Permissions",
            category: "Session",
            path: "session.permissions",
            default: "prompt",
            values: Some(&["prompt", "autoaccept"]),
            labels: Some(&["prompt", "auto accept"]),
            step: None,
            keywords: "approve, accept, permission requests",
        },
        S {
            title: "Mode",
            category: "Tabs",
            path: "tabs.mode",
            default: "auto",
            values: Some(&["off", "on", "auto"]),
            labels: None,
            step: None,
            keywords: "",
        },
        S {
            title: "Scope",
            category: "Tabs",
            path: "tabs.scope",
            default: "cwd",
            values: Some(&["cwd", "global"]),
            labels: Some(&["current directory", "global"]),
            step: None,
            keywords: "",
        },
        S {
            title: "Layout",
            category: "Tabs",
            path: "tabs.layout",
            default: "horizontal",
            values: Some(&["horizontal", "vertical"]),
            labels: None,
            step: None,
            keywords: "sidebar, orientation, left",
        },
        S {
            title: "Indicators",
            category: "Tabs",
            path: "tabs.indicators",
            default: "status",
            values: Some(&["status", "numbers"]),
            labels: Some(&["status icons", "always show numbers"]),
            step: None,
            keywords: "tab numbers, number mode, status icons",
        },
        S {
            title: "Layout",
            category: "Diffs",
            path: "diffs.view",
            default: "auto",
            values: Some(&["auto", "split", "unified"]),
            labels: None,
            step: None,
            keywords: "diff layout, split diff, unified diff",
        },
        S {
            title: "Wrapping",
            category: "Diffs",
            path: "diffs.wrap",
            default: "word",
            values: Some(&["none", "word"]),
            labels: None,
            step: None,
            keywords: "diff wrap, word wrap, line wrap",
        },
        S {
            title: "File tree",
            category: "Diffs",
            path: "diffs.tree",
            default: "true",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "diff files",
        },
        S {
            title: "Single patch",
            category: "Diffs",
            path: "diffs.single",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "one file, selected file",
        },
        S {
            title: "Scroll speed",
            category: "Input",
            path: "scroll.speed",
            default: "3.00",
            values: None,
            labels: None,
            step: Some((0.25, 0.25, 10.0)),
            keywords: "scrolling",
        },
        S {
            title: "Acceleration",
            category: "Input",
            path: "scroll.acceleration",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "scroll acceleration",
        },
        S {
            title: "Mouse",
            category: "Input",
            path: "mouse",
            default: "true",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "mouse capture",
        },
        S {
            title: "Editor context",
            category: "Input",
            path: "prompt.editor",
            default: "true",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "file context, prompt context, editor selection",
        },
        S {
            title: "Large pastes",
            category: "Input",
            path: "prompt.paste",
            default: "compact",
            values: Some(&["compact", "full"]),
            labels: None,
            step: None,
            keywords: "paste summary, clipboard, pasted content",
        },
        S {
            title: "Image previews",
            category: "Input",
            path: "prompt.image_preview",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "attachments, clipboard, images, prompt",
        },
        S {
            title: "Leader timeout",
            category: "Input",
            path: "leader.timeout",
            default: "2000",
            values: None,
            labels: None,
            step: Some((250.0, 250.0, 10_000.0)),
            keywords: "leader key, shortcut timeout",
        },
        S {
            title: "Notifications",
            category: "Alerts",
            path: "attention.notifications",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "system notifications, desktop notifications, alerts",
        },
        S {
            title: "Sounds",
            category: "Alerts",
            path: "attention.sound",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "audio, sound effects, alerts",
        },
        S {
            title: "Volume",
            category: "Alerts",
            path: "attention.volume",
            default: "0.40",
            values: None,
            labels: None,
            step: Some((0.1, 0.0, 1.0)),
            keywords: "sound volume, audio volume",
        },
        S {
            title: "Window title",
            category: "Terminal",
            path: "terminal.title",
            default: "true",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "terminal title, tab title",
        },
        S {
            title: "Copy behavior",
            category: "Terminal",
            path: "terminal.copy",
            default: "select",
            values: Some(&["manual", "select"]),
            labels: None,
            step: None,
            keywords: "selection, clipboard",
        },
        S {
            title: "Developer tools",
            category: "Debug",
            path: "debug.devtools",
            default: "false",
            values: Some(&["false", "true"]),
            labels: Some(&["off", "on"]),
            step: None,
            keywords: "debug bar, developer tools",
        },
    ]
}

/// The CURRENT value of one row (v2's `value(setting)`): the LIVE
/// store state where the port models it, else the boot snapshot,
/// else the row default. Returns the RAW value (pre-label).
#[must_use]
pub fn current_value(store: &crate::store::Store, setting: &SettingDef) -> String {
    match setting.path {
        "theme.name" => store.theme_name(),
        "theme.mode" => store.theme_mode_pref().unwrap_or_else(|| "system".to_owned()),
        "session.sidebar" => match store.sidebar_mode() {
            crate::components::sidebar::SidebarMode::Hide => "hide".to_owned(),
            crate::components::sidebar::SidebarMode::Auto => "auto".to_owned(),
        },
        "session.thinking" => if store.session_thinking() { "show" } else { "hide" }.to_owned(),
        "session.markdown" => {
            if store.session_markdown_source() { "source" } else { "rendered" }.to_owned()
        }
        "session.grouping" => if store.exploration_grouping() { "auto" } else { "none" }.to_owned(),
        "session.verbosity" => store.session_verbosity(),
        "session.image_preview" => if store.image_preview() { "true" } else { "false" }.to_owned(),
        "session.tps" => if store.session_tps() { "true" } else { "false" }.to_owned(),
        "tabs.mode" => if store.tabs_enabled() { "on" } else { "off" }.to_owned(),
        other => {
            let snapshot = store.settings_snapshot_value(other);
            if snapshot.is_empty() {
                setting.default.to_owned()
            } else {
                snapshot
            }
        }
    }
}

/// `change(direction)` over one row (dialog-config.tsx:334-357): the
/// value cycle for list rows, the clamped step for numeric rows;
/// `next === current` is a no-op. Applies the LIVE consumer where one
/// exists, records the display into the snapshot, and writes through
/// the config sink. Returns the NEW raw value.
#[must_use]
pub fn change_setting(store: &crate::store::Store, setting: &SettingDef, direction: i64) -> String {
    let current = current_value(store, setting);
    let next = if let Some(values) = setting.values {
        let count = values.len();
        let index = values.iter().position(|value| *value == current).unwrap_or(0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let next_index = (i64::try_from(index).unwrap_or(0) + direction)
            .rem_euclid(i64::try_from(count).unwrap_or(1))
            .max(0) as usize;
        values[next_index].to_owned()
    } else if let Some((step, min, max)) = setting.step {
        let value: f64 = current.parse().unwrap_or_else(|_| setting.default.parse().unwrap_or(0.0));
        (value + f64::from(i32::try_from(direction).unwrap_or(0)) * step)
            .clamp(min, max)
            .to_string()
    } else {
        return current;
    };
    if next == current {
        return current;
    }
    // The LIVE consumers (v2's config.update effect re-reads; the
    // port updates the modeled state directly).
    match setting.path {
        "theme.name" => store.setting_cycle("theme.name", direction),
        "theme.mode" => store.setting_cycle("theme.mode", direction),
        "session.sidebar" => store.setting_cycle("session.sidebar", direction),
        "session.grouping" => store.setting_cycle("session.grouping", direction),
        "session.thinking" => store.set_session_thinking(next == "show"),
        "session.markdown" => store.set_session_markdown_source(next == "source"),
        "session.verbosity" => store.set_session_verbosity(&next),
        "session.image_preview" => store.set_image_preview(next == "true"),
        "session.tps" => store.set_session_tps(next == "true"),
        "tabs.mode" => store.set_tabs_enabled(next == "on"),
        "tabs.layout" => store.set_tabs_layout(if next == "vertical" {
            crate::components::session_tabs_rail::TabsLayout::Vertical
        } else {
            crate::components::session_tabs_rail::TabsLayout::Horizontal
        }),
        _ => {}
    }
    // The snapshot keeps the RAW value (the props-time `display`
    // formats); the config sink receives the same raw string.
    store.settings_snapshot_set(setting.path, &next);
    store.config_sink_handle().update_setting(setting.path, &next);
    store.request_redraw();
    next
}
