//! `opencode stats` (handlers/stats.ts) — the usage dashboard: the
//! painted activity calendar (grid + legend + COLORFGBG palette), the
//! local-midnight range math, and the COST & TOKENS / MODELS / TOOL
//! RELIABILITY tables (G-155 restored the calendar, the local
//! day-boundary math, the ANSI colors, and en-US number grouping).

use std::io::IsTerminal as _;

use serde_json::Value;

use crate::cli::ServerParams;
use crate::handlers::connect;
use crate::util::error_message;

#[allow(clippy::struct_excessive_bools)]
pub struct Flags {
    pub days: Option<u32>,
    pub year: Option<u32>,
    pub all: bool,
    pub project: Option<String>,
    pub models: bool,
    pub tools: bool,
    pub cost: bool,
    pub full: bool,
    pub limit: u32,
    pub json: bool,
}

#[allow(clippy::cast_possible_wrap)]
pub async fn run(flags: &Flags, server: &ServerParams) -> i32 {
    let Flags { days, year, all, project, models, tools, cost, full, limit, json } = flags;
    if usize::from(days.is_some()) + usize::from(year.is_some()) + usize::from(*all) > 1 {
        eprintln!("Error: --days, --year, and --all cannot be combined");
        return 1;
    }
    let resolved = match connect(server, None).await {
        Ok(resolved) => resolved,
        Err(code) => return code,
    };
    let client = resolved.endpoint.client();

    let now_ms = crate::util::now_ms() as i64;
    let zone = local_zone();
    let (from, to, label) = stats_range(*days, *year, *all, now_ms, &zone);
    let project_id = if project.as_deref() == Some(".") {
        let directory =
            std::env::current_dir().map(|path| path.display().to_string()).unwrap_or_default();
        match client
            .location()
            .get(&oc_client::types::LocationGetQuery {
                location: Some(oc_client::types::QueryLocation { directory: Some(directory) }),
            })
            .await
        {
            Ok(location) => Some(location.project.id),
            Err(error) => {
                eprintln!("{}", error_message(&error.to_string()));
                return 1;
            }
        }
    } else {
        project.clone()
    };
    let details = *models || *tools || *cost || *full;
    let tools_mode = if *json || *tools || *full {
        "detail"
    } else if details {
        "none"
    } else {
        "summary"
    };
    let stats = match client
        .session()
        .stats(&oc_client::types::ExperimentalSessionStatsQuery {
            from: from.map(|from| from.to_string()),
            to: Some(to.to_string()),
            project: project_id,
            // `Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC"`
            // (stats.ts:30): the IANA name of the local zone.
            timezone: Some(local_timezone(&zone)),
            tools: Some(match tools_mode {
                "detail" => oc_client::types::ExperimentalSessionStatsQueryTools::Detail,
                "none" => oc_client::types::ExperimentalSessionStatsQueryTools::None,
                _ => oc_client::types::ExperimentalSessionStatsQueryTools::Summary,
            }),
        })
        .await
    {
        Ok(stats) => stats,
        Err(error) => {
            eprintln!("{}", error_message(&error.to_string()));
            return 1;
        }
    };
    let stats = serde_json::to_value(&stats).expect("stats serializes");
    if *json {
        println!("{}", serde_json::to_string_pretty(&stats).expect("serializes"));
        return 0;
    }
    let scope = if project.is_none() {
        "all projects"
    } else if project.as_deref() == Some(".") {
        "current project"
    } else {
        "selected project"
    };
    // `color: process.stdout.isTTY && NO_COLOR === undefined` and
    // `width: process.stdout.columns ?? 80` (stats.ts:47-48).
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let width = stdout_columns();
    print!(
        "{}",
        render(
            &stats,
            &label,
            scope,
            *models || *full,
            *tools || *full,
            *cost || *full,
            *limit,
            color,
            width
        )
    );
    0
}

// ===========================================================================
// Timezone + terminal context
// ===========================================================================

/// The process-local zone: `$TZ` when it names a valid IANA/POSIX zone,
/// else the system zone (jiff resolves `/etc/localtime`; G-155 — the
/// day buckets follow the local wall clock like ref's `new Date(...)`
/// locals, not UTC epoch math).
fn local_zone() -> jiff::tz::TimeZone {
    std::env::var("TZ")
        .ok()
        .and_then(|tz| jiff::tz::TimeZone::get(&tz).ok())
        .unwrap_or_else(jiff::tz::TimeZone::system)
}

/// The IANA name ref sends to the server
/// (`Intl.DateTimeFormat().resolvedOptions().timeZone`): `$TZ` when it
/// is an IANA name, else the `/etc/localtime` symlink target, else
/// `"UTC"`.
fn local_timezone(zone: &jiff::tz::TimeZone) -> String {
    if let Ok(tz) = std::env::var("TZ") {
        if jiff::tz::db().get(&tz).is_ok() {
            return tz;
        }
    }
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let text = link.display().to_string();
        if let Some(index) = text.find("zoneinfo/") {
            return text[index + "zoneinfo/".len()..].to_owned();
        }
    }
    let _ = zone;
    "UTC".to_owned()
}

/// `process.stdout.columns ?? 80` — the terminal width via
/// `TIOCGWINSZ` on stdout, falling back to 80.
#[allow(clippy::cast_possible_truncation)]
fn stdout_columns() -> i64 {
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }
    let mut size = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: a winsize out-param ioctl on the stdout fd.
    if unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut size) } == 0 && size.ws_col > 0 {
        i64::from(size.ws_col)
    } else {
        80
    }
}

// ===========================================================================
// Range math (`statsRange`, stats.ts:141-163) — local-midnight buckets
// ===========================================================================

/// Local midnight of the given instant in `zone`.
fn local_midnight(millis: i64, zone: &jiff::tz::TimeZone) -> Option<i64> {
    Some(
        jiff::Timestamp::from_millisecond(millis)
            .ok()?
            .to_zoned(zone.clone())
            .start_of_day()
            .ok()?
            .timestamp()
            .as_millisecond(),
    )
}

/// `statsRange`: local-midnight bucketing (G-155: the old UTC
/// `rem_euclid(86_400_000)` math shifted day-bucket starts in non-UTC
/// zones).
fn stats_range(
    days: Option<u32>,
    year: Option<u32>,
    all: bool,
    now_ms: i64,
    zone: &jiff::tz::TimeZone,
) -> (Option<i64>, i64, String) {
    let to = now_ms + 1;
    if all {
        return (None, to, "all time".to_owned());
    }
    if let Some(days) = days {
        // `new Date(y, m, d)` then `setDate(getDate() - (days - 1))`:
        // calendar-day arithmetic from local midnight.
        let today_start =
            local_midnight(now_ms, zone).unwrap_or(now_ms - now_ms.rem_euclid(86_400_000));
        let from = civil_shift_days(today_start, zone, -i64::from(days.saturating_sub(1)));
        let label =
            if days == 0 || days == 1 { "today".to_owned() } else { format!("last {days} days") };
        return (Some(from), to, label);
    }
    let now_year = jiff::Timestamp::from_millisecond(now_ms)
        .ok()
        .map(|instant| i64::from(instant.to_zoned(zone.clone()).year()))
        .unwrap_or(2026);
    let year = i64::from(year.unwrap_or(u32::try_from(now_year).unwrap_or(2026)));
    let from = local_january_first(year, zone);
    let next_year = local_january_first(year + 1, zone);
    let label = if from <= now_ms && now_ms < next_year {
        format!("{year} so far")
    } else {
        year.to_string()
    };
    let to = if from <= now_ms && now_ms < next_year { to } else { next_year };
    (Some(from), to, label)
}

/// Local midnight `shift` calendar days away from the local midnight of
/// `millis` (`Date#setDate` semantics).
fn civil_shift_days(millis: i64, zone: &jiff::tz::TimeZone, shift: i64) -> i64 {
    let Some(zoned) = (|| {
        let zoned = jiff::Timestamp::from_millisecond(millis).ok()?.to_zoned(zone.clone());
        let date = zoned.date().checked_add(jiff::Span::new().days(shift)).ok()?;
        date.at(0, 0, 0, 0).to_zoned(zone.clone()).ok()
    })() else {
        return millis + shift * 86_400_000;
    };
    zoned.timestamp().as_millisecond()
}

/// `new Date(year, 0, 1)` — local midnight of January 1st.
fn local_january_first(year: i64, zone: &jiff::tz::TimeZone) -> i64 {
    jiff::civil::date(i16::try_from(year).unwrap_or(2026), 1, 1)
        .at(0, 0, 0, 0)
        .to_zoned(zone.clone())
        .map(|zoned| zoned.timestamp().as_millisecond())
        .unwrap_or_else(|_| unix_ms_from_civil(year, 1, 1))
}

fn unix_ms_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400_000
}

/// Inverse days-since-epoch → `(year, month, day)` (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ===========================================================================
// Terminal palette + styling (stats.ts:93,100,319-342)
// ===========================================================================

/// `terminalPalette()` (stats.ts:325-342): COLORFGBG-based light/dark
/// detection with truecolor palettes, cyan fallback.
#[derive(Clone, Copy)]
struct Palette {
    /// The SGR params of the primary accent (`38;2;r;g;b` or `36`).
    primary: &'static str,
    /// The four activity-level colors.
    activity: [&'static str; 4],
}

fn terminal_palette() -> Palette {
    // `Number(process.env.COLORFGBG?.split(";").at(-1))` — the last
    // field is the background color index; `>= 7` means a light one.
    let background = std::env::var("COLORFGBG")
        .ok()
        .and_then(|value| value.split(';').next_back().and_then(|last| last.parse::<i64>().ok()));
    match background {
        Some(bg) if bg >= 7 => Palette {
            primary: "38;2;59;125;216",
            activity: [
                "38;2;153;169;192",
                "38;2;122;155;200",
                "38;2;90;140;208",
                "38;2;59;125;216",
            ],
        },
        Some(_) => Palette {
            primary: "38;2;250;178;131",
            activity: [
                "38;2;117;99;87",
                "38;2;161;125;102",
                "38;2;206;152;116",
                "38;2;250;178;131",
            ],
        },
        None => Palette { primary: "36", activity: ["2;36", "36", "1;36", "1;96"] },
    }
}

/// `style(value, code, color)` (stats.ts:348-350).
fn style(value: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value.to_owned()
    }
}

/// `paintActivity` (stats.ts:352-358): the level glyph, dim for level
/// 0 and the palette color for 1-4.
fn paint_activity(level: usize, color: bool, palette: &Palette) -> String {
    const GLYPHS: [&str; 5] = ["·", "░", "▒", "▓", "█"];
    let glyph = GLYPHS[level];
    if !color {
        return glyph.to_owned();
    }
    if level == 0 {
        return format!("\x1b[2m{glyph}\x1b[22m");
    }
    format!("\x1b[{}m{glyph}\x1b[39m", palette.activity[level - 1])
}

// =========================================================================++
// Activity calendar (`activity-calendar.ts`)
// ===========================================================================

struct Calendar {
    weeks: Vec<[i32; 7]>,
    months: Vec<(String, usize)>,
    clipped: bool,
}

/// `activityCalendar` (util/activity-calendar.ts:6-57): local dates for
/// the ordinals (DST cannot add or drop cells), Monday-aligned weeks,
/// at most `max_weeks` columns clipped from the left, quartile levels
/// from the distinct positive step counts.
fn activity_calendar(
    activity: &Value,
    from: i64,
    to: i64,
    max_weeks: i64,
    zone: &jiff::tz::TimeZone,
) -> Calendar {
    let ordinal = |time: i64| {
        // `Date.UTC(date.getFullYear(), date.getMonth(), date.getDate()) /
        // 86_400_000` — the LOCAL calendar date as a day ordinal (the
        // activity instants are re-read in the local zone; the server
        // buckets by the requested timezone, which is the same one).
        let zone = zone.clone();
        let (y, m, d) = jiff::Timestamp::from_millisecond(time)
            .map(|instant| {
                let zoned = instant.to_zoned(zone);
                (i64::from(zoned.year()), i64::from(zoned.month()), i64::from(zoned.day()))
            })
            .unwrap_or((1970, 1, 1));
        (unix_ms_from_civil(y, m, d) - unix_ms_from_civil(1970, 1, 1)) / 86_400_000
    };
    let from = ordinal(from);
    let to = ordinal(to - 1);
    // `new Date(from * 86_400_000).getUTCDay()` — 1970-01-01 was a
    // Thursday (4), so the ordinal's weekday is `(ordinal + 4) % 7`.
    let weekday = |date: i64| (date + 4).rem_euclid(7);
    let monday = from - (weekday(from) + 6).rem_euclid(7);
    let total = if to <= from { 0 } else { (to - monday).div_euclid(7) + 1 };
    // `Math.min(total, 53, Math.max(1, Math.floor(maxWeeks)))` — note
    // an empty range (total 0) yields zero weeks.
    let count = total.min(53).min(max_weeks.max(1));
    let start = monday + (total - count).max(0) * 7;

    let mut values = std::collections::HashMap::<String, i64>::new();
    if let Some(days) = activity.as_array() {
        for day in days {
            if let (Some(date), Some(steps)) =
                (day.get("date").and_then(Value::as_str), day.get("steps").and_then(Value::as_i64))
            {
                values.insert(date.to_owned(), steps);
            }
        }
    }
    let mut levels: Vec<i64> = values.values().copied().filter(|steps| *steps > 0).collect();
    levels.sort_unstable();
    levels.dedup();

    let iso_date = |date: i64| {
        let (y, m, d) = civil_from_days(date);
        format!("{y:04}-{m:02}-{d:02}")
    };
    let weeks: Vec<[i32; 7]> = (0..count)
        .map(|week| {
            let mut column = [0_i32; 7];
            for (day, slot) in column.iter_mut().enumerate() {
                let date = start + week * 7 + i64::try_from(day).unwrap_or(0);
                let key = iso_date(date);
                let steps = values.get(&key).copied().unwrap_or(0);
                *slot = if date < from || date > to {
                    -1
                } else if steps == 0 {
                    0
                } else {
                    let index = levels.iter().position(|level| *level == steps).unwrap_or(0);
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let level = (((index + 1) as f64 / levels.len() as f64) * 4.0).ceil() as i32;
                    level.max(1).min(4)
                };
            }
            column
        })
        .collect();

    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    // Thursday (offset +3) identifies the month owning a week.
    let mut labeled: Vec<(usize, String)> = Vec::new();
    for week in 0..count {
        let anchor = (start + week * 7 + 3).min(to).max(from);
        let (_, month, _) = civil_from_days(anchor);
        let label = MONTHS[(month - 1).clamp(0, 11) as usize].to_owned();
        if labeled.last().map_or(true, |(_, previous): &(usize, String)| *previous != label) {
            labeled.push((usize::try_from(week).unwrap_or(0), label));
        }
    }
    let mut months: Vec<(String, usize)> = Vec::new();
    for index in 0..labeled.len() {
        let (week, label) = &labeled[index];
        let span =
            labeled.get(index + 1).map_or(usize::try_from(count).unwrap_or(0), |(next, _)| *next)
                - *week;
        months.push((label.clone(), span));
    }

    Calendar { weeks, months, clipped: count < total }
}

// ===========================================================================
// Rendering (stats.ts:89-317)
// ===========================================================================

fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn i64_of(value: &Value) -> i64 {
    value.as_i64().unwrap_or(0)
}

fn f64_of(value: &Value) -> f64 {
    value.as_f64().unwrap_or(0.0)
}

/// `toLocaleString("en-US")` — comma groupings (stats.ts:308,289,315).
fn locale_string(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

#[allow(clippy::cast_possible_truncation)]
fn format_number(value: f64) -> String {
    if value >= 1_000_000_000.0 {
        format!("{}b", trim_decimal(value / 1_000_000_000.0))
    } else if value >= 1_000_000.0 {
        format!("{}m", trim_decimal(value / 1_000_000.0))
    } else if value >= 1_000.0 {
        format!("{}k", trim_decimal(value / 1_000.0))
    } else {
        locale_string(value.round() as i64)
    }
}

fn trim_decimal(value: f64) -> String {
    let text = format!("{value:.1}");
    text.trim_end_matches(".0").to_owned()
}

fn format_percent(value: f64) -> String {
    format!("{value:.prec$}%", value = value, prec = if value >= 10.0 { 1 } else { 2 })
}

#[allow(clippy::cast_possible_truncation)]
fn format_duration(value: f64) -> String {
    if value < 1000.0 {
        format!("{}ms", value.round() as i64)
    } else {
        format!("{}s", trim_decimal(value / 1000.0))
    }
}

/// `metricCount` (stats.ts:306-308): the number in the primary color.
fn metric_count(value: i64, noun: &str, color: bool, palette: &Palette) -> String {
    metric_count_f64(value as f64, noun, color, palette)
}

#[allow(clippy::cast_possible_truncation, clippy::float_cmp)]
fn metric_count_f64(value: f64, noun: &str, color: bool, palette: &Palette) -> String {
    let primary = format!("1;{}", palette.primary);
    format!(
        "{} {noun}{}",
        style(&format_number(value), &primary, color),
        if value == 1.0 { "" } else { "s" }
    )
}

fn token_total(tokens: &Value) -> f64 {
    f64_of(field(tokens, "input"))
        + f64_of(field(tokens, "output"))
        + f64_of(field(tokens, "reasoning"))
        + f64_of(field(field(tokens, "cache"), "read"))
        + f64_of(field(field(tokens, "cache"), "write"))
}

fn table_row(label: &str, second: &str, third: &str, fourth: &str) -> String {
    let truncated: String = label.chars().take(34).collect();
    format!("{truncated:<34}{second:>10}{third:>12}{fourth:>12}")
}

/// `renderActivity` (stats.ts:165-194): the painted calendar — heading,
/// month labels, seven weekday rows, and the less/more legend.
fn render_activity(
    activity: &Value,
    from: i64,
    to: i64,
    color: bool,
    width: i64,
    palette: &Palette,
    zone: &jiff::tz::TimeZone,
) -> Vec<String> {
    let primary = format!("1;{}", palette.primary);
    let calendar = activity_calendar(activity, from, to, width - 4, zone);
    let dim = |value: &str| style(value, "2", color);
    let months: String = calendar
        .months
        .iter()
        .enumerate()
        .map(|(index, (label, span))| {
            let text = if index == calendar.months.len() - 1 || label.len() <= *span {
                label.clone()
            } else {
                String::new()
            };
            format!("{text:<span$}", span = span)
        })
        .collect::<String>()
        .trim_end()
        .to_owned();
    const WEEKDAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    let heading_text = if calendar.clipped {
        format!("activity · last {} weeks", calendar.weeks.len())
    } else {
        "activity".to_owned()
    };
    let mut lines = vec![style(&heading_text, &primary, color), format!("   {}", dim(&months))];
    for (day, label) in WEEKDAYS.iter().enumerate() {
        let row: String = calendar
            .weeks
            .iter()
            .map(|week| {
                let level = week[day];
                if level < 0 {
                    " ".to_owned()
                } else {
                    #[allow(clippy::cast_sign_loss)]
                    paint_activity(level as usize, color, palette)
                }
            })
            .collect();
        lines.push(format!("{} {}", dim(label), row));
        if day < WEEKDAYS.len() - 1 {
            lines.push(String::new());
        }
    }
    lines.push(String::new());
    let legend: String = (0..5).map(|level| paint_activity(level, color, palette)).collect();
    lines.push(format!("   {} {} {}", dim("less"), legend, dim("more")));
    lines
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn render(
    stats: &Value,
    label: &str,
    scope: &str,
    models: bool,
    tools: bool,
    cost: bool,
    limit: u32,
    color: bool,
    width: i64,
) -> String {
    let palette = terminal_palette();
    let zone = local_zone();
    let primary = format!("1;{}", palette.primary);
    let dim = |value: &str| style(value, "2", color);
    let mut lines: Vec<String> = Vec::new();
    let details = models || tools || cost;
    let empty = i64_of(field(stats, "sessions")) == 0
        && i64_of(field(stats, "prompts")) == 0
        && i64_of(field(stats, "steps")) == 0;
    let tokens = field(stats, "tokens");
    let total_tokens = token_total(tokens);
    let tools_mode = field(field(stats, "tools"), "mode").as_str().unwrap_or("none");
    let tool_totals =
        if tools_mode == "none" { None } else { Some(field(field(stats, "tools"), "totals")) };
    #[allow(clippy::cast_precision_loss)]
    let tool_rate = tool_totals.and_then(|totals| {
        let terminal = i64_of(field(totals, "succeeded")) + i64_of(field(totals, "failed"));
        if terminal == 0 {
            None
        } else {
            Some((i64_of(field(totals, "succeeded")) as f64 / terminal as f64) * 100.0)
        }
    });
    let tool_summary = match (tool_totals, tool_rate) {
        (None, _) => "tool stats unavailable".to_owned(),
        (Some(_), None) => "no tool calls".to_owned(),
        (Some(_), Some(rate)) => {
            format!("{} tool success", style(&format_percent(rate), &primary, color))
        }
    };
    let subagents = i64_of(field(stats, "subagents"));
    let session_line = if subagents > 0 {
        format!(
            "{} · {}",
            metric_count(i64_of(field(stats, "sessions")), "session", color, &palette),
            metric_count(subagents, "subagent", color, &palette)
        )
    } else {
        metric_count(i64_of(field(stats, "sessions")), "session", color, &palette)
    };
    let streak = i64_of(field(stats, "streak"));
    // The single best-streak mention rides the tool-summary line
    // (stats.ts:130) — the old duplicated "activity · ... best streak"
    // summary line is gone with the painted calendar (G-155 ②).
    let summary_line = format!(
        "{tool_summary} · {} · best streak {} day{}",
        metric_count(i64_of(field(stats, "activeDays")), "active day", color, &palette),
        style(&streak.to_string(), &primary, color),
        if streak == 1 { "" } else { "s" }
    );
    let heading = format!(
        "{} {}",
        style("opencode stats", &primary, color),
        dim(&format!("· {label} · {scope}"))
    );

    if details {
        lines.push(dim(&format!("{label} · {scope}")));
    } else if empty {
        lines.push(heading);
        lines.push(String::new());
        lines.push(dim("no activity in this range"));
        lines.push(String::new());
        lines.push(dim("opencode.ai"));
    } else {
        lines.push(heading);
        lines.push(String::new());
        lines.extend(render_activity(
            field(stats, "activity"),
            i64_of(field(field(stats, "range"), "from")),
            i64_of(field(field(stats, "range"), "to")),
            color,
            width,
            &palette,
            &zone,
        ));
        lines.push(String::new());
        lines.push(session_line);
        lines.push(format!(
            "{} · {} · {}",
            metric_count(i64_of(field(stats, "prompts")), "prompt", color, &palette),
            metric_count(i64_of(field(stats, "steps")), "step", color, &palette),
            metric_count_f64(total_tokens, "token", color, &palette)
        ));
        lines.push(summary_line);
        lines.push(String::new());
        lines.push(dim("opencode.ai"));
    }

    if cost {
        lines.push(String::new());
        lines.extend(render_cost(stats));
    }
    if models {
        lines.push(String::new());
        lines.extend(render_models(stats, limit));
    }
    if tools {
        lines.push(String::new());
        lines.extend(render_tools(stats, limit));
    }
    let mut output = lines.join("\n");
    output.push('\n');
    output
}

fn render_cost(stats: &Value) -> Vec<String> {
    let tokens = field(stats, "tokens");
    let cache = field(tokens, "cache");
    let input = f64_of(field(tokens, "input"))
        + f64_of(field(cache, "read"))
        + f64_of(field(cache, "write"));
    let cached = if input == 0.0 { 0.0 } else { f64_of(field(cache, "read")) / input * 100.0 };
    vec![
        "COST & TOKENS".to_owned(),
        row("cost", &format!("${:.2}", f64_of(field(stats, "cost")))),
        row("input", &format_number(f64_of(field(tokens, "input")))),
        row("output", &format_number(f64_of(field(tokens, "output")))),
        row("reasoning", &format_number(f64_of(field(tokens, "reasoning")))),
        row("cache read", &format_number(f64_of(field(cache, "read")))),
        row("cache write", &format_number(f64_of(field(cache, "write")))),
        row("cached input", &format_percent(cached)),
    ]
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<20}{value}")
}

#[allow(clippy::cast_precision_loss)]
fn render_models(stats: &Value, limit: u32) -> Vec<String> {
    let models = field(stats, "models").as_array().cloned().unwrap_or_default();
    if models.is_empty() {
        return vec!["MODELS".to_owned(), "  no model usage".to_owned()];
    }
    let selected: Vec<&Value> = models.iter().take(limit as usize).collect();
    let more = models.len() - selected.len();
    let mut lines = vec!["MODELS".to_owned(), table_row("model", "tokens", "steps", "cost")];
    for item in &selected {
        let model = field(item, "model");
        let reference = format!(
            "{}/{}{}",
            field(model, "providerID").as_str().unwrap_or_default(),
            field(model, "id").as_str().unwrap_or_default(),
            field(model, "variant")
                .as_str()
                .map(|variant| format!("#{variant}"))
                .unwrap_or_default()
        );
        lines.push(table_row(
            &reference,
            &format_number(token_total(field(item, "tokens"))),
            &format_number(i64_of(field(item, "steps")) as f64),
            &format!("${:.2}", f64_of(field(item, "cost"))),
        ));
    }
    if more > 0 {
        lines.push(String::new());
        lines.push(format!(
            "+{} more model{}",
            locale_string(i64::try_from(more).unwrap_or(0)),
            if more == 1 { "" } else { "s" }
        ));
    }
    lines
}

#[allow(clippy::cast_precision_loss)]
fn render_tools(stats: &Value, limit: u32) -> Vec<String> {
    let tools = field(stats, "tools");
    if field(tools, "mode").as_str() != Some("detail") {
        return vec!["TOOL RELIABILITY".to_owned(), "  tool details unavailable".to_owned()];
    }
    let usage = field(tools, "usage").as_array().cloned().unwrap_or_default();
    if usage.is_empty() {
        return vec!["TOOL RELIABILITY".to_owned(), "  no tool calls".to_owned()];
    }
    let selected: Vec<&Value> = usage.iter().take(limit as usize).collect();
    let more = usage.len() - selected.len();
    let totals = field(tools, "totals");
    let mut lines = vec!["TOOL RELIABILITY".to_owned(), table_row("tool", "calls", "error", "p50")];
    for tool in &selected {
        let terminal = i64_of(field(tool, "succeeded")) + i64_of(field(tool, "failed"));
        let error = if terminal == 0 {
            "-".to_owned()
        } else {
            format_percent(i64_of(field(tool, "failed")) as f64 / terminal as f64 * 100.0)
        };
        let p50 =
            field(tool, "durationP50").as_f64().map_or_else(|| "-".to_owned(), format_duration);
        lines.push(table_row(
            field(tool, "name").as_str().unwrap_or_default(),
            &format_number(i64_of(field(tool, "calls")) as f64),
            &error,
            &p50,
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "{} finished calls · {} unfinished",
        format_number(
            (i64_of(field(totals, "succeeded")) + i64_of(field(totals, "failed"))) as f64
        ),
        format_number(i64_of(field(totals, "unfinished")) as f64)
    ));
    if more > 0 {
        lines.push(format!(
            "+{} more tool{}",
            locale_string(i64::try_from(more).unwrap_or(0)),
            if more == 1 { "" } else { "s" }
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_labels() {
        let zone = jiff::tz::TimeZone::UTC;
        let now = 1_700_000_000_000_i64;
        let (from, _to, label) = stats_range(Some(1), None, false, now, &zone);
        assert_eq!(label, "today");
        assert!(from.is_some());
        let (_from, _to, label) = stats_range(None, None, true, now, &zone);
        assert_eq!(label, "all time");
        let (_from, _to, label) = stats_range(Some(7), None, false, now, &zone);
        assert_eq!(label, "last 7 days");
    }

    /// G-155 ③: the day buckets start at LOCAL midnight — in a UTC+5
    /// zone "today" starts 5h before the UTC epoch-midnight.
    #[test]
    fn range_buckets_start_at_local_midnight() {
        let zone = jiff::tz::TimeZone::get("Asia/Karachi").expect("utc+5");
        // 2023-11-14 12:26:40 UTC = 17:26:40 local.
        let now = 1_700_000_000_000_i64;
        let (from, _, label) = stats_range(Some(1), None, false, now, &zone);
        assert_eq!(label, "today");
        // Expected local midnight computed independently via jiff.
        let local_midnight = jiff::Timestamp::from_millisecond(now)
            .expect("now")
            .to_zoned(zone.clone())
            .start_of_day()
            .expect("midnight")
            .timestamp()
            .as_millisecond();
        // 1700000000 is 2023-11-15 03:13:20 PKT — local midnight is
        // 2023-11-14T19:00Z, NOT the UTC epoch-midnight bucket.
        assert_eq!(local_midnight, 1_699_988_400_000);
        assert_eq!(from.expect("bounded"), local_midnight);
        // ...and 7 "last days" reach back by calendar days.
        let (from, _, _) = stats_range(Some(7), None, false, now, &zone);
        assert_eq!(from.expect("bounded"), local_midnight - 6 * 86_400_000);
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_number(1500.0), "1.5k");
        assert_eq!(format_number(2_000_000.0), "2m");
        assert_eq!(format_number(999.0), "999");
        // `toLocaleString("en-US")` groupings (G-155 ④).
        assert_eq!(locale_string(1234), "1,234");
        assert_eq!(locale_string(1234567), "1,234,567");
        assert_eq!(locale_string(999), "999");
        assert_eq!(locale_string(0), "0");
        assert_eq!(locale_string(-1234), "-1,234");
    }

    /// G-155 ①: the painted calendar renders Monday-aligned weeks, the
    /// month labels, and the less/more legend.
    #[test]
    fn activity_calendar_renders_grid_and_legend() {
        // `render` reads the process zone — pin UTC so the fixtures
        // below are timezone-independent.
        std::env::set_var("TZ", "UTC");
        // 2023-11-14 (Tue) with 4 steps; range = that single day.
        let activity = serde_json::json!([{ "date": "2023-11-14", "steps": 4 }]);
        let stats = serde_json::json!({
            "sessions": 1, "prompts": 1, "steps": 4, "subagents": 0,
            "tokens": { "input": 10, "output": 5, "reasoning": 0,
                        "cache": { "read": 0, "write": 0 } },
            "cost": 0.0, "activeDays": 1, "streak": 1,
            "activity": activity,
            "range": { "from": 1_700_000_000_000_i64, "to": 1_700_008_600_000_i64 },
            "tools": { "mode": "summary", "totals": { "succeeded": 1, "failed": 0, "unfinished": 0 } },
        });
        let output = render(&stats, "today", "all projects", false, false, false, 10, false, 80);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "opencode stats · today · all projects");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "activity", "calendar heading");
        assert!(lines[3].starts_with("   "), "month labels are indented");
        assert!(lines[4].starts_with("Mo"), "first weekday row");
        // activity block: heading, months, 7 weekday rows with blanks
        // between, a blank, the legend — 17 lines (indices 2..=18).
        assert!(lines[6].starts_with("Tu"), "weekday rows are separated by blanks");
        assert_eq!(lines[6].chars().nth(3), Some('█'), "Nov 14 (Tue) painted at level 4");
        assert_eq!(lines[4].chars().nth(3), Some(' '), "Nov 13 (Mon) is out of range");
        assert!(lines[18].contains("less"), "legend row");
        assert!(lines[18].contains("more"), "legend row");
        // G-155 ②: exactly ONE "best streak" line.
        assert_eq!(output.matches("best streak").count(), 1);
        assert!(output.contains("best streak 1 day"));
    }

    #[test]
    fn calendar_levels_and_clipping() {
        let zone = jiff::tz::TimeZone::UTC;
        // Two active days inside the [Nov 13, Nov 15] range with
        // different steps → two levels.
        let activity = serde_json::json!([
            { "date": "2023-11-13", "steps": 100 },
            { "date": "2023-11-14", "steps": 500 },
        ]);
        // 2023-11-13 22:13:20 UTC → 2023-11-15 22:06:40 UTC.
        let calendar =
            activity_calendar(&activity, 1_699_914_000_000, 1_700_008_400_000, 53, &zone);
        assert!(!calendar.clipped, "a one-week range is not clipped");
        assert_eq!(calendar.weeks.len(), 1);
        assert_eq!(calendar.months.len(), 1, "single month label (Nov)");
        assert_eq!(calendar.months[0].0, "Nov");
        // Monday level 2 (smaller), Tuesday level 4 (larger).
        assert_eq!(calendar.weeks[0][0], 2);
        assert_eq!(calendar.weeks[0][1], 4);
        // Narrow width clips from the left and reports it (a two-week
        // Nov 6 → Nov 15 range shown in one column).
        let clipped = activity_calendar(&activity, 1_699_308_800_000, 1_700_008_400_000, 1, &zone);
        assert!(clipped.clipped);
        assert_eq!(clipped.weeks.len(), 1);
    }

    /// G-155 ①: COLORFGBG light/dark palettes and the cyan fallback.
    #[test]
    fn colorfgbg_palette_detection() {
        std::env::set_var("COLORFGBG", "15;0");
        assert_eq!(terminal_palette().primary, "38;2;250;178;131", "dark bg");
        std::env::set_var("COLORFGBG", "0;15");
        assert_eq!(terminal_palette().primary, "38;2;59;125;216", "light bg");
        std::env::remove_var("COLORFGBG");
        assert_eq!(terminal_palette().primary, "36", "no COLORFGBG → cyan");
        // Paint in color mode emits the SGR sequences.
        let palette = terminal_palette();
        assert!(paint_activity(2, true, &palette).contains("\x1b["));
        assert_eq!(paint_activity(2, false, &palette), "▒");
        assert_eq!(paint_activity(0, true, &palette), "\x1b[2m·\x1b[22m");
    }
}
