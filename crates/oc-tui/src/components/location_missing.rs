//! TUI-30 — the session-location recovery screen
//! (`routes/session/location-missing.tsx`): when the location worker
//! failed to resolve the routed session's `location.directory`
//! (`currentLocation.error?.location.directory ===
//! session().location.directory`, index.tsx:1484-1496), the
//! `SessionLocationUnavailable` question REPLACES the prompt mount —
//! a `SessionQuestion` (permission.tsx:407+) with the warning header,
//! the abbreviated/truncated directory + guidance body, and the
//! single `Choose directory` recovery action (→ `DialogWorkspaces`,
//! whose selection `session.move`s the session).

/// The mount's row budget (v2's `SessionQuestion` clamps at 15; the
/// single-option body needs 6: header, directory, guidance, spacing,
/// the choices label + the pill).
pub const LOCATION_MISSING_ROWS: usize = 6;

/// The home directory for `abbreviateHome` (the same resolution the
/// worktrees dialog uses — `$HOME` at render time).
fn home_directory() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// `Locale.truncateMiddle(abbreviateHome(directory), 72)`
/// (location-missing.tsx:69-70).
#[must_use]
pub fn display_directory(directory: &str, home: &str) -> String {
    let abbreviated = crate::components::dialog_worktrees::abbreviate_home(directory, home);
    truncate_middle(&abbreviated, 72)
}

fn truncate_middle(value: &str, budget: usize) -> String {
    if budget == 0 {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= budget {
        return value.to_owned();
    }
    if budget == 1 {
        return "…".to_owned();
    }
    let head_budget = budget.div_ceil(2);
    let tail_budget = budget - head_budget;
    let head: String = chars.iter().take(head_budget.saturating_sub(1)).collect();
    let tail: String = chars.iter().skip(chars.len().saturating_sub(tail_budget)).collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::location_missing_solid::render_location_missing_solid;
    use crate::theme::ThemeState;

    /// The body copy + the truncation (`truncateMiddle(…, 72)`).
    #[test]
    fn directory_display_truncates_the_middle() {
        assert_eq!(display_directory("/gone/dir", "/home/u"), "/gone/dir");
        assert_eq!(display_directory("/home/u/work", "/home/u"), "~/work");
        let long = format!("/base/{}", "x".repeat(120));
        let shown = display_directory(&long, "/nowhere");
        assert!(shown.starts_with('/'));
        assert!(shown.contains('…'));
        assert!(shown.chars().count() <= 72);
    }

    /// The mount paints the question's rows.
    #[test]
    fn mount_paints_the_question() {
        let theme = ThemeState::v2_dark();
        let frame = render_location_missing_solid("/gone/dir", &theme.styles(), 60);
        let text = frame.text_grid().join("\n");
        assert!(text.contains("Session location unavailable"));
        assert!(text.contains("/gone/dir"));
        assert!(text.contains("Choose another directory to continue this session."));
        assert!(text.contains("Recovery actions"));
        assert!(text.contains("Choose directory"));
    }
}
