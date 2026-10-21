//! The cell-grid layout engine (upstream `packages/latex/src/layout.ts`).

use crate::render::latex::types::{
    BracePosition, MathNode, MathVariant, MatrixEnvironment, NumeratorAlign, ParseOptions,
    SymbolRole,
};

/// `MathStyle`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MathStyle {
    pub color: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
}

/// `MathCell`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathCell {
    pub char: String,
    pub style: Option<MathStyle>,
}

/// `MathLayout`.
#[derive(Debug, Clone, PartialEq)]
pub struct MathLayout {
    pub width: usize,
    pub height: usize,
    pub baseline: usize,
    pub cells: Vec<Vec<Option<MathCell>>>,
}

impl std::fmt::Display for MathLayout {
    /// `toString` — rows trimmed of trailing spaces, wide glyphs skip
    /// their continuation columns.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = self.render_text();
        f.write_str(&text)
    }
}

impl MathLayout {
    /// The `toString` body: trimmed rows joined by newlines.
    #[must_use]
    pub fn render_text(&self) -> String {
        let mut rows = Vec::with_capacity(self.height);
        for row in &self.cells {
            let mut output = String::new();
            let mut x = 0usize;
            while x < self.width {
                let Some(cell) = &row[x] else {
                    output.push(' ');
                    x += 1;
                    continue;
                };
                output.push_str(&cell.char);
                let width = cell_width(&cell.char);
                if width > 1 {
                    x += width - 1;
                }
                x += 1;
            }
            rows.push(output.trim_end().to_string());
        }
        rows.join("\n")
    }
}

/// `RenderLatexOptions`.
#[derive(Debug, Clone, Default)]
pub struct RenderLatexOptions {
    pub display_mode: Option<bool>,
    pub compact_scripts: Option<bool>,
    pub color: Option<String>,
    pub parse: ParseDefaults,
}

/// The parse-side defaults folded out of `RenderLatexOptions`
/// (upstream `RenderLatexOptions extends ParseOptions`; the markdown
/// renderer passes `strict: true`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseDefaults {
    pub strict: bool,
}

impl RenderLatexOptions {
    #[must_use]
    pub fn parse_options(&self) -> ParseOptions {
        ParseOptions { strict: self.parse.strict, ..ParseOptions::default() }
    }
}

#[derive(Debug, Clone)]
struct Box_ {
    width: usize,
    height: usize,
    baseline: usize,
    cells: Vec<Vec<Option<MathCell>>>,
}

#[derive(Debug, Clone)]
struct LayoutContext {
    display_mode: bool,
    compact_scripts: bool,
    style: Option<MathStyleBits>,
    variant: Option<MathVariant>,
}

/// The carried style bits (upstream's optional `MathStyle` overlay).
#[derive(Debug, Clone, Default, PartialEq)]
struct MathStyleBits {
    color: Option<String>,
    bold: bool,
    italic: bool,
    dim: bool,
}

impl MathStyleBits {
    fn merge(self, other: Self) -> Self {
        Self {
            color: other.color.or(self.color),
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            dim: self.dim || other.dim,
        }
    }
}

fn superscript(char: char) -> Option<&'static str> {
    Some(match char {
        '0' => "⁰",
        '1' => "¹",
        '2' => "²",
        '3' => "³",
        '4' => "⁴",
        '5' => "⁵",
        '6' => "⁶",
        '7' => "⁷",
        '8' => "⁸",
        '9' => "⁹",
        '+' => "⁺",
        '-' => "⁻",
        '=' => "⁼",
        '(' => "⁽",
        ')' => "⁾",
        'n' => "ⁿ",
        'i' => "ⁱ",
        _ => return None,
    })
}

fn subscript(char: char) -> Option<&'static str> {
    Some(match char {
        '0' => "₀",
        '1' => "₁",
        '2' => "₂",
        '3' => "₃",
        '4' => "₄",
        '5' => "₅",
        '6' => "₆",
        '7' => "₇",
        '8' => "₈",
        '9' => "₉",
        '+' => "₊",
        '-' => "₋",
        '=' => "₌",
        '(' => "₍",
        ')' => "₎",
        'a' => "ₐ",
        'e' => "ₑ",
        'h' => "ₕ",
        'i' => "ᵢ",
        'j' => "ⱼ",
        'k' => "ₖ",
        'l' => "ₗ",
        'm' => "ₘ",
        'n' => "ₙ",
        'o' => "ₒ",
        'p' => "ₚ",
        'r' => "ᵣ",
        's' => "ₛ",
        't' => "ₜ",
        'u' => "ᵤ",
        'v' => "ᵥ",
        'x' => "ₓ",
        _ => return None,
    })
}

/// `layoutMath`.
#[must_use]
pub fn layout_math(node: &MathNode, options: &RenderLatexOptions) -> MathLayout {
    let context = LayoutContext {
        display_mode: options.display_mode.unwrap_or(true),
        compact_scripts: options.compact_scripts.unwrap_or(true),
        style: options
            .color
            .clone()
            .map(|color| MathStyleBits { color: Some(color), ..Default::default() }),
        variant: None,
    };
    as_public_layout(layout_node(node, &context))
}

fn layout_node(node: &MathNode, context: &LayoutContext) -> Box_ {
    match node {
        MathNode::Row(body) => layout_row(body, context),
        MathNode::Symbol { value, .. }
        | MathNode::Text(value)
        | MathNode::Operator { value, .. } => {
            text_box(&apply_variant(value, context.variant), context.style.as_ref())
        }
        MathNode::Space(width) => blank(*width as usize, 1, 0),
        MathNode::Fraction { numerator, denominator, bar, numerator_align } => {
            layout_fraction(numerator, denominator, *bar, *numerator_align, context)
        }
        MathNode::Root { body, index } => layout_root(body, index.as_deref(), context),
        MathNode::Scripts { base, superscript, subscript } => {
            layout_scripts(base, superscript.as_deref(), subscript.as_deref(), context)
        }
        MathNode::Delimited { left, body, right } => layout_delimited(left, body, right, context),
        MathNode::Matrix { rows, environment, columns } => {
            layout_matrix(rows, *environment, columns.as_deref(), context)
        }
        MathNode::Brace { body, position } => layout_brace(body, *position, context),
        MathNode::Accent { accent, body } => layout_accent(*accent, body, context),
        MathNode::Variant { variant, body } => layout_node(body, &with_variant(context, *variant)),
        MathNode::OverUnder { base, over, under } => {
            layout_over_under(base, over.as_deref(), under.as_deref(), context)
        }
        MathNode::Color { color, body } => {
            let style = MathStyleBits {
                color: Some(color.clone()),
                ..context.style.clone().unwrap_or_default()
            };
            layout_node(body, &LayoutContext { style: Some(style), ..context.clone() })
        }
    }
}

fn layout_row(nodes: &[MathNode], context: &LayoutContext) -> Box_ {
    if nodes.is_empty() {
        return blank(0, 1, 0);
    }

    let mut boxes: Vec<Box_> = Vec::new();
    let mut previous_role: Option<SymbolRole> = None;

    for (index, node) in nodes.iter().enumerate() {
        let raw_role = node_role(node);
        let role =
            normalize_binary_role(raw_role, previous_role, next_significant_role(nodes, index + 1));
        if needs_math_space(previous_role, role, boxes.len()) {
            boxes.push(blank(1, 1, 0));
        }
        boxes.push(layout_node(node, context));
        if !matches!(node, MathNode::Space(_)) {
            previous_role = role.or(Some(SymbolRole::Ordinary));
        }
    }

    hpack(&boxes)
}

fn layout_fraction(
    numerator: &MathNode,
    denominator: &MathNode,
    bar: bool,
    numerator_align: Option<NumeratorAlign>,
    context: &LayoutContext,
) -> Box_ {
    let numerator = layout_node(numerator, context);
    let denominator = layout_node(denominator, context);
    let width = numerator.width.max(denominator.width) + 2;
    // Barless fractions (binomials) still reserve an axis row.
    let gap = 1usize;
    let height = numerator.height + denominator.height + gap;
    // TeX places a fraction's math axis on its rule.
    let baseline = numerator.height;
    let mut result = blank(width, height, baseline);

    let numerator_x = match numerator_align {
        Some(NumeratorAlign::Left) => 1,
        Some(NumeratorAlign::Right) => width - numerator.width - 1,
        None => floor_div(width - numerator.width, 2),
    };
    overlay(&mut result, &numerator, numerator_x, 0);
    if bar {
        draw_horizontal(&mut result, numerator.height, 0, width, "─", context.style.as_ref());
    }
    overlay(
        &mut result,
        &denominator,
        floor_div(width - denominator.width, 2),
        numerator.height + gap,
    );
    result
}

fn layout_root(
    body_node: &MathNode,
    index_node: Option<&MathNode>,
    context: &LayoutContext,
) -> Box_ {
    let body = layout_node(body_node, context);
    let index = index_node.map(|node| layout_node(node, context));
    let index_width = index.as_ref().map_or(0, |index| index.width.saturating_sub(1));
    // The index ends beside the overbar, never inside the hook or radicand.
    let top = index.as_ref().map_or(0, |index| index.height.saturating_sub(1));
    let body_x = index_width + 2;
    let width = body_x + body.width;
    let height = top + body.height + 1;
    let baseline = top + body.baseline + 1;
    let mut result = blank(width, height, baseline);

    set_cell(&mut result, body_x - 1, top, "╭", context.style.as_ref());
    draw_horizontal(&mut result, top, body_x, body.width, "─", context.style.as_ref());
    for y in (top + 1)..height.saturating_sub(1) {
        set_cell(&mut result, body_x - 1, y, "│", context.style.as_ref());
    }
    set_cell(&mut result, body_x - 2, height - 1, "╰", context.style.as_ref());
    set_cell(&mut result, body_x - 1, height - 1, "╯", context.style.as_ref());
    overlay(&mut result, &body, body_x, top + 1);
    if let Some(index) = index {
        overlay(&mut result, &index, 0, 0);
    }
    result
}

fn layout_scripts(
    base_node: &MathNode,
    superscript_node: Option<&MathNode>,
    subscript_node: Option<&MathNode>,
    context: &LayoutContext,
) -> Box_ {
    // Upstream keeps the script node unless its text IS "" (undefined text
    // — a fraction — stays and forces the stacked layout).
    let keeps = |node: &MathNode| simple_node_text(node) != Some(String::new());
    let superscript_node = superscript_node.filter(|node| keeps(node));
    let subscript_node = subscript_node.filter(|node| keeps(node));
    let base_is_brace = matches!(base_node, MathNode::Brace { .. });
    let base_is_limited_operator =
        matches!(base_node, MathNode::Operator { limits: true, .. }) && context.display_mode;
    if base_is_brace || base_is_limited_operator {
        return layout_over_under(base_node, superscript_node, subscript_node, context);
    }

    let base = layout_node(base_node, context);
    if context.compact_scripts && base.height == 1 {
        // mapScript("") → "" (Some): an absent script still compacts; only a
        // present-but-unmappable script (e.g. a fraction, text undefined)
        // forces the stacked layout.
        let superscript_text = match superscript_node {
            None => Some(String::new()),
            Some(node) => simple_node_text(node),
        };
        let subscript_text = match subscript_node {
            None => Some(String::new()),
            Some(node) => simple_node_text(node),
        };
        if let (Some(superscript), Some(subscript)) = (
            superscript_text.as_deref().and_then(|text| map_script(text, superscript)),
            subscript_text.as_deref().and_then(|text| map_script(text, subscript)),
        ) {
            return hpack(&[
                base,
                text_box(&format!("{superscript}{subscript}"), context.style.as_ref()),
            ]);
        }
    }

    let superscript = superscript_node.map(|node| layout_node(node, context));
    let subscript = subscript_node.map(|node| layout_node(node, context));
    let script_width = superscript
        .as_ref()
        .map_or(0, |box_| box_.width)
        .max(subscript.as_ref().map_or(0, |box_| box_.width));
    let top_height = superscript.as_ref().map_or(0, |box_| box_.height);
    let bottom_height = subscript.as_ref().map_or(0, |box_| box_.height);
    let width = base.width + script_width;
    let height = top_height + base.height + bottom_height;
    let baseline = top_height + base.baseline;
    let mut result = blank(width, height, baseline);

    overlay(&mut result, &base, 0, top_height);
    if let Some(superscript) = &superscript {
        overlay(&mut result, superscript, base.width, 0);
    }
    if let Some(subscript) = &subscript {
        overlay(&mut result, subscript, base.width, top_height + base.height);
    }
    result
}

fn layout_over_under(
    base_node: &MathNode,
    over_node: Option<&MathNode>,
    under_node: Option<&MathNode>,
    context: &LayoutContext,
) -> Box_ {
    let base = layout_node(base_node, context);
    let over = over_node.map(|node| layout_node(node, context));
    let under = under_node.map(|node| layout_node(node, context));
    let width = base
        .width
        .max(over.as_ref().map_or(0, |box_| box_.width))
        .max(under.as_ref().map_or(0, |box_| box_.width));
    let over_height = over.as_ref().map_or(0, |box_| box_.height);
    let height = over_height + base.height + under.as_ref().map_or(0, |box_| box_.height);
    let baseline = over_height + base.baseline;
    let mut result = blank(width, height, baseline);

    if let Some(over) = &over {
        overlay(&mut result, over, floor_div(width - over.width, 2), 0);
    }
    overlay(&mut result, &base, floor_div(width - base.width, 2), over_height);
    if let Some(under) = &under {
        overlay(&mut result, under, floor_div(width - under.width, 2), over_height + base.height);
    }
    result
}

fn layout_delimited(left: &str, body_node: &MathNode, right: &str, context: &LayoutContext) -> Box_ {
    let body = layout_node(body_node, context);
    let left_box = delimiter_box(left, body.height, body.baseline, true, context.style.as_ref());
    let right_box = delimiter_box(right, body.height, body.baseline, false, context.style.as_ref());
    hpack(&[left_box, body, right_box])
}

#[allow(clippy::too_many_lines)]
fn layout_matrix(
    rows: &[Vec<MathNode>],
    environment: MatrixEnvironment,
    columns: Option<&str>,
    context: &LayoutContext,
) -> Box_ {
    let cell_rows: Vec<Vec<Box_>> = rows
        .iter()
        .map(|row| row.iter().map(|cell| layout_node(cell, context)).collect())
        .collect();
    let column_specs: Vec<char> =
        columns.unwrap_or("").chars().filter(|char| matches!(char, 'l' | 'c' | 'r')).collect();
    let column_count = column_specs.len().max(cell_rows.iter().map(Vec::len).max().unwrap_or(0));
    // rules[boundary]: pipe counts between/around alignment letters
    // (JS `split(/[lcr]/)` keeps every piece, including the trailing
    // right-edge rule).
    let mut rules: Vec<usize> = columns
        .map(|columns| columns.split(['l', 'c', 'r']).map(str::len).collect::<Vec<_>>())
        .unwrap_or_default();
    while rules.len() < column_count + 1 {
        rules.push(0);
    }
    let column_widths: Vec<usize> = (0..column_count)
        .map(|column| {
            cell_rows
                .iter()
                .map(|row| row.get(column).map_or(0, |cell| cell.width))
                .max()
                .unwrap_or(0)
        })
        .collect();
    let row_ascents: Vec<usize> = cell_rows
        .iter()
        .map(|row| row.iter().map(|cell| cell.baseline).max().unwrap_or(0))
        .collect();
    let row_descents: Vec<usize> = cell_rows
        .iter()
        .map(|row| {
            row.iter().map(|cell| cell.height.saturating_sub(cell.baseline + 1)).max().unwrap_or(0)
        })
        .collect();
    let row_heights: Vec<usize> = row_ascents
        .iter()
        .zip(&row_descents)
        .map(|(ascent, descent)| ascent + 1 + descent)
        .collect();
    let aligned = matches!(environment, MatrixEnvironment::Aligned | MatrixEnvironment::Align);
    let column_gap = if matches!(environment, MatrixEnvironment::Cases) || aligned { 2 } else { 1 };
    let gaps: Vec<usize> = (0..=column_count)
        .map(|boundary| {
            let edge = boundary == 0 || boundary == column_count;
            if rules.get(boundary).copied().unwrap_or(0) > 0 {
                rules[boundary] + if edge { 1 } else { 2 }
            } else if edge {
                0
            } else {
                column_gap
            }
        })
        .collect();
    let width = column_widths.iter().sum::<usize>() + gaps.iter().sum::<usize>();
    let height = 1usize.max(row_heights.iter().sum::<usize>() + rows.len().saturating_sub(1));
    let mut result = blank(width, height, floor_div(height, 2));
    let mut y = 0usize;

    for (row_index, cells) in cell_rows.iter().enumerate() {
        let mut x = gaps[0];
        for column in 0..column_count {
            if let Some(cell) = cells.get(column) {
                let column_width = column_widths[column];
                let alignment = column_specs.get(column).copied().unwrap_or(
                    if matches!(environment, MatrixEnvironment::Cases) {
                        'l'
                    } else if aligned {
                        if column % 2 == 0 {
                            'r'
                        } else {
                            'l'
                        }
                    } else {
                        'c'
                    },
                );
                let cell_x = x + match alignment {
                    'l' => 0,
                    'r' => column_width - cell.width,
                    _ => floor_div(column_width - cell.width, 2),
                };
                let cell_y = y + row_ascents[row_index] - cell.baseline;
                overlay(&mut result, cell, cell_x, cell_y);
            }
            x += column_widths[column] + gaps[column + 1];
        }
        y += row_heights[row_index] + 1;
    }

    let mut boundary_x = 0usize;
    for (boundary, gap) in gaps.iter().enumerate().take(column_count + 1) {
        for rule in 0..rules.get(boundary).copied().unwrap_or(0) {
            for row in 0..height {
                set_cell(
                    &mut result,
                    boundary_x + usize::from(boundary != 0) + rule,
                    row,
                    "│",
                    context.style.as_ref(),
                );
            }
        }
        boundary_x += gap + column_widths.get(boundary).copied().unwrap_or(0);
    }

    let delimiters = matrix_delimiters(environment);
    match delimiters {
        Some((left, right)) => {
            let baseline = result.baseline;
            hpack(&[
                delimiter_box(left, height, baseline, true, context.style.as_ref()),
                result,
                delimiter_box(right, height, baseline, false, context.style.as_ref()),
            ])
        }
        None => result,
    }
}

fn layout_brace(body_node: &MathNode, position: BracePosition, context: &LayoutContext) -> Box_ {
    let body = layout_node(body_node, context);
    let over = position == BracePosition::Over;
    let width = 3.max(body.width);
    let mut result = blank(width, body.height + 1, body.baseline + usize::from(over));
    let y = if over { 0 } else { body.height };
    overlay(&mut result, &body, floor_div(width - body.width, 2), usize::from(over));
    draw_horizontal(&mut result, y, 0, width, "─", context.style.as_ref());
    set_cell(&mut result, 0, y, if over { "╭" } else { "╰" }, context.style.as_ref());
    set_cell(&mut result, width - 1, y, if over { "╮" } else { "╯" }, context.style.as_ref());
    set_cell(
        &mut result,
        floor_div(width - 1, 2),
        y,
        if over { "┴" } else { "┬" },
        context.style.as_ref(),
    );
    result
}

fn layout_accent(
    accent: crate::render::latex::types::AccentKind,
    body_node: &MathNode,
    context: &LayoutContext,
) -> Box_ {
    use crate::render::latex::types::AccentKind;
    let body = layout_node(body_node, context);
    if accent == AccentKind::Underline {
        let mut result = blank(body.width, body.height + 1, body.baseline);
        overlay(&mut result, &body, 0, 0);
        draw_horizontal(&mut result, body.height, 0, body.width, "─", context.style.as_ref());
        return result;
    }

    let mut result = blank(body.width, body.height + 1, body.baseline + 1);
    overlay(&mut result, &body, 0, 1);
    let mark = match accent {
        AccentKind::Hat | AccentKind::WideHat => {
            if body.width == 1 {
                "^"
            } else {
                "⌢"
            }
        }
        AccentKind::Bar | AccentKind::Overline => "─",
        AccentKind::Vec => "→",
        AccentKind::Tilde => "~",
        AccentKind::Dot => "·",
        _ => "¨",
    };

    if matches!(accent, AccentKind::Bar | AccentKind::Overline) {
        draw_horizontal(&mut result, 0, 0, body.width, mark, context.style.as_ref());
    } else {
        let x = body.width.saturating_sub(cell_width(mark)) / 2;
        set_cell(&mut result, x, 0, mark, context.style.as_ref());
    }
    result
}

fn delimiter_box(
    delimiter: &str,
    height: usize,
    baseline: usize,
    left: bool,
    style: Option<&MathStyleBits>,
) -> Box_ {
    if delimiter.is_empty() {
        return blank(0, height, baseline);
    }
    if height <= 1 {
        return text_box(delimiter, style);
    }
    let glyphs = delimiter_glyphs(delimiter);
    let width = glyphs.iter().map(|glyph| cell_width(glyph)).max().unwrap_or(1);
    let mut result = blank(width, height, baseline);
    for y in 0..height {
        let glyph = if y == 0 {
            &glyphs[0]
        } else if y == height - 1 {
            &glyphs[2]
        } else {
            &glyphs[1]
        };
        set_cell(&mut result, 0, y, glyph, style);
    }
    if (delimiter == "{" || delimiter == "}") && height >= 3 {
        set_cell(&mut result, 0, floor_div(height, 2), if left { "⎨" } else { "⎬" }, style);
    }
    result
}

fn delimiter_glyphs(delimiter: &str) -> [String; 3] {
    let glyphs: [&str; 3] = match delimiter {
        "(" => ["⎛", "⎜", "⎝"],
        ")" => ["⎞", "⎟", "⎠"],
        "[" => ["⎡", "⎢", "⎣"],
        "]" => ["⎤", "⎥", "⎦"],
        "{" => ["⎧", "⎪", "⎩"],
        "}" => ["⎫", "⎪", "⎭"],
        "⌊" => ["│", "│", "⌊"],
        "⌋" => ["│", "│", "⌋"],
        "⌈" => ["⌈", "│", "│"],
        "⌉" => ["⌉", "│", "│"],
        "⟨" => ["/", "│", "\\"],
        "⟩" => ["\\", "│", "/"],
        _ => [delimiter, delimiter, delimiter],
    };
    glyphs.map(str::to_string)
}

fn matrix_delimiters(environment: MatrixEnvironment) -> Option<(&'static str, &'static str)> {
    match environment {
        MatrixEnvironment::Pmatrix => Some(("(", ")")),
        MatrixEnvironment::Bmatrix => Some(("[", "]")),
        MatrixEnvironment::BigBmatrix => Some(("{", "}")),
        MatrixEnvironment::Vmatrix => Some(("│", "│")),
        MatrixEnvironment::DoubleVmatrix => Some(("║", "║")),
        MatrixEnvironment::Cases => Some(("{", "")),
        _ => None,
    }
}

fn hpack(boxes: &[Box_]) -> Box_ {
    if boxes.is_empty() {
        return blank(0, 1, 0);
    }
    let ascent = boxes.iter().map(|box_| box_.baseline).max().unwrap_or(0);
    let descent =
        boxes.iter().map(|box_| box_.height.saturating_sub(box_.baseline + 1)).max().unwrap_or(0);
    let width = boxes.iter().map(|box_| box_.width).sum();
    let mut result = blank(width, ascent + descent + 1, ascent);
    let mut x = 0usize;
    for box_ in boxes {
        overlay(&mut result, box_, x, ascent - box_.baseline);
        x += box_.width;
    }
    result
}

fn text_box(text: &str, style: Option<&MathStyleBits>) -> Box_ {
    // Grapheme segmentation: single chars + combining-mark joins cover the
    // math glyph inventory (upstream uses Intl.Segmenter).
    let graphemes = split_graphemes(text);
    let width = graphemes.iter().map(|grapheme| cell_width(grapheme)).sum::<usize>();
    let mut result = blank(width, 1, 0);
    let mut x = 0usize;
    for grapheme in graphemes {
        set_cell(&mut result, x, 0, grapheme, style);
        x += cell_width(grapheme);
    }
    result
}

fn blank(width: usize, height: usize, baseline: usize) -> Box_ {
    Box_ { width, height: 1.max(height), baseline, cells: vec![vec![None; width]; 1.max(height)] }
}

fn overlay(target: &mut Box_, source: &Box_, x: usize, y: usize) {
    for source_y in 0..source.height {
        for source_x in 0..source.width {
            if let Some(cell) = &source.cells[source_y][source_x] {
                if y + source_y < target.height && x + source_x < target.width {
                    target.cells[y + source_y][x + source_x] = Some(cell.clone());
                }
            }
        }
    }
}

fn draw_horizontal(
    box_: &mut Box_,
    y: usize,
    x: usize,
    width: usize,
    char: &str,
    style: Option<&MathStyleBits>,
) {
    for offset in 0..width {
        set_cell(box_, x + offset, y, char, style);
    }
}

fn set_cell(box_: &mut Box_, x: usize, y: usize, char: &str, style: Option<&MathStyleBits>) {
    if x >= box_.width || y >= box_.height {
        return;
    }
    let style = style.as_ref().map(|bits| MathStyle {
        color: bits.color.clone(),
        bold: bits.bold,
        italic: bits.italic,
        dim: bits.dim,
    });
    box_.cells[y][x] = Some(MathCell { char: char.to_string(), style });
}

fn node_role(node: &MathNode) -> Option<SymbolRole> {
    match node {
        MathNode::Symbol { role, .. } => *role,
        MathNode::Operator { .. } => Some(SymbolRole::Operator),
        // Tall constructs need a terminal-cell side bearing.
        MathNode::Fraction { .. } | MathNode::Root { .. } | MathNode::Matrix { .. } => {
            Some(SymbolRole::Operator)
        }
        MathNode::Scripts { base, .. } => node_role(base),
        _ => None,
    }
}

fn needs_math_space(
    previous: Option<SymbolRole>,
    current: Option<SymbolRole>,
    count: usize,
) -> bool {
    use SymbolRole::{Binary, Closing, Opening, Operator, Punctuation, Relation};
    if count == 0 {
        return false;
    }
    if matches!(previous, Some(Punctuation | Opening))
        || matches!(current, Some(Punctuation | Closing))
    {
        return false;
    }
    matches!(previous, Some(Binary | Relation | Operator))
        || matches!(current, Some(Binary | Relation | Operator))
}

fn normalize_binary_role(
    role: Option<SymbolRole>,
    previous: Option<SymbolRole>,
    next: Option<SymbolRole>,
) -> Option<SymbolRole> {
    use SymbolRole::{Binary, Closing, Ordinary, Opening, Operator, Punctuation, Relation};
    if role != Some(Binary) {
        return role;
    }
    let blocking = |other: Option<SymbolRole>| {
        matches!(other, None | Some(Binary | Relation | Operator | Punctuation | Opening))
    };
    if blocking(previous) || matches!(next, None | Some(Binary | Relation | Punctuation | Closing))
    {
        return Some(Ordinary);
    }
    role
}

fn next_significant_role(nodes: &[MathNode], start: usize) -> Option<SymbolRole> {
    for node in nodes.iter().skip(start) {
        if matches!(node, MathNode::Space(_)) {
            continue;
        }
        return node_role(node).or(Some(SymbolRole::Ordinary));
    }
    None
}

fn simple_node_text(node: &MathNode) -> Option<String> {
    match node {
        MathNode::Symbol { value, .. }
        | MathNode::Text(value)
        | MathNode::Operator { value, .. } => Some(value.clone()),
        MathNode::Row(body) => {
            let mut text = String::new();
            for node in body {
                text.push_str(&simple_node_text(node)?);
            }
            Some(text)
        }
        _ => None,
    }
}

fn map_script(value: &str, table: fn(char) -> Option<&'static str>) -> Option<String> {
    let mut result = String::new();
    for char in value.chars() {
        result.push_str(table(char)?);
    }
    Some(result)
}

fn with_variant(context: &LayoutContext, variant: MathVariant) -> LayoutContext {
    let bits = match variant {
        MathVariant::Bold => MathStyleBits { bold: true, ..Default::default() },
        MathVariant::Italic => MathStyleBits { italic: true, ..Default::default() },
        _ => MathStyleBits::default(),
    };
    LayoutContext {
        variant: Some(variant),
        style: Some(context.style.clone().unwrap_or_default().merge(bits)),
        ..context.clone()
    }
}

fn apply_variant(value: &str, variant: Option<MathVariant>) -> String {
    let Some(variant) = variant else { return value.to_string() };
    if matches!(variant, MathVariant::Normal | MathVariant::Bold | MathVariant::Italic) {
        return value.to_string();
    }

    let exception = |char: char| -> Option<&'static str> {
        match variant {
            MathVariant::DoubleStruck => match char {
                'C' => Some("ℂ"),
                'H' => Some("ℍ"),
                'N' => Some("ℕ"),
                'P' => Some("ℙ"),
                'Q' => Some("ℚ"),
                'R' => Some("ℝ"),
                'Z' => Some("ℤ"),
                _ => None,
            },
            MathVariant::Script => match char {
                'B' => Some("ℬ"),
                'E' => Some("ℰ"),
                'F' => Some("ℱ"),
                'H' => Some("ℋ"),
                'I' => Some("ℐ"),
                'L' => Some("ℒ"),
                'M' => Some("ℳ"),
                'R' => Some("ℛ"),
                'e' => Some("ℯ"),
                'g' => Some("ℊ"),
                'o' => Some("ℴ"),
                _ => None,
            },
            MathVariant::Fraktur => match char {
                'C' => Some("ℭ"),
                'H' => Some("ℌ"),
                'I' => Some("ℑ"),
                'R' => Some("ℜ"),
                'Z' => Some("ℨ"),
                _ => None,
            },
            _ => None,
        }
    };
    let range: Option<(u32, u32, Option<u32>)> = match variant {
        MathVariant::DoubleStruck => Some((0x1d538, 0x1d552, Some(0x1d7d8))),
        MathVariant::Script => Some((0x1d49c, 0x1d4b6, None)),
        MathVariant::Fraktur => Some((0x1d504, 0x1d51e, None)),
        MathVariant::Sans => Some((0x1d5a0, 0x1d5ba, Some(0x1d7e2))),
        MathVariant::Monospace => Some((0x1d670, 0x1d68a, Some(0x1d7f6))),
        _ => None,
    };
    let Some((upper, lower, digits)) = range else { return value.to_string() };

    value
        .chars()
        .map(|char| {
            if let Some(exception) = exception(char) {
                return exception.to_string();
            }
            let code = char as u32;
            if (65..=90).contains(&code) {
                return char::from_u32(upper + code - 65).unwrap_or(char).to_string();
            }
            if (97..=122).contains(&code) {
                return char::from_u32(lower + code - 97).unwrap_or(char).to_string();
            }
            if let Some(digits) = digits {
                if (48..=57).contains(&code) {
                    return char::from_u32(digits + code - 48).unwrap_or(char).to_string();
                }
            }
            char.to_string()
        })
        .collect()
}

/// `cellWidth` — the upstream width table (0-width control/combining
/// chars, 2-width CJK/emoji ranges; a multi-char cluster measures by
/// its base code point like upstream's `codePointAt(0)` fallthrough).
#[must_use]
pub fn cell_width(value: &str) -> usize {
    let mut chars = value.chars();
    let Some(first) = chars.next() else { return 0 };
    let code = first as u32;
    let single = chars.next().is_none();
    if single
        && (code <= 0x1f
            || (0x7f..=0x9f).contains(&code)
            || (0x300..=0x36f).contains(&code)
            || (0xfe00..=0xfe0f).contains(&code))
    {
        return 0;
    }
    if code >= 0x1100
        && (code <= 0x115f
            || code == 0x2329
            || code == 0x232a
            || (0x2e80..=0xa4cf).contains(&code)
            || (0xac00..=0xd7a3).contains(&code)
            || (0xf900..=0xfaff).contains(&code)
            || (0xfe10..=0xfe6f).contains(&code)
            || (0xff00..=0xff60).contains(&code)
            || (0xffe0..=0xffe6).contains(&code)
            || (0x1f300..=0x1faff).contains(&code))
    {
        return 2;
    }
    1
}

fn as_public_layout(box_: Box_) -> MathLayout {
    MathLayout {
        width: box_.width,
        height: box_.height,
        baseline: box_.baseline,
        cells: box_.cells,
    }
}

/// Grapheme split: base char + trailing combining marks (upstream's
/// Intl.Segmenter behavior for the math glyph inventory).
fn split_graphemes(text: &str) -> Vec<&str> {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).collect()
}

/// `Math.floor(a / b)` for possibly-negative dividends.
pub(crate) fn floor_div(a: usize, b: usize) -> usize {
    a / b
}
