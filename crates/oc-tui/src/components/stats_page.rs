//! RE-14 — the `/stats` usage poster's MODEL (feature-plugins/
//! system/stats-data.ts + util/activity-calendar.ts): the pure math
//! the poster paints — the compact number formatter, the four
//! metrics, the big-digit bitmaps, the M-W-S activity calendar and
//! the poster's line layout (stats.tsx:30-137). The solid renderer
//! ([`super::stats_page_solid`]) paints this model; the layout is
//! the ref's, not a redesign.

use oc_client::types::{SessionStatsActivity, SessionStatsInfo, TokenUsageInfo};

/// The en-US short month names (`Intl.DateTimeFormat("en-US", {month:
/// "short"})` — the calendar's month labels).
const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The big-number digit bitmaps (stats.tsx:12-28): 5 rows per glyph,
/// `"1"` cells render as `██` and `"0"` as two spaces.
const DIGITS: &[(&str, [&str; 5])] = &[
    ("0", ["111", "101", "101", "101", "111"]),
    ("1", ["010", "110", "010", "010", "111"]),
    ("2", ["111", "001", "111", "100", "111"]),
    ("3", ["111", "001", "111", "001", "111"]),
    ("4", ["101", "101", "111", "001", "001"]),
    ("5", ["111", "100", "111", "001", "111"]),
    ("6", ["111", "100", "111", "101", "111"]),
    ("7", ["111", "001", "010", "010", "010"]),
    ("8", ["111", "101", "111", "101", "111"]),
    ("9", ["111", "101", "111", "001", "111"]),
    (".", ["0", "0", "0", "0", "1"]),
    ("K", ["101", "110", "100", "110", "101"]),
    ("M", ["10001", "11011", "10101", "10001", "10001"]),
    ("B", ["110", "101", "110", "101", "110"]),
    ("T", ["111", "010", "010", "010", "010"]),
];

/// `TokenUsage.total` (schema/token-usage.ts:16): input + output +
/// reasoning + cache read + cache write.
#[must_use]
pub fn token_total(tokens: &TokenUsageInfo) -> f64 {
    tokens.input + tokens.output + tokens.reasoning + tokens.cache.read + tokens.cache.write
}

/// `statsNumber` (stats-data.ts:16-18): `Intl.NumberFormat("en-US",
/// { notation: "compact", maximumFractionDigits: 1 })` — the tier
/// bump is the formatter's own rounding (`999,999 → "1M"`, not
/// `1000K`).
#[must_use]
pub fn stats_number(value: f64) -> String {
    if !value.is_finite() || value < 0.0 {
        return "0".to_owned();
    }
    // The tier walk: divide while the mantissa still rounds up into
    // the next group (999.95 · 10³ⁿ is the 1-decimal bump edge).
    const UNITS: [&str; 4] = ["K", "M", "B", "T"];
    let mut scaled = value;
    let mut unit = "";
    for candidate in UNITS {
        if scaled < 999.95 {
            break;
        }
        scaled /= 1000.0;
        unit = candidate;
    }
    let rounded = (scaled * 10.0).round() / 10.0;
    let mut text = if (rounded - rounded.round()).abs() < 1e-9 {
        format!("{}", rounded.round() as i64)
    } else {
        format!("{rounded:.1}")
    };
    text.push_str(unit);
    text
}

/// `statsMetrics` (stats-data.ts:4-14): tokens first (the poster's
/// big number), then the three footer metrics.
#[must_use]
pub fn stats_metrics(stats: &SessionStatsInfo) -> [(&'static str, f64); 4] {
    [
        ("tokens", token_total(&stats.tokens)),
        ("best streak", stats.streak as f64),
        ("active days", stats.active_days as f64),
        ("sessions", stats.sessions as f64),
    ]
}

/// Days-from-epoch → `(year, month 1-12, day 1-31)` (Howard Hinnant's
/// `civil_from_days` — the UTC ordinals the ref derives through
/// `Date`/`toISOString`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// `(year, month, day)` → days from epoch (`days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The ISO date of an epoch-day ordinal (`toISOString().slice(0,10)`
/// — the calendar's cell keys).
#[must_use]
pub fn iso_date(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The UTC weekday of an epoch-day ordinal (`getUTCDay`: 0 = Sunday).
fn weekday(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

/// One calendar cell: the activity date key, its steps and the shade
/// level (−1 outside the range, 0 empty, 1-4 scaled).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayCell {
    pub date: String,
    pub steps: i64,
    pub level: i32,
}

/// One month label's anchor: the week it starts at and the columns it
/// spans (`span * 2` cells).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthLabel {
    pub week: usize,
    pub label: String,
    pub span: usize,
}

/// `activityCalendar` (util/activity-calendar.ts): the M-W-S grid —
/// Monday-first weeks, the last `min(53, maxWeeks)` weeks of the
/// range, levels scaled over the DISTINCT positive step counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityCalendarView {
    /// `weeks[week][day]` — day 0 is Monday, 6 is Sunday.
    pub weeks: Vec<Vec<DayCell>>,
    pub months: Vec<MonthLabel>,
    /// The range exceeded the visible window (`count < total`).
    pub clipped: bool,
}

#[must_use]
pub fn activity_calendar(
    activity: &[SessionStatsActivity],
    from_ms: f64,
    to_ms: f64,
    max_weeks: usize,
) -> ActivityCalendarView {
    // The ref extracts LOCAL dates then works in UTC ordinals so DST
    // cannot add or drop cells; the port's inputs (the server's
    // range + date keys) are already day-aligned, so the ordinals are
    // the UTC day numbers of the range endpoints.
    let ord = |ms: f64| (ms / 86_400_000.0).floor() as i64;
    let from = ord(from_ms);
    let to = ord(to_ms - 1.0);
    let monday = from - (weekday(from) + 6).rem_euclid(7);
    let total = if to_ms <= from_ms { 0 } else { (to - monday).div_euclid(7) + 1 };
    let count = total.min(53).min(i64::try_from(max_weeks.max(1)).unwrap_or(1));
    let start = monday + (total - count).max(0) * 7;
    let values: std::collections::HashMap<&str, i64> =
        activity.iter().map(|day| (day.date.as_str(), day.steps)).collect();
    let mut levels: Vec<i64> =
        values.values().copied().filter(|steps| *steps > 0).collect();
    levels.sort_unstable();
    levels.dedup();
    let weeks = (0..count)
        .map(|week| {
            (0..7)
                .map(|day| {
                    let date = start + week * 7 + day;
                    let key = iso_date(date);
                    let steps = values.get(key.as_str()).copied().unwrap_or(0);
                    let level = if date < from || date > to {
                        -1
                    } else if steps == 0 {
                        0
                    } else {
                        let index = levels.iter().position(|value| *value == steps).map_or(0, |index: usize| index as i64);
                        let len = i64::try_from(levels.len()).unwrap_or(1).max(1);
                        (((index + 1) as f64 / len as f64) * 4.0).ceil().max(1.0) as i32
                    };
                    DayCell { date: key, steps, level }
                })
                .collect()
        })
        .collect();
    // Thursday (start + week·7 + 3) identifies the month owning a
    // week; the edges clamp inside the range, consecutive duplicates
    // collapse (activity-calendar.ts:41-46).
    let mut anchors: Vec<(usize, &str)> = Vec::new();
    for week in 0..count {
        let thursday = start + week * 7 + 3;
        let day = thursday.min(to).max(from);
        let (_, month, _) = civil_from_days(day);
        let label = MONTHS[(month - 1) as usize];
        if anchors.last().is_none_or(|(_, previous)| *previous != label) {
            anchors.push((usize::try_from(week).unwrap_or(0), label));
        }
    }
    let months = anchors
        .iter()
        .enumerate()
        .map(|(index, (week, label))| MonthLabel {
            week: *week,
            label: (*label).to_owned(),
            span: anchors.get(index + 1).map_or_else(
                || usize::try_from(count).unwrap_or(0) - *week,
                |(next, _)| *next - *week,
            ),
        })
        .collect();
    ActivityCalendarView { weeks, months, clipped: count < total }
}

/// The poster's line styles (theme-mapped by the renderer; the four
/// `Shade` indexes are the calendar ramp: muted + the four tints of
/// stats.tsx:54-59).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStyle {
    Base,
    Muted,
    Bold,
    Shade(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

impl Run {
    fn new(text: impl Into<String>, style: RunStyle) -> Self {
        Self { text: text.into(), style }
    }
}

/// One poster row: placed left/center/right at the poster width, or
/// at explicit columns (the calendar rows and the metrics row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Left(Vec<Run>),
    Center(Vec<Run>),
    Right(Vec<Run>),
    At(Vec<(usize, Run)>),
    /// The flex `gap` between the poster's children (1 compact, 2
    /// otherwise) — an empty spacer row.
    Spacer,
}

/// The poster model: the clamped poster width and the rows top to
/// bottom (stats.tsx:61-136's box tree flattened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poster {
    pub width: usize,
    pub lines: Vec<Line>,
}

/// The compact number's big-glyph rows (stats.tsx:81-91): one row per
/// bitmap line, `'1'` → `██`, `'0'` → two spaces, glyphs joined by
/// two spaces. `None` when any character lacks a bitmap.
#[must_use]
pub fn big_number_rows(number: &str) -> Option<Vec<String>> {
    let glyphs: Vec<&[&str; 5]> =
        number.chars().map(|ch| DIGITS.iter().find(|(key, _)| *key == ch.encode_utf8(&mut [0; 4])).map(|(_, rows)| rows)).collect::<Option<Vec<_>>>()?;
    Some((0..5)
        .map(|row| {
            glyphs
                .iter()
                .map(|glyph| {
                    glyph[row].replace('1', "\u{2588}\u{2588}").replace('0', "  ")
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect())
}

/// The big glyph block's display width (`sum(char_width·2 + 2) − 2`,
/// the ref's `large()` reduction).
fn big_number_width(number: &str) -> Option<usize> {
    let glyphs: Vec<usize> = number
        .chars()
        .map(|ch| {
            DIGITS.iter().find(|(key, _)| *key == ch.encode_utf8(&mut [0; 4])).map(|(_, rows)| rows[0].chars().count() * 2)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(glyphs.iter().map(|width| width + 2).sum::<usize>().saturating_sub(2))
}

/// The header's date range (`Intl.DateTimeFormat("en-US", {month:
/// "short", year: "numeric"}).formatRange`): same month "Jan 2025",
/// same year "Jan – Dec 2025", else "Jan 2025 – Oct 2026".
#[must_use]
pub fn range_label(from_ms: f64, to_ms: f64) -> String {
    let label = |ms: f64| {
        let days = (ms / 86_400_000.0).floor() as i64;
        let (year, month, _) = civil_from_days(days);
        (year, month)
    };
    let (from_year, from_month) = label(from_ms);
    let (to_year, to_month) = label((to_ms - 1.0).max(from_ms));
    let from = format!("{} {from_year}", MONTHS[(from_month - 1) as usize]);
    let to = format!("{} {to_year}", MONTHS[(to_month - 1) as usize]);
    if (from_year, from_month) == (to_year, to_month) {
        from
    } else if from_year == to_year {
        format!("{} \u{2013} {}", MONTHS[(from_month - 1) as usize], to)
    } else {
        format!("{from} \u{2013} {to}")
    }
}

/// The local timezone name (`Intl.DateTimeFormat().resolvedOptions()
/// .timeZone || "UTC"`): `TZ` when shaped, else the `/etc/localtime`
/// symlink's zoneinfo path, else UTC.
#[must_use]
pub fn local_timezone_name() -> String {
    if let Ok(tz) = std::env::var("TZ") {
        if !tz.is_empty() {
            return tz;
        }
    }
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let text = link.to_string_lossy();
        if let Some((_, zone)) = text.split_once("zoneinfo/") {
            let zone = zone.trim_matches('/');
            if !zone.is_empty() {
                return zone.to_owned();
            }
        }
    }
    "UTC".to_owned()
}

/// The stats fetch's `from` (stats.tsx:143): local January 1st of the
/// current year, in epoch ms.
#[must_use]
pub fn year_start_ms(now_ms: i64) -> i64 {
    let (year, _, _) = civil_from_days(now_ms.div_euclid(86_400_000));
    days_from_civil(year, 1, 1) * 86_400_000
}

/// The wall clock in epoch ms (the dispatch path's fetch input).
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(0))
        .unwrap_or(0)
}

/// The calendar's month-label row (stats.tsx:96-101): the 4-space
/// day-label gutter, each label dropped when it cannot fit its span.
#[must_use]
fn month_label_row(calendar: &ActivityCalendarView) -> String {
    let mut row = String::from("    ");
    for month in &calendar.months {
        let span = month.span * 2;
        let label =
            if month.label.chars().count() <= span { month.label.as_str() } else { "" };
        row.push_str(&format!("{label:<span$}"));
    }
    row
}

/// `StatsPoster`'s layout (stats.tsx:30-137) as the line model the
/// solid renderer paints. `size` is the terminal size.
#[must_use]
pub fn stats_poster(stats: &SessionStatsInfo, size: (u16, u16)) -> Poster {
    let width = (usize::from(size.0).saturating_sub(8)).clamp(12, 110);
    let compact = size.1 < 38;
    let _gap = if compact { 1 } else { 2 };
    let metrics = stats_metrics(stats);
    let number = stats_number(metrics[0].1);
    let calendar =
        activity_calendar(&stats.activity, stats.range.from, stats.range.to, (width - 4) / 2);
    let mut lines: Vec<Line> = Vec::new();
    macro_rules! spacer { () => { lines.push(Line::Spacer) }; }
    // The header (stats.tsx:63-68): title bold left, range right — a
    // column below 44 cols (the children center in the narrow box).
    let dates = range_label(stats.range.from, stats.range.to);
    if width >= 44 {
        lines.push(Line::Left(vec![Run::new("opencode / stats", RunStyle::Bold)]));
        lines.push(Line::Right(vec![Run::new(dates, RunStyle::Muted)]));
    } else {
        lines.push(Line::Center(vec![Run::new("opencode / stats", RunStyle::Bold)]));
        lines.push(Line::Center(vec![Run::new(dates, RunStyle::Muted)]));
    }
    spacer!();
    // The wordmark (stats.tsx:69-71): the full halves ≥ 44 cols, the
    // sliced pair ≥ 22, the go mark under (logo.tsx:26-33). The port
    // renders the SAME art on home (home.rs LOGO_*) — those display
    // strings are reused: the left half muted, the right bold.
    if !compact {
        const GO_RIGHT: [&str; 3] = ["\u{2588}\u{2580}\u{2580}\u{2588}", "\u{2588}__\u{2588}", "\u{2580}\u{2580}\u{2580}\u{2580}"];
        if width >= 44 {
            for (left, right) in crate::components::home::logo_left()
                .iter()
                .zip(crate::components::home::logo_right())
            {
                lines.push(Line::Left(vec![
                    Run::new(*left, RunStyle::Muted),
                    Run::new(" ", RunStyle::Muted),
                    Run::new(*right, RunStyle::Bold),
                ]));
            }
        } else if width >= 22 {
            for (left, right) in crate::components::home::logo_left()
                .iter()
                .skip(1)
                .zip(crate::components::home::logo_right())
            {
                lines.push(Line::Left(vec![
                    Run::new(*left, RunStyle::Muted),
                    Run::new(" ", RunStyle::Muted),
                    Run::new(*right, RunStyle::Bold),
                ]));
            }
        } else {
            for row in GO_RIGHT.iter().skip(1) {
                lines.push(Line::Left(vec![Run::new(*row, RunStyle::Bold)]));
            }
        }
        spacer!();
    }
    // The big TOKENS number (stats.tsx:72-94): the glyph block when
    // every character has a bitmap AND it fits, else the bold string.
    let big = big_number_rows(&number).filter(|_rows| {
        big_number_width(&number).is_some_and(|block| block + 1 + 6 <= width)
    });
    match (&big, big_number_width(&number)) {
        (Some(rows), Some(block)) => {
            let x = (width - (block + 1 + 6)) / 2;
            for (row, text) in rows.iter().enumerate() {
                let mut runs = vec![(x, Run::new(text.clone(), RunStyle::Base))];
                // The TOKENS label centers vertically beside the
                // block (`alignItems: center` on the row box).
                if row == 2 {
                    runs.push((x + block + 1, Run::new("TOKENS", RunStyle::Muted)));
                }
                lines.push(Line::At(runs));
            }
        }
        _ => {
            lines.push(Line::Center(vec![
                Run::new(number, RunStyle::Bold),
                Run::new(" TOKENS", RunStyle::Muted),
            ]));
        }
    }
    spacer!();
    // The activity calendar (stats.tsx:95-119): the month labels over
    // the seven M-W-S rows, one 2-col cell per week.
    let row_width = 4 + 4 + calendar.weeks.len() * 2;
    let x = width.saturating_sub(row_width) / 2;
    lines.push(Line::At(vec![(x, Run::new(month_label_row(&calendar), RunStyle::Muted))]));
    for (day, letter) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        let mut row = vec![(x, Run::new(format!("{letter}   "), RunStyle::Muted))];
        for (index, week) in calendar.weeks.iter().enumerate() {
            let cell = &week[day];
            let (glyph, style) = if cell.level < 0 {
                ("  ", RunStyle::Muted)
            } else if cell.level == 0 {
                ("\u{b7} ", RunStyle::Shade(0))
            } else {
                ("\u{25a0} ", RunStyle::Shade(cell.level.min(4) as usize))
            };
            row.push((x + 8 + index * 2, Run::new(glyph, style)));
        }
        lines.push(Line::At(row));
    }
    if calendar.clipped {
        lines.push(Line::Center(vec![
            Run::new(format!("Your last {} weeks", calendar.weeks.len()), RunStyle::Muted),
        ]));
    }
    spacer!();
    // The three footer metrics (stats.tsx:120-132): value bold over
    // its label, the boxes spread `space-around` across the width.
    let entries: Vec<(String, &str)> = metrics[1..]
        .iter()
        .map(|(label, value)| {
            (format!("{}{}", stats_number(*value), if *label == "best streak" { " days" } else { "" }), *label)
        })
        .collect();
    let slot = width / 3;
    let mut value_runs = Vec::new();
    let mut label_runs = Vec::new();
    for (index, (value, label)) in entries.iter().enumerate() {
        let center = slot * index + slot / 2;
        let vx = center.saturating_sub(value.chars().count() / 2);
        let lx = center.saturating_sub(label.chars().count() / 2);
        value_runs.push((vx, Run::new(value, RunStyle::Bold)));
        label_runs.push((lx, Run::new(*label, RunStyle::Muted)));
    }
    lines.push(Line::At(value_runs));
    lines.push(Line::At(label_runs));
    spacer!();
    // The sign-off (stats.tsx:133-135).
    lines.push(Line::Right(vec![Run::new("opencode.ai", RunStyle::Base)]));
    Poster { width, lines }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oc_client::types::{
        SessionStatsActivity, SessionStatsInfo, SessionStatsInfoRange, SessionStatsTools,
        SessionStatsToolsVariant1, SessionStatsToolsVariant1Mode, TokenUsageInfo,
    };

    fn activity(date: &str, steps: i64) -> SessionStatsActivity {
        SessionStatsActivity { date: date.to_owned(), steps }
    }

    /// `2026-01-01T00:00:00Z` in epoch ms — a Thursday.
    fn ms(year: i64, month: u32, day: u32) -> f64 {
        (days_from_civil(year, month, day) * 86_400_000) as f64
    }

    /// The ref's number formatter anchors (Intl compact, 1 fraction).
    #[test]
    fn stats_number_matches_the_compact_formatter() {
        assert_eq!(stats_number(0.0), "0");
        assert_eq!(stats_number(842.0), "842");
        assert_eq!(stats_number(1000.0), "1K");
        assert_eq!(stats_number(1234.0), "1.2K");
        assert_eq!(stats_number(950_000.0), "950K");
        // The rounding BUMP: 999,999 formats as 1M, not 1000K.
        assert_eq!(stats_number(999_999.0), "1M");
        assert_eq!(stats_number(1_234_567.0), "1.2M");
        assert_eq!(stats_number(2_500_000_000.0), "2.5B");
        assert_eq!(stats_number(1.2e12), "1.2T");
    }

    /// `TokenUsage.total` sums all five counters (token-usage.ts:16).
    #[test]
    fn token_total_sums_every_counter() {
        let tokens = TokenUsageInfo {
            input: 100.0,
            output: 20.0,
            reasoning: 3.0,
            cache: oc_client::types::TokenUsageInfoCache { read: 1000.0, write: 77.0 },
        };
        assert_eq!(token_total(&tokens), 1200.0);
    }

    /// The calendar's date→cell mapping against the ref math
    /// (activity-calendar.ts): Monday-first weeks (day 0 is Monday,
    /// 6 Sunday), out-of-range edge cells level −1, the distinct-step
    /// level scaling, and the clipping window.
    #[test]
    fn calendar_maps_dates_to_monday_first_cells() {
        // Range 2026-01-01 (Thursday) .. 2026-01-15: the grid starts
        // Monday 2025-12-29 — two lead cells sit before the range.
        let calendar = activity_calendar(
            &[
                activity("2026-01-01", 10), // Thursday of week 0
                activity("2026-01-02", 30), // Friday of week 0
                activity("2026-01-05", 10), // Monday of week 1
            ],
            ms(2026, 1, 1),
            ms(2026, 1, 15),
            53,
        );
        assert_eq!(calendar.weeks.len(), 3, "12-29 .. 01-11 is 3 weeks incl. the lead");
        // Week 0 starts Monday 2025-12-29; its Thursday (day 3) is
        // the range's first day.
        assert_eq!(calendar.weeks[0][0].date, "2025-12-29");
        assert_eq!(calendar.weeks[0][0].level, -1, "before the range → -1");
        assert_eq!(calendar.weeks[0][3].date, "2026-01-01");
        assert_eq!(calendar.weeks[0][3].steps, 10);
        // ceil((0+1)/2 · 4) = 2 — the levels scale over the
        // DISTINCT counts, not the raw steps.
        assert_eq!(calendar.weeks[0][3].level, 2);
        assert_eq!(calendar.weeks[0][4].date, "2026-01-02");
        // ceil((index+1)/len * 4): 30 is index 1 of 2 → ceil(4) = 4.
        assert_eq!(calendar.weeks[0][4].level, 4);
        // Monday of week 1.
        assert_eq!(calendar.weeks[1][0].date, "2026-01-05");
        assert_eq!(calendar.weeks[1][0].level, 2, "steps 10 → the same level as 01-01");
        // A zero-activity weekday inside the range is level 0.
        assert_eq!(calendar.weeks[1][1].date, "2026-01-06");
        assert_eq!(calendar.weeks[1][1].level, 0);
        assert!(!calendar.clipped, "the whole range fits 53 weeks");
        // The Thursday anchors: week 0's Thursday is Jan (in-range),
        // the labels collapse to one.
        assert_eq!(calendar.months.len(), 1);
        assert_eq!(calendar.months[0].label, "Jan");
        assert_eq!(calendar.months[0].span, 3);
    }

    /// `maxWeeks` clips the WINDOW to the last weeks of the range
    /// (activity-calendar.ts:10-12) and flags `clipped`.
    #[test]
    fn calendar_clips_to_max_weeks_from_the_end() {
        let calendar = activity_calendar(
            &[activity("2026-01-01", 5)],
            ms(2026, 1, 1),
            ms(2026, 3, 1),
            2,
        );
        assert!(calendar.clipped);
        assert_eq!(calendar.weeks.len(), 2);
        // The last week covers 2026-02-23 (Monday) ..
        assert_eq!(calendar.weeks[1][0].date, "2026-02-23");
        // .. 2026-03-01 (Sunday) — the range's final day.
        assert_eq!(calendar.weeks[1][6].date, "2026-03-01");
        // `to` is exclusive: ordinal(to - 1) is Feb 28, so the final
        // Sunday sits one day past the range → -1.
        assert_eq!(calendar.weeks[1][6].level, -1);
    }

    /// The big-number bitmaps (stats.tsx:12-28, 81-91).
    #[test]
    fn big_number_rows_lay_the_bitmaps_out() {
        let rows = big_number_rows("1").expect("digits");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0], "  ██  ", "\"010\" → two spaces, ██, two spaces");
        assert!(big_number_rows("7").is_some());
        assert!(big_number_rows("X").is_none(), "no bitmap → fallback");
        // "1.2M": every glyph column renders 2 cells wide (even the
        // 1-column '.' bitmap), joined by two spaces → 8+4+8+12-2.
        assert_eq!(big_number_width("1.2M"), Some(30));
    }

    /// The header's `formatRange` shapes (en-US, month short + year).
    #[test]
    fn range_label_matches_intl_format_range() {
        assert_eq!(range_label(ms(2026, 1, 10), ms(2026, 1, 20)), "Jan 2026");
        assert_eq!(range_label(ms(2026, 1, 1), ms(2026, 12, 31)), "Jan \u{2013} Dec 2026");
        assert_eq!(range_label(ms(2025, 11, 4), ms(2026, 2, 9)), "Nov 2025 \u{2013} Feb 2026");
    }

    fn fixture() -> SessionStatsInfo {
        SessionStatsInfo {
            active_days: 120,
            activity: vec![
                activity("2026-01-05", 4),
                activity("2026-01-06", 9),
                activity("2026-01-07", 2),
            ],
            cost: 1.25,
            models: Vec::new(),
            prompts: 300,
            range: SessionStatsInfoRange { from: ms(2026, 1, 1), to: ms(2026, 3, 1) },
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

    /// The poster layout (stats.tsx:61-136): width clamp, the
    /// header/logo/big-number/calendar/metrics/footer order, the
    /// M-W-S day rows and the metrics' " days" suffix.
    #[test]
    fn poster_lays_out_the_ref_rows() {
        let poster = stats_poster(&fixture(), (120, 40));
        assert_eq!(poster.width, 110, "min(110, 120-8)");
        let lefts: Vec<&Line> = poster.lines.iter().collect();
        // Header: bold title, muted range (same row pair).
        assert!(matches!(&lefts[0], Line::Left(runs)
            if runs[0].text == "opencode / stats" && runs[0].style == RunStyle::Bold));
        assert!(matches!(&lefts[1], Line::Right(runs) if runs[0].text == "Jan \u{2013} Feb 2026"));
        // Non-compact: the 4-row logo follows the spacer.
        assert!(matches!(lefts[2], Line::Spacer));
        assert!(matches!(&lefts[3], Line::Left(runs)
            if runs.last().is_some_and(|run| run.style == RunStyle::Bold)
            && runs[0].style == RunStyle::Muted));
        // The big "1.2M TOKENS" block (fits 110 cols).
        let big = poster.lines.iter().position(|line| matches!(
            line,
            Line::At(runs) if runs.iter().any(|(_, run)| run.text == "TOKENS")
        ));
        let big = big.expect("the TOKENS label rides the big block's middle row");
        let Line::At(runs) = &poster.lines[big] else { unreachable!() };
        assert!(runs.iter().any(|(x, run)| *x > 0 && run.text.contains('\u{2588}')),
            "the glyph block paints ██ cells");
        // The calendar: the month-label row, then the M-W-S rows —
        // Monday's row holds one cell per week, first cell level 1.
        let monday = poster
            .lines
            .iter()
            .find_map(|line| match line {
                Line::At(runs) if runs.first().is_some_and(|(_, run)| run.text == "M   ") => {
                    Some(runs)
                }
                _ => None,
            })
            .expect("the Monday row");
        assert!(monday.iter().any(|(_, run)| run.text == "\u{25a0} " && run.style == RunStyle::Shade(3)),
            "2026-01-05 (Monday, steps 4 → level 3): {monday:?}");
        // Metrics: "7 days" bold over "best streak" muted, then
        // "120"/"active days", "42"/"sessions".
        let flat: Vec<&Run> = poster.lines.iter().flat_map(|line| match line {
            Line::At(runs) => runs.iter().map(|(_, run)| run).collect(),
            _ => Vec::new(),
        }).collect();
        assert!(flat.iter().any(|run| run.text == "7 days" && run.style == RunStyle::Bold));
        assert!(flat.iter().any(|run| run.text == "best streak" && run.style == RunStyle::Muted));
        assert!(flat.iter().any(|run| run.text == "120" && run.style == RunStyle::Bold));
        assert!(flat.iter().any(|run| run.text == "sessions"));
        // The sign-off.
        assert!(matches!(poster.lines.last(), Some(Line::Right(runs)) if runs[0].text == "opencode.ai"));
    }

    /// The compact/narrow shapes: no logo under 38 rows, the inline
    /// number fallback when the glyphs cannot fit.
    #[test]
    fn poster_compact_and_narrow_shapes() {
        let compact = stats_poster(&fixture(), (120, 20));
        assert!(!compact
            .lines
            .iter()
            .any(|line| matches!(line, Line::Left(runs) if runs.len() == 3)),
            "no logo rows when compact");
        let narrow = stats_poster(&fixture(), (16, 40));
        assert_eq!(narrow.width, 12);
        assert!(narrow.lines.iter().any(|line| matches!(
            line,
            Line::Center(runs) if runs.iter().any(|run| run.text == "1.2M")
        )), "the glyph block cannot fit 12 cols → the bold fallback");
    }

    /// The fetch inputs (stats.tsx:143-146): local January 1st + the
    /// timezone name.
    #[test]
    fn year_start_is_local_january_first() {
        // Mid-year: the same year's Jan 1.
        let mid = days_from_civil(2026, 7, 4) * 86_400_000 + 13 * 3_600_000;
        assert_eq!(year_start_ms(mid), days_from_civil(2026, 1, 1) * 86_400_000);
    }
}
