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

//! `timeline/*` — the `timeline` spine renderer
//! (`packages/merman/src/timeline.ts` upstream).

use crate::render::mermaid::core::canvas::{DiagramCanvas, TextOptions};
use crate::render::mermaid::core::color::{blend_color, Rgba};
use crate::render::mermaid::core::mermaid::{
    first_meaningful_mermaid_line, meaningful_numbered_mermaid_lines, strip_mermaid_quotes,
};
use crate::render::mermaid::core::text::diagram_text_width;
use crate::render::mermaid::core::text_lines::split_diagram_lines;
use crate::render::mermaid::diagnostics::{MermaidDiagramKind, MermaidSyntaxError};

/// `TimelineDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineDirection {
    Td,
    #[default]
    Lr,
}

/// `TimelinePeriod`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelinePeriod {
    pub period: String,
    pub events: Vec<String>,
}

/// `TimelineEntry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineEntry {
    Section { label: String },
    Period(usize),
}

/// `TimelineDiagram`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimelineDiagram {
    pub direction: TimelineDirection,
    pub title: Option<String>,
    pub sections: Vec<String>,
    pub periods: Vec<TimelinePeriod>,
    pub entries: Vec<TimelineEntry>,
}

/// `TimelineDiagramRenderOptions` — parsed for Mermaid compatibility;
/// timelines always use a vertical terminal layout.
#[derive(Debug, Clone, Copy, Default)]
pub struct TimelineRenderOptions {
    pub direction: Option<TimelineDirection>,
}

/// `TimelineCellStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimelineCellStyle {
    Title,
    Section,
    Period,
    Spine,
    Event,
    SectionFade1,
    SectionFade2,
    SectionFade3,
    PeriodFade1,
    PeriodFade2,
    PeriodFade3,
}

/// `TimelineGrid`.
pub type TimelineGrid = DiagramCanvas<TimelineCellStyle>;

/// `TIMELINE_SECTION_FADE_STYLES` / `TIMELINE_PERIOD_FADE_STYLES`.
const SECTION_FADE_STYLES: [TimelineCellStyle; 3] = [
    TimelineCellStyle::SectionFade1,
    TimelineCellStyle::SectionFade2,
    TimelineCellStyle::SectionFade3,
];
const PERIOD_FADE_STYLES: [TimelineCellStyle; 3] = [
    TimelineCellStyle::PeriodFade1,
    TimelineCellStyle::PeriodFade2,
    TimelineCellStyle::PeriodFade3,
];

fn syntax_error(line_number: usize, source_line: &str, reason: &str) -> MermaidSyntaxError {
    MermaidSyntaxError::new(MermaidDiagramKind::Timeline, line_number, source_line, reason)
}

/// `stripTimelineComment`.
fn strip_timeline_comment(value: &str) -> &str {
    match value.find("%%") {
        Some(comment) => value[..comment].trim(),
        None => value.trim(),
    }
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

/// `HEADER_RE = ^timeline(?:\s+(TD|LR))?$` — the direction or `None`.
fn parse_header(line: &str) -> Option<Option<TimelineDirection>> {
    if !line.get(..8).is_some_and(|head| head.eq_ignore_ascii_case("timeline")) {
        return None;
    }
    let tail = &line[8..];
    let trimmed = tail.trim_start();
    if trimmed.is_empty() {
        return Some(None);
    }
    if trimmed.len() < tail.len() {
        // `\s+ (TD|LR)` with nothing after.
        let direction = match trimmed.to_uppercase().as_str() {
            "TD" => TimelineDirection::Td,
            "LR" => TimelineDirection::Lr,
            _ => return None,
        };
        return Some(Some(direction));
    }
    None
}

/// `isMermaidTimelineDiagram`.
#[must_use]
pub fn is_mermaid_timeline_diagram(content: &str) -> bool {
    first_meaningful_mermaid_line(content).and_then(parse_header).is_some()
}

/// `parseMermaidTimelineDiagram`.
pub fn parse_mermaid_timeline_diagram(
    content: &str,
) -> Result<TimelineDiagram, MermaidSyntaxError> {
    let mut sections: Vec<String> = Vec::new();
    let mut periods: Vec<TimelinePeriod> = Vec::new();
    let mut entries: Vec<TimelineEntry> = Vec::new();
    let mut direction = TimelineDirection::Lr;
    let mut title: Option<String> = None;
    let mut current_period: Option<usize> = None;
    let mut in_accessibility_description = false;

    for source in meaningful_numbered_mermaid_lines(content) {
        let line = strip_timeline_comment(source.text).to_string();
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
        if line.is_empty() || line.starts_with('#') || is_accessibility_line(&line) {
            continue;
        }
        if let Some(header_direction) = parse_header(&line) {
            direction = header_direction.unwrap_or(TimelineDirection::Lr);
            continue;
        }

        // TITLE_RE = ^title(?:\s+(.+))?$/i
        if line.get(..5).is_some_and(|head| head.eq_ignore_ascii_case("title")) {
            let Some(rest) = line[5..].strip_prefix(|c: char| c.is_whitespace()) else {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    "Timeline title cannot be empty",
                ));
            };
            title = Some(strip_mermaid_quotes(rest));
            continue;
        }

        // SECTION_RE = ^section(?:\s+(.+))?$/i
        if line.get(..7).is_some_and(|head| head.eq_ignore_ascii_case("section")) {
            let Some(rest) = line[7..].strip_prefix(|c: char| c.is_whitespace()) else {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    "Timeline section cannot be empty",
                ));
            };
            sections.push(strip_mermaid_quotes(rest));
            entries.push(TimelineEntry::Section { label: sections[sections.len() - 1].clone() });
            current_period = None;
            continue;
        }

        if let Some(rest) = line.strip_prefix(':') {
            let Some(period_index) = current_period else {
                return Err(syntax_error(
                    source.line_number,
                    &line,
                    "Timeline continuation requires a preceding period",
                ));
            };
            let mut events = parse_events(rest, source.line_number, &line)?;
            periods[period_index].events.append(&mut events);
            continue;
        }

        let mut fields = split_event_fields(&line);
        let period_label = strip_mermaid_quotes(&fields.remove(0));
        if period_label.is_empty() {
            return Err(syntax_error(source.line_number, &line, "Timeline period cannot be empty"));
        }
        let events = if fields.is_empty() {
            Vec::new()
        } else {
            parse_event_fields(fields, source.line_number, &line)?
        };
        periods.push(TimelinePeriod { period: period_label, events });
        entries.push(TimelineEntry::Period(periods.len() - 1));
        current_period = Some(periods.len() - 1);
    }

    Ok(TimelineDiagram { direction, title, sections, periods, entries })
}

/// `parseEvents`.
fn parse_events(
    value: &str,
    line_number: usize,
    source_line: &str,
) -> Result<Vec<String>, MermaidSyntaxError> {
    parse_event_fields(split_event_fields(value), line_number, source_line)
}

/// `parseEventFields` — rejects empty events.
fn parse_event_fields(
    fields: Vec<String>,
    line_number: usize,
    source_line: &str,
) -> Result<Vec<String>, MermaidSyntaxError> {
    let events: Vec<String> =
        fields.into_iter().map(|field| strip_mermaid_quotes(&field)).collect();
    if events.is_empty() || events.iter().any(String::is_empty) {
        return Err(syntax_error(line_number, source_line, "Timeline event cannot be empty"));
    }
    Ok(events)
}

/// `splitEventFields` — splits on `:` outside quotes, where a quote
/// only opens at the start of a field and a separator needs trailing
/// whitespace (or end-of-line).
fn split_event_fields(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut fields: Vec<String> = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0usize;
    for index in 0..chars.len() {
        let char = chars[index];
        if char == '"' || char == '\'' {
            if quote == Some(char) {
                quote = None;
            } else if quote.is_none()
                && chars[start..index].iter().collect::<String>().trim().is_empty()
            {
                quote = Some(char);
            }
            continue;
        }
        let next = chars.get(index + 1);
        if char != ':' || quote.is_some() || next.is_some_and(|next| !next.is_whitespace()) {
            continue;
        }
        fields.push(chars[start..index].iter().collect());
        start = index + 1;
    }
    fields.push(chars[start..].iter().collect());
    fields
}

const JOIN_WIDTH: i64 = SECTION_FADE_STYLES.len() as i64;
const SPINE_OFFSET: i64 = JOIN_WIDTH + 1;
const EVENT_OFFSET: i64 = 3;

/// `drawTimelineDiagramGrid`.
pub fn draw_timeline_diagram_grid(
    diagram: &TimelineDiagram,
    _options: &TimelineRenderOptions,
) -> TimelineGrid {
    let mut period_heights: Vec<i64> = Vec::new();
    let mut left_width = 0i64;
    let mut right_width = 0i64;
    let mut body_height = 0i64;

    for entry in &diagram.entries {
        match entry {
            TimelineEntry::Section { label } => {
                let lines = split_diagram_lines(label);
                body_height += lines.len() as i64 + 1;
                for line in &lines {
                    left_width = left_width.max(diagram_text_width(line) as i64);
                }
            }
            TimelineEntry::Period(index) => {
                let index = *index;
                let period = &diagram.periods[index];
                let event_height: i64 =
                    period.events.iter().map(|event| split_diagram_lines(event).len() as i64).sum();
                let height = period_lines(&period.period).len().max(event_height as usize) as i64;
                period_heights.push(height);
                for line in period_lines(&period.period) {
                    left_width = left_width.max(diagram_text_width(&line) as i64);
                }
                for event in &period.events {
                    for line in split_diagram_lines(event) {
                        right_width = right_width.max(diagram_text_width(&line) as i64);
                    }
                }
                body_height += height + 1;
            }
        }
    }

    let title_lines = diagram.title.as_deref().map(split_diagram_lines).unwrap_or_default();
    let body_width = if diagram.entries.is_empty() {
        0
    } else {
        left_width + SPINE_OFFSET + EVENT_OFFSET + right_width + 1
    };
    let mut title_width = 0i64;
    for line in &title_lines {
        title_width = title_width.max(diagram_text_width(line) as i64);
    }
    let width = body_width.max(title_width);
    let title_height = if title_lines.is_empty() {
        0
    } else {
        title_lines.len() as i64 + i64::from(!diagram.entries.is_empty())
    };
    if width == 0 {
        return DiagramCanvas::new(0, 0, None)
            .unwrap_or_else(|_| DiagramCanvas::new(1, 1, None).unwrap());
    }

    let mut grid: TimelineGrid =
        DiagramCanvas::new(width, title_height + body_height, None).unwrap();
    for (index, line) in title_lines.iter().enumerate() {
        grid.set_text(
            (width - diagram_text_width(line) as i64).div_euclid(2),
            index as i64,
            line,
            Some(TimelineCellStyle::Title),
        );
    }
    if diagram.entries.is_empty() {
        return grid;
    }

    let spine_x = left_width + SPINE_OFFSET;
    let mut y = title_height;
    let mut rail_started = false;
    let mut period_cursor = 0usize;
    for entry in &diagram.entries {
        match entry {
            TimelineEntry::Section { label } => {
                let lines = split_diagram_lines(label);
                for (index, line) in lines.iter().enumerate() {
                    grid.set_text(
                        left_width - diagram_text_width(line) as i64,
                        y + index as i64,
                        line,
                        Some(TimelineCellStyle::Section),
                    );
                    if index > 0 {
                        grid.set_cell(
                            spine_x,
                            y + index as i64,
                            "│",
                            Some(TimelineCellStyle::Spine),
                        );
                    }
                }
                draw_join(&mut grid, left_width, y, &SECTION_FADE_STYLES);
                grid.set_cell(
                    spine_x,
                    y,
                    if rail_started { "┤" } else { "┐" },
                    Some(TimelineCellStyle::Spine),
                );
                grid.set_cell(spine_x, y + lines.len() as i64, "│", Some(TimelineCellStyle::Spine));
                rail_started = true;
                y += lines.len() as i64 + 1;
            }
            TimelineEntry::Period(index) => {
                let index = *index;
                let period = &diagram.periods[index];
                let period_lines = period_lines(&period.period);
                let height = period_heights[period_cursor];
                period_cursor += 1;
                for row in 0..=height {
                    grid.set_cell(spine_x, y + row, "│", Some(TimelineCellStyle::Spine));
                }
                rail_started = true;
                grid.set_cell(spine_x, y, "●", Some(TimelineCellStyle::Spine));
                for (index, line) in period_lines.iter().enumerate() {
                    grid.set_text(
                        left_width - diagram_text_width(line) as i64,
                        y + index as i64,
                        line,
                        Some(TimelineCellStyle::Period),
                    );
                }
                draw_join(&mut grid, left_width, y, &PERIOD_FADE_STYLES);

                let mut event_y = y;
                for event in &period.events {
                    for (index, line) in split_diagram_lines(event).into_iter().enumerate() {
                        grid.set_text(
                            spine_x + EVENT_OFFSET,
                            event_y + index as i64,
                            &line,
                            Some(TimelineCellStyle::Event),
                        );
                    }
                    event_y += split_diagram_lines(event).len() as i64;
                }
                y += height + 1;
            }
        }
    }
    let _ = rail_started;
    grid
}

fn period_lines(period: &str) -> Vec<String> {
    split_diagram_lines(period)
}

/// `drawJoin` — the three-cell fade ramp from the label to the spine.
fn draw_join(grid: &mut TimelineGrid, x: i64, y: i64, styles: &[TimelineCellStyle; 3]) {
    #![allow(clippy::trivially_copy_pass_by_ref)]
    for (index, style) in styles.iter().enumerate() {
        grid.set_cell(x + index as i64 + 1, y, "─", Some(*style));
    }
}

/// `renderTimelineGridText` — `toString({ trimBottom: true })`.
#[must_use]
pub fn render_timeline_grid_text(grid: &TimelineGrid) -> String {
    grid.to_text(TextOptions { trim_top: false, trim_bottom: true })
}

/// `renderTimelineDiagram`.
pub fn render_timeline_diagram(content: &str) -> Result<String, MermaidSyntaxError> {
    let diagram = parse_mermaid_timeline_diagram(content)?;
    Ok(render_timeline_grid_text(&draw_timeline_diagram_grid(
        &diagram,
        &TimelineRenderOptions::default(),
    )))
}

/// `TimelineStyleColors` — `resolveTimelineStyleColors`.
#[derive(Debug, Clone, Copy)]
pub struct TimelineStyleColors {
    pub title: Rgba,
    pub section: Rgba,
    pub period: Rgba,
    pub spine: Rgba,
    pub event: Rgba,
    pub section_fade1: Rgba,
    pub section_fade2: Rgba,
    pub section_fade3: Rgba,
    pub period_fade1: Rgba,
    pub period_fade2: Rgba,
    pub period_fade3: Rgba,
}

impl TimelineStyleColors {
    #[must_use]
    pub fn color(&self, style: Option<TimelineCellStyle>) -> Option<Rgba> {
        let style = style?;
        Some(match style {
            TimelineCellStyle::Title => self.title,
            TimelineCellStyle::Section => self.section,
            TimelineCellStyle::Period => self.period,
            TimelineCellStyle::Spine => self.spine,
            TimelineCellStyle::Event => self.event,
            TimelineCellStyle::SectionFade1 => self.section_fade1,
            TimelineCellStyle::SectionFade2 => self.section_fade2,
            TimelineCellStyle::SectionFade3 => self.section_fade3,
            TimelineCellStyle::PeriodFade1 => self.period_fade1,
            TimelineCellStyle::PeriodFade2 => self.period_fade2,
            TimelineCellStyle::PeriodFade3 => self.period_fade3,
        })
    }
}

/// `TimelineColorInput`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TimelineColorInput {
    pub title: Option<Rgba>,
    pub section: Option<Rgba>,
    pub period: Option<Rgba>,
    pub spine: Option<Rgba>,
    pub event: Option<Rgba>,
}

/// `resolveTimelineStyleColors`.
#[must_use]
pub fn resolve_timeline_style_colors(input: TimelineColorInput) -> TimelineStyleColors {
    let section = input.section.unwrap_or(Rgba::from_ints(154, 184, 169, 255));
    let period = input.period.unwrap_or(Rgba::from_ints(230, 177, 126, 255));
    let spine = input.spine.unwrap_or(Rgba::from_ints(111, 138, 126, 255));
    let blend =
        |from: Rgba, amount: f64| blend_color(Some(from), Some(spine), amount).unwrap_or(from);
    TimelineStyleColors {
        title: input.title.unwrap_or(Rgba::from_ints(228, 239, 232, 255)),
        section,
        period,
        spine,
        event: input.event.unwrap_or(Rgba::from_ints(134, 225, 200, 255)),
        section_fade1: blend(section, 0.5),
        section_fade2: blend(section, 0.67),
        section_fade3: blend(section, 0.83),
        period_fade1: blend(period, 0.5),
        period_fade2: blend(period, 0.67),
        period_fade3: blend(period, 0.83),
    }
}

#[cfg(test)]
mod tests;
