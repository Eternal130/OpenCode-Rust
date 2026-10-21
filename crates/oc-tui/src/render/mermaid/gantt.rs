// Upstream-mirror lint posture: the merman port keeps the TypeScript
// arithmetic shape (JS Number semantics on an i64 grid, the same cast
// sites and long single-responsibility functions), so the pedantic
// numeric/style lints that would rewrite that shape are quieted here.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::doc_markdown,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::items_after_statements,
    clippy::explicit_iter_loop,
    clippy::for_kv_map,
    clippy::map_unwrap_or,
    clippy::manual_strip,
    clippy::range_plus_one,
    clippy::option_option,
    clippy::implicit_hasher,
    clippy::match_same_arms
)]

//! `gantt/*` — the `gantt` timeline renderer
//! (`packages/merman/src/gantt.ts` upstream).

use std::collections::HashMap;

use crate::render::mermaid::core::canvas::{DiagramCanvas, TextOptions};
use crate::render::mermaid::core::color::{blend_color, Rgba};
use crate::render::mermaid::core::mermaid::{
    first_meaningful_mermaid_line, meaningful_numbered_mermaid_lines, strip_mermaid_quotes,
};
use crate::render::mermaid::core::text::diagram_text_width;
use crate::render::mermaid::diagnostics::{MermaidDiagramKind, MermaidSyntaxError};

/// `GanttTaskState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GanttTaskState {
    #[default]
    Task,
    Active,
    Critical,
    Done,
    Milestone,
}

/// `GanttEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GanttEntry {
    Section { label: String },
    Task(usize),
}

/// `GanttTask`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GanttTask {
    pub label: String,
    pub id: Option<String>,
    pub section: Option<String>,
    pub start: i64,
    pub end: i64,
    pub state: GanttTaskState,
}

/// `GanttDiagram`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GanttDiagram {
    pub title: Option<String>,
    pub date_format: String,
    pub axis_format: String,
    pub tasks: Vec<GanttTask>,
    pub entries: Vec<GanttEntry>,
}

/// `GanttRenderStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GanttRenderStyle {
    Rail,
    Block,
    Capsule,
    Points,
    #[default]
    Track,
}

/// `GanttLineStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GanttLineStyle {
    Heavy,
    #[default]
    Thin,
    Double,
    Dashed,
}

/// `GanttLabelLayout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GanttLabelLayout {
    Right,
    #[default]
    Left,
    Tree,
}

/// `GanttTrackTone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GanttTrackTone {
    Medium,
    Dim,
    #[default]
    Faint,
}

/// `GanttDiagramRenderOptions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GanttRenderOptions {
    pub layout_max_width: Option<i64>,
    pub style: Option<GanttRenderStyle>,
    pub track: Option<GanttTrack>,
    pub endpoints: Option<GanttEndpoints>,
    pub line: Option<GanttLineStyle>,
    pub labels: Option<GanttLabelLayout>,
    pub sections_spaced: Option<bool>,
    pub track_tone: Option<GanttTrackTone>,
}

/// `track?: "dots" | "line"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GanttTrack {
    Dots,
    Line,
}

/// `endpoints?: "plain" | "points"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GanttEndpoints {
    Plain,
    Points,
}

/// `GanttCellStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GanttCellStyle {
    Title,
    Axis,
    Section,
    Task,
    Active,
    Critical,
    Done,
    Milestone,
    TrackMedium,
    TrackDim,
    TrackFaint,
}

/// `GanttGrid`.
pub type GanttGrid = DiagramCanvas<GanttCellStyle>;

fn syntax_error(line_number: usize, source_line: &str, reason: &str) -> MermaidSyntaxError {
    MermaidSyntaxError::new(MermaidDiagramKind::Gantt, line_number, source_line, reason)
}

/// `stripComment` — `%%` truncates (no quote awareness here).
fn strip_comment(value: &str) -> &str {
    match value.find("%%") {
        Some(comment) => value[..comment].trim(),
        None => value.trim(),
    }
}

/// `isMermaidGanttDiagram` — `^gantt$/i`.
#[must_use]
pub fn is_mermaid_gantt_diagram(content: &str) -> bool {
    first_meaningful_mermaid_line(content).is_some_and(|line| line.eq_ignore_ascii_case("gantt"))
}

/// `parseMermaidGanttDiagram`.
pub fn parse_mermaid_gantt_diagram(content: &str) -> Result<GanttDiagram, MermaidSyntaxError> {
    let mut tasks: Vec<GanttTask> = Vec::new();
    let mut entries: Vec<GanttEntry> = Vec::new();
    let mut tasks_by_id: HashMap<String, i64> = HashMap::new(); // id -> end
    let mut title: Option<String> = None;
    let mut date_format = "YYYY-MM-DD".to_string();
    let mut axis_format = "%Y-%m-%d".to_string();
    let mut section: Option<String> = None;
    let mut header_seen = false;
    let mut in_accessibility_description = false;

    for source in meaningful_numbered_mermaid_lines(content) {
        let line = strip_comment(source.text).to_string();
        if in_accessibility_description {
            if line == "}" {
                in_accessibility_description = false;
            }
            continue;
        }
        // `^accDescr\s*\{$/i`
        if line.len() >= 8
            && line.get(..8).is_some_and(|head| head.eq_ignore_ascii_case("accDescr"))
            && line[8..].trim() == "{"
        {
            in_accessibility_description = true;
            continue;
        }
        if is_accessibility_line(&line) {
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("gantt") {
            if header_seen {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    "Gantt header can only appear once",
                ));
            }
            header_seen = true;
            continue;
        }
        if !header_seen {
            return Err(syntax_error(source.line_number, &line, "Gantt header is required"));
        }

        // SECTION_RE = ^section(?:\s+(.+))?$/i
        if let Some(section_label) = section_label(&line) {
            let Some(section_label) = section_label else {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    "Gantt section cannot be empty",
                ));
            };
            let label = strip_mermaid_quotes(&section_label);
            entries.push(GanttEntry::Section { label: label.clone() });
            section = Some(label);
            continue;
        }

        // DIRECTIVE_RE = ^(title|dateFormat|axisFormat|tickInterval|excludes|todayMarker)\s+(.+)$/i
        if let Some((name, value)) = directive(&line) {
            let lowered = name.to_lowercase();
            let value = value.trim();
            match lowered.as_str() {
                "title" => title = Some(strip_mermaid_quotes(value)),
                "dateformat" => date_format = value.to_string(),
                "axisformat" => axis_format = value.to_string(),
                _ => {
                    return Err(syntax_error(
                        source.line_number,
                        &line,
                        &format!("{name} is not supported"),
                    ))
                }
            }
            continue;
        }

        let Some(separator) = line.find(':') else {
            return Err(syntax_error(source.line_number, &line, ""));
        };
        if separator < 1 {
            return Err(syntax_error(source.line_number, &line, ""));
        }
        let label = strip_mermaid_quotes(&line[..separator]);
        let mut fields: Vec<String> = line[separator + 1..]
            .split(',')
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .map(str::to_string)
            .collect();
        let mut flags: Vec<String> = Vec::new();
        while fields.first().is_some_and(|field| is_task_state_flag(&field.to_lowercase())) {
            flags.push(fields.remove(0).to_lowercase());
        }
        if fields.len() < 2 || fields.len() > 3 {
            return Err(syntax_error(
                source.line_number,
                &line,
                "Gantt tasks require a start and end or duration",
            ));
        }
        let id = if fields.len() == 3 { Some(fields.remove(0)) } else { None };
        if let Some(id) = &id {
            if tasks_by_id.contains_key(id) {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    &format!("Duplicate task id \"{id}\""),
                ));
            }
        }
        let start = parse_start(&fields[0], &date_format, &tasks_by_id, source.line_number, &line)?;
        let end = parse_end(&fields[1], start, &date_format, source.line_number, &line)?;
        if end < start {
            return Err(syntax_error(
                source.line_number,
                &line,
                "Gantt task cannot end before it starts",
            ));
        }
        let task = GanttTask {
            label,
            id: id.clone(),
            section: section.clone(),
            start,
            end,
            state: task_state(&flags),
        };
        tasks.push(task.clone());
        entries.push(GanttEntry::Task(tasks.len() - 1));
        if let Some(id) = id {
            tasks_by_id.insert(id, task.end);
        }
    }

    if !header_seen {
        return Err(MermaidSyntaxError::new(
            MermaidDiagramKind::Gantt,
            1,
            "",
            "Gantt header is required",
        ));
    }
    Ok(GanttDiagram { title, date_format, axis_format, tasks, entries })
}

fn is_task_state_flag(flag: &str) -> bool {
    matches!(flag, "active" | "done" | "crit" | "milestone" | "vert")
}

fn task_state(flags: &[String]) -> GanttTaskState {
    if flags.iter().any(|flag| flag == "milestone") {
        return GanttTaskState::Milestone;
    }
    if flags.iter().any(|flag| flag == "crit") {
        return GanttTaskState::Critical;
    }
    if flags.iter().any(|flag| flag == "done") {
        return GanttTaskState::Done;
    }
    if flags.iter().any(|flag| flag == "active") {
        return GanttTaskState::Active;
    }
    GanttTaskState::Task
}

/// `^section(?:\s+(.+))?$/i` — `Some(None)` matches a bare `section`.
fn section_label(line: &str) -> Option<Option<String>> {
    line.get(..7).filter(|head| head.eq_ignore_ascii_case("section"))?;
    let rest = &line[7..];
    if rest.is_empty() {
        return Some(None);
    }
    let rest = rest.strip_prefix(|c: char| c.is_whitespace())?;
    Some(Some(rest.to_string()))
}

/// `^(title|dateFormat|axisFormat|tickInterval|excludes|todayMarker)\s+(.+)$/i`.
fn directive(line: &str) -> Option<(&str, &str)> {
    for name in ["title", "dateFormat", "axisFormat", "tickInterval", "excludes", "todayMarker"] {
        if line.get(..name.len()).is_some_and(|head| head.eq_ignore_ascii_case(name)) {
            let Some(value) = line[name.len()..].strip_prefix(|c: char| c.is_whitespace()) else {
                continue;
            };
            if value.is_empty() {
                continue;
            }
            return Some((name, value));
        }
    }
    None
}

/// `ACCESSIBILITY_RE = ^acc(?:Title|Descr)(?::|\s|$)/i`.
fn is_accessibility_line(line: &str) -> bool {
    for word in ["accTitle", "accDescr"] {
        if let Some(rest) = line
            .get(..word.len())
            .filter(|head| head.eq_ignore_ascii_case(word))
            .map(|_| &line[word.len()..])
        {
            return rest.is_empty()
                || rest.starts_with(':')
                || rest.starts_with(|c: char| c.is_whitespace());
        }
    }
    false
}

/// `parseStart` — `after id...` chains or a date.
fn parse_start(
    value: &str,
    format: &str,
    tasks_by_id: &HashMap<String, i64>,
    line_number: usize,
    source_line: &str,
) -> Result<i64, MermaidSyntaxError> {
    if !is_after_prefix(value) {
        return parse_date(value, format, line_number, source_line);
    }
    let dependencies: Vec<&str> = strip_after_prefix(value).split_whitespace().collect();
    let mut ends: Vec<i64> = Vec::new();
    for id in &dependencies {
        let Some(end) = tasks_by_id.get(*id) else {
            return Err(syntax_error(
                line_number,
                source_line,
                &format!("Unknown Gantt task id \"{id}\""),
            ));
        };
        ends.push(*end);
    }
    Ok(ends.iter().copied().max().unwrap_or(0))
}

/// `^after\s+/i`.
fn is_after_prefix(value: &str) -> bool {
    value.get(..5).is_some_and(|head| head.eq_ignore_ascii_case("after"))
        && value[5..].starts_with(|c: char| c.is_whitespace())
}

fn strip_after_prefix(value: &str) -> &str {
    value[5..].trim()
}

/// `parseEnd` — durations (`2d`) or a date.
fn parse_end(
    value: &str,
    start: i64,
    format: &str,
    line_number: usize,
    source_line: &str,
) -> Result<i64, MermaidSyntaxError> {
    if let Some(duration) = parse_duration(value) {
        return Ok(start + duration);
    }
    parse_date(value, format, line_number, source_line)
}

/// `^(\d+(?:\.\d+)?)\s*(ms|s|m|h|d|w)$/i`.
fn parse_duration(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    let unit_start = trimmed.find(|c: char| c.is_ascii_alphabetic()).filter(|&index| index > 0)?;
    let (amount, unit) = trimmed.split_at(unit_start);
    let unit = unit.trim();
    if amount.is_empty() || !amount.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    if amount.matches('.').count() > 1 {
        return None;
    }
    let amount: f64 = amount.parse().ok()?;
    let factor = match unit.to_lowercase().as_str() {
        "ms" => 1.0,
        "s" => 1_000.0,
        "m" => 60_000.0,
        "h" => 3_600_000.0,
        "d" => 86_400_000.0,
        "w" => 604_800_000.0,
        _ => return None,
    };
    Some((amount * factor) as i64)
}

/// `parseDate` — `s`/`X`/`x` numeric epochs and `YYYY-MM-DD[ HH:mm[:ss]]`.
fn parse_date(
    value: &str,
    format: &str,
    line_number: usize,
    source_line: &str,
) -> Result<i64, MermaidSyntaxError> {
    if let Ok(numeric) = value.trim().parse::<f64>() {
        if numeric.is_finite() {
            if format == "s" || format == "X" {
                return Ok((numeric * 1_000.0) as i64);
            }
            if format == "x" {
                return Ok(numeric as i64);
            }
        }
    }

    if let Some(calendar) = parse_calendar(value) {
        if matches!(format, "YYYY-MM-DD" | "YYYY-MM-DD HH:mm" | "YYYY-MM-DD HH:mm:ss") {
            let (year, month, day, hour, minute, second) = calendar;
            return Ok(utc_ms(year, month - 1, day, hour, minute, second));
        }
    }
    Err(syntax_error(
        line_number,
        source_line,
        &format!("Unsupported date \"{value}\" for dateFormat {format}"),
    ))
}

/// `^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?)?$`.
fn parse_calendar(value: &str) -> Option<(i64, i64, i64, i64, i64, i64)> {
    let bytes = value.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        let slice = bytes.get(range)?;
        if slice.iter().all(u8::is_ascii_digit) {
            Some(std::str::from_utf8(slice).ok()?.parse().ok()?)
        } else {
            None
        }
    };
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }
    let year = digits(0..4)?;
    let month = digits(5..7)?;
    let day = digits(8..10)?;
    let mut hour = 0;
    let mut minute = 0;
    let mut second = 0;
    if bytes.len() > 10 {
        let separator = bytes.get(10).copied()?;
        if separator != b' ' && separator != b'T' {
            return None;
        }
        if bytes.len() < 16 || bytes.get(13) != Some(&b':') {
            return None;
        }
        hour = digits(11..13)?;
        minute = digits(14..16)?;
        if bytes.len() > 16 {
            if bytes.len() != 19 || bytes.get(16) != Some(&b':') {
                return None;
            }
            second = digits(17..19)?;
        }
    } else if bytes.len() != 10 {
        return None;
    }
    Some((year, month, day, hour, minute, second))
}

/// `Date.UTC(year, month0, day, hour, minute, second)` with JS-style
/// component rollover (Howard Hinnant's `days_from_civil`).
fn utc_ms(year: i64, month0: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    let year = year + month0.div_euclid(12);
    let month = month0.rem_euclid(12) + 1; // 1..=12
    let days = days_from_civil(year, month, 1) + (day - 1);
    days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000
}

/// Days since 1970-01-01 for a proleptic-Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `civil_from_days` — the inverse of `days_from_civil`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (year + i64::from(month <= 2), month, day)
}

/// `formatTime` — the six strftime tokens over UTC parts.
fn format_time(value: i64, format: &str) -> String {
    let days = value.div_euclid(86_400_000);
    let time_of_day = value.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = time_of_day / 3_600_000;
    let minute = (time_of_day / 60_000) % 60;
    let second = (time_of_day / 1_000) % 60;
    let pad = |number: i64| format!("{number:02}");
    format
        .replace("%Y", &year.to_string())
        .replace("%m", &pad(month))
        .replace("%d", &pad(day))
        .replace("%H", &pad(hour))
        .replace("%M", &pad(minute))
        .replace("%S", &pad(second))
}

const LABEL_GAP: i64 = 2;
const MIN_CHART_WIDTH: i64 = 24;
const MAX_CHART_WIDTH: i64 = 64;

/// `drawGanttDiagramGrid`.
pub fn draw_gantt_diagram_grid(diagram: &GanttDiagram, options: &GanttRenderOptions) -> GanttGrid {
    if diagram.entries.is_empty() {
        return DiagramCanvas::new(0, 0, None)
            .unwrap_or_else(|_| DiagramCanvas::new(1, 1, None).unwrap());
    }
    let labels: Vec<String> = diagram
        .entries
        .iter()
        .enumerate()
        .map(|(index, _)| entry_label(diagram, index, options.labels.unwrap_or_default()))
        .collect();
    let label_width =
        labels.iter().map(|label| diagram_text_width(label) as i64).max().unwrap_or(0);
    let spaced = options.sections_spaced.unwrap_or(true);
    let mut entry_rows: Vec<i64> = Vec::new();
    let mut body_height = 0i64;
    for (index, entry) in diagram.entries.iter().enumerate() {
        if spaced && matches!(entry, GanttEntry::Section { .. }) && index > 0 {
            body_height += 1;
        }
        entry_rows.push(body_height);
        body_height += 1;
    }
    if diagram.tasks.is_empty() {
        let mut grid: GanttGrid = DiagramCanvas::new(label_width, body_height, None).unwrap();
        for (index, entry) in diagram.entries.iter().enumerate() {
            if let GanttEntry::Section { label } = entry {
                grid.set_text(0, entry_rows[index], label, Some(GanttCellStyle::Section));
            }
        }
        return grid;
    }
    let available = options.layout_max_width.unwrap_or(120) - label_width - LABEL_GAP;
    let chart_width = MIN_CHART_WIDTH.min(MAX_CHART_WIDTH.min(available)).max(MIN_CHART_WIDTH);
    let minimum = diagram.tasks.iter().map(|task| task.start).min().unwrap_or(0);
    let maximum = diagram.tasks.iter().map(|task| task.end).max().unwrap_or(0);
    let span = 1.max(maximum - minimum);
    let title_height = i64::from(diagram.title.is_some()) * 2;
    let axis_height = 2i64;
    let mut grid: GanttGrid = DiagramCanvas::new(
        label_width + LABEL_GAP + chart_width,
        title_height + axis_height + body_height,
        None,
    )
    .unwrap();
    let chart_x = label_width + LABEL_GAP;

    if let Some(title) = &diagram.title {
        grid.set_text(
            0.max(chart_x + (chart_width - diagram_text_width(title) as i64).div_euclid(2)),
            0,
            title,
            Some(GanttCellStyle::Title),
        );
    }
    draw_axis(&mut grid, chart_x, title_height, chart_width, minimum, span, &diagram.axis_format);

    for (index, entry) in diagram.entries.iter().enumerate() {
        let y = title_height + axis_height + entry_rows[index];
        match entry {
            GanttEntry::Section { .. } => {
                grid.set_text(0, y, &labels[index], Some(GanttCellStyle::Section));
            }
            GanttEntry::Task(task_index) => {
                let task_index = *task_index;
                let label = &labels[index];
                let label_x = if options.labels == Some(GanttLabelLayout::Right) {
                    label_width - diagram_text_width(label) as i64
                } else {
                    0
                };
                let style = diagram.tasks[task_index].state;
                grid.set_text(label_x, y, label, Some(task_style(style)));
                draw_task(
                    &mut grid,
                    &diagram.tasks[task_index],
                    chart_x,
                    y,
                    chart_width,
                    minimum,
                    span,
                    options,
                );
            }
        }
    }
    grid
}

fn task_style(state: GanttTaskState) -> GanttCellStyle {
    match state {
        GanttTaskState::Task => GanttCellStyle::Task,
        GanttTaskState::Active => GanttCellStyle::Active,
        GanttTaskState::Critical => GanttCellStyle::Critical,
        GanttTaskState::Done => GanttCellStyle::Done,
        GanttTaskState::Milestone => GanttCellStyle::Milestone,
    }
}

/// `drawAxis`.
fn draw_axis(
    grid: &mut GanttGrid,
    x: i64,
    y: i64,
    width: i64,
    minimum: i64,
    span: i64,
    format: &str,
) {
    for offset in 0..width {
        grid.set_cell(x + offset, y + 1, "─", Some(GanttCellStyle::Axis));
    }
    let step = tick_step(span);
    let mut ticks: Vec<i64> = Vec::new();
    let first = ((minimum as f64) / (step as f64)).ceil() as i64 * step;
    let mut value = first;
    while value <= minimum + span {
        ticks.push(value);
        value += step;
    }
    if ticks.is_empty() {
        ticks.push(minimum);
        ticks.push(minimum + span);
    }
    for value in ticks {
        let offset =
            (((value - minimum) as f64 / span as f64) * ((width - 1) as f64)).round() as i64;
        let label = format_time(value, format);
        let label_width = diagram_text_width(&label) as i64;
        let label_x = x.max((x + width - label_width).min(x + offset - label_width.div_euclid(2)));
        grid.set_text(label_x, y, &label, Some(GanttCellStyle::Axis));
        grid.set_cell(x + offset, y + 1, "┬", Some(GanttCellStyle::Axis));
    }
}

/// `tickStep`.
fn tick_step(span: i64) -> i64 {
    const STEPS: [i64; 15] = [
        1_000,
        5_000,
        10_000,
        30_000,
        60_000,
        300_000,
        900_000,
        3_600_000,
        21_600_000,
        43_200_000,
        86_400_000,
        172_800_000,
        604_800_000,
        2_592_000_000,
        31_536_000_000,
    ];
    STEPS.iter().copied().find(|step| *step >= span.div_euclid(4)).unwrap_or(STEPS[STEPS.len() - 1])
}

/// `drawTask`.
fn draw_task(
    grid: &mut GanttGrid,
    task: &GanttTask,
    x: i64,
    y: i64,
    width: i64,
    minimum: i64,
    span: i64,
    options: &GanttRenderOptions,
) {
    let style = options.style.unwrap_or_default();
    let line = line_glyph(options.line.unwrap_or_default());
    let start =
        (((task.start - minimum) as f64 / span as f64) * ((width - 1) as f64)).round() as i64;
    let end = (((task.end - minimum) as f64 / span as f64) * ((width - 1) as f64)).round() as i64;
    let state = task_style(task.state);
    if task.state == GanttTaskState::Milestone || start == end {
        grid.set_cell(x + start, y, "◆", Some(state));
        return;
    }
    if style == GanttRenderStyle::Track {
        let track_glyph = if options.track == Some(GanttTrack::Dots) { "·" } else { line };
        let track_style = track_cell_style(options);
        for offset in 0..width {
            grid.set_cell(x + offset, y, track_glyph, Some(track_style));
        }
    }
    let glyph = if style == GanttRenderStyle::Block { "█" } else { line };
    for offset in start..=end {
        grid.set_cell(x + offset, y, glyph, Some(state));
    }
    if style == GanttRenderStyle::Block {
        return;
    }
    if style == GanttRenderStyle::Capsule || style == GanttRenderStyle::Track {
        if style == GanttRenderStyle::Track {
            if options.endpoints == Some(GanttEndpoints::Points) {
                let track_style = track_cell_style(options);
                grid.set_cell(x + start, y, "●", Some(track_style));
                grid.set_cell(x + end, y, "●", Some(track_style));
            }
            return;
        }
        let caps = cap_glyphs(options.line.unwrap_or_default());
        grid.set_cell(x + start, y, caps.0, Some(state));
        grid.set_cell(x + end, y, caps.1, Some(state));
        return;
    }
    if style == GanttRenderStyle::Points {
        grid.set_cell(x + start, y, "●", Some(state));
        grid.set_cell(x + end, y, "●", Some(state));
        return;
    }
    grid.set_cell(x + start, y, "┣", Some(state));
    grid.set_cell(x + end, y, "┫", Some(state));
}

/// `entryLabel`.
fn entry_label(diagram: &GanttDiagram, index: usize, layout: GanttLabelLayout) -> String {
    match &diagram.entries[index] {
        GanttEntry::Section { label } => label.clone(),
        GanttEntry::Task(task_index) => {
            let task_index = *task_index;
            if layout != GanttLabelLayout::Tree {
                return diagram.tasks[task_index].label.clone();
            }
            let last = !matches!(diagram.entries.get(index + 1), Some(GanttEntry::Task(_)));
            format!("  {}─ {}", if last { "└" } else { "├" }, diagram.tasks[task_index].label)
        }
    }
}

/// `lineGlyph`.
fn line_glyph(line: GanttLineStyle) -> &'static str {
    match line {
        GanttLineStyle::Thin => "─",
        GanttLineStyle::Double => "═",
        GanttLineStyle::Dashed => "╌",
        GanttLineStyle::Heavy => "━",
    }
}

/// `capGlyphs`.
fn cap_glyphs(line: GanttLineStyle) -> (&'static str, &'static str) {
    match line {
        GanttLineStyle::Thin | GanttLineStyle::Dashed => ("╶", "╴"),
        GanttLineStyle::Double => ("╞", "╡"),
        GanttLineStyle::Heavy => ("╺", "╸"),
    }
}

/// `trackCellStyle`.
fn track_cell_style(options: &GanttRenderOptions) -> GanttCellStyle {
    match options.track_tone {
        Some(GanttTrackTone::Medium) => GanttCellStyle::TrackMedium,
        Some(GanttTrackTone::Dim) => GanttCellStyle::TrackDim,
        _ => GanttCellStyle::TrackFaint,
    }
}

/// `renderGanttGridText` — `toString({ trimBottom: true })`.
#[must_use]
pub fn render_gantt_grid_text(grid: &GanttGrid) -> String {
    grid.to_text(TextOptions { trim_top: false, trim_bottom: true })
}

/// `renderGanttDiagram`.
pub fn render_gantt_diagram(
    content: &str,
    options: &GanttRenderOptions,
) -> Result<String, MermaidSyntaxError> {
    let diagram = parse_mermaid_gantt_diagram(content)?;
    Ok(render_gantt_grid_text(&draw_gantt_diagram_grid(&diagram, options)))
}

/// `GanttStyleColors` — `resolveGanttStyleColors`.
#[derive(Debug, Clone, Copy)]
pub struct GanttStyleColors {
    pub title: Rgba,
    pub axis: Rgba,
    pub section: Rgba,
    pub task: Rgba,
    pub active: Rgba,
    pub critical: Rgba,
    pub done: Rgba,
    pub milestone: Rgba,
    pub track_medium: Rgba,
    pub track_dim: Rgba,
    pub track_faint: Rgba,
}

impl GanttStyleColors {
    #[must_use]
    pub fn color(&self, style: Option<GanttCellStyle>) -> Option<Rgba> {
        let style = style?;
        Some(match style {
            GanttCellStyle::Title => self.title,
            GanttCellStyle::Axis => self.axis,
            GanttCellStyle::Section => self.section,
            GanttCellStyle::Task => self.task,
            GanttCellStyle::Active => self.active,
            GanttCellStyle::Critical => self.critical,
            GanttCellStyle::Done => self.done,
            GanttCellStyle::Milestone => self.milestone,
            GanttCellStyle::TrackMedium => self.track_medium,
            GanttCellStyle::TrackDim => self.track_dim,
            GanttCellStyle::TrackFaint => self.track_faint,
        })
    }
}

/// `GanttColorInput`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GanttColorInput {
    pub title: Option<Rgba>,
    pub axis: Option<Rgba>,
    pub background: Option<Rgba>,
    pub section: Option<Rgba>,
    pub task: Option<Rgba>,
    pub active: Option<Rgba>,
    pub critical: Option<Rgba>,
    pub done: Option<Rgba>,
    pub milestone: Option<Rgba>,
}

/// `resolveGanttStyleColors`.
#[must_use]
pub fn resolve_gantt_style_colors(input: GanttColorInput) -> GanttStyleColors {
    let axis = input.axis.unwrap_or(Rgba::from_ints(111, 138, 126, 255));
    let background = input.background.unwrap_or(Rgba::from_ints(13, 17, 23, 255));
    GanttStyleColors {
        title: input.title.unwrap_or(Rgba::from_ints(228, 239, 232, 255)),
        axis,
        section: input.section.unwrap_or(Rgba::from_ints(154, 184, 169, 255)),
        task: input.task.unwrap_or(Rgba::from_ints(134, 225, 200, 255)),
        active: input.active.unwrap_or(Rgba::from_ints(134, 225, 200, 255)),
        critical: input.critical.unwrap_or(Rgba::from_ints(230, 177, 126, 255)),
        done: input.done.unwrap_or(Rgba::from_ints(111, 138, 126, 255)),
        milestone: input.milestone.unwrap_or(Rgba::from_ints(198, 160, 246, 255)),
        track_medium: blend_color(Some(axis), Some(background), 0.35).unwrap_or(axis),
        track_dim: blend_color(Some(axis), Some(background), 0.55).unwrap_or(axis),
        track_faint: blend_color(Some(axis), Some(background), 0.72).unwrap_or(axis),
    }
}

#[cfg(test)]
mod tests;
