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

//! `core/drawing.ts` — line glyphs, arrows, frames, diamonds, and
//! orthogonal path rendering.

use std::collections::HashSet;

use crate::render::mermaid::core::geometry::{
    bounds_side_point, direction_between, Bounds, Direction, Point, Side,
};

/// `DiagramLineCornerStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerStyle {
    Square,
    Rounded,
}

/// `DiagramLineStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Single,
    Heavy,
    Dashed,
}

/// `DiagramArrowHeadStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowHeadStyle {
    Filled,
    Line,
}

/// `BorderCharacters` (the `@opentui/core` border tables — the two
/// styles the diagram renderers use).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderCharacters {
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    pub horizontal: &'static str,
    pub vertical: &'static str,
    pub left_t: &'static str,
    pub right_t: &'static str,
    pub top_t: &'static str,
    pub bottom_t: &'static str,
    pub cross: &'static str,
}

/// `BorderChars.rounded`.
pub const BORDER_ROUNDED: BorderCharacters = BorderCharacters {
    top_left: "╭",
    top_right: "╮",
    bottom_left: "╰",
    bottom_right: "╯",
    horizontal: "─",
    vertical: "│",
    left_t: "├",
    right_t: "┤",
    top_t: "┬",
    bottom_t: "┴",
    cross: "┼",
};

/// `BorderChars.double`.
pub const BORDER_DOUBLE: BorderCharacters = BorderCharacters {
    top_left: "╔",
    top_right: "╗",
    bottom_left: "╚",
    bottom_right: "╝",
    horizontal: "═",
    vertical: "║",
    left_t: "╠",
    right_t: "╣",
    top_t: "╦",
    bottom_t: "╩",
    cross: "╬",
};

/// `DIAGRAM_ARROW_HEADS`.
pub fn is_arrow_head(char: &str) -> bool {
    matches!(char, "▶" | "◀" | "▼" | "▲" | "→" | "←" | "↓" | "↑")
}

fn is_heavy_glyph(char: &str) -> bool {
    matches!(char, "━" | "┃" | "┏" | "┓" | "┗" | "┛" | "┣" | "┫" | "┳" | "┻" | "╋")
}

/// `lineDirections`.
fn line_directions(char: &str) -> Option<&'static [Direction]> {
    Some(match char {
        "─" => &[Direction::Left, Direction::Right],
        "│" => &[Direction::Up, Direction::Down],
        "━" => &[Direction::Left, Direction::Right],
        "┃" => &[Direction::Up, Direction::Down],
        "┌" | "╭" | "┏" => &[Direction::Right, Direction::Down],
        "┐" | "╮" | "┓" => &[Direction::Left, Direction::Down],
        "└" | "╰" | "┗" => &[Direction::Up, Direction::Right],
        "┘" | "╯" | "┛" => &[Direction::Up, Direction::Left],
        "├" | "┣" => &[Direction::Up, Direction::Down, Direction::Right],
        "┤" | "┫" => &[Direction::Up, Direction::Down, Direction::Left],
        "┬" | "┳" => &[Direction::Left, Direction::Right, Direction::Down],
        "┴" | "┻" => &[Direction::Left, Direction::Right, Direction::Up],
        "┼" | "╋" => &[Direction::Up, Direction::Down, Direction::Left, Direction::Right],
        _ => return None,
    })
}

/// `diagramLineGlyph`.
#[must_use]
pub fn diagram_line_glyph(
    directions: &HashSet<Direction>,
    corner_style: CornerStyle,
    line_style: LineStyle,
) -> &'static str {
    let up = directions.contains(&Direction::Up);
    let down = directions.contains(&Direction::Down);
    let left = directions.contains(&Direction::Left);
    let right = directions.contains(&Direction::Right);
    if line_style == LineStyle::Heavy {
        let chars = heavy_chars();
        return glyph(up, down, left, right, corner_style, chars);
    }
    glyph(up, down, left, right, corner_style, single_chars())
}

#[derive(Clone, Copy)]
struct GlyphTable {
    cross: &'static str,
    left_t: &'static str,
    right_t: &'static str,
    top_t: &'static str,
    bottom_t: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
    top_left: &'static str,
    top_right: &'static str,
    vertical: &'static str,
    horizontal: &'static str,
}

fn single_chars() -> GlyphTable {
    GlyphTable {
        cross: "┼",
        left_t: "├",
        right_t: "┤",
        top_t: "┬",
        bottom_t: "┴",
        bottom_left: "└",
        bottom_right: "┘",
        top_left: "┌",
        top_right: "┐",
        vertical: "│",
        horizontal: "─",
    }
}

fn heavy_chars() -> GlyphTable {
    GlyphTable {
        cross: "╋",
        left_t: "┣",
        right_t: "┫",
        top_t: "┳",
        bottom_t: "┻",
        bottom_left: "┗",
        bottom_right: "┛",
        top_left: "┏",
        top_right: "┓",
        vertical: "┃",
        horizontal: "━",
    }
}

// The four booleans mirror the upstream `glyph(up, down, left, right, ...)`
// signature used by `diagramLineGlyph`.
#[allow(clippy::fn_params_excessive_bools)]
fn glyph(
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    corner_style: CornerStyle,
    chars: GlyphTable,
) -> &'static str {
    if up && down && left && right {
        return chars.cross;
    }
    if up && down && right {
        return chars.left_t;
    }
    if up && down && left {
        return chars.right_t;
    }
    if left && right && down {
        return chars.top_t;
    }
    if left && right && up {
        return chars.bottom_t;
    }
    if up && right {
        return if corner_style == CornerStyle::Rounded { "╰" } else { chars.bottom_left };
    }
    if up && left {
        return if corner_style == CornerStyle::Rounded { "╯" } else { chars.bottom_right };
    }
    if down && right {
        return if corner_style == CornerStyle::Rounded { "╭" } else { chars.top_left };
    }
    if down && left {
        return if corner_style == CornerStyle::Rounded { "╮" } else { chars.top_right };
    }
    if up || down {
        return chars.vertical;
    }
    chars.horizontal
}

/// `mergeDiagramLineGlyph`.
#[must_use]
pub fn merge_diagram_line_glyph(
    existing: &str,
    incoming: &str,
    corner_style: CornerStyle,
) -> Option<&'static str> {
    let existing_directions = line_directions(existing)?;
    let incoming_directions = line_directions(incoming)?;
    let mut directions = HashSet::new();
    directions.extend(existing_directions.iter().copied());
    directions.extend(incoming_directions.iter().copied());
    let heavy = is_heavy_glyph(existing) && is_heavy_glyph(incoming);
    Some(diagram_line_glyph(
        &directions,
        corner_style,
        if heavy { LineStyle::Heavy } else { LineStyle::Single },
    ))
}

/// `diagramArrowHead`.
#[must_use]
pub fn diagram_arrow_head(direction: Direction, style: ArrowHeadStyle) -> &'static str {
    if style == ArrowHeadStyle::Line {
        return match direction {
            Direction::Right => "→",
            Direction::Left => "←",
            Direction::Up => "↑",
            Direction::Down => "↓",
        };
    }
    match direction {
        Direction::Right => "▶",
        Direction::Left => "◀",
        Direction::Up => "▲",
        Direction::Down => "▼",
    }
}

/// `diagramArrowHeadBetween`.
#[must_use]
pub fn diagram_arrow_head_between(from: Point, to: Point, style: ArrowHeadStyle) -> &'static str {
    match direction_between(from, to) {
        Some(direction) => diagram_arrow_head(direction, style),
        None => diagram_arrow_head(Direction::Right, style),
    }
}

/// `drawDiagramFrame` — writes through the `set_cell` closure.
pub fn draw_diagram_frame(
    bounds: Bounds,
    chars: &BorderCharacters,
    set_cell: &mut dyn FnMut(i64, i64, &str),
) {
    set_cell(bounds.left, bounds.top, chars.top_left);
    set_cell(bounds.left + bounds.width - 1, bounds.top, chars.top_right);
    set_cell(bounds.left, bounds.top + bounds.height - 1, chars.bottom_left);
    set_cell(bounds.left + bounds.width - 1, bounds.top + bounds.height - 1, chars.bottom_right);
    for x in (bounds.left + 1)..(bounds.left + bounds.width - 1) {
        set_cell(x, bounds.top, chars.horizontal);
        set_cell(x, bounds.top + bounds.height - 1, chars.horizontal);
    }
    for y in (bounds.top + 1)..(bounds.top + bounds.height - 1) {
        set_cell(bounds.left, y, chars.vertical);
        set_cell(bounds.left + bounds.width - 1, y, chars.vertical);
    }
}

/// `fillDiagramFrameInterior`.
pub fn fill_diagram_frame_interior(bounds: Bounds, set_cell: &mut dyn FnMut(i64, i64)) {
    for y in (bounds.top + 1)..(bounds.top + bounds.height - 1) {
        for x in (bounds.left + 1)..(bounds.left + bounds.width - 1) {
            set_cell(x, y);
        }
    }
}

/// `DIAGRAM_DIAMOND_CHARS`.
#[derive(Debug, Clone, Copy)]
pub struct DiamondCharacters {
    pub top_left: &'static str,
    pub top_right: &'static str,
    pub upper_left: &'static str,
    pub upper_left_join: &'static str,
    pub upper_right_join: &'static str,
    pub upper_right: &'static str,
    pub vertical: &'static str,
    pub lower_left: &'static str,
    pub lower_left_join: &'static str,
    pub lower_right_join: &'static str,
    pub lower_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom_right: &'static str,
    pub horizontal: &'static str,
}

pub const DIAGRAM_DIAMOND_CHARS: DiamondCharacters = DiamondCharacters {
    top_left: "╭",
    top_right: "╮",
    upper_left: "╭",
    upper_left_join: "╯",
    upper_right_join: "╰",
    upper_right: "╮",
    vertical: "│",
    lower_left: "╰",
    lower_left_join: "╮",
    lower_right_join: "╭",
    lower_right: "╯",
    bottom_left: "╰",
    bottom_right: "╯",
    horizontal: "─",
};

/// `diagramDiamondCharactersFromBorder`.
#[must_use]
pub fn diamond_characters_from_border(chars: &BorderCharacters) -> DiamondCharacters {
    DiamondCharacters {
        top_left: chars.top_left,
        top_right: chars.top_right,
        upper_left: chars.top_left,
        upper_left_join: chars.bottom_right,
        upper_right_join: chars.bottom_left,
        upper_right: chars.top_right,
        vertical: chars.vertical,
        lower_left: chars.bottom_left,
        lower_left_join: chars.top_right,
        lower_right_join: chars.top_left,
        lower_right: chars.bottom_right,
        bottom_left: chars.bottom_left,
        bottom_right: chars.bottom_right,
        horizontal: chars.horizontal,
    }
}

/// `drawDiagramDiamond`.
pub fn draw_diagram_diamond(
    bounds: Bounds,
    set_cell: &mut dyn FnMut(i64, i64, &str),
    chars: &DiamondCharacters,
) {
    let left = bounds.left;
    let right = bounds.left + bounds.width - 1;
    let top = bounds.top;
    let bottom = bounds.top + bounds.height - 1;
    let cap_inset = 2.min(((bounds.width - 1) / 2).max(1));
    let cap_left = left + cap_inset;
    let cap_right = right - cap_inset;

    set_cell(cap_left, top, chars.top_left);
    for x in (cap_left + 1)..cap_right {
        set_cell(x, top, chars.horizontal);
    }
    set_cell(cap_right, top, chars.top_right);

    set_cell(left, top + 1, chars.upper_left);
    for x in (left + 1)..cap_left {
        set_cell(x, top + 1, chars.horizontal);
    }
    set_cell(cap_left, top + 1, chars.upper_left_join);
    set_cell(cap_right, top + 1, chars.upper_right_join);
    for x in (cap_right + 1)..right {
        set_cell(x, top + 1, chars.horizontal);
    }
    set_cell(right, top + 1, chars.upper_right);

    for y in (top + 2)..(bottom - 1) {
        set_cell(left, y, chars.vertical);
        set_cell(right, y, chars.vertical);
    }

    set_cell(left, bottom - 1, chars.lower_left);
    for x in (left + 1)..cap_left {
        set_cell(x, bottom - 1, chars.horizontal);
    }
    set_cell(cap_left, bottom - 1, chars.lower_left_join);
    set_cell(cap_right, bottom - 1, chars.lower_right_join);
    for x in (cap_right + 1)..right {
        set_cell(x, bottom - 1, chars.horizontal);
    }
    set_cell(right, bottom - 1, chars.lower_right);

    set_cell(cap_left, bottom, chars.bottom_left);
    for x in (cap_left + 1)..cap_right {
        set_cell(x, bottom, chars.horizontal);
    }
    set_cell(cap_right, bottom, chars.bottom_right);
}

/// `drawOrthogonalPath`.
pub fn draw_orthogonal_path(
    points: &[Point],
    set_cell: &mut dyn FnMut(i64, i64, &str),
    corner_style: CornerStyle,
    line_style: LineStyle,
) {
    for (index, window) in points.windows(2).enumerate() {
        let from = window[0];
        let to = window[1];
        let Some(direction) = direction_between(from, to) else { continue };
        let mut directions = HashSet::new();
        if matches!(direction, Direction::Left | Direction::Right) {
            directions.insert(Direction::Left);
            directions.insert(Direction::Right);
        } else {
            directions.insert(Direction::Up);
            directions.insert(Direction::Down);
        }
        let glyph = diagram_line_glyph(
            &directions,
            CornerStyle::Square,
            if line_style == LineStyle::Heavy { LineStyle::Heavy } else { LineStyle::Single },
        );
        // The FIRST segment paints its start point; later segments start
        // one cell in (the corner pass below owns the joint).
        let first = index == 0;
        #[allow(clippy::bool_to_int_with_if)]
        let mut step = if first { 0 } else { 1 };
        crate::render::mermaid::core::geometry::walk_orthogonal_segment(
            from,
            to,
            first,
            &mut |point| {
                if line_style != LineStyle::Dashed || step % 2 == 0 {
                    set_cell(point.x, point.y, glyph);
                }
                step += 1;
                true
            },
        );
    }

    for index in 1..points.len().saturating_sub(1) {
        let previous = points[index - 1];
        let current = points[index];
        let next = points[index + 1];
        let from_direction = direction_between(current, previous);
        let to_direction = direction_between(current, next);
        let mut directions = HashSet::new();
        if let Some(direction) = from_direction {
            directions.insert(direction);
        }
        if let Some(direction) = to_direction {
            directions.insert(direction);
        }
        set_cell(
            current.x,
            current.y,
            diagram_line_glyph(
                &directions,
                corner_style,
                if line_style == LineStyle::Heavy { LineStyle::Heavy } else { LineStyle::Single },
            ),
        );
    }
}

/// `drawOrthogonalPath` segment walker re-export (used by route-cell
/// occupancy accounting).
pub use crate::render::mermaid::core::geometry::walk_orthogonal_segment;

/// `boundsSidePoint(surface="outside")` convenience.
#[must_use]
pub fn outside_side_point(bounds: Bounds, side: Side) -> Point {
    bounds_side_point(bounds, side, false)
}
