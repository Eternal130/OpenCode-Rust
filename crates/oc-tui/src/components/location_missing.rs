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

use crate::render::{Cell, CellColor, Frame};
use crate::theme::ThemeStyles;

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

/// Paint the question mount (the composer's dock region): the `△`
/// warning header, the muted directory + the guidance line, the
/// `Recovery actions` choices label with the `Choose directory` pill.
#[must_use]
pub fn render_location_missing(directory: &str, styles: &ThemeStyles, width: usize) -> Frame {
    let width = width.max(4);
    let mut cells: Vec<Vec<Cell>> = vec![vec![Cell::default(); width]; LOCATION_MISSING_ROWS];
    let text = styles.text.fg.map_or(CellColor::Default, CellColor::from);
    let muted = styles.text_muted.fg.map_or(CellColor::Default, CellColor::from);
    let warning = CellColor::from(styles.warning);
    let accent = CellColor::from(styles.action_hovered);

    let put = |cells: &mut [Vec<Cell>], row: usize, x: usize, text: &str, fg: CellColor| {
        if let Some(line) = cells.get_mut(row) {
            for (index, ch) in text.chars().enumerate() {
                if x + index + 1 >= width {
                    break;
                }
                if let Some(cell) = line.get_mut(x + index) {
                    cell.ch = ch;
                    cell.fg = fg;
                }
            }
        }
    };

    // The left rail (SessionQuestion's `border: ["left"]` in the
    // focused action color).
    for row in &mut cells {
        if let Some(cell) = row.first_mut() {
            cell.ch = '▎';
            cell.fg = accent;
        }
    }
    // Header: `△ {title}` (permission.tsx's fallback header shape).
    put(&mut cells, 0, 2, "△", warning);
    put(&mut cells, 0, 4, "Session location unavailable", text);
    // Body: the truncated directory (muted) + the guidance line.
    let home = home_directory();
    put(&mut cells, 1, 2, &display_directory(directory, &home), muted);
    put(&mut cells, 2, 2, "Choose another directory to continue this session.", text);
    // The choices: the label + the single pill (`options: { move:
    // "Choose directory" }`).
    put(&mut cells, 4, 2, "Recovery actions", muted);
    put(&mut cells, 5, 3, "Choose directory", text);
    if let Some(row) = cells.get_mut(5) {
        for cell in row.iter_mut().skip(2).take(17) {
            cell.attrs |= 1 << 7; // the pill's raised background
        }
    }
    let height = LOCATION_MISSING_ROWS;
    Frame { cells, width, height }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let frame = render_location_missing("/gone/dir", &theme.styles(), 60);
        let text = frame.text_grid().join("\n");
        assert!(text.contains("Session location unavailable"));
        assert!(text.contains("/gone/dir"));
        assert!(text.contains("Choose another directory to continue this session."));
        assert!(text.contains("Recovery actions"));
        assert!(text.contains("Choose directory"));
    }
}
