//! TUI-5/TUI-6 (lane-tui2): `DialogOpen` (component/dialog-open.tsx)
//! and the enriched session-list rows (component/dialog-session-list.
//! tsx).
//!
//! * `open_menu_props` — the Open menu's options: the Sessions
//!   recents (fallback titles + `{label} · {timeAgo}` footers) followed
//!   by the Projects section (name titles + abbreviated directory
//!   footers), over the store's project registry.
//! * `open_worktrees_props` — the right-arrow worktrees view
//!   (`{Project} / Worktrees`): the canonical root plus the fetched
//!   worktree directories.
//! * `session_list_options` — the Sessions dialog's rows: date-group
//!   categories ("Today" / the day string), timestamped fallback
//!   titles, the outside-project footer basename, and the armed
//!   two-press delete's confirm title.
//!
//! The wire's `time.*` fields are epoch MILLISECONDS (the schema's
//! `DateTimeUtcFromMillis`); every helper here takes millis.

//! The wall-clock helpers port JS `Date` semantics over wire epoch
//! milliseconds — the integer casts are bounded by the domain
//! (in-range timestamps, small month/day indices).
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]

use crate::components::dialog_select::{SelectOption, SelectProps};
use crate::store::Store;

/// One `project.list` row (the Open menu's Projects section + the
/// session footers' project labels).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub id: String,
    pub name: Option<String>,
    pub canonical: String,
    /// `vcs == "git"` marks the worktrees-capable rows (the " →"
    /// footer suffix + the right-arrow view).
    pub vcs: bool,
}

/// `projectName` (util/project.ts): the configured name, else the
/// canonical path's basename.
#[must_use]
pub fn project_name(name: Option<&str>, canonical: &str) -> String {
    name.map_or_else(
        || basename(canonical),
        |text| {
            if text.is_empty() {
                basename(canonical)
            } else {
                text.to_owned()
            }
        },
    )
}

/// `path.basename` (POSIX): the segment after the final `/`.
#[must_use]
pub fn basename(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((_, last)) if !last.is_empty() => last.to_owned(),
        _ => path.to_owned(),
    }
}

// -- wall-clock helpers ------------------------------------------------------

/// `Date.now()` — epoch milliseconds.
#[must_use]
pub fn wall_now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_millis() as f64)
}

/// Civil date parts (year, month 1-12, day 1-31) from epoch days
/// (`civil_from_days`, Howard Hinnant's algorithm).
#[must_use]
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Weekday index from epoch days: 0 = Sunday … 6 = Saturday
/// (1970-01-01 was a Thursday).
#[must_use]
fn weekday_from_days(days: i64) -> u32 {
    u32::try_from((days + 4).rem_euclid(7)).unwrap_or(0)
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn epoch_days(ms: f64) -> i64 {
    (ms / 86_400_000.0).floor() as i64 // bounded: date math on wire timestamps
}

/// `new Date(ms).toDateString()` — "Thu Jan 01 1970".
#[must_use]
pub fn date_string(ms: f64) -> String {
    let days = epoch_days(ms);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{} {} {:02} {}",
        WEEKDAYS[weekday_from_days(days) as usize],
        MONTHS[(month - 1) as usize],
        day,
        year
    )
}

/// `Date.toDateString() === new Date().toDateString()` — the session
/// list's "Today" group test, else the day string.
#[must_use]
pub fn date_group(updated_ms: f64, now_ms: f64) -> String {
    let updated = date_string(updated_ms);
    if updated == date_string(now_ms) {
        "Today".to_owned()
    } else {
        updated
    }
}

/// `new Date(ms).toISOString()` — "1970-01-01T00:00:00.000Z"
/// (UTC, millisecond precision).
#[must_use]
pub fn iso_utc(ms: f64) -> String {
    let total_seconds = (ms / 1000.0).floor() as i64;
    let millis = (ms - (f64::from(i32::try_from(total_seconds).unwrap_or(0)) * 1000.0))
        .round()
        .rem_euclid(1000.0) as u32;
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60
    )
}

/// `timeAgo` (dialog-open.tsx:497-508): "now" under a minute, then
/// m/h/d/mo/y buckets.
#[must_use]
pub fn time_ago(timestamp_ms: f64, now_ms: f64) -> String {
    let minutes = ((now_ms - timestamp_ms) / 60_000.0).floor();
    if minutes < 1.0 {
        return "now".to_owned();
    }
    if minutes < 60.0 {
        return format!("{}m", minutes.max(0.0) as u64);
    }
    let hours = (minutes / 60.0).floor();
    if hours < 24.0 {
        return format!("{}h", hours.max(0.0) as u64);
    }
    let days = (hours / 24.0).floor();
    if days < 30.0 {
        return format!("{}d", days.max(0.0) as u64);
    }
    let months = (days / 30.0).floor();
    if months < 12.0 {
        return format!("{}mo", months.max(0.0) as u64);
    }
    format!("{}y", (days / 365.0).floor().max(0.0) as u64)
}

/// `withTimestampedFallback` (util/session-title-fallback.ts): the
/// title, else "New session - <ISO created>" (a child row: "Child
/// session - …"). `created_ms` is only read when the title is absent.
#[must_use]
pub fn with_timestamped_fallback(
    title: Option<&str>,
    parent_id: Option<&str>,
    created_ms: Option<f64>,
) -> String {
    if let Some(title) = title.filter(|text| !text.is_empty()) {
        return title.to_owned();
    }
    let created = created_ms.unwrap_or_default();
    let kind = if parent_id.is_some() { "Child" } else { "New" };
    format!("{kind} session - {}", iso_utc(created))
}

/// `Locale.truncate` (util/locale.ts): the clipped string with an
/// ellipsis when it exceeds `max`.
#[must_use]
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

/// The session-list row footer (dialog-session-list.tsx:152-160): the
/// directory basename ONLY when the session root sits OUTSIDE the
/// owning project's canonical root (the path-relative test).
#[must_use]
pub fn outside_project_footer(
    directory: Option<&str>,
    subpath: Option<&str>,
    project_canonical: Option<&str>,
) -> Option<String> {
    let directory = directory?;
    // `path.resolve(directory, ..."..")` — a subpath walks the session
    // root up out of the checkout.
    let root = match subpath {
        Some(sub) if !sub.is_empty() => {
            let ups = sub.matches('/').count() + 1;
            let mut root = directory.trim_end_matches('/').to_owned();
            for _ in 0..ups {
                root = match root.rsplit_once('/') {
                    Some((parent, _)) => parent.to_owned(),
                    None => String::new(),
                };
            }
            root
        }
        _ => directory.to_owned(),
    };
    let base = project_canonical.unwrap_or(&root);
    let relative = path_relative(base, &root);
    if relative.starts_with("..") || relative.starts_with('/') {
        Some(truncate(&basename(&relative), 25))
    } else {
        None
    }
}

/// `path.relative(from, to)` (the lexically simple cases the dialog
/// feeds it: absolute POSIX paths).
fn path_relative(from: &str, to: &str) -> String {
    let from_parts: Vec<&str> = from.split('/').filter(|part| !part.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|part| !part.is_empty()).collect();
    let mut common = 0;
    while common < from_parts.len()
        && common < to_parts.len()
        && from_parts[common] == to_parts[common]
    {
        common += 1;
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_parts.len() {
        parts.push("..".to_owned());
    }
    for part in &to_parts[common..] {
        parts.push((*part).to_owned());
    }
    if parts.is_empty() {
        String::new()
    } else {
        parts.join("/")
    }
}

// -- the Open menu (dialog-open.tsx) -----------------------------------------

/// The Open dialog's recents cap (`RECENT_LIMIT`).
const RECENT_LIMIT: usize = 8;

/// The Open menu's options memo (dialog-open.tsx:197-274): the
/// Sessions recents (open tabs excluded on an empty filter — the strip
/// already shows them) then the Projects rows. Loading projects paint
/// the refresh placeholder row; the both-empty case is the dialog's
/// `emptyView`.
#[must_use]
#[allow(clippy::too_many_lines)] // one options memo, v2's dialog-open.tsx:197-274 shape
pub fn open_menu_props(store: &Store) -> SelectProps {
    let now = wall_now_ms();
    let open_tabs: std::collections::HashSet<String> =
        store.tabs().tabs.iter().map(|tab| tab.session_id.clone()).collect();
    let current_session = match store.route() {
        crate::store::Route::Session { session_id } => Some(session_id),
        crate::store::Route::Home => None,
    };

    // The sessions memo (:160-175): parents only, deduped, newest
    // first; the live registry title wins (data.session.get).
    let mut sessions: Vec<crate::data::SessionSnapshot> =
        store.session_history().into_iter().filter(|session| session.parent_id.is_none()).collect();
    sessions.sort_by(|a, b| {
        b.time_updated.unwrap_or_default().total_cmp(&a.time_updated.unwrap_or_default())
    });

    // The recents memo (:203-207): an empty filter hides what the tab
    // strip already shows; typing widens to every session. (The
    // attention-first sort rides the tab state the port tracks per
    // tab, not per history row — the recents keep newest-first.)
    let recents: Vec<crate::data::SessionSnapshot> = sessions
        .into_iter()
        .filter(|session| {
            !open_tabs.contains(&session.id) || Some(&session.id) != current_session.as_ref()
        })
        .take(RECENT_LIMIT)
        .collect();

    let mut options: Vec<SelectOption> = Vec::new();
    let mut has_sessions = false;
    for session in &recents {
        has_sessions = true;
        let project = project_by_id(store, session.project_id.as_deref());
        let name = project.as_ref().map(|row| project_name(row.name.as_deref(), &row.canonical));
        let directory = session.directory.clone().unwrap_or_default();
        let dir_basename = basename(&directory);
        // The label (:211-215): "{name} · {basename}" when the session
        // sits outside the project root under a distinct name, else the
        // name (or the bare basename).
        let label = match (&name, &project) {
            (Some(name), Some(project))
                if !directory.is_empty()
                    && directory != project.canonical
                    && name.to_lowercase() != dir_basename.to_lowercase() =>
            {
                format!("{name} · {dir_basename}")
            }
            (Some(name), _) => name.clone(),
            (None, _) if !directory.is_empty() => dir_basename.clone(),
            (None, _) => String::new(),
        };
        let title = with_timestamped_fallback(
            store.session_title(&session.id).as_deref().or(session.title.as_deref()),
            session.parent_id.as_deref(),
            session.time_created,
        );
        let footer = format!(
            "{}{}",
            if label.is_empty() { String::new() } else { format!("{} · ", truncate(&label, 30)) },
            time_ago(session.time_updated.unwrap_or_default(), now)
        );
        options.push(
            SelectOption::titled(title, format!("ses:{}", session.id))
                .categorized("Sessions")
                .with_footer(footer)
                .with_search(format!("{} {}", session.id, directory)),
        );
    }

    // The projects memo (:238-271): non-root canonicals, deduped,
    // name-titled with the abbreviated directory footer (" →" on git).
    let mut has_projects = false;
    match store.project_list() {
        crate::store::ListState::Loading => {
            options.push(
                SelectOption::titled("Refreshing sessions and projects…", "status")
                    .categorized("Projects")
                    .muted_placeholder(),
            );
        }
        crate::store::ListState::Failed(error) => {
            options.push(
                SelectOption::titled(format!("Could not refresh projects: {error}"), "status")
                    .categorized("Projects")
                    .muted_placeholder(),
            );
        }
        crate::store::ListState::Loaded(projects) => {
            let mut seen: Vec<&str> = Vec::new();
            for project in &projects {
                if project.canonical == "/" || seen.contains(&project.canonical.as_str()) {
                    continue;
                }
                seen.push(&project.canonical);
                has_projects = true;
                let title = project_name(project.name.as_deref(), &project.canonical);
                let mut footer = project.canonical.clone();
                if project.vcs {
                    footer.push_str(" →");
                }
                options.push(
                    SelectOption::titled(title, format!("prj:{}", project.id))
                        .categorized("Projects")
                        .with_footer(footer)
                        .with_search(format!(
                            "{} {}",
                            project.canonical,
                            project.name.clone().unwrap_or_default()
                        )),
                );
            }
        }
    }
    let _ = (has_sessions, has_projects);

    SelectProps::new("Open", options)
        .with_empty_texts("No recent sessions or projects", "No matches")
}

/// The worktrees view (`{Project} / Worktrees`, dialog-open.tsx
/// :276-322 + :338): the project's canonical root first, then the
/// fetched worktree directories — basename titles, abbreviated
/// directory footers, the current root dotted.
#[must_use]
pub fn open_worktrees_props(
    store: &Store,
    project_id: &str,
    project_name: &str,
    home: &str,
) -> SelectProps {
    let state = store.worktree_dialog();
    let project = project_by_id(store, Some(project_id));
    let canonical = project.map_or_else(String::new, |row| row.canonical.clone());
    let mut options = Vec::new();
    // The main checkout row (the dialog's restore focus target).
    if !canonical.is_empty() {
        options.push(
            SelectOption::titled(project_name.to_owned(), format!("dir:{canonical}"))
                .categorized("Worktrees")
                .with_footer(crate::components::dialog_worktrees::abbreviate_home(
                    &canonical, home,
                )),
        );
    }
    match &state.list {
        crate::store::ListState::Loading => {
            options.push(
                SelectOption::titled("Loading worktrees…", "status")
                    .categorized("Worktrees")
                    .muted_placeholder(),
            );
        }
        crate::store::ListState::Failed(error) => {
            options.push(
                SelectOption::titled(format!("Loading worktrees failed: {error}"), "status")
                    .categorized("Worktrees")
                    .muted_placeholder(),
            );
        }
        crate::store::ListState::Loaded(entries) => {
            for entry in entries {
                if entry.directory == canonical {
                    continue;
                }
                options.push(
                    SelectOption::titled(
                        basename(&entry.directory),
                        format!("dir:{}", entry.directory),
                    )
                    .categorized("Worktrees")
                    .with_footer(
                        crate::components::dialog_worktrees::abbreviate_home(
                            &entry.directory,
                            home,
                        ),
                    ),
                );
            }
        }
    }
    SelectProps::new(format!("{project_name} / Worktrees"), options)
        .with_empty_texts("No matching worktrees", "No matching worktrees")
        .with_action(crate::components::dialog_select::SelectAction::new(
            "dialog.worktree.generate",
            "new worktree",
            "ctrl+n",
        ))
}

/// The store's project row by id (the footer labels' lookup).
fn project_by_id(store: &Store, id: Option<&str>) -> Option<ProjectRow> {
    let id = id?;
    match store.project_list() {
        crate::store::ListState::Loaded(projects) => {
            projects.into_iter().find(|project| project.id == id)
        }
        _ => None,
    }
}

/// The Open dialog's `onSelect` (dialog-open.tsx:435-446): a session
/// row navigates the session route; a project row navigates home.
pub fn execute_open_selection(
    store: &Store,
    value: &str,
) -> crate::components::command_palette::Dispatch {
    if let Some(session_id) = value.strip_prefix("ses:") {
        store.select_tab(session_id);
        return crate::components::command_palette::Dispatch::Handled;
    }
    // A project/directory row: v2 navigates home at the directory (the
    // port's home route keeps the process cwd — directory-scoped home
    // routing rides the location service, outside this surface).
    crate::components::command_palette::Dispatch::Handled
}

// -- the Sessions dialog rows (dialog-session-list.tsx) ----------------------

/// The Sessions dialog's options memo (dialog-session-list.tsx
/// :141-192): parents only, newest first, one category per day
/// ("Today" for the current day), the timestamped fallback title, the
/// outside-project footer basename, and the armed two-press delete's
/// confirm title.
#[must_use]
#[allow(clippy::too_many_lines)] // one row builder, v2's memo shape
pub fn session_list_options(store: &Store) -> Vec<SelectOption> {
    let now = wall_now_ms();
    let armed = store.session_delete_armed();
    let mut sessions: Vec<crate::data::SessionSnapshot> =
        store.session_history().into_iter().filter(|session| session.parent_id.is_none()).collect();
    sessions.sort_by(|a, b| {
        b.time_updated.unwrap_or_default().total_cmp(&a.time_updated.unwrap_or_default())
    });
    // The Pinned group (dialog-session-list.tsx:148/191): the
    // quick-switch pins, only while multi-tab mode is OFF (v2's
    // `sessionTabs.enabled() ? [] : local.session.pinned()`).
    let pinned: Vec<String> = if store.tabs_enabled() {
        Vec::new()
    } else {
        store
            .tabs()
            .pinned
            .iter()
            .filter(|id| sessions.iter().any(|session| &session.id == *id))
            .cloned()
            .collect()
    };
    let row = |session: &crate::data::SessionSnapshot, category: &str| -> SelectOption {
        let deleting = armed.as_deref() == Some(session.id.as_str());
        let title = if deleting {
            "Press ctrl+d again to confirm".to_owned()
        } else {
            with_timestamped_fallback(
                store.session_title(&session.id).as_deref().or(session.title.as_deref()),
                session.parent_id.as_deref(),
                session.time_created,
            )
        };
        let project = project_by_id(store, session.project_id.as_deref());
        let footer = outside_project_footer(
            session.directory.as_deref(),
            None,
            project.as_ref().map(|row| row.canonical.as_str()),
        );
        let mut option =
            SelectOption::titled(title, session.id.clone()).categorized(category.to_owned());
        if let Some(footer) = footer {
            option = option.with_footer(footer);
        }
        option
    };
    let mut options: Vec<SelectOption> = pinned
        .iter()
        .filter_map(|id| sessions.iter().find(|session| &session.id == id))
        .map(|session| row(session, "Pinned"))
        .collect();
    options.extend(
        sessions.iter().filter(|session| !pinned.contains(&session.id)).map(|session| {
            row(session, &date_group(session.time_updated.unwrap_or_default(), now))
        }),
    );
    options
}
