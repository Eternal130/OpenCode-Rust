//! RE-14 — the `/stats` page's SOLID renderer (feature-plugins/
//! system/stats.tsx:140-186): the full-screen page over the app —
//! the poster model ([`super::stats_page::stats_poster`]) painted on
//! the chrome bridge, centered like the ref's scrollbox
//! (`alignItems: center, justifyContent: center, minHeight: 100%`),
//! with the resource's two interim states (`Gathering your stats…`,
//! the error fallback).

use super::chrome_solid::{chrome_frame, chrome_run_units};
use super::stats_page::{Line, Poster, Run, RunStyle};
use crate::render::CellColor;
use crate::theme::ThemeStyles;

/// The poster's own background fill (theme.background.base — the
/// ref's page box paints it edge-to-edge).
fn page_bg(styles: &ThemeStyles) -> CellColor {
    CellColor::from(styles.background)
}

/// Map one model run to the frame's fg + attrs (bold = ECMA bit 0).
fn run_style(style: RunStyle, styles: &ThemeStyles) -> (CellColor, u16) {
    match style {
        RunStyle::Base => (CellColor::from(styles.text.fg.unwrap_or_default()), 0),
        RunStyle::Bold => (CellColor::from(styles.text.fg.unwrap_or_default()), 1),
        RunStyle::Muted => (CellColor::from(styles.text_muted.fg.unwrap_or_default()), 0),
        RunStyle::Shade(_) => (CellColor::from(styles.accent), 0),
    }
}

/// Paint one poster line at `(x, y)` — each run is one unit.
fn paint_line(units: &mut Vec<crate::components::chrome_solid::ChromeUnit>, line: &Line, x0: usize, y: usize, width: usize, styles: &ThemeStyles) {
    let runs: Vec<(usize, Run)> = match line {
        Line::Spacer => return,
        Line::Left(runs) => runs.iter().map(|run| (0, run.clone())).collect(),
        Line::Center(runs) => {
            let text_width: usize =
                runs.iter().map(|run| run.text.chars().count()).sum::<usize>() + runs.len().saturating_sub(1);
            let x = width.saturating_sub(text_width) / 2;
            let mut at = x;
            runs.iter()
                .map(|run| {
                    let offset = at;
                    at += run.text.chars().count() + 1;
                    (offset, run.clone())
                })
                .collect()
        }
        Line::Right(runs) => {
            let text_width: usize =
                runs.iter().map(|run| run.text.chars().count()).sum::<usize>() + runs.len().saturating_sub(1);
            let x = width.saturating_sub(text_width);
            let mut at = x;
            runs.iter()
                .map(|run| {
                    let offset = at;
                    at += run.text.chars().count() + 1;
                    (offset, run.clone())
                })
                .collect()
        }
        Line::At(runs) => runs.clone(),
    };
    for (offset, run) in runs {
        let (fg, attrs) = run_style(run.style, styles);
        units.extend(chrome_run_units(
            &format!("stats@{y}#{offset}"),
            x0 + offset,
            y,
            &run.text,
            fg,
            page_bg(styles),
            attrs,
        ));
    }
}

/// The full page: the loading line, the error fallback, or the
/// painted poster. `loaded` receives the poster model when the
/// resource resolved.
#[must_use]
pub fn render_stats_page_solid(
    phase: StatsPhaseView<'_>,
    styles: &ThemeStyles,
    size: (u16, u16),
) -> crate::render::Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let bg = page_bg(styles);
    match phase {
        StatsPhaseView::Loading => {
            let line = "Gathering your stats…";
            let x = width.saturating_sub(line.chars().count()) / 2;
            let y = height / 2;
            let units = chrome_run_units("stats.loading", x, y, line, CellColor::from(styles.text_muted.fg.unwrap_or_default()), bg, 0);
            chrome_frame("stats_page", width.max(1), height.max(1), bg, &units)
        }
        StatsPhaseView::Failed => {
            let line = "Could not load stats. Reopen /stats to try again.";
            let x = width.saturating_sub(line.chars().count()) / 2;
            let y = height / 2;
            let units = chrome_run_units("stats.error", x, y, line, CellColor::from(styles.error), bg, 0);
            chrome_frame("stats_page", width.max(1), height.max(1), bg, &units)
        }
        StatsPhaseView::Loaded(poster) => render_poster_solid(poster, styles, size),
    }
}

/// The painted poster: rows stacked with the model's own gap rows,
/// the block centered in the terminal (the scrollbox's centered
/// content; paddingTop/Bottom 1).
#[must_use]
pub fn render_poster_solid(poster: &Poster, styles: &ThemeStyles, size: (u16, u16)) -> crate::render::Frame {
    let (width, height) = (usize::from(size.0), usize::from(size.1));
    let bg = page_bg(styles);
    let content = poster.lines.len();
    let y0 = (height.saturating_sub(content) / 2).max(1);
    let x0 = width.saturating_sub(poster.width) / 2;
    let mut units = Vec::new();
    for (offset, line) in poster.lines.iter().enumerate() {
        paint_line(&mut units, line, x0, y0 + offset, poster.width, styles);
    }
    chrome_frame("stats_poster", width.max(1), height.max(1), bg, &units)
}

/// The page's three render states (the resource's shape,
/// stats.tsx:167-176) — the borrow-friendly view over the store's
/// phase.
pub enum StatsPhaseView<'a> {
    Loading,
    Failed,
    Loaded(&'a Poster),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeState;
    use oc_client::types::{
        SessionStatsActivity, SessionStatsInfo, SessionStatsInfoRange, SessionStatsTools,
        SessionStatsToolsVariant1, SessionStatsToolsVariant1Mode, TokenUsageInfo,
    };

    fn fixture() -> SessionStatsInfo {
        SessionStatsInfo {
            active_days: 120,
            activity: vec![
                SessionStatsActivity { date: "2026-01-05".to_owned(), steps: 4 },
                SessionStatsActivity { date: "2026-01-06".to_owned(), steps: 9 },
            ],
            cost: 1.25,
            models: Vec::new(),
            prompts: 300,
            range: SessionStatsInfoRange {
                from: 1_767_225_600_000.0, // 2026-01-01T00:00:00Z
                to: 1_772_323_200_000.0, // 2026-03-01T00:00:00Z
            },
            sessions: 42,
            steps: 300,
            streak: 7,
            subagents: 0,
            tokens: TokenUsageInfo {
                input: 1_100_000.0,
                output: 100_000.0,
                reasoning: 0.0,
                cache: oc_client::types::TokenUsageInfoCache { read: 0.0, write: 0.0 },
            },
            tools: SessionStatsTools::None(SessionStatsToolsVariant1 {
                mode: SessionStatsToolsVariant1Mode::None,
            }),
        }
    }

    /// The page paints the poster's rows (stats.tsx:156-178): the
    /// centered poster over the theme background, the header, the
    /// big TOKENS block, the calendar rows, the metrics, the
    /// sign-off.
    #[test]
    fn poster_page_paints_the_ref_rows() {
        let theme = ThemeState::v2_dark();
        let stats = fixture();
        let poster = super::super::stats_page::stats_poster(&stats, (120, 40));
        let frame = render_stats_page_solid(StatsPhaseView::Loaded(&poster), &theme.styles(), (120, 40));
        let grid = frame.text_grid();
        let all = grid.join("\n");
        assert!(all.contains("opencode / stats"), "the header title");
        assert!(all.contains("Jan \u{2013} Feb 2026"), "the range");
        assert!(all.contains("TOKENS"), "the big number's label");
        assert!(all.contains("M   "), "the Monday calendar row");
        assert!(all.contains("\u{25a0} "), "an active calendar cell");
        assert!(all.contains("\u{b7} "), "an empty calendar cell");
        assert!(all.contains("7 days"), "the streak metric");
        assert!(all.contains("active days"));
        assert!(all.contains("opencode.ai"), "the sign-off");
    }

    /// The two interim states (stats.tsx:167-176).
    #[test]
    fn page_paints_loading_and_error_states() {
        let theme = ThemeState::v2_dark();
        let loading =
            render_stats_page_solid(StatsPhaseView::Loading, &theme.styles(), (80, 24));
        assert!(loading.text_grid().join("\n").contains("Gathering your stats…"));
        let failed =
            render_stats_page_solid(StatsPhaseView::Failed, &theme.styles(), (80, 24));
        assert!(failed
            .text_grid()
            .join("\n")
            .contains("Could not load stats. Reopen /stats to try again."));
    }
}
